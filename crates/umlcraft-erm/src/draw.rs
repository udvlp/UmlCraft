//! What is drawn, independent of where to: a list of shapes, lines and text lines in drawing
//! order (lines, nodes, line labels, free texts). The SVG export writes it; the app will paint
//! the same list.

use crate::geometry::{
    double_line, inner_inset, inset, label_anchor, points_of, rhombus_corners, triangle,
};
use crate::metrics::{
    BASELINE, LABEL_FONT_SIZE, Metrics, NODE_FONT_SIZE, UNDERLINE_OFFSET, line_height,
};
use crate::model::{
    Diagram, Key, LabelPos, Line, Link, Node, NodeKind, Point, Rect, Style, TextAlign,
};
use crate::size::text_box;

pub const BLACK: &str = "#000000";
pub const WHITE: &str = "#ffffff";

/// Stroke patterns, in px at a stroke width of 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dash {
    Solid,
    /// `3 3`: derived attributes (draw.io `dashed=1`).
    Dashed,
    /// `1 2`: optional participation (`dashPattern=1 2`).
    Dotted,
    /// `1 1`: the underline of a partial key (CSS `underline dotted`).
    Dots,
}

impl Dash {
    pub fn pattern(self) -> Option<&'static str> {
        match self {
            Self::Solid => None,
            Self::Dashed => Some("3 3"),
            Self::Dotted => Some("1 2"),
            Self::Dots => Some("1 1"),
        }
    }
}

/// How a text line is placed horizontally around its `x`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Anchor {
    Start,
    Middle,
    End,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Primitive {
    /// `fill` `None` means unfilled.
    Rect {
        rect: Rect,
        radius: f64,
        fill: Option<String>,
        stroke: String,
        dash: Dash,
    },
    Ellipse {
        rect: Rect,
        fill: Option<String>,
        stroke: String,
        dash: Dash,
    },
    Polygon {
        points: Vec<Point>,
        fill: Option<String>,
        stroke: String,
        dash: Dash,
    },
    Polyline {
        points: Vec<Point>,
        stroke: String,
        dash: Dash,
    },
    /// A filled box without outline (behind line labels).
    Background {
        rect: Rect,
        fill: String,
    },
    /// One line of text; `y` is its baseline, `width` its advance width.
    Text {
        x: f64,
        y: f64,
        text: String,
        size: f64,
        bold: bool,
        italic: bool,
        color: String,
        anchor: Anchor,
        width: f64,
    },
}

