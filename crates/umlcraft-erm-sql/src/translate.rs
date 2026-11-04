//! Types and conditions from one dialect in the other, for [`crate::ddl`]: what a PostgreSQL
//! dump writes as `character varying(50)` or `timestamp without time zone` is `varchar(50)`
//! and `datetime` in MySQL, MySQL's `int unsigned` and `longtext` are `bigint` and `text` in
//! PostgreSQL. A type the other dialect knows by the same name stays as it is.
//!
//! Without a source dialect (a diagram drawn by hand) only names the target does not know
//! are translated; `timestamp` and `real`, which both know but mean something else by, stay.

use crate::dialect::Dialect;

/// `datatype` (with its length, as [`crate::ddl`] writes it) for `to`, coming from `from`.
pub fn datatype(datatype: &str, from: Option<Dialect>, to: Dialect) -> String {
    let t = datatype.trim();
    if from.is_some_and(|f| f.is_mysql() == to.is_mysql()) {
        return t.to_owned();
    }
    let known = from.is_some();
    let (words, args) = split(t);
    let arg = |base: &str| match &args {
        Some(a) => format!("{base}({a})"),
        None => base.to_owned(),
    };
    let has = |w: &str| words.iter().any(|x| x == w);
    let base: Vec<&str> = words
        .iter()
        .map(String::as_str)
        .filter(|w| !matches!(*w, "unsigned" | "signed" | "zerofill"))
        .collect();
    let base = base.join(" ");
    // Drawn by hand: a name both dialects know stays.
    const BOTH: [&str; 16] = [
        "int",
        "integer",
        "smallint",
        "bigint",
        "decimal",
        "numeric",
        "real",
        "float",
        "char",
        "varchar",
        "text",
        "boolean",
        "date",
        "time",
        "timestamp",
        "json",
    ];
    if !known && !has("unsigned") && BOTH.contains(&base.as_str()) {
        return t.to_owned();
    }
    if to.is_mysql() {
        if base.ends_with("[]") {
            return "json".to_owned();
        }
        let out = match base.as_str() {
            "character varying" | "varchar" if args.is_none() => "varchar(255)".to_owned(),
            "character varying" => arg("varchar"),
            "character" | "bpchar" => arg("char"),
            "timestamp without time zone" => arg("datetime"),
            "timestamp" if known => arg("datetime"),
            "timestamp with time zone" | "timestamptz" => arg("timestamp"),
            "time without time zone" | "time with time zone" | "timetz" => arg("time"),
            "integer" | "int4" => "int".to_owned(),
            "int8" | "bigserial" | "serial8" => "bigint".to_owned(),
            "int2" | "smallserial" | "serial2" => "smallint".to_owned(),
            "serial" | "serial4" => "int".to_owned(),
            "double precision" | "float8" => "double".to_owned(),
            "float4" => "float".to_owned(),
            "real" if known => "float".to_owned(),
            "bool" => "boolean".to_owned(),
            "bytea" => "longblob".to_owned(),
            "money" => "decimal(19,2)".to_owned(),
            "uuid" => "char(36)".to_owned(),
            "jsonb" => "json".to_owned(),
            "inet" | "cidr" => "varchar(45)".to_owned(),
            "macaddr" | "macaddr8" => "varchar(23)".to_owned(),
            "interval" => "varchar(64)".to_owned(),
            "xml" | "citext" | "name" | "tsvector" => "text".to_owned(),
            "numeric" => arg("decimal"),
            _ => t.to_owned(),
        };
        return out;
    }
    let unsigned = has("unsigned");
    let out = match base.as_str() {
        "tinyint" if args.as_deref() == Some("1") => "boolean",
        "tinyint" => "smallint",
        "smallint" if unsigned => "integer",
        "smallint" => "smallint",
        "mediumint" => "integer",
        "int" | "integer" if unsigned => "bigint",
        "int" | "integer" => "integer",
        "bigint" if unsigned => "numeric(20)",
        "bigint" => "bigint",
        "datetime" => return arg("timestamp"),
        "year" => "smallint",
        "double" => "double precision",
        "real" if known => "double precision",
        "float" if known => "real",
        "decimal" | "dec" | "fixed" | "numeric" => return arg("numeric"),
        "tinytext" | "mediumtext" | "longtext" => "text",
        "tinyblob" | "blob" | "mediumblob" | "longblob" | "binary" | "varbinary" => "bytea",
        "set" => "text",
        _ if unsigned => return arg(&base),
        _ => return t.to_owned(),
    };
    out.to_owned()
}

