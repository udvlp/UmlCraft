//! Names as written in the sources resolved to model elements. Where a name stands for nothing
//! in the model, a placeholder is created, as Astah does: library types at their package path
//! (`java › util › List`), anything else under its qualified name or, for an unknown simple
//! name, in the root.
//!
//! A simple name is looked up in this order:
//! 1. template parameters of the operation, then of the type and its outer types;
//! 2. member types of the type, of its supertypes and of its outer types;
//! 3. single-type imports (unknown ones become placeholders at the imported path), aliases
//!    by the name they give;
//! 4. the type's own package (with `Rules::enclosing_namespaces`, then the packages around it,
//!    outwards, as C# does);
//! 5. on-demand imports (`.*`), in the model, then in the library;
//! 6. the implicit imports (`java.lang`);
//! 7. otherwise a placeholder in the root, Astah's default package.
//!
//! A simple name in a type as written may first be an alias (`typedef`), replaced by its target
//! ([`Importer::type_ref`]).
//!
//! A qualified name whose first segment a single import makes visible stands for the import's
//! path followed by the rest (`so.Order` after `import shop.orders as so`). It is first looked
//! up from the model's root (with `Rules::enclosing_namespaces`, first from the type's package
//! and the ones around it); then, if it starts with a type name, from that type; then in the
//! library; otherwise it is split into package and type names by the language's convention and
//! created there. Such a lookup ends at a type, never at a package. The qualified path of an
//! import itself is looked up without the imports ([`Importer::resolve_import`]).

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
        .children_named(owner, name)
        .into_iter()
        .find(|&c| model.get(c).is_some_and(|e| is_type(e.kind())))
}

pub(crate) fn child_package(model: &Model, owner: Id, name: &str) -> Option<Id> {
    model
        .children_named(owner, name)
        .into_iter()
        .find(|&c| model.get(c).is_some_and(|e| matches!(e.kind(), ElementKind::Package)))
}

/// Finds or creates the packages of `path` under `at`, one inside the other: the innermost, and
/// how many were created.
pub(crate) fn packages(tx: &mut Tx<'_>, mut at: Id, path: &[String]) -> Result<(Id, usize)> {
    let mut created = 0;
    for name in path {
        at = match child_package(tx.model(), at, name) {
            Some(p) => p,
            None => {
                created += 1;
                tx.create(at, ElementData::new(name.clone(), ElementKind::Package))?
            }
        };
    }
    Ok((at, created))
}

/// The package or type at `path` from the root.
fn path_lookup(model: &Model, path: &[String]) -> Option<Id> {
    path_lookup_from(model, model.root(), path)
}

/// The package or type at `path` from `start`.
fn path_lookup_from(model: &Model, start: Id, path: &[String]) -> Option<Id> {
    path.iter().try_fold(start, |at, name| {
        child_package(model, at, name).or_else(|| child_type(model, at, name))
    })
}

/// The type at `path` from `start`, through packages or types: never a package, which a name
/// written as a type may also stand for (`x: os.path`, a module).
fn type_lookup_from(model: &Model, start: Id, path: &[String]) -> Option<Id> {
    let (name, outer) = path.split_last()?;
    child_type(model, path_lookup_from(model, start, outer)?, name)
}

