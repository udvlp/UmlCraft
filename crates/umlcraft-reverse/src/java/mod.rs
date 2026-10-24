//! Java: declarations from `.java` files, lowered as Astah's "Import Java" does.
//!
//! - `package a.b` becomes nested namespaces; without one, types go into the default (root)
//!   package.
//! - Members without a visibility are package-private; interface fields and methods are
//!   public, the fields static and final, the methods abstract unless they are `default`,
//!   `static` or `private` or have a body. Interfaces themselves are always public; nested
//!   types in interfaces are static but keep their own visibility.
//! - `final` makes classes and methods leaves and fields read-only.
//! - Modifiers UML has no place for (`transient`, `volatile`, `native`, `synchronized`,
//!   `strictfp`) and annotations become tagged values `jude.java.*`, as in Astah.
//! - Records are classes whose components are public attributes; `@interface`s are classes
//!   tagged `jude.java.atmark_interface`.
//! - A declaration's documentation is the block or Javadoc comment right before it.

mod javadoc;
mod jdk;
pub(crate) mod rules;

use tree_sitter::{Node, Parser};
use umlcraft_model::Visibility;

use crate::ir::{
    Field, Import, Item, Literal, Member, Operation, Param, Relation, Scope, Supertype,
    TemplateParam, TypeDecl, TypeExpr, TypeKind, TypeName,
};
use crate::lang::{Frontend, Skip, Skipped, SyntaxError, first_error};

/// Tag holding a declaration's annotations as written, one per line.
pub const ANNOTATIONS_TAG: &str = "jude.java.annotations";
/// Tag marking an interface declared as `@interface`.
pub const ANNOTATION_TYPE_TAG: &str = "jude.java.atmark_interface";
/// Modifiers kept as tags `jude.java.<modifier>` with the value `true`.
const TAGGED_MODIFIERS: [&str; 5] = ["strictfp", "transient", "volatile", "native", "synchronized"];

pub struct Java;

impl Frontend for Java {
    fn parse(&self, text: &str, skipped: &mut Skipped) -> Result<Scope, SyntaxError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::LANGUAGE.into())
            .expect("the Java grammar matches the tree-sitter version");
        let tree = parser.parse(text, None).expect("parsing without a timeout always finishes");
        let root = tree.root_node();
        if let Some(error) = first_error(root, text) {
            return Err(error);
        }
        let mut reader = Reader { src: text, skipped };
        Ok(reader.program(root))
    }
}

struct Reader<'a> {
    src: &'a str,
    skipped: &'a mut Skipped,
}

/// Where a declaration sits; it decides the implicit modifiers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    TopLevel,
    Class,
    Interface,
    Enum,
}

/// The modifiers of a declaration.
#[derive(Default)]
struct Modifiers {
    visibility: Option<Visibility>,
    is_static: bool,
    is_abstract: bool,
    is_final: bool,
    is_default: bool,
    tags: Vec<(String, String)>,
    annotations: Vec<String>,
}

impl Modifiers {
    /// The tags: modifiers kept as tags, then the annotations.
    fn into_tags(self) -> Vec<(String, String)> {
        let mut tags = self.tags;
        if !self.annotations.is_empty() {
            tags.push((ANNOTATIONS_TAG.to_owned(), self.annotations.join("\n")));
        }
        tags
    }
}

fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

