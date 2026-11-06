//! What a laid-out diagram draws: filled areas, 1 px lines and texts, in whole pixels.
//!
//! Colours are roles (`Ink`) except the colours of elements and branches, which the diagram
//! sets: a painter draws lines, texts and the background in its theme, an export black on
//! white as Structorizer does. Fills come first, then lines, then texts.

use crate::layout::{
    CASE_LABELS, CATCH_LABEL, COND_BASELINE, ElementBox, FALSE_LABEL, FINALLY_LABEL, INCLUDES,
    INCLUDES_LABEL, Layout, PADDING, Rect, SeqBox, Shape, TRUE_LABEL, TRY_LABEL,
};
use crate::metrics::Metrics;
use crate::model::{Color, Diagram, Element, ElementKind, RootKind, RootStyle, Sequence};

/// A point in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Px {
    pub x: i32,
    pub y: i32,
}

pub fn px(x: i32, y: i32) -> Px {
    Px { x, y }
}

/// What a primitive is painted with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// Lines and texts: black in exports.
    Line,
    /// The diagram's background: white in exports.
    Background,
    /// The head of an unframed diagram: Structorizer's grey.
    Head,
    /// The marker of elements with a comment.
    Comment,
    /// The hatching of disabled elements: [`HATCH_TILE`] repeated from the image's origin, in
    /// [`HATCH`].
    Hatch,
    /// The frame of the include list.
    Includes,
    /// An element's or branch head's own colour.
    Fill(Color),
}

/// The colour of the hatching of disabled elements.
pub const HATCH: Color = Color([0x40, 0x40, 0x40]);
/// The pixels of one 5 × 10 tile of the hatching: a line falling two pixels per pixel.
pub const HATCH_TILE: [(i32, i32); 9] =
    [(0, 9), (1, 7), (1, 8), (2, 5), (2, 6), (3, 3), (3, 4), (4, 1), (4, 2)];
/// The colour of the include list's frame.
pub const INCLUDES_FRAME: Color = Color([0x80, 0x80, 0x80]);

/// Structorizer's grey of comment markers and unframed heads.
pub const GREY: Color = Color([0xc0, 0xc0, 0xc0]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Primitive {
    /// A filled area without border.
    Area { points: Vec<Px>, ink: Ink },
    /// A straight line of 1 px between pixel centres, both ends included.
    Line { from: Px, to: Px, ink: Ink },
    /// A text from its left end on its baseline; `upwards` turns it by 90° (rotated case
    /// branches), reading from bottom to top. `small` texts are those of the include list.
    Text { at: Px, text: String, bold: bool, upwards: bool, small: bool },
    /// The outline of a frame with rounded corners (subroutine diagrams).
    RoundFrame { area: (Px, Px), radius: i32 },
}

fn rect_area(r: Rect) -> Vec<Px> {
    vec![px(r.x, r.y), px(r.right(), r.y), px(r.right(), r.bottom()), px(r.x, r.bottom())]
}

/// Everything the diagram draws, in painting order.
pub fn draw(diagram: &Diagram, layout: &Layout, m: &Metrics) -> Vec<Primitive> {
    let mut d = Drawing { m, fills: Vec::new(), lines: Vec::new(), texts: Vec::new() };
    d.frame(diagram, layout);
    d.sequence(&diagram.children, &layout.body);
    let mut all = d.fills;
    all.append(&mut d.lines);
    all.append(&mut d.texts);
    all
}

struct Drawing<'a> {
    m: &'a Metrics,
    fills: Vec<Primitive>,
    lines: Vec<Primitive>,
    texts: Vec<Primitive>,
}

