//! Editing sequence diagrams the way Astah does. The geometry is what is stored; edits place
//! things where the user puts them and then tidy up:
//!
//! - A call gets a bar of 25 on the receiver; a sender without a bar there gets one of 45 (60
//!   for a create message, 30 for a lost message). A reply leaves at the end of its bar.
//! - Bars grow to end 20 below the last thing inside them, lifelines to reach 30 below the last
//!   thing on them, the diagram frame to surround everything.
//! - The interaction's order follows the heights: messages and fragments belong to the operand
//!   of the innermost fragment drawn around them, in the order they are drawn.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    Document, ElementKind, Id, MessageGate, MessageSort, Model, ModelError, Presentation, Rect,
    Result, Shape, Tx, View,
};

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
/// How far a message to self comes back below where it leaves.
const SELF_DROP: f32 = 20.0;
/// How far the open end of a found or lost message is from its lifeline.
const GATE_DISTANCE: f32 = 120.0;
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
                Shape::Activation { lifeline, top, height } => {
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
                Shape::Node { bounds } if p.element == Some(diagram) => {
                    g.diagram_frame = Some((p.id, *bounds));
                }
                Shape::Node { bounds } => {
                    match p.element.and_then(|e| model.get(e)).map(|e| e.kind()) {
                        Some(ElementKind::InteractionUse(_)) => g.frames.push(Frame {
                            id: p.id,
                            element: p.element.expect("matched"),
                            rect: *bounds,
                            separators: Vec::new(),
                        }),
                        _ => g.notes.push(*bounds),
                    }
                }
                Shape::Edge { .. } => {}
            }
        }
        Ok(g)
    }

    fn lane(&self, id: Id) -> Option<&Lane> {
        self.lanes.iter().find(|l| l.id == id)
    }

    /// The innermost bar on `lane` at `y`.
    fn bar_at(&self, lane: Id, y: f32) -> Option<Bar> {
        self.bars
            .iter()
            .filter(|b| b.lane == lane && b.top - 0.5 <= y && y <= b.bottom + 0.5)
            .min_by(|a, b| (a.bottom - a.top).total_cmp(&(b.bottom - b.top)))
            .copied()
    }

    /// The lowest point of what is drawn between the heads and the lifelines' ends.
    fn content_bottom(&self) -> Option<f32> {
        let messages = self.messages.iter().map(|m| m.end());
        let bars = self.bars.iter().map(|b| b.bottom);
        let frames = self.frames.iter().map(|f| f.rect.bottom());
        messages.chain(bars).chain(frames).reduce(f32::max)
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
    let Ok(g) = Geometry::read(model, diagram) else { return 100.0 };
    let heads = g.lanes.iter().map(|l| l.line_top()).reduce(f32::max).unwrap_or(60.0);
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
        .filter(|l| !l.destroyed)
        .map(|l| l.bottom())
        .reduce(f32::max)
        .map_or(LIFELINE_LENGTH, |bottom| (bottom - head.bottom()).max(60.0));
    let id = tx.add_presentation(diagram, Presentation::lifeline(lifeline, head, length))?;
    tidy(tx, diagram)?;
    Ok(id)
}

