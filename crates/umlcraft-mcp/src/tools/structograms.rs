//! The structogram tools.

use super::*;

/// The structogram `id` names.
pub(super) fn structogram_of(
    model: &Model,
    id: Id,
) -> Result<&umlcraft_model::StructogramProps, String> {
    model
        .get(id)
        .and_then(|e| e.kind().as_structogram())
        .ok_or_else(|| format!("{id} is no structogram"))
}

/// `operation` if it names an operation.
pub(super) fn operation_arg(model: &Model, operation: Id) -> Result<Id, String> {
    match model.get(operation).map(|e| e.kind()) {
        Some(ElementKind::Operation(_)) => Ok(operation),
        _ => Err(format!("{operation} is no operation")),
    }
}

pub(super) fn structogram_reply(model: &Model, id: Id) -> Result<Value, String> {
    let p = structogram_of(model, id)?;
    // `element` as after create_element, `diagram` as after create_diagram: the tools that
    // draw or export a structogram take it as their `diagram`.
    Ok(json!({
        "id": id,
        "element": id,
        "diagram": id,
        "kind": "structogram",
        "owner": model.get(id).and_then(|e| e.owner()),
        "operation": p.operation,
        "structogram": structogram::to_json(&p.structogram),
    }))
}

pub(super) fn get_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    Ok(Reply::json(structogram_reply(host.document().model(), id_arg(args, "id")?)?))
}

pub(super) fn create_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let model = host.document().model();
    let tree = args.get("structogram").ok_or("`structogram` is missing")?;
    let diagram = structogram::from_json(tree, None)?;
    let operation = opt_id(args, "operation")?.map(|o| operation_arg(model, o)).transpose()?;
    let owner = match opt_id(args, "owner")? {
        Some(owner) => owner,
        None => operation.and_then(|o| model.get(o)?.owner()).unwrap_or(model.root()),
    };
    let id = change(host, "create_structogram", |tx| {
        let mut props = umlcraft_model::StructogramProps::new(&diagram);
        props.operation = operation;
        tx.create(owner, ElementData::new("", ElementKind::Structogram(props)))
    })?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}

pub(super) fn update_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let model = host.document().model();
    let before = structogram_of(model, id)?;
    let diagram = match args.get("structogram") {
        Some(tree) => Some(structogram::from_json(tree, Some(&before.structogram))?),
        None => None,
    };
    let operation = match opt_str(args, "operation").map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(text) => Some(Some(operation_arg(model, parse_id(text)?)?)),
    };
    change(host, "update_structogram", |tx| {
        if let Some(diagram) = &diagram {
            tx.set_structogram(id, diagram)?;
        }
        if let Some(operation) = operation {
            tx.update(id, |data| {
                if let ElementKind::Structogram(p) = &mut data.kind {
                    p.operation = operation;
                }
            })?;
        }
        Ok(())
    })?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}

pub(super) fn edit_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let before = structogram_of(host.document().model(), id)?;
    let mut tree = structogram::to_json(&before.structogram);
    let action = str_arg(args, "action")?;
    structogram::edit(&mut tree, action, str_arg(args, "path")?, args.get("element"))?;
    let diagram = structogram::from_json(&tree, Some(&before.structogram))?;
    change(host, "edit_structogram", |tx| tx.set_structogram(id, &diagram))?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}
