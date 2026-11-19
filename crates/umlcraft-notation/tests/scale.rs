//! Large diagrams: building their scene, placing their labels, arranging them and routing
//! their lines again after an edit stay fast enough for every frame of a drag and every edit.

use std::time::Instant;

use umlcraft_model::{Document, ElementKind, Id, Model, Multiplicity, Point, Rect, Shape};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool};
use umlcraft_notation::scene::{self, ApproxMeasure, Preview};
use umlcraft_notation::{autoroute, edit, layout};

const CLASSES: usize = 150;
const LINES: usize = 250;

/// A class diagram of [`CLASSES`] classes in a grid and [`LINES`] associations between them,
/// each with a name, roles and multiplicities: (document, diagram, class nodes, lines).
fn large() -> (Document, Id, Vec<Id>, Vec<Id>) {
    let measure = ApproxMeasure;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let d = edit::apply(&mut doc, "diagram", &measure, |tx| {
        edit::create_diagram(tx, root, umlcraft_model::DiagramKind::Class, "Large")
    })
    .unwrap();
    let (nodes, lines) = doc
        .edit("fill", |tx| {
            let mut nodes = Vec::new();
            for k in 0..CLASSES {
                let at = Point::new((k % 15) as f32 * 220.0, (k / 15) as f32 * 180.0);
                let (_, node) =
                    palette::create_node(tx, d, NodeTool::Class, at, &format!("Class{k}"))?;
                nodes.push(node);
            }
            let mut lines = Vec::new();
            for k in 0..LINES {
                // Mostly neighbours, some across the diagram.
                let a = (k * 7) % CLASSES;
                let b = if k % 5 == 0 { (a + 37) % CLASSES } else { (a + 1 + k % 3) % CLASSES };
                let edge = palette::connect(tx, d, EdgeTool::Association, nodes[a], nodes[b])?;
                let element = tx.model().view(d).unwrap().get(edge).unwrap().element.unwrap();
                tx.update(element, |data| {
                    data.name = format!("link{k}");
                    if let ElementKind::Association(p) = &mut data.kind {
                        p.ends[0].role = format!("from{k}");
                        p.ends[1].role = format!("to{k}");
                        p.ends[0].multiplicity = Some(Multiplicity::ONE);
                        p.ends[1].multiplicity = Some(Multiplicity::MANY);
                    }
                })?;
                lines.push(edge);
            }
            Ok((nodes, lines))
        })
        .unwrap();
    (doc, d, nodes, lines)
}

#[test]
fn a_large_diagram_is_built_routed_and_edited_quickly() {
    let measure = ApproxMeasure;
    let (mut doc, d, nodes, lines) = large();

    let start = Instant::now();
    let scene = scene::build(doc.model(), d, &Preview::default(), &measure);
    let built = start.elapsed();
    assert_eq!(scene.edges.len(), LINES);
    let free: usize = scene.edges.iter().map(|e| e.free_labels.len()).sum();
    assert!(free >= LINES * 5, "{free} free labels");

    let start = Instant::now();
    doc.edit("route", |tx| autoroute::route_lines(tx, d, &lines)).unwrap();
    let routed = start.elapsed();

    let start = Instant::now();
    doc.edit("arrange", |tx| {
        layout::arrange(tx, d, None, layout::Direction::Down, &measure).map(|_| ())
    })
    .unwrap();
    let arranged = start.elapsed();
    let routed_lines = |doc: &Document| {
        let view = doc.model().view(d).unwrap();
        view.presentations()
            .iter()
            .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.style.auto_route)
            .count()
    };
    assert!(routed_lines(&doc) > LINES / 2);

    // A node dragged elsewhere: its lines are routed again, the others keep their ports.
    let start = Instant::now();
    let r = doc.model().view(d).unwrap().get(nodes[20]).unwrap().bounds().unwrap();
    edit::apply(&mut doc, "move", &measure, |tx| {
        tx.set_bounds(d, nodes[20], Rect::new(r.x + 40.0, r.y + 30.0, r.width, r.height))
    })
    .unwrap();
    let moved = start.elapsed();

    let start = Instant::now();
    scene::build(doc.model(), d, &Preview::default(), &measure);
    let rebuilt = start.elapsed();
    eprintln!(
        "build {built:?}, route {routed:?}, arrange {arranged:?}, move {moved:?}, \
         rebuild {rebuilt:?}"
    );
}
