//! The tools that make diagrams, elements, relationships and members.

use super::*;

pub(super) fn create_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let key = str_arg(args, "kind")?;
    // An object diagram is a class diagram whose palette starts with Object and Link.
    let objects = key == "object";
    let kind = DIAGRAM_KINDS
        .iter()
        .find(|(k, _)| *k == key || (objects && *k == "class"))
        .map(|(_, kind)| *kind)
        .ok_or_else(|| format!("no diagram kind `{key}`"))?;
    let name = str_arg(args, "name")?.to_owned();
    let owner = opt_id(args, "owner")?.unwrap_or(host.document().model().root());
    let notation = pick(args, "notation", &ER_NOTATIONS)?;
    if notation.is_some() && kind != DiagramKind::EntityRelationship {
        return Err("only ER diagrams take `notation`".into());
    }
    let mut settings = ElementKind::diagram(kind);
    set_diagram_settings(&mut settings, args)?;
    let diagram = change_measured(host, "create_diagram", |tx, measure| {
        let diagram = if objects {
            edit::create_object_diagram(tx, owner, &name)?
        } else {
            edit::create_diagram(tx, owner, kind, &name)?
        };
        if let ElementKind::Diagram(wanted) = &settings {
            tx.update(diagram, |d| {
                if let ElementKind::Diagram(p) = &mut d.kind {
                    p.hide_message_numbers = wanted.hide_message_numbers;
                    p.flat_message_numbers = wanted.flat_message_numbers;
                    p.hide_return_values = wanted.hide_return_values;
                    p.hide_frame = wanted.hide_frame;
                    p.simple_states = wanted.simple_states;
                }
            })?;
        }
        if let Some(notation) = notation {
            er::set_notation(tx, diagram, notation, measure)?;
        }
        Ok(diagram)
    })?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "diagram": diagram, "kind": key, "name": name })))
}

