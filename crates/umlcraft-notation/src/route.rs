//! How edges run between their ends: Astah's line styles. Straight lines go through their
//! waypoints. Right-angled lines have horizontal and vertical segments only, with the waypoints
//! as corners. Curves are one cubic Bézier curve from end to end whose control points are the
//! first and the last waypoint (or the other end, when there are none), as Astah draws them.
//!
//! The corners Astah makes when a line gets right angles were measured through its API; see
//! "Line styles" in `docs/asta-format.md`.

use umlcraft_model::{Point, Rect};

/// What an edge stores about its course: waypoints and where it aims inside its end nodes,
/// relative to their bounds (the center when unset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Route {
    pub waypoints: Vec<Point>,
    pub source_anchor: Option<Point>,
    pub target_anchor: Option<Point>,
}

/// Coordinates closer than this count as lined up (Astah's own points are not always exact).
const SLACK: f32 = 0.5;

fn same(a: f32, b: f32) -> bool {
    (a - b).abs() < SLACK
}

/// The point at relative position `anchor` inside `rect`; its center when unset.
pub fn anchor_point(rect: Rect, anchor: Option<Point>) -> Point {
    match anchor {
        Some(a) => Point::new(rect.x + a.x * rect.width, rect.y + a.y * rect.height),
        None => rect.center(),
    }
}

/// `p` relative to `rect`, as anchors are stored.
fn relative(rect: Rect, p: Point) -> Point {
    let fraction =
        |v: f32, start: f32, size: f32| if size > 0.0 { (v - start) / size } else { 0.5 };
    Point::new(fraction(p.x, rect.x, rect.width), fraction(p.y, rect.y, rect.height))
}

/// Corners of a right-angled line from `from` in `source` to `to` in `target` that has no
/// waypoints of its own, as Astah makes them: none when the ends line up; when the nodes overlap
/// horizontally, a step down (or up) with the crossing in the middle; when they overlap
/// vertically, a step sideways; otherwise one corner straight below (or above) the source.
pub fn default_corners(source: Rect, from: Point, target: Rect, to: Point) -> Vec<Point> {
    if same(from.x, to.x) || same(from.y, to.y) {
        return Vec::new();
    }
    let overlap_x = source.x < target.right() && target.x < source.right();
    let overlap_y = source.y < target.bottom() && target.y < source.bottom();
    if overlap_x {
        let y = (from.y + to.y) / 2.0;
        vec![Point::new(from.x, y), Point::new(to.x, y)]
    } else if overlap_y {
        let x = (from.x + to.x) / 2.0;
        vec![Point::new(x, from.y), Point::new(x, to.y)]
    } else {
        vec![Point::new(from.x, to.y)]
    }
}

/// The corners of a line through `waypoints` turned into right angles, as Astah does when the
/// line style changes. The line gets as many corners as it had waypoints. Its segments alternate
/// between horizontal and vertical; the last one runs along the larger distance from the last
/// waypoint to `to`. Each inner segment lies on the waypoint at its end (so the first waypoint
/// only counts through the number of corners).
pub fn orthogonalize(
    source: Rect,
    from: Point,
    waypoints: &[Point],
    target: Rect,
    to: Point,
) -> Vec<Point> {
    let Some(&last) = waypoints.last() else {
        return default_corners(source, from, target, to);
    };
    let n = waypoints.len();
    let last_horizontal = (last.x - to.x).abs() >= (last.y - to.y).abs();
    // Segment k runs from corner k to corner k + 1; corner 0 is `from`, corner n + 1 is `to`.
    let horizontal = |k: usize| (n - k).is_multiple_of(2) == last_horizontal;
    // Where a segment lies: its y if it is horizontal, its x if it is vertical.
    let level = |k: usize| {
        let p = match k {
            0 => from,
            k if k == n => to,
            k => waypoints[k],
        };
        if horizontal(k) { p.y } else { p.x }
    };
    let corners = (1..=n)
        .map(|k| {
            if horizontal(k) {
                Point::new(level(k - 1), level(k))
            } else {
                Point::new(level(k), level(k - 1))
            }
        })
        .collect::<Vec<_>>();
    simplify(from, &corners, to)
}

/// The corners of a right-angled line whose ends moved (or whose nodes grew): the first and the
/// last segment keep their direction and follow the ends; a single corner stays on its side.
/// Corners that do not form right angles are made to (`orthogonalize`).
pub fn fit_corners(
    source: Rect,
    from: Point,
    waypoints: &[Point],
    target: Rect,
    to: Point,
) -> Vec<Point> {
    let n = waypoints.len();
    let mut corners = waypoints.to_vec();
    match n {
        0 => return default_corners(source, from, target, to),
        1 => {
            let below = Point::new(from.x, to.y);
            let beside = Point::new(to.x, from.y);
            let c = corners[0];
            corners[0] = if c.distance(below) <= c.distance(beside) { below } else { beside };
        }
        _ => {
            let square = corners.windows(2).all(|w| same(w[0].x, w[1].x) || same(w[0].y, w[1].y));
            if !square {
                return orthogonalize(source, from, waypoints, target, to);
            }
            // After a horizontal second segment the first one is vertical, and so on.
            if same(corners[0].y, corners[1].y) {
                corners[0].x = from.x;
            } else {
                corners[0].y = from.y;
            }
            if same(corners[n - 2].y, corners[n - 1].y) {
                corners[n - 1].x = to.x;
            } else {
                corners[n - 1].y = to.y;
            }
        }
    }
    simplify(from, &corners, to)
}

