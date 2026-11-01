//! The structograms of a source import (Tools › Import …). Each file is read and parsed once:
//! its declarations go to the class import, its methods with a body into a dialog (grouped by
//! file and class, with a search field, none ticked) where structograms are chosen. Those are
//! built from the files and filed in the same undo step as the classes, under the class of
//! the operation each shows, linked to it. A structogram the import made carries
//! [`IMPORTED_TAG`] until it is edited by hand and [`METHOD_TAG`] always: a new import replaces
//! or deletes the untouched ones and leaves edited ones alone.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt, WindowExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{
    ElementData, ElementKind, IMPORTED_TAG, Id, Model, StructogramProps, TaggedValue, Tx,
};
use umlcraft_nsd::Diagram;
use umlcraft_nsd_code::{Made, MethodRef, NoteKind, Report, Source, SourceLanguage};
use umlcraft_reverse::lang::Skipped;
use umlcraft_reverse::{FileProblem, Parsed, Placed, Problem};

/// Which method of which file a structogram shows (see [`key`]); stays when it is edited.
pub const METHOD_TAG: &str = "umlcraft.method";

/// At most this many methods are listed at once; the search finds the others.
const LISTED: usize = 300;

/// The files of a source import, parsed once for classes and structograms.
pub struct Found {
    pub language: SourceLanguage,
    pub parsed: Parsed,
    /// Every file read, with its unit in `parsed` (none for a file with a syntax error, whose
    /// methods are offered all the same).
    pub files: Vec<(PathBuf, Option<usize>)>,
    /// The methods with a body, with the index of their file.
    pub methods: Vec<(usize, MethodRef)>,
}

/// Reads and parses `files` (in the background): what `umlcraft_reverse::parse_sources` does,
/// plus the methods of each file from the same tree.
pub fn parse(language: SourceLanguage, files: &[PathBuf]) -> Found {
    let mut found =
        Found { language, parsed: Parsed::default(), files: Vec::new(), methods: Vec::new() };
    for path in files {
        let decoded = match umlcraft_nsd_code::read(path) {
            Ok(decoded) => decoded,
            Err(err) => {
                let problem = Problem::Read(err.to_string());
                found.parsed.failed.push(FileProblem { path: path.clone(), problem });
                continue;
            }
        };
        if let Some(encoding) = decoded.guessed {
            found.parsed.guessed_encodings.push((path.clone(), encoding));
        }
        let source = Source::parse(language, decoded.text);
        let file = found.files.len();
        found.methods.extend(source.methods().into_iter().map(|m| (file, m)));
        // Counted only for files that are imported.
        let mut skipped = Skipped::new();
        let unit = match umlcraft_reverse::lower(source.tree(), path, &mut skipped) {
            Ok(unit) => {
                found.parsed.units.push(unit);
                for (skip, n) in skipped {
                    *found.parsed.skipped.entry(skip).or_default() += n;
                }
                Some(found.parsed.units.len() - 1)
            }
            Err(error) => {
                let problem = Problem::Syntax(error);
                found.parsed.failed.push(FileProblem { path: path.clone(), problem });
                None
            }
        };
        found.files.push((path.clone(), unit));
    }
    found
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// What tells a method's structogram on a new import: file name, classes and signature
/// (`Order.java:Order:void add(Item item)`).
pub fn key(path: &Path, method: &MethodRef) -> String {
    format!("{}:{}:{}", file_name(path), method.class_name(), method.signature)
}

/// The structograms a source import made, by [`key`], and whether each is still untouched.
pub fn imported(model: &Model) -> HashMap<String, (Id, bool)> {
    model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Structogram(_)))
        .filter_map(|e| {
            let tags = &e.data().tagged_values;
            let key = tags.iter().find(|t| t.key == METHOD_TAG)?.value.clone();
            let untouched = tags.iter().any(|t| t.key == IMPORTED_TAG);
            Some((key, (e.id(), untouched)))
        })
        .collect()
}

