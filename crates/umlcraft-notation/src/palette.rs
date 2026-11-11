//! Diagram tools: which elements a diagram offers, which connections are allowed, and the edits
//! that create them.

use umlcraft_model::{
    ActivityNodeKind, ClassifierProps, CombinedFragmentProps, CommentProps, ConnectorEnd,
    DiagramKind, DiagramProps, DirectedProps, DurationConstraintProps, ElementData, ElementKind,
    ErAttributeProps, ErEntityProps, ErIsAProps, ErKey, ErRelationshipProps, ExtendProps, Id,
    IncludeProps, InstanceKind, InstanceProps, InteractionOperandProps, InteractionOperator,
    InteractionUseProps, LifelineProps, MessageGate, MessageSort, Model, ModelError, Point,
    PseudostateKind, Rect, Result, Shape, Size, StateInvariantProps, StateProps, TerminationProps,
    TimeConstraintProps, Tx, TypeRef, View, Visibility,
};

use crate::sequence::{self, Slot};
use crate::sequence_edit;

/// The stereotype that makes a package a subsystem.
pub const SUBSYSTEM: &str = "subsystem";

/// Whether `e` is a subsystem: a package marked «subsystem».
pub fn is_subsystem(e: &umlcraft_model::Element) -> bool {
    matches!(e.kind(), ElementKind::Package) && e.data().stereotypes.iter().any(|s| s == SUBSYSTEM)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeTool {
    Class,
    /// Class diagrams: a class «boundary» drawn as Astah's robustness icon.
    Boundary,
    /// Class diagrams: a class «control» drawn as Astah's robustness icon.
    Control,
    /// Class diagrams: a class «entity» drawn as Astah's robustness icon.
    Entity,
    Interface,
    Enumeration,
    Package,
    /// A package marked «subsystem», drawn with Astah's fork icon in its tab.
    Subsystem,
    Note,
    Actor,
    UseCase,
    Lifeline,
    SystemBoundary,
    CombinedFragment,
    InteractionUse,
    /// Sequence diagrams: a condition on the lifeline clicked, at that height.
    StateInvariant,
    /// Sequence diagrams: a cross ending the lifeline clicked, at that height.
    Termination,
    Action,
    InitialNode,
    ActivityFinal,
    FlowFinal,
    Decision,
    Merge,
    Fork,
    Join,
    ObjectNode,
    Partition,
    /// Activity diagrams: an action calling another activity.
    CallBehavior,
    SendSignal,
    AcceptEvent,
    AcceptTimeEvent,
    /// Activity diagrams: a circle continuing a flow elsewhere.
    ActivityConnector,
    /// Activity diagrams: pins on the border of the action they are dropped on.
    InputPin,
    OutputPin,
    /// Activity diagrams: an activity parameter node on the frame.
    ActivityParameter,
    State,
    /// State machine diagrams: a state standing for another state machine.
    SubmachineState,
    FinalState,
    InitialPseudostate,
    Choice,
    Junction,
    ForkPseudostate,
    JoinPseudostate,
    ShallowHistory,
    DeepHistory,
    /// State machine diagrams: entry and exit points on the border of the state they are
    /// dropped on, or of the frame.
    EntryPoint,
    ExitPoint,
    /// State machine diagrams: a stub state in the submachine state it is dropped on.
    StubState,
    Component,
    Artifact,
    /// A deployment node; `Node` would be ambiguous among diagram tools.
    DeploymentNode,
    ComponentInstance,
    NodeInstance,
    Object,
    /// Composite structure diagrams: a part of the class drawn around it, an attribute.
    Part,
    /// Composite structure diagrams: a port on the border of a class or a part.
    Port,
    /// ER diagrams (Chen notation).
    ErEntity,
    ErWeakEntity,
    ErAssociativeEntity,
    ErAttribute,
    ErKeyAttribute,
    ErPartialKeyAttribute,
    ErMultivaluedAttribute,
    ErDerivedAttribute,
    ErRelationship,
    ErIdentifyingRelationship,
    ErIsA,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeTool {
    Association,
    /// An association with an association class, drawn with a dashed line to its middle.
    AssociationClass,
    Link,
    Generalization,
    Realization,
    Dependency,
    Include,
    Extend,
    NoteAnchor,
    Message,
    AsyncMessage,
    Reply,
    CreateMessage,
    DestroyMessage,
    FoundMessage,
    LostMessage,
    Flow,
    Transition,
    /// Composite structure diagrams: between parts and ports.
    Connector,
    /// Sequence diagrams: the time between two messages.
    DurationConstraint,
    /// Sequence diagrams: when the message clicked arrives.
    TimeConstraint,
    /// ER diagrams: one line for everything; what it becomes follows from its ends (see
    /// [`crate::er::connect`]).
    ErLine,
    /// ER diagrams in IE and IDEF1X: relationships from the parent to the child, which gets
    /// the parent's primary key as foreign key columns (in its own key when identifying), and
    /// many-to-many ones without (see [`crate::er::relate`]).
    ErIdentifying,
    ErNonIdentifying,
    ErManyToMany,
    /// ER diagrams in IE and IDEF1X: from a supertype to a subtype.
    ErSubtype,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Node(NodeTool),
    Edge(EdgeTool),
    /// A free text, not tied to a model element.
    Text,
    /// A free line, drawn from where the drag starts to where it ends.
    Line,
}

impl Tool {
    /// Stable key, e.g. for translations (`tool.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Node(n) => n.key(),
            Self::Edge(e) => e.key(),
            Self::Text => "text",
            Self::Line => "line",
        }
    }
}

/// Tools offered for a diagram kind, in palette order.
pub fn palette(kind: DiagramKind) -> Vec<Tool> {
    use EdgeTool as E;
    use NodeTool as N;
    let (nodes, edges): (&[NodeTool], &[EdgeTool]) = match kind {
        DiagramKind::Class => (
            &[
                N::Class,
                N::Interface,
                N::Enumeration,
                N::Package,
                N::Subsystem,
                N::Boundary,
                N::Control,
                N::Entity,
                N::Object,
                N::Note,
            ],
            &[
                E::Association,
                E::AssociationClass,
                E::Generalization,
                E::Realization,
                E::Dependency,
                E::Link,
                E::NoteAnchor,
            ],
        ),
        DiagramKind::UseCase => (
            &[N::Actor, N::UseCase, N::SystemBoundary, N::Note],
            &[
                E::Association,
                E::Generalization,
                E::Include,
                E::Extend,
                E::Dependency,
                E::NoteAnchor,
            ],
        ),
        DiagramKind::Sequence => (
            &[
                N::Lifeline,
                N::CombinedFragment,
                N::InteractionUse,
                N::StateInvariant,
                N::Termination,
                N::Note,
            ],
            &[
                E::Message,
                E::AsyncMessage,
                E::Reply,
                E::CreateMessage,
                E::DestroyMessage,
                E::FoundMessage,
                E::LostMessage,
                E::DurationConstraint,
                E::TimeConstraint,
                E::NoteAnchor,
            ],
        ),
        DiagramKind::Activity => (
            &[
                N::Action,
                N::CallBehavior,
                N::SendSignal,
                N::AcceptEvent,
                N::AcceptTimeEvent,
                N::InitialNode,
                N::ActivityFinal,
                N::FlowFinal,
                N::Decision,
                N::Merge,
                N::Fork,
                N::Join,
                N::ObjectNode,
                N::InputPin,
                N::OutputPin,
                N::ActivityParameter,
                N::ActivityConnector,
                N::Partition,
                N::Note,
            ],
            &[E::Flow, E::NoteAnchor],
        ),
        DiagramKind::StateMachine => (
            &[
                N::State,
                N::SubmachineState,
                N::InitialPseudostate,
                N::FinalState,
                N::Choice,
                N::Junction,
                N::ForkPseudostate,
                N::JoinPseudostate,
                N::ShallowHistory,
                N::DeepHistory,
                N::EntryPoint,
                N::ExitPoint,
                N::StubState,
                N::Note,
            ],
            &[E::Transition, E::NoteAnchor],
        ),
        DiagramKind::Component => (
            &[N::Component, N::Interface, N::Artifact, N::Class, N::Package, N::Subsystem, N::Note],
            &[E::Dependency, E::Realization, E::Association, E::Generalization, E::NoteAnchor],
        ),
        DiagramKind::Deployment => (
            &[
                N::DeploymentNode,
                N::NodeInstance,
                N::Component,
                N::ComponentInstance,
                N::Artifact,
                N::Object,
                N::Interface,
                N::Note,
            ],
            &[E::Association, E::Link, E::Dependency, E::Realization, E::NoteAnchor],
        ),
        DiagramKind::CompositeStructure => (
            &[N::Class, N::Part, N::Port, N::Interface, N::Note],
            &[E::Connector, E::Realization, E::Dependency, E::NoteAnchor],
        ),
        DiagramKind::EntityRelationship => (
            &[
                N::ErEntity,
                N::ErWeakEntity,
                N::ErAssociativeEntity,
                N::ErAttribute,
                N::ErKeyAttribute,
                N::ErPartialKeyAttribute,
                N::ErMultivaluedAttribute,
                N::ErDerivedAttribute,
                N::ErRelationship,
                N::ErIdentifyingRelationship,
                N::ErIsA,
                N::Note,
            ],
            &[E::ErLine, E::NoteAnchor],
        ),
    };
    std::iter::once(Tool::Select)
        .chain(nodes.iter().map(|&n| Tool::Node(n)))
        .chain(edges.iter().map(|&e| Tool::Edge(e)))
        .chain([Tool::Text, Tool::Line])
        .collect()
}

/// Tools offered for a diagram, in palette order.
pub fn diagram_palette(props: &DiagramProps) -> Vec<Tool> {
    if props.diagram == DiagramKind::EntityRelationship && props.notation.lists_attributes() {
        return listing_palette();
    }
    match props.objects {
        true => object_palette(),
        false => palette(props.diagram),
    }
}

/// Tools offered for an ER diagram in IE or IDEF1X: entities, whose attributes are rows added
/// to them, and the relationship and subtype lines.
fn listing_palette() -> Vec<Tool> {
    use EdgeTool as E;
    use NodeTool as N;
    let nodes = [N::ErEntity, N::ErWeakEntity, N::Note];
    let edges =
        [E::ErIdentifying, E::ErNonIdentifying, E::ErManyToMany, E::ErSubtype, E::NoteAnchor];
    std::iter::once(Tool::Select)
        .chain(nodes.map(Tool::Node))
        .chain(edges.map(Tool::Edge))
        .chain([Tool::Text, Tool::Line])
        .collect()
}

/// The lines a diagram drawn with `props` offers from an element of kind `source` to one of
/// kind `target` (`same`: both are one element), in palette order: what a line dragged from a
/// node's handle can become. Note anchors and messages with one lifeline are left out.
pub fn connections(
    props: &DiagramProps,
    source: &ElementKind,
    target: &ElementKind,
    same: bool,
) -> Vec<EdgeTool> {
    diagram_palette(props)
        .into_iter()
        .filter_map(|t| match t {
            Tool::Edge(e) => Some(e),
            _ => None,
        })
        .filter(|e| {
            !matches!(e, EdgeTool::NoteAnchor | EdgeTool::FoundMessage | EdgeTool::LostMessage)
        })
        .filter(|e| e.accepts(source, target, same))
        .collect()
}

/// Tools offered for an object diagram: a class diagram's, with Object and Link first.
pub fn object_palette() -> Vec<Tool> {
    let mut tools = palette(DiagramKind::Class);
    let first = |t: &Tool| matches!(t, Tool::Node(NodeTool::Object) | Tool::Edge(EdgeTool::Link));
    tools.sort_by_key(|t| (*t != Tool::Select, !first(t)));
    tools
}

