//! Export to Astah project files (`.asta`) through an installed Astah.
//!
//! An `.asta` file is Astah's Java-serialized object graph, which is fragile to write and changes
//! between Astah versions. So instead of writing it, the export drives Astah's official API: the
//! Java tool in `tools/asta-export` reads a `.ucraft` file plus a layout sidecar and builds the
//! project with the API's model and diagram editors. It runs in source-file mode on the JRE that
//! comes with Astah, so nothing has to be compiled beforehand.

mod layout;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;
use umlcraft_model::{Document, ElementKind, Id, Model};
use umlcraft_notation::scene::ApproxMeasure;

/// The Java tool, written to a temporary folder for each run.
const TOOL_SOURCE: &str = include_str!("../../../tools/asta-export/AstaExport.java");
const TOOL_FILE: &str = "AstaExport.java";

/// Main jar of each Astah edition that has the API.
const EDITION_JARS: &[&str] = &["astah-pro.jar", "astah-uml.jar", "astah-sysml.jar"];
/// Install folder names under "Program Files".
const INSTALL_DIRS: &[&str] = &["astah-professional", "astah-uml", "astah-sysml"];
/// Where Astah's installers put the jars on macOS (inside the app bundle) and Linux. Each is
/// only a candidate: `Astah::at` checks for the jars.
const UNIX_HOMES: &[&str] = &[
    "/Applications/astah professional/astah professional.app/Contents/Java",
    "/Applications/astah UML/astah UML.app/Contents/Java",
    "/Applications/astah SysML/astah SysML.app/Contents/Java",
    "/usr/lib/astah_professional",
    "/usr/lib/astah_uml",
    "/usr/lib/astah_sysml",
    "/opt/astah_professional",
];
/// How long the export tool may run. Astah can hang on a modal dialog it cannot show, and the
/// export then has to end rather than block every further export.
const TIMEOUT: Duration = Duration::from_secs(10 * 60);

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
    /// Astah's license check refused saving; Astah then tries to open its license page.
    #[error("Astah refused to save the project: its license check failed")]
    License,
    #[error("the export tool did not finish within {minutes} minutes")]
    TimedOut { minutes: u64 },
    #[error("the export was cancelled")]
    Cancelled,
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
    /// The installation named by `ASTAH_HOME`, else the first one found in the usual places
    /// (Windows' program folders, the macOS and Linux install folders).
    pub fn find() -> Option<Self> {
        if let Some(found) = std::env::var_os("ASTAH_HOME").and_then(|h| Self::at(Path::new(&h))) {
            return Some(found);
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
            .chain(UNIX_HOMES.iter().filter(|_| !cfg!(windows)).map(PathBuf::from))
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
        // Without a bundled runtime, a Java 21 or newer on the PATH will do; if there is none,
        // `ExportError::Launch` names `java` as what could not be started.
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
    /// Structograms left out: Astah has none.
    structograms: usize,
}

impl Export {
    /// Prepares the export, measuring text with an estimate.
    pub fn new(model: &Model) -> Self {
        let (model, structograms) = without_structograms(model);
        let layout = serde_json::to_string(&layout::layout(&model, &ApproxMeasure))
            .expect("layout serialization cannot fail");
        Self { project: umlcraft_format::to_string(&model), layout, structograms }
    }

    /// Builds the Astah project and writes it to `target`, replacing what is there.
    pub fn run(&self, astah: &Astah, target: &Path) -> Result<Report, ExportError> {
        self.run_cancellable(astah, target, &AtomicBool::new(false))
    }

    /// [`run`](Self::run), stopped with [`ExportError::Cancelled`] as soon as `cancel` is set:
    /// the tool is killed and the target left as it was.
    pub fn run_cancellable(
        &self,
        astah: &Astah,
        target: &Path,
        cancel: &AtomicBool,
    ) -> Result<Report, ExportError> {
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
            // Bare names: the tool runs in the folder, and Windows' `java` decodes arguments
            // in the ANSI code page, which a user's temp path may not fit.
            .args([TOOL_FILE, "project.ucraft", "layout.json", "out.asta", "report.txt"]);
        hide_console(&mut command);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = command
            .spawn()
            .map_err(|source| ExportError::Launch { java: astah.java.clone(), source })?;
        let output = wait_with_deadline(child, TIMEOUT, cancel)?;

        let report = fs::read_to_string(file("report.txt")).ok();
        let mut result = match &report {
            Some(text) => parse_report(text)?,
            None => Report::default(),
        };
        if self.structograms > 0 {
            result.dropped.insert("structogram".to_owned(), self.structograms);
        }
        if report.is_none() || !output.status.success() {
            let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stdout));
            // Saving checks the license and ends the program without a report when it fails.
            if report.is_none()
                && text.lines().any(|l| l.contains(".license.") && l.ends_with(" - NO"))
            {
                return Err(ExportError::License);
            }
            return Err(ExportError::Tool {
                status: output.status.to_string(),
                output: tail(&text),
            });
        }
        // Next to the target first, then over it: a failed copy leaves the old file whole. The
        // partial file has a name of its own and is made new, so that no file of the user's
        // (`model.asta.tmp`) is overwritten.
        let mut partial = target.as_os_str().to_owned();
        partial.push(format!(".{}.tmp", Id::generate()));
        let partial = PathBuf::from(partial);
        let copied = fs::File::create_new(&partial)
            .and_then(|mut to| io::copy(&mut fs::File::open(file("out.asta"))?, &mut to))
            .and_then(|_| fs::rename(&partial, target));
        if let Err(e) = copied {
            fs::remove_file(&partial).ok();
            return Err(e.into());
        }
        Ok(result)
    }
}

