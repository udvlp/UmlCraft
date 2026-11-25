//! Combined fragments and their operands in sequence diagrams.

use super::*;

pub(super) fn wrap_in_fragment(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if kind != DiagramKind::Sequence {
        return Err("fragments belong to sequence diagrams".into());
    }
    let model = host.document().model();
    let view = model.view(diagram).ok_or("no view")?;
    // Presentations stand for their messages.
    let items: Vec<Id> = ids_arg(args, "items")?
        .ok_or("`items` is missing")?
        .into_iter()
        .map(|i| view.get(i).and_then(|p| p.element).unwrap_or(i))
        .collect();
    let operator = pick(args, "operator", &OPERATORS)?.unwrap_or(InteractionOperator::Opt);
    let guard = operand(model, args)?;
    // A loop's guard goes into the tab; any other stands below it, above the first message.
    let guarded = operator != InteractionOperator::Loop
        && opt_str(args, "guard").is_some_and(|g| !g.trim().is_empty());
    let fragment = change(host, "wrap_in_fragment", |tx| {
        let (fragment, _) = palette::wrap_in_guarded_fragment(tx, diagram, &items, guarded)?;
        tx.update(fragment, |data| {
            if let ElementKind::CombinedFragment(p) = &mut data.kind {
                p.operator = operator;
            }
        })?;
        if let Some(&operand) = tx.model().children(fragment).first() {
            tx.update(operand, |data| data.kind = guard)?;
        }
        if operator == InteractionOperator::Alt {
            palette::add_else(tx, fragment)?;
        }
        sequence_edit::tidy(tx, diagram)?;
        Ok(fragment)
    })?;
    let model = host.document().model();
    let mut reply = created(model, fragment);
    reply["operands"] = json!(model.children(fragment));
    Ok(Reply::json(reply))
}

pub(super) fn add_operand(host: &mut dyn Host, args: &Value) -> Outcome {
    let fragment = id_arg(args, "fragment")?;
    let model = host.document().model();
    if !matches!(model.get(fragment).map(|e| e.kind()), Some(ElementKind::CombinedFragment(_))) {
        return Err(format!("{fragment} is no combined fragment"));
    }
    let guard = operand(model, args)?;
    let operand = change(host, "add_operand", |tx| {
        let operand = palette::add_operand(tx, fragment)?;
        tx.update(operand, |data| data.kind = guard)?;
        sequence_edit::tidy_all(tx)?;
        Ok(operand)
    })?;
    Ok(Reply::json(created(host.document().model(), operand)))
}

/// An operand with the guard of `args` ("" if none).
fn operand(model: &Model, args: &Value) -> Result<ElementKind, String> {
    let mut operand = ElementKind::InteractionOperand(Default::default());
    let guard = opt_str(args, "guard").unwrap_or("");
    set_properties(model, &mut operand, &json!({ "guard": guard }))?;
    Ok(operand)
}
