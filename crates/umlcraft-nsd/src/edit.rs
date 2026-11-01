//! Changes to a diagram, as the editor makes them. Every operation keeps the diagram in a shape
//! Structorizer writes itself: empty sequences carry `color="ffffff"` and full ones none, a case
//! element has a branch for each of its lines but the first, a parallel element's text is its
//! number of branches, a for loop's attributes follow its text. Each returns what to select
//! afterwards.

use crate::model::{Diagram, Element, ElementKind, RootKind};
use crate::path::{self, Path, Selection, Slot, Span};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    #[error("no element or sequence at this place")]
    NoSuchPlace,
    #[error("an element cannot go into itself")]
    IntoItself,
    #[error("this element has no branches to add or remove")]
    NoBranches,
    #[error("the last branch of this kind cannot be removed")]
    LastBranch,
}

type Result<T> = std::result::Result<T, EditError>;

/// The texts new elements start with: Structorizer's (its "Structure Preferences" as
/// installed), but conditions without the parentheses it puts around them, which a
/// structogram does not need. Case lines: selector, two cases, default.
pub fn default_text(kind: ElementKind) -> Vec<String> {
    let text: &[&str] = match kind {
        ElementKind::Alternative => &["?"],
        ElementKind::Case => &["?", "!", "!", "default"],
        ElementKind::For => &["for ? <- ? to ?"],
        ElementKind::While => &["while ?"],
        ElementKind::Repeat => &["until ?"],
        ElementKind::Parallel => &["2"],
        _ => &[""],
    };
    text.iter().map(|s| (*s).to_owned()).collect()
}

/// A new element of `kind` with `text` (the number of branches for a parallel element, the
/// exception's name for a try block).
pub fn new_element(d: &Diagram, kind: ElementKind, text: &[String]) -> Element {
    let mut e = match kind {
        ElementKind::Instruction => Element::instruction(text),
        ElementKind::Call => Element::call(text),
        ElementKind::Jump => Element::jump(text),
        ElementKind::Alternative => Element::alternative(text),
        ElementKind::Case => Element::case(&case_lines(text)),
        ElementKind::For => Element::for_loop(text, crate::ForLoop::Free),
        ElementKind::While => Element::while_loop(text),
        ElementKind::Repeat => Element::repeat_loop(text),
        ElementKind::Forever => Element::forever_loop(),
        ElementKind::Try => Element::try_block(text.first().map_or("", String::as_str)),
        ElementKind::Parallel => Element::parallel(parallel_count(text)),
    };
    if kind == ElementKind::For {
        fit_for(d, &mut e);
    }
    e
}

/// A case element needs a selector, at least one case and the default line.
fn case_lines(text: &[String]) -> Vec<String> {
    let mut lines = text.to_vec();
    lines.resize(lines.len().max(3), String::new());
    lines
}

fn parallel_count(text: &[String]) -> usize {
    text.first().and_then(|t| t.trim().parse().ok()).unwrap_or(2).clamp(1, 99)
}

/// Puts `elements` at `slot` and selects them.
pub fn insert(d: &mut Diagram, slot: &Slot, elements: Vec<Element>) -> Result<Selection> {
    let seq = path::sequence_mut(d, &slot.seq).ok_or(EditError::NoSuchPlace)?;
    if slot.index > seq.elements.len() || elements.is_empty() {
        return Err(EditError::NoSuchPlace);
    }
    let n = elements.len();
    seq.elements.splice(slot.index..slot.index, elements);
    seq.fit_color();
    Ok(Selection::Elements(Span { seq: slot.seq.clone(), start: slot.index, end: slot.index + n }))
}

/// Takes the elements of `span` out and returns them, selecting their neighbour.
pub fn remove(d: &mut Diagram, span: &Span) -> Result<(Vec<Element>, Selection)> {
    let seq = path::sequence_mut(d, &span.seq).ok_or(EditError::NoSuchPlace)?;
    if span.is_empty() || span.end > seq.elements.len() {
        return Err(EditError::NoSuchPlace);
    }
    let taken: Vec<Element> = seq.elements.drain(span.start..span.end).collect();
    seq.fit_color();
    let left = seq.elements.len();
    let selection = if left == 0 {
        Selection::Empty(span.seq.clone())
    } else {
        Selection::element(&span.element(span.start.min(left - 1)))
    };
    Ok((taken, selection))
}

