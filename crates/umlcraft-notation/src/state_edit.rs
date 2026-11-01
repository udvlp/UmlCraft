//! Edits of state machine diagrams: which state or region a vertex belongs to, following where
//! it is drawn, and the regions of orthogonal states.

use umlcraft_model::{
    ElementData, ElementKind, Id, InternalTransitionProps, Model, Point, Regions, Result, Shape, Tx,
};

use crate::scene::{self, MIN_REGION};

/// What a vertex placed at `p` on `diagram` becomes part of: the innermost state drawn around
/// `p`, or for an orthogonal state the region `p` lies in (the first one, outside them all).
/// `None` outside every state. States for which `excluded` holds are passed over.
pub fn vertex_owner_at(
    model: &Model,
    diagram: Id,
    p: Point,
    excluded: &dyn Fn(Id) -> bool,
) -> Option<Id> {
    let (presentation, state) = model
        .view(diagram)?
        .presentations()
        .iter()
        .filter_map(|pr| Some((pr, pr.element?, pr.bounds()?)))
        .filter(|(_, e, b)| {
            b.contains(p)
                && !excluded(*e)
                && model.get(*e).is_some_and(|e| matches!(e.kind(), ElementKind::State(_)))
        })
        .min_by(|(_, _, a), (_, _, b)| (a.width * a.height).total_cmp(&(b.width * b.height)))
        .map(|(pr, e, _)| (pr, e))?;
    let regions = scene::state_regions(model, presentation);
    if let Some(region) = regions.iter().find(|r| r.rect.contains(p)) {
        return Some(region.region);
    }
    // A state with regions keeps its substates in them, even where it shows none.
    Some(regions_of(model, state).first().copied().unwrap_or(state))
}

/// The regions of `state`, in order.
pub fn regions_of(model: &Model, state: Id) -> Vec<Id> {
    model
        .children(state)
        .iter()
        .copied()
        .filter(|&c| model.get(c).is_some_and(|c| matches!(c.kind(), ElementKind::Region)))
        .collect()
}

/// Gives the vertices shown by `presentations` on `diagram` the owner their place says: the
/// state or region drawn around their center, else the state machine. Used after moving them.
/// One after the other, so that two vertices dropped onto each other nest one way, not both.
pub fn reparent_vertices(tx: &mut Tx<'_>, diagram: Id, presentations: &[Id]) -> Result<()> {
    let Some(machine) = tx.model().element(diagram)?.owner() else { return Ok(()) };
    for &id in presentations {
        let model = tx.model();
        let Some(p) = model.view(diagram).and_then(|v| v.get(id)) else { continue };
        let (Some(element), Some(bounds)) = (p.element, p.bounds()) else { continue };
        let Some(e) = model.get(element) else { continue };
        if !e.kind().is_vertex() {
            continue;
        }
        // Not into itself or what lies inside it.
        let inside = |candidate: Id| candidate == element || model.is_ancestor(element, candidate);
        let owner = vertex_owner_at(model, diagram, bounds.center(), &inside).unwrap_or(machine);
        if e.owner() != Some(owner) && model.get(owner).is_some_and(|o| o.kind().can_own(e.kind()))
        {
            let index = model.children(owner).len();
            tx.move_element(element, owner, index)?;
        }
    }
    Ok(())
}

