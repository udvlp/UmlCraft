//! Exports the sample project through the installed Astah and reads the file back with
//! UmlCraft's own importer. Skipped when no Astah installation with the API is found.

use std::collections::{BTreeMap, BTreeSet};

use umlcraft_asta_export::Astah;
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Color, CommentProps, DiagramKind, DirectedProps, Document,
    ElementData, ElementKind, ExtendProps, Id, IncludeProps, InteractionUseProps, LifelineProps,
    LineShape, MessageGate, MessageSort, Model, OperationProps, ParameterDirections, Point,
    Qualifier, Rect, Shape, Style, TemplateParameterKind, TemplateParameterProps, TypeExpr,
    TypeRef, Visibility,
};
use umlcraft_notation::scene::{self, ApproxMeasure, Preview};
use umlcraft_notation::sequence_edit;

/// The sample project plus what it lacks: an interface with a realization, a dependency, an
/// enumeration, a note, include and extend.
fn sample() -> Document {
    let (mut doc, ids) = order_system();
    let root = doc.model().root();
    let use_cases = ids.use_case_diagram;
    let place_order = doc.model().elements().find(|e| e.name() == "Place order").unwrap().id();
    doc.edit("extend sample", |tx| {
        let domain = ids.domain;
        let priced = tx.create(domain, ElementData::new("Priced", ElementKind::interface()))?;
        let realization = tx.create(
            domain,
            ElementData::new(
                "",
                ElementKind::Realization(DirectedProps { client: ids.order, supplier: priced }),
            ),
        )?;
        let dependency = tx.create(
            domain,
            ElementData::new(
                "",
                ElementKind::Dependency(DirectedProps { client: ids.premium, supplier: ids.order }),
            ),
        )?;
        let status = tx.create(
            domain,
            ElementData::new(
                "Status",
                ElementKind::Enumeration(umlcraft_model::ClassifierProps::default()),
            ),
        )?;
        tx.create(status, ElementData::new("OPEN", ElementKind::EnumerationLiteral))?;
        let note = tx.create(
            domain,
            ElementData::new(
                "",
                ElementKind::Comment(CommentProps {
                    body: "Orders are never deleted.".into(),
                    annotated: vec![ids.order],
                }),
            ),
        )?;
        let d = ids.class_diagram;
        let node = |tx: &umlcraft_model::Tx, element: Id| {
            tx.model().view(d).unwrap().presentations_of(element).next().unwrap().id
        };
        let n_order = node(tx, ids.order);
        let n_premium = node(tx, ids.premium);
        let n_priced = tx.add_node(d, Some(priced), Rect::new(640.0, 80.0, 120.0, 60.0))?;
        let n_status = tx.add_node(d, Some(status), Rect::new(380.0, 260.0, 120.0, 70.0))?;
        let n_note = tx.add_node(d, Some(note), Rect::new(640.0, 260.0, 150.0, 50.0))?;
        // A curve and a line with right angles.
        let curve =
            tx.add_edge(d, Some(realization), n_order, n_priced, vec![Point::new(560.0, 40.0)])?;
        tx.update_presentation(d, curve, |p| p.style.line_shape = LineShape::Curve)?;
        let square = tx.add_edge(
            d,
            Some(dependency),
            n_premium,
            n_order,
            vec![Point::new(300.0, 290.0), Point::new(300.0, 140.0)],
        )?;
        tx.update_presentation(d, square, |p| p.style.line_shape = LineShape::RightAngle)?;
        tx.add_edge(d, None, n_note, n_order, vec![])?;
        let _ = n_status;
        // A line aimed off center, a role placed by hand and an ordered end.
        let line = node(tx, ids.order_customer);
        tx.update_presentation(d, line, |p| {
            if let Shape::Edge { source_anchor, labels, .. } = &mut p.shape {
                *source_anchor = Some(Point::new(0.25, 0.75));
                labels.target_role = Some(Point::new(6.0, -24.0));
            }
        })?;
        tx.update(ids.order_customer, |data| {
            if let ElementKind::Association(p) = &mut data.kind {
                p.ends[0].constraints.push("ordered".into());
            }
        })?;

        let check = tx.create(root, ElementData::new("Check stock", ElementKind::use_case()))?;
        let rush = tx.create(root, ElementData::new("Rush order", ElementKind::use_case()))?;
        tx.create(place_order, ElementData::new("delivery", ElementKind::ExtensionPoint))?;
        let include = tx.create(
            root,
            ElementData::new(
                "",
                ElementKind::Include(IncludeProps { including: place_order, addition: check }),
            ),
        )?;
        let extend = tx.create(
            root,
            ElementData::new(
                "",
                ElementKind::Extend(ExtendProps {
                    extension: rush,
                    extended: place_order,
                    extension_points: Vec::new(),
                    condition: String::new(),
                }),
            ),
        )?;
        let n_place = tx.model().view(use_cases).unwrap().presentations_of(place_order).next();
        let n_place = n_place.unwrap().id;
        let n_check = tx.add_node(use_cases, Some(check), Rect::new(460.0, 60.0, 140.0, 60.0))?;
        let n_rush = tx.add_node(use_cases, Some(rush), Rect::new(460.0, 200.0, 140.0, 60.0))?;
        tx.add_edge(use_cases, Some(include), n_place, n_check, vec![])?;
        let n_extend = tx.add_edge(use_cases, Some(extend), n_rush, n_place, vec![])?;
        // A note anchored to the extend's line.
        let condition = tx.create(
            root,
            ElementData::new(
                "",
                ElementKind::Comment(CommentProps {
                    body: "Condition: rush".into(),
                    annotated: vec![extend],
                }),
            ),
        )?;
        let n_condition =
            tx.add_node(use_cases, Some(condition), Rect::new(640.0, 200.0, 120.0, 40.0))?;
        tx.add_edge(use_cases, None, n_condition, n_extend, vec![])?;

        // A second interaction that "Place order" refers to, and messages from and to gates.
        let interaction = tx.model().get(ids.sequence_diagram).unwrap().owner().unwrap();
        let pay = tx.create(place_order, ElementData::new("Pay", ElementKind::Interaction))?;
        let pay_diagram =
            tx.create(pay, ElementData::new("Pay", ElementKind::diagram(DiagramKind::Sequence)))?;
        let cashier = tx.create(
            pay,
            ElementData::new("cashier", ElementKind::Lifeline(LifelineProps::default())),
        )?;
        let n_cashier = sequence_edit::add_lifeline(tx, pay_diagram, cashier, 160.0, 40.0)?;
        let gate_message = |name: &str, lifeline: Id, gate: MessageGate| {
            let mut kind = ElementKind::message(MessageSort::Synchronous, lifeline, lifeline);
            if let ElementKind::Message(p) = &mut kind {
                p.gate = gate;
            }
            ElementData::new(name, kind)
        };
        let found = tx.create(pay, gate_message("pay", cashier, MessageGate::Found))?;
        sequence_edit::add_message(tx, pay_diagram, found, n_cashier, n_cashier, 120.0)?;
        let reference = tx.create(
            interaction,
            ElementData::new(
                "",
                ElementKind::InteractionUse(InteractionUseProps {
                    refers_to: Some(pay),
                    arguments: "total".into(),
                    covered: Vec::new(),
                }),
            ),
        )?;
        let below = sequence_edit::next_y(tx.model(), ids.sequence_diagram);
        sequence_edit::add_fragment(tx, ids.sequence_diagram, reference, &[], below)?;
        let n_customer_in_sequence = tx
            .model()
            .view(ids.sequence_diagram)
            .unwrap()
            .presentations_of(ids.customer_lifeline)
            .next()
            .unwrap()
            .id;
        let lost = tx.create(
            interaction,
            gate_message("notify", ids.customer_lifeline, MessageGate::Lost),
        )?;
        let below = sequence_edit::next_y(tx.model(), ids.sequence_diagram);
        let n = n_customer_in_sequence;
        sequence_edit::add_message(tx, ids.sequence_diagram, lost, n, n, below)?;

        // Display options: a label moved by hand, a message without its arguments, a diagram
        // without message numbers.
        let message = |model: &Model, name: &str| {
            let view = model.view(ids.sequence_diagram).unwrap();
            view.presentations()
                .iter()
                .find(|p| {
                    matches!(p.shape, Shape::Message { .. })
                        && p.element.is_some_and(|e| model.get(e).unwrap().name() == name)
                })
                .unwrap()
                .id
        };
        let add_item = message(tx.model(), "addItem");
        let submit = message(tx.model(), "submit");
        sequence_edit::place_label(tx, ids.sequence_diagram, add_item, Point::new(200.0, 110.0))?;
        tx.update_presentation(ids.sequence_diagram, submit, |p| p.style.hide_parameters = true)?;
        tx.update_presentation(ids.sequence_diagram, add_item, |p| {
            p.style.hide_parameter_types = true;
            p.style.show_return_type = true;
        })?;
        tx.update(pay_diagram, |data| {
            if let ElementKind::Diagram(p) = &mut data.kind {
                p.hide_message_numbers = true;
            }
        })?;

        // A fragment with two operands below everything, a call in each.
        let submit_element =
            tx.model().view(ids.sequence_diagram).unwrap().get(submit).unwrap().element.unwrap();
        let d = ids.sequence_diagram;
        let below = sequence_edit::next_y(tx.model(), d);
        let (alt, alt_node) = umlcraft_notation::palette::create_fragment(
            tx,
            d,
            umlcraft_notation::palette::NodeTool::CombinedFragment,
            None,
            Vec::new(),
            Some(Point::new(0.0, below)),
        )?;
        // Astah allows a second operand for alt, not for the default opt.
        tx.update(alt, |data| {
            if let ElementKind::CombinedFragment(p) = &mut data.kind {
                p.operator = umlcraft_model::InteractionOperator::Alt;
            }
        })?;
        umlcraft_notation::palette::add_operand(tx, alt)?;
        let Shape::Fragment { bounds, separators, .. } =
            tx.model().view(d).unwrap().get(alt_node).unwrap().shape.clone()
        else {
            unreachable!("a fragment")
        };
        let submitted =
            tx.model().view(d).unwrap().presentations_of(submit_element).next().unwrap();
        let Shape::Message { source, target, .. } = submitted.shape else {
            unreachable!("a message")
        };
        let order = tx.model().view(d).unwrap().get(target).unwrap().element.unwrap();
        let kind = ElementKind::message(MessageSort::Asynchronous, ids.customer_lifeline, order);
        let cancel = tx.create(interaction, ElementData::new("cancel", kind))?;
        sequence_edit::add_message(tx, d, cancel, source, target, bounds.y + separators[0] + 20.0)?;
        let retry = tx.create(
            interaction,
            ElementData::new("retry", tx.model().get(cancel).unwrap().kind().clone()),
        )?;
        sequence_edit::add_message(tx, d, retry, source, target, bounds.y + 45.0)?;

        // An orthogonal state with two regions side by side, and an internal transition.
        let d = ids.state_diagram;
        let machine = tx.model().get(d).unwrap().owner().unwrap();
        let state = |name: &str| ElementData::new(name, ElementKind::State(Default::default()));
        let packing = tx.create(machine, state("Packing"))?;
        let n_packing = tx.add_node(d, Some(packing), Rect::new(480.0, 260.0, 300.0, 160.0))?;
        let wrapping = tx.create(packing, state("Wrapping"))?;
        let labeling = tx.create(packing, state("Labeling"))?;
        tx.add_node(d, Some(wrapping), Rect::new(500.0, 320.0, 80.0, 40.0))?;
        tx.add_node(d, Some(labeling), Rect::new(660.0, 320.0, 80.0, 40.0))?;
        umlcraft_notation::state_edit::add_region(tx, packing, true, None)?;
        umlcraft_notation::state_edit::move_region_line(tx, d, n_packing, 0, 150.0)?;
        let submitted = tx.model().elements().find(|e| e.name() == "Submitted").unwrap().id();
        let retry = umlcraft_notation::state_edit::add_internal_transition(tx, submitted, "retry")?;
        tx.update(retry, |data| {
            if let ElementKind::InternalTransition(p) = &mut data.kind {
                p.guard = "tries < 3".into();
                p.effect = "recheck".into();
            }
        })?;

        // Object nodes in a state, typed and untyped.
        let d = ids.activity_diagram;
        let activity = tx.model().get(d).unwrap().owner().unwrap();
        for (name, type_ref, in_state, x) in [
            ("order", Some(TypeRef::Element(ids.order)), "filled", 40.0),
            ("", Some(TypeRef::Element(ids.order)), "paid, shipped", 160.0),
            ("draft", None, "loose", 280.0),
        ] {
            let kind = ElementKind::ActivityNode(ActivityNodeProps {
                node: ActivityNodeKind::Object,
                type_ref,
                in_state: in_state.into(),
            });
            let object = tx.create(activity, ElementData::new(name, kind))?;
            tx.add_node(d, Some(object), Rect::new(x, 520.0, 100.0, 40.0))?;
        }
        Ok(())
    })
    .unwrap();
    doc
}

