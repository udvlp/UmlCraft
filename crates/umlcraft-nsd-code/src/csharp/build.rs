//! A C# member body as structogram elements, the same mapping as Java's where the languages
//! agree (`docs/plan-nsd-csharp.md`).

use tree_sitter::Node;
use umlcraft_nsd::{Diagram, Element, ElementKind, Provenance, RootKind, Sequence};

use super::methods::{Body, Member};
use super::{CSharp, declaration, declarator_value};
use crate::core::{Builder, Catch, Group, Sink, append_comment};
use crate::pseudo;
use crate::text::{condition, lines, node_text, one_line, without_semicolon};
use crate::{Made, NoteKind, Report};

pub fn diagram(member: &Member, src: &str, made: &Made) -> (Diagram, Report) {
    let provenance = Provenance { author: made.author, date: made.date, origin: made.origin };
    let mut d = Diagram::new(RootKind::Sub, std::slice::from_ref(&member.title), &provenance);
    d.set_keywords(made.keywords);
    if let Some(doc) = &member.doc {
        d.set_comment(doc);
    }
    let mut b = Builder::<CSharp>::new(src, d.clone());
    let mut body = Vec::new();
    if let Some(init) = member.initializer {
        // `: base(x)` runs first, as Java's `super(x);` does.
        let text = node_text(init, src).trim_start_matches(':').trim_start();
        body.push(b.element(ElementKind::Instruction, &crate::text::lines_of(text, 0)));
    }
    match &member.body {
        Body::Block(block) => body.extend(b.block(*block)),
        Body::Expression(value) => body.extend(b.arrow(*value, member.returns)),
        Body::Statements(statements) => body.extend(b.statements(statements)),
    }
    d.children = Sequence::new(body);
    (d, Report { notes: b.notes })
}

pub fn statement(b: &mut Builder<CSharp>, node: Node, out: &mut Vec<Element>) {
    let src = b.src;
    match node.kind() {
        "block" => out.extend(b.block(node)),
        "empty_statement" => {}
        _ if node.has_error() => out.push(b.syntax_error(node)),
        "local_declaration_statement" => b.declaration_statement(node, out),
        "expression_statement" => match node.named_child(0) {
            Some(expr) => out.extend(b.expression(expr)),
            None => out.push(b.instruction(node)),
        },
        "if_statement" => out.push(b.if_statement(node)),
        "for_statement" => {
            let counting = counted(node, src)
                .and_then(|(var, start)| pseudo::counting(b.keywords(), node, var, start, src));
            b.for_loop(node, counting, out);
        }
        "foreach_statement" => {
            let name = node.child_by_field_name("left").map(|n| one_line(node_text(n, src)));
            let value = node.child_by_field_name("right");
            out.push(b.traversal(node, &name.unwrap_or_default(), value));
        }
        "while_statement" => out.push(b.while_statement(node)),
        "do_statement" => out.push(b.do_statement(node)),
        "return_statement"
            if node.named_child(0).is_some_and(|v| v.kind() == "switch_expression") =>
        {
            let switch = node.named_child(0).expect("checked above");
            out.extend(b.switch_expression(switch, Some(Sink::Return)));
        }
        "return_statement" => out.push(b.returned(node)),
        "throw_statement" | "break_statement" => out.push(b.jump(node)),
        "continue_statement" => {
            b.note(node, NoteKind::Continue);
            out.push(b.stand_in(node));
        }
        "goto_statement" => {
            b.note(node, NoteKind::Goto);
            out.push(b.jump(node));
        }
        "yield_statement" => {
            b.note(node, NoteKind::Yield);
            // `yield break` leaves the method; `yield return x` hands out a value and goes on.
            let mut words = node_text(node, src).split(|c: char| c.is_whitespace() || c == ';');
            let ends = words.find(|w| !w.is_empty() && *w != "yield") == Some("break");
            out.push(if ends { b.jump(node) } else { b.instruction(node) });
        }
        "labeled_statement" => {
            b.note(node, NoteKind::Label);
            let mut cursor = node.walk();
            let inner: Vec<Node> =
                node.named_children(&mut cursor).filter(|c| c.kind() != "identifier").collect();
            out.extend(b.statements(&inner));
        }
        "switch_statement" => out.extend(b.switch_statement(node)),
        "try_statement" => out.push(b.try_statement(node)),
        "using_statement" => b.using_statement(node, out),
        "lock_statement" => {
            b.note(node, NoteKind::Lock);
            let lock = node.named_child(0).map(|l| node_text(l, src)).unwrap_or_default();
            let body = last_named(node).filter(|_| node.named_child_count() > 1);
            b.framed(format!("lock ({lock})"), body, out);
        }
        "fixed_statement" => {
            let mut cursor = node.walk();
            let declared =
                node.named_children(&mut cursor).find(|c| c.kind() == "variable_declaration");
            let head = declared.map(|d| one_line(node_text(d, src))).unwrap_or_default();
            b.framed(format!("fixed ({head})"), last_named(node), out);
        }
        "checked_statement" | "unsafe_statement" => {
            if let Some(block) = last_named(node) {
                out.extend(b.block(block));
            }
        }
        "local_function_statement" => b.note(node, NoteKind::LocalFunction),
        _ => out.push(b.instruction(node)),
    }
}

