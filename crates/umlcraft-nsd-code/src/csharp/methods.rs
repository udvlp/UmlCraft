//! The members of a C# file with a body, for choosing one: methods, constructors, finalizers,
//! operators, the accessors of properties, indexers and events (named as .NET names them,
//! `get_Count`), and the top-level statements as `Program.Main`.

use std::ops::Range;

use tree_sitter::Node;

use super::parameter_names;
use crate::text::{documentation, node_text, one_line};
use crate::{MethodRef, SourceLanguage, operator_name};

const TYPES: [&str; 4] =
    ["class_declaration", "struct_declaration", "record_declaration", "interface_declaration"];

/// What a member's diagram is made of.
pub enum Body<'t> {
    Block(Node<'t>),
    /// `=> value`: the expression.
    Expression(Node<'t>),
    /// The top-level statements.
    Statements(Vec<Node<'t>>),
}

/// A member with a body.
pub struct Member<'t> {
    pub reference: MethodRef,
    /// The diagram's title: `Max(a, b)`, `get_Count()`, `operator +(a, b)`.
    pub title: String,
    pub doc: Option<Vec<String>>,
    pub body: Body<'t>,
    /// Whether an expression body is the member's value (else it is a statement).
    pub returns: bool,
    /// A constructor's `: base(x)` or `: this(x)`.
    pub initializer: Option<Node<'t>>,
    range: Range<usize>,
}

impl Member<'_> {
    /// Where it is in the file.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
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

fn members<'t>(root: Node<'t>, src: &str) -> Vec<Member<'t>> {
    let mut out = Vec::new();
    top_level(root, &mut out);
    declarations(root, src, &mut Vec::new(), &mut out);
    out.sort_by_key(|m| m.reference.start);
    out
}

/// The statements outside any type, as the compiler's `Program.Main(string[] args)`.
fn top_level<'t>(root: Node<'t>, out: &mut Vec<Member<'t>>) {
    let mut cursor = root.walk();
    let children: Vec<Node> = root.named_children(&mut cursor).collect();
    let is_statement = |c: &Node| c.kind() == "global_statement";
    let (Some(a), Some(b)) =
        (children.iter().position(is_statement), children.iter().rposition(is_statement))
    else {
        return;
    };
    // The statements with the comments between them.
    let statements: Vec<Node> = children[a..=b]
        .iter()
        .filter_map(|c| match c.kind() {
            "global_statement" => c.named_child(0),
            "comment" => Some(*c),
            _ => None,
        })
        .collect();
    let (Some(first), Some(last)) = (statements.first(), statements.last()) else { return };
    out.push(Member {
        reference: MethodRef {
            class: vec!["Program".to_owned()],
            name: "Main".to_owned(),
            signature: "Main(string[] args)".to_owned(),
            line: first.start_position().row + 1,
            start: first.start_byte(),
            end: last.end_byte(),
        },
        title: "Main(args)".to_owned(),
        doc: None,
        range: first.start_byte()..last.end_byte(),
        body: Body::Statements(statements),
        returns: false,
        initializer: None,
    });
}

/// Types among `node`'s children (in namespaces too) and their members.
fn declarations<'t>(node: Node<'t>, src: &str, class: &mut Vec<String>, out: &mut Vec<Member<'t>>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "namespace_declaration" | "file_scoped_namespace_declaration" | "declaration_list" => {
                if let Some(body) = child.child_by_field_name("body") {
                    declarations(body, src, class, out);
                } else {
                    declarations(child, src, class, out);
                }
            }
            kind if TYPES.contains(&kind) => {
                let name = child.child_by_field_name("name").map(|n| node_text(n, src));
                class.push(name.unwrap_or_default().to_owned());
                if let Some(body) = child.child_by_field_name("body") {
                    type_members(body, src, class, out);
                }
                class.pop();
            }
            _ => {}
        }
    }
}

fn type_members<'t>(body: Node<'t>, src: &str, class: &[String], out: &mut Vec<Member<'t>>) {
    let mut cursor = body.walk();
    for member in body.named_children(&mut cursor) {
        match member.kind() {
            "method_declaration"
            | "constructor_declaration"
            | "destructor_declaration"
            | "operator_declaration"
            | "conversion_operator_declaration" => {
                if let Some(m) = operation(member, src, class) {
                    out.push(m);
                }
            }
            "property_declaration" | "indexer_declaration" | "event_declaration" => {
                accessors(member, src, class, out);
            }
            _ => {}
        }
    }
    declarations(body, src, &mut class.to_vec(), out);
}

