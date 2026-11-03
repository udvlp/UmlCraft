//! Entity-relationship diagrams in Chen notation: what a line between two nodes becomes, and the
//! rules a diagram keeps while it is drawn.
//!
//! The palette has one line ([`EdgeTool::ErLine`](crate::palette::EdgeTool::ErLine)); what it
//! becomes follows from its ends ([`joins`]):
//!
//! - an attribute and what it describes (an entity, a relationship or, for a composite
//!   attribute, another attribute): the attribute moves under its owner, and the line shows the
//!   attribute, drawn from the owner;
//! - a relationship (or an associative entity) and an entity: a participation with `[0,n]`;
//! - an IS-A and an entity: a subtyping, the first one to the supertype.
//!
//! Unfinished diagrams are fine (a relationship with one entity, an IS-A without subtypes);
//! what contradicts itself is refused: an attribute with two owners, a second weak entity at an
//! identifying relationship, a second supertype.

use umlcraft_model::{
    DiagramKind, ElementData, ElementKind, ErEntityProps, ErParticipationProps, ErSubtypingProps,
    Id, Model, ModelError, Point, Result, Tx,
};

use crate::palette::NodeTool;

/// Why a line was refused, as [`ModelError::Invalid`] says it; the app words these in the
/// language of its user.
pub const NOT_JOINED: &str = "these two cannot be joined";
pub const ATTRIBUTE_OWNED: &str = "the attribute already belongs to something";
pub const SECOND_WEAK: &str = "an identifying relationship has one weak entity";

/// Whether `diagram` is an ER diagram.
pub fn is_er(model: &Model, diagram: Id) -> bool {
    model.get(diagram).and_then(|d| d.kind().as_diagram()) == Some(DiagramKind::EntityRelationship)
}

/// What a line joins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Join {
    /// The attribute belongs to the other node.
    Attribute,
    /// The relationship (or associative entity) and an entity taking part in it.
    Participation,
    /// The IS-A and one of its entities.
    Subtyping,
}

fn one_way(a: &ElementKind, b: &ElementKind) -> Option<Join> {
    use ElementKind as K;
    let plain_entity = matches!(b, K::ErEntity(ErEntityProps { associative: false, .. }));
    match a {
        K::ErAttribute(_) => matches!(b, K::ErEntity(_) | K::ErRelationship(_) | K::ErAttribute(_))
            .then_some(Join::Attribute),
        K::ErRelationship(_) => matches!(b, K::ErEntity(_)).then_some(Join::Participation),
        // An associative entity is also a relationship: entities take part in it.
        K::ErEntity(ErEntityProps { associative: true, .. }) => {
            plain_entity.then_some(Join::Participation)
        }
        K::ErIsA(_) => plain_entity.then_some(Join::Subtyping),
        _ => None,
    }
}

/// What a line from a node of kind `a` to one of kind `b` becomes, and whether its ends swap so
/// that the attribute (the part, between two attributes), the relationship or the IS-A comes
/// first. `None`: the two cannot be joined.
pub fn joins(a: &ElementKind, b: &ElementKind) -> Option<(Join, bool)> {
    one_way(a, b).map(|j| (j, false)).or_else(|| one_way(b, a).map(|j| (j, true)))
}

/// Whether `kind` is an element of ER diagrams.
pub fn is_er_kind(kind: &ElementKind) -> bool {
    use ElementKind as K;
    matches!(
        kind,
        K::ErEntity(_)
            | K::ErAttribute(_)
            | K::ErRelationship(_)
            | K::ErIsA(_)
            | K::ErParticipation(_)
            | K::ErSubtyping(_)
    )
}

