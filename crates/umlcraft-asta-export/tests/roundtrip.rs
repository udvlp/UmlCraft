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
                ..Default::default()
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
    through_astah_reporting(model, name).map(|(model, _)| model)
}

/// The same, with the export's report of what Astah did not take.
fn through_astah_reporting(
    model: &Model,
    name: &str,
) -> Option<(Model, umlcraft_asta_export::Report)> {
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
    Some((imported.model, report))
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

/// A composite structure diagram: parts, ports on the class and on parts, connectors, and a
/// provided and a required interface drawn as ball and socket.
#[test]
fn composite_structure_through_astah() {
    use umlcraft_model::ConnectorEnd;
    use umlcraft_notation::composite::PORT_SIZE;
    let (mut doc, ids) = order_system();
    let owner = ids.domain;
    let mut ports = BTreeMap::new();
    doc.edit("composite", |tx| {
        let create = |tx: &mut umlcraft_model::Tx, owner, name: &str, kind| {
            tx.create(owner, ElementData::new(name, kind))
        };
        let controller = create(tx, owner, "Controller", ElementKind::class())?;
        let reader_class = create(tx, owner, "CardReader", ElementKind::class())?;
        let lock_class = create(tx, owner, "Lock", ElementKind::class())?;
        let report = create(tx, owner, "Report", ElementKind::interface())?;
        let power = create(tx, owner, "Power", ElementKind::interface())?;
        let card = create(tx, controller, "card", ElementKind::port(None))?;
        let uplink = create(tx, controller, "uplink", ElementKind::port(None))?;
        let card_out = create(tx, reader_class, "out", ElementKind::port(None))?;
        let open = create(tx, lock_class, "open", ElementKind::port(None))?;
        let part = |of| ElementKind::attribute(Some(TypeRef::Element(of)));
        let reader = create(tx, controller, "reader", part(reader_class))?;
        let lock = create(tx, controller, "lock", part(lock_class))?;
        let provides = DirectedProps { client: uplink, supplier: report };
        let realization = create(tx, owner, "", ElementKind::Realization(provides))?;
        let requires = DirectedProps { client: card, supplier: power };
        let usage = create(tx, owner, "", ElementKind::Dependency(requires))?;

        let d = create(
            tx,
            controller,
            "Inside",
            ElementKind::diagram(DiagramKind::CompositeStructure),
        )?;
        tx.add_node(d, Some(controller), Rect::new(100.0, 40.0, 500.0, 200.0))?;
        // Astah widens a part narrower than its text in Astah's font, and its ports with it.
        let n_reader = tx.add_node(d, Some(reader), Rect::new(140.0, 120.0, 180.0, 56.0))?;
        tx.add_node(d, Some(lock), Rect::new(400.0, 120.0, 150.0, 56.0))?;
        let half = PORT_SIZE / 2.0;
        let mut port = |tx: &mut umlcraft_model::Tx, id, (x, y): (f32, f32)| {
            ports.insert(name(tx.model(), id), Point::new(x, y));
            tx.add_node(d, Some(id), Rect::new(x - half, y - half, PORT_SIZE, PORT_SIZE))
        };
        let n_card = port(tx, card, (100.0, 148.0))?;
        let n_uplink = port(tx, uplink, (600.0, 148.0))?;
        let n_out = port(tx, card_out, (320.0, 148.0))?;
        let n_open = port(tx, open, (400.0, 148.0))?;
        let icon = |tx: &mut umlcraft_model::Tx, id, x, socket| {
            let n = tx.add_node(d, Some(id), Rect::new(x, 136.0, 24.0, 24.0))?;
            tx.update_presentation(d, n, |p| {
                p.style.iconic = true;
                p.style.socket = socket;
            })?;
            Ok::<_, umlcraft_model::ModelError>(n)
        };
        let n_report = icon(tx, report, 680.0, false)?;
        let n_power = icon(tx, power, 20.0, true)?;
        tx.add_edge(d, Some(realization), n_uplink, n_report, vec![])?;
        tx.add_edge(d, Some(usage), n_card, n_power, vec![])?;
        let end = ConnectorEnd::new;
        for (a, b, n_a, n_b) in [
            (end(card, None), end(reader, None), n_card, n_reader),
            (end(card_out, Some(reader)), end(open, Some(lock)), n_out, n_open),
        ] {
            let connector = create(tx, controller, "", ElementKind::connector(a, b))?;
            tx.add_edge(d, Some(connector), n_a, n_b, vec![])?;
        }

        // Astah gives components no attributes; their parts go as association ends.
        let dock = create(tx, owner, "Dock", ElementKind::Component(Default::default()))?;
        let latch = create(tx, dock, "latch", part(lock_class))?;
        let d =
            create(tx, dock, "Dock inside", ElementKind::diagram(DiagramKind::CompositeStructure))?;
        tx.add_node(d, Some(dock), Rect::new(100.0, 40.0, 300.0, 160.0))?;
        tx.add_node(d, Some(latch), Rect::new(140.0, 100.0, 150.0, 56.0))?;
        Ok(())
    })
    .unwrap();
    let Some(back) = through_astah(doc.model(), "composite") else { return };

    let n = |id: Id| name(&back, id);
    let mut model: Vec<String> = back
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Port(_) => Some(format!("port {} of {}", e.name(), n(e.owner()?))),
            ElementKind::Attribute(p)
                if matches!(n(e.owner()?).as_str(), "Controller" | "Dock") =>
            {
                let (owner, t) = (n(e.owner()?), type_text(&back, &p.type_ref));
                Some(format!("part {} : {t} of {owner}", e.name()))
            }
            ElementKind::Association(p)
                if p.ends.iter().any(|end| n(end.participant) == "Dock") =>
            {
                Some("association of Dock".into())
            }
            ElementKind::Connector(p) => {
                let end = |i: usize| {
                    let end = &p.ends[i];
                    let with = end.part_with_port.map(|w| format!(" of {}", n(w)));
                    format!("{}{}", n(end.role), with.unwrap_or_default())
                };
                Some(format!("connector {} — {} in {}", end(0), end(1), n(e.owner()?)))
            }
            _ => None,
        })
        .collect();
    model.sort();
    pretty_assertions::assert_eq!(
        model,
        [
            "connector card — reader in Controller",
            "connector out of reader — open of lock in Controller",
            "part latch : Lock of Dock",
            "part lock : Lock of Controller",
            "part reader : CardReader of Controller",
            "port card of Controller",
            "port open of Lock",
            "port out of CardReader",
            "port uplink of Controller",
        ]
    );
    let lines = relationships(&back);
    for line in ["realization uplink → Report", "dependency card → Power"] {
        assert!(lines.contains(line), "{line} not in {lines:#?}");
    }

    let d = back.elements().find(|e| e.name() == "Inside").unwrap();
    assert_eq!(d.kind().as_diagram(), Some(DiagramKind::CompositeStructure));
    let view = back.view(d.id()).unwrap();
    let mut icons = Vec::new();
    for p in view.presentations() {
        let (Some(e), Shape::Node { bounds, .. }) = (p.element, &p.shape) else { continue };
        let element = back.get(e).unwrap();
        match element.kind() {
            ElementKind::Port(_) => {
                let at = ports[element.name()];
                assert!(bounds.center().distance(at) < 1.0, "{} at {bounds:?}", element.name());
            }
            ElementKind::Interface(_) => {
                icons.push(format!("{} {} {}", element.name(), p.style.iconic, p.style.socket));
            }
            _ => {}
        }
    }
    icons.sort();
    assert_eq!(icons, ["Power true true", "Report true false"]);
    let connectors = view
        .presentations()
        .iter()
        .filter(|p| {
            p.element
                .is_some_and(|e| matches!(back.get(e).unwrap().kind(), ElementKind::Connector(_)))
        })
        .count();
    assert_eq!(connectors, 2);
}

