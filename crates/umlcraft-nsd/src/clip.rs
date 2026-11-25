//! Elements on the clipboard. They go there as a whole `.nsd` file holding them, the form in
//! which Structorizer puts a diagram on the system clipboard; so a diagram copied there pastes
//! its elements here, and text from anywhere else pastes as one instruction.

use crate::model::{Diagram, Element, Sequence};
use crate::{read, write};

/// The clipboard text for `elements`, taken from `from` (whose header it keeps).
pub fn copy(from: &Diagram, elements: Vec<Element>) -> String {
    let d = Diagram { attrs: from.attrs.clone(), children: Sequence::new(elements) };
    write(&d)
}

/// The clipboard text for a whole diagram.
pub fn copy_diagram(d: &Diagram) -> String {
    write(d)
}

/// What pasting `text` inserts: the elements of a diagram, or one instruction with the text's
/// lines. Nothing for empty text or an empty diagram.
pub fn paste(text: &str) -> Vec<Element> {
    if let Ok(d) = read(text) {
        return d.children.elements;
    }
    let text = text.replace("\r\n", "\n");
    let text = text.trim_end_matches('\n');
    if text.trim().is_empty() {
        return Vec::new();
    }
    let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    vec![Element::instruction(&lines)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::test_diagram;
    use crate::lines;

    #[test]
    fn elements_come_back_as_they_went() {
        let d = test_diagram(Vec::new());
        let mut alt = Element::alternative(&lines(&["a > b"]));
        alt.branches[0] = Sequence::new(vec![Element::call(&lines(&["f(x)"]))]);
        let elements = vec![Element::instruction(&lines(&["a", "b"])), alt];
        let text = copy(&d, elements.clone());
        assert_eq!(paste(&text), elements);
        assert!(paste(&copy_diagram(&d)).is_empty());
    }

    #[test]
    fn other_text_is_an_instruction() {
        let pasted = paste("int x = 1;\r\nx++;\n");
        assert_eq!(pasted, vec![Element::instruction(&lines(&["int x = 1;", "x++;"]))]);
        assert!(paste(" \n").is_empty());
    }
}
