//! What the languages share in turning a method body into structogram elements: comments,
//! conditions and loops, the cases of a switch, the parts of a try. A language reads its own
//! nodes and hands the parts over; its [`Grammar`] names the node kinds that differ.

use std::marker::PhantomData;

use tree_sitter::Node;
use umlcraft_nsd::edit::new_element;
use umlcraft_nsd::{Color, Diagram, Element, ElementKind, Keywords, Sequence};

use crate::pseudo;
use crate::text::{comment, condition, lines, lines_at, without_semicolon};
use crate::{Note, NoteKind};

/// Structorizer's colour for what its import could not express.
const RED: Color = Color([0xff, 0x00, 0x00]);

/// A language's node kinds and its statements.
pub trait Grammar: Sized {
    /// Comments between statements.
    const COMMENTS: &'static [&'static str];
    /// Nodes whose children are statements.
    const BLOCKS: &'static [&'static str];
    /// Nodes among statements that are no statements themselves.
    const IGNORED: &'static [&'static str];
    /// Statements that leave a `switch` case.
    const ENDS_CASE: &'static [&'static str];
    /// Whether a case that does not end runs on into the next (Java); C# forbids it, so a
    /// case there never runs on, whatever its last statement.
    const RUNS_ON: bool;
    /// Whether a `break` at the end of a case only ends the case (Java, C#); in Python it
    /// leaves the loop around the `match`.
    const BREAK_ENDS_CASE: bool;
    /// The literal that makes `while (…)` endless: `true`, Python's `True`.
    const TRUE: &'static str;

    /// Appends the elements of one statement.
    fn statement(b: &mut Builder<'_, Self>, node: Node, out: &mut Vec<Element>);

    /// A declaration or expression without its statement (a for loop's initialization or
    /// update): one instruction per assignment, other expressions as written.
    fn simple(b: &Builder<'_, Self>, node: Node) -> Vec<Element>;
}

/// Where the values of a `switch` expression go.
#[derive(Clone)]
pub enum Sink {
    /// `x = switch …`, `T x = switch …`: each value is assigned.
    Assign(String),
    /// `return switch …`.
    Return,
}

/// The cases of a `switch` before they are joined.
pub struct Group<'t> {
    pub node: Node<'t>,
    pub labels: Vec<String>,
    pub default: bool,
    /// `case … ->`: never runs on.
    pub rule: bool,
    pub statements: Vec<Node<'t>>,
}

/// A catch clause: the exception's name (none for C#'s `catch (T)` and `catch`), its types,
/// its filter (C#'s `when (c)`) and its body.
pub struct Catch<'t> {
    pub name: String,
    pub types: Vec<String>,
    pub filter: Option<String>,
    pub body: Option<Node<'t>>,
}

impl Catch<'_> {
    /// Its case label: `IOException, RuntimeException`, `IOException when e.Code > 0`.
    fn label(&self) -> String {
        let types = self.types.join(", ");
        match &self.filter {
            Some(filter) => format!("{types} when {filter}"),
            None => types,
        }
    }
}

/// What a catch of everything names as its type: C++'s `catch (...)`, a bare `except:` and
/// `catch { }`, and the base classes they stand for.
const CATCH_ALL: &[&str] = &["...", "BaseException", "Exception", "Throwable"];

pub struct Builder<'a, G> {
    pub src: &'a str,
    /// The diagram's header, for the keywords for loops are read with.
    header: Diagram,
    pub notes: Vec<Note>,
    /// The switch expressions being built, innermost last (for `yield`).
    sinks: Vec<Sink>,
    /// The names of the functions the file defines, where a call without an object only is
    /// a call of the program's own when it names one of them (Python).
    pub own_functions: std::collections::BTreeSet<String>,
    grammar: PhantomData<G>,
}

impl<'a, G: Grammar> Builder<'a, G> {
    pub fn new(src: &'a str, header: Diagram) -> Self {
        Builder {
            src,
            header,
            notes: Vec::new(),
            sinks: Vec::new(),
            own_functions: Default::default(),
            grammar: PhantomData,
        }
    }

