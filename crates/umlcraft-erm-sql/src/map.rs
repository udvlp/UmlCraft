//! The tables of a dump as a Chen diagram in the model, by the rules S1–S11 in their order
//! (S1, partitions, is done by the parser; S2 same foreign key twice → once; S3–S11 below).
//!
//! - A junction table (its primary key is the columns of two or more foreign keys, nothing
//!   else refers to it) becomes a relationship between the tables it joins (S3).
//! - A table whose key is a foreign key plus columns of its own becomes a weak entity with an
//!   identifying relationship (S4); every other table an entity (S5).
//! - Columns become attributes, except those of foreign keys: they are the relationships
//!   (S6, S7). Cardinalities are read look-across: `[1,1]` at the referenced table says that
//!   each row of the referencing one has exactly one. The second end of a junction table's
//!   relationship is `[0,m]` (`[0,n]` to `[0,m]`).
//!
//! Everything goes into a new package named after the database, with an ER diagram of the
//! same name that shows it, laid out by [`er_layout`]. PostgreSQL's domains and enumerated
//! types become ER domains in the package, which the columns name as their datatype; checks
//! and indexes go with their columns and tables (not the indexes MySQL makes for each foreign
//! key by itself).

use std::collections::{BTreeMap, HashMap, HashSet};

use umlcraft_model::{
    Cardinality, CardinalityMax, DiagramKind, ElementData, ElementKind, ErAttributeProps,
    ErDomainProps, ErEntityProps, ErIndex, ErKey, ErNotation, ErParticipationProps,
    ErRelationshipProps, Id, Rect, Result, Tx,
};
use umlcraft_notation::er_layout;
use umlcraft_notation::layout::Direction;
use umlcraft_notation::palette::default_size_for;
use umlcraft_notation::scene::TextMeasure;

use crate::dialect::Dialect;
use crate::ir::{Column, ForeignKey, Problem, Schema, Table, TableKey};

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
    /// The dump's file name, for the package's name (if the dump names no database) and its
    /// documentation.
    pub file_name: Option<String>,
    /// How the diagram draws the tables: Chen (by default), IE or IDEF1X.
    pub notation: ErNotation,
}

/// Where the import went and what it found.
#[derive(Clone, PartialEq, Debug)]
pub struct Import {
    /// The new package, and the ER diagram in it.
    pub package: Id,
    pub diagram: Id,
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
    pub fn element(&self) -> Option<Id> {
        match self {
            Self::NoPrimaryKey { element, .. } | Self::NotInDump { element, .. } => Some(*element),
            Self::Recoded { .. } => None,
        }
    }
}

/// The documentation a placeholder entity gets.
pub const PLACEHOLDER_COMMENT: &str = "not in the dump";

/// Maps the tables into a new package under `owner` and lays out its diagram, with the text
/// widths of `measure`.
pub fn import(
    tx: &mut Tx<'_>,
    owner: Id,
    schema: &Schema,
    options: &Options,
    measure: &dyn TextMeasure,
) -> Result<Import> {
    let result = map(tx, owner, schema, options)?;
    if options.notation != ErNotation::Chen {
        umlcraft_notation::er::set_notation(tx, result.diagram, options.notation, measure)?;
    }
    // Tables as in IE and IDEF1X: the referenced ones above those that refer to them.
    er_layout::arrange(tx, result.diagram, Direction::Down, measure)?;
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Entity,
    Weak,
    Junction,
}