impl NodeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Boundary => "boundary",
            Self::Control => "control",
            Self::Entity => "entity",
            Self::Interface => "interface",
            Self::Enumeration => "enumeration",
            Self::Package => "package",
            Self::Subsystem => "subsystem",
            Self::Note => "comment",
            Self::Actor => "actor",
            Self::UseCase => "useCase",
            Self::Lifeline => "lifeline",
            Self::SystemBoundary => "systemBoundary",
            Self::CombinedFragment => "combinedFragment",
            Self::InteractionUse => "interactionUse",
            Self::StateInvariant => "stateInvariant",
            Self::Termination => "termination",
            Self::Action => "action",
            Self::InitialNode => "initialNode",
            Self::ActivityFinal => "activityFinal",
            Self::FlowFinal => "flowFinal",
            Self::Decision => "decision",
            Self::Merge => "merge",
            Self::Fork => "fork",
            Self::Join => "join",
            Self::ObjectNode => "objectNode",
            Self::Partition => "partition",
            Self::CallBehavior => "callBehavior",
            Self::SendSignal => "sendSignal",
            Self::AcceptEvent => "acceptEvent",
            Self::AcceptTimeEvent => "acceptTimeEvent",
            Self::ActivityConnector => "activityConnector",
            Self::InputPin => "inputPin",
            Self::OutputPin => "outputPin",
            Self::ActivityParameter => "activityParameter",
            Self::State => "state",
            Self::SubmachineState => "submachineState",
            Self::FinalState => "finalState",
            Self::InitialPseudostate => "initialPseudostate",
            Self::Choice => "choice",
            Self::Junction => "junction",
            Self::ForkPseudostate => "forkPseudostate",
            Self::JoinPseudostate => "joinPseudostate",
            Self::ShallowHistory => "shallowHistory",
            Self::DeepHistory => "deepHistory",
            Self::EntryPoint => "entryPoint",
            Self::ExitPoint => "exitPoint",
            Self::StubState => "stubState",
            Self::Component => "component",
            Self::Artifact => "artifact",
            Self::DeploymentNode => "node",
            Self::ComponentInstance => "componentInstance",
            Self::NodeInstance => "nodeInstance",
            Self::Object => "object",
            Self::Part => "part",
            Self::Port => "port",
            Self::ErEntity => "erEntity",
            Self::ErWeakEntity => "erWeakEntity",
            Self::ErAssociativeEntity => "erAssociativeEntity",
            Self::ErAttribute => "erAttribute",
            Self::ErKeyAttribute => "erKeyAttribute",
            Self::ErPartialKeyAttribute => "erPartialKeyAttribute",
            Self::ErMultivaluedAttribute => "erMultivaluedAttribute",
            Self::ErDerivedAttribute => "erDerivedAttribute",
            Self::ErRelationship => "erRelationship",
            Self::ErIdentifyingRelationship => "erIdentifyingRelationship",
            Self::ErIsA => "erIsA",
        }
    }

    /// The tool that creates instances of `kind`.
    pub fn for_instance(kind: InstanceKind) -> Self {
        match kind {
            InstanceKind::Object => Self::Object,
            InstanceKind::Component => Self::ComponentInstance,
            InstanceKind::Node => Self::NodeInstance,
        }
    }

    /// The kind of activity node this tool creates, `None` for other tools.
    pub fn activity_node(self) -> Option<ActivityNodeKind> {
        match self.element_kind() {
            ElementKind::ActivityNode(p) => Some(p.node),
            _ => None,
        }
    }

    /// The tool that creates activity nodes of `kind`.
    pub fn for_activity_node(kind: ActivityNodeKind) -> Self {
        match kind {
            ActivityNodeKind::Action => Self::Action,
            ActivityNodeKind::Initial => Self::InitialNode,
            ActivityNodeKind::ActivityFinal => Self::ActivityFinal,
            ActivityNodeKind::FlowFinal => Self::FlowFinal,
            ActivityNodeKind::Decision => Self::Decision,
            ActivityNodeKind::Merge => Self::Merge,
            ActivityNodeKind::Fork => Self::Fork,
            ActivityNodeKind::Join => Self::Join,
            ActivityNodeKind::Object => Self::ObjectNode,
            ActivityNodeKind::SendSignal => Self::SendSignal,
            ActivityNodeKind::AcceptEvent => Self::AcceptEvent,
            ActivityNodeKind::AcceptTimeEvent => Self::AcceptTimeEvent,
            ActivityNodeKind::CallBehavior => Self::CallBehavior,
            ActivityNodeKind::Connector => Self::ActivityConnector,
            ActivityNodeKind::InputPin => Self::InputPin,
            ActivityNodeKind::OutputPin => Self::OutputPin,
            ActivityNodeKind::ActivityParameter => Self::ActivityParameter,
        }
    }

    /// The tool that creates pseudostates of `kind`.
    pub fn for_pseudostate(kind: PseudostateKind) -> Self {
        match kind {
            PseudostateKind::Initial => Self::InitialPseudostate,
            PseudostateKind::Choice => Self::Choice,
            PseudostateKind::Junction => Self::Junction,
            PseudostateKind::Fork => Self::ForkPseudostate,
            PseudostateKind::Join => Self::JoinPseudostate,
            PseudostateKind::ShallowHistory => Self::ShallowHistory,
            PseudostateKind::DeepHistory => Self::DeepHistory,
            PseudostateKind::EntryPoint => Self::EntryPoint,
            PseudostateKind::ExitPoint => Self::ExitPoint,
            PseudostateKind::StubState => Self::StubState,
        }
    }

    /// Whether new elements of this tool get a generated name. Notes, control nodes, final
    /// states and most pseudostates stay unnamed; their names are not shown.
    pub fn is_named(self) -> bool {
        match self.element_kind() {
            ElementKind::ActivityNode(p) => p.node.is_named(),
            ElementKind::Pseudostate(p) => p.pseudostate.is_named(),
            // An IS-A reads "IS-A" until it is given a name.
            ElementKind::Comment(_)
            | ElementKind::FinalState
            | ElementKind::Termination(_)
            | ElementKind::ErIsA(_) => false,
            _ => true,
        }
    }

    pub fn element_kind(self) -> ElementKind {
        let classifier = ClassifierProps::default();
        let node = ElementKind::activity_node;
        let pseudostate = ElementKind::pseudostate;
        let instance =
            |instance| ElementKind::Instance(InstanceProps { instance, classifier: None });
        match self {
            Self::Class | Self::Boundary | Self::Control | Self::Entity => {
                ElementKind::Class(classifier)
            }
            Self::Interface => ElementKind::Interface(classifier),
            Self::Enumeration => ElementKind::Enumeration(classifier),
            Self::Package | Self::Subsystem => ElementKind::Package,
            Self::Note => ElementKind::Comment(CommentProps::default()),
            Self::Actor => ElementKind::Actor(classifier),
            Self::UseCase => ElementKind::UseCase(classifier),
            Self::Lifeline => ElementKind::Lifeline(LifelineProps::default()),
            Self::SystemBoundary => ElementKind::SystemBoundary,
            Self::CombinedFragment => ElementKind::CombinedFragment(CombinedFragmentProps {
                operator: InteractionOperator::Opt,
                covered: Vec::new(),
            }),
            Self::InteractionUse => ElementKind::InteractionUse(InteractionUseProps::default()),
            // `sequence_edit::create_on_lifeline` puts them on the lifeline clicked.
            Self::StateInvariant => {
                ElementKind::StateInvariant(StateInvariantProps { lifeline: Id::from_u128(0) })
            }
            Self::Termination => {
                ElementKind::Termination(TerminationProps { lifeline: Id::from_u128(0) })
            }
            Self::Action => node(ActivityNodeKind::Action),
            Self::InitialNode => node(ActivityNodeKind::Initial),
            Self::ActivityFinal => node(ActivityNodeKind::ActivityFinal),
            Self::FlowFinal => node(ActivityNodeKind::FlowFinal),
            Self::Decision => node(ActivityNodeKind::Decision),
            Self::Merge => node(ActivityNodeKind::Merge),
            Self::Fork => node(ActivityNodeKind::Fork),
            Self::Join => node(ActivityNodeKind::Join),
            Self::ObjectNode => node(ActivityNodeKind::Object),
            Self::Partition => ElementKind::Partition,
            Self::CallBehavior => node(ActivityNodeKind::CallBehavior),
            Self::SendSignal => node(ActivityNodeKind::SendSignal),
            Self::AcceptEvent => node(ActivityNodeKind::AcceptEvent),
            Self::AcceptTimeEvent => node(ActivityNodeKind::AcceptTimeEvent),
            Self::ActivityConnector => node(ActivityNodeKind::Connector),
            Self::InputPin => node(ActivityNodeKind::InputPin),
            Self::OutputPin => node(ActivityNodeKind::OutputPin),
            Self::ActivityParameter => node(ActivityNodeKind::ActivityParameter),
            // `create_node` gives a new submachine state its state machine.
            Self::State | Self::SubmachineState => ElementKind::State(StateProps::default()),
            Self::FinalState => ElementKind::FinalState,
            Self::InitialPseudostate => pseudostate(PseudostateKind::Initial),
            Self::Choice => pseudostate(PseudostateKind::Choice),
            Self::Junction => pseudostate(PseudostateKind::Junction),
            Self::ForkPseudostate => pseudostate(PseudostateKind::Fork),
            Self::JoinPseudostate => pseudostate(PseudostateKind::Join),
            Self::ShallowHistory => pseudostate(PseudostateKind::ShallowHistory),
            Self::DeepHistory => pseudostate(PseudostateKind::DeepHistory),
            Self::EntryPoint => pseudostate(PseudostateKind::EntryPoint),
            Self::ExitPoint => pseudostate(PseudostateKind::ExitPoint),
            Self::StubState => pseudostate(PseudostateKind::StubState),
            Self::Component => ElementKind::Component(classifier),
            Self::Artifact => ElementKind::Artifact(classifier),
            Self::DeploymentNode => ElementKind::Node(classifier),
            Self::ComponentInstance => instance(InstanceKind::Component),
            Self::NodeInstance => instance(InstanceKind::Node),
            Self::Object => instance(InstanceKind::Object),
            Self::Part => ElementKind::attribute(None),
            Self::Port => ElementKind::port(None),
            Self::ErEntity => ElementKind::ErEntity(ErEntityProps::default()),
            Self::ErWeakEntity => {
                ElementKind::ErEntity(ErEntityProps { weak: true, ..Default::default() })
            }
            Self::ErAssociativeEntity => {
                ElementKind::ErEntity(ErEntityProps { associative: true, ..Default::default() })
            }
            Self::ErAttribute => ElementKind::ErAttribute(ErAttributeProps::default()),
            Self::ErKeyAttribute | Self::ErPartialKeyAttribute => {
                let key =
                    if self == Self::ErKeyAttribute { ErKey::Primary } else { ErKey::Partial };
                ElementKind::ErAttribute(ErAttributeProps { key: Some(key), ..Default::default() })
            }
            Self::ErMultivaluedAttribute => ElementKind::ErAttribute(ErAttributeProps {
                multivalued: true,
                ..Default::default()
            }),
            Self::ErDerivedAttribute => {
                ElementKind::ErAttribute(ErAttributeProps { derived: true, ..Default::default() })
            }
            Self::ErRelationship => ElementKind::ErRelationship(ErRelationshipProps::default()),
            Self::ErIdentifyingRelationship => {
                ElementKind::ErRelationship(ErRelationshipProps::new(true))
            }
            Self::ErIsA => ElementKind::ErIsA(ErIsAProps::default()),
        }
    }

    /// A new element of this tool named `name`. A subsystem is a package marked «subsystem», as
    /// in UML 1.
    pub fn element_data(self, name: impl Into<String>) -> ElementData {
        let mut data = ElementData::new(name, self.element_kind());
        if self == Self::Subsystem {
            data.stereotypes = vec![SUBSYSTEM.into()];
        }
        if let Some(stereotype) = self.robustness_stereotype() {
            data.stereotypes = vec![stereotype.into()];
        }
        data
    }

    /// The stereotype of a robustness tool, drawn as its icon.
    pub fn robustness_stereotype(self) -> Option<&'static str> {
        match self {
            Self::Boundary => Some("boundary"),
            Self::Control => Some("control"),
            Self::Entity => Some("entity"),
            _ => None,
        }
    }

    pub fn default_size(self) -> Size {
        match self {
            Self::Class => Size::new(160.0, 70.0),
            // Astah's robustness icons.
            Self::Boundary => Size::new(60.0, 40.0),
            Self::Control | Self::Entity => Size::new(40.0, 40.0),
            Self::Interface | Self::Enumeration => Size::new(160.0, 60.0),
            Self::Package | Self::Subsystem => Size::new(200.0, 140.0),
            Self::Note => Size::new(160.0, 60.0),
            Self::Actor => Size::new(40.0, 55.0),
            Self::UseCase => Size::new(90.0, 40.0),
            Self::Lifeline => Size::new(100.0, 300.0),
            Self::SystemBoundary => Size::new(300.0, 360.0),
            // The layout sizes fragments; this is only what the file keeps.
            Self::CombinedFragment | Self::InteractionUse => Size::new(200.0, 80.0),
            Self::StateInvariant => {
                Size::new(sequence_edit::INVARIANT_WIDTH, sequence_edit::INVARIANT_HEIGHT)
            }
            Self::Termination => Size::new(sequence_edit::CROSS, sequence_edit::CROSS),
            Self::Action => Size::new(120.0, 36.0),
            // Astah's control nodes: 20 px circles, 5 px bars.
            Self::InitialNode | Self::ActivityFinal | Self::FlowFinal => Size::new(20.0, 20.0),
            Self::Decision | Self::Merge => Size::new(24.0, 24.0),
            Self::Fork | Self::Join => Size::new(60.0, 5.0),
            Self::ObjectNode => Size::new(80.0, 36.0),
            Self::Partition => Size::new(200.0, 400.0),
            // Astah's sizes: signal actions as wide as their stereotype and 20 on each side,
            // with room for the name below it; a call behavior action keeps 15 for its rake.
            Self::SendSignal => Size::new(150.0, 38.19),
            Self::AcceptEvent => Size::new(142.0, 38.19),
            Self::AcceptTimeEvent => Size::new(20.0, 25.0),
            Self::CallBehavior => Size::new(120.0, 38.09),
            Self::ActivityConnector => Size::new(21.5625, 21.5625),
            Self::InputPin | Self::OutputPin => {
                Size::new(crate::scene::PIN_SIZE, crate::scene::PIN_SIZE)
            }
            Self::ActivityParameter => Size::new(80.0, 25.09),
            Self::State => Size::new(100.0, 50.0),
            Self::FinalState | Self::InitialPseudostate => Size::new(20.0, 20.0),
            Self::Choice => Size::new(24.0, 24.0),
            Self::Junction => Size::new(14.0, 14.0),
            Self::ForkPseudostate | Self::JoinPseudostate => Size::new(60.0, 5.0),
            Self::ShallowHistory | Self::DeepHistory => Size::new(24.0, 24.0),
            // Astah's default size of submachine states; points are circles of 20, a stub state
            // is as high as its bar and name.
            Self::SubmachineState => Size::new(150.0, 60.0),
            Self::EntryPoint | Self::ExitPoint => {
                Size::new(crate::scene::POINT_SIZE, crate::scene::POINT_SIZE)
            }
            Self::StubState => Size::new(60.0, 24.09),
            Self::Component | Self::ComponentInstance => Size::new(140.0, 60.0),
            Self::Artifact => Size::new(120.0, 60.0),
            Self::DeploymentNode | Self::NodeInstance => Size::new(200.0, 140.0),
            Self::Object => Size::new(120.0, 40.0),
            Self::Part => Size::new(120.0, 50.0),
            Self::Port => Size::new(crate::composite::PORT_SIZE, crate::composite::PORT_SIZE),
            // Steven's sizes of the flight example; names that need more grow the box.
            Self::ErEntity | Self::ErWeakEntity => Size::new(100.0, 40.0),
            Self::ErAssociativeEntity => Size::new(140.0, 60.0),
            Self::ErAttribute
            | Self::ErKeyAttribute
            | Self::ErPartialKeyAttribute
            | Self::ErMultivaluedAttribute
            | Self::ErDerivedAttribute => Size::new(80.0, 30.0),
            Self::ErRelationship | Self::ErIdentifyingRelationship => Size::new(80.0, 50.0),
            Self::ErIsA => Size::new(80.0, 50.0),
        }
    }
}

