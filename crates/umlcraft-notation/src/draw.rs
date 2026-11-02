//! The outlines of a scene as simple drawing primitives, so the canvas and image export draw
//! the same figures. Texts stay in [`Row`](crate::scene::Row) and
//! [`EdgeLabel`](crate::scene::EdgeLabel); this module only says where their baselines go.

use std::f32::consts::FRAC_1_SQRT_2;

use umlcraft_model::{Point, Rect};

use crate::route;
use crate::scene::{
    EdgeLabel, EdgeShape, EndMarker, Figure, HeadIcon, LABEL_HEIGHT, LabelAlign, LineStyle,
    NodeShape,
};
use crate::sequence::SequenceLayout;

/// How far below the top of its line Astah puts the baseline of "Dialog" at `size`: Java's
/// whole-pixel ascent, one more than the size (13 at 12 px, 15 at 14 px).
pub fn baseline(size: f32) -> f32 {
    size * 12.06 / 12.0
}
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
    /// See-through fill of activity partitions.
    Translucent,
    /// Border of figures UmlCraft has no notation for.
    Muted,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// An open line in [`Paint::Line`].
    Polyline { points: Vec<Point>, width: f32, style: LineStyle },
    /// A cubic Bézier curve in [`Paint::Line`]: start, two control points, end.
    Curve { points: [Point; 4], width: f32, style: LineStyle },
    /// A closed polygon with a 1 px [`Paint::Line`] border.
    Polygon { points: Vec<Point>, fill: Paint },
    /// An ellipse with a [`Paint::Line`] border of `width`.
    Ellipse { center: Point, rx: f32, ry: f32, fill: Paint, width: f32 },
    /// A rectangle whose border of `width` is centered on its outline, like a Java stroke.
    /// `radius` rounds its corners.
    Frame { rect: Rect, width: f32, radius: f32, fill: Paint, border: Paint, style: LineStyle },
}

impl Primitive {
    /// The area the primitive covers, its line widths left aside.
    pub fn bounds(&self) -> Option<Rect> {
        let around = |points: &[Point]| {
            points.iter().map(|&p| Rect::from_corners(p, p)).reduce(|a, b| a.union(&b))
        };
        match self {
            Primitive::Polyline { points, .. } | Primitive::Polygon { points, .. } => {
                around(points)
            }
            Primitive::Curve { points, .. } => around(points),
            Primitive::Ellipse { center, rx, ry, .. } => {
                Some(Rect::new(center.x - rx, center.y - ry, 2.0 * rx, 2.0 * ry))
            }
            Primitive::Frame { rect, .. } => Some(*rect),
        }
    }
}

fn line(points: Vec<Point>, style: LineStyle) -> Primitive {
    Primitive::Polyline { points, width: 1.0, style }
}

fn frame(rect: Rect, fill: Paint) -> Primitive {
    Primitive::Frame {
        rect,
        width: 1.0,
        radius: 0.0,
        fill,
        border: Paint::Line,
        style: LineStyle::Solid,
    }
}

/// Astah draws components and artifacts with 2 px lines.
fn thick_frame(rect: Rect) -> Primitive {
    Primitive::Frame {
        rect,
        width: 2.0,
        radius: 0.0,
        fill: Paint::Fill,
        border: Paint::Line,
        style: LineStyle::Solid,
    }
}

fn ellipse(center: Point, rx: f32, ry: f32) -> Primitive {
    Primitive::Ellipse { center, rx, ry, fill: Paint::Fill, width: 1.0 }
}

/// Astah draws lifeline heads and their icons with 2 px lines.
fn thick(points: Vec<Point>) -> Primitive {
    Primitive::Polyline { points, width: 2.0, style: LineStyle::Solid }
}