/// Maps without laying out: the nodes have the palette's sizes, all at (0, 0).
pub fn map(tx: &mut Tx<'_>, owner: Id, schema: &Schema, options: &Options) -> Result<Import> {
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

    let name = package_name(schema, options);
    let origin = match &options.file_name {
        Some(f) => format!("SQL: {f} ({})", schema.dialect),
        None => format!("SQL ({})", schema.dialect),
    };
    let mut data = ElementData::new(name.clone(), ElementKind::Package);
    data.documentation = origin;
    let package = tx.create(owner, data)?;
    let kind = ElementKind::diagram(DiagramKind::EntityRelationship);
    let diagram = tx.create(package, ElementData::new(name, kind))?;
    // Domains and enumerated types, by the ways a column may spell them.
    let mut types = HashMap::new();
    for (key, t) in &schema.types {
        if options.schemas.as_ref().is_some_and(|s| !s.contains(&key.schema)) {
            continue;
        }
        let (datatype, length) = split_type(&t.datatype);
        let props = ErDomainProps {
            datatype,
            length_precision: length.unwrap_or_default(),
            values: t.values.clone(),
            not_null: t.not_null,
            default_value: t.default.clone().unwrap_or_default(),
            check: t.check.clone().unwrap_or_default(),
        };
        let mut data = ElementData::new(key.to_string(), ElementKind::ErDomain(props));
        data.documentation = t.comment.clone().unwrap_or_default();
        tx.create(package, data)?;
        types.insert(format!("{}.{}", key.schema, key.name), key.to_string());
        types.insert(key.to_string(), key.to_string());
    }
    let mut m = Mapper {
        tx,
        package,
        diagram,
        entities: HashMap::new(),
        hints: Vec::new(),
        attributes: options.attributes,
        counts: [0; 3],
        types,
    };
    for (t, class) in tables.iter().zip(&classes) {
        if *class == Class::Junction {
            continue;
        }
        let (id, _) = m.entity(&t.key, *class == Class::Weak, t.comment.clone())?;
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
            let all: Vec<&ForeignKey> = fks.iter().collect();
            let r = m.relationship(&t.key.to_string(), false, &all, t.comment.clone())?;
            for (k, fk) in fks.iter().enumerate() {
                let role = (fks.iter().filter(|f| f.target == fk.target).count() > 1)
                    .then(|| relationship_name(&fk.columns, ""));
                let target = m.target(&fk.target)?;
                // `[0,n]` to `[0,m]`; a third and further ends are `n` again.
                let max = if k == 1 { CardinalityMax::ManyM } else { CardinalityMax::Many };
                m.join(r, target, Cardinality::new(0, max), role, false)?;
            }
            m.attributes(r, t, &own, &[], &[])?;
            continue;
        }
        let owner = m.entities[&t.key];
        m.table_rules(owner.0, t, fks)?;
        let in_pk: Vec<&ForeignKey> = if *class == Class::Weak { in_key(t, fks) } else { vec![] };
        if *class == Class::Weak {
            let id_cols: HashSet<&str> =
                in_pk.iter().flat_map(|fk| fk.columns.iter().map(String::as_str)).collect();
            let partial: Vec<&str> =
                t.primary_key.iter().map(String::as_str).filter(|c| !id_cols.contains(c)).collect();
            m.attributes(owner, t, &own, &[], &partial)?;
        } else {
            let keys: Vec<&str> =
                t.primary_key.iter().map(String::as_str).filter(|c| !fk_cols.contains(c)).collect();
            m.attributes(owner, t, &own, &keys, &[])?;
        }
        for fk in fks {
            let identifying = in_pk.contains(&fk);
            let other = m.target(&fk.target)?;
            let r = m.relationship(
                &relationship_name(&fk.columns, &fk.target.name),
                identifying,
                &[fk],
                None,
            )?;
            m.foreign_columns(owner.0, t, fk, r.0)?;
            let nullable = fk.columns.iter().any(|c| !t.column(c).is_some_and(|c| c.not_null));
            let near_target =
                Cardinality::new(if nullable { 0 } else { 1 }, CardinalityMax::Count(1));
            let near_source = if unique(t, fk) {
                Cardinality::new(0, CardinalityMax::Count(1))
            } else {
                Cardinality::new(0, CardinalityMax::Many)
            };
            if fk.target == t.key {
                // S8: the line of the referenced side carries the role.
                let role = relationship_name(&fk.columns, "");
                m.join(r, other, near_target, Some(role), false)?;
                m.join(r, owner, near_source, None, false)?;
            } else {
                // A weak entity takes part totally in its identifying relationship.
                m.join(r, other, near_target, None, false)?;
                m.join(r, owner, near_source, None, identifying)?;
            }
        }
        // The foreign key columns came last; the table's order is the order of its keys, which
        // MySQL's foreign keys to them must follow.
        let mut index = 0;
        for c in &t.columns {
            let named = m.tx.model().children_named(owner.0, &c.name);
            if let Some(&column) = named.first() {
                if m.tx.model().children(owner.0).get(index) != Some(&column) {
                    m.tx.move_element(column, owner.0, index)?;
                }
                index += 1;
            }
        }
    }
    let [entities, relationships, attributes] = m.counts;
    let mut hints = m.hints;
    hints.extend(schema.recoded.iter().map(|&line| Hint::Recoded { line }));
    Ok(Import {
        package,
        diagram,
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
    })
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

