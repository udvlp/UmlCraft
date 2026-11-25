//! C#: methods, accessors, operators and top-level statements of a `.cs` file and their bodies
//! as structograms.

mod build;
mod methods;

use tree_sitter::Node;

use crate::pseudo::assign;
use crate::text::{lines_at, node_text};

pub use build::diagram;
pub use methods::{members, methods};

/// C#'s grammar (`core::Grammar`, in `build`).
pub struct CSharp;

/// The names of a parameter list's parameters: `(int a, params string[] rest)` → `a, rest`.
fn parameter_names(parameters: Option<Node>, src: &str) -> Vec<String> {
    let Some(parameters) = parameters else { return Vec::new() };
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter(|p| p.kind() == "parameter")
        .filter_map(|p| p.child_by_field_name("name"))
        .map(|n| node_text(n, src).to_owned())
        .collect()
}

/// The assignments of a variable declaration, one per variable with a value:
/// `int a = 1, b;` → `a := 1`. A variable without a value has none.
pub fn declaration(node: Node, src: &str) -> Vec<Vec<String>> {
    // Continuation lines are taken relative to the statement (`using var r = …` included).
    let column = node.parent().unwrap_or(node).start_position().column;
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|d| d.kind() == "variable_declarator")
        .filter_map(|d| {
            // `var (a, b) = p` declares a tuple pattern instead of a name.
            let mut cursor = d.walk();
            let tuple = d.named_children(&mut cursor).find(|c| c.kind() == "tuple_pattern");
            let name = node_text(d.child_by_field_name("name").or(tuple)?, src);
            Some(assign(name, lines_at(declarator_value(d)?, src, column)))
        })
        .collect()
}

/// The value of `x = value` in a variable declarator (C#'s grammar gives it no field).
fn declarator_value(declarator: Node) -> Option<Node> {
    let name = declarator.child_by_field_name("name");
    let mut cursor = declarator.walk();
    declarator
        .named_children(&mut cursor)
        .filter(|c| Some(*c) != name)
        .filter(|c| !matches!(c.kind(), "bracketed_argument_list" | "tuple_pattern" | "comment"))
        .last()
}
