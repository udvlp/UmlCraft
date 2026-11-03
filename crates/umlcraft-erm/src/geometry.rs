//! Where draw.io puts things: the outlines of the shapes, where lines start and end, the
//! second frames, double lines and the position of line labels. Every rule names its source in
//! draw.io 31.7; the line ends were checked on 338 lines of draw.io's exports. The rules come
//! from draw.io; the code computing them is our own.

use crate::model::{Diagram, LabelPos, Link, Node, NodeKind, Point, Rect};

/// The outline a node's lines end on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outline {
    Rectangle,
    Ellipse,
    Rhombus,
    /// Pointing up: apex in the middle of the top, base along the bottom.
    Triangle,
}

impl Outline {
    pub fn of(kind: &NodeKind) -> Self {
        match kind {
            NodeKind::Entity { .. } | NodeKind::AssociativeEntity => Self::Rectangle,
            NodeKind::Attribute { .. } => Self::Ellipse,
            NodeKind::Relationship { .. } => Self::Rhombus,
            NodeKind::IsA { .. } => Self::Triangle,
        }
    }
}

/// The point of the outline in the direction of `next`, seen from the middle of the box
/// (`mxPerimeter`).
pub fn perimeter(outline: Outline, b: &Rect, next: Point) -> Point {
    match outline {
        Outline::Rectangle => rectangle(b, next),
        Outline::Ellipse => ellipse(b, next),
        Outline::Rhombus => rhombus(b, next),
        Outline::Triangle => polygon(&triangle(b), b, next),
    }
}

/// Where the ray from the middle of a rectangle towards `next` leaves it: the ray is scaled
/// until it reaches the nearer of the vertical and the horizontal sides. Towards the middle
/// itself, the middle of the right side (draw.io's `RectanglePerimeter`).
fn rectangle(b: &Rect, next: Point) -> Point {
    let c = b.center();
    let (dx, dy) = (next.x - c.x, next.y - c.y);
    if dx == 0.0 && dy == 0.0 {
        return Point::new(b.x + b.width, c.y);
    }
    let reach = |half: f64, d: f64| if d == 0.0 { f64::INFINITY } else { half / d.abs() };
    let k = reach(b.width / 2.0, dx).min(reach(b.height / 2.0, dy));
    Point::new(c.x + k * dx, c.y + k * dy)
}

/// Where the ray from the middle of an ellipse towards `next` leaves it. draw.io's
/// `EllipsePerimeter` cuts the direction to whole numbers first, so a line towards a point a
/// fraction of a pixel to the side still meets the top or the bottom; towards the middle
/// itself it gives `next`.
fn ellipse(b: &Rect, next: Point) -> Point {
    let c = b.center();
    let (dx, dy) = ((next.x - c.x).trunc(), (next.y - c.y).trunc());
    if dx == 0.0 && dy == 0.0 {
        return next;
    }
    // The point c + k·(dx, dy) lies on the ellipse where (k·dx/rx)² + (k·dy/ry)² = 1.
    let (rx, ry) = (b.width / 2.0, b.height / 2.0);
    let k = 1.0 / ((dx / rx).powi(2) + (dy / ry).powi(2)).sqrt();
    Point::new(c.x + k * dx, c.y + k * dy)
}

/// Where the ray from the middle of a rhombus towards `next` leaves it. As in draw.io's
/// `RhombusPerimeter`, a direction along an axis gives that corner (straight down when `next`
/// is the middle), and a `next` inside the rhombus off the axes gives the middle.
fn rhombus(b: &Rect, next: Point) -> Point {
    let c = b.center();
    let (dx, dy) = (next.x - c.x, next.y - c.y);
    let (rx, ry) = (b.width / 2.0, b.height / 2.0);
    if dx == 0.0 {
        return Point::new(c.x, if dy < 0.0 { c.y - ry } else { c.y + ry });
    }
    if dy == 0.0 {
        return Point::new(if dx < 0.0 { c.x - rx } else { c.x + rx }, c.y);
    }
    // The point c + k·(dx, dy) lies on the rhombus where k·(|dx|/rx + |dy|/ry) = 1.
    let k = 1.0 / (dx.abs() / rx + dy.abs() / ry);
    if k > 1.0 { c } else { Point::new(c.x + k * dx, c.y + k * dy) }
}

