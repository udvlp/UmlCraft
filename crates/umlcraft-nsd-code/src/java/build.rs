//! A Java method body as structogram elements (the mapping of `docs/nsd-spec.md` § 4).

use tree_sitter::Node;
use umlcraft_nsd::{Diagram, Element, ElementKind, Provenance, RootKind, Sequence};

use super::{Java, declaration, title};
use crate::core::{Builder, Catch, Group, Sink};
use crate::pseudo;
use crate::text::{condition, documentation, lines, lines_at, node_text, without_semicolon};
use crate::{Made, NoteKind, Report, SourceLanguage};

const LOCAL_TYPES: [&str; 4] =
    ["class_declaration", "record_declaration", "enum_declaration", "interface_declaration"];

pub fn diagram(method: Node, src: &str, made: &Made) -> (Diagram, Report) {
    let provenance = Provenance { author: made.author, date: made.date, origin: made.origin };
    let mut d = Diagram::new(RootKind::Sub, &[title(method, src)], &provenance);
    d.set_keywords(made.keywords);
    if let Some(doc) = documentation(SourceLanguage::Java, method, src) {
        d.set_comment(&doc);
    }
    let mut b = Builder::<Java>::new(src, d.clone());
    let body = method.child_by_field_name("body").map(|n| b.block(n)).unwrap_or_default();
    d.children = Sequence::new(body);
    (d, Report { notes: b.notes })
}

pub fn statement(b: &mut Builder<Java>, node: Node, out: &mut Vec<Element>) {
    let src = b.src;
    match node.kind() {
        "block" => out.extend(b.block(node)),
        ";" | "empty_statement" => {}
        "local_variable_declaration" => {
            if let Some((name, switch)) = declared_switch(node, src) {
                out.extend(b.switch(switch, Some(Sink::Assign(name))));
            } else if node.has_error() {
                out.push(b.syntax_error(node));
            } else {
                out.extend(simple(b, node));
            }
        }
        "expression_statement" => out.extend(b.expression_statement(node)),
        "switch_expression" => out.extend(b.switch(node, None)),
        "if_statement" => out.push(b.if_statement(node)),
        "for_statement" => {
            let counting = counted(node, src)
                .and_then(|(var, start)| pseudo::counting(b.keywords(), node, var, start, src));
            b.for_loop(node, counting, out);
        }
        "enhanced_for_statement" => {
            let name = node.child_by_field_name("name").map(|n| node_text(n, src));
            out.push(b.traversal(
                node,
                name.unwrap_or_default(),
                node.child_by_field_name("value"),
            ));
        }
        "while_statement" => out.push(b.while_statement(node)),
        "do_statement" => out.push(b.do_statement(node)),
        "return_statement"
            if node.named_child(0).is_some_and(|v| v.kind() == "switch_expression") =>
        {
            let switch = node.named_child(0).expect("checked above");
            out.extend(b.switch(switch, Some(Sink::Return)));
        }
        "return_statement" => out.push(b.returned(node)),
        "throw_statement" | "break_statement" => {
            out.push(b.element(ElementKind::Jump, &without_semicolon(lines(node, src))));
        }
        "yield_statement" => {
            let value = node.named_child(0).map(|v| lines(v, src)).unwrap_or_default();
            out.push(b.valued(value, node));
        }
        "continue_statement" => {
            b.note(node, NoteKind::Continue);
            out.push(b.stand_in(node));
        }
        "labeled_statement" => {
            b.note(node, NoteKind::Label);
            let mut cursor = node.walk();
            let inner: Vec<Node> =
                node.named_children(&mut cursor).filter(|c| c.kind() != "identifier").collect();
            out.extend(b.statements(&inner));
        }
        "try_statement" | "try_with_resources_statement" => out.push(b.try_statement(node)),
        "synchronized_statement" => {
            b.note(node, NoteKind::Synchronized);
            let lock = node.child_by_field_name("lock").or_else(|| node.named_child(0));
            let lock = lock.map(|l| node_text(l, src)).unwrap_or_default();
            b.framed(format!("synchronized {lock}"), node.child_by_field_name("body"), out);
        }
        kind if LOCAL_TYPES.contains(&kind) => b.note(node, NoteKind::LocalClass),
        _ if node.has_error() => out.push(b.syntax_error(node)),
        _ => out.push(b.instruction(node)),
    }
}

/// A declaration or expression without its statement: one instruction per assignment
/// (`x := 1`, none for `int x;`), other expressions as written.
pub fn simple(b: &Builder<Java>, node: Node) -> Vec<Element> {
    let texts = match node.kind() {
        "local_variable_declaration" => declaration(node, b.src),
        _ => vec![pseudo::assignment(node, b.src).unwrap_or_else(|| lines(node, b.src))],
    };
    texts.iter().map(|text| b.element(ElementKind::Instruction, text)).collect()
}

/// The variable a for loop counts and its start: `int i = 0` or `i = 0` as its only
/// initialization.
fn counted<'t>(node: Node<'t>, src: &str) -> Option<(Node<'t>, Node<'t>)> {
    let mut cursor = node.walk();
    let inits: Vec<Node> = node.children_by_field_name("init", &mut cursor).collect();
    match inits.as_slice() {
        [init] if init.kind() == "local_variable_declaration" => {
            let mut cursor = init.walk();
            let declarators: Vec<Node> =
                init.children_by_field_name("declarator", &mut cursor).collect();
            let [d] = declarators.as_slice() else { return None };
            Some((d.child_by_field_name("name")?, d.child_by_field_name("value")?))
        }
        [init] if init.kind() == "assignment_expression" => {
            let op = node_text(init.child_by_field_name("operator")?, src);
            let left = init.child_by_field_name("left")?;
            (op == "=" && left.kind() == "identifier")
                .then_some((left, init.child_by_field_name("right")?))
        }
        _ => None,
    }
}

