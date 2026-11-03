//! Snapshots of how the fixtures are laid out and drawn. The geometry itself is checked
//! against Structorizer's images by `structorizer_drawing.rs`; these keep it from changing
//! unnoticed where no reference folder is at hand. `UPDATE_SNAPSHOTS=1` rewrites them.

use std::fmt::Write as _;
use std::path::Path;

use umlcraft_nsd::layout::{Layout, SeqBox};
use umlcraft_nsd::{Metrics, read, svg};

fn snapshot(file: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(file);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing; run with UPDATE_SNAPSHOTS=1", path.display()));
    pretty_assertions::assert_eq!(expected, actual, "{file}");
}

fn dump(out: &mut String, seq: &SeqBox, depth: usize) {
    let r = seq.rect;
    let _ = writeln!(out, "{}[] {} {} {} {}", "  ".repeat(depth), r.x, r.y, r.w, r.h);
    for e in &seq.elements {
        let r = e.rect;
        let _ = writeln!(
            out,
            "{}{:?} {} {} {} {} {:?}",
            "  ".repeat(depth + 1),
            e.kind,
            r.x,
            r.y,
            r.w,
            r.h,
            e.shape
        );
        for b in &e.branches {
            dump(out, b, depth + 2);
        }
    }
}

#[test]
fn fixtures_lay_out_as_before() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structorizer");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let mut out = String::new();
    for path in files {
        let diagram = read(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let layout = Layout::of(&diagram, Metrics::linux());
        let _ = writeln!(
            out,
            "== {} {}x{}",
            path.file_name().unwrap().to_string_lossy(),
            layout.size.0,
            layout.size.1
        );
        dump(&mut out, &layout.body, 0);
    }
    snapshot("layout.txt", &out);
}

#[test]
fn a_diagram_of_every_kind_draws_as_before() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/new");
    let diagram = read(&std::fs::read_to_string(dir.join("all-elements.nsd")).unwrap()).unwrap();
    // The font list depends on the platform the image is made on.
    let svg = svg::render(&diagram, Metrics::linux());
    let family = Metrics::linux().font_family();
    let svg = svg.replace(family, "'DejaVu Sans', Dialog, Arial, sans-serif");
    snapshot("new/all-elements.svg", &svg);
}

#[test]
fn a_palette_recolours_everything() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/new");
    let diagram = read(&std::fs::read_to_string(dir.join("all-elements.nsd")).unwrap()).unwrap();
    let m = Metrics::linux();
    let layout = Layout::of(&diagram, m);
    let light = svg::render_with(&diagram, &layout, m, &svg::Palette::STRUCTORIZER);
    assert_eq!(light, svg::render(&diagram, m), "exports keep Structorizer's colours");
    let dark = svg::Palette {
        line: umlcraft_nsd::Color([0xee, 0xee, 0xee]),
        background: umlcraft_nsd::Color([0x20, 0x20, 0x20]),
        head: umlcraft_nsd::Color([0x30, 0x30, 0x30]),
        comment: Some(umlcraft_nsd::Color([0x90, 0x90, 0x90])),
        hatch: umlcraft_nsd::Color([0x80, 0x80, 0x80]),
        includes: umlcraft_nsd::Color([0x80, 0x80, 0x80]),
        tint: 0.35,
    };
    let dark = svg::render_with(&diagram, &layout, m, &dark);
    for black_or_white in ["#000000", "#ffffff", "#c0c0c0", "#404040"] {
        assert!(!dark.contains(black_or_white), "{black_or_white} left in the dark image");
    }
    assert!(dark.contains("fill=\"#eeeeee\""), "texts in the line colour");
}

#[test]
fn the_plain_palette_leaves_out_comment_markers() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/structorizer");
    let diagram = read(&std::fs::read_to_string(dir.join("39-comments.nsd")).unwrap()).unwrap();
    let m = Metrics::linux();
    let layout = Layout::of(&diagram, m);
    let light = svg::render_with(&diagram, &layout, m, &svg::Palette::STRUCTORIZER);
    let plain = svg::render_with(&diagram, &layout, m, &svg::Palette::PLAIN);
    // Only the grey markers are left out.
    let left_out: Vec<&str> = light.lines().filter(|l| !plain.lines().any(|p| p == *l)).collect();
    assert!(!left_out.is_empty(), "the fixture has comments");
    assert!(left_out.iter().all(|l| l.starts_with("<polygon") && l.contains("#c0c0c0")));
    assert_eq!(plain.lines().count() + left_out.len(), light.lines().count());
}
