//! Maps Astah's object graph onto an UmlCraft model.
//!
//! Astah follows the UML 1.4 metamodel (`JP.co.esm.caddies.uml.*`) plus its own presentation
//! classes (`JP.co.esm.caddies.jomt.jmodel.*`). Only what UmlCraft can represent is imported:
//! packages, classifiers with their features, use cases, components, nodes, artifacts,
//! instances and links, the relationships between them, activities, interactions with their
//! interaction uses, state machines, and the parts, ports and connectors of structured
//! classes; class, use case, sequence, activity, state machine, component, deployment and
//! composite structure diagrams. Everything else is counted in [`Imported::skipped`].

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd, AssociationProps,
    AttributeProps, ClassifierProps, Color, CombinedFragmentProps, CommentProps, ConnectorEnd,
    DiagramKind, DirectedProps, DurationConstraintProps, EdgeLabels, ElementData, ElementKind,
    ExtendProps, GeneralizationProps, Id, IncludeProps, InstanceKind, InstanceProps,
    InteractionOperandProps, InteractionOperator, InteractionUseProps, InternalTransitionProps,
    LifelineProps, LineShape, LinkEnd, LinkProps, MessageGate, MessageProps, MessageSort, Model,
    Multiplicity, Navigability, ObjectEffect, ObjectOrdering, Op, OperationProps,
    ParameterDirection, ParameterDirections, ParameterProps, Point, PortProps, Presentation,
    PseudostateKind, Qualifier, Rect, Regions, Shape, SharedStyle, SlotProps, StateInvariantProps,
    StateProps, Style, TaggedValue, TemplateParameterKind, TemplateParameterProps,
    TerminationProps, TimeConstraintProps, TransitionProps, TypeExpr, TypeRef, Visibility,
};
use umlcraft_notation::composite;
use umlcraft_notation::palette;
use umlcraft_notation::route;
use umlcraft_notation::scene::{self, Outline};
use umlcraft_notation::shared;

use crate::jser::{Graph, ObjId, Value};

mod diagrams;
mod sequence;

/// The result of an import.
#[derive(Debug)]
pub struct Imported {
    pub model: Model,
    /// Astah classes of model elements and diagrams that were not imported, with how often
    /// they occurred. Presentations of skipped elements are left out silently.
    pub skipped: BTreeMap<String, usize>,
    /// Astah classes of elements UmlCraft knows but could not place in the model (no fitting
    /// owner, or the model refused them), with how often.
    pub failed: BTreeMap<String, usize>,
    /// Shapes on diagrams that were left out although their element was imported: the model
    /// refused them, or their ends did not resolve.
    pub lost_presentations: usize,
}

/// Astah's default fill color, not worth carrying over as an explicit style.
const DEFAULT_FILL: &str = "#FFFFCC";
/// How far right of its first stored point Astah draws the loop of a message to self.
const SELF_LOOP: f32 = 49.9;

/// Leftmost and topmost position of imported diagram content.
const MARGIN: f32 = 20.0;

/// Typed access to the fields of Astah objects.
#[derive(Clone, Copy)]
struct Reader<'g> {
    g: &'g Graph,
}

impl<'g> Reader<'g> {
    /// Unqualified class name, e.g. `UClassifierImp`.
    fn class(&self, id: ObjId) -> &'g str {
        let name = self.g.class_name(id).unwrap_or_default();
        name.rsplit('.').next().unwrap_or(name)
    }

    /// The first non-null object in a field of this name; a subclass may declare an unused
    /// field with the same name as the one that holds the value.
    fn obj(&self, id: ObjId, field: &str) -> Option<ObjId> {
        self.g.fields(id, field).find_map(Value::as_object)
    }

    /// The objects of a collection field.
    fn list(&self, id: ObjId, field: &str) -> Vec<ObjId> {
        self.obj(id, field)
            .map(|list| self.g.collection(list).into_iter().filter_map(Value::as_object).collect())
            .unwrap_or_default()
    }

    /// Text of a field: a plain string, or the body or label of the value object (names,
    /// expressions and enumeration kinds are such objects). Astah nests these one or two levels
    /// deep; a longer chain is a damaged file, possibly a cycle, and gives no text.
    fn text(&self, id: ObjId, field: &str) -> String {
        let mut current = self.obj(id, field);
        for _ in 0..16 {
            let Some(value) = current else { break };
            if let Some(s) = self.g.string(value) {
                return s.to_owned();
            }
            current = self.obj(value, "body").or_else(|| self.obj(value, "label"));
        }
        String::new()
    }

    fn name(&self, id: ObjId) -> String {
        self.text(id, "name")
    }

    /// A finite number; NaN and infinity would make the model unsavable.
    fn num(&self, id: ObjId, field: &str) -> Option<f64> {
        self.g.field(id, field).and_then(Value::as_f64).filter(|n| n.is_finite())
    }

    fn flag(&self, id: ObjId, field: &str) -> bool {
        self.g.bool_field(id, field).unwrap_or(false)
    }

    /// The numbers of an array or collection field.
    fn numbers(&self, id: ObjId, field: &str) -> Vec<f64> {
        self.obj(id, field)
            .map(|a| {
                let numbers = self.g.collection(a).into_iter().filter_map(Value::as_f64);
                numbers.filter(|n| n.is_finite()).collect()
            })
            .unwrap_or_default()
    }

    fn point(&self, id: ObjId) -> Option<Point> {
        Some(Point::new(self.num(id, "x")? as f32, self.num(id, "y")? as f32))
    }

    fn points(&self, id: ObjId, field: &str) -> Vec<Point> {
        self.list(id, field).into_iter().filter_map(|p| self.point(p)).collect()
    }

    fn stereotypes(&self, id: ObjId) -> Vec<String> {
        self.list(id, "stereotype").into_iter().map(|s| self.name(s)).collect()
    }

    fn tagged_values(&self, id: ObjId) -> Vec<(String, String)> {
        self.list(id, "taggedValue")
            .into_iter()
            .map(|t| (self.text(t, "tag"), self.text(t, "value")))
            .collect()
    }

    /// A string-to-string map field such as a presentation's `styleMap`.
    fn style(&self, id: ObjId, key: &str) -> Option<String> {
        let map = self.obj(id, "styleMap")?;
        let entries = self.g.collection(map);
        entries.chunks(2).find_map(|pair| {
            let [k, v] = pair else { return None };
            let k = self.g.string(k.as_object()?)?;
            (k == key).then(|| self.g.string(v.as_object()?).map(str::to_owned)).flatten()
        })
    }

    fn visibility(&self, id: ObjId) -> Visibility {
        let mut label = self.text(id, "visibility");
        if label.is_empty()
            && let Some(ownership) = self.obj(id, "namespaceOwnership")
        {
            label = self.text(ownership, "visibility");
        }
        match label.as_str() {
            "private" => Visibility::Private,
            "protected" => Visibility::Protected,
            "package" => Visibility::Package,
            _ => Visibility::Public,
        }
    }

    /// Astah stores `*` as -1 and "not set" as -100; textual bounds cannot be represented.
    fn multiplicity(&self, id: ObjId) -> Option<Multiplicity> {
        let range = *self.list(self.obj(id, "multiplicity")?, "multiplicityRange").first()?;
        if !self.text(range, "lowerValue").is_empty() || !self.text(range, "upperValue").is_empty()
        {
            return None;
        }
        let bound = |field| self.g.field(range, field).and_then(Value::as_i64);
        let (lower, upper) = (bound("lower")?, bound("upper")?);
        let upper = match upper {
            -1 => None,
            u if u >= 1 => Some(u32::try_from(u).ok()?),
            _ => return None,
        };
        // A lone "*" has no lower bound (-1); "0..*" has 0.
        let (lower, star) = match lower {
            -1 => (0, true),
            l if l >= 0 => (u32::try_from(l).ok()?, false),
            _ => return None,
        };
        let m = Multiplicity { lower, upper, star: star && upper.is_none() };
        m.to_string().parse::<Multiplicity>().ok()
    }
}

/// `actionType` of the action behind a message.
const CREATE: i64 = 0;
const REPLY: i64 = 2;
const DESTROY: i64 = 4;

fn action_type(r: Reader<'_>, message: ObjId) -> Option<i64> {
    r.g.field(r.obj(message, "action")?, "actionType").and_then(Value::as_i64)
}

/// Astah's prefixes of its language profiles' tagged values and UmlCraft's names for them.
const LANGUAGE_TAGS: [(&str, &str); 3] =
    [("jude.java.", "java."), ("jude.c_sharp.", "csharp."), ("jude.c_plus.", "cpp.")];

/// A language profile's tagged value under UmlCraft's name: `jude.java.transient` becomes
/// `java.transient`. Other keys stay as they are.
fn neutral_tag(key: &str) -> String {
    LANGUAGE_TAGS
        .iter()
        .find_map(|(astah, ours)| key.strip_prefix(astah).map(|rest| format!("{ours}{rest}")))
        .unwrap_or_else(|| key.to_owned())
}

/// Tagged values Astah uses for itself. Those of its language profiles stay: they hold
/// modifiers such as Java's `transient` (`jude.java.transient`), which UML has no place for.
/// The stereotypes by which Astah tells kinds of actions apart; they are no stereotypes of the
/// user's.
const SEND_SIGNAL: &str = "signal sending";
const ACCEPT_EVENT: &str = "signal receipt";
const ACCEPT_TIME_EVENT: &str = "accept_time_event";
const CONNECTOR: &str = "internal_connector";
const ACTIVITY_NODE_STEREOTYPES: &[&str] =
    &[SEND_SIGNAL, ACCEPT_EVENT, ACCEPT_TIME_EVENT, CONNECTOR];

fn is_internal_tag(key: &str) -> bool {
    let profile = LANGUAGE_TAGS.iter().any(|(astah, _)| key.starts_with(astah));
    (key.starts_with("jude.") && !profile)
        || matches!(key, "hiddenDependency" | "matrix_access_type")
}

/// `ty` followed by `modifier` (`[]`, `*` …); a bound template takes it on directly.
fn with_modifier(ty: TypeRef, modifier: String) -> TypeRef {
    if modifier.is_empty() {
        return ty;
    }
    match ty {
        TypeRef::Expr(mut e) if e.modifier.is_empty() => {
            e.modifier = modifier;
            TypeRef::Expr(e)
        }
        base => TypeRef::Expr(Box::new(TypeExpr {
            base,
            arguments: Vec::new(),
            prefix: String::new(),
            modifier,
        })),
    }
}

/// Stereotypes such as "Java Class" or "C# Method" mark the target language of code generation;
/// Astah does not display them.
fn is_language_marker(stereotype: &str) -> bool {
    stereotype.split_once(' ').is_some_and(|(language, what)| {
        matches!(language, "Java" | "C#" | "C++")
            && !what.is_empty()
            && !what.contains(' ')
            && what.starts_with(char::is_uppercase)
    })
}

/// Astah marks elements of its ER, data flow and requirement notations with tagged values on
/// otherwise plain UML elements.
fn is_foreign_notation(tags: &[(String, String)]) -> bool {
    foreign_notation(tags).is_some()
}

/// The notation that tagged values mark, as reported among the skipped kinds. Data flow
/// diagrams and flowcharts are activity diagrams with such tags.
fn foreign_notation(tags: &[(String, String)]) -> Option<&'static str> {
    tags.iter().find_map(|(key, value)| {
        if key.starts_with("jude.er.") || (key == "jude.type" && value.starts_with("er_")) {
            Some("EntityRelationship")
        } else if key.starts_with("jude.dfd.") {
            Some("DataFlowDiagram")
        } else if key.starts_with("jude.biz.") {
            Some("Flowchart")
        } else if key.starts_with("jude.require.") {
            Some("Requirement")
        } else {
            None
        }
    })
}

/// Astah's names for regions it creates, `Region0`, `Region1` …, or none.
fn is_default_region_name(name: &str) -> bool {
    name.is_empty() || name.trim_end_matches(|c: char| c.is_ascii_digit()) == "Region"
}

