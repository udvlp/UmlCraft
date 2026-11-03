//! Activity diagrams: pins on the border of their action and activity parameter nodes on the
//! diagram's frame.
//!
//! As with ports ([`crate::composite`]), where they go follows from the drawing: a new pin
//! belongs to the action whose border is nearest to where it is dropped and sits just outside
//! that border, as Astah draws pins; an activity parameter node belongs to the activity and sits
//! on the frame, which is added around the drawing when the diagram has none.
//! [`composite::follow`](crate::composite::follow) keeps both there after every edit.

use std::collections::HashMap;

use umlcraft_model::{
    ElementKind, Id, Model, ModelError, Point, Presentation, Rect, Result, Size, Tx, View,
};

use crate::composite::{border_distance, snap};
use crate::palette::{NodeTool, owner_for, unique_name};
use crate::scene::{
    self, EXCEPTION_GAP, EXCEPTION_HEIGHT, EXCEPTION_WIDTH, PIN_SIZE, Preview, TextMeasure,
};

/// Why a pin could not be placed, as [`ModelError::Invalid`] says it; the app words it in the
/// language of its user.
pub const PIN_OUTSIDE: &str = "a pin goes on an action";

/// How close to an action's border a new pin must be dropped to go on it, when not dropped on
/// the action itself.
const NEAR_ACTION: f32 = 20.0;

/// Room the frame Astah draws leaves around the drawing: below its tab, and on the other sides.
const FRAME_TOP: f32 = 40.0;
const FRAME_MARGIN: f32 = 20.0;

fn is_action(model: &Model, p: &Presentation) -> bool {
    p.element
        .and_then(|e| model.get(e))
        .is_some_and(|e| matches!(e.kind(), ElementKind::ActivityNode(n) if n.node.is_action()))
}

/// The action node of `view` a pin dropped with its center at `at` goes on: the one drawn
/// around the point, else the one whose border is nearest, within reach.
fn action_at<'v>(model: &Model, view: &'v View, at: Point) -> Option<&'v Presentation> {
    view.presentations()
        .iter()
        .filter(|p| is_action(model, p))
        .filter_map(|p| Some((p, p.bounds()?)))
        .map(|(p, r)| (p, if r.contains(at) { -1.0 } else { border_distance(r, at) }))
        .filter(|(_, d)| *d <= NEAR_ACTION)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// The frame of `diagram` (a node of the diagram itself), if it has one.
pub fn frame(view: &View, diagram: Id) -> Option<&Presentation> {
    view.presentations().iter().find(|p| p.element == Some(diagram) && p.bounds().is_some())
}

/// The bounds of a frame drawn around `content`, with room for its tab.
pub(crate) fn frame_around(content: Rect) -> Rect {
    Rect::new(
        content.x - FRAME_MARGIN,
        content.y - FRAME_TOP,
        content.width + 2.0 * FRAME_MARGIN,
        content.height + FRAME_TOP + FRAME_MARGIN,
    )
}

/// Adds a frame to `diagram` around everything drawn on it and `at`, behind the rest, and
/// returns its bounds.
pub(crate) fn add_frame(tx: &mut Tx<'_>, diagram: Id, at: Point) -> Result<Rect> {
    let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let content = view
        .presentations()
        .iter()
        .filter_map(|p| p.bounds())
        .fold(Rect::new(at.x, at.y, 0.0, 0.0), |a, b| a.union(&b));
    let bounds = frame_around(content);
    let presentation = umlcraft_model::Presentation::node(Some(diagram), bounds);
    tx.apply(umlcraft_model::Op::InsertPresentation { diagram, index: 0, presentation })?;
    Ok(bounds)
}

/// Creates a pin or an activity parameter node with `tool` at `origin` on the activity diagram
/// `diagram`. A pin goes on the action at or nearest to the point and belongs to it; an
/// activity parameter node belongs to the activity and goes on the frame's border nearest to
/// the point, the frame being added first if the diagram has none. Returns (element,
/// presentation).
pub fn create_node(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    origin: Point,
    base_name: &str,
) -> Result<(Id, Id)> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let mut bounds = Rect::from_origin_size(origin, tool.default_size());
    let at = bounds.center();
    let kind = tool.element_kind();
    let owner = match tool {
        NodeTool::InputPin | NodeTool::OutputPin => {
            let action = action_at(model, view, at)
                .ok_or_else(|| ModelError::Invalid(PIN_OUTSIDE.into()))?;
            let r = action.bounds().ok_or(ModelError::NotFound(action.id))?;
            bounds = snap(bounds, r.inflate(PIN_SIZE / 2.0));
            action.element.ok_or(ModelError::NotFound(action.id))?
        }
        _ => {
            let start = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
            let owner = owner_for(model, start, &kind)
                .ok_or_else(|| ModelError::Invalid("no activity for the parameter".into()))?;
            let framed = frame(view, diagram).and_then(|f| f.bounds());
            let r = match framed {
                Some(r) => r,
                None => add_frame(tx, diagram, at)?,
            };
            bounds = snap(bounds, r);
            owner
        }
    };
    let name = unique_name(tx.model(), owner, base_name);
    let element = tx.create(owner, tool.element_data(name))?;
    let node = tx.add_node(diagram, Some(element), bounds)?;
    Ok((element, node))
}

