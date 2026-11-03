//! The diagram: a graph of typed nodes (entities, attributes, relationships, IS-A), the links
//! between them and free texts, each with its place on the canvas as draw.io keeps it.
//!
//! Links are directed as in the files: an attribute points to its owner, a relationship to
//! the entities taking part in it, an IS-A to its super- and subtypes. The cardinality on a
//! line between a relationship and an entity is read look-across: `[1,1]` at `Kunde` on
//! `Kunde – bucht – Ticket` says that every ticket has exactly one customer.

use std::fmt;

/// Identifier of a node, link or text. New ids are UUIDv7 (they sort by creation time, so
/// files written sorted by id grow at the end); ids read from a file are kept as they are.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Id(pub String);

impl Id {
    /// A fresh, time-ordered id.
    pub fn generate() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A point on the canvas, in px at zoom 1.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A box: top left corner and size.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

/// The whole diagram.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Diagram {
    pub meta: Meta,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub texts: Vec<Text>,
}

/// Name and origin of a diagram.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Meta {
    pub name: String,
    pub author: Option<String>,
    /// Dates as written by the app (ISO 8601).
    pub created: Option<String>,
    pub modified: Option<String>,
    /// Where the diagram came from, such as `SQL: shop.sql`.
    pub origin: Option<String>,
    /// The notation of the cardinalities; `None` means [`Meta::DEFAULT_NOTATION`].
    pub notation: Option<String>,
}

impl Meta {
    /// Chen notation with `[min,max]`, read look-across.
    pub const DEFAULT_NOTATION: &'static str = "chen-minmax-across";
}

/// A shape on the canvas.
#[derive(Clone, PartialEq, Debug)]
pub struct Node {
    pub id: Id,
    pub kind: NodeKind,
    pub name: String,
    pub bounds: Rect,
    pub style: Style,
    pub props: Props,
}

/// What a node is, with what only that kind of node has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NodeKind {
    /// A rectangle; a weak entity has a second frame inside.
    Entity { weak: bool },
    /// A rectangle with a rhombus inside: a relationship that is also an entity.
    AssociativeEntity,
    /// An ellipse: underlined as a key, dotted as a partial key, dashed if derived, double if
    /// multivalued.
    Attribute { key: Option<Key>, derived: bool, multivalued: bool },
    /// A rhombus; an identifying relationship has a second rhombus inside.
    Relationship { identifying: bool },
    /// A triangle pointing to the supertype. `total` draws a double line to the supertype,
    /// `disjoint` writes "d" (true) or "o" (false) beside it.
    IsA { total: bool, disjoint: Option<bool> },
}

impl NodeKind {
    pub const fn entity() -> Self {
        Self::Entity { weak: false }
    }

    pub const fn attribute() -> Self {
        Self::Attribute { key: None, derived: false, multivalued: false }
    }

    pub const fn relationship() -> Self {
        Self::Relationship { identifying: false }
    }

    pub const fn is_a() -> Self {
        Self::IsA { total: false, disjoint: None }
    }

    /// The name of the kind in files (`type`).
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Entity { .. } => "entity",
            Self::AssociativeEntity => "associativeEntity",
            Self::Attribute { .. } => "attribute",
            Self::Relationship { .. } => "relationship",
            Self::IsA { .. } => "isA",
        }
    }

    pub const fn is_attribute(&self) -> bool {
        matches!(self, Self::Attribute { .. })
    }

    /// Entities and associative entities: what takes part in relationships.
    pub const fn is_entity_like(&self) -> bool {
        matches!(self, Self::Entity { .. } | Self::AssociativeEntity)
    }
}

/// Which kind of key an attribute is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// Identifies an entity (underlined).
    Primary,
    /// Identifies a weak entity together with its owner (dotted underline).
    Partial,
}

/// Deviations from the standard look. All `None`: black on white, 12 px, plain.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Style {
    /// Colors as `#rrggbb`.
    pub fill: Option<String>,
    pub stroke: Option<String>,
    pub font_color: Option<String>,
    pub font_size: Option<f64>,
    pub bold: bool,
    pub italic: bool,
    /// Rounded corners (entities).
    pub rounded: bool,
    /// Alignment of free texts.
    pub align: Option<TextAlign>,
}

impl Style {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Database properties, named as in Astah's ER model. Kept, not drawn.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Props {
    pub physical_name: Option<String>,
    pub datatype: Option<String>,
    pub length_precision: Option<String>,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub comment: Option<String>,
}