impl Builder<'_, Java> {
    /// Calls of the class's own methods (`f(x);`, `this.f(x);`) become call elements; a
    /// `switch` assigned to a variable becomes a case element; assignments read `x := value`.
    fn expression_statement(&mut self, node: Node) -> Vec<Element> {
        let src = self.src;
        let Some(expr) = node.named_child(0) else { return vec![self.instruction(node)] };
        match expr.kind() {
            "method_invocation" => {
                let object = expr.child_by_field_name("object");
                let own = object.is_none_or(|o| o.kind() == "this");
                if own {
                    return vec![self.element(ElementKind::Call, &lines(expr, src))];
                }
            }
            "assignment_expression" => {
                let op = expr.child_by_field_name("operator").map(|o| node_text(o, src));
                if let (Some("="), Some(left), Some(right)) =
                    (op, expr.child_by_field_name("left"), expr.child_by_field_name("right"))
                    && right.kind() == "switch_expression"
                {
                    let sink = Sink::Assign(node_text(left, src).to_owned());
                    return self.switch(right, Some(sink));
                }
            }
            "switch_expression" => return self.switch(expr, None),
            _ => {}
        }
        if node.has_error() {
            return vec![self.syntax_error(node)];
        }
        simple(self, expr)
    }

    /// A switch statement or expression as a case element (see [`Builder::cases`]).
    fn switch(&mut self, node: Node, sink: Option<Sink>) -> Vec<Element> {
        let src = self.src;
        let discriminator = node
            .child_by_field_name("condition")
            .map(|c| condition(c, src).join(" "))
            .unwrap_or_default();
        let mut groups: Vec<Group> = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for child in body.named_children(&mut cursor) {
                if matches!(child.kind(), "switch_block_statement_group" | "switch_rule") {
                    groups.push(group(child, src));
                }
            }
        }
        self.cases(discriminator, groups, sink)
    }

    fn try_statement(&mut self, node: Node) -> Element {
        let src = self.src;
        let mut catches = Vec::new();
        let mut finally = None;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "catch_clause" => catches.push(Catch {
                    name: catch_name(child, src),
                    types: catch_type(child, src).split('|').map(|t| t.trim().to_owned()).collect(),
                    filter: None,
                    body: child.child_by_field_name("body"),
                }),
                "finally_clause" => finally = Some(child),
                _ => {}
            }
        }
        let mut tried = Vec::new();
        if let Some(resources) = node.child_by_field_name("resources") {
            let mut cursor = resources.walk();
            for r in resources.named_children(&mut cursor).filter(|r| r.kind() == "resource") {
                // `Reader r = open()` assigns; `r` alone names a variable from before.
                let text = match (r.child_by_field_name("name"), r.child_by_field_name("value")) {
                    (Some(name), Some(value)) => {
                        let column = r.start_position().column;
                        pseudo::assign(node_text(name, src), lines_at(value, src, column))
                    }
                    _ => lines(r, src),
                };
                tried.push(self.element(ElementKind::Instruction, &text));
            }
        }
        if let Some(body) = node.child_by_field_name("body") {
            tried.extend(self.block(body));
        }
        self.try_element(tried, catches, finally)
    }
}

/// The cases of one `case …:` group or `case … ->` rule.
fn group<'t>(node: Node<'t>, src: &str) -> Group<'t> {
    let mut labels = Vec::new();
    let mut default = false;
    let mut statements = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "switch_label" {
            let label = node_text(child, src).trim();
            match label.strip_prefix("case") {
                Some(rest) => labels.push(crate::text::one_line(rest)),
                None => default = true,
            }
        } else {
            statements.push(child);
        }
    }
    Group { node, labels, default, rule: node.kind() == "switch_rule", statements }
}

/// `T x = switch …;` with a single variable: its name and the switch.
fn declared_switch<'t>(node: Node<'t>, src: &str) -> Option<(String, Node<'t>)> {
    let mut cursor = node.walk();
    let declarators: Vec<Node> = node.children_by_field_name("declarator", &mut cursor).collect();
    let [declarator] = declarators.as_slice() else { return None };
    let value = declarator.child_by_field_name("value")?;
    let name = declarator.child_by_field_name("name")?;
    (value.kind() == "switch_expression").then(|| (node_text(name, src).to_owned(), value))
}

fn catch_parameter(clause: Node) -> Option<Node> {
    let mut cursor = clause.walk();
    clause.named_children(&mut cursor).find(|c| c.kind() == "catch_formal_parameter")
}

fn catch_type(clause: Node, src: &str) -> String {
    let parameter = catch_parameter(clause);
    let mut cursor = clause.walk();
    let t =
        parameter.and_then(|p| p.named_children(&mut cursor).find(|c| c.kind() == "catch_type"));
    t.map(|t| crate::text::one_line(node_text(t, src))).unwrap_or_default()
}

fn catch_name(clause: Node, src: &str) -> String {
    catch_parameter(clause)
        .and_then(|p| p.child_by_field_name("name"))
        .map(|n| node_text(n, src).to_owned())
        .unwrap_or_default()
}
