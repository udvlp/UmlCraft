//! Arranging ER diagrams, for "Arrange" and the SQL import.
//!
//! 1. Entities, relationships and IS-As are the core graph. Each relationship goes into the
//!    layer below the entity on its "one" side (the line with `[x,1]`, look-across: the
//!    referenced table) and above the others; an IS-A below its supertype.
//! 2. Each core node reserves room for its attributes around it; the boxes go to the layered
//!    layout. A second round reserves the room on the sides that turned out to be free of
//!    lines.
//! 3. Attributes go on the sides without lines first (top, right, left, bottom): rows above
//!    and below, columns at the sides, keys first, 30 px from their owner, further out when a
//!    side is full. One that would cover a node, a line or a label moves to the next free
//!    place. The parts of a composite attribute go on beyond it.
//! 4. Lines are straight; several lines between the same two nodes get a waypoint each so
//!    they do not lie on top of each other. A line that would run through another node (one
//!    that skips a layer passes a node there) goes around it with one or two bends beside
//!    it; a self-reference goes round the first corner of its node where it and its labels
//!    keep clear of the others.
//! 5. Nodes take the size their name needs and lie on the 10 px grid; the whole starts at
//!    (20, 20). Notes and free texts stay where they are.

use std::collections::HashMap;

use umlcraft_model::{
    CardinalityMax, ElementKind, Id, Model, ModelError, Point, Rect, Result, Shape, Size, Tx,
};

use crate::layout::{Direction, Edge, layered};
use crate::scene::{self, Preview, Scene, TextMeasure};

const GRID: f32 = 10.0;
/// Between a node and its nearest attributes.
const GAP: f32 = 30.0;
/// Between attributes in a row.
const ROW_GAP: f32 = 10.0;
/// From one attribute to the next in a column (a whole grid step more than an attribute's
/// height, so that two neighbours stay apart after snapping).
const COLUMN_PITCH: f32 = 40.0;
/// Between the columns of two rings.
const COLUMN_GAP: f32 = 20.0;
/// From one ring of rows to the next.
const RING_STEP: f32 = 40.0;
/// Attributes per row (top, bottom) and per column (sides) in one ring.
const ROW_CAPACITY: usize = 4;
const COLUMN_CAPACITY: usize = 3;
/// Between lines that join the same two nodes.
const PARALLEL_SPREAD: f32 = 60.0;
/// Free room kept around what is placed.
const CLEARANCE: f32 = 6.0;
/// Between a node and a line led around it.
const DETOUR_GAP: f32 = 20.0;
/// The top left corner of the arranged diagram.
const MARGIN: f32 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    /// The order attributes take the sides in: top, then the sides, the bottom last.
    const ORDER: [Self; 4] = [Self::Top, Self::Right, Self::Left, Self::Bottom];

    const fn index(self) -> usize {
        self as usize
    }

    const fn capacity(self) -> usize {
        match self {
            Self::Top | Self::Bottom => ROW_CAPACITY,
            Self::Left | Self::Right => COLUMN_CAPACITY,
        }
    }

    /// The direction away from the node.
    const fn outward(self) -> (f32, f32) {
        match self {
            Self::Top => (0.0, -1.0),
            Self::Right => (1.0, 0.0),
            Self::Bottom => (0.0, 1.0),
            Self::Left => (-1.0, 0.0),
        }
    }

    /// The side of a `w` × `h` box that the direction `(dx, dy)` leaves it through.
    fn of(dx: f32, dy: f32, w: f32, h: f32) -> Self {
        if dx.abs() * h >= dy.abs() * w {
            if dx > 0.0 { Self::Right } else { Self::Left }
        } else if dy > 0.0 {
            Self::Bottom
        } else {
            Self::Top
        }
    }
}

/// Which sides of each core node lines leave through.
type Used = [bool; 4];

/// A node of the diagram that the layout places.
struct Node {
    id: Id,
    bounds: Rect,
    attribute: bool,
    key: bool,
}

/// A line between two placed nodes.
struct Line {
    id: Id,
    ends: [usize; 2],
    /// Between two core nodes (not an attribute's line).
    core: bool,
    waypoints: Vec<Point>,
}

/// The diagram as the layout sees it.
struct Graph {
    nodes: Vec<Node>,
    lines: Vec<Line>,
    /// Node indices of entities, relationships, IS-As (and attributes without a shown owner).
    core: Vec<usize>,
    /// Core position of a node index.
    position: HashMap<usize, usize>,
    /// Per core node the node indices of its attributes, keys first.
    attributes: Vec<Vec<usize>>,
    /// Parts of composite attributes with their parent, parents before their parts.
    parts: Vec<(usize, usize)>,
    edges: Vec<Edge>,
}

/// Arranges the whole ER diagram `diagram`. Returns the nodes that moved or changed size.
pub fn arrange(tx: &mut Tx<'_>, diagram: Id, measure: &dyn TextMeasure) -> Result<Vec<Id>> {
    let scene = scene::build(tx.model(), diagram, &Preview::default(), measure);
    let Some(mut g) = Graph::of(tx.model(), diagram, &scene)? else { return Ok(Vec::new()) };
    let before: Vec<Rect> = g.nodes.iter().map(|n| n.bounds).collect();
    let first = place(&g, &vec![[false; 4]; g.core.len()]);
    apply(&mut g, &first);
    let used = used_sides(&g);
    let second = place(&g, &used);
    apply(&mut g, &second);
    detour(&mut g);
    // Where the lines run and their labels lie follow from the scene of the placed core.
    write(tx, diagram, &g)?;
    place_loops(tx, diagram, &mut g, measure)?;
    let core = scene::build(tx.model(), diagram, &Preview::default(), measure);
    // Labels give way to the attributes where they can; where one moved onto an attribute,
    // the attributes are placed again, clear of both its places.
    let mut labels = label_rects(&g, &core, measure);
    for _ in 0..3 {
        place_attributes(&mut g, &core, &labels);
        write(tx, diagram, &g)?;
        let scene = scene::build(tx.model(), diagram, &Preview::default(), measure);
        let moved = label_rects(&g, &scene, measure);
        let mut attributes = g.nodes.iter().filter(|n| n.attribute).map(|n| n.bounds);
        if !attributes.any(|a| moved.iter().any(|l| overlap(&a, l))) {
            break;
        }
        labels.extend(moved);
    }
    normalize(&mut g);
    write(tx, diagram, &g)?;
    Ok(g.nodes.iter().zip(before).filter(|(n, b)| n.bounds != *b).map(|(n, _)| n.id).collect())
}

