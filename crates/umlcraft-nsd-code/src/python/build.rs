//! A Python function body as structogram elements, the same mapping as Java's where the
//! languages agree (`docs/plan-nsd-python.md`).

use tree_sitter::Node;
use umlcraft_nsd::{Diagram, Element, ElementKind, Keywords, Provenance, RootKind, Sequence};

use super::Python;
use super::methods::{Member, function_names};
use crate::core::{Builder, Catch, Group, append_comment};
use crate::pseudo;
use crate::text::{condition, lines, lines_at, node_text, one_line, without_semicolon};
use crate::{Made, NoteKind, Report};

pub fn diagram(member: &Member, root: Node, src: &str, made: &Made) -> (Diagram, Report) {
    let provenance = Provenance { author: made.author, date: made.date, origin: made.origin };
    let mut d = Diagram::new(RootKind::Sub, std::slice::from_ref(&member.title), &provenance);
    d.set_keywords(made.keywords);
    if let Some(doc) = &member.doc {
        d.set_comment(doc);
    }
    let mut b = Builder::<Python>::new(src, d.clone());
    b.own_functions = function_names(root, src);
    d.children = Sequence::new(b.statements(&member.statements));
    (d, Report { notes: b.notes })
}

pub fn statement(b: &mut Builder<Python>, node: Node, out: &mut Vec<Element>) {
    let src = b.src;
    match node.kind() {
        "pass_statement" => {}
        _ if node.has_error() => out.push(b.syntax_error(node)),
        "block" => out.extend(b.block(node)),
        "expression_statement" => match node.named_child(0) {
            Some(expr) => out.extend(b.expression(expr)),
            None => out.push(b.instruction(node)),
        },
        "if_statement" => {
            let alternatives = alternatives(node);
            out.push(b.if_chain(node, &alternatives));
        }
        "for_statement" => {
            let left = node.child_by_field_name("left");
            let right = node.child_by_field_name("right");
            let name = left.map(|l| one_line(node_text(l, src))).unwrap_or_default();
            let counting = left
                .filter(|l| l.kind() == "identifier")
                .zip(right)
                .and_then(|(_, r)| range_counting(b.keywords(), &name, r, src));
            out.push(match counting {
                Some(text) => {
                    let mut e = b.element(ElementKind::For, &[text]);
                    e.branches[0] = b.body(node);
                    e
                }
                None => b.traversal(node, &name, right),
            });
            b.loop_else(node, out);
        }
        "while_statement" => {
            out.push(b.while_statement(node));
            b.loop_else(node, out);
        }
        "match_statement" => out.extend(b.match_statement(node)),
        "return_statement" => out.push(b.returned(node)),
        "raise_statement" | "break_statement" => {
            out.push(b.element(ElementKind::Jump, &without_semicolon(lines(node, src))));
        }
        "continue_statement" => {
            b.note(node, NoteKind::Continue);
            out.push(b.stand_in(node));
        }
        "try_statement" => out.push(b.try_statement(node)),
        "with_statement" => b.with_statement(node, out),
        "function_definition" | "decorated_definition" => {
            b.note(node, NoteKind::NestedFunction);
        }
        "class_definition" => b.note(node, NoteKind::LocalClass),
        _ => out.push(b.instruction(node)),
    }
}

/// An expression without its statement: one instruction per assignment, other expressions
/// as written.
pub fn simple(b: &Builder<Python>, node: Node) -> Vec<Element> {
    let texts = match node.kind() {
        "assignment" => assignments(node, b.src),
        _ => vec![pseudo::assignment(node, b.src).unwrap_or_else(|| lines(node, b.src))],
    };
    texts.iter().map(|text| b.element(ElementKind::Instruction, text)).collect()
}

/// `x = 1`, `x: int = 1` → `x := 1`; `a = b = 0` → `a := 0`, `b := 0`; `x: int` → nothing.
fn assignments(node: Node, src: &str) -> Vec<Vec<String>> {
    let column = node.start_position().column;
    let mut targets = Vec::new();
    let mut at = node;
    loop {
        if let Some(left) = at.child_by_field_name("left") {
            targets.push(one_line(node_text(left, src)));
        }
        match at.child_by_field_name("right") {
            Some(right) if right.kind() == "assignment" => at = right,
            Some(right) => {
                let value = lines_at(right, src, column);
                return targets.iter().map(|t| pseudo::assign(t, value.clone())).collect();
            }
            None => return Vec::new(),
        }
    }
}

/// The `elif` and `else` clauses of an `if`.
fn alternatives(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.children_by_field_name("alternative", &mut cursor).collect()
}

