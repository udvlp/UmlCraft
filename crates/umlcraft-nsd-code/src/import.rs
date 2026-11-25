//! Filing the structograms of a source import in a project: the files are parsed once for
//! classes and methods, the structograms of the methods chosen are built, and filed under the
//! class of the operation each shows, linked to it. A structogram the import made carries
//! [`IMPORTED_TAG`] until it is edited by hand and [`METHOD_TAG`] always: a new import replaces
//! or deletes the untouched ones of its files and leaves edited ones alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use umlcraft_model::{
    ElementData, ElementKind, IMPORTED_TAG, Id, Model, StructogramProps, TaggedValue, Tx,
};
use umlcraft_nsd::Diagram;
use umlcraft_reverse::lang::{Skip, Skipped};
use umlcraft_reverse::{Parsed, Placed};

use crate::{Made, MethodRef, Report, Source, SourceLanguage, Unbuilt};

/// Which method of which file a structogram shows (`crate::method_key`); stays
/// when it is edited.
pub const METHOD_TAG: &str = "umlcraft.method";

/// The files of a source import, parsed once for classes and structograms.
pub struct Found {
    pub language: SourceLanguage,
    pub parsed: Parsed,
    /// Every file read, with its unit in `parsed` (none for a file with a syntax error, whose
    /// methods are offered all the same).
    pub files: Vec<(PathBuf, Option<usize>)>,
    /// The methods with a body, with the index of their file.
    pub methods: Vec<(usize, MethodRef)>,
    /// The folder the paths in the keys of structograms start from
    /// (`crate::key_base`).
    pub base: PathBuf,
}

impl Found {
    /// A file's path from [`Found::base`], `/` between folders: `shop/orders/views.py`.
    pub fn shown(&self, file: usize) -> String {
        crate::file_path(&self.base, &self.files[file].0)
    }
}

/// Reads and parses `files` (in the background) as `umlcraft_reverse::parse_sources` does,
/// plus the methods of each file from the same tree.
pub fn parse(language: SourceLanguage, files: &[PathBuf]) -> Found {
    let base = crate::key_base(files);
    let mut found =
        Found { language, parsed: Parsed::default(), files: Vec::new(), methods: Vec::new(), base };
    for path in files {
        let Some((tree, unit)) = umlcraft_reverse::parse_file(language, path, &mut found.parsed)
        else {
            continue;
        };
        let source = Source::new(tree);
        let file = found.files.len();
        found.methods.extend(source.methods().into_iter().map(|m| (file, m)));
        found.files.push((path.clone(), unit));
    }
    found
}

/// A structogram a source import made.
pub struct Imported {
    /// Which method it shows (`crate::method_key`).
    pub key: String,
    pub id: Id,
    /// Whether it is as the import made it (not edited by hand).
    pub untouched: bool,
    /// The operation it is linked to.
    operation: Option<Id>,
    /// Where it stands in the model.
    owner: Option<Id>,
}

/// The structograms a source import made.
pub fn imported(model: &Model) -> Vec<Imported> {
    model
        .elements()
        .filter_map(|e| {
            let ElementKind::Structogram(p) = e.kind() else { return None };
            let tags = &e.data().tagged_values;
            let key = tags.iter().find(|t| t.key == METHOD_TAG)?.value.clone();
            let untouched = tags.iter().any(|t| t.key == IMPORTED_TAG);
            Some(Imported { key, id: e.id(), untouched, operation: p.operation, owner: e.owner() })
        })
        .collect()
}

/// For each method of `found`, the structogram of `existing` made from it before, if any
/// (`crate::pair`); `prefer` tells (method, structogram) pairs to try first.
pub fn paired(
    found: &Found,
    existing: &[Imported],
    prefer: impl Fn(usize, usize) -> bool,
) -> Vec<Option<usize>> {
    let keys: Vec<&str> = existing.iter().map(|e| e.key.as_str()).collect();
    let methods: Vec<(&Path, &MethodRef)> =
        found.methods.iter().map(|(file, m)| (found.files[*file].0.as_path(), m)).collect();
    crate::pair(&keys, &methods, prefer)
}