/// `corners` without those that do not turn the line: repeated points and points in the middle
/// of a straight horizontal or vertical stretch.
pub fn simplify(from: Point, corners: &[Point], to: Point) -> Vec<Point> {
    let mut points = Vec::with_capacity(corners.len() + 2);
    points.push(from);
    points.extend_from_slice(corners);
    points.push(to);
    let mut i = 1;
    while i + 1 < points.len() {
        let (a, p, b) = (points[i - 1], points[i], points[i + 1]);
        let repeated = same(a.x, p.x) && same(a.y, p.y);
        let straight = (same(a.x, p.x) && same(p.x, b.x)) || (same(a.y, p.y) && same(p.y, b.y));
        if repeated || straight {
            points.remove(i);
            i = i.max(2) - 1;
        } else {
            i += 1;
        }
    }
    points[1..points.len() - 1].to_vec()
}

/// The route after dragging segment `index` of a right-angled line across to `to`. `points` is
/// the line as drawn (border to border, corners between) and `ends` the bounds of its end nodes.
/// An inner segment moves with both of its corners; the first and the last segment move the end
/// of the line inside its node, as far as the node reaches.
pub fn drag_segment(
    points: &[Point],
    route: &Route,
    ends: [Rect; 2],
    index: usize,
    to: Point,
) -> Route {
    let n = points.len();
    if index + 1 >= n {
        return route.clone();
    }
    let (a, b) = (points[index], points[index + 1]);
    let horizontal = same(a.y, b.y);
    // How far the segment may go: the nodes its ends sit in limit it.
    let (mut low, mut high) = (f32::NEG_INFINITY, f32::INFINITY);
    let span = |r: Rect| if horizontal { (r.y, r.bottom()) } else { (r.x, r.right()) };
    let at_source = index == 0;
    let at_target = index + 2 == n;
    for (touches, rect) in [(at_source, ends[0]), (at_target, ends[1])] {
        if touches {
            let (l, h) = span(rect);
            (low, high) = (low.max(l), high.min(h));
        }
    }
    if low > high {
        return route.clone();
    }
    let level = if horizontal { to.y } else { to.x }.clamp(low, high);
    let put = |p: &mut Point| {
        if horizontal {
            p.y = level;
        } else {
            p.x = level;
        }
    };
    let mut corners = points[1..n - 1].to_vec();
    for i in [index, index + 1] {
        if (1..n - 1).contains(&i) {
            put(&mut corners[i - 1]);
        }
    }
    let mut result = route.clone();
    let mut from = anchor_point(ends[0], route.source_anchor);
    let mut target_point = anchor_point(ends[1], route.target_anchor);
    if at_source {
        put(&mut from);
        result.source_anchor = Some(relative(ends[0], from));
    }
    if at_target {
        put(&mut target_point);
        result.target_anchor = Some(relative(ends[1], target_point));
    }
    result.waypoints = simplify(from, &corners, target_point);
    result
}

/// The control points of the curve along `points` (border, waypoints, border).
pub fn bezier(points: &[Point]) -> [Point; 4] {
    let n = points.len();
    [points[0], points[1], points[n - 2], points[n - 1]]
}

