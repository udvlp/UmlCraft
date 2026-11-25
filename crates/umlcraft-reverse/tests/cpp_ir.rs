//! What the C++ front-end reads from each fixture, compared with `expected.ir` next to the
//! sources. Run with `UPDATE_SNAPSHOTS=1` to write the snapshots after a deliberate change, and
//! review the diff.

#[macro_use]
mod common;

use umlcraft_reverse::SourceLanguage;

ir_tests!(SourceLanguage::Cpp;
    namespaces,
    kinds,
    access,
    modifiers,
    docs,
    inheritance,
    templates,
    pointers_arrays,
    definitions,
    typedefs,
    resolution,
    associations,
    preprocessor,
    syntax_error,
    encoding,
);
