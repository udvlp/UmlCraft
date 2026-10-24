//! Model elements: the semantic part of a UML model, independent of any diagram.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::diagram::DiagramKind;
use crate::id::Id;

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// Everything about an element except its place in the ownership tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementData {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub visibility: Visibility,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stereotypes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tagged_values: Vec<TaggedValue>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub documentation: String,
    #[serde(flatten)]
    pub kind: ElementKind,
}

impl ElementData {
    pub fn new(name: impl Into<String>, kind: ElementKind) -> Self {
        Self {
            name: name.into(),
            visibility: Visibility::default(),
            stereotypes: Vec::new(),
            tagged_values: Vec::new(),
            documentation: String::new(),
            kind,
        }
    }

    pub fn with_visibility(mut self, visibility: Visibility) -> Self {
        self.visibility = visibility;
        self
    }

    pub fn with_stereotype(mut self, stereotype: impl Into<String>) -> Self {
        self.stereotypes.push(stereotype.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaggedValue {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Package,
    Private,
}

impl Visibility {
    /// UML notation symbol (`+`, `#`, `~`, `-`).
    pub fn symbol(self) -> char {
        match self {
            Self::Public => '+',
            Self::Protected => '#',
            Self::Package => '~',
            Self::Private => '-',
        }
    }

    pub fn from_symbol(symbol: char) -> Option<Self> {
        match symbol {
            '+' => Some(Self::Public),
            '#' => Some(Self::Protected),
            '~' => Some(Self::Package),
            '-' => Some(Self::Private),
            _ => None,
        }
    }
}

/// A multiplicity range such as `1`, `0..1` or `1..*`. `upper == None` means unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Multiplicity {
    pub lower: u32,
    pub upper: Option<u32>,
}

impl Multiplicity {
    pub const ONE: Self = Self { lower: 1, upper: Some(1) };
    pub const OPTIONAL: Self = Self { lower: 0, upper: Some(1) };
    pub const MANY: Self = Self { lower: 0, upper: None };
    pub const ONE_OR_MORE: Self = Self { lower: 1, upper: None };
}

impl fmt::Display for Multiplicity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.lower, self.upper) {
            (0, None) => f.write_str("*"),
            (l, Some(u)) if l == u => write!(f, "{l}"),
            (l, Some(u)) => write!(f, "{l}..{u}"),
            (l, None) => write!(f, "{l}..*"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid multiplicity: {0:?}")]
pub struct ParseMultiplicityError(pub String);

impl FromStr for Multiplicity {
    type Err = ParseMultiplicityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseMultiplicityError(s.to_owned());
        let parse_bound = |b: &str| -> Result<Option<u32>, ParseMultiplicityError> {
            match b.trim() {
                "*" => Ok(None),
                n => n.parse().map(Some).map_err(|_| err()),
            }
        };
        let result = match s.trim().split_once("..") {
            Some((lower, upper)) => {
                Self { lower: parse_bound(lower)?.ok_or_else(err)?, upper: parse_bound(upper)? }
            }
            None => match parse_bound(s)? {
                None => Self::MANY,
                Some(n) => Self { lower: n, upper: Some(n) },
            },
        };
        match result.upper {
            Some(upper) if upper < result.lower || upper == 0 => Err(err()),
            _ => Ok(result),
        }
    }
}

impl Serialize for Multiplicity {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Multiplicity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Type of an attribute, parameter or operation result: a classifier or template parameter in
/// the model, free text (e.g. `int`, or a type that is not modeled), or a type built from
/// others.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TypeRef {
    Element(Id),
    Text(String),
    Expr(Box<TypeExpr>),
}

/// A type built from others: `List<Foo>`, `Foo[]`, `? extends Number`, `const Foo*`. Each part
/// is a type reference of its own, so `List` and `Foo` stay linked to the model.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeExpr {
    pub base: TypeRef,
    /// Template arguments, in order. Values such as C++'s `10` are text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<TypeRef>,
    /// Written before the base, e.g. `? extends ` or `const `.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    /// Written after the arguments, e.g. `[]`, `...`, `*` or `&`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modifier: String,
}

impl TypeRef {
    /// The referenced elements: the base first, then the arguments in order.
    pub fn ids(&self) -> Vec<Id> {
        let mut ids = Vec::new();
        self.collect_ids(&mut ids);
        ids
    }

    fn collect_ids(&self, into: &mut Vec<Id>) {
        match self {
            Self::Element(id) => into.push(*id),
            Self::Text(_) => {}
            Self::Expr(e) => {
                e.base.collect_ids(into);
                e.arguments.iter().for_each(|a| a.collect_ids(into));
            }
        }
    }

    /// Calls `f` for every referenced element, in the order of [`TypeRef::ids`].
    pub fn for_each_id_mut(&mut self, f: &mut dyn FnMut(&mut Id)) {
        match self {
            Self::Element(id) => f(id),
            Self::Text(_) => {}
            Self::Expr(e) => {
                e.base.for_each_id_mut(f);
                e.arguments.iter_mut().for_each(|a| a.for_each_id_mut(f));
            }
        }
    }

