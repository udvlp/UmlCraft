//! Helpers the fixture tests of every language share: where fixtures are, what an import makes
//! of them as text, and the comparison with the snapshots next to them. Run with
//! `UPDATE_SNAPSHOTS=1` to write the snapshots after a deliberate change, and review the diff.

#![allow(dead_code)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use umlcraft_model::{Document, ElementKind, Id, Model};
use umlcraft_notation::member::{member_text, type_label, type_text};
use umlcraft_reverse::{
    Problem, Report, SourceLanguage, collect_sources_for, import_into, parse_sources,
};

/// One test per fixture case of `language`, named after the case: [`check_ir`].
#[allow(unused_macros)]
macro_rules! ir_tests {
    ($language:expr; $($case:ident),* $(,)?) => {
        $(
            #[test]
            fn $case() {
                crate::common::check_ir($language, stringify!($case));
            }
        )*
    };
}

/// The fixture folder of `case`: `tests/fixtures/<language>/<case>`.
pub fn fixture(language: SourceLanguage, case: &str) -> PathBuf {
    let folder = match language {
        SourceLanguage::Java => "java",
        SourceLanguage::CSharp => "csharp",
        SourceLanguage::Cpp => "cpp",
        SourceLanguage::Python => "python",
    };
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(folder).join(case)
}

/// Compares `actual` with the snapshot `file` of `case`, or writes it with `UPDATE_SNAPSHOTS`.
pub fn snapshot(language: SourceLanguage, case: &str, file: &str, actual: &str) {
    let path = fixture(language, case).join(file);
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing; run with UPDATE_SNAPSHOTS=1", path.display()))
        .replace("\r\n", "\n");
    pretty_assertions::assert_eq!(expected, actual, "{case}");
}

/// Everything parsed from a fixture folder as text: each file's IR, then what was left out,
/// failed or decoded by guess.
pub fn ir_dump(language: SourceLanguage, case: &str) -> String {
    let dir = fixture(language, case);
    let files = collect_sources_for(language, std::slice::from_ref(&dir)).unwrap();
    let parsed = parse_sources(language, &files);
    let name = |p: &Path| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
    let mut out = String::new();
    for unit in &parsed.units {
        let _ = writeln!(out, "== {}", name(&unit.path));
        out.push_str(&unit.dump());
    }
    for (skip, n) in &parsed.skipped {
        let _ = writeln!(out, "skipped {} × {n}", skip.key());
    }
    for failed in &parsed.failed {
        let _ = match &failed.problem {
            Problem::Syntax(e) => writeln!(
                out,
                "failed {} at {}:{} near {:?}",
                name(&failed.path),
                e.line,
                e.column,
                e.token
            ),
            Problem::Read(e) => writeln!(out, "failed {}: {e}", name(&failed.path)),
        };
    }
    for (path, encoding) in &parsed.guessed_encodings {
        let _ = writeln!(out, "guessed {} as {encoding}", name(path));
    }
    out
}

/// Compares the IR of `case` with its `expected.ir`.
pub fn check_ir(language: SourceLanguage, case: &str) {
    snapshot(language, case, "expected.ir", &ir_dump(language, case));
}

/// Imports `case` into `doc` and checks the model's invariants.
pub fn import(doc: &mut Document, language: SourceLanguage, case: &str) -> Report {
    let report = import_into(doc, language, &[fixture(language, case)], "Import").unwrap();
    doc.model().check().unwrap();
    report
}

/// `case` imported into a new project.
pub fn fresh(language: SourceLanguage, case: &str) -> (Document, Report) {
    let mut doc = Document::new(Model::new("t"));
    let report = import(&mut doc, language, case);
    (doc, report)
}

/// Compares the model with the `expected.model` of `case`.
pub fn check_model(language: SourceLanguage, case: &str, model: &Model) {
    snapshot(language, case, "expected.model", &dump(model));
}

pub fn qualified(model: &Model, id: Id) -> String {
    let mut names: Vec<String> = model
        .ancestors(id)
        .into_iter()
        .rev()
        .filter(|&a| a != model.root())
        .filter_map(|a| model.get(a).map(|e| e.name().to_owned()))
        .collect();
    names.push(model.get(id).map(|e| e.name().to_owned()).unwrap_or_default());
    names.join(".")
}

/// The model as an indented tree, with what each element is about.
pub fn dump(model: &Model) -> String {
    let mut out = String::new();
    for &child in model.children(model.root()) {
        dump_element(model, child, 0, &mut out);
    }
    out
}

