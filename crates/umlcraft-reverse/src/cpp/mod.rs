//! C++: declarations from header and source files.
//!
//! - Namespaces (also `a::b`) become nested namespaces; an anonymous namespace's declarations
//!   belong to the one around it. `using namespace N;` imports a namespace on demand,
//!   `using N::T;` a single name, `namespace A = N::M;` a namespace under another name.
//! - Classes are classes, structs structs and unions classes; members are private in a class
//!   and public in a struct or union until an access specifier says otherwise. Enums (also
//!   `enum class`) are enumerations; their values and underlying type are left out. A type
//!   without a body (`class A;`) only declares the name. Types at namespace scope are public.
//! - Base classes are generalizations with their access (private in a class, public in a
//!   struct, unless written) and whether they are `virtual`.
//! - Templates: type, value (`int N`), variadic (`typename... Ts`) and template template
//!   parameters, with defaults. Specializations (`template<>`, `class A<T*>`) are left out.
//! - Member functions are operations: pure virtual ones (`= 0`) are abstract and make their
//!   class abstract, `const` ones are queries, `final` ones leaves, `static` ones static.
//!   Constructors and destructors (`~A`) have no return type; operators are named as written
//!   (`operator=`, `operator int`). Functions declared `= delete` are left out; `= default`
//!   ones are kept.
//! - Fields: one per declarator (`int x, *y;`). Pointers, references and arrays are type
//!   modifiers (`int*`, `Line&`, `char[16]`); `const` and `volatile` are part of the type;
//!   `static const` and `constexpr` fields are read-only.
//! - Members defined outside their class (`void A::f() {}`, `int A::n = 0;`) are definitions;
//!   the mapping joins them to the class.
//! - `typedef` and `using X = …` are aliases the mapping replaces by their targets.
//! - A declaration's documentation is the run of `///` or `//!` comments right before it, or a
//!   `/** */` or `/*! */` comment.
//! - Of `#if`/`#ifdef`/`#ifndef` … `#else`, only the first branch is read; nothing is
//!   evaluated, macros are not expanded, so declarations made by macros are not seen.
//! - Left out and counted: function bodies, functions and variables outside classes, macro
//!   definitions, specializations, deleted functions, friends, anonymous types and enum values.

pub(crate) mod rules;

use tree_sitter::{Node, Parser};
use umlcraft_model::{ParameterDirection, Visibility};

use crate::ir::{
    Field, Import, Item, Literal, Member, Operation, Param, Relation, Scope, Supertype,
    TemplateParam, TypeDecl, TypeExpr, TypeKind, TypeName,
};
use crate::lang::{Frontend, Skip, Skipped, SyntaxError, first_error};
use crate::preprocessor;

pub struct Cpp;

impl Frontend for Cpp {
    fn parse(
        &self,
        text: &str,
        _module: &[String],
        skipped: &mut Skipped,
    ) -> Result<Scope, SyntaxError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_cpp::LANGUAGE.into())
            .expect("the C++ grammar matches the tree-sitter version");
        let (text, branches) = preprocessor::first_branches(text, preprocessor::Dialect::Cpp);
        let tree = parser.parse(&text, None).expect("parsing without a timeout always finishes");
        let root = tree.root_node();
        if let Some(error) = first_error(root, &text) {
            return Err(error);
        }
        if branches > 0 {
            *skipped.entry(Skip::PreprocessorBranch).or_default() += branches;
        }
        let mut reader = Reader { src: &text, skipped };
        let mut scope = Scope::default();
        reader.scope(&named_children(root), &mut scope);
        Ok(scope)
    }
}

struct Reader<'a> {
    src: &'a str,
    skipped: &'a mut Skipped,
}

/// What a declarator declares: `*y[4]`, `&operator=(const A&) const`, `A::count = 0`.
#[derive(Default)]
struct Declarator<'t> {
    /// The declared name: an identifier, a qualified name, `~A`, `operator=`, `operator int`.
    name: Option<Node<'t>>,
    /// Pointers, references and arrays around the name, in the order they apply to the type.
    modifier: String,
    /// The function declarator, for functions.
    function: Option<Node<'t>>,
    /// A pointer to a function (`int (*f)(int)`): a variable, not a function.
    is_function_pointer: bool,
    /// The initializer of an `init_declarator`.
    value: Option<Node<'t>>,
}

/// The modifiers written before a declaration's type and declarators.
#[derive(Default)]
struct Specifiers {
    is_static: bool,
    is_const: bool,
    is_constexpr: bool,
    is_volatile: bool,
}

fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// The children of `node` with the names of their fields.
fn fields<'t>(node: Node<'t>) -> Vec<(Option<&'t str>, Node<'t>)> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .enumerate()
        .map(|(i, n)| (node.field_name_for_child(i as u32), n))
        .collect()
}

fn is_type_specifier(kind: &str) -> bool {
    matches!(kind, "class_specifier" | "struct_specifier" | "union_specifier" | "enum_specifier")
}

