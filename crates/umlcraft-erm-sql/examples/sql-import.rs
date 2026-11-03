//! Imports SQL dumps and prints what was found, how long it took and what overlaps.
//!
//! ```text
//! cargo run --release -p umlcraft-erm-sql --example sql-import -- DUMP… \
//!     [--attributes all|keys|none] [--out DIR]
//! ```
//!
//! With `--out`, each dump is written as `DIR/<name>.ucerm` and `DIR/<name>.svg`.

use std::path::PathBuf;
use std::time::Instant;

use umlcraft_erm::{layout, svg, write};
use umlcraft_erm_sql::{Attributes, Options, import, read};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dumps = Vec::new();
    let mut out: Option<PathBuf> = None;
    let mut attributes = Attributes::All;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next().map(PathBuf::from),
            "--attributes" => {
                attributes = match args.next().as_deref() {
                    Some("keys") => Attributes::Keys,
                    Some("none") => Attributes::None,
                    _ => Attributes::All,
                }
            }
            _ => dumps.push(PathBuf::from(a)),
        }
    }
    let mut failed = false;
    for path in dumps {
        let bytes = std::fs::read(&path).expect("readable dump");
        let start = Instant::now();
        let schema = match read(&bytes, None) {
            Ok(s) => s,
            Err(e) => {
                println!("{}: {e}", path.display());
                failed = true;
                continue;
            }
        };
        let parsed = start.elapsed();
        let file_name = path.file_name().map(|f| f.to_string_lossy().into_owned());
        let result = import(&schema, &Options { attributes, file_name, ..Options::default() });
        let total = start.elapsed();
        let d = &result.diagram;
        let r = &result.report;
        let overlaps = layout::overlaps(d);
        let crossings = layout::crossings(d);
        let (w, h) = d.nodes.iter().fold((0.0f64, 0.0f64), |(w, h), n| {
            (w.max(n.bounds.x + n.bounds.width), h.max(n.bounds.y + n.bounds.height))
        });
        println!(
            "{}: {} ({:.1} MB) {} tables, {} entities, {} relationships, {} attributes, {} \
             skipped, {} problems, {} hints | {w:.0} × {h:.0} px, {} overlaps, {} lines \
             through nodes | parse {:.0} ms, total {:.0} ms",
            path.display(),
            r.dialect,
            bytes.len() as f64 / 1e6,
            r.tables,
            r.entities,
            r.relationships,
            r.attributes,
            r.skipped_total(),
            r.problems.len(),
            r.hints.len(),
            overlaps.len(),
            crossings.len(),
            parsed.as_secs_f64() * 1e3,
            total.as_secs_f64() * 1e3,
        );
        for p in &r.problems {
            println!("  line {}: {:?}: {}", p.line, p.kind, p.text);
        }
        failed |= !r.problems.is_empty() || !overlaps.is_empty();
        if let Some(dir) = &out {
            std::fs::create_dir_all(dir).expect("output folder");
            let stem = path.with_extension("");
            let parent =
                stem.parent().and_then(|p| p.file_name()).map(|p| p.to_string_lossy().into_owned());
            let name = format!(
                "{}-{}",
                parent.unwrap_or_default(),
                stem.file_name().unwrap_or_default().to_string_lossy()
            );
            std::fs::write(dir.join(format!("{name}.ucerm")), write(d)).expect("written");
            std::fs::write(dir.join(format!("{name}.svg")), svg::render(d)).expect("written");
        }
    }
    if failed {
        std::process::exit(1);
    }
}
