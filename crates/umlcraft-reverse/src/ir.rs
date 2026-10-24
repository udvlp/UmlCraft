//! The intermediate representation every front-end lowers its language to: declarations only,
//! with names as written. Resolving names to model elements is the mapping's job
//! ([`crate::resolve`]), so nothing here refers to the model, and language sugar (properties,
//! implicit modifiers) is already lowered.

use std::fmt::{self, Write as _};
use std::path::PathBuf;

use umlcraft_model::Visibility;

/// One parsed source file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Unit {
    pub path: PathBuf,
    pub scope: Scope,
}

/// Declarations together with the imports that apply to them (and to nested scopes).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scope {
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A package or namespace, as a path from the enclosing scope (`a.b` is `["a", "b"]`).
    /// Namespaces may be opened again in other files or further down the same file.
    Namespace {
        path: Vec<String>,
        scope: Scope,
    },
    Type(TypeDecl),
    /// A member defined outside its type, such as C++'s `void A::f() {}`; `owner` is the type's
    /// qualified name as written.
    Definition {
        owner: Vec<String>,
        member: Member,
    },
    /// `typedef` / `using X = …`: a name standing for a type.
    Alias {
        name: String,
        target: TypeExpr,
    },
    /// A function or variable outside any type (C++). UML has no place for them.
    Free(Member),
}

/// An import as written: `java.util.List` (`path` = `["java", "util", "List"]`) or
/// `java.util.*` (`wildcard`).
#[derive(Clone, Debug, PartialEq)]
pub struct Import {
    pub path: Vec<String>,
    pub wildcard: bool,
    /// `import static …`: imports members, not types.
    pub is_static: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    /// A C/C++ struct or C# struct: a class whose members are public by default.
    Struct,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub kind: TypeKind,
    pub visibility: Visibility,
    pub is_abstract: bool,
    /// Cannot be specialized (`final`, `sealed`).
    pub is_leaf: bool,
    pub is_static: bool,
    /// Only declares the name (C++ `class A;`); a full declaration elsewhere wins.
    pub is_forward: bool,
    /// One part of a type declared in several places (C# `partial`).
    pub is_partial: bool,
    pub template_parameters: Vec<TemplateParam>,
    pub supertypes: Vec<Supertype>,
    pub members: Vec<Member>,
    pub tags: Vec<(String, String)>,
    pub documentation: String,
    /// 1-based line of the declaration, for messages.
    pub line: usize,
}

