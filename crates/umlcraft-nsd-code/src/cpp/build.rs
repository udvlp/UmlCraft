//! A C++ function body as structogram elements, the same mapping as Java's where the
//! languages agree (`docs/plan-nsd-cpp.md`).

use tree_sitter::Node;
use umlcraft_nsd::{Diagram, Element, ElementKind, Provenance, RootKind, Sequence};

use super::methods::{Member, function_names};
use super::{Cpp, declaration};
use crate::core::{Builder, Catch, Group};
use crate::pseudo;
use crate::text::{condition, declared_name, lines, node_text, one_line, without_semicolon};
use crate::{Made, NoteKind, Report};

pub fn diagram(member: &Member, root: Node, src: &str, made: &Made) -> (Diagram, Report) {
    let provenance = Provenance { author: made.author, date: made.date, origin: made.origin };
    let mut d = Diagram::new(RootKind::Sub, std::slice::from_ref(&member.title), &provenance);
    d.set_keywords(made.keywords);
    if let Some(doc) = &member.doc {
        d.set_comment(doc);
    }
    let mut b = Builder::<Cpp>::new(src, d.clone());
    b.own_functions = function_names(root, src);
    let mut body = Vec::new();
    if let Some(list) = member.initializers {
        // `: x_(x), Base(x)` run first, each as written.
        let mut cursor = list.walk();
        for init in list.named_children(&mut cursor).filter(|i| i.kind() == "field_initializer") {
            body.push(b.element(ElementKind::Instruction, &lines(init, src)));
        }
    }
    body.extend(b.block(member.body));
    d.children = Sequence::new(body);
    (d, Report { notes: b.notes })
}

pub fn statement(b: &mut Builder<Cpp>, node: Node, out: &mut Vec<Element>) {
    let src = b.src;
    match node.kind() {
        "compound_statement" => out.extend(b.block(node)),
        _ if node.has_error() => out.push(b.syntax_error(node)),
        "expression_statement" => {
            if let Some(expr) = node.named_child(0) {
                out.extend(b.expression(expr));
            }
        }
        "declaration" => out.extend(simple(b, node)),
        "if_statement" => {
            b.clause(node, out);
            out.push(b.if_statement(node));
        }
        "else_clause" | "attributed_statement" => {
            let mut cursor = node.walk();
            let inner: Vec<Node> = node
                .named_children(&mut cursor)
                .filter(|c| c.kind() != "attribute_declaration")
                .collect();
            out.extend(b.statements(&inner));
        }
        "while_statement" => {
            b.clause(node, out);
            out.push(b.while_statement(node));
        }
        "do_statement" => out.push(b.do_statement(node)),
        "for_statement" => {
            let counting = counted(node, src)
                .and_then(|(var, start)| pseudo::counting(b.keywords(), node, var, start, src));
            b.for_loop(node, counting, out);
        }
        "for_range_loop" => {
            if let Some(init) = node.child_by_field_name("initializer") {
                out.extend(b.block(init));
            }
            let name = node
                .child_by_field_name("declarator")
                .and_then(declared_name)
                .map(|n| one_line(node_text(n, src)))
                .unwrap_or_default();
            out.push(b.traversal(node, &name, node.child_by_field_name("right")));
        }
        "switch_statement" => {
            b.clause(node, out);
            out.extend(b.switch_statement(node));
        }
        "return_statement" | "co_return_statement" => out.push(b.returned(node)),
        "throw_statement" | "break_statement" => {
            out.push(b.element(ElementKind::Jump, &without_semicolon(lines(node, src))));
        }
        "continue_statement" => {
            b.note(node, NoteKind::Continue);
            out.push(b.stand_in(node));
        }
        "goto_statement" => {
            b.note(node, NoteKind::Goto);
            out.push(b.element(ElementKind::Jump, &without_semicolon(lines(node, src))));
        }
        "labeled_statement" => {
            b.note(node, NoteKind::Label);
            let mut cursor = node.walk();
            let inner: Vec<Node> = node
                .named_children(&mut cursor)
                .filter(|c| c.kind() != "statement_identifier")
                .collect();
            out.extend(b.statements(&inner));
        }
        "try_statement" => out.push(b.try_statement(node)),
        "co_yield_statement" => {
            b.note(node, NoteKind::Yield);
            out.push(b.instruction(node));
        }
        "init_statement" => {
            let mut cursor = node.walk();
            let inner: Vec<Node> = node.named_children(&mut cursor).collect();
            out.extend(b.statements(&inner));
        }
        kind if kind.ends_with("_specifier") && kind != "type_specifier" => {
            b.note(node, NoteKind::LocalClass);
        }
        _ => out.push(b.instruction(node)),
    }
}

/// A declaration or expression without its statement: one instruction per assignment
/// (`x := 1`, none for `int x;`), other expressions as written.
pub fn simple(b: &Builder<Cpp>, node: Node) -> Vec<Element> {
    // `i++, j--` (in a for loop's update): each part on its own.
    if node.kind() == "comma_expression" {
        let mut cursor = node.walk();
        let parts: Vec<Node> = node.named_children(&mut cursor).collect();
        return parts.into_iter().flat_map(|p| simple(b, p)).collect();
    }
    let texts = match node.kind() {
        "declaration" => declaration(node, b.src),
        _ => vec![pseudo::assignment(node, b.src).unwrap_or_else(|| lines(node, b.src))],
    };
    texts.iter().map(|text| b.element(ElementKind::Instruction, text)).collect()
}