/// Places the attributes of `diagram` around their owners, which stay where they are: for a
/// diagram turned from IE or IDEF1X back into Chen.
pub fn arrange_attributes(tx: &mut Tx<'_>, diagram: Id, measure: &dyn TextMeasure) -> Result<()> {
    let scene = scene::build(tx.model(), diagram, &Preview::default(), measure);
    let Some(mut g) = Graph::of(tx.model(), diagram, &scene)? else { return Ok(()) };
    if g.attributes.iter().all(Vec::is_empty) && g.parts.is_empty() {
        return Ok(());
    }
    let mut labels = label_rects(&g, &scene, measure);
    for _ in 0..3 {
        place_attributes(&mut g, &scene, &labels);
        write(tx, diagram, &g)?;
        let scene = scene::build(tx.model(), diagram, &Preview::default(), measure);
        let moved = label_rects(&g, &scene, measure);
        let mut attributes = g.nodes.iter().filter(|n| n.attribute).map(|n| n.bounds);
        if !attributes.any(|a| moved.iter().any(|l| overlap(&a, l))) {
            break;
        }
        labels.extend(moved);
    }
    Ok(())
}

/// Writes the places of the nodes and the routes of the lines.
fn write(tx: &mut Tx<'_>, diagram: Id, g: &Graph) -> Result<()> {
    for n in &g.nodes {
        tx.set_bounds(diagram, n.id, n.bounds)?;
    }
    for l in &g.lines {
        tx.set_route(diagram, l.id, l.waypoints.clone(), (None, None))?;
    }
    Ok(())
}

fn is_hub_line(kind: &ElementKind) -> bool {
    matches!(kind, ElementKind::ErParticipation(_) | ElementKind::ErSubtyping(_))
}

impl Graph {
    /// `None` when the diagram shows nothing of an ER diagram.
    fn of(model: &Model, diagram: Id, scene: &Scene) -> Result<Option<Self>> {
        let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let mut nodes = Vec::new();
        let mut node_of: HashMap<Id, usize> = HashMap::new();
        // The node that shows an element, for its attributes.
        let mut shown: HashMap<Id, usize> = HashMap::new();
        for p in view.presentations() {
            let Shape::Node { bounds, .. } = p.shape else { continue };
            let Some(element) = p.element.and_then(|e| model.get(e)) else { continue };
            if !crate::er::is_er_kind(element.kind()) {
                continue;
            }
            // As large as it is drawn: a long name makes the box grow.
            let drawn = scene.node(p.id).map_or(bounds, |n| n.rect);
            let bounds = Rect::new(bounds.x, bounds.y, drawn.width, drawn.height);
            let (attribute, key) = match element.kind() {
                ElementKind::ErAttribute(a) => (true, a.key.is_some()),
                _ => (false, false),
            };
            node_of.insert(p.id, nodes.len());
            shown.entry(element.id()).or_insert(nodes.len());
            nodes.push(Node { id: p.id, bounds, attribute, key });
        }
        if nodes.is_empty() {
            return Ok(None);
        }
        let element_of = |i: usize| view.get(nodes[i].id).and_then(|p| p.element);
        // Attributes go with the shown node of their owner.
        let mut core = Vec::new();
        let mut roots: Vec<(usize, usize)> = Vec::new();
        let mut parts = Vec::new();
        for i in 0..nodes.len() {
            if !nodes[i].attribute {
                core.push(i);
                continue;
            }
            let owner = element_of(i)
                .and_then(|e| model.get(e)?.owner())
                .and_then(|o| shown.get(&o).copied());
            match owner {
                Some(o) if nodes[o].attribute => parts.push((i, o)),
                Some(o) => roots.push((i, o)),
                None => core.push(i),
            }
        }
        let position: HashMap<usize, usize> =
            core.iter().enumerate().map(|(p, &i)| (i, p)).collect();
        let mut attributes = vec![Vec::new(); core.len()];
        for &(a, o) in &roots {
            attributes[position[&o]].push(a);
        }
        for list in &mut attributes {
            let (keys, rest): (Vec<usize>, Vec<usize>) = list.iter().partition(|&&i| nodes[i].key);
            *list = keys.into_iter().chain(rest).collect();
        }
        // Parents first: a part whose parent is a part waits for it.
        let mut ordered: Vec<(usize, usize)> = Vec::new();
        let mut placed: Vec<usize> = roots.iter().map(|r| r.0).collect();
        let mut rest = parts;
        while !rest.is_empty() {
            let (ready, wait): (Vec<_>, Vec<_>) =
                rest.into_iter().partition(|(_, p)| placed.contains(p));
            if ready.is_empty() {
                break;
            }
            placed.extend(ready.iter().map(|r| r.0));
            ordered.extend(ready);
            rest = wait;
        }
        // The lines, and for the core graph each hub's ends: (hub, other, upper).
        let mut lines = Vec::new();
        let mut ends: Vec<(usize, usize, bool)> = Vec::new();
        let mut direct_lines: Vec<(usize, usize)> = Vec::new();
        for p in view.presentations() {
            let Shape::Edge { source, target, .. } = p.shape else { continue };
            let (Some(&s), Some(&t)) = (node_of.get(&source), node_of.get(&target)) else {
                continue;
            };
            let element = p.element.and_then(|e| model.get(e));
            // In IE and IDEF1X a relationship between two entities is a line, its parent
            // (the source) above the child.
            let direct =
                element.is_some_and(|e| matches!(e.kind(), ElementKind::ErRelationship(_)));
            let core_line = direct || element.is_some_and(|e| is_hub_line(e.kind()));
            lines.push(Line { id: p.id, ends: [s, t], core: core_line, waypoints: Vec::new() });
            if direct && s != t {
                direct_lines.push((s, t));
            }
            let hub_of = |hub: Id| if element_of(s) == Some(hub) { (s, t) } else { (t, s) };
            match element.map(|e| e.kind()) {
                Some(ElementKind::ErParticipation(p)) => {
                    let (hub, other) = hub_of(p.relationship);
                    let one = p.cardinality.is_some_and(|c| c.max == CardinalityMax::Count(1));
                    ends.push((hub, other, one));
                }
                Some(ElementKind::ErSubtyping(p)) => {
                    let (hub, other) = hub_of(p.is_a);
                    ends.push((hub, other, p.supertype));
                }
                _ => {}
            }
        }
        let mut edges = core_edges(&ends, &position, model, &nodes, view);
        for (s, t) in direct_lines {
            if let (Some(&upper), Some(&lower)) = (position.get(&s), position.get(&t)) {
                edges.push(Edge { upper, lower, hierarchy: false });
            }
        }
        Ok(Some(Self { nodes, lines, core, position, attributes, parts: ordered, edges }))
    }
}

