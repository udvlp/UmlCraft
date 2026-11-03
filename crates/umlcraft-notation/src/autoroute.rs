//! Automatic routing: right-angled lines around the nodes of a diagram, the way Wybrow,
//! Marriott and Stuckey route connectors ("Orthogonal Connector Routing", 2009), in a simpler
//! form:
//!
//! 1. A grid of the lines along the sides of all nodes (kept [`MARGIN`] away), through their
//!    centers and through the middles of the gaps between them.
//! 2. For each line, the cheapest path through the grid from a side of its source to a side of
//!    its target (A*): length, plus a price for each bend, for stretches that earlier lines
//!    already use, and for running right along a node instead of through the middle of a gap.
//! 3. Lines that run on top of each other are spread apart within the free space around them
//!    ("nudging"); where a shared stretch ends at a node, the lines get ports of their own along
//!    that side.
//!
//! [`route`] works on rectangles alone; [`route_lines`] reads a diagram and writes the routes
//! back as the waypoints and anchors of right-angled lines.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use umlcraft_model::{
    DiagramKind, ElementKind, Id, LineShape, ModelError, Point, Rect, Result, Shape, Tx,
};

/// Distance a route keeps from the nodes it passes.
pub const MARGIN: f32 = 12.0;
/// Distance between lines that share a stretch, where there is room.
const SPACING: f32 = 8.0;
/// What a bend costs, as a length.
const BEND: f32 = 40.0;
/// Extra cost per unit of length of a stretch that an earlier route already uses.
const SHARED: f32 = 0.6;
/// Extra cost per unit of length for running along a node's margin.
const HUG: f32 = 0.05;
/// How close to a corner of its node a port may move.
const PORT_INSET: f32 = 4.0;
/// Coordinates closer than this are the same grid line.
const SAME: f32 = 0.5;

/// A line to route from box `source` to box `target` (indices into the boxes). With `facing`,
/// boxes clear of each other above and below (or side by side) are joined through the sides
/// they turn to each other, where those are free, as generalization trees are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub source: usize,
    pub target: usize,
    pub facing: bool,
}

impl Request {
    /// The sides the line may leave `source` and enter `target` by.
    fn sides(self, source: Rect, target: Rect) -> (Vec<Side>, Vec<Side>) {
        let pair = |s: Side, t: Side| (vec![s], vec![t]);
        match () {
            _ if !self.facing => (Side::ALL.to_vec(), Side::ALL.to_vec()),
            _ if source.y >= target.bottom() => pair(Side::Top, Side::Bottom),
            _ if source.bottom() <= target.y => pair(Side::Bottom, Side::Top),
            _ if source.x >= target.right() => pair(Side::Left, Side::Right),
            _ if source.right() <= target.x => pair(Side::Right, Side::Left),
            _ => (Side::ALL.to_vec(), Side::ALL.to_vec()),
        }
    }
}

/// The sides of a box, in the order the router tries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Right,
    Left,
    Bottom,
    Top,
}

impl Side {
    const ALL: [Side; 4] = [Side::Right, Side::Left, Side::Bottom, Side::Top];

    /// The direction leaving through this side.
    fn outward(self) -> Dir {
        match self {
            Side::Right => Dir::East,
            Side::Left => Dir::West,
            Side::Bottom => Dir::South,
            Side::Top => Dir::North,
        }
    }