/// The IS-A triangle's corners: apex, bottom right, bottom left.
pub fn triangle(b: &Rect) -> [Point; 3] {
    [
        Point::new(b.x + b.width / 2.0, b.y),
        Point::new(b.x + b.width, b.y + b.height),
        Point::new(b.x, b.y + b.height),
    ]
}

/// The rhombus through the middles of a box's sides: top, right, bottom, left.
pub fn rhombus_corners(b: &Rect) -> [Point; 4] {
    let c = b.center();
    [
        Point::new(c.x, b.y),
        Point::new(b.x + b.width, c.y),
        Point::new(c.x, b.y + b.height),
        Point::new(b.x, c.y),
    ]
}

/// Where the ray from the middle towards `next` leaves a convex polygon.
fn polygon(points: &[Point], b: &Rect, next: Point) -> Point {
    let c = b.center();
    let far = Point::new(c.x + (next.x - c.x) * 1e4, c.y + (next.y - c.y) * 1e4);
    (0..points.len())
        .find_map(|i| intersection(c, far, points[i], points[(i + 1) % points.len()]))
        .unwrap_or(c)
}

/// Where the segments a–b and c–d cross; none when they are parallel or miss each other.
fn intersection(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    // a + t·r = c + u·s, solved with the 2D cross product.
    let cross = |p: (f64, f64), q: (f64, f64)| p.0 * q.1 - p.1 * q.0;
    let (r, s, ac) = ((b.x - a.x, b.y - a.y), (d.x - c.x, d.y - c.y), (c.x - a.x, c.y - a.y));
    let rs = cross(r, s);
    if rs == 0.0 {
        return None;
    }
    let (t, u) = (cross(ac, s) / rs, cross(ac, r) / rs);
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u))
        .then(|| Point::new(a.x + t * r.0, a.y + t * r.1))
}

