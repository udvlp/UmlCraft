//! Entity-relationship diagrams: what a line between two nodes becomes, the rules a diagram
//! keeps while it is drawn, and switching between the notations.
//!
//! In IE and IDEF1X ([`relate`]) the lines are relationships drawn from the parent to the
//! child: identifying, non-identifying, many to many, and subtypes, which join an IS-A drawn as
//! the subtype symbol. The parent's key goes into the child as foreign key columns, attributes
//! that know their relationship. Tables list the attributes, so these have no nodes.
//! [`set_notation`] converts a diagram from one way of drawing to the other.
//!
//! In Chen notation the palette has one line ([`EdgeTool::ErLine`](crate::palette::EdgeTool::ErLine)); what it
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
    Cardinality, CardinalityMax, DiagramKind, ElementData, ElementKind, ErAttributeProps,
    ErEntityProps, ErIsAProps, ErKey, ErNotation, ErParticipationProps, ErRelationshipProps,
    ErSubtypingProps, Id, Model, ModelError, Point, Rect, Result, Tx,
};

use crate::palette::{EdgeTool, NodeTool};

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

/// The notation `diagram` is drawn in.
pub fn notation(model: &Model, diagram: Id) -> ErNotation {
    match model.get(diagram).map(|d| d.kind()) {
        Some(ElementKind::Diagram(p)) => p.notation,
        _ => ErNotation::Chen,
    }
}

/// A line of IE and IDEF1X, drawn from the parent (the supertype) to the child (the subtype).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IeLine {
    Identifying,
    NonIdentifying,
    ManyToMany,
    Subtype,
}

impl IeLine {
    /// The line `tool` draws, if it is one of IE's.
    pub fn of(tool: EdgeTool) -> Option<Self> {
        match tool {
            EdgeTool::ErIdentifying => Some(Self::Identifying),
            EdgeTool::ErNonIdentifying => Some(Self::NonIdentifying),
            EdgeTool::ErManyToMany => Some(Self::ManyToMany),
            EdgeTool::ErSubtype => Some(Self::Subtype),
            _ => None,
        }
    }
}

/// Joins the entities shown by `parent` and `child` on `diagram` with `line`, as IE and IDEF1X
/// draw it, and returns the new edge:
///
/// - a relationship with both participations (the parent's `[1,1]` when identifying, else
///   `[0,1]`; the child's `[0,n]`; many-to-many `[0,n]` at both), drawn as one line; the child
///   gets the parent's primary key as foreign key columns, in its own key (and becomes weak)
///   when identifying;
/// - a subtype: a subtyping of the IS-A below the supertype (made with its line when there is
///   none on the diagram), the subtype keyed by the supertype's key.
pub fn relate(tx: &mut Tx<'_>, diagram: Id, line: IeLine, parent: Id, child: Id) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let element_of = |p: Id| -> Result<Id> {
        view.get(p).ok_or(ModelError::NotFound(p))?.element.ok_or(ModelError::NotFound(p))
    };
    let (p, c) = (element_of(parent)?, element_of(child)?);
    let entities = [p, c]
        .iter()
        .all(|&e| matches!(model.get(e).map(|e| e.kind()), Some(ElementKind::ErEntity(_))));
    let looped = p == c;
    if !entities || (looped && matches!(line, IeLine::Identifying | IeLine::Subtype)) {
        return Err(ModelError::Invalid(NOT_JOINED.into()));
    }
    let owner = model.element(p)?.owner().ok_or(ModelError::NotFound(p))?;
    if line == IeLine::Subtype {
        return subtype(tx, diagram, (p, parent), (c, child), owner);
    }
    let identifying = line == IeLine::Identifying;
    let kind = ElementKind::ErRelationship(ErRelationshipProps { identifying });
    let relationship = tx.create(owner, ElementData::new("", kind))?;
    let (parent_side, child_side) = match line {
        IeLine::ManyToMany => (Cardinality::ZERO_MANY, Cardinality::ZERO_MANY),
        IeLine::Identifying => (Cardinality::ONE_ONE, Cardinality::ZERO_MANY),
        _ => (Cardinality::ZERO_ONE, Cardinality::ZERO_MANY),
    };
    for (entity, cardinality) in [(p, parent_side), (c, child_side)] {
        let mut props = ErParticipationProps::new(relationship, entity);
        props.cardinality = Some(cardinality);
        tx.create(relationship, ElementData::new("", ElementKind::ErParticipation(props)))?;
    }
    if line != IeLine::ManyToMany {
        copy_key(tx, p, c, relationship, identifying)?;
    }
    if identifying {
        set_weak(tx, c, true)?;
    }
    tx.add_edge(diagram, Some(relationship), parent, child, Vec::new())
}