/// The database's name if the dump names it, else the file's without its extension.
fn package_name(schema: &Schema, options: &Options) -> String {
    if let Some(db) = &schema.database {
        return db.clone();
    }
    let stem = options.file_name.as_deref().map(|f| match f.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => f,
    });
    stem.unwrap_or("SQL").to_owned()
}

struct Mapper<'a, 'b> {
    tx: &'a mut Tx<'b>,
    package: Id,
    diagram: Id,
    /// Each table's entity and its node.
    entities: HashMap<TableKey, (Id, Id)>,
    hints: Vec<Hint>,
    attributes: Attributes,
    /// Entities, relationships, attributes made.
    counts: [usize; 3],
    /// The domains' names by the ways a column's datatype may spell them.
    types: HashMap<String, String>,
}

impl Mapper<'_, '_> {
    /// A new element owned by `owner`, shown on the diagram: (element, node).
    fn node(&mut self, owner: Id, data: ElementData) -> Result<(Id, Id)> {
        let size = default_size_for(&data.kind);
        let element = self.tx.create(owner, data)?;
        let bounds = Rect::new(0.0, 0.0, size.width, size.height);
        let node = self.tx.add_node(self.diagram, Some(element), bounds)?;
        Ok((element, node))
    }

    fn entity(&mut self, key: &TableKey, weak: bool, comment: Option<String>) -> Result<(Id, Id)> {
        let props = ErEntityProps { weak, physical_name: key.to_string(), ..Default::default() };
        let mut data = ElementData::new(entity_name(key), ElementKind::ErEntity(props));
        data.documentation = comment.unwrap_or_default();
        let ids = self.node(self.package, data)?;
        self.entities.insert(key.clone(), ids);
        self.counts[0] += 1;
        Ok(ids)
    }

    /// A relationship for foreign keys `fks`, with their actions if they all have the same.
    fn relationship(
        &mut self,
        name: &str,
        identifying: bool,
        fks: &[&ForeignKey],
        comment: Option<String>,
    ) -> Result<(Id, Id)> {
        let actions = |fk: &&ForeignKey| (fk.on_delete, fk.on_update);
        let first = fks.first().map(actions).unwrap_or_default();
        let (on_delete, on_update) =
            if fks.iter().all(|fk| actions(fk) == first) { first } else { (None, None) };
        let props = ErRelationshipProps { identifying, on_delete, on_update };
        let kind = ElementKind::ErRelationship(props);
        let mut data = ElementData::new(name, kind);
        data.documentation = comment.unwrap_or_default();
        self.counts[1] += 1;
        self.node(self.package, data)
    }

    /// The checks of table `t` and its indexes, those of several columns among its unique
    /// constraints as well, without those MySQL makes for each foreign key.
    fn table_rules(&mut self, entity: Id, t: &Table, fks: &[ForeignKey]) -> Result<()> {
        let mut indexes: Vec<ErIndex> = t
            .indexes
            .iter()
            .filter(|i| !fks.iter().any(|fk| fk.columns == i.columns))
            .map(|i| ErIndex { name: i.name.clone(), columns: i.columns.clone(), unique: false })
            .collect();
        indexes.extend(t.uniques.iter().filter(|u| u.len() > 1).map(|u| ErIndex {
            name: String::new(),
            columns: u.clone(),
            unique: true,
        }));
        if indexes.is_empty() && t.checks.is_empty() {
            return Ok(());
        }
        let checks = t.checks.clone();
        self.tx.update(entity, |data| {
            if let ElementKind::ErEntity(p) = &mut data.kind {
                (p.checks, p.indexes) = (checks, indexes);
            }
        })
    }

