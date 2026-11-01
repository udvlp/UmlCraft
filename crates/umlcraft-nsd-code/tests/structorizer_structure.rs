//! Compares the structure of our diagrams with Structorizer's own Java and C imports of the
//! same fixture files: which elements, nested how. Texts differ by design (Structorizer writes
//! its own pseudocode). Structorizer's results live outside the repository; with
//! `UMLCRAFT_NSD_REFS` pointing at them (`java/structorizer-default/Class.method-N.nsd`,
//! `c/structorizer-default/File.function-N.nsd`) the test runs, otherwise it is skipped.
//! Methods whose structure differs on purpose are listed with the reason in
//! `structorizer_deviations.txt`.

use std::collections::BTreeMap;
use std::path::Path;

use umlcraft_nsd::{Element, Sequence, read};
use umlcraft_nsd_code::{Made, Source, SourceLanguage};

/// The kinds and nesting of a sequence, one line per element.
fn shape(seq: &Sequence, depth: usize, out: &mut String) {
    for e in &seq.elements {
        element(e, depth, out);
    }
}

fn element(e: &Element, depth: usize, out: &mut String) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(e.kind.tag());
    out.push('\n');
    for (i, b) in e.branches.iter().enumerate() {
        out.push_str(&format!("{}[{i}]\n", "  ".repeat(depth + 1)));
        shape(b, depth + 2, out);
    }
}

fn deviations() -> BTreeMap<String, String> {
    include_str!("structorizer_deviations.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(m, why)| (m.trim().to_owned(), why.trim().to_owned()))
        .collect()
}

#[test]
fn structure_matches_structorizers_import() {
    let Some(refs) = std::env::var_os("UMLCRAFT_NSD_REFS") else {
        eprintln!("UMLCRAFT_NSD_REFS not set; skipped");
        return;
    };
    let made = Made {
        author: "t",
        date: "2026-10-29",
        origin: "t",
        keywords: umlcraft_nsd::Keywords::English,
    };
    let expected_deviations = deviations();
    let mut compared = 0;
    let mut differing = Vec::new();
    let mut entries = Vec::new();
    for (refs_dir, folder, extension, language) in [
        ("java/structorizer-default", "java", "java", SourceLanguage::Java),
        ("c/structorizer-default", "cpp", "c", SourceLanguage::Cpp),
    ] {
        let dir = Path::new(&refs).join(refs_dir);
        let mut found: Vec<_> =
            std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        found.sort();
        entries.extend(found.into_iter().map(|p| (p, folder, extension, language)));
    }
    for (path, folder, extension, language) in entries {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(folder);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // `Class.method-N.nsd` (N = number of parameters); class diagrams have no `-`.
        let Some((qualified, _)) = name.strip_suffix(".nsd").and_then(|n| n.rsplit_once('-'))
        else {
            continue;
        };
        let Some((class, method)) = qualified.split_once('.') else { continue };
        let file = fixtures.join(format!("{class}.{extension}"));
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let source = Source::parse(language, &text);
        let Some(m) = source.methods().into_iter().find(|m| m.name == method) else {
            panic!("{qualified}: no such method in {}", file.display());
        };
        let (ours, _) = source.diagram(&m, &made).unwrap();
        let theirs = read(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let (mut a, mut b) = (String::new(), String::new());
        shape(&ours.children, 0, &mut a);
        shape(&theirs.children, 0, &mut b);
        compared += 1;
        if a != b {
            differing.push(qualified.to_owned());
            if !expected_deviations.contains_key(qualified) {
                panic!(
                    "{qualified} differs from Structorizer:\n--- ours\n{a}--- Structorizer\n{b}"
                );
            }
        }
    }
    eprintln!("{compared} methods compared, {} deviations as listed", differing.len());
    let stale: Vec<_> = expected_deviations.keys().filter(|k| !differing.contains(k)).collect();
    assert!(stale.is_empty(), "listed deviations that no longer differ: {stale:?}");
    assert!(compared >= 38);
}
