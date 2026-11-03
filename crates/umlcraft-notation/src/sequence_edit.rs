//! Editing sequence diagrams the way Astah does. The geometry is what is stored; edits place
//! things where the user puts them and then tidy up:
//!
//! - A call gets a bar of 25 on the receiver; a sender without a bar there gets one of 45 (60
//!   for a create message, 30 for a lost message), unless a bar of its ends just above: that
//!   one goes on. A reply leaves at the end of its bar.
//! - A further call to the same receiver starts below the bar the last one started there, 20
//!   apart, so that the two bars do not run into each other.
//! - Bars grow to end 20 below the last thing inside them, lifelines to reach 30 below the last
//!   thing on them, the diagram frame to surround everything.
//! - The interaction's order follows the heights: messages and fragments belong to the operand
//!   of the innermost fragment drawn around them, in the order they are drawn.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    Document, DurationConstraintProps, ElementKind, Id, MessageGate, MessageSort, Model,
    ModelError, Presentation, Rect, Result, Shape, StateInvariantProps, TerminationProps,
    TimeConstraintProps, Tx, View,
};

use crate::sequence::SELF_DROP;

/// Height of the bar a call starts.
pub const RECEIVER_BAR: f32 = 25.0;
/// Height of a bar a lifeline gets for sending.
const SENDER_BAR: f32 = 45.0;
const CREATE_SENDER_BAR: f32 = 60.0;
const LOST_SENDER_BAR: f32 = 30.0;
/// Space a bar keeps below the last thing inside it.
const BAR_MARGIN: f32 = 20.0;
/// Space a lifeline keeps below the last thing on it.
const LIFELINE_MARGIN: f32 = 30.0;
/// How far the open end of a found or lost message is from its lifeline.
const GATE_DISTANCE: f32 = 120.0;
/// How close the open end of a found or lost message may come to its lifeline.
const MIN_GATE_DISTANCE: f32 = 40.0;
/// Length of a lifeline on an empty diagram.
const LIFELINE_LENGTH: f32 = 300.0;
/// Space between the frame and what it surrounds.
const FRAME_MARGIN: f32 = 15.0;
/// The frame's top lies this far above the lifeline heads, as Astah puts it.
const FRAME_ABOVE_HEADS: f32 = 35.093_75;
/// Below an icon head, the name; the line starts below it.
const ICON_NAME: f32 = 15.093_75;
/// Distance between a new message and the last thing above it.
const NEXT: f32 = 40.0;
/// Space between the bar a call starts and the next call from the same sender to the same
/// receiver.
const MESSAGE_GAP: f32 = 20.0;
/// How far below a bar a message sent from its lifeline still continues that bar.
const CONTINUE_BAR: f32 = SENDER_BAR;
/// Where lifelines start on a diagram without any.
const FIRST_LINE_TOP: f32 = 60.0;
/// The shortest lifeline a new one gets.
const MIN_LIFELINE: f32 = 60.0;
/// Left and right of a frame on a diagram without lifelines.
const EMPTY_SPAN: (f32, f32) = (40.0, 240.0);
/// Default heights of a new combined fragment and interaction use.
const FRAGMENT_HEIGHT: f32 = 80.0;
const USE_HEIGHT: f32 = 60.0;
/// How far a new frame reaches beyond the heads of the lifelines it covers.
const FRAGMENT_MARGIN: f32 = 10.0;
/// How much a fragment grows for a new operand.
const OPERAND_HEIGHT: f32 = 50.0;
/// Space a fragment keeps below what lies in it.
const FRAGMENT_PADDING: f32 = 10.0;

#[derive(Clone, Copy)]
struct Lane {
    id: Id,
    head: Rect,
    length: f32,
    destroyed: bool,
    iconic: bool,
}

impl Lane {
    fn line_top(&self) -> f32 {
        self.head.bottom() + if self.iconic { ICON_NAME } else { 0.0 }
    }

    fn bottom(&self) -> f32 {
        self.line_top() + self.length
    }

    fn center(&self) -> f32 {
        self.head.center().x
    }
}

#[derive(Clone, Copy)]
struct Bar {
    id: Id,
    lane: Id,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy)]
struct Msg {
    id: Id,
    element: Option<Id>,
    source: Id,
    target: Id,
    y: f32,
    end_y: Option<f32>,
    sort: MessageSort,
}

impl Msg {
    fn end(&self) -> f32 {
        self.end_y.unwrap_or(self.y)
    }
}

#[derive(Clone)]
struct Frame {
    id: Id,
    element: Id,
    rect: Rect,
    separators: Vec<f32>,
}

/// A state invariant or a termination on the lifeline presentation `lane`.
#[derive(Clone, Copy)]
struct Mark {
    id: Id,
    element: Id,
    lane: Id,
    rect: Rect,
    termination: bool,
}

impl Mark {
    /// Where it lies on its lifeline: an invariant's top, a cross's center.
    fn y(&self) -> f32 {
        if self.termination { self.rect.center().y } else { self.rect.y }
    }
}

/// The node of a duration or time constraint.
#[derive(Clone, Copy)]
struct Constraint {
    id: Id,
    element: Id,
    rect: Rect,
    /// Its label's offset from its usual place, if moved by hand.
    label: Option<umlcraft_model::Point>,
}

/// Size of a termination's cross.
pub const CROSS: f32 = 20.0;
/// State invariants: Astah's box, as wide whatever it reads (longer text wraps), at least as
/// high as for one line of text, and the height of each line.
pub const INVARIANT_WIDTH: f32 = 74.044_92;
pub const INVARIANT_HEIGHT: f32 = 21.0;
pub const INVARIANT_LINE: f32 = 15.093_75;
/// What a state invariant adds to the height of its text.
const INVARIANT_TEXT_PAD: f32 = 5.0;

/// The height Astah gives a state invariant of `lines` lines: theirs and 5, at least 21.
pub fn invariant_height(lines: usize) -> f32 {
    (lines as f32 * INVARIANT_LINE + INVARIANT_TEXT_PAD).max(INVARIANT_HEIGHT)
}
/// Time constraints: how far the line goes on beyond the message's arrowhead.
pub const TIME_TICK: f32 = 30.0;
/// Edge labels' height, by which constraint labels are centered on their point.
const CONSTRAINT_LABEL: f32 = 15.093_75;

/// Refusals of the tools that put things on lifelines and messages.
pub const NOT_ON_LIFELINE: &str = "a state invariant or termination goes on a lifeline";
pub const LIFELINE_ENDED: &str = "the lifeline ends already";
pub const NOT_ON_MESSAGE: &str = "a duration or time constraint goes on messages";

/// The stored geometry of a sequence diagram.
struct Geometry {
    lanes: Vec<Lane>,
    bars: Vec<Bar>,
    messages: Vec<Msg>,
    /// Combined fragments and interaction uses.
    frames: Vec<Frame>,
    /// The diagram frame.
    diagram_frame: Option<(Id, Rect)>,
    notes: Vec<Rect>,
    /// State invariants and terminations.
    marks: Vec<Mark>,
    /// Duration and time constraints.
    constraints: Vec<Constraint>,
}

impl Geometry {
    fn read(model: &Model, diagram: Id) -> Result<Self> {
        let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let mut g = Geometry {
            lanes: Vec::new(),
            bars: Vec::new(),
            messages: Vec::new(),
            frames: Vec::new(),
            diagram_frame: None,
            notes: Vec::new(),
            marks: Vec::new(),
            constraints: Vec::new(),
        };
        for p in view.presentations() {
            match &p.shape {
                Shape::Lifeline { head, length, destroyed } => g.lanes.push(Lane {
                    id: p.id,
                    head: *head,
                    length: *length,
                    destroyed: *destroyed,
                    iconic: p.style.iconic,
                }),
                Shape::Activation { lifeline, top, height, .. } => {
                    g.bars.push(Bar { id: p.id, lane: *lifeline, top: *top, bottom: top + height })
                }
                Shape::Message { source, target, y, end_y, .. } => {
                    let sort = p
                        .element
                        .and_then(|e| model.get(e))
                        .and_then(|e| match e.kind() {
                            ElementKind::Message(m) => Some(m.sort),
                            _ => None,
                        })
                        .unwrap_or_default();
                    g.messages.push(Msg {
                        id: p.id,
                        element: p.element,
                        source: *source,
                        target: *target,
                        y: *y,
                        end_y: *end_y,
                        sort,
                    });
                }
                Shape::Fragment { bounds, separators, .. } => {
                    if let Some(element) = p.element {
                        g.frames.push(Frame {
                            id: p.id,
                            element,
                            rect: *bounds,
                            separators: separators.iter().map(|s| bounds.y + s).collect(),
                        });
                    }
                }
                Shape::Node { bounds, .. } if p.element == Some(diagram) => {
                    g.diagram_frame = Some((p.id, *bounds));
                }
                Shape::Node { bounds, .. } => {
                    let element = p.element.and_then(|e| model.get(e));
                    match element.map(|e| e.kind()) {
                        Some(ElementKind::InteractionUse(_)) => g.frames.push(Frame {
                            id: p.id,
                            element: p.element.expect("matched"),
                            rect: *bounds,
                            separators: Vec::new(),
                        }),
                        Some(
                            ElementKind::StateInvariant(StateInvariantProps { lifeline })
                            | ElementKind::Termination(TerminationProps { lifeline }),
                        ) => {
                            if let Some(lane) = lane_of(view, *lifeline) {
                                g.marks.push(Mark {
                                    id: p.id,
                                    element: p.element.expect("matched"),
                                    lane,
                                    rect: *bounds,
                                    termination: matches!(
                                        element.map(|e| e.kind()),
                                        Some(ElementKind::Termination(_))
                                    ),
                                });
                            }
                        }
                        Some(
                            ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_),
                        ) => {
                            g.constraints.push(Constraint {
                                id: p.id,
                                element: p.element.expect("matched"),
                                rect: *bounds,
                                label: p.style.name_offset,
                            });
                        }
                        _ => g.notes.push(*bounds),
                    }
                }
                // Free texts and lines stay where they are, as in Astah.
                Shape::Edge { .. } | Shape::Text { .. } | Shape::Line { .. } => {}
            }
        }
        Ok(g)
    }

    fn lane(&self, id: Id) -> Option<&Lane> {
        self.lanes.iter().find(|l| l.id == id)
    }

    /// The lifeline whose line runs at `x`: under its head, the nearest one.
    fn lane_at(&self, x: f32) -> Option<&Lane> {
        self.lanes
            .iter()
            .filter(|l| l.head.x - 0.5 <= x && x <= l.head.right() + 0.5)
            .min_by(|a, b| (a.center() - x).abs().total_cmp(&(b.center() - x).abs()))
    }

    /// The cross that ends `lane` without a destroy message, if any.
    fn termination(&self, lane: Id) -> Option<&Mark> {
        self.marks.iter().find(|m| m.termination && m.lane == lane)
    }

    /// The state invariants on `lane`.
    fn invariants(&self, lane: Id) -> impl Iterator<Item = &Mark> {
        self.marks.iter().filter(move |m| !m.termination && m.lane == lane)
    }

    /// The message presentation showing the message `element`, if this diagram shows it.
    fn message_of(&self, element: Id) -> Option<&Msg> {
        self.messages.iter().find(|m| m.element == Some(element))
    }

    /// The innermost bar on `lane` at `y`.
    fn bar_at(&self, lane: Id, y: f32) -> Option<Bar> {
        self.bars
            .iter()
            .filter(|b| b.lane == lane && b.top - 0.5 <= y && y <= b.bottom + 0.5)
            .min_by(|a, b| (a.bottom - a.top).total_cmp(&(b.bottom - b.top)))
            .copied()
    }

    /// The bar on `lane` that a message sent at `y` continues: one ending at most
    /// `CONTINUE_BAR` above, the lowest of them, unless a reply has ended it.
    fn bar_ending_above(&self, lane: Id, y: f32) -> Option<Bar> {
        self.bars
            .iter()
            .filter(|b| b.lane == lane && b.bottom < y && y - b.bottom <= CONTINUE_BAR)
            .filter(|b| !self.replied(b))
            .max_by(|a, b| a.bottom.total_cmp(&b.bottom))
            .copied()
    }

    /// Whether a reply leaves at the end of `bar`, so that the call it holds has returned.
    fn replied(&self, bar: &Bar) -> bool {
        self.messages.iter().any(|m| {
            m.sort == MessageSort::Reply && m.source == bar.lane && (m.y - bar.bottom).abs() < 0.5
        })
    }

    /// Where `lane` ends in a cross, if a message destroys it: with the bar the destroy message
    /// starts, which comes along, or else at the message.
    fn cross(&self, lane: Id) -> Option<(Option<Id>, f32)> {
        let m = self
            .messages
            .iter()
            .filter(|m| m.target == lane && m.sort == MessageSort::Destroy)
            .min_by(|a, b| a.end().total_cmp(&b.end()))?;
        let bar = self.bars.iter().find(|b| b.lane == lane && (b.top - m.end()).abs() < 0.5);
        Some(bar.map_or((None, m.end()), |b| (Some(b.id), b.bottom)))
    }

    /// Where a call from `from` to `to` drawn at `y` goes: below the bars earlier calls between
    /// the two started on `to`, unless `to` called back in between (a nested call).
    fn clear_of_calls(&self, from: Id, to: Id, mut y: f32) -> f32 {
        let started_by_from = |bar: &Bar| {
            self.messages.iter().any(|m| {
                m.source == from
                    && m.target == to
                    && m.sort != MessageSort::Reply
                    && (bar.top - m.end()).abs() < 0.5
            })
        };
        let called_back = |bar: &Bar, y: f32| {
            self.messages
                .iter()
                .any(|m| m.source == to && m.target == from && bar.top < m.y && m.y < y)
        };
        // Each step moves `y` below one more bar; there are only so many.
        for _ in 0..self.bars.len() {
            let blocking = self.bars.iter().find(|b| {
                b.lane == to
                    && b.top < y
                    && y < b.bottom + MESSAGE_GAP
                    && started_by_from(b)
                    && !called_back(b, y)
            });
            match blocking {
                Some(bar) => y = bar.bottom + MESSAGE_GAP,
                None => break,
            }
        }
        y
    }

    /// Where a new message of `sort` from `source` to `target` may go, from and to: below the
    /// heads of its lifelines and above their crosses. A create message stays above what the
    /// lifeline it makes does, a destroy message goes below it.
    fn room(&self, source: Id, target: Id, sort: MessageSort) -> (f32, f32) {
        let (mut least, mut most) = (f32::MIN, f32::MAX);
        for lane in [source, target] {
            let Some(l) = self.lane(lane) else { continue };
            let destroys = |m: &&Msg| m.sort == MessageSort::Destroy && m.target == lane;
            if let Some(cross) =
                self.messages.iter().filter(destroys).map(|m| m.end()).reduce(f32::min)
            {
                most = most.min(cross - MESSAGE_GAP);
            }
            if let Some(t) = self.termination(lane) {
                most = most.min(t.y() - MESSAGE_GAP);
            }
            if sort != MessageSort::Create || lane != target {
                least = least.max(l.line_top() + MESSAGE_GAP);
            }
        }
        let on_target = || {
            let messages = self
                .messages
                .iter()
                .filter(|m| {
                    m.source == target || (m.target == target && m.sort != MessageSort::Create)
                })
                .map(|m| if m.source == target { m.y } else { m.end() });
            let bars = self.bars.iter().filter(|b| b.lane == target);
            messages.chain(bars.flat_map(|b| [b.top, b.bottom]))
        };
        match (sort, self.lane(target)) {
            (MessageSort::Create, Some(l)) => {
                // The head comes down to the message, its line starts below.
                let below = l.line_top() - l.head.center().y;
                let constructor = |y: &f32| (y - l.line_top()).abs() > 0.5;
                if let Some(first) = on_target().filter(constructor).reduce(f32::min) {
                    most = most.min(first - MESSAGE_GAP - below);
                }
            }
            (MessageSort::Destroy, Some(_)) => {
                if let Some(last) = on_target().reduce(f32::max) {
                    least = least.max(last + MESSAGE_GAP);
                }
            }
            _ => {}
        }
        (least, most)
    }

    /// The lowest point of what is drawn between the heads and the lifelines' ends.
    fn content_bottom(&self) -> Option<f32> {
        let messages = self.messages.iter().map(|m| m.end());
        let bars = self.bars.iter().map(|b| b.bottom);
        let frames = self.frames.iter().map(|f| f.rect.bottom());
        let invariants = self.marks.iter().filter(|m| !m.termination).map(|m| m.rect.bottom());
        messages.chain(bars).chain(frames).chain(invariants).reduce(f32::max)
    }
}

/// The lifeline presentation showing `lifeline` on `diagram`.
pub fn lane_of(view: &View, lifeline: Id) -> Option<Id> {
    view.presentations_of(lifeline)
        .find(|p| matches!(p.shape, Shape::Lifeline { .. }))
        .map(|p| p.id)
}

/// The top and bottom of what `items` (messages, fragments, interaction uses) span on
/// `diagram`, and the lifelines they touch.
pub fn extent(model: &Model, diagram: Id, items: &[Id]) -> (f32, f32, Vec<Id>) {
    let Ok(g) = Geometry::read(model, diagram) else { return (0.0, 0.0, Vec::new()) };
    let Some(view) = model.view(diagram) else { return (0.0, 0.0, Vec::new()) };
    let (mut top, mut bottom) = (f32::MAX, f32::MIN);
    let mut lanes: Vec<Id> = Vec::new();
    let touch = |lane: Id, lanes: &mut Vec<Id>| {
        if let Some(lifeline) = view.get(lane).and_then(|p| p.element)
            && !lanes.contains(&lifeline)
        {
            lanes.push(lifeline);
        }
    };
    for &item in items {
        if let Some(m) = g.messages.iter().find(|m| m.element == Some(item)) {
            top = top.min(m.y);
            bottom = bottom.max(m.end());
            // The bar a call starts belongs to it.
            if let Some(bar) =
                g.bars.iter().find(|b| b.lane == m.target && (b.top - m.end()).abs() < 0.5)
            {
                bottom = bottom.max(bar.bottom);
            }
            touch(m.source, &mut lanes);
            touch(m.target, &mut lanes);
        }
        if let Some(f) = g.frames.iter().find(|f| f.element == item) {
            top = top.min(f.rect.y);
            bottom = bottom.max(f.rect.bottom());
            for lane in &g.lanes {
                if f.rect.x <= lane.center() && lane.center() <= f.rect.right() {
                    touch(lane.id, &mut lanes);
                }
            }
        }
    }
    if top > bottom {
        return (next_y(model, diagram), next_y(model, diagram), lanes);
    }
    (top, bottom, lanes)
}

