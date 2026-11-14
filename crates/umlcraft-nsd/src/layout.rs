//! Where Structorizer puts every element, in whole pixels, worked out from its images.
//!
//! Layout runs in two passes: `measure` finds each element's natural size from the inside out,
//! `place` hands every element its box, which may be larger than its natural size: a sequence
//! is stretched to the width of its parent and its last element takes up spare height (the
//! branches of a structured last element take it in turn). A box is given by its lines: an
//! element at `x` of width `w` draws its left line at `x` and its right line at `x + w`, which
//! is the left line of its neighbour.

use crate::create::Keywords;
use crate::metrics::Metrics;
use crate::model::{Diagram, Element, ElementKind, RootKind, RootStyle, Sequence};
use crate::path::{Selection, Slot};

/// Space around texts and in the diagram's frame (Structorizer's `E_PADDING` at 12 px).
pub const PADDING: i32 = 20;
/// The width and height of the box that stands for an empty sequence (`∅`).
pub const EMPTY: (i32, i32) = (40, 33);
/// Case elements with more branches than this draw single simple elements turned by 90°.
pub const ROTATE_ABOVE: usize = 8;
/// The width of the bar on the left of loops and of the bands of forever loops.
const BAR: i32 = 19;
/// The height of the bottom band of forever loops, try and parallel elements.
const BAND: i32 = 20;
/// The rows of try elements: indents of the bodies.
const TRY_INDENT: i32 = 20;
const CATCH_INDENT: i32 = 40;
/// Parallel elements are at least this wide.
const PARALLEL_MIN: i32 = 31;
/// The first baseline of an alternative's condition below its top line.
pub const COND_BASELINE: i32 = 19;
/// The baseline of an alternative's branch labels above the bottom line of its head.
pub const LABEL_BASELINE: i32 = 10;
/// The least room between a branch label and the diagonal above it.
const LABEL_GAP: i32 = 3;
/// The include list above the title: its box's top and the space below it (its height and
/// caption width depend on the platform's small font, `Metrics::includes`).
pub const INCLUDES: (i32, i32) = (20, 10);
/// Case heads: the diagonal ends this far above the head's bottom line.
pub const CASE_LABELS: i32 = 24;

/// A box in pixels, by its lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// Where an element is: alternating element and branch indices from the diagram's children,
/// `[2]` the third element, `[2, 1, 0]` the first element in the second branch of that one.
pub type Path = Vec<usize>;

/// The geometry of one element beyond its box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Instructions, calls and jumps; `rotated` in case elements with many branches.
    Plain {
        rotated: bool,
    },
    /// The head's height and the x of the line between the branches.
    Alternative {
        head: i32,
        split: i32,
    },
    /// The head's height, the left x of each shown branch's column and where the diagonal
    /// turns (`x` of the default column, or the right line without one).
    Case {
        head: i32,
        columns: Vec<i32>,
        apex: i32,
        default: bool,
    },
    /// While and for loops: the head's height.
    Head {
        head: i32,
    },
    /// Repeat loops: the y of the footer's top line.
    Foot {
        foot: i32,
    },
    Forever,
    /// The y of the label rows (try, catch, finally) and of the bottom band.
    Try {
        rows: Vec<i32>,
        band: i32,
    },
    /// The left x of each branch's column.
    Parallel {
        columns: Vec<i32>,
    },
}

/// A laid-out sequence (the diagram's body or a branch).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeqBox {
    pub rect: Rect,
    /// Its elements, or none for an empty sequence (drawn as `∅`).
    pub elements: Vec<ElementBox>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementBox {
    pub path: Path,
    pub kind: ElementKind,
    pub rect: Rect,
    pub shape: Shape,
    /// One per branch Structorizer draws (a case element without default hides its last).
    pub branches: Vec<SeqBox>,
}

/// A whole diagram laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The image: the frame from (0, 0).
    pub size: (i32, i32),
    /// The frame's height down to the body's top line (title area).
    pub header: i32,
    pub body: SeqBox,
}

