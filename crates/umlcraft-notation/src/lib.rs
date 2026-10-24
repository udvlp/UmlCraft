//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.
//! - [`sequence`] draws sequence diagrams from the geometry Astah stores;
//!   [`sequence_edit`] edits that geometry the way Astah does.
//! - [`route`] runs edges as straight lines, at right angles or as curves (Astah's line styles).
//! - [`draw`] turns a scene's figures into drawing primitives; [`svg`] writes them as an image.

pub mod clipboard;
pub mod draw;
pub mod member;
pub mod metrics;
pub mod palette;
pub mod route;
pub mod scene;
pub mod sequence;
pub mod sequence_edit;
pub mod svg;