/// Where something new goes when no place is given: below everything.
pub fn next_y(model: &Model, diagram: Id) -> f32 {
    let Ok(g) = Geometry::read(model, diagram) else { return FIRST_LINE_TOP + NEXT };
    let heads = g.lanes.iter().map(|l| l.line_top()).reduce(f32::max).unwrap_or(FIRST_LINE_TOP);
    g.content_bottom().unwrap_or(heads) + NEXT
}

/// Shows `lifeline` with its head at `x`, lined up with the other heads and as long as the
/// longest lifeline.
pub fn add_lifeline(tx: &mut Tx<'_>, diagram: Id, lifeline: Id, x: f32, y: f32) -> Result<Id> {
    let g = Geometry::read(tx.model(), diagram)?;
    let top = g.lanes.iter().map(|l| l.head.y).reduce(f32::min).unwrap_or(y);
    let head = Rect::new(x, top, crate::sequence::HEAD_WIDTH, crate::sequence::HEAD_HEIGHT);
    let length = g
        .lanes
        .iter()
        .filter(|l| !l.destroyed && g.termination(l.id).is_none())
        .map(|l| l.bottom())
        .reduce(f32::max)
        .map_or(LIFELINE_LENGTH, |bottom| (bottom - head.bottom()).max(MIN_LIFELINE));
    let id = tx.add_presentation(diagram, Presentation::lifeline(lifeline, head, length))?;
    tidy(tx, diagram)?;
    Ok(id)
}

fn add_bar(tx: &mut Tx<'_>, diagram: Id, lane: Id, top: f32, height: f32) -> Result<Id> {
    tx.add_presentation(diagram, Presentation::activation(lane, top, height))
}

/// Lets the outermost bar on `lane` that no reply has ended reach down to a message sent from
/// `lane` at `y`, however far below it: what a lifeline does after it was called belongs to
/// that call until it answers. For messages added below everything without a place given (the
/// MCP server); a message drawn at a height keeps the bars found there.
pub fn continue_open_bar(tx: &mut Tx<'_>, diagram: Id, lane: Id, y: f32) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    if g.bar_at(lane, y).is_some() {
        return Ok(());
    }
    let on_lane = |b: &&Bar| b.lane == lane;
    let inside = |b: &Bar| {
        g.bars
            .iter()
            .filter(on_lane)
            .any(|o| o.id != b.id && o.top <= b.top && o.bottom >= b.bottom)
    };
    let open = g
        .bars
        .iter()
        .filter(on_lane)
        .filter(|b| b.bottom < y && !inside(b) && !g.replied(b))
        .max_by(|a, b| a.bottom.total_cmp(&b.bottom));
    let Some(bar) = open.copied() else { return Ok(()) };
    tx.update_presentation(diagram, bar.id, |p| {
        if let Shape::Activation { height, .. } = &mut p.shape {
            *height = y - bar.top + BAR_MARGIN;
        }
    })
}

/// Gives a message sent from `lane` at `y` a bar there: the bar ending just above goes on to
/// hold it, else a new one of `height` starts.
fn sender_bar(
    tx: &mut Tx<'_>,
    diagram: Id,
    g: &Geometry,
    lane: Id,
    y: f32,
    height: f32,
) -> Result<()> {
    if g.bar_at(lane, y).is_some() {
        return Ok(());
    }
    match g.bar_ending_above(lane, y) {
        Some(bar) => tx.update_presentation(diagram, bar.id, |p| {
            if let Shape::Activation { height: h, .. } = &mut p.shape {
                *h = y - bar.top + BAR_MARGIN;
            }
        }),
        None => add_bar(tx, diagram, lane, y, height).map(|_| ()),
    }
}

/// Draws `message` from lane `source` to lane `target` (the same one for messages to self and
/// for found and lost messages) at height `y`, with the bars Astah gives it.
pub fn add_message(
    tx: &mut Tx<'_>,
    diagram: Id,
    message: Id,
    source: Id,
    target: Id,
    y: f32,
) -> Result<Id> {
    let model = tx.model();
    let ElementKind::Message(props) = model.element(message)?.kind() else {
        return Err(ModelError::Invalid("not a message".into()));
    };
    let (sort, gate) = (props.sort, props.gate);
    let g = Geometry::read(model, diagram)?;
    let (from, to) = match (g.lane(source), g.lane(target)) {
        (Some(from), Some(to)) => (*from, *to),
        _ => return Err(ModelError::Invalid("messages connect lifelines".into())),
    };
    let call = matches!(sort, MessageSort::Synchronous | MessageSort::Asynchronous);
    let y = if call && gate == MessageGate::None && source != target {
        g.clear_of_calls(from.id, to.id, y)
    } else {
        y
    };
    // A lifeline is made once.
    if sort == MessageSort::Create
        && g.messages.iter().any(|m| m.sort == MessageSort::Create && m.target == to.id)
    {
        return Err(ModelError::Invalid("the lifeline is created already".into()));
    }
    let (least, most) = g.room(from.id, to.id, sort);
    if least > most {
        return Err(ModelError::Invalid("no room for the message on these lifelines".into()));
    }
    let y = y.clamp(least, most);
    let sender = |tx: &mut Tx<'_>, height: f32| sender_bar(tx, diagram, &g, from.id, y, height);
    // A reply leaves at the end of the bar it returns from: the one at `y`, else the closest
    // one above.
    let reply_at = || {
        let above = || {
            g.bars
                .iter()
                .filter(|b| b.lane == from.id && b.bottom <= y)
                .max_by(|a, b| a.bottom.total_cmp(&b.bottom))
                .copied()
        };
        g.bar_at(from.id, y).or_else(above).map(|bar| bar.bottom)
    };
    let mut presentation = Presentation::message(message, source, target, y);
    match (gate, sort) {
        (MessageGate::Found, _) => {
            add_bar(tx, diagram, to.id, y, RECEIVER_BAR)?;
            if let Shape::Message { gate_x, .. } = &mut presentation.shape {
                *gate_x = Some(to.center() - GATE_DISTANCE);
            }
        }
        (MessageGate::Lost, _) => {
            let at = if sort == MessageSort::Reply {
                reply_at()
            } else {
                sender(tx, LOST_SENDER_BAR)?;
                None
            };
            if let Shape::Message { y, gate_x, .. } = &mut presentation.shape {
                *y = at.unwrap_or(*y);
                *gate_x = Some(from.center() + GATE_DISTANCE);
            }
        }
        (_, MessageSort::Reply) => {
            if let (Some(at), Shape::Message { y, .. }) = (reply_at(), &mut presentation.shape) {
                *y = at;
            }
        }
        _ if source == target => {
            sender(tx, SENDER_BAR)?;
            add_bar(tx, diagram, to.id, y + SELF_DROP, RECEIVER_BAR)?;
            if let Shape::Message { end_y, .. } = &mut presentation.shape {
                *end_y = Some(y + SELF_DROP);
            }
        }
        (_, MessageSort::Create) => {
            sender(tx, CREATE_SENDER_BAR)?;
            // The new lifeline's head comes down to the message; its line keeps its end. The
            // message ends at the head, and the constructor runs in a bar right below it
            // (Astah draws none there).
            let head =
                Rect::new(to.head.x, y - to.head.height / 2.0, to.head.width, to.head.height);
            let line_top = head.bottom() + if to.iconic { ICON_NAME } else { 0.0 };
            let length = (to.bottom() - line_top).max(RECEIVER_BAR + BAR_MARGIN);
            tx.update_presentation(diagram, to.id, |p| {
                if let Shape::Lifeline { head: h, length: l, .. } = &mut p.shape {
                    (*h, *l) = (head, length);
                }
            })?;
            add_bar(tx, diagram, to.id, line_top, RECEIVER_BAR)?;
        }
        (_, MessageSort::Destroy) => {
            sender(tx, SENDER_BAR)?;
            add_bar(tx, diagram, to.id, y, RECEIVER_BAR)?;
            // Above the head the lifeline keeps at least the bar's length.
            let length = (y + RECEIVER_BAR - to.line_top()).max(RECEIVER_BAR);
            tx.update_presentation(diagram, to.id, |p| {
                if let Shape::Lifeline { length: l, destroyed, .. } = &mut p.shape {
                    (*l, *destroyed) = (length, true);
                }
            })?;
        }
        _ => {
            sender(tx, SENDER_BAR)?;
            add_bar(tx, diagram, to.id, y, RECEIVER_BAR)?;
        }
    }
    // A reply leaves where its bar ends, which may lie beyond the lifeline's cross.
    if let Shape::Message { y, .. } = presentation.shape
        && (y < least - 0.5 || y > most + 0.5)
    {
        return Err(ModelError::Invalid("no room for the message on these lifelines".into()));
    }
    let id = tx.add_presentation(diagram, presentation)?;
    tidy(tx, diagram)?;
    Ok(id)
}

/// Draws a combined fragment or interaction use across the lanes of `covered` (all lanes when
/// empty) with its top at `y`.
pub fn add_fragment(
    tx: &mut Tx<'_>,
    diagram: Id,
    element: Id,
    covered: &[Id],
    y: f32,
) -> Result<Id> {
    let model = tx.model();
    let is_use = matches!(model.element(element)?.kind(), ElementKind::InteractionUse(_));
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let g = Geometry::read(model, diagram)?;
    let mut lanes: Vec<&Lane> =
        covered.iter().filter_map(|&l| lane_of(view, l)).filter_map(|l| g.lane(l)).collect();
    if lanes.is_empty() {
        lanes = g.lanes.iter().collect();
    }
    let left =
        lanes.iter().map(|l| l.head.x).reduce(f32::min).unwrap_or(EMPTY_SPAN.0) - FRAGMENT_MARGIN;
    let right = lanes.iter().map(|l| l.head.right()).reduce(f32::max).unwrap_or(EMPTY_SPAN.1)
        + FRAGMENT_MARGIN;
    let height = if is_use { USE_HEIGHT } else { FRAGMENT_HEIGHT };
    let bounds = Rect::new(left, y, right - left, height);
    let presentation = if is_use {
        Presentation::node(Some(element), bounds)
    } else {
        Presentation::fragment(element, bounds)
    };
    let id = tx.add_presentation(diagram, presentation)?;
    tidy(tx, diagram)?;
    Ok(id)
}

/// Creates a state invariant reading `name` (`termination` false) or a termination on the
/// lifeline drawn at `at`, and shows it there (see [`add_mark`]). Refused off a lifeline
/// ([`NOT_ON_LIFELINE`]) and for a termination on a lifeline that ends already
/// ([`LIFELINE_ENDED`]). Returns (element, presentation).
pub fn create_on_lifeline(
    tx: &mut Tx<'_>,
    diagram: Id,
    termination: bool,
    at: umlcraft_model::Point,
    name: &str,
) -> Result<(Id, Id)> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let g = Geometry::read(model, diagram)?;
    let lane = g.lane_at(at.x).ok_or_else(|| ModelError::Invalid(NOT_ON_LIFELINE.into()))?;
    let lifeline =
        view.get(lane.id).and_then(|p| p.element).ok_or(ModelError::NotFound(lane.id))?;
    let interaction = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
    let (kind, name) = if termination {
        (ElementKind::Termination(TerminationProps { lifeline }), "")
    } else {
        (ElementKind::StateInvariant(StateInvariantProps { lifeline }), name)
    };
    let element = tx.create(interaction, umlcraft_model::ElementData::new(name, kind))?;
    let node = add_mark(tx, diagram, element, at.y)?;
    Ok((element, node))
}

/// The point on the lifeline drawn as `lane` for [`create_on_lifeline`]: at height `y`, or
/// without one below everything drawn so far.
pub fn spot_on_lifeline(
    model: &Model,
    diagram: Id,
    lane: Id,
    y: Option<f32>,
) -> Result<umlcraft_model::Point> {
    let g = Geometry::read(model, diagram)?;
    let lane = g.lane(lane).ok_or_else(|| ModelError::Invalid(NOT_ON_LIFELINE.into()))?;
    let below = g.content_bottom().map_or(lane.line_top(), |b| b + MESSAGE_GAP);
    Ok(umlcraft_model::Point::new(lane.center(), y.unwrap_or(below)))
}

/// Shows the state invariant or termination `element` on its lifeline at height `y`: an
/// invariant with its top there, centered on the line; a termination's cross ends the lifeline
/// there, though not above what happens on it. A lifeline ends once ([`LIFELINE_ENDED`]).
pub fn add_mark(tx: &mut Tx<'_>, diagram: Id, element: Id, y: f32) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let e = model.element(element)?;
    let (lifeline, termination) = match e.kind() {
        ElementKind::StateInvariant(p) => (p.lifeline, false),
        ElementKind::Termination(p) => (p.lifeline, true),
        _ => return Err(ModelError::Invalid(NOT_ON_LIFELINE.into())),
    };
    let lane =
        lane_of(view, lifeline).ok_or_else(|| ModelError::Invalid(NOT_ON_LIFELINE.into()))?;
    let g = Geometry::read(model, diagram)?;
    let lane = *g.lane(lane).ok_or(ModelError::NotFound(lane))?;
    let bounds = if termination {
        if lane.destroyed || g.termination(lane.id).is_some() {
            return Err(ModelError::Invalid(LIFELINE_ENDED.into()));
        }
        // `fit_lifelines` keeps it below what is on the lifeline.
        let y = y.max(lane.line_top() + MESSAGE_GAP);
        Rect::new(lane.center() - CROSS / 2.0, y - CROSS / 2.0, CROSS, CROSS)
    } else {
        let width = INVARIANT_WIDTH;
        let y = y.max(lane.line_top());
        Rect::new(lane.center() - width / 2.0, y, width, INVARIANT_HEIGHT)
    };
    let id = tx.add_presentation(diagram, Presentation::node(Some(element), bounds))?;
    tidy(tx, diagram)?;
    Ok(id)
}

/// Puts a duration constraint between the message presentations `source` and `target`, or a
/// time constraint on `source` when both are the same, and shows it. The element is unnamed:
/// its name is the constraint. Refused for anything but messages ([`NOT_ON_MESSAGE`]). Returns
/// the new presentation.
pub fn constrain(tx: &mut Tx<'_>, diagram: Id, source: Id, target: Id) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let message = |p: Id| {
        view.get(p)
            .filter(|p| matches!(p.shape, Shape::Message { .. }))
            .and_then(|p| p.element)
            .ok_or_else(|| ModelError::Invalid(NOT_ON_MESSAGE.into()))
    };
    let (from, to) = (message(source)?, message(target)?);
    let kind = if from == to {
        ElementKind::TimeConstraint(TimeConstraintProps { message: from })
    } else {
        ElementKind::DurationConstraint(DurationConstraintProps { from, to })
    };
    let interaction = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
    let element = tx.create(interaction, umlcraft_model::ElementData::new("", kind))?;
    add_constraint(tx, diagram, element)
}

/// Shows the duration or time constraint `element` on `diagram`, which must show its
/// messages. A duration's line goes through the middle of its first message, as Astah puts it.
pub fn add_constraint(tx: &mut Tx<'_>, diagram: Id, element: Id) -> Result<Id> {
    let model = tx.model();
    let g = Geometry::read(model, diagram)?;
    let shown = |m: Id| g.message_of(m).ok_or_else(|| ModelError::Invalid(NOT_ON_MESSAGE.into()));
    let bounds = match model.element(element)?.kind() {
        ElementKind::DurationConstraint(p) => {
            let (a, b) = (*shown(p.from)?, *shown(p.to)?);
            let center = |lane: Id| g.lane(lane).map_or(0.0, |l| l.center());
            let x = if a.source == a.target {
                center(a.source) + crate::sequence::BAR / 2.0 + 20.0
            } else {
                (center(a.source) + center(a.target)) / 2.0
            };
            duration_bounds(x, a.y, b.y)
        }
        ElementKind::TimeConstraint(p) => time_bounds(&g, shown(p.message)?),
        _ => return Err(ModelError::Invalid(NOT_ON_MESSAGE.into())),
    };
    let id = tx.add_presentation(diagram, Presentation::node(Some(element), bounds))?;
    tidy(tx, diagram)?;
    Ok(id)
}

/// The node of a duration constraint: its line at `x` from one message's height to the other's,
/// as wide as the bars at its ends.
fn duration_bounds(x: f32, a: f32, b: f32) -> Rect {
    Rect::new(x - CROSS / 2.0, a.min(b), CROSS, (a - b).abs())
}

/// Where a time constraint's line goes, roughly: on from the arrowhead of `m`, away from the
/// sender. The layout places it exactly.
fn time_bounds(g: &Geometry, m: &Msg) -> Rect {
    let (tip, end) = time_tick(g, m);
    Rect::new(tip.min(end), m.end() - CONSTRAINT_LABEL / 2.0, TIME_TICK, CONSTRAINT_LABEL)
}

/// Roughly where a time constraint's line on `m` starts and ends: at the arrowhead and 30 on,
/// away from the sender.
fn time_tick(g: &Geometry, m: &Msg) -> (f32, f32) {
    let center = |lane: Id| g.lane(lane).map_or(0.0, |l| l.center());
    let (from, to) = (center(m.source), center(m.target));
    let dir = if m.source != m.target && to >= from { 1.0 } else { -1.0 };
    let tip = to - dir * crate::sequence::BAR / 2.0;
    (tip, tip + dir * TIME_TICK)
}