/// `model` without its structograms, and how many there were.
fn without_structograms(model: &Model) -> (Model, usize) {
    let ids: Vec<Id> = model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Structogram(_)))
        .map(|e| e.id())
        .collect();
    if ids.is_empty() {
        return (model.clone(), 0);
    }
    let mut doc = Document::new(model.clone());
    doc.edit("", |tx| tx.delete_many(&ids)).expect("structograms can always be deleted");
    (doc.model().clone(), ids.len())
}

/// Exports `model` to `target` in one go.
pub fn export(astah: &Astah, model: &Model, target: &Path) -> Result<Report, ExportError> {
    Export::new(model).run(astah, target)
}

/// The tool's report: one line per finding, `kind<TAB>details`.
fn parse_report(text: &str) -> Result<Report, ExportError> {
    let mut result = Report::default();
    for line in text.lines() {
        let Some((kind, rest)) = line.split_once('\t') else { continue };
        match kind {
            "error" => return Err(ExportError::Astah(rest.to_owned())),
            "dropped" => {
                let (key, count) = rest.split_once('\t').unwrap_or((rest, "1"));
                *result.dropped.entry(key.to_owned()).or_default() += count.parse().unwrap_or(1);
            }
            "problem" => result.problems.push(rest.to_owned()),
            _ => {}
        }
    }
    Ok(result)
}

struct Output {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

/// Waits for `child`, reading its output on threads so that a full pipe cannot stall it, and
/// kills it once `timeout` has passed.
fn wait_with_deadline(
    mut child: Child,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<Output, ExportError> {
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if cancel.load(Ordering::Relaxed) {
            child.kill().ok();
            child.wait().ok();
            return Err(ExportError::Cancelled);
        }
        if start.elapsed() >= timeout {
            child.kill().ok();
            child.wait().ok();
            return Err(ExportError::TimedOut { minutes: timeout.as_secs() / 60 });
        }
        thread::sleep(Duration::from_millis(100));
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Ok(Output { status, stdout, stderr })
}

/// Reads all of `pipe` on a thread of its own.
fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_end(&mut bytes).ok();
        }
        bytes
    })
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