fn is_class_like(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

/// What a dependency may connect. Ports provide interfaces by realizing them and require them
/// by depending on them.
fn is_dependable(kind: &ElementKind) -> bool {
    kind.is_classifier()
        || matches!(kind, ElementKind::Package | ElementKind::Instance(_) | ElementKind::Port(_))
}

impl EdgeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Association => "association",
            Self::AssociationClass => "associationClass",
            Self::Link => "link",
            Self::Generalization => "generalization",
            Self::Realization => "realization",
            Self::Dependency => "dependency",
            Self::Include => "include",
            Self::Extend => "extend",
            Self::NoteAnchor => "noteAnchor",
            Self::Message => "message",
            Self::AsyncMessage => "asyncMessage",
            Self::Reply => "reply",
            Self::CreateMessage => "createMessage",
            Self::DestroyMessage => "destroyMessage",
            Self::FoundMessage => "foundMessage",
            Self::LostMessage => "lostMessage",
            Self::Flow => "flow",
            Self::Transition => "transition",
            Self::Connector => "connector",
            Self::DurationConstraint => "durationConstraint",
            Self::TimeConstraint => "timeConstraint",
            Self::ErLine => "erLine",
            Self::ErIdentifying => "erIdentifying",
            Self::ErNonIdentifying => "erNonIdentifying",
            Self::ErManyToMany => "erManyToMany",
            Self::ErSubtype => "erSubtype",
        }
    }

    /// The kind of message this tool draws, `None` for other tools.
    pub fn message_sort(self) -> Option<MessageSort> {
        match self {
            Self::Message | Self::FoundMessage => Some(MessageSort::Synchronous),
            Self::AsyncMessage => Some(MessageSort::Asynchronous),
            // A lost message mostly answers a found one: by default it is a reply.
            Self::Reply | Self::LostMessage => Some(MessageSort::Reply),
            Self::CreateMessage => Some(MessageSort::Create),
            Self::DestroyMessage => Some(MessageSort::Destroy),
            _ => None,
        }
    }

    /// Whether this tool may connect an element of kind `source` to one of kind `target`.
    /// `same` tells whether both ends are the same element.
    pub fn accepts(self, source: &ElementKind, target: &ElementKind, same: bool) -> bool {
        use ElementKind as K;
        match self {
            Self::Message | Self::AsyncMessage | Self::Reply => {
                matches!((source, target), (K::Lifeline(_), K::Lifeline(_)))
            }
            // Found and lost messages have one lifeline: they are drawn from it to itself.
            Self::FoundMessage | Self::LostMessage => {
                same && matches!((source, target), (K::Lifeline(_), K::Lifeline(_)))
            }
            // A lifeline can neither create nor destroy itself.
            Self::CreateMessage | Self::DestroyMessage => {
                !same && matches!((source, target), (K::Lifeline(_), K::Lifeline(_)))
            }
            Self::Association => source.is_classifier() && target.is_classifier(),
            // Astah draws association classes between classes and interfaces.
            Self::AssociationClass => is_class_like(source) && is_class_like(target),
            Self::Link => matches!((source, target), (K::Instance(_), K::Instance(_))),
            Self::Generalization => {
                !same
                    && std::mem::discriminant(source) == std::mem::discriminant(target)
                    && (is_class_like(source)
                        || matches!(
                            source,
                            K::Actor(_)
                                | K::UseCase(_)
                                | K::Component(_)
                                | K::Artifact(_)
                                | K::Node(_)
                        ))
            }
            Self::Realization => {
                !same
                    && matches!(
                        source,
                        K::Class(_)
                            | K::Enumeration(_)
                            | K::DataType(_)
                            | K::Component(_)
                            | K::Port(_)
                    )
                    && matches!(target, K::Interface(_))
            }
            Self::Connector => {
                let end = |k: &ElementKind| matches!(k, K::Attribute(_) | K::Port(_));
                !same && end(source) && end(target)
            }
            Self::DurationConstraint => {
                !same && matches!((source, target), (K::Message(_), K::Message(_)))
            }
            Self::TimeConstraint => same && matches!(source, K::Message(_)),
            Self::Dependency => !same && is_dependable(source) && is_dependable(target),
            Self::Include | Self::Extend => {
                !same && matches!(source, K::UseCase(_)) && matches!(target, K::UseCase(_))
            }
            // Nothing flows into an initial node or out of a final node.
            Self::Flow => match (source, target) {
                (K::ActivityNode(s), K::ActivityNode(t)) => {
                    !same && s.node.has_outgoing() && t.node.has_incoming()
                }
                _ => false,
            },
            // Nothing leaves a final state or enters an initial pseudostate; only states loop
            // back to themselves; no transition runs between two stub states (Astah refuses it).
            Self::Transition => {
                let initial = matches!(
                    target,
                    K::Pseudostate(p) if p.pseudostate == PseudostateKind::Initial
                );
                let stub = |k: &K| matches!(k, K::Pseudostate(p) if p.pseudostate == PseudostateKind::StubState);
                source.is_vertex()
                    && target.is_vertex()
                    && !matches!(source, K::FinalState)
                    && !initial
                    && !(stub(source) && stub(target))
                    && (!same || matches!(source, K::State(_)))
            }
            Self::NoteAnchor => {
                matches!(source, K::Comment(_)) != matches!(target, K::Comment(_))
                    && !matches!(source, K::Diagram(_) | K::Model)
                    && !matches!(target, K::Diagram(_) | K::Model)
            }
            Self::ErLine => !same && crate::er::joins(source, target).is_some(),
            // A plain relationship may join an entity to itself (a manager of employees).
            Self::ErNonIdentifying | Self::ErManyToMany => {
                matches!((source, target), (K::ErEntity(_), K::ErEntity(_)))
            }
            Self::ErIdentifying | Self::ErSubtype => {
                !same && matches!((source, target), (K::ErEntity(_), K::ErEntity(_)))
            }
        }
    }

    /// The relationship element this tool creates, `None` for purely graphical note anchors.
    pub fn element_kind(self, source: Id, target: Id) -> Option<ElementKind> {
        let directed = DirectedProps { client: source, supplier: target };
        match self {
            Self::Association | Self::AssociationClass => {
                // Astah makes association ends private.
                let mut kind = ElementKind::association(source, target);
                if let ElementKind::Association(p) = &mut kind {
                    p.ends.iter_mut().for_each(|end| end.visibility = Visibility::Private);
                }
                Some(kind)
            }
            Self::Link => Some(ElementKind::link(source, target)),
            Self::Generalization => Some(ElementKind::generalization(source, target)),
            Self::Realization => Some(ElementKind::Realization(directed)),
            Self::Dependency => Some(ElementKind::Dependency(directed)),
            Self::Include => {
                Some(ElementKind::Include(IncludeProps { including: source, addition: target }))
            }
            Self::Extend => Some(ElementKind::Extend(ExtendProps {
                extension: source,
                extended: target,
                extension_points: Vec::new(),
                condition: String::new(),
            })),
            Self::Flow => Some(ElementKind::flow(source, target)),
            Self::Transition => Some(ElementKind::transition(source, target)),
            // Without the diagram it cannot tell which part a port is reached through;
            // `connect` does.
            Self::Connector => Some(ElementKind::connector(
                ConnectorEnd::new(source, None),
                ConnectorEnd::new(target, None),
            )),
            // What it becomes depends on the kinds of its ends; `er::connect` makes it, and
            // `er::relate` the lines of IE and IDEF1X.
            Self::NoteAnchor
            | Self::ErLine
            | Self::ErIdentifying
            | Self::ErNonIdentifying
            | Self::ErManyToMany
            | Self::ErSubtype => None,
            Self::DurationConstraint => {
                Some(ElementKind::DurationConstraint(DurationConstraintProps {
                    from: source,
                    to: target,
                }))
            }
            Self::TimeConstraint => {
                Some(ElementKind::TimeConstraint(TimeConstraintProps { message: source }))
            }
            Self::Message
            | Self::AsyncMessage
            | Self::Reply
            | Self::CreateMessage
            | Self::DestroyMessage => {
                Some(ElementKind::message(self.message_sort()?, source, target))
            }
            Self::FoundMessage | Self::LostMessage => {
                let mut kind = ElementKind::message(self.message_sort()?, source, target);
                if let ElementKind::Message(p) = &mut kind {
                    p.gate = if self == Self::FoundMessage {
                        MessageGate::Found
                    } else {
                        MessageGate::Lost
                    };
                }
                Some(kind)
            }
        }
    }
}

