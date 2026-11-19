//! The tools end to end, through `call` on a `Headless` host, by area; the helpers they
//! share.

use super::*;
use crate::host::Headless;
use umlcraft_model::{LineShape, PseudostateKind};
use umlcraft_notation::scene::Figure;
use umlcraft_notation::{labels, sequence};

mod class_diagrams;
mod er_diagrams;
mod general;
mod sequence_diagrams;
mod states_activities;
mod structogram_tools;

fn ok(reply: Reply) -> Value {
    assert!(!reply.is_error, "{reply:?}");
    match &reply.content[0] {
        Content::Text(text) => serde_json::from_str(text).unwrap_or(json!(text)),
        Content::Image(_) => json!("image"),
    }
}

fn run(host: &mut Headless, name: &str, args: Value) -> Value {
    ok(call(host, &Access::default(), name, &args))
}

fn id(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}

/// A line's points and its labels with their rectangles.
type DrawnLine = (Vec<Point>, Vec<(String, Rect)>);

/// A message a test draws: tool, source and target (by index), name, height.
type Step = (&'static str, usize, usize, &'static str, f32);

/// The labels of each line of diagram `d` as drawn, with their rectangles, and the lines'
/// points and the nodes.
fn drawn_labels(host: &mut Headless, d: &str) -> (Vec<DrawnLine>, Vec<Rect>) {
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    let lines = scene
        .edges
        .iter()
        .map(|e| {
            let labels = e
                .labels
                .iter()
                .map(|l| {
                    let w = scene::label_width(&l.text, measure);
                    (l.text.clone(), Rect::new(l.anchor.x, l.anchor.y, w, scene::LABEL_HEIGHT))
                })
                .collect();
            (e.points.clone(), labels)
        })
        .collect();
    (lines, scene.nodes.iter().map(|n| n.rect).collect())
}

/// Asserts that each label of `lines` (as from [`drawn_labels`]) reads as its own line's:
/// right beside it, or with its middle nearer it than any other line.
fn keep_to_own_lines(lines: &[DrawnLine], case: &str) {
    let distance = |points: &[Point], p: Point| {
        points
            .windows(2)
            .map(|w| umlcraft_notation::scene::distance_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min)
    };
    for (k, (points, labels)) in lines.iter().enumerate() {
        for (text, r) in labels {
            // Within a few units of its own line along a side.
            let beside = points
                .windows(2)
                .any(|w| r.inflate(6.0).intersects(&Rect::from_corners(w[0], w[1])));
            if beside {
                continue;
            }
            let own = distance(points, r.center());
            for (j, (other, _)) in lines.iter().enumerate().filter(|&(j, _)| j != k) {
                let d = distance(other, r.center());
                assert!(own < d, "{case}: {text} {r:?} {own} to {points:?}, {d} to line {j}");
            }
        }
    }
}

/// A sequence diagram of the lifelines p, t and g with the messages `steps` (tool, source
/// and target by index, name, height), as a client draws it. Returns the diagram and the
/// messages' replies.
fn sequence_script(host: &mut Headless, steps: &[Step]) -> (String, Vec<Value>) {
    let d = run(host, "create_diagram", json!({ "kind": "sequence", "name": "S" }));
    let d = id(&d["diagram"]);
    let lifelines: Vec<Value> = ["p", "t", "g"]
        .iter()
        .map(|n| {
            let args = json!({ "diagram": d, "tool": "lifeline", "name": n });
            run(host, "create_element", args)
        })
        .collect();
    let messages = steps
        .iter()
        .map(|&(tool, from, to, name, y)| {
            let args = json!({ "diagram": d, "tool": tool, "name": name, "y": y,
                               "source": lifelines[from]["element"],
                               "target": lifelines[to]["element"] });
            run(host, "connect", args)
        })
        .collect();
    (d, messages)
}

/// The bars of `lifeline` (by its index among p, t, g) on `d`, as (top, bottom).
fn bars_of(host: &Headless, d: &str, lifeline: usize) -> Vec<(f32, f32)> {
    let model = host.document().model();
    let view = model.view(d.parse().unwrap()).unwrap();
    let lanes: Vec<Id> = view
        .presentations()
        .iter()
        .filter(|p| matches!(p.shape, Shape::Lifeline { .. }))
        .map(|p| p.id)
        .collect();
    view.presentations()
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Activation { lifeline: l, top, height, .. } if l == lanes[lifeline] => {
                Some((top, top + height))
            }
            _ => None,
        })
        .collect()
}

