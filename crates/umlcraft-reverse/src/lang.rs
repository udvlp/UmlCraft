//! What a language contributes: a front-end that turns source text into the IR, and the rules
//! the mapping follows for it ([`Rules`]).

use std::collections::BTreeMap;

use tree_sitter::Node;

use crate::SourceLanguage;
use crate::comment;
use crate::ir::{Scope, TypeKind};
use crate::library::TableLibrary;

/// How a library type holds other values, which decides how an attribute of that type becomes
/// an association (see [`crate::associations`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    None,
    /// Holds any number of its first type argument.
    Collection {
        ordered: bool,
        unique: bool,
    },
    /// Maps its first type argument to its second: a qualified association.
    Map,
    /// Holds zero or one of its first type argument.
    Optional,
    /// Owns exactly one of its first type argument (C++ `unique_ptr`).
    Owning,
}

/// A type of the language's standard library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryType {
    pub kind: TypeKind,
    pub parameters: Vec<String>,
    pub container: Container,
}

/// What the mapping needs to know about a language besides its IR.
pub struct Rules {
    /// Built-in types, kept as text.
    pub primitives: &'static [&'static str],
    /// Packages whose types are visible without an import (`java.lang`).
    pub implicit_imports: &'static [&'static [&'static str]],
    /// The standard library's public types, so names imported from it resolve to placeholders
    /// of the right kind at the right place.
    pub library: &'static TableLibrary,
    /// Packages whose types never become association ends (`java.lang`: `String`, `Integer` …).
    pub value_packages: &'static [&'static [&'static str]],
    /// Splits a qualified name that names nothing known into package and type segments.
    pub split_qualified: fn(&[String]) -> usize,
    /// Names are also looked up in the namespaces around a type's own, outwards to the root,
    /// and qualified names relative to them (C#). Otherwise only the own package counts (Java).
    pub enclosing_namespaces: bool,
    /// The base list does not say which entries are classes and which interfaces (C#'s
    /// `class A : B, IC`): where the target is known, its kind decides between generalization
    /// and realization; the front-end's guess only gives unknown targets their kind.
    pub base_list_by_target: bool,
    /// Type modifiers that make a type optional (C#'s `Order?`, C++'s `Order*`): an attribute
    /// of such a type becomes an association end `0..1`.
    pub optional_modifiers: &'static [&'static str],
    /// Qualifiers in a type's prefix that do not change what it refers to (C++'s `const `):
    /// associations look through them.
    pub type_qualifiers: &'static [&'static str],
    /// A package re-exports what its own scope imports (Python's `from .orders import Order` in
    /// `shop/__init__.py` makes `shop.Order` the class in `shop.orders`).
    pub reexports: bool,
}

/// Parts of the source the import leaves out, counted for the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Skip {
    /// Bodies of methods and constructors.
    MethodBody,
    /// Static and instance initializer blocks.
    Initializer,
    /// Classes declared inside a method.
    LocalClass,
    /// Anonymous classes (`new Runnable() { … }`).
    AnonymousClass,
    /// Arguments and bodies of enum constants.
    EnumConstantBody,
    /// Functions and variables outside any type.
    FreeMember,
    /// C# attributes (`[Serializable]`).
    Attribute,
    /// C# delegate types.
    Delegate,
    /// C# events.
    Event,
    /// Explicit values of enum members (`A = 1`).
    EnumValue,
    /// Branches of conditionals: `#elif` and `#else` after the branch read, and branches under
    /// a constant false condition (`#if 0`).
    PreprocessorBranch,
    /// Aliases of constructed types (`using Ids = List<int>;`).
    GenericAlias,
    /// Members defined outside a type the sources do not declare (`void Gone::f() {}`).
    UnknownOwner,
    /// C/C++ macro definitions (`#define`).
    Macro,
    /// C++ template specializations (`template<> class A<int>`, `class A<T*>`).
    Specialization,
    /// C++ functions declared `= delete`.
    DeletedFunction,
    /// C++ friend declarations.
    Friend,
    /// Python statements that declare nothing (`if __name__ == "__main__":`).
    Statement,
    /// Methods whose statements nest too deeply for a structogram
    /// (`umlcraft_nsd::read::MAX_DEPTH`): chosen for a structogram, but none is made.
    DeepMethod,
}

