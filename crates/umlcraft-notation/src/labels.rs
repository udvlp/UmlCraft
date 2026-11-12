//! Places the labels of lines that were not placed by hand. Each label starts where Astah puts
//! it ([`scene`](crate::scene)); where that covers a node, a line or another label, it moves to
//! the spot nearby that covers least: the other side of its line, further along it, or far
//! enough from it not to cross it. Labels placed by hand, such as all labels in Astah's files,
//! stay where they are and only count as obstacles.

use umlcraft_model::{Point, Rect};

use crate::draw::{end_marker_bounds, label_area, text_left};
use crate::route;
use crate::scene::{
    EdgeLabel, EdgeShape, FONT_SIZE, LABEL_HEIGHT, NodeShape, TextMeasure, label_width, name_arrow,
};

/// Which label of a line a free label is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelKind {
    /// The name, stereotypes, a flow's guard or a transition's `trigger [guard] / effect`.
    Name,
    /// «include», «extend» or «bind».
    Keyword,
    Role {
        source: bool,
    },
    Multiplicity {
        source: bool,
    },
}

/// A label of a line that was not placed by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreeLabel {
    pub kind: LabelKind,
    /// Where it is in [`EdgeShape::labels`]; the `lines - 1` labels after it move along with
    /// it (a transition's guard on the line below its trigger).
    pub index: usize,
    pub lines: usize,
    /// Whether it moved away from where Astah puts it.
    pub moved: bool,
}

/// Room between a label and the line it moves away from.
const GAP: f32 = 4.0;
/// How far from its line a name may go, closest first: further off where nodes crowd it.
const OFF: [f32; 3] = [GAP, 20.0, 36.0];
/// Where along its line a name may go, as a share of the line's length, the middle first.
const ALONG: [f32; 7] = [0.5, 0.38, 0.62, 0.26, 0.74, 0.14, 0.86];
/// How much further from its end than Astah a role or multiplicity may go.
const FURTHER: [f32; 4] = [0.0, 14.0, 28.0, 44.0];
/// What covering costs, per pixel of line crossed or per row-high pixel of area covered.
const LINE_COST: f32 = 1.0;
const NODE_COST: f32 = 1.5;
const LABEL_COST: f32 = 2.0;
/// What moving costs: per spot further down the list, and per pixel away from Astah's spot.
const RANK_COST: f32 = 0.6;
const DISTANCE_COST: f32 = 0.03;
/// What a label pays for standing between its line and another line between the same two
/// nodes, where it would read as that line's.
const BETWEEN_COST: f32 = 30.0;

/// Moves the free labels of `edges` to where they cover least of `nodes`, the lines and the
/// other labels.
pub fn place(nodes: &[NodeShape], edges: &mut [EdgeShape], measure: &dyn TextMeasure) {
    let mut blocks = Vec::new();
    for (e, edge) in edges.iter().enumerate() {
        for (f, free) in edge.free_labels.iter().enumerate() {
            let mut block = Block::new(edge, e, f, free, measure);
            block.keep_outside(edge, free, edges);
            blocks.push(block);
        }
    }
    if blocks.is_empty() {
        return;
    }
    let obstacles = Obstacles::new(nodes, edges, measure);
    let nearby: Vec<Obstacles> = blocks.iter().map(|b| obstacles.near(b.hull())).collect();
    // The first round places each label among those placed before it, the second among all.
    for round in 0..2 {
        for k in 0..blocks.len() {
            let hull = blocks[k].hull();
            let others: Vec<Rect> = blocks
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != k && (round > 0 || j < k))
                .map(|(_, b)| b.rect(b.chosen))
                .filter(|r| r.intersects(&hull))
                .collect();
            let block = &blocks[k];
            let score = |i: usize| {
                let r = block.rect(i);
                let moved = block.spots[i].distance(block.spots[0]);
                nearby[k].cost(r)
                    + overlaps(r, &others) * LABEL_COST
                    + i as f32 * RANK_COST
                    + moved * DISTANCE_COST
                    + block.penalty.get(i).copied().unwrap_or(0.0)
            };
            let best = (0..block.spots.len())
                .map(|i| (i, score(i)))
                .fold((0, f32::INFINITY), |best, (i, s)| if s < best.1 { (i, s) } else { best });
            blocks[k].chosen = best.0;
        }
    }
    for block in blocks.iter().filter(|b| b.chosen != 0) {
        let edge = &mut edges[block.edge];
        let free = &mut edge.free_labels[block.free];
        free.moved = true;
        let (from, to) = (block.spots[0], block.spots[block.chosen]);
        let (index, lines, kind) = (free.index, free.lines, free.kind);
        for label in &mut edge.labels[index..index + lines] {
            label.anchor = label.anchor.offset(to.x - from.x, to.y - from.y);
        }
        if kind == LabelKind::Name && edge.name_arrow.is_some() {
            let width = label_width(&edge.labels[index].text, measure);
            let segment = route::middle_segment(&edge.points, edge.shape.is_curve());
            edge.name_arrow = Some(name_arrow(edge.labels[index].anchor, width, segment));
        }
    }
}

