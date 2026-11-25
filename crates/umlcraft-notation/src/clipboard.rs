//! Copy and paste of diagram content.
//!
//! A [`Clip`] is a self-contained snapshot: the copied elements with everything they own, the
//! relationships among them, and their presentations. Pasting either duplicates the elements
//! (new ids, free names) or shows the original elements again ("paste as reference").
//!
//! What a copied element owns comes along in the model, and so do the nodes the diagram shows of
//! it (substates, pins, parts and ports, the attribute ellipses of a Chen entity), selected or
//! not, so that nothing is pasted undrawn. Lines between copied nodes come along: relationships
//! whose ends are copied, note anchors, and on ER diagrams the lines of attributes and the lines
//! of IE and IDEF1X relationships, whose relationship is copied with its participations and keeps
//! the foreign key columns of its copied child. On an ER diagram in the other notation, what is
//! pasted is drawn in that one ([`crate::er::fit_pasted`]).

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    DiagramKind, EdgeLabels, ElementData, ElementKind, ErNotation, Id, Model, ModelError, Op,
    Point, Presentation, Rect, Regions, Result, Shape, Tx,
};

use crate::palette::{can_show, owner_for};

#[derive(Clone, Debug, PartialEq)]
pub struct ClipElement {
    pub id: Id,
    pub data: ElementData,
    pub children: Vec<ClipElement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipNode {
    pub presentation: Id,
    pub element: Id,
    pub bounds: Rect,
    /// How an orthogonal state's regions are laid out.
    pub regions: Option<Regions>,
    pub style: umlcraft_model::Style,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipEdge {
    /// The relationship, or `None` for a note anchor.
    pub element: Option<Id>,
    pub source: Id,
    pub target: Id,
    pub waypoints: Vec<Point>,
    pub anchors: [Option<Point>; 2],
    pub labels: EdgeLabels,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// Copied elements that are not owned by another copied element.
    pub roots: Vec<ClipElement>,
    /// Relationships whose ends are all copied, pasted after the roots.
    pub relationships: Vec<ClipElement>,
    pub nodes: Vec<ClipNode>,
    pub edges: Vec<ClipEdge>,
    /// Free texts and lines, which show no element and are copied as they are.
    pub free: Vec<Presentation>,
    /// The notation of the ER diagram copied from.
    pub notation: Option<ErNotation>,
}

impl Clip {
    /// The area the copied nodes, texts and lines cover.
    pub fn bounds(&self) -> Option<Rect> {
        let free = self.free.iter().filter_map(|p| match &p.shape {
            Shape::Text { bounds, .. } => Some(*bounds),
            Shape::Line { from, to, .. } => Some(Rect::from_corners(*from, *to)),
            _ => None,
        });
        self.nodes.iter().map(|n| n.bounds).chain(free).reduce(|a, b| a.union(&b))
    }
}

/// `p` moved by `offset`, if it is a free text or line.
fn moved_free(p: &Presentation, offset: Point) -> Option<Presentation> {
    let mut p = p.clone();
    match &mut p.shape {
        Shape::Text { bounds, .. } => *bounds = bounds.translate(offset.x, offset.y),
        Shape::Line { from, to, .. } => {
            *from = from.offset(offset.x, offset.y);
            *to = to.offset(offset.x, offset.y);
        }
        _ => return None,
    }
    Some(p)
}

fn snapshot(model: &Model, id: Id) -> Option<ClipElement> {
    let e = model.get(id)?;
    Some(ClipElement {
        id,
        data: e.data().clone(),
        children: e.children().iter().filter_map(|&c| snapshot(model, c)).collect(),
    })
}

fn collect_ids(element: &ClipElement, into: &mut Vec<Id>) {
    into.push(element.id);
    for child in &element.children {
        collect_ids(child, into);
    }
}

/// Copies the selected node presentations of `diagram` (edges in the selection are taken along
/// automatically when both their ends are copied). Returns `None` if nothing can be copied.
pub fn copy(model: &Model, diagram: Id, selection: &[Id]) -> Option<Clip> {
    let view = model.view(diagram)?;
    let clip_node = |p: &Presentation| {
        Some(ClipNode {
            presentation: p.id,
            element: p.element?,
            bounds: p.bounds()?,
            regions: p.regions().cloned(),
            style: p.style.clone(),
        })
    };
    let mut nodes: Vec<ClipNode> = selection
        .iter()
        .filter_map(|&p| view.get(p))
        // The frame of a sequence or activity diagram stands for the diagram itself.
        .filter(|p| p.element != Some(diagram))
        .filter_map(clip_node)
        .collect();
    let free: Vec<Presentation> = selection
        .iter()
        .filter_map(|&p| view.get(p))
        .filter_map(|p| moved_free(p, Point::default()))
        .collect();
    if nodes.is_empty() && free.is_empty() {
        return None;
    }
    let selected: HashSet<Id> = nodes.iter().map(|n| n.element).collect();
    // Elements already inside another copied element come along with it. The roots keep the
    // order of the selection, so pasting does the same every time.
    let mut seen = HashSet::new();
    let roots: Vec<ClipElement> = nodes
        .iter()
        .map(|n| n.element)
        .filter(|&e| seen.insert(e))
        .filter(|&e| !model.ancestors(e).iter().any(|a| selected.contains(a)))
        .filter_map(|e| snapshot(model, e))
        .collect();
    let mut copied = Vec::new();
    roots.iter().for_each(|r| collect_ids(r, &mut copied));
    let copied: HashSet<Id> = copied.into_iter().collect();

    // What the copied elements own and the diagram shows comes along drawn, as the copy has it.
    let mut node_ids: HashSet<Id> = nodes.iter().map(|n| n.presentation).collect();
    let owned: Vec<ClipNode> = view
        .presentations()
        .iter()
        .filter(|p| !node_ids.contains(&p.id) && p.element.is_some_and(|e| copied.contains(&e)))
        .filter(|p| matches!(p.shape, Shape::Node { .. }))
        .filter_map(clip_node)
        .collect();
    node_ids.extend(owned.iter().map(|n| n.presentation));
    nodes.extend(owned);

    let mut relationships = Vec::new();
    let mut taken = HashSet::new();
    let mut edges = Vec::new();
    for p in view.presentations() {
        let Shape::Edge { source, target, waypoints, source_anchor, target_anchor, labels } =
            &p.shape
        else {
            continue;
        };
        if !node_ids.contains(source) || !node_ids.contains(target) {
            continue;
        }
        if let Some(element) = p.element {
            let Some(e) = model.get(element) else { continue };
            let comes_along = match e.kind() {
                // An attribute's line in Chen shows the attribute, copied with its owner.
                ElementKind::ErAttribute(_) => copied.contains(&element),
                // A relationship drawn as one line in IE and IDEF1X, between copied entities.
                ElementKind::ErRelationship(_) => {
                    let mut ends = crate::er::participations(model, element).peekable();
                    ends.peek().is_some() && ends.all(|p| copied.contains(&p.entity))
                }
                kind => {
                    kind.is_relationship()
                        && kind
                            .references()
                            .iter()
                            .filter(|(_, role)| role.is_strong())
                            .all(|(id, _)| copied.contains(id))
                }
            };
            if !comes_along {
                continue;
            }
            if !copied.contains(&element) && taken.insert(element) {
                relationships.extend(snapshot(model, element));
            }
        }
        edges.push(ClipEdge {
            element: p.element,
            source: *source,
            target: *target,
            waypoints: waypoints.clone(),
            anchors: [*source_anchor, *target_anchor],
            labels: *labels,
        });
    }
    let notation = crate::er::is_er(model, diagram).then(|| crate::er::notation(model, diagram));
    Some(Clip { roots, relationships, nodes, edges, free, notation })
}

/// `name` if no sibling under `owner` has it, else `name2`, `name3`, …; empty names stay empty.
pub fn free_name(model: &Model, owner: Id, name: &str) -> String {
    let taken = |n: &str| {
        model.children(owner).iter().any(|&c| model.get(c).is_some_and(|e| e.name() == n))
    };
    if name.is_empty() || !taken(name) {
        return name.to_owned();
    }
    (2..).map(|n| format!("{name}{n}")).find(|n| !taken(n)).expect("names are unbounded")
}

/// Copies that cannot be inserted yet, collected while pasting.
#[derive(Default)]
struct Pending<'c> {
    /// Elements with weak references to copies not inserted yet, such as an attribute typed by
    /// a nested class declared after it, with their complete kind, to be set once everything
    /// is in place.
    deferred: Vec<(Id, ElementKind)>,
    /// Elements (with what they own) whose strong references point to copies not inserted
    /// yet, such as a generalization owned by its specific class that names a general class
    /// pasted later; with their owner, the index they would have had there and whether to
    /// rename them.
    postponed: Vec<(&'c ClipElement, Id, usize, bool)>,
}

/// The ids of the copies by the id of what they copy, and the set of them.
struct Copies {
    map: HashMap<Id, Id>,
    new: HashSet<Id>,
}

impl Copies {
    fn new(map: HashMap<Id, Id>) -> Self {
        let new = map.values().copied().collect();
        Self { map, new }
    }
}

/// Inserts `element` and what it owns under `owner`, at `index` or at the end, or postpones
/// it (see [`Pending`]).
fn insert_tree<'c>(
    tx: &mut Tx<'_>,
    element: &'c ClipElement,
    owner: Id,
    index: Option<usize>,
    copies: &Copies,
    rename: bool,
    pending: &mut Pending<'c>,
) -> Result<()> {
    let map = &copies.map;
    let mut data = element.data.clone();
    data.kind.remap(map);
    let index = match index {
        Some(index) => index,
        None => tx.model().element(owner)?.children().len(),
    };
    let is_copy = |id: &Id| copies.new.contains(id);
    // A foreign key column copied without its relationship is a plain column of the copy.
    if let ElementKind::ErAttribute(p) = &mut data.kind
        && p.foreign_key.is_some_and(|r| !is_copy(&r))
    {
        p.foreign_key = None;
    }
    let references = data.kind.references();
    if references
        .iter()
        .any(|(id, role)| role.is_strong() && !tx.model().contains(*id) && is_copy(id))
    {
        pending.postponed.push((element, owner, index, rename));
        return Ok(());
    }
    let absent = references
        .into_iter()
        .filter(|(id, role)| !role.is_strong() && !tx.model().contains(*id))
        .map(|(id, _)| id);
    let (later, deleted): (Vec<Id>, Vec<Id>) = absent.partition(is_copy);
    // Weak references to elements deleted since copying are dropped.
    for id in deleted {
        data.kind.unlink(id, "");
    }
    let id = map[&element.id];
    if !later.is_empty() {
        pending.deferred.push((id, data.kind.clone()));
        for target in later {
            data.kind.unlink(target, "");
        }
    }
    if rename {
        data.name = free_name(tx.model(), owner, &data.name);
    }
    tx.apply(Op::InsertElement { id, owner, index, data })?;
    for child in &element.children {
        insert_tree(tx, child, id, None, copies, false, pending)?;
    }
    Ok(())
}

/// Inserts the postponed elements once what they refer to is in place. Later ones go first,
/// so that each index still counts only the siblings inserted before it.
fn insert_postponed<'c>(tx: &mut Tx<'_>, map: &Copies, pending: &mut Pending<'c>) -> Result<()> {
    loop {
        let postponed = std::mem::take(&mut pending.postponed);
        let count = postponed.len();
        if count == 0 {
            return Ok(());
        }
        for (element, owner, index, rename) in postponed.into_iter().rev() {
            let index = index.min(tx.model().element(owner)?.children().len());
            insert_tree(tx, element, owner, Some(index), map, rename, pending)?;
        }
        if pending.postponed.len() >= count {
            return Err(ModelError::Invalid("the copied elements refer to each other".into()));
        }
        pending.postponed.reverse();
    }
}