pub fn distance(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// A fixed connection point: `port` relative to the box, projected onto the outline. Not
/// rounded: `mxGraphView.getFixedTerminalPoint` asks `getConnectionPoint` not to.
pub fn port_point(node: &Node, port: Point) -> Point {
    let b = &node.bounds;
    let p = Point::new(b.x + port.x * b.width, b.y + port.y * b.height);
    perimeter(Outline::of(&node.kind), b, p)
}

/// The points a link is drawn through: start, waypoints, end. A floating end aims at the next
/// point (waypoint, the other end's fixed point, the other node's middle); the target end is
/// computed first, so a floating start aims at it (`mxGraphView.updateFloatingTerminalPoints`).
pub fn link_points(source: &Node, target: &Node, link: &Link) -> Vec<Point> {
    let start = link.source_port.map(|p| port_point(source, p));
    let mut end = link.target_port.map(|p| port_point(target, p));
    if end.is_none() {
        let next = link.waypoints.last().copied().or(start).unwrap_or(source.bounds.center());
        end = Some(perimeter(Outline::of(&target.kind), &target.bounds, next));
    }
    let end = end.unwrap_or(target.bounds.center());
    let start = start.unwrap_or_else(|| {
        let next = link.waypoints.first().copied().unwrap_or(end);
        perimeter(Outline::of(&source.kind), &source.bounds, next)
    });
    let mut points = Vec::with_capacity(link.waypoints.len() + 2);
    points.push(start);
    points.extend(&link.waypoints);
    points.push(end);
    points
}

/// The points of a link in a diagram, `None` if an end is missing.
pub fn points_of(d: &Diagram, link: &Link) -> Option<Vec<Point>> {
    Some(link_points(d.node(&link.source)?, d.node(&link.target)?, link))
}

/// Where a label's text is centered, as draw.io places it: `x` from -1 to 1 along the
/// polyline, measured in whole pixels, then `y` px to the left of the direction of the
/// segment it falls on, then the offset.
pub fn label_anchor(points: &[Point], pos: &LabelPos) -> Point {
    let lengths: Vec<f64> = points.windows(2).map(|w| distance(w[0], w[1])).collect();
    let Some(last) = lengths.len().checked_sub(1) else {
        let p = points.first().copied().unwrap_or_default();
        return Point::new(p.x + pos.dx, p.y + pos.dy);
    };
    let along = ((pos.x / 2.0 + 0.5) * lengths.iter().sum::<f64>()).round();
    // The segment the label falls on: the first whose end, rounded, lies beyond `along`.
    let mut start = 0.0;
    let mut i = 0;
    while i < last && along >= (start + lengths[i]).round() {
        start += lengths[i];
        i += 1;
    }
    let (from, to, length) = (points[i], points[i + 1], lengths[i]);
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    // The share of the segment and the unit normal to its left (y grows downwards).
    let (share, left) = if length == 0.0 {
        (0.0, (0.0, 0.0))
    } else {
        ((along - start) / length, (dy / length, -dx / length))
    };
    Point::new(
        from.x + share * dx + left.0 * pos.y + pos.dx,
        from.y + share * dy + left.1 * pos.y + pos.dy,
    )
}

/// The two lines of a double line (`shape=link`, 4 px wide): the polyline moved 2 px to each
/// side; at waypoints the moved segments meet where they cross.
pub fn double_line(points: &[Point]) -> [Vec<Point>; 2] {
    [offset_polyline(points, 2.0), offset_polyline(points, -2.0)]
}

fn offset_polyline(points: &[Point], by: f64) -> Vec<Point> {
    let normal = |a: Point, b: Point| {
        let len = distance(a, b);
        if len == 0.0 { (0.0, 0.0) } else { (-(b.y - a.y) / len * by, (b.x - a.x) / len * by) }
    };
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let before = (i > 0).then(|| normal(points[i - 1], points[i]));
        let after = (i + 1 < n).then(|| normal(points[i], points[i + 1]));
        let p = points[i];
        let moved = match (before, after) {
            (Some(a), Some(b)) => {
                // Cut of the two moved segments; parallel segments just move.
                let a0 = Point::new(points[i - 1].x + a.0, points[i - 1].y + a.1);
                let a1 = Point::new(p.x + a.0, p.y + a.1);
                let b0 = Point::new(p.x + b.0, p.y + b.1);
                let b1 = Point::new(points[i + 1].x + b.0, points[i + 1].y + b.1);
                line_cut(a0, a1, b0, b1).unwrap_or(a1)
            }
            (Some(v), None) | (None, Some(v)) => Point::new(p.x + v.0, p.y + v.1),
            (None, None) => p,
        };
        out.push(moved);
    }
    out
}

/// Where the infinite lines through a–b and c–d cross.
fn line_cut(a: Point, b: Point, c: Point, d: Point) -> Option<Point> {
    let denom = (d.y - c.y) * (b.x - a.x) - (d.x - c.x) * (b.y - a.y);
    if denom.abs() < 1e-9 {
        return None;
    }
    let ua = ((d.x - c.x) * (a.y - c.y) - (d.y - c.y) * (a.x - c.x)) / denom;
    Some(Point::new(a.x + ua * (b.x - a.x), a.y + ua * (b.y - a.y)))
}

/// How far a second frame lies inside the box: weak entity
/// (`ExtendedShape`: max(2, stroke + 1) + margin 3), multivalued attribute
/// (`mxDoubleEllipse`: margin 3), identifying relationship (`mxRhombus`: max(2, stroke + 1)
/// × 2), for a stroke width of 1.
pub fn inner_inset(kind: &NodeKind) -> Option<f64> {
    match kind {
        NodeKind::Entity { weak: true } => Some(5.0),
        NodeKind::Attribute { multivalued: true, .. } => Some(3.0),
        NodeKind::Relationship { identifying: true } => Some(4.0),
        _ => None,
    }
}