/// A free label with the lines that move along, and the spots it may go to.
struct Block {
    edge: usize,
    /// Which of the edge's free labels.
    free: usize,
    width: f32,
    height: f32,
    /// Where the lines' bounding box starts relative to the first line's top left corner:
    /// constraints below a name are centered on their own.
    offset: Point,
    /// Top left corners to choose from; the first is where Astah puts the label.
    spots: Vec<Point>,
    /// What each spot costs besides what it covers; none if empty.
    penalty: Vec<f32>,
    chosen: usize,
}

impl Block {
    fn new(
        edge: &EdgeShape,
        index: usize,
        free_index: usize,
        free: &FreeLabel,
        measure: &dyn TextMeasure,
    ) -> Self {
        let lines = &edge.labels[free.index..free.index + free.lines];
        let first = lines[0].anchor;
        let bounds = lines
            .iter()
            .map(|l| Rect::new(l.anchor.x, l.anchor.y, label_width(&l.text, measure), LABEL_HEIGHT))
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default();
        let (width, height) = (bounds.width, bounds.height);
        let offset = Point::new(bounds.x - first.x, bounds.y - first.y);
        let size = (width, height);
        let mut spots = vec![lines[0].anchor];
        match free.kind {
            LabelKind::Name | LabelKind::Keyword => {
                let path = edge.path();
                let total: f32 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
                for off in OFF {
                    for share in ALONG {
                        let Some((p, dir)) = at_length(&path, share * total) else { continue };
                        let up = up_or_right(dir);
                        for side in [1.0, -1.0] {
                            spots.push(beside(p, (up.0 * side, up.1 * side), off, size));
                        }
                    }
                }
            }
            LabelKind::Role { source } | LabelKind::Multiplicity { source } => {
                let role = matches!(free.kind, LabelKind::Role { .. });
                // A mirrored loop keeps its labels beside it, on the other sides.
                let flip = crate::scene::mirrored_loop(&edge.points, edge.ends);
                // On a loop, the inside first after Astah's spot.
                let [a, b] = edge.ends;
                let inside = (a == b && edge.points.len() > 2).then(|| {
                    let n = edge.points.len() as f32;
                    let x = edge.points.iter().map(|p| p.x).sum::<f32>() / n;
                    let y = edge.points.iter().map(|p| p.y).sum::<f32>() / n;
                    Point::new(x, y)
                });
                spots.extend(end_spots(&edge.points, source, role, flip, inside, size));
            }
        }
        Self {
            edge: index,
            free: free_index,
            width,
            height,
            offset,
            spots,
            penalty: Vec::new(),
            chosen: 0,
        }
    }

