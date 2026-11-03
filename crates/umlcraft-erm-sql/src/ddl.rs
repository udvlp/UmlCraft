//! SQL from an ER diagram: the tables of the entities it shows as `CREATE TABLE` statements
//! for MySQL/MariaDB or PostgreSQL, the way back of the import in [`crate::map`].
//!
//! - An entity is a table (its physical name, else its name; `schema.table` in PostgreSQL),
//!   its attributes are the columns (the parts of a composite attribute instead of it;
//!   multivalued ones have no column and are listed in [`Ddl::left_out`]), its keys and
//!   partial keys the primary key.
//! - Foreign key columns (attributes with a `foreignKey`) reference the primary key of the
//!   relationship's other entity, or the supertype's for a subtyping, in key order.
//! - A relationship without such columns gets them. A binary one puts them into the entity
//!   across from the side with `[x,1]` (read look-across, as the import writes it), named
//!   after the referenced table and column, in the key when it is identifying. A relationship
//!   of more than two entities, one with attributes, a many-to-many one and an associative
//!   entity become a table of their own with a column per key column of each entity, all of
//!   them its key unless it has one.
//! - A subtype without foreign key columns takes the supertype's key as its own.
//! - Foreign keys come last, as `ALTER TABLE … ADD CONSTRAINT`, so that the order of the
//!   tables does not matter.
//!
//! Types are written as the model has them; attributes without one get `varchar(255)`.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use umlcraft_model::{
    Cardinality, CardinalityMax, CardinalityMin, ElementKind, ErAttributeProps, ErEntityProps, Id,
    Model, ModelError,
};

use crate::dialect::Dialect;

/// The type of attributes that have none.
const DEFAULT_TYPE: &str = "varchar(255)";
/// The longest name a constraint gets (PostgreSQL allows 63, MySQL 64).
const NAME_LIMIT: usize = 60;

/// The statements and what went into them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ddl {
    pub sql: String,
    pub tables: usize,
    pub foreign_keys: usize,
    /// What has no place in the tables.
    pub left_out: Vec<LeftOut>,
}

/// Something of the diagram the tables do without.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LeftOut {
    /// A multivalued attribute: no column.
    Multivalued { table: String, attribute: String },
    /// A relationship to an entity without a primary key: no foreign key.
    NoKey { table: String, target: String },
}

#[derive(Clone, Debug)]
struct Column {
    name: String,
    datatype: String,
    not_null: bool,
    /// The default, or the expression of a generated column.
    default: String,
    generated: bool,
    unique: bool,
    comment: String,
}

#[derive(Clone, Debug)]
struct Table {
    schema: Option<String>,
    name: String,
    columns: Vec<Column>,
    key: Vec<String>,
    uniques: Vec<Vec<String>>,
    comment: String,
}

impl Table {
    fn new(schema: Option<String>, name: String, comment: String) -> Table {
        Table { schema, name, columns: Vec::new(), key: Vec::new(), uniques: Vec::new(), comment }
    }

    fn has(&self, column: &str) -> bool {
        self.columns.iter().any(|c| c.name.eq_ignore_ascii_case(column))
    }

    /// Makes `columns` unique, unless they are already (as the key, or a column of its own).
    fn unique(&mut self, columns: Vec<String>) {
        let same = |other: &[String]| {
            other.len() == columns.len() && columns.iter().all(|c| other.contains(c))
        };
        let single =
            columns.len() == 1 && self.columns.iter().any(|c| c.name == columns[0] && c.unique);
        if columns.is_empty() || same(&self.key) || single || self.uniques.iter().any(|u| same(u)) {
            return;
        }
        self.uniques.push(columns);
    }

    /// `base`, or `base2`, `base3`, … if the table has that column already.
    fn free_name(&self, base: &str) -> String {
        if !self.has(base) {
            return base.to_owned();
        }
        (2..).map(|n| format!("{base}{n}")).find(|n| !self.has(n)).expect("names are unbounded")
    }
}

/// A foreign key: columns of `table` that reference columns of `target`, mostly its key.
#[derive(Clone, Debug)]
struct Reference {
    table: usize,
    columns: Vec<String>,
    target: usize,
    target_columns: Vec<String>,
}