    pub fn note(&mut self, node: Node, kind: NoteKind) {
        self.notes.push(Note { line: node.start_position().row + 1, kind });
    }

    /// The language of the diagram's keywords.
    pub fn keywords(&self) -> Keywords {
        Keywords::of(&self.header)
    }

    pub fn element(&self, kind: ElementKind, text: &[String]) -> Element {
        new_element(&self.header, kind, text)
    }

    /// A statement as written, without its `;`.
    pub fn instruction(&self, node: Node) -> Element {
        self.element(ElementKind::Instruction, &without_semicolon(lines(node, self.src)))
    }

    /// A stand-in for what cannot be shown, as written: red and disabled, as Structorizer
    /// marks it.
    pub fn stand_in(&self, node: Node) -> Element {
        let mut e = self.element(ElementKind::Instruction, &lines(node, self.src));
        e.set_color(RED);
        e.set_disabled(true);
        e
    }

    /// A stand-in for a part the parser could not read, named in the report.
    pub fn syntax_error(&mut self, node: Node) -> Element {
        self.note(node, NoteKind::SyntaxError);
        self.stand_in(node)
    }

    /// A block's statements, or a single statement where a block could be.
    pub fn block(&mut self, node: Node) -> Vec<Element> {
        if G::BLOCKS.contains(&node.kind()) {
            let mut cursor = node.walk();
            let children: Vec<Node> = node.named_children(&mut cursor).collect();
            self.statements(&children)
        } else {
            self.statements(&[node])
        }
    }

    /// The statements of a node's `body`.
    pub fn body(&mut self, node: Node) -> Sequence {
        Sequence::new(node.child_by_field_name("body").map(|b| self.block(b)).unwrap_or_default())
    }

    /// Statements with their comments: a comment goes to the element made of the next
    /// statement; one on the line a statement ends on goes to that statement's element; at
    /// the end of a block to its last element.
    pub fn statements(&mut self, nodes: &[Node]) -> Vec<Element> {
        let mut out: Vec<Element> = Vec::new();
        let mut pending: Vec<String> = Vec::new();
        let mut last_row: Option<(usize, usize)> = None;
        for &node in nodes {
            if G::COMMENTS.contains(&node.kind()) {
                let text = comment(node, self.src);
                match last_row {
                    Some((row, at)) if row == node.start_position().row && at < out.len() => {
                        append_comment(&mut out[at], &text);
                    }
                    _ => pending.extend(text),
                }
                continue;
            }
            if G::IGNORED.contains(&node.kind()) {
                continue;
            }
            let before = out.len();
            if node.is_error() || node.is_missing() {
                out.push(self.syntax_error(node));
            } else {
                G::statement(self, node, &mut out);
            }
            if out.len() > before {
                if !pending.is_empty() {
                    append_comment(&mut out[before], &std::mem::take(&mut pending));
                }
                last_row = Some((node.end_position().row, out.len() - 1));
            }
        }
        if !pending.is_empty()
            && let Some(last) = out.last_mut()
        {
            append_comment(last, &pending);
        }
        out
    }

    /// `if (c) … else …` as an alternative.
    pub fn if_statement(&mut self, node: Node) -> Element {
        let text = node
            .child_by_field_name("condition")
            .map(|c| condition(c, self.src))
            .unwrap_or_default();
        let mut e = self.element(ElementKind::Alternative, &pseudo::question(text));
        if let Some(then) = node.child_by_field_name("consequence") {
            e.branches[0] = Sequence::new(self.block(then));
        }
        if let Some(otherwise) = node.child_by_field_name("alternative") {
            e.branches[1] = Sequence::new(self.block(otherwise));
        }
        e
    }

    /// `while (c)` as a while loop, `while (true)` as an endless one.
    pub fn while_statement(&mut self, node: Node) -> Element {
        let cond = node.child_by_field_name("condition").map(|c| condition(c, self.src));
        let mut e = match cond {
            Some(text) if text == [G::TRUE] => self.element(ElementKind::Forever, &[]),
            text => self.element(ElementKind::While, &pseudo::question(text.unwrap_or_default())),
        };
        e.branches[0] = self.body(node);
        e
    }