/// The variable a for loop counts and its start: `int i = 0` or `i = 0` as its only
/// initialization.
fn counted<'t>(node: Node<'t>, src: &str) -> Option<(Node<'t>, Node<'t>)> {
    let init = node.child_by_field_name("initializer")?;
    match init.kind() {
        "declaration" => {
            let mut cursor = init.walk();
            let declarators: Vec<Node> =
                init.children_by_field_name("declarator", &mut cursor).collect();
            let [d] = declarators.as_slice() else { return None };
            if d.kind() != "init_declarator" {
                return None;
            }
            let name = d.child_by_field_name("declarator")?;
            let value = d.child_by_field_name("value")?;
            (name.kind() == "identifier" && !value.kind().ends_with("_list"))
                .then_some((name, value))
        }
        "assignment_expression" => {
            let op = node_text(init.child_by_field_name("operator")?, src);
            let left = init.child_by_field_name("left")?;
            (op == "=" && left.kind() == "identifier")
                .then_some((left, init.child_by_field_name("right")?))
        }
        _ => None,
    }
}

impl Builder<'_, Cpp> {
    /// The initialization in `if (init; c)`, `switch (init; c)` and a declaration as the
    /// condition (`if (Foo* p = get())`), as instructions before the statement.
    fn clause(&mut self, node: Node, out: &mut Vec<Element>) {
        let Some(clause) = node.child_by_field_name("condition") else { return };
        if clause.kind() != "condition_clause" {
            return;
        }
        if let Some(init) = clause.child_by_field_name("initializer") {
            let mut cursor = init.walk();
            for part in init.named_children(&mut cursor) {
                out.extend(match part.kind() {
                    "declaration" => simple(self, part),
                    _ => self.expression(part),
                });
            }
        }
        if let Some(value) = clause.child_by_field_name("value")
            && value.kind() == "declaration"
        {
            out.extend(simple(self, value));
        }
    }

    /// An expression as a statement. Calls of the file's own functions (`f(x)`) and of the
    /// object's methods (`this->f(x)`) become call elements; assignments read `x := value`.
    fn expression(&mut self, expr: Node) -> Vec<Element> {
        let src = self.src;
        if expr.kind() == "call_expression" {
            let function = expr.child_by_field_name("function");
            let own = function.is_some_and(|f| match f.kind() {
                "identifier" => self.own_functions.contains(node_text(f, src)),
                "field_expression" => {
                    f.child_by_field_name("argument").is_some_and(|a| a.kind() == "this")
                }
                _ => false,
            });
            let kind = if own { ElementKind::Call } else { ElementKind::Instruction };
            return vec![self.element(kind, &lines(expr, src))];
        }
        simple(self, expr)
    }

    /// A switch statement as a case element (see [`Builder::cases`]); each `case` holds its
    /// statements.
    fn switch_statement(&mut self, node: Node) -> Vec<Element> {
        let src = self.src;
        let discriminator = node
            .child_by_field_name("condition")
            .map(|c| condition(c, src).join(" "))
            .unwrap_or_default();
        let mut groups = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for case in body.named_children(&mut cursor).filter(|c| c.kind() == "case_statement") {
                let value = case.child_by_field_name("value");
                let mut c = case.walk();
                let statements: Vec<Node> =
                    case.named_children(&mut c).filter(|s| Some(*s) != value).collect();
                groups.push(Group {
                    node: case,
                    labels: value.map(|v| one_line(node_text(v, src))).into_iter().collect(),
                    default: value.is_none(),
                    rule: false,
                    statements,
                });
            }
        }
        self.cases(discriminator, groups, None)
    }

    fn try_statement(&mut self, node: Node) -> Element {
        let src = self.src;
        let tried = node.child_by_field_name("body").map(|b| self.block(b)).unwrap_or_default();
        let mut catches = Vec::new();
        let mut cursor = node.walk();
        for clause in node.named_children(&mut cursor).filter(|c| c.kind() == "catch_clause") {
            let parameters = clause.child_by_field_name("parameters");
            let parameter = parameters.and_then(|p| {
                let mut c = p.walk();
                p.named_children(&mut c).find(|n| n.kind() == "parameter_declaration")
            });
            let (types, name) = match parameter {
                // `const std::exception& e`: the type without `const` and `&`.
                Some(p) => (
                    p.child_by_field_name("type").map(|t| one_line(node_text(t, src))),
                    p.child_by_field_name("declarator")
                        .and_then(declared_name)
                        .map(|n| node_text(n, src).to_owned()),
                ),
                None => (Some("...".to_owned()), None),
            };
            catches.push(Catch {
                name: name.unwrap_or_default(),
                types: types.into_iter().collect(),
                filter: None,
                body: clause.child_by_field_name("body"),
            });
        }
        self.try_element(tried, catches, None)
    }
}