impl Drawing<'_> {
    fn fill(&mut self, points: Vec<Px>, ink: Ink) {
        self.fills.push(Primitive::Area { points, ink });
    }

    fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32) {
        self.lines.push(Primitive::Line { from: px(x0, y0), to: px(x1, y1), ink: Ink::Line });
    }

    fn outline(&mut self, r: Rect) {
        self.line(r.x, r.y, r.right(), r.y);
        self.line(r.x, r.bottom(), r.right(), r.bottom());
        self.line(r.x, r.y, r.x, r.bottom());
        self.line(r.right(), r.y, r.right(), r.bottom());
    }

    fn text(&mut self, x: i32, baseline: i32, text: &str, bold: bool) {
        if !text.is_empty() {
            self.texts.push(Primitive::Text {
                at: px(x, baseline),
                text: text.to_owned(),
                bold,
                upwards: false,
                small: false,
            });
        }
    }

    /// Lines of text from `x`, the first baseline at `y`.
    fn lines_at(&mut self, lines: &[String], x: i32, y: i32) {
        for (i, l) in lines.iter().enumerate() {
            self.text(x, y + self.m.line() * i as i32, l, false);
        }
    }

    fn frame(&mut self, diagram: &Diagram, layout: &Layout) {
        let (w, h) = layout.size;
        let kind = diagram.kind();
        let outline = match kind {
            RootKind::Program => rect_area(Rect::new(0, 0, w, h)),
            RootKind::Sub => rounded(Rect::new(0, 0, w, h), 10),
            RootKind::Includable => {
                // Unframed diagrams cut their corners less.
                let c = if diagram.style() == RootStyle::Abbr { 10 } else { 14 };
                vec![px(c, 0), px(w, 0), px(w, h - c), px(w - c, h), px(0, h), px(0, c)]
            }
        };
        self.fill(outline.clone(), Ink::Background);
        if !diagram.comment().iter().all(String::is_empty) {
            self.fill(rect_area(Rect::new(2, 2, 2, h - 3)), Ink::Comment);
        }
        let title = diagram.text();
        if diagram.style() == RootStyle::Abbr {
            self.fill(rect_area(Rect::new(0, 0, w, layout.header - 1)), Ink::Head);
            if kind != RootKind::Program {
                let bottom = layout.body.rect.bottom() + 1;
                self.fill(rect_area(Rect::new(0, bottom, w, h - bottom)), Ink::Head);
                self.line(0, bottom, w, bottom);
            }
            self.line(0, layout.header - 1, w, layout.header - 1);
            for (i, l) in title.iter().enumerate() {
                self.text(10, 10 + self.m.title_line() * (i as i32 + 1), l, true);
            }
        } else {
            let top = layout.header - PADDING - self.m.title_line() * title.len().max(1) as i32;
            for (i, l) in title.iter().enumerate() {
                self.text(PADDING, top + self.m.title_line() * (i as i32 + 1), l, true);
            }
        }
        if kind == RootKind::Sub {
            self.lines.push(Primitive::RoundFrame { area: (px(0, 0), px(w, h)), radius: 13 });
        } else {
            for pair in outline.windows(2) {
                self.line(pair[0].x, pair[0].y, pair[1].x, pair[1].y);
            }
            let (last, first) = (outline[outline.len() - 1], outline[0]);
            self.line(last.x, last.y, first.x, first.y);
        }
        if let Some(list) = diagram.header("includeList").filter(|l| !l.is_empty()) {
            let small = self.m.includes();
            let (top, height) = (INCLUDES.0, small.height);
            let (l, r, b) = (PADDING, w - PADDING, top + height);
            for (x0, y0, x1, y1) in [
                (l, top, r - 5, top),
                (r - 5, top, r, top + 5),
                (r, top + 5, r, b - 5),
                (r, b - 5, r - 5, b),
                (r - 5, b, l, b),
                (l, b, l, top),
            ] {
                self.lines.push(Primitive::Line {
                    from: px(x0, y0),
                    to: px(x1, y1),
                    ink: Ink::Includes,
                });
            }
            let names = list.split(',').map(str::trim).collect::<Vec<_>>().join(", ");
            for (i, t) in [INCLUDES_LABEL.to_owned(), names].into_iter().enumerate() {
                self.texts.push(Primitive::Text {
                    at: px(l + 10, top + small.baseline + small.line * i as i32),
                    text: t,
                    bold: false,
                    upwards: false,
                    small: true,
                });
            }
        }
    }

    fn sequence(&mut self, seq: &Sequence, b: &SeqBox) {
        if b.elements.is_empty() {
            self.outline(b.rect);
            let w = self.m.width("\u{2205}", false);
            let x = b.rect.x + (b.rect.w - w) / 2;
            let y = b.rect.y + (b.rect.h + self.m.line()) / 2 - 1;
            self.text(x, y, "\u{2205}", false);
            return;
        }
        for (i, eb) in b.elements.iter().enumerate() {
            self.element(&seq.elements[i], eb);
        }
    }

    fn element(&mut self, e: &Element, b: &ElementBox) {
        let r = b.rect;
        self.fill(rect_area(r), Ink::Fill(e.color()));
        let text = e.text();
        match &b.shape {
            Shape::Plain { rotated } => {
                self.outline(r);
                // Where the text starts, from the edge it starts at.
                let inset = match e.kind {
                    ElementKind::Call | ElementKind::Jump => PADDING,
                    _ => PADDING / 2,
                };
                // A turned element is the upright one turned as a whole, reading upwards: its
                // left edge becomes the bottom and its top the left. `(dx, dy)` is a point
                // of the upright element, `(w, h)` its size.
                let rotated = *rotated;
                let (w, h) = if rotated { (r.h, r.w) } else { (r.w, r.h) };
                let pt = |dx: i32, dy: i32| {
                    if rotated { (r.x + dy, r.bottom() - dx) } else { (r.x + dx, r.y + dy) }
                };
                if rotated {
                    for (i, l) in text.iter().enumerate() {
                        let (x, y) = pt(inset, PADDING / 2 + self.m.line() * (i as i32 + 1));
                        self.texts.push(Primitive::Text {
                            at: px(x, y),
                            text: l.clone(),
                            bold: false,
                            upwards: true,
                            small: false,
                        });
                    }
                } else {
                    self.lines_at(&text, r.x + inset, r.y + PADDING / 2 + self.m.line());
                }
                let mut segment = |a: (i32, i32), b: (i32, i32)| {
                    let ((x1, y1), (x2, y2)) = (pt(a.0, a.1), pt(b.0, b.1));
                    self.line(x1, y1, x2, y2);
                };
                match e.kind {
                    ElementKind::Call => {
                        segment((10, 0), (10, h));
                        segment((w - 10, 0), (w - 10, h));
                    }
                    ElementKind::Jump => {
                        segment((10, 0), (0, h / 2));
                        segment((0, h / 2), (10, h));
                    }
                    _ => {}
                }
            }
            Shape::Alternative { head, split } => {
                let hy = r.y + head;
                let colors = e.branch_colors();
                for (i, tri) in [
                    vec![px(r.x, r.y), px(*split, hy), px(r.x, hy)],
                    vec![px(r.right(), r.y), px(r.right(), hy), px(*split, hy)],
                ]
                .into_iter()
                .enumerate()
                {
                    if let Some(Some(c)) = colors.get(i) {
                        self.fill(tri, Ink::Fill(*c));
                    }
                }
                self.outline(r);
                self.line(r.x, hy, r.right(), hy);
                self.line(r.x, r.y, *split, hy);
                self.line(*split, hy, r.right(), r.y);
                self.line(*split, hy - 1, *split, r.bottom());
                let tw = self.m.max_width(&text, false);
                let x = ((0.3224 * f64::from(r.w) + 0.3584 * f64::from(split - r.x)
                    - 0.5315 * f64::from(tw)
                    + 5.83)
                    .round()) as i32;
                self.lines_at(&text, r.x + x, r.y + COND_BASELINE);
                self.text(r.x + 10, hy - 10, TRUE_LABEL, false);
                let fw = self.m.width(FALSE_LABEL, false);
                self.text(r.right() - 16 - fw, hy - 10, FALSE_LABEL, false);
            }
            Shape::Case { head, columns, apex, default } => {
                let hy = r.y + head;
                let dy = hy - CASE_LABELS;
                let colors = e.branch_colors();
                self.outline(r);
                self.line(r.x, hy, r.right(), hy);
                self.line(r.x, r.y, *apex, dy);
                if *default {
                    self.line(*apex, dy, r.right(), r.y);
                    self.line(*apex, dy, *apex, hy);
                }
                let mut lines = text.clone();
                if lines.is_empty() {
                    lines.push(String::new());
                }
                let discriminator = lines.remove(0);
                for (k, &x) in columns.iter().enumerate() {
                    let right = columns.get(k + 1).copied().unwrap_or(r.right());
                    if k > 0 && (k + 1 < columns.len() || !*default) {
                        // From the bottom up to the diagonal.
                        let run = i64::from(*apex - r.x).max(1);
                        let y = r.y
                            + ((i64::from(x - r.x) * i64::from(dy - r.y) + run - 1) / run) as i32;
                        self.line(x, y, x, hy);
                    }
                    if let Some(Some(c)) = colors.get(k) {
                        let diagonal = |at: i32| {
                            if at <= *apex {
                                r.y + (at - r.x) * (dy - r.y) / (*apex - r.x).max(1)
                            } else {
                                dy - (at - *apex) * (dy - r.y) / (r.right() - *apex).max(1)
                            }
                        };
                        let area = if *default && k + 1 == columns.len() {
                            vec![px(x, dy), px(right, r.y), px(right, hy), px(x, hy)]
                        } else {
                            vec![
                                px(x, diagonal(x)),
                                px(right, diagonal(right)),
                                px(right, hy),
                                px(x, hy),
                            ]
                        };
                        self.fill(area, Ink::Fill(*c));
                    }
                    if let Some(label) = lines.get(k) {
                        let lw = self.m.width(label, false);
                        let lx = x + (right - x - lw) / 2;
                        self.text(lx, hy - 5, label, false);
                    }
                }
                let dw = self.m.width(&discriminator, false);
                let centre = (r.w + (*apex - r.x)) / 3 + 10;
                self.text(r.x + centre - dw / 2, r.y + 19, &discriminator, false);
            }
            Shape::Head { head } => {
                self.outline(r);
                self.line(r.x + 19, r.y + head, r.right(), r.y + head);
                self.lines_at(&text, r.x + PADDING / 2, r.y + PADDING / 2 + self.m.line());
            }
            Shape::Foot { foot } => {
                self.outline(r);
                self.line(r.x + 19, *foot, r.right(), *foot);
                self.lines_at(&text, r.x + PADDING / 2, foot + PADDING / 2 + self.m.line());
            }
            Shape::Forever => self.outline(r),
            Shape::Try { rows, .. } => {
                self.outline(r);
                let variable = text.first().cloned().unwrap_or_default();
                let labels = [
                    TRY_LABEL.to_owned(),
                    format!("{CATCH_LABEL} {variable}"),
                    FINALLY_LABEL.to_owned(),
                ];
                for (k, y) in rows.iter().enumerate() {
                    self.text(
                        r.x + PADDING / 2,
                        y + PADDING / 2 + self.m.line(),
                        &labels[k],
                        false,
                    );
                }
                if let Some(catch) = b.branches.get(1) {
                    let body = catch.rect;
                    let mid = body.y + body.h / 2;
                    self.line(r.x + 20, body.y, r.x + 40, mid);
                    self.line(r.x + 40, mid, r.x + 20, body.bottom());
                }
            }
            Shape::Parallel { columns } => {
                self.outline(r);
                let (top, bottom) = (r.y + 20, r.bottom() - 20);
                self.line(r.x, top, r.right(), top);
                self.line(r.x, bottom, r.right(), bottom);
                self.line(r.x, top, r.x + 20, r.y);
                self.line(r.right() - 20, r.y, r.right(), top);
                self.line(r.x, bottom, r.x + 20, r.bottom());
                self.line(r.right() - 20, r.bottom(), r.right(), bottom);
                for &x in columns.iter().skip(1) {
                    self.line(x, top, x, bottom);
                }
            }
        }
        // Shown branches come first and in order (a case hides only its last).
        for (k, sb) in b.branches.iter().enumerate() {
            self.sequence(&e.branches[k], sb);
        }
        if !e.comment().iter().all(String::is_empty) {
            // A grey bar inside the left line: along the head of forking elements, along the
            // bands of parallel ones, else the whole height.
            let bar = |from: i32, to: i32| Rect::new(r.x + 2, from + 2, 2, to - from - 3);
            let bars = match &b.shape {
                Shape::Alternative { head, .. } | Shape::Case { head, .. } => {
                    vec![bar(r.y, r.y + head)]
                }
                Shape::Parallel { .. } => {
                    vec![bar(r.y, r.y + 20), bar(r.bottom() - 20, r.bottom())]
                }
                _ => vec![bar(r.y, r.bottom())],
            };
            for area in bars {
                self.fill(rect_area(area), Ink::Comment);
            }
        }
        if e.disabled() {
            // After the fills of the branches, so that it covers them too.
            self.fill(rect_area(r), Ink::Hatch);
        }
    }
}