pub fn inset(r: &Rect, by: f64) -> Rect {
    Rect::new(r.x + by, r.y + by, r.width - 2.0 * by, r.height - 2.0 * by)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Id, Props, Style};

    fn node(kind: NodeKind, x: f64, y: f64, w: f64, h: f64) -> Node {
        Node {
            id: Id::from("n"),
            kind,
            name: String::new(),
            bounds: Rect::new(x, y, w, h),
            style: Style::default(),
            props: Props::default(),
        }
    }

    fn close(p: Point, x: f64, y: f64) {
        assert!((p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01, "{p:?} is not ({x}, {y})");
    }

    /// Numbers from `refs/tools/geom.py` on `lines.drawio` and Steven's original, both
    /// matching draw.io's export to 0.007 px.
    #[test]
    fn fixed_ports_project_onto_the_outline() {
        // An ellipse's box corner (exitX=1;exitY=1) lands on the ellipse, not on the corner
        // (line …-16 of the original: attribute "vorname" to "Kunde").
        let vorname = node(NodeKind::attribute(), -50.0, 68.0, 70.0, 28.0);
        close(port_point(&vorname, Point::new(1.0, 1.0)), 9.749, 91.899);
        let kunde = node(NodeKind::entity(), 80.0, 120.0, 100.0, 40.0);
        close(port_point(&kunde, Point::new(0.0, 0.0)), 80.0, 120.0);
        // A rhombus's box corner lands on its edge.
        let r = node(NodeKind::relationship(), 0.0, 0.0, 60.0, 40.0);
        close(port_point(&r, Point::new(1.0, 0.0)), 45.0, 10.0);
    }

    #[test]
    fn floating_ends_aim_at_the_middle_or_the_fixed_point() {
        let a = node(NodeKind::entity(), 0.0, 0.0, 100.0, 40.0);
        let b = node(NodeKind::attribute(), 200.0, 100.0, 70.0, 28.0);
        let link = Link::new(Id::from("a"), Id::from("b"));
        let p = link_points(&a, &b, &link);
        // From (50, 20) towards (235, 114): flatter than the diagonal, so the bottom side.
        close(p[0], 89.362, 40.0);
        assert!((p[1].x - 200.0).abs() < 35.0 && p[1].y > 100.0);
        // With a fixed start, the floating end aims at that point.
        let mut fixed = link.clone();
        fixed.source_port = Some(Point::new(1.0, 1.0));
        let p = link_points(&a, &b, &fixed);
        close(p[0], 100.0, 40.0);
    }

    #[test]
    fn ellipse_directions_are_cut_to_whole_numbers() {
        // dx = 0.6 counts as 0: the line goes straight up.
        let b = Rect::new(0.0, 0.0, 70.0, 28.0);
        close(ellipse(&b, Point::new(35.6, -100.0)), 35.0, 0.0);
    }

    #[test]
    fn labels_sit_left_of_the_direction() {
        // labels.drawio e5: x = -1, y = -20 on a line to the right: 20 px below its start.
        let line = [Point::new(230.0, 170.0), Point::new(300.0, 170.0)];
        let pos = |x, y| LabelPos { x, y, dx: 0.0, dy: 0.0 };
        close(label_anchor(&line, &pos(-1.0, -20.0)), 230.0, 190.0);
        // e9: upwards, y = -20 lies 20 px to the right.
        let up = [Point::new(180.0, 150.0), Point::new(180.0, 40.0)];
        close(label_anchor(&up, &pos(-1.0, -20.0)), 200.0, 150.0);
        close(label_anchor(&line, &pos(0.0, 14.0)), 265.0, 156.0);
        let moved = LabelPos { x: 0.0, y: 0.0, dx: 31.0, dy: -10.0 };
        close(label_anchor(&line, &moved), 296.0, 160.0);
    }

    #[test]
    fn double_lines_run_two_px_to_each_side() {
        let [a, b] = double_line(&[Point::new(1040.0, 1410.0), Point::new(1040.0, 1300.0)]);
        close(a[0], 1042.0, 1410.0);
        close(b[1], 1038.0, 1300.0);
        let [bend, _] =
            double_line(&[Point::new(0.0, 0.0), Point::new(100.0, 0.0), Point::new(100.0, 100.0)]);
        close(bend[1], 98.0, 2.0);
    }
}