/// Moves the elements of `span` to `slot` (a place before the move).
pub fn move_span(d: &mut Diagram, span: &Span, slot: &Slot) -> Result<Selection> {
    if span.covers(&slot.seq) {
        return Err(EditError::IntoItself);
    }
    if slot.seq == span.seq && (span.start..=span.end).contains(&slot.index) {
        // Already there.
        return Ok(Selection::Elements(span.clone()));
    }
    path::sequence(d, &slot.seq).ok_or(EditError::NoSuchPlace)?;
    let (taken, _) = remove(d, span)?;
    let target = shifted(slot, span);
    insert(d, &target, taken)
}

/// Where `slot` is once the elements of `span` are gone.
fn shifted(slot: &Slot, span: &Span) -> Slot {
    let k = span.seq.len();
    let mut s = slot.clone();
    if slot.seq == span.seq {
        if slot.index >= span.end {
            s.index -= span.len();
        }
    } else if slot.seq.len() > k && slot.seq.starts_with(&span.seq) && slot.seq[k] >= span.end {
        s.seq[k] -= span.len();
    }
    s
}

/// Moves the span one place up (`up`) or down within its sequence, as Structorizer's
/// "Move up"/"Move down".
pub fn shift(d: &mut Diagram, span: &Span, up: bool) -> Result<Selection> {
    let len = path::sequence(d, &span.seq).ok_or(EditError::NoSuchPlace)?.elements.len();
    if up && span.start == 0 || !up && span.end >= len {
        return Ok(Selection::Elements(span.clone()));
    }
    let index = if up { span.start - 1 } else { span.end + 1 };
    move_span(d, span, &Slot { seq: span.seq.clone(), index })
}

/// Copies of the elements of `span`.
pub fn copy(d: &Diagram, span: &Span) -> Result<Vec<Element>> {
    let seq = path::sequence(d, &span.seq).ok_or(EditError::NoSuchPlace)?;
    seq.elements.get(span.start..span.end).map(<[Element]>::to_vec).ok_or(EditError::NoSuchPlace)
}

/// Gives the element at `path` new text: a case element gets or loses branches with its lines
/// (keeping the default branch last), a parallel element takes the first line as its number
/// of branches, a for loop fits its attributes.
pub fn set_text(d: &mut Diagram, at: &[usize], text: &[String]) -> Result<Selection> {
    let kind = path::element(d, at).ok_or(EditError::NoSuchPlace)?.kind;
    match kind {
        ElementKind::Case => {
            let lines = case_lines(text);
            let e = path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?;
            let cases = lines.len() - 2;
            while e.branches.len() - 1 > cases {
                let last_case = e.branches.len() - 2;
                remove_branch_at(e, last_case);
            }
            while e.branches.len() - 1 < cases {
                let end = e.branches.len() - 1;
                add_branch_at(e, end);
            }
            e.set_text(&lines);
        }
        ElementKind::Parallel => {
            let n = parallel_count(text);
            let e = path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?;
            e.branches.resize_with(n, || crate::Sequence::new(Vec::new()));
            e.set_text(&[n.to_string()]);
        }
        _ => {
            let e = path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?;
            e.set_text(text);
        }
    }
    if kind == ElementKind::For {
        let mut e = path::element(d, at).cloned().ok_or(EditError::NoSuchPlace)?;
        fit_for(d, &mut e);
        *path::element_mut(d, at).ok_or(EditError::NoSuchPlace)? = e;
    }
    Ok(Selection::element(at))
}

pub fn set_comment(d: &mut Diagram, at: &[usize], comment: &[String]) -> Result<Selection> {
    path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?.set_comment(comment);
    Ok(Selection::element(at))
}

/// Applies `f` to every element of `span`.
fn each(d: &mut Diagram, span: &Span, f: impl Fn(&mut Element)) -> Result<Selection> {
    let seq = path::sequence_mut(d, &span.seq).ok_or(EditError::NoSuchPlace)?;
    let elements = seq.elements.get_mut(span.start..span.end).ok_or(EditError::NoSuchPlace)?;
    elements.iter_mut().for_each(f);
    Ok(Selection::Elements(span.clone()))
}

pub fn set_color(d: &mut Diagram, span: &Span, color: crate::Color) -> Result<Selection> {
    each(d, span, |e| e.set_color(color))
}

