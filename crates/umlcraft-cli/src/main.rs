//! `umlcraft-cli`: UmlCraft on the command line. `umlcraft-cli mcp` is the MCP server that AI
//! clients start (on stdin and stdout); `umlcraft-cli mcp setup` registers it with them.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::{Value, json};
use umlcraft_mcp::backend::Auto;
use umlcraft_mcp::{Access, Headless};

const USAGE: &str = "\
usage:
  umlcraft-cli mcp [--headless] [--in project.ucraft] [--connect PORT]
                   [--allow-path DIR]... [--allow-any-path]
      The MCP server, on stdin and stdout. Calls go to the UmlCraft app when it runs (the
      user sees every change and can undo it), else to a project of the server's own.
      --headless        never the app
      --in FILE         start headless with this project
      --connect PORT    the app at this port of 127.0.0.1, never headless
      --allow-path DIR  a folder in which projects and images may be opened and saved
      --allow-any-path  any folder
  umlcraft-cli mcp setup [--dry-run] [--client claude-code|claude-desktop|cursor]
                         [--allow-path DIR]... [--allow-any-path]
      Adds an `umlcraft` server with this program to each installed AI client. Without
      --allow-path the server may use the Documents folder.
  umlcraft-cli --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--version" | "-V"] => {
            println!("umlcraft-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["mcp", "setup" | "install", rest @ ..] => setup(rest),
        ["mcp", rest @ ..] => serve(rest),
        [] | ["--help" | "-h" | "help"] => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unknown command\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("umlcraft-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Options shared by `mcp` and `mcp setup`.
#[derive(Default)]
struct Options {
    access: Access,
    headless: bool,
    input: Option<PathBuf>,
    port: Option<u16>,
    dry_run: bool,
    client: Option<String>,
}

fn parse(args: &[&str], setup: bool) -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = args.iter();
    let value = |args: &mut std::slice::Iter<&str>, flag: &str| {
        args.next().map(|v| v.to_string()).ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(&arg) = args.next() {
        match arg {
            // Absolute, as the server may run in another folder (started by an AI client) and
            // the app checks paths in its own.
            "--allow-path" => {
                let folder = PathBuf::from(value(&mut args, arg)?);
                let folder = std::path::absolute(&folder)
                    .map_err(|e| format!("--allow-path {}: {e}", folder.display()))?;
                options.access.folders.push(folder);
            }
            "--allow-any-path" => options.access.any = true,
            "--headless" if !setup => options.headless = true,
            "--in" if !setup => options.input = Some(PathBuf::from(value(&mut args, arg)?)),
            "--connect" if !setup => {
                let text = value(&mut args, arg)?;
                let port = text.rsplit(':').next().unwrap_or(&text);
                options.port = Some(port.parse().map_err(|_| format!("bad port `{text}`"))?);
            }
            "--dry-run" if setup => options.dry_run = true,
            "--client" if setup => options.client = Some(value(&mut args, arg)?),
            _ => return Err(format!("unknown option `{arg}`\n\n{USAGE}")),
        }
    }
    Ok(options)
}

fn serve(args: &[&str]) -> Result<(), String> {
    let mut options = parse(args, false)?;
    let mut backend = if let Some(path) = &options.input {
        let doc = umlcraft_mcp::host::open(path)?;
        // The user named the project, so its folder may be used too.
        if let Some(folder) =
            std::path::absolute(path).ok().and_then(|p| p.parent().map(Path::to_path_buf))
        {
            options.access.folders.push(folder);
        }
        Auto::headless(options.access, Headless::with(doc, Some(path.clone())))
    } else if options.headless {
        Auto::headless(options.access, Headless::new())
    } else if let Some(port) = options.port {
        Auto::connect(options.access, port)
    } else {
        Auto::new(options.access)
    };
    let (stdin, stdout) = (std::io::stdin(), std::io::stdout());
    umlcraft_mcp::protocol::serve(&mut backend, stdin.lock(), stdout.lock())
        .map_err(|e| e.to_string())
}

// ----- Setup ---------------------------------------------------------------------------------

fn setup(args: &[&str]) -> Result<(), String> {
    let mut options = parse(args, true)?;
    if options.access.folders.is_empty()
        && !options.access.any
        && let Some(documents) = documents_dir()
    {
        options.access.folders.push(documents);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut server_args = vec!["mcp".to_owned()];
    for folder in &options.access.folders {
        server_args.push("--allow-path".into());
        server_args.push(folder.display().to_string());
    }
    if options.access.any {
        server_args.push("--allow-any-path".into());
    }
    let wanted = |client: &str| options.client.as_deref().is_none_or(|c| c == client);
    let mut done = 0;
    if wanted("claude-code") {
        done += usize::from(claude_code(&exe, &server_args, options.dry_run)?);
    }
    if wanted("claude-desktop")
        && let Some(file) = claude_desktop_config()
    {
        let entry = json!({ "command": exe, "args": server_args });
        done +=
            usize::from(add_to_json(&file, &["mcpServers", "umlcraft"], entry, options.dry_run)?);
    }
    if wanted("cursor")
        && let Some(home) = home_dir()
        && home.join(".cursor").is_dir()
    {
        let entry = json!({ "command": exe, "args": server_args });
        let file = home.join(".cursor").join("mcp.json");
        done +=
            usize::from(add_to_json(&file, &["mcpServers", "umlcraft"], entry, options.dry_run)?);
    }
    if done == 0 {
        return Err(
            "found no AI client to set up (Claude Code, Claude Desktop, Cursor). Register it by \
             hand: command `umlcraft-cli`, arguments `mcp --allow-path <folder>`."
                .into(),
        );
    }
    if !options.dry_run {
        println!("Restart the AI client to use UmlCraft.");
    }
    Ok(())
}

/// Registers the server with Claude Code's `claude mcp add`, if `claude` is installed.
fn claude_code(exe: &Path, server_args: &[String], dry_run: bool) -> Result<bool, String> {
    let Some(claude) = ["claude", "claude.cmd"]
        .into_iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok_and(|o| o.status.success()))
    else {
        return Ok(false);
    };
    let mut add = vec!["mcp", "add", "--scope", "user", "umlcraft", "--"];
    let exe = exe.display().to_string();
    add.push(&exe);
    add.extend(server_args.iter().map(String::as_str));
    if dry_run {
        println!("Claude Code: {claude} {}", add.join(" "));
        return Ok(true);
    }
    let run = |args: &[&str]| Command::new(claude).args(args).output().map_err(|e| e.to_string());
    let mut output = run(&add)?;
    // An older entry makes `add` fail. It is removed only then, so that a working entry stays
    // when nothing replaces it.
    if !output.status.success() {
        let _ = run(&["mcp", "remove", "--scope", "user", "umlcraft"]);
        output = run(&add)?;
    }
    if !output.status.success() {
        return Err(format!(
            "`claude mcp add` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    println!("Claude Code: added the server `umlcraft`.");
    Ok(true)
}

/// Claude Desktop's configuration file, if Claude Desktop is installed.
fn claude_desktop_config() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        var("APPDATA")?.join("Claude")
    } else if cfg!(target_os = "macos") {
        home_dir()?.join("Library/Application Support/Claude")
    } else {
        var("XDG_CONFIG_HOME").or_else(|| home_dir().map(|h| h.join(".config")))?.join("Claude")
    };
    dir.is_dir().then(|| dir.join("claude_desktop_config.json"))
}

/// Sets `path` in the JSON file `file` to `entry`, keeping everything else. A file that cannot
/// be read or is not a JSON object is left as it is.
fn add_to_json(file: &Path, path: &[&str], entry: Value, dry_run: bool) -> Result<bool, String> {
    let text = merged_json(file, path, entry)?;
    if dry_run {
        println!("{}:\n{text}", file.display());
        return Ok(true);
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    // Written beside it and renamed over it, so that the client's configuration is never left
    // half written.
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(".umlcraft-tmp");
    let tmp = PathBuf::from(tmp);
    let written = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, file));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{}: {e}", file.display()));
    }
    println!("{}: added the server `umlcraft`.", file.display());
    Ok(true)
}

/// The text of the JSON file `file` with `path` set to `entry`; a missing or empty file counts
/// as `{}`.
fn merged_json(file: &Path, path: &[&str], entry: Value) -> Result<String, String> {
    let keep = |e: String| format!("{}: {e}; left as it is", file.display());
    let mut root: Value = match std::fs::read(file) {
        Ok(bytes) if bytes.iter().all(u8::is_ascii_whitespace) => json!({}),
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|e| keep(format!("not valid JSON ({e})")))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(keep(format!("cannot be read ({e})"))),
    };
    let Some((last, parents)) = path.split_last() else { return Err("no key".into()) };
    let mut slot = root.as_object_mut().ok_or_else(|| keep("not a JSON object".into()))?;
    for key in parents {
        let child = slot.entry(*key).or_insert_with(|| json!({}));
        if !child.is_object() {
            *child = json!({});
        }
        slot = child.as_object_mut().expect("made an object above");
    }
    slot.insert((*last).to_owned(), entry);
    Ok(serde_json::to_string_pretty(&root).map_err(|e| e.to_string())? + "\n")
}

