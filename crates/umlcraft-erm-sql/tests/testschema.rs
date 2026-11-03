//! The test schema (`tests/fixtures/schema`, the flight example and every import case) as
//! dumped by MySQL 8.4 and 9.7, MariaDB 11.8 and PostgreSQL 16 and 17.6, three dumps each:
//! without data, with data (`COPY` / `INSERT`) and with routines and triggers. All dumps of
//! one system give the same model, and it is the one in `docs/erm-spec.md` § 6.3.

mod support;

use pretty_assertions::assert_eq;
use support::{describe, expected, fixtures};
use umlcraft_erm::{check, layout, svg};
use umlcraft_erm_sql::map::map;
use umlcraft_erm_sql::{Dialect, Options, import, read, split::split};

/// Each system with its dumps, the expected result and how many statements the prototype's
/// splitter found in each dump.
/// A system: its folder, dialect, expected table and dumps with their statement counts.
type System = (&'static str, Dialect, &'static str, [(&'static str, usize); 3]);

const SYSTEMS: [System; 5] = [
    ("mysql-9.7", Dialect::MySql, "mysql.md", [("no-data", 142), ("data", 232), ("routines", 246)]),
    (
        "mysql-8.4",
        Dialect::MySql,
        "mysql-8.4.md",
        [("no-data", 138), ("data", 228), ("routines", 242)],
    ),
    (
        "mariadb-11.8",
        Dialect::MariaDb,
        "mysql.md",
        [("no-data", 138), ("data", 282), ("routines", 296)],
    ),
    (
        "postgres-17.6",
        Dialect::Postgres,
        "postgres.md",
        [("schema", 150), ("copy", 180), ("inserts", 191)],
    ),
    (
        "postgres-16",
        Dialect::Postgres,
        "postgres.md",
        [("schema", 149), ("copy", 179), ("inserts", 190)],
    ),
];

fn dump(system: &str, name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join("dumps").join(system).join(format!("{name}.sql"))).unwrap()
}

#[test]
fn the_splitter_finds_every_statement() {
    for (system, dialect, _, dumps) in SYSTEMS {
        for (name, count) in dumps {
            let bytes = dump(system, name);
            assert_eq!(umlcraft_erm_sql::dialect::detect(&bytes), dialect, "{system}/{name}");
            assert_eq!(split(&bytes, dialect).count(), count, "{system}/{name}");
        }
    }
}

#[test]
fn every_dump_gives_the_expected_model() {
    for (system, _, expect, dumps) in SYSTEMS {
        for (name, _) in dumps {
            let schema = read(&dump(system, name), None).unwrap();
            assert_eq!(schema.problems, [], "{system}/{name}");
            let result = map(&schema, &Options::default());
            assert_eq!(describe(&result.diagram), expected(expect).trim_end(), "{system}/{name}");
            // pg_dump names the database only with -C.
            let name_expected = if system.starts_with("postgres") { "SQL" } else { "flug" };
            assert_eq!(result.diagram.meta.name, name_expected, "{system}/{name}");
        }
    }
}

#[test]
fn the_imported_diagram_is_valid_and_readable() {
    for (system, _, _, [(name, _), ..]) in SYSTEMS {
        let result = import(&read(&dump(system, name), None).unwrap(), &Options::default());
        let d = &result.diagram;
        // Not even warnings: an import is a finished diagram.
        assert_eq!(check(d), [], "{system}");
        let named = |id: &umlcraft_erm::Id| d.node(id).map_or(id.to_string(), |n| n.name.clone());
        let overlaps: Vec<_> = layout::overlaps(d)
            .iter()
            .map(|(a, b)| format!("{} / {}", named(a), named(b)))
            .collect();
        assert_eq!(overlaps, Vec::<String>::new(), "{system}");
        assert!(d.nodes.iter().all(|n| n.bounds.x % 10.0 == 0.0 && n.bounds.y % 10.0 == 0.0));
        // The file round trip keeps it.
        let (back, _) = umlcraft_erm::read(&umlcraft_erm::write(d)).unwrap();
        let mut sorted = d.clone();
        sorted.sort();
        assert_eq!(back, sorted, "{system}");
    }
}

#[test]
fn the_report_counts() {
    let s = read(&dump("postgres-17.6", "copy"), None).unwrap();
    let r = import(&s, &Options::default()).report;
    assert_eq!((r.tables, r.entities, r.relationships), (19, 16, 17));
    assert!(r.skipped_total() > 100);
    let hints: Vec<_> = r.hints.iter().map(|h| format!("{h:?}")).collect();
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert!(hints[0].contains("fracht.Fracht Stück"));
    assert_eq!(s.schemas(), ["fracht", "public"]);
}

/// The table without the order of the tables and of a relationship's ends, which follow
/// the source (so does which end is `m`).
fn unordered(table: &str) -> Vec<String> {
    let table = table.replace("[0,m]", "[0,n]");
    let mut lines: Vec<String> = table
        .lines()
        .map(|l| {
            let mut cells: Vec<String> = l.split(" | ").map(str::to_owned).collect();
            if let Some(ends) = cells.get_mut(3) {
                let mut e: Vec<&str> = ends.trim_end_matches(" |").split("; ").collect();
                e.sort_unstable();
                *ends = e.join("; ");
            }
            cells.join(" | ")
        })
        .collect();
    lines.sort();
    lines
}

#[test]
fn the_sources_give_the_same_model_as_their_dumps() {
    for (file, dialect, expect) in [
        ("mysql.sql", Dialect::MySql, "mysql.md"),
        ("postgres.sql", Dialect::Postgres, "postgres.md"),
    ] {
        let bytes = std::fs::read(fixtures().join("schema").join(file)).unwrap();
        let s = read(&bytes, None).unwrap();
        assert_eq!(s.dialect, dialect);
        assert_eq!(s.problems, [], "{file}");
        let got = describe(&map(&s, &Options::default()).diagram);
        assert_eq!(unordered(&got), unordered(&expected(expect)), "{file}");
    }
}

/// The picture of the test schema stays as it is (`tests/snapshots/*.svg`). After an intended
/// change to the layout, `UMLCRAFT_ERM_BLESS=1 cargo test -p umlcraft-erm-sql --test
/// testschema` writes them anew; look at them before committing.
#[test]
fn the_layout_stays_as_it_is() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    for (system, name) in [("postgres-17.6", "schema"), ("mysql-9.7", "no-data")] {
        let result = import(&read(&dump(system, name), None).unwrap(), &Options::default());
        let ours = svg::render(&result.diagram);
        let path = dir.join(format!("{system}.svg"));
        if std::env::var_os("UMLCRAFT_ERM_BLESS").is_some() {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &ours).unwrap();
        }
        assert_eq!(ours, std::fs::read_to_string(&path).unwrap(), "{system}.svg");
    }
}
