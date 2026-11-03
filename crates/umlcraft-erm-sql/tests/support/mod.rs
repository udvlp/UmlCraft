//! The imported model as a table, one row per entity and relationship with its attributes
//! and ends (the form of `tests/expected/*.md`, which the Python prototype the rules were
//! worked out with writes as well), and imports into a new model.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use umlcraft_erm_sql::{Import, Options, Schema};
use umlcraft_model::{Document, ElementKind, ErKey, Id, Model};
use umlcraft_notation::scene::ApproxMeasure;

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn expected(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected").join(name);
    std::fs::read_to_string(path).unwrap()
}

/// Imports `schema` into a new model; with `arrange`, laid out as the app does it.
pub fn imported(schema: &Schema, options: &Options, arrange: bool) -> (Document, Import) {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let mut result = None;
    doc.edit("import", |tx| {
        result = Some(if arrange {
            umlcraft_erm_sql::import(tx, root, schema, options, &ApproxMeasure)?
        } else {
            umlcraft_erm_sql::map::map(tx, root, schema, options)?
        });
        Ok(())
    })
    .unwrap();
    (doc, result.unwrap())
}

/// The entities and relationships of `package` in their order.
pub fn describe(model: &Model, package: Id) -> String {
    let mut lines = vec![
        "| Art | Name | Attribute | Enden (look-across) |".to_owned(),
        "|-----|------|-----------|---------------------|".to_owned(),
    ];
    for e in model.children(package).iter().filter_map(|&c| model.get(c)) {
        let own: Vec<String> = model
            .children(e.id())
            .iter()
            .filter_map(|&c| model.get(c))
            .filter_map(|a| match a.kind() {
                // Foreign key columns are their relationships here, as Chen draws them.
                ElementKind::ErAttribute(p) if p.foreign_key.is_none() => Some((a.name(), p)),
                _ => None,
            })
            .map(|(name, p)| {
                let mut s = match p.key {
                    Some(ErKey::Primary) => format!("<u>{name}</u>"),
                    Some(ErKey::Partial) => format!("<u>{name}</u>˙"),
                    None => name.to_owned(),
                };
                if p.derived {
                    s.push_str(" (abgeleitet)");
                }
                s
            })
            .collect();
        let at = if own.is_empty() { "–".to_owned() } else { own.join(", ") };
        match e.kind() {
            ElementKind::ErEntity(p) => {
                let k = if p.weak { "schwache Entity" } else { "Entity" };
                lines.push(format!("| {k} | {} | {at} | |", e.name()));
            }
            ElementKind::ErRelationship(p) => {
                let ends: Vec<String> = umlcraft_notation::er::participations(model, e.id())
                    .map(|l| {
                        let entity = model.get(l.entity).map_or("", |t| t.name());
                        let label = l.cardinality.map(|c| c.to_string()).unwrap_or_default();
                        let mut end = format!("{entity} {label}");
                        if !l.role.is_empty() {
                            end.push_str(&format!(" (Rolle {})", l.role));
                        }
                        if l.total {
                            end.push_str(" ═");
                        }
                        end
                    })
                    .collect();
                let k = if p.identifying { "ident. Beziehung" } else { "Beziehung" };
                lines.push(format!("| {k} | {} | {at} | {} |", e.name(), ends.join("; ")));
            }
            _ => {}
        }
    }
    lines.join("\n")
}

/// The table without the order of the tables and of a relationship's ends, which follow
/// the source (so does which end is `m`).
pub fn unordered(table: &str) -> Vec<String> {
    let table = table.replace("[0,m]", "[0,n]");
    let mut lines: Vec<String> = table
        .lines()
        .map(|l| {
            let mut cells: Vec<String> = l.split(" | ").map(str::to_owned).collect();
            if let Some(ends) = cells.get_mut(3) {
                let mut e: Vec<&str> = ends.trim_end_matches(" |").split("; ").collect();
                e.sort_unstable();
                *ends = e.join("; ");
            }
            cells.join(" | ")
        })
        .collect();
    lines.sort();
    lines
}
