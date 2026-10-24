//! Sequence diagrams, drawn from the geometry stored with the presentations, the way Astah
//! stores and draws it:
//!
//! - The diagram frame is a node presentation of the diagram itself, with "sd" and the name in
//!   a tab at its top left.
//! - A lifeline ([`Shape::Lifeline`]) has a head, a box or an icon, and a dashed line of a stored
//!   length below it.
//! - Activation bars ([`Shape::Activation`]) hang on a lifeline with a stored top and height;
//!   their x follows from the lifeline and from how many bars on it they lie within.
//! - A message ([`Shape::Message`]) runs at a stored height between two lifelines; its ends
//!   meet the activation bars there. Messages are numbered by the bars they are sent from.
//! - Combined fragments and interaction uses are frames with stored bounds.
//!
//! The order of the interaction's children follows the heights.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    ElementKind, Id, InteractionOperator, MessageGate, MessageSort, Model, Point, Rect, Shape, View,
};

use crate::scene::{
    EdgeLabel, EdgeShape, EndMarker, Figure, HeadIcon, LabelAlign, LineStyle, NodeShape, Row,
    RowRole, TextMeasure, line_height,
};

/// Height of a lifeline's head box as Astah creates it.
pub const HEAD_HEIGHT: f32 = 30.0;
/// Width of a lifeline's head box as Astah creates it.
pub const HEAD_WIDTH: f32 = 110.0;
/// Width of an activation bar; a bar within another one sits one width further right.
pub const BAR: f32 = 10.0;
/// Radius of the dot at the open end of a found or lost message.
pub const GATE_DOT: f32 = 4.0;
/// How far a message to self loops out to the right of its bar.
const SELF_WIDTH: f32 = 40.0;
/// How far right a message that branches off another one runs before it turns.
const BRANCH_RUN: f32 = 20.0;
/// The top of a message's label above its line.
const LABEL_ABOVE: f32 = 21.05;
/// The top of a message's stereotype above the top of its label.
const STEREOTYPE_ABOVE: f32 = 16.55;
/// The label of a message to self is centered this far right of the loop.
const SELF_LABEL: f32 = 7.45;
/// Height of a frame's tab: one line of text and 10.
pub const TAB_HEIGHT: f32 = 25.093_75;
/// Texts in a tab start this far from the frame's left edge.
const TAB_INDENT: f32 = 10.0;
/// Below a node's top: the top of a text row whose baseline Astah puts 17 or 18 px down.
const ROW_17: f32 = 17.0 - 12.06;
const ROW_18: f32 = 18.0 - 12.06;
/// Edge labels have their baseline 13 below their top, rows 12.06.
const LABEL_TO_ROW: f32 = 13.0 - 12.06;

/// What the sequence layout adds to a scene besides nodes and edges.
#[derive(Clone, Debug, Default)]
pub struct SequenceLayout {
    pub lanes: Vec<Lane>,
    /// Where lifelines end in a cross.
    pub destructions: Vec<Point>,
    /// The dots at the open ends of found and lost messages.
    pub gates: Vec<Point>,
    /// Gaps where a message or fragment can go, in no particular order.
    pub slots: Vec<Slot>,
    /// Horizontal extent of the lifelines, e.g. for drawing an insertion line.
    pub left: f32,
    pub right: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub presentation: Id,
    pub lifeline: Id,
    pub center: f32,
}

/// A place in the interaction: before child `index` of `owner` (an interaction or operand),
/// drawn at height `y`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub y: f32,
    pub owner: Id,
    pub index: usize,
}

impl SequenceLayout {
    /// The place at height `y`: in the interaction or operand of the nearest gap. Edits put
    /// things at `y` itself; the order of the interaction follows the heights.
    pub fn slot_at(&self, y: f32) -> Option<Slot> {
        self.slots
            .iter()
            .copied()
            .min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs()))
            .map(|slot| Slot { y, ..slot })
    }

    /// The lifelines left and right of `x`: what a new fragment placed there covers.
    pub fn lanes_around(&self, x: f32) -> Vec<Id> {
        let mut lanes = self.lanes.clone();
        lanes.sort_by(|a, b| a.center.total_cmp(&b.center));
        let n = lanes.len();
        let range = match lanes.iter().rposition(|l| l.center <= x) {
            None => 0..2,
            Some(i) if i + 1 >= n => i.saturating_sub(1)..i + 1,
            Some(i) => i..i + 2,
        };
        lanes[range.start.min(n)..range.end.min(n)].iter().map(|l| l.lifeline).collect()
    }
}

