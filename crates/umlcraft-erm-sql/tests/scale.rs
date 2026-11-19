//! A large dump is laid out in seconds, not minutes: placing the attributes of hundreds of
//! tables once tested every box against every node, line and attribute placed before.

mod support;

use std::time::{Duration, Instant};

use support::imported;
use umlcraft_erm_sql::{Options, read};

/// A MySQL dump of `tables` tables with four columns each, every table referring to the one
/// before it and to the one at half its number.
fn dump(tables: usize) -> String {
    let mut out = String::from("-- MySQL dump 10.13  Distrib 8.4.0, for Linux (x86_64)\n");
    for t in 0..tables {
        let mut refs: Vec<usize> =
            [t.checked_sub(1), (t > 1).then_some(t / 2)].into_iter().flatten().collect();
        refs.dedup();
        let mut lines = vec![
            "  `id` int NOT NULL AUTO_INCREMENT".to_owned(),
            "  `name` varchar(80) NOT NULL".to_owned(),
            "  `note` text".to_owned(),
            "  `created` datetime NOT NULL".to_owned(),
        ];
        lines.extend(refs.iter().map(|r| format!("  `t{r}_id` int DEFAULT NULL")));
        lines.push("  PRIMARY KEY (`id`)".to_owned());
        lines.extend(refs.iter().map(|r| format!("  KEY `fk_t{t}_t{r}` (`t{r}_id`)")));
        lines.extend(refs.iter().map(|r| {
            format!("  CONSTRAINT `fk_t{t}_t{r}` FOREIGN KEY (`t{r}_id`) REFERENCES `t{r}` (`id`)")
        }));
        out.push_str(&format!(
            "CREATE TABLE `t{t}` (\n{}\n) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4;\n\n",
            lines.join(",\n")
        ));
    }
    out
}

#[test]
fn a_dump_of_hundreds_of_tables_is_laid_out_in_seconds() {
    let tables = 200;
    let schema = read(dump(tables).as_bytes(), None).unwrap();
    assert_eq!(schema.problems, []);
    let start = Instant::now();
    let (_, result) = imported(&schema, &Options::default(), true);
    let took = start.elapsed();
    assert_eq!(result.report.tables, tables);
    // Measured: about 1 s in a release build and 8 s in a debug build; before, 18 s in a
    // release build.
    assert!(took < Duration::from_secs(60), "{tables} tables took {took:?}");
}