pub fn set_disabled(d: &mut Diagram, span: &Span, disabled: bool) -> Result<Selection> {
    each(d, span, |e| e.set_disabled(disabled))
}

/// Adds an empty branch before branch `index` of a case (a new case line `?`; not after the
/// default) or parallel element.
pub fn add_branch(d: &mut Diagram, at: &[usize], index: usize) -> Result<Selection> {
    let e = path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?;
    let last = match e.kind {
        ElementKind::Case => e.branches.len() - 1,
        ElementKind::Parallel => e.branches.len(),
        _ => return Err(EditError::NoBranches),
    };
    if index > last {
        return Err(EditError::NoSuchPlace);
    }
    add_branch_at(e, index);
    Ok(Selection::element(at))
}

/// Removes branch `index` of a case (not its default) or parallel element.
pub fn remove_branch(d: &mut Diagram, at: &[usize], index: usize) -> Result<Selection> {
    let e = path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?;
    let (cases, least) = match e.kind {
        ElementKind::Case => (e.branches.len() - 1, 1),
        ElementKind::Parallel => (e.branches.len(), 1),
        _ => return Err(EditError::NoBranches),
    };
    if index >= cases {
        return Err(EditError::NoSuchPlace);
    }
    if cases <= least {
        return Err(EditError::LastBranch);
    }
    remove_branch_at(e, index);
    Ok(Selection::element(at))
}

fn add_branch_at(e: &mut Element, index: usize) {
    e.branches.insert(index, crate::Sequence::new(Vec::new()));
    match e.kind {
        ElementKind::Case => {
            let mut lines = e.text();
            lines.insert((index + 1).min(lines.len()), "?".to_owned());
            e.set_text(&lines);
            edit_branch_colors(e, |c| c.insert(index.min(c.len()), String::new()));
        }
        _ => e.set_text(&[e.branches.len().to_string()]),
    }
}

fn remove_branch_at(e: &mut Element, index: usize) {
    e.branches.remove(index);
    match e.kind {
        ElementKind::Case => {
            let mut lines = e.text();
            if index + 1 < lines.len() {
                lines.remove(index + 1);
            }
            e.set_text(&lines);
            edit_branch_colors(e, |c| {
                if index < c.len() {
                    c.remove(index);
                }
            });
        }
        _ => e.set_text(&[e.branches.len().to_string()]),
    }
}

/// Changes the comma list of branch head colours like the branches.
fn edit_branch_colors(e: &mut Element, f: impl FnOnce(&mut Vec<String>)) {
    let Some(value) = e.attr("branch_colors") else { return };
    let mut colors: Vec<String> = value.split(',').map(str::to_owned).collect();
    f(&mut colors);
    colors.resize(e.branches.len().max(1), String::new());
    e.set_attr("branch_colors", colors.join(","));
}

/// Sets the diagram's signature, comment and kind.
pub fn set_root(
    d: &mut Diagram,
    text: &[String],
    comment: &[String],
    kind: RootKind,
) -> Result<Selection> {
    d.set_text(text);
    d.set_comment(comment);
    d.set_kind(kind);
    Ok(Selection::Root)
}

/// Fits a for loop's attributes to its text as Structorizer does on loading: `for i <- a to b
/// by s` counts, `foreach x in c` traverses, anything else is free text.
pub fn fit_for(d: &Diagram, e: &mut Element) {
    let keyword = |name: &str, default: &str| {
        d.header(name).map(str::trim).filter(|k| !k.is_empty()).unwrap_or(default).to_owned()
    };
    let (pre_for, post_for, step_for) =
        (keyword("preFor", "for"), keyword("postFor", "to"), keyword("stepFor", "by"));
    let (pre_for_in, post_for_in) = (keyword("preForIn", "foreach"), keyword("postForIn", "in"));
    let ignore_case = d.header("ignoreCase") != Some("false");
    let text = e.text().join(" ");
    let text = text.trim();

    for name in ["counterVar", "startValue", "endValue", "stepConst", "style", "insep"] {
        e.attrs.remove(name);
    }
    if let Some(rest) = after_keyword(text, &pre_for_in, ignore_case)
        && let Some((var, _)) = split_word(rest, &post_for_in, ignore_case)
    {
        e.set_attr("counterVar", var.trim());
        e.set_attr("startValue", "");
        e.set_attr("stepConst", "");
        e.set_attr("style", "TRAVERSAL");
        e.set_attr("insep", post_for_in);
        return;
    }
    let rest = after_keyword(text, &pre_for, ignore_case);
    let counting = rest.and_then(|rest| {
        let (var, rest) = rest.split_once("<-").or_else(|| rest.split_once(":="))?;
        let (start, rest) = split_word(rest, &post_for, ignore_case)?;
        let (end, step) = match split_word(rest, &step_for, ignore_case) {
            Some((end, step)) => (end, step.trim()),
            None => (rest, "1"),
        };
        Some((var.trim(), start.trim(), end.trim(), step))
    });
    match counting {
        Some((var, start, end, step)) if !var.is_empty() => {
            e.set_attr("counterVar", var);
            e.set_attr("startValue", start);
            e.set_attr("endValue", end);
            e.set_attr("stepConst", step);
            e.set_attr("style", "COUNTER");
        }
        _ => {
            e.set_attr("counterVar", "dummy_counter");
            e.set_attr("startValue", rest.unwrap_or(text).trim());
            e.set_attr("stepConst", "1");
            e.set_attr("style", "FREETEXT");
        }
    }
}