/// Joins the nodes `source` and `target` of `diagram` with a line, as [`joins`] says. Returns
/// the new edge.
pub fn connect(tx: &mut Tx<'_>, diagram: Id, source: Id, target: Id) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let element_of = |p: Id| -> Result<Id> {
        view.get(p).ok_or(ModelError::NotFound(p))?.element.ok_or(ModelError::NotFound(p))
    };
    let (s, t) = (element_of(source)?, element_of(target)?);
    let (sk, tk) = (model.element(s)?.kind(), model.element(t)?.kind());
    let Some((join, swap)) = joins(sk, tk).filter(|_| s != t) else {
        return Err(ModelError::Invalid(NOT_JOINED.into()));
    };
    // The attribute, relationship or IS-A first.
    let ((first, first_node), (second, second_node)) =
        if swap { ((t, target), (s, source)) } else { ((s, source), (t, target)) };
    match join {
        Join::Attribute => {
            let owner = model.element(first)?.owner();
            if owner.is_some_and(|o| model.get(o).is_some_and(|o| is_er_kind(o.kind()))) {
                return Err(ModelError::Invalid(ATTRIBUTE_OWNED.into()));
            }
            // A cycle of composite attributes is an ownership cycle: the model refuses it.
            let index = model.children(second).len();
            tx.move_element(first, second, index)?;
            // Drawn from the owner, as containment lines are.
            tx.add_edge(diagram, Some(first), second_node, first_node, Vec::new())
        }
        Join::Participation => {
            if second_weak(model, first, second) {
                return Err(ModelError::Invalid(SECOND_WEAK.into()));
            }
            let kind = ElementKind::ErParticipation(ErParticipationProps::new(first, second));
            let line = tx.create(first, ElementData::new("", kind))?;
            tx.add_edge(diagram, Some(line), first_node, second_node, Vec::new())
        }
        Join::Subtyping => {
            let supertype = !subtypings(model, first).any(|p| p.supertype);
            let kind = ElementKind::ErSubtyping(ErSubtypingProps {
                is_a: first,
                entity: second,
                supertype,
            });
            let line = tx.create(first, ElementData::new("", kind))?;
            tx.add_edge(diagram, Some(line), first_node, second_node, Vec::new())
        }
    }
}

/// How far above its owner a new attribute goes, and how far apart new attributes stand.
const ATTRIBUTE_RISE: f32 = 60.0;
const ATTRIBUTE_STEP: f32 = 90.0;

/// Gives what node `owner` of `diagram` shows a new attribute named after `base_name`: an
/// ellipse above it, right of the attributes it has, joined by its line. Returns the attribute
/// and its node.
pub fn add_attribute(tx: &mut Tx<'_>, diagram: Id, owner: Id, base_name: &str) -> Result<(Id, Id)> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let node = view.get(owner).ok_or(ModelError::NotFound(owner))?;
    let (element, bounds) = node.element.zip(node.bounds()).ok_or(ModelError::NotFound(owner))?;
    let count = model
        .children(element)
        .iter()
        .filter(|&&c| matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ErAttribute(_))))
        .count();
    // Named apart from the owner's attributes, not from the package it is made in.
    let name = crate::palette::unique_name(model, element, base_name);
    let origin = Point::new(bounds.x + count as f32 * ATTRIBUTE_STEP, bounds.y - ATTRIBUTE_RISE);
    let (attribute, attribute_node) =
        crate::palette::create_node(tx, diagram, NodeTool::ErAttribute, origin, base_name)?;
    connect(tx, diagram, attribute_node, owner)?;
    tx.update(attribute, |data| data.name = name)?;
    Ok((attribute, attribute_node))
}

/// The participations of a relationship (or associative entity).
pub fn participations(
    model: &Model,
    relationship: Id,
) -> impl Iterator<Item = &ErParticipationProps> {
    model.children(relationship).iter().filter_map(move |&c| match model.get(c)?.kind() {
        ElementKind::ErParticipation(p) if p.relationship == relationship => Some(p),
        _ => None,
    })
}

