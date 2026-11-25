//! `cargo xtask layers`: checks that the crates depend on one another only downwards, through
//! the layers in [`LAYERS`], that only the app knows the user interface (GPUI) and only
//! `umlcraft-screenshot` the Windows API. `cargo test --workspace` runs the same check.

use std::path::Path;
use std::process::{Command, ExitCode};

use serde_json::Value;

/// The workspace's crates from the bottom up. A crate depends (as a normal or build
/// dependency) only on crates of lower layers; a new crate goes into the table.
const LAYERS: &[&[&str]] = &[
    // The structogram model, which knows nothing of UML; the window capture.
    &["umlcraft-nsd", "umlcraft-screenshot"],
    // Elements, presentations, operations and undo.
    &["umlcraft-model"],
    // Drawing and editing the way the app does; the file format.
    &["umlcraft-notation", "umlcraft-format"],
    // Reading Astah's files; SQL in and out.
    &["umlcraft-asta", "umlcraft-erm-sql"],
    // Source import, export through Astah, the MCP tools.
    &["umlcraft-reverse", "umlcraft-asta-export", "umlcraft-mcp"],
    // Structograms from and to code; the command line.
    &["umlcraft-nsd-code", "umlcraft-cli"],
    // The app.
    &["umlcraft-app"],
];

/// Crates outside the layers: tools that build nothing UmlCraft ships.
const TOOLS: &[&str] = &["xtask"];

/// The only crates that may depend on GPUI (`gpui-kit` and the `gpui*` crates).
const UI_CRATES: &[&str] = &["umlcraft-app"];

/// Platform APIs and the only crates that may use them.
const PLATFORM: &[&str] =
    &["windows", "windows-sys", "windows-core", "winapi", "raw-window-handle"];
const PLATFORM_CRATES: &[&str] = &["umlcraft-screenshot"];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["layers"] => match metadata().map(|m| check(&m)) {
            Ok(problems) if problems.is_empty() => {
                let crates: usize = LAYERS.iter().map(|l| l.len()).sum();
                println!(
                    "layers: {crates} crates in {} layers, every dependency points down; GPUI \
                     only in {}",
                    LAYERS.len(),
                    UI_CRATES.join(", ")
                );
                ExitCode::SUCCESS
            }
            Ok(problems) => {
                for problem in problems {
                    eprintln!("layers: {problem}");
                }
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("layers: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask layers");
            ExitCode::FAILURE
        }
    }
}

/// `cargo metadata` of the workspace, without the dependencies' own packages.
fn metadata() -> Result<Value, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

fn layer(name: &str) -> Option<usize> {
    LAYERS.iter().position(|layer| layer.contains(&name))
}

fn is_ui(name: &str) -> bool {
    name.starts_with("gpui")
}

/// What breaks the rules, one line each; nothing when all is well.
fn check(metadata: &Value) -> Vec<String> {
    let mut problems = Vec::new();
    let packages = metadata["packages"].as_array().map(Vec::as_slice).unwrap_or_default();
    let names: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let internal = |name: &str| names.contains(&name);
    for package in packages {
        let Some(name) = package["name"].as_str() else { continue };
        let own = layer(name);
        if own.is_none() && !TOOLS.contains(&name) {
            problems.push(format!(
                "{name} is in no layer: add it to LAYERS in crates/xtask/src/main.rs, above \
                 every crate it depends on"
            ));
        }
        let dependencies = package["dependencies"].as_array().map(Vec::as_slice);
        for dependency in dependencies.unwrap_or_default() {
            let Some(dep) = dependency["name"].as_str() else { continue };
            let kind = dependency["kind"].as_str().unwrap_or("normal");
            if is_ui(dep) && !UI_CRATES.contains(&name) {
                problems.push(format!(
                    "{name} depends on {dep} ({kind}): only {} may know the user interface",
                    UI_CRATES.join(", ")
                ));
            }
            // Tests may reach further; they ship nowhere.
            if kind == "dev" {
                continue;
            }
            if PLATFORM.contains(&dep) && !PLATFORM_CRATES.contains(&name) {
                problems.push(format!(
                    "{name} depends on {dep}: only {} may use the platform's API",
                    PLATFORM_CRATES.join(", ")
                ));
            }
            if !internal(dep) {
                continue;
            }
            match (own, layer(dep)) {
                (Some(own), Some(theirs)) if theirs >= own => problems.push(format!(
                    "{name} (layer {own}) depends on {dep} (layer {theirs}), which is not below it"
                )),
                (None, _) => problems.push(format!("{name} is a tool and depends on {dep}")),
                (Some(_), None) => {
                    problems.push(format!("{name} depends on the tool {dep}"));
                }
                _ => {}
            }
        }
    }
    for listed in LAYERS.iter().flat_map(|layer| layer.iter()) {
        if !internal(listed) {
            problems.push(format!("{listed} is in LAYERS but not in the workspace"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_workspace_keeps_its_layers() {
        let problems = check(&metadata().unwrap());
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }

    #[test]
    fn upward_ui_platform_and_unknown_dependencies_are_reported() {
        let package = |name: &str, deps: &[(&str, Option<&str>)]| {
            let deps: Vec<Value> =
                deps.iter().map(|(name, kind)| json!({ "name": name, "kind": kind })).collect();
            json!({ "name": name, "dependencies": deps })
        };
        let mut packages: Vec<Value> = LAYERS
            .iter()
            .flat_map(|layer| layer.iter())
            .filter(|name| !["umlcraft-model", "umlcraft-mcp"].contains(name))
            .map(|name| package(name, &[]))
            .collect();
        packages.push(package(
            "umlcraft-model",
            &[
                ("umlcraft-notation", None),
                ("gpui-kit", Some("dev")),
                ("windows", None),
                ("windows-sys", None),
                ("xtask", None),
                ("umlcraft-app", Some("dev")),
            ],
        ));
        packages.push(package("umlcraft-mcp", &[("umlcraft-model", None), ("serde_json", None)]));
        packages.push(package("umlcraft-new", &[]));
        packages.push(package("xtask", &[]));
        let problems = check(&json!({ "packages": packages }));
        let expected = [
            "umlcraft-model (layer 1) depends on umlcraft-notation (layer 2)",
            "umlcraft-model depends on gpui-kit (dev)",
            "umlcraft-model depends on windows:",
            "umlcraft-model depends on windows-sys",
            "umlcraft-model depends on the tool xtask",
            "umlcraft-new is in no layer",
        ];
        for text in expected {
            assert!(problems.iter().any(|p| p.starts_with(text)), "{text}: {problems:#?}");
        }
        // A test reaching up is allowed, a dependency down too.
        assert_eq!(problems.len(), expected.len(), "{problems:#?}");
    }
}
