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
//!   tables does not matter; indexes before them. They take their relationship's `ON DELETE`
//!   and `ON UPDATE`.
//! - Counted columns are `AUTO_INCREMENT` in MySQL (where they lead the key or are unique)
//!   and identity columns in PostgreSQL.
//! - Domains the columns use come first in PostgreSQL (`CREATE DOMAIN`, `CREATE TYPE … AS
//!   ENUM`); MySQL has none, so there a column gets the domain's type (`enum(…)` for an
//!   enumeration), its condition, default and `NOT NULL`. MySQL's `enum(…)` becomes a type of
//!   its own in PostgreSQL, named after table and column.
//!
//! Types and conditions written for the other dialect are translated ([`crate::translate`]);
//! attributes without a type get `varchar(255)`. A generated column's expression does not go
//! from one dialect to the other: the column is a plain one there.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use umlcraft_model::{
    Cardinality, CardinalityMax, CardinalityMin, ElementKind, ErAction, ErAttributeProps,
    ErDomainProps, ErEntityProps, ErIndex, Id, Model, ModelError,
};

use crate::dialect::Dialect;
use crate::keywords::{MYSQL_RESERVED, POSTGRES_RESERVED};
use crate::parse::is_sequence;
use crate::translate;

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
    /// A condition the other dialect cannot read.
    Check { table: String, check: String },
    /// A generated column written for the other dialect: a plain column.
    Generated { table: String, column: String },
    /// A counted column MySQL cannot count, as it does not lead the key or a unique index.
    AutoIncrement { table: String, column: String },
    /// A default, or MySQL's `ON UPDATE`, the dialect has nothing like: the column goes
    /// without it.
    Default { table: String, column: String, default: String },
}

/// A domain of the model, and whether a column uses it.
#[derive(Clone, Debug)]
struct Domain {
    name: String,
    props: ErDomainProps,
    comment: String,
    used: bool,
}

#[derive(Clone, Debug)]
struct Column {
    name: String,
    datatype: String,
    not_null: bool,
    /// The default as the model keeps it, or the expression of a generated column.
    default: String,
    /// The default as the dialect writes it: ` DEFAULT …`, MySQL's ` ON UPDATE …`, or
    /// nothing.
    default_sql: String,
    generated: bool,
    /// The generated column's value is computed when read.
    virtual_column: bool,
    auto_increment: bool,
    unique: bool,
    check: String,
    comment: String,
}

#[derive(Clone, Debug, Default)]
struct Table {
    schema: Option<String>,
    name: String,
    columns: Vec<Column>,
    key: Vec<String>,
    uniques: Vec<Vec<String>>,
    checks: Vec<String>,
    indexes: Vec<ErIndex>,
    comment: String,
}

impl Table {
    fn new(schema: Option<String>, name: String, comment: String) -> Table {
        Table { schema, name, comment, ..Table::default() }
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
    on_delete: Option<ErAction>,
    on_update: Option<ErAction>,
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
    // The domains in the packages of the entities shown and the packages around them, the
    // nearer ones first.
    let mut domains: Vec<Domain> = Vec::new();
    let mut packages: Vec<Id> = Vec::new();
    for &e in &shown {
        let mut owner = model.get(e).and_then(|e| e.owner());
        while let Some(o) = owner {
            if !packages.contains(&o) {
                packages.push(o);
            }
            owner = model.get(o).and_then(|e| e.owner());
        }
    }
    for &p in &packages {
        for &c in model.children(p) {
            let Some(element) = model.get(c) else { continue };
            if let ElementKind::ErDomain(props) = element.kind() {
                domains.push(Domain {
                    name: element.name().to_owned(),
                    props: props.clone(),
                    comment: element.data().documentation.clone(),
                    used: false,
                });
            }
        }
    }
    let mut b = Builder {
        model,
        dialect,
        from: imported_dialect(model, diagram),
        domains,
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
                let before = b.refs.len();
                b.relationship(e, p.identifying, columns_of.get(&e).map(Vec::as_slice));
                for r in &mut b.refs[before..] {
                    (r.on_delete, r.on_update) = (p.on_delete, p.on_update);
                }
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
    /// The dialect the diagram was imported from, if it was.
    from: Option<Dialect>,
    domains: Vec<Domain>,
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
        table.checks = props.checks.clone();
        table.indexes = props.indexes.clone();
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
            let mut column = Column {
                datatype: datatype_of(p),
                not_null: p.not_null || p.key.is_some(),
                default: p.default_value.clone(),
                default_sql: String::new(),
                // The import keeps the expression of a generated column as its default.
                generated: p.derived && !p.default_value.is_empty(),
                virtual_column: p.virtual_column,
                auto_increment: p.auto_increment,
                unique: p.unique,
                check: p.check.clone(),
                comment: element.data().documentation.clone(),
                name,
            };
            if column.generated && self.foreign() {
                column.generated = false;
                column.default.clear();
                let (table, column) = (table.name.clone(), column.name.clone());
                self.left_out.push(LeftOut::Generated { table, column });
            }
            self.resolve_type(&table.name, &mut column, p);
            table.columns.push(column);
        }
    }