fn add_bar(tx: &mut Tx<'_>, diagram: Id, lane: Id, top: f32, height: f32) -> Result<Id> {
    tx.add_presentation(diagram, Presentation::activation(lane, top, height))
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
    let sender = |tx: &mut Tx<'_>, height: f32| -> Result<()> {
        if g.bar_at(from.id, y).is_none() {
            add_bar(tx, diagram, from.id, y, height)?;
        }
        Ok(())
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
            sender(tx, LOST_SENDER_BAR)?;
            if let Shape::Message { gate_x, .. } = &mut presentation.shape {
                *gate_x = Some(from.center() + GATE_DISTANCE);
            }
        }
        (_, MessageSort::Reply) => {
            // A reply leaves at the end of the bar it returns from: the one at `y`, else the
            // closest one above.
            let above = || {
                g.bars
                    .iter()
                    .filter(|b| b.lane == from.id && b.bottom <= y)
                    .max_by(|a, b| a.bottom.total_cmp(&b.bottom))
                    .copied()
            };
            if let (Some(bar), Shape::Message { y: at, .. }) =
                (g.bar_at(from.id, y).or_else(above), &mut presentation.shape)
            {
                *at = bar.bottom;
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
            // The new lifeline's head comes down to the message; its line keeps its end.
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
            let length = y + RECEIVER_BAR - to.line_top();
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
    let left = lanes.iter().map(|l| l.head.x).reduce(f32::min).unwrap_or(40.0) - FRAGMENT_MARGIN;
    let right =
        lanes.iter().map(|l| l.head.right()).reduce(f32::max).unwrap_or(240.0) + FRAGMENT_MARGIN;
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

/// Makes room for a new last operand in the frames of `fragment`.
pub fn add_operand_space(tx: &mut Tx<'_>, fragment: Id) -> Result<()> {
    let shown: Vec<(Id, Id)> = tx
        .model()
        .views()
        .flat_map(|(diagram, view)| view.presentations_of(fragment).map(move |p| (diagram, p.id)))
        .collect();
    for (diagram, id) in shown {
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

/// Moves a message, together with the bar it starts and everything sent from it, or a
/// fragment's frame, so that it starts at height `y`.
pub fn move_item(tx: &mut Tx<'_>, diagram: Id, element: Id, y: f32) -> Result<()> {
    let model = tx.model();
    let g = Geometry::read(model, diagram)?;
    if let Some(frame) = g.frames.iter().find(|f| f.element == element) {
        let dy = y - frame.rect.y;
        let id = frame.id;
        tx.update_presentation(diagram, id, |p| match &mut p.shape {
            Shape::Fragment { bounds, .. } | Shape::Node { bounds } => bounds.y += dy,
            _ => {}
        })?;
        return tidy(tx, diagram);
    }
    let Some(m) = g.messages.iter().find(|m| m.element == Some(element)).copied() else {
        return Ok(());
    };
    let dy = y - m.y;
    if dy.abs() < 0.01 {
        return Ok(());
    }
    // The bar the message starts and, from the inside out, what lies within it.
    let mut bars: HashSet<Id> = HashSet::new();
    let mut messages: HashSet<Id> = HashSet::from([m.id]);
    let mut queue: Vec<Bar> = g
        .bars
        .iter()
        .filter(|b| b.lane == m.target && (b.top - m.end()).abs() < 0.5)
        .copied()
        .collect();
    while let Some(bar) = queue.pop() {
        if !bars.insert(bar.id) {
            continue;
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
                queue.extend(
                    g.bars
                        .iter()
                        .filter(|b| b.lane == other.target && (b.top - other.end()).abs() < 0.5)
                        .copied(),
                );
            }
        }
    }
    for id in bars {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Activation { top, .. } = &mut p.shape {
                *top += dy;
            }
        })?;
    }
    for id in messages {
        tx.update_presentation(diagram, id, |p| {
            if let Shape::Message { y, end_y, .. } = &mut p.shape {
                *y += dy;
                *end_y = end_y.map(|e| e + dy);
            }
        })?;
    }
    if m.sort == MessageSort::Create
        && let Some(to) = g.lane(m.target)
    {
        let bottom = to.bottom();
        tx.update_presentation(diagram, to.id, |p| {
            if let Shape::Lifeline { head, length, .. } = &mut p.shape {
                head.y += dy;
                *length = (bottom - head.bottom()).max(RECEIVER_BAR + BAR_MARGIN);
            }
        })?;
    }
    tidy(tx, diagram)
}

/// Grows bars, lifelines and the diagram frame around their content and puts the
/// interaction's messages and fragments in the order and operands they are drawn in.
pub fn tidy(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    drop_idle_bars(tx, diagram)?;
    fit_bars(tx, diagram)?;
    fit_fragments(tx, diagram)?;
    fit_lifelines(tx, diagram)?;
    fit_frame(tx, diagram)?;
    sync_order(tx, diagram)
}

/// Tidies every sequence diagram, e.g. after deleting elements that may have been shown there.
pub fn tidy_all(tx: &mut Tx<'_>) -> Result<()> {
    let diagrams: Vec<Id> = tx
        .model()
        .elements()
        .filter(|e| e.kind().as_diagram() == Some(umlcraft_model::DiagramKind::Sequence))
        .map(|e| e.id())
        .collect();
    diagrams.into_iter().try_for_each(|d| tidy(tx, d))
}

/// Removes bars that no message reaches any more, as Astah removes a bar with the message that
/// started it.
fn drop_idle_bars(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    let touches = |bar: &Bar, lane: Id, y: f32| {
        lane == bar.lane && bar.top - 0.5 <= y && y <= bar.bottom + 0.5
    };
    let idle: indexmap::IndexSet<Id> = g
        .bars
        .iter()
        .filter(|bar| {
            !g.messages
                .iter()
                .any(|m| touches(bar, m.source, m.y) || touches(bar, m.target, m.end()))
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
            }
            if need > bar.bottom + 0.01 {
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
/// Astah refuses a bar that reaches out of the fragment its message lies in.
fn fit_fragments(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    // Inner fragments first, so that outer ones grow around them.
    for _ in 0..8 {
        let g = Geometry::read(tx.model(), diagram)?;
        let center = |lane: Id| g.lane(lane).map_or(f32::MIN, |l| l.center());
        let mut grown = Vec::new();
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
            let messages = g
                .messages
                .iter()
                .filter(|m| inside(center(m.source), m.y))
                .map(|m| m.y.max(m.end()));
            let frames = g
                .frames
                .iter()
                .filter(|o| o.id != f.id && inside(o.rect.x, o.rect.y))
                .map(|o| o.rect.bottom());
            let need = bars
                .chain(messages)
                .chain(frames)
                .fold(r.bottom(), |n, b| n.max(b + FRAGMENT_PADDING));
            if need > r.bottom() + 0.01 {
                grown.push((f.id, need - r.y));
            }
        }
        if grown.is_empty() {
            return Ok(());
        }
        for (id, height) in grown {
            tx.update_presentation(diagram, id, |p| {
                if let Shape::Fragment { bounds, .. } = &mut p.shape {
                    bounds.height = height;
                }
            })?;
        }
    }
    Ok(())
}

fn fit_lifelines(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let g = Geometry::read(tx.model(), diagram)?;
    for lane in g.lanes.iter().filter(|l| !l.destroyed) {
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
        let Some(last) = bars.chain(messages).chain(frames).reduce(f32::max) else { continue };
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
    let Some(content) =
        lanes.chain(frames).chain(g.notes.iter().copied()).reduce(|a, b| a.union(&b))
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
    let descends = |item: Id, from: Id| {
        let mut current = model.get(item).and_then(|e| e.owner());
        while let Some(id) = current {
            if id == from {
                return true;
            }
            current = model.get(id).and_then(|e| e.owner());
        }
        false
    };
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
            // A fragment never goes into its own operands.
            .filter(|f| !descends(f.element, element))
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
    let mut moves = Vec::new();
    for owner in owners {
        let mut list = wanted.remove(&owner).unwrap_or_default();
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        // What is not shown here keeps its place before the shown items.
        let kept = model.children(owner).iter().filter(|c| !shown.contains(c)).count();
        for (i, (_, element)) in list.into_iter().enumerate() {
            moves.push((element, owner, kept + i));
        }
    }
    // Into place from the first index on, so that later moves do not disturb earlier ones.
    moves.sort_by_key(|m| m.2);
    for (element, owner, index) in moves {
        let model = tx.model();
        let current = model.get(element).and_then(|e| e.owner());
        let position = model.children(owner).iter().position(|&c| c == element);
        if current == Some(owner) && position == Some(index) {
            continue;
        }
        let index = index.min(model.children(owner).len() - usize::from(current == Some(owner)));
        tx.move_element(element, owner, index)?;
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
    fn the_sample_is_drawn_from_its_geometry() {
        let (doc, ids) = order_system();
        let scene = scene_of(&doc, ids.sequence_diagram);
        // As in Astah, the reply's return value follows the call it answers.
        assert_eq!(labels(&scene), ["1: addItem(item)", "2: submit() : confirmation", ""]);
        let count = |f: fn(&Figure) -> bool| scene.nodes.iter().filter(|n| f(&n.figure)).count();
        assert_eq!(count(|f| matches!(f, Figure::Lifeline { .. })), 2);
        assert_eq!(count(|f| matches!(f, Figure::Activation)), 3);
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
        draw(&mut doc, d, EdgeTool::DestroyMessage, a, b, 260.0);
        let (head, length, destroyed) = lane(&doc);
        assert!(destroyed);
        assert_eq!(head.bottom() + length, 285.0);
        assert_eq!(scene_of(&doc, d).sequence.unwrap().destructions.len(), 1);
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
