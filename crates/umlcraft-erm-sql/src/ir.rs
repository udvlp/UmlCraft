//! What the parser reads from a dump: tables with their columns and constraints, neutral to
//! the dialect.

use std::collections::BTreeMap;
use std::fmt;

use indexmap::IndexMap;

use crate::dialect::Dialect;

/// A table's schema and name. MySQL has no schemas here (`""`); PostgreSQL's names without
/// a schema are in `public`. Unquoted PostgreSQL names are lower case, as the server keeps
/// them.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct TableKey {
    pub schema: String,
    pub name: String,
}

impl TableKey {
    pub fn new(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self { schema: schema.into(), name: name.into() }
    }

    /// Whether the schema is the one names go without (`public`, or none).
    pub fn in_default_schema(&self) -> bool {
        self.schema.is_empty() || self.schema == "public"
    }
}

impl fmt::Display for TableKey {
    /// The name as the diagram shows it: with the schema only outside the default one.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.in_default_schema() {
            f.write_str(&self.name)
        } else {
            write!(f, "{}.{}", self.schema, self.name)
        }
    }
}

/// Everything the import needs from a dump.
#[derive(Clone, PartialEq, Debug)]
pub struct Schema {
    pub dialect: Dialect,
    /// In the order of the dump.
    pub tables: IndexMap<TableKey, Table>,
    /// The database's name, if the dump names it (header, `USE`, `CREATE DATABASE`).
    pub database: Option<String>,
    /// Statements that were not needed, counted by their first words (`INSERT INTO`).
    pub skipped: BTreeMap<String, usize>,
    /// Statements or parts of them that could not be read; each costs only itself.
    pub problems: Vec<Problem>,
    /// Lines of statements that were not UTF-8 and were read as Windows-1252.
    pub recoded: Vec<usize>,
    /// Partitions folded into their parent table (they are not drawn).
    pub partitions: usize,
}

impl Schema {
    pub fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            tables: IndexMap::new(),
            database: None,
            skipped: BTreeMap::new(),
            problems: Vec::new(),
            recoded: Vec::new(),
            partitions: 0,
        }
    }

    /// The schemas that have tables, in the order they first appear.
    pub fn schemas(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for key in self.tables.keys() {
            if !out.contains(&key.schema) {
                out.push(key.schema.clone());
            }
        }
        out
    }

    /// The table a reference names: exactly, or else ignoring case (MySQL on Windows and
    /// macOS keeps table names without case).
    pub fn find(&self, key: &TableKey) -> Option<&TableKey> {
        if let Some((k, _)) = self.tables.get_key_value(key) {
            return Some(k);
        }
        self.tables.keys().find(|k| {
            k.schema.eq_ignore_ascii_case(&key.schema) && k.name.eq_ignore_ascii_case(&key.name)
        })
    }

    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Table {
    pub key: TableKey,
    /// The line of its `CREATE TABLE`.
    pub line: usize,
    pub columns: Vec<Column>,
    /// Empty without a primary key.
    pub primary_key: Vec<String>,
    pub uniques: Vec<Vec<String>>,
    pub foreign_keys: Vec<ForeignKey>,
    pub comment: Option<String>,
}

impl Table {
    pub fn new(key: TableKey, line: usize) -> Self {
        Self {
            key,
            line,
            columns: Vec::new(),
            primary_key: Vec::new(),
            uniques: Vec::new(),
            foreign_keys: Vec::new(),
            comment: None,
        }
    }

    pub fn column(&self, name: &str) -> Option<&Column> {
        self.columns.iter().find(|c| c.name == name)
    }

    pub fn column_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.columns.iter_mut().find(|c| c.name == name)
    }
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Column {
    pub name: String,
    /// The type as written (`int unsigned`, `character varying(50)`, `enum('a','b')`).
    pub datatype: String,
    pub not_null: bool,
    /// The default without quotes, the expression of a generated column; `None` also for
    /// `DEFAULT NULL`.
    pub default: Option<String>,
    pub comment: Option<String>,
    /// `GENERATED ALWAYS AS (…)` or MySQL's `AS (…)`; identity columns are not.
    pub generated: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ForeignKey {
    pub columns: Vec<String>,
    pub target: TableKey,
    /// Empty when the reference names no columns (it means the primary key).
    pub target_columns: Vec<String>,
}

/// Something the parser could not read.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Problem {
    pub line: usize,
    pub kind: ProblemKind,
    /// The start of what was not understood, on one line.
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProblemKind {
    /// A whole statement (`CREATE TABLE` without a name, …).
    Statement,
    /// A column definition.
    Column,
    /// A key or reference.
    Constraint,
    /// `ALTER TABLE` or `COMMENT ON` for a table the dump does not create.
    UnknownTable,
}