/// The tables of the entities on `diagram` (an ER diagram) as SQL for `dialect`.
pub fn ddl(model: &Model, diagram: Id, dialect: Dialect) -> Result<Ddl, ModelError> {
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let mut shown: Vec<Id> = Vec::new();
    for p in view.presentations() {
        if let Some(e) = p.element
            && !shown.contains(&e)
        {
            shown.push(e);
        }
    }
    let kind = |id: Id| model.get(id).map(|e| e.kind());
    let mut b = Builder {
        model,
        dialect,
        tables: Vec::new(),
        of: HashMap::new(),
        refs: Vec::new(),
        left_out: Vec::new(),
        pairs: HashMap::new(),
    };
    for &e in &shown {
        if let Some(ElementKind::ErEntity(p)) = kind(e) {
            b.entity(e, p);
        }
    }
    // How many relationships join each two tables: the columns of one of several are named
    // after the relationship.
    for &e in &shown {
        if let Some(ElementKind::ErRelationship(_)) = kind(e) {
            let ends: Vec<usize> = umlcraft_notation::er::participations(model, e)
                .filter_map(|p| b.of.get(&p.entity).copied())
                .collect();
            if let [x, y] = ends[..] {
                *b.pairs.entry((x.min(y), x.max(y))).or_default() += 1;
            }
        }
    }
    // Foreign key columns the model has, by what they belong to and the table they are in.
    let mut columns_of: HashMap<Id, Vec<(usize, String)>> = HashMap::new();
    for &entity in &shown {
        let Some(&table) = b.of.get(&entity) else { continue };
        for (attribute, name) in b.columns_of_entity(entity) {
            if let Some(ElementKind::ErAttribute(p)) = kind(attribute)
                && let Some(owner) = p.foreign_key
            {
                columns_of.entry(owner).or_default().push((table, name));
            }
        }
    }
    for &e in &shown {
        match kind(e) {
            Some(ElementKind::ErRelationship(p)) => {
                let identifying = p.identifying;
                b.relationship(e, identifying, columns_of.get(&e).map(Vec::as_slice));
            }
            Some(ElementKind::ErEntity(ErEntityProps { associative: true, .. })) => {
                b.relationship(e, false, columns_of.get(&e).map(Vec::as_slice));
            }
            Some(ElementKind::ErIsA(_)) => b.is_a(e, &columns_of),
            _ => {}
        }
    }
    Ok(b.write(model.get(diagram).map_or("", |d| d.name())))
}

/// The dialect the package of `diagram` was imported from, as the import notes it in the
/// package's documentation (`SQL: file (MySQL)`).
pub fn imported_dialect(model: &Model, diagram: Id) -> Option<Dialect> {
    let package = model.get(diagram).and_then(|d| d.owner()).and_then(|p| model.get(p))?;
    let text = &package.data().documentation;
    Dialect::ALL.into_iter().find(|d| text.contains(&format!("({})", d.name())))
}

struct Builder<'a> {
    model: &'a Model,
    dialect: Dialect,
    tables: Vec<Table>,
    /// The table of each entity.
    of: HashMap<Id, usize>,
    refs: Vec<Reference>,
    left_out: Vec<LeftOut>,
    /// The number of binary relationships between two tables (the lower index first).
    pairs: HashMap<(usize, usize), usize>,
}