/// Whether an existing element of `kind` may be shown on a diagram of kind `diagram`.
pub fn can_show(diagram: DiagramKind, kind: &ElementKind) -> bool {
    match diagram {
        // Class diagrams may show components, nodes and instances too (object diagrams); Astah's
        // API exports component and deployment diagrams as class diagrams.
        DiagramKind::Class => {
            is_class_like(kind)
                || matches!(
                    kind,
                    ElementKind::Package
                        | ElementKind::Comment(_)
                        | ElementKind::Component(_)
                        | ElementKind::Artifact(_)
                        | ElementKind::Node(_)
                        | ElementKind::Instance(_)
                )
        }
        DiagramKind::UseCase => matches!(
            kind,
            ElementKind::Actor(_)
                | ElementKind::UseCase(_)
                | ElementKind::SystemBoundary
                | ElementKind::Comment(_)
        ),
        DiagramKind::Sequence => matches!(
            kind,
            ElementKind::Lifeline(_)
                | ElementKind::CombinedFragment(_)
                | ElementKind::InteractionUse(_)
                | ElementKind::StateInvariant(_)
                | ElementKind::Termination(_)
                | ElementKind::DurationConstraint(_)
                | ElementKind::TimeConstraint(_)
                | ElementKind::Comment(_)
        ),
        DiagramKind::Activity => matches!(
            kind,
            ElementKind::ActivityNode(_) | ElementKind::Partition | ElementKind::Comment(_)
        ),
        DiagramKind::StateMachine => kind.is_vertex() || matches!(kind, ElementKind::Comment(_)),
        DiagramKind::Component => {
            is_class_like(kind)
                || matches!(
                    kind,
                    ElementKind::Component(_)
                        | ElementKind::Artifact(_)
                        | ElementKind::Package
                        | ElementKind::Comment(_)
                )
        }
        DiagramKind::Deployment => matches!(
            kind,
            ElementKind::Node(_)
                | ElementKind::Component(_)
                | ElementKind::Artifact(_)
                | ElementKind::Instance(_)
                | ElementKind::Interface(_)
                | ElementKind::Class(_)
                | ElementKind::Comment(_)
        ),
        DiagramKind::CompositeStructure => matches!(
            kind,
            ElementKind::Class(_)
                | ElementKind::Component(_)
                | ElementKind::Interface(_)
                | ElementKind::Attribute(_)
                | ElementKind::Port(_)
                | ElementKind::Comment(_)
        ),
        DiagramKind::EntityRelationship => matches!(
            kind,
            ElementKind::ErEntity(_)
                | ElementKind::ErAttribute(_)
                | ElementKind::ErRelationship(_)
                | ElementKind::ErIsA(_)
                | ElementKind::Comment(_)
        ),
    }
}

/// Default node size for showing an existing element.
pub fn default_size_for(kind: &ElementKind) -> Size {
    let tool = match kind {
        ElementKind::Interface(_) => NodeTool::Interface,
        ElementKind::Enumeration(_) => NodeTool::Enumeration,
        ElementKind::Package => NodeTool::Package,
        ElementKind::Comment(_) => NodeTool::Note,
        ElementKind::Actor(_) => NodeTool::Actor,
        ElementKind::UseCase(_) => NodeTool::UseCase,
        ElementKind::Lifeline(_) => NodeTool::Lifeline,
        ElementKind::SystemBoundary => NodeTool::SystemBoundary,
        ElementKind::CombinedFragment(_) => NodeTool::CombinedFragment,
        ElementKind::InteractionUse(_) => NodeTool::InteractionUse,
        ElementKind::StateInvariant(_) => NodeTool::StateInvariant,
        ElementKind::Termination(_) => NodeTool::Termination,
        ElementKind::ActivityNode(p) => NodeTool::for_activity_node(p.node),
        ElementKind::State(p) if p.submachine.is_some() => NodeTool::SubmachineState,
        ElementKind::State(_) => NodeTool::State,
        ElementKind::FinalState => NodeTool::FinalState,
        ElementKind::Pseudostate(p) => NodeTool::for_pseudostate(p.pseudostate),
        ElementKind::Component(_) => NodeTool::Component,
        ElementKind::Artifact(_) => NodeTool::Artifact,
        ElementKind::Node(_) => NodeTool::DeploymentNode,
        ElementKind::Instance(p) => NodeTool::for_instance(p.instance),
        ElementKind::Partition => NodeTool::Partition,
        ElementKind::Attribute(_) => NodeTool::Part,
        ElementKind::Port(_) => NodeTool::Port,
        ElementKind::ErEntity(p) if p.associative => NodeTool::ErAssociativeEntity,
        ElementKind::ErEntity(_) => NodeTool::ErEntity,
        ElementKind::ErAttribute(_) => NodeTool::ErAttribute,
        ElementKind::ErRelationship(_) => NodeTool::ErRelationship,
        ElementKind::ErIsA(_) => NodeTool::ErIsA,
        _ => NodeTool::Class,
    };
    tool.default_size()
}

/// Shows an existing element on `diagram` at `origin`, together with edges for its binary
/// relationships to elements already on the diagram. Returns the new node.
pub fn show_element(tx: &mut Tx<'_>, diagram: Id, element: Id, origin: Point) -> Result<Id> {
    let model = tx.model();
    let diagram_kind =
        model.element(diagram)?.kind().as_diagram().ok_or(ModelError::NotADiagram(diagram))?;
    let kind = model.element(element)?.kind().clone();
    if !can_show(diagram_kind, &kind) {
        return Err(ModelError::Refused(umlcraft_model::Refusal::CannotShow(kind.type_name())));
    }
    // Lifelines and fragments belong to one interaction and show only on its diagram.
    let interaction = model.element(diagram)?.owner();
    if diagram_kind == DiagramKind::Sequence
        && !matches!(kind, ElementKind::Comment(_))
        && !interaction.is_some_and(|i| model.is_ancestor(i, element))
    {
        return Err(ModelError::Refused(umlcraft_model::Refusal::OtherInteraction));
    }
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    if view.presentations_of(element).next().is_some() {
        return Err(ModelError::Refused(umlcraft_model::Refusal::AlreadyShown));
    }
    // Relationships connecting the element to something shown, as (relationship, source, target)
    // with `None` standing for the new node.
    let mut edges = Vec::new();
    // Messages need a place in the sequence; they are drawn, not shown.
    let shows_edge = |k: &ElementKind| k.is_relationship();
    for e in model.elements().filter(|e| shows_edge(e.kind())) {
        let ends: Vec<Id> = e
            .kind()
            .references()
            .iter()
            .filter(|(_, r)| r.is_strong())
            .map(|(id, _)| *id)
            .collect();
        let [a, b] = ends[..] else { continue };
        if view.presentations_of(e.id()).next().is_some() || (a != element && b != element) {
            continue;
        }
        let end = |id: Id| -> Option<Option<Id>> {
            if id == element {
                Some(None)
            } else {
                view.presentations_of(id).next().map(|p| Some(p.id))
            }
        };
        if let (Some(source), Some(target)) = (end(a), end(b)) {
            edges.push((e.id(), source, target));
        }
    }
    // An ER attribute's line runs from its owner and shows the attribute.
    let shown = |id: Id| view.presentations_of(id).next().map(|p| p.id);
    let er_attribute =
        |id: Id| model.get(id).is_some_and(|e| matches!(e.kind(), ElementKind::ErAttribute(_)));
    if er_attribute(element)
        && let Some(owner) = model.element(element)?.owner().and_then(shown)
    {
        edges.push((element, Some(owner), None));
    }
    for &child in model.children(element).iter().filter(|&&c| er_attribute(c)) {
        if let Some(node) = shown(child) {
            edges.push((child, None, Some(node)));
        }
    }
    let node = add_node_for(
        tx,
        diagram,
        element,
        Rect::from_origin_size(origin, default_size_for(&kind)),
    )?;
    for (relationship, source, target) in edges {
        let waypoints = if source == target {
            crate::scene::self_loop(Rect::from_origin_size(origin, default_size_for(&kind)))
        } else {
            Vec::new()
        };
        tx.add_edge(
            diagram,
            Some(relationship),
            source.unwrap_or(node),
            target.unwrap_or(node),
            waypoints,
        )?;
    }
    Ok(node)
}

/// Whether nodes of this kind contain other nodes drawn inside them: the same kinds whose
/// contents move with them ([`scene::with_contents`](crate::scene::with_contents)).
pub fn is_container(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::SystemBoundary
            | ElementKind::Package
            | ElementKind::Partition
            | ElementKind::State(_)
            | ElementKind::Component(_)
            | ElementKind::Node(_)
    ) || matches!(kind, ElementKind::Instance(p) if p.instance == InstanceKind::Node)
}

/// Adds a node for `element`. System boundaries and partitions go to the back, so they never
/// hide what is drawn inside them. Lifelines line up with the heads of the others.
fn add_node_for(tx: &mut Tx<'_>, diagram: Id, element: Id, bounds: Rect) -> Result<Id> {
    let model = tx.model();
    let kind = model.element(element)?.kind();
    if matches!(kind, ElementKind::Lifeline(_)) {
        return sequence_edit::add_lifeline(tx, diagram, element, bounds.x, bounds.y);
    }
    if matches!(kind, ElementKind::StateInvariant(_) | ElementKind::Termination(_)) {
        return sequence_edit::add_mark(tx, diagram, element, bounds.y);
    }
    if matches!(kind, ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_)) {
        return sequence_edit::add_constraint(tx, diagram, element);
    }
    let fragment = matches!(kind, ElementKind::CombinedFragment(_));
    // On composite structure diagrams classes are drawn around their parts: behind them, and
    // keeping the size they are given.
    let structured =
        crate::composite::is_composite(model, diagram) && crate::composite::is_structured(kind);
    let mut presentation = if fragment {
        umlcraft_model::Presentation::fragment(element, bounds)
    } else {
        umlcraft_model::Presentation::node(Some(element), bounds)
    };
    // New class boxes size themselves to their content, as in Astah.
    presentation.style.auto_size = crate::scene::is_boxed_classifier(kind) && !structured;
    if is_container(kind) || fragment || structured {
        let id = presentation.id;
        let index = behind_contents(model, diagram, bounds);
        tx.apply(umlcraft_model::Op::InsertPresentation { diagram, index, presentation })?;
        Ok(id)
    } else {
        tx.add_presentation(diagram, presentation)
    }
}