fn name(model: &Model, id: Id) -> String {
    model.get(id).map(|e| e.name().to_owned()).unwrap_or_default()
}

fn path(model: &Model, id: Id) -> String {
    let mut names: Vec<String> = model.ancestors(id).iter().map(|&a| name(model, a)).collect();
    names.reverse();
    names.join("/")
}

fn type_text(model: &Model, t: &Option<TypeRef>) -> String {
    umlcraft_notation::member::type_label(model, t).unwrap_or_default()
}

/// Named elements with their place in the ownership tree and their essentials.
fn tree(model: &Model) -> BTreeSet<String> {
    let mut lines = BTreeSet::new();
    for e in model.elements() {
        let detail = match e.kind() {
            ElementKind::Attribute(p) => format!(" : {}", type_text(model, &p.type_ref)),
            ElementKind::Operation(p) => format!("() : {}", type_text(model, &p.return_type)),
            ElementKind::Lifeline(p) => {
                format!(" : {}", p.represents.map(|r| name(model, r)).unwrap_or_default())
            }
            ElementKind::ActivityNode(p) => {
                format!(" {:?} : {} [{}]", p.node, type_text(model, &p.type_ref), p.in_state)
            }
            ElementKind::State(p) => {
                format!(" entry {:?} do {:?} exit {:?}", p.entry, p.do_activity, p.exit)
            }
            ElementKind::Pseudostate(p) => format!(" {:?}", p.pseudostate),
            ElementKind::InternalTransition(p) => format!(" {}", p.label()),
            ElementKind::Region => String::new(),
            // Astah's API creates every instance as an object; the classifier comes back.
            ElementKind::Instance(p) => {
                format!(" : {}", p.classifier.map(|c| name(model, c)).unwrap_or_default())
            }
            ElementKind::Slot(p) => {
                let attribute = p.attribute.map(|a| path(model, a)).unwrap_or_default();
                format!(" = {} ({attribute})", p.value)
            }
            ElementKind::Component(_) | ElementKind::Artifact(_) | ElementKind::Node(_) => {
                String::new()
            }
            ElementKind::StateMachine
            | ElementKind::FinalState
            | ElementKind::Activity
            | ElementKind::Partition
            | ElementKind::Package
            | ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::EnumerationLiteral
            | ElementKind::Actor(_)
            | ElementKind::UseCase(_)
            | ElementKind::ExtensionPoint
            | ElementKind::Interaction
            | ElementKind::Diagram(_) => String::new(),
            _ => continue,
        };
        lines.insert(format!(
            "{}/{} {}{detail}",
            path(model, e.id()),
            e.kind().type_name(),
            e.name()
        ));
    }
    lines
}

