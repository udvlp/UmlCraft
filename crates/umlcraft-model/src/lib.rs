//! UmlCraft's model core: UML elements, diagram presentations and undoable edits.
//!
//! - [`Model`] holds the ownership tree of [`ElementData`] plus one [`View`] per diagram.
//! - [`Op`] is a primitive, validated, invertible change; [`Model::apply`] returns the inverse.
//! - [`Document`] groups ops into undoable transactions via [`Tx`].

mod diagram;
mod document;
mod element;
pub mod er;
mod error;
mod geometry;
mod id;
mod model;
mod op;
pub mod sample;

pub use diagram::{
    Color, DiagramKind, EdgeLabels, LineShape, ParameterDirections, ParseColorError, Presentation,
    Regions, Shape, SharedStyle, Style, View,
};
pub use document::{Document, Tx};
pub use element::{
    ActivityEdgeProps, ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd,
    AssociationProps, AttributeProps, ClassifierProps, CombinedFragmentProps, CommentProps,
    ConnectorEnd, ConnectorProps, DiagramProps, DirectedProps, DurationConstraintProps,
    ElementData, ElementKind, ExtendProps, GeneralizationProps, IMPORTED_TAG, IncludeProps,
    InstanceKind, InstanceProps, InteractionOperandProps, InteractionOperator, InteractionUseProps,
    InternalTransitionProps, LifelineProps, LinkEnd, LinkProps, MessageGate, MessageProps,
    MessageSort, Multiplicity, Navigability, ObjectEffect, ObjectOrdering, OperationProps,
    ParameterDirection, ParameterProps, ParseMultiplicityError, PortProps, PseudostateKind,
    PseudostateProps, Qualifier, RefRole, SlotProps, StateInvariantProps, StateProps,
    StructogramProps, TaggedValue, TemplateParameterKind, TemplateParameterProps, TerminationProps,
    TimeConstraintProps, TransitionProps, TypeExpr, TypeRef, Visibility,
};
pub use er::{
    Cardinality, CardinalityMax, CardinalityMin, ErAttributeProps, ErEntityProps, ErIsAProps,
    ErKey, ErParticipationProps, ErRelationshipProps, ErSubtypingProps, ParseCardinalityError,
};
pub use error::{ModelError, Result};
pub use geometry::{Point, Rect, Size};
pub use id::Id;
pub use model::{Element, ElementRecord, Model, ModelRecords};
pub use op::Op;

/// Deepest type expression a model holds, e.g. 3 for `Map<String, List<Order>>`. JSON readers
/// stop at a nesting limit of their own (serde_json: 128, about 40 levels of types), and no real
/// type comes near either.
pub const MAX_TYPE_DEPTH: usize = 32;
