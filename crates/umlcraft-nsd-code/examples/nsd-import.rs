//! Smoke test over real code: every method of every Java, C#, Python and C++ file under the
//! given paths as a structogram, laid out, drawn, written and read back. Prints counts, time
//! and what the reports name; with `-o DIR` writes the diagrams there (`Class.method-LINE.nsd`).
//!
//! `cargo run --release -p umlcraft-nsd-code --example nsd-import -- [-o DIR] PATH...`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use umlcraft_nsd::layout::Layout;
use umlcraft_nsd::{Metrics, read, write};
use umlcraft_nsd_code::{Made, Source, SourceLanguage};

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let out = match args.iter().position(|a| a == "-o") {
        Some(i) => {
            let dir = PathBuf::from(args.remove(i + 1));
            args.remove(i);
            std::fs::create_dir_all(&dir).expect("output folder");
            Some(dir)
        }
        None => None,
    };
    let paths: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();
    let extensions: Vec<&str> =
        SourceLanguage::ALL.into_iter().flat_map(|l| l.extensions().iter().copied()).collect();
    let files = umlcraft_reverse::source::collect_sources(&paths, &extensions).expect("sources");
    let made = Made {
        author: "smoke",
        date: "2026-10-29",
        origin: "UmlCraft smoke test",
        keywords: umlcraft_nsd::Keywords::English,
    };
    let start = Instant::now();
    let (mut methods, mut elements, mut largest) = (0usize, 0usize, (0usize, String::new()));
    let mut notes: BTreeMap<String, usize> = BTreeMap::new();
    let mut failures = Vec::new();
    for file in &files {
        let text = umlcraft_nsd_code::read(file).expect("readable file").text;
        let Some(language) = SourceLanguage::of(file) else { continue };
        let source = Source::parse(language, text);
        for m in source.methods() {
            let label = format!("{}:{} {}", file.display(), m.line, m.name);
            let result = std::panic::catch_unwind(|| {
                let (d, report) = source.diagram(&m, &made).expect("method found again");
                let layout = Layout::of(&d, Metrics::linux());
                let _ = umlcraft_nsd::svg::render(&d, Metrics::linux());
                let written = write(&d);
                let back = read(&written).map_err(|e| e.to_string())?;
                if back != d {
                    return Err("does not read back as written".to_owned());
                }
                Ok((d, report, layout.size, written))
            });
            methods += 1;
            match result {
                Ok(Ok((d, report, size, written))) => {
                    elements += count(&d.children);
                    let area = (size.0 as usize) * (size.1 as usize);
                    if area > largest.0 {
                        largest = (area, format!("{label} ({}×{} px)", size.0, size.1));
                    }
                    for n in report.notes {
                        *notes.entry(format!("{:?}", n.kind)).or_default() += 1;
                    }
                    if let Some(dir) = &out {
                        let stem = m.file_name();
                        let name = format!("{}-{}.nsd", stem.trim_end_matches(".nsd"), m.line);
                        std::fs::write(dir.join(name), written).expect("writable output");
                    }
                }
                Ok(Err(e)) => failures.push(format!("{label}: {e}")),
                Err(_) => failures.push(format!("{label}: panicked")),
            }
        }
    }
    println!(
        "{} files, {methods} methods, {elements} elements in {:.2?}",
        files.len(),
        start.elapsed()
    );
    println!("largest: {}", largest.1);
    for (kind, n) in &notes {
        println!("  {kind}: {n}");
    }
    for f in &failures {
        println!("FAILED {f}");
    }
    if !failures.is_empty() {
        std::process::exit(1);
    }
}

fn count(seq: &umlcraft_nsd::Sequence) -> usize {
    seq.elements.iter().map(|e| 1 + e.branches.iter().map(count).sum::<usize>()).sum()
}
