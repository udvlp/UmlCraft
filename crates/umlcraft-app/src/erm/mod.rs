//! Entity-relationship diagrams (`.ucerm`, crate `umlcraft-erm`): each one opens in a window of
//! its own, outside the project, from the File menu, the command line or "Open with", and is
//! edited there. The window is a frame (`window`) around the canvas (`canvas`, with the mouse
//! and keys in `input` and in-place editing in `text`), the palette (`palette`) and the
//! properties (`props`), so that the canvas can later become a tab of the main window. The
//! editing itself (rules, undo, hit tests, clipboard) is in the crate. `import` makes a
//! diagram from an SQL dump (crate `umlcraft-erm-sql`).

mod canvas;
mod import;
mod input;
mod palette;
mod props;
mod text;
mod window;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::{Diagram, EditError, Id, Issue, IssueKind, ReadError};

pub use palette::Shape;

actions!(
    erm,
    [
        // Asks for a `.ucerm` file and opens it in a window of its own.
        OpenErm,
        // Opens a window with a new, empty diagram.
        NewErm,
        // Asks for an SQL dump and opens a window with the diagram made from it.
        ImportSql,
        // Lays the whole diagram out anew (as the SQL import does).
        Arrange,
        ExportErmSvg,
        // White background, as the app's other PNG images.
        ExportErmPng,
        ExportErmPngTransparent,
        // Shows the 10 px grid, or hides it.
        ToggleGrid,
        // Shows the properties beside the canvas, or hides them.
        ToggleProperties,
        // Edits the text of the selected element in place.
        EditText,
        Duplicate,
        FitToText,
        // Adds an attribute to the selected node.
        AddAttribute,
        // Waits for a click on a second entity and joins both by a new relationship.
        RelationshipTo,
        // Lets the ends of the selected lines float.
        ReleaseEnds,
    ]
);

/// Moves the selection by `(dx, dy)` px (arrow keys).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = erm, no_json)]
pub struct Nudge {
    pub dx: i32,
    pub dy: i32,
}

/// Grows the selected node by `(dw, dh)` px (Ctrl+arrow keys).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = erm, no_json)]
pub struct Grow {
    pub dw: i32,
    pub dh: i32,
}

/// Puts a new shape of the palette into the middle of the view.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = erm, no_json)]
pub struct PlaceShape {
    pub shape: Shape,
}

/// The keys of the ER canvas; they only apply there (Ulf's canvas binds its own in `Canvas`).
pub fn bind_keys(cx: &mut App) {
    let c = Some("ErmCanvas");
    let mut keys = vec![
        crate::key(cx, "delete", crate::DeleteSelection, c),
        crate::key(cx, "backspace", crate::DeleteSelection, c),
        crate::key(cx, "secondary-a", crate::SelectAll, c),
        crate::key(cx, "secondary-c", crate::Copy, c),
        crate::key(cx, "secondary-x", crate::Cut, c),
        crate::key(cx, "secondary-v", crate::Paste, c),
        crate::key(cx, "secondary-d", Duplicate, c),
        crate::key(cx, "escape", crate::Cancel, c),
        crate::key(cx, "f2", EditText, c),
        crate::key(cx, "enter", EditText, c),
        crate::key(cx, "secondary-shift-f", FitToText, c),
    ];
    // Arrows move by 1 px, with Shift by a grid step; with Ctrl they change the size.
    for (name, x, y) in [("left", -1, 0), ("right", 1, 0), ("up", 0, -1), ("down", 0, 1)] {
        keys.push(crate::key(cx, name, Nudge { dx: x, dy: y }, c));
        let step = canvas::GRID as i32;
        keys.push(crate::key(
            cx,
            &format!("shift-{name}"),
            Nudge { dx: x * step, dy: y * step },
            c,
        ));
        keys.push(crate::key(cx, &format!("secondary-{name}"), Grow { dw: x, dh: y }, c));
        keys.push(crate::key(
            cx,
            &format!("secondary-shift-{name}"),
            Grow { dw: x * step, dh: y * step },
            c,
        ));
    }
    cx.bind_keys(keys);
}

/// An open window: the file it shows (canonical path; `None` for a new diagram never saved),
/// its handle and its frame.
struct Open {
    key: Option<PathBuf>,
    handle: AnyWindowHandle,
    view: WeakEntity<window::ErmWindow>,
}

