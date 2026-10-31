//! Adding elements with the sizes a new diagram uses.
//!
//! [`SizeSet::Compact`] are the sizes of the flight example the feature started from
//! (attribute 70 × 28, relationship 60 × 40); [`SizeSet::Palette`] are those of draw.io's
//! "Entity Relation" library (attribute 100 × 40, relationship 120 × 60).

use crate::check::allowed;
use crate::model::{Diagram, Id, Link, Node, NodeKind, Point, Props, Rect, Style, Text};

/// Which default sizes new shapes get.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SizeSet {
    #[default]
    Compact,
    Palette,
}

impl SizeSet {
    /// Width and height of a new node of this kind.
    pub const fn size(self, kind: &NodeKind) -> (f64, f64) {
        match (kind, self) {
            (NodeKind::Entity { .. }, _) => (100.0, 40.0),
            (NodeKind::AssociativeEntity, _) => (140.0, 60.0),
            (NodeKind::Attribute { .. }, Self::Compact) => (70.0, 28.0),
            (NodeKind::Attribute { .. }, Self::Palette) => (100.0, 40.0),
            (NodeKind::Relationship { .. }, Self::Compact) => (60.0, 40.0),
            (NodeKind::Relationship { .. }, Self::Palette) => (120.0, 60.0),
            (NodeKind::IsA { .. }, _) => (80.0, 50.0),
        }
    }
}

/// The size of a new free text.
pub const TEXT_SIZE: (f64, f64) = (100.0, 30.0);
/// How far above its owner a new attribute is put (gap between the boxes).
const ATTRIBUTE_GAP: f64 = 30.0;

/// Why two nodes could not be joined.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ConnectError {
    #[error("node {0} does not exist")]
    Missing(Id),
    #[error("{0} and {1} cannot be joined")]
    NotAllowed(&'static str, &'static str),
}

impl Diagram {
    /// Adds a node of the given kind with its center at `center`; returns its id.
    pub fn add_node(&mut self, kind: NodeKind, name: &str, center: Point, sizes: SizeSet) -> Id {
        let (w, h) = sizes.size(&kind);
        let id = Id::generate();
        self.nodes.push(Node {
            id: id.clone(),
            kind,
            name: name.to_owned(),
            bounds: Rect::new(center.x - w / 2.0, center.y - h / 2.0, w, h),
            style: Style::default(),
            props: Props::default(),
        });
        id
    }

    pub fn add_entity(&mut self, name: &str, center: Point, sizes: SizeSet) -> Id {
        self.add_node(NodeKind::entity(), name, center, sizes)
    }

    pub fn add_relationship(&mut self, name: &str, center: Point, sizes: SizeSet) -> Id {
        self.add_node(NodeKind::relationship(), name, center, sizes)
    }

    pub fn add_is_a(&mut self, center: Point, sizes: SizeSet) -> Id {
        self.add_node(NodeKind::is_a(), "IS-A", center, sizes)
    }

    /// Adds an attribute above its owner and the link to it; returns the attribute's id, or
    /// an error when the owner is missing or cannot own attributes.
    pub fn add_attribute(
        &mut self,
        owner: &Id,
        name: &str,
        sizes: SizeSet,
    ) -> Result<Id, ConnectError> {
        let bounds = self.node(owner).ok_or_else(|| ConnectError::Missing(owner.clone()))?.bounds;
        let kind = NodeKind::attribute();
        let (_, h) = sizes.size(&kind);
        let center = Point::new(bounds.center().x, bounds.y - ATTRIBUTE_GAP - h / 2.0);
        let id = self.add_node(kind, name, center, sizes);
        match self.connect(&id, owner) {
            Ok(_) => Ok(id),
            Err(e) => {
                self.nodes.retain(|n| n.id != id);
                Err(e)
            }
        }
    }

    /// Adds a free text with its top left corner at `at`.
    pub fn add_text(&mut self, text: &str, at: Point) -> Id {
        let id = Id::generate();
        self.texts.push(Text {
            id: id.clone(),
            bounds: Rect::new(at.x, at.y, TEXT_SIZE.0, TEXT_SIZE.1),
            text: text.to_owned(),
            style: Style::default(),
        });
        id
    }

    /// Adds a link from `source` to `target` if the two kinds may be joined.
    pub fn connect(&mut self, source: &Id, target: &Id) -> Result<Id, ConnectError> {
        let kind = |id: &Id| {
            self.node(id).map(|n| n.kind).ok_or_else(|| ConnectError::Missing(id.clone()))
        };
        let (a, b) = (kind(source)?, kind(target)?);
        if !allowed(&a, &b) {
            return Err(ConnectError::NotAllowed(a.type_name(), b.type_name()));
        }
        let link = Link::new(source.clone(), target.clone());
        let id = link.id.clone();
        self.links.push(link);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_nodes_have_the_sizes_of_the_set() {
        let mut d = Diagram::new("t");
        let e = d.add_entity("Kunde", Point::new(100.0, 100.0), SizeSet::Compact);
        let r = d.add_relationship("bucht", Point::new(100.0, 200.0), SizeSet::Palette);
        assert_eq!(d.node(&e).unwrap().bounds, Rect::new(50.0, 80.0, 100.0, 40.0));
        assert_eq!(d.node(&r).unwrap().bounds, Rect::new(40.0, 170.0, 120.0, 60.0));
        assert_ne!(e, r);
    }

    #[test]
    fn attributes_go_above_their_owner_and_are_linked() {
        let mut d = Diagram::new("t");
        let e = d.add_entity("Kunde", Point::new(100.0, 100.0), SizeSet::Compact);
        let a = d.add_attribute(&e, "name", SizeSet::Compact).unwrap();
        assert_eq!(d.node(&a).unwrap().bounds, Rect::new(65.0, 22.0, 70.0, 28.0));
        assert_eq!(d.owner_of(&a).unwrap().id, e);
        assert!(crate::check(&d).is_empty());
    }

    #[test]
    fn only_allowed_kinds_are_joined() {
        let mut d = Diagram::new("t");
        let a = d.add_entity("A", Point::default(), SizeSet::Compact);
        let b = d.add_entity("B", Point::default(), SizeSet::Compact);
        assert_eq!(d.connect(&a, &b), Err(ConnectError::NotAllowed("entity", "entity")));
        let i = d.add_is_a(Point::default(), SizeSet::Compact);
        assert!(d.add_attribute(&i, "x", SizeSet::Compact).is_err());
        assert_eq!(d.nodes.len(), 3);
    }
}
