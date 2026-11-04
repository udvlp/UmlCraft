//! Measures how much the labels of lines cover each other, lines and nodes once a diagram is
//! arranged, over the diagrams of the local examples: every label is put back where the layout
//! places it (as for a diagram drawn in UmlCraft), then the diagram is arranged. Run by hand:
//!
//! `cargo test -p umlcraft-asta --test label_overlap -- --ignored --nocapture`
//!
//! With `UMLCRAFT_LABELS_OUT` set to a folder, the SVGs of the worst diagrams go there.

use std::path::{Path, PathBuf};

use umlcraft_model::{Document, EdgeLabels, ElementKind, Id, Point, Rect, Shape};
use umlcraft_notation::layout;
use umlcraft_notation::scene::{self, ApproxMeasure, LABEL_HEIGHT, Preview, Scene, label_width};

fn asta_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            asta_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "asta") {
            out.push(path);
        }
    }
}

/// What the labels of a scene cover, in pixels: of each other (area over a row's height),
/// of lines (length inside) and of nodes (area over a row's height); and how many labels
/// cover anything.
#[derive(Default, Clone, Copy, Debug)]
struct Cover {
    labels: f32,
    lines: f32,
    nodes: f32,
    /// Of arrowheads, diamonds and crosses at the ends of lines.
    markers: f32,
    hit: usize,
    count: usize,
    /// Length of lines on top of other lines.
    stacked: f32,
}

impl Cover {
    fn add(&mut self, o: Cover) {
        self.labels += o.labels;
        self.lines += o.lines;
        self.nodes += o.nodes;
        self.markers += o.markers;
        self.hit += o.hit;
        self.count += o.count;
        self.stacked += o.stacked;
    }

    fn total(&self) -> f32 {
        self.labels * 2.0 + self.lines + (self.nodes + self.markers) * 1.5
    }
}

