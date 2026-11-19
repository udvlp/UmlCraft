//! Reverse engineering: reads declarations from source files (Java, C#, C++, Python) into the
//! model, as
//! Astah's "Import Java" does.
//!
//! Each file is read ([`source::read`]) and parsed with tree-sitter once ([`parse_tree`]); a
//! front-end per language lowers the tree into a language-neutral IR ([`ir`], [`lower`]); one
//! mapping for all languages resolves names and merges the declarations into the model. Parsing
//! needs nothing but the files and may run in the background ([`parse_sources`]); the mapping
//! runs in a single edit of the document ([`apply`]), so one undo takes the whole import back.
//! The structograms of `umlcraft-nsd-code` read method bodies from the same [`SourceTree`].

pub mod associations;
pub mod comment;
pub mod cpp;
pub mod csharp;
pub mod ir;
pub mod java;
pub mod lang;
mod library;
mod merge;
pub mod preprocessor;
pub mod python;
pub mod report;
mod resolve;
pub mod source;
mod tree;

use std::ops::Range;
use std::path::{Path, PathBuf};

use lang::{Frontend, Rules, Skipped, SyntaxError};
use umlcraft_model::{Document, Id, Result, Tx};

pub use report::Report;
pub use tree::{SourceTree, parse_tree};

/// The languages the import reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    Java,
    CSharp,
    Cpp,
    Python,
}

impl SourceLanguage {
    /// Every language the import reads.
    pub const ALL: [SourceLanguage; 4] =
        [SourceLanguage::Java, SourceLanguage::CSharp, SourceLanguage::Cpp, SourceLanguage::Python];