    /// Whether the diagram's types and conditions may be another dialect's: it was imported
    /// from one, or drawn.
    fn foreign(&self) -> bool {
        self.from.is_none_or(|f| f.is_mysql() != self.dialect.is_mysql())
    }

    /// The type of `column` for the dialect: a domain's (in PostgreSQL by its name, in MySQL
    /// what it stands for), MySQL's `enum(…)` as a type of its own in PostgreSQL, others
    /// translated.
    fn resolve_type(&mut self, table: &str, column: &mut Column, p: &ErAttributeProps) {
        let d = self.dialect;
        let named =
            self.domains.iter().position(|x| x.name.eq_ignore_ascii_case(p.datatype.trim()));
        if let Some(k) = named {
            if d == Dialect::Postgres {
                self.domains[k].used = true;
                column.datatype = qualified(&self.domains[k].name, d);
                return;
            }
            let domain = self.domains[k].props.clone();
            column.datatype = if domain.values.is_empty() {
                let base = ErAttributeProps {
                    datatype: domain.datatype.clone(),
                    length_precision: domain.length_precision.clone(),
                    ..Default::default()
                };
                translate::datatype(&datatype_of(&base), self.from, d)
            } else {
                let values: Vec<String> = domain.values.iter().map(|v| string(v, d)).collect();
                format!("enum({})", values.join(","))
            };
            column.not_null |= domain.not_null;
            if column.default.is_empty() {
                column.default = domain.default_value.clone();
            }
            if !domain.check.is_empty() {
                let check = with_value(&domain.check, &quote(&column.name, d));
                column.check = if column.check.is_empty() {
                    check
                } else {
                    format!("({}) AND ({check})", column.check)
                };
            }
            return;
        }
        if d == Dialect::Postgres
            && let Some(values) = translate::enum_values(&column.datatype)
        {
            let name = format!("{}_{}", snake(table), snake(&column.name));
            let props = ErDomainProps { values, ..Default::default() };
            self.domains.push(Domain {
                name: name.clone(),
                props,
                comment: String::new(),
                used: true,
            });
            column.datatype = quote(&name, d);
            return;
        }
        column.datatype = translate::datatype(&column.datatype, self.from, d);
    }

    /// A condition for the dialect; `None` (and noted) if it cannot be written there.
    fn condition(&mut self, table: &str, check: &str) -> Option<String> {
        if !self.foreign() {
            return Some(check.to_owned());
        }
        let out = translate::condition(check, self.dialect);
        if out.is_none() {
            let (table, check) = (table.to_owned(), check.to_owned());
            self.left_out.push(LeftOut::Check { table, check });
        }
        out
    }

    /// Types the server would refuse where they stand: a PostgreSQL identity column of
    /// another type than an integer (MySQL's `bigint unsigned`, `numeric(20)` there) becomes
    /// `bigint`; in MySQL a `text` or `blob` column in a key, unique constraint, unique index
    /// or reference becomes `varchar(255)` or `varbinary(255)`, as MySQL keys them only by a
    /// prefix. Written for the other dialect, a foreign key column takes the type of the
    /// column it references, as both servers want them alike.
    fn fit_types(&mut self) {
        let d = self.dialect;
        for t in &mut self.tables {
            for c in &mut t.columns {
                let counted = c.auto_increment || is_sequence(&c.default);
                let integer = matches!(
                    base_type(&c.datatype).as_str(),
                    "smallint" | "integer" | "int" | "bigint" | "int2" | "int4" | "int8"
                );
                if d == Dialect::Postgres && counted && !c.generated && !integer {
                    c.datatype = "bigint".to_owned();
                }
            }
        }
        if d.is_mysql() {
            let mut keyed: Vec<HashSet<String>> = self
                .tables
                .iter()
                .map(|t| {
                    let mut k: HashSet<String> = t.key.iter().cloned().collect();
                    k.extend(t.uniques.iter().flatten().cloned());
                    k.extend(t.columns.iter().filter(|c| c.unique).map(|c| c.name.clone()));
                    let unique_indexes = t.indexes.iter().filter(|i| i.unique).flat_map(|i| {
                        i.columns.iter().filter(|c| prefix_length(c).1.is_none()).cloned()
                    });
                    k.extend(unique_indexes);
                    k
                })
                .collect();
            for r in &self.refs {
                keyed[r.table].extend(r.columns.iter().cloned());
                keyed[r.target].extend(r.target_columns.iter().cloned());
            }
            for (t, keyed) in self.tables.iter_mut().zip(&keyed) {
                for c in t.columns.iter_mut().filter(|c| keyed.contains(&c.name)) {
                    match base_type(&c.datatype).as_str() {
                        "tinytext" | "text" | "mediumtext" | "longtext" => {
                            c.datatype = "varchar(255)".to_owned();
                        }
                        "tinyblob" | "blob" | "mediumblob" | "longblob" => {
                            c.datatype = "varbinary(255)".to_owned();
                        }
                        _ => {}
                    }
                }
            }
        }
        if self.foreign() {
            for r in &self.refs {
                for (column, target) in r.columns.iter().zip(&r.target_columns) {
                    let datatype = self.tables[r.target]
                        .columns
                        .iter()
                        .find(|c| &c.name == target)
                        .map(|c| c.datatype.clone());
                    let column =
                        self.tables[r.table].columns.iter_mut().find(|c| &c.name == column);
                    if let (Some(datatype), Some(column)) = (datatype, column) {
                        column.datatype = datatype;
                    }
                }
            }
        }
    }