/// The dialog listing the methods of `found`: those whose structogram the project has are
/// ticked, those edited by hand ticked and locked. `chosen` gets the methods ticked (indices
/// into `found.methods`) when the import goes on; cancelling ends the import.
pub fn offer(
    found: &Found,
    existing: &HashMap<String, (Id, bool)>,
    chosen: impl Fn(Vec<usize>, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    struct Row {
        file: String,
        method: MethodRef,
        locked: bool,
    }
    let mut ticked = BTreeSet::new();
    let rows: Rc<Vec<Row>> = Rc::new(
        found
            .methods
            .iter()
            .enumerate()
            .map(|(i, (file, method))| {
                let path = &found.files[*file].0;
                let state = existing.get(&key(path, method)).map(|&(_, untouched)| untouched);
                if state.is_some() {
                    ticked.insert(i);
                }
                Row { file: file_name(path), method: method.clone(), locked: state == Some(false) }
            })
            .collect(),
    );
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("nsd.source_search")));
    let first_search = search.clone();
    let ticked = Rc::new(RefCell::new(ticked));
    let chosen = Rc::new(chosen);

    window.open_dialog(cx, move |dialog, _, cx| {
        let filter = search.read(cx).value().to_lowercase();
        let muted = cx.theme().muted_foreground;
        let mut list = v_flex().gap_0p5();
        let mut heading: Option<(&str, String)> = None;
        let mut shown = 0;
        let mut more = 0;
        for (i, row) in rows.iter().enumerate() {
            let m = &row.method;
            let label = format!("{} {} {}", row.file, m.class_name(), m.signature).to_lowercase();
            if !filter.is_empty() && !label.contains(&filter) {
                continue;
            }
            if shown == LISTED {
                more += 1;
                continue;
            }
            shown += 1;
            let class = m.class_name();
            if heading.as_ref() != Some(&(row.file.as_str(), class.clone())) {
                // Functions outside classes (Python, C++) stand under the file's name.
                let text = match m.class.is_empty() {
                    true => row.file.clone(),
                    false => format!("{class}  ·  {}", row.file),
                };
                list = list.child(div().pt_2().text_sm().font_semibold().child(text));
                heading = Some((row.file.as_str(), class));
            }
            let checked = ticked.borrow().contains(&i);
            let toggle = ticked.clone();
            let mut line = div()
                .id(("source-method", i))
                .pl_3()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    Checkbox::new(("source-method-check", i))
                        .checked(checked)
                        .disabled(row.locked)
                        .on_click(move |&on, window, _| {
                            let mut set = toggle.borrow_mut();
                            if on {
                                set.insert(i);
                            } else {
                                set.remove(&i);
                            }
                            window.refresh();
                        }),
                )
                .child(div().text_sm().child(m.signature.clone()))
                .child(div().text_xs().text_color(muted).child(format!("{}", m.line)));
            if row.locked {
                line = line.child(
                    div().text_xs().text_color(muted).child(t!("nsd.source_edited").to_string()),
                );
            }
            list = list.child(line);
        }
        if more > 0 {
            list = list.child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("nsd.source_more", count = more).to_string()),
            );
        }
        let go = {
            let (ticked, chosen) = (ticked.clone(), chosen.clone());
            move |window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                let picked = ticked.borrow().iter().copied().collect();
                chosen(picked, window, cx);
            }
        };
        let go_click = go.clone();
        dialog
            .title(t!("nsd.source_title").to_string())
            .width(px(600.0))
            .child(
                v_flex()
                    .gap_2()
                    .child(Input::new(&search).small())
                    .child(
                        div().text_xs().text_color(muted).child(t!("nsd.source_hint").to_string()),
                    )
                    .child(
                        div().id("source-methods").max_h(px(380.0)).overflow_y_scroll().child(list),
                    ),
            )
            .on_ok(move |_, window, cx| {
                go(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("source-cancel")
                            .label(t!("dialog.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("source-import")
                            .primary()
                            .label(t!("nsd.source_import").to_string())
                            .on_click(move |_, window, cx| go_click(window, cx)),
                    ),
            )
    });
    // Only now: closing gives the focus back to what had it on opening, which must not be
    // the search field.
    first_search.update(cx, |s, cx| s.focus(window, cx));
}

/// A structogram built from a method.
pub struct Built {
    /// Index into [`Found::methods`].
    pub method: usize,
    pub diagram: Diagram,
    pub report: Report,
}

/// Builds the structograms of the methods `picked` (in the background), reading their files
/// again; a file that cannot be read any more gives none.
pub fn build(found: &Found, picked: &[usize], made: &Made) -> Vec<Built> {
    let mut by_file: HashMap<usize, Vec<usize>> = HashMap::new();
    for &i in picked {
        by_file.entry(found.methods[i].0).or_default().push(i);
    }
    let mut built = Vec::new();
    for (file, methods) in by_file {
        let Ok(decoded) = umlcraft_nsd_code::read(&found.files[file].0) else { continue };
        let source = Source::parse(found.language, decoded.text);
        for i in methods {
            if let Some((diagram, report)) = source.diagram(&found.methods[i].1, made) {
                built.push(Built { method: i, diagram, report });
            }
        }
    }
    built.sort_by_key(|b| b.method);
    built
}

