//! C#: declarations from `.cs` files.
//!
//! - Namespaces (block or file-scoped) become nested namespaces; `using N;` imports a namespace
//!   on demand, `using A = N.T;` a type (or namespace) under another name.
//! - Types without a visibility are internal (package) in a namespace and private when nested;
//!   members are private, in interfaces public and abstract unless they have a body or are
//!   static. `protected internal` is protected, `private protected` and `file` private.
//! - `sealed` makes types and members leaves; `const` fields are static and read-only.
//! - Structs and record structs are structs, records classes; the parameters of a positional
//!   record are public attributes, read-only unless it is a mutable record struct. A class's
//!   primary constructor becomes a constructor.
//! - Base lists: C# puts the base class first, so every entry after the first is an interface,
//!   and the first one too if it is named `I` followed by a capital letter (`IDisposable`); in
//!   interfaces and structs every entry is one. The mapping corrects the guess for types it knows
//!   (`Rules::base_list_by_target`).
//! - Auto-properties become attributes, read-only without `set`. Properties with bodies become
//!   accessor operations as .NET names them (`get_Count`, `set_Count(value)`), indexers
//!   `get_Item`/`set_Item`; properties without bodies in interfaces or declared `abstract` become
//!   abstract accessor operations.
//! - `out` parameters are out, `ref` parameters inout.
//! - A declaration's documentation is the run of `///` comments right before it, or a `/** */`
//!   comment.
//! - Of `#if` … `#elif` … `#else`, only the first branch is read; nothing is evaluated. The
//!   other branches are blanked out before parsing, so conditionals may cut declarations apart.
//! - Attributes, events, delegates, enum member values and aliases of constructed types are left
//!   out and counted, as are method bodies and top-level statements.

pub(crate) mod rules;

use tree_sitter::Node;
use umlcraft_model::{ParameterDirection, Visibility};

use crate::ir::{
    Field, Import, Item, Literal, Member, Operation, Param, Relation, Scope, Supertype,
    TemplateParam, TypeDecl, TypeExpr, TypeKind, TypeName,
};
use crate::lang::{Frontend, Language, Skip, Skipped, children, named_children, squeeze};

pub struct CSharp;

impl Frontend for CSharp {
    fn lower(&self, root: Node, text: &str, _module: &[String], skipped: &mut Skipped) -> Scope {
        let mut reader = Reader { src: text, skipped, lang: CSharp };
        reader.scope(&named_children(root))
    }
}

impl Language for CSharp {
    const LANGUAGE: crate::SourceLanguage = crate::SourceLanguage::CSharp;
}

type Reader<'a> = crate::lang::Reader<'a, CSharp>;

/// Where a declaration sits; it decides the implicit modifiers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Namespace,
    Class,
    Interface,
}

/// The modifiers of a declaration.
#[derive(Default)]
struct Modifiers {
    visibility: Option<Visibility>,
    is_static: bool,
    is_abstract: bool,
    is_sealed: bool,
    is_partial: bool,
    is_const: bool,
    is_readonly: bool,
}

/// An accessor of a property or indexer.
struct Accessor {
    /// `get`, `set` or `init`.
    kind: String,
    visibility: Option<Visibility>,
    has_body: bool,
    span: std::ops::Range<usize>,
}

/// The node right after the `=` among `node`'s children: an initializer or default value.
fn after_equals(node: Node) -> Option<Node> {
    let parts = children(node);
    let at = parts.iter().position(|n| n.kind() == "=")?;
    parts[at + 1..].iter().copied().find(|n| n.is_named())
}

/// Whether a base class name follows the .NET convention for interfaces: `I` and a capital.
fn is_interface_name(ty: &TypeExpr) -> bool {
    let TypeName::Named(path) = &ty.base else { return false };
    let mut chars = path.last().map(|s| s.chars()).into_iter().flatten();
    chars.next() == Some('I') && chars.next().is_some_and(char::is_uppercase)
}

