//! Imports an `.asta` file and prints the resulting UmlCraft model as a tree:
//! `cargo run -p umlcraft-asta --example asta-import -- file.asta [out.ucraft]`.

use std::path::PathBuf;

use umlcraft_model::{ElementKind, Id, Model};

fn print(model: &Model, id: Id, indent: usize) {
    let e = model.get(id).expect("element");
    let detail = match e.kind() {
        ElementKind::Message(p) => format!(
            " {:?} {} → {}",
            p.sort,
            model.get(p.from).unwrap().name(),
            model.get(p.to).unwrap().name()
        ),
        ElementKind::Diagram(_) => {
            format!(" ({} presentations)", model.view(id).map_or(0, |v| v.presentations().len()))
        }
        _ => String::new(),
    };
    println!("{}{} {:?}{detail}", "  ".repeat(indent), e.kind().type_name(), e.name());
    for &child in e.children() {
        print(model, child, indent + 1);
    }
}

fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: PathBuf = args.next().expect("usage: asta-import <file.asta> [out.ucraft]").into();
    let imported = umlcraft_asta::import(&path).expect("import");
    print(&imported.model, imported.model.root(), 0);
    println!("skipped: {:?}", imported.skipped);
    if let Some(out) = args.next() {
        umlcraft_format::save(&imported.model, &PathBuf::from(out)).expect("save");
    }
}