pub(super) fn create_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let tool = node_tool(host.document().model(), diagram, kind, str_arg(args, "tool")?)?;
    // A use case without a place goes into the system boundary, if there is one, which grows
    // to hold it: a script that makes the boundary and then its use cases gets them in it.
    let unplaced = opt_num(args, "x").is_none() && opt_num(args, "y").is_none();
    let use_case = matches!(tool, NodeTool::UseCase);
    let in_boundary = if use_case && unplaced && opt_id(args, "on")?.is_none() {
        let (doc, measure) = host.parts();
        spot_in_boundary(doc.model(), diagram, measure)
    } else {
        None
    };
    let model = host.document().model();
    let on_lifeline = matches!(tool, NodeTool::StateInvariant | NodeTool::Termination);
    let origin = match (opt_num(args, "x"), opt_num(args, "y"), opt_id(args, "on")?) {
        _ if in_boundary.is_some() => in_boundary.map_or(Point::new(0.0, 0.0), |(_, at)| at),
        // A state invariant or termination goes on the lifeline `on`, at `y` or below the rest.
        (_, y, Some(on)) if on_lifeline => {
            let view = model.view(diagram).ok_or("not a diagram")?;
            let lane = palette::lifeline_of(model, view, on_diagram(model, diagram, on)?);
            sequence_edit::spot_on_lifeline(model, diagram, lane, y)
                .map_err(|_| "`on` is not a lifeline")?
        }
        (_, _, None) if on_lifeline && opt_num(args, "x").is_none() => {
            return Err("`on` (the lifeline) is missing".into());
        }
        (Some(x), Some(y), _) => Point::new(x, y),
        (_, _, Some(on)) => {
            let on = on_diagram(model, diagram, on)?;
            composite::free_spot(model, diagram, on, tool).ok_or("`on` is not a node")?
        }
        _ => free_origin(model, diagram),
    };
    let name = opt_str(args, "name").map(str::to_owned);
    let base = name.clone().unwrap_or_else(|| default_base(tool));
    // Object node settings (`control`, `exception`, …) apply to the new node; wrong ones are
    // refused before anything is drawn.
    let settings = ACTIVITY_NODE_KEYS.iter().any(|k| args.get(k).is_some());
    if settings {
        set_activity_node(model, diagram, &mut tool.element_kind(), args)?;
    }
    let is_abstract = uml::flag(args, "abstract")?;
    let classifier = uml::classifier(model, args)?;
    let slots = uml::slots(args)?;
    let mut trial = tool.element_kind();
    let empty_slots = uml::empty_slots(&trial, args)?;
    uml::set_flags(&mut trial, None, is_abstract)?;
    if let Some(classifier) = classifier {
        uml::set_classifier(&mut trial, classifier)?;
    }
    // A pin put on an action looks for room once its texts are known.
    let place_pin = matches!(tool, NodeTool::InputPin | NodeTool::OutputPin)
        && opt_id(args, "on")?.is_some()
        && (opt_num(args, "x").is_none() || opt_num(args, "y").is_none());
    let (element, node) = change_measured(host, "create_element", |tx, measure| {
        let (element, node) = palette::create_node(tx, diagram, tool, origin, &base)?;
        if let Some(name) = &name
            && tool.is_named()
        {
            tx.rename(element, name.clone())?;
        }
        // A comment's `name` is its text, which it shows.
        if let Some(text) = &name {
            tx.update(element, |d| {
                if let ElementKind::Comment(p) = &mut d.kind {
                    p.body = text.clone();
                }
            })?;
        }
        if settings {
            let mut kind = tx.model().element(element)?.kind().clone();
            set_activity_node(tx.model(), element, &mut kind, args)
                .map_err(umlcraft_model::ModelError::Invalid)?;
            tx.update(element, |data| data.kind = kind)?;
        }
        if is_abstract.is_some() || classifier.is_some() {
            // Checked above.
            tx.update(element, |data| {
                drop(uml::set_flags(&mut data.kind, None, is_abstract));
                if let Some(classifier) = classifier {
                    drop(uml::set_classifier(&mut data.kind, classifier));
                }
            })?;
            // A lifeline that stands for an actor is a stick figure.
            sequence_edit::fit_head_to_type(tx, element)?;
        }
        if let Some(slots) = &slots {
            uml::set_slots(tx, element, slots)?;
        }
        if let Some(shown) = empty_slots {
            tx.update_presentation(diagram, node, |p| p.style.hide_empty_slots = !shown)?;
        }
        if place_pin {
            activity::place_pin(tx, diagram, node, measure)?;
        }
        if let Some((boundary, _)) = in_boundary {
            let model = tx.model();
            let p = model.view(diagram).and_then(|v| v.get(node));
            let case = p.and_then(|p| {
                let b = p.bounds()?;
                let size = scene::drawn_size(model, p, measure).unwrap_or(b.size());
                Some(Rect::new(b.x, b.y, size.width, size.height))
            });
            if let Some(case) = case {
                tx.update_presentation(diagram, boundary, |p| {
                    if let Shape::Node { bounds, .. } = &mut p.shape {
                        bounds.width = bounds.width.max(case.right() + BOUNDARY_PADDING - bounds.x);
                        bounds.height =
                            bounds.height.max(case.bottom() + BOUNDARY_PADDING - bounds.y);
                    }
                })?;
            }
        }
        if is_sequence(tx.model(), diagram) {
            sequence_edit::tidy(tx, diagram)?;
        }
        Ok((element, node))
    })?;
    host.show_diagram(diagram);
    let model = host.document().model();
    let mut reply = json!({
        "element": element,
        "presentation": node,
        "kind": kind_name(model, element),
        "name": member::display_name(model, element),
    });
    if let Some(ElementKind::Comment(p)) = model.get(element).map(|e| e.kind()) {
        reply["text"] = json!(p.body);
    }
    Ok(Reply::json(reply))
}

/// The start of a free default name for `tool`, e.g. `Class` for `Class1`.
pub(super) fn default_base(tool: NodeTool) -> String {
    let key = tool.key();
    let mut chars = key.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

pub(super) fn create_model_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let owner = id_arg(args, "owner")?;
    let key = str_arg(args, "tool")?;
    let name = str_arg(args, "name")?.to_owned();
    let own_kind = match key {
        "erDomain" => Some(ElementKind::ErDomain(Default::default())),
        "extensionPoint" => Some(ElementKind::ExtensionPoint),
        _ => None,
    };
    if let Some(kind) = own_kind {
        let model = host.document().model();
        let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
        let data = ElementData::new(name, kind);
        if !owner_kind.can_own(&data.kind) {
            return Err(format!("a {} cannot own an {key}", owner_kind.type_name()));
        }
        let element = change(host, "create_model_element", |tx| tx.create(owner, data))?;
        return Ok(Reply::json(created(host.document().model(), element)));
    }
    let tool = DIAGRAM_KINDS
        .iter()
        .flat_map(|(_, kind)| palette::palette(*kind))
        .find_map(|t| match t {
            Tool::Node(tool) if tool.key() == key => Some(tool),
            _ => None,
        })
        .ok_or_else(|| format!("no element tool `{key}`"))?;
    let model = host.document().model();
    let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
    let data = tool.element_data(if tool.is_named() { name } else { String::new() });
    if !owner_kind.can_own(&data.kind) {
        return Err(format!("a {} cannot own a {}", owner_kind.type_name(), data.kind.type_name()));
    }
    let element = change(host, "create_model_element", |tx| tx.create(owner, data))?;
    Ok(Reply::json(created(host.document().model(), element)))
}