/// The text after a leading keyword followed by a space.
fn after_keyword<'a>(text: &'a str, keyword: &str, ignore_case: bool) -> Option<&'a str> {
    let head = text.get(..keyword.len())?;
    let same = if ignore_case { head.eq_ignore_ascii_case(keyword) } else { head == keyword };
    let rest = &text[keyword.len()..];
    (same && rest.starts_with(char::is_whitespace)).then_some(rest)
}

/// `text` split around the first `keyword` standing as a word between spaces.
fn split_word<'a>(text: &'a str, keyword: &str, ignore_case: bool) -> Option<(&'a str, &'a str)> {
    let mut from = 0;
    while let Some(found) = find(&text[from..], keyword, ignore_case) {
        let at = from + found;
        let end = at + keyword.len();
        let before = text[..at].ends_with(char::is_whitespace);
        let after = text[end..].starts_with(char::is_whitespace);
        if before && after {
            return Some((&text[..at], &text[end..]));
        }
        from = end;
    }
    None
}

fn find(text: &str, keyword: &str, ignore_case: bool) -> Option<usize> {
    if ignore_case {
        text.to_ascii_lowercase().find(&keyword.to_ascii_lowercase())
    } else {
        text.find(keyword)
    }
}

/// The path of the element that holds the sequence `seq`, if it is a branch.
pub fn owner(seq: &[usize]) -> Option<Path> {
    (!seq.is_empty()).then(|| seq[..seq.len() - 1].to_vec())
}

/// Whether the element at `path` has text the editor shows (all but forever loops).
pub fn has_text(kind: ElementKind) -> bool {
    kind != ElementKind::Forever
}

