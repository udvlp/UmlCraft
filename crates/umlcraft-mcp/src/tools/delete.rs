//! Deleting elements and removing them from diagrams.

use super::*;

pub(super) fn delete_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let model = host.document().model();
    if model.contains(id) {
        if id == model.root() {
            return Err("the model's root cannot be deleted".into());
        }
        let what = summary(model, id);
        let kind = model.get(id).map(|e| e.kind().clone());
        change(host, "delete_element", |tx| {
            // As in the app: the neighbors of a region take its room, and an operand takes the
            // line above it and its guard's place along.
            match kind {
                Some(ElementKind::Region) => state_edit::delete_region(tx, id)?,
                Some(ElementKind::InteractionOperand(_)) => sequence_edit::delete_operand(tx, id)?,
                _ => tx.delete(id)?,
            }
            sequence_edit::tidy_all(tx)
        })?;
        return Ok(Reply::json(json!({ "deleted": what })));
    }
    let shown = model.views().find_map(|(d, v)| v.get(id).map(|p| (d, p.element)));
    match shown {
        Some((diagram, None)) => {
            change(host, "delete_element", |tx| {
                tx.remove_presentation(diagram, id)?;
                sequence_edit::tidy_all(tx)
            })?;
            Ok(Reply::json(json!({ "removed": id })))
        }
        Some((_, Some(element))) => delete_element(host, &json!({ "id": element.to_string() })),
        None => Err(format!("no element or presentation {id}")),
    }
}

pub(super) fn remove_from_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let presentation = on_diagram(model, diagram, id_arg(args, "id")?)?;
    change(host, "remove_from_diagram", |tx| {
        tx.remove_presentation(diagram, presentation)?;
        sequence_edit::tidy_all(tx)
    })?;
    Ok(Reply::json(json!({ "removed": presentation })))
}
