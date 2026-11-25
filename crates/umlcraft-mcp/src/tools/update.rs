//! `update_element` and the settings it shares with `create_element`.

use super::*;

pub(super) fn update_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    // A line's presentation (a note anchor has no element) takes its route.
    let model = host.document().model();
    let drawn_on = (!model.contains(id))
        .then(|| model.views().find_map(|(d, v)| v.get(id).map(|_| d)))
        .flatten();
    if let Some(diagram) = drawn_on {
        let keys = args.as_object().into_iter().flat_map(|a| a.keys().map(String::as_str));
        let other =
            keys.into_iter().find(|k| !matches!(*k, "id" | "diagram") && !line::KEYS.contains(k));
        if let Some(key) = other {
            return Err(format!("a presentation takes only a line's route, not `{key}`"));
        }
        let route = line::Route::parse(args)?.ok_or("a presentation takes only a line's route")?;
        change(host, "update_element", |tx| route.apply(tx, diagram, id))?;
        return Ok(Reply::json(json!({ "presentation": id })));
    }
    let model = host.document().model();
    let kind = model.get(id).ok_or_else(|| format!("no element {id}"))?.kind().clone();
    let name = opt_str(args, "name").map(|n| match kind {
        ElementKind::Constraint(_) => unbraced(n),
        _ => n.to_owned(),
    });
    let text = opt_str(args, "text").map(str::to_owned);
    let documentation = opt_str(args, "documentation").map(str::to_owned);
    let stereotypes: Option<Vec<String>> = args
        .get("stereotypes")
        .and_then(Value::as_array)
        .map(|s| s.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    let transition =
        matches!(kind, ElementKind::InternalTransition(_) | ElementKind::Transition(_));
    // A comment shows its text, not a name: `name` sets the text too.
    let comment = matches!(kind, ElementKind::Comment(_));
    let (name, text) = if comment { (None, text.or(name)) } else { (name, text) };
    if text.is_some()
        && !transition
        && !comment
        && !matches!(kind, ElementKind::Attribute(_) | ElementKind::Operation(_))
    {
        return Err(format!(
            "only attributes, operations, transitions and comments take `text`, not a {}",
            kind.type_name()
        ));
    }
    let constraints: Option<Vec<String>> =
        args.get("constraints").and_then(Value::as_array).map(|c| {
            c.iter()
                .filter_map(Value::as_str)
                .map(|c| c.trim().trim_start_matches('{').trim_end_matches('}').trim().to_owned())
                .filter(|c| !c.is_empty())
                .collect()
        });
    let shared = pick(args, "shared_style", &SHARED_STYLES)?;
    if shared.is_some() && !matches!(kind, ElementKind::Generalization(_)) {
        return Err(format!("a {} has no `shared_style`", kind.type_name()));
    }
    let icon = args.get("icon").and_then(Value::as_bool);
    let shown = model.shown_on(id);
    // A line's route goes to the one drawing of it that `diagram` (or the only one) names.
    let route = line::Route::parse(args)?;
    let routed = match &route {
        None => None,
        Some(_) => {
            let wanted = opt_diagram(model, args, "diagram", false)?;
            let lines: Vec<(Id, Id)> = shown
                .iter()
                .copied()
                .filter(|&(d, _)| wanted.is_none_or(|w| w == d))
                .filter(|&(d, p)| {
                    model
                        .view(d)
                        .and_then(|v| v.get(p))
                        .is_some_and(|p| matches!(p.shape, Shape::Edge { .. }))
                })
                .collect();
            match lines[..] {
                [line] => Some(line),
                [] if wanted.is_some() => {
                    return Err("`diagram` shows no line of the element".into());
                }
                [] => {
                    return Err(format!(
                        "a {} drawn as a line between nodes has a route, not this one",
                        kind.type_name()
                    ));
                }
                _ => return Err("several diagrams show the line; give `diagram`".into()),
            }
        }
    };
    if route.is_none() && args.get("diagram").is_some() {
        return Err("`diagram` goes with a line's route (`waypoints`, …)".into());
    }
    let mut settings = kind.clone();
    set_properties(model, &mut settings, args)?;
    set_diagram_settings(&mut settings, args)?;
    uml::set_ends(&mut settings, args)?;
    if let Some(classifier) = uml::classifier(model, args)? {
        uml::set_classifier(&mut settings, classifier)?;
    }
    // Robustness classes, and lifelines standing for an actor or a robustness class.
    let has_icon = match &settings {
        ElementKind::Lifeline(p) => {
            p.represents.is_some_and(|r| umlcraft_notation::sequence::type_has_icon(model, r))
        }
        _ => model.get(id).and_then(scene::robustness_icon).is_some(),
    };
    if icon.is_some() && !has_icon {
        return Err("only classes whose first stereotype is boundary, control or entity, and \
                    lifelines standing for an actor or such a class take `icon`"
            .into());
    }
    let represents_changed = match (&kind, &settings) {
        (ElementKind::Lifeline(a), ElementKind::Lifeline(b)) => a.represents != b.represents,
        _ => false,
    };
    let slots = uml::slots(args)?;
    let empty_slots = uml::empty_slots(&kind, args)?;
    // `{static}` and `{abstract}` in a member's text, or given as flags. Set after the text,
    // which changes the member on its own.
    let member = matches!(kind, ElementKind::Attribute(_) | ElementKind::Operation(_));
    let (text, written_static, written_abstract) = match text {
        Some(text) if member => {
            let (text, s, a) = uml::member_flags(&text);
            (Some(text), s, a)
        }
        text => (text, false, false),
    };
    let is_static = uml::flag(args, "static")?.or(written_static.then_some(true));
    let is_abstract = uml::flag(args, "abstract")?.or(written_abstract.then_some(true));
    let flags_later = member && text.is_some();
    if flags_later {
        uml::set_flags(&mut kind.clone(), is_static, is_abstract)?;
    } else {
        uml::set_flags(&mut settings, is_static, is_abstract)?;
    }
    set_activity_node(model, id, &mut settings, args)?;
    set_submachine(model, id, &mut settings, args)?;
    set_state_activities(&mut settings, args)?;
    if let Some(text) = &text {
        match &mut settings {
            ElementKind::InternalTransition(p) => *p = InternalTransitionProps::parse(text),
            ElementKind::Transition(p) => p.set_label(text),
            ElementKind::Comment(p) => p.body = text.clone(),
            _ => {}
        }
        // The text sets a transition's trigger, guard and effect at once; a `guard` given
        // beside it wins.
        if matches!(settings, ElementKind::Transition(_) | ElementKind::InternalTransition(_)) {
            set_properties(model, &mut settings, args)?;
        }
    }
    set_er(&mut settings, args)?;
    set_extend(model, &mut settings, args)?;
    // A message shows its parentheses once it is given arguments or a name written with them,
    // and none with `"arguments": null`, as a signal is mostly written.
    let call = matches!(&kind, ElementKind::Message(p) if p.sort != MessageSort::Reply);
    let parentheses = match (args.get("arguments"), &name) {
        _ if !call => None,
        (Some(Value::Null), _) => Some(false),
        (Some(_), _) => Some(true),
        (None, Some(name)) if name.contains('(') => Some(true),
        _ => None,
    };
    // A call named `rechne(a)` is named `rechne` with the arguments `a`, unless they are given.
    let name = match (&mut settings, name) {
        (ElementKind::Message(p), Some(name)) if p.sort != MessageSort::Reply => {
            let (call, arguments) = umlcraft_notation::sequence::split_call(&name);
            if let Some(arguments) = arguments.filter(|_| args.get("arguments").is_none()) {
                p.arguments = arguments.to_owned();
            }
            Some(call.to_owned())
        }
        (_, name) => name,
    };
    if let ElementKind::ErParticipation(p) = &kind
        && args.get("cardinality_here").is_some()
        && umlcraft_notation::er::participations(model, p.relationship).count() == 2
    {
        return Err("between two entities, the cardinality read at one is the other \
                    participation's `cardinality`; set that instead"
            .into());
    }
    let notation = pick(args, "notation", &ER_NOTATIONS)?;
    let is_er_diagram =
        matches!(&kind, ElementKind::Diagram(p) if p.diagram == DiagramKind::EntityRelationship);
    if notation.is_some() && !is_er_diagram {
        return Err(format!("a {} has no `notation`", kind.type_name()));
    }
    let identifying = match &settings {
        ElementKind::ErRelationship(p) if args.get("identifying").is_some() => Some(p.identifying),
        _ => None,
    };
    let associative = match &settings {
        ElementKind::ErEntity(p) if args.get("associative").is_some() => Some(p.associative),
        _ => None,
    };
    let is_alt = |kind: &ElementKind| matches!(kind, ElementKind::CombinedFragment(p) if p.operator == InteractionOperator::Alt);
    let becomes_alt = is_alt(&settings) && !is_alt(&kind);
    let recovered = args.get("covered").is_some();
    // A state divided into as many regions as `regions` says, as the app divides it.
    let regions = match args.get("regions") {
        None => None,
        Some(v) => {
            // Bounded: each region is an edit of its own, made on the app's main thread.
            let n = v
                .as_u64()
                .filter(|&n| (2..=MAX_REGIONS as u64).contains(&n))
                .ok_or(format!("`regions` is a number from 2 to {MAX_REGIONS}"))?;
            if !matches!(kind, ElementKind::State(_)) {
                return Err(format!("a {} has no regions", kind.type_name()));
            }
            let have = state_edit::regions_of(model, id).len();
            if (n as usize) < have {
                return Err(format!(
                    "the state has {have} regions; `delete_element` removes one of them"
                ));
            }
            Some(n as usize)
        }
    };
    let side_by_side = match args.get("side_by_side") {
        None => false,
        Some(_) if regions.is_none() => return Err("`side_by_side` goes with `regions`".into()),
        Some(v) => v.as_bool().ok_or("`side_by_side` is true or false")?,
    };
    // Only a changed kind is written back: a member's `text` changes the kind on its own (type,
    // multiplicity, return type), and `settings`, taken before, would undo that.
    let kind_changed = settings != kind;
    let sequences = sequences_near(model, id);
    change_measured(host, "update_element", |tx, measure| {
        if let Some(n) = regions {
            let diagram = shown.first().map(|&(d, _)| d);
            while state_edit::regions_of(tx.model(), id).len() < n {
                state_edit::add_region(tx, id, side_by_side, diagram)?;
            }
        }
        // Refused while entities take part in the associative entity.
        if let Some(associative) = associative {
            er::set_associative(tx, id, associative)?;
        }
        if let Some(text) = text.as_ref().filter(|_| !transition && !comment) {
            member::apply_member_text(tx, id, text)?;
        }
        if let Some(name) = name {
            tx.rename(id, name)?;
        }
        if documentation.is_some() || stereotypes.is_some() || constraints.is_some() || kind_changed
        {
            tx.update(id, |data| {
                if let Some(documentation) = documentation {
                    data.documentation = documentation;
                }
                if let Some(stereotypes) = stereotypes {
                    data.stereotypes = stereotypes;
                }
                if let Some(constraints) = constraints {
                    data.constraints = constraints;
                }
                if kind_changed {
                    data.kind = settings;
                }
            })?;
        }
        if flags_later && (is_static.is_some() || is_abstract.is_some()) {
            // Checked above.
            tx.update(id, |d| drop(uml::set_flags(&mut d.kind, is_static, is_abstract)))?;
        }
        if let Some(slots) = &slots {
            uml::set_slots(tx, id, slots)?;
        }
        // A lifeline that comes to stand for an actor becomes a stick figure.
        if represents_changed {
            sequence_edit::fit_head_to_type(tx, id)?;
        }
        for &(diagram, presentation) in &shown {
            if let Some(style) = shared {
                umlcraft_notation::shared::set_style(tx, diagram, presentation, style)?;
            }
            if let Some(icon) = icon {
                tx.update_presentation(diagram, presentation, |p| p.style.iconic = icon)?;
            }
            if let Some(shown) = empty_slots {
                tx.update_presentation(diagram, presentation, |p| {
                    p.style.hide_empty_slots = !shown;
                })?;
            }
            if let Some(shown) = parentheses {
                tx.update_presentation(diagram, presentation, |p| {
                    p.style.hide_parameters = !shown;
                })?;
            }
        }
        // A fragment turned into an alt gets its else, as in the app.
        if becomes_alt {
            palette::add_else(tx, id)?;
        }
        // Frames span the lifelines they cover once these are given.
        if recovered {
            sequence_edit::cover(tx, id)?;
        }
        // Foreign key columns move into or out of the child's key.
        if let Some(identifying) = identifying {
            er::set_identifying(tx, id, identifying)?;
        }
        if let Some(notation) = notation {
            er::set_notation(tx, id, notation, measure)?;
        }
        if let (Some(route), Some((diagram, line))) = (&route, routed) {
            route.apply(tx, diagram, line)?;
        }
        // As `tidy_all` does: a diagram that cannot be tidied stays as it was.
        for &diagram in &sequences {
            let _ = tx.attempt(|tx| sequence_edit::tidy(tx, diagram));
        }
        Ok(())
    })?;
    let model = host.document().model();
    let mut reply = summary(model, id);
    if regions.is_some() {
        reply["regions"] = json!(state_edit::regions_of(model, id));
    }
    match model.get(id).map(|e| e.kind()) {
        Some(ElementKind::Comment(p)) => reply["text"] = json!(p.body),
        Some(ElementKind::Extend(p)) => {
            reply["condition"] = json!(p.condition);
            reply["extension_points"] = json!(p.extension_points);
        }
        _ => {}
    }
    Ok(Reply::json(reply))
}

