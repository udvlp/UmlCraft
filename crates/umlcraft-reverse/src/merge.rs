//! Merges the declarations of the IR into the model, in one transaction.
//!
//! Packages and types are found by name or created. A type whose kind changed (a placeholder
//! class that turns out to be an interface) is replaced by a new element that takes over its
//! children, references and presentations ([`retype`]). Members of types from the sources are
//! matched with what the model has: attributes and literals by name, operations by name and
//! parameter types, template parameters and parameters by position. Matched members are
//! updated, new ones created, the rest deleted, so presentations and hand-made relationships of
//! what stays survive a new import. Members defined outside their type (C++'s
//! `void A::f() {}`) join the type's own: they fill in what its declaration leaves out, or add
//! an operation it does not declare. Generalizations and realizations follow the sources, so
//! one drawn by hand between types of the sources is deleted; dependencies are left alone.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use umlcraft_model::{
    AttributeProps, ClassifierProps, DirectedProps, ElementData, ElementKind, GeneralizationProps,
    Id, Model, OperationProps, ParameterProps, Result, TaggedValue, TemplateParameterKind,
    TemplateParameterProps, Tx,
};
use umlcraft_notation::member::{template_parameters, type_text};

use crate::Placed;
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
    /// The imports that apply, outermost scope first; shared by the types of one scope and
    /// cloned for every name resolved.
    pub imports: Rc<[&'p Import]>,
    /// The entry of the type this one is nested in.
    pub outer: Option<usize>,
    /// The file it is declared in (index into the parsed units).
    pub unit: usize,
}

/// A member defined outside its type (C++'s `void A::f() {}`) and where it stands.
pub(crate) struct Definition<'p> {
    pub owner: &'p [String],
    pub member: &'p Member,
    pub namespace: Id,
    pub imports: Rc<[&'p Import]>,
    pub unit: usize,
}

/// Where a name is resolved: in a type (its entry) and maybe in one of its operations.
#[derive(Clone, Copy)]
pub(crate) struct Ctx {
    pub entry: usize,
    pub operation: Option<Id>,
}

