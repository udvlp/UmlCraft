//! Which database wrote a dump, and the name of the database if the dump says it.

use std::fmt;

/// The SQL dialect of a dump. MySQL and MariaDB read the same; they are kept apart for the
/// report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dialect {
    MySql,
    MariaDb,
    Postgres,
}

impl Dialect {
    pub const ALL: [Self; 3] = [Self::MySql, Self::MariaDb, Self::Postgres];

    /// MySQL syntax: backticks, `#` comments, backslash escapes, `/*!…*/`, `DELIMITER`.
    pub const fn is_mysql(self) -> bool {
        matches!(self, Self::MySql | Self::MariaDb)
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
            Self::Postgres => "PostgreSQL",
        }
    }
}

impl fmt::Display for Dialect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// How far into the file the dump tools' header comments are looked for. MariaDB writes a
/// sandbox comment before its header, so it is not always the first line.
const HEADER: usize = 2048;
/// How far the guess looks when there is no header (scripts written by hand).
const GUESS: usize = 20_000;

/// The dialect after the header of `mysqldump`, `mariadb-dump` or `pg_dump`; without one a
/// guess from the code (not the comments): backticks or `ENGINE=` mean MySQL, else
/// PostgreSQL.
pub fn detect(bytes: &[u8]) -> Dialect {
    let head = &bytes[..bytes.len().min(HEADER)];
    if contains(head, b"-- MariaDB dump") {
        return Dialect::MariaDb;
    }
    if contains(head, b"-- MySQL dump") {
        return Dialect::MySql;
    }
    if contains(head, b"PostgreSQL database dump") {
        return Dialect::Postgres;
    }
    let code = &bytes[..bytes.len().min(GUESS)];
    let mysql = code
        .split(|&b| b == b'\n')
        .map(trim)
        .filter(|l| !l.starts_with(b"--") && !l.starts_with(b"#"))
        .any(|l| l.contains(&b'`') || contains(&l.to_ascii_uppercase(), b"ENGINE="));
    if mysql { Dialect::MySql } else { Dialect::Postgres }
}

/// The name of the database from the header line `-- Host: … Database: flug` of
/// `mysqldump`/`mariadb-dump`. `USE` and `CREATE DATABASE` are read with the statements.
pub fn header_database(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(HEADER)];
    head.split(|&b| b == b'\n').find_map(|line| {
        let line = std::str::from_utf8(line).ok()?;
        let rest = line.strip_prefix("-- Host:")?;
        let name = rest.split("Database:").nth(1)?.trim();
        (!name.is_empty()).then(|| name.to_owned())
    })
}

pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

pub(crate) fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !b.is_ascii_whitespace()).map_or(start, |e| e + 1);
    &s[start..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_name_the_dialect() {
        assert_eq!(detect(b"-- MySQL dump 10.13  Distrib 9.7.4\n"), Dialect::MySql);
        let maria = b"/*M!999999\\- enable the sandbox mode */ \n-- MariaDB dump 10.20\n";
        assert_eq!(detect(maria), Dialect::MariaDb);
        assert_eq!(detect(b"--\n-- PostgreSQL database dump\n--\n"), Dialect::Postgres);
    }

    #[test]
    fn without_header_the_code_decides() {
        assert_eq!(detect(b"CREATE TABLE `a` (id int);"), Dialect::MySql);
        assert_eq!(detect(b"CREATE TABLE a (id int) engine=InnoDB;"), Dialect::MySql);
        // A backtick in a comment does not make it MySQL.
        assert_eq!(detect(b"-- the schema `fracht`\nCREATE TABLE a (id int);"), Dialect::Postgres);
    }

    #[test]
    fn the_header_names_the_database() {
        let dump = b"-- MySQL dump 10.13\n--\n-- Host: 127.0.0.1    Database: flug\n";
        assert_eq!(header_database(dump).as_deref(), Some("flug"));
        assert_eq!(header_database(b"-- Host: localhost    Database: \n"), None);
    }
}
