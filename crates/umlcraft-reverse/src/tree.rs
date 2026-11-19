//! A source file parsed by tree-sitter, once for everything that reads it: the front-ends lower
//! its declarations to the IR, `umlcraft-nsd-code` reads method bodies from the same tree.

use tree_sitter::{Node, Parser, Tree};

use crate::SourceLanguage;
use crate::lang::{SyntaxError, first_error};
use crate::preprocessor::{self, Dialect};

/// A source file's syntax tree with the text it was parsed from.
pub struct SourceTree {
    language: SourceLanguage,
    tree: Tree,
    /// As parsed: in C# and C++, the branches of `#if` after the first and the directives are
    /// blanked out (every byte keeps its place).
    text: String,
    /// As written, where it differs from `text`.
    written: Option<String>,
    /// Where the branches left out start (`#elif`, `#else`, `#if 0`): the byte their
    /// directive's line starts at.
    branches: Vec<usize>,
}

/// Parses a file's text (decoded, see [`crate::source::read`]) in `language`. C# and C++ are
/// parsed as the first branches of their conditionals read them
/// ([`preprocessor::first_branches`]). Syntax errors are part of the tree
/// ([`SourceTree::syntax_error`]).
pub fn parse_tree(language: SourceLanguage, text: impl Into<String>) -> SourceTree {
    let text = text.into();
    let dialect = match language {
        SourceLanguage::CSharp => Some(Dialect::CSharp),
        SourceLanguage::Cpp => Some(Dialect::Cpp),
        SourceLanguage::Java | SourceLanguage::Python => None,
    };
    let (text, written, branches) = match dialect {
        Some(dialect) => {
            let (parsed, branches) = preprocessor::first_branches(&text, dialect);
            let written = (parsed != text).then_some(text);
            (parsed, written, branches)
        }
        None => (text, None, Vec::new()),
    };
    let grammar: tree_sitter::Language = match language {
        SourceLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        SourceLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        SourceLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        SourceLanguage::Python => tree_sitter_python::LANGUAGE.into(),
    };
    let mut parser = Parser::new();
    parser.set_language(&grammar).expect("the grammar matches the tree-sitter version");
    let tree = parser.parse(&text, None).expect("parsing without a timeout always finishes");
    SourceTree { language, tree, text, written, branches }
}

impl SourceTree {
    pub fn language(&self) -> SourceLanguage {
        self.language
    }

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn root(&self) -> Node<'_> {
        self.tree.root_node()
    }

    /// The text as parsed, which the tree's byte ranges and positions refer to.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text as written, before conditionals were blanked out; the same byte positions as
    /// [`SourceTree::text`].
    pub fn written(&self) -> &str {
        self.written.as_deref().unwrap_or(&self.text)
    }

    /// Whether the text as parsed differs from the text as written.
    pub fn is_preprocessed(&self) -> bool {
        self.written.is_some()
    }

    /// The number of branches of conditionals left out (C#, C++): `#elif`, `#else`, `#if 0`.
    pub fn left_out_branches(&self) -> usize {
        self.branches.len()
    }

    /// Where the branches left out start, in file order: the byte their directive's line
    /// (`#else`, `#elif …`, `#if 0`) starts at.
    pub fn left_out_at(&self) -> &[usize] {
        &self.branches
    }

    /// The first syntax error, if any.
    pub fn syntax_error(&self) -> Option<SyntaxError> {
        first_error(self.root(), &self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_text_as_written_where_conditionals_were_blanked() {
        let text = "#if A\nclass B {}\n#else\nclass C {}\n#endif\n";
        let tree = parse_tree(SourceLanguage::CSharp, text);
        assert!(tree.is_preprocessed());
        assert_eq!(tree.written(), text);
        assert_eq!(tree.text().len(), text.len());
        assert!(!tree.text().contains("class C"));
        assert_eq!(tree.left_out_branches(), 1);
        assert_eq!(tree.syntax_error(), None);

        let java = parse_tree(SourceLanguage::Java, "class A {");
        assert!(!java.is_preprocessed());
        assert_eq!(java.written(), "class A {");
        assert!(java.syntax_error().is_some_and(|e| e.missing));
    }
}