fn dump_element(model: &Model, id: Id, depth: usize, out: &mut String) {
    let e = model.get(id).unwrap();
    let pad = "  ".repeat(depth);
    let line = match e.kind() {
        ElementKind::Package => format!("package {}", e.name()),
        ElementKind::Class(p) | ElementKind::Interface(p) | ElementKind::Enumeration(p) => {
            let flags: String = [(p.is_abstract, " abstract"), (p.is_leaf, " leaf")]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, f)| *f)
                .collect();
            format!("{} {}{flags} {:?}", e.kind().type_name(), e.name(), e.data().visibility)
        }
        ElementKind::Attribute(p) => {
            let flags: String = [(p.is_static, " {static}"), (p.is_read_only, " {readOnly}")]
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, f)| *f)
                .collect();
            format!("attribute {}{flags}", member_text(model, id))
        }
        ElementKind::Operation(p) => {
            let flags: String = [
                (p.is_static, " {static}"),
                (p.is_abstract, " {abstract}"),
                (p.is_leaf, " {leaf}"),
                (p.is_query, " {query}"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, f)| *f)
            .collect();
            format!("operation {}{flags}", member_text(model, id))
        }
        ElementKind::EnumerationLiteral => format!("literal {}", e.name()),
        ElementKind::TemplateParameter(_) => format!("template {}", member_text(model, id)),
        ElementKind::Parameter(_) => return,
        ElementKind::Generalization(p) => {
            format!("generalization -> {}", qualified(model, p.general))
        }
        ElementKind::Realization(p) => format!("realization -> {}", qualified(model, p.supplier)),
        ElementKind::Association(p) => {
            let end = |i: usize| {
                let end = &p.ends[i];
                let mut s = qualified(model, end.participant);
                let _ = write!(s, " {:?}", end.navigability);
                if !end.role.is_empty() {
                    let _ = write!(s, " {} {:?}", end.role, end.visibility);
                }
                if let Some(m) = end.multiplicity {
                    let _ = write!(s, " [{m}]");
                }
                for c in &end.constraints {
                    let _ = write!(s, " {{{c}}}");
                }
                for q in &end.qualifiers {
                    let _ = write!(
                        s,
                        " qualifier {} : {}",
                        q.name,
                        type_label(model, &q.type_ref).unwrap_or_default()
                    );
                }
                for (on, flag) in [(end.is_read_only, "readOnly"), (end.is_static, "static")] {
                    if on {
                        let _ = write!(s, " {{{flag}}}");
                    }
                }
                if !end.default_value.is_empty() {
                    let _ = write!(s, " = {}", end.default_value);
                }
                s
            };
            format!("association {} -- {}", end(0), end(1))
        }
        other => format!("{} {}", other.type_name(), e.name()),
    };
    let _ = writeln!(out, "{pad}{line}");
    for t in &e.data().tagged_values {
        let _ = writeln!(out, "{pad}  @{} = {:?}", t.key, t.value);
    }
    if !e.data().documentation.is_empty() {
        let _ = writeln!(out, "{pad}  doc {:?}", e.data().documentation);
    }
    for &child in model.children(id) {
        dump_element(model, child, depth + 1, out);
    }
}

/// The element at a dotted path from the root.
pub fn at(model: &Model, path: &str) -> Id {
    path.split('.').fold(model.root(), |owner, name| {
        *model
            .children(owner)
            .iter()
            .find(|&&c| model.get(c).is_some_and(|e| e.name() == name))
            .unwrap_or_else(|| panic!("no {name} in {path}"))
    })
}

pub fn attribute_type(model: &Model, class: &str, attribute: &str) -> String {
    let id = at(model, &format!("{class}.{attribute}"));
    match model.get(id).unwrap().kind() {
        ElementKind::Attribute(p) => type_text(model, p.type_ref.as_ref().unwrap()),
        _ => panic!("{attribute} is no attribute"),
    }
}

/// Whether the attribute `attribute` of `class`, or the association that replaced it, refers to
/// `target`.
pub fn links_to(model: &Model, class: &str, attribute: &str, target: &str) -> bool {
    let target = at(model, target);
    let owner = at(model, class);
    let ends = model.elements().find_map(|e| match e.kind() {
        ElementKind::Association(p) => (0..2)
            .find(|&i| p.ends[i].role == attribute && p.ends[1 - i].participant == owner)
            .map(|i| p.ends[i].participant),
        _ => None,
    });
    if let Some(participant) = ends {
        return participant == target;
    }
    let id = at(model, &format!("{class}.{attribute}"));
    match model.get(id).unwrap().kind() {
        ElementKind::Attribute(p) => p.type_ref.as_ref().unwrap().ids().contains(&target),
        _ => false,
    }
}
