//! The smoke corpus outside the repository (`$UMLCRAFT_ERM_REFS/corpus/<system>/<db>`):
//! Sakila, Pagila, Employees, Chinook and Northwind as dumped by MySQL 9.7, MariaDB 11.8 and
//! PostgreSQL 17.6, three dumps each. Skipped without `UMLCRAFT_ERM_REFS`.
//!
//! Every dump reads without a problem, gives the model the prototype gives
//! (`$UMLCRAFT_ERM_REFS/expected/<system>-<db>.md`, written by the Python prototype the
//! rules were worked out with, in the form of `support::describe`) and is laid out
//! without overlaps.

mod support;

use std::path::PathBuf;

use pretty_assertions::assert_eq;
use support::describe;
use umlcraft_erm::layout;
use umlcraft_erm_sql::{Options, import, read};

fn refs() -> Option<PathBuf> {
    std::env::var_os("UMLCRAFT_ERM_REFS").map(PathBuf::from)
}

#[test]
fn every_corpus_dump_imports_like_the_prototype() {
    let Some(refs) = refs() else {
        eprintln!("UMLCRAFT_ERM_REFS not set: corpus skipped");
        return;
    };
    let mut seen = 0;
    for system in ["mysql-9.7", "mariadb-11.8", "postgres-17.6"] {
        let dir = refs.join("corpus").join(system);
        let mut dbs: Vec<_> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        dbs.sort();
        for db in dbs {
            let name = db.file_name().unwrap().to_string_lossy().into_owned();
            let expected =
                std::fs::read_to_string(refs.join("expected").join(format!("{system}-{name}.md")))
                    .unwrap();
            let mut dumps: Vec<_> =
                std::fs::read_dir(&db).unwrap().flatten().map(|e| e.path()).collect();
            dumps.sort();
            for dump in dumps {
                let schema = read(&std::fs::read(&dump).unwrap(), None).unwrap();
                let what = dump.display().to_string();
                assert_eq!(schema.problems, [], "{what}");
                let result = import(&schema, &Options::default());
                assert_eq!(describe(&result.diagram), expected.trim_end(), "{what}");
                assert_eq!(layout::overlaps(&result.diagram), [], "{what}");
                seen += 1;
            }
        }
    }
    assert_eq!(seen, 27);
}
