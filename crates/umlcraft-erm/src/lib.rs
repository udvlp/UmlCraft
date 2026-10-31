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

pub mod check;
pub mod create;
pub mod model;

pub use check::{Issue, IssueKind, Severity, check};
pub use model::{
    Caption, Cardinality, Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Max, Meta,
    Node, NodeKind, Point, Props, Rect, Style, Text, TextAlign,
};
