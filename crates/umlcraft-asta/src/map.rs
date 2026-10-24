//! Maps Astah's object graph onto an UmlCraft model.
//!
//! Astah follows the UML 1.4 metamodel (`JP.co.esm.caddies.uml.*`) plus its own presentation
//! classes (`JP.co.esm.caddies.jomt.jmodel.*`). Only what UmlCraft can represent is imported:
//! packages, classifiers with their features, use cases, the relationships between them,
//! activities, and class, use case, sequence and activity diagrams. Everything else is counted
//! in [`Imported::skipped`].

use std::collections::{BTreeMap, HashMap, HashSet};

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd, AssociationProps,
    AttributeProps, ClassifierProps, CombinedFragmentProps, CommentProps, DiagramKind,
    DirectedProps, EdgeLabels, ElementData, ElementKind, ExtendProps, GeneralizationProps, Id,
    IncludeProps, InstanceKind, InstanceProps, InteractionOperandProps, InteractionOperator,
    InteractionUseProps, LifelineProps, MessageGate, MessageProps, MessageSort, Model,
    Multiplicity, Navigability, Op, OperationProps, ParameterDirection, ParameterProps, Point,
    Presentation, PseudostateKind, Rect, Shape, StateProps, Style, TaggedValue, TransitionProps,
    TypeRef, Visibility,
};
use umlcraft_notation::palette;
use umlcraft_notation::scene::border_point_from;

use crate::jser::{Graph, ObjId, Value};

/// The result of an import.
#[derive(Debug)]
pub struct Imported {
    pub model: Model,
    /// Astah classes of model elements and diagrams that were not imported, with how often
    /// they occurred. Presentations of skipped elements are left out silently.
    pub skipped: BTreeMap<String, usize>,
}