impl Layout {
    pub fn of(diagram: &Diagram, m: &Metrics) -> Layout {
        Layouter { m, labels: Keywords::of(diagram).branch_labels() }.diagram(diagram)
    }

    /// The innermost element at a point, with its path.
    pub fn element_at(&self, x: i32, y: i32) -> Option<&ElementBox> {
        fn find(seq: &SeqBox, x: i32, y: i32) -> Option<&ElementBox> {
            let e = seq.elements.iter().find(|e| e.rect.contains(x, y))?;
            e.branches.iter().find_map(|b| find(b, x, y)).or(Some(e))
        }
        find(&self.body, x, y)
    }

    /// What a click at a point selects: the innermost element, an empty branch (a try's
    /// missing `finally` in its bottom band), or the diagram itself in its frame and title.
    pub fn hit(&self, x: i32, y: i32) -> Option<Selection> {
        fn find(seq: &SeqBox, path: Path, x: i32, y: i32) -> Option<Selection> {
            if seq.elements.is_empty() {
                return seq.rect.contains(x, y).then_some(Selection::Empty(path));
            }
            let e = seq.elements.iter().find(|e| e.rect.contains(x, y))?;
            e.branches
                .iter()
                .enumerate()
                .find_map(|(i, b)| find(b, branch_path(&e.path, i), x, y))
                .or_else(|| {
                    finally_band(e)
                        .filter(|band| band.contains(x, y))
                        .map(|_| Selection::Empty(branch_path(&e.path, 2)))
                })
                .or_else(|| Some(Selection::element(&e.path)))
        }
        find(&self.body, Vec::new(), x, y).or_else(|| {
            (0..=self.size.0).contains(&x).then_some(())?;
            (0..=self.size.1).contains(&y).then_some(Selection::Root)
        })
    }

    /// Where something dropped at a point goes: before an element when the point is in its
    /// upper half (outside its branches), after it in the lower half, into an empty branch,
    /// into a try's missing `finally` in its bottom band.
    pub fn slot_at(&self, x: i32, y: i32) -> Option<Slot> {
        fn find(seq: &SeqBox, path: Path, x: i32, y: i32) -> Option<Slot> {
            if seq.elements.is_empty() {
                return seq.rect.contains(x, y).then_some(Slot { seq: path, index: 0 });
            }
            let (i, e) = seq.elements.iter().enumerate().find(|(_, e)| e.rect.contains(x, y))?;
            e.branches
                .iter()
                .enumerate()
                .find_map(|(b, s)| find(s, branch_path(&e.path, b), x, y))
                .or_else(|| {
                    finally_band(e)
                        .filter(|band| band.contains(x, y))
                        .map(|_| Slot { seq: branch_path(&e.path, 2), index: 0 })
                })
                .or_else(|| {
                    let after = y >= e.rect.y + e.rect.h / 2;
                    Some(Slot { seq: path, index: i + usize::from(after) })
                })
        }
        find(&self.body, Vec::new(), x, y).or_else(|| {
            let inside = (0..=self.size.0).contains(&x) && (0..=self.size.1).contains(&y);
            inside.then(|| {
                let end = if y < self.body.rect.y { 0 } else { self.body.elements.len() };
                Slot { seq: Vec::new(), index: end }
            })
        })
    }

    /// The box of the element at `path`.
    pub fn element(&self, path: &[usize]) -> Option<&ElementBox> {
        let mut seq = &self.body;
        let mut found = None;
        for (i, &index) in path.iter().enumerate() {
            if i % 2 == 0 {
                let e = seq.elements.get(index)?;
                found = Some(e);
            } else {
                seq = found?.branches.get(index)?;
                found = None;
            }
        }
        found
    }

    /// The box of the sequence at `seq` (the body for `[]`).
    pub fn sequence(&self, seq: &[usize]) -> Option<&SeqBox> {
        match seq.split_last() {
            None => Some(&self.body),
            Some((b, e)) => self.element(e)?.branches.get(*b),
        }
    }