/// The sequence diagrams an update of `id` can change: the one it is, and those that show it,
/// one of its owners (an operand's fragment) or an element referring to it (a lifeline
/// standing for a class, a message calling an operation). Others stay as they are.
pub(super) fn sequences_near(model: &Model, id: Id) -> Vec<Id> {
    let mut near: std::collections::HashSet<Id> = model.ancestors(id).into_iter().collect();
    near.insert(id);
    near.extend(model.referrers(id).into_iter().map(|(r, _)| r));
    model
        .views()
        .filter(|&(d, _)| {
            model.get(d).and_then(|e| e.kind().as_diagram()) == Some(DiagramKind::Sequence)
        })
        .filter(|&(d, view)| {
            d == id
                || view.presentations().iter().any(|p| p.element.is_some_and(|e| near.contains(&e)))
        })
        .map(|(d, _)| d)
        .collect()
}

/// The condition and extension points of an «extend» from `args`.
pub(super) fn set_extend(
    model: &Model,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let condition = args.get("condition");
    let points = args.get("extension_points");
    if condition.is_none() && points.is_none() {
        return Ok(());
    }
    let ElementKind::Extend(p) = kind else {
        return Err(format!(
            "only an extend has a `condition` and `extension_points`, not a {}",
            kind.type_name()
        ));
    };
    if let Some(condition) = condition {
        p.condition = condition.as_str().ok_or("`condition` is text")?.trim().to_owned();
    }
    if let Some(points) = points {
        p.extension_points = extension_points_arg(model, p.extended, points)?;
    }
    Ok(())
}