/// Divides `state` into one more region and returns it. A state without regions gets two, laid
/// out as `vertical` says (side by side) on every diagram that shows it, and its substates go
/// into the one they are drawn in on `diagram`, the one the user divides it on (else the first
/// diagram that shows it): a substate is in one region, however other diagrams draw it.
/// Further regions take half of the last one.
pub fn add_region(tx: &mut Tx<'_>, state: Id, vertical: bool, diagram: Option<Id>) -> Result<Id> {
    let model = tx.model();
    let existing = regions_of(model, state);
    let shown = model.shown_on(state);
    // Where the lines go once there is one more region, per presentation.
    let mut lines = Vec::new();
    for &(diagram, presentation) in &shown {
        let Some(p) = model.view(diagram).and_then(|v| v.get(presentation)) else { continue };
        let Some(bounds) = p.bounds() else { continue };
        let layout = p.regions().cloned().unwrap_or(Regions { vertical, separators: Vec::new() });
        let shapes = scene::state_regions(model, p);
        let mut separators = scene::region_separators(bounds, &shapes, layout.vertical);
        // The new line halves the last region. The first two regions share the room evenly
        // until a line is moved.
        if let Some(last) = shapes.last() {
            let (start, extent) = if layout.vertical {
                (last.rect.x - bounds.x, bounds.width)
            } else {
                (last.rect.y - bounds.y, bounds.height)
            };
            let line = start + ((extent - start) / 2.0).max(MIN_REGION);
            separators.push(line.min(extent - MIN_REGION));
        }
        lines.push((diagram, presentation, Regions { vertical: layout.vertical, separators }));
    }
    let mut created = Vec::new();
    for _ in 0..if existing.is_empty() { 2 } else { 1 } {
        created.push(tx.create(state, ElementData::new("", ElementKind::Region))?);
    }
    // The substates go into the first region, then to where they are drawn.
    if existing.is_empty() {
        let vertices: Vec<Id> = tx
            .model()
            .children(state)
            .iter()
            .copied()
            .filter(|&c| tx.model().get(c).is_some_and(|c| c.kind().is_vertex()))
            .collect();
        for (index, vertex) in vertices.iter().enumerate() {
            tx.move_element(*vertex, created[0], index)?;
        }
    }
    for (diagram, presentation, layout) in lines {
        set_layout(tx, diagram, presentation, layout)?;
    }
    if existing.is_empty() {
        let decides = diagram
            .filter(|d| shown.iter().any(|(s, _)| s == d))
            .or(shown.first().map(|(d, _)| *d));
        if let Some(diagram) = decides {
            redistribute(tx, diagram, state)?;
        }
    }
    Ok(*created.last().expect("at least one region"))
}

/// Deletes `region` with what it holds; the line before it (after it, for the first region)
/// goes with it, so its neighbor takes its room.
pub fn delete_region(tx: &mut Tx<'_>, region: Id) -> Result<()> {
    let model = tx.model();
    let Some(state) = model.element(region)?.owner() else { return Ok(()) };
    let index = regions_of(model, state).iter().position(|&r| r == region).unwrap_or(0);
    let mut layouts = Vec::new();
    for (diagram, presentation) in model.shown_on(state) {
        let Some(p) = model.view(diagram).and_then(|v| v.get(presentation)) else { continue };
        let (Some(layout), Some(bounds)) = (p.regions(), p.bounds()) else { continue };
        let shapes = scene::state_regions(model, p);
        let mut separators = scene::region_separators(bounds, &shapes, layout.vertical);
        if !separators.is_empty() {
            separators.remove(index.saturating_sub(1).min(separators.len() - 1));
        }
        layouts.push((diagram, presentation, Regions { separators, ..layout.clone() }));
    }
    tx.delete(region)?;
    for (diagram, presentation, layout) in layouts {
        set_layout(tx, diagram, presentation, layout)?;
    }
    Ok(())
}

/// Moves the line before region `index` + 1 of the orthogonal state shown by `presentation`
/// to `offset` from the state's top (or left) edge, and puts the substates drawn on the other
/// side into the region they now lie in.
pub fn move_region_line(
    tx: &mut Tx<'_>,
    diagram: Id,
    presentation: Id,
    index: usize,
    offset: f32,
) -> Result<()> {
    let model = tx.model();
    let Some(p) = model.view(diagram).and_then(|v| v.get(presentation)) else { return Ok(()) };
    let (Some(state), Some(bounds)) = (p.element, p.bounds()) else { return Ok(()) };
    let layout = p.regions().cloned().unwrap_or_default();
    let shapes = scene::state_regions(model, p);
    let mut separators = scene::region_separators(bounds, &shapes, layout.vertical);
    let Some(line) = separators.get_mut(index) else { return Ok(()) };
    // Kept between its neighbors as the drawing keeps it, so what is stored is what is seen.
    let origin = if layout.vertical { bounds.x } else { bounds.y };
    let start = |r: umlcraft_model::Rect| if layout.vertical { r.x } else { r.y } - origin;
    let end =
        |r: umlcraft_model::Rect| if layout.vertical { r.right() } else { r.bottom() } - origin;
    let offset = match (shapes.get(index), shapes.get(index + 1)) {
        (Some(before), Some(after)) => {
            let (low, high) = (start(before.rect) + MIN_REGION, end(after.rect) - MIN_REGION);
            if low <= high { offset.clamp(low, high) } else { offset }
        }
        _ => offset,
    };
    *line = offset;
    set_layout(tx, diagram, presentation, Regions { separators, ..layout })?;
    redistribute(tx, diagram, state)
}