/// An outline with rounded corners as a polygon.
fn rounded(r: Rect, radius: i32) -> Vec<Px> {
    let mut points = Vec::new();
    let corners = [
        (r.right() - radius, r.y + radius, -90.0),
        (r.right() - radius, r.bottom() - radius, 0.0),
        (r.x + radius, r.bottom() - radius, 90.0),
        (r.x + radius, r.y + radius, 180.0),
    ];
    for (cx, cy, start) in corners {
        for step in 0..=6 {
            let a = (start + 15.0 * f64::from(step)).to_radians();
            points.push(px(
                cx + (f64::from(radius) * a.cos()).round() as i32,
                cy + (f64::from(radius) * a.sin()).round() as i32,
            ));
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::{Provenance, lines};

    /// A case with so many branches that its plain elements are turned.
    fn turned(e: Element) -> (Rect, Vec<Primitive>) {
        let mut case =
            Element::case(&lines(&["c", "1", "2", "3", "4", "5", "6", "7", "8", "9", "%"]));
        case.branches[0] = Sequence::new(vec![e]);
        let made = Provenance { author: "", date: "", origin: "" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["t"]), &made);
        d.children = Sequence::new(vec![case]);
        let m = Metrics::linux();
        let layout = Layout::of(&d, m);
        let b = layout.element(&[0, 0, 0]).unwrap();
        assert_eq!(b.shape, Shape::Plain { rotated: true });
        (b.rect, draw(&d, &layout, m))
    }

    fn texts(primitives: &[Primitive]) -> Vec<(Px, &str)> {
        primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text { at, text, upwards: true, .. } => Some((*at, text.as_str())),
                _ => None,
            })
            .collect()
    }

    fn has_line(primitives: &[Primitive], from: Px, to: Px) -> bool {
        primitives
            .iter()
            .any(|p| matches!(p, Primitive::Line { from: f, to: t, .. } if *f == from && *t == to))
    }

    #[test]
    fn a_turned_call_is_the_upright_one_turned_as_a_whole() {
        let (r, drawn) = turned(Element::call(&lines(&["ab", "cd"])));
        // Its bars run across, 10 px inside the bottom and the top.
        assert!(has_line(&drawn, px(r.x, r.bottom() - 10), px(r.right(), r.bottom() - 10)));
        assert!(has_line(&drawn, px(r.x, r.y + 10), px(r.right(), r.y + 10)));
        // Its lines start above the bottom bar, the last one's baseline inside the box.
        let line = Metrics::linux().line();
        let shown = texts(&drawn);
        let at: Vec<Px> = shown.iter().filter(|(_, t)| *t != "c").map(|(at, _)| *at).collect();
        assert_eq!(
            at,
            [px(r.x + 10 + line, r.bottom() - 20), px(r.x + 10 + 2 * line, r.bottom() - 20)]
        );
        assert!(at[1].x < r.right());
    }

    #[test]
    fn a_turned_jump_points_down() {
        let (r, drawn) = turned(Element::jump(&lines(&["leave"])));
        let mid = r.x + r.w / 2;
        assert!(has_line(&drawn, px(r.x, r.bottom() - 10), px(mid, r.bottom())));
        assert!(has_line(&drawn, px(mid, r.bottom()), px(r.right(), r.bottom() - 10)));
    }
}
