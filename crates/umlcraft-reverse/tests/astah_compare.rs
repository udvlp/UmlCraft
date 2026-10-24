//! Compares UmlCraft's Java import with Astah's, fixture by fixture.
//!
//! Astah's import runs only in its user interface, so the references are made by hand: import
//! each folder under `tests/fixtures/java` with Tools › Java › Import Java into an empty
//! project and save it as `<fixture>.asta` in `%USERPROFILE%\UmlCraft-reference\java` (or in
//! the folder `UMLCRAFT_ASTAH_REFERENCE` names); for `reimport`, import `v1` and then `v2`.
//! Attributes are left as they are in the chooser Astah shows afterwards. The test reads each
//! reference with `umlcraft_asta`, imports the fixture itself, and compares both models as a
//! normalized text: types by qualified name, their members in order, template parameters,
//! supertypes, associations, Java tags and documentation. Fixtures without a reference are
//! skipped.
//!
//! Where UmlCraft deliberately differs, because Astah loses what the sources say, the lines
//! that differ are listed in `tests/astah_deviations/<fixture>.txt`, each group explained in
//! `#` lines. The test fails when the differences no longer match that list; review and accept
//! them with `UPDATE_SNAPSHOTS=1`, which keeps the explanations. `encoding` has no reference:
//! Astah stops the whole import at the first file that does not decode with the chosen charset,
//! where UmlCraft falls back to Windows-1252 and says so.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use umlcraft_model::{Document, ElementKind, Id, Model};
use umlcraft_notation::member::{member_text, type_label};
use umlcraft_reverse::{SourceLanguage, import_into};

const CASES: &[&str] = &[
    "packages",
    "kinds",
    "modifiers",
    "javadoc",
    "annotations",
    "initializers",
    "inheritance",
    "generics",
    "arrays_varargs",
    "resolution",
    "associations",
    "reimport",
];

fn reference_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("UMLCRAFT_ASTAH_REFERENCE") {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("USERPROFILE")
        .map(|home| Path::new(&home).join("UmlCraft-reference").join("java"))
}