/// Astah's default fill color, not worth carrying over as an explicit style.
const DEFAULT_FILL: &str = "#FFFFCC";

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
    /// expressions and enumeration kinds are such objects).
    fn text(&self, id: ObjId, field: &str) -> String {
        let mut current = self.obj(id, field);
        while let Some(value) = current {
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

    /// The name of a type as Astah shows it. A bound template such as `List<Projekt>` is an
    /// anonymous classifier whose template binding names the template and the arguments.
    fn type_name(&self, ty: ObjId, depth: usize) -> String {
        let name = self.name(ty);
        let Some(&binding) = self.list(ty, "templateBinding").first() else { return name };
        if !name.is_empty() || depth > 8 {
            return name;
        }
        let Some(signature) = self.obj(binding, "signature") else { return name };
        let template = self.obj(signature, "template").map(|t| self.name(t)).unwrap_or_default();
        let formals = self.list(signature, "ownedParameter");
        let mut substitutions = self.list(binding, "parameterSubstition");
        let position = |s: &ObjId| {
            let formal = self.obj(*s, "formal");
            formals.iter().position(|f| Some(*f) == formal).unwrap_or(usize::MAX)
        };
        substitutions.sort_by_key(position);
        let arguments: Vec<String> = substitutions
            .iter()
            .filter_map(|s| self.list(*s, "actual").first().map(|a| self.type_name(*a, depth + 1)))
            .collect();
        format!("{template}<{}>", arguments.join(", "))
    }

    fn num(&self, id: ObjId, field: &str) -> Option<f64> {
        self.g.field(id, field).and_then(Value::as_f64)
    }

    fn flag(&self, id: ObjId, field: &str) -> bool {
        self.g.field(id, field).and_then(Value::as_bool).unwrap_or(false)
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
        let lower = match lower {
            -1 => 0,
            l if l >= 0 => u32::try_from(l).ok()?,
            _ => return None,
        };
        let m = Multiplicity { lower, upper };
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

/// Tagged values Astah uses for itself.
fn is_internal_tag(key: &str) -> bool {
    key.starts_with("jude.") || matches!(key, "hiddenDependency" | "matrix_access_type")
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
    features: Vec<(Id, ObjId)>,
    /// Features already in `features`.
    queued: HashSet<ObjId>,
    /// Calls that have a reply message.
    replied: HashSet<ObjId>,
    relationships: Vec<(Id, ObjId)>,
    /// Instance specifications, created once their classifiers exist.
    instances: Vec<(Id, ObjId)>,
    /// Interaction uses and the Astah interaction they refer to.
    references: Vec<(Id, ObjId)>,
    /// Activity graphs and state machines with the element they belong to.
    behaviors: Vec<(Id, ObjId)>,
    diagrams: Vec<(Id, ObjId)>,
}

pub fn import(graph: &Graph) -> Option<Imported> {
    let r = Reader { g: graph };
    let root = project_model(&r)?;
    let model = Model::new(r.name(root));
    let mut im = Importer {
        r,
        ids: HashMap::from([(root, model.root())]),
        model,
        skipped: BTreeMap::new(),
        features: Vec::new(),
        queued: HashSet::new(),
        replied: HashSet::new(),
        relationships: Vec::new(),
        instances: Vec::new(),
        references: Vec::new(),
        behaviors: Vec::new(),
        diagrams: Vec::new(),
    };
    let root_id = im.model.root();
    im.walk(root, root_id);
    for (owner, feature) in std::mem::take(&mut im.features) {
        im.feature(owner, feature);
    }
    // Dependencies may connect instances, so they come first.
    for (owner, instance) in std::mem::take(&mut im.instances) {
        im.instance(owner, instance);
    }
    for (owner, relationship) in std::mem::take(&mut im.relationships) {
        im.relationship(owner, relationship);
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
    Some(Imported { model: im.model, skipped: im.skipped })
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
            .map(|(key, value)| TaggedValue { key, value })
            .collect();
        data.documentation = self.r.text(object, "definition");
        data
    }

    /// Inserts an element under `owner`, or under the nearest ancestor that may own it.
    fn create(&mut self, owner: Id, data: ElementData, object: Option<ObjId>) -> Option<Id> {
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
                    self.skip(object);
                }
                None
            }
        }
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
                    let data = self.data(child, ElementKind::Package, &[]);
                    if let Some(package) = self.create(id, data, Some(child)) {
                        self.walk(child, package);
                    }
                }
                "UClassifierImp" | "UInterfaceImp" | "UActorImp" | "UUseCaseImp"
                | "UComponentImp" | "UArtifactImp" | "UNodeImp" => {
                    // Anonymous classifiers bind a template to arguments; they live on as type
                    // names such as `List<Projekt>` (see `Reader::type_name`).
                    if !self.r.list(child, "templateBinding").is_empty()
                        && self.r.name(child).is_empty()
                    {
                        continue;
                    }
                    // Template parameters are classifiers owned by a template parameter. Some
                    // classes declare `templateParameter` as a (usually empty) list. Astah
                    // creates them for library types like `List<E>`; UmlCraft has no templates
                    // and shows their uses as type names, so they are left out silently.
                    let is_parameter = |field| {
                        self.r.obj(child, field).is_some_and(|p| self.r.class(p) != "ArrayList")
                    };
                    if is_parameter("owningParameter") || is_parameter("templateParameter") {
                        continue;
                    }
                    let (kind, hide) = self.classifier_kind(child);
                    let data = self.data(child, kind, hide);
                    if let Some(classifier) = self.create(id, data, Some(child)) {
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
                // Features listed by their classifier are queued above already.
                "UAttributeImp" | "UOperationImp" => {
                    if self.queued.insert(child) {
                        self.features.push((id, child));
                    }
                }
                // Parameters come with their operation, association ends with their association.
                "JUParameterImp" | "UParameterImp" | "UAssociationEndImp" => {}
                "UAssociationImp" | "UGeneralizationImp" | "UDependencyImp" | "UUsageImp"
                | "UAbstractionImp" | "URealizationImp" | "UIncludeImp" | "UExtendImp" => {
                    self.relationships.push((id, child));
                }
                "UDiagramImp" | "USequenceDiagramImp" => self.diagrams.push((id, child)),
                "UComponentInstanceImp" | "UNodeInstanceImp" | "UObjectImp" => {
                    self.instances.push((id, child));
                }
                // Bindings live on in the bound types' names, see `Reader::type_name`.
                "UTemplateBindingImp" => {}
                // Constraints on association ends come with their association.
                "UConstraintImp"
                    if self
                        .r
                        .list(child, "constrainedElement")
                        .iter()
                        .all(|&e| self.r.class(e) == "UAssociationEndImp") => {}
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
        };
        let stereotypes = self.r.stereotypes(object);
        let has = |s: &str| stereotypes.iter().any(|t| t == s);
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
            _ if has("dataType") || has("datatype") => {
                (ElementKind::DataType(props), &["dataType", "datatype"])
            }
            _ => (ElementKind::Class(props), &[]),
        }
    }

    /// Points interaction uses to the interactions they refer to, now that all exist.
    fn resolve_references(&mut self) {
        for (id, target) in std::mem::take(&mut self.references) {
            let Some(&interaction) = self.ids.get(&target) else { continue };
            if !matches!(
                self.model.get(interaction).map(|e| e.kind()),
                Some(ElementKind::Interaction)
            ) {
                continue;
            }
            let Some(e) = self.model.get(id) else { continue };
            let mut data = e.data().clone();
            if let ElementKind::InteractionUse(p) = &mut data.kind {
                p.refers_to = Some(interaction);
                let _ = self.model.apply(Op::UpdateElement { id, data });
            }
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
        self.create(owner, data, Some(object));
    }

    /// A type reference: a classifier of the model, or the type's name as text.
    fn type_ref(&self, typed: ObjId) -> Option<TypeRef> {
        let ty = self.r.obj(typed, "type")?;
        if let Some(id) = self.classifier(ty) {
            return Some(TypeRef::Element(id));
        }
        let name = self.r.type_name(ty, 0);
        (!name.is_empty()).then_some(TypeRef::Text(name))
    }

    fn feature(&mut self, owner: Id, object: ObjId) {
        let is_static = self.r.text(object, "ownerScope") == "classifier";
        if self.r.class(object) == "UAttributeImp" {
            let owner_is_enum = matches!(
                self.model.get(owner).map(|e| e.kind()),
                Some(ElementKind::Enumeration(_))
            );
            let kind = if owner_is_enum {
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
            let data = self.data(object, kind, &[]);
            self.create(owner, data, Some(object));
            return;
        }

        let parameters = self.r.list(object, "parameter");
        let r = self.r;
        let is_return = move |p: &ObjId| r.text(*p, "kind") == "return";
        let return_type = parameters
            .iter()
            .find(|p| is_return(p))
            .and_then(|&p| self.type_ref(p))
            .filter(|t| *t != TypeRef::Text("void".into()));
        let kind = ElementKind::Operation(OperationProps {
            return_type,
            is_static,
            is_abstract: self.r.flag(object, "isAbst"),
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
        let r = self.r;
        let mapped = |field: &str| r.obj(object, field).and_then(|o| self.ids.get(&o).copied());
        let first =
            |field: &str| r.list(object, field).first().and_then(|o| self.ids.get(o).copied());
        let class = r.class(object);
        let stereotypes = r.stereotypes(object);
        let kind = match class {
            "UAssociationImp" => {
                let ends: Vec<AssociationEnd> = r
                    .list(object, "connection")
                    .into_iter()
                    .filter_map(|end| self.association_end(end))
                    .collect();
                ends.try_into().ok().map(|ends| ElementKind::Association(AssociationProps { ends }))
            }
            "UGeneralizationImp" => {
                let specific = r.obj(object, "subtype").and_then(|o| self.classifier(o));
                let general = r.obj(object, "supertype").and_then(|o| self.classifier(o));
                specific.zip(general).map(|(specific, general)| {
                    ElementKind::Generalization(GeneralizationProps { specific, general })
                })
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
                let realize = class == "URealizationImp"
                    || stereotypes.iter().any(|s| s == "realize" || s == "realization");
                first("client").zip(first("supplier")).filter(|_| !hidden).map(
                    |(client, supplier)| {
                        let props = DirectedProps { client, supplier };
                        if realize {
                            ElementKind::Realization(props)
                        } else {
                            ElementKind::Dependency(props)
                        }
                    },
                )
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
        for vertex in vertices {
            let Some(node) = self.activity_node_kind(vertex) else {
                self.skip(vertex);
                continue;
            };
            // Object nodes have a classifier in a state as their type; the state is left out.
            let type_ref = (node == ActivityNodeKind::Object)
                .then(|| r.obj(vertex, "type").and_then(|t| self.type_ref(t)))
                .flatten();
            let mut data = self.data(
                vertex,
                ElementKind::ActivityNode(ActivityNodeProps { node, type_ref }),
                &[],
            );
            // Astah names control nodes by itself; UmlCraft neither shows nor needs those names.
            if !matches!(node, ActivityNodeKind::Action | ActivityNodeKind::Object)
                && is_default_node_name(&data.name)
            {
                data.name.clear();
            }
            self.create(activity, data, Some(vertex));
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
            // Orthogonal regions are anonymous composite states; UmlCraft has no regions, so
            // their states go to the state that holds the regions.
            if r.flag(composite, "isOrthogonal") && r.name(vertex).is_empty() {
                *self.skipped.entry("OrthogonalRegion".to_owned()).or_default() += 1;
                self.vertices(owner, vertex);
                continue;
            }
            let action = |field| r.obj(vertex, field).map(|a| r.name(a)).unwrap_or_default();
            let kind = match r.class(vertex) {
                "UCompositeStateImp" | "UStateImp" | "USimpleStateImp" => {
                    ElementKind::State(StateProps {
                        entry: action("entry"),
                        do_activity: action("doActivity"),
                        exit: action("exit"),
                    })
                }
                "UFinalStateImp" => ElementKind::FinalState,
                "UPseudostateImp" => {
                    let pseudostate = match r.text(vertex, "kind").as_str() {
                        "initial" => PseudostateKind::Initial,
                        "choice" => PseudostateKind::Choice,
                        "junction" => PseudostateKind::Junction,
                        "fork" => PseudostateKind::Fork,
                        "join" => PseudostateKind::Join,
                        "shallowHistory" => PseudostateKind::ShallowHistory,
                        "deepHistory" => PseudostateKind::DeepHistory,
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
            if !r.list(vertex, "internalTransition").is_empty() {
                *self.skipped.entry("InternalTransition".to_owned()).or_default() += 1;
            }
            let Some(id) = self.create(owner, data, Some(vertex)) else { continue };
            self.vertices(id, vertex);
        }
    }

    fn partition(&mut self, owner: Id, partition: ObjId) {
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
        Some(match self.r.class(vertex) {
            // A subactivity calls another activity; UmlCraft shows it as an action.
            "UActionStateImp" | "USubactivityStateImp" => ActivityNodeKind::Action,
            "UObjectFlowStateImp" => ActivityNodeKind::Object,
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
        let data = self.data(object, ElementKind::diagram(kind), &[]);
        let Some(diagram) = self.create(owner, data, Some(object)) else { return };
        self.presentations(diagram, kind, object);
    }

    /// Creates the interaction behind a sequence diagram: lifelines, then messages and
    /// combined fragments from top to bottom.
    fn interaction(&mut self, owner: Id, diagram: ObjId) -> Option<Id> {
        let r = self.r;
        let collaboration = r.obj(diagram, "collaboration")?;
        let source = *r.list(collaboration, "interactions").first()?;
        let data = ElementData::new(r.name(diagram), ElementKind::Interaction);
        let interaction = self.create(owner, data, Some(source))?;

        for ownership in self.r.list(collaboration, "ownedElementOwnership") {
            let Some(role) = self.r.obj(ownership, "ownedElement") else { continue };
            if self.r.class(role) != "UClassifierRoleImp" {
                continue;
            }
            let represents = self.r.obj(role, "base").and_then(|b| self.classifier(b));
            let data = self.data(role, ElementKind::Lifeline(LifelineProps { represents }), &[]);
            self.create(interaction, data, Some(role));
        }

        // The interaction lists every message and fragment; those inside an operand belong to
        // the operand.
        let mut nested = HashSet::new();
        let mut stack = self.r.list(source, "fragments");
        while let Some(fragment) = stack.pop() {
            for operand in self.r.list(fragment, "operands") {
                nested.extend(self.r.list(operand, "messages"));
                nested.extend(self.r.list(operand, "fragments"));
                stack.extend(self.r.list(operand, "fragments"));
            }
        }
        let top_level = |list: Vec<ObjId>| -> Vec<ObjId> {
            list.into_iter().filter(|m| !nested.contains(m)).collect()
        };
        let messages = top_level(self.r.list(source, "message"));
        let fragments = top_level(self.r.list(source, "fragments"));
        // A reply points to its call as `activator`. The interaction lists all messages.
        for message in self.r.list(source, "message") {
            if action_type(self.r, message) == Some(REPLY)
                && let Some(call) = self.r.obj(message, "activator")
            {
                self.replied.insert(call);
            }
        }
        self.interaction_items(interaction, messages, fragments);
        Some(interaction)
    }

    /// Vertical position of a message or fragment on its diagram, for ordering.
    fn top(&self, object: ObjId) -> f32 {
        let r = self.r;
        r.list(object, "presentation")
            .first()
            .and_then(|&p| {
                r.points(p, "allPoints")
                    .first()
                    .map(|pt| pt.y)
                    .or_else(|| r.obj(p, "location").and_then(|l| r.point(l)).map(|pt| pt.y))
            })
            .unwrap_or(f32::MAX)
    }

    fn interaction_items(&mut self, container: Id, messages: Vec<ObjId>, fragments: Vec<ObjId>) {
        let mut items: Vec<(f32, bool, ObjId)> = messages
            .into_iter()
            .map(|m| (self.top(m), true, m))
            .chain(fragments.into_iter().map(|f| (self.top(f), false, f)))
            .collect();
        items.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, is_message, object) in items {
            if is_message {
                self.message(container, object);
            } else {
                self.fragment(container, object);
            }
        }
    }

    fn message(&mut self, container: Id, object: ObjId) {
        let r = self.r;
        let lifeline = |field| {
            r.obj(object, field).and_then(|o| self.ids.get(&o).copied()).filter(|id| {
                matches!(self.model.get(*id).map(|e| e.kind()), Some(ElementKind::Lifeline(_)))
            })
        };
        // Found and lost messages start or end at a gate instead of a lifeline; Astah's API
        // leaves that end empty instead.
        let is_gate = |field| r.obj(object, field).is_none_or(|o| r.class(o) == "UGateImp");
        let (from, to, gate) = match (lifeline("sender"), lifeline("receiver")) {
            (Some(from), Some(to)) => (from, to, MessageGate::None),
            (None, Some(to)) if is_gate("sender") => (to, to, MessageGate::Found),
            (Some(from), None) if is_gate("receiver") => (from, from, MessageGate::Lost),
            _ => {
                self.skip(object);
                return;
            }
        };
        let action = r.obj(object, "action");
        let asynchronous = action.is_some_and(|a| r.flag(a, "asynchronous"));
        let sort = match action_type(r, object) {
            Some(CREATE) => MessageSort::Create,
            Some(REPLY) => MessageSort::Reply,
            Some(DESTROY) => MessageSort::Destroy,
            _ if asynchronous => MessageSort::Asynchronous,
            _ => MessageSort::Synchronous,
        };
        // Astah keeps the return value on the call; UmlCraft shows it on the reply.
        let return_value = match sort {
            MessageSort::Reply => Some(r.text(object, "returnValue"))
                .filter(|v| !v.is_empty())
                .or_else(|| r.obj(object, "activator").map(|call| r.text(call, "returnValue")))
                .unwrap_or_default(),
            _ if self.replied.contains(&object) => String::new(),
            _ => r.text(object, "returnValue"),
        };
        let arguments = action
            .map(|a| r.list(a, "actualArgument"))
            .unwrap_or_default()
            .into_iter()
            .map(|argument| r.text(argument, "value"))
            .collect::<Vec<_>>()
            .join(", ");
        let operation = action
            .and_then(|a| r.obj(a, "operation"))
            .and_then(|o| self.ids.get(&o).copied())
            .filter(|id| {
                matches!(self.model.get(*id).map(|e| e.kind()), Some(ElementKind::Operation(_)))
            });
        let kind = ElementKind::Message(MessageProps {
            sort,
            from,
            to,
            gate,
            operation,
            arguments,
            return_value,
        });
        let data = self.data(object, kind, &[]);
        self.create(container, data, Some(object));
    }

    fn fragment(&mut self, container: Id, object: ObjId) {
        let r = self.r;
        let covered =
            r.list(object, "covereds").iter().filter_map(|c| self.ids.get(c).copied()).collect();
        if r.class(object) == "UInteractionUseImp" {
            // The interaction it refers to may not exist yet; see `resolve_references`.
            let arguments = r.text(object, "argument");
            let kind = ElementKind::InteractionUse(InteractionUseProps {
                refers_to: None,
                arguments,
                covered,
            });
            let data = self.data(object, kind, &[]);
            if let Some(id) = self.create(container, data, Some(object))
                && let Some(target) = r.obj(object, "refersTo")
            {
                self.references.push((id, target));
            }
            return;
        }
        if r.class(object) != "UCombinedFragmentImp" {
            self.skip(object);
            return;
        }
        let operator = match r.text(object, "operator").as_str() {
            "opt" => InteractionOperator::Opt,
            "loop" => InteractionOperator::Loop,
            "par" => InteractionOperator::Par,
            "break" => InteractionOperator::Break,
            "critical" => InteractionOperator::Critical,
            "neg" => InteractionOperator::Neg,
            "assert" => InteractionOperator::Assert,
            "strict" => InteractionOperator::Strict,
            "seq" => InteractionOperator::Seq,
            "ignore" => InteractionOperator::Ignore,
            "consider" => InteractionOperator::Consider,
            _ => InteractionOperator::Alt,
        };
        let kind = ElementKind::CombinedFragment(CombinedFragmentProps { operator, covered });
        let data = self.data(object, kind, &[]);
        let Some(fragment) = self.create(container, data, Some(object)) else { return };
        for operand in self.r.list(object, "operands") {
            let guard =
                self.r.obj(operand, "guard").map(|g| self.r.text(g, "body")).unwrap_or_default();
            let kind = ElementKind::InteractionOperand(InteractionOperandProps { guard });
            let data = self.data(operand, kind, &[]);
            let Some(id) = self.create(fragment, data, Some(operand)) else { continue };
            let messages = self.r.list(operand, "messages");
            let fragments = self.r.list(operand, "fragments");
            self.interaction_items(id, messages, fragments);
        }
    }

    /// Recreates the diagram's nodes and edges.
    fn presentations(&mut self, diagram: Id, kind: DiagramKind, object: ObjId) {
        // A greater depth lies further back.
        let mut sources = self.r.list(object, "presentation");
        let depth = |p: ObjId| self.r.g.field(p, "depth").and_then(Value::as_i64).unwrap_or(0);
        sources.sort_by_key(|&p| std::cmp::Reverse(depth(p)));
        let (edges, nodes): (Vec<ObjId>, Vec<ObjId>) = sources.into_iter().partition(|&p| {
            self.r.obj(p, "allPoints").is_some() || self.r.class(p) == "NoteAnchorPresentation"
        });

        let mut placed = Vec::new();
        for p in nodes {
            if let Some(node) = self.node(diagram, kind, p) {
                placed.push(node);
            }
        }
        let min = placed
            .iter()
            .map(|(_, n)| n.bounds)
            .fold((f32::MAX, f32::MAX), |(x, y), b| (x.min(b.x), y.min(b.y)));
        let shift = Point::new((MARGIN - min.0).max(0.0), (MARGIN - min.1).max(0.0));

        let mut presentation_of = HashMap::new();
        for (source, mut node) in placed {
            node.bounds.x += shift.x;
            node.bounds.y += shift.y;
            // Containers and fragments go to the back, as when they are drawn in UmlCraft.
            let container = node.element.and_then(|e| self.model.get(e)).is_some_and(|e| {
                palette::is_container(e.kind())
                    || matches!(e.kind(), ElementKind::CombinedFragment(_))
            });
            let presentation = Presentation {
                id: Id::generate(),
                element: node.element,
                shape: Shape::Node { bounds: node.bounds },
                style: node.style,
            };
            let index = if container {
                0
            } else {
                self.model.view(diagram).map_or(0, |v| v.presentations().len())
            };
            let id = presentation.id;
            if self.model.apply(Op::InsertPresentation { diagram, index, presentation }).is_ok() {
                presentation_of.insert(source, id);
            }
        }
        for p in edges {
            self.edge(diagram, p, &presentation_of, shift);
        }
        if kind == DiagramKind::Sequence {
            self.message_edges(diagram);
        }
    }

    /// One edge per message between the nodes of its lifelines. Astah attaches messages to
    /// activation bars instead, and UmlCraft derives their rows from the interaction's order,
    /// so the messages are taken from the model rather than from Astah's presentations.
    fn message_edges(&mut self, diagram: Id) {
        let Some(interaction) = self.model.get(diagram).and_then(|d| d.owner()) else { return };
        let node_of = |model: &Model, lifeline: Id| {
            model.view(diagram)?.presentations_of(lifeline).find(|p| !p.is_edge()).map(|p| p.id)
        };
        let mut edges = Vec::new();
        for id in self.model.subtree(interaction) {
            if let Some(ElementKind::Message(p)) = self.model.get(id).map(|e| e.kind())
                && let (Some(source), Some(target)) =
                    (node_of(&self.model, p.from), node_of(&self.model, p.to))
            {
                edges.push(Presentation::edge(Some(id), source, target, Vec::new()));
            }
        }
        for presentation in edges {
            let index = self.model.view(diagram).map_or(0, |v| v.presentations().len());
            let _ = self.model.apply(Op::InsertPresentation { diagram, index, presentation });
        }
    }

    fn node(&mut self, diagram: Id, kind: DiagramKind, p: ObjId) -> Option<(ObjId, NodeData)> {
        let r = self.r;
        let class = r.class(p);
        let element = match class {
            "FramePresentation" | "LabelPresentation" => return None,
            "NotePresentation" => self.note(diagram, p)?,
            _ => {
                let model = r.obj(p, "model")?;
                match self.ids.get(&model) {
                    Some(&id) => id,
                    None => return None,
                }
            }
        };
        let element_kind = self.model.get(element)?.kind().clone();
        if !palette::can_show(kind, &element_kind) {
            return None;
        }
        let r = self.r;
        let origin = r.obj(p, "location").and_then(|l| r.point(l)).unwrap_or_default();
        let default = palette::default_size_for(&element_kind);
        let width = r.num(p, "width").filter(|w| *w > 0.0).map_or(default.width, |w| w as f32);
        let mut height =
            r.num(p, "height").filter(|h| *h > 0.0).map_or(default.height, |h| h as f32);
        if let Some(length) = r.num(p, "lifelineLength") {
            height += length as f32;
        }
        let mut style = Style::default();
        if let Some(fill) =
            r.style(p, "fill.color").filter(|c| !c.eq_ignore_ascii_case(DEFAULT_FILL))
        {
            style.fill = fill.to_ascii_lowercase().parse().ok();
        }
        if class == "ClassifierPresentation" {
            style.hide_attributes =
                r.g.field(p, "allAttributeVisibility").and_then(Value::as_bool) == Some(false);
            style.hide_operations =
                r.g.field(p, "allOperationVisibility").and_then(Value::as_bool) == Some(false);
            style.iconic = matches!(element_kind, ElementKind::Interface(_))
                && r.g.field(p, "notationType").and_then(Value::as_i64) == Some(1);
        }
        Some((
            p,
            NodeData {
                element: Some(element),
                bounds: Rect::new(origin.x, origin.y, width, height),
                style,
            },
        ))
    }

    /// The comment shown by a note, created on first use next to the diagram.
    fn note(&mut self, diagram: Id, p: ObjId) -> Option<Id> {
        let comment = self.r.obj(p, "model")?;
        if let Some(&id) = self.ids.get(&comment) {
            return Some(id);
        }
        // Astah lists the package a note belongs to among its annotated elements. Packages that
        // a note really annotates are connected by an anchor, which `edge` adds.
        let (owners, annotated): (Vec<Id>, Vec<Id>) = self
            .r
            .list(comment, "annotatedElement")
            .iter()
            .filter_map(|a| self.ids.get(a).copied())
            .partition(|&id| {
                matches!(
                    self.model.get(id).map(|e| e.kind()),
                    Some(ElementKind::Package | ElementKind::Model)
                )
            });
        let body = self.r.text(comment, "body");
        let data = self.data(comment, ElementKind::Comment(CommentProps { body, annotated }), &[]);
        let owner = match owners.first() {
            Some(&package) => package,
            None => self.model.get(diagram)?.owner()?,
        };
        self.create(owner, data, Some(comment))
    }

    fn edge(&mut self, diagram: Id, p: ObjId, presentation_of: &HashMap<ObjId, Id>, shift: Point) {
        let r = self.r;
        let ends: Vec<Id> =
            r.list(p, "servers").iter().filter_map(|s| presentation_of.get(s).copied()).collect();
        let [mut source, mut target] = ends[..] else { return };
        // Astah's "source" end and the A labels belong to the first server.
        let mut swapped = false;
        let element_of = |presentation: Id| {
            self.model.view(diagram).and_then(|v| v.get(presentation)).and_then(|p| p.element)
        };
        let element = if r.class(p) == "NoteAnchorPresentation" {
            // Anchors run from the note to the annotated element.
            let is_note = |id: Id| {
                element_of(id)
                    .and_then(|e| self.model.get(e))
                    .is_some_and(|e| matches!(e.kind(), ElementKind::Comment(_)))
            };
            if !is_note(source) && is_note(target) {
                std::mem::swap(&mut source, &mut target);
                swapped = true;
            }
            if let (Some(comment), Some(annotated)) = (element_of(source), element_of(target)) {
                self.annotate(comment, annotated);
            }
            None
        } else {
            let Some(&element) = r.obj(p, "model").and_then(|m| self.ids.get(&m)) else {
                return;
            };
            // Message edges are added from the model, see `message_edges`.
            if matches!(self.model.get(element).map(|e| e.kind()), Some(ElementKind::Message(_))) {
                return;
            }
            if let Some((from, to)) = self.model.get(element).and_then(|e| edge_ends(e.kind()))
                && element_of(source) == Some(to)
                && element_of(target) == Some(from)
            {
                std::mem::swap(&mut source, &mut target);
                swapped = true;
            }
            Some(element)
        };
        let mut waypoints: Vec<Point> =
            self.r.points(p, "points").into_iter().map(|pt| pt.offset(shift.x, shift.y)).collect();
        // Astah lists the waypoints from its first server to the second.
        if swapped {
            waypoints.reverse();
        }
        let mut presentation = Presentation::edge(element, source, target, waypoints.clone());
        let bounds = |id: Id| self.model.view(diagram)?.get(id)?.bounds();
        if let (Some(source_rect), Some(target_rect)) = (bounds(source), bounds(target)) {
            let geometry = EdgeGeometry { source_rect, target_rect, waypoints, shift, swapped };
            if let Shape::Edge { source_anchor, target_anchor, labels, .. } =
                &mut presentation.shape
            {
                (*source_anchor, *target_anchor, *labels) = self.edge_layout(p, &geometry);
            }
        }
        let index = self.model.view(diagram).map_or(0, |v| v.presentations().len());
        // An edge that does not fit its ends is left out, like a presentation of a skipped
        // element.
        let _ = self.model.apply(Op::InsertPresentation { diagram, index, presentation });
    }

    /// Where Astah's line aims inside its nodes, and where its labels sit. Labels are stored
    /// relative to the point they belong to, as the scene computes it: the end of the line for
    /// roles and multiplicities, the middle of the middle segment for the name.
    fn edge_layout(
        &self,
        p: ObjId,
        g: &EdgeGeometry,
    ) -> (Option<Point>, Option<Point>, EdgeLabels) {
        let r = self.r;
        // The center is where UmlCraft aims anyway; only other anchors are worth keeping.
        let anchor = |x, y| {
            Some(Point::new(r.num(p, x)? as f32, r.num(p, y)? as f32))
                .filter(|a| (a.x - 0.5).abs() > 1e-4 || (a.y - 0.5).abs() > 1e-4)
        };
        let (mut source_anchor, mut target_anchor) =
            (anchor("sourceEndX", "sourceEndY"), anchor("targetEndX", "targetEndY"));
        if g.swapped {
            std::mem::swap(&mut source_anchor, &mut target_anchor);
        }
        let inside = |rect: Rect, a: Option<Point>| match a {
            Some(a) => Point::new(rect.x + a.x * rect.width, rect.y + a.y * rect.height),
            None => rect.center(),
        };
        let (from, to) =
            (inside(g.source_rect, source_anchor), inside(g.target_rect, target_anchor));
        let start =
            border_point_from(g.source_rect, from, g.waypoints.first().copied().unwrap_or(to));
        let end = border_point_from(g.target_rect, to, g.waypoints.last().copied().unwrap_or(from));
        let mut line = vec![start];
        line.extend(&g.waypoints);
        line.push(end);
        let n = line.len();
        let (a, b) = (line[n / 2 - 1], line[n / 2]);
        let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

        // The top left corner of a label, relative to `origin`.
        let label = |field: &str, origin: Point| {
            let label = r.obj(p, field)?;
            let at = r.obj(label, "location").and_then(|l| r.point(l))?;
            let moved = r.obj(label, "localMovement").and_then(|l| r.point(l)).unwrap_or_default();
            Some(Point::new(
                at.x + moved.x + g.shift.x - origin.x,
                at.y + moved.y + g.shift.y - origin.y,
            ))
        };
        let (a_end, b_end) = if g.swapped { (end, start) } else { (start, end) };
        let (role_a, mult_a) =
            (label("roleAPresentation", a_end), label("multiplicityAPresentation", a_end));
        let (role_b, mult_b) =
            (label("roleBPresentation", b_end), label("multiplicityBPresentation", b_end));
        let mut labels =
            EdgeLabels { name: label("namePresentation", middle), ..Default::default() };
        (
            labels.source_role,
            labels.source_multiplicity,
            labels.target_role,
            labels.target_multiplicity,
        ) = if g.swapped {
            (role_b, mult_b, role_a, mult_a)
        } else {
            (role_a, mult_a, role_b, mult_b)
        };
        (source_anchor, target_anchor, labels)
    }

    fn annotate(&mut self, comment: Id, annotated: Id) {
        let Some(e) = self.model.get(comment) else { return };
        let mut data = e.data().clone();
        if let ElementKind::Comment(props) = &mut data.kind
            && !props.annotated.contains(&annotated)
        {
            props.annotated.push(annotated);
            let _ = self.model.apply(Op::UpdateElement { id: comment, data });
        }
    }
}

/// An edge's ends and waypoints in UmlCraft coordinates, and whether its ends are in the
/// opposite order of Astah's.
struct EdgeGeometry {
    source_rect: Rect,
    target_rect: Rect,
    waypoints: Vec<Point>,
    shift: Point,
    swapped: bool,
}

struct NodeData {
    element: Option<Id>,
    bounds: Rect,
    style: Style,
}

/// The elements an edge of this relationship runs from and to, as UmlCraft draws it.
fn edge_ends(kind: &ElementKind) -> Option<(Id, Id)> {
    match kind {
        ElementKind::Association(p) => Some((p.ends[0].participant, p.ends[1].participant)),
        ElementKind::Generalization(p) => Some((p.specific, p.general)),
        ElementKind::Realization(p) | ElementKind::Dependency(p) => Some((p.client, p.supplier)),
        ElementKind::Include(p) => Some((p.including, p.addition)),
        ElementKind::Extend(p) => Some((p.extension, p.extended)),
        ElementKind::Message(p) => Some((p.from, p.to)),
        ElementKind::ActivityEdge(p) => Some((p.source, p.target)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_markers() {
        for marker in ["Java Class", "Java Attribute", "Java Method", "C# Class", "C++ Method"] {
            assert!(is_language_marker(marker), "{marker}");
        }
        for stereotype in ["abstract", "Java", "Java class", "Java Class Thing", "entity"] {
            assert!(!is_language_marker(stereotype), "{stereotype}");
        }
    }
}
