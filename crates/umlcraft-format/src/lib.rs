//! UmlCraft project files (`.ucraft`): pretty-printed JSON meant to live in Git.
//!
//! Layout:
//! ```json
//! {
//!   "format": "umlcraft",
//!   "formatVersion": 7,
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
use umlcraft_model::{
    ElementData, ElementRecord, Id, Model, ModelError, ModelRecords, Presentation,
};

pub const FORMAT_ID: &str = "umlcraft";
pub const FORMAT_VERSION: u32 = 7;
pub const FILE_EXTENSION: &str = "ucraft";

/// How deep the JSON of a project file may nest. The deepest structogram the model holds
/// (`umlcraft_nsd::read::MAX_DEPTH` levels) takes three JSON levels per level of nesting
/// below six outer ones, a type ([`umlcraft_model::MAX_TYPE_DEPTH`]) three per level below
/// about ten. Deeper files are refused before they are parsed.
pub const MAX_JSON_DEPTH: usize = 1024;
const _: () = assert!(3 * umlcraft_nsd::read::MAX_DEPTH + 16 < MAX_JSON_DEPTH);
const _: () = assert!(3 * umlcraft_model::MAX_TYPE_DEPTH + 16 < MAX_JSON_DEPTH);

/// The stack files are read and written on. Parsing, checking and writing recurse once per
/// level of JSON, and a file at [`MAX_JSON_DEPTH`] needs more than a thread's usual stack
/// (1 MB for the main thread on Windows) in an unoptimized build. Only what a file uses of it
/// is ever committed.
const STACK_SIZE: usize = 64 << 20;

/// Upgrades a document from version `index + 1` to `index + 2`.
type Migration = fn(&mut Value) -> Result<(), FormatError>;
/// Version 2 stores the geometry of sequence diagrams (lifelines, bars, messages and
/// fragments as Astah keeps them). Version 1 sequence diagrams are still valid; the app lays
/// them out once when opening them (`umlcraft_notation::sequence_edit::upgrade_document`).
/// Version 3 keeps the tagged values of languages under UmlCraft's keys. Version 4 adds
/// structograms (elements of type `structogram`); older files have none. Version 5 adds
/// entity-relationship diagrams (`er…` elements); older files have none either. Version 6 adds
/// the IE and IDEF1X notations and foreign key columns; older ER diagrams are Chen's. Version
/// 7 adds `{xor}` constraints between associations (`constraint` elements), domains, keys,
/// indexes, checks and column details of ER diagrams, the diagrams' `simpleStates` and
/// `lookHere`, the styles' `sharedChosen` and letters besides `n` and `m` as the maximum of a
/// cardinality (`"[1,q]"`); older files have none of them. Each addition
/// gets a new version even when older files need no change: an older UmlCraft ignores fields it
/// does not know, and would drop them on saving instead of refusing the newer file.
const MIGRATIONS: &[Migration] =
    &[|_| Ok(()), rename_language_tags, |_| Ok(()), |_| Ok(()), |_| Ok(()), |_| Ok(())];
// One migration per version step.
const _: () = assert!(MIGRATIONS.len() == FORMAT_VERSION as usize - 1);

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("malformed project file: {0}")]
    Json(#[from] serde_json::Error),
    /// Nested deeper than [`MAX_JSON_DEPTH`]; the position is where that depth is reached.
    #[error(
        "malformed project file: nested deeper than {max} levels at line {line} column {column}"
    )]
    TooDeep { line: usize, column: usize, max: usize },
    #[error("not an UmlCraft project file")]
    NotAProject,
    #[error("file format version {found} is newer than the supported version {supported}")]
    TooNew { found: u32, supported: u32 },
    /// For a migration that finds what it cannot upgrade; none does so far.
    #[error("migration from version {from} failed: {reason}")]
    Migration { from: u32, reason: String },
    #[error(transparent)]
    Model(#[from] ModelError),
    /// Saving was refused because the text written would not load again; the file was left
    /// as it was.
    #[error("the project was not saved because it could not be opened again: {0}")]
    NotReadable(Box<FormatError>),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectFile {
    // `format` and `formatVersion` are read as the `Header`.
    root: Id,
    elements: Vec<ElementRecord>,
    #[serde(default)]
    views: BTreeMap<Id, Vec<Presentation>>,
}

/// [`ProjectFile`] as written, borrowing from the model instead of copying it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectOut<'a> {
    format: &'static str,
    format_version: u32,
    root: Id,
    elements: Vec<ElementOut<'a>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    views: BTreeMap<Id, &'a [Presentation]>,
}

/// An [`ElementRecord`] as written.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ElementOut<'a> {
    id: Id,
    #[serde(flatten)]
    data: &'a ElementData,
    #[serde(skip_serializing_if = "<[Id]>::is_empty")]
    children: &'a [Id],
}

/// The first two fields of a project file.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    format: Option<String>,
    format_version: Option<u64>,
}

