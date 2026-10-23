//! Sequence diagrams: lifelines stand side by side, messages run from top to bottom in the order
//! of the interaction. Vertical positions are never stored; they follow from that order, so
//! inserting or moving a message makes everything below move along.
//!
//! - A lifeline is a node presentation; only its x position and width count. All heads share
//!   one top line, except lifelines created by a message, whose head sits at that message.
//! - A message is an edge presentation from the sender's lifeline node to the receiver's.
//! - A combined fragment is a node presentation; its frame follows from the lifelines it covers
//!   and from what its operands contain. An interaction use ("ref") is a frame of one row.
//! - Found and lost messages start or end at a dot beside their one lifeline.
//!
//! Only messages and fragments shown on the diagram take up space.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    ElementKind, Id, InteractionOperator, MessageGate, MessageSort, Model, Point, Rect, Shape, View,
};

use crate::scene::{
    EdgeLabel, EdgeShape, EndMarker, Figure, LABEL_HEIGHT, LabelAlign, LineStyle, NodeShape, Row,
    RowRole, TextMeasure, middle_label,
};

/// Height of a lifeline's head box.
pub const HEAD_HEIGHT: f32 = 32.0;
/// Space between the heads and the first message.
const FIRST_GAP: f32 = 24.0;
/// Vertical space taken by one message.
const ROW: f32 = 36.0;
/// Height of the loop drawn for a message to the same lifeline.
const SELF_LOOP: f32 = 18.0;
/// How far a self message loops out to the right.
const SELF_WIDTH: f32 = 30.0;
/// Width of an activation bar; nested activations are offset by half of it.
const BAR: f32 = 10.0;
/// Length of the lifelines below the last message.
const TAIL: f32 = 40.0;
/// Space above the first operand's content: the operator tab and the guard.
const FRAGMENT_HEADER: f32 = 38.0;
/// Space above the content of further operands: the guard below the separator.
const OPERAND_HEADER: f32 = 20.0;
/// Height of an operand without content.
const EMPTY_OPERAND: f32 = 18.0;
/// How far a frame reaches beyond the heads of the lifelines it covers.
const FRAME_MARGIN: f32 = 10.0;
/// Height of an interaction use's frame.
const REF_HEIGHT: f32 = 44.0;
/// Length of a found or lost message, from its lifeline to the dot.
const GATE_LENGTH: f32 = 90.0;
/// Radius of the dot at the open end of a found or lost message.
pub const GATE_DOT: f32 = 4.0;

/// What the sequence layout adds to a scene besides nodes and edges.
#[derive(Clone, Debug, Default)]
pub struct SequenceLayout {
    pub lanes: Vec<Lane>,
    /// Activation bars ("execution specifications").
    pub activations: Vec<Rect>,
    /// Where lifelines end in a cross, after a destroy message.
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
    /// The slot nearest to height `y`.
    pub fn slot_at(&self, y: f32) -> Option<Slot> {
        self.slots.iter().copied().min_by(|a, b| (a.y - y).abs().total_cmp(&(b.y - y).abs()))
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
    let Some(e) = model.get(message) else { return String::new() };
    let ElementKind::Message(p) = e.kind() else { return e.name().to_owned() };
    let operation = p.operation.and_then(|o| model.get(o)).map(|o| o.name());
    let name = if e.name().is_empty() { operation.unwrap_or_default() } else { e.name() };
    let text = match p.sort {
        MessageSort::Reply if !p.return_value.is_empty() => p.return_value.clone(),
        MessageSort::Reply => name.to_owned(),
        MessageSort::Create | MessageSort::Destroy if name.is_empty() && p.arguments.is_empty() => {
            let keyword = if p.sort == MessageSort::Create { "create" } else { "destroy" };
            format!("<<{keyword}>>")
        }
        _ if !p.arguments.is_empty() || (operation.is_some() && !name.is_empty()) => {
            format!("{name}({})", p.arguments)
        }
        _ => name.to_owned(),
    };
    match e.data().stereotypes.join(", ") {
        s if s.is_empty() => text,
        s => format!("<<{s}>> {text}").trim_end().to_owned(),
    }
}

struct LaneState {
    lifeline: Id,
    head: Rect,
}

/// An activation that has not ended yet.
struct Open {
    lane: Id,
    level: usize,
    start: f32,
    /// Last time the lifeline sent or received a message while active.
    last: f32,
}

/// What a part of the interaction touched: lifelines (by presentation) and fragment frames.
#[derive(Default)]
struct Walked {
    lanes: HashSet<Id>,
    frames: Vec<Rect>,
}

struct Builder<'a> {
    model: &'a Model,
    view: &'a View,
    measure: &'a dyn TextMeasure,
    lanes: HashMap<Id, LaneState>,
    /// Top of the free space below everything laid out so far.
    c: f32,
    stack: Vec<Open>,
    layout: SequenceLayout,
    created: HashMap<Id, f32>,
    destroyed: HashMap<Id, f32>,
    messages: Vec<EdgeShape>,
    fragments: HashMap<Id, NodeShape>,
}

