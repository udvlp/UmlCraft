//! Structograms as JSON for the MCP tools: a tree an AI client reads and writes instead of
//! Structorizer's XML.
//!
//! ```json
//! { "title": "total(a, n)", "kind": "sub", "keywords": "english",
//!   "body": [
//!     { "kind": "instruction", "text": "s := 0" },
//!     { "kind": "for", "text": "for i := 1 to n",
//!       "body": [{ "kind": "instruction", "text": "s := s + a[i]" }] },
//!     { "kind": "if", "text": "s > 100?", "then": [ … ], "else": [ … ] },
//!     { "kind": "case", "text": "unit?",
//!       "branches": [{ "label": "\"kg\"", "body": [ … ] }, { "label": "default", "body": [] }] },
//!     { "kind": "jump", "text": "return s" } ] }
//! ```
//!
//! Texts with several lines hold `\n`. A case's last branch is its default (label `%`: none);
//! a try block's text is the name of the caught exception, its parts `body`, `catch` and
//! `finally`; a parallel element has unlabeled `branches`. `comment`, `disabled` and `color`
//! (six hex digits) are optional.

use serde_json::{Value, json};
use umlcraft_nsd::edit::new_element;
use umlcraft_nsd::{
    Color, Diagram, Element, ElementKind, Keywords, Provenance, RootKind, Sequence,
};

const KINDS: [(&str, ElementKind); 11] = [
    ("instruction", ElementKind::Instruction),
    ("call", ElementKind::Call),
    ("jump", ElementKind::Jump),
    ("if", ElementKind::Alternative),
    ("case", ElementKind::Case),
    ("for", ElementKind::For),
    ("while", ElementKind::While),
    ("repeat", ElementKind::Repeat),
    ("forever", ElementKind::Forever),
    ("try", ElementKind::Try),
    ("parallel", ElementKind::Parallel),
];

/// The kinds' names, for the tool's schema.
pub fn kind_names() -> Vec<&'static str> {
    KINDS.iter().map(|(name, _)| *name).collect()
}

fn kind_name(kind: ElementKind) -> &'static str {
    KINDS.iter().find(|(_, k)| *k == kind).map_or("instruction", |(name, _)| name)
}

fn root_name(kind: RootKind) -> &'static str {
    match kind {
        RootKind::Program => "program",
        RootKind::Sub => "sub",
        RootKind::Includable => "includable",
    }
}

/// `d` as the JSON tree.
pub fn to_json(d: &Diagram) -> Value {
    let mut root = json!({
        "title": d.text().join("\n"),
        "kind": root_name(d.kind()),
        "keywords": match Keywords::of(d) {
            Keywords::English => "english",
            Keywords::German => "german",
        },
        "body": sequence(&d.children),
    });
    if !d.comment().iter().all(String::is_empty) {
        root["comment"] = json!(d.comment().join("\n"));
    }
    root
}

fn sequence(s: &Sequence) -> Value {
    Value::Array(s.elements.iter().map(element).collect())
}

fn element(e: &Element) -> Value {
    let text = e.text();
    let mut v = json!({ "kind": kind_name(e.kind) });
    let branch = |i: usize| e.branches.get(i).map_or(json!([]), sequence);
    match e.kind {
        ElementKind::Forever => {}
        ElementKind::Case => {
            v["text"] = json!(text.first().cloned().unwrap_or_default());
            let labels = text.iter().skip(1);
            let branches: Vec<Value> = labels
                .zip(&e.branches)
                .map(|(label, body)| json!({ "label": label, "body": sequence(body) }))
                .collect();
            v["branches"] = json!(branches);
        }
        ElementKind::Parallel => {
            v["branches"] = Value::Array(e.branches.iter().map(sequence).collect());
        }
        _ => v["text"] = json!(text.join("\n")),
    }
    match e.kind {
        ElementKind::Alternative => {
            v["then"] = branch(0);
            v["else"] = branch(1);
        }
        ElementKind::Try => {
            v["body"] = branch(0);
            v["catch"] = branch(1);
            v["finally"] = branch(2);
        }
        ElementKind::For | ElementKind::While | ElementKind::Repeat | ElementKind::Forever => {
            v["body"] = branch(0);
        }
        _ => {}
    }
    if !e.comment().iter().all(String::is_empty) {
        v["comment"] = json!(e.comment().join("\n"));
    }
    if e.disabled() {
        v["disabled"] = json!(true);
    }
    if e.color() != Color::WHITE {
        v["color"] = json!(e.color().to_hex());
    }
    v
}