/// Runs `f` on a thread with a stack of [`STACK_SIZE`].
fn on_large_stack<T: Send>(f: impl FnOnce() -> T + Send) -> io::Result<T> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("umlcraft-format".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, f)?;
        Ok(worker.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

/// Serializes a model in the current format version, with a trailing newline.
pub fn to_string(model: &Model) -> String {
    on_large_stack(|| write_json(model)).unwrap_or_else(|_| write_json(model))
}

/// Like [`to_string`], but reads the text back first and refuses it with
/// [`FormatError::NotReadable`] if it would not load, so that what cannot be opened again
/// never replaces a project file.
pub fn to_string_checked(model: &Model) -> Result<String, FormatError> {
    on_large_stack(|| checked(write_json(model)))?
}

/// `json` if it loads, else why not.
fn checked(json: String) -> Result<String, FormatError> {
    match read_json(&json) {
        Ok(_) => Ok(json),
        Err(err) => Err(FormatError::NotReadable(Box::new(err))),
    }
}

fn write_json(model: &Model) -> String {
    let mut elements: Vec<ElementOut> = model
        .elements()
        .map(|e| ElementOut { id: e.id(), data: e.data(), children: e.children() })
        .collect();
    elements.sort_by_key(|e| e.id);
    let file = ProjectOut {
        format: FORMAT_ID,
        format_version: FORMAT_VERSION,
        root: model.root(),
        elements,
        views: model.views().map(|(id, view)| (id, view.presentations())).collect(),
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
    on_large_stack(|| read_json(json))?
}

/// Parses any JSON text as deep as a project file may be (up to [`MAX_JSON_DEPTH`]), for
/// messages that carry parts of a model: a structogram as deep as a project holds is deeper
/// than serde_json's own limit of 128.
pub fn parse_value(json: &str) -> Result<Value, FormatError> {
    check_depth(json)?;
    on_large_stack(|| {
        let mut de = deserializer(json);
        let value = Value::deserialize(&mut de)?;
        de.end()?;
        Ok(value)
    })?
}

/// A JSON reader for `json` without serde_json's own nesting limit (128), which the model's
/// structograms exceed; [`check_depth`] bounds the depth instead.
fn deserializer(json: &str) -> serde_json::Deserializer<serde_json::de::StrRead<'_>> {
    let mut de = serde_json::Deserializer::from_str(json);
    de.disable_recursion_limit();
    de
}

fn read_json(json: &str) -> Result<(Model, u32), FormatError> {
    // Some editors save UTF-8 with a byte order mark.
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    check_depth(json)?;
    let mut de = deserializer(json);
    let header = match Header::deserialize(&mut de) {
        Ok(header) => header,
        Err(err) if err.is_data() => return Err(FormatError::NotAProject),
        Err(err) => return Err(err.into()),
    };
    if header.format.as_deref() != Some(FORMAT_ID) {
        return Err(FormatError::NotAProject);
    }
    let version = header
        .format_version
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
        .ok_or(FormatError::NotAProject)?;
    if version > FORMAT_VERSION {
        return Err(FormatError::TooNew { found: version, supported: FORMAT_VERSION });
    }
    // A current file is read straight into its records; an older one as a JSON tree first,
    // for the migrations.
    let file = if version == FORMAT_VERSION {
        let mut de = deserializer(json);
        let file = ProjectFile::deserialize(&mut de)?;
        de.end()?;
        file
    } else {
        let mut de = deserializer(json);
        let mut value = Value::deserialize(&mut de)?;
        de.end()?;
        for (index, migrate) in MIGRATIONS.iter().enumerate().skip(version as usize - 1) {
            migrate(&mut value)?;
            value["formatVersion"] = Value::from(index as u32 + 2);
        }
        serde_json::from_value(value)?
    };
    let model = Model::from_records(ModelRecords {
        root: file.root,
        elements: file.elements,
        views: file.views,
    })?;
    Ok((model, version))
}

/// Refuses JSON that nests deeper than [`MAX_JSON_DEPTH`], counting brackets and braces
/// outside strings. Where the text is well-formed up to a point, the count there is the
/// parser's depth, so the parser never goes deeper.
fn check_depth(json: &str) -> Result<(), FormatError> {
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (at, byte) in json.bytes().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    let before = &json[..at];
                    let line = before.matches('\n').count() + 1;
                    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
                    return Err(FormatError::TooDeep { line, column, max: MAX_JSON_DEPTH });
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
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
///
/// The text is read back before it replaces anything ([`to_string_checked`]): what would not
/// open again is refused with [`FormatError::NotReadable`], and the file stays as it was.
pub fn save(model: &Model, path: &Path) -> Result<(), FormatError> {
    let json = to_string_checked(model)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_would_not_load_is_refused_before_saving() {
        let model = Model::new("m");
        assert_eq!(to_string_checked(&model).unwrap(), to_string(&model));
        // A file whose root element is missing, as a defect in the model could write it.
        let root = model.root();
        let broken = format!(
            r#"{{"format":"umlcraft","formatVersion":{FORMAT_VERSION},"root":"{root}","elements":[]}}"#
        );
        let refused = checked(broken);
        assert!(
            matches!(&refused, Err(FormatError::NotReadable(e)) if matches!(**e, FormatError::Model(_))),
            "{refused:?}"
        );
    }

    #[test]
    fn the_depth_check_counts_outside_strings() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(check_depth(&nested(MAX_JSON_DEPTH)).is_ok());
        let deeper = format!("[\"{}\",\n{}]", "[\\\"".repeat(2000), nested(MAX_JSON_DEPTH));
        assert!(check_depth(&deeper).is_err(), "one more around the deepest");
        let quoted = format!("[\"{}\"]", "[{".repeat(2000));
        assert!(check_depth(&quoted).is_ok());
        match check_depth(&format!("\n {}", nested(MAX_JSON_DEPTH + 1))) {
            Err(FormatError::TooDeep { line: 2, column, max: MAX_JSON_DEPTH }) => {
                assert_eq!(column, 2 + MAX_JSON_DEPTH);
            }
            other => panic!("{other:?}"),
        }
    }
}
