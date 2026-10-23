//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.
//! - [`sequence`] lays out sequence diagrams from the order of their interaction.
//! - [`draw`] turns a scene's figures into drawing primitives; [`svg`] writes them as an image.

pub mod clipboard;
pub mod draw;
pub mod member;
pub mod palette;
pub mod scene;
pub mod sequence;
pub mod svg;
