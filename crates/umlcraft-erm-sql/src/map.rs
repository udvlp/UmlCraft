//! The tables of a dump as a Chen diagram, by the rules S1–S11 in their order (S1,
//! partitions, is done by the parser; S2 same foreign key twice → once; S3–S11 below).
//!
//! - A junction table (its primary key is the columns of two or more foreign keys, nothing
//!   else refers to it) becomes a relationship between the tables it joins (S3).
//! - A table whose key is a foreign key plus columns of its own becomes a weak entity with an
//!   identifying relationship (S4); every other table an entity (S5).
//! - Columns become attributes, except those of foreign keys: they are the relationships
//!   (S6, S7). Cardinalities are read look-across: `[1,1]` at the referenced table says that
//!   each row of the referencing one has exactly one. The second end of a junction table's
//!   relationship is `[0,m]` (`[0,n]` to `[0,m]`).

use std::collections::{BTreeMap, HashMap, HashSet};

use umlcraft_erm::{
    Caption, Cardinality, Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Max, Node,
    NodeKind, Props, Rect, Style, layout, size,
};

use crate::dialect::Dialect;
use crate::ir::{ForeignKey, Problem, Schema, Table, TableKey};

/// Which attributes the import draws (O6: large schemas get very wide with all of them).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Attributes {
    #[default]
    All,
    /// Only primary and partial keys.
    Keys,
    None,
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Options {
    pub attributes: Attributes,
    /// The schemas to take (PostgreSQL); `None` takes all.
    pub schemas: Option<Vec<String>>,
    /// The dump's file name, for the diagram's name (if the dump names no database) and
    /// origin.
    pub file_name: Option<String>,
}

/// The diagram and what the import found.
#[derive(Clone, PartialEq, Debug)]
pub struct Import {
    pub diagram: Diagram,
    pub report: Report,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Report {
    pub dialect: Dialect,
    /// Tables taken (after the choice of schemas), junction tables included.
    pub tables: usize,
    pub entities: usize,
    pub relationships: usize,
    pub attributes: usize,
    pub hints: Vec<Hint>,
    /// Statements not needed, by their first words.
    pub skipped: BTreeMap<String, usize>,
    pub problems: Vec<Problem>,
}

impl Report {
    pub fn skipped_total(&self) -> usize {
        self.skipped.values().sum()
    }
}

/// Something worth knowing about the result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Hint {
    /// The table has no primary key: its entity has no key attribute.
    NoPrimaryKey { element: Id, table: String },
    /// A foreign key refers to a table that is not in the dump (or not in the schemas
    /// taken): a placeholder entity stands for it.
    NotInDump { element: Id, table: String },
    /// The statement on this line was not UTF-8 and was read as Windows-1252.
    Recoded { line: usize },
}

impl Hint {
    /// The element the hint is about.
    pub fn element(&self) -> Option<&Id> {
        match self {
            Self::NoPrimaryKey { element, .. } | Self::NotInDump { element, .. } => Some(element),
            Self::Recoded { .. } => None,
        }
    }
}

/// The comment a placeholder entity gets.
pub const PLACEHOLDER_COMMENT: &str = "not in the dump";

/// Maps and lays out: the finished diagram.
pub fn import(schema: &Schema, options: &Options) -> Import {
    let mut result = map(schema, options);
    layout::arrange(&mut result.diagram);
    result
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Entity,
    Weak,
    Junction,
}