impl Props {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A line between two nodes.
#[derive(Clone, PartialEq, Debug)]
pub struct Link {
    pub id: Id,
    pub source: Id,
    pub target: Id,
    /// Fixed connection points relative to the node's box (0..=1), like draw.io's
    /// `exitX/exitY` and `entryX/entryY`; `None` ends on the outline towards the other end.
    pub source_port: Option<Point>,
    pub target_port: Option<Point>,
    /// The cardinality or another text on the line.
    pub label: Option<Label>,
    /// A role (self-references), drawn as a second label.
    pub role: Option<Caption>,
    pub line: Line,
    /// Points the line passes through.
    pub waypoints: Vec<Point>,
    /// On IS-A links: this one goes to the supertype.
    pub super_type: bool,
}

impl Link {
    pub fn new(source: Id, target: Id) -> Self {
        Self {
            id: Id::generate(),
            source,
            target,
            source_port: None,
            target_port: None,
            label: None,
            role: None,
            line: Line::Solid,
            waypoints: Vec::new(),
            super_type: false,
        }
    }

    /// The other end of the link, if `id` is one of its ends.
    pub fn other(&self, id: &Id) -> Option<&Id> {
        if self.source == *id {
            Some(&self.target)
        } else if self.target == *id {
            Some(&self.source)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Line {
    #[default]
    Solid,
    /// Dotted `1 2` (optional participation).
    Dotted,
    /// Two parallel lines (total participation).
    Double,
}

/// Where a label sits, as in draw.io: `x` from -1 (start) over 0 (middle) to 1 (end) along
/// the line, `y` px to the left of the direction of the line, then moved by `dx`, `dy`.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct LabelPos {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}

/// The label of a link.
#[derive(Clone, PartialEq, Debug)]
pub struct Label {
    pub text: LabelText,
    pub pos: LabelPos,
}

#[derive(Clone, PartialEq, Debug)]
pub enum LabelText {
    Text(String),
    Cardinality(Cardinality),
}

/// `[min,max]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cardinality {
    pub min: u32,
    pub max: Max,
}

impl Cardinality {
    pub const fn new(min: u32, max: Max) -> Self {
        Self { min, max }
    }
}

impl fmt::Display for Cardinality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.max {
            Max::Count(n) => write!(f, "[{},{n}]", self.min),
            Max::Many => write!(f, "[{},n]", self.min),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Max {
    Count(u32),
    /// Any number (`n`).
    Many,
}

impl Label {
    /// The text that is drawn.
    pub fn display(&self) -> String {
        match &self.text {
            LabelText::Text(t) => t.clone(),
            LabelText::Cardinality(c) => c.to_string(),
        }
    }
}

/// A plain text on a line (roles).
#[derive(Clone, PartialEq, Debug)]
pub struct Caption {
    pub text: String,
    pub pos: LabelPos,
}

/// A free text on the canvas.
#[derive(Clone, PartialEq, Debug)]
pub struct Text {
    pub id: Id,
    pub bounds: Rect,
    /// Plain text, `\n` breaks lines.
    pub text: String,
    pub style: Style,
}

impl Diagram {
    pub fn new(name: impl Into<String>) -> Self {
        Self { meta: Meta { name: name.into(), ..Meta::default() }, ..Self::default() }
    }

    pub fn node(&self, id: &Id) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == *id)
    }

    pub fn node_mut(&mut self, id: &Id) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == *id)
    }

    pub fn link(&self, id: &Id) -> Option<&Link> {
        self.links.iter().find(|l| l.id == *id)
    }

    /// The links that end at the node.
    pub fn links_of<'a>(&'a self, id: &'a Id) -> impl Iterator<Item = &'a Link> + 'a {
        self.links.iter().filter(move |l| l.source == *id || l.target == *id)
    }

    /// The node an attribute belongs to: the non-attribute at the other end of one of its
    /// links, or for a part of a composite attribute the attribute its link points to.
    pub fn owner_of<'a>(&'a self, attribute: &'a Id) -> Option<&'a Node> {
        self.owners_of(attribute).next()
    }

    pub(crate) fn owners_of<'a>(&'a self, attribute: &'a Id) -> impl Iterator<Item = &'a Node> {
        self.links_of(attribute).filter_map(move |l| {
            let other = self.node(l.other(attribute)?)?;
            let owns = !other.kind.is_attribute() || l.source == *attribute;
            (owns && other.id != *attribute).then_some(other)
        })
    }

    /// Nodes, links and texts sorted by id, as files list them.
    pub fn sort(&mut self) {
        self.nodes.sort_by(|a, b| a.id.cmp(&b.id));
        self.links.sort_by(|a, b| a.id.cmp(&b.id));
        self.texts.sort_by(|a, b| a.id.cmp(&b.id));
    }
}