impl Skip {
    /// The key of the locale text naming it: `skipped.<key>`.
    pub fn key(self) -> &'static str {
        match self {
            Skip::MethodBody => "method_body",
            Skip::Initializer => "initializer",
            Skip::LocalClass => "local_class",
            Skip::AnonymousClass => "anonymous_class",
            Skip::EnumConstantBody => "enum_constant_body",
            Skip::FreeMember => "free_member",
            Skip::Attribute => "attribute",
            Skip::Delegate => "delegate",
            Skip::Event => "event",
            Skip::EnumValue => "enum_value",
            Skip::PreprocessorBranch => "conditional_branch",
            Skip::GenericAlias => "generic_alias",
            Skip::UnknownOwner => "unknown_owner",
            Skip::Macro => "macro",
            Skip::Specialization => "specialization",
            Skip::DeletedFunction => "deleted_function",
            Skip::Friend => "friend",
            Skip::Statement => "statement",
            Skip::DeepMethod => "deep_method",
        }
    }
}

pub type Skipped = BTreeMap<Skip, usize>;

/// Where a file stops being valid source: 1-based line and column (in characters), and the
/// text found there, or for something missing, what was expected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub line: usize,
    pub column: usize,
    pub token: String,
    pub missing: bool,
}

/// Lowers one language's syntax tree to the IR. Implementations hold no state between files.
pub trait Frontend: Sync {
    /// The declarations of one file, parsed without syntax errors into the tree at `root` from
    /// `text` (see [`crate::parse_tree`]), counting what is left out in `skipped`. `module` is
    /// the module the file is, for languages whose files are modules (Python), else empty.
    fn lower(&self, root: Node, text: &str, module: &[String], skipped: &mut Skipped) -> Scope;
}

/// The language a front-end's [`Reader`] reads.
pub(crate) trait Language {
    const LANGUAGE: SourceLanguage;
}

/// What a front-end reads one file with: the text the tree was parsed from and the count of
/// what it leaves out. Each front-end adds its own methods for its `L`, which also holds what
/// it keeps per file.
pub(crate) struct Reader<'a, L> {
    pub src: &'a str,
    pub skipped: &'a mut Skipped,
    pub lang: L,
}

impl<'a, L: Language> Reader<'a, L> {
    pub fn text(&self, node: Node) -> &'a str {
        &self.src[node.byte_range()]
    }

    pub fn count(&mut self, skip: Skip) {
        *self.skipped.entry(skip).or_default() += 1;
    }

    /// The documentation comment right before `node`, as the language writes it
    /// ([`comment::documentation`]).
    pub fn documentation(&self, node: Node) -> String {
        comment::documentation(L::LANGUAGE, node, self.src)
    }
}

pub(crate) fn named_children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub(crate) fn children<'t>(node: Node<'t>) -> Vec<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Text with its whitespace runs made single blanks: `unsigned  long` is `unsigned long`.
pub(crate) fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// [`Rules::split_qualified`] where namespaces and types look alike (C#, C++): the type is the
/// last segment.
pub(crate) fn split_at_last(path: &[String]) -> usize {
    path.len().saturating_sub(1)
}

/// [`Rules::split_qualified`] where packages are lower case and types start with a capital
/// letter (Java, Python's PEP 8): the package ends before the first segment that starts with a
/// capital letter or contains a `$` (Java's nested types), else the type is the last segment.
pub(crate) fn split_at_capital(path: &[String]) -> usize {
    path.iter()
        .position(|s| s.starts_with(|c: char| c.is_uppercase()) || s.contains('$'))
        .unwrap_or(split_at_last(path))
}

/// The first syntax error in a tree-sitter tree, if any.
pub(crate) fn first_error(root: Node, text: &str) -> Option<SyntaxError> {
    if !root.has_error() {
        return None;
    }
    let mut cursor = root.walk();
    loop {
        let node = cursor.node();
        if node.is_error() || node.is_missing() {
            let start = node.start_position();
            let line_start = text[..node.start_byte()].rfind('\n').map_or(0, |i| i + 1);
            let column = text[line_start..node.start_byte()].chars().count() + 1;
            let token = if node.is_missing() {
                node.kind().to_owned()
            } else {
                let found = &text[node.byte_range()];
                found.split_whitespace().next().unwrap_or("").chars().take(30).collect()
            };
            return Some(SyntaxError {
                line: start.row + 1,
                column,
                token,
                missing: node.is_missing(),
            });
        }
        // Into the first child that contains an error, else onwards.
        if node.has_error() && cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                return None;
            }
        }
    }
}