    /// Where the empty `finally` at `seq` (`[.., try, 2]`) would go, which is not drawn, as
    /// in Structorizer: the bottom band of its try element.
    pub fn hidden_finally(&self, seq: &[usize]) -> Option<Rect> {
        let (&branch, owner) = seq.split_last()?;
        if branch != 2 {
            return None;
        }
        finally_band(self.element(owner)?)
    }
}

/// The bottom band of a try element without a `finally`, between its side lines: where a
/// selected empty `finally` shows and where dropped elements go into it.
fn finally_band(e: &ElementBox) -> Option<Rect> {
    match e.shape {
        Shape::Try { band, .. } if e.branches.len() == 2 => Some(Rect::new(
            e.rect.x + TRY_INDENT,
            band,
            e.rect.w - 2 * TRY_INDENT,
            e.rect.bottom() - band,
        )),
        _ => None,
    }
}

fn branch_path(element: &[usize], branch: usize) -> Path {
    let mut p = element.to_vec();
    p.push(branch);
    p
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Size {
    w: i32,
    h: i32,
}

struct Layouter<'a> {
    m: &'a Metrics,
    /// The labels of an alternative's branches (`T` and `F`, or `ja` and `nein`).
    labels: (&'static str, &'static str),
}

/// Lines of `e`'s text and the widest one.
fn text_of(m: &Metrics, e: &Element) -> (usize, i32) {
    let lines = e.text();
    (lines.len(), m.max_width(&lines, false))
}

/// Java's `Math.round` of `a / b` for positive numbers.
fn round_div(a: i64, b: i64) -> i32 {
    ((2 * a + b) / (2 * b)) as i32
}

/// The case lines: discriminator, then one per branch; `%` as the last means no default.
fn case_parts(e: &Element) -> (String, Vec<String>, bool) {
    let mut lines = e.text();
    if lines.is_empty() {
        lines.push(String::new());
    }
    let discriminator = lines.remove(0);
    let default = lines.last().is_none_or(|l| l.trim() != "%");
    (discriminator, lines, default)
}

