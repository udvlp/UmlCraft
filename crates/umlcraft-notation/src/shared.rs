//! Astah's shared style of generalizations: the generalizations that end at one node share
//! one triangle, their lines joining on a common line. In the vertical style the lines leave
//! the general element straight up or down to a horizontal line and drop from it to each
//! specific element; the horizontal style turns this on its side.
//!
//! Each edge of a group keeps the common line in its waypoints, `[(specific, at), (general,
//! at)]` in the vertical style, where `at` is where the common line runs. The drawing takes
//! `at` from the first edge of the group with waypoints and the rest from the nodes as drawn,
//! so lines follow their nodes; [`set_style`] and [`move_common_line`] keep the waypoints of
//! the whole group alike.

use std::collections::HashMap;

use umlcraft_model::{
    ElementKind, Id, Model, Point, Presentation, Rect, Result, Shape, SharedStyle, Tx, View,
};

/// Astah's least distance of the common line from the general element.
const MIN_GAP: f32 = 15.0;

/// The node of the general element and the node of the specific one, for a generalization
/// edge of `view`.
pub fn ends(model: &Model, view: &View, edge: &Presentation) -> Option<(Id, Id)> {
    let Shape::Edge { source, target, .. } = &edge.shape else { return None };
    let ElementKind::Generalization(g) = model.get(edge.element?)?.kind() else { return None };
    let source_element = view.get(*source)?.element;
    if source_element == Some(g.general) && view.get(*target)?.element != Some(g.general) {
        Some((*source, *target))
    } else {
        Some((*target, *source))
    }
}

/// The generalization edges of `view` that end at the general node `general`.
fn generalizations_to(model: &Model, view: &View, general: Id) -> Vec<Id> {
    view.presentations()
        .iter()
        .filter(|p| ends(model, view, p).is_some_and(|(g, _)| g == general))
        .map(|p| p.id)
        .collect()
}

/// Where Astah puts the common line: halfway between the general element and the nearest
/// specific one, at least 15 away from the general element, on the side the specific elements
/// are on (below or right of it unless all lie above or left).
pub fn default_line(general: Rect, specifics: &[Rect], style: SharedStyle) -> f32 {
    let vertical = style != SharedStyle::Horizontal;
    let (near, far) =
        if vertical { (general.bottom(), general.y) } else { (general.right(), general.x) };
    let start = |r: &Rect| if vertical { r.y } else { r.x };
    let end = |r: &Rect| if vertical { r.bottom() } else { r.right() };
    let beyond = specifics.iter().all(|r| end(r) <= far) && !specifics.is_empty();
    if beyond {
        let closest = specifics.iter().map(end).fold(f32::MIN, f32::max);
        far - ((far - closest) / 2.0).max(MIN_GAP)
    } else {
        let closest = specifics.iter().map(start).filter(|&s| s >= near).fold(f32::MAX, f32::min);
        let gap = if closest == f32::MAX { 0.0 } else { (closest - near) / 2.0 };
        near + gap.max(MIN_GAP)
    }
}

/// The waypoints of a shared line from `specific` (where the line aims inside it) to `general`
/// with the common line at `at`.
pub fn waypoints(specific: Point, general: Point, at: f32, style: SharedStyle) -> Vec<Point> {
    match style {
        SharedStyle::Horizontal => vec![Point::new(at, specific.y), Point::new(at, general.y)],
        _ => vec![Point::new(specific.x, at), Point::new(general.x, at)],
    }
}

/// Where the common line runs, per general node and style, for the drawing: from the first
/// edge of each group with waypoints, else where Astah puts it.
pub fn common_lines(
    model: &Model,
    view: &View,
    rects: &HashMap<Id, Rect>,
) -> HashMap<(Id, SharedStyle), f32> {
    let mut lines = HashMap::new();
    let mut specifics: HashMap<(Id, SharedStyle), Vec<Rect>> = HashMap::new();
    for p in view.presentations().iter().filter(|p| !p.style.shared.is_separate()) {
        let Some((general, specific)) = ends(model, view, p) else { continue };
        let key = (general, p.style.shared);
        if let Some(r) = rects.get(&specific) {
            specifics.entry(key).or_default().push(*r);
        }
        if let Shape::Edge { waypoints, .. } = &p.shape
            && let Some(w) = waypoints.first()
        {
            let at = if p.style.shared == SharedStyle::Horizontal { w.x } else { w.y };
            lines.entry(key).or_insert(at);
        }
    }
    for (key, rects_of) in specifics {
        if let Some(general) = rects.get(&key.0) {
            lines.entry(key).or_insert_with(|| default_line(*general, &rects_of, key.1));
        }
    }
    lines
}

/// Gives all generalizations that end where `edge` ends the shared `style` (or separate lines
/// again): Astah's "Shared Style" for the generalizations to one element. Their common line goes
/// where Astah puts it; separate lines lose their waypoints. The style counts as chosen:
/// arranging the diagram keeps separate lines apart.
pub fn set_style(tx: &mut Tx<'_>, diagram: Id, edge: Id, style: SharedStyle) -> Result<()> {
    apply_style(tx, diagram, edge, style, true)
}

