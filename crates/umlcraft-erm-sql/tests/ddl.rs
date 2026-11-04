//! SQL from the imported test schema: it reads again without a problem and gives the model
//! it came from (Dump → ER → SQL → ER).

mod support;

use pretty_assertions::assert_eq;
use support::{describe, fixtures, imported, rules, unordered};
use umlcraft_erm_sql::{Dialect, LeftOut, Options, ddl, read};

/// With `UMLCRAFT_DDL_OUT` set to a folder, the SQL a test writes goes there as
/// `{system}-{name}-{dialect}.sql`, to try it on a real server.
fn keep(system: &str, name: &str, dialect: Dialect, sql: &str) {
    if let Some(dir) = std::env::var_os("UMLCRAFT_DDL_OUT") {
        let file = std::path::Path::new(&dir).join(format!("{system}-{name}-{dialect:?}.sql"));
        std::fs::write(file, sql).unwrap();
    }
}

#[test]
fn the_sql_of_a_dump_gives_its_model_again() {
    for (system, name, dialect) in [
        ("mysql-9.7", "no-data", Dialect::MySql),
        ("mysql-8.4", "data", Dialect::MySql),
        ("mariadb-11.8", "routines", Dialect::MariaDb),
        ("postgres-17.6", "schema", Dialect::Postgres),
        ("postgres-16", "copy", Dialect::Postgres),
    ] {
        let path = fixtures().join("dumps").join(system).join(format!("{name}.sql"));
        let schema = read(&std::fs::read(path).unwrap(), None).unwrap();
        let (doc, result) = imported(&schema, &Options::default(), false);
        let model = doc.model();
        let named = umlcraft_erm_sql::imported_dialect(model, result.diagram);
        assert_eq!(named, Some(dialect), "{system}");
        let out = ddl(model, result.diagram, dialect).unwrap();
        keep(system, name, dialect, &out.sql);
        assert_eq!(out.left_out, [], "{system}");
        let again = read(out.sql.as_bytes(), Some(dialect)).unwrap();
        assert_eq!(again.problems, [], "{system}\n{}", out.sql);
        assert_eq!(again.tables.len(), schema.tables.len(), "{system}");
        let (doc2, result2) = imported(&again, &Options::default(), false);
        assert_eq!(
            unordered(&describe(doc2.model(), result2.package)),
            unordered(&describe(model, result.package)),
            "{system}\n{}",
            out.sql
        );
        // Types, conditions, uniqueness, indexes and domains come back as well.
        assert_eq!(
            rules(doc2.model(), result2.package),
            rules(model, result.package),
            "{system}\n{}",
            out.sql
        );
    }
}

/// A dump of one dialect written for the other reads there without a problem and gives the
/// same tables, keys and relationships; generated columns become plain ones.
#[test]
fn the_sql_for_the_other_dialect_gives_the_same_model() {
    for (system, name, to) in [
        ("postgres-17.6", "schema", Dialect::MySql),
        ("mysql-9.7", "no-data", Dialect::Postgres),
        ("mariadb-11.8", "no-data", Dialect::Postgres),
    ] {
        let path = fixtures().join("dumps").join(system).join(format!("{name}.sql"));
        let schema = read(&std::fs::read(path).unwrap(), None).unwrap();
        let (doc, result) = imported(&schema, &Options::default(), false);
        let model = doc.model();
        let out = ddl(model, result.diagram, to).unwrap();
        keep(system, name, to, &out.sql);
        let generated = LeftOut::Generated { table: "flug".into(), column: "dauer_min".into() };
        assert_eq!(out.left_out, [generated], "{system}");
        let again = read(out.sql.as_bytes(), Some(to)).unwrap();
        assert_eq!(again.problems, [], "{system}\n{}", out.sql);
        assert_eq!(again.tables.len(), schema.tables.len(), "{system}");
        let (doc2, result2) = imported(&again, &Options::default(), false);
        // A schema of PostgreSQL is part of the table's name in MySQL, which capitalizes it.
        let plain = |t: String| t.replace(" (abgeleitet)", "").replace("Fracht.", "fracht.");
        assert_eq!(
            unordered(&plain(describe(doc2.model(), result2.package))),
            unordered(&plain(describe(model, result.package))),
            "{system}\n{}",
            out.sql
        );
    }
}
