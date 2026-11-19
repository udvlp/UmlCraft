//! Every method of the fixtures finds the operation the class import made for it
//! (`umlcraft_reverse::Placed`), by the bytes their declarations stand on: one parse gives both.

use std::path::{Path, PathBuf};

use umlcraft_model::{Document, ElementKind, Model};
use umlcraft_nsd_code::{Source, SourceLanguage, read};
use umlcraft_reverse::{Parsed, apply_placed, lower};

fn fixtures(language: &str) -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(language);
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| SourceLanguage::of(p).is_some())
        .collect();
    files.sort();
    files
}

/// Imports the fixtures of `language` and lists each method with the operation it found:
/// `file: Class.method -> operation` (or `-> type` / `-> package` where it has none).
fn placed(language: &str) -> Vec<String> {
    let mut parsed = Parsed::default();
    let mut methods = Vec::new();
    for path in fixtures(language) {
        let decoded = read(&path).unwrap();
        let source = Source::parse(SourceLanguage::of(&path).unwrap(), decoded.text);
        let mut skipped = Default::default();
        if let Ok(unit) = lower(source.tree(), &path, &mut skipped) {
            methods.push((parsed.units.len(), path.clone(), source.methods()));
            parsed.units.push(unit);
        }
    }
    let language = SourceLanguage::of(&parsed.units[0].path).unwrap();
    let mut doc = Document::new(Model::new("t"));
    let (_, places) = doc.edit("Import", |tx| apply_placed(tx, language, &parsed)).unwrap();
    let model = doc.model();
    let mut out = Vec::new();
    for (unit, path, methods) in methods {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        for m in methods {
            let found = match m.declaration().and_then(|d| places.operation(unit, d)) {
                Some(op) => {
                    let e = model.get(op).unwrap();
                    assert!(matches!(e.kind(), ElementKind::Operation(_)));
                    let owner = model.get(e.owner().unwrap()).unwrap().name().to_owned();
                    format!("{owner}.{}", e.name())
                }
                None => match places.type_at(unit, &m.class) {
                    Some(t) => format!("type {}", model.get(t).unwrap().name()),
                    None => match places.namespace(unit) {
                        Some(p) => format!("package {}", model.get(p).unwrap().name()),
                        None => "nothing".to_owned(),
                    },
                },
            };
            out.push(format!("{file}: {}.{} -> {found}", m.class_name(), m.name));
        }
    }
    out
}

/// Methods whose operation has another name than the method: those `umlcraft-reverse` names
/// differently (C#'s operators as `operator +`).
fn renamed(lines: &[String]) -> Vec<&String> {
    lines
        .iter()
        .filter(|l| {
            let (method, found) = l.split_once(" -> ").unwrap();
            let method = method.split_once(": ").unwrap().1;
            let (name, operation) = (method.rsplit('.').next(), found.rsplit('.').next());
            let operator = name.is_some_and(|n| n.starts_with("op_"))
                && operation.is_some_and(|o| o.starts_with("operator"));
            !found.starts_with("type ")
                && !found.starts_with("package ")
                && name != operation
                && !operator
        })
        .collect()
}

fn check(language: &str) {
    let lines = placed(language);
    assert!(!lines.is_empty());
    let unplaced: Vec<&String> = lines.iter().filter(|l| l.ends_with("-> nothing")).collect();
    assert!(unplaced.is_empty(), "{unplaced:#?}");
    let renamed = renamed(&lines);
    assert!(renamed.is_empty(), "{renamed:#?}");
}

#[test]
fn java_methods_find_their_operations() {
    check("java");
}

#[test]
fn csharp_methods_find_their_operations() {
    check("csharp");
}

#[test]
fn python_methods_find_their_operations() {
    check("python");
}

#[test]
fn cpp_methods_find_their_operations() {
    check("cpp");
}

/// The code of a module around its classes is no method of theirs: it finds no operation
/// and stands in the module's package.
#[test]
fn module_code_around_classes_finds_no_operation() {
    let text = "import logging
log = logging.getLogger(__name__)

class First:
    def __init__(self):
        self.n = 0

if __name__ == \"__main__\":
    First()
";
    let path = Path::new("tool.py");
    let source = Source::parse(SourceLanguage::Python, text);
    let unit = lower(source.tree(), path, &mut Default::default()).unwrap();
    let parsed = Parsed { units: vec![unit], ..Default::default() };
    let mut doc = Document::new(Model::new("t"));
    let (_, places) =
        doc.edit("Import", |tx| apply_placed(tx, SourceLanguage::Python, &parsed)).unwrap();
    let methods = source.methods();
    let main = methods.iter().find(|m| m.name == "__main__").unwrap();
    assert!(main.module_code && main.declaration().is_none());
    // Its bytes do overlap `__init__`'s.
    assert!(places.operation(0, main.start..main.end).is_some());
    let init = methods.iter().find(|m| m.name == "__init__").unwrap();
    assert!(init.declaration().and_then(|d| places.operation(0, d)).is_some());
}

#[test]
#[ignore = "prints the list"]
fn print_placed() {
    for language in ["java", "csharp", "python", "cpp"] {
        for line in placed(language) {
            println!("{line}");
        }
    }
}
