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
    ElementKind, Id, InteractionOperator, LineShape, MessageGate, MessageSort, Model,
    ParameterDirection, Point, Rect, Shape, Style, View,
};

use crate::scene::{
    EdgeLabel, EdgeShape, EndMarker, FONT_SIZE, Figure, HeadIcon, LabelAlign, LineStyle, NodeShape,
    Row, RowRole, TextMeasure, line_height,
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
/// How far a message to self comes back below where it leaves.
pub const SELF_DROP: f32 = 20.0;
/// How far right a message that branches off another one runs before it turns.
const BRANCH_RUN: f32 = 20.0;
/// The top of a message's label above its line.
pub const LABEL_ABOVE: f32 = 21.05;
/// The top of a message's stereotype above the top of its label.
const STEREOTYPE_ABOVE: f32 = 16.55;
/// The label of a message to self is centered this far right of the loop.
const SELF_LABEL: f32 = 7.45;
/// The least room between the bar a message to self leaves and the left end of its label.
const SELF_GAP: f32 = 4.0;
/// Height of a frame's tab: one line of text and 10.
pub const TAB_HEIGHT: f32 = 25.093_75;
/// Texts in a tab start this far from the frame's left edge.
const TAB_INDENT: f32 = 10.0;
/// Below a node's top: the top of a text row whose baseline Astah puts 17 or 18 px down.
const ROW_17: f32 = 17.0 - 12.06;
const ROW_18: f32 = 18.0 - 12.06;
/// Edge labels have their baseline 13 below their top, rows 12.06.
const LABEL_TO_ROW: f32 = 13.0 - 12.06;
/// Room a guard keeps from the lifelines and bars beside it.
const GUARD_GAP: f32 = 4.0;
/// Room a message label keeps from a lifeline its line crosses, where it fits between two.
const LINE_GAP: f32 = 4.0;
/// What the arrowhead of a message takes of its line, with a little room: a label keeps off
/// it, or is lifted by [`HEAD_LIFT`] where the line is too short for that.
const HEAD_ROOM: f32 = 13.0;
const HEAD_LIFT: f32 = 3.0;

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

/// A call written as `name(arguments)`: its name, and its arguments if it has parentheses.
pub fn split_call(text: &str) -> (&str, Option<&str>) {
    let text = text.trim();
    match text.split_once('(') {
        Some((name, rest)) => {
            let rest = rest.trim_end();
            (name.trim(), Some(rest.strip_suffix(')').unwrap_or(rest).trim()))
        }
        None => (text, None),
    }
}

/// The label of a message: `name(arguments)` for calls, the return value for replies.
pub fn message_text(model: &Model, message: Id) -> String {
    let (stereotype, text) = message_parts(model, message, &Style::default(), true);
    match stereotype {
        Some(s) => format!("{s} {text}").trim_end().to_owned(),
        None => text,
    }
}

/// A message's stereotypes (with «create» and «destroy» for those messages) and its text. Calls
/// show their arguments in parentheses, as Astah does, unless the presentation's `style` hides
/// them; a call without arguments shows the parameters of its operation instead. A reply shows
/// its name; with `reply_value`, its return value instead if it has one (on the diagram, Astah
/// shows that after the call).
fn message_parts(
    model: &Model,
    message: Id,
    style: &Style,
    reply_value: bool,
) -> (Option<String>, String) {
    let parameters = !style.hide_parameters;
    let Some(e) = model.get(message) else { return (None, String::new()) };
    let ElementKind::Message(p) = e.kind() else { return (None, e.name().to_owned()) };
    let operation = p.operation.and_then(|o| model.get(o)).map(|o| o.name());
    let name = if e.name().is_empty() { operation.unwrap_or_default() } else { e.name() };
    let text = match p.sort {
        MessageSort::Reply if reply_value && !p.return_value.is_empty() => p.return_value.clone(),
        MessageSort::Reply => name.to_owned(),
        // A call without arguments shows the parameters of the operation it calls, as in
        // Astah: `findRegions(b:User)`, with the return type where shown: `… : List<Region>`.
        _ if parameters && p.arguments.is_empty() && p.operation.is_some() => {
            p.operation.map(|o| signature(model, o, name, style)).unwrap_or_default()
        }
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

/// `text` in lines no wider than `room`, broken at spaces and, as Astah does, after a colon
/// that a word follows (`erstelleHitzewarnungen(b:` | `Benutzer)`); a piece wider than `room`
/// stands on a line of its own.
fn break_lines(text: &str, room: f32, measure: &dyn TextMeasure) -> Vec<String> {
    // Each piece with what joins it to the one before: a space, or nothing after a colon.
    let mut pieces: Vec<(&str, &str)> = Vec::new();
    for (i, word) in text.split(' ').enumerate() {
        let mut join = if i == 0 { "" } else { " " };
        let mut rest = word;
        while let Some(at) = rest.find(':').filter(|&at| at + 1 < rest.len()) {
            pieces.push((join, &rest[..=at]));
            join = "";
            rest = &rest[at + 1..];
        }
        pieces.push((join, rest));
    }
    let mut lines: Vec<String> = Vec::new();
    for (join, piece) in pieces {
        match lines.last_mut() {
            Some(line)
                if crate::scene::label_width(&format!("{line}{join}{piece}"), measure) <= room =>
            {
                line.push_str(join);
                line.push_str(piece);
            }
            _ => lines.push(piece.to_owned()),
        }
    }
    lines
}

/// `name(p: Type, q: Type) : Result`, an operation called by a message without arguments.
fn signature(model: &Model, operation: Id, name: &str, style: &Style) -> String {
    let Some(o) = model.get(operation) else { return format!("{name}()") };
    let parameters: Vec<String> = o
        .children()
        .iter()
        .filter_map(|&c| model.get(c))
        .filter_map(|c| match c.kind() {
            ElementKind::Parameter(p) if p.direction != ParameterDirection::Return => {
                Some(match crate::member::type_label(model, &p.type_ref) {
                    Some(t) if !style.hide_parameter_types => format!("{}:{t}", c.name()),
                    _ => c.name().to_owned(),
                })
            }
            _ => None,
        })
        .collect();
    let mut text = format!("{name}({})", parameters.join(", "));
    if let ElementKind::Operation(p) = o.kind()
        && style.show_return_type
        && let Some(t) = crate::member::type_label(model, &p.return_type)
    {
        text.push_str(&format!(" : {t}"));
    }
    text
}

/// An activation bar as laid out.
#[derive(Clone, Copy)]
struct Bar {
    id: Id,
    top: f32,
    bottom: f32,
    left: f32,
    width: f32,
    level: usize,
}

impl Bar {
    fn right(&self) -> f32 {
        self.left + self.width
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
    loop_x: Option<f32>,
    label: Option<Point>,
    style: Style,
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
    let (hide_numbers, flat_numbers) = match model.get(diagram).map(|d| d.kind()) {
        Some(ElementKind::Diagram(p)) => (p.hide_message_numbers, p.flat_message_numbers),
        _ => (false, false),
    };

    // The guards that stand where the fragment puts them, by node and row: not moved by hand.
    let mut placed_guards: Vec<(Id, usize)> = Vec::new();
    for node in nodes.iter_mut() {
        let Some(p) = view.get(node.id) else { continue };
        match &p.shape {
            Shape::Lifeline { length, destroyed, .. } => {
                let Some(lifeline) = p.element else { continue };
                let icon = if p.style.iconic { head_icon(model, lifeline) } else { HeadIcon::Box };
                let head = node.rect;
                let (line_top, rows) =
                    lifeline_rows(model, lifeline, &p.style, head, icon, measure);
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
                // Astah writes the fragment's name (`loop : name`) and a loop's condition into
                // the tab.
                let in_tab = props.operator == InteractionOperator::Loop;
                let mut keyword = operator_keyword(props.operator).to_owned();
                if !e.name().trim().is_empty() {
                    keyword = format!("{keyword} : {}", e.name());
                }
                if let Some(guard) = operands.first().and_then(|&o| guard_of(o)).filter(|_| in_tab)
                {
                    keyword = format!("{keyword} {guard}");
                }
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
                    let by_hand = guards.get(k).copied().flatten();
                    if by_hand.is_none() {
                        placed_guards.push((node.id, rows.len()));
                    }
                    let moved = by_hand.unwrap_or_default();
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
            Shape::Node { .. } => {
                let Some(e) = p.element.and_then(|e| model.get(e)) else { continue };
                match e.kind() {
                    ElementKind::StateInvariant(_) => {
                        (node.rect, node.rows) = invariant(e.id(), e.name(), node.rect, measure);
                        node.figure = Figure::StateInvariant;
                    }
                    ElementKind::Termination(_) => {
                        node.figure = Figure::Termination;
                        node.rows = Vec::new();
                    }
                    ElementKind::DurationConstraint(c) => {
                        let y_of = |m: Id| {
                            view.presentations_of(m).find_map(|p| match p.shape {
                                Shape::Message { y, .. } => Some(y),
                                _ => None,
                            })
                        };
                        let r = node.rect;
                        let (top, bottom) = match (y_of(c.from), y_of(c.to)) {
                            (Some(a), Some(b)) => (a.min(b), a.max(b)),
                            _ => (r.y, r.bottom()),
                        };
                        let x = r.center().x;
                        node.figure = Figure::Duration { x, top, bottom };
                        let anchor = ConstraintAnchor::Duration { x, middle: (top + bottom) / 2.0 };
                        node.rows = constraint_row(e, anchor, p.style.name_offset, measure);
                        let line = Rect::new(x - BAR, top, 2.0 * BAR, bottom - top);
                        node.rect = node.rows.iter().fold(line, |a, row| a.union(&row.rect));
                    }
                    // Placed once the messages are laid out.
                    ElementKind::TimeConstraint(_) => {
                        node.figure =
                            Figure::TimeTick { from: node.rect.origin(), to: node.rect.origin() };
                        node.rows = Vec::new();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Bars, with x from their lifeline and from the bars they lie within.
    let mut bars: Vec<(Id, Id, f32, f32, f32)> = view
        .presentations()
        .iter()
        .filter_map(|p| match p.shape {
            Shape::Activation { lifeline, top, height, width } if lanes.contains_key(&lifeline) => {
                Some((p.id, lifeline, top, top + height, width.unwrap_or(BAR)))
            }
            _ => None,
        })
        .collect();
    bars.sort_by(|a, b| a.2.total_cmp(&b.2).then(b.3.total_cmp(&a.3)));
    for &(id, lane, top, bottom, width) in &bars {
        let state = lanes.get_mut(&lane).expect("filtered above");
        let level = state.bars.iter().filter(|b| b.top <= top && bottom <= b.bottom).count();
        let left = state.center() - width / 2.0 + level as f32 * BAR;
        state.bars.push(Bar { id, top, bottom, left, width, level });
    }
    for node in nodes.iter_mut() {
        if let Some(Shape::Activation { lifeline, .. }) = view.get(node.id).map(|p| &p.shape)
            && let Some(bar) =
                lanes.get(lifeline).and_then(|l| l.bars.iter().find(|b| b.id == node.id))
        {
            node.rect = Rect::new(bar.left, bar.top, bar.width, bar.bottom - bar.top);
            node.figure = Figure::Activation;
            node.rows = Vec::new();
        }
    }
    // A guard keeps off the lifelines and bars that pass beneath it: it starts right of the
    // first lifeline it would run into, or of the next one that leaves it room.
    for (id, k) in placed_guards {
        let Some(node) = nodes.iter_mut().find(|n| n.id == id) else { continue };
        let frame = node.rect;
        let Some(row) = node.rows.get_mut(k) else { continue };
        let (top, bottom) = (row.rect.y - GUARD_GAP, row.rect.bottom() + GUARD_GAP);
        // What each lifeline inside the frame takes up at the guard's height, from the left.
        let mut taken: Vec<(f32, f32)> = lanes
            .values()
            .filter(|l| frame.x < l.center() && l.center() < frame.right())
            .filter(|l| l.head.y < bottom)
            .map(|l| {
                let bars = l.bars.iter().filter(|b| b.top < bottom && b.bottom > top);
                bars.fold((l.center(), l.center()), |(lo, hi), b| {
                    (lo.min(b.left), hi.max(b.right()))
                })
            })
            .collect();
        taken.sort_by(|a, b| a.0.total_cmp(&b.0));
        let width = row.rect.width;
        let clear =
            |x: f32| taken.iter().all(|&(l, r)| r + GUARD_GAP <= x || x + width + GUARD_GAP <= l);
        if clear(row.rect.x) {
            continue;
        }
        let starts: Vec<f32> = taken.iter().map(|&(_, r)| r + GUARD_GAP).collect();
        let fits = |x: &&f32| **x > row.rect.x && clear(**x) && **x + width <= frame.right();
        let first = starts.iter().find(|x| **x > row.rect.x);
        if let Some(&x) = starts.iter().find(fits).or(first) {
            row.rect.x = x;
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
            rect: Rect::new(bar.left, bar.top, bar.width, bar.bottom - bar.top),
            figure: Figure::Activation,
            rows: Vec::new(),
            fill: p.style.fill,
        });
    }
    // Keep the drawing order of the presentations: bars come right after what precedes them.
    // Combined fragments go on top, see-through: their frame, tab and text stay visible where
    // lifelines and bars cross them, and so do the lifelines and bars.
    let order: HashMap<Id, usize> =
        view.presentations().iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    nodes.sort_by_key(|n| {
        let combined = matches!(n.figure, Figure::Fragment { filled: false, .. });
        (combined, order.get(&n.id).copied().unwrap_or(usize::MAX))
    });

    let frame =
        view.presentations().iter().find(|p| p.element == Some(diagram)).and_then(|p| p.bounds());
    let messages: Vec<Msg> = view
        .presentations()
        .iter()
        .filter_map(|p| {
            let Shape::Message { source, target, y, end_y, gate_x, loop_x, labels } = p.shape
            else {
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
                loop_x,
                label: labels.name,
                style: p.style.clone(),
                sort: props.map_or(MessageSort::Synchronous, |m| m.sort),
                gate: props.map_or(MessageGate::None, |m| m.gate),
            })
        })
        .collect();
    let numbers =
        if hide_numbers { HashMap::new() } else { numbers(&messages, &lanes, flat_numbers) };
    let hide_returns = matches!(
        model.get(diagram).map(|d| d.kind()),
        Some(ElementKind::Diagram(p)) if p.hide_return_values
    );
    let returns = if hide_returns { HashMap::new() } else { returns(model, &messages, &lanes) };
    let edges: Vec<EdgeShape> = messages
        .iter()
        .map(|m| {
            let label = Label {
                number: numbers.get(&m.presentation),
                returned: returns.get(&m.presentation),
            };
            message_edge(model, m, &lanes, frame, label, measure, &mut layout)
        })
        .collect();
    time_constraints(model, view, &edges, nodes, measure);

    layout.slots = slots(model, diagram, view);
    layout.left = lanes.values().map(|l| l.head.x).reduce(f32::min).unwrap_or(0.0);
    layout.right = lanes.values().map(|l| l.head.right()).reduce(f32::max).unwrap_or(0.0);
    (layout, edges)
}

/// A duration constraint's label: Astah centers it this far right of its line.
pub const DURATION_LABEL: f32 = 9.0;
/// A duration constraint's label starts no further left than this right of its line, so that a
/// label wider than Astah's `{d}` stays beside the arrow instead of crossing it.
pub const DURATION_GAP: f32 = 0.5;
/// Constraint labels are as high as Astah's edge labels and centered on their point.
pub const CONSTRAINT_LABEL: f32 = 15.093_75;
/// A state invariant's text: if it fits on one line, each of its lines centered, the first
/// baseline 15.52 below the top; otherwise lines 10 in from the left, the first baseline 14.58
/// below the top. One line per 15.09.
const INVARIANT_BASELINE: f32 = 15.52;
const INVARIANT_FIRST_BASELINE: f32 = 14.58;
const INVARIANT_PAD: f32 = 10.0;

/// `text` broken into lines as Java's `LineBreakMeasurer` breaks a state invariant for Astah:
/// at its line breaks (an empty line counts), then before a word that would make the line wider
/// than `room`; a word wider than `room` breaks between characters.
fn invariant_lines(text: &str, room: f32, measure: &dyn TextMeasure) -> Vec<String> {
    let fits = |s: &str| measure.width(s.trim_end(), FONT_SIZE, false) <= room;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut rest = paragraph.strip_suffix('\r').unwrap_or(paragraph);
        if rest.is_empty() {
            lines.push(String::new());
        }
        while !rest.is_empty() {
            // Where a line may end: after a run of spaces, or at the end.
            let ends = rest
                .char_indices()
                .zip(rest.chars().skip(1))
                .filter(|&((_, c), next)| c == ' ' && next != ' ')
                .map(|((i, _), _)| i + 1)
                .chain([rest.len()]);
            let mut end = None;
            for at in ends {
                if !fits(&rest[..at]) {
                    break;
                }
                end = Some(at);
            }
            // A word wider than the line: as many characters as fit, at least one.
            let end = end.unwrap_or_else(|| {
                let mut chars = rest.char_indices().skip(1).map(|(i, _)| i).chain([rest.len()]);
                let first = chars.next().unwrap_or(rest.len());
                chars.take_while(|&at| fits(&rest[..at])).last().unwrap_or(first)
            });
            lines.push(rest[..end].trim_end().to_owned());
            rest = &rest[end..];
        }
    }
    lines
}

/// The height Astah gives a state invariant `width` wide that reads `text`.
pub fn invariant_height(text: &str, width: f32, measure: &dyn TextMeasure) -> f32 {
    let lines = invariant_lines(text, width - 2.0 * INVARIANT_PAD, measure);
    crate::sequence_edit::invariant_height(lines.len())
}

/// A state invariant in `r`: its condition broken into lines as wide as the box leaves room
/// for, and the box, grown to hold them. A condition as narrow as the room, without its line
/// breaks, has each of its lines centered; a wider one is left-aligned.
fn invariant(id: Id, text: &str, r: Rect, measure: &dyn TextMeasure) -> (Rect, Vec<Row>) {
    let room = r.width - 2.0 * INVARIANT_PAD;
    let lines = if text.is_empty() { Vec::new() } else { invariant_lines(text, room, measure) };
    let needed = crate::sequence_edit::invariant_height(lines.len());
    let rect = Rect::new(r.x, r.y, r.width, r.height.max(needed));
    let centered = measure.width(&text.replace(['\r', '\n'], ""), FONT_SIZE, false) <= room;
    let h = line_height(FONT_SIZE);
    let rows = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let w = measure.width(&line, FONT_SIZE, false);
            let below = i as f32 * crate::sequence_edit::INVARIANT_LINE;
            let (x, baseline) = if centered {
                (r.center().x - w / 2.0, r.y + INVARIANT_BASELINE + below)
            } else {
                (r.x + INVARIANT_PAD, r.y + INVARIANT_FIRST_BASELINE + below)
            };
            Row {
                rect: Rect::new(x, baseline - crate::draw::baseline(FONT_SIZE), w, h),
                role: RowRole::Name,
                ..plain_row(line, id)
            }
        })
        .collect();
    (rect, rows)
}

/// Where the label of a duration or time constraint goes by default.
#[derive(Clone, Copy, Debug)]
pub enum ConstraintAnchor {
    /// A duration's line at `x`, `middle` halfway down it: the label is centered 9 right of it,
    /// as Astah's, but never crosses the line.
    Duration { x: f32, middle: f32 },
    /// The end of a time constraint's line: the label starts there, or ends there when the line
    /// points left.
    Time { end: Point, left: bool },
}

/// The box of a constraint's label `width` wide: where `anchor` puts it, moved by `offset`
/// (placed by hand). It is as high as an edge label, centered on the anchor's height.
pub fn constraint_label_box(anchor: ConstraintAnchor, width: f32, offset: Option<Point>) -> Rect {
    let (left, y) = match anchor {
        ConstraintAnchor::Duration { x, middle } => {
            ((x + DURATION_LABEL - width / 2.0).max(x + DURATION_GAP), middle)
        }
        ConstraintAnchor::Time { end, left: false } => (end.x, end.y),
        ConstraintAnchor::Time { end, left: true } => (end.x - width, end.y),
    };
    let by = offset.unwrap_or_default();
    Rect::new(left + by.x, y - CONSTRAINT_LABEL / 2.0 + by.y, width, CONSTRAINT_LABEL)
}

/// The text of a constraint's label, `{constraint}`; none for an empty constraint.
pub fn constraint_text(e: &umlcraft_model::Element) -> Option<String> {
    (!e.name().trim().is_empty()).then(|| format!("{{{}}}", e.name()))
}

/// The label of a duration or time constraint `e` where `anchor` and `offset` put it.
fn constraint_row(
    e: &umlcraft_model::Element,
    anchor: ConstraintAnchor,
    offset: Option<Point>,
    measure: &dyn TextMeasure,
) -> Vec<Row> {
    let Some(text) = constraint_text(e) else { return Vec::new() };
    let w = measure.width(&text, FONT_SIZE, false);
    let label = constraint_label_box(anchor, w, offset);
    vec![Row {
        rect: Rect::new(label.x, label.y + LABEL_TO_ROW, w, line_height(FONT_SIZE)),
        role: RowRole::Name,
        ..plain_row(text, e.id())
    }]
}

/// Puts time constraints where Astah does: a line going on 30 beyond their message's
/// arrowhead, the label after its end.
fn time_constraints(
    model: &Model,
    view: &View,
    edges: &[EdgeShape],
    nodes: &mut [NodeShape],
    measure: &dyn TextMeasure,
) {
    for node in nodes.iter_mut() {
        let Some(e) = node.element.and_then(|e| model.get(e)) else { continue };
        let ElementKind::TimeConstraint(c) = e.kind() else { continue };
        let edge = view
            .presentations_of(c.message)
            .find(|p| matches!(p.shape, Shape::Message { .. }))
            .and_then(|p| edges.iter().find(|edge| edge.id == p.id));
        let Some(edge) = edge else { continue };
        let n = edge.points.len();
        let (tip, before) = (edge.points[n - 1], edge.points[n - 2]);
        let dir = if tip.x < before.x { -1.0 } else { 1.0 };
        let end = tip.offset(dir * crate::sequence_edit::TIME_TICK, 0.0);
        node.figure = Figure::TimeTick { from: tip, to: end };
        let anchor = ConstraintAnchor::Time { end, left: dir < 0.0 };
        let offset = view.get(node.id).and_then(|p| p.style.name_offset);
        node.rows = constraint_row(e, anchor, offset, measure);
        let line = Rect::from_corners(tip, end).inflate(BAR / 2.0);
        node.rect = node.rows.iter().fold(line, |a, row| a.union(&row.rect));
    }
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
    style: &Style,
    head: Rect,
    icon: HeadIcon,
    measure: &dyn TextMeasure,
) -> (f32, Vec<Row>) {
    let Some(e) = model.get(lifeline) else { return (head.bottom(), Vec::new()) };
    let ElementKind::Lifeline(p) = e.kind() else { return (head.bottom(), Vec::new()) };
    // A bound class shows what it binds: `list : List<Order>`. Name and classifier may be
    // hidden.
    let type_name = p.represents.filter(|&t| model.get(t).is_some() && !style.hide_instance_type);
    let name = if style.hide_instance_name { "" } else { e.name() };
    let text = match type_name {
        Some(t) => format!("{name} : {}", crate::member::display_name(model, t)),
        None => name.to_owned(),
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

/// Room a lifeline's head keeps on either side of its name.
const HEAD_PADDING: f32 = 10.0;

/// The width the head box of `lifeline` needs for its name, [`HEAD_WIDTH`] at least, and the
/// text it shows; `None` for a lifeline drawn as an icon.
pub(crate) fn fitted_head(
    model: &Model,
    lifeline: Id,
    style: &Style,
    head: Rect,
    measure: &dyn TextMeasure,
) -> Option<(f32, String)> {
    if style.iconic {
        return None;
    }
    let (_, rows) = lifeline_rows(model, lifeline, style, head, HeadIcon::Box, measure);
    let row = rows.into_iter().next()?;
    Some(((row.rect.width + 2.0 * HEAD_PADDING).max(HEAD_WIDTH), row.text))
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
        hinted: false,
        color: None,
    }
}

/// The tab of a fragment or interaction use: the keyword with 10 on either side.
fn tab(keyword: &str, r: Rect, measure: &dyn TextMeasure) -> Rect {
    // A loop's tab with its condition is 3 wider than others.
    let extra = if keyword.starts_with("loop [") { 3.0 } else { 0.0 };
    Rect::new(r.x, r.y, 2.0 * TAB_INDENT + wide(keyword, measure) + extra, TAB_HEIGHT)
}

/// The larger of a plain text's whole-pixel and exact width, by which Astah sizes tabs.
fn wide(text: &str, measure: &dyn TextMeasure) -> f32 {
    measure.astah_width(text, FONT_SIZE, false)
}

fn tab_row(keyword: &str, r: Rect, top: f32, element: Id) -> Row {
    Row {
        rect: Rect::new(r.x + TAB_INDENT, r.y + top, 40.0, line_height(12.0)),
        role: RowRole::Stereotype,
        hinted: true,
        ..plain_row(keyword.to_owned(), element)
    }
}

/// The frame around the diagram: "sd" in bold and the name in its tab. "sd" is 14 wide, the
/// name 3 apart from it.
fn diagram_frame(
    name: &str,
    r: Rect,
    diagram: Id,
    measure: &dyn TextMeasure,
) -> (Figure, Vec<Row>) {
    let offset = TAB_INDENT + measure.width("sd", 12.0, true) + 3.0;
    frame_with_tab("sd", offset, name, r, diagram, measure)
}

/// A diagram frame with `keyword` in bold and the name `name_offset` from the left in its tab,
/// which ends 10 after the name. Astah writes "pkg" frames this way with the name at 33.
pub(crate) fn frame_with_tab(
    keyword: &str,
    name_offset: f32,
    name: &str,
    r: Rect,
    diagram: Id,
    measure: &dyn TextMeasure,
) -> (Figure, Vec<Row>) {
    let tab = Rect::new(r.x, r.y, name_offset + wide(name, measure) + TAB_INDENT, TAB_HEIGHT);
    let h = line_height(12.0);
    let kind = Row {
        rect: Rect::new(r.x + TAB_INDENT, r.y + ROW_18, measure.width(keyword, 12.0, true), h),
        bold: true,
        role: RowRole::Stereotype,
        element: None,
        ..plain_row(keyword.to_owned(), diagram)
    };
    let title = Row {
        rect: Rect::new(r.x + name_offset, r.y + ROW_18, measure.width(name, 12.0, false), h),
        role: RowRole::Name,
        ..plain_row(name.to_owned(), diagram)
    };
    (Figure::Frame { tab }, vec![kind, title])
}

/// Message numbers as Astah gives them: a message sent from a bar is numbered below the
/// message that started the bar ("2.1"), the others count from 1. Messages that branch off the
/// same point share a number with "-A", "-B" appended. Replies have none. `flat` counts all
/// calls from top to bottom instead.
fn numbers(messages: &[Msg], lanes: &HashMap<Id, LaneState>, flat: bool) -> HashMap<Id, String> {
    let mut calls: Vec<&Msg> = messages.iter().filter(|m| m.sort != MessageSort::Reply).collect();
    calls.sort_by(|a, b| a.y.total_cmp(&b.y).then(end(a).total_cmp(&end(b))));
    if flat {
        return calls
            .iter()
            .enumerate()
            .map(|(i, m)| (m.presentation, (i + 1).to_string()))
            .collect();
    }
    // The bar each call starts; of two calls arriving at one bar's top, the first in order.
    let mut started: HashMap<Id, Id> = HashMap::new();
    for m in &calls {
        if let Some(bar) = lanes[&m.target].bar_starting(end(m)) {
            started.entry(bar.id).or_insert(m.presentation);
        }
    }
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
        // Calls leaving the same point at the same height branch; a message to self never does,
        // whichever of them comes first.
        let first = calls[i];
        let parent = parent_of[&first.presentation];
        let mut j = i + 1;
        while j < calls.len()
            && first.source != first.target
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

/// Where a message label `width` wide goes centered on its line from `span.0` to `span.1`:
/// at `center`, or, where it would lie on a bar of a lifeline the line crosses (`bars`, from
/// left to right side), in the middle of the widest stretch of the line those bars leave free.
fn clear_of_bars(center: f32, width: f32, span: (f32, f32), mut bars: Vec<(f32, f32)>) -> f32 {
    let half = width / 2.0;
    if bars.iter().all(|&(l, r)| r <= center - half || l >= center + half) {
        return center;
    }
    bars.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut free = Vec::new();
    let mut from = span.0;
    for (l, r) in bars {
        free.push((from, l));
        from = from.max(r);
    }
    free.push((from, span.1));
    free.into_iter()
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
        .map_or(center, |(l, r)| (l + r) / 2.0)
}

/// Where a message label `width` wide goes on its line from `span.0` to `span.1` to keep off
/// `taken` (stretches of the line, from left to right side): at `center` where it is clear
/// there, else as near to it as it fits between them, toward `source` if both ways are as far.
/// None where it fits nowhere.
fn between(
    center: f32,
    width: f32,
    span: (f32, f32),
    source: f32,
    mut taken: Vec<(f32, f32)>,
) -> Option<f32> {
    let half = width / 2.0;
    if taken.iter().all(|&(l, r)| r <= center - half || l >= center + half) {
        return Some(center);
    }
    taken.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut free = Vec::new();
    let mut from = span.0;
    for (l, r) in taken {
        free.push((from, l));
        from = from.max(r);
    }
    free.push((from, span.1));
    free.into_iter()
        .filter(|&(l, r)| r - l >= width)
        .map(|(l, r)| center.clamp(l + half, r - half))
        .min_by(|a, b| {
            let off = |x: &f32| ((x - center).abs() * 2.0).round();
            off(a).total_cmp(&off(b)).then((a - source).abs().total_cmp(&(b - source).abs()))
        })
}

fn message_edge(
    model: &Model,
    m: &Msg,
    lanes: &HashMap<Id, LaneState>,
    frame: Option<Rect>,
    label: Label<'_>,
    measure: &dyn TextMeasure,
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
            let y1 = if m.end_y.is_some() { y1 } else { y0 + SELF_DROP };
            let end = to.side(own.or_else(|| to.bar_at(y1, None)), true);
            let loop_x = match m.loop_x {
                Some(x) => (from.center() + x).max(start + BAR),
                None => start + SELF_WIDTH,
            };
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
        m.element.map(|e| message_parts(model, e, &m.style, false)).unwrap_or_default();
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
    let width = [Some(&text), stereotype.as_ref()]
        .into_iter()
        .flatten()
        .map(|t| measure.width(t, FONT_SIZE, false))
        .fold(0.0, f32::max);
    let (label_top, center) = match m.label {
        Some(offset) => (y0 + offset.y, None),
        // Over the loop, but a long text no further left than where the loop leaves its bar:
        // the bar would run through it.
        None if m.source == m.target && m.gate == MessageGate::None => {
            let center = (points[1].x + SELF_LABEL).max(points[0].x + SELF_GAP + width / 2.0);
            (y0 - LABEL_ABOVE, Some(center))
        }
        None => {
            let (a, b) = (points[n - 2], points[n - 1]);
            let top = b.y - LABEL_ABOVE;
            let above = if stereotype.is_some() { STEREOTYPE_ABOVE } else { 0.0 };
            let span = (a.x.min(b.x), a.x.max(b.x));
            let bars: Vec<(f32, f32)> = lanes
                .iter()
                .filter(|(id, _)| **id != m.source && **id != m.target)
                .filter(|(_, l)| span.0 < l.center() && l.center() < span.1)
                .flat_map(|(_, l)| &l.bars)
                .filter(|b| b.top < top + line_height(FONT_SIZE) && b.bottom > top - above)
                .map(|b| (b.left, b.left + b.width))
                .collect();
            // Off the lifelines the line crosses and off the arrowhead, where the label fits
            // between them; a label too long for that covers the lifelines, and goes up a
            // little where it reaches over the arrowhead.
            let crossed = lanes
                .iter()
                .filter(|(id, _)| **id != m.source && **id != m.target)
                .map(|(_, l)| l.center())
                .filter(|&x| span.0 < x && x < span.1)
                .map(|x| (x - LINE_GAP, x + LINE_GAP));
            let head = if b.x >= a.x { (b.x - HEAD_ROOM, b.x) } else { (b.x, b.x + HEAD_ROOM) };
            let taken: Vec<(f32, f32)> =
                bars.iter().copied().chain(crossed).chain([head]).collect();
            let middle = (a.x + b.x) / 2.0;
            match between(middle, width + 2.0, span, a.x, taken) {
                Some(center) => (top, Some(center)),
                None => {
                    let center = clear_of_bars(middle, width, span, bars);
                    let half = width / 2.0 + 1.0;
                    let over_head = center + half > head.0 && center - half < head.1;
                    (if over_head { top - HEAD_LIFT } else { top }, Some(center))
                }
            }
        }
    };
    let left_at = m.label.map(|offset| from.center() + offset.x);
    if !text.trim().is_empty() {
        labels.push(match (center, left_at) {
            (Some(x), _) => EdgeLabel {
                text: text.clone(),
                anchor: Point::new(x, label_top),
                align: LabelAlign::Center,
                underline: false,
                keyword: false,
                color: None,
            },
            (None, Some(x)) => EdgeLabel {
                text: text.clone(),
                anchor: Point::new(x, label_top),
                align: LabelAlign::Left,
                underline: false,
                keyword: false,
                color: None,
            },
            (None, None) => unreachable!("one of them is set"),
        });
    }
    // Astah breaks the label of a found message at spaces where it would run past the lifeline
    // it calls; the last line stays where the whole label would be.
    if m.gate == MessageGate::Found
        && let Some(first) = labels.first().filter(|l| l.align == LabelAlign::Left).cloned()
    {
        let room = to.center() - first.anchor.x;
        let lines = break_lines(&first.text, room, measure);
        if lines.len() > 1 {
            let count = lines.len();
            labels.splice(
                0..1,
                lines.into_iter().enumerate().map(|(i, text)| EdgeLabel {
                    text,
                    anchor: first
                        .anchor
                        .offset(0.0, -(line_height(FONT_SIZE) * (count - 1 - i) as f32)),
                    ..first.clone()
                }),
            );
        }
    }
    if let Some(stereotype) = stereotype {
        let (x, above) = match (labels.first(), left_at) {
            // Centered over a label placed by hand.
            (Some(l), _) if l.align == LabelAlign::Left => {
                (l.anchor.x + crate::scene::label_width(&l.text, measure) / 2.0, STEREOTYPE_ABOVE)
            }
            // Without a text, in the label's place: where it was placed, or centered.
            (None, Some(x)) => (x, 0.0),
            (None, None) => (center.unwrap_or_default(), 0.0),
            _ => (center.unwrap_or_default(), STEREOTYPE_ABOVE),
        };
        labels.push(EdgeLabel {
            text: stereotype,
            anchor: Point::new(x, label_top - above),
            align: LabelAlign::Center,
            underline: false,
            keyword: false,
            color: None,
        });
    }
    // Astah centers a label's text by Java's whole-pixel width, mostly a little wider than the
    // exact one. (Labels placed by hand carry that from the import.)
    for label in labels.iter_mut().filter(|l| l.align == LabelAlign::Center) {
        let exact = measure.width(&label.text, FONT_SIZE, false);
        let rounded = measure.rounded_width(&label.text, FONT_SIZE, false);
        label.anchor.x -= (rounded - exact).max(0.0) / 2.0;
    }
    let node_tips = [points[0], points[points.len() - 1]];
    EdgeShape {
        id: m.presentation,
        points,
        node_tips,
        line,
        shape: LineShape::Line,
        source_marker: EndMarker::None,
        target_marker,
        labels,
        fixed: true,
        ends: [Rect::default(); 2],
        anchors: [None; 2],
        name_arrow: None,
        qualifiers: Vec::new(),
        free_labels: Vec::new(),
        association: false,
        end_labels: Vec::new(),
        constraint_labels: Vec::new(),
        stroke: None,
        double: false,
    }
}

/// Gaps between the items of the interaction and its operands, at the items' heights.
fn slots(model: &Model, diagram: Id, view: &View) -> Vec<Slot> {
    let mut slots = Vec::new();
    let Some(interaction) = model.get(diagram).and_then(|d| d.owner()) else { return slots };
    let top_of = |id: Id| -> Option<f32> {
        view.presentations_of(id).find_map(|p| match p.shape {
            Shape::Message { y, .. } => Some(y),
            Shape::Node { bounds, .. } | Shape::Fragment { bounds, .. } => Some(bounds.y),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_move_off_the_bars_their_line_crosses() {
        // Clear of the bar, the label stays in the middle.
        assert_eq!(clear_of_bars(300.0, 100.0, (100.0, 500.0), vec![(390.0, 400.0)]), 300.0);
        // On it, the label goes into the wider stretch the bar leaves free.
        assert_eq!(clear_of_bars(300.0, 100.0, (100.0, 500.0), vec![(280.0, 290.0)]), 395.0);
        assert_eq!(clear_of_bars(300.0, 100.0, (100.0, 500.0), vec![(310.0, 320.0)]), 205.0);
    }

    fn lane(x: f32, bars: &[(u128, f32, f32)]) -> LaneState {
        let bars = bars
            .iter()
            .map(|&(id, top, bottom)| Bar {
                id: Id::from_u128(id),
                top,
                bottom,
                left: x - 5.0,
                width: 10.0,
                level: 0,
            })
            .collect();
        LaneState { head: Rect::new(x - 40.0, 40.0, 80.0, 40.0), bars }
    }

    fn msg(id: u128, source: Id, target: Id, y: f32, end_y: Option<f32>) -> Msg {
        Msg {
            presentation: Id::from_u128(id),
            element: None,
            source,
            target,
            y,
            end_y,
            gate_x: None,
            loop_x: None,
            label: None,
            style: Style::default(),
            sort: MessageSort::Synchronous,
            gate: MessageGate::None,
        }
    }

    #[test]
    fn numbers_take_the_first_call_into_a_bar_and_never_branch_a_message_to_self() {
        let (a, b, c) = (Id::from_u128(1), Id::from_u128(2), Id::from_u128(3));
        let lanes = HashMap::from([
            (a, lane(100.0, &[(10, 100.0, 200.0), (11, 150.0, 175.0)])),
            (b, lane(300.0, &[(20, 100.0, 200.0)])),
            (c, lane(500.0, &[(30, 100.0, 150.0)])),
        ]);
        let messages = [
            // Two calls arrive at the top of b's bar; the first numbers what b sends from it.
            msg(100, a, b, 100.0, None),
            msg(101, c, b, 100.0, None),
            msg(102, b, c, 120.0, None),
            // A message to self beside a call from the same point: no "-A" / "-B".
            msg(103, a, b, 130.0, None),
            msg(104, a, a, 130.0, Some(150.0)),
        ];
        let numbers = numbers(&messages, &lanes, false);
        let number = |id: u128| numbers[&Id::from_u128(id)].clone();
        assert_eq!(number(102), "1.1");
        assert!(!number(103).contains('-') && !number(104).contains('-'), "{numbers:?}");
    }
}