/// The extension points of the use case `use_case`, with their ids and names.
pub(super) fn extension_points(model: &Model, use_case: Option<Id>) -> Vec<Value> {
    let Some(use_case) = use_case else { return Vec::new() };
    model
        .children(use_case)
        .iter()
        .filter(|&&c| matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ExtensionPoint)))
        .map(|&c| json!({ "id": c, "name": model.get(c).map_or("", |e| e.name()) }))
        .collect()
}

/// The extension points `value` names (ids or names) among those of the use case `use_case`,
/// for an «extend» into it.
pub(super) fn extension_points_arg(
    model: &Model,
    use_case: Id,
    value: &Value,
) -> Result<Vec<Id>, String> {
    let items = value.as_array().ok_or("`extension_points` is a list of ids or names")?;
    let points: Vec<Id> = model
        .children(use_case)
        .iter()
        .copied()
        .filter(|&c| matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ExtensionPoint)))
        .collect();
    items
        .iter()
        .map(|item| {
            let text = item.as_str().ok_or("`extension_points` is a list of ids or names")?;
            let by_id = parse_id(text).ok().filter(|id| points.contains(id));
            let by_name =
                || points.iter().copied().find(|&p| model.get(p).is_some_and(|e| e.name() == text));
            by_id.or_else(by_name).ok_or_else(|| {
                let case = model.get(use_case).map_or("", |e| e.name());
                format!(
                    "the extended use case {case} has no extension point `{text}`; \
                     `add_member` on it adds one"
                )
            })
        })
        .collect()
}

