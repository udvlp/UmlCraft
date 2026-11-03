//! Sequence diagrams: interactions, their messages and fragments, and how they are drawn.

use super::*;

impl Importer<'_> {
    /// Creates the interaction behind a sequence diagram: lifelines, then messages and
    /// combined fragments from top to bottom.
    pub(super) fn interaction(&mut self, owner: Id, diagram: ObjId) -> Option<Id> {
        let r = self.r;
        let collaboration = r.obj(diagram, "collaboration")?;
        let source = *r.list(collaboration, "interactions").first()?;
        let data = ElementData::new(r.name(diagram), ElementKind::Interaction);
        let interaction = self.create(owner, data, Some(source))?;

        for ownership in self.r.list(collaboration, "ownedElementOwnership") {
            let Some(role) = self.r.obj(ownership, "ownedElement") else { continue };
            if self.r.class(role) != "UClassifierRoleImp" {
                continue;
            }
            let represents = self.r.obj(role, "base").and_then(|b| self.classifier(b));
            let data = self.data(role, ElementKind::Lifeline(LifelineProps { represents }), &[]);
            self.create(interaction, data, Some(role));
        }

        // The interaction lists every message and fragment; those inside an operand belong to
        // the operand.
        let mut nested = HashSet::new();
        let mut visited = HashSet::new();
        let mut stack = self.r.list(source, "fragments");
        while let Some(fragment) = stack.pop() {
            // A damaged file may nest a fragment in itself.
            if !visited.insert(fragment) {
                continue;
            }
            for operand in self.r.list(fragment, "operands") {
                nested.extend(self.r.list(operand, "messages"));
                nested.extend(self.r.list(operand, "fragments"));
                stack.extend(self.r.list(operand, "fragments"));
            }
        }
        let top_level = |list: Vec<ObjId>| -> Vec<ObjId> {
            list.into_iter().filter(|m| !nested.contains(m)).collect()
        };
        let messages = top_level(self.r.list(source, "message"));
        let fragments = top_level(self.r.list(source, "fragments"));
        // A reply points to its call as `activator`. The interaction lists all messages.
        for message in self.r.list(source, "message") {
            if action_type(self.r, message) == Some(REPLY)
                && let Some(call) = self.r.obj(message, "activator")
            {
                self.replied.insert(call);
            }
        }
        self.interaction_items(interaction, messages, fragments);

        // Duration and time constraints belong to the collaboration and name the messages
        // they constrain.
        for ownership in self.r.list(collaboration, "ownedElementOwnership") {
            let Some(constraint) = self.r.obj(ownership, "ownedElement") else { continue };
            let class = self.r.class(constraint);
            if !matches!(class, "UDurationConstraintImp" | "UTimeConstraintImp") {
                continue;
            }
            let messages: Vec<Id> = self
                .r
                .list(constraint, "constrainedElement")
                .iter()
                .filter_map(|m| self.ids.get(m).copied())
                .filter(|id| {
                    matches!(self.model.get(*id).map(|e| e.kind()), Some(ElementKind::Message(_)))
                })
                .collect();
            let kind = match (class, &messages[..]) {
                ("UDurationConstraintImp", &[from, to]) => {
                    ElementKind::DurationConstraint(DurationConstraintProps { from, to })
                }
                ("UTimeConstraintImp", &[message]) => {
                    ElementKind::TimeConstraint(TimeConstraintProps { message })
                }
                _ => {
                    self.skip(constraint);
                    continue;
                }
            };
            let data = self.data(constraint, kind, &[]);
            self.create(interaction, data, Some(constraint));
        }
        Some(interaction)
    }

    /// Vertical position of a message or fragment on its diagram, for ordering.
    pub(super) fn top(&self, object: ObjId) -> f32 {
        let r = self.r;
        r.list(object, "presentation")
            .first()
            .and_then(|&p| {
                r.points(p, "allPoints")
                    .first()
                    .map(|pt| pt.y)
                    .or_else(|| r.obj(p, "location").and_then(|l| r.point(l)).map(|pt| pt.y))
            })
            .unwrap_or(f32::MAX)
    }

    pub(super) fn interaction_items(
        &mut self,
        container: Id,
        messages: Vec<ObjId>,
        fragments: Vec<ObjId>,
    ) {
        let mut items: Vec<(f32, bool, ObjId)> = messages
            .into_iter()
            .map(|m| (self.top(m), true, m))
            .chain(fragments.into_iter().map(|f| (self.top(f), false, f)))
            .collect();
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, is_message, object) in items {
            if is_message {
                self.message(container, object);
            } else {
                self.fragment(container, object);
            }
        }
    }

    pub(super) fn message(&mut self, container: Id, object: ObjId) {
        let r = self.r;
        let lifeline = |field| {
            r.obj(object, field).and_then(|o| self.ids.get(&o).copied()).filter(|id| {
                matches!(self.model.get(*id).map(|e| e.kind()), Some(ElementKind::Lifeline(_)))
            })
        };
        // Found and lost messages start or end at a gate instead of a lifeline; Astah's API
        // leaves that end empty instead.
        let is_gate = |field| r.obj(object, field).is_none_or(|o| r.class(o) == "UGateImp");
        let (from, to, gate) = match (lifeline("sender"), lifeline("receiver")) {
            (Some(from), Some(to)) => (from, to, MessageGate::None),
            (None, Some(to)) if is_gate("sender") => (to, to, MessageGate::Found),
            (Some(from), None) if is_gate("receiver") => (from, from, MessageGate::Lost),
            _ => {
                self.skip(object);
                return;
            }
        };
        // A termination is a destroy message from a lifeline to itself that only a cross shows.
        let shown_as_line =
            r.list(object, "presentation").iter().any(|&p| r.class(p) == "MessagePresentation");
        if action_type(r, object) == Some(DESTROY)
            && gate == MessageGate::None
            && from == to
            && !shown_as_line
        {
            let kind = ElementKind::Termination(TerminationProps { lifeline: from });
            let mut data = self.data(object, kind, &[]);
            // Astah names them Stop0, Stop1, …; nothing shows the name.
            data.name.clear();
            self.create(container, data, Some(object));
            return;
        }
        let action = r.obj(object, "action");
        let asynchronous = action.is_some_and(|a| r.flag(a, "asynchronous"));
        let sort = match action_type(r, object) {
            Some(CREATE) => MessageSort::Create,
            Some(REPLY) => MessageSort::Reply,
            Some(DESTROY) => MessageSort::Destroy,
            _ if asynchronous => MessageSort::Asynchronous,
            _ => MessageSort::Synchronous,
        };
        // Astah keeps the return value on the call; UmlCraft shows it on the reply.
        let return_value = match sort {
            MessageSort::Reply => Some(r.text(object, "returnValue"))
                .filter(|v| !v.is_empty())
                .or_else(|| r.obj(object, "activator").map(|call| r.text(call, "returnValue")))
                .unwrap_or_default(),
            _ if self.replied.contains(&object) => String::new(),
            _ => r.text(object, "returnValue"),
        };
        let arguments = action
            .map(|a| r.list(a, "actualArgument"))
            .unwrap_or_default()
            .into_iter()
            .map(|argument| r.text(argument, "value"))
            .collect::<Vec<_>>()
            .join(", ");
        let operation = action
            .and_then(|a| r.obj(a, "operation"))
            .and_then(|o| self.ids.get(&o).copied())
            .filter(|id| {
                matches!(self.model.get(*id).map(|e| e.kind()), Some(ElementKind::Operation(_)))
            });
        let guard = r.obj(object, "guard").map(|g| r.name(g)).unwrap_or_default();
        let kind = ElementKind::Message(MessageProps {
            sort,
            from,
            to,
            gate,
            operation,
            arguments,
            return_value,
            guard,
        });
        let data = self.data(object, kind, &[]);
        self.create(container, data, Some(object));
    }

    pub(super) fn fragment(&mut self, container: Id, object: ObjId) {
        let r = self.r;
        let covered: Vec<Id> =
            r.list(object, "covereds").iter().filter_map(|c| self.ids.get(c).copied()).collect();
        if r.class(object) == "UInteractionUseImp" {
            // The interaction it refers to may not exist yet; see `resolve_references`.
            let arguments = r.text(object, "argument");
            let kind = ElementKind::InteractionUse(InteractionUseProps {
                refers_to: None,
                arguments,
                covered,
            });
            let data = self.data(object, kind, &[]);
            if let Some(id) = self.create(container, data, Some(object))
                && let Some(target) = r.obj(object, "refersTo")
            {
                self.references.push((id, target));
            }
            return;
        }
        // A state invariant's condition is its name.
        if r.class(object) == "UStateInvariantImp" {
            let Some(&lifeline) = covered.first() else {
                self.skip(object);
                return;
            };
            let kind = ElementKind::StateInvariant(StateInvariantProps { lifeline });
            let data = self.data(object, kind, &[]);
            self.create(container, data, Some(object));
            return;
        }
        if r.class(object) != "UCombinedFragmentImp" {
            self.skip(object);
            return;
        }
        let operator = match r.text(object, "operator").as_str() {
            "opt" => InteractionOperator::Opt,
            "loop" => InteractionOperator::Loop,
            "par" => InteractionOperator::Par,
            "break" => InteractionOperator::Break,
            "critical" => InteractionOperator::Critical,
            "neg" => InteractionOperator::Neg,
            "assert" => InteractionOperator::Assert,
            "strict" => InteractionOperator::Strict,
            "seq" => InteractionOperator::Seq,
            "ignore" => InteractionOperator::Ignore,
            "consider" => InteractionOperator::Consider,
            _ => InteractionOperator::Alt,
        };
        let kind = ElementKind::CombinedFragment(CombinedFragmentProps { operator, covered });
        let data = self.data(object, kind, &[]);
        let Some(fragment) = self.create(container, data, Some(object)) else { return };
        for operand in self.r.list(object, "operands") {
            let guard =
                self.r.obj(operand, "guard").map(|g| self.r.text(g, "body")).unwrap_or_default();
            let kind = ElementKind::InteractionOperand(InteractionOperandProps { guard });
            let data = self.data(operand, kind, &[]);
            let Some(id) = self.create(fragment, data, Some(operand)) else { continue };
            let messages = self.r.list(operand, "messages");
            let fragments = self.r.list(operand, "fragments");
            self.interaction_items(id, messages, fragments);
        }
    }

    /// Recreates a sequence diagram with Astah's geometry: the frame, lifelines with their
    /// activation bars, messages, fragments and notes. Everything is moved by the same amount
    /// as on other diagrams, so that the content starts at the margin.
    pub(super) fn sequence_presentations(&mut self, diagram: Id, object: ObjId) {
        let r = self.r;
        let sources = self.presentations_back_to_front(object);
        let class = |p: ObjId| r.class(p);
        let is_node = |p: &ObjId| {
            !matches!(class(*p), "LabelPresentation" | "MessagePresentation")
                && r.obj(*p, "allPoints").is_none()
        };
        let min = sources
            .iter()
            .filter(|p| is_node(p))
            .filter_map(|&p| self.rect(p))
            .fold((f32::MAX, f32::MAX), |(x, y), b| (x.min(b.x), y.min(b.y)));
        let shift = Point::new((MARGIN - min.0).max(0.0), (MARGIN - min.1).max(0.0));
        let rect = |this: &Self, p: ObjId| this.rect(p).map(|b| b.translate(shift.x, shift.y));
        let element =
            |this: &Self, p: ObjId| r.obj(p, "model").and_then(|m| this.ids.get(&m)).copied();
        let mut presentation_of: HashMap<ObjId, Id> = HashMap::new();

        // The crosses of destroy messages; those of terminations are nodes of their own.
        let terminates = |this: &Self, p: ObjId| {
            matches!(
                element(this, p).and_then(|e| this.model.get(e)).map(|e| e.kind()),
                Some(ElementKind::Termination(_))
            )
        };
        let destroyed: HashSet<ObjId> = sources
            .iter()
            .filter(|&&p| class(p) == "TerminationPresentation" && !terminates(self, p))
            .filter_map(|&p| r.list(p, "servers").first().copied())
            .collect();
        // Nodes in Astah's z-order; bars and messages lie in front of them.
        for &p in &sources {
            if class(p) == "NotePresentation" {
                if let Some((_, node)) = self.node(diagram, DiagramKind::Sequence, p) {
                    let bounds = node.bounds.translate(shift.x, shift.y);
                    let mut presentation = Presentation::node(node.element, bounds);
                    presentation.style = node.style;
                    if let Some(id) = self.insert(diagram, presentation, false) {
                        presentation_of.insert(p, id);
                    }
                }
                continue;
            }
            let Some(bounds) = rect(self, p) else { continue };
            let presentation = match class(p) {
                "FramePresentation" => Presentation::node(Some(diagram), bounds),
                "ClassifierRolePresentation" => {
                    let Some(lifeline) = element(self, p).filter(|&e| {
                        matches!(
                            self.model.get(e).map(|e| e.kind()),
                            Some(ElementKind::Lifeline(_))
                        )
                    }) else {
                        continue;
                    };
                    let length = r.num(p, "lifelineLength").unwrap_or(0.0) as f32;
                    let mut presentation = Presentation::lifeline(lifeline, bounds, length);
                    if let Shape::Lifeline { destroyed: d, .. } = &mut presentation.shape {
                        *d = destroyed.contains(&p);
                    }
                    presentation.style = self.fill(p);
                    presentation.style.iconic =
                        r.g.field(p, "notationType").and_then(Value::as_i64) == Some(1);
                    let off = |field: &str| r.g.bool_field(p, field) == Some(false);
                    presentation.style.hide_instance_name = off("nameVisibility");
                    presentation.style.hide_instance_type = off("baseVisibility");
                    presentation
                }
                "CombinedFragmentPresentation" | "InteractionUsePresentation" => {
                    let Some(e) = element(self, p) else { continue };
                    self.fragment_presentation(p, e, bounds)
                }
                _ => continue,
            };
            // The diagram frame is always behind everything.
            let back = presentation.element == Some(diagram);
            if let Some(id) = self.insert(diagram, presentation, back) {
                presentation_of.insert(p, id);
            }
        }

        // Activation bars hang on their lifeline; a gate is an activation of the frame.
        let lane_of = |this: &Self, server: ObjId, presentation_of: &HashMap<ObjId, Id>| {
            let lifeline = match class(server) {
                "ActivationPresentation" => *r.list(server, "servers").first()?,
                _ => server,
            };
            presentation_of.get(&lifeline).copied().filter(|&id| {
                this.model
                    .view(diagram)
                    .and_then(|v| v.get(id))
                    .is_some_and(|l| matches!(l.shape, Shape::Lifeline { .. }))
            })
        };
        for &p in &sources {
            if class(p) != "ActivationPresentation"
                || r.obj(p, "model").is_some_and(|m| class(m) == "UGateImp")
            {
                continue;
            }
            let Some(bounds) = rect(self, p) else { continue };
            let Some(&server) = r.list(p, "servers").first() else { continue };
            let Some(lifeline) = lane_of(self, server, &presentation_of) else { continue };
            let mut presentation = Presentation::activation(lifeline, bounds.y, bounds.height);
            // Astah keeps bars of width 0 and draws them as a line.
            if let Shape::Activation { width, .. } = &mut presentation.shape {
                *width = (bounds.width.abs() < 1e-3).then_some(0.0);
            }
            presentation.style = self.fill(p);
            if let Some(id) = self.insert(diagram, presentation, false) {
                presentation_of.insert(p, id);
            }
        }

        for &p in &sources {
            if class(p) == "MessagePresentation" {
                self.message_presentation(diagram, p, shift, |this, s| {
                    lane_of(this, s, &presentation_of)
                });
            }
        }
        // State invariants, terminations and constraints lie in front of the messages.
        for &p in &sources {
            let Some(e) = element(self, p) else { continue };
            let kind = self.model.get(e).map(|e| e.kind());
            let bounds = match (class(p), kind) {
                ("StateInvariantPresentation", Some(ElementKind::StateInvariant(_)))
                | ("TerminationPresentation", Some(ElementKind::Termination(_))) => rect(self, p),
                // A duration's line keeps its x; its ends follow the messages.
                ("DurationConstraintPresentation", Some(ElementKind::DurationConstraint(_))) => {
                    let points = r.points(p, "allPoints");
                    match (points.first(), points.last()) {
                        (Some(a), Some(b)) => Some(Rect::new(
                            a.x + shift.x - 10.0,
                            a.y.min(b.y) + shift.y,
                            20.0,
                            (a.y - b.y).abs(),
                        )),
                        _ => None,
                    }
                }
                // A time constraint goes where its message's arrowhead is.
                ("TimeConstraintPresentation", Some(ElementKind::TimeConstraint(_))) => {
                    let points = r.points(p, "points");
                    match (points.first(), points.last()) {
                        (Some(&a), Some(&b)) => {
                            Some(Rect::from_corners(a, b).translate(shift.x, shift.y).inflate(5.0))
                        }
                        _ => None,
                    }
                }
                _ => continue,
            };
            let Some(bounds) = bounds else { continue };
            let mut presentation = Presentation::node(Some(e), bounds);
            presentation.style = self.fill(p);
            presentation.style.name_offset = self.constraint_label(p, shift);
            if let Some(id) = self.insert(diagram, presentation, false) {
                presentation_of.insert(p, id);
            }
        }
        for &p in &sources {
            if class(p) == "NoteAnchorPresentation" {
                self.edge(diagram, p, &presentation_of, shift);
            }
        }
        // Astah draws free texts and lines in front of the bars.
        for &p in &sources {
            if let Some(presentation) = self.free_shape(p, shift) {
                self.insert(diagram, presentation, false);
            }
        }
    }

    /// How far the label of the duration or time constraint presentation `p` lies from where
    /// UmlCraft puts it: Astah draws it at its `location` (its own place, which centers a
    /// duration's label 9 right of the line even where it crosses the line) moved by
    /// `localMovement` (by hand). None where both agree.
    fn constraint_label(&self, p: ObjId, shift: Point) -> Option<Point> {
        use umlcraft_notation::sequence::{ConstraintAnchor, constraint_label_box};
        let r = self.r;
        let label = r.obj(p, "namePresentation")?;
        let at = r.obj(label, "location").and_then(|l| r.point(l))?;
        let moved = r.obj(label, "localMovement").and_then(|l| r.point(l)).unwrap_or_default();
        let width = r.num(label, "width")? as f32;
        let anchor = match r.class(p) {
            "DurationConstraintPresentation" => {
                let points = r.points(p, "allPoints");
                let (a, b) = (points.first()?, points.last()?);
                ConstraintAnchor::Duration { x: a.x + shift.x, middle: (a.y + b.y) / 2.0 + shift.y }
            }
            "TimeConstraintPresentation" => {
                let points = r.points(p, "points");
                let (tip, end) = (points.first()?, points.last()?);
                let end = end.offset(shift.x, shift.y);
                ConstraintAnchor::Time { end, left: end.x < tip.x + shift.x }
            }
            _ => return None,
        };
        let usual = constraint_label_box(anchor, width, None);
        let by = Point::new(at.x + moved.x + shift.x - usual.x, at.y + moved.y + shift.y - usual.y);
        (by.x.abs() > 0.05 || by.y.abs() > 0.05).then_some(by)
    }

    /// A combined fragment with the offsets of its operands and its guards placed by hand, or
    /// an interaction use.
    pub(super) fn fragment_presentation(
        &self,
        p: ObjId,
        element: Id,
        bounds: Rect,
    ) -> Presentation {
        let r = self.r;
        let mut presentation = Presentation::fragment(element, bounds);
        if r.class(p) == "InteractionUsePresentation" {
            presentation.shape = Shape::Node { bounds, regions: None };
        } else if let Shape::Fragment { separators, guards, .. } = &mut presentation.shape {
            *separators = r
                .list(p, "operandOffsets")
                .iter()
                .filter_map(|&o| r.num(o, "value"))
                .map(|o| o as f32)
                .collect();
            *guards = r
                .list(p, "guardPresentations")
                .iter()
                .map(|&g| {
                    r.obj(g, "localMovement")
                        .and_then(|l| r.point(l))
                        .filter(|m| m.x.abs() > 1e-3 || m.y.abs() > 1e-3)
                })
                .collect();
            if guards.iter().all(Option::is_none) {
                guards.clear();
            }
        }
        presentation.style = self.fill(p);
        presentation
    }

    /// A message between the lifelines of its ends; `lane` finds the lifeline presentation
    /// behind one of Astah's servers (a lifeline or an activation bar).
    pub(super) fn message_presentation(
        &mut self,
        diagram: Id,
        p: ObjId,
        shift: Point,
        lane: impl Fn(&Self, ObjId) -> Option<Id>,
    ) {
        let r = self.r;
        let Some(element) = r.obj(p, "model").and_then(|m| self.ids.get(&m)).copied() else {
            return;
        };
        let Some(ElementKind::Message(props)) = self.model.get(element).map(|e| e.kind()) else {
            return;
        };
        let (gate, props_sort) = (props.gate, props.sort);
        let points: Vec<Point> =
            r.points(p, "allPoints").into_iter().map(|pt| pt.offset(shift.x, shift.y)).collect();
        let (Some(&first), Some(&last)) = (points.first(), points.last()) else { return };
        let servers = r.list(p, "servers");
        let ends: Vec<Option<Id>> = servers.iter().map(|&s| lane(self, s)).collect();
        let (source, target, gate_x) =
            match (gate, ends.first().copied().flatten(), ends.get(1).copied().flatten()) {
                (MessageGate::None, Some(s), Some(t)) => (s, t, None),
                (MessageGate::Found, _, Some(t)) => (t, t, Some(first.x)),
                (MessageGate::Lost, Some(s), _) => (s, s, Some(last.x)),
                _ => return,
            };
        let center = self
            .model
            .view(diagram)
            .and_then(|v| v.get(source))
            .and_then(|l| l.bounds())
            .map_or(first.x, |head| head.center().x);
        let mut presentation = Presentation::message(element, source, target, first.y);
        if let Shape::Message { end_y, gate_x: g, loop_x, labels, .. } = &mut presentation.shape {
            if (last.y - first.y).abs() > 1e-3 {
                *end_y = Some(last.y);
            }
            *g = gate_x;
            // A loop to self dragged wider or narrower than Astah's `SELF_LOOP` beyond its start.
            if source == target && gate_x.is_none() && points.len() == 4 {
                let width = points[1].x - points[0].x;
                if (width - SELF_LOOP).abs() > 0.5 {
                    *loop_x = Some(points[1].x - center);
                }
            }
            // A label moved by hand, relative to the sender's lifeline at the message's height.
            // Astah draws it `localMovement` away from its own place: across from the stored
            // `location`, which older versions left stale in height, and up from the label's
            // place above the line.
            labels.name = r.obj(p, "namePresentation").and_then(|label| {
                let moved = r.obj(label, "localMovement").and_then(|l| r.point(l))?;
                if moved.x.abs() < 1e-3 && moved.y.abs() < 1e-3 {
                    return None;
                }
                let at = r.obj(label, "location").and_then(|l| r.point(l))?;
                // Astah centers the text in the stored box by Java's whole-pixel width; the box
                // is as wide as the exact width (newer files) or the whole-pixel one (older).
                // Labels broken into several lines are left as they are.
                let text = r.text(label, "label");
                let one_line = r.num(label, "height") < Some(20.0);
                let stored = r.num(label, "width").filter(|_| one_line).map(|w| w as f32);
                let java = umlcraft_notation::metrics::java_width(&text, scene::FONT_SIZE, false);
                let inset = stored.zip(java).map_or(0.0, |(w, j)| (w - j) / 2.0);
                Some(Point::new(
                    at.x + moved.x + inset + shift.x - center,
                    moved.y - umlcraft_notation::sequence::LABEL_ABOVE,
                ))
            });
        }
        presentation.style = self.fill(p);
        // Astah hides the parentheses of every reply; only for calls it is a choice.
        let flag = |name| r.g.bool_field(p, name);
        let call = props_sort != MessageSort::Reply;
        presentation.style.hide_parameters = call && flag("parameterVisibility") == Some(false);
        presentation.style.hide_parameter_types =
            call && flag("parameterTypeVisibility") == Some(false);
        presentation.style.show_return_type = call && flag("returnValueVisibility") == Some(true);
        self.insert(diagram, presentation, false);
    }
}
