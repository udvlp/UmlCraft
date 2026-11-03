//! Reading `.ucerm`. Unknown fields and fields that do not belong to a node's kind are errors
//! (with line and column where serde knows them); a newer format version asks for a newer
//! UmlCraft; older versions are upgraded step by step. The diagram is then checked: broken
//! error rules reject the file, warnings are returned with the diagram.

use serde_json::Value;

use crate::check::{Issue, Severity, check};
use crate::model::{
    Caption, Cardinality, Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Max, Meta, Min,
    Node, NodeKind, Point, Props, Rect, Style, Text, TextAlign,
};
use crate::record::{
    FileRecord, LabelRecord, LinkRecord, MaxRecord, NodeRecord, PointRecord, PropsRecord,
    RectRecord, StyleRecord, TextRecord,
};
use crate::{FORMAT_ID, FORMAT_VERSION};

/// Upgrades a document from version `index + 1` to `index + 2`.
type Migration = fn(&mut Value) -> Result<(), ReadError>;
/// One migration per version step; none yet.
const MIGRATIONS: &[Migration] = &[];
const _: () = assert!(MIGRATIONS.len() == FORMAT_VERSION as usize - 1);

#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    /// Malformed JSON or a field that does not exist; the message names line and column.
    #[error("malformed ER diagram file: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not an UmlCraft ER diagram file")]
    NotAnErmFile,
    #[error("file format version {found} is newer than the supported version {supported}")]
    TooNew { found: u32, supported: u32 },
    #[error("migration from version {from} failed: {reason}")]
    Migration { from: u32, reason: String },
    /// A field with a value or a combination the format does not allow.
    #[error("{element}: {reason}")]
    Field { element: String, reason: String },
    /// The diagram breaks rules of [`check`]; only the errors are listed.
    #[error("invalid ER diagram: {}", list(.0))]
    Invalid(Vec<Issue>),
}

fn list(issues: &[Issue]) -> String {
    issues.iter().map(Issue::to_string).collect::<Vec<_>>().join("; ")
}

/// Reads a file; returns the diagram and the warnings [`check`] found.
pub fn read(json: &str) -> Result<(Diagram, Vec<Issue>), ReadError> {
    let json = json.strip_prefix('\u{feff}').unwrap_or(json);
    let value: Value = serde_json::from_str(json)?;
    let version = version(&value)?;
    let file: FileRecord = if version == FORMAT_VERSION {
        // From the text, so that errors name line and column.
        serde_json::from_str(json)?
    } else {
        serde_json::from_value(migrate(value, version)?)?
    };
    finish(file)
}

/// Reads a diagram embedded as JSON value (see [`crate::to_value`]).
pub fn read_value(value: Value) -> Result<(Diagram, Vec<Issue>), ReadError> {
    let version = version(&value)?;
    finish(serde_json::from_value(migrate(value, version)?)?)
}

fn version(value: &Value) -> Result<u32, ReadError> {
    if value.get("format").and_then(Value::as_str) != Some(FORMAT_ID) {
        return Err(ReadError::NotAnErmFile);
    }
    let version = value
        .get("formatVersion")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
        .ok_or(ReadError::NotAnErmFile)?;
    if version > FORMAT_VERSION {
        return Err(ReadError::TooNew { found: version, supported: FORMAT_VERSION });
    }
    Ok(version)
}

fn migrate(mut value: Value, version: u32) -> Result<Value, ReadError> {
    for (index, step) in MIGRATIONS.iter().enumerate().skip(version as usize - 1) {
        step(&mut value)?;
        value["formatVersion"] = Value::from(index as u32 + 2);
    }
    Ok(value)
}

fn finish(file: FileRecord) -> Result<(Diagram, Vec<Issue>), ReadError> {
    let m = file.meta;
    let diagram = Diagram {
        meta: Meta {
            name: m.name,
            author: m.author,
            created: m.created,
            modified: m.modified,
            origin: m.origin,
            notation: m.notation,
        },
        nodes: file.nodes.into_iter().map(node).collect::<Result<_, _>>()?,
        links: file.links.into_iter().map(link).collect::<Result<_, _>>()?,
        texts: file.texts.into_iter().map(text).collect::<Result<_, _>>()?,
    };
    let (errors, warnings): (Vec<Issue>, Vec<Issue>) =
        check(&diagram).into_iter().partition(|i| i.severity() == Severity::Error);
    if errors.is_empty() { Ok((diagram, warnings)) } else { Err(ReadError::Invalid(errors)) }
}

fn field(element: &str, reason: impl Into<String>) -> ReadError {
    ReadError::Field { element: element.to_owned(), reason: reason.into() }
}

fn rect(r: RectRecord) -> Rect {
    Rect::new(r.x, r.y, r.width, r.height)
}

fn point(p: PointRecord) -> Point {
    Point::new(p.x, p.y)
}