/// Roughly where the label of the constraint `c` goes, measured by Java's widths, for the
/// diagram's frame to hold it.
fn constraint_label(model: &Model, g: &Geometry, c: &Constraint) -> Option<Rect> {
    use crate::sequence::ConstraintAnchor;
    let e = model.get(c.element)?;
    let text = crate::sequence::constraint_text(e)?;
    let size = crate::scene::FONT_SIZE;
    let width = crate::metrics::java_width(&text, size, false).unwrap_or_else(|| {
        crate::scene::TextMeasure::width(&crate::scene::ApproxMeasure, &text, size, false)
    });
    let anchor = match e.kind() {
        ElementKind::DurationConstraint(_) => {
            ConstraintAnchor::Duration { x: c.rect.center().x, middle: c.rect.center().y }
        }
        ElementKind::TimeConstraint(p) => {
            let m = g.message_of(p.message)?;
            let (tip, end) = time_tick(g, m);
            ConstraintAnchor::Time {
                end: umlcraft_model::Point::new(end, m.end()),
                left: end < tip,
            }
        }
        _ => return None,
    };
    Some(crate::sequence::constraint_label_box(anchor, width, c.label))
}

/// Moves the label of the duration or time constraint presentation `constraint` by `by` from
/// where it is, and lets the diagram's frame hold it.
pub fn move_constraint_label(
    tx: &mut Tx<'_>,
    diagram: Id,
    constraint: Id,
    by: umlcraft_model::Point,
) -> Result<()> {
    tx.update_presentation(diagram, constraint, |p| {
        let was = p.style.name_offset.unwrap_or_default();
        let to = umlcraft_model::Point::new(was.x + by.x, was.y + by.y);
        p.style.name_offset = (to.x.abs() > 0.01 || to.y.abs() > 0.01).then_some(to);
    })?;
    tidy(tx, diagram)
}

/// The lifelines a combined fragment or interaction use covers, by its `covered` list.
fn covered_of(model: &Model, element: Id) -> Vec<Id> {
    match model.get(element).map(|e| e.kind()) {
        Some(ElementKind::CombinedFragment(p)) => p.covered.clone(),
        Some(ElementKind::InteractionUse(p)) => p.covered.clone(),
        _ => Vec::new(),
    }
}

/// Spans the frames of the combined fragment or interaction use `element` across the lifelines
/// it now covers (all of them when it names none), on every diagram that shows it. Called when
/// the user changes what it covers; imported frames otherwise keep their width.
pub fn cover(tx: &mut Tx<'_>, element: Id) -> Result<()> {
    let model = tx.model();
    let covered = covered_of(model, element);
    let mut spans = Vec::new();
    for (diagram, view) in model.views() {
        let Some(p) = view.presentations_of(element).next() else { continue };
        let g = Geometry::read(model, diagram)?;
        let mut lanes: Vec<&Lane> =
            covered.iter().filter_map(|&l| lane_of(view, l)).filter_map(|l| g.lane(l)).collect();
        if lanes.is_empty() {
            lanes = g.lanes.iter().collect();
        }
        let left = lanes.iter().map(|l| l.head.x).reduce(f32::min);
        let right = lanes.iter().map(|l| l.head.right()).reduce(f32::max);
        if let (Some(left), Some(right)) = (left, right) {
            spans.push((diagram, p.id, left - FRAGMENT_MARGIN, right + FRAGMENT_MARGIN));
        }
    }
    for (diagram, id, left, right) in spans {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Fragment { bounds, .. } | Shape::Node { bounds, .. } = &mut p.shape {
                (bounds.x, bounds.width) = (left, right - left);
            }
        })?;
        tidy(tx, diagram)?;
    }
    Ok(())
}

/// Makes room for a new last operand in the frames of `fragment`.
pub fn add_operand_space(tx: &mut Tx<'_>, fragment: Id) -> Result<()> {
    let shown: Vec<(Id, Id)> = tx
        .model()
        .views()
        .flat_map(|(diagram, view)| view.presentations_of(fragment).map(move |p| (diagram, p.id)))
        .collect();
    for (diagram, id) in shown {
        // What follows the fragment goes down, so that the new operand does not take it in.
        let bottom = tx.model().view(diagram).and_then(|v| v.get(id)).and_then(|p| p.bounds());
        if let Some(bounds) = bottom {
            make_room(tx, diagram, bounds.bottom(), OPERAND_HEIGHT)?;
        }
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Fragment { bounds, separators, .. } = &mut p.shape {
                separators.push(bounds.height);
                bounds.height += OPERAND_HEIGHT;
            }
        })?;
        tidy(tx, diagram)?;
    }
    Ok(())
}

/// Moves what starts at or below `at` on `diagram` down by `dy`: messages (with their labels),
/// bars, frames, interaction uses, notes and free texts. Bars and frames that reach across
/// `at` grow by `dy`, and the operand lines of such a frame below `at` go down with the rest.
pub fn make_room(tx: &mut Tx<'_>, diagram: Id, at: f32, dy: f32) -> Result<()> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let below = |y: f32| y >= at - 0.01;
    let mut changed: Vec<(Id, Shape)> = Vec::new();
    for p in view.presentations() {
        let mut shape = p.shape.clone();
        match &mut shape {
            Shape::Message { y, end_y, .. } if below(*y) => {
                *y += dy;
                *end_y = end_y.map(|e| e + dy);
            }
            Shape::Activation { top, height, .. } if below(*top) => *top += dy,
            Shape::Activation { top, height, .. } if *top + *height > at => *height += dy,
            Shape::Fragment { bounds, .. } if below(bounds.y) => bounds.y += dy,
            Shape::Fragment { bounds, separators, .. } if bounds.bottom() > at => {
                for s in separators.iter_mut().filter(|s| below(bounds.y + **s)) {
                    *s += dy;
                }
                bounds.height += dy;
            }
            // Lifeline heads and the diagram's frame stay where they are.
            Shape::Node { bounds, .. } | Shape::Text { bounds, .. }
                if below(bounds.y) && p.element != Some(diagram) =>
            {
                bounds.y += dy;
            }
            _ => continue,
        }
        changed.push((p.id, shape));
    }
    for (id, shape) in changed {
        tx.update_presentation(diagram, id, |p| p.shape = shape)?;
    }
    Ok(())
}

/// Deletes `operand` with the line above it (below it for the first one) and its guard's place
/// on every diagram that shows its fragment. The last operand takes the fragment along: a
/// combined fragment has at least one.
pub fn delete_operand(tx: &mut Tx<'_>, operand: Id) -> Result<()> {
    let model = tx.model();
    let Some(fragment) = model.element(operand)?.owner() else { return Ok(()) };
    let operands: Vec<Id> = model
        .children(fragment)
        .iter()
        .copied()
        .filter(|&c| {
            matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::InteractionOperand(_)))
        })
        .collect();
    if operands == [operand] {
        return tx.delete(fragment);
    }
    let index = operands.iter().position(|&o| o == operand).unwrap_or(0);
    let shown: Vec<(Id, Id)> = model
        .views()
        .flat_map(|(diagram, view)| view.presentations_of(fragment).map(move |p| (diagram, p.id)))
        .collect();
    tx.delete(operand)?;
    for (diagram, id) in shown {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Fragment { separators, guards, .. } = &mut p.shape {
                if !separators.is_empty() {
                    separators.remove(index.saturating_sub(1).min(separators.len() - 1));
                }
                if index < guards.len() {
                    guards.remove(index);
                }
            }
        })?;
    }
    Ok(())
}

/// Puts the name label of the message presentation `message` with its top left corner at
/// `top_left`. It is kept relative to the sender's lifeline at the message's height, so it moves
/// along with both.
pub fn place_label(
    tx: &mut Tx<'_>,
    diagram: Id,
    message: Id,
    top_left: umlcraft_model::Point,
) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let Some(m) = g.messages.iter().find(|m| m.id == message) else { return Ok(()) };
    let Some(lane) = g.lane(m.source) else { return Ok(()) };
    let offset = umlcraft_model::Point::new(top_left.x - lane.center(), top_left.y - m.y);
    tx.update_presentation(diagram, message, |p| {
        if let Shape::Message { labels, .. } = &mut p.shape {
            labels.name = Some(offset);
        }
    })
}

/// Puts the open end of the found or lost message `message` at `x`: farther from its lifeline
/// or closer, on either side, though not closer than `MIN_GATE_DISTANCE`.
pub fn set_gate(tx: &mut Tx<'_>, diagram: Id, message: Id, x: f32) -> Result<()> {
    let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let (lane, old) = match view.get(message).map(|p| &p.shape) {
        Some(Shape::Message { source, gate_x: Some(old), .. }) => (*source, *old),
        _ => return Ok(()),
    };
    let Some(Shape::Lifeline { head, .. }) = view.get(lane).map(|p| &p.shape) else {
        return Ok(());
    };
    let center = head.center().x;
    // Right on the lifeline, it keeps its side.
    let side = if (x - center).abs() > 0.5 { x - center } else { old - center };
    let x = center + side.signum() * (x - center).abs().max(MIN_GATE_DISTANCE);
    tx.update_presentation(diagram, message, |p| {
        if let Shape::Message { gate_x, .. } = &mut p.shape {
            *gate_x = Some(x);
        }
    })?;
    tidy(tx, diagram)
}

/// Where the lifelines of all sequence diagrams stand, by diagram and presentation: the x of
/// their line, taken before an edit for [`follow_lifelines`].
pub fn lifeline_centers(model: &Model) -> HashMap<(Id, Id), f32> {
    model
        .views()
        .flat_map(|(diagram, view)| {
            view.presentations().iter().filter_map(move |p| match p.shape {
                Shape::Lifeline { head, .. } => Some(((diagram, p.id), head.center().x)),
                _ => None,
            })
        })
        .collect()
}

/// Moves the open ends of found and lost messages along with their lifelines, which stood
/// where `before` says before the edit. An end on the diagram frame stays there.
pub fn follow_lifelines(tx: &mut Tx<'_>, before: &HashMap<(Id, Id), f32>) -> Result<()> {
    let mut moved = Vec::new();
    for (diagram, view) in tx.model().views() {
        let frame = view
            .presentations()
            .iter()
            .find(|p| p.element == Some(diagram))
            .and_then(|p| p.bounds());
        for p in view.presentations() {
            let Shape::Message { source, gate_x: Some(x), .. } = p.shape else { continue };
            let (Some(&old), Some(Shape::Lifeline { head, .. })) =
                (before.get(&(diagram, source)), view.get(source).map(|l| &l.shape))
            else {
                continue;
            };
            let dx = head.center().x - old;
            let on_frame =
                frame.is_some_and(|f| (x - f.x).abs() < 1.5 || (x - f.right()).abs() < 1.5);
            if dx.abs() > 0.01 && !on_frame {
                moved.push((diagram, p.id, x + dx));
            }
        }
    }
    for (diagram, id, x) in moved {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Message { gate_x, .. } = &mut p.shape {
                *gate_x = Some(x);
            }
        })?;
    }
    Ok(())
}

/// Lets the lifeline `presentation` end at `bottom`, though not above the last message, bar or
/// fragment it reaches, and tidies the diagram. A destroyed lifeline ends at its cross and
/// stays as it is.
pub fn set_lifeline_end(tx: &mut Tx<'_>, diagram: Id, presentation: Id, bottom: f32) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let lane = g.lane(presentation).ok_or(ModelError::NotFound(presentation))?;
    if lane.destroyed {
        return Ok(());
    }
    // A terminated one takes its cross along.
    if let Some(t) = g.termination(lane.id) {
        let rect = t.rect.translate(0.0, bottom - t.y());
        tx.set_bounds(diagram, t.id, rect)?;
    }
    let length = (bottom - lane.line_top()).max(RECEIVER_BAR);
    tx.update_presentation(diagram, presentation, |p| {
        if let Shape::Lifeline { length: l, .. } = &mut p.shape {
            *l = length;
        }
    })?;
    tidy(tx, diagram)
}

/// Moves a message, together with the bar it starts and everything sent from it, or a
/// fragment's frame, so that it starts at height `y`.
pub fn move_item(tx: &mut Tx<'_>, diagram: Id, element: Id, y: f32) -> Result<()> {
    let model = tx.model();
    let g = Geometry::read(model, diagram)?;
    if let Some(frame) = g.frames.iter().find(|f| f.element == element) {
        let dy = y - frame.rect.y;
        let id = frame.id;
        tx.update_presentation(diagram, id, |p| match &mut p.shape {
            Shape::Fragment { bounds, .. } | Shape::Node { bounds, .. } => bounds.y += dy,
            _ => {}
        })?;
        return tidy(tx, diagram);
    }
    let Some(m) = g.messages.iter().find(|m| m.element == Some(element)).copied() else {
        return Ok(());
    };
    // The bar the message starts and, from the inside out, what lies within it.
    let mut bars: HashSet<Id> = HashSet::new();
    let mut messages: HashSet<Id> = HashSet::from([m.id]);
    // The bars a message starts. A create message takes the head along, and with it the
    // constructor's bar right below.
    let started = |msg: &Msg| -> Vec<Bar> {
        let below_head = |b: &Bar| {
            msg.sort == MessageSort::Create
                && g.lane(msg.target).is_some_and(|l| (b.top - l.line_top()).abs() < 0.5)
        };
        g.bars
            .iter()
            .filter(|b| b.lane == msg.target && ((b.top - msg.end()).abs() < 0.5 || below_head(b)))
            .copied()
            .collect()
    };
    let mut queue: Vec<Bar> = started(&m);
    while let Some(bar) = queue.pop() {
        if !bars.insert(bar.id) {
            continue;
        }
        // The messages that start the bar come along, so a call into a busy lifeline keeps
        // its bar.
        for other in &g.messages {
            if other.target == bar.lane && (bar.top - other.end()).abs() < 0.5 {
                messages.insert(other.id);
            }
        }
        for b in &g.bars {
            if b.lane == bar.lane && b.id != bar.id && bar.top < b.top && b.top <= bar.bottom {
                queue.push(*b);
            }
        }
        for other in &g.messages {
            let leaves =
                other.source == bar.lane && bar.top - 0.5 <= other.y && other.y <= bar.bottom + 0.5;
            if leaves && messages.insert(other.id) {
                queue.extend(started(other));
            }
        }
    }
    let dy = movable(&g, &bars, &messages, y - m.y);
    if dy.abs() < 0.01 {
        return Ok(());
    }
    // The bar the message was sent from, unless it moves along.
    let gate = match tx.model().get(element).map(|e| e.kind()) {
        Some(ElementKind::Message(p)) => p.gate,
        _ => MessageGate::None,
    };
    let sends = m.sort != MessageSort::Reply && gate != MessageGate::Found;
    let left = g.bar_at(m.source, m.y).filter(|b| sends && !bars.contains(&b.id));
    // The bar a reply leaves, which ends where the reply now starts.
    let returned = g.bar_at(m.source, m.y).filter(|_| m.sort == MessageSort::Reply);
    for id in bars {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Activation { top, .. } = &mut p.shape {
                *top += dy;
            }
        })?;
    }
    // Create messages that move take the heads they made along.
    let made: Vec<Lane> = g
        .messages
        .iter()
        .filter(|mm| messages.contains(&mm.id) && mm.sort == MessageSort::Create)
        .filter_map(|mm| g.lane(mm.target).copied())
        .collect();
    for id in messages {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Message { y, end_y, .. } = &mut p.shape {
                *y += dy;
                *end_y = end_y.map(|e| e + dy);
            }
        })?;
    }
    for to in made {
        let bottom = to.bottom();
        tx.update_presentation(diagram, to.id, |p| {
            if let Shape::Lifeline { head, length, .. } = &mut p.shape {
                head.y += dy;
                *length = (bottom - head.bottom()).max(RECEIVER_BAR + BAR_MARGIN);
            }
        })?;
    }
    // The bar it left keeps only what is still in it; bars otherwise only grow.
    if let Some(bar) = left {
        shrink_bar(tx, diagram, bar)?;
    }
    if let Some(bar) = returned {
        end_at_reply(tx, diagram, bar, m.id)?;
    }
    // Moved off its sender's bar, a message gets a bar there, as `add_message` gives it: the
    // one ending just above goes on, else a new one starts. Replies leave the bar they end and
    // found messages have no sender.
    if sends {
        let moved = Geometry::read(tx.model(), diagram)?;
        let height = match (gate, m.sort) {
            (MessageGate::Lost, _) => LOST_SENDER_BAR,
            (_, MessageSort::Create) => CREATE_SENDER_BAR,
            _ => SENDER_BAR,
        };
        sender_bar(tx, diagram, &moved, m.source, m.y + dy, height)?;
    }
    tidy(tx, diagram)
}

/// How far of `dy` the `bars` and `messages` that move can go: they stay below the heads of
/// their lifelines and above the cross of a destroyed one, and a create message, which takes
/// the head along, stays above whatever else happens on its lifeline. Where something lies out
/// of bounds already, it may stay there.
fn movable(g: &Geometry, bars: &HashSet<Id>, messages: &HashSet<Id>, dy: f32) -> f32 {
    let (mut least, mut most) = (f32::MIN, f32::MAX);
    let moving = || g.messages.iter().filter(|mm| messages.contains(&mm.id));
    let made: Vec<Id> =
        moving().filter(|mm| mm.sort == MessageSort::Create).map(|mm| mm.target).collect();
    for mm in moving() {
        for (lane, y) in [(mm.source, mm.y), (mm.target, mm.end())] {
            let Some(l) = g.lane(lane) else { continue };
            if mm.sort == MessageSort::Create && lane == mm.target {
                continue;
            }
            least = least.max(l.line_top() + MESSAGE_GAP - y);
            let cross = g.messages.iter().find(|d| {
                d.sort == MessageSort::Destroy && d.target == lane && !messages.contains(&d.id)
            });
            if let Some(d) = cross {
                most = most.min(d.end() - MESSAGE_GAP - y);
            }
            if let Some(t) = g.termination(lane) {
                most = most.min(t.y() - MESSAGE_GAP - y);
            }
        }
    }
    // A destroy message stays below what else happens on the lifeline it ends.
    for mm in moving().filter(|mm| mm.sort == MessageSort::Destroy) {
        let lane = mm.target;
        let others = g
            .messages
            .iter()
            .filter(|o| !messages.contains(&o.id) && (o.source == lane || o.target == lane))
            .map(|o| if o.source == lane { o.y } else { o.end() });
        let tops = g
            .bars
            .iter()
            .filter(|b| b.lane == lane && !bars.contains(&b.id) && (b.top - mm.end()).abs() > 0.5)
            .map(|b| b.top);
        if let Some(last) = others.chain(tops).reduce(f32::max) {
            least = least.max(last + MESSAGE_GAP - mm.end());
        }
    }
    for l in made.iter().filter_map(|&lane| g.lane(lane)) {
        let on_lane = |mm: &&Msg| {
            !messages.contains(&mm.id)
                && (mm.source == l.id || (mm.target == l.id && mm.sort != MessageSort::Create))
        };
        let others = g
            .messages
            .iter()
            .filter(on_lane)
            .map(|mm| if mm.source == l.id { mm.y } else { mm.end() });
        let bars = g.bars.iter().filter(|b| b.lane == l.id && !bars.contains(&b.id));
        if let Some(first) = others.chain(bars.map(|b| b.top)).reduce(f32::min) {
            most = most.min(first - MESSAGE_GAP - l.line_top());
        }
    }
    dy.max(least.min(0.0)).min(most.max(0.0))
}

