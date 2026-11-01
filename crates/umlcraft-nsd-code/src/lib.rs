//! Methods of source files as structograms: the methods and functions of a Java, C#, Python or
//! C++ file, and one of them as a Nassi-Shneiderman diagram in Structorizer's `.nsd` form.
//! Each language reads its own nodes (`java`, `csharp`, `python`, `cpp`); what they share is
//! in `core` and `pseudo`.
//!
//! Texts are written as structograms write them: no types (the root is `name(a, b)`),
//! assignments as `x := value` (`i++` as `i := i + 1`), no `;`, counting loops as
//! `for i := 0 to n - 1` and `foreach x in c`, conditions as questions (`a > 0?`), returned
//! values as `return x`, or with German keywords ([`Made::keywords`]) `für i := 0 bis n - 1`,
//! `für jedes x in c` and `gib x`; expressions and conditions stay as in the code otherwise (without their
//! outer parentheses). Calls of the class's own methods (`f(x);`, `this.f(x);`)
//! become call elements, other statements instructions. What has no place in a structogram
//! (`continue`, labels, local classes …) is named in the [`Report`]; nothing is left out
//! silently. Structorizer's own Java import was only compared by its results, never by its
//! code (as was its C import); Structorizer has no C# or Python import.

mod core;
mod cpp;
mod csharp;
mod java;
mod pseudo;
mod python;
mod text;

use umlcraft_nsd::Diagram;
use umlcraft_reverse::{SourceTree, parse_tree};

/// The languages whose methods become structograms: those of the class import, with the same
/// file extensions (`SourceLanguage::of`).
pub use umlcraft_reverse::SourceLanguage;
/// Reading a source file as the class import does: its encoding guessed where it has to be,
/// files larger than `MAX_FILE_SIZE` not read.
pub use umlcraft_reverse::source::{Decoded, MAX_FILE_SIZE, ReadError, read};

/// A method or constructor with a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodRef {
    /// The classes it is in, outermost first; none for a Python function of the module or a
    /// C++ function outside namespaces (in C++ one entry, `shop::Cart`, as written).
    pub class: Vec<String>,
    pub name: String,
    /// As written, on one line, without modifiers, annotations and `throws`:
    /// `double getSpeed(SpeedUnit unit)`, `<T> T first(List<T> list)`, `Point(int x, int y)`;
    /// C# accessors as .NET names them: `int get_Count()`.
    pub signature: String,
    /// 1-based line of the declaration.
    pub line: usize,
    /// Where the declaration starts in the file (identifies it).
    pub start: usize,
    /// Where it ends: with `start` the bytes the class import's operation for it overlaps.
    pub end: usize,
}

impl MethodRef {
    /// `Outer.Inner`.
    pub fn class_name(&self) -> String {
        self.class.join(".")
    }

    /// A file name for its diagram: `Class.method.nsd`, `function.nsd` outside classes;
    /// `shop::Cart` as `shop.Cart`, characters no file system takes as `_`, at most 120
    /// characters before `.nsd`.
    pub fn file_name(&self) -> String {
        let stem = match self.class.is_empty() {
            true => self.name.clone(),
            false => format!("{}.{}", self.class_name().replace("::", "."), self.name),
        };
        let stem: String = stem
            .chars()
            .map(|c| if c.is_whitespace() || "<>:\"/\\|?*".contains(c) { '_' } else { c })
            .take(120)
            .collect();
        format!("{stem}.nsd")
    }
}

/// .NET's name of an operator (`op_Addition`), so that it makes a file name (C# and C++).
fn operator_name(op: &str, binary: bool) -> String {
    let name = match (op, binary) {
        ("+", true) => "Addition",
        ("-", true) => "Subtraction",
        ("+", false) => "UnaryPlus",
        ("-", false) => "UnaryNegation",
        ("*", _) => "Multiply",
        ("/", _) => "Division",
        ("%", _) => "Modulus",
        ("&", _) => "BitwiseAnd",
        ("|", _) => "BitwiseOr",
        ("^", _) => "ExclusiveOr",
        ("<<", _) => "LeftShift",
        (">>", _) => "RightShift",
        (">>>", _) => "UnsignedRightShift",
        ("==", _) => "Equality",
        ("!=", _) => "Inequality",
        ("<", _) => "LessThan",
        (">", _) => "GreaterThan",
        ("<=", _) => "LessThanOrEqual",
        (">=", _) => "GreaterThanOrEqual",
        ("!", _) => "LogicalNot",
        ("~", _) => "OnesComplement",
        ("++", _) => "Increment",
        ("--", _) => "Decrement",
        ("true", _) => "True",
        ("=", _) => "Assign",
        ("+=", _) => "AdditionAssignment",
        ("-=", _) => "SubtractionAssignment",
        ("*=", _) => "MultiplicationAssignment",
        ("/=", _) => "DivisionAssignment",
        ("()", _) => "Call",
        ("[]", _) => "Index",
        ("<<=", _) => "LeftShiftAssignment",
        (">>=", _) => "RightShiftAssignment",
        ("<=>", _) => "Comparison",
        ("->", _) => "Arrow",
        ("false", _) => "False",
        _ => "Operator",
    };
    format!("op_{name}")
}

