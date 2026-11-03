//! The fixtures read without errors and write back byte for byte.
//!
//! `flug.ucerm` is the flight example the feature started from (drawn in draw.io by Steven Gast,
//! translated with the reference tools, every ID marked as key), `chen-extra.ucerm` has every
//! Chen shape (ternary and identifying relationships, weak and associative entities,
//! composite, derived and multivalued attributes, IS-A), `hand.ucerm` a self-reference with
//! waypoints and labels moved by an offset, as the draw.io GUI draws them today.

use std::path::Path;

use pretty_assertions::assert_eq;
use umlcraft_erm::{LabelText, Max, NodeKind, read, write};

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read_to_string(path).unwrap()
}

fn round_trip(name: &str) -> umlcraft_erm::Diagram {
    let file = fixture(name);
    let (diagram, warnings) = read(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(warnings, vec![], "{name}");
    assert_eq!(write(&diagram), file, "{name}");
    diagram
}

#[test]
fn flight_example() {
    let d = round_trip("flug.ucerm");
    assert_eq!(d.meta.name, "flug");
    let count = |f: fn(&NodeKind) -> bool| d.nodes.iter().filter(|n| f(&n.kind)).count();
    assert_eq!(count(|k| matches!(k, NodeKind::Entity { .. })), 6);
    assert_eq!(count(|k| matches!(k, NodeKind::Relationship { .. })), 6);
    assert_eq!(count(NodeKind::is_attribute), 26);
    assert_eq!(d.texts.len(), 1);
    // Every entity's ID is its key (the drawing marked only half of them).
    let ids = d.nodes.iter().filter(|n| n.name == "ID");
    assert!(
        ids.clone().count() == 6
            && ids.into_iter().all(|n| matches!(
                n.kind,
                NodeKind::Attribute { key: Some(umlcraft_erm::Key::Primary), .. }
            ))
    );
    // Kunde – bucht: [1,1], read look-across.
    let kunde = d.nodes.iter().find(|n| n.name == "Kunde").unwrap();
    let bucht = d.nodes.iter().find(|n| n.name == "bucht").unwrap();
    let link =
        d.links.iter().find(|l| l.other(&kunde.id) == Some(&bucht.id)).expect("Kunde – bucht");
    let label = link.label.as_ref().unwrap();
    assert_eq!(label.display(), "[1,1]");
    assert_eq!(label.pos.y, 20.0);
    let many = d
        .links
        .iter()
        .filter_map(|l| l.label.as_ref())
        .filter(|l| matches!(&l.text, LabelText::Cardinality(c) if c.max == Max::Many));
    assert_eq!(many.count(), 6); // five [0,n] and one [1,n]
}

#[test]
fn all_chen_shapes() {
    let d = round_trip("chen-extra.ucerm");
    assert!(d.nodes.iter().any(|n| matches!(n.kind, NodeKind::IsA { total: true, .. })));
    assert!(d.nodes.iter().any(|n| matches!(n.kind, NodeKind::Entity { weak: true })));
    assert!(d.nodes.iter().any(|n| matches!(n.kind, NodeKind::AssociativeEntity)));
    let straße = d.nodes.iter().find(|n| n.name == "straße").unwrap();
    assert_eq!(d.owner_of(&straße.id).unwrap().name, "adresse");
}

#[test]
fn gui_labels_with_offsets() {
    let d = round_trip("hand.ucerm");
    let moved = d.links.iter().filter_map(|l| l.label.as_ref()).find(|l| l.pos.dx == 31.0);
    assert_eq!(moved.unwrap().pos.dy, -10.0);
    assert!(d.links.iter().any(|l| l.waypoints.len() == 2));
}