/// The lower-case words of a type without what its parentheses hold, and that: `int(10)
/// unsigned` → `[int, unsigned]`, `10`.
fn split(t: &str) -> (Vec<String>, Option<String>) {
    let Some(open) = t.find('(') else {
        return (t.split_whitespace().map(str::to_ascii_lowercase).collect(), None);
    };
    let close = t.rfind(')').filter(|&c| c > open).unwrap_or(t.len());
    let args = t[open + 1..close.min(t.len())].trim().to_owned();
    let rest = format!("{} {}", &t[..open], t.get(close + 1..).unwrap_or(""));
    (rest.split_whitespace().map(str::to_ascii_lowercase).collect(), Some(args))
}

/// The values of MySQL's `enum('a','b')`, if `datatype` is one.
pub fn enum_values(datatype: &str) -> Option<Vec<String>> {
    let t = datatype.trim();
    let lower = t.to_ascii_lowercase();
    if !lower.starts_with("enum") {
        return None;
    }
    let open = t.find('(')?;
    let close = t.rfind(')')?;
    let mut values = Vec::new();
    let inner: Vec<char> = t[open + 1..close].chars().collect();
    let mut k = 0;
    while k < inner.len() {
        if inner[k] != '\'' {
            k += 1;
            continue;
        }
        let mut v = String::new();
        k += 1;
        while k < inner.len() {
            match inner[k] {
                '\'' if inner.get(k + 1) == Some(&'\'') => {
                    v.push('\'');
                    k += 2;
                }
                '\'' => break,
                '\\' if k + 1 < inner.len() => {
                    v.push(inner[k + 1]);
                    k += 2;
                }
                c => {
                    v.push(c);
                    k += 1;
                }
            }
        }
        values.push(v);
        k += 1;
    }
    Some(values)
}

/// A condition (`CHECK`) for `to`, or `None` if it holds what `to` cannot read.
pub fn condition(text: &str, to: Dialect) -> Option<String> {
    if to.is_mysql() {
        let mut s = any_array(text);
        s = without_casts(&s);
        s = s.replace("!~~*", "NOT LIKE").replace("~~*", "LIKE");
        s = s.replace("!~~", "NOT LIKE").replace("~~", "LIKE");
        let foreign = ["::", "ARRAY[", "~", "E'", "$$"];
        (!foreign.iter().any(|f| s.contains(f))).then_some(s)
    } else {
        // MySQL quotes names with backticks and marks strings with their character set.
        let mut s = String::new();
        let mut chars = text.chars().peekable();
        let mut in_string = false;
        while let Some(c) = chars.next() {
            match c {
                '\'' => {
                    in_string = !in_string;
                    s.push(c);
                }
                '`' if !in_string => {
                    let name: String = chars.by_ref().take_while(|&c| c != '`').collect();
                    let plain = name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit());
                    if plain {
                        s.push_str(&name);
                    } else {
                        s.push_str(&format!("\"{}\"", name.replace('"', "\"\"")));
                    }
                }
                '_' if !in_string && !s.ends_with(|p: char| p.is_alphanumeric() || p == '_') => {
                    // `_utf8mb4'x'`: the introducer goes, the string stays.
                    let word: String =
                        chars.clone().take_while(|c| c.is_ascii_alphanumeric()).collect();
                    if !word.is_empty() && chars.clone().nth(word.len()) == Some('\'') {
                        for _ in 0..word.len() {
                            chars.next();
                        }
                    } else {
                        s.push(c);
                    }
                }
                _ => s.push(c),
            }
        }
        Some(s.replace("\\'", "''"))
    }
}

/// `x = ANY ((ARRAY['a'::text, 'b'::text])::text[])` → `x IN ('a'::text, 'b'::text)`.
fn any_array(text: &str) -> String {
    let mut s = text.to_owned();
    while let Some(at) = s.find("= ANY (") {
        let open = at + "= ANY ".len();
        let Some(close) = matching(&s, open) else { break };
        let inside = &s[open..=close];
        let Some(a) = inside.find("ARRAY[") else { break };
        // The array's own end, not the one of a cast to `text[]` after it.
        let Some(b) = inside[a..].find(']').map(|b| a + b) else { break };
        let items = inside[a + "ARRAY[".len()..b].to_owned();
        s = format!("{}IN ({items}){}", &s[..at], &s[close + 1..]);
    }
    s
}