pub fn operator_keyword(operator: InteractionOperator) -> &'static str {
    use InteractionOperator as O;
    match operator {
        O::Alt => "alt",
        O::Opt => "opt",
        O::Loop => "loop",
        O::Par => "par",
        O::Break => "break",
        O::Critical => "critical",
        O::Neg => "neg",
        O::Assert => "assert",
        O::Strict => "strict",
        O::Seq => "seq",
        O::Ignore => "ignore",
        O::Consider => "consider",
    }
}

/// The label of an interaction use: its name, or else the name of the interaction it refers
/// to, with the arguments in parentheses.
pub fn interaction_use_text(model: &Model, id: Id) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    let ElementKind::InteractionUse(p) = e.kind() else { return e.name().to_owned() };
    let referred = p.refers_to.and_then(|r| model.get(r)).map(|r| r.name());
    let name = if e.name().is_empty() { referred.unwrap_or_default() } else { e.name() };
    if p.arguments.trim().is_empty() { name.to_owned() } else { format!("{name}({})", p.arguments) }
}

/// The label of a message: `name(arguments)` for calls, the return value for replies.
pub fn message_text(model: &Model, message: Id) -> String {
    let (stereotype, text) = message_parts(model, message, true, true);
    match stereotype {
        Some(s) => format!("{s} {text}").trim_end().to_owned(),
        None => text,
    }
}

/// A message's stereotypes (with «create» and «destroy» for those messages) and its text. Calls
/// show their arguments in parentheses, as Astah does, unless `parameters` is off. A reply shows
/// its name; with `reply_value`, its return value instead if it has one (on the diagram, Astah
/// shows that after the call).
fn message_parts(
    model: &Model,
    message: Id,
    parameters: bool,
    reply_value: bool,
) -> (Option<String>, String) {
    let Some(e) = model.get(message) else { return (None, String::new()) };
    let ElementKind::Message(p) = e.kind() else { return (None, e.name().to_owned()) };
    let operation = p.operation.and_then(|o| model.get(o)).map(|o| o.name());
    let name = if e.name().is_empty() { operation.unwrap_or_default() } else { e.name() };
    let text = match p.sort {
        MessageSort::Reply if reply_value && !p.return_value.is_empty() => p.return_value.clone(),
        MessageSort::Reply => name.to_owned(),
        _ if parameters && !name.is_empty() => format!("{name}({})", p.arguments),
        _ => name.to_owned(),
    };
    let mut stereotypes = e.data().stereotypes.clone();
    let keyword = match p.sort {
        MessageSort::Create => Some("create"),
        MessageSort::Destroy => Some("destroy"),
        _ => None,
    };
    if let Some(keyword) = keyword
        && !stereotypes.iter().any(|s| s == keyword)
    {
        stereotypes.insert(0, keyword.to_owned());
    }
    let stereotype = (!stereotypes.is_empty()).then(|| format!("<<{}>>", stereotypes.join(", ")));
    (stereotype, text)
}

/// An activation bar as laid out.
#[derive(Clone, Copy)]
struct Bar {
    id: Id,
    top: f32,
    bottom: f32,
    left: f32,
    level: usize,
}

impl Bar {
    fn right(&self) -> f32 {
        self.left + BAR
    }

    fn contains(&self, y: f32) -> bool {
        self.top - 0.5 <= y && y <= self.bottom + 0.5
    }
}

/// A lifeline as laid out.
struct LaneState {
    head: Rect,
    bars: Vec<Bar>,
}

impl LaneState {
    fn center(&self) -> f32 {
        self.head.center().x
    }

    /// The innermost bar at `y`, leaving out `except`.
    fn bar_at(&self, y: f32, except: Option<Id>) -> Option<Bar> {
        self.bars
            .iter()
            .filter(|b| b.contains(y) && Some(b.id) != except)
            .max_by_key(|b| b.level)
            .copied()
    }