/// Names Astah gives control nodes, final states and pseudostates by itself, such as
/// `ActivityNode0`, `ForkNode1` or `InitialPseudostate0`, and the ones UmlCraft's export hands
/// out.
fn is_default_node_name(name: &str) -> bool {
    let stem = name.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    matches!(
        stem,
        "" | "ActivityNode"
            | "InitialNode"
            | "ActivityFinal"
            | "FinalNode"
            | "FlowFinal"
            | "FlowFinalNode"
            | "ForkNode"
            | "JoinNode"
            | "DecisionNode"
            | "MergeNode"
            | "Decision Node & Merge Node"
            | "InitialPseudostate"
            | "FinalState"
            | "ChoicePseudostate"
            | "JunctionPseudostate"
            | "ForkPseudostate"
            | "JoinPseudostate"
            | "ShallowHistoryPseudostate"
            | "DeepHistoryPseudostate"
    )
}

/// The name Astah gives a dimension of lanes it creates by itself.
fn is_default_dimension_name(name: &str) -> bool {
    name.strip_prefix("Dimension").is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
}

struct Importer<'g> {
    r: Reader<'g>,
    model: Model,
    ids: HashMap<ObjId, Id>,
    skipped: BTreeMap<String, usize>,
    failed: BTreeMap<String, usize>,
    lost_presentations: usize,
    features: Vec<(Id, ObjId)>,
    /// Features already in `features`.
    queued: HashSet<ObjId>,
    /// Calls that have a reply message.
    replied: HashSet<ObjId>,
    relationships: Vec<(Id, ObjId)>,
    /// Connectors of structured classes, created once their parts and ports exist.
    connectors: Vec<(Id, ObjId)>,
    /// Instance specifications, created once their classifiers exist.
    instances: Vec<(Id, ObjId)>,
    /// Interaction uses and the Astah interaction they refer to.
    references: Vec<(Id, ObjId)>,
    /// Activity graphs and state machines with the element they belong to.
    behaviors: Vec<(Id, ObjId)>,
    diagrams: Vec<(Id, ObjId)>,
    /// Template classifiers, whose parameters are created once every classifier exists.
    templates: Vec<(Id, ObjId)>,
    /// Bound classes, whose bindings are set once every template parameter exists.
    bound: Vec<(Id, ObjId)>,
    /// Partitions visited, including the dimensions that are left out.
    dimensions: HashSet<ObjId>,
    /// Bound types `type_of` may still expand for the type it was asked for.
    type_budget: Cell<usize>,
    /// Astah's association classes, each one element there: `ids` maps it to its class, this
    /// to its association.
    association_classes: HashMap<ObjId, Id>,
    /// Generalization presentations in a shared group: the style and where the common line
    /// runs (y for the vertical style, x for the horizontal one), before shifting.
    shared: HashMap<ObjId, (SharedStyle, f32)>,
}

/// Most bound types expanded for one type; real ones such as `Map<String, List<Order>>` need
/// a handful.
const MAX_TYPE_NODES: usize = 1000;

pub fn import(graph: &Graph) -> Option<Imported> {
    let r = Reader { g: graph };
    let root = project_model(&r)?;
    let model = Model::new(r.name(root));
    let mut im = Importer {
        r,
        ids: HashMap::from([(root, model.root())]),
        model,
        skipped: BTreeMap::new(),
        failed: BTreeMap::new(),
        lost_presentations: 0,
        features: Vec::new(),
        queued: HashSet::new(),
        replied: HashSet::new(),
        relationships: Vec::new(),
        connectors: Vec::new(),
        instances: Vec::new(),
        references: Vec::new(),
        behaviors: Vec::new(),
        diagrams: Vec::new(),
        templates: Vec::new(),
        bound: Vec::new(),
        dimensions: HashSet::new(),
        type_budget: Cell::new(MAX_TYPE_NODES),
        association_classes: HashMap::new(),
        shared: HashMap::new(),
    };
    let root_id = im.model.root();
    im.walk(root, root_id);
    im.template_parameters();
    im.bindings();
    for (owner, feature) in std::mem::take(&mut im.features) {
        im.feature(owner, feature);
    }
    // Dependencies may connect instances, so they come first.
    for (owner, instance) in std::mem::take(&mut im.instances) {
        im.instance(owner, instance);
    }
    im.links();
    for (owner, relationship) in std::mem::take(&mut im.relationships) {
        im.relationship(owner, relationship);
    }
    // Parts may be association ends, created with their association.
    for (owner, connector) in std::mem::take(&mut im.connectors) {
        im.connector(owner, connector);
    }
    for (owner, behavior) in std::mem::take(&mut im.behaviors) {
        if im.r.class(behavior) == "UStateMachineImp" {
            im.state_machine(owner, behavior);
        } else {
            im.activity(owner, behavior);
        }
    }
    im.unowned_diagrams();
    for (owner, diagram) in std::mem::take(&mut im.diagrams) {
        im.diagram(owner, diagram);
    }
    im.resolve_references();
    Some(Imported {
        model: im.model,
        skipped: im.skipped,
        failed: im.failed,
        lost_presentations: im.lost_presentations,
    })
}

/// The project's model: `EntityRoot.entry["ProjectModel"]`, or else the first model object.
fn project_model(r: &Reader<'_>) -> Option<ObjId> {
    let g = r.g;
    let all = || (0..g.objects.len()).map(ObjId);
    let from_entry = all().filter(|&id| r.class(id) == "EntityRoot").find_map(|root| {
        let entries = g.collection(r.obj(root, "entry")?);
        entries.chunks(2).find_map(|pair| {
            let [k, v] = pair else { return None };
            (g.string(k.as_object()?)? == "ProjectModel").then(|| v.as_object()).flatten()
        })
    });
    from_entry.or_else(|| all().find(|&id| r.class(id) == "UModelImp"))
}

