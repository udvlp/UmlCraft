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
use std::collections::{BinaryHeap, HashMap, HashSet};

use umlcraft_model::{
    DiagramKind, ElementKind, Id, LineShape, ModelError, Point, Rect, Result, Shape, Tx,
};

/// Distance a route keeps from the nodes it passes.
pub const MARGIN: f32 = 12.0;
/// Distance between lines that share a stretch, where there is room.
const SPACING: f32 = 8.0;
/// The same for stretches that start or end at a port, where the lines' labels go.
const PORT_SPACING: f32 = 20.0;
/// What a bend costs, as a length.
const BEND: f32 = 40.0;
/// Extra cost per unit of length of a stretch that an earlier route already uses.
const SHARED: f32 = 0.6;
/// Extra cost per unit of length for running along a node's margin.
const HUG: f32 = 0.05;
/// How close to a corner of its node a port may move.
const PORT_INSET: f32 = 4.0;
/// How close to a corner a port may slide to make a line straighter (at most a quarter of
/// the side), and what that costs per unit of length away from the middle: much less than
/// the length it saves, so that a port leaves the middle for a straight line through a
/// narrow overlap of two nodes, and to spread the ends of lines on a crowded side.
const SLIDE_INSET: f32 = 12.0;
const OFF_MIDDLE: f32 = 0.2;
/// What a port costs at most for lying close to a port an earlier line took on the same side,
/// and from how far it costs nothing: lines spread their ends where they can, so that their
/// labels find room.
const CROWD: f32 = 40.0;
const CROWD_RANGE: f32 = 40.0;
/// Coordinates closer than this are the same grid line.
const SAME: f32 = 0.5;
/// Distance a route keeps from texts drawn outside nodes, such as a pin's name, where it can.
const TEXT_MARGIN: f32 = 4.0;
/// Extra cost per unit of length of a stretch through such a text: routes may cross texts,
/// but go around them where that is not much longer.
const THROUGH_TEXT: f32 = 6.0;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    /// The ports this side of `r` offers on `grid`: where each grid line across it meets it,
    /// clear of the corners, with the point `MARGIN` outside it and how far it lies from the
    /// middle. The middle is always one of them.
    fn ports(self, r: Rect, grid: &Grid) -> Vec<(Point, Point, f32)> {
        let (port, exit) = self.port(r);
        let vertical = matches!(self, Side::Top | Side::Bottom);
        let (lo, hi, middle, lines) = if vertical {
            (r.x, r.right(), port.x, &grid.xs)
        } else {
            (r.y, r.bottom(), port.y, &grid.ys)
        };
        let inset = SLIDE_INSET.min((hi - lo) / 4.0);
        let mut out = vec![(port, exit, 0.0)];
        for &at in lines.iter().filter(|&&v| v >= lo + inset && v <= hi - inset) {
            if (at - middle).abs() < SAME {
                continue;
            }
            let moved = |p: Point| if vertical { Point::new(at, p.y) } else { Point::new(p.x, at) };
            out.push((moved(port), moved(exit), (at - middle).abs()));
        }
        out
    }

    /// Where `p` lies along this side.
    fn along(self, p: Point) -> f32 {
        if matches!(self, Side::Top | Side::Bottom) { p.x } else { p.y }
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
    /// Whether the stretch from vertex `(i, j)` to `(i + 1, j)` runs through a text.
    text_east: Vec<bool>,
    /// Whether the stretch from vertex `(i, j)` to `(i, j + 1)` runs through a text.
    text_south: Vec<bool>,
    /// Whether a vertex lies in a text (with its margin).
    text_cells: Vec<bool>,
}