impl Reader<'_> {
    /// The declarations among `nodes`, the children of a file or a namespace body.
    fn scope(&mut self, nodes: &[Node]) -> Scope {
        let mut scope = Scope::default();
        for (i, &node) in nodes.iter().enumerate() {
            match node.kind() {
                "using_directive" => {
                    if let Some(import) = self.using(node) {
                        scope.imports.push(import);
                    }
                }
                "namespace_declaration" => {
                    let path = self.namespace_path(node);
                    let body = node.child_by_field_name("body");
                    let inner = body.map(|b| self.scope(&named_children(b))).unwrap_or_default();
                    scope.items.push(Item::Namespace { path, scope: inner });
                }
                // `namespace A.B;`: everything after it is in the namespace.
                "file_scoped_namespace_declaration" => {
                    let path = self.namespace_path(node);
                    let inner = self.scope(&nodes[i + 1..]);
                    scope.items.push(Item::Namespace { path, scope: inner });
                    break;
                }
                "global_statement" => self.count(Skip::FreeMember),
                "delegate_declaration" => self.count(Skip::Delegate),
                // `[assembly: …]`
                "attribute_list" => self.count_attributes(node),
                _ => {
                    if let Some(t) = self.type_decl(node, Context::Namespace) {
                        scope.items.push(Item::Type(t));
                    }
                }
            }
        }
        scope
    }

    fn namespace_path(&mut self, namespace: Node) -> Vec<String> {
        namespace.child_by_field_name("name").map(|n| self.path(n)).unwrap_or_default()
    }

    /// `A.B.C` as `["A", "B", "C"]`.
    fn path(&mut self, node: Node) -> Vec<String> {
        match self.type_expr(node) {
            Some(TypeExpr { base: TypeName::Named(path), .. }) => path,
            _ => Vec::new(),
        }
    }

    fn using(&mut self, node: Node) -> Option<Import> {
        let is_static = children(node).iter().any(|n| n.kind() == "static");
        let alias = node.child_by_field_name("name");
        let target = named_children(node).into_iter().rfind(|&n| Some(n) != alias)?;
        let ty = self.type_expr(target)?;
        let TypeName::Named(path) = ty.base else { return None };
        if !ty.arguments.is_empty() {
            self.count(Skip::GenericAlias);
            return None;
        }
        let alias = alias.map(|a| self.text(a).to_owned());
        Some(Import { path, wildcard: alias.is_none(), is_static, alias })
    }

    fn count_attributes(&mut self, list: Node) {
        let n = named_children(list).iter().filter(|n| n.kind() == "attribute").count();
        *self.skipped.entry(Skip::Attribute).or_default() += n;
    }

    /// The modifiers of a declaration; its attributes are counted.
    fn modifiers(&mut self, decl: Node) -> Modifiers {
        let mut m = Modifiers::default();
        let mut protected = false;
        let mut internal = false;
        let mut private = false;
        for child in named_children(decl) {
            match child.kind() {
                "attribute_list" => self.count_attributes(child),
                "modifier" => match self.text(child) {
                    "public" => m.visibility = Some(Visibility::Public),
                    "protected" => protected = true,
                    "internal" => internal = true,
                    "private" | "file" => private = true,
                    "static" => m.is_static = true,
                    "abstract" => m.is_abstract = true,
                    "sealed" => m.is_sealed = true,
                    "partial" => m.is_partial = true,
                    "const" => m.is_const = true,
                    "readonly" => m.is_readonly = true,
                    _ => {}
                },
                _ => {}
            }
        }
        // `private protected` is private, `protected internal` protected.
        if m.visibility.is_none() {
            m.visibility = if private {
                Some(Visibility::Private)
            } else if protected {
                Some(Visibility::Protected)
            } else if internal {
                Some(Visibility::Package)
            } else {
                None
            };
        }
        m
    }

    fn type_decl(&mut self, node: Node, context: Context) -> Option<TypeDecl> {
        let is_record_struct = node.kind() == "record_declaration"
            && children(node).iter().any(|n| n.kind() == "struct");
        let kind = match node.kind() {
            "class_declaration" => TypeKind::Class,
            "record_declaration" if is_record_struct => TypeKind::Struct,
            "record_declaration" => TypeKind::Class,
            "struct_declaration" => TypeKind::Struct,
            "interface_declaration" => TypeKind::Interface,
            "enum_declaration" => TypeKind::Enum,
            _ => return None,
        };
        let name = self.text(node.child_by_field_name("name")?);
        let mut t = TypeDecl::new(name, kind);
        t.line = node.start_position().row + 1;
        t.documentation = self.documentation(node);
        let m = self.modifiers(node);
        let implicit =
            if context == Context::Namespace { Visibility::Package } else { Visibility::Private };
        t.visibility = m.visibility.unwrap_or(implicit);
        t.is_abstract = m.is_abstract;
        t.is_leaf = m.is_sealed;
        t.is_static = m.is_static;
        t.is_partial = m.is_partial;
        let parts = named_children(node);
        if let Some(params) = parts.iter().find(|n| n.kind() == "type_parameter_list") {
            t.template_parameters = self.type_parameters(*params);
        }
        self.constraints(node, &mut t.template_parameters);
        // An enum's base is the type of its values, no supertype.
        if kind != TypeKind::Enum
            && let Some(bases) = parts.iter().find(|n| n.kind() == "base_list")
        {
            self.bases(*bases, &mut t);
        }
        if let Some(&list) = parts.iter().find(|n| n.kind() == "parameter_list") {
            let params = self.parameters(list);
            if node.kind() == "record_declaration" {
                // Positional records: public properties, `init` only unless a mutable record
                // struct.
                let is_read_only = !is_record_struct || m.is_readonly;
                for p in params {
                    t.members.push(Member::Field(Field {
                        name: p.name,
                        ty: p.ty,
                        visibility: Visibility::Public,
                        is_read_only,
                        ..Default::default()
                    }));
                }
            } else {
                t.members.push(Member::Operation(Operation {
                    name: t.name.clone(),
                    parameters: params,
                    visibility: Visibility::Public,
                    is_constructor: true,
                    span: list.byte_range(),
                    ..Default::default()
                }));
            }
        }
        let inner = if kind == TypeKind::Interface { Context::Interface } else { Context::Class };
        if let Some(body) = node.child_by_field_name("body") {
            self.body(&named_children(body), inner, &mut t.members);
        }
        Some(t)
    }

    /// The supertypes of `t` from its base list, with the relation C#'s rules suggest.
    fn bases(&mut self, list: Node, t: &mut TypeDecl) {
        let types: Vec<TypeExpr> = named_children(list)
            .into_iter()
            .filter_map(|n| match n.kind() {
                // `class B(int x) : A(x)`: the base with the arguments of its constructor.
                "primary_constructor_base_type" => {
                    named_children(n).into_iter().find_map(|c| self.type_expr(c))
                }
                _ => self.type_expr(n),
            })
            .collect();
        for (i, ty) in types.into_iter().enumerate() {
            let relation = match t.kind {
                // An interface's bases are interfaces it generalizes.
                TypeKind::Interface => Relation::Extends,
                TypeKind::Struct => Relation::Implements,
                _ if i > 0 || is_interface_name(&ty) => Relation::Implements,
                _ => Relation::Extends,
            };
            t.supertypes.push(Supertype { ty, relation, access: None, is_virtual: false });
        }
    }

    fn body(&mut self, nodes: &[Node], context: Context, members: &mut Vec<Member>) {
        for &node in nodes {
            match node.kind() {
                "enum_member_declaration" => {
                    let Some(name) = node.child_by_field_name("name") else { continue };
                    let attributes = named_children(node);
                    for list in attributes.into_iter().filter(|n| n.kind() == "attribute_list") {
                        self.count_attributes(list);
                    }
                    if node.child_by_field_name("value").is_some() {
                        self.count(Skip::EnumValue);
                    }
                    members.push(Member::Literal(Literal {
                        name: self.text(name).to_owned(),
                        documentation: self.documentation(node),
                    }));
                }
                "field_declaration" => {
                    members.extend(self.fields(node, context).into_iter().map(Member::Field));
                }
                "property_declaration" | "indexer_declaration" => {
                    self.property(node, context, members);
                }
                "method_declaration"
                | "constructor_declaration"
                | "destructor_declaration"
                | "operator_declaration"
                | "conversion_operator_declaration" => {
                    if let Some(o) = self.operation(node, context) {
                        members.push(Member::Operation(o));
                    }
                }
                "event_field_declaration" | "event_declaration" => self.count(Skip::Event),
                "delegate_declaration" => self.count(Skip::Delegate),
                _ => {
                    if let Some(t) = self.type_decl(node, context) {
                        members.push(Member::Type(t));
                    }
                }
            }
        }
    }

    fn implicit_visibility(context: Context) -> Visibility {
        if context == Context::Interface { Visibility::Public } else { Visibility::Private }
    }

    /// One field per declarator: `int a = 1, b;` is `a : int = 1` and `b : int`.
    fn fields(&mut self, node: Node, context: Context) -> Vec<Field> {
        let documentation = self.documentation(node);
        let m = self.modifiers(node);
        let Some(declaration) =
            named_children(node).into_iter().find(|n| n.kind() == "variable_declaration")
        else {
            return Vec::new();
        };
        let Some(ty) = declaration.child_by_field_name("type").and_then(|t| self.type_expr(t))
        else {
            return Vec::new();
        };
        let visibility = m.visibility.unwrap_or(Self::implicit_visibility(context));
        named_children(declaration)
            .into_iter()
            .filter(|d| d.kind() == "variable_declarator")
            .filter_map(|d| {
                let name = self.text(d.child_by_field_name("name")?).to_owned();
                let default = after_equals(d).map(|v| self.text(v).to_owned());
                Some(Field {
                    name,
                    ty: ty.clone(),
                    visibility,
                    is_static: m.is_static || m.is_const,
                    is_read_only: m.is_readonly || m.is_const,
                    default,
                    documentation: documentation.clone(),
                    ..Default::default()
                })
            })
            .collect()
    }

    /// A property or indexer: an attribute if it is an auto-property, else its accessors as
    /// operations.
    fn property(&mut self, node: Node, context: Context, members: &mut Vec<Member>) {
        let documentation = self.documentation(node);
        let m = self.modifiers(node);
        let Some(ty) = node.child_by_field_name("type").and_then(|t| self.type_expr(t)) else {
            return;
        };
        let is_indexer = node.kind() == "indexer_declaration";
        let name = match node.child_by_field_name("name") {
            Some(n) => self.text(n).to_owned(),
            None if is_indexer => "Item".to_owned(),
            None => return,
        };
        let visibility = m.visibility.unwrap_or(Self::implicit_visibility(context));
        let value = node.child_by_field_name("value");
        // `int Count => …;`: a getter with a body.
        let arrow = value.filter(|v| v.kind() == "arrow_expression_clause");
        let mut accessors = Vec::new();
        if arrow.is_some() {
            self.count(Skip::MethodBody);
            let span = arrow.map(|a| a.byte_range()).unwrap_or_default();
            accessors.push(Accessor { kind: "get".into(), visibility: None, has_body: true, span });
        }
        if let Some(list) = node.child_by_field_name("accessors") {
            let declared = named_children(list);
            for a in declared.into_iter().filter(|a| a.kind() == "accessor_declaration") {
                let visibility = self.modifiers(a).visibility;
                let has_body = a.child_by_field_name("body").is_some();
                if has_body {
                    self.count(Skip::MethodBody);
                }
                let kind = a.child_by_field_name("name").map_or("get", |n| self.text(n)).to_owned();
                accessors.push(Accessor { kind, visibility, has_body, span: a.byte_range() });
            }
        }
        let has_bodies = accessors.iter().any(|a| a.has_body);
        let is_abstract = m.is_abstract || (context == Context::Interface && !m.is_static);
        if !is_indexer && !has_bodies && !is_abstract {
            members.push(Member::Field(Field {
                name,
                ty,
                visibility,
                is_static: m.is_static,
                is_read_only: !accessors.iter().any(|a| a.kind == "set"),
                default: value.map(|v| self.text(v).to_owned()),
                documentation,
                ..Default::default()
            }));
            return;
        }
        let index = match node.child_by_field_name("parameters") {
            Some(params) => self.parameters(params),
            None => Vec::new(),
        };
        for a in accessors {
            let (op_name, parameters, return_type) = if a.kind == "get" {
                (format!("get_{name}"), index.clone(), Some(ty.clone()))
            } else {
                let mut params = index.clone();
                params.push(Param { name: "value".into(), ty: ty.clone(), ..Default::default() });
                (format!("set_{name}"), params, Some(TypeExpr::builtin("void")))
            };
            members.push(Member::Operation(Operation {
                name: op_name,
                parameters,
                return_type,
                visibility: a.visibility.unwrap_or(visibility),
                is_static: m.is_static,
                is_abstract: !a.has_body && is_abstract,
                is_leaf: m.is_sealed,
                documentation: documentation.clone(),
                span: a.span,
                ..Default::default()
            }));
        }
    }

    fn operation(&mut self, node: Node, context: Context) -> Option<Operation> {
        let documentation = self.documentation(node);
        let m = self.modifiers(node);
        let name = match node.kind() {
            "destructor_declaration" => {
                format!("~{}", self.text(node.child_by_field_name("name")?))
            }
            "operator_declaration" => {
                format!("operator {}", self.text(node.child_by_field_name("operator")?))
            }
            "conversion_operator_declaration" => {
                let direction = children(node)
                    .into_iter()
                    .find(|n| matches!(n.kind(), "implicit" | "explicit"))
                    .map_or("", |n| self.text(n));
                let ty = self.text(node.child_by_field_name("type")?);
                format!("{direction} operator {ty}")
            }
            _ => self.text(node.child_by_field_name("name")?).to_owned(),
        };
        let is_constructor = node.kind() == "constructor_declaration";
        let return_type = match node.kind() {
            "constructor_declaration" | "destructor_declaration" => None,
            "method_declaration" => Some(self.type_expr(node.child_by_field_name("returns")?)?),
            _ => Some(self.type_expr(node.child_by_field_name("type")?)?),
        };
        let has_body = node.child_by_field_name("body").is_some();
        if has_body {
            self.count(Skip::MethodBody);
        }
        let parameters = match node.child_by_field_name("parameters") {
            Some(params) => self.parameters(params),
            None => Vec::new(),
        };
        let mut template_parameters = named_children(node)
            .into_iter()
            .find(|n| n.kind() == "type_parameter_list")
            .map(|p| self.type_parameters(p))
            .unwrap_or_default();
        self.constraints(node, &mut template_parameters);
        let interface = context == Context::Interface;
        Some(Operation {
            name,
            parameters,
            return_type,
            visibility: m.visibility.unwrap_or(Self::implicit_visibility(context)),
            is_static: m.is_static,
            is_abstract: m.is_abstract || (interface && !has_body && !m.is_static),
            is_leaf: m.is_sealed,
            is_constructor,
            template_parameters,
            documentation,
            span: node.byte_range(),
            ..Default::default()
        })
    }

    /// The parameters of a parameter list. The grammar puts a `params` array's type and name
    /// right into the list.
    fn parameters(&mut self, list: Node) -> Vec<Param> {
        let mut params = Vec::new();
        let mut cursor = list.walk();
        let parts: Vec<(Option<&str>, Node)> = list
            .children(&mut cursor)
            .enumerate()
            .map(|(i, n)| (list.field_name_for_child(i as u32), n))
            .collect();
        let mut spread: Option<TypeExpr> = None;
        for (field, node) in parts {
            match (field, node.kind()) {
                (_, "parameter") => {
                    if let Some(p) = self.parameter(node) {
                        params.push(p);
                    }
                }
                (_, "attribute_list") => self.count_attributes(node),
                (Some("type"), _) => spread = self.type_expr(node),
                (Some("name"), _) => {
                    if let Some(ty) = spread.take() {
                        let name = self.text(node).to_owned();
                        params.push(Param { name, ty, ..Default::default() });
                    }
                }
                _ => {}
            }
        }
        params
    }

    fn parameter(&mut self, node: Node) -> Option<Param> {
        let mut direction = ParameterDirection::In;
        for child in named_children(node) {
            match child.kind() {
                "attribute_list" => self.count_attributes(child),
                "modifier" => match self.text(child) {
                    "out" => direction = ParameterDirection::Out,
                    "ref" => direction = ParameterDirection::InOut,
                    _ => {}
                },
                _ => {}
            }
        }
        let name = self.text(node.child_by_field_name("name")?).to_owned();
        let ty = self.type_expr(node.child_by_field_name("type")?)?;
        let default = after_equals(node).map(|v| self.text(v).to_owned());
        Some(Param { name, ty, default, direction })
    }

    fn type_parameters(&mut self, node: Node) -> Vec<TemplateParam> {
        named_children(node)
            .into_iter()
            .filter(|n| n.kind() == "type_parameter")
            .filter_map(|p| {
                let name = self.text(p.child_by_field_name("name")?).to_owned();
                Some(TemplateParam { name, ..Default::default() })
            })
            .collect()
    }

    /// The type constraints of `where` clauses as bounds of the parameters they name; `class`,
    /// `struct`, `new()` and the like are no types and left out.
    fn constraints(&mut self, decl: Node, params: &mut [TemplateParam]) {
        let clauses = named_children(decl)
            .into_iter()
            .filter(|n| n.kind() == "type_parameter_constraints_clause");
        for clause in clauses {
            let parts = named_children(clause);
            let Some(name) = parts.first().map(|n| self.text(*n)) else { continue };
            let bounds: Vec<TypeExpr> = parts
                .iter()
                .filter(|n| n.kind() == "type_parameter_constraint")
                .filter_map(|c| c.child_by_field_name("type"))
                .filter_map(|t| self.type_expr(t))
                .collect();
            if let Some(p) = params.iter_mut().find(|p| p.name == name) {
                p.bounds.extend(bounds);
            }
        }
    }

    /// A type as written; `None` for nodes that are no type.
    fn type_expr(&mut self, node: Node) -> Option<TypeExpr> {
        Some(match node.kind() {
            "identifier" => TypeExpr::named(vec![self.text(node).to_owned()]),
            "predefined_type" => TypeExpr::builtin(self.text(node)),
            // `A.B.C<T>`. The arguments of a qualifying type (`Outer<A>.Inner`) are dropped.
            "qualified_name" => {
                let mut path = self.path(node.child_by_field_name("qualifier")?);
                let last = self.type_expr(node.child_by_field_name("name")?)?;
                let TypeName::Named(name) = last.base else { return None };
                path.extend(name);
                TypeExpr { base: TypeName::Named(path), ..last }
            }
            // `global::System`: the alias is dropped.
            "alias_qualified_name" => return self.type_expr(node.child_by_field_name("name")?),
            "generic_name" => {
                let parts = named_children(node);
                let name = parts.iter().find(|n| n.kind() == "identifier")?;
                let mut ty = TypeExpr::named(vec![self.text(*name).to_owned()]);
                if let Some(args) = parts.iter().find(|n| n.kind() == "type_argument_list") {
                    ty.arguments = named_children(*args)
                        .into_iter()
                        .filter_map(|a| self.type_expr(a))
                        .collect();
                }
                ty
            }
            "array_type" => {
                let element = self.type_expr(node.child_by_field_name("type")?)?;
                let rank = node.child_by_field_name("rank").map_or("[]", |r| self.text(r));
                element.with_modifier(&rank.split_whitespace().collect::<String>())
            }
            "nullable_type" => {
                self.type_expr(node.child_by_field_name("type")?)?.with_modifier("?")
            }
            "pointer_type" => self.type_expr(node.child_by_field_name("type")?)?.with_modifier("*"),
            "ref_type" | "scoped_type" => {
                return self.type_expr(node.child_by_field_name("type")?);
            }
            "tuple_type" | "function_pointer_type" => TypeExpr::builtin(squeeze(self.text(node))),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceLanguage, dump_text};

    fn parse(text: &str) -> (String, Skipped) {
        dump_text(SourceLanguage::CSharp, text, &[]).unwrap()
    }

    #[test]
    fn reads_a_small_class() {
        let (dump, skipped) = parse(
            "using System.Collections.Generic;\nnamespace A.B;\n/// <summary>An order.</summary>\npublic sealed class Order<T> : Base, IComparable<Order<T>> where T : IComparable<T>, new()\n{\n    private readonly List<T> items = new(), more;\n    public int Count { get; private set; }\n    public static T Pick<K>(out int n, params K[] ks) { n = 0; return default; }\n}\n",
        );
        pretty_assertions::assert_eq!(
            dump,
            "import System.Collections.Generic.*\nnamespace A.B\n  public class leaf Order<T extends IComparable<T>> extends Base implements IComparable<Order<T>> (line 4)\n    doc \"<summary>An order.</summary>\"\n    private field readonly items : List<T> = new()\n    private field readonly more : List<T>\n    public field Count : int\n    public operation static Pick<K>(out n : int, ks : K[]) : T\n"
        );
        assert_eq!(skipped.get(&Skip::MethodBody), Some(&1));
    }

    #[test]
    fn guesses_interfaces_in_base_lists() {
        let (dump, _) = parse("class A : IFoo, Bar {}\nstruct S : Baz {}\ninterface I : J {}\n");
        pretty_assertions::assert_eq!(
            dump,
            "package class A implements IFoo implements Bar (line 1)\npackage struct S implements Baz (line 2)\npackage interface I extends J (line 3)\n"
        );
    }

    #[test]
    fn reports_the_first_syntax_error() {
        let error =
            dump_text(SourceLanguage::CSharp, "class A {\n  int x = ;\n}\n", &[]).unwrap_err();
        assert_eq!((error.line, error.column, error.token.as_str()), (2, 9, "="));
    }
}