/// A folder of its own for one run, removed afterwards. Its name is random and it must not
/// exist yet, so that nobody else can prepare it with a tool of their own for Astah's Java to
/// run; on Unix only the user may enter it.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> io::Result<Self> {
        let mut last = None;
        for _ in 0..8 {
            let path = std::env::temp_dir()
                .join(format!("umlcraft-asta-export-{}", umlcraft_model::Id::generate()));
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = fs::DirBuilder::new();
                builder.mode(0o700);
                builder
            };
            #[cfg(not(unix))]
            let builder = fs::DirBuilder::new();
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => last = Some(e),
                Err(e) => return Err(e),
            }
        }
        Err(last.expect("tried at least once"))
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
    fn the_layout_has_drawn_sizes_and_the_project_the_sequence_geometry() {
        let (doc, ids) = umlcraft_model::sample::order_system();
        let export = Export::new(doc.model());
        let layout: serde_json::Value = serde_json::from_str(&export.layout).unwrap();
        let class = &layout["diagrams"][ids.class_diagram.to_string()];
        assert_eq!(class["nodes"].as_object().unwrap().len(), 3);
        // The tool reads lifelines, bars and messages from the project itself.
        assert!(export.project.contains("\"shape\": \"lifeline\""), "{}", export.project);
        assert!(export.project.contains("\"shape\": \"activation\""));
        assert!(export.project.contains("\"shape\": \"message\""));
    }

    #[test]
    fn structograms_stay_out_of_the_project_and_are_counted() {
        let (mut doc, ids) = umlcraft_model::sample::order_system();
        let made = umlcraft_nsd::Provenance { author: "a", date: "2026-10-30", origin: "t" };
        let sg =
            umlcraft_nsd::Diagram::new(umlcraft_nsd::RootKind::Program, &["f()".into()], &made);
        let data = umlcraft_model::ElementData::new(
            "",
            ElementKind::Structogram(umlcraft_model::StructogramProps::new(&sg)),
        );
        doc.edit("structogram", |tx| tx.create(ids.order, data)).unwrap();
        let export = Export::new(doc.model());
        assert!(!export.project.contains("structogram"), "{}", export.project);
        assert_eq!(export.structograms, 1);
    }

    #[test]
    fn reports_are_read_line_by_line() {
        let report =
            parse_report("dropped\tquery\t2\nproblem\tno owner\ndropped\tquery\nnoise\n").unwrap();
        assert_eq!(report.dropped.get("query"), Some(&3));
        assert_eq!(report.problems, ["no owner"]);
        assert!(parse_report("").unwrap().is_complete());
        let error = parse_report("dropped\tx\nerror\tcannot save\n").unwrap_err();
        assert!(matches!(error, ExportError::Astah(text) if text == "cannot save"));
    }

    #[test]
    fn temp_folders_are_new_and_random() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        assert_ne!(a.0, b.0);
        assert!(a.0.is_dir());
        let path = a.0.clone();
        drop(a);
        assert!(!path.exists());
    }

    #[test]
    fn a_hanging_tool_is_ended() {
        let mut command = if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-Command", "Start-Sleep 30"]);
            c
        } else {
            let mut c = Command::new("sleep");
            c.arg("30");
            c
        };
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let Ok(child) = command.spawn() else { return };
        let start = Instant::now();
        let result = wait_with_deadline(child, Duration::from_millis(300), &AtomicBool::new(false));
        assert!(matches!(result, Err(ExportError::TimedOut { .. })));
        assert!(start.elapsed() < Duration::from_secs(10));
        // Cancelling ends it at once, long before the deadline.
        let Ok(child) = command.spawn() else { return };
        let start = Instant::now();
        let result = wait_with_deadline(child, Duration::from_secs(60), &AtomicBool::new(true));
        assert!(matches!(result, Err(ExportError::Cancelled)));
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn the_tail_keeps_the_last_lines() {
        let text = (1..=12).map(|i| format!("line {i}\n\n")).collect::<String>();
        assert_eq!(tail(&text).lines().next(), Some("line 5"));
    }
}