    /// Replaces every reference to `target` with free text holding `target_name`. Returns
    /// whether anything changed.
    pub fn unlink(&mut self, target: Id, target_name: &str) -> bool {
        match self {
            Self::Element(id) if *id == target => {
                *self = Self::Text(target_name.to_owned());
                true
            }
            Self::Element(_) | Self::Text(_) => false,
            Self::Expr(e) => {
                let base = e.base.unlink(target, target_name);
                e.arguments
                    .iter_mut()
                    .fold(base, |changed, a| a.unlink(target, target_name) || changed)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_abstract: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_leaf: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributeProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_ref: Option<TypeRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplicity: Option<Multiplicity>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_static: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_derived: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_read_only: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_type: Option<TypeRef>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_static: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_abstract: bool,
    /// Cannot be overridden (`final` in Java, `sealed` in C#).
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_leaf: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_query: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterDirection {
    #[default]
    In,
    Out,
    InOut,
    Return,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParameterProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_ref: Option<TypeRef>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub direction: ParameterDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplicity: Option<Multiplicity>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateParameterKind {
    /// Stands for a type, such as Java's `T`.
    #[default]
    Type,
    /// Stands for a value, such as C++'s `int N`.
    Value,
}

/// A formal parameter of a template class, interface, data type or operation: `T extends
/// Number` in Java, `int N = 10` in C++. Its name is the element's name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateParameterProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub parameter: TemplateParameterKind,
    /// Value parameters: the type of the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_ref: Option<TypeRef>,
    /// Type parameters: the types an argument must conform to (`extends A & B`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bounds: Vec<TypeRef>,
    /// Used when no argument is given: a type, or a value as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<TypeRef>,
    /// Takes any number of arguments (C++'s `typename... Ts`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_variadic: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LifelineProps {
    /// Classifier this lifeline is an instance of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub represents: Option<Id>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageSort {
    #[default]
    Synchronous,
    Asynchronous,
    Reply,
    Create,
    Destroy,
}

/// Where a message comes from or goes to when it does not connect two lifelines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MessageGate {
    #[default]
    None,
    /// The message comes from outside the interaction; `from` repeats `to`.
    Found,
    /// The message goes outside the interaction; `to` repeats `from`.
    Lost,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub sort: MessageSort,
    /// Sending lifeline.
    pub from: Id,
    /// Receiving lifeline.
    pub to: Id,
    #[serde(default, skip_serializing_if = "is_default")]
    pub gate: MessageGate,
    /// Operation invoked by this message, if it is modeled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Id>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arguments: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub return_value: String,
    /// Condition under which the message is sent, shown as `[guard]` before the name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InteractionOperator {
    #[default]
    Alt,
    Opt,
    Loop,
    Par,
    Break,
    Critical,
    Neg,
    Assert,
    Strict,
    Seq,
    Ignore,
    Consider,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedFragmentProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub operator: InteractionOperator,
    /// Lifelines covered by the fragment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covered: Vec<Id>,
}

/// A reference from one interaction to another, drawn as a "ref" frame.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionUseProps {
    /// The interaction this one refers to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refers_to: Option<Id>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub arguments: String,
    /// Lifelines covered by the frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub covered: Vec<Id>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionOperandProps {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Navigability {
    #[default]
    Unspecified,
    Navigable,
    NonNavigable,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Aggregation {
    #[default]
    None,
    Shared,
    Composite,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationEnd {
    pub participant: Id,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplicity: Option<Multiplicity>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub navigability: Navigability,
    #[serde(default, skip_serializing_if = "is_default")]
    pub aggregation: Aggregation,
    #[serde(default, skip_serializing_if = "is_default")]
    pub visibility: Visibility,
    /// Constraints on this end, without braces, e.g. `ordered` for `{ordered}`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_read_only: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub is_static: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
    /// Qualifiers of this end, such as the key of a map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualifiers: Vec<Qualifier>,
}

impl AssociationEnd {
    pub fn new(participant: Id) -> Self {
        Self {
            participant,
            role: String::new(),
            multiplicity: None,
            navigability: Navigability::default(),
            aggregation: Aggregation::default(),
            visibility: Visibility::default(),
            constraints: Vec::new(),
            is_read_only: false,
            is_static: false,
            default_value: String::new(),
            qualifiers: Vec::new(),
        }
    }
}

/// A qualifier of an association end, such as `key : K`: the participant at the other end is
/// looked up by it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Qualifier {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_ref: Option<TypeRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssociationProps {
    pub ends: [AssociationEnd; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralizationProps {
    pub specific: Id,
    pub general: Id,
}

/// Realization, dependency: `client` depends on / realizes `supplier`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectedProps {
    pub client: Id,
    pub supplier: Id,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IncludeProps {
    pub including: Id,
    pub addition: Id,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtendProps {
    pub extension: Id,
    pub extended: Id,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extension_points: Vec<Id>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub condition: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentProps {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotated: Vec<Id>,
}

/// What an activity node does: an action, a control node or an object node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityNodeKind {
    #[default]
    Action,
    Initial,
    /// Ends the whole activity.
    ActivityFinal,
    /// Ends only the flow that reaches it.
    FlowFinal,
    Decision,
    Merge,
    Fork,
    Join,
    /// Objects passed along object flows.
    Object,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityNodeProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub node: ActivityNodeKind,
    /// Object nodes: the type of the objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_ref: Option<TypeRef>,
}

impl ActivityNodeKind {
    /// Whether flows may leave a node of this kind.
    pub fn has_outgoing(self) -> bool {
        !matches!(self, Self::ActivityFinal | Self::FlowFinal)
    }

    /// Whether flows may enter a node of this kind.
    pub fn has_incoming(self) -> bool {
        self != Self::Initial
    }
}

/// A control or object flow between two activity nodes. It is an object flow when one of its
/// ends is an object node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEdgeProps {
    pub source: Id,
    pub target: Id,
    /// Condition for taking the flow, without brackets.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,
}

/// Internal activities of a state, as `entry / …`, `do / …` and `exit / …`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateProps {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub entry: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub do_activity: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub exit: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PseudostateKind {
    #[default]
    Initial,
    Choice,
    Junction,
    Fork,
    Join,
    ShallowHistory,
    DeepHistory,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PseudostateProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub pseudostate: PseudostateKind,
}

/// A transition between two vertices of a state machine, labeled `trigger [guard] / effect`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransitionProps {
    pub source: Id,
    pub target: Id,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trigger: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guard: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub effect: String,
}

impl TransitionProps {
    /// The label as UML writes it: `trigger [guard] / effect`, leaving out what is empty.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if !self.trigger.trim().is_empty() {
            parts.push(self.trigger.trim().to_owned());
        }
        if !self.guard.trim().is_empty() {
            parts.push(format!("[{}]", self.guard.trim()));
        }
        if !self.effect.trim().is_empty() {
            parts.push(format!("/ {}", self.effect.trim()));
        }
        parts.join(" ")
    }
}

/// What an instance specification shows: an object, a component instance or a node instance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstanceKind {
    #[default]
    Object,
    Component,
    Node,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub instance: InstanceKind,
    /// The classifier this is an instance of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifier: Option<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramProps {
    pub diagram: DiagramKind,
    /// Sequence diagrams: leave out the numbers before message names ("2.1: ").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_message_numbers: bool,
    /// Sequence diagrams: leave out the return values after calls ("submit() : ok").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_return_values: bool,
}

/// The metaclass of an element together with its metaclass-specific properties.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ElementKind {
    /// Root of the ownership tree; exactly one per model.
    Model,
    Package,
    Class(ClassifierProps),
    Interface(ClassifierProps),
    Enumeration(ClassifierProps),
    DataType(ClassifierProps),
    EnumerationLiteral,
    Attribute(AttributeProps),
    Operation(OperationProps),
    Parameter(ParameterProps),
    TemplateParameter(TemplateParameterProps),
    Actor(ClassifierProps),
    UseCase(ClassifierProps),
    Component(ClassifierProps),
    Artifact(ClassifierProps),
    /// A deployment target such as a server or a device.
    Node(ClassifierProps),
    /// An instance specification: an object, a component instance or a node instance.
    Instance(InstanceProps),
    ExtensionPoint,
    /// The system (subject) whose use cases are drawn inside its boundary.
    SystemBoundary,
    Interaction,
    Lifeline(LifelineProps),
    Message(MessageProps),
    CombinedFragment(CombinedFragmentProps),
    InteractionOperand(InteractionOperandProps),
    InteractionUse(InteractionUseProps),
    /// Owns the nodes, flows and partitions of activity diagrams.
    Activity,
    ActivityNode(ActivityNodeProps),
    ActivityEdge(ActivityEdgeProps),
    /// A swimlane. Which nodes lie in it follows from the diagram, as for system boundaries.
    Partition,
    /// Owns the states and transitions of a state machine diagram.
    StateMachine,
    /// A state; it is a composite state when it owns states of its own.
    State(StateProps),
    FinalState,
    Pseudostate(PseudostateProps),
    Transition(TransitionProps),
    Association(AssociationProps),
    Generalization(GeneralizationProps),
    Realization(DirectedProps),
    Dependency(DirectedProps),
    Include(IncludeProps),
    Extend(ExtendProps),
    Comment(CommentProps),
    Diagram(DiagramProps),
}