/// The structogram the JSON tree `v` describes. Its header (keywords and the like) comes from
/// `base` where `v` does not set it, else from a new diagram's.
pub fn from_json(v: &Value, base: Option<&Diagram>) -> Result<Diagram, String> {
    let made = Provenance { author: "", date: "", origin: "" };
    let mut d = match base {
        Some(base) => base.clone(),
        None => Diagram::new(RootKind::Program, &[], &made),
    };
    if let Some(title) = v.get("title").and_then(Value::as_str) {
        d.set_text(&lines(title));
    } else if base.is_none() {
        return Err("`title` is missing".into());
    }
    match v.get("kind").and_then(Value::as_str) {
        Some(kind) => d.set_kind(
            RootKind::parse(kind)
                .ok_or_else(|| format!("`kind` is program, sub or includable, not `{kind}`"))?,
        ),
        None if base.is_none() => d.set_kind(RootKind::Program),
        None => {}
    }
    match v.get("keywords").and_then(Value::as_str) {
        Some("english") => d.set_keywords(Keywords::English),
        Some("german") => d.set_keywords(Keywords::German),
        Some(other) => return Err(format!("`keywords` is english or german, not `{other}`")),
        None => {}
    }
    if let Some(comment) = v.get("comment").and_then(Value::as_str) {
        d.set_comment(&lines(comment));
    }
    if let Some(body) = v.get("body") {
        let root = d.clone();
        d.children = sequence_of(&root, body, "body")?;
    }
    Ok(d)
}

fn lines(text: &str) -> Vec<String> {
    text.split('\n').map(|l| l.trim_end_matches('\r').to_owned()).collect()
}

fn sequence_of(d: &Diagram, v: &Value, at: &str) -> Result<Sequence, String> {
    let items = match v {
        Value::Null => return Ok(Sequence::new(Vec::new())),
        Value::Array(items) => items,
        _ => return Err(format!("`{at}` must be a list of elements")),
    };
    let elements = items
        .iter()
        .enumerate()
        .map(|(i, e)| element_of(d, e, &format!("{at}.{i}")))
        .collect::<Result<_, _>>()?;
    Ok(Sequence::new(elements))
}

fn element_of(d: &Diagram, v: &Value, at: &str) -> Result<Element, String> {
    let name = v.get("kind").and_then(Value::as_str).ok_or(format!("`{at}.kind` is missing"))?;
    let kind = KINDS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k).ok_or_else(|| {
        format!("`{at}.kind` is one of {}, not `{name}`", kind_names().join(", "))
    })?;
    let text = v.get("text").and_then(Value::as_str).unwrap_or_default();
    let part =
        |key: &str| sequence_of(d, v.get(key).unwrap_or(&Value::Null), &format!("{at}.{key}"));
    let branches: &[Value] = v.get("branches").and_then(Value::as_array).map_or(&[], Vec::as_slice);
    let mut e = match kind {
        ElementKind::Case => {
            if branches.len() < 2 {
                return Err(format!("`{at}` needs two branches or more: cases, then the default"));
            }
            let mut texts = vec![text.to_owned()];
            for b in branches {
                texts.push(b.get("label").and_then(Value::as_str).unwrap_or_default().to_owned());
            }
            let mut e = new_element(d, kind, &texts);
            for (i, b) in branches.iter().enumerate() {
                let body = b.get("body").unwrap_or(&Value::Null);
                e.branches[i] = sequence_of(d, body, &format!("{at}.branches.{i}.body"))?;
            }
            e
        }
        ElementKind::Parallel => {
            let count = branches.len().max(1);
            let mut e = new_element(d, kind, &[count.to_string()]);
            for (i, b) in branches.iter().enumerate() {
                e.branches[i] = sequence_of(d, b, &format!("{at}.branches.{i}"))?;
            }
            e
        }
        _ => new_element(d, kind, &lines(text)),
    };
    match kind {
        ElementKind::Alternative => e.branches = vec![part("then")?, part("else")?],
        ElementKind::Try => e.branches = vec![part("body")?, part("catch")?, part("finally")?],
        ElementKind::For | ElementKind::While | ElementKind::Repeat | ElementKind::Forever => {
            e.branches = vec![part("body")?];
        }
        _ => {}
    }
    if let Some(comment) = v.get("comment").and_then(Value::as_str) {
        e.set_comment(&lines(comment));
    }
    if let Some(disabled) = v.get("disabled").and_then(Value::as_bool) {
        e.set_disabled(disabled);
    }
    if let Some(color) = v.get("color").and_then(Value::as_str) {
        let hex = color.trim_start_matches('#');
        e.set_color(
            Color::parse(hex).ok_or_else(|| format!("`{at}.color` is not six hex digits"))?,
        );
    }
    Ok(e)
}

