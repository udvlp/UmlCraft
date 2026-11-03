//! SQL from the imported test schema: it reads again without a problem and gives the model
//! it came from (Dump → ER → SQL → ER).

mod support;

use pretty_assertions::assert_eq;
use support::{describe, fixtures, imported, unordered};
use umlcraft_erm_sql::{Dialect, Options, ddl, read};

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
        if std::env::var_os("UMLCRAFT_DDL_PRINT").is_some() {
            println!("{}", out.sql);
        }
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
    }
}
