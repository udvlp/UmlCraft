//! The diagram as a standalone SVG image: the draw list with a margin around it, shapes moved
//! by half a pixel like draw.io's (sharp 1 px lines), texts as `<text>` with the lines broken
//! here, not as HTML.

use std::fmt::Write;

use crate::draw::{Anchor, Primitive, WHITE, draw};
use crate::model::{Diagram, Point, Rect};

/// The fonts an image names: draw.io's Helvetica, Arial as on Windows, Liberation Sans whose
/// widths the layout uses.
pub const FONT_FAMILY: &str = "Helvetica, Arial, &quot;Liberation Sans&quot;, sans-serif";
/// Space around the drawing.
pub const MARGIN: f64 = 10.0;

/// How an image is made.
#[derive(Clone, Debug)]
pub struct Options {
    /// `None`: transparent.
    pub background: Option<String>,
    pub margin: f64,
    /// The part of the canvas to show; `None`: everything plus the margin.
    pub view_box: Option<Rect>,
}

impl Default for Options {
    fn default() -> Self {
        Self { background: Some(WHITE.to_owned()), margin: MARGIN, view_box: None }
    }
}

/// The SVG image of a diagram with white background and a margin of 10 px.
pub fn render(d: &Diagram) -> String {
    render_with(d, &Options::default())
}

pub fn render_with(d: &Diagram, options: &Options) -> String {
    let primitives = draw(d);
    let view = options.view_box.unwrap_or_else(|| extent(&primitives, options.margin));
    let mut s = String::new();
    let _ = write!(
        s,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" \
         width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {w} {h}\">\n",
        x = n(view.x),
        y = n(view.y),
        w = n(view.width),
        h = n(view.height),
    );
    if let Some(bg) = &options.background {
        let _ = writeln!(
            s,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            n(view.x),
            n(view.y),
            n(view.width),
            n(view.height),
            attr(bg)
        );
    }
    let mut shapes = String::new();
    let flush = |s: &mut String, shapes: &mut String| {
        if !shapes.is_empty() {
            let _ = write!(s, "<g transform=\"translate(0.5,0.5)\">\n{shapes}</g>\n");
            shapes.clear();
        }
    };
    for p in &primitives {
        match p {
            Primitive::Text { .. } | Primitive::Background { .. } => {
                flush(&mut s, &mut shapes);
                element(&mut s, p);
            }
            _ => element(&mut shapes, p),
        }
    }
    flush(&mut s, &mut shapes);
    s.push_str("</svg>\n");
    s
}

/// The box around everything drawn, grown by `margin`, on whole pixels.
fn extent(primitives: &[Primitive], margin: f64) -> Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for (i, p) in primitives.iter().enumerate() {
        let b = p.bounds();
        if i == 0 {
            (x0, y0, x1, y1) = (b.x, b.y, b.x + b.width, b.y + b.height);
        }
        (x0, y0) = (x0.min(b.x), y0.min(b.y));
        (x1, y1) = (x1.max(b.x + b.width), y1.max(b.y + b.height));
    }
    let (x0, y0) = ((x0 - margin).floor(), (y0 - margin).floor());
    let (x1, y1) = ((x1 + margin + 1.0).ceil(), (y1 + margin + 1.0).ceil());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

fn stroke_attrs(stroke: &str, dash: crate::draw::Dash) -> String {
    let mut s = format!(" stroke=\"{}\"", attr(stroke));
    if let Some(p) = dash.pattern() {
        let _ = write!(s, " stroke-dasharray=\"{p}\"");
    }
    s
}

fn fill_attr(fill: &Option<String>) -> String {
    format!(" fill=\"{}\"", fill.as_deref().map_or("none".to_owned(), attr))
}

fn points(ps: &[Point]) -> String {
    ps.iter().map(|p| format!("{},{}", n(p.x), n(p.y))).collect::<Vec<_>>().join(" ")
}

fn element(s: &mut String, p: &Primitive) {
    let _ = match p {
        Primitive::Rect { rect, radius, fill, stroke, dash } => {
            let rounded = if *radius > 0.0 {
                format!(" rx=\"{0}\" ry=\"{0}\"", n(*radius))
            } else {
                String::new()
            };
            writeln!(
                s,
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rounded}{}{}/>",
                n(rect.x),
                n(rect.y),
                n(rect.width),
                n(rect.height),
                fill_attr(fill),
                stroke_attrs(stroke, *dash)
            )
        }
        Primitive::Ellipse { rect, fill, stroke, dash } => {
            let c = rect.center();
            writeln!(
                s,
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}{}/>",
                n(c.x),
                n(c.y),
                n(rect.width / 2.0),
                n(rect.height / 2.0),
                fill_attr(fill),
                stroke_attrs(stroke, *dash)
            )
        }
        Primitive::Polygon { points: ps, fill, stroke, dash } => writeln!(
            s,
            "<polygon points=\"{}\"{}{}/>",
            points(ps),
            fill_attr(fill),
            stroke_attrs(stroke, *dash)
        ),
        Primitive::Polyline { points: ps, stroke, dash } => writeln!(
            s,
            "<polyline points=\"{}\" fill=\"none\"{}/>",
            points(ps),
            stroke_attrs(stroke, *dash)
        ),
        Primitive::Background { rect, fill } => writeln!(
            s,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            n(rect.x),
            n(rect.y),
            n(rect.width),
            n(rect.height),
            attr(fill)
        ),
        Primitive::Text { x, y, text, size, bold, italic, color, anchor, .. } => {
            let anchor = match anchor {
                Anchor::Start => "start",
                Anchor::Middle => "middle",
                Anchor::End => "end",
            };
            let mut font = format!(" font-family=\"{FONT_FAMILY}\" font-size=\"{}\"", n(*size));
            if *bold {
                font.push_str(" font-weight=\"bold\"");
            }
            if *italic {
                font.push_str(" font-style=\"italic\"");
            }
            writeln!(
                s,
                "<text x=\"{}\" y=\"{}\"{font} text-anchor=\"{anchor}\" fill=\"{}\">{}</text>",
                n(*x),
                n(*y),
                attr(color),
                escape(text)
            )
        }
    };
}

/// Numbers with at most two decimals, without trailing zeros.
fn n(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r };
    format!("{r}")
}

/// Text content: `&`, `<`, `>` escaped; characters XML forbids become U+FFFD.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' => out.push(' '),
            c if (c as u32) < 0x20 || matches!(c, '\u{fffe}' | '\u{ffff}') => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    out
}

fn attr(value: &str) -> String {
    escape(value).replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::SizeSet;

    #[test]
    fn texts_are_escaped_and_control_characters_replaced() {
        assert_eq!(escape("a<b & \"c\"\u{1}"), "a&lt;b &amp; \"c\"\u{fffd}");
        assert_eq!(attr("\"x\""), "&quot;x&quot;");
    }

    #[test]
    fn the_image_frames_the_drawing_with_a_margin() {
        let mut d = Diagram::new("t");
        d.add_entity("Kunde", Point::new(50.0, 20.0), SizeSet::Compact);
        let svg = render(&d);
        assert!(svg.contains("viewBox=\"-10 -10 121 61\""), "{svg}");
        assert!(svg.contains("<g transform=\"translate(0.5,0.5)\">\n<rect x=\"0\" y=\"0\""));
        assert!(svg.contains(">Kunde</text>"));
        let transparent = render_with(&d, &Options { background: None, ..Options::default() });
        assert!(!transparent.contains("fill=\"#ffffff\"/>\n<g"));
    }
}
