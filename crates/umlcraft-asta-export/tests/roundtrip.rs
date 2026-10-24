//! Exports the sample project through the installed Astah and reads the file back with
//! UmlCraft's own importer. Skipped when no Astah installation with the API is found.

use std::collections::{BTreeMap, BTreeSet};

use umlcraft_asta_export::Astah;
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    CommentProps, DiagramKind, DirectedProps, Document, ElementData, ElementKind, ExtendProps, Id,
    IncludeProps, InteractionUseProps, LifelineProps, LineShape, MessageGate, MessageSort, Model,
    Point, Rect, Shape, TypeRef,
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
        tx.add_edge(use_cases, Some(extend), n_rush, n_place, vec![])?;

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
            Point::new(0.0, below),
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
    match t {
        Some(TypeRef::Element(id)) => name(model, *id),
        Some(TypeRef::Text(text)) => text.clone(),
        None => String::new(),
    }
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
            ElementKind::ActivityNode(p) => format!(" {:?}", p.node),
            ElementKind::State(p) => {
                format!(" entry {:?} do {:?} exit {:?}", p.entry, p.do_activity, p.exit)
            }
            ElementKind::Pseudostate(p) => format!(" {:?}", p.pseudostate),
            // Astah's API creates every instance as an object; the classifier comes back.
            ElementKind::Instance(p) => {
                format!(" : {}", p.classifier.map(|c| name(model, c)).unwrap_or_default())
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
        let label = |presentation: Id| {
            let element = view.get(presentation).and_then(|p| p.element);
            element.map(|e| name(model, e)).unwrap_or_default()
        };
        for p in view.presentations() {
            let element = p.element.and_then(|e| model.get(e));
            let what = element.map_or("", |e| e.kind().type_name());
            let shown = element.map(|e| e.name().to_owned()).unwrap_or_default();
            let line = match &p.shape {
                Shape::Node { .. } => {
                    let r = scene.node(p.id).map_or(p.bounds().unwrap(), |n| n.rect);
                    let line = format!("{}: {what} {shown:?} at {:.0},{:.0}", d.name(), r.x, r.y);
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

/// Exports `model` and imports the result again.
fn through_astah(model: &Model, name: &str) -> Option<Model> {
    let Some(astah) = Astah::find() else {
        eprintln!("skipped: no Astah installation with the API found");
        return None;
    };
    let dir = std::env::temp_dir().join(format!("umlcraft-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.asta"));
    let report = match umlcraft_asta_export::export(&astah, model, &file) {
        Err(umlcraft_asta_export::ExportError::License) => {
            eprintln!("skipped: Astah's license check refused saving");
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
                    "message {m:?} at {:.0}{} parameters {}",
                    y - top,
                    label.unwrap_or_default(),
                    !p.style.hide_parameters
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
