//! What a UmlCraft project keeps of a diagram: its content, not the file it came from.
//!
//! A project holds diagrams in their normal form ([`normalize`]): the root's text, comment,
//! colour, kind, style, include list and namespace, the parser keywords that differ from those
//! of a fresh Structorizer installation, and the elements with the attributes this crate knows,
//! for loops' attributes fitted to their text. The file's header (version, author, dates,
//! origin, licence) and unknown attributes are left out; [`to_file`] gives a diagram a header
//! again for writing a `.nsd` file. With the `serde` feature a diagram serializes as that
//! content, element by element, so that a project file shows changes line by line.

use crate::create::{DEFAULT_KEYWORDS, Provenance};
use crate::edit::fit_for;
use crate::model::{Color, Diagram, Element, ElementKind, Sequence};

/// The root attributes that tell Structorizer's parser its keywords.
pub const KEYWORDS: &[&str] = &[
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
];

/// The keyword a fresh Structorizer installation has for `name`; the ones it does not write
/// are empty.
fn default_keyword(name: &str) -> &'static str {
    DEFAULT_KEYWORDS.iter().find(|(n, _)| *n == name).map_or("", |(_, v)| v)
}

/// The keywords of `d` that differ from the defaults, in the order Structorizer writes them.
pub fn custom_keywords(d: &Diagram) -> Vec<(&'static str, String)> {
    KEYWORDS
        .iter()
        .filter_map(|&name| {
            let value = d.header(name)?;
            (value != default_keyword(name)).then(|| (name, value.to_owned()))
        })
        .collect()
}

/// The content of `d` in its normal form. Normalizing twice changes nothing more.
pub fn normalize(d: &Diagram) -> Diagram {
    let mut root = Diagram { attrs: Default::default(), children: Sequence::new(Vec::new()) };
    for (name, value) in custom_keywords(d) {
        root.set_header(name, value);
    }
    for name in ["includeList", "namespace"] {
        if let Some(value) = d.header(name).filter(|v| !v.is_empty()) {
            root.set_header(name, value);
        }
    }
    root.set_text(&d.text());
    root.set_comment(&d.comment());
    root.set_header("color", root_color(d).to_hex());
    root.set_kind(d.kind());
    root.set_style(d.style());
    root.children = normal_sequence(&root, &d.children);
    root
}

fn root_color(d: &Diagram) -> Color {
    d.header("color").and_then(Color::parse).unwrap_or(Color::WHITE)
}

fn normal_sequence(root: &Diagram, s: &Sequence) -> Sequence {
    Sequence::new(s.elements.iter().map(|e| normal_element(root, e)).collect())
}

fn normal_element(root: &Diagram, e: &Element) -> Element {
    let flag = |name: &str| e.attr(name) == Some("1");
    let mut n = Element { kind: e.kind, attrs: Default::default(), branches: Vec::new() };
    n.branches = e.branches.iter().map(|b| normal_sequence(root, b)).collect();
    n.set_text(&e.text());
    n.set_comment(&e.comment());
    n.set_color(e.color());
    match e.kind {
        ElementKind::Instruction => n.set_attr("rotated", bit(flag("rotated"))),
        ElementKind::Call => n.set_attr("ismethoddecl", bit(flag("ismethoddecl"))),
        ElementKind::Alternative | ElementKind::Case => {
            let mut colors: Vec<String> = e
                .branch_colors()
                .into_iter()
                .map(|c| c.map(Color::to_hex).unwrap_or_default())
                .collect();
            colors.resize(n.branches.len().max(1), String::new());
            n.set_attr("branch_colors", colors.join(","));
        }
        ElementKind::For => fit_for(root, &mut n),
        _ => {}
    }
    n.set_disabled(e.disabled());
    n
}

fn bit(on: bool) -> &'static str {
    if on { "1" } else { "0" }
}

/// `d` (in normal form) as a file: with the header of a new diagram made by `made`, and the
/// keywords that differ from the defaults.
pub fn to_file(d: &Diagram, made: &Provenance) -> Diagram {
    let mut file = Diagram::new(d.kind(), &d.text(), made);
    for (name, value) in custom_keywords(d) {
        file.set_header(name, value);
    }
    for name in ["includeList", "namespace"] {
        if let Some(value) = d.header(name).filter(|v| !v.is_empty()) {
            file.set_header(name, value);
        }
    }
    file.set_comment(&d.comment());
    file.set_header("color", root_color(d).to_hex());
    file.set_style(d.style());
    file.children = d.children.clone();
    file
}

/// Whether every element has the branches its kind takes.
pub fn branches_fit(kind: ElementKind, count: usize) -> bool {
    match kind.branches() {
        crate::model::Branches::Fixed(fixed) => fixed.len() == count,
        crate::model::Branches::Repeated(_) => count >= 1,
    }
}

