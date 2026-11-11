//! The diagram as Structorizer stores it: a root with a sequence of elements, structured
//! elements with their branches.
//!
//! Every node keeps its XML attributes as an ordered list, exactly as read, and offers typed
//! access on top. Writing a file that was read gives the same bytes, also for attributes this
//! crate does not know (e.g. from a newer Structorizer) and for files from older versions that
//! lack some attributes. Setting an attribute a node does not have yet inserts it where
//! Structorizer writes it (`order`).

use crate::text;

/// The lines of a text or comment.
pub type Lines = Vec<String>;

/// An RGB colour, written as six lower-case hex digits (`ffffff`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color(pub [u8; 3]);

impl Color {
    pub const WHITE: Color = Color([0xff, 0xff, 0xff]);

    /// Parses six hex digits, either case.
    pub fn parse(hex: &str) -> Option<Color> {
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        Some(Color([byte(0)?, byte(2)?, byte(4)?]))
    }

    pub fn to_hex(self) -> String {
        let [r, g, b] = self.0;
        format!("{r:02x}{g:02x}{b:02x}")
    }
}

/// One XML attribute, value unescaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    pub value: String,
}

/// The attributes of a node in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attrs(pub Vec<Attr>);

impl Attrs {
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|a| a.name == name).map(|a| a.value.as_str())
    }

    /// Replaces the value, or inserts the attribute before the first attribute that comes after
    /// it in `order` (unknown attributes keep their place).
    pub fn set(&mut self, name: &str, value: impl Into<String>, order: &[&str]) {
        let value = value.into();
        if let Some(a) = self.0.iter_mut().find(|a| a.name == name) {
            a.value = value;
            return;
        }
        let rank = |n: &str| order.iter().position(|o| *o == n);
        let at = match rank(name) {
            Some(own) => self
                .0
                .iter()
                .position(|a| rank(&a.name).is_some_and(|r| r > own))
                .unwrap_or(self.0.len()),
            None => self.0.len(),
        };
        self.0.insert(at, Attr { name: name.to_owned(), value });
    }

    pub fn remove(&mut self, name: &str) {
        self.0.retain(|a| a.name != name);
    }

    /// Attributes from name/value pairs, in the given order.
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Attrs {
        Attrs(pairs.iter().map(|(n, v)| Attr { name: (*n).into(), value: (*v).into() }).collect())
    }
}

/// The kinds of elements, with their XML names and branch tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ElementKind {
    Instruction,
    Call,
    Jump,
    Alternative,
    Case,
    For,
    While,
    Repeat,
    Forever,
    Try,
    Parallel,
}

/// How many branches an element has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Branches {
    /// Exactly these, in this order.
    Fixed(&'static [&'static str]),
    /// One or more of this tag.
    Repeated(&'static str),
}

impl ElementKind {
    pub const ALL: [ElementKind; 11] = [
        Self::Instruction,
        Self::Call,
        Self::Jump,
        Self::Alternative,
        Self::Case,
        Self::For,
        Self::While,
        Self::Repeat,
        Self::Forever,
        Self::Try,
        Self::Parallel,
    ];

    pub fn tag(self) -> &'static str {
        match self {
            Self::Instruction => "instruction",
            Self::Call => "call",
            Self::Jump => "jump",
            Self::Alternative => "alternative",
            Self::Case => "case",
            Self::For => "for",
            Self::While => "while",
            Self::Repeat => "repeat",
            Self::Forever => "forever",
            Self::Try => "try",
            Self::Parallel => "parallel",
        }
    }

    pub fn from_tag(tag: &str) -> Option<ElementKind> {
        Self::ALL.into_iter().find(|k| k.tag() == tag)
    }

    pub fn branches(self) -> Branches {
        match self {
            Self::Instruction | Self::Call | Self::Jump => Branches::Fixed(&[]),
            Self::Alternative => Branches::Fixed(&["qTrue", "qFalse"]),
            Self::Case => Branches::Repeated("qCase"),
            Self::For => Branches::Fixed(&["qFor"]),
            Self::While => Branches::Fixed(&["qWhile"]),
            Self::Repeat => Branches::Fixed(&["qRepeat"]),
            Self::Forever => Branches::Fixed(&["qForever"]),
            Self::Try => Branches::Fixed(&["qTry", "qCatch", "qFinally"]),
            Self::Parallel => Branches::Repeated("qPara"),
        }
    }

    /// The order in which Structorizer writes the attributes of this kind.
    pub fn order(self) -> &'static [&'static str] {
        match self {
            Self::Instruction => &["text", "comment", "color", "rotated", "disabled"],
            Self::Call => &["text", "comment", "color", "disabled", "ismethoddecl"],
            Self::Alternative | Self::Case => {
                &["text", "comment", "color", "branch_colors", "disabled"]
            }
            Self::For => &[
                "text",
                "comment",
                "counterVar",
                "startValue",
                "endValue",
                "stepConst",
                "style",
                "insep",
                "color",
                "disabled",
            ],
            Self::Forever => &["comment", "color", "disabled"],
            Self::Jump | Self::While | Self::Repeat | Self::Try | Self::Parallel => {
                &["text", "comment", "color", "disabled"]
            }
        }
    }
}

