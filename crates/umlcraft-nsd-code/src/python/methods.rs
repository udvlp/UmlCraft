//! The functions of a Python file, for choosing one: functions of the module, methods (in
//! nested classes too) and the code of the module as `__main__`.

use std::collections::BTreeSet;

use tree_sitter::Node;

use umlcraft_reverse::comment;

use crate::MethodRef;
use crate::text::{node_text, one_line, split_lines};

/// Statements of the module that are no part of `__main__`.
const DECLARATIONS: [&str; 6] = [
    "function_definition",
    "decorated_definition",
    "class_definition",
    "import_statement",
    "import_from_statement",
    "future_import_statement",
];

/// A function, or the code of the module.
pub struct Member<'t> {
    pub reference: MethodRef,
    /// The diagram's title: `area(unit)`, `__main__`.
    pub title: String,
    /// The docstring.
    pub doc: Option<Vec<String>>,
    /// The body's statements and comments, without the docstring.
    pub statements: Vec<Node<'t>>,
}

pub fn methods(root: Node, src: &str) -> Vec<MethodRef> {
    members(root, src).into_iter().map(|m| m.reference).collect()
}

/// The member `method` names.
pub fn find<'t>(root: Node<'t>, src: &str, method: &MethodRef) -> Option<Member<'t>> {
    members(root, src)
        .into_iter()
        .find(|m| m.reference.start == method.start && m.reference.name == method.name)
}

/// The names of every function the file defines (methods included).
pub fn function_names(root: Node, src: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "function_definition"
            && let Some(name) = node.child_by_field_name("name")
        {
            names.insert(node_text(name, src).to_owned());
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    names
}

fn members<'t>(root: Node<'t>, src: &str) -> Vec<Member<'t>> {
    let mut out = Vec::new();
    definitions(root, src, &mut Vec::new(), &mut out);
    module_code(root, src, &mut out);
    out.sort_by_key(|m| m.reference.start);
    out
}

/// Functions among a module's or class body's statements, and the methods of its classes.
fn definitions<'t>(body: Node<'t>, src: &str, class: &mut Vec<String>, out: &mut Vec<Member<'t>>) {
    let mut cursor = body.walk();
    for child in body.named_children(&mut cursor) {
        let (definition, decorators) = match child.kind() {
            "decorated_definition" => {
                let Some(d) = child.child_by_field_name("definition") else { continue };
                let mut c = child.walk();
                let names: Vec<String> = child
                    .named_children(&mut c)
                    .filter(|n| n.kind() == "decorator")
                    .map(|n| one_line(node_text(n, src)))
                    .collect();
                (d, names)
            }
            _ => (child, Vec::new()),
        };
        match definition.kind() {
            "function_definition" => {
                if let Some(m) = function(definition, src, class, &decorators) {
                    out.push(m);
                }
            }
            "class_definition" => {
                let name = definition.child_by_field_name("name").map(|n| node_text(n, src));
                class.push(name.unwrap_or_default().to_owned());
                if let Some(body) = definition.child_by_field_name("body") {
                    definitions(body, src, class, out);
                }
                class.pop();
            }
            _ => {}
        }
    }
}

fn function<'t>(
    node: Node<'t>,
    src: &str,
    class: &[String],
    decorators: &[String],
) -> Option<Member<'t>> {
    let name = node_text(node.child_by_field_name("name")?, src).to_owned();
    let body = node.child_by_field_name("body")?;
    // A method's first parameter is the object or class, unless it is static.
    let skip_first = !class.is_empty() && !decorators.iter().any(|d| d == "@staticmethod");
    let mut names = parameter_names(node.child_by_field_name("parameters"), src);
    if skip_first && !names.is_empty() {
        names.remove(0);
    }
    let (doc, statements) = docstring(body, src);
    let end = node
        .child_by_field_name("return_type")
        .or(node.child_by_field_name("parameters"))
        .map_or(node.end_byte(), |n| n.end_byte());
    let start = node.child_by_field_name("name").map_or(node.start_byte(), |n| n.start_byte());
    Some(Member {
        reference: MethodRef {
            class: class.to_vec(),
            name: name.clone(),
            signature: one_line(&src[start..end]),
            line: node.start_position().row + 1,
            start: node.start_byte(),
            end: node.end_byte(),
        },
        title: format!("{name}({})", names.join(", ")),
        doc,
        statements,
    })
}