pub(super) fn show_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let element = id_arg(args, "element")?;
    let model = host.document().model();
    let origin = match (opt_num(args, "x"), opt_num(args, "y")) {
        (Some(x), Some(y)) => Point::new(x, y),
        _ => free_origin(model, diagram),
    };
    let lines = |model: &Model| -> Vec<Id> {
        model.view(diagram).map_or_else(Vec::new, |v| {
            v.presentations()
                .iter()
                .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
                .map(|p| p.id)
                .collect()
        })
    };
    let node = change(host, "show_element", |tx| {
        let before = lines(tx.model());
        let node = palette::show_element(tx, diagram, element, origin)?;
        let new: Vec<Id> = lines(tx.model()).into_iter().filter(|l| !before.contains(l)).collect();
        autoroute::route_new_lines(tx, diagram, &new)?;
        if is_sequence(tx.model(), diagram) {
            sequence_edit::tidy(tx, diagram)?;
        }
        Ok(node)
    })?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "presentation": node })))
}

pub(super) fn connect(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let tool = edge_tool(host.document().model(), diagram, kind, str_arg(args, "tool")?)?;
    // A constraint between associations is drawn in braces; given with them, they go.
    let name = opt_str(args, "name").map(|n| match tool {
        EdgeTool::Constraint => unbraced(n),
        _ => n.to_owned(),
    });
    let anyway = args.get("allow_invalid").and_then(Value::as_bool).unwrap_or(false);
    let with_ends = uml::END_KEYS.iter().any(|k| args.get(k).is_some());
    if with_ends {
        let mut trial = match tool {
            EdgeTool::Link => ElementKind::link(Id::generate(), Id::generate()),
            EdgeTool::Association | EdgeTool::AssociationClass => {
                ElementKind::association(Id::generate(), Id::generate())
            }
            _ => return Err(format!("a {} has no ends to set", tool.key())),
        };
        uml::set_ends(&mut trial, args)?;
    }
    let route = line::Route::parse(args)?;
    if route.is_some() && tool.message_sort().is_some() {
        return Err("a message has no route; `y` gives its height".into());
    }
    let extend = args.get("condition").is_some() || args.get("extension_points").is_some();
    if extend && tool != EdgeTool::Extend {
        return Err("only an extend has a `condition` and `extension_points`".into());
    }
    let model = host.document().model();
    let source = on_diagram(model, diagram, id_arg(args, "source")?)?;
    // Found and lost messages are drawn from their lifeline to itself.
    let target = match (tool, opt_id(args, "target")?) {
        (EdgeTool::FoundMessage | EdgeTool::LostMessage | EdgeTool::TimeConstraint, _) => source,
        (_, Some(target)) => on_diagram(model, diagram, target)?,
        (_, None) => return Err("`target` is missing".into()),
    };
    let slot = match opt_num(args, "y") {
        Some(y) if tool.message_sort().is_some() => {
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
            scene.sequence.as_ref().and_then(|l| l.slot_at(y))
        }
        _ => None,
    };
    let edge = change(host, "connect", |tx| {
        let edge = if tool.message_sort().is_some() {
            // What a lifeline sends after it was called belongs to that call until it answers,
            // however far below the call it is drawn: a client gives the heights in the order
            // of the interaction, not to start new activations.
            let sends = !matches!(tool, EdgeTool::Reply | EdgeTool::FoundMessage);
            if let Some(slot) = slot.filter(|_| sends) {
                let model = tx.model();
                let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
                let lane = palette::lifeline_of(model, view, source);
                sequence_edit::continue_open_bar(tx, diagram, lane, slot.y)?;
            }
            palette::connect_message(tx, diagram, tool, source, target, slot)?
        } else if anyway {
            palette::connect_anyway(tx, diagram, tool, source, target)?
        } else {
            palette::connect(tx, diagram, tool, source, target)?
        };
        if tool != EdgeTool::NoteAnchor {
            autoroute::route_new_lines(tx, diagram, &[edge])?;
        }
        let element = tx.model().view(diagram).and_then(|v| v.get(edge)).and_then(|p| p.element);
        // A reply starts where its bar ends; one given a height goes there, as dragged, and
        // its bar goes on to it.
        // One given none goes below the fragments its bar only reaches into.
        if let (EdgeTool::Reply, Some(reply)) = (tool, element) {
            match opt_num(args, "y") {
                Some(y) => sequence_edit::move_item(tx, diagram, reply, y)?,
                None => sequence_edit::reply_below_frames(tx, diagram, reply)?,
            }
        }
        // An association class's name is its class's.
        let class = match element.and_then(|e| tx.model().get(e)).map(|e| e.kind()) {
            Some(ElementKind::Association(a)) if tool == EdgeTool::AssociationClass => a.class,
            _ => None,
        };
        let transition = element.filter(|&e| {
            matches!(tx.model().get(e).map(|e| e.kind()), Some(ElementKind::Transition(_)))
        });
        let call = element.filter(|&e| {
            matches!(tx.model().get(e).map(|e| e.kind()),
                Some(ElementKind::Message(p)) if p.sort != MessageSort::Reply)
        });
        if let (Some(name), Some(element)) = (&name, transition) {
            // A transition's name is its label: trigger, guard and effect, as UML has them.
            tx.update(element, |d| {
                if let ElementKind::Transition(p) = &mut d.kind {
                    p.set_label(name);
                }
            })?;
        } else if let (Some(name), Some(element)) = (&name, call) {
            set_call(tx, element, name)?;
            // An asynchronous message named without parentheses is a signal, written bare.
            if tool == EdgeTool::AsyncMessage && !name.contains('(') {
                tx.update_presentation(diagram, edge, |p| p.style.hide_parameters = true)?;
            }
        } else if let (Some(name), Some(element)) = (&name, class.or(element)) {
            tx.rename(element, name.clone())?;
        }
        if let Some(element) = element.filter(|_| with_ends) {
            // Checked above.
            tx.update(element, |d| drop(uml::set_ends(&mut d.kind, args)))?;
        }
        if let Some(route) = &route {
            route.apply(tx, diagram, edge)?;
        }
        if let Some(element) = element.filter(|_| extend) {
            let mut kind = tx.model().element(element)?.kind().clone();
            set_extend(tx.model(), &mut kind, args).map_err(ModelError::Invalid)?;
            tx.update(element, |d| d.kind = kind)?;
        }
        Ok(edge)
    })?;
    let model = host.document().model();
    let element = model.view(diagram).and_then(|v| v.get(edge)).and_then(|p| p.element);
    Ok(Reply::json(json!({
        "presentation": edge,
        "element": element,
        "kind": element.map_or("noteAnchor", |e| kind_name(model, e)),
    })))
}

