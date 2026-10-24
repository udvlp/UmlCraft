use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::discriminant;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::diagram::{Presentation, Shape, View};
use crate::element::{ElementData, ElementKind, RefRole};
use crate::error::{ModelError, Result};
use crate::id::Id;
use crate::op::Op;

/// An element together with its place in the ownership tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    id: Id,
    owner: Option<Id>,
    children: Vec<Id>,
    data: ElementData,
}

impl Element {
    pub fn id(&self) -> Id {
        self.id
    }

    /// `None` only for the model root.
    pub fn owner(&self) -> Option<Id> {
        self.owner
    }

    /// Owned elements in their user-visible order.
    pub fn children(&self) -> &[Id] {
        &self.children
    }

    pub fn data(&self) -> &ElementData {
        &self.data
    }

    pub fn name(&self) -> &str {
        &self.data.name
    }

    pub fn kind(&self) -> &ElementKind {
        &self.data.kind
    }
}

/// Flat, serializable snapshot of a model, see [`Model::to_records`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementRecord {
    pub id: Id,
    #[serde(flatten)]
    pub data: ElementData,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Id>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelRecords {
    pub root: Id,
    /// Sorted by id.
    pub elements: Vec<ElementRecord>,
    /// Presentations of every diagram, in z-order.
    pub views: BTreeMap<Id, Vec<Presentation>>,
}

/// A UML model: an ownership tree of elements plus one view per diagram element.
///
/// The model only changes through [`Model::apply`], which validates every operation and keeps
/// the invariants checked by [`Model::check`].
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    root: Id,
    elements: IndexMap<Id, Element>,
    views: IndexMap<Id, View>,
}

