//! Imports SQL dumps and writes their ER diagrams as SQL for MySQL and for PostgreSQL, to load
//! into real servers (the CI job "SQL on real servers" does so).
//!
//! ```text
//! cargo run -p umlcraft-erm-sql --example sql-export -- --out DIR DUMP…
//! ```
//!
//! Each dump becomes `DIR/<folder>-<name>-MySql.sql` and `DIR/<folder>-<name>-Postgres.sql`.
//! It fails if a dump or the SQL written for it does not read without a problem.

use std::path::PathBuf;

use umlcraft_erm_sql::{Dialect, Options, ddl, map, read};
use umlcraft_model::{Document, Model};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut dumps = Vec::new();
    let mut out = PathBuf::from(".");
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out = args.next().map(PathBuf::from).expect("--out DIR"),
            _ => dumps.push(PathBuf::from(a)),
        }
    }
    std::fs::create_dir_all(&out).expect("output folder");
    let mut failed = false;
    for path in dumps {
        let schema = read(&std::fs::read(&path).expect("readable dump"), None).expect("a dump");
        for p in &schema.problems {
            println!("{}: line {}: {:?}: {}", path.display(), p.line, p.kind, p.text);
            failed = true;
        }
        let mut doc = Document::new(Model::new("sql"));
        let root = doc.model().root();
        let mut diagram = None;
        doc.edit("import", |tx| {
            diagram = Some(map::map(tx, root, &schema, &Options::default())?.diagram);
            Ok(())
        })
        .expect("the import makes a valid model");
        let diagram = diagram.expect("imported");
        let folder = path.parent().and_then(|p| p.file_name()).map(|p| p.to_string_lossy());
        let stem = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();
        for dialect in [Dialect::MySql, Dialect::Postgres] {
            let sql = ddl(doc.model(), diagram, dialect).expect("an ER diagram");
            let name = format!("{}-{stem}-{dialect:?}.sql", folder.as_deref().unwrap_or("dump"));
            std::fs::write(out.join(&name), &sql.sql).expect("written");
            let again = read(sql.sql.as_bytes(), Some(dialect)).expect("SQL");
            for p in &again.problems {
                println!("{name}: line {}: {:?}: {}", p.line, p.kind, p.text);
                failed = true;
            }
            println!("{name}: {} tables, {} foreign keys", sql.tables, sql.foreign_keys);
            for l in &sql.left_out {
                println!("  left out: {l:?}");
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