/// A declaration or expression without its statement: one instruction per assignment
/// (`x := 1`, none for `int x;`), other expressions as written.
pub fn simple(b: &Builder<CSharp>, node: Node) -> Vec<Element> {
    let texts = match node.kind() {
        "variable_declaration" => declaration(node, b.src),
        _ => vec![pseudo::assignment(node, b.src).unwrap_or_else(|| lines(node, b.src))],
    };
    texts.iter().map(|text| b.element(ElementKind::Instruction, text)).collect()
}

/// The variable a for loop counts and its start: `int i = 0` or `i = 0` as its only
/// initialization.
fn counted<'t>(node: Node<'t>, src: &str) -> Option<(Node<'t>, Node<'t>)> {
    let mut cursor = node.walk();
    let inits: Vec<Node> =
        node.children_by_field_name("initializer", &mut cursor).filter(|n| n.is_named()).collect();
    match inits.as_slice() {
        [init] if init.kind() == "variable_declaration" => {
            let mut cursor = init.walk();
            let declarators: Vec<Node> = init
                .named_children(&mut cursor)
                .filter(|d| d.kind() == "variable_declarator")
                .collect();
            let [d] = declarators.as_slice() else { return None };
            Some((d.child_by_field_name("name")?, declarator_value(*d)?))
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

fn last_named(node: Node) -> Option<Node> {
    node.named_child(node.named_child_count().checked_sub(1)?.try_into().ok()?)
}

impl Builder<'_, CSharp> {
    /// A statement as a jump, without its `;`: `return x`, `throw`, `goto case 2`.
    fn jump(&self, node: Node) -> Element {
        self.element(ElementKind::Jump, &without_semicolon(lines(node, self.src)))
    }

    /// An expression body `=> value`: the value returned, or the expression as a statement.
    fn arrow(&mut self, value: Node, returns: bool) -> Vec<Element> {
        match value.kind() {
            "switch_expression" if returns => self.switch_expression(value, Some(Sink::Return)),
            "throw_expression" => vec![self.jump(value)],
            _ if returns => {
                vec![self.element(
                    ElementKind::Jump,
                    &pseudo::give(self.keywords(), lines(value, self.src)),
                )]
            }
            _ => self.expression(value),
        }
    }

    /// An expression as a statement. Calls of the class's own methods (`F(x)`, `this.F(x)`)
    /// become call elements; a switch expression assigned to a variable becomes a case
    /// element; assignments read `x := value`.
    fn expression(&mut self, expr: Node) -> Vec<Element> {
        let src = self.src;
        match expr.kind() {
            "invocation_expression" => {
                let function = expr.child_by_field_name("function");
                let own = function.is_some_and(|f| match f.kind() {
                    "identifier" | "generic_name" => true,
                    "member_access_expression" => {
                        f.child_by_field_name("expression").is_some_and(|o| o.kind() == "this")
                    }
                    _ => false,
                });
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
                    return self.switch_expression(right, Some(sink));
                }
            }
            _ => {}
        }
        simple(self, expr)
    }

    /// `int a = 1, b;` as its assignments; `var x = v switch { … };` as a case element;
    /// `using var r = Open();` with `using` as a comment.
    fn declaration_statement(&mut self, node: Node, out: &mut Vec<Element>) {
        let src = self.src;
        let mut cursor = node.walk();
        let Some(declared) =
            node.named_children(&mut cursor).find(|c| c.kind() == "variable_declaration")
        else {
            out.push(self.instruction(node));
            return;
        };
        let mut cursor = declared.walk();
        let declarators: Vec<Node> = declared
            .named_children(&mut cursor)
            .filter(|d| d.kind() == "variable_declarator")
            .collect();
        if let [d] = declarators.as_slice()
            && let (Some(name), Some(value)) = (d.child_by_field_name("name"), declarator_value(*d))
            && value.kind() == "switch_expression"
        {
            let sink = Sink::Assign(node_text(name, src).to_owned());
            out.extend(self.switch_expression(value, Some(sink)));
            return;
        }
        let before = out.len();
        out.extend(simple(self, declared));
        let first_word = node_text(node, src).split_whitespace().take(2).collect::<Vec<_>>();
        if first_word.contains(&"using") {
            self.note(node, NoteKind::Using);
            if let Some(first) = out.get_mut(before) {
                append_comment(first, &["using".to_owned()]);
            }
        }
    }

    /// `using (var r = Open()) { … }`: the resource assigned (with `using` as a comment),
    /// then the body in its place.
    fn using_statement(&mut self, node: Node, out: &mut Vec<Element>) {
        self.note(node, NoteKind::Using);
        let body = node.child_by_field_name("body");
        let mut cursor = node.walk();
        let resource = node.named_children(&mut cursor).find(|c| Some(*c) != body);
        let mut resources = match resource {
            Some(r) if r.kind() == "variable_declaration" => simple(self, r),
            Some(r) => vec![self.element(ElementKind::Instruction, &lines(r, self.src))],
            None => Vec::new(),
        };
        if let Some(first) = resources.first_mut() {
            append_comment(first, &["using".to_owned()]);
        }
        out.extend(resources);
        if let Some(body) = body {
            out.extend(self.block(body));
        }
    }

    /// A switch statement as a case element (see [`Builder::cases`]). C#'s sections have no
    /// label nodes: what stands between `case` and `:` is a label.
    fn switch_statement(&mut self, node: Node) -> Vec<Element> {
        let src = self.src;
        let discriminator = node
            .child_by_field_name("value")
            .map(|v| condition(v, src).join(" "))
            .unwrap_or_default();
        let mut groups = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for section in body.named_children(&mut cursor).filter(|s| s.kind() == "switch_section")
            {
                groups.push(section_group(section, src));
            }
        }
        self.cases(discriminator, groups, None)
    }

    /// `v switch { 1 => a, _ => b }` as a case element whose branches hand their value to
    /// `sink`; `_` is the default.
    fn switch_expression(&mut self, node: Node, sink: Option<Sink>) -> Vec<Element> {
        let src = self.src;
        let mut cursor = node.walk();
        let mut discriminator = String::new();
        let mut groups = Vec::new();
        for child in node.named_children(&mut cursor) {
            if child.kind() != "switch_expression_arm" {
                if discriminator.is_empty() && !child.is_extra() {
                    discriminator = condition(child, src).join(" ");
                }
                continue;
            }
            let mut arm_cursor = child.walk();
            let arrow = child.children(&mut arm_cursor).find(|c| c.kind() == "=>");
            let Some(arrow) = arrow else { continue };
            let label = one_line(&src[child.start_byte()..arrow.start_byte()]);
            let Some(value) = last_named(child).filter(|v| v.start_byte() > arrow.start_byte())
            else {
                continue;
            };
            let default = label == "_";
            groups.push(Group {
                node: child,
                labels: if default { Vec::new() } else { vec![label] },
                default,
                rule: true,
                statements: vec![value],
            });
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
                "catch_clause" => {
                    let mut c = child.walk();
                    let parts: Vec<Node> = child.named_children(&mut c).collect();
                    let declared = parts.iter().find(|p| p.kind() == "catch_declaration");
                    let filter = parts.iter().find(|p| p.kind() == "catch_filter_clause");
                    let name = declared
                        .and_then(|d| d.child_by_field_name("name"))
                        .map(|n| node_text(n, src).to_owned());
                    let type_text = declared
                        .and_then(|d| d.child_by_field_name("type"))
                        .map(|t| one_line(node_text(t, src)));
                    let filter =
                        filter.and_then(|f| f.named_child(0)).map(|e| condition(e, src).join(" "));
                    catches.push(Catch {
                        name: name.unwrap_or_default(),
                        // `catch { … }` catches every exception.
                        types: vec![type_text.unwrap_or_else(|| "Exception".to_owned())],
                        filter,
                        body: child.child_by_field_name("body"),
                    });
                }
                "finally_clause" => finally = Some(child),
                _ => {}
            }
        }
        let tried = node.child_by_field_name("body").map(|b| self.block(b)).unwrap_or_default();
        self.try_element(tried, catches, finally)
    }
}

/// The labels and statements of one switch section: `case 1: case 2 when x > 0: …`.
fn section_group<'t>(section: Node<'t>, src: &str) -> Group<'t> {
    let mut labels = Vec::new();
    let mut default = false;
    let mut statements = Vec::new();
    let mut label_start: Option<usize> = None;
    let mut cursor = section.walk();
    for child in section.children(&mut cursor) {
        match child.kind() {
            "case" => label_start = Some(child.end_byte()),
            ":" if label_start.is_some() => {
                let start = label_start.take().expect("checked above");
                labels.push(one_line(&src[start..child.start_byte()]));
            }
            "default" => default = true,
            _ if child.is_named() && label_start.is_none() => statements.push(child),
            _ => {}
        }
    }
    Group { node: section, labels, default, rule: false, statements }
}