/// The tab of a frame: down from the top at `tab.right()`, a cut corner of 10, along the
/// bottom to the left edge. Diagram frames start the line 1 below the top.
fn tab_line(tab: Rect, top: f32) -> Primitive {
    line(
        vec![
            Point::new(tab.right(), tab.y + top),
            Point::new(tab.right(), tab.bottom() - TAB_CUT),
            Point::new(tab.right() - TAB_CUT, tab.bottom()),
            Point::new(tab.x, tab.bottom()),
        ],
        LineStyle::Solid,
    )
}

const TAB_CUT: f32 = 10.0;

/// A lifeline's head: a box with 2 px border or one of the robustness icons.
fn head(head: Rect, icon: HeadIcon) -> Vec<Primitive> {
    let filled = |center: Point, rx: f32, ry: f32| Primitive::Ellipse {
        center,
        rx,
        ry,
        fill: Paint::Fill,
        width: 2.0,
    };
    let (x, y, cy) = (head.x, head.y, head.center().y);
    match icon {
        HeadIcon::Box => vec![Primitive::Frame {
            rect: head,
            width: 2.0,
            radius: 0.0,
            fill: Paint::Fill,
            border: Paint::Line,
            style: LineStyle::Solid,
        }],
        HeadIcon::Boundary => {
            // A T on its side left of an ellipse two thirds of the width.
            let r = head.height / 2.0;
            let left = x + 5.0;
            let circle_left = x + head.width / 3.0;
            vec![
                thick(vec![Point::new(left, cy), Point::new(circle_left, cy)]),
                thick(vec![Point::new(left, y + 5.0), Point::new(left, head.bottom() - 5.0)]),
                filled(
                    Point::new((circle_left + head.right()) / 2.0, cy),
                    (head.right() - circle_left) / 2.0,
                    r,
                ),
            ]
        }
        HeadIcon::Control => {
            // A circle with an arrow head on top, pointing left.
            let c = head.center();
            let r = head.height / 2.0;
            let arm = 3.12;
            vec![
                filled(c, r, r),
                thick(vec![Point::new(c.x + arm, y - arm), Point::new(c.x, y)]),
                thick(vec![Point::new(c.x, y), Point::new(c.x + arm, y + arm)]),
            ]
        }
        HeadIcon::Entity => {
            // A circle standing on a line.
            let c = head.center();
            let r = head.height / 2.0;
            vec![
                filled(c, r, r),
                thick(vec![Point::new(c.x - r, head.bottom()), Point::new(c.x + r, head.bottom())]),
            ]
        }
        HeadIcon::Actor => {
            let cx = head.center().x;
            let r = head.height * 0.18;
            let neck = y + 2.0 * r;
            let hip = y + head.height * 0.62;
            let arms = y + head.height * 0.38;
            vec![
                filled(Point::new(cx, y + r), r, r),
                line(vec![Point::new(cx, neck), Point::new(cx, hip)], LineStyle::Solid),
                line(
                    vec![Point::new(cx - r * 1.5, arms), Point::new(cx + r * 1.5, arms)],
                    LineStyle::Solid,
                ),
                line(
                    vec![
                        Point::new(cx - r * 1.5, head.bottom()),
                        Point::new(cx, hip),
                        Point::new(cx + r * 1.5, head.bottom()),
                    ],
                    LineStyle::Solid,
                ),
            ]
        }
    }
}

