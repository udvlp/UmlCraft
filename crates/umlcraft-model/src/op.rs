use crate::diagram::Presentation;
use crate::element::ElementData;
use crate::id::Id;

/// A primitive, validated and invertible change to a [`Model`](crate::Model).
///
/// [`Model::apply`](crate::Model::apply) returns the inverse operation, which is what undo
/// replays. Higher-level edits (cascading delete, creating a connected edge, …) are sequences of
/// these, see [`Tx`](crate::Tx).
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    InsertElement {
        id: Id,
        owner: Id,
        index: usize,
        data: ElementData,
    },
    /// Removes a leaf element that nothing refers to and no diagram shows.
    RemoveElement {
        id: Id,
    },
    /// Replaces the data of an element; the metaclass must stay the same.
    UpdateElement {
        id: Id,
        data: ElementData,
    },
    MoveElement {
        id: Id,
        owner: Id,
        index: usize,
    },
    InsertPresentation {
        diagram: Id,
        index: usize,
        presentation: Presentation,
    },
    /// Removes a presentation that no edge, message or activation is attached to.
    RemovePresentation {
        diagram: Id,
        id: Id,
    },
    /// Removes several presentations of one diagram at once; nothing outside them may be
    /// attached to any of them. The list is compacted once, however many there are.
    RemovePresentations {
        diagram: Id,
        ids: Vec<Id>,
    },
    /// Puts presentations back where [`Op::RemovePresentations`] took them from: each at its
    /// index in the list once all are in, by ascending index.
    InsertPresentations {
        diagram: Id,
        entries: Vec<(usize, Presentation)>,
    },
    /// Replaces a presentation (bounds, waypoints, style); node stays node, edge stays edge.
    UpdatePresentation {
        diagram: Id,
        presentation: Presentation,
    },
    /// Changes the z-order of a presentation.
    ReorderPresentation {
        diagram: Id,
        id: Id,
        index: usize,
    },
}
