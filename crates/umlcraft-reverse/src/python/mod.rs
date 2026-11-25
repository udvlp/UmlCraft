//! Python: declarations from `.py` and `.pyi` files.
//!
//! - Every file is a module: its declarations are in the package of its module path
//!   (`shop/orders.py` in `shop › orders`). `import a.b` makes `a` visible, `import a.b as c`
//!   the module `a.b` as `c`, `from m import X` the name `X`, `from m import *` everything;
//!   relative imports (`from .orders import Order`) start at the file's package. Imports under
//!   `if TYPE_CHECKING:` count too.
//! - Classes are classes; a class based on `Protocol` is an interface, one based on `Enum`
//!   (`IntEnum`, `StrEnum`, `Flag`, `IntFlag`) an enumeration whose assignments are its
//!   literals, and one based on `ABC` or with `metaclass=ABCMeta` abstract. `Generic[T]`,
//!   `Protocol[T]` and `class Box[T]:` give template parameters; other bases are
//!   generalizations.
//! - A class's annotated assignments are attributes (`ClassVar[T]` static, `Final[T]`
//!   read-only), unannotated ones static attributes without a type. `self.x = …` and
//!   `self.x: T = …` directly in `__init__` are attributes too, unless the class declares them.
//! - Functions in a class are operations without their first parameter (`self`, `cls`), unless
//!   `@staticmethod`; `__init__` is the constructor. `@staticmethod` and `@classmethod` make
//!   them static, `@abstractmethod` abstract, `@final` a leaf; all decorators are kept as
//!   written in `umlcraft.python.decorators`, `async` in `umlcraft.python.async`. `*args` is
//!   `args` with the modifier `...`, `**kwargs` `kwargs` with `**`.
//! - What has no annotation has no type. `X | None` and `Optional[X]` are optional, string
//!   annotations (`"Order"`) are read as the type they name.
//! - Names: `__x` is private, `_x` protected, everything else (`__init__` too) public.
//! - Docstrings are documentation, cleaned like `inspect.cleandoc`.
//! - Left out and counted: function bodies (not those of stubs: `...`, `pass`, a docstring),
//!   module-level functions and variables, other statements, enum values.

pub(crate) mod rules;

use tree_sitter::Node;
use umlcraft_model::{ParameterDirection, Visibility};

use crate::comment;
use crate::ir::{
    Field, Import, Item, Literal, Member, Operation, Param, Relation, Scope, Supertype,
    TemplateParam, TypeDecl, TypeExpr, TypeKind, TypeName,
};
use crate::lang::{Frontend, Skip, Skipped};

const DECORATORS_TAG: &str = "umlcraft.python.decorators";
const ASYNC_TAG: &str = "umlcraft.python.async";

/// Bases that make a class an enumeration.
const ENUM_BASES: &[&str] = &["Enum", "IntEnum", "StrEnum", "Flag", "IntFlag"];

pub struct Python;

impl Frontend for Python {
    fn lower(&self, root: Node, text: &str, module: &[String], skipped: &mut Skipped) -> Scope {
        // A package's `__init__` file is the package; relative imports start there.
        let (module, package) = match module.split_last() {
            Some((last, package)) if last == "__init__" => (package, package),
            Some((_, package)) => (module, package),
            None => (module, module),
        };
        let mut reader = Reader { src: text, package, skipped };
        let mut scope = Scope::default();
        reader.module(&named_children(root), &mut scope);
        if module.is_empty() {
            return scope;
        }
        Scope { imports: Vec::new(), items: vec![Item::Namespace { path: module.to_vec(), scope }] }
    }
}

struct Reader<'a> {
    src: &'a str,
    /// The package of the file, where relative imports start.
    package: &'a [String],
    skipped: &'a mut Skipped,
}

fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Text with its whitespace runs made single blanks.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python's naming conventions: `__x` is private (name mangling), `_x` protected.
fn visibility(name: &str) -> Visibility {
    if name.starts_with("__") && !name.ends_with("__") {
        Visibility::Private
    } else if name.starts_with('_') && !name.starts_with("__") {
        Visibility::Protected
    } else {
        Visibility::Public
    }
}

