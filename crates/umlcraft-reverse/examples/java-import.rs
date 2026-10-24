//! Imports Java sources the way Tools › Java › Import Java does in the app, then checks what an
//! app user would do next: undo, and import the same sources again.
//!
//! `cargo run --release -p umlcraft-reverse --example java-import -- <folder or files>…`

use std::path::PathBuf;
use std::time::Instant;

use umlcraft_model::{Document, Model};
use umlcraft_reverse::{SourceLanguage, apply, collect_sources_for, parse_sources};

fn main() {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if paths.is_empty() {
        eprintln!("usage: java-import <folder or files>…");
        std::process::exit(2);
    }
    let language = SourceLanguage::Java;
    let mut doc = Document::new(Model::new("import"));
    let empty = doc.model().clone();
    let import = |doc: &mut Document| {
        let start = Instant::now();
        let files = collect_sources_for(language, &paths).expect("readable paths");
        let parsed = parse_sources(language, &files);
        let report = doc.edit("Import", |tx| apply(tx, language, &parsed)).expect("import");
        (report, start.elapsed())
    };

    let (report, took) = import(&mut doc);
    if report.files == 0 {
        eprintln!("no Java files found");
        std::process::exit(1);
    }
    doc.model().check().expect("a consistent model after the import");
    let elements = doc.model().len();
    println!(
        "first import: {} files in {took:?}; {} types created, {} placeholders, {} associations, \
         {elements} elements",
        report.files, report.created, report.placeholders, report.associations
    );
    for problem in &report.failed {
        println!("  failed {}: {:?}", problem.path.display(), problem.problem);
    }
    if !report.skipped.is_empty() {
        println!("  skipped {:?}", report.skipped);
    }
    if !report.guessed_encodings.is_empty() {
        println!("  guessed encodings: {}", report.guessed_encodings.len());
    }

    assert!(doc.undo(), "the import is one undo step");
    assert_eq!(*doc.model(), empty, "undo restores the empty project");
    assert!(doc.redo());
    let after_first = doc.model().clone();

    let (report, took) = import(&mut doc);
    doc.model().check().expect("a consistent model after the second import");
    println!(
        "second import: {took:?}; {} types updated, {} created, {} elements",
        report.updated,
        report.created,
        doc.model().len()
    );
    let changed = differences(&after_first, doc.model());
    for line in changed.iter().take(20) {
        println!("  changed {line}");
    }
    assert!(changed.is_empty(), "importing again leaves the model as it was");
    println!("ok");
}

/// The elements that differ between two models, by path.
fn differences(before: &Model, after: &Model) -> Vec<String> {
    let path = |m: &Model, id| {
        let mut names: Vec<String> = m.ancestors(id).iter().rev().map(|&a| name(m, a)).collect();
        names.push(name(m, id));
        names.join(".")
    };
    // What an element refers to, by path and kind.
    let refs = |m: &Model, d: &umlcraft_model::ElementData| {
        d.kind
            .references()
            .into_iter()
            .map(|(id, _)| {
                let kind = m.get(id).map(|t| t.kind().type_name()).unwrap_or("gone");
                format!("{} {kind}", path(m, id))
            })
            .collect::<Vec<_>>()
    };
    let mut out = Vec::new();
    for e in after.elements() {
        match before.get(e.id()) {
            None => out.push(format!("new {} {:?}", path(after, e.id()), refs(after, e.data()))),
            Some(old) if old.data() != e.data() => out.push(format!(
                "{}: {:?}\n    -> {:?}",
                path(after, e.id()),
                refs(before, old.data()),
                refs(after, e.data())
            )),
            Some(_) if before.children(e.id()) != after.children(e.id()) => {
                let names =
                    |m: &Model| m.children(e.id()).iter().map(|&c| name(m, c)).collect::<Vec<_>>();
                out.push(format!(
                    "children of {}: {:?}\n    -> {:?}",
                    path(after, e.id()),
                    names(before),
                    names(after)
                ))
            }
            Some(_) => {}
        }
    }
    for e in before.elements().filter(|e| after.get(e.id()).is_none()) {
        out.push(format!("gone {} {:?}", path(before, e.id()), refs(before, e.data())));
    }
    out
}

fn name(model: &Model, id: umlcraft_model::Id) -> String {
    model.get(id).map(|e| e.name().to_owned()).unwrap_or_default()
}
