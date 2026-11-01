//! What should hold after every edit, as sentences naming the diagram and the presentations
//! concerned. Tests check it after random edits; debug builds check it after each edit the app or
//! the MCP server makes ([`edit::apply`](crate::edit::apply)) and print what the edit broke.
//!
//! Files from Astah do not always keep these rules, so what counts is what an edit adds: compare
//! the violations before and after it ([`new_violations`]).

use std::collections::HashSet;

use umlcraft_model::{DiagramKind, ElementKind, Id, Model, Point, Rect, Shape, View};

use crate::route;
use crate::scene::{self, TextMeasure};
use crate::sequence_edit;

/// Every rule `model` breaks.
pub fn check(model: &Model, measure: &dyn TextMeasure) -> Vec<String> {
    let mut found = Vec::new();
    if let Err(err) = model.check() {
        found.push(format!("model: {err}"));
    }
    for (diagram, view) in model.views() {
        let name = model.get(diagram).map_or("", |d| d.name());
        for problem in check_diagram(model, diagram, view, measure) {
            found.push(format!("{name:?}: {problem}"));
        }
    }
    found
}

/// The violations in `after` that `before` does not have.
pub fn new_violations(before: &[String], after: &[String]) -> Vec<String> {
    let known: HashSet<&String> = before.iter().collect();
    after.iter().filter(|v| !known.contains(v)).cloned().collect()
}

fn check_diagram(
    model: &Model,
    diagram: Id,
    view: &View,
    measure: &dyn TextMeasure,
) -> Vec<String> {
    let mut found = Vec::new();
    for p in view.presentations() {
        if !p.is_finite() {
            found.push(format!(
                "{} has a coordinate that is not a number",
                describe(model, view, p.id)
            ));
        }
        if let Some(b) = p.bounds()
            && (b.width < 0.0 || b.height < 0.0)
        {
            found.push(format!("{} has a negative size", describe(model, view, p.id)));
        }
    }
    let drawn = scene::build(model, diagram, &scene::Preview::default(), measure);
    for edge in &drawn.edges {
        let what = || describe(model, view, edge.id);
        if edge.points.len() < 2 || edge.points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
        {
            found.push(format!("{} is drawn without a proper line", what()));
            continue;
        }
        // A line starts and ends at its nodes; a qualifier box moves an end off the node.
        let (first, last) = (edge.points[0], edge.points[edge.points.len() - 1]);
        if !edge.fixed && edge.qualifiers.is_empty() {
            for (end, point, rect) in
                [("starts", first, edge.ends[0]), ("ends", last, edge.ends[1])]
            {
                if !near(rect, point) {
                    found.push(format!("{} {end} away from its node", what()));
                }
            }
        }
        // Ends barely apart are joined by a slightly slanted line, as in Astah.
        if edge.shape.is_right_angle()
            && edge.points.windows(2).any(|w| {
                (w[0].x - w[1].x).abs() > route::NO_STEP && (w[0].y - w[1].y).abs() > route::NO_STEP
            })
        {
            found.push(format!("{} has a slanted segment though it runs at right angles", what()));
        }
    }
    match model.get(diagram).and_then(|d| d.kind().as_diagram()) {
        Some(DiagramKind::Sequence) => found.extend(sequence_edit::violations(model, diagram)),
        Some(DiagramKind::StateMachine) => found.extend(state_violations(model, view)),
        _ => {}
    }
    found
}

/// A substate is drawn inside the state it belongs to, and inside its region where the state
/// shows its regions. (A vertex drawn inside a state it does not belong to is no fault: moving
/// a state over others leaves them where they belong, as in Astah.)
fn state_violations(model: &Model, view: &View) -> Vec<String> {
    let mut found = Vec::new();
    for p in view.presentations() {
        let (Some(element), Some(bounds)) = (p.element, p.bounds()) else { continue };
        let Some(e) = model.get(element) else { continue };
        let Some(owner) = e.owner().and_then(|o| model.get(o)) else { continue };
        if !e.kind().is_vertex() {
            continue;
        }
        let (state, region) = match owner.kind() {
            ElementKind::State(_) => (owner.id(), None),
            ElementKind::Region => match owner.owner() {
                Some(state) => (state, Some(owner.id())),
                None => continue,
            },
            _ => continue,
        };
        let shown: Vec<_> = view.presentations_of(state).collect();
        let [parent] = shown[..] else { continue };
        let Some(outline) = parent.bounds() else { continue };
        let regions = scene::state_regions(model, parent);
        let area = region
            .and_then(|r| regions.iter().find(|shape| shape.region == r))
            .map_or(outline, |shape| shape.rect);
        if !area.contains(bounds.center()) {
            let name = model.get(state).map_or("", |s| s.name());
            let what = describe(model, view, p.id);
            found.push(format!("{what} lies outside {name:?}, which it belongs to"));
        }
    }
    found
}

fn near(rect: Rect, p: Point) -> bool {
    rect.inflate(1.0).contains(p)
}

/// A presentation for a message: its element's kind and name, or its shape, and its id.
pub(crate) fn describe(model: &Model, view: &View, presentation: Id) -> String {
    let Some(p) = view.get(presentation) else { return format!("{presentation}") };
    // The variant's name, without its fields.
    let kind = |e: &umlcraft_model::Element| {
        let debug = format!("{:?}", e.kind());
        debug.split(['(', ' ', '{']).next().unwrap_or_default().to_string()
    };
    let what = match p.element.and_then(|e| model.get(e)) {
        Some(e) if e.name().is_empty() => kind(e),
        Some(e) => format!("{} {:?}", kind(e), e.name()),
        None => match p.shape {
            Shape::Node { .. } => "node",
            Shape::Edge { .. } => "line",
            Shape::Activation { .. } => "bar",
            Shape::Text { .. } => "text",
            Shape::Line { .. } => "free line",
            _ => "presentation",
        }
        .to_string(),
    };
    match p.shape {
        Shape::Activation { .. } => format!("bar {presentation}"),
        _ => format!("{what} {presentation}"),
    }
}