/// The height of the message presentation `m` (a reply of `connect`), and its end.
fn message_y(host: &Headless, d: &str, m: &Value) -> (f32, f32) {
    let model = host.document().model();
    let view = model.view(d.parse().unwrap()).unwrap();
    match view.get(id(&m["presentation"]).parse().unwrap()).unwrap().shape {
        Shape::Message { y, end_y, .. } => (y, end_y.unwrap_or(y)),
        _ => panic!("no message"),
    }
}

/// The bounds of the fragment `f` (a reply of `wrap_in_fragment`) on `d`.
fn frame_of(host: &Headless, d: &str, f: &Value) -> Rect {
    let model = host.document().model();
    let view = model.view(d.parse().unwrap()).unwrap();
    view.presentations_of(id(&f["id"]).parse().unwrap()).next().unwrap().bounds().unwrap()
}

/// An ER diagram with the entity `User` and the relationship `refers`, joined twice: a
/// recursive relationship. Returns the diagram, the relationship's presentation and the
/// two participations' presentations.
fn recursive_relationship(host: &mut Headless) -> (String, String, [String; 2]) {
    let d = run(host, "create_diagram", json!({ "kind": "entity_relationship", "name": "R" }));
    let d = id(&d["diagram"]);
    let user = run(
        host,
        "create_element",
        json!({ "diagram": d, "tool": "erEntity", "name": "User", "x": 20, "y": 100 }),
    );
    let refers = run(
        host,
        "create_element",
        json!({ "diagram": d, "tool": "erRelationship", "name": "refers", "x": 260, "y": 95 }),
    );
    let mut join = |cardinality: &str, role: &str| {
        let args = json!({ "diagram": d, "tool": "erLine", "source": refers["element"],
                           "target": user["element"] });
        let line = run(host, "connect", args);
        let args = json!({ "id": line["element"], "cardinality": cardinality, "role": role });
        run(host, "update_element", args);
        id(&line["presentation"])
    };
    let lines = [join("[0,1]", "referrer"), join("[0,n]", "referred")];
    (d, id(&refers["presentation"]), lines)
}