/// Relationships by the names of what they connect.
fn relationships(model: &Model) -> BTreeSet<String> {
    let n = |id: Id| name(model, id);
    model
        .elements()
        .filter_map(|e| {
            Some(match e.kind() {
                ElementKind::Association(p) => {
                    let end = |i: usize| {
                        let end = &p.ends[i];
                        let multiplicity =
                            end.multiplicity.map(|m| m.to_string()).unwrap_or_default();
                        format!(
                            "{}:{}[{multiplicity}] {:?} {:?} {:?}",
                            end.role,
                            n(end.participant),
                            end.navigability,
                            end.aggregation,
                            end.constraints
                        )
                    };
                    format!("association {} — {}", end(0), end(1))
                }
                // Which association a link instantiates, the API cannot say.
                ElementKind::Link(p) => {
                    let end = |i: usize| {
                        let end = &p.ends[i];
                        format!(
                            "{}:{} {:?} {:?}",
                            end.role,
                            n(end.instance),
                            end.navigability,
                            end.aggregation
                        )
                    };
                    format!("link {:?} {} — {}", e.name(), end(0), end(1))
                }
                ElementKind::Generalization(p) => {
                    format!("generalization {} → {}", n(p.specific), n(p.general))
                }
                ElementKind::Realization(p) => {
                    format!("realization {} → {}", n(p.client), n(p.supplier))
                }
                ElementKind::Dependency(p) => {
                    format!("dependency {} → {}", n(p.client), n(p.supplier))
                }
                ElementKind::Include(p) => {
                    format!("include {} → {}", n(p.including), n(p.addition))
                }
                ElementKind::Extend(p) => format!("extend {} → {}", n(p.extension), n(p.extended)),
                ElementKind::ActivityEdge(p) => {
                    format!("flow {:?} {} → {} [{}]", e.name(), n(p.source), n(p.target), p.guard)
                }
                ElementKind::Transition(p) => {
                    format!(
                        "transition {:?} {} → {} {}",
                        e.name(),
                        n(p.source),
                        n(p.target),
                        p.label()
                    )
                }
                ElementKind::Comment(p) => {
                    let annotated: Vec<String> = p.annotated.iter().map(|&a| n(a)).collect();
                    format!("comment {:?} on {}", p.body, annotated.join(", "))
                }
                _ => return None,
            })
        })
        .collect()
}

