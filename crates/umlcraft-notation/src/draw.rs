//! The outlines of a scene as simple drawing primitives, so the canvas and image export draw
//! the same figures. Texts stay in [`Row`](crate::scene::Row) and
//! [`EdgeLabel`](crate::scene::EdgeLabel); this module only says where their baselines go.

use umlcraft_model::{Point, Rect};

use crate::scene::{
    EdgeLabel, EdgeShape, EndMarker, Figure, LABEL_HEIGHT, LabelAlign, LineStyle, NodeShape,
};
use crate::sequence::SequenceLayout;

/// How far below the top of its line Java puts the baseline of "Dialog", relative to the font
/// size (12.06 px at 12 px).
pub const BASELINE: f32 = 12.06 / 12.0;
/// Astah underlines 2.3 px below the baseline (at 12 px), relative to the font size.
pub const UNDERLINE: f32 = 14.34 / 12.0;
/// Edge labels sit about 1 px lower than rows: Astah puts their baseline 13 px below the
/// label's top.
pub const LABEL_SHIFT: f32 = 0.94;

/// A color role; the canvas maps it to its theme, export to black on white.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Inside of figures.
    Fill,
    /// Lines, borders and filled arrow heads.
    Line,
    /// The diagram background, e.g. inside a hollow diamond.
    Background,
    /// See-through fill of system boundaries.
    Translucent,
    /// Border of figures UmlCraft has no notation for.
    Muted,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// An open line in [`Paint::Line`].
    Polyline { points: Vec<Point>, width: f32, style: LineStyle },
    /// A closed polygon with a 1 px [`Paint::Line`] border.
    Polygon { points: Vec<Point>, fill: Paint },
    /// An ellipse in [`Paint::Fill`] with a 1 px [`Paint::Line`] border.
    Ellipse { center: Point, rx: f32, ry: f32 },
    /// A rectangle whose border of `width` is centered on its outline, like a Java stroke.
    Frame { rect: Rect, width: f32, fill: Paint, border: Paint, style: LineStyle },
}

fn line(points: Vec<Point>, style: LineStyle) -> Primitive {
    Primitive::Polyline { points, width: 1.0, style }
}

fn frame(rect: Rect, fill: Paint) -> Primitive {
    Primitive::Frame { rect, width: 1.0, fill, border: Paint::Line, style: LineStyle::Solid }
}

/// The outline of a node, in drawing order.
pub fn figure(node: &NodeShape) -> Vec<Primitive> {
    let r = node.rect;
    let solid = LineStyle::Solid;
    match &node.figure {
        Figure::Classifier { separators } => {
            // Astah draws classifier boxes with a 2 px border, compartment lines with 1 px.
            let mut result = vec![Primitive::Frame {
                rect: r,
                width: 2.0,
                fill: Paint::Fill,
                border: Paint::Line,
                style: solid,
            }];
            result.extend(
                separators
                    .iter()
                    .map(|&y| line(vec![Point::new(r.x, y), Point::new(r.right(), y)], solid)),
            );
            result
        }
        Figure::Lollipop => {
            vec![Primitive::Ellipse { center: r.center(), rx: r.width / 2.0, ry: r.height / 2.0 }]
        }
        Figure::Package { tab } => {
            let body = Rect::new(r.x, r.y + tab.height, r.width, r.height - tab.height);
            vec![frame(*tab, Paint::Fill), frame(body, Paint::Fill)]
        }
        Figure::Actor => {
            let cx = r.x + r.width / 2.0;
            let head = (r.width.min(r.height) * 0.18).max(6.0);
            let neck = r.y + head * 2.0;
            let hip = r.y + r.height * 0.62;
            let arms = r.y + r.height * 0.38;
            vec![
                Primitive::Ellipse { center: Point::new(cx, r.y + head), rx: head, ry: head },
                line(vec![Point::new(cx, neck), Point::new(cx, hip)], solid),
                line(vec![Point::new(r.x, arms), Point::new(r.right(), arms)], solid),
                line(
                    vec![
                        Point::new(r.x, r.bottom()),
                        Point::new(cx, hip),
                        Point::new(r.right(), r.bottom()),
                    ],
                    solid,
                ),
            ]
        }
        Figure::UseCase { separator } => {
            let (rx, ry) = (r.width / 2.0, r.height / 2.0);
            let center = r.center();
            let mut result = vec![Primitive::Ellipse { center, rx, ry }];
            // A chord across the ellipse at the separator's height.
            if let Some(y) = *separator {
                let t = ((y - center.y) / ry).clamp(-1.0, 1.0);
                let half = rx * (1.0 - t * t).sqrt();
                result.push(line(
                    vec![Point::new(center.x - half, y), Point::new(center.x + half, y)],
                    solid,
                ));
            }
            result
        }
        Figure::Boundary => vec![frame(r, Paint::Translucent)],
        Figure::Lifeline { head } => {
            let x = r.x + r.width / 2.0;
            vec![
                frame(*head, Paint::Fill),
                line(
                    vec![Point::new(x, head.bottom()), Point::new(x, r.bottom())],
                    LineStyle::Dashed,
                ),
            ]
        }
        Figure::Fragment { tab, separators } => {
            // The operator sits in a pentagon with a cut-off bottom right corner.
            let cut = 6.0;
            let mut result = vec![
                frame(r, Paint::None),
                Primitive::Polygon {
                    points: vec![
                        tab.origin(),
                        Point::new(tab.right(), tab.y),
                        Point::new(tab.right(), tab.bottom() - cut),
                        Point::new(tab.right() - cut, tab.bottom()),
                        Point::new(tab.x, tab.bottom()),
                    ],
                    fill: Paint::Fill,
                },
            ];
            result.extend(separators.iter().map(|&y| {
                line(vec![Point::new(r.x, y), Point::new(r.right(), y)], LineStyle::Dashed)
            }));
            result
        }
        Figure::Note => {
            let fold = 10.0;
            vec![Primitive::Polygon {
                points: vec![
                    r.origin(),
                    Point::new(r.right() - fold, r.y),
                    Point::new(r.right(), r.y + fold),
                    Point::new(r.right(), r.bottom()),
                    Point::new(r.x, r.bottom()),
                ],
                fill: Paint::Fill,
            }]
        }
        Figure::Other => vec![Primitive::Frame {
            rect: r,
            width: 1.0,
            fill: Paint::Fill,
            border: Paint::Muted,
            style: LineStyle::Dashed,
        }],
    }
}