// ----- Pins and their texts --------------------------------------------------------------------
//
// Astah draws a pin's name, state and properties centered below the pin (stacked above it on
// the top side) wherever the pin is, and lets them run over the texts of other pins: pins
// placed by hand close together have overlapping texts there, and so do imported drawings
// here. Where UmlCraft places pins itself, a new pin on an action and arranging a diagram,
// it keeps what each pin shows clear of the others.

/// Room kept between what one pin of an action shows (its square, texts and exception
/// triangle) and what another one shows.
pub const PIN_GAP: f32 = 5.0;

/// How far an action grows at most for a new pin.
const MAX_GROWTH: f32 = 2000.0;

/// A pin of an action as drawn: its node, the side of the action it is on (0 top, 1 right,
/// 2 bottom, 3 left), where its center is along that side (y on the left and right, x on the
/// top and bottom), the sizes of its lines of text and whether it shows an exception triangle.
#[derive(Clone, Debug)]
struct PinLabel {
    node: Id,
    side: u8,
    at: f32,
    rows: Vec<Size>,
    exception: bool,
}

impl PinLabel {
    fn width(&self) -> f32 {
        self.rows.iter().map(|r| r.width).fold(0.0, f32::max)
    }

    fn height(&self) -> f32 {
        self.rows.iter().map(|r| r.height).sum()
    }

    /// How far its triangle reaches above its square.
    fn mark(&self) -> f32 {
        if self.exception { EXCEPTION_HEIGHT } else { 0.0 }
    }

    /// How far its texts reach past its square across the side, into the action on the left
    /// and right sides.
    fn reach_in(&self) -> f32 {
        (self.width() / 2.0 - PIN_SIZE / 2.0).max(0.0)
    }

    /// How far what it shows reaches left and right of its center on the top or bottom side:
    /// its texts, its square and its triangle on the left.
    fn span(&self) -> (f32, f32) {
        let half = PIN_SIZE / 2.0;
        let text = self.width() / 2.0;
        let left = if self.exception { half + EXCEPTION_GAP + EXCEPTION_WIDTH } else { half };
        (text.max(left), text.max(half))
    }

    /// What it covers with its square at `square`.
    fn cover(&self, square: Rect) -> Rect {
        scene::pin_cover(square, self.side, &self.rows, self.exception)
    }
}

/// The square of a pin on `side` of an action drawn at `r`, its center `at` along that side.
fn pin_square(r: Rect, side: u8, at: f32) -> Rect {
    let (s, half) = (PIN_SIZE, PIN_SIZE / 2.0);
    match side {
        0 => Rect::new(at - half, r.y - s, s, s),
        1 => Rect::new(r.right(), at - half, s, s),
        2 => Rect::new(at - half, r.bottom(), s, s),
        _ => Rect::new(r.x - s, at - half, s, s),
    }
}

/// Whether `a` and `b` come closer than [`PIN_GAP`] (with a little slack for rounding).
fn crowds(a: Rect, b: Rect) -> bool {
    let gap = PIN_GAP - 0.01;
    a.x < b.right() + gap
        && b.x < a.right() + gap
        && a.y < b.bottom() + gap
        && b.y < a.bottom() + gap
}

/// The name of an action as drawn, which the texts of its pins keep off: the width of its
/// widest line, the height of its lines and how far their middle lies below the action's
/// middle. The name stays centered when the action grows.
#[derive(Clone, Copy, Debug)]
struct NameBlock {
    width: f32,
    height: f32,
    offset: f32,
}

/// An action node with the pins on its border, as drawn.
struct ActionPins {
    node: Id,
    rect: Rect,
    name: Option<NameBlock>,
    pins: Vec<PinLabel>,
}

/// The pins of the actions of `diagram` as `scene` draws them, by action node. Only the
/// actions in `hosts` (presentations), or all.
fn pins_by_action(
    model: &Model,
    diagram: Id,
    hosts: Option<&[Id]>,
    scene: &scene::Scene,
    measure: &dyn TextMeasure,
) -> Vec<ActionPins> {
    let Some(view) = model.view(diagram) else { return Vec::new() };
    let mut actions: HashMap<Id, Vec<PinLabel>> = HashMap::new();
    for (pin, host) in crate::composite::borne_nodes(model, diagram) {
        let Some(host) = host.filter(|h| hosts.is_none_or(|hs| hs.contains(h))) else { continue };
        let props = view.get(pin).and_then(|p| p.element).and_then(|e| model.get(e));
        let Some(ElementKind::ActivityNode(props)) = props.map(|e| e.kind()) else { continue };
        let (Some(shape), Some(action)) = (scene.node(pin), scene.node(host)) else { continue };
        if !props.node.is_pin() {
            continue;
        }
        let c = shape.rect.center();
        let side = crate::composite::border_place(action.rect.inflate(PIN_SIZE / 2.0), c).0;
        actions.entry(host).or_default().push(PinLabel {
            node: pin,
            side,
            at: if side.is_multiple_of(2) { c.x } else { c.y },
            rows: shape.rows.iter().map(|r| r.rect.size()).collect(),
            exception: props.is_exception,
        });
    }
    let mut actions: Vec<ActionPins> = actions
        .into_iter()
        .filter_map(|(node, pins)| {
            let shape = scene.node(node)?;
            let name = shape.rows.iter().map(|r| r.rect).reduce(|a, b| a.union(&b)).map(|block| {
                NameBlock {
                    width: shape
                        .rows
                        .iter()
                        .map(|r| measure.width(&r.text, r.size, r.bold))
                        .fold(0.0, f32::max),
                    height: block.height,
                    offset: block.center().y - shape.rect.center().y,
                }
            });
            Some(ActionPins { node, rect: shape.rect, name, pins })
        })
        .collect();
    actions.sort_by_key(|a| a.node);
    actions
}

