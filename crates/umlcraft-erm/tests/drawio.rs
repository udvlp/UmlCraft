//! Our drawing compared with draw.io's exports of the same diagrams. Needs the references
//! (`UMLCRAFT_ERM_REFS` pointing at `~/git/UmlCraft-refs/erm`, see `docs/architecture.md`);
//! without them the test passes without checking anything.
//!
//! - Line ends: every straight line of every reference file within 0.05 px of draw.io's.
//! - Label positions: the middle of draw.io's label box, on whole pixels as draw.io puts it.
//! - Images: our SVG rasterized with resvg (Liberation Sans) against draw.io's PNG made with
//!   Liberation Sans (`drawio/fonts/png1`); the share of pixels that differ clearly must stay
//!   below the limit `tests/drawio_deviations.txt` gives each file, with the reason.

mod support;

use std::path::{Path, PathBuf};

use resvg::{tiny_skia, usvg};
use support::drawio::{read, read_svg};
use umlcraft_erm::geometry::{distance, label_anchor, points_of};
use umlcraft_erm::model::{Point, Rect};
use umlcraft_erm::svg::{Options, render_with};

const FILES: &[&str] = &[
    "drawio/src/shapes.drawio",
    "drawio/src/sizes.drawio",
    "drawio/src/text.drawio",
    "drawio/src/lines.drawio",
    "drawio/src/labels.drawio",
    "drawio/src/chen-extra.drawio",
    "drawio/src/styles.drawio",
    "drawio/gui/hand.drawio",
    "drawio/gui/flug-original.xml",
];

fn refs() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("UMLCRAFT_ERM_REFS")?);
    path.join("drawio").is_dir().then_some(path)
}

fn stem(file: &str) -> String {
    Path::new(file).file_stem().unwrap().to_string_lossy().into_owned()
}

#[test]
fn lines_and_labels_end_where_draw_io_puts_them() {
    let Some(refs) = refs() else { return };
    let mut failures = Vec::new();
    let (mut lines, mut labels) = (0, 0);
    for file in FILES {
        let r = read(&std::fs::read_to_string(refs.join(file)).unwrap());
        let svg = read_svg(
            &std::fs::read_to_string(refs.join(format!("drawio/fonts/svg/{}.svg", stem(file))))
                .unwrap(),
        );
        // draw.io moves the drawing so that it starts near (0, 0): find by how much.
        let first = r.diagram.nodes.iter().find(|n| svg.cells[n.id.as_str()].rect.is_some());
        let first = first.expect("a rectangle");
        let corner = svg.cells[first.id.as_str()].rect.unwrap();
        let (tx, ty) = (corner.x - first.bounds.x, corner.y - first.bounds.y);
        let moved = |p: Point| Point::new(p.x + tx, p.y + ty);
        for link in &r.diagram.links {
            if r.routed.contains(&link.id) || link.line == umlcraft_erm::Line::Double {
                continue;
            }
            let points = points_of(&r.diagram, link).unwrap();
            if let Some((start, end)) = svg.cells.get(link.id.as_str()).and_then(|c| c.path) {
                lines += 1;
                let off = distance(moved(points[0]), start)
                    .max(distance(moved(*points.last().unwrap()), end));
                if off > 0.05 {
                    failures.push(format!("{file} {}: line ends {off:.3} px off", link.id));
                }
            }
            if let (Some(label), Some(cell)) = (&link.label, r.label_cell.get(&link.id)) {
                let Some(expected) = svg.cells.get(cell).and_then(|c| c.label) else { continue };
                labels += 1;
                // draw.io puts the label's box on whole pixels, as our drawing does.
                let exact = moved(label_anchor(&points, &label.pos));
                let anchor = Point::new((exact.x + 0.5).floor(), (exact.y + 0.5).floor());
                let off = distance(anchor, expected);
                if off > 0.01 {
                    failures.push(format!("{file} {}: label {off:.2} px off", link.id));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(lines > 300 && labels > 150, "only {lines} lines and {labels} labels compared");
}

#[test]
fn images_look_like_draw_io() {
    let Some(refs) = refs() else { return };
    let deviations = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/drawio_deviations.txt"),
    )
    .unwrap();
    let limit = |name: &str| {
        deviations
            .lines()
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| {
                let mut parts = l.split_whitespace();
                (parts.next() == Some(name)).then(|| parts.next()?.parse::<f64>().ok())?
            })
            .unwrap_or_else(|| panic!("{name} has no limit in drawio_deviations.txt"))
    };
    let mut options = usvg::Options::default();
    let liberation = Path::new("/usr/share/fonts/liberation-sans-fonts");
    if liberation.is_dir() {
        options.fontdb_mut().load_fonts_dir(liberation);
        options.fontdb_mut().set_sans_serif_family("Liberation Sans");
    } else {
        options.fontdb_mut().load_system_fonts();
    }
    let out = std::env::var_os("UMLCRAFT_ERM_OUT").map(PathBuf::from);
    let mut failures = Vec::new();
    for file in FILES {
        let name = stem(file);
        let r = read(&std::fs::read_to_string(refs.join(file)).unwrap());
        let svg_ref = read_svg(
            &std::fs::read_to_string(refs.join(format!("drawio/fonts/svg/{name}.svg"))).unwrap(),
        );
        let first = r.diagram.nodes.iter().find(|n| svg_ref.cells[n.id.as_str()].rect.is_some());
        let first = first.unwrap();
        let corner = svg_ref.cells[first.id.as_str()].rect.unwrap();
        let (tx, ty) = (corner.x - first.bounds.x, corner.y - first.bounds.y);
        // The same window on the canvas as draw.io's image, which is 1 px bigger on each side.
        let view = Rect::new(-tx - 1.0, -ty - 1.0, svg_ref.width + 2.0, svg_ref.height + 2.0);
        let ours = render_with(&r.diagram, &Options { view_box: Some(view), ..Options::default() });
        let tree = usvg::Tree::from_str(&ours, &options).unwrap();
        let size = tree.size().to_int_size();
        let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
        resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());
        let expected =
            tiny_skia::Pixmap::load_png(refs.join(format!("drawio/fonts/png1/{name}.png")))
                .unwrap();
        if let Some(dir) = &out {
            std::fs::create_dir_all(dir).unwrap();
            pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
        }
        // draw.io's border is not always the same width: take the best of small shifts.
        let differ = |sx: i32, sy: i32| {
            let mut n = 0usize;
            for y in 2..expected.height().saturating_sub(2) {
                for x in 2..expected.width().saturating_sub(2) {
                    let (ox, oy) = ((x as i32 + sx) as u32, (y as i32 + sy) as u32);
                    let (Some(a), Some(b)) = (pixmap.pixel(ox, oy), expected.pixel(x, y)) else {
                        n += 1;
                        continue;
                    };
                    let d = [
                        a.red().abs_diff(b.red()),
                        a.green().abs_diff(b.green()),
                        a.blue().abs_diff(b.blue()),
                    ];
                    if d.into_iter().max().unwrap() > 96 {
                        n += 1;
                    }
                }
            }
            n
        };
        let shifts = (-1..=1).flat_map(|x| (-1..=1).map(move |y| (x, y)));
        let differ = shifts.map(|(x, y)| differ(x, y)).min().unwrap();
        let (w, h) = (expected.width(), expected.height());
        let share = 100.0 * differ as f64 / (w as f64 * h as f64);
        eprintln!("{name}: {share:.2} % of the pixels differ");
        if share > limit(&name) {
            failures.push(format!("{name}: {share:.2} % differ, limit {}", limit(&name)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
