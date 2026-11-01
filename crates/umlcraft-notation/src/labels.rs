//! Places the labels of lines that were not placed by hand. Each label starts where Astah puts
//! it ([`scene`](crate::scene)); where that covers a node, a line or another label, it moves to
//! the spot nearby that covers least: the other side of its line, further along it, or far
//! enough from it not to cross it. Labels placed by hand, such as all labels in Astah's files,
//! stay where they are and only count as obstacles.

use umlcraft_model::{Point, Rect};

use crate::draw::{label_area, text_left};
use crate::route;
use crate::scene::{
    EdgeLabel, EdgeShape, FONT_SIZE, LABEL_HEIGHT, NodeShape, TextMeasure, label_width,
    line_height, name_arrow,
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

/// Moves the free labels of `edges` to where they cover least of `nodes`, the lines and the
/// other labels.
pub fn place(nodes: &[NodeShape], edges: &mut [EdgeShape], measure: &dyn TextMeasure) {
    let mut blocks = Vec::new();
    for (e, edge) in edges.iter().enumerate() {
        for (f, free) in edge.free_labels.iter().enumerate() {
            blocks.push(Block::new(edge, e, f, free, measure));
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
    /// Top left corners to choose from; the first is where Astah puts the label.
    spots: Vec<Point>,
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
        let width = lines.iter().map(|l| label_width(&l.text, measure)).fold(0.0, f32::max);
        let height = LABEL_HEIGHT + (free.lines - 1) as f32 * line_height(FONT_SIZE);
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
                spots.extend(end_spots(&edge.points, source, role, size));
            }
        }
        Self { edge: index, free: free_index, width, height, spots, chosen: 0 }
    }

    fn rect(&self, spot: usize) -> Rect {
        let p = self.spots[spot];
        Rect::new(p.x, p.y, self.width, self.height)
    }

    /// What the label covers wherever it goes.
    fn hull(&self) -> Rect {
        (1..self.spots.len()).fold(self.rect(0), |r, i| r.union(&self.rect(i))).inflate(1.0)
    }
}

/// Spots for a role (`role`) or multiplicity at the source or target end of `points`, on the
/// side Astah puts it first, then on the other: as far along the line as Astah or further, and
/// as far across as Astah or far enough not to cross the line.
fn end_spots(points: &[Point], source: bool, role: bool, (w, h): (f32, f32)) -> Vec<Point> {
    let n = points.len();
    let (tip, next) = if source { (points[0], points[1]) } else { (points[n - 1], points[n - 2]) };
    let len = tip.distance(next);
    if len < 1e-3 {
        return Vec::new();
    }
    let u = ((next.x - tip.x) / len, (next.y - tip.y) / len);
    // To the left of the direction the line runs in, from source to target.
    let run = if source { u } else { (-u.0, -u.1) };
    let left = (run.1, -run.0);
    let (base, first_side) = if role { (27.0, 1.0) } else { (18.0, -1.0) };
    let (half_w, half_h) = (w / 2.0, h / 2.0);
    let across = (half_w * left.0.abs() + half_h * left.1.abs() + GAP).max(18.0);
    let clear = half_w * u.0.abs() + half_h * u.1.abs() + GAP;
    let mut spots = Vec::new();
    for side in [first_side, -first_side] {
        for extra in FURTHER {
            let along = (base + extra).max(clear);
            let center = Point::new(
                tip.x + along * u.0 + side * across * left.0,
                tip.y + along * u.1 + side * across * left.1,
            );
            spots.push(center.offset(-half_w, -half_h));
        }
    }
    spots
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
fn label_rect(label: &EdgeLabel, measure: &dyn TextMeasure) -> Rect {
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
    fn spots_beside_a_line_keep_their_distance() {
        // Beside a vertical line, a box 40 wide keeps 4 from it to the right.
        let up = up_or_right((0.0, 1.0));
        let at = beside(Point::new(100.0, 50.0), up, GAP, (40.0, 15.0));
        assert_eq!(up, (1.0, 0.0));
        assert_eq!(at, Point::new(104.0, 42.5));
    }
}
