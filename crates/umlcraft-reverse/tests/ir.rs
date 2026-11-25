//! What the Java front-end reads from each fixture, compared with `expected.ir` next to the
//! sources. Run with `UPDATE_SNAPSHOTS=1` to write the snapshots after a deliberate change, and
//! review the diff.

#[macro_use]
mod common;

use umlcraft_reverse::SourceLanguage;

ir_tests!(SourceLanguage::Java;
    packages,
    kinds,
    modifiers,
    javadoc,
    annotations,
    initializers,
    inheritance,
    generics,
    arrays_varargs,
    syntax_error,
    encoding,
);