/// Edges for the layered layout: the "one" side above a relationship, the others below; the
/// supertype above an IS-A, its subtypes below.
fn core_edges(
    ends: &[(usize, usize, bool)],
    position: &HashMap<usize, usize>,
    model: &Model,
    nodes: &[Node],
    view: &umlcraft_model::View,
) -> Vec<Edge> {
    let is_a = |i: usize| {
        let element = view.get(nodes[i].id).and_then(|p| p.element);
        matches!(element.and_then(|e| model.get(e)).map(|e| e.kind()), Some(ElementKind::ErIsA(_)))
    };
    let mut hubs: Vec<usize> = Vec::new();
    for &(hub, ..) in ends {
        if !hubs.contains(&hub) {
            hubs.push(hub);
        }
    }
    let mut edges = Vec::new();
    for hub in hubs {
        let Some(&h) = position.get(&hub) else { continue };
        let mine: Vec<(usize, bool)> = ends
            .iter()
            .filter(|&&(x, other, _)| x == hub && other != hub)
            .filter_map(|&(_, other, upper)| Some((*position.get(&other)?, upper)))
            .collect();
        let hierarchy = is_a(hub);
        let upper = mine.iter().find(|e| e.1).or(if hierarchy { None } else { mine.first() });
        let upper = upper.map(|e| e.0);
        if let Some(u) = upper {
            edges.push(Edge { upper: u, lower: h, hierarchy });
        }
        let mut seen = Vec::new();
        for &(p, _) in &mine {
            if Some(p) == upper || seen.contains(&p) {
                continue;
            }
            seen.push(p);
            edges.push(Edge { upper: h, lower: p, hierarchy });
        }
    }
    edges
}

fn sizes_of(g: &Graph, list: &[usize]) -> Vec<(f32, f32)> {
    list.iter().map(|&a| (g.nodes[a].bounds.width, g.nodes[a].bounds.height)).collect()
}

/// The top left corners of the core nodes, on the grid.
fn place(g: &Graph, used: &[Used]) -> Vec<Point> {
    let mut sizes = Vec::with_capacity(g.core.len());
    let mut offsets = Vec::with_capacity(g.core.len());
    for (c, &i) in g.core.iter().enumerate() {
        let b = g.nodes[i].bounds;
        let attrs = sizes_of(g, &g.attributes[c]);
        let slots = slots(b.width, b.height, &attrs, used[c]);
        // The box of the node and its attribute slots, around the node's center.
        let (mut x0, mut y0, mut x1, mut y1) =
            (-b.width / 2.0, -b.height / 2.0, b.width / 2.0, b.height / 2.0);
        for (s, &(w, h)) in slots.iter().zip(&attrs) {
            x0 = x0.min(s.center.x - w / 2.0);
            x1 = x1.max(s.center.x + w / 2.0);
            y0 = y0.min(s.center.y - h / 2.0);
            y1 = y1.max(s.center.y + h / 2.0);
        }
        let slack = if attrs.is_empty() { 0.0 } else { GRID };
        sizes.push(Size::new(x1 - x0 + 2.0 * slack, y1 - y0 + 2.0 * slack));
        offsets.push(Point::new(-x0 + slack - b.width / 2.0, -y0 + slack - b.height / 2.0));
    }
    let corners = layered(&sizes, &g.edges, Direction::Down);
    corners
        .iter()
        .zip(&offsets)
        .map(|(p, o)| Point::new(snap(p.x + o.x), snap(p.y + o.y)))
        .collect()
}

/// Moves the core nodes; lines that join the same two nodes are spread apart.
fn apply(g: &mut Graph, corners: &[Point]) {
    for (&i, p) in g.core.iter().zip(corners) {
        let b = &mut g.nodes[i].bounds;
        b.x = p.x;
        b.y = p.y;
    }
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut groups: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (k, l) in g.lines.iter_mut().enumerate() {
        l.waypoints.clear();
        let [a, b] = l.ends;
        let pair = (a.min(b), a.max(b));
        let list = groups.entry(pair).or_default();
        if list.is_empty() {
            order.push(pair);
        }
        list.push(k);
    }
    for pair in order {
        let lines = &groups[&pair];
        if lines.len() < 2 {
            continue;
        }
        let (ca, cb) = (g.nodes[pair.0].bounds.center(), g.nodes[pair.1].bounds.center());
        let (dx, dy) = (cb.x - ca.x, cb.y - ca.y);
        let len = dx.hypot(dy);
        let (nx, ny) = if len > 0.0 { (-dy / len, dx / len) } else { (1.0, 0.0) };
        // A line from a node to itself gets its bend beside the node.
        let reach = if len > 0.0 { 0.0 } else { g.nodes[pair.0].bounds.width / 2.0 + GAP };
        let mid = Point::new((ca.x + cb.x) / 2.0 + reach, (ca.y + cb.y) / 2.0);
        let m = lines.len() as f32;
        for (k, &l) in lines.iter().enumerate() {
            let off = (k as f32 - (m - 1.0) / 2.0) * PARALLEL_SPREAD;
            if off != 0.0 {
                g.lines[l].waypoints = vec![Point::new(mid.x + nx * off, mid.y + ny * off)];
            }
        }
    }
}

