//! UML notation, independent of any UI toolkit:
//!
//! - [`scene`] turns a diagram view into drawable shapes and does hit testing.
//! - [`palette`] lists the tools of each diagram kind, checks connections and creates elements.

pub mod clipboard;
pub mod member;
pub mod palette;
pub mod scene;
