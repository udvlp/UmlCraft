//! New diagrams and elements in the form Structorizer 3.32-35 gives them, so that the editor
//! and the code import only make what Structorizer would write itself.

use crate::model::{
    Attrs, Color, Diagram, Element, ElementKind, Lines, ROOT_ORDER, RootKind, Sequence,
};
use crate::text::encode_lines;

/// The version whose format this crate writes.
pub const STRUCTORIZER_VERSION: &str = "3.32-35";

/// What a language's `structogram:` section in `locales/` says, as `build.rs` reads it. Only
/// the generated table makes these.
#[derive(Debug)]
pub struct KeywordSet {
    code: &'static str,
    name: &'static str,
    header: &'static [(&'static str, &'static str)],
    branches: (&'static str, &'static str),
    new: [&'static str; 3],
    examples: [&'static str; 2],
}

include!(concat!(env!("OUT_DIR"), "/keywords.rs"));

/// The language of the keywords a diagram is written with: those of its loops and of `return`
/// in its header, and the texts new elements start with. One per `locales/*.yml` with a
/// `structogram:` section; English is the default.
#[derive(Clone, Copy, Debug)]
pub struct Keywords(&'static KeywordSet);

impl Default for Keywords {
    fn default() -> Self {
        Self::english()
    }
}

impl PartialEq for Keywords {
    fn eq(&self, other: &Self) -> bool {
        self.0.code == other.0.code
    }
}

impl Eq for Keywords {}

impl Keywords {
    /// Every language, ordered by code.
    pub fn all() -> impl Iterator<Item = Keywords> + Clone {
        SETS.iter().map(Keywords)
    }

    /// Structorizer's own keywords, as a fresh installation has them.
    pub const fn english() -> Keywords {
        Keywords(&SETS[ENGLISH])
    }

    /// The language with exactly this code (`de`, not `de-AT`).
    pub fn by_code(code: &str) -> Option<Keywords> {
        Self::all().find(|k| k.code() == code)
    }

    /// The code of the language, which is the name of its locale file (`de`, `en`).
    pub fn code(self) -> &'static str {
        self.0.code
    }

    /// The name of the language in itself (`Deutsch`, `English`).
    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// The parser keywords of the header, as Structorizer saves them (two of English's end in
    /// a space).
    pub fn header(self) -> &'static [(&'static str, &'static str)] {
        self.0.header
    }

    /// The language `d` is written in, told by its keyword for while loops; English when no
    /// language has that keyword.
    pub fn of(d: &Diagram) -> Keywords {
        let Some(keyword) = d.header("preWhile") else { return Self::english() };
        let keyword = keyword.trim().to_lowercase();
        Self::all().find(|k| k.word("preWhile").to_lowercase() == keyword).unwrap_or_default()
    }

    /// The keyword `name` (`preFor`, `postFor`, …) without the space some end in.
    pub fn word(self, name: &str) -> &'static str {
        self.header().iter().find(|(n, _)| *n == name).map_or("", |(_, v)| v.trim())
    }

    /// The labels of an alternative's two branches: Structorizer's `T` and `F`, or `ja` and
    /// `nein` as German schoolbooks write them.
    pub fn branch_labels(self) -> (&'static str, &'static str) {
        self.0.branches
    }

    /// The text a new for loop starts with.
    pub fn new_for(self) -> &'static str {
        self.0.new[0]
    }

    /// The text a new while loop starts with.
    pub fn new_while(self) -> &'static str {
        self.0.new[1]
    }

    /// The text a new repeat loop starts with.
    pub fn new_repeat(self) -> &'static str {
        self.0.new[2]
    }

    /// A counting loop as the language writes it, for the hint of the properties.
    pub fn example_counting(self) -> &'static str {
        self.0.examples[0]
    }

    /// A traversing loop as the language writes it, for the hint of the properties.
    pub fn example_traversing(self) -> &'static str {
        self.0.examples[1]
    }
}

/// Who made a new diagram, when (`YYYY-MM-DD`) and with what (`origin`).
pub struct Provenance<'a> {
    pub author: &'a str,
    pub date: &'a str,
    pub origin: &'a str,
}

