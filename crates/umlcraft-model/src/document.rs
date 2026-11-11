use std::collections::{HashMap, HashSet, VecDeque};

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

    /// Marks the document as never saved, e.g. after an import: it stays dirty until saved,
    /// whatever is undone or redone.
    pub fn mark_unsaved(&mut self) {
        // Sequence numbers start at 1, so no history state ever matches 0.
        self.saved_seq = Some(0);
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
        // A panicking edit is taken back too before the panic goes on, so the model never
        // keeps changes that no undo step records.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&mut tx)));
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(panic) => {
                let inverse = std::mem::take(&mut tx.inverse);
                revert(&mut self.model, inverse);
                std::panic::resume_unwind(panic);
            }
        };
        match outcome {
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

    /// How many steps can be undone; a mark for [`Document::squash_since`] and
    /// [`Document::roll_back_to`].
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Makes the steps done since the history was `depth` deep one step called `label`.
    pub fn squash_since(&mut self, depth: usize, label: impl Into<String>) {
        if self.undo.len() <= depth {
            return;
        }
        let steps = self.undo.split_off(depth);
        let seq = steps.last().map_or(self.next_seq, |t| t.seq);
        let inverse = steps.into_iter().flat_map(|t| t.inverse).collect();
        self.undo.push(Transaction { seq, label: label.into(), inverse });
    }

    /// Takes back the steps done since the history was `depth` deep and forgets them; they
    /// cannot be redone.
    pub fn roll_back_to(&mut self, depth: usize) {
        while self.undo.len() > depth {
            let t = self.undo.pop().expect("deeper than depth");
            revert(&mut self.model, t.inverse);
            self.revision += 1;
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
        .map(|op| model.apply_recorded(op).expect("inverse operations always apply"))
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

    /// Runs `f` as a part of this transaction that is taken back on its own when it fails: the
    /// error is returned and the model is as it was before, while the rest of the transaction
    /// goes on.
    pub fn attempt<R>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<R>) -> Result<R> {
        let mark = self.inverse.len();
        let result = f(self);
        if result.is_err() {
            let tail = self.inverse.split_off(mark);
            revert(self.model, tail);
        }
        result
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

    /// Replaces the content of structogram `id` with `structogram` in its normal form. A
    /// structogram changed this way is someone's own: it loses [`crate::IMPORTED_TAG`], so that
    /// a new source import leaves it alone.
    pub fn set_structogram(&mut self, id: Id, structogram: &umlcraft_nsd::Diagram) -> Result<()> {
        let structogram = umlcraft_nsd::content::normalize(structogram);
        self.update(id, |data| {
            if let crate::ElementKind::Structogram(p) = &mut data.kind {
                p.structogram = structogram;
            }
            data.tagged_values.retain(|t| t.key != crate::IMPORTED_TAG);
        })
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
        self.delete_many(&[id])
    }

    /// Deletes several elements as [`Tx::delete`] does, looking through the model once for all
    /// of them instead of once each. Ids inside the subtree of another are fine.
    pub fn delete_many(&mut self, ids: &[Id]) -> Result<()> {
        for &id in ids {
            if id == self.model.root() {
                return Err(ModelError::Root);
            }
            self.model.element(id)?;
        }
        if ids.is_empty() {
            return Ok(());
        }
        let doomed = self.deletion_closure(ids);

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
            .filter(|e| {
                let mut hit = false;
                e.kind().visit_references(&mut |t, _| hit |= doomed.contains(&t));
                hit
            })
            .map(|e| e.id())
            .collect();
        for survivor in survivors_to_unlink {
            self.unlink_all(survivor, &doomed)?;
        }

        // Leaves first, and nothing another doomed element still refers to.
        let mut remaining = doomed;
        while !remaining.is_empty() {
            let referenced: HashSet<Id> = remaining
                .iter()
                .filter_map(|&other| self.model.get(other))
                .flat_map(|e| e.kind().references())
                .map(|(t, _)| t)
                .collect();
            let removable: Vec<Id> = remaining
                .iter()
                .copied()
                .filter(|&candidate| {
                    self.model.children(candidate).is_empty() && !referenced.contains(&candidate)
                })
                .collect();
            if removable.is_empty() {
                // Only weak reference cycles inside the deleted set can block; break them.
                let mut progressed = false;
                for other in remaining.clone() {
                    let before = self.inverse.len();
                    self.unlink_all(other, &remaining)?;
                    progressed |= self.inverse.len() != before;
                }
                if !progressed {
                    return Err(ModelError::Invalid(format!(
                        "cannot delete {}: reference cycle",
                        ids[0]
                    )));
                }
                continue;
            }
            // Nothing outside refers to the doomed elements any more, and nothing inside to
            // these; no diagram shows them.
            for &candidate in &removable {
                let inverse = self.model.remove_unreferenced(candidate)?;
                self.inverse.push(inverse);
            }
            let removed: HashSet<Id> = removable.into_iter().collect();
            remaining.retain(|id| !removed.contains(id));
        }
        Ok(())
    }

    /// The subtrees of `ids` plus, transitively, everything strongly referring into the set.
    fn deletion_closure(&self, ids: &[Id]) -> IndexSet<Id> {
        // target → the elements referring to it strongly, gathered in one pass.
        let mut strong: HashMap<Id, Vec<Id>> = HashMap::new();
        for e in self.model.elements() {
            e.kind().visit_references(&mut |t, role| {
                if role.is_strong() {
                    strong.entry(t).or_default().push(e.id());
                }
            });
        }
        let mut doomed: IndexSet<Id> = IndexSet::new();
        for &id in ids {
            if !doomed.contains(&id) {
                doomed.extend(self.model.subtree(id));
            }
        }
        let mut i = 0;
        while i < doomed.len() {
            for &referrer in strong.get(&doomed[i]).into_iter().flatten() {
                if !doomed.contains(&referrer) {
                    doomed.extend(self.model.subtree(referrer));
                }
            }
            i += 1;
        }
        doomed
    }

    /// Unlinks the weak references of `element` to any of `targets`.
    fn unlink_all(&mut self, element: Id, targets: &IndexSet<Id>) -> Result<()> {
        let mut hit = Vec::new();
        if let Some(e) = self.model.get(element) {
            e.kind().visit_references(&mut |t, _| {
                if targets.contains(&t) && !hit.contains(&t) {
                    hit.push(t);
                }
            });
        }
        if hit.is_empty() {
            return Ok(());
        }
        let names: Vec<(Id, String)> = hit
            .into_iter()
            .filter_map(|t| self.model.get(t).map(|e| (t, e.name().to_owned())))
            .collect();
        self.update(element, |data| {
            for (target, name) in &names {
                data.kind.unlink(*target, name);
            }
        })
    }

    /// The view of `diagram`: `NotFound` if there is no such element, `NotADiagram` if it is
    /// one of another kind.
    fn view_of(&self, diagram: Id) -> Result<&crate::View> {
        match self.model.view(diagram) {
            Some(view) => Ok(view),
            None if self.model.contains(diagram) => Err(ModelError::NotADiagram(diagram)),
            None => Err(ModelError::NotFound(diagram)),
        }
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
        let index = self.view_of(diagram)?.presentations().len();
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
        let view = self.view_of(diagram)?;
        let old = view.get(id).ok_or(ModelError::NotFound(id))?;
        let mut presentation = old.clone();
        f(&mut presentation);
        // The closure changes the presentation, not which one it is.
        presentation.id = id;
        if presentation != *old {
            self.apply(Op::UpdatePresentation { diagram, presentation })?;
        }
        Ok(())
    }

    /// Moves or resizes a node or free text; for a lifeline, its head. The lines between a
    /// node's regions are measured from its left (or top) edge: where that edge moves while the
    /// node changes size, resized or grown that way, they stay where they are on the diagram.
    pub fn set_bounds(&mut self, diagram: Id, id: Id, bounds: Rect) -> Result<()> {
        self.update_presentation(diagram, id, |p| match &mut p.shape {
            Shape::Node { bounds: b, regions, .. } => {
                if let Some(regions) = regions {
                    let (shift, resized) = if regions.vertical {
                        (b.x - bounds.x, b.width != bounds.width)
                    } else {
                        (b.y - bounds.y, b.height != bounds.height)
                    };
                    if resized && shift != 0.0 {
                        regions.separators.iter_mut().for_each(|s| *s += shift);
                    }
                }
                *b = bounds;
            }
            Shape::Lifeline { head: b, .. }
            | Shape::Fragment { bounds: b, .. }
            | Shape::Text { bounds: b, .. } => *b = bounds,
            _ => {}
        })
    }

    /// Moves a free line by (`dx`, `dy`).
    pub fn move_line(&mut self, diagram: Id, id: Id, dx: f32, dy: f32) -> Result<()> {
        self.update_presentation(diagram, id, |p| {
            if let Shape::Line { from, to, .. } = &mut p.shape {
                (*from, *to) = (from.offset(dx, dy), to.offset(dx, dy));
            }
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
        let view = self.view_of(diagram)?;
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
        let view = self.view_of(diagram)?;
        // What each presentation hangs on: an edge's or message's ends, an activation's lifeline.
        let holds = |p: &Presentation| -> [Option<Id>; 2] {
            match p.shape {
                Shape::Activation { lifeline, .. } => [Some(lifeline), None],
                _ => match p.endpoints() {
                    Some((s, t)) if s == t => [Some(s), None],
                    Some((s, t)) => [Some(s), Some(t)],
                    None => [None, None],
                },
            }
        };
        let mut hanging: HashMap<Id, Vec<Id>> = HashMap::new();
        let mut holding: HashMap<Id, [Option<Id>; 2]> = HashMap::new();
        for p in view.presentations() {
            let held = holds(p);
            for &on in held.iter().flatten() {
                hanging.entry(on).or_default().push(p.id);
            }
            holding.insert(p.id, held);
        }
        // Whatever hangs on a removed presentation goes with it.
        let mut gone = ids;
        let mut i = 0;
        while i < gone.len() {
            let extra = hanging.get(&gone[i]).cloned().unwrap_or_default();
            gone.extend(extra);
            i += 1;
        }
        // Then remove from the outside in: a presentation once nothing hangs on it any more.
        let mut attached: HashMap<Id, usize> =
            gone.iter().map(|&id| (id, hanging.get(&id).map_or(0, Vec::len))).collect();
        let mut ready: VecDeque<Id> = gone.iter().copied().filter(|id| attached[id] == 0).collect();
        let mut removed = 0;
        while let Some(next) = ready.pop_front() {
            self.apply(Op::RemovePresentation { diagram, id: next })?;
            removed += 1;
            for &on in holding.get(&next).into_iter().flatten().flatten() {
                if let Some(count) = attached.get_mut(&on) {
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(on);
                    }
                }
            }
        }
        if removed < gone.len() {
            return Err(ModelError::Invalid("edges attached in a cycle".into()));
        }
        Ok(())
    }
}
