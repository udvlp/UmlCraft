//! The settings of activity nodes.

use super::*;

/// The settings of activity nodes `update_element` and `create_element` take.
pub(super) const ACTIVITY_NODE_KEYS: [&str; 9] = [
    "object_type",
    "in_state",
    "ordering",
    "upper_bound",
    "stream",
    "control",
    "exception",
    "object_effect",
    "calls",
];

/// Writes the object node settings and the activity called of `args` into the activity node
/// `kind`.
pub(super) fn set_activity_node(
    model: &Model,
    id: Id,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let Some(key) = ACTIVITY_NODE_KEYS.into_iter().find(|k| args.get(k).is_some()) else {
        return Ok(());
    };
    let type_name = kind.type_name();
    let ElementKind::ActivityNode(p) = kind else {
        return Err(format!("a {type_name} has no `{key}`"));
    };
    let object = p.node.is_object();
    let only_objects =
        |key: &str| format!("only object nodes, pins and activity parameters have `{key}`");
    if let Some(text) = opt_str(args, "object_type") {
        if !object {
            return Err(only_objects("object_type"));
        }
        let text = text.trim();
        // Names resolve as in a declaration, from the node outward.
        p.type_ref = (!text.is_empty()).then(|| member::parse_type(model, id, text));
    }
    if let Some(state) = opt_str(args, "in_state") {
        if !object {
            return Err(only_objects("in_state"));
        }
        let state = state.trim();
        let state = state.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(state);
        p.in_state = state.trim().to_owned();
    }
    if let Some(ordering) = opt_str(args, "ordering") {
        if !object {
            return Err(only_objects("ordering"));
        }
        p.ordering = match ordering.trim() {
            "" => None,
            o => Some(umlcraft_model::ObjectOrdering::parse(o).ok_or_else(|| {
                format!("`ordering` is one of unordered, ordered, LIFO, FIFO, not `{o}`")
            })?),
        };
    }
    if let Some(bound) = opt_str(args, "upper_bound") {
        if !object {
            return Err(only_objects("upper_bound"));
        }
        p.upper_bound = bound.trim().to_owned();
    }
    if let Some(stream) = args.get("stream") {
        if !object {
            return Err(only_objects("stream"));
        }
        p.is_stream = stream.as_bool().ok_or("`stream` is true or false")?;
    }
    if let Some(control) = args.get("control") {
        if !object {
            return Err(only_objects("control"));
        }
        p.is_control = control.as_bool().ok_or("`control` is true or false")?;
    }
    if let Some(exception) = args.get("exception") {
        if !object {
            return Err(only_objects("exception"));
        }
        p.is_exception = exception.as_bool().ok_or("`exception` is true or false")?;
    }
    if let Some(effect) = opt_str(args, "object_effect") {
        if !object {
            return Err(only_objects("object_effect"));
        }
        p.effect = match effect.trim() {
            "" => None,
            e => Some(umlcraft_model::ObjectEffect::parse(e).ok_or_else(|| {
                format!("`object_effect` is one of create, read, update, delete, not `{e}`")
            })?),
        };
    }
    if args.get("calls").is_some() {
        if p.node != ActivityNodeKind::CallBehavior {
            return Err("only call behavior actions have `calls`".into());
        }
        // The activity is given as itself or as one of its diagrams.
        let calls = opt_str(args, "calls").map(str::trim).filter(|c| !c.is_empty());
        p.behavior = match calls.map(parse_id).transpose()? {
            Some(id) => match model.get(id).map(|e| e.kind()) {
                Some(ElementKind::Activity) => Some(id),
                Some(k) if k.as_diagram() == Some(DiagramKind::Activity) => Some(
                    model.get(id).and_then(|d| d.owner()).ok_or("the diagram has no activity")?,
                ),
                _ => return Err(format!("{id} is no activity")),
            },
            None => None,
        };
    }
    Ok(())
}
