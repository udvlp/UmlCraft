//! Merges the declarations of the IR into the model, in one transaction.
//!
//! Packages and types are found by name or created. A type whose kind changed (a placeholder
//! class that turns out to be an interface) is replaced by a new element that takes over its
//! children, references and presentations ([`retype`]). Members of types from the sources are
//! matched with what the model has: attributes and literals by name, operations by name and
//! parameter types, template parameters and parameters by position. Matched members are
//! updated, new ones created, the rest deleted, so presentations and hand-made relationships of
//! what stays survive a new import. Generalizations and realizations follow the sources;
//! dependencies are left alone.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    AttributeProps, ClassifierProps, DirectedProps, ElementData, ElementKind, GeneralizationProps,
    Id, Model, OperationProps, ParameterDirection, ParameterProps, Result, TaggedValue,
    TemplateParameterKind, TemplateParameterProps, Tx,
};
use umlcraft_notation::member::{template_parameters, type_text};

use crate::ir::{
    self, Field, Import, Item, Literal, Member, Operation, Relation, Scope, TemplateParam,
    TypeDecl, TypeKind, TypeName,
};
use crate::lang::{Rules, Skip};
use crate::report::Report;
use crate::resolve::{child_package, child_type, type_kind};

/// A type declared in the sources and the element it maps to.
pub(crate) struct Entry<'p> {
    pub decl: &'p TypeDecl,
    pub id: Id,
    /// The package the type is in.
    pub namespace: Id,
    /// The imports that apply, outermost scope first.
    pub imports: Vec<&'p Import>,
    /// The entry of the type this one is nested in.
    pub outer: Option<usize>,
}

/// Where a name is resolved: in a type (its entry) and maybe in one of its operations.
#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    pub entry: usize,
    pub operation: Option<Id>,
}

pub(crate) struct Importer<'p> {
    pub rules: &'static Rules,
    /// Tagged values of this language start with it: `jude.java.`.
    pub tag_prefix: &'static str,
    pub report: Report,
    pub entries: Vec<Entry<'p>>,
    /// The supertypes of each type from the sources, as resolved.
    pub supertypes: HashMap<Id, Vec<Id>>,
    /// Placeholders created in this import.
    pub placeholders: HashSet<Id>,
    /// Attributes created or updated from fields, with the entry of their type: the candidates
    /// for associations.
    pub fields: Vec<(Id, &'p Field, usize)>,
}

impl<'p> Importer<'p> {
    pub(crate) fn new(rules: &'static Rules, tag_prefix: &'static str) -> Self {
        Self {
            rules,
            tag_prefix,
            report: Report::default(),
            entries: Vec::new(),
            supertypes: HashMap::new(),
            placeholders: HashSet::new(),
            fields: Vec::new(),
        }
    }