    /// What column `c` of `t` says about itself, as an attribute.
    fn column(&self, t: &Table, c: &Column) -> ErAttributeProps {
        let datatype = self.types.get(&c.datatype).cloned().unwrap_or_else(|| c.datatype.clone());
        let (datatype, length) = split_type(&datatype);
        ErAttributeProps {
            derived: c.generated,
            virtual_column: c.generated && c.virtual_column,
            auto_increment: c.auto_increment,
            physical_name: c.name.clone(),
            datatype,
            length_precision: length.unwrap_or_default(),
            not_null: c.not_null,
            default_value: c.default.clone().unwrap_or_default(),
            unique: t.uniques.iter().any(|u| *u == [c.name.as_str()]),
            check: c.check.clone().unwrap_or_default(),
            ..Default::default()
        }
    }

    /// The entity of a referenced table; S10: a placeholder if it is not there.
    fn target(&mut self, key: &TableKey) -> Result<(Id, Id)> {
        if let Some(&ids) = self.entities.get(key) {
            return Ok(ids);
        }
        let ids = self.entity(key, false, Some(PLACEHOLDER_COMMENT.to_owned()))?;
        self.hints.push(Hint::NotInDump { element: ids.0, table: key.to_string() });
        Ok(ids)
    }

    /// The line from relationship `r` to entity `e`.
    fn join(
        &mut self,
        r: (Id, Id),
        e: (Id, Id),
        cardinality: Cardinality,
        role: Option<String>,
        total: bool,
    ) -> Result<()> {
        let props = ErParticipationProps {
            cardinality: Some(cardinality),
            role: role.unwrap_or_default(),
            total,
            ..ErParticipationProps::new(r.0, e.0)
        };
        let line =
            self.tx.create(r.0, ElementData::new("", ElementKind::ErParticipation(props)))?;
        self.tx.add_edge(self.diagram, Some(line), r.1, e.1, Vec::new())?;
        Ok(())
    }

    /// The columns of `fk` as foreign key columns of `owner` belonging to `relationship`,
    /// without nodes: Chen draws the relationship instead, IE and IDEF1X list them. Those in the
    /// primary key are keys.
    fn foreign_columns(
        &mut self,
        owner: Id,
        t: &Table,
        fk: &ForeignKey,
        relationship: Id,
    ) -> Result<()> {
        for name in &fk.columns {
            let Some(c) = t.column(name) else { continue };
            let key = t.primary_key.contains(name).then_some(ErKey::Primary);
            let take = match self.attributes {
                Attributes::All => true,
                Attributes::Keys => key.is_some(),
                Attributes::None => false,
            };
            // A column of two foreign keys belongs to the first.
            let taken = !self.tx.model().children_named(owner, name).is_empty();
            if !take || taken {
                continue;
            }
            let props =
                ErAttributeProps { key, foreign_key: Some(relationship), ..self.column(t, c) };
            let mut data = ElementData::new(c.name.clone(), ElementKind::ErAttribute(props));
            data.documentation = c.comment.clone().unwrap_or_default();
            self.tx.create(owner, data)?;
        }
        Ok(())
    }