/// The attribute order of `root`. Where Structorizer puts `postAlt`, `postCase`, `postWhile`,
/// `postRepeat` and `preImport` was not seen; they go next to their kin.
pub const ROOT_ORDER: &[&str] = &[
    "xmlns:nsd",
    "version",
    "preCase",
    "postCase",
    "preRepeat",
    "postRepeat",
    "postFor",
    "preReturn",
    "postForIn",
    "preWhile",
    "postWhile",
    "output",
    "input",
    "preAlt",
    "postAlt",
    "preFor",
    "preExit",
    "preLeave",
    "ignoreCase",
    "preThrow",
    "preForIn",
    "stepFor",
    "preImport",
    "includeList",
    "namespace",
    "author",
    "created",
    "changedby",
    "changed",
    "origin",
    "licenseName",
    "license",
    "text",
    "comment",
    "color",
    "type",
    "style",
];

/// A branch or the root's body (`children`, `qTrue`, `qCase` …).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sequence {
    /// Structorizer writes `color="ffffff"` on empty sequences only.
    pub attrs: Attrs,
    pub elements: Vec<Element>,
}

impl Sequence {
    pub fn new(elements: Vec<Element>) -> Sequence {
        let mut s = Sequence { attrs: Attrs::default(), elements };
        s.fit_color();
        s
    }

    /// Gives the sequence the colour attribute when empty and drops it otherwise, as
    /// Structorizer writes it. Editing calls this after changing `elements`.
    pub fn fit_color(&mut self) {
        if self.elements.is_empty() {
            self.attrs.set("color", Color::WHITE.to_hex(), &["color"]);
        } else {
            self.attrs.remove("color");
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Element {
    pub kind: ElementKind,
    pub attrs: Attrs,
    pub branches: Vec<Sequence>,
}

impl Element {
    pub fn text(&self) -> Lines {
        self.attrs.get("text").map(text::decode_lines_lossy).unwrap_or_default()
    }

    /// Forever loops have no text; setting one there is ignored.
    pub fn set_text(&mut self, lines: &[String]) {
        if self.kind != ElementKind::Forever {
            self.attrs.set("text", text_value(lines), self.kind.order());
        }
    }

    pub fn comment(&self) -> Lines {
        self.attrs.get("comment").map(text::decode_lines_lossy).unwrap_or_default()
    }

    /// An empty comment is written as `comment=""`, as Structorizer does.
    pub fn set_comment(&mut self, lines: &[String]) {
        let value = if lines.iter().all(String::is_empty) {
            String::new()
        } else {
            text::encode_lines(lines)
        };
        self.attrs.set("comment", value, self.kind.order());
    }

    pub fn color(&self) -> Color {
        self.attrs.get("color").and_then(Color::parse).unwrap_or(Color::WHITE)
    }

    pub fn set_color(&mut self, color: Color) {
        self.attrs.set("color", color.to_hex(), self.kind.order());
    }

    pub fn disabled(&self) -> bool {
        self.attrs.get("disabled") == Some("1")
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.attrs.set("disabled", if disabled { "1" } else { "0" }, self.kind.order());
    }

    /// The colours of the branch heads of an alternative or case: `None` = element colour.
    pub fn branch_colors(&self) -> Vec<Option<Color>> {
        match self.attrs.get("branch_colors") {
            Some(v) => v.split(',').map(Color::parse).collect(),
            None => vec![None; self.branches.len()],
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name)
    }

    pub fn set_attr(&mut self, name: &str, value: impl Into<String>) {
        self.attrs.set(name, value, self.kind.order());
    }
}

/// An element's `text` attribute: Structorizer's editor writes a single empty line as no
/// text at all (`text=""`), unlike comments.
pub(crate) fn text_value(lines: &[String]) -> String {
    match lines {
        [line] if line.is_empty() => String::new(),
        _ => text::encode_lines(lines),
    }
}

/// What a diagram is: Structorizer draws the three with different frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootKind {
    Program,
    Sub,
    Includable,
}

impl RootKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Sub => "sub",
            Self::Includable => "includable",
        }
    }

    pub fn parse(s: &str) -> Option<RootKind> {
        [Self::Program, Self::Sub, Self::Includable].into_iter().find(|k| k.as_str() == s)
    }
}

