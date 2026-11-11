//! Edits of state machine diagrams: which state or region a vertex belongs to, following where
//! it is drawn, the regions of orthogonal states, and what sits on or in a submachine state.
//!
//! Entry and exit points sit on the border of the state they belong to, or of the diagram's
//! frame for those of the state machine itself, as Astah draws them; like ports
//! ([`crate::composite`]) they keep their place there when the state moves or changes size.
//! Stub states go inside a submachine state, which holds nothing else.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    ElementData, ElementKind, Id, InternalTransitionProps, Model, ModelError, Point,
    PseudostateKind, Rect, Regions, Result, Shape, Tx,
};

use crate::composite::{border_distance, snap};
use crate::palette::{NodeTool, owner_for, unique_name};
use crate::scene::{self, MIN_REGION};

/// Why a stub state could not be placed, as [`ModelError::Invalid`] says it; the app words it
/// in the language of its user.
pub const STUB_OUTSIDE: &str = "a stub state goes into a submachine state";

/// How close to a state's border a new entry or exit point must be dropped to go on it, when
/// not dropped inside the state.
const NEAR_STATE: f32 = 20.0;

/// Whether `state` is a submachine state.
pub fn is_submachine(model: &Model, state: Id) -> bool {
    model
        .get(state)
        .is_some_and(|e| matches!(e.kind(), ElementKind::State(p) if p.submachine.is_some()))
}

/// The state machine a new submachine state on `diagram` stands for: the first other state
/// machine of the project, else a new one with a diagram of its own, next to the diagram's.
pub fn submachine_for(tx: &mut Tx<'_>, diagram: Id) -> Result<Id> {
    let model = tx.model();
    let own = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
    let other = model.subtree(model.root()).into_iter().find(|&e| {
        e != own && model.get(e).is_some_and(|e| matches!(e.kind(), ElementKind::StateMachine))
    });
    if let Some(machine) = other {
        return Ok(machine);
    }
    let owner = model.element(own)?.owner().unwrap_or(model.root());
    let name = unique_name(model, owner, "StateMachine");
    let created =
        crate::edit::create_diagram(tx, owner, umlcraft_model::DiagramKind::StateMachine, &name)?;
    tx.model().element(created)?.owner().ok_or(ModelError::Root)
}

/// Whether `kind` is an entry or exit point.
fn is_border_point(kind: &ElementKind) -> bool {
    matches!(kind, ElementKind::Pseudostate(p) if p.pseudostate.is_border_point())
}

fn is_stub(kind: &ElementKind) -> bool {
    matches!(kind, ElementKind::Pseudostate(p) if p.pseudostate == PseudostateKind::StubState)
}

/// The state whose border an entry or exit point dropped with its center at `at` goes on: the
/// innermost one drawn around the point whose border is within reach, else the one whose
/// border is nearest, within reach.
fn border_state_at(model: &Model, diagram: Id, at: Point) -> Option<(Id, Id, Rect)> {
    model
        .view(diagram)?
        .presentations()
        .iter()
        .filter_map(|p| Some((p.id, p.element?, p.bounds()?)))
        .filter(|(_, e, _)| {
            model.get(*e).is_some_and(|e| matches!(e.kind(), ElementKind::State(_)))
        })
        .map(|(p, e, r)| (p, e, r, border_distance(r, at)))
        .filter(|(.., d)| *d <= NEAR_STATE)
        // Nearest border first; of two as near, the smaller state.
        .min_by(|a, b| {
            a.3.total_cmp(&b.3).then((a.2.width * a.2.height).total_cmp(&(b.2.width * b.2.height)))
        })
        .map(|(p, e, r, _)| (p, e, r))
}