impl Layouter<'_> {
    fn line(&self) -> i32 {
        self.m.line()
    }

    /// The height of a box holding `n` lines of text.
    fn text_box(&self, n: usize) -> i32 {
        PADDING + self.line() * n as i32
    }

    fn diagram(&self, d: &Diagram) -> Layout {
        let title = d.text();
        let n = title.len().max(1) as i32;
        let title_w = self.m.title_width(&title);
        let body = self.sequence_size(&d.children, false);
        let framed = d.style() == RootStyle::Nice;
        let includes = if d.header("includeList").is_some_and(|l| !l.is_empty()) {
            self.m.includes().height + INCLUDES.1
        } else {
            0
        };
        if framed {
            let header = includes + PADDING + self.m.title_line() * n + PADDING;
            let mut w = (title_w + 2 * PADDING).max(body.w + 2 * PADDING);
            if includes > 0 {
                // The list's caption in its small font with the box's margins.
                w = w.max(self.m.includes().caption + 26 + 2 * PADDING);
            }
            let body_rect = Rect::new(PADDING, header, w - 2 * PADDING, body.h);
            let body = self.place_sequence(&d.children, body_rect, &[], false);
            Layout { size: (w, header + body_rect.h + PADDING), header, body }
        } else {
            // Unframed: a grey head over the body, which fills the width.
            let header = self.m.title_line() * n + PADDING;
            let w = body.w.max(title_w + PADDING);
            let body_rect = Rect::new(0, header, w, body.h);
            let body = self.place_sequence(&d.children, body_rect, &[], false);
            let margin = if d.kind() == RootKind::Program { 0 } else { 10 };
            Layout { size: (w, header + body_rect.h + margin), header, body }
        }
    }

    // ---- natural sizes ----

    fn sequence_size(&self, seq: &Sequence, rotate: bool) -> Size {
        if seq.elements.is_empty() {
            return Size { w: EMPTY.0, h: EMPTY.1 };
        }
        let mut size = Size { w: 0, h: 0 };
        for e in &seq.elements {
            let s = self.element_size(e, rotate && seq.elements.len() == 1);
            size.w = size.w.max(s.w);
            size.h += s.h;
        }
        size
    }

    /// The unturned size of an instruction, call or jump.
    fn plain_size(&self, e: &Element) -> Size {
        let (n, tw) = text_of(self.m, e);
        let extra = match e.kind {
            ElementKind::Call => 2 * PADDING,
            ElementKind::Jump => PADDING + PADDING / 2,
            _ => PADDING,
        };
        Size { w: tw + extra, h: self.text_box(n) }
    }

    fn element_size(&self, e: &Element, rotate: bool) -> Size {
        let (n, tw) = text_of(self.m, e);
        match e.kind {
            ElementKind::Instruction | ElementKind::Call | ElementKind::Jump => {
                let s = self.plain_size(e);
                if rotate && s.h < s.w {
                    // Only the text turns: the bars of a call and the point of a jump keep
                    // their room across, as Structorizer draws them.
                    let sides = s.w - tw - PADDING;
                    Size { w: s.h + sides, h: tw + PADDING }
                } else {
                    s
                }
            }
            ElementKind::Alternative => {
                let (head, min_w, l, r) = self.alternative_parts(e, n);
                Size { w: min_w.max(l.w + r.w), h: head + l.h.max(r.h) }
            }
            ElementKind::Case => {
                let (columns, head, w, _) = self.case_columns(e);
                let h = columns.iter().map(|(_, s)| s.h).max().unwrap_or(0);
                Size { w, h: head + h }
            }
            ElementKind::For | ElementKind::While => {
                let body = self.sequence_size(&e.branches[0], false);
                Size { w: (tw + PADDING).max(body.w + PADDING), h: self.text_box(n) + body.h }
            }
            ElementKind::Repeat => {
                let body = self.sequence_size(&e.branches[0], false);
                Size { w: (tw + PADDING).max(body.w + PADDING), h: body.h + self.text_box(n) }
            }
            ElementKind::Forever => {
                let body = self.sequence_size(&e.branches[0], false);
                Size { w: body.w + PADDING, h: PADDING + body.h + BAND }
            }
            ElementKind::Try => {
                let (rows, w) = self.try_parts(e);
                Size { w, h: rows.iter().sum::<i32>() + BAND }
            }
            ElementKind::Parallel => {
                let sizes: Vec<Size> =
                    e.branches.iter().map(|b| self.sequence_size(b, false)).collect();
                let w = (sizes.iter().map(|s| s.w).sum::<i32>() + 1).max(PARALLEL_MIN);
                let h = sizes.iter().map(|s| s.h).max().unwrap_or(EMPTY.1);
                Size { w, h: BAND + h + BAND }
            }
        }
    }

    /// Head height, the head's minimum width (the condition fits into the triangle) and the
    /// branches' sizes, at least as wide as their labels need.
    fn alternative_parts(&self, e: &Element, n: usize) -> (i32, i32, Size, Size) {
        let head = self.line() * n.max(1) as i32 + 2 * PADDING - 1;
        // Each line of the condition fits between the diagonals at its baseline (19 px below
        // the top for the first, a line further for each next).
        let lines = e.text();
        let min_w = lines
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let depth = head + 1 - COND_BASELINE - self.line() * i as i32;
                let c = self.m.width(l, false);
                round_div(i64::from(c + PADDING) * i64::from(head + 1), i64::from(depth.max(1)))
            })
            .max()
            .unwrap_or(0);
        let mut l = self.sequence_size(&e.branches[0], false);
        let mut r = self.sequence_size(&e.branches[1], false);
        let (yes, no) = (self.m.width(self.labels.0, false), self.m.width(self.labels.1, false));
        l.w = l.w.max(2 * yes + 16);
        r.w = r.w.max(no + 23);
        if self.labels != (TRUE_LABEL, FALSE_LABEL) {
            // Longer labels (`ja`, `nein`) get columns wide enough that the diagonals pass above
            // them; Structorizer's rule above is made for its one-letter `T` and `F`.
            l.w = l.w.max(self.label_clearance(head, 10 + yes));
            r.w = r.w.max(self.label_clearance(head, 16 + no) - 1);
        }
        (head, min_w, l, r)
    }

    /// The width a branch column of an alternative needs so that its diagonal clears the label
    /// at the column's outer side: `inset` is the room from the outer line to the label's inner
    /// end. The diagonal is nearest at the top of the label, `LABEL_BASELINE` plus the font's
    /// cap height above the head's bottom line.
    fn label_clearance(&self, head: i32, inset: i32) -> i32 {
        let top = LABEL_BASELINE + self.line() * 3 / 4;
        let free = i64::from((head - top).max(1));
        let need = i64::from(inset + LABEL_GAP) * i64::from(head);
        ((need + free - 1) / free) as i32
    }

    /// The shown branches with their natural column sizes, the head's height, the total width
    /// and whether a default column is shown.
    fn case_columns(&self, e: &Element) -> (Vec<(usize, Size)>, i32, i32, bool) {
        let (discriminator, labels, default) = case_parts(e);
        let rotate = e.branches.len() > ROTATE_ABOVE;
        let shown = if default { e.branches.len() } else { e.branches.len().saturating_sub(1) };
        let columns: Vec<(usize, Size)> = (0..shown)
            .map(|i| {
                let mut s = self.sequence_size(&e.branches[i], rotate);
                let label = labels.get(i).map_or(0, |l| self.m.width(l, false));
                s.w = s.w.max(label + PADDING / 2);
                (i, s)
            })
            .collect();
        let head = 2 * self.line() + 2 * PADDING;
        let sum: i32 = columns.iter().map(|(_, s)| s.w).sum();
        let w = sum.max(self.m.width(&discriminator, false) + PADDING);
        (columns, head, w, default)
    }

    /// Label row and body heights of a try element in order, and its width.
    fn try_parts(&self, e: &Element) -> (Vec<i32>, i32) {
        let label = self.text_box(1);
        let body = self.sequence_size(&e.branches[0], false);
        let catch = self.sequence_size(&e.branches[1], false);
        let variable = e.text().first().cloned().unwrap_or_default();
        let catch_label = self.m.width(&format!("{CATCH_LABEL} {variable}"), false);
        let mut rows = vec![label, body.h, label, catch.h];
        let mut w = (body.w + 2 * TRY_INDENT)
            .max(catch.w + CATCH_INDENT + TRY_INDENT)
            .max(catch_label + 30);
        if !e.branches[2].elements.is_empty() {
            let finally = self.sequence_size(&e.branches[2], false);
            rows.extend([label, finally.h]);
            w = w.max(finally.w + 2 * TRY_INDENT);
        }
        (rows, w)
    }

    // ---- placing ----

    fn place_sequence(&self, seq: &Sequence, rect: Rect, path: &[usize], rotate: bool) -> SeqBox {
        let mut elements = Vec::new();
        let mut y = rect.y;
        let count = seq.elements.len();
        for (i, e) in seq.elements.iter().enumerate() {
            let natural = self.element_size(e, rotate && count == 1);
            let h = if i + 1 == count { rect.bottom() - y } else { natural.h };
            let mut p = path.to_vec();
            p.push(i);
            let rect = Rect::new(rect.x, y, rect.w, h);
            elements.push(self.place_element(e, rect, p, rotate && count == 1));
            y += h;
        }
        SeqBox { rect, elements }
    }

    fn branch(&self, seq: &Sequence, rect: Rect, path: &[usize], b: usize) -> SeqBox {
        let mut p = path.to_vec();
        p.push(b);
        self.place_sequence(seq, rect, &p, false)
    }

    fn place_element(&self, e: &Element, r: Rect, path: Path, rotate: bool) -> ElementBox {
        let (n, _) = text_of(self.m, e);

        let mut branches = Vec::new();
        let shape = match e.kind {
            ElementKind::Instruction | ElementKind::Call | ElementKind::Jump => {
                // Turned only where that makes it narrower (as measured).
                let s = self.plain_size(e);
                Shape::Plain { rotated: rotate && s.h < s.w }
            }
            ElementKind::Alternative => {
                let (head, _, _, right) = self.alternative_parts(e, n);
                let split = r.right() - right.w - 1;
                let h = r.h - head;
                branches.push(self.branch(
                    &e.branches[0],
                    Rect::new(r.x, r.y + head, split - r.x, h),
                    &path,
                    0,
                ));
                branches.push(self.branch(
                    &e.branches[1],
                    Rect::new(split, r.y + head, r.right() - split, h),
                    &path,
                    1,
                ));
                Shape::Alternative { head, split }
            }
            ElementKind::Case => {
                let (columns, head, _, default) = self.case_columns(e);
                let rotate = e.branches.len() > ROTATE_ABOVE;
                let mut x = r.x;
                let mut lefts = Vec::new();
                for (k, (i, s)) in columns.iter().enumerate() {
                    let w = if k + 1 == columns.len() { r.right() - x } else { s.w };
                    lefts.push(x);
                    let mut p = path.clone();
                    p.push(*i);
                    branches.push(self.place_sequence(
                        &e.branches[*i],
                        Rect::new(x, r.y + head, w, r.h - head),
                        &p,
                        rotate,
                    ));
                    x += w;
                }
                let apex = if default { *lefts.last().unwrap_or(&r.x) } else { r.right() };
                Shape::Case { head, columns: lefts, apex, default }
            }
            ElementKind::For | ElementKind::While => {
                let head = self.text_box(n);
                branches.push(self.branch(
                    &e.branches[0],
                    Rect::new(r.x + BAR, r.y + head - 1, r.w - BAR, r.h - head + 1),
                    &path,
                    0,
                ));
                Shape::Head { head: head - 1 }
            }
            ElementKind::Repeat => {
                let foot = r.bottom() - self.text_box(n);
                branches.push(self.branch(
                    &e.branches[0],
                    Rect::new(r.x + BAR, r.y, r.w - BAR, foot - r.y),
                    &path,
                    0,
                ));
                Shape::Foot { foot }
            }
            ElementKind::Forever => {
                branches.push(self.branch(
                    &e.branches[0],
                    Rect::new(r.x + BAR, r.y + BAR, r.w - BAR, r.h - BAR - BAND),
                    &path,
                    0,
                ));
                Shape::Forever
            }
            ElementKind::Try => {
                let (heights, natural) = self.try_parts(e);
                // Spare height goes to the bottom band. Given more width, the bodies end a
                // pixel closer to the right line.
                let right = if r.w > natural { TRY_INDENT - 1 } else { TRY_INDENT };
                let mut y = r.y;
                let mut rows = Vec::new();
                for k in 0..heights.len() / 2 {
                    rows.push(y);
                    y += heights[2 * k];
                    let h = heights[2 * k + 1];
                    // Bodies in order: try, catch (indented further), finally.
                    let left = if k == 1 { CATCH_INDENT } else { TRY_INDENT };
                    let rect = Rect::new(r.x + left, y, r.w - left - right, h);
                    branches.push(self.branch(&e.branches[k], rect, &path, k));
                    y += h;
                }
                Shape::Try { rows, band: y }
            }
            ElementKind::Parallel => {
                let sizes: Vec<Size> =
                    e.branches.iter().map(|b| self.sequence_size(b, false)).collect();
                let mut x = r.x;
                let mut lefts = Vec::new();
                for (i, s) in sizes.iter().enumerate() {
                    let w = if i + 1 == sizes.len() { r.right() - x } else { s.w };
                    lefts.push(x);
                    branches.push(self.branch(
                        &e.branches[i],
                        Rect::new(x, r.y + BAND, w, r.h - 2 * BAND),
                        &path,
                        i,
                    ));
                    x += w;
                }
                Shape::Parallel { columns: lefts }
            }
        };
        ElementBox { path, kind: e.kind, rect: r, shape, branches }
    }
}

