//! UmlCraft project files (`.ucraft`): pretty-printed JSON meant to live in Git.
//!
//! Layout:
//! ```json
//! {
//!   "format": "umlcraft",
//!   "formatVersion": 2,
//!   "root": "<id>",
//!   "elements": [ { "id": "…", "name": "…", "type": "class", …, "children": ["…"] } ],
//!   "views": { "<diagram id>": [ { "id": "…", "element": "…", "shape": "node", … } ] }
//! }
//! ```
//! Elements are sorted by id (UUIDv7, i.e. creation time), so new elements append at the end
//! and diffs stay small. Ownership is stored once, as each element's ordered `children`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use umlcraft_model::{ElementRecord, Id, Model, ModelError, ModelRecords, Presentation};

pub const FORMAT_ID: &str = "umlcraft";
pub const FORMAT_VERSION: u32 = 2;
pub const FILE_EXTENSION: &str = "ucraft";

/// Upgrades a document from version `index + 1` to `index + 2`.
type Migration = fn(&mut Value) -> Result<(), FormatError>;
/// Version 2 stores the geometry of sequence diagrams (lifelines, bars, messages and
/// fragments as Astah keeps them). Version 1 sequence diagrams are still valid; the app lays
/// them out once when opening them (`umlcraft_notation::sequence_edit::upgrade_document`).
const MIGRATIONS: &[Migration] = &[|_| Ok(())];

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("malformed project file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not an UmlCraft project file")]
    NotAProject,
    #[error("file format version {found} is newer than the supported version {supported}")]
    TooNew { found: u32, supported: u32 },
    #[error("migration from version {from} failed: {reason}")]
    Migration { from: u32, reason: String },
    #[error(transparent)]
    Model(#[from] ModelError),
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFile {
    format: String,
    format_version: u32,
    root: Id,
    elements: Vec<ElementRecord>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    views: BTreeMap<Id, Vec<Presentation>>,
}

/// Serializes a model in the current format version, with a trailing newline.
pub fn to_string(model: &Model) -> String {
    let ModelRecords { root, elements, views } = model.to_records();
    let file = ProjectFile {
        format: FORMAT_ID.to_owned(),
        format_version: FORMAT_VERSION,
        root,
        elements,
        views,
    };
    let mut json = serde_json::to_string_pretty(&file).expect("model serialization cannot fail");
    json.push('\n');
    json
}

/// Parses a project file of any supported version and validates the model.
pub fn from_str(json: &str) -> Result<Model, FormatError> {
    let mut value: Value = serde_json::from_str(json)?;
    if value.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(FormatError::NotAProject);
    }
    let version = value
        .get("formatVersion")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
        .ok_or(FormatError::NotAProject)?;
    if version > FORMAT_VERSION {
        return Err(FormatError::TooNew { found: version, supported: FORMAT_VERSION });
    }
    for (index, migrate) in MIGRATIONS.iter().enumerate().skip(version as usize - 1) {
        migrate(&mut value)?;
        value["formatVersion"] = Value::from(index as u32 + 2);
    }
    let file: ProjectFile = serde_json::from_value(value)?;
    let model = Model::from_records(ModelRecords {
        root: file.root,
        elements: file.elements,
        views: file.views,
    })?;
    Ok(model)
}

/// Writes atomically: the file is written next to the target and then renamed over it, so a
/// crash never leaves a half-written project behind.
pub fn save(model: &Model, path: &Path) -> Result<(), FormatError> {
    let json = to_string(model);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn load(path: &Path) -> Result<Model, FormatError> {
    let json = fs::read_to_string(path)?;
    from_str(&json)
}