/// A method, constructor, finalizer or operator with a body.
fn operation<'t>(node: Node<'t>, src: &str, class: &[String]) -> Option<Member<'t>> {
    let body = node.child_by_field_name("body")?;
    let declared = node.child_by_field_name("name").map(|n| node_text(n, src).to_owned());
    let parameters = parameter_names(node.child_by_field_name("parameters"), src).join(", ");
    let (name, title) = match node.kind() {
        "destructor_declaration" => {
            let name = format!("~{}", declared.unwrap_or_default());
            (name.clone(), format!("{name}()"))
        }
        "operator_declaration" => {
            let op = node.child_by_field_name("operator").map(|o| node_text(o, src));
            let op = op.unwrap_or_default();
            (operator_name(op, parameters.contains(',')), format!("operator {op}({parameters})"))
        }
        "conversion_operator_declaration" => {
            let implicit = node_text(node, src).contains("implicit");
            let target = node.child_by_field_name("type").map(|t| node_text(t, src));
            let name = if implicit { "op_Implicit" } else { "op_Explicit" };
            (name.to_owned(), format!("operator {}({parameters})", target.unwrap_or_default()))
        }
        _ => {
            let name = declared?;
            (name.clone(), format!("{name}({parameters})"))
        }
    };
    let returns = match node.child_by_field_name("returns") {
        Some(t) => {
            let t = node_text(t, src);
            let is_async = modifiers(node, src).iter().any(|m| m == "async");
            t != "void" && !(is_async && matches!(t, "Task" | "ValueTask"))
        }
        None => matches!(node.kind(), "operator_declaration" | "conversion_operator_declaration"),
    };
    let mut cursor = node.walk();
    let initializer =
        node.named_children(&mut cursor).find(|c| c.kind() == "constructor_initializer");
    Some(Member {
        reference: MethodRef {
            class: class.to_vec(),
            name,
            signature: signature(node, src),
            line: node.start_position().row + 1,
            start: node.start_byte(),
            end: node.end_byte(),
        },
        title,
        doc: doc(node, src),
        range: node.byte_range(),
        body: body_of(body),
        returns,
        initializer,
    })
}

/// The accessors with a body of a property, indexer or event: `get_Count()`,
/// `set_Item(i, value)`, `add_Changed(value)`; `int Count => n;` is a getter.
fn accessors<'t>(node: Node<'t>, src: &str, class: &[String], out: &mut Vec<Member<'t>>) {
    let indexer = node.kind() == "indexer_declaration";
    let property = if indexer {
        "Item".to_owned()
    } else {
        node.child_by_field_name("name").map(|n| node_text(n, src).to_owned()).unwrap_or_default()
    };
    let indices = parameter_names(node.child_by_field_name("parameters"), src);
    let type_text = node.child_by_field_name("type").map(|t| one_line(node_text(t, src)));
    let type_text = type_text.unwrap_or_default();
    let bracketed = node.child_by_field_name("parameters").map(|p| one_line(node_text(p, src)));
    // The indexer's `[int i]` as the accessors' parameters.
    let index_parameters = bracketed
        .as_deref()
        .map(|p| p.trim_start_matches('[').trim_end_matches(']').to_owned())
        .unwrap_or_default();
    let doc = doc(node, src);
    let mut push = |accessor: &str, at: Node<'t>, body: Body<'t>| {
        let reads = accessor == "get";
        let name = format!("{accessor}_{property}");
        let mut names = indices.clone();
        let mut parameters = index_parameters.clone();
        if !reads {
            names.push("value".to_owned());
            if !parameters.is_empty() {
                parameters.push_str(", ");
            }
            parameters.push_str(&format!("{type_text} value"));
        }
        let result = if reads { type_text.as_str() } else { "void" };
        out.push(Member {
            reference: MethodRef {
                class: class.to_vec(),
                name: name.clone(),
                signature: format!("{result} {name}({parameters})"),
                line: at.start_position().row + 1,
                start: at.start_byte(),
                end: at.end_byte(),
            },
            title: format!("{name}({})", names.join(", ")),
            doc: doc.clone(),
            range: at.byte_range(),
            body,
            returns: reads,
            initializer: None,
        });
    };
    if let Some(value) = node.child_by_field_name("value")
        && value.kind() == "arrow_expression_clause"
    {
        push("get", value, body_of(value));
    }
    let Some(list) = node.child_by_field_name("accessors") else { return };
    let mut cursor = list.walk();
    for accessor in list.named_children(&mut cursor).filter(|a| a.kind() == "accessor_declaration")
    {
        let Some(body) = accessor.child_by_field_name("body") else { continue };
        let kind = accessor.child_by_field_name("name").map(|n| node_text(n, src));
        let kind = match kind.unwrap_or_default() {
            "init" => "set",
            other => other,
        };
        push(kind, accessor, body_of(body));
    }
}