impl Grid {
    fn new(boxes: &[Rect], texts: &[Rect], taken: &[Point], requests: &[Request]) -> Grid {
        let mut xs = Vec::new();
        let mut ys = Vec::new();
        for r in boxes {
            let p = r.inflate(MARGIN);
            let c = r.center();
            xs.extend([(p.x, true), (p.right(), true), (c.x, false)]);
            ys.extend([(p.y, true), (p.bottom(), true), (c.y, false)]);
        }
        // Through the middle of where the two ends of a line overlap, however narrow, so that
        // a straight line can run there.
        for r in requests {
            let (a, b) = (boxes[r.source], boxes[r.target]);
            let (x0, x1) = (a.x.max(b.x), a.right().min(b.right()));
            let (y0, y1) = (a.y.max(b.y), a.bottom().min(b.bottom()));
            if x1 > x0 {
                xs.push(((x0 + x1) / 2.0, false));
            }
            if y1 > y0 {
                ys.push(((y0 + y1) / 2.0, false));
            }
        }
        for r in texts {
            let p = r.inflate(TEXT_MARGIN);
            xs.extend([(p.x, false), (p.right(), false)]);
            ys.extend([(p.y, false), (p.bottom(), false)]);
        }
        // Beside the ports of lines that stay, where a line routed now can have its own.
        for p in taken {
            xs.extend([(p.x - PORT_SPACING, false), (p.x + PORT_SPACING, false)]);
            ys.extend([(p.y - PORT_SPACING, false), (p.y + PORT_SPACING, false)]);
        }
        let (xs, hug_x) = grid_lines(xs);
        let (ys, hug_y) = grid_lines(ys);
        let (nx, ny) = (xs.len(), ys.len());
        let mut grid = Grid {
            cells: vec![Vec::new(); nx * ny],
            east: vec![Vec::new(); nx * ny],
            south: vec![Vec::new(); nx * ny],
            text_east: vec![false; nx * ny],
            text_south: vec![false; nx * ny],
            text_cells: vec![false; nx * ny],
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
        for r in texts {
            let p = r.inflate(TEXT_MARGIN);
            let (x0, x1) = (line_index(&grid.xs, p.x), line_index(&grid.xs, p.right()));
            let (y0, y1) = (line_index(&grid.ys, p.y), line_index(&grid.ys, p.bottom()));
            for j in y0..=y1 {
                for i in x0..=x1 {
                    let k = j * nx + i;
                    if i < x1 && y0 < j && j < y1 {
                        grid.text_east[k] = true;
                    }
                    if j < y1 && x0 < i && i < x1 {
                        grid.text_south[k] = true;
                    }
                    if x0 < i && i < x1 && y0 < j && j < y1 {
                        grid.text_cells[k] = true;
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

    /// The vertex one step from `v` in direction `d`, the boxes that step runs through and
    /// whether it runs through a text.
    fn step(&self, v: usize, d: Dir) -> Option<(usize, &[u32], bool)> {
        let (nx, ny) = (self.nx(), self.ys.len());
        let (i, j) = (v % nx, v / nx);
        match d {
            Dir::East if i + 1 < nx => Some((v + 1, &self.east[v], self.text_east[v])),
            Dir::West if i > 0 => Some((v - 1, &self.east[v - 1], self.text_east[v - 1])),
            Dir::South if j + 1 < ny => Some((v + nx, &self.south[v], self.text_south[v])),
            Dir::North if j > 0 => Some((v - nx, &self.south[v - nx], self.text_south[v - nx])),
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
    route_around(boxes, &[], &[], &[], requests)
}

/// Routes `requests` around `boxes` as [`route`] does, keeping off `texts` (drawn outside
/// nodes) and the `loops` (lines from a box to itself, by box and waypoints) where a way
/// around them is not much longer. The ends of the loops count as ports already taken, as do
/// `taken`: where lines that stay as they are meet a box, by box and point on its border.
pub fn route_around(
    boxes: &[Rect],
    texts: &[Rect],
    loops: &[(usize, Vec<Point>)],
    taken: &[(usize, Point)],
    requests: &[Request],
) -> Vec<Option<Vec<Point>>> {
    let mut ports: Ports = HashMap::new();
    let mut beside = Vec::new();
    for &(b, p) in taken {
        let (side, port) = on_side(boxes[b], p);
        ports.entry((b, side)).or_default().push(side.along(port));
        beside.push(port);
    }
    let mut texts = texts.to_vec();
    for (b, waypoints) in loops {
        let (Some(&first), Some(&last)) = (waypoints.first(), waypoints.last()) else { continue };
        let r = boxes[*b];
        let mut area = Rect::from_corners(first, first);
        for p in waypoints.iter().copied().chain([leaving(r, first).1, leaving(r, last).1]) {
            area = area.union(&Rect::from_corners(p, p));
        }
        texts.push(area);
        for p in [first, last] {
            let (side, port) = leaving(r, p);
            ports.entry((*b, side)).or_default().push(side.along(port));
        }
    }
    let grid = Grid::new(boxes, &texts, &beside, requests);
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
        let request = requests[i];
        if let Some((points, vertices, ends)) = shortest(&grid, boxes, request, &used, &ports) {
            for pair in vertices.windows(2) {
                *used.entry((pair[0].min(pair[1]), pair[0].max(pair[1]))).or_default() += 1;
            }
            let points = unjog(points, boxes, request);
            for (b, side, p) in [
                (request.source, ends.0, points[0]),
                (request.target, ends.1, points[points.len() - 1]),
            ] {
                ports.entry((b, side)).or_default().push(side.along(p));
            }
            routes[i] = Some(points);
            sides[i] = Some(ends);
        }
    }
    nudge(&mut routes, &sides, boxes, requests, true);
    nudge(&mut routes, &sides, boxes, requests, false);
    routes
}

/// The side of `r` a line to `p` outside it leaves by, and about where: across from `p`, or
/// at the end of the side nearest to it.
fn leaving(r: Rect, p: Point) -> (Side, Point) {
    let (x, y) = (p.x.clamp(r.x, r.right()), p.y.clamp(r.y, r.bottom()));
    match () {
        _ if p.x > r.right() => (Side::Right, Point::new(r.right(), y)),
        _ if p.x < r.x => (Side::Left, Point::new(r.x, y)),
        _ if p.y < r.y => (Side::Top, Point::new(x, r.y)),
        _ => (Side::Bottom, Point::new(x, r.bottom())),
    }
}

/// The side of `r` nearest to `p`, a point on or near its border, and `p` moved onto it.
fn on_side(r: Rect, p: Point) -> (Side, Point) {
    let (x, y) = (p.x.clamp(r.x, r.right()), p.y.clamp(r.y, r.bottom()));
    [
        (Side::Left, (p.x - r.x).abs(), Point::new(r.x, y)),
        (Side::Right, (p.x - r.right()).abs(), Point::new(r.right(), y)),
        (Side::Top, (p.y - r.y).abs(), Point::new(x, r.y)),
        (Side::Bottom, (p.y - r.bottom()).abs(), Point::new(x, r.bottom())),
    ]
    .into_iter()
    .min_by(|a, b| a.1.total_cmp(&b.1))
    .map(|(side, _, port)| (side, port))
    .expect("four sides")
}

/// A route found on the grid: its points, the grid vertices it passes and the sides it leaves
/// and enters by.
type Found = (Vec<Point>, Vec<usize>, (Side, Side));

/// The ports earlier lines took, by box and side, as their place along the side.
type Ports = HashMap<(usize, Side), Vec<f32>>;

/// What a port at `at` along side `side` of box `b` costs for the ports already taken there.
fn crowd(ports: &Ports, b: usize, side: Side, at: f32) -> f32 {
    ports.get(&(b, side)).map_or(0.0, |taken| {
        taken.iter().map(|&p| CROWD * (1.0 - (at - p).abs() / CROWD_RANGE).max(0.0)).sum()
    })
}

/// The cheapest route for `request`.
fn shortest(
    grid: &Grid,
    boxes: &[Rect],
    request: Request,
    used: &HashMap<(usize, usize), u32>,
    ports: &Ports,
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
    let anywhere = || shortest(grid, boxes, Request { facing: false, ..request }, used, ports);
    let (source_sides, target_sides) = request.sides(source, target);

    // States are (vertex, direction of arrival); the goal is one state past the target's
    // exits, reached through any of them.
    let goal = grid.cells.len() * 4;
    // Each with what its distance from the middle of its side costs.
    let exits: Vec<(usize, Side, Point, f32)> = target_sides
        .iter()
        .flat_map(|&side| {
            side.ports(target, grid).into_iter().map(move |(port, exit, off)| {
                let cost = off * OFF_MIDDLE + crowd(ports, request.target, side, side.along(port));
                (grid.vertex(exit), side, port, cost)
            })
        })
        .filter(|&(v, _, _, _)| !blocked(&grid.cells[v]))
        .collect();
    // Leaving or entering a node through a text costs as running through it.
    let stub = |v: usize| MARGIN * if grid.text_cells[v] { 1.0 + THROUGH_TEXT } else { 1.0 };
    if exits.is_empty() {
        return if request.facing { anywhere() } else { None };
    }
    let heuristic = |v: usize| {
        let p = grid.point(v);
        exits
            .iter()
            .map(|&(e, _, _, _)| {
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
        for (port, exit, off) in side.ports(source, grid) {
            let v = grid.vertex(exit);
            if blocked(&grid.cells[v]) {
                continue;
            }
            let state = v * 4 + side.outward().index();
            let cost =
                stub(v) + off * OFF_MIDDLE + crowd(ports, request.source, side, side.along(port));
            if best.get(&state).is_some_and(|&b| b <= cost) {
                continue;
            }
            best.insert(state, cost);
            starts.insert(state, (side, port));
            queue.push(Entry { estimate: cost + heuristic(v), cost, state });
        }
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
        for (k, &(e, side, _, off)) in exits.iter().enumerate() {
            if e == v {
                let inward = side.outward().opposite();
                let total = cost + stub(e) + off + if arrived == inward { 0.0 } else { BEND };
                let done = goal + k;
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
            let Some((w, through, text)) = grid.step(v, d) else { continue };
            if blocked(through) || blocked(&grid.cells[w]) {
                continue;
            }
            let (a, b) = (grid.point(v), grid.point(w));
            let length = (a.x - b.x).abs() + (a.y - b.y).abs();
            let hugs =
                if d.horizontal() { grid.hug_y[v / grid.nx()] } else { grid.hug_x[v % grid.nx()] };
            let shared = used.get(&(v.min(w), v.max(w))).copied().unwrap_or(0) as f32;
            let step = length
                * (1.0
                    + if hugs { HUG } else { 0.0 }
                    + SHARED * shared
                    + if text { THROUGH_TEXT } else { 0.0 })
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
    let (_, entered, end_port, _) = exits[exit];
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
/// overlap, made a straight line where one fits through the overlap, halfway between the boxes'
/// middles where it can.
fn unjog(points: Vec<Point>, boxes: &[Rect], request: Request) -> Vec<Point> {
    let [a, b, c, d] = points[..] else { return points };
    let vertical = (a.x - b.x).abs() < SAME && (c.x - d.x).abs() < SAME;
    let horizontal = (a.y - b.y).abs() < SAME && (c.y - d.y).abs() < SAME;
    let (source, target) = (boxes[request.source], boxes[request.target]);
    let (s, t) = (source.center(), target.center());
    let (lo, hi, at, jog) = if vertical {
        (source.x.max(target.x), source.right().min(target.right()), (s.x + t.x) / 2.0, b.x - c.x)
    } else if horizontal {
        (source.y.max(target.y), source.bottom().min(target.bottom()), (s.y + t.y) / 2.0, b.y - c.y)
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
    // Whether a segment leaves or enters a node through the side it would move its port along.
    let at_port = |s: &Segment| {
        let (Some(points), Some((left, entered))) = (&routes[s.line], sides[s.line]) else {
            return false;
        };
        let along_side = |side: Side| matches!(side, Side::Left | Side::Right) == horizontal;
        (s.k == 0 && along_side(left)) || (s.k + 2 == points.len() && along_side(entered))
    };
    segments.sort_by(|a, b| a.level.total_cmp(&b.level).then(a.from.total_cmp(&b.from)));
    // Segments closer than their spacing along a common stretch are one group; at ports the
    // spacing is wider, so that the labels at the ends fit between the lines.
    let mut groups: Vec<(Vec<Segment>, bool, f32, f32)> = Vec::new();
    for s in segments {
        let port = at_port(&s);
        let close = |(group, ports, from, to): &(Vec<Segment>, bool, f32, f32)| {
            let last = group.last().expect("groups are not empty").level;
            let gap = if port && *ports { PORT_SPACING } else { SPACING };
            s.level - last < gap - SAME && s.from < to - 1.0 && s.to > from + 1.0
        };
        match groups.iter_mut().rev().take(8).find(|g| close(g)) {
            Some((group, ports, from, to)) => {
                (*ports, *from, *to) = (*ports || port, from.min(s.from), to.max(s.to));
                group.push(s);
            }
            None => groups.push((vec![s], port, s.from, s.to)),
        }
    }
    for (mut group, ports, _, _) in groups.into_iter().filter(|g| g.0.len() > 1) {
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
        group.sort_by(|a, b| {
            turn(a).total_cmp(&turn(b)).then(a.level.total_cmp(&b.level)).then(a.line.cmp(&b.line))
        });
        let level = group.iter().map(|s| s.level).sum::<f32>() / group.len() as f32;
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
        let mut spacing = if ports { PORT_SPACING } else { SPACING };
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

/// Routes lines just drawn or brought along, like [`route_lines`], except on use case and ER diagrams,
/// whose lines are drawn straight, and on composite structure diagrams, whose connectors start
/// at ports half inside a class; Route lines still routes them on request.
pub fn route_new_lines(tx: &mut Tx<'_>, diagram: Id, edges: &[Id]) -> Result<usize> {
    if matches!(
        tx.model().element(diagram)?.kind().as_diagram(),
        Some(
            DiagramKind::UseCase
                | DiagramKind::CompositeStructure
                | DiagramKind::EntityRelationship
        )
    ) {
        return Ok(0);
    }
    let edges = with_neighbours(tx.model(), diagram, edges);
    route_lines(tx, diagram, &edges)
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
        // Shared generalizations run over their common line (`crate::shared`).
        if !p.style.shared.is_separate() {
            continue;
        }
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
    // On activity diagrams lines keep off the texts outside nodes (pin names, object node
    // properties); widths as estimated, which is close enough to steer around them.
    let texts: Vec<Rect> =
        if model.element(diagram)?.kind().as_diagram() == Some(DiagramKind::Activity) {
            let scene = crate::scene::build(
                model,
                diagram,
                &crate::scene::Preview::default(),
                &crate::scene::ApproxMeasure,
            );
            scene.nodes.iter().flat_map(|n| n.outside()).collect()
        } else {
            Vec::new()
        };
    // Lines keep clear of the loops of nodes to themselves, with their corners as drawn.
    let loops: Vec<(usize, Vec<Point>)> = view
        .presentations()
        .iter()
        .filter_map(|p| match &p.shape {
            Shape::Edge { source, target, waypoints, source_anchor, target_anchor, .. }
                if source == target =>
            {
                let &b = index.get(source)?;
                let r = boxes[b];
                let waypoints = if waypoints.is_empty() {
                    crate::scene::self_loop(r)
                } else {
                    waypoints.clone()
                };
                let corners = if p.style.line_shape == LineShape::RightAngle {
                    let from = crate::route::anchor_point(r, *source_anchor);
                    let to = crate::route::anchor_point(r, *target_anchor);
                    crate::route::orthogonalize(r, from, &waypoints, r, to)
                } else {
                    waypoints
                };
                Some((b, corners))
            }
            _ => None,
        })
        .collect();
    // Lines that stay as they are keep their ports: a line routed now takes others.
    let routing: HashSet<Id> = routed.iter().copied().collect();
    let taken: Vec<(usize, Point)> = view
        .presentations()
        .iter()
        .filter(|p| !routing.contains(&p.id))
        .flat_map(|p| match &p.shape {
            Shape::Edge { source, target, source_anchor, target_anchor, .. }
                if source != target =>
            {
                vec![(*source, *source_anchor), (*target, *target_anchor)]
            }
            _ => Vec::new(),
        })
        .filter_map(|(node, anchor)| {
            let (&b, anchor) = (index.get(&node)?, anchor?);
            Some((b, crate::route::anchor_point(boxes[b], Some(anchor))))
        })
        .collect();
    let routes = route_around(&boxes, &texts, &loops, &taken, &requests);
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
        follow_loops(tx, diagram, &before[&diagram])?;
        let edges = stale(tx, diagram, &before[&diagram]);
        if !edges.is_empty() {
            let edges = with_neighbours(tx.model(), diagram, &edges);
            count += route_lines(tx, diagram, &edges)?;
        }
    }
    Ok(count)
}

/// `edges` and the lines the router keeps between the same two nodes as one of them, either
/// way: routed together they run apart, which a line routed alone cannot know of. Lines that
/// share only one node stay as they are, so that moving a node does not reroute every line
/// of its neighbours; the lines routed now keep off their ports.
pub fn with_neighbours(model: &umlcraft_model::Model, diagram: Id, edges: &[Id]) -> Vec<Id> {
    let Some(view) = model.view(diagram) else { return edges.to_vec() };
    let ends = |p: &umlcraft_model::Presentation| match &p.shape {
        Shape::Edge { source, target, .. } => Some((*source, *target)),
        _ => None,
    };
    // Each pair in one order, so that a line and the line back meet.
    let pair = |(s, t): (Id, Id)| if s <= t { (s, t) } else { (t, s) };
    let pairs: HashSet<(Id, Id)> =
        edges.iter().filter_map(|&e| ends(view.get(e)?)).map(pair).collect();
    let mut taken: HashSet<Id> = edges.iter().copied().collect();
    let mut result = edges.to_vec();
    for p in view.presentations() {
        let Some((s, t)) = ends(p) else { continue };
        let kept = p.style.auto_route && p.style.shared.is_separate() && s != t;
        if kept && pairs.contains(&pair((s, t))) && taken.insert(p.id) {
            result.push(p.id);
        }
    }
    result
}

/// How many rounds of loops around one corner [`follow_loops`] looks for.
const LOOP_ROUNDS: usize = 8;

/// Loops from a node to itself keep to their corner of the node: where the node moved or
/// changed size and the edit left its loop as it was, the loop moves as far as that corner,
/// and ends placed at the corner stay there.
fn follow_loops(tx: &mut Tx<'_>, diagram: Id, before: &Snapshot) -> Result<()> {
    let Some(view) = tx.model().view(diagram) else { return Ok(()) };
    let close = |a: Option<Point>, b: Point| {
        a.is_some_and(|a| (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001)
    };
    let mut changes = Vec::new();
    for p in view.presentations() {
        let Shape::Edge { source, target, waypoints, source_anchor, target_anchor, .. } = &p.shape
        else {
            continue;
        };
        if source != target || before.loops.get(&p.id) != Some(waypoints) {
            continue;
        }
        let (Some(&old), Some(new)) =
            (before.nodes.get(source), view.get(*source).and_then(|n| n.bounds()))
        else {
            continue;
        };
        if old == new {
            continue;
        }
        let corner = crate::scene::loop_corner_of(old, waypoints);
        let dx = if matches!(corner, 0 | 1) { new.right() - old.right() } else { new.x - old.x };
        let dy = if matches!(corner, 1 | 2) { new.bottom() - old.bottom() } else { new.y - old.y };
        let points = waypoints.iter().map(|w| w.offset(dx, dy)).collect();
        // Ends placed at the corner, by a loop of any round there, stay at it.
        let placed = (0..LOOP_ROUNDS).map(|round| round * 4 + corner).find(|&slot| {
            let (from, to) = crate::scene::self_loop_anchors_at(old, slot);
            close(*source_anchor, from) && close(*target_anchor, to)
        });
        let anchors = match placed {
            Some(slot) => {
                let (from, to) = crate::scene::self_loop_anchors_at(new, slot);
                (Some(from), Some(to))
            }
            None => (*source_anchor, *target_anchor),
        };
        changes.push((p.id, points, anchors));
    }
    for (edge, points, anchors) in changes {
        tx.set_route(diagram, edge, points, anchors)?;
    }
    Ok(())
}

/// Whether the horizontal or vertical segment from `a` to `b` runs through `r`.
pub(crate) fn segment_hits(a: Point, b: Point, r: Rect) -> bool {
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    x0 < r.right() && x1 > r.x && y0 < r.bottom() && y1 > r.y
}

/// The node bounds, the routed edges and the waypoints of the loops of a diagram, to tell
/// afterwards what an edit changed.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    nodes: HashMap<Id, Rect>,
    edges: Vec<Id>,
    loops: HashMap<Id, Vec<Point>>,
}

pub fn snapshot(model: &umlcraft_model::Model, diagram: Id) -> Snapshot {
    let Some(view) = model.view(diagram) else { return Snapshot::default() };
    let mut snapshot = Snapshot::default();
    for p in view.presentations() {
        if let Shape::Edge { source, target, waypoints, .. } = &p.shape
            && source == target
        {
            snapshot.loops.insert(p.id, waypoints.clone());
        }
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
    fn lines_keep_clear_of_a_loop() {
        // The boxes overlap narrowly at the corner of the first, where its loop is.
        let boxes = [Rect::new(0.0, 0.0, 100.0, 60.0), Rect::new(75.0, -150.0, 100.0, 60.0)];
        let requests = [Request { source: 0, target: 1, facing: false }];
        let lp = crate::scene::self_loop(boxes[0]);
        let area = Rect::from_corners(Point::new(80.0, -30.0), Point::new(130.0, 20.0));
        let crosses = |points: &[Point]| points.windows(2).any(|w| segment_hits(w[0], w[1], area));
        let straight = route(&boxes, &requests);
        assert!(crosses(straight[0].as_ref().expect("routed")));
        let around = route_around(&boxes, &[], &[(0, lp)], &[], &requests);
        let points = around[0].as_ref().expect("routed");
        assert!(!crosses(points) && clear(points, &boxes, [0, 1]), "{points:?}");
    }

    #[test]
    fn a_line_keeps_off_the_port_of_a_line_that_stays() {
        let boxes = [Rect::new(0.0, 0.0, 100.0, 100.0), Rect::new(300.0, 0.0, 100.0, 100.0)];
        let requests = [Request { source: 0, target: 1, facing: false }];
        let alone = route(&boxes, &requests);
        let end = *alone[0].as_ref().expect("routed").last().unwrap();
        assert_eq!(end, Point::new(300.0, 50.0));
        // Another line, not routed now, meets the target's left side there.
        let around = route_around(&boxes, &[], &[], &[(1, end)], &requests);
        let points = around[0].as_ref().expect("routed");
        assert_ne!(*points.last().unwrap(), end, "{points:?}");
        assert!(clear(points, &boxes, [0, 1]), "{points:?}");
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
        // into the bottom of the target, with two bends; the ports slide towards each other.
        assert_eq!(points.len(), 4, "{points:?}");
        assert_eq!((points[0].y, points[3].y), (60.0, 60.0), "{points:?}");
        assert!(points[1].y > 100.0 && points[1].y == points[2].y, "{points:?}");
        assert!((50.0..=88.0).contains(&points[0].x), "{points:?}");
        assert!((412.0..=450.0).contains(&points[3].x), "{points:?}");
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
        // The source's port slides to the target's middle, rather than take two bends.
        let routes = route(&boxes, &[Request { source: 0, target: 2, facing: false }]);
        assert_eq!(
            routes[0].as_deref(),
            Some(&[Point::new(56.0, 60.0), Point::new(56.0, 200.0)][..])
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
        // They leave the box through different points, far enough apart for their labels.
        assert!(a[0].distance(b[0]) >= PORT_SPACING - 0.1, "{a:?} {b:?}");
    }

    #[test]
    fn a_port_slides_along_its_side_for_a_straight_line() {
        // The middles lie 100 apart, too far for a jog to be straightened: a straight line
        // through the overlap, not one from middle to middle with two bends.
        let boxes = [Rect::new(0.0, 0.0, 300.0, 60.0), Rect::new(100.0, 200.0, 300.0, 60.0)];
        let routes = route(&boxes, &[Request { source: 0, target: 1, facing: false }]);
        let points = routes[0].as_ref().expect("routed");
        assert_eq!(points.len(), 2, "{points:?}");
        assert!(points[0].x > 100.0 && points[0].x < 300.0, "{points:?}");
    }

    #[test]
    fn two_lines_between_the_same_boxes_keep_apart() {
        let boxes = [Rect::new(0.0, 0.0, 200.0, 60.0), Rect::new(0.0, 300.0, 200.0, 60.0)];
        let requests = [
            Request { source: 0, target: 1, facing: false },
            Request { source: 1, target: 0, facing: false },
        ];
        let routes = route(&boxes, &requests);
        let (a, b) = (routes[0].as_ref().unwrap(), routes[1].as_ref().unwrap());
        // Both straight, not on top of each other, with room for the labels between them.
        assert_eq!((a.len(), b.len()), (2, 2), "{a:?} {b:?}");
        assert!((a[0].x - b[0].x).abs() >= PORT_SPACING - 0.1, "{a:?} {b:?}");
    }

    #[test]
    fn ports_close_to_a_taken_one_cost_more() {
        let mut ports = Ports::new();
        ports.insert((0, Side::Top), vec![100.0]);
        assert_eq!(crowd(&ports, 0, Side::Top, 100.0), CROWD);
        assert_eq!(crowd(&ports, 0, Side::Top, 100.0 + CROWD_RANGE / 2.0), CROWD / 2.0);
        assert_eq!(crowd(&ports, 0, Side::Top, 100.0 + CROWD_RANGE), 0.0);
        assert_eq!(crowd(&ports, 0, Side::Bottom, 100.0), 0.0);
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