    /// `do … while (c)` as a repeat loop until `!(c)?`.
    pub fn do_statement(&mut self, node: Node) -> Element {
        let cond = node.child_by_field_name("condition").map(|c| condition(c, self.src));
        let mut text = cond.unwrap_or_default();
        if let Some(first) = text.first_mut() {
            first.insert_str(0, "!(");
        }
        if let Some(last) = text.last_mut() {
            last.push(')');
        }
        let mut e = self.element(ElementKind::Repeat, &pseudo::question(text));
        e.branches[0] = self.body(node);
        e
    }

    /// A for loop: a counting loop when `counting` is its text, else its initialization
    /// before a while loop (endless without a condition) with its update at the end of the
    /// body.
    pub fn for_loop(&mut self, node: Node, counting: Option<String>, out: &mut Vec<Element>) {
        if let Some(text) = counting {
            let mut e = self.element(ElementKind::For, &[text]);
            e.branches[0] = self.body(node);
            out.push(e);
            return;
        }
        let (inits, updates) = for_parts(node);
        for init in inits {
            out.extend(G::simple(self, init));
        }
        let mut e = match node.child_by_field_name("condition") {
            Some(c) => self.element(ElementKind::While, &pseudo::question(condition(c, self.src))),
            None => self.element(ElementKind::Forever, &[]),
        };
        let mut body = node.child_by_field_name("body").map(|b| self.block(b)).unwrap_or_default();
        for update in updates {
            body.extend(G::simple(self, update));
        }
        e.branches[0] = Sequence::new(body);
        out.push(e);
    }

    /// `foreach name in value` around the loop's body.
    pub fn traversal(&mut self, node: Node, name: &str, value: Option<Node>) -> Element {
        let value = value.map(|v| lines(v, self.src)).unwrap_or_default();
        let mut e =
            self.element(ElementKind::For, &pseudo::traversal(self.keywords(), name, value));
        e.branches[0] = self.body(node);
        e
    }

    /// The body of a statement that only frames it (`synchronized`, `lock` …): its
    /// statements in its place, `head` as a comment on the first of them.
    pub fn framed(&mut self, head: String, body: Option<Node>, out: &mut Vec<Element>) {
        let mut body = body.map(|b| self.block(b)).unwrap_or_default();
        if let Some(first) = body.first_mut() {
            let mut text = vec![head];
            text.extend(first.comment());
            first.set_comment(&text);
        }
        out.extend(body);
    }

    /// Runs `build` with `sink` as where values of a switch expression go.
    fn with_sink<T>(&mut self, sink: Option<Sink>, build: impl FnOnce(&mut Self) -> T) -> T {
        let pushed = sink.is_some();
        if let Some(sink) = sink {
            self.sinks.push(sink);
        }
        let result = build(self);
        if pushed {
            self.sinks.pop();
        }
        result
    }

    /// A value of a switch expression (`yield v;`, `case 1 -> v;`) where it goes.
    pub fn valued(&mut self, value: Vec<String>, node: Node) -> Element {
        match self.sinks.last().cloned() {
            Some(Sink::Assign(name)) => {
                self.element(ElementKind::Instruction, &pseudo::assign(&name, value))
            }
            Some(Sink::Return) => {
                self.element(ElementKind::Jump, &pseudo::give(self.keywords(), value))
            }
            None => self.instruction(node),
        }
    }

    /// `return x;` as a jump `return x` (`gib x`); `return;` stays `return` (`gib`): the method
    /// is left.
    pub fn returned(&self, node: Node) -> Element {
        let mut cursor = node.walk();
        let value = node.named_children(&mut cursor).find(|c| !G::COMMENTS.contains(&c.kind()));
        let column = node.start_position().column;
        let value = value.map(|v| lines_at(v, self.src, column)).unwrap_or_default();
        self.element(ElementKind::Jump, &pseudo::give(self.keywords(), value))
    }

