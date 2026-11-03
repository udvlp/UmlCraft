//! Cutting a dump into statements, as `mysqldump`, `mariadb-dump` and `pg_dump` write them.
//!
//! Works on bytes: data in `INSERT`s may be binary and need not be UTF-8, and lines may be
//! megabytes long. Comments are dropped, except MySQL's version comments `/*!40101 …*/` and
//! MariaDB's `/*M!100616 …*/`, whose content is code. Strings, quoted names, PostgreSQL's
//! `$tag$` bodies, `DELIMITER` and the data after `COPY … FROM stdin;` are kept whole.

use crate::dialect::{Dialect, trim};

/// One statement without its delimiter and comments, and the line it starts on (from 1).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Statement {
    pub line: usize,
    pub text: Vec<u8>,
}

/// The statements of `bytes` in order. A byte order mark is skipped.
pub fn split(bytes: &[u8], dialect: Dialect) -> Splitter<'_> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    Splitter {
        s: bytes,
        i: 0,
        line: 1,
        mysql: dialect.is_mysql(),
        delimiter: b";".to_vec(),
        at_line_start: true,
    }
}

pub struct Splitter<'a> {
    s: &'a [u8],
    i: usize,
    line: usize,
    mysql: bool,
    delimiter: Vec<u8>,
    at_line_start: bool,
}

impl Splitter<'_> {
    fn line_end(&self, from: usize) -> usize {
        self.s[from..].iter().position(|&b| b == b'\n').map_or(self.s.len(), |p| from + p)
    }

    /// Moves past the line that starts at `self.i` (and its line break).
    fn skip_line(&mut self) {
        let end = self.line_end(self.i);
        if end < self.s.len() {
            self.line += 1;
        }
        self.i = (end + 1).min(self.s.len());
    }

    /// Whole-line constructs where no statement is open: `DELIMITER` (MySQL) and psql's
    /// meta-commands such as `\restrict` (PostgreSQL). True if the line was used up.
    fn line_command(&mut self) -> bool {
        let end = self.line_end(self.i);
        let line = trim(&self.s[self.i..end]);
        if self.mysql && line.len() > 10 && line[..10].eq_ignore_ascii_case(b"DELIMITER ") {
            let d = trim(&line[10..]);
            self.delimiter = if d.is_empty() { b";".to_vec() } else { d.to_vec() };
            self.skip_line();
            return true;
        }
        if !self.mysql && line.first() == Some(&b'\\') {
            self.skip_line();
            return true;
        }
        false
    }

    /// After `COPY … FROM stdin;`: the data rows up to the line `\.`.
    fn skip_copy_data(&mut self) {
        // The rest of the COPY line.
        self.skip_line();
        while self.i < self.s.len() {
            let end = self.line_end(self.i);
            let row = &self.s[self.i..end];
            let done = row.strip_suffix(b"\r").unwrap_or(row) == b"\\.";
            self.skip_line();
            if done {
                break;
            }
        }
        self.at_line_start = true;
    }

    /// The end (exclusive) of the string or quoted name that starts at `i`.
    fn quoted_end(&self, i: usize) -> usize {
        let s = self.s;
        let q = s[i];
        // MySQL escapes with backslashes in strings; PostgreSQL only in E'…' strings.
        let escapes = q != b'`'
            && (self.mysql
                || (q == b'\''
                    && i > 0
                    && s[i - 1].eq_ignore_ascii_case(&b'e')
                    && (i < 2 || !is_ident(s[i - 2]))));
        let mut j = i + 1;
        while j < s.len() {
            if escapes && s[j] == b'\\' {
                j += 2;
                continue;
            }
            if s[j] == q {
                if s.get(j + 1) == Some(&q) {
                    j += 2;
                    continue;
                }
                return j + 1;
            }
            j += 1;
        }
        s.len()
    }

    /// The end (exclusive) of a `$tag$ … $tag$` body that starts at `i`, if one starts there.
    fn dollar_end(&self, i: usize) -> Option<usize> {
        let s = self.s;
        if i > 0 && is_ident(s[i - 1]) {
            return None;
        }
        let mut j = i + 1;
        if j < s.len() && (s[j].is_ascii_alphabetic() || s[j] == b'_') {
            while j < s.len() && (s[j].is_ascii_alphanumeric() || s[j] == b'_') {
                j += 1;
            }
        }
        if s.get(j) != Some(&b'$') {
            return None;
        }
        let tag = &s[i..=j];
        let body = j + 1;
        Some(find(&s[body..], tag).map_or(s.len(), |p| body + p + tag.len()))
    }
}