/// Lines between core nodes that would run through another core node go around it: with one
/// bend beside it or two along its side, whichever is shortest and clear of the other core
/// nodes. A line that skips a layer of the layered layout passes a node there.
fn detour(g: &mut Graph) {
    let boxes: Vec<(usize, Rect)> = g.core.iter().map(|&i| (i, g.nodes[i].bounds)).collect();
    for l in g.lines.iter_mut().filter(|l| l.core && l.ends[0] != l.ends[1]) {
        let [s, t] = l.ends;
        let others: Vec<Rect> =
            boxes.iter().filter(|(i, _)| *i != s && *i != t).map(|(_, r)| *r).collect();
        let (from, to) = (g.nodes[s].bounds.center(), g.nodes[t].bounds.center());
        let path_of = |waypoints: &[Point]| -> Vec<Point> {
            std::iter::once(from).chain(waypoints.iter().copied()).chain([to]).collect()
        };
        // Whether a segment runs through one of the other nodes, kept `by` away from them.
        let blocked = |a: Point, b: Point, by: f32| -> Option<Rect> {
            others.iter().find(|r| segment_hits(a, b, &inflate(r, by))).copied()
        };
        // A bend that spreads parallel lines and fell into another node moves out of it,
        // sideways: to the side of the node's middle it lies on.
        for p in &mut l.waypoints {
            if let Some(r) = others.iter().find(|r| inside(*p, &inflate(r, DETOUR_GAP / 2.0))) {
                let vertical = (to.y - from.y).abs() >= (to.x - from.x).abs();
                let c = r.center();
                if vertical {
                    p.x = if p.x < c.x { r.x - DETOUR_GAP } else { r.x + r.width + DETOUR_GAP };
                    p.x = snap(p.x);
                } else {
                    p.y = if p.y < c.y { r.y - DETOUR_GAP } else { r.y + r.height + DETOUR_GAP };
                    p.y = snap(p.y);
                }
            }
        }
        // Several nodes in a row are passed one after the other.
        for _ in 0..6 {
            let path = path_of(&l.waypoints);
            let Some((k, r)) = path
                .windows(2)
                .enumerate()
                .find_map(|(k, w)| blocked(w[0], w[1], 0.0).map(|r| (k, r)))
            else {
                break;
            };
            let (a, b) = (path[k], path[k + 1]);
            let (left, right) = (snap(r.x - DETOUR_GAP), snap(r.x + r.width + DETOUR_GAP));
            let (top, bottom) = (snap(r.y - DETOUR_GAP), snap(r.y + r.height + DETOUR_GAP));
            let (mid_x, mid_y) = (snap(r.x + r.width / 2.0), snap(r.y + r.height / 2.0));
            // Beside the node: one bend at its middle, or two along the whole side.
            let candidates: Vec<Vec<Point>> = if (b.y - a.y).abs() >= (b.x - a.x).abs() {
                let (first, last) = if b.y > a.y { (top, bottom) } else { (bottom, top) };
                [left, right]
                    .into_iter()
                    .flat_map(|x| {
                        [
                            vec![Point::new(x, mid_y)],
                            vec![Point::new(x, first), Point::new(x, last)],
                        ]
                    })
                    .collect()
            } else {
                let (first, last) = if b.x > a.x { (left, right) } else { (right, left) };
                [top, bottom]
                    .into_iter()
                    .flat_map(|y| {
                        [
                            vec![Point::new(mid_x, y)],
                            vec![Point::new(first, y), Point::new(last, y)],
                        ]
                    })
                    .collect()
            };
            let length = |p: &[Point]| -> f32 {
                p.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).sum()
            };
            // The new stretch from `a` through the bends to `b` keeps clear of `r`; of those,
            // the one passing the fewest other nodes (the next round leads it around them),
            // then the shortest.
            let best = candidates
                .into_iter()
                .filter_map(|bends| {
                    let stretch: Vec<Point> =
                        std::iter::once(a).chain(bends.iter().copied()).chain([b]).collect();
                    let near = |w: &[Point], q: &Rect| segment_hits(w[0], w[1], q);
                    if stretch.windows(2).any(|w| near(w, &inflate(&r, DETOUR_GAP / 2.0))) {
                        return None;
                    }
                    let passed = stretch
                        .windows(2)
                        .filter(|w| blocked(w[0], w[1], DETOUR_GAP / 2.0).is_some())
                        .count();
                    let mut waypoints = l.waypoints.clone();
                    waypoints.splice(k..k, bends);
                    Some((passed, length(&path_of(&waypoints)), waypoints))
                })
                .min_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
            match best {
                Some((_, _, waypoints)) => l.waypoints = waypoints,
                None => break,
            }
        }
    }
}

/// A line from a node to itself (a self-reference in IE and IDEF1X) goes round the first
/// corner of its node, top right, top left, bottom right, bottom left, where it and its labels
/// keep clear of the other core nodes; top right, as drawn by default, if none is.
fn place_loops(
    tx: &mut Tx<'_>,
    diagram: Id,
    g: &mut Graph,
    measure: &dyn TextMeasure,
) -> Result<()> {
    let loops: Vec<usize> = (0..g.lines.len())
        .filter(|&k| {
            let l = &g.lines[k];
            l.core && l.ends[0] == l.ends[1] && l.waypoints.is_empty()
        })
        .collect();
    for k in loops {
        let node = g.lines[k].ends[0];
        let r = g.nodes[node].bounds;
        let others: Vec<Rect> = g
            .core
            .iter()
            .filter(|&&i| i != node)
            .map(|&i| inflate(&g.nodes[i].bounds, CLEARANCE))
            .collect();
        let (left, right, top, bottom) = (r.x, r.x + r.width, r.y, r.y + r.height);
        let corners = [
            [(right + 30.0, top + 20.0), (right + 30.0, top - 30.0), (right - 20.0, top - 30.0)],
            [(left - 30.0, top + 20.0), (left - 30.0, top - 30.0), (left + 20.0, top - 30.0)],
            [
                (right + 30.0, bottom - 20.0),
                (right + 30.0, bottom + 30.0),
                (right - 20.0, bottom + 30.0),
            ],
            [
                (left - 30.0, bottom - 20.0),
                (left - 30.0, bottom + 30.0),
                (left + 20.0, bottom + 30.0),
            ],
        ];
        let id = g.lines[k].id;
        let mut chosen = Vec::new();
        // The first corner is the one drawn without waypoints.
        for (c, corner) in corners.into_iter().enumerate() {
            let waypoints: Vec<Point> = if c == 0 {
                Vec::new()
            } else {
                corner.iter().map(|&(x, y)| Point::new(x, y)).collect()
            };
            tx.set_route(diagram, id, waypoints.clone(), (None, None))?;
            let scene = scene::build(tx.model(), diagram, &Preview::default(), measure);
            let Some(edge) = scene.edges.iter().find(|e| e.id == id) else { break };
            let mut rects: Vec<Rect> =
                edge.labels.iter().map(|l| crate::labels::label_rect(l, measure)).collect();
            let xs = edge.points.iter().map(|p| p.x);
            let ys = edge.points.iter().map(|p| p.y);
            let (x0, x1) = (xs.clone().fold(f32::INFINITY, f32::min), xs.fold(f32::MIN, f32::max));
            let (y0, y1) = (ys.clone().fold(f32::INFINITY, f32::min), ys.fold(f32::MIN, f32::max));
            rects.push(Rect::new(x0, y0, x1 - x0, y1 - y0));
            if rects.iter().all(|a| others.iter().all(|o| !overlap(a, o))) {
                chosen = waypoints;
                break;
            }
        }
        tx.set_route(diagram, id, chosen.clone(), (None, None))?;
        g.lines[k].waypoints = chosen;
    }
    Ok(())
}