/// `namespace` and the packages around it, innermost first, without the root.
fn enclosing(model: &Model, namespace: Id) -> Vec<Id> {
    std::iter::once(namespace)
        .chain(model.ancestors(namespace))
        .filter(|&n| n != model.root())
        .collect()
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
    /// A type as written, as the model's type reference, or none where no type is written.
    pub(crate) fn written_type_ref(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        t: &ir::TypeExpr,
        hint: TypeKind,
    ) -> Result<Option<TypeRef>> {
        // `*args` without a type keeps its modifier as text.
        if t.is_unwritten() {
            return Ok((!t.modifier.is_empty()).then(|| TypeRef::Text(t.modifier.clone())));
        }
        self.type_ref(tx, ctx, t, hint).map(Some)
    }

    /// A type as written, as the model's type reference: names linked, built-in types and
    /// whatever links nothing as text.
    pub(crate) fn type_ref(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        t: &ir::TypeExpr,
        hint: TypeKind,
    ) -> Result<TypeRef> {
        if let Some((name, expanded)) = self.alias_target(tx.model(), ctx, t, &self.expanding) {
            self.expanding.push(name);
            let found = self.type_ref(tx, ctx, &expanded, hint);
            self.expanding.pop();
            return found;
        }
        let base = match &t.base {
            TypeName::Builtin(name) => TypeRef::Text(name.clone()),
            // Only in type arguments, which no language without types writes.
            TypeName::Unwritten => TypeRef::Text(String::new()),
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

    /// `t` with the alias it names replaced by the alias's target (`Ptr*` is
    /// `shared_ptr<Order>*` after `using Ptr = shared_ptr<Order>;`), and the alias's name. An
    /// alias counts where it is declared in the type, its outer types or the namespaces around
    /// them; its target resolves where it is used. Aliases that are already being replaced
    /// (`typedef struct Node Node;`, cycles) stand for themselves.
    fn alias_target(
        &self,
        model: &Model,
        ctx: Ctx,
        t: &ir::TypeExpr,
        expanding: &[&'p str],
    ) -> Option<(&'p str, ir::TypeExpr)> {
        if self.aliases.is_empty() {
            return None;
        }
        let TypeName::Named(path) = &t.base else { return None };
        let [name] = path.as_slice() else { return None };
        if !t.arguments.is_empty() || expanding.contains(&name.as_str()) {
            return None;
        }
        let entry = &self.entries[ctx.entry];
        let scopes = self
            .chain(ctx.entry)
            .into_iter()
            .map(|e| self.entries[e].id)
            .chain(enclosing(model, entry.namespace))
            .chain([model.root()]);
        let (&name, &target) = scopes
            .filter_map(|s| self.aliases.get(&s))
            .find_map(|aliases| aliases.get_key_value(name.as_str()))?;
        let expanded = ir::TypeExpr {
            prefix: format!("{}{}", t.prefix, target.prefix),
            base: target.base.clone(),
            arguments: target.arguments.clone(),
            modifier: format!("{}{}", target.modifier, t.modifier),
        };
        Some((name, expanded))
    }

    /// With `Rules::reexports`, what `path` stands for through a package that imports one of its
    /// segments in its own scope: `shop.Order` is `shop.orders.Order` after
    /// `from .orders import Order` (or `import *`) in `shop/__init__.py`. Also the package and
    /// the name it re-exports, which are not followed again while they are being followed
    /// (`from .a import b` and `from .b import a` in one package).
    fn reexported(&self, model: &Model, path: &[String]) -> Option<((Id, String), Vec<String>)> {
        if !self.rules.reexports {
            return None;
        }
        (1..path.len()).rev().find_map(|k| {
            let package = path_lookup(model, &path[..k]).filter(|&p| {
                model.get(p).is_some_and(|e| matches!(e.kind(), ElementKind::Package))
            })?;
            let imports = self.namespace_imports.get(&package)?;
            let name = path[k].as_str();
            if self.reexporting.iter().any(|(p, n)| *p == package && n == name) {
                return None;
            }
            let found = imports.iter().filter(|i| !i.is_static).find_map(|i| {
                // `shop.repair.Setting` after `from .repair import repair` is in the module
                // `repair`, not in what the module defines under its own name.
                let through_itself = i.path.starts_with(&path[..=k]) && k + 1 < path.len();
                if !i.wildcard && i.visible_name() == Some(name) && !through_itself {
                    Some(i.path.clone())
                } else if i.wildcard
                    && path_lookup(model, &i.path)
                        .is_some_and(|p| child_type(model, p, name).is_some())
                {
                    Some(i.path.iter().cloned().chain([name.to_owned()]).collect())
                } else {
                    None
                }
            })?;
            let target: Vec<String> =
                found.into_iter().chain(path[k + 1..].iter().cloned()).collect();
            (target != path).then_some(((package, name.to_owned()), target))
        })
    }

    /// `t` with its aliases replaced by their targets as [`Self::type_ref`] replaces them, without
    /// resolving anything: to compare signatures with the model's.
    pub(crate) fn without_aliases(
        &self,
        model: &Model,
        ctx: Ctx,
        t: &ir::TypeExpr,
    ) -> ir::TypeExpr {
        self.expand(model, ctx, t, &mut self.expanding.clone())
    }

    fn expand(
        &self,
        model: &Model,
        ctx: Ctx,
        t: &ir::TypeExpr,
        expanding: &mut Vec<&'p str>,
    ) -> ir::TypeExpr {
        let depth = expanding.len();
        let mut t = t.clone();
        while let Some((name, expanded)) = self.alias_target(model, ctx, &t, expanding) {
            expanding.push(name);
            t = expanded;
        }
        t.arguments = t.arguments.iter().map(|a| self.expand(model, ctx, a, expanding)).collect();
        expanding.truncate(depth);
        t
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
        // `so.Order` after `import shop.orders as so`: the import's path, then the rest. Once
        // only, as an import's path does not go through other imports.
        let imports = &self.entries[ctx.entry].imports;
        let single = imports
            .iter()
            .find(|i| !i.wildcard && !i.is_static && i.visible_name() == Some(path[0].as_str()));
        if let Some(import) = single {
            let full: Vec<String> = import.path.iter().chain(&path[1..]).cloned().collect();
            return self.resolve_qualified(tx, ctx, &full, hint);
        }
        self.resolve_qualified(tx, ctx, path, hint)
    }

    /// The element the path of a single import names. A qualified path does not go through
    /// the imports: `from socket import AddressFamily, socket` imports `socket.AddressFamily`,
    /// not `socket.socket.AddressFamily`. A simple one may name another import (C#'s
    /// `using A = B;`).
    pub(crate) fn resolve_import(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        path: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        if path.len() == 1 {
            self.resolve(tx, ctx, path, hint)
        } else {
            self.resolve_qualified(tx, ctx, path, hint)
        }
    }

    /// A qualified name, imports left aside.
    fn resolve_qualified(
        &mut self,
        tx: &mut Tx<'_>,
        ctx: Ctx,
        path: &[String],
        hint: TypeKind,
    ) -> Result<Id> {
        if self.rules.enclosing_namespaces {
            let model = tx.model();
            let namespace = self.entries[ctx.entry].namespace;
            let found = enclosing(model, namespace)
                .into_iter()
                .find_map(|n| type_lookup_from(model, n, path));
            if let Some(id) = found {
                return Ok(id);
            }
        }
        if let Some(id) = type_lookup_from(tx.model(), tx.model().root(), path) {
            return Ok(id);
        }
        if let Some((reexport, target)) = self.reexported(tx.model(), path) {
            self.reexporting.push(reexport);
            let found = self.resolve_qualified(tx, ctx, &target, hint);
            self.reexporting.pop();
            return found;
        }
        let split = (self.rules.split_qualified)(path);
        // `Map.Entry`: the first name is a type in scope. Where the convention cannot tell, a
        // type the model has in scope counts.
        let outer = if split == 0 {
            Some(self.resolve_simple(tx, ctx, &path[0], TypeKind::Class)?)
        } else if self.rules.enclosing_namespaces {
            self.type_in_scope(tx.model(), ctx, &path[0])
        } else {
            None
        };
        if let Some(mut at) = outer {
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
        for e in self.chain(ctx.entry) {
            if let Some(t) = named(model, template_parameters(model, self.entries[e].id), name) {
                return Ok(t);
            }
        }
        // 2. Member types, also those inherited.
        if let Some(t) = self.member_type_around(model, ctx.entry, name) {
            return Ok(t);
        }
        let entry = &self.entries[ctx.entry];
        let (namespace, imports) = (entry.namespace, entry.imports.clone());
        // 3. Single-type imports.
        // `import shop` names the module `shop`, no type.
        // Not again for a name whose import is being followed now: `using A = B; using B = A;`.
        let single = imports.iter().find(|i| {
            !i.wildcard
                && !i.is_static
                && i.visible_name() == Some(name)
                && i.path != [name]
                && !self.importing.iter().any(|n| n == name)
        });
        if let Some(import) = single {
            let path = import.path.clone();
            self.importing.push(name.to_owned());
            let result = self.resolve_import(tx, ctx, &path, hint);
            self.importing.pop();
            return result;
        }
        // 4. The own package, and where the language says so the ones around it up to the
        // global one, whose types are found here instead of by scanning for a placeholder.
        let model = tx.model();
        let namespaces = if self.rules.enclosing_namespaces {
            let mut around = enclosing(model, namespace);
            around.push(model.root());
            around
        } else {
            vec![namespace]
        };
        if let Some(t) = namespaces.into_iter().find_map(|n| child_type(model, n, name)) {
            return Ok(t);
        }
        // 5. On-demand imports, then 6. the implicit ones.
        let implicit = self.implicit_imports.clone();
        let on_demand: Vec<&[String]> = imports
            .iter()
            .filter(|i| i.wildcard && !i.is_static)
            .map(|i| i.path.as_slice())
            .chain(implicit.iter().map(Vec::as_slice))
            .collect();
        // Library types are left to the library's turn: whether their placeholder exists yet
        // depends on the order of the files.
        for package in &on_demand {
            let found = self
                .import_target(model, namespace, package)
                .and_then(|p| child_type(model, p, name));
            if let Some(t) = found.filter(|&t| self.library_type(model, t).is_none()) {
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

    /// The type of the model a simple name stands for in `ctx`, without creating anything:
    /// member types, single-type imports, the packages around and on-demand imports.
    fn type_in_scope(&self, model: &Model, ctx: Ctx, name: &str) -> Option<Id> {
        if let Some(t) = self.member_type_around(model, ctx.entry, name) {
            return Some(t);
        }
        let entry = &self.entries[ctx.entry];
        let imports = entry.imports.iter().filter(|i| !i.is_static);
        let single = imports.clone().filter(|i| !i.wildcard && i.visible_name() == Some(name));
        let target = |i: &ir::Import| self.import_target(model, entry.namespace, &i.path);
        if let Some(t) = single.filter_map(|i| target(i)).next() {
            return Some(t);
        }
        let namespaces = enclosing(model, entry.namespace);
        if let Some(t) = namespaces.into_iter().find_map(|n| child_type(model, n, name)) {
            return Some(t);
        }
        imports
            .filter(|i| i.wildcard)
            .find_map(|i| target(i).and_then(|p| child_type(model, p, name)))
    }

    /// The type a member defined outside it belongs to (`owner` as written, `A` in
    /// `void A::f() {}`), only looked up: from the definition's namespace (with
    /// `Rules::enclosing_namespaces`, also the ones around it), from the root, then through
    /// the imports.
    pub(crate) fn definition_owner(
        &self,
        model: &Model,
        namespace: Id,
        imports: &[&ir::Import],
        owner: &[String],
    ) -> Option<Id> {
        let is_type_id = |id: &Id| model.get(*id).is_some_and(|e| is_type(e.kind()));
        let namespaces = if self.rules.enclosing_namespaces {
            enclosing(model, namespace)
        } else {
            vec![namespace]
        };
        let found = namespaces
            .into_iter()
            .chain([model.root()])
            .find_map(|n| path_lookup_from(model, n, owner).filter(is_type_id));
        if found.is_some() {
            return found;
        }
        let (first, rest) = owner.split_first()?;
        imports.iter().filter(|i| !i.is_static).find_map(|i| {
            let start = if i.wildcard {
                child_type(model, self.import_target(model, namespace, &i.path)?, first)?
            } else if i.visible_name() == Some(first.as_str()) {
                self.import_target(model, namespace, &i.path)?
            } else {
                return None;
            };
            path_lookup_from(model, start, rest).filter(is_type_id)
        })
    }

    /// The package or type an import's path names. With `Rules::enclosing_namespaces`, the path
    /// is first taken relative to `namespace` and the ones around it (C++'s `using namespace
    /// lib;` inside `acme` for `acme::lib`), then from the root.
    fn import_target(&self, model: &Model, namespace: Id, path: &[String]) -> Option<Id> {
        let relative = if self.rules.enclosing_namespaces {
            enclosing(model, namespace).into_iter().find_map(|n| path_lookup_from(model, n, path))
        } else {
            None
        };
        relative.or_else(|| path_lookup(model, path))
    }

    /// Whether `path` names a package from `namespace`, the ones around it or the root.
    pub(crate) fn is_namespace(&self, model: &Model, namespace: Id, path: &[String]) -> bool {
        enclosing(model, namespace).into_iter().chain([model.root()]).any(|n| {
            path_lookup_from(model, n, path).is_some_and(|p| {
                model.get(p).is_some_and(|e| matches!(e.kind(), ElementKind::Package))
            })
        })
    }

    /// The entry and its outer entries, innermost first.
    pub(crate) fn chain(&self, entry: usize) -> Vec<usize> {
        std::iter::successors(Some(entry), |&e| self.entries[e].outer).collect()
    }

    /// A type nested in the type of `entry`, in one of its outer types or in one of their
    /// supertypes, innermost first.
    fn member_type_around(&self, model: &Model, entry: usize, name: &str) -> Option<Id> {
        self.chain(entry)
            .into_iter()
            .find_map(|e| self.member_type(model, self.entries[e].id, name, &mut HashSet::new()))
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
        let root = tx.model().root();
        let (at, created) = packages(tx, root, package)?;
        self.report.placeholders += created;
        self.nested_placeholder(tx, at, package, &[], types, hint)
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