/// Creates an entry point, an exit point or a stub state with `tool` at `origin` on the state
/// machine diagram `diagram`. A point goes on the border of the state nearest to where it is
/// dropped and belongs to it; dropped away from every state, it belongs to the state machine
/// and goes on the frame, which is added when the diagram has none. A stub state goes into the
/// submachine state it is dropped on (refused elsewhere, [`STUB_OUTSIDE`]). Returns (element,
/// presentation).
pub fn create_node(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    origin: Point,
    base_name: &str,
) -> Result<(Id, Id)> {
    let model = tx.model();
    let mut bounds = Rect::from_origin_size(origin, tool.default_size());
    let at = bounds.center();
    let kind = tool.element_kind();
    let owner = if is_stub(&kind) {
        let state = vertex_owner_at(model, diagram, at, &|s| !is_submachine(model, s))
            .filter(|&s| is_submachine(model, s))
            .ok_or_else(|| ModelError::Invalid(STUB_OUTSIDE.into()))?;
        // Within the state, below its texts.
        let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        if let Some(r) = view.presentations_of(state).next().and_then(|p| p.bounds()) {
            bounds.x = bounds.x.clamp(r.x, (r.right() - bounds.width).max(r.x));
            bounds.y = bounds.y.clamp(r.y, (r.bottom() - bounds.height).max(r.y));
        }
        state
    } else {
        match border_state_at(model, diagram, at) {
            Some((_, state, r)) => {
                bounds = snap(bounds, r);
                state
            }
            None => {
                let start = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
                let machine = owner_for(model, start, &kind)
                    .ok_or_else(|| ModelError::Invalid("no state machine for the point".into()))?;
                let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
                let framed = crate::activity::frame(view, diagram).and_then(|f| f.bounds());
                let r = match framed {
                    Some(r) => r,
                    None => crate::activity::add_frame(tx, diagram, at)?,
                };
                bounds = snap(bounds, r);
                machine
            }
        }
    };
    let name = unique_name(tx.model(), owner, base_name);
    let element = tx.create(owner, tool.element_data(name))?;
    let node = tx.add_node(diagram, Some(element), bounds)?;
    Ok((element, node))
}

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

/// What follows when the nodes `moved` (with their contents) have been moved on a state
/// machine `diagram`, by a drag in the app or by an AI client: each vertex becomes part of the
/// state or region now drawn around it ([`reparent_vertices`]); lines between a vertex that
/// changed its owner and one that stayed lose their bends, which were made for the old place
/// and would lead out of the new one and back; and a vertex is drawn after the state it now
/// belongs to, which would otherwise hide it.
pub fn after_move(tx: &mut Tx<'_>, diagram: Id, moved: &[Id]) -> Result<()> {
    let owners = |tx: &Tx<'_>| -> HashMap<Id, Option<Id>> {
        let model = tx.model();
        let view = model.view(diagram);
        moved
            .iter()
            .filter_map(|&p| Some((p, view?.get(p)?.element?)))
            .map(|(p, e)| (p, model.get(e).and_then(|e| e.owner())))
            .collect()
    };
    let before = owners(tx);
    reparent_vertices(tx, diagram, moved)?;
    grow_owners(tx, diagram, moved)?;
    let after = owners(tx);
    let changed: HashSet<Id> =
        after.iter().filter(|(p, o)| before.get(p) != Some(o)).map(|(p, _)| *p).collect();
    if changed.is_empty() {
        return Ok(());
    }
    let moving: HashSet<Id> = moved.iter().copied().collect();
    let straightened: Vec<Id> = tx
        .model()
        .view(diagram)
        .map(|v| {
            v.presentations()
                .iter()
                .filter(|p| match &p.shape {
                    Shape::Edge { source, target, waypoints, .. } => {
                        !waypoints.is_empty()
                            && moving.contains(source) != moving.contains(target)
                            && (changed.contains(source) || changed.contains(target))
                    }
                    _ => false,
                })
                .map(|p| p.id)
                .collect()
        })
        .unwrap_or_default();
    for edge in straightened {
        tx.set_waypoints(diagram, edge, Vec::new())?;
    }
    in_front_of_owners(tx, diagram, moved)
}

/// Room a state keeps around a substate it grows for.
const GROW_MARGIN: f32 = 20.0;