/// Which sides of each core node lines to other core nodes leave through.
fn used_sides(g: &Graph) -> Vec<Used> {
    let mut used = vec![[false; 4]; g.core.len()];
    for l in g.lines.iter().filter(|l| l.core) {
        let [si, ti] = l.ends;
        let ends = [(si, ti, l.waypoints.first()), (ti, si, l.waypoints.last())];
        for (me, other, toward) in ends {
            let Some(&c) = g.position.get(&me) else { continue };
            let b = g.nodes[me].bounds;
            let from = b.center();
            let to = toward.copied().unwrap_or_else(|| g.nodes[other].bounds.center());
            if from == to {
                continue;
            }
            let side = Side::of(to.x - from.x, to.y - from.y, b.width, b.height);
            used[c][side.index()] = true;
        }
    }
    used
}

/// A place for an attribute next to its owner.
struct Slot {
    side: Side,
    /// Relative to the owner's center.
    center: Point,
}

/// Places for attributes of the given sizes around a `w` × `h` node: the free sides ring by
/// ring, then the sides with lines.
fn slots(w: f32, h: f32, attrs: &[(f32, f32)], used: Used) -> Vec<Slot> {
    // Which (side, ring) each attribute goes to.
    let mut plan: Vec<(Side, usize)> = Vec::with_capacity(attrs.len());
    let free = Side::ORDER.into_iter().filter(|s| !used[s.index()]);
    let busy = Side::ORDER.into_iter().filter(|s| used[s.index()]);
    // Ring by ring: the free sides, then those with lines (attributes there slide past the
    // lines when they are placed).
    let sides: Vec<Side> = free.chain(busy).collect();
    'fill: for ring in 0.. {
        for &side in &sides {
            for _ in 0..side.capacity() {
                if plan.len() == attrs.len() {
                    break 'fill;
                }
                plan.push((side, ring));
            }
        }
    }
    let mut out: Vec<Slot> =
        plan.iter().map(|&(side, _)| Slot { side, center: Point::new(0.0, 0.0) }).collect();
    let rings = plan.iter().map(|p| p.1).max().map_or(0, |r| r + 1);
    // Rows keep clear of the first ring's columns, which may be taller than the node.
    let column = |side: Side| {
        let members: Vec<usize> = (0..plan.len()).filter(|&k| plan[k] == (side, 0)).collect();
        let tallest = members.iter().map(|&k| attrs[k].1).fold(0.0, f32::max);
        match members.len() {
            0 => 0.0,
            n => ((n - 1) as f32 * COLUMN_PITCH + tallest) / 2.0 + ROW_GAP,
        }
    };
    let row_base = (h / 2.0 + GAP).max(column(Side::Left)).max(column(Side::Right));
    for side in Side::ORDER {
        // Inner edge of each ring's column on the sides.
        let mut inner = w / 2.0 + GAP;
        for ring in 0..rings {
            let members: Vec<usize> =
                (0..plan.len()).filter(|&k| plan[k] == (side, ring)).collect();
            if members.is_empty() {
                continue;
            }
            match side {
                Side::Top | Side::Bottom => {
                    let total: f32 = members.iter().map(|&k| attrs[k].0).sum::<f32>()
                        + ROW_GAP * (members.len() as f32 - 1.0);
                    let mut x = -total / 2.0;
                    for &k in &members {
                        let (aw, ah) = attrs[k];
                        let y = row_base + ah / 2.0 + ring as f32 * RING_STEP;
                        out[k].center =
                            Point::new(x + aw / 2.0, if side == Side::Top { -y } else { y });
                        x += aw + ROW_GAP;
                    }
                }
                Side::Left | Side::Right => {
                    let widest = members.iter().map(|&k| attrs[k].0).fold(0.0, f32::max);
                    let m = members.len() as f32;
                    for (n, &k) in members.iter().enumerate() {
                        let x = inner + attrs[k].0 / 2.0;
                        let y = (n as f32 - (m - 1.0) / 2.0) * COLUMN_PITCH;
                        out[k].center = Point::new(if side == Side::Right { x } else { -x }, y);
                    }
                    inner += widest + COLUMN_GAP;
                }
            }
        }
    }
    out
}

/// What attributes keep clear of: nodes, lines and labels.
struct Obstacles {
    /// Boxes, with the core node whose attribute they are (`None`: nodes, labels).
    rects: Vec<(Rect, Option<usize>)>,
    /// Line segments, with the core node whose attributes may touch them (their own lines).
    segments: Vec<(Point, Point, Option<usize>)>,
}

impl Obstacles {
    /// Whether `r` is free for an attribute of `owner`. `strict` also keeps it off the lines
    /// of its siblings and its own line off them; without, only foreign things count.
    fn free(&self, r: &Rect, owner: Option<usize>, owner_box: &Rect, strict: bool) -> bool {
        let foreign = |of: &Option<usize>| strict || of.is_none() || *of != owner;
        let wide = inflate(r, CLEARANCE);
        if self.rects.iter().any(|(o, _)| overlap(&wide, o)) {
            return false;
        }
        let near = inflate(r, CLEARANCE / 2.0);
        if self.segments.iter().any(|(a, b, o)| foreign(o) && segment_hits(*a, *b, &near)) {
            return false;
        }
        // Its own line must not run through another node.
        let (from, to) = (r.center(), owner_box.center());
        !self.rects.iter().any(|(o, of)| {
            o != owner_box && foreign(of) && segment_hits(from, to, &inflate(o, -1.0))
        })
    }
}

/// Where the labels of the lines between core nodes are drawn.
fn label_rects(g: &Graph, scene: &Scene, measure: &dyn TextMeasure) -> Vec<Rect> {
    let edges = g.lines.iter().filter(|l| l.core).filter_map(|l| scene.edge(l.id));
    edges.flat_map(|e| &e.labels).map(|l| crate::labels::label_rect(l, measure)).collect()
}

