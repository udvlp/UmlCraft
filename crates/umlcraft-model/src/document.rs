use indexmap::IndexSet;

use crate::diagram::{Presentation, Shape};
use crate::element::ElementData;
use crate::error::{ModelError, Result};
use crate::geometry::{Point, Rect};
use crate::id::Id;
use crate::model::Model;
use crate::op::Op;

/// A recorded edit: the inverse operations in the order they were produced.
#[derive(Clone, Debug)]
struct Transaction {
    seq: u64,
    label: String,
    inverse: Vec<Op>,
}

/// A model with undo/redo history and dirty tracking.
#[derive(Clone, Debug)]
pub struct Document {
    model: Model,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    next_seq: u64,
    saved_seq: Option<u64>,
    revision: u64,
}

impl Document {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            undo: Vec::new(),
            redo: Vec::new(),
            next_seq: 1,
            saved_seq: None,
            revision: 0,
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Increases with every change, including undo and redo. Views use it to refresh caches.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether the model differs from the last saved (or initial) state.
    pub fn is_dirty(&self) -> bool {
        self.undo.last().map(|t| t.seq) != self.saved_seq
    }

    pub fn mark_saved(&mut self) {
        self.saved_seq = self.undo.last().map(|t| t.seq);
    }

    /// Forgets undo and redo history, e.g. after generating a start document.
    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.saved_seq = None;
    }

    /// Runs `f` as one undoable step. If `f` fails, every change it made is rolled back.
    pub fn edit<R>(
        &mut self,
        label: impl Into<String>,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R>,
    ) -> Result<R> {
        let mut tx = Tx { model: &mut self.model, inverse: Vec::new() };
        match f(&mut tx) {
            Ok(result) => {
                let inverse = tx.inverse;
                if !inverse.is_empty() {
                    self.undo.push(Transaction {
                        seq: self.next_seq,
                        label: label.into(),
                        inverse,
                    });
                    self.next_seq += 1;
                    self.redo.clear();
                    self.revision += 1;
                }
                Ok(result)
            }
            Err(err) => {
                let inverse = tx.inverse;
                revert(&mut self.model, inverse);
                Err(err)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|t| t.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|t| t.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        let Some(t) = self.undo.pop() else {
            return false;
        };
        let inverse = revert(&mut self.model, t.inverse);
        self.redo.push(Transaction { inverse, ..t });
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(t) = self.redo.pop() else {
            return false;
        };
        let inverse = revert(&mut self.model, t.inverse);
        self.undo.push(Transaction { inverse, ..t });
        self.revision += 1;
        true
    }
}

/// Applies recorded inverse operations in reverse order and returns their inverses, again in
/// application order, so the result can be reverted the same way.
fn revert(model: &mut Model, inverse: Vec<Op>) -> Vec<Op> {
    inverse
        .into_iter()
        .rev()
        .map(|op| model.apply(op).expect("inverse operations always apply"))
        .collect()
}

/// An open transaction. All edits go through here so they can be undone as one step.
pub struct Tx<'a> {
    model: &'a mut Model,
    inverse: Vec<Op>,
}