impl Builder<'_> {
    fn entity(&mut self, entity: Id, props: &ErEntityProps) {
        let element = &self.model.get(entity).expect("shown");
        let full =
            if props.physical_name.is_empty() { element.name() } else { &props.physical_name };
        let (schema, name) = match full.split_once('.') {
            Some((s, n)) if self.dialect == Dialect::Postgres && !s.is_empty() && !n.is_empty() => {
                (Some(s.to_owned()), n.to_owned())
            }
            _ => (None, full.to_owned()),
        };
        let mut table = Table::new(schema, name, element.data().documentation.clone());
        self.add_columns(&mut table, entity);
        self.of.insert(entity, self.tables.len());
        self.tables.push(table);
    }

    /// The attributes of `owner` as columns of `table`, its keys into the key.
    fn add_columns(&mut self, table: &mut Table, owner: Id) {
        for attribute in leaf_attributes(self.model, owner) {
            let element = self.model.get(attribute).expect("child");
            let ElementKind::ErAttribute(p) = element.kind() else { continue };
            let name = column_name(element.name(), p);
            if p.multivalued {
                let (table, attribute) = (table.name.clone(), name);
                self.left_out.push(LeftOut::Multivalued { table, attribute });
                continue;
            }
            if p.key.is_some() {
                table.key.push(name.clone());
            }
            table.columns.push(Column {
                datatype: datatype_of(p),
                not_null: p.not_null || p.key.is_some(),
                default: p.default_value.clone(),
                // The import keeps the expression of a generated column as its default.
                generated: p.derived && !p.default_value.is_empty(),
                unique: p.unique,
                comment: element.data().documentation.clone(),
                name,
            });
        }
    }

    /// The columns of an entity's table with the attributes they come from.
    fn columns_of_entity(&self, entity: Id) -> Vec<(Id, String)> {
        leaf_attributes(self.model, entity)
            .into_iter()
            .filter_map(|a| {
                let element = self.model.get(a)?;
                let ElementKind::ErAttribute(p) = element.kind() else { return None };
                (!p.multivalued).then(|| (a, column_name(element.name(), p)))
            })
            .collect()
    }

    /// The key columns of a table with their types.
    fn key_of(&self, table: usize) -> Vec<(String, String)> {
        let t = &self.tables[table];
        t.key
            .iter()
            .filter_map(|k| t.columns.iter().find(|c| &c.name == k))
            .map(|c| (c.name.clone(), c.datatype.clone()))
            .collect()
    }

    /// New columns in `table` that reference the key of `target`: `prefix_column` for each of
    /// its key columns, in the key of `table` when `in_key`, unique when `unique`.
    fn refer(
        &mut self,
        table: usize,
        target: usize,
        prefix: &str,
        not_null: bool,
        in_key: bool,
        unique: bool,
    ) {
        let key = self.key_of(target);
        if key.is_empty() {
            self.no_key(table, target);
            return;
        }
        let mut columns = Vec::new();
        let target_columns = key.iter().map(|k| k.0.clone()).collect();
        for (name, datatype) in key {
            let t = &mut self.tables[table];
            let base = if prefix.is_empty() { name } else { format!("{prefix}_{name}") };
            let name = t.free_name(&base);
            t.columns.push(Column {
                name: name.clone(),
                datatype,
                not_null: not_null || in_key,
                default: String::new(),
                generated: false,
                unique: false,
                comment: String::new(),
            });
            if in_key {
                t.key.push(name.clone());
            }
            columns.push(name);
        }
        if unique {
            self.tables[table].unique(columns.clone());
        }
        self.refs.push(Reference { table, columns, target, target_columns });
    }

    /// Foreign key columns the model has: they reference the columns of `target` they are
    /// named after (`kunde_id` → `id`, `gate_nummer` → `nummer`), keys first, else its key
    /// in order.
    fn link(&mut self, table: usize, columns: Vec<String>, target: usize, unique: bool) {
        let t = &self.tables[target];
        let named = |c: &String| {
            let c = c.to_ascii_lowercase();
            let fits = |n: &String| {
                let n = n.to_ascii_lowercase();
                c == n || c.ends_with(&format!("_{n}"))
            };
            let longest = |names: &mut dyn Iterator<Item = &String>| {
                names.filter(|n| fits(n)).max_by_key(|n| n.len()).cloned()
            };
            longest(&mut t.key.iter()).or_else(|| longest(&mut t.columns.iter().map(|c| &c.name)))
        };
        let by_name: Option<Vec<String>> = columns.iter().map(named).collect();
        let target_columns = match by_name {
            Some(n) if n.iter().collect::<HashSet<_>>().len() == n.len() => n,
            _ if columns.len() == t.key.len() => t.key.clone(),
            _ => {
                self.no_key(table, target);
                return;
            }
        };
        // A reference needs a key or unique columns to point at.
        self.tables[target].unique(target_columns.clone());
        if unique {
            self.tables[table].unique(columns.clone());
        }
        self.refs.push(Reference { table, columns, target, target_columns });
    }

    fn no_key(&mut self, table: usize, target: usize) {
        let (table, target) = (self.tables[table].name.clone(), self.tables[target].name.clone());
        self.left_out.push(LeftOut::NoKey { table, target });
    }

    /// A relationship, or an associative entity: its foreign keys, from the `existing`
    /// foreign key columns or new ones.
    fn relationship(&mut self, hub: Id, identifying: bool, existing: Option<&[(usize, String)]>) {
        let parts: Vec<(usize, Option<Cardinality>, String)> =
            umlcraft_notation::er::participations(self.model, hub)
                .filter_map(|p| Some((*self.of.get(&p.entity)?, p.cardinality, p.role.clone())))
                .collect();
        if let Some(existing) = existing {
            // Each table with columns of it references the other end (itself, for a
            // self-reference).
            let mut tables: Vec<usize> = existing.iter().map(|(t, _)| *t).collect();
            tables.sort_unstable();
            tables.dedup();
            let one = |p: &(usize, Option<Cardinality>, String)| {
                p.1.is_some_and(|c| c.max == CardinalityMax::Count(1))
            };
            for table in tables {
                let target = parts
                    .iter()
                    .map(|p| p.0)
                    .find(|&t| t != table)
                    .or_else(|| parts.iter().map(|p| p.0).find(|&t| t == table));
                let Some(target) = target else { continue };
                let columns: Vec<String> =
                    existing.iter().filter(|(t, _)| *t == table).map(|(_, c)| c.clone()).collect();
                // `[x,1]` at the table itself: one row of it per row of the other, 1:1.
                let unique = if target == table {
                    parts.iter().all(one)
                } else {
                    parts.iter().filter(|p| p.0 == table).all(one)
                };
                self.link(table, columns, target, unique);
            }
            return;
        }
        let model = self.model;
        let hub_element = model.get(hub).expect("shown");
        let associative = self.of.get(&hub).copied();
        let has_attributes = associative.is_none()
            && model.children(hub).iter().any(|&c| {
                matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ErAttribute(_)))
            });
        let one = |c: &Option<Cardinality>| c.is_some_and(|c| c.max == CardinalityMax::Count(1));
        let binary = parts.len() == 2 && associative.is_none() && !has_attributes;
        if binary && (one(&parts[0].1) || one(&parts[1].1)) {
            // Look-across: `[x,1]` at an entity says each row of the other has one of it.
            let required = |c: &Option<Cardinality>| {
                c.is_some_and(|c| matches!(c.min, CardinalityMin::Count(n) if n >= 1))
            };
            let parent = match (one(&parts[0].1), one(&parts[1].1)) {
                (true, false) => 0,
                (false, true) => 1,
                _ if required(&parts[1].1) && !required(&parts[0].1) => 1,
                _ => 0,
            };
            let (target, child) = (parts[parent].0, parts[1 - parent].0);
            let role = &parts[parent].2;
            let several = self.pairs.get(&(target.min(child), target.max(child))) > Some(&1);
            let prefix = if !role.is_empty() {
                snake(role)
            } else if target == child || several {
                snake(hub_element.name())
            } else {
                snake(&self.tables[target].name)
            };
            // One to one: a row of the child for each of the parent at most. A child without
            // a key of its own takes the parent's.
            let unique = one(&parts[1 - parent].1);
            let in_key = identifying || (unique && self.tables[child].key.is_empty());
            self.refer(child, target, &prefix, required(&parts[parent].1), in_key, unique);
            return;
        }
        // A table of its own: the associative entity's, or a new one named after the
        // relationship, with its attributes.
        let table = match associative {
            Some(t) => t,
            None => {
                let mut table = Table::new(
                    None,
                    hub_element.name().to_owned(),
                    hub_element.data().documentation.clone(),
                );
                if self.dialect == Dialect::Postgres
                    && let Some((s, n)) = table.name.clone().split_once('.')
                {
                    (table.schema, table.name) = (Some(s.to_owned()), n.to_owned());
                }
                self.add_columns(&mut table, hub);
                self.tables.push(table);
                self.tables.len() - 1
            }
        };
        let in_key = self.tables[table].key.is_empty();
        for (target, _, role) in &parts {
            let twice = parts.iter().filter(|p| p.0 == *target).count() > 1;
            let prefix = if twice && !role.is_empty() {
                snake(role)
            } else {
                snake(&self.tables[*target].name)
            };
            self.refer(table, *target, &prefix, true, in_key, false);
        }
    }

    /// The subtypes of an IS-A reference their supertype.
    fn is_a(&mut self, is_a: Id, existing: &HashMap<Id, Vec<(usize, String)>>) {
        let model = self.model;
        let lines: Vec<(Id, Id, bool)> = model
            .children(is_a)
            .iter()
            .filter_map(|&c| match model.get(c)?.kind() {
                ElementKind::ErSubtyping(p) => Some((c, p.entity, p.supertype)),
                _ => None,
            })
            .collect();
        let Some(&(_, supertype, _)) = lines.iter().find(|l| l.2) else { return };
        let Some(&target) = self.of.get(&supertype) else { return };
        for &(line, entity, upper) in &lines {
            let Some(&table) = self.of.get(&entity) else { continue };
            if upper || table == target {
                continue;
            }
            match existing.get(&line) {
                Some(columns) => {
                    let columns = columns.iter().map(|(_, c)| c.clone()).collect();
                    self.link(table, columns, target, false);
                }
                None => {
                    let in_key = self.tables[table].key.is_empty();
                    self.refer(table, target, "", true, in_key, false);
                }
            }
        }
    }

    fn write(self, diagram: &str) -> Ddl {
        let d = self.dialect;
        let mut sql = String::new();
        let _ =
            writeln!(sql, "-- UmlCraft: the ER diagram \"{}\" for {d}", diagram.replace('\n', " "));
        let mut schemas: Vec<&str> = Vec::new();
        for t in &self.tables {
            if let Some(s) = t.schema.as_deref()
                && s != "public"
                && !schemas.contains(&s)
            {
                schemas.push(s);
            }
        }
        if !schemas.is_empty() {
            sql.push('\n');
        }
        for s in &schemas {
            let _ = writeln!(sql, "CREATE SCHEMA IF NOT EXISTS {};", quote(s, d));
        }
        for t in &self.tables {
            let name = table_name(t, d);
            let _ = write!(sql, "\nCREATE TABLE {name} (");
            let mut lines: Vec<String> = t.columns.iter().map(|c| column(c, d)).collect();
            if !t.key.is_empty() {
                let key: Vec<String> = t.key.iter().map(|k| quote(k, d)).collect();
                lines.push(format!("PRIMARY KEY ({})", key.join(", ")));
            }
            for u in &t.uniques {
                let u: Vec<String> = u.iter().map(|c| quote(c, d)).collect();
                lines.push(format!("UNIQUE ({})", u.join(", ")));
            }
            for (k, line) in lines.iter().enumerate() {
                let comma = if k + 1 < lines.len() { "," } else { "" };
                let _ = write!(sql, "\n  {line}{comma}");
            }
            sql.push_str("\n)");
            if d.is_mysql() {
                sql.push_str(" ENGINE=InnoDB");
                if !t.comment.is_empty() {
                    let _ = write!(sql, " COMMENT={}", string(&t.comment, d));
                }
            }
            sql.push_str(";\n");
            if !d.is_mysql() {
                if !t.comment.is_empty() {
                    let _ = writeln!(sql, "COMMENT ON TABLE {name} IS {};", string(&t.comment, d));
                }
                for c in t.columns.iter().filter(|c| !c.comment.is_empty()) {
                    let column = quote(&c.name, d);
                    let text = string(&c.comment, d);
                    let _ = writeln!(sql, "COMMENT ON COLUMN {name}.{column} IS {text};");
                }
            }
        }
        if !self.refs.is_empty() {
            sql.push('\n');
        }
        let mut names: HashSet<String> = HashSet::new();
        for r in &self.refs {
            let (t, target) = (&self.tables[r.table], &self.tables[r.target]);
            let key: Vec<String> = r.target_columns.iter().map(|k| quote(k, d)).collect();
            let columns: Vec<String> = r.columns.iter().map(|c| quote(c, d)).collect();
            let mut base: String = format!("fk_{}_{}", snake(&t.name), snake(&r.columns.join("_")));
            if base.len() > NAME_LIMIT {
                let cut = (0..=NAME_LIMIT).rev().find(|&i| base.is_char_boundary(i)).unwrap_or(0);
                base.truncate(cut);
            }
            let name = if names.contains(&base) {
                (2..).map(|n| format!("{base}{n}")).find(|n| !names.contains(n)).expect("unbounded")
            } else {
                base
            };
            names.insert(name.clone());
            let _ = writeln!(
                sql,
                "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({});",
                table_name(t, d),
                quote(&name, d),
                columns.join(", "),
                table_name(target, d),
                key.join(", "),
            );
        }
        Ddl {
            sql,
            tables: self.tables.len(),
            foreign_keys: self.refs.len(),
            left_out: self.left_out,
        }
    }
}