    /// File extensions, without the dot. C and C headers are read as C++.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            SourceLanguage::Java => &["java"],
            SourceLanguage::CSharp => &["cs"],
            SourceLanguage::Cpp => {
                &["h", "hh", "hpp", "hxx", "c", "cpp", "cc", "cxx", "c++", "inl", "ipp", "tpp"]
            }
            SourceLanguage::Python => &["py", "pyi", "pyw"],
        }
    }

    /// The language of a file by its extension (compared ignoring case), if it is one of
    /// [`SourceLanguage::extensions`].
    pub fn of(path: &Path) -> Option<SourceLanguage> {
        let extension = path.extension()?.to_str()?;
        SourceLanguage::ALL
            .into_iter()
            .find(|l| l.extensions().iter().any(|e| e.eq_ignore_ascii_case(extension)))
    }

    /// The language's name as people write it: `Java`, `C#`, `C++`, `Python`.
    pub fn name(self) -> &'static str {
        match self {
            SourceLanguage::Java => "Java",
            SourceLanguage::CSharp => "C#",
            SourceLanguage::Cpp => "C++",
            SourceLanguage::Python => "Python",
        }
    }

    fn frontend(self) -> &'static dyn Frontend {
        match self {
            SourceLanguage::Java => &java::Java,
            SourceLanguage::CSharp => &csharp::CSharp,
            SourceLanguage::Cpp => &cpp::Cpp,
            SourceLanguage::Python => &python::Python,
        }
    }

    fn rules(self) -> &'static Rules {
        match self {
            SourceLanguage::Java => &java::rules::RULES,
            SourceLanguage::CSharp => &csharp::rules::RULES,
            SourceLanguage::Cpp => &cpp::rules::RULES,
            SourceLanguage::Python => &python::rules::RULES,
        }
    }

    /// The module the file at `path` is, for languages whose files are modules; see
    /// [`source::python_module`]. A package's `__init__` file adds `__init__`, so the front-end
    /// knows the package relative imports start from. Empty for Java, C# and C++, whose files
    /// declare their namespaces.
    fn module_path(self, path: &Path) -> Vec<String> {
        match self {
            SourceLanguage::Java | SourceLanguage::CSharp | SourceLanguage::Cpp => Vec::new(),
            SourceLanguage::Python => {
                let mut module = source::python_module(path);
                if path.file_stem().is_some_and(|s| s == "__init__") {
                    module.push("__init__".to_owned());
                }
                module
            }
        }
    }

    /// Tagged values of this language's profile start with it.
    fn tag_prefix(self) -> &'static str {
        match self {
            SourceLanguage::Java => "java.",
            SourceLanguage::CSharp => "csharp.",
            SourceLanguage::Cpp => "cpp.",
            SourceLanguage::Python => "umlcraft.python.",
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

/// Where an import put the declarations of each file (by its index in [`Parsed::units`]): the
/// operations by the bytes their declarations stand on, the types by their names in the file,
/// and the package of what a file declares outside types. The structograms of method bodies
/// are filed by it.
#[derive(Clone, Debug, Default)]
pub struct Placed {
    /// By file once the import is done ([`Placed::by_unit`]).
    pub(crate) operations: Vec<(usize, Range<usize>, Id)>,
    /// With the package each type is in; by file once the import is done.
    types: Vec<(usize, Vec<String>, Id, Id)>,
    free: Vec<(usize, Id)>,
}

/// The entries of `unit` in `entries` sorted by unit.
fn of_unit<T>(entries: &[T], unit: usize, unit_of: impl Fn(&T) -> usize) -> &[T] {
    let start = entries.partition_point(|e| unit_of(e) < unit);
    let end = entries.partition_point(|e| unit_of(e) <= unit);
    &entries[start..end]
}

impl Placed {
    pub(crate) fn free_namespace(&mut self, unit: usize, namespace: Id) {
        if !self.free.iter().any(|&(u, _)| u == unit) {
            self.free.push((unit, namespace));
        }
    }

    /// Sorts the operations and types by file, keeping their order within a file, so that a
    /// lookup only looks at one file's.
    fn by_unit(&mut self) {
        self.operations.sort_by_key(|o| o.0);
        self.types.sort_by_key(|t| t.0);
    }

    /// The operation whose declaration or definition in file `unit` overlaps `range`.
    pub fn operation(&self, unit: usize, range: Range<usize>) -> Option<Id> {
        of_unit(&self.operations, unit, |o| o.0)
            .iter()
            .find(|(_, span, _)| span.start < range.end && range.start < span.end)
            .map(|&(_, _, id)| id)
    }

    /// The type that file `unit` declares as `path`, outermost first (`shop::Cart` counts as
    /// `shop`, `Cart`); where no type has that whole path, one whose path ends with it, else
    /// the longest whose path `path` ends with (`shop::Cart` finds `Cart`: types have no
    /// namespaces in their paths).
    pub fn type_at(&self, unit: usize, path: &[String]) -> Option<Id> {
        let wanted: Vec<&str> = path.iter().flat_map(|p| p.split("::")).collect();
        if wanted.is_empty() {
            return None;
        }
        let types = of_unit(&self.types, unit, |t| t.0);
        let is = |a: &[String], b: &[&str]| a.iter().map(String::as_str).eq(b.iter().copied());
        let exact = || types.iter().find(|t| is(&t.1, &wanted));
        let ends = || {
            types.iter().find(|t| {
                t.1.len() >= wanted.len() && is(&t.1[t.1.len() - wanted.len()..], &wanted)
            })
        };
        let within = || {
            types
                .iter()
                .filter(|t| {
                    !t.1.is_empty()
                        && t.1.len() < wanted.len()
                        && is(&t.1, &wanted[wanted.len() - t.1.len()..])
                })
                .max_by_key(|t| t.1.len())
        };
        exact().or_else(ends).or_else(within).map(|t| t.2)
    }

    /// The package of what file `unit` declares outside types: where its free functions stand,
    /// else the package of its first type.
    pub fn namespace(&self, unit: usize) -> Option<Id> {
        let free = self.free.iter().find(|f| f.0 == unit).map(|f| f.1);
        free.or_else(|| of_unit(&self.types, unit, |t| t.0).first().map(|t| t.3))
    }

    /// Whether the import put a type of its files at `id`, or the declarations of a file
    /// outside types (a package).
    pub fn holds(&self, id: Id) -> bool {
        self.types.iter().any(|t| t.2 == id || t.3 == id) || self.free.iter().any(|f| f.1 == id)
    }
}

/// Merges parsed sources into the model: packages and types are found by name or created,
/// their members matched with the sources, and attributes of class types turned into
/// associations. Run it inside one [`Document::edit`] so the import is one undo step.
pub fn apply(tx: &mut Tx<'_>, language: SourceLanguage, parsed: &Parsed) -> Result<Report> {
    Ok(apply_placed(tx, language, parsed)?.0)
}

/// [`apply`], telling also where the declarations of each file went.
pub fn apply_placed(
    tx: &mut Tx<'_>,
    language: SourceLanguage,
    parsed: &Parsed,
) -> Result<(Report, Placed)> {
    let mut importer = merge::Importer::new(language.rules(), language.tag_prefix());
    let root = tx.model().root();
    for (index, unit) in parsed.units.iter().enumerate() {
        importer.unit = index;
        importer.declare_scope(tx, &unit.scope, root, &[])?;
    }
    importer.merge(tx)?;
    importer.associations(tx)?;
    let mut placed = std::mem::take(&mut importer.placed);
    for entry in &importer.entries {
        let mut path = vec![entry.decl.name.clone()];
        let mut outer = entry.outer;
        while let Some(o) = outer {
            path.insert(0, importer.entries[o].decl.name.clone());
            outer = importer.entries[o].outer;
        }
        placed.types.push((entry.unit, path, entry.id, entry.namespace));
    }
    placed.by_unit();
    let mut report = importer.report;
    report.files = parsed.units.len();
    for (&skip, &n) in &parsed.skipped {
        *report.skipped.entry(skip).or_default() += n;
    }
    report.failed = parsed.failed.clone();
    report.guessed_encodings = parsed.guessed_encodings.clone();
    Ok((report, placed))
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

/// Parses one file's text; `path` also tells the module a Python file is.
pub fn parse_text(
    language: SourceLanguage,
    path: &Path,
    text: &str,
    skipped: &mut Skipped,
) -> Result<ir::Unit, SyntaxError> {
    lower(&parse_tree(language, text), path, skipped)
}

/// The declarations of a parsed file, counting what is left out in `skipped`; `path` also tells
/// the module a Python file is. A file with a syntax error has none.
pub fn lower(
    source: &SourceTree,
    path: &Path,
    skipped: &mut Skipped,
) -> Result<ir::Unit, SyntaxError> {
    let module = source.language().module_path(path);
    let scope = lower_scope(source, &module, skipped)?;
    Ok(ir::Unit { path: path.to_owned(), scope })
}

/// [`lower`] for the file that is `module`.
pub(crate) fn lower_scope(
    source: &SourceTree,
    module: &[String],
    skipped: &mut Skipped,
) -> Result<ir::Scope, SyntaxError> {
    if let Some(error) = source.syntax_error() {
        return Err(error);
    }
    let branches = source.left_out_branches();
    if branches > 0 {
        *skipped.entry(lang::Skip::PreprocessorBranch).or_default() += branches;
    }
    Ok(source.language().frontend().lower(source.root(), source.text(), module, skipped))
}

/// Reads and parses `files`. Files that cannot be read or parsed are reported in
/// [`Parsed::failed`]; the others are imported anyway.
pub fn parse_sources(language: SourceLanguage, files: &[PathBuf]) -> Parsed {
    let mut parsed = Parsed::default();
    for path in files {
        parse_file(language, path, &mut parsed);
    }
    parsed
}

/// Reads and parses one file into `parsed` as [`parse_sources`] does, and gives its tree (for
/// what else reads the file, such as structograms) with the index of its unit in
/// [`Parsed::units`]: none for a file with a syntax error. A file that cannot be read gives
/// nothing.
pub fn parse_file(
    language: SourceLanguage,
    path: &Path,
    parsed: &mut Parsed,
) -> Option<(SourceTree, Option<usize>)> {
    let decoded = match source::read(path) {
        Ok(decoded) => decoded,
        Err(err) => {
            let problem = Problem::Read(err.to_string());
            parsed.failed.push(FileProblem { path: path.to_owned(), problem });
            return None;
        }
    };
    if let Some(encoding) = decoded.guessed {
        parsed.guessed_encodings.push((path.to_owned(), encoding));
    }
    let tree = parse_tree(language, decoded.text);
    // Counted only for files that are imported.
    let mut skipped = Skipped::new();
    let unit = match lower(&tree, path, &mut skipped) {
        Ok(unit) => {
            parsed.units.push(unit);
            for (skip, n) in skipped {
                *parsed.skipped.entry(skip).or_default() += n;
            }
            Some(parsed.units.len() - 1)
        }
        Err(error) => {
            let problem = Problem::Syntax(error);
            parsed.failed.push(FileProblem { path: path.to_owned(), problem });
            None
        }
    };
    Some((tree, unit))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn a_type_is_found_by_a_path_with_namespaces() {
        let (cart, inner, other, package) =
            (Id::from_u128(1), Id::from_u128(2), Id::from_u128(3), Id::from_u128(9));
        let mut placed = Placed::default();
        placed.types.push((1, path(&["Other"]), other, package));
        placed.types.push((0, path(&["Cart"]), cart, package));
        placed.types.push((0, path(&["Cart", "Line"]), inner, package));
        placed.operations.push((1, 0..10, other));
        placed.operations.push((0, 5..8, cart));
        placed.by_unit();
        assert_eq!(placed.type_at(0, &path(&["Cart"])), Some(cart));
        assert_eq!(placed.type_at(0, &path(&["Line"])), Some(inner));
        // A C++ method's class as written: the namespace is no type.
        assert_eq!(placed.type_at(0, &path(&["shop::Cart"])), Some(cart));
        assert_eq!(placed.type_at(0, &path(&["shop::Cart::Line"])), Some(inner));
        assert_eq!(placed.type_at(0, &path(&["shop::Other"])), None);
        assert_eq!(placed.operation(0, 0..6), Some(cart));
        assert_eq!(placed.operation(1, 0..6), Some(other));
        assert_eq!(placed.operation(2, 0..6), None);
        assert!(placed.holds(package) && placed.holds(inner) && !placed.holds(Id::from_u128(5)));
    }
}