    /// Finds or creates the packages and types of `scope` under `namespace`.
    pub(crate) fn declare_scope(
        &mut self,
        tx: &mut Tx<'_>,
        scope: &'p Scope,
        namespace: Id,
        imports: &[&'p Import],
    ) -> Result<()> {
        let mut imports = imports.to_vec();
        imports.extend(&scope.imports);
        for item in &scope.items {
            match item {
                Item::Namespace { path, scope } => {
                    let mut at = namespace;
                    for name in path {
                        at = match child_package(tx.model(), at, name) {
                            Some(p) => p,
                            None => {
                                tx.create(at, ElementData::new(name.clone(), ElementKind::Package))?
                            }
                        };
                    }
                    self.declare_scope(tx, scope, at, &imports)?;
                }
                Item::Type(decl) => {
                    self.declare_type(tx, decl, namespace, namespace, &imports, None)?;
                }
                // Neither has a place in a Java model; C++ will bring them.
                Item::Definition { .. } | Item::Alias { .. } => {}
                Item::Free(_) => *self.report.skipped.entry(Skip::FreeMember).or_default() += 1,
            }
        }
        Ok(())
    }

    fn declare_type(
        &mut self,
        tx: &mut Tx<'_>,
        decl: &'p TypeDecl,
        owner: Id,
        namespace: Id,
        imports: &[&'p Import],
        outer: Option<usize>,
    ) -> Result<()> {
        let existing = child_type(tx.model(), owner, &decl.name);
        let id = match existing {
            Some(id) if decl.is_forward => id,
            Some(id) => {
                let current = tx.model().element(id)?.kind().clone();
                let wanted = type_kind(decl.kind, classifier_props(&current));
                if std::mem::discriminant(&current) != std::mem::discriminant(&wanted) {
                    retype(tx, id, wanted)?
                } else {
                    id
                }
            }
            None => {
                let kind = type_kind(decl.kind, ClassifierProps::default());
                tx.create(owner, ElementData::new(decl.name.clone(), kind))?
            }
        };
        if !decl.is_forward {
            if existing.is_some() {
                self.report.updated += 1;
            } else {
                self.report.created += 1;
            }
        }
        self.placeholders.remove(&id);
        let index = self.entries.len();
        self.entries.push(Entry { decl, id, namespace, imports: imports.to_vec(), outer });
        for member in &decl.members {
            if let Member::Type(nested) = member {
                self.declare_type(tx, nested, id, namespace, imports, Some(index))?;
            }
        }
        Ok(())
    }

    /// Brings the types from the sources up to date: their own properties and template
    /// parameters first, then the supertypes of all of them, then their members, so member types
    /// resolve through supertypes declared anywhere in the import.
    pub(crate) fn merge(&mut self, tx: &mut Tx<'_>) -> Result<()> {
        // Several declarations may make one type (C# partial classes, forward declarations).
        let mut by_type: Vec<(Id, Vec<usize>)> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.decl.is_forward {
                continue;
            }
            match by_type.iter_mut().find(|(id, _)| *id == entry.id) {
                Some((_, list)) => list.push(index),
                None => by_type.push((entry.id, vec![index])),
            }
        }
        // Qualified names, and single-type imports of names the sources use, before any simple
        // name resolves: whether a simple name finds a placeholder in a package must not depend
        // on the order the files come in, and must be the same on a new import.
        let mut used: HashSet<&'p str> = HashSet::new();
        let mut qualified: Vec<(usize, &'p [String])> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            named_types(entry.decl, &mut |path| {
                if let Some(first) = path.first() {
                    used.insert(first);
                }
                if path.len() > 1 {
                    qualified.push((index, path));
                }
            });
        }
        let mut resolved: HashSet<&'p [String]> = HashSet::new();
        for index in 0..self.entries.len() {
            let imports = self.entries[index].imports.clone();
            for import in imports {
                let used = import.path.last().is_some_and(|last| used.contains(last.as_str()));
                if import.wildcard || import.is_static || !used || !resolved.insert(&import.path) {
                    continue;
                }
                let ctx = Ctx { entry: index, operation: None };
                self.resolve(tx, ctx, &import.path, TypeKind::Class)?;
            }
        }
        for (index, path) in qualified {
            let ctx = Ctx { entry: index, operation: None };
            self.resolve(tx, ctx, path, TypeKind::Class)?;
        }
        for (id, entries) in &by_type {
            self.properties(tx, *id, entries[0])?;
            let params = entries.iter().map(|&e| self.entries[e].decl).find_map(|d| {
                (!d.template_parameters.is_empty()).then_some(&d.template_parameters)
            });
            let ctx = Ctx { entry: entries[0], operation: None };
            self.template_parameters(tx, ctx, *id, params.map_or(&[][..], |p| &p[..]))?;
        }
        // Generalizations and realizations by the type they start from, found in one pass.
        let mut current: HashMap<Id, Vec<(Id, Id, Relation)>> = HashMap::new();
        for e in tx.model().elements() {
            let (from, to, relation) = match e.kind() {
                ElementKind::Generalization(p) => (p.specific, p.general, Relation::Extends),
                ElementKind::Realization(p) => (p.client, p.supplier, Relation::Implements),
                _ => continue,
            };
            current.entry(from).or_default().push((e.id(), to, relation));
        }
        for (id, entries) in &by_type {
            let current = current.remove(id).unwrap_or_default();
            self.supertypes(tx, *id, entries, current)?;
        }
        for (id, entries) in &by_type {
            self.members(tx, *id, entries)?;
        }
        Ok(())
    }

    /// Name, visibility, documentation, tags and flags of a type.
    fn properties(&mut self, tx: &mut Tx<'_>, id: Id, entry: usize) -> Result<()> {
        let decl = self.entries[entry].decl;
        let prefix = self.tag_prefix;
        tx.update(id, |data| {
            data.visibility = decl.visibility;
            data.documentation = decl.documentation.clone();
            set_tags(data, &decl.tags, prefix);
            if let Some(props) = classifier_props_mut(&mut data.kind) {
                props.is_abstract = decl.is_abstract;
                props.is_leaf = decl.is_leaf;
            }
        })
    }

    /// Matches the template parameters of `owner` by position with `params`.
    fn template_parameters(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        owner: Id,
        params: &[TemplateParam],
    ) -> Result<()> {
        let existing = template_parameters(tx.model(), owner);
        let mut ids = Vec::with_capacity(params.len());
        // Names first, so bounds such as `T extends Comparable<T>` find them.
        for (i, p) in params.iter().enumerate() {
            let id = match existing.get(i) {
                Some(&id) => {
                    tx.rename(id, p.name.clone())?;
                    id
                }
                None => {
                    let kind = ElementKind::TemplateParameter(TemplateParameterProps::default());
                    tx.create(owner, ElementData::new(p.name.clone(), kind))?
                }
            };
            ids.push(id);
        }
        for &extra in existing.iter().skip(params.len()) {
            tx.delete(extra)?;
        }
        for (p, id) in params.iter().zip(ids) {
            let value_type = match &p.value_type {
                Some(t) => Some(self.type_ref(tx, ctx, t, TypeKind::Class)?),
                None => None,
            };
            let mut bounds = Vec::with_capacity(p.bounds.len());
            for b in &p.bounds {
                bounds.push(self.type_ref(tx, ctx, b, TypeKind::Class)?);
            }
            let default = match &p.default {
                Some(t) => Some(self.type_ref(tx, ctx, t, TypeKind::Class)?),
                None => None,
            };
            let props = TemplateParameterProps {
                parameter: if value_type.is_some() {
                    TemplateParameterKind::Value
                } else {
                    TemplateParameterKind::Type
                },
                type_ref: value_type,
                bounds,
                default,
                is_variadic: p.is_variadic,
            };
            tx.update(id, |data| data.kind = ElementKind::TemplateParameter(props))?;
        }
        Ok(())
    }

    /// Makes the generalizations and realizations of `id` those of the sources; `current` are
    /// the ones the model has, with their targets.
    fn supertypes(
        &mut self,
        tx: &mut Tx<'_>,
        id: Id,
        entries: &[usize],
        current: Vec<(Id, Id, Relation)>,
    ) -> Result<()> {
        let mut wanted: Vec<(Id, Relation)> = Vec::new();
        for &e in entries {
            // Resolved in the type: it sees its template parameters and outer types, but not
            // inherited member types, as its supertypes are not known yet.
            let ctx = Ctx { entry: e, operation: None };
            for s in &self.entries[e].decl.supertypes {
                let hint = match (s.relation, self.entries[e].decl.kind) {
                    (Relation::Implements, _) | (Relation::Extends, TypeKind::Interface) => {
                        TypeKind::Interface
                    }
                    _ => TypeKind::Class,
                };
                let path = match &s.ty.base {
                    TypeName::Named(path) => path.clone(),
                    TypeName::Builtin(_) => continue,
                };
                let mut target = self.resolve(tx, ctx, &path, hint)?;
                // A placeholder first seen as a class that is implemented is an interface.
                if s.relation == Relation::Implements
                    && self.placeholders.contains(&target)
                    && matches!(tx.model().element(target)?.kind(), ElementKind::Class(_))
                    && self.library_type(tx.model(), target).is_none()
                {
                    let new =
                        retype(tx, target, ElementKind::Interface(ClassifierProps::default()))?;
                    self.placeholders.remove(&target);
                    self.placeholders.insert(new);
                    target = new;
                }
                if target != id && !wanted.iter().any(|(t, _)| *t == target) {
                    wanted.push((target, s.relation));
                }
            }
        }
        self.supertypes.insert(id, wanted.iter().map(|(t, _)| *t).collect());
        // A retype may have replaced a target since `current` was taken.
        let current: Vec<(Id, Id, Relation)> = current
            .into_iter()
            .filter_map(|(r, _, relation)| {
                let target = match tx.model().get(r)?.kind() {
                    ElementKind::Generalization(p) => p.general,
                    ElementKind::Realization(p) => p.supplier,
                    _ => return None,
                };
                Some((r, target, relation))
            })
            .collect();
        let stale: Vec<Id> = current
            .iter()
            .filter(|&&(_, target, relation)| !wanted.contains(&(target, relation)))
            .map(|&(r, _, _)| r)
            .collect();
        tx.delete_many(&stale)?;
        for (target, relation) in wanted {
            if current.iter().any(|&(_, t, r)| t == target && r == relation) {
                continue;
            }
            let kind = match relation {
                Relation::Extends => ElementKind::Generalization(GeneralizationProps {
                    specific: id,
                    general: target,
                }),
                Relation::Implements => {
                    ElementKind::Realization(DirectedProps { client: id, supplier: target })
                }
            };
            tx.create(id, ElementData::new("", kind))?;
        }
        Ok(())
    }

    /// Merges the members of all declarations of type `id`.
    fn members(&mut self, tx: &mut Tx<'_>, id: Id, entries: &[usize]) -> Result<()> {
        let mut fields: Vec<(&'p Field, usize)> = Vec::new();
        let mut operations: Vec<(&'p Operation, usize)> = Vec::new();
        let mut literals: Vec<&'p Literal> = Vec::new();
        for &e in entries {
            for m in &self.entries[e].decl.members {
                match m {
                    Member::Field(f) => fields.push((f, e)),
                    Member::Operation(o) => operations.push((o, e)),
                    Member::Literal(l) => literals.push(l),
                    Member::Type(_) => {}
                }
            }
        }
        let children: Vec<Id> = tx.model().children(id).to_vec();
        let of_kind = |model: &Model, pred: fn(&ElementKind) -> bool| -> Vec<Id> {
            children
                .iter()
                .copied()
                .filter(|&c| model.get(c).is_some_and(|e| pred(e.kind())))
                .collect()
        };
        let model = tx.model();
        let mut attributes = of_kind(model, |k| matches!(k, ElementKind::Attribute(_)));
        let mut existing_ops = of_kind(model, |k| matches!(k, ElementKind::Operation(_)));
        let mut existing_literals =
            of_kind(model, |k| matches!(k, ElementKind::EnumerationLiteral));

        // The members as the sources list them: literals, attributes, then operations.
        let mut order = Vec::new();
        // Literals first, as an enum declares them.
        for literal in literals {
            let model = tx.model();
            let found = existing_literals
                .iter()
                .position(|&l| model.get(l).is_some_and(|l| l.name() == literal.name));
            let id_literal = match found {
                Some(i) => existing_literals.remove(i),
                None => tx.create(
                    id,
                    ElementData::new(literal.name.clone(), ElementKind::EnumerationLiteral),
                )?,
            };
            tx.update(id_literal, |data| data.documentation = literal.documentation.clone())?;
            order.push(id_literal);
        }

        for (field, e) in fields {
            let ctx = Ctx { entry: e, operation: None };
            let type_ref = self.type_ref(tx, ctx, &field.ty, TypeKind::Class)?;
            let model = tx.model();
            let found = attributes
                .iter()
                .position(|&a| model.get(a).is_some_and(|a| a.name() == field.name));
            let attribute = match found {
                Some(i) => attributes.remove(i),
                None => tx.create(
                    id,
                    ElementData::new(
                        field.name.clone(),
                        ElementKind::Attribute(AttributeProps::default()),
                    ),
                )?,
            };
            let prefix = self.tag_prefix;
            tx.update(attribute, |data| {
                data.visibility = field.visibility;
                data.documentation = field.documentation.clone();
                set_tags(data, &field.tags, prefix);
                if let ElementKind::Attribute(p) = &mut data.kind {
                    p.type_ref = Some(type_ref);
                    p.multiplicity = None;
                    p.default_value = field.default.clone().unwrap_or_default();
                    p.is_static = field.is_static;
                    p.is_read_only = field.is_read_only;
                }
            })?;
            self.fields.push((attribute, field, e));
            order.push(attribute);
        }

        for (operation, e) in operations {
            let key = signature_of_ir(operation);
            let model = tx.model();
            let found = existing_ops.iter().position(|&o| signature_of_model(model, o) == key);
            let op = match found {
                Some(i) => existing_ops.remove(i),
                None => tx.create(
                    id,
                    ElementData::new(
                        operation.name.clone(),
                        ElementKind::Operation(OperationProps::default()),
                    ),
                )?,
            };
            self.operation(tx, Ctx { entry: e, operation: Some(op) }, op, operation)?;
            order.push(op);
        }

        // What the sources no longer declare.
        let leftovers: Vec<Id> =
            attributes.into_iter().chain(existing_ops).chain(existing_literals).collect();
        tx.delete_many(&leftovers)?;
        reorder(tx, id, &order)
    }

    fn operation(&mut self, tx: &mut Tx<'_>, ctx: Ctx, op: Id, o: &'p Operation) -> Result<()> {
        // Template parameters first: parameter and return types may name them.
        self.template_parameters(tx, ctx, op, &o.template_parameters)?;
        let return_type = match &o.return_type {
            Some(t) => Some(self.type_ref(tx, ctx, t, TypeKind::Class)?),
            None => None,
        };
        let prefix = self.tag_prefix;
        tx.update(op, |data| {
            data.name = o.name.clone();
            data.visibility = o.visibility;
            data.documentation = o.documentation.clone();
            set_tags(data, &o.tags, prefix);
            if let ElementKind::Operation(p) = &mut data.kind {
                p.return_type = return_type;
                p.is_static = o.is_static;
                p.is_abstract = o.is_abstract;
                p.is_leaf = o.is_leaf;
            }
        })?;
        let model = tx.model();
        let existing: Vec<Id> = model
            .children(op)
            .iter()
            .copied()
            .filter(|&c| {
                model.get(c).is_some_and(|e| matches!(e.kind(), ElementKind::Parameter(_)))
            })
            .collect();
        for (i, p) in o.parameters.iter().enumerate() {
            let type_ref = self.type_ref(tx, ctx, &p.ty, TypeKind::Class)?;
            let parameter = match existing.get(i) {
                Some(&id) => id,
                None => tx.create(
                    op,
                    ElementData::new(
                        p.name.clone(),
                        ElementKind::Parameter(ParameterProps::default()),
                    ),
                )?,
            };
            tx.update(parameter, |data| {
                data.name = p.name.clone();
                if let ElementKind::Parameter(props) = &mut data.kind {
                    props.type_ref = Some(type_ref);
                    props.default_value = p.default.clone().unwrap_or_default();
                    props.direction = ParameterDirection::In;
                }
            })?;
        }
        for &extra in existing.iter().skip(o.parameters.len()) {
            tx.delete(extra)?;
        }
        Ok(())
    }
}

/// Replaces the tagged values of this language (`prefix…`) with `tags`, keeping all others.
fn set_tags(data: &mut ElementData, tags: &[(String, String)], prefix: &str) {
    data.tagged_values.retain(|t| !t.key.starts_with(prefix));
    data.tagged_values.extend(
        tags.iter().map(|(key, value)| TaggedValue { key: key.clone(), value: value.clone() }),
    );
}

fn classifier_props(kind: &ElementKind) -> ClassifierProps {
    match kind {
        ElementKind::Class(p)
        | ElementKind::Interface(p)
        | ElementKind::Enumeration(p)
        | ElementKind::DataType(p) => *p,
        _ => ClassifierProps::default(),
    }
}

fn classifier_props_mut(kind: &mut ElementKind) -> Option<&mut ClassifierProps> {
    match kind {
        ElementKind::Class(p)
        | ElementKind::Interface(p)
        | ElementKind::Enumeration(p)
        | ElementKind::DataType(p) => Some(p),
        _ => None,
    }
}

/// Calls `f` with every type name a declaration writes, its own members' included (nested
/// types are declarations of their own), template arguments too.
fn named_types<'p>(decl: &'p TypeDecl, f: &mut impl FnMut(&'p [String])) {
    fn walk<'p>(t: &'p ir::TypeExpr, f: &mut impl FnMut(&'p [String])) {
        if let TypeName::Named(path) = &t.base {
            f(path);
        }
        for a in &t.arguments {
            walk(a, f);
        }
    }
    fn template<'p>(params: &'p [TemplateParam], f: &mut impl FnMut(&'p [String])) {
        for p in params {
            p.value_type.iter().chain(&p.bounds).chain(&p.default).for_each(|t| walk(t, f));
        }
    }
    template(&decl.template_parameters, f);
    decl.supertypes.iter().for_each(|s| walk(&s.ty, f));
    for member in &decl.members {
        match member {
            Member::Field(field) => walk(&field.ty, f),
            Member::Operation(o) => {
                template(&o.template_parameters, f);
                o.return_type.iter().for_each(|t| walk(t, f));
                o.parameters.iter().for_each(|p| walk(&p.ty, f));
            }
            Member::Literal(_) | Member::Type(_) => {}
        }
    }
}

/// Puts the members `order` of `owner` in that order, together where the first of them is;
/// the other children (supertype links, nested types, associations) keep their order before
/// and after them. Members created by this import, which come last, so join the others.
fn reorder(tx: &mut Tx<'_>, owner: Id, order: &[Id]) -> Result<()> {
    let children = tx.model().children(owner).to_vec();
    let members: HashSet<Id> = order.iter().copied().collect();
    let first = children.iter().position(|c| members.contains(c)).unwrap_or(children.len());
    let target: Vec<Id> = children[..first]
        .iter()
        .copied()
        .chain(order.iter().copied())
        .chain(children[first..].iter().copied().filter(|c| !members.contains(c)))
        .collect();
    for (index, &id) in target.iter().enumerate() {
        if tx.model().children(owner).get(index) != Some(&id) {
            tx.move_element(id, owner, index)?;
        }
    }
    Ok(())
}

/// An operation's name and parameter types, as text without package names, to match an
/// operation of the sources with one of the model.
fn signature_of_ir(o: &Operation) -> (String, Vec<String>) {
    let simple = |t: &ir::TypeExpr| simple_text(t);
    (o.name.clone(), o.parameters.iter().map(|p| simple(&p.ty)).collect())
}

fn simple_text(t: &ir::TypeExpr) -> String {
    let base = match &t.base {
        TypeName::Named(path) => path.last().cloned().unwrap_or_default(),
        TypeName::Builtin(name) => name.clone(),
    };
    let mut s = format!("{}{base}", t.prefix);
    if !t.arguments.is_empty() {
        let args: Vec<String> = t.arguments.iter().map(simple_text).collect();
        s.push_str(&format!("<{}>", args.join(",")));
    }
    s.push_str(&t.modifier);
    s
}

fn signature_of_model(model: &Model, op: Id) -> (String, Vec<String>) {
    let name = model.get(op).map(|e| e.name().to_owned()).unwrap_or_default();
    let types = model
        .children(op)
        .iter()
        .filter_map(|&c| match model.get(c)?.kind() {
            ElementKind::Parameter(p) => {
                Some(p.type_ref.as_ref().map(|t| type_text(model, t)).unwrap_or_default())
            }
            _ => None,
        })
        .collect();
    (name, types)
}

/// Replaces element `old` by a new one of `kind` at the same place: it takes over the children
/// the new kind can own, every reference to `old` and its presentations. Returns the new id.
pub(crate) fn retype(tx: &mut Tx<'_>, old: Id, kind: ElementKind) -> Result<Id> {
    let element = tx.model().element(old)?;
    let owner = element.owner().expect("types have owners");
    let index = tx.model().children(owner).iter().position(|&c| c == old).unwrap_or(0);
    let mut data = element.data().clone();
    data.kind = kind.clone();
    let children = element.children().to_vec();
    let new = tx.create_at(owner, index, data)?;
    for child in children {
        let fits = tx.model().get(child).is_some_and(|c| kind.can_own(c.kind()));
        if fits {
            let end = tx.model().children(new).len();
            tx.move_element(child, new, end)?;
        }
    }
    for (referrer, _) in tx.model().referrers(old) {
        tx.update(referrer, |data| {
            data.kind.for_each_reference_mut(|id| {
                if *id == old {
                    *id = new;
                }
            });
        })?;
    }
    for (diagram, presentation) in tx.model().shown_on(old) {
        tx.update_presentation(diagram, presentation, |p| p.element = Some(new))?;
    }
    tx.delete(old)?;
    Ok(new)
}