#[cfg(feature = "serde")]
mod records {
    //! The content as serde records.

    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::*;
    use crate::model::{RootKind, RootStyle};

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Root {
        #[serde(default, skip_serializing_if = "is_program")]
        r#type: String,
        #[serde(default, skip_serializing_if = "is_nice")]
        style: String,
        #[serde(default)]
        text: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        comment: Vec<String>,
        #[serde(default, skip_serializing_if = "is_white")]
        color: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        include_list: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        namespace: String,
        #[serde(default, skip_serializing_if = "indexmap::IndexMap::is_empty")]
        keywords: indexmap::IndexMap<String, String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        body: Vec<Item>,
    }

    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Item {
        kind: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        text: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        comment: Vec<String>,
        #[serde(default, skip_serializing_if = "is_white")]
        color: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        disabled: bool,
        /// Instructions: drawn turned by 90° in a case with many branches.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rotated: bool,
        /// Calls: a method declaration.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        method_declaration: bool,
        /// Alternatives and cases: the colours of the branch heads, empty for the element's.
        #[serde(default, skip_serializing_if = "no_colors")]
        branch_colors: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        branches: Vec<Vec<Item>>,
    }

    fn is_program(s: &str) -> bool {
        s.is_empty() || s == RootKind::Program.as_str()
    }

    fn is_nice(s: &str) -> bool {
        s.is_empty() || s == RootStyle::Nice.as_str()
    }

    fn is_white(s: &str) -> bool {
        s.is_empty() || s == Color::WHITE.to_hex()
    }

    fn no_colors(colors: &[String]) -> bool {
        colors.iter().all(String::is_empty)
    }

    fn item(e: &Element) -> Item {
        let flag = |name: &str| e.attr(name) == Some("1");
        Item {
            kind: e.kind.tag().to_owned(),
            text: e.text(),
            comment: e.comment(),
            color: e.color().to_hex(),
            disabled: e.disabled(),
            rotated: flag("rotated"),
            method_declaration: flag("ismethoddecl"),
            branch_colors: e
                .branch_colors()
                .into_iter()
                .map(|c| c.map(Color::to_hex).unwrap_or_default())
                .collect(),
            branches: e.branches.iter().map(|b| b.elements.iter().map(item).collect()).collect(),
        }
    }

    /// The element of `item` as read, put into normal form by the caller once for the whole
    /// body (normalizing each element on the way up would redo its subtree at every level).
    fn element<E: serde::de::Error>(item: Item) -> Result<Element, E> {
        let kind = ElementKind::from_tag(&item.kind)
            .ok_or_else(|| E::custom(format!("unknown structogram element `{}`", item.kind)))?;
        if !branches_fit(kind, item.branches.len()) {
            return Err(E::custom(format!(
                "a structogram's `{}` has {} branches",
                item.kind,
                item.branches.len()
            )));
        }
        let mut e = Element { kind, attrs: Default::default(), branches: Vec::new() };
        for branch in item.branches {
            let elements = branch.into_iter().map(element).collect::<Result<_, E>>()?;
            e.branches.push(Sequence::new(elements));
        }
        e.set_text(&item.text);
        e.set_comment(&item.comment);
        e.set_color(color(&item.color)?);
        e.set_attr("rotated", bit(item.rotated));
        e.set_attr("ismethoddecl", bit(item.method_declaration));
        for c in &item.branch_colors {
            color::<E>(c)?;
        }
        e.set_attr("branch_colors", item.branch_colors.join(","));
        e.set_disabled(item.disabled);
        Ok(e)
    }

    /// A colour, white when empty.
    fn color<E: serde::de::Error>(hex: &str) -> Result<Color, E> {
        if hex.is_empty() {
            return Ok(Color::WHITE);
        }
        Color::parse(hex).ok_or_else(|| E::custom(format!("`{hex}` is not a colour")))
    }

