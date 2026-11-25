//! Python: functions and methods of a `.py` file and the code of the module, and their bodies
//! as structograms.

mod build;
mod methods;

pub use build::diagram;
pub use methods::{function_names, members, methods};

/// Python's grammar (`core::Grammar`, in `build`).
pub struct Python;
