//! Diagrams as SVG images, drawn as Structorizer exports them: black lines and texts on white,
//! element colours as set, 1 px lines on pixel centres. A [`Palette`] draws them in other
//! colours, for a screen in a dark theme.

use std::fmt::Write as _;

use crate::draw::{GREY, HATCH, HATCH_TILE, INCLUDES_FRAME, Ink, Primitive, draw};
use crate::layout::Layout;
use crate::metrics::Metrics;
use crate::model::{Color, Diagram};

/// The colours an image is drawn in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Lines and texts.
    pub line: Color,
    /// The diagram's background, and elements without a colour of their own (white).
    pub background: Color,
    /// Unframed heads.
    pub head: Color,
    /// The markers of elements with a comment; without a colour they are left out.
    pub comment: Option<Color>,
    /// The hatching of disabled elements.
    pub hatch: Color,
    /// The frame of the include list.
    pub includes: Color,
    /// How much of an element's own colour shows over the background: 1 shows it as set.
    pub tint: f32,
}

impl Palette {
    /// Structorizer's colours, as exports have them.
    pub const STRUCTORIZER: Palette = Palette {
        line: Color([0, 0, 0]),
        background: Color::WHITE,
        head: GREY,
        comment: Some(GREY),
        hatch: HATCH,
        includes: INCLUDES_FRAME,
        tint: 1.0,
    };

    /// Structorizer's colours without the markers of elements with a comment, as UmlCraft
    /// shows and exports structograms.
    pub const PLAIN: Palette = Palette { comment: None, ..Palette::STRUCTORIZER };

    /// The colour `ink` is drawn in; the hatching's pixels for [`Ink::Hatch`], the background
    /// for comment markers the palette leaves out.
    pub fn color(&self, ink: Ink) -> Color {
        match ink {
            Ink::Line => self.line,
            Ink::Background => self.background,
            Ink::Head => self.head,
            Ink::Comment => self.comment.unwrap_or(self.background),
            Ink::Hatch => self.hatch,
            Ink::Includes => self.includes,
            Ink::Fill(c) => self.fill(c),
        }
    }

    /// What an element's own colour `c` is drawn as.
    fn fill(&self, c: Color) -> Color {
        if c == Color::WHITE {
            return self.background;
        }
        let mix = |i: usize| {
            let (b, c) = (f32::from(self.background.0[i]), f32::from(c.0[i]));
            (b + (c - b) * self.tint).round().clamp(0.0, 255.0) as u8
        };
        Color([mix(0), mix(1), mix(2)])
    }
}

/// The diagram as a standalone SVG document, one unit per pixel.
pub fn render(diagram: &Diagram, m: &Metrics) -> String {
    render_layout(diagram, &Layout::of(diagram, m), m)
}

/// The same for a diagram laid out already (with the same metrics).
pub fn render_layout(diagram: &Diagram, layout: &Layout, m: &Metrics) -> String {
    render_with(diagram, layout, m, &Palette::STRUCTORIZER)
}

/// The same in the colours of `palette`.
pub fn render_with(diagram: &Diagram, layout: &Layout, m: &Metrics, palette: &Palette) -> String {
    let (w, h) = layout.size;
    // Lines lie on pixel centres; the image reaches one pixel beyond the frame's right and
    // bottom lines, as Structorizer's PNG export does.
    let (w, h) = (w + 1, h + 1);
    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    // Texts are black unless the palette says otherwise.
    let text = if palette.line == Color([0, 0, 0]) {
        String::new()
    } else {
        format!(" fill=\"#{}\"", palette.line.to_hex())
    };
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\" font-family=\"{}\" font-size=\"12\"{text} xml:space=\"preserve\">",
        m.font_family()
    );
    let color = |ink: Ink| color(ink, palette);
    // Disabled elements: a pixel pattern anchored at the image's origin.
    out.push_str(
        "<defs><pattern id=\"hatch\" patternUnits=\"userSpaceOnUse\" width=\"5\" height=\"10\">",
    );
    for (x, y) in HATCH_TILE {
        let _ = write!(
            out,
            "<rect x=\"{x}\" y=\"{y}\" width=\"1\" height=\"1\" fill=\"#{}\"/>",
            palette.hatch.to_hex()
        );
    }
    out.push_str("</pattern></defs>\n");
    for p in draw(diagram, layout, m) {
        match p {
            Primitive::Area { ink: Ink::Comment, .. } if palette.comment.is_none() => {}
            Primitive::Area { points, ink } => {
                let points: Vec<String> =
                    points.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
                let _ = writeln!(
                    out,
                    "<polygon points=\"{}\" fill=\"{}\" shape-rendering=\"crispEdges\"/>",
                    points.join(" "),
                    color(ink)
                );
            }
            Primitive::Line { from, to, ink } => {
                let crisp = from.x == to.x || from.y == to.y;
                let _ = writeln!(
                    out,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" \
                     stroke-width=\"1\" stroke-linecap=\"square\"{}/>",
                    f64::from(from.x) + 0.5,
                    f64::from(from.y) + 0.5,
                    f64::from(to.x) + 0.5,
                    f64::from(to.y) + 0.5,
                    color(ink),
                    if crisp { " shape-rendering=\"crispEdges\"" } else { "" }
                );
            }
            Primitive::RoundFrame { area: (a, b), radius } => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" \
                     fill=\"none\" stroke=\"#{}\" stroke-width=\"1\"/>",
                    f64::from(a.x) + 0.5,
                    f64::from(a.y) + 0.5,
                    b.x - a.x,
                    b.y - a.y,
                    radius,
                    palette.line.to_hex()
                );
            }
            Primitive::Text { at, text, bold, upwards, small } => {
                let weight = if bold { " font-weight=\"bold\"" } else { "" };
                let weight = if small { " font-size=\"8\"" } else { weight };
                let place = if upwards {
                    format!(" transform=\"translate({} {}) rotate(-90)\"", at.x, at.y)
                } else {
                    format!(" x=\"{}\" y=\"{}\"", at.x, at.y)
                };
                // Java's width, so that a line ends where Structorizer's does whatever font
                // the viewer has.
                let width = if small {
                    m.width(&text, false) * 2 / 3
                } else if bold {
                    m.title_width(std::slice::from_ref(&text))
                } else {
                    m.width(&text, false)
                };
                let _ = writeln!(
                    out,
                    "<text{place}{weight} textLength=\"{width}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
                    escape(&text)
                );
            }
        }
    }
    out.push_str("</svg>\n");
    out
}

fn color(ink: Ink, palette: &Palette) -> String {
    match ink {
        Ink::Hatch => "url(#hatch)".into(),
        ink => format!("#{}", palette.color(ink).to_hex()),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
