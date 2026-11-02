//! Diagrams as SVG images, drawn as the canvas shows them in the light theme and laid out the
//! way Astah exports them: black lines, white fill, a margin of 10 and 96 dpi (4/3 pixels per
//! logical unit).

use umlcraft_model::{Color, Point, Rect};

use crate::draw::{self, Paint, Primitive};
use crate::scene::{self, FONT_SIZE, LabelAlign, LineStyle, Row, RowRole, Scene, TextMeasure};

/// Arial is the diagram font; the others are close enough where it is missing.
const FONT_FAMILY: &str = "Arial, Helvetica, sans-serif";
/// Empty space around the drawing.
const MARGIN: f32 = 10.0;
/// Pixels per logical unit: Astah exports at 96 dpi what it lays out at 72.
pub const SCALE: f32 = 4.0 / 3.0;
const WHITE: &str = "#ffffff";
/// The fill of nodes without a color of their own, as on the canvas in the light theme.
const DEFAULT_FILL: &str = WHITE;
const BLACK: &str = "#000000";
const GRAY: &str = "#808080";

/// Units per logical unit for images meant for other programs ([`render_scaled`]).
pub const FINE_UNITS: f32 = 10.0;

/// The scene as a standalone SVG document in logical units, as Astah's own export. `measure`
/// should be the one the scene was built with; it places the texts and sizes underlines and the
/// image bounds.
pub fn render(scene: &Scene, measure: &dyn TextMeasure) -> String {
    render_scaled(scene, measure, 1.0)
}

/// Like [`render`], with every coordinate and size inside the image `units` times larger and the
/// image itself as large as before. Word's PDF export places the letters of SVG text on whole
/// units; at [`FINE_UNITS`] per logical unit their spacing stays even.
pub fn render_scaled(scene: &Scene, measure: &dyn TextMeasure, units: f32) -> String {
    let area = bounds(scene, measure).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)).inflate(MARGIN);
    let mut svg = Svg { out: String::new(), measure, fill: None, line: None, ink: None, units };
    svg.out += "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
    svg.out += &format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{pw}\" height=\"{ph}\" \
         viewBox=\"{x} {y} {w} {h}\" font-family=\"{FONT_FAMILY}\" xml:space=\"preserve\">\n",
        pw = (area.width * SCALE).ceil(),
        ph = (area.height * SCALE).ceil(),
        x = num(area.x * units),
        y = num(area.y * units),
        w = num(area.width * units),
        h = num(area.height * units),
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
        svg.fill = node.fill;
        for p in draw::figure(node) {
            svg.primitive(&p);
        }
        svg.fill = None;
        for row in &node.rows {
            svg.ink = row.color;
            svg.row(row);
        }
        svg.ink = None;
    }
    if let Some(layout) = &scene.sequence {
        for p in draw::sequence(layout) {
            svg.primitive(&p);
        }
    }
    for edge in &scene.edges {
        svg.line = edge.stroke;
        svg.primitive(&draw::edge_line(edge, 1.0));
        for p in draw::end_markers(edge) {
            svg.primitive(&p);
        }
        svg.line = None;
        if let Some(p) = draw::name_arrow(edge) {
            svg.primitive(&p);
        }
        for label in &edge.labels {
            svg.ink = label.color;
            let width = measure.width(&label.text, FONT_SIZE, false);
            let area = draw::label_area(label);
            svg.text(&label.text, area, FONT_SIZE, false, false, label.align, width);
            if label.underline {
                let x = draw::text_left(area, width, label.align);
                let y = area.y + FONT_SIZE * draw::UNDERLINE;
                svg.primitive(&Primitive::Polyline {
                    points: vec![Point::new(x, y), Point::new(x + width, y)],
                    width: 1.0,
                    style: LineStyle::Solid,
                });
            }
        }
        svg.ink = None;
        for (rect, rows) in &edge.qualifiers {
            svg.primitive(&draw::qualifier_box(*rect));
            for row in rows {
                svg.row(row);
            }
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
        e.path()
            .into_owned()
            .into_iter()
            .map(|p| Rect::from_corners(p, p))
            .chain(
                e.labels
                    .iter()
                    .map(|l| text(&l.text, draw::label_area(l), FONT_SIZE, false, l.align)),
            )
            .chain(e.qualifiers.iter().map(|(rect, _)| *rect))
            // Arrow heads, diamonds and the name's arrow reach beyond the line's points.
            .chain(draw::markers(e).into_iter().filter_map(|m| m.bounds()))
    });
    nodes.chain(edges).reduce(|a, b| a.union(&b))
}