/// Grows the states that the vertices `presentations` lie in, innermost first, where a vertex
/// reaches out of one, so that each holds it with some room: a state dropped into a smaller
/// one, or moved over its state's border, stays inside.
pub fn grow_owners(tx: &mut Tx<'_>, diagram: Id, presentations: &[Id]) -> Result<()> {
    for &id in presentations {
        let model = tx.model();
        let Some(view) = model.view(diagram) else { break };
        let Some(p) = view.get(id) else { continue };
        let (Some(element), Some(mut inner)) = (p.element, p.bounds()) else { continue };
        // The loops from the vertex to itself stay inside too, with room above them for their
        // labels. Their waypoints may not have followed the move yet (`autoroute::follow`
        // moves them afterwards), so they are taken where they end up: at the vertex's
        // corners in turn.
        let loops = view
            .presentations()
            .iter()
            .filter(|q| {
                matches!(&q.shape, Shape::Edge { source, target, .. }
                    if *source == id && *target == id)
            })
            .count();
        let vertex = inner;
        for slot in 0..loops {
            for w in scene::self_loop_at(vertex, slot) {
                inner = inner.union(&Rect::new(w.x, w.y - scene::LABEL_HEIGHT, 0.0, 0.0));
                inner = inner.union(&Rect::new(w.x, w.y, 0.0, 0.0));
            }
        }
        let mut grown = Vec::new();
        for owner in model.ancestors(element) {
            if !matches!(model.get(owner).map(|e| e.kind()), Some(ElementKind::State(_))) {
                continue;
            }
            let Some((q, outer)) =
                view.presentations_of(owner).find_map(|q| Some((q.id, q.bounds()?)))
            else {
                continue;
            };
            if outer.contains_rect(&inner) {
                break;
            }
            let r = outer.union(&inner.inflate(GROW_MARGIN));
            grown.push((q, r));
            inner = r;
        }
        // Lines between regions stay in place (`Tx::set_bounds`).
        for (q, r) in grown {
            tx.set_bounds(diagram, q, r)?;
        }
    }
    Ok(())
}