/// How far below its supertype a new IS-A goes.
const SUBTYPE_DROP: f32 = 30.0;

fn subtype(
    tx: &mut Tx<'_>,
    diagram: Id,
    (supertype, super_node): (Id, Id),
    (sub, sub_node): (Id, Id),
    owner: Id,
) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    // The IS-A of the supertype that this diagram shows.
    let shown = model.elements().find_map(|e| {
        let ElementKind::ErIsA(_) = e.kind() else { return None };
        subtypings(model, e.id()).any(|s| s.supertype && s.entity == supertype).then_some(())?;
        view.presentations_of(e.id()).find(|p| p.bounds().is_some()).map(|p| (e.id(), p.id))
    });
    let (is_a, is_a_node) = match shown {
        Some(found) => found,
        None => {
            let below = view
                .get(super_node)
                .and_then(|n| n.bounds())
                .ok_or(ModelError::NotFound(super_node))?;
            let is_a =
                tx.create(owner, ElementData::new("", ElementKind::ErIsA(ErIsAProps::default())))?;
            let size = crate::palette::default_size_for(tx.model().element(is_a)?.kind());
            let at = Rect::new(
                below.center().x - size.width / 2.0,
                below.bottom() + SUBTYPE_DROP,
                size.width,
                size.height,
            );
            let node = tx.add_node(diagram, Some(is_a), at)?;
            let kind = ElementKind::ErSubtyping(ErSubtypingProps {
                is_a,
                entity: supertype,
                supertype: true,
            });
            let line = tx.create(is_a, ElementData::new("", kind))?;
            tx.add_edge(diagram, Some(line), node, super_node, Vec::new())?;
            (is_a, node)
        }
    };
    if subtypings(tx.model(), is_a).any(|s| s.entity == sub) {
        return Err(ModelError::Invalid(NOT_JOINED.into()));
    }
    let kind = ElementKind::ErSubtyping(ErSubtypingProps { is_a, entity: sub, supertype: false });
    let line = tx.create(is_a, ElementData::new("", kind))?;
    copy_key(tx, supertype, sub, line, true)?;
    tx.add_edge(diagram, Some(line), is_a_node, sub_node, Vec::new())
}

/// `base`, or `base2`, `base3` … if one of `owner`'s children is called so.
fn free_name(model: &Model, owner: Id, base: &str) -> String {
    let taken = |name: &str| !model.children_named(owner, name).is_empty();
    if !taken(base) {
        return base.to_owned();
    }
    (2..).map(|n| format!("{base}{n}")).find(|name| !taken(name)).expect("names are unbounded")
}