/// The end markers of an edge; the line itself is [`EdgeShape::points`].
pub fn markers(edge: &EdgeShape) -> Vec<Primitive> {
    let pts = &edge.points;
    let n = pts.len();
    let mut result = marker(pts[n - 1], pts[n - 2], edge.target_marker);
    result.extend(marker(pts[0], pts[1], edge.source_marker));
    result
}

fn marker(tip: Point, from: Point, marker: EndMarker) -> Vec<Primitive> {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return Vec::new();
    }
    let (ux, uy) = (dx / len, dy / len);
    let at = |back: f32, side: f32| {
        Point::new(tip.x - ux * back - uy * side, tip.y - uy * back + ux * side)
    };
    let solid = LineStyle::Solid;
    // Astah's arrow heads: 12 px sides at 30° to the line.
    let (back, side) = (12.0 * 0.866_025_4, 6.0);
    match marker {
        EndMarker::None => Vec::new(),
        EndMarker::OpenArrow => vec![line(vec![at(back, side), tip, at(back, -side)], solid)],
        EndMarker::FilledArrow => {
            vec![Primitive::Polygon {
                points: vec![tip, at(back, side), at(back, -side)],
                fill: Paint::Line,
            }]
        }
        EndMarker::HollowTriangle => {
            vec![Primitive::Polygon {
                points: vec![tip, at(11.0, 6.5), at(11.0, -6.5)],
                fill: Paint::Fill,
            }]
        }
        EndMarker::Cross => {
            // 19 px from the end, arms of 5 px at 45° to the line.
            let arm = 5.0 * std::f32::consts::FRAC_1_SQRT_2;
            vec![
                line(vec![at(19.0 - arm, arm), at(19.0 + arm, -arm)], solid),
                line(vec![at(19.0 - arm, -arm), at(19.0 + arm, arm)], solid),
            ]
        }
        EndMarker::HollowDiamond | EndMarker::FilledDiamond => {
            let fill =
                if marker == EndMarker::FilledDiamond { Paint::Line } else { Paint::Background };
            vec![Primitive::Polygon {
                points: vec![tip, at(8.0, 5.0), at(16.0, 0.0), at(8.0, -5.0)],
                fill,
            }]
        }
    }
}

/// Activation bars and the crosses of destroyed lifelines.
pub fn sequence(layout: &SequenceLayout) -> Vec<Primitive> {
    let mut result: Vec<Primitive> =
        layout.activations.iter().map(|&bar| frame(bar, Paint::Fill)).collect();
    let d = 8.0;
    for &p in &layout.destructions {
        for points in
            [vec![p.offset(-d, -d), p.offset(d, d)], vec![p.offset(-d, d), p.offset(d, -d)]]
        {
            result.push(Primitive::Polyline { points, width: 1.5, style: LineStyle::Solid });
        }
    }
    result
}

/// The line box an edge label is drawn in; its alignment does the actual placement.
pub fn label_area(label: &EdgeLabel) -> Rect {
    // Wide enough for any role name.
    const BOX: f32 = 200.0;
    let Point { x, y } = label.anchor;
    let y = y + LABEL_SHIFT;
    match label.align {
        LabelAlign::Left => Rect::new(x, y, BOX, LABEL_HEIGHT),
        LabelAlign::Center => Rect::new(x - BOX / 2.0, y, BOX, LABEL_HEIGHT),
        LabelAlign::Right => Rect::new(x - BOX, y, BOX, LABEL_HEIGHT),
    }
}

/// Left edge of a text of `width` aligned in `area`.
pub fn text_left(area: Rect, width: f32, align: LabelAlign) -> f32 {
    match align {
        LabelAlign::Left => area.x,
        LabelAlign::Center => area.x + (area.width - width) / 2.0,
        LabelAlign::Right => area.right() - width,
    }
}