/// What the class, use case, activity and state machine diagrams show.
#[derive(Default)]
struct Contents {
    /// Nodes with their position, edges by the elements they connect.
    lines: BTreeSet<String>,
    /// Each node's size as drawn.
    sizes: BTreeMap<String, (f32, f32)>,
    /// Each edge's anchors (source, target), then its labels (name, source role and
    /// multiplicity, target role and multiplicity).
    placements: BTreeMap<String, [Option<Point>; 7]>,
}

fn diagram_contents(model: &Model) -> Contents {
    let mut contents = Contents::default();
    let kinds = [
        Some(DiagramKind::Class),
        Some(DiagramKind::UseCase),
        Some(DiagramKind::Activity),
        Some(DiagramKind::StateMachine),
        // These come back as class diagrams, which the lines do not tell apart.
        Some(DiagramKind::Component),
        Some(DiagramKind::Deployment),
    ];
    for d in model.elements().filter(|e| kinds.contains(&e.kind().as_diagram())) {
        let scene = scene::build(model, d.id(), &Preview::default(), &ApproxMeasure);
        let view = model.view(d.id()).unwrap();
        // Lines (a note may hang on one) by what they connect.
        let label = |presentation: Id| {
            let p = view.get(presentation);
            let element = p.and_then(|p| p.element);
            let text = element.map(|e| name(model, e)).unwrap_or_default();
            match p.map(|p| &p.shape) {
                Some(Shape::Edge { source, target, .. }) => {
                    let end = |id: &Id| view.get(*id).and_then(|p| p.element);
                    let end = |id: &Id| end(id).map(|e| name(model, e)).unwrap_or_default();
                    format!("({} → {})", end(source), end(target))
                }
                _ => text,
            }
        };
        for p in view.presentations() {
            let element = p.element.and_then(|e| model.get(e));
            let what = element.map_or("", |e| e.kind().type_name());
            let shown = element.map(|e| e.name().to_owned()).unwrap_or_default();
            let line = match &p.shape {
                Shape::Node { .. } => {
                    // Use cases from Astah widen around their center for a name ApproxMeasure
                    // takes wider than Arial; their stored place is what the export kept.
                    let stored = matches!(element.map(|e| e.kind()), Some(ElementKind::UseCase(_)));
                    let r = match scene.node(p.id) {
                        Some(n) if !stored => n.rect,
                        _ => p.bounds().unwrap(),
                    };
                    // Astah places the lines between regions itself; their direction stays.
                    let regions = p.regions().map(|l| format!(" regions vertical {}", l.vertical));
                    let line = format!(
                        "{}: {what} {shown:?} at {:.0},{:.0}{}",
                        d.name(),
                        r.x,
                        r.y,
                        regions.unwrap_or_default()
                    );
                    contents.sizes.insert(line.clone(), (r.width, r.height));
                    line
                }
                Shape::Edge { source, target, waypoints, source_anchor, target_anchor, labels } => {
                    // Astah routes the first and last segment of a loop itself, at right angles.
                    // Right angles are compared as drawn.
                    let drawn = scene.edge(p.id).map(|e| e.waypoints().to_vec());
                    let (points, shape): (Vec<String>, _) = if source == target {
                        (vec![format!("loop of {}", waypoints.len())], None)
                    } else {
                        let waypoints = drawn.as_ref().unwrap_or(waypoints);
                        let points = waypoints.iter().map(|w| format!("{:.0},{:.0}", w.x, w.y));
                        (points.collect(), Some(p.style.line_shape))
                    };
                    let line = format!(
                        "{}: {what} {} → {} via [{}] {shape:?}",
                        d.name(),
                        label(*source),
                        label(*target),
                        points.join(" "),
                    );
                    let placed = [
                        *source_anchor,
                        *target_anchor,
                        labels.name,
                        labels.source_role,
                        labels.source_multiplicity,
                        labels.target_role,
                        labels.target_multiplicity,
                    ];
                    contents.placements.insert(line.clone(), placed);
                    line
                }
                Shape::Text { bounds, text } => {
                    format!("{}: text {text:?} at {:.0},{:.0}", d.name(), bounds.x, bounds.y)
                }
                Shape::Line { from, to, dashed, start_arrow, end_arrow } => format!(
                    "{}: line {:.0},{:.0} → {:.0},{:.0} dashed {dashed} arrows {start_arrow} \
                     {end_arrow}",
                    d.name(),
                    from.x,
                    from.y,
                    to.x,
                    to.y
                ),
                // Only on sequence diagrams.
                Shape::Lifeline { .. }
                | Shape::Activation { .. }
                | Shape::Message { .. }
                | Shape::Fragment { .. } => {
                    continue;
                }
            };
            contents.lines.insert(line);
        }
    }
    contents
}