/// `for i in range(…)` as a counting loop: `range(n)` → `for i := 0 to n - 1`,
/// `range(a, b, s)` with a constant step `s` → `for i := a to b - 1 by s`.
fn range_counting(keywords: Keywords, var: &str, call: Node, src: &str) -> Option<String> {
    if call.kind() != "call"
        || call.child_by_field_name("function").map(|f| node_text(f, src)) != Some("range")
    {
        return None;
    }
    let arguments = call.child_by_field_name("arguments")?;
    let mut cursor = arguments.walk();
    let args: Vec<Node> =
        arguments.named_children(&mut cursor).filter(|a| a.kind() != "comment").collect();
    if args
        .iter()
        .any(|a| matches!(a.kind(), "keyword_argument" | "list_splat" | "dictionary_splat"))
    {
        return None;
    }
    let (start, bound, step) = match args.as_slice() {
        [bound] => ("0".to_owned(), *bound, 1),
        [start, bound] => (one_line(node_text(*start, src)), *bound, 1),
        [start, bound, step] => {
            let step: i64 = node_text(*step, src).replace(' ', "").parse().ok()?;
            (one_line(node_text(*start, src)), *bound, step)
        }
        _ => return None,
    };
    // The bound is not reached: the last value is one step short of it.
    let end = match step {
        0 => return None,
        s if s > 0 => plus_one(bound, "+", src).unwrap_or_else(|| pseudo::offset(bound, src, -1)),
        _ => plus_one(bound, "-", src).unwrap_or_else(|| pseudo::offset(bound, src, 1)),
    };
    Some(pseudo::counter(keywords, var, &start, &end, step))
}

/// `n` for a bound `n + 1` (`op` `+`) or `n - 1` (`op` `-`), as written.
fn plus_one(bound: Node, op: &str, src: &str) -> Option<String> {
    let operator = bound.child_by_field_name("operator").map(|o| node_text(o, src));
    let right = bound.child_by_field_name("right").map(|r| node_text(r, src));
    if bound.kind() != "binary_operator" || operator != Some(op) || right != Some("1") {
        return None;
    }
    Some(one_line(node_text(bound.child_by_field_name("left")?, src)))
}

