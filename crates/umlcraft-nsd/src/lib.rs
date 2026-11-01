//! Nassi-Shneiderman diagrams (structograms) in the `.nsd` format of
//! [Structorizer](https://structorizer.fisch.lu).
//!
//! Files written by Structorizer 3.32-35 are read and written again byte for byte, and diagrams
//! are laid out and drawn line for line as Structorizer draws them without syntax
//! highlighting. The format and the drawing were worked out from Structorizer's XML schema, its
//! user guide, the files it writes and its images; Structorizer is GPL-licensed and none of its
//! code is used here.

pub mod clip;
pub mod content;
pub mod create;
pub mod draw;
pub mod edit;
pub mod history;
pub mod layout;
pub mod metrics;
pub mod model;
pub mod path;
pub mod read;
pub mod save;
pub mod svg;
pub mod text;
pub mod write;

pub use create::{ForLoop, Keywords, Provenance, STRUCTORIZER_VERSION, lines};
pub use edit::EditError;
pub use history::Editor;
pub use metrics::Metrics;
pub use model::{
    Attr, Attrs, Branches, Color, Diagram, Element, ElementKind, Lines, RootKind, RootStyle,
    Sequence,
};
pub use path::{Selection, Slot, Span};
pub use read::{ReadError, read};
pub use write::write;