impl Primitive {
    /// The box the primitive covers, for the extent of an image.
    pub fn bounds(&self) -> Rect {
        let around = |points: &[Point]| {
            let (mut x0, mut y0) = (f64::INFINITY, f64::INFINITY);
            let (mut x1, mut y1) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in points {
                (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
            }
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        };
        match self {
            Self::Rect { rect, .. }
            | Self::Ellipse { rect, .. }
            | Self::Background { rect, .. } => *rect,
            Self::Polygon { points, .. } | Self::Polyline { points, .. } => around(points),
            Self::Text { x, y, size, anchor, width, .. } => {
                let m = Metrics::get();
                let left = match anchor {
                    Anchor::Start => *x,
                    Anchor::Middle => x - width / 2.0,
                    Anchor::End => x - width,
                };
                let top = y - m.ascent(*size);
                Rect::new(left, top, *width, m.ascent(*size) + m.descent(*size))
            }
        }
    }
}

/// The whole diagram, in drawing order.
pub fn draw(d: &Diagram) -> Vec<Primitive> {
    let mut out = Vec::new();
    for link in &d.links {
        draw_link(d, link, &mut out);
    }
    for node in &d.nodes {
        draw_node(node, &mut out);
    }
    for link in &d.links {
        draw_labels(d, link, &mut out);
    }
    for t in &d.texts {
        let size = t.style.font_size.unwrap_or(NODE_FONT_SIZE);
        let b = inset(&t.bounds, 1.0);
        let (x, anchor) = match t.style.align.unwrap_or(TextAlign::Center) {
            TextAlign::Left => (b.x, Anchor::Start),
            TextAlign::Center => (b.center().x, Anchor::Middle),
            TextAlign::Right => (b.x + b.width, Anchor::End),
        };
        let color = t.style.font_color.clone().unwrap_or_else(|| BLACK.to_owned());
        text_block(&t.text, b, x, anchor, Valign::Middle, size, &t.style, &color, None, &mut out);
    }
    out
}

fn stroke(style: &Style) -> String {
    style.stroke.clone().unwrap_or_else(|| BLACK.to_owned())
}

fn fill(style: &Style) -> Option<String> {
    Some(style.fill.clone().unwrap_or_else(|| WHITE.to_owned()))
}

fn draw_node(node: &Node, out: &mut Vec<Primitive>) {
    let b = node.bounds;
    let (fill, stroke) = (fill(&node.style), stroke(&node.style));
    let solid = Dash::Solid;
    match node.kind {
        NodeKind::Entity { .. } => {
            let radius = if node.style.rounded { b.width.min(b.height) * 0.1 } else { 0.0 };
            out.push(Primitive::Rect {
                rect: b,
                radius,
                fill,
                stroke: stroke.clone(),
                dash: solid,
            });
        }
        NodeKind::AssociativeEntity => {
            out.push(Primitive::Rect {
                rect: b,
                radius: 0.0,
                fill,
                stroke: stroke.clone(),
                dash: solid,
            });
            let points = rhombus_corners(&b).to_vec();
            out.push(Primitive::Polygon {
                points,
                fill: None,
                stroke: stroke.clone(),
                dash: solid,
            });
        }
        NodeKind::Attribute { derived, .. } => {
            let dash = if derived { Dash::Dashed } else { solid };
            out.push(Primitive::Ellipse { rect: b, fill, stroke: stroke.clone(), dash });
        }
        NodeKind::Relationship { .. } => {
            let points = rhombus_corners(&b).to_vec();
            out.push(Primitive::Polygon { points, fill, stroke: stroke.clone(), dash: solid });
        }
        NodeKind::IsA { .. } => {
            let points = triangle(&b).to_vec();
            out.push(Primitive::Polygon { points, fill, stroke: stroke.clone(), dash: solid });
        }
    }
    if let Some(by) = inner_inset(&node.kind) {
        let inner = inset(&b, by);
        out.push(match node.kind {
            NodeKind::Attribute { .. } => {
                Primitive::Ellipse { rect: inner, fill: None, stroke: stroke.clone(), dash: solid }
            }
            NodeKind::Relationship { .. } => Primitive::Polygon {
                points: rhombus_corners(&inner).to_vec(),
                fill: None,
                stroke: stroke.clone(),
                dash: solid,
            },
            _ => Primitive::Rect {
                rect: inner,
                radius: 0.0,
                fill: None,
                stroke: stroke.clone(),
                dash: solid,
            },
        });
    }
    let size = node.style.font_size.unwrap_or(NODE_FONT_SIZE);
    let color = node.style.font_color.clone().unwrap_or_else(|| BLACK.to_owned());
    let tb = text_box(node);
    let underline = match node.kind {
        NodeKind::Attribute { key: Some(Key::Primary), .. } => Some(Dash::Solid),
        NodeKind::Attribute { key: Some(Key::Partial), .. } => Some(Dash::Dots),
        _ => None,
    };
    // IS-A: text at the bottom, 5 px above the base (verticalAlign=bottom, spacingBottom=2).
    let valign = if matches!(node.kind, NodeKind::IsA { .. }) {
        Valign::Bottom(b.y + b.height - 5.0)
    } else {
        Valign::Middle
    };
    let x = tb.center().x;
    text_block(
        &node.name,
        tb,
        x,
        Anchor::Middle,
        valign,
        size,
        &node.style,
        &color,
        underline,
        out,
    );
    if let NodeKind::IsA { disjoint: Some(d), .. } = node.kind {
        let letter = if d { "d" } else { "o" };
        let width = Metrics::get().width(letter, size, false);
        out.push(Primitive::Text {
            x: b.x + b.width + 15.0,
            y: b.center().y + BASELINE * size,
            text: letter.to_owned(),
            size,
            bold: false,
            italic: false,
            color,
            anchor: Anchor::Middle,
            width,
        });
    }
}

#[derive(Clone, Copy)]
enum Valign {
    Middle,
    /// The bottom of the last line's box at this y.
    Bottom(f64),
}

/// Lines of text wrapped into `b`, centered vertically or set on a bottom line; underlines as
/// lines below each text line.
#[allow(clippy::too_many_arguments)]
fn text_block(
    text: &str,
    b: Rect,
    x: f64,
    anchor: Anchor,
    valign: Valign,
    size: f64,
    style: &Style,
    color: &str,
    underline: Option<Dash>,
    out: &mut Vec<Primitive>,
) {
    if text.is_empty() {
        return;
    }
    let m = Metrics::get();
    let lines = m.wrap(text, b.width, size, style.bold);
    let lh = line_height(size);
    let height = lh * lines.len() as f64;
    let top = match valign {
        Valign::Middle => b.center().y - height / 2.0,
        Valign::Bottom(bottom) => bottom - height,
    };
    for (i, line) in lines.into_iter().enumerate() {
        let baseline = top + lh * (i as f64 + 0.5) + BASELINE * size;
        let width = m.width(&line, size, style.bold);
        if let Some(dash) = underline {
            let left = match anchor {
                Anchor::Start => x,
                Anchor::Middle => x - width / 2.0,
                Anchor::End => x - width,
            };
            let y = baseline + UNDERLINE_OFFSET * size;
            out.push(Primitive::Polyline {
                points: vec![Point::new(left, y), Point::new(left + width, y)],
                stroke: color.to_owned(),
                dash,
            });
        }
        out.push(Primitive::Text {
            x,
            y: baseline,
            text: line,
            size,
            bold: style.bold,
            italic: style.italic,
            color: color.to_owned(),
            anchor,
            width,
        });
    }
}

/// Whether the link is drawn as a double line: its own style, or the supertype line of a
/// total IS-A.
fn is_double(d: &Diagram, link: &Link) -> bool {
    link.line == Line::Double
        || link.super_type
            && d.node(&link.source)
                .is_some_and(|n| matches!(n.kind, NodeKind::IsA { total: true, .. }))
}

fn draw_link(d: &Diagram, link: &Link, out: &mut Vec<Primitive>) {
    let Some(points) = points_of(d, link) else { return };
    let stroke = BLACK.to_owned();
    if is_double(d, link) {
        for side in double_line(&points) {
            out.push(Primitive::Polyline {
                points: side,
                stroke: stroke.clone(),
                dash: Dash::Solid,
            });
        }
    } else {
        let dash = if link.line == Line::Dotted { Dash::Dotted } else { Dash::Solid };
        out.push(Primitive::Polyline { points, stroke, dash });
    }
}

/// Where a label is drawn: its anchor on whole pixels (as in draw.io's exports) and the box of
/// its lines, centred on the anchor.
pub fn label_box(points: &[Point], text: &str, pos: &LabelPos) -> (Point, Rect) {
    let exact = label_anchor(points, pos);
    let anchor = Point::new((exact.x + 0.5).floor(), (exact.y + 0.5).floor());
    let m = Metrics::get();
    let lines = text.split('\n');
    let height = line_height(LABEL_FONT_SIZE) * text.split('\n').count() as f64;
    let widest = lines.map(|l| m.width(l, LABEL_FONT_SIZE, false)).fold(0.0, f64::max);
    (anchor, Rect::new(anchor.x - widest / 2.0, anchor.y - height / 2.0, widest, height))
}

fn draw_labels(d: &Diagram, link: &Link, out: &mut Vec<Primitive>) {
    let Some(points) = points_of(d, link) else { return };
    let texts = link
        .label
        .iter()
        .map(|l| (l.display(), l.pos))
        .chain(link.role.iter().map(|r| (r.text.clone(), r.pos)));
    let m = Metrics::get();
    let size = LABEL_FONT_SIZE;
    let lh = line_height(size);
    for (text, pos) in texts {
        let (anchor, rect) = label_box(&points, &text, &pos);
        out.push(Primitive::Background { rect, fill: WHITE.to_owned() });
        for (i, line) in text.split('\n').enumerate() {
            out.push(Primitive::Text {
                x: anchor.x,
                y: rect.y + lh * (i as f64 + 0.5) + BASELINE * size,
                text: line.to_owned(),
                size,
                bold: false,
                italic: false,
                color: BLACK.to_owned(),
                anchor: Anchor::Middle,
                width: m.width(line, size, false),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::SizeSet;

    #[test]
    fn a_key_attribute_is_an_underlined_ellipse() {
        let mut d = Diagram::new("t");
        let e = d.add_entity("Kunde", Point::new(100.0, 100.0), SizeSet::Compact);
        let a = d.add_attribute(&e, "ID", SizeSet::Compact).unwrap();
        d.node_mut(&a).unwrap().kind =
            NodeKind::Attribute { key: Some(Key::Primary), derived: false, multivalued: false };
        let p = draw(&d);
        // Line, entity, its text, attribute, underline, its text.
        assert!(matches!(p[0], Primitive::Polyline { .. }));
        assert!(matches!(p[1], Primitive::Rect { radius: 0.0, .. }));
        assert!(matches!(&p[3], Primitive::Ellipse { dash: Dash::Solid, .. }));
        let Primitive::Polyline { points, .. } = &p[4] else { panic!("{:?}", p[4]) };
        assert!((points[1].x - points[0].x - 12.0).abs() < 0.01, "as wide as \"ID\"");
        let Primitive::Text { text, y, .. } = &p[5] else { panic!() };
        assert_eq!(text, "ID");
        // Middle of the ellipse (22 + 14) plus the baseline offset.
        assert!((y - 39.5).abs() < 0.01, "{y}");
    }

    #[test]
    fn weak_entities_and_identifying_relationships_get_a_second_frame() {
        let mut d = Diagram::new("t");
        let e = d.add_entity("Sitz", Point::new(50.0, 20.0), SizeSet::Compact);
        d.node_mut(&e).unwrap().kind = NodeKind::Entity { weak: true };
        let p = draw(&d);
        assert_eq!(
            p[1],
            Primitive::Rect {
                rect: Rect::new(5.0, 5.0, 90.0, 30.0),
                radius: 0.0,
                fill: None,
                stroke: BLACK.to_owned(),
                dash: Dash::Solid
            }
        );
    }
}