thread_local! {
    /// The windows open, so that opening a file again brings its window to the front and
    /// quitting can ask about unsaved diagrams. Closed windows drop out when looked up.
    static OPEN: RefCell<Vec<Open>> = const { RefCell::new(Vec::new()) };
}

/// The canonical path a file is known by.
fn key_of(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Whether a file is an ER diagram, by its extension.
pub fn is_erm(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(umlcraft_erm::FILE_EXTENSION))
}

/// Asks for a file and opens it; errors are shown over `window`.
pub fn prompt_open(window: &mut Window, cx: &mut App) {
    let rx = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: None,
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Ok(Ok(Some(paths))) = rx.await else { return };
        let Some(path) = paths.into_iter().next() else { return };
        handle.update(cx, |_, window, cx| open(path, window, cx)).ok();
    })
    .detach();
}

/// Asks for an SQL dump and imports it into a new window; errors are shown over `window`.
pub fn prompt_import_sql(window: &mut Window, cx: &mut App) {
    import::prompt(window, cx);
}

/// Opens a window with an imported diagram, unsaved, its notes saying what was found; it is
/// saved next to the dump by default.
fn open_imported(diagram: Diagram, note: import::ImportNote, dump: &Path, cx: &mut App) {
    if let Some((handle, view)) = window::open(None, diagram, cx) {
        let dir = dump.parent().map(Path::to_path_buf);
        view.update(cx, |w, cx| w.imported(note, dir, cx)).ok();
        OPEN.with_borrow_mut(|open| open.push(Open { key: None, handle, view }));
    }
}

/// Opens a window with a new, empty diagram.
pub fn new_diagram(cx: &mut App) {
    let diagram = Diagram::new(t!("erm.untitled_stem").to_string());
    if let Some((handle, view)) = window::open(None, diagram, cx) {
        OPEN.with_borrow_mut(|open| open.push(Open { key: None, handle, view }));
    }
}

/// Opens `path` in a window of its own, or brings the window that shows it to the front. A
/// file that cannot be read is reported over `window`.
pub fn open(path: PathBuf, window: &mut Window, cx: &mut App) {
    let path = std::path::absolute(&path).unwrap_or(path);
    let key = key_of(&path);
    if activate(&key, cx) {
        return;
    }
    match read(&path) {
        Ok((diagram, _)) => {
            if let Some((handle, view)) = window::open(Some(path), diagram, cx) {
                OPEN.with_borrow_mut(|open| open.push(Open { key: Some(key), handle, view }));
            }
        }
        Err(detail) => {
            let rx = window.prompt(
                PromptLevel::Critical,
                &t!("erm.open_failed"),
                Some(&detail),
                &[t!("dialog.ok").as_ref()],
                cx,
            );
            // Dropping the answer would cancel the dialog before it appears.
            cx.spawn(async move |_| rx.await.ok()).detach();
        }
    }
}

/// Forgets windows that are closed.
fn prune(cx: &mut App) {
    let handles: Vec<AnyWindowHandle> = OPEN.with_borrow(|o| o.iter().map(|w| w.handle).collect());
    let closed: Vec<AnyWindowHandle> =
        handles.into_iter().filter(|h| h.update(cx, |_, _, _| ()).is_err()).collect();
    OPEN.with_borrow_mut(|open| open.retain(|w| !closed.contains(&w.handle)));
}

/// Brings the window showing `key` to the front; whether there is one.
fn activate(key: &Path, cx: &mut App) -> bool {
    prune(cx);
    let windows: Vec<_> = OPEN.with_borrow(|open| {
        open.iter().filter(|w| w.key.as_deref() == Some(key)).map(|w| w.handle).collect()
    });
    windows
        .into_iter()
        .any(|handle| handle.update(cx, |_, window, _| window.activate_window()).is_ok())
}

/// After "Save as": the window now shows `path`.
fn rekey(handle: AnyWindowHandle, path: &Path) {
    let key = key_of(path);
    OPEN.with_borrow_mut(|open| {
        for w in open.iter_mut().filter(|w| w.handle == handle) {
            w.key = Some(key.clone());
        }
    });
}

