//! What the Java front-end reads from each fixture, compared with `expected.ir` next to the
//! sources. Run with `UPDATE_SNAPSHOTS=1` to write the snapshots after a deliberate change, and
//! review the diff.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use umlcraft_reverse::{Problem, SourceLanguage, collect_sources_for, parse_sources};

fn fixture(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/java").join(case)
}

/// Everything parsed from a fixture folder as text: each file's IR, then what was left out,
/// failed or decoded by guess.
fn dump(case: &str) -> String {
    let dir = fixture(case);
    let files = collect_sources_for(SourceLanguage::Java, std::slice::from_ref(&dir)).unwrap();
    let parsed = parse_sources(SourceLanguage::Java, &files);
    let name = |p: &Path| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/");
    let mut out = String::new();
    for unit in &parsed.units {
        let _ = writeln!(out, "== {}", name(&unit.path));
        out.push_str(&unit.dump());
    }
    for (skip, n) in &parsed.skipped {
        let _ = writeln!(out, "skipped {} × {n}", skip.key());
    }
    for failed in &parsed.failed {
        let _ = match &failed.problem {
            Problem::Syntax(e) => writeln!(
                out,
                "failed {} at {}:{} near {:?}",
                name(&failed.path),
                e.line,
                e.column,
                e.token
            ),
            Problem::Read(e) => writeln!(out, "failed {}: {e}", name(&failed.path)),
        };
    }
    for (path, encoding) in &parsed.guessed_encodings {
        let _ = writeln!(out, "guessed {} as {encoding}", name(path));
    }
    out
}

fn check(case: &str) {
    let actual = dump(case);
    let path = fixture(case).join("expected.ir");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing; run with UPDATE_SNAPSHOTS=1", path.display()))
        .replace("\r\n", "\n");
    pretty_assertions::assert_eq!(expected, actual, "{case}");
}

#[test]
fn packages() {
    check("packages");
}

#[test]
fn kinds() {
    check("kinds");
}

#[test]
fn modifiers() {
    check("modifiers");
}

#[test]
fn javadoc() {
    check("javadoc");
}

#[test]
fn annotations() {
    check("annotations");
}

#[test]
fn initializers() {
    check("initializers");
}

#[test]
fn inheritance() {
    check("inheritance");
}

#[test]
fn generics() {
    check("generics");
}

#[test]
fn arrays_varargs() {
    check("arrays_varargs");
}

#[test]
fn syntax_error() {
    check("syntax_error");
}

#[test]
fn encoding() {
    check("encoding");
}