    /// S6: the columns `names` in table order as attributes of `owner`.
    fn attributes(
        &mut self,
        owner: (Id, Id),
        t: &Table,
        names: &[&str],
        keys: &[&str],
        partial: &[&str],
    ) -> Result<()> {
        for c in &t.columns {
            if !names.contains(&c.name.as_str()) {
                continue;
            }
            let key = if keys.contains(&c.name.as_str()) {
                Some(ErKey::Primary)
            } else if partial.contains(&c.name.as_str()) {
                Some(ErKey::Partial)
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
            let props = ErAttributeProps { key, ..self.column(t, c) };
            let mut data = ElementData::new(c.name.clone(), ElementKind::ErAttribute(props));
            data.documentation = c.comment.clone().unwrap_or_default();
            let (attribute, node) = self.node(owner.0, data)?;
            // Drawn from the owner, as `er::connect` draws it.
            self.tx.add_edge(self.diagram, Some(attribute), owner.1, node, Vec::new())?;
            self.counts[2] += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use umlcraft_model::{Document, Model, Presentation};
    use umlcraft_notation::scene::ApproxMeasure;

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

    /// Maps `sql` into a new model: the document and what the import gave.
    fn mapped(sql: &str, dialect: Dialect, options: &Options) -> (Document, Import) {
        let schema = parse(sql.as_bytes(), dialect);
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let mut result = None;
        doc.edit("import", |tx| {
            result = Some(map(tx, root, &schema, options)?);
            Ok(())
        })
        .unwrap();
        (doc, result.unwrap())
    }

    /// The entities and relationships of the import, as `type name`.
    fn kinds(sql: &str) -> Vec<String> {
        let (doc, import) = mapped(sql, Dialect::MySql, &Options::default());
        let model = doc.model();
        model
            .children(import.package)
            .iter()
            .filter_map(|&c| model.get(c))
            .filter(|e| {
                matches!(e.kind(), ElementKind::ErEntity(_) | ElementKind::ErRelationship(_))
            })
            .map(|e| format!("{} {}", e.kind().type_name(), e.name()))
            .collect()
    }

    #[test]
    fn duplicate_keys_count_once_and_missing_targets_get_placeholders() {
        let sql = "CREATE TABLE a (id int PRIMARY KEY, b_id int, FOREIGN KEY (b_id) REFERENCES b (id), \
                   FOREIGN KEY (b_id) REFERENCES b (id));";
        assert_eq!(kinds(sql), ["erEntity A", "erEntity B", "erRelationship hat"]);
        let (_, import) = mapped(sql, Dialect::MySql, &Options::default());
        assert!(
            matches!(&import.report.hints[..], [Hint::NotInDump { table, .. }] if table == "b")
        );
    }

    #[test]
    fn junction_with_two_keys_to_one_table_gets_roles() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY);\nCREATE TABLE f (a int, b int, since date, \
                   PRIMARY KEY (a, b), FOREIGN KEY (a) REFERENCES p (id), FOREIGN KEY (b) \
                   REFERENCES p (id));";
        let (doc, import) = mapped(sql, Dialect::MySql, &Options::default());
        let model = doc.model();
        let f = model.children_named(import.package, "f")[0];
        assert!(matches!(
            model.get(f).unwrap().kind(),
            ElementKind::ErRelationship(ErRelationshipProps { identifying: false, .. })
        ));
        let roles: Vec<&str> =
            umlcraft_notation::er::participations(model, f).map(|p| p.role.as_str()).collect();
        assert_eq!(roles, ["a", "b"]);
        assert!(model.children(f).iter().any(|&c| model.get(c).unwrap().name() == "since"));
    }

    #[test]
    fn junction_with_its_own_key_column_is_a_weak_entity() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY);\nCREATE TABLE q (id int PRIMARY KEY);\n\
                   CREATE TABLE pq (p_id int, q_id int, nr int, PRIMARY KEY (p_id, q_id, nr), \
                   FOREIGN KEY (p_id) REFERENCES p (id), FOREIGN KEY (q_id) REFERENCES q (id));";
        assert_eq!(
            kinds(sql),
            ["erEntity P", "erEntity Q", "erEntity Pq", "erRelationship hat", "erRelationship hat"]
        );
        let (doc, import) = mapped(sql, Dialect::MySql, &Options::default());
        let model = doc.model();
        let pq = model.children_named(import.package, "Pq")[0];
        assert!(matches!(model.get(pq).unwrap().kind(), ElementKind::ErEntity(p) if p.weak));
        let nr = model.children_named(pq, "nr")[0];
        assert!(matches!(
            model.get(nr).unwrap().kind(),
            ElementKind::ErAttribute(a) if a.key == Some(ErKey::Partial)
        ));
        model.check().unwrap();
    }

    #[test]
    fn foreign_keys_become_columns_and_ie_draws_them_in_the_tables() {
        let sql = "CREATE TABLE p (id int PRIMARY KEY);\nCREATE TABLE q (id int PRIMARY KEY);\n\
                   CREATE TABLE pq (p_id int, q_id int, nr int, PRIMARY KEY (p_id, q_id, nr), \
                   FOREIGN KEY (p_id) REFERENCES p (id), FOREIGN KEY (q_id) REFERENCES q (id));";
        let schema = parse(sql.as_bytes(), Dialect::MySql);
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let options = Options { notation: ErNotation::Ie, ..Options::default() };
        let mut result = None;
        doc.edit("import", |tx| {
            result = Some(import(tx, root, &schema, &options, &ApproxMeasure)?);
            Ok(())
        })
        .unwrap();
        let (model, import) = (doc.model(), result.unwrap());
        model.check().unwrap();
        let pq = model.children_named(import.package, "Pq")[0];
        for column in ["p_id", "q_id"] {
            let c = model.children_named(pq, column)[0];
            let ElementKind::ErAttribute(a) = model.get(c).unwrap().kind() else { panic!() };
            assert_eq!(a.key, Some(ErKey::Primary), "{column}");
            let by = a.foreign_key.and_then(|r| model.get(r)).map(|r| r.kind().type_name());
            assert_eq!(by, Some("erRelationship"), "{column}");
        }
        // No attribute nodes; the two relationships are lines between the tables.
        let view = model.view(import.diagram).unwrap();
        let kind = |p: &Presentation| p.element.and_then(|e| model.get(e)).map(|e| e.kind());
        let nodes = view.presentations().iter().filter(|p| p.bounds().is_some());
        assert_eq!(nodes.clone().count(), 3);
        assert!(nodes.clone().all(|p| matches!(kind(p), Some(ElementKind::ErEntity(_)))));
        let lines = view.presentations().iter().filter(|p| p.endpoints().is_some());
        assert!(lines.clone().all(|p| matches!(kind(p), Some(ElementKind::ErRelationship(_)))));
        assert_eq!(lines.count(), 2);
        assert!(er_layout::overlaps(model, import.diagram, &ApproxMeasure).is_empty());
    }

    #[test]
    fn chained_weak_entities_make_a_valid_model() {
        // order → item → note: the item is weak and owns the weak note.
        let sql = "CREATE TABLE o (id int PRIMARY KEY);\n\
                   CREATE TABLE oi (o_id int, nr int, PRIMARY KEY (o_id, nr), \
                   FOREIGN KEY (o_id) REFERENCES o (id));\n\
                   CREATE TABLE oin (o_id int, nr int, k int, PRIMARY KEY (o_id, nr, k), \
                   FOREIGN KEY (o_id, nr) REFERENCES oi (o_id, nr));";
        let (doc, import) = mapped(sql, Dialect::MySql, &Options::default());
        let model = doc.model();
        model.check().unwrap();
        let weak = model
            .children(import.package)
            .iter()
            .filter(
                |&&c| matches!(model.get(c).unwrap().kind(), ElementKind::ErEntity(p) if p.weak),
            )
            .count();
        assert_eq!(weak, 2);
        let text = umlcraft_format::to_string(model);
        assert_eq!(umlcraft_format::to_string(&umlcraft_format::from_str(&text).unwrap()), text);
    }

    #[test]
    fn options_choose_attributes_and_schemas() {
        let sql = "CREATE TABLE public.a (id int, x int);\nALTER TABLE public.a ADD PRIMARY KEY (id);\n\
                   CREATE TABLE s.b (id int);";
        let count = |o: &Options| mapped(sql, Dialect::Postgres, o).1.report.attributes;
        assert_eq!(count(&Options::default()), 3);
        assert_eq!(count(&Options { attributes: Attributes::Keys, ..Options::default() }), 1);
        assert_eq!(count(&Options { attributes: Attributes::None, ..Options::default() }), 0);
        let only_public = Options { schemas: Some(vec!["public".into()]), ..Options::default() };
        let r = mapped(sql, Dialect::Postgres, &only_public).1.report;
        assert_eq!((r.tables, r.entities), (1, 1));
    }

    #[test]
    fn the_package_is_named_after_the_database_or_the_file() {
        let o = Options { file_name: Some("dump.sql".into()), ..Options::default() };
        let sql = "-- Host: x    Database: flug\nCREATE TABLE a (id int);";
        let (doc, import) = mapped(sql, Dialect::MySql, &o);
        let package = doc.model().get(import.package).unwrap();
        assert_eq!(package.name(), "flug");
        assert_eq!(package.data().documentation, "SQL: dump.sql (MySQL)");
        assert_eq!(doc.model().get(import.diagram).unwrap().name(), "flug");
        let (doc, import) = mapped("CREATE TABLE a (id int);", Dialect::Postgres, &o);
        assert_eq!(doc.model().get(import.package).unwrap().name(), "dump");
    }
}