    /// A switch statement or expression as a case element: one branch per case (labels of a
    /// shared body joined with `, `), the default last (`%` when there is none). A case that
    /// runs on gets the following cases' statements copied, as Structorizer does. A switch
    /// without cases is just its default's statements (or nothing).
    pub fn cases(
        &mut self,
        discriminator: String,
        groups: Vec<Group>,
        sink: Option<Sink>,
    ) -> Vec<Element> {
        // An empty group (`case 2:` right before `case 3:`) shares the next one's body.
        let mut merged: Vec<Group> = Vec::new();
        let mut carried: Option<Group> = None;
        for mut g in groups {
            if let Some(c) = carried.take() {
                let mut labels = c.labels;
                labels.extend(g.labels);
                g.labels = labels;
                g.default |= c.default;
            }
            if !g.rule {
                g.statements = unbraced::<G>(g.statements);
            }
            if g.statements.is_empty() && !g.rule {
                carried = Some(g);
            } else {
                merged.push(g);
            }
        }
        if let Some(c) = carried {
            merged.push(c);
        }
        // What runs for each case: its statements and, while they don't end the case, the
        // next case's.
        let mut bodies: Vec<Vec<Node>> = Vec::new();
        for (i, g) in merged.iter().enumerate() {
            let mut body = g.statements.clone();
            let mut j = i;
            while G::RUNS_ON && !g.rule && !ends_case::<G>(&body) && j + 1 < merged.len() {
                j += 1;
                body.extend(merged[j].statements.iter().copied());
            }
            if j > i {
                self.note(g.node, NoteKind::Fallthrough);
            }
            // The last unlabelled `break` only ends the case.
            if G::BREAK_ENDS_CASE
                && body
                    .last()
                    .is_some_and(|s| s.kind() == "break_statement" && s.named_child(0).is_none())
            {
                body.pop();
            }
            bodies.push(body);
        }
        let valued = sink.is_some();
        let default = merged.iter().position(|g| g.default);
        let mut order: Vec<usize> = (0..merged.len()).filter(|&i| Some(i) != default).collect();
        if order.is_empty() {
            let Some(i) = default else { return Vec::new() };
            let rule = merged[i].rule;
            return self.with_sink(sink, |b| b.case_body(&bodies[i], rule && valued));
        }
        let mut text = pseudo::question(vec![discriminator]);
        text.extend(order.iter().map(|&i| merged[i].labels.join(", ")));
        match default {
            Some(i) => {
                order.push(i);
                text.push("default".to_owned());
            }
            None => text.push("%".to_owned()),
        }
        let mut e = self.element(ElementKind::Case, &text);
        self.with_sink(sink, |b| {
            for (branch, &i) in order.iter().enumerate() {
                let elements = b.case_body(&bodies[i], merged[i].rule && valued);
                e.branches[branch] = Sequence::new(elements);
            }
        });
        vec![e]
    }

    /// A case's statements; `value` for the body of a switch expression's case, where a
    /// single expression (Java's `case 1 -> v;`, C#'s `1 => v`) is the value and a `throw`
    /// expression a jump.
    fn case_body(&mut self, body: &[Node], value: bool) -> Vec<Element> {
        let [single] = body else { return self.statements(body) };
        if !value {
            return self.statements(body);
        }
        match single.kind() {
            "expression_statement" => {
                let value = single.named_child(0).map(|v| lines(v, self.src)).unwrap_or_default();
                vec![self.valued(value, *single)]
            }
            "throw_expression" => vec![self.element(ElementKind::Jump, &lines(*single, self.src))],
            kind if G::BLOCKS.contains(&kind) || kind.ends_with("_statement") => {
                self.statements(body)
            }
            _ => vec![self.valued(lines(*single, self.src), *single)],
        }
    }