/// Copies the primary key of `parent` into `child` as foreign key columns of `by` (a
/// relationship or subtyping), in the child's key when `into_key`.
fn copy_key(tx: &mut Tx<'_>, parent: Id, child: Id, by: Id, into_key: bool) -> Result<()> {
    let keys: Vec<(String, ErAttributeProps)> = attributes(tx.model(), parent)
        .filter_map(|a| match a.kind() {
            ElementKind::ErAttribute(p) if p.key.is_some() => {
                Some((a.name().to_owned(), p.clone()))
            }
            _ => None,
        })
        .collect();
    for (name, key) in keys {
        let props = ErAttributeProps {
            key: into_key.then_some(ErKey::Primary),
            physical_name: key.physical_name,
            datatype: key.datatype,
            length_precision: key.length_precision,
            not_null: true,
            foreign_key: Some(by),
            ..Default::default()
        };
        let name = free_name(tx.model(), child, &name);
        tx.create(child, ElementData::new(name, ElementKind::ErAttribute(props)))?;
    }
    Ok(())
}

fn set_weak(tx: &mut Tx<'_>, entity: Id, weak: bool) -> Result<()> {
    tx.update(entity, |data| {
        if let ElementKind::ErEntity(p) = &mut data.kind {
            p.weak = weak;
        }
    })
}

/// The attributes `owner` has itself (not the parts of composite ones).
pub fn attributes(model: &Model, owner: Id) -> impl Iterator<Item = &umlcraft_model::Element> {
    model
        .children(owner)
        .iter()
        .filter_map(move |&c| model.get(c))
        .filter(|c| matches!(c.kind(), ElementKind::ErAttribute(_)))
}

/// Makes `relationship` identifying or not: its foreign key columns move into the child's key
/// or out of it, and the child becomes weak or no longer is.
pub fn set_identifying(tx: &mut Tx<'_>, relationship: Id, identifying: bool) -> Result<()> {
    tx.update(relationship, |data| {
        if let ElementKind::ErRelationship(p) = &mut data.kind {
            p.identifying = identifying;
        }
    })?;
    let model = tx.model();
    let columns: Vec<(Id, Option<Id>)> = model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::ErAttribute(p) if p.foreign_key == Some(relationship)))
        .map(|e| (e.id(), e.owner()))
        .collect();
    for &(column, _) in &columns {
        tx.update(column, |data| {
            if let ElementKind::ErAttribute(p) = &mut data.kind {
                p.key = identifying.then_some(ErKey::Primary);
            }
        })?;
    }
    let mut children: Vec<Id> = columns.iter().filter_map(|c| c.1).collect();
    children.dedup();
    for child in children {
        set_weak(tx, child, identifying)?;
    }
    Ok(())
}

/// Gives `owner` (an entity, a relationship or a composite attribute) a new attribute named
/// after `base_name`, without a node: a row of its box in IE and IDEF1X; a primary key column
/// when `key`. Returns the attribute.
pub fn add_column(tx: &mut Tx<'_>, owner: Id, base_name: &str, key: bool) -> Result<Id> {
    let name = crate::palette::unique_name(tx.model(), owner, base_name);
    let props = ErAttributeProps { key: key.then_some(ErKey::Primary), ..Default::default() };
    tx.create(owner, ElementData::new(name, ElementKind::ErAttribute(props)))
}

/// A column typed into a table's row, `name` or `name : TYPE(length)`: its name and properties,
/// a primary key with `key`.
pub fn parse_column(text: &str, key: bool) -> (String, ErAttributeProps) {
    let (name, datatype) = text.split_once(':').unwrap_or((text, ""));
    let datatype = datatype.trim();
    let (datatype, length) = match datatype.split_once('(') {
        Some((t, rest)) if rest.ends_with(')') => (t.trim(), rest[..rest.len() - 1].trim()),
        _ => (datatype, ""),
    };
    let props = ErAttributeProps {
        key: key.then_some(ErKey::Primary),
        datatype: datatype.to_owned(),
        length_precision: length.to_owned(),
        not_null: key,
        ..Default::default()
    };
    (name.trim().to_owned(), props)
}