    /// For a label of a line beside another line between the same two nodes: makes the spots
    /// between the two cost more, so that the label goes to the outer side, where it reads as
    /// its own line's. A role or multiplicity looks at its end segment, a name or keyword at the
    /// middle segment.
    fn keep_outside(&mut self, edge: &EdgeShape, free: &FreeLabel, edges: &[EdgeShape]) {
        let ends = |e: &EdgeShape| {
            let [a, b] = e.ends;
            if a == b { None } else { Some([a, b]) }
        };
        let Some([a, b]) = ends(edge) else { return };
        let n = edge.points.len();
        if n < 2 {
            return;
        }
        let middle_segment = |e: &EdgeShape| route::middle_segment(&e.points, e.shape.is_curve());
        // The segment the label belongs to and, for a role or multiplicity, the node its end
        // is at.
        let (tip, next, end) = match free.kind {
            LabelKind::Role { source } | LabelKind::Multiplicity { source } if source => {
                (edge.points[0], edge.points[1], Some(a))
            }
            LabelKind::Role { .. } | LabelKind::Multiplicity { .. } => {
                (edge.points[n - 1], edge.points[n - 2], Some(b))
            }
            LabelKind::Name | LabelKind::Keyword => {
                let (p, q) = middle_segment(edge);
                (p, q, None)
            }
        };
        let len = tip.distance(next);
        if len < 1e-3 {
            return;
        }
        // Across the end segment.
        let normal = ((next.y - tip.y) / len, -(next.x - tip.x) / len);
        let side = |p: Point| (p.x - tip.x) * normal.0 + (p.y - tip.y) * normal.1;
        let middle = |e: &EdgeShape| {
            let k = e.points.len();
            let (p, q) = (e.points[0], e.points[k - 1]);
            Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0)
        };
        // Which side of the segment another line lies on. At an end: where it meets the node
        // this end is at, as drawn (a shared generalization runs over its common line), or its
        // middle where both meet the node at one point. For a name: its middle segment.
        let at_node = |e: &EdgeShape| match end {
            Some(node) => {
                let k = e.points.len();
                let p = if e.ends[0] == node { e.points[0] } else { e.points[k - 1] };
                if side(p).abs() > 0.5 { side(p) } else { side(middle(e)) }
            }
            None => {
                let (p, q) = middle_segment(e);
                side(Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0))
            }
        };
        let others: Vec<f32> = edges
            .iter()
            .filter(|e| !std::ptr::eq(*e, edge) && e.points.len() >= 2)
            .filter(|e| ends(e).is_some_and(|[c, d]| (c == a && d == b) || (c == b && d == a)))
            .map(at_node)
            .filter(|d| d.abs() > 0.5)
            .collect();
        // Only a line with others on one side has an outer side.
        let Some(&first) = others.first() else { return };
        if others.iter().any(|d| d.signum() != first.signum()) {
            return;
        }
        let (w, h) = (self.width, self.height);
        self.penalty = (0..self.spots.len())
            .map(|i| {
                let r = self.rect(i);
                let center = Point::new(r.x + w / 2.0, r.y + h / 2.0);
                if side(center).signum() == first.signum() { BETWEEN_COST } else { 0.0 }
            })
            .collect();
    }

    fn rect(&self, spot: usize) -> Rect {
        let p = self.spots[spot];
        Rect::new(p.x + self.offset.x, p.y + self.offset.y, self.width, self.height)
    }

    /// What the label covers wherever it goes.
    fn hull(&self) -> Rect {
        (1..self.spots.len()).fold(self.rect(0), |r, i| r.union(&self.rect(i))).inflate(1.0)
    }
}

/// Spots for a role (`role`) or multiplicity at the source or target end of `points`, on the
/// side Astah puts it first, then on the other: as far along the line as Astah or further, and
/// as far across as Astah or far enough not to cross the line. Past the end segment the spots
/// follow the line around its bend, beside the segment they reach, and also lie on the end
/// segment's extension, which is clear where the line turns back (a loop). With `inside`, the
/// side of the end segment facing that point comes first: the inside of a loop, where a short
/// text reads as its own.
fn end_spots(
    points: &[Point],
    source: bool,
    role: bool,
    flip: bool,
    inside: Option<Point>,
    (w, h): (f32, f32),
) -> Vec<Point> {
    let n = points.len();
    let (tip, next) = if source { (points[0], points[1]) } else { (points[n - 1], points[n - 2]) };
    let len = tip.distance(next);
    if len < 1e-3 {
        return Vec::new();
    }
    // The line from this end on.
    let path: Vec<Point> =
        if source { points.to_vec() } else { points.iter().rev().copied().collect() };
    let u = ((next.x - tip.x) / len, (next.y - tip.y) / len);
    let (base, first_side) = if role { (27.0, 1.0) } else { (18.0, -1.0) };
    let first_side = if flip { -first_side } else { first_side };
    // Left of the end segment as the line runs, from source to target (as `left` below).
    let first_side = match inside {
        Some(p) => {
            let run = if source { u } else { (-u.0, -u.1) };
            let toward = run.1 * (p.x - tip.x) - run.0 * (p.y - tip.y);
            if toward.abs() > 0.5 { toward.signum() } else { first_side }
        }
        None => first_side,
    };
    let (half_w, half_h) = (w / 2.0, h / 2.0);
    // Far enough from the end not to reach back over its node.
    let clear = half_w * u.0.abs() + half_h * u.1.abs() + GAP;
    let mut spots = Vec::new();
    for side in [first_side, -first_side] {
        for extra in FURTHER {
            let along = (base + extra).max(clear);
            let (p, d, first) = along_from_end(&path, along);
            let extended = Point::new(tip.x + u.0 * along, tip.y + u.1 * along);
            let beyond = (!first).then_some((extended, u, true));
            for (p, d, first) in std::iter::once((p, d, first)).chain(beyond) {
                // To the left of the direction the line runs in, from source to target.
                let run = if source { d } else { (-d.0, -d.1) };
                let left = (run.1, -run.0);
                let mut across = half_w * left.0.abs() + half_h * left.1.abs() + GAP;
                if first {
                    across = across.max(18.0);
                }
                let center = Point::new(p.x + side * across * left.0, p.y + side * across * left.1);
                spots.push(center.offset(-half_w, -half_h));
            }
        }
    }
    spots
}