    /// ` DEFAULT …` (and MySQL's ` ON UPDATE …`) of a column, or nothing.
    /// A column's default clause, and what of its default the dialect has nothing for.
    fn default_clause(&self, c: &Column) -> (String, Vec<String>) {
        if c.generated || c.auto_increment || is_sequence(&c.default) {
            return (String::new(), Vec::new());
        }
        let (value, on_update) = split_on_update(&c.default);
        let mut out = String::new();
        let mut dropped = Vec::new();
        if !value.is_empty() {
            match self.default_value(value, &c.datatype) {
                Some(v) => {
                    let _ = write!(out, " DEFAULT {v}");
                }
                None => dropped.push(value.to_owned()),
            }
        }
        if let Some(u) = on_update {
            if self.dialect.is_mysql() {
                let _ = write!(out, " ON UPDATE {u}");
            } else {
                dropped.push(format!("ON UPDATE {u}"));
            }
        }
        (out, dropped)
    }

    /// A default (as the model keeps it) for a column or domain of type `datatype` (in the
    /// dialect), as SQL; `None` where the dialect has nothing like it.
    ///
    /// - For the other dialect, a keyword or expression is translated (`uuid()` ↔
    ///   `gen_random_uuid()`, `curdate()` → `CURRENT_DATE`, MySQL's introducers go), or
    ///   left out if it has no counterpart; strings and numbers stay.
    /// - MySQL's zero dates are left out for MySQL (its strict mode refuses them) and
    ///   PostgreSQL; PostgreSQL's `infinity` for MySQL.
    /// - `0` and `1` are `FALSE` and `TRUE` for a PostgreSQL `boolean` (MySQL's
    ///   `tinyint(1)`).
    /// - MySQL takes a literal for a `text`, `blob`, `json` or spatial column and an
    ///   expression other than the current time only in parentheses.
    fn default_value(&self, value: &str, datatype: &str) -> Option<String> {
        let d = self.dialect;
        let base = base_type(datatype);
        let mut v = Value::of(value);
        if let Value::Word(e) | Value::Expr(e) = &v
            && self.foreign()
        {
            v = Value::of(&translate::default_expression(e, self.from, d, datatype)?);
        }
        if let Value::Str(s) = &v {
            let lower = s.to_ascii_lowercase();
            if s.starts_with("0000-00-00") && d != Dialect::MariaDb
                || d.is_mysql() && matches!(lower.as_str(), "infinity" | "-infinity")
            {
                return None;
            }
        }
        if d == Dialect::Postgres && matches!(base.as_str(), "boolean" | "bool") {
            let bit = match &v {
                Value::Num(n) | Value::Str(n) => n.as_str(),
                _ => "",
            };
            match bit {
                "0" => return Some("FALSE".to_owned()),
                "1" => return Some("TRUE".to_owned()),
                _ => {}
            }
        }
        let literal = matches!(v, Value::Str(_) | Value::Num(_) | Value::Bits(_));
        let sql = match v {
            Value::Str(s) => string(&s, d),
            Value::Num(s) | Value::Bits(s) | Value::Word(s) | Value::Expr(s) => s,
        };
        if !d.is_mysql() {
            return Some(sql);
        }
        const NO_LITERAL: [&str; 16] = [
            "tinytext",
            "text",
            "mediumtext",
            "longtext",
            "tinyblob",
            "blob",
            "mediumblob",
            "longblob",
            "json",
            "geometry",
            "point",
            "linestring",
            "polygon",
            "multipoint",
            "multilinestring",
            "multipolygon",
        ];
        let lower = sql.to_ascii_lowercase();
        let call = lower.split('(').next().unwrap_or_default().trim();
        let bare = if literal {
            !NO_LITERAL.contains(&base.as_str()) && !base.starts_with("geom")
        } else {
            matches!(lower.as_str(), "null" | "true" | "false")
                || matches!(call, "current_timestamp" | "now" | "localtime" | "localtimestamp")
                    && matches!(base.as_str(), "timestamp" | "datetime")
                || sql.starts_with('(') && translate::matching(&sql, 0) == Some(sql.len() - 1)
        };
        Some(if bare { sql } else { format!("({sql})") })
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
                default_sql: String::new(),
                generated: false,
                virtual_column: false,
                auto_increment: false,
                unique: false,
                check: String::new(),
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
        self.refs.push(Reference {
            table,
            columns,
            target,
            target_columns,
            on_delete: None,
            on_update: None,
        });
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
        self.refs.push(Reference {
            table,
            columns,
            target,
            target_columns,
            on_delete: None,
            on_update: None,
        });
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

    fn write(mut self, diagram: &str) -> Ddl {
        let d = self.dialect;
        // The conditions as the dialect writes them.
        for t in 0..self.tables.len() {
            let name = self.tables[t].name.clone();
            let checks = std::mem::take(&mut self.tables[t].checks);
            self.tables[t].checks =
                checks.iter().filter_map(|c| self.condition(&name, c)).collect();
            for c in 0..self.tables[t].columns.len() {
                let table = &mut self.tables[t];
                let column = &mut table.columns[c];
                let leads = table.key.first() == Some(&column.name) || column.unique;
                if d.is_mysql() && column.auto_increment && !leads {
                    column.auto_increment = false;
                    let (table, column) = (name.clone(), column.name.clone());
                    self.left_out.push(LeftOut::AutoIncrement { table, column });
                }
                let check = std::mem::take(&mut self.tables[t].columns[c].check);
                if !check.is_empty() {
                    let check = self.condition(&name, &check).unwrap_or_default();
                    self.tables[t].columns[c].check = check;
                }
            }
        }
        self.fit_types();
        for t in 0..self.tables.len() {
            for c in 0..self.tables[t].columns.len() {
                let (clause, dropped) = self.default_clause(&self.tables[t].columns[c]);
                self.tables[t].columns[c].default_sql = clause;
                for default in dropped {
                    let table = self.tables[t].name.clone();
                    let column = self.tables[t].columns[c].name.clone();
                    self.left_out.push(LeftOut::Default { table, column, default });
                }
            }
        }
        let mut domains = std::mem::take(&mut self.domains);
        domains.retain(|x| x.used);
        for x in &mut domains {
            if !x.props.check.is_empty() {
                x.props.check = self.condition(&x.name, &x.props.check).unwrap_or_default();
            }
        }
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
        if !domains.is_empty() {
            sql.push('\n');
        }
        for x in &domains {
            let name = qualified(&x.name, d);
            let p = &x.props;
            let what = if p.values.is_empty() {
                let base = ErAttributeProps {
                    datatype: p.datatype.clone(),
                    length_precision: p.length_precision.clone(),
                    ..Default::default()
                };
                let datatype = translate::datatype(&datatype_of(&base), self.from, d);
                let mut s = format!("CREATE DOMAIN {name} AS {datatype}");
                if !p.default_value.is_empty()
                    && let Some(v) = self.default_value(&p.default_value, &datatype)
                {
                    let _ = write!(s, " DEFAULT {v}");
                }
                if p.not_null {
                    s.push_str(" NOT NULL");
                }
                if !p.check.is_empty() {
                    let _ = write!(s, " CHECK ({})", p.check);
                }
                let _ = writeln!(sql, "{s};");
                "DOMAIN"
            } else {
                let values: Vec<String> = p.values.iter().map(|v| string(v, d)).collect();
                let _ = writeln!(sql, "CREATE TYPE {name} AS ENUM ({});", values.join(", "));
                "TYPE"
            };
            if !x.comment.is_empty() {
                let _ = writeln!(sql, "COMMENT ON {what} {name} IS {};", string(&x.comment, d));
            }
        }
        for t in &self.tables {
            let name = table_name(t, d);
            let _ = write!(sql, "\nCREATE TABLE {name} (");
            let mut lines: Vec<String> = t.columns.iter().map(|c| column(c, t, d)).collect();
            if !t.key.is_empty() {
                let key: Vec<String> = t.key.iter().map(|k| quote(k, d)).collect();
                lines.push(format!("PRIMARY KEY ({})", key.join(", ")));
            }
            for u in &t.uniques {
                let u: Vec<String> = u.iter().map(|c| quote(c, d)).collect();
                lines.push(format!("UNIQUE ({})", u.join(", ")));
            }
            for c in &t.checks {
                lines.push(format!("CHECK ({c})"));
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
        // Names of indexes and constraints, by schema: PostgreSQL keeps indexes in one
        // namespace with the tables, so a name from MySQL (where an index's name is the
        // table's own) must not be a table's.
        let mut names: HashSet<(String, String)> = HashSet::new();
        if !d.is_mysql() {
            names.extend(self.tables.iter().map(|t| (schema_of(t), t.name.clone())));
        }
        let mut name_of = |t: &Table, base: String| {
            let mut base = base;
            if base.len() > NAME_LIMIT {
                let cut = (0..=NAME_LIMIT).rev().find(|&i| base.is_char_boundary(i)).unwrap_or(0);
                base.truncate(cut);
            }
            let schema = schema_of(t);
            let taken = |n: &String| names.contains(&(schema.clone(), n.clone()));
            let name = if taken(&base) {
                (2..).map(|n| format!("{base}{n}")).find(|n| !taken(n)).expect("unbounded")
            } else {
                base
            };
            names.insert((schema, name.clone()));
            name
        };
        // MySQL indexes no JSON column.
        let indexable = |t: &Table, i: &ErIndex| {
            !d.is_mysql()
                || i.columns.iter().all(|c| {
                    let (name, _) = prefix_length(c);
                    t.columns.iter().find(|x| x.name == name).is_none_or(|x| {
                        !matches!(base_type(&x.datatype).as_str(), "json" | "jsonb")
                    })
                })
        };
        let indexes: Vec<(&Table, &ErIndex)> = self
            .tables
            .iter()
            .flat_map(|t| t.indexes.iter().map(move |i| (t, i)))
            .filter(|(t, i)| !i.columns.is_empty() && !(i.unique && i.columns == t.key))
            .filter(|(t, i)| indexable(t, i))
            .collect();
        if !indexes.is_empty() {
            sql.push('\n');
        }
        for (t, i) in indexes {
            let columns: Vec<String> = i.columns.iter().map(|c| index_column(t, c, d)).collect();
            let prefix = if i.unique { "uq" } else { "ix" };
            let base = if i.name.is_empty() {
                let plain: Vec<&str> = i.columns.iter().map(|c| prefix_length(c).0).collect();
                format!("{prefix}_{}_{}", snake(&t.name), snake(&plain.join("_")))
            } else {
                i.name.clone()
            };
            let name = name_of(t, base);
            let unique = if i.unique { "UNIQUE " } else { "" };
            let _ = writeln!(
                sql,
                "CREATE {unique}INDEX {} ON {} ({});",
                quote(&name, d),
                table_name(t, d),
                columns.join(", ")
            );
        }
        if !self.refs.is_empty() {
            sql.push('\n');
        }
        for r in &self.refs {
            let (t, target) = (&self.tables[r.table], &self.tables[r.target]);
            let key: Vec<String> = r.target_columns.iter().map(|k| quote(k, d)).collect();
            let columns: Vec<String> = r.columns.iter().map(|c| quote(c, d)).collect();
            let name = name_of(t, format!("fk_{}_{}", snake(&t.name), snake(&r.columns.join("_"))));
            let mut actions = String::new();
            if let Some(a) = r.on_delete {
                let _ = write!(actions, " ON DELETE {}", a.sql());
            }
            if let Some(a) = r.on_update {
                let _ = write!(actions, " ON UPDATE {}", a.sql());
            }
            let _ = writeln!(
                sql,
                "ALTER TABLE {} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}){actions};",
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

fn column(c: &Column, t: &Table, d: Dialect) -> String {
    let mut out = format!("{} {}", quote(&c.name, d), c.datatype);
    // A counted column; a sequence of the dump's own (as models imported before kept it)
    // too: the table gets one of its own instead.
    let sequence = c.auto_increment || is_sequence(&c.default);
    if c.generated {
        let how = if c.virtual_column { "VIRTUAL" } else { "STORED" };
        let _ = write!(out, " GENERATED ALWAYS AS ({}) {how}", c.default);
    }
    if c.not_null {
        out.push_str(" NOT NULL");
    }
    if c.generated {
        // Its expression is written above.
    } else if sequence && d == Dialect::Postgres {
        out.push_str(" GENERATED BY DEFAULT AS IDENTITY");
    } else if sequence {
        // MySQL counts only a column that leads an index.
        if t.key.first() == Some(&c.name) || c.unique {
            out.push_str(" AUTO_INCREMENT");
        }
    } else {
        out.push_str(&c.default_sql);
    }
    if c.unique {
        out.push_str(" UNIQUE");
    }
    if !c.check.is_empty() {
        let _ = write!(out, " CHECK ({})", c.check);
    }
    if d.is_mysql() && !c.comment.is_empty() {
        let _ = write!(out, " COMMENT {}", string(&c.comment, d));
    }
    out
}

/// A condition of a domain for one column: `VALUE` is the column (outside strings).
fn with_value(check: &str, column: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let mut in_string = false;
    let flush = |word: &mut String, out: &mut String| {
        if word.eq_ignore_ascii_case("value") {
            out.push_str(column);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in check.chars() {
        if !in_string && (c.is_alphanumeric() || c == '_') {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        if c == '\'' {
            in_string = !in_string;
        }
        out.push(c);
    }
    flush(&mut word, &mut out);
    out
}

/// A name that may have a schema in front: `fracht.Typ` → `fracht."Typ"` in PostgreSQL.
fn qualified(name: &str, d: Dialect) -> String {
    match name.split_once('.') {
        Some((s, n)) if d == Dialect::Postgres && !s.is_empty() && !n.is_empty() => {
            format!("{}.{}", quote(s, d), quote(n, d))
        }
        _ => quote(name, d),
    }
}

fn table_name(t: &Table, d: Dialect) -> String {
    match &t.schema {
        Some(s) => format!("{}.{}", quote(s, d), quote(&t.name, d)),
        None => quote(&t.name, d),
    }
}

/// A default as the model keeps it (and the import writes it): a string in single quotes
/// (`'007'`, `'it''s'`), or without them where it cannot be read as anything else; a
/// number, MySQL's `b'0'` or `x'ff'`, a keyword (`NULL`, `CURRENT_TIMESTAMP` …) or an
/// expression (anything with parentheses, a cast or brackets).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) enum Value {
    /// A string's value.
    Str(String),
    Num(String),
    Bits(String),
    Word(String),
    Expr(String),
}

impl Value {
    pub(crate) fn of(text: &str) -> Value {
        if let Some(s) = unquoted(text) {
            return Value::Str(s);
        }
        let v = text.trim();
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
            "SESSION_USER",
        ];
        let bits = v.len() >= 3
            && matches!(v.as_bytes()[0], b'b' | b'B' | b'x' | b'X')
            && unquoted(&v[1..]).is_some();
        if bits {
            Value::Bits(v.to_owned())
        } else if is_number(v) {
            Value::Num(v.to_owned())
        } else if KEYWORDS.iter().any(|k| v.eq_ignore_ascii_case(k)) {
            Value::Word(v.to_owned())
        } else if v.contains('(') || v.contains("::") || v.contains('[') {
            Value::Expr(v.to_owned())
        } else {
            Value::Str(v.to_owned())
        }
    }
}

/// The value of an SQL string in single quotes (doubled quotes inside), if `text` is one.
fn unquoted(text: &str) -> Option<String> {
    let inner = text.strip_prefix('\'')?.strip_suffix('\'')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\'' && chars.next() != Some('\'') {
            return None;
        }
        out.push(c);
    }
    Some(out)
}

/// A number as SQL writes it: `12`, `-0.5`, `.5`, `1e3` (not Rust's `inf` or `NaN`).
fn is_number(v: &str) -> bool {
    let v = v.strip_prefix(['-', '+']).unwrap_or(v);
    let (mantissa, exponent) = match v.find(['e', 'E']) {
        Some(at) => (&v[..at], Some(&v[at + 1..])),
        None => (v, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((a, b)) => {
            (digits(a) || a.is_empty()) && (digits(b) || b.is_empty()) && a.len() + b.len() > 0
        }
        None => digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    mantissa_ok && exponent_ok
}

/// How the model keeps a default the import read as SQL (`sql`, strings in quotes) for a
/// column of type `datatype`: a string without its quotes where it reads back as the same
/// string (`'A1'` → `A1`, but `'007'`, `'(none)'`, `'NULL'`, `''` and `' '` keep them), a
/// number in quotes for a numeric column without them (`'0'` → `0`, as mysqldump writes
/// them); anything else as it is.
pub(crate) fn model_default(sql: &str, datatype: &str) -> String {
    let Some(s) = unquoted(sql) else { return sql.to_owned() };
    let base = base_type(datatype);
    let numeric = matches!(
        base.split(' ').next().unwrap_or_default(),
        "tinyint"
            | "smallint"
            | "mediumint"
            | "int"
            | "integer"
            | "bigint"
            | "int2"
            | "int4"
            | "int8"
            | "decimal"
            | "dec"
            | "numeric"
            | "fixed"
            | "float"
            | "float4"
            | "float8"
            | "double"
            | "real"
    );
    let plain = !s.is_empty() && Value::of(&s) == Value::Str(s.clone());
    if plain || numeric && is_number(&s) { s } else { sql.to_owned() }
}

/// The lower-case name of a type without its length and what follows: `varchar(20)` →
/// `varchar`, `timestamp(3) without time zone` → `timestamp`, `int unsigned` → `int
/// unsigned`.
pub(crate) fn base_type(datatype: &str) -> String {
    let t = datatype.split('(').next().unwrap_or_default();
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase()
}

/// A default and what MySQL's `ON UPDATE` sets the column to, as the model keeps them
/// together (`CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP`, as MySQL Workbench shows them).
fn split_on_update(default: &str) -> (&str, Option<&str>) {
    let lower = default.to_ascii_lowercase();
    let mut in_string = false;
    for (k, c) in default.char_indices() {
        if c == '\'' {
            in_string = !in_string;
        }
        let boundary = k == 0 || default[..k].ends_with(char::is_whitespace);
        if !in_string && boundary && lower[k..].starts_with("on update") {
            let rest = default[k + "on update".len()..].trim();
            if !rest.is_empty() {
                return (default[..k].trim(), Some(rest));
            }
        }
    }
    (default.trim(), None)
}

/// A column with MySQL's prefix length split off: `body(100)` → (`body`, `100`).
fn prefix_length(column: &str) -> (&str, Option<&str>) {
    if let Some(rest) = column.strip_suffix(')')
        && let Some((name, length)) = rest.rsplit_once('(')
        && !name.is_empty()
        && !length.is_empty()
        && length.bytes().all(|b| b.is_ascii_digit())
    {
        return (name, Some(length));
    }
    (column, None)
}

/// A column of an index: in MySQL with its prefix length, one of a `text` or `blob` column
/// without one with 255 (MySQL indexes those only by their start); PostgreSQL indexes all of
/// it.
fn index_column(t: &Table, column: &str, d: Dialect) -> String {
    let (name, length) = prefix_length(column);
    if !d.is_mysql() {
        return quote(name, d);
    }
    let long = t.columns.iter().find(|c| c.name == name).is_some_and(|c| {
        matches!(
            base_type(&c.datatype).as_str(),
            "tinytext"
                | "text"
                | "mediumtext"
                | "longtext"
                | "tinyblob"
                | "blob"
                | "mediumblob"
                | "longblob"
        )
    });
    match length {
        Some(n) => format!("{}({n})", quote(name, d)),
        None if long => format!("{}(255)", quote(name, d)),
        None => quote(name, d),
    }
}

/// The schema a table is in, for the names of its indexes.
fn schema_of(t: &Table) -> String {
    t.schema.clone().unwrap_or_else(|| "public".to_owned())
}

fn string(text: &str, d: Dialect) -> String {
    let mut escaped = text.replace('\'', "''");
    if d.is_mysql() {
        escaped = escaped.replace('\\', "\\\\");
    }
    format!("'{escaped}'")
}

/// A name, quoted where it has to be: with backticks in MySQL, double quotes in PostgreSQL,
/// where it is not a plain name (in PostgreSQL also where it has capitals, which only quotes
/// keep) or a reserved word of the dialect.
fn quote(name: &str, d: Dialect) -> String {
    let mut chars = name.chars();
    let plain_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || (d.is_mysql() && c == '$');
    let reserved = if d.is_mysql() { MYSQL_RESERVED } else { POSTGRES_RESERVED };
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(plain_char)
        && (d.is_mysql() || !name.chars().any(|c| c.is_ascii_uppercase()))
        && !reserved.contains(&name.to_ascii_lowercase().as_str());
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
        let kind = ElementKind::ErRelationship(ErRelationshipProps::new(identifying));
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
    fn a_letter_as_the_maximum_is_many_like_n() {
        let (doc, diagram) = shop();
        let with_n = ddl(doc.model(), diagram, Dialect::Postgres).unwrap().sql;
        let (mut doc, diagram) = shop();
        let parts: Vec<Id> = doc
            .model()
            .elements()
            .filter(|e| matches!(e.kind(), ElementKind::ErParticipation(_)))
            .map(|e| e.id())
            .collect();
        let mut renamed = 0;
        doc.edit("letters", |tx| {
            for (i, part) in parts.into_iter().enumerate() {
                tx.update(part, |data| {
                    if let ElementKind::ErParticipation(p) = &mut data.kind
                        && let Some(c) = &mut p.cardinality
                        && c.max == CardinalityMax::N
                    {
                        c.max = CardinalityMax::Many(if i % 2 == 0 { 'p' } else { 'q' });
                        renamed += 1;
                    }
                })?;
            }
            Ok(())
        })
        .unwrap();
        assert!(renamed >= 6, "{renamed}");
        // `[0,p]` and `[1,q]` make the same tables and foreign keys as `[0,n]`.
        assert_eq!(ddl(doc.model(), diagram, Dialect::Postgres).unwrap().sql, with_n);
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
        // MySQL takes the current date as a default only in parentheses.
        assert!(sql.contains("  datum date DEFAULT (CURRENT_DATE),"), "{sql}");
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
        let same = |value: &str, datatype: &str, d: Dialect| default(value, datatype, Some(d), d);
        assert_eq!(same("0.00", "numeric", Dialect::Postgres).as_deref(), Some("0.00"));
        let now = same("current_timestamp", "timestamp", Dialect::MySql);
        assert_eq!(now.as_deref(), Some("current_timestamp"));
        assert_eq!(same("now()", "timestamp", Dialect::Postgres).as_deref(), Some("now()"));
        assert_eq!(same("basis", "varchar", Dialect::Postgres).as_deref(), Some("'basis'"));
        assert_eq!(same(r"a\b'c", "varchar", Dialect::MySql).as_deref(), Some(r"'a\\b''c'"));
    }

    /// The default `value` of a column of type `datatype` (in `to`) from `from` for `to`.
    fn default(value: &str, datatype: &str, from: Option<Dialect>, to: Dialect) -> Option<String> {
        let model = Model::new("m");
        let b = Builder {
            model: &model,
            dialect: to,
            from,
            domains: Vec::new(),
            tables: Vec::new(),
            of: HashMap::new(),
            refs: Vec::new(),
            left_out: Vec::new(),
            pairs: HashMap::new(),
        };
        b.default_value(value, datatype)
    }

    #[test]
    fn reserved_words_of_each_dialect_are_quoted() {
        for (name, mysql, postgres) in [
            ("read", "`read`", "read"),
            ("rank", "`rank`", "rank"),
            ("values", "`values`", "values"),
            ("left", "`left`", r#""left""#),
            ("offset", "`offset`", r#""offset""#),
            ("window", "`window`", r#""window""#),
            ("user", "user", r#""user""#),
            ("array", "array", r#""array""#),
            ("verbose", "verbose", r#""verbose""#),
            ("Rank", "`Rank`", r#""Rank""#),
            ("name", "name", "name"),
        ] {
            assert_eq!(quote(name, Dialect::MySql), mysql, "{name}");
            assert_eq!(quote(name, Dialect::Postgres), postgres, "{name}");
        }
    }

    #[test]
    fn defaults_keep_strings_apart_from_numbers_keywords_and_expressions() {
        let pg =
            |value: &str| default(value, "varchar(20)", Some(Dialect::Postgres), Dialect::Postgres);
        // Quoted, a string stays one, whatever it looks like.
        assert_eq!(pg("'(none)'").as_deref(), Some("'(none)'"));
        assert_eq!(pg("'007'").as_deref(), Some("'007'"));
        assert_eq!(pg("'NULL'").as_deref(), Some("'NULL'"));
        assert_eq!(pg("' '").as_deref(), Some("' '"));
        assert_eq!(pg("''").as_deref(), Some("''"));
        assert_eq!(pg("'it''s'").as_deref(), Some("'it''s'"));
        // Without quotes: Rust's `inf` and `NaN` are no numbers in SQL.
        assert_eq!(pg("infinity").as_deref(), Some("'infinity'"));
        assert_eq!(pg("NaN").as_deref(), Some("'NaN'"));
        assert_eq!(pg("1e3").as_deref(), Some("1e3"));
        assert_eq!(pg("ARRAY[]::text[]").as_deref(), Some("ARRAY[]::text[]"));
        // The import keeps quotes only where they are needed.
        assert_eq!(model_default("'A1'", "varchar(8)"), "A1");
        assert_eq!(model_default("'0'", "tinyint(1)"), "0");
        assert_eq!(model_default("'0'", "varchar(8)"), "'0'");
        assert_eq!(model_default("'(none)'", "varchar(8)"), "'(none)'");
        assert_eq!(model_default("''", "text"), "''");
        assert_eq!(model_default("now()", "timestamp"), "now()");
    }

    #[test]
    fn defaults_for_the_other_dialect_are_ones_it_takes() {
        let to_pg = |value: &str, datatype: &str| {
            default(value, datatype, Some(Dialect::MySql), Dialect::Postgres)
        };
        assert_eq!(to_pg("0", "boolean").as_deref(), Some("FALSE"));
        assert_eq!(to_pg("'1'", "boolean").as_deref(), Some("TRUE"));
        assert_eq!(to_pg("(uuid())", "char(36)").as_deref(), Some("gen_random_uuid()"));
        assert_eq!(to_pg("(_utf8mb4'x')", "varchar(20)").as_deref(), Some("'x'"));
        assert_eq!(to_pg("(curdate())", "date").as_deref(), Some("CURRENT_DATE"));
        assert_eq!(to_pg("current_timestamp()", "timestamp").as_deref(), Some("CURRENT_TIMESTAMP"));
        assert_eq!(to_pg("'0000-00-00 00:00:00'", "timestamp"), None);
        assert_eq!(to_pg("(rand())", "integer"), None);
        let to_my = |value: &str, datatype: &str| {
            default(value, datatype, Some(Dialect::Postgres), Dialect::MySql)
        };
        assert_eq!(to_my("''", "text").as_deref(), Some("('')"));
        assert_eq!(to_my("{}", "json").as_deref(), Some("('{}')"));
        assert_eq!(to_my("ARRAY[]::text[]", "json"), None);
        assert_eq!(to_my("infinity", "datetime"), None);
        assert_eq!(to_my("gen_random_uuid()", "char(36)").as_deref(), Some("(uuid())"));
        assert_eq!(to_my("now()", "datetime(6)").as_deref(), Some("CURRENT_TIMESTAMP(6)"));
        assert_eq!(to_my("CURRENT_DATE", "date").as_deref(), Some("(CURRENT_DATE)"));
        assert_eq!(to_my("true", "boolean").as_deref(), Some("TRUE"));
        // MySQL's zero date is a value MariaDB takes.
        let maria = default("'0000-00-00'", "date", Some(Dialect::MySql), Dialect::MariaDb);
        assert_eq!(maria.as_deref(), Some("'0000-00-00'"));
    }

    #[test]
    fn on_update_goes_with_the_default_in_mysql_only() {
        assert_eq!(
            split_on_update("CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP"),
            ("CURRENT_TIMESTAMP", Some("CURRENT_TIMESTAMP"))
        );
        assert_eq!(split_on_update("ON UPDATE now()"), ("", Some("now()")));
        assert_eq!(split_on_update("'on update x'"), ("'on update x'", None));
    }
}