/// Lays out the sequence diagram `diagram`: moves the lifeline nodes to their final place,
/// replaces the fragment nodes by their frames and returns the message edges.
pub(crate) fn arrange(
    model: &Model,
    diagram: Id,
    view: &View,
    nodes: &mut Vec<NodeShape>,
    measure: &dyn TextMeasure,
) -> (SequenceLayout, Vec<EdgeShape>) {
    let is_lifeline = |e: Option<Id>| {
        e.and_then(|e| model.get(e)).is_some_and(|e| matches!(e.kind(), ElementKind::Lifeline(_)))
    };
    let top = view
        .presentations()
        .iter()
        .filter(|p| is_lifeline(p.element))
        .filter_map(|p| p.bounds().map(|b| b.y))
        .reduce(f32::min)
        .unwrap_or(40.0);
    let lanes: HashMap<Id, LaneState> = nodes
        .iter()
        .filter(|n| is_lifeline(n.element))
        .map(|n| {
            let head = Rect::new(n.rect.x, top, n.rect.width, HEAD_HEIGHT);
            (n.id, LaneState { lifeline: n.element.expect("checked above"), head })
        })
        .collect();

    let mut b = Builder {
        model,
        view,
        measure,
        lanes,
        c: top + HEAD_HEIGHT + FIRST_GAP,
        stack: Vec::new(),
        layout: SequenceLayout::default(),
        created: HashMap::new(),
        destroyed: HashMap::new(),
        messages: Vec::new(),
        fragments: HashMap::new(),
    };
    if let Some(interaction) = model.get(diagram).and_then(|d| d.owner()) {
        b.walk(interaction);
    }
    while let Some(open) = b.stack.pop() {
        let end = (open.last + 10.0).max(open.start + 16.0);
        b.close(&open, end);
    }

    let bottom = b.c + TAIL;
    let mut layout = std::mem::take(&mut b.layout);
    let mut fragments = std::mem::take(&mut b.fragments);
    nodes.retain_mut(|n| {
        if let Some(lane) = b.lanes.get(&n.id) {
            let head_y = b.created.get(&n.id).map_or(top, |y| y - HEAD_HEIGHT / 2.0);
            let end = b.destroyed.get(&n.id).copied().unwrap_or(bottom).max(head_y + HEAD_HEIGHT);
            let dy = head_y - n.rect.y;
            n.rows.iter_mut().for_each(|r| r.rect = r.rect.translate(0.0, dy));
            let head = Rect::new(lane.head.x, head_y, lane.head.width, HEAD_HEIGHT);
            n.rect = Rect::new(head.x, head.y, head.width, end - head.y);
            n.figure = Figure::Lifeline { head };
            layout.lanes.push(Lane {
                presentation: n.id,
                lifeline: lane.lifeline,
                center: head.center().x,
            });
            return true;
        }
        let is_fragment = n.element.and_then(|e| model.get(e)).is_some_and(|e| {
            matches!(e.kind(), ElementKind::CombinedFragment(_) | ElementKind::InteractionUse(_))
        });
        match fragments.remove(&n.id) {
            Some(frame) => {
                *n = frame;
                true
            }
            // A fragment of another interaction has no place here.
            None => !is_fragment,
        }
    });
    layout.left = b.lanes.values().map(|l| l.head.x).reduce(f32::min).unwrap_or(0.0);
    layout.right = b.lanes.values().map(|l| l.head.right()).reduce(f32::max).unwrap_or(0.0);
    (layout, b.messages)
}