/// Framed (`nice`) or unframed (`abbr`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootStyle {
    Nice,
    Abbr,
}

impl RootStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nice => "nice",
            Self::Abbr => "abbr",
        }
    }

    pub fn parse(s: &str) -> Option<RootStyle> {
        [Self::Nice, Self::Abbr].into_iter().find(|k| k.as_str() == s)
    }
}

/// A whole `.nsd` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagram {
    /// The attributes of `<root>`: header (version, keywords, author …) and the root's own
    /// text, comment, colour, type and style.
    pub attrs: Attrs,
    pub children: Sequence,
}

/// How many levels deep `elements` nest, they themselves the first; 0 for none. Counted
/// without recursion, so that it holds for trees of any depth.
pub fn depth(elements: &[Element]) -> usize {
    let mut deepest = 0;
    let mut open: Vec<(&Element, usize)> = elements.iter().map(|e| (e, 1)).collect();
    while let Some((e, level)) = open.pop() {
        deepest = deepest.max(level);
        for branch in &e.branches {
            open.extend(branch.elements.iter().map(|c| (c, level + 1)));
        }
    }
    deepest
}

impl Diagram {
    /// How many levels deep the elements nest (see [`depth`]).
    pub fn depth(&self) -> usize {
        depth(&self.children.elements)
    }

    pub fn text(&self) -> Lines {
        self.attrs.get("text").map(text::decode_lines_lossy).unwrap_or_default()
    }

    pub fn set_text(&mut self, lines: &[String]) {
        self.attrs.set("text", text::encode_lines(lines), ROOT_ORDER);
    }

    pub fn comment(&self) -> Lines {
        self.attrs.get("comment").map(text::decode_lines_lossy).unwrap_or_default()
    }

    pub fn set_comment(&mut self, lines: &[String]) {
        let value = if lines.iter().all(String::is_empty) {
            String::new()
        } else {
            text::encode_lines(lines)
        };
        self.attrs.set("comment", value, ROOT_ORDER);
    }

    pub fn kind(&self) -> RootKind {
        self.attrs.get("type").and_then(RootKind::parse).unwrap_or(RootKind::Program)
    }

    pub fn set_kind(&mut self, kind: RootKind) {
        self.attrs.set("type", kind.as_str(), ROOT_ORDER);
    }

    pub fn style(&self) -> RootStyle {
        self.attrs.get("style").and_then(RootStyle::parse).unwrap_or(RootStyle::Nice)
    }

    pub fn set_style(&mut self, style: RootStyle) {
        self.attrs.set("style", style.as_str(), ROOT_ORDER);
    }

    /// A header attribute such as `version`, `author` or `preWhile`.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.attrs.get(name)
    }

    pub fn set_header(&mut self, name: &str, value: impl Into<String>) {
        self.attrs.set(name, value, ROOT_ORDER);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse_and_print_lower_case() {
        assert_eq!(Color::parse("FF8000"), Some(Color([0xff, 0x80, 0x00])));
        assert_eq!(Color::parse("ff800"), None);
        assert_eq!(Color::parse("gg8000"), None);
        assert_eq!(Color([0x80, 0x80, 0xff]).to_hex(), "8080ff");
    }

    #[test]
    fn missing_attributes_go_where_structorizer_writes_them() {
        let mut attrs = Attrs::from_pairs(&[("text", ""), ("comment", ""), ("disabled", "0")]);
        attrs.set("color", "ffffff", ElementKind::Instruction.order());
        attrs.set("rotated", "0", ElementKind::Instruction.order());
        let names: Vec<_> = attrs.0.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["text", "comment", "color", "rotated", "disabled"]);
    }

    #[test]
    fn unknown_attributes_keep_their_place() {
        let mut attrs = Attrs::from_pairs(&[("text", ""), ("future", "x"), ("disabled", "0")]);
        attrs.set("color", "ffffff", ElementKind::Jump.order());
        let names: Vec<_> = attrs.0.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["text", "future", "color", "disabled"]);
    }

    #[test]
    fn empty_sequences_carry_the_colour() {
        let mut s = Sequence::new(Vec::new());
        assert_eq!(s.attrs.get("color"), Some("ffffff"));
        s.elements.push(Element {
            kind: ElementKind::Jump,
            attrs: Attrs::default(),
            branches: Vec::new(),
        });
        s.fit_color();
        assert_eq!(s.attrs.get("color"), None);
    }

    #[test]
    fn kinds_round_trip_through_their_tags() {
        for kind in ElementKind::ALL {
            assert_eq!(ElementKind::from_tag(kind.tag()), Some(kind));
        }
        assert_eq!(ElementKind::from_tag("qTrue"), None);
    }
}
