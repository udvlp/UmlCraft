//! The imported model as a table, one row per entity and relationship with its attributes
//! and ends (the form of `tests/expected/*.md`, which the Python prototype the rules were
//! worked out with writes as well).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use umlcraft_erm::{Diagram, Key, Line, NodeKind};

pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn expected(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/expected").join(name);
    std::fs::read_to_string(path).unwrap()
}

pub fn describe(d: &Diagram) -> String {
    let mut lines = vec![
        "| Art | Name | Attribute | Enden (look-across) |".to_owned(),
        "|-----|------|-----------|---------------------|".to_owned(),
    ];
    for n in d.nodes.iter().filter(|n| !n.kind.is_attribute()) {
        let own: Vec<String> = d
            .links
            .iter()
            .filter(|l| l.target == n.id)
            .filter_map(|l| d.node(&l.source))
            .filter(|a| a.kind.is_attribute())
            .map(|a| {
                let NodeKind::Attribute { key, derived, .. } = a.kind else { unreachable!() };
                let mut s = match key {
                    Some(Key::Primary) => format!("<u>{}</u>", a.name),
                    Some(Key::Partial) => format!("<u>{}</u>˙", a.name),
                    None => a.name.clone(),
                };
                if derived {
                    s.push_str(" (abgeleitet)");
                }
                s
            })
            .collect();
        let at = if own.is_empty() { "–".to_owned() } else { own.join(", ") };
        match n.kind {
            NodeKind::Entity { weak } => {
                let k = if weak { "schwache Entity" } else { "Entity" };
                lines.push(format!("| {k} | {} | {at} | |", n.name));
            }
            NodeKind::Relationship { identifying } => {
                let ends: Vec<String> = d
                    .links
                    .iter()
                    .filter(|l| l.source == n.id)
                    .filter_map(|l| Some((l, d.node(&l.target)?)))
                    .filter(|(_, t)| !t.kind.is_attribute())
                    .map(|(l, t)| {
                        let label = l.label.as_ref().map(|x| x.display()).unwrap_or_default();
                        let mut e = format!("{} {label}", t.name);
                        if let Some(r) = &l.role {
                            e.push_str(&format!(" (Rolle {})", r.text));
                        }
                        if l.line == Line::Double {
                            e.push_str(" ═");
                        }
                        e
                    })
                    .collect();
                let k = if identifying { "ident. Beziehung" } else { "Beziehung" };
                lines.push(format!("| {k} | {} | {at} | {} |", n.name, ends.join("; ")));
            }
            _ => {}
        }
    }
    lines.join("\n")
}
