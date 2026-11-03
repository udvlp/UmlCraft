//! UmlCraft project files (`.ucraft`): pretty-printed JSON meant to live in Git.
//!
//! Layout:
//! ```json
//! {
//!   "format": "umlcraft",
//!   "formatVersion": 5,
//!   "root": "<id>",
//!   "elements": [ { "id": "…", "name": "…", "type": "class", …, "children": ["…"] } ],
//!   "views": { "<diagram id>": [ { "id": "…", "element": "…", "shape": "node", … } ] }
//! }
//! ```
//! Elements are sorted by id (UUIDv7, i.e. creation time), so new elements append at the end
//! and diffs stay small. Ownership is stored once, as each element's ordered `children`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use umlcraft_model::{ElementRecord, Id, Model, ModelError, ModelRecords, Presentation};

pub const FORMAT_ID: &str = "umlcraft";
pub const FORMAT_VERSION: u32 = 5;
pub const FILE_EXTENSION: &str = "ucraft";

/// Upgrades a document from version `index + 1` to `index + 2`.
type Migration = fn(&mut Value) -> Result<(), FormatError>;
/// Version 2 stores the geometry of sequence diagrams (lifelines, bars, messages and
/// fragments as Astah keeps them). Version 1 sequence diagrams are still valid; the app lays
/// them out once when opening them (`umlcraft_notation::sequence_edit::upgrade_document`).
/// Version 3 keeps the tagged values of languages under UmlCraft's keys. Version 4 adds
/// structograms (elements of type `structogram`); older files have none. Version 5 adds
/// entity-relationship diagrams (`er…` elements); older files have none either.
const MIGRATIONS: &[Migration] = &[|_| Ok(()), rename_language_tags, |_| Ok(()), |_| Ok(())];
// One migration per version step.
const _: () = assert!(MIGRATIONS.len() == FORMAT_VERSION as usize - 1);

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
    from_str_versioned(json).map(|(model, _)| model)
}

/// Like [`from_str`], also returning the version the file was written in, so the caller can
/// upgrade what the migrations cannot (such as the layout of version 1 sequence diagrams).
pub fn from_str_versioned(json: &str) -> Result<(Model, u32), FormatError> {
    // Some editors save UTF-8 with a byte order mark.
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
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
    Ok((model, version))
}

/// Version 2 files may keep the tagged values of languages under Astah's keys
/// (`jude.java.transient`); UmlCraft's are `java.`, `csharp.` and `cpp.`.
fn rename_language_tags(value: &mut Value) -> Result<(), FormatError> {
    const RENAMES: [(&str, &str); 3] =
        [("jude.java.", "java."), ("jude.c_sharp.", "csharp."), ("jude.c_plus.", "cpp.")];
    let Some(elements) = value.get_mut("elements").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let tags = elements
        .iter_mut()
        .filter_map(|e| e.get_mut("taggedValues").and_then(Value::as_array_mut))
        .flatten();
    for tag in tags {
        let Some(key) = tag.get("key").and_then(Value::as_str) else { continue };
        let renamed = RENAMES
            .iter()
            .find_map(|(old, new)| key.strip_prefix(old).map(|rest| format!("{new}{rest}")));
        if let Some(renamed) = renamed {
            tag["key"] = Value::from(renamed);
        }
    }
    Ok(())
}

/// Writes atomically: the file is written next to the target, flushed to disk and then renamed
/// over it, so neither a crash nor a power loss leaves a half-written project behind. A
/// symbolic link is followed, and the target keeps its permissions.
pub fn save(model: &Model, path: &Path) -> Result<(), FormatError> {
    let json = to_string(model);
    let path = resolve_link(path);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = write_synced(&tmp, json.as_bytes(), &path).and_then(|()| replace(&tmp, &path));
    if result.is_err() {
        fs::remove_file(&tmp).ok();
    }
    result?;
    sync_dir(&path);
    Ok(())
}

/// The file a symbolic link points to, so that saving replaces it rather than the link.
fn resolve_link(path: &Path) -> PathBuf {
    let is_link = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if is_link {
        fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
    } else {
        path.to_owned()
    }
}

fn write_synced(tmp: &Path, bytes: &[u8], target: &Path) -> io::Result<()> {
    let mut file = fs::File::create(tmp)?;
    file.write_all(bytes)?;
    // On Windows permissions are only the read-only flag, which would block the rename.
    #[cfg(unix)]
    if let Ok(metadata) = fs::metadata(target) {
        file.set_permissions(metadata.permissions())?;
    }
    #[cfg(not(unix))]
    let _ = target;
    file.sync_all()
}

/// Renames `tmp` over `target`. On Windows a virus scanner, indexer or sync client may hold
/// the target open for a moment, which makes the rename fail; it is retried for about a second.
fn replace(tmp: &Path, target: &Path) -> io::Result<()> {
    let mut attempts = 0;
    loop {
        match fs::rename(tmp, target) {
            Err(err) if cfg!(windows) && is_sharing_violation(&err) && attempts < 10 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            result => return result,
        }
    }
}

/// `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION` and `ERROR_LOCK_VIOLATION`.
fn is_sharing_violation(err: &io::Error) -> bool {
    matches!(err.raw_os_error(), Some(5 | 32 | 33))
}

/// Makes the rename itself durable. Windows has no directory handles for this, and NTFS
/// journals the rename.
fn sync_dir(path: &Path) {
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::File::open(dir).and_then(|d| d.sync_all()).ok();
    }
    #[cfg(not(unix))]
    let _ = path;
}

pub fn load(path: &Path) -> Result<Model, FormatError> {
    load_versioned(path).map(|(model, _)| model)
}

/// Like [`load`], also returning the version the file was written in.
pub fn load_versioned(path: &Path) -> Result<(Model, u32), FormatError> {
    let json = fs::read_to_string(path)?;
    from_str_versioned(&json)
}