/// Puts each of `presentations` that is drawn before a state it belongs to right after that
/// state, in the order they are drawn, so that contents move along behind their state's
/// substates.
fn in_front_of_owners(tx: &mut Tx<'_>, diagram: Id, presentations: &[Id]) -> Result<()> {
    let model = tx.model();
    let Some(view) = model.view(diagram) else { return Ok(()) };
    let at = |id: Id| view.presentations().iter().position(|p| p.id == id);
    let mut order: Vec<(usize, Id)> =
        presentations.iter().filter_map(|&p| Some((at(p)?, p))).collect();
    order.sort_unstable();
    for (_, p) in order {
        let model = tx.model();
        let Some(view) = model.view(diagram) else { break };
        let index = |id: Id| view.presentations().iter().position(|q| q.id == id);
        let Some(element) = view.get(p).and_then(|p| p.element) else { continue };
        let Some(own) = index(p) else { continue };
        // The last presentation, on this diagram, of a state this vertex lies in.
        let last_owner = model
            .ancestors(element)
            .into_iter()
            .filter_map(|a| view.presentations_of(a).next().and_then(|q| index(q.id)))
            .max();
        if let Some(owner) = last_owner.filter(|&o| o > own) {
            tx.apply(umlcraft_model::Op::ReorderPresentation { diagram, id: p, index: owner })?;
        }
    }
    Ok(())
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
        // Entry and exit points stay with the state on whose border they sit.
        if !e.kind().is_vertex() || is_border_point(e.kind()) {
            continue;
        }
        // Not into itself or what lies inside it; stub states go into submachine states only,
        // other vertices never.
        let stub = is_stub(e.kind());
        let inside = |candidate: Id| {
            candidate == element
                || model.is_ancestor(element, candidate)
                || stub != is_submachine(model, candidate)
        };
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
            .filter(|&c| {
                tx.model().get(c).is_some_and(|c| {
                    c.kind().is_vertex() && !is_border_point(c.kind()) && !is_stub(c.kind())
                })
            })
            .collect();
        for (index, vertex) in vertices.iter().enumerate() {
            tx.move_element(*vertex, created[0], index)?;
        }
    }
    for (diagram, presentation, layout) in lines {
        set_layout(tx, diagram, presentation, layout)?;
    }
    if existing.is_empty() {
        grow_for_regions(tx, &shown)?;
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

/// How deep and wide a state just divided makes its regions at least: room for a substate.
pub const NEW_REGION: f32 = 60.0;

/// Makes the state shown by each of `shown` (just divided into regions) large enough that
/// every region gets [`NEW_REGION`] below its name and activities, so that a small state does
/// not end up with regions squeezed into what is left below its text.
fn grow_for_regions(tx: &mut Tx<'_>, shown: &[(Id, Id)]) -> Result<()> {
    let model = tx.model();
    let mut grown = Vec::new();
    for &(diagram, presentation) in shown {
        let Some(p) = model.view(diagram).and_then(|v| v.get(presentation)) else { continue };
        let (Some(bounds), Some(layout)) = (p.bounds(), p.regions()) else { continue };
        let shapes = scene::state_regions(model, p);
        let Some(first) = shapes.first() else { continue };
        let n = shapes.len() as f32;
        let top = first.rect.y - bounds.y;
        let (width, height) = if layout.vertical {
            (bounds.width.max(n * NEW_REGION), bounds.height.max(top + NEW_REGION))
        } else {
            (bounds.width, bounds.height.max(top + n * NEW_REGION))
        };
        if (width, height) != (bounds.width, bounds.height) {
            let rect = umlcraft_model::Rect::new(bounds.x, bounds.y, width, height);
            grown.push((diagram, presentation, rect));
        }
    }
    for (diagram, presentation, rect) in grown {
        tx.set_bounds(diagram, presentation, rect)?;
    }
    Ok(())
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
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{Document, Rect};

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
    fn a_state_grown_to_the_left_keeps_its_region_line_in_place() {
        let (mut doc, _) = order_system();
        let model = doc.model();
        let (open, empty) = (named(model, "Open"), named(model, "Empty"));
        let (diagram, node) = node_of(model, open);
        doc.edit("divide", |tx| add_region(tx, open, true, None)).unwrap();
        doc.edit("line", |tx| move_region_line(tx, diagram, node, 0, 250.0)).unwrap();
        let bounds = |doc: &Document, id: Id| {
            doc.model().view(diagram).unwrap().get(id).unwrap().bounds().unwrap()
        };
        let line_x = |doc: &Document| {
            let p = doc.model().view(diagram).unwrap().get(node).unwrap();
            bounds(doc, node).x + p.regions().unwrap().separators[0]
        };
        let (before, state) = (line_x(&doc), bounds(&doc, node));
        // "Empty", in the left region, dragged half over the state's left border.
        let (_, empty_node) = node_of(doc.model(), empty);
        let moved =
            bounds(&doc, empty_node).translate(state.x - bounds(&doc, empty_node).x - 30.0, 0.0);
        doc.edit("move", |tx| {
            tx.set_bounds(diagram, empty_node, moved)?;
            after_move(tx, diagram, &[empty_node])
        })
        .unwrap();
        assert!(bounds(&doc, node).x < state.x, "the state grew to the left");
        assert_eq!(line_x(&doc), before);
        assert_eq!(doc.model().get(empty).unwrap().owner(), Some(regions_of(doc.model(), open)[0]));

        // Resized at its left edge, the state keeps the line where it is; moved, it takes it
        // along.
        let state = bounds(&doc, node);
        let wider = Rect::new(state.x - 40.0, state.y, state.width + 40.0, state.height);
        doc.edit("resize", |tx| tx.set_bounds(diagram, node, wider)).unwrap();
        assert_eq!(line_x(&doc), before);
        let moved = wider.translate(25.0, 0.0);
        doc.edit("move", |tx| tx.set_bounds(diagram, node, moved)).unwrap();
        assert_eq!(line_x(&doc), before + 25.0);
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
    fn a_small_state_grows_to_give_its_regions_room() {
        let (mut doc, ids) = order_system();
        let d = ids.state_diagram;
        let (small, node) = doc
            .edit("create", |tx| {
                let at = Point::new(600.0, 500.0);
                let (s, n) = create_node(tx, d, NodeTool::State, at, "Small")?;
                tx.update(s, |data| {
                    if let ElementKind::State(p) = &mut data.kind {
                        p.entry = "light on".into();
                        p.exit = "light off".into();
                    }
                })?;
                Ok((s, n))
            })
            .unwrap();
        let before = doc.model().view(d).unwrap().get(node).unwrap().bounds().unwrap();
        doc.edit("divide", |tx| add_region(tx, small, true, Some(d))).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let p = model.view(d).unwrap().get(node).unwrap();
        let after = p.bounds().unwrap();
        assert_eq!((after.x, after.y), (before.x, before.y), "it grows, it does not move");
        let shapes = scene::state_regions(model, p);
        assert_eq!(shapes.len(), 2);
        for s in &shapes {
            assert!(s.rect.height >= NEW_REGION, "{s:?} is deep enough");
            assert!(s.rect.width >= NEW_REGION, "{s:?} is wide enough");
            // Below the name and both activities (header 22, two rows of 16 and padding).
            assert!(s.rect.y >= after.y + 50.0, "{s:?} starts below the text");
        }
        // A large state keeps its size.
        doc.undo();
        let open = named(doc.model(), "Open");
        let (_, open_node) = node_of(doc.model(), open);
        let open_before = doc.model().view(d).unwrap().get(open_node).unwrap().bounds();
        doc.edit("divide", |tx| add_region(tx, open, false, Some(d))).unwrap();
        let open_after = doc.model().view(d).unwrap().get(open_node).unwrap().bounds();
        assert_eq!(open_before, open_after);
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

    fn create(
        doc: &mut umlcraft_model::Document,
        diagram: Id,
        tool: NodeTool,
        at: Point,
    ) -> Result<(Id, Id)> {
        crate::edit::apply(doc, "create", &crate::scene::ApproxMeasure, |tx| {
            create_node(tx, diagram, tool, at, tool.key())
        })
    }

    fn bounds(doc: &umlcraft_model::Document, diagram: Id, node: Id) -> Rect {
        doc.model().view(diagram).unwrap().get(node).unwrap().bounds().unwrap()
    }

    #[test]
    fn entry_and_exit_points_stay_on_their_state_border() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let (diagram, state_node) = node_of(doc.model(), shipped);
        let s = bounds(&doc, diagram, state_node);
        // Dropped near the left edge, the point goes onto it, centered on the border.
        let (entry, entry_node) =
            create(&mut doc, diagram, NodeTool::EntryPoint, Point::new(s.x - 15.0, s.y)).unwrap();
        assert_eq!(doc.model().element(entry).unwrap().owner(), Some(shipped));
        assert_eq!(bounds(&doc, diagram, entry_node).center().x, s.x);
        let (exit, exit_node) =
            create(&mut doc, diagram, NodeTool::ExitPoint, Point::new(s.right() - 10.0, s.y))
                .unwrap();
        assert_eq!(bounds(&doc, diagram, exit_node).center().x, s.right());
        // Moving and widening the state takes them along, each on its side.
        let moved = Rect::new(s.x + 40.0, s.y + 30.0, s.width + 60.0, s.height);
        crate::edit::apply(&mut doc, "move", &crate::scene::ApproxMeasure, |tx| {
            tx.set_bounds(diagram, state_node, moved)?;
            reparent_vertices(tx, diagram, &[state_node])
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, entry_node).center().x, moved.x);
        assert_eq!(bounds(&doc, diagram, exit_node).center().x, moved.right());
        assert_eq!(doc.model().element(entry).unwrap().owner(), Some(shipped), "not reparented");
        // A transition ends at the entry point and starts at the exit point.
        let (cancelled, cancelled_node) = {
            let c = named(doc.model(), "Cancelled");
            (c, node_of(doc.model(), c).1)
        };
        let _ = cancelled;
        crate::edit::apply(&mut doc, "transitions", &crate::scene::ApproxMeasure, |tx| {
            crate::palette::connect(
                tx,
                diagram,
                crate::palette::EdgeTool::Transition,
                cancelled_node,
                entry_node,
            )?;
            crate::palette::connect(
                tx,
                diagram,
                crate::palette::EdgeTool::Transition,
                exit_node,
                cancelled_node,
            )
        })
        .unwrap();
        // Deleting the state deletes its points.
        crate::edit::apply(&mut doc, "delete", &crate::scene::ApproxMeasure, |tx| {
            tx.delete(shipped)
        })
        .unwrap();
        assert!(doc.model().get(entry).is_none() && doc.model().get(exit).is_none());
        doc.model().check().unwrap();
    }

    #[test]
    fn points_away_from_states_go_on_the_frame() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let (diagram, _) = node_of(doc.model(), shipped);
        let machine = doc.model().element(diagram).unwrap().owner();
        let (point, node) =
            create(&mut doc, diagram, NodeTool::ExitPoint, Point::new(1500.0, 900.0)).unwrap();
        assert_eq!(doc.model().element(point).unwrap().owner(), machine);
        let view = doc.model().view(diagram).unwrap();
        let frame = crate::activity::frame(view, diagram).expect("a frame was added");
        let r = frame.bounds().unwrap();
        assert!(border_distance(r, bounds(&doc, diagram, node).center()) < 0.5);
    }

    #[test]
    fn submachine_states_hold_stub_states_only() {
        let (mut doc, _) = order_system();
        let shipped = named(doc.model(), "Shipped");
        let (diagram, _) = node_of(doc.model(), shipped);
        let own = doc.model().element(diagram).unwrap().owner().unwrap();
        // A stub state needs a submachine state.
        assert!(create(&mut doc, diagram, NodeTool::StubState, Point::new(1500.0, 900.0)).is_err());
        let s = bounds(&doc, diagram, node_of(doc.model(), shipped).1);
        let refused = create(&mut doc, diagram, NodeTool::StubState, s.center());
        assert!(matches!(refused, Err(ModelError::Invalid(m)) if m == STUB_OUTSIDE));
        // The sample has one state machine, so the submachine state brings a new one along.
        let machines = |doc: &umlcraft_model::Document| {
            doc.model().elements().filter(|e| matches!(e.kind(), ElementKind::StateMachine)).count()
        };
        assert_eq!(machines(&doc), 1);
        let (sub, sub_node) =
            create(&mut doc, diagram, NodeTool::SubmachineState, Point::new(900.0, 600.0)).unwrap();
        assert_eq!(machines(&doc), 2);
        let submachine = |doc: &umlcraft_model::Document, state: Id| match doc.model().get(state) {
            Some(e) => match e.kind() {
                ElementKind::State(p) => p.submachine,
                _ => None,
            },
            None => None,
        };
        let machine = submachine(&doc, sub);
        assert!(machine.is_some_and(|m| m != own));
        assert!(is_submachine(doc.model(), sub));
        let r = bounds(&doc, diagram, sub_node);
        let (stub, stub_node) = create(
            &mut doc,
            diagram,
            NodeTool::StubState,
            Point::new(r.x + 5.0, r.bottom() - 30.0),
        )
        .unwrap();
        assert_eq!(doc.model().element(stub).unwrap().owner(), Some(sub));
        assert!(r.contains_rect(&bounds(&doc, diagram, stub_node)), "clamped inside");
        // No transition runs between two stub states.
        let (_, other_stub) = create(
            &mut doc,
            diagram,
            NodeTool::StubState,
            Point::new(r.right() - 65.0, r.bottom() - 30.0),
        )
        .unwrap();
        let between = crate::edit::apply(&mut doc, "t", &crate::scene::ApproxMeasure, |tx| {
            crate::palette::connect(
                tx,
                diagram,
                crate::palette::EdgeTool::Transition,
                stub_node,
                other_stub,
            )
        });
        assert!(between.is_err());
        // An ordinary state dropped onto it stays outside, and the stub stays in it.
        let (_, shipped_node) = node_of(doc.model(), shipped);
        crate::edit::apply(&mut doc, "drop", &crate::scene::ApproxMeasure, |tx| {
            tx.set_bounds(diagram, shipped_node, Rect::new(r.x + 10.0, r.y + 5.0, 40.0, 30.0))?;
            reparent_vertices(tx, diagram, &[shipped_node, stub_node])
        })
        .unwrap();
        assert_eq!(doc.model().element(shipped).unwrap().owner(), Some(own));
        assert_eq!(doc.model().element(stub).unwrap().owner(), Some(sub));
        // A second submachine state stands for the machine there is now.
        let (other, _) =
            create(&mut doc, diagram, NodeTool::SubmachineState, Point::new(900.0, 800.0)).unwrap();
        assert_eq!(machines(&doc), 2);
        assert_eq!(submachine(&doc, other), machine);
        doc.model().check().unwrap();
    }
}