impl Model {
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_root(Id::generate(), name)
    }

    pub fn with_root(root: Id, name: impl Into<String>) -> Self {
        let mut elements = IndexMap::new();
        elements.insert(
            root,
            Element {
                id: root,
                owner: None,
                children: Vec::new(),
                data: ElementData::new(name, ElementKind::Model),
            },
        );
        Self { root, elements, views: IndexMap::new() }
    }

    pub fn root(&self) -> Id {
        self.root
    }

    pub fn get(&self, id: Id) -> Option<&Element> {
        self.elements.get(&id)
    }

    pub fn element(&self, id: Id) -> Result<&Element> {
        self.get(id).ok_or(ModelError::NotFound(id))
    }

    pub fn contains(&self, id: Id) -> bool {
        self.elements.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        // The root always exists.
        false
    }

    /// All elements, in no particular order.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.elements.values()
    }

    pub fn children(&self, id: Id) -> &[Id] {
        self.get(id).map_or(&[], |e| e.children())
    }

    /// `id` and everything it owns, in pre-order.
    pub fn subtree(&self, id: Id) -> Vec<Id> {
        let mut result = Vec::new();
        let mut stack = vec![id];
        while let Some(current) = stack.pop() {
            if let Some(e) = self.get(current) {
                result.push(current);
                stack.extend(e.children.iter().rev());
            }
        }
        result
    }

    /// Whether `ancestor` owns `id` directly or indirectly.
    pub fn is_ancestor(&self, ancestor: Id, id: Id) -> bool {
        let mut current = self.get(id).and_then(|e| e.owner);
        while let Some(owner) = current {
            if owner == ancestor {
                return true;
            }
            current = self.get(owner).and_then(|e| e.owner);
        }
        false
    }

    /// Owner chain from the direct owner up to the root.
    pub fn ancestors(&self, id: Id) -> Vec<Id> {
        let mut result = Vec::new();
        let mut current = self.get(id).and_then(|e| e.owner);
        while let Some(owner) = current {
            result.push(owner);
            current = self.get(owner).and_then(|e| e.owner);
        }
        result
    }

    /// Diagram elements and their views.
    pub fn views(&self) -> impl Iterator<Item = (Id, &View)> {
        self.views.iter().map(|(&id, view)| (id, view))
    }

    pub fn view(&self, diagram: Id) -> Option<&View> {
        self.views.get(&diagram)
    }

    /// Elements referring to `target`, with the role of the reference.
    pub fn referrers(&self, target: Id) -> Vec<(Id, RefRole)> {
        let mut found = Vec::new();
        for e in self.elements.values() {
            e.data.kind.visit_references(&mut |to, role| {
                if to == target {
                    found.push((e.id, role));
                }
            });
        }
        found
    }

    /// `(diagram, presentation)` pairs showing `element`.
    pub fn shown_on(&self, element: Id) -> Vec<(Id, Id)> {
        self.views
            .iter()
            .flat_map(|(&diagram, view)| {
                view.presentations_of(element).map(move |p| (diagram, p.id))
            })
            .collect()
    }

    /// Applies an operation recorded as the inverse of one that was applied, when undoing or
    /// redoing. Such an operation is valid by construction: a removal undoes an insertion, so
    /// what referred to the element or showed it has been taken back before. That spares the
    /// look through the whole model per removal, which made undoing a large import quadratic.
    pub(crate) fn apply_recorded(&mut self, op: Op) -> Result<Op> {
        match op {
            Op::RemoveElement { id } => self.remove_unreferenced(id),
            op => self.apply(op),
        }
    }

    /// Applies a primitive operation and returns its inverse.
    pub fn apply(&mut self, op: Op) -> Result<Op> {
        match op {
            Op::InsertElement { id, owner, index, data } => {
                self.insert_element(id, owner, index, data)
            }
            Op::RemoveElement { id } => self.remove_element(id),
            Op::UpdateElement { id, data } => self.update_element(id, data),
            Op::MoveElement { id, owner, index } => self.move_element(id, owner, index),
            Op::InsertPresentation { diagram, index, presentation } => {
                self.insert_presentation(diagram, index, presentation)
            }
            Op::RemovePresentation { diagram, id } => self.remove_presentation(diagram, id),
            Op::UpdatePresentation { diagram, presentation } => {
                self.update_presentation(diagram, presentation)
            }
            Op::ReorderPresentation { diagram, id, index } => {
                self.reorder_presentation(diagram, id, index)
            }
        }
    }

    fn insert_element(&mut self, id: Id, owner: Id, index: usize, data: ElementData) -> Result<Op> {
        if matches!(data.kind, ElementKind::Model) {
            return Err(ModelError::Root);
        }
        if self.elements.contains_key(&id) {
            return Err(ModelError::DuplicateId(id));
        }
        let owner_element = self.element(owner)?;
        if !owner_element.data.kind.can_own(&data.kind) {
            return Err(ModelError::InvalidOwner {
                owner,
                owner_type: owner_element.data.kind.type_name(),
                child: id,
                child_type: data.kind.type_name(),
            });
        }
        if index > owner_element.children.len() {
            return Err(ModelError::IndexOutOfRange { index });
        }
        self.check_references(id, &data.kind)?;

        if data.kind.as_diagram().is_some() {
            self.views.insert(id, View::default());
        }
        self.elements.insert(id, Element { id, owner: Some(owner), children: Vec::new(), data });
        self.elements[&owner].children.insert(index, id);
        Ok(Op::RemoveElement { id })
    }

    fn remove_element(&mut self, id: Id) -> Result<Op> {
        if id == self.root {
            return Err(ModelError::Root);
        }
        let element = self.element(id)?;
        if !element.children.is_empty() {
            return Err(ModelError::HasChildren(id));
        }
        if let Some(&(referrer, _)) = self.referrers(id).first() {
            return Err(ModelError::StillReferenced { target: id, referrer });
        }
        if let Some(&(diagram, _)) = self.shown_on(id).first() {
            return Err(ModelError::StillShown { target: id, diagram });
        }
        self.remove_unreferenced(id)
    }

    /// Removes a leaf element that nothing refers to and no diagram shows, without looking
    /// through the whole model for that: the caller has made sure of it, as
    /// [`Tx::delete_many`](crate::Tx::delete_many) does for all it deletes at once.
    pub(crate) fn remove_unreferenced(&mut self, id: Id) -> Result<Op> {
        if id == self.root {
            return Err(ModelError::Root);
        }
        if !self.element(id)?.children.is_empty() {
            return Err(ModelError::HasChildren(id));
        }
        if self.views.get(&id).is_some_and(|v| !v.presentations.is_empty()) {
            return Err(ModelError::DiagramNotEmpty(id));
        }

        let element = self.elements.swap_remove(&id).expect("checked above");
        self.views.swap_remove(&id);
        let owner = element.owner.expect("non-root elements have an owner");
        let siblings = &mut self.elements[&owner].children;
        let index = siblings.iter().position(|&c| c == id).expect("owner lists its children");
        siblings.remove(index);
        Ok(Op::InsertElement { id, owner, index, data: element.data })
    }

    fn update_element(&mut self, id: Id, data: ElementData) -> Result<Op> {
        let element = self.element(id)?;
        let old_kind = &element.data.kind;
        if discriminant(old_kind) != discriminant(&data.kind)
            || old_kind.as_diagram() != data.kind.as_diagram()
        {
            return Err(ModelError::KindChanged(id));
        }
        self.check_references(id, &data.kind)?;
        let old = std::mem::replace(&mut self.elements[&id].data, data);
        Ok(Op::UpdateElement { id, data: old })
    }

    fn move_element(&mut self, id: Id, owner: Id, index: usize) -> Result<Op> {
        if id == self.root {
            return Err(ModelError::Root);
        }
        let element = self.element(id)?;
        let new_owner = self.element(owner)?;
        if owner == id || self.is_ancestor(id, owner) {
            return Err(ModelError::OwnershipCycle(id));
        }
        if !new_owner.data.kind.can_own(&element.data.kind) {
            return Err(ModelError::InvalidOwner {
                owner,
                owner_type: new_owner.data.kind.type_name(),
                child: id,
                child_type: element.data.kind.type_name(),
            });
        }
        let old_owner = element.owner.expect("non-root elements have an owner");
        let len_after_removal = new_owner.children.len() - usize::from(old_owner == owner);
        if index > len_after_removal {
            return Err(ModelError::IndexOutOfRange { index });
        }

        let siblings = &mut self.elements[&old_owner].children;
        let old_index = siblings.iter().position(|&c| c == id).expect("owner lists its children");
        siblings.remove(old_index);
        self.elements[&owner].children.insert(index, id);
        self.elements[&id].owner = Some(owner);
        Ok(Op::MoveElement { id, owner: old_owner, index: old_index })
    }

    fn view_of(&self, diagram: Id) -> Result<&View> {
        match self.views.get(&diagram) {
            Some(view) => Ok(view),
            None if self.contains(diagram) => Err(ModelError::NotADiagram(diagram)),
            None => Err(ModelError::NotFound(diagram)),
        }
    }

    fn check_presentation(&self, view: &View, presentation: &Presentation) -> Result<()> {
        let invalid = |to| ModelError::InvalidReference { from: presentation.id, to };
        if let Some(element) = presentation.element
            && !self.get(element).is_some_and(|e| !matches!(e.data.kind, ElementKind::Model))
        {
            return Err(invalid(element));
        }
        if let Some((source, target)) = presentation.endpoints() {
            for end in [source, target] {
                if end == presentation.id || view.get(end).is_none() {
                    return Err(invalid(end));
                }
            }
        }
        if let Shape::Activation { lifeline, .. } = presentation.shape
            && !view.get(lifeline).is_some_and(|l| matches!(l.shape, Shape::Lifeline { .. }))
        {
            return Err(invalid(lifeline));
        }
        Ok(())
    }

    fn insert_presentation(
        &mut self,
        diagram: Id,
        index: usize,
        presentation: Presentation,
    ) -> Result<Op> {
        let view = self.view_of(diagram)?;
        if view.get(presentation.id).is_some() {
            return Err(ModelError::DuplicateId(presentation.id));
        }
        if index > view.presentations.len() {
            return Err(ModelError::IndexOutOfRange { index });
        }
        self.check_presentation(view, &presentation)?;
        let id = presentation.id;
        self.views[&diagram].presentations.insert(index, presentation);
        Ok(Op::RemovePresentation { diagram, id })
    }

    fn remove_presentation(&mut self, diagram: Id, id: Id) -> Result<Op> {
        let view = self.view_of(diagram)?;
        let index = view.index_of(id).ok_or(ModelError::NotFound(id))?;
        if view.edges_attached_to(id).next().is_some() {
            return Err(ModelError::EdgesAttached(id));
        }
        let presentation = self.views[&diagram].presentations.remove(index);
        Ok(Op::InsertPresentation { diagram, index, presentation })
    }

    fn update_presentation(&mut self, diagram: Id, presentation: Presentation) -> Result<Op> {
        let view = self.view_of(diagram)?;
        let index = view.index_of(presentation.id).ok_or(ModelError::NotFound(presentation.id))?;
        if discriminant(&view.presentations[index].shape) != discriminant(&presentation.shape) {
            return Err(ModelError::ShapeChanged(presentation.id));
        }
        self.check_presentation(view, &presentation)?;
        let old = std::mem::replace(&mut self.views[&diagram].presentations[index], presentation);
        Ok(Op::UpdatePresentation { diagram, presentation: old })
    }

    fn reorder_presentation(&mut self, diagram: Id, id: Id, index: usize) -> Result<Op> {
        let view = self.view_of(diagram)?;
        let old_index = view.index_of(id).ok_or(ModelError::NotFound(id))?;
        if index >= view.presentations.len() {
            return Err(ModelError::IndexOutOfRange { index });
        }
        let presentations = &mut self.views[&diagram].presentations;
        let presentation = presentations.remove(old_index);
        presentations.insert(index, presentation);
        Ok(Op::ReorderPresentation { diagram, id, index: old_index })
    }

    /// References must point to existing elements of a suitable metaclass, and generalizations
    /// must not form cycles.
    fn check_references(&self, id: Id, kind: &ElementKind) -> Result<()> {
        for (target, role) in kind.references() {
            if !self.get(target).is_some_and(|t| role.accepts(&t.data.kind)) {
                return Err(ModelError::InvalidReference { from: id, to: target });
            }
        }
        if let Some(g) = kind.as_generalization() {
            // Adding specific → general closes a cycle if general already reaches specific.
            let parents = self.generalization_parents(Some(id));
            let mut stack = vec![g.general];
            let mut seen = HashSet::new();
            while let Some(current) = stack.pop() {
                if current == g.specific {
                    return Err(ModelError::GeneralizationCycle(id));
                }
                if seen.insert(current) {
                    stack.extend(parents.get(&current).into_iter().flatten());
                }
            }
        }
        Ok(())
    }

    /// specific → generals, over all generalizations except `excluding`.
    fn generalization_parents(&self, excluding: Option<Id>) -> HashMap<Id, Vec<Id>> {
        let mut parents: HashMap<Id, Vec<Id>> = HashMap::new();
        for e in self.elements.values() {
            if Some(e.id) != excluding
                && let Some(g) = e.data.kind.as_generalization()
            {
                parents.entry(g.specific).or_default().push(g.general);
            }
        }
        parents
    }

    /// Verifies all structural invariants. Used after loading and in tests.
    pub fn check(&self) -> Result<()> {
        let invalid = |msg: String| Err(ModelError::Invalid(msg));
        let root = self.element(self.root)?;
        if root.owner.is_some() || !matches!(root.data.kind, ElementKind::Model) {
            return invalid("root must be an unowned model element".into());
        }
        for e in self.elements.values() {
            if e.id != self.root && matches!(e.data.kind, ElementKind::Model) {
                return invalid(format!("{} is a second model root", e.id));
            }
            for &child in &e.children {
                let Some(c) = self.get(child) else {
                    return invalid(format!("{} owns missing element {child}", e.id));
                };
                if c.owner != Some(e.id) {
                    return invalid(format!(
                        "{child} is listed by {} but owned by {:?}",
                        e.id, c.owner
                    ));
                }
                if !e.data.kind.can_own(&c.data.kind) {
                    return invalid(format!(
                        "{} ({}) cannot own {child} ({})",
                        e.id,
                        e.data.kind.type_name(),
                        c.data.kind.type_name()
                    ));
                }
            }
            if let Some(owner) = e.owner
                && !self.get(owner).is_some_and(|o| o.children.contains(&e.id))
            {
                return invalid(format!("{} is not listed by its owner {owner}", e.id));
            }
        }
        if self.subtree(self.root).len() != self.elements.len() {
            return invalid("some elements are not reachable from the root".into());
        }
        for e in self.elements.values() {
            for (target, role) in e.data.kind.references() {
                if !self.get(target).is_some_and(|t| role.accepts(&t.data.kind)) {
                    return Err(ModelError::InvalidReference { from: e.id, to: target });
                }
            }
        }
        self.check_generalizations_acyclic()?;

        let diagrams: HashSet<Id> = self
            .elements
            .values()
            .filter(|e| e.data.kind.as_diagram().is_some())
            .map(|e| e.id)
            .collect();
        if diagrams.len() != self.views.len() || !self.views.keys().all(|d| diagrams.contains(d)) {
            return invalid("diagram elements and views do not match".into());
        }
        for view in self.views.values() {
            let mut ids = HashSet::new();
            for p in &view.presentations {
                if !ids.insert(p.id) {
                    return Err(ModelError::DuplicateId(p.id));
                }
                self.check_presentation(view, p)?;
            }
        }
        Ok(())
    }

    fn check_generalizations_acyclic(&self) -> Result<()> {
        let parents = self.generalization_parents(None);
        // Iterative DFS with colors: 1 = on stack, 2 = done.
        let mut color: HashMap<Id, u8> = HashMap::new();
        for &start in parents.keys() {
            if color.contains_key(&start) {
                continue;
            }
            let mut stack = vec![(start, 0usize)];
            color.insert(start, 1);
            while let Some((node, next)) = stack.pop() {
                let edges = parents.get(&node).map_or(&[][..], Vec::as_slice);
                if let Some(&parent) = edges.get(next) {
                    stack.push((node, next + 1));
                    match color.get(&parent) {
                        Some(1) => {
                            return Err(ModelError::Invalid(format!(
                                "generalization cycle through {parent}"
                            )));
                        }
                        Some(_) => {}
                        None => {
                            color.insert(parent, 1);
                            stack.push((parent, 0));
                        }
                    }
                } else {
                    color.insert(node, 2);
                }
            }
        }
        Ok(())
    }

    pub fn to_records(&self) -> ModelRecords {
        let mut elements: Vec<ElementRecord> = self
            .elements
            .values()
            .map(|e| ElementRecord { id: e.id, data: e.data.clone(), children: e.children.clone() })
            .collect();
        elements.sort_by_key(|r| r.id);
        let views = self.views.iter().map(|(&id, view)| (id, view.presentations.clone())).collect();
        ModelRecords { root: self.root, elements, views }
    }

    pub fn from_records(records: ModelRecords) -> Result<Self> {
        let mut elements: IndexMap<Id, Element> = IndexMap::with_capacity(records.elements.len());
        for record in records.elements {
            let element = Element {
                id: record.id,
                owner: None,
                children: record.children,
                data: record.data,
            };
            if elements.insert(record.id, element).is_some() {
                return Err(ModelError::DuplicateId(record.id));
            }
        }
        let ownership: Vec<(Id, Id)> =
            elements.values().flat_map(|e| e.children.iter().map(move |&c| (e.id, c))).collect();
        for (owner, child) in ownership {
            let child_element = elements.get_mut(&child).ok_or(ModelError::NotFound(child))?;
            if child_element.owner.replace(owner).is_some() {
                return Err(ModelError::Invalid(format!("{child} is owned more than once")));
            }
        }
        let views = records
            .views
            .into_iter()
            .map(|(id, presentations)| (id, View { presentations }))
            .collect();
        let model = Self { root: records.root, elements, views };
        model.check()?;
        Ok(model)
    }
}