/// The most regions `update_element` divides a state into (the schema says the same).
const MAX_REGIONS: usize = 99;

pub(super) const SHARED_STYLES: [(&str, SharedStyle); 3] = [
    ("separate", SharedStyle::Separate),
    ("vertical", SharedStyle::Vertical),
    ("horizontal", SharedStyle::Horizontal),
];

pub(super) const SORTS: [(&str, MessageSort); 5] = [
    ("synchronous", MessageSort::Synchronous),
    ("asynchronous", MessageSort::Asynchronous),
    ("reply", MessageSort::Reply),
    ("create", MessageSort::Create),
    ("destroy", MessageSort::Destroy),
];

pub(super) const OPERATORS: [(&str, InteractionOperator); 12] = [
    ("alt", InteractionOperator::Alt),
    ("opt", InteractionOperator::Opt),
    ("loop", InteractionOperator::Loop),
    ("par", InteractionOperator::Par),
    ("break", InteractionOperator::Break),
    ("critical", InteractionOperator::Critical),
    ("neg", InteractionOperator::Neg),
    ("assert", InteractionOperator::Assert),
    ("strict", InteractionOperator::Strict),
    ("seq", InteractionOperator::Seq),
    ("ignore", InteractionOperator::Ignore),
    ("consider", InteractionOperator::Consider),
];