/// The subtypings of an IS-A.
pub fn subtypings(model: &Model, is_a: Id) -> impl Iterator<Item = &ErSubtypingProps> {
    model.children(is_a).iter().filter_map(move |&c| match model.get(c)?.kind() {
        ElementKind::ErSubtyping(p) if p.is_a == is_a => Some(p),
        _ => None,
    })
}

fn is_weak(model: &Model, entity: Id) -> bool {
    matches!(model.get(entity).map(|e| e.kind()), Some(ElementKind::ErEntity(p)) if p.weak)
}

/// Whether joining `entity` to `relationship` would give an identifying relationship a second
/// weak entity.
fn second_weak(model: &Model, relationship: Id, entity: Id) -> bool {
    let identifying = matches!(
        model.get(relationship).map(|e| e.kind()),
        Some(ElementKind::ErRelationship(p)) if p.identifying
    );
    identifying
        && is_weak(model, entity)
        && participations(model, relationship)
            .any(|p| p.entity != entity && is_weak(model, p.entity))
}

#[cfg(test)]
mod tests {
    use umlcraft_model::{
        DiagramKind, Document, ElementData, ElementKind, ErAttributeProps, ErEntityProps,
        ErIsAProps, ErRelationshipProps, Id, Rect, Tx,
    };

    use super::*;

    struct Diagram {
        doc: Document,
        package: Id,
        diagram: Id,
    }

    fn diagram() -> Diagram {
        let mut doc = Document::new(umlcraft_model::Model::new("m"));
        let root = doc.model().root();
        let (mut package, mut diagram) = (root, root);
        doc.edit("setup", |tx| {
            package = tx.create(root, ElementData::new("p", ElementKind::Package))?;
            let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
            diagram = tx.create(package, ElementData::new("d", kind))?;
            Ok(())
        })
        .unwrap();
        Diagram { doc, package, diagram }
    }

    impl Diagram {
        /// A new element of `kind` shown on the diagram: (element, node).
        fn add(&mut self, kind: ElementKind) -> (Id, Id) {
            let (package, diagram) = (self.package, self.diagram);
            let mut ids = (package, package);
            self.doc
                .edit("add", |tx| {
                    let e = tx.create(package, ElementData::new("x", kind))?;
                    let n = tx.add_node(diagram, Some(e), Rect::new(0.0, 0.0, 80.0, 40.0))?;
                    ids = (e, n);
                    Ok(())
                })
                .unwrap();
            ids
        }