/// [`set_style`] as arranging the diagram gives it, not chosen.
pub(crate) fn arrange_style(
    tx: &mut Tx<'_>,
    diagram: Id,
    edge: Id,
    style: SharedStyle,
) -> Result<()> {
    apply_style(tx, diagram, edge, style, false)
}

fn apply_style(
    tx: &mut Tx<'_>,
    diagram: Id,
    edge: Id,
    style: SharedStyle,
    chosen: bool,
) -> Result<()> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(umlcraft_model::ModelError::NotADiagram(diagram))?;
    let presentation = view.get(edge).ok_or(umlcraft_model::ModelError::NotFound(edge))?;
    let Some((general, _)) = ends(model, view, presentation) else {
        return Err(umlcraft_model::ModelError::Invalid(format!("{edge} is no generalization")));
    };
    let group = generalizations_to(model, view, general);
    let bounds = |id: Id| view.get(id).and_then(|p| p.bounds());
    let general_rect = bounds(general).unwrap_or_default();
    let mut plans = Vec::new();
    let specific_rects: Vec<Rect> = group
        .iter()
        .filter_map(|&e| ends(model, view, view.get(e)?))
        .filter_map(|(_, s)| bounds(s))
        .collect();
    let at = default_line(general_rect, &specific_rects, style);
    for &e in &group {
        let Some((_, specific)) = view.get(e).and_then(|p| ends(model, view, p)) else { continue };
        let points = match style {
            SharedStyle::Separate => Vec::new(),
            _ => {
                let s = bounds(specific).unwrap_or_default().center();
                waypoints(s, general_rect.center(), at, style)
            }
        };
        plans.push((e, points));
    }
    for (e, points) in plans {
        tx.update_presentation(diagram, e, |p| {
            p.style.shared = style;
            p.style.shared_chosen = chosen;
            p.style.auto_route = false;
            p.style.line_shape = umlcraft_model::LineShape::Line;
            if let Shape::Edge { waypoints, source_anchor, target_anchor, .. } = &mut p.shape {
                *waypoints = points;
                (*source_anchor, *target_anchor) = (None, None);
            }
        })?;
    }
    Ok(())
}

/// After the waypoints of the shared edge `edge` were changed by hand from `old` to their
/// present ones, moves the common line of its whole group to where the moved point went.
/// Does nothing for other edges.
pub fn move_common_line(tx: &mut Tx<'_>, diagram: Id, edge: Id, old: &[Point]) -> Result<()> {
    let model = tx.model();
    let Some(view) = model.view(diagram) else { return Ok(()) };
    let Some(p) = view.get(edge) else { return Ok(()) };
    let style = p.style.shared;
    let (Some((general, _)), Shape::Edge { waypoints: new, .. }) = (ends(model, view, p), &p.shape)
    else {
        return Ok(());
    };
    if style.is_separate() || new.is_empty() {
        return Ok(());
    }
    let coordinate = |w: &Point| if style == SharedStyle::Horizontal { w.x } else { w.y };
    let before = old.first().map(coordinate);
    let at = new
        .iter()
        .map(coordinate)
        .max_by(|a, b| {
            let off = |v: f32| before.map_or(0.0, |b| (v - b).abs());
            off(*a).total_cmp(&off(*b))
        })
        .unwrap_or_default();
    let bounds = |id: Id| view.get(id).and_then(|p| p.bounds()).unwrap_or_default();
    let general_center = bounds(general).center();
    let mut plans = Vec::new();
    for e in generalizations_to(model, view, general) {
        let Some(q) = view.get(e).filter(|q| q.style.shared == style) else { continue };
        let Some((_, specific)) = ends(model, view, q) else { continue };
        plans.push((e, waypoints(bounds(specific).center(), general_center, at, style)));
    }
    for (e, points) in plans {
        tx.update_presentation(diagram, e, |p| {
            if let Shape::Edge { waypoints, .. } = &mut p.shape {
                *waypoints = points;
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_common_line_lies_halfway_and_at_least_15_from_the_general_element() {
        let general = Rect::new(300.0, 100.0, 62.0, 42.09);
        let subs = [Rect::new(150.0, 300.0, 40.0, 42.0), Rect::new(450.0, 260.0, 53.0, 42.0)];
        // Astah: halfway between 142.09 and 260.
        let at = default_line(general, &subs, SharedStyle::Vertical);
        assert!((at - 201.045).abs() < 0.01, "{at}");
        let close = [Rect::new(300.0, 150.0, 40.0, 42.0)];
        assert!((default_line(general, &close, SharedStyle::Vertical) - 157.09).abs() < 0.01);
        let right = [Rect::new(900.0, 150.0, 53.0, 42.0), Rect::new(900.0, 300.0, 61.0, 42.0)];
        let shape = Rect::new(700.0, 300.0, 56.01, 42.09);
        let at = default_line(shape, &right, SharedStyle::Horizontal);
        assert!((at - 828.005).abs() < 0.01, "{at}");
        let above = [Rect::new(300.0, 0.0, 40.0, 40.0)];
        assert!((default_line(general, &above, SharedStyle::Vertical) - 70.0).abs() < 0.01);
    }
}