fn fixture(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java").join(case)
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

fn is_type(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

/// The parts of a model a Java import is about, as sorted text.
fn normalize(model: &Model) -> String {
    let mut types: Vec<(String, Id)> = model
        .elements()
        .filter(|e| is_type(e.kind()))
        .map(|e| (qualified(model, e.id()), e.id()))
        .collect();
    types.sort();
    let mut out = String::new();
    for (name, id) in &types {
        let e = model.get(*id).unwrap();
        let _ = writeln!(out, "{} {name} {:?}", e.kind().type_name(), e.data().visibility);
        let mut tags: Vec<String> = e
            .data()
            .tagged_values
            .iter()
            .filter(|t| t.key.starts_with("jude.java."))
            .map(|t| format!("  @{} = {:?}", t.key, t.value))
            .collect();
        tags.sort();
        for t in tags {
            let _ = writeln!(out, "{t}");
        }
        if !e.data().documentation.is_empty() {
            let _ = writeln!(out, "  doc {:?}", e.data().documentation.replace("\r\n", "\n"));
        }
        let mut supertypes = BTreeSet::new();
        for other in model.elements() {
            match other.kind() {
                ElementKind::Generalization(p) if p.specific == *id => {
                    supertypes.insert(format!("  extends {}", qualified(model, p.general)));
                }
                ElementKind::Realization(p) if p.client == *id => {
                    supertypes.insert(format!("  implements {}", qualified(model, p.supplier)));
                }
                _ => {}
            }
        }
        for s in supertypes {
            let _ = writeln!(out, "{s}");
        }
        for &child in model.children(*id) {
            let Some(c) = model.get(child) else { continue };
            let line = match c.kind() {
                ElementKind::Attribute(p) => {
                    format!(
                        "attribute {} static={} final={}",
                        member_text(model, child),
                        p.is_static,
                        p.is_read_only
                    )
                }
                ElementKind::Operation(p) => format!(
                    "operation {} static={} abstract={} final={}",
                    member_text(model, child),
                    p.is_static,
                    p.is_abstract,
                    p.is_leaf
                ),
                ElementKind::EnumerationLiteral => format!("literal {}", c.name()),
                ElementKind::TemplateParameter(_) => {
                    format!("template {}", member_text(model, child))
                }
                _ => continue,
            };
            let _ = writeln!(out, "  {line}");
            if !c.data().documentation.is_empty() {
                let _ = writeln!(out, "    doc {:?}", c.data().documentation.replace("\r\n", "\n"));
            }
        }
    }
    let mut associations: Vec<String> = model
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Association(p) => {
                let mut ends: Vec<String> = p
                    .ends
                    .iter()
                    .map(|end| {
                        let mut s =
                            format!("{} {:?}", qualified(model, end.participant), end.navigability);
                        if !end.role.is_empty() {
                            let _ = write!(s, " {}", end.role);
                        }
                        if let Some(m) = end.multiplicity {
                            let _ = write!(s, " [{m}]");
                        }
                        let mut constraints = end.constraints.clone();
                        constraints.sort();
                        for c in constraints {
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
                        s
                    })
                    .collect();
                ends.sort();
                Some(format!("association {}", ends.join(" -- ")))
            }
            _ => None,
        })
        .collect();
    associations.sort();
    for a in associations {
        let _ = writeln!(out, "{a}");
    }
    out
}

fn ours(case: &str) -> Model {
    let mut doc = Document::new(Model::new("t"));
    if case == "reimport" {
        for version in ["v1", "v2"] {
            import_into(&mut doc, SourceLanguage::Java, &[fixture(case).join(version)], "Import")
                .unwrap();
        }
    } else {
        import_into(&mut doc, SourceLanguage::Java, &[fixture(case)], "Import").unwrap();
    }
    doc.model().clone()
}

/// The lines only Astah's dump has (`- `) and those only ours has (`+ `), in dump order, from
/// a longest common subsequence of the lines.
fn deviations(astah: &str, ours: &str) -> Vec<String> {
    let (a, b): (Vec<&str>, Vec<&str>) = (astah.lines().collect(), ours.lines().collect());
    // common[i][j]: length of the longest common subsequence of a[i..] and b[j..].
    let mut common = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            common[i][j] = if a[i] == b[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < a.len() || j < b.len() {
        if i < a.len() && j < b.len() && a[i] == b[j] {
            (i, j) = (i + 1, j + 1);
        } else if j == b.len() || (i < a.len() && common[i + 1][j] >= common[i][j + 1]) {
            out.push(format!("- {}", a[i]));
            i += 1;
        } else {
            out.push(format!("+ {}", b[j]));
            j += 1;
        }
    }
    out
}

fn deviations_file(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/astah_deviations").join(format!("{case}.txt"))
}

#[test]
fn imports_like_astah() {
    let Some(dir) = reference_dir() else { return };
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut compared = 0;
    let mut differences = Vec::new();
    for case in CASES {
        let path = dir.join(format!("{case}.asta"));
        if !path.exists() {
            continue;
        }
        let astah = umlcraft_asta::import(&path).unwrap().model;
        let actual = deviations(&normalize(&astah), &normalize(&ours(case)));
        compared += 1;
        // The file explains each deviation in `#` lines, which the comparison skips.
        let file = deviations_file(case);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let (notes, accepted): (Vec<&str>, Vec<&str>) =
            text.lines().partition(|l| l.starts_with('#') || l.is_empty());
        if accepted == actual {
            continue;
        }
        if update {
            let mut out: Vec<String> = notes.iter().map(|n| n.to_string()).collect();
            if !out.is_empty() && !actual.is_empty() {
                out.push(String::new());
            }
            out.extend(actual);
            if out.is_empty() {
                std::fs::remove_file(&file).ok();
            } else {
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(&file, out.join("\n") + "\n").unwrap();
            }
        } else {
            differences.push(format!(
                "== {case}: accepted\n{}\n-- now\n{}",
                accepted.join("\n"),
                actual.join("\n")
            ));
        }
    }
    if compared == 0 {
        eprintln!("no Astah references in {}; skipped", dir.display());
        return;
    }
    assert!(
        differences.is_empty(),
        "deviations from Astah changed (accept with UPDATE_SNAPSHOTS=1):\n{}",
        differences.join("\n")
    );
}
