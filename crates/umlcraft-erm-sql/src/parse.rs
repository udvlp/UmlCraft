//! Reading the statements the import needs: `CREATE TABLE`,
//! `ALTER TABLE … ADD PRIMARY KEY | UNIQUE | FOREIGN KEY`, `ATTACH PARTITION`, `CREATE UNIQUE
//! INDEX`, `COMMENT ON TABLE | COLUMN`; `USE`/`CREATE DATABASE` for the name and
//! `CREATE VIEW` for the placeholder tables of older `mysqldump`s.
//!
//! The parser is tolerant on purpose: it looks for the few things it needs and steps over
//! table and column options it does not know, so that one unusual clause does not lose a
//! table. Types stay as written. Everything else is counted by its first words.

use std::borrow::Cow;
use std::collections::HashSet;

use crate::dialect::{Dialect, header_database};
use crate::ir::{Column, ForeignKey, Problem, ProblemKind, Schema, Table, TableKey};
use crate::split::split;

/// Reads `bytes` as a dump in `dialect`.
pub fn parse(bytes: &[u8], dialect: Dialect) -> Schema {
    let mut p =
        Parser { schema: Schema::new(dialect), views: HashSet::new(), partitions: HashSet::new() };
    p.schema.database = header_database(bytes);
    for statement in split(bytes, dialect) {
        let head = head_words(&statement.text);
        let first = head.first().map_or("", String::as_str);
        let handled = matches!(first, "CREATE" | "ALTER" | "COMMENT" | "USE") && {
            let text = decode(&statement.text);
            if matches!(text, Cow::Owned(_)) {
                p.schema.recoded.push(statement.line);
            }
            let database = p.schema.database.clone();
            let s = Stmt::new(&text, dialect, database.as_deref());
            p.statement(&s, statement.line)
        };
        if !handled {
            let kind = head.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
            *p.schema
                .skipped
                .entry(if kind.is_empty() { "?".into() } else { kind })
                .or_default() += 1;
        }
    }
    p.finish()
}

struct Parser {
    schema: Schema,
    views: HashSet<TableKey>,
    partitions: HashSet<TableKey>,
}

impl Parser {
    /// True if the statement was used.
    fn statement(&mut self, s: &Stmt, line: usize) -> bool {
        let mut i = 0;
        if s.is(0, "USE") {
            if let Some(name) = s.ident(1) {
                self.schema.database.get_or_insert(name);
            }
            return false;
        }
        if s.is(0, "COMMENT") && s.is(1, "ON") {
            return self.comment(s, line);
        }
        if s.is(0, "ALTER") && s.is(1, "TABLE") {
            return self.alter(s, line);
        }
        if !s.is(0, "CREATE") {
            return false;
        }
        i += 1;
        if s.is(i, "OR") && s.is(i + 1, "REPLACE") {
            i += 2;
        }
        if s.is(i, "GLOBAL") || s.is(i, "LOCAL") {
            i += 1;
        }
        if s.is(i, "TEMPORARY") || s.is(i, "TEMP") || s.is(i, "UNLOGGED") {
            i += 1;
        }
        if s.is(i, "TABLE") {
            return self.create_table(s, i + 1, line);
        }
        if s.is(i, "UNIQUE") && s.is(i + 1, "INDEX") {
            return self.unique_index(s, i + 2);
        }
        if s.is(i, "DATABASE") || (s.is(i, "SCHEMA") && s.dialect.is_mysql()) {
            let mut j = i + 1;
            if s.is(j, "IF") {
                j += 3;
            }
            if let Some(name) = s.ident(j) {
                self.schema.database.get_or_insert(name);
            }
            return false;
        }
        // CREATE [ALGORITHM=…] [DEFINER=…] [SQL SECURITY …] VIEW name AS …
        let view =
            (i..s.toks.len().min(40)).take_while(|&k| !s.is(k, "AS")).find(|&k| s.is(k, "VIEW"));
        if let Some(k) = view {
            let mut j = k + 1;
            if let Some(key) = s.table_name(&mut j) {
                self.views.insert(key);
            }
        }
        false
    }