fn bezier_at(c: &[Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    let (a, b, cc, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point::new(
        a * c[0].x + b * c[1].x + cc * c[2].x + d * c[3].x,
        a * c[0].y + b * c[1].y + cc * c[2].y + d * c[3].y,
    )
}

/// The curve as a polyline, for painting and hit testing.
pub fn flatten(c: &[Point; 4]) -> Vec<Point> {
    const STEPS: usize = 32;
    (0..=STEPS).map(|i| bezier_at(c, i as f32 / STEPS as f32)).collect()
}

/// The segment whose middle carries an edge's name: the middle segment of a polyline, or a short
/// piece of a curve around its middle.
pub fn middle_segment(points: &[Point], curve: bool) -> (Point, Point) {
    let n = points.len();
    if !curve {
        return (points[n / 2 - 1], points[n / 2]);
    }
    let c = bezier(points);
    let (a, b) = (bezier_at(&c, 0.49), bezier_at(&c, 0.51));
    let m = bezier_at(&c, 0.5);
    let half = Point::new((b.x - a.x) / 2.0, (b.y - a.y) / 2.0);
    (m.offset(-half.x, -half.y), m.offset(half.x, half.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// A 40 × 42 node with its center at `c`.
    fn node(c: Point) -> Rect {
        Rect::new(c.x - 20.0, c.y - 21.0, 40.0, 42.0)
    }

    #[test]
    fn default_corners_follow_astah() {
        // Cases measured through Astah's API (centers of 40 × 42 nodes).
        let from = p(20.0, 21.0);
        let corners = |to: Point| default_corners(node(from), from, node(to), to);
        assert_eq!(corners(p(320.0, 171.0)), vec![p(20.0, 171.0)]);
        assert_eq!(corners(p(-280.0, -129.0)), vec![p(20.0, -129.0)]);
        assert_eq!(corners(p(320.0, 21.0)), vec![]);
        // Overlapping horizontally: a step down through the middle.
        assert_eq!(corners(p(41.0, 171.0)), vec![p(20.0, 96.0), p(41.0, 96.0)]);
        // Overlapping vertically: a step sideways.
        assert_eq!(corners(p(320.0, 31.0)), vec![p(170.0, 21.0), p(170.0, 31.0)]);
    }

    #[test]
    fn orthogonalize_follows_astah() {
        let (from, to) = (p(20.0, 21.0), p(320.0, 171.0));
        let ortho = |w: &[Point]| orthogonalize(node(from), from, w, node(to), to);
        assert_eq!(ortho(&[p(150.0, 0.0)]), vec![p(320.0, 21.0)]);
        assert_eq!(ortho(&[p(150.0, 100.0)]), vec![p(20.0, 171.0)]);
        assert_eq!(ortho(&[p(-50.0, 100.0)]), vec![p(20.0, 171.0)]);
        assert_eq!(ortho(&[p(150.0, -50.0)]), vec![p(320.0, 21.0)]);
        assert_eq!(
            ortho(&[p(100.0, 50.0), p(200.0, 100.0)]),
            vec![p(200.0, 21.0), p(200.0, 171.0)]
        );
        assert_eq!(
            ortho(&[p(100.0, -50.0), p(200.0, 200.0)]),
            vec![p(200.0, 21.0), p(200.0, 171.0)]
        );
        assert_eq!(
            ortho(&[p(50.0, 100.0), p(200.0, 50.0), p(250.0, 120.0)]),
            vec![p(20.0, 50.0), p(250.0, 50.0), p(250.0, 171.0)]
        );
    }

    #[test]
    fn corners_follow_moved_ends() {
        let from = p(20.0, 21.0);
        let corners = vec![p(200.0, 21.0), p(200.0, 171.0)];
        // The target moved down: the last segment follows, the middle one stays.
        let moved = p(330.0, 211.0);
        assert_eq!(
            fit_corners(node(from), from, &corners, node(moved), moved),
            vec![p(200.0, 21.0), p(200.0, 211.0)]
        );
        // A single corner stays on its side.
        let fitted = fit_corners(node(from), from, &[p(20.0, 171.0)], node(moved), moved);
        assert_eq!(fitted, vec![p(20.0, 211.0)]);
        // Lined up: no corner left.
        let below = p(20.0, 200.0);
        assert_eq!(fit_corners(node(from), from, &[p(20.0, 171.0)], node(below), below), vec![]);
    }

    #[test]
    fn dragging_segments() {
        let (a, b) = (node(p(20.0, 21.0)), node(p(320.0, 171.0)));
        let route = Route { waypoints: vec![p(200.0, 21.0), p(200.0, 171.0)], ..Route::default() };
        let points = [p(40.0, 21.0), p(200.0, 21.0), p(200.0, 171.0), p(300.0, 171.0)];
        // The middle segment moves sideways with both corners.
        let moved = drag_segment(&points, &route, [a, b], 1, p(150.0, 90.0));
        assert_eq!(moved.waypoints, vec![p(150.0, 21.0), p(150.0, 171.0)]);
        assert_eq!((moved.source_anchor, moved.target_anchor), (None, None));
        // The first segment moves the end inside the source, at most to its border.
        let moved = drag_segment(&points, &route, [a, b], 0, p(100.0, 35.0));
        assert_eq!(moved.waypoints, vec![p(200.0, 35.0), p(200.0, 171.0)]);
        let anchor = moved.source_anchor.unwrap();
        assert_eq!((anchor.x, anchor.y), (0.5, 35.0 / 42.0));
        let moved = drag_segment(&points, &route, [a, b], 0, p(100.0, 500.0));
        assert_eq!(moved.waypoints[0], p(200.0, 42.0));
        // Moving a segment onto its neighbor's line removes the corners in between.
        let moved = drag_segment(&points, &route, [a, b], 1, p(320.0, 90.0));
        assert_eq!(moved.waypoints, vec![p(320.0, 21.0)]);
    }

    #[test]
    fn curves_use_the_outer_waypoints_as_control_points() {
        let points = [p(90.0, 667.77), p(200.0, 650.0), p(350.0, 800.88)];
        assert_eq!(bezier(&points), [points[0], points[1], points[1], points[2]]);
        let line = flatten(&bezier(&points));
        assert_eq!((line[0], line[line.len() - 1]), (points[0], points[2]));
        // Without waypoints the curve is the straight line.
        let straight = flatten(&bezier(&[p(0.0, 0.0), p(100.0, 50.0)]));
        assert!(straight.iter().all(|q| (q.y - q.x / 2.0).abs() < 1e-3));
    }
}
