//! `umlcraft-cli export`: images of a project's diagrams for scripts, without the MCP
//! protocol. It runs the MCP tool `export_image` on a project of its own, so the files are the
//! ones the MCP server writes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use umlcraft_mcp::{Access, Content, Headless, Reply};

/// What `export` was asked for.
#[derive(Debug, PartialEq)]
enum Wanted {
    /// One diagram (id or name) to one file, the format by its extension.
    One { diagram: String, out: PathBuf },
    /// Every diagram and structogram, each to a file in `dir` named after it.
    All { dir: PathBuf, format: String },
}

fn parse(args: &[&str]) -> Result<(PathBuf, Wanted), String> {
    let mut project = None;
    let (mut diagram, mut out, mut dir, mut format, mut all) = (None, None, None, None, false);
    let mut args = args.iter();
    let value = |args: &mut std::slice::Iter<&str>, flag: &str| {
        args.next().map(|v| v.to_string()).ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(&arg) = args.next() {
        match arg {
            "--diagram" => once(&mut diagram, value(&mut args, arg)?, arg)?,
            "--out" => once(&mut out, PathBuf::from(value(&mut args, arg)?), arg)?,
            "--all" => all = true,
            "--out-dir" => once(&mut dir, PathBuf::from(value(&mut args, arg)?), arg)?,
            "--format" => once(&mut format, value(&mut args, arg)?, arg)?,
            _ if arg.starts_with('-') => return Err(format!("unknown option `{arg}`")),
            _ if project.is_none() => project = Some(PathBuf::from(arg)),
            _ => return Err(format!("one project only, not also `{arg}`")),
        }
    }
    let project = project.ok_or("export needs a project (.ucraft)")?;
    let wanted = match (diagram, out, all, dir) {
        (Some(diagram), Some(out), false, None) if format.is_none() => Wanted::One { diagram, out },
        (None, None, true, Some(dir)) => {
            let format = format.unwrap_or_else(|| "svg".into());
            if !matches!(format.as_str(), "svg" | "png") {
                return Err(format!("--format is svg or png, not `{format}`"));
            }
            Wanted::All { dir, format }
        }
        (Some(_), Some(_), false, None) => {
            return Err("--format goes with --all; --out names the format by its extension".into());
        }
        _ => {
            return Err(
                "export takes --diagram with --out, or --all with --out-dir [--format]".into()
            );
        }
    };
    Ok((project, wanted))
}

/// Sets `slot` to `value`, unless `flag` gave it a value before.
fn once<T>(slot: &mut Option<T>, value: T, flag: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("{flag} is given twice"));
    }
    *slot = Some(value);
    Ok(())
}

/// Runs `umlcraft-cli export` with the arguments after `export`; `usage` follows a mistake in
/// them.
pub fn run(args: &[&str], usage: &str) -> Result<(), String> {
    if args.iter().any(|a| matches!(*a, "--help" | "-h")) {
        println!("{usage}");
        return Ok(());
    }
    let (project, wanted) = parse(args).map_err(|e| format!("{e}\n\n{usage}"))?;
    let doc = umlcraft_mcp::host::open(&project)?;
    // Drawn as in the app, whose settings say how, as by the MCP server.
    let display = umlcraft_mcp::host::app_display();
    let mut host = Headless::with(doc, Some(project)).drawn_as(display);
    // The user names the files on the command line.
    let access = Access { folders: Vec::new(), any: true };
    match wanted {
        Wanted::One { diagram, out } => {
            let written = export(&mut host, &access, &diagram, &out)?;
            println!("{}", written.display());
            Ok(())
        }
        Wanted::All { dir, format } => {
            let document = reply(umlcraft_mcp::tools::call_guarded(
                &mut host,
                &access,
                "get_document",
                &json!({}),
            ))?;
            let mut failed = 0;
            for (id, file) in file_names(&document, &format) {
                match export(&mut host, &access, &id, &dir.join(file)) {
                    Ok(written) => println!("{}", written.display()),
                    Err(e) => {
                        eprintln!("umlcraft-cli: {e}");
                        failed += 1;
                    }
                }
            }
            match failed {
                0 => Ok(()),
                1 => Err("one diagram was not written".into()),
                n => Err(format!("{n} diagrams were not written")),
            }
        }
    }
}

