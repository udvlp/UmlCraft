//! Lining up nodes while they are moved: the dragged selection's center snaps to another node's
//! center when it comes close, and its edges to the other node's edges (left or right, top or
//! bottom). Guides show what it lines up with. Two nodes whose centers line up are joined by a
//! straight horizontal or vertical line.

use umlcraft_model::{Point, Rect};

/// How near, in screen pixels, an edge or center must come to another node's to snap to it.
pub const REACH: f32 = 6.0;

/// What a dragged selection may line up with, taken when the drag starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Targets {
    /// The dragged nodes together, where they were.
    moved: Rect,
    /// The other nodes of the diagram.
    others: Vec<Rect>,
}

/// A node's lines across one axis: its center, then its two edges.
type Lines = (f32, [f32; 2]);

fn xs(r: &Rect) -> Lines {
    (r.center().x, [r.x, r.right()])
}

fn ys(r: &Rect) -> Lines {
    (r.center().y, [r.y, r.bottom()])
}

/// Equal up to the rounding of adding offsets.
fn same(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

/// Whether `theirs` has a line on `v`, a center if `center`, else an edge.
fn has(theirs: Lines, v: f32, center: bool) -> bool {
    match center {
        true => same(theirs.0, v),
        false => theirs.1.iter().any(|&e| same(e, v)),
    }
}

impl Targets {
    /// `None` when nothing that lines up is moved (only free lines, say).
    pub fn new(moved: impl IntoIterator<Item = Rect>, others: Vec<Rect>) -> Option<Self> {
        let moved = moved.into_iter().reduce(|a, b| a.union(&b))?;
        Some(Self { moved, others })
    }

    /// The offsets across and down at which the selection, dragged by `(dx, dy)`, lines up with
    /// another node, where one at most `reach` away does (`None` across or down where none
    /// does), and the guides to draw, each from one end to the other.
    pub fn snap(
        &self,
        dx: f32,
        dy: f32,
        reach: f32,
    ) -> (Option<f32>, Option<f32>, Vec<[Point; 2]>) {
        let at = self.moved.translate(dx, dy);
        let sx = nearest(xs(&at), self.others.iter().map(xs), reach).map(|s| dx + s);
        let sy = nearest(ys(&at), self.others.iter().map(ys), reach).map(|s| dy + s);
        let at = self.moved.translate(sx.unwrap_or(dx), sy.unwrap_or(dy));
        let mut guides = Vec::new();
        if sx.is_some() {
            let (center, edges) = xs(&at);
            let lines = [(center, true), (edges[0], false), (edges[1], false)];
            for (x, center) in lines {
                let lined: Vec<&Rect> =
                    self.others.iter().filter(|r| has(xs(r), x, center)).collect();
                if !lined.is_empty() {
                    let top = lined.iter().map(|r| r.y).fold(at.y, f32::min);
                    let bottom = lined.iter().map(|r| r.bottom()).fold(at.bottom(), f32::max);
                    guides.push([Point::new(x, top), Point::new(x, bottom)]);
                }
            }
        }
        if sy.is_some() {
            let (center, edges) = ys(&at);
            let lines = [(center, true), (edges[0], false), (edges[1], false)];
            for (y, center) in lines {
                let lined: Vec<&Rect> =
                    self.others.iter().filter(|r| has(ys(r), y, center)).collect();
                if !lined.is_empty() {
                    let left = lined.iter().map(|r| r.x).fold(at.x, f32::min);
                    let right = lined.iter().map(|r| r.right()).fold(at.right(), f32::max);
                    guides.push([Point::new(left, y), Point::new(right, y)]);
                }
            }
        }
        (sx, sy, guides)
    }
}

/// The smallest shift within `reach` that puts `own` center on another's center or one of its
/// edges on another's edge; a center wins a tie, since lining up centers straightens lines.
fn nearest(own: Lines, others: impl Iterator<Item = Lines>, reach: f32) -> Option<f32> {
    let mut best: Option<f32> = None;
    let mut consider = |d: f32| {
        if d.abs() <= reach && best.is_none_or(|b| d.abs() < b.abs()) {
            best = Some(d);
        }
    };
    for (center, edges) in others {
        consider(center - own.0);
        for e in edges {
            for o in own.1 {
                consider(e - o);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity() -> Rect {
        Rect::new(0.0, 0.0, 100.0, 40.0)
    }

    #[test]
    fn centers_line_up_when_close() {
        // A relationship diamond to the right of the entity, its middle 11 above the entity's
        // (9 against 20), out of reach of 6; its edges are farther off still.
        let t = Targets::new([Rect::new(200.0, -31.0, 80.0, 80.0)], vec![entity()]).unwrap();
        assert_eq!(t.snap(0.0, 0.0, 6.0), (None, None, vec![]));
        // Moved down by 10, its middle is 1 above the entity's and snaps to it.
        let (dx, dy, guides) = t.snap(0.0, 10.0, 6.0);
        assert_eq!((dx, dy), (None, Some(11.0)));
        assert_eq!(guides, vec![[Point::new(0.0, 20.0), Point::new(280.0, 20.0)]]);
    }

    #[test]
    fn the_nearest_line_wins() {
        // Top 2 away from the entity's top, middle 4 away from its middle: the top snaps.
        let t = Targets::new([Rect::new(200.0, 2.0, 80.0, 44.0)], vec![entity()]).unwrap();
        assert_eq!(t.snap(0.0, 0.0, 6.0).1, Some(-2.0));
        // A diamond 10 higher than the entity lines up its top at 0 and its middle at -5:
        // between the two, whichever is nearer.
        let t = Targets::new([Rect::new(200.0, 0.0, 80.0, 50.0)], vec![entity()]).unwrap();
        assert_eq!(t.snap(0.0, -1.0, 6.0).1, Some(0.0));
        assert_eq!(t.snap(0.0, -4.0, 6.0).1, Some(-5.0));
    }

    #[test]
    fn edges_line_up_with_edges() {
        // Below the entity, the left edge 3 to the right of the entity's.
        let t = Targets::new([Rect::new(3.0, 100.0, 60.0, 40.0)], vec![entity()]).unwrap();
        let (dx, dy, guides) = t.snap(0.0, 0.0, 6.0);
        assert_eq!((dx, dy), (Some(-3.0), None));
        assert_eq!(guides, vec![[Point::new(0.0, 0.0), Point::new(0.0, 140.0)]]);
    }

    #[test]
    fn an_edge_does_not_snap_to_a_center() {
        // The bottom 2 above the entity's middle, nothing else near.
        let t = Targets::new([Rect::new(200.0, -82.0, 80.0, 100.0)], vec![entity()]).unwrap();
        assert_eq!(t.snap(0.0, 0.0, 6.0), (None, None, vec![]));
    }

    #[test]
    fn nothing_to_line_up() {
        assert!(Targets::new([], vec![entity()]).is_none());
        let alone = Targets::new([Rect::new(0.0, 0.0, 10.0, 10.0)], vec![]).unwrap();
        assert_eq!(alone.snap(3.0, 4.0, 6.0), (None, None, vec![]));
    }
}