/// The activity elements beyond actions and control nodes: signal, time and call behavior
/// actions, pins, connectors, an activity parameter node and object node properties, made with
/// the editor's tools.
#[test]
fn activity_elements_through_astah() {
    use umlcraft_model::ObjectOrdering;
    use umlcraft_notation::edit;
    use umlcraft_notation::palette::{self, EdgeTool, NodeTool};
    let (mut doc, ids) = order_system();
    let root = doc.model().root();
    let measure = ApproxMeasure;
    let d = edit::apply(&mut doc, "diagram", &measure, |tx| {
        let called = edit::create_diagram(tx, root, DiagramKind::Activity, "Pay")?;
        let activity = tx.model().get(called).unwrap().owner().unwrap();
        let (_, _) =
            palette::create_node(tx, called, NodeTool::Action, Point::new(100.0, 100.0), "Charge")?;
        let d = edit::create_diagram(tx, root, DiagramKind::Activity, "Checkout")?;
        Ok::<_, umlcraft_model::ModelError>((d, activity))
    });
    let (d, pay) = d.unwrap();
    let mut nodes = BTreeMap::new();
    let mut add = |doc: &mut Document, tool: NodeTool, name: &str, x: f32, y: f32| {
        let (element, node) = edit::apply(doc, "node", &measure, |tx| {
            let (element, node) = palette::create_node(tx, d, tool, Point::new(x, y), name)?;
            if tool.is_named() {
                tx.rename(element, name)?;
            }
            Ok((element, node))
        })
        .unwrap();
        nodes.insert(name.to_owned(), (element, node));
        (element, node)
    };
    let (pick, pick_node) = add(&mut doc, NodeTool::Action, "Pick", 100.0, 100.0);
    let pick_rect = doc.model().view(d).unwrap().get(pick_node).unwrap().bounds().unwrap();
    add(&mut doc, NodeTool::InputPin, "order", pick_rect.x - 10.0, pick_rect.y + 2.0);
    add(&mut doc, NodeTool::OutputPin, "items", pick_rect.right() - 5.0, pick_rect.y + 2.0);
    add(&mut doc, NodeTool::ObjectNode, "cart", 300.0, 100.0);
    add(&mut doc, NodeTool::SendSignal, "Notify", 300.0, 220.0);
    add(&mut doc, NodeTool::ActivityConnector, "A", 340.0, 340.0);
    add(&mut doc, NodeTool::ActivityConnector, "B", 540.0, 100.0);
    add(&mut doc, NodeTool::AcceptEvent, "Paid", 500.0, 220.0);
    add(&mut doc, NodeTool::AcceptTimeEvent, "Wait", 700.0, 220.0);
    let (call, _) = add(&mut doc, NodeTool::CallBehavior, "Pay order", 650.0, 340.0);
    add(&mut doc, NodeTool::ActivityParameter, "input", 0.0, 120.0);
    edit::apply(&mut doc, "details", &measure, |tx| {
        let mut set = |id: Id, f: &dyn Fn(&mut ActivityNodeProps)| {
            tx.update(id, |data| {
                if let ElementKind::ActivityNode(p) = &mut data.kind {
                    f(p);
                }
            })
        };
        set(nodes["cart"].0, &|p| {
            p.ordering = Some(ObjectOrdering::Fifo);
            p.upper_bound = "3".into();
            p.is_stream = true;
        })?;
        set(nodes["order"].0, &|p| {
            p.type_ref = Some(TypeRef::Element(ids.order));
            p.ordering = Some(ObjectOrdering::Lifo);
        })?;
        set(nodes["input"].0, &|p| p.type_ref = Some(TypeRef::Element(ids.order)))?;
        set(call, &|p| p.behavior = Some(pay))?;
        let node = |name: &str| nodes[name].1;
        for (a, b) in [
            ("items", "cart"),
            ("cart", "Notify"),
            ("Notify", "A"),
            ("B", "Paid"),
            ("Paid", "Wait"),
            ("Wait", "Pay order"),
            ("input", "order"),
        ] {
            palette::connect(tx, d, EdgeTool::Flow, node(a), node(b))?;
        }
        Ok(())
    })
    .unwrap();
    // The diagram as UmlCraft draws it, to compare with what comes back.
    let drawn = scene::build(doc.model(), d, &Preview::default(), &measure);
    let pins_before: BTreeMap<String, u8> = ["order", "items"]
        .iter()
        .map(|&n| {
            let pin = drawn.node(nodes[n].1).unwrap().rect;
            let action = drawn.node(pick_node).unwrap().rect.inflate(7.5);
            (n.to_owned(), umlcraft_notation::composite::side_on(&[action], pin.center()).unwrap())
        })
        .collect();
    let _ = pick;

    let Some(back) = through_astah(doc.model(), "activity-elements") else { return };
    let n = |id: Id| name(&back, id);
    let mut lines: Vec<String> = back
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::ActivityNode(p) if nodes.contains_key(e.name()) => {
                let mut line = format!("{} {:?} in {}", e.name(), p.node, n(e.owner()?));
                if let Some(t) = &p.type_ref {
                    line += &format!(" : {}", type_text(&back, &Some(t.clone())));
                }
                for property in p.property_lines() {
                    line += &format!(" {property}");
                }
                if let Some(b) = p.behavior {
                    line += &format!(" calls {}", n(b));
                }
                Some(line)
            }
            _ => None,
        })
        .collect();
    lines.sort();
    pretty_assertions::assert_eq!(
        lines,
        [
            "A Connector in Checkout",
            "B Connector in Checkout",
            "Notify SendSignal in Checkout",
            "Paid AcceptEvent in Checkout",
            "Pay order CallBehavior in Checkout calls Pay",
            "Pick Action in Checkout",
            "Wait AcceptTimeEvent in Checkout",
            "cart Object in Checkout {ordering=FIFO} {upperBound=3} {stream}",
            "input ActivityParameter in Checkout : Order",
            "items OutputPin in Pick",
            "order InputPin in Pick : Order {ordering=LIFO}",
        ]
    );
    let flows = relationships(&back);
    for flow in [
        "flow \"\" items → cart []",
        "flow \"\" cart → Notify []",
        "flow \"\" Notify → A []",
        "flow \"\" B → Paid []",
        "flow \"\" Wait → Pay order []",
        "flow \"\" input → order []",
    ] {
        assert!(flows.contains(flow), "{flow} not in {flows:#?}");
    }

    // Pins stay on their side of the action, the parameter on the frame.
    let diagram =
        back.elements().find(|e| e.name() == "Checkout" && e.kind().as_diagram().is_some());
    let diagram = diagram.unwrap().id();
    let scene = scene::build(&back, diagram, &Preview::default(), &measure);
    let find = |name: &str| {
        scene
            .nodes
            .iter()
            .find(|node| node.element.is_some_and(|e| back.get(e).unwrap().name() == name))
            .unwrap()
    };
    for (pin, side) in &pins_before {
        let action = find("Pick").rect.inflate(7.5);
        let at = find(pin).rect.center();
        assert_eq!(umlcraft_notation::composite::side_on(&[action], at), Some(*side), "{pin}");
    }
    let frame = scene.nodes.iter().find(|node| node.element == Some(diagram)).unwrap().rect;
    let parameter = find("input").rect.center();
    assert!((parameter.x - frame.x).abs() < 1.0, "{parameter:?} on {frame:?}");
}