/// The encoded `text` attribute for `lines`, for comparisons in tests.
#[cfg(test)]
fn encoded(lines: &[&str]) -> String {
    crate::text::encode_lines(&crate::lines(lines))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Provenance, Sequence, lines, read, write};

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

    fn texts(d: &Diagram, seq: &[usize]) -> Vec<String> {
        path::sequence(d, seq).unwrap().elements.iter().map(|e| e.text().join("/")).collect()
    }

    /// The invariants every edit keeps, and that the file reads back as it is.
    fn check(d: &Diagram) {
        fn seq(s: &Sequence) {
            assert_eq!(s.attrs.get("color").is_some(), s.elements.is_empty());
            for e in &s.elements {
                match e.kind {
                    ElementKind::Case => {
                        assert_eq!(e.text().len(), e.branches.len() + 1);
                        assert_eq!(e.branch_colors().len(), e.branches.len());
                    }
                    ElementKind::Parallel => {
                        assert_eq!(e.text(), [e.branches.len().to_string()]);
                    }
                    _ => {}
                }
                e.branches.iter().for_each(seq);
            }
        }
        seq(&d.children);
        assert_eq!(&read(&write(d)).unwrap(), d);
    }

    fn span(seq: &[usize], start: usize, end: usize) -> Span {
        Span { seq: seq.to_vec(), start, end }
    }

    #[test]
    fn inserting_fills_empty_branches_and_selects_the_new() {
        let mut d = sample();
        let new = Element::instruction(&lines(&["y"]));
        let sel = insert(&mut d, &Slot { seq: vec![1, 1], index: 0 }, vec![new]).unwrap();
        assert_eq!(sel, Selection::Elements(span(&[1, 1], 0, 1)));
        assert_eq!(texts(&d, &[1, 1]), ["y"]);
        check(&d);
        let bad = insert(&mut d, &Slot { seq: vec![1, 1], index: 5 }, vec![]);
        assert_eq!(bad, Err(EditError::NoSuchPlace));
    }

    #[test]
    fn removing_selects_the_neighbour_or_the_empty_branch() {
        let mut d = sample();
        let (taken, sel) = remove(&mut d, &span(&[1, 0], 0, 1)).unwrap();
        assert_eq!(taken[0].text(), ["x"]);
        assert_eq!(sel, Selection::Empty(vec![1, 0]));
        let (_, sel) = remove(&mut d, &span(&[], 1, 3)).unwrap();
        assert_eq!(sel, Selection::element(&[0]));
        assert_eq!(texts(&d, &[]), ["a"]);
        check(&d);
    }

    #[test]
    fn moving_keeps_the_target_where_it_was() {
        let mut d = sample();
        // "a" into the false branch of the alternative, which then moves up one place.
        let sel = move_span(&mut d, &span(&[], 0, 1), &Slot { seq: vec![1, 1], index: 0 }).unwrap();
        assert_eq!(sel, Selection::Elements(span(&[0, 1], 0, 1)));
        assert_eq!(texts(&d, &[0, 1]), ["a"]);
        // "b" before the alternative.
        let sel = move_span(&mut d, &span(&[], 1, 2), &Slot { seq: vec![], index: 0 }).unwrap();
        assert_eq!(sel, Selection::Elements(span(&[], 0, 1)));
        assert_eq!(texts(&d, &[]), ["b", "a > b"]);
        check(&d);
    }

    #[test]
    fn nothing_moves_into_itself() {
        let mut d = sample();
        let into = move_span(&mut d, &span(&[], 1, 2), &Slot { seq: vec![1, 0], index: 0 });
        assert_eq!(into, Err(EditError::IntoItself));
        let same = move_span(&mut d, &span(&[], 1, 2), &Slot { seq: vec![], index: 2 });
        assert_eq!(same, Ok(Selection::Elements(span(&[], 1, 2))));
        assert_eq!(d, sample());
    }

    #[test]
    fn shifting_moves_by_one_place() {
        let mut d = sample();
        assert_eq!(
            shift(&mut d, &span(&[], 0, 1), true).unwrap(),
            Selection::Elements(span(&[], 0, 1))
        );
        assert_eq!(
            shift(&mut d, &span(&[], 0, 1), false).unwrap(),
            Selection::Elements(span(&[], 1, 2))
        );
        assert_eq!(texts(&d, &[]), ["a > b", "a", "b"]);
        shift(&mut d, &span(&[], 1, 3), true).unwrap();
        assert_eq!(texts(&d, &[]), ["a", "b", "a > b"]);
        check(&d);
    }

    #[test]
    fn case_text_brings_its_branches() {
        let mut d = sample();
        let case = new_element(&d, ElementKind::Case, &default_text(ElementKind::Case));
        assert_eq!(case.branches.len(), 3);
        insert(&mut d, &Slot { seq: vec![], index: 0 }, vec![case]).unwrap();
        let x = Element::instruction(&lines(&["in default"]));
        insert(&mut d, &Slot { seq: vec![0, 2], index: 0 }, vec![x]).unwrap();
        set_text(&mut d, &[0], &lines(&["n", "1", "2", "3", "default"])).unwrap();
        let case = path::element(&d, &[0]).unwrap();
        assert_eq!(case.branches.len(), 4);
        assert_eq!(texts(&d, &[0, 3]), ["in default"]);
        check(&d);
        set_text(&mut d, &[0], &lines(&["n"])).unwrap();
        let case = path::element(&d, &[0]).unwrap();
        assert_eq!(case.text(), ["n", "", ""]);
        assert_eq!(texts(&d, &[0, 1]), ["in default"]);
        check(&d);
    }

    #[test]
    fn branches_come_and_go_with_their_lines() {
        let mut d = sample();
        let case = Element::case(&lines(&["n", "1", "2", "default"]));
        insert(&mut d, &Slot { seq: vec![], index: 0 }, vec![case]).unwrap();
        add_branch(&mut d, &[0], 1).unwrap();
        assert_eq!(path::element(&d, &[0]).unwrap().text(), ["n", "1", "?", "2", "default"]);
        check(&d);
        assert_eq!(add_branch(&mut d, &[0], 4), Err(EditError::NoSuchPlace));
        remove_branch(&mut d, &[0], 0).unwrap();
        remove_branch(&mut d, &[0], 0).unwrap();
        assert_eq!(remove_branch(&mut d, &[0], 0), Err(EditError::LastBranch));
        assert_eq!(remove_branch(&mut d, &[0], 1), Err(EditError::NoSuchPlace));
        assert_eq!(path::element(&d, &[0]).unwrap().text(), ["n", "2", "default"]);
        check(&d);

        insert(&mut d, &Slot { seq: vec![], index: 0 }, vec![Element::parallel(2)]).unwrap();
        add_branch(&mut d, &[0], 2).unwrap();
        assert_eq!(path::element(&d, &[0]).unwrap().branches.len(), 3);
        set_text(&mut d, &[0], &lines(&["5"])).unwrap();
        assert_eq!(path::element(&d, &[0]).unwrap().branches.len(), 5);
        check(&d);
        assert_eq!(add_branch(&mut d, &[2], 0), Err(EditError::NoBranches));
    }

    #[test]
    fn colour_and_disabling_reach_every_selected_element() {
        let mut d = sample();
        let red = crate::Color([0xff, 0x80, 0x80]);
        set_color(&mut d, &span(&[], 0, 2), red).unwrap();
        set_disabled(&mut d, &span(&[], 1, 3), true).unwrap();
        let c = &d.children.elements;
        assert_eq!((c[0].color(), c[1].color(), c[2].color()), (red, red, crate::Color::WHITE));
        assert_eq!((c[0].disabled(), c[1].disabled(), c[2].disabled()), (false, true, true));
        check(&d);
    }

    #[test]
    fn the_root_takes_signature_comment_and_kind() {
        let mut d = sample();
        set_root(&mut d, &lines(&["int f(int a)"]), &lines(&["doc"]), RootKind::Sub).unwrap();
        assert_eq!(
            (d.text(), d.comment(), d.kind()),
            (lines(&["int f(int a)"]), lines(&["doc"]), RootKind::Sub)
        );
        check(&d);
    }

    #[test]
    fn texts_and_comments_change() {
        let mut d = sample();
        set_text(&mut d, &[1, 0, 0], &lines(&["y <- 1", "z"])).unwrap();
        set_comment(&mut d, &[1, 0, 0], &lines(&["why"])).unwrap();
        let e = path::element(&d, &[1, 0, 0]).unwrap();
        assert_eq!(e.attr("text"), Some(encoded(&["y <- 1", "z"]).as_str()));
        assert_eq!(e.comment(), ["why"]);
        set_comment(&mut d, &[1, 0, 0], &lines(&[""])).unwrap();
        assert_eq!(path::element(&d, &[1, 0, 0]).unwrap().attr("comment"), Some(""));
        check(&d);
    }

    /// Structorizer's own for loops (saved by it from bare texts) get the same attributes.
    #[test]
    fn for_loops_fit_their_text_like_structorizer() {
        let file = include_str!("../tests/fixtures/structorizer/for-headers.nsd");
        let d = read(file).unwrap();
        assert!(d.children.elements.len() >= 7);
        for e in &d.children.elements {
            let mut fitted = e.clone();
            fitted
                .attrs
                .0
                .retain(|a| matches!(a.name.as_str(), "text" | "comment" | "color" | "disabled"));
            fit_for(&d, &mut fitted);
            assert_eq!(&fitted, e, "{:?}", e.text());
        }
    }

    #[test]
    fn new_elements_are_what_structorizer_starts_with() {
        let d = sample();
        for kind in ElementKind::ALL {
            let e = new_element(&d, kind, &default_text(kind));
            assert_eq!(e.kind, kind);
            if has_text(kind) {
                assert!(e.attr("text").is_some());
            }
        }
        // Structorizer writes an empty text as none at all.
        let empty =
            new_element(&d, ElementKind::Instruction, &default_text(ElementKind::Instruction));
        assert_eq!(empty.attr("text"), Some(""));
        let f = new_element(&d, ElementKind::For, &default_text(ElementKind::For));
        assert_eq!(f.attr("style"), Some("COUNTER"));
        assert_eq!(f.attr("counterVar"), Some("?"));
    }
}
