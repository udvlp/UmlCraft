//! Copy and paste. A copied part is a diagram of its own (the selected nodes and texts, and
//! the lines whose two ends are among them) and goes to the clipboard as the text of a
//! `.ucerm` file, so that it can be pasted into another ER window – or a whole file's text
//! pasted into a diagram.

use std::collections::{HashMap, HashSet};

use crate::edit::{EditError, Item};
use crate::model::{Diagram, Id, Point};

/// The selected part of `d`; `None` when nothing of it can be copied.
pub fn copy(d: &Diagram, items: &[Item]) -> Option<Diagram> {
    let mut nodes: HashSet<&Id> = HashSet::new();
    let mut texts: HashSet<&Id> = HashSet::new();
    for item in items {
        match item {
            Item::Node(id) => {
                nodes.insert(id);
            }
            Item::Text(id) => {
                texts.insert(id);
            }
            // A line goes along with its two ends, never alone.
            Item::Link(_) | Item::Label(_) => {}
        }
    }
    let mut part = Diagram::new(d.meta.name.clone());
    part.nodes = d.nodes.iter().filter(|n| nodes.contains(&n.id)).cloned().collect();
    part.texts = d.texts.iter().filter(|t| texts.contains(&t.id)).cloned().collect();
    // Lines between copied nodes come along, selected or not.
    part.links = d
        .links
        .iter()
        .filter(|l| nodes.contains(&l.source) && nodes.contains(&l.target))
        .cloned()
        .collect();
    (!part.nodes.is_empty() || !part.texts.is_empty()).then_some(part)
}

/// The clipboard text of a part.
pub fn to_text(part: &Diagram) -> String {
    crate::write(part)
}

/// A part from clipboard text, if it is an ER diagram.
pub fn from_text(text: &str) -> Option<Diagram> {
    crate::read(text).ok().map(|(d, _)| d)
}

/// Adds a copy of `part` to `d` with new ids, moved by `offset`; returns what was added, to
/// select it.
pub fn paste(d: &mut Diagram, part: &Diagram, offset: Point) -> Result<Vec<Item>, EditError> {
    // Old id → new id.
    fn fresh(old: &Id, map: &mut HashMap<Id, Id>) -> Id {
        let new = Id::generate();
        map.insert(old.clone(), new.clone());
        new
    }
    let mut added = Vec::new();
    let mut copy = d.clone();
    let mut map: HashMap<Id, Id> = HashMap::new();
    for n in &part.nodes {
        let mut n = n.clone();
        n.id = fresh(&n.id, &mut map);
        n.bounds.x += offset.x;
        n.bounds.y += offset.y;
        added.push(Item::Node(n.id.clone()));
        copy.nodes.push(n);
    }
    for t in &part.texts {
        let mut t = t.clone();
        t.id = fresh(&t.id, &mut map);
        t.bounds.x += offset.x;
        t.bounds.y += offset.y;
        added.push(Item::Text(t.id.clone()));
        copy.texts.push(t);
    }
    for l in &part.links {
        let (Some(source), Some(target)) = (map.get(&l.source), map.get(&l.target)) else {
            continue;
        };
        let mut l = l.clone();
        l.id = Id::generate();
        l.source = source.clone();
        l.target = target.clone();
        for p in &mut l.waypoints {
            p.x += offset.x;
            p.y += offset.y;
        }
        copy.links.push(l);
    }
    let before = crate::check(d);
    let new_error = crate::check(&copy)
        .into_iter()
        .filter(|i| i.severity() == crate::Severity::Error)
        .find(|i| !before.contains(i));
    if let Some(issue) = new_error {
        return Err(EditError::Rule(issue.kind));
    }
    *d = copy;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit;
    use crate::model::NodeKind;

    #[test]
    fn a_copied_part_keeps_the_lines_between_its_nodes() {
        let mut d = Diagram::new("t");
        let e = edit::add_node(&mut d, NodeKind::entity(), "A", Point::new(0.0, 0.0));
        let f = edit::add_node(&mut d, NodeKind::entity(), "B", Point::new(300.0, 0.0));
        let r = edit::add_node(&mut d, NodeKind::relationship(), "r", Point::new(150.0, 0.0));
        edit::connect(&mut d, &r, &e, (None, None)).unwrap();
        edit::connect(&mut d, &r, &f, (None, None)).unwrap();
        let part = copy(&d, &[Item::Node(e.clone()), Item::Node(r.clone())]).unwrap();
        assert_eq!((part.nodes.len(), part.links.len()), (2, 1));
        let part = from_text(&to_text(&part)).unwrap();
        let added = paste(&mut d, &part, Point::new(10.0, 10.0)).unwrap();
        assert_eq!(added.len(), 2);
        assert_eq!(d.nodes.len(), 5);
        assert_eq!(d.links.len(), 3);
        let pasted = d.node(added[0].id()).unwrap();
        assert_ne!(pasted.id, e);
        assert_eq!(pasted.bounds.x, d.node(&e).unwrap().bounds.x + 10.0);
        assert!(copy(&d, &[]).is_none());
        assert!(from_text("hello").is_none());
    }
}
