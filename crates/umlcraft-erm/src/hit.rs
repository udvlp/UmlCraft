//! What is under the mouse, what a rubber band holds, the grips of a selected box and the
//! connection points of a node – everything the editor asks of the geometry.

use crate::draw::label_box;
use crate::edit::Item;
use crate::geometry::{Outline, distance, perimeter, points_of, port_point, triangle};
use crate::model::{Diagram, Link, Node, NodeKind, Point, Rect};

/// The eight grips of a box, clockwise from the top left corner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    /// Where the grip sits, relative to the box (0..1).
    pub fn at(self) -> (f64, f64) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::Left => (0.0, 0.5),
        }
    }

    pub fn point(self, b: &Rect) -> Point {
        let (fx, fy) = self.at();
        Point::new(b.x + fx * b.width, b.y + fy * b.height)
    }

    /// The box after dragging this grip by `(dx, dy)`; the opposite side stays put and the box
    /// keeps `min`. With `keep_ratio` (Shift) a corner keeps the box's proportions.
    pub fn resize(self, b: &Rect, dx: f64, dy: f64, min: (f64, f64), keep_ratio: bool) -> Rect {
        let (fx, fy) = self.at();
        let (mut x0, mut y0, mut x1, mut y1) = (b.x, b.y, b.x + b.width, b.y + b.height);
        if fx == 0.0 {
            x0 = (x0 + dx).min(x1 - min.0);
        } else if fx == 1.0 {
            x1 = (x1 + dx).max(x0 + min.0);
        }
        if fy == 0.0 {
            y0 = (y0 + dy).min(y1 - min.1);
        } else if fy == 1.0 {
            y1 = (y1 + dy).max(y0 + min.1);
        }
        let corner = fx != 0.5 && fy != 0.5;
        if keep_ratio && corner && b.width > 0.0 && b.height > 0.0 {
            let ratio = b.width / b.height;
            let (w, h) = (x1 - x0, y1 - y0);
            // The larger change wins.
            let (w, h) = if w / b.width >= h / b.height { (w, w / ratio) } else { (h * ratio, h) };
            if fx == 0.0 {
                x0 = x1 - w;
            } else {
                x1 = x0 + w;
            }
            if fy == 0.0 {
                y0 = y1 - h;
            } else {
                y1 = y0 + h;
            }
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// Whether `p` lies on the shape of `node` (its exact outline: ellipse, rhombus, triangle).
pub fn on_node(node: &Node, p: Point) -> bool {
    let b = &node.bounds;
    if p.x < b.x || p.y < b.y || p.x > b.x + b.width || p.y > b.y + b.height {
        return false;
    }
    let c = b.center();
    let (rx, ry) = (b.width / 2.0, b.height / 2.0);
    match Outline::of(&node.kind) {
        Outline::Rectangle => true,
        Outline::Ellipse => ((p.x - c.x) / rx).powi(2) + ((p.y - c.y) / ry).powi(2) <= 1.0,
        Outline::Rhombus => (p.x - c.x).abs() / rx + (p.y - c.y).abs() / ry <= 1.0,
        Outline::Triangle => {
            let [a, b2, c2] = triangle(b);
            let sign = |p1: Point, p2: Point, p3: Point| {
                (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
            };
            let (d1, d2, d3) = (sign(p, a, b2), sign(p, b2, c2), sign(p, c2, a));
            let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
            let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
            !(negative && positive)
        }
    }
}

/// The distance from `p` to a polyline.
pub fn distance_to_line(points: &[Point], p: Point) -> f64 {
    points
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 == 0.0 {
                0.0
            } else {
                (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
            };
            distance(p, Point::new(a.x + t * dx, a.y + t * dy))
        })
        .fold(f64::INFINITY, f64::min)
}

fn contains(r: &Rect, p: Point, grow: f64) -> bool {
    p.x >= r.x - grow
        && p.y >= r.y - grow
        && p.x <= r.x + r.width + grow
        && p.y <= r.y + r.height + grow
}

/// The boxes of a link's label and role, as drawn.
pub fn label_boxes(d: &Diagram, link: &Link) -> (Option<Rect>, Option<Rect>) {
    let Some(points) = points_of(d, link) else { return (None, None) };
    let label = link.label.as_ref().map(|l| label_box(&points, &l.display(), &l.pos).1);
    let role = link.role.as_ref().map(|r| label_box(&points, &r.text, &r.pos).1);
    (label, role)
}

/// The topmost element at `p`, in the reverse of the drawing order: free texts, line labels,
/// nodes, lines (within `tolerance` px). A role counts as its line.
pub fn at(d: &Diagram, p: Point, tolerance: f64) -> Option<Item> {
    if let Some(t) = d.texts.iter().rev().find(|t| contains(&t.bounds, p, 0.0)) {
        return Some(Item::Text(t.id.clone()));
    }
    for link in d.links.iter().rev() {
        let (label, role) = label_boxes(d, link);
        if label.is_some_and(|r| contains(&r, p, 2.0)) {
            return Some(Item::Label(link.id.clone()));
        }
        if role.is_some_and(|r| contains(&r, p, 2.0)) {
            return Some(Item::Link(link.id.clone()));
        }
    }
    if let Some(n) = d.nodes.iter().rev().find(|n| on_node(n, p)) {
        return Some(Item::Node(n.id.clone()));
    }
    d.links
        .iter()
        .rev()
        .filter_map(|l| Some((l, distance_to_line(&points_of(d, l)?, p))))
        .filter(|(_, dist)| *dist <= tolerance)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(l, _)| Item::Link(l.id.clone()))
}

/// The node whose shape is at `p`, topmost first.
pub fn node_at(d: &Diagram, p: Point) -> Option<&Node> {
    d.nodes.iter().rev().find(|n| on_node(n, p))
}

/// What lies wholly inside `r` (a rubber band, as in draw.io): nodes, texts and lines.
pub fn inside(d: &Diagram, r: &Rect) -> Vec<Item> {
    let within = |b: &Rect| {
        b.x >= r.x
            && b.y >= r.y
            && b.x + b.width <= r.x + r.width
            && b.y + b.height <= r.y + r.height
    };
    let mut out: Vec<Item> =
        d.nodes.iter().filter(|n| within(&n.bounds)).map(|n| Item::Node(n.id.clone())).collect();
    for l in &d.links {
        if let Some(points) = points_of(d, l)
            && points.iter().all(|p| within(&Rect::new(p.x, p.y, 0.0, 0.0)))
        {
            out.push(Item::Link(l.id.clone()));
        }
    }
    out.extend(d.texts.iter().filter(|t| within(&t.bounds)).map(|t| Item::Text(t.id.clone())));
    out
}

/// The connection points of a node as ports (relative to its box), as draw.io offers them:
/// a rectangle has its corners and the quarters of each side, an ellipse and a rhombus the
/// corners and middles of their box (projected onto the outline), the IS-A triangle the
/// quarters of its base, its tip and the middles of its sides.
pub fn connection_ports(kind: &NodeKind) -> Vec<Point> {
    let p = Point::new;
    match Outline::of(kind) {
        Outline::Rectangle => {
            let q = [0.0, 0.25, 0.5, 0.75, 1.0];
            let mut out = Vec::with_capacity(16);
            for x in q {
                out.push(p(x, 0.0));
            }
            for y in &q[1..4] {
                out.push(p(0.0, *y));
                out.push(p(1.0, *y));
            }
            for x in q {
                out.push(p(x, 1.0));
            }
            out
        }
        Outline::Ellipse | Outline::Rhombus => vec![
            p(0.0, 0.0),
            p(1.0, 0.0),
            p(0.0, 1.0),
            p(1.0, 1.0),
            p(0.5, 0.0),
            p(0.5, 1.0),
            p(0.0, 0.5),
            p(1.0, 0.5),
        ],
        // draw.io's east-pointing triangle turned to point north: (x, y) → (y, 1 − x).
        Outline::Triangle => {
            vec![p(0.25, 1.0), p(0.5, 1.0), p(0.75, 1.0), p(0.0, 0.5), p(1.0, 0.5), p(0.5, 0.0)]
        }
    }
}

/// The connection point of `node` nearest to `p` within `tolerance`, as port and as point.
pub fn nearest_port(node: &Node, p: Point, tolerance: f64) -> Option<(Point, Point)> {
    connection_ports(&node.kind)
        .into_iter()
        .map(|port| (port, port_point(node, port)))
        .map(|(port, at)| (port, at, distance(at, p)))
        .filter(|(.., dist)| *dist <= tolerance)
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(port, at, _)| (port, at))
}