/// Who makes the diagram, when, and from which file (for the header).
pub struct Made<'a> {
    pub author: &'a str,
    pub date: &'a str,
    /// The `origin` attribute, e.g. `UmlCraft 0.9.0 / Java: Shop.java`.
    pub origin: &'a str,
    /// The language of the keywords: of loops, of `return`, and in the header.
    pub keywords: umlcraft_nsd::Keywords,
}

/// Something of the method that the diagram does not show as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 1-based line in the file.
    pub line: usize,
    pub kind: NoteKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteKind {
    /// `continue` has no element: kept as a red, disabled instruction.
    Continue,
    /// A label (`outer:`) is left out; `break outer` keeps its text.
    Label,
    /// A class, record, enum or interface declared in the method is left out.
    LocalClass,
    /// `synchronized (x) { … }`: the body stands in its place, the lock as a comment.
    Synchronized,
    /// A `switch` case runs on into the next: its statements are copied there.
    Fallthrough,
    /// A part the parser could not read: kept as a red, disabled instruction.
    SyntaxError,
    /// C# `using`: the resource is assigned, the body stands in its place.
    Using,
    /// C# `lock (x) { … }`: the body stands in its place, the lock as a comment.
    Lock,
    /// C# `yield return`/`yield break`: kept as written; structograms have no iterators.
    Yield,
    /// C# `goto`: kept as a jump with its text.
    Goto,
    /// A function declared in the method is left out.
    LocalFunction,
    /// Of `#if` … `#else`, only the first branch is read.
    Conditional,
    /// Python `with`: the context is assigned, the body stands in its place.
    With,
    /// Python `for`/`while … else`: the `else` body follows the loop (where it also runs
    /// after a `break`).
    LoopElse,
    /// Python `try … else`: the `else` body ends the try body.
    TryElse,
    /// A function defined in the function is left out.
    NestedFunction,
}

/// What the import of one method did not show as written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub notes: Vec<Note>,
}

impl Report {
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }
}

/// A parsed source file: the tree the class import parses too ([`SourceTree`]), so that one
/// parse serves both.
pub struct Source {
    tree: SourceTree,
}

impl Source {
    /// Parses a file's text (see [`read`]) as [`umlcraft_reverse::parse_tree`] does.
    pub fn parse(language: SourceLanguage, text: impl Into<String>) -> Source {
        Source::new(parse_tree(language, text))
    }

    pub fn new(tree: SourceTree) -> Source {
        Source { tree }
    }

    pub fn language(&self) -> SourceLanguage {
        self.tree.language()
    }

    /// The parsed file, for the class import ([`umlcraft_reverse::lower`]).
    pub fn tree(&self) -> &SourceTree {
        &self.tree
    }

    /// The methods and constructors with a body, in file order, nested classes included
    /// (but not classes inside methods); in C# also accessors, operators, finalizers and the
    /// top-level statements (as `Program.Main`); in Python also the functions of the module
    /// and the code of the module (as `__main__`); in C++ also free functions, destructors,
    /// operators and definitions outside the class (`void Cart::add(…)`).
    pub fn methods(&self) -> Vec<MethodRef> {
        let (root, text) = (self.tree.root(), self.tree.text());
        match self.language() {
            SourceLanguage::Java => java::methods(root, text),
            SourceLanguage::CSharp => csharp::methods(root, text),
            SourceLanguage::Python => python::methods(root, text),
            SourceLanguage::Cpp => cpp::methods(root, text),
        }
    }

    /// The diagram of `method` (one of [`Source::methods`]).
    pub fn diagram(&self, method: &MethodRef, made: &Made) -> Option<(Diagram, Report)> {
        let (root, text) = (self.tree.root(), self.tree.text());
        match self.language() {
            SourceLanguage::Java => {
                let node = java::find(root, method)?;
                Some(java::diagram(node, text, made))
            }
            SourceLanguage::CSharp => {
                let member = csharp::find(root, text, method)?;
                let (d, mut report) = csharp::diagram(&member, text, made);
                if let Some(line) = self.left_out(member.range()) {
                    report.notes.insert(0, Note { line, kind: NoteKind::Conditional });
                }
                Some((d, report))
            }
            SourceLanguage::Python => {
                let member = python::find(root, text, method)?;
                Some(python::diagram(&member, root, text, made))
            }
            SourceLanguage::Cpp => {
                let member = cpp::find(root, text, method)?;
                let (d, mut report) = cpp::diagram(&member, root, text, made);
                if let Some(line) = self.left_out(member.range()) {
                    report.notes.insert(0, Note { line, kind: NoteKind::Conditional });
                }
                Some((d, report))
            }
        }
    }

    /// The line of the first `#elif` or `#else` in `range`, whose branch was left out.
    fn left_out(&self, range: std::ops::Range<usize>) -> Option<usize> {
        if !self.tree.is_preprocessed() {
            return None;
        }
        let written = self.tree.written();
        let before = written[..range.start].matches('\n').count();
        let at = written[range].lines().position(|line| {
            let directive = line.trim_start().strip_prefix('#').map(str::trim_start);
            directive.is_some_and(|d| d.starts_with("elif") || d.starts_with("else"))
        })?;
        Some(before + at + 1)
    }
}
