//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.
//! - [`sequence`] lays out sequence diagrams from the order of their interaction.

pub mod clipboard;
pub mod member;
pub mod palette;
pub mod scene;
pub mod sequence;
