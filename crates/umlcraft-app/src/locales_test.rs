//! The locale files against the English one. German and English come with every new text;
//! another language may lag behind: rust-i18n shows a text it lacks in English. No file has a
//! key that English has not, which would be a text nobody shows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The folder of the locale files.
const LOCALES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales");

/// The languages every new text comes in, the maintainer's; the others may lag behind.
pub(crate) const COMPLETE: [&str; 2] = ["de", "en"];

/// The texts of `value` by their keys as dotted paths (`toolbar.undo`). Only maps go deeper;
/// anything else, lists included, is a text.
fn flatten(value: &Value, prefix: &str, texts: &mut BTreeMap<String, String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let path = match prefix {
                    "" => key.clone(),
                    _ => format!("{prefix}.{key}"),
                };
                flatten(value, &path, texts);
            }
        }
        Value::String(text) => {
            texts.insert(prefix.to_owned(), text.clone());
        }
        other => {
            texts.insert(prefix.to_owned(), other.to_string());
        }
    }
}

/// The texts of the locale file `path` by their keys, without the structogram keywords: those
/// are no texts of the user interface, and `umlcraft-nsd`'s build checks them.
fn texts_of(path: &Path) -> BTreeMap<String, String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{shown}: {e}"));
    let root: Value =
        serde_saphyr::from_str(&text).unwrap_or_else(|e| panic!("{shown}: invalid YAML: {e}"));
    let mut texts = BTreeMap::new();
    flatten(&root, "", &mut texts);
    texts.retain(|k, _| !k.starts_with("structogram."));
    texts
}

/// The keys of the locale file `path`, as `texts_of` reads them.
fn keys_of(path: &Path) -> BTreeSet<String> {
    texts_of(path).into_keys().collect()
}

/// The locale files, sorted, and among them the English one.
fn locale_files() -> (Vec<PathBuf>, PathBuf) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(LOCALES)
        .unwrap_or_else(|e| panic!("{LOCALES}: {e}"))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yml"))
        .collect();
    files.sort();
    let english = files
        .iter()
        .find(|p| p.file_stem().is_some_and(|s| s == "en"))
        .expect("locales/en.yml")
        .clone();
    (files, english)
}

/// The placeholders of `text`: each `%{name}`. A `%` without a brace is a plain character.
fn placeholders(text: &str) -> BTreeSet<&str> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(start) = rest.find("%{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else { break };
        found.insert(&rest[start..start + 2 + end + 1]);
        rest = &after[end + 1..];
    }
    found
}

#[test]
fn locales_have_the_keys_of_english() {
    let (files, english) = locale_files();
    let english = keys_of(&english);
    assert!(english.len() > 100, "en.yml has only {} keys", english.len());
    let mut wrong = Vec::new();
    for file in &files {
        let locale = file.file_stem().unwrap().to_string_lossy();
        let keys = keys_of(file);
        let mut missing: Vec<&String> = english.difference(&keys).collect();
        // A language may lack texts, but never its own name.
        if !COMPLETE.contains(&&*locale) {
            missing.retain(|k| *k == "language.name");
        }
        let extra: Vec<&String> = keys.difference(&english).collect();
        if !missing.is_empty() || !extra.is_empty() {
            wrong.push(format!("{locale}: missing {missing:?}, extra {extra:?}"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// rust-i18n shows a placeholder it does not fill as written (`%{version}`), so a translation
/// must keep every placeholder of the English text, and add none.
#[test]
fn every_locale_has_the_placeholders_of_english() {
    let (files, english) = locale_files();
    let english = texts_of(&english);
    assert!(english.values().any(|t| !placeholders(t).is_empty()));
    let mut wrong = Vec::new();
    for file in &files {
        let locale = file.file_stem().unwrap().to_string_lossy();
        for (key, text) in texts_of(file) {
            let Some(original) = english.get(&key) else { continue };
            let (theirs, ours) = (placeholders(original), placeholders(&text));
            if theirs != ours {
                wrong.push(format!("{locale}: {key} has {ours:?}, en.yml {theirs:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn placeholders_are_the_braced_names() {
    assert_eq!(placeholders("%{a} – %{b} %"), BTreeSet::from(["%{a}", "%{b}"]));
    assert_eq!(placeholders("% for none"), BTreeSet::new());
    assert_eq!(placeholders("%{open"), BTreeSet::new());
}