    fn create_table(&mut self, s: &Stmt, mut i: usize, line: usize) -> bool {
        if s.is(i, "IF") && s.is(i + 1, "NOT") && s.is(i + 2, "EXISTS") {
            i += 3;
        }
        let Some(key) = s.table_name(&mut i) else {
            self.problem(line, ProblemKind::Statement, s.text(0, s.toks.len()));
            return true;
        };
        if s.is(i, "PARTITION") && s.is(i + 1, "OF") {
            self.partitions.insert(key);
            return true;
        }
        if !s.punct(i, b'(') {
            // CREATE TABLE … AS SELECT / LIKE: nothing to read.
            return false;
        }
        let close = s.close(i);
        let mut table = Table::new(key, line);
        for (a, b) in s.commas(i + 1, close) {
            if a == b {
                continue;
            }
            if s.starts_constraint(a) {
                if !constraint(s, a, b, &mut table) {
                    self.problem(line, ProblemKind::Constraint, s.text(a, b));
                }
            } else if !s.is(a, "LIKE") && !s.is(a, "PERIOD") && !column(s, a, b, &mut table) {
                self.problem(line, ProblemKind::Column, s.text(a, b));
            }
        }
        // Table options: COMMENT='…' (MySQL).
        let mut k = close + 1;
        while k < s.toks.len() {
            if s.is(k, "COMMENT") {
                let v = if s.punct(k + 1, b'=') { k + 2 } else { k + 1 };
                if let Some(text) = s.string(v) {
                    table.comment = Some(text.to_owned());
                }
            }
            k += 1;
        }
        self.schema.tables.insert(table.key.clone(), table);
        true
    }

    fn alter(&mut self, s: &Stmt, line: usize) -> bool {
        let mut i = 2;
        if s.is(i, "ONLY") {
            i += 1;
        }
        if s.is(i, "IF") && s.is(i + 1, "EXISTS") {
            i += 2;
        }
        let Some(key) = s.table_name(&mut i) else { return false };
        if s.punct(i, b'*') {
            i += 1;
        }
        let mut handled = false;
        for (a, b) in s.commas(i, s.toks.len()) {
            if s.is(a, "ATTACH") && s.is(a + 1, "PARTITION") {
                let mut j = a + 2;
                if let Some(part) = s.table_name(&mut j) {
                    self.partitions.insert(part);
                    handled = true;
                }
                continue;
            }
            if !s.is(a, "ADD") {
                continue;
            }
            let Some(found) = self.schema.find(&key).cloned() else {
                self.problem(line, ProblemKind::UnknownTable, s.text(0, s.toks.len()));
                return true;
            };
            let table = self.schema.tables.get_mut(&found).expect("found above");
            let c = a + 1;
            let ok = if s.starts_constraint(c) {
                constraint(s, c, b, table)
            } else {
                let c = if s.is(c, "COLUMN") { c + 1 } else { c };
                let c = if s.is(c, "IF") { c + 3 } else { c };
                column(s, c, b, table)
            };
            if !ok {
                self.problem(line, ProblemKind::Constraint, s.text(a, b));
            }
            handled = true;
        }
        handled
    }

    fn unique_index(&mut self, s: &Stmt, mut i: usize) -> bool {
        let Some(on) = (i..s.toks.len()).find(|&k| s.is(k, "ON")) else { return false };
        i = on + 1;
        if s.is(i, "ONLY") {
            i += 1;
        }
        let Some(key) = s.table_name(&mut i) else { return false };
        if s.is(i, "USING") {
            i += 2;
        }
        if !s.punct(i, b'(') {
            return false;
        }
        let close = s.close(i);
        // Expressions and partial indexes do not make plain columns unique.
        if (close + 1..s.toks.len()).any(|k| s.is(k, "WHERE")) {
            return false;
        }
        let Some(cols) = s.ident_list(i) else { return false };
        let Some(found) = self.schema.find(&key).cloned() else { return false };
        if let Some(t) = self.schema.tables.get_mut(&found) {
            t.uniques.push(cols);
        }
        true
    }

    fn comment(&mut self, s: &Stmt, line: usize) -> bool {
        let column = s.is(2, "COLUMN");
        if !column && !s.is(2, "TABLE") {
            return false;
        }
        let mut i = 3;
        let parts = s.name_parts(&mut i);
        if !s.is(i, "IS") {
            return false;
        }
        let text = s.string(i + 1).map(str::to_owned);
        let (table_parts, col) = if column {
            match parts.split_last() {
                Some((c, rest)) if !rest.is_empty() => (rest.to_vec(), Some(c.clone())),
                _ => return false,
            }
        } else {
            (parts, None)
        };
        let Some(key) = s.key_of(table_parts) else { return false };
        let Some(found) = self.schema.find(&key).cloned() else {
            self.problem(line, ProblemKind::UnknownTable, s.text(0, s.toks.len()));
            return true;
        };
        let table = self.schema.tables.get_mut(&found).expect("found above");
        match col {
            None => table.comment = text,
            Some(c) => {
                if let Some(c) = table.column_mut(&c) {
                    c.comment = text;
                }
            }
        }
        true
    }