struct Svg<'a> {
    out: String,
    measure: &'a dyn TextMeasure,
    /// Fill color of the node being drawn.
    fill: Option<Color>,
    /// Line color of the edge being drawn.
    line: Option<Color>,
    /// Color of the text being drawn.
    ink: Option<Color>,
    /// Image units per logical unit.
    units: f32,
}

impl Svg<'_> {
    fn primitive(&mut self, p: &Primitive) {
        // Logical units written in image units.
        let k = self.units;
        let num = |v: f32| num(v * k);
        let element = match p {
            Primitive::Polyline { points, width, style } => format!(
                "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}/>",
                points_attr(points, k),
                self.line_color(),
                num(*width),
                dashes(*style, k),
            ),
            Primitive::Curve { points: [a, b, c, d], width, style } => format!(
                "<path d=\"M{} {} C{} {} {} {} {} {}\" fill=\"none\" stroke=\"{}\" \
                 stroke-width=\"{}\"{}/>",
                num(a.x),
                num(a.y),
                num(b.x),
                num(b.y),
                num(c.x),
                num(c.y),
                num(d.x),
                num(d.y),
                self.line_color(),
                num(*width),
                dashes(*style, k),
            ),
            Primitive::Polygon { points, fill } => format!(
                "<polygon points=\"{}\"{} stroke=\"{}\"/>",
                points_attr(points, k),
                self.fill_attr(*fill),
                self.line_color(),
            ),
            Primitive::Ellipse { center, rx, ry, fill, width } => format!(
                "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{} stroke=\"{BLACK}\" \
                 stroke-width=\"{}\"/>",
                num(center.x),
                num(center.y),
                num(*rx),
                num(*ry),
                self.fill_attr(*fill),
                num(*width),
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
                        dashes(*style, k)
                    ),
                };
                format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{corners}{}{stroke}/>",
                    num(rect.x),
                    num(rect.y),
                    num(rect.width),
                    num(rect.height),
                    self.fill_attr(*fill),
                )
            }
        };
        self.out += &element;
        self.out.push('\n');
    }

    fn fill_attr(&self, paint: Paint) -> String {
        match paint {
            Paint::Fill => match self.fill {
                Some(c) if c.a < 255 => format!(
                    " fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\"",
                    c.r,
                    c.g,
                    c.b,
                    num(f32::from(c.a) / 255.0)
                ),
                Some(c) => format!(" fill=\"#{:02x}{:02x}{:02x}\"", c.r, c.g, c.b),
                None => format!(" fill=\"{DEFAULT_FILL}\""),
            },
            Paint::Translucent => format!(" fill=\"{WHITE}\" fill-opacity=\"0.35\""),
            Paint::Line => format!(" fill=\"{}\"", self.line_color()),
            paint => format!(" fill=\"{}\"", color(paint)),
        }
    }

    /// The color of lines: the edge's own or black.
    fn line_color(&self) -> String {
        self.line.map_or_else(|| BLACK.to_owned(), hex)
    }

    /// A row of a node. Astah centers such texts by their whole-pixel width or their exact one,
    /// whichever is larger (Java rounds many bold characters down); texts it draws with whole-
    /// pixel advances by the former.
    fn row(&mut self, row: &Row) {
        if let Some((symbol, rest)) = scene::visibility_column(&row.text)
            .filter(|_| row.role == RowRole::Member && row.align == LabelAlign::Left && !row.hinted)
            .filter(|_| self.measure.visibility_column())
        {
            self.member_row(row, symbol, rest);
            return;
        }
        let rounded = self.measure.rounded_width(&row.text, row.size, row.bold);
        let width = if row.hinted {
            rounded
        } else {
            self.measure.astah_width(&row.text, row.size, row.bold)
        };
        // Whole-pixel advances: every character at its own x.
        let advances: Option<Vec<f32>> = row
            .hinted
            .then(|| {
                row.text
                    .chars()
                    .map(|c| {
                        crate::metrics::java_width(c.encode_utf8(&mut [0; 4]), row.size, row.bold)
                    })
                    .collect()
            })
            .flatten();
        if let Some(advances) = advances.filter(|a| !a.is_empty()) {
            let k = self.units;
            let num = |v: f32| num(v * k);
            let mut x = draw::text_left(row.rect, width, row.align);
            let xs: Vec<String> = advances
                .iter()
                .map(|a| {
                    let at = num(x);
                    x += a;
                    at
                })
                .collect();
            self.out += &format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{}>{}</text>\n",
                xs.join(" "),
                num(row.rect.y + draw::baseline(row.size)),
                num(row.size),
                if row.bold { " font-weight=\"bold\"" } else { "" },
                escape(&row.text),
            );
            return;
        }
        self.text(&row.text, row.rect, row.size, row.bold, row.italic, row.align, width);
        if row.underline && !row.text.is_empty() {
            // Astah starts the line where the text starts and makes it as long as the text.
            let x = draw::text_left(row.rect, width, row.align);
            let width = self.measure.width(&row.text, row.size, row.bold);
            let y = row.rect.y + row.size * draw::UNDERLINE;
            self.primitive(&Primitive::Polyline {
                points: vec![Point::new(x, y), Point::new(x + width, y)],
                width: 1.0,
                style: LineStyle::Solid,
            });
        }
    }

    /// A member row, its visibility symbol centered in a column as wide as a `+`
    /// ([`scene::visibility_column`]).
    fn member_row(&mut self, row: &Row, symbol: &str, rest: &str) {
        let width = |text: &str| self.measure.width(text, row.size, row.bold);
        // SVG drops leading spaces, so the text after the symbol starts after their width.
        let text = rest.trim_start();
        let (slot, symbol_w, text_w) = (width("+"), width(symbol), width(text));
        let x = row.rect.x + slot + width(&rest[..rest.len() - text.len()]);
        let r = row.rect;
        let cell = Rect::new(r.x, r.y, slot, r.height);
        self.text(symbol, cell, row.size, row.bold, row.italic, LabelAlign::Center, symbol_w);
        let after = Rect::new(x, r.y, r.right() - x, r.height);
        self.text(text, after, row.size, row.bold, row.italic, LabelAlign::Left, text_w);
        if row.underline {
            let y = r.y + row.size * draw::UNDERLINE;
            self.primitive(&Primitive::Polyline {
                points: vec![Point::new(r.x, y), Point::new(x + text_w, y)],
                width: 1.0,
                style: LineStyle::Solid,
            });
        }
    }

    /// One line of text of `width` aligned in `area`. It starts at a fixed x, as Astah's own
    /// export does, whatever font the viewer has.
    #[expect(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        area: Rect,
        size: f32,
        bold: bool,
        italic: bool,
        align: LabelAlign,
        width: f32,
    ) {
        if text.is_empty() {
            return;
        }
        let k = self.units;
        let num = |v: f32| num(v * k);
        self.out += &format!(
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\"{}{}{}>{}</text>\n",
            num(draw::text_left(area, width, align)),
            num(area.y + draw::baseline(size)),
            num(size),
            if bold { " font-weight=\"bold\"" } else { "" },
            if italic { " font-style=\"italic\"" } else { "" },
            self.ink.map(|c| format!(" fill=\"{}\"", hex(c))).unwrap_or_default(),
            escape(text),
        );
    }
}

