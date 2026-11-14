//! Edits as the app makes them, for the app and for the MCP server alike: one undo step, after
//! which boxes sized to their content take the size it needs, ports stay on the border of their
//! class or part and lines kept by the router are routed again ([`apply`]); and new diagrams with the behavior they belong to
//! ([`create_diagram`]).

use std::collections::HashMap;

use umlcraft_model::{
    DiagramKind, Document, ElementData, ElementKind, Id, Model, ModelError, Point, Presentation,
    Rect, Result, Shape, Size, Tx,
};

use crate::scene::{self, TextMeasure};
use crate::{autoroute, composite, er_layout, sequence_edit, state_edit};

/// An automatically sized box: where it is, its bounds, the size its content needs, and the
/// content.
struct FittedBox {
    diagram: Id,
    presentation: Id,
    bounds: Rect,
    size: Size,
    content: String,
}

/// All automatically sized boxes of all diagrams.
fn fitted_boxes(model: &Model, measure: &dyn TextMeasure) -> Vec<FittedBox> {
    model
        .views()
        .flat_map(|(diagram, view)| {
            view.presentations().iter().filter_map(move |p| {
                let bounds = p.bounds()?;
                let (size, content) = scene::fitted_box(model, p, measure)?;
                Some(FittedBox { diagram, presentation: p.id, bounds, size, content })
            })
        })
        .collect()
}

/// The texts of the duration and time constraints of all diagrams, by diagram and presentation.
fn constraint_texts(model: &Model) -> HashMap<(Id, Id), String> {
    model
        .views()
        .flat_map(|(diagram, view)| {
            view.presentations().iter().filter_map(move |p| {
                let e = model.get(p.element?)?;
                matches!(
                    e.kind(),
                    ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_)
                )
                .then(|| ((diagram, p.id), e.name().to_owned()))
            })
        })
        .collect()
}

