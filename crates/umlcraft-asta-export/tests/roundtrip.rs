//! Exports the sample project through the installed Astah and reads the file back with
//! UmlCraft's own importer. Skipped when no Astah installation with the API is found.

use std::collections::{BTreeMap, BTreeSet};

use umlcraft_asta_export::Astah;
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    CommentProps, DiagramKind, DirectedProps, Document, ElementData, ElementKind, ExtendProps, Id,
    IncludeProps, Model, Point, Rect, Shape, TypeRef,
};
use umlcraft_notation::scene::{self, ApproxMeasure, Preview};

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
        tx.add_edge(d, Some(realization), n_order, n_priced, vec![])?;
        tx.add_edge(
            d,
            Some(dependency),
            n_premium,
            n_order,
            vec![Point::new(300.0, 290.0), Point::new(300.0, 140.0)],
        )?;
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
            ElementKind::Activity
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
                ElementKind::Comment(p) => {
                    let annotated: Vec<String> = p.annotated.iter().map(|&a| n(a)).collect();
                    format!("comment {:?} on {}", p.body, annotated.join(", "))
                }
                _ => return None,
            })
        })
        .collect()
}

/// What the class, use case and activity diagrams show.
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
    let kinds = [Some(DiagramKind::Class), Some(DiagramKind::UseCase), Some(DiagramKind::Activity)];
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
                    let points: Vec<String> =
                        waypoints.iter().map(|w| format!("{:.0},{:.0}", w.x, w.y)).collect();
                    let line = format!(
                        "{}: {what} {} → {} via [{}]",
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
                        "{indent}{:?} {} → {} {:?} returns {:?}",
                        p.sort,
                        name(model, p.from),
                        name(model, p.to),
                        e.name(),
                        p.return_value
                    ));
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
    let mut out = Vec::new();
    for e in model.elements().filter(|e| matches!(e.kind(), ElementKind::Interaction)) {
        out.push(format!("{} in {}", e.name(), path(model, e.id())));
        walk(model, e.id(), 1, &mut out);
    }
    out
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
    let report = umlcraft_asta_export::export(&astah, model, &file).unwrap();
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
    // extension points, for example).
    for (node, (width, height)) in expected_shown.sizes {
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
}