/// The labels of an alternative's branches and of try elements (Structorizer's defaults).
pub const TRUE_LABEL: &str = "T";
pub const FALSE_LABEL: &str = "F";
pub const TRY_LABEL: &str = "try";
pub const CATCH_LABEL: &str = "catch";
pub const FINALLY_LABEL: &str = "finally";
/// The caption of the include list.
pub const INCLUDES_LABEL: &str = "Included diagrams:";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::lines;
    use crate::model::Element;

    fn diagram(elements: Vec<Element>) -> Diagram {
        let made = crate::create::Provenance { author: "", date: "", origin: "" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["t"]), &made);
        d.children = Sequence::new(elements);
        d
    }

    fn first(d: &Diagram) -> ElementBox {
        Layout::of(d, Metrics::linux()).body.elements[0].clone()
    }

    #[test]
    fn clicks_and_drops_find_their_place() {
        let mut alt = Element::alternative(&lines(&["c"]));
        alt.branches[0] = Sequence::new(vec![Element::instruction(&lines(&["x"]))]);
        let d = diagram(vec![Element::instruction(&lines(&["a"])), alt]);
        let l = Layout::of(&d, Metrics::linux());
        let a = l.element(&[0]).unwrap().rect;
        let x = l.element(&[1, 0, 0]).unwrap().rect;
        let f = l.sequence(&[1, 1]).unwrap().rect;
        assert_eq!(l.hit(a.x + 2, a.y + 2), Some(Selection::element(&[0])));
        assert_eq!(l.hit(x.x + 2, x.y + 2), Some(Selection::element(&[1, 0, 0])));
        assert_eq!(l.hit(f.x + 2, f.y + 2), Some(Selection::Empty(vec![1, 1])));
        assert_eq!(l.hit(2, 2), Some(Selection::Root));
        assert_eq!(l.hit(-5, 2), None);
        assert_eq!(l.slot_at(a.x + 2, a.y + 2), Some(Slot { seq: vec![], index: 0 }));
        assert_eq!(l.slot_at(a.x + 2, a.bottom() - 2), Some(Slot { seq: vec![], index: 1 }));
        assert_eq!(l.slot_at(x.x + 2, x.bottom() - 2), Some(Slot { seq: vec![1, 0], index: 1 }));
        assert_eq!(l.slot_at(f.x + 2, f.y + 2), Some(Slot { seq: vec![1, 1], index: 0 }));
        assert_eq!(l.slot_at(2, l.size.1 - 2), Some(Slot { seq: vec![], index: 2 }));
    }

    #[test]
    fn an_empty_finally_lies_in_the_bottom_band() {
        let mut t = Element::try_block("ex");
        t.branches[0] = Sequence::new(vec![Element::instruction(&lines(&["x"]))]);
        let l = Layout::of(&diagram(vec![t]), Metrics::linux());
        let e = l.element(&[0]).unwrap().rect;
        let band = l.hidden_finally(&[0, 2]).unwrap();
        assert_eq!((band.x, band.right(), band.bottom()), (e.x + 20, e.right() - 20, e.bottom()));
        assert!(band.h > 0);
        assert_eq!(l.slot_at(band.x + 2, band.y + 2), Some(Slot { seq: vec![0, 2], index: 0 }));
        // A click into the band selects the empty finally, ready for pasting into it.
        let finally = Selection::Empty(vec![0, 2]);
        assert_eq!(l.hit(band.x + 2, band.y + 2), Some(finally.clone()));
        assert!(finally.is_valid(&diagram(vec![Element::try_block("ex")])));
        // Outside the band the try's own places stay.
        assert_eq!(l.slot_at(e.x + 2, e.bottom() - 2), Some(Slot { seq: vec![], index: 1 }));
        assert_eq!(l.hit(e.x + 2, e.bottom() - 2), Some(Selection::element(&[0])));
        assert_eq!(l.hidden_finally(&[0, 1]), None);
        // A finally with elements is drawn and has its own box.
        let mut t = Element::try_block("ex");
        t.branches[2] = Sequence::new(vec![Element::instruction(&lines(&["y"]))]);
        let l = Layout::of(&diagram(vec![t]), Metrics::linux());
        assert_eq!(l.hidden_finally(&[0, 2]), None);
        assert!(l.sequence(&[0, 2]).is_some());
    }

    #[test]
    fn turned_calls_are_placed_as_measured() {
        // Two short lines: a call is 56 wide and 46 high, so with many branches it is turned.
        // Only its text turns: its bars keep their room across, so Structorizer's export
        // draws it 66 wide (two lines of 13, padding, 20 for the bars), a jump 43 and an
        // instruction 33 for one line.
        let mut case =
            Element::case(&lines(&["c", "1", "2", "3", "4", "5", "6", "7", "8", "9", "%"]));
        case.branches[0] = Sequence::new(vec![Element::call(&lines(&["ab", "cd"]))]);
        case.branches[1] = Sequence::new(vec![Element::jump(&lines(&["leave"]))]);
        case.branches[2] = Sequence::new(vec![Element::instruction(&lines(&["w <- 6"]))]);
        let l = Layout::of(&diagram(vec![case]), Metrics::linux());
        let call = l.element(&[0, 0, 0]).unwrap();
        assert_eq!(call.shape, Shape::Plain { rotated: true });
        assert_eq!(call.rect.w, 66);
        assert_eq!(l.element(&[0, 1, 0]).unwrap().rect.w, 43);
        assert_eq!(l.element(&[0, 2, 0]).unwrap().rect.w, 33);
    }

    #[test]
    fn instructions_are_their_text_and_padding() {
        let d = diagram(vec![Element::instruction(&lines(&["hello world", "x"]))]);
        let e = first(&d);
        assert_eq!((e.rect.w, e.rect.h), (64 + 20, 20 + 2 * 13));
    }

    #[test]
    fn a_long_condition_widens_the_head_to_fit_the_triangle() {
        // 120 px of condition: round((120 + 20) * 53 / 34) = 218, as Structorizer draws it.
        let mut alt = Element::alternative(&lines(&["xxxxxxxxxxxxxxxxxxxx"]));
        alt.branches[1].fit_color();
        let e = first(&diagram(vec![alt]));
        assert_eq!(e.rect.w, 218);
        assert_eq!(e.shape, Shape::Alternative { head: 52, split: 20 + 218 - 40 - 1 });
    }

    #[test]
    fn case_elements_turn_simple_branches_only_when_that_is_narrower() {
        let labels: Vec<String> = std::iter::once("x".to_owned())
            .chain((1..9).map(|k| k.to_string()))
            .chain(std::iter::once("default".to_owned()))
            .collect();
        let mut wide = Element::case(&labels);
        let mut narrow = Element::case(&labels);
        for b in &mut wide.branches {
            b.elements.push(Element::instruction(&lines(&["xxxx"])));
            b.fit_color();
        }
        for b in &mut narrow.branches {
            b.elements.push(Element::instruction(&lines(&["a"])));
            b.fit_color();
        }
        let turned = |e: &ElementBox| {
            e.branches.iter().all(|b| b.elements[0].shape == Shape::Plain { rotated: true })
        };
        assert!(turned(&first(&diagram(vec![wide]))));
        assert!(!turned(&first(&diagram(vec![narrow]))));
    }

    #[test]
    fn spare_height_goes_to_the_last_element() {
        let mut alt = Element::alternative(&lines(&["c"]));
        alt.branches[0].elements.push(Element::instruction(&lines(&["a"])));
        alt.branches[0].fit_color();
        for t in ["b", "c", "d"] {
            alt.branches[1].elements.push(Element::instruction(&lines(&[t])));
        }
        alt.branches[1].fit_color();
        let e = first(&diagram(vec![alt]));
        assert_eq!(e.branches[0].elements[0].rect.h, 3 * 33);
    }

    #[test]
    fn points_find_the_innermost_element() {
        let mut w = Element::while_loop(&lines(&["c"]));
        w.branches[0].elements.push(Element::instruction(&lines(&["a"])));
        w.branches[0].fit_color();
        let layout = Layout::of(&diagram(vec![w]), Metrics::linux());
        assert_eq!(layout.element_at(25, 60).map(|e| e.path.clone()), Some(vec![0]));
        assert_eq!(layout.element_at(60, 100).map(|e| e.path.clone()), Some(vec![0, 0, 0]));
        assert!(layout.element_at(5, 5).is_none());
    }
}