impl Diagram {
    /// An empty diagram with Structorizer's default header.
    pub fn new(kind: RootKind, text: &[String], made: &Provenance) -> Diagram {
        let mut attrs = Attrs(Vec::new());
        attrs.set("xmlns:nsd", "https://structorizer.fisch.lu", ROOT_ORDER);
        attrs.set("version", STRUCTORIZER_VERSION, ROOT_ORDER);
        for (name, value) in Keywords::english().header() {
            attrs.set(name, *value, ROOT_ORDER);
        }
        attrs.set("author", made.author, ROOT_ORDER);
        attrs.set("created", made.date, ROOT_ORDER);
        attrs.set("origin", made.origin, ROOT_ORDER);
        attrs.set("text", encode_lines(text), ROOT_ORDER);
        attrs.set("comment", "", ROOT_ORDER);
        attrs.set("color", Color::WHITE.to_hex(), ROOT_ORDER);
        attrs.set("type", kind.as_str(), ROOT_ORDER);
        attrs.set("style", "nice", ROOT_ORDER);
        Diagram { attrs, children: Sequence::new(Vec::new()) }
    }

    /// Gives the header the parser keywords of `keywords`.
    pub fn set_keywords(&mut self, keywords: Keywords) {
        for (name, value) in keywords.header() {
            self.set_header(name, *value);
        }
    }
}

/// The header of a new for loop.
pub enum ForLoop<'a> {
    /// `for i <- 1 to 10 by 2`: counter, start, end, step.
    Counting { var: &'a str, start: &'a str, end: &'a str, step: &'a str },
    /// `foreach x in list`: the loop variable.
    Traversing { var: &'a str },
    /// Any other text.
    Free,
}

impl Element {
    /// An element of `kind` with Structorizer's attributes and empty branches (one for a case
    /// or parallel element; use the dedicated constructors for more).
    fn with(kind: ElementKind, text: Option<&[String]>, branches: usize) -> Element {
        let mut e = Element { kind, attrs: Attrs::default(), branches: Vec::new() };
        if let Some(text) = text {
            e.set_attr("text", crate::model::text_value(text));
        }
        e.set_attr("comment", "");
        e.set_attr("color", Color::WHITE.to_hex());
        match kind {
            ElementKind::Instruction => e.set_attr("rotated", "0"),
            ElementKind::Call => e.set_attr("ismethoddecl", "0"),
            ElementKind::Alternative | ElementKind::Case => {
                e.set_attr("branch_colors", ",".repeat(branches.max(1) - 1));
            }
            _ => {}
        }
        e.set_attr("disabled", "0");
        e.branches = (0..branches).map(|_| Sequence::new(Vec::new())).collect();
        e
    }

    pub fn instruction(text: &[String]) -> Element {
        Self::with(ElementKind::Instruction, Some(text), 0)
    }

    pub fn call(text: &[String]) -> Element {
        Self::with(ElementKind::Call, Some(text), 0)
    }

    pub fn jump(text: &[String]) -> Element {
        Self::with(ElementKind::Jump, Some(text), 0)
    }

    pub fn alternative(condition: &[String]) -> Element {
        Self::with(ElementKind::Alternative, Some(condition), 2)
    }

    /// `lines`: the discriminator, one line per case, then the default label (`%` = none).
    /// Gets a branch per case and one for the default.
    pub fn case(lines: &[String]) -> Element {
        let branches = lines.len().saturating_sub(1).max(2);
        Self::with(ElementKind::Case, Some(lines), branches)
    }

    pub fn for_loop(text: &[String], header: ForLoop) -> Element {
        let mut e = Self::with(ElementKind::For, Some(text), 1);
        match header {
            ForLoop::Counting { var, start, end, step } => e.set_counting(var, start, end, step),
            ForLoop::Traversing { var } => e.set_traversal(var, "in"),
            ForLoop::Free => e.set_attr("style", "FREETEXT"),
        }
        e
    }

    /// A for loop's attributes for counting `var` from `start` to `end` by `step`.
    pub(crate) fn set_counting(&mut self, var: &str, start: &str, end: &str, step: &str) {
        self.set_attr("counterVar", var);
        self.set_attr("startValue", start);
        self.set_attr("endValue", end);
        self.set_attr("stepConst", step);
        self.set_attr("style", "COUNTER");
    }