    /// The middle of this side of `r`, and the point `MARGIN` outside it.
    fn port(self, r: Rect) -> (Point, Point) {
        let c = r.center();
        match self {
            Side::Right => (Point::new(r.right(), c.y), Point::new(r.right() + MARGIN, c.y)),
            Side::Left => (Point::new(r.x, c.y), Point::new(r.x - MARGIN, c.y)),
            Side::Bottom => (Point::new(c.x, r.bottom()), Point::new(c.x, r.bottom() + MARGIN)),
            Side::Top => (Point::new(c.x, r.y), Point::new(c.x, r.y - MARGIN)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    East,
    West,
    South,
    North,
}

impl Dir {
    const ALL: [Dir; 4] = [Dir::East, Dir::West, Dir::South, Dir::North];

    fn index(self) -> usize {
        self as usize
    }

    fn opposite(self) -> Dir {
        match self {
            Dir::East => Dir::West,
            Dir::West => Dir::East,
            Dir::South => Dir::North,
            Dir::North => Dir::South,
        }
    }

    fn horizontal(self) -> bool {
        matches!(self, Dir::East | Dir::West)
    }
}

/// Sorted, distinct coordinates of grid lines, each marked when it runs along a node's margin.
fn grid_lines(mut values: Vec<(f32, bool)>) -> (Vec<f32>, Vec<bool>) {
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut lines: Vec<(f32, bool)> = Vec::new();
    for (v, hug) in values {
        match lines.last_mut() {
            Some(last) if v - last.0 < SAME => last.1 |= hug,
            _ => lines.push((v, hug)),
        }
    }
    // The middles of the gaps, so that a lone line can run between two nodes.
    let mut with_middles = Vec::with_capacity(lines.len() * 2);
    for (i, &line) in lines.iter().enumerate() {
        if let Some(&(next, _)) = lines.get(i + 1)
            && next - line.0 > 4.0 * SPACING
        {
            with_middles.push(line);
            with_middles.push(((line.0 + next) / 2.0, false));
        } else {
            with_middles.push(line);
        }
    }
    with_middles.into_iter().unzip()
}

/// The index of the grid line at `v`, which must be one: the nearest, as two lines may lie just
/// `SAME` apart.
fn line_index(lines: &[f32], v: f32) -> usize {
    let i = lines.partition_point(|&l| l < v);
    let i = if i == lines.len() || (i > 0 && v - lines[i - 1] < lines[i] - v) {
        i.saturating_sub(1)
    } else {
        i
    };
    debug_assert!(i < lines.len() && (lines[i] - v).abs() < SAME, "{v} is no grid line");
    i
}

struct Grid {
    xs: Vec<f32>,
    ys: Vec<f32>,
    hug_x: Vec<bool>,
    hug_y: Vec<bool>,
    /// The boxes (with their margin) each vertex lies inside, by `j * nx + i`.
    cells: Vec<Vec<u32>>,
    /// The boxes each stretch from vertex `(i, j)` to `(i + 1, j)` runs through.
    east: Vec<Vec<u32>>,
    /// The boxes each stretch from vertex `(i, j)` to `(i, j + 1)` runs through.
    south: Vec<Vec<u32>>,
}

impl Grid {
    fn new(boxes: &[Rect]) -> Grid {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for r in boxes {
            let p = r.inflate(MARGIN);
            let c = r.center();
            xs.extend([(p.x, true), (p.right(), true), (c.x, false)]);
            ys.extend([(p.y, true), (p.bottom(), true), (c.y, false)]);
        }
        let (xs, hug_x) = grid_lines(xs);
        let (ys, hug_y) = grid_lines(ys);
        let (nx, ny) = (xs.len(), ys.len());
        let mut grid = Grid {
            cells: vec![Vec::new(); nx * ny],
            east: vec![Vec::new(); nx * ny],
            south: vec![Vec::new(); nx * ny],
            xs,
            ys,
            hug_x,
            hug_y,
        };
        for (b, r) in boxes.iter().enumerate() {
            let p = r.inflate(MARGIN);
            let (x0, x1) = (line_index(&grid.xs, p.x), line_index(&grid.xs, p.right()));
            let (y0, y1) = (line_index(&grid.ys, p.y), line_index(&grid.ys, p.bottom()));
            let b = b as u32;
            for j in y0..=y1 {
                for i in x0..=x1 {
                    let k = j * nx + i;
                    let inside_x = x0 < i && i < x1;
                    let inside_y = y0 < j && j < y1;
                    if inside_x && inside_y {
                        grid.cells[k].push(b);
                    }
                    if i < x1 && inside_y {
                        grid.east[k].push(b);
                    }
                    if j < y1 && inside_x {
                        grid.south[k].push(b);
                    }
                }
            }
        }
        grid
    }

    fn nx(&self) -> usize {
        self.xs.len()
    }

    fn vertex(&self, p: Point) -> usize {
        line_index(&self.ys, p.y) * self.nx() + line_index(&self.xs, p.x)
    }

    fn point(&self, v: usize) -> Point {
        Point::new(self.xs[v % self.nx()], self.ys[v / self.nx()])
    }

    /// The vertex one step from `v` in direction `d`, and the boxes that step runs through.
    fn step(&self, v: usize, d: Dir) -> Option<(usize, &[u32])> {
        let (nx, ny) = (self.nx(), self.ys.len());
        let (i, j) = (v % nx, v / nx);
        match d {
            Dir::East if i + 1 < nx => Some((v + 1, &self.east[v])),
            Dir::West if i > 0 => Some((v - 1, &self.east[v - 1])),
            Dir::South if j + 1 < ny => Some((v + nx, &self.south[v])),
            Dir::North if j > 0 => Some((v - nx, &self.south[v - nx])),
            _ => None,
        }
    }
}

/// An entry of the A* queue, cheapest first; ties go to the lower state, so that the result
/// does not depend on the order things were pushed in.
#[derive(Clone, Copy, PartialEq)]
struct Entry {
    estimate: f32,
    cost: f32,
    state: usize,
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other.estimate.total_cmp(&self.estimate).then_with(|| other.state.cmp(&self.state))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Routes `requests` around `boxes`. A route runs from the border of its source through its
/// corners to the border of its target; consecutive points differ in x or in y only. `None`
/// where there is no route: a box contains the other end, or the ends are shut in.
pub fn route(boxes: &[Rect], requests: &[Request]) -> Vec<Option<Vec<Point>>> {
    let grid = Grid::new(boxes);
    let mut used: HashMap<(usize, usize), u32> = HashMap::new();
    let mut order: Vec<usize> = (0..requests.len()).collect();
    let span = |r: &Request| {
        let (a, b) = (boxes[r.source].center(), boxes[r.target].center());
        (a.x - b.x).abs() + (a.y - b.y).abs()
    };
    order.sort_by(|&a, &b| span(&requests[a]).total_cmp(&span(&requests[b])).then(a.cmp(&b)));
    let mut routes = vec![None; requests.len()];
    let mut sides = vec![None; requests.len()];
    for i in order {
        if let Some((points, vertices, ends)) = shortest(&grid, boxes, requests[i], &used) {
            for pair in vertices.windows(2) {
                *used.entry((pair[0].min(pair[1]), pair[0].max(pair[1]))).or_default() += 1;
            }
            routes[i] = Some(unjog(points, boxes, requests[i]));
            sides[i] = Some(ends);
        }
    }
    nudge(&mut routes, &sides, boxes, requests, true);
    nudge(&mut routes, &sides, boxes, requests, false);
    routes
}

/// A route found on the grid: its points, the grid vertices it passes and the sides it leaves
/// and enters by.
type Found = (Vec<Point>, Vec<usize>, (Side, Side));

/// The cheapest route for `request`.
fn shortest(
    grid: &Grid,
    boxes: &[Rect],
    request: Request,
    used: &HashMap<(usize, usize), u32>,
) -> Option<Found> {
    let (source, target) = (boxes[request.source], boxes[request.target]);
    if request.source == request.target
        || source.contains_rect(&target)
        || target.contains_rect(&source)
    {
        return None;
    }
    // Boxes around either end (packages, components, nodes) may be crossed.
    let passable: Vec<u32> = (0..boxes.len())
        .filter(|&b| b != request.source && b != request.target)
        .filter(|&b| boxes[b].contains_rect(&source) || boxes[b].contains_rect(&target))
        .map(|b| b as u32)
        .collect();
    let blocked = |list: &[u32]| list.iter().any(|b| !passable.contains(b));

    // Facing sides that are shut lift the restriction.
    let anywhere = || shortest(grid, boxes, Request { facing: false, ..request }, used);
    let (source_sides, target_sides) = request.sides(source, target);

    // States are (vertex, direction of arrival); the goal is one state past the target's
    // exits, reached through any of them.
    let goal = grid.cells.len() * 4;
    let exits: Vec<(usize, Side, Point)> = target_sides
        .iter()
        .map(|&side| {
            let (port, exit) = side.port(target);
            (grid.vertex(exit), side, port)
        })
        .filter(|&(v, _, _)| !blocked(&grid.cells[v]))
        .collect();
    if exits.is_empty() {
        return if request.facing { anywhere() } else { None };
    }
    let heuristic = |v: usize| {
        let p = grid.point(v);
        exits
            .iter()
            .map(|&(e, _, _)| {
                let q = grid.point(e);
                (p.x - q.x).abs() + (p.y - q.y).abs()
            })
            .fold(f32::INFINITY, f32::min)
            + MARGIN
    };
    let mut best: HashMap<usize, f32> = HashMap::new();
    let mut came: HashMap<usize, usize> = HashMap::new();
    let mut queue = BinaryHeap::new();
    let mut starts: HashMap<usize, (Side, Point)> = HashMap::new();
    for side in source_sides {
        let (port, exit) = side.port(source);
        let v = grid.vertex(exit);
        if blocked(&grid.cells[v]) {
            continue;
        }
        let state = v * 4 + side.outward().index();
        best.insert(state, MARGIN);
        starts.insert(state, (side, port));
        queue.push(Entry { estimate: MARGIN + heuristic(v), cost: MARGIN, state });
    }
    let mut finish = None;
    while let Some(Entry { cost, state, .. }) = queue.pop() {
        if state >= goal {
            finish = Some(state - goal);
            break;
        }
        if best.get(&state).is_some_and(|&b| cost > b + 1e-3) {
            continue;
        }
        let (v, arrived) = (state / 4, Dir::ALL[state % 4]);
        for &(e, side, _) in &exits {
            if e == v {
                let inward = side.outward().opposite();
                let total = cost + MARGIN + if arrived == inward { 0.0 } else { BEND };
                let done = goal + exits.iter().position(|x| x.0 == e && x.1 == side).unwrap_or(0);
                if best.get(&done).is_none_or(|&b| total < b) {
                    best.insert(done, total);
                    came.insert(done, state);
                    queue.push(Entry { estimate: total, cost: total, state: done });
                }
            }
        }
        for d in Dir::ALL {
            if d == arrived.opposite() {
                continue;
            }
            let Some((w, through)) = grid.step(v, d) else { continue };
            if blocked(through) || blocked(&grid.cells[w]) {
                continue;
            }
            let (a, b) = (grid.point(v), grid.point(w));
            let length = (a.x - b.x).abs() + (a.y - b.y).abs();
            let hugs =
                if d.horizontal() { grid.hug_y[v / grid.nx()] } else { grid.hug_x[v % grid.nx()] };
            let shared = used.get(&(v.min(w), v.max(w))).copied().unwrap_or(0) as f32;
            let step = length * (1.0 + if hugs { HUG } else { 0.0 } + SHARED * shared)
                + if d == arrived { 0.0 } else { BEND };
            let next = w * 4 + d.index();
            let total = cost + step;
            if best.get(&next).is_none_or(|&b| total < b - 1e-3) {
                best.insert(next, total);
                came.insert(next, state);
                queue.push(Entry { estimate: total + heuristic(w), cost: total, state: next });
            }
        }
    }
    let Some(exit) = finish else {
        return if request.facing { anywhere() } else { None };
    };
    let (_, entered, end_port) = exits[exit];
    let mut states = vec![came[&(goal + exit)]];
    while let Some(&previous) = came.get(states.last().expect("not empty")) {
        states.push(previous);
    }
    states.reverse();
    let (left, start_port) = starts[&states[0]];
    let vertices: Vec<usize> = states.iter().map(|s| s / 4).collect();
    let mut points = vec![start_port];
    points.extend(vertices.iter().map(|&v| grid.point(v)));
    points.push(end_port);
    Some((straighten(points), vertices, (left, entered)))
}

/// Longest middle stretch of a Z-shaped route that [`unjog`] straightens.
const JOG: f32 = 40.0;

/// A Z-shaped route with a short middle stretch, between boxes that face each other with an
/// overlap, made a straight line where one fits through the overlap.
fn unjog(points: Vec<Point>, boxes: &[Rect], request: Request) -> Vec<Point> {
    let [a, b, c, d] = points[..] else { return points };
    let vertical = (a.x - b.x).abs() < SAME && (c.x - d.x).abs() < SAME;
    let horizontal = (a.y - b.y).abs() < SAME && (c.y - d.y).abs() < SAME;
    let (source, target) = (boxes[request.source], boxes[request.target]);
    let (lo, hi, at, jog) = if vertical {
        (source.x.max(target.x), source.right().min(target.right()), (a.x + d.x) / 2.0, b.x - c.x)
    } else if horizontal {
        (source.y.max(target.y), source.bottom().min(target.bottom()), (a.y + d.y) / 2.0, b.y - c.y)
    } else {
        return points;
    };
    let (lo, hi) = (lo + PORT_INSET, hi - PORT_INSET);
    if jog.abs() > JOG || lo > hi {
        return points;
    }
    let at = at.clamp(lo, hi);
    let (from, to) = if vertical {
        (Point::new(at, a.y), Point::new(at, d.y))
    } else {
        (Point::new(a.x, at), Point::new(d.x, at))
    };
    // Boxes around an end are crossed anyway.
    let free = boxes.iter().enumerate().all(|(i, r)| {
        i == request.source
            || i == request.target
            || r.contains_rect(&source)
            || r.contains_rect(&target)
            || !segment_hits(from, to, r.inflate(MARGIN / 2.0 - 1.0))
    });
    if free { vec![from, to] } else { points }
}

/// `points` without those that do not turn the line.
fn straighten(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_some_and(|q| (q.x - p.x).abs() < SAME && (q.y - p.y).abs() < SAME) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let vertical = (a.x - b.x).abs() < SAME && (b.x - p.x).abs() < SAME;
            let horizontal = (a.y - b.y).abs() < SAME && (b.y - p.y).abs() < SAME;
            if vertical || horizontal {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// A stretch of a route: segment `k` of route `line`, at `level` (its y if horizontal, its x
/// if vertical) from `from` to `to` along it.
#[derive(Clone, Copy, Debug)]
struct Segment {
    line: usize,
    k: usize,
    level: f32,
    from: f32,
    to: f32,
}

/// Spreads apart horizontal (or vertical) segments that lie on top of each other.
fn nudge(
    routes: &mut [Option<Vec<Point>>],
    sides: &[Option<(Side, Side)>],
    boxes: &[Rect],
    requests: &[Request],
    horizontal: bool,
) {
    // Along and across the segments.
    let along = |p: Point| if horizontal { p.x } else { p.y };
    let across = |p: Point| if horizontal { p.y } else { p.x };
    let mut segments = Vec::new();
    for (line, route) in routes.iter().enumerate() {
        let Some(points) = route else { continue };
        for k in 0..points.len().saturating_sub(1) {
            let (a, b) = (points[k], points[k + 1]);
            if (across(a) - across(b)).abs() < SAME && (along(a) - along(b)).abs() >= SAME {
                let (from, to) = (along(a).min(along(b)), along(a).max(along(b)));
                segments.push(Segment { line, k, level: across(a), from, to });
            }
        }
    }
    segments.sort_by(|a, b| a.level.total_cmp(&b.level).then(a.from.total_cmp(&b.from)));
    let mut groups: Vec<Vec<Segment>> = Vec::new();
    let mut reach = f32::NEG_INFINITY;
    for s in segments {
        match groups.last_mut() {
            Some(group) if (group[0].level - s.level).abs() < SAME && s.from < reach - 1.0 => {
                reach = reach.max(s.to);
                group.push(s);
            }
            _ => {
                reach = s.to;
                groups.push(vec![s]);
            }
        }
    }
    for mut group in groups.into_iter().filter(|g| g.len() > 1) {
        // Lines that turn toward lower levels at their ends go first, so that they do not
        // cross the others where they leave the shared stretch.
        let turn = |s: &Segment| {
            let points = routes[s.line].as_ref().expect("grouped routes exist");
            let mut sum = 0.0;
            if s.k > 0 {
                sum += (across(points[s.k - 1]) - s.level).signum();
            }
            if s.k + 2 < points.len() {
                sum += (across(points[s.k + 2]) - s.level).signum();
            }
            sum
        };
        group.sort_by(|a, b| turn(a).total_cmp(&turn(b)).then(a.line.cmp(&b.line)));
        let level = group[0].level;
        let (from, to) = group
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(f, t), s| (f.min(s.from), t.max(s.to)));
        // The free space on either side: up to the nearest box that the stretch passes.
        let (mut low, mut high) = (f32::NEG_INFINITY, f32::INFINITY);
        for r in boxes {
            let (r_from, r_to) = if horizontal { (r.x, r.right()) } else { (r.y, r.bottom()) };
            let (r_low, r_high) = if horizontal { (r.y, r.bottom()) } else { (r.x, r.right()) };
            if r_to <= from + SAME || r_from >= to - SAME {
                continue;
            }
            if r_high <= level + SAME {
                low = low.max(r_high + MARGIN / 2.0);
            } else if r_low >= level - SAME {
                high = high.min(r_low - MARGIN / 2.0);
            }
        }
        let n = group.len() as f32;
        let mut spacing = SPACING;
        let mut start = level - (n - 1.0) * spacing / 2.0;
        if high - low >= (n - 1.0) * spacing {
            start = start.clamp(low, high - (n - 1.0) * spacing);
        } else if high > low {
            spacing = (high - low) / (n - 1.0);
            start = low;
        }
        for (i, s) in group.iter().enumerate() {
            let mut target = start + i as f32 * spacing;
            let points = routes[s.line].as_mut().expect("grouped routes exist");
            let last = points.len() - 2;
            // A stretch that starts or ends at a port moves the port along its side.
            let (Some((left, entered)), request) = (sides[s.line], requests[s.line]) else {
                continue;
            };
            for (at_end, side, rect) in [
                (s.k == 0, left, boxes[request.source]),
                (s.k == last, entered, boxes[request.target]),
            ] {
                if at_end && matches!(side, Side::Left | Side::Right) == horizontal {
                    let (lo, hi) =
                        if horizontal { (rect.y, rect.bottom()) } else { (rect.x, rect.right()) };
                    // Sides too short for the inset, like a fork's, keep the port in the middle.
                    target = if hi - lo > 2.0 * PORT_INSET {
                        target.clamp(lo + PORT_INSET, hi - PORT_INSET)
                    } else {
                        (lo + hi) / 2.0
                    };
                }
            }
            for p in [s.k, s.k + 1] {
                if horizontal {
                    points[p].y = target;
                } else {
                    points[p].x = target;
                }
            }
        }
    }
}

/// Routes lines just drawn or brought along, like [`route_lines`], except on use case diagrams,
/// whose lines are drawn straight, and on composite structure diagrams, whose connectors start
/// at ports half inside a class; Route lines still routes them on request.
pub fn route_new_lines(tx: &mut Tx<'_>, diagram: Id, edges: &[Id]) -> Result<usize> {
    if matches!(
        tx.model().element(diagram)?.kind().as_diagram(),
        Some(DiagramKind::UseCase | DiagramKind::CompositeStructure)
    ) {
        return Ok(0);
    }
    route_lines(tx, diagram, edges)
}

/// Routes the edges `edges` of `diagram` around its nodes and makes them right-angled lines
/// that the router keeps (`Style::auto_route`). Edges from a node to itself, edges to or from
/// another edge, edges whose node contains the other end and those without a way through stay
/// as they are, as does everything on sequence diagrams. Returns how many were routed.
pub fn route_lines(tx: &mut Tx<'_>, diagram: Id, edges: &[Id]) -> Result<usize> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    if model.element(diagram)?.kind().as_diagram() == Some(DiagramKind::Sequence) {
        return Ok(0);
    }
    let mut boxes = Vec::new();
    let mut index: HashMap<Id, usize> = HashMap::new();
    for p in view.presentations() {
        let Shape::Node { bounds, .. } = p.shape else { continue };
        let kind = p.element.and_then(|e| model.get(e)).map(|e| e.kind());
        // Lines cross swimlanes freely.
        if matches!(kind, Some(ElementKind::Partition)) {
            continue;
        }
        let lollipop = p.style.iconic && matches!(kind, Some(ElementKind::Interface(_)));
        let rect = if lollipop { Rect::new(bounds.x, bounds.y, 24.0, 24.0) } else { bounds };
        index.insert(p.id, boxes.len());
        boxes.push(rect);
    }
    let mut requests = Vec::new();
    let mut routed = Vec::new();
    for &edge in edges {
        let Some(p) = view.get(edge) else { continue };
        let Shape::Edge { source, target, .. } = &p.shape else { continue };
        if let (Some(&s), Some(&t)) = (index.get(source), index.get(target))
            && s != t
        {
            let facing = p.element.and_then(|e| model.get(e)).is_some_and(|e| {
                matches!(e.kind(), ElementKind::Generalization(_) | ElementKind::Realization(_))
            });
            requests.push(Request { source: s, target: t, facing });
            routed.push(edge);
        }
    }
    let routes = route(&boxes, &requests);
    let relative = |r: Rect, p: Point| {
        let fraction = |v: f32, start: f32, size: f32| {
            if size > 0.0 { ((v - start) / size).clamp(0.0, 1.0) } else { 0.5 }
        };
        Point::new(fraction(p.x, r.x, r.width), fraction(p.y, r.y, r.height))
    };
    let mut count = 0;
    for ((edge, request), points) in routed.into_iter().zip(requests).zip(routes) {
        let Some(points) = points.filter(|p| p.len() >= 2) else { continue };
        let anchors = (
            Some(relative(boxes[request.source], points[0])),
            Some(relative(boxes[request.target], points[points.len() - 1])),
        );
        tx.set_route(diagram, edge, points[1..points.len() - 1].to_vec(), anchors)?;
        tx.update_presentation(diagram, edge, |p| {
            p.style.line_shape = LineShape::RightAngle;
            p.style.auto_route = true;
        })?;
        count += 1;
    }
    Ok(count)
}

/// The edges of `diagram` that the router keeps and that need a new route after an edit:
/// new ones, those whose end nodes moved or changed size, and those that now run through a
/// node that moved or appeared. `before` holds the node bounds and the routed edges of the
/// diagram before the edit (see [`snapshot`]).
pub fn stale(tx: &Tx<'_>, diagram: Id, before: &Snapshot) -> Vec<Id> {
    let Some(view) = tx.model().view(diagram) else { return Vec::new() };
    let now = snapshot(tx.model(), diagram);
    let changed: Vec<(Id, Rect)> = now
        .nodes
        .iter()
        .filter(|(id, r)| before.nodes.get(id) != Some(r))
        .map(|(id, r)| (*id, *r))
        .collect();
    if changed.is_empty() && now.edges.iter().all(|e| before.edges.contains(e)) {
        return Vec::new();
    }
    let moved = |id: &Id| changed.iter().any(|(c, _)| c == id);
    view.presentations()
        .iter()
        .filter(|p| p.style.auto_route)
        .filter_map(|p| match &p.shape {
            Shape::Edge { source, target, waypoints, .. } => {
                let new = !before.edges.contains(&p.id);
                let ends = moved(source) || moved(target);
                // A node that moved onto the line, judged by its corners.
                let crossed = changed.iter().any(|(c, r)| {
                    c != source && c != target && {
                        let r = r.inflate(MARGIN / 2.0);
                        waypoints.windows(2).any(|w| segment_hits(w[0], w[1], r))
                    }
                });
                (new || ends || crossed).then_some(p.id)
            }
            _ => None,
        })
        .collect()
}

/// [`snapshot`]s of every diagram, to pass to [`follow`] after an edit.
pub fn snapshots(model: &umlcraft_model::Model) -> HashMap<Id, Snapshot> {
    model.views().map(|(diagram, _)| (diagram, snapshot(model, diagram))).collect()
}

/// Routes again what an edit made [`stale`] on the diagrams of `before`. Returns how many
/// lines were routed.
pub fn follow(tx: &mut Tx<'_>, before: &HashMap<Id, Snapshot>) -> Result<usize> {
    let mut count = 0;
    let mut diagrams: Vec<&Id> = before.keys().collect();
    diagrams.sort();
    for &diagram in diagrams {
        let edges = stale(tx, diagram, &before[&diagram]);
        if !edges.is_empty() {
            count += route_lines(tx, diagram, &edges)?;
        }
    }
    Ok(count)
}

/// Whether the horizontal or vertical segment from `a` to `b` runs through `r`.
fn segment_hits(a: Point, b: Point, r: Rect) -> bool {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    x0 < r.right() && x1 > r.x && y0 < r.bottom() && y1 > r.y
}

/// The node bounds and the routed edges of a diagram, to tell afterwards what an edit changed.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    nodes: HashMap<Id, Rect>,
    edges: Vec<Id>,
}

pub fn snapshot(model: &umlcraft_model::Model, diagram: Id) -> Snapshot {
    let Some(view) = model.view(diagram) else { return Snapshot::default() };
    let mut snapshot = Snapshot::default();
    for p in view.presentations() {
        match &p.shape {
            Shape::Node { bounds, .. } => {
                snapshot.nodes.insert(p.id, *bounds);
            }
            Shape::Edge { .. } if p.style.auto_route => snapshot.edges.push(p.id),
            _ => {}
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_lines_half_a_unit_apart_are_told_apart() {
        assert_eq!(line_index(&[72.5, 73.0], 73.0), 1);
        assert_eq!(line_index(&[72.5, 73.0], 72.5), 0);
        // The margin of one box runs at 72.5, the middle of the other at 73.
        let boxes = [Rect::new(84.5, 0.0, 40.0, 30.0), Rect::new(63.0, 100.0, 20.0, 30.0)];
        let routes = route(&boxes, &[Request { source: 0, target: 1, facing: false }]);
        assert!(routes[0].is_some());
    }

    /// Whether every segment is horizontal or vertical and stays outside every box but the
    /// ends (by at least half the margin).
    fn clear(points: &[Point], boxes: &[Rect], ends: [usize; 2]) -> bool {
        points.windows(2).all(|w| {
            let square = (w[0].x - w[1].x).abs() < SAME || (w[0].y - w[1].y).abs() < SAME;
            let free = boxes.iter().enumerate().all(|(i, r)| {
                ends.contains(&i) || !segment_hits(w[0], w[1], r.inflate(MARGIN / 2.0 - 1.0))
            });
            square && free
        })
    }

    #[test]
    fn a_line_goes_around_a_box_in_its_way() {
        let boxes = [
            Rect::new(0.0, 0.0, 100.0, 60.0),
            Rect::new(400.0, 0.0, 100.0, 60.0),
            Rect::new(200.0, -40.0, 100.0, 140.0),
        ];
        let routes = route(&boxes, &[Request { source: 0, target: 1, facing: false }]);
        let points = routes[0].as_ref().expect("routed");
        assert!(clear(points, &boxes, [0, 1]), "{points:?}");
        // Below the box, which reaches less far down than up: out of the bottom, across and
        // into the bottom of the target, with two bends.
        assert_eq!(
            points,
            &[
                Point::new(50.0, 60.0),
                Point::new(50.0, 112.0),
                Point::new(450.0, 112.0),
                Point::new(450.0, 60.0)
            ]
        );
    }

    #[test]
    fn aligned_boxes_get_a_straight_line() {
        let boxes = [Rect::new(0.0, 0.0, 100.0, 60.0), Rect::new(0.0, 200.0, 100.0, 60.0)];
        let routes = route(&boxes, &[Request { source: 0, target: 1, facing: false }]);
        assert_eq!(
            routes[0].as_deref(),
            Some(&[Point::new(50.0, 60.0), Point::new(50.0, 200.0)][..])
        );
    }

    #[test]
    fn nearly_aligned_boxes_get_a_straight_line_too() {
        // Inside a container, like a node in a deployment's cloud.
        let boxes = [
            Rect::new(0.0, 0.0, 100.0, 60.0),
            Rect::new(-20.0, 160.0, 400.0, 200.0),
            Rect::new(6.0, 200.0, 100.0, 60.0),
        ];
        let routes = route(&boxes, &[Request { source: 0, target: 2, facing: false }]);
        assert_eq!(
            routes[0].as_deref(),
            Some(&[Point::new(53.0, 60.0), Point::new(53.0, 200.0)][..])
        );
    }

    #[test]
    fn lines_on_one_stretch_are_spread_apart() {
        let boxes = [
            Rect::new(0.0, 0.0, 100.0, 60.0),
            Rect::new(0.0, 300.0, 100.0, 60.0),
            Rect::new(300.0, 300.0, 100.0, 60.0),
        ];
        let requests = [
            Request { source: 0, target: 1, facing: false },
            Request { source: 0, target: 2, facing: false },
        ];
        let routes = route(&boxes, &requests);
        let (a, b) = (routes[0].as_ref().unwrap(), routes[1].as_ref().unwrap());
        assert!(clear(a, &boxes, [0, 1]) && clear(b, &boxes, [0, 2]));
        // They leave the box through different points.
        assert!(a[0].distance(b[0]) >= SPACING - 0.1, "{a:?} {b:?}");
    }

    #[test]
    fn a_line_out_of_a_container_crosses_its_border() {
        let boxes = [
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(40.0, 60.0, 100.0, 60.0),
            Rect::new(500.0, 60.0, 100.0, 60.0),
        ];
        let routes = route(&boxes, &[Request { source: 1, target: 2, facing: false }]);
        assert_eq!(
            routes[0].as_deref(),
            Some(&[Point::new(140.0, 90.0), Point::new(500.0, 90.0)][..]),
            "straight out to the right"
        );
    }

    #[test]
    fn nested_boxes_are_not_routed() {
        let boxes = [Rect::new(0.0, 0.0, 300.0, 200.0), Rect::new(40.0, 60.0, 100.0, 60.0)];
        assert_eq!(route(&boxes, &[Request { source: 1, target: 0, facing: false }]), vec![None]);
    }
}