/// Text with its whitespace runs made single blanks: `unsigned  long` is `unsigned long`.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An operator's name as one spelling: `operator ==` and `operator ( )` as `operator==` and
/// `operator()`, `operator new []` as `operator new[]`. Other names stay as they are.
fn operator_name(name: &str) -> String {
    let Some(rest) = name.strip_prefix("operator") else { return name.to_owned() };
    // `operatorX` is an ordinary name.
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return name.to_owned();
    }
    let symbol = rest.trim_start();
    let word = symbol.find(|c: char| !c.is_alphanumeric() && c != '_').unwrap_or(symbol.len());
    let (word, tail) = symbol.split_at(word);
    let tail: String = tail.split_whitespace().collect();
    if word.is_empty() { format!("operator{tail}") } else { format!("operator {word}{tail}") }
}

/// Whether `t` is just the name `name` (`typedef struct Node Node;`).
fn is_named(t: &TypeExpr, name: &str) -> bool {
    t.prefix.is_empty()
        && t.arguments.is_empty()
        && t.modifier.is_empty()
        && matches!(&t.base, TypeName::Named(p) if p.len() == 1 && p[0] == name)
}

fn access(text: &str) -> Option<Visibility> {
    match text {
        "public" => Some(Visibility::Public),
        "protected" => Some(Visibility::Protected),
        "private" => Some(Visibility::Private),
        _ => None,
    }
}