/// A structogram built from a method.
pub struct Built {
    /// Index into [`Found::methods`].
    pub method: usize,
    /// The diagram with its report, or why there is none: a method that nests too deeply for
    /// a structogram gets none and keeps what it had.
    built: Result<(Diagram, Report), Unbuilt>,
}

/// Builds the structograms of the methods `picked` (in the background), reading their files
/// again; a file that cannot be read any more gives none, nor does a method gone from it.
pub fn build(found: &Found, picked: &[usize], made: &Made) -> Vec<Built> {
    let mut by_file: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in picked {
        by_file.entry(found.methods[i].0).or_default().push(i);
    }
    let mut built = Vec::new();
    for (file, methods) in by_file {
        let Ok(decoded) = crate::read(&found.files[file].0) else { continue };
        let source = Source::parse(found.language, decoded.text);
        let refs: Vec<MethodRef> = methods.iter().map(|&i| found.methods[i].1.clone()).collect();
        for (i, result) in methods.into_iter().zip(source.diagrams(&refs, made)) {
            if !matches!(result, Err(Unbuilt::NotFound)) {
                built.push(Built { method: i, built: result });
            }
        }
    }
    built.sort_by_key(|b| b.method);
    built
}

/// What of `built` the import leaves out on purpose, for its report: the methods that nest
/// too deeply for a structogram (`Skip::DeepMethod`).
pub fn skipped(built: &[Built]) -> Skipped {
    let deep = built.iter().filter(|b| matches!(b.built, Err(Unbuilt::TooDeep))).count();
    let mut skipped = Skipped::new();
    if deep > 0 {
        skipped.insert(Skip::DeepMethod, deep);
    }
    skipped
}

/// Files the structograms of a source import, after `umlcraft_reverse::apply_placed` put the
/// classes where `placed` says: those `built` are created or replace the untouched ones made
/// from their methods before; untouched ones of the imported files that were not chosen, or
/// whose method is gone, are deleted. Structograms of other files, also of files with the
/// same name elsewhere, are left alone. Keys of earlier versions are brought up to date
/// (`crate::better_key`). Returns each structogram made or replaced, with the
/// report of its method.
pub fn file_structograms(
    tx: &mut Tx<'_>,
    found: &Found,
    placed: &Placed,
    built: Vec<Built>,
) -> umlcraft_model::Result<Vec<(Id, Report)>> {
    let existing = imported(tx.model());
    // The operation each method's structogram is linked to.
    let operations: Vec<Option<Id>> = found
        .methods
        .iter()
        .map(|(file, m)| placed.operation(found.files[*file].1?, m.declaration()?))
        .collect();
    // Of keys that name a method alike (a file name only), the one linked to its operation.
    let pairs = paired(found, &existing, |m, k| {
        operations[m].is_some() && operations[m] == existing[k].operation
    });
    let mut kept = vec![false; existing.len()];
    let mut made = Vec::new();
    for b in built {
        let (file, method) = &found.methods[b.method];
        let (path, unit) = &found.files[*file];
        let before = pairs[b.method].map(|k| (k, &existing[k]));
        if let Some((k, _)) = before {
            kept[k] = true;
        }
        // Too deep for a structogram: the one it has stays as it is.
        let Ok((diagram, report)) = b.built else { continue };
        let new_key = crate::method_key(&found.base, path, method);
        let key = match before {
            Some((_, e)) => crate::better_key(&e.key, &new_key),
            None => new_key,
        };
        let root = tx.model().root();
        let operation = operations[b.method];
        let owner = match operation {
            Some(op) => tx.model().element(op)?.owner().unwrap_or(root),
            None => unit
                .and_then(|u| placed.type_at(u, &method.class).or_else(|| placed.namespace(u)))
                .unwrap_or(root),
        };
        let structogram = umlcraft_nsd::content::normalize(&diagram);
        let id = match before {
            // Edited by hand: not offered, but listed as ticked; only its key is renewed.
            Some((_, e)) if !e.untouched => {
                if key != e.key {
                    tx.update(e.id, |data| set_method(data, &key))?;
                }
                continue;
            }
            Some((_, e)) => {
                tx.update(e.id, |data| {
                    set_method(data, &key);
                    if let ElementKind::Structogram(p) = &mut data.kind {
                        p.structogram = structogram;
                        p.operation = operation;
                    }
                })?;
                if tx.model().element(e.id)?.owner() != Some(owner) {
                    let index = tx.model().children(owner).len();
                    tx.move_element(e.id, owner, index)?;
                }
                e.id
            }
            None => {
                let kind = ElementKind::Structogram(StructogramProps { operation, structogram });
                let mut data = ElementData::new("", kind);
                data.tagged_values = vec![
                    TaggedValue { key: IMPORTED_TAG.to_owned(), value: "true".to_owned() },
                    TaggedValue { key: METHOD_TAG.to_owned(), value: key },
                ];
                tx.create(owner, data)?
            }
        };
        made.push((id, report));
    }
    // Untouched structograms of these files that were not built again. A key of 0.22.0 names
    // a file by its name only: it counts as one of these files where it names a method of
    // them, or where it stands under a class or package this import put there.
    let stale: Vec<Id> = existing
        .iter()
        .enumerate()
        .filter(|&(k, e)| {
            let file = found.files.iter().any(|(p, _)| crate::names_file(&e.key, p));
            let short = !crate::key_path(&e.key).contains('/');
            let ours = !short
                || pairs.contains(&Some(k))
                || e.owner.is_some_and(|owner| placed.holds(owner));
            e.untouched && !kept[k] && file && ours
        })
        .map(|(_, e)| e.id)
        .collect();
    tx.delete_many(&stale)?;
    Ok(made)
}

