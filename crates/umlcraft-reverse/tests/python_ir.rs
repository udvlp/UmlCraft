//! What the Python front-end reads from each fixture, compared with `expected.ir` next to the
//! sources. Run with `UPDATE_SNAPSHOTS=1` to write the snapshots after a deliberate change, and
//! review the diff.

#[macro_use]
mod common;

use umlcraft_reverse::SourceLanguage;

ir_tests!(SourceLanguage::Python;
    modules,
    kinds,
    visibility,
    docs,
    inheritance,
    generics,
    type_hints,
    init,
    methods,
    resolution,
    associations,
    statements,
    syntax_error,
    encoding,
);