/// The outline of a node, in drawing order.
pub fn figure(node: &NodeShape) -> Vec<Primitive> {
    let r = node.rect;
    let solid = LineStyle::Solid;
    match &node.figure {
        Figure::Classifier { separators, template, .. } => {
            // Astah draws classifier boxes with a 2 px border, compartment lines with 1 px.
            let mut result = vec![Primitive::Frame {
                rect: r,
                width: 2.0,
                radius: 0.0,
                fill: Paint::Fill,
                border: Paint::Line,
                style: solid,
            }];
            result.extend(
                separators
                    .iter()
                    .map(|&y| line(vec![Point::new(r.x, y), Point::new(r.right(), y)], solid)),
            );
            // The template box lies over the corner, filled like the box, with a 1 px dotted
            // border.
            result.extend(template.map(|rect| Primitive::Frame {
                rect,
                width: 1.0,
                radius: 0.0,
                fill: Paint::Fill,
                border: Paint::Line,
                style: LineStyle::Dotted,
            }));
            result
        }
        Figure::Lollipop => {
            let radius = crate::scene::LOLLIPOP_RADIUS;
            vec![Primitive::Ellipse {
                center: r.center(),
                rx: radius,
                ry: radius,
                fill: Paint::Fill,
                width: 2.0,
            }]
        }
        Figure::Package { tab, subsystem } => {
            // Astah draws packages with 2 px lines.
            let body = Rect::new(r.x, r.y + tab.height, r.width, r.height - tab.height);
            let thick = |rect| Primitive::Frame {
                rect,
                width: 2.0,
                radius: 0.0,
                fill: Paint::Fill,
                border: Paint::Line,
                style: solid,
            };
            let mut result = vec![thick(*tab), thick(body)];
            if *subsystem {
                // A fork 7 wide ending 5 left of the tab's right edge, 4 to 11 below its top.
                let (right, top) = (tab.right() - 5.0, tab.y);
                let (left, mid) = (right - 7.0, right - 3.5);
                result.extend([
                    line(vec![Point::new(mid, top + 4.0), Point::new(mid, top + 7.5)], solid),
                    line(
                        vec![
                            Point::new(left, top + 11.0),
                            Point::new(left, top + 7.5),
                            Point::new(right, top + 7.5),
                            Point::new(right, top + 11.0),
                        ],
                        solid,
                    ),
                ]);
            }
            result
        }
        Figure::Actor => {
            // Astah's figure in the middle of the box, with 2 px lines: a head of radius 9
            // (placed on a whole pixel across, as Java rounds it), a body of 18, arms 28 wide
            // 6.75 below the neck and legs reaching 14 to each side.
            let (half, head) = (crate::scene::ACTOR_FIGURE.width / 2.0, 9.0);
            let cx = r.center().x;
            let top = r.center().y - crate::scene::ACTOR_FIGURE.height / 2.0;
            let neck = top + 2.0 * head;
            let hip = neck + 18.0;
            let arms = neck + 6.75;
            let feet = top + crate::scene::ACTOR_FIGURE.height;
            vec![
                Primitive::Ellipse {
                    center: Point::new((cx - head + 0.5).floor() + head, top + head),
                    rx: head,
                    ry: head,
                    fill: Paint::Fill,
                    width: 2.0,
                },
                thick(vec![Point::new(cx, neck), Point::new(cx, hip)]),
                thick(vec![Point::new(cx - half, arms), Point::new(cx + half, arms)]),
                thick(vec![Point::new(cx, hip), Point::new(cx - half, feet)]),
                thick(vec![Point::new(cx, hip), Point::new(cx + half, feet)]),
            ]
        }
        Figure::UseCase { separator } => {
            // Astah: a 2 px ellipse; the line below the name spans the rectangle inscribed in
            // the ellipse.
            let center = r.center();
            let mut result = vec![Primitive::Ellipse {
                center,
                rx: r.width / 2.0,
                ry: r.height / 2.0,
                fill: Paint::Fill,
                width: 2.0,
            }];
            if let Some(y) = *separator {
                let half = r.width / (2.0 * std::f32::consts::SQRT_2);
                result.push(line(
                    vec![Point::new(center.x - half, y), Point::new(center.x + half, y)],
                    solid,
                ));
            }
            result
        }
        Figure::Boundary => vec![frame(r, Paint::None)],
        Figure::Lifeline { head: h, icon, line: (top, bottom) } => {
            let x = h.center().x;
            let mut result = vec![line(
                vec![Point::new(x, *top), Point::new(x, *bottom)],
                LineStyle::LongDashed,
            )];
            result.extend(head(*h, *icon));
            result
        }
        // Astah draws a bar of width 0 as a line.
        Figure::Activation if r.width < 0.5 => {
            vec![line(vec![Point::new(r.x, r.y), Point::new(r.x, r.bottom())], solid)]
        }
        Figure::Activation => vec![frame(r, Paint::Fill)],
        Figure::Frame { tab } => vec![frame(r, Paint::None), tab_line(*tab, 1.0)],
        Figure::Fragment { tab, separators, filled } => {
            let fill = if *filled { Paint::Fill } else { Paint::None };
            let mut result = vec![frame(r, fill), tab_line(*tab, 0.0)];
            result.extend(separators.iter().map(|&y| {
                line(vec![Point::new(r.x, y), Point::new(r.right(), y)], LineStyle::ShortDashed)
            }));
            result
        }
        Figure::Note => {
            let fold = 10.0;
            vec![
                Primitive::Polygon {
                    points: vec![
                        r.origin(),
                        Point::new(r.right() - fold, r.y),
                        Point::new(r.right(), r.y + fold),
                        Point::new(r.right(), r.bottom()),
                        Point::new(r.x, r.bottom()),
                    ],
                    fill: Paint::Fill,
                },
                // The folded corner.
                line(
                    vec![
                        Point::new(r.right(), r.y + fold),
                        Point::new(r.right() - fold, r.y + fold),
                        Point::new(r.right() - fold, r.y),
                    ],
                    solid,
                ),
            ]
        }
        Figure::Action => vec![Primitive::Frame {
            rect: r,
            width: 1.0,
            radius: ACTION_RADIUS.min(r.height / 2.0).min(r.width / 2.0),
            fill: Paint::Fill,
            border: Paint::Line,
            style: solid,
        }],
        Figure::Initial => {
            vec![Primitive::Ellipse {
                center: r.center(),
                rx: r.width / 2.0,
                ry: r.height / 2.0,
                fill: Paint::Line,
                width: 1.0,
            }]
        }
        Figure::ActivityFinal => {
            // A ring around a filled circle.
            let (rx, ry) = (r.width / 2.0, r.height / 2.0);
            vec![
                ellipse(r.center(), rx, ry),
                Primitive::Ellipse {
                    center: r.center(),
                    rx: rx * 0.6,
                    ry: ry * 0.6,
                    fill: Paint::Line,
                    width: 1.0,
                },
            ]
        }
        Figure::FlowFinal => {
            // A circle crossed out at 45°.
            let c = r.center();
            let (dx, dy) = (r.width / 2.0 * FRAC_1_SQRT_2, r.height / 2.0 * FRAC_1_SQRT_2);
            vec![
                ellipse(c, r.width / 2.0, r.height / 2.0),
                line(vec![c.offset(-dx, -dy), c.offset(dx, dy)], solid),
                line(vec![c.offset(-dx, dy), c.offset(dx, -dy)], solid),
            ]
        }
        Figure::Diamond => {
            let c = r.center();
            vec![Primitive::Polygon {
                points: vec![
                    Point::new(c.x, r.y),
                    Point::new(r.right(), c.y),
                    Point::new(c.x, r.bottom()),
                    Point::new(r.x, c.y),
                ],
                fill: Paint::Fill,
            }]
        }
        Figure::Bar => vec![Primitive::Frame {
            rect: r,
            width: 1.0,
            radius: 0.0,
            fill: Paint::Line,
            border: Paint::Line,
            style: solid,
        }],
        Figure::Rectangle => vec![frame(r, Paint::Fill)],
        Figure::State { separator, regions, vertical } => {
            // Astah outlines states 2 px wide.
            let mut result = vec![Primitive::Frame {
                rect: r,
                width: 2.0,
                radius: state_radius(r).min(r.height / 2.0).min(r.width / 2.0),
                fill: Paint::Fill,
                border: Paint::Line,
                style: solid,
            }];
            result.extend(
                separator.map(|y| line(vec![Point::new(r.x, y), Point::new(r.right(), y)], solid)),
            );
            // Dashed lines between the regions.
            result.extend(regions.iter().skip(1).map(|region| {
                let a = region.rect;
                let points = if *vertical {
                    vec![Point::new(a.x, a.y), Point::new(a.x, r.bottom())]
                } else {
                    vec![Point::new(r.x, a.y), Point::new(r.right(), a.y)]
                };
                line(points, LineStyle::ShortDashed)
            }));
            result
        }
        Figure::Circle => vec![ellipse(r.center(), r.width / 2.0, r.height / 2.0)],
        Figure::Component { icon } => {
            // UML's component icon: a box with two small tabs sticking out 2 on its left.
            let body = Rect::new(icon.x + 2.0, icon.y, icon.width - 2.0, icon.height);
            let tab = |y: f32| Rect::new(icon.x, y, 6.0, 3.0);
            vec![
                thick_frame(r),
                frame(body, Paint::Fill),
                frame(tab(icon.y + 3.0), Paint::Fill),
                frame(tab(icon.y + 8.0), Paint::Fill),
            ]
        }
        Figure::Artifact { icon } => {
            // A sheet of paper with a folded corner.
            let fold = 4.0;
            let (x, y, right, bottom) = (icon.x, icon.y, icon.right(), icon.bottom());
            vec![
                thick_frame(r),
                Primitive::Polygon {
                    points: vec![
                        Point::new(x, y),
                        Point::new(right - fold, y),
                        Point::new(right, y + fold),
                        Point::new(right, bottom),
                        Point::new(x, bottom),
                    ],
                    fill: Paint::Fill,
                },
                line(
                    vec![
                        Point::new(right - fold, y),
                        Point::new(right - fold, y + fold),
                        Point::new(right, y + fold),
                    ],
                    solid,
                ),
            ]
        }
        Figure::Cube { depth } => {
            // Astah: the outline and the front face's top and right edges, all 2 px. The node's
            // rectangle is the front face; top and side stick out of it.
            let d = *depth;
            let front = r;
            let r = Rect::new(r.x, r.y - d, r.width + d, r.height + d);
            let outline = vec![
                Point::new(r.x, front.y),
                Point::new(r.x + d, r.y),
                Point::new(r.right(), r.y),
                Point::new(r.right(), r.bottom() - d),
                Point::new(front.right(), r.bottom()),
                Point::new(r.x, r.bottom()),
            ];
            let mut closed = outline.clone();
            closed.extend(outline.iter().take(2).copied());
            vec![
                Primitive::Polygon { points: outline, fill: Paint::Fill },
                thick(closed),
                thick(vec![
                    Point::new(r.x, front.y),
                    Point::new(front.right(), front.y),
                    Point::new(front.right(), front.bottom()),
                ]),
                thick(vec![Point::new(front.right(), front.y), Point::new(r.right(), r.y)]),
            ]
        }
        Figure::Partition { header } => vec![
            frame(r, Paint::Translucent),
            line(vec![Point::new(r.x, *header), Point::new(r.right(), *header)], solid),
        ],
        Figure::Text => Vec::new(),
        Figure::Other => vec![Primitive::Frame {
            rect: r,
            width: 1.0,
            radius: 0.0,
            fill: Paint::Fill,
            border: Paint::Muted,
            style: LineStyle::Dashed,
        }],
    }
}