/// Makes `key` the method a structogram's data names.
fn set_method(data: &mut ElementData, key: &str) {
    match data.tagged_values.iter_mut().find(|t| t.key == METHOD_TAG) {
        Some(tag) => tag.value = key.to_owned(),
        None => data
            .tagged_values
            .push(TaggedValue { key: METHOD_TAG.to_owned(), value: key.to_owned() }),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::{Made, Report, SourceLanguage};
    use umlcraft_model::{Document, ElementKind, IMPORTED_TAG, Id, Model};
    use umlcraft_nsd::Diagram;

    use super::{METHOD_TAG, build, file_structograms, imported, parse, skipped};

    const MADE: Made = Made {
        author: "tester",
        date: "2026-10-30",
        origin: "test",
        keywords: umlcraft_nsd::Keywords::English,
    };

    /// A folder of its own for `name`'s files, emptied first.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umlcraft-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Imports `file` with the structograms of the methods named `picked`.
    fn import(doc: &mut Document, file: &Path, picked: &[&str]) -> Vec<(Id, Report)> {
        import_files(doc, &[file.to_owned()], picked)
    }

    /// Imports `files` with the structograms of the methods named `picked`.
    fn import_files(doc: &mut Document, files: &[PathBuf], picked: &[&str]) -> Vec<(Id, Report)> {
        let found = parse(SourceLanguage::Java, files);
        let picked: Vec<usize> = (0..found.methods.len())
            .filter(|&i| picked.contains(&found.methods[i].1.name.as_str()))
            .collect();
        let built = build(&found, &picked, &MADE);
        doc.edit("Import", |tx| {
            let (_, placed) = umlcraft_reverse::apply_placed(tx, found.language, &found.parsed)?;
            file_structograms(tx, &found, &placed, built)
        })
        .unwrap()
    }

    fn structogram_of<'m>(model: &'m Model, method: &str) -> Option<&'m umlcraft_model::Element> {
        model.elements().find(|e| {
            e.data().tagged_values.iter().any(|t| t.key == METHOD_TAG && t.value.contains(method))
        })
    }

    #[test]
    fn structograms_go_under_their_class_and_a_new_import_keeps_what_was_edited() {
        let dir = folder("reimport");
        let file = dir.join("Shop.java");
        let first = "package shop;\nclass Cart {\n  int total(int a) { return a + 1; }\n  void clear() { total(0); }\n  void log() { }\n}\n";
        std::fs::write(&file, first).unwrap();
        let mut doc = Document::new(Model::new("t"));

        let made = import(&mut doc, &file, &["total", "clear", "log"]);
        assert_eq!(made.len(), 3);
        let model = doc.model();
        let total = structogram_of(model, "total(int a)").unwrap();
        let cart = model.get(total.owner().unwrap()).unwrap();
        assert_eq!(cart.name(), "Cart");
        let ElementKind::Structogram(p) = total.kind() else { panic!() };
        let operation = model.get(p.operation.unwrap()).unwrap();
        assert_eq!((operation.name(), operation.owner()), ("total", Some(cart.id())));
        assert!(total.data().tagged_values.iter().any(|t| t.key == IMPORTED_TAG));
        model.check().unwrap();

        // `total` edited by hand; `clear` left as imported; `log` gone from the source.
        let (total, clear) = (total.id(), structogram_of(model, "clear()").unwrap().id());
        let made = umlcraft_nsd::Provenance { author: "tester", date: "2026-10-30", origin: "t" };
        let edited = Diagram::new(umlcraft_nsd::RootKind::Program, &["mine".into()], &made);
        doc.edit("Edit", |tx| tx.set_structogram(total, &edited)).unwrap();
        let tags = &doc.model().get(total).unwrap().data().tagged_values;
        assert!(!tags.iter().any(|t| t.key == IMPORTED_TAG));
        assert!(tags.iter().any(|t| t.key == METHOD_TAG));
        let second = "package shop;\nclass Cart {\n  int total(int a) { return a + 2; }\n  void clear() { total(1); }\n}\n";
        std::fs::write(&file, second).unwrap();

        // Ticked again: `clear` is renewed in place, the edited `total` stays as it is.
        let made = import(&mut doc, &file, &["total", "clear"]);
        let model = doc.model();
        assert_eq!(made.iter().map(|m| m.0).collect::<Vec<_>>(), vec![clear]);
        assert!(structogram_of(model, "log()").is_none());
        let ElementKind::Structogram(p) = model.get(total).unwrap().kind() else { panic!() };
        assert_eq!(p.structogram.text(), vec!["mine".to_owned()]);

        // Not ticked: the untouched one goes, the edited one stays.
        import(&mut doc, &file, &[]);
        let model = doc.model();
        assert!(model.get(clear).is_none());
        assert!(model.get(total).is_some());
        model.check().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The structograms of the import with their keys and their first instruction.
    fn shown(model: &Model) -> Vec<(String, Vec<String>)> {
        let mut shown: Vec<(String, Vec<String>)> = imported(model)
            .iter()
            .map(|e| {
                let ElementKind::Structogram(p) = model.get(e.id).unwrap().kind() else { panic!() };
                let first = p.structogram.children.elements.first().map(|e| e.text());
                (e.key.clone(), first.unwrap_or_default())
            })
            .collect();
        shown.sort();
        shown
    }

    /// `a/Util.java` and `b/Util.java` both with `class Util { int f() }`: each keeps its own
    /// structogram, and an import of one leaves the other's alone.
    #[test]
    fn files_of_the_same_name_keep_their_own_structograms() {
        let dir = folder("same-name");
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let files = [dir.join("a").join("Util.java"), dir.join("b").join("Util.java")];
        for (file, (package, value)) in files.iter().zip([("a", 1), ("b", 2)]) {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            let text =
                format!("package {package};\nclass Util {{\n  int f() {{ return {value}; }}\n}}\n");
            std::fs::write(file, text).unwrap();
        }
        let mut doc = Document::new(Model::new("t"));
        let made = import_files(&mut doc, &files, &["f"]);
        assert_eq!(made.len(), 2);
        let expected = vec![
            (format!("{name}/a/Util.java:Util:int f()"), vec!["return 1".to_owned()]),
            (format!("{name}/b/Util.java:Util:int f()"), vec!["return 2".to_owned()]),
        ];
        assert_eq!(shown(doc.model()), expected);
        // Again: each renewed in place, none mixed up.
        let again = import_files(&mut doc, &files, &["f"]);
        assert_eq!(
            again.iter().map(|m| m.0).collect::<Vec<_>>(),
            made.iter().map(|m| m.0).collect::<Vec<_>>()
        );
        assert_eq!(shown(doc.model()), expected);
        // `b` alone, nothing ticked: its structogram goes, `a`'s stays.
        import(&mut doc, &files[1], &[]);
        assert_eq!(shown(doc.model()), expected[..1]);
        doc.model().check().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    /// UmlCraft 0.22.0 named a structogram's file by its name only: such a key is found
    /// again and renewed, and a file of the same name in another folder leaves it alone.
    #[test]
    fn keys_of_0_22_are_found_again_and_renewed() {
        let dir = folder("old-keys");
        let (shop, other) =
            (dir.join("shop").join("Shop.java"), dir.join("other").join("Shop.java"));
        for (file, text) in [
            (&shop, "package shop;\nclass Cart {\n  int total(int a) { return a; }\n}\n"),
            (&other, "package other;\nclass Till {\n  void open() { }\n}\n"),
        ] {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        let mut doc = Document::new(Model::new("t"));
        let id = import(&mut doc, &shop, &["total"])[0].0;
        let old = "Shop.java:Cart:int total(int a)";
        doc.edit("0.22.0", |tx| {
            tx.update(id, |data| {
                data.tagged_values.iter_mut().find(|t| t.key == METHOD_TAG).unwrap().value =
                    old.to_owned()
            })
        })
        .unwrap();
        import(&mut doc, &other, &[]);
        assert_eq!(shown(doc.model())[0].0, old);
        let made = import(&mut doc, &shop, &["total"]);
        assert_eq!(made[0].0, id);
        // One file imported: its path starts with its folder.
        assert_eq!(shown(doc.model())[0].0, "shop/Shop.java:Cart:int total(int a)");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A method nested too deeply for a structogram gets none and is counted as left out.
    #[test]
    fn a_method_too_deep_for_a_structogram_is_left_out() {
        let dir = folder("deep");
        let file = dir.join("Deep.java");
        let chain: String = (1..300).map(|i| format!("    else if (x == {i}) a();\n")).collect();
        let text = format!(
            "class Deep {{\n  void f(int x) {{\n    if (x == 0) a();\n{chain}  }}\n  void g() {{ }}\n}}\n"
        );
        std::fs::write(&file, text).unwrap();
        let found = parse(SourceLanguage::Java, std::slice::from_ref(&file));
        let built = build(&found, &[0, 1], &MADE);
        let left_out = skipped(&built);
        assert_eq!(left_out.get(&umlcraft_reverse::lang::Skip::DeepMethod), Some(&1));
        let mut doc = Document::new(Model::new("t"));
        let made = doc
            .edit("Import", |tx| {
                let (_, placed) =
                    umlcraft_reverse::apply_placed(tx, found.language, &found.parsed)?;
                file_structograms(tx, &found, &placed, built)
            })
            .unwrap();
        assert_eq!(made.len(), 1);
        assert_eq!(imported(doc.model()).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_the_class_import_cannot_read_still_offers_its_methods() {
        let dir = folder("syntax");
        let file = dir.join("Broken.java");
        std::fs::write(&file, "class Broken {\n  void ok() { run(); }\n  void bad( { }\n").unwrap();
        let found = parse(SourceLanguage::Java, std::slice::from_ref(&file));
        assert_eq!(found.files[0].1, None);
        assert_eq!(found.parsed.failed.len(), 1);
        assert!(found.methods.iter().any(|(_, m)| m.name == "ok"));
        let mut doc = Document::new(Model::new("t"));
        let made = import(&mut doc, &file, &["ok"]);
        let model = doc.model();
        assert_eq!(model.get(made[0].0).unwrap().owner(), Some(model.root()));
        let _ = std::fs::remove_dir_all(dir);
    }
}
