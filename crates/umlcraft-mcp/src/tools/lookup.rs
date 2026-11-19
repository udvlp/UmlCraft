//! Finding diagrams, elements and palette tools, and making changes as the app makes them.

use super::*;

pub(super) const DIAGRAM_KINDS: [(&str, DiagramKind); 9] = [
    ("class", DiagramKind::Class),
    ("use_case", DiagramKind::UseCase),
    ("sequence", DiagramKind::Sequence),
    ("activity", DiagramKind::Activity),
    ("state_machine", DiagramKind::StateMachine),
    ("component", DiagramKind::Component),
    ("deployment", DiagramKind::Deployment),
    ("composite_structure", DiagramKind::CompositeStructure),
    ("entity_relationship", DiagramKind::EntityRelationship),
];

pub(super) fn diagram_key(kind: DiagramKind) -> &'static str {
    DIAGRAM_KINDS.iter().find(|(_, k)| *k == kind).map_or("class", |(key, _)| key)
}

/// The kind of the diagram `id` as the tools name it: an object diagram is a class diagram
/// whose palette starts with Object and Link.
pub(super) fn diagram_key_of(model: &Model, id: Id) -> &'static str {
    match model.get(id).map(|e| e.kind()) {
        Some(ElementKind::Diagram(p)) if p.objects => "object",
        Some(ElementKind::Diagram(p)) => diagram_key(p.diagram),
        _ => "class",
    }
}

pub(super) fn kind_name(model: &Model, id: Id) -> &'static str {
    model.get(id).map_or("", |e| e.kind().type_name())
}

pub(super) fn summary(model: &Model, id: Id) -> Value {
    let element = model.get(id);
    json!({
        "id": id,
        "kind": element.map_or("", |e| e.kind().type_name()),
        "name": element.map_or_else(String::new, |_| member::display_name(model, id)),
    })
}

/// The reply of a tool that made `id`: its summary, with the id also as `element`, the name
/// `create_element` and `connect` reply with, so that `$n.element` in a batch works after any
/// of them.
pub(super) fn created(model: &Model, id: Id) -> Value {
    let mut reply = summary(model, id);
    reply["element"] = json!(id);
    reply
}

/// `id` if it names a diagram, else an error.
pub(super) fn diagram_of(model: &Model, id: Id) -> Result<DiagramKind, String> {
    model
        .get(id)
        .and_then(|e| e.kind().as_diagram())
        .filter(|_| model.view(id).is_some())
        .ok_or_else(|| format!("{id} is not a diagram"))
}

/// The diagram given as `key`, else the one shown.
pub(super) fn diagram_arg(
    host: &dyn Host,
    args: &Value,
    key: &str,
) -> Result<(Id, DiagramKind), String> {
    let diagram = match opt_id(args, key)? {
        Some(diagram) => diagram,
        None => host.active_diagram().ok_or("no diagram is shown; give `diagram`")?,
    };
    Ok((diagram, diagram_of(host.document().model(), diagram)?))
}

/// The presentation on `diagram` that `id` names: `id` itself, or the node (else the first
/// presentation) showing the element `id`.
pub(super) fn on_diagram(model: &Model, diagram: Id, id: Id) -> Result<Id, String> {
    let view = model.view(diagram).ok_or_else(|| format!("{diagram} is not a diagram"))?;
    if view.get(id).is_some() {
        return Ok(id);
    }
    let mut shown = view.presentations_of(id);
    let first = shown.next().map(|p| p.id);
    view.presentations_of(id).find(|p| p.bounds().is_some()).map(|p| p.id).or(first).ok_or_else(
        || {
            let what = model.get(id).map_or_else(
                || id.to_string(),
                |e| format!("{} {}", e.kind().type_name(), e.name()),
            );
            format!("{what} is not shown on this diagram; `show_element` puts it there")
        },
    )
}

/// The tools `diagram` offers: its kind's, an object diagram's, or those of an ER notation.
pub(super) fn tools_of(model: &Model, diagram: Id, kind: DiagramKind) -> Vec<Tool> {
    match model.get(diagram).map(|d| d.kind()) {
        Some(ElementKind::Diagram(p)) => palette::diagram_palette(p),
        _ => palette::palette(kind),
    }
}