#[test]
fn submachine_states_through_astah() {
    use umlcraft_model::{PseudostateKind, StateProps};
    use umlcraft_notation::edit;
    use umlcraft_notation::palette::{self, EdgeTool, NodeTool};
    let mut doc = Document::new(Model::new("Cars"));
    let root = doc.model().root();
    let measure = ApproxMeasure;
    // The machine a submachine state stands for comes after it, so the export has to make its
    // diagram first.
    let (car, engine) = edit::apply(&mut doc, "diagrams", &measure, |tx| {
        let car = edit::create_diagram(tx, root, DiagramKind::StateMachine, "Car")?;
        let engine = edit::create_diagram(tx, root, DiagramKind::StateMachine, "Engine")?;
        palette::create_node(tx, engine, NodeTool::State, Point::new(100.0, 100.0), "Idle")?;
        Ok::<_, umlcraft_model::ModelError>((car, engine))
    })
    .unwrap();
    let machine = doc.model().get(engine).unwrap().owner().unwrap();
    let machine_name = doc.model().get(machine).unwrap().name().to_owned();
    let mut nodes = BTreeMap::new();
    let mut add = |doc: &mut Document, tool: NodeTool, name: &str, x: f32, y: f32| {
        let made = edit::apply(doc, "node", &measure, |tx| {
            let (element, node) = palette::create_node(tx, car, tool, Point::new(x, y), name)?;
            tx.rename(element, name)?;
            Ok((element, node))
        })
        .unwrap();
        nodes.insert(name.to_owned(), made);
        made
    };
    add(&mut doc, NodeTool::State, "Parked", 40.0, 100.0);
    let (drive, drive_node) = add(&mut doc, NodeTool::SubmachineState, "Drive", 300.0, 100.0);
    let r = doc.model().view(car).unwrap().get(drive_node).unwrap().bounds().unwrap();
    add(&mut doc, NodeTool::EntryPoint, "start", r.x - 10.0, r.y + 10.0);
    add(&mut doc, NodeTool::ExitPoint, "stop", r.right() - 10.0, r.y + 10.0);
    add(&mut doc, NodeTool::StubState, "Halt", r.x + 10.0, r.bottom() - 30.0);
    edit::apply(&mut doc, "details", &measure, |tx| {
        tx.update(drive, |data| {
            data.kind = ElementKind::State(StateProps {
                entry: "start engine".into(),
                submachine: Some(machine),
                ..StateProps::default()
            })
        })?;
        let node = |name: &str| nodes[name].1;
        for (a, b) in [("Parked", "start"), ("stop", "Parked"), ("Parked", "Drive")] {
            palette::connect(tx, car, EdgeTool::Transition, node(a), node(b))?;
        }
        Ok(())
    })
    .unwrap();
    let kinds: Vec<PseudostateKind> = ["start", "stop", "Halt"]
        .iter()
        .map(|n| match doc.model().get(nodes[*n].0).unwrap().kind() {
            ElementKind::Pseudostate(p) => p.pseudostate,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [PseudostateKind::EntryPoint, PseudostateKind::ExitPoint, PseudostateKind::StubState]
    );
    doc.model().check().unwrap();

    let Some((back, report)) = through_astah_reporting(doc.model(), "submachine-states") else {
        return;
    };
    // The API makes submachine states, but no entry and exit points and no stub states; the
    // transitions at them go too, with their lines.
    let dropped: Vec<(&str, usize)> =
        report.dropped.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(
        dropped,
        [
            ("entryPoint", 1),
            ("exitPoint", 1),
            ("presentation", 2),
            ("stubState", 1),
            ("transition", 2)
        ]
    );
    let drive = back.elements().find(|e| e.name() == "Drive").unwrap();
    let ElementKind::State(p) = drive.kind() else { panic!("{:?}", drive.kind()) };
    assert_eq!(p.entry, "start engine");
    assert_eq!(p.submachine.map(|m| name(&back, m)), Some(machine_name));
    assert!(
        relationships(&back).iter().any(|r| r.contains("Parked → Drive")),
        "{:#?}",
        relationships(&back)
    );
    // It is drawn where it was, as a submachine state.
    let diagram = back.elements().find(|e| e.name() == "Car" && e.kind().as_diagram().is_some());
    let scene = scene::build(&back, diagram.unwrap().id(), &Preview::default(), &measure);
    let node = scene.nodes.iter().find(|n| n.element == Some(drive.id())).unwrap();
    assert!(
        (node.rect.x - r.x).abs() < 1.0 && (node.rect.y - r.y).abs() < 1.0,
        "{:?} {r:?}",
        node.rect
    );
    assert!(matches!(node.figure, scene::Figure::State { submachine: Some(_), .. }));
}

#[test]
fn state_invariants_and_terminations_through_astah() {
    use umlcraft_model::{DurationConstraintProps, StateInvariantProps, TerminationProps};
    use umlcraft_notation::edit;
    use umlcraft_notation::palette::{self, EdgeTool, NodeTool};
    use umlcraft_notation::sequence::Slot;
    let mut doc = Document::new(Model::new("Shop"));
    let root = doc.model().root();
    let measure = ApproxMeasure;
    let d = edit::apply(&mut doc, "diagram", &measure, |tx| {
        edit::create_diagram(tx, root, DiagramKind::Sequence, "Timing")
    })
    .unwrap();
    let interaction = doc.model().get(d).unwrap().owner().unwrap();
    let lane = |doc: &mut Document, x: f32, name: &str| {
        edit::apply(doc, "lifeline", &measure, |tx| {
            let (element, node) =
                palette::create_node(tx, d, NodeTool::Lifeline, Point::new(x, 40.0), name)?;
            tx.rename(element, name)?;
            Ok::<_, umlcraft_model::ModelError>(node)
        })
        .unwrap()
    };
    let (shop, store) = (lane(&mut doc, 40.0, "shop"), lane(&mut doc, 240.0, "store"));
    let center =
        |doc: &Document, lane: Id| match doc.model().view(d).unwrap().get(lane).unwrap().shape {
            Shape::Lifeline { head, .. } => head.center().x,
            ref other => panic!("{other:?}"),
        };
    // Each call answered, so that Astah takes the bars as they are.
    let send = |doc: &mut Document, name: &str, y: f32| {
        edit::apply(doc, "message", &measure, |tx| {
            let slot = |y| Some(Slot { y, owner: interaction, index: 0 });
            let p = palette::connect_message(tx, d, EdgeTool::Message, shop, store, slot(y))?;
            let element = tx.model().view(d).unwrap().get(p).unwrap().element.unwrap();
            tx.rename(element, name)?;
            palette::connect_message(tx, d, EdgeTool::Reply, store, shop, slot(y + 40.0))?;
            Ok::<_, umlcraft_model::ModelError>(p)
        })
        .unwrap()
    };
    let (check, save) = (send(&mut doc, "check", 120.0), send(&mut doc, "save", 220.0));
    let at = Point::new(center(&doc, store), 300.0);
    let shop_at = Point::new(center(&doc, shop), 100.0);
    let (invariant, invariant_node) = edit::apply(&mut doc, "marks", &measure, |tx| {
        let made = sequence_edit::create_on_lifeline(tx, d, false, at, "stock > 0")?;
        sequence_edit::create_on_lifeline(tx, d, true, shop_at, "")?;
        let duration = sequence_edit::constrain(tx, d, check, save)?;
        let time = sequence_edit::constrain(tx, d, save, save)?;
        for (p, text) in [(duration, "d..3*d"), (time, "t..t+3")] {
            let element = tx.model().view(d).unwrap().get(p).unwrap().element.unwrap();
            tx.rename(element, text)?;
        }
        Ok::<_, umlcraft_model::ModelError>(made)
    })
    .unwrap();
    let before = doc.model().view(d).unwrap().get(invariant_node).unwrap().bounds().unwrap();
    assert!(matches!(doc.model().get(invariant).unwrap().kind(), ElementKind::StateInvariant(_)));
    let constraints = doc
        .model()
        .elements()
        .filter(|e| {
            matches!(e.kind(), ElementKind::DurationConstraint(DurationConstraintProps { .. }))
        })
        .count();
    assert_eq!(constraints, 1);
    doc.model().check().unwrap();

    let Some((back, report)) = through_astah_reporting(doc.model(), "sequence-marks") else {
        return;
    };
    // Astah's API makes state invariants and terminations, but no duration or time
    // constraints.
    let dropped: Vec<(&str, usize)> =
        report.dropped.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(dropped, [("durationConstraint", 1), ("timeConstraint", 1)]);
    let lifeline = |name: &str| {
        back.elements()
            .find(|e| e.name() == name && matches!(e.kind(), ElementKind::Lifeline(_)))
            .unwrap()
            .id()
    };
    let invariant = back
        .elements()
        .find(|e| matches!(e.kind(), ElementKind::StateInvariant(_)))
        .expect("the state invariant");
    assert_eq!(invariant.name(), "stock > 0");
    assert_eq!(
        invariant.kind(),
        &ElementKind::StateInvariant(StateInvariantProps { lifeline: lifeline("store") })
    );
    let terminations: Vec<&ElementKind> = back
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Termination(_)))
        .map(|e| e.kind())
        .collect();
    assert_eq!(
        terminations,
        [&ElementKind::Termination(TerminationProps { lifeline: lifeline("shop") })]
    );
    // Both drawn where they were, relative to the heads.
    let diagram = back.elements().find(|e| e.name() == "Timing" && e.kind().as_diagram().is_some());
    let diagram = diagram.unwrap().id();
    let view = back.view(diagram).unwrap();
    let top = |model: &Model, d: Id| {
        model
            .view(d)
            .unwrap()
            .presentations()
            .iter()
            .filter_map(|p| match p.shape {
                Shape::Lifeline { head, .. } => Some(head.y),
                _ => None,
            })
            .fold(f32::MAX, f32::min)
    };
    let after = view.presentations_of(invariant.id()).next().unwrap().bounds().unwrap();
    let (dy_before, dy_after) = (before.y - top(doc.model(), d), after.y - top(&back, diagram));
    assert!((dy_before - dy_after).abs() < 1.0, "{dy_before} {dy_after} {before:?} {after:?}");
    assert!((before.width - after.width).abs() < 1.0, "{before:?} {after:?}");
    // The lifeline with the termination is not destroyed, and its cross ends it.
    let cross = view
        .presentations()
        .iter()
        .find(|p| {
            p.element
                .is_some_and(|e| matches!(back.get(e).unwrap().kind(), ElementKind::Termination(_)))
        })
        .and_then(|p| p.bounds())
        .expect("the cross");
    let shop_lane = sequence_edit::lane_of(view, lifeline("shop")).unwrap();
    let Shape::Lifeline { head, length, destroyed } = view.get(shop_lane).unwrap().shape else {
        panic!()
    };
    assert!(!destroyed);
    assert!((cross.center().y - (head.bottom() + length)).abs() < 1.0, "{cross:?}");
    // Tidying the import moves nothing.
    let mut back = Document::new(back);
    let model = back.model().clone();
    back.edit("tidy", |tx| sequence_edit::tidy(tx, diagram)).unwrap();
    let changed: Vec<(&Shape, &Shape)> = model
        .view(diagram)
        .unwrap()
        .presentations()
        .iter()
        .zip(back.model().view(diagram).unwrap().presentations())
        .map(|(a, b)| (&a.shape, &b.shape))
        .filter(|(a, b)| a != b)
        .collect();
    assert!(changed.is_empty(), "{changed:#?}");
}

