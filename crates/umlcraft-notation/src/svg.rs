//! Diagrams as SVG images, drawn like the canvas in Astah's black on white.

use umlcraft_model::{Point, Rect};

use crate::draw::{self, Paint, Primitive};
use crate::scene::{FONT_SIZE, LabelAlign, LineStyle, Row, Scene, TextMeasure};

/// Arial is the diagram font; the others are close enough where it is missing.
const FONT_FAMILY: &str = "Arial, Helvetica, sans-serif";
/// Empty space around the drawing.
const MARGIN: f32 = 20.0;
const WHITE: &str = "#ffffff";
const BLACK: &str = "#000000";
const GRAY: &str = "#808080";

/// The scene as a standalone SVG document. `measure` should be the one the scene was built
/// with; it sizes underlines and the image bounds.
pub fn render(scene: &Scene, measure: &dyn TextMeasure) -> String {
    let area = bounds(scene, measure).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)).inflate(MARGIN);
    let mut svg = Svg { out: String::new(), measure };
    svg.out += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    svg.out += &format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"{x} {y} {w} {h}\" font-family=\"{FONT_FAMILY}\" xml:space=\"preserve\">\n",
        x = num(area.x),
        y = num(area.y),
        w = num(area.width),
        h = num(area.height),
    );
    svg.primitive(&Primitive::Frame {
        rect: area,
        width: 0.0,
        radius: 0.0,
        fill: Paint::Background,
        border: Paint::None,
        style: LineStyle::Solid,
    });
    for node in &scene.nodes {
        for p in draw::figure(node) {
            svg.primitive(&p);
        }
        for row in &node.rows {
            svg.row(row);
        }
    }
    if let Some(layout) = &scene.sequence {
        for p in draw::sequence(layout) {
            svg.primitive(&p);
        }
    }
    for edge in &scene.edges {
        svg.primitive(&Primitive::Polyline {
            points: edge.points.clone(),
            width: 1.0,
            style: edge.line,
        });
        for p in draw::markers(edge) {
            svg.primitive(&p);
        }
        for label in &edge.labels {
            svg.text(&label.text, draw::label_area(label), FONT_SIZE, false, false, label.align);
        }
    }
    svg.out += "</svg>\n";
    svg.out
}

/// Everything drawn: nodes, their texts, edges and labels.
fn bounds(scene: &Scene, measure: &dyn TextMeasure) -> Option<Rect> {
    let text = |text: &str, area: Rect, size: f32, bold: bool, align: LabelAlign| {
        let width = measure.width(text, size, bold);
        Rect::new(draw::text_left(area, width, align), area.y, width, area.height)
    };
    let nodes = scene.nodes.iter().flat_map(|n| {
        std::iter::once(n.rect)
            .chain(n.rows.iter().map(|r| text(&r.text, r.rect, r.size, r.bold, r.align)))
    });
    let edges = scene.edges.iter().flat_map(|e| {
        e.points.iter().map(|&p| Rect::from_corners(p, p)).chain(
            e.labels.iter().map(|l| text(&l.text, draw::label_area(l), FONT_SIZE, false, l.align)),
        )
    });
    nodes.chain(edges).reduce(|a, b| a.union(&b))
}

struct Svg<'a> {
    out: String,
    measure: &'a dyn TextMeasure,
}