fn color(paint: Paint) -> &'static str {
    match paint {
        Paint::Fill => DEFAULT_FILL,
        Paint::Background | Paint::Translucent => WHITE,
        Paint::Line => BLACK,
        Paint::Muted => GRAY,
        Paint::None => "none",
    }
}

/// `#rrggbb`: line and text colors are opaque.
fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// The dash pattern of `style`, `k` image units per logical unit.
fn dashes(style: LineStyle, k: f32) -> String {
    let dashes = style.dashes();
    if dashes.is_empty() {
        return String::new();
    }
    let list: Vec<String> = dashes.iter().map(|&d| num(d * k)).collect();
    format!(" stroke-dasharray=\"{}\"", list.join(" "))
}

/// `points`, `k` image units per logical unit.
fn points_attr(points: &[Point], k: f32) -> String {
    let point = |p: &Point| format!("{},{}", num(p.x * k), num(p.y * k));
    points.iter().map(point).collect::<Vec<_>>().join(" ")
}

/// A coordinate with at most two decimals, without trailing zeros.
fn num(v: f32) -> String {
    let rounded = (v * 100.0).round() / 100.0;
    // No "-0".
    format!("{}", if rounded == 0.0 { 0.0 } else { rounded })
}

/// `text` for XML content and attributes. Characters XML 1.0 forbids, even as references
/// (control characters other than tab and line breaks, U+FFFE, U+FFFF), become U+FFFD.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            '\u{0}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}' => out.push('\u{FFFD}'),
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
    fn characters_xml_forbids_are_replaced() {
        assert_eq!(escape("a\u{1}b\u{c}<c>\u{FFFE}"), "a\u{FFFD}b\u{FFFD}&lt;c&gt;\u{FFFD}");
        assert_eq!(escape("tab\there"), "tab\there");
    }

    #[test]
    fn rows_and_labels_are_single_lines() {
        let (mut doc, ids) = order_system();
        doc.edit("names", |tx| {
            tx.rename(ids.customer, "Cus\ttomer\u{1}")?;
            tx.rename(ids.order_customer, "places\r\norders")
        })
        .unwrap();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let svg = render(&scene, &ApproxMeasure);
        assert!(svg.contains(">Cus tomer</text>"), "{svg}");
        assert!(svg.contains(">places orders</text>"), "{svg}");
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
        // Members as in Astah: the visibility right before the name.
        assert!(svg.contains(">- name : String</text>"), "{svg}");
        // White as on the canvas, not Astah's yellow.
        assert!(svg.contains("fill=\"#ffffff\""), "{svg}");
        assert!(!svg.contains("#ffffcc"), "{svg}");
    }

    /// Measures a hyphen and a hash narrower than the other characters, as most fonts do, with
    /// UmlCraft's visibility column on or off.
    struct Narrow(bool);

    impl TextMeasure for Narrow {
        fn width(&self, text: &str, _: f32, _: bool) -> f32 {
            text.chars()
                .map(|c| match c {
                    '-' => 4.0,
                    '#' => 6.5,
                    _ => 7.0,
                })
                .sum()
        }

        fn visibility_column(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn member_names_start_in_one_column() {
        let (doc, ids) = order_system();
        let svg = |column: bool| {
            let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &Narrow(column));
            render(&scene, &Narrow(column))
        };
        // Off, the default, a member is one text as in Astah.
        assert!(svg(false).contains(">- total : Money</text>"));
        let svg = svg(true);
        let xs = |text: &str| -> Vec<String> {
            let lines = svg.lines().filter(|l| l.ends_with(&format!(">{text}</text>")));
            lines
                .map(|l| l.split("x=\"").nth(1).unwrap().split('"').next().unwrap().into())
                .collect()
        };
        // Order's "- total : Money" and "+ submit()", the minus drawn as wide as the plus.
        assert_eq!(xs("total : Money"), xs("submit()"));
        assert!(xs("\u{2212}").contains(&xs("+")[0]), "{svg}");
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
        let width: f32 =
            svg.split("width=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        assert_eq!(width, (v[2] * SCALE).ceil());
    }

    #[test]
    fn texts_are_escaped_and_numbers_short() {
        assert_eq!(escape("List<T> & \"x\""), "List&lt;T&gt; &amp; &quot;x&quot;");
        assert_eq!(num(160.0), "160");
        assert_eq!(num(12.345_67), "12.35");
        assert_eq!(num(-0.001), "0");
        assert_eq!(dashes(LineStyle::Dashed, 1.0), " stroke-dasharray=\"10 5\"");
    }

    #[test]
    fn scaled_images_keep_their_size() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let (plain, fine) =
            (render(&scene, &ApproxMeasure), render_scaled(&scene, &ApproxMeasure, FINE_UNITS));
        let attr = |svg: &str, name: &str| -> String {
            svg.split(&format!(" {name}=\"")).nth(1).unwrap().split('"').next().unwrap().into()
        };
        assert_eq!(attr(&fine, "width"), attr(&plain, "width"));
        assert_eq!(attr(&fine, "height"), attr(&plain, "height"));
        let numbers =
            |s: String| -> Vec<f32> { s.split(' ').map(|n| n.parse().unwrap()).collect() };
        let (a, b) = (numbers(attr(&plain, "viewBox")), numbers(attr(&fine, "viewBox")));
        assert!(a.iter().zip(&b).all(|(a, b)| (a * FINE_UNITS - b).abs() < 0.1), "{a:?} {b:?}");
        assert!(fine.contains("font-size=\"120\"") && !fine.contains("font-size=\"12\""));
        assert!(fine.contains("stroke-width=\"20\""));
    }

    #[test]
    fn an_empty_diagram_is_a_small_blank_image() {
        let svg = render(&Scene::empty(), &ApproxMeasure);
        assert!(svg.contains("viewBox=\"-10 -10 20 20\""), "{svg}");
        assert!(!svg.contains("<text"), "{svg}");
    }
}
