//! UmlCraft's model core: UML elements, diagram presentations and undoable edits.
//!
//! - [`Model`] holds the ownership tree of [`ElementData`] plus one [`View`] per diagram.
//! - [`Op`] is a primitive, validated, invertible change; [`Model::apply`] returns the inverse.
//! - [`Document`] groups ops into undoable transactions via [`Tx`].

mod diagram;
mod document;
mod element;
mod error;
mod geometry;
mod id;
mod model;
mod op;
pub mod sample;

pub use diagram::{
    Color, DiagramKind, EdgeLabels, LineShape, ParseColorError, Presentation, Shape, Style, View,
};
pub use document::{Document, Tx};
pub use element::{
    ActivityEdgeProps, ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd,
    AssociationProps, AttributeProps, ClassifierProps, CombinedFragmentProps, CommentProps,
    DiagramProps, DirectedProps, ElementData, ElementKind, ExtendProps, GeneralizationProps,
    IncludeProps, InstanceKind, InstanceProps, InteractionOperandProps, InteractionOperator,
    InteractionUseProps, LifelineProps, LinkEnd, LinkProps, MessageGate, MessageProps, MessageSort,
    Multiplicity, Navigability, OperationProps, ParameterDirection, ParameterProps,
    ParseMultiplicityError, PseudostateKind, PseudostateProps, Qualifier, RefRole, SlotProps,
    StateProps, TaggedValue, TemplateParameterKind, TemplateParameterProps, TransitionProps,
    TypeExpr, TypeRef, Visibility,
};
pub use error::{ModelError, Result};
pub use geometry::{Point, Rect, Size};
pub use id::Id;
pub use model::{Element, ElementRecord, Model, ModelRecords};
pub use op::Op;