/// Shortens `bar` (as it was before an edit) to what still lies in it: down to the height a new
/// bar gets, 20 below the last message or bar inside. A bar a reply ends keeps its end.
fn shrink_bar(tx: &mut Tx<'_>, diagram: Id, bar: Bar) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let lane = bar.lane;
    let inside = |y: f32| bar.top + 0.5 < y && y <= bar.bottom + 0.5;
    let replied = g.messages.iter().any(|m| {
        m.sort == MessageSort::Reply && m.source == lane && (m.y - bar.bottom).abs() < 0.5
    });
    if replied {
        return Ok(());
    }
    // A bar a call starts is a receiver's; any other a sender's.
    let called = g.messages.iter().any(|m| m.target == lane && (bar.top - m.end()).abs() < 0.5);
    let mut need = bar.top + if called { RECEIVER_BAR } else { SENDER_BAR };
    for m in &g.messages {
        if (m.source == lane && inside(m.y)) || (m.target == lane && inside(m.end())) {
            need = need.max(m.y.max(m.end()) + BAR_MARGIN);
        }
    }
    for b in g.bars.iter().filter(|b| b.lane == lane && b.id != bar.id && inside(b.top)) {
        need = need.max(b.bottom + BAR_MARGIN);
    }
    if need >= bar.bottom - 0.01 {
        return Ok(());
    }
    tx.update_presentation(diagram, bar.id, |p| {
        if let Shape::Activation { height, .. } = &mut p.shape {
            *height = need - bar.top;
        }
    })
}

/// Lets `bar` (as it was before an edit) end at the reply `reply` moved up inside it, unless
/// something else in the bar still needs room below.
fn end_at_reply(tx: &mut Tx<'_>, diagram: Id, bar: Bar, reply: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let Some(r) = g.messages.iter().find(|m| m.id == reply) else { return Ok(()) };
    if r.y <= bar.top + 0.5 || r.y >= bar.bottom - 0.01 {
        return Ok(());
    }
    let lane = bar.lane;
    let inside = |y: f32| bar.top + 0.5 < y && y <= bar.bottom + 0.5;
    let mut need = r.y;
    for m in g.messages.iter().filter(|m| m.id != reply) {
        if (m.source == lane && inside(m.y)) || (m.target == lane && inside(m.end())) {
            need = need.max(m.y.max(m.end()) + BAR_MARGIN);
        }
    }
    for b in g.bars.iter().filter(|b| b.lane == lane && b.id != bar.id && inside(b.top)) {
        need = need.max(b.bottom + BAR_MARGIN);
    }
    if need >= bar.bottom - 0.01 {
        return Ok(());
    }
    tx.update_presentation(diagram, bar.id, |p| {
        if let Shape::Activation { height, .. } = &mut p.shape {
            *height = need - bar.top;
        }
    })
}

/// Grows bars, lifelines and the diagram frame around their content and puts the
/// interaction's messages and fragments in the order and operands they are drawn in.
pub fn tidy(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    drop_idle_bars(tx, diagram)?;
    fit_bars(tx, diagram)?;
    fit_invariants(tx, diagram)?;
    fit_fragments(tx, diagram)?;
    fit_lifelines(tx, diagram)?;
    place_terminations(tx, diagram)?;
    fit_constraints(tx, diagram)?;
    fit_frame(tx, diagram)?;
    sync_order(tx, diagram)
}

/// Whether two rectangles are the same but for rounding.
fn same_rect(a: Rect, b: Rect) -> bool {
    [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height].iter().all(|d| d.abs() < 0.01)
}

/// Keeps state invariants centered on their lifeline, below its head.
fn fit_invariants(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    for m in g.marks.iter().filter(|m| !m.termination) {
        let Some(lane) = g.lane(m.lane) else { continue };
        let wanted = Rect::new(
            lane.center() - m.rect.width / 2.0,
            m.rect.y.max(lane.line_top()),
            m.rect.width,
            m.rect.height,
        );
        if !same_rect(wanted, m.rect) {
            tx.set_bounds(diagram, m.id, wanted)?;
        }
    }
    Ok(())
}

/// Puts the cross of every termination where its lifeline ends.
fn place_terminations(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    for m in g.marks.iter().filter(|m| m.termination) {
        let Some(lane) = g.lane(m.lane) else { continue };
        let wanted =
            Rect::new(lane.center() - CROSS / 2.0, lane.bottom() - CROSS / 2.0, CROSS, CROSS);
        if !same_rect(wanted, m.rect) {
            tx.set_bounds(diagram, m.id, wanted)?;
        }
    }
    Ok(())
}

/// Lets duration constraints reach from one of their messages to the other where these are
/// now, keeping the line's x, and time constraints go on from their message's arrowhead.
fn fit_constraints(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let mut changed = Vec::new();
    for c in &g.constraints {
        let wanted = match tx.model().get(c.element).map(|e| e.kind()) {
            Some(ElementKind::DurationConstraint(p)) => {
                let (Some(a), Some(b)) = (g.message_of(p.from), g.message_of(p.to)) else {
                    continue;
                };
                duration_bounds(c.rect.center().x, a.y, b.y)
            }
            Some(ElementKind::TimeConstraint(p)) => {
                let Some(m) = g.message_of(p.message) else { continue };
                time_bounds(&g, m)
            }
            _ => continue,
        };
        if !same_rect(wanted, c.rect) {
            changed.push((c.id, wanted));
        }
    }
    for (id, rect) in changed {
        tx.set_bounds(diagram, id, rect)?;
    }
    Ok(())
}

/// Tidies every sequence diagram, e.g. after deleting elements that may have been shown there.
/// A diagram that cannot be tidied (a broken file, say) stays as it is rather than failing an
/// edit elsewhere.
pub fn tidy_all(tx: &mut Tx<'_>) -> Result<()> {
    let diagrams: Vec<Id> = tx
        .model()
        .elements()
        .filter(|e| e.kind().as_diagram() == Some(umlcraft_model::DiagramKind::Sequence))
        .map(|e| e.id())
        .collect();
    for d in diagrams {
        // Skipped on purpose: the edit that asked for the tidy-up is not about this diagram.
        let _ = tx.attempt(|tx| tidy(tx, d));
    }
    Ok(())
}

/// Removes bars that no message reaches any more, as Astah removes a bar with the message that
/// started it. A create message keeps the bar right below the head of the lifeline it made.
fn drop_idle_bars(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let touches = |bar: &Bar, lane: Id, y: f32| {
        lane == bar.lane && bar.top - 0.5 <= y && y <= bar.bottom + 0.5
    };
    let constructs = |bar: &Bar, m: &Msg| {
        m.sort == MessageSort::Create
            && m.target == bar.lane
            && g.lanes.iter().any(|l| l.id == bar.lane && (bar.top - l.line_top()).abs() < 0.5)
    };
    let idle: indexmap::IndexSet<Id> = g
        .bars
        .iter()
        .filter(|bar| {
            !g.messages.iter().any(|m| {
                touches(bar, m.source, m.y) || touches(bar, m.target, m.end()) || constructs(bar, m)
            })
        })
        .map(|bar| bar.id)
        .collect();
    if idle.is_empty() { Ok(()) } else { tx.remove_presentations(diagram, idle) }
}

fn fit_bars(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    // Growing one bar can make room needed in a bar around it; a few rounds settle it.
    for _ in 0..16 {
        let g = Geometry::read(tx.model(), diagram)?;
        let mut grown = Vec::new();
        for bar in &g.bars {
            let inside = |y: f32| bar.top + 0.5 < y && y <= bar.bottom + 0.5;
            let mut need = bar.bottom;
            for b in g.bars.iter().filter(|b| b.lane == bar.lane && b.id != bar.id) {
                if inside(b.top) {
                    need = need.max(b.bottom + BAR_MARGIN);
                }
            }
            for m in &g.messages {
                // A reply leaves its own bar at the end; anything else on the lifeline inside
                // the bar needs room below.
                let own_reply = m.sort == MessageSort::Reply
                    && m.source == bar.lane
                    && (m.y - bar.bottom).abs() < 0.5
                    && g.bar_at(bar.lane, m.y).is_some_and(|b| b.id == bar.id);
                if own_reply {
                    continue;
                }
                if (m.source == bar.lane && inside(m.y))
                    || (m.target == bar.lane && inside(m.end()))
                {
                    need = need.max(m.y.max(m.end()) + BAR_MARGIN);
                }
                // A reply returns into the bar its call was sent from, however far below. A lost
                // one returns nowhere.
                if m.sort == MessageSort::Reply
                    && m.target == bar.lane
                    && m.source != m.target
                    && m.end() > bar.bottom
                {
                    let call = g
                        .messages
                        .iter()
                        .filter(|c| c.sort != MessageSort::Reply && c.y < m.y)
                        .filter(|c| c.source == m.target && c.target == m.source)
                        .max_by(|a, b| a.y.total_cmp(&b.y));
                    if call.is_some_and(|c| g.bar_at(bar.lane, c.y).is_some_and(|b| b.id == bar.id))
                    {
                        need = need.max(m.end() + BAR_MARGIN);
                    }
                }
                // And it leaves the bar its call arrived in at its end, however far below.
                if m.sort == MessageSort::Reply && m.source == bar.lane && m.y > bar.bottom {
                    let call = g
                        .messages
                        .iter()
                        .filter(|c| c.sort != MessageSort::Reply && c.end() < m.y)
                        .filter(|c| c.source == m.target && c.target == m.source)
                        .max_by(|a, b| a.y.total_cmp(&b.y));
                    let arrived =
                        |c: &&Msg| g.bar_at(bar.lane, c.end()).is_some_and(|b| b.id == bar.id);
                    if call.as_ref().is_some_and(arrived) {
                        need = need.max(m.y);
                    }
                }
                // A caller waits for its call, reply drawn or not: its bar lasts until the bar
                // the call started has ended. A call to itself nests, as above.
                if m.sort == MessageSort::Synchronous
                    && m.source == bar.lane
                    && m.target != bar.lane
                    && g.bar_at(bar.lane, m.y).is_some_and(|b| b.id == bar.id)
                    && let Some(called) =
                        g.bars.iter().find(|b| b.lane == m.target && (b.top - m.end()).abs() < 0.5)
                {
                    need = need.max(called.bottom);
                }
            }
            // Nothing lasts beyond a lifeline's cross: bars there end with the bar the destroy
            // message starts, which holds the cross.
            let need = match g.cross(bar.lane) {
                Some((destroy_bar, cross)) if destroy_bar != Some(bar.id) && bar.top < cross => {
                    need.min(cross)
                }
                _ => need,
            };
            if (need - bar.bottom).abs() > 0.01 {
                grown.push((bar.id, need - bar.top));
            }
        }
        if grown.is_empty() {
            return Ok(());
        }
        for (id, height) in grown {
            tx.update_presentation(diagram, id, |p| {
                if let Shape::Activation { height: h, .. } = &mut p.shape {
                    *h = height;
                }
            })?;
        }
    }
    Ok(())
}

/// Grows combined fragments to hold the bars, messages and frames that start inside them:
/// Astah refuses a bar that reaches out of the fragment its message lies in. Operand lines go
/// down below the bars of the operand above where there is room.
fn fit_fragments(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    // Inner fragments first, so that outer ones grow around them.
    for _ in 0..8 {
        let g = Geometry::read(tx.model(), diagram)?;
        let center = |lane: Id| g.lane(lane).map_or(f32::MIN, |l| l.center());
        let mut grown = Vec::new();
        let mut moved = Vec::new();
        for f in &g.frames {
            if !matches!(
                tx.model().get(f.element).map(|e| e.kind()),
                Some(ElementKind::CombinedFragment(_))
            ) {
                continue;
            }
            let r = f.rect;
            let inside = |x: f32, y: f32| r.y < y && y < r.bottom() && r.x <= x && x <= r.right();
            let bars = g.bars.iter().filter(|b| inside(center(b.lane), b.top)).map(|b| b.bottom);
            // As in `sync_order`: a message is inside when both of its ends are, so one that
            // crosses the frame's side does not grow the fragment it is not put into.
            let messages = g
                .messages
                .iter()
                .filter(|m| inside(center(m.source), m.y) && inside(center(m.target), m.y))
                .map(|m| m.y.max(m.end()));
            let frames = g
                .frames
                .iter()
                .filter(|o| o.id != f.id && inside(o.rect.x, o.rect.y))
                .map(|o| o.rect.bottom());
            let invariants = g
                .marks
                .iter()
                .filter(|m| !m.termination && inside(center(m.lane), m.rect.y))
                .map(|m| m.rect.bottom());
            let need = bars
                .chain(messages)
                .chain(frames)
                .chain(invariants)
                .fold(r.bottom(), |n, b| n.max(b + FRAGMENT_PADDING));
            // The last operand keeps room below its line, e.g. when the frame is made smaller.
            let need = f.separators.last().map_or(need, |s| need.max(s + NEXT / 2.0));
            if need > r.bottom() + 0.01 {
                grown.push((f.id, need - r.y));
            }
            // A bar stays in its operand too: the line below goes down to clear it, as long as
            // the next operand keeps room for what lies in it and for a message's label.
            let mut separators = f.separators.clone();
            for k in 0..separators.len() {
                let above = if k == 0 { r.y } else { separators[k - 1] };
                let below = separators.get(k + 1).copied().unwrap_or(r.bottom());
                let s = separators[k];
                let need = g
                    .bars
                    .iter()
                    .filter(|b| inside(center(b.lane), b.top) && above < b.top && b.top < s)
                    .map(|b| b.bottom + FRAGMENT_PADDING)
                    .fold(s, f32::max);
                if need <= s + 0.01 {
                    continue;
                }
                let next = g
                    .messages
                    .iter()
                    .filter(|m| inside(center(m.source), m.y) && inside(center(m.target), m.y))
                    .map(|m| m.y - NEXT / 2.0)
                    .chain(g.bars.iter().filter(|b| inside(center(b.lane), b.top)).map(|b| b.top))
                    .chain(g.frames.iter().filter(|o| inside(o.rect.x, o.rect.y)).map(|o| o.rect.y))
                    .filter(|&y| y >= s)
                    .fold(below - NEXT / 2.0, f32::min);
                if need <= next + 0.01 {
                    separators[k] = need;
                }
            }
            if separators != f.separators {
                moved.push((f.id, separators.iter().map(|s| s - r.y).collect::<Vec<_>>()));
            }
        }
        if grown.is_empty() && moved.is_empty() {
            return Ok(());
        }
        for (id, height) in grown {
            tx.update_presentation(diagram, id, |p| {
                if let Shape::Fragment { bounds, .. } = &mut p.shape {
                    bounds.height = height;
                }
            })?;
        }
        for (id, lines) in moved {
            tx.update_presentation(diagram, id, |p| {
                if let Shape::Fragment { separators, .. } = &mut p.shape {
                    *separators = lines;
                }
            })?;
        }
    }
    Ok(())
}

fn fit_lifelines(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    // A lifeline whose destroy message is gone runs on again.
    let destroying =
        |lane: Id| g.messages.iter().any(|m| m.target == lane && m.sort == MessageSort::Destroy);
    for lane in g.lanes.iter().filter(|l| l.destroyed && !destroying(l.id)) {
        tx.update_presentation(diagram, lane.id, |p| {
            if let Shape::Lifeline { destroyed, .. } = &mut p.shape {
                *destroyed = false;
            }
        })?;
    }
    // A destroyed one ends with the bar its destroy message starts, wherever that has moved.
    for lane in g.lanes.iter().filter(|l| l.destroyed) {
        let Some((_, end)) = g.cross(lane.id) else { continue };
        // Above the head the lifeline keeps at least the bar's length, as `add_message` gives it.
        let length = (end - lane.line_top()).max(RECEIVER_BAR);
        if (lane.bottom() - lane.line_top() - length).abs() > 0.01 {
            tx.update_presentation(diagram, lane.id, |p| {
                if let Shape::Lifeline { length: l, .. } = &mut p.shape {
                    *l = length;
                }
            })?;
        }
    }
    for lane in g.lanes.iter().filter(|l| !l.destroyed || !destroying(l.id)) {
        let bars = g.bars.iter().filter(|b| b.lane == lane.id).map(|b| b.bottom);
        let messages = g
            .messages
            .iter()
            .filter(|m| m.source == lane.id || m.target == lane.id)
            .map(|m| m.y.max(m.end()));
        // Lifelines run on below the frames drawn across them.
        let frames = g
            .frames
            .iter()
            .filter(|f| f.rect.x <= lane.center() && lane.center() <= f.rect.right())
            .map(|f| f.rect.bottom());
        let invariants = g.invariants(lane.id).map(|m| m.rect.bottom());
        let last = bars.chain(messages).chain(frames).chain(invariants).reduce(f32::max);
        // A terminated lifeline ends at its cross, below what is on it.
        if let Some(t) = g.termination(lane.id).filter(|_| !lane.destroyed) {
            let end = last
                .map_or(t.y(), |last| t.y().max(last + LIFELINE_MARGIN))
                .max(lane.line_top() + MESSAGE_GAP);
            let length = end - lane.line_top();
            if (lane.length - length).abs() > 0.01 {
                tx.update_presentation(diagram, lane.id, |p| {
                    if let Shape::Lifeline { length: l, .. } = &mut p.shape {
                        *l = length;
                    }
                })?;
            }
            continue;
        }
        let Some(last) = last else { continue };
        let need = last + LIFELINE_MARGIN;
        if need > lane.bottom() + 0.01 {
            let length = need - lane.line_top();
            tx.update_presentation(diagram, lane.id, |p| {
                if let Shape::Lifeline { length: l, .. } = &mut p.shape {
                    *l = length;
                }
            })?;
        }
    }
    Ok(())
}