/// Runs `f` as one undo step of `doc`. Afterwards, as in Astah, boxes sized to their content
/// whose content the edit changed take the size it now needs (a new box always does); others
/// keep their size, even if Astah stored one that does not fit. State invariants take the
/// height of their text; a sequence diagram where one did, or where a duration or time
/// constraint was renamed, is tidied again, so that lifelines and frame hold them and their
/// labels. Classes grow around their parts,
/// and ports follow the border of the class or part they sit on ([`composite::follow`]). Then lines that the router
/// keeps are routed again where their nodes moved or something moved into their way, and found
/// and lost messages keep their length to lifelines that moved.
pub fn apply<R>(
    doc: &mut Document,
    label: impl Into<String>,
    measure: &dyn TextMeasure,
    f: impl FnOnce(&mut Tx<'_>) -> Result<R>,
) -> Result<R> {
    let before: HashMap<Id, String> = fitted_boxes(doc.model(), measure)
        .into_iter()
        .map(|b| (b.presentation, b.content))
        .collect();
    let constraints = constraint_texts(doc.model());
    let routes = autoroute::snapshots(doc.model());
    let lifelines = sequence_edit::lifeline_centers(doc.model());
    let ports = composite::snapshots(doc.model());
    let label = label.into();
    // Debug builds say what an edit broke, to catch it where it happens.
    #[cfg(debug_assertions)]
    let (rules, what) = (crate::invariants::check(doc.model(), measure), label.clone());
    let result = doc.edit(label, |tx| {
        let result = f(tx)?;
        sequence_edit::follow_lifelines(tx, &lifelines)?;
        composite::follow(tx, &ports)?;
        let close = |a: Size, b: Size| {
            (a.width - b.width).abs() < 0.01 && (a.height - b.height).abs() < 0.01
        };
        let mut sequences = Vec::new();
        for b in fitted_boxes(tx.model(), measure) {
            let unchanged = before.get(&b.presentation) == Some(&b.content);
            if !unchanged && !close(b.bounds.size(), b.size) {
                // A lifeline's head grows to both sides, so that its line stays where it is.
                let lifeline = tx
                    .model()
                    .view(b.diagram)
                    .and_then(|v| v.get(b.presentation))
                    .is_some_and(|p| matches!(p.shape, Shape::Lifeline { .. }));
                let x =
                    if lifeline { b.bounds.center().x - b.size.width / 2.0 } else { b.bounds.x };
                let rect = Rect::from_origin_size(Point::new(x, b.bounds.y), b.size);
                tx.set_bounds(b.diagram, b.presentation, rect)?;
                let kind = tx.model().get(b.diagram).and_then(|d| d.kind().as_diagram());
                if kind == Some(DiagramKind::Sequence) && !sequences.contains(&b.diagram) {
                    sequences.push(b.diagram);
                }
            }
        }
        // A constraint renamed: the frame holds its label.
        for ((diagram, p), text) in constraint_texts(tx.model()) {
            if constraints.get(&(diagram, p)) != Some(&text) && !sequences.contains(&diagram) {
                sequences.push(diagram);
            }
        }
        // A state invariant grown or shrunk for its text: the lifeline and frame follow.
        for diagram in sequences {
            sequence_edit::tidy(tx, diagram)?;
        }
        autoroute::follow(tx, &routes)?;
        Ok(result)
    });
    #[cfg(debug_assertions)]
    if result.is_ok() {
        let broken = crate::invariants::check(doc.model(), measure);
        for rule in crate::invariants::new_violations(&rules, &broken) {
            eprintln!("edit {what:?} broke a rule: {rule}");
        }
    }
    result
}

/// Gives the nodes `nodes` of `diagram` their new bounds, as dragging, resizing or aligning
/// them does, for the app and the MCP server alike, and what follows on the diagram:
///
/// - Lines between two nodes whose corner moved by the same distance keep their shape: their
///   bends move along. Loops from a node to itself follow in [`apply`].
/// - On ER diagrams, a line spread beside another between the same two nodes keeps its place by
///   it when only one of them moved ([`er_layout::follow_nodes`]).
/// - Sequence diagrams are tidied.
/// - On state machine diagrams, moved vertices go into the state drawn around them
///   ([`state_edit::after_move`]). A state that changed size, and what lies in it, only change
///   their owner ([`state_edit::reparent_vertices`]): a state shrunk over a substate is not
///   grown back around it.
pub fn set_node_bounds(tx: &mut Tx<'_>, diagram: Id, nodes: &[(Id, Rect)]) -> Result<()> {
    let model = tx.model();
    let Some(view) = model.view(diagram) else { return Ok(()) };
    let kind = model.get(diagram).and_then(|d| d.kind().as_diagram());
    let before: Vec<(Id, Rect)> =
        nodes.iter().filter_map(|&(p, _)| Some((p, view.get(p)?.bounds()?))).collect();
    let old: HashMap<Id, Rect> = before.iter().copied().collect();
    let shift: HashMap<Id, (f32, f32)> = nodes
        .iter()
        .filter_map(|&(p, b)| old.get(&p).map(|o| (p, (b.x - o.x, b.y - o.y))))
        .collect();
    let bends: Vec<(Id, Vec<Point>)> = view
        .presentations()
        .iter()
        .filter_map(|p| match &p.shape {
            Shape::Edge { source, target, waypoints, .. }
                if source != target && !waypoints.is_empty() =>
            {
                let (dx, dy) = *shift.get(source)?;
                (shift.get(target) == Some(&(dx, dy)) && (dx, dy) != (0.0, 0.0))
                    .then(|| (p.id, waypoints.iter().map(|w| w.offset(dx, dy)).collect()))
            }
            _ => None,
        })
        .collect();
    // States that changed size, with what lies in them.
    let resized: Vec<Id> = match kind {
        Some(DiagramKind::StateMachine) => {
            let states: Vec<Id> = nodes
                .iter()
                .filter(|&&(p, b)| old.get(&p).is_some_and(|o| o.size() != b.size()))
                .filter_map(|&(p, _)| view.get(p)?.element)
                .collect();
            let inside = |p: &Presentation| {
                p.element.is_some_and(|e| states.iter().any(|&s| s == e || model.is_ancestor(s, e)))
            };
            view.presentations().iter().filter(|p| inside(p)).map(|p| p.id).collect()
        }
        _ => Vec::new(),
    };
    for &(p, b) in nodes {
        tx.set_bounds(diagram, p, b)?;
    }
    for (edge, points) in bends {
        tx.set_waypoints(diagram, edge, points)?;
    }
    er_layout::follow_nodes(tx, diagram, &before)?;
    match kind {
        Some(DiagramKind::Sequence) => sequence_edit::tidy(tx, diagram),
        Some(DiagramKind::StateMachine) => {
            let moved: Vec<Id> =
                nodes.iter().map(|&(p, _)| p).filter(|p| !resized.contains(p)).collect();
            if !moved.is_empty() {
                state_edit::after_move(tx, diagram, &moved)?;
            }
            state_edit::reparent_vertices(tx, diagram, &resized)
        }
        _ => Ok(()),
    }
}

/// The behavior a diagram of `kind` belongs to, if any: sequence diagrams show an interaction,
/// activity diagrams an activity, state machine diagrams a state machine.
pub fn behavior_of(kind: DiagramKind) -> Option<ElementKind> {
    match kind {
        DiagramKind::Sequence => Some(ElementKind::Interaction),
        DiagramKind::Activity => Some(ElementKind::Activity),
        DiagramKind::StateMachine => Some(ElementKind::StateMachine),
        DiagramKind::Class
        | DiagramKind::UseCase
        | DiagramKind::Component
        | DiagramKind::Deployment
        | DiagramKind::CompositeStructure
        | DiagramKind::EntityRelationship => None,
    }
}

/// Creates a diagram of `kind` named `name` under `owner`, inside a new behavior of the same
/// name where the kind needs one. Returns the diagram.
pub fn create_diagram(tx: &mut Tx<'_>, owner: Id, kind: DiagramKind, name: &str) -> Result<Id> {
    let diagram = ElementData::new(name, ElementKind::diagram(kind));
    match behavior_of(kind) {
        Some(behavior) => {
            let behavior = tx.create(owner, ElementData::new(name, behavior))?;
            tx.create(behavior, diagram)
        }
        None => tx.create(owner, diagram),
    }
}

/// Moves all constraint labels of the line `presentation` by `by`, as Astah's editor moves them
/// together; each keeps its own offset from where the layout puts it. Offsets that come to
/// nothing are dropped.
pub fn shift_line_constraints(
    tx: &mut Tx<'_>,
    diagram: Id,
    presentation: Id,
    by: Point,
) -> Result<()> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let element = view.get(presentation).and_then(|p| p.element);
    let count = element.and_then(|e| model.get(e)).map_or(0, |e| e.data().constraints.len());
    tx.update_presentation(diagram, presentation, |p| {
        let offsets = &mut p.style.constraint_offsets;
        offsets.resize(count.max(offsets.len()), None);
        for offset in offsets.iter_mut().take(count) {
            let o = offset.unwrap_or_default().offset(by.x, by.y);
            *offset = (o.x.abs() > 0.01 || o.y.abs() > 0.01).then_some(o);
        }
        while offsets.last().is_some_and(Option::is_none) {
            offsets.pop();
        }
    })
}

/// Creates an object diagram named `name` under `owner`: a class diagram whose palette starts
/// with Object and Link.
pub fn create_object_diagram(tx: &mut Tx<'_>, owner: Id, name: &str) -> Result<Id> {
    tx.create(owner, ElementData::new(name, ElementKind::object_diagram()))
}