impl<'a> Reader<'a> {
    fn text(&self, node: Node) -> &'a str {
        &self.src[node.byte_range()]
    }

    fn count(&mut self, skip: Skip) {
        *self.skipped.entry(skip).or_default() += 1;
    }

    fn program(&mut self, root: Node) -> Scope {
        let mut package: Option<Vec<String>> = None;
        let mut imports = Vec::new();
        let mut types = Vec::new();
        for node in named_children(root) {
            match node.kind() {
                "package_declaration" => {
                    let name = named_children(node)
                        .into_iter()
                        .find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"));
                    package = name.map(|n| self.path(n));
                }
                "import_declaration" => imports.push(self.import(node)),
                _ => {
                    if let Some(t) = self.type_decl(node, Context::TopLevel) {
                        types.push(Item::Type(t));
                    }
                }
            }
        }
        let items = match package {
            Some(path) => {
                vec![Item::Namespace { path, scope: Scope { imports: Vec::new(), items: types } }]
            }
            None => types,
        };
        Scope { imports, items }
    }

    /// `a.b.c` as `["a", "b", "c"]`, ignoring whitespace and comments in between.
    fn path(&self, node: Node) -> Vec<String> {
        self.text(node)
            .split('.')
            .map(|s| s.split_whitespace().collect::<String>())
            .filter(|s| !s.is_empty())
            .collect()
    }

    fn import(&self, node: Node) -> Import {
        let parts = children(node);
        let is_static = parts.iter().any(|n| n.kind() == "static");
        let wildcard = parts.iter().any(|n| n.kind() == "asterisk");
        let path = parts
            .iter()
            .find(|n| matches!(n.kind(), "identifier" | "scoped_identifier"))
            .map(|&n| self.path(n))
            .unwrap_or_default();
        Import { path, wildcard, is_static }
    }

    /// The comment right before `node`, as Astah takes it: a Javadoc or block comment counts,
    /// a line comment stands for none.
    fn documentation(&self, node: Node) -> String {
        match node.prev_sibling() {
            Some(p) if p.kind() == "block_comment" => javadoc::clean(self.text(p)),
            _ => String::new(),
        }
    }

    fn modifiers(&self, decl: Node) -> Modifiers {
        let mut m = Modifiers::default();
        let annotation = |n: Node, m: &mut Modifiers| {
            if matches!(n.kind(), "annotation" | "marker_annotation") {
                m.annotations.push(self.text(n).to_owned());
            }
        };
        for child in children(decl) {
            annotation(child, &mut m);
            if child.kind() != "modifiers" {
                continue;
            }
            for n in children(child) {
                annotation(n, &mut m);
                match n.kind() {
                    "public" => m.visibility = Some(Visibility::Public),
                    "protected" => m.visibility = Some(Visibility::Protected),
                    "private" => m.visibility = Some(Visibility::Private),
                    "static" => m.is_static = true,
                    "abstract" => m.is_abstract = true,
                    "final" => m.is_final = true,
                    "default" => m.is_default = true,
                    k if TAGGED_MODIFIERS.contains(&k) => {
                        m.tags.push((format!("jude.java.{k}"), "true".to_owned()));
                    }
                    _ => {}
                }
            }
        }
        m
    }

    fn type_decl(&mut self, node: Node, context: Context) -> Option<TypeDecl> {
        let kind = match node.kind() {
            // Astah makes annotation types classes, tagged as such.
            "class_declaration" | "record_declaration" | "annotation_type_declaration" => {
                TypeKind::Class
            }
            "interface_declaration" => TypeKind::Interface,
            "enum_declaration" => TypeKind::Enum,
            _ => return None,
        };
        let name = self.text(node.child_by_field_name("name")?);
        let mut t = TypeDecl::new(name, kind);
        t.line = node.start_position().row + 1;
        t.documentation = self.documentation(node);
        let m = self.modifiers(node);
        // Astah makes every interface public, and other types package-private unless declared
        // otherwise, even inside interfaces.
        t.visibility = if kind == TypeKind::Interface {
            Visibility::Public
        } else {
            m.visibility.unwrap_or(Visibility::Package)
        };
        t.is_abstract = m.is_abstract;
        t.is_leaf = m.is_final;
        t.is_static = m.is_static || context == Context::Interface;
        t.tags = m.into_tags();
        if node.kind() == "annotation_type_declaration" {
            t.tags.push((ANNOTATION_TYPE_TAG.to_owned(), "true".to_owned()));
        }
        if let Some(params) = node.child_by_field_name("type_parameters") {
            t.template_parameters = self.type_parameters(params);
        }
        if let Some(superclass) = node.child_by_field_name("superclass") {
            for ty in named_children(superclass) {
                if let Some(ty) = self.type_expr(ty) {
                    t.supertypes.push(supertype(ty, Relation::Extends));
                }
            }
        }
        let interfaces = node.child_by_field_name("interfaces").map(|n| (n, Relation::Implements));
        // An interface's `extends` lists more interfaces: generalizations.
        let extended = named_children(node)
            .into_iter()
            .find(|n| n.kind() == "extends_interfaces")
            .map(|n| (n, Relation::Extends));
        for (list, relation) in interfaces.into_iter().chain(extended) {
            for type_list in named_children(list) {
                for ty in named_children(type_list) {
                    if let Some(ty) = self.type_expr(ty) {
                        t.supertypes.push(supertype(ty, relation));
                    }
                }
            }
        }
        // Record components: public attributes, as Astah has them.
        if node.kind() == "record_declaration"
            && let Some(params) = node.child_by_field_name("parameters")
        {
            for p in self.parameters(params) {
                t.members.push(Member::Field(Field {
                    name: p.name,
                    ty: p.ty,
                    visibility: Visibility::Public,
                    is_static: false,
                    is_read_only: false,
                    default: None,
                    tags: Vec::new(),
                    documentation: String::new(),
                }));
            }
        }
        let inner = match kind {
            _ if node.kind() == "annotation_type_declaration" => Context::Interface,
            TypeKind::Interface => Context::Interface,
            TypeKind::Enum => Context::Enum,
            _ => Context::Class,
        };
        if let Some(body) = node.child_by_field_name("body") {
            self.body(body, inner, node, &mut t.members);
        }
        Some(t)
    }

    fn body(&mut self, body: Node, context: Context, owner: Node, members: &mut Vec<Member>) {
        for node in named_children(body) {
            match node.kind() {
                "enum_constant" => {
                    let name = node.child_by_field_name("name").map(|n| self.text(n));
                    if node.child_by_field_name("arguments").is_some()
                        || node.child_by_field_name("body").is_some()
                    {
                        self.count(Skip::EnumConstantBody);
                    }
                    if let Some(name) = name {
                        members.push(Member::Literal(Literal {
                            name: name.to_owned(),
                            documentation: self.documentation(node),
                        }));
                    }
                }
                // An enum's members after its constants.
                "enum_body_declarations" => self.body(node, context, owner, members),
                "field_declaration" | "constant_declaration" => {
                    members.extend(self.fields(node, context).into_iter().map(Member::Field));
                }
                "method_declaration"
                | "constructor_declaration"
                | "compact_constructor_declaration"
                | "annotation_type_element_declaration" => {
                    if let Some(o) = self.operation(node, context, owner) {
                        members.push(Member::Operation(o));
                    }
                }
                "static_initializer" | "block" => self.count(Skip::Initializer),
                _ => {
                    if let Some(t) = self.type_decl(node, context) {
                        members.push(Member::Type(t));
                    }
                }
            }
        }
    }

    /// One field per declarator: `int a, b[];` is `a : int` and `b : int[]`.
    fn fields(&mut self, node: Node, context: Context) -> Vec<Field> {
        let Some(ty) = node.child_by_field_name("type").and_then(|t| self.type_expr(t)) else {
            return Vec::new();
        };
        let documentation = self.documentation(node);
        let m = self.modifiers(node);
        let interface = context == Context::Interface;
        let visibility = m.visibility.unwrap_or(if interface {
            Visibility::Public
        } else {
            Visibility::Package
        });
        let (is_static, is_read_only) = (m.is_static || interface, m.is_final || interface);
        let tags = m.into_tags();
        let mut cursor = node.walk();
        let declarators: Vec<Node> =
            node.children_by_field_name("declarator", &mut cursor).collect();
        declarators
            .into_iter()
            .filter_map(|d| {
                let name = self.text(d.child_by_field_name("name")?).to_owned();
                let ty = match d.child_by_field_name("dimensions") {
                    Some(dims) => ty.clone().with_modifier(&dimensions(self.text(dims))),
                    None => ty.clone(),
                };
                let value = d.child_by_field_name("value");
                if let Some(value) = value {
                    self.count_nested_classes(value);
                }
                let default = value.map(|v| self.text(v).to_owned());
                Some(Field {
                    name,
                    ty,
                    visibility,
                    is_static,
                    is_read_only,
                    default,
                    tags: tags.clone(),
                    documentation: documentation.clone(),
                })
            })
            .collect()
    }

    fn operation(&mut self, node: Node, context: Context, owner: Node) -> Option<Operation> {
        let name = self.text(node.child_by_field_name("name")?).to_owned();
        let documentation = self.documentation(node);
        let m = self.modifiers(node);
        let interface = context == Context::Interface;
        let is_constructor =
            matches!(node.kind(), "constructor_declaration" | "compact_constructor_declaration");
        let body = node.child_by_field_name("body");
        if let Some(body) = body {
            self.count(Skip::MethodBody);
            self.count_nested_classes(body);
        }
        // Enum constructors stay package-private as written, as in Astah.
        let implicit = if interface { Visibility::Public } else { Visibility::Package };
        let visibility = m.visibility.unwrap_or(implicit);
        let is_private = visibility == Visibility::Private;
        let is_abstract = m.is_abstract
            || (interface && body.is_none() && !m.is_default && !m.is_static && !is_private);
        let return_type = if is_constructor {
            None
        } else {
            let ty = node.child_by_field_name("type").and_then(|t| self.type_expr(t))?;
            // `int f()[]`, the old way to return an array.
            Some(match node.child_by_field_name("dimensions") {
                Some(dims) => ty.with_modifier(&dimensions(self.text(dims))),
                None => ty,
            })
        };
        let parameters = match node.child_by_field_name("parameters") {
            Some(params) => self.parameters(params),
            // A record's compact constructor takes the components.
            None if node.kind() == "compact_constructor_declaration" => owner
                .child_by_field_name("parameters")
                .map(|p| self.parameters(p))
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let template_parameters = node
            .child_by_field_name("type_parameters")
            .map(|p| self.type_parameters(p))
            .unwrap_or_default();
        Some(Operation {
            name,
            parameters,
            return_type,
            visibility,
            is_static: m.is_static,
            is_abstract,
            is_leaf: m.is_final,
            is_constructor,
            template_parameters,
            tags: m.into_tags(),
            documentation,
        })
    }

    /// Counts the classes declared in a method body, local and anonymous; they are left out.
    fn count_nested_classes(&mut self, body: Node) {
        for child in named_children(body) {
            match child.kind() {
                "class_declaration"
                | "record_declaration"
                | "enum_declaration"
                | "interface_declaration" => self.count(Skip::LocalClass),
                "class_body"
                    if child.parent().is_some_and(|p| p.kind() == "object_creation_expression") =>
                {
                    self.count(Skip::AnonymousClass)
                }
                _ => {}
            }
            self.count_nested_classes(child);
        }
    }

    fn parameters(&mut self, node: Node) -> Vec<Param> {
        named_children(node)
            .into_iter()
            .filter_map(|p| match p.kind() {
                "formal_parameter" => {
                    let name = self.text(p.child_by_field_name("name")?).to_owned();
                    let ty = self.type_expr(p.child_by_field_name("type")?)?;
                    let ty = match p.child_by_field_name("dimensions") {
                        Some(dims) => ty.with_modifier(&dimensions(self.text(dims))),
                        None => ty,
                    };
                    Some(Param { name, ty, default: None })
                }
                // `String... names`: the type, then a declarator with the name.
                "spread_parameter" => {
                    let parts = named_children(p);
                    let ty = parts.iter().find_map(|&n| self.type_expr(n))?;
                    let declarator = parts.iter().find(|n| n.kind() == "variable_declarator")?;
                    let name = self.text(declarator.child_by_field_name("name")?).to_owned();
                    Some(Param { name, ty: ty.with_modifier("..."), default: None })
                }
                _ => None,
            })
            .collect()
    }

    fn type_parameters(&mut self, node: Node) -> Vec<TemplateParam> {
        named_children(node)
            .into_iter()
            .filter(|n| n.kind() == "type_parameter")
            .filter_map(|p| {
                let parts = named_children(p);
                let name = parts.iter().find(|n| n.kind() == "type_identifier")?;
                let bounds = parts
                    .iter()
                    .filter(|n| n.kind() == "type_bound")
                    .flat_map(|&b| named_children(b))
                    .filter_map(|b| self.type_expr(b))
                    .collect();
                Some(TemplateParam {
                    name: self.text(*name).to_owned(),
                    bounds,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// A type as written; `None` for nodes that are no type (annotations, comments).
    fn type_expr(&mut self, node: Node) -> Option<TypeExpr> {
        Some(match node.kind() {
            "type_identifier" | "identifier" => TypeExpr::named(vec![self.text(node).to_owned()]),
            // `Map.Entry`, `a.b.C`. The arguments of a qualifying type (`Outer<A>.Inner`) are
            // dropped; the last segment's come with the `generic_type` around this one.
            "scoped_type_identifier" | "scoped_identifier" => TypeExpr::named(
                named_children(node)
                    .into_iter()
                    .filter_map(|part| self.type_expr(part))
                    .flat_map(|inner| match inner.base {
                        TypeName::Named(segments) => segments,
                        TypeName::Builtin(_) => Vec::new(),
                    })
                    .collect(),
            ),
            "generic_type" => {
                let parts = named_children(node);
                let mut ty = parts.iter().find_map(|&n| {
                    matches!(n.kind(), "type_identifier" | "scoped_type_identifier")
                        .then(|| self.type_expr(n))
                        .flatten()
                })?;
                if let Some(args) = parts.iter().find(|n| n.kind() == "type_arguments") {
                    ty.arguments = named_children(*args)
                        .into_iter()
                        .filter_map(|a| self.type_expr(a))
                        .collect();
                }
                ty
            }
            "array_type" => {
                let element = self.type_expr(node.child_by_field_name("element")?)?;
                let dims = node.child_by_field_name("dimensions").map(|d| self.text(d));
                element.with_modifier(&dimensions(dims.unwrap_or("[]")))
            }
            "integral_type" | "floating_point_type" | "boolean_type" | "void_type" => {
                TypeExpr::builtin(self.text(node))
            }
            "annotated_type" => {
                return named_children(node)
                    .into_iter()
                    .filter(|n| !matches!(n.kind(), "annotation" | "marker_annotation"))
                    .find_map(|n| self.type_expr(n));
            }
            "wildcard" => {
                let parts = children(node);
                let bound = named_children(node).into_iter().find_map(|n| self.type_expr(n));
                match bound {
                    Some(mut ty) => {
                        let keyword = if parts.iter().any(|n| n.kind() == "super") {
                            "super"
                        } else {
                            "extends"
                        };
                        ty.prefix = format!("? {keyword} {}", ty.prefix);
                        ty
                    }
                    None => TypeExpr::builtin("?"),
                }
            }
            _ => return None,
        })
    }
}

fn supertype(ty: TypeExpr, relation: Relation) -> Supertype {
    Supertype { ty, relation, access: None, is_virtual: false }
}

/// `[] []` or `@A []` as `[][]`.
fn dimensions(text: &str) -> String {
    "[]".repeat(text.matches('[').count().max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> (String, Skipped) {
        let mut skipped = Skipped::new();
        let scope = Java.parse(text, &mut skipped).unwrap();
        let unit = crate::ir::Unit { path: Default::default(), scope };
        (unit.dump(), skipped)
    }

    #[test]
    fn reads_a_small_class() {
        let (dump, skipped) = parse(
            "package a.b;\nimport java.util.*;\n/** An order. */\npublic class Order<T extends Number> extends Base implements Comparable<Order<T>> {\n  private final java.util.List<? extends T> items = null, more[];\n  int count;\n  public static <K> K pick(K... ks) { class Local {} return null; }\n}\n",
        );
        pretty_assertions::assert_eq!(
            dump,
            "import java.util.*\nnamespace a.b\n  public class Order<T extends Number> extends Base implements Comparable<Order<T>> (line 4)\n    doc \"An order.\"\n    private field readonly items : java.util.List<? extends T> = null\n    private field readonly more : java.util.List<? extends T>[]\n    package field count : int\n    public operation static pick<K>(ks : K...) : K\n"
        );
        assert_eq!(skipped.get(&Skip::MethodBody), Some(&1));
        assert_eq!(skipped.get(&Skip::LocalClass), Some(&1));
    }

    #[test]
    fn reports_the_first_syntax_error() {
        let mut skipped = Skipped::new();
        let error = Java.parse("class A {\n  int x = ;\n}\n", &mut skipped).unwrap_err();
        assert_eq!((error.line, error.column, error.token.as_str()), (2, 9, "="));
    }
}
