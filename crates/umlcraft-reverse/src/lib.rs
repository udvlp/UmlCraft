//! Reverse engineering: reads declarations from source files into the model, as Astah's
//! "Import Java" does.
//!
//! A front-end per language parses each file into a language-neutral IR ([`ir`]); one mapping
//! for all languages resolves names and merges the declarations into the model. Parsing needs
//! nothing but the files and may run in the background ([`parse_sources`]); the mapping runs in
//! a single edit of the document ([`apply`]), so one undo takes the whole import back.

pub mod associations;
pub mod ir;
pub mod java;
pub mod lang;
mod merge;
pub mod report;
mod resolve;
pub mod source;

use std::path::{Path, PathBuf};

use lang::{Frontend, Rules, Skipped, SyntaxError};
use umlcraft_model::{Document, Result, Tx};

pub use report::Report;

/// The languages the import reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    Java,
}

impl SourceLanguage {
    /// File extensions, without the dot.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SourceLanguage::Java => &["java"],
        }
    }

    fn frontend(self) -> &'static dyn Frontend {
        match self {
            SourceLanguage::Java => &java::Java,
        }
    }

    fn rules(self) -> &'static Rules {
        match self {
            SourceLanguage::Java => &java::rules::RULES,
        }
    }

    /// Tagged values of this language's profile start with it.
    fn tag_prefix(self) -> &'static str {
        match self {
            SourceLanguage::Java => "jude.java.",
        }
    }
}

/// Why a file was not imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    Read(String),
    Syntax(SyntaxError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileProblem {
    pub path: PathBuf,
    pub problem: Problem,
}

/// The parsed files of one import, ready to be applied to a model.
#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub units: Vec<ir::Unit>,
    pub failed: Vec<FileProblem>,
    pub skipped: Skipped,
    /// Files whose encoding was guessed, with the encoding used.
    pub guessed_encodings: Vec<(PathBuf, &'static str)>,
}

/// The source files of `language` among `paths` (files, or folders searched recursively).
pub fn collect_sources_for(
    language: SourceLanguage,
    paths: &[PathBuf],
) -> std::io::Result<Vec<PathBuf>> {
    source::collect_sources(paths, language.extensions())
}

/// Merges parsed sources into the model: packages and types are found by name or created,
/// their members matched with the sources, and attributes of class types turned into
/// associations. Run it inside one [`Document::edit`] so the import is one undo step.
pub fn apply(tx: &mut Tx<'_>, language: SourceLanguage, parsed: &Parsed) -> Result<Report> {
    let mut importer = merge::Importer::new(language.rules(), language.tag_prefix());
    let root = tx.model().root();
    for unit in &parsed.units {
        importer.declare_scope(tx, &unit.scope, root, &[])?;
    }
    importer.merge(tx)?;
    importer.associations(tx)?;
    let mut report = importer.report;
    report.files = parsed.units.len();
    for (&skip, &n) in &parsed.skipped {
        *report.skipped.entry(skip).or_default() += n;
    }
    report.failed = parsed.failed.clone();
    report.guessed_encodings = parsed.guessed_encodings.clone();
    Ok(report)
}

/// Collects, parses and applies `paths` in one edit of `doc` labelled `label`: what the app
/// does in two steps, for tests and tools.
pub fn import_into(
    doc: &mut Document,
    language: SourceLanguage,
    paths: &[PathBuf],
    label: &str,
) -> std::result::Result<Report, ImportError> {
    let files = collect_sources_for(language, paths)?;
    let parsed = parse_sources(language, &files);
    Ok(doc.edit(label, |tx| apply(tx, language, &parsed))?)
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Model(#[from] umlcraft_model::ModelError),
}

/// Parses one file's text.
pub fn parse_text(
    language: SourceLanguage,
    path: &Path,
    text: &str,
    skipped: &mut Skipped,
) -> Result<ir::Unit, SyntaxError> {
    let scope = language.frontend().parse(text, skipped)?;
    Ok(ir::Unit { path: path.to_owned(), scope })
}

/// Reads and parses `files`. Files that cannot be read or parsed are reported in
/// [`Parsed::failed`]; the others are imported anyway.
pub fn parse_sources(language: SourceLanguage, files: &[PathBuf]) -> Parsed {
    let mut parsed = Parsed::default();
    for path in files {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                let problem = Problem::Read(err.to_string());
                parsed.failed.push(FileProblem { path: path.clone(), problem });
                continue;
            }
        };
        let decoded = source::decode(&bytes);
        if let Some(encoding) = decoded.guessed {
            parsed.guessed_encodings.push((path.clone(), encoding));
        }
        // Counted only for files that are imported.
        let mut skipped = Skipped::new();
        match parse_text(language, path, &decoded.text, &mut skipped) {
            Ok(unit) => {
                parsed.units.push(unit);
                for (skip, n) in skipped {
                    *parsed.skipped.entry(skip).or_default() += n;
                }
            }
            Err(error) => {
                let problem = Problem::Syntax(error);
                parsed.failed.push(FileProblem { path: path.clone(), problem });
            }
        }
    }
    parsed
}