    /// A for loop's attributes for going through a collection with `var`; `insep` is the
    /// keyword between them (`in`).
    pub(crate) fn set_traversal(&mut self, var: &str, insep: &str) {
        self.set_attr("counterVar", var);
        self.set_attr("startValue", "");
        self.set_attr("stepConst", "");
        self.set_attr("style", "TRAVERSAL");
        self.set_attr("insep", insep);
    }

    pub fn while_loop(condition: &[String]) -> Element {
        Self::with(ElementKind::While, Some(condition), 1)
    }

    pub fn repeat_loop(condition: &[String]) -> Element {
        Self::with(ElementKind::Repeat, Some(condition), 1)
    }

    pub fn forever_loop() -> Element {
        Self::with(ElementKind::Forever, None, 1)
    }

    /// `variable`: the name the caught exception gets.
    pub fn try_block(variable: &str) -> Element {
        Self::with(ElementKind::Try, Some(&[variable.to_owned()]), 3)
    }

    /// Structorizer keeps the number of branches as the text.
    pub fn parallel(branches: usize) -> Element {
        let branches = branches.max(1);
        Self::with(ElementKind::Parallel, Some(&[branches.to_string()]), branches)
    }
}

/// Lines from string slices, for building diagrams in code.
pub fn lines(texts: &[&str]) -> Lines {
    texts.iter().map(|s| (*s).to_owned()).collect()
}

