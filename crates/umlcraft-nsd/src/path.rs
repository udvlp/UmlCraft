//! Addresses in a diagram. A path alternates element and branch indices from the diagram's
//! children (see [`layout::Path`](crate::layout::Path)): odd lengths name elements, even ones
//! sequences, `[]` the diagram's body and `[2, 1]` the second branch of its third element.

use crate::model::{Diagram, Element, Sequence};

pub use crate::layout::Path;

/// The sequence at `seq` (an even-length path).
pub fn sequence<'a>(d: &'a Diagram, seq: &[usize]) -> Option<&'a Sequence> {
    let mut s = &d.children;
    for pair in seq.chunks(2) {
        let [e, b] = pair else { return None };
        s = s.elements.get(*e)?.branches.get(*b)?;
    }
    Some(s)
}

pub fn sequence_mut<'a>(d: &'a mut Diagram, seq: &[usize]) -> Option<&'a mut Sequence> {
    let mut s = &mut d.children;
    for pair in seq.chunks(2) {
        let [e, b] = pair else { return None };
        s = s.elements.get_mut(*e)?.branches.get_mut(*b)?;
    }
    Some(s)
}

/// The element at `path` (an odd-length path).
pub fn element<'a>(d: &'a Diagram, path: &[usize]) -> Option<&'a Element> {
    let (last, seq) = path.split_last()?;
    sequence(d, seq)?.elements.get(*last)
}

pub fn element_mut<'a>(d: &'a mut Diagram, path: &[usize]) -> Option<&'a mut Element> {
    let (last, seq) = path.split_last()?;
    sequence_mut(d, seq)?.elements.get_mut(*last)
}

/// Where `new` first differs from `old`, reading top down: the innermost element whose own text,
/// comment, colour or branches changed, or the place in a sequence where one has an element the
/// other lacks. `None` when the elements are the same (the root's own text may still differ).
/// After an undo, what it names in `new` is what came back or changed.
pub fn first_difference(old: &Diagram, new: &Diagram) -> Option<Path> {
    fn same_itself(a: &Element, b: &Element) -> bool {
        a.kind == b.kind && a.attrs == b.attrs && a.branches.len() == b.branches.len()
    }
    fn in_sequence(a: &Sequence, b: &Sequence, at: &mut Path) -> bool {
        for (i, (x, y)) in a.elements.iter().zip(&b.elements).enumerate() {
            if x == y {
                continue;
            }
            at.push(i);
            if same_itself(x, y) {
                for (j, (p, q)) in x.branches.iter().zip(&y.branches).enumerate() {
                    if p == q {
                        continue;
                    }
                    at.push(j);
                    if in_sequence(p, q, at) {
                        return true;
                    }
                    at.pop();
                }
            }
            return true;
        }
        if a.elements.len() == b.elements.len() {
            return false;
        }
        at.push(a.elements.len().min(b.elements.len()));
        true
    }
    let mut at = Vec::new();
    in_sequence(&old.children, &new.children, &mut at).then_some(at)
}

/// A place between two elements of a sequence: before the element `index`, or at the end.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Slot {
    pub seq: Path,
    pub index: usize,
}

/// Consecutive elements `start..end` of one sequence, at least one.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub seq: Path,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn one(path: &[usize]) -> Option<Span> {
        let (last, seq) = path.split_last()?;
        Some(Span { seq: seq.to_vec(), start: *last, end: last + 1 })
    }

    /// The path of the span's first element.
    pub fn first(&self) -> Path {
        self.element(self.start)
    }

    pub fn element(&self, index: usize) -> Path {
        let mut p = self.seq.clone();
        p.push(index);
        p
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Whether `path` (element or sequence) is one of the span's elements or inside one.
    pub fn covers(&self, path: &[usize]) -> bool {
        path.len() > self.seq.len()
            && path.starts_with(&self.seq)
            && (self.start..self.end).contains(&path[self.seq.len()])
    }

    /// The element that stays put when the span is stretched: `anchor` (where a selection
    /// extended with Shift began) if that is one of the span's two ends, else the first.
    pub fn fixed(&self, anchor: Option<&[usize]>) -> usize {
        anchor
            .filter(|a| a.len() == self.seq.len() + 1 && a.starts_with(&self.seq))
            .map(|a| a[self.seq.len()])
            .filter(|&i| i == self.start || i + 1 == self.end)
            .unwrap_or(self.start)
    }

    /// The span from its fixed element (see [`Span::fixed`]) to the element `to` of the same
    /// sequence.
    pub fn to(&self, anchor: Option<&[usize]>, to: usize) -> Span {
        let fixed = self.fixed(anchor);
        Span { seq: self.seq.clone(), start: fixed.min(to), end: fixed.max(to) + 1 }
    }

    /// The span with its loose end (the one that is not fixed) moved one element up or down
    /// in a sequence of `len` elements, growing or shrinking it; `None` at the sequence's ends.
    pub fn step(&self, anchor: Option<&[usize]>, up: bool, len: usize) -> Option<Span> {
        let fixed = self.fixed(anchor);
        let loose = if fixed == self.start { self.end - 1 } else { self.start };
        let to = if up { loose.checked_sub(1)? } else { Some(loose + 1).filter(|&i| i < len)? };
        Some(self.to(anchor, to))
    }
}

/// What is selected in a diagram.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Selection {
    #[default]
    Nothing,
    /// The diagram as a whole (its frame and title).
    Root,
    Elements(Span),
    /// An empty sequence (an `∅` branch or an empty diagram's body).
    Empty(Path),
}

impl Selection {
    pub fn element(path: &[usize]) -> Selection {
        Span::one(path).map_or(Selection::Nothing, Selection::Elements)
    }