    /// The bar a message arriving at `y` starts, if any.
    fn bar_starting(&self, y: f32) -> Option<Bar> {
        self.bars.iter().filter(|b| (b.top - y).abs() < 0.5).max_by_key(|b| b.level).copied()
    }

    /// Where a message at `y` meets this lifeline coming from the side `from_right`.
    fn side(&self, bar: Option<Bar>, from_right: bool) -> f32 {
        match bar {
            Some(b) if from_right => b.right(),
            Some(b) => b.left,
            None => self.center(),
        }
    }
}

/// A message shown on the diagram.
struct Msg {
    presentation: Id,
    element: Option<Id>,
    source: Id,
    target: Id,
    y: f32,
    end_y: Option<f32>,
    gate_x: Option<f32>,
    label: Option<Point>,
    parameters: bool,
    sort: MessageSort,
    gate: MessageGate,
}

/// Lays out the sequence diagram `diagram`: gives lifelines, bars, frames and fragments their
/// figures and texts and returns the message edges.
pub(crate) fn arrange(
    model: &Model,
    diagram: Id,
    view: &View,
    nodes: &mut Vec<NodeShape>,
    measure: &dyn TextMeasure,
) -> (SequenceLayout, Vec<EdgeShape>) {
    let mut layout = SequenceLayout::default();
    let mut lanes: HashMap<Id, LaneState> = HashMap::new();
    let hide_numbers = matches!(
        model.get(diagram).map(|d| d.kind()),
        Some(ElementKind::Diagram(p)) if p.hide_message_numbers
    );

    for node in nodes.iter_mut() {
        let Some(p) = view.get(node.id) else { continue };
        match &p.shape {
            Shape::Lifeline { length, destroyed, .. } => {
                let Some(lifeline) = p.element else { continue };
                let icon = if p.style.iconic { head_icon(model, lifeline) } else { HeadIcon::Box };
                let head = node.rect;
                let (line_top, rows) = lifeline_rows(model, lifeline, head, icon, measure);
                let bottom = line_top + length;
                node.rect = Rect::new(head.x, head.y, head.width, bottom - head.y);
                node.figure = Figure::Lifeline { head, icon, line: (line_top, bottom) };
                node.rows = rows;
                if *destroyed {
                    layout.destructions.push(Point::new(head.center().x, bottom));
                }
                layout.lanes.push(Lane { presentation: p.id, lifeline, center: head.center().x });
                lanes.insert(p.id, LaneState { head, bars: Vec::new() });
            }
            Shape::Node { .. } if p.element == Some(diagram) => {
                let name = model.get(diagram).map(|d| d.name()).unwrap_or_default();
                (node.figure, node.rows) = diagram_frame(name, node.rect, diagram, measure);
            }
            Shape::Fragment { separators, guards, .. } => {
                let Some(e) = p.element.and_then(|e| model.get(e)) else { continue };
                let ElementKind::CombinedFragment(props) = e.kind() else { continue };
                let operands = model.children(e.id()).to_vec();
                let r = node.rect;
                let separators: Vec<f32> = separators.iter().map(|s| r.y + s).collect();
                let guard_of = |operand: Id| match model.get(operand).map(|o| o.kind()) {
                    Some(ElementKind::InteractionOperand(o)) if !o.guard.trim().is_empty() => {
                        Some(format!("[{}]", o.guard))
                    }
                    _ => None,
                };
                // Astah writes a loop's condition into the tab.
                let in_tab = props.operator == InteractionOperator::Loop;
                let keyword = match operands.first().and_then(|&o| guard_of(o)) {
                    Some(guard) if in_tab => format!("loop {guard}"),
                    _ => operator_keyword(props.operator).to_owned(),
                };
                let tab = tab(&keyword, r, measure);
                let mut rows = vec![tab_row(&keyword, r, ROW_17, e.id())];
                for (k, &operand) in operands.iter().enumerate() {
                    let Some(text) = guard_of(operand) else { continue };
                    if k == 0 && in_tab {
                        continue;
                    }
                    let top = if k == 0 {
                        tab.bottom()
                    } else {
                        separators.get(k - 1).copied().unwrap_or(r.bottom())
                    };
                    let moved = guards.get(k).copied().flatten().unwrap_or_default();
                    let w = measure.width(&text, 12.0, false);
                    rows.push(Row {
                        rect: Rect::new(
                            r.x + TAB_INDENT + moved.x,
                            top + 2.0 + moved.y + LABEL_TO_ROW,
                            w,
                            line_height(12.0),
                        ),
                        ..plain_row(text, operand)
                    });
                }
                node.figure = Figure::Fragment { tab, separators, filled: false };
                node.rows = rows;
            }
            Shape::Node { .. }
                if matches!(
                    p.element.and_then(|e| model.get(e)).map(|e| e.kind()),
                    Some(ElementKind::InteractionUse(_))
                ) =>
            {
                let id = p.element.expect("matched above");
                let r = node.rect;
                let tab = tab("ref", r, measure);
                let text = interaction_use_text(model, id);
                let w = measure.rounded_width(&text, 12.0, false);
                let name = Row {
                    rect: Rect::new(
                        r.x + (r.width - w) / 2.0,
                        tab.bottom() + ROW_18,
                        w,
                        line_height(12.0),
                    ),
                    role: RowRole::Name,
                    ..plain_row(text, id)
                };
                node.figure = Figure::Fragment { tab, separators: Vec::new(), filled: true };
                node.rows = vec![tab_row("ref", r, ROW_18, id), name];
            }
            _ => {}
        }
    }

    // Bars, with x from their lifeline and from the bars they lie within.
    let mut bars: Vec<(Id, Id, f32, f32)> = view
        .presentations()
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Activation { lifeline, top, height } if lanes.contains_key(&lifeline) => {
                Some((p.id, lifeline, top, top + height))
            }
            _ => None,
        })
        .collect();
    bars.sort_by(|a, b| a.2.total_cmp(&b.2).then(b.3.total_cmp(&a.3)));
    for &(id, lane, top, bottom) in &bars {
        let state = lanes.get_mut(&lane).expect("filtered above");
        let level = state.bars.iter().filter(|b| b.top <= top && bottom <= b.bottom).count();
        let left = state.center() - BAR / 2.0 + level as f32 * BAR;
        state.bars.push(Bar { id, top, bottom, left, level });
    }
    for node in nodes.iter_mut() {
        if let Some(Shape::Activation { lifeline, .. }) = view.get(node.id).map(|p| &p.shape)
            && let Some(bar) =
                lanes.get(lifeline).and_then(|l| l.bars.iter().find(|b| b.id == node.id))
        {
            node.rect = Rect::new(bar.left, bar.top, BAR, bar.bottom - bar.top);
            node.figure = Figure::Activation;
            node.rows = Vec::new();
        }
    }
    // Bars are not nodes of their own presentations' bounds; add those the scene left out.
    let present: HashSet<Id> = nodes.iter().map(|n| n.id).collect();
    for p in view.presentations() {
        let Shape::Activation { lifeline, .. } = p.shape else { continue };
        if present.contains(&p.id) {
            continue;
        }
        let Some(bar) = lanes.get(&lifeline).and_then(|l| l.bars.iter().find(|b| b.id == p.id))
        else {
            continue;
        };
        nodes.push(NodeShape {
            id: p.id,
            element: None,
            rect: Rect::new(bar.left, bar.top, BAR, bar.bottom - bar.top),
            figure: Figure::Activation,
            rows: Vec::new(),
            fill: p.style.fill,
        });
    }
    // Keep the drawing order of the presentations: bars come right after what precedes them.
    let order: HashMap<Id, usize> =
        view.presentations().iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    nodes.sort_by_key(|n| order.get(&n.id).copied().unwrap_or(usize::MAX));

    let frame =
        view.presentations().iter().find(|p| p.element == Some(diagram)).and_then(|p| p.bounds());
    let messages: Vec<Msg> = view
        .presentations()
        .iter()
        .filter_map(|p| {
            let Shape::Message { source, target, y, end_y, gate_x, labels } = p.shape else {
                return None;
            };
            if !lanes.contains_key(&source) || !lanes.contains_key(&target) {
                return None;
            }
            let props = p.element.and_then(|e| model.get(e)).and_then(|e| match e.kind() {
                ElementKind::Message(m) => Some(m),
                _ => None,
            });
            Some(Msg {
                presentation: p.id,
                element: p.element,
                source,
                target,
                y,
                end_y,
                gate_x,
                label: labels.name,
                parameters: !p.style.hide_parameters,
                sort: props.map_or(MessageSort::Synchronous, |m| m.sort),
                gate: props.map_or(MessageGate::None, |m| m.gate),
            })
        })
        .collect();
    let numbers = if hide_numbers { HashMap::new() } else { numbers(&messages, &lanes) };
    let hide_returns = matches!(
        model.get(diagram).map(|d| d.kind()),
        Some(ElementKind::Diagram(p)) if p.hide_return_values
    );
    let returns = if hide_returns { HashMap::new() } else { returns(model, &messages, &lanes) };
    let edges = messages
        .iter()
        .map(|m| {
            let label = Label {
                number: numbers.get(&m.presentation),
                returned: returns.get(&m.presentation),
            };
            message_edge(model, m, &lanes, frame, label, &mut layout)
        })
        .collect();

    layout.slots = slots(model, diagram, view);
    layout.left = lanes.values().map(|l| l.head.x).reduce(f32::min).unwrap_or(0.0);
    layout.right = lanes.values().map(|l| l.head.right()).reduce(f32::max).unwrap_or(0.0);
    (layout, edges)
}