/// Places the attributes around their owners, clear of the core nodes, the lines between
/// them (as `scene` draws them) and `labels`.
fn place_attributes(g: &mut Graph, scene: &Scene, labels: &[Rect]) {
    let mut obstacles = Obstacles { rects: Vec::new(), segments: Vec::new() };
    for &i in &g.core {
        obstacles.rects.push((g.nodes[i].bounds, None));
    }
    obstacles.rects.extend(labels.iter().map(|&r| (r, None)));
    for l in g.lines.iter().filter(|l| l.core) {
        let Some(edge) = scene.edge(l.id) else { continue };
        for w in edge.points.windows(2) {
            obstacles.segments.push((w[0], w[1], None));
        }
    }
    let used = used_sides(g);
    for (c, &sides) in used.iter().enumerate() {
        let owner = g.nodes[g.core[c]].bounds;
        let attrs = sizes_of(g, &g.attributes[c]);
        let slots = slots(owner.width, owner.height, &attrs, sides);
        for k in 0..attrs.len() {
            let center = owner.center();
            let wanted = Point::new(center.x + slots[k].center.x, center.y + slots[k].center.y);
            let outward = slots[k].side.outward();
            let r = find_place(&obstacles, Some(c), &owner, wanted, outward, attrs[k]);
            g.nodes[g.attributes[c][k]].bounds = r;
            obstacles.rects.push((r, Some(c)));
            obstacles.segments.push((r.center(), owner.center(), Some(c)));
        }
    }
    for k in 0..g.parts.len() {
        let (part, parent) = g.parts[k];
        let p = g.nodes[parent].bounds;
        let root_owner = root_center(g, parent);
        let (dx, dy) = (p.center().x - root_owner.x, p.center().y - root_owner.y);
        let len = dx.hypot(dy);
        let dir = if len > 0.0 { (dx / len, dy / len) } else { (0.0, -1.0) };
        let (aw, ah) = (g.nodes[part].bounds.width, g.nodes[part].bounds.height);
        let reach = (p.width + aw) / 2.0 * dir.0.abs() + (p.height + ah) / 2.0 * dir.1.abs() + GAP;
        let wanted = Point::new(p.center().x + dir.0 * reach, p.center().y + dir.1 * reach);
        let at = find_place(&obstacles, None, &p, wanted, dir, (aw, ah));
        g.nodes[part].bounds = at;
        obstacles.rects.push((at, None));
        obstacles.segments.push((at.center(), p.center(), None));
    }
}

/// The center of the core node a (possibly nested) attribute belongs to.
fn root_center(g: &Graph, mut attribute: usize) -> Point {
    for _ in 0..16 {
        if let Some(c) = g.attributes.iter().position(|list| list.contains(&attribute)) {
            return g.nodes[g.core[c]].bounds.center();
        }
        match g.parts.iter().find(|(p, _)| *p == attribute) {
            Some(&(_, parent)) => attribute = parent,
            None => break,
        }
    }
    g.nodes[attribute].bounds.center()
}

/// The box for an attribute of `size` centered at `wanted`, on the grid, or the nearest free
/// one: further out in `outward`, then around the owner.
fn find_place(
    obstacles: &Obstacles,
    owner: Option<usize>,
    owner_box: &Rect,
    wanted: Point,
    outward: (f32, f32),
    (w, h): (f32, f32),
) -> Rect {
    let at = |c: Point| Rect::new(snap(c.x - w / 2.0), snap(c.y - h / 2.0), w, h);
    let c = owner_box.center();
    let (rx, ry) = (owner_box.width / 2.0 + GAP + w / 2.0, owner_box.height / 2.0 + GAP + h / 2.0);
    // Along the side: rows slide sideways, columns up and down.
    let along = (outward.1.abs(), outward.0.abs());
    let slides =
        (0..=8).map(|k: i32| ((k + 1) / 2) as f32 * GRID * if k % 2 == 0 { 1.0 } else { -1.0 });
    // Around the owner, starting in the direction of the wanted place.
    let start = (wanted.y - c.y).atan2(wanted.x - c.x);
    let angles: Vec<f32> = (0..24)
        .map(|k: i32| {
            start + ((k + 1) / 2) as f32 * 15f32.to_radians() * if k % 2 == 0 { 1.0 } else { -1.0 }
        })
        .collect();
    // The slot (sliding a little), further out, then around the owner: first clear of
    // everything, then at least clear of what is not the owner's.
    for (strict, rings) in [(true, 12), (false, 30)] {
        for step in 0..=3 {
            let s = step as f32 * RING_STEP;
            for t in slides.clone() {
                let p = Point::new(
                    wanted.x + outward.0 * s + along.0 * t,
                    wanted.y + outward.1 * s + along.1 * t,
                );
                let r = at(p);
                if obstacles.free(&r, owner, owner_box, strict) {
                    return r;
                }
            }
        }
        for ring in 0..rings {
            let grow = ring as f32 * 20.0;
            for &angle in &angles {
                let p =
                    Point::new(c.x + angle.cos() * (rx + grow), c.y + angle.sin() * (ry + grow));
                let r = at(p);
                if obstacles.free(&r, owner, owner_box, strict) {
                    return r;
                }
            }
        }
    }
    at(wanted)
}

/// Moves everything so that the nodes start at (`MARGIN`, `MARGIN`).
fn normalize(g: &mut Graph) {
    let x0 = g.nodes.iter().map(|n| n.bounds.x).fold(f32::INFINITY, f32::min);
    let y0 = g.nodes.iter().map(|n| n.bounds.y).fold(f32::INFINITY, f32::min);
    let (dx, dy) = (snap(MARGIN - x0), snap(MARGIN - y0));
    for n in &mut g.nodes {
        n.bounds.x += dx;
        n.bounds.y += dy;
    }
    for l in &mut g.lines {
        for p in &mut l.waypoints {
            *p = p.offset(dx, dy);
        }
    }
}

fn snap(v: f32) -> f32 {
    (v / GRID).round() * GRID
}

fn inflate(r: &Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.width + 2.0 * by, r.height + 2.0 * by)
}

/// Whether two boxes share more than a touching edge.
fn overlap(a: &Rect, b: &Rect) -> bool {
    const EPS: f32 = 0.5;
    a.x < b.x + b.width - EPS
        && b.x < a.x + a.width - EPS
        && a.y < b.y + b.height - EPS
        && b.y < a.y + a.height - EPS
}

fn inside(p: Point, r: &Rect) -> bool {
    p.x > r.x && p.x < r.x + r.width && p.y > r.y && p.y < r.y + r.height
}

/// Whether the segment `a`–`b` touches the inside of `r`.
fn segment_hits(a: Point, b: Point, r: &Rect) -> bool {
    if r.width <= 0.0 || r.height <= 0.0 {
        return false;
    }
    if inside(a, r) || inside(b, r) {
        return true;
    }
    let corners = [
        Point::new(r.x, r.y),
        Point::new(r.x + r.width, r.y),
        Point::new(r.x + r.width, r.y + r.height),
        Point::new(r.x, r.y + r.height),
    ];
    (0..4).any(|k| crosses(a, b, corners[k], corners[(k + 1) % 4]))
}

/// Whether the segments `a`–`b` and `c`–`d` cross (touching ends do not count).
fn crosses(a: Point, b: Point, c: Point, d: Point) -> bool {
    let cross =
        |o: Point, p: Point, q: Point| (p.x - o.x) * (q.y - o.y) - (p.y - o.y) * (q.x - o.x);
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
}