/// How long two axis-parallel segments run closer than 3 to each other along the same axis.
fn stacked(p: Point, q: Point, r: Point, s: Point) -> f32 {
    let horizontal = |a: Point, b: Point| (a.y - b.y).abs() < 0.5 && (a.x - b.x).abs() >= 0.5;
    let vertical = |a: Point, b: Point| (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() >= 0.5;
    let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| {
        (a0.max(a1).min(b0.max(b1)) - a0.min(a1).max(b0.min(b1))).max(0.0)
    };
    if horizontal(p, q) && horizontal(r, s) && (p.y - r.y).abs() < 3.0 {
        overlap(p.x, q.x, r.x, s.x)
    } else if vertical(p, q) && vertical(r, s) && (p.x - r.x).abs() < 3.0 {
        overlap(p.y, q.y, r.y, s.y)
    } else {
        0.0
    }
}

fn area(a: Rect, b: Rect) -> f32 {
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    w.max(0.0) * h.max(0.0) / LABEL_HEIGHT
}

fn inside_length(r: Rect, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in
        [(-dx, a.x - r.x), (dx, r.right() - a.x), (-dy, a.y - r.y), (dy, r.bottom() - a.y)]
    {
        if p == 0.0 {
            if q < 0.0 {
                return 0.0;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    if t1 > t0 { (t1 - t0) * dx.hypot(dy) } else { 0.0 }
}

/// The text of the label of edge `e` at `r`.
fn labels_text(scene: &Scene, e: usize, r: Rect) -> String {
    scene.edges[e]
        .labels
        .iter()
        .find(|l| (l.anchor.x + 1.0 - r.x).abs() < 0.01 && (l.anchor.y + 1.0 - r.y).abs() < 0.01)
        .map_or_else(String::new, |l| l.text.clone())
}

/// `general` tells the generalization lines (by presentation).
fn cover(scene: &Scene, general: &dyn Fn(Id) -> bool) -> Cover {
    let labels: Vec<(usize, Rect)> = scene
        .edges
        .iter()
        .enumerate()
        .flat_map(|(e, edge)| {
            edge.labels.iter().filter(|l| !l.text.trim().is_empty()).map(move |l| {
                let w = label_width(&l.text, &ApproxMeasure);
                (e, Rect::new(l.anchor.x, l.anchor.y, w, LABEL_HEIGHT).inflate(-1.0))
            })
        })
        .collect();
    let segments: Vec<(Point, Point)> = scene
        .edges
        .iter()
        .flat_map(|e| e.path().windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>())
        .collect();
    let boxes: Vec<Rect> = scene
        .nodes
        .iter()
        .filter(|n| !scene.nodes.iter().any(|o| o.rect != n.rect && n.rect.contains_rect(&o.rect)))
        .map(|n| n.extent())
        .collect();
    let markers: Vec<Rect> = scene
        .edges
        .iter()
        .flat_map(umlcraft_notation::draw::end_markers)
        .filter_map(|p| p.bounds())
        .collect();
    let mut out = Cover { count: labels.len(), ..Cover::default() };
    // Lines of different edges on top of each other (closer than 3 along the same stretch),
    // but for generalizations, which may share their line.
    let per_edge: Vec<(bool, Vec<(Point, Point)>)> = scene
        .edges
        .iter()
        .map(|e| (general(e.id), e.path().windows(2).map(|w| (w[0], w[1])).collect()))
        .collect();
    for (i, (ga, a)) in per_edge.iter().enumerate() {
        for (gb, b) in &per_edge[i + 1..] {
            if *ga && *gb {
                continue;
            }
            for &(p, q) in a {
                for &(r, s) in b {
                    out.stacked += stacked(p, q, r, s);
                }
            }
        }
    }
    for (k, &(_, r)) in labels.iter().enumerate() {
        let l: f32 =
            labels.iter().enumerate().filter(|&(j, _)| j != k).map(|(_, &(_, o))| area(r, o)).sum();
        let s: f32 = segments.iter().map(|&(a, b)| inside_length(r, a, b)).sum();
        let n: f32 = boxes.iter().map(|&b| area(r, b)).sum();
        let m: f32 = markers.iter().map(|&b| area(r, b)).sum();
        out.labels += l / 2.0;
        out.lines += s;
        out.nodes += n;
        out.markers += m;
        if l + s + n + m > 1.0 {
            out.hit += 1;
            if std::env::var_os("UMLCRAFT_LABELS_VERBOSE").is_some() {
                let text = labels_text(scene, labels[k].0, r);
                let own: f32 = scene.edges[labels[k].0]
                    .path()
                    .windows(2)
                    .map(|w| inside_length(r, w[0], w[1]))
                    .sum();
                println!(
                    "    {text:24} labels {l:6.1} lines {s:6.1} (own {own:5.1}) nodes {n:6.1}"
                );
            }
        }
    }
    out
}

#[test]
#[ignore = "a measurement, run by hand"]
fn labels_of_arranged_diagrams() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    asta_files(&repo.join("examples"), &mut files);
    let home = PathBuf::from(r"C:\Program Files\astah-professional\Sample.asta");
    if home.exists() {
        files.push(home);
    }
    files.sort();
    let out_dir = std::env::var_os("UMLCRAFT_LABELS_OUT").map(PathBuf::from);
    let mut all = Cover::default();
    let mut by_kind: std::collections::BTreeMap<String, Cover> = Default::default();
    let mut worst: Vec<(f32, String, String)> = Vec::new();
    for path in &files {
        let Ok(imported) = umlcraft_asta::import(path) else { continue };
        let mut doc = Document::new(imported.model);
        let diagrams: Vec<(Id, String, umlcraft_model::DiagramKind)> = doc
            .model()
            .elements()
            .filter_map(|e| Some((e.id(), e.name().to_owned(), e.kind().as_diagram()?)))
            .filter(|(_, _, k)| layout::can_arrange(*k))
            .collect();
        for (d, name, kind) in diagrams {
            let ok = doc.edit("arrange", |tx| {
                let edges: Vec<Id> = tx
                    .model()
                    .view(d)
                    .map(|v| {
                        v.presentations()
                            .iter()
                            .filter(|p| matches!(p.shape, Shape::Edge { .. }))
                            .map(|p| p.id)
                            .collect()
                    })
                    .unwrap_or_default();
                for e in edges {
                    tx.update_presentation(d, e, |p| {
                        if let Shape::Edge { labels, .. } = &mut p.shape {
                            *labels = EdgeLabels::default();
                        }
                    })?;
                }
                layout::arrange(tx, d, None, layout::default_direction(kind), &ApproxMeasure)?;
                Ok(())
            });
            if ok.is_err() {
                continue;
            }
            let scene = scene::build(doc.model(), d, &Preview::default(), &ApproxMeasure);
            let model = doc.model();
            let general = |p: Id| {
                let element = model.view(d).and_then(|v| v.get(p)).and_then(|p| p.element);
                element.and_then(|e| model.get(e)).is_some_and(|e| {
                    matches!(e.kind(), ElementKind::Generalization(_) | ElementKind::Realization(_))
                })
            };
            let c = cover(&scene, &general);
            all.add(c);
            by_kind.entry(format!("{kind:?}")).or_default().add(c);
            let file = path.file_stem().unwrap().to_string_lossy().into_owned();
            let label = format!("{file} / {name}");
            if c.stacked > 0.0 && std::env::var_os("UMLCRAFT_LABELS_VERBOSE").is_some() {
                println!("  stacked {:7.1}  {label}", c.stacked);
            }
            if let Some(dir) = &out_dir {
                let svg = umlcraft_notation::svg::render(&scene, &ApproxMeasure);
                let safe: String =
                    label.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
                worst.push((c.total(), label.clone(), safe.clone()));
                std::fs::write(dir.join(format!("{safe}.svg")), svg).unwrap();
            } else {
                worst.push((c.total(), label, String::new()));
            }
        }
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (score, label, _) in worst.iter().take(15) {
        println!("{score:8.1}  {label}");
    }
    for (kind, c) in &by_kind {
        println!(
            "{kind:16} labels {:4} hit {:4}  over labels {:7.1} lines {:7.1} nodes {:7.1}  stacked lines {:7.1}",
            c.count, c.hit, c.labels, c.lines, c.nodes, c.stacked
        );
    }
    println!(
        "ALL diagrams {} labels {} hit {}  over labels {:.1} lines {:.1} nodes {:.1} markers {:.1}  score {:.1}  stacked lines {:.1}",
        worst.len(),
        all.count,
        all.hit,
        all.labels,
        all.lines,
        all.nodes,
        all.markers,
        all.total(),
        all.stacked
    );
}