/// The point `length` along `path` from its start, the direction of the segment it lies on and
/// whether that is the first; past the end, on the last segment's extension.
fn along_from_end(path: &[Point], length: f32) -> (Point, (f32, f32), bool) {
    let mut rest = length;
    let mut first = true;
    for (k, w) in path.windows(2).enumerate() {
        let d = w[0].distance(w[1]);
        if d < 1e-3 {
            continue;
        }
        let dir = ((w[1].x - w[0].x) / d, (w[1].y - w[0].y) / d);
        if rest <= d || k + 2 == path.len() {
            return (Point::new(w[0].x + dir.0 * rest, w[0].y + dir.1 * rest), dir, first);
        }
        rest -= d;
        first = false;
    }
    (path[0], (1.0, 0.0), true)
}

/// The point `length` along `path` and the direction of the segment it lies on.
fn at_length(path: &[Point], length: f32) -> Option<(Point, (f32, f32))> {
    let mut rest = length;
    let mut last = None;
    for w in path.windows(2) {
        let d = w[0].distance(w[1]);
        if d < 1e-3 {
            continue;
        }
        let dir = ((w[1].x - w[0].x) / d, (w[1].y - w[0].y) / d);
        if rest <= d {
            return Some((Point::new(w[0].x + dir.0 * rest, w[0].y + dir.1 * rest), dir));
        }
        rest -= d;
        last = Some((w[1], dir));
    }
    last
}

/// The normal of direction `dir` that points up or to the right.
fn up_or_right((dx, dy): (f32, f32)) -> (f32, f32) {
    let (nx, ny) = (dy, -dx);
    if nx - ny > 0.0 || (nx - ny == 0.0 && ny < 0.0) { (nx, ny) } else { (-nx, -ny) }
}

/// The top left corner of a box of `size` beside `p`, `gap` away from it toward `normal`.
fn beside(p: Point, (nx, ny): (f32, f32), gap: f32, (w, h): (f32, f32)) -> Point {
    let (half_w, half_h) = (w / 2.0, h / 2.0);
    let distance = gap + half_w * nx.abs() + half_h * ny.abs();
    Point::new(p.x + nx * distance - half_w, p.y + ny * distance - half_h)
}

/// What labels may not cover.
#[derive(Default)]
struct Obstacles {
    /// Nodes (whose area counts).
    boxes: Vec<Rect>,
    /// Lines and the borders of nodes with others inside (whose length inside counts).
    segments: Vec<(Point, Point)>,
    /// Labels placed by hand and labels of sequence diagrams.
    labels: Vec<Rect>,
}

impl Obstacles {
    fn new(nodes: &[NodeShape], edges: &[EdgeShape], measure: &dyn TextMeasure) -> Self {
        let mut obstacles = Self::default();
        for node in nodes {
            let r = node.rect;
            let container = nodes.iter().any(|o| o.rect != r && r.contains_rect(&o.rect));
            if container {
                // A label may lie inside a package, a composite state or a lane, but not across
                // its border or on its name.
                let corners = [
                    Point::new(r.x, r.y),
                    Point::new(r.right(), r.y),
                    Point::new(r.right(), r.bottom()),
                    Point::new(r.x, r.bottom()),
                ];
                obstacles.segments.extend((0..4).map(|i| (corners[i], corners[(i + 1) % 4])));
                obstacles.boxes.extend(node.rows.iter().map(|row| row.rect));
            } else {
                obstacles.boxes.push(node.extent());
            }
        }
        for edge in edges {
            let path = edge.path();
            obstacles.segments.extend(path.windows(2).map(|w| (w[0], w[1])));
            obstacles.boxes.extend(edge.qualifiers.iter().map(|(rect, _)| *rect));
            // Arrowheads, diamonds and crosses, with room so that a label does not touch them.
            obstacles.boxes.extend(end_marker_bounds(edge).into_iter().map(|r| r.inflate(2.0)));
            let free = |i: usize| {
                edge.free_labels.iter().any(|f| (f.index..f.index + f.lines).contains(&i))
            };
            obstacles.labels.extend(
                edge.labels
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| !free(i))
                    .map(|(_, label)| label_rect(label, measure)),
            );
        }
        obstacles
    }

    /// The obstacles that reach into `area`.
    fn near(&self, area: Rect) -> Self {
        Self {
            boxes: self.boxes.iter().filter(|b| b.intersects(&area)).copied().collect(),
            segments: self
                .segments
                .iter()
                .filter(|(a, b)| Rect::from_corners(*a, *b).intersects(&area))
                .copied()
                .collect(),
            labels: self.labels.iter().filter(|l| l.intersects(&area)).copied().collect(),
        }
    }

    /// What a label at `r` covers, weighed.
    fn cost(&self, r: Rect) -> f32 {
        let r = r.inflate(1.0);
        let lines: f32 = self.segments.iter().map(|&(a, b)| inside_length(r, a, b)).sum();
        overlaps(r, &self.boxes) * NODE_COST
            + overlaps(r, &self.labels) * LABEL_COST
            + lines * LINE_COST
    }
}