fn home_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) { var("USERPROFILE") } else { var("HOME") }
}

fn documents_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join("Documents")).filter(|dir| dir.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umlcraft-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_entry_joins_what_the_configuration_has() {
        let dir = scratch("merge");
        let file = dir.join("config.json");
        std::fs::write(&file, r#"{"mcpServers": {"other": {"command": "x"}}, "theme": 1}"#)
            .unwrap();
        let text =
            merged_json(&file, &["mcpServers", "umlcraft"], json!({"command": "u"})).unwrap();
        let merged: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(merged["mcpServers"]["other"]["command"], "x");
        assert_eq!(merged["mcpServers"]["umlcraft"]["command"], "u");
        assert_eq!(merged["theme"], 1);
        // A missing file starts empty.
        let new = dir.join("new.json");
        assert!(merged_json(&new, &["a", "b"], json!(1)).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_configuration_that_is_no_object_or_unreadable_is_left_alone() {
        let dir = scratch("keep");
        let file = dir.join("config.json");
        std::fs::write(&file, "[1, 2]").unwrap();
        assert!(merged_json(&file, &["mcpServers", "umlcraft"], json!({})).is_err());
        // A folder in place of the file cannot be read: no fresh `{}` overwrites it.
        assert!(merged_json(&dir, &["mcpServers", "umlcraft"], json!({})).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "[1, 2]");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn allowed_folders_are_made_absolute() {
        let options = parse(&["--allow-path", "diagrams"], true).unwrap();
        assert!(options.access.folders[0].is_absolute(), "{:?}", options.access.folders);
        assert!(options.access.folders[0].ends_with("diagrams"));
    }
}