fn node(r: NodeRecord) -> Result<Node, ReadError> {
    // Fields set in the record that the kind does not have.
    let set = [
        ("weak", r.weak),
        ("key", r.key.is_some()),
        ("derived", r.derived),
        ("multivalued", r.multivalued),
        ("identifying", r.identifying),
        ("total", r.total),
        ("disjoint", r.disjoint.is_some()),
    ];
    let own: &[&str] = match r.kind.as_str() {
        "entity" => &["weak"],
        "associativeEntity" => &[],
        "attribute" => &["key", "derived", "multivalued"],
        "relationship" => &["identifying"],
        "isA" => &["total", "disjoint"],
        other => return Err(field(&r.id, format!("unknown node type `{other}`"))),
    };
    if let Some((name, _)) = set.iter().find(|(name, on)| *on && !own.contains(name)) {
        return Err(field(&r.id, format!("field `{name}` does not belong to type `{}`", r.kind)));
    }
    let kind = match r.kind.as_str() {
        "entity" => NodeKind::Entity { weak: r.weak },
        "associativeEntity" => NodeKind::AssociativeEntity,
        "attribute" => NodeKind::Attribute {
            key: match r.key.as_deref() {
                None => None,
                Some("primary") => Some(Key::Primary),
                Some("partial") => Some(Key::Partial),
                Some(other) => return Err(field(&r.id, format!("unknown key `{other}`"))),
            },
            derived: r.derived,
            multivalued: r.multivalued,
        },
        "relationship" => NodeKind::Relationship { identifying: r.identifying },
        _ => NodeKind::IsA { total: r.total, disjoint: r.disjoint },
    };
    Ok(Node {
        style: style(&r.id, r.style)?,
        props: props(r.props),
        id: Id(r.id),
        kind,
        name: r.name,
        bounds: rect(r.bounds),
    })
}

fn style(id: &str, s: StyleRecord) -> Result<Style, ReadError> {
    Ok(Style {
        fill: s.fill,
        stroke: s.stroke,
        font_color: s.font_color,
        font_size: s.font_size,
        bold: s.bold,
        italic: s.italic,
        rounded: s.rounded,
        align: match s.align.as_deref() {
            None => None,
            Some("left") => Some(TextAlign::Left),
            Some("center") => Some(TextAlign::Center),
            Some("right") => Some(TextAlign::Right),
            Some(other) => return Err(field(id, format!("unknown alignment `{other}`"))),
        },
    })
}

fn props(p: PropsRecord) -> Props {
    Props {
        physical_name: p.physical_name,
        datatype: p.datatype,
        length_precision: p.length_precision,
        not_null: p.not_null,
        default_value: p.default_value,
        comment: p.comment,
    }
}

fn position(r: &LabelRecord) -> LabelPos {
    LabelPos { x: r.x, y: r.y, dx: r.dx, dy: r.dy }
}

fn label(id: &str, r: LabelRecord) -> Result<Label, ReadError> {
    let text = match (r.text.clone(), r.min.clone(), r.max.clone()) {
        (Some(t), None, None) => LabelText::Text(t),
        (None, Some(min), Some(max)) => LabelText::Cardinality(Cardinality {
            min: match min {
                MaxRecord::Count(n) => Min::Count(n),
                MaxRecord::Word(w) if w == "*" => Min::Any,
                MaxRecord::Word(w) => {
                    return Err(field(id, format!("`min` is a number or \"*\", not `{w}`")));
                }
            },
            max: match max {
                MaxRecord::Count(n) => Max::Count(n),
                MaxRecord::Word(w) if w == "n" => Max::Many,
                MaxRecord::Word(w) if w == "m" => Max::ManyM,
                MaxRecord::Word(w) => {
                    return Err(field(id, format!("`max` is a number, \"n\" or \"m\", not `{w}`")));
                }
            },
        }),
        _ => return Err(field(id, "a label has either `text` or `min` and `max`")),
    };
    Ok(Label { pos: position(&r), text })
}

fn caption(id: &str, r: LabelRecord) -> Result<Caption, ReadError> {
    match (&r.text, &r.min, &r.max) {
        (Some(t), None, None) => Ok(Caption { text: t.clone(), pos: position(&r) }),
        _ => Err(field(id, "a role has a `text` and no cardinality")),
    }
}

fn link(r: LinkRecord) -> Result<Link, ReadError> {
    let line = match r.line.as_deref() {
        None => Line::Solid,
        Some("dotted") => Line::Dotted,
        Some("double") => Line::Double,
        Some(other) => return Err(field(&r.id, format!("unknown line `{other}`"))),
    };
    Ok(Link {
        label: r.label.map(|l| label(&r.id, l)).transpose()?,
        role: r.role.map(|l| caption(&r.id, l)).transpose()?,
        id: Id(r.id),
        source: Id(r.source),
        target: Id(r.target),
        source_port: r.source_port.map(point),
        target_port: r.target_port.map(point),
        line,
        waypoints: r.waypoints.into_iter().map(point).collect(),
        super_type: r.super_type,
    })
}

fn text(r: TextRecord) -> Result<Text, ReadError> {
    Ok(Text { style: style(&r.id, r.style)?, id: Id(r.id), bounds: rect(r.bounds), text: r.text })
}
