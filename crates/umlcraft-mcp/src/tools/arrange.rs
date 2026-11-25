//! Moving nodes, arranging diagrams and routing lines.

use super::*;

pub(super) fn move_node(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let node = on_diagram(model, diagram, id_arg(args, "node")?)?;
    let presentation = model.view(diagram).and_then(|v| v.get(node)).ok_or("no presentation")?;
    if let Shape::Activation { .. } = presentation.shape {
        return move_bar(host, args, diagram, node);
    }
    // Messages and fragments of a sequence diagram move as when dragged in the app: to height
    // `y`, into the interaction or operand found there.
    let item = presentation.element.filter(|&e| {
        is_sequence(model, diagram)
            && matches!(
                model.get(e).map(|e| e.kind()),
                Some(
                    ElementKind::Message(_)
                        | ElementKind::CombinedFragment(_)
                        | ElementKind::InteractionUse(_)
                )
            )
    });
    let y = opt_num(args, "y").ok_or("`y` is missing")?;
    let bounds = presentation.bounds();
    let slot = item.and_then(|_| slot_at(host, diagram, y));
    if let (Some(item), Some(slot), None) = (item, slot, bounds) {
        change(host, "move_node", |tx| palette::move_to_slot(tx, item, slot))?;
        return Ok(Reply::json(json!({ "presentation": node, "y": y })));
    }
    let bounds = bounds.ok_or("only nodes, messages and activation bars can be moved")?;
    // A box sized by hand keeps that size, as in the app.
    let resized = args.get("width").is_some() || args.get("height").is_some();
    let x = opt_num(args, "x").unwrap_or(bounds.x);
    let width = opt_num(args, "width").unwrap_or(bounds.width).max(4.0);
    let height = opt_num(args, "height").unwrap_or(bounds.height).max(4.0);
    let (dx, dy) = (x - bounds.x, y - bounds.y);
    let model = host.document().model();
    let sequence = is_sequence(model, diagram);
    // As when dragged: what lies in a container (substates, the nodes of a package or swimlane)
    // moves along, and so do the bends of lines between what moves.
    let contents: Vec<Id> = if sequence || (dx == 0.0 && dy == 0.0) {
        Vec::new()
    } else {
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
        scene::with_contents(doc.model(), &scene, &[node])
            .into_iter()
            .filter(|&p| p != node)
            .collect()
    };
    let view = host.document().model().view(diagram);
    let nodes: Vec<(Id, Rect)> = std::iter::once((node, Rect::new(x, y, width, height)))
        .chain(
            contents.iter().filter_map(|&p| Some((p, view?.get(p)?.bounds()?.translate(dx, dy)))),
        )
        .collect();
    change(host, "move_node", |tx| {
        if let (Some(item), Some(slot)) = (item, slot) {
            palette::move_to_slot(tx, item, slot)?;
        }
        if resized {
            tx.update_presentation(diagram, node, |p| p.style.auto_size = false)?;
        }
        // Lines between what moves keep their shape; a state moved into another one (or out of
        // it) becomes its substate.
        edit::set_node_bounds(tx, diagram, &nodes)
    })?;
    Ok(Reply::json(
        json!({ "presentation": node, "x": x, "y": y, "width": width, "height": height }),
    ))
}

/// Moves the activation bar `bar` of a sequence diagram: its top to `y`, its length to
/// `height`. Its x follows from its lifeline. Stretched over the next bars of its lifeline, it
/// takes them in: they go, and it reaches as far as they did. Bars that lay in it before, or in
/// a bar it takes in, stay nested in it. A bar that no message reaches at its new place is
/// refused, as tidying would drop it.
pub(super) fn move_bar(host: &mut dyn Host, args: &Value, diagram: Id, bar: Id) -> Outcome {
    if args.get("x").is_some() || args.get("width").is_some() {
        return Err("an activation bar lies on its lifeline; give only `y` (its top) and \
                    `height`"
            .into());
    }
    let view = host.document().model().view(diagram).ok_or("no view")?;
    let bars: Vec<(Id, Id, f32, f32)> = view
        .presentations()
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Activation { lifeline, top, height, .. } => {
                Some((p.id, lifeline, top, top + height))
            }
            _ => None,
        })
        .collect();
    let &(_, lane, old_top, old_bottom) =
        bars.iter().find(|b| b.0 == bar).ok_or("no activation bar")?;
    let top = opt_num(args, "y").unwrap_or(old_top);
    let height = opt_num(args, "height").unwrap_or(old_bottom - old_top);
    if height.is_nan() || height <= 0.0 {
        return Err("`height` of an activation bar is more than 0".into());
    }
    let within = |y: f32, top: f32, bottom: f32| top + 0.5 < y && y <= bottom + 0.5;
    let reached: Vec<&(Id, Id, f32, f32)> = bars
        .iter()
        .filter(|b| b.0 != bar && b.1 == lane && within(b.2, top, top + height))
        .filter(|b| !within(b.2, old_top, old_bottom))
        .collect();
    let taken: Vec<&(Id, Id, f32, f32)> = reached
        .iter()
        .copied()
        .filter(|b| !reached.iter().any(|o| o.0 != b.0 && within(b.2, o.2, o.3) && b.3 <= o.3))
        .collect();
    let bottom = taken.iter().map(|b| b.3).fold(top + height, f32::max);
    let gone: Vec<Id> = taken.iter().map(|b| b.0).collect();
    let (top, height) = change(host, "move_node", |tx| {
        for &taken in &gone {
            tx.remove_presentation(diagram, taken)?;
        }
        tx.update_presentation(diagram, bar, |p| {
            if let Shape::Activation { top: t, height: h, .. } = &mut p.shape {
                (*t, *h) = (top, bottom - top);
            }
        })?;
        sequence_edit::tidy(tx, diagram)?;
        match tx.model().view(diagram).and_then(|v| v.get(bar)).map(|p| &p.shape) {
            Some(&Shape::Activation { top, height, .. }) => Ok((top, height)),
            _ => Err(ModelError::Invalid(
                "no message starts or ends in the bar there, so it would vanish".into(),
            )),
        }
    })?;
    Ok(Reply::json(json!({ "presentation": bar, "top": top, "bottom": top + height })))
}

pub(super) fn arrange_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if !layout::can_arrange(kind) {
        return Err(format!("a {} diagram cannot be arranged", diagram_key(kind)));
    }
    let direction = match opt_str(args, "direction") {
        Some("right") => layout::Direction::Right,
        Some("down") => layout::Direction::Down,
        _ => layout::default_direction(kind),
    };
    let model = host.document().model();
    let scope: Option<Vec<Id>> = ids_arg(args, "nodes")?
        .map(|ids| ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect())
        .transpose()?;
    let moved = change_measured(host, "arrange_diagram", |tx, measure| {
        layout::arrange(tx, diagram, scope.as_deref(), direction, measure)
    })?;
    Ok(Reply::json(json!({ "moved": moved.len() })))
}

pub(super) fn route_lines(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let view = model.view(diagram).ok_or("no view")?;
    let lines: Vec<Id> = match ids_arg(args, "lines")? {
        Some(ids) => {
            ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect::<Result<_, _>>()?
        }
        None => view
            .presentations()
            .iter()
            .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
            .map(|p| p.id)
            .collect(),
    };
    let routed = change(host, "route_lines", |tx| autoroute::route_lines(tx, diagram, &lines))?;
    Ok(Reply::json(json!({ "routed": routed })))
}