    fn problem(&mut self, line: usize, kind: ProblemKind, text: String) {
        let mut text: String = text.chars().take(80).collect();
        if text.len() < 80 && text.ends_with(' ') {
            text.pop();
        }
        self.schema.problems.push(Problem { line, kind, text });
    }

    fn finish(mut self) -> Schema {
        // Partitions belong to their parent; older mysqldumps write a placeholder table for
        // each view and create the view later.
        self.schema.tables.retain(|k, _| !self.partitions.contains(k));
        self.schema.partitions = self.partitions.len();
        self.schema.tables.retain(|k, _| !self.views.contains(k));
        self.schema
    }
}

/// A column definition in `a..b`; false if it has no name.
fn column(s: &Stmt, a: usize, b: usize, table: &mut Table) -> bool {
    let Some(name) = s.ident(a) else { return false };
    let mut col = Column { name, ..Column::default() };
    let tail = (a + 1..b).find(|&k| s.depth0_keyword(k, a + 1)).unwrap_or(b);
    if tail > a + 1 {
        col.datatype = s.text(a + 1, tail);
    }
    let mut k = tail;
    let mut depth = 0usize;
    while k < b {
        if s.punct(k, b'(') {
            depth += 1;
        } else if s.punct(k, b')') {
            depth = depth.saturating_sub(1);
        }
        if depth > 0 {
            k += 1;
            continue;
        }
        if s.is(k, "NOT") && s.is(k + 1, "NULL") {
            col.not_null = true;
        } else if s.is(k, "PRIMARY") && s.is(k + 1, "KEY") {
            col.not_null = true;
            table.primary_key = vec![col.name.clone()];
        } else if s.is(k, "UNIQUE") {
            table.uniques.push(vec![col.name.clone()]);
        } else if s.is(k, "REFERENCES") {
            let mut j = k + 1;
            if let Some(target) = s.table_name(&mut j) {
                let target_columns =
                    if s.punct(j, b'(') { s.ident_list(j).unwrap_or_default() } else { vec![] };
                table.foreign_keys.push(ForeignKey {
                    columns: vec![col.name.clone()],
                    target,
                    target_columns,
                });
            }
        } else if s.is(k, "DEFAULT") {
            let end = (k + 1..b).find(|&j| j > k + 1 && s.depth0_keyword(j, k + 1)).unwrap_or(b);
            col.default = s.value(k + 1, end);
        } else if s.is(k, "COMMENT") {
            col.comment = s.string(k + 1).map(str::to_owned);
        } else if s.is(k, "AS") && s.punct(k + 1, b'(') {
            col.generated = true;
        }
        k += 1;
    }
    table.columns.push(col);
    true
}

/// A key or reference in `a..b`. True if read or not needed (indexes, checks).
fn constraint(s: &Stmt, a: usize, b: usize, table: &mut Table) -> bool {
    let mut i = a;
    if s.is(i, "CONSTRAINT") {
        i += 1;
        if !CONSTRAINT_START.iter().any(|k| s.is(i, k)) {
            i += 1;
        }
    }
    let first_paren = |from: usize| (from..b).find(|&k| s.punct(k, b'('));
    if s.is(i, "PRIMARY") && s.is(i + 1, "KEY") {
        let Some(cols) = first_paren(i + 2).and_then(|p| s.ident_list(p)) else {
            return false;
        };
        // Key columns are NOT NULL without saying so.
        for c in &cols {
            if let Some(c) = table.column_mut(c) {
                c.not_null = true;
            }
        }
        table.primary_key = cols;
        return true;
    }
    if s.is(i, "UNIQUE") {
        let Some(cols) = first_paren(i + 1).and_then(|p| s.ident_list(p)) else {
            return false;
        };
        table.uniques.push(cols);
        return true;
    }
    if s.is(i, "FOREIGN") && s.is(i + 1, "KEY") {
        let Some(open) = first_paren(i + 2) else { return false };
        let Some(columns) = s.ident_list(open) else { return false };
        let mut j = s.close(open) + 1;
        if !s.is(j, "REFERENCES") {
            return false;
        }
        j += 1;
        let Some(target) = s.table_name(&mut j) else { return false };
        let target_columns =
            if s.punct(j, b'(') { s.ident_list(j).unwrap_or_default() } else { vec![] };
        table.foreign_keys.push(ForeignKey { columns, target, target_columns });
        return true;
    }
    ["KEY", "INDEX", "FULLTEXT", "SPATIAL", "CHECK", "EXCLUDE"].iter().any(|k| s.is(i, k))
}

