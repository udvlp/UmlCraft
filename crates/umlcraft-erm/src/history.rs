//! A diagram being edited: the selection, undo and redo over snapshots of the whole diagram
//! (ER diagrams are small), and whether it differs from what was saved last.

use crate::edit::{EditError, Item};
use crate::model::Diagram;

/// How many steps undo keeps.
const LIMIT: usize = 200;

pub struct Editor {
    diagram: Diagram,
    selection: Vec<Item>,
    undo: Vec<(Diagram, Vec<Item>)>,
    redo: Vec<(Diagram, Vec<Item>)>,
    /// The diagram as saved or loaded last; `None` for a new one never saved.
    saved: Option<Diagram>,
}

impl Editor {
    /// `saved`: whether the diagram is what a file holds (loaded), or new.
    pub fn new(diagram: Diagram, saved: bool) -> Editor {
        Editor {
            saved: saved.then(|| diagram.clone()),
            diagram,
            selection: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    pub fn selection(&self) -> &[Item] {
        &self.selection
    }

    /// Selects without an undo step; items that do not exist and repeats are left out.
    pub fn select(&mut self, items: Vec<Item>) {
        let mut selection: Vec<Item> = Vec::with_capacity(items.len());
        for item in items {
            if item.exists(&self.diagram) && !selection.contains(&item) {
                selection.push(item);
            }
        }
        self.selection = selection;
    }

    /// Makes one undoable change; `change` returns what to select afterwards (`None`: keep
    /// the selection, less what no longer exists). A change that fails, or changes nothing,
    /// leaves no step.
    pub fn apply(
        &mut self,
        change: impl FnOnce(&mut Diagram) -> Result<Option<Vec<Item>>, EditError>,
    ) -> Result<(), EditError> {
        let mut changed = self.diagram.clone();
        let selection = change(&mut changed)?;
        if changed != self.diagram {
            let before = std::mem::replace(&mut self.diagram, changed);
            self.undo.push((before, self.selection.clone()));
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        let selection = selection.unwrap_or_else(|| self.selection.clone());
        self.select(selection);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some((diagram, selection)) = self.undo.pop() else { return false };
        let current = std::mem::replace(&mut self.diagram, diagram);
        self.redo.push((current, std::mem::replace(&mut self.selection, selection)));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some((diagram, selection)) = self.redo.pop() else { return false };
        let current = std::mem::replace(&mut self.diagram, diagram);
        self.undo.push((current, std::mem::replace(&mut self.selection, selection)));
        true
    }

    /// Whether the diagram differs from the file (always for a new one).
    pub fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(&self.diagram)
    }

    /// Whether the diagram was never saved.
    pub fn is_new(&self) -> bool {
        self.saved.is_none()
    }

    /// The diagram to write: as loaded when it is unchanged (so the file stays byte for
    /// byte), otherwise stamped: `modified` is `now`, a new diagram also gets `created` and,
    /// if it has none, `user` as author. Call [`Editor::saved`] with it once it is written.
    pub fn to_save(&self, user: &str, now: &str) -> Diagram {
        let mut d = self.diagram.clone();
        if self.saved.as_ref() != Some(&d) {
            d.meta.modified = Some(now.to_owned());
            if self.saved.is_none() && d.meta.created.is_none() {
                d.meta.created = Some(now.to_owned());
            }
            if d.meta.author.as_deref().is_none_or(str::is_empty) && !user.is_empty() {
                d.meta.author = Some(user.to_owned());
            }
        }
        d
    }

    /// Takes `written` (from [`Editor::to_save`]) as the diagram and as saved. The stamp is no
    /// undo step of its own.
    pub fn saved(&mut self, written: Diagram) {
        self.diagram = written.clone();
        self.saved = Some(written);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit;
    use crate::model::{NodeKind, Point};

    fn add(e: &mut Editor, name: &str) -> crate::Id {
        let mut id = None;
        e.apply(|d| {
            let n = edit::add_node(d, NodeKind::entity(), name, Point::new(0.0, 0.0));
            id = Some(n.clone());
            Ok(Some(vec![Item::Node(n)]))
        })
        .unwrap();
        id.unwrap()
    }

    #[test]
    fn undo_and_redo_restore_diagram_and_selection() {
        let mut e = Editor::new(Diagram::new("t"), true);
        let a = add(&mut e, "A");
        let b = add(&mut e, "B");
        assert_eq!(e.selection(), [Item::Node(b.clone())]);
        assert!(e.undo());
        assert_eq!(e.diagram().nodes.len(), 1);
        assert_eq!(e.selection(), [Item::Node(a)]);
        assert!(e.redo());
        assert_eq!(e.selection(), [Item::Node(b)]);
        assert!(!e.redo());
        assert!(e.undo() && e.undo() && !e.undo());
        assert!(!e.is_dirty(), "back at the loaded state");
    }

    #[test]
    fn refused_and_empty_changes_leave_no_step() {
        let mut e = Editor::new(Diagram::new("t"), true);
        assert!(e.apply(|_| Err(EditError::NotApplicable)).is_err());
        e.apply(|_| Ok(None)).unwrap();
        assert!(!e.can_undo());
    }

    #[test]
    fn selection_drops_what_no_longer_exists() {
        let mut e = Editor::new(Diagram::new("t"), true);
        let a = add(&mut e, "A");
        e.apply(|d| edit::delete(d, &[Item::Node(a.clone())]).map(|()| None)).unwrap();
        assert!(e.selection().is_empty());
        e.select(vec![Item::Node(a)]);
        assert!(e.selection().is_empty());
    }

    #[test]
    fn saving_stamps_changes_only() {
        let mut e = Editor::new(Diagram::new("t"), true);
        assert_eq!(e.to_save("ann", "2026-11-02T10:00:00"), *e.diagram());
        add(&mut e, "A");
        let d = e.to_save("ann", "2026-11-02T10:00:00");
        assert_eq!(d.meta.modified.as_deref(), Some("2026-11-02T10:00:00"));
        assert_eq!(d.meta.created, None, "loaded diagrams keep their creation date");
        assert_eq!(d.meta.author.as_deref(), Some("ann"));
        e.saved(d);
        assert!(!e.is_dirty() && e.can_undo());

        let mut fresh = Editor::new(Diagram::new("neu"), false);
        assert!(fresh.is_dirty() && fresh.is_new());
        let d = fresh.to_save("bo", "2026-11-02T11:00:00");
        assert_eq!(d.meta.created.as_deref(), Some("2026-11-02T11:00:00"));
        fresh.saved(d);
        assert!(!fresh.is_dirty() && !fresh.is_new());
    }
}