/// Whether the segments `a`–`b` and `c`–`d` cross (touching ends do not count).
fn cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    let side = |o: Point, p: Point, q: Point| (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x);
    let (d1, d2, d3, d4) = (side(c, d, a), side(c, d, b), side(a, b, c), side(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// A use case diagram: the system boundary "Mensa" at 200, 20 (400 × 300), the use cases
/// at the positions given (none: where `create_element` puts them), the actor Mitglied
/// associated with the first two, and the first including the third. Returns the diagram
/// and the replies for boundary, use cases and actor.
fn mensa(host: &mut Headless, at: [Option<(f32, f32)>; 3]) -> (String, Vec<Value>) {
    let d = run(host, "create_diagram", json!({ "kind": "use_case", "name": "Mensa" }));
    let d = id(&d["diagram"]);
    let args = json!({ "diagram": d, "tool": "systemBoundary", "name": "Mensa",
                       "x": 200, "y": 20 });
    let boundary = run(host, "create_element", args);
    let args = json!({ "diagram": d, "node": boundary["presentation"], "x": 200, "y": 20,
                       "width": 400, "height": 300 });
    run(host, "move_node", args);
    let names = ["Essen bestellen", "Bestellung stornieren", "Guthaben prüfen"];
    let mut made = vec![boundary];
    for (name, at) in names.iter().zip(at) {
        let mut args = json!({ "diagram": d, "tool": "useCase", "name": name });
        if let Some((x, y)) = at {
            (args["x"], args["y"]) = (json!(x), json!(y));
        }
        made.push(run(host, "create_element", args));
    }
    let args = json!({ "diagram": d, "tool": "actor", "name": "Mitglied", "x": 40, "y": 60 });
    made.push(run(host, "create_element", args));
    for (tool, s, t) in [("association", 4, 1), ("association", 4, 2), ("include", 1, 3)] {
        let args = json!({ "diagram": d, "tool": tool, "source": made[s]["element"],
                           "target": made[t]["element"] });
        run(host, "connect", args);
    }
    (d, made)
}

/// The drawn rectangles of the nodes `made` on diagram `d`.
fn drawn_rects(host: &mut Headless, d: &str, made: &[Value]) -> Vec<Rect> {
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    made.iter().map(|v| scene.node(id(&v["presentation"]).parse().unwrap()).unwrap().rect).collect()
}

/// What is wrong with the labels of the lines on diagram `d`: for each label, the nodes,
/// other labels and other lines it covers, and the other lines it lies closer to than to
/// its own.
fn covered_by_labels(host: &mut Headless, d: &str) -> Vec<String> {
    let (lines, nodes) = drawn_labels(host, d);
    let inside = |r: &Rect, p: Point| {
        p.x > r.x + 0.5 && p.x < r.right() - 0.5 && p.y > r.y + 0.5 && p.y < r.bottom() - 0.5
    };
    let samples = |points: &[Point]| -> Vec<Point> {
        points
            .windows(2)
            .flat_map(|w| {
                let steps = w[0].distance(w[1]).ceil().max(1.0) as usize;
                (0..=steps).map(move |i| {
                    let t = i as f32 / steps as f32;
                    Point::new(w[0].x + (w[1].x - w[0].x) * t, w[0].y + (w[1].y - w[0].y) * t)
                })
            })
            .collect()
    };
    let crosses = |points: &[Point], r: &Rect| samples(points).into_iter().any(|p| inside(r, p));
    let distance = |points: &[Point], r: &Rect| {
        samples(points)
            .into_iter()
            .map(|p| {
                let dx = (r.x - p.x).max(p.x - r.right()).max(0.0);
                let dy = (r.y - p.y).max(p.y - r.bottom()).max(0.0);
                dx.hypot(dy)
            })
            .fold(f32::INFINITY, f32::min)
    };
    let mut found = Vec::new();
    for (e, (own, labels)) in lines.iter().enumerate() {
        for (text, r) in labels {
            for n in nodes.iter().filter(|n| n.intersects(r)) {
                found.push(format!("{text:?} {r:?} on node {n:?}"));
            }
            let mine = distance(own, r);
            for (f, (points, others)) in lines.iter().enumerate().filter(|&(f, _)| f != e) {
                if crosses(points, r) {
                    found.push(format!("{text:?} {r:?} on line {f} {points:?}"));
                } else if distance(points, r) < mine - 0.5 {
                    found.push(format!("{text:?} {r:?} closer to line {f} {points:?}"));
                }
                for (other, o) in others.iter().filter(|(_, o)| o.intersects(r)) {
                    found.push(format!("{text:?} {r:?} on label {other:?} {o:?}"));
                }
            }
        }
    }
    found
}

/// Runs `calls` as one batch and returns the replies.
fn build(host: &mut Headless, calls: Value) -> Vec<Value> {
    run(host, "batch", json!({ "calls": calls }))["results"].as_array().unwrap().clone()
}

/// The scene of the diagram `d`.
fn drawn(host: &mut Headless, d: &Value) -> scene::Scene {
    let (doc, measure) = host.parts();
    scene::build(doc.model(), id(d).parse().unwrap(), &Preview::default(), measure)
}

/// The heights of the messages of the sequence diagram `d`, by name.
fn message_heights(host: &mut Headless, d: &Value) -> Vec<(String, f64)> {
    let lines = run(host, "get_diagram", json!({ "diagram": d }))["lines"].clone();
    let lines = lines.as_array().unwrap().iter();
    lines.map(|l| (l["name"].as_str().unwrap().to_owned(), l["y"].as_f64().unwrap())).collect()
}

/// A sequence diagram without numbers or frame, with lifelines named `names`.
fn lifelines(names: &[&str]) -> Vec<Value> {
    let mut calls = vec![json!({ "tool": "create_diagram", "arguments": {
        "kind": "sequence", "name": "s", "message_numbers": false, "frame": false } })];
    for name in names {
        calls.push(json!({ "tool": "create_element", "arguments": {
            "diagram": "$0.diagram", "tool": "lifeline", "name": name } }));
    }
    calls
}

fn message(tool: &str, from: usize, to: usize, name: &str, y: Option<f64>) -> Value {
    let mut args = json!({ "diagram": "$0.diagram", "tool": tool, "name": name,
                           "source": format!("${from}.element"),
                           "target": format!("${to}.element") });
    if let Some(y) = y {
        args["y"] = json!(y);
    }
    json!({ "tool": "connect", "arguments": args })
}

/// The width and height of a PNG, from its header.
fn png_size(png: &[u8]) -> (u32, u32) {
    let at = |i: usize| u32::from_be_bytes(png[i..i + 4].try_into().unwrap());
    (at(16), at(20))
}