/// A new internal transition of `state`, triggered by `trigger`.
pub fn add_internal_transition(tx: &mut Tx<'_>, state: Id, trigger: &str) -> Result<Id> {
    let props = InternalTransitionProps { trigger: trigger.to_owned(), ..Default::default() };
    tx.create(state, ElementData::new("", ElementKind::InternalTransition(props)))
}

fn set_layout(tx: &mut Tx<'_>, diagram: Id, presentation: Id, layout: Regions) -> Result<()> {
    tx.update_presentation(diagram, presentation, |p| {
        if let Shape::Node { regions, .. } = &mut p.shape {
            *regions = Some(layout);
        }
    })
}

/// Puts the vertices inside `state` that `diagram` shows into the region they are drawn in.
fn redistribute(tx: &mut Tx<'_>, diagram: Id, state: Id) -> Result<()> {
    let model = tx.model();
    let Some(view) = model.view(diagram) else { return Ok(()) };
    let inside: Vec<Id> = view
        .presentations()
        .iter()
        .filter(|p| p.element.is_some_and(|e| model.is_ancestor(state, e)))
        .map(|p| p.id)
        .collect();
    reparent_vertices(tx, diagram, &inside)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{NodeTool, create_node};
    use umlcraft_model::Rect;
    use umlcraft_model::sample::order_system;

    fn named(model: &Model, name: &str) -> Id {
        model.elements().find(|e| e.name() == name).map(|e| e.id()).expect("in the sample")
    }

    fn node_of(model: &Model, element: Id) -> (Id, Id) {
        model.shown_on(element)[0]
    }

    fn owner_name(model: &Model, element: Id) -> String {
        let owner = model.get(element).and_then(|e| e.owner()).expect("owned");
        model.get(owner).map(|o| o.name().to_owned()).unwrap_or_default()
    }

    #[test]
    fn two_states_dropped_onto_each_other_nest_one_way() {
        let (mut doc, _) = order_system();
        let (shipped, empty) = (named(doc.model(), "Shipped"), named(doc.model(), "Empty"));
        let (diagram, shipped_node) = node_of(doc.model(), shipped);
        let (_, empty_node) = node_of(doc.model(), empty);
        let machine = doc.model().element(diagram).unwrap().owner().unwrap();
        // "Empty" leaves "Open" for the very place of "Shipped", outside every other state.
        let place = doc.model().view(diagram).unwrap().get(shipped_node).unwrap().bounds().unwrap();
        doc.edit("drop", |tx| {
            tx.set_bounds(diagram, empty_node, place)?;
            reparent_vertices(tx, diagram, &[shipped_node, empty_node])
        })
        .unwrap();
        let model = doc.model();
        assert_eq!(owner_name(model, shipped), "Empty");
        assert_eq!(model.get(empty).unwrap().owner(), Some(machine), "not left in Open");
    }

    #[test]
    fn states_move_into_and_out_of_composite_states() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let (diagram, node) = node_of(doc.model(), shipped);
        // Below "Empty" and "Filled", inside "Open".
        doc.edit("in", |tx| {
            tx.set_bounds(diagram, node, Rect::new(250.0, 180.0, 100.0, 40.0))?;
            reparent_vertices(tx, diagram, &[node])
        })
        .unwrap();
        assert_eq!(owner_name(doc.model(), shipped), "Open");
        doc.model().check().unwrap();
        doc.edit("out", |tx| {
            tx.set_bounds(diagram, node, Rect::new(680.0, 60.0, 100.0, 40.0))?;
            reparent_vertices(tx, diagram, &[node])
        })
        .unwrap();
        assert_eq!(owner_name(doc.model(), shipped), "Order lifecycle", "back in the machine");
        // A composite state moved with its substates keeps them.
        let open = named(doc.model(), "Open");
        let moved: Vec<Id> = doc
            .model()
            .view(diagram)
            .unwrap()
            .presentations()
            .iter()
            .filter(|p| p.element.is_some_and(|e| e == open || doc.model().is_ancestor(open, e)))
            .filter(|p| !p.is_edge())
            .map(|p| p.id)
            .collect();
        doc.edit("move", |tx| {
            for &p in &moved {
                let b = tx.model().view(diagram).unwrap().get(p).unwrap().bounds().unwrap();
                tx.set_bounds(diagram, p, b.translate(0.0, 300.0))?;
            }
            reparent_vertices(tx, diagram, &moved)
        })
        .unwrap();
        assert_eq!(owner_name(doc.model(), named(doc.model(), "Empty")), "Open");
        assert_eq!(owner_name(doc.model(), open), "Order lifecycle");
    }

    #[test]
    fn a_state_does_not_go_into_its_own_substate() {
        let (mut doc, _) = order_system();
        let open = named(doc.model(), "Open");
        let (diagram, node) = node_of(doc.model(), open);
        // Its center lands in "Empty", which lies inside it.
        doc.edit("onto", |tx| {
            tx.set_bounds(diagram, node, Rect::new(50.0, 55.0, 300.0, 170.0))?;
            reparent_vertices(tx, diagram, &[node])
        })
        .unwrap();
        assert_eq!(owner_name(doc.model(), open), "Order lifecycle");
        doc.model().check().unwrap();
    }

    #[test]
    fn regions_divide_a_state_and_take_its_substates() {
        let (mut doc, _) = order_system();
        let model = doc.model();
        let (open, empty, filled) =
            (named(model, "Open"), named(model, "Empty"), named(model, "Filled"));
        let (diagram, node) = node_of(model, open);
        // Side by side: the line halves the state, between "Empty" and "Filled".
        let second = doc.edit("divide", |tx| add_region(tx, open, true, None)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let regions = regions_of(model, open);
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[1], second);
        assert_eq!(model.get(empty).unwrap().owner(), Some(regions[0]));
        assert_eq!(model.get(filled).unwrap().owner(), Some(regions[1]));
        let p = model.view(diagram).unwrap().get(node).unwrap();
        assert!(p.regions().is_some_and(|r| r.vertical && r.separators.is_empty()));
        let shapes = scene::state_regions(model, p);
        assert_eq!(shapes[1].rect.x, 250.0, "evenly");

        // Moving the line (250 from the left edge) past "Filled" takes it into the first region.
        doc.edit("line", |tx| move_region_line(tx, diagram, node, 0, 250.0)).unwrap();
        let model = doc.model();
        assert_eq!(model.get(filled).unwrap().owner(), Some(regions[0]));
        let p = model.view(diagram).unwrap().get(node).unwrap();
        assert_eq!(p.regions().unwrap().separators, vec![250.0]);

        // A third region halves the last one.
        let third = doc.edit("third", |tx| add_region(tx, open, false, None)).unwrap();
        let model = doc.model();
        let p = model.view(diagram).unwrap().get(node).unwrap();
        assert_eq!(p.regions().unwrap().separators, vec![250.0, 275.0]);
        assert!(p.regions().unwrap().vertical, "later regions keep the direction");

        // Deleting the middle region gives its room to the first one.
        doc.edit("delete", |tx| delete_region(tx, regions[1])).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(regions_of(model, open), vec![regions[0], third]);
        let p = model.view(diagram).unwrap().get(node).unwrap();
        assert_eq!(p.regions().unwrap().separators, vec![275.0]);
        assert!(model.contains(filled), "it was in the first region");

        while doc.undo() {}
        assert!(regions_of(doc.model(), open).is_empty());
        assert_eq!(doc.model().get(empty).unwrap().owner(), Some(open));
    }

    #[test]
    fn the_diagram_divided_on_decides_where_substates_go() {
        let (mut doc, ids) = order_system();
        let model = doc.model();
        let (open, empty, filled) =
            (named(model, "Open"), named(model, "Empty"), named(model, "Filled"));
        let machine = model.get(ids.state_diagram).unwrap().owner().unwrap();
        // A second diagram draws "Empty" and "Filled" the other way round.
        let second = doc
            .edit("second", |tx| {
                let kind = ElementKind::diagram(umlcraft_model::DiagramKind::StateMachine);
                let d = tx.create(machine, ElementData::new("other", kind))?;
                tx.add_node(d, Some(open), Rect::new(100.0, 60.0, 300.0, 170.0))?;
                tx.add_node(d, Some(empty), Rect::new(290.0, 120.0, 80.0, 40.0))?;
                tx.add_node(d, Some(filled), Rect::new(160.0, 120.0, 80.0, 40.0))?;
                Ok(d)
            })
            .unwrap();
        // Divided on the first one: its drawing counts, not the one shown last.
        let first = ids.state_diagram;
        assert_eq!(doc.model().shown_on(open).last().map(|(d, _)| *d), Some(second));
        doc.edit("divide", |tx| add_region(tx, open, true, Some(first))).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let regions = regions_of(model, open);
        assert_eq!(model.get(empty).unwrap().owner(), Some(regions[0]));
        assert_eq!(model.get(filled).unwrap().owner(), Some(regions[1]));
        // Undone and divided on the second one, it is the other way round.
        doc.undo();
        doc.edit("divide", |tx| add_region(tx, open, true, Some(second))).unwrap();
        let model = doc.model();
        let regions = regions_of(model, open);
        assert_eq!(model.get(filled).unwrap().owner(), Some(regions[0]));
        assert_eq!(model.get(empty).unwrap().owner(), Some(regions[1]));
    }

    #[test]
    fn new_states_land_in_the_region_clicked() {
        let (mut doc, ids) = order_system();
        let open = named(doc.model(), "Open");
        doc.edit("divide", |tx| add_region(tx, open, false, None)).unwrap();
        let regions = regions_of(doc.model(), open);
        // One below the other: the line lies halfway below the name, at about 156.
        let d = ids.state_diagram;
        let (upper, lower) = doc
            .edit("create", |tx| {
                let (a, _) = create_node(tx, d, NodeTool::State, Point::new(120.0, 90.0), "A")?;
                let (b, _) = create_node(tx, d, NodeTool::State, Point::new(120.0, 200.0), "B")?;
                Ok((a, b))
            })
            .unwrap();
        let model = doc.model();
        assert_eq!(model.get(upper).unwrap().owner(), Some(regions[0]));
        assert_eq!(model.get(lower).unwrap().owner(), Some(regions[1]));
        model.check().unwrap();
    }

    #[test]
    fn internal_transitions_belong_to_their_state() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let t = doc.edit("add", |tx| add_internal_transition(tx, shipped, "track")).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(model.get(t).unwrap().owner(), Some(shipped));
        let ElementKind::InternalTransition(p) = model.get(t).unwrap().kind() else { panic!() };
        assert_eq!(p.label(), "track");
    }

    #[test]
    fn every_region_of_a_small_state_keeps_its_room() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let (diagram, node) = node_of(doc.model(), shipped);
        doc.edit("small", |tx| {
            tx.set_bounds(diagram, node, Rect::new(680.0, 60.0, 100.0, 40.0))?;
            for _ in 0..4 {
                add_region(tx, shipped, false, None)?;
            }
            Ok(())
        })
        .unwrap();
        let model = doc.model();
        let p = model.view(diagram).unwrap().get(node).unwrap();
        let shapes = scene::state_regions(model, p);
        assert_eq!(shapes.len(), 5);
        for s in &shapes {
            assert!(s.rect.height >= MIN_REGION - 0.01 && s.rect.y >= 60.0, "{:?}", s.rect);
        }
        // A line dragged past its neighbor stays where it is drawn.
        doc.edit("drag", |tx| move_region_line(tx, diagram, node, 1, -50.0)).unwrap();
        let model = doc.model();
        let p = model.view(diagram).unwrap().get(node).unwrap();
        let drawn =
            scene::region_separators(p.bounds().unwrap(), &scene::state_regions(model, p), false);
        assert_eq!(p.regions().unwrap().separators[1], drawn[1]);
    }

    #[test]
    fn states_moved_into_each_other_close_no_cycle() {
        let (mut doc, _) = order_system();
        let (shipped, cancelled) = (named(doc.model(), "Shipped"), named(doc.model(), "Cancelled"));
        let (diagram, a) = node_of(doc.model(), shipped);
        let (_, b) = node_of(doc.model(), cancelled);
        // Each center lies inside the other state.
        doc.edit("overlap", |tx| {
            tx.set_bounds(diagram, a, Rect::new(100.0, 300.0, 200.0, 100.0))?;
            tx.set_bounds(diagram, b, Rect::new(150.0, 320.0, 200.0, 100.0))?;
            reparent_vertices(tx, diagram, &[a, b])
        })
        .unwrap();
        let model = doc.model();
        let owners = [shipped, cancelled].map(|s| model.get(s).unwrap().owner().unwrap());
        // One goes into the other, the other stays where it is.
        assert!((owners[0] == cancelled) != (owners[1] == shipped), "{owners:?}");
        model.check().unwrap();
    }
}
