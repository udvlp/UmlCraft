//! Source text of nodes as element texts.

use tree_sitter::Node;
use umlcraft_reverse::SourceLanguage;
use umlcraft_reverse::comment;

pub fn node_text<'s>(node: Node, src: &'s str) -> &'s str {
    &src[node.byte_range()]
}

/// Whitespace runs (line breaks included) as single spaces.
pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A node's text as lines: the first as it starts, the following ones without the
/// indentation the node's first line has in the file, trailing blanks removed.
pub fn lines(node: Node, src: &str) -> Vec<String> {
    lines_of(node_text(node, src), node.start_position().column)
}

/// A node's text as lines, the following ones without the indentation of `column` (where the
/// statement the node is part of starts).
pub fn lines_at(node: Node, src: &str, column: usize) -> Vec<String> {
    lines_of(node_text(node, src), column)
}

pub fn lines_of(text: &str, column: usize) -> Vec<String> {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            let line = if i == 0 {
                line
            } else {
                let blank = line.len() - line.trim_start_matches([' ', '\t']).len();
                &line[blank.min(column)..]
            };
            line.trim_end().to_owned()
        })
        .collect()
}

/// A condition without its outer parentheses: `(a > b)` → `a > b`. C++'s `condition_clause`
/// gives its value; a declaration there (`if (Foo* p = get())`) the name it declares.
pub fn condition(node: Node, src: &str) -> Vec<String> {
    if node.kind() == "condition_clause"
        && let Some(value) = node.child_by_field_name("value")
    {
        if value.kind() == "declaration"
            && let Some(name) = value.child_by_field_name("declarator").and_then(declared_name)
        {
            return vec![node_text(name, src).to_owned()];
        }
        return condition(value, src);
    }
    if node.kind() == "parenthesized_expression"
        && let Some(inner) = node.named_child(0)
    {
        return lines(inner, src);
    }
    lines(node, src)
}

/// The name a C++ declarator declares, through pointers, references, arrays and an
/// initialization (`*p = …` → `p`).
pub fn declared_name(mut declarator: Node) -> Option<Node> {
    loop {
        match declarator.kind() {
            "identifier" | "field_identifier" | "structured_binding_declarator" => {
                return Some(declarator);
            }
            _ => {
                declarator = declarator.child_by_field_name("declarator").or_else(|| {
                    let mut cursor = declarator.walk();
                    declarator
                        .named_children(&mut cursor)
                        .find(|c| !c.kind().ends_with("qualifier"))
                })?;
            }
        }
    }
}

/// The text of a comment without its delimiters (`//`, Python's `#`, `/* */`).
pub fn comment(node: Node, src: &str) -> Vec<String> {
    let text = node_text(node, src);
    if let Some(line) = text.strip_prefix("//").or(text.strip_prefix('#')) {
        return vec![line.trim().to_owned()];
    }
    split_lines(&comment::clean(text))
}

/// The documentation of a declaration as lines, as the class import finds and cleans it
/// (`umlcraft_reverse::comment::documentation`); `None` without one.
pub fn documentation(language: SourceLanguage, node: Node, src: &str) -> Option<Vec<String>> {
    let doc = comment::documentation(language, node, src);
    (!doc.is_empty()).then(|| split_lines(&doc))
}

pub fn split_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

/// Lines without a final `;`.
pub fn without_semicolon(mut lines: Vec<String>) -> Vec<String> {
    if let Some(last) = lines.last_mut()
        && let Some(stripped) = last.strip_suffix(';')
    {
        *last = stripped.trim_end().to_owned();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_lines_lose_the_first_lines_indentation() {
        let text = "builder\n            .add(1)\n            .build();";
        assert_eq!(lines_of(text, 8), ["builder", "    .add(1)", "    .build();"]);
        assert_eq!(lines_of("a\n  b", 8), ["a", "b"]);
    }

    #[test]
    fn semicolons_go_from_the_end_only() {
        assert_eq!(without_semicolon(vec!["return a;".into()]), ["return a"]);
        assert_eq!(without_semicolon(vec!["x".into(), "y ;".into()]), ["x", "y"]);
    }
}