/// What covers what in an ER diagram as drawn: nodes on nodes, attributes on lines other than
/// their own and their owner's other attributes', labels on nodes and on each other. Pairs of
/// presentation ids; a label is named by its line.
pub fn overlaps(model: &Model, diagram: Id, measure: &dyn TextMeasure) -> Vec<(Id, Id)> {
    let scene = scene::build(model, diagram, &Preview::default(), measure);
    let Some(view) = model.view(diagram) else { return Vec::new() };
    let element = |id: Id| view.get(id).and_then(|p| p.element).and_then(|e| model.get(e));
    let is_attribute =
        |id: Id| matches!(element(id).map(|e| e.kind()), Some(ElementKind::ErAttribute(_)));
    let owner = |id: Id| element(id).and_then(|e| e.owner());
    let mut out = Vec::new();
    for (k, a) in scene.nodes.iter().enumerate() {
        for b in &scene.nodes[k + 1..] {
            if overlap(&a.rect, &b.rect) {
                out.push((a.id, b.id));
            }
        }
    }
    let mut labels: Vec<(Id, Rect)> = Vec::new();
    for e in &scene.edges {
        let ends = view.get(e.id).and_then(|p| p.endpoints());
        // An attribute's line: the attributes of the same owner may touch it.
        let line_owner =
            ends.and_then(|(s, t)| [s, t].into_iter().find(|&n| is_attribute(n)).and_then(owner));
        for n in scene.nodes.iter().filter(|n| is_attribute(n.id)) {
            if ends.is_some_and(|(s, t)| s == n.id || t == n.id) {
                continue;
            }
            if line_owner.is_some() && line_owner == owner(n.id) {
                continue;
            }
            let r = inflate(&n.rect, -1.0);
            if e.points.windows(2).any(|w| segment_hits(w[0], w[1], &r)) {
                out.push((n.id, e.id));
            }
        }
        // No line runs through an entity, relationship or IS-A it does not join.
        for n in scene.nodes.iter().filter(|n| !is_attribute(n.id) && element(n.id).is_some()) {
            if ends.is_some_and(|(s, t)| s == n.id || t == n.id) {
                continue;
            }
            let r = inflate(&n.rect, -1.0);
            if e.points.windows(2).any(|w| segment_hits(w[0], w[1], &r)) {
                out.push((n.id, e.id));
            }
        }
        labels.extend(e.labels.iter().map(|l| (e.id, crate::labels::label_rect(l, measure))));
    }
    for (k, (id, r)) in labels.iter().enumerate() {
        for n in &scene.nodes {
            if overlap(r, &n.rect) {
                out.push((*id, n.id));
            }
        }
        for (other, s) in &labels[k + 1..] {
            if overlap(r, s) {
                out.push((*id, *other));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use umlcraft_model::{
        Cardinality, DiagramKind, Document, ElementData, ElementKind, ErAttributeProps,
        ErEntityProps, ErIsAProps, ErKey, ErNotation, ErRelationshipProps, Model, Rect, Tx,
    };

    use super::*;
    use crate::scene::ApproxMeasure;

    struct Diagram {
        doc: Document,
        package: Id,
        diagram: Id,
    }

    impl Diagram {
        fn new() -> Self {
            let mut doc = Document::new(Model::new("m"));
            let root = doc.model().root();
            let (mut package, mut diagram) = (root, root);
            doc.edit("setup", |tx| {
                package = tx.create(root, ElementData::new("p", ElementKind::Package))?;
                let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
                diagram = tx.create(package, ElementData::new("d", kind))?;
                Ok(())
            })
            .unwrap();
            Self { doc, package, diagram }
        }

        fn edit<T>(&mut self, f: impl FnOnce(&mut Tx<'_>, Id, Id) -> Result<T>) -> T {
            let (package, diagram) = (self.package, self.diagram);
            let mut out = None;
            self.doc
                .edit("edit", |tx| {
                    out = Some(f(tx, package, diagram)?);
                    Ok(())
                })
                .unwrap();
            out.unwrap()
        }

        /// A new element shown at (0, 0) in the palette's size: (element, node).
        fn add(&mut self, name: &str, kind: ElementKind) -> (Id, Id) {
            let name = name.to_owned();
            let size = crate::palette::default_size_for(&kind);
            self.edit(|tx, package, diagram| {
                let e = tx.create(package, ElementData::new(name, kind))?;
                let bounds = Rect::new(0.0, 0.0, size.width, size.height);
                let n = tx.add_node(diagram, Some(e), bounds)?;
                Ok((e, n))
            })
        }

        fn entity(&mut self, name: &str) -> (Id, Id) {
            self.add(name, ElementKind::ErEntity(ErEntityProps::default()))
        }

        fn attribute(&mut self, owner: Id, name: &str, key: bool) -> Id {
            let key = key.then_some(ErKey::Primary);
            let (_, node) = self.add(
                name,
                ElementKind::ErAttribute(ErAttributeProps { key, ..Default::default() }),
            );
            self.edit(|tx, _, diagram| crate::er::connect(tx, diagram, node, owner));
            node
        }

        /// Joins `relationship` to `entity` with `cardinality`.
        fn join(&mut self, relationship: Id, entity: Id, cardinality: Cardinality) {
            self.edit(|tx, _, diagram| {
                let edge = crate::er::connect(tx, diagram, relationship, entity)?;
                let line = tx.model().view(diagram).unwrap().get(edge).unwrap().element.unwrap();
                tx.update(line, |d| {
                    if let ElementKind::ErParticipation(p) = &mut d.kind {
                        p.cardinality = Some(cardinality);
                    }
                })
            });
        }

        fn arrange(&mut self) {
            self.edit(|tx, _, diagram| arrange(tx, diagram, &ApproxMeasure));
        }

        fn bounds(&self, node: Id) -> Rect {
            self.doc.model().view(self.diagram).unwrap().get(node).unwrap().bounds().unwrap()
        }

        fn overlaps(&self) -> Vec<(Id, Id)> {
            overlaps(self.doc.model(), self.diagram, &ApproxMeasure)
        }

        fn on_grid(&self) -> bool {
            let view = self.doc.model().view(self.diagram).unwrap();
            view.presentations()
                .iter()
                .filter_map(|p| p.bounds())
                .all(|b| b.x % GRID == 0.0 && b.y % GRID == 0.0)
        }
    }

    /// Kunde – bucht – Ticket, Kunde with six attributes.
    fn sample() -> (Diagram, Id, Id, Id) {
        let mut d = Diagram::new();
        let (_, kunde) = d.entity("Kunde");
        let (_, ticket) = d.entity("Ticket");
        let (_, bucht) =
            d.add("bucht", ElementKind::ErRelationship(ErRelationshipProps::default()));
        d.join(bucht, kunde, Cardinality::ONE_ONE);
        d.join(bucht, ticket, Cardinality::ZERO_MANY);
        for (k, name) in
            ["id", "vorname", "nachname", "adresse", "email", "telefon"].iter().enumerate()
        {
            d.attribute(kunde, name, k == 0);
        }
        d.attribute(ticket, "id", true);
        (d, kunde, ticket, bucht)
    }

    #[test]
    fn ie_lines_that_skip_a_layer_go_around_the_tables_there() {
        let mut d = Diagram::new();
        d.edit(|tx, _, diagram| {
            crate::er::set_notation(tx, diagram, ErNotation::Ie, &ApproxMeasure)
        });
        let (_, airport) = d.entity("Flughafen");
        let (_, gate) = d.entity("Gate");
        let (_, flight) = d.entity("Flug");
        let relate = |d: &mut Diagram, line, parent, child| {
            d.edit(|tx, _, diagram| crate::er::relate(tx, diagram, line, parent, child))
        };
        relate(&mut d, crate::er::IeLine::Identifying, airport, gate);
        relate(&mut d, crate::er::IeLine::NonIdentifying, gate, flight);
        // Start and destination: two lines past the gate, one on each side.
        let start = relate(&mut d, crate::er::IeLine::NonIdentifying, airport, flight);
        let destination = relate(&mut d, crate::er::IeLine::NonIdentifying, airport, flight);
        d.arrange();
        let y = |n: Id| d.bounds(n).center().y;
        assert!(y(airport) < y(gate) && y(gate) < y(flight));
        assert_eq!(d.overlaps(), []);
        let view = d.doc.model().view(d.diagram).unwrap();
        let bends = |edge: Id| match &view.get(edge).unwrap().shape {
            Shape::Edge { waypoints, .. } => waypoints.clone(),
            _ => Vec::new(),
        };
        let g = d.bounds(gate);
        let (a, b) = (bends(start), bends(destination));
        assert!(!a.is_empty() && !b.is_empty());
        let left = |w: &[Point]| w.iter().all(|p| p.x < g.x);
        let right = |w: &[Point]| w.iter().all(|p| p.x > g.x + g.width);
        assert!((left(&a) && right(&b)) || (right(&a) && left(&b)), "{a:?} {b:?} {g:?}");
    }

    #[test]
    fn the_one_side_goes_above_the_relationship() {
        let (mut d, kunde, ticket, bucht) = sample();
        d.arrange();
        let y = |n: Id| d.bounds(n).center().y;
        assert!(y(kunde) < y(bucht) && y(bucht) < y(ticket));
        assert_eq!(d.overlaps(), []);
        assert!(d.on_grid());
        let view = d.doc.model().view(d.diagram).unwrap();
        let x0 = view
            .presentations()
            .iter()
            .filter_map(|p| p.bounds())
            .map(|b| b.x)
            .fold(f32::INFINITY, f32::min);
        let y0 = view
            .presentations()
            .iter()
            .filter_map(|p| p.bounds())
            .map(|b| b.y)
            .fold(f32::INFINITY, f32::min);
        assert_eq!((x0, y0), (MARGIN, MARGIN));
    }

    #[test]
    fn attributes_keep_off_the_sides_with_lines() {
        let (mut d, kunde, ..) = sample();
        d.arrange();
        let k = d.bounds(kunde);
        let model = d.doc.model();
        let view = model.view(d.diagram).unwrap();
        let kunde_element = view.get(kunde).unwrap().element;
        // Kunde's line leaves at the bottom: no attribute below it.
        let mut seen = 0;
        for p in view.presentations() {
            let (Some(e), Some(b)) = (p.element.and_then(|e| model.get(e)), p.bounds()) else {
                continue;
            };
            if matches!(e.kind(), ElementKind::ErAttribute(_)) && e.owner() == kunde_element {
                assert!(b.y < k.y + k.height, "{}", e.name());
                seen += 1;
            }
        }
        assert_eq!(seen, 6);
    }

    #[test]
    fn arranging_again_gives_the_same_picture() {
        let (mut d, ..) = sample();
        d.arrange();
        let once = umlcraft_model::Model::clone(d.doc.model());
        d.arrange();
        assert_eq!(*d.doc.model(), once);
    }

    #[test]
    fn self_references_and_parallel_lines_are_spread() {
        let mut d = Diagram::new();
        let (_, m) = d.entity("Mitarbeiter");
        let (_, r) =
            d.add("vorgesetzter", ElementKind::ErRelationship(ErRelationshipProps::default()));
        d.join(r, m, Cardinality::ONE_ONE);
        d.join(r, m, Cardinality::ZERO_MANY);
        let (_, a) = d.entity("A");
        let (_, b) = d.entity("B");
        for name in ["x", "y"] {
            let (_, r) = d.add(name, ElementKind::ErRelationship(ErRelationshipProps::default()));
            d.join(r, a, Cardinality::ONE_ONE);
            d.join(r, b, Cardinality::ZERO_MANY);
        }
        let (_, lonely) = d.entity("Allein");
        d.attribute(lonely, "id", true);
        d.arrange();
        let view = d.doc.model().view(d.diagram).unwrap();
        let routes: Vec<_> = view
            .edges_attached_to(r)
            .filter_map(|p| match &p.shape {
                Shape::Edge { waypoints, .. } => Some(waypoints.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(routes.len(), 2);
        assert_ne!(routes[0], routes[1]);
        assert_eq!(d.overlaps(), []);
        assert!(d.on_grid());
    }

    #[test]
    fn many_attributes_take_more_rings() {
        let mut d = Diagram::new();
        let (_, e) = d.entity("Viel");
        for k in 0..30 {
            d.attribute(e, &format!("spalte_{k}"), false);
        }
        d.arrange();
        assert_eq!(d.overlaps(), []);
        assert!(d.on_grid());
    }

    #[test]
    fn is_a_puts_the_supertype_above() {
        let mut d = Diagram::new();
        let (_, sup) = d.entity("Person");
        let (_, sub) = d.entity("Kunde");
        let (_, is_a) = d.add("", ElementKind::ErIsA(ErIsAProps::default()));
        d.edit(|tx, _, diagram| crate::er::connect(tx, diagram, is_a, sup));
        d.edit(|tx, _, diagram| crate::er::connect(tx, diagram, is_a, sub));
        d.arrange();
        let y = |n: Id| d.bounds(n).y;
        assert!(y(sup) < y(is_a) && y(is_a) < y(sub));
    }

    #[test]
    fn long_names_make_their_box_grow() {
        let mut d = Diagram::new();
        let (_, e) = d.entity("Fluggesellschaftsverwaltung");
        d.arrange();
        let width = ApproxMeasure.width("Fluggesellschaftsverwaltung", scene::FONT_SIZE, false);
        assert!(d.bounds(e).width >= width);
    }
}