/// The attributes of `owner` that are columns, in order: the parts of a composite attribute
/// instead of it.
fn leaf_attributes(model: &Model, owner: Id) -> Vec<Id> {
    let mut out = Vec::new();
    for &c in model.children(owner) {
        if !matches!(model.get(c).map(|e| e.kind()), Some(ElementKind::ErAttribute(_))) {
            continue;
        }
        let parts = leaf_attributes(model, c);
        if parts.is_empty() {
            out.push(c);
        } else {
            out.extend(parts);
        }
    }
    out
}

fn column_name(name: &str, p: &ErAttributeProps) -> String {
    if p.physical_name.is_empty() { name.to_owned() } else { p.physical_name.clone() }
}

/// The type with its length: `varchar` and `50` → `varchar(50)`, `int unsigned` and `10` →
/// `int(10) unsigned`, as [`crate::map::split_type`] took them apart.
fn datatype_of(p: &ErAttributeProps) -> String {
    let base = p.datatype.trim();
    let base = if base.is_empty() { DEFAULT_TYPE } else { base };
    let length = p.length_precision.trim();
    if length.is_empty() || base.contains('(') {
        return base.to_owned();
    }
    let lower = base.to_ascii_lowercase();
    let suffix = [" unsigned", " signed", " zerofill", " without time zone", " with time zone"]
        .into_iter()
        .filter_map(|s| lower.find(s))
        .min();
    match suffix {
        Some(at) => format!("{}({length}){}", &base[..at], &base[at..]),
        None => format!("{base}({length})"),
    }
}