/// Corner radius of actions, as far as their size allows.
const ACTION_RADIUS: f32 = 12.0;
/// Corner radius of states, measured in Astah: 4 plus a twentieth of the height.
fn state_radius(r: Rect) -> f32 {
    4.0 + r.height / 20.0
}

/// The line of an edge, `width` wide: a polyline, or one curve.
pub fn edge_line(edge: &EdgeShape, width: f32) -> Primitive {
    if edge.shape.is_curve() {
        Primitive::Curve { points: route::bezier(&edge.points), width, style: edge.line }
    } else {
        Primitive::Polyline { points: edge.points.clone(), width, style: edge.line }
    }
}

/// The box of an association end's qualifiers: filled, with a 1 px border.
pub fn qualifier_box(rect: Rect) -> Primitive {
    frame(rect, Paint::Fill)
}

/// The end markers of an edge and the arrow next to its name ([`end_markers`] and
/// [`name_arrow`]).
pub fn markers(edge: &EdgeShape) -> Vec<Primitive> {
    let mut result = end_markers(edge);
    result.extend(name_arrow(edge));
    result
}

/// The markers at the ends of an edge, drawn in its line color. On curves they point along the
/// curve's end, which runs toward the neighboring control point.
pub fn end_markers(edge: &EdgeShape) -> Vec<Primitive> {
    let pts = &edge.points;
    let n = pts.len();
    let mut result = marker(pts[n - 1], pts[n - 2], edge.target_marker);
    result.extend(marker(pts[0], pts[1], edge.source_marker));
    result
}

