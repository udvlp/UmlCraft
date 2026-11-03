//! Automatic layout (`docs/erm-spec.md` § 7.2): for the SQL import and for "arrange" in the
//! editor. Works on any diagram.
//!
//! 1. Entities, relationships and IS-As are the core graph. Each relationship goes into the
//!    layer below the entity on its "one" side (the line with `[x,1]`, look-across: the
//!    referenced table) and above the others; an IS-A below its supertype.
//! 2. Each core node reserves room for its attributes around it; the boxes go to the layered
//!    layout of `umlcraft-notation`. A second round reserves the room on the sides that turned
//!    out to be free of lines.
//! 3. Attributes go on the sides without lines first (top, right, left, bottom): rows above
//!    and below, columns at the sides, keys first, 30 px from their owner, further out when a
//!    side is full. One that would cover a node, a line or a label moves to the next free
//!    place.
//! 4. Lines float (no fixed ports); several lines between the same two nodes get a waypoint
//!    each so they do not lie on top of each other. Labels sit in the middle of their line.
//! 5. Nodes are on the 10 px grid; the whole starts at (20, 20). Free texts stay where they are.

use std::collections::HashMap;

use umlcraft_model::Size;
use umlcraft_notation::layout::{Direction, Edge, layered};

use crate::draw::label_box;
use crate::geometry::points_of;
use crate::model::{Diagram, Id, LabelPos, LabelText, Max, NodeKind, Point, Rect};

/// How far a cardinality sits beside its line (a role on the other side).
pub const LABEL_OFFSET: f64 = 14.0;
const GRID: f64 = 10.0;
/// Between a node and its nearest attributes.
const GAP: f64 = 30.0;
/// Between attributes in a row.
const ROW_GAP: f64 = 10.0;
/// From one attribute to the next in a column (the specification's 36 px, raised to a whole
/// grid step so that two neighbours stay apart after snapping).
const COLUMN_PITCH: f64 = 40.0;
/// Between the columns of two rings.
const COLUMN_GAP: f64 = 20.0;
/// From one ring of rows to the next.
const RING_STEP: f64 = 40.0;
/// Attributes per row (top, bottom) and per column (sides) in one ring.
const ROW_CAPACITY: usize = 4;
const COLUMN_CAPACITY: usize = 3;
/// Between lines that join the same two nodes.
const PARALLEL_SPREAD: f64 = 60.0;
/// Free room kept around what is placed.
const CLEARANCE: f64 = 6.0;
/// The top left corner of the arranged diagram.
const MARGIN: f64 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

impl Side {
    /// The order attributes take the sides in (as in the flight example: top, then the
    /// sides, the bottom last).
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
    const fn outward(self) -> (f64, f64) {
        match self {
            Self::Top => (0.0, -1.0),
            Self::Right => (1.0, 0.0),
            Self::Bottom => (0.0, 1.0),
            Self::Left => (-1.0, 0.0),
        }
    }