/// How an element refers to another element, and what the target must be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RefRole {
    AssociationEnd,
    Specific,
    General,
    Client,
    Supplier,
    Including,
    Addition,
    Extension,
    Extended,
    MessageEnd,
    Type,
    Represents,
    Operation,
    Covered,
    ExtensionPoint,
    Annotated,
    FlowEnd,
    TransitionEnd,
    Classifier,
    RefersTo,
}

impl RefRole {
    /// A strong reference cannot dangle: deleting the target deletes the referrer as well.
    /// A weak reference is unlinked instead (see [`ElementKind::unlink`]).
    pub fn is_strong(self) -> bool {
        matches!(
            self,
            Self::AssociationEnd
                | Self::Specific
                | Self::General
                | Self::Client
                | Self::Supplier
                | Self::Including
                | Self::Addition
                | Self::Extension
                | Self::Extended
                | Self::MessageEnd
                | Self::FlowEnd
                | Self::TransitionEnd
        )
    }

    /// Whether an element of `target` kind may be referenced in this role.
    pub fn accepts(self, target: &ElementKind) -> bool {
        match self {
            Self::AssociationEnd
            | Self::Specific
            | Self::General
            | Self::Represents
            | Self::Classifier => target.is_classifier(),
            Self::Type => {
                target.is_classifier() || matches!(target, ElementKind::TemplateParameter(_))
            }
            Self::Including | Self::Addition | Self::Extension | Self::Extended => {
                matches!(target, ElementKind::UseCase(_))
            }
            Self::MessageEnd | Self::Covered => matches!(target, ElementKind::Lifeline(_)),
            Self::Operation => matches!(target, ElementKind::Operation(_)),
            Self::ExtensionPoint => matches!(target, ElementKind::ExtensionPoint),
            Self::FlowEnd => matches!(target, ElementKind::ActivityNode(_)),
            Self::RefersTo => matches!(target, ElementKind::Interaction),
            Self::TransitionEnd => target.is_vertex(),
            Self::Client | Self::Supplier | Self::Annotated => {
                !matches!(target, ElementKind::Model | ElementKind::Diagram(_))
            }
        }
    }
}

