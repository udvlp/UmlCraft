use crate::id::Id;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("no element or presentation {0}")]
    NotFound(Id),
    #[error("id {0} is already in use")]
    DuplicateId(Id),
    #[error("{child} ({child_type}) cannot be owned by {owner} ({owner_type})")]
    InvalidOwner { owner: Id, owner_type: &'static str, child: Id, child_type: &'static str },
    #[error("index {index} out of range")]
    IndexOutOfRange { index: usize },
    #[error("the model root cannot be removed, moved or created")]
    Root,
    #[error("{0} still owns elements")]
    HasChildren(Id),
    #[error("{target} is still referenced by {referrer}")]
    StillReferenced { target: Id, referrer: Id },
    #[error("{target} is still shown on diagram {diagram}")]
    StillShown { target: Id, diagram: Id },
    #[error("{from} refers to {to}, which does not exist or has the wrong type")]
    InvalidReference { from: Id, to: Id },
    #[error("the metaclass of {0} cannot change")]
    KindChanged(Id),
    #[error("moving {0} there would create an ownership cycle")]
    OwnershipCycle(Id),
    #[error("generalization {0} would create a cycle")]
    GeneralizationCycle(Id),
    #[error("{0} is not a diagram")]
    NotADiagram(Id),
    #[error("diagram {0} still has presentations")]
    DiagramNotEmpty(Id),
    #[error("edges are still attached to presentation {0}")]
    EdgesAttached(Id),
    #[error("presentation {0} cannot change between node and edge")]
    ShapeChanged(Id),
    #[error("edge {0} would end on itself through other edges")]
    EdgeCycle(Id),
    #[error("relationship {0} would connect itself through other relationships")]
    ReferenceCycle(Id),
    #[error("presentation {0} has a position or size that is not a finite number")]
    NotFinite(Id),
    #[error("a type of {0} is nested more than {max} levels deep", max = crate::MAX_TYPE_DEPTH)]
    TypeTooDeep(Id),
    #[error(
        "structogram {0} nests elements more than {max} levels deep",
        max = umlcraft_nsd::read::MAX_DEPTH
    )]
    StructogramTooDeep(Id),
    #[error("{0}")]
    Refused(Refusal),
    #[error("invalid model: {0}")]
    Invalid(String),
}

/// Why an edit the user asked for was refused, as opposed to a check of the model's own
/// consistency (`ModelError::Invalid`). The texts are English for AI clients and logs; the app
/// tells each in the user's language.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("the element is already on this diagram")]
    AlreadyShown,
    /// A lifeline or fragment of another interaction dropped on a sequence diagram.
    #[error("the element belongs to another interaction")]
    OtherInteraction,
    /// An element of this kind (its type name) on a diagram that does not show such.
    #[error("a {0} cannot be shown on this diagram")]
    CannotShow(&'static str),
    /// A line drawn with `tool` between elements of these kinds.
    #[error("{tool} cannot connect {from} to {to}")]
    CannotConnect { tool: &'static str, from: &'static str, to: &'static str },
    #[error("cannot paste a {0} here")]
    CannotPaste(&'static str),
    /// A connector from a part or port to itself.
    #[error("a connector needs two different ends")]
    ConnectorSameEnds,
    /// A connector between parts of different classes.
    #[error("no class holds both ends of the connector")]
    ConnectorNoClass,
    #[error("a connector cannot connect a {0}")]
    ConnectorCannot(&'static str),
    /// A second create message to one lifeline.
    #[error("the lifeline is created already")]
    CreatedAlready,
    /// No height between the lifelines' ends where the message fits.
    #[error("no room for the message on these lifelines")]
    NoRoom,
    /// A message drawn to something that is no lifeline.
    #[error("messages connect lifelines")]
    MessagesConnectLifelines,
    /// Arranging an activity whose partitions lie one below the other.
    #[error("partitions one below the other")]
    PartitionsStacked,
    #[error("sequence diagrams are not arranged")]
    SequenceNotArranged,
    #[error("the name must not be empty")]
    EmptyName,
    /// A bound class whose binding names no template.
    #[error("a binding names a template and its arguments")]
    BindingWithoutTemplate,
    /// Converting an association that has an association class already.
    #[error("the association has an association class")]
    HasAssociationClass,
    /// A member typed without a name; the text typed.
    #[error("no valid name in {0:?}")]
    MemberNoName(String),
    #[error("invalid multiplicity {0:?}")]
    MemberMultiplicity(String),
    /// An unclosed bracket or quote in the text typed.
    #[error("missing \"{missing}\" in {text:?}")]
    MemberMissing { missing: char, text: String },
    /// What follows the closing parenthesis of an operation.
    #[error("unexpected {0:?} after \")\"")]
    MemberUnexpected(String),
}

pub type Result<T, E = ModelError> = std::result::Result<T, E>;