/// A constraint as written without the braces it is drawn in: `{xor}` is `xor`.
pub(super) fn unbraced(text: &str) -> String {
    text.trim().trim_start_matches('{').trim_end_matches('}').trim().to_owned()
}

/// Names the call `message` as written: `rechne(a, 2)` is the name `rechne` with the arguments
/// `a, 2`, so that the parentheses are not drawn twice.
pub(super) fn set_call(tx: &mut Tx<'_>, message: Id, text: &str) -> Result<(), ModelError> {
    let (name, arguments) = umlcraft_notation::sequence::split_call(text);
    tx.rename(message, name.to_owned())?;
    if let Some(arguments) = arguments {
        tx.update(message, |d| {
            if let ElementKind::Message(p) = &mut d.kind {
                p.arguments = arguments.to_owned();
            }
        })?;
    }
    Ok(())
}

pub(super) fn add_member(host: &mut dyn Host, args: &Value) -> Outcome {
    let owner = id_arg(args, "owner")?;
    let text = str_arg(args, "text")?.trim().to_owned();
    let model = host.document().model();
    let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
    // A use case lists its extension points, which «extend»s name.
    if matches!(owner_kind, ElementKind::UseCase(_)) {
        if text.is_empty() {
            return Err("an extension point needs a name".into());
        }
        let data = ElementData::new(text.clone(), ElementKind::ExtensionPoint);
        let point = change(host, "add_member", |tx| tx.create(owner, data))?;
        return Ok(Reply::json(json!({
            "id": point,
            "element": point,
            "kind": "extensionPoint",
            "text": text,
        })));
    }
    let is_class_member = !matches!(
        owner_kind,
        ElementKind::State(_) | ElementKind::ErEntity(_) | ElementKind::ErRelationship(_)
    );
    let (text, written_static, written_abstract) =
        if is_class_member { uml::member_flags(&text) } else { (text, false, false) };
    let is_static = uml::flag(args, "static")?.or(written_static.then_some(true));
    let is_abstract = uml::flag(args, "abstract")?.or(written_abstract.then_some(true));
    if let ElementKind::State(props) = owner_kind {
        let props = props.clone();
        return add_state_member(host, owner, props, &text);
    }
    let literal = matches!(owner_kind, ElementKind::Enumeration(_))
        && !text.contains('(')
        && !text.contains(':');
    let column = matches!(owner_kind, ElementKind::ErEntity(_) | ElementKind::ErRelationship(_));
    let data = if column {
        let key = args.get("key").and_then(Value::as_bool).unwrap_or(false);
        let (name, props) = er::parse_column(&text, key);
        ElementData::new(name, ElementKind::ErAttribute(props))
    } else if literal {
        ElementData::new(text.clone(), ElementKind::EnumerationLiteral)
    } else if text.contains('(') {
        ElementData::new("", ElementKind::operation(None))
    } else {
        ElementData::new("", ElementKind::attribute(None)).with_visibility(Visibility::Private)
    };
    if !owner_kind.can_own(&data.kind) {
        return Err(format!("a {} cannot own a {}", owner_kind.type_name(), data.kind.type_name()));
    }
    uml::set_flags(&mut data.kind.clone(), is_static, is_abstract)?;
    let member = change(host, "add_member", |tx| {
        let member = tx.create(owner, data)?;
        if !literal && !column {
            member::apply_member_text(tx, member, &text)?;
        }
        if is_static.is_some() || is_abstract.is_some() {
            // Checked above.
            tx.update(member, |d| drop(uml::set_flags(&mut d.kind, is_static, is_abstract)))?;
        }
        Ok(member)
    })?;
    let model = host.document().model();
    let shown = if literal || column { text } else { member::member_text(model, member) };
    Ok(Reply::json(json!({
        "id": member,
        "element": member,
        "kind": kind_name(model, member),
        "text": shown,
    })))
}