impl ElementKind {
    pub fn class() -> Self {
        Self::Class(ClassifierProps::default())
    }

    pub fn interface() -> Self {
        Self::Interface(ClassifierProps::default())
    }

    pub fn actor() -> Self {
        Self::Actor(ClassifierProps::default())
    }

    pub fn use_case() -> Self {
        Self::UseCase(ClassifierProps::default())
    }

    pub fn attribute(type_ref: Option<TypeRef>) -> Self {
        Self::Attribute(AttributeProps { type_ref, ..Default::default() })
    }

    pub fn operation(return_type: Option<TypeRef>) -> Self {
        Self::Operation(OperationProps { return_type, ..Default::default() })
    }

    pub fn association(a: Id, b: Id) -> Self {
        Self::Association(AssociationProps {
            ends: [AssociationEnd::new(a), AssociationEnd::new(b)],
        })
    }

    pub fn generalization(specific: Id, general: Id) -> Self {
        Self::Generalization(GeneralizationProps { specific, general })
    }

    pub fn message(sort: MessageSort, from: Id, to: Id) -> Self {
        Self::Message(MessageProps {
            sort,
            from,
            to,
            gate: MessageGate::None,
            operation: None,
            arguments: String::new(),
            return_value: String::new(),
            guard: String::new(),
        })
    }

    pub fn activity_node(node: ActivityNodeKind) -> Self {
        Self::ActivityNode(ActivityNodeProps { node, type_ref: None })
    }

    pub fn flow(source: Id, target: Id) -> Self {
        Self::ActivityEdge(ActivityEdgeProps { source, target, guard: String::new() })
    }

    pub fn pseudostate(pseudostate: PseudostateKind) -> Self {
        Self::Pseudostate(PseudostateProps { pseudostate })
    }

    pub fn transition(source: Id, target: Id) -> Self {
        Self::Transition(TransitionProps {
            source,
            target,
            trigger: String::new(),
            guard: String::new(),
            effect: String::new(),
        })
    }

    pub fn diagram(diagram: DiagramKind) -> Self {
        Self::Diagram(DiagramProps {
            diagram,
            hide_message_numbers: false,
            hide_return_values: false,
        })
    }

