//! Entity-relationship diagrams in Chen notation: entities, attributes, relationships with
//! `[min,max]` on their lines (look-across), weak entities, n-ary relationships, relationship
//! attributes and generalization (IS-A).
//!
//! The diagrams look like the ER shapes of draw.io. draw.io is Apache-2.0 licensed; the
//! drawing rules (sizes, outlines, where lines end and labels sit) were worked out from its
//! source and its exports, no code of it is used here.
//!
//! - [`model`]: the diagram as a graph of typed nodes, links and free texts.
//! - [`create`]: adding elements with the sizes and texts a new diagram uses.
//! - [`check`]: the rules a valid diagram keeps (which nodes may be joined, owners, …).
//! - [`edit`]: the editor's changes, each keeping the error rules.
//! - [`write`] and [`read`]: the `.ucerm` file, pretty-printed JSON like `.ucraft`.
//! - [`metrics`], [`size`], [`geometry`]: text widths and breaks, text boxes and sizes that
//!   fit a name, outlines, line ends and label positions as draw.io computes them.
//! - [`draw`] and [`svg`]: what is drawn, and the SVG image of it.

pub mod check;
pub mod create;
pub mod draw;
pub mod edit;
pub mod geometry;
pub mod metrics;
pub mod model;
pub mod read;
mod record;
pub mod size;
pub mod svg;
pub mod write;

pub use check::{Issue, IssueKind, Severity, check};
pub use edit::{EditError, End, Item};
pub use model::{
    Caption, Cardinality, Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Max, Meta,
    Node, NodeKind, Point, Props, Rect, Style, Text, TextAlign,
};
pub use read::{ReadError, read, read_value};
pub use write::{to_value, write};

/// What the `format` field of a `.ucerm` file says.
pub const FORMAT_ID: &str = "umlcraft-erm";
/// The version files are written in.
pub const FORMAT_VERSION: u32 = 1;
/// The file extension, without the dot.
pub const FILE_EXTENSION: &str = "ucerm";