/// `entry / …`, `do / …` or `exit / …` sets that activity of `state`; any other text adds an
/// internal transition `trigger [guard] / effect`.
pub(super) fn add_state_member(
    host: &mut dyn Host,
    state: Id,
    mut props: umlcraft_model::StateProps,
    text: &str,
) -> Outcome {
    if let Some((label, activity)) = state_activity(text) {
        let activity = activity.to_owned();
        match label {
            "entry" => props.entry = activity,
            "do" => props.do_activity = activity,
            _ => props.exit = activity,
        }
        change(host, "add_member", |tx| tx.update(state, |d| d.kind = ElementKind::State(props)))?;
        return Ok(Reply::json(
            json!({ "id": state, "element": state, "kind": "state", "text": text }),
        ));
    }
    if text.is_empty() {
        return Err("an internal transition needs a trigger, a guard or an effect".into());
    }
    let props = InternalTransitionProps::parse(text);
    let member = change(host, "add_member", |tx| {
        tx.create(state, ElementData::new("", ElementKind::InternalTransition(props)))
    })?;
    let model = host.document().model();
    let shown = match model.get(member).map(|e| e.kind()) {
        Some(ElementKind::InternalTransition(t)) => t.label(),
        _ => String::new(),
    };
    Ok(Reply::json(json!({
        "id": member,
        "element": member,
        "kind": kind_name(model, member),
        "text": shown,
    })))
}

/// `entry / light on` as (`entry`, `light on`); likewise `do` and `exit`, in any case.
pub(super) fn state_activity(text: &str) -> Option<(&'static str, &str)> {
    let (head, rest) = text.split_once('/')?;
    let label =
        ["entry", "do", "exit"].into_iter().find(|l| head.trim().eq_ignore_ascii_case(l))?;
    Some((label, rest.trim()))
}

/// The `entry`, `do` and `exit` arguments of a state.
pub(super) fn set_state_activities(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    for key in ["entry", "do", "exit"] {
        let Some(text) = opt_str(args, key) else { continue };
        let ElementKind::State(p) = kind else {
            return Err(format!("a {} has no `{key}`", kind.type_name()));
        };
        let text = text.trim().to_owned();
        match key {
            "entry" => p.entry = text,
            "do" => p.do_activity = text,
            _ => p.exit = text,
        }
    }
    Ok(())
}
