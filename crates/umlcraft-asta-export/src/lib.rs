//! Export to Astah project files (`.asta`) through an installed Astah.
//!
//! An `.asta` file is Astah's Java-serialized object graph, which is fragile to write and changes
//! between Astah versions. So instead of writing it, the export drives Astah's official API: the
//! Java tool in `tools/asta-export` reads a `.ucraft` file plus a layout sidecar and builds the
//! project with the API's model and diagram editors. It runs in source-file mode on the JRE that
//! comes with Astah, so nothing has to be compiled beforehand.

mod layout;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use thiserror::Error;
use umlcraft_model::Model;
use umlcraft_notation::scene::{ApproxMeasure, TextMeasure};

/// The Java tool, written to a temporary folder for each run.
const TOOL_SOURCE: &str = include_str!("../../../tools/asta-export/AstaExport.java");
const TOOL_FILE: &str = "AstaExport.java";

/// Main jar of each Astah edition that has the API.
const EDITION_JARS: &[&str] = &["astah-pro.jar", "astah-uml.jar", "astah-sysml.jar"];
/// Install folder names under "Program Files".
const INSTALL_DIRS: &[&str] = &["astah-professional", "astah-uml", "astah-sysml"];

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Java could not be started ({java}): {source}")]
    Launch { java: PathBuf, source: io::Error },
    #[error("Astah reported: {0}")]
    Astah(String),
    #[error("the export tool failed ({status}): {output}")]
    Tool { status: String, output: String },
}

/// What could not be carried over. `dropped` counts by kind, with the keys the tool reports
/// (e.g. `query`, `extendCondition`, `systemBoundary`); `problems` describes single failures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub dropped: BTreeMap<String, usize>,
    pub problems: Vec<String>,
}

impl Report {
    pub fn is_complete(&self) -> bool {
        self.dropped.is_empty() && self.problems.is_empty()
    }
}

/// An Astah installation whose API can be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Astah {
    home: PathBuf,
    java: PathBuf,
    jars: Vec<PathBuf>,
}

impl Astah {
    /// The installation named by `ASTAH_HOME`, else the first one found in the usual places.
    pub fn find() -> Option<Self> {
        if let Some(home) = std::env::var_os("ASTAH_HOME") {
            return Self::at(Path::new(&home));
        }
        let mut roots = Vec::new();
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(dir) = std::env::var_os(var) {
                roots.push(PathBuf::from(dir));
            }
        }
        roots
            .iter()
            .flat_map(|root| INSTALL_DIRS.iter().map(move |dir| root.join(dir)))
            .find_map(|home| Self::at(&home))
    }

    /// The installation in `home`, if it has the API.
    pub fn at(home: &Path) -> Option<Self> {
        let api = home.join("astah-api.jar");
        let edition = EDITION_JARS.iter().map(|jar| home.join(jar)).find(|p| p.is_file())?;
        if !api.is_file() {
            return None;
        }
        let bundled =
            home.join("jre").join("bin").join(if cfg!(windows) { "java.exe" } else { "java" });
        // Without a bundled runtime, a Java 21 or newer on the PATH will do.
        let java = if bundled.is_file() { bundled } else { PathBuf::from("java") };
        Some(Self { home: home.to_path_buf(), java, jars: vec![api, edition] })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }
}

/// An export prepared from a model: the project and its layout as the tool reads them.
/// Preparing needs the model; running does not, so it can happen on another thread.
pub struct Export {
    project: String,
    layout: String,
}

impl Export {
    /// Prepares the export, measuring text with an estimate.
    pub fn new(model: &Model) -> Self {
        Self::with_measure(model, &ApproxMeasure)
    }

    pub fn with_measure(model: &Model, measure: &dyn TextMeasure) -> Self {
        let layout = serde_json::to_string(&layout::layout(model, measure))
            .expect("layout serialization cannot fail");
        Self { project: umlcraft_format::to_string(model), layout }
    }