/// Where a link's ends are drawn.
pub fn ends(d: &Diagram, link: &Link) -> Option<(Point, Point)> {
    let points = points_of(d, link)?;
    Some((*points.first()?, *points.last()?))
}

/// The point on a node's outline in the direction of `toward`, where a floating end would go.
pub fn outline_toward(node: &Node, toward: Point) -> Point {
    perimeter(Outline::of(&node.kind), &node.bounds, toward)
}

/// The box around all the items (nodes, texts, lines, labels), for fitting and pasting.
pub fn bounds_of(d: &Diagram, items: &[Item]) -> Option<Rect> {
    let mut rects: Vec<Rect> = Vec::new();
    for item in items {
        match item {
            Item::Node(id) => rects.extend(d.node(id).map(|n| n.bounds)),
            Item::Text(id) => rects.extend(d.texts.iter().find(|t| t.id == *id).map(|t| t.bounds)),
            Item::Link(id) => {
                if let Some(points) = d.link(id).and_then(|l| points_of(d, l)) {
                    rects.extend(points.iter().map(|p| Rect::new(p.x, p.y, 0.0, 0.0)));
                }
            }
            Item::Label(id) => rects.extend(d.link(id).and_then(|l| label_boxes(d, l).0)),
        }
    }
    let first = rects.first()?;
    let (mut x0, mut y0) = (first.x, first.y);
    let (mut x1, mut y1) = (first.x + first.width, first.y + first.height);
    for r in &rects {
        (x0, y0) = (x0.min(r.x), y0.min(r.y));
        (x1, y1) = (x1.max(r.x + r.width), y1.max(r.y + r.height));
    }
    Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit;

    fn diagram() -> (Diagram, crate::Id, crate::Id, crate::Id, crate::Id) {
        let mut d = Diagram::new("t");
        let e = edit::add_node(&mut d, NodeKind::entity(), "A", Point::new(0.0, 0.0));
        let f = edit::add_node(&mut d, NodeKind::entity(), "B", Point::new(400.0, 0.0));
        let r = edit::add_node(&mut d, NodeKind::relationship(), "r", Point::new(200.0, 0.0));
        let l = edit::connect(&mut d, &r, &e, (None, None)).unwrap();
        edit::connect(&mut d, &r, &f, (None, None)).unwrap();
        (d, e, f, r, l)
    }

    #[test]
    fn shapes_are_hit_on_their_outline_only() {
        let (d, e, _, r, _) = diagram();
        // Rhombus 60 × 40 around (200, 0): its corner region is empty.
        assert_eq!(at(&d, Point::new(200.0, 0.0), 4.0), Some(Item::Node(r.clone())));
        assert_eq!(at(&d, Point::new(225.0, 15.0), 0.0), None);
        assert_eq!(at(&d, Point::new(-45.0, -15.0), 4.0), Some(Item::Node(e)));
        let mut ellipse = d.nodes[0].clone();
        ellipse.kind = NodeKind::attribute();
        assert!(on_node(&ellipse, Point::new(0.0, 0.0)));
        assert!(!on_node(&ellipse, Point::new(-48.0, -18.0)));
        let mut tri = d.nodes[0].clone();
        tri.kind = NodeKind::is_a();
        assert!(on_node(&tri, Point::new(0.0, 15.0)));
        assert!(!on_node(&tri, Point::new(-45.0, -15.0)));
    }

    #[test]
    fn lines_and_labels_are_hit_near_them() {
        let (d, _, _, _, l) = diagram();
        // The line r → A runs at y = 0 from x 50 to 170.
        assert_eq!(at(&d, Point::new(120.0, 3.0), 4.0), Some(Item::Link(l.clone())));
        // The label sits 15 px to the left of r → A, which runs leftwards: below the line.
        assert_eq!(at(&d, Point::new(120.0, -9.0), 4.0), None);
        let (label, _) = label_boxes(&d, d.link(&l).unwrap());
        let c = label.unwrap().center();
        assert_eq!(at(&d, c, 4.0), Some(Item::Label(l)));
    }

    #[test]
    fn a_rubber_band_takes_what_is_wholly_inside() {
        let (d, e, _, r, l) = diagram();
        let got = inside(&d, &Rect::new(-60.0, -30.0, 300.0, 60.0));
        assert_eq!(got, vec![Item::Node(e), Item::Node(r), Item::Link(l)]);
    }

    #[test]
    fn grips_resize_from_the_opposite_side() {
        let b = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(
            Handle::Right.resize(&b, 20.0, 99.0, (20.0, 10.0), false),
            Rect::new(0.0, 0.0, 120.0, 40.0)
        );
        assert_eq!(
            Handle::TopLeft.resize(&b, 10.0, 10.0, (20.0, 10.0), false),
            Rect::new(10.0, 10.0, 90.0, 30.0)
        );
        assert_eq!(
            Handle::Left.resize(&b, 500.0, 0.0, (20.0, 10.0), false),
            Rect::new(80.0, 0.0, 20.0, 40.0)
        );
        assert_eq!(
            Handle::BottomRight.resize(&b, 100.0, 0.0, (20.0, 10.0), true),
            Rect::new(0.0, 0.0, 200.0, 80.0)
        );
        assert_eq!(Handle::Top.point(&b), Point::new(50.0, 0.0));
    }

    #[test]
    fn connection_points_are_draw_ios() {
        assert_eq!(connection_ports(&NodeKind::entity()).len(), 16);
        assert_eq!(connection_ports(&NodeKind::attribute()).len(), 8);
        assert_eq!(connection_ports(&NodeKind::relationship()).len(), 8);
        assert_eq!(connection_ports(&NodeKind::is_a()).len(), 6);
        let (d, e, ..) = diagram();
        let n = d.node(&e).unwrap();
        // The middle of the right side of A (100 × 40 around the origin).
        let (port, at) = nearest_port(n, Point::new(52.0, 1.0), 6.0).unwrap();
        assert_eq!(port, Point::new(1.0, 0.5));
        assert_eq!(at, Point::new(50.0, 0.0));
        assert!(nearest_port(n, Point::new(20.0, 5.0), 6.0).is_none());
    }

    #[test]
    fn bounds_cover_the_items() {
        let (d, e, f, ..) = diagram();
        let b = bounds_of(&d, &[Item::Node(e), Item::Node(f)]).unwrap();
        assert_eq!(b, Rect::new(-50.0, -20.0, 500.0, 40.0));
        assert_eq!(bounds_of(&d, &[]), None);
    }
}