/// The triangle that shows which way an association's name reads. Astah draws it black
/// whatever the line's color.
pub fn name_arrow(edge: &EdgeShape) -> Option<Primitive> {
    edge.name_arrow.map(|points| Primitive::Polygon { points: points.to_vec(), fill: Paint::Line })
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
            // Synchronous messages: a solid triangle 11 deep and 13 wide.
            vec![Primitive::Polygon {
                points: vec![tip, at(11.0, 6.5), at(11.0, -6.5)],
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
            let arm = 5.0 * FRAC_1_SQRT_2;
            vec![
                line(vec![at(19.0 - arm, arm), at(19.0 + arm, -arm)], solid),
                line(vec![at(19.0 - arm, -arm), at(19.0 + arm, arm)], solid),
            ]
        }
        EndMarker::HollowDiamond | EndMarker::FilledDiamond => {
            // Astah's diamonds: 14 long, 10 wide.
            let fill =
                if marker == EndMarker::FilledDiamond { Paint::Line } else { Paint::Background };
            vec![Primitive::Polygon {
                points: vec![tip, at(7.0, 5.0), at(14.0, 0.0), at(7.0, -5.0)],
                fill,
            }]
        }
        // The cross 19 from the end lies just behind the diamond, as in Astah.
        EndMarker::DiamondCross { filled } => {
            let mut result = self::marker(tip, from, diamond(filled));
            result.extend(self::marker(tip, from, EndMarker::Cross));
            result
        }
        // The arrow points at the diamond's far corner.
        EndMarker::DiamondArrow { filled } => {
            let mut result = self::marker(tip, from, diamond(filled));
            let corner = at(14.0, 0.0);
            result.push(line(vec![at(14.0 + back, side), corner, at(14.0 + back, -side)], solid));
            result
        }
        EndMarker::Containment => {
            // A circle of radius 5 touching the node, crossed along and across the line.
            let r = 5.0;
            let center = at(r, 0.0);
            vec![
                Primitive::Ellipse { center, rx: r, ry: r, fill: Paint::Background, width: 1.0 },
                line(vec![tip, at(2.0 * r, 0.0)], solid),
                line(vec![at(r, r), at(r, -r)], solid),
            ]
        }
    }
}

fn diamond(filled: bool) -> EndMarker {
    if filled { EndMarker::FilledDiamond } else { EndMarker::HollowDiamond }
}

/// The dots of found and lost messages and the crosses of destroyed lifelines, 20 px wide.
pub fn sequence(layout: &SequenceLayout) -> Vec<Primitive> {
    let r = crate::sequence::GATE_DOT;
    let mut result: Vec<Primitive> = layout
        .gates
        .iter()
        .map(|&center| Primitive::Ellipse { center, rx: r, ry: r, fill: Paint::Line, width: 1.0 })
        .collect();
    let d = 10.0;
    for &p in &layout.destructions {
        result.push(thick(vec![p.offset(d, -d), p.offset(-d, d)]));
        result.push(thick(vec![p.offset(-d, -d), p.offset(d, d)]));
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