/// The last segment of a dotted name: `typing.Protocol` is `Protocol`.
fn last_segment(text: &str) -> &str {
    text.rsplit('.').next().unwrap_or(text)
}

impl<'a> Reader<'a> {
    fn text(&self, node: Node) -> &'a str {
        &self.src[node.byte_range()]
    }

    fn count(&mut self, skip: Skip) {
        *self.skipped.entry(skip).or_default() += 1;
    }

    /// The statements of a module, into `scope`.
    fn module(&mut self, nodes: &[Node], scope: &mut Scope) {
        for (i, &node) in nodes.iter().enumerate() {
            match node.kind() {
                "import_statement" => scope.imports.extend(self.import(node)),
                "import_from_statement" => scope.imports.extend(self.import_from(node)),
                "future_import_statement" => {}
                "class_definition" | "decorated_definition" => {
                    if let Some(t) = self.definition(node).and_then(|d| self.class(d.0, &d.1)) {
                        scope.items.push(Item::Type(t));
                    } else if self.definition(node).is_some() {
                        self.count(Skip::FreeMember);
                    }
                }
                "function_definition" => self.count(Skip::FreeMember),
                // The module's docstring.
                "expression_statement" if i == 0 && self.docstring_node(node).is_some() => {}
                "expression_statement" if self.assigned_name(node).is_some() => {
                    self.count(Skip::FreeMember)
                }
                // `if TYPE_CHECKING:` holds imports for annotations.
                "if_statement" if self.is_type_checking(node) => {
                    if let Some(block) = node.child_by_field_name("consequence") {
                        self.module(&named_children(block), scope);
                    }
                }
                "comment" => {}
                _ => self.count(Skip::Statement),
            }
        }
    }

    fn is_type_checking(&self, node: Node) -> bool {
        node.child_by_field_name("condition")
            .is_some_and(|c| last_segment(self.text(c)) == "TYPE_CHECKING")
    }

    /// The class or function a definition is, with its decorators as written (without `@`).
    fn definition<'t>(&self, node: Node<'t>) -> Option<(Node<'t>, Vec<String>)> {
        match node.kind() {
            "decorated_definition" => {
                let decorators = named_children(node)
                    .into_iter()
                    .filter(|n| n.kind() == "decorator")
                    .map(|d| squeeze(self.text(d).trim_start_matches('@').trim()))
                    .collect();
                Some((node.child_by_field_name("definition")?, decorators))
            }
            _ => Some((node, Vec::new())),
        }
        .filter(|(d, _)| matches!(d.kind(), "class_definition" | "function_definition"))
    }

    /// `import a.b` makes `a` visible; `import a.b as c` the module `a.b` as `c`.
    fn import(&self, node: Node) -> Vec<Import> {
        named_children(node)
            .into_iter()
            .filter_map(|n| match n.kind() {
                "dotted_name" => {
                    let first = self.dotted(n).into_iter().next()?;
                    Some(Import {
                        path: vec![first],
                        wildcard: false,
                        is_static: false,
                        alias: None,
                    })
                }
                "aliased_import" => Some(Import {
                    path: self.dotted(n.child_by_field_name("name")?),
                    wildcard: false,
                    is_static: false,
                    alias: Some(self.text(n.child_by_field_name("alias")?).to_owned()),
                }),
                _ => None,
            })
            .collect()
    }

    /// `from m import X, Y as Z` or `from m import *`, relative ones made absolute.
    fn import_from(&self, node: Node) -> Vec<Import> {
        let Some(module) = node.child_by_field_name("module_name") else { return Vec::new() };
        let module = match module.kind() {
            "relative_import" => {
                let parts = named_children(module);
                let dots = parts
                    .iter()
                    .find(|n| n.kind() == "import_prefix")
                    .map_or(0, |p| self.text(*p).trim().len());
                // `.` is the package itself, every further dot one package up.
                let up = dots.saturating_sub(1).min(self.package.len());
                let mut path = self.package[..self.package.len() - up].to_vec();
                if let Some(name) = parts.iter().find(|n| n.kind() == "dotted_name") {
                    path.extend(self.dotted(*name));
                }
                path
            }
            _ => self.dotted(module),
        };
        let mut imports = Vec::new();
        let mut cursor = node.walk();
        for (i, n) in node.children(&mut cursor).enumerate() {
            let field = node.field_name_for_child(i as u32);
            match (field, n.kind()) {
                (_, "wildcard_import") => imports.push(Import {
                    path: module.clone(),
                    wildcard: true,
                    is_static: false,
                    alias: None,
                }),
                (Some("name"), "dotted_name") => imports.push(Import {
                    path: module.iter().cloned().chain(self.dotted(n)).collect(),
                    wildcard: false,
                    is_static: false,
                    alias: None,
                }),
                (Some("name"), "aliased_import") => {
                    let (Some(name), Some(alias)) =
                        (n.child_by_field_name("name"), n.child_by_field_name("alias"))
                    else {
                        continue;
                    };
                    imports.push(Import {
                        path: module.iter().cloned().chain(self.dotted(name)).collect(),
                        wildcard: false,
                        is_static: false,
                        alias: Some(self.text(alias).to_owned()),
                    });
                }
                _ => {}
            }
        }
        imports
    }

    fn dotted(&self, node: Node) -> Vec<String> {
        self.text(node).split('.').map(|s| s.trim().to_owned()).collect()
    }

    /// The string of an expression statement that is only a string: a docstring.
    fn docstring_node<'t>(&self, statement: Node<'t>) -> Option<Node<'t>> {
        comment::docstring_node(statement)
    }

    /// The text of a string literal, without prefix and quotes.
    fn string_content(&self, string: Node) -> String {
        comment::string_content(string, self.src)
    }

    /// The docstring of a block: its first statement, if that is a string.
    fn docstring(&self, block: Option<Node>) -> String {
        block.and_then(|b| comment::docstring(b, self.src)).map(|(doc, _)| doc).unwrap_or_default()
    }

    /// The name an assignment statement assigns to, if it is a single name (`x = 1`, `x: int`).
    fn assigned_name(&self, statement: Node) -> Option<String> {
        let assignment =
            named_children(statement).into_iter().find(|n| n.kind() == "assignment")?;
        let left = assignment.child_by_field_name("left")?;
        (left.kind() == "identifier").then(|| self.text(left).to_owned())
    }

    fn class(&mut self, node: Node, decorators: &[String]) -> Option<TypeDecl> {
        if node.kind() != "class_definition" {
            return None;
        }
        let name = self.text(node.child_by_field_name("name")?);
        let mut t = TypeDecl::new(name, TypeKind::Class);
        t.line = node.start_position().row + 1;
        t.visibility = visibility(name);
        let body = node.child_by_field_name("body");
        t.documentation = self.docstring(body);
        if !decorators.is_empty() {
            t.tags.push((DECORATORS_TAG.to_owned(), decorators.join("\n")));
        }
        t.is_leaf = decorators.iter().any(|d| last_segment(d) == "final");
        if let Some(params) = node.child_by_field_name("type_parameters") {
            t.template_parameters = self.type_parameters(params);
        }
        if let Some(bases) = node.child_by_field_name("superclasses") {
            self.bases(bases, &mut t);
        }
        let Some(body) = body else { return Some(t) };
        let statements = named_children(body);
        for (i, &statement) in statements.iter().enumerate() {
            match statement.kind() {
                "expression_statement" if i == 0 && self.docstring_node(statement).is_some() => {}
                "expression_statement" => self.class_assignment(statement, &mut t),
                "class_definition" | "decorated_definition" | "function_definition" => {
                    let Some((definition, decorators)) = self.definition(statement) else {
                        self.count(Skip::Statement);
                        continue;
                    };
                    if definition.kind() == "class_definition" {
                        if let Some(nested) = self.class(definition, &decorators) {
                            t.members.push(Member::Type(nested));
                        }
                    } else if let Some(o) = self.function(definition, &decorators, &mut t) {
                        t.members.push(Member::Operation(o));
                    }
                }
                "pass_statement" | "comment" => {}
                _ => self.count(Skip::Statement),
            }
        }
        Some(t)
    }

    /// The bases of a class: generalizations, and what they say about the class itself.
    fn bases(&mut self, list: Node, t: &mut TypeDecl) {
        for base in named_children(list) {
            match base.kind() {
                "keyword_argument" => {
                    let name = base.child_by_field_name("name").map(|n| self.text(n));
                    let value = base.child_by_field_name("value").map(|v| self.text(v));
                    if name == Some("metaclass")
                        && value.is_some_and(|v| last_segment(v) == "ABCMeta")
                    {
                        t.is_abstract = true;
                    }
                }
                // `Generic[T]`, `Protocol[T]`: template parameters.
                "subscript" => {
                    let Some(value) = base.child_by_field_name("value") else { continue };
                    let name = last_segment(self.text(value));
                    if matches!(name, "Generic" | "Protocol") {
                        if name == "Protocol" {
                            t.kind = TypeKind::Interface;
                        }
                        for p in named_children(base).into_iter().filter(|&n| n != value) {
                            let name = self.text(p).to_owned();
                            if !t.template_parameters.iter().any(|t| t.name == name) {
                                t.template_parameters
                                    .push(TemplateParam { name, ..Default::default() });
                            }
                        }
                    } else if let Some(ty) = self.type_expr(base) {
                        t.supertypes.push(supertype(ty));
                    }
                }
                _ => {
                    let text = self.text(base);
                    match last_segment(text) {
                        "Protocol" => t.kind = TypeKind::Interface,
                        "ABC" => t.is_abstract = true,
                        "object" | "Generic" => {}
                        name if ENUM_BASES.contains(&name) => t.kind = TypeKind::Enum,
                        _ => {
                            if let Some(ty) = self.type_expr(base) {
                                t.supertypes.push(supertype(ty));
                            }
                        }
                    }
                }
            }
        }
    }

    /// PEP 695: `class Box[T, *Ts]:`, `class Box[T: Comparable]:`. Each parameter is a `type`
    /// around its name, or around a `constrained_type` of the name and its bound.
    fn type_parameters(&mut self, node: Node) -> Vec<TemplateParam> {
        named_children(node)
            .into_iter()
            .map(|p| {
                let inner = named_children(p).into_iter().next().unwrap_or(p);
                let (name, bound) = match inner.kind() {
                    "constrained_type" => {
                        let parts = named_children(inner);
                        (parts.first().copied().unwrap_or(inner), parts.get(1).copied())
                    }
                    _ => (inner, None),
                };
                let text = self.text(name);
                let bounds = bound
                    .map(|b| {
                        self.type_expr(b)
                            .unwrap_or_else(|| TypeExpr::builtin(squeeze(self.text(b))))
                    })
                    .into_iter()
                    .collect();
                TemplateParam {
                    name: text.trim_start_matches('*').to_owned(),
                    is_variadic: text.starts_with('*'),
                    bounds,
                    ..Default::default()
                }
            })
            .collect()
    }

    /// An assignment in a class body: an attribute, or in an enumeration a literal.
    fn class_assignment(&mut self, statement: Node, t: &mut TypeDecl) {
        let assignment = named_children(statement).into_iter().find(|n| n.kind() == "assignment");
        let Some(assignment) = assignment else {
            // A bare string after the docstring, `...`: nothing.
            if self.docstring_node(statement).is_none() && self.text(statement) != "..." {
                self.count(Skip::Statement);
            }
            return;
        };
        let Some(name) = self.assigned_name(statement) else {
            self.count(Skip::Statement);
            return;
        };
        let value = assignment.child_by_field_name("right");
        if t.kind == TypeKind::Enum {
            if value.is_some() {
                self.count(Skip::EnumValue);
            }
            t.members.push(Member::Literal(Literal { name, documentation: String::new() }));
            return;
        }
        let annotation = assignment.child_by_field_name("type");
        let mut field = Field {
            visibility: visibility(&name),
            name,
            ty: TypeExpr::unwritten(),
            // Without an annotation, an assignment in a class body is a class variable.
            is_static: annotation.is_none(),
            is_read_only: false,
            default: value.map(|v| squeeze(self.text(v))),
            tags: Vec::new(),
            documentation: String::new(),
        };
        if let Some(annotation) = annotation {
            self.annotate(annotation, &mut field);
        }
        // Declared again, or after `__init__` assigned it (`self.total = 0`): one attribute,
        // as the class body says, with the value assigned first if the body gives none.
        let same = |m: &&mut Member| matches!(m, Member::Field(f) if f.name == field.name);
        if let Some(Member::Field(earlier)) = t.members.iter_mut().find(same) {
            if field.default.is_none() {
                field.default = earlier.default.take();
            }
            *earlier = field;
            return;
        }
        t.members.push(Member::Field(field));
    }

    /// The type of a field from its annotation; `ClassVar[T]` makes it static, `Final[T]`
    /// read-only.
    fn annotate(&mut self, annotation: Node, field: &mut Field) {
        let Some(ty) = self.type_expr(annotation) else { return };
        let wrapper = match &ty.base {
            TypeName::Named(path) => path.last().map(String::as_str),
            _ => None,
        };
        match wrapper {
            Some("ClassVar") => {
                field.is_static = true;
                field.ty = ty.arguments.into_iter().next().unwrap_or_else(TypeExpr::unwritten);
            }
            Some("Final") => {
                field.is_read_only = true;
                field.ty = ty.arguments.into_iter().next().unwrap_or_else(TypeExpr::unwritten);
            }
            _ => field.ty = ty,
        }
    }

    /// A function in class `t` as an operation; `__init__` also adds the attributes it
    /// assigns to `self`.
    fn function(
        &mut self,
        node: Node,
        decorators: &[String],
        t: &mut TypeDecl,
    ) -> Option<Operation> {
        let name = self.text(node.child_by_field_name("name")?).to_owned();
        let has = |d: &str| decorators.iter().any(|x| last_segment(x) == d);
        let is_static = has("staticmethod");
        let is_class = has("classmethod");
        let mut parameters =
            node.child_by_field_name("parameters").map(|p| self.parameters(p)).unwrap_or_default();
        // `self` or `cls`.
        let receiver =
            if is_static || parameters.is_empty() { None } else { Some(parameters.remove(0).name) };
        let is_constructor = name == "__init__";
        let return_type = if is_constructor {
            None
        } else {
            Some(
                node.child_by_field_name("return_type")
                    .and_then(|r| self.type_expr(r))
                    .unwrap_or_else(TypeExpr::unwritten),
            )
        };
        let body = node.child_by_field_name("body");
        if let Some(body) = body {
            if !self.is_stub(body) {
                self.count(Skip::MethodBody);
            }
            if is_constructor && let Some(receiver) = &receiver {
                self.instance_attributes(body, receiver, t);
            }
        }
        let mut tags = Vec::new();
        if !decorators.is_empty() {
            tags.push((DECORATORS_TAG.to_owned(), decorators.join("\n")));
        }
        if self.text(node).starts_with("async") {
            tags.push((ASYNC_TAG.to_owned(), "true".to_owned()));
        }
        Some(Operation {
            visibility: visibility(&name),
            name,
            parameters,
            return_type,
            is_static: is_static || is_class,
            is_abstract: has("abstractmethod"),
            is_leaf: has("final"),
            is_constructor,
            is_query: false,
            template_parameters: node
                .child_by_field_name("type_parameters")
                .map(|p| self.type_parameters(p))
                .unwrap_or_default(),
            tags,
            documentation: self.docstring(body),
            span: node.byte_range(),
        })
    }

    /// Whether a body is only a docstring, `...` or `pass`: a stub's.
    fn is_stub(&self, body: Node) -> bool {
        named_children(body).into_iter().all(|s| match s.kind() {
            "pass_statement" | "comment" => true,
            "expression_statement" => self.text(s) == "..." || self.docstring_node(s).is_some(),
            _ => false,
        })
    }

    /// `self.x = …` and `self.x: T = …` directly in `__init__`: attributes of `t`, unless the
    /// class declares them.
    fn instance_attributes(&mut self, body: Node, receiver: &str, t: &mut TypeDecl) {
        for statement in
            named_children(body).into_iter().filter(|s| s.kind() == "expression_statement")
        {
            let Some(assignment) =
                named_children(statement).into_iter().find(|n| n.kind() == "assignment")
            else {
                continue;
            };
            let Some(left) =
                assignment.child_by_field_name("left").filter(|l| l.kind() == "attribute")
            else {
                continue;
            };
            let object = left.child_by_field_name("object").map(|o| self.text(o));
            let Some(attribute) = left.child_by_field_name("attribute") else { continue };
            if object != Some(receiver) {
                continue;
            }
            let name = self.text(attribute).to_owned();
            let declared =
                t.members.iter().any(|m| matches!(m, Member::Field(f) if f.name == name));
            if declared {
                continue;
            }
            let mut field = Field {
                visibility: visibility(&name),
                name,
                ty: TypeExpr::unwritten(),
                is_static: false,
                is_read_only: false,
                default: assignment.child_by_field_name("right").map(|v| squeeze(self.text(v))),
                tags: Vec::new(),
                documentation: String::new(),
            };
            if let Some(annotation) = assignment.child_by_field_name("type") {
                self.annotate(annotation, &mut field);
            }
            t.members.push(Member::Field(field));
        }
    }

    /// The parameters of a function, `self` included; `*` and `/` are left out.
    fn parameters(&mut self, list: Node) -> Vec<Param> {
        let mut params = Vec::new();
        for p in named_children(list) {
            let (name, ty, default) = match p.kind() {
                "identifier" => (self.text(p).to_owned(), TypeExpr::unwritten(), None),
                "list_splat_pattern" | "dictionary_splat_pattern" => {
                    (self.splat_name(p), TypeExpr::unwritten().with_modifier(splat(p)), None)
                }
                "default_parameter" | "typed_default_parameter" | "typed_parameter" => {
                    let name_node = p
                        .child_by_field_name("name")
                        .or_else(|| named_children(p).into_iter().find(|n| n.kind() != "type"));
                    let Some(name_node) = name_node else { continue };
                    let mut ty = p
                        .child_by_field_name("type")
                        .and_then(|t| self.type_expr(t))
                        .unwrap_or_else(TypeExpr::unwritten);
                    let name = match name_node.kind() {
                        "list_splat_pattern" | "dictionary_splat_pattern" => {
                            ty = ty.with_modifier(splat(name_node));
                            self.splat_name(name_node)
                        }
                        _ => self.text(name_node).to_owned(),
                    };
                    let default = p.child_by_field_name("value").map(|v| squeeze(self.text(v)));
                    (name, ty, default)
                }
                _ => continue,
            };
            params.push(Param { name, ty, default, direction: ParameterDirection::In });
        }
        params
    }

    fn splat_name(&self, node: Node) -> String {
        named_children(node).into_iter().next().map_or(String::new(), |n| self.text(n).to_owned())
    }

    /// A type annotation as written; `None` for what names no type.
    fn type_expr(&mut self, node: Node) -> Option<TypeExpr> {
        Some(match node.kind() {
            "type" => return self.type_expr(named_children(node).into_iter().next()?),
            "identifier" => TypeExpr::named(vec![self.text(node).to_owned()]),
            "attribute" => TypeExpr::named(self.dotted(node)),
            "none" => TypeExpr::builtin("None"),
            // `list[Line]`, `typing.Optional[Line]`.
            "generic_type" | "subscript" => {
                let parts = named_children(node);
                let base = node.child_by_field_name("value").or_else(|| parts.first().copied())?;
                let mut ty = self.type_expr(base)?;
                let arguments: Vec<Node> = match node.kind() {
                    "generic_type" => parts
                        .iter()
                        .filter(|n| n.kind() == "type_parameter")
                        .flat_map(|p| named_children(*p))
                        .collect(),
                    _ => parts.into_iter().filter(|&n| n != base).collect(),
                };
                ty.arguments = arguments
                    .into_iter()
                    .map(|a| {
                        self.type_expr(a)
                            .unwrap_or_else(|| TypeExpr::builtin(squeeze(self.text(a))))
                    })
                    .collect();
                ty
            }
            // `Line | None` is an optional `Line`; other unions stay text.
            "binary_operator" => {
                let left = node.child_by_field_name("left")?;
                let right = node.child_by_field_name("right")?;
                let other = match (left.kind(), right.kind()) {
                    (_, "none") => left,
                    ("none", _) => right,
                    _ => return Some(TypeExpr::builtin(squeeze(self.text(node)))),
                };
                match self.type_expr(other) {
                    Some(ty) if ty.modifier.is_empty() => ty.with_modifier(" | None"),
                    _ => TypeExpr::builtin(squeeze(self.text(node))),
                }
            }
            // `"Order"`: the annotation inside the string.
            "string" => {
                let content = self.string_content(node);
                let tree =
                    crate::parse_tree(crate::SourceLanguage::Python, format!("_: {content}\n"));
                let annotation = tree
                    .root()
                    .named_child(0)
                    .and_then(|s| s.named_child(0))
                    .and_then(|a| a.child_by_field_name("type"));
                let mut inner =
                    Reader { src: tree.text(), package: self.package, skipped: self.skipped };
                match annotation.filter(|_| !tree.root().has_error()) {
                    Some(a) => {
                        inner.type_expr(a).unwrap_or_else(|| TypeExpr::builtin(squeeze(&content)))
                    }
                    None => TypeExpr::builtin(squeeze(&content)),
                }
            }
            _ => TypeExpr::builtin(squeeze(self.text(node))),
        })
    }
}

