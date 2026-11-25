//! Documentation comments as documentation text: block and Javadoc comments, runs of `///` or
//! `//!` line comments (C#'s XML documentation, Doxygen) and Python docstrings. The class
//! import and the structograms find and clean them alike, so a method's documentation reads
//! the same as an operation and as a diagram's comment.

use tree_sitter::Node;

use crate::SourceLanguage;

/// The documentation of the declaration `node` in a tree of `text`, cleaned, empty for none:
///
/// - Java: the block or Javadoc comment right before it, as Astah takes it (a line comment
///   stands for none).
/// - C#: a run of `///` comments on the lines just above it, or a `/** */` comment.
/// - C++: a run of `///` or `//!` comments on the lines just above it, or a `/** */` or
///   `/*! */` comment. Comments after other code on their line (`int x; ///< …`) belong to
///   that code.
/// - Python: none; its documentation is the docstring ([`docstring`]).
pub fn documentation(language: SourceLanguage, node: Node, text: &str) -> String {
    let at = |n: Node| &text[n.byte_range()];
    match language {
        SourceLanguage::Java => match node.prev_sibling() {
            Some(p) if p.kind() == "block_comment" => clean(at(p)),
            _ => String::new(),
        },
        SourceLanguage::CSharp => {
            let run = line_run(node, |c| c.kind() == "comment" && at(c).starts_with("///"));
            if !run.is_empty() {
                return clean_lines(run.into_iter().map(at));
            }
            match node.prev_sibling() {
                Some(p) if p.kind() == "comment" && at(p).starts_with("/**") => clean(at(p)),
                _ => String::new(),
            }
        }
        SourceLanguage::Cpp => {
            let own_line = |c: Node| match c.prev_sibling() {
                Some(p) => p.end_position().row < c.start_position().row,
                None => true,
            };
            let run = line_run(node, |c| {
                let t = at(c);
                c.kind() == "comment"
                    && (t.starts_with("///") || t.starts_with("//!"))
                    && !t.starts_with("///<")
                    && !t.starts_with("//!<")
                    && own_line(c)
            });
            if !run.is_empty() {
                return clean_lines(run.into_iter().map(at));
            }
            match node.prev_sibling() {
                Some(p)
                    if p.kind() == "comment"
                        && own_line(p)
                        && (at(p).starts_with("/**") || at(p).starts_with("/*!"))
                        && !at(p).starts_with("/**<") =>
                {
                    clean(at(p))
                }
                _ => String::new(),
            }
        }
        SourceLanguage::Python => String::new(),
    }
}

/// The run of siblings right before `node` for which `is_line` holds, each on the line just
/// above the next one, first first: the line comments that document `node`.
fn line_run<'t>(node: Node<'t>, is_line: impl Fn(Node<'t>) -> bool) -> Vec<Node<'t>> {
    let mut run = Vec::new();
    let mut next = node;
    while let Some(p) = next.prev_sibling() {
        let adjacent = p.end_position().row + 1 >= next.start_position().row;
        if !adjacent || !is_line(p) {
            break;
        }
        run.push(p);
        next = p;
    }
    run.reverse();
    run
}

/// The docstring of a Python block (a class's or function's body, a module): its first
/// statement, if that is only a string, cleaned like `inspect.cleandoc`; with that statement.
pub fn docstring<'t>(block: Node<'t>, text: &str) -> Option<(String, Node<'t>)> {
    let mut cursor = block.walk();
    let first = block.named_children(&mut cursor).find(|n| n.kind() != "comment")?;
    let string = docstring_node(first)?;
    Some((cleandoc(&string_content(string, text)), first))
}

/// The text of a Python string literal, without prefix and quotes.
pub(crate) fn string_content(string: Node, text: &str) -> String {
    let mut cursor = string.walk();
    string
        .named_children(&mut cursor)
        .filter(|n| n.kind() == "string_content")
        .map(|n| &text[n.byte_range()])
        .collect()
}