/// The icon a lifeline drawn as an icon gets: an actor, or the robustness stereotype of the
/// class it represents.
fn head_icon(model: &Model, lifeline: Id) -> HeadIcon {
    let Some(ElementKind::Lifeline(p)) = model.get(lifeline).map(|e| e.kind()) else {
        return HeadIcon::Box;
    };
    let Some(represented) = p.represents.and_then(|r| model.get(r)) else {
        return HeadIcon::Box;
    };
    if matches!(represented.kind(), ElementKind::Actor(_)) {
        return HeadIcon::Actor;
    }
    let stereotypes = &represented.data().stereotypes;
    let has = |s: &str| stereotypes.iter().any(|t| t.eq_ignore_ascii_case(s));
    if has("boundary") {
        HeadIcon::Boundary
    } else if has("control") {
        HeadIcon::Control
    } else if has("entity") {
        HeadIcon::Entity
    } else if has("actor") {
        HeadIcon::Actor
    } else {
        HeadIcon::Box
    }
}

/// Where a lifeline's dashed line starts, and its name: inside a box head, below an icon.
/// Astah centers the name by its rounded width.
fn lifeline_rows(
    model: &Model,
    lifeline: Id,
    head: Rect,
    icon: HeadIcon,
    measure: &dyn TextMeasure,
) -> (f32, Vec<Row>) {
    let Some(e) = model.get(lifeline) else { return (head.bottom(), Vec::new()) };
    let ElementKind::Lifeline(p) = e.kind() else { return (head.bottom(), Vec::new()) };
    let type_name = p.represents.and_then(|t| model.get(t)).map(|t| t.name());
    let text = match type_name {
        Some(t) => format!("{} : {t}", e.name()),
        None => e.name().to_owned(),
    };
    let w = measure.rounded_width(&text, 12.0, false);
    let h = line_height(12.0);
    let left = head.center().x - w / 2.0;
    let (line_top, row_top) = match icon {
        HeadIcon::Box => (head.bottom(), head.y + ROW_17),
        _ => (head.bottom() + h, head.bottom() + LABEL_TO_ROW),
    };
    let row = Row {
        rect: Rect::new(left, row_top, w, h),
        role: RowRole::Name,
        ..plain_row(text, lifeline)
    };
    (line_top, vec![row])
}

