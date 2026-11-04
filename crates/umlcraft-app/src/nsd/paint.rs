//! Paints a structogram's primitives (`umlcraft_nsd::draw`) on the canvas with GPUI: areas as
//! filled paths, the 1 px lines as the pixels they cover, texts shaped by GPUI in the
//! diagram's font. Texts turned by 90° (case branches) are painted as the outlines of their
//! glyphs, which usvg makes, since GPUI cannot turn text. Everything scales with the zoom, so
//! the drawing stays sharp; an export (`umlcraft_nsd::svg`) puts the same primitives in the
//! same places.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::*;
use resvg::usvg;
use umlcraft_nsd::Color;
use umlcraft_nsd::draw::{Ink, Primitive, Px};
use umlcraft_nsd::svg::Palette;

/// Where GPUI puts the baseline below the top of a line of `line_height`, relative to the
/// font size, for Arial (see the canvas).
const GPUI_BASELINE: f32 = crate::canvas::GPUI_BASELINE;

/// The distance between the hatching's lines along a vertical, and how steeply they fall:
/// Structorizer's 5 × 10 tile ([`umlcraft_nsd::draw::HATCH_TILE`]) holds one line falling two
/// pixels per pixel.
const HATCH_STEP: f32 = 10.0;
const HATCH_SLOPE: f32 = 2.0;

/// Outlines of turned texts kept at most, so that a long edit session cannot fill the memory.
const MAX_OUTLINES: usize = 500;

thread_local! {
    /// The system's fonts, loaded once for the outlines of turned texts.
    static OPTIONS: Rc<usvg::Options<'static>> = {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        Rc::new(options)
    };
    /// The outlines of turned texts by font family and text: shaping them is the expensive
    /// part, and they stay the same from frame to frame.
    static OUTLINES: RefCell<Outlines> = RefCell::new(HashMap::new());
}

/// Outlines by font family and text.
type Outlines = HashMap<(String, String), Rc<Vec<Segment>>>;

/// A piece of a glyph's outline, relative to the text's start on its baseline, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    Move(f32, f32),
    Line(f32, f32),
    Quad((f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32)),
    Close,
}

/// Where a structogram is shown: the screen position of its origin and the zoom.
#[derive(Clone, Copy)]
pub struct Place {
    pub origin: Point<Pixels>,
    pub zoom: f32,
}

impl Place {
    fn pt(&self, x: f32, y: f32) -> Point<Pixels> {
        self.origin + point(px(x * self.zoom), px(y * self.zoom))
    }

    /// The pixels from `(x, y)` on, `w` × `h` of them.
    fn pixels(&self, x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(self.pt(x, y), size(px(w * self.zoom), px(h * self.zoom)))
    }
}

pub fn hsla(c: Color) -> Hsla {
    let [r, g, b] = c.0;
    Hsla::from(Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a: 1.0,
    })
}

/// Paints `primitives` in the colours of `palette`, their texts in `family`.
pub fn paint(
    primitives: &[Primitive],
    palette: &Palette,
    family: &SharedString,
    place: Place,
    window: &mut Window,
    cx: &mut App,
) {
    for p in primitives {
        match p {
            Primitive::Area { ink: Ink::Comment, .. } if palette.comment.is_none() => {}
            Primitive::Area { points, ink: Ink::Hatch } => {
                hatch(points, hsla(palette.color(Ink::Hatch)), place, window)
            }
            Primitive::Area { points, ink } => {
                area(points, hsla(palette.color(*ink)), place, window)
            }
            Primitive::Line { from, to, ink } => {
                line(*from, *to, hsla(palette.color(*ink)), place, window)
            }
            Primitive::RoundFrame { area: (a, b), radius } => {
                // A 1 px line through the pixels from `a` to `b`, rounded at the corners.
                let bounds = place.pixels(
                    a.x as f32,
                    a.y as f32,
                    (b.x - a.x + 1) as f32,
                    (b.y - a.y + 1) as f32,
                );
                window.paint_quad(quad(
                    bounds,
                    px((*radius as f32 + 0.5) * place.zoom),
                    transparent_black(),
                    px(place.zoom),
                    hsla(palette.line),
                    BorderStyle::Solid,
                ));
            }
            Primitive::Text { at, text, bold, upwards, small } => {
                let color = hsla(palette.line);
                if *upwards {
                    turned_text(text, *at, family, color, place, window);
                } else {
                    flat_text(text, *at, *bold, *small, family, color, place, window, cx);
                }
            }
        }
    }
}

