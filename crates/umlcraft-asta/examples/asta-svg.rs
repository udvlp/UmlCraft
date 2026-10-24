//! Imports an `.asta` file and writes each diagram as UmlCraft's SVG export, to compare it with
//! Astah's own: `cargo run -p umlcraft-asta --example asta-svg -- file.asta out-dir [name]`.
//!
//! Texts are measured with the system's Arial the way Java does: exact advances, and rounded to
//! whole pixels per character where Astah centers by such widths.

use std::path::PathBuf;

use umlcraft_model::{ElementKind, Id, Model};
use umlcraft_notation::scene::{self, Preview, TextMeasure};

struct ArialMeasure {
    regular: Vec<u8>,
    bold: Vec<u8>,
}

impl ArialMeasure {
    fn advances(&self, text: &str, font_size: f32, bold: bool) -> Vec<f32> {
        let data = if bold { &self.bold } else { &self.regular };
        let face = ttf_parser::Face::parse(data, 0).expect("font");
        let scale = font_size / f32::from(face.units_per_em());
        text.chars()
            .map(|c| {
                let advance = face
                    .glyph_index(c)
                    .and_then(|g| face.glyph_hor_advance(g))
                    .unwrap_or(face.units_per_em() / 2);
                f32::from(advance) * scale
            })
            .collect()
    }
}

impl TextMeasure for ArialMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        self.advances(text, font_size, bold).iter().sum()
    }

    fn rounded_width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        self.advances(text, font_size, bold).iter().map(|a| a.round()).sum()
    }
}

fn diagrams(model: &Model, id: Id, out: &mut Vec<Id>) {
    let e = model.get(id).expect("element");
    if matches!(e.kind(), ElementKind::Diagram(_)) {
        out.push(id);
    }
    for &child in e.children() {
        diagrams(model, child, out);
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let usage = "usage: asta-svg <file.asta> <out-dir> [diagram name]";
    let path: PathBuf = args.next().expect(usage).into();
    let out: PathBuf = args.next().expect(usage).into();
    let only = args.next().map(|n| n.to_string_lossy().into_owned());
    let fonts =
        PathBuf::from(std::env::var("WINDIR").unwrap_or("C:\\Windows".into())).join("Fonts");
    let measure = ArialMeasure {
        regular: std::fs::read(fonts.join("arial.ttf")).expect("arial.ttf"),
        bold: std::fs::read(fonts.join("arialbd.ttf")).expect("arialbd.ttf"),
    };

    let imported = umlcraft_asta::import(&path).expect("import");
    let model = &imported.model;
    let mut found = Vec::new();
    diagrams(model, model.root(), &mut found);
    std::fs::create_dir_all(&out).expect("out dir");
    for diagram in found {
        let name = model.get(diagram).expect("diagram").name();
        if only.as_ref().is_some_and(|o| o != name) {
            continue;
        }
        let scene = scene::build(model, diagram, &Preview::default(), &measure);
        let svg = umlcraft_notation::svg::render(&scene, &measure);
        let file = out.join(format!(
            "{}.svg",
            name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
        ));
        std::fs::write(&file, svg).expect("write");
        println!("{}", file.display());
    }
}