/// How a structogram's file header names where it came from.
pub fn made_origin(language: SourceLanguage) -> String {
    format!("{} / {}", super::origin(), language.name())
}

/// Files the structograms of a source import, after `umlcraft_reverse::apply_placed` put the
/// classes where `placed` says: those `built` are created or replace untouched ones; untouched
/// ones of the imported files that were not chosen, or whose method is gone, are deleted.
/// Returns each structogram made or replaced, with the report of its method.
pub fn file_structograms(
    tx: &mut Tx<'_>,
    found: &Found,
    placed: &Placed,
    built: Vec<Built>,
) -> umlcraft_model::Result<Vec<(Id, Report)>> {
    let existing = imported(tx.model());
    let mut kept: HashSet<String> = HashSet::new();
    let mut made = Vec::new();
    for b in built {
        let (file, method) = &found.methods[b.method];
        let (path, unit) = &found.files[*file];
        let key = key(path, method);
        kept.insert(key.clone());
        let root = tx.model().root();
        let operation = unit.and_then(|u| placed.operation(u, method.start..method.end));
        let owner = match operation {
            Some(op) => tx.model().element(op)?.owner().unwrap_or(root),
            None => unit
                .and_then(|u| placed.type_at(u, &method.class).or_else(|| placed.namespace(u)))
                .unwrap_or(root),
        };
        let structogram = umlcraft_nsd::content::normalize(&b.diagram);
        let id = match existing.get(&key) {
            // Edited by hand: not offered, but listed as ticked.
            Some(&(_, false)) => continue,
            Some(&(id, true)) => {
                tx.update(id, |data| {
                    if let ElementKind::Structogram(p) = &mut data.kind {
                        p.structogram = structogram;
                        p.operation = operation;
                    }
                })?;
                if tx.model().element(id)?.owner() != Some(owner) {
                    let index = tx.model().children(owner).len();
                    tx.move_element(id, owner, index)?;
                }
                id
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
        made.push((id, b.report));
    }
    // Untouched structograms of these files that were not built again.
    let files: HashSet<String> = found.files.iter().map(|(p, _)| file_name(p) + ":").collect();
    let stale: Vec<Id> = existing
        .iter()
        .filter(|(key, (_, untouched))| {
            *untouched && !kept.contains(*key) && files.iter().any(|f| key.starts_with(f.as_str()))
        })
        .map(|(_, &(id, _))| id)
        .collect();
    tx.delete_many(&stale)?;
    Ok(made)
}

/// The report in the UI's language, one line per note.
pub fn notes(report: &Report) -> Vec<String> {
    report
        .notes
        .iter()
        .map(|n| {
            let what = match n.kind {
                NoteKind::Continue => t!("nsd.note_continue"),
                NoteKind::Label => t!("nsd.note_label"),
                NoteKind::LocalClass => t!("nsd.note_local_class"),
                NoteKind::Synchronized => t!("nsd.note_synchronized"),
                NoteKind::Fallthrough => t!("nsd.note_fallthrough"),
                NoteKind::SyntaxError => t!("nsd.note_syntax_error"),
                NoteKind::Using => t!("nsd.note_using"),
                NoteKind::Lock => t!("nsd.note_lock"),
                NoteKind::Yield => t!("nsd.note_yield"),
                NoteKind::Goto => t!("nsd.note_goto"),
                NoteKind::LocalFunction => t!("nsd.note_local_function"),
                NoteKind::Conditional => t!("nsd.note_conditional"),
                NoteKind::With => t!("nsd.note_with"),
                NoteKind::LoopElse => t!("nsd.note_loop_else"),
                NoteKind::TryElse => t!("nsd.note_try_else"),
                NoteKind::NestedFunction => t!("nsd.note_nested_function"),
            };
            t!("nsd.note_line", line = n.line, what = what).to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use std::path::{Path, PathBuf};

    use umlcraft_model::{Document, ElementKind, IMPORTED_TAG, Id, Model};
    use umlcraft_nsd::Diagram;
    use umlcraft_nsd_code::{Made, Report, SourceLanguage};

    use super::{METHOD_TAG, build, file_structograms, parse};

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
        let found = parse(SourceLanguage::Java, &[file.to_owned()]);
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