/// The interactions from top to bottom: messages, fragments and operands with their guards.
fn interactions(model: &Model) -> Vec<String> {
    fn walk(model: &Model, id: Id, depth: usize, out: &mut Vec<String>) {
        for &child in model.children(id) {
            let e = model.get(child).unwrap();
            let indent = "  ".repeat(depth);
            match e.kind() {
                ElementKind::Message(p) => {
                    out.push(format!(
                        "{indent}{:?} {} → {} {:?} returns {:?} {:?}",
                        p.sort,
                        name(model, p.from),
                        name(model, p.to),
                        e.name(),
                        p.return_value,
                        p.gate
                    ));
                }
                // Astah names the frame after the interaction when it has no name of its own.
                ElementKind::InteractionUse(p) => {
                    let referred = p.refers_to.map(|r| name(model, r)).unwrap_or_default();
                    let shown =
                        if e.name().is_empty() { referred.clone() } else { e.name().into() };
                    out.push(format!("{indent}ref {shown} → {referred} ({})", p.arguments));
                }
                ElementKind::CombinedFragment(p) => {
                    out.push(format!("{indent}{:?}", p.operator));
                    walk(model, child, depth + 1, out);
                }
                ElementKind::InteractionOperand(p) => {
                    out.push(format!("{indent}[{}]", p.guard));
                    walk(model, child, depth + 1, out);
                }
                _ => {}
            }
        }
    }
    let mut blocks: Vec<Vec<String>> = model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Interaction))
        .map(|e| {
            let mut out = vec![format!("{} in {}", e.name(), path(model, e.id()))];
            walk(model, e.id(), 1, &mut out);
            out
        })
        .collect();
    blocks.sort();
    blocks.concat()
}

/// Reports a test that cannot run here. With `UMLCRAFT_REQUIRE_ASTAH` set, as on a machine that
/// has Astah, a skip fails instead, so a missing installation cannot pass for a green run.
fn skip(reason: &str) {
    assert!(std::env::var_os("UMLCRAFT_REQUIRE_ASTAH").is_none(), "not skipped: {reason}");
    eprintln!("skipped: {reason}");
}

/// Exports `model` and imports the result again.
fn through_astah(model: &Model, name: &str) -> Option<Model> {
    let Some(astah) = Astah::find() else {
        skip("no Astah installation with the API found");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("umlcraft-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.asta"));
    let report = match umlcraft_asta_export::export(&astah, model, &file) {
        Err(umlcraft_asta_export::ExportError::License) => {
            skip("Astah's license check refused saving");
            std::fs::remove_dir_all(&dir).ok();
            return None;
        }
        result => result.unwrap(),
    };
    let imported = umlcraft_asta::import(&file).unwrap();
    // To look at the file in Astah: UMLCRAFT_KEEP_ASTA=<folder> cargo test …
    if let Some(keep) = std::env::var_os("UMLCRAFT_KEEP_ASTA") {
        std::fs::copy(&file, std::path::Path::new(&keep).join(file.file_name().unwrap())).unwrap();
    }
    std::fs::remove_dir_all(&dir).unwrap();
    imported.model.check().unwrap();
    assert!(report.problems.is_empty(), "{:#?}", report.problems);
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    Some(imported.model)
}

/// Sequence diagrams by name: each lifeline's head x and each message's height, both relative
/// to the leftmost head and the top of the heads, as Astah moves the whole drawing.
fn sequence_geometry(model: &Model) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for d in model.elements().filter(|e| e.kind().as_diagram() == Some(DiagramKind::Sequence)) {
        let view = model.view(d.id()).unwrap();
        let heads: Vec<(Id, Rect)> = view
            .presentations()
            .iter()
            .filter_map(|p| match p.shape {
                Shape::Lifeline { head, .. } => Some((p.element?, head)),
                _ => None,
            })
            .collect();
        let left = heads.iter().map(|h| h.1.x).fold(f32::MAX, f32::min);
        let top = heads.iter().map(|h| h.1.y).fold(f32::MAX, f32::min);
        let mut lines: Vec<String> = heads
            .iter()
            .map(|(e, h)| format!("lifeline {} at {:.0}", name(model, *e), h.x - left))
            .collect();
        for p in view.presentations() {
            if let Shape::Message { y, labels, .. } = p.shape {
                let m = p.element.map(|e| name(model, e)).unwrap_or_default();
                let label = labels.name.map(|l| format!(" label {:.0},{:.0}", l.x, l.y));
                lines.push(format!(
                    "message {m:?} at {:.0}{} parameters {} types {} return {}",
                    y - top,
                    label.unwrap_or_default(),
                    !p.style.hide_parameters,
                    !p.style.hide_parameter_types,
                    p.style.show_return_type
                ));
            }
        }
        for p in view.presentations() {
            if let Shape::Fragment { bounds, separators, .. } = &p.shape {
                lines.push(format!(
                    "fragment at {:.0},{:.0} size {:.0}x{:.0} separators {:?}",
                    bounds.x - left,
                    bounds.y - top,
                    bounds.width,
                    bounds.height,
                    separators.iter().map(|s| s.round()).collect::<Vec<_>>()
                ));
            }
        }
        if let ElementKind::Diagram(p) = d.kind() {
            lines.push(format!(
                "numbers {} returns {}",
                !p.hide_message_numbers, !p.hide_return_values
            ));
        }
        lines.sort();
        result.insert(d.name().to_owned(), lines);
    }
    result
}