/// Maps without laying out: the nodes have their sizes, all at (0, 0).
pub fn map(schema: &Schema, options: &Options) -> Import {
    let tables: Vec<&Table> = schema
        .tables
        .values()
        .filter(|t| options.schemas.as_ref().is_none_or(|s| s.contains(&t.key.schema)))
        .collect();
    let taken: HashSet<&TableKey> = tables.iter().map(|t| &t.key).collect();
    // S2: the same foreign key twice counts once. Targets are resolved to the tables taken.
    let resolve = |key: &TableKey| -> TableKey {
        schema.find(key).filter(|k| taken.contains(k)).cloned().unwrap_or_else(|| key.clone())
    };
    let fks: Vec<Vec<ForeignKey>> = tables
        .iter()
        .map(|t| {
            let mut seen = Vec::new();
            for fk in &t.foreign_keys {
                let fk = ForeignKey { target: resolve(&fk.target), ..fk.clone() };
                if !seen.contains(&fk) {
                    seen.push(fk);
                }
            }
            seen
        })
        .collect();
    let referenced: HashSet<&TableKey> = fks.iter().flatten().map(|fk| &fk.target).collect();
    let classes: Vec<Class> = tables
        .iter()
        .zip(&fks)
        .map(|(t, fks)| classify(t, fks, referenced.contains(&t.key)))
        .collect();

    let mut m = Mapper {
        d: Diagram::new(diagram_name(schema, options)),
        entities: HashMap::new(),
        hints: Vec::new(),
        attributes: options.attributes,
    };
    m.d.meta.origin = Some(match &options.file_name {
        Some(f) => format!("SQL: {f} ({})", schema.dialect),
        None => format!("SQL ({})", schema.dialect),
    });
    for (t, class) in tables.iter().zip(&classes) {
        if *class == Class::Junction {
            continue;
        }
        let id = m.entity(&t.key, *class == Class::Weak, t.comment.clone());
        if t.primary_key.is_empty() {
            let table = t.key.to_string();
            m.hints.push(Hint::NoPrimaryKey { element: id, table });
        }
    }
    for ((t, fks), class) in tables.iter().zip(&fks).zip(&classes) {
        let fk_cols: HashSet<&str> =
            fks.iter().flat_map(|fk| fk.columns.iter().map(String::as_str)).collect();
        let own: Vec<&str> =
            t.columns.iter().map(|c| c.name.as_str()).filter(|c| !fk_cols.contains(c)).collect();
        if *class == Class::Junction {
            let r = m.node(
                NodeKind::relationship(),
                &t.key.to_string(),
                Props {
                    physical_name: Some(t.key.to_string()),
                    comment: t.comment.clone(),
                    ..Props::default()
                },
            );
            for (k, fk) in fks.iter().enumerate() {
                let role = (fks.iter().filter(|f| f.target == fk.target).count() > 1)
                    .then(|| relationship_name(&fk.columns, ""));
                let target = m.target(&fk.target);
                // `[0,n]` to `[0,m]`; a third and further ends are `n` again.
                let max = if k == 1 { Max::ManyM } else { Max::Many };
                m.join(&r, &target, Cardinality::new(0, max), role, Line::Solid);
            }
            m.attributes(&r, t, &own, &[], &[]);
            continue;
        }
        let owner = m.entities[&t.key].clone();
        let in_pk: Vec<&ForeignKey> = if *class == Class::Weak { in_key(t, fks) } else { vec![] };
        if *class == Class::Weak {
            let id_cols: HashSet<&str> =
                in_pk.iter().flat_map(|fk| fk.columns.iter().map(String::as_str)).collect();
            let partial: Vec<&str> =
                t.primary_key.iter().map(String::as_str).filter(|c| !id_cols.contains(c)).collect();
            m.attributes(&owner, t, &own, &[], &partial);
        } else {
            let keys: Vec<&str> =
                t.primary_key.iter().map(String::as_str).filter(|c| !fk_cols.contains(c)).collect();
            m.attributes(&owner, t, &own, &keys, &[]);
        }
        for fk in fks {
            let identifying = in_pk.contains(&fk);
            let other = m.target(&fk.target);
            let r = m.node(
                NodeKind::Relationship { identifying },
                &relationship_name(&fk.columns, &fk.target.name),
                Props::default(),
            );
            let nullable = fk.columns.iter().any(|c| !t.column(c).is_some_and(|c| c.not_null));
            let near_target = Cardinality::new(if nullable { 0 } else { 1 }, Max::Count(1));
            let near_source = if unique(t, fk) {
                Cardinality::new(0, Max::Count(1))
            } else {
                Cardinality::new(0, Max::Many)
            };
            if fk.target == t.key {
                // S8: the line of the referenced side carries the role.
                let role = relationship_name(&fk.columns, "");
                m.join(&r, &other, near_target, Some(role), Line::Solid);
                m.join(&r, &owner, near_source, None, Line::Solid);
            } else {
                // A weak entity takes part totally in its identifying relationship.
                let line = if identifying { Line::Double } else { Line::Solid };
                m.join(&r, &other, near_target, None, Line::Solid);
                m.join(&r, &owner, near_source, None, line);
            }
        }
    }
    let count = |f: fn(&NodeKind) -> bool| m.d.nodes.iter().filter(|n| f(&n.kind)).count();
    let entities = count(|k| matches!(k, NodeKind::Entity { .. }));
    let relationships = count(|k| matches!(k, NodeKind::Relationship { .. }));
    let attributes = count(NodeKind::is_attribute);
    let mut hints = m.hints;
    hints.extend(schema.recoded.iter().map(|&line| Hint::Recoded { line }));
    Import {
        diagram: m.d,
        report: Report {
            dialect: schema.dialect,
            tables: tables.len(),
            entities,
            relationships,
            attributes,
            hints,
            skipped: schema.skipped.clone(),
            problems: schema.problems.clone(),
        },
    }
}