/// The tool of `diagram`'s palette with `key`.
pub(super) fn diagram_tool(
    model: &Model,
    diagram: Id,
    kind: DiagramKind,
    key: &str,
) -> Result<Tool, String> {
    let tools = tools_of(model, diagram, kind);
    tools.iter().copied().find(|t| t.key() == key).ok_or_else(|| {
        let keys: Vec<&str> = tools.iter().map(|t| t.key()).filter(|k| *k != "select").collect();
        let mut message = format!(
            "a {} diagram has no tool `{key}`; it has: {}",
            diagram_key(kind),
            keys.join(", ")
        );
        if kind == DiagramKind::Sequence && key == "actor" {
            message.push_str(
                ". An actor takes part as a `lifeline` whose `classifier` is the actor \
                 (`create_model_element` makes one): it is drawn as a stick figure",
            );
        }
        message
    })
}

pub(super) fn node_tool(
    model: &Model,
    diagram: Id,
    kind: DiagramKind,
    key: &str,
) -> Result<NodeTool, String> {
    match diagram_tool(model, diagram, kind, key)? {
        Tool::Node(tool) => Ok(tool),
        _ => Err(format!("`{key}` draws no element; `connect` draws relationships")),
    }
}

pub(super) fn edge_tool(
    model: &Model,
    diagram: Id,
    kind: DiagramKind,
    key: &str,
) -> Result<EdgeTool, String> {
    match diagram_tool(model, diagram, kind, key)? {
        Tool::Edge(tool) => Ok(tool),
        _ => Err(format!("`{key}` draws no relationship; `create_element` draws elements")),
    }
}

/// A free place for a new node: right of what the diagram shows, or below it once that is wide.
pub(super) fn free_origin(model: &Model, diagram: Id) -> Point {
    let snap = |v: f32| (v / 10.0).round() * 10.0;
    match model.view(diagram).and_then(|v| v.content_bounds()) {
        None => Point::new(40.0, 40.0),
        Some(r) if r.right() < 1000.0 => Point::new(snap(r.right() + 60.0), snap(r.y.max(40.0))),
        Some(r) => Point::new(snap(r.x), snap(r.bottom() + 60.0)),
    }
}

/// Room kept inside a system boundary around its use cases, and between them.
pub(super) const BOUNDARY_PADDING: f32 = 20.0;

pub(super) const BOUNDARY_HEADER: f32 = 40.0;

pub(super) const USE_CASE_GAP: f32 = 40.0;

/// On a diagram with exactly one system boundary: that boundary's presentation and a free
/// place in it for a new use case, below the use cases it holds as they are drawn.
pub(super) fn spot_in_boundary(
    model: &Model,
    diagram: Id,
    measure: &dyn TextMeasure,
) -> Option<(Id, Point)> {
    let view = model.view(diagram)?;
    let kind_of = |p: &umlcraft_model::Presentation| p.element.and_then(|e| model.get(e));
    let mut boundaries = view
        .presentations()
        .iter()
        .filter(|p| kind_of(p).is_some_and(|e| matches!(e.kind(), ElementKind::SystemBoundary)));
    let boundary = boundaries.next()?;
    if boundaries.next().is_some() {
        return None;
    }
    let frame = boundary.bounds()?;
    let lowest = view
        .presentations()
        .iter()
        .filter(|p| kind_of(p).is_some_and(|e| matches!(e.kind(), ElementKind::UseCase(_))))
        .filter_map(|p| {
            let b = p.bounds()?;
            let size = scene::drawn_size(model, p, measure).unwrap_or(b.size());
            Some(Rect::new(b.x, b.y, size.width, size.height))
        })
        .filter(|r| frame.contains(r.center()))
        .map(|r| r.bottom())
        .reduce(f32::max);
    let y = lowest.map_or(frame.y + BOUNDARY_HEADER, |b| b + USE_CASE_GAP);
    Some((boundary.id, Point::new(frame.x + BOUNDARY_PADDING, y)))
}

pub(super) fn is_sequence(model: &Model, diagram: Id) -> bool {
    model.get(diagram).and_then(|e| e.kind().as_diagram()) == Some(DiagramKind::Sequence)
}

/// One undo step on the host's document.
pub(super) fn change<R>(
    host: &mut dyn Host,
    tool: &str,
    f: impl FnOnce(&mut Tx<'_>) -> umlcraft_model::Result<R>,
) -> Result<R, String> {
    let label = host.label(tool);
    let (doc, measure) = host.parts();
    edit::apply(doc, label, measure, f).map_err(model_error)
}

/// [`change`] for a change that measures text.
pub(super) fn change_measured<R>(
    host: &mut dyn Host,
    tool: &str,
    f: impl FnOnce(&mut Tx<'_>, &dyn TextMeasure) -> umlcraft_model::Result<R>,
) -> Result<R, String> {
    let label = host.label(tool);
    let (doc, measure) = host.parts();
    edit::apply(doc, label, measure, |tx| f(tx, measure)).map_err(model_error)
}