/// Interactions usually live in a package in Astah; the sample's belongs to a use case.
#[test]
fn sequence_diagram_in_a_package() {
    let (mut doc, ids) = order_system();
    let interaction = doc.model().get(ids.sequence_diagram).unwrap().owner().unwrap();
    doc.edit("move", |tx| tx.move_element(interaction, ids.domain, 0)).unwrap();
    let Some(back) = through_astah(doc.model(), "sequence") else { return };
    pretty_assertions::assert_eq!(interactions(&back), interactions(doc.model()));
}

/// Astah draws a system boundary as a rectangle with its name as a text; both come back as the
/// boundary.
#[test]
fn system_boundary_through_astah() {
    let (mut doc, ids) = order_system();
    let d = ids.use_case_diagram;
    let bounds = Rect::new(180.0, 60.0, 300.0, 360.0);
    doc.edit("boundary", |tx| {
        let owner = tx.model().get(d).unwrap().owner().unwrap();
        let boundary = tx.create(owner, ElementData::new("Shop", ElementKind::SystemBoundary))?;
        tx.add_node(d, Some(boundary), bounds)
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "boundary") else { return };
    let boundary = back
        .elements()
        .find(|e| matches!(e.kind(), ElementKind::SystemBoundary))
        .expect("the boundary comes back");
    assert_eq!(boundary.name(), "Shop");
    let diagram = back.elements().find(|e| e.name() == "Use cases").unwrap().id();
    let shown = back.view(diagram).unwrap().presentations_of(boundary.id()).next().unwrap();
    let Shape::Node { bounds: got, .. } = shown.shape else { panic!() };
    assert!((got.width - bounds.width).abs() < 0.01 && (got.height - bounds.height).abs() < 0.01);
}

#[test]
fn hidden_slots_through_astah() {
    let (mut doc, ids) = order_system();
    let d = ids.object_diagram;
    let node = doc.model().view(d).unwrap().presentations()[0].id;
    doc.edit("hide", |tx| tx.update_presentation(d, node, |p| p.style.hide_attributes = true))
        .unwrap();
    let Some(back) = through_astah(doc.model(), "slots") else { return };
    let hidden: Vec<bool> = back
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Instance(_)))
        .flat_map(|e| back.shown_on(e.id()))
        .map(|(d, n)| back.view(d).unwrap().get(n).unwrap().style.hide_attributes)
        .collect();
    assert_eq!(hidden.iter().filter(|&&h| h).count(), 1, "{hidden:?}");
}

#[test]
fn bound_classes_through_astah() {
    let (mut doc, ids) = order_system();
    let d = ids.class_diagram;
    doc.edit("bound", |tx| {
        let owner = tx.model().get(d).unwrap().owner().unwrap();
        let pair = tx.create(owner, ElementData::new("Pair", ElementKind::class()))?;
        for name in ["T1", "T2"] {
            let parameter = ElementKind::TemplateParameter(TemplateParameterProps::default());
            tx.create(pair, ElementData::new(name, parameter))?;
        }
        let holder = tx.create(owner, ElementData::new("Holder", ElementKind::class()))?;
        let binding = |arguments| {
            Some(TypeRef::Expr(Box::new(TypeExpr {
                base: TypeRef::Element(pair),
                arguments,
                prefix: String::new(),
                modifier: String::new(),
            })))
        };
        let mut bound = Vec::new();
        for (name, arguments) in [
            ("", vec![TypeRef::Element(holder), TypeRef::Text("int".into())]),
            ("HolderPair", vec![TypeRef::Element(holder), TypeRef::Element(holder)]),
        ] {
            let mut kind = ElementKind::class();
            if let ElementKind::Class(p) = &mut kind {
                p.binding = binding(arguments);
            }
            let class = tx.create(owner, ElementData::new(name, kind))?;
            let mut bind = ElementData::new(
                "",
                ElementKind::Dependency(DirectedProps { client: class, supplier: pair }),
            );
            bind.stereotypes = vec!["bind".into()];
            bound.push((class, tx.create(owner, bind)?));
        }
        let n_pair = tx.add_node(d, Some(pair), Rect::new(500.0, 40.0, 80.0, 50.0))?;
        tx.add_node(d, Some(holder), Rect::new(500.0, 300.0, 80.0, 50.0))?;
        for (i, (class, bind)) in bound.into_iter().enumerate() {
            let y = 120.0 + 90.0 * i as f32;
            let node = tx.add_node(d, Some(class), Rect::new(700.0, y, 140.0, 50.0))?;
            tx.add_edge(d, Some(bind), node, n_pair, Vec::new())?;
        }
        Ok(())
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "bound") else { return };
    use umlcraft_notation::member::{binding_text, display_name};
    let shown: BTreeSet<String> = back
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Dependency(p) if e.data().stereotypes == ["bind"] => Some(format!(
                "{} {}",
                display_name(&back, p.client),
                binding_text(&back, p.client, p.supplier)?
            )),
            _ => None,
        })
        .collect();
    let expected = ["HolderPair <T1->Holder,T2->Holder>", "Pair<Holder,int> <T1->Holder,T2->int>"];
    assert_eq!(shown, expected.into_iter().map(String::from).collect());
}