fn supertype(ty: TypeExpr) -> Supertype {
    Supertype { ty, relation: Relation::Extends, access: None, is_virtual: false }
}

/// `...` for `*args`, `**` for `**kwargs`.
fn splat(node: Node) -> &'static str {
    if node.kind() == "dictionary_splat_pattern" { "**" } else { "..." }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str, module: &[&str]) -> (String, Skipped) {
        let mut skipped = Skipped::new();
        let module: Vec<String> = module.iter().map(|s| s.to_string()).collect();
        let scope = crate::lower_scope(
            &crate::parse_tree(crate::SourceLanguage::Python, text),
            &module,
            &mut skipped,
        )
        .unwrap();
        let unit = crate::ir::Unit { path: Default::default(), scope };
        (unit.dump(), skipped)
    }

    #[test]
    fn reads_a_small_class() {
        let (dump, skipped) = parse(
            "from .lines import Line\n\nclass Order(Base):\n    \"\"\"An order.\n\n    More.\n    \"\"\"\n    count: ClassVar[int] = 0\n    lines: list[Line]\n\n    def __init__(self, n: int, *args, **kwargs) -> None:\n        self.n = n\n\n    def total(self) -> \"Line | None\":\n        return None\n",
            &["shop", "orders"],
        );
        pretty_assertions::assert_eq!(
            dump,
            "namespace shop.orders\n  import shop.lines.Line\n  public class Order extends Base (line 3)\n    doc \"An order.\\n\\nMore.\"\n    public field static count : int = 0\n    public field lines : list<Line>\n    public field n = n\n    public operation constructor __init__(n : int, args : ..., kwargs : **)\n    public operation total() : Line | None\n"
        );
        assert_eq!(skipped.get(&Skip::MethodBody), Some(&2));
    }

    #[test]
    fn reports_the_first_syntax_error() {
        let mut skipped = Skipped::new();
        let error = crate::lower_scope(
            &crate::parse_tree(crate::SourceLanguage::Python, "class A:\n    x = = 1\n"),
            &[],
            &mut skipped,
        )
        .unwrap_err();
        assert_eq!(error.line, 2);
    }
}
