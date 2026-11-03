//! The box a node's text is laid out in, and node sizes that fit a name (for the import and
//! for the editor's "fit to text"). Drawing never grows a node: text that is too wide sticks
//! out, as in draw.io.

use crate::geometry::{inner_inset, inset};
use crate::metrics::{Metrics, NODE_FONT_SIZE, line_height};
use crate::model::{Node, NodeKind, Rect, Text};

/// The text box: the node's box 1 px in on each side (draw.io's label bounds less its
/// spacing), and also inside a second frame.
pub fn text_box(node: &Node) -> Rect {
    let b = inset(&node.bounds, 1.0);
    match inner_inset(&node.kind) {
        Some(by) => inset(&b, by),
        None => b,
    }
}

/// A size that fits `name` on one line, never smaller than the sizes new nodes get
/// (sizes of the flight example): ellipses and entities grow by 10 px steps, rhombi get room for
/// the text at their narrower middle. IS-A and associative entities keep their size.
pub fn fit(kind: &NodeKind, name: &str, current: (f64, f64)) -> (f64, f64) {
    let longest = name
        .split('\n')
        .map(|l| Metrics::get().width(l, NODE_FONT_SIZE, false))
        .fold(0.0, f64::max);
    let tens = |v: f64| (v / 10.0).ceil() * 10.0;
    let (w, h) = current;
    match kind {
        NodeKind::Entity { weak } => {
            (tens(longest + if *weak { 30.0 } else { 20.0 }).max(100.0), h)
        }
        NodeKind::Attribute { .. } => (tens(longest + 20.0).max(70.0), h),
        NodeKind::Relationship { .. } => (tens(longest / 0.65 + 10.0).max(60.0), h),
        NodeKind::AssociativeEntity | NodeKind::IsA { .. } => (w, h),
    }
}

/// A size that fits a free text: its longest line and its lines, with 10 px room on each
/// side, in 10 px steps.
pub fn fit_text(t: &Text) -> (f64, f64) {
    let size = t.style.font_size.unwrap_or(NODE_FONT_SIZE);
    let lines: Vec<&str> = t.text.split('\n').collect();
    let longest =
        lines.iter().map(|l| Metrics::get().width(l, size, t.style.bold)).fold(0.0, f64::max);
    let tens = |v: f64| (v / 10.0).ceil() * 10.0;
    (tens(longest + 20.0), tens(lines.len() as f64 * line_height(size) + 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_get_room() {
        assert_eq!(fit(&NodeKind::attribute(), "id", (70.0, 28.0)), (70.0, 28.0));
        // geschwindigkeit is 84.7 px wide: 110 × 28, as in the flight example.
        assert_eq!(fit(&NodeKind::attribute(), "geschwindigkeit", (70.0, 28.0)), (110.0, 28.0));
        assert_eq!(fit(&NodeKind::relationship(), "hat", (60.0, 40.0)), (60.0, 40.0));
        assert_eq!(fit(&NodeKind::relationship(), "start_flughafen", (60.0, 40.0)).0, 140.0);
        assert_eq!(fit(&NodeKind::entity(), "Kunde", (100.0, 40.0)), (100.0, 40.0));
    }

    #[test]
    fn free_texts_get_room_for_every_line() {
        let mut d = crate::Diagram::new("t");
        let id = d.add_text("Ein recht langer Hinweis\nzwei", crate::Point::default());
        let t = d.texts.iter().find(|t| t.id == id).unwrap();
        let (w, h) = fit_text(t);
        let longest = Metrics::get().width("Ein recht langer Hinweis", NODE_FONT_SIZE, false);
        assert!(w >= longest + 20.0 && w < longest + 30.0, "{w}");
        assert_eq!(h, 40.0);
    }
}