/// How wide an action drawn at `r` must be for its `name`, centered, to stay clear of the
/// texts that the pins on its left and right sides (`placed`, each with its square) reach
/// into it level with the name.
fn name_room(r: Rect, name: Option<NameBlock>, placed: &[(&PinLabel, Rect)]) -> f32 {
    let Some(name) = name else { return 0.0 };
    let middle = r.center().y + name.offset;
    let top = middle - name.height / 2.0 - PIN_GAP;
    let bottom = middle + name.height / 2.0 + PIN_GAP;
    let reach = placed
        .iter()
        .filter(|(p, sq)| p.side % 2 == 1 && sq.bottom() < bottom && sq.bottom() + p.height() > top)
        .map(|(p, _)| p.reach_in())
        .fold(0.0, f32::max);
    if reach > 0.0 { 2.0 * (reach + PIN_GAP) + name.width } else { 0.0 }
}

/// Lays out the pins of an action drawn at `r` so that what one shows stays clear of what the
/// others show and of the action's name: along the left and right sides one below the other,
/// each one's texts above the next one, the lowest level with the action's bottom so that its
/// texts stand below the action, as a pin alone on such a side does; along the top and bottom
/// side by side, clear of the texts of the lowest pins on the left and right. The action grows
/// where a side is too short, as Astah's actions grow to hold the pins along their sides, and
/// widens where the texts of pins on its left and right would meet inside it or run into its
/// name. Returns the action's new bounds and each pin's square.
fn spread(r: Rect, name: Option<NameBlock>, pins: &[PinLabel]) -> (Rect, Vec<(Id, Rect)>) {
    let half = PIN_SIZE / 2.0;
    let mut sides: [Vec<&PinLabel>; 4] = Default::default();
    for p in pins {
        sides[p.side as usize % 4].push(p);
    }
    for side in &mut sides {
        side.sort_by(|a, b| a.at.total_cmp(&b.at).then(a.node.cmp(&b.node)));
    }
    let mut r = r;
    // One below the other: a square, its texts, the gap, the next one's triangle and square.
    let stack = |side: &[&PinLabel]| -> f32 {
        let n = side.len();
        side.iter()
            .enumerate()
            .map(|(i, p)| {
                let above = if i > 0 { p.mark() + PIN_GAP } else { 0.0 };
                let below = if i + 1 < n { p.height() } else { 0.0 };
                above + PIN_SIZE + below
            })
            .sum()
    };
    for s in [1, 3] {
        if sides[s].len() > 1 {
            r.height = r.height.max(stack(&sides[s]));
        }
    }
    // The squares down a side of an action at `r`, spread over its height.
    let column = |r: Rect, s: u8| -> Vec<(&PinLabel, Rect)> {
        let side = &sides[s as usize];
        if side.len() == 1 {
            return vec![(side[0], pin_square(r, s, r.bottom() - half))];
        }
        let extra = (r.height - stack(side)).max(0.0) / side.len().saturating_sub(1).max(1) as f32;
        let mut y = r.y;
        let mut squares = Vec::new();
        for (i, p) in side.iter().enumerate() {
            if i > 0 {
                y += p.mark();
            }
            squares.push((*p, pin_square(r, s, y + half)));
            y += PIN_SIZE + p.height() + PIN_GAP + extra;
        }
        squares
    };
    // Texts of pins on the left and right reach into the action from both sides, beside its
    // name.
    let reach_in = |side: &[&PinLabel]| side.iter().map(|p| p.reach_in()).fold(0.0, f32::max);
    if !sides[1].is_empty() && !sides[3].is_empty() {
        r.width = r.width.max(reach_in(&sides[1]) + reach_in(&sides[3]) + PIN_GAP);
    }
    let placed: Vec<(&PinLabel, Rect)> = [1, 3].into_iter().flat_map(|s| column(r, s)).collect();
    r.width = r.width.max(name_room(r, name, &placed));
    // Side by side, and below the action clear of the lowest pins' texts on the left and right.
    let row = |side: &[&PinLabel]| -> f32 {
        let spans: f32 = side.iter().map(|p| p.span().0 + p.span().1).sum();
        spans + PIN_GAP * side.len().saturating_sub(1) as f32
    };
    let corner = |side: &[&PinLabel]| side.last().map_or(0.0, |p| p.reach_in() + PIN_GAP);
    let ends = [(0.0, 0.0), (0.0, 0.0), (corner(&sides[3]), corner(&sides[1])), (0.0, 0.0)];
    for s in [0, 2] {
        if !sides[s].is_empty() {
            let (lo, hi) = ends[s];
            r.width = r.width.max(lo + row(&sides[s]) + hi);
        }
    }

    let mut squares: Vec<(Id, Rect)> =
        [1, 3].into_iter().flat_map(|s| column(r, s)).map(|(p, sq)| (p.node, sq)).collect();
    for s in [0u8, 2] {
        let side = &sides[s as usize];
        let (lo, hi) = ends[s as usize];
        let (from, to) = (r.x + lo, r.right() - hi);
        match side.len() {
            0 => {}
            1 => {
                let (a, b) = side[0].span();
                let at = side[0].at.min(to - b).max(from + a);
                squares.push((side[0].node, pin_square(r, s, at)));
            }
            n => {
                let extra = (to - from - row(side)).max(0.0) / (n + 1) as f32;
                let mut x = from + extra;
                for p in side {
                    let (a, b) = p.span();
                    squares.push((p.node, pin_square(r, s, x + a)));
                    x += a + b + PIN_GAP + extra;
                }
            }
        }
    }
    (r, squares)
}