    /// Stable, lower camel case metaclass name, as used in files.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Package => "package",
            Self::Class(_) => "class",
            Self::Interface(_) => "interface",
            Self::Enumeration(_) => "enumeration",
            Self::DataType(_) => "dataType",
            Self::EnumerationLiteral => "enumerationLiteral",
            Self::Attribute(_) => "attribute",
            Self::Operation(_) => "operation",
            Self::Parameter(_) => "parameter",
            Self::TemplateParameter(_) => "templateParameter",
            Self::Actor(_) => "actor",
            Self::UseCase(_) => "useCase",
            Self::Component(_) => "component",
            Self::Artifact(_) => "artifact",
            Self::Node(_) => "node",
            Self::Instance(_) => "instance",
            Self::ExtensionPoint => "extensionPoint",
            Self::SystemBoundary => "systemBoundary",
            Self::Interaction => "interaction",
            Self::Lifeline(_) => "lifeline",
            Self::Message(_) => "message",
            Self::CombinedFragment(_) => "combinedFragment",
            Self::InteractionOperand(_) => "interactionOperand",
            Self::InteractionUse(_) => "interactionUse",
            Self::Activity => "activity",
            Self::ActivityNode(_) => "activityNode",
            Self::ActivityEdge(_) => "activityEdge",
            Self::Partition => "partition",
            Self::StateMachine => "stateMachine",
            Self::State(_) => "state",
            Self::FinalState => "finalState",
            Self::Pseudostate(_) => "pseudostate",
            Self::Transition(_) => "transition",
            Self::Association(_) => "association",
            Self::Generalization(_) => "generalization",
            Self::Realization(_) => "realization",
            Self::Dependency(_) => "dependency",
            Self::Include(_) => "include",
            Self::Extend(_) => "extend",
            Self::Comment(_) => "comment",
            Self::Diagram(_) => "diagram",
        }
    }

    pub fn is_classifier(&self) -> bool {
        matches!(
            self,
            Self::Class(_)
                | Self::Interface(_)
                | Self::Enumeration(_)
                | Self::DataType(_)
                | Self::Actor(_)
                | Self::UseCase(_)
                | Self::Component(_)
                | Self::Artifact(_)
                | Self::Node(_)
        )
    }

    /// Binary connections drawn as edges between the nodes they connect, including activity
    /// flows. Messages are not: sequence diagrams lay them out themselves.
    pub fn is_relationship(&self) -> bool {
        matches!(
            self,
            Self::Association(_)
                | Self::Generalization(_)
                | Self::Realization(_)
                | Self::Dependency(_)
                | Self::Include(_)
                | Self::Extend(_)
                | Self::ActivityEdge(_)
                | Self::Transition(_)
        )
    }

    /// States, final states and pseudostates: what transitions connect.
    pub fn is_vertex(&self) -> bool {
        matches!(self, Self::State(_) | Self::FinalState | Self::Pseudostate(_))
    }

    fn is_namespace(&self) -> bool {
        matches!(self, Self::Model | Self::Package)
    }

    /// Ownership rules: may an element of this kind own an element of `child` kind?
    pub fn can_own(&self, child: &ElementKind) -> bool {
        use ElementKind as K;
        match child {
            K::Model => false,
            K::Package => self.is_namespace(),
            K::Class(_) | K::Interface(_) | K::Enumeration(_) | K::DataType(_) => {
                self.is_namespace()
                    || matches!(
                        self,
                        K::Class(_) | K::Interface(_) | K::Enumeration(_) | K::Component(_)
                    )
            }
            // Components nest, and nodes hold nested nodes such as execution environments.
            K::Component(_) | K::Artifact(_) | K::Node(_) => {
                self.is_namespace() || matches!(self, K::Component(_) | K::Node(_))
            }
            K::Instance(_) => self.is_namespace(),
            K::Actor(_) | K::UseCase(_) | K::SystemBoundary => self.is_namespace(),
            K::Attribute(_) | K::Operation(_) => self.is_classifier(),
            K::EnumerationLiteral => matches!(self, K::Enumeration(_)),
            K::Parameter(_) => matches!(self, K::Operation(_)),
            K::TemplateParameter(_) => {
                matches!(self, K::Class(_) | K::Interface(_) | K::DataType(_) | K::Operation(_))
            }
            K::ExtensionPoint => matches!(self, K::UseCase(_)),
            K::Interaction => self.is_namespace() || matches!(self, K::Class(_) | K::UseCase(_)),
            K::Lifeline(_) => matches!(self, K::Interaction),
            K::Message(_) | K::CombinedFragment(_) | K::InteractionUse(_) => {
                matches!(self, K::Interaction | K::InteractionOperand(_))
            }
            K::InteractionOperand(_) => matches!(self, K::CombinedFragment(_)),
            K::Activity => self.is_namespace() || matches!(self, K::Class(_) | K::UseCase(_)),
            K::ActivityNode(_) | K::ActivityEdge(_) => matches!(self, K::Activity),
            K::Partition => matches!(self, K::Activity | K::Partition),
            K::StateMachine => self.is_namespace() || matches!(self, K::Class(_) | K::UseCase(_)),
            K::State(_) | K::FinalState | K::Pseudostate(_) => {
                matches!(self, K::StateMachine | K::State(_))
            }
            K::Transition(_) => matches!(self, K::StateMachine),
            K::Association(_)
            | K::Generalization(_)
            | K::Realization(_)
            | K::Dependency(_)
            | K::Include(_)
            | K::Extend(_) => self.is_namespace() || self.is_classifier(),
            K::Comment(_) => !matches!(self, K::Diagram(_) | K::Comment(_)),
            K::Diagram(_) => {
                self.is_namespace()
                    || self.is_classifier()
                    || matches!(self, K::Interaction | K::Activity | K::StateMachine)
            }
        }
    }

    /// All references to other elements, with their role.
    pub fn references(&self) -> Vec<(Id, RefRole)> {
        let types = |t: &mut dyn Iterator<Item = &TypeRef>| -> Vec<(Id, RefRole)> {
            t.flat_map(TypeRef::ids).map(|id| (id, RefRole::Type)).collect()
        };
        let type_ref = |t: &Option<TypeRef>| types(&mut t.iter());
        match self {
            Self::Attribute(p) => type_ref(&p.type_ref),
            Self::Operation(p) => type_ref(&p.return_type),
            Self::Parameter(p) => type_ref(&p.type_ref),
            Self::TemplateParameter(p) => {
                types(&mut p.type_ref.iter().chain(&p.bounds).chain(&p.default))
            }
            Self::Lifeline(p) => {
                p.represents.map(|id| (id, RefRole::Represents)).into_iter().collect()
            }
            Self::Instance(p) => {
                p.classifier.map(|id| (id, RefRole::Classifier)).into_iter().collect()
            }
            Self::Message(p) => {
                let mut refs = vec![(p.from, RefRole::MessageEnd), (p.to, RefRole::MessageEnd)];
                refs.extend(p.operation.map(|id| (id, RefRole::Operation)));
                refs
            }
            Self::CombinedFragment(p) => {
                p.covered.iter().map(|&id| (id, RefRole::Covered)).collect()
            }
            Self::InteractionUse(p) => {
                let mut refs: Vec<(Id, RefRole)> =
                    p.refers_to.map(|id| (id, RefRole::RefersTo)).into_iter().collect();
                refs.extend(p.covered.iter().map(|&id| (id, RefRole::Covered)));
                refs
            }
            Self::ActivityNode(p) => type_ref(&p.type_ref),
            Self::ActivityEdge(p) => {
                vec![(p.source, RefRole::FlowEnd), (p.target, RefRole::FlowEnd)]
            }
            Self::Transition(p) => {
                vec![(p.source, RefRole::TransitionEnd), (p.target, RefRole::TransitionEnd)]
            }
            Self::Association(p) => {
                let mut refs: Vec<(Id, RefRole)> =
                    p.ends.iter().map(|end| (end.participant, RefRole::AssociationEnd)).collect();
                let qualifiers = p.ends.iter().flat_map(|end| &end.qualifiers);
                refs.extend(types(&mut qualifiers.filter_map(|q| q.type_ref.as_ref())));
                refs
            }
            Self::Generalization(p) => {
                vec![(p.specific, RefRole::Specific), (p.general, RefRole::General)]
            }
            Self::Realization(p) | Self::Dependency(p) => {
                vec![(p.client, RefRole::Client), (p.supplier, RefRole::Supplier)]
            }
            Self::Include(p) => {
                vec![(p.including, RefRole::Including), (p.addition, RefRole::Addition)]
            }
            Self::Extend(p) => {
                let mut refs =
                    vec![(p.extension, RefRole::Extension), (p.extended, RefRole::Extended)];
                refs.extend(p.extension_points.iter().map(|&id| (id, RefRole::ExtensionPoint)));
                refs
            }
            Self::Comment(p) => p.annotated.iter().map(|&id| (id, RefRole::Annotated)).collect(),
            _ => Vec::new(),
        }
    }

    /// Calls `f` for every reference, allowing it to be changed. Visits the same references,
    /// in the same order, as [`ElementKind::references`].
    pub fn for_each_reference_mut(&mut self, mut f: impl FnMut(&mut Id)) {
        let type_ref = |t: &mut Option<TypeRef>, f: &mut dyn FnMut(&mut Id)| {
            t.iter_mut().for_each(|t| t.for_each_id_mut(f));
        };
        match self {
            Self::Attribute(p) => type_ref(&mut p.type_ref, &mut f),
            Self::Operation(p) => type_ref(&mut p.return_type, &mut f),
            Self::Parameter(p) => type_ref(&mut p.type_ref, &mut f),
            Self::TemplateParameter(p) => {
                type_ref(&mut p.type_ref, &mut f);
                p.bounds.iter_mut().for_each(|t| t.for_each_id_mut(&mut f));
                type_ref(&mut p.default, &mut f);
            }
            Self::Lifeline(p) => p.represents.iter_mut().for_each(f),
            Self::Instance(p) => p.classifier.iter_mut().for_each(f),
            Self::Message(p) => {
                f(&mut p.from);
                f(&mut p.to);
                p.operation.iter_mut().for_each(f);
            }
            Self::CombinedFragment(p) => p.covered.iter_mut().for_each(f),
            Self::InteractionUse(p) => {
                p.refers_to.iter_mut().for_each(&mut f);
                p.covered.iter_mut().for_each(f);
            }
            Self::ActivityNode(p) => type_ref(&mut p.type_ref, &mut f),
            Self::ActivityEdge(p) => {
                f(&mut p.source);
                f(&mut p.target);
            }
            Self::Transition(p) => {
                f(&mut p.source);
                f(&mut p.target);
            }
            Self::Association(p) => {
                p.ends.iter_mut().for_each(|end| f(&mut end.participant));
                for q in p.ends.iter_mut().flat_map(|end| &mut end.qualifiers) {
                    type_ref(&mut q.type_ref, &mut f);
                }
            }
            Self::Generalization(p) => {
                f(&mut p.specific);
                f(&mut p.general);
            }
            Self::Realization(p) | Self::Dependency(p) => {
                f(&mut p.client);
                f(&mut p.supplier);
            }
            Self::Include(p) => {
                f(&mut p.including);
                f(&mut p.addition);
            }
            Self::Extend(p) => {
                f(&mut p.extension);
                f(&mut p.extended);
                p.extension_points.iter_mut().for_each(f);
            }
            Self::Comment(p) => p.annotated.iter_mut().for_each(f),
            _ => {}
        }
    }

    /// Replaces every referenced id that `map` knows with its new value.
    pub fn remap(&mut self, map: &std::collections::HashMap<Id, Id>) {
        self.for_each_reference_mut(|id| {
            if let Some(&new) = map.get(id) {
                *id = new;
            }
        });
    }

    /// Whether this element holds a strong reference to `target`.
    pub fn refers_strongly_to(&self, target: Id) -> bool {
        self.references().iter().any(|&(id, role)| id == target && role.is_strong())
    }

    /// Removes every weak reference to `target`. Type references become free text with the
    /// target's former name, so the attribute still reads the same. Returns whether anything
    /// changed. Strong references are left alone; their owner has to be deleted instead.
    pub fn unlink(&mut self, target: Id, target_name: &str) -> bool {
        let unlink_type =
            |t: &mut Option<TypeRef>| t.as_mut().is_some_and(|t| t.unlink(target, target_name));
        let unlink_vec = |v: &mut Vec<Id>| {
            let len = v.len();
            v.retain(|&id| id != target);
            v.len() != len
        };
        let unlink_opt = |o: &mut Option<Id>| {
            if *o == Some(target) {
                *o = None;
                true
            } else {
                false
            }
        };
        match self {
            Self::Attribute(p) => unlink_type(&mut p.type_ref),
            Self::Operation(p) => unlink_type(&mut p.return_type),
            Self::Parameter(p) => unlink_type(&mut p.type_ref),
            Self::TemplateParameter(p) => {
                let mut changed = unlink_type(&mut p.type_ref);
                for bound in &mut p.bounds {
                    changed |= bound.unlink(target, target_name);
                }
                unlink_type(&mut p.default) || changed
            }
            Self::Association(p) => {
                let mut changed = false;
                for q in p.ends.iter_mut().flat_map(|end| &mut end.qualifiers) {
                    changed |= unlink_type(&mut q.type_ref);
                }
                changed
            }
            Self::Lifeline(p) => unlink_opt(&mut p.represents),
            Self::Instance(p) => unlink_opt(&mut p.classifier),
            Self::Message(p) => unlink_opt(&mut p.operation),
            Self::CombinedFragment(p) => unlink_vec(&mut p.covered),
            Self::InteractionUse(p) => {
                let refers = unlink_opt(&mut p.refers_to);
                unlink_vec(&mut p.covered) || refers
            }
            Self::ActivityNode(p) => unlink_type(&mut p.type_ref),
            Self::Extend(p) => unlink_vec(&mut p.extension_points),
            Self::Comment(p) => unlink_vec(&mut p.annotated),
            _ => false,
        }
    }

    pub fn as_generalization(&self) -> Option<&GeneralizationProps> {
        match self {
            Self::Generalization(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_diagram(&self) -> Option<DiagramKind> {
        match self {
            Self::Diagram(p) => Some(p.diagram),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplicity_roundtrip() {
        for (text, expected, canonical) in [
            ("1", Multiplicity::ONE, "1"),
            ("0..1", Multiplicity::OPTIONAL, "0..1"),
            ("*", Multiplicity::MANY, "*"),
            ("0..*", Multiplicity::MANY, "*"),
            ("1..*", Multiplicity::ONE_OR_MORE, "1..*"),
            (" 2 .. 5 ", Multiplicity { lower: 2, upper: Some(5) }, "2..5"),
        ] {
            let parsed: Multiplicity = text.parse().unwrap();
            assert_eq!(parsed, expected, "{text}");
            assert_eq!(parsed.to_string(), canonical);
        }
        for bad in ["", "x", "3..1", "*..2", "0", "1..", "-1"] {
            assert!(bad.parse::<Multiplicity>().is_err(), "{bad:?} should not parse");
        }
    }

    /// `base<argument>[]`.
    fn list_of(base: Id, argument: Id) -> TypeRef {
        TypeRef::Expr(Box::new(TypeExpr {
            base: TypeRef::Element(base),
            arguments: vec![TypeRef::Element(argument)],
            prefix: String::new(),
            modifier: "[]".into(),
        }))
    }

    #[test]
    fn unlink_replaces_only_the_affected_part_of_an_expression() {
        let (list, foo) = (Id::from_u128(1), Id::from_u128(2));
        let mut kind = ElementKind::attribute(Some(list_of(list, foo)));
        assert_eq!(kind.references(), vec![(list, RefRole::Type), (foo, RefRole::Type)]);
        assert!(kind.unlink(foo, "Foo"));
        let ElementKind::Attribute(p) = &kind else { unreachable!() };
        let Some(TypeRef::Expr(e)) = &p.type_ref else { panic!("{kind:?}") };
        assert_eq!(e.base, TypeRef::Element(list));
        assert_eq!(e.arguments, vec![TypeRef::Text("Foo".into())]);
        assert_eq!(e.modifier, "[]");
        assert!(!kind.unlink(foo, "Foo"));

        let mut parameter = ElementKind::TemplateParameter(TemplateParameterProps {
            bounds: vec![TypeRef::Element(foo)],
            ..Default::default()
        });
        assert!(parameter.unlink(foo, "Foo"));
        assert!(parameter.references().is_empty());
    }

    #[test]
    fn unlink_turns_type_into_text() {
        let target = Id::from_u128(7);
        let mut kind = ElementKind::attribute(Some(TypeRef::Element(target)));
        assert!(kind.unlink(target, "Customer"));
        assert_eq!(kind, ElementKind::attribute(Some(TypeRef::Text("Customer".into()))));
        assert!(!kind.unlink(target, "Customer"));
    }

    #[test]
    fn remap_visits_the_same_references() {
        let (a, b, c) = (Id::from_u128(1), Id::from_u128(2), Id::from_u128(3));
        let mut kinds = vec![
            ElementKind::association(a, b),
            ElementKind::generalization(a, b),
            ElementKind::attribute(Some(TypeRef::Element(a))),
            ElementKind::message(MessageSort::Reply, a, b),
            ElementKind::Comment(CommentProps { body: String::new(), annotated: vec![a, b] }),
            ElementKind::flow(a, b),
            ElementKind::transition(a, b),
            ElementKind::Instance(InstanceProps {
                instance: InstanceKind::Node,
                classifier: Some(a),
            }),
            ElementKind::ActivityNode(ActivityNodeProps {
                node: ActivityNodeKind::Object,
                type_ref: Some(TypeRef::Element(a)),
            }),
            ElementKind::attribute(Some(list_of(b, a))),
            ElementKind::TemplateParameter(TemplateParameterProps {
                bounds: vec![TypeRef::Element(b), list_of(a, a)],
                default: Some(TypeRef::Element(a)),
                ..Default::default()
            }),
            {
                let mut kind = ElementKind::association(b, a);
                if let ElementKind::Association(p) = &mut kind {
                    p.ends[1].qualifiers =
                        vec![Qualifier { name: "key".into(), type_ref: Some(list_of(b, a)) }];
                }
                kind
            },
        ];
        let map = std::collections::HashMap::from([(a, c)]);
        for kind in &mut kinds {
            let mut seen = Vec::new();
            kind.for_each_reference_mut(|id| seen.push(*id));
            let expected: Vec<Id> = kind.references().into_iter().map(|(id, _)| id).collect();
            assert_eq!(seen, expected, "{kind:?}");
            kind.remap(&map);
            assert!(kind.references().iter().all(|(id, _)| *id != a), "{kind:?}");
            assert!(kind.references().iter().any(|(id, _)| *id == c), "{kind:?}");
        }
    }

    #[test]
    fn ownership_rules() {
        assert!(ElementKind::Package.can_own(&ElementKind::class()));
        assert!(ElementKind::class().can_own(&ElementKind::attribute(None)));
        assert!(!ElementKind::attribute(None).can_own(&ElementKind::class()));
        assert!(!ElementKind::Package.can_own(&ElementKind::Model));
        assert!(ElementKind::Interaction.can_own(&ElementKind::Lifeline(LifelineProps::default())));
        let action = ElementKind::activity_node(ActivityNodeKind::Action);
        assert!(ElementKind::Activity.can_own(&action));
        assert!(!ElementKind::Package.can_own(&action));
        assert!(ElementKind::Partition.can_own(&ElementKind::Partition));
        assert!(!ElementKind::Partition.can_own(&action));
        assert!(ElementKind::Activity.can_own(&ElementKind::diagram(DiagramKind::Activity)));
        let state = ElementKind::State(StateProps::default());
        assert!(ElementKind::StateMachine.can_own(&state));
        assert!(state.can_own(&ElementKind::FinalState), "composite states");
        assert!(!state.can_own(&ElementKind::transition(Id::from_u128(1), Id::from_u128(2))));
        let node = ElementKind::Node(ClassifierProps::default());
        let component = ElementKind::Component(ClassifierProps::default());
        assert!(node.can_own(&node), "nested nodes");
        assert!(component.can_own(&ElementKind::class()));
        assert!(component.can_own(&ElementKind::operation(None)));
        assert!(!node.can_own(&ElementKind::Instance(InstanceProps::default())));
        assert!(
            component.is_classifier()
                && !ElementKind::Instance(InstanceProps::default()).is_classifier()
        );
        let enumeration = ElementKind::Enumeration(ClassifierProps::default());
        assert!(enumeration.can_own(&ElementKind::class()), "types nested in an enum");
        let parameter = ElementKind::TemplateParameter(TemplateParameterProps::default());
        assert!(ElementKind::class().can_own(&parameter));
        assert!(ElementKind::operation(None).can_own(&parameter), "generic methods");
        assert!(!ElementKind::Package.can_own(&parameter));
        assert!(!enumeration.can_own(&parameter));
        assert!(!parameter.is_classifier() && !parameter.can_own(&ElementKind::attribute(None)));
        assert!(RefRole::Type.accepts(&parameter), "attributes typed `T`");
        assert!(!RefRole::General.accepts(&parameter));
    }

    #[test]
    fn transition_labels() {
        let mut t = TransitionProps {
            source: Id::from_u128(1),
            target: Id::from_u128(2),
            trigger: "submit".into(),
            guard: "in stock".into(),
            effect: "ship".into(),
        };
        assert_eq!(t.label(), "submit [in stock] / ship");
        t.trigger.clear();
        t.effect.clear();
        assert_eq!(t.label(), "[in stock]");
    }
}