impl Builder<'_, Python> {
    /// An expression statement. Calls of the file's own functions (`f(x)`) and of the
    /// object's or class's methods (`self.f(x)`, `cls.f(x)`) become call elements;
    /// assignments read `x := value`.
    fn expression(&mut self, expr: Node) -> Vec<Element> {
        let src = self.src;
        match expr.kind() {
            "call" => {
                let function = expr.child_by_field_name("function");
                let own = function.is_some_and(|f| match f.kind() {
                    "identifier" => self.own_functions.contains(node_text(f, src)),
                    "attribute" => f
                        .child_by_field_name("object")
                        .is_some_and(|o| matches!(node_text(o, src), "self" | "cls")),
                    _ => false,
                });
                let kind = if own { ElementKind::Call } else { ElementKind::Instruction };
                vec![self.element(kind, &lines(expr, src))]
            }
            "yield" => {
                self.note(expr, NoteKind::Yield);
                vec![self.element(ElementKind::Instruction, &lines(expr, src))]
            }
            "assignment" => {
                if expr.child_by_field_name("right").is_some_and(|r| r.kind() == "yield") {
                    self.note(expr, NoteKind::Yield);
                }
                simple(self, expr)
            }
            _ => simple(self, expr),
        }
    }

    /// `if c: … elif d: … else: …` as nested alternatives `c?`, `d?` (as Java's `else if`).
    fn if_chain(&mut self, node: Node, alternatives: &[Node]) -> Element {
        let text = node
            .child_by_field_name("condition")
            .map(|c| condition(c, self.src))
            .unwrap_or_default();
        let mut e = self.element(ElementKind::Alternative, &pseudo::question(text));
        if let Some(then) = node.child_by_field_name("consequence") {
            e.branches[0] = Sequence::new(self.block(then));
        }
        match alternatives.split_first() {
            Some((first, rest)) if first.kind() == "elif_clause" => {
                e.branches[1] = Sequence::new(vec![self.if_chain(*first, rest)]);
            }
            Some((first, _)) => e.branches[1] = self.body(*first),
            None => {}
        }
        e
    }

    /// A loop's `else` body after the loop, named in the report: it runs there after a
    /// `break` too.
    fn loop_else(&mut self, node: Node, out: &mut Vec<Element>) {
        if let Some(otherwise) = node.child_by_field_name("alternative") {
            self.note(otherwise, NoteKind::LoopElse);
            out.extend(self.body(otherwise).elements);
        }
    }

    /// `match x:` as a case element `x?`; each `case`'s pattern (with its guard) is a label,
    /// `case _` the default.
    fn match_statement(&mut self, node: Node) -> Vec<Element> {
        let src = self.src;
        let mut cursor = node.walk();
        let subjects: Vec<Node> = node.children_by_field_name("subject", &mut cursor).collect();
        let discriminator = match (subjects.first(), subjects.last()) {
            (Some(a), Some(b)) => one_line(&src[a.start_byte()..b.end_byte()]),
            _ => String::new(),
        };
        let mut groups = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for clause in body.named_children(&mut cursor).filter(|c| c.kind() == "case_clause") {
                let mut c = clause.walk();
                let tokens: Vec<Node> = clause.children(&mut c).collect();
                let start = tokens.iter().find(|t| t.kind() == "case").map(|t| t.end_byte());
                let end = tokens.iter().rfind(|t| t.kind() == ":").map(|t| t.start_byte());
                let label = match (start, end) {
                    (Some(a), Some(b)) if a < b => one_line(&src[a..b]),
                    _ => String::new(),
                };
                let statements = clause
                    .child_by_field_name("consequence")
                    .map(|block| {
                        let mut c = block.walk();
                        block.named_children(&mut c).collect()
                    })
                    .unwrap_or_default();
                let default = label == "_";
                groups.push(Group {
                    node: clause,
                    labels: if default { Vec::new() } else { vec![label] },
                    default,
                    rule: false,
                    statements,
                });
            }
        }
        self.cases(discriminator, groups, None)
    }

    fn try_statement(&mut self, node: Node) -> Element {
        let src = self.src;
        let mut tried = node.child_by_field_name("body").map(|b| self.block(b)).unwrap_or_default();
        let mut catches = Vec::new();
        let mut finally = None;
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                "except_clause" | "except_group_clause" => {
                    let mut c = child.walk();
                    let parts: Vec<Node> = child
                        .named_children(&mut c)
                        .filter(|p| !matches!(p.kind(), "block" | "comment"))
                        .collect();
                    let mut c = child.walk();
                    let body = child.named_children(&mut c).find(|p| p.kind() == "block");
                    let (types, name) = match parts.first() {
                        Some(p) if p.kind() == "as_pattern" => {
                            let mut c = p.walk();
                            let target =
                                p.named_children(&mut c).find(|t| t.kind() == "as_pattern_target");
                            let name = target.map(|t| one_line(node_text(t, src)));
                            (p.named_child(0).map(|t| types(t, src)), name)
                        }
                        Some(p) => (Some(types(*p, src)), None),
                        None => (None, None),
                    };
                    catches.push(Catch {
                        name: name.unwrap_or_default(),
                        // A bare `except:` catches every exception.
                        types: types.unwrap_or_else(|| vec!["BaseException".to_owned()]),
                        filter: None,
                        body,
                    });
                }
                "else_clause" => {
                    self.note(child, NoteKind::TryElse);
                    tried.extend(self.body(child).elements);
                }
                "finally_clause" => finally = Some(child),
                _ => {}
            }
        }
        self.try_element(tried, catches, finally)
    }

    /// `with open(p) as f, lock:`: `f := open(p)` and `lock` (with `with` as a comment), then
    /// the body in its place.
    fn with_statement(&mut self, node: Node, out: &mut Vec<Element>) {
        let src = self.src;
        self.note(node, NoteKind::With);
        let column = node.start_position().column;
        let mut contexts = Vec::new();
        let mut cursor = node.walk();
        for clause in node.named_children(&mut cursor).filter(|c| c.kind() == "with_clause") {
            let mut c = clause.walk();
            for item in clause.named_children(&mut c).filter(|i| i.kind() == "with_item") {
                let Some(value) = item.named_child(0) else { continue };
                let text = match value.kind() {
                    "as_pattern" => {
                        let mut c = value.walk();
                        let target =
                            value.named_children(&mut c).find(|t| t.kind() == "as_pattern_target");
                        match (value.named_child(0), target) {
                            (Some(v), Some(t)) => pseudo::assign(
                                &one_line(node_text(t, src)),
                                lines_at(v, src, column),
                            ),
                            _ => lines(value, src),
                        }
                    }
                    _ => lines(value, src),
                };
                contexts.push(self.element(ElementKind::Instruction, &text));
            }
        }
        if let Some(first) = contexts.first_mut() {
            append_comment(first, &["with".to_owned()]);
        }
        out.extend(contexts);
        if let Some(body) = node.child_by_field_name("body") {
            out.extend(self.block(body));
        }
    }
}

/// The types an `except` names: `(A, B)` → `A`, `B`.
fn types(expr: Node, src: &str) -> Vec<String> {
    if matches!(expr.kind(), "tuple" | "parenthesized_expression") {
        let mut cursor = expr.walk();
        let items: Vec<String> = expr
            .named_children(&mut cursor)
            .filter(|i| i.kind() != "comment")
            .map(|i| one_line(node_text(i, src)))
            .collect();
        if !items.is_empty() {
            return items;
        }
    }
    vec![one_line(node_text(expr, src))]
}
