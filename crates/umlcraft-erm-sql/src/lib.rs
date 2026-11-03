//! Entity-relationship diagrams from SQL dumps (`mysqldump`, `mariadb-dump`, `pg_dump` in
//! plain format): the tables become entities, their columns attributes and their foreign
//! keys relationships, after the rules in [`map`], in a package of the model with an ER
//! diagram laid out by [`umlcraft_notation::er_layout`]; and the way back, an ER diagram as
//! `CREATE TABLE` statements ([`ddl`]).
//!
//! - [`dialect`]: which database wrote the dump, and its name.
//! - [`split`]: the dump as statements (strings, comments, `DELIMITER`, `COPY` data).
//! - [`parse`] into [`ir`]: tables, columns, keys, comments – nothing else is read.
//! - [`map`]: the tables as a Chen diagram, with a report of what was found.
//! - [`ddl`]: the entities of an ER diagram as tables, its relationships as foreign keys.

pub mod ddl;
pub mod dialect;
pub mod ir;
pub mod map;
pub mod parse;
pub mod split;

pub use ddl::{Ddl, LeftOut, ddl, imported_dialect};
pub use dialect::Dialect;
pub use ir::Schema;
pub use map::{Attributes, Hint, Import, Options, Report, import};

/// Why a file cannot be read as a dump at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DumpError {
    /// `pg_dump -Fc` (or `-Ft`): only the plain SQL format is read.
    #[error("the dump is in pg_dump's custom format; export it with -Fp")]
    PgCustomFormat,
    /// gzip, zip, …
    #[error("the dump is compressed")]
    Compressed,
}

/// Reads a dump: in `dialect`, or in the one its header names.
pub fn read(bytes: &[u8], dialect: Option<Dialect>) -> Result<Schema, DumpError> {
    if bytes.starts_with(b"PGDMP") {
        return Err(DumpError::PgCustomFormat);
    }
    if bytes.starts_with(&[0x1f, 0x8b])
        || bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"\x28\xb5\x2f\xfd")
    {
        return Err(DumpError::Compressed);
    }
    let dialect = dialect.unwrap_or_else(|| dialect::detect(bytes));
    Ok(parse::parse(bytes, dialect))
}
