//! Edits as the app makes them, for the app and for the MCP server alike: one undo step, after
//! which boxes sized to their content take the size it needs, ports stay on the border of their
//! class or part and lines kept by the router are routed again ([`apply`]); and new diagrams with the behavior they belong to
//! ([`create_diagram`]).

use std::collections::HashMap;

use umlcraft_model::{
    DiagramKind, Document, ElementData, ElementKind, Id, Model, Rect, Result, Size, Tx,
};

use crate::scene::{self, TextMeasure};
use crate::{autoroute, composite, sequence_edit};

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

/// Runs `f` as one undo step of `doc`. Afterwards, as in Astah, boxes sized to their content
/// whose content the edit changed take the size it now needs (a new box always does); others
/// keep their size, even if Astah stored one that does not fit. Classes grow around their parts,
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
        for b in fitted_boxes(tx.model(), measure) {
            let unchanged = before.get(&b.presentation) == Some(&b.content);
            if !unchanged && !close(b.bounds.size(), b.size) {
                let rect = Rect::from_origin_size(b.bounds.origin(), b.size);
                tx.set_bounds(b.diagram, b.presentation, rect)?;
            }
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
        | DiagramKind::CompositeStructure => None,
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

/// Creates an object diagram named `name` under `owner`: a class diagram whose palette starts
/// with Object and Link.
pub fn create_object_diagram(tx: &mut Tx<'_>, owner: Id, name: &str) -> Result<Id> {
    tx.create(owner, ElementData::new(name, ElementKind::object_diagram()))
}