/// Where a new container at `bounds` goes in the drawing order: behind what is drawn there
/// already, but in front of the containers drawn around it (a substate in front of its
/// composite state, a nested package in front of the outer one), each of which is larger and
/// holds its centre.
fn behind_contents(model: &Model, diagram: Id, bounds: Rect) -> usize {
    let Some(view) = model.view(diagram) else { return 0 };
    let centre = bounds.center();
    let area = bounds.width * bounds.height;
    view.presentations()
        .iter()
        .rposition(|p| p.bounds().is_some_and(|b| b.contains(centre) && b.width * b.height > area))
        .map_or(0, |i| i + 1)
}

/// Size of a new class or component on a composite structure diagram, with room for parts.
pub const STRUCTURED_SIZE: Size = Size { width: 320.0, height: 200.0 };

/// The closest element, starting at `start` and walking up the owners, that may own `kind`.
pub fn owner_for(model: &Model, start: Id, kind: &ElementKind) -> Option<Id> {
    let mut current = Some(start);
    while let Some(id) = current {
        let element = model.get(id)?;
        if element.kind().can_own(kind) {
            return Some(id);
        }
        current = element.owner();
    }
    None
}

/// `base` followed by the smallest number ≥ 1 not yet used by a sibling under `owner`.
pub fn unique_name(model: &Model, owner: Id, base: &str) -> String {
    let taken = |name: &str| !model.children_named(owner, name).is_empty();
    (1..).map(|n| format!("{base}{n}")).find(|name| !taken(name)).expect("names are unbounded")
}

/// Creates a new element for `tool` and shows it on `diagram` at `origin`.
/// Returns (element, presentation).
pub fn create_node(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    origin: Point,
    base_name: &str,
) -> Result<(Id, Id)> {
    if matches!(tool, NodeTool::CombinedFragment | NodeTool::InteractionUse) {
        return create_fragment(tx, diagram, tool, None, Vec::new(), Some(origin));
    }
    if matches!(tool, NodeTool::Part | NodeTool::Port) {
        return crate::composite::create_node(tx, diagram, tool, origin, base_name);
    }
    if matches!(tool, NodeTool::InputPin | NodeTool::OutputPin | NodeTool::ActivityParameter) {
        return crate::activity::create_node(tx, diagram, tool, origin, base_name);
    }
    if matches!(tool, NodeTool::EntryPoint | NodeTool::ExitPoint | NodeTool::StubState) {
        return crate::state_edit::create_node(tx, diagram, tool, origin, base_name);
    }
    if matches!(tool, NodeTool::StateInvariant | NodeTool::Termination) {
        let termination = tool == NodeTool::Termination;
        let model = tx.model();
        let interaction = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
        let name =
            if termination { String::new() } else { unique_name(model, interaction, base_name) };
        return sequence_edit::create_on_lifeline(tx, diagram, termination, origin, &name);
    }
    let kind = tool.element_kind();
    let diagram_owner = tx.model().element(diagram)?.owner().ok_or(ModelError::Root)?;
    let mut bounds = Rect::from_origin_size(origin, tool.default_size());
    // A structured class has room for its parts.
    if crate::composite::is_composite(tx.model(), diagram) && crate::composite::is_structured(&kind)
    {
        bounds.width = bounds.width.max(STRUCTURED_SIZE.width);
        bounds.height = bounds.height.max(STRUCTURED_SIZE.height);
    }
    // By its center, as moved vertices are; never into a submachine state.
    let enclosing = if kind.is_vertex() {
        let model = tx.model();
        let submachine = |s: Id| crate::state_edit::is_submachine(model, s);
        crate::state_edit::vertex_owner_at(model, diagram, bounds.center(), &submachine)
    } else {
        None
    };
    let owner = match enclosing {
        Some(state) => state,
        None => owner_for(tx.model(), diagram_owner, &kind).ok_or_else(|| {
            ModelError::Invalid(format!("no owner for a new {}", kind.type_name()))
        })?,
    };
    let name =
        if tool.is_named() { unique_name(tx.model(), owner, base_name) } else { String::new() };
    let mut data = tool.element_data(name);
    if tool == NodeTool::SubmachineState
        && let ElementKind::State(p) = &mut data.kind
    {
        p.submachine = Some(crate::state_edit::submachine_for(tx, diagram)?);
    }
    let element = tx.create(owner, data)?;
    let node = add_node_for(tx, diagram, element, bounds)?;
    // Robustness classes are drawn as their icon, as Astah makes them.
    if tool.robustness_stereotype().is_some() {
        tx.update_presentation(diagram, node, |p| p.style.iconic = true)?;
    }
    Ok((element, node))
}

/// Connects two presentations of `diagram` with `tool`. Creates the relationship element in the
/// namespace of the source element and returns the new edge presentation.
pub fn connect(tx: &mut Tx<'_>, diagram: Id, tool: EdgeTool, source: Id, target: Id) -> Result<Id> {
    if tool.message_sort().is_some() {
        return connect_message(tx, diagram, tool, source, target, None);
    }
    if tool == EdgeTool::Connector {
        return crate::composite::connect(tx, diagram, source, target);
    }
    if tool == EdgeTool::ErLine {
        return crate::er::connect(tx, diagram, source, target);
    }
    if let Some(line) = crate::er::IeLine::of(tool) {
        return crate::er::relate(tx, diagram, line, source, target);
    }
    if matches!(tool, EdgeTool::DurationConstraint | EdgeTool::TimeConstraint) {
        if (source == target) != (tool == EdgeTool::TimeConstraint) {
            return Err(ModelError::Invalid(sequence_edit::NOT_ON_MESSAGE.into()));
        }
        return sequence_edit::constrain(tx, diagram, source, target);
    }
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let element_of = |p: Id| -> Result<Id> {
        view.get(p).ok_or(ModelError::NotFound(p))?.element.ok_or(ModelError::NotFound(p))
    };
    let (s, t) = (element_of(source)?, element_of(target)?);
    let (sk, tk) = (model.element(s)?.kind(), model.element(t)?.kind());
    // Only associations and transitions may loop back to the same node; they get a loop of
    // waypoints.
    let self_loop = source == target;
    let may_loop = matches!(
        tool,
        EdgeTool::Association | EdgeTool::AssociationClass | EdgeTool::Link | EdgeTool::Transition
    );
    if (self_loop && !may_loop) || !tool.accepts(sk, tk, s == t) {
        let (tool, from, to) = (tool.key(), sk.type_name(), tk.type_name());
        return Err(ModelError::Refused(umlcraft_model::Refusal::CannotConnect { tool, from, to }));
    }

    match tool.element_kind(s, t) {
        Some(kind) => {
            let start = model.element(s)?.owner().unwrap_or(model.root());
            let owner = owner_for(model, start, &kind)
                .ok_or_else(|| ModelError::Invalid(format!("no owner for {}", kind.type_name())))?;
            let waypoints = match view.get(source).and_then(|p| p.bounds()) {
                Some(bounds) if self_loop => crate::scene::self_loop(bounds),
                _ => Vec::new(),
            };
            let element = tx.create(owner, ElementData::new("", kind))?;
            let edge = tx.add_edge(diagram, Some(element), source, target, waypoints)?;
            if tool == EdgeTool::AssociationClass {
                let owner = tx.model().element(element)?.owner().unwrap_or(owner);
                let name = unique_name(tx.model(), owner, ASSOCIATION_CLASS_NAME);
                make_association_class(tx, diagram, edge, &name)?;
            }
            Ok(edge)
        }
        None => {
            // Note anchor: the comment annotates the other element.
            let (note, other) = if matches!(sk, ElementKind::Comment(_)) { (s, t) } else { (t, s) };
            tx.update(note, |data| {
                if let ElementKind::Comment(p) = &mut data.kind
                    && !p.annotated.contains(&other)
                {
                    p.annotated.push(other);
                }
            })?;
            tx.add_edge(diagram, None, source, target, Vec::new())
        }
    }
}

/// The name a new association class gets before a number, as in Astah ("AssociationClass0").
pub const ASSOCIATION_CLASS_NAME: &str = "AssociationClass";

/// Turns the association that the edge `edge` of `diagram` shows into an association class
/// (Astah's "Convert to association class"): a new class named `name` beside the association,
/// shown below the middle of the line, which the drawing joins to it with a dashed line.
/// Returns the class and its node.
pub fn make_association_class(
    tx: &mut Tx<'_>,
    diagram: Id,
    edge: Id,
    name: &str,
) -> Result<(Id, Id)> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let presentation = view.get(edge).ok_or(ModelError::NotFound(edge))?;
    let association = presentation.element.ok_or(ModelError::NotFound(edge))?;
    let ElementKind::Association(a) = model.element(association)?.kind() else {
        return Err(ModelError::Invalid(format!("{association} is no association")));
    };
    if a.class.is_some() {
        return Err(ModelError::Refused(umlcraft_model::Refusal::HasAssociationClass));
    }
    let Shape::Edge { source, target, waypoints, .. } = &presentation.shape else {
        return Err(ModelError::Invalid(format!("{edge} is no edge")));
    };
    let center = |id: &Id| view.get(*id).and_then(|p| p.bounds()).map(|b| b.center());
    let (Some(a), Some(b)) = (center(source), center(target)) else {
        return Err(ModelError::Invalid("the line's ends are not on the diagram".into()));
    };
    // The middle of the line through its waypoints, roughly where the dashed line will end.
    let mut points = vec![a];
    points.extend(waypoints.iter().copied());
    points.push(b);
    let middle = points[points.len() / 2 - usize::from(points.len() % 2 == 0)];
    let next = points[points.len() / 2];
    let middle = Point::new((middle.x + next.x) / 2.0, (middle.y + next.y) / 2.0);
    let owner = model.element(association)?.owner().ok_or(ModelError::Root)?;
    let owner = owner_for(model, owner, &ElementKind::class()).ok_or(ModelError::Root)?;
    let class = tx.create(owner, ElementData::new(name, ElementKind::class()))?;
    tx.update(association, |data| {
        if let ElementKind::Association(a) = &mut data.kind {
            a.class = Some(class);
        }
    })?;
    let size = NodeTool::Class.default_size();
    let origin = Point::new(middle.x - size.width / 2.0, middle.y + 50.0);
    let node = add_node_for(tx, diagram, class, Rect::from_origin_size(origin, size))?;
    Ok((class, node))
}

/// The relationships "Change kind" turns into each other.
pub const RELATIONSHIP_TOOLS: [EdgeTool; 4] =
    [EdgeTool::Association, EdgeTool::Generalization, EdgeTool::Realization, EdgeTool::Dependency];

/// Which of [`RELATIONSHIP_TOOLS`] makes an element of `kind`.
pub fn relationship_tool(kind: &ElementKind) -> Option<EdgeTool> {
    match kind {
        ElementKind::Association(_) => Some(EdgeTool::Association),
        ElementKind::Generalization(_) => Some(EdgeTool::Generalization),
        ElementKind::Realization(_) => Some(EdgeTool::Realization),
        ElementKind::Dependency(_) => Some(EdgeTool::Dependency),
        _ => None,
    }
}