fn column(c: &Column, d: Dialect) -> String {
    let mut out = format!("{} {}", quote(&c.name, d), c.datatype);
    let identity = d == Dialect::Postgres && c.default.trim_start().starts_with("nextval(");
    if c.generated {
        let _ = write!(out, " GENERATED ALWAYS AS ({}) STORED", c.default);
    }
    if c.not_null {
        out.push_str(" NOT NULL");
    }
    if identity && !c.generated {
        // A sequence of the dump's own; the table gets one of its own instead.
        out.push_str(" GENERATED BY DEFAULT AS IDENTITY");
    } else if !c.default.is_empty() && !c.generated {
        let _ = write!(out, " DEFAULT {}", literal(&c.default, d));
    }
    if c.unique {
        out.push_str(" UNIQUE");
    }
    if d.is_mysql() && !c.comment.is_empty() {
        let _ = write!(out, " COMMENT {}", string(&c.comment, d));
    }
    out
}

fn table_name(t: &Table, d: Dialect) -> String {
    match &t.schema {
        Some(s) => format!("{}.{}", quote(s, d), quote(&t.name, d)),
        None => quote(&t.name, d),
    }
}

/// A default as SQL: numbers, `NULL`, `TRUE`, `CURRENT_TIMESTAMP` and calls as they are,
/// everything else as a string (the import keeps defaults without their quotes).
fn literal(value: &str, d: Dialect) -> String {
    const KEYWORDS: [&str; 10] = [
        "NULL",
        "TRUE",
        "FALSE",
        "CURRENT_TIMESTAMP",
        "CURRENT_DATE",
        "CURRENT_TIME",
        "LOCALTIMESTAMP",
        "LOCALTIME",
        "CURRENT_USER",
        "NOW()",
    ];
    let v = value.trim();
    let keyword = KEYWORDS.iter().any(|k| v.eq_ignore_ascii_case(k));
    if keyword || v.parse::<f64>().is_ok() || v.contains('(') { v.to_owned() } else { string(v, d) }
}

