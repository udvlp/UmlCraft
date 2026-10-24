//! Diagram tools: which elements a diagram offers, which connections are allowed, and the edits
//! that create them.

use umlcraft_model::{
    ActivityNodeKind, ClassifierProps, CombinedFragmentProps, CommentProps, DiagramKind,
    DirectedProps, ElementData, ElementKind, ExtendProps, Id, IncludeProps, InstanceKind,
    InstanceProps, InteractionOperandProps, InteractionOperator, InteractionUseProps,
    LifelineProps, MessageGate, MessageSort, Model, ModelError, Point, PseudostateKind, Rect,
    Result, Size, StateProps, Tx,
};

use crate::sequence::{self, Slot};
use crate::sequence_edit;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeTool {
    Class,
    Interface,
    Enumeration,
    Package,
    Note,
    Actor,
    UseCase,
    Lifeline,
    SystemBoundary,
    CombinedFragment,
    InteractionUse,
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
    State,
    FinalState,
    InitialPseudostate,
    Choice,
    Junction,
    ForkPseudostate,
    JoinPseudostate,
    ShallowHistory,
    DeepHistory,
    Component,
    Artifact,
    /// A deployment node; `Node` would be ambiguous among diagram tools.
    DeploymentNode,
    ComponentInstance,
    NodeInstance,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeTool {
    Association,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Node(NodeTool),
    Edge(EdgeTool),
}

impl Tool {
    /// Stable key, e.g. for translations (`tool.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Node(n) => n.key(),
            Self::Edge(e) => e.key(),
        }
    }
}

/// Tools offered for a diagram kind, in palette order.
pub fn palette(kind: DiagramKind) -> Vec<Tool> {
    use EdgeTool as E;
    use NodeTool as N;
    let (nodes, edges): (&[NodeTool], &[EdgeTool]) = match kind {
        DiagramKind::Class => (
            &[N::Class, N::Interface, N::Enumeration, N::Package, N::Note],
            &[E::Association, E::Generalization, E::Realization, E::Dependency, E::NoteAnchor],
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
            &[N::Lifeline, N::CombinedFragment, N::InteractionUse, N::Note],
            &[
                E::Message,
                E::AsyncMessage,
                E::Reply,
                E::CreateMessage,
                E::DestroyMessage,
                E::FoundMessage,
                E::LostMessage,
                E::NoteAnchor,
            ],
        ),
        DiagramKind::Activity => (
            &[
                N::Action,
                N::InitialNode,
                N::ActivityFinal,
                N::FlowFinal,
                N::Decision,
                N::Merge,
                N::Fork,
                N::Join,
                N::ObjectNode,
                N::Partition,
                N::Note,
            ],
            &[E::Flow, E::NoteAnchor],
        ),
        DiagramKind::StateMachine => (
            &[
                N::State,
                N::InitialPseudostate,
                N::FinalState,
                N::Choice,
                N::Junction,
                N::ForkPseudostate,
                N::JoinPseudostate,
                N::ShallowHistory,
                N::DeepHistory,
                N::Note,
            ],
            &[E::Transition, E::NoteAnchor],
        ),
        DiagramKind::Component => (
            &[N::Component, N::Interface, N::Artifact, N::Class, N::Package, N::Note],
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
            &[E::Association, E::Dependency, E::Realization, E::NoteAnchor],
        ),
    };
    std::iter::once(Tool::Select)
        .chain(nodes.iter().map(|&n| Tool::Node(n)))
        .chain(edges.iter().map(|&e| Tool::Edge(e)))
        .collect()
}