/// Words that start a constraint in a table's list (instead of a column).
const CONSTRAINT_START: [&str; 10] = [
    "CONSTRAINT",
    "PRIMARY",
    "UNIQUE",
    "FOREIGN",
    "KEY",
    "INDEX",
    "FULLTEXT",
    "SPATIAL",
    "CHECK",
    "EXCLUDE",
];

/// Words after a column's type that end it (and a default).
const COLUMN_KEYWORDS: [&str; 22] = [
    "NOT",
    "NULL",
    "DEFAULT",
    "PRIMARY",
    "UNIQUE",
    "REFERENCES",
    "CHECK",
    "COLLATE",
    "COMMENT",
    "AUTO_INCREMENT",
    "GENERATED",
    "CONSTRAINT",
    "CHARSET",
    "SRID",
    "INVISIBLE",
    "VISIBLE",
    "STORAGE",
    "COLUMN_FORMAT",
    "ENGINE_ATTRIBUTE",
    "SECONDARY_ENGINE_ATTRIBUTE",
    "IDENTITY",
    "KEY",
];

/// The first words of a statement in upper case, as long as only spaces separate them.
fn head_words(text: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < 4 && i < text.len() {
        let start = i;
        while i < text.len() && (text[i].is_ascii_alphanumeric() || text[i] == b'_') {
            i += 1;
        }
        if i == start {
            break;
        }
        out.push(String::from_utf8_lossy(&text[start..i]).to_ascii_uppercase());
        if i >= text.len() || !text[i].is_ascii_whitespace() {
            break;
        }
        while i < text.len() && text[i].is_ascii_whitespace() {
            i += 1;
        }
    }
    out
}

/// UTF-8, or else Windows-1252 (owned then).
fn decode(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(s) => Cow::Borrowed(s),
        Err(_) => Cow::Owned(encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned()),
    }
}

// -------------------------------------------------------------------------------------------
// Tokens

#[derive(Clone, PartialEq, Debug)]
enum Tok {
    Word,
    /// A quoted name, without its quotes.
    Quoted(String),
    /// A string literal's value.
    Str(String),
    Number,
    Punct(u8),
}

#[derive(Clone, PartialEq, Debug)]
struct Token {
    tok: Tok,
    start: usize,
    end: usize,
}

fn lex(src: &str, mysql: bool) -> Vec<Token> {
    let b = src.as_bytes();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0;
    let word_byte = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    while i < b.len() {
        let c = b[i];
        let start = i;
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' || c >= 0x80 {
            while i < b.len() && word_byte(b[i]) {
                i += 1;
            }
            out.push(Token { tok: Tok::Word, start, end: i });
            continue;
        }
        if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.') {
                i += 1;
            }
            out.push(Token { tok: Tok::Number, start, end: i });
            continue;
        }
        let tok = match c {
            b'\'' => {
                // E'…' (PostgreSQL) escapes with backslashes, as MySQL does in every string.
                let e_string = out.last().is_some_and(|t| {
                    t.tok == Tok::Word
                        && t.end == i
                        && src[t.start..t.end].eq_ignore_ascii_case("e")
                });
                let (value, end) = string(src, i, mysql || e_string);
                i = end;
                Tok::Str(value)
            }
            b'"' => {
                let (value, end) = string(src, i, mysql);
                i = end;
                if mysql { Tok::Str(value) } else { Tok::Quoted(value) }
            }
            b'`' => {
                let (value, end) = string(src, i, false);
                i = end;
                Tok::Quoted(value)
            }
            b'$' if !mysql => match dollar(src, i) {
                Some((value, end)) => {
                    i = end;
                    Tok::Str(value)
                }
                None => {
                    i += 1;
                    Tok::Punct(c)
                }
            },
            _ => {
                i += 1;
                Tok::Punct(c)
            }
        };
        out.push(Token { tok, start, end: i });
    }
    out
}