impl Iterator for Splitter<'_> {
    type Item = Statement;

    fn next(&mut self) -> Option<Statement> {
        let mut buf: Vec<u8> = Vec::new();
        let mut start: Option<usize> = None;
        let n = self.s.len();
        while self.i < n {
            if self.at_line_start {
                if start.is_none() && self.line_command() {
                    continue;
                }
                self.at_line_start = false;
            }
            let i = self.i;
            let c = self.s[i];
            if c == b'\n' {
                self.line += 1;
                self.at_line_start = true;
                if start.is_some() {
                    buf.push(c);
                }
                self.i += 1;
                continue;
            }
            let next = self.s.get(i + 1).copied();
            let dash_comment = c == b'-'
                && next == Some(b'-')
                && (!self.mysql || self.s.get(i + 2).is_none_or(|b| b.is_ascii_whitespace()));
            if dash_comment || (self.mysql && c == b'#') {
                self.i = self.line_end(i);
                continue;
            }
            if c == b'/' && next == Some(b'*') {
                let end = find(&self.s[i + 2..], b"*/").map_or(n, |p| i + 2 + p);
                let body = &self.s[i + 2..end];
                self.i = (end + 2).min(n);
                let first_line = self.line;
                self.line += count_lines(body);
                if self.mysql
                    && let Some(code) = versioned(body)
                    && !trim(code).starts_with(b"\\")
                    && !trim(code).is_empty()
                {
                    start.get_or_insert(first_line);
                    buf.push(b' ');
                    buf.extend_from_slice(code);
                    buf.push(b' ');
                }
                continue;
            }
            let quoted = match c {
                b'\'' | b'"' | b'`' => Some(self.quoted_end(i)),
                b'$' if !self.mysql => self.dollar_end(i),
                _ => None,
            };
            if let Some(end) = quoted {
                start.get_or_insert(self.line);
                let part = &self.s[i..end];
                self.line += count_lines(part);
                buf.extend_from_slice(part);
                self.i = end;
                continue;
            }
            if self.s[i..].starts_with(&self.delimiter) {
                self.i += self.delimiter.len();
                let Some(line) = start else { continue };
                let text = trim(&buf).to_vec();
                if !self.mysql && is_copy_from_stdin(&text) {
                    self.skip_copy_data();
                }
                return Some(Statement { line, text });
            }
            if !c.is_ascii_whitespace() {
                start.get_or_insert(self.line);
            }
            if start.is_some() {
                buf.push(c);
            }
            self.i += 1;
        }
        start.map(|line| Statement { line, text: trim(&buf).to_vec() })
    }
}

/// The code inside a version comment (`!40101 …`, `M!100616 …`), without the marker.
fn versioned(body: &[u8]) -> Option<&[u8]> {
    let rest = body.strip_prefix(b"!").or_else(|| body.strip_prefix(b"M!"))?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    Some(&rest[digits..])
}

fn is_copy_from_stdin(text: &[u8]) -> bool {
    let upper = text.to_ascii_uppercase();
    upper.starts_with(b"COPY") && upper.ends_with(b"FROM STDIN")
}

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}

fn count_lines(s: &[u8]) -> usize {
    s.iter().filter(|&&b| b == b'\n').count()
}