    impl Serialize for Diagram {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let header = |name: &str| self.header(name).unwrap_or_default().to_owned();
            Root {
                r#type: self.kind().as_str().to_owned(),
                style: self.style().as_str().to_owned(),
                text: self.text(),
                comment: self.comment(),
                color: root_color(self).to_hex(),
                include_list: header("includeList"),
                namespace: header("namespace"),
                keywords: custom_keywords(self)
                    .into_iter()
                    .map(|(n, v)| (n.to_owned(), v))
                    .collect(),
                body: self.children.elements.iter().map(item).collect(),
            }
            .serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for Diagram {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Diagram, D::Error> {
            let r = Root::deserialize(deserializer)?;
            let kind = match r.r#type.as_str() {
                "" => RootKind::Program,
                t => RootKind::parse(t)
                    .ok_or_else(|| D::Error::custom(format!("unknown structogram type `{t}`")))?,
            };
            let style = match r.style.as_str() {
                "" => RootStyle::Nice,
                s => RootStyle::parse(s)
                    .ok_or_else(|| D::Error::custom(format!("unknown structogram style `{s}`")))?,
            };
            let mut d = Diagram { attrs: Default::default(), children: Sequence::new(Vec::new()) };
            for (name, value) in r.keywords {
                if !KEYWORDS.contains(&name.as_str()) {
                    return Err(D::Error::custom(format!("unknown structogram keyword `{name}`")));
                }
                d.set_header(&name, value);
            }
            d.set_header("includeList", r.include_list);
            d.set_header("namespace", r.namespace);
            d.set_text(&r.text);
            d.set_comment(&r.comment);
            d.set_header("color", color::<D::Error>(&r.color)?.to_hex());
            d.set_kind(kind);
            d.set_style(style);
            let mut d = normalize(&d);
            let body = r.body.into_iter().map(element).collect::<Result<_, D::Error>>()?;
            d.children = normal_sequence(&d, &Sequence::new(body));
            Ok(d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RootKind;
    use crate::{lines, read, write};

    fn fixtures() -> Vec<(String, Diagram)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut out = Vec::new();
        let mut stack = vec![dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "nsd") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    out.push((path.display().to_string(), read(&text).unwrap()));
                }
            }
        }
        assert!(out.len() >= 45);
        out
    }

    #[test]
    fn normalizing_twice_changes_nothing() {
        for (name, d) in fixtures() {
            let n = normalize(&d);
            assert_eq!(normalize(&n), n, "{name}");
        }
    }

    #[test]
    fn the_normal_form_draws_the_same() {
        let m = crate::Metrics::for_platform();
        for (name, d) in fixtures() {
            assert_eq!(crate::svg::render(&normalize(&d), m), crate::svg::render(&d, m), "{name}");
        }
    }

    #[test]
    fn the_header_goes_and_the_content_stays() {
        let file = r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns:nsd="https://structorizer.fisch.lu" version="3.30" preFor="für" postFor="bis" author="ann" created="2026-10-01" origin="x" future="1" text="&#34;f(a)&#34;" comment="" color="ffffff" type="sub" style="nice">
	<children>
		<instruction text="&#34;a := 1&#34;" comment="&#34;why&#34;" color="ff8080" rotated="0" disabled="1" other="2"></instruction>
	</children>
</root>"#;
        let n = normalize(&read(file).unwrap());
        assert_eq!(n.header("author"), None);
        assert_eq!(n.header("version"), None);
        assert_eq!(n.header("future"), None);
        assert_eq!(n.header("preFor"), Some("für"));
        assert_eq!(n.text(), lines(&["f(a)"]));
        assert_eq!(n.kind(), RootKind::Sub);
        let e = &n.children.elements[0];
        assert_eq!(e.comment(), lines(&["why"]));
        assert!(e.disabled());
        assert_eq!(e.attr("other"), None);
    }

    #[test]
    fn a_file_gets_a_header_and_keeps_its_own_keywords() {
        let made = Provenance { author: "bob", date: "2026-10-30", origin: "UmlCraft" };
        let mut d = Diagram::new(RootKind::Program, &lines(&["p"]), &made);
        d.set_header("preFor", "für");
        let file = to_file(&normalize(&d), &made);
        let text = write(&file);
        assert!(text.contains(r#"preFor="für""#), "{text}");
        assert!(text.contains(r#"author="bob" created="2026-10-30""#), "{text}");
        assert_eq!(normalize(&read(&text).unwrap()), normalize(&d));
    }

    #[test]
    fn a_new_diagram_is_its_own_content_apart_from_the_header() {
        let made = Provenance { author: "a", date: "2026-10-30", origin: "o" };
        let d = Diagram::new(RootKind::Program, &lines(&["p"]), &made);
        assert_eq!(write(&to_file(&normalize(&d), &made)), write(&d));
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;
    use crate::read;

    #[test]
    fn content_survives_json() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut stack = vec![dir];
        let mut count = 0;
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "nsd") {
                    continue;
                }
                let d = normalize(&read(&std::fs::read_to_string(&path).unwrap()).unwrap());
                let json = serde_json::to_string_pretty(&d).unwrap();
                let back: Diagram = serde_json::from_str(&json).unwrap();
                assert_eq!(back, d, "{}\n{json}", path.display());
                count += 1;
            }
        }
        assert!(count >= 45);
    }

    #[test]
    fn broken_content_is_refused() {
        let wrong =
            r#"{"text":["p"],"body":[{"kind":"alternative","text":["a?"],"branches":[[]]}]}"#;
        assert!(serde_json::from_str::<Diagram>(wrong).is_err());
        let unknown = r#"{"text":["p"],"body":[{"kind":"goto"}]}"#;
        assert!(serde_json::from_str::<Diagram>(unknown).is_err());
        let keyword = r#"{"text":["p"],"keywords":{"preGoto":"x"}}"#;
        assert!(serde_json::from_str::<Diagram>(keyword).is_err());
    }

    #[test]
    fn json_reads_line_by_line() {
        let json = r#"{"text":["p"],"body":[{"kind":"for","text":["for i <- 1 to 3"],"branches":[[{"kind":"instruction","text":["x"]}]]}]}"#;
        let d: Diagram = serde_json::from_str(json).unwrap();
        let f = &d.children.elements[0];
        assert_eq!(f.attr("style"), Some("COUNTER"));
        assert_eq!(f.attr("endValue"), Some("3"));
        let again = serde_json::to_value(&d).unwrap();
        assert_eq!(again, serde_json::from_str::<serde_json::Value>(json).unwrap());
    }
}
