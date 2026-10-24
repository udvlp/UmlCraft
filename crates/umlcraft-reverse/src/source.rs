//! Finding source files and reading them as text.

use std::io;
use std::path::{Path, PathBuf};

/// The files among `paths` with one of `extensions` (without the dot, compared ignoring case):
/// files as given, directories searched recursively. Sorted and without duplicates, so imports
/// run in the same order on every system.
pub fn collect_sources(paths: &[PathBuf], extensions: &[&str]) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            walk(path, extensions, &mut found)?;
        } else if has_extension(path, extensions) {
            found.push(path.clone());
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn walk(dir: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(&path, extensions, found)?;
        } else if has_extension(&path, extensions) {
            found.push(path);
        }
    }
    Ok(())
}

/// A source file as text, with line ends normalized to `\n`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub text: String,
    /// Set when the encoding had to be guessed: the name of the encoding used.
    pub guessed: Option<&'static str>,
}

/// Decodes a source file: a byte order mark decides between UTF-8 and UTF-16; without one,
/// valid UTF-8 is taken as such and anything else as Windows-1252, the usual legacy encoding of
/// sources written on Windows.
pub fn decode(bytes: &[u8]) -> Decoded {
    let (text, guessed) = match encoding_rs::Encoding::for_bom(bytes) {
        Some((encoding, bom)) => (encoding.decode_without_bom_handling(&bytes[bom..]).0, None),
        None => match std::str::from_utf8(bytes) {
            Ok(text) => (text.into(), None),
            Err(_) => {
                let encoding = encoding_rs::WINDOWS_1252;
                (encoding.decode_without_bom_handling(bytes).0, Some(encoding.name()))
            }
        },
    };
    let text = if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.into_owned()
    };
    Decoded { text, guessed }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_by_bom_then_utf8_then_windows_1252() {
        assert_eq!(decode(b"\xEF\xBB\xBFclass \xC3\x84 {}").text, "class Ä {}");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("Ä\r\n".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(&utf16), Decoded { text: "Ä\n".into(), guessed: None });
        assert_eq!(decode("Ä\r\nb\rc".as_bytes()).text, "Ä\nb\nc");
        let legacy = decode(b"// Gr\xFC\xDFe");
        assert_eq!(legacy.text, "// Grüße");
        assert_eq!(legacy.guessed, Some("windows-1252"));
    }

    #[test]
    fn collects_recursively_by_extension_in_order() {
        let dir = std::env::temp_dir().join(format!("umlcraft-collect-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("b/c")).unwrap();
        for f in ["b/c/Z.java", "b/A.JAVA", "b/readme.txt", "Top.java"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        let found = collect_sources(&[dir.clone(), dir.join("Top.java")], &["java"]).unwrap();
        let names: Vec<String> = found
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(names, ["Top.java", "b/A.JAVA", "b/c/Z.java"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
