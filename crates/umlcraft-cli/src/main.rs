//! `umlcraft-cli`: UmlCraft on the command line. `umlcraft-cli mcp` is the MCP server that AI
//! clients start (on stdin and stdout); `umlcraft-cli mcp setup` registers it with them;
//! `umlcraft-cli export` writes a project's diagrams as images.

mod export;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::{Value, json};
use umlcraft_mcp::backend::Auto;
use umlcraft_mcp::{Access, Headless};

const USAGE: &str = "\
usage:
  umlcraft-cli mcp [--headless] [--in project.ucraft] [--connect PORT]
                   [--automation-read-root DIR]... [--automation-write-root DIR]...
                   [--allow-path DIR]... [--allow-any-path]
      The MCP server, on stdin and stdout. Calls go to the UmlCraft app when it runs (the
      user sees every change and can undo it), else to a project of the server's own.
      Without a folder it opens, saves and exports no file.
      --headless                    never the app
      --in FILE                     start headless with this project; its folder may be
                                    read and written
      --connect PORT                the app at this port of 127.0.0.1, never headless
      --automation-read-root DIR    an existing folder whose files may be read, not written
      --automation-write-root DIR   an existing folder whose files may be read and written
      --allow-path DIR              a folder to read and write, which need not exist yet
      --allow-any-path              any folder
      A relative path in a call is taken from the first write folder (else read folder).
      DIR may also follow as --flag=DIR, which a folder starting with - needs.
  umlcraft-cli mcp setup [--dry-run] [--client claude-code|claude-desktop|cursor]
                         [--automation-read-root DIR]... [--automation-write-root DIR]...
                         [--allow-path DIR]... [--allow-any-path]
      Adds an `umlcraft` server with this program to each installed AI client. Without a
      folder the server may read and write in the Documents folder.
  umlcraft-cli export PROJECT.ucraft --diagram NAME|ID --out FILE
  umlcraft-cli export PROJECT.ucraft --all --out-dir DIR [--format svg|png]
      Writes a diagram or structogram as an image, as the MCP tool `export_image` does: the
      format by the extension of FILE (.svg, .png, .nsd for a structogram), or each one to a
      file in DIR named after it (default svg). Existing files are replaced, missing folders
      made.
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
        ["export", rest @ ..] => export::run(rest, USAGE),
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
        // The roots of the Craft apps' RFC-0015, also as `--flag=DIR`.
        let (flag, inline) = arg.split_once('=').map_or((arg, None), |(f, v)| (f, Some(v)));
        if let "--automation-read-root" | "--automation-write-root" = flag {
            let folder = root_value(flag, inline, &mut args)?;
            let folder = existing_folder(flag, &folder, !setup)?;
            match flag {
                "--automation-read-root" => options.access.read.push(folder),
                _ => options.access.write.push(folder),
            }
            continue;
        }
        match arg {
            // Absolute, as the server may run in another folder (started by an AI client) and
            // the app checks paths in its own.
            "--allow-path" => {
                let folder = PathBuf::from(value(&mut args, arg)?);
                let folder = std::path::absolute(&folder)
                    .map_err(|e| format!("--allow-path {}: {e}", folder.display()))?;
                options.access.write.push(folder);
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
    // `--in` starts headless; `--connect` never does.
    if options.port.is_some() && (options.headless || options.input.is_some()) {
        return Err(format!("--connect goes with neither --headless nor --in\n\n{USAGE}"));
    }
    Ok(options)
}

/// The folder after a root flag: `inline` (`--flag=DIR`) or the next argument, which must not
/// look like another flag, so that a forgotten folder does not swallow it.
fn root_value(
    flag: &str,
    inline: Option<&str>,
    args: &mut std::slice::Iter<&str>,
) -> Result<String, String> {
    let value = match inline {
        Some(value) => value.to_owned(),
        None => {
            let value = args.next().ok_or_else(|| format!("{flag} needs a folder"))?;
            if value.starts_with('-') {
                return Err(format!(
                    "{flag} needs a folder; write {flag}=<folder> for one starting with `-`"
                ));
            }
            (*value).to_owned()
        }
    };
    if value.is_empty() {
        return Err(format!("{flag} needs a folder"));
    }
    Ok(value)
}

/// `folder` made absolute; when the server starts (`check`), it must be an existing folder, so
/// that a mistyped root stops the server rather than leaving it without the folder.
fn existing_folder(flag: &str, folder: &str, check: bool) -> Result<PathBuf, String> {
    let absolute = std::path::absolute(folder).map_err(|e| format!("{flag} {folder}: {e}"))?;
    if check && !absolute.is_dir() {
        let why = if absolute.exists() { "is not a folder" } else { "does not exist" };
        return Err(format!("{flag} {}: {why}", absolute.display()));
    }
    Ok(absolute)
}

fn serve(args: &[&str]) -> Result<(), String> {
    let mut options = parse(args, false)?;
    // Diagrams drawn as in the app, whose settings say how.
    let display = umlcraft_mcp::host::app_display();
    let mut backend = if let Some(path) = &options.input {
        let doc = umlcraft_mcp::host::open(path)?;
        // The user named the project, so its folder may be used too.
        if let Some(folder) =
            std::path::absolute(path).ok().and_then(|p| p.parent().map(Path::to_path_buf))
        {
            options.access.write.push(folder);
        }
        Auto::headless(options.access, Headless::with(doc, Some(path.clone())).drawn_as(display))
    } else if options.headless {
        Auto::headless(options.access, Headless::new().drawn_as(display))
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
    let access = &mut options.access;
    if access.read.is_empty()
        && access.write.is_empty()
        && !access.any
        && let Some(documents) = documents_dir()
    {
        access.write.push(documents);
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let server_args = server_args(&options.access);
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
             hand: command `umlcraft-cli`, arguments `mcp --automation-write-root <folder>`."
                .into(),
        );
    }
    if !options.dry_run {
        println!("Restart the AI client to use UmlCraft.");
    }
    Ok(())
}

/// The server's arguments for an AI client's configuration: each folder as a root of the
/// Craft apps' RFC-0015 (`--allow-path` folders as write roots).
fn server_args(access: &Access) -> Vec<String> {
    let mut args = vec!["mcp".to_owned()];
    for (flag, folders) in
        [("--automation-read-root", &access.read), ("--automation-write-root", &access.write)]
    {
        for folder in folders {
            args.push(flag.into());
            args.push(folder.display().to_string());
        }
    }
    if access.any {
        args.push("--allow-any-path".into());
    }
    args
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
    // An older entry makes `add` fail. It is removed only then, when it is there, so that a
    // working entry stays when `add` fails for another reason.
    let exists = || {
        run(&["mcp", "get", "umlcraft"]).is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
    };
    if !output.status.success() && exists() {
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
/// be read or is not a JSON object is left as it is. A symbolic link is followed, so that the
/// file it points to changes and the link stays.
fn add_to_json(file: &Path, path: &[&str], entry: Value, dry_run: bool) -> Result<bool, String> {
    let is_link = std::fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink());
    let target = if is_link {
        std::fs::canonicalize(file).map_err(|e| format!("{}: {e}", file.display()))?
    } else {
        file.to_owned()
    };
    let file = target.as_path();
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
/// as `{}`, a missing key on the way as `{}` too. Anything else on the way that is not an
/// object is not understood, and the file is left as it is.
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
        slot = child.as_object_mut().ok_or_else(|| keep(format!("`{key}` is not an object")))?;
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
        // Nor is a file whose servers are not an object.
        for servers in ["null", "\"x\"", "[]"] {
            std::fs::write(&file, format!(r#"{{"mcpServers": {servers}}}"#)).unwrap();
            assert!(merged_json(&file, &["mcpServers", "umlcraft"], json!({})).is_err());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn conflicting_options_are_refused() {
        for args in
            [&["--headless", "--connect", "4000"][..], &["--connect", "4000", "--in", "x.ucraft"]]
        {
            assert!(parse(args, false).is_err(), "{args:?}");
        }
        assert!(parse(&["--headless", "--in", "x.ucraft"], false).is_ok());
        assert!(parse(&["--connect", "127.0.0.1:4000"], false).is_ok_and(|o| o.port == Some(4000)));
    }

    /// A configuration kept elsewhere and linked into place stays linked.
    #[cfg(unix)]
    #[test]
    fn a_linked_configuration_stays_a_link() {
        let dir = scratch("link");
        let real = dir.join("real.json");
        std::fs::write(&real, "{}").unwrap();
        let link = dir.join("config.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        add_to_json(&link, &["mcpServers", "umlcraft"], json!({"command": "u"}), false).unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("umlcraft"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn allowed_folders_are_made_absolute() {
        let options = parse(&["--allow-path", "diagrams"], true).unwrap();
        assert!(options.access.write[0].is_absolute(), "{:?}", options.access.write);
        assert!(options.access.write[0].ends_with("diagrams"));
        assert!(options.access.read.is_empty());
    }

    #[test]
    fn roots_read_and_write_apart() {
        let dir = scratch("roots");
        let (input, output) = (dir.join("in"), dir.join("-out"));
        std::fs::create_dir_all(&input).unwrap();
        std::fs::create_dir_all(&output).unwrap();
        let (input_text, output_text) = (input.display().to_string(), output.display().to_string());
        let write_flag = format!("--automation-write-root={output_text}");
        let options =
            parse(&["--automation-read-root", &input_text, &write_flag, "--headless"], false)
                .unwrap();
        assert_eq!(options.access.read, vec![input.clone()]);
        assert_eq!(options.access.write, vec![output.clone()]);
        // Several of each.
        let twice = ["--automation-read-root", &input_text, "--automation-read-root", &input_text];
        assert_eq!(parse(&twice, false).unwrap().access.read.len(), 2);
        // The server's command for a client keeps them apart.
        assert_eq!(
            server_args(&options.access),
            ["mcp", "--automation-read-root", &input_text, "--automation-write-root", &output_text]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_root_needs_an_existing_folder() {
        let dir = scratch("missing-root");
        let missing = dir.join("missing").display().to_string();
        let file = dir.join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let file = file.display().to_string();
        for flag in ["--automation-read-root", "--automation-write-root"] {
            for args in [
                vec![flag],
                vec![flag, ""],
                vec![flag, "--headless"],
                vec![flag, "-folder"],
                vec![flag, &missing],
                vec![flag, &file],
            ] {
                assert!(parse(&args, false).is_err(), "{args:?}");
            }
            assert!(parse(&[&format!("{flag}=")], false).is_err());
            // `mcp setup` may name a folder made later; the server checks it when it starts.
            assert!(parse(&[flag, &missing], true).is_ok());
        }
        // `--allow-path` stays as it was, for the clients set up with it.
        assert!(parse(&["--allow-path", &missing], false).is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
