//! What the tools work on ([`Host`]): the project open in the app, or a [`Headless`] one; and
//! which folders they may read and write files in ([`Access`]).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use umlcraft_model::{DiagramKind, Document, Id, Model};
use umlcraft_notation::edit;
use umlcraft_notation::scene::{ApproxMeasure, TextMeasure};

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
    /// The app's sample project; only the app has it.
    fn sample(&self) -> Result<Document, String> {
        Err("The sample project comes with the UmlCraft app, which is not running.".into())
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
/// existing part is canonicalized (links resolved), the rest appended.
pub(crate) fn absolute(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut existing = path.as_path();
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
            _ => return path,
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
/// saved to files the [`Access`] allows. Texts are measured by estimate.
pub struct Headless {
    doc: Document,
    path: Option<PathBuf>,
    active: Option<Id>,
    measure: ApproxMeasure,
}

impl Headless {
    /// An empty project with one class diagram.
    pub fn new() -> Self {
        Self::with(new_document("Project"), None)
    }

    pub fn with(doc: Document, path: Option<PathBuf>) -> Self {
        let active = first_diagram(doc.model());
        Self { doc, path, active, measure: ApproxMeasure }
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
        *self = Self::with(doc, path);
        Ok(())
    }

    fn saved(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    // Nobody sees a selection here.
    fn select(&mut self, _presentations: Vec<Id>) {}
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
        // Leaving the folder with `..` does not count as inside it.
        assert!(access.check(&dir.join("..").join("b.ucraft"), None).is_err());
        let own = std::env::temp_dir().join("own.ucraft");
        assert!(access.check(&own, Some(&own)).is_ok());
        assert!(Access::default().check(&own, None).is_err());
        assert!(Access { folders: Vec::new(), any: true }.check(&own, None).is_ok());
    }
}