fn plain_row(text: String, element: Id) -> Row {
    Row {
        text,
        rect: Rect::default(),
        size: 12.0,
        bold: false,
        italic: false,
        underline: false,
        align: LabelAlign::Left,
        role: RowRole::Member,
        element: Some(element),
    }
}

/// The tab of a fragment or interaction use: the keyword with 10 on either side.
fn tab(keyword: &str, r: Rect, measure: &dyn TextMeasure) -> Rect {
    let w = measure.width(keyword, 12.0, false);
    Rect::new(r.x, r.y, 2.0 * TAB_INDENT + w, TAB_HEIGHT)
}

fn tab_row(keyword: &str, r: Rect, top: f32, element: Id) -> Row {
    Row {
        rect: Rect::new(r.x + TAB_INDENT, r.y + top, 40.0, line_height(12.0)),
        role: RowRole::Stereotype,
        ..plain_row(keyword.to_owned(), element)
    }
}

/// The frame around the diagram: "sd" in bold and the name in its tab.
fn diagram_frame(
    name: &str,
    r: Rect,
    diagram: Id,
    measure: &dyn TextMeasure,
) -> (Figure, Vec<Row>) {
    let sd = "sd";
    let sd_width = measure.width(sd, 12.0, true);
    let name_width = measure.width(name, 12.0, false);
    // Astah sizes the tab by the name in bold.
    let tab = Rect::new(r.x, r.y, 36.0 + measure.width(name, 12.0, true), TAB_HEIGHT);
    let h = line_height(12.0);
    let kind = Row {
        rect: Rect::new(r.x + TAB_INDENT, r.y + ROW_18, sd_width, h),
        bold: true,
        role: RowRole::Stereotype,
        element: None,
        ..plain_row(sd.to_owned(), diagram)
    };
    let title = Row {
        rect: Rect::new(r.x + TAB_INDENT + sd_width + 3.0, r.y + ROW_18, name_width, h),
        role: RowRole::Name,
        ..plain_row(name.to_owned(), diagram)
    };
    (Figure::Frame { tab }, vec![kind, title])
}

