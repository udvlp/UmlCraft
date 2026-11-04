//! Astah's ER models: the ER model with its schemas and packages, entities with their
//! attributes, relationships and subtype relationships, and the ER diagrams that show them
//! (`docs/asta-format.md`, "ER models").
//!
//! Astah draws ER diagrams in IE or IDEF1X; they come in with that notation. Domains become
//! domains of the ER model's package, which columns of that type name; the datatype model is
//! not imported: an attribute keeps its type's name.

use umlcraft_model::{
    Cardinality, CardinalityMax, ErAttributeProps, ErDisplay, ErDomainProps, ErEntityProps,
    ErIndex, ErIsAProps, ErKey, ErNotation, ErParticipationProps, ErRelationshipProps,
    ErSubtypingProps,
};

use super::*;

/// The tagged values Astah keeps ER names and types in, which become properties.
const ER_TAGS: [&str; 3] = ["alias1", "alias2", "alias3"];

/// What the ER walk leaves for later: relationships once every entity exists, foreign key
/// columns once their relationships do.
#[derive(Default)]
pub(super) struct ErPending {
    relationships: Vec<(Id, ObjId)>,
    subtypes: Vec<(Id, ObjId)>,
    /// Foreign key columns and the Astah relationship or subtype relationship they belong to.
    foreign_keys: Vec<(Id, ObjId)>,
    /// The subtyping of each subtype relationship, for its presentations.
    subtypings: HashMap<ObjId, Id>,
    /// The IS-A of each subtype relationship.
    is_a: HashMap<ObjId, Id>,
}

/// Whether the tagged values mark Astah's ER model.
pub(super) fn is_er_model(tags: &[(String, String)]) -> bool {
    tags.iter().any(|(k, _)| k == "jude.er.model")
}