/// Turns the relationship the edge `edge` of `diagram` shows into one made by `tool`, between
/// the same elements: the line's source becomes the first end, the specific element or the
/// client. The new element keeps what the old one says besides its kind and takes its place on
/// every diagram, where the line keeps its route and labels; the old one is deleted. Returns
/// the new element.
pub fn change_relationship(tx: &mut Tx<'_>, diagram: Id, edge: Id, tool: EdgeTool) -> Result<Id> {
    let model = tx.model();
    let (old, s, t) = relationship_ends(model, diagram, edge)?;
    let element = model.element(old)?;
    if relationship_tool(element.kind()) == Some(tool) {
        return Ok(old);
    }
    let kind = tool.element_kind(s, t).filter(|_| may_become(model, s, t, tool));
    let Some(kind) = kind else {
        return Err(ModelError::Invalid(format!(
            "{} cannot connect {} to {}",
            tool.key(),
            model.element(s)?.kind().type_name(),
            model.element(t)?.kind().type_name()
        )));
    };
    let start = model.element(s)?.owner().unwrap_or(model.root());
    let owner = owner_for(model, start, &kind)
        .ok_or_else(|| ModelError::Invalid(format!("no owner for {}", kind.type_name())))?;
    // Name, documentation, stereotypes and tagged values stay; «bind» belongs to a dependency.
    let mut data = ElementData { kind, ..element.data().clone() };
    if tool != EdgeTool::Dependency {
        data.stereotypes.retain(|s| s != "bind");
    }
    let shown = model.shown_on(old);
    let new = tx.create(owner, data)?;
    for (diagram, presentation) in shown {
        tx.update_presentation(diagram, presentation, |p| p.element = Some(new))?;
    }
    tx.delete(old)?;
    Ok(new)
}

/// What the edge `edge` of `diagram` is for "Change kind": the relationship it shows, and for
/// each of [`RELATIONSHIP_TOOLS`] whether it may become one of those. `None` for other edges.
pub fn relationship_kinds(
    model: &Model,
    diagram: Id,
    edge: Id,
) -> Option<(EdgeTool, Vec<(EdgeTool, bool)>)> {
    let (element, s, t) = relationship_ends(model, diagram, edge).ok()?;
    let current = relationship_tool(model.get(element)?.kind())?;
    let kinds = RELATIONSHIP_TOOLS.iter().map(|&tool| (tool, may_become(model, s, t, tool)));
    Some((current, kinds.collect()))
}

/// The element an edge shows, with the elements at its source and its target.
fn relationship_ends(model: &Model, diagram: Id, edge: Id) -> Result<(Id, Id, Id)> {
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let presentation = view.get(edge).ok_or(ModelError::NotFound(edge))?;
    let element = presentation.element.ok_or(ModelError::NotFound(edge))?;
    let Shape::Edge { source, target, .. } = &presentation.shape else {
        return Err(ModelError::Invalid(format!("{edge} is no edge")));
    };
    let element_of = |p: Id| view.get(p).and_then(|p| p.element).ok_or(ModelError::NotFound(p));
    Ok((element, element_of(*source)?, element_of(*target)?))
}

/// Whether a relationship from `source` to `target` may be one that `tool` makes; only
/// associations loop.
fn may_become(model: &Model, source: Id, target: Id, tool: EdgeTool) -> bool {
    let (Some(s), Some(t)) = (model.get(source), model.get(target)) else { return false };
    let loops = source == target && tool != EdgeTool::Association;
    RELATIONSHIP_TOOLS.contains(&tool)
        && !loops
        && tool.accepts(s.kind(), t.kind(), source == target)
}

/// Owner and index for something new in the sequence diagram's interaction: `slot`, or else
/// after everything.
fn slot_or_end(model: &Model, diagram: Id, slot: Option<Slot>) -> Result<(Id, usize)> {
    match slot {
        Some(slot) => Ok((slot.owner, slot.index)),
        None => {
            let interaction = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
            Ok((interaction, model.children(interaction).len()))
        }
    }
}

/// Draws a message from one lifeline of a sequence diagram to another (or the same) at the
/// height of `slot`, or below everything. An activation bar stands for its lifeline. Returns
/// the new message presentation.
pub fn connect_message(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: EdgeTool,
    source: Id,
    target: Id,
    slot: Option<Slot>,
) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let (source, target) = (lifeline_of(model, view, source), lifeline_of(model, view, target));
    let element_of = |p: Id| -> Result<Id> {
        view.get(p).ok_or(ModelError::NotFound(p))?.element.ok_or(ModelError::NotFound(p))
    };
    let (s, t) = (element_of(source)?, element_of(target)?);
    let (sk, tk) = (model.element(s)?.kind(), model.element(t)?.kind());
    let kind =
        tool.element_kind(s, t).filter(|_| tool.accepts(sk, tk, s == t)).ok_or_else(|| {
            ModelError::Invalid(format!(
                "{} cannot connect {} to {}",
                tool.key(),
                sk.type_name(),
                tk.type_name()
            ))
        })?;
    let y = slot.map_or_else(|| sequence_edit::next_y(model, diagram), |s| s.y);
    let (owner, index) = slot_or_end(model, diagram, slot)?;
    // Added below everything: sent from the call the sender is busy with, if any.
    let sends = !matches!(tool, EdgeTool::Reply | EdgeTool::FoundMessage);
    if slot.is_none() && sends {
        sequence_edit::continue_open_bar(tx, diagram, source, y)?;
    }
    let message = tx.create_at(owner, index, ElementData::new("", kind))?;
    sequence_edit::add_message(tx, diagram, message, source, target, y)
}

/// The lifeline an activation bar `presentation` lies on, else `presentation` itself.
pub fn lifeline_of(model: &Model, view: &View, presentation: Id) -> Id {
    let Some(p) = view.get(presentation) else { return presentation };
    if let Shape::Activation { lifeline, .. } = p.shape {
        return lifeline;
    }
    // So do a state invariant and a termination drawn on it.
    match p.element.and_then(|e| model.get(e)).map(|e| e.kind()) {
        Some(
            ElementKind::StateInvariant(StateInvariantProps { lifeline })
            | ElementKind::Termination(TerminationProps { lifeline }),
        ) => sequence_edit::lane_of(view, *lifeline).unwrap_or(presentation),
        _ => presentation,
    }
}

/// Puts a new combined fragment with one empty operand, or an interaction use (`tool`), at the
/// height of `slot`, else at `origin`, else below everything, covering the lifelines `covered`.
/// Returns (element, node).
pub fn create_fragment(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    slot: Option<Slot>,
    covered: Vec<Id>,
    origin: Option<Point>,
) -> Result<(Id, Id)> {
    let (owner, index) = slot_or_end(tx.model(), diagram, slot)?;
    let y = match (slot, origin) {
        (Some(slot), _) => slot.y,
        (None, Some(origin)) => origin.y,
        (None, None) => sequence_edit::next_y(tx.model(), diagram),
    };
    let kind = if tool == NodeTool::InteractionUse {
        ElementKind::InteractionUse(InteractionUseProps {
            covered: covered.clone(),
            ..Default::default()
        })
    } else {
        ElementKind::CombinedFragment(CombinedFragmentProps {
            operator: InteractionOperator::Opt,
            covered: covered.clone(),
        })
    };
    let fragment = tx.create_at(owner, index, ElementData::new("", kind))?;
    if tool != NodeTool::InteractionUse {
        let operand = ElementKind::InteractionOperand(InteractionOperandProps::default());
        tx.create(fragment, ElementData::new("", operand))?;
    }
    let node = sequence_edit::add_fragment(tx, diagram, fragment, &covered, y)?;
    Ok((fragment, node))
}

/// Adds an empty operand at the end of `fragment`, with room for it in its frames.
pub fn add_operand(tx: &mut Tx<'_>, fragment: Id) -> Result<Id> {
    let operand = ElementKind::InteractionOperand(InteractionOperandProps::default());
    let id = tx.create(fragment, ElementData::new("", operand))?;
    sequence_edit::add_operand_space(tx, fragment)?;
    Ok(id)
}

/// Gives `fragment` a second operand guarded `else` when it has become an `alt` with a single
/// operand, since an alternative comes with its other case. Returns the new operand.
pub fn add_else(tx: &mut Tx<'_>, fragment: Id) -> Result<Option<Id>> {
    let model = tx.model();
    let alt = matches!(
        model.element(fragment)?.kind(),
        ElementKind::CombinedFragment(p) if p.operator == InteractionOperator::Alt
    );
    let operands = model
        .children(fragment)
        .iter()
        .filter(|&&c| {
            matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::InteractionOperand(_)))
        })
        .count();
    if !alt || operands != 1 {
        return Ok(None);
    }
    let operand = add_operand(tx, fragment)?;
    tx.update(operand, |d| {
        if let ElementKind::InteractionOperand(p) = &mut d.kind {
            p.guard = "else".into();
        }
    })?;
    Ok(Some(operand))
}

/// Makes `class` bind the template and arguments of `binding` (`Pair<int,Holder>`), or nothing
/// with `None`. The class keeps one «bind» dependency on its template, drawn on `diagram` where
/// that shows both; dependencies on a former template go.
pub fn set_binding(
    tx: &mut Tx<'_>,
    diagram: Option<Id>,
    class: Id,
    binding: Option<TypeRef>,
) -> Result<()> {
    let template = match &binding {
        None => None,
        Some(TypeRef::Expr(e)) => match e.base {
            TypeRef::Element(t) if tx.model().get(t).is_some_and(|t| t.kind().is_classifier()) => {
                Some(t)
            }
            _ => None,
        },
        Some(_) => None,
    };
    if binding.is_some() && template.is_none() {
        return Err(ModelError::Refused(umlcraft_model::Refusal::BindingWithoutTemplate));
    }
    tx.update(class, |data| {
        if let ElementKind::Class(p) = &mut data.kind {
            p.binding = binding;
        }
    })?;
    let binds: Vec<(Id, Id)> = tx
        .model()
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Dependency(d)
                if d.client == class && e.data().stereotypes.iter().any(|s| s == "bind") =>
            {
                Some((e.id(), d.supplier))
            }
            _ => None,
        })
        .collect();
    let stale: Vec<Id> =
        binds.iter().filter(|&&(_, s)| Some(s) != template).map(|&(id, _)| id).collect();
    if !stale.is_empty() {
        tx.delete_many(&stale)?;
    }
    let Some(template) = template else { return Ok(()) };
    let dependency = match binds.iter().find(|&&(_, s)| s == template) {
        Some(&(id, _)) => id,
        None => {
            let kind = ElementKind::Dependency(DirectedProps { client: class, supplier: template });
            let start = tx.model().element(class)?.owner().unwrap_or(tx.model().root());
            let owner = owner_for(tx.model(), start, &kind).unwrap_or(tx.model().root());
            let mut data = ElementData::new("", kind);
            data.stereotypes = vec!["bind".into()];
            tx.create(owner, data)?
        }
    };
    let Some(diagram) = diagram else { return Ok(()) };
    let Some(view) = tx.model().view(diagram) else { return Ok(()) };
    let node_of =
        |e: Id| view.presentations().iter().find(|p| p.element == Some(e) && !p.is_edge());
    let drawn = view.presentations().iter().any(|p| p.element == Some(dependency));
    if let (Some(from), Some(to), false) = (node_of(class), node_of(template), drawn) {
        let (from, to) = (from.id, to.id);
        tx.add_edge(diagram, Some(dependency), from, to, Vec::new())?;
    }
    Ok(())
}

