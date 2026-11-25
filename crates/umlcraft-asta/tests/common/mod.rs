//! Helpers for the tests that read Astah's own files; umlcraft-asta-export's tests use them too.

#![allow(dead_code, reason = "each test file uses some of the helpers")]

use std::path::{Path, PathBuf};

/// Astah's install folder: `ASTAH_HOME`, else where Astah Professional installs on Windows.
pub fn astah_home() -> PathBuf {
    std::env::var_os("ASTAH_HOME")
        .map_or_else(|| PathBuf::from(r"C:\Program Files\astah-professional"), PathBuf::from)
}

/// Reports a test that cannot run here. With `UMLCRAFT_REQUIRE_ASTAH` set, as on a machine that
/// has Astah, a skip fails instead, so a missing installation cannot pass for a green run.
pub fn skip(reason: &str) {
    assert!(std::env::var_os("UMLCRAFT_REQUIRE_ASTAH").is_none(), "not skipped: {reason}");
    eprintln!("skipped: {reason}");
}

/// The `.asta` files in `dir` and its subfolders, added to `out`.
pub fn asta_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            asta_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "asta") {
            out.push(path);
        }
    }
}
