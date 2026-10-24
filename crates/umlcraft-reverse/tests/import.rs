//! Java imports into a fresh model, one per fixture: the model as text is compared with
//! `expected.model` (write it with `UPDATE_SNAPSHOTS=1` and review the diff), and the rules each
//! fixture is about are checked directly.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use umlcraft_model::{Document, ElementKind, Id, Model, Navigability, Rect, TypeRef};
use umlcraft_notation::member::{member_text, type_label, type_text};
use umlcraft_reverse::{Report, SourceLanguage, import_into};

fn fixture(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java").join(case)
}

fn import(doc: &mut Document, case: &str) -> Report {
    let report = import_into(doc, SourceLanguage::Java, &[fixture(case)], "Import").unwrap();
    doc.model().check().unwrap();
    report
}

fn fresh(case: &str) -> (Document, Report) {
    let mut doc = Document::new(Model::new("t"));
    let report = import(&mut doc, case);
    (doc, report)
}

fn qualified(model: &Model, id: Id) -> String {
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
fn dump(model: &Model) -> String {
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

fn check(case: &str, model: &Model) {
    let actual = dump(model);
    let path = fixture(case).join("expected.model");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing; run with UPDATE_SNAPSHOTS=1", path.display()))
        .replace("\r\n", "\n");
    pretty_assertions::assert_eq!(expected, actual, "{case}");
}

/// The element at a dotted path from the root.
fn at(model: &Model, path: &str) -> Id {
    path.split('.').fold(model.root(), |owner, name| {
        *model
            .children(owner)
            .iter()
            .find(|&&c| model.get(c).is_some_and(|e| e.name() == name))
            .unwrap_or_else(|| panic!("no {name} in {path}"))
    })
}

fn attribute_type(model: &Model, class: &str, attribute: &str) -> String {
    let id = at(model, &format!("{class}.{attribute}"));
    match model.get(id).unwrap().kind() {
        ElementKind::Attribute(p) => type_text(model, p.type_ref.as_ref().unwrap()),
        _ => panic!("{attribute} is no attribute"),
    }
}

/// Whether the attribute `attribute` of `class`, or the association that replaced it, refers to
/// `target`.
fn links_to(model: &Model, class: &str, attribute: &str, target: &str) -> bool {
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

#[test]
fn packages_nest_and_the_default_package_is_the_root() {
    let (doc, _) = fresh("packages");
    let model = doc.model();
    at(model, "a.b.Inner");
    at(model, "a.Outer");
    at(model, "Root");
    check("packages", model);
}

#[test]
fn kinds_map_to_classes_interfaces_and_enumerations() {
    let (doc, _) = fresh("kinds");
    let model = doc.model();
    let kind = |path: &str| model.get(at(model, path)).unwrap().kind().type_name();
    assert_eq!(kind("kinds.Kinds.Shape"), "interface");
    assert_eq!(kind("kinds.Kinds.Color"), "enumeration");
    assert_eq!(kind("kinds.Kinds.Point"), "class");
    // Astah makes annotation types classes.
    assert_eq!(kind("kinds.Kinds.Marker"), "class");
    assert_eq!(kind("kinds.Kinds.Nested.Deeper"), "class");
    check("kinds", model);
}

#[test]
fn modifiers() {
    let (doc, _) = fresh("modifiers");
    check("modifiers", doc.model());
}

#[test]
fn javadoc_becomes_documentation() {
    let (doc, _) = fresh("javadoc");
    let model = doc.model();
    let documentation =
        |path: &str| model.get(at(model, path)).unwrap().data().documentation.clone();
    assert_eq!(documentation("javadoc.Documented.reset"), "This one.");
    assert_eq!(documentation("javadoc.Unit.METRE"), "Metres.");
    check("javadoc", model);
}

#[test]
fn annotations() {
    let (doc, _) = fresh("annotations");
    check("annotations", doc.model());
}

#[test]
fn initializers_become_default_values() {
    let (doc, report) = fresh("initializers");
    assert_eq!(report.skipped.values().sum::<usize>(), 6);
    check("initializers", doc.model());
}

#[test]
fn inheritance_follows_extends_and_implements() {
    let (doc, _) = fresh("inheritance");
    let model = doc.model();
    // Library types the sources implement are interfaces at their package path.
    let kind = |path: &str| model.get(at(model, path)).unwrap().kind().type_name();
    assert_eq!(kind("java.io.Serializable"), "interface");
    assert_eq!(kind("java.lang.Cloneable"), "interface");
    check("inheritance", model);
}

#[test]
fn generics_link_their_arguments() {
    let (doc, _) = fresh("generics");
    let model = doc.model();
    assert!(links_to(model, "generics.Generics", "index", "generics.Foo"));
    assert!(links_to(model, "generics.Generics", "index", "java.util.List"));
    assert_eq!(attribute_type(model, "generics.Generics", "upper"), "List<? extends T>");
    assert!(links_to(model, "generics.Generics", "entry", "java.util.Map.Entry"));
    check("generics", model);
}

#[test]
fn arrays_and_varargs_keep_their_modifiers() {
    let (doc, _) = fresh("arrays_varargs");
    let model = doc.model();
    assert_eq!(attribute_type(model, "arrays_varargs.Arrays", "b"), "int[]");
    assert_eq!(attribute_type(model, "arrays_varargs.Arrays", "grid"), "int[][]");
    check("arrays_varargs", model);
}

#[test]
fn names_resolve_in_astahs_order() {
    let (doc, _) = fresh("resolution");
    let model = doc.model();
    let c = "app.App";
    let linked = |attribute: &str, target: &str| links_to(model, c, attribute, target);
    assert!(linked("value", "app.App.T"));
    assert!(linked("inner", "app.App.Inner"));
    assert!(linked("shared", "app.Base.Shared"));
    assert!(linked("helper", "lib.Helper"));
    assert!(linked("gone", "lib.missing.Gone"));
    assert!(linked("base", "app.Base"));
    assert!(linked("date", "java.util.Date"));
    assert!(linked("name", "java.lang.String"));
    assert!(linked("file", "java.io.File"));
    assert!(linked("qualified", "lib.Helper"));
    assert!(linked("entry", "java.util.Map.Entry"));
    assert!(linked("thing", "com.acme.Thing"));
    // Unknown simple names go to the root, Astah's default package.
    assert!(linked("unknown", "Unknown"));
    // An unknown name after `implements` is an interface.
    assert_eq!(model.get(at(model, "Unseen")).unwrap().kind().type_name(), "interface");
    // The operation's own template parameter.
    let first = at(model, "app.App.first");
    let return_type = match model.get(first).unwrap().kind() {
        ElementKind::Operation(p) => p.return_type.clone().unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(return_type, TypeRef::Element(at(model, "app.App.first.E")));
    check("resolution", model);
}

#[test]
fn attributes_of_class_types_become_associations() {
    let (doc, report) = fresh("associations");
    let model = doc.model();
    assert_eq!(report.associations, 7);
    // Values, template parameters and `Optional` (which Astah does not unwrap) stay attributes.
    for kept in ["name", "count", "tags", "notes", "current"] {
        at(model, &format!("shop.Shop.{kept}"));
    }
    at(model, "shop.Box.content");
    at(model, "shop.Box.more");
    check("associations", model);
}

#[test]
fn syntax_errors_skip_only_their_file() {
    let (doc, report) = fresh("syntax_error");
    at(doc.model(), "syntax_error.Fine");
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].path.ends_with("Broken.java"));
}

#[test]
fn encodings_are_detected() {
    let (doc, report) = fresh("encoding");
    let model = doc.model();
    assert_eq!(model.get(at(model, "Legacy")).unwrap().data().documentation, "Grüße aus München.");
    at(model, "Legacy.straße");
    assert_eq!(report.guessed_encodings.len(), 1);
}

#[test]
fn the_import_is_one_undo_step() {
    let mut doc = Document::new(Model::new("t"));
    let before = doc.model().clone();
    import(&mut doc, "associations");
    assert!(doc.undo());
    assert_eq!(*doc.model(), before);
}

#[test]
fn a_type_that_changes_kind_keeps_its_place() {
    let dir = std::env::temp_dir().join(format!("umlcraft-retype-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Shape.java");
    let mut doc = Document::new(Model::new("t"));
    std::fs::write(&file, "package geo;\nclass Shape {\n    int sides;\n}\n").unwrap();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    let shape = at(doc.model(), "geo.Shape");
    let diagram = doc
        .edit("Diagram", |tx| {
            let root = tx.model().root();
            let kind = ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            let d = tx.create(root, umlcraft_model::ElementData::new("d", kind))?;
            tx.add_node(d, Some(shape), Rect::new(0.0, 0.0, 100.0, 60.0))?;
            Ok(d)
        })
        .unwrap();
    std::fs::write(&file, "package geo;\ninterface Shape {\n    int sides();\n}\n").unwrap();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let model = doc.model();
    let shape = at(model, "geo.Shape");
    assert_eq!(model.get(shape).unwrap().kind().type_name(), "interface");
    at(model, "geo.Shape.sides");
    let shown: Vec<Id> =
        model.view(diagram).unwrap().presentations().iter().filter_map(|p| p.element).collect();
    assert_eq!(shown, [shape]);
    model.check().unwrap();
}

/// Imports 1000 generated files, then again. Run with `--release --ignored` to time it.
#[test]
#[ignore]
fn large_imports_are_fast() {
    let dir = std::env::temp_dir().join(format!("umlcraft-large-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for p in 0..20 {
        let package = dir.join(format!("p{p}"));
        std::fs::create_dir_all(&package).unwrap();
        for c in 0..50 {
            let other = format!("p{}.C{}", (p + 1) % 20, (c + 7) % 50);
            let source = format!(
                "package p{p};\nimport java.util.*;\n/** Class {c}. */\npublic class C{c} extends Base{c} implements Comparable<C{c}> {{\n    private int a; private String b; private List<{other}> items; private {other} link;\n    private Map<String, {other}> index;\n    public int f(int x, String y) {{ return 0; }}\n    public <T> T g(List<T> t) {{ return null; }}\n    public int compareTo(C{c} o) {{ return 0; }}\n}}\nclass Base{c} {{ protected long id; }}\n"
            );
            std::fs::write(package.join(format!("C{c}.java")), source).unwrap();
        }
    }
    let mut doc = Document::new(Model::new("t"));
    let start = std::time::Instant::now();
    let files =
        umlcraft_reverse::collect_sources_for(SourceLanguage::Java, std::slice::from_ref(&dir))
            .unwrap();
    let parsed = umlcraft_reverse::parse_sources(SourceLanguage::Java, &files);
    let parsing = start.elapsed();
    doc.edit("Import", |tx| umlcraft_reverse::apply(tx, SourceLanguage::Java, &parsed)).unwrap();
    let first = start.elapsed();
    eprintln!("parsing {parsing:?}");
    let start = std::time::Instant::now();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    let second = start.elapsed();
    eprintln!(
        "1000 files: first import {first:?}, again {second:?}, {} elements",
        doc.model().len()
    );
    doc.model().check().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_new_import_matches_what_is_there() {
    let mut doc = Document::new(Model::new("t"));
    import(&mut doc, "reimport/v1");
    let model = doc.model();
    let account = at(model, "model.Account");
    let deposit = at(model, "model.Account.deposit");
    let balance = at(model, "model.Account.balance");
    // `Missing` is unknown in v1: a placeholder in the root.
    let missing = at(model, "Missing");
    // Show the account and the placeholder on a diagram.
    let diagram = doc
        .edit("Diagram", |tx| {
            let root = tx.model().root();
            let kind = ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            let d = tx.create(root, umlcraft_model::ElementData::new("d", kind))?;
            tx.add_node(d, Some(account), Rect::new(0.0, 0.0, 100.0, 60.0))?;
            tx.add_node(d, Some(missing), Rect::new(200.0, 0.0, 100.0, 60.0))?;
            Ok(d)
        })
        .unwrap();
    import(&mut doc, "reimport/v2");
    let model = doc.model();
    // Matched members keep their ids.
    assert_eq!(at(model, "model.Account"), account);
    assert_eq!(at(model, "model.Account.balance"), balance);
    assert_eq!(at(model, "model.Account.deposit"), deposit);
    assert_eq!(attribute_type(model, "model.Account", "balance"), "long");
    // v2 declares `model.Missing`; the root placeholder stays, as in Astah, and on the diagram.
    assert_eq!(model.get(at(model, "model.Missing")).unwrap().kind().type_name(), "interface");
    assert_eq!(at(model, "Missing"), missing);
    let shown: Vec<Id> =
        model.view(diagram).unwrap().presentations().iter().filter_map(|p| p.element).collect();
    assert!(shown.contains(&missing));
    // New members take their place in source order: attributes before operations.
    let members: Vec<&str> = model
        .children(account)
        .iter()
        .filter_map(|&c| model.get(c))
        .filter(|e| matches!(e.kind(), ElementKind::Attribute(_) | ElementKind::Operation(_)))
        .map(|e| e.name())
        .collect();
    assert_eq!(members, ["balance", "note", "deposit", "deposit"]);
    // The association for the field that is gone is gone, the other one stays.
    let associations: Vec<String> = model
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Association(p) => Some(p.ends[1].role.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(associations, ["owner"]);
    let navigable = model.elements().any(|e| {
        matches!(e.kind(), ElementKind::Association(p) if p.ends[1].navigability == Navigability::Navigable)
    });
    assert!(navigable);
    check("reimport", model);
}