pub(crate) struct Importer<'p> {
    pub rules: &'static Rules,
    /// Tagged values of this language start with it: `java.`.
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
    /// The imports of each namespace's own scope, which it re-exports (`Rules::reexports`).
    pub namespace_imports: HashMap<Id, Vec<&'p Import>>,
    /// The re-exports being followed now, as package and name, against cycles.
    pub reexporting: Vec<(Id, String)>,
    /// The names whose single imports are being followed now, against cycles.
    pub importing: Vec<String>,
    /// The aliases (`typedef`) each namespace or type declares, by name.
    pub aliases: HashMap<Id, HashMap<&'p str, &'p ir::TypeExpr>>,
    /// The aliases being replaced by their targets now, against cycles.
    pub expanding: Vec<&'p str>,
    /// Members defined outside their type, until the types are known.
    pub definitions: Vec<Definition<'p>>,
    /// `Rules::implicit_imports` as paths, made once.
    pub implicit_imports: Rc<[Vec<String>]>,
    /// Types this import created, so that one declared in parts or forward first counts once as
    /// created.
    pub created: HashSet<Id>,
    /// The file whose scope is being declared.
    pub unit: usize,
    /// Where each file's declarations went.
    pub placed: Placed,
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
            definitions: Vec::new(),
            namespace_imports: HashMap::new(),
            reexporting: Vec::new(),
            importing: Vec::new(),
            aliases: HashMap::new(),
            expanding: Vec::new(),
            created: HashSet::new(),
            unit: 0,
            placed: Placed::default(),
            implicit_imports: rules
                .implicit_imports
                .iter()
                .map(|p| p.iter().map(|s| s.to_string()).collect())
                .collect(),
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
        let mut all = imports.to_vec();
        all.extend(&scope.imports);
        let imports: Rc<[&'p Import]> = all.into();
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
                    self.namespace_imports.entry(at).or_default().extend(&scope.imports);
                    self.declare_scope(tx, scope, at, &imports)?;
                }
                Item::Type(decl) => {
                    self.declare_type(tx, decl, namespace, namespace, &imports, None)?;
                }
                Item::Definition { owner, member } => self.definitions.push(Definition {
                    owner,
                    member,
                    namespace,
                    imports: imports.clone(),
                    unit: self.unit,
                }),
                Item::Alias { name, target } => {
                    self.aliases.entry(namespace).or_default().insert(name, target);
                }
                Item::Free(_) => {
                    *self.report.skipped.entry(Skip::FreeMember).or_default() += 1;
                    self.placed.free_namespace(self.unit, namespace);
                }
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
        imports: &Rc<[&'p Import]>,
        outer: Option<usize>,
    ) -> Result<()> {
        let existing = child_type(tx.model(), owner, &decl.name);
        // A name declared with two kinds (`class Foo` in one file, `interface Foo` in another)
        // keeps the kind of its first declaration in this import, so that a new import of the
        // same files gives the same element.
        let declared = |id| self.entries.iter().any(|e| e.id == id && !e.decl.is_forward);
        // Counted once per type: a second part (C# partial classes) is neither created nor
        // updated again.
        let counted = existing.is_some_and(declared);
        let id = match existing {
            Some(id) if decl.is_forward || declared(id) => id,
            Some(id) => {
                let current = tx.model().element(id)?.kind().clone();
                let wanted = type_kind(decl.kind, classifier_props(&current));
                if std::mem::discriminant(&current) != std::mem::discriminant(&wanted) {
                    self.retype(tx, id, wanted)?
                } else {
                    id
                }
            }
            None => {
                let kind = type_kind(decl.kind, ClassifierProps::default());
                let id = tx.create(owner, ElementData::new(decl.name.clone(), kind))?;
                self.created.insert(id);
                id
            }
        };
        if !decl.is_forward && !counted {
            if self.created.contains(&id) {
                self.report.created += 1;
            } else {
                self.report.updated += 1;
            }
        }
        self.placeholders.remove(&id);
        let index = self.entries.len();
        let unit = self.unit;
        self.entries.push(Entry { decl, id, namespace, imports: imports.clone(), outer, unit });
        for member in &decl.members {
            match member {
                Member::Type(nested) => {
                    self.declare_type(tx, nested, id, namespace, imports, Some(index))?;
                }
                Member::Alias { name, target } => {
                    self.aliases.entry(id).or_default().insert(name, target);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Replaces type `old` by a new element of `kind` (see [`retype`]) and makes everything
    /// this import remembers about `old` refer to the new element: earlier declarations of the
    /// same name with another kind (`class Foo` in one file, `interface Foo` in another), the
    /// supertypes found so far, aliases and placeholders.
    fn retype(&mut self, tx: &mut Tx<'_>, old: Id, kind: ElementKind) -> Result<Id> {
        let new = retype(tx, old, kind)?;
        for entry in &mut self.entries {
            if entry.id == old {
                entry.id = new;
            }
        }
        if let Some(supertypes) = self.supertypes.remove(&old) {
            self.supertypes.insert(new, supertypes);
        }
        for supertype in self.supertypes.values_mut().flatten() {
            if *supertype == old {
                *supertype = new;
            }
        }
        if let Some(aliases) = self.aliases.remove(&old) {
            self.aliases.insert(new, aliases);
        }
        if self.placeholders.remove(&old) {
            self.placeholders.insert(new);
        }
        Ok(new)
    }

    /// Brings the types from the sources up to date: their own properties and template
    /// parameters first, then the supertypes of all of them, then their members, so member types
    /// resolve through supertypes declared anywhere in the import.
    pub(crate) fn merge(&mut self, tx: &mut Tx<'_>) -> Result<()> {
        // Several declarations may make one type (C# partial classes, forward declarations).
        let mut by_type: Vec<(Id, Vec<usize>)> = Vec::new();
        let mut position: HashMap<Id, usize> = HashMap::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.decl.is_forward {
                continue;
            }
            match position.get(&entry.id) {
                Some(&at) => by_type[at].1.push(index),
                None => {
                    position.insert(entry.id, by_type.len());
                    by_type.push((entry.id, vec![index]));
                }
            }
        }
        // Qualified names, and single-type imports of names the sources use, before any simple
        // name resolves: whether a simple name finds a placeholder in a package must not depend
        // on the order the files come in, and must be the same on a new import.
        let mut used: HashSet<&'p str> = HashSet::new();
        let mut qualified: Vec<(usize, &'p [String])> = Vec::new();
        for (index, entry) in self.entries.iter().enumerate() {
            // An import used only as a qualifier (`nn.Module` after `import torch.nn as nn`)
            // names a module or namespace; the qualified name makes the right placeholders.
            named_types(entry.decl, &mut |path| match path {
                [] => {}
                [name] => {
                    used.insert(name);
                }
                _ => qualified.push((index, path)),
            });
        }
        let mut resolved: HashSet<&'p [String]> = HashSet::new();
        for index in 0..self.entries.len() {
            let imports = self.entries[index].imports.clone();
            for &import in imports.iter() {
                let used = import.visible_name().is_some_and(|name| used.contains(name));
                if import.wildcard || import.is_static || !used || !resolved.insert(&import.path) {
                    continue;
                }
                // A module (`import numpy as np`, Python): no type to make a placeholder of.
                let model = tx.model();
                let package = path_lookup_package(model, &import.path);
                if import.path.len() == 1 || package {
                    continue;
                }
                let ctx = Ctx { entry: index, operation: None };
                self.resolve_import(tx, ctx, &import.path, TypeKind::Class)?;
            }
        }
        for (index, path) in qualified {
            let ctx = Ctx { entry: index, operation: None };
            self.resolve(tx, ctx, path, TypeKind::Class)?;
        }
        for (id, entries) in &by_type {
            self.properties(tx, *id, entries)?;
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
        // Definitions belong to a type the sources declare; the owner is only looked up. Each
        // resolves its types in an entry of its own: the type's scope with the definition's own
        // imports added (`using namespace lib; void A::f(Str s) {}`).
        let mut defined: HashMap<Id, Vec<(&'p Member, usize)>> = HashMap::new();
        for d in std::mem::take(&mut self.definitions) {
            let model = tx.model();
            let owner = self
                .definition_owner(model, d.namespace, &d.imports, d.owner)
                .filter(|id| position.contains_key(id));
            // `void ns::f() {}`: a free function of a namespace.
            let skip = if self.is_namespace(model, d.namespace, d.owner) {
                Skip::FreeMember
            } else {
                Skip::UnknownOwner
            };
            match owner {
                Some(id) => {
                    let first = by_type[position[&id]].1[0];
                    let owner = &self.entries[first];
                    let mut imports = owner.imports.to_vec();
                    imports.extend(
                        d.imports
                            .iter()
                            .filter(|i| !owner.imports.iter().any(|o| std::ptr::eq(*o, **i))),
                    );
                    let entry = Entry { imports: imports.into(), unit: d.unit, ..*owner };
                    self.entries.push(entry);
                    defined.entry(id).or_default().push((d.member, self.entries.len() - 1));
                }
                None => *self.report.skipped.entry(skip).or_default() += 1,
            }
        }
        for (id, entries) in &by_type {
            let defined = defined.remove(id).unwrap_or_default();
            self.members(tx, *id, entries, &defined)?;
        }
        Ok(())
    }

    /// Name, visibility, documentation, tags and flags of a type. Of a type declared in parts,
    /// the first part gives visibility and tags, the first documented part the documentation;
    /// `abstract` and `sealed` on any part count for the type. As the import may hold only
    /// some of the parts, these keep what the type has where its parts here say nothing.
    fn properties(&mut self, tx: &mut Tx<'_>, id: Id, entries: &[usize]) -> Result<()> {
        let decls: Vec<&TypeDecl> = entries.iter().map(|&e| self.entries[e].decl).collect();
        let decl = decls[0];
        let documentation = decls.iter().map(|d| &d.documentation).find(|d| !d.is_empty());
        let is_abstract = decls.iter().any(|d| d.is_abstract);
        let is_leaf = decls.iter().any(|d| d.is_leaf);
        let partial = self.in_parts(entries);
        let prefix = self.tag_prefix;
        tx.update(id, |data| {
            data.visibility = decl.visibility;
            match documentation {
                Some(d) => data.documentation = d.clone(),
                None if partial => {}
                None => data.documentation.clear(),
            }
            set_tags(data, &decl.tags, prefix);
            if let Some(props) = classifier_props_mut(&mut data.kind) {
                props.is_abstract = is_abstract || (partial && props.is_abstract);
                props.is_leaf = is_leaf || (partial && props.is_leaf);
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
                    TypeName::Builtin(_) | TypeName::Unwritten => continue,
                };
                let mut target = self.resolve(tx, ctx, &path, hint)?;
                // A placeholder first seen as a class that is implemented is an interface.
                if s.relation == Relation::Implements
                    && self.placeholders.contains(&target)
                    && matches!(tx.model().element(target)?.kind(), ElementKind::Class(_))
                    && self.library_type(tx.model(), target).is_none()
                {
                    let interface = ElementKind::Interface(ClassifierProps::default());
                    target = self.retype(tx, target, interface)?;
                }
                let relation = if self.rules.base_list_by_target {
                    let to_interface =
                        matches!(tx.model().element(target)?.kind(), ElementKind::Interface(_));
                    let from_interface = self.entries[e].decl.kind == TypeKind::Interface;
                    if to_interface && !from_interface {
                        Relation::Implements
                    } else {
                        Relation::Extends
                    }
                } else {
                    s.relation
                };
                if target != id && !wanted.iter().any(|(t, _)| *t == target) {
                    wanted.push((target, relation));
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
        // A type declared in parts may name a supertype in a part the import does not hold.
        let stale: Vec<Id> = current
            .iter()
            .filter(|&&(_, target, relation)| !wanted.contains(&(target, relation)))
            .filter(|_| !self.in_parts(entries))
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

    /// Merges the members of all declarations of type `id`, and the members defined outside it
    /// (`defined`): a definition of a declared member gives it the documentation and parameter
    /// names (for a field, the default) the declaration leaves out; an operation defined but not
    /// declared is added after the declared members.
    fn members(
        &mut self,
        tx: &mut Tx<'_>,
        id: Id,
        entries: &[usize],
        defined: &[(&'p Member, usize)],
    ) -> Result<()> {
        let mut fields: Vec<(&'p Field, usize)> = Vec::new();
        let mut operations: Vec<(&'p Operation, usize)> = Vec::new();
        let mut literals: Vec<&'p Literal> = Vec::new();
        for &e in entries {
            for m in &self.entries[e].decl.members {
                match m {
                    Member::Field(f) => fields.push((f, e)),
                    Member::Operation(o) => operations.push((o, e)),
                    Member::Literal(l) => literals.push(l),
                    Member::Type(_) | Member::Alias { .. } => {}
                }
            }
        }
        let mut field_definitions: Vec<&'p Field> = Vec::new();
        // With the entry each definition resolves in.
        let mut definitions: Vec<Option<(&'p Operation, usize)>> = vec![None; operations.len()];
        let model = tx.model();
        // A C# partial method declared in one part and implemented in another (or the same) is
        // one operation: the second with the same signature counts as its definition.
        let partial = self.in_parts(entries);
        if partial {
            let mut kept: Vec<(&'p Operation, usize)> = Vec::new();
            let mut kept_definitions: Vec<Option<(&'p Operation, usize)>> = Vec::new();
            for (o, e) in operations {
                let key = self.signature(model, e, o);
                let same = kept.iter().position(|(k, ke)| self.signature(model, *ke, k) == key);
                match same {
                    Some(i) if kept_definitions[i].is_none() => kept_definitions[i] = Some((o, e)),
                    _ => {
                        kept.push((o, e));
                        kept_definitions.push(None);
                    }
                }
            }
            (operations, definitions) = (kept, kept_definitions);
        }
        for &(member, entry) in defined {
            match member {
                Member::Field(f) => field_definitions.push(f),
                Member::Operation(o) => {
                    let key = self.signature(model, entry, o);
                    let declared = |(d, e): &(&Operation, usize)| self.signature(model, *e, d);
                    match operations.iter().position(|d| declared(d) == key) {
                        Some(i) => {
                            definitions[i].get_or_insert((o, entry));
                        }
                        None => {
                            operations.push((o, entry));
                            definitions.push(None);
                        }
                    }
                }
                Member::Literal(_) | Member::Type(_) | Member::Alias { .. } => {}
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
            let definition = field_definitions.iter().find(|d| d.name == field.name);
            let documentation = match definition {
                Some(d) if field.documentation.is_empty() => &d.documentation,
                _ => &field.documentation,
            };
            let default = field.default.as_ref().or(definition.and_then(|d| d.default.as_ref()));
            let ctx = Ctx { entry: e, operation: None };
            let type_ref = self.written_type_ref(tx, ctx, &field.ty, TypeKind::Class)?;
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
                data.documentation = documentation.clone();
                set_tags(data, &field.tags, prefix);
                if let ElementKind::Attribute(p) = &mut data.kind {
                    p.type_ref = type_ref;
                    p.multiplicity = None;
                    p.default_value = default.cloned().unwrap_or_default();
                    p.is_static = field.is_static;
                    p.is_read_only = field.is_read_only;
                }
            })?;
            self.fields.push((attribute, field, e));
            order.push(attribute);
        }

        // Operations match by name and parameter types, and by being a query where that tells
        // two apart (C++'s `f()` and `f() const`); one that only became or stopped being a query
        // keeps its element.
        let model = tx.model();
        // The model's signatures once per class, not once per comparison.
        let mut existing: Vec<(Id, Signature)> =
            existing_ops.drain(..).map(|o| (o, signature_of_model(model, o))).collect();
        let mut matched: Vec<Option<Id>> = vec![None; operations.len()];
        for exact in [true, false] {
            for (i, &(operation, e)) in operations.iter().enumerate() {
                if matched[i].is_some() {
                    continue;
                }
                let (key, query) = self.signature(model, e, operation);
                let found =
                    existing.iter().position(|(_, (k, q))| *k == key && (!exact || *q == query));
                matched[i] = found.map(|f| existing.remove(f).0);
            }
        }
        existing_ops.extend(existing.into_iter().map(|(o, _)| o));
        for (((operation, e), found), definition) in
            operations.into_iter().zip(matched).zip(definitions)
        {
            let op = match found {
                Some(op) => op,
                None => tx.create(
                    id,
                    ElementData::new(
                        operation.name.clone(),
                        ElementKind::Operation(OperationProps::default()),
                    ),
                )?,
            };
            let ctx = Ctx { entry: e, operation: Some(op) };
            self.placed.operations.push((self.entries[e].unit, operation.span.clone(), op));
            if let Some((d, entry)) = definition {
                self.placed.operations.push((self.entries[entry].unit, d.span.clone(), op));
            }
            self.operation(tx, ctx, op, operation, definition.map(|(d, _)| d))?;
            order.push(op);
        }

        // What the sources no longer declare. Of a type declared in parts (C# `partial`), the
        // import may hold only some of the parts: what it does not find may stand in another
        // part, so it stays, and so does the order.
        let leftovers: Vec<Id> =
            attributes.into_iter().chain(existing_ops).chain(existing_literals).collect();
        if partial && !leftovers.is_empty() {
            return Ok(());
        }
        tx.delete_many(&leftovers)?;
        reorder(tx, id, &order)
    }

    /// Whether the type of `entries` is declared in parts (C# `partial`), of which an import
    /// may hold only some.
    pub(crate) fn in_parts(&self, entries: &[usize]) -> bool {
        !entries.is_empty() && entries.iter().all(|&e| self.entries[e].decl.is_partial)
    }

    /// The signature of `o`, declared in `entry`, with its aliases replaced as the model has
    /// them: typedefs by their targets, and names imported under another name
    /// (`from shop.orders import Order as O`, `using O = Shop.Order;`) by what they import.
    fn signature(&self, model: &Model, entry: usize, o: &Operation) -> Signature {
        let ctx = Ctx { entry, operation: None };
        let types = o
            .parameters
            .iter()
            .map(|p| {
                let t = self.without_aliases(model, ctx, &p.ty);
                simple_text(&self.through_imports(entry, t))
            })
            .collect();
        ((o.name.clone(), types), o.is_query)
    }

    /// `t` with each simple name that a single import of `entry` makes visible under another
    /// name replaced by the imported path.
    fn through_imports(&self, entry: usize, mut t: ir::TypeExpr) -> ir::TypeExpr {
        if let TypeName::Named(path) = &t.base
            && let [name] = &path[..]
        {
            let import = self.entries[entry].imports.iter().find(|i| {
                !i.wildcard
                    && !i.is_static
                    && i.visible_name() == Some(name.as_str())
                    && i.path.last() != Some(name)
            });
            if let Some(import) = import {
                t.base = TypeName::Named(import.path.clone());
            }
        }
        t.arguments = t.arguments.into_iter().map(|a| self.through_imports(entry, a)).collect();
        t
    }

    /// Brings operation `op` up to date with `o`, and with its `definition` outside the type
    /// where `o` leaves documentation or parameter names out.
    fn operation(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        op: Id,
        o: &'p Operation,
        definition: Option<&'p Operation>,
    ) -> Result<()> {
        // Template parameters first: parameter and return types may name them.
        self.template_parameters(tx, ctx, op, &o.template_parameters)?;
        let return_type = match &o.return_type {
            Some(t) => self.written_type_ref(tx, ctx, t, TypeKind::Class)?,
            None => None,
        };
        let documentation = match definition {
            Some(d) if o.documentation.is_empty() => &d.documentation,
            _ => &o.documentation,
        };
        let prefix = self.tag_prefix;
        tx.update(op, |data| {
            data.name = o.name.clone();
            data.visibility = o.visibility;
            data.documentation = documentation.clone();
            set_tags(data, &o.tags, prefix);
            if let ElementKind::Operation(p) = &mut data.kind {
                p.return_type = return_type;
                p.is_static = o.is_static;
                p.is_abstract = o.is_abstract;
                p.is_leaf = o.is_leaf;
                p.is_query = o.is_query;
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
            let name = match definition.and_then(|d| d.parameters.get(i)) {
                Some(d) if p.name.is_empty() => &d.name,
                _ => &p.name,
            };
            let type_ref = self.written_type_ref(tx, ctx, &p.ty, TypeKind::Class)?;
            let parameter = match existing.get(i) {
                Some(&id) => id,
                None => tx.create(
                    op,
                    ElementData::new(
                        name.clone(),
                        ElementKind::Parameter(ParameterProps::default()),
                    ),
                )?,
            };
            tx.update(parameter, |data| {
                data.name = name.clone();
                if let ElementKind::Parameter(props) = &mut data.kind {
                    props.type_ref = type_ref;
                    props.default_value = p.default.clone().unwrap_or_default();
                    props.direction = p.direction;
                }
            })?;
        }
        for &extra in existing.iter().skip(o.parameters.len()) {
            tx.delete(extra)?;
        }
        Ok(())
    }
}

/// Whether `path` from the root is a package of the model.
fn path_lookup_package(model: &Model, path: &[String]) -> bool {
    let mut at = model.root();
    for name in path {
        match child_package(model, at, name) {
            Some(p) => at = p,
            None => return false,
        }
    }
    true
}

/// Replaces the tagged values of this language (`prefix…`) with `tags`, keeping all others.
pub(crate) fn set_tags(data: &mut ElementData, tags: &[(String, String)], prefix: &str) {
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
        | ElementKind::DataType(p) => p.clone(),
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
            Member::Alias { target, .. } => walk(target, f),
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

/// An operation's name and parameter types, as text without package names, and whether it is
/// a query: to match an operation of the sources with one of the model.
type Signature = ((String, Vec<String>), bool);

fn simple_text(t: &ir::TypeExpr) -> String {
    let base = match &t.base {
        TypeName::Named(path) => path.last().cloned().unwrap_or_default(),
        TypeName::Builtin(name) => name.clone(),
        TypeName::Unwritten => String::new(),
    };
    let mut s = format!("{}{base}", t.prefix);
    if !t.arguments.is_empty() {
        let args: Vec<String> = t.arguments.iter().map(simple_text).collect();
        s.push_str(&format!("<{}>", args.join(",")));
    }
    s.push_str(&t.modifier);
    s
}

fn signature_of_model(model: &Model, op: Id) -> Signature {
    let element = model.get(op);
    let name = element.map(|e| e.name().to_owned()).unwrap_or_default();
    let query =
        element.is_some_and(|e| matches!(e.kind(), ElementKind::Operation(p) if p.is_query));
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
    ((name, types), query)
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