    /// Builds the Astah project and writes it to `target`, replacing what is there.
    pub fn run(&self, astah: &Astah, target: &Path) -> Result<Report, ExportError> {
        let dir = TempDir::new()?;
        let file = |name: &str| dir.0.join(name);
        fs::write(file(TOOL_FILE), TOOL_SOURCE)?;
        fs::write(file("project.ucraft"), &self.project)?;
        fs::write(file("layout.json"), &self.layout)?;
        let classpath = std::env::join_paths(&astah.jars)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        let mut command = Command::new(&astah.java);
        command
            .current_dir(&dir.0)
            // Astah's runtime ships a class data archive that does not match it; skip it quietly.
            .arg("-Xshare:off")
            .arg("-Xmx1024m")
            .arg("-Djava.awt.headless=true")
            .arg("-cp")
            .arg(classpath)
            .arg(file(TOOL_FILE))
            .args([file("project.ucraft"), file("layout.json"), file("out.asta")])
            .arg(file("report.txt"));
        hide_console(&mut command);
        let output = command
            .output()
            .map_err(|source| ExportError::Launch { java: astah.java.clone(), source })?;

        let report = fs::read_to_string(file("report.txt")).ok();
        let mut result = Report::default();
        for line in report.iter().flat_map(|r| r.lines()) {
            let Some((kind, rest)) = line.split_once('\t') else { continue };
            match kind {
                "error" => return Err(ExportError::Astah(rest.to_owned())),
                "dropped" => {
                    let (key, count) = rest.split_once('\t').unwrap_or((rest, "1"));
                    result.dropped.insert(key.to_owned(), count.parse().unwrap_or(1));
                }
                "problem" => result.problems.push(rest.to_owned()),
                _ => {}
            }
        }
        if report.is_none() || !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            return Err(ExportError::Tool {
                status: output.status.to_string(),
                output: tail(&text),
            });
        }
        fs::copy(file("out.asta"), target)?;
        Ok(result)
    }
}

/// Exports `model` to `target` in one go.
pub fn export(astah: &Astah, model: &Model, target: &Path) -> Result<Report, ExportError> {
    Export::new(model).run(astah, target)
}

/// The last lines of the tool's output, enough to see what went wrong.
fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(8)..].join("\n")
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

/// A folder of its own for one run, removed afterwards.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> io::Result<Self> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let mut name = OsString::from("umlcraft-asta-export-");
        name.push(format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)));
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_installation_needs_the_api_and_an_edition() {
        let dir = std::env::temp_dir().join(format!("umlcraft-astah-home-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(Astah::at(&dir), None);
        fs::write(dir.join("astah-api.jar"), b"").unwrap();
        assert_eq!(Astah::at(&dir), None, "no edition jar");
        fs::write(dir.join("astah-uml.jar"), b"").unwrap();
        let astah = Astah::at(&dir).unwrap();
        assert_eq!(astah.java, PathBuf::from("java"), "no bundled runtime");
        assert_eq!(astah.jars[1], dir.join("astah-uml.jar"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_layout_has_message_rows_and_drawn_sizes() {
        let (doc, ids) = umlcraft_model::sample::order_system();
        let export = Export::new(doc.model());
        let layout: serde_json::Value = serde_json::from_str(&export.layout).unwrap();
        let sequence = &layout["diagrams"][ids.sequence_diagram.to_string()];
        assert_eq!(sequence["headBottom"], 72.0);
        assert_eq!(sequence["edges"].as_object().unwrap().len(), 3);
        let class = &layout["diagrams"][ids.class_diagram.to_string()];
        assert!(class.get("headBottom").is_none());
        assert_eq!(class["nodes"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn the_tail_keeps_the_last_lines() {
        let text = (1..=12).map(|i| format!("line {i}\n\n")).collect::<String>();
        assert_eq!(tail(&text).lines().next(), Some("line 5"));
    }
}