/// Grows the diagram frame to surround everything, or adds one.
fn fit_frame(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let lanes =
        g.lanes.iter().map(|l| Rect::new(l.head.x, l.head.y, l.head.width, l.bottom() - l.head.y));
    let frames = g.frames.iter().map(|f| f.rect);
    let marks = g.marks.iter().map(|m| m.rect);
    // Constraints with their labels, which a time constraint's carries beyond the lifelines.
    let constraints = g.constraints.iter().flat_map(|c| {
        let label = constraint_label(tx.model(), &g, c);
        std::iter::once(c.rect).chain(label)
    });
    let Some(content) = lanes
        .chain(frames)
        .chain(marks)
        .chain(constraints)
        .chain(g.notes.iter().copied())
        .reduce(|a, b| a.union(&b))
    else {
        return Ok(());
    };
    let heads = g.lanes.iter().map(|l| l.head.y).reduce(f32::min).unwrap_or(content.y);
    let wanted = Rect::from_corners(
        umlcraft_model::Point::new(content.x - FRAME_MARGIN, heads - FRAME_ABOVE_HEADS),
        umlcraft_model::Point::new(content.right() + FRAME_MARGIN, content.bottom() + FRAME_MARGIN),
    );
    match g.diagram_frame {
        None => {
            let frame = Presentation::node(Some(diagram), wanted);
            tx.apply(umlcraft_model::Op::InsertPresentation {
                diagram,
                index: 0,
                presentation: frame,
            })
        }
        Some((id, rect)) => {
            let grown = rect.union(&wanted);
            if grown != rect { tx.set_bounds(diagram, id, grown) } else { Ok(()) }
        }
    }
}

/// Puts the messages, fragments and interaction uses shown on `diagram` into the operand of the
/// innermost fragment drawn around them, in the order they are drawn.
pub fn sync_order(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let model = tx.model();
    let Some(interaction) = model.element(diagram)?.owner() else { return Ok(()) };
    if !matches!(model.element(interaction)?.kind(), ElementKind::Interaction) {
        return Ok(());
    }
    let g = Geometry::read(model, diagram)?;
    let center = |lane: Id| g.lane(lane).map_or(0.0, |l| l.center());
    /// An item with where it is drawn: its height and its horizontal extent; for frames,
    /// their own presentation.
    struct Item {
        element: Id,
        y: f32,
        left: f32,
        right: f32,
        own: Option<Id>,
    }
    let mut items: Vec<Item> = Vec::new();
    for m in &g.messages {
        let Some(element) = m.element else { continue };
        let (a, b) = (center(m.source), center(m.target));
        items.push(Item { element, y: m.y, left: a.min(b), right: a.max(b), own: None });
    }
    for f in &g.frames {
        let (left, right) = (f.rect.x, f.rect.right());
        items.push(Item { element: f.element, y: f.rect.y, left, right, own: Some(f.id) });
    }
    // State invariants and terminations take their place on the lifeline.
    for m in &g.marks {
        let x = center(m.lane);
        items.push(Item { element: m.element, y: m.y(), left: x, right: x, own: None });
    }
    let mut wanted: HashMap<Id, Vec<(f32, Id)>> = HashMap::new();
    for &Item { element, y, left, right, own } in &items {
        let around = g
            .frames
            .iter()
            .filter(|f| Some(f.id) != own && f.element != element)
            .filter(|f| {
                f.rect.y + 0.5 < y
                    && y < f.rect.bottom()
                    && f.rect.x <= left + 0.5
                    && right <= f.rect.right() + 0.5
            })
            .filter(|f| {
                matches!(
                    model.get(f.element).map(|e| e.kind()),
                    Some(ElementKind::CombinedFragment(_))
                )
            })
            .min_by(|a, b| {
                (a.rect.width * a.rect.height).total_cmp(&(b.rect.width * b.rect.height))
            });
        let owner = match around {
            Some(f) => {
                let operands: Vec<Id> = model
                    .children(f.element)
                    .iter()
                    .copied()
                    .filter(|&o| {
                        matches!(
                            model.get(o).map(|e| e.kind()),
                            Some(ElementKind::InteractionOperand(_))
                        )
                    })
                    .collect();
                let k = f.separators.iter().filter(|&&s| s < y).count();
                match operands.get(k.min(operands.len().saturating_sub(1))) {
                    Some(&operand) => operand,
                    None => continue,
                }
            }
            None => interaction,
        };
        wanted.entry(owner).or_default().push((y, element));
    }
    // Owners that may lose items, too.
    let mut owners: Vec<Id> = wanted.keys().copied().collect();
    for item in &items {
        if let Some(owner) = model.get(item.element).and_then(|e| e.owner())
            && !owners.contains(&owner)
        {
            owners.push(owner);
        }
    }
    let shown: HashSet<Id> = items.iter().map(|i| i.element).collect();
    // Each owner's children as they should be: what is not shown here first, in its order,
    // then the shown items as drawn.
    let mut orders: Vec<(Id, Vec<Id>)> = Vec::new();
    for owner in owners {
        let mut list = wanted.remove(&owner).unwrap_or_default();
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let kept = model.children(owner).iter().copied().filter(|c| !shown.contains(c));
        orders.push((owner, kept.chain(list.into_iter().map(|(_, e)| e)).collect()));
    }
    // Into place from the first index on, so that later moves do not disturb earlier ones. A
    // frame drawn inside one it holds in the model waits for a later round, after which the
    // other one has moved out: frames only hold tops lying lower, so the nesting wanted has no
    // cycle.
    for _ in 0..8 {
        let mut waiting = false;
        for (owner, order) in &orders {
            let owner = *owner;
            let mut index = 0;
            for &element in order {
                let model = tx.model();
                if model.children(owner).get(index) == Some(&element) {
                    index += 1;
                    continue;
                }
                if owner == element || model.is_ancestor(element, owner) {
                    waiting = true;
                    continue;
                }
                let current = model.get(element).and_then(|e| e.owner());
                let at =
                    index.min(model.children(owner).len() - usize::from(current == Some(owner)));
                tx.move_element(element, owner, at)?;
                index += 1;
            }
        }
        if !waiting {
            break;
        }
    }
    Ok(())
}

/// Upgrades every sequence diagram of a document just opened (see [`upgrade`]). The upgrade
/// is not an edit of the user's: it cannot be undone and leaves the document unchanged.
pub fn upgrade_document(doc: &mut Document) {
    let diagrams: Vec<Id> = doc
        .model()
        .elements()
        .filter(|e| e.kind().as_diagram() == Some(umlcraft_model::DiagramKind::Sequence))
        .map(|e| e.id())
        .collect();
    let changed = doc.edit("upgrade", |tx| {
        let mut changed = false;
        for &diagram in &diagrams {
            changed |= upgrade(tx, diagram)?;
        }
        Ok(changed)
    });
    if matches!(changed, Ok(true)) {
        doc.clear_history();
        doc.mark_saved();
    }
}

/// Gives a sequence diagram from before UmlCraft stored Astah's geometry its geometry: lifeline
/// nodes become lifelines, message edges messages placed one after the other in the
/// interaction's order, fragment nodes frames around their content. Returns whether anything
/// changed.
pub fn upgrade(tx: &mut Tx<'_>, diagram: Id) -> Result<bool> {
    let model = tx.model();
    let Some(view) = model.view(diagram) else { return Ok(false) };
    let is_lifeline = |p: &Presentation| {
        matches!(p.shape, Shape::Node { .. })
            && p.element
                .and_then(|e| model.get(e))
                .is_some_and(|e| matches!(e.kind(), ElementKind::Lifeline(_)))
    };
    let is_old_message = |p: &Presentation| {
        matches!(p.shape, Shape::Edge { .. })
            && p.element
                .and_then(|e| model.get(e))
                .is_some_and(|e| matches!(e.kind(), ElementKind::Message(_)))
    };
    let is_old_fragment = |p: &Presentation| {
        matches!(p.shape, Shape::Node { .. })
            && p.element.and_then(|e| model.get(e)).is_some_and(|e| {
                matches!(
                    e.kind(),
                    ElementKind::CombinedFragment(_) | ElementKind::InteractionUse(_)
                )
            })
    };
    let old: Vec<Presentation> = view
        .presentations()
        .iter()
        .filter(|p| is_lifeline(p) || is_old_message(p) || is_old_fragment(p))
        .cloned()
        .collect();
    if old.is_empty() {
        return Ok(false);
    }
    let Some(interaction) = model.element(diagram)?.owner() else { return Ok(false) };
    // Old message edges by message, with the lifelines at their ends.
    let lifeline_of: HashMap<Id, Id> =
        old.iter().filter(|p| is_lifeline(p)).filter_map(|p| Some((p.id, p.element?))).collect();
    let ends: HashMap<Id, (Id, Id)> = old
        .iter()
        .filter_map(|p| {
            let (s, t) = p.endpoints()?;
            Some((p.element?, (*lifeline_of.get(&s)?, *lifeline_of.get(&t)?)))
        })
        .collect();
    let shown_fragments: HashSet<Id> =
        old.iter().filter(|p| is_old_fragment(p)).filter_map(|p| p.element).collect();
    let mut lifelines: Vec<(Id, Rect)> = old
        .iter()
        .filter(|p| is_lifeline(p))
        .filter_map(|p| Some((p.element?, p.bounds()?)))
        .collect();
    lifelines.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
    let top = lifelines.iter().map(|l| l.1.y).reduce(f32::min).unwrap_or(40.0);

    tx.remove_presentations(diagram, old.iter().map(|p| p.id).collect())?;
    let mut lanes = HashMap::new();
    for (lifeline, rect) in lifelines {
        let head = Rect::new(
            rect.x,
            top,
            rect.width.max(crate::sequence::HEAD_WIDTH),
            crate::sequence::HEAD_HEIGHT,
        );
        let id =
            tx.add_presentation(diagram, Presentation::lifeline(lifeline, head, LIFELINE_LENGTH))?;
        lanes.insert(lifeline, id);
    }
    let mut y = top + crate::sequence::HEAD_HEIGHT + NEXT;
    place(tx, diagram, interaction, &ends, &lanes, &shown_fragments, &mut y)?;
    tidy(tx, diagram)?;
    Ok(true)
}