/// Template parameters, leaf operations and what association ends carry besides their role.
fn templates_and_ends(model: &Model) -> BTreeSet<String> {
    let label = |t: &Option<TypeRef>| type_text(model, t);
    let path = |id: Id| format!("{}/{}", path(model, id), name(model, id));
    model
        .elements()
        .filter_map(|e| {
            Some(match e.kind() {
                ElementKind::TemplateParameter(p) => format!(
                    "{} {:?} type {} bounds {:?} default {}",
                    path(e.id()),
                    p.parameter,
                    label(&p.type_ref),
                    p.bounds.iter().map(|b| label(&Some(b.clone()))).collect::<Vec<_>>(),
                    label(&p.default)
                ),
                ElementKind::Operation(p) => {
                    format!("{} : {} leaf {}", path(e.id()), label(&p.return_type), p.is_leaf)
                }
                ElementKind::Attribute(p) => {
                    let bound = matches!(p.type_ref, Some(TypeRef::Expr(_)));
                    format!("{} : {} expression {bound}", path(e.id()), label(&p.type_ref))
                }
                ElementKind::Association(p) => {
                    let ends: Vec<String> = p
                        .ends
                        .iter()
                        .map(|end| {
                            let qualifiers: Vec<String> = end
                                .qualifiers
                                .iter()
                                .map(|q| format!("{} : {}", q.name, label(&q.type_ref)))
                                .collect();
                            format!(
                                "{}:{} readOnly {} static {} default {:?} qualifiers {qualifiers:?}",
                                end.role,
                                name(model, end.participant),
                                end.is_read_only,
                                end.is_static,
                                end.default_value
                            )
                        })
                        .collect();
                    format!("association {}", ends.join(" — "))
                }
                _ => return None,
            })
        })
        .collect()
}

#[test]
fn templates_and_end_details_through_astah() {
    let mut doc = Document::new(Model::new("Templates"));
    let root = doc.model().root();
    doc.edit("templates", |tx| {
        let priced = tx.create(root, ElementData::new("Priced", ElementKind::interface()))?;
        let boxed = tx.create(root, ElementData::new("Box", ElementKind::class()))?;
        let t = tx.create(
            boxed,
            ElementData::new(
                "T",
                ElementKind::TemplateParameter(TemplateParameterProps {
                    bounds: vec![TypeRef::Element(priced)],
                    ..Default::default()
                }),
            ),
        )?;
        tx.create(
            boxed,
            ElementData::new(
                "N",
                ElementKind::TemplateParameter(TemplateParameterProps {
                    parameter: TemplateParameterKind::Value,
                    type_ref: Some(TypeRef::Text("int".into())),
                    default: Some(TypeRef::Text("10".into())),
                    ..Default::default()
                }),
            ),
        )?;
        tx.create(
            boxed,
            ElementData::new("value", ElementKind::attribute(Some(TypeRef::Element(t)))),
        )?;
        let mut get =
            OperationProps { return_type: Some(TypeRef::Element(t)), ..Default::default() };
        get.is_leaf = true;
        tx.create(boxed, ElementData::new("get", ElementKind::Operation(get)))?;
        let holder = tx.create(root, ElementData::new("Holder", ElementKind::class()))?;
        let mut kind = ElementKind::association(holder, boxed);
        if let ElementKind::Association(p) = &mut kind {
            p.ends[0].role = "owner".into();
            p.ends[0].qualifiers =
                vec![Qualifier { name: "key".into(), type_ref: Some(TypeRef::Element(priced)) }];
            p.ends[1].role = "boxes".into();
            p.ends[1].is_read_only = true;
            p.ends[1].is_static = true;
            p.ends[1].default_value = "none".into();
        }
        tx.create(root, ElementData::new("", kind))?;
        // A bound template, which the API builds from the type's text.
        let bound = |modifier: &str| {
            TypeRef::Expr(Box::new(TypeExpr {
                base: TypeRef::Element(boxed),
                arguments: vec![TypeRef::Element(holder), TypeRef::Text("20".into())],
                prefix: String::new(),
                modifier: modifier.into(),
            }))
        };
        tx.create(holder, ElementData::new("held", ElementKind::attribute(Some(bound("")))))?;
        tx.create(holder, ElementData::new("rows", ElementKind::attribute(Some(bound("[]")))))?;
        Ok(())
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "templates") else { return };
    pretty_assertions::assert_eq!(templates_and_ends(&back), templates_and_ends(doc.model()));
}

#[test]
fn round_trip_through_astah() {
    let doc = sample();
    let original = doc.model();
    let Some(back) = through_astah(original, "sample") else { return };
    let back = &back;

    // Astah has no free-text types: the API turns type names into classes of the package.
    let mut expected = tree(original);
    expected.insert("Order System/domain/class Money".into());
    expected.insert("Order System/domain/class String".into());
    pretty_assertions::assert_eq!(relationships(back), relationships(original));
    let shown = diagram_contents(back);
    let expected_shown = diagram_contents(original);
    pretty_assertions::assert_eq!(shown.lines, expected_shown.lines);
    // Astah keeps UmlCraft's sizes, but may grow a node to fit its content (a use case with
    // extension points, for example). Instances it sizes by itself.
    for (node, (width, height)) in expected_shown.sizes {
        if node.contains(": instance ") {
            continue;
        }
        let (w, h) = shown.sizes[&node];
        assert!(
            (w - width).abs() < 0.5 && h > height - 0.5,
            "{node}: {w}x{h}, not {width}x{height}"
        );
    }
    // Anchors and labels set by hand come back; the importer fills in the others as well.
    for (edge, placed) in expected_shown.placements {
        let Some(back) = shown.placements.get(&edge) else { continue };
        for (i, (want, got)) in placed.iter().zip(back).enumerate() {
            let Some(want) = want else { continue };
            let tolerance = if i < 2 { 0.01 } else { 1.0 };
            let close = got.is_some_and(|g| want.distance(g) < tolerance);
            assert!(close, "{edge}: placement {i} is {got:?}, not {want:?}");
        }
    }
    pretty_assertions::assert_eq!(tree(back), expected);
    pretty_assertions::assert_eq!(interactions(back), interactions(original));
    // Astah keeps where lifelines and messages are.
    pretty_assertions::assert_eq!(sequence_geometry(back), sequence_geometry(original));
}

#[test]
fn subsystems_through_astah() {
    use umlcraft_notation::palette::{self, NodeTool, create_node};
    let (mut doc, ids) = order_system();
    let d = ids.class_diagram;
    doc.edit("subsystem", |tx| {
        create_node(tx, d, NodeTool::Subsystem, Point::new(500.0, 300.0), "Billing")
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "subsystem") else { return };
    let subsystems: Vec<_> = back.elements().filter(|e| palette::is_subsystem(e)).collect();
    assert_eq!(subsystems.len(), 1);
    let e = subsystems[0];
    assert!(matches!(e.kind(), ElementKind::Package));
    // One stereotype, not one from the import and one exported as such.
    assert_eq!(e.data().stereotypes, [palette::SUBSYSTEM]);
    assert_eq!(back.shown_on(e.id()).len(), 1);
}

#[test]
fn display_settings_through_astah() {
    let (mut doc, ids) = order_system();
    let class_node = doc.model().view(ids.class_diagram).unwrap().presentations()[0].id;
    doc.edit("settings", |tx| {
        tx.update_presentation(ids.class_diagram, class_node, |p| p.style.font_size = Some(14.0))?;
        tx.update(ids.sequence_diagram, |data| {
            if let ElementKind::Diagram(p) = &mut data.kind {
                p.flat_message_numbers = true;
                p.hide_frame = true;
            }
        })?;
        let owner = tx.model().get(ids.use_case_diagram).unwrap().owner().unwrap();
        let boundary = tx.create(owner, ElementData::new("Shop", ElementKind::SystemBoundary))?;
        let node = tx.add_node(
            ids.use_case_diagram,
            Some(boundary),
            Rect::new(400.0, 300.0, 300.0, 200.0),
        )?;
        tx.update_presentation(ids.use_case_diagram, node, |p| {
            p.style.name_offset = Some(Point::new(20.0, -30.0));
        })
    })
    .unwrap();
    // Astah's API refuses font sizes; the export reports them as dropped.
    let Some(back) = through_astah(doc.model(), "settings") else { return };
    let sequence = back
        .elements()
        .find_map(|e| match e.kind() {
            ElementKind::Diagram(p) if p.diagram == DiagramKind::Sequence => Some(*p),
            _ => None,
        })
        .unwrap();
    assert!(sequence.flat_message_numbers && sequence.hide_frame, "{sequence:?}");
    let offsets: Vec<Point> = back
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::SystemBoundary))
        .flat_map(|e| back.shown_on(e.id()))
        .filter_map(|(d, n)| back.view(d).unwrap().get(n).unwrap().style.name_offset)
        .collect();
    assert_eq!(offsets.len(), 1, "{offsets:?}");
    assert!(offsets[0].distance(Point::new(20.0, -30.0)) < 0.01, "{offsets:?}");
}