/// Inserts `element` at `path` of the JSON tree (`body.2`, `body.1.then.0`,
/// `body.0.branches.1.body.3`; the index may be the list's length to append), replaces the
/// element there, or removes it.
pub fn edit(
    tree: &mut Value,
    action: &str,
    path: &str,
    element: Option<&Value>,
) -> Result<(), String> {
    let mut keys: Vec<&str> = path.split('.').collect();
    let last = keys.pop().and_then(|k| k.parse::<usize>().ok());
    let Some(index) = last.filter(|_| !keys.is_empty()) else {
        return Err(format!("`{path}` does not end in a list and an index, as in `body.0`"));
    };
    let mut at: &mut Value = tree;
    for key in &keys {
        at = match at {
            Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get_mut(i)),
            Value::Object(fields) => fields.get_mut(*key),
            _ => None,
        }
        .ok_or_else(|| format!("the structogram has no `{path}`"))?;
    }
    let Value::Array(list) = at else {
        return Err(format!("`{}` is no list of elements", keys.join(".")));
    };
    let element = || element.cloned().ok_or_else(|| format!("`{action}` needs `element`"));
    match action {
        "insert" if index <= list.len() => list.insert(index, element()?),
        "replace" if index < list.len() => list[index] = element()?,
        "remove" if index < list.len() => {
            list.remove(index);
        }
        "insert" | "replace" | "remove" => {
            return Err(format!("`{path}`: the list has {} elements", list.len()));
        }
        _ => return Err(format!("`action` is insert, replace or remove, not `{action}`")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tree_goes_there_and_back() {
        let tree = json!({
            "title": "total(a, n)", "kind": "sub", "keywords": "german",
            "body": [
                { "kind": "instruction", "text": "s := 0\nk := 1", "comment": "start" },
                { "kind": "for", "text": "für i := 1 bis n",
                  "body": [{ "kind": "call", "text": "add(a[i])" }] },
                { "kind": "if", "text": "s > 100?",
                  "then": [{ "kind": "jump", "text": "gib 100" }], "else": [] },
                { "kind": "case", "text": "unit?", "branches": [
                    { "label": "\"kg\"", "body": [{ "kind": "instruction", "text": "f := 1" }] },
                    { "label": "default", "body": [] } ] },
                { "kind": "while", "text": "solange x > 0", "body": [] },
                { "kind": "repeat", "text": "bis x = 0", "body": [] },
                { "kind": "forever", "body": [{ "kind": "instruction", "text": "tick",
                                                 "disabled": true, "color": "ff8080" }] },
                { "kind": "try", "text": "e", "body": [], "catch": [], "finally": [] },
                { "kind": "parallel", "branches": [[], [{ "kind": "instruction", "text": "b" }], []] },
                { "kind": "jump", "text": "gib s" },
            ],
        });
        let d = umlcraft_nsd::content::normalize(&from_json(&tree, None).unwrap());
        assert_eq!(d.kind(), RootKind::Sub);
        assert_eq!(Keywords::of(&d), Keywords::German);
        assert_eq!(to_json(&d), tree);
        // Through Structorizer's file and back.
        let file = umlcraft_nsd::read(&umlcraft_nsd::write(&d)).unwrap();
        assert_eq!(to_json(&umlcraft_nsd::content::normalize(&file)), tree);
    }

    #[test]
    fn elements_change_by_their_path() {
        let instruction = |text: &str| json!({ "kind": "instruction", "text": text });
        let mut tree = json!({ "title": "t", "body": [
            { "kind": "if", "text": "x?", "then": [instruction("a")], "else": [] } ] });
        edit(&mut tree, "insert", "body.0.then.1", Some(&instruction("b"))).unwrap();
        edit(&mut tree, "insert", "body.0", Some(&instruction("first"))).unwrap();
        edit(&mut tree, "replace", "body.1.then.0", Some(&instruction("A"))).unwrap();
        edit(&mut tree, "insert", "body.1.else.0", Some(&instruction("c"))).unwrap();
        edit(&mut tree, "remove", "body.0", None).unwrap();
        assert_eq!(
            tree["body"][0],
            json!({ "kind": "if", "text": "x?", "then": [instruction("A"), instruction("b")],
                    "else": [instruction("c")] })
        );
        assert!(edit(&mut tree, "remove", "body.5", None).is_err());
        assert!(edit(&mut tree, "insert", "body.0.then", Some(&instruction("x"))).is_err());
        assert!(edit(&mut tree, "insert", "body.0.text.0", Some(&instruction("x"))).is_err());
        assert!(edit(&mut tree, "replace", "body.0", None).is_err());
        assert!(edit(&mut tree, "move", "body.0", None).is_err());
    }

    #[test]
    fn mistakes_are_named() {
        let bad = |tree: Value| from_json(&tree, None).unwrap_err();
        assert!(bad(json!({ "body": [] })).contains("title"));
        let case =
            json!({ "title": "t", "body": [{ "kind": "case", "text": "x?", "branches": [] }] });
        assert!(bad(case).contains("body.0"));
        let kind = json!({ "title": "t", "body": [{ "kind": "goto" }] });
        assert!(bad(kind).contains("goto"));
    }
}