/// Lays out the children of `container` from height `y` down, as [`upgrade`] does.
fn place(
    tx: &mut Tx<'_>,
    diagram: Id,
    container: Id,
    ends: &HashMap<Id, (Id, Id)>,
    lanes: &HashMap<Id, Id>,
    fragments: &HashSet<Id>,
    y: &mut f32,
) -> Result<()> {
    let children = tx.model().children(container).to_vec();
    for child in children {
        let kind = tx.model().element(child)?.kind().clone();
        match kind {
            ElementKind::Message(_) => {
                let Some(&(from, to)) = ends.get(&child) else { continue };
                let (Some(&source), Some(&target)) = (lanes.get(&from), lanes.get(&to)) else {
                    continue;
                };
                let id = add_message(tx, diagram, child, source, target, *y)?;
                let placed =
                    match tx.model().view(diagram).and_then(|v| v.get(id)).map(|p| &p.shape) {
                        Some(Shape::Message { y, end_y, .. }) => end_y.unwrap_or(*y),
                        _ => *y,
                    };
                *y = placed.max(*y) + NEXT;
            }
            ElementKind::InteractionUse(p) if fragments.contains(&child) => {
                add_fragment(tx, diagram, child, &p.covered, *y)?;
                *y += USE_HEIGHT + NEXT / 2.0;
            }
            ElementKind::CombinedFragment(p) if fragments.contains(&child) => {
                let top = *y;
                *y += crate::sequence::TAB_HEIGHT + NEXT / 2.0;
                let operands = tx.model().children(child).to_vec();
                let mut separators = Vec::new();
                for (k, operand) in operands.into_iter().enumerate() {
                    if k > 0 {
                        separators.push(*y - NEXT / 2.0 - top);
                        *y += NEXT / 2.0;
                    }
                    place(tx, diagram, operand, ends, lanes, fragments, y)?;
                }
                let id = add_fragment(tx, diagram, child, &p.covered, top)?;
                let height = (*y - NEXT / 2.0 - top).max(FRAGMENT_HEIGHT / 2.0);
                tx.update_presentation(diagram, id, |pr| {
                    if let Shape::Fragment { bounds, separators: s, .. } = &mut pr.shape {
                        bounds.height = height;
                        *s = separators;
                    }
                })?;
                *y = top + height + NEXT / 2.0;
            }
            ElementKind::CombinedFragment(_) => {
                // Not shown: its content still is.
                for operand in tx.model().children(child).to_vec() {
                    place(tx, diagram, operand, ends, lanes, fragments, y)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// What should hold on a sequence diagram after every edit ([`crate::invariants`]): bars and
/// messages on their lifelines, bars nested or apart, operand lines inside their fragment in
/// order, and nothing left for [`tidy`] to do.
pub(crate) fn violations(model: &Model, diagram: Id) -> Vec<String> {
    let (Ok(g), Some(view)) = (Geometry::read(model, diagram), model.view(diagram)) else {
        return Vec::new();
    };
    let what = |id: Id| crate::invariants::describe(model, view, id);
    let mut found = Vec::new();
    for bar in &g.bars {
        let Some(lane) = g.lane(bar.lane) else {
            found.push(format!("{} has no lifeline", what(bar.id)));
            continue;
        };
        if bar.top < lane.line_top() - 0.5 || bar.bottom > lane.bottom() + 0.5 {
            found.push(format!("{} runs off {}", what(bar.id), what(lane.id)));
        }
        for other in g.bars.iter().filter(|o| o.lane == bar.lane && o.id != bar.id) {
            if bar.top + 0.5 < other.top
                && other.top < bar.bottom - 0.5
                && other.bottom > bar.bottom + 0.5
            {
                found.push(format!(
                    "{} and {} overlap without nesting",
                    what(bar.id),
                    what(other.id)
                ));
            }
        }
    }
    for m in &g.messages {
        // A create message ends at the head it makes.
        let ends = [(m.source, m.y, "starts"), (m.target, m.end(), "ends")];
        for (lane, y, end) in
            ends.into_iter().take(if m.sort == MessageSort::Create { 1 } else { 2 })
        {
            match g.lane(lane) {
                Some(l) if y < l.line_top() - 0.5 || y > l.bottom() + 0.5 => {
                    found.push(format!("{} {end} off {}", what(m.id), what(l.id)));
                }
                Some(_) => {}
                None => found.push(format!("{} {end} on no lifeline", what(m.id))),
            }
        }
    }
    for f in &g.frames {
        let mut above = f.rect.y;
        for &s in &f.separators {
            if s < above + 0.5 || s > f.rect.bottom() - 0.5 {
                found.push(format!("{} has an operand line out of place", what(f.id)));
                break;
            }
            above = s;
        }
    }
    // Every edit ends tidied, so tidying again changes nothing.
    let mut tidied = Document::new(model.clone());
    match tidied.edit("check", |tx| tidy(tx, diagram)) {
        Ok(()) => {
            let after = tidied.model().view(diagram);
            for p in view.presentations() {
                match after.and_then(|v| v.get(p.id)) {
                    Some(q) if q == p => {}
                    Some(_) => found.push(format!("tidying would change {}", what(p.id))),
                    None => found.push(format!("tidying would remove {}", what(p.id))),
                }
            }
            if after.is_some_and(|v| v.presentations().len() > view.presentations().len()) {
                found.push("tidying would add presentations".into());
            }
            let owner = model.get(diagram).and_then(|d| d.owner());
            let order = |m: &Model| owner.map(|o| order_of(m, o)).unwrap_or_default();
            if order(model) != order(tidied.model()) {
                found.push("tidying would reorder the interaction".into());
            }
        }
        Err(err) => found.push(format!("tidying fails: {err}")),
    }
    found
}

/// The elements under `owner`, depth first, with their owners: what [`sync_order`] arranges.
fn order_of(model: &Model, owner: Id) -> Vec<(Id, Id)> {
    let mut order = Vec::new();
    let mut stack = vec![owner];
    while let Some(o) = stack.pop() {
        for &child in model.children(o) {
            order.push((o, child));
            stack.push(child);
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{
        DiagramKind, Document, ElementData, ElementKind, Id, Model, Point, Rect, Shape,
    };

    use crate::palette::{self, EdgeTool, NodeTool};
    use crate::scene::{self, ApproxMeasure, Figure, Preview, Scene};
    use crate::sequence::Slot;

    fn scene_of(doc: &Document, diagram: Id) -> Scene {
        scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure)
    }

    /// Message labels from top to bottom.
    fn labels(scene: &Scene) -> Vec<String> {
        let mut edges: Vec<_> = scene.edges.iter().filter(|e| e.fixed).collect();
        edges.sort_by(|a, b| a.points[0].y.total_cmp(&b.points[0].y));
        edges.iter().map(|e| e.labels.first().map(|l| l.text.clone()).unwrap_or_default()).collect()
    }

    fn shapes(doc: &Document, diagram: Id) -> Vec<Shape> {
        doc.model().view(diagram).unwrap().presentations().iter().map(|p| p.shape.clone()).collect()
    }

    fn bars(doc: &Document, diagram: Id) -> Vec<(f32, f32)> {
        let mut bars: Vec<(f32, f32)> = shapes(doc, diagram)
            .into_iter()
            .filter_map(|s| match s {
                Shape::Activation { top, height, .. } => Some((top, top + height)),
                _ => None,
            })
            .collect();
        bars.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        bars
    }

    fn message(doc: &Document, diagram: Id, message: Id) -> Shape {
        let view = doc.model().view(diagram).unwrap();
        view.presentations_of(message).next().unwrap().shape.clone()
    }

    /// An empty sequence diagram with two lifelines, 200 apart. Returns their presentations.
    fn two_lifelines() -> (Document, Id, Id, Id) {
        let (mut doc, ids) = order_system();
        let interaction = doc.model().get(ids.sequence_diagram).unwrap().owner().unwrap();
        let d = doc
            .edit("diagram", |tx| {
                let kind = ElementKind::diagram(DiagramKind::Sequence);
                tx.create(interaction, ElementData::new("d", kind))
            })
            .unwrap();
        let lifeline = |doc: &mut Document, x: f32, y: f32| {
            let at = Point::new(x, y);
            doc.edit("lifeline", |tx| palette::create_node(tx, d, NodeTool::Lifeline, at, "l"))
                .unwrap()
                .1
        };
        let a = lifeline(&mut doc, 40.0, 40.0);
        let b = lifeline(&mut doc, 240.0, 60.0);
        (doc, d, a, b)
    }

    fn draw(doc: &mut Document, d: Id, tool: EdgeTool, from: Id, to: Id, y: f32) -> Id {
        let slot = Slot { y, owner: doc.model().get(d).unwrap().owner().unwrap(), index: 0 };
        let p = doc
            .edit("message", |tx| palette::connect_message(tx, d, tool, from, to, Some(slot)))
            .unwrap();
        doc.model().view(d).unwrap().get(p).unwrap().element.unwrap()
    }

    #[test]
    fn tidying_leaves_the_sample_as_it_is() {
        let (mut doc, ids) = order_system();
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, ids.sequence_diagram)).unwrap();
        assert_eq!(*doc.model(), before);
    }

    #[test]
    fn the_sample_is_drawn_from_its_geometry() {
        let (doc, ids) = order_system();
        let scene = scene_of(&doc, ids.sequence_diagram);
        // As in Astah, the reply's return value follows the call it answers.
        assert_eq!(labels(&scene), ["1: addItem(item)", "2: submit() : confirmation", ""]);
        let count = |f: fn(&Figure) -> bool| scene.nodes.iter().filter(|n| f(&n.figure)).count();
        assert_eq!(count(|f| matches!(f, Figure::Lifeline { .. })), 2);
        assert_eq!(count(|f| matches!(f, Figure::Activation)), 4);
        assert_eq!(count(|f| matches!(f, Figure::Frame { .. })), 1);
        let rows = |figure: fn(&Figure) -> bool| -> Vec<String> {
            let node = scene.nodes.iter().find(|n| figure(&n.figure)).unwrap();
            node.rows.iter().map(|r| r.text.clone()).collect()
        };
        assert_eq!(rows(|f| matches!(f, Figure::Frame { .. })), ["sd", "Place order"]);
        // A loop's condition goes into its tab.
        assert_eq!(rows(|f| matches!(f, Figure::Fragment { .. })), ["loop [more items]"]);
        // Calls run between the bars' sides, 5 beside the lifelines' centers.
        let add_item = scene.edges.iter().find(|e| e.points[0].y == 150.0).unwrap();
        assert_eq!((add_item.points[0].x, add_item.points[1].x), (120.0, 310.0));
    }

    #[test]
    fn a_fragment_names_itself_in_its_tab_and_lifelines_hide_their_name() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let scene = scene_of(&doc, d);
        let fragment = scene.nodes.iter().find(|n| matches!(n.figure, Figure::Fragment { .. }));
        let fragment = fragment.unwrap().element.unwrap();
        let lifeline = scene.nodes.iter().find(|n| matches!(n.figure, Figure::Lifeline { .. }));
        let lifeline = lifeline.unwrap().id;
        doc.edit("name", |tx| {
            tx.rename(fragment, "each item")?;
            tx.update_presentation(d, lifeline, |p| p.style.hide_instance_name = true)
        })
        .unwrap();
        let scene = scene_of(&doc, d);
        let rows = |figure: fn(&Figure) -> bool| -> Vec<String> {
            let node = scene.nodes.iter().find(|n| figure(&n.figure)).unwrap();
            node.rows.iter().map(|r| r.text.clone()).collect()
        };
        assert_eq!(
            rows(|f| matches!(f, Figure::Fragment { .. })),
            ["loop : each item [more items]"]
        );
        let head = &scene.node(lifeline).unwrap().rows[0].text;
        assert!(head.is_empty() || head.starts_with(" : "), "{head:?}");
    }

    #[test]
    fn a_call_without_arguments_shows_its_operations_parameters() {
        use umlcraft_model::{ParameterProps, TypeRef};
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let model = doc.model();
        let submit = model
            .elements()
            .find(|e| e.name() == "submit" && matches!(e.kind(), ElementKind::Message(_)))
            .unwrap()
            .id();
        let order = ids.order;
        let presentation = model.shown_on(submit)[0].1;
        doc.edit("operation", |tx| {
            let check = TypeRef::Text("Bool".into());
            let op =
                tx.create(order, ElementData::new("check", ElementKind::operation(Some(check))))?;
            let parameter = ParameterProps {
                type_ref: Some(TypeRef::Text("Item".into())),
                ..ParameterProps::default()
            };
            tx.create(op, ElementData::new("item", ElementKind::Parameter(parameter)))?;
            tx.update(submit, |data| {
                data.name = String::new();
                if let ElementKind::Message(p) = &mut data.kind {
                    p.operation = Some(op);
                }
            })
        })
        .unwrap();
        let label = |doc: &Document| {
            let scene = scene_of(doc, d);
            scene
                .edge(presentation)
                .unwrap()
                .labels
                .iter()
                .map(|l| l.text.clone())
                .collect::<String>()
        };
        // As Astah: the parameters with their types, no space after the colon.
        assert!(label(&doc).contains("check(item:Item)"), "{}", label(&doc));
        doc.edit("style", |tx| {
            tx.update_presentation(d, presentation, |p| {
                p.style.hide_parameter_types = true;
                p.style.show_return_type = true;
            })
        })
        .unwrap();
        assert!(label(&doc).contains("check(item) : Bool"), "{}", label(&doc));
    }

    #[test]
    fn calls_get_bars_and_replies_end_them() {
        let (mut doc, d, a, b) = two_lifelines();
        // The second lifeline lines up with the first; a frame surrounds both.
        let heads: Vec<Rect> = shapes(&doc, d)
            .into_iter()
            .filter_map(|s| match s {
                Shape::Lifeline { head, .. } => Some(head),
                _ => None,
            })
            .collect();
        assert_eq!(heads.len(), 2);
        assert_eq!(heads[0].y, heads[1].y);
        assert_eq!(doc.model().view(d).unwrap().presentations()[0].element, Some(d));

        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        assert_eq!(bars(&doc, d), [(120.0, 145.0), (120.0, 165.0)]);
        // Drawn anywhere on the called bar, the reply leaves at its end.
        let reply = draw(&mut doc, d, EdgeTool::Reply, b, a, 128.0);
        assert!(matches!(message(&doc, d, reply), Shape::Message { y: 145.0, .. }));
        // A message to self drops 20 into a bar of its own; the bar around it grows, and so
        // does the caller's.
        let own = draw(&mut doc, d, EdgeTool::Message, b, b, 130.0);
        assert!(matches!(message(&doc, d, own), Shape::Message { end_y: Some(150.0), .. }));
        let all = bars(&doc, d);
        assert!(all.contains(&(150.0, 175.0)), "{all:?}");
        assert!(all.contains(&(120.0, 195.0)), "{all:?}");
        // The interaction's order follows the heights.
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let order: Vec<Id> = doc
            .model()
            .children(interaction)
            .iter()
            .copied()
            .filter(|c| [call, reply, own].contains(c))
            .collect();
        assert_eq!(order, [call, own, reply]);
        // Numbered by the bars they are sent from.
        let numbers: Vec<String> = labels(&scene_of(&doc, d));
        assert_eq!(numbers[..2], ["1: ".to_owned(), "1.1: ".to_owned()]);
        doc.model().check().unwrap();
    }

    #[test]
    fn a_reply_returns_into_the_bar_its_call_left() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        // The called side works longer than the caller's bar reaches.
        draw(&mut doc, d, EdgeTool::Message, b, b, 130.0);
        draw(&mut doc, d, EdgeTool::Message, b, b, 160.0);
        let reply = draw(&mut doc, d, EdgeTool::Reply, b, a, 200.0);
        let Shape::Message { y, .. } = message(&doc, d, reply) else { unreachable!() };
        let view = doc.model().view(d).unwrap();
        let caller = view
            .presentations()
            .iter()
            .find_map(|p| match p.shape {
                Shape::Activation { lifeline, top, height, .. } if lifeline == a => {
                    Some((top, top + height))
                }
                _ => None,
            })
            .unwrap();
        assert!(caller.0 <= 120.0 && y < caller.1, "reply at {y}, caller's bar {caller:?}");
        doc.model().check().unwrap();
    }

    #[test]
    fn a_reply_moved_down_takes_the_end_of_its_bar_along() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let reply = draw(&mut doc, d, EdgeTool::Reply, b, a, 130.0);
        let p = doc.model().view(d).unwrap().presentations_of(reply).next().unwrap().id;
        doc.edit("move", |tx| {
            tx.update_presentation(d, p, |p| {
                if let Shape::Message { y, .. } = &mut p.shape {
                    *y = 230.0;
                }
            })?;
            super::tidy(tx, d)
        })
        .unwrap();
        let called =
            doc.model().view(d).unwrap().presentations().iter().find_map(|p| match p.shape {
                Shape::Activation { lifeline, top, height, .. } if lifeline == b => {
                    Some(top + height)
                }
                _ => None,
            });
        assert_eq!(called, Some(230.0));
    }

    #[test]
    fn a_caller_waits_until_its_call_has_ended() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let bottom = |doc: &Document, lane: Id| {
            doc.model().view(d).unwrap().presentations().iter().find_map(|p| match p.shape {
                Shape::Activation { lifeline, top, height, .. } if lifeline == lane => {
                    Some(top + height)
                }
                _ => None,
            })
        };
        doc.edit("longer", |tx| {
            let view = tx.model().view(d).unwrap();
            let called = view
                .presentations()
                .iter()
                .find(|p| matches!(p.shape, Shape::Activation { lifeline, .. } if lifeline == b))
                .unwrap()
                .id;
            tx.update_presentation(d, called, |p| {
                if let Shape::Activation { height, .. } = &mut p.shape {
                    *height = 180.0;
                }
            })?;
            super::tidy(tx, d)
        })
        .unwrap();
        assert_eq!(bottom(&doc, b), Some(300.0));
        assert_eq!(bottom(&doc, a), Some(300.0));
    }

    #[test]
    fn a_reply_moved_up_shortens_its_bar() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let reply = draw(&mut doc, d, EdgeTool::Reply, b, a, 130.0);
        let called = |doc: &Document| {
            doc.model().view(d).unwrap().presentations().iter().find_map(|p| match p.shape {
                Shape::Activation { lifeline, top, height, .. } if lifeline == b => {
                    Some(top + height)
                }
                _ => None,
            })
        };
        doc.edit("move", |tx| super::move_item(tx, d, reply, 230.0)).unwrap();
        assert_eq!(called(&doc), Some(230.0));
        doc.edit("move", |tx| super::move_item(tx, d, reply, 160.0)).unwrap();
        assert_eq!(called(&doc), Some(160.0));
        doc.model().check().unwrap();
    }

    #[test]
    fn a_message_drawn_between_bars_goes_between_their_lifelines() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let view = doc.model().view(d).unwrap();
        let bar_on = |lifeline: Id| {
            view.presentations()
                .iter()
                .find(|p| matches!(p.shape, Shape::Activation { lifeline: l, .. } if l == lifeline))
                .unwrap()
                .id
        };
        let (caller, called) = (bar_on(a), bar_on(b));
        let reply = draw(&mut doc, d, EdgeTool::Reply, called, caller, 130.0);
        assert!(matches!(
            message(&doc, d, reply),
            Shape::Message { source, target, .. } if source == b && target == a
        ));
        doc.model().check().unwrap();
    }

    #[test]
    fn moving_a_call_takes_what_starts_inside_it_and_finds_a_sender_bar() {
        let (mut doc, d, a, b) = two_lifelines();
        let at = Point::new(440.0, 40.0);
        let c = doc
            .edit("lifeline", |tx| palette::create_node(tx, d, NodeTool::Lifeline, at, "c"))
            .unwrap()
            .1;
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        // A call into the busy lifeline: its bar sits inside the first call's bar.
        let busy = draw(&mut doc, d, EdgeTool::Message, c, b, 135.0);
        doc.edit("move", |tx| super::move_item(tx, d, call, 300.0)).unwrap();
        assert!(matches!(message(&doc, d, busy), Shape::Message { y: 315.0, .. }));
        // A message sent from the moved bar, moved up out of it, starts a bar of its own.
        let onward = draw(&mut doc, d, EdgeTool::Message, b, c, 340.0);
        doc.edit("move", |tx| super::move_item(tx, d, onward, 200.0)).unwrap();
        let tops = bars(&doc, d).iter().filter(|(top, _)| *top == 200.0).count();
        assert_eq!(tops, 2, "{:?}", bars(&doc, d));
        doc.model().check().unwrap();
    }

    #[test]
    fn a_lifeline_is_dragged_longer_but_not_shorter_than_its_messages() {
        let (mut doc, d, a, b) = two_lifelines();
        let bottom = |doc: &Document| match doc.model().view(d).unwrap().get(a).unwrap().shape {
            Shape::Lifeline { head, length, .. } => head.bottom() + length,
            _ => unreachable!(),
        };
        doc.edit("longer", |tx| super::set_lifeline_end(tx, d, a, 700.0)).unwrap();
        assert_eq!(bottom(&doc), 700.0);
        draw(&mut doc, d, EdgeTool::Message, a, b, 300.0);
        // Above its message, it stops just below what it reaches.
        doc.edit("shorter", |tx| super::set_lifeline_end(tx, d, a, 100.0)).unwrap();
        let reach = bottom(&doc);
        assert!((300.0..400.0).contains(&reach), "{reach}");
    }

    #[test]
    fn calls_one_after_the_other_share_the_senders_bar() {
        let (mut doc, d, a, b) = two_lifelines();
        // The bars on one lifeline, from the top.
        let on = |doc: &Document, lane: Id| -> Vec<(f32, f32)> {
            let mut bars: Vec<(f32, f32)> = shapes(doc, d)
                .into_iter()
                .filter_map(|s| match s {
                    Shape::Activation { lifeline, top, height, .. } if lifeline == lane => {
                        Some((top, top + height))
                    }
                    _ => None,
                })
                .collect();
            bars.sort_by(|x, y| x.0.total_cmp(&y.0));
            bars
        };
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        // Drawn into the receiver's bar, the second call starts 20 below it.
        let second = draw(&mut doc, d, EdgeTool::Message, a, b, 130.0);
        assert!(matches!(message(&doc, d, second), Shape::Message { y: 165.0, .. }));
        assert_eq!(on(&doc, b), [(120.0, 145.0), (165.0, 190.0)]);
        assert_eq!(on(&doc, a), [(120.0, 190.0)]);
        // Dragged far down, it leaves the first bar as short as before and starts its own.
        doc.edit("move", |tx| super::move_item(tx, d, second, 300.0)).unwrap();
        assert_eq!(on(&doc, a), [(120.0, 165.0), (300.0, 345.0)]);
        // Back just below the first bar, it continues that one; no second bar is left over.
        doc.edit("move", |tx| super::move_item(tx, d, second, 190.0)).unwrap();
        assert_eq!(on(&doc, a), [(120.0, 215.0)]);
        // A call drawn just below the sender's bar continues it as well.
        draw(&mut doc, d, EdgeTool::Message, a, b, 240.0);
        assert_eq!(on(&doc, a), [(120.0, 265.0)]);
        doc.model().check().unwrap();
    }

    #[test]
    fn deleting_a_call_takes_its_bars_along() {
        let (mut doc, d, a, b) = two_lifelines();
        let first = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        draw(&mut doc, d, EdgeTool::Message, a, b, 220.0);
        assert_eq!(bars(&doc, d).len(), 4);
        doc.edit("delete", |tx| {
            tx.delete(first)?;
            super::tidy_all(tx)
        })
        .unwrap();
        assert_eq!(bars(&doc, d), [(220.0, 245.0), (220.0, 265.0)]);
    }

    #[test]
    fn a_label_moved_by_hand_stays_with_its_message() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let p = doc.model().view(d).unwrap().presentations_of(call).next().unwrap().id;
        let at = Point::new(150.0, 60.0);
        doc.edit("label", |tx| super::place_label(tx, d, p, at)).unwrap();
        let label = |doc: &Document| scene_of(doc, d).edge(p).unwrap().labels[0].anchor;
        assert_eq!(label(&doc), at);
        // Moving the message takes the label along.
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let slot = Slot { y: 150.0, owner: interaction, index: 0 };
        doc.edit("move", |tx| palette::move_to_slot(tx, call, slot)).unwrap();
        assert_eq!(label(&doc), Point::new(150.0, 90.0));
    }

    #[test]
    fn messages_stay_below_the_heads_and_a_create_above_what_follows() {
        let line_top = |doc: &Document, d: Id, p: Id| match doc.model().view(d).unwrap().get(p) {
            Some(umlcraft_model::Presentation { shape: Shape::Lifeline { head, .. }, .. }) => {
                head.bottom()
            }
            other => panic!("{other:?}"),
        };
        let y_of = |doc: &Document, d: Id, m: Id| match message(doc, d, m) {
            Shape::Message { y, .. } => y,
            other => panic!("{other:?}"),
        };
        // Dragged above the heads, a message stops below them.
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 150.0);
        doc.edit("up", |tx| super::move_item(tx, d, call, 0.0)).unwrap();
        let top = line_top(&doc, d, a).max(line_top(&doc, d, b));
        assert_eq!(y_of(&doc, d, call), top + super::MESSAGE_GAP);
        // The sender's bar starts there too, not where the message was dragged to.
        assert!(
            bars(&doc, d).iter().all(|&(t, _)| t == top + super::MESSAGE_GAP),
            "{:?}",
            bars(&doc, d)
        );
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
        // Dragged below what the lifeline it makes does, a create message stops above that.
        let (mut doc, d, a, b) = two_lifelines();
        let create = draw(&mut doc, d, EdgeTool::CreateMessage, a, b, 250.0);
        // The constructor's bar goes along with the head.
        doc.edit("down", |tx| super::move_item(tx, d, create, 280.0)).unwrap();
        let top = line_top(&doc, d, b);
        assert!(bars(&doc, d).contains(&(top, top + super::RECEIVER_BAR)), "{:?}", bars(&doc, d));
        let back = draw(&mut doc, d, EdgeTool::Message, b, a, 400.0);
        let first = y_of(&doc, d, back);
        doc.edit("down", |tx| super::move_item(tx, d, create, 600.0)).unwrap();
        assert_eq!(line_top(&doc, d, b), first - super::MESSAGE_GAP);
        assert_eq!(y_of(&doc, d, back), first);
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn create_and_destroy_move_the_lifeline() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::CreateMessage, a, b, 150.0);
        let lane = |doc: &Document| {
            let view = doc.model().view(d).unwrap();
            match view.get(b).unwrap().shape {
                Shape::Lifeline { head, length, destroyed } => (head, length, destroyed),
                ref other => panic!("{other:?}"),
            }
        };
        let (head, _, _) = lane(&doc);
        assert_eq!(head.center().y, 150.0);
        // The sender gets a bar, and the new lifeline one below its head for the constructor.
        assert_eq!(bars(&doc, d), vec![(150.0, 210.0), (head.bottom(), head.bottom() + 25.0)]);
        draw(&mut doc, d, EdgeTool::DestroyMessage, a, b, 260.0);
        let (head, length, destroyed) = lane(&doc);
        assert!(destroyed);
        assert_eq!(head.bottom() + length, 285.0);
        assert_eq!(scene_of(&doc, d).sequence.unwrap().destructions.len(), 1);
        let destroy =
            doc.model().view(d).unwrap().presentations().iter().find_map(|p| match p.shape {
                Shape::Message { y: 260.0, .. } => p.element,
                _ => None,
            });
        // Moved, the destroy message takes the cross along, down and up.
        for y in [340.0, 230.0] {
            doc.edit("move", |tx| super::move_item(tx, d, destroy.unwrap(), y)).unwrap();
            let (head, length, _) = lane(&doc);
            assert_eq!(head.bottom() + length, y + 25.0);
        }
        // Without its destroy message the lifeline runs on.
        doc.edit("undestroy", |tx| {
            tx.delete(destroy.unwrap())?;
            super::tidy_all(tx)
        })
        .unwrap();
        assert!(!lane(&doc).2);
        assert!(scene_of(&doc, d).sequence.unwrap().destructions.is_empty());
    }

    /// The center of the lifeline drawn as `lane`, and where its line ends.
    fn lane_of(doc: &Document, d: Id, lane: Id) -> (f32, f32) {
        match doc.model().view(d).unwrap().get(lane).unwrap().shape {
            Shape::Lifeline { head, length, .. } => (head.center().x, head.bottom() + length),
            ref other => panic!("{other:?}"),
        }
    }

    fn bounds_of(doc: &Document, d: Id, presentation: Id) -> Rect {
        doc.model().view(d).unwrap().get(presentation).unwrap().bounds().unwrap()
    }

    fn presentation_of(doc: &Document, d: Id, element: Id) -> Id {
        doc.model().view(d).unwrap().presentations_of(element).next().unwrap().id
    }

    fn on_lifeline(doc: &mut Document, d: Id, termination: bool, at: Point) -> (Id, Id) {
        doc.edit("mark", |tx| super::create_on_lifeline(tx, d, termination, at, "x > 0")).unwrap()
    }

    #[test]
    fn state_invariants_sit_on_their_lifeline_in_the_order_of_the_drawing() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 150.0);
        let (center, _) = lane_of(&doc, d, b);
        // Clicked off center, it goes on the middle of the lifeline.
        let (invariant, node) = on_lifeline(&mut doc, d, false, Point::new(center + 15.0, 200.0));
        let r = bounds_of(&doc, d, node);
        assert!((r.center().x - center).abs() < 0.01, "{r:?}");
        assert_eq!((r.y, r.height), (200.0, super::INVARIANT_HEIGHT));
        assert_eq!(r.width, super::INVARIANT_WIDTH);
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let order = |doc: &Document| doc.model().children(interaction).to_vec();
        let items = |doc: &Document| -> Vec<Id> {
            order(doc).into_iter().filter(|&c| c == call || c == invariant).collect()
        };
        assert_eq!(items(&doc), [call, invariant]);
        // Moved below it, the message comes after it.
        doc.edit("move", |tx| super::move_item(tx, d, call, 260.0)).unwrap();
        assert_eq!(items(&doc), [invariant, call]);
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, d)).unwrap();
        assert_eq!(*doc.model(), before);
        // Off every lifeline it is refused.
        let off = doc.edit("mark", |tx| {
            super::create_on_lifeline(tx, d, false, Point::new(center + 400.0, 200.0), "y")
        });
        assert!(off.is_err());
        let scene = scene_of(&doc, d);
        let drawn = scene.node(node).unwrap();
        assert!(matches!(drawn.figure, Figure::StateInvariant));
        assert_eq!(drawn.rows.first().map(|r| r.text.as_str()), Some("x > 0"));
    }

    #[test]
    fn a_termination_ends_its_lifeline_below_what_happens_on_it() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 150.0);
        let (center, _) = lane_of(&doc, d, b);
        let (_, cross) = on_lifeline(&mut doc, d, true, Point::new(center, 100.0));
        let ends = |doc: &Document| {
            let (_, end) = lane_of(doc, d, b);
            let r = bounds_of(doc, d, cross);
            assert_eq!(r.center().y, end, "the cross sits where the line ends");
            assert_eq!(r.width, super::CROSS);
            end
        };
        // 30 below the bar the message starts on the lifeline.
        assert_eq!(ends(&doc), 205.0);
        let y = |doc: &Document| match message(doc, d, call) {
            Shape::Message { y, .. } => y,
            other => panic!("{other:?}"),
        };
        // Moved down, the message stops above the cross, which keeps below its bar.
        doc.edit("move", |tx| super::move_item(tx, d, call, 300.0)).unwrap();
        assert_eq!(y(&doc), 185.0);
        assert_eq!(ends(&doc), 240.0);
        // Dragged longer, the lifeline keeps its length, and there is room above the cross.
        doc.edit("longer", |tx| super::set_lifeline_end(tx, d, b, 400.0)).unwrap();
        assert_eq!(ends(&doc), 400.0);
        doc.edit("move", |tx| super::move_item(tx, d, call, 300.0)).unwrap();
        assert_eq!((y(&doc), ends(&doc)), (300.0, 400.0));
        // A lifeline ends once.
        let again = doc.edit("again", |tx| {
            super::create_on_lifeline(tx, d, true, Point::new(center, 500.0), "")
        });
        assert!(again.is_err());
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, d)).unwrap();
        assert_eq!(*doc.model(), before);
        // A termination is no destroy message: the lifeline itself is not destroyed.
        assert!(matches!(
            doc.model().view(d).unwrap().get(b).unwrap().shape,
            Shape::Lifeline { destroyed: false, .. }
        ));
        let scene = scene_of(&doc, d);
        assert!(matches!(scene.node(cross).unwrap().figure, Figure::Termination));
    }

    #[test]
    fn duration_and_time_constraints_follow_their_messages() {
        let (mut doc, d, a, b) = two_lifelines();
        let first = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 150.0);
        let second = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 250.0);
        let (p1, p2) = (presentation_of(&doc, d, first), presentation_of(&doc, d, second));
        let duration = doc.edit("duration", |tx| super::constrain(tx, d, p1, p2)).unwrap();
        let time = doc.edit("time", |tx| super::constrain(tx, d, p2, p2)).unwrap();
        let element = |doc: &Document, p: Id| doc.model().view(d).unwrap().get(p).unwrap().element;
        let (de, te) = (element(&doc, duration).unwrap(), element(&doc, time).unwrap());
        let kinds: Vec<ElementKind> =
            [de, te].map(|e| doc.model().get(e).unwrap().kind().clone()).into();
        assert_eq!(
            kinds,
            [
                ElementKind::DurationConstraint(umlcraft_model::DurationConstraintProps {
                    from: first,
                    to: second
                }),
                ElementKind::TimeConstraint(umlcraft_model::TimeConstraintProps {
                    message: second
                }),
            ]
        );
        let (ca, cb) = (lane_of(&doc, d, a).0, lane_of(&doc, d, b).0);
        let r = bounds_of(&doc, d, duration);
        assert_eq!((r.center().x, r.y, r.bottom()), ((ca + cb) / 2.0, 150.0, 250.0));
        // Moved, the messages take the line's ends along; it keeps its x.
        doc.edit("move", |tx| super::move_item(tx, d, second, 320.0)).unwrap();
        let moved = bounds_of(&doc, d, duration);
        assert_eq!((moved.center().x, moved.y, moved.bottom()), (r.center().x, 150.0, 320.0));
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, d)).unwrap();
        assert_eq!(*doc.model(), before);
        // Named, they read in braces; the time constraint's line goes on from the arrowhead.
        doc.edit("name", |tx| {
            tx.rename(de, "d..3*d".to_owned())?;
            tx.rename(te, "t..t+3".to_owned())
        })
        .unwrap();
        let scene = scene_of(&doc, d);
        let node = scene.node(duration).unwrap();
        assert!(matches!(node.figure, Figure::Duration { top: 150.0, bottom: 320.0, .. }));
        assert_eq!(node.rows.first().map(|r| r.text.as_str()), Some("{d..3*d}"));
        let node = scene.node(time).unwrap();
        let Figure::TimeTick { from, to } = node.figure else { panic!("no time tick") };
        assert_eq!((from.y, to.y), (320.0, 320.0));
        assert!(from.x < cb && to.x - from.x == super::TIME_TICK, "{from:?} {to:?}");
        assert_eq!(node.rows.first().map(|r| r.text.as_str()), Some("{t..t+3}"));
        // Not on a message, refused; without its message, gone.
        assert!(doc.edit("off", |tx| super::constrain(tx, d, a, p1)).is_err());
        doc.edit("delete", |tx| {
            tx.delete(second)?;
            super::tidy_all(tx)
        })
        .unwrap();
        let view = doc.model().view(d).unwrap();
        assert!(view.get(duration).is_none() && view.get(time).is_none());
    }

    #[test]
    fn state_invariants_wrap_and_grow_as_astah_draws_them() {
        let (mut doc, d, _, b) = two_lifelines();
        let (center, _) = lane_of(&doc, d, b);
        let (invariant, node) = on_lifeline(&mut doc, d, false, Point::new(center, 200.0));
        let rename = |doc: &mut Document, text: &str| {
            crate::edit::apply(doc, "name", &ApproxMeasure, |tx| {
                tx.rename(invariant, text.to_owned())
            })
            .unwrap();
            let r = bounds_of(doc, d, node);
            let scene = scene_of(doc, d);
            let rows: Vec<(String, Rect)> = scene
                .node(node)
                .unwrap()
                .rows
                .iter()
                .map(|row| (row.text.clone(), row.rect))
                .collect();
            (r, rows)
        };
        let texts = |rows: &[(String, Rect)]| -> Vec<String> {
            rows.iter().map(|(t, _)| t.clone()).collect()
        };
        // Wider than the box: broken before the word that does not fit, left-aligned, the box
        // 15.09 higher for each line beyond the first and 5 below the text.
        let (r, rows) = rename(&mut doc, "Waiting for a reply from the server");
        assert_eq!(texts(&rows), ["Waiting", "for a", "reply", "from the", "server"]);
        assert!((r.height - (5.0 * 15.093_75 + 5.0)).abs() < 0.01, "{r:?}");
        assert!(rows.iter().all(|(_, row)| (row.x - (r.x + 10.0)).abs() < 0.01));
        assert!((rows[1].1.y - rows[0].1.y - super::INVARIANT_LINE).abs() < 0.01);
        assert_eq!(r.width, super::INVARIANT_WIDTH, "the width stays");
        // Drawn with Astah's corners, 10 wide and 20 high, and a border of 2.
        let drawn = crate::draw::figure(scene_of(&doc, d).node(node).unwrap());
        let crate::draw::Primitive::Polygon { points, .. } = &drawn[0] else {
            panic!("no outline")
        };
        let at = |x: f32, y: f32| {
            points.iter().any(|p| (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01)
        };
        assert!(at(r.x + 10.0, r.y) && at(r.x, r.y + 20.0) && at(r.right(), r.bottom() - 20.0));
        assert!(matches!(drawn[1], crate::draw::Primitive::Polyline { width: 2.0, .. }));
        // The lifeline reaches below it, and tidying moves nothing.
        assert!(lane_of(&doc, d, b).1 >= r.bottom() + 15.0);
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, d)).unwrap();
        assert_eq!(*doc.model(), before);
        // A word wider than the box breaks between characters.
        let (r, rows) = rename(&mut doc, "abcdefghijklmnopqrstuvwxyz0123");
        assert_eq!(texts(&rows), ["abcdefgh", "ijklmnop", "qrstuvwx", "yz0123"]);
        assert!((r.height - 65.375).abs() < 0.01, "{r:?}");
        // Line breaks count, empty lines too; a text as narrow as the box keeps each line
        // centered, as one line is.
        let (r, rows) = rename(&mut doc, "a\n\nb");
        assert_eq!(texts(&rows), ["a", "", "b"]);
        assert!((r.height - 50.281_25).abs() < 0.01, "{r:?}");
        let middle = |row: &Rect| row.center().x;
        assert!((middle(&rows[0].1) - r.center().x).abs() < 0.01);
        assert!((middle(&rows[2].1) - r.center().x).abs() < 0.01);
        let (r, rows) = rename(&mut doc, "first\nsecond");
        assert_eq!(texts(&rows), ["first", "second"]);
        assert!((r.height - 35.1875).abs() < 0.01, "{r:?}");
        assert!((rows[0].1.x - (r.x + 10.0)).abs() < 0.01);
        // Shortened, it shrinks back to one line.
        let (r, _) = rename(&mut doc, "x");
        assert_eq!(r.height, super::INVARIANT_HEIGHT);
    }

    #[test]
    fn constraint_labels_stand_beside_their_line_and_can_be_moved() {
        let (mut doc, d, a, b) = two_lifelines();
        let first = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 150.0);
        let second = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 250.0);
        let (p1, p2) = (presentation_of(&doc, d, first), presentation_of(&doc, d, second));
        let duration = doc.edit("duration", |tx| super::constrain(tx, d, p1, p2)).unwrap();
        let time = doc.edit("time", |tx| super::constrain(tx, d, p2, p2)).unwrap();
        let element =
            |doc: &Document, p: Id| doc.model().view(d).unwrap().get(p).unwrap().element.unwrap();
        let (de, te) = (element(&doc, duration), element(&doc, time));
        let label = |doc: &Document, p: Id| scene_of(doc, d).node(p).unwrap().rows[0].rect;
        let line_x = |doc: &Document| match scene_of(doc, d).node(duration).unwrap().figure {
            Figure::Duration { x, .. } => x,
            _ => panic!("no duration"),
        };
        // Astah's `{d}` (16.7 wide in Arial) is centered 9 right of the line, beside it.
        use crate::sequence::{ConstraintAnchor, DURATION_GAP, constraint_label_box};
        let anchor = ConstraintAnchor::Duration { x: 100.0, middle: 200.0 };
        let short = constraint_label_box(anchor, 16.7, None);
        assert!((short.center().x - 109.0).abs() < 0.01 && short.x > 100.0 + DURATION_GAP);
        // A wider label does not cross the line: it starts right of it.
        doc.edit("name", |tx| tx.rename(de, "0..2 seconds".to_owned())).unwrap();
        let wide = label(&doc, duration);
        let x = line_x(&doc);
        assert!((wide.x - (x + DURATION_GAP)).abs() < 0.01, "{wide:?} {x}");
        assert!((wide.center().y - 200.0).abs() < 1.0, "{wide:?}");
        // Moved by hand, the label keeps its offset, which adds up over moves and follows the
        // line when its messages move.
        let by = |x, y| umlcraft_model::Point::new(x, y);
        doc.edit("label", |tx| super::move_constraint_label(tx, d, duration, by(30.0, -20.0)))
            .unwrap();
        doc.edit("label", |tx| super::move_constraint_label(tx, d, duration, by(5.0, 5.0)))
            .unwrap();
        let style =
            |doc: &Document, p: Id| doc.model().view(d).unwrap().get(p).unwrap().style.name_offset;
        assert_eq!(style(&doc, duration), Some(by(35.0, -15.0)));
        let moved = label(&doc, duration);
        assert!((moved.x - wide.x - 35.0).abs() < 0.01 && (moved.y - wide.y + 15.0).abs() < 0.01);
        doc.edit("move", |tx| super::move_item(tx, d, second, 350.0)).unwrap();
        let along = label(&doc, duration);
        assert!((along.center().y - moved.center().y - 50.0).abs() < 1.0, "{along:?}");
        // Moved back, it is in its place again.
        doc.edit("label", |tx| super::move_constraint_label(tx, d, duration, by(-35.0, 15.0)))
            .unwrap();
        assert_eq!(style(&doc, duration), None);
        // The frame holds a time constraint's label beyond the last lifeline.
        let long = "t..t+3 and a long while after";
        crate::edit::apply(&mut doc, "name", &ApproxMeasure, |tx| tx.rename(te, long.to_owned()))
            .unwrap();
        let text = label(&doc, time);
        let frame =
            scene_of(&doc, d).nodes.iter().find(|n| n.element == Some(d)).map(|n| n.rect).unwrap();
        // (The tidy measures by Java's widths, the app as it draws, close to them.)
        let java = crate::metrics::java_width(&format!("{{{long}}}"), 12.0, false).unwrap();
        assert!(frame.right() >= text.x + java + 10.0, "{frame:?} {text:?}");
        let before = doc.model().clone();
        doc.edit("tidy", |tx| super::tidy(tx, d)).unwrap();
        assert_eq!(*doc.model(), before);
        // Its label moves too.
        doc.edit("label", |tx| super::move_constraint_label(tx, d, time, by(0.0, 30.0))).unwrap();
        assert!((label(&doc, time).y - text.y - 30.0).abs() < 0.01);
    }

    #[test]
    fn deleting_operands_takes_their_lines_along() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let (fragment, node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[call])).unwrap();
        doc.edit("operands", |tx| {
            palette::add_operand(tx, fragment).and(palette::add_operand(tx, fragment))
        })
        .unwrap();
        let separators =
            |doc: &Document| match &doc.model().view(d).unwrap().get(node).unwrap().shape {
                Shape::Fragment { separators, .. } => separators.clone(),
                other => panic!("{other:?}"),
            };
        assert_eq!(separators(&doc).len(), 2);
        let operands = doc.model().children(fragment).to_vec();
        doc.edit("first", |tx| super::delete_operand(tx, operands[0]).and(super::tidy_all(tx)))
            .unwrap();
        assert_eq!(separators(&doc).len(), 1);
        doc.edit("second", |tx| super::delete_operand(tx, operands[1]).and(super::tidy_all(tx)))
            .unwrap();
        assert!(separators(&doc).is_empty());
        // The last one takes the fragment along.
        doc.edit("last", |tx| super::delete_operand(tx, operands[2]).and(super::tidy_all(tx)))
            .unwrap();
        assert!(!doc.model().contains(fragment));
        doc.model().check().unwrap();
    }

    #[test]
    fn new_messages_go_below_the_heads_and_a_destroy_below_what_happens() {
        let (mut doc, d, a, b) = two_lifelines();
        let line_top = |doc: &Document, p: Id| match doc.model().view(d).unwrap().get(p) {
            Some(umlcraft_model::Presentation { shape: Shape::Lifeline { head, .. }, .. }) => {
                head.bottom()
            }
            other => panic!("{other:?}"),
        };
        let y_of = |doc: &Document, m: Id| match message(doc, d, m) {
            Shape::Message { y, .. } => y,
            other => panic!("{other:?}"),
        };
        // Drawn on a head, a call goes right below the heads.
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 50.0);
        let top = line_top(&doc, a).max(line_top(&doc, b));
        assert_eq!(y_of(&doc, call), top + super::MESSAGE_GAP);
        // Drawn above that call, a destroy message goes below its bar.
        let destroy = draw(&mut doc, d, EdgeTool::DestroyMessage, a, b, 100.0);
        let call_bar_end = y_of(&doc, call) + super::RECEIVER_BAR;
        assert_eq!(y_of(&doc, destroy), call_bar_end + super::MESSAGE_GAP);
        // Tidied in one go: the destroy message drawn before the call in the model goes after it.
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn a_caller_destroyed_by_its_callee_waits_no_longer_than_its_cross() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        // Sent right after the call, the destroy message continues the callee's bar, which the
        // caller waits for; the caller's bar still ends at its cross.
        draw(&mut doc, d, EdgeTool::DestroyMessage, b, a, 100.0);
        let g = super::Geometry::read(doc.model(), d).unwrap();
        let (_, cross) = g.cross(a).unwrap();
        let waiting = g.bars.iter().filter(|bar| bar.lane == a).map(|bar| bar.bottom);
        assert_eq!(waiting.reduce(f32::max), Some(cross));
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn a_destroy_message_dragged_up_stays_below_what_happens() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let destroy = draw(&mut doc, d, EdgeTool::DestroyMessage, a, b, 300.0);
        doc.edit("move", |tx| super::move_item(tx, d, destroy, 60.0)).unwrap();
        let y_of = |m: Id| match message(&doc, d, m) {
            Shape::Message { y, .. } => y,
            other => panic!("{other:?}"),
        };
        assert_eq!(y_of(destroy), y_of(call) + super::MESSAGE_GAP);
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn a_lifeline_is_created_once() {
        let (mut doc, d, a, b) = two_lifelines();
        draw(&mut doc, d, EdgeTool::CreateMessage, a, b, 150.0);
        let slot = Slot { y: 300.0, owner: doc.model().get(d).unwrap().owner().unwrap(), index: 0 };
        let tool = EdgeTool::CreateMessage;
        let again = doc.edit("again", |tx| palette::connect_message(tx, d, tool, a, b, Some(slot)));
        assert!(again.is_err());
    }

    #[test]
    fn a_create_message_moved_along_takes_its_head_along() {
        let (mut doc, d, a, b) = two_lifelines();
        let own = draw(&mut doc, d, EdgeTool::Message, a, a, 150.0);
        let create = draw(&mut doc, d, EdgeTool::CreateMessage, a, b, 180.0);
        let at = |doc: &Document| {
            let view = doc.model().view(d).unwrap();
            let y = match view.presentations_of(create).next().unwrap().shape {
                Shape::Message { y, .. } => y,
                ref other => panic!("{other:?}"),
            };
            let Shape::Lifeline { head, .. } = view.get(b).unwrap().shape else { panic!() };
            (y, head.center().y)
        };
        let (y, head) = at(&doc);
        // The call to itself takes the create message sent from inside it along, and with it
        // the head it made and the constructor's bar.
        for to in [250.0, 120.0] {
            doc.edit("move", |tx| super::move_item(tx, d, own, to)).unwrap();
            let (moved, moved_head) = at(&doc);
            assert_eq!(moved - y, moved_head - head);
            assert!(
                super::violations(doc.model(), d).is_empty(),
                "{:?}",
                super::violations(doc.model(), d)
            );
        }
    }

    #[test]
    fn a_fragment_dragged_into_the_one_it_holds_goes_inside_in_one_go() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let (inner, inner_node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[call])).unwrap();
        let (outer, outer_node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[inner])).unwrap();
        let bounds = |doc: &Document, node: Id| {
            doc.model().view(d).unwrap().get(node).unwrap().bounds().unwrap()
        };
        // Its top dragged below the inner one's, the outer frame now lies inside that.
        let dy = bounds(&doc, inner_node).y - bounds(&doc, outer_node).y + 5.0;
        let moved = bounds(&doc, outer_node).translate(0.0, dy);
        doc.edit("move", |tx| tx.set_bounds(d, outer_node, moved).and(super::tidy(tx, d))).unwrap();
        assert!(doc.model().is_ancestor(inner, outer));
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn a_fragment_made_smaller_keeps_its_last_operand() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let (fragment, node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[call])).unwrap();
        doc.edit("operand", |tx| palette::add_operand(tx, fragment)).unwrap();
        let shape = |doc: &Document| doc.model().view(d).unwrap().get(node).unwrap().shape.clone();
        let Shape::Fragment { bounds, separators, .. } = shape(&doc) else { unreachable!() };
        // Dragged up to above its operand line, the bottom stops below it.
        let smaller = Rect { height: separators[0] - 10.0, ..bounds };
        doc.edit("resize", |tx| tx.set_bounds(d, node, smaller).and(super::tidy(tx, d))).unwrap();
        let Shape::Fragment { bounds, separators, .. } = shape(&doc) else { unreachable!() };
        assert_eq!(bounds.height, separators[0] + super::NEXT / 2.0);
        assert!(
            super::violations(doc.model(), d).is_empty(),
            "{:?}",
            super::violations(doc.model(), d)
        );
    }

    #[test]
    fn an_operand_line_clears_the_bars_above_it() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let (fragment, node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[call])).unwrap();
        doc.edit("operand", |tx| palette::add_operand(tx, fragment)).unwrap();
        let line = |doc: &Document| match &doc.model().view(d).unwrap().get(node).unwrap().shape {
            Shape::Fragment { bounds, separators, .. } => bounds.y + separators[0],
            other => panic!("{other:?}"),
        };
        let set_line = |doc: &mut Document, y: f32| {
            doc.edit("line", |tx| {
                tx.update_presentation(d, node, |p| {
                    if let Shape::Fragment { bounds, separators, .. } = &mut p.shape {
                        separators[0] = y - bounds.y;
                    }
                })?;
                super::tidy(tx, d)
            })
            .unwrap();
        };
        // The caller's bar on a ends at 165, the called one on b at 145: a line across them
        // goes down below both.
        assert_eq!(bars(&doc, d), [(120.0, 145.0), (120.0, 165.0)]);
        let clear = 165.0 + super::FRAGMENT_PADDING;
        set_line(&mut doc, 135.0);
        assert_eq!(line(&doc), clear);
        // Not when the next operand would lose the room for a message's label.
        let next = draw(&mut doc, d, EdgeTool::FoundMessage, b, b, clear + 10.0);
        set_line(&mut doc, 135.0);
        assert_eq!(line(&doc), 135.0);
        doc.edit("move", |tx| super::move_item(tx, d, next, clear + super::NEXT / 2.0)).unwrap();
        set_line(&mut doc, 135.0);
        assert_eq!(line(&doc), clear);
        doc.model().check().unwrap();
    }

    #[test]
    fn frames_follow_what_they_cover() {
        let (mut doc, d, a, b) = two_lifelines();
        let call = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let (fragment, node) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[call])).unwrap();
        let lifeline = |p: Id| doc.model().view(d).unwrap().get(p).unwrap().element.unwrap();
        let (la, lb) = (lifeline(a), lifeline(b));
        let frame =
            |doc: &Document| doc.model().view(d).unwrap().get(node).unwrap().bounds().unwrap();
        let head = |doc: &Document, p: Id| match doc.model().view(d).unwrap().get(p).unwrap().shape
        {
            Shape::Lifeline { head, .. } => head,
            ref other => panic!("{other:?}"),
        };
        let set = |doc: &mut Document, covered: Vec<Id>| {
            doc.edit("cover", |tx| {
                tx.update(fragment, |data| {
                    if let ElementKind::CombinedFragment(p) = &mut data.kind {
                        p.covered = covered;
                    }
                })?;
                super::cover(tx, fragment)
            })
            .unwrap();
        };
        set(&mut doc, vec![lb]);
        assert_eq!(frame(&doc).x, head(&doc, b).x - super::FRAGMENT_MARGIN);
        set(&mut doc, vec![la, lb]);
        assert_eq!(frame(&doc).x, head(&doc, a).x - super::FRAGMENT_MARGIN);
        assert_eq!(frame(&doc).right(), head(&doc, b).right() + super::FRAGMENT_MARGIN);
        doc.model().check().unwrap();
    }

    #[test]
    fn a_destroy_above_the_head_leaves_a_line() {
        let (mut doc, d, a, b) = two_lifelines();
        let top = match doc.model().view(d).unwrap().get(b).unwrap().shape {
            Shape::Lifeline { head, .. } => head.y,
            ref other => panic!("{other:?}"),
        };
        draw(&mut doc, d, EdgeTool::DestroyMessage, a, b, top - 30.0);
        match doc.model().view(d).unwrap().get(b).unwrap().shape {
            Shape::Lifeline { length, .. } => assert!(length > 0.0, "{length}"),
            ref other => panic!("{other:?}"),
        }
        doc.model().check().unwrap();
    }

    #[test]
    fn fragments_take_what_is_drawn_inside() {
        let (mut doc, d, a, b) = two_lifelines();
        let first = draw(&mut doc, d, EdgeTool::Message, a, b, 120.0);
        let second = draw(&mut doc, d, EdgeTool::AsyncMessage, a, b, 200.0);
        let (fragment, _) =
            doc.edit("wrap", |tx| palette::wrap_in_fragment(tx, d, &[second])).unwrap();
        let operand = doc.model().children(fragment)[0];
        assert_eq!(doc.model().get(second).unwrap().owner(), Some(operand));
        assert_ne!(doc.model().get(first).unwrap().owner(), Some(operand));
        // Moved below the frame's top, the first message goes into the operand, before the
        // second, and takes its bar along; the bar now holds the second call's bar and grows.
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let slot = Slot { y: 190.0, owner: interaction, index: 0 };
        doc.edit("move", |tx| palette::move_to_slot(tx, first, slot)).unwrap();
        assert_eq!(doc.model().children(operand), [first, second]);
        assert!(bars(&doc, d).contains(&(190.0, 245.0)), "{:?}", bars(&doc, d));
        doc.model().check().unwrap();
    }

    #[test]
    fn found_and_lost_messages_end_beside_their_lifeline() {
        let (mut doc, d, a, _) = two_lifelines();
        draw(&mut doc, d, EdgeTool::FoundMessage, a, a, 120.0);
        draw(&mut doc, d, EdgeTool::LostMessage, a, a, 200.0);
        let scene = scene_of(&doc, d);
        let layout = scene.sequence.as_ref().unwrap();
        assert_eq!(layout.gates.len(), 2);
        let center = layout.lanes.iter().find(|l| l.presentation == a).unwrap().center;
        assert!(layout.gates.iter().any(|g| g.x < center));
        assert!(layout.gates.iter().any(|g| g.x > center));
    }

    #[test]
    fn a_lost_message_answers_as_a_reply_from_the_end_of_its_bar() {
        let (mut doc, d, a, _) = two_lifelines();
        draw(&mut doc, d, EdgeTool::FoundMessage, a, a, 120.0);
        let lost = draw(&mut doc, d, EdgeTool::LostMessage, a, a, 130.0);
        let ElementKind::Message(props) = doc.model().element(lost).unwrap().kind() else {
            unreachable!()
        };
        assert_eq!(props.sort, umlcraft_model::MessageSort::Reply);
        assert!(matches!(message(&doc, d, lost), Shape::Message { y: 145.0, .. }));
        assert_eq!(bars(&doc, d), [(120.0, 145.0)]);
        // Moved down, it takes the end of the bar along, and back up it shortens it.
        doc.edit("move", |tx| super::move_item(tx, d, lost, 250.0)).unwrap();
        assert_eq!(bars(&doc, d), [(120.0, 250.0)]);
        doc.edit("move", |tx| super::move_item(tx, d, lost, 180.0)).unwrap();
        assert_eq!(bars(&doc, d), [(120.0, 180.0)]);
        doc.model().check().unwrap();
    }

    #[test]
    fn the_open_end_of_a_lost_message_can_be_moved_and_follows_its_lifeline() {
        let (mut doc, d, a, _) = two_lifelines();
        let lost = draw(&mut doc, d, EdgeTool::LostMessage, a, a, 130.0);
        let p = doc.model().view(d).unwrap().presentations_of(lost).next().unwrap().id;
        let gate = |doc: &Document| match message(doc, d, lost) {
            Shape::Message { gate_x, .. } => gate_x.unwrap(),
            _ => unreachable!(),
        };
        let center = |doc: &Document| match &doc.model().view(d).unwrap().get(a).unwrap().shape {
            Shape::Lifeline { head, .. } => head.center().x,
            _ => unreachable!(),
        };
        let c = center(&doc);
        // Longer, then over to the left, then too close: it keeps its least distance.
        doc.edit("gate", |tx| super::set_gate(tx, d, p, c + 200.0)).unwrap();
        assert_eq!(gate(&doc), c + 200.0);
        doc.edit("gate", |tx| super::set_gate(tx, d, p, c - 90.0)).unwrap();
        assert_eq!(gate(&doc), c - 90.0);
        doc.edit("gate", |tx| super::set_gate(tx, d, p, c - 5.0)).unwrap();
        assert_eq!(gate(&doc), c - super::MIN_GATE_DISTANCE);
        // The lifeline moves, the message keeps its length.
        crate::edit::apply(&mut doc, "move", &ApproxMeasure, |tx| {
            let head = match &tx.model().view(d).unwrap().get(a).unwrap().shape {
                Shape::Lifeline { head, .. } => *head,
                _ => unreachable!(),
            };
            tx.set_bounds(d, a, head.translate(100.0, 0.0))
        })
        .unwrap();
        assert_eq!(gate(&doc), c + 100.0 - super::MIN_GATE_DISTANCE);
    }

    #[test]
    fn old_diagrams_get_their_geometry() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        // The sample as UmlCraft stored it before: lifeline nodes, message edges and a
        // fragment node, with the order of the interaction as the only order.
        let model: &Model = doc.model();
        let interaction = model.get(d).unwrap().owner().unwrap();
        let messages: Vec<(Id, Id, Id)> = model
            .subtree(interaction)
            .into_iter()
            .filter_map(|id| match model.get(id).unwrap().kind() {
                ElementKind::Message(p) => Some((id, p.from, p.to)),
                _ => None,
            })
            .collect();
        let lifelines = [messages[0].1, messages[0].2];
        let fragment =
            model.children(interaction).iter().copied().find(|&c| {
                matches!(model.get(c).unwrap().kind(), ElementKind::CombinedFragment(_))
            });
        let old = doc.model().view(d).unwrap().presentations().iter().map(|p| p.id).collect();
        doc.edit("old", |tx| {
            tx.remove_presentations(d, old)?;
            tx.add_node(d, fragment, Rect::new(40.0, 100.0, 320.0, 80.0))?;
            let a = tx.add_node(d, Some(lifelines[0]), Rect::new(60.0, 40.0, 100.0, 300.0))?;
            let b = tx.add_node(d, Some(lifelines[1]), Rect::new(260.0, 40.0, 100.0, 300.0))?;
            for &(m, from, _) in &messages {
                let (s, t) = if from == lifelines[0] { (a, b) } else { (b, a) };
                tx.add_edge(d, Some(m), s, t, vec![])?;
            }
            Ok(())
        })
        .unwrap();
        super::upgrade_document(&mut doc);
        assert!(!doc.can_undo() && !doc.is_dirty());
        let shapes = shapes(&doc, d);
        assert!(shapes.iter().all(|s| !matches!(s, Shape::Edge { .. })));
        assert_eq!(shapes.iter().filter(|s| matches!(s, Shape::Lifeline { .. })).count(), 2);
        assert_eq!(shapes.iter().filter(|s| matches!(s, Shape::Message { .. })).count(), 3);
        let scene = scene_of(&doc, d);
        assert_eq!(labels(&scene), ["1: addItem(item)", "2: submit() : confirmation", ""]);
        // The first message is still in the loop.
        let first = doc.model().get(messages[0].0).unwrap();
        let owner = doc.model().get(first.owner().unwrap()).unwrap();
        assert!(matches!(owner.kind(), ElementKind::InteractionOperand(_)));
        doc.model().check().unwrap();
    }
}