/// The string of a Python expression statement that is only a string: a docstring.
pub(crate) fn docstring_node(statement: Node) -> Option<Node> {
    if statement.kind() != "expression_statement" || statement.named_child_count() != 1 {
        return None;
    }
    statement.named_child(0).filter(|s| s.kind() == "string")
}

/// A docstring as `inspect.cleandoc` cleans it: the first line without its indentation, the
/// others without the indentation they share, blank lines at the start and end dropped.
pub(crate) fn cleandoc(doc: &str) -> String {
    let lines: Vec<&str> = doc.lines().collect();
    let indent = lines
        .iter()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let cleaned: Vec<&str> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let l = if i == 0 { l.trim_start() } else { l.get(indent..).unwrap_or("") };
            l.trim_end()
        })
        .collect();
    join_trimmed(cleaned)
}

/// The text of a `/** … */`, `/*! … */` or `/* … */` comment: without the delimiters, and on
/// every line without the leading whitespace, stars and the blanks after them (Astah removes
/// `\**[ \t]*` at line starts). Blank lines at the start and end are dropped.
pub fn clean(comment: &str) -> String {
    let inner = comment.strip_prefix("/*!").or(comment.strip_prefix("/*")).unwrap_or(comment);
    let inner = inner.strip_suffix("*/").unwrap_or(inner);
    let lines = inner.lines().map(|line| {
        line.trim_start().trim_start_matches('*').trim_start_matches([' ', '\t']).trim_end()
    });
    join_trimmed(lines.collect())
}

/// The text of consecutive `///` or `//!` comments, one per line: without the slashes (and the
/// `!`) and the blanks after them, the rest (XML tags included) as written. Blank lines at the
/// start and end are dropped.
fn clean_lines<'a>(comments: impl IntoIterator<Item = &'a str>) -> String {
    let lines = comments.into_iter().map(|line| {
        let line = line.trim_start();
        let text = line.strip_prefix("///").or(line.strip_prefix("//!")).unwrap_or(line);
        text.trim_start_matches([' ', '\t']).trim_end()
    });
    join_trimmed(lines.collect())
}

fn join_trimmed(lines: Vec<&str>) -> String {
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(first), Some(last)) => lines[first..=last].join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_delimiters_stars_and_blank_edges() {
        let comment = "/**\n   * Holds an order.\n   *\n   *   @author Ulf\n   */";
        assert_eq!(clean(comment), "Holds an order.\n\n@author Ulf");
        assert_eq!(clean("/** One line. */"), "One line.");
        assert_eq!(clean("/***/"), "");
        assert_eq!(clean("/**/"), "");
        assert_eq!(clean("/* Plain. */"), "Plain.");
    }

    #[test]
    fn strips_the_slashes_of_line_comments() {
        let lines = ["///", "/// <summary>", "///   An order.", "/// </summary>", "///"];
        assert_eq!(clean_lines(lines), "<summary>\nAn order.\n</summary>");
        assert_eq!(clean_lines(["///"]), "");
        assert_eq!(clean_lines(["//! Doxygen.", "//!"]), "Doxygen.");
        assert_eq!(clean("/*! Qt style. */"), "Qt style.");
    }

    #[test]
    fn cleans_docstrings() {
        assert_eq!(
            cleandoc("First.\n\n    Indented.\n      More.\n    "),
            "First.\n\nIndented.\n  More."
        );
    }

    #[test]
    fn takes_the_adjacent_run_of_line_comments() {
        let text = "class A {\n  /// Far.\n\n  /// Near\n  /// lines.\n  void f() {}\n}\n";
        let tree = crate::parse_tree(SourceLanguage::CSharp, text);
        let class = tree.root().named_child(0).unwrap();
        let body = class.child_by_field_name("body").unwrap();
        let mut cursor = body.walk();
        let method =
            body.named_children(&mut cursor).find(|n| n.kind() == "method_declaration").unwrap();
        assert_eq!(documentation(SourceLanguage::CSharp, method, text), "Near\nlines.");
    }
}