/// Lays out the pins of the actions of `diagram` (only those of the action nodes in `hosts`,
/// or all) with [`spread`], growing actions where needed. Used when arranging a diagram.
pub(crate) fn spread_pins(
    tx: &mut Tx<'_>,
    diagram: Id,
    hosts: Option<&[Id]>,
    measure: &dyn TextMeasure,
) -> Result<()> {
    let model = tx.model();
    let scene = scene::build(model, diagram, &Preview::default(), measure);
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let bounds = |id: Id| view.get(id).and_then(|p| p.bounds());
    let mut actions = Vec::new();
    let mut pins = Vec::new();
    for action in pins_by_action(model, diagram, hosts, &scene, measure) {
        let (r, squares) = spread(action.rect, action.name, &action.pins);
        if r != action.rect {
            actions.push((action.node, r));
        }
        pins.extend(squares.into_iter().filter(|&(pin, sq)| bounds(pin) != Some(sq)));
    }
    for (id, r) in actions.into_iter().chain(pins) {
        tx.set_bounds(diagram, id, r)?;
    }
    Ok(())
}

/// How a new pin runs into another one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Clash {
    None,
    /// With one that moves away as the action widens.
    Wider,
    /// With one that widening does not move away from it.
    Fixed,
}

/// Finds a place for the new pin `pin` (a node of `diagram`) on its action where what it shows
/// stays clear of what the action's other pins show, which keep their places, and of the
/// action's name. It goes on its side, the left for an input pin and the right for an output
/// pin: in the middle, at the bottom or else the first free place from the top, or at the new
/// bottom of the action grown higher; where that is not enough, the action also widens. Of
/// these the place that grows the action least wins, the first one on a tie. Only if none is
/// found within reach, another side. Run after the pin has its name and properties.
pub fn place_pin(tx: &mut Tx<'_>, diagram: Id, pin: Id, measure: &dyn TextMeasure) -> Result<()> {
    let model = tx.model();
    let scene = scene::build(model, diagram, &Preview::default(), measure);
    let Some(host) = crate::composite::borne_nodes(model, diagram).get(&pin).copied().flatten()
    else {
        return Ok(());
    };
    let input = view_kind(model, diagram, pin)
        .is_some_and(|k| k == umlcraft_model::ActivityNodeKind::InputPin);
    let Some(action) =
        pins_by_action(model, diagram, Some(&[host]), &scene, measure).into_iter().next()
    else {
        return Ok(());
    };
    let r = action.rect;
    let Some(new) = action.pins.iter().find(|p| p.node == pin).cloned() else { return Ok(()) };
    let others: Vec<(&PinLabel, Rect)> = action
        .pins
        .iter()
        .filter(|p| p.node != pin)
        .filter_map(|p| Some((p, scene.node(p.node)?.extent())))
        .collect();
    let grown = |dx: f32, dy: f32| Rect::new(r.x, r.y, r.width + dx, r.height + dy);
    // How the pin on `side` at `at` runs into the others once the action has grown by `dx` to
    // the right and `dy` down; the pins on the right and bottom go along.
    let clash = |side: u8, at: f32, dx: f32, dy: f32| {
        let cover = PinLabel { side, ..new.clone() }.cover(pin_square(grown(dx, dy), side, at));
        let mut clash = Clash::None;
        for &(p, o) in &others {
            let o = match p.side {
                1 => o.translate(dx, 0.0),
                2 => o.translate(0.0, dy),
                _ => o,
            };
            if crowds(cover, o) {
                if (p.side == 1) == (side == 1) {
                    return Clash::Fixed;
                }
                clash = Clash::Wider;
            }
        }
        clash
    };
    // How much wider the action must be for the pin on `side` at `at`, with the action `dy`
    // higher: for its name, then for the pins' texts across it.
    let widen = |side: u8, at: f32, dy: f32| -> Option<f32> {
        let r = grown(0.0, dy);
        let label = PinLabel { side, at, ..new.clone() };
        let placed: Vec<(&PinLabel, Rect)> = others
            .iter()
            .map(|&(p, _)| p)
            .chain([&label])
            .map(|p| (p, pin_square(r, p.side, p.at)))
            .collect();
        let mut dx = (name_room(r, action.name, &placed) - r.width).max(0.0);
        loop {
            match clash(side, at, dx, dy) {
                Clash::None => return Some(dx),
                Clash::Fixed => return None,
                Clash::Wider if dx < MAX_GROWTH => dx += 1.0,
                Clash::Wider => return None,
            }
        }
    };
    let half = PIN_SIZE / 2.0;
    // Places along a side of `length` starting at `from`: the middle, the end, then from the
    // start on.
    let along = |from: f32, length: f32| {
        let n = (length - PIN_SIZE).max(0.0).floor() as usize;
        [from + length / 2.0, from + length - half]
            .into_iter()
            .chain((0..=n).map(move |i| from + half + i as f32))
    };
    let side = if input { 3 } else { 1 };
    // The least growth: (dx + dy, at, dx, dy).
    let mut best: Option<(f32, f32, f32, f32)> = None;
    for at in along(r.y, r.height) {
        if let Some(dx) = widen(side, at, 0.0)
            && best.is_none_or(|b| dx < b.0)
        {
            best = Some((dx, at, dx, 0.0));
            if dx == 0.0 {
                break;
            }
        }
    }
    let mut dy = 1.0;
    while dy <= MAX_GROWTH && best.is_none_or(|b| dy < b.0) {
        let at = r.bottom() + dy - half;
        if let Some(dx) = widen(side, at, dy)
            && best.is_none_or(|b| dx + dy < b.0)
        {
            best = Some((dx + dy, at, dx, dy));
        }
        dy += 1.0;
    }
    let chosen = best.map(|(_, at, dx, dy)| (side, at, dx, dy)).or_else(|| {
        // Another side, as the action is.
        let free = |side: u8, from: f32, length: f32| {
            along(from, length)
                .find(|&at| clash(side, at, 0.0, 0.0) == Clash::None)
                .map(|at| (side, at, 0.0, 0.0))
        };
        free(4 - side, r.y, r.height)
            .or_else(|| free(0, r.x, r.width))
            .or_else(|| free(2, r.x, r.width))
    });
    let Some((side, at, dx, dy)) = chosen else { return Ok(()) };
    let r2 = grown(dx, dy);
    if r2 != r {
        tx.set_bounds(diagram, host, r2)?;
    }
    tx.set_bounds(diagram, pin, pin_square(r2, side, at))?;
    Ok(())
}