/// Writes the state machine of `args`' `submachine` into the submachine state `kind`. The
/// machine is given as itself or as one of its diagrams.
pub(super) fn set_submachine(
    model: &Model,
    id: Id,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let Some(text) = opt_str(args, "submachine") else { return Ok(()) };
    let ElementKind::State(p) = kind else {
        return Err(format!("a {} has no `submachine`", kind.type_name()));
    };
    if p.submachine.is_none() {
        return Err("only submachine states have `submachine`".into());
    }
    let given = parse_id(text.trim())?;
    let machine = match model.get(given).map(|e| e.kind()) {
        Some(ElementKind::StateMachine) => given,
        Some(k) if k.as_diagram() == Some(DiagramKind::StateMachine) => {
            model.get(given).and_then(|d| d.owner()).ok_or("the diagram has no state machine")?
        }
        _ => return Err(format!("{given} is no state machine")),
    };
    if model.ancestors(id).contains(&machine) {
        return Err("a submachine state cannot stand for the state machine it is part of".into());
    }
    p.submachine = Some(machine);
    Ok(())
}

/// Writes the settings of `args` that belong to the element's kind into `kind`: a guard, a
/// message's sort, arguments, return value and operation, a fragment's operator.
/// The settings of a diagram in `args`: what a sequence diagram numbers, shows and frames;
/// whether a state machine diagram draws simple states as UML does.
pub(super) fn set_diagram_settings(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    if let Some(v) = args.get("simple_states") {
        let on = v.as_bool().ok_or("`simple_states` is true or false")?;
        match kind {
            ElementKind::Diagram(p) if p.diagram == DiagramKind::StateMachine => {
                p.simple_states = on;
            }
            _ => return Err("only state machine diagrams take `simple_states`".into()),
        }
    }
    let keys = ["message_numbers", "flat_numbers", "return_values", "frame"];
    let Some(key) = keys.into_iter().find(|k| args.get(k).is_some()) else { return Ok(()) };
    let p = match kind {
        ElementKind::Diagram(p) if p.diagram == DiagramKind::Sequence => p,
        _ => return Err(format!("only sequence diagrams take `{key}`")),
    };
    let flag = |key: &str| -> Result<Option<bool>, String> {
        match args.get(key) {
            None => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("`{key}` is true or false")),
        }
    };
    if let Some(on) = flag("message_numbers")? {
        p.hide_message_numbers = !on;
    }
    if let Some(on) = flag("flat_numbers")? {
        p.flat_message_numbers = on;
    }
    if let Some(on) = flag("return_values")? {
        p.hide_return_values = !on;
    }
    if let Some(on) = flag("frame")? {
        p.hide_frame = !on;
    }
    Ok(())
}