/// Where a label is drawn.
pub(crate) fn label_rect(label: &EdgeLabel, measure: &dyn TextMeasure) -> Rect {
    let width = measure.width(&label.text, FONT_SIZE, false);
    let left = text_left(label_area(label), width, label.align);
    Rect::new(left, label.anchor.y, width, LABEL_HEIGHT)
}

/// How much of `rects` `r` covers, in pixels of a row's height.
fn overlaps(r: Rect, rects: &[Rect]) -> f32 {
    let area: f32 = rects
        .iter()
        .map(|o| {
            let w = r.right().min(o.right()) - r.x.max(o.x);
            let h = r.bottom().min(o.bottom()) - r.y.max(o.y);
            w.max(0.0) * h.max(0.0)
        })
        .sum();
    area / LABEL_HEIGHT
}

/// How long the part of the segment `a`–`b` inside `r` is.
fn inside_length(r: Rect, a: Point, b: Point) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in
        [(-dx, a.x - r.x), (dx, r.right() - a.x), (-dy, a.y - r.y), (dy, r.bottom() - a.y)]
    {
        if p == 0.0 {
            if q < 0.0 {
                return 0.0;
            }
        } else if p < 0.0 {
            t0 = t0.max(q / p);
        } else {
            t1 = t1.min(q / p);
        }
    }
    if t1 > t0 { (t1 - t0) * dx.hypot(dy) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_count_with_their_length_inside() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let across = inside_length(r, Point::new(-5.0, 5.0), Point::new(15.0, 5.0));
        let outside = inside_length(r, Point::new(-5.0, 20.0), Point::new(15.0, 20.0));
        let diagonal = inside_length(r, Point::new(-10.0, -10.0), Point::new(20.0, 20.0));
        assert_eq!((across, outside), (10.0, 0.0));
        assert!((diagonal - 200f32.sqrt()).abs() < 1e-3);
    }

    #[test]
    fn end_spots_follow_the_line_around_its_bend() {
        // Down 20 from the source, then right: a role 27 along lies beside the second segment,
        // above it (to the left of the way the line runs), not on it; others lie further down
        // the first segment's extension, below the bend.
        let points = [Point::new(0.0, 0.0), Point::new(0.0, 20.0), Point::new(200.0, 20.0)];
        let (p, d, first) = along_from_end(&points, 27.0);
        assert_eq!((p, d, first), (Point::new(7.0, 20.0), (1.0, 0.0), false));
        let (w, h) = (40.0, 15.0);
        let (mut above, mut below) = (0, 0);
        for spot in end_spots(&points, true, true, false, None, (w, h)) {
            let r = Rect::new(spot.x, spot.y, w, h);
            let crossed: f32 = points.windows(2).map(|s| inside_length(r, s[0], s[1])).sum();
            if spot.x > 0.0 && r.bottom() <= 20.0 {
                assert_eq!(crossed, 0.0, "{spot:?}");
                above += 1;
            }
            below += usize::from(spot.x > 0.0 && r.y > 20.0 && crossed == 0.0);
        }
        assert!(above > 0 && below > 0, "{above} {below}");
    }

    #[test]
    fn spots_beside_a_line_keep_their_distance() {
        // Beside a vertical line, a box 40 wide keeps 4 from it to the right.
        let up = up_or_right((0.0, 1.0));
        let at = beside(Point::new(100.0, 50.0), up, GAP, (40.0, 15.0));
        assert_eq!(up, (1.0, 0.0));
        assert_eq!(at, Point::new(104.0, 42.5));
    }
}