/// Message numbers as Astah gives them: a message sent from a bar is numbered below the
/// message that started the bar ("2.1"), the others count from 1. Messages that branch off the
/// same point share a number with "-A", "-B" appended. Replies have none.
fn numbers(messages: &[Msg], lanes: &HashMap<Id, LaneState>) -> HashMap<Id, String> {
    let mut calls: Vec<&Msg> = messages.iter().filter(|m| m.sort != MessageSort::Reply).collect();
    calls.sort_by(|a, b| a.y.total_cmp(&b.y).then(end(a).total_cmp(&end(b))));
    // The bar each call starts.
    let started: HashMap<Id, Id> = calls
        .iter()
        .filter_map(|m| {
            let bar = lanes[&m.target].bar_starting(end(m))?;
            Some((bar.id, m.presentation))
        })
        .collect();
    let mut parent_of: HashMap<Id, Option<Id>> = HashMap::new();
    for m in &calls {
        let parent = if m.gate == MessageGate::Found {
            None
        } else {
            let own = lanes[&m.target].bar_starting(end(m)).map(|b| b.id);
            let from = lanes[&m.source].bar_at(m.y, own.filter(|_| m.source == m.target));
            from.and_then(|b| started.get(&b.id).copied())
        };
        parent_of.insert(m.presentation, parent);
    }
    let mut numbers: HashMap<Id, String> = HashMap::new();
    let mut counts: HashMap<Option<Id>, usize> = HashMap::new();
    let mut i = 0;
    while i < calls.len() {
        // Calls leaving the same point at the same height branch.
        let first = calls[i];
        let parent = parent_of[&first.presentation];
        let mut j = i + 1;
        while j < calls.len()
            && (calls[j].y - first.y).abs() < 0.5
            && calls[j].source == first.source
            && parent_of[&calls[j].presentation] == parent
            && calls[j].source != calls[j].target
        {
            j += 1;
        }
        let count = counts.entry(parent).or_insert(0);
        *count += 1;
        let base = match parent.and_then(|p| numbers.get(&p)) {
            Some(n) => format!("{}.{count}", n.split('-').next().unwrap_or(n)),
            None => count.to_string(),
        };
        for (k, m) in calls[i..j].iter().enumerate() {
            let number = if j - i > 1 {
                format!("{base}-{}", char::from(b'A' + (k as u8 % 26)))
            } else {
                base.clone()
            };
            numbers.insert(m.presentation, number);
        }
        i = j;
    }
    numbers
}