/// The participation of a binary relationship that is its parent: the one at most one of
/// facing one of more, else the first. `None` unless it has two participations.
pub fn parent_and_child(model: &Model, relationship: Id) -> Option<[ErParticipationProps; 2]> {
    let parts: Vec<&ErParticipationProps> = participations(model, relationship).collect();
    let [a, b] = parts[..] else { return None };
    let one =
        |p: &ErParticipationProps| p.cardinality.is_some_and(|c| c.max == CardinalityMax::Count(1));
    Some(if one(b) && !one(a) { [b.clone(), a.clone()] } else { [a.clone(), b.clone()] })
}

/// Whether `relationship` is drawn as a line in IE and IDEF1X: it joins two entities and has no
/// attributes of its own. Others (n-ary, with attributes) are boxes with their lines.
pub fn is_line(model: &Model, relationship: Id) -> bool {
    matches!(model.get(relationship).map(|e| e.kind()), Some(ElementKind::ErRelationship(_)))
        && parent_and_child(model, relationship).is_some()
        && attributes(model, relationship).next().is_none()
}

/// Draws `diagram` in `notation`, keeping where its entities, relationships and IS-As are.
/// Chen → IE or IDEF1X: the attributes' nodes and lines go (the boxes list them), and each
/// relationship drawn as a line ([`is_line`]) loses its node and participation lines for one line
/// from the parent to the child. Back to Chen: those lines become a relationship node between
/// the two entities with its participation lines, and the attributes of what is shown (but
/// foreign key columns) get their nodes and lines around their owners.
pub fn set_notation(
    tx: &mut Tx<'_>,
    diagram: Id,
    notation: ErNotation,
    measure: &dyn crate::scene::TextMeasure,
) -> Result<()> {
    let before = self::notation(tx.model(), diagram);
    tx.update(diagram, |data| {
        if let ElementKind::Diagram(p) = &mut data.kind {
            p.notation = notation;
        }
    })?;
    if before.lists_attributes() == notation.lists_attributes() {
        return Ok(());
    }
    if notation.lists_attributes() {
        to_listing(tx, diagram)
    } else {
        to_chen(tx, diagram, measure)
    }
}

fn to_listing(tx: &mut Tx<'_>, diagram: Id) -> Result<()> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let kind_of =
        |p: &umlcraft_model::Presentation| p.element.and_then(|e| model.get(e)).map(|e| e.kind());
    let attributes: indexmap::IndexSet<Id> = view
        .presentations()
        .iter()
        .filter(|p| p.bounds().is_some() && matches!(kind_of(p), Some(ElementKind::ErAttribute(_))))
        .map(|p| p.id)
        .collect();
    // Relationship nodes that become lines: (node, relationship, parent node, child node).
    let node_of =
        |entity: Id| view.presentations_of(entity).find(|p| p.bounds().is_some()).map(|p| p.id);
    let mut lines = Vec::new();
    for p in view.presentations().iter().filter(|p| p.bounds().is_some()) {
        let Some(relationship) = p.element.filter(|&e| is_line(model, e)) else { continue };
        let Some([parent, child]) = parent_and_child(model, relationship) else { continue };
        if let (Some(a), Some(b)) = (node_of(parent.entity), node_of(child.entity)) {
            lines.push((p.id, relationship, a, b));
        }
    }
    let mut removed = attributes;
    removed.extend(lines.iter().map(|l| l.0));
    if !removed.is_empty() {
        tx.remove_presentations(diagram, removed)?;
    }
    for (_, relationship, parent, child) in lines {
        tx.add_edge(diagram, Some(relationship), parent, child, Vec::new())?;
    }
    Ok(())
}

