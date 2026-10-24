//! Names as written in the sources resolved to model elements. Where a name stands for nothing
//! in the model, a placeholder is created, as Astah does: library types at their package path
//! (`java › util › List`), anything else under its qualified name or, for an unknown simple
//! name, in the root.
//!
//! A simple name is looked up in this order:
//! 1. template parameters of the operation, then of the type and its outer types;
//! 2. member types of the type, of its supertypes and of its outer types;
//! 3. single-type imports (unknown ones become placeholders at the imported path);
//! 4. the type's own package;
//! 5. on-demand imports (`.*`), in the model, then in the library;
//! 6. the implicit imports (`java.lang`);
//! 7. otherwise a placeholder in the root, Astah's default package.
//!
//! A qualified name is first looked up from the model's root; then, if it starts with a type
//! name, from that type; then in the library; otherwise it is split into package and type
//! names by the language's convention and created there.

use std::collections::HashSet;

use umlcraft_model::{
    ClassifierProps, ElementData, ElementKind, Id, Model, Result, TemplateParameterProps, Tx,
    TypeRef,
};
use umlcraft_notation::member::template_parameters;

use crate::ir::{self, TypeKind, TypeName};
use crate::merge::{Ctx, Importer};

/// Whether `kind` is a type that can hold members and nested types.
pub(crate) fn is_type(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

/// The model kind for a type of `kind`.
pub(crate) fn type_kind(kind: TypeKind, props: ClassifierProps) -> ElementKind {
    match kind {
        TypeKind::Class | TypeKind::Struct => ElementKind::Class(props),
        TypeKind::Interface => ElementKind::Interface(props),
        TypeKind::Enum => ElementKind::Enumeration(props),
    }
}

pub(crate) fn child_type(model: &Model, owner: Id, name: &str) -> Option<Id> {
    model
        .children(owner)
        .iter()
        .copied()
        .find(|&c| model.get(c).is_some_and(|e| e.name() == name && is_type(e.kind())))
}

pub(crate) fn child_package(model: &Model, owner: Id, name: &str) -> Option<Id> {
    model.children(owner).iter().copied().find(|&c| {
        model.get(c).is_some_and(|e| e.name() == name && matches!(e.kind(), ElementKind::Package))
    })
}

/// The package or type at `path` from the root.
fn path_lookup(model: &Model, path: &[String]) -> Option<Id> {
    path.iter().try_fold(model.root(), |at, name| {
        child_package(model, at, name).or_else(|| child_type(model, at, name))
    })
}

/// The package path and type path of a type: `java.util.Map.Entry` is
/// (`[java, util]`, `[Map, Entry]`).
fn qualified_parts(model: &Model, id: Id) -> (Vec<String>, Vec<String>) {
    let mut chain: Vec<Id> = model.ancestors(id);
    chain.reverse();
    chain.push(id);
    let (mut package, mut types) = (Vec::new(), Vec::new());
    for a in chain.into_iter().filter(|&a| a != model.root()) {
        let Some(e) = model.get(a) else { continue };
        if matches!(e.kind(), ElementKind::Package) && types.is_empty() {
            package.push(e.name().to_owned());
        } else {
            types.push(e.name().to_owned());
        }
    }
    (package, types)
}

fn named(model: &Model, ids: Vec<Id>, name: &str) -> Option<Id> {
    ids.into_iter().find(|&t| model.get(t).is_some_and(|t| t.name() == name))
}

impl<'p> Importer<'p> {
    /// A type as written, as the model's type reference: names linked, built-in types and
    /// whatever links nothing as text.
    pub(crate) fn type_ref(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        t: &ir::TypeExpr,
        hint: TypeKind,
    ) -> Result<TypeRef> {
        let base = match &t.base {
            TypeName::Builtin(name) => TypeRef::Text(name.clone()),
            TypeName::Named(path)
                if path.len() == 1 && self.rules.primitives.contains(&path[0].as_str()) =>
            {
                TypeRef::Text(path[0].clone())
            }
            TypeName::Named(path) => TypeRef::Element(self.resolve(tx, ctx, path, hint)?),
        };
        if t.arguments.is_empty() && t.prefix.is_empty() && t.modifier.is_empty() {
            return Ok(base);
        }
        let mut arguments = Vec::with_capacity(t.arguments.len());
        for a in &t.arguments {
            arguments.push(self.type_ref(tx, ctx, a, TypeKind::Class)?);
        }
        let expr = TypeRef::Expr(Box::new(umlcraft_model::TypeExpr {
            base,
            arguments,
            prefix: t.prefix.clone(),
            modifier: t.modifier.clone(),
        }));
        Ok(if expr.ids().is_empty() { TypeRef::Text(t.to_string()) } else { expr })
    }

    /// The element a type name stands for; a placeholder of kind `hint` if there is none.
    pub(crate) fn resolve(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        path: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        if let [name] = path {
            return self.resolve_simple(tx, ctx, name, hint);
        }
        if let Some(id) = path_lookup(tx.model(), path) {
            return Ok(id);
        }
        let split = (self.rules.split_qualified)(path);
        if split == 0 {
            // `Map.Entry`: the first name is a type in scope.
            let mut at = self.resolve_simple(tx, ctx, &path[0], TypeKind::Class)?;
            for (i, name) in path.iter().enumerate().skip(1) {
                at = match child_type(tx.model(), at, name) {
                    Some(id) => id,
                    None => {
                        // The rest of the path, one placeholder inside the other.
                        let (package, mut types) = qualified_parts(tx.model(), at);
                        types.extend(path[i..].iter().cloned());
                        return self.placeholder(tx, &package, &types, hint);
                    }
                };
            }
            return Ok(at);
        }
        // The longest package prefix the library knows the rest of.
        let library = self.rules.library;
        let known = (1..path.len()).rev().find(|&k| {
            library.is_package(&path[..k]) && library.get(&path[..k], &path[k..]).is_some()
        });
        let split = known.unwrap_or(split);
        self.placeholder(tx, &path[..split], &path[split..], hint)
    }

    fn resolve_simple(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        name: &str,
        hint: TypeKind,
    ) -> Result<Id> {
        let model = tx.model();
        // 1. Template parameters.
        if let Some(op) = ctx.operation
            && let Some(t) = named(model, template_parameters(model, op), name)
        {
            return Ok(t);
        }
        let chain = self.chain(ctx.entry);
        for &e in &chain {
            if let Some(t) = named(model, template_parameters(model, self.entries[e].id), name) {
                return Ok(t);
            }
        }
        // 2. Member types, also those inherited.
        for &e in &chain {
            let mut seen = HashSet::new();
            if let Some(t) = self.member_type(model, self.entries[e].id, name, &mut seen) {
                return Ok(t);
            }
        }
        let entry = &self.entries[ctx.entry];
        let (namespace, imports) = (entry.namespace, entry.imports.clone());
        // 3. Single-type imports.
        let single = imports.iter().find(|i| {
            !i.wildcard && !i.is_static && i.path.last().is_some_and(|last| last == name)
        });
        if let Some(import) = single {
            let path = import.path.clone();
            return self.resolve(tx, ctx, &path, hint);
        }
        // 4. The own package.
        let model = tx.model();
        if let Some(t) = child_type(model, namespace, name) {
            return Ok(t);
        }
        // 5. On-demand imports, then 6. the implicit ones.
        let on_demand =
            imports.iter().filter(|i| i.wildcard && !i.is_static).map(|i| i.path.clone()).chain(
                self.rules
                    .implicit_imports
                    .iter()
                    .map(|p| p.iter().map(|s| s.to_string()).collect()),
            );
        let on_demand: Vec<Vec<String>> = on_demand.collect();
        for package in &on_demand {
            if let Some(t) = path_lookup(model, package).and_then(|p| child_type(model, p, name)) {
                return Ok(t);
            }
        }
        let library = self.rules.library;
        let name = name.to_owned();
        for package in &on_demand {
            let split = if library.is_package(package) {
                package.len()
            } else {
                (self.rules.split_qualified)(package)
            };
            let (pkg, mut types) = (package[..split].to_vec(), package[split..].to_vec());
            types.push(name.clone());
            if library.get(&pkg, &types).is_some() {
                return self.placeholder(tx, &pkg, &types, hint);
            }
        }
        // 7. A placeholder in the root (Astah's default package).
        let root = tx.model().root();
        self.placeholder_under(tx, root, &[name], hint)
    }

    /// The entry and its outer entries, innermost first.
    fn chain(&self, entry: usize) -> Vec<usize> {
        std::iter::successors(Some(entry), |&e| self.entries[e].outer).collect()
    }

    /// A type nested in `class` or in one of its supertypes.
    fn member_type(
        &self,
        model: &Model,
        class: Id,
        name: &str,
        seen: &mut HashSet<Id>,
    ) -> Option<Id> {
        if !seen.insert(class) {
            return None;
        }
        child_type(model, class, name).or_else(|| {
            self.supertypes
                .get(&class)
                .into_iter()
                .flatten()
                .find_map(|&s| self.member_type(model, s, name, seen))
        })
    }

    /// The placeholder for `types` in package `package`, created from the root.
    pub(crate) fn placeholder(
        &mut self,
        tx: &mut Tx<'_>,
        package: &[String],
        types: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        let mut at = tx.model().root();
        for name in package {
            at = match child_package(tx.model(), at, name) {
                Some(p) => p,
                None => {
                    self.report.placeholders += 1;
                    tx.create(at, ElementData::new(name.clone(), ElementKind::Package))?
                }
            };
        }
        let (package_path, at) = (package.to_vec(), at);
        self.nested_placeholder(tx, at, &package_path, &[], types, hint)
    }

    /// A placeholder for `types` under `owner`, a package of unknown path.
    fn placeholder_under(
        &mut self,
        tx: &mut Tx<'_>,
        owner: Id,
        types: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        let (package, outer) = qualified_parts(tx.model(), owner);
        self.nested_placeholder(tx, owner, &package, &outer, types, hint)
    }

    /// Finds or creates `types` one inside the other under `owner`, which is the package
    /// `package` (or the type `outer` in it). Kinds and template parameters come from the
    /// library where it knows the type; the last one is of kind `hint` otherwise.
    fn nested_placeholder(
        &mut self,
        tx: &mut Tx<'_>,
        owner: Id,
        package: &[String],
        outer: &[String],
        types: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        let mut at = owner;
        let mut path = outer.to_vec();
        for (i, name) in types.iter().enumerate() {
            path.push(name.clone());
            if let Some(t) = child_type(tx.model(), at, name) {
                at = t;
                continue;
            }
            let known = self.rules.library.get(package, &path).cloned();
            let kind = known
                .as_ref()
                .map_or(if i + 1 == types.len() { hint } else { TypeKind::Class }, |k| k.kind);
            let data = ElementData::new(name.clone(), type_kind(kind, ClassifierProps::default()));
            let id = tx.create(at, data)?;
            for parameter in known.iter().flat_map(|k| &k.parameters) {
                let kind = ElementKind::TemplateParameter(TemplateParameterProps::default());
                tx.create(id, ElementData::new(parameter.clone(), kind))?;
            }
            self.report.placeholders += 1;
            self.placeholders.insert(id);
            at = id;
        }
        Ok(at)
    }

    /// The library's description of a type in the model, if it is a library placeholder.
    pub(crate) fn library_type(
        &self,
        model: &Model,
        id: Id,
    ) -> Option<&'static crate::lang::LibraryType> {
        let (package, types) = qualified_parts(model, id);
        self.rules.library.get(&package, &types)
    }

    /// Whether a type lives in a package whose types are values, never association ends.
    pub(crate) fn is_value_type(&self, model: &Model, id: Id) -> bool {
        let (package, _) = qualified_parts(model, id);
        self.rules.value_packages.iter().any(|p| p.iter().eq(package.iter()))
    }
}