#[test]
fn class_diagram_notations_through_astah() {
    use umlcraft_model::SharedStyle;
    use umlcraft_notation::edit;
    use umlcraft_notation::palette::{self, EdgeTool, NodeTool};
    let mut doc = Document::new(Model::new("School"));
    let root = doc.model().root();
    let measure = ApproxMeasure;
    let d = edit::apply(&mut doc, "diagram", &measure, |tx| {
        edit::create_diagram(tx, root, DiagramKind::Class, "Main")
    })
    .unwrap();
    let node = |doc: &mut Document, tool, x: f32, y: f32, name: &str| {
        edit::apply(doc, "node", &measure, |tx| {
            let (element, node) = palette::create_node(tx, d, tool, Point::new(x, y), name)?;
            tx.rename(element, name)?;
            Ok::<_, umlcraft_model::ModelError>((element, node))
        })
        .unwrap()
    };
    let (student, n_student) = node(&mut doc, NodeTool::Class, 40.0, 40.0, "Student");
    let (_, n_course) = node(&mut doc, NodeTool::Class, 400.0, 40.0, "Course");
    let (_, n_person) = node(&mut doc, NodeTool::Class, 400.0, 300.0, "Person");
    let (_, n_teacher) = node(&mut doc, NodeTool::Class, 300.0, 480.0, "Teacher");
    let (_, n_pupil) = node(&mut doc, NodeTool::Class, 500.0, 480.0, "Pupil");
    node(&mut doc, NodeTool::Boundary, 40.0, 640.0, "Screen");
    let (_, n_rules) = node(&mut doc, NodeTool::Control, 200.0, 640.0, "Rules");
    edit::apply(&mut doc, "details", &measure, |tx| {
        let age = tx.create(student, ElementData::new("age", ElementKind::attribute(None)))?;
        let enrol = tx.create(student, ElementData::new("enrol", ElementKind::operation(None)))?;
        let enrolment = palette::connect(tx, d, EdgeTool::AssociationClass, n_student, n_course)?;
        let teaches = palette::connect(tx, d, EdgeTool::Association, n_person, n_course)?;
        let g1 = palette::connect(tx, d, EdgeTool::Generalization, n_teacher, n_person)?;
        palette::connect(tx, d, EdgeTool::Generalization, n_pupil, n_person)?;
        umlcraft_notation::shared::set_style(tx, d, g1, SharedStyle::Vertical)?;
        let element = |tx: &umlcraft_model::Tx, p: Id| {
            tx.model().view(d).unwrap().get(p).unwrap().element.unwrap()
        };
        let (enrolment, teaches, g1) =
            (element(tx, enrolment), element(tx, teaches), element(tx, g1));
        let class = match tx.model().get(enrolment).unwrap().kind() {
            ElementKind::Association(a) => a.class.unwrap(),
            other => panic!("{other:?}"),
        };
        tx.rename(class, "Enrolment")?;
        tx.create(class, ElementData::new("since", ElementKind::attribute(None)))?;
        for (id, constraints) in [
            (age, vec!["age > 0", "age < 150"]),
            (enrol, vec!["pre: open"]),
            (class, vec!["ac c"]),
            (teaches, vec!["xor"]),
            (g1, vec!["complete"]),
            (student, vec!["class c"]),
        ] {
            tx.update(id, |data| {
                data.constraints = constraints.iter().map(|&c| c.into()).collect()
            })?;
        }
        tx.update_presentation(d, n_rules, |p| p.style.iconic = false)?;
        let teaches_edge = tx.model().shown_on(teaches)[0].1;
        tx.update_presentation(d, teaches_edge, |p| p.style.hide_constraints = true)?;
        tx.update_presentation(d, n_course, |p| p.style.hide_operation_constraints = true)?;
        Ok::<_, umlcraft_model::ModelError>(())
    })
    .unwrap();
    let Some((back, report)) = through_astah_reporting(doc.model(), "class-notations") else {
        return;
    };
    // Astah's API cannot share generalizations' triangles: the lines keep their course.
    assert_eq!(report.dropped.get("sharedGeneralization"), Some(&2), "{:?}", report.dropped);
    assert_eq!(report.dropped.len(), 1, "{:?}", report.dropped);

    let find = |name: &str| back.elements().find(|e| e.name() == name).unwrap();
    let constraints = |name: &str| find(name).data().constraints.clone();
    assert_eq!(constraints("age"), ["age > 0", "age < 150"]);
    assert_eq!(constraints("enrol"), ["pre: open"]);
    assert_eq!(constraints("Enrolment"), ["ac c"]);
    assert_eq!(constraints("Student"), ["class c"]);
    let association_class = back
        .elements()
        .find_map(|e| match e.kind() {
            ElementKind::Association(a) if a.class.is_some() => Some((e, a.clone())),
            _ => None,
        })
        .expect("an association class");
    let class = back.get(association_class.1.class.unwrap()).unwrap();
    assert_eq!(class.name(), "Enrolment");
    let members: Vec<String> = back.children(class.id()).iter().map(|&c| name(&back, c)).collect();
    assert_eq!(members, ["since"]);
    let ends: Vec<String> =
        association_class.1.ends.iter().map(|e| name(&back, e.participant)).collect();
    assert_eq!(ends, ["Student", "Course"]);
    let other = back
        .elements()
        .find(|e| {
            matches!(e.kind(), ElementKind::Association(a) if a.class.is_none())
                && e.data().constraints == ["xor"]
        })
        .expect("the association with its constraint");
    let generalization = back
        .elements()
        .find(|e| {
            matches!(e.kind(), ElementKind::Generalization(_)) && !e.data().constraints.is_empty()
        })
        .unwrap();
    assert_eq!(generalization.data().constraints, ["complete"]);

    // The class box and the line, and the dashed line between them drawn from those.
    let diagram =
        back.elements().find(|e| e.kind().as_diagram() == Some(DiagramKind::Class)).unwrap();
    let view = back.view(diagram.id()).unwrap();
    let style_of = |id: Id| {
        let (_, p) = back.shown_on(id)[0];
        view.get(p).unwrap().style.clone()
    };
    assert_eq!(back.shown_on(class.id()).len(), 1);
    assert_eq!(back.shown_on(association_class.0.id()).len(), 1);
    assert!(style_of(other.id()).hide_constraints);
    assert!(style_of(find("Course").id()).hide_operation_constraints);
    assert!(style_of(find("Screen").id()).iconic);
    assert!(!style_of(find("Rules").id()).iconic);
    assert_eq!(find("Rules").data().stereotypes, ["control"]);
    // The generalizations still run over one common line.
    let commons: Vec<f32> = back
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Generalization(_)))
        .map(|e| match &view.get(back.shown_on(e.id())[0].1).unwrap().shape {
            Shape::Edge { waypoints, .. } => {
                assert_eq!(waypoints.len(), 2, "{waypoints:?}");
                assert!((waypoints[0].y - waypoints[1].y).abs() < 0.5, "{waypoints:?}");
                waypoints[0].y
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(commons.len(), 2);
    assert!((commons[0] - commons[1]).abs() < 0.5, "{commons:?}");
    let scene = scene::build(&back, diagram.id(), &Preview::default(), &ApproxMeasure);
    let dashed =
        scene.edges.iter().filter(|e| e.line == scene::LineStyle::ShortDashed && e.fixed).count();
    assert_eq!(dashed, 1, "the dashed line to the association class");
}