impl Svg<'_> {
    fn primitive(&mut self, p: &Primitive) {
        let element = match p {
            Primitive::Polyline { points, width, style } => format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{BLACK}\" stroke-width=\"{}\"{}/>",
                points_attr(points),
                num(*width),
                dashes(*style),
            ),
            Primitive::Polygon { points, fill } => format!(
                "<polygon points=\"{}\"{} stroke=\"{BLACK}\"/>",
                points_attr(points),
                fill_attr(*fill),
            ),
            Primitive::Ellipse { center, rx, ry, fill } => format!(
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{} stroke=\"{BLACK}\"/>",
                num(center.x),
                num(center.y),
                num(*rx),
                num(*ry),
                fill_attr(*fill),
            ),
            Primitive::Frame { rect, width, radius, fill, border, style } => {
                let corners = if *radius > 0.0 {
                    format!(" rx=\"{0}\" ry=\"{0}\"", num(*radius))
                } else {
                    String::new()
                };
                let stroke = match border {
                    Paint::None => String::new(),
                    paint => format!(
                        " stroke=\"{}\" stroke-width=\"{}\"{}",
                        color(*paint),
                        num(*width),
                        dashes(*style)
                    ),
                };
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{corners}{}{stroke}/>",
                    num(rect.x),
                    num(rect.y),
                    num(rect.width),
                    num(rect.height),
                    fill_attr(*fill),
                )
            }
        };
        self.out += &element;
        self.out.push('\n');
    }

    fn row(&mut self, row: &Row) {
        self.text(&row.text, row.rect, row.size, row.bold, row.italic, row.align);
        if row.underline && !row.text.is_empty() {
            let width = self.measure.width(&row.text, row.size, row.bold);
            let x = draw::text_left(row.rect, width, row.align);
            let y = row.rect.y + row.size * draw::UNDERLINE;
            self.primitive(&Primitive::Polyline {
                points: vec![Point::new(x, y), Point::new(x + width, y)],
                width: 1.0,
                style: LineStyle::Solid,
            });
        }
    }

    /// One line of text in `area`, anchored by its alignment so it stays in place even where
    /// the viewer's font is a little wider or narrower.
    fn text(
        &mut self,
        text: &str,
        area: Rect,
        size: f32,
        bold: bool,
        italic: bool,
        align: LabelAlign,
    ) {
        if text.is_empty() {
            return;
        }
        let (x, anchor) = match align {
            LabelAlign::Left => (area.x, ""),
            LabelAlign::Center => (area.x + area.width / 2.0, " text-anchor=\"middle\""),
            LabelAlign::Right => (area.right(), " text-anchor=\"end\""),
        };
        self.out += &format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{}{}{anchor}>{}</text>\n",
            num(x),
            num(area.y + size * draw::BASELINE),
            num(size),
            if bold { " font-weight=\"bold\"" } else { "" },
            if italic { " font-style=\"italic\"" } else { "" },
            escape(text),
        );
    }
}

fn color(paint: Paint) -> &'static str {
    match paint {
        Paint::Fill | Paint::Background | Paint::Translucent => WHITE,
        Paint::Line => BLACK,
        Paint::Muted => GRAY,
        Paint::None => "none",
    }
}

fn fill_attr(paint: Paint) -> String {
    match paint {
        Paint::Translucent => format!(" fill=\"{WHITE}\" fill-opacity=\"0.35\""),
        paint => format!(" fill=\"{}\"", color(paint)),
    }
}

fn dashes(style: LineStyle) -> &'static str {
    match style {
        LineStyle::Solid => "",
        LineStyle::Dashed => " stroke-dasharray=\"6 4\"",
    }
}

fn points_attr(points: &[Point]) -> String {
    points.iter().map(|p| format!("{},{}", num(p.x), num(p.y))).collect::<Vec<_>>().join(" ")
}

/// A coordinate with at most two decimals, without trailing zeros.
fn num(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    // No "-0".
    format!("{}", if rounded == 0.0 { 0.0 } else { rounded })
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{ApproxMeasure, Preview, build};
    use umlcraft_model::sample::order_system;

    fn sample_svg() -> String {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        render(&scene, &ApproxMeasure)
    }

    #[test]
    fn class_diagram_becomes_an_svg_document() {
        let svg = sample_svg();
        assert!(svg.starts_with("<?xml"), "{svg}");
        assert!(svg.trim_end().ends_with("</svg>"), "{svg}");
        // Three classes with 2 px borders, the generalization's hollow triangle, the names.
        assert_eq!(svg.matches("stroke-width=\"2\"").count(), 3, "{svg}");
        assert!(svg.contains("<polygon"), "{svg}");
        assert!(svg.contains(">Customer</text>"), "{svg}");
        assert!(svg.contains(">- name : String</text>"), "{svg}");
    }

    #[test]
    fn the_image_covers_the_drawing_with_a_margin() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let drawn = scene.bounds().unwrap();
        let svg = render(&scene, &ApproxMeasure);
        let view_box = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let v: Vec<f32> = view_box.split(' ').map(|n| n.parse().unwrap()).collect();
        assert!(v[0] <= drawn.x - MARGIN && v[1] <= drawn.y - MARGIN, "{view_box}");
        assert!(v[0] + v[2] >= drawn.right() + MARGIN, "{view_box}");
        assert!(v[1] + v[3] >= drawn.bottom() + MARGIN, "{view_box}");
    }

    #[test]
    fn texts_are_escaped_and_numbers_short() {
        assert_eq!(escape("List<T> & \"x\""), "List&lt;T&gt; &amp; &quot;x&quot;");
        assert_eq!(num(160.0), "160");
        assert_eq!(num(12.345_67), "12.35");
        assert_eq!(num(-0.001), "0");
    }

    #[test]
    fn an_empty_diagram_is_a_small_blank_image() {
        let svg = render(&Scene::empty(), &ApproxMeasure);
        assert!(svg.contains("viewBox=\"-20 -20 40 40\""), "{svg}");
        assert!(!svg.contains("<text"), "{svg}");
    }
}