impl Tx<'_> {
    pub fn model(&self) -> &Model {
        self.model
    }

    pub fn apply(&mut self, op: Op) -> Result<()> {
        let inverse = self.model.apply(op)?;
        self.inverse.push(inverse);
        Ok(())
    }

    /// Creates an element as the last child of `owner`.
    pub fn create(&mut self, owner: Id, data: ElementData) -> Result<Id> {
        let index = self.model.element(owner)?.children().len();
        self.create_at(owner, index, data)
    }

    pub fn create_at(&mut self, owner: Id, index: usize, data: ElementData) -> Result<Id> {
        let id = Id::generate();
        self.apply(Op::InsertElement { id, owner, index, data })?;
        Ok(id)
    }

    /// Changes element data through a closure; does nothing if nothing changed.
    pub fn update(&mut self, id: Id, f: impl FnOnce(&mut ElementData)) -> Result<()> {
        let mut data = self.model.element(id)?.data().clone();
        f(&mut data);
        if data != *self.model.element(id)?.data() {
            self.apply(Op::UpdateElement { id, data })?;
        }
        Ok(())
    }

    pub fn rename(&mut self, id: Id, name: impl Into<String>) -> Result<()> {
        let name = name.into();
        self.update(id, |data| data.name = name)
    }

    pub fn move_element(&mut self, id: Id, owner: Id, index: usize) -> Result<()> {
        self.apply(Op::MoveElement { id, owner, index })
    }

    /// Deletes an element from the model, together with
    /// - everything it owns,
    /// - every element that refers to it strongly (e.g. relationships, messages),
    /// - all their presentations on any diagram, and edges attached to those.
    ///
    /// Weak references from surviving elements are unlinked (see
    /// [`ElementKind::unlink`](crate::ElementKind::unlink)).
    pub fn delete(&mut self, id: Id) -> Result<()> {
        if id == self.model.root() {
            return Err(ModelError::Root);
        }
        self.model.element(id)?;
        let doomed = self.deletion_closure(id);

        let diagrams: Vec<Id> = self.model.views().map(|(d, _)| d).collect();
        for diagram in diagrams {
            let view = self.model.view(diagram).expect("listed above");
            let gone: IndexSet<Id> = view
                .presentations()
                .iter()
                .filter(|p| {
                    doomed.contains(&diagram) || p.element.is_some_and(|e| doomed.contains(&e))
                })
                .map(|p| p.id)
                .collect();
            if !gone.is_empty() {
                self.remove_presentations(diagram, gone)?;
            }
        }

        let survivors_to_unlink: Vec<Id> = self
            .model
            .elements()
            .filter(|e| !doomed.contains(&e.id()))
            .filter(|e| e.kind().references().iter().any(|(t, _)| doomed.contains(t)))
            .map(|e| e.id())
            .collect();
        for survivor in survivors_to_unlink {
            self.unlink_all(survivor, &doomed)?;
        }

        let mut remaining = doomed;
        while !remaining.is_empty() {
            let removable = remaining.iter().copied().find(|&candidate| {
                self.model.children(candidate).is_empty()
                    && !remaining.iter().any(|&other| {
                        self.model.get(other).is_some_and(|e| {
                            e.kind().references().iter().any(|&(t, _)| t == candidate)
                        })
                    })
            });
            match removable {
                Some(candidate) => {
                    self.apply(Op::RemoveElement { id: candidate })?;
                    remaining.shift_remove(&candidate);
                }
                None => {
                    // Only weak reference cycles inside the deleted set can block; break them.
                    let mut progressed = false;
                    for other in remaining.clone() {
                        let before = self.inverse.len();
                        self.unlink_all(other, &remaining)?;
                        progressed |= self.inverse.len() != before;
                    }
                    if !progressed {
                        return Err(ModelError::Invalid(format!(
                            "cannot delete {id}: reference cycle"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    /// The subtree of `id` plus, transitively, everything strongly referring into the set.
    fn deletion_closure(&self, id: Id) -> IndexSet<Id> {
        let mut doomed: IndexSet<Id> = self.model.subtree(id).into_iter().collect();
        loop {
            let extra: Vec<Id> = self
                .model
                .elements()
                .filter(|e| !doomed.contains(&e.id()))
                .filter(|e| {
                    e.kind()
                        .references()
                        .iter()
                        .any(|&(t, role)| role.is_strong() && doomed.contains(&t))
                })
                .flat_map(|e| self.model.subtree(e.id()))
                .collect();
            if extra.is_empty() {
                return doomed;
            }
            doomed.extend(extra);
        }
    }

    fn unlink_all(&mut self, element: Id, targets: &IndexSet<Id>) -> Result<()> {
        let names: Vec<(Id, String)> = targets
            .iter()
            .filter_map(|&t| self.model.get(t).map(|e| (t, e.name().to_owned())))
            .collect();
        self.update(element, |data| {
            for (target, name) in &names {
                data.kind.unlink(*target, name);
            }
        })
    }

    /// Adds a node presentation on top of `diagram`.
    pub fn add_node(&mut self, diagram: Id, element: Option<Id>, bounds: Rect) -> Result<Id> {
        self.add_presentation(diagram, Presentation::node(element, bounds))
    }

    /// Adds an edge between two presentations of `diagram`.
    pub fn add_edge(
        &mut self,
        diagram: Id,
        element: Option<Id>,
        source: Id,
        target: Id,
        waypoints: Vec<Point>,
    ) -> Result<Id> {
        self.add_presentation(diagram, Presentation::edge(element, source, target, waypoints))
    }

    pub fn add_presentation(&mut self, diagram: Id, presentation: Presentation) -> Result<Id> {
        let index =
            self.model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?.presentations().len();
        let id = presentation.id;
        self.apply(Op::InsertPresentation { diagram, index, presentation })?;
        Ok(id)
    }

    pub fn update_presentation(
        &mut self,
        diagram: Id,
        id: Id,
        f: impl FnOnce(&mut Presentation),
    ) -> Result<()> {
        let view = self.model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let old = view.get(id).ok_or(ModelError::NotFound(id))?;
        let mut presentation = old.clone();
        f(&mut presentation);
        if presentation != *old {
            self.apply(Op::UpdatePresentation { diagram, presentation })?;
        }
        Ok(())
    }

    /// Moves or resizes a node; for a lifeline, its head.
    pub fn set_bounds(&mut self, diagram: Id, id: Id, bounds: Rect) -> Result<()> {
        self.update_presentation(diagram, id, |p| match &mut p.shape {
            Shape::Node { bounds: b }
            | Shape::Lifeline { head: b, .. }
            | Shape::Fragment { bounds: b, .. } => *b = bounds,
            _ => {}
        })
    }

    pub fn set_waypoints(&mut self, diagram: Id, id: Id, points: Vec<Point>) -> Result<()> {
        self.update_presentation(diagram, id, |p| {
            if let Shape::Edge { waypoints, .. } = &mut p.shape {
                *waypoints = points;
            }
        })
    }

    /// Sets an edge's waypoints and where it aims inside its end nodes.
    pub fn set_route(
        &mut self,
        diagram: Id,
        id: Id,
        points: Vec<Point>,
        anchors: (Option<Point>, Option<Point>),
    ) -> Result<()> {
        self.update_presentation(diagram, id, |p| {
            if let Shape::Edge { waypoints, source_anchor, target_anchor, .. } = &mut p.shape {
                *waypoints = points;
                (*source_anchor, *target_anchor) = anchors;
            }
        })
    }

    pub fn bring_to_front(&mut self, diagram: Id, id: Id) -> Result<()> {
        let view = self.model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let last = view.presentations().len().saturating_sub(1);
        if view.presentations().last().is_some_and(|p| p.id == id) {
            return Ok(());
        }
        self.apply(Op::ReorderPresentation { diagram, id, index: last })
    }

    /// Removes a presentation from its diagram ("delete from diagram"), together with all edges
    /// attached to it. The model elements stay.
    pub fn remove_presentation(&mut self, diagram: Id, id: Id) -> Result<()> {
        self.remove_presentations(diagram, IndexSet::from([id]))
    }

    /// Removes the given presentations and, transitively, all edges attached to them.
    pub fn remove_presentations(&mut self, diagram: Id, ids: IndexSet<Id>) -> Result<()> {
        let view = self.model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let mut gone = ids;
        loop {
            let extra: Vec<Id> = view
                .presentations()
                .iter()
                .filter(|p| !gone.contains(&p.id) && gone.iter().any(|&g| p.is_attached_to(g)))
                .map(|p| p.id)
                .collect();
            if extra.is_empty() {
                break;
            }
            gone.extend(extra);
        }
        while !gone.is_empty() {
            let view = self.model.view(diagram).expect("checked above");
            let next = gone
                .iter()
                .copied()
                .find(|&id| view.edges_attached_to(id).next().is_none())
                .ok_or_else(|| ModelError::Invalid("edges attached in a cycle".into()))?;
            self.apply(Op::RemovePresentation { diagram, id: next })?;
            gone.shift_remove(&next);
        }
        Ok(())
    }
}
