//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`labels`] moves labels not placed by hand off the nodes, lines and labels they cover.
//! - [`route`] runs edges as straight lines, at right angles or as curves (Astah's line styles);
//!   [`autoroute`] routes them around the nodes and again after edits that put nodes in their way.
//! - [`shared`] draws generalizations to one general element as a tree on a common line.
//! - [`layout`] arranges diagrams in layers; [`er_layout`] arranges entity-relationship diagrams.
//! - [`draw`] turns a scene's figures into drawing primitives; [`svg`] writes them as an image.
//! - [`metrics`] and [`arial`] hold the measured widths of Astah's fonts.
//! - [`sequence`] draws sequence diagrams from the geometry Astah stores;
//!   [`sequence_edit`] edits that geometry the way Astah does.
//! - [`state_edit`] keeps substates in the state or region they are drawn in and edits regions.
//! - [`composite`] keeps parts inside their structured class and ports on their border;
//!   [`activity`] does the same for pins and activity parameter nodes.
//! - [`er`] edits entity-relationship diagrams and switches between their notations.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.
//! - [`edit`] makes edits as the app does: sizes, ports and routed lines follow them.
//! - [`member`] writes and parses attributes and operations; [`object`] gives an object the
//!   slots of its classifier.
//! - [`clipboard`] copies and pastes diagram content; [`invariants`] states what holds after
//!   every edit.

pub mod activity;
pub mod arial;
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
pub mod route;
pub mod scene;
pub mod sequence;
pub mod sequence_edit;
pub mod shared;
pub mod state_edit;
pub mod svg;