/// The value of the quoted text at `i` (doubled quotes, and backslash escapes if
/// `escapes`), and where it ends.
fn string(src: &str, i: usize, escapes: bool) -> (String, usize) {
    let b = src.as_bytes();
    let q = b[i];
    let mut out = String::new();
    let mut j = i + 1;
    let mut seg = j;
    while j < b.len() {
        if escapes && b[j] == b'\\' && j + 1 < b.len() {
            out.push_str(&src[seg..j]);
            let next = src[j + 1..].chars().next().expect("not at the end");
            out.push(match next {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '0' => '\0',
                'Z' => '\x1a',
                'b' => '\x08',
                other => other,
            });
            j += 1 + next.len_utf8();
            seg = j;
            continue;
        }
        if b[j] == q {
            out.push_str(&src[seg..j]);
            if b.get(j + 1) == Some(&q) {
                out.push(q as char);
                j += 2;
                seg = j;
                continue;
            }
            return (out, j + 1);
        }
        j += 1;
    }
    out.push_str(&src[seg..]);
    (out, b.len())
}

/// A `$tag$ … $tag$` string at `i`.
fn dollar(src: &str, i: usize) -> Option<(String, usize)> {
    let b = src.as_bytes();
    let mut j = i + 1;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
        j += 1;
    }
    if b.get(j) != Some(&b'$') || b.get(i + 1).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let tag = &src[i..=j];
    let body = j + 1;
    match src[body..].find(tag) {
        Some(p) => Some((src[body..body + p].to_owned(), body + p + tag.len())),
        None => Some((src[body..].to_owned(), src.len())),
    }
}

/// A statement's tokens with what the parser asks of them.
struct Stmt<'a> {
    src: &'a str,
    toks: Vec<Token>,
    dialect: Dialect,
    database: Option<&'a str>,
}

impl<'a> Stmt<'a> {
    /// `database`: the dump's database, so far as it is known (MySQL).
    fn new(src: &'a str, dialect: Dialect, database: Option<&'a str>) -> Self {
        Self { src, toks: lex(src, dialect.is_mysql()), dialect, database }
    }

    /// Whether the part at `i` of a table's list is a key, index or check rather than a
    /// column. `KEY`, `INDEX`, `FULLTEXT` and `SPATIAL` are reserved in MySQL only; in
    /// PostgreSQL `key text` is a column. `EXCLUDE` is a constraint only with `USING` or `(`.
    fn starts_constraint(&self, i: usize) -> bool {
        let always = ["CONSTRAINT", "PRIMARY", "UNIQUE", "FOREIGN", "CHECK"];
        let mysql = ["KEY", "INDEX", "FULLTEXT", "SPATIAL"];
        always.iter().any(|k| self.is(i, k))
            || (self.dialect.is_mysql() && mysql.iter().any(|k| self.is(i, k)))
            || (self.is(i, "EXCLUDE") && (self.is(i + 1, "USING") || self.punct(i + 1, b'(')))
    }

    fn word(&self, i: usize) -> Option<&str> {
        let t = self.toks.get(i)?;
        (t.tok == Tok::Word).then(|| &self.src[t.start..t.end])
    }

    fn is(&self, i: usize, keyword: &str) -> bool {
        self.word(i).is_some_and(|w| w.eq_ignore_ascii_case(keyword))
    }

    fn punct(&self, i: usize, c: u8) -> bool {
        self.toks.get(i).is_some_and(|t| t.tok == Tok::Punct(c))
    }