impl NodeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enumeration => "enumeration",
            Self::Package => "package",
            Self::Note => "comment",
            Self::Actor => "actor",
            Self::UseCase => "useCase",
            Self::Lifeline => "lifeline",
            Self::SystemBoundary => "systemBoundary",
            Self::CombinedFragment => "combinedFragment",
            Self::InteractionUse => "interactionUse",
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
            Self::State => "state",
            Self::FinalState => "finalState",
            Self::InitialPseudostate => "initialPseudostate",
            Self::Choice => "choice",
            Self::Junction => "junction",
            Self::ForkPseudostate => "forkPseudostate",
            Self::JoinPseudostate => "joinPseudostate",
            Self::ShallowHistory => "shallowHistory",
            Self::DeepHistory => "deepHistory",
            Self::Component => "component",
            Self::Artifact => "artifact",
            Self::DeploymentNode => "node",
            Self::ComponentInstance => "componentInstance",
            Self::NodeInstance => "nodeInstance",
            Self::Object => "object",
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
        }
    }

    /// Whether new elements of this tool get a generated name. Notes, control nodes, final
    /// states and pseudostates stay unnamed; their names are not shown.
    pub fn is_named(self) -> bool {
        match self.element_kind() {
            ElementKind::ActivityNode(p) => {
                matches!(p.node, ActivityNodeKind::Action | ActivityNodeKind::Object)
            }
            ElementKind::Comment(_) | ElementKind::FinalState | ElementKind::Pseudostate(_) => {
                false
            }
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
            Self::Class => ElementKind::Class(classifier),
            Self::Interface => ElementKind::Interface(classifier),
            Self::Enumeration => ElementKind::Enumeration(classifier),
            Self::Package => ElementKind::Package,
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
            Self::State => ElementKind::State(StateProps::default()),
            Self::FinalState => ElementKind::FinalState,
            Self::InitialPseudostate => pseudostate(PseudostateKind::Initial),
            Self::Choice => pseudostate(PseudostateKind::Choice),
            Self::Junction => pseudostate(PseudostateKind::Junction),
            Self::ForkPseudostate => pseudostate(PseudostateKind::Fork),
            Self::JoinPseudostate => pseudostate(PseudostateKind::Join),
            Self::ShallowHistory => pseudostate(PseudostateKind::ShallowHistory),
            Self::DeepHistory => pseudostate(PseudostateKind::DeepHistory),
            Self::Component => ElementKind::Component(classifier),
            Self::Artifact => ElementKind::Artifact(classifier),
            Self::DeploymentNode => ElementKind::Node(classifier),
            Self::ComponentInstance => instance(InstanceKind::Component),
            Self::NodeInstance => instance(InstanceKind::Node),
            Self::Object => instance(InstanceKind::Object),
        }
    }

    pub fn default_size(self) -> Size {
        match self {
            Self::Class => Size::new(160.0, 70.0),
            Self::Interface | Self::Enumeration => Size::new(160.0, 60.0),
            Self::Package => Size::new(200.0, 140.0),
            Self::Note => Size::new(160.0, 60.0),
            Self::Actor => Size::new(40.0, 80.0),
            Self::UseCase => Size::new(140.0, 60.0),
            Self::Lifeline => Size::new(100.0, 300.0),
            Self::SystemBoundary => Size::new(300.0, 360.0),
            // The layout sizes fragments; this is only what the file keeps.
            Self::CombinedFragment | Self::InteractionUse => Size::new(200.0, 80.0),
            Self::Action => Size::new(120.0, 36.0),
            // Astah's control nodes: 20 px circles, 5 px bars.
            Self::InitialNode | Self::ActivityFinal | Self::FlowFinal => Size::new(20.0, 20.0),
            Self::Decision | Self::Merge => Size::new(24.0, 24.0),
            Self::Fork | Self::Join => Size::new(60.0, 5.0),
            Self::ObjectNode => Size::new(80.0, 36.0),
            Self::Partition => Size::new(200.0, 400.0),
            Self::State => Size::new(100.0, 50.0),
            Self::FinalState | Self::InitialPseudostate => Size::new(20.0, 20.0),
            Self::Choice => Size::new(24.0, 24.0),
            Self::Junction => Size::new(14.0, 14.0),
            Self::ForkPseudostate | Self::JoinPseudostate => Size::new(60.0, 5.0),
            Self::ShallowHistory | Self::DeepHistory => Size::new(24.0, 24.0),
            Self::Component | Self::ComponentInstance => Size::new(140.0, 60.0),
            Self::Artifact => Size::new(120.0, 60.0),
            Self::DeploymentNode | Self::NodeInstance => Size::new(200.0, 140.0),
            Self::Object => Size::new(120.0, 40.0),
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

fn is_dependable(kind: &ElementKind) -> bool {
    kind.is_classifier() || matches!(kind, ElementKind::Package | ElementKind::Instance(_))
}

impl EdgeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Association => "association",
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
        }
    }

    /// The kind of message this tool draws, `None` for other tools.
    pub fn message_sort(self) -> Option<MessageSort> {
        match self {
            Self::Message | Self::FoundMessage | Self::LostMessage => {
                Some(MessageSort::Synchronous)
            }
            Self::AsyncMessage => Some(MessageSort::Asynchronous),
            Self::Reply => Some(MessageSort::Reply),
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
                        K::Class(_) | K::Enumeration(_) | K::DataType(_) | K::Component(_)
                    )
                    && matches!(target, K::Interface(_))
            }
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
            // back to themselves.
            Self::Transition => {
                let initial = matches!(
                    target,
                    K::Pseudostate(p) if p.pseudostate == PseudostateKind::Initial
                );
                source.is_vertex()
                    && target.is_vertex()
                    && !matches!(source, K::FinalState)
                    && !initial
                    && (!same || matches!(source, K::State(_)))
            }
            Self::NoteAnchor => {
                matches!(source, K::Comment(_)) != matches!(target, K::Comment(_))
                    && !matches!(source, K::Diagram(_) | K::Model)
                    && !matches!(target, K::Diagram(_) | K::Model)
            }
        }
    }

    /// The relationship element this tool creates, `None` for purely graphical note anchors.
    pub fn element_kind(self, source: Id, target: Id) -> Option<ElementKind> {
        let directed = DirectedProps { client: source, supplier: target };
        match self {
            Self::Association => Some(ElementKind::association(source, target)),
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
            Self::NoteAnchor => None,
            Self::Message
            | Self::AsyncMessage
            | Self::Reply
            | Self::CreateMessage
            | Self::DestroyMessage => {
                Some(ElementKind::message(self.message_sort()?, source, target))
            }
            Self::FoundMessage | Self::LostMessage => {
                let mut kind = ElementKind::message(MessageSort::Synchronous, source, target);
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
        ElementKind::ActivityNode(p) => NodeTool::for_activity_node(p.node),
        ElementKind::State(_) => NodeTool::State,
        ElementKind::FinalState => NodeTool::FinalState,
        ElementKind::Pseudostate(p) => NodeTool::for_pseudostate(p.pseudostate),
        ElementKind::Component(_) => NodeTool::Component,
        ElementKind::Artifact(_) => NodeTool::Artifact,
        ElementKind::Node(_) => NodeTool::DeploymentNode,
        ElementKind::Instance(p) => NodeTool::for_instance(p.instance),
        ElementKind::Partition => NodeTool::Partition,
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
        return Err(ModelError::Invalid(format!(
            "a {} cannot be shown on this diagram",
            kind.type_name()
        )));
    }
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    if view.presentations_of(element).next().is_some() {
        return Err(ModelError::Invalid("the element is already on this diagram".into()));
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

/// Whether nodes of this kind contain other nodes drawn inside them.
pub fn is_container(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::SystemBoundary
            | ElementKind::Package
            | ElementKind::Partition
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
    let presentation = umlcraft_model::Presentation::node(Some(element), bounds);
    if is_container(kind) || matches!(kind, ElementKind::CombinedFragment(_)) {
        let id = presentation.id;
        tx.apply(umlcraft_model::Op::InsertPresentation { diagram, index: 0, presentation })?;
        Ok(id)
    } else {
        tx.add_presentation(diagram, presentation)
    }
}

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
    let taken = |name: &str| {
        model.children(owner).iter().any(|&c| model.get(c).is_some_and(|e| e.name() == name))
    };
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
        return create_fragment(tx, diagram, tool, None, Vec::new(), origin);
    }
    let kind = tool.element_kind();
    let diagram_owner = tx.model().element(diagram)?.owner().ok_or(ModelError::Root)?;
    let owner = match enclosing_state(tx.model(), diagram, origin, &kind) {
        Some(state) => state,
        None => owner_for(tx.model(), diagram_owner, &kind).ok_or_else(|| {
            ModelError::Invalid(format!("no owner for a new {}", kind.type_name()))
        })?,
    };
    let name =
        if tool.is_named() { unique_name(tx.model(), owner, base_name) } else { String::new() };
    let element = tx.create(owner, ElementData::new(name, kind))?;
    let bounds = Rect::from_origin_size(origin, tool.default_size());
    let node = add_node_for(tx, diagram, element, bounds)?;
    Ok((element, node))
}

/// The innermost state drawn around `p` on `diagram`, if a vertex of `kind` placed there is to
/// become one of its substates.
fn enclosing_state(model: &Model, diagram: Id, p: Point, kind: &ElementKind) -> Option<Id> {
    if !kind.is_vertex() {
        return None;
    }
    model
        .view(diagram)?
        .presentations()
        .iter()
        .filter_map(|pr| Some((pr.element?, pr.bounds()?)))
        .filter(|(e, b)| {
            b.contains(p)
                && model.get(*e).is_some_and(|e| matches!(e.kind(), ElementKind::State(_)))
        })
        .min_by(|(_, a), (_, b)| (a.width * a.height).total_cmp(&(b.width * b.height)))
        .map(|(e, _)| e)
}

/// Connects two presentations of `diagram` with `tool`. Creates the relationship element in the
/// namespace of the source element and returns the new edge presentation.
pub fn connect(tx: &mut Tx<'_>, diagram: Id, tool: EdgeTool, source: Id, target: Id) -> Result<Id> {
    if tool.message_sort().is_some() {
        return connect_message(tx, diagram, tool, source, target, None);
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
    let may_loop = matches!(tool, EdgeTool::Association | EdgeTool::Transition);
    if (self_loop && !may_loop) || !tool.accepts(sk, tk, s == t) {
        return Err(ModelError::Invalid(format!(
            "{} cannot connect {} to {}",
            tool.key(),
            sk.type_name(),
            tk.type_name()
        )));
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
            tx.add_edge(diagram, Some(element), source, target, waypoints)
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
/// height of `slot`, or below everything. Returns the new message presentation.
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
    let message = tx.create_at(owner, index, ElementData::new("", kind))?;
    sequence_edit::add_message(tx, diagram, message, source, target, y)
}

/// Puts a new combined fragment with one empty operand, or an interaction use (`tool`), at the
/// height of `slot` (or below everything), covering the lifelines `covered`. Returns
/// (element, node).
pub fn create_fragment(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    slot: Option<Slot>,
    covered: Vec<Id>,
    origin: Point,
) -> Result<(Id, Id)> {
    let (owner, index) = slot_or_end(tx.model(), diagram, slot)?;
    let y = match slot {
        Some(slot) => slot.y,
        None if origin != Point::default() => origin.y,
        None => sequence_edit::next_y(tx.model(), diagram),
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
    let slot = Slot { y: top - sequence::TAB_HEIGHT - 10.0, owner, index: first };
    let (fragment, node) = create_fragment(
        tx,
        diagram,
        NodeTool::CombinedFragment,
        Some(slot),
        lifelines,
        Point::default(),
    )?;
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
    fn unique_names_fill_gaps() {
        let (doc, ids) = order_system();
        assert_eq!(unique_name(doc.model(), ids.domain, "Class"), "Class1");
        assert_eq!(unique_name(doc.model(), ids.domain, "Customer"), "Customer1");
    }
}