/// S3/S4/S5.
fn classify(t: &Table, fks: &[ForeignKey], referenced: bool) -> Class {
    let pk: HashSet<&str> = t.primary_key.iter().map(String::as_str).collect();
    let fk_cols: HashSet<&str> =
        fks.iter().flat_map(|fk| fk.columns.iter().map(String::as_str)).collect();
    let in_pk = in_key(t, fks);
    let in_pk_cols: HashSet<&str> =
        in_pk.iter().flat_map(|fk| fk.columns.iter().map(String::as_str)).collect();
    if in_pk.len() >= 2
        && !pk.is_empty()
        && pk == in_pk_cols
        && in_pk.len() == fks.len()
        && !referenced
    {
        return Class::Junction;
    }
    let own_key = pk.iter().any(|c| !fk_cols.contains(c));
    let loose = pk.iter().any(|c| fk_cols.contains(c) && !in_pk_cols.contains(c));
    if !in_pk.is_empty() && own_key && !loose { Class::Weak } else { Class::Entity }
}

/// The foreign keys that lie wholly in the primary key.
fn in_key<'a>(t: &Table, fks: &'a [ForeignKey]) -> Vec<&'a ForeignKey> {
    fks.iter()
        .filter(|fk| {
            !t.primary_key.is_empty() && fk.columns.iter().all(|c| t.primary_key.contains(c))
        })
        .collect()
}

/// Whether the foreign key's columns are the primary key or a unique key (S7).
fn unique(t: &Table, fk: &ForeignKey) -> bool {
    let cols: HashSet<&String> = fk.columns.iter().collect();
    let same = |k: &Vec<String>| !k.is_empty() && k.iter().collect::<HashSet<_>>() == cols;
    same(&t.primary_key) || t.uniques.iter().any(same)
}

/// S5 (Q3): the table's name with a capital first letter; other schemas than `public` in
/// front, in lower case.
pub fn entity_name(key: &TableKey) -> String {
    let mut chars = key.name.chars();
    let name = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if key.in_default_schema() { name } else { format!("{}.{name}", key.schema) }
}

/// S9 (O3): the foreign key's column(s) without `_id` and the like; "hat" if that only
/// names the target table again (`emp_no` → `employees`, `category_id` → `categories`).
pub fn relationship_name(columns: &[String], target: &str) -> String {
    let stem = match columns {
        [] => String::new(),
        [one] => strip_suffix(one),
        [first, rest @ ..] => {
            let mut prefix: Vec<char> = first.chars().collect();
            for c in rest {
                let c: Vec<char> = c.chars().collect();
                let common = prefix.iter().zip(&c).take_while(|(a, b)| a == b).count();
                prefix.truncate(common);
            }
            let prefix: String = prefix.into_iter().collect();
            let prefix = prefix.trim_end_matches('_');
            if prefix.is_empty() { String::new() } else { strip_suffix(prefix) }
        }
    };
    let singular = |w: &str| {
        let w = w.to_lowercase();
        if let Some(s) = w.strip_suffix("ies") {
            format!("{s}y")
        } else if let Some(s) = w.strip_suffix('s') {
            s.to_owned()
        } else {
            w
        }
    };
    if stem.is_empty()
        || target.to_lowercase().starts_with(&stem.to_lowercase())
        || singular(target) == singular(&stem)
    {
        return "hat".to_owned();
    }
    stem
}