/// The statements of the module outside functions, classes and imports, as `__main__`.
fn module_code<'t>(root: Node<'t>, src: &str, out: &mut Vec<Member<'t>>) {
    let (_, statements) = docstring(root, src);
    let kept: Vec<Node> =
        statements.into_iter().filter(|s| !DECLARATIONS.contains(&s.kind())).collect();
    // Comments after the last statement, and before the first unless right above it, belong
    // to no code.
    let first = kept.iter().position(|s| s.kind() != "comment");
    let last = kept.iter().rposition(|s| s.kind() != "comment");
    let (Some(mut a), Some(b)) = (first, last) else { return };
    while a > 0 && kept[a - 1].end_position().row + 1 >= kept[a].start_position().row {
        a -= 1;
    }
    let kept = kept[a..=b].to_vec();
    out.push(Member {
        reference: MethodRef {
            class: Vec::new(),
            name: "__main__".to_owned(),
            signature: "__main__".to_owned(),
            line: kept[0].start_position().row + 1,
            start: kept[0].start_byte(),
            end: kept[kept.len() - 1].end_byte(),
        },
        title: "__main__".to_owned(),
        doc: None,
        statements: kept,
    });
}

/// `(a, b=1, *args, c: int, **kw)` → `a, b, *args, c, **kw` (without `*` and `/` alone).
fn parameter_names(parameters: Option<Node>, src: &str) -> Vec<String> {
    let Some(parameters) = parameters else { return Vec::new() };
    let mut cursor = parameters.walk();
    parameters
        .named_children(&mut cursor)
        .filter_map(|p| match p.kind() {
            "identifier" | "list_splat_pattern" | "dictionary_splat_pattern" => {
                Some(node_text(p, src).to_owned())
            }
            "default_parameter" | "typed_default_parameter" => {
                p.child_by_field_name("name").map(|n| node_text(n, src).to_owned())
            }
            "typed_parameter" => p.named_child(0).map(|n| node_text(n, src).to_owned()),
            _ => None,
        })
        .collect()
}

/// A body's docstring (its first statement, if a string) as lines, cleaned as the class import
/// cleans it (like `inspect.cleandoc`), and the other statements with the comments between
/// them.
fn docstring<'t>(body: Node<'t>, src: &str) -> (Option<Vec<String>>, Vec<Node<'t>>) {
    let mut cursor = body.walk();
    let mut statements: Vec<Node> = body.named_children(&mut cursor).collect();
    let Some((doc, statement)) = comment::docstring(body, src) else { return (None, statements) };
    statements.retain(|s| *s != statement);
    (Some(split_lines(&doc)), statements)
}

#[cfg(test)]
mod tests {
    use crate::{Source, SourceLanguage};

    #[test]
    fn functions_methods_and_module_code() {
        let src = r#""""Module."""
import os

def area(width: float, height: float = 1.0, *args, **kw) -> float:
    """Area.

    Multiplies:
        width by height.
    """
    return width * height

class Shop:
    @staticmethod
    def make(name):
        pass

    @classmethod
    def load(cls, path):
        pass

    async def run(self, n):
        pass

    class Inner:
        def go(self):
            pass

# setup
total = area(2)
if __name__ == "__main__":
    print(total)
"#;
        let s = Source::parse(SourceLanguage::Python, src);
        let found: Vec<(String, String, String)> =
            s.methods().into_iter().map(|m| (m.class_name(), m.name, m.signature)).collect();
        let expected = [
            ("", "area", "area(width: float, height: float = 1.0, *args, **kw) -> float"),
            ("Shop", "make", "make(name)"),
            ("Shop", "load", "load(cls, path)"),
            ("Shop", "run", "run(self, n)"),
            ("Shop.Inner", "go", "go(self)"),
            ("", "__main__", "__main__"),
        ];
        let expected: Vec<(String, String, String)> = expected
            .iter()
            .map(|(c, n, s)| ((*c).to_owned(), (*n).to_owned(), (*s).to_owned()))
            .collect();
        assert_eq!(found, expected);
        let made = crate::Made {
            author: "t",
            date: "2026-10-30",
            origin: "test",
            keywords: umlcraft_nsd::Keywords::English,
        };
        let titles: Vec<Vec<String>> =
            s.methods().iter().map(|m| s.diagram(m, &made).unwrap().0.text()).collect();
        assert_eq!(
            titles,
            [
                vec!["area(width, height, *args, **kw)"],
                vec!["make(name)"],
                vec!["load(path)"],
                vec!["run(n)"],
                vec!["go()"],
                vec!["__main__"],
            ]
        );
        let (area, _) = s.diagram(&s.methods()[0], &made).unwrap();
        // Cleaned as `inspect.cleandoc` does: what is indented more stays so.
        assert_eq!(area.comment(), ["Area.", "", "Multiplies:", "    width by height."]);
        assert_eq!(s.methods()[0].file_name(), "area.nsd");
    }
}