pub(super) fn set_properties(
    model: &Model,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let type_name = kind.type_name();
    let refuse = |key: &str| format!("a {type_name} has no `{key}`");
    if let Some(guard) = opt_str(args, "guard") {
        let guard = guard.trim();
        let guard = guard.strip_prefix('[').and_then(|g| g.strip_suffix(']')).unwrap_or(guard);
        let guard = guard.trim().to_owned();
        match kind {
            ElementKind::Message(p) => p.guard = guard,
            ElementKind::InteractionOperand(p) => p.guard = guard,
            ElementKind::ActivityEdge(p) => p.guard = guard,
            ElementKind::Transition(p) => p.guard = guard,
            ElementKind::InternalTransition(p) => p.guard = guard,
            _ => return Err(refuse("guard")),
        }
    }
    if let Some(operator) = pick(args, "operator", &OPERATORS)? {
        let ElementKind::CombinedFragment(p) = kind else { return Err(refuse("operator")) };
        p.operator = operator;
    }
    if let Some(ids) = ids_arg(args, "covered")? {
        // Presentations stand for their lifelines.
        let lifeline = |id: Id| -> Result<Id, String> {
            let id = model.views().find_map(|(_, v)| v.get(id)?.element).unwrap_or(id);
            match model.get(id).map(|e| e.kind()) {
                Some(ElementKind::Lifeline(_)) => Ok(id),
                _ => Err(format!("{id} is no lifeline")),
            }
        };
        let covered = ids.into_iter().map(lifeline).collect::<Result<Vec<_>, _>>()?;
        match kind {
            ElementKind::CombinedFragment(p) => p.covered = covered,
            ElementKind::InteractionUse(p) => p.covered = covered,
            _ => return Err(refuse("covered")),
        }
    }
    let message_keys = ["sort", "arguments", "return_value", "operation"];
    let Some(key) = message_keys.into_iter().find(|k| args.get(k).is_some()) else {
        return Ok(());
    };
    let ElementKind::Message(p) = kind else { return Err(refuse(key)) };
    if let Some(sort) = pick(args, "sort", &SORTS)? {
        p.sort = sort;
    }
    if let Some(arguments) = opt_str(args, "arguments") {
        p.arguments = arguments.trim().to_owned();
    }
    if let Some(value) = opt_str(args, "return_value") {
        p.return_value = value.trim().to_owned();
    }
    if let Some(operation) = opt_str(args, "operation") {
        p.operation = match operation.trim() {
            "" => None,
            text => {
                let operation = parse_id(text)?;
                if !matches!(
                    model.get(operation).map(|e| e.kind()),
                    Some(ElementKind::Operation(_))
                ) {
                    return Err(format!("{operation} is no operation"));
                }
                Some(operation)
            }
        };
    }
    Ok(())
}
