//! Maps Astah's object graph onto an UmlCraft model.
//!
//! Astah follows the UML 1.4 metamodel (`JP.co.esm.caddies.uml.*`) plus its own presentation
//! classes (`JP.co.esm.caddies.jomt.jmodel.*`). Only what UmlCraft can represent is imported:
//! packages, classifiers with their features, use cases, the relationships between them, and
//! class, use case and sequence diagrams. Everything else is counted in
//! [`Imported::skipped`].

use std::collections::{BTreeMap, HashMap, HashSet};

use umlcraft_model::{
    Aggregation, AssociationEnd, AssociationProps, AttributeProps, ClassifierProps,
    CombinedFragmentProps, CommentProps, DiagramKind, DirectedProps, ElementData, ElementKind,
    ExtendProps, GeneralizationProps, Id, IncludeProps, InteractionOperandProps,
    InteractionOperator, LifelineProps, MessageProps, MessageSort, Model, Multiplicity,
    Navigability, Op, OperationProps, ParameterDirection, ParameterProps, Point, Presentation,
    Rect, Shape, Style, TaggedValue, TypeRef, Visibility,
};
use umlcraft_notation::palette;

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

/// Tagged values Astah uses for itself.
fn is_internal_tag(key: &str) -> bool {
    key.starts_with("jude.") || matches!(key, "hiddenDependency" | "matrix_access_type")
}

/// Astah marks elements of its ER, data flow and requirement notations with tagged values on
/// otherwise plain UML elements.
fn is_foreign_notation(tags: &[(String, String)]) -> bool {
    tags.iter().any(|(key, value)| {
        key.starts_with("jude.er.")
            || key.starts_with("jude.dfd.")
            || key.starts_with("jude.require.")
            || (key == "jude.type" && value.starts_with("er_"))
    })
}

struct Importer<'g> {
    r: Reader<'g>,
    model: Model,
    ids: HashMap<ObjId, Id>,
    skipped: BTreeMap<String, usize>,
    features: Vec<(Id, ObjId)>,
    relationships: Vec<(Id, ObjId)>,
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
        relationships: Vec::new(),
        diagrams: Vec::new(),
    };
    let root_id = im.model.root();
    im.walk(root, root_id);
    for (owner, feature) in std::mem::take(&mut im.features) {
        im.feature(owner, feature);
    }
    for (owner, relationship) in std::mem::take(&mut im.relationships) {
        im.relationship(owner, relationship);
    }
    for (owner, diagram) in std::mem::take(&mut im.diagrams) {
        im.diagram(owner, diagram);
    }
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
            .filter(|s| !s.is_empty() && !hide_stereotypes.contains(&s.as_str()))
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
                "UClassifierImp" | "UInterfaceImp" | "UActorImp" | "UUseCaseImp" => {
                    // Template parameters are classifiers owned by a template parameter;
                    // anonymous classifiers bind a template to arguments.
                    let bound = !self.r.list(child, "templateBinding").is_empty()
                        && self.r.name(child).is_empty();
                    // Some classes declare `templateParameter` as a (usually empty) list.
                    let is_parameter = |field| {
                        self.r.obj(child, field).is_some_and(|p| self.r.class(p) != "ArrayList")
                    };
                    if bound || is_parameter("owningParameter") || is_parameter("templateParameter")
                    {
                        self.skip(child);
                        continue;
                    }
                    let (kind, hide) = self.classifier_kind(child);
                    let data = self.data(child, kind, hide);
                    if let Some(classifier) = self.create(id, data, Some(child)) {
                        self.walk(child, classifier);
                    }
                }
                "UExtensionPointImp" => {
                    let data = self.data(child, ElementKind::ExtensionPoint, &[]);
                    self.create(id, data, Some(child));
                }
                "UAttributeImp" | "UOperationImp" => self.features.push((id, child)),
                // Parameters come with their operation, association ends with their association.
                "JUParameterImp" | "UParameterImp" | "UAssociationEndImp" => {}
                "UAssociationImp" | "UGeneralizationImp" | "UDependencyImp" | "UUsageImp"
                | "UAbstractionImp" | "URealizationImp" | "UIncludeImp" | "UExtendImp" => {
                    self.relationships.push((id, child));
                }
                "UDiagramImp" | "USequenceDiagramImp" => self.diagrams.push((id, child)),
                _ => self.skip(child),
            }
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

    /// A type reference: a classifier of the model, or the type's name as text.
    fn type_ref(&self, typed: ObjId) -> Option<TypeRef> {
        let ty = self.r.obj(typed, "type")?;
        if let Some(id) = self.classifier(ty) {
            return Some(TypeRef::Element(id));
        }
        let name = self.r.name(ty);
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
        let mut data = self.data(object, kind, hide);
        if class == "UUsageImp" && !data.stereotypes.iter().any(|s| s == "use") {
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

    fn diagram(&mut self, owner: Id, object: ObjId) {
        let kind = match (self.r.class(object), self.r.text(object, "type").as_str()) {
            ("USequenceDiagramImp", _) => DiagramKind::Sequence,
            (_, "Class Diagram") => DiagramKind::Class,
            (_, "UseCase Diagram") => DiagramKind::UseCase,
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
        let (Some(from), Some(to)) = (lifeline("sender"), lifeline("receiver")) else {
            // Found and lost messages start or end at a gate, which UmlCraft cannot represent.
            self.skip(object);
            return;
        };
        let action = r.obj(object, "action");
        let action_type = action.and_then(|a| r.g.field(a, "actionType")).and_then(Value::as_i64);
        let asynchronous = action.is_some_and(|a| r.flag(a, "asynchronous"));
        let sort = match action_type {
            Some(0) => MessageSort::Create,
            Some(2) => MessageSort::Reply,
            Some(4) => MessageSort::Destroy,
            _ if asynchronous => MessageSort::Asynchronous,
            _ => MessageSort::Synchronous,
        };
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
            operation,
            arguments: String::new(),
            return_value: r.text(object, "returnValue"),
        });
        let data = self.data(object, kind, &[]);
        self.create(container, data, Some(object));
    }

    fn fragment(&mut self, container: Id, object: ObjId) {
        let r = self.r;
        // Interaction uses ("ref") and the like have no counterpart yet.
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
        let covered =
            r.list(object, "covereds").iter().filter_map(|c| self.ids.get(c).copied()).collect();
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
        let annotated = self
            .r
            .list(comment, "annotatedElement")
            .iter()
            .filter_map(|a| self.ids.get(a).copied())
            .collect();
        let body = self.r.text(comment, "body");
        let data = self.data(comment, ElementKind::Comment(CommentProps { body, annotated }), &[]);
        let owner = self.model.get(diagram)?.owner()?;
        self.create(owner, data, Some(comment))
    }

    fn edge(&mut self, diagram: Id, p: ObjId, presentation_of: &HashMap<ObjId, Id>, shift: Point) {
        let r = self.r;
        let ends: Vec<Id> =
            r.list(p, "servers").iter().filter_map(|s| presentation_of.get(s).copied()).collect();
        let [mut source, mut target] = ends[..] else { return };
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
            }
            Some(element)
        };
        let waypoints =
            self.r.points(p, "points").into_iter().map(|pt| pt.offset(shift.x, shift.y)).collect();
        let presentation = Presentation::edge(element, source, target, waypoints);
        let index = self.model.view(diagram).map_or(0, |v| v.presentations().len());
        // An edge that does not fit its ends is left out, like a presentation of a skipped
        // element.
        let _ = self.model.apply(Op::InsertPresentation { diagram, index, presentation });
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
        _ => None,
    }
}