/// Pastes `clip` into `diagram`, shifted by `offset`. Returns the new presentations.
///
/// With `as_reference`, the original elements are shown again (those already on the diagram are
/// skipped); otherwise they are duplicated into the diagram's namespace.
///
/// Only elements the diagram can show are pasted. Sequence diagrams take only free texts and
/// lines: their lifelines, bars, messages and fragments are laid out together and cannot be
/// pasted as boxes.
pub fn paste(
    tx: &mut Tx<'_>,
    diagram: Id,
    clip: &Clip,
    offset: Point,
    as_reference: bool,
) -> Result<Vec<Id>> {
    let diagram_element = tx.model().element(diagram)?;
    let diagram_owner = diagram_element.owner().ok_or(ModelError::Root)?;
    let diagram_kind =
        diagram_element.kind().as_diagram().ok_or(ModelError::NotADiagram(diagram))?;
    let shows =
        |kind: &ElementKind| diagram_kind != DiagramKind::Sequence && can_show(diagram_kind, kind);
    let mut element_map: HashMap<Id, Id> = HashMap::new();

    if as_reference {
        for node in &clip.nodes {
            if tx.model().get(node.element).is_some_and(|e| shows(e.kind())) {
                element_map.insert(node.element, node.element);
            }
        }
        for edge in &clip.edges {
            if let Some(e) = edge.element {
                element_map.insert(e, e);
            }
        }
    } else {
        let roots: Vec<&ClipElement> = clip.roots.iter().filter(|r| shows(&r.data.kind)).collect();
        let mut ids = Vec::new();
        roots.iter().copied().chain(&clip.relationships).for_each(|r| collect_ids(r, &mut ids));
        let mut copies = Copies::new(ids.into_iter().map(|old| (old, Id::generate())).collect());
        let mut pending = Pending::default();
        for root in roots {
            let owner = owner_for(tx.model(), diagram_owner, &root.data.kind).ok_or_else(|| {
                let kind = root.data.kind.type_name();
                ModelError::Refused(umlcraft_model::Refusal::CannotPaste(kind))
            })?;
            insert_tree(tx, root, owner, None, &copies, true, &mut pending)?;
        }
        insert_postponed(tx, &copies, &mut pending)?;
        for relationship in &clip.relationships {
            let ends = relationship_ends(relationship, &copies.map);
            // Only between copies: an end the diagram could not take keeps its original id,
            // and a relationship to the original would be pasted undrawn.
            let pasted = |id: &Id| copies.new.contains(id) && tx.model().contains(*id);
            if ends.is_empty() || !ends.iter().all(pasted) {
                copies.map.remove(&relationship.id);
                continue;
            }
            let mut kind = relationship.data.kind.clone();
            kind.remap(&copies.map);
            let start = tx.model().element(ends[0])?.owner().unwrap_or(diagram_owner);
            let owner = owner_for(tx.model(), start, &kind)
                .ok_or_else(|| ModelError::Invalid("no owner for a relationship".into()))?;
            insert_tree(tx, relationship, owner, None, &copies, false, &mut pending)?;
        }
        insert_postponed(tx, &copies, &mut pending)?;
        for (id, mut kind) in pending.deferred {
            // What was to come but did not (a relationship whose ends were not pasted) is not
            // referred to.
            let gone: Vec<Id> = kind
                .references()
                .into_iter()
                .filter(|(id, role)| !role.is_strong() && !tx.model().contains(*id))
                .map(|(id, _)| id)
                .collect();
            for target in gone {
                kind.unlink(target, "");
            }
            tx.update(id, |data| data.kind = kind)?;
        }
        element_map = copies.map;
    }

    let mut presentation_map: HashMap<Id, Id> = HashMap::new();
    let mut created = Vec::new();
    let mut shown: HashSet<Id> = match tx.model().view(diagram) {
        Some(view) if as_reference => {
            view.presentations().iter().filter_map(|p| p.element).collect()
        }
        _ => HashSet::new(),
    };
    for node in &clip.nodes {
        let Some(&element) = element_map.get(&node.element) else { continue };
        if !tx.model().contains(element) || (as_reference && !shown.insert(element)) {
            continue;
        }
        let mut presentation = umlcraft_model::Presentation::node(
            Some(element),
            node.bounds.translate(offset.x, offset.y),
        );
        if let Shape::Node { regions, .. } = &mut presentation.shape {
            regions.clone_from(&node.regions);
        }
        presentation.style = node.style.clone();
        let id = tx.add_presentation(diagram, presentation)?;
        presentation_map.insert(node.presentation, id);
        created.push(id);
    }
    for edge in &clip.edges {
        let (Some(&source), Some(&target)) =
            (presentation_map.get(&edge.source), presentation_map.get(&edge.target))
        else {
            continue;
        };
        let element = match edge.element {
            Some(e) => match element_map.get(&e) {
                Some(&new) if tx.model().contains(new) => Some(new),
                _ => continue,
            },
            None => None,
        };
        let waypoints = edge.waypoints.iter().map(|w| w.offset(offset.x, offset.y)).collect();
        let mut presentation = Presentation::edge(element, source, target, waypoints);
        if let Shape::Edge { source_anchor, target_anchor, labels, .. } = &mut presentation.shape {
            [*source_anchor, *target_anchor] = edge.anchors;
            *labels = edge.labels;
        }
        created.push(tx.add_presentation(diagram, presentation)?);
    }
    // Free items are drawings, not elements: pasting them as reference duplicates them too.
    for free in &clip.free {
        let Some(mut presentation) = moved_free(free, offset) else { continue };
        presentation.id = Id::generate();
        created.push(tx.add_presentation(diagram, presentation)?);
    }
    if diagram_kind == DiagramKind::EntityRelationship {
        created = crate::er::fit_pasted(tx, diagram, created, clip.notation)?;
    }
    Ok(created)
}

