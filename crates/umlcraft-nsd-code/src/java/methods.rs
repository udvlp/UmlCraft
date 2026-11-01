//! The methods of a Java file, for choosing one.

use tree_sitter::Node;

use crate::MethodRef;
use crate::text::{node_text, one_line};

const TYPES: [&str; 4] =
    ["class_declaration", "interface_declaration", "enum_declaration", "record_declaration"];

const METHODS: [&str; 3] =
    ["method_declaration", "constructor_declaration", "compact_constructor_declaration"];

pub fn methods(root: Node, src: &str) -> Vec<MethodRef> {
    let mut out = Vec::new();
    types(root, src, &mut Vec::new(), &mut out);
    out.sort_by_key(|m| m.start);
    out
}

/// Looks for type declarations among `node`'s children and their members.
fn types(node: Node, src: &str, class: &mut Vec<String>, out: &mut Vec<MethodRef>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if TYPES.contains(&child.kind()) {
            let name = child.child_by_field_name("name").map(|n| node_text(n, src));
            class.push(name.unwrap_or_default().to_owned());
            if let Some(body) = child.child_by_field_name("body") {
                members(body, src, class, out);
            }
            class.pop();
        }
    }
}

fn members(body: Node, src: &str, class: &mut Vec<String>, out: &mut Vec<MethodRef>) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        if METHODS.contains(&member.kind()) {
            if let Some(m) = method(member, src, class) {
                out.push(m);
            }
        } else if member.kind() == "enum_body_declarations" {
            members(member, src, class, out);
        }
    }
    types(body, src, class, out);
}

fn method(node: Node, src: &str, class: &[String]) -> Option<MethodRef> {
    node.child_by_field_name("body")?;
    let name = node_text(node.child_by_field_name("name")?, src).to_owned();
    Some(MethodRef {
        class: class.to_vec(),
        signature: signature(node, src),
        name,
        line: node.start_position().row + 1,
        start: node.start_byte(),
        end: node.end_byte(),
    })
}

/// Type parameters, result type, name and parameters as written, on one line.
pub fn signature(node: Node, src: &str) -> String {
    let parts = ["type_parameters", "type", "name", "parameters"]
        .iter()
        .filter_map(|field| node.child_by_field_name(field))
        .map(|n| one_line(node_text(n, src)))
        .collect::<Vec<_>>();
    // No space between the name and its parameters.
    let mut s = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 && !part.starts_with('(') {
            s.push(' ');
        }
        s.push_str(part);
    }
    s
}

/// The declaration `method` names.
pub fn find<'t>(root: Node<'t>, method: &MethodRef) -> Option<Node<'t>> {
    let mut node = root.descendant_for_byte_range(method.start, method.start)?;
    while node.start_byte() == method.start {
        if METHODS.contains(&node.kind()) {
            return Some(node);
        }
        node = node.parent()?;
    }
    None
}

#[cfg(test)]
mod tests {
    use crate::{Source, SourceLanguage};

    #[test]
    fn methods_of_nested_types_with_their_signatures() {
        let src = r#"
package a;
public class Outer {
    /** Doc. */
    @Override
    public static <T extends Comparable<T>> T max(final List<T> xs,
                                                  int n) throws Exception {
        return null;
    }
    Outer(int x) { }
    abstract void none();
    class Inner {
        void run() { class Local { void hidden() { } } }
    }
    enum Kind { A, B; int code() { return 1; } }
    interface Shape { default double area() { return 0; } }
    record P(int x) { P { } }
}
"#;
        let found: Vec<(String, String, String)> = Source::parse(SourceLanguage::Java, src)
            .methods()
            .into_iter()
            .map(|m| (m.class_name(), m.name, m.signature))
            .collect();
        let expected = [
            ("Outer", "max", "<T extends Comparable<T>> T max(final List<T> xs, int n)"),
            ("Outer", "Outer", "Outer(int x)"),
            ("Outer.Inner", "run", "void run()"),
            ("Outer.Kind", "code", "int code()"),
            ("Outer.Shape", "area", "double area()"),
            ("Outer.P", "P", "P"),
        ];
        let expected: Vec<(String, String, String)> = expected
            .iter()
            .map(|(c, n, s)| ((*c).to_owned(), (*n).to_owned(), (*s).to_owned()))
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn methods_are_found_again() {
        let s = Source::parse(SourceLanguage::Java, "class A { void f() { } void g() { } }");
        let ms = s.methods();
        assert_eq!(ms[1].line, 1);
        assert_eq!(ms[1].file_name(), "A.g.nsd");
        let made = crate::Made {
            author: "t",
            date: "2026-10-29",
            origin: "test",
            keywords: umlcraft_nsd::Keywords::English,
        };
        let (d, _) = s.diagram(&ms[1], &made).unwrap();
        assert_eq!(d.text(), ["g()"]);
    }

    #[test]
    fn documentation_is_the_block_comment_before_as_for_classes() {
        let src = "class A {\n  /* Plain. */\n  void f() { }\n  // Line.\n  void g() { }\n}";
        let s = Source::parse(SourceLanguage::Java, src);
        let made = crate::Made {
            author: "t",
            date: "2026-10-30",
            origin: "test",
            keywords: umlcraft_nsd::Keywords::English,
        };
        let comments: Vec<Vec<String>> =
            s.methods().iter().map(|m| s.diagram(m, &made).unwrap().0.comment().to_vec()).collect();
        assert_eq!(comments, [vec!["Plain.".to_owned()], vec![]]);
    }
}