    /// The side of a `w` × `h` box that the direction `(dx, dy)` leaves it through.
    fn of(dx: f64, dy: f64, w: f64, h: f64) -> Self {
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

/// Lays out the whole diagram.
pub fn arrange(d: &mut Diagram) {
    if d.nodes.is_empty() {
        return;
    }
    let g = Graph::of(d);
    let first = place(d, &g, &vec![[false; 4]; g.core.len()]);
    apply(d, &g, &first);
    let used = used_sides(d, &g);
    let second = place(d, &g, &used);
    apply(d, &g, &second);
    place_labels(d, &g);
    place_attributes(d, &g);
    normalize(d);
}

/// The core graph: what the layered layout places, and whose attributes go around what.
struct Graph {
    /// Node indices of entities, relationships, IS-As (and attributes without owner).
    core: Vec<usize>,
    /// Core position of a node index.
    position: HashMap<usize, usize>,
    /// Node index of an id.
    index: HashMap<Id, usize>,
    /// Per core node the node indices of its attributes, keys first.
    attributes: Vec<Vec<usize>>,
    /// Parts of composite attributes with their parent, parents before their parts.
    parts: Vec<(usize, usize)>,
    /// Core position of each root attribute's owner (by node index).
    owner: HashMap<usize, usize>,
    edges: Vec<Edge>,
}

impl Graph {
    fn of(d: &Diagram) -> Self {
        let index: HashMap<&Id, usize> =
            d.nodes.iter().enumerate().map(|(i, n)| (&n.id, i)).collect();
        let mut core = Vec::new();
        let mut roots: Vec<(usize, usize)> = Vec::new();
        let mut parts = Vec::new();
        for (i, n) in d.nodes.iter().enumerate() {
            if !n.kind.is_attribute() {
                core.push(i);
                continue;
            }
            match d.owner_of(&n.id) {
                Some(o) if o.kind.is_attribute() => parts.push((i, index[&o.id])),
                Some(o) => roots.push((i, index[&o.id])),
                None => core.push(i),
            }
        }
        let position: HashMap<usize, usize> =
            core.iter().enumerate().map(|(p, &i)| (i, p)).collect();
        let mut attributes = vec![Vec::new(); core.len()];
        let mut owner = HashMap::new();
        for &(a, o) in &roots {
            attributes[position[&o]].push(a);
            owner.insert(a, position[&o]);
        }
        let is_key =
            |i: &usize| matches!(d.nodes[*i].kind, NodeKind::Attribute { key: Some(_), .. });
        for list in &mut attributes {
            let (keys, rest): (Vec<usize>, Vec<usize>) = list.iter().partition(|i| is_key(i));
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
        let edges = core_edges(d, &index, &position);
        let index = d.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
        Self { core, position, index, attributes, parts: ordered, owner, edges }
    }
}

/// Edges for the layered layout: the "one" side above a relationship, the others below; the
/// supertype above an IS-A, its subtypes below.
fn core_edges(
    d: &Diagram,
    index: &HashMap<&Id, usize>,
    position: &HashMap<usize, usize>,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    for (i, hub) in d.nodes.iter().enumerate() {
        let is_hub = matches!(
            hub.kind,
            NodeKind::Relationship { .. } | NodeKind::IsA { .. } | NodeKind::AssociativeEntity
        );
        let Some(&h) = position.get(&i).filter(|_| is_hub) else { continue };
        let ends: Vec<(usize, &crate::model::Link)> = d
            .links
            .iter()
            .filter(|l| l.source == hub.id && l.target != hub.id)
            .filter_map(|l| {
                let t = *index.get(&l.target)?;
                let p = *position.get(&t)?;
                (!d.nodes[t].kind.is_attribute()).then_some((p, l))
            })
            .collect();
        let is_a = matches!(hub.kind, NodeKind::IsA { .. });
        let upper: Option<usize> = if is_a {
            ends.iter().find(|(_, l)| l.super_type).map(|e| e.0)
        } else {
            let one = ends.iter().find(|(_, l)| {
                matches!(&l.label, Some(lb) if matches!(lb.text,
                    LabelText::Cardinality(c) if c.max == Max::Count(1)))
            });
            one.or(ends.first()).map(|e| e.0)
        };
        if let Some(u) = upper {
            edges.push(Edge { upper: u, lower: h, hierarchy: is_a });
        }
        let mut seen = Vec::new();
        for &(p, _) in &ends {
            if Some(p) == upper || seen.contains(&p) {
                continue;
            }
            seen.push(p);
            edges.push(Edge { upper: h, lower: p, hierarchy: is_a });
        }
    }
    edges
}

/// The top left corners of the core nodes, on the grid.
fn place(d: &Diagram, g: &Graph, used: &[Used]) -> Vec<Point> {
    let mut sizes = Vec::with_capacity(g.core.len());
    let mut offsets = Vec::with_capacity(g.core.len());
    for (c, &i) in g.core.iter().enumerate() {
        let b = d.nodes[i].bounds;
        let attrs: Vec<(f64, f64)> = g.attributes[c]
            .iter()
            .map(|&a| (d.nodes[a].bounds.width, d.nodes[a].bounds.height))
            .collect();
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
        sizes.push(Size::new((x1 - x0 + 2.0 * slack) as f32, (y1 - y0 + 2.0 * slack) as f32));
        offsets.push(Point::new(-x0 + slack - b.width / 2.0, -y0 + slack - b.height / 2.0));
    }
    let corners = layered(&sizes, &g.edges, Direction::Down);
    corners
        .iter()
        .zip(&offsets)
        .map(|(p, o)| Point::new(snap(f64::from(p.x) + o.x), snap(f64::from(p.y) + o.y)))
        .collect()
}

/// Moves the core nodes and routes the lines between them.
fn apply(d: &mut Diagram, g: &Graph, corners: &[Point]) {
    for (&i, p) in g.core.iter().zip(corners) {
        let b = &mut d.nodes[i].bounds;
        b.x = p.x;
        b.y = p.y;
    }
    // Lines float; lines that join the same two nodes are spread apart.
    let mut order: Vec<(Id, Id)> = Vec::new();
    let mut groups: HashMap<(Id, Id), Vec<usize>> = HashMap::new();
    for (k, l) in d.links.iter_mut().enumerate() {
        l.source_port = None;
        l.target_port = None;
        l.waypoints.clear();
        let pair = if l.source <= l.target {
            (l.source.clone(), l.target.clone())
        } else {
            (l.target.clone(), l.source.clone())
        };
        let list = groups.entry(pair.clone()).or_default();
        if list.is_empty() {
            order.push(pair);
        }
        list.push(k);
    }
    for (a, b) in order {
        let links = &groups[&(a.clone(), b.clone())];
        if links.len() < 2 {
            continue;
        }
        let (Some(na), Some(nb)) = (d.node(&a), d.node(&b)) else { continue };
        let (ca, cb) = (na.bounds.center(), nb.bounds.center());
        let (dx, dy) = (cb.x - ca.x, cb.y - ca.y);
        let len = dx.hypot(dy);
        let (nx, ny) = if len > 0.0 { (-dy / len, dx / len) } else { (1.0, 0.0) };
        let mid = Point::new((ca.x + cb.x) / 2.0, (ca.y + cb.y) / 2.0);
        let m = links.len() as f64;
        for (k, &l) in links.iter().enumerate() {
            let off = (k as f64 - (m - 1.0) / 2.0) * PARALLEL_SPREAD;
            if off != 0.0 {
                d.links[l].waypoints = vec![Point::new(mid.x + nx * off, mid.y + ny * off)];
            }
        }
    }
}

/// Which sides of each core node lines to other core nodes leave through.
fn used_sides(d: &Diagram, g: &Graph) -> Vec<Used> {
    let mut used = vec![[false; 4]; g.core.len()];
    for l in &d.links {
        let (Some(&si), Some(&ti)) = (g.index.get(&l.source), g.index.get(&l.target)) else {
            continue;
        };
        let (s, t) = (&d.nodes[si], &d.nodes[ti]);
        if s.kind.is_attribute() || t.kind.is_attribute() {
            continue;
        }
        let ends = [(si, s, t, l.waypoints.first()), (ti, t, s, l.waypoints.last())];
        for (i, me, other, toward) in ends {
            let Some(&c) = g.position.get(&i) else { continue };
            let from = me.bounds.center();
            let to = toward.copied().unwrap_or_else(|| other.bounds.center());
            if from == to {
                continue;
            }
            let side = Side::of(to.x - from.x, to.y - from.y, me.bounds.width, me.bounds.height);
            used[c][side.index()] = true;
        }
    }
    used
}

/// Labels in the middle of their line, a cardinality on one side and a role on the other;
/// one that would cover a node or another label moves along its line or to the other side.
/// Attributes are placed later and keep clear of the labels.
fn place_labels(d: &mut Diagram, g: &Graph) {
    let nodes: Vec<Rect> = g.core.iter().map(|&i| d.nodes[i].bounds).collect();
    let mut placed: Vec<Rect> = Vec::new();
    for k in 0..d.links.len() {
        let l = &d.links[k];
        let Some(points) = points_of(d, l) else { continue };
        let free = |r: &Rect, placed: &[Rect]| {
            let r = inflate(r, 2.0);
            !nodes.iter().chain(placed).any(|o| overlap(&r, o))
        };
        let pick = |text: &str, side: f64, placed: &[Rect]| {
            let candidates =
                [(0.0, side), (0.0, -side), (0.4, side), (-0.4, side), (0.4, -side), (-0.4, -side)];
            let pos = |(x, y): (f64, f64)| LabelPos { x, y, ..LabelPos::default() };
            let best = candidates
                .into_iter()
                .map(pos)
                .find(|p| free(&label_box(&points, text, p).1, placed));
            let p = best.unwrap_or_else(|| pos(candidates[0]));
            (p, label_box(&points, text, &p).1)
        };
        let label = l.label.as_ref().map(|lb| pick(&lb.display(), LABEL_OFFSET, &placed));
        if let Some((_, r)) = label {
            placed.push(r);
        }
        let role = l.role.as_ref().map(|r| pick(&r.text, -LABEL_OFFSET, &placed));
        if let Some((_, r)) = role {
            placed.push(r);
        }
        let l = &mut d.links[k];
        if let (Some(lb), Some((p, _))) = (&mut l.label, label) {
            lb.pos = p;
        }
        if let (Some(r), Some((p, _))) = (&mut l.role, role) {
            r.pos = p;
        }
    }
}

/// A place for an attribute next to its owner.
struct Slot {
    side: Side,
    /// Relative to the owner's center.
    center: Point,
}

/// Places for attributes of the given sizes around a `w` × `h` node: the free sides ring by
/// ring, then the sides with lines.
fn slots(w: f64, h: f64, attrs: &[(f64, f64)], used: Used) -> Vec<Slot> {
    // Which (side, ring) each attribute goes to.
    let mut plan: Vec<(Side, usize)> = Vec::with_capacity(attrs.len());
    let free: Vec<Side> = Side::ORDER.into_iter().filter(|s| !used[s.index()]).collect();
    let busy: Vec<Side> = Side::ORDER.into_iter().filter(|s| used[s.index()]).collect();
    // Ring by ring: the free sides, then those with lines (attributes there slide past the
    // lines when they are placed).
    let sides: Vec<Side> = free.into_iter().chain(busy).collect();
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
        plan.iter().map(|&(side, _)| Slot { side, center: Point::default() }).collect();
    let rings = plan.iter().map(|p| p.1).max().map_or(0, |r| r + 1);
    // Rows keep clear of the first ring's columns, which may be taller than the node.
    let column = |side: Side| {
        let n = plan.iter().filter(|p| **p == (side, 0)).count();
        let tallest = (0..plan.len())
            .filter(|&k| plan[k] == (side, 0))
            .map(|k| attrs[k].1)
            .fold(0.0, f64::max);
        if n == 0 { 0.0 } else { ((n - 1) as f64 * COLUMN_PITCH + tallest) / 2.0 + ROW_GAP }
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
                    let total: f64 = members.iter().map(|&k| attrs[k].0).sum::<f64>()
                        + ROW_GAP * (members.len() as f64 - 1.0);
                    let mut x = -total / 2.0;
                    for &k in &members {
                        let (aw, ah) = attrs[k];
                        let y = row_base + ah / 2.0 + ring as f64 * RING_STEP;
                        out[k].center =
                            Point::new(x + aw / 2.0, if side == Side::Top { -y } else { y });
                        x += aw + ROW_GAP;
                    }
                }
                Side::Left | Side::Right => {
                    let widest = members.iter().map(|&k| attrs[k].0).fold(0.0, f64::max);
                    let m = members.len() as f64;
                    for (n, &k) in members.iter().enumerate() {
                        let x = inner + attrs[k].0 / 2.0;
                        let y = (n as f64 - (m - 1.0) / 2.0) * COLUMN_PITCH;
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

fn place_attributes(d: &mut Diagram, g: &Graph) {
    let mut obstacles = Obstacles { rects: Vec::new(), segments: Vec::new() };
    for &i in &g.core {
        obstacles.rects.push((d.nodes[i].bounds, None));
    }
    for l in &d.links {
        let (Some(s), Some(t)) = (d.node(&l.source), d.node(&l.target)) else { continue };
        if s.kind.is_attribute() || t.kind.is_attribute() {
            continue;
        }
        let Some(points) = points_of(d, l) else { continue };
        for w in points.windows(2) {
            obstacles.segments.push((w[0], w[1], None));
        }
        let texts = l
            .label
            .iter()
            .map(|lb| (lb.display(), lb.pos))
            .chain(l.role.iter().map(|r| (r.text.clone(), r.pos)));
        for (text, pos) in texts {
            obstacles.rects.push((label_box(&points, &text, &pos).1, None));
        }
    }
    let used = used_sides(d, g);
    for (c, &i) in g.core.iter().enumerate() {
        let owner = d.nodes[i].bounds;
        let attrs: Vec<(f64, f64)> = g.attributes[c]
            .iter()
            .map(|&a| (d.nodes[a].bounds.width, d.nodes[a].bounds.height))
            .collect();
        let slots = slots(owner.width, owner.height, &attrs, used[c]);
        for ((&a, slot), &(aw, ah)) in g.attributes[c].iter().zip(&slots).zip(&attrs) {
            let center = owner.center();
            let wanted = Point::new(center.x + slot.center.x, center.y + slot.center.y);
            let r = find_place(&obstacles, Some(c), &owner, wanted, slot.side.outward(), (aw, ah));
            settle(d, &mut obstacles, a, r, c, &owner);
        }
    }
    for &(part, parent) in &g.parts {
        let p = d.nodes[parent].bounds;
        let root_owner = root_center(d, g, parent);
        let (dx, dy) = (p.center().x - root_owner.x, p.center().y - root_owner.y);
        let len = dx.hypot(dy);
        let dir = if len > 0.0 { (dx / len, dy / len) } else { (0.0, -1.0) };
        let (aw, ah) = (d.nodes[part].bounds.width, d.nodes[part].bounds.height);
        let reach = (p.width + aw) / 2.0 * dir.0.abs() + (p.height + ah) / 2.0 * dir.1.abs() + GAP;
        let wanted = Point::new(p.center().x + dir.0 * reach, p.center().y + dir.1 * reach);
        let at = find_place(&obstacles, None, &p, wanted, dir, (aw, ah));
        d.nodes[part].bounds = at;
        obstacles.rects.push((at, None));
        obstacles.segments.push((at.center(), p.center(), None));
    }
}

fn settle(
    d: &mut Diagram,
    obstacles: &mut Obstacles,
    a: usize,
    r: Rect,
    owner: usize,
    owner_box: &Rect,
) {
    d.nodes[a].bounds = r;
    obstacles.rects.push((r, Some(owner)));
    obstacles.segments.push((r.center(), owner_box.center(), Some(owner)));
}

/// The center of the core node a (possibly nested) attribute belongs to.
fn root_center(d: &Diagram, g: &Graph, mut attribute: usize) -> Point {
    for _ in 0..16 {
        if let Some(&c) = g.owner.get(&attribute) {
            return d.nodes[g.core[c]].bounds.center();
        }
        match g.parts.iter().find(|(p, _)| *p == attribute) {
            Some(&(_, parent)) => attribute = parent,
            None => break,
        }
    }
    d.nodes[attribute].bounds.center()
}

/// The box for an attribute of `size` centered at `wanted`, on the grid, or the nearest free
/// one: further out in `outward`, then around the owner.
fn find_place(
    obstacles: &Obstacles,
    owner: Option<usize>,
    owner_box: &Rect,
    wanted: Point,
    outward: (f64, f64),
    (w, h): (f64, f64),
) -> Rect {
    let at = |c: Point| Rect::new(snap(c.x - w / 2.0), snap(c.y - h / 2.0), w, h);
    let c = owner_box.center();
    let (rx, ry) = (owner_box.width / 2.0 + GAP + w / 2.0, owner_box.height / 2.0 + GAP + h / 2.0);
    // Along the side: rows slide sideways, columns up and down.
    let along = (outward.1.abs(), outward.0.abs());
    let slides =
        (0..=8).map(|k| f64::from((k + 1) / 2) * GRID * if k % 2 == 0 { 1.0 } else { -1.0 });
    // Around the owner, starting in the direction of the wanted place.
    let start = (wanted.y - c.y).atan2(wanted.x - c.x);
    let angles: Vec<f64> = (0..24)
        .map(|k| {
            start
                + f64::from((k + 1) / 2) * 15f64.to_radians() * if k % 2 == 0 { 1.0 } else { -1.0 }
        })
        .collect();
    // The slot (sliding a little), further out, then around the owner: first clear of
    // everything, then at least clear of what is not the owner's.
    for (strict, rings) in [(true, 12), (false, 30)] {
        for step in 0..=3 {
            let s = f64::from(step) * RING_STEP;
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
            let grow = f64::from(ring) * 20.0;
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
fn normalize(d: &mut Diagram) {
    let x0 = d.nodes.iter().map(|n| n.bounds.x).fold(f64::INFINITY, f64::min);
    let y0 = d.nodes.iter().map(|n| n.bounds.y).fold(f64::INFINITY, f64::min);
    let (dx, dy) = (snap(MARGIN - x0), snap(MARGIN - y0));
    for n in &mut d.nodes {
        n.bounds.x += dx;
        n.bounds.y += dy;
    }
    for l in &mut d.links {
        for p in &mut l.waypoints {
            p.x += dx;
            p.y += dy;
        }
    }
}

fn snap(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

fn inflate(r: &Rect, by: f64) -> Rect {
    Rect::new(r.x - by, r.y - by, r.width + 2.0 * by, r.height + 2.0 * by)
}

/// Whether two boxes share more than a touching edge.
fn overlap(a: &Rect, b: &Rect) -> bool {
    const EPS: f64 = 0.5;
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

/// What covers what in a laid-out diagram (for tests and the import's check): nodes on
/// nodes, attributes on lines other than those of their owner's attributes, labels on nodes
/// and on each other. Pairs of ids; a label is named by its link.
pub fn overlaps(d: &Diagram) -> Vec<(Id, Id)> {
    let mut out = Vec::new();
    for (k, a) in d.nodes.iter().enumerate() {
        for b in &d.nodes[k + 1..] {
            if overlap(&a.bounds, &b.bounds) {
                out.push((a.id.clone(), b.id.clone()));
            }
        }
    }
    let mut labels: Vec<(Id, Rect)> = Vec::new();
    for l in &d.links {
        let Some(points) = points_of(d, l) else { continue };
        let owner_of_line = |id: &Id| {
            d.node(id).filter(|n| n.kind.is_attribute()).and(d.owner_of(id)).map(|o| o.id.clone())
        };
        let line_owner = owner_of_line(&l.source).or_else(|| owner_of_line(&l.target));
        for n in d.nodes.iter().filter(|n| n.kind.is_attribute()) {
            if l.source == n.id || l.target == n.id {
                continue;
            }
            let mine = d.owner_of(&n.id).map(|o| o.id.clone());
            if line_owner.is_some() && line_owner == mine {
                continue;
            }
            let r = inflate(&n.bounds, -1.0);
            if points.windows(2).any(|w| segment_hits(w[0], w[1], &r)) {
                out.push((n.id.clone(), l.id.clone()));
            }
        }
        let texts = l
            .label
            .iter()
            .map(|lb| (lb.display(), lb.pos))
            .chain(l.role.iter().map(|r| (r.text.clone(), r.pos)));
        for (text, pos) in texts {
            labels.push((l.id.clone(), label_box(&points, &text, &pos).1));
        }
    }
    for (k, (id, r)) in labels.iter().enumerate() {
        for n in &d.nodes {
            if overlap(r, &n.bounds) {
                out.push((id.clone(), n.id.clone()));
            }
        }
        for (other, s) in &labels[k + 1..] {
            if overlap(r, s) {
                out.push((id.clone(), other.clone()));
            }
        }
    }
    out
}

/// Lines between entities and relationships that run through another node (long edges of
/// the layered layout drawn straight). Pairs of link and node.
pub fn crossings(d: &Diagram) -> Vec<(Id, Id)> {
    let mut out = Vec::new();
    for l in &d.links {
        let Some(points) = points_of(d, l) else { continue };
        for n in &d.nodes {
            if n.id == l.source || n.id == l.target {
                continue;
            }
            let r = inflate(&n.bounds, -1.0);
            if points.windows(2).any(|w| segment_hits(w[0], w[1], &r)) {
                out.push((l.id.clone(), n.id.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::SizeSet;
    use crate::model::{Cardinality, Label};

    fn on_grid(d: &Diagram) -> bool {
        d.nodes.iter().all(|n| n.bounds.x % GRID == 0.0 && n.bounds.y % GRID == 0.0)
    }

    fn join(d: &mut Diagram, r: &Id, e: &Id, max: Max) {
        let l = d.connect(r, e).unwrap();
        let link = d.links.iter_mut().find(|x| x.id == l).unwrap();
        link.label = Some(Label {
            text: LabelText::Cardinality(Cardinality::new(0, max)),
            pos: LabelPos::default(),
        });
    }

    fn sample() -> (Diagram, Id, Id, Id) {
        let mut d = Diagram::new("t");
        let o = Point::default();
        let kunde = d.add_entity("Kunde", o, SizeSet::Compact);
        let ticket = d.add_entity("Ticket", o, SizeSet::Compact);
        let r = d.add_relationship("bucht", o, SizeSet::Compact);
        join(&mut d, &r, &kunde, Max::Count(1));
        join(&mut d, &r, &ticket, Max::Many);
        for name in ["id", "vorname", "nachname", "adresse", "email", "telefon"] {
            d.add_attribute(&kunde, name, SizeSet::Compact).unwrap();
        }
        d.add_attribute(&ticket, "id", SizeSet::Compact).unwrap();
        (d, kunde, ticket, r)
    }

    #[test]
    fn the_one_side_goes_above_the_relationship() {
        let (mut d, kunde, ticket, r) = sample();
        arrange(&mut d);
        let y = |id: &Id| d.node(id).unwrap().bounds.center().y;
        assert!(y(&kunde) < y(&r) && y(&r) < y(&ticket));
        assert_eq!(overlaps(&d), []);
        assert!(on_grid(&d));
        let x0 = d.nodes.iter().map(|n| n.bounds.x).fold(f64::INFINITY, f64::min);
        let y0 = d.nodes.iter().map(|n| n.bounds.y).fold(f64::INFINITY, f64::min);
        assert_eq!((x0, y0), (MARGIN, MARGIN));
    }

    #[test]
    fn attributes_keep_off_the_sides_with_lines() {
        let (mut d, kunde, _, _) = sample();
        arrange(&mut d);
        let k = d.node(&kunde).unwrap().bounds;
        // Kunde's line leaves at the bottom: no attribute below it.
        for n in d.nodes.iter().filter(|n| n.kind.is_attribute()) {
            if d.owner_of(&n.id).unwrap().id == kunde {
                assert!(n.bounds.y < k.y + k.height, "{}", n.name);
            }
        }
    }

    #[test]
    fn layouts_do_not_change_from_run_to_run() {
        let (mut a, ..) = sample();
        let mut b = a.clone();
        arrange(&mut a);
        arrange(&mut b);
        assert_eq!(a, b);
        // Arranging again gives the same picture.
        let mut c = a.clone();
        arrange(&mut c);
        assert_eq!(a, c);
    }

    #[test]
    fn self_references_and_parallel_lines_are_spread() {
        let mut d = Diagram::new("t");
        let o = Point::default();
        let m = d.add_entity("Mitarbeiter", o, SizeSet::Compact);
        let r = d.add_relationship("vorgesetzter", o, SizeSet::Compact);
        join(&mut d, &r, &m, Max::Count(1));
        join(&mut d, &r, &m, Max::Many);
        let a = d.add_entity("A", o, SizeSet::Compact);
        let b = d.add_entity("B", o, SizeSet::Compact);
        for name in ["x", "y"] {
            let r = d.add_relationship(name, o, SizeSet::Compact);
            join(&mut d, &r, &a, Max::Count(1));
            join(&mut d, &r, &b, Max::Many);
        }
        let lonely = d.add_entity("Allein", o, SizeSet::Compact);
        d.add_attribute(&lonely, "id", SizeSet::Compact).unwrap();
        arrange(&mut d);
        let self_lines: Vec<_> = d.links.iter().filter(|l| l.source == r).collect();
        assert_eq!(self_lines.len(), 2);
        assert_ne!(self_lines[0].waypoints, self_lines[1].waypoints);
        assert_eq!(overlaps(&d), []);
        assert!(on_grid(&d));
    }

    #[test]
    fn many_attributes_take_more_rings() {
        let mut d = Diagram::new("t");
        let e = d.add_entity("Viel", Point::default(), SizeSet::Compact);
        for k in 0..30 {
            d.add_attribute(&e, &format!("spalte_{k}"), SizeSet::Compact).unwrap();
        }
        arrange(&mut d);
        assert_eq!(overlaps(&d), []);
        assert!(on_grid(&d));
    }

    #[test]
    fn is_a_puts_the_supertype_above() {
        let mut d = Diagram::new("t");
        let o = Point::default();
        let sup = d.add_entity("Person", o, SizeSet::Compact);
        let sub = d.add_entity("Kunde", o, SizeSet::Compact);
        let i = d.add_is_a(o, SizeSet::Compact);
        d.connect(&i, &sub).unwrap();
        let l = d.connect(&i, &sup).unwrap();
        d.links.iter_mut().find(|x| x.id == l).unwrap().super_type = true;
        arrange(&mut d);
        let y = |id: &Id| d.node(id).unwrap().bounds.y;
        assert!(y(&sup) < y(&i) && y(&i) < y(&sub));
    }
}