/// What the copy of `relationship` connects, as copied: the ends its strong references name, or
/// for a relationship that names none itself (an ER relationship), the entities its
/// participations name.
fn relationship_ends(relationship: &ClipElement, map: &HashMap<Id, Id>) -> Vec<Id> {
    let strong = |kind: &ElementKind| -> Vec<Id> {
        let mut kind = kind.clone();
        kind.remap(map);
        kind.references()
            .into_iter()
            .filter(|(_, role)| role.is_strong())
            .map(|(id, _)| id)
            .collect()
    };
    let own = strong(&relationship.data.kind);
    if !own.is_empty() {
        return own;
    }
    let itself = map.get(&relationship.id).copied();
    relationship
        .children
        .iter()
        .flat_map(|c| strong(&c.data.kind))
        .filter(|&id| Some(id) != itself)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::ElementKind;
    use umlcraft_model::sample::order_system;

    #[test]
    fn copy_takes_subtrees_and_inner_relationships() {
        let (doc, ids) = order_system();
        let model = doc.model();
        let d = ids.class_diagram;
        let customer = model.shown_on(ids.customer)[0].1;
        let premium = model.shown_on(ids.premium)[0].1;
        let clip = copy(model, d, &[customer, premium]).unwrap();
        assert_eq!(clip.roots.len(), 2);
        assert_eq!(clip.relationships.len(), 1, "the generalization, not the association");
        assert_eq!(clip.edges.len(), 1);
        let customer_root = clip.roots.iter().find(|r| r.id == ids.customer).unwrap();
        assert_eq!(customer_root.children.len(), 1, "the name attribute comes along");
    }

    #[test]
    fn paste_duplicates_with_new_ids_and_free_names() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let customer = doc.model().shown_on(ids.customer)[0].1;
        let premium = doc.model().shown_on(ids.premium)[0].1;
        let clip = copy(doc.model(), d, &[customer, premium]).unwrap();
        let created =
            doc.edit("paste", |tx| paste(tx, d, &clip, Point::new(20.0, 20.0), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(created.len(), 3, "two nodes and the generalization edge");
        let names: Vec<&str> =
            model.children(ids.domain).iter().map(|&c| model.get(c).unwrap().name()).collect();
        assert!(names.contains(&"Customer2") && names.contains(&"PremiumCustomer2"), "{names:?}");
        // The copied generalization connects the copies, not the originals.
        let copy_of = |name: &str| {
            model
                .children(ids.domain)
                .iter()
                .copied()
                .find(|&c| model.get(c).unwrap().name() == name)
                .unwrap()
        };
        let (c2, p2) = (copy_of("Customer2"), copy_of("PremiumCustomer2"));
        let generalizations: Vec<_> =
            model.elements().filter_map(|e| e.kind().as_generalization().copied()).collect();
        assert!(generalizations.iter().any(|g| g.specific == p2 && g.general == c2));
        // New presentations are shifted.
        let node = model.view(d).unwrap().get(created[0]).unwrap();
        assert_eq!(node.bounds().unwrap().x, 100.0);
    }

    #[test]
    fn paste_as_reference_shows_originals_once() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let customer = doc.model().shown_on(ids.customer)[0].1;
        let clip = copy(doc.model(), d, &[customer]).unwrap();
        // Same diagram: already shown, nothing to add.
        let created = doc.edit("ref", |tx| paste(tx, d, &clip, Point::default(), true)).unwrap();
        assert!(created.is_empty());
        // Another diagram: shown there, same element.
        let other = doc
            .edit("diagram", |tx| {
                tx.create(
                    ids.domain,
                    ElementData::new(
                        "Other",
                        ElementKind::diagram(umlcraft_model::DiagramKind::Class),
                    ),
                )
            })
            .unwrap();
        let elements_before = doc.model().len();
        let created =
            doc.edit("ref", |tx| paste(tx, other, &clip, Point::default(), true)).unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(doc.model().shown_on(ids.customer).len(), 2);
        assert_eq!(doc.model().len(), elements_before, "no new model elements");
    }

    #[test]
    fn paste_keeps_types_that_refer_to_later_copies() {
        use umlcraft_model::{TemplateParameterProps, TypeRef};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (inner, t) = doc
            .edit("setup", |tx| {
                let inner =
                    tx.create(ids.customer, ElementData::new("Inner", ElementKind::class()))?;
                let parameter = ElementKind::TemplateParameter(TemplateParameterProps::default());
                let t = tx.create(ids.customer, ElementData::new("T", parameter))?;
                // Declared before what they refer to.
                let typed = |name: &str, target| {
                    ElementData::new(name, ElementKind::attribute(Some(TypeRef::Element(target))))
                };
                tx.create_at(ids.customer, 0, typed("inner", inner))?;
                tx.create_at(ids.customer, 0, typed("value", t))?;
                Ok((inner, t))
            })
            .unwrap();
        let customer = doc.model().shown_on(ids.customer)[0].1;
        let clip = copy(doc.model(), d, &[customer]).unwrap();
        doc.edit("paste", |tx| paste(tx, d, &clip, Point::default(), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let copy = model
            .children(ids.domain)
            .iter()
            .copied()
            .find(|&c| model.get(c).unwrap().name() == "Customer2")
            .unwrap();
        let child = |name: &str| {
            model.children(copy).iter().copied().find(|&c| model.get(c).unwrap().name() == name)
        };
        let type_of = |name: &str| match model.get(child(name).unwrap()).unwrap().kind() {
            ElementKind::Attribute(p) => p.type_ref.clone(),
            _ => None,
        };
        let (inner2, t2) = (child("Inner").unwrap(), child("T").unwrap());
        assert!(inner2 != inner && t2 != t);
        assert_eq!(type_of("inner"), Some(TypeRef::Element(inner2)));
        assert_eq!(type_of("value"), Some(TypeRef::Element(t2)));
    }

    /// The source import stores a generalization under its specific class, so the copy of
    /// that class refers to the copy of the general one, which may come later.
    #[test]
    fn paste_waits_for_what_owned_relationships_connect() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        doc.edit("own", |tx| tx.move_element(ids.premium_generalization, ids.premium, 0)).unwrap();
        let customer = doc.model().shown_on(ids.customer)[0].1;
        let premium = doc.model().shown_on(ids.premium)[0].1;
        for selection in [[premium, customer], [customer, premium]] {
            let clip = copy(doc.model(), d, &selection).unwrap();
            let mut copy = doc.clone();
            copy.edit("paste", |tx| paste(tx, d, &clip, Point::default(), false)).unwrap();
            let model = copy.model();
            model.check().unwrap();
            let premium2 = model
                .children(ids.domain)
                .iter()
                .copied()
                .find(|&c| model.get(c).unwrap().name() == "PremiumCustomer2")
                .unwrap();
            // The generalization is back in its place, first in the class.
            let first = model.children(premium2)[0];
            assert!(model.get(first).unwrap().kind().as_generalization().is_some());
        }
    }

    #[test]
    fn sequence_diagrams_take_no_pasted_boxes() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let all: Vec<Id> =
            doc.model().view(d).unwrap().presentations().iter().map(|p| p.id).collect();
        let clip = copy(doc.model(), d, &all).unwrap();
        assert!(clip.nodes.iter().all(|n| n.element != d), "the frame is not copied");
        let before = doc.model().clone();
        for as_reference in [false, true] {
            let created = doc
                .edit("paste", |tx| paste(tx, d, &clip, Point::new(20.0, 20.0), as_reference))
                .unwrap();
            assert!(created.is_empty());
            assert_eq!(*doc.model(), before);
        }
        // Classes are no lifelines.
        let customer = doc.model().shown_on(ids.customer)[0].1;
        let clip = copy(doc.model(), ids.class_diagram, &[customer]).unwrap();
        let created = doc.edit("paste", |tx| paste(tx, d, &clip, Point::default(), true)).unwrap();
        assert!(created.is_empty());
        // Nor as copies: the generalization between them must not join the originals.
        let premium = doc.model().shown_on(ids.premium)[0].1;
        let clip = copy(doc.model(), ids.class_diagram, &[customer, premium]).unwrap();
        assert_eq!(clip.relationships.len(), 1);
        let before = doc.model().clone();
        let created = doc.edit("paste", |tx| paste(tx, d, &clip, Point::default(), false)).unwrap();
        assert!(created.is_empty());
        assert_eq!(*doc.model(), before);
    }

    #[test]
    fn free_texts_and_lines_are_copied_and_pasted() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (text, line) = doc
            .edit("setup", |tx| {
                let text = Presentation::text(Rect::new(10.0, 10.0, 60.0, 25.0), "note".into());
                let mut line = Presentation::line(Point::new(0.0, 100.0), Point::new(50.0, 120.0));
                if let Shape::Line { dashed, .. } = &mut line.shape {
                    *dashed = true;
                }
                Ok((tx.add_presentation(d, text)?, tx.add_presentation(d, line)?))
            })
            .unwrap();
        let clip = copy(doc.model(), d, &[text, line]).unwrap();
        assert!(clip.nodes.is_empty() && clip.free.len() == 2);
        assert_eq!(clip.bounds(), Some(Rect::new(0.0, 10.0, 70.0, 110.0)));
        let created =
            doc.edit("paste", |tx| paste(tx, d, &clip, Point::new(5.0, 5.0), true)).unwrap();
        let view = doc.model().view(d).unwrap();
        assert_eq!(created.len(), 2);
        assert!(!created.contains(&text) && !created.contains(&line));
        assert_eq!(view.get(created[0]).unwrap().bounds(), Some(Rect::new(15.0, 15.0, 60.0, 25.0)));
        let Shape::Line { from, dashed, .. } = view.get(created[1]).unwrap().shape else {
            panic!("a line")
        };
        assert_eq!((from, dashed), (Point::new(5.0, 105.0), true));
    }

    fn named(model: &Model, name: &str) -> Vec<Id> {
        model.elements().filter(|e| e.name() == name).map(|e| e.id()).collect()
    }

    /// An orthogonal state copied alone comes back with its regions side by side as they were
    /// and its substates drawn in them.
    #[test]
    fn orthogonal_states_keep_their_regions_and_substates() {
        let (mut doc, _) = order_system();
        let open = named(doc.model(), "Open")[0];
        let (d, node) = doc.model().shown_on(open)[0];
        doc.edit("divide", |tx| crate::state_edit::add_region(tx, open, true, None)).unwrap();
        doc.edit("line", |tx| crate::state_edit::move_region_line(tx, d, node, 0, 250.0)).unwrap();
        let regions = doc.model().view(d).unwrap().get(node).unwrap().regions().cloned();
        assert!(regions.as_ref().is_some_and(|r| r.vertical));
        let clip = copy(doc.model(), d, &[node]).unwrap();
        let before = crate::invariants::check(doc.model(), &crate::scene::ApproxMeasure);
        let created =
            doc.edit("paste", |tx| paste(tx, d, &clip, Point::new(0.0, 400.0), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let view = model.view(d).unwrap();
        let state = view.get(created[0]).unwrap();
        assert_eq!(state.regions().cloned(), regions);
        // "Empty", "Filled" and the rest come along drawn, inside the copy of "Open".
        let inside = |state: Id| {
            view.presentations()
                .iter()
                .filter(|p| p.bounds().is_some())
                .filter(|p| p.element.is_some_and(|e| model.is_ancestor(state, e)))
                .count()
        };
        let copy_of_open = state.element.unwrap();
        assert!(inside(open) >= 2);
        assert_eq!(inside(copy_of_open), inside(open), "every substate drawn");
        let after = crate::invariants::check(model, &crate::scene::ApproxMeasure);
        assert_eq!(crate::invariants::new_violations(&before, &after), Vec::<String>::new());
    }

    /// An ER diagram with `Order` and `OrderLine` (keyed by the order) in `notation`, the
    /// second one keyed by an identifying relationship in IE. Returns the document, the
    /// diagram and the entities' nodes.
    fn er(notation: umlcraft_model::ErNotation) -> (umlcraft_model::Document, Id, Id, Id) {
        use crate::palette::{NodeTool, create_node};
        let mut doc = umlcraft_model::Document::new(Model::new("m"));
        let root = doc.model().root();
        let measure = crate::scene::ApproxMeasure;
        let (d, order, line) = doc
            .edit("setup", |tx| {
                let p = tx.create(root, ElementData::new("p", ElementKind::Package))?;
                let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
                let d = tx.create(p, ElementData::new("d", kind))?;
                crate::er::set_notation(tx, d, notation, &measure)?;
                let (o, order) =
                    create_node(tx, d, NodeTool::ErEntity, Point::new(0.0, 100.0), "Order")?;
                let (_, line) =
                    create_node(tx, d, NodeTool::ErEntity, Point::new(300.0, 100.0), "Line")?;
                crate::er::add_column(tx, o, "order no", true)?;
                Ok((d, order, line))
            })
            .unwrap();
        (doc, d, order, line)
    }

    fn other_er_diagram(doc: &mut umlcraft_model::Document, d: Id, notation: ErNotation) -> Id {
        let owner = doc.model().get(d).unwrap().owner().unwrap();
        doc.edit("other", |tx| {
            let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
            let other = tx.create(owner, ElementData::new("other", kind))?;
            crate::er::set_notation(tx, other, notation, &crate::scene::ApproxMeasure)?;
            Ok(other)
        })
        .unwrap()
    }

    fn foreign_keys(model: &Model, entity: Id) -> Vec<Option<Id>> {
        crate::er::attributes(model, entity)
            .filter_map(|a| match a.kind() {
                ElementKind::ErAttribute(p) => Some(p.foreign_key),
                _ => None,
            })
            .collect()
    }

    /// Two tables joined by an IE line: the relationship comes along, and the copied child keeps
    /// its foreign key, to the copied relationship. Pasted onto a Chen diagram, the line becomes
    /// a rhombus with its participation lines, and the attributes get their ellipses.
    #[test]
    fn ie_lines_and_foreign_keys_are_copied() {
        let (mut doc, d, order, line) = er(ErNotation::Ie);
        let edge = doc
            .edit("relate", |tx| {
                crate::er::relate(tx, d, crate::er::IeLine::Identifying, order, line)
            })
            .unwrap();
        let relationship = doc.model().view(d).unwrap().get(edge).unwrap().element.unwrap();
        let clip = copy(doc.model(), d, &[order, line]).unwrap();
        assert_eq!(clip.relationships.len(), 1);
        assert_eq!(clip.edges.len(), 1);
        let created =
            doc.edit("paste", |tx| paste(tx, d, &clip, Point::new(0.0, 300.0), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(created.len(), 3, "two tables and their line");
        let view = model.view(d).unwrap();
        let copy = view.get(created[2]).unwrap().element.unwrap();
        assert_ne!(copy, relationship);
        let line_copy = view.get(created[1]).unwrap().element.unwrap();
        assert_eq!(foreign_keys(model, line_copy), [Some(copy)], "(FK) to the copied line");
        let ends: Vec<Id> = crate::er::participations(model, copy).map(|p| p.entity).collect();
        let order_copy = view.get(created[0]).unwrap().element.unwrap();
        assert_eq!(ends, [order_copy, line_copy]);
        assert!(crate::invariants::check(model, &crate::scene::ApproxMeasure).is_empty());

        // Onto a Chen diagram: a rhombus, two participation lines and the key's ellipse.
        let chen = other_er_diagram(&mut doc, d, ErNotation::Chen);
        let created =
            doc.edit("paste", |tx| paste(tx, chen, &clip, Point::default(), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let view = model.view(chen).unwrap();
        let kinds: Vec<&str> = created
            .iter()
            .filter_map(|&p| view.get(p))
            .filter_map(|p| Some(model.get(p.element?)?.kind().type_name()))
            .collect();
        let count = |name: &str| kinds.iter().filter(|&&k| k == name).count();
        assert_eq!(count("erEntity"), 2, "{kinds:?}");
        assert_eq!(count("erRelationship"), 1, "a rhombus: {kinds:?}");
        assert_eq!(count("erParticipation"), 2, "{kinds:?}");
        // The order's key gets an ellipse and its line; the foreign key column does not.
        assert_eq!(count("erAttribute"), 2, "{kinds:?}");
        assert!(created.iter().all(|&p| view.get(p).is_some()));
    }

    /// A Chen entity copied alone takes its attribute ellipses and their lines along; pasted
    /// onto an IE diagram, it lists them instead.
    #[test]
    fn chen_attributes_come_along_with_their_lines() {
        let (mut doc, d, order, _) = er(ErNotation::Chen);
        doc.edit("attribute", |tx| crate::er::add_attribute(tx, d, order, "date")).unwrap();
        let clip = copy(doc.model(), d, &[order]).unwrap();
        assert_eq!(clip.nodes.len(), 2, "the entity and its ellipse");
        assert_eq!(clip.edges.len(), 1, "the attribute's line");
        let created =
            doc.edit("paste", |tx| paste(tx, d, &clip, Point::new(0.0, 300.0), false)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(created.len(), 3);
        let view = model.view(d).unwrap();
        let line = view.get(created[2]).unwrap();
        assert_eq!(line.endpoints(), Some((created[0], created[1])), "from the owner");
        let attribute = view.get(created[1]).unwrap().element.unwrap();
        assert_eq!(line.element, Some(attribute));
        assert_eq!(model.get(attribute).unwrap().owner(), view.get(created[0]).unwrap().element);

        let ie = other_er_diagram(&mut doc, d, ErNotation::Ie);
        let created = doc.edit("paste", |tx| paste(tx, ie, &clip, Point::default(), true)).unwrap();
        let model = doc.model();
        let view = model.view(ie).unwrap();
        assert_eq!(created.len(), 1, "the table alone");
        let entity = model.view(d).unwrap().get(order).unwrap().element;
        assert_eq!(view.get(created[0]).unwrap().element, entity);
        assert_eq!(view.presentations().len(), 1);
    }

    /// On IE, an attribute is not shown, a relationship is shown as its line, and an entity
    /// shown comes with the lines to the tables already there.
    #[test]
    fn showing_on_ie_draws_lines() {
        let (mut doc, d, order, line) = er(ErNotation::Ie);
        let edge = doc
            .edit("relate", |tx| {
                crate::er::relate(tx, d, crate::er::IeLine::Identifying, order, line)
            })
            .unwrap();
        let view = doc.model().view(d).unwrap();
        let relationship = view.get(edge).unwrap().element.unwrap();
        let (order_entity, line_entity) =
            (view.get(order).unwrap().element.unwrap(), view.get(line).unwrap().element.unwrap());
        let column = crate::er::attributes(doc.model(), order_entity).next().unwrap().id();
        let show = |doc: &mut umlcraft_model::Document, e: Id| {
            doc.edit("show", |tx| crate::palette::show_element(tx, d, e, Point::new(0.0, 400.0)))
        };
        assert!(show(&mut doc, column).is_err(), "IE lists attributes");
        doc.edit("remove", |tx| tx.remove_presentation(d, edge)).unwrap();
        let shown = show(&mut doc, relationship).unwrap();
        let model = doc.model();
        assert_eq!(model.view(d).unwrap().get(shown).unwrap().endpoints(), Some((order, line)));
        // The child table shown again brings its line along, and no rhombus.
        doc.edit("remove", |tx| tx.remove_presentation(d, line)).unwrap();
        let node = show(&mut doc, line_entity).unwrap();
        let model = doc.model();
        let view = model.view(d).unwrap();
        let lines: Vec<_> = view.presentations_of(relationship).collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].endpoints(), Some((order, node)));
        assert_eq!(view.presentations().len(), 3);
    }

    #[test]
    fn free_names() {
        let (doc, ids) = order_system();
        assert_eq!(free_name(doc.model(), ids.domain, "Customer"), "Customer2");
        assert_eq!(free_name(doc.model(), ids.domain, "Fresh"), "Fresh");
        assert_eq!(free_name(doc.model(), ids.domain, ""), "");
    }
}