fn to_chen(tx: &mut Tx<'_>, diagram: Id, measure: &dyn crate::scene::TextMeasure) -> Result<()> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    // Relationship lines: (edge, relationship, parent node, child node).
    let lines: Vec<(Id, Id, Id, Id)> = view
        .presentations()
        .iter()
        .filter_map(|p| {
            let (s, t) = p.endpoints()?;
            let e = p.element?;
            matches!(model.get(e)?.kind(), ElementKind::ErRelationship(_))
                .then_some((p.id, e, s, t))
        })
        .collect();
    for &(edge, relationship, s, t) in &lines {
        let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let (a, b) = (view.get(s).and_then(|p| p.bounds()), view.get(t).and_then(|p| p.bounds()));
        let (Some(a), Some(b)) = (a, b) else { continue };
        tx.remove_presentation(diagram, edge)?;
        let size = crate::palette::default_size_for(tx.model().element(relationship)?.kind());
        let (ca, cb) = (a.center(), b.center());
        let at = Point::new((ca.x + cb.x - size.width) / 2.0, (ca.y + cb.y - size.height) / 2.0);
        let node = tx.add_node(
            diagram,
            Some(relationship),
            Rect::new(at.x, at.y, size.width, size.height),
        )?;
        let parts: Vec<(Id, Id)> = tx
            .model()
            .children(relationship)
            .iter()
            .filter_map(|&c| match tx.model().get(c)?.kind() {
                ElementKind::ErParticipation(p) => Some((c, p.entity)),
                _ => None,
            })
            .collect();
        let mut used = Vec::new();
        for (part, entity) in parts {
            let end = [s, t].into_iter().find(|&n| {
                !used.contains(&n)
                    && tx.model().view(diagram).and_then(|v| v.get(n)).and_then(|p| p.element)
                        == Some(entity)
            });
            if let Some(end) = end {
                used.push(end);
                tx.add_edge(diagram, Some(part), node, end, Vec::new())?;
            }
        }
    }
    // Attribute nodes around what shows them.
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let mut owners: Vec<(Id, Id)> = Vec::new();
    for p in view.presentations().iter().filter(|p| p.bounds().is_some()) {
        let Some(e) = p.element.and_then(|e| model.get(e)) else { continue };
        if matches!(e.kind(), ElementKind::ErEntity(_) | ElementKind::ErRelationship(_)) {
            owners.push((e.id(), p.id));
        }
    }
    let mut stack = owners;
    while let Some((owner, owner_node)) = stack.pop() {
        let columns: Vec<Id> = attributes(tx.model(), owner)
            .filter(|a| matches!(a.kind(), ElementKind::ErAttribute(p) if p.foreign_key.is_none()))
            .map(|a| a.id())
            .collect();
        let at = tx.model().view(diagram).and_then(|v| v.get(owner_node)).and_then(|p| p.bounds());
        let Some(at) = at else { continue };
        for column in columns {
            if tx.model().view(diagram).is_some_and(|v| v.presentations_of(column).next().is_some())
            {
                continue;
            }
            let size = crate::palette::default_size_for(tx.model().element(column)?.kind());
            let node =
                tx.add_node(diagram, Some(column), Rect::new(at.x, at.y, size.width, size.height))?;
            tx.add_edge(diagram, Some(column), owner_node, node, Vec::new())?;
            stack.push((column, node));
        }
    }
    crate::er_layout::arrange_attributes(tx, diagram, measure)
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

/// The participation that end `end` (0 at the source, 1 at the target) of the IE or IDEF1X line
/// `edge` on `diagram` stands for, the line being a relationship's. A loop's ends are its two
/// participations in order.
pub fn line_participation(model: &Model, diagram: Id, edge: Id, end: usize) -> Option<Id> {
    let view = model.view(diagram)?;
    let line = view.get(edge)?;
    let relationship = line.element?;
    let (source, target) = line.endpoints()?;
    let entity = |node: Id| view.get(node).and_then(|n| n.element);
    let parts: Vec<(Id, Id)> = model
        .children(relationship)
        .iter()
        .filter_map(|&c| match model.get(c)?.kind() {
            ElementKind::ErParticipation(p) if p.relationship == relationship => {
                Some((c, p.entity))
            }
            _ => None,
        })
        .collect();
    let first = parts.iter().position(|p| Some(p.1) == entity(source))?;
    if end == 0 {
        return Some(parts[first].0);
    }
    (0..parts.len()).find(|&i| i != first && Some(parts[i].1) == entity(target)).map(|i| parts[i].0)
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
        let is_a = d.add(ElementKind::ErIsA(ErIsAProps {
            total: true,
            disjoint: Some(true),
            ..Default::default()
        }));
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

    impl Diagram {
        fn set_notation(&mut self, notation: ErNotation) {
            let diagram = self.diagram;
            let measure = crate::scene::ApproxMeasure;
            self.doc.edit("notation", |tx| set_notation(tx, diagram, notation, &measure)).unwrap();
        }

        fn relate(&mut self, line: IeLine, a: Id, b: Id) -> std::result::Result<Id, ModelError> {
            let diagram = self.diagram;
            let mut edge = None;
            self.doc.edit("relate", |tx: &mut Tx<'_>| {
                edge = Some(relate(tx, diagram, line, a, b)?);
                Ok(())
            })?;
            Ok(edge.unwrap())
        }

        fn key(&mut self, owner: Id, name: &str) -> Id {
            let mut id = owner;
            self.doc
                .edit("key", |tx| {
                    id = add_column(tx, owner, "k", true)?;
                    tx.rename(id, name)
                })
                .unwrap();
            id
        }

        fn scene(&self) -> crate::scene::Scene {
            use crate::scene::{ApproxMeasure, Preview, build};
            build(self.doc.model(), self.diagram, &Preview::default(), &ApproxMeasure)
        }
    }

    fn columns(model: &Model, entity: Id) -> Vec<(&str, ErAttributeProps)> {
        attributes(model, entity)
            .map(|a| match a.kind() {
                ElementKind::ErAttribute(p) => (a.name(), p.clone()),
                _ => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn ie_relationships_copy_the_parents_key() {
        use crate::scene::{EndMarker, Figure, RowRole};
        let mut d = diagram();
        d.set_notation(ErNotation::Ie);
        let (order, order_node) = d.add(entity(false));
        let (line, line_node) = d.add(entity(false));
        let (customer, customer_node) = d.add(entity(false));
        let (tag, tag_node) = d.add(entity(false));
        d.key(order, "order no");
        d.key(customer, "customer id");
        d.key(line, "line no");
        let has = d.relate(IeLine::Identifying, order_node, line_node).unwrap();
        let places = d.relate(IeLine::NonIdentifying, customer_node, order_node).unwrap();
        let tagged = d.relate(IeLine::ManyToMany, line_node, tag_node).unwrap();
        let model = d.doc.model();
        let view = model.view(d.diagram).unwrap();
        let relationship = view.get(has).unwrap().element.unwrap();
        assert!(
            matches!(model.get(relationship).unwrap().kind(), ElementKind::ErRelationship(p) if p.identifying)
        );
        let [parent, child] = parent_and_child(model, relationship).unwrap();
        assert_eq!((parent.entity, child.entity), (order, line));
        assert_eq!(parent.cardinality, Some(Cardinality::ONE_ONE));
        assert_eq!(child.cardinality, Some(Cardinality::ZERO_MANY));
        // The child is keyed by its parent: weak, the copied column in its key.
        assert!(is_weak(model, line));
        let line_columns = columns(model, line);
        let copied = line_columns.iter().find(|(n, _)| *n == "order no").unwrap();
        assert_eq!(copied.1.key, Some(ErKey::Primary));
        assert_eq!(copied.1.foreign_key, Some(relationship));
        // A non-identifying one copies it outside the key; many-to-many copies nothing.
        let order_columns = columns(model, order);
        let fk = order_columns.iter().find(|(n, _)| *n == "customer id").unwrap();
        assert!(fk.1.key.is_none() && fk.1.foreign_key.is_some());
        assert_eq!(columns(model, tag).len(), 0);
        assert!(!is_weak(model, order));
        // Drawn: boxes listing their columns, the child rounded, crow's feet at the ends.
        let scene = d.scene();
        let Figure::ErTable { rounded, compartments, .. } = &scene.node(line_node).unwrap().figure
        else {
            panic!("a box")
        };
        assert!(*rounded && compartments.len() == 2);
        assert!(matches!(
            &scene.node(order_node).unwrap().figure,
            Figure::ErTable { rounded: false, .. }
        ));
        let texts: Vec<&str> =
            scene.node(order_node).unwrap().rows.iter().map(|r| r.text.as_str()).collect();
        assert!(texts.contains(&"customer id") && texts.contains(&"(FK)"), "{texts:?}");
        let name = scene.node(order_node).unwrap().rows.iter().find(|r| r.role == RowRole::Name);
        assert!(name.unwrap().rect.bottom() < scene.node(order_node).unwrap().rect.y, "above");
        let edge = |id| scene.edge(id).unwrap();
        assert_eq!(
            (edge(has).source_marker, edge(has).target_marker),
            (EndMarker::ErOne, EndMarker::ErZeroMany)
        );
        assert_eq!(edge(places).source_marker, EndMarker::ErZeroOne);
        assert_eq!(edge(tagged).source_marker, EndMarker::ErZeroMany);
        // Identifying no more: the column leaves the key, the child is no longer weak.
        d.doc.edit("plain", |tx| set_identifying(tx, relationship, false)).unwrap();
        let model = d.doc.model();
        assert!(!is_weak(model, line));
        assert!(columns(model, line).iter().all(|(n, p)| *n != "order no" || p.key.is_none()));
        // Two subtypes share the IS-A below their supertype.
        let (person, person_node) = d.add(entity(false));
        let (_, pilot_node) = d.add(entity(false));
        let (student, student_node) = d.add(entity(false));
        d.key(person, "person id");
        d.relate(IeLine::Subtype, person_node, pilot_node).unwrap();
        d.relate(IeLine::Subtype, person_node, student_node).unwrap();
        let model = d.doc.model();
        let is_a: Vec<Id> = model
            .elements()
            .filter(|e| matches!(e.kind(), ElementKind::ErIsA(_)))
            .map(|e| e.id())
            .collect();
        assert_eq!(is_a.len(), 1);
        assert_eq!(subtypings(model, is_a[0]).count(), 3);
        let student_key = &columns(model, student)[0];
        assert_eq!((student_key.0, student_key.1.key), ("person id", Some(ErKey::Primary)));
        let scene = d.scene();
        let symbol = scene.nodes.iter().find(|n| n.element == Some(is_a[0])).unwrap();
        assert!(matches!(symbol.figure, Figure::ErSubtype { complete: false, idef1x: false }));
        assert!(matches!(
            &scene.node(student_node).unwrap().figure,
            Figure::ErTable { rounded: true, .. }
        ));
        // Refused: a subtype of itself, lines to what is not an entity.
        assert_eq!(
            d.relate(IeLine::Identifying, person_node, person_node),
            Err(ModelError::Invalid(NOT_JOINED.into()))
        );
    }

    #[test]
    fn switching_notation_keeps_the_entities_where_they_are() {
        let mut d = diagram();
        let (customer, customer_node) = d.add(entity(false));
        let (ticket, ticket_node) = d.add(entity(false));
        let (books, books_node) =
            d.add(ElementKind::ErRelationship(ErRelationshipProps::default()));
        let (_, name_node) = d.add(ElementKind::ErAttribute(ErAttributeProps::default()));
        d.connect(name_node, customer_node).unwrap();
        d.connect(books_node, customer_node).unwrap();
        d.connect(books_node, ticket_node).unwrap();
        let diagram = d.diagram;
        // Positions apart, as a user would have them.
        d.doc
            .edit("place", |tx| {
                tx.set_bounds(diagram, customer_node, Rect::new(0.0, 0.0, 100.0, 40.0))?;
                tx.set_bounds(diagram, ticket_node, Rect::new(300.0, 0.0, 100.0, 40.0))?;
                tx.set_bounds(diagram, books_node, Rect::new(150.0, 0.0, 80.0, 50.0))
            })
            .unwrap();
        let count = d.doc.model().elements().count();
        let at = |d: &Diagram, n: Id| d.doc.model().view(diagram).unwrap().get(n).unwrap().bounds();
        d.set_notation(ErNotation::Ie);
        let model = d.doc.model();
        let view = model.view(diagram).unwrap();
        assert!(view.get(name_node).is_none() && view.get(books_node).is_none());
        let line: Vec<_> = view.presentations_of(books).collect();
        assert_eq!(line.len(), 1);
        assert!(line[0].endpoints().is_some(), "a line");
        assert_eq!(at(&d, customer_node), Some(Rect::new(0.0, 0.0, 100.0, 40.0)));
        assert_eq!(d.doc.model().elements().count(), count, "the model stays");
        let scene = d.scene();
        let rows: Vec<&str> =
            scene.node(customer_node).unwrap().rows.iter().map(|r| r.text.as_str()).collect();
        assert!(rows.contains(&"x"), "the attribute is a row: {rows:?}");
        d.set_notation(ErNotation::Chen);
        let model = d.doc.model();
        let view = model.view(diagram).unwrap();
        let nodes: Vec<_> = view.presentations_of(books).filter(|p| p.bounds().is_some()).collect();
        assert_eq!(nodes.len(), 1, "a rhombus again");
        assert_eq!(view.edges_attached_to(nodes[0].id).count(), 2);
        let attribute =
            model.children(customer).iter().copied().find(|&c| {
                matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ErAttribute(_)))
            });
        assert_eq!(view.presentations_of(attribute.unwrap()).count(), 2, "node and line");
        assert_eq!(at(&d, ticket_node), Some(Rect::new(300.0, 0.0, 100.0, 40.0)));
        assert_eq!(d.doc.model().elements().count(), count);
        let _ = ticket;
    }

    #[test]
    fn ends_in_ie_and_idef1x() {
        use crate::scene::{EndMarker as M, er_end};
        let c = |s: &str| Some(s.parse::<Cardinality>().unwrap());
        assert_eq!(er_end(false, c("[1,1]"), c("[0,n]"), true), (M::ErOne, None));
        assert_eq!(er_end(false, c("[0,1]"), c("[0,n]"), true), (M::ErZeroOne, None));
        assert_eq!(er_end(false, c("[1,n]"), c("[1,1]"), false), (M::ErOneMany, None));
        assert_eq!(
            er_end(false, c("[2,5]"), c("[1,1]"), false),
            (M::ErOneMany, Some("[2,5]".into()))
        );
        // IDEF1X: nothing or a diamond at the parent, a dot with a letter at the child.
        assert_eq!(er_end(true, c("[1,1]"), c("[0,n]"), true), (M::None, None));
        assert_eq!(er_end(true, c("[0,1]"), c("[0,n]"), true), (M::ErOptional, None));
        assert_eq!(er_end(true, c("[0,n]"), c("[1,1]"), false), (M::ErDot, None));
        assert_eq!(er_end(true, c("[1,n]"), c("[1,1]"), false), (M::ErDot, Some("P".into())));
        assert_eq!(er_end(true, c("[0,1]"), c("[1,1]"), false), (M::ErDot, Some("Z".into())));
        assert_eq!(er_end(true, c("[3,3]"), c("[1,1]"), false), (M::ErDot, Some("3".into())));
        assert_eq!(er_end(true, c("[0,n]"), c("[0,n]"), true), (M::ErDot, None), "many-to-many");
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
