//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.
//! - [`sequence`] draws sequence diagrams from the geometry Astah stores;
//!   [`sequence_edit`] edits that geometry the way Astah does.
//! - [`state_edit`] keeps substates in the state or region they are drawn in and edits regions.
//! - [`route`] runs edges as straight lines, at right angles or as curves (Astah's line styles).
//! - [`labels`] moves labels not placed by hand off the nodes, lines and labels they cover.
//! - [`draw`] turns a scene's figures into drawing primitives; [`svg`] writes them as an image.

pub mod activity;
pub mod autoroute;
pub mod clipboard;
pub mod composite;
pub mod draw;
pub mod edit;
pub mod er;
pub mod er_layout;
pub mod invariants;
pub mod labels;
pub mod layout;
pub mod member;
pub mod metrics;
pub mod object;
pub mod palette;
pub mod refusal;
pub mod route;
pub mod scene;
pub mod sequence;
pub mod sequence_edit;
pub mod shared;
pub mod state_edit;
pub mod svg;
