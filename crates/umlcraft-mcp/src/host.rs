//! What the tools work on ([`Host`]): the project open in the app, or a [`Headless`] one; and
//! which folders they may read and write files in ([`Access`]).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use umlcraft_model::{DiagramKind, Document, Id, Model};
use umlcraft_notation::arial::ArialMeasure;
use umlcraft_notation::edit;
use umlcraft_notation::scene::{Display, Displayed, TextMeasure};

/// The project the tools read and edit, and what the user is looking at.
pub trait Host {
    fn document(&self) -> &Document;
    /// The document to edit, with the measure that sizes its boxes.
    fn parts(&mut self) -> (&mut Document, &dyn TextMeasure);
    /// The undo label of a change made by `tool`.
    fn label(&self, tool: &str) -> String;
    /// The file the project was opened from or last saved to.
    fn path(&self) -> Option<&Path>;
    /// Whether this is the project open in the app, which the user sees change.
    fn live(&self) -> bool;
    /// The diagram shown, or last worked on.
    fn active_diagram(&self) -> Option<Id>;
    /// Selected presentations of the active diagram.
    fn selection(&self) -> Vec<Id>;
    /// Shows `diagram` (in the app: brings its tab to the front).
    fn show_diagram(&mut self, diagram: Id);
    /// Replaces the project, e.g. with one opened from a file; `Err` with the reason when it
    /// may not (the app keeps a project with unsaved changes).
    fn replace(&mut self, doc: Document, path: Option<PathBuf>) -> Result<(), String>;
    /// Notes that the project was saved to `path`.
    fn saved(&mut self, path: PathBuf);
    /// Selects `presentations` of the active diagram (none: clears the selection).
    fn select(&mut self, presentations: Vec<Id>);
    /// Selects in `structogram`, which is shown: an element, the structogram as a whole or
    /// nothing. Only the app's editor has a selection in a structogram.
    fn select_in_structogram(&mut self, _structogram: Id, _selection: umlcraft_nsd::Selection) {}
    /// The app's sample project; only the app has it.
    fn sample(&self) -> Result<Document, String> {
        Err("The sample project comes with the UmlCraft app, which is not running.".into())
    }
    /// How diagrams are drawn beyond what they store.
    fn display(&mut self) -> Display {
        Display::of(self.parts().1)
    }
    /// Draws diagrams as `display` says from now on: in the app, as if the user changed its
    /// settings.
    fn set_display(&mut self, display: Display);
}

/// The app's settings file, `UmlCraft/settings.json` in the platform's folder for such
/// settings: `%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
/// `$XDG_CONFIG_HOME` (else `~/.config`) elsewhere.
pub fn settings_file() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Application Support"))
    } else {
        var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join("UmlCraft").join("settings.json"))
}

/// How the app draws diagrams, from its settings file, so that a headless session draws them
/// the same; all off without the file.
pub fn app_display() -> Display {
    let settings: Value = settings_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let on = |key: &str| settings.get(key).and_then(Value::as_bool).unwrap_or(false);
    Display {
        visibility_column: on("visibilityColumn"),
        abstract_stereotype: on("abstractStereotype"),
        read_only_constraint: on("readOnlyConstraint"),
    }
}

/// Folders in which tools may open, save and export files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    pub folders: Vec<PathBuf>,
    /// Any folder (`--allow-any-path`).
    pub any: bool,
}

impl Access {
    /// `path` made absolute, if it lies in an allowed folder or is `own` (the project's file).
    pub fn check(&self, path: &Path, own: Option<&Path>) -> Result<PathBuf, String> {
        let absolute = absolute(path);
        if self.any || own.is_some_and(|own| absolute == self::absolute(own)) {
            return Ok(absolute);
        }
        if self.folders.iter().any(|folder| absolute.starts_with(self::absolute(folder))) {
            return Ok(absolute);
        }
        let folders: Vec<String> = self.folders.iter().map(|f| f.display().to_string()).collect();
        Err(if folders.is_empty() {
            format!(
                "{} is not in a folder UmlCraft may use. No folder is allowed; the user can \
                 add one with `--allow-path` in the MCP server's command (`umlcraft-cli mcp \
                 setup --allow-path <folder>`).",
                absolute.display()
            )
        } else {
            format!(
                "{} is not in a folder UmlCraft may use: {}.",
                absolute.display(),
                folders.join(", ")
            )
        })
    }

    pub fn to_json(&self) -> Value {
        json!({ "folders": self.folders, "any": self.any })
    }

    pub fn from_json(value: &Value) -> Self {
        let folders = value["folders"].as_array().map_or_else(Vec::new, |folders| {
            folders.iter().filter_map(Value::as_str).map(PathBuf::from).collect()
        });
        Self { folders, any: value["any"].as_bool().unwrap_or(false) }
    }
}

/// `path` absolute and without `.` and `..`, also for files that do not exist yet: the
/// existing part is canonicalized (links resolved), the rest appended. On Unix
/// `std::path::absolute` keeps `..`, and one after a folder that does not exist yet survives
/// canonicalizing; such a path is resolved again with its `..` taken back, so that
/// `allowed/new/../../x` counts as the `x` beside `allowed` that writing it reaches.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let resolved = resolve(&path);
    let dotted = resolved
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir));
    if !dotted {
        return resolved;
    }
    // The `..` left lie after the existing part, which is canonical: going up from it is going
    // up on the disk, so taking them back leaves none, and the rest only needs resolving again.
    resolve(&lexical(&resolved))
}