fn strip_suffix(column: &str) -> String {
    for suffix in ["_id", "_fk", "_nr", "_no", "id"] {
        let n = column.len();
        if n >= suffix.len()
            && column.is_char_boundary(n - suffix.len())
            && column[n - suffix.len()..].eq_ignore_ascii_case(suffix)
        {
            return column[..n - suffix.len()].trim_end_matches('_').to_owned();
        }
    }
    column.trim_end_matches('_').to_owned()
}

/// `numeric(8,2)` → (`numeric`, `8,2`); `int(10) unsigned` → (`int unsigned`, `10`); other
/// parentheses (`enum('a','b')`) stay in the type.
pub fn split_type(datatype: &str) -> (String, Option<String>) {
    let (Some(open), Some(close)) = (datatype.find('('), datatype.find(')')) else {
        return (datatype.to_owned(), None);
    };
    if close < open {
        return (datatype.to_owned(), None);
    }
    let inside: String = datatype[open + 1..close].chars().filter(|c| !c.is_whitespace()).collect();
    let mut parts = inside.split(',');
    let numbers =
        parts.next().is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            && parts.clone().count() <= 1
            && parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if !numbers {
        return (datatype.to_owned(), None);
    }
    let before = datatype[..open].trim();
    let after = datatype[close + 1..].trim();
    let base = if after.is_empty() { before.to_owned() } else { format!("{before} {after}") };
    (base, Some(inside))
}

fn diagram_name(schema: &Schema, options: &Options) -> String {
    if let Some(db) = &schema.database {
        return db.clone();
    }
    let stem = options.file_name.as_deref().map(|f| match f.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => f,
    });
    stem.unwrap_or("SQL").to_owned()
}

struct Mapper {
    d: Diagram,
    entities: HashMap<TableKey, Id>,
    hints: Vec<Hint>,
    attributes: Attributes,
}

impl Mapper {
    fn node(&mut self, kind: NodeKind, name: &str, props: Props) -> Id {
        let (w, h) = umlcraft_erm::create::SizeSet::Compact.size(&kind);
        let (w, h) = size::fit(&kind, name, (w, h));
        let id = Id::generate();
        self.d.nodes.push(Node {
            id: id.clone(),
            kind,
            name: name.to_owned(),
            bounds: Rect::new(0.0, 0.0, w, h),
            style: Style::default(),
            props,
        });
        id
    }

    fn entity(&mut self, key: &TableKey, weak: bool, comment: Option<String>) -> Id {
        let props = Props { physical_name: Some(key.to_string()), comment, ..Props::default() };
        let id = self.node(NodeKind::Entity { weak }, &entity_name(key), props);
        self.entities.insert(key.clone(), id.clone());
        id
    }

    /// The entity of a referenced table; S10: a placeholder if it is not there.
    fn target(&mut self, key: &TableKey) -> Id {
        if let Some(id) = self.entities.get(key) {
            return id.clone();
        }
        let id = self.entity(key, false, Some(PLACEHOLDER_COMMENT.to_owned()));
        self.hints.push(Hint::NotInDump { element: id.clone(), table: key.to_string() });
        id
    }

    fn join(&mut self, r: &Id, e: &Id, c: Cardinality, role: Option<String>, line: Line) {
        let mut link = Link::new(r.clone(), e.clone());
        link.label = Some(Label {
            text: LabelText::Cardinality(c),
            pos: LabelPos { y: layout::LABEL_OFFSET, ..LabelPos::default() },
        });
        link.role = role.map(|text| Caption {
            text,
            pos: LabelPos { y: -layout::LABEL_OFFSET, ..LabelPos::default() },
        });
        link.line = line;
        self.d.links.push(link);
    }