/// The names of the diagrams with unsaved changes, for asking before the app quits.
pub fn unsaved(cx: &mut App) -> Vec<String> {
    prune(cx);
    let views: Vec<_> = OPEN.with_borrow(|open| open.iter().map(|w| w.view.clone()).collect());
    views
        .into_iter()
        .filter_map(|v| v.upgrade())
        .filter(|v| v.read(cx).unsaved(cx))
        .map(|v| v.read(cx).title())
        .collect()
}

/// Why an edit was refused, in the UI's language.
pub fn edit_error(err: &EditError) -> String {
    match err {
        EditError::Missing(_) => t!("erm.edit_missing").to_string(),
        EditError::NotAllowed(a, b) => {
            t!("erm.issue.not_allowed", source = kind_name(a), target = kind_name(b)).to_string()
        }
        EditError::Rule(kind) => issue_what(kind),
        EditError::NotApplicable => t!("erm.edit_not_applicable").to_string(),
    }
}

/// Reads a file; the error is the reason in the UI's language.
pub fn read(path: &Path) -> Result<(Diagram, Vec<Issue>), String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| t!("erm.not_utf8").to_string())?;
    umlcraft_erm::read(&text).map_err(|e| read_error(&e))
}

/// Why a file could not be read, in the UI's language. serde's own messages (with line and
/// column) and the reasons of field errors stay as they are.
pub fn read_error(err: &ReadError) -> String {
    match err {
        ReadError::Json(e) => t!("erm.error_json", detail = e.to_string()).to_string(),
        ReadError::NotAnErmFile => t!("erm.error_not_erm").to_string(),
        ReadError::TooNew { found, supported } => {
            t!("erm.error_too_new", found = found, supported = supported).to_string()
        }
        ReadError::Migration { from, reason } => {
            t!("erm.error_migration", from = from, reason = reason).to_string()
        }
        ReadError::Field { element, reason } => {
            t!("erm.error_field", element = element, reason = reason).to_string()
        }
        ReadError::Invalid(issues) => {
            let mut text = t!("erm.error_invalid").to_string();
            for issue in issues {
                text.push('\n');
                text.push_str(&issue_text(None, issue));
            }
            text
        }
    }
}

/// A broken rule as a line for the user: its number, the element and what is wrong.
/// `diagram` names the element when it is at hand; a rejected file names it by its id.
pub fn issue_text(diagram: Option<&Diagram>, issue: &Issue) -> String {
    let element = match diagram {
        Some(d) => element_label(d, &issue.element),
        None => issue.element.to_string(),
    };
    let what = issue_what(&issue.kind);
    t!("erm.issue_line", code = issue.kind.code(), element = element, what = what).to_string()
}

/// What a rule says, in the UI's language.
fn issue_what(kind: &IssueKind) -> String {
    let key = format!("erm.issue.{}", issue_key(kind));
    match kind {
        IssueKind::MissingEnd { end } => t!(key.as_str(), end = end.to_string()),
        IssueKind::NotAllowed { source, target } => {
            t!(key.as_str(), source = kind_name(source), target = kind_name(target))
        }
        _ => t!(key.as_str()),
    }
    .to_string()
}

/// The text of a rule in the locales, under `erm.issue`.
fn issue_key(kind: &IssueKind) -> &'static str {
    match kind {
        IssueKind::DuplicateId => "duplicate_id",
        IssueKind::MissingEnd { .. } => "missing_end",
        IssueKind::NotAllowed { .. } => "not_allowed",
        IssueKind::NoOwner => "no_owner",
        IssueKind::SeveralOwners => "several_owners",
        IssueKind::OwnPart => "own_part",
        IssueKind::TooFewParticipants => "too_few_participants",
        IssueKind::IdentifyingWithoutWeak => "identifying_without_weak",
        IssueKind::IdentifyingWithSeveralWeak => "identifying_with_several_weak",
        IssueKind::WeakWithoutIdentifying => "weak_without_identifying",
        IssueKind::IsAEnds => "is_a_ends",
        IssueKind::SuperTypeOutsideIsA => "super_type_outside_is_a",
        IssueKind::CardinalityOutsideParticipation => "cardinality_outside_participation",
        IssueKind::BadGeometry => "bad_geometry",
        IssueKind::BadCardinality => "bad_cardinality",
    }
}

/// A node kind as files name it (`entity`, `isA`, …), in the UI's language.
fn kind_name(type_name: &str) -> String {
    let key = format!("erm.kind.{type_name}");
    let name = t!(key.as_str()).to_string();
    if name == key { type_name.to_owned() } else { name }
}