/// `path` with `.` dropped and each `..` taking back the component before it.
fn lexical(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The existing part of `path` canonicalized, the rest appended as it is.
fn resolve(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = std::fs::canonicalize(existing) {
            let mut out = strip_verbatim(canonical);
            out.extend(rest.iter().rev());
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Windows' canonical paths start with `\\?\`; paths given by people do not.
fn strip_verbatim(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if !rest.starts_with("UNC") => PathBuf::from(rest),
        _ => path,
    }
}

/// A project of the MCP server's own, when the app is not running: in memory, opened from and
/// saved to files the [`Access`] allows. Texts are measured in Arial, the diagram font; the
/// diagrams are drawn as in Astah unless [`Host::set_display`] says otherwise.
pub struct Headless {
    doc: Document,
    path: Option<PathBuf>,
    active: Option<Id>,
    measure: Displayed<ArialMeasure>,
}

impl Headless {
    /// An empty project with one class diagram.
    pub fn new() -> Self {
        Self::with(new_document("Project"), None)
    }

    pub fn with(doc: Document, path: Option<PathBuf>) -> Self {
        let active = first_diagram(doc.model());
        let measure = Displayed { measure: ArialMeasure, display: Display::default() };
        Self { doc, path, active, measure }
    }

    /// The project drawn as `display` says.
    pub fn drawn_as(mut self, display: Display) -> Self {
        self.measure.display = display;
        self
    }

    /// Whether anything happened to the project yet: an edit, or a file opened.
    pub fn touched(&self) -> bool {
        self.doc.revision() > 0 || self.path.is_some()
    }
}

impl Default for Headless {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for Headless {
    fn document(&self) -> &Document {
        &self.doc
    }

    fn parts(&mut self) -> (&mut Document, &dyn TextMeasure) {
        (&mut self.doc, &self.measure)
    }

    fn label(&self, tool: &str) -> String {
        tool.to_owned()
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn live(&self) -> bool {
        false
    }

    fn active_diagram(&self) -> Option<Id> {
        self.active.filter(|&d| self.doc.model().view(d).is_some())
    }

    fn selection(&self) -> Vec<Id> {
        Vec::new()
    }

    fn show_diagram(&mut self, diagram: Id) {
        self.active = Some(diagram);
    }

    fn replace(&mut self, doc: Document, path: Option<PathBuf>) -> Result<(), String> {
        *self = Self::with(doc, path).drawn_as(self.measure.display);
        Ok(())
    }

    fn saved(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    // Nobody sees a selection here.
    fn select(&mut self, _presentations: Vec<Id>) {}

    fn set_display(&mut self, display: Display) {
        self.measure.display = display;
    }
}

/// Opens the UmlCraft project at `path`.
pub fn open(path: &Path) -> Result<Document, String> {
    let (model, version) =
        umlcraft_format::load_versioned(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut doc = Document::new(model);
    // Version 1 stored no geometry for sequence diagrams.
    if version < 2 {
        umlcraft_notation::sequence_edit::upgrade_document(&mut doc);
    }
    Ok(doc)
}

/// A new project named `name` with one class diagram and no history.
pub fn new_document(name: &str) -> Document {
    let mut doc = Document::new(Model::new(name));
    let root = doc.model().root();
    doc.edit("", |tx| edit::create_diagram(tx, root, DiagramKind::Class, "Class diagram"))
        .expect("a model can always own a class diagram");
    doc.clear_history();
    doc
}

/// The first diagram of the model, in tree order.
pub fn first_diagram(model: &Model) -> Option<Id> {
    let mut stack = vec![model.root()];
    while let Some(id) = stack.pop() {
        if model.view(id).is_some() {
            return Some(id);
        }
        stack.extend(model.children(id).iter().rev());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_allows_files_in_its_folders_and_the_projects_own() {
        let dir = std::env::temp_dir().join("umlcraft-mcp-access");
        std::fs::create_dir_all(&dir).unwrap();
        let access = Access { folders: vec![dir.clone()], any: false };
        assert!(access.check(&dir.join("new").join("a.ucraft"), None).is_ok());
        // Leaving the folder with `..` does not count as inside it, also after a folder that
        // does not exist yet; going back into it does.
        assert!(access.check(&dir.join("..").join("b.ucraft"), None).is_err());
        let up = dir.join("missing").join("..").join("..").join("b.ucraft");
        assert!(access.check(&up, None).is_err(), "{:?}", access.check(&up, None));
        let back = dir.join("missing").join("..").join("c.ucraft");
        assert_eq!(access.check(&back, None), Ok(absolute(&dir).join("c.ucraft")));
        let own = std::env::temp_dir().join("own.ucraft");
        assert!(access.check(&own, Some(&own)).is_ok());
        assert!(Access::default().check(&own, None).is_err());
        assert!(Access { folders: Vec::new(), any: true }.check(&own, None).is_ok());
    }
}