#[test]
fn member_filters_and_colors_through_astah() {
    let (mut doc, ids) = order_system();
    let d = ids.class_diagram;
    let view = doc.model().view(d).unwrap();
    let boxed = view
        .presentations()
        .iter()
        .find(|p| {
            !p.is_edge()
                && p.element.and_then(|e| doc.model().get(e)).is_some_and(|e| {
                    matches!(e.kind(), ElementKind::Class(_)) && !e.children().is_empty()
                })
        })
        .unwrap();
    let (node, class) = (boxed.id, name(doc.model(), boxed.element.unwrap()));
    let edge = view.presentations().iter().find(|p| p.is_edge() && p.element.is_some()).unwrap().id;
    let red = Color::rgb(0xff, 0, 0);
    let blue = Color::rgb(0, 0, 0xff);
    let want = |s: &mut Style| {
        s.font_color = Some(red);
        s.hide_member_visibility = true;
        s.hide_attribute_types = true;
        s.hide_initial_values = true;
        s.hide_parameter_types = true;
        s.hide_return_types = true;
        s.parameter_directions = ParameterDirections::All;
        s.hidden_attributes = vec![Visibility::Private];
        s.hidden_operations = vec![Visibility::Protected, Visibility::Package];
    };
    doc.edit("filters", |tx| {
        tx.update_presentation(d, node, |p| want(&mut p.style))?;
        tx.update_presentation(d, edge, |p| {
            p.style.stroke = Some(blue);
            p.style.font_color = Some(red);
        })
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "filters") else { return };
    let styles: Vec<Style> = back
        .elements()
        .filter(|e| e.name() == class && matches!(e.kind(), ElementKind::Class(_)))
        .flat_map(|e| back.shown_on(e.id()))
        .map(|(d, n)| back.view(d).unwrap().get(n).unwrap().style.clone())
        .collect();
    let [got] = &styles[..] else { panic!("{styles:?}") };
    let mut expected = got.clone();
    want(&mut expected);
    assert_eq!(*got, expected);
    let lines: Vec<(Option<Color>, Option<Color>)> = back
        .elements()
        .flat_map(|e| back.shown_on(e.id()))
        .map(|(d, n)| &back.view(d).unwrap().get(n).unwrap().style)
        .filter(|s| s.stroke.is_some())
        .map(|s| (s.stroke, s.font_color))
        .collect();
    assert_eq!(lines, [(Some(blue), Some(red))]);
}