/// The `)` that closes the `(` at byte `open`, outside strings.
fn matching(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    for (k, c) in s.char_indices().skip_while(|&(k, _)| k < open) {
        match c {
            '\'' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
    }
    None
}

/// Without PostgreSQL's casts: `(klasse)::text` → `(klasse)`, `'a'::character varying` → `'a'`.
fn without_casts(text: &str) -> String {
    const MORE: [&str; 6] = ["varying", "precision", "without", "with", "time", "zone"];
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("::") {
        out.push_str(&rest[..at]);
        rest = &rest[at + 2..];
        loop {
            let word = rest
                .find(|c: char| {
                    !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '"' | '[' | ']'))
                })
                .unwrap_or(rest.len());
            rest = &rest[word..];
            let trimmed = rest.trim_start();
            match MORE.iter().find(|m| trimmed.starts_with(*m)) {
                Some(_) if word > 0 => rest = trimmed,
                _ => break,
            }
        }
        // `(…)` of a type such as `character varying(5)`.
        if rest.starts_with('(')
            && let Some(close) = matching(rest, 0)
            && rest[1..close].chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ')
        {
            rest = &rest[close + 1..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PG: Option<Dialect> = Some(Dialect::Postgres);
    const MY: Option<Dialect> = Some(Dialect::MySql);

    #[test]
    fn postgresql_types_in_mysql() {
        let t = |d: &str| datatype(d, PG, Dialect::MySql);
        assert_eq!(t("character varying(120)"), "varchar(120)");
        assert_eq!(t("character(3)"), "char(3)");
        assert_eq!(t("timestamp without time zone"), "datetime");
        assert_eq!(t("timestamp(3) without time zone"), "datetime(3)");
        assert_eq!(t("timestamp"), "datetime");
        assert_eq!(t("integer"), "int");
        assert_eq!(t("numeric(8,2)"), "decimal(8,2)");
        assert_eq!(t("bytea"), "longblob");
        assert_eq!(t("text[]"), "json");
        assert_eq!(t("text"), "text");
        assert_eq!(t("smallint"), "smallint");
        // Unknown in the source: the name the target does not know alone.
        assert_eq!(datatype("timestamp", None, Dialect::MySql), "timestamp");
        assert_eq!(datatype("character varying(5)", None, Dialect::MySql), "varchar(5)");
        // Same family: as it is.
        assert_eq!(datatype("int(10) unsigned", MY, Dialect::MariaDb), "int(10) unsigned");
    }

    #[test]
    fn mysql_types_in_postgresql() {
        let t = |d: &str| datatype(d, MY, Dialect::Postgres);
        assert_eq!(t("int(10) unsigned"), "bigint");
        assert_eq!(t("int"), "integer");
        assert_eq!(t("tinyint(1)"), "boolean");
        assert_eq!(t("tinyint unsigned"), "smallint");
        assert_eq!(t("datetime(6)"), "timestamp(6)");
        assert_eq!(t("decimal(8,2) unsigned"), "numeric(8,2)");
        assert_eq!(t("longtext"), "text");
        assert_eq!(t("varbinary(64)"), "bytea");
        assert_eq!(t("varchar(40)"), "varchar(40)");
        assert_eq!(t("double"), "double precision");
    }

    #[test]
    fn enum_values_are_read() {
        assert_eq!(enum_values("enum('a','b''c')").unwrap(), ["a", "b'c"]);
        assert_eq!(enum_values("ENUM('x')").unwrap(), ["x"]);
        assert_eq!(enum_values("varchar(5)"), None);
    }

    #[test]
    fn conditions_move_between_the_dialects() {
        let my = |c: &str| condition(c, Dialect::MySql);
        assert_eq!(my("sitze > 0").as_deref(), Some("sitze > 0"));
        assert_eq!(my("(VALUE)::text ~~ '%@%'::text").as_deref(), Some("(VALUE) LIKE '%@%'"));
        let any = "(klasse)::text = ANY ((ARRAY['economy'::character varying, \
                   'first'::character varying])::text[])";
        assert_eq!(my(any).as_deref(), Some("(klasse) IN ('economy', 'first')"));
        assert_eq!(my("x ~ '^a'"), None);
        let pg = |c: &str| condition(c, Dialect::Postgres);
        assert_eq!(pg("`a` > `b`").as_deref(), Some("a > b"));
        assert_eq!(pg("`Preis` >= 0").as_deref(), Some("\"Preis\" >= 0"));
        assert_eq!(pg("`k` in (_utf8mb4'x',_utf8mb4'y')").as_deref(), Some("k in ('x','y')"));
        assert_eq!(pg("v_x = 1").as_deref(), Some("v_x = 1"));
    }
}
