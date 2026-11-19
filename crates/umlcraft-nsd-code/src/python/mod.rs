//! Python: functions and methods of a `.py` file and the code of the module, and their bodies
//! as structograms.

mod build;
mod methods;

use tree_sitter::Node;
use umlcraft_nsd::Element;

use crate::core::{Builder, Grammar};

pub use build::diagram;
pub use methods::{function_names, members, methods};

pub struct Python;

impl Grammar for Python {
    const COMMENTS: &'static [&'static str] = &["comment"];
    const BLOCKS: &'static [&'static str] = &["block"];
    const IGNORED: &'static [&'static str] = &[];
    const ENDS_CASE: &'static [&'static str] = &[];
    const RUNS_ON: bool = false;
    const BREAK_ENDS_CASE: bool = false;
    const TRUE: &'static str = "True";

    fn statement(b: &mut Builder<'_, Self>, node: Node, out: &mut Vec<Element>) {
        build::statement(b, node, out);
    }

    fn simple(b: &Builder<'_, Self>, node: Node) -> Vec<Element> {
        build::simple(b, node)
    }
}