fn end(m: &Msg) -> f32 {
    m.end_y.unwrap_or(m.y)
}

/// The return values of replies, by the call they answer: the one that started the bar the
/// reply leaves.
fn returns(model: &Model, messages: &[Msg], lanes: &HashMap<Id, LaneState>) -> HashMap<Id, String> {
    let mut returns = HashMap::new();
    for reply in messages.iter().filter(|m| m.sort == MessageSort::Reply) {
        let value = match reply.element.and_then(|e| model.get(e)).map(|e| e.kind()) {
            Some(ElementKind::Message(p)) if !p.return_value.is_empty() => p.return_value.clone(),
            _ => continue,
        };
        let Some(bar) = lanes[&reply.source].bar_at(reply.y, None) else { continue };
        let call = messages.iter().find(|m| {
            m.sort != MessageSort::Reply
                && m.target == reply.source
                && lanes[&m.target].bar_starting(end(m)).is_some_and(|b| b.id == bar.id)
        });
        if let Some(call) = call {
            returns.insert(call.presentation, value);
        }
    }
    returns
}

/// What a message's label shows besides the message itself.
struct Label<'a> {
    number: Option<&'a String>,
    /// The return value of the reply to this call.
    returned: Option<&'a String>,
}

fn message_edge(
    model: &Model,
    m: &Msg,
    lanes: &HashMap<Id, LaneState>,
    frame: Option<Rect>,
    label: Label<'_>,
    layout: &mut SequenceLayout,
) -> EdgeShape {
    let (from, to) = (&lanes[&m.source], &lanes[&m.target]);
    let (y0, y1) = (m.y, end(m));
    let points = match (m.gate, m.gate_x) {
        (MessageGate::Found, Some(gx)) => {
            let from_right = gx > to.center();
            let bar = to.bar_starting(y0).or_else(|| to.bar_at(y0, None));
            vec![Point::new(gx, y0), Point::new(to.side(bar, from_right), y0)]
        }
        (MessageGate::Lost, Some(gx)) => {
            let right = gx >= from.center();
            vec![Point::new(from.side(from.bar_at(y0, None), right), y0), Point::new(gx, y0)]
        }
        _ if m.source == m.target => {
            // Out of the outer bar's right side, down and back into the bar the call starts.
            let own = to.bar_starting(y1);
            let outer = from.bar_at(y0, own.map(|b| b.id));
            let start = from.side(outer, true);
            let y1 = if m.end_y.is_some() { y1 } else { y0 + 20.0 };
            let end = to.side(own.or_else(|| to.bar_at(y1, None)), true);
            let loop_x = start + SELF_WIDTH;
            vec![
                Point::new(start, y0),
                Point::new(loop_x, y0),
                Point::new(loop_x, y1),
                Point::new(end, y1),
            ]
        }
        _ => {
            let right = to.center() >= from.center();
            let start = from.side(from.bar_at(y0, None), right);
            let end = if m.sort == MessageSort::Create {
                if right { to.head.x } else { to.head.right() }
            } else {
                let bar = to.bar_starting(y1).or_else(|| to.bar_at(y1, None));
                to.side(bar, !right)
            };
            if (y1 - y0).abs() > 0.5 {
                let turn = if right { start + BRANCH_RUN } else { start - BRANCH_RUN };
                vec![Point::new(start, y0), Point::new(turn, y1), Point::new(end, y1)]
            } else {
                vec![Point::new(start, y0), Point::new(end, y1)]
            }
        }
    };
    // A gate on the frame's border has no dot.
    if let Some(gx) = m.gate_x {
        let on_frame =
            frame.is_some_and(|f| (gx - f.x).abs() < 1.5 || (gx - f.right()).abs() < 1.5);
        if !on_frame {
            layout.gates.push(Point::new(gx, y0));
        }
    }
    let (line, target_marker) = match m.sort {
        MessageSort::Synchronous | MessageSort::Destroy => {
            (LineStyle::Solid, EndMarker::FilledArrow)
        }
        MessageSort::Asynchronous => (LineStyle::Solid, EndMarker::OpenArrow),
        MessageSort::Reply | MessageSort::Create => (LineStyle::Dashed, EndMarker::OpenArrow),
    };

    let (stereotype, text) =
        m.element.map(|e| message_parts(model, e, m.parameters, false)).unwrap_or_default();
    let guard = match m.element.and_then(|e| model.get(e)).map(|e| e.kind()) {
        Some(ElementKind::Message(p)) if !p.guard.trim().is_empty() => format!("[{}] ", p.guard),
        _ => String::new(),
    };
    let returned = label.returned.map(|v| format!(" : {v}")).unwrap_or_default();
    let text = match label.number {
        Some(n) => format!("{n}: {guard}{text}{returned}"),
        None => format!("{guard}{text}{returned}"),
    };
    let mut labels = Vec::new();
    let n = points.len();
    let (label_top, center) = match m.label {
        Some(offset) => (y0 + offset.y, None),
        None if m.source == m.target && m.gate == MessageGate::None => {
            (y0 - LABEL_ABOVE, Some(points[1].x + SELF_LABEL))
        }
        None => {
            let (a, b) = (points[n - 2], points[n - 1]);
            (b.y - LABEL_ABOVE, Some((a.x + b.x) / 2.0))
        }
    };
    let left_at = m.label.map(|offset| from.center() + offset.x);
    if !text.trim().is_empty() {
        labels.push(match (center, left_at) {
            (Some(x), _) => EdgeLabel {
                text: text.clone(),
                anchor: Point::new(x, label_top),
                align: LabelAlign::Center,
            },
            (None, Some(x)) => EdgeLabel {
                text: text.clone(),
                anchor: Point::new(x, label_top),
                align: LabelAlign::Left,
            },
            (None, None) => unreachable!("one of them is set"),
        });
    }
    if let Some(stereotype) = stereotype {
        let anchor = match labels.first() {
            Some(l) if l.align == LabelAlign::Left => {
                // Centered over a label placed by hand; its width is not known here, so over its
                // start.
                Point::new(l.anchor.x, label_top - STEREOTYPE_ABOVE)
            }
            _ => Point::new(center.unwrap_or_default(), label_top - STEREOTYPE_ABOVE),
        };
        let align = if m.label.is_some() { LabelAlign::Left } else { LabelAlign::Center };
        labels.push(EdgeLabel { text: stereotype, anchor, align });
    }
    EdgeShape {
        id: m.presentation,
        points,
        line,
        source_marker: EndMarker::None,
        target_marker,
        labels,
        fixed: true,
    }
}