        fn connect(&mut self, a: Id, b: Id) -> std::result::Result<Id, ModelError> {
            let diagram = self.diagram;
            let mut edge = None;
            self.doc.edit("connect", |tx: &mut Tx<'_>| {
                edge = Some(connect(tx, diagram, a, b)?);
                Ok(())
            })?;
            Ok(edge.unwrap())
        }
    }

    fn entity(weak: bool) -> ElementKind {
        ElementKind::ErEntity(ErEntityProps { weak, ..Default::default() })
    }

    #[test]
    fn pairs_that_join() {
        use ElementKind as K;
        let attribute = K::ErAttribute(ErAttributeProps::default());
        let relationship = K::ErRelationship(ErRelationshipProps::default());
        let associative = K::ErEntity(ErEntityProps { associative: true, ..Default::default() });
        let is_a = K::ErIsA(ErIsAProps::default());
        let e = entity(false);
        assert_eq!(joins(&attribute, &e), Some((Join::Attribute, false)));
        assert_eq!(joins(&e, &attribute), Some((Join::Attribute, true)));
        assert_eq!(joins(&attribute, &attribute), Some((Join::Attribute, false)), "composite");
        assert_eq!(joins(&relationship, &attribute), Some((Join::Attribute, true)));
        assert_eq!(joins(&e, &relationship), Some((Join::Participation, true)));
        assert_eq!(joins(&relationship, &associative), Some((Join::Participation, false)));
        assert_eq!(joins(&associative, &e), Some((Join::Participation, false)));
        assert_eq!(joins(&is_a, &e), Some((Join::Subtyping, false)));
        for (a, b) in [
            (&e, &e),
            (&relationship, &relationship),
            (&attribute, &is_a),
            (&is_a, &associative),
            (&is_a, &relationship),
            (&associative, &associative),
        ] {
            assert_eq!(joins(a, b), None, "{a:?} – {b:?}");
        }
    }

    #[test]
    fn an_attribute_moves_under_its_owner_once() {
        let mut d = diagram();
        let (customer, customer_node) = d.add(entity(false));
        let (name, name_node) = d.add(ElementKind::ErAttribute(ErAttributeProps::default()));
        let (other, other_node) = d.add(entity(false));
        let edge = d.connect(name_node, customer_node).unwrap();
        let model = d.doc.model();
        assert_eq!(model.get(name).unwrap().owner(), Some(customer));
        let presentation = model.view(d.diagram).unwrap().get(edge).unwrap();
        assert_eq!(presentation.element, Some(name));
        assert_eq!(presentation.endpoints(), Some((customer_node, name_node)), "from the owner");
        let err = d.connect(name_node, other_node).unwrap_err();
        assert_eq!(err, ModelError::Invalid(ATTRIBUTE_OWNED.into()));
        assert_eq!(d.doc.model().get(name).unwrap().owner(), Some(customer));
        // Deleting the entity takes its attributes.
        d.doc.edit("delete", |tx| tx.delete(customer)).unwrap();
        assert!(d.doc.model().get(name).is_none() && d.doc.model().get(other).is_some());
    }

    #[test]
    fn added_attributes_stand_in_a_row_above_their_owner() {
        let mut d = diagram();
        let (customer, customer_node) = d.add(entity(false));
        let diagram = d.diagram;
        let mut added = Vec::new();
        for _ in 0..2 {
            d.doc
                .edit("add", |tx| {
                    added.push(add_attribute(tx, diagram, customer_node, "Attribute")?);
                    Ok(())
                })
                .unwrap();
        }
        let model = d.doc.model();
        let view = model.view(diagram).unwrap();
        let rects: Vec<_> =
            added.iter().map(|&(_, n)| view.get(n).unwrap().bounds().unwrap()).collect();
        for (&(attribute, _), rect) in added.iter().zip(&rects) {
            assert_eq!(model.get(attribute).unwrap().owner(), Some(customer));
            assert!(rect.y < 0.0, "above the entity");
        }
        assert!(rects[1].x > rects[0].x + rects[0].width, "side by side");
        let names: Vec<_> = added.iter().map(|&(a, _)| model.get(a).unwrap().name()).collect();
        assert_ne!(names[0], names[1]);
        assert_eq!(view.edges_attached_to(customer_node).count(), 2);
    }

    #[test]
    fn participations_and_the_one_weak_entity() {
        let mut d = diagram();
        let identifying = ElementKind::ErRelationship(ErRelationshipProps { identifying: true });
        let (rel, rel_node) = d.add(identifying);
        let (_, owner_node) = d.add(entity(false));
        let (_, weak_node) = d.add(entity(true));
        let (_, weak2_node) = d.add(entity(true));
        d.connect(owner_node, rel_node).unwrap();
        d.connect(rel_node, weak_node).unwrap();
        let err = d.connect(weak2_node, rel_node).unwrap_err();
        assert_eq!(err, ModelError::Invalid(SECOND_WEAK.into()));
        let model = d.doc.model();
        let parts: Vec<_> = participations(model, rel).collect();
        assert_eq!(parts.len(), 2);
        assert!(
            parts.iter().all(|p| p.cardinality == Some(umlcraft_model::Cardinality::ZERO_MANY))
        );
        // An entity may take part twice (a role each).
        d.connect(owner_node, rel_node).unwrap();
        assert_eq!(participations(d.doc.model(), rel).count(), 3);
    }

    #[test]
    fn every_chen_shape_draws() {
        use crate::scene::{ApproxMeasure, Figure, Preview, build};
        let mut d = diagram();
        let weak = d.add(entity(true));
        let key =
            ErAttributeProps { key: Some(umlcraft_model::ErKey::Partial), ..Default::default() };
        let (_, key_node) = d.add(ElementKind::ErAttribute(key));
        let derived = ErAttributeProps { derived: true, multivalued: true, ..Default::default() };
        let (_, derived_node) = d.add(ElementKind::ErAttribute(derived));
        let rel = d.add(ElementKind::ErRelationship(ErRelationshipProps { identifying: true }));
        let is_a = d.add(ElementKind::ErIsA(ErIsAProps { total: true, disjoint: Some(true) }));
        let (_, super_node) = d.add(entity(false));
        d.connect(key_node, weak.1).unwrap();
        d.connect(derived_node, weak.1).unwrap();
        let part = d.connect(rel.1, weak.1).unwrap();
        let supertype = d.connect(is_a.1, super_node).unwrap();
        let (doc, diagram) = (&mut d.doc, d.diagram);
        let line = doc.model().view(diagram).unwrap().get(part).unwrap().element.unwrap();
        doc.edit("total", |tx| {
            tx.update(is_a.0, |data| data.name.clear())?;
            tx.update(line, |data| {
                if let ElementKind::ErParticipation(p) = &mut data.kind {
                    p.total = true;
                    p.role = "owner".into();
                }
            })
        })
        .unwrap();
        let scene = build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        let figure = |node: Id| &scene.node(node).unwrap().figure;
        assert!(matches!(figure(weak.1), Figure::ErEntity { weak: true, associative: false }));
        assert!(matches!(
            figure(key_node),
            Figure::ErAttribute { dotted: true, underlines, .. } if underlines.len() == 1
        ));
        assert!(matches!(
            figure(derived_node),
            Figure::ErAttribute { derived: true, multivalued: true, underlines, .. } if underlines.is_empty()
        ));
        assert!(matches!(figure(rel.1), Figure::ErRelationship { identifying: true }));
        assert!(matches!(figure(is_a.1), Figure::ErIsA));
        let is_a_rows: Vec<&str> =
            scene.node(is_a.1).unwrap().rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(is_a_rows, [crate::scene::IS_A, "d"]);
        let edge = |id: Id| scene.edges.iter().find(|e| e.id == id).unwrap();
        let part = edge(part);
        assert!(part.double && part.association);
        let texts: Vec<&str> = part.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["owner", "[0,n]"], "role and cardinality at the entity");
        assert!(edge(supertype).double, "a total IS-A's supertype line");
        // Attribute lines are plain and carry no name.
        assert!(scene.edges.iter().all(|e| e.line == crate::scene::LineStyle::Solid));
        assert_eq!(scene.edges.iter().filter(|e| e.labels.is_empty()).count(), 3);
        // Every figure draws, in the export too.
        for node in &scene.nodes {
            assert!(!crate::draw::figure(node).is_empty());
        }
        let svg = crate::svg::render(&scene, &ApproxMeasure);
        assert!(svg.contains("owner") && svg.contains("[0,n]"));
    }

    #[test]
    fn the_first_subtyping_goes_to_the_supertype() {
        let mut d = diagram();
        let (is_a, is_a_node) = d.add(ElementKind::ErIsA(ErIsAProps::default()));
        let (person, person_node) = d.add(entity(false));
        let (_, pilot_node) = d.add(entity(false));
        d.connect(person_node, is_a_node).unwrap();
        d.connect(is_a_node, pilot_node).unwrap();
        let model = d.doc.model();
        let supertypes: Vec<_> =
            subtypings(model, is_a).filter(|p| p.supertype).map(|p| p.entity).collect();
        assert_eq!(supertypes, [person]);
        assert_eq!(subtypings(model, is_a).count(), 2);
        assert_eq!(
            d.connect(person_node, person_node),
            Err(ModelError::Invalid(NOT_JOINED.into()))
        );
    }
}