/// Writes `diagram` to `out` with the MCP tool `export_image`, replacing a file that is there;
/// a panic in it is an error, as in the MCP server, so `--all` goes on with the next one.
fn export(
    host: &mut Headless,
    access: &Access,
    diagram: &str,
    out: &Path,
) -> Result<PathBuf, String> {
    let args = json!({ "diagram": diagram, "path": out, "overwrite": true });
    let written = reply(umlcraft_mcp::tools::call_guarded(host, access, "export_image", &args))?;
    Ok(PathBuf::from(written["written"].as_str().unwrap_or_default()))
}

/// The JSON of a tool's reply, or its error.
fn reply(reply: Reply) -> Result<Value, String> {
    let text = match reply.content.first() {
        Some(Content::Text(text)) => text.clone(),
        _ => String::new(),
    };
    if reply.is_error {
        return Err(text);
    }
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

/// The id of each diagram and structogram in `get_document`'s reply, with the file it goes to:
/// its name (a structogram's title) with what file names cannot hold replaced by `_`, made
/// unique as file systems that ignore case see it.
fn file_names(document: &Value, format: &str) -> Vec<(String, String)> {
    let mut taken = HashSet::new();
    let mut files = Vec::new();
    for (key, name, fallback) in
        [("diagrams", "name", "diagram"), ("structograms", "title", "structogram")]
    {
        for d in document[key].as_array().into_iter().flatten() {
            let stem = file_stem(d[name].as_str().unwrap_or_default(), fallback);
            let file = (1..)
                .map(|n| match n {
                    1 => format!("{stem}.{format}"),
                    n => format!("{stem} ({n}).{format}"),
                })
                .find(|file| taken.insert(file.to_lowercase()))
                .unwrap_or_default();
            files.push((d["id"].as_str().unwrap_or_default().to_owned(), file));
        }
    }
    files
}

/// `name` as a file name on any system: no separators, no characters Windows refuses, no
/// trailing dots or spaces, no device name of Windows (`CON`, `COM1`, …), at most 200 bytes, so
/// that the number and extension still fit; `fallback` when nothing is left.
fn file_stem(name: &str, fallback: &str) -> String {
    let mut stem: String = name
        .chars()
        .map(|c| if c.is_control() || r#"/\:*?"<>|"#.contains(c) { '_' } else { c })
        .collect();
    let end = (0..=stem.len().min(200)).rev().find(|&i| stem.is_char_boundary(i)).unwrap_or(0);
    stem.truncate(end);
    let stem = stem.trim().trim_end_matches(['.', ' ']);
    if stem.is_empty() {
        return fallback.to_owned();
    }
    let device = stem.split('.').next().unwrap_or_default().trim_end().to_uppercase();
    let numbered = |prefix: &str| {
        device
            .strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit())
    };
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || numbered("COM")
        || numbered("LPT")
    {
        format!("_{stem}")
    } else {
        stem.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_mcp::Host;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("umlcraft-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn call(host: &mut Headless, name: &str, args: Value) -> Value {
        reply(umlcraft_mcp::tools::call(
            host,
            &Access { folders: Vec::new(), any: true },
            name,
            &args,
        ))
        .unwrap()
    }

    #[test]
    fn options_are_checked() {
        let one = parse(&["p.ucraft", "--diagram", "D", "--out", "d.svg"]).unwrap();
        assert_eq!(one.1, Wanted::One { diagram: "D".into(), out: "d.svg".into() });
        let all = parse(&["--all", "--out-dir", "svg", "p.ucraft"]).unwrap();
        assert_eq!(all.1, Wanted::All { dir: "svg".into(), format: "svg".into() });
        for wrong in [
            &[][..],
            &["p.ucraft"],
            &["p.ucraft", "--diagram", "D"],
            &["p.ucraft", "--diagram", "D", "--out", "d.svg", "--all"],
            &["p.ucraft", "--diagram", "D", "--out", "d.svg", "--format", "png"],
            &["p.ucraft", "--all", "--out-dir", "x", "--format", "pdf"],
            &["p.ucraft", "--all", "--out", "x.svg"],
            &["p.ucraft", "q.ucraft", "--all", "--out-dir", "x"],
            &["p.ucraft", "--diagram"],
            &["p.ucraft", "--scale", "2"],
            &["p.ucraft", "--diagram", "A", "--out", "a.svg", "--diagram", "B"],
        ] {
            assert!(parse(wrong).is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn file_names_hold_on_every_system_and_differ() {
        let document = json!({
            "diagrams": [
                { "id": "1", "name": "Shop" },
                { "id": "2", "name": "shop" },
                { "id": "3", "name": "a/b: c?" },
                { "id": "4", "name": "  " },
                { "id": "5", "name": "end." },
                { "id": "8", "name": "con" },
                { "id": "9", "name": "Com1.x" },
                { "id": "10", "name": "Computer" },
                { "id": "11", "name": &"ä".repeat(150) },
            ],
            "structograms": [{ "id": "6", "title": "total(n)" }, { "id": "7", "title": "" }],
        });
        let names: Vec<String> = file_names(&document, "png").into_iter().map(|(_, f)| f).collect();
        assert_eq!(
            names,
            [
                "Shop.png",
                "shop (2).png",
                "a_b_ c_.png",
                "diagram.png",
                "end.png",
                "_con.png",
                "_Com1.x.png",
                "Computer.png",
                format!("{}.png", "ä".repeat(100)).as_str(),
                "total(n).png",
                "structogram.png"
            ]
        );
    }

    /// The files are the ones `export_image` writes, for one diagram by name and for all.
    #[test]
    fn exports_are_those_of_the_mcp_tool() {
        let dir = scratch("export");
        let project = dir.join("p.ucraft");
        let mut host = Headless::new();
        let shop = call(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
        let order = json!({ "diagram": shop["diagram"], "tool": "class", "name": "Order" });
        call(&mut host, "create_element", order);
        let tree =
            json!({ "title": "total(n)", "body": [{ "kind": "instruction", "text": "s := 0" }] });
        call(&mut host, "create_structogram", json!({ "structogram": tree }));
        call(&mut host, "save_file", json!({ "path": project }));

        let mut by_tool = Headless::with(umlcraft_mcp::host::open(&project).unwrap(), None)
            .drawn_as(umlcraft_mcp::host::app_display());
        let expected = dir.join("expected.svg");
        call(&mut by_tool, "export_image", json!({ "diagram": shop["diagram"], "path": expected }));
        assert!(!by_tool.document().is_dirty());

        let out = dir.join("new").join("shop.svg");
        let p = project.to_str().unwrap();
        run(&[p, "--diagram", "Shop", "--out", out.to_str().unwrap()], "").unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), std::fs::read(&expected).unwrap());
        // Again: the file is replaced.
        run(&[p, "--diagram", "Shop", "--out", out.to_str().unwrap()], "").unwrap();

        let all = dir.join("all");
        run(&[p, "--all", "--out-dir", all.to_str().unwrap(), "--format", "png"], "").unwrap();
        let mut files: Vec<String> = std::fs::read_dir(&all)
            .unwrap()
            .map(|f| f.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        let document = call(&mut by_tool, "get_document", json!({}));
        let diagrams = document["diagrams"].as_array().unwrap().iter();
        let mut expected: Vec<String> =
            diagrams.map(|d| format!("{}.png", d["name"].as_str().unwrap())).collect();
        expected.push("total(n).png".into());
        expected.sort();
        assert_eq!(files, expected);

        assert!(run(&[p, "--diagram", "Nowhere", "--out", out.to_str().unwrap()], "").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