/// Gaps between the items of the interaction and its operands, at the items' heights.
fn slots(model: &Model, diagram: Id, view: &View) -> Vec<Slot> {
    let mut slots = Vec::new();
    let Some(interaction) = model.get(diagram).and_then(|d| d.owner()) else { return slots };
    let top_of = |id: Id| -> Option<f32> {
        view.presentations_of(id).find_map(|p| match p.shape {
            Shape::Message { y, .. } => Some(y),
            Shape::Node { bounds } | Shape::Fragment { bounds, .. } => Some(bounds.y),
            _ => None,
        })
    };
    let bottom = view
        .presentations()
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Lifeline { head, length, .. } => Some(head.bottom() + length),
            _ => None,
        })
        .fold(0.0, f32::max);
    fn walk(
        model: &Model,
        owner: Id,
        top_of: &dyn Fn(Id) -> Option<f32>,
        bottom: f32,
        slots: &mut Vec<Slot>,
    ) {
        let children = model.children(owner);
        let mut last = None;
        for (index, &child) in children.iter().enumerate() {
            let Some(y) = top_of(child) else { continue };
            slots.push(Slot { y: y - 10.0, owner, index });
            last = Some(y);
            for &operand in model.children(child) {
                if matches!(
                    model.get(operand).map(|o| o.kind()),
                    Some(ElementKind::InteractionOperand(_))
                ) {
                    walk(model, operand, top_of, bottom, slots);
                }
            }
        }
        let end = last.map_or(bottom - 20.0, |y| y + 20.0);
        slots.push(Slot { y: end, owner, index: children.len() });
    }
    walk(model, interaction, &top_of, bottom, &mut slots);
    slots
}