    pub fn span(&self) -> Option<&Span> {
        match self {
            Selection::Elements(s) => Some(s),
            _ => None,
        }
    }

    /// The single selected element.
    pub fn single(&self) -> Option<Path> {
        self.span().filter(|s| s.len() == 1).map(Span::first)
    }

    /// Where something inserted before (`after` false) or after the selection goes.
    pub fn slot(&self, after: bool) -> Option<Slot> {
        match self {
            Selection::Elements(s) => {
                Some(Slot { seq: s.seq.clone(), index: if after { s.end } else { s.start } })
            }
            Selection::Empty(seq) => Some(Slot { seq: seq.clone(), index: 0 }),
            Selection::Root => Some(Slot { seq: Vec::new(), index: 0 }),
            Selection::Nothing => None,
        }
    }

    /// Whether the selection still names something in `d`.
    pub fn is_valid(&self, d: &Diagram) -> bool {
        match self {
            Selection::Nothing | Selection::Root => true,
            Selection::Elements(s) => {
                !s.is_empty() && sequence(d, &s.seq).is_some_and(|q| s.end <= q.elements.len())
            }
            Selection::Empty(seq) => sequence(d, seq).is_some_and(|q| q.elements.is_empty()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Element, Provenance, RootKind, lines};

    fn sample() -> Diagram {
        let made = Provenance { author: "t", date: "2026-10-29", origin: "test" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["demo"]), &made);
        let mut alt = Element::alternative(&lines(&["a > b"]));
        alt.branches[0] = Sequence::new(vec![Element::instruction(&lines(&["x"]))]);
        d.children = Sequence::new(vec![
            Element::instruction(&lines(&["a"])),
            alt,
            Element::instruction(&lines(&["b"])),
        ]);
        d
    }

    #[test]
    fn paths_reach_elements_and_sequences() {
        let d = sample();
        assert_eq!(element(&d, &[0]).unwrap().text(), ["a"]);
        assert_eq!(element(&d, &[1, 0, 0]).unwrap().text(), ["x"]);
        assert!(sequence(&d, &[1, 1]).unwrap().elements.is_empty());
        assert!(element(&d, &[1, 1, 0]).is_none());
        assert!(sequence(&d, &[0, 0]).is_none());
        assert!(element(&d, &[]).is_none());
    }

    #[test]
    fn the_first_difference_names_what_changed() {
        let d = sample();
        assert_eq!(first_difference(&d, &d), None);
        // An element deleted, then back: the place where it is again.
        let mut gone = d.clone();
        gone.children.elements.remove(1);
        assert_eq!(first_difference(&gone, &d), Some(vec![1]));
        // Deleted at the end: where it was, which the shorter one lacks.
        let mut last = d.clone();
        last.children.elements.pop();
        assert_eq!(first_difference(&last, &d), Some(vec![2]));
        // A change inside a branch names the element there, not the one around it.
        let mut inner = d.clone();
        inner.children.elements[1].branches[0].elements[0].set_text(&lines(&["y"]));
        assert_eq!(first_difference(&d, &inner), Some(vec![1, 0, 0]));
        // A change of the element itself names it.
        let mut outer = d.clone();
        outer.children.elements[1].set_text(&lines(&["a < b"]));
        assert_eq!(first_difference(&d, &outer), Some(vec![1]));
    }

    #[test]
    fn spans_cover_their_insides() {
        let s = Span { seq: vec![], start: 1, end: 2 };
        assert!(s.covers(&[1]));
        assert!(s.covers(&[1, 0, 0]));
        assert!(!s.covers(&[2]));
        assert!(!s.covers(&[]));
        assert_eq!(s.to(None, 0), Span { seq: vec![], start: 0, end: 2 });
        assert_eq!(s.to(None, 2), Span { seq: vec![], start: 1, end: 3 });
    }

    #[test]
    fn stretching_keeps_the_anchor() {
        let span = |start, end| Span { seq: vec![1, 0], start, end };
        let anchor: &[usize] = &[1, 0, 2];
        // Shift+click [0], then [4]: from 2 both times.
        let up = span(2, 3).to(Some(anchor), 0);
        assert_eq!(up, span(0, 3));
        assert_eq!(up.to(Some(anchor), 4), span(2, 5));
        // Shift+Up, then Shift+Down: back to the anchor alone.
        let grown = span(2, 3).step(Some(anchor), true, 5).unwrap();
        assert_eq!(grown, span(1, 3));
        assert_eq!(grown.step(Some(anchor), false, 5), Some(span(2, 3)));
        assert_eq!(span(2, 3).step(Some(anchor), false, 5), Some(span(2, 4)));
        // At the sequence's ends nothing moves.
        assert_eq!(span(0, 3).step(Some(anchor), true, 5), None);
        assert_eq!(span(2, 5).step(Some(anchor), false, 5), None);
        // An anchor that is no end of the span (or elsewhere) leaves the first element fixed.
        assert_eq!(span(0, 5).fixed(Some(anchor)), 0);
        assert_eq!(span(2, 3).fixed(Some(&[0])), 2);
        assert_eq!(span(2, 4).step(None, true, 5), Some(span(2, 3)));
    }

    #[test]
    fn selections_give_their_slots() {
        let d = sample();
        let sel = Selection::element(&[1, 0, 0]);
        assert!(sel.is_valid(&d));
        assert_eq!(sel.slot(true), Some(Slot { seq: vec![1, 0], index: 1 }));
        assert_eq!(
            Selection::Empty(vec![1, 1]).slot(false),
            Some(Slot { seq: vec![1, 1], index: 0 })
        );
        assert!(!Selection::Empty(vec![1, 0]).is_valid(&d));
        assert!(!Selection::element(&[5]).is_valid(&d));
    }
}