fn string(text: &str, d: Dialect) -> String {
    let mut escaped = text.replace('\'', "''");
    if d.is_mysql() {
        escaped = escaped.replace('\\', "\\\\");
    }
    format!("'{escaped}'")
}

/// Words that cannot be names without quotes in either dialect (the common ones).
const RESERVED: [&str; 40] = [
    "all",
    "and",
    "as",
    "asc",
    "between",
    "by",
    "case",
    "check",
    "column",
    "constraint",
    "create",
    "default",
    "delete",
    "desc",
    "distinct",
    "drop",
    "else",
    "end",
    "for",
    "foreign",
    "from",
    "group",
    "having",
    "in",
    "index",
    "insert",
    "into",
    "key",
    "limit",
    "not",
    "null",
    "on",
    "or",
    "order",
    "primary",
    "references",
    "select",
    "table",
    "to",
    "user",
];

/// A name, quoted where it has to be: with backticks in MySQL, double quotes in PostgreSQL,
/// where it is not a plain name (in PostgreSQL also where it has capitals, which only quotes
/// keep) or a reserved word.
fn quote(name: &str, d: Dialect) -> String {
    let mut chars = name.chars();
    let plain_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || (d.is_mysql() && c == '$');
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(plain_char)
        && (d.is_mysql() || !name.chars().any(|c| c.is_ascii_uppercase()))
        && !RESERVED.contains(&name.to_ascii_lowercase().as_str());
    if plain {
        name.to_owned()
    } else if d.is_mysql() {
        format!("`{}`", name.replace('`', "``"))
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

/// A name for new columns and constraints: lower case, spaces and dots as `_`.
fn snake(name: &str) -> String {
    name.chars()
        .map(
            |c| {
                if c.is_whitespace() || c == '.' || c == '-' { '_' } else { c.to_ascii_lowercase() }
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use umlcraft_model::{
        DiagramKind, Document, ElementData, ErIsAProps, ErKey, ErParticipationProps,
        ErRelationshipProps, ErSubtypingProps, Rect, Tx,
    };

    use super::*;

    type R<T> = Result<T, ModelError>;

    /// A new element shown on the diagram.
    fn shown(tx: &mut Tx<'_>, at: (Id, Id), name: &str, kind: ElementKind) -> R<Id> {
        let e = tx.create(at.0, ElementData::new(name, kind))?;
        tx.add_node(at.1, Some(e), Rect::new(0.0, 0.0, 80.0, 40.0))?;
        Ok(e)
    }

    fn entity(tx: &mut Tx<'_>, at: (Id, Id), name: &str, weak: bool) -> R<Id> {
        shown(tx, at, name, ElementKind::ErEntity(ErEntityProps { weak, ..Default::default() }))
    }

    fn attribute(tx: &mut Tx<'_>, owner: Id, name: &str, p: ErAttributeProps) -> R<Id> {
        tx.create(owner, ElementData::new(name, ElementKind::ErAttribute(p)))
    }

    fn key(datatype: &str) -> ErAttributeProps {
        ErAttributeProps {
            key: Some(ErKey::Primary),
            datatype: datatype.into(),
            ..Default::default()
        }
    }

    fn relate(
        tx: &mut Tx<'_>,
        at: (Id, Id),
        name: &str,
        identifying: bool,
        ends: &[(Id, Cardinality, &str)],
    ) -> R<Id> {
        let kind = ElementKind::ErRelationship(ErRelationshipProps { identifying });
        let r = shown(tx, at, name, kind)?;
        for &(entity, cardinality, role) in ends {
            let mut p = ErParticipationProps::new(r, entity);
            (p.cardinality, p.role) = (Some(cardinality), role.into());
            tx.create(r, ElementData::new("", ElementKind::ErParticipation(p)))?;
        }
        Ok(r)
    }

    /// A drawn diagram: no foreign key columns, every kind of relationship.
    fn shop() -> (Document, Id) {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let mut diagram = root;
        doc.edit("shop", |tx| {
            let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
            diagram = tx.create(root, ElementData::new("Shop", kind))?;
            let at = (root, diagram);
            let (one, zero_one, many) =
                (Cardinality::ONE_ONE, Cardinality::ZERO_ONE, Cardinality::ZERO_MANY);
            let kunde = entity(tx, at, "Kunde", false)?;
            attribute(tx, kunde, "id", key("int"))?;
            let name = ErAttributeProps {
                datatype: "varchar".into(),
                length_precision: "100".into(),
                not_null: true,
                ..Default::default()
            };
            attribute(tx, kunde, "name", name)?;
            let phones = ErAttributeProps { multivalued: true, ..Default::default() };
            attribute(tx, kunde, "telefon", phones)?;
            let konto = entity(tx, at, "Konto", false)?;
            let stand = ErAttributeProps {
                datatype: "numeric".into(),
                length_precision: "10,2".into(),
                default_value: "0".into(),
                ..Default::default()
            };
            attribute(tx, konto, "stand", stand)?;
            relate(tx, at, "besitzt", false, &[(kunde, one, ""), (konto, zero_one, "")])?;
            let bestellung = entity(tx, at, "Bestellung", false)?;
            attribute(tx, bestellung, "nr", key("int"))?;
            let datum = ErAttributeProps {
                datatype: "date".into(),
                default_value: "CURRENT_DATE".into(),
                ..Default::default()
            };
            attribute(tx, bestellung, "datum", datum)?;
            relate(tx, at, "gibt auf", false, &[(kunde, one, ""), (bestellung, many, "")])?;
            let position = entity(tx, at, "Position", true)?;
            let pos = ErAttributeProps {
                key: Some(ErKey::Partial),
                datatype: "smallint".into(),
                ..Default::default()
            };
            attribute(tx, position, "pos", pos)?;
            relate(tx, at, "hat", true, &[(bestellung, one, ""), (position, many, "")])?;
            let produkt = entity(tx, at, "Produkt", false)?;
            attribute(tx, produkt, "nr", key("int"))?;
            let ends = [(bestellung, many, ""), (produkt, many, "")];
            let contains = relate(tx, at, "enthält", false, &ends)?;
            attribute(tx, contains, "menge", ErAttributeProps::default())?;
            let mitarbeiter = entity(tx, at, "Mitarbeiter", false)?;
            attribute(tx, mitarbeiter, "id", key("int"))?;
            let ends = [(mitarbeiter, zero_one, "chef"), (mitarbeiter, many, "")];
            relate(tx, at, "leitet", false, &ends)?;
            let flughafen = entity(tx, at, "Flughafen", false)?;
            attribute(tx, flughafen, "code", key("char(3)"))?;
            let flug = entity(tx, at, "Flug", false)?;
            attribute(tx, flug, "nr", key("int"))?;
            relate(tx, at, "start", false, &[(flughafen, one, ""), (flug, many, "")])?;
            relate(tx, at, "ziel", false, &[(flughafen, one, ""), (flug, many, "")])?;
            let person = entity(tx, at, "Person", false)?;
            attribute(tx, person, "id", key("bigint"))?;
            let student = entity(tx, at, "Student", false)?;
            attribute(tx, student, "matrikel", ErAttributeProps::default())?;
            let is_a = shown(tx, at, "", ElementKind::ErIsA(ErIsAProps::default()))?;
            for (entity, supertype) in [(person, true), (student, false)] {
                let p = ErSubtypingProps { is_a, entity, supertype };
                tx.create(is_a, ElementData::new("", ElementKind::ErSubtyping(p)))?;
            }
            Ok(())
        })
        .unwrap();
        (doc, diagram)
    }

    #[test]
    fn a_drawn_diagram_gets_its_foreign_keys() {
        let (doc, diagram) = shop();
        let out = ddl(doc.model(), diagram, Dialect::Postgres).unwrap();
        let sql = &out.sql;
        let has = |text: &str| assert!(sql.contains(text), "{text}\n{sql}");
        // 1:1, the child without a key of its own: the parent's key is its key.
        has(r#"CREATE TABLE "Konto" (
  stand numeric(10,2) DEFAULT 0,
  kunde_id int NOT NULL,
  PRIMARY KEY (kunde_id)
);"#);
        // 1:n, required.
        has("  datum date DEFAULT CURRENT_DATE,\n  kunde_id int NOT NULL,");
        // Identifying: the weak entity's partial key and its owner's.
        has("  bestellung_nr int NOT NULL,\n  PRIMARY KEY (pos, bestellung_nr)");
        // Many-to-many with an attribute: a table of its own.
        has(r#"CREATE TABLE "enthält" (
  menge varchar(255),
  bestellung_nr int NOT NULL,
  produkt_nr int NOT NULL,
  PRIMARY KEY (bestellung_nr, produkt_nr)
);"#);
        // A self-reference by its role; two relationships between the same tables by name.
        has("  chef_id int,\n");
        has("  start_code char(3) NOT NULL,\n  ziel_code char(3) NOT NULL,");
        // The subtype takes the supertype's key.
        has(r#"CREATE TABLE "Student" (
  matrikel varchar(255),
  id bigint NOT NULL,
  PRIMARY KEY (id)
);"#);
        has(
            r#"ALTER TABLE "Student" ADD CONSTRAINT fk_student_id FOREIGN KEY (id) REFERENCES "Person" (id);"#,
        );
        has(
            r#"ALTER TABLE "Mitarbeiter" ADD CONSTRAINT fk_mitarbeiter_chef_id FOREIGN KEY (chef_id) REFERENCES "Mitarbeiter" (id);"#,
        );
        assert_eq!((out.tables, out.foreign_keys), (11, 9));
        let phones = LeftOut::Multivalued { table: "Kunde".into(), attribute: "telefon".into() };
        assert_eq!(out.left_out, [phones]);
        // It reads back as written.
        let again = crate::read(sql.as_bytes(), Some(Dialect::Postgres)).unwrap();
        assert_eq!(again.problems, []);
        assert_eq!(again.tables.len(), 11);
        assert_eq!(again.tables.values().map(|t| t.foreign_keys.len()).sum::<usize>(), 9);
    }

    #[test]
    fn mysql_quotes_with_backticks_and_comments_inline() {
        let (mut doc, diagram) = shop();
        let model = doc.model();
        let kunde = model
            .children(model.root())
            .iter()
            .copied()
            .find(|&c| model.get(c).is_some_and(|e| e.name() == "Kunde"));
        doc.edit("comment", |tx| tx.update(kunde.unwrap(), |d| d.documentation = "it's".into()))
            .unwrap();
        let sql = ddl(doc.model(), diagram, Dialect::MySql).unwrap().sql;
        assert!(sql.contains("CREATE TABLE Kunde ("), "{sql}");
        assert!(sql.contains(") ENGINE=InnoDB COMMENT='it''s';"), "{sql}");
        assert!(sql.contains("CREATE TABLE `enthält` ("), "{sql}");
    }

    #[test]
    fn types_get_their_length_back() {
        let t = |datatype: &str, length: &str| {
            let p = ErAttributeProps {
                datatype: datatype.into(),
                length_precision: length.into(),
                ..Default::default()
            };
            datatype_of(&p)
        };
        assert_eq!(t("varchar", "50"), "varchar(50)");
        assert_eq!(t("int unsigned", "10"), "int(10) unsigned");
        assert_eq!(t("timestamp without time zone", "3"), "timestamp(3) without time zone");
        assert_eq!(t("enum('a','b')", ""), "enum('a','b')");
        assert_eq!(t("", ""), "varchar(255)");
    }

    #[test]
    fn names_and_defaults_are_quoted_where_needed() {
        assert_eq!(quote("kunde_id", Dialect::Postgres), "kunde_id");
        assert_eq!(quote("Kunde", Dialect::Postgres), r#""Kunde""#);
        assert_eq!(quote("Kunde", Dialect::MySql), "Kunde");
        assert_eq!(quote("order", Dialect::MySql), "`order`");
        assert_eq!(quote("Fracht Stück", Dialect::MariaDb), "`Fracht Stück`");
        assert_eq!(literal("0.00", Dialect::Postgres), "0.00");
        assert_eq!(literal("current_timestamp", Dialect::MySql), "current_timestamp");
        assert_eq!(literal("now()", Dialect::Postgres), "now()");
        assert_eq!(literal("basis", Dialect::Postgres), "'basis'");
        assert_eq!(literal(r"a\b'c", Dialect::MySql), r"'a\\b''c'");
    }
}