pub(crate) fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let mut from = 0;
    while let Some(p) = haystack[from..].iter().position(|&b| b == first) {
        let at = from + p;
        if haystack[at..].starts_with(needle) {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(sql: &str, dialect: Dialect) -> Vec<(usize, String)> {
        split(sql.as_bytes(), dialect)
            .map(|s| (s.line, String::from_utf8(s.text).unwrap()))
            .collect()
    }

    fn only(sql: &str, dialect: Dialect) -> Vec<String> {
        texts(sql, dialect).into_iter().map(|(_, t)| t).collect()
    }

    #[test]
    fn semicolons_in_strings_and_names_do_not_end_a_statement() {
        let sql = "INSERT INTO t VALUES ('a;b', 'O\\'Brien;');\nSELECT `x;y` FROM t;";
        assert_eq!(
            only(sql, Dialect::MySql),
            ["INSERT INTO t VALUES ('a;b', 'O\\'Brien;')", "SELECT `x;y` FROM t"]
        );
        let pg = "INSERT INTO t VALUES ('O''Brien; x', E'it\\'s;');\nSELECT \"a;\" FROM t;";
        assert_eq!(
            only(pg, Dialect::Postgres),
            ["INSERT INTO t VALUES ('O''Brien; x', E'it\\'s;')", "SELECT \"a;\" FROM t"]
        );
    }

    #[test]
    fn comments_go_and_version_comments_stay() {
        let sql = "-- head\n/*!40101 SET NAMES utf8mb4 */;\n# hash\n/* plain; */ SELECT 1;\n\
                   /*M!999999\\- enable the sandbox mode */\nSELECT 2;";
        assert_eq!(
            texts(sql, Dialect::MariaDb),
            [(2, "SET NAMES utf8mb4".to_owned()), (4, "SELECT 1".into()), (6, "SELECT 2".into())]
        );
        // MySQL needs a space after `--`; PostgreSQL does not.
        assert_eq!(only("SELECT 1--1;", Dialect::MySql), ["SELECT 1--1"]);
        assert_eq!(only("SELECT 1--1;\nSELECT 2;", Dialect::Postgres), ["SELECT 1\nSELECT 2"]);
    }

    #[test]
    fn delimiter_changes_what_ends_a_statement() {
        let sql = "DELIMITER ;;\n/*!50003 CREATE*/ /*!50003 TRIGGER t BEFORE INSERT ON a FOR EACH \
                   ROW BEGIN\n  -- note; here\n  SET NEW.x = 1;\nEND */;;\nDELIMITER ;\nSELECT 1;";
        let got = only(sql, Dialect::MySql);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[0].contains("SET NEW.x = 1;") && got[0].ends_with("END"), "{}", got[0]);
        assert_eq!(got[1], "SELECT 1");
    }

    #[test]
    fn dollar_bodies_copy_data_and_meta_commands() {
        let sql = "\\restrict KEY\nCREATE FUNCTION f() RETURNS int AS $body$ SELECT 1; -- x\n\
                   $body$;\nCOPY public.t (a, b) FROM stdin;\n1\t'x;y\n2\t\\N\n\\.\n\
                   SELECT $$a;b$$, $1;\n\\unrestrict KEY\n";
        let got = texts(sql, Dialect::Postgres);
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[0].1.ends_with("$body$ SELECT 1; -- x\n$body$"));
        assert_eq!(got[1], (4, "COPY public.t (a, b) FROM stdin".to_owned()));
        assert_eq!(got[2], (8, "SELECT $$a;b$$, $1".to_owned()));
    }

    #[test]
    fn binary_data_crlf_and_bom() {
        let mut sql = b"\xef\xbb\xbfCREATE TABLE a (id int);\r\nINSERT INTO a VALUES (_binary '\x00\xff\\'\xc0');\r\n"
            .to_vec();
        sql.extend_from_slice(b"SELECT 1");
        let got: Vec<_> = split(&sql, Dialect::MySql).collect();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].text, b"CREATE TABLE a (id int)");
        assert_eq!(got[1].line, 2);
        assert_eq!(got[2], Statement { line: 3, text: b"SELECT 1".to_vec() });
    }

    #[test]
    fn empty_statements_are_dropped() {
        assert_eq!(only(";;\n ; SELECT 1;;", Dialect::MySql), ["SELECT 1"]);
    }
}
