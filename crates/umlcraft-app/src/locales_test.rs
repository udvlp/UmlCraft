//! The locale files against the English one. German and English come with every new text;
//! another language may lag behind: rust-i18n shows a text it lacks in English. No file has a
//! key that English has not, which would be a text nobody shows.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde_json::Value;

/// The folder of the locale files.
const LOCALES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../locales");

/// The languages every new text comes in, the maintainer's; the others may lag behind.
pub(crate) const COMPLETE: [&str; 2] = ["de", "en"];

/// The keys of `value` as dotted paths (`toolbar.undo`). Only maps go deeper; anything else,
/// lists included, is a text.
fn flatten(value: &Value, prefix: &str, keys: &mut BTreeSet<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let path = match prefix {
                    "" => key.clone(),
                    _ => format!("{prefix}.{key}"),
                };
                flatten(value, &path, keys);
            }
        }
        _ => {
            keys.insert(prefix.to_owned());
        }
    }
}

/// The keys of the locale file `path`, without the structogram keywords: those are no texts of
/// the user interface, and `umlcraft-nsd`'s build checks them.
fn keys_of(path: &std::path::Path) -> BTreeSet<String> {
    let shown = path.display();
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{shown}: {e}"));
    let root: Value =
        serde_saphyr::from_str(&text).unwrap_or_else(|e| panic!("{shown}: invalid YAML: {e}"));
    let mut keys = BTreeSet::new();
    flatten(&root, "", &mut keys);
    keys.retain(|k| !k.starts_with("structogram."));
    keys
}

#[test]
fn locales_have_the_keys_of_english() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(LOCALES)
        .unwrap_or_else(|e| panic!("{LOCALES}: {e}"))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "yml"))
        .collect();
    files.sort();
    let english =
        files.iter().find(|p| p.file_stem().is_some_and(|s| s == "en")).expect("locales/en.yml");
    let english = keys_of(english);
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