    fn string(&self, i: usize) -> Option<&str> {
        match &self.toks.get(i)?.tok {
            Tok::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A name: unquoted (PostgreSQL folds it to lower case) or quoted.
    fn ident(&self, i: usize) -> Option<String> {
        match &self.toks.get(i)?.tok {
            Tok::Word => {
                let w = self.word(i)?;
                Some(if self.dialect.is_mysql() { w.to_owned() } else { w.to_lowercase() })
            }
            Tok::Quoted(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// `a.b.c` from `i`; moves `i` past it.
    fn name_parts(&self, i: &mut usize) -> Vec<String> {
        let mut parts = Vec::new();
        let Some(first) = self.ident(*i) else { return parts };
        parts.push(first);
        *i += 1;
        while self.punct(*i, b'.')
            && let Some(next) = self.ident(*i + 1)
        {
            parts.push(next);
            *i += 2;
        }
        parts
    }

    fn key_of(&self, mut parts: Vec<String>) -> Option<TableKey> {
        let name = parts.pop()?;
        Some(if self.dialect.is_mysql() {
            // A dump holds one database: its own name in front is left out, another one
            // stays (a reference into another database is not in the dump).
            match parts.pop() {
                Some(db) if self.database.is_some_and(|own| !own.eq_ignore_ascii_case(&db)) => {
                    TableKey::new(db, name)
                }
                _ => TableKey::new("", name),
            }
        } else {
            TableKey::new(parts.pop().unwrap_or_else(|| "public".to_owned()), name)
        })
    }

    fn table_name(&self, i: &mut usize) -> Option<TableKey> {
        let parts = self.name_parts(i);
        self.key_of(parts)
    }

    /// The `)` that closes the `(` at `open` (or the end).
    fn close(&self, open: usize) -> usize {
        let mut depth = 0usize;
        for k in open..self.toks.len() {
            if self.punct(k, b'(') {
                depth += 1;
            } else if self.punct(k, b')') {
                depth -= 1;
                if depth == 0 {
                    return k;
                }
            }
        }
        self.toks.len()
    }

    /// The parts of `from..to` between commas outside parentheses.
    fn commas(&self, from: usize, to: usize) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let (mut depth, mut start) = (0usize, from);
        for k in from..to {
            if self.punct(k, b'(') {
                depth += 1;
            } else if self.punct(k, b')') {
                depth = depth.saturating_sub(1);
            } else if depth == 0 && self.punct(k, b',') {
                out.push((start, k));
                start = k + 1;
            }
        }
        out.push((start, to));
        out
    }

    /// Whether token `k` ends a column's type or default: a column keyword outside
    /// parentheses (counted from `from`).
    fn depth0_keyword(&self, k: usize, from: usize) -> bool {
        let depth = (from..k).fold(0i32, |d, j| {
            if self.punct(j, b'(') {
                d + 1
            } else if self.punct(j, b')') {
                d - 1
            } else {
                d
            }
        });
        if depth > 0 {
            return false;
        }
        COLUMN_KEYWORDS.iter().any(|w| self.is(k, w))
            || (self.is(k, "CHARACTER") && self.is(k + 1, "SET"))
            || (self.is(k, "ON") && self.is(k + 1, "UPDATE"))
            || (self.is(k, "AS") && self.punct(k + 1, b'('))
    }

    /// The names in the parentheses at `open`; `None` if one is an expression. Prefix
    /// lengths (`email(10)`), sort orders and operator classes are left out.
    fn ident_list(&self, open: usize) -> Option<Vec<String>> {
        let close = self.close(open);
        let mut out = Vec::new();
        for (a, b) in self.commas(open + 1, close) {
            if a == b {
                continue;
            }
            let name = self.ident(a)?;
            let rest_ok = (a + 1..b).all(|k| {
                self.punct(k, b'(')
                    || self.punct(k, b')')
                    || matches!(self.toks[k].tok, Tok::Number | Tok::Word)
            });
            if !rest_ok
                || self.punct(a + 1, b'(')
                    && !self.toks.get(a + 2).is_some_and(|t| t.tok == Tok::Number)
            {
                return None;
            }
            out.push(name);
        }
        Some(out)
    }

    /// The source of `from..to` with runs of white space as one space.
    fn text(&self, from: usize, to: usize) -> String {
        if from >= to || from >= self.toks.len() {
            return String::new();
        }
        let to = to.min(self.toks.len());
        let raw = &self.src[self.toks[from].start..self.toks[to - 1].end];
        raw.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// A default: a string's value (also with an introducer such as `_utf8mb4` or a cast
    /// after it), `None` for `NULL`, else the expression as written.
    fn value(&self, from: usize, to: usize) -> Option<String> {
        if from >= to {
            return None;
        }
        if self.is(from, "NULL") && to == from + 1 {
            return None;
        }
        let at = if self.word(from).is_some() && self.string(from + 1).is_some() {
            from + 1
        } else {
            from
        };
        if let Some(s) = self.string(at) {
            let cast = self.punct(at + 1, b':') && self.punct(at + 2, b':');
            if at + 1 == to || cast {
                return Some(s.to_owned());
            }
        }
        Some(self.text(from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn table<'a>(s: &'a Schema, schema: &str, name: &str) -> &'a Table {
        &s.tables[&TableKey::new(schema, name)]
    }

    #[test]
    fn mysql_tables_with_inline_keys() {
        let sql = "CREATE TABLE `gate` (\n  `flughafen_id` int NOT NULL,\n  `nummer` varchar(8) \
                   CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL DEFAULT 'A1' COMMENT 'Gate, \
                   z. B. ''B12''',\n  `pos` geometry  SRID 0 ,\n  `art` enum('a,b','c') DEFAULT \
                   NULL,\n  `dauer` int GENERATED ALWAYS AS (timestampdiff(MINUTE,`a`,`b`)) \
                   VIRTUAL,\n  PRIMARY KEY (`flughafen_id`,`nummer`),\n  UNIQUE KEY `u` \
                   (`nummer`(4)),\n  KEY `fk` (`flughafen_id`),\n  CONSTRAINT `fk_gate` FOREIGN \
                   KEY (`flughafen_id`) REFERENCES `flughafen` (`id`) ON DELETE CASCADE\n) \
                   ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COMMENT='Gates';";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        assert_eq!(s.problems, []);
        let t = table(&s, "", "gate");
        let cols: Vec<_> = t.columns.iter().map(|c| c.datatype.as_str()).collect();
        assert_eq!(cols, ["int", "varchar(8)", "geometry", "enum('a,b','c')", "int"]);
        assert_eq!(t.columns[1].default.as_deref(), Some("A1"));
        assert_eq!(t.columns[1].comment.as_deref(), Some("Gate, z. B. 'B12'"));
        assert!(t.columns[1].not_null && !t.columns[2].not_null);
        assert_eq!(t.columns[3].default, None);
        assert!(t.columns[4].generated && !t.columns[0].generated);
        assert_eq!(t.primary_key, ["flughafen_id", "nummer"]);
        assert_eq!(t.uniques, [vec!["nummer".to_owned()]]);
        assert_eq!(
            t.foreign_keys,
            [ForeignKey {
                columns: vec!["flughafen_id".into()],
                target: TableKey::new("", "flughafen"),
                target_columns: vec!["id".into()],
            }]
        );
        assert_eq!(t.comment.as_deref(), Some("Gates"));
    }

    #[test]
    fn postgres_tables_keys_by_alter_and_comments() {
        let sql = "CREATE TABLE public.flug (\n    id integer NOT NULL,\n    preis numeric(8,2) \
                   DEFAULT 0.00,\n    status public.ticket_status DEFAULT \
                   'reserviert'::public.ticket_status NOT NULL,\n    start_zeit timestamp without \
                   time zone,\n    dauer_min integer GENERATED ALWAYS AS (1) STORED,\n    n \
                   integer GENERATED BY DEFAULT AS IDENTITY,\n    CONSTRAINT c CHECK ((preis > \
                   (0)::numeric))\n);\nCREATE TABLE fracht.\"Fracht Stück\" (\"Sendungs Nr\" \
                   integer, flug_id integer);\nALTER TABLE ONLY public.flug ADD CONSTRAINT \
                   flug_pkey PRIMARY KEY (id);\nALTER TABLE ONLY fracht.\"Fracht Stück\"\n    ADD \
                   CONSTRAINT fk FOREIGN KEY (flug_id) REFERENCES public.flug(id) ON DELETE SET \
                   NULL;\nALTER TABLE public.flug OWNER TO x;\nCREATE UNIQUE INDEX u ON ONLY \
                   public.flug USING btree (start_zeit);\nCREATE UNIQUE INDEX v ON public.flug \
                   (lower(status));\nCOMMENT ON TABLE public.flug IS 'Ein Flug';\nCOMMENT ON \
                   COLUMN fracht.\"Fracht Stück\".\"Sendungs Nr\" IS 'Nr.';\n";
        let s = parse(sql.as_bytes(), Dialect::Postgres);
        assert_eq!(s.problems, []);
        let f = table(&s, "public", "flug");
        assert_eq!(f.columns[1].datatype, "numeric(8,2)");
        assert_eq!(f.columns[1].default.as_deref(), Some("0.00"));
        assert_eq!(f.columns[2].default.as_deref(), Some("reserviert"));
        assert!(f.columns[2].not_null);
        assert_eq!(f.columns[3].datatype, "timestamp without time zone");
        assert!(f.columns[4].generated && !f.columns[5].generated);
        assert_eq!(f.columns.len(), 6);
        assert_eq!(f.primary_key, ["id"]);
        assert_eq!(f.uniques, [vec!["start_zeit".to_owned()]]);
        assert_eq!(f.comment.as_deref(), Some("Ein Flug"));
        let fs = table(&s, "fracht", "Fracht Stück");
        assert_eq!(fs.foreign_keys[0].target, TableKey::new("public", "flug"));
        assert_eq!(fs.columns[0].comment.as_deref(), Some("Nr."));
        assert_eq!(s.skipped.get("ALTER TABLE"), Some(&1));
        assert_eq!(s.skipped.get("CREATE UNIQUE"), Some(&1));
        assert_eq!(s.schemas(), ["public", "fracht"]);
    }

    #[test]
    fn inline_references_partitions_and_view_placeholders() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY, t int NOT NULL REFERENCES t (id));\n\
                   CREATE TABLE public.pay (id int) PARTITION BY RANGE (id);\n\
                   CREATE TABLE public.pay_1 (id int);\n\
                   ALTER TABLE ONLY public.pay ATTACH PARTITION public.pay_1 FOR VALUES FROM (1) TO (2);\n\
                   CREATE TABLE public.pay_2 PARTITION OF public.pay FOR VALUES FROM (2) TO (3);\n\
                   CREATE TABLE v (a int);\nCREATE VIEW public.v AS SELECT 1;";
        let s = parse(sql.as_bytes(), Dialect::Postgres);
        let keys: Vec<_> = s.tables.keys().map(|k| k.to_string()).collect();
        assert_eq!(keys, ["p", "pay"]);
        assert_eq!(s.partitions, 2);
        let p = table(&s, "public", "p");
        assert_eq!(p.primary_key, ["id"]);
        assert_eq!(p.foreign_keys[0].target, TableKey::new("public", "t"));
    }

    #[test]
    fn key_and_index_are_column_names_in_postgresql() {
        let sql = "CREATE TABLE public.settings (id integer NOT NULL, key text NOT NULL, \
                   index integer, value text, EXCLUDE USING gist (id WITH =));";
        let s = parse(sql.as_bytes(), Dialect::Postgres);
        let cols: Vec<_> =
            table(&s, "public", "settings").columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(cols, ["id", "key", "index", "value"]);
        assert_eq!(s.problems, []);
    }

    #[test]
    fn primary_key_columns_are_not_null() {
        let sql = "CREATE TABLE w (p_id int, nr int, x int, PRIMARY KEY (p_id, nr));";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let nn: Vec<_> = table(&s, "", "w").columns.iter().map(|c| c.not_null).collect();
        assert_eq!(nn, [true, true, false]);
    }

    #[test]
    fn references_into_another_mysql_database_keep_it() {
        let sql = "-- Host: x    Database: shop\nCREATE TABLE `users` (id int);\n\
                   CREATE TABLE `orders` (id int, u int, a int, \
                   FOREIGN KEY (u) REFERENCES `auth`.`users` (id), \
                   FOREIGN KEY (a) REFERENCES `shop`.`users` (id));";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let targets: Vec<_> =
            table(&s, "", "orders").foreign_keys.iter().map(|f| f.target.to_string()).collect();
        assert_eq!(targets, ["auth.users", "users"]);
    }

    #[test]
    fn the_database_name_and_what_is_skipped() {
        let sql = "CREATE DATABASE /*!32312 IF NOT EXISTS*/ `shop`;\nUSE `other`;\nSET @a = 1;\n\
                   INSERT INTO `x` VALUES (1);\nINSERT INTO `x` VALUES (2);\nLOCK TABLES `x` WRITE;";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        assert_eq!(s.database.as_deref(), Some("shop"));
        let skipped: Vec<_> = s.skipped.iter().map(|(k, v)| format!("{k}={v}")).collect();
        assert_eq!(
            skipped,
            ["CREATE DATABASE=1", "INSERT INTO=2", "LOCK TABLES=1", "SET=1", "USE=1"]
        );
    }

    #[test]
    fn what_cannot_be_read_costs_only_itself() {
        let sql = "CREATE TABLE a (id int, FOREIGN KEY (id), 'x' int);\nCREATE TABLE b (id int);\n\
                   ALTER TABLE c ADD PRIMARY KEY (id);";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        assert_eq!(s.tables.len(), 2);
        let kinds: Vec<_> = s.problems.iter().map(|p| (p.line, p.kind)).collect();
        assert_eq!(
            kinds,
            [
                (1, ProblemKind::Constraint),
                (1, ProblemKind::Column),
                (3, ProblemKind::UnknownTable)
            ]
        );
    }

    #[test]
    fn statements_that_are_not_utf8_are_read_as_windows_1252() {
        let sql = b"CREATE TABLE `Stra\xdfe` (`gr\xf6\xdfe` int);";
        let s = parse(sql, Dialect::MySql);
        assert_eq!(table(&s, "", "Straße").columns[0].name, "größe");
        assert_eq!(s.recoded, [1]);
    }
}
