//! Reads the `structogram:` section of every `locales/*.yml` and writes the keyword table
//! `Keywords` is built on (`$OUT_DIR/keywords.rs`). A language is added by adding its file.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The parser keywords of a Structorizer header, in the order it writes them.
const HEADER: [&str; 14] = [
    "preRepeat",
    "postFor",
    "preReturn",
    "postForIn",
    "preWhile",
    "output",
    "input",
    "preFor",
    "preExit",
    "preLeave",
    "ignoreCase",
    "preThrow",
    "preForIn",
    "stepFor",
];

/// What `Keywords` knows of one language.
struct Language {
    code: String,
    name: String,
    header: Vec<(&'static str, String)>,
    branches: [String; 2],
    new: [String; 3],
    examples: [String; 2],
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|entry| entry.expect("cannot list the locales").path())
        .filter(|path| path.extension().is_some_and(|e| e == "yml"))
        .collect();
    files.sort();

    let mut languages = Vec::new();
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        if let Some(language) = read(file) {
            languages.push(language);
        }
    }
    languages.sort_by(|a, b| a.code.cmp(&b.code));
    if !languages.iter().any(|l| l.code == "en") {
        panic!("locales/en.yml has no `structogram:` section; English is the fallback");
    }
    let mut seen: HashMap<String, &str> = HashMap::new();
    for l in &languages {
        let while_keyword = l.header.iter().find(|(n, _)| *n == "preWhile").unwrap().1.clone();
        if let Some(other) = seen.insert(while_keyword.trim().to_lowercase(), &l.code) {
            panic!(
                "locales/{}.yml and locales/{other}.yml have the same `structogram.header.\
                 preWhile`; it tells the languages apart",
                l.code
            );
        }
    }

    let mut out = String::from("static SETS: &[KeywordSet] = &[\n");
    for l in &languages {
        let header: Vec<String> = l.header.iter().map(|(n, v)| format!("({n:?}, {v:?})")).collect();
        writeln!(
            out,
            "    KeywordSet {{ code: {:?}, name: {:?}, header: &[{}], branches: ({:?}, {:?}), \
             new: [{:?}, {:?}, {:?}], examples: [{:?}, {:?}] }},",
            l.code,
            l.name,
            header.join(", "),
            l.branches[0],
            l.branches[1],
            l.new[0],
            l.new[1],
            l.new[2],
            l.examples[0],
            l.examples[1],
        )
        .unwrap();
    }
    out.push_str("];\n");
    let english = languages.iter().position(|l| l.code == "en").unwrap();
    writeln!(out, "const ENGLISH: usize = {english};").unwrap();
    let target = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("keywords.rs");
    std::fs::write(&target, out).unwrap_or_else(|e| panic!("cannot write keywords.rs: {e}"));
}

/// The language of `file`, or `None` when it has no `structogram:` section.
fn read(file: &Path) -> Option<Language> {
    let shown = file.file_name().unwrap().to_string_lossy().into_owned();
    let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{shown}: {e}"));
    let root: Value =
        serde_saphyr::from_str(&text).unwrap_or_else(|e| panic!("{shown}: invalid YAML: {e}"));
    let section = root.get("structogram")?;
    let text_of = |group: &str, value: &Value, key: &str| -> String {
        match value.get(key).and_then(Value::as_str) {
            Some(s) => s.to_owned(),
            None => panic!(
                "{shown}: `structogram.{group}.{key}` needs a quoted text (an unquoted \
                 `true` is a YAML boolean)"
            ),
        }
    };
    let group = |name: &str| -> &Value {
        section.get(name).unwrap_or_else(|| panic!("{shown}: `structogram.{name}` is missing"))
    };
    let header = group("header");
    let given = header.as_object().map_or(0, |o| o.len());
    for key in header.as_object().into_iter().flat_map(|o| o.keys()) {
        if !HEADER.contains(&key.as_str()) {
            panic!("{shown}: `structogram.header.{key}` is not a Structorizer keyword");
        }
    }
    if given != HEADER.len() {
        panic!("{shown}: `structogram.header` needs exactly the {} keywords", HEADER.len());
    }
    let branches = group("branches");
    let new = group("new");
    let examples = group("examples");
    let language = root.get("language").unwrap_or(&Value::Null);
    let name = match language.get("name").and_then(Value::as_str) {
        Some(name) => name.to_owned(),
        None => panic!("{shown}: `language.name` is missing"),
    };
    Some(Language {
        code: file.file_stem().unwrap().to_string_lossy().into_owned(),
        name,
        header: HEADER.iter().map(|&n| (n, text_of("header", header, n))).collect(),
        branches: [text_of("branches", branches, "true"), text_of("branches", branches, "false")],
        new: [
            text_of("new", new, "for"),
            text_of("new", new, "while"),
            text_of("new", new, "repeat"),
        ],
        examples: [
            text_of("examples", examples, "counting"),
            text_of("examples", examples, "traversing"),
        ],
    })
}