impl<'a> Reader<'a> {
    fn text(&self, node: Node) -> &'a str {
        &self.src[node.byte_range()]
    }

    fn count(&mut self, skip: Skip) {
        *self.skipped.entry(skip).or_default() += 1;
    }

    /// The declarations among `nodes`, the children of a file, a namespace body or an
    /// `extern "C"` block, into `scope`.
    fn scope(&mut self, nodes: &[Node], scope: &mut Scope) {
        for &node in nodes {
            match node.kind() {
                "namespace_definition" => {
                    let mut inner = Scope::default();
                    if let Some(body) = node.child_by_field_name("body") {
                        self.scope(&named_children(body), &mut inner);
                    }
                    match node.child_by_field_name("name") {
                        Some(name) => {
                            let path = self.namespace_path(name);
                            scope.items.push(Item::Namespace { path, scope: inner });
                        }
                        // Anonymous: its declarations belong to the namespace around it.
                        None => {
                            scope.imports.extend(inner.imports);
                            scope.items.extend(inner.items);
                        }
                    }
                }
                "linkage_specification" => {
                    if let Some(body) = node.child_by_field_name("body") {
                        let nodes = match body.kind() {
                            "declaration_list" => named_children(body),
                            _ => vec![body],
                        };
                        self.scope(&nodes, scope);
                    }
                }
                "using_declaration" => {
                    if let Some(import) = self.using(node) {
                        scope.imports.push(import);
                    }
                }
                "namespace_alias_definition" => {
                    let parts = named_children(node);
                    let name = node.child_by_field_name("name");
                    let target = parts.iter().copied().rfind(|&n| Some(n) != name);
                    if let (Some(name), Some(target)) = (name, target) {
                        scope.imports.push(Import {
                            path: self.namespace_path(target),
                            wildcard: false,
                            is_static: false,
                            alias: Some(self.text(name).to_owned()),
                        });
                    }
                }
                "alias_declaration" | "type_definition" => {
                    for (name, target, t) in self.aliases(node) {
                        if let Some(t) = t {
                            scope.items.push(Item::Type(t));
                        }
                        if let Some(target) = target {
                            scope.items.push(Item::Alias { name, target });
                        }
                    }
                }
                "template_declaration" => self.namespace_template(node, scope),
                kind if is_type_specifier(kind) => {
                    if let Some(t) = self.type_decl(node, node, Vec::new(), Visibility::Public) {
                        scope.items.push(Item::Type(t));
                    }
                }
                "declaration" | "function_definition" => {
                    self.namespace_declaration(node, node, scope);
                }
                "preproc_def" | "preproc_function_def" => self.count(Skip::Macro),
                _ => {}
            }
        }
    }

    /// `a`, `a::b` (nested or qualified) as its segments.
    fn namespace_path(&self, node: Node) -> Vec<String> {
        match node.kind() {
            "nested_namespace_specifier" => {
                named_children(node).into_iter().flat_map(|n| self.namespace_path(n)).collect()
            }
            "qualified_identifier" => {
                let mut path = Vec::new();
                if let Some(scope) = node.child_by_field_name("scope") {
                    path.extend(self.namespace_path(scope));
                }
                if let Some(name) = node.child_by_field_name("name") {
                    path.extend(self.namespace_path(name));
                }
                path
            }
            // `A<int>` in a scope: the arguments are dropped.
            "template_type" => node
                .child_by_field_name("name")
                .map(|n| vec![self.text(n).to_owned()])
                .unwrap_or_default(),
            _ => vec![self.text(node).to_owned()],
        }
    }

    /// `using namespace N;` (on demand) or `using N::T;` (a single name).
    fn using(&self, node: Node) -> Option<Import> {
        let wildcard = children(node).iter().any(|n| n.kind() == "namespace");
        let target = named_children(node).into_iter().next()?;
        let path = self.namespace_path(target);
        Some(Import { path, wildcard, is_static: false, alias: None })
    }

    /// The aliases a `typedef` or `using X = …` declares, as name and target, with the type it
    /// declares on the way (`typedef struct Node { … } Node;`). A `typedef` of an anonymous
    /// type names the type (`typedef struct { … } Point;`). Alias templates are left out.
    fn aliases(&mut self, node: Node) -> Vec<(String, Option<TypeExpr>, Option<TypeDecl>)> {
        let Some(ty) = node.child_by_field_name("type") else { return Vec::new() };
        if node.kind() == "alias_declaration" {
            let Some(name) = node.child_by_field_name("name") else { return Vec::new() };
            let name = self.text(name).to_owned();
            let target = self.type_expr(ty).filter(|t| !is_named(t, &name));
            return vec![(name, target, None)];
        }
        let declarators: Vec<Node> = fields(node)
            .into_iter()
            .filter(|(f, _)| *f == Some("declarator"))
            .map(|(_, n)| n)
            .collect();
        let mut out = Vec::new();
        let mut declared = None;
        let mut base = None;
        if is_type_specifier(ty.kind()) && ty.child_by_field_name("body").is_some() {
            let anonymous = ty.child_by_field_name("name").is_none();
            let named = declarators
                .first()
                .map(|&d| self.declarator(d))
                .filter(|d| d.modifier.is_empty() && d.function.is_none() && d.name.is_some());
            match (anonymous, named) {
                (true, Some(d)) => {
                    let name = self.text(d.name.expect("filtered"));
                    declared = self.type_decl_named(ty, node, Vec::new(), Visibility::Public, name);
                    // The other declarators refer to it by that name (`*PointPtr`).
                    base = Some(TypeExpr::named(vec![name.to_owned()]));
                }
                (true, None) => self.count(Skip::AnonymousClass),
                (false, _) => declared = self.type_decl(ty, node, Vec::new(), Visibility::Public),
            }
        }
        let base = base.or_else(|| self.type_expr(ty));
        for d in declarators {
            let d = self.declarator(d);
            let Some(name) = d.name else { continue };
            let name = self.text(name).to_owned();
            let target =
                base.clone().map(|b| self.variable_type(b, &d)).filter(|t| !is_named(t, &name));
            out.push((name, target, declared.take()));
        }
        if let Some(t) = declared {
            out.push((String::new(), None, Some(t)));
        }
        out
    }

    /// A template at namespace scope: a class template, or a definition or function template.
    fn namespace_template(&mut self, node: Node, scope: &mut Scope) {
        let Some((params, inner)) = self.template_parts(node) else { return };
        match inner.kind() {
            kind if is_type_specifier(kind) => {
                if let Some(t) = self.type_decl(inner, node, params, Visibility::Public) {
                    scope.items.push(Item::Type(t));
                }
            }
            "declaration" | "function_definition" => {
                self.namespace_declaration(inner, node, scope);
            }
            // `template<class T> template<class U> void A<T>::f(U)`.
            "template_declaration" => self.namespace_template(inner, scope),
            "alias_declaration" => self.count(Skip::GenericAlias),
            _ => {}
        }
    }

    /// The parameters of a template and what it declares; `None` for specializations
    /// (`template<>`), which are counted.
    fn template_parts<'t>(&mut self, node: Node<'t>) -> Option<(Vec<TemplateParam>, Node<'t>)> {
        let list = node.child_by_field_name("parameters")?;
        let inner = named_children(node).into_iter().rfind(|&n| n != list)?;
        let params = self.template_parameters(list);
        if params.is_empty() {
            self.count(Skip::Specialization);
            return None;
        }
        Some((params, inner))
    }

    fn template_parameters(&mut self, list: Node) -> Vec<TemplateParam> {
        let mut params = Vec::new();
        for p in named_children(list) {
            let name_of = |p: Node| {
                named_children(p)
                    .into_iter()
                    .rfind(|n| n.kind() == "type_identifier")
                    .map(|n| self.text(n).to_owned())
            };
            let param = match p.kind() {
                "type_parameter_declaration" => {
                    TemplateParam { name: name_of(p).unwrap_or_default(), ..Default::default() }
                }
                "variadic_type_parameter_declaration" => TemplateParam {
                    name: name_of(p).unwrap_or_default(),
                    is_variadic: true,
                    ..Default::default()
                },
                "optional_type_parameter_declaration" => TemplateParam {
                    name: p
                        .child_by_field_name("name")
                        .map_or(String::new(), |n| self.text(n).to_owned()),
                    default: p.child_by_field_name("default_type").and_then(|t| self.type_expr(t)),
                    ..Default::default()
                },
                // A value: `int N`, `int N = 4`, `int... Ns`.
                "parameter_declaration"
                | "optional_parameter_declaration"
                | "variadic_parameter_declaration" => {
                    let Some(ty) = self.declared_type(p) else { continue };
                    let d = p.child_by_field_name("declarator").map(|d| self.declarator(d));
                    let d = d.unwrap_or_default();
                    TemplateParam {
                        name: d.name.map_or(String::new(), |n| self.text(n).to_owned()),
                        value_type: Some(ty.with_modifier(&d.modifier.replace("...", ""))),
                        default: p
                            .child_by_field_name("default_value")
                            .map(|v| TypeExpr::builtin(squeeze(self.text(v)))),
                        is_variadic: p.kind() == "variadic_parameter_declaration",
                        bounds: Vec::new(),
                    }
                }
                // `template<class> class C`: the parameter's own parameters are left out.
                "template_template_parameter_declaration" => {
                    let inner = named_children(p)
                        .into_iter()
                        .rfind(|n| n.kind().contains("parameter_declaration"));
                    TemplateParam {
                        name: inner.and_then(name_of).unwrap_or_default(),
                        ..Default::default()
                    }
                }
                _ => continue,
            };
            params.push(param);
        }
        // An unnamed parameter (`template <typename>`) still takes its place; it is named by
        // its position, as the model needs a name.
        for (i, p) in params.iter_mut().enumerate() {
            if p.name.is_empty() {
                p.name = format!("_{}", i + 1);
            }
        }
        params
    }

    /// A declaration or function definition at namespace scope: the type it declares on the
    /// way, then per declarator a definition of a member (qualified name) or a free function
    /// or variable. `doc` is where its documentation is looked for.
    fn namespace_declaration(&mut self, node: Node, doc: Node, scope: &mut Scope) {
        if let Some(ty) = node.child_by_field_name("type")
            && is_type_specifier(ty.kind())
            && ty.child_by_field_name("body").is_some()
            && let Some(t) = self.type_decl(ty, doc, Vec::new(), Visibility::Public)
        {
            scope.items.push(Item::Type(t));
        }
        let documentation = self.documentation(doc);
        let specifiers = self.specifiers(node);
        let base = self.declared_type(node);
        for (d, default) in self.declarators(node) {
            let Some(name) = d.name else { continue };
            let (owner, name) = match name.kind() {
                "qualified_identifier" => self.qualified_member(name),
                _ => (Vec::new(), name),
            };
            let member = if d.function.is_some() && !d.is_function_pointer {
                let class = owner.last().map(String::as_str);
                let Some(o) = self.function(&d, name, class, base.clone(), &specifiers) else {
                    continue;
                };
                Member::Operation(Operation { documentation: documentation.clone(), ..o })
            } else {
                let Some(f) = self.field(&d, name, base.clone(), &specifiers, default) else {
                    continue;
                };
                Member::Field(Field { documentation: documentation.clone(), ..f })
            };
            if owner.is_empty() {
                scope.items.push(Item::Free(member));
            } else {
                scope.items.push(Item::Definition { owner, member });
            }
        }
    }

    /// `ns::A<T>::f` as the owner's path (`["ns", "A"]`, template arguments dropped) and the
    /// node of the member's own name.
    fn qualified_member<'t>(&self, node: Node<'t>) -> (Vec<String>, Node<'t>) {
        let mut owner = Vec::new();
        let mut at = node;
        while at.kind() == "qualified_identifier" {
            if let Some(scope) = at.child_by_field_name("scope") {
                owner.extend(self.namespace_path(scope));
            }
            let Some(name) = at.child_by_field_name("name") else { break };
            at = name;
        }
        (owner, at)
    }

    /// The documentation right before `node`: a run of `///` or `//!` comments on the lines
    /// just above it, or a `/** */` or `/*! */` comment. Comments after other code on their
    /// line (`int x; ///< …`) belong to that code.
    fn documentation(&self, node: Node) -> String {
        let own_line = |c: Node| match c.prev_sibling() {
            Some(p) => p.end_position().row < c.start_position().row,
            None => true,
        };
        let mut lines = Vec::new();
        let mut next = node;
        while let Some(p) = next.prev_sibling() {
            let text = self.text(p);
            let adjacent = p.end_position().row + 1 >= next.start_position().row;
            let line_doc = (text.starts_with("///") || text.starts_with("//!"))
                && !text.starts_with("///<")
                && !text.starts_with("//!<");
            if p.kind() != "comment" || !line_doc || !adjacent || !own_line(p) {
                break;
            }
            lines.push(text);
            next = p;
        }
        if !lines.is_empty() {
            lines.reverse();
            return crate::comment::clean_lines(lines);
        }
        match node.prev_sibling() {
            Some(p)
                if p.kind() == "comment"
                    && own_line(p)
                    && (self.text(p).starts_with("/**") || self.text(p).starts_with("/*!"))
                    && !self.text(p).starts_with("/**<") =>
            {
                crate::comment::clean(self.text(p))
            }
            _ => String::new(),
        }
    }

    /// A class, struct, union or enum with its members; `None` for anonymous types and
    /// specializations, which are counted. `doc` is where its documentation is looked for,
    /// `visibility` the type's own.
    fn type_decl(
        &mut self,
        node: Node,
        doc: Node,
        template_parameters: Vec<TemplateParam>,
        visibility: Visibility,
    ) -> Option<TypeDecl> {
        let Some(name) = node.child_by_field_name("name") else {
            if node.child_by_field_name("body").is_some() {
                self.count(Skip::AnonymousClass);
            }
            return None;
        };
        if name.kind() != "type_identifier" {
            self.count(Skip::Specialization);
            return None;
        }
        let name = self.text(name);
        self.type_decl_named(node, doc, template_parameters, visibility, name)
    }

    fn type_decl_named(
        &mut self,
        node: Node,
        doc: Node,
        template_parameters: Vec<TemplateParam>,
        visibility: Visibility,
        name: &str,
    ) -> Option<TypeDecl> {
        let (kind, default_access) = match node.kind() {
            "class_specifier" => (TypeKind::Class, Visibility::Private),
            "struct_specifier" => (TypeKind::Struct, Visibility::Public),
            "union_specifier" => (TypeKind::Class, Visibility::Public),
            "enum_specifier" => (TypeKind::Enum, Visibility::Public),
            _ => return None,
        };
        let mut t = TypeDecl::new(name, kind);
        t.line = node.start_position().row + 1;
        t.documentation = self.documentation(doc);
        t.visibility = visibility;
        t.template_parameters = template_parameters;
        let parts = named_children(node);
        t.is_leaf =
            parts.iter().any(|n| n.kind() == "virtual_specifier" && self.text(*n) == "final");
        let Some(body) = node.child_by_field_name("body") else {
            t.is_forward = true;
            return Some(t);
        };
        if let Some(bases) = parts.iter().find(|n| n.kind() == "base_class_clause") {
            self.bases(*bases, default_access, &mut t);
        }
        if kind == TypeKind::Enum {
            for e in named_children(body).into_iter().filter(|n| n.kind() == "enumerator") {
                let Some(name) = e.child_by_field_name("name") else { continue };
                if e.child_by_field_name("value").is_some() {
                    self.count(Skip::EnumValue);
                }
                t.members.push(Member::Literal(Literal {
                    name: self.text(name).to_owned(),
                    documentation: self.documentation(e),
                }));
            }
        } else {
            let mut access = default_access;
            for member in named_children(body) {
                self.member(member, member, &mut access, &mut t, Vec::new());
            }
            t.is_abstract =
                t.members.iter().any(|m| matches!(m, Member::Operation(o) if o.is_abstract));
        }
        Some(t)
    }

    /// `: public A, protected virtual B<T>`: generalizations with their access.
    fn bases(&mut self, clause: Node, default_access: Visibility, t: &mut TypeDecl) {
        let mut access = None;
        let mut is_virtual = false;
        for part in children(clause) {
            match part.kind() {
                "access_specifier" => access = self::access(self.text(part)),
                "virtual" => is_virtual = true,
                _ => {
                    if let Some(ty) = self.type_expr(part) {
                        t.supertypes.push(Supertype {
                            ty,
                            relation: Relation::Extends,
                            access: Some(access.take().unwrap_or(default_access)),
                            is_virtual,
                        });
                        is_virtual = false;
                    }
                }
            }
        }
    }

    /// One entry of a class body into `t`. `access` is the visibility in force, changed by
    /// access specifiers; `doc` is where the documentation is looked for.
    fn member(
        &mut self,
        node: Node,
        doc: Node,
        access: &mut Visibility,
        t: &mut TypeDecl,
        template_parameters: Vec<TemplateParam>,
    ) {
        match node.kind() {
            "access_specifier" => {
                if let Some(a) = self::access(self.text(node)) {
                    *access = a;
                }
            }
            "field_declaration" | "declaration" | "function_definition" => {
                self.member_declaration(node, doc, *access, t, template_parameters);
            }
            "template_declaration" => {
                if let Some((params, inner)) = self.template_parts(node) {
                    self.member(inner, node, access, t, params);
                }
            }
            "alias_declaration" | "type_definition" => {
                if !template_parameters.is_empty() {
                    self.count(Skip::GenericAlias);
                    return;
                }
                for (name, target, nested) in self.aliases(node) {
                    if let Some(mut nested) = nested {
                        nested.visibility = *access;
                        t.members.push(Member::Type(nested));
                    }
                    if let Some(target) = target {
                        t.members.push(Member::Alias { name, target });
                    }
                }
            }
            kind if is_type_specifier(kind) => {
                if let Some(nested) = self.type_decl(node, doc, template_parameters, *access) {
                    t.members.push(Member::Type(nested));
                }
            }
            "friend_declaration" => self.count(Skip::Friend),
            "preproc_def" | "preproc_function_def" => self.count(Skip::Macro),
            _ => {}
        }
    }

    /// A member declaration: a nested type on the way, then per declarator an operation or a
    /// field.
    fn member_declaration(
        &mut self,
        node: Node,
        doc: Node,
        access: Visibility,
        t: &mut TypeDecl,
        template_parameters: Vec<TemplateParam>,
    ) {
        let ty = node.child_by_field_name("type");
        if let Some(ty) = ty.filter(|ty| is_type_specifier(ty.kind())) {
            let has_body = ty.child_by_field_name("body").is_some();
            if has_body
                && let Some(nested) = self.type_decl(ty, doc, template_parameters.clone(), access)
            {
                t.members.push(Member::Type(nested));
            }
            // Fields of an anonymous type have no type to name.
            if has_body && ty.child_by_field_name("name").is_none() {
                return;
            }
        }
        let documentation = self.documentation(doc);
        let specifiers = self.specifiers(node);
        let base = self.declared_type(node);
        let parts = children(node);
        let deleted = parts.iter().any(|n| n.kind() == "delete_method_clause");
        let defaulted = parts.iter().any(|n| n.kind() == "default_method_clause");
        let has_body = node.child_by_field_name("body").is_some();
        for (d, default) in self.declarators(node) {
            let Some(name) = d.name else { continue };
            if d.function.is_some() && !d.is_function_pointer {
                if deleted {
                    self.count(Skip::DeletedFunction);
                    continue;
                }
                let class = Some(t.name.as_str());
                let Some(mut o) = self.function(&d, name, class, base.clone(), &specifiers) else {
                    continue;
                };
                // `= 0` on a function: pure virtual.
                o.is_abstract = default.is_some_and(|v| self.text(v) == "0");
                if has_body && !defaulted {
                    self.count(Skip::MethodBody);
                }
                o.visibility = access;
                o.documentation = documentation.clone();
                o.template_parameters = template_parameters.clone();
                t.members.push(Member::Operation(o));
            } else if let Some(mut f) = self.field(&d, name, base.clone(), &specifiers, default) {
                f.visibility = access;
                f.documentation = documentation.clone();
                t.members.push(Member::Field(f));
            }
        }
    }

    fn specifiers(&self, node: Node) -> Specifiers {
        let mut s = Specifiers::default();
        for child in named_children(node) {
            match child.kind() {
                "storage_class_specifier" | "type_qualifier" => match self.text(child) {
                    "static" => s.is_static = true,
                    "const" => s.is_const = true,
                    "constexpr" | "constinit" => s.is_constexpr = true,
                    "volatile" => s.is_volatile = true,
                    _ => {}
                },
                _ => {}
            }
        }
        s
    }

    /// The type of a declaration, before its declarators: its `type` with the `const` and
    /// `volatile` written around it.
    fn declared_type(&mut self, node: Node) -> Option<TypeExpr> {
        let mut ty = self.type_expr(node.child_by_field_name("type")?)?;
        let s = self.specifiers(node);
        let mut prefix = String::new();
        if s.is_const {
            prefix.push_str("const ");
        }
        if s.is_volatile {
            prefix.push_str("volatile ");
        }
        ty.prefix.insert_str(0, &prefix);
        Some(ty)
    }

    /// The declarators of a declaration, each with the default value written after it
    /// (`int a = 1, b;`, `void f() = 0;`).
    fn declarators<'t>(&self, node: Node<'t>) -> Vec<(Declarator<'t>, Option<Node<'t>>)> {
        let mut out: Vec<(Declarator<'t>, Option<Node<'t>>)> = Vec::new();
        for (field, child) in fields(node) {
            match field {
                Some("declarator") => out.push((self.declarator(child), None)),
                Some("default_value") => {
                    if let Some(last) = out.last_mut() {
                        last.1 = Some(child);
                    }
                }
                _ => {}
            }
        }
        for (d, default) in &mut out {
            if default.is_none() {
                *default = d.value;
            }
        }
        out
    }

    /// What `node` declares, from the outside in: `*y[4]` is `y` with the modifier `*[4]`.
    fn declarator<'t>(&self, node: Node<'t>) -> Declarator<'t> {
        let mut d = Declarator::default();
        let mut at = Some(node);
        while let Some(node) = at {
            let inner = node.child_by_field_name("declarator");
            let first = || named_children(node).into_iter().find(|n| n.kind() != "type_qualifier");
            at = match node.kind() {
                "pointer_declarator" | "abstract_pointer_declarator" => {
                    if d.function.is_some() {
                        d.is_function_pointer = true;
                    } else {
                        d.modifier.push('*');
                    }
                    inner
                }
                "reference_declarator" | "abstract_reference_declarator" => {
                    let ampersands = children(node).into_iter().find(|n| n.kind().starts_with('&'));
                    d.modifier.push_str(ampersands.map_or("&", |n| n.kind()));
                    first()
                }
                "array_declarator" | "abstract_array_declarator" => {
                    let size = node
                        .child_by_field_name("size")
                        .map_or(String::new(), |s| squeeze(self.text(s)));
                    d.modifier.push_str(&format!("[{size}]"));
                    inner
                }
                "function_declarator" | "abstract_function_declarator" => {
                    d.function.get_or_insert(node);
                    inner
                }
                "parenthesized_declarator" | "attributed_declarator" => first(),
                "init_declarator" => {
                    d.value = node.child_by_field_name("value");
                    inner
                }
                "variadic_declarator" => {
                    d.modifier.push_str("...");
                    first()
                }
                // `operator int() const`: the function declarator is inside.
                "operator_cast" => {
                    d.name = Some(node);
                    d.function = inner;
                    None
                }
                _ => {
                    d.name = Some(node);
                    None
                }
            };
        }
        d
    }

    /// An operation from a function declarator. `class` is the name of the class it belongs
    /// to, which tells constructors; `base` the declaration's type, before the declarator.
    fn function(
        &mut self,
        d: &Declarator,
        name: Node,
        class: Option<&str>,
        base: Option<TypeExpr>,
        specifiers: &Specifiers,
    ) -> Option<Operation> {
        let function = d.function?;
        let (name, cast) = match name.kind() {
            "operator_cast" => {
                let ty = self.type_expr(name.child_by_field_name("type")?)?;
                (format!("operator {ty}"), Some(ty))
            }
            "template_function" => {
                self.count(Skip::Specialization);
                return None;
            }
            _ => (operator_name(&squeeze(self.text(name))), None),
        };
        let trailing = named_children(function)
            .into_iter()
            .find(|n| n.kind() == "trailing_return_type")
            .and_then(|r| named_children(r).into_iter().next())
            .and_then(|t| self.type_expr(t));
        let is_destructor = name.starts_with('~');
        let return_type = match (trailing, cast, base) {
            (Some(t), _, _) | (None, Some(t), _) => Some(t),
            (None, None, Some(b)) => Some(b.with_modifier(&d.modifier)),
            (None, None, None) => None,
        };
        let is_constructor =
            return_type.is_none() && !is_destructor && Some(name.as_str()) == class;
        let parts = named_children(function);
        let is_query =
            parts.iter().any(|n| n.kind() == "type_qualifier" && self.text(*n) == "const");
        let is_leaf =
            parts.iter().any(|n| n.kind() == "virtual_specifier" && self.text(*n) == "final");
        let parameters = function
            .child_by_field_name("parameters")
            .map(|p| self.parameters(p))
            .unwrap_or_default();
        Some(Operation {
            name,
            parameters,
            return_type,
            visibility: Visibility::Public,
            is_static: specifiers.is_static,
            is_abstract: false,
            is_leaf,
            is_constructor,
            is_query,
            template_parameters: Vec::new(),
            tags: Vec::new(),
            documentation: String::new(),
        })
    }

    fn field(
        &mut self,
        d: &Declarator,
        name: Node,
        base: Option<TypeExpr>,
        specifiers: &Specifiers,
        default: Option<Node>,
    ) -> Option<Field> {
        let base = base?;
        let ty = self.variable_type(base, d);
        Some(Field {
            name: self.text(name).to_owned(),
            ty,
            visibility: Visibility::Public,
            is_static: specifiers.is_static,
            is_read_only: specifiers.is_constexpr || (specifiers.is_static && specifiers.is_const),
            default: default.map(|v| squeeze(self.text(v))),
            tags: Vec::new(),
            documentation: String::new(),
        })
    }

    /// The type of a variable (or typedef) declared by `d` with the declaration's type `base`.
    fn variable_type(&self, base: TypeExpr, d: &Declarator) -> TypeExpr {
        match d.function {
            // `int (*f)(int)`: kept as text.
            Some(function) if d.is_function_pointer => {
                let params =
                    function.child_by_field_name("parameters").map_or("()", |p| self.text(p));
                let base = base.with_modifier(&d.modifier);
                TypeExpr::builtin(format!("{base} (*){}", squeeze(params)))
            }
            _ => base.with_modifier(&d.modifier),
        }
    }

    /// The parameters of a parameter list; a nameless parameter keeps an empty name, and C's
    /// `...` is a parameter of type `...`.
    fn parameters(&mut self, list: Node) -> Vec<Param> {
        let mut params = Vec::new();
        for p in children(list) {
            if p.kind() == "..." {
                params.push(Param {
                    name: String::new(),
                    ty: TypeExpr::builtin("..."),
                    default: None,
                    direction: ParameterDirection::In,
                });
                continue;
            }
            if !matches!(
                p.kind(),
                "parameter_declaration"
                    | "optional_parameter_declaration"
                    | "variadic_parameter_declaration"
            ) {
                continue;
            }
            let Some(base) = self.declared_type(p) else { continue };
            let d =
                p.child_by_field_name("declarator").map(|d| self.declarator(d)).unwrap_or_default();
            // `f(void)` takes nothing.
            if d.name.is_none() && d.modifier.is_empty() && self.text(p) == "void" {
                continue;
            }
            params.push(Param {
                name: d.name.map_or(String::new(), |n| self.text(n).to_owned()),
                ty: base.with_modifier(&d.modifier),
                default: p.child_by_field_name("default_value").map(|v| squeeze(self.text(v))),
                direction: ParameterDirection::In,
            });
        }
        params
    }

    /// A type as written; `None` for nodes that are no type.
    fn type_expr(&mut self, node: Node) -> Option<TypeExpr> {
        Some(match node.kind() {
            "primitive_type"
            | "sized_type_specifier"
            | "placeholder_type_specifier"
            | "decltype"
            | "auto" => TypeExpr::builtin(squeeze(self.text(node))),
            "type_identifier" | "namespace_identifier" | "identifier" => {
                TypeExpr::named(vec![self.text(node).to_owned()])
            }
            // `a::b::C<T>`. The arguments of a qualifying type (`Outer<A>::Inner`) are dropped.
            "qualified_identifier" => {
                let mut path = match node.child_by_field_name("scope") {
                    Some(scope) => self.namespace_path(scope),
                    None => Vec::new(),
                };
                let last = self.type_expr(node.child_by_field_name("name")?)?;
                let TypeName::Named(name) = last.base else { return None };
                path.extend(name);
                TypeExpr { base: TypeName::Named(path), ..last }
            }
            "template_type" => {
                let mut ty = self.type_expr(node.child_by_field_name("name")?)?;
                if let Some(args) = node.child_by_field_name("arguments") {
                    ty.arguments = named_children(args)
                        .into_iter()
                        .map(|a| {
                            self.type_expr(a)
                                .unwrap_or_else(|| TypeExpr::builtin(squeeze(self.text(a))))
                        })
                        .collect();
                }
                ty
            }
            // A type in a template argument or cast: `const Line*`.
            "type_descriptor" => {
                let base = self.declared_type(node)?;
                match node.child_by_field_name("declarator") {
                    Some(d) => {
                        let d = self.declarator(d);
                        if d.function.is_some() {
                            TypeExpr::builtin(squeeze(self.text(node)))
                        } else {
                            base.with_modifier(&d.modifier)
                        }
                    }
                    None => base,
                }
            }
            // `struct Line* next;`
            kind if is_type_specifier(kind) => {
                return self.type_expr(node.child_by_field_name("name")?);
            }
            // `typename T::value_type`
            "dependent_type" => return self.type_expr(named_children(node).into_iter().next()?),
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (String, Skipped) {
        let mut skipped = Skipped::new();
        let scope = Cpp.parse(text, &[], &mut skipped).unwrap();
        let unit = crate::ir::Unit { path: Default::default(), scope };
        (unit.dump(), skipped)
    }

    #[test]
    fn reads_a_small_class() {
        let (dump, skipped) = parse(
            "namespace shop::orders {\n/// An order.\ntemplate <typename T, int N = 4>\nclass Order : public Base, protected virtual Other<T> {\n  int x, *y[4];\npublic:\n  Order(int a);\n  virtual ~Order();\n  virtual void ship(const Line& l, int = 0) const = 0;\n  static const int max = 3;\n  int get() const { return x; }\n};\n}\n",
        );
        pretty_assertions::assert_eq!(
            dump,
            "namespace shop.orders\n  public class abstract Order<T, int N = 4> extends public Base extends protected virtual Other<T> (line 4)\n    doc \"An order.\"\n    private field x : int\n    private field y : int*[4]\n    public operation constructor Order(a : int)\n    public operation ~Order()\n    public operation abstract query ship(l : const Line&,  : int = 0) : void\n    public field static readonly max : const int = 3\n    public operation query get() : int\n"
        );
        assert_eq!(skipped.get(&Skip::MethodBody), Some(&1));
    }

    #[test]
    fn spells_operators_once_and_names_what_is_unnamed() {
        let (dump, _) = parse(
            "template <typename, class U> class A {
  bool operator ==(const A&);
  bool operator==(int);
  void* operator new [] (unsigned long);
};
typedef struct { int x; } Point, *PointPtr;
",
        );
        pretty_assertions::assert_eq!(
            dump,
            "public class A<_1, U> (line 1)
  private operation operator==( : const A&) : bool
  private operation operator==( : int) : bool
  private operation operator new[]( : unsigned long) : void*
public struct Point (line 6)
  public field x : int
alias PointPtr = Point*
"
        );
    }

    #[test]
    fn reports_the_first_syntax_error() {
        let mut skipped = Skipped::new();
        let error = Cpp.parse("class A {\n  int x = ;\n};\n", &[], &mut skipped).unwrap_err();
        assert_eq!((error.line, error.column), (2, 9));
    }
}