/// Puts the messages and fragments `items` into a new combined fragment, together with
/// everything between them. They must all be in the same interaction or operand.
/// Returns (fragment, node).
pub fn wrap_in_fragment(tx: &mut Tx<'_>, diagram: Id, items: &[Id]) -> Result<(Id, Id)> {
    let model = tx.model();
    let invalid = |msg: &str| ModelError::Invalid(msg.to_owned());
    let mut owners = items.iter().map(|&i| model.element(i).map(|e| e.owner()));
    let owner = owners.next().ok_or_else(|| invalid("nothing to put into a fragment"))??;
    if owners.any(|o| o.ok().flatten() != owner) {
        return Err(invalid("only messages at the same level can go into one fragment"));
    }
    let owner = owner.ok_or(ModelError::Root)?;
    let children = model.children(owner);
    let positions: Vec<usize> =
        items.iter().filter_map(|i| children.iter().position(|c| c == i)).collect();
    let (Some(&first), Some(&last)) = (positions.iter().min(), positions.iter().max()) else {
        return Err(invalid("nothing to put into a fragment"));
    };
    let moved: Vec<Id> = children[first..=last]
        .iter()
        .copied()
        .filter(|&c| {
            model.get(c).is_some_and(|e| {
                matches!(
                    e.kind(),
                    ElementKind::Message(_)
                        | ElementKind::CombinedFragment(_)
                        | ElementKind::InteractionUse(_)
                )
            })
        })
        .collect();
    if moved.is_empty() {
        return Err(invalid("only messages and fragments can go into a fragment"));
    }
    // The frame goes around what the items span on the diagram; the order then puts them into
    // its operand.
    let (top, bottom, lifelines) = sequence_edit::extent(tx.model(), diagram, &moved);
    // A message at the top keeps its label below the tab.
    let topmost = tx.model().view(diagram).and_then(|view| {
        view.presentations()
            .iter()
            .find_map(|p| match p.shape {
                Shape::Message { y, .. } if moved.iter().any(|&m| p.element == Some(m)) => Some(y),
                _ => None,
            })
            .filter(|&y| (y - top).abs() < 0.5)
    });
    let label = if topmost.is_some() { sequence::LABEL_ABOVE } else { 0.0 };
    // ...as far as the item before it leaves room: a frame reaching into that would take it in.
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let bottom_of = |item: Id| {
        view.presentations_of(item).find_map(|p| match p.shape {
            Shape::Message { y, end_y, .. } => Some(y.max(end_y.unwrap_or(y))),
            Shape::Fragment { bounds, .. } | Shape::Node { bounds, .. } => Some(bounds.bottom()),
            _ => None,
        })
    };
    let floor = model.children(owner)[..first].iter().rev().find_map(|&c| bottom_of(c));
    let lowest = top - sequence::TAB_HEIGHT - 10.0;
    let y = floor.map_or(lowest - label, |f| (lowest - label).max(f + 10.0)).min(lowest);
    let slot = Slot { y, owner, index: first };
    let (fragment, node) =
        create_fragment(tx, diagram, NodeTool::CombinedFragment, Some(slot), lifelines, None)?;
    tx.update_presentation(diagram, node, |p| {
        if let umlcraft_model::Shape::Fragment { bounds, .. } = &mut p.shape {
            bounds.height = bounds.height.max(bottom + 20.0 - bounds.y);
        }
    })?;
    sequence_edit::tidy(tx, diagram)?;
    Ok((fragment, node))
}