impl Importer<'_> {
    /// Name, documentation and the tagged values other than Astah's ER ones; the constraints
    /// Astah keeps keys, NOT NULL, UNIQUE and defaults in become properties.
    fn er_data(&self, object: ObjId, kind: ElementKind) -> ElementData {
        let mut data = self.data(object, kind, &[]);
        data.tagged_values.retain(|t| !ER_TAGS.contains(&t.key.as_str()));
        data.constraints.retain(|c| {
            !matches!(
                c.to_ascii_uppercase().as_str(),
                "PRIMARY KEY" | "NOT NULL" | "UNIQUE" | "FOREIGN KEY"
            ) && !c.starts_with("DEFAULT:")
        });
        data
    }

    /// Creates the schemas, packages, entities and diagrams the ER model `object` owns, as
    /// packages and elements under `id`.
    pub(super) fn er_walk(&mut self, object: ObjId, id: Id) {
        for ownership in self.r.list(object, "ownedElementOwnership") {
            let Some(child) = self.r.obj(ownership, "ownedElement") else { continue };
            let tags = self.r.tagged_values(child);
            let tagged = |key: &str| tags.iter().any(|(k, _)| k == key);
            match self.r.class(child) {
                // Astah's SQL types live on in the attributes that use them; the domains
                // become domains of the ER model's package.
                "UModelImp" if tagged("jude.er.datatype.model") => {}
                "UModelImp" if tagged("jude.er.domain.model") => {
                    for ownership in self.r.list(child, "ownedElementOwnership") {
                        // NOT NULL is read with its domain.
                        if let Some(domain) = self.r.obj(ownership, "ownedElement")
                            && self.r.class(domain) != "UConstraintImp"
                        {
                            self.er_domain(id, domain);
                        }
                    }
                }
                "UModelImp" | "UPackageImp" => {
                    let data = self.er_data(child, ElementKind::Package);
                    if let Some(package) = self.create(id, data, Some(child)) {
                        self.er_walk(child, package);
                    }
                }
                "EREntityImp" => self.er_entity(id, child),
                "ERRelationshipImp" => self.er.relationships.push((id, child)),
                "ERSubtypeRelationshipImp" => self.er.subtypes.push((id, child)),
                "UERDiagramImp" => self.diagrams.push((id, child)),
                // Keys and NOT NULL are read with the attributes, ends with their relationship.
                "UConstraintImp" | "ERRelationshipEndImp" => {}
                _ => self.skip(child),
            }
        }
    }

    fn er_entity(&mut self, owner: Id, object: ObjId) {
        let props =
            ErEntityProps { physical_name: self.tag(object, "alias3"), ..Default::default() };
        let data = self.er_data(object, ElementKind::ErEntity(props));
        let Some(entity) = self.create(owner, data, Some(object)) else { return };
        for attribute in self.r.list(object, "structuralFeature") {
            if self.r.class(attribute) == "ERAttributeImp" {
                self.er_attribute(entity, attribute);
            }
        }
        let indexes = self.er_indexes(object);
        if !indexes.is_empty() {
            let Some(mut data) = self.model.get(entity).map(|e| e.data().clone()) else { return };
            if let ElementKind::ErEntity(p) = &mut data.kind {
                p.indexes = indexes;
                let _ = self.model.apply(Op::UpdateElement { id: entity, data });
            }
        }
    }

    /// The name SQL knows an Astah ER element by: its physical name, else its logical one.
    fn er_sql_name(&self, object: ObjId) -> String {
        let physical = self.tag(object, "alias3");
        if physical.is_empty() { self.r.name(object) } else { physical }
    }

    /// A domain, named as SQL knows it so that the columns of that type find it.
    fn er_domain(&mut self, owner: Id, object: ObjId) {
        if self.tag(object, "jude.type") != "er_Domain" {
            self.skip(object);
            return;
        }
        let not_null = self
            .r
            .list(object, "constraint")
            .into_iter()
            .any(|c| self.r.text(c, "body").eq_ignore_ascii_case("not null"));
        let props = ErDomainProps {
            datatype: self.tag(object, "jude.domain.datatype"),
            length_precision: self.tag(object, "jude.domain.modifier"),
            not_null,
            default_value: self.tag(object, "jude.domain.default"),
            ..Default::default()
        };
        let mut data = self.er_data(object, ElementKind::ErDomain(props));
        data.name = self.er_sql_name(object);
        data.tagged_values.retain(|t| !t.key.starts_with("jude."));
        self.create(owner, data, Some(object));
    }

    /// The entity's indexes; the ones Astah keeps for keys and relationships are not SQL's.
    fn er_indexes(&self, entity: ObjId) -> Vec<ErIndex> {
        let r = self.r;
        r.list(entity, "erIndexes")
            .into_iter()
            .filter(|&i| !r.flag(i, "isKey") && r.list(i, "erRelationships").is_empty())
            .map(|i| ErIndex {
                name: r.name(i),
                columns: r
                    .list(i, "erAttributes")
                    .into_iter()
                    .map(|a| self.er_sql_name(a))
                    .collect(),
                unique: r.flag(i, "isUnique"),
            })
            .collect()
    }

    /// An attribute with what its entity's constraints say about it: `PRIMARY KEY`,
    /// `NOT NULL`, `UNIQUE`, `DEFAULT:<value>`, `FOREIGN KEY`.
    fn er_attribute(&mut self, entity: Id, object: ObjId) {
        let r = self.r;
        let mut props = ErAttributeProps {
            physical_name: self.tag(object, "alias3"),
            length_precision: self.tag(object, "jude.modifier"),
            ..Default::default()
        };
        if let Some(ty) = r.obj(object, "type") {
            if self.tag(ty, "jude.type") == "er_Domain" {
                props.datatype = self.er_sql_name(ty);
                // Astah copies the domain's length into its columns; the domain has it.
                if props.length_precision == self.tag(ty, "jude.domain.modifier") {
                    props.length_precision.clear();
                }
            } else {
                props.datatype = r.name(ty);
            }
        }
        for constraint in r.list(object, "constraint") {
            let body = r.text(constraint, "body");
            match body.as_str() {
                "PRIMARY KEY" => props.key = Some(ErKey::Primary),
                "NOT NULL" => props.not_null = true,
                "UNIQUE" => props.unique = true,
                _ => {
                    if let Some(value) = body.strip_prefix("DEFAULT:") {
                        props.default_value = value.to_owned();
                    }
                }
            }
        }
        let by = r.obj(object, "relationship");
        let data = self.er_data(object, ElementKind::ErAttribute(props));
        let Some(attribute) = self.create(entity, data, Some(object)) else { return };
        if let Some(by) = by {
            self.er.foreign_keys.push((attribute, by));
        }
    }

    /// Relationships and subtype relationships, then what refers to them.
    pub(super) fn er_relationships(&mut self) {
        for (owner, object) in std::mem::take(&mut self.er.relationships) {
            self.er_relationship(owner, object);
        }
        let mut subtypes = std::mem::take(&mut self.er.subtypes);
        // Grouped subtypes share one IS-A: Astah's group number per supertype.
        subtypes.sort_by_key(|&(_, s)| (self.r.obj(s, "supertype"), self.er_group(s)));
        for (owner, object) in subtypes {
            self.er_subtype(owner, object);
        }
        for (column, by) in std::mem::take(&mut self.er.foreign_keys) {
            let by = self.ids.get(&by).or_else(|| self.er.subtypings.get(&by)).copied();
            self.set_er_attribute(column, |p| p.foreign_key = by);
        }
    }

    fn set_er_attribute(&mut self, id: Id, f: impl FnOnce(&mut ErAttributeProps)) {
        let Some(mut data) = self.model.get(id).map(|e| e.data().clone()) else { return };
        if let ElementKind::ErAttribute(p) = &mut data.kind {
            f(p);
            let _ = self.model.apply(Op::UpdateElement { id, data });
        }
    }

    fn er_group(&self, subtype: ObjId) -> i64 {
        self.r.g.field(subtype, "group").and_then(Value::as_i64).unwrap_or(0)
    }

    /// A relationship between its two ends' entities: parent first, the child weak when
    /// identifying. The ends' names, Astah's verb phrases, are the roles.
    fn er_relationship(&mut self, owner: Id, object: ObjId) {
        let r = self.r;
        let ends = r.list(object, "connection");
        let [parent, child] = ends[..] else {
            self.skip(object);
            return;
        };
        let entity = |end: ObjId| r.obj(end, "type").and_then(|t| self.ids.get(&t).copied());
        let (Some(parent_entity), Some(child_entity)) = (entity(parent), entity(child)) else {
            self.skip(object);
            return;
        };
        let identifying = r.flag(object, "isIdentifying");
        let data = self
            .er_data(object, ElementKind::ErRelationship(ErRelationshipProps::new(identifying)));
        let Some(relationship) = self.create(owner, data, Some(object)) else { return };
        for (end, entity) in [(parent, parent_entity), (child, child_entity)] {
            let cardinality = r.multiplicity(end).map(|m| {
                let max = match m.upper {
                    None => CardinalityMax::Many,
                    Some(n) => CardinalityMax::Count(n),
                };
                Cardinality::new(m.lower, max)
            });
            let kind = ElementKind::ErParticipation(ErParticipationProps {
                relationship,
                entity,
                cardinality,
                look_here: None,
                role: r.name(end),
                total: false,
            });
            self.create(relationship, ElementData::new("", kind), Some(end));
        }
        if identifying {
            self.set_weak(child_entity);
        }
    }

    fn set_weak(&mut self, entity: Id) {
        let Some(mut data) = self.model.get(entity).map(|e| e.data().clone()) else { return };
        if let ElementKind::ErEntity(p) = &mut data.kind {
            p.weak = true;
            let _ = self.model.apply(Op::UpdateElement { id: entity, data });
        }
    }

    /// A subtype relationship: the subtype joins the IS-A of its supertype's group, which is
    /// made with the first. Astah's `conclusive` (complete) is a total IS-A.
    fn er_subtype(&mut self, owner: Id, object: ObjId) {
        let r = self.r;
        let entity = |field| r.obj(object, field).and_then(|e| self.ids.get(&e).copied());
        let (Some(supertype), Some(subtype)) = (entity("supertype"), entity("subtype")) else {
            self.skip(object);
            return;
        };
        let group = self.er_group(object);
        // The IS-A made for an earlier subtype relationship of the same supertype and group.
        let earlier = self.er.is_a.iter().find_map(|(&s, &is_a)| {
            (r.obj(s, "supertype") == r.obj(object, "supertype") && self.er_group(s) == group)
                .then_some(is_a)
        });
        let is_a = match earlier {
            Some(is_a) => is_a,
            None => {
                let props = ErIsAProps {
                    total: r.g.bool_field(object, "conclusive") != Some(false),
                    disjoint: None,
                    discriminator: r
                        .obj(object, "discriminatorAttribute")
                        .and_then(|a| self.ids.get(&a).copied()),
                };
                let Some(is_a) =
                    self.create(owner, ElementData::new("", ElementKind::ErIsA(props)), None)
                else {
                    return;
                };
                let kind = ElementKind::ErSubtyping(ErSubtypingProps {
                    is_a,
                    entity: supertype,
                    supertype: true,
                });
                self.create(is_a, ElementData::new("", kind), None);
                is_a
            }
        };
        self.er.is_a.insert(object, is_a);
        let kind =
            ElementKind::ErSubtyping(ErSubtypingProps { is_a, entity: subtype, supertype: false });
        // Astah does not show the name of a subtype relationship; UmlCraft would label the line.
        let mut data = self.er_data(object, kind);
        data.name.clear();
        let Some(subtyping) = self.create(is_a, data, Some(object)) else { return };
        self.er.subtypings.insert(object, subtyping);
        // The subtype's copy of the supertype's key.
        for column in r.list(object, "subTypeForeignKeys") {
            if let Some(&column) = self.ids.get(&column) {
                self.er.foreign_keys.push((column, object));
            }
        }
    }

    /// An ER diagram's notation, display level and model type.
    pub(super) fn er_diagram_props(&self, object: ObjId, props: &mut umlcraft_model::DiagramProps) {
        let r = self.r;
        props.notation = match r.text(object, "notation").as_str() {
            "IDEF1X" => ErNotation::Idef1x,
            _ => ErNotation::Ie,
        };
        props.er_display = match r.text(object, "displayLevel").as_str() {
            "entity" | "Entity" => ErDisplay::Name,
            "primarykey" | "Primary Key" => ErDisplay::Keys,
            _ => ErDisplay::Attributes,
        };
        props.physical_names = r.text(object, "modelType") == "Physical Model";
    }

    /// Astah's node of an IE or IDEF1X table includes the name above it; UmlCraft's starts at
    /// the box. Showing names only, the name is in the box.
    pub(super) fn er_table_bounds(&self, diagram: Id, bounds: Rect, style: &Style) -> Rect {
        let names_only = matches!(self.model.get(diagram).map(|d| d.kind()),
            Some(ElementKind::Diagram(p)) if p.er_display == ErDisplay::Name);
        if names_only {
            return bounds;
        }
        let band = scene::er_name_band(style.font_size.unwrap_or(scene::FONT_SIZE));
        Rect::new(bounds.x, bounds.y + band, bounds.width, (bounds.height - band).max(1.0))
    }

    /// The subtype symbols of an ER diagram, one IS-A node each, with a line to the supertype
    /// and one to each subtype. Astah keeps a symbol per subtype line (its `rect`) or one per
    /// group; the IS-A stands where the first of them is.
    pub(super) fn er_subtype_presentations(
        &mut self,
        diagram: Id,
        lines: &[ObjId],
        presentation_of: &HashMap<ObjId, Id>,
        shift: Point,
    ) {
        let r = self.r;
        let mut nodes: HashMap<Id, Id> = HashMap::new();
        for &p in lines {
            let Some(model) = r.obj(p, "model") else { continue };
            let (Some(&is_a), Some(&subtyping)) =
                (self.er.is_a.get(&model), self.er.subtypings.get(&model))
            else {
                self.lost_presentations += 1;
                continue;
            };
            let servers: Vec<Id> = r
                .list(p, "servers")
                .iter()
                .filter_map(|s| presentation_of.get(s).copied())
                .collect();
            let [super_node, sub_node, ..] = servers[..] else {
                self.lost_presentations += 1;
                continue;
            };
            let node = match nodes.get(&is_a) {
                Some(&node) => node,
                None => {
                    // The symbol: the line's own, else its group's.
                    let symbol = r.obj(p, "rect").and_then(|l| self.rect(l)).or_else(|| {
                        r.list(p, "servers").into_iter().find_map(|s| {
                            (r.class(s) == "GeneralizationGroupPresentation")
                                .then(|| r.obj(s, "location").and_then(|l| r.point(l)))
                                .flatten()
                                .map(|at| Rect::new(at.x - 10.0, at.y - 5.0, 20.0, 10.0))
                        })
                    });
                    let Some(symbol) = symbol else {
                        self.lost_presentations += 1;
                        continue;
                    };
                    let Some(kind) = self.model.get(is_a).map(|e| e.kind().clone()) else {
                        continue;
                    };
                    let size = palette::default_size_for(&kind);
                    let c = symbol.center();
                    let bounds = Rect::new(
                        c.x + shift.x - size.width / 2.0,
                        c.y + shift.y - size.height / 2.0,
                        size.width,
                        size.height,
                    );
                    let presentation = Presentation {
                        id: Id::generate(),
                        element: Some(is_a),
                        shape: Shape::Node { bounds, regions: None },
                        style: Style::default(),
                    };
                    let Some(node) = self.insert(diagram, presentation, false) else { continue };
                    let supertyping = self.model.children(is_a).iter().copied().find(|&c| {
                        matches!(self.model.get(c).map(|e| e.kind()),
                            Some(ElementKind::ErSubtyping(s)) if s.supertype)
                    });
                    if let Some(line) = supertyping {
                        let edge = Presentation::edge(Some(line), node, super_node, Vec::new());
                        self.insert(diagram, edge, false);
                    }
                    nodes.insert(is_a, node);
                    node
                }
            };
            // A group's lines run level from its symbol, then up or down into the subtype.
            let grouped = r
                .list(p, "servers")
                .into_iter()
                .any(|s| r.class(s) == "GeneralizationGroupPresentation");
            let bounds = |id: Id| self.model.view(diagram)?.get(id)?.bounds();
            let bend = match (grouped, bounds(node), bounds(sub_node)) {
                (true, Some(symbol), Some(sub)) => {
                    vec![Point::new(sub.center().x, symbol.center().y)]
                }
                _ => Vec::new(),
            };
            let mut edge = Presentation::edge(Some(subtyping), node, sub_node, bend);
            if grouped {
                edge.style.line_shape = LineShape::RightAngle;
            }
            self.insert(diagram, edge, false);
        }
    }
}