/// A program `t` holding `elements`, for tests that do not look at the header.
#[cfg(test)]
pub(crate) fn test_diagram(elements: Vec<Element>) -> Diagram {
    let made = Provenance { author: "", date: "", origin: "" };
    let mut d = Diagram::new(RootKind::Program, &lines(&["t"]), &made);
    d.children = Sequence::new(elements);
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read, write};

    fn put(seq: &mut Sequence, e: Element) {
        seq.elements.push(e);
        seq.fit_color();
    }

    /// One element of each kind, nested a little, as the editor or the import would build it.
    fn all_elements() -> Diagram {
        let made = Provenance { author: "tester", date: "2026-10-29", origin: "UmlCraft test" };
        let mut d = Diagram::new(RootKind::Sub, &lines(&["sum(a: int; b: int): int"]), &made);
        let c = &mut d.children;
        put(c, Element::instruction(&lines(&["s <- a + b", "say \"hi\" & <go>"])));
        put(c, Element::call(&lines(&["log(s)"])));
        let mut alt = Element::alternative(&lines(&["s > 10"]));
        put(&mut alt.branches[0], Element::jump(&lines(&["return s"])));
        put(c, alt);
        put(c, Element::case(&lines(&["s", "1", "2, 3", "default"])));
        put(c, Element::case(&lines(&["s", "1", "%"])));
        let mut counting = Element::for_loop(
            &lines(&["for i <- 1 to 10 by 2"]),
            ForLoop::Counting { var: "i", start: "1", end: "10", step: "2" },
        );
        put(&mut counting.branches[0], Element::instruction(&lines(&["s <- s + i"])));
        put(c, counting);
        put(c, Element::for_loop(&lines(&["foreach x in list"]), ForLoop::Traversing { var: "x" }));
        put(c, Element::for_loop(&lines(&["for every line"]), ForLoop::Free));
        put(c, Element::while_loop(&lines(&["s < 100"])));
        put(c, Element::repeat_loop(&lines(&["s >= 100"])));
        put(c, Element::forever_loop());
        put(c, Element::try_block("e"));
        put(c, Element::parallel(3));
        put(c, Element::jump(&[]));
        d
    }

    #[test]
    fn new_diagrams_match_the_snapshot() {
        let written = write(&all_elements());
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/new/all-elements.nsd");
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(path, &written).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("{path} is missing; run with UPDATE_SNAPSHOTS=1"));
        pretty_assertions::assert_eq!(expected, written);
    }

    #[test]
    fn new_diagrams_read_back_unchanged() {
        let d = all_elements();
        assert_eq!(read(&write(&d)).unwrap(), d);
    }

    #[test]
    fn constructors_give_structorizers_shape() {
        let case = Element::case(&lines(&["x", "1", "2", "default"]));
        assert_eq!(case.branches.len(), 3);
        assert_eq!(case.attr("branch_colors"), Some(",,"));
        let par = Element::parallel(4);
        assert_eq!(par.text(), ["4"]);
        assert_eq!(par.branches.len(), 4);
        assert_eq!(Element::forever_loop().attr("text"), None);
        assert!(Element::alternative(&[]).branches.iter().all(|b| b.attrs.get("color").is_some()));
    }

    const ENGLISH_HEADER: &[(&str, &str)] = &[
        ("preRepeat", "until "),
        ("postFor", "to"),
        ("preReturn", "return"),
        ("postForIn", "in"),
        ("preWhile", "while "),
        ("output", "OUTPUT"),
        ("input", "INPUT"),
        ("preFor", "for"),
        ("preExit", "exit"),
        ("preLeave", "leave"),
        ("ignoreCase", "true"),
        ("preThrow", "throw"),
        ("preForIn", "foreach"),
        ("stepFor", "by"),
    ];

    const GERMAN_HEADER: &[(&str, &str)] = &[
        ("preRepeat", "bis "),
        ("postFor", "bis"),
        ("preReturn", "gib"),
        ("postForIn", "in"),
        ("preWhile", "solange "),
        ("output", "AUSGABE"),
        ("input", "EINGABE"),
        ("preFor", "für"),
        ("preExit", "verlasse"),
        ("preLeave", "leave"),
        ("ignoreCase", "true"),
        ("preThrow", "throw"),
        ("preForIn", "für jedes"),
        ("stepFor", "Schritt"),
    ];

    #[test]
    fn english_keywords_are_structorizers_defaults() {
        let en = Keywords::english();
        assert_eq!(en.header(), ENGLISH_HEADER);
        assert_eq!(en.code(), "en");
        assert_eq!(en.name(), "English");
        assert_eq!(en.branch_labels(), ("T", "F"));
        assert_eq!(en.new_for(), "for ? <- ? to ?");
        assert_eq!(en.new_while(), "while ?");
        assert_eq!(en.new_repeat(), "until ?");
        assert_eq!(en.example_counting(), "for i <- 1 to 10 by 2");
        assert_eq!(en.example_traversing(), "foreach x in list");
        assert_eq!(Keywords::default(), en);
    }

    #[test]
    fn german_keywords_are_as_before() {
        let de = Keywords::by_code("de").unwrap();
        assert_eq!(de.header(), GERMAN_HEADER);
        assert_eq!(de.name(), "Deutsch");
        assert_eq!(de.branch_labels(), ("ja", "nein"));
        assert_eq!(de.new_for(), "für ? := ? bis ?");
        assert_eq!(de.new_while(), "solange ?");
        assert_eq!(de.new_repeat(), "bis ?");
        assert_eq!(de.example_counting(), "für i := 1 bis 10 Schritt 2");
        assert_eq!(de.example_traversing(), "für jedes x in liste");
        assert_ne!(de, Keywords::english());
    }

    #[test]
    fn languages_are_listed_by_code() {
        let codes: Vec<_> = Keywords::all().map(Keywords::code).collect();
        assert!(codes.is_sorted());
        assert!(codes.contains(&"de") && codes.contains(&"en"));
        assert_eq!(Keywords::by_code("xx"), None);
        assert_eq!(Keywords::by_code("xx-YY"), None);
    }

    #[test]
    fn unknown_keywords_read_as_english() {
        let made = Provenance { author: "tester", date: "2026-10-29", origin: "UmlCraft test" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["x"]), &made);
        d.set_header("preWhile", "mientras ");
        assert_eq!(Keywords::of(&d), Keywords::english());
        assert_eq!(d.header("preWhile"), Some("mientras "));
    }

    #[test]
    fn the_language_is_told_by_the_while_keyword() {
        let made = Provenance { author: "tester", date: "2026-10-29", origin: "UmlCraft test" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["x"]), &made);
        assert_eq!(Keywords::of(&d), Keywords::english());
        d.set_keywords(Keywords::by_code("de").unwrap());
        assert_eq!(Keywords::of(&d).code(), "de");
        d.set_header("preWhile", " SOLANGE");
        assert_eq!(Keywords::of(&d).code(), "de");
    }
}