/// The kind of activity node the node `node` of `diagram` shows.
fn view_kind(model: &Model, diagram: Id, node: Id) -> Option<umlcraft_model::ActivityNodeKind> {
    let element = model.view(diagram)?.get(node)?.element?;
    match model.get(element)?.kind() {
        ElementKind::ActivityNode(p) => Some(p.node),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{self, EdgeTool};
    use crate::scene::{self, ApproxMeasure, Figure, Preview};
    use umlcraft_model::{ActivityNodeKind, DiagramKind, Document};

    fn diagram() -> (Document, Id, Id) {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let diagram = doc
            .edit("d", |tx| crate::edit::create_diagram(tx, root, DiagramKind::Activity, "Order"))
            .unwrap();
        let activity = doc.model().element(diagram).unwrap().owner().unwrap();
        (doc, activity, diagram)
    }

    fn create(doc: &mut Document, diagram: Id, tool: NodeTool, at: Point) -> Result<(Id, Id)> {
        crate::edit::apply(doc, "create", &ApproxMeasure, |tx| {
            palette::create_node(tx, diagram, tool, at, "n")
        })
    }

    fn bounds(doc: &Document, diagram: Id, node: Id) -> Rect {
        doc.model().view(diagram).unwrap().get(node).unwrap().bounds().unwrap()
    }

    #[test]
    fn pins_sit_outside_their_action_and_follow_it() {
        let (mut doc, _, diagram) = diagram();
        let (action, action_node) =
            create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 100.0)).unwrap();
        assert!(
            create(&mut doc, diagram, NodeTool::InputPin, Point::new(600.0, 600.0)).is_err(),
            "a pin needs an action"
        );
        let a = bounds(&doc, diagram, action_node);
        // Dropped on the left edge, the pin goes just outside it and belongs to the action.
        let (pin, pin_node) =
            create(&mut doc, diagram, NodeTool::InputPin, Point::new(a.x - 5.0, a.y + 5.0))
                .unwrap();
        assert_eq!(doc.model().element(pin).unwrap().owner(), Some(action));
        let p = bounds(&doc, diagram, pin_node);
        assert_eq!(p.size(), umlcraft_model::Size::new(PIN_SIZE, PIN_SIZE));
        assert_eq!(p.right(), a.x, "outside the left edge");
        // An output pin dropped on the action's right half lands on its right edge.
        let (_, out_node) = create(
            &mut doc,
            diagram,
            NodeTool::OutputPin,
            Point::new(a.right() - 10.0, a.center().y - 7.5),
        )
        .unwrap();
        assert_eq!(bounds(&doc, diagram, out_node).x, a.right());

        // Moving and widening the action takes its pins along.
        let moved = Rect::new(a.x + 50.0, a.y + 30.0, a.width + 40.0, a.height);
        crate::edit::apply(&mut doc, "move", &ApproxMeasure, |tx| {
            tx.set_bounds(diagram, action_node, moved)
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, pin_node).right(), moved.x);
        assert_eq!(bounds(&doc, diagram, out_node).x, moved.right());

        // A pin dragged away comes back to its action's border, on the side nearest.
        let away = bounds(&doc, diagram, out_node).translate(0.0, -200.0);
        crate::edit::apply(&mut doc, "drag", &ApproxMeasure, |tx| {
            tx.set_bounds(diagram, out_node, away)
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, out_node).bottom(), moved.y, "on top");

        // Flows connect pins; a flow into an output pin is refused.
        let (_, object_node) =
            create(&mut doc, diagram, NodeTool::ObjectNode, Point::new(100.0, 300.0)).unwrap();
        let flow = crate::edit::apply(&mut doc, "flow", &ApproxMeasure, |tx| {
            palette::connect(tx, diagram, EdgeTool::Flow, out_node, object_node)
        });
        assert!(flow.is_ok());
        let wrong = crate::edit::apply(&mut doc, "flow", &ApproxMeasure, |tx| {
            palette::connect(tx, diagram, EdgeTool::Flow, object_node, out_node)
        });
        assert!(wrong.is_err());
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        // The connected pin shows no arrow, the other one points into the action.
        let figure = |id| &scene.node(id).unwrap().figure;
        assert!(matches!(figure(out_node), Figure::Pin { arrow: None, .. }));
        let Figure::Pin { arrow: Some([from, to]), .. } = figure(pin_node) else { panic!() };
        assert!(to.x > from.x, "into the action: {from:?} {to:?}");

        // Removing the action from the diagram takes its pins along; deleting it, the pins.
        crate::edit::apply(&mut doc, "remove", &ApproxMeasure, |tx| {
            tx.remove_presentation(diagram, action_node)
        })
        .unwrap();
        assert!(doc.model().view(diagram).unwrap().get(pin_node).is_none());
        crate::edit::apply(&mut doc, "delete", &ApproxMeasure, |tx| tx.delete(action)).unwrap();
        assert!(doc.model().get(pin).is_none());
        assert!(doc.model().check().is_ok());
    }

    #[test]
    fn activity_parameters_sit_on_the_frame() {
        let (mut doc, activity, diagram) = diagram();
        create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 100.0)).unwrap();
        let (parameter, node) =
            create(&mut doc, diagram, NodeTool::ActivityParameter, Point::new(60.0, 110.0))
                .unwrap();
        assert_eq!(doc.model().element(parameter).unwrap().owner(), Some(activity));
        let view = doc.model().view(diagram).unwrap();
        let framed = frame(view, diagram).expect("a frame was added").bounds().unwrap();
        assert_eq!(bounds(&doc, diagram, node).center().x, framed.x, "on the left border");
        // The frame grows around what is drawn; the parameter stays on its border.
        create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 700.0)).unwrap();
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        let frame_rect = scene.nodes.iter().find(|n| n.element == Some(diagram)).unwrap().rect;
        assert!(frame_rect.bottom() > 700.0);
        let drawn = scene.node(node).unwrap().rect;
        assert_eq!(drawn.center().x, frame_rect.x);
        let kind = doc.model().element(parameter).unwrap().kind();
        assert!(
            matches!(kind, ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::ActivityParameter)
        );
    }

    /// What each pin of `action` shows, as drawn: its square, its lines of text and its
    /// exception triangle.
    fn pin_shapes(doc: &Document, diagram: Id, action: Id) -> Vec<(Id, Rect, Vec<Rect>)> {
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        let borne = crate::composite::borne_nodes(doc.model(), diagram);
        let mut pins: Vec<(Id, Rect, Vec<Rect>)> = scene
            .nodes
            .iter()
            .filter(|n| borne.get(&n.id).copied().flatten() == Some(action))
            .map(|n| (n.id, n.rect, n.outside()))
            .collect();
        pins.sort_by_key(|p| p.0);
        pins
    }

    /// Where a text of one pin runs over the square or a text of another pin of `action`.
    fn clashes(doc: &Document, diagram: Id, action: Id) -> Vec<String> {
        let pins = pin_shapes(doc, diagram, action);
        let overlap = |a: &Rect, b: &Rect| {
            a.x < b.right() - 0.01
                && b.x < a.right() - 0.01
                && a.y < b.bottom() - 0.01
                && b.y < a.bottom() - 0.01
        };
        let mut found = Vec::new();
        for (i, (_, _, texts)) in pins.iter().enumerate() {
            for (j, (_, square, others)) in pins.iter().enumerate().filter(|(j, _)| *j != i) {
                for t in texts {
                    if overlap(t, square) {
                        found.push(format!("pin {i}: {t:?} on the square {square:?} of pin {j}"));
                    }
                    for o in others.iter().filter(|o| overlap(t, o)) {
                        found.push(format!("pin {i}: {t:?} on {o:?} of pin {j}"));
                    }
                }
            }
        }
        found
    }

    fn assert_texts_clear(doc: &Document, diagram: Id, action: Id) {
        let found = clashes(doc, diagram, action);
        assert!(found.is_empty(), "{found:#?}");
    }

    /// No text of a pin of `action` runs over the action's name.
    fn assert_name_clear(doc: &Document, diagram: Id, action: Id) {
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        let names: Vec<Rect> = scene
            .node(action)
            .unwrap()
            .rows
            .iter()
            .map(|row| {
                let w = ApproxMeasure.width(&row.text, row.size, row.bold);
                Rect::new(row.rect.center().x - w / 2.0, row.rect.y, w, row.rect.height)
            })
            .collect();
        for (_, _, texts) in pin_shapes(doc, diagram, action) {
            for t in &texts {
                for n in &names {
                    let apart = t.right() <= n.x + 0.01
                        || n.right() <= t.x + 0.01
                        || t.bottom() <= n.y + 0.01
                        || n.bottom() <= t.y + 0.01;
                    assert!(apart, "{t:?} on the name {n:?}");
                }
            }
        }
    }

    /// Three input pins on the left of an action and two output pins on its right, created
    /// as `create_element` with `on` creates them, with names, states and properties: their
    /// texts stay clear of each other, the action growing to hold them, and so after
    /// arranging the diagram.
    #[test]
    fn texts_of_pins_on_one_side_stay_apart() {
        use umlcraft_model::ObjectOrdering;
        let (mut doc, _, diagram) = diagram();
        let (ship, action) = create(&mut doc, diagram, NodeTool::Action, Point::new(200.0, 100.0))
            .expect("an action");
        crate::edit::apply(&mut doc, "name", &ApproxMeasure, |tx| tx.rename(ship, "Ship order"))
            .unwrap();
        let before = bounds(&doc, diagram, action);
        let pins = [
            (NodeTool::InputPin, "order", "paid", true),
            (NodeTool::InputPin, "customer with a long name", "", false),
            (NodeTool::InputPin, "voucher", "", true),
            (NodeTool::OutputPin, "invoice", "sent", true),
            (NodeTool::OutputPin, "parcel", "", false),
        ];
        let mut nodes = Vec::new();
        for (tool, name, state, more) in pins {
            let node = crate::edit::apply(&mut doc, "pin", &ApproxMeasure, |tx| {
                let at = crate::composite::free_spot(tx.model(), diagram, action, tool).unwrap();
                let (element, node) = palette::create_node(tx, diagram, tool, at, name)?;
                tx.rename(element, name)?;
                tx.update(element, |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.in_state = state.into();
                        if more {
                            p.ordering = Some(ObjectOrdering::Fifo);
                            p.upper_bound = "2".into();
                            p.is_control = true;
                            p.is_exception = true;
                        }
                    }
                })?;
                place_pin(tx, diagram, node, &ApproxMeasure)?;
                Ok(node)
            })
            .unwrap();
            nodes.push(node);
            assert_texts_clear(&doc, diagram, action);
            assert_name_clear(&doc, diagram, action);
        }
        let grown = bounds(&doc, diagram, action);
        assert!(grown.height > before.height, "{grown:?}");
        // Input pins on the left, output pins on the right, all just outside the action.
        for (&node, (tool, ..)) in nodes.iter().zip(pins) {
            let r = bounds(&doc, diagram, node);
            let x = if tool == NodeTool::InputPin { grown.x - PIN_SIZE } else { grown.right() };
            assert!((r.x - x).abs() < 0.01, "{r:?} {grown:?}");
            assert!(r.y >= grown.y - 0.01 && r.bottom() <= grown.bottom() + 0.01, "{r:?}");
        }
        doc.model().check().unwrap();

        // Arranging lays them out along their sides, still clear of each other.
        crate::edit::apply(&mut doc, "arrange", &ApproxMeasure, |tx| {
            crate::layout::arrange(
                tx,
                diagram,
                None,
                crate::layout::Direction::Down,
                &ApproxMeasure,
            )
        })
        .unwrap();
        assert_texts_clear(&doc, diagram, action);
        assert_name_clear(&doc, diagram, action);
        let arranged = bounds(&doc, diagram, action);
        let squares: Vec<Rect> = nodes.iter().map(|&n| bounds(&doc, diagram, n)).collect();
        // The lowest pin on each side stands level with the action's bottom.
        for side in [&squares[..3], &squares[3..]] {
            let lowest = side.iter().map(|r| r.bottom()).fold(f32::MIN, f32::max);
            assert!((lowest - arranged.bottom()).abs() < 0.01, "{side:?} {arranged:?}");
        }
        // Arranging again neither grows the action nor moves its pins along it.
        let relative = |doc: &Document| -> Vec<(f32, f32)> {
            let a = bounds(doc, diagram, action);
            nodes.iter().map(|&n| bounds(doc, diagram, n)).map(|r| (r.x - a.x, r.y - a.y)).collect()
        };
        let places = relative(&doc);
        crate::edit::apply(&mut doc, "arrange", &ApproxMeasure, |tx| {
            crate::layout::arrange(
                tx,
                diagram,
                None,
                crate::layout::Direction::Down,
                &ApproxMeasure,
            )
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, action).size(), arranged.size());
        let close = |a: &[(f32, f32)], b: &[(f32, f32)]| {
            a.iter().zip(b).all(|(p, q)| (p.0 - q.0).abs() < 0.01 && (p.1 - q.1).abs() < 0.01)
        };
        assert!(close(&relative(&doc), &places), "{:?} {places:?}", relative(&doc));
        doc.model().check().unwrap();
    }

    /// Arranging a diagram whose pins stand over each other's texts, as Astah lets pins
    /// placed by hand do, moves them apart; three input pins on the top and two output pins
    /// on the bottom go side by side, the action widening.
    #[test]
    fn arranging_moves_crowded_pins_apart() {
        let (mut doc, _, diagram) = diagram();
        let (_, action) = create(&mut doc, diagram, NodeTool::Action, Point::new(200.0, 100.0))
            .expect("an action");
        let a = bounds(&doc, diagram, action);
        let mut nodes = Vec::new();
        for (tool, at) in [
            (NodeTool::InputPin, Point::new(a.x - 10.0, a.y + 2.0)),
            (NodeTool::InputPin, Point::new(a.x - 10.0, a.y + 6.0)),
            (NodeTool::InputPin, Point::new(a.x + 10.0, a.y - 10.0)),
            (NodeTool::InputPin, Point::new(a.x + 16.0, a.y - 10.0)),
            (NodeTool::InputPin, Point::new(a.x + 22.0, a.y - 10.0)),
            (NodeTool::OutputPin, Point::new(a.x + 10.0, a.bottom() - 4.0)),
            (NodeTool::OutputPin, Point::new(a.x + 14.0, a.bottom() - 4.0)),
            (NodeTool::OutputPin, Point::new(a.right() - 4.0, a.y + 3.0)),
        ] {
            let (element, node) = create(&mut doc, diagram, tool, at).unwrap();
            crate::edit::apply(&mut doc, "name", &ApproxMeasure, |tx| {
                tx.rename(element, format!("item number {}", nodes.len()))
            })
            .unwrap();
            nodes.push(node);
        }
        // As placed, the pins' texts run into each other.
        assert!(!clashes(&doc, diagram, action).is_empty(), "the probe starts crowded");
        crate::edit::apply(&mut doc, "arrange", &ApproxMeasure, |tx| {
            crate::layout::arrange(
                tx,
                diagram,
                None,
                crate::layout::Direction::Down,
                &ApproxMeasure,
            )
        })
        .unwrap();
        assert_texts_clear(&doc, diagram, action);
        assert!(bounds(&doc, diagram, action).width > a.width);
        doc.model().check().unwrap();
    }

    /// [`scene::pin_cover`] covers what the scene draws for a pin on each side.
    #[test]
    fn pin_cover_matches_the_drawing() {
        let (mut doc, _, diagram) = diagram();
        let (_, action) = create(&mut doc, diagram, NodeTool::Action, Point::new(200.0, 100.0))
            .expect("an action");
        let a = bounds(&doc, diagram, action);
        let spots = [
            Point::new(a.center().x, a.y - 10.0),
            Point::new(a.right() - 4.0, a.center().y),
            Point::new(a.center().x, a.bottom() - 4.0),
            Point::new(a.x - 10.0, a.center().y),
        ];
        for (side, at) in spots.into_iter().enumerate() {
            let (element, node) = create(&mut doc, diagram, NodeTool::InputPin, at).unwrap();
            crate::edit::apply(&mut doc, "props", &ApproxMeasure, |tx| {
                tx.update(element, |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.in_state = "ready".into();
                        p.is_exception = true;
                    }
                })
            })
            .unwrap();
            let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
            let shape = scene.node(node).unwrap();
            let rows: Vec<Size> = shape.rows.iter().map(|r| r.rect.size()).collect();
            let cover = scene::pin_cover(shape.rect, side as u8, &rows, true);
            let extent = shape.extent();
            for (c, e) in [(cover.x, extent.x), (cover.y, extent.y)]
                .into_iter()
                .chain([(cover.right(), extent.right()), (cover.bottom(), extent.bottom())])
            {
                assert!((c - e).abs() < 0.01, "side {side}: {cover:?} {extent:?}");
            }
        }
    }
}