/// Moves a message or fragment to the height of `slot`, e.g. while dragging it up or down.
pub fn move_to_slot(tx: &mut Tx<'_>, item: Id, slot: Slot) -> Result<()> {
    let diagrams: Vec<Id> = tx
        .model()
        .views()
        .filter(|(_, view)| view.presentations_of(item).next().is_some())
        .map(|(diagram, _)| diagram)
        .collect();
    for diagram in diagrams {
        sequence_edit::move_item(tx, diagram, item, slot.y)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::Shape;
    use umlcraft_model::sample::order_system;

    #[test]
    fn an_alt_gets_its_else_once() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let operands = |model: &Model, f: Id| -> Vec<String> {
            model
                .children(f)
                .iter()
                .filter_map(|&c| match model.get(c)?.kind() {
                    ElementKind::InteractionOperand(p) => Some(p.guard.clone()),
                    _ => None,
                })
                .collect()
        };
        let (fragment, _) = doc
            .edit("fragment", |tx| {
                create_fragment(tx, d, NodeTool::CombinedFragment, None, Vec::new(), None)
            })
            .unwrap();
        // An opt stays as it is.
        doc.edit("else", |tx| add_else(tx, fragment)).unwrap();
        assert_eq!(operands(doc.model(), fragment), [""]);
        doc.edit("alt", |tx| {
            tx.update(fragment, |data| {
                if let ElementKind::CombinedFragment(p) = &mut data.kind {
                    p.operator = InteractionOperator::Alt;
                }
            })?;
            add_else(tx, fragment)
        })
        .unwrap();
        assert_eq!(operands(doc.model(), fragment), ["", "else"]);
        // Two operands are enough.
        doc.edit("else", |tx| add_else(tx, fragment)).unwrap();
        assert_eq!(operands(doc.model(), fragment), ["", "else"]);
    }

    #[test]
    fn bindings_keep_one_bind_line() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let at = |x| Point::new(x, 400.0);
        let (class, templates) = doc
            .edit("classes", |tx| {
                let mut templates = Vec::new();
                for x in [100.0, 300.0] {
                    let (t, _) = create_node(tx, d, NodeTool::Class, at(x), "Template")?;
                    tx.create(
                        t,
                        ElementData::new("T", ElementKind::TemplateParameter(Default::default())),
                    )?;
                    templates.push(t);
                }
                let (class, _) = create_node(tx, d, NodeTool::Class, at(500.0), "Bound")?;
                Ok((class, templates))
            })
            .unwrap();
        let bind_of = |model: &Model| -> Vec<Id> {
            model
                .elements()
                .filter_map(|e| match e.kind() {
                    ElementKind::Dependency(p) if p.client == class => Some(p.supplier),
                    _ => None,
                })
                .collect()
        };
        let edges = |model: &Model| {
            model.view(d).unwrap().presentations().iter().filter(|p| p.is_edge()).count()
        };
        let before = edges(doc.model());
        let binding = |t| {
            Some(TypeRef::Expr(Box::new(umlcraft_model::TypeExpr {
                base: TypeRef::Element(t),
                arguments: vec![TypeRef::Text("int".into())],
                prefix: String::new(),
                modifier: String::new(),
            })))
        };
        for &t in &templates {
            doc.edit("bind", |tx| set_binding(tx, Some(d), class, binding(t))).unwrap();
            assert_eq!(bind_of(doc.model()), [t]);
            assert_eq!(edges(doc.model()), before + 1);
        }
        doc.edit("unbind", |tx| set_binding(tx, Some(d), class, None)).unwrap();
        assert!(bind_of(doc.model()).is_empty());
        assert_eq!(edges(doc.model()), before);
        // A binding names a template.
        let text = Some(TypeRef::Text("int".into()));
        assert!(doc.edit("bind", |tx| set_binding(tx, Some(d), class, text)).is_err());
    }

    #[test]
    fn palettes_match_diagram_kinds() {
        let class = palette(DiagramKind::Class);
        assert_eq!(class[0], Tool::Select);
        assert!(class.contains(&Tool::Node(NodeTool::Interface)));
        assert!(!class.contains(&Tool::Node(NodeTool::Actor)));
        let use_case = palette(DiagramKind::UseCase);
        assert!(use_case.contains(&Tool::Edge(EdgeTool::Include)));
        assert!(palette(DiagramKind::Sequence).contains(&Tool::Node(NodeTool::Lifeline)));
    }

    #[test]
    fn an_object_diagram_offers_objects_and_links_first() {
        let (ElementKind::Diagram(class), ElementKind::Diagram(object)) =
            (ElementKind::diagram(DiagramKind::Class), ElementKind::object_diagram())
        else {
            unreachable!()
        };
        assert_eq!(diagram_palette(&class), palette(DiagramKind::Class));
        let objects = diagram_palette(&object);
        assert_eq!(
            objects[..3],
            [Tool::Select, Tool::Node(NodeTool::Object), Tool::Edge(EdgeTool::Link)]
        );
        assert_eq!(objects[3], Tool::Node(NodeTool::Class));
        assert_eq!(objects.len(), palette(DiagramKind::Class).len());
    }

    #[test]
    fn handles_offer_the_lines_allowed_between_two_elements() {
        let props = |kind| match ElementKind::diagram(kind) {
            ElementKind::Diagram(p) => p,
            _ => unreachable!(),
        };
        let (class, interface) = (ElementKind::class(), ElementKind::interface());
        let classes = props(DiagramKind::Class);
        use EdgeTool as E;
        assert_eq!(
            connections(&classes, &class, &class, false),
            [E::Association, E::AssociationClass, E::Generalization, E::Dependency]
        );
        assert_eq!(
            connections(&classes, &class, &interface, false),
            [E::Association, E::AssociationClass, E::Realization, E::Dependency]
        );
        assert_eq!(
            connections(&classes, &class, &class, true),
            [E::Association, E::AssociationClass]
        );
        let (actor, use_case) = (ElementKind::actor(), ElementKind::use_case());
        let use_cases = props(DiagramKind::UseCase);
        assert_eq!(
            connections(&use_cases, &actor, &use_case, false),
            [E::Association, E::Dependency]
        );
        assert_eq!(
            connections(&use_cases, &use_case, &use_case, false),
            [E::Association, E::Generalization, E::Include, E::Extend, E::Dependency]
        );
        let lifeline = ElementKind::Lifeline(Default::default());
        let sequence = props(DiagramKind::Sequence);
        assert_eq!(
            connections(&sequence, &lifeline, &lifeline, false),
            [E::Message, E::AsyncMessage, E::Reply, E::CreateMessage, E::DestroyMessage]
        );
        assert_eq!(
            connections(&sequence, &lifeline, &lifeline, true),
            [E::Message, E::AsyncMessage, E::Reply]
        );
    }

    #[test]
    fn connection_rules() {
        let class = ElementKind::class();
        let interface = ElementKind::interface();
        let actor = ElementKind::actor();
        let use_case = ElementKind::use_case();
        let note = ElementKind::Comment(CommentProps::default());
        assert!(EdgeTool::Association.accepts(&class, &class, true));
        assert!(EdgeTool::Association.accepts(&actor, &use_case, false));
        assert!(EdgeTool::Generalization.accepts(&class, &class, false));
        assert!(!EdgeTool::Generalization.accepts(&class, &class, true));
        assert!(!EdgeTool::Generalization.accepts(&class, &interface, false));
        assert!(EdgeTool::Realization.accepts(&class, &interface, false));
        assert!(!EdgeTool::Realization.accepts(&interface, &class, false));
        assert!(EdgeTool::Include.accepts(&use_case, &use_case, false));
        assert!(!EdgeTool::Include.accepts(&actor, &use_case, false));
        assert!(EdgeTool::NoteAnchor.accepts(&note, &class, false));
        assert!(EdgeTool::NoteAnchor.accepts(&class, &note, false));
        assert!(!EdgeTool::NoteAnchor.accepts(&note, &note, false));
    }

    #[test]
    fn links_connect_objects() {
        let object = ElementKind::Instance(InstanceProps {
            instance: InstanceKind::Object,
            classifier: None,
        });
        assert!(palette(DiagramKind::Class).contains(&Tool::Edge(EdgeTool::Link)));
        assert!(EdgeTool::Link.accepts(&object, &object, false));
        assert!(EdgeTool::Link.accepts(&object, &object, true));
        assert!(!EdgeTool::Link.accepts(&object, &ElementKind::class(), false));

        let (mut doc, ids) = order_system();
        let d = ids.object_diagram;
        let link = doc
            .edit("link", |tx| {
                let (_, a) = create_node(tx, d, NodeTool::Object, Point::new(100.0, 300.0), "")?;
                let (_, b) = create_node(tx, d, NodeTool::Object, Point::new(400.0, 300.0), "")?;
                connect(tx, d, EdgeTool::Link, a, b)
            })
            .unwrap();
        let model = doc.model();
        let element = model.view(d).unwrap().get(link).unwrap().element.unwrap();
        assert_eq!(model.get(element).unwrap().owner(), Some(ids.domain));
        assert!(matches!(model.get(element).unwrap().kind(), ElementKind::Link(_)));
        model.check().unwrap();
    }

    #[test]
    fn create_and_connect_on_the_class_diagram() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (a, b, edge) = doc
            .edit("create", |tx| {
                let (a, na) =
                    create_node(tx, d, NodeTool::Class, Point::new(600.0, 80.0), "Class")?;
                let (b, nb) =
                    create_node(tx, d, NodeTool::Interface, Point::new(600.0, 300.0), "Interface")?;
                let edge = connect(tx, d, EdgeTool::Realization, na, nb)?;
                Ok((a, b, edge))
            })
            .unwrap();
        let model = doc.model();
        model.check().unwrap();
        // New elements land in the diagram's package with generated names.
        assert_eq!(model.get(a).unwrap().owner(), Some(ids.domain));
        assert_eq!(model.get(a).unwrap().name(), "Class1");
        assert_eq!(model.get(b).unwrap().name(), "Interface1");
        let realization = model.view(d).unwrap().get(edge).unwrap().element.unwrap();
        assert_eq!(model.get(realization).unwrap().owner(), Some(ids.domain));

        // Wrong direction is refused and leaves nothing behind.
        let before = doc.model().clone();
        let (na, nb) = (doc.model().shown_on(a)[0].1, doc.model().shown_on(b)[0].1);
        assert!(doc.edit("bad", |tx| connect(tx, d, EdgeTool::Realization, nb, na)).is_err());
        assert_eq!(*doc.model(), before);
    }

    #[test]
    fn a_relationship_changes_its_kind_in_place() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (a, b, na, ni, edge) = doc
            .edit("create", |tx| {
                let (a, na) = create_node(tx, d, NodeTool::Class, Point::new(600.0, 80.0), "A")?;
                let (b, nb) = create_node(tx, d, NodeTool::Class, Point::new(600.0, 300.0), "B")?;
                let (_, ni) =
                    create_node(tx, d, NodeTool::Interface, Point::new(900.0, 300.0), "I")?;
                let edge = connect(tx, d, EdgeTool::Association, na, nb)?;
                Ok((a, b, na, ni, edge))
            })
            .unwrap();
        let association = doc.model().view(d).unwrap().get(edge).unwrap().element.unwrap();
        doc.edit("name", |tx| {
            tx.update(association, |data| {
                data.name = "uses".into();
                data.stereotypes = vec!["weak".into()];
            })
        })
        .unwrap();

        // The line's source becomes the specific end; the same line now shows the new element.
        let general = doc
            .edit("change", |tx| change_relationship(tx, d, edge, EdgeTool::Generalization))
            .unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert!(model.get(association).is_none());
        assert_eq!(model.view(d).unwrap().get(edge).unwrap().element, Some(general));
        assert_eq!(model.get(general).unwrap().name(), "uses");
        assert_eq!(model.get(general).unwrap().data().stereotypes, ["weak"]);
        assert_eq!(model.generals_of(a), [b]);

        let dependency =
            doc.edit("again", |tx| change_relationship(tx, d, edge, EdgeTool::Dependency)).unwrap();
        assert!(matches!(
            doc.model().get(dependency).unwrap().kind(),
            ElementKind::Dependency(DirectedProps { client, supplier }) if (*client, *supplier) == (a, b)
        ));
        // A class does not realize a class: not offered, and refused, nothing changes.
        let (current, kinds) = relationship_kinds(doc.model(), d, edge).unwrap();
        assert_eq!(current, EdgeTool::Dependency);
        let offered: Vec<EdgeTool> = kinds.iter().filter(|k| k.1).map(|k| k.0).collect();
        assert_eq!(
            offered,
            [EdgeTool::Association, EdgeTool::Generalization, EdgeTool::Dependency]
        );
        let before = doc.model().clone();
        let refused = doc.edit("bad", |tx| change_relationship(tx, d, edge, EdgeTool::Realization));
        assert!(refused.is_err());
        assert_eq!(*doc.model(), before);
        // To an interface it does.
        let line = doc.edit("line", |tx| connect(tx, d, EdgeTool::Dependency, na, ni)).unwrap();
        doc.edit("realize", |tx| change_relationship(tx, d, line, EdgeTool::Realization)).unwrap();
        doc.model().check().unwrap();
    }

    #[test]
    fn lifelines_go_into_the_interaction_and_notes_annotate() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let (lifeline, note) = doc
            .edit("create", |tx| {
                let (l, nl) =
                    create_node(tx, d, NodeTool::Lifeline, Point::new(460.0, 40.0), "lifeline")?;
                let (n, nn) = create_node(tx, d, NodeTool::Note, Point::new(460.0, 400.0), "")?;
                connect(tx, d, EdgeTool::NoteAnchor, nn, nl)?;
                Ok((l, n))
            })
            .unwrap();
        let model = doc.model();
        assert_eq!(model.get(lifeline).unwrap().owner(), Some(interaction));
        let ElementKind::Comment(p) = model.get(note).unwrap().kind() else { panic!() };
        assert_eq!(p.annotated, vec![lifeline]);
    }

    #[test]
    fn showing_an_element_brings_its_relationships() {
        let (mut doc, ids) = order_system();
        let other = doc
            .edit("diagram", |tx| {
                tx.create(
                    ids.domain,
                    ElementData::new("Other", ElementKind::diagram(DiagramKind::Class)),
                )
            })
            .unwrap();
        doc.edit("show", |tx| {
            show_element(tx, other, ids.customer, Point::new(0.0, 0.0))?;
            show_element(tx, other, ids.order, Point::new(300.0, 0.0))?;
            show_element(tx, other, ids.premium, Point::new(0.0, 200.0))
        })
        .unwrap();
        let view = doc.model().view(other).unwrap();
        assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 2);
        // Twice is refused, and so is the wrong diagram kind.
        assert!(
            doc.edit("again", |tx| show_element(tx, other, ids.order, Point::default())).is_err()
        );
        assert!(
            doc.edit("wrong", |tx| show_element(
                tx,
                ids.use_case_diagram,
                ids.order,
                Point::default()
            ))
            .is_err()
        );
    }

    #[test]
    fn self_associations_loop() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let node = doc.model().shown_on(ids.customer)[0].1;
        let edge =
            doc.edit("loop", |tx| connect(tx, d, EdgeTool::Association, node, node)).unwrap();
        let p = doc.model().view(d).unwrap().get(edge).unwrap().clone();
        let umlcraft_model::Shape::Edge { waypoints, .. } = p.shape else { panic!() };
        assert_eq!(waypoints.len(), 3);
        assert!(
            doc.edit("bad", |tx| connect(tx, d, EdgeTool::Generalization, node, node)).is_err()
        );
    }

    #[test]
    fn flows_connect_activity_nodes_in_their_direction() {
        let node = ElementKind::activity_node;
        let (initial, action, last) = (
            node(ActivityNodeKind::Initial),
            node(ActivityNodeKind::Action),
            node(ActivityNodeKind::ActivityFinal),
        );
        assert!(EdgeTool::Flow.accepts(&initial, &action, false));
        assert!(EdgeTool::Flow.accepts(&action, &last, false));
        assert!(!EdgeTool::Flow.accepts(&action, &initial, false));
        assert!(!EdgeTool::Flow.accepts(&last, &action, false));
        assert!(!EdgeTool::Flow.accepts(&action, &action, true));
        assert!(!EdgeTool::Flow.accepts(&action, &ElementKind::class(), false));
    }

    #[test]
    fn activity_nodes_go_into_the_activity() {
        let (mut doc, ids) = order_system();
        let d = ids.activity_diagram;
        let activity = doc.model().get(d).unwrap().owner().unwrap();
        let (action, fork, flow) = doc
            .edit("create", |tx| {
                let (a, na) =
                    create_node(tx, d, NodeTool::Action, Point::new(600.0, 80.0), "Action")?;
                let (f, nf) = create_node(tx, d, NodeTool::Fork, Point::new(600.0, 200.0), "Fork")?;
                let flow = connect(tx, d, EdgeTool::Flow, na, nf)?;
                Ok((a, f, flow))
            })
            .unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(model.get(action).unwrap().owner(), Some(activity));
        assert_eq!(model.get(action).unwrap().name(), "Action1");
        // Control nodes stay unnamed.
        assert_eq!(model.get(fork).unwrap().name(), "");
        let flow = model.view(d).unwrap().get(flow).unwrap().element.unwrap();
        assert_eq!(model.get(flow).unwrap().owner(), Some(activity));
        // Partitions go behind everything.
        let (_, lane) = doc
            .edit("lane", |tx| {
                create_node(tx, d, NodeTool::Partition, Point::new(560.0, 40.0), "Partition")
            })
            .unwrap();
        assert_eq!(doc.model().view(d).unwrap().presentations()[0].id, lane);
    }

    #[test]
    fn transitions_and_substates() {
        let state = ElementKind::State(StateProps::default());
        let initial = ElementKind::pseudostate(PseudostateKind::Initial);
        let t = EdgeTool::Transition;
        assert!(t.accepts(&initial, &state, false));
        assert!(t.accepts(&state, &ElementKind::FinalState, false));
        assert!(t.accepts(&state, &state, true), "self-transition");
        assert!(!t.accepts(&state, &initial, false));
        assert!(!t.accepts(&ElementKind::FinalState, &state, false));
        assert!(!t.accepts(&initial, &initial, true));

        let (mut doc, ids) = order_system();
        let d = ids.state_diagram;
        let machine = doc.model().get(d).unwrap().owner().unwrap();
        let (outside, inside, loop_edge) = doc
            .edit("create", |tx| {
                let (a, na) = create_node(tx, d, NodeTool::State, Point::new(100.0, 400.0), "S")?;
                // (170, 140) lies in "Empty", which lies in "Open".
                let (b, _) = create_node(tx, d, NodeTool::Choice, Point::new(170.0, 140.0), "")?;
                let edge = connect(tx, d, EdgeTool::Transition, na, na)?;
                Ok((a, b, edge))
            })
            .unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(model.get(outside).unwrap().owner(), Some(machine));
        assert_eq!(model.get(model.get(inside).unwrap().owner().unwrap()).unwrap().name(), "Empty");
        let Shape::Edge { waypoints, .. } = &model.view(d).unwrap().get(loop_edge).unwrap().shape
        else {
            panic!()
        };
        assert_eq!(waypoints.len(), 3, "a self-transition loops");
    }

    #[test]
    fn a_new_substate_is_drawn_in_front_of_its_composite_state() {
        let (mut doc, ids) = order_system();
        let d = ids.state_diagram;
        let (outer, inner, around) = doc
            .edit("create", |tx| {
                let (_, outer) =
                    create_node(tx, d, NodeTool::State, Point::new(600.0, 600.0), "Outer")?;
                tx.set_bounds(d, outer, Rect::new(600.0, 600.0, 400.0, 300.0))?;
                let (_, inner) =
                    create_node(tx, d, NodeTool::State, Point::new(650.0, 680.0), "Inner")?;
                // A state drawn where nothing is goes to the back, as before.
                let (_, around) =
                    create_node(tx, d, NodeTool::State, Point::new(1200.0, 600.0), "Alone")?;
                Ok((outer, inner, around))
            })
            .unwrap();
        let view = doc.model().view(d).unwrap();
        let at = |id| view.presentations().iter().position(|p| p.id == id).unwrap();
        assert!(at(outer) < at(inner), "the substate is drawn after its composite state");
        assert_eq!(at(around), 0);
    }

    #[test]
    fn unique_names_fill_gaps() {
        let (doc, ids) = order_system();
        assert_eq!(unique_name(doc.model(), ids.domain, "Class"), "Class1");
        assert_eq!(unique_name(doc.model(), ids.domain, "Customer"), "Customer1");
    }
}