impl Builder<'_> {
    fn slot(&mut self, owner: Id, index: usize) {
        self.layout.slots.push(Slot { y: self.c, owner, index });
    }

    /// Lays out the children of an interaction or operand.
    fn walk(&mut self, container: Id) -> Walked {
        let model = self.model;
        let mut walked = Walked::default();
        let children = model.children(container);
        for (index, &child) in children.iter().enumerate() {
            let Some(e) = model.get(child) else { continue };
            match e.kind() {
                ElementKind::Message(p) => {
                    let (sort, gate) = (p.sort, p.gate);
                    let Some((edge, from, to)) = self.message_edge(child) else { continue };
                    self.slot(container, index);
                    if gate == MessageGate::None {
                        self.message(child, edge, from, to, sort);
                    } else {
                        self.gate_message(child, edge, to, sort, gate);
                    }
                    walked.lanes.extend([from, to]);
                }
                ElementKind::InteractionUse(_) => {
                    self.slot(container, index);
                    let inner = self.interaction_use(child);
                    walked.lanes.extend(inner.lanes);
                    walked.frames.extend(inner.frames);
                }
                ElementKind::CombinedFragment(_) => {
                    self.slot(container, index);
                    let inner = self.fragment(child);
                    walked.lanes.extend(inner.lanes);
                    walked.frames.extend(inner.frames);
                }
                _ => {}
            }
        }
        self.slot(container, children.len());
        walked
    }

    /// The message's edge on this diagram, with the lifeline nodes it runs between.
    fn message_edge(&self, message: Id) -> Option<(Id, Id, Id)> {
        self.view.presentations_of(message).find_map(|p| match p.shape {
            Shape::Edge { source, target, .. }
                if self.lanes.contains_key(&source) && self.lanes.contains_key(&target) =>
            {
                Some((p.id, source, target))
            }
            _ => None,
        })
    }

    fn center(&self, lane: Id) -> f32 {
        self.lanes[&lane].head.center().x
    }

    fn level(&self, lane: Id) -> Option<usize> {
        self.stack.iter().filter(|o| o.lane == lane).map(|o| o.level).max()
    }

    fn bar_left(&self, lane: Id, level: usize) -> f32 {
        self.center(lane) - BAR / 2.0 + level as f32 * BAR / 2.0
    }

    /// Where a message leaves or reaches `lane`: the side of its active bar facing `right` (or
    /// the other side), or the lifeline itself when it is not active.
    fn edge_x(&self, lane: Id, right: bool) -> f32 {
        match self.level(lane) {
            Some(level) if right => self.bar_left(lane, level) + BAR,
            Some(level) => self.bar_left(lane, level),
            None => self.center(lane),
        }
    }

    fn push(&mut self, lane: Id, start: f32) {
        let level = self.level(lane).map_or(0, |l| l + 1);
        self.stack.push(Open { lane, level, start, last: start });
    }

    fn close(&mut self, open: &Open, end: f32) {
        let x = self.bar_left(open.lane, open.level);
        self.layout.activations.push(Rect::new(x, open.start, BAR, (end - open.start).max(4.0)));
    }

    fn touch(&mut self, lane: Id, y: f32) {
        if let Some(open) = self.stack.iter_mut().rev().find(|o| o.lane == lane) {
            open.last = y;
        }
    }

    /// `lane` has control again: calls it made that never got a reply end here. A lifeline
    /// without an active bar started everything, so then all activations end.
    fn return_to(&mut self, lane: Id, y: f32) {
        let keep = self.stack.iter().rposition(|o| o.lane == lane).map_or(0, |pos| pos + 1);
        while self.stack.len() > keep {
            let open = self.stack.pop().expect("longer than pos");
            let end = (open.last + 10.0).max(open.start + 16.0).min(y - 4.0);
            self.close(&open, end);
        }
    }

    /// Ends the innermost activation of `lane` and everything started after it.
    fn finish(&mut self, lane: Id, y: f32) {
        let Some(pos) = self.stack.iter().rposition(|o| o.lane == lane) else { return };
        while self.stack.len() > pos {
            let open = self.stack.pop().expect("longer than pos");
            self.close(&open, y);
        }
    }

    fn message(&mut self, message: Id, edge: Id, from: Id, to: Id, sort: MessageSort) {
        let own = from == to;
        let y = if own { self.c + 14.0 } else { self.c + ROW / 2.0 };
        if matches!(sort, MessageSort::Synchronous | MessageSort::Create | MessageSort::Destroy) {
            self.return_to(from, y);
        }
        self.touch(from, y);
        let right = own || self.center(to) >= self.center(from);
        let start = self.edge_x(from, right);
        let loop_x = start.max(self.edge_x(from, true)) + SELF_WIDTH;
        let end_y = if own { y + SELF_LOOP } else { y };
        let end = match sort {
            MessageSort::Synchronous => {
                self.push(to, end_y);
                self.edge_x(to, !right || own)
            }
            MessageSort::Reply => {
                let end = self.edge_x(to, !right || own);
                self.finish(from, y);
                end
            }
            MessageSort::Create if !own => {
                self.created.insert(to, y);
                let head = self.lanes[&to].head;
                if right { head.x } else { head.right() }
            }
            MessageSort::Destroy if !own => {
                let end = self.center(to);
                self.finish(to, y);
                self.destroyed.insert(to, y);
                self.layout.destructions.push(Point::new(end, y));
                end
            }
            _ => self.edge_x(to, !right || own),
        };
        self.touch(to, end_y);

        let points = if own {
            vec![
                Point::new(start, y),
                Point::new(loop_x, y),
                Point::new(loop_x, end_y),
                Point::new(end, end_y),
            ]
        } else {
            vec![Point::new(start, y), Point::new(end, y)]
        };
        let (line, target_marker) = match sort {
            MessageSort::Synchronous | MessageSort::Destroy => {
                (LineStyle::Solid, EndMarker::FilledArrow)
            }
            MessageSort::Asynchronous => (LineStyle::Solid, EndMarker::OpenArrow),
            MessageSort::Reply | MessageSort::Create => (LineStyle::Dashed, EndMarker::OpenArrow),
        };
        let text = message_text(self.model, message);
        let labels = match text.is_empty() {
            true => Vec::new(),
            false if own => vec![EdgeLabel {
                text,
                anchor: Point::new(start + 4.0, y - LABEL_HEIGHT - 3.0),
                align: LabelAlign::Left,
            }],
            false => vec![middle_label(text, points[0], points[1])],
        };
        self.messages.push(EdgeShape {
            id: edge,
            points,
            line,
            source_marker: EndMarker::None,
            target_marker,
            labels,
            fixed: true,
        });
        self.c += if own { 14.0 + SELF_LOOP + 18.0 } else { ROW };
    }

    /// A found message comes from a dot left of `lane`, a lost one goes to a dot right of it.
    fn gate_message(
        &mut self,
        message: Id,
        edge: Id,
        lane: Id,
        sort: MessageSort,
        gate: MessageGate,
    ) {
        let y = self.c + ROW / 2.0;
        let (start, end, dot) = if gate == MessageGate::Found {
            if sort == MessageSort::Synchronous {
                self.push(lane, y);
            }
            let end = self.edge_x(lane, false);
            (end - GATE_LENGTH, end, Point::new(end - GATE_LENGTH, y))
        } else {
            if sort == MessageSort::Synchronous {
                self.return_to(lane, y);
            }
            let start = self.edge_x(lane, true);
            // The arrow ends where the dot begins.
            (start, start + GATE_LENGTH, Point::new(start + GATE_LENGTH + GATE_DOT, y))
        };
        self.touch(lane, y);
        self.layout.gates.push(dot);
        let points = vec![Point::new(start, y), Point::new(end, y)];
        let (line, target_marker) = match sort {
            MessageSort::Asynchronous => (LineStyle::Solid, EndMarker::OpenArrow),
            MessageSort::Reply | MessageSort::Create => (LineStyle::Dashed, EndMarker::OpenArrow),
            MessageSort::Synchronous | MessageSort::Destroy => {
                (LineStyle::Solid, EndMarker::FilledArrow)
            }
        };
        let text = message_text(self.model, message);
        let labels = if text.is_empty() {
            Vec::new()
        } else {
            vec![middle_label(text, points[0], points[1])]
        };
        self.messages.push(EdgeShape {
            id: edge,
            points,
            line,
            source_marker: EndMarker::None,
            target_marker,
            labels,
            fixed: true,
        });
        self.c += ROW;
    }

    /// The lifeline presentations a frame covers: those of `covered`, else `touched`, else all.
    fn covered_lanes(&self, covered: &[Id], touched: &HashSet<Id>) -> Vec<Id> {
        let mut lanes: Vec<Id> = self
            .lanes
            .iter()
            .filter(|(_, l)| covered.contains(&l.lifeline))
            .map(|(&p, _)| p)
            .collect();
        if lanes.is_empty() {
            lanes = touched.iter().copied().collect();
        }
        if lanes.is_empty() {
            lanes = self.lanes.keys().copied().collect();
        }
        lanes
    }

    /// Left and right edge of a frame around `lanes` and the frames inside it.
    fn frame_sides(&self, lanes: &[Id], inner: &[Rect]) -> (f32, f32) {
        let heads = lanes.iter().map(|l| self.lanes[l].head);
        let mut left = heads.clone().map(|h| h.x - FRAME_MARGIN).fold(f32::MAX, f32::min);
        let mut right = heads.map(|h| h.right() + FRAME_MARGIN).fold(f32::MIN, f32::max);
        for frame in inner {
            left = left.min(frame.x - 8.0);
            right = right.max(frame.right() + 8.0);
        }
        if left > right {
            // No lifelines on the diagram at all.
            (left, right) = (40.0, 200.0);
        }
        (left, right)
    }

    /// The tab at the top left of a frame, with its keyword.
    fn frame_tab(&self, keyword: &str, left: f32, top: f32, element: Id) -> (Rect, Row) {
        let tab = Rect::new(left, top, self.measure.width(keyword, 11.0, true) + 18.0, 18.0);
        let row = Row {
            text: keyword.to_owned(),
            rect: Rect::new(tab.x + 6.0, tab.y + 2.0, tab.width - 12.0, 14.0),
            size: 11.0,
            bold: true,
            italic: false,
            underline: false,
            align: LabelAlign::Left,
            role: RowRole::Stereotype,
            element: Some(element),
        };
        (tab, row)
    }

    /// A "ref" frame across the lifelines it covers, one row high. Not shown, it takes no
    /// space.
    fn interaction_use(&mut self, id: Id) -> Walked {
        let Some(presentation) =
            self.view.presentations_of(id).find(|p| !p.is_edge()).map(|p| p.id)
        else {
            return Walked::default();
        };
        let covered = match self.model.get(id).map(|e| e.kind()) {
            Some(ElementKind::InteractionUse(p)) => p.covered.clone(),
            _ => Vec::new(),
        };
        let lanes = self.covered_lanes(&covered, &HashSet::new());
        let (left, right) = self.frame_sides(&lanes, &[]);
        let top = self.c + 6.0;
        let text = interaction_use_text(self.model, id);
        let (tab, keyword) = self.frame_tab("ref", left, top, id);
        let right = right.max(left + tab.width + self.measure.width(&text, 12.0, false) + 30.0);
        let frame = Rect::new(left, top, right - left, REF_HEIGHT);
        self.c = frame.bottom() + 10.0;
        let h = crate::scene::line_height(12.0);
        let name = Row {
            text,
            rect: Rect::new(
                left,
                top + tab.height + (REF_HEIGHT - tab.height - h) / 2.0,
                frame.width,
                h,
            ),
            size: 12.0,
            bold: false,
            italic: false,
            underline: false,
            align: LabelAlign::Center,
            role: RowRole::Name,
            element: Some(id),
        };
        self.fragments.insert(
            presentation,
            NodeShape {
                id: presentation,
                element: Some(id),
                rect: frame,
                figure: Figure::Fragment { tab, separators: Vec::new() },
                rows: vec![keyword, name],
            },
        );
        Walked { lanes: lanes.into_iter().collect(), frames: vec![frame] }
    }

    fn fragment(&mut self, fragment: Id) -> Walked {
        let model = self.model;
        let operands: Vec<(Id, String)> = model
            .children(fragment)
            .iter()
            .filter_map(|&c| model.get(c))
            .filter_map(|c| match c.kind() {
                ElementKind::InteractionOperand(p) => Some((c.id(), p.guard.clone())),
                _ => None,
            })
            .collect();
        let presentation =
            self.view.presentations_of(fragment).find(|p| !p.is_edge()).map(|p| p.id);
        let Some(presentation) = presentation else {
            // Not shown: its content still is, as if it were not in a fragment.
            let mut walked = Walked::default();
            for (operand, _) in operands {
                let inner = self.walk(operand);
                walked.lanes.extend(inner.lanes);
                walked.frames.extend(inner.frames);
            }
            return walked;
        };
        let ElementKind::CombinedFragment(props) = model.get(fragment).expect("walked").kind()
        else {
            unreachable!("walk only passes fragments")
        };
        let (operator, covered) = (props.operator, props.covered.clone());

        let top = self.c + 6.0;
        let mut inside = Walked::default();
        let mut guards = Vec::new();
        let mut separators = Vec::new();
        if operands.is_empty() {
            self.c = top + FRAGMENT_HEADER + EMPTY_OPERAND;
        }
        for (k, (operand, guard)) in operands.into_iter().enumerate() {
            let guard_y = if k == 0 {
                self.c = top + FRAGMENT_HEADER;
                top + 22.0
            } else {
                let separator = self.c + 2.0;
                separators.push(separator);
                self.c = separator + OPERAND_HEADER;
                separator + 3.0
            };
            guards.push((operand, guard, guard_y));
            let start = self.c;
            let walked = self.walk(operand);
            if self.c == start {
                self.c += EMPTY_OPERAND;
            }
            inside.lanes.extend(walked.lanes);
            inside.frames.extend(walked.frames);
        }
        let bottom = self.c + 4.0;
        self.c = bottom + 10.0;

        // Covered lifelines as stored, else those its messages touch, else all of them.
        let lanes = self.covered_lanes(&covered, &inside.lanes);
        let (left, right) = self.frame_sides(&lanes, &inside.frames);
        let (tab, keyword) = self.frame_tab(operator_keyword(operator), left, top, fragment);
        let right = right.max(left + tab.width + 40.0);
        let frame = Rect::new(left, top, right - left, bottom - top);
        let mut rows = vec![keyword];
        for (operand, guard, y) in guards {
            let text = if guard.trim().is_empty() { String::new() } else { format!("[{guard}]") };
            let width = (self.measure.width(&text, 11.0, false) + 4.0).max(40.0);
            rows.push(Row {
                text,
                rect: Rect::new(left + 8.0, y, width, 15.0),
                size: 11.0,
                bold: false,
                italic: false,
                underline: false,
                align: LabelAlign::Left,
                role: RowRole::Member,
                element: Some(operand),
            });
        }
        self.fragments.insert(
            presentation,
            NodeShape {
                id: presentation,
                element: Some(fragment),
                rect: frame,
                figure: Figure::Fragment { tab, separators },
                rows,
            },
        );
        Walked { lanes: inside.lanes, frames: vec![frame] }
    }
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{Document, ElementKind, Id, MessageSort, Point};

    use crate::palette::{self, EdgeTool, NodeTool};
    use crate::scene::{self, ApproxMeasure, EndMarker, Figure, LineStyle, Preview, Scene};

    fn scene_of(doc: &Document, diagram: Id) -> Scene {
        scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure)
    }

    /// Message labels from top to bottom.
    fn order(scene: &Scene) -> Vec<String> {
        let mut edges: Vec<_> = scene.edges.iter().filter(|e| e.fixed).collect();
        edges.sort_by(|a, b| a.points[0].y.total_cmp(&b.points[0].y));
        edges.iter().map(|e| e.labels.first().map(|l| l.text.clone()).unwrap_or_default()).collect()
    }

    #[test]
    fn messages_follow_the_interaction_order() {
        let (doc, ids) = order_system();
        let scene = scene_of(&doc, ids.sequence_diagram);
        assert_eq!(order(&scene), ["addItem(item)", "submit", "confirmation"]);

        // Both heads on one line, the lifelines reach below the last message.
        let lanes: Vec<_> =
            scene.nodes.iter().filter(|n| matches!(n.figure, Figure::Lifeline { .. })).collect();
        assert_eq!(lanes.len(), 2);
        assert!(lanes.iter().all(|n| n.rect.y == 40.0));
        let last = scene.edges.iter().map(|e| e.points[0].y).fold(0.0, f32::max);
        assert!(lanes.iter().all(|n| n.rect.bottom() > last));

        let reply = scene.edges.iter().find(|e| e.line == LineStyle::Dashed).unwrap();
        assert_eq!(reply.target_marker, EndMarker::OpenArrow);
        assert!(reply.points[0].x > reply.points[1].x, "the reply runs back to the left");

        // The loop frames its message and covers both lifelines.
        let frame = scene
            .nodes
            .iter()
            .find(|n| matches!(n.figure, Figure::Fragment { .. }))
            .expect("loop fragment");
        let add_item = scene.edges.iter().map(|e| e.points[0]).fold(f32::MAX, |m, p| m.min(p.y));
        assert!(frame.rect.y < add_item && frame.rect.bottom() > add_item);
        assert!(
            lanes.iter().all(|l| frame.rect.x < l.rect.x && frame.rect.right() > l.rect.right())
        );
        let texts: Vec<&str> = frame.rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["loop", "[more items]"]);

        // addItem and submit each activate the order; the second call ends the first.
        let layout = scene.sequence.as_ref().unwrap();
        assert_eq!(layout.activations.len(), 2);
        assert!(layout.activations[0].bottom() <= layout.activations[1].y);
    }

    #[test]
    fn slots_reorder_and_wrap() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let message = |doc: &Document, name: &str| {
            doc.model()
                .subtree(interaction)
                .into_iter()
                .find(|&m| {
                    let e = doc.model().get(m).unwrap();
                    matches!(e.kind(), ElementKind::Message(_)) && e.name() == name
                })
                .unwrap()
        };
        let scene = scene_of(&doc, d);
        let layout = scene.sequence.as_ref().unwrap();
        // Dropping right above the top message puts it first in the interaction.
        let top = scene.edges.iter().map(|e| e.points[0].y).fold(f32::MAX, f32::min);
        let frame = scene.nodes.iter().find(|n| matches!(n.figure, Figure::Fragment { .. }));
        let slot = layout.slot_at(frame.unwrap().rect.y - 8.0).unwrap();
        assert_eq!((slot.owner, slot.index), (interaction, 2), "before the loop");
        let slot_inside = layout.slot_at(top - 10.0).unwrap();
        assert_ne!(slot_inside.owner, interaction, "inside the loop's operand");

        let submit = message(&doc, "submit");
        doc.edit("move", |tx| palette::move_to_slot(tx, submit, slot)).unwrap();
        assert_eq!(order(&scene_of(&doc, d)), ["submit", "addItem(item)", "confirmation"]);
        doc.edit("move in", |tx| palette::move_to_slot(tx, submit, slot_inside)).unwrap();
        assert_eq!(doc.model().get(submit).unwrap().owner(), Some(slot_inside.owner));
        doc.model().check().unwrap();

        // Wrapping the reply adds a second frame around it.
        let reply = doc
            .model()
            .children(interaction)
            .iter()
            .copied()
            .find(|&c| {
                matches!(
                    doc.model().get(c).unwrap().kind(),
                    ElementKind::Message(p) if p.sort == MessageSort::Reply
                )
            })
            .unwrap();
        doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[reply])).unwrap();
        let scene = scene_of(&doc, d);
        let frames =
            scene.nodes.iter().filter(|n| matches!(n.figure, Figure::Fragment { .. })).count();
        assert_eq!(frames, 2);
        // submit went into the loop before addItem.
        assert_eq!(order(&scene), ["submit", "addItem(item)", "confirmation"]);
        doc.model().check().unwrap();
    }

    #[test]
    fn new_messages_land_where_they_are_drawn() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let scene = scene_of(&doc, d);
        let layout = scene.sequence.clone().unwrap();
        let lanes: Vec<Id> = layout.lanes.iter().map(|l| l.presentation).collect();
        let y = scene.edges.iter().map(|e| e.points[0].y).fold(0.0, f32::max) + 30.0;
        let slot = layout.slot_at(y);
        // A self call at the end, then a create message is refused on the same lifeline.
        doc.edit("self", |tx| {
            palette::connect_message(tx, d, EdgeTool::Message, lanes[0], lanes[0], slot)
        })
        .unwrap();
        assert!(
            doc.edit("bad", |tx| {
                palette::connect_message(tx, d, EdgeTool::CreateMessage, lanes[0], lanes[0], None)
            })
            .is_err()
        );
        let scene = scene_of(&doc, d);
        let own = scene.edges.iter().find(|e| e.points.len() == 4).expect("self message loop");
        assert!(own.points[1].x > own.points[0].x && own.points[2].y > own.points[1].y);

        // Fragments placed with the tool cover the lifelines left and right of the click.
        let between = (layout.lanes[0].center + layout.lanes[1].center) / 2.0;
        assert_eq!(layout.lanes_around(between).len(), 2);
        assert_eq!(layout.lanes_around(-100.0).len(), 2);
    }

    #[test]
    fn interaction_uses_and_gate_messages() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let scene = scene_of(&doc, d);
        let lanes: Vec<Id> =
            scene.sequence.as_ref().unwrap().lanes.iter().map(|l| l.presentation).collect();
        let (found_lane, lost_lane) = (lanes[1], lanes[0]);
        doc.edit("add", |tx| {
            let (use_id, _) = palette::create_fragment(
                tx,
                d,
                NodeTool::InteractionUse,
                None,
                Vec::new(),
                Point::default(),
            )?;
            tx.update(use_id, |data| {
                if let ElementKind::InteractionUse(p) = &mut data.kind {
                    p.refers_to = Some(interaction);
                    p.arguments = "2".into();
                }
            })?;
            palette::connect_message(tx, d, EdgeTool::FoundMessage, found_lane, found_lane, None)?;
            palette::connect_message(tx, d, EdgeTool::LostMessage, lost_lane, lost_lane, None)
        })
        .unwrap();
        doc.model().check().unwrap();
        let scene = scene_of(&doc, d);

        // The interaction use is a "ref" frame below the last message, named after the
        // interaction it refers to.
        let frame = scene
            .nodes
            .iter()
            .find(|n| n.rows.first().is_some_and(|r| r.text == "ref"))
            .expect("ref frame");
        assert_eq!(frame.rows[1].text, "Place order(2)");
        let replies = scene.edges.iter().filter(|e| e.line == LineStyle::Dashed);
        assert!(replies.map(|e| e.points[0].y).all(|y| y < frame.rect.y));

        // Gate messages end at a dot beside their lifeline: left for found, right for lost.
        let layout = scene.sequence.as_ref().unwrap();
        assert_eq!(layout.gates.len(), 2);
        let center = |p: Id| layout.lanes.iter().find(|l| l.presentation == p).unwrap().center;
        let found = scene.edges.iter().find(|e| e.points[0].y > frame.rect.bottom()).unwrap();
        assert!(found.points[0].x < center(found_lane) && found.points[1].x <= center(found_lane));
        assert_eq!(found.target_marker, EndMarker::FilledArrow);
        assert!(layout.gates.iter().any(|g| g.x > center(lost_lane) && g.y > found.points[0].y));
    }
}