fn body_of(body: Node) -> Body {
    if body.kind() == "arrow_expression_clause"
        && let Some(value) = body.named_child(0)
    {
        return Body::Expression(value);
    }
    Body::Block(body)
}

/// The declaration as written, without attributes and modifiers, up to its parameters:
/// `int Max<T>(T a, T b)`, `static Vector operator +(Vector a, Vector b)` without `static`.
fn signature(node: Node, src: &str) -> String {
    let mut cursor = node.walk();
    let start = node
        .children(&mut cursor)
        .find(|c| !matches!(c.kind(), "attribute_list" | "modifier") && !c.is_extra())
        .map_or(node.start_byte(), |c| c.start_byte());
    let end = node.child_by_field_name("parameters").map_or(node.end_byte(), |p| p.end_byte());
    one_line(&src[start..end.max(start)])
}

fn modifiers(node: Node, src: &str) -> Vec<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|c| c.kind() == "modifier")
        .map(|c| node_text(c, src).to_owned())
        .collect()
}

/// The documentation right before a declaration, as the class import takes it: a run of `///`
/// comments (XML as written) or a `/** … */` comment.
fn doc(node: Node, src: &str) -> Option<Vec<String>> {
    documentation(SourceLanguage::CSharp, node, src)
}

#[cfg(test)]
mod tests {
    use crate::{Source, SourceLanguage};

    #[test]
    fn members_with_their_signatures() {
        let src = r#"
using System;
var x = 1;
Console.WriteLine(x);

namespace Shop
{
    public class Outer
    {
        /// <summary>Doc.</summary>
        [Obsolete]
        public static async Task<T> Max<T>(List<T> xs,
                                           int n) where T : IComparable { return default; }
        public Outer(int x) : this() { }
        ~Outer() { }
        public abstract void None();
        public int Count { get { return 1; } private set { } }
        public int Auto { get; set; }
        public int Twice => 2;
        public int this[int i] { get => i; set { } }
        public static Outer operator +(Outer a, Outer b) => a;
        public static implicit operator int(Outer o) => 0;
        struct Inner { void Run() { } }
        interface IShape { double Area() { return 0; } }
        record P(int X) { int Sum() => X; }
    }
}
"#;
        let found: Vec<(String, String, String)> = Source::parse(SourceLanguage::CSharp, src)
            .methods()
            .into_iter()
            .map(|m| (m.class_name(), m.name, m.signature))
            .collect();
        let expected = [
            ("Program", "Main", "Main(string[] args)"),
            ("Outer", "Max", "Task<T> Max<T>(List<T> xs, int n)"),
            ("Outer", "Outer", "Outer(int x)"),
            ("Outer", "~Outer", "~Outer()"),
            ("Outer", "get_Count", "int get_Count()"),
            ("Outer", "set_Count", "void set_Count(int value)"),
            ("Outer", "get_Twice", "int get_Twice()"),
            ("Outer", "get_Item", "int get_Item(int i)"),
            ("Outer", "set_Item", "void set_Item(int i, int value)"),
            ("Outer", "op_Addition", "Outer operator +(Outer a, Outer b)"),
            ("Outer", "op_Implicit", "implicit operator int(Outer o)"),
            ("Outer.Inner", "Run", "void Run()"),
            ("Outer.IShape", "Area", "double Area()"),
            ("Outer.P", "Sum", "int Sum()"),
        ];
        let expected: Vec<(String, String, String)> = expected
            .iter()
            .map(|(c, n, s)| ((*c).to_owned(), (*n).to_owned(), (*s).to_owned()))
            .collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn titles_and_documentation() {
        let src = "namespace A;\nclass B {\n    /// <summary>\n    /// Adds.\n    /// </summary>\n    \
                   int Add(int a, int b) => a + b;\n    int this[int i] { set { } }\n}\n";
        let s = Source::parse(SourceLanguage::CSharp, src);
        let made = crate::Made {
            author: "t",
            date: "2026-10-30",
            origin: "test",
            keywords: umlcraft_nsd::Keywords::English,
        };
        let ms = s.methods();
        let (add, _) = s.diagram(&ms[0], &made).unwrap();
        assert_eq!(add.text(), ["Add(a, b)"]);
        assert_eq!(add.comment(), ["<summary>", "Adds.", "</summary>"]);
        assert_eq!(ms[0].file_name(), "B.Add.nsd");
        let (set, _) = s.diagram(&ms[1], &made).unwrap();
        assert_eq!(set.text(), ["set_Item(i, value)"]);
    }
}