impl TypeDecl {
    pub fn new(name: impl Into<String>, kind: TypeKind) -> Self {
        Self {
            name: name.into(),
            kind,
            visibility: Visibility::Package,
            is_abstract: false,
            is_leaf: false,
            is_static: false,
            is_forward: false,
            is_partial: false,
            template_parameters: Vec::new(),
            supertypes: Vec::new(),
            members: Vec::new(),
            tags: Vec::new(),
            documentation: String::new(),
            line: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    /// Generalization: `extends`, or a base class.
    Extends,
    /// Realization: `implements`.
    Implements,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Supertype {
    pub ty: TypeExpr,
    pub relation: Relation,
    /// C++'s `class B : protected A`.
    pub access: Option<Visibility>,
    /// C++'s `virtual` base.
    pub is_virtual: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Member {
    Field(Field),
    Operation(Operation),
    Literal(Literal),
    Type(TypeDecl),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: TypeExpr,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_read_only: bool,
    /// The initializer as written.
    pub default: Option<String>,
    pub tags: Vec<(String, String)>,
    pub documentation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub name: String,
    pub parameters: Vec<Param>,
    /// `None` for constructors (and destructors).
    pub return_type: Option<TypeExpr>,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_abstract: bool,
    pub is_leaf: bool,
    pub is_constructor: bool,
    pub template_parameters: Vec<TemplateParam>,
    pub tags: Vec<(String, String)>,
    pub documentation: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: TypeExpr,
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Literal {
    pub name: String,
    pub documentation: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TemplateParam {
    pub name: String,
    /// Set for a value parameter (C++ `int N`): the type of the value.
    pub value_type: Option<TypeExpr>,
    pub bounds: Vec<TypeExpr>,
    pub default: Option<TypeExpr>,
    pub is_variadic: bool,
}

/// A type as written: `prefix base<arguments> modifier`, e.g. `? extends Number`,
/// `Map<String, List<Foo>>`, `int[]`, `const char*`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeExpr {
    pub prefix: String,
    pub base: TypeName,
    pub arguments: Vec<TypeExpr>,
    pub modifier: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeName {
    /// A name to resolve, as its segments: `Map.Entry` is `["Map", "Entry"]`.
    Named(Vec<String>),
    /// A built-in type or anything else that is kept as text: `int`, `void`, `?`.
    Builtin(String),
}

impl TypeExpr {
    pub fn named(path: Vec<String>) -> Self {
        Self::of(TypeName::Named(path))
    }

    pub fn builtin(name: impl Into<String>) -> Self {
        Self::of(TypeName::Builtin(name.into()))
    }

    fn of(base: TypeName) -> Self {
        Self { prefix: String::new(), base, arguments: Vec::new(), modifier: String::new() }
    }

    pub fn with_modifier(mut self, modifier: &str) -> Self {
        self.modifier.push_str(modifier);
        self
    }
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.prefix)?;
        match &self.base {
            TypeName::Named(path) => f.write_str(&path.join("."))?,
            TypeName::Builtin(name) => f.write_str(name)?,
        }
        if !self.arguments.is_empty() {
            let args: Vec<String> = self.arguments.iter().map(ToString::to_string).collect();
            write!(f, "<{}>", args.join(","))?;
        }
        f.write_str(&self.modifier)
    }
}

impl Unit {
    /// A stable text form, one declaration per line, for snapshot tests.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        dump_scope(&mut out, &self.scope, 0);
        out
    }
}

fn indent(out: &mut String, depth: usize) {
    out.extend(std::iter::repeat_n("  ", depth));
}

fn dump_scope(out: &mut String, scope: &Scope, depth: usize) {
    for import in &scope.imports {
        indent(out, depth);
        let _ = writeln!(
            out,
            "import {}{}{}",
            if import.is_static { "static " } else { "" },
            import.path.join("."),
            if import.wildcard { ".*" } else { "" }
        );
    }
    for item in &scope.items {
        dump_item(out, item, depth);
    }
}

fn dump_item(out: &mut String, item: &Item, depth: usize) {
    match item {
        Item::Namespace { path, scope } => {
            indent(out, depth);
            let _ = writeln!(out, "namespace {}", path.join("."));
            dump_scope(out, scope, depth + 1);
        }
        Item::Type(t) => dump_type(out, t, depth),
        Item::Definition { owner, member } => {
            indent(out, depth);
            let _ = writeln!(out, "definition in {}", owner.join("::"));
            dump_member(out, member, depth + 1);
        }
        Item::Alias { name, target } => {
            indent(out, depth);
            let _ = writeln!(out, "alias {name} = {target}");
        }
        Item::Free(member) => {
            indent(out, depth);
            let _ = writeln!(out, "free");
            dump_member(out, member, depth + 1);
        }
    }
}

fn visibility(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "public",
        Visibility::Protected => "protected",
        Visibility::Package => "package",
        Visibility::Private => "private",
    }
}

/// `" a b"` for the flags that are set.
fn flags(list: &[(bool, &str)]) -> String {
    list.iter().filter(|(on, _)| *on).map(|(_, name)| format!(" {name}")).collect()
}

fn template_params(params: &[TemplateParam]) -> String {
    if params.is_empty() {
        return String::new();
    }
    let list: Vec<String> = params
        .iter()
        .map(|p| {
            let mut s = p.name.clone();
            if p.is_variadic {
                s.push_str("...");
            }
            if let Some(t) = &p.value_type {
                s = format!("{t} {s}");
            }
            if !p.bounds.is_empty() {
                let bounds: Vec<String> = p.bounds.iter().map(ToString::to_string).collect();
                s.push_str(&format!(" extends {}", bounds.join(" & ")));
            }
            if let Some(d) = &p.default {
                s.push_str(&format!(" = {d}"));
            }
            s
        })
        .collect();
    format!("<{}>", list.join(", "))
}

fn dump_extras(out: &mut String, tags: &[(String, String)], doc: &str, depth: usize) {
    for (key, value) in tags {
        indent(out, depth);
        let _ = writeln!(out, "@{key} = {value:?}");
    }
    if !doc.is_empty() {
        indent(out, depth);
        let _ = writeln!(out, "doc {doc:?}");
    }
}

fn dump_type(out: &mut String, t: &TypeDecl, depth: usize) {
    indent(out, depth);
    let kind = match t.kind {
        TypeKind::Class => "class",
        TypeKind::Interface => "interface",
        TypeKind::Enum => "enum",
        TypeKind::Struct => "struct",
    };
    let _ = write!(
        out,
        "{} {kind}{} {}{}",
        visibility(t.visibility),
        flags(&[
            (t.is_abstract, "abstract"),
            (t.is_leaf, "leaf"),
            (t.is_static, "static"),
            (t.is_forward, "forward"),
            (t.is_partial, "partial"),
        ]),
        t.name,
        template_params(&t.template_parameters),
    );
    for s in &t.supertypes {
        let relation = match s.relation {
            Relation::Extends => "extends",
            Relation::Implements => "implements",
        };
        let access = s.access.map(|a| format!("{} ", visibility(a))).unwrap_or_default();
        let virt = if s.is_virtual { "virtual " } else { "" };
        let _ = write!(out, " {relation} {access}{virt}{}", s.ty);
    }
    let _ = writeln!(out, " (line {})", t.line);
    dump_extras(out, &t.tags, &t.documentation, depth + 1);
    for m in &t.members {
        dump_member(out, m, depth + 1);
    }
}

fn dump_member(out: &mut String, member: &Member, depth: usize) {
    match member {
        Member::Field(f) => {
            indent(out, depth);
            let _ = write!(
                out,
                "{} field{} {} : {}",
                visibility(f.visibility),
                flags(&[(f.is_static, "static"), (f.is_read_only, "readonly")]),
                f.name,
                f.ty
            );
            if let Some(d) = &f.default {
                let _ = write!(out, " = {d}");
            }
            out.push('\n');
            dump_extras(out, &f.tags, &f.documentation, depth + 1);
        }
        Member::Operation(o) => {
            indent(out, depth);
            let params: Vec<String> = o
                .parameters
                .iter()
                .map(|p| match &p.default {
                    Some(d) => format!("{} : {} = {d}", p.name, p.ty),
                    None => format!("{} : {}", p.name, p.ty),
                })
                .collect();
            let _ = write!(
                out,
                "{} operation{} {}{}({})",
                visibility(o.visibility),
                flags(&[
                    (o.is_static, "static"),
                    (o.is_abstract, "abstract"),
                    (o.is_leaf, "leaf"),
                    (o.is_constructor, "constructor"),
                ]),
                o.name,
                template_params(&o.template_parameters),
                params.join(", ")
            );
            if let Some(r) = &o.return_type {
                let _ = write!(out, " : {r}");
            }
            out.push('\n');
            dump_extras(out, &o.tags, &o.documentation, depth + 1);
        }
        Member::Literal(l) => {
            indent(out, depth);
            let _ = writeln!(out, "literal {}", l.name);
            dump_extras(out, &[], &l.documentation, depth + 1);
        }
        Member::Type(t) => dump_type(out, t, depth),
    }
}