impl Importer<'_> {
    fn skip(&mut self, object: ObjId) {
        *self.skipped.entry(self.r.class(object).to_owned()).or_default() += 1;
    }

    /// Name, visibility, stereotypes, tagged values and documentation of an Astah element.
    fn data(&self, object: ObjId, kind: ElementKind, hide_stereotypes: &[&str]) -> ElementData {
        let mut data = ElementData::new(self.r.name(object), kind);
        data.visibility = self.r.visibility(object);
        data.stereotypes = self
            .r
            .stereotypes(object)
            .into_iter()
            .filter(|s| {
                !s.is_empty() && !hide_stereotypes.contains(&s.as_str()) && !is_language_marker(s)
            })
            .collect();
        data.tagged_values = self
            .r
            .tagged_values(object)
            .into_iter()
            .filter(|(key, _)| !key.is_empty() && !is_internal_tag(key))
            .map(|(key, value)| TaggedValue { key: neutral_tag(&key), value })
            .collect();
        data.documentation = self.r.text(object, "definition");
        data.constraints = self.constraints(object);
        data
    }

    /// The constraints of an Astah element, as Astah shows them: the specification (`body`),
    /// else the name. Duration and time constraints are elements of their own.
    fn constraints(&self, object: ObjId) -> Vec<String> {
        let r = self.r;
        r.list(object, "constraint")
            .into_iter()
            .filter(|&c| r.class(c) == "UConstraintImp")
            .map(|c| Some(r.text(c, "body")).filter(|b| !b.is_empty()).unwrap_or_else(|| r.name(c)))
            .filter(|c| !c.is_empty())
            .collect()
    }

    /// Inserts an element under `owner`, or under the nearest ancestor that may own it.
    ///
    /// Each Astah object becomes at most one element. An object reached a second time means a
    /// damaged file in which, say, a package owns itself; refusing it ends the walks that
    /// recurse into what they create.
    fn create(&mut self, owner: Id, data: ElementData, object: Option<ObjId>) -> Option<Id> {
        if object.is_some_and(|o| self.ids.contains_key(&o)) {
            return None;
        }
        let owner = std::iter::once(owner)
            .chain(self.model.ancestors(owner))
            .find(|&o| self.model.get(o).is_some_and(|e| e.kind().can_own(&data.kind)));
        let id = Id::generate();
        let inserted = owner.map(|owner| {
            let index = self.model.children(owner).len();
            self.model.apply(Op::InsertElement { id, owner, index, data })
        });
        match inserted {
            Some(Ok(_)) => {
                if let Some(object) = object {
                    self.ids.insert(object, id);
                }
                Some(id)
            }
            _ => {
                if let Some(object) = object {
                    *self.failed.entry(self.r.class(object).to_owned()).or_default() += 1;
                }
                None
            }
        }
    }

    /// Whether a classifier of Astah's stands for a template parameter. Some classes declare
    /// `templateParameter` as a (usually empty) list.
    fn is_template_parameter(&self, classifier: ObjId) -> bool {
        ["owningParameter", "templateParameter"].iter().any(|&field| {
            self.r.obj(classifier, field).is_some_and(|p| self.r.class(p) != "ArrayList")
        })
    }

    fn kind_of(&self, object: ObjId) -> Option<&ElementKind> {
        self.ids.get(&object).and_then(|id| self.model.get(*id)).map(|e| e.kind())
    }

    fn classifier(&self, object: ObjId) -> Option<Id> {
        self.kind_of(object).filter(|k| k.is_classifier())?;
        self.ids.get(&object).copied()
    }

    /// Creates the elements owned by `object`. Features, relationships and diagrams wait until
    /// every classifier exists, so their references can be resolved.
    fn walk(&mut self, object: ObjId, id: Id) {
        // Behaviors are not owned elements; the element they describe lists them.
        for behavior in self.r.list(object, "behavior") {
            match self.r.class(behavior) {
                "UActivityGraphImp" | "UStateMachineImp" => self.behaviors.push((id, behavior)),
                _ => self.skip(behavior),
            }
        }
        for ownership in self.r.list(object, "ownedElementOwnership") {
            let Some(child) = self.r.obj(ownership, "ownedElement") else { continue };
            if is_foreign_notation(&self.r.tagged_values(child)) {
                self.skip(child);
                continue;
            }
            match self.r.class(child) {
                "UPackageImp" | "USubsystemImp" | "UModelImp" => {
                    let mut data = self.data(child, ElementKind::Package, &[]);
                    // Astah keeps subsystems as a class of their own; UmlCraft as a stereotype.
                    if self.r.class(child) == "USubsystemImp"
                        && !data.stereotypes.iter().any(|s| s == palette::SUBSYSTEM)
                    {
                        data.stereotypes.insert(0, palette::SUBSYSTEM.to_owned());
                    }
                    if let Some(package) = self.create(id, data, Some(child)) {
                        self.walk(child, package);
                    }
                }
                "UClassifierImp"
                | "UInterfaceImp"
                | "UActorImp"
                | "UUseCaseImp"
                | "UComponentImp"
                | "UArtifactImp"
                | "UNodeImp"
                | "UAssociationClassImp" => {
                    // Anonymous classifiers bind a template to arguments; they live on as type
                    // expressions such as `List<Projekt>` (see `type_of`), and as bound classes
                    // where a diagram shows them, a lifeline represents them (`baseInv`) or a
                    // relationship ends at them (`implements Comparable<Order>`, an association
                    // to `List<User>`).
                    let bound = !self.r.list(child, "templateBinding").is_empty();
                    let referenced = [
                        "presentation",
                        "baseInv",
                        "supplierDependency",
                        "clientDependency",
                        "specialization",
                        "generalization",
                        "associationEnd",
                    ]
                    .iter()
                    .any(|field| !self.r.list(child, field).is_empty());
                    if bound && self.r.name(child).is_empty() && !referenced {
                        continue;
                    }
                    // Template parameters are classifiers owned by a template parameter; they
                    // are created with their template (see `template_parameters`).
                    if self.is_template_parameter(child) {
                        continue;
                    }
                    let (kind, hide) = self.classifier_kind(child);
                    let data = self.data(child, kind, hide);
                    if let Some(classifier) = self.create(id, data, Some(child)) {
                        // An association class is a class and an association in one: its
                        // association comes with the other relationships.
                        if self.r.class(child) == "UAssociationClassImp" {
                            self.relationships.push((id, child));
                        }
                        if self.r.obj(child, "ownedTemplateSignatrue").is_some() {
                            self.templates.push((classifier, child));
                        }
                        if bound {
                            self.bound.push((classifier, child));
                        }
                        // The feature lists hold the order Astah displays; ownership does not.
                        for list in ["structuralFeature", "behavioralFeature"] {
                            for feature in self.r.list(child, list) {
                                if self.queued.insert(feature) {
                                    self.features.push((classifier, feature));
                                }
                            }
                        }
                        self.walk(child, classifier);
                    }
                }
                "UExtensionPointImp" => {
                    let data = self.data(child, ElementKind::ExtensionPoint, &[]);
                    self.create(id, data, Some(child));
                }
                // Features listed by their classifier are queued above already. Ports are owned
                // without being listed.
                "UAttributeImp" | "UOperationImp" | "UPortImp" => {
                    if self.queued.insert(child) {
                        self.features.push((id, child));
                    }
                }
                // Parameters come with their operation, association ends with their association,
                // connector ends with their connector.
                "JUParameterImp" | "UParameterImp" | "UAssociationEndImp" | "UConnectorEndImp" => {}
                "UConnectorImp" => self.connectors.push((id, child)),
                "UAssociationImp"
                | "UGeneralizationImp"
                | "UDependencyImp"
                | "UUsageImp"
                | "UAbstractionImp"
                | "URealizationImp"
                | "UIncludeImp"
                | "UExtendImp"
                | "UTemplateBindingImp" => {
                    self.relationships.push((id, child));
                }
                "UDiagramImp" | "USequenceDiagramImp" => self.diagrams.push((id, child)),
                "UComponentInstanceImp" | "UNodeInstanceImp" | "UObjectImp" => {
                    self.instances.push((id, child));
                }
                // Constraints come with the element they constrain (see `constraints`).
                "UConstraintImp" => {}
                _ => self.skip(child),
            }
        }
    }

    /// Diagrams outside the ownership tree. A sequence diagram that belongs to a classifier, use
    /// case or operation is reachable only through its collaboration, which names that element
    /// as `representedClassifier` or `representedOperation`; an operation's interaction goes to
    /// its class (see `create`). Others go to the model root.
    fn unowned_diagrams(&mut self) {
        let queued: HashSet<ObjId> = self.diagrams.iter().map(|&(_, d)| d).collect();
        let r = self.r;
        for diagram in (0..r.g.objects.len()).map(ObjId) {
            if !matches!(r.class(diagram), "UDiagramImp" | "USequenceDiagramImp")
                || queued.contains(&diagram)
            {
                continue;
            }
            let collaboration = r.obj(diagram, "collaboration");
            let owner = ["representedClassifier", "representedOperation"]
                .iter()
                .find_map(|field| collaboration.and_then(|c| r.obj(c, field)))
                .and_then(|c| self.ids.get(&c).copied())
                .unwrap_or_else(|| self.model.root());
            self.diagrams.push((owner, diagram));
        }
    }

    fn classifier_kind(&self, object: ObjId) -> (ElementKind, &'static [&'static str]) {
        let props = ClassifierProps {
            is_abstract: self.r.flag(object, "abst"),
            is_leaf: self.r.flag(object, "leaf"),
            is_active: self.r.flag(object, "active"),
            binding: None,
        };
        let stereotypes = self.r.stereotypes(object);
        let has = |s: &str| stereotypes.iter().any(|t| t == s);
        let enum_first = stereotypes.first().is_some_and(|s| s == "enum");
        let constants = self
            .r
            .list(object, "structuralFeature")
            .iter()
            .any(|&a| self.r.stereotypes(a).iter().any(|s| s == "enum constant"));
        match self.r.class(object) {
            "UUseCaseImp" => (ElementKind::UseCase(props), &[]),
            "UComponentImp" => (ElementKind::Component(props), &[]),
            "UArtifactImp" => (ElementKind::Artifact(props), &[]),
            "UNodeImp" => (ElementKind::Node(props), &[]),
            "UInterfaceImp" => (ElementKind::Interface(props), &["interface"]),
            "UActorImp" => (ElementKind::Actor(props), &["actor"]),
            _ if has("interface") => (ElementKind::Interface(props), &["interface"]),
            _ if has("actor") => (ElementKind::Actor(props), &["actor"]),
            _ if has("enumeration") => (ElementKind::Enumeration(props), &["enumeration"]),
            // A Java enum imported by Astah: an enumeration when «enum» comes first and there
            // are «enum constant»s. Otherwise a class, which shows «enum» only in front.
            _ if enum_first && constants => (ElementKind::Enumeration(props), &["enum"]),
            _ if has("dataType") || has("datatype") => {
                (ElementKind::DataType(props), &["dataType", "datatype"])
            }
            _ if enum_first => (ElementKind::Class(props), &[]),
            _ => (ElementKind::Class(props), &["enum"]),
        }
    }

    /// Points interaction uses to the interactions they refer to, call behavior actions to the
    /// activities they call and submachine states to their state machines, now that all exist.
    fn resolve_references(&mut self) {
        for (id, target) in std::mem::take(&mut self.references) {
            let Some(&behavior) = self.ids.get(&target) else { continue };
            let Some(target_kind) = self.model.get(behavior).map(|e| e.kind().clone()) else {
                continue;
            };
            let Some(e) = self.model.get(id) else { continue };
            let mut data = e.data().clone();
            match (&mut data.kind, target_kind) {
                (ElementKind::InteractionUse(p), ElementKind::Interaction) => {
                    p.refers_to = Some(behavior);
                }
                (ElementKind::ActivityNode(p), ElementKind::Activity) => {
                    p.behavior = Some(behavior);
                }
                (ElementKind::State(p), ElementKind::StateMachine) => {
                    p.submachine = Some(behavior);
                }
                _ => continue,
            }
            let _ = self.model.apply(Op::UpdateElement { id, data });
        }
    }

    /// An object, component instance or node instance of the first of its classifiers.
    fn instance(&mut self, owner: Id, object: ObjId) {
        let instance = match self.r.class(object) {
            "UComponentInstanceImp" => InstanceKind::Component,
            "UNodeInstanceImp" => InstanceKind::Node,
            _ => InstanceKind::Object,
        };
        let classifier =
            self.r.list(object, "classifiers").first().and_then(|&c| self.classifier(c));
        let data =
            self.data(object, ElementKind::Instance(InstanceProps { instance, classifier }), &[]);
        let Some(id) = self.create(owner, data, Some(object)) else { return };
        // Slots: the value is an instance whose name is the value's text.
        for slot in self.r.list(object, "slots") {
            let attribute = self.r.obj(slot, "attribute").and_then(|a| self.ids.get(&a).copied());
            let value = self.r.obj(slot, "value").map(|v| self.r.name(v)).unwrap_or_default();
            if attribute.is_none() || value.is_empty() {
                continue;
            }
            let data = self.data(slot, ElementKind::Slot(SlotProps { attribute, value }), &[]);
            self.create(id, data, Some(slot));
        }
    }

    /// Links between objects. Astah keeps them outside the ownership tree, reachable from their
    /// objects only; they go next to their first object. Navigability and aggregation of an end
    /// are tagged values.
    fn links(&mut self) {
        let r = self.r;
        for link in (0..r.g.objects.len()).map(ObjId).filter(|&o| r.class(o) == "ULinkImp") {
            let ends: Vec<LinkEnd> = r
                .list(link, "connections")
                .into_iter()
                .filter_map(|end| {
                    let instance = *self.ids.get(&r.obj(end, "instance")?)?;
                    let navigability = match self.tag(end, "object_navigable_type").as_str() {
                        "navigable" => Navigability::Navigable,
                        "non navigable" => Navigability::NonNavigable,
                        _ => Navigability::Unspecified,
                    };
                    let aggregation = match self.tag(end, "object_aggregation_kind").as_str() {
                        "aggregate" => Aggregation::Shared,
                        "composite" => Aggregation::Composite,
                        _ => Aggregation::None,
                    };
                    Some(LinkEnd { instance, role: r.name(end), navigability, aggregation })
                })
                .collect();
            let Ok(ends) = <[LinkEnd; 2]>::try_from(ends) else {
                self.skip(link);
                continue;
            };
            let association = r.obj(link, "association").and_then(|a| self.ids.get(&a).copied());
            let owner = self.model.get(ends[0].instance).and_then(|e| e.owner());
            let owner = owner.unwrap_or_else(|| self.model.root());
            let data = self.data(link, ElementKind::Link(LinkProps { ends, association }), &[]);
            self.create(owner, data, Some(link));
        }
    }

    /// The type of an attribute, parameter, qualifier or template parameter default. Astah marks
    /// arrays with a `jude.type_modifier` tag (`[][]`), or with one unset multiplicity range per
    /// dimension when the type was written as `Foo[]`. Like typed text (`member::parse_type`), a
    /// type that links to nothing in the model stays text.
    fn type_ref(&self, typed: ObjId) -> Option<TypeRef> {
        let ty = self.type_of(self.r.obj(typed, "type")?, 0)?;
        let mut modifier = self.tag(typed, "jude.type_modifier");
        // An unset range makes an attribute an array. Association ends made through Astah's API
        // carry one too, there meaning no multiplicity.
        if modifier.is_empty() && self.r.class(typed) != "UAssociationEndImp" {
            let unset = |range: &ObjId| {
                let bound = |field| self.r.g.field(*range, field).and_then(Value::as_i64);
                bound("lower") == Some(-100) && bound("upper") == Some(-100)
            };
            let ranges =
                self.r.obj(typed, "multiplicity").map(|m| self.r.list(m, "multiplicityRange"));
            modifier = "[]".repeat(ranges.unwrap_or_default().iter().filter(|r| unset(r)).count());
        }
        let ty = with_modifier(ty, modifier);
        Some(match ty {
            TypeRef::Expr(_) if ty.ids().is_empty() => {
                TypeRef::Text(umlcraft_notation::member::type_text(&self.model, &ty))
            }
            ty => ty,
        })
    }

    /// A type object: a classifier or template parameter of the model, a bound template such
    /// as `List<Projekt>` (an anonymous classifier whose template binding names the template
    /// and the arguments), or else the type's name as text.
    fn type_of(&self, ty: ObjId, depth: usize) -> Option<TypeRef> {
        if depth == 0 {
            self.type_budget.set(MAX_TYPE_NODES);
        }
        let r = self.r;
        let name = r.name(ty);
        let anonymous = name.is_empty() && !r.list(ty, "templateBinding").is_empty();
        // Types stay expressions even where a diagram shows the bound class.
        if let Some(&id) = self.ids.get(&ty).filter(|_| !anonymous)
            && self.model.get(id).is_some_and(|e| {
                e.kind().is_classifier() || matches!(e.kind(), ElementKind::TemplateParameter(_))
            })
        {
            return Some(TypeRef::Element(id));
        }
        if anonymous {
            return self.bound_type(ty, depth);
        }
        (!name.is_empty()).then_some(TypeRef::Text(name))
    }

    /// The template and arguments a classifier binds, as `Pair<int,Holder>`.
    fn bound_type(&self, ty: ObjId, depth: usize) -> Option<TypeRef> {
        if depth == 0 {
            self.type_budget.set(MAX_TYPE_NODES);
        }
        // The depth limit alone does not bound the work: arguments naming bound classes again
        // multiply at every level.
        let budget = self.type_budget.get().checked_sub(1)?;
        self.type_budget.set(budget);
        let r = self.r;
        let binding = r.list(ty, "templateBinding").first().copied();
        if let Some(binding) = binding.filter(|_| depth < 8)
            && let Some(signature) = r.obj(binding, "signature")
        {
            let base = r.obj(signature, "template").and_then(|t| self.type_of(t, depth + 1))?;
            let formals = r.list(signature, "ownedParameter");
            let mut substitutions = r.list(binding, "parameterSubstition");
            let position = |s: &ObjId| {
                let formal = r.obj(*s, "formal");
                formals.iter().position(|f| Some(*f) == formal).unwrap_or(usize::MAX)
            };
            substitutions.sort_by_key(position);
            let arguments =
                substitutions.iter().filter_map(|&s| self.actual(s, depth + 1)).collect();
            return Some(TypeRef::Expr(Box::new(TypeExpr {
                base,
                arguments,
                prefix: String::new(),
                modifier: String::new(),
            })));
        }
        None
    }

    /// Sets the bindings of bound classes, now that templates and their parameters exist.
    fn bindings(&mut self) {
        for (id, object) in std::mem::take(&mut self.bound) {
            let binding = self.bound_type(object, 0);
            let Some(e) = self.model.get(id) else { continue };
            let mut data = e.data().clone();
            if let ElementKind::Class(p) = &mut data.kind {
                p.binding = binding;
                let _ = self.model.apply(Op::UpdateElement { id, data });
            }
        }
    }

    /// The argument of a template parameter substitution: a type, or a value such as `20`.
    fn actual(&self, substitution: ObjId, depth: usize) -> Option<TypeRef> {
        let r = self.r;
        let argument = match r.list(substitution, "actual").first() {
            Some(&a) if r.class(a) == "UExpression" => Some(TypeRef::Text(r.text(a, "body"))),
            Some(&a) => self.type_of(a, depth),
            None => None,
        };
        let argument = argument.filter(|a| *a != TypeRef::Text(String::new())).or_else(|| {
            Some(self.tag(substitution, "actual_value"))
                .filter(|v| !v.is_empty())
                .map(TypeRef::Text)
        })?;
        Some(with_modifier(argument, self.tag(substitution, "jude.type_modifier")))
    }

    /// The value of one of Astah's tagged values, or an empty string.
    fn tag(&self, object: ObjId, key: &str) -> String {
        let tags = self.r.tagged_values(object);
        tags.into_iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or_default()
    }

    /// Creates the template parameters of template classifiers, before the features that use
    /// them. Astah's parameter owns a classifier that stands for it: its generalizations are
    /// the bounds, and a `jude.type` tag makes it a value parameter of that type.
    fn template_parameters(&mut self) {
        let r = self.r;
        for (owner, template) in std::mem::take(&mut self.templates) {
            let Some(signature) = r.obj(template, "ownedTemplateSignatrue") else { continue };
            for parameter in r.list(signature, "ownedParameter") {
                let Some(element) = r.obj(parameter, "ownedParameteredElement") else {
                    self.skip(parameter);
                    continue;
                };
                let value_type = Some(self.tag(element, "jude.type")).filter(|t| !t.is_empty());
                let bounds = r
                    .list(element, "generalization")
                    .into_iter()
                    .filter_map(|g| r.obj(g, "supertype"))
                    .filter_map(|s| self.type_of(s, 0))
                    .collect();
                // A value default is an expression whose body is the text.
                let default = r.obj(parameter, "defaultValue").and_then(|d| match r.g.string(d) {
                    Some(text) => Some(TypeRef::Text(text.to_owned())),
                    None if r.class(d) == "UExpression" => {
                        Some(r.text(d, "body")).filter(|b| !b.is_empty()).map(TypeRef::Text)
                    }
                    None => self.type_of(d, 0),
                });
                let props = TemplateParameterProps {
                    parameter: if value_type.is_some() {
                        TemplateParameterKind::Value
                    } else {
                        TemplateParameterKind::Type
                    },
                    type_ref: value_type.map(TypeRef::Text),
                    bounds,
                    default,
                    is_variadic: false,
                };
                let data = self.data(element, ElementKind::TemplateParameter(props), &[]);
                self.create(owner, data, Some(element));
            }
        }
    }

    fn feature(&mut self, owner: Id, object: ObjId) {
        let is_static = self.r.text(object, "ownerScope") == "classifier";
        if self.r.class(object) == "UPortImp" {
            let kind = ElementKind::Port(PortProps {
                type_ref: self.type_ref(object),
                multiplicity: self.r.multiplicity(object),
                is_behavior: self.r.flag(object, "isBehavior"),
            });
            let data = self.data(object, kind, &[]);
            self.create(owner, data, Some(object));
            return;
        }
        if self.r.class(object) == "UAttributeImp" {
            let owner_is_enum = matches!(
                self.model.get(owner).map(|e| e.kind()),
                Some(ElementKind::Enumeration(_))
            );
            // In a Java enum only the «enum constant»s are literals.
            let java_enum = self
                .r
                .obj(object, "owner")
                .is_some_and(|o| self.r.stereotypes(o).first().is_some_and(|s| s == "enum"));
            let constant = self.r.stereotypes(object).iter().any(|s| s == "enum constant");
            let kind = if owner_is_enum && (constant || !java_enum) {
                ElementKind::EnumerationLiteral
            } else {
                ElementKind::Attribute(AttributeProps {
                    type_ref: self.type_ref(object),
                    multiplicity: self.r.multiplicity(object),
                    default_value: self.r.text(object, "initialValue"),
                    is_static,
                    is_derived: false,
                    is_read_only: matches!(
                        self.r.text(object, "changeable").as_str(),
                        "frozen" | "readOnly"
                    ),
                })
            };
            let literal = matches!(kind, ElementKind::EnumerationLiteral);
            let data = self.data(object, kind, if literal { &["enum constant"] } else { &[] });
            self.create(owner, data, Some(object));
            return;
        }

        let parameters = self.r.list(object, "parameter");
        let r = self.r;
        let is_return = move |p: &ObjId| r.text(*p, "kind") == "return";
        // Astah shows a return type, "void" included, exactly when there is a return parameter.
        let return_type = parameters.iter().find(|p| is_return(p)).and_then(|&p| self.type_ref(p));
        let kind = ElementKind::Operation(OperationProps {
            return_type,
            is_static,
            is_abstract: self.r.flag(object, "isAbst"),
            is_leaf: self.r.flag(object, "leaf"),
            is_query: self.r.flag(object, "query"),
        });
        let data = self.data(object, kind, &[]);
        let Some(operation) = self.create(owner, data, Some(object)) else { return };
        for parameter in parameters.into_iter().filter(|p| !is_return(p)) {
            let direction = match self.r.text(parameter, "kind").as_str() {
                "out" => ParameterDirection::Out,
                "inout" => ParameterDirection::InOut,
                _ => ParameterDirection::In,
            };
            let kind = ElementKind::Parameter(ParameterProps {
                type_ref: self.type_ref(parameter),
                direction,
                multiplicity: self.r.multiplicity(parameter),
                default_value: self.r.text(parameter, "defaultValue"),
            });
            let data = self.data(parameter, kind, &[]);
            self.create(operation, data, Some(parameter));
        }
    }

    fn relationship(&mut self, owner: Id, object: ObjId) {
        if self.r.class(object) == "UAssociationImp" && self.parts(object) {
            return;
        }
        let r = self.r;
        let mapped = |field: &str| r.obj(object, field).and_then(|o| self.ids.get(&o).copied());
        let first =
            |field: &str| r.list(object, field).first().and_then(|o| self.ids.get(o).copied());
        let class = r.class(object);
        let stereotypes = r.stereotypes(object);
        let kind = match class {
            // Its name, members, stereotypes and constraints are its class's.
            "UAssociationClassImp" => {
                let ends: Vec<AssociationEnd> = r
                    .list(object, "connection")
                    .into_iter()
                    .filter_map(|end| self.association_end(end))
                    .collect();
                let class = self.ids.get(&object).copied();
                let Ok(ends) = <[AssociationEnd; 2]>::try_from(ends) else {
                    self.skip(object);
                    return;
                };
                let kind = ElementKind::Association(AssociationProps { ends, class });
                if let Some(id) = self.create(owner, ElementData::new("", kind), None) {
                    self.association_classes.insert(object, id);
                }
                return;
            }
            "UAssociationImp" => {
                let ends: Vec<AssociationEnd> = r
                    .list(object, "connection")
                    .into_iter()
                    .filter_map(|end| self.association_end(end))
                    .collect();
                ends.try_into()
                    .ok()
                    .map(|ends| ElementKind::Association(AssociationProps { ends, class: None }))
            }
            // A template parameter's generalizations are its bounds (see `template_parameters`).
            "UGeneralizationImp"
                if r.obj(object, "subtype").is_some_and(|s| self.is_template_parameter(s)) =>
            {
                return;
            }
            "UGeneralizationImp" => {
                let specific = r.obj(object, "subtype").and_then(|o| self.classifier(o));
                let general = r.obj(object, "supertype").and_then(|o| self.classifier(o));
                specific.zip(general).map(|(specific, general)| {
                    ElementKind::Generalization(GeneralizationProps { specific, general })
                })
            }
            // A binding shown on a diagram is a «bind» dependency of the bound class on its
            // template; the class holds what it binds (see `bindings`). Bindings of types that
            // are not shown live on in the types.
            "UTemplateBindingImp" => {
                let template = r.obj(object, "signature").and_then(|s| r.obj(s, "template"));
                let client = r.obj(object, "boundElement").and_then(|o| self.classifier(o));
                let Some((client, supplier)) =
                    client.zip(template.and_then(|t| self.classifier(t)))
                else {
                    return;
                };
                let mut data = self.data(
                    object,
                    ElementKind::Dependency(DirectedProps { client, supplier }),
                    &["bind"],
                );
                data.stereotypes.insert(0, "bind".into());
                self.create(owner, data, Some(object));
                return;
            }
            // Astah's fields are swapped against UML: `addition` is the including use case.
            "UIncludeImp" => mapped("addition").zip(mapped("base")).map(|(including, addition)| {
                ElementKind::Include(IncludeProps { including, addition })
            }),
            "UExtendImp" => mapped("extension").zip(mapped("base")).map(|(extension, extended)| {
                ElementKind::Extend(ExtendProps {
                    extension,
                    extended,
                    extension_points: r
                        .list(object, "location")
                        .iter()
                        .filter_map(|p| self.ids.get(p).copied())
                        .collect(),
                    condition: r.text(object, "condition"),
                })
            }),
            _ => {
                let hidden = r
                    .tagged_values(object)
                    .iter()
                    .any(|(k, v)| k == "hiddenDependency" && v == "true");
                // Astah's own bookkeeping, not a dependency the user drew: nothing is lost.
                if hidden {
                    return;
                }
                let realize = class == "URealizationImp"
                    || stereotypes.iter().any(|s| s == "realize" || s == "realization");
                first("client").zip(first("supplier")).map(|(client, supplier)| {
                    let props = DirectedProps { client, supplier };
                    if realize {
                        ElementKind::Realization(props)
                    } else {
                        ElementKind::Dependency(props)
                    }
                })
            }
        };
        let Some(kind) = kind else {
            self.skip(object);
            return;
        };
        let hide: &[&str] = match &kind {
            ElementKind::Include(_) => &["include"],
            ElementKind::Extend(_) => &["extend"],
            ElementKind::Realization(_) => &["realize", "realization"],
            _ => &[],
        };
        let usage = matches!(kind, ElementKind::Dependency(_));
        let mut data = self.data(object, kind, hide);
        // A usage is a dependency marked «use», unless Astah made it a realization (a component
        // providing an interface on a deployment diagram).
        if class == "UUsageImp" && usage && !data.stereotypes.iter().any(|s| s == "use") {
            data.stereotypes.insert(0, "use".into());
        }
        self.create(owner, data, Some(object));
    }

    /// Astah may draw a part in a structured class for the end of a composite association that
    /// no diagram shows. UmlCraft keeps parts as attributes: such an end becomes an attribute
    /// of the class at the other end, and the association is not imported. Returns whether the
    /// association was taken so.
    fn parts(&mut self, association: ObjId) -> bool {
        let r = self.r;
        let ends = r.list(association, "connection");
        let [a, b] = ends[..] else { return false };
        let is_part = |end: ObjId| {
            r.list(end, "presentation").iter().any(|&p| r.class(p) == "PartPresentation")
                || !r.list(end, "connectorEnd").is_empty()
        };
        if !r.list(association, "presentation").is_empty() || !is_part(a) && !is_part(b) {
            return false;
        }
        for (end, whole) in [(a, b), (b, a)].into_iter().filter(|&(end, _)| is_part(end)) {
            let Some(class) = r.obj(whole, "type").and_then(|t| self.classifier(t)) else {
                self.skip(end);
                continue;
            };
            let kind = ElementKind::Attribute(AttributeProps {
                type_ref: self.type_ref(end),
                multiplicity: r.multiplicity(end),
                default_value: r.text(end, "initialValue"),
                is_static: false,
                is_derived: false,
                is_read_only: matches!(r.text(end, "changeable").as_str(), "frozen" | "readOnly"),
            });
            let data = self.data(end, kind, &[]);
            self.create(class, data, Some(end));
        }
        true
    }

    /// A connector of a structured class between its parts and ports.
    fn connector(&mut self, owner: Id, object: ObjId) {
        let r = self.r;
        let ends: Vec<ConnectorEnd> = r
            .list(object, "connection")
            .into_iter()
            .filter_map(|end| {
                let role = *self.ids.get(&r.obj(end, "role")?)?;
                let part_with_port = r.obj(end, "partWithPort").and_then(|p| self.ids.get(&p));
                let mut result = ConnectorEnd::new(role, part_with_port.copied());
                result.multiplicity = r.multiplicity(end);
                Some(result)
            })
            .collect();
        let Ok([a, b]) = <[ConnectorEnd; 2]>::try_from(ends) else {
            self.skip(object);
            return;
        };
        let data = self.data(object, ElementKind::connector(a, b), &[]);
        self.create(owner, data, Some(object));
    }

    fn association_end(&self, end: ObjId) -> Option<AssociationEnd> {
        let r = self.r;
        let mut result = AssociationEnd::new(self.classifier(r.obj(end, "type")?)?);
        result.role = r.name(end);
        result.multiplicity = r.multiplicity(end);
        result.visibility = r.visibility(end);
        result.constraints = r
            .list(end, "constraint")
            .into_iter()
            .map(|c| Some(r.text(c, "body")).filter(|b| !b.is_empty()).unwrap_or_else(|| r.name(c)))
            .filter(|c| !c.is_empty())
            .collect();
        result.navigability = match r.text(end, "navigableType").as_str() {
            "navigable" => Navigability::Navigable,
            "non navigable" | "unnavigable" | "not navigable" => Navigability::NonNavigable,
            _ => Navigability::Unspecified,
        };
        result.aggregation = match r.text(end, "aggregation").as_str() {
            "composite" => Aggregation::Composite,
            "aggregate" | "shared" => Aggregation::Shared,
            _ => Aggregation::None,
        };
        result.is_read_only = matches!(r.text(end, "changeable").as_str(), "frozen" | "readOnly");
        result.is_static = r.text(end, "targetScope") == "classifier";
        result.default_value = r.text(end, "initialValue");
        // A qualifier belongs to the end whose participant its box is drawn at.
        result.qualifiers = r
            .list(end, "qualifier")
            .into_iter()
            .map(|q| Qualifier { name: r.name(q), type_ref: self.type_ref(q) })
            .collect();
        Some(result)
    }

    /// Creates an activity with its nodes, flows and partitions, and queues its diagram.
    /// Astah keeps UML 1.4's activity graphs: states of a state machine connected by
    /// transitions.
    fn activity(&mut self, owner: Id, graph: ObjId) {
        let r = self.r;
        let diagram = r.obj(graph, "diagram");
        let mut tags = r.tagged_values(graph);
        tags.extend(diagram.map(|d| r.tagged_values(d)).unwrap_or_default());
        if let Some(notation) = foreign_notation(&tags) {
            *self.skipped.entry(notation.to_owned()).or_default() += 1;
            return;
        }
        let data = self.data(graph, ElementKind::Activity, &[]);
        let Some(activity) = self.create(owner, data, Some(graph)) else { return };
        for partition in r.list(graph, "partition") {
            self.partition(activity, partition);
        }
        let vertices = r.obj(graph, "top").map(|top| r.list(top, "subvertex")).unwrap_or_default();
        // Pins after the actions they belong to.
        let kinds: Vec<(ObjId, Option<ActivityNodeKind>)> =
            vertices.iter().map(|&v| (v, self.activity_node_kind(v))).collect();
        let (pins, others): (Vec<_>, Vec<_>) =
            kinds.into_iter().partition(|(_, k)| k.is_some_and(|k| k.is_pin()));
        for (vertex, node) in others.into_iter().chain(pins) {
            let Some(node) = node else {
                self.skip(vertex);
                continue;
            };
            // A pin belongs to its action, which Astah links to it by a dependency marked as
            // ownership (`jude.uml.owned_element`), from the action to the pin.
            let owner = if node.is_pin() {
                let action = r
                    .list(vertex, "supplierDependency")
                    .into_iter()
                    .flat_map(|d| r.list(d, "client"))
                    .find_map(|a| self.ids.get(&a).copied());
                let Some(action) = action else {
                    self.skip(vertex);
                    continue;
                };
                action
            } else {
                activity
            };
            // Object nodes have a classifier in a state as their type: the classifier and the
            // states it is in. Astah keeps `filled, paid` typed in as one state of that name.
            let in_state = r.obj(vertex, "type").filter(|_| node.is_object());
            let type_ref = in_state.and_then(|t| self.type_ref(t));
            let in_state = in_state
                .map(|t| r.list(t, "inStates").into_iter().map(|s| r.name(s)).collect::<Vec<_>>())
                .unwrap_or_default()
                .join(", ");
            // Object node properties are tagged values of Astah's own.
            let tag = |key: &str| {
                r.tagged_values(vertex).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
            };
            let props = ActivityNodeProps {
                node,
                type_ref,
                in_state,
                ordering: tag("jude.uml.objectnode.ordering")
                    .and_then(|o| ObjectOrdering::parse(&o)),
                upper_bound: tag("jude.uml.objectnode.upperbound").unwrap_or_default(),
                is_stream: tag("jude.uml.objectnode.stream").as_deref() == Some("stream"),
                is_control: tag("jude.uml.objectnode.control").as_deref() == Some("true"),
                effect: tag("jude.uml.objectnode.effect").and_then(|e| ObjectEffect::parse(&e)),
                is_exception: tag("jude.uml.objectnode.exception").as_deref() == Some("true"),
                behavior: None,
            };
            let mut data =
                self.data(vertex, ElementKind::ActivityNode(props), ACTIVITY_NODE_STEREOTYPES);
            // Astah names control nodes by itself; UmlCraft neither shows nor needs those names.
            if !node.is_named() && is_default_node_name(&data.name) {
                data.name.clear();
            }
            let Some(id) = self.create(owner, data, Some(vertex)) else { continue };
            // The activity a call behavior action calls may come later.
            if let Some(called) = r.obj(vertex, "submachine") {
                self.references.push((id, called));
            }
        }
        for transition in r.list(graph, "transition") {
            let end = |field| r.obj(transition, field).and_then(|v| self.ids.get(&v).copied());
            let (Some(source), Some(target)) = (end("source"), end("target")) else {
                self.skip(transition);
                continue;
            };
            let guard = r.obj(transition, "guard").map(|g| r.name(g)).unwrap_or_default();
            let mut data = self.data(transition, ElementKind::flow(source, target), &[]);
            // Astah names a transition after its whole label, "trigger [guard] / effect".
            data.name = data.name.replace(&format!("[{guard}]"), "").trim().to_owned();
            if let ElementKind::ActivityEdge(p) = &mut data.kind {
                p.guard = guard;
            }
            self.create(activity, data, Some(transition));
        }
        if let Some(diagram) = diagram {
            self.diagrams.push((activity, diagram));
        }
    }

    /// Creates a state machine with its states and transitions, and queues its diagram. All
    /// of Astah's states are composite states; one with substates becomes a composite state in
    /// UmlCraft as well.
    fn state_machine(&mut self, owner: Id, machine: ObjId) {
        let r = self.r;
        let data = self.data(machine, ElementKind::StateMachine, &[]);
        let Some(id) = self.create(owner, data, Some(machine)) else { return };
        if let Some(top) = r.obj(machine, "top") {
            self.vertices(id, top);
        }
        for transition in r.list(machine, "transition") {
            let end = |field| r.obj(transition, field).and_then(|v| self.ids.get(&v).copied());
            let (Some(source), Some(target)) = (end("source"), end("target")) else {
                self.skip(transition);
                continue;
            };
            let named = |field| r.obj(transition, field).map(|o| r.name(o)).unwrap_or_default();
            let props = TransitionProps {
                source,
                target,
                trigger: named("trigger"),
                guard: named("guard"),
                effect: named("effect"),
            };
            let mut data = self.data(transition, ElementKind::Transition(props.clone()), &[]);
            // Astah names a transition after its whole label.
            let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
            if words(&data.name) == words(&props.label()) {
                data.name.clear();
            }
            self.create(id, data, Some(transition));
        }
        if let Some(diagram) = r.obj(machine, "diagram") {
            self.diagrams.push((id, diagram));
        }
    }

    /// The substates of `composite`, owned by `owner`, each with its own substates.
    fn vertices(&mut self, owner: Id, composite: ObjId) {
        let r = self.r;
        for vertex in r.list(composite, "subvertex") {
            // The substates of an orthogonal state are its regions, composite states of their
            // own; Astah's API names them Region0, Region1 … Its entry and exit points are
            // pseudostates beside them.
            if r.flag(composite, "isOrthogonal") && r.class(vertex) == "UCompositeStateImp" {
                let mut data = self.data(vertex, ElementKind::Region, &[]);
                if is_default_region_name(&data.name) {
                    data.name.clear();
                }
                if let Some(region) = self.create(owner, data, Some(vertex)) {
                    self.vertices(region, vertex);
                }
                continue;
            }
            let action = |field| r.obj(vertex, field).map(|a| r.name(a)).unwrap_or_default();
            let kind = match r.class(vertex) {
                // A submachine state names its state machine (`submachine`), set once all
                // machines exist.
                "UCompositeStateImp" | "UStateImp" | "USimpleStateImp" | "USubmachineStateImp" => {
                    ElementKind::State(StateProps {
                        entry: action("entry"),
                        do_activity: action("doActivity"),
                        exit: action("exit"),
                        submachine: None,
                    })
                }
                "UFinalStateImp" => ElementKind::FinalState,
                // Astah's stub states keep the name of what they stand for (`referenceState`)
                // empty; their own name is what they show.
                "UStubStateImp" => ElementKind::pseudostate(PseudostateKind::StubState),
                "UPseudostateImp" => {
                    let pseudostate = match r.text(vertex, "kind").as_str() {
                        "initial" => PseudostateKind::Initial,
                        "choice" => PseudostateKind::Choice,
                        "junction" => PseudostateKind::Junction,
                        "fork" => PseudostateKind::Fork,
                        "join" => PseudostateKind::Join,
                        "shallowHistory" => PseudostateKind::ShallowHistory,
                        "deepHistory" => PseudostateKind::DeepHistory,
                        "entryPoint" => PseudostateKind::EntryPoint,
                        "exitPoint" => PseudostateKind::ExitPoint,
                        _ => {
                            self.skip(vertex);
                            continue;
                        }
                    };
                    ElementKind::pseudostate(pseudostate)
                }
                _ => {
                    self.skip(vertex);
                    continue;
                }
            };
            let mut data = self.data(vertex, kind, &[]);
            if !matches!(data.kind, ElementKind::State(_)) && is_default_node_name(&data.name) {
                data.name.clear();
            }
            let Some(id) = self.create(owner, data, Some(vertex)) else { continue };
            // The state machine a submachine state stands for may come later.
            if let Some(machine) = r.obj(vertex, "submachine") {
                self.references.push((id, machine));
            }
            for transition in r.list(vertex, "internalTransitions") {
                let named = |field| r.obj(transition, field).map(|o| r.name(o)).unwrap_or_default();
                let props = InternalTransitionProps {
                    trigger: named("trigger"),
                    guard: named("guard"),
                    effect: named("effect"),
                };
                let mut data =
                    self.data(transition, ElementKind::InternalTransition(props.clone()), &[]);
                // Astah names it after its label, which UmlCraft shows instead of a name.
                data.name.clear();
                self.create(id, data, Some(transition));
            }
            self.vertices(id, vertex);
        }
    }

    fn partition(&mut self, owner: Id, partition: ObjId) {
        // A damaged file may list a partition among its own lanes.
        if !self.dimensions.insert(partition) {
            return;
        }
        // Astah puts every set of lanes into a "dimension" of its own, drawn as a header above
        // them. Unless the user named it, it is left out and its lanes take its place.
        let is_dimension =
            self.model.get(owner).is_some_and(|e| matches!(e.kind(), ElementKind::Activity))
                && is_default_dimension_name(&self.r.name(partition));
        if is_dimension {
            for sub in self.r.list(partition, "subGroup") {
                self.partition(owner, sub);
            }
            return;
        }
        let data = self.data(partition, ElementKind::Partition, &[]);
        let Some(id) = self.create(owner, data, Some(partition)) else { return };
        for sub in self.r.list(partition, "subGroup") {
            self.partition(id, sub);
        }
    }

    fn activity_node_kind(&self, vertex: ObjId) -> Option<ActivityNodeKind> {
        let r = self.r;
        let tagged =
            |key: &str| r.tagged_values(vertex).iter().any(|(k, v)| k == key && v == "true");
        Some(match r.class(vertex) {
            // Astah marks the kinds of actions by stereotypes of its own.
            "UActionStateImp" => {
                let stereotypes = r.stereotypes(vertex);
                let has = |s: &str| stereotypes.iter().any(|t| t == s);
                if has(SEND_SIGNAL) {
                    ActivityNodeKind::SendSignal
                } else if has(ACCEPT_EVENT) {
                    ActivityNodeKind::AcceptEvent
                } else if has(ACCEPT_TIME_EVENT) {
                    ActivityNodeKind::AcceptTimeEvent
                } else if has(CONNECTOR) {
                    ActivityNodeKind::Connector
                } else {
                    ActivityNodeKind::Action
                }
            }
            // A subactivity calls another activity.
            "USubactivityStateImp" => ActivityNodeKind::CallBehavior,
            "UObjectFlowStateImp" if tagged("jude.uml.inputpin") => ActivityNodeKind::InputPin,
            "UObjectFlowStateImp" if tagged("jude.uml.outputpin") => ActivityNodeKind::OutputPin,
            "UObjectFlowStateImp" => ActivityNodeKind::Object,
            "UActivityParameterNodeImp" => ActivityNodeKind::ActivityParameter,
            "UFinalStateImp" => ActivityNodeKind::ActivityFinal,
            "UFlowFinalNodeImp" => ActivityNodeKind::FlowFinal,
            "UPseudostateImp" => match self.r.text(vertex, "kind").as_str() {
                "initial" => ActivityNodeKind::Initial,
                "fork" => ActivityNodeKind::Fork,
                "join" => ActivityNodeKind::Join,
                // Astah draws one "decision and merge" node; its flows tell which it is.
                "choice" | "junction" => {
                    let count = |field| self.r.list(vertex, field).len();
                    if count("incoming") > 1 && count("outgoing") <= 1 {
                        ActivityNodeKind::Merge
                    } else {
                        ActivityNodeKind::Decision
                    }
                }
                _ => return None,
            },
            _ => return None,
        })
    }

    fn diagram(&mut self, owner: Id, object: ObjId) {
        let kind = match (self.r.class(object), self.r.text(object, "type").as_str()) {
            ("USequenceDiagramImp", _) => DiagramKind::Sequence,
            ("UActivityDiagramImp", _) => DiagramKind::Activity,
            ("UStateChartDiagramImp", _) => DiagramKind::StateMachine,
            (_, "Class Diagram") => DiagramKind::Class,
            (_, "UseCase Diagram") => DiagramKind::UseCase,
            (_, "Component Diagram") => DiagramKind::Component,
            (_, "Deployment Diagram") => DiagramKind::Deployment,
            (_, "CompositeStructure Diagram") => DiagramKind::CompositeStructure,
            _ => {
                self.skip(object);
                return;
            }
        };
        let mut owner = owner;
        if kind == DiagramKind::Sequence {
            match self.interaction(owner, object) {
                Some(interaction) => owner = interaction,
                None => {
                    self.skip(object);
                    return;
                }
            }
        }
        let mut data = self.data(object, ElementKind::diagram(kind), &[]);
        if let ElementKind::Diagram(props) = &mut data.kind {
            let off = |field| self.r.g.bool_field(object, field) == Some(false);
            props.hide_message_numbers = kind == DiagramKind::Sequence && off("indexVisible");
            props.hide_return_values = kind == DiagramKind::Sequence && off("ReturnValueVisible");
            props.flat_message_numbers = kind == DiagramKind::Sequence
                && self.r.g.bool_field(object, "flatIndex") == Some(true);
            props.hide_frame = kind == DiagramKind::Sequence
                && self.r.list(object, "presentation").into_iter().any(|p| {
                    self.r.class(p) == "FramePresentation"
                        && self.r.g.bool_field(p, "visibility") == Some(false)
                });
        }
        let Some(diagram) = self.create(owner, data, Some(object)) else { return };
        self.presentations(diagram, kind, object);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_profile_tags_are_kept() {
        for tag in ["jude.java.transient", "jude.c_sharp.override", "jude.c_plus.virtual"] {
            assert!(!is_internal_tag(tag), "{tag}");
        }
        for tag in ["jude.type", "jude.type_modifier", "jude.hyperlink", "hiddenDependency"] {
            assert!(is_internal_tag(tag), "{tag}");
        }
        assert_eq!(neutral_tag("jude.java.annotations"), "java.annotations");
        assert_eq!(neutral_tag("jude.c_sharp.override"), "csharp.override");
        assert_eq!(neutral_tag("jude.c_plus.virtual"), "cpp.virtual");
        assert_eq!(neutral_tag("author"), "author");
    }

    #[test]
    fn language_markers() {
        for marker in ["Java Class", "Java Attribute", "Java Method", "C# Class", "C++ Method"] {
            assert!(is_language_marker(marker), "{marker}");
        }
        for stereotype in ["abstract", "Java", "Java class", "Java Class Thing", "entity"] {
            assert!(!is_language_marker(stereotype), "{stereotype}");
        }
    }

    use crate::jser::{ClassData, ClassDesc, Object};

    /// Builds small Astah object graphs by hand.
    #[derive(Default)]
    struct Builder(Graph);

    impl Builder {
        fn class(&mut self, name: &str) -> crate::jser::ClassId {
            if let Some(i) = self.0.classes.iter().position(|c| *c.name == *name) {
                return crate::jser::ClassId(i);
            }
            self.0.classes.push(ClassDesc::new(name.to_owned(), 0, 0));
            crate::jser::ClassId(self.0.classes.len() - 1)
        }

        fn push(&mut self, object: Object) -> ObjId {
            self.0.objects.push(object);
            ObjId(self.0.objects.len() - 1)
        }

        fn string(&mut self, s: &str) -> Value {
            Value::Object(self.push(Object::String(s.to_owned())))
        }

        fn list(&mut self, items: &[ObjId]) -> Value {
            let class = self.class("[Ljava.lang.Object;");
            let items = items.iter().map(|&i| Value::Object(i)).collect();
            Value::Object(self.push(Object::Array { class, items }))
        }

        /// An instance of `class` without fields, to be filled by `set`.
        fn instance(&mut self, class: &str) -> ObjId {
            let class_id = self.class(class);
            let data = vec![ClassData { class: class.into(), ..Default::default() }];
            self.push(Object::Instance { class: class_id, data })
        }

        fn set(&mut self, id: ObjId, field: &str, value: Value) {
            let Object::Instance { data, .. } = &mut self.0.objects[id.0] else { panic!() };
            data[0].fields.push((field.into(), value));
        }

        /// Makes `owner` own `child` through an ownership object, as Astah does.
        fn own(&mut self, owner: ObjId, children: &[ObjId]) {
            let ownerships: Vec<ObjId> = children
                .iter()
                .map(|&child| {
                    let ownership = self.instance("JomtOwnership");
                    self.set(ownership, "ownedElement", Value::Object(child));
                    ownership
                })
                .collect();
            let list = self.list(&ownerships);
            self.set(owner, "ownedElementOwnership", list);
        }
    }

    /// An instance of `class` named `name`.
    fn named(b: &mut Builder, class: &str, name: &str) -> ObjId {
        let id = b.instance(class);
        let name = b.string(name);
        b.set(id, "name", name);
        id
    }

    /// The kinds of the root's children, by name.
    fn kinds(model: &Model) -> BTreeMap<String, &'static str> {
        model
            .children(model.root())
            .iter()
            .filter_map(|&c| model.get(c))
            .map(|e| (e.name().to_owned(), e.kind().type_name()))
            .collect()
    }

    #[test]
    fn classifiers_map_by_their_astah_class() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let children = [
            named(&mut b, "UClassifierImp", "Order"),
            named(&mut b, "UInterfaceImp", "Shape"),
            named(&mut b, "UActorImp", "User"),
            named(&mut b, "UUseCaseImp", "Buy"),
            named(&mut b, "UComponentImp", "Shop"),
            named(&mut b, "UArtifactImp", "shop.jar"),
            named(&mut b, "UNodeImp", "Server"),
            named(&mut b, "UPackageImp", "orders"),
        ];
        b.own(model, &children);
        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        let kinds = kinds(m);
        let expected = [
            ("Order", ElementKind::Class(ClassifierProps::default())),
            ("Shape", ElementKind::Interface(ClassifierProps::default())),
            ("User", ElementKind::Actor(ClassifierProps::default())),
            ("Buy", ElementKind::UseCase(ClassifierProps::default())),
            ("Shop", ElementKind::Component(ClassifierProps::default())),
            ("shop.jar", ElementKind::Artifact(ClassifierProps::default())),
            ("Server", ElementKind::Node(ClassifierProps::default())),
            ("orders", ElementKind::Package),
        ];
        for (name, kind) in expected {
            assert_eq!(kinds.get(name).copied(), Some(kind.type_name()), "{name}");
        }
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    }

    #[test]
    fn features_keep_their_order_scope_and_parameters() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let class = named(&mut b, "UClassifierImp", "Order");
        b.own(model, &[class]);
        let count = named(&mut b, "UAttributeImp", "count");
        let scope = b.string("classifier");
        b.set(count, "ownerScope", scope);
        let total = named(&mut b, "UAttributeImp", "total");
        let ship = named(&mut b, "UOperationImp", "ship");
        let days = named(&mut b, "UParameterImp", "days");
        let kind = b.string("in");
        b.set(days, "kind", kind);
        let parameters = b.list(&[days]);
        b.set(ship, "parameter", parameters);
        // Astah's display order, not the order of ownership.
        let structural = b.list(&[total, count]);
        b.set(class, "structuralFeature", structural);
        let behavioral = b.list(&[ship]);
        b.set(class, "behavioralFeature", behavioral);
        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        let order = m.children(m.root())[0];
        let features: Vec<(&str, &str)> = m
            .children(order)
            .iter()
            .filter_map(|&c| m.get(c))
            .map(|e| (e.name(), e.kind().type_name()))
            .collect();
        assert_eq!(
            features,
            [("total", "attribute"), ("count", "attribute"), ("ship", "operation")]
        );
        let count = m.children(order)[1];
        assert!(matches!(m.get(count).unwrap().kind(), ElementKind::Attribute(p) if p.is_static));
        let ship = m.children(order)[2];
        let parameters: Vec<&str> = m
            .children(ship)
            .iter()
            .filter_map(|&c| m.get(c))
            .filter(|e| matches!(e.kind(), ElementKind::Parameter(_)))
            .map(|e| e.name())
            .collect();
        assert_eq!(parameters, ["days"]);
    }

    #[test]
    fn relationships_connect_what_astah_names() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let (order, item) =
            (named(&mut b, "UClassifierImp", "Order"), named(&mut b, "UClassifierImp", "Item"));
        let (buy, pay) = (named(&mut b, "UUseCaseImp", "Buy"), named(&mut b, "UUseCaseImp", "Pay"));
        let generalization = b.instance("UGeneralizationImp");
        b.set(generalization, "subtype", Value::Object(order));
        b.set(generalization, "supertype", Value::Object(item));
        let dependency = b.instance("UDependencyImp");
        let (client, supplier) = (b.list(&[order]), b.list(&[item]));
        b.set(dependency, "client", client);
        b.set(dependency, "supplier", supplier);
        let ends: Vec<ObjId> = [
            (order, "buyer", "navigableType", "navigable"),
            (item, "items", "aggregation", "composite"),
        ]
        .into_iter()
        .map(|(ty, role, field, value)| {
            let end = named(&mut b, "UAssociationEndImp", role);
            b.set(end, "type", Value::Object(ty));
            let value = b.string(value);
            b.set(end, field, value);
            end
        })
        .collect();
        let association = b.instance("UAssociationImp");
        let connection = b.list(&ends);
        b.set(association, "connection", connection);
        // Astah's fields are swapped against UML: `addition` is the including use case.
        let include = b.instance("UIncludeImp");
        b.set(include, "addition", Value::Object(buy));
        b.set(include, "base", Value::Object(pay));
        b.own(model, &[order, item, buy, pay, generalization, dependency, association, include]);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        let id = |name: &str| {
            m.children(m.root())
                .iter()
                .copied()
                .find(|&c| m.get(c).unwrap().name() == name)
                .unwrap()
        };
        let (order, item, buy, pay) = (id("Order"), id("Item"), id("Buy"), id("Pay"));
        let kinds: Vec<&ElementKind> = m.elements().map(|e| e.kind()).collect();
        assert!(kinds.iter().any(|k| matches!(k, ElementKind::Generalization(p) if p.specific == order && p.general == item)));
        assert!(kinds.iter().any(
            |k| matches!(k, ElementKind::Dependency(p) if p.client == order && p.supplier == item)
        ));
        assert!(kinds.iter().any(
            |k| matches!(k, ElementKind::Include(p) if p.including == buy && p.addition == pay)
        ));
        let ends = kinds.iter().find_map(|k| match k {
            ElementKind::Association(p) => Some(&p.ends),
            _ => None,
        });
        let ends = ends.expect("an association");
        assert_eq!((ends[0].participant, ends[0].role.as_str()), (order, "buyer"));
        assert_eq!(ends[0].navigability, Navigability::Navigable);
        assert_eq!((ends[1].participant, ends[1].aggregation), (item, Aggregation::Composite));
    }

    #[test]
    fn state_machines_bring_their_states_and_transitions() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let order = named(&mut b, "UClassifierImp", "Order");
        b.own(model, &[order]);
        let machine = named(&mut b, "UStateMachineImp", "Life");
        let behaviors = b.list(&[machine]);
        b.set(order, "behavior", behaviors);
        let start = named(&mut b, "UPseudostateImp", "InitialPseudostate0");
        let kind = b.string("initial");
        b.set(start, "kind", kind);
        let open = named(&mut b, "UStateImp", "Open");
        let entry = named(&mut b, "UActionImp", "count");
        b.set(open, "entry", Value::Object(entry));
        let done = named(&mut b, "UFinalStateImp", "FinalState0");
        let top = b.instance("UCompositeStateImp");
        let vertices = b.list(&[start, open, done]);
        b.set(top, "subvertex", vertices);
        b.set(machine, "top", Value::Object(top));
        let transition = |b: &mut Builder, from: ObjId, to: ObjId, trigger: Option<&str>| {
            let t = b.instance("UTransitionImp");
            b.set(t, "source", Value::Object(from));
            b.set(t, "target", Value::Object(to));
            if let Some(trigger) = trigger {
                let event = named(b, "UEventImp", trigger);
                b.set(t, "trigger", Value::Object(event));
            }
            t
        };
        let first = transition(&mut b, start, open, None);
        let close = transition(&mut b, open, done, Some("close"));
        let transitions = b.list(&[first, close]);
        b.set(machine, "transition", transitions);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        let order = m.children(m.root())[0];
        let life = m.children(order).iter().copied().find(|&c| m.get(c).unwrap().name() == "Life");
        let life = life.expect("the state machine under its class");
        let children: Vec<&ElementKind> =
            m.children(life).iter().map(|&c| m.get(c).unwrap().kind()).collect();
        let pseudostates = children.iter().filter(|k| matches!(k, ElementKind::Pseudostate(_)));
        assert_eq!(pseudostates.count(), 1);
        assert!(children.iter().any(|k| matches!(k, ElementKind::FinalState)));
        let state = children.iter().find_map(|k| match k {
            ElementKind::State(p) => Some(p),
            _ => None,
        });
        assert_eq!(state.map(|p| p.entry.as_str()), Some("count"));
        let triggers: Vec<&str> = children
            .iter()
            .filter_map(|k| match k {
                ElementKind::Transition(p) => Some(p.trigger.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(triggers, ["", "close"]);
        // Astah's default names of pseudostates and final states are not kept.
        let names: Vec<&str> = m.children(life).iter().map(|&c| m.get(c).unwrap().name()).collect();
        assert!(
            !names.contains(&"InitialPseudostate0") && !names.contains(&"FinalState0"),
            "{names:?}"
        );
    }

    #[test]
    fn activities_bring_their_nodes_and_flows() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let order = named(&mut b, "UClassifierImp", "Order");
        b.own(model, &[order]);
        let graph = named(&mut b, "UActivityGraphImp", "Ship");
        let behaviors = b.list(&[graph]);
        b.set(order, "behavior", behaviors);
        let pseudostate = |b: &mut Builder, name: &str, kind: &str| {
            let node = named(b, "UPseudostateImp", name);
            let kind = b.string(kind);
            b.set(node, "kind", kind);
            node
        };
        let start = pseudostate(&mut b, "InitialNode0", "initial");
        let pack = named(&mut b, "UActionStateImp", "Pack");
        let decide = pseudostate(&mut b, "DecisionNode0", "choice");
        let done = named(&mut b, "UFinalStateImp", "ActivityFinal0");
        let top = b.instance("UCompositeStateImp");
        let vertices = b.list(&[start, pack, decide, done]);
        b.set(top, "subvertex", vertices);
        b.set(graph, "top", Value::Object(top));
        let flow = |b: &mut Builder, from: ObjId, to: ObjId| {
            let t = b.instance("UTransitionImp");
            b.set(t, "source", Value::Object(from));
            b.set(t, "target", Value::Object(to));
            t
        };
        let flows =
            [flow(&mut b, start, pack), flow(&mut b, pack, decide), flow(&mut b, decide, done)];
        // Astah names a guarded flow after its label.
        let guard = named(&mut b, "UGuardImp", "paid");
        b.set(flows[2], "guard", Value::Object(guard));
        let label = b.string("[paid]");
        b.set(flows[2], "name", label);
        let outgoing = b.list(&flows[2..]);
        b.set(decide, "outgoing", outgoing);
        let transitions = b.list(&flows);
        b.set(graph, "transition", transitions);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        let order = m.children(m.root())[0];
        let ship = m.children(order).iter().copied().find(|&c| m.get(c).unwrap().name() == "Ship");
        let ship = ship.expect("the activity under its class");
        let nodes: Vec<(ActivityNodeKind, &str)> = m
            .children(ship)
            .iter()
            .filter_map(|&c| m.get(c))
            .filter_map(|e| match e.kind() {
                ElementKind::ActivityNode(p) => Some((p.node, e.name())),
                _ => None,
            })
            .collect();
        assert_eq!(
            nodes,
            [
                (ActivityNodeKind::Initial, ""),
                (ActivityNodeKind::Action, "Pack"),
                (ActivityNodeKind::Decision, ""),
                (ActivityNodeKind::ActivityFinal, ""),
            ]
        );
        let flows: Vec<(&str, &str)> = m
            .children(ship)
            .iter()
            .filter_map(|&c| m.get(c))
            .filter_map(|e| match e.kind() {
                ElementKind::ActivityEdge(p) => Some((e.name(), p.guard.as_str())),
                _ => None,
            })
            .collect();
        assert_eq!(flows, [("", ""), ("", ""), ("", "paid")]);
    }

    #[test]
    fn class_diagrams_show_their_classes_where_astah_put_them() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let (order, item) =
            (named(&mut b, "UClassifierImp", "Order"), named(&mut b, "UClassifierImp", "Item"));
        let diagram = named(&mut b, "UDiagramImp", "Overview");
        let kind = b.string("Class Diagram");
        b.set(diagram, "type", kind);
        b.own(model, &[order, item, diagram]);
        let node = |b: &mut Builder, element: ObjId, x: f64, y: f64| {
            let p = b.instance("ClassifierPresentation");
            b.set(p, "model", Value::Object(element));
            let at = b.instance("Pnt2d");
            b.set(at, "x", Value::Double(x));
            b.set(at, "y", Value::Double(y));
            b.set(p, "location", Value::Object(at));
            b.set(p, "width", Value::Double(100.0));
            b.set(p, "height", Value::Double(60.0));
            p
        };
        // Astah's coordinates may start anywhere; UmlCraft's leave a margin of 20.
        let shown = [node(&mut b, order, -30.0, 100.0), node(&mut b, item, 200.0, 40.0)];
        let presentations = b.list(&shown);
        b.set(diagram, "presentation", presentations);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        let id = |name: &str| {
            let found =
                m.children(m.root()).iter().copied().find(|&c| m.get(c).unwrap().name() == name);
            found.unwrap()
        };
        let (order, item, diagram) = (id("Order"), id("Item"), id("Overview"));
        assert_eq!(m.get(diagram).unwrap().kind().as_diagram(), Some(DiagramKind::Class));
        let bounds = |element: Id| {
            let view = m.view(diagram).unwrap();
            let p = view.presentations_of(element).next().expect("shown");
            p.bounds().unwrap()
        };
        assert_eq!(bounds(order), Rect::new(20.0, 100.0, 100.0, 60.0));
        assert_eq!(bounds(item), Rect::new(250.0, 40.0, 100.0, 60.0));
    }

    /// A point object as Astah stores locations and waypoints.
    fn point(b: &mut Builder, x: f64, y: f64) -> ObjId {
        let at = b.instance("Pnt2d");
        b.set(at, "x", Value::Double(x));
        b.set(at, "y", Value::Double(y));
        at
    }

    #[test]
    fn edges_run_the_way_their_relationship_does() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let (order, item) =
            (named(&mut b, "UClassifierImp", "Order"), named(&mut b, "UClassifierImp", "Item"));
        let generalization = b.instance("UGeneralizationImp");
        b.set(generalization, "subtype", Value::Object(order));
        b.set(generalization, "supertype", Value::Object(item));
        let diagram = named(&mut b, "UDiagramImp", "Overview");
        let kind = b.string("Class Diagram");
        b.set(diagram, "type", kind);
        b.own(model, &[order, item, generalization, diagram]);
        let node = |b: &mut Builder, element: ObjId, x: f64| {
            let p = b.instance("ClassifierPresentation");
            b.set(p, "model", Value::Object(element));
            let at = point(b, x, 40.0);
            b.set(p, "location", Value::Object(at));
            b.set(p, "width", Value::Double(100.0));
            b.set(p, "height", Value::Double(60.0));
            p
        };
        let (order_p, item_p) = (node(&mut b, order, 20.0), node(&mut b, item, 300.0));
        // Astah's servers run from the general class to the specific one here, and the
        // waypoints with them.
        let edge = b.instance("GeneralizationPresentation");
        b.set(edge, "model", Value::Object(generalization));
        let servers = b.list(&[item_p, order_p]);
        b.set(edge, "servers", servers);
        let waypoints = [point(&mut b, 250.0, 200.0), point(&mut b, 150.0, 200.0)];
        let all = b.list(&waypoints);
        b.set(edge, "allPoints", all.clone());
        b.set(edge, "points", all);
        let presentations = b.list(&[order_p, item_p, edge]);
        b.set(diagram, "presentation", presentations);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        let id = |name: &str| {
            let children = m.children(m.root()).iter().copied();
            children.into_iter().find(|&c| m.get(c).unwrap().name() == name).unwrap()
        };
        let view = m.view(id("Overview")).unwrap();
        let shown = |element: Id| view.presentations_of(element).next().expect("shown").id;
        let generalization =
            m.elements().find(|e| matches!(e.kind(), ElementKind::Generalization(_))).unwrap().id();
        let edge = view.presentations_of(generalization).next().expect("an edge");
        let Shape::Edge { source, target, waypoints, .. } = &edge.shape else { panic!() };
        assert_eq!((*source, *target), (shown(id("Order")), shown(id("Item"))));
        assert_eq!(*waypoints, [Point::new(150.0, 200.0), Point::new(250.0, 200.0)]);
    }

    #[test]
    fn constraint_labels_of_a_line_keep_their_offsets() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let (order, item) =
            (named(&mut b, "UClassifierImp", "Order"), named(&mut b, "UClassifierImp", "Item"));
        let generalization = b.instance("UGeneralizationImp");
        b.set(generalization, "subtype", Value::Object(order));
        b.set(generalization, "supertype", Value::Object(item));
        let constraints: Vec<ObjId> = ["first", "second", "third"]
            .iter()
            .map(|c| named(&mut b, "UConstraintImp", c))
            .collect();
        let list = b.list(&constraints);
        b.set(generalization, "constraint", list);
        let diagram = named(&mut b, "UDiagramImp", "Overview");
        let kind = b.string("Class Diagram");
        b.set(diagram, "type", kind);
        b.own(model, &[order, item, generalization, diagram]);
        let node = |b: &mut Builder, element: ObjId, x: f64| {
            let p = b.instance("ClassifierPresentation");
            b.set(p, "model", Value::Object(element));
            let at = point(b, x, 40.0);
            b.set(p, "location", Value::Object(at));
            b.set(p, "width", Value::Double(100.0));
            b.set(p, "height", Value::Double(60.0));
            p
        };
        let (order_p, item_p) = (node(&mut b, order, 20.0), node(&mut b, item, 300.0));
        let edge = b.instance("GeneralizationPresentation");
        b.set(edge, "model", Value::Object(generalization));
        let servers = b.list(&[item_p, order_p]);
        b.set(edge, "servers", servers);
        let ends = [point(&mut b, 350.0, 100.0), point(&mut b, 70.0, 100.0)];
        let all = b.list(&ends);
        b.set(edge, "allPoints", all.clone());
        b.set(edge, "points", all);
        // Astah keeps a label per constraint with its offset by hand, and makes the third one
        // only as it draws it, with the first one's offset.
        let label = |b: &mut Builder, x: f64, y: f64| {
            let label = b.instance("LabelPresentation");
            let at = point(b, 0.0, 0.0);
            b.set(label, "location", Value::Object(at));
            let moved = point(b, x, y);
            b.set(label, "localMovement", Value::Object(moved));
            label
        };
        let labels = [label(&mut b, 5.0, 6.0), label(&mut b, 20.0, -30.0)];
        let labels = b.list(&labels);
        b.set(edge, "constraintPresentations", labels);
        let presentations = b.list(&[order_p, item_p, edge]);
        b.set(diagram, "presentation", presentations);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        let generalization =
            m.elements().find(|e| matches!(e.kind(), ElementKind::Generalization(_))).unwrap();
        assert_eq!(generalization.data().constraints, ["first", "second", "third"]);
        let diagram = m.elements().find(|e| e.name() == "Overview").unwrap().id();
        let view = m.view(diagram).unwrap();
        let edge = view.presentations_of(generalization.id()).next().expect("an edge");
        let (first, second) = (Point::new(5.0, 6.0), Point::new(20.0, -30.0));
        assert_eq!(edge.style.constraint_offsets, [Some(first), Some(second), Some(first)]);
    }

    #[test]
    fn sequence_diagrams_bring_lifelines_and_messages_in_drawing_order() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let order = named(&mut b, "UClassifierImp", "Order");
        let diagram = named(&mut b, "USequenceDiagramImp", "Place order");
        b.own(model, &[order, diagram]);
        let collaboration = b.instance("UCollaborationImp");
        b.set(diagram, "collaboration", Value::Object(collaboration));
        let (shop, store) = (
            named(&mut b, "UClassifierRoleImp", "shop"),
            named(&mut b, "UClassifierRoleImp", "store"),
        );
        b.set(store, "base", Value::Object(order));
        b.own(collaboration, &[shop, store]);
        // A message at height `y`; `action_type` 2 makes it a reply.
        let message = |b: &mut Builder, name: &str, from, to, y: f64, action_type: Option<i64>| {
            let m = named(b, "UMessageImp", name);
            b.set(m, "sender", Value::Object(from));
            b.set(m, "receiver", Value::Object(to));
            let p = b.instance("MessagePresentation");
            let at = point(b, 0.0, y);
            let points = b.list(&[at]);
            b.set(p, "allPoints", points);
            let presentations = b.list(&[p]);
            b.set(m, "presentation", presentations);
            if let Some(t) = action_type {
                let action = b.instance("UActionImp");
                b.set(action, "actionType", Value::Long(t));
                b.set(m, "action", Value::Object(action));
            }
            m
        };
        let call = message(&mut b, "check", shop, store, 100.0, None);
        let answer = b.string("ok");
        b.set(call, "returnValue", answer);
        let reply = message(&mut b, "", store, shop, 200.0, Some(REPLY));
        b.set(reply, "activator", Value::Object(call));
        let interaction = b.instance("UInteractionImp");
        // Listed bottom up: the drawing decides the order.
        let messages = b.list(&[reply, call]);
        b.set(interaction, "message", messages);
        let interactions = b.list(&[interaction]);
        b.set(collaboration, "interactions", interactions);

        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        m.check().unwrap();
        let interaction = m
            .elements()
            .find(|e| matches!(e.kind(), ElementKind::Interaction))
            .expect("an interaction");
        assert_eq!(interaction.name(), "Place order");
        let items: Vec<&umlcraft_model::Element> =
            interaction.children().iter().filter_map(|&c| m.get(c)).collect();
        let lifelines: Vec<(&str, Option<Id>)> = items
            .iter()
            .filter_map(|e| match e.kind() {
                ElementKind::Lifeline(p) => Some((e.name(), p.represents)),
                _ => None,
            })
            .collect();
        let order = m.children(m.root())[0];
        assert_eq!(lifelines, [("shop", None), ("store", Some(order))]);
        let messages: Vec<(MessageSort, &str)> = items
            .iter()
            .filter_map(|e| match e.kind() {
                ElementKind::Message(p) => Some((p.sort, p.return_value.as_str())),
                _ => None,
            })
            .collect();
        // Astah keeps the return value on the call; UmlCraft shows it on the reply.
        assert_eq!(messages, [(MessageSort::Synchronous, ""), (MessageSort::Reply, "ok")]);
    }

    #[test]
    fn sequence_diagrams_bring_state_invariants_terminations_and_constraints() {
        let mut b = Builder::default();
        let model = named(&mut b, "UModelImp", "Model");
        let diagram = named(&mut b, "USequenceDiagramImp", "Timing");
        b.own(model, &[diagram]);
        let collaboration = b.instance("UCollaborationImp");
        b.set(diagram, "collaboration", Value::Object(collaboration));
        let (shop, store) = (
            named(&mut b, "UClassifierRoleImp", "shop"),
            named(&mut b, "UClassifierRoleImp", "store"),
        );
        let message = |b: &mut Builder, name: &str, from, to, action_type: Option<i64>| {
            let m = named(b, "UMessageImp", name);
            b.set(m, "sender", Value::Object(from));
            b.set(m, "receiver", Value::Object(to));
            if let Some(t) = action_type {
                let action = b.instance("UActionImp");
                b.set(action, "actionType", Value::Long(t));
                b.set(m, "action", Value::Object(action));
            }
            m
        };
        let check = message(&mut b, "check", shop, store, None);
        let save = message(&mut b, "save", shop, store, None);
        // A termination is a destroy message from the lifeline to itself, drawn only as a cross.
        let stop = message(&mut b, "Stop0", store, store, Some(DESTROY));
        let invariant = named(&mut b, "UStateInvariantImp", "x > 0");
        let covered = b.list(&[store]);
        b.set(invariant, "covereds", covered);
        let constraint = |b: &mut Builder, class: &str, name: &str, on: &[ObjId]| {
            let c = named(b, class, name);
            let on = b.list(on);
            b.set(c, "constrainedElement", on);
            c
        };
        let duration = constraint(&mut b, "UDurationConstraintImp", "d..3*d", &[check, save]);
        let time = constraint(&mut b, "UTimeConstraintImp", "t..t+3", &[save]);
        b.own(collaboration, &[shop, store, duration, time]);
        let interaction = b.instance("UInteractionImp");
        let messages = b.list(&[check, save, stop]);
        b.set(interaction, "message", messages);
        let fragments = b.list(&[invariant]);
        b.set(interaction, "fragments", fragments);
        let interactions = b.list(&[interaction]);
        b.set(collaboration, "interactions", interactions);

        let imported = import(&b.0).unwrap();
        assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
        let m = &imported.model;
        m.check().unwrap();
        let by_name = |name: &str| m.elements().find(|e| e.name() == name).unwrap().id();
        let (store, check, save) = (by_name("store"), by_name("check"), by_name("save"));
        let messages = m.elements().filter(|e| matches!(e.kind(), ElementKind::Message(_))).count();
        assert_eq!(messages, 2, "the termination is no message");
        let kinds: Vec<ElementKind> = m
            .elements()
            .filter(|e| {
                matches!(
                    e.kind(),
                    ElementKind::StateInvariant(_)
                        | ElementKind::Termination(_)
                        | ElementKind::DurationConstraint(_)
                        | ElementKind::TimeConstraint(_)
                )
            })
            .map(|e| e.kind().clone())
            .collect();
        assert_eq!(kinds.len(), 4);
        assert!(
            kinds.contains(&ElementKind::StateInvariant(StateInvariantProps { lifeline: store }))
        );
        assert!(kinds.contains(&ElementKind::Termination(TerminationProps { lifeline: store })));
        assert!(kinds.contains(&ElementKind::DurationConstraint(DurationConstraintProps {
            from: check,
            to: save
        })));
        assert!(
            kinds.contains(&ElementKind::TimeConstraint(TimeConstraintProps { message: save }))
        );
        assert_eq!(m.get(by_name("x > 0")).map(|e| e.kind().type_name()), Some("stateInvariant"));
        assert_eq!(
            m.get(by_name("d..3*d")).map(|e| e.kind().type_name()),
            Some("durationConstraint")
        );
    }

    #[test]
    fn a_package_that_owns_itself_is_imported_once() {
        let mut b = Builder::default();
        let model = b.instance("UModelImp");
        let name = b.string("Model");
        b.set(model, "name", name);
        let package = b.instance("UPackageImp");
        let name = b.string("P");
        b.set(package, "name", name);
        b.own(model, &[package]);
        b.own(package, &[package]);
        let imported = import(&b.0).unwrap();
        let m = &imported.model;
        let packages: Vec<_> = m.children(m.root()).to_vec();
        assert_eq!(packages.len(), 1);
        assert!(m.children(packages[0]).is_empty());
    }

    #[test]
    fn a_name_that_is_its_own_body_gives_no_text() {
        let mut b = Builder::default();
        let model = b.instance("UModelImp");
        let name = b.instance("UNameImp");
        b.set(name, "body", Value::Object(name));
        b.set(model, "name", Value::Object(name));
        let imported = import(&b.0).unwrap();
        assert_eq!(imported.model.element(imported.model.root()).unwrap().name(), "");
    }
}