    /// A try element from its parts. One catch of one type names the exception as the try's
    /// text (none for a catch of everything); one catch of several types, or of one type
    /// without a name, becomes a case on the exception's type; several catches become one
    /// case on `exception`, each branch naming it as its catch does.
    pub fn try_element(
        &mut self,
        tried: Vec<Element>,
        catches: Vec<Catch>,
        finally: Option<Node>,
    ) -> Element {
        let (variable, caught) = match catches.as_slice() {
            [] => (String::new(), Vec::new()),
            [only]
                if only.filter.is_none()
                    && match only.types.as_slice() {
                        [] => true,
                        [one] => !only.name.is_empty() || CATCH_ALL.contains(&one.as_str()),
                        _ => false,
                    } =>
            {
                (only.name.clone(), self.catch_body(only))
            }
            [only] => {
                let name = if only.name.is_empty() { "exception" } else { &only.name };
                let text = vec![format!("{name} instanceof?"), only.label(), "%".into()];
                let mut case = self.element(ElementKind::Case, &text);
                case.branches[0] = Sequence::new(self.catch_body(only));
                (name.to_owned(), vec![case])
            }
            several => {
                let mut text = vec!["exception instanceof?".to_owned()];
                text.extend(several.iter().map(Catch::label));
                text.push("%".to_owned());
                let mut case = self.element(ElementKind::Case, &text);
                for (i, c) in several.iter().enumerate() {
                    let mut body = Vec::new();
                    if !c.name.is_empty() {
                        let rename = pseudo::assign(&c.name, vec!["exception".to_owned()]);
                        body.push(self.element(ElementKind::Instruction, &rename));
                    }
                    body.extend(self.catch_body(c));
                    case.branches[i] = Sequence::new(body);
                }
                ("exception".to_owned(), vec![case])
            }
        };
        let mut e = self.element(ElementKind::Try, &[variable]);
        e.branches[0] = Sequence::new(tried);
        e.branches[1] = Sequence::new(caught);
        if let Some(f) = finally {
            let mut cursor = f.walk();
            let block = f.named_children(&mut cursor).find(|c| G::BLOCKS.contains(&c.kind()));
            e.branches[2] = Sequence::new(block.map(|b| self.block(b)).unwrap_or_default());
        }
        e
    }

    fn catch_body(&mut self, catch: &Catch) -> Vec<Element> {
        catch.body.map(|b| self.block(b)).unwrap_or_default()
    }
}

/// A for loop's initializations and updates (`init` in Java, `initializer` in C#), without
/// the commas between them.
fn for_parts(node: Node) -> (Vec<Node>, Vec<Node>) {
    let mut cursor = node.walk();
    let inits = ["init", "initializer"]
        .iter()
        .flat_map(|field| node.children_by_field_name(field, &mut cursor).collect::<Vec<_>>())
        .filter(|n| n.is_named())
        .collect();
    let mut cursor = node.walk();
    let updates =
        node.children_by_field_name("update", &mut cursor).filter(|n| n.is_named()).collect();
    (inits, updates)
}

/// A case's statements with a closing block opened up (`case 1: { f(); break; }`), so that
/// its `break` ends the case like an unbraced one.
fn unbraced<'t, G: Grammar>(mut statements: Vec<Node<'t>>) -> Vec<Node<'t>> {
    let last = statements.iter().rposition(|s| !G::COMMENTS.contains(&s.kind()));
    if let Some(at) = last
        && G::BLOCKS.contains(&statements[at].kind())
    {
        let block = statements.remove(at);
        let mut cursor = block.walk();
        let inner: Vec<Node> = block.named_children(&mut cursor).collect();
        statements.splice(at..at, unbraced::<G>(inner));
    }
    statements
}

/// Whether the statements leave the case (comments aside).
fn ends_case<G: Grammar>(statements: &[Node]) -> bool {
    statements
        .iter()
        .rev()
        .find(|s| !G::COMMENTS.contains(&s.kind()))
        .is_some_and(|s| G::ENDS_CASE.contains(&s.kind()))
}

pub fn append_comment(e: &mut Element, lines: &[String]) {
    let mut text: Vec<String> = e.comment().into_iter().filter(|l| !l.is_empty()).collect();
    text.extend(lines.iter().cloned());
    e.set_comment(&text);
}