/// The bounds of `points` if they are the corners of an axis-parallel rectangle.
fn as_rect(points: &[Px]) -> Option<(i32, i32, i32, i32)> {
    let [a, b, c, d] = points else { return None };
    let rect = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
    let rect = rect || (a.x == b.x && b.y == c.y && c.x == d.x && d.y == a.y);
    rect.then(|| (a.x.min(c.x), a.y.min(c.y), a.x.max(c.x), a.y.max(c.y)))
}

/// A filled area: as Structorizer fills it, without the pixels of its right and bottom edge,
/// which a line covers.
fn area(points: &[Px], color: Hsla, place: Place, window: &mut Window) {
    if let Some((x0, y0, x1, y1)) = as_rect(points) {
        let bounds = place.pixels(x0 as f32, y0 as f32, (x1 - x0) as f32, (y1 - y0) as f32);
        window.paint_quad(fill(bounds, color));
        return;
    }
    let screen: Vec<Point<Pixels>> =
        points.iter().map(|p| place.pt(p.x as f32, p.y as f32)).collect();
    let mut path = PathBuilder::fill();
    path.add_polygon(&screen, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A 1 px line between pixel centres, both ends included: the pixels themselves where it runs
/// straight, a stroke where it slants.
fn line(from: Px, to: Px, color: Hsla, place: Place, window: &mut Window) {
    if from.x == to.x || from.y == to.y {
        let (x0, x1) = (from.x.min(to.x), from.x.max(to.x));
        let (y0, y1) = (from.y.min(to.y), from.y.max(to.y));
        let bounds = place.pixels(x0 as f32, y0 as f32, (x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
        window.paint_quad(fill(bounds, color));
        return;
    }
    let mut path = PathBuilder::stroke(px(place.zoom));
    path.move_to(place.pt(from.x as f32 + 0.5, from.y as f32 + 0.5));
    path.line_to(place.pt(to.x as f32 + 0.5, to.y as f32 + 0.5));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The hatching of a disabled element's rectangle: lines falling two pixels per pixel, ten
/// apart, anchored at the diagram's origin as Structorizer's tile is.
fn hatch(points: &[Px], color: Hsla, place: Place, window: &mut Window) {
    let Some((x0, y0, x1, y1)) = as_rect(points) else { return };
    let (x0, y0, x1, y1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
    let mask = ContentMask { bounds: place.pixels(x0, y0, x1 - x0, y1 - y0) };
    // The tile's line runs through the pixel centres (0, 9) to (4, 1): y = 10.5 − 2x + 10k.
    let at = |x: f32, k: f32| 10.5 + HATCH_STEP * k - HATCH_SLOPE * x;
    let first = ((y0 + HATCH_SLOPE * x0 - 10.5) / HATCH_STEP).floor() - 1.0;
    let last = ((y1 + HATCH_SLOPE * x1 - 10.5) / HATCH_STEP).ceil() + 1.0;
    window.with_content_mask(Some(mask), |window| {
        let mut k = first;
        while k <= last {
            let mut path = PathBuilder::stroke(px(place.zoom));
            path.move_to(place.pt(x0 - 1.0, at(x0 - 1.0, k)));
            path.line_to(place.pt(x1 + 1.0, at(x1 + 1.0, k)));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
            k += 1.0;
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn flat_text(
    text: &str,
    at: Px,
    bold: bool,
    small: bool,
    family: &SharedString,
    color: Hsla,
    place: Place,
    window: &mut Window,
    cx: &mut App,
) {
    if text.is_empty() {
        return;
    }
    let size = if small { 8.0 } else { umlcraft_notation::scene::FONT_SIZE };
    let mut f = font(family.clone());
    if bold {
        f = f.bold();
    }
    let text: SharedString = text.to_owned().into();
    let run = TextRun { len: text.len(), font: f, color, ..Default::default() };
    let shaped = window.text_system().shape_line(text, px(size * place.zoom), &[run], None);
    let line_height = px(umlcraft_notation::scene::line_height(size) * place.zoom);
    let top = place.pt(at.x as f32, at.y as f32 - size * GPUI_BASELINE);
    shaped.paint(top, line_height, TextAlign::Left, None, window, cx).ok();
}

/// A text reading upwards from `at` on its baseline, as the outlines of its glyphs.
fn turned_text(
    text: &str,
    at: Px,
    family: &SharedString,
    color: Hsla,
    place: Place,
    window: &mut Window,
) {
    let segments = outline(text, family);
    if segments.is_empty() {
        return;
    }
    let (x, y) = (at.x as f32, at.y as f32);
    let p = |(dx, dy): (f32, f32)| place.pt(x + dx, y + dy);
    let mut path = PathBuilder::fill();
    for s in segments.iter() {
        match *s {
            Segment::Move(dx, dy) => path.move_to(p((dx, dy))),
            Segment::Line(dx, dy) => path.line_to(p((dx, dy))),
            Segment::Quad(c, to) => path.curve_to(p(to), p(c)),
            Segment::Cubic(a, b, to) => path.cubic_bezier_to(p(to), p(a), p(b)),
            Segment::Close => path.close(),
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// The outline of `text` in `family` at 12 px, turned to read upwards, relative to its start
/// on the baseline; empty where usvg finds no font for it.
fn outline(text: &str, family: &str) -> Rc<Vec<Segment>> {
    let key = (family.to_owned(), text.to_owned());
    if let Some(found) = OUTLINES.with(|o| o.borrow().get(&key).cloned()) {
        return found;
    }
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\" \
         font-family=\"{family}\" font-size=\"12\" xml:space=\"preserve\">\
         <text transform=\"rotate(-90)\">{escaped}</text></svg>"
    );
    let mut segments = Vec::new();
    if let Ok(tree) = OPTIONS.with(|options| usvg::Tree::from_str(&svg, options)) {
        collect(tree.root(), &mut segments);
    }
    let segments = Rc::new(segments);
    OUTLINES.with(|o| {
        let mut o = o.borrow_mut();
        if o.len() >= MAX_OUTLINES {
            o.clear();
        }
        o.insert(key, segments.clone());
    });
    segments
}

/// The path segments under `group`, in the coordinates of the document.
fn collect(group: &usvg::Group, out: &mut Vec<Segment>) {
    use usvg::tiny_skia_path::{PathSegment, Point as P};
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, out),
            usvg::Node::Text(t) => collect(t.flattened(), out),
            usvg::Node::Path(path) => {
                let transform = path.abs_transform();
                let map = |mut p: P| {
                    transform.map_point(&mut p);
                    (p.x, p.y)
                };
                out.extend(path.data().segments().map(|s| match s {
                    PathSegment::MoveTo(p) => Segment::Move(map(p).0, map(p).1),
                    PathSegment::LineTo(p) => Segment::Line(map(p).0, map(p).1),
                    PathSegment::QuadTo(c, p) => Segment::Quad(map(c), map(p)),
                    PathSegment::CubicTo(a, b, p) => Segment::Cubic(map(a), map(b), map(p)),
                    PathSegment::Close => Segment::Close,
                }));
            }
            usvg::Node::Image(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Segment, as_rect, outline};

    #[test]
    fn rectangles_are_told_from_other_areas() {
        let p = umlcraft_nsd::draw::px;
        let rect = [p(2, 3), p(10, 3), p(10, 7), p(2, 7)];
        assert_eq!(as_rect(&rect), Some((2, 3, 10, 7)));
        let triangle = [p(0, 0), p(10, 0), p(5, 8)];
        assert_eq!(as_rect(&triangle), None);
        let slanted = [p(0, 0), p(10, 1), p(10, 7), p(0, 7)];
        assert_eq!(as_rect(&slanted), None);
    }

    #[test]
    fn turned_texts_read_upwards_from_their_start() {
        let family = umlcraft_nsd::Metrics::for_platform().font_family();
        let segments = outline("Branch", family);
        // Without a font for the family (a bare Linux build machine) there is nothing to
        // check; Windows always has Arial.
        assert!(!cfg!(windows) || !segments.is_empty());
        if segments.is_empty() {
            return;
        }
        let points: Vec<(f32, f32)> = segments
            .iter()
            .filter_map(|s| match *s {
                Segment::Move(x, y) | Segment::Line(x, y) => Some((x, y)),
                Segment::Quad(_, p) | Segment::Cubic(_, _, p) => Some(p),
                Segment::Close => None,
            })
            .collect();
        let (min_x, max_x) =
            points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (min_y, max_y) =
            points.iter().fold((f32::MAX, f32::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        // Upwards from the start, the glyphs' tops to the left of the baseline.
        assert!(max_y <= 1.0 && min_y < -30.0, "{min_y} {max_y}");
        assert!(min_x < -6.0 && max_x <= 3.5, "{min_x} {max_x}");
    }
}