    /// S6: the columns `names` in table order as attributes of `owner`.
    fn attributes(
        &mut self,
        owner: &Id,
        t: &Table,
        names: &[&str],
        keys: &[&str],
        partial: &[&str],
    ) {
        for c in &t.columns {
            if !names.contains(&c.name.as_str()) {
                continue;
            }
            let key = if keys.contains(&c.name.as_str()) {
                Some(Key::Primary)
            } else if partial.contains(&c.name.as_str()) {
                Some(Key::Partial)
            } else {
                None
            };
            let take = match self.attributes {
                Attributes::All => true,
                Attributes::Keys => key.is_some(),
                Attributes::None => false,
            };
            if !take {
                continue;
            }
            let (datatype, length) = split_type(&c.datatype);
            let props = Props {
                physical_name: Some(c.name.clone()),
                datatype: (!datatype.is_empty()).then_some(datatype),
                length_precision: length,
                not_null: c.not_null,
                default_value: c.default.clone(),
                comment: c.comment.clone(),
            };
            let kind = NodeKind::Attribute { key, derived: c.generated, multivalued: false };
            let a = self.node(kind, &c.name, props);
            self.d.links.push(Link::new(a, owner.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Dialect, parse::parse};
    use pretty_assertions::assert_eq;

    fn names(cols: &[&str], target: &str) -> String {
        let cols: Vec<String> = cols.iter().map(|c| (*c).to_owned()).collect();
        relationship_name(&cols, target)
    }

    #[test]
    fn relationship_names() {
        assert_eq!(names(&["start_flughafen_id"], "flughafen"), "start_flughafen");
        assert_eq!(names(&["flughafen_id"], "flughafen"), "hat");
        assert_eq!(names(&["emp_no"], "employees"), "hat");
        assert_eq!(names(&["category_id"], "categories"), "hat");
        assert_eq!(names(&["ArtistId"], "Artist"), "hat");
        assert_eq!(names(&["ReportsTo"], "Employee"), "ReportsTo");
        assert_eq!(names(&["original_language_id"], "language"), "original_language");
        assert_eq!(names(&["gate_flughafen_id", "gate_nummer"], "gate"), "hat");
        assert_eq!(names(&["vorgesetzter_id"], ""), "vorgesetzter");
    }

    #[test]
    fn entity_names_and_types() {
        assert_eq!(entity_name(&TableKey::new("public", "kunde_konto")), "Kunde_konto");
        assert_eq!(entity_name(&TableKey::new("fracht", "Container")), "fracht.Container");
        assert_eq!(entity_name(&TableKey::new("", "über")), "Über");
        assert_eq!(split_type("numeric(8,2)"), ("numeric".into(), Some("8,2".into())));
        assert_eq!(split_type("int(10) unsigned"), ("int unsigned".into(), Some("10".into())));
        assert_eq!(
            split_type("character varying( 50 )"),
            ("character varying".into(), Some("50".into()))
        );
        assert_eq!(split_type("enum('a','b')"), ("enum('a','b')".into(), None));
        assert_eq!(split_type("decimal(1,2,3)"), ("decimal(1,2,3)".into(), None));
    }

    fn kinds(sql: &str) -> Vec<String> {
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let imp = map(&s, &Options::default());
        imp.diagram
            .nodes
            .iter()
            .filter(|n| !n.kind.is_attribute())
            .map(|n| format!("{} {}", n.kind.type_name(), n.name))
            .collect()
    }

    #[test]
    fn duplicate_keys_count_once_and_missing_targets_get_placeholders() {
        let sql = "CREATE TABLE a (id int PRIMARY KEY, b_id int, FOREIGN KEY (b_id) REFERENCES b (id), \
                   FOREIGN KEY (b_id) REFERENCES b (id));";
        assert_eq!(kinds(sql), ["entity A", "entity B", "relationship hat"]);
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let imp = map(&s, &Options::default());
        assert!(matches!(&imp.report.hints[..], [Hint::NotInDump { table, .. }] if table == "b"));
    }

    #[test]
    fn junction_with_two_keys_to_one_table_gets_roles() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY);\nCREATE TABLE f (a int, b int, since date, \
                   PRIMARY KEY (a, b), FOREIGN KEY (a) REFERENCES p (id), FOREIGN KEY (b) \
                   REFERENCES p (id));";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let d = map(&s, &Options::default()).diagram;
        let r = d.nodes.iter().find(|n| n.name == "f").unwrap();
        assert!(matches!(r.kind, NodeKind::Relationship { identifying: false }));
        let roles: Vec<_> = d
            .links
            .iter()
            .filter(|l| l.source == r.id && d.node(&l.target).unwrap().name == "P")
            .map(|l| l.role.as_ref().unwrap().text.clone())
            .collect();
        assert_eq!(roles, ["a", "b"]);
        assert!(d.nodes.iter().any(|n| n.name == "since"));
    }

    #[test]
    fn junction_with_its_own_key_column_is_a_weak_entity() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY);\nCREATE TABLE q (id int PRIMARY KEY);\n\
                   CREATE TABLE pq (p_id int, q_id int, nr int, PRIMARY KEY (p_id, q_id, nr), \
                   FOREIGN KEY (p_id) REFERENCES p (id), FOREIGN KEY (q_id) REFERENCES q (id));";
        assert_eq!(
            kinds(sql),
            ["entity P", "entity Q", "entity Pq", "relationship hat", "relationship hat"]
        );
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let d = map(&s, &Options::default()).diagram;
        let pq = d.nodes.iter().find(|n| n.name == "Pq").unwrap();
        assert_eq!(pq.kind, NodeKind::Entity { weak: true });
        let nr = d.nodes.iter().find(|n| n.name == "nr").unwrap();
        assert!(matches!(nr.kind, NodeKind::Attribute { key: Some(Key::Partial), .. }));
        assert!(
            umlcraft_erm::check(&d).iter().all(|i| i.severity() == umlcraft_erm::Severity::Warning)
        );
    }

    #[test]
    fn chained_weak_entities_make_a_valid_diagram() {
        // order → item → note: the item is weak and owns the weak note.
        let sql = "CREATE TABLE o (id int PRIMARY KEY);\n\
                   CREATE TABLE oi (o_id int, nr int, PRIMARY KEY (o_id, nr), \
                   FOREIGN KEY (o_id) REFERENCES o (id));\n\
                   CREATE TABLE oin (o_id int, nr int, k int, PRIMARY KEY (o_id, nr, k), \
                   FOREIGN KEY (o_id, nr) REFERENCES oi (o_id, nr));";
        let s = parse(sql.as_bytes(), Dialect::MySql);
        let d = map(&s, &Options::default()).diagram;
        let weak = d.nodes.iter().filter(|n| n.kind == NodeKind::Entity { weak: true }).count();
        assert_eq!(weak, 2);
        assert_eq!(umlcraft_erm::check(&d), []);
        let text = umlcraft_erm::write(&d);
        assert!(umlcraft_erm::read(&text).is_ok());
    }

    #[test]
    fn options_choose_attributes_and_schemas() {
        let sql = "CREATE TABLE public.a (id int, x int);\nALTER TABLE public.a ADD PRIMARY KEY (id);\n\
                   CREATE TABLE s.b (id int);";
        let s = parse(sql.as_bytes(), Dialect::Postgres);
        let count = |o: &Options| map(&s, o).report.attributes;
        assert_eq!(count(&Options::default()), 3);
        assert_eq!(count(&Options { attributes: Attributes::Keys, ..Options::default() }), 1);
        assert_eq!(count(&Options { attributes: Attributes::None, ..Options::default() }), 0);
        let only_public = Options { schemas: Some(vec!["public".into()]), ..Options::default() };
        let r = map(&s, &only_public).report;
        assert_eq!((r.tables, r.entities), (1, 1));
    }

    #[test]
    fn the_diagram_is_named_after_the_database_or_the_file() {
        let s = parse(b"-- Host: x    Database: flug\nCREATE TABLE a (id int);", Dialect::MySql);
        let o = Options { file_name: Some("dump.sql".into()), ..Options::default() };
        let d = map(&s, &o).diagram;
        assert_eq!(d.meta.name, "flug");
        assert_eq!(d.meta.origin.as_deref(), Some("SQL: dump.sql (MySQL)"));
        let s = parse(b"CREATE TABLE a (id int);", Dialect::Postgres);
        assert_eq!(map(&s, &o).diagram.meta.name, "dump");
    }
}