/// An element as the user knows it: a node by its name, a line by the nodes it joins.
fn element_label(d: &Diagram, id: &Id) -> String {
    let node = |id: &Id| {
        d.node(id).map(|n| match n.name.trim() {
            "" => t!("erm.unnamed", kind = kind_name(n.kind.type_name())).to_string(),
            name => t!("error.named", name = name.replace('\n', " ")).to_string(),
        })
    };
    if let Some(name) = node(id) {
        return name;
    }
    if let Some(link) = d.link(id) {
        let end = |id: &Id| node(id).unwrap_or_else(|| id.to_string());
        return t!("erm.line_between", a = end(&link.source), b = end(&link.target)).to_string();
    }
    if d.texts.iter().any(|text| &text.id == id) {
        return t!("erm.free_text").to_string();
    }
    id.to_string()
}

/// The file name an export or a new file suggests: the diagram's name, else the file's.
fn file_stem(diagram: &Diagram, path: Option<&Path>) -> String {
    let name = diagram.meta.name.trim();
    let name = if name.is_empty() {
        path.and_then(Path::file_stem).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    } else {
        name.to_owned()
    };
    if name.trim().is_empty() {
        return t!("erm.untitled_stem").to_string();
    }
    crate::main_view::sanitize_file_name(&name)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui_kit brings a `test` attribute of its own.
    use std::path::{Path, PathBuf};

    use umlcraft_erm::{Diagram, Id, Issue, IssueKind, NodeKind, ReadError};

    use super::{file_stem, is_erm, issue_key, issue_text, issue_what, read, read_error};

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../umlcraft-erm/tests/fixtures").join(name)
    }

    /// Every rule, with data where it carries some.
    fn every_kind() -> Vec<IssueKind> {
        vec![
            IssueKind::DuplicateId,
            IssueKind::MissingEnd { end: Id::from("n9") },
            IssueKind::NotAllowed { source: "attribute", target: "isA" },
            IssueKind::NoOwner,
            IssueKind::SeveralOwners,
            IssueKind::OwnPart,
            IssueKind::TooFewParticipants,
            IssueKind::IdentifyingWithoutWeak,
            IssueKind::IdentifyingWithSeveralWeak,
            IssueKind::WeakWithoutIdentifying,
            IssueKind::IsAEnds,
            IssueKind::SuperTypeOutsideIsA,
            IssueKind::CardinalityOutsideParticipation,
            IssueKind::BadGeometry,
            IssueKind::BadCardinality,
        ]
    }

    #[test]
    fn every_text_of_the_window_exists_in_both_languages() {
        let sources = [
            include_str!("mod.rs"),
            include_str!("canvas.rs"),
            include_str!("import.rs"),
            include_str!("input.rs"),
            include_str!("palette.rs"),
            include_str!("props.rs"),
            include_str!("text.rs"),
            include_str!("window.rs"),
        ];
        let mut keys: Vec<String> = Vec::new();
        for source in sources {
            for part in source.split("t!(\"").skip(1) {
                let key = part.split('"').next().unwrap();
                if !key.contains('{') && key.contains('.') {
                    keys.push(key.to_owned());
                }
            }
        }
        for shape in super::Shape::ALL {
            keys.push(format!("erm.shape.{}", shape.key()));
        }
        keys.push("menu.import_sql".to_owned());
        assert!(keys.len() > 80, "{} keys found", keys.len());
        for locale in ["de", "en"] {
            for key in &keys {
                let text = rust_i18n::t!(key.as_str(), locale = locale).to_string();
                assert!(!text.ends_with(key.as_str()), "{locale}: {key} is missing");
            }
        }
    }

    #[test]
    fn er_diagrams_go_by_their_extension() {
        assert!(is_erm(Path::new("flug.ucerm")));
        assert!(is_erm(Path::new(r"C:\Modelle\FLUG.UCERM")));
        assert!(!is_erm(Path::new("shop.ucraft")));
        assert!(!is_erm(Path::new("sort.nsd")));
        assert!(!is_erm(Path::new("ucerm")));
    }

    #[test]
    fn every_rule_and_kind_has_a_text_in_both_languages() {
        // By locale, not through the UI's language: tests run side by side.
        for locale in ["de", "en"] {
            for kind in every_kind() {
                let key = format!("erm.issue.{}", issue_key(&kind));
                let text = rust_i18n::t!(key.as_str(), locale = locale).to_string();
                assert!(!text.contains("erm."), "{locale}: {kind:?} has no text ({text})");
            }
            for node in [
                NodeKind::entity(),
                NodeKind::AssociativeEntity,
                NodeKind::attribute(),
                NodeKind::relationship(),
                NodeKind::is_a(),
            ] {
                let key = format!("erm.kind.{}", node.type_name());
                let text = rust_i18n::t!(key.as_str(), locale = locale).to_string();
                assert!(!text.contains("erm."), "{locale}: {node:?} has no name ({text})");
            }
        }
        // The rules with data fill it in.
        let text = issue_what(&IssueKind::MissingEnd { end: Id::from("n9") });
        assert!(text.contains("n9"), "{text}");
        let text = issue_what(&IssueKind::NotAllowed { source: "attribute", target: "isA" });
        assert!(!text.contains("%{"), "{text}");
    }

    #[test]
    fn issues_name_the_element() {
        let (d, _) = read(&fixture("flug.ucerm")).unwrap();
        let entity = d.nodes.iter().find(|n| n.kind.is_entity_like()).unwrap();
        let issue = Issue { element: entity.id.clone(), kind: IssueKind::BadGeometry };
        let text = issue_text(Some(&d), &issue);
        assert!(text.starts_with("P11"), "{text}");
        let named = rust_i18n::t!("error.named", name = entity.name.replace('\n', " "));
        assert!(text.contains(named.as_ref()), "{text}");
        let link = &d.links[0];
        let issue = Issue { element: link.id.clone(), kind: IssueKind::BadCardinality };
        assert!(!issue_text(Some(&d), &issue).contains(link.id.as_str()));
        // Without the diagram (a rejected file), by the id.
        assert!(issue_text(None, &issue).contains(link.id.as_str()));
    }

    #[test]
    fn read_errors_are_told_for_the_user() {
        let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let text = read_error(&ReadError::Json(json));
        assert!(text.contains("line 1"), "serde's place stays: {text}");
        // Not the crate's developer text.
        let text = read_error(&ReadError::NotAnErmFile);
        assert_ne!(text, ReadError::NotAnErmFile.to_string());
        let text = read_error(&ReadError::TooNew { found: 7, supported: 1 });
        assert!(text.contains('7') && text.contains('1'), "{text}");
        let issues = vec![
            Issue { element: Id::from("a"), kind: IssueKind::DuplicateId },
            Issue { element: Id::from("b"), kind: IssueKind::NoOwner },
        ];
        let text = read_error(&ReadError::Invalid(issues));
        assert_eq!(text.lines().count(), 3, "{text}");
        assert!(text.contains("P1") && text.contains("P4"), "{text}");
    }

    #[test]
    fn broken_files_are_reported_and_good_ones_open() {
        assert!(read(&fixture("flug.ucerm")).is_ok());
        assert!(read(&fixture("chen-extra.ucerm")).is_ok());
        let invalid = fixture("invalid");
        let mut rejected = 0;
        for entry in std::fs::read_dir(&invalid).unwrap() {
            let path = entry.unwrap().path();
            if is_erm(&path) {
                let err = read(&path).unwrap_err();
                assert!(!err.is_empty());
                rejected += 1;
            }
        }
        assert!(rejected >= 10, "{rejected} invalid fixtures");
        assert!(read(Path::new("/nonexistent/x.ucerm")).is_err());
        // Unfinished diagrams open, with their warnings.
        let unfinished = fixture("warnings");
        for entry in std::fs::read_dir(&unfinished).unwrap() {
            let path = entry.unwrap().path();
            let (_, issues) = read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!issues.is_empty());
        }
    }

    #[test]
    fn exports_are_named_after_the_diagram_else_the_file() {
        let mut d = Diagram::new("Flug buchen");
        assert_eq!(file_stem(&d, Some(Path::new("/a/flug.ucerm"))), "Flug buchen");
        d.meta.name = "Kunde/Ticket".into();
        assert_eq!(file_stem(&d, Some(Path::new("/a/flug.ucerm"))), "Kunde_Ticket");
        d.meta.name = "  ".into();
        assert_eq!(file_stem(&d, Some(Path::new("/a/flug.ucerm"))), "flug");
    }
}
