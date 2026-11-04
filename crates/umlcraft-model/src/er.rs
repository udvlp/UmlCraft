//! The elements of entity-relationship diagrams in Chen notation: entities, their attributes,
//! relationships with the entities taking part in them, and IS-A generalizations.
//!
//! Entities, relationships and IS-As belong to a package; attributes to what they describe (an
//! entity, a relationship or a composite attribute), so that the line to the owner follows from
//! the ownership tree. A participation joins a relationship (or an associative entity) to an
//! entity and belongs to the relationship; a subtyping joins an IS-A to one of its entities and
//! belongs to the IS-A.
//!
//! The same elements are drawn in IE (crow's feet) and IDEF1X as well, chosen per diagram
//! ([`ErNotation`]): there an entity lists its attributes, a relationship between two entities
//! is a line, and the foreign key columns a relationship gives its child are attributes of
//! their own ([`ErAttributeProps::foreign_key`]), which Chen leaves out.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::id::Id;

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

/// A rectangle; a weak entity has a second frame inside, an associative one a rhombus.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErEntityProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub weak: bool,
    /// A relationship that is also an entity: entities take part in it.
    #[serde(default, skip_serializing_if = "is_default")]
    pub associative: bool,
    /// The table's name in the database.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub physical_name: String,
    /// Conditions on the table's rows, as SQL (`end_zeit > start_zeit`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
    /// The table's indexes besides its primary key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub indexes: Vec<ErIndex>,
}

/// An index of a table: its columns by their names in the database, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErIndex {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub unique: bool,
}

/// A named type for columns: a domain (a datatype with a condition, a default or `NOT NULL`)
/// or an enumerated type. A column takes it by its name as its datatype. Not drawn; it lives in
/// the package of its entities.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErDomainProps {
    /// The datatype it is based on, and its length or precision; empty for an enumeration.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub datatype: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub length_precision: String,
    /// The values of an enumerated type, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub not_null: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
    /// A condition on its values, as SQL with `VALUE` for the value (`VALUE > 0`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub check: String,
}

/// What part of an identifier an attribute is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErKey {
    /// Underlined: identifies its entity.
    Primary,
    /// Underlined with dots: identifies a weak entity together with its owner's key.
    Partial,
}

/// An ellipse: dashed if derived, double if multivalued; composite when it owns attributes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErAttributeProps {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ErKey>,
    #[serde(default, skip_serializing_if = "is_default")]
    pub derived: bool,
    #[serde(default, skip_serializing_if = "is_default")]
    pub multivalued: bool,
    /// The column: its name in the database, data type, length or precision, whether it may
    /// be empty and its default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub physical_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub datatype: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub length_precision: String,
    #[serde(default, skip_serializing_if = "is_default")]
    pub not_null: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub default_value: String,
    /// The column's values are unique.
    #[serde(default, skip_serializing_if = "is_default")]
    pub unique: bool,
    /// A condition on the column's values, as SQL (`preis >= 0`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub check: String,
    /// The database numbers new rows in it (`AUTO_INCREMENT`, an identity column).
    #[serde(default, skip_serializing_if = "is_default")]
    pub auto_increment: bool,
    /// A derived column's expression (its default) is computed when read, not stored
    /// (`VIRTUAL`).
    #[serde(default, skip_serializing_if = "is_default")]
    pub virtual_column: bool,
    /// A foreign key column: the relationship (or the subtyping of a subtype) it belongs to,
    /// which copied it from the parent's primary key. Deleting that one keeps the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_key: Option<Id>,
}

/// How an ER diagram draws its elements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErNotation {
    /// Entities as rectangles, attributes as ellipses, relationships as rhombi.
    #[default]
    Chen,
    /// Information Engineering: entities listing their attributes, relationships as lines with
    /// crow's feet.
    Ie,
    /// IDEF1X: as IE, with dots and diamonds at the ends of the lines.
    Idef1x,
}

impl ErNotation {
    /// IE and IDEF1X: entities list their attributes, binary relationships are lines.
    pub fn lists_attributes(self) -> bool {
        self != Self::Chen
    }
}

/// What the entities of an IE or IDEF1X diagram show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErDisplay {
    #[default]
    Attributes,
    /// Only the primary keys.
    Keys,
    /// Only the name.
    Name,
}

/// A rhombus; an identifying relationship (of a weak entity) has a second one inside.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErRelationshipProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub identifying: bool,
    /// What its foreign keys do when a referenced row is deleted, or its key changes; `None`
    /// leaves it to the database (`NO ACTION`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_delete: Option<ErAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_update: Option<ErAction>,
}

impl ErRelationshipProps {
    pub fn new(identifying: bool) -> Self {
        Self { identifying, ..Self::default() }
    }
}

/// A foreign key's referential action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErAction {
    Cascade,
    SetNull,
    SetDefault,
    Restrict,
    NoAction,
}

impl ErAction {
    pub const ALL: [Self; 5] =
        [Self::Cascade, Self::SetNull, Self::SetDefault, Self::Restrict, Self::NoAction];

    /// As SQL writes it: `CASCADE`, `SET NULL`, …
    pub fn sql(self) -> &'static str {
        match self {
            Self::Cascade => "CASCADE",
            Self::SetNull => "SET NULL",
            Self::SetDefault => "SET DEFAULT",
            Self::Restrict => "RESTRICT",
            Self::NoAction => "NO ACTION",
        }
    }
}

impl FromStr for ErAction {
    type Err = String;

    /// The SQL words in any case, with spaces, `_` or nothing between them (`set null`,
    /// `SET_NULL`, `setNull`).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let plain: String =
            s.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
        Self::ALL
            .into_iter()
            .find(|a| a.sql().replace(' ', "").eq_ignore_ascii_case(&plain))
            .ok_or_else(|| s.to_owned())
    }
}

/// A triangle pointing to the supertype. A total IS-A has a double line to the supertype;
/// `disjoint` writes "d" (true) or "o" (false) beside it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErIsAProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub total: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disjoint: Option<bool>,
    /// The supertype's attribute that tells the subtypes apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<Id>,
}

/// How often an entity takes part in a relationship, read look-across: `[1,1]` at `Kunde` on
/// `Kunde – bucht – Ticket` says that every ticket has one customer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Cardinality {
    pub min: CardinalityMin,
    pub max: CardinalityMax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardinalityMin {
    Count(u32),
    /// Any number (`*`).
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CardinalityMax {
    Count(u32),
    /// Any number (`n`).
    Many,
    /// Any number, named `m`: the second "many" side of a relationship (`[0,n]` to `[0,m]`).
    ManyM,
}

impl Cardinality {
    pub const ZERO_ONE: Self = Self::new(0, CardinalityMax::Count(1));
    pub const ONE_ONE: Self = Self::new(1, CardinalityMax::Count(1));
    pub const ZERO_MANY: Self = Self::new(0, CardinalityMax::Many);
    pub const ONE_MANY: Self = Self::new(1, CardinalityMax::Many);

    /// `[min,max]` with a number as the minimum.
    pub const fn new(min: u32, max: CardinalityMax) -> Self {
        Self { min: CardinalityMin::Count(min), max }
    }

    /// `(min,max)`, as cardinalities read at their entity are written.
    pub fn parenthesized(&self) -> String {
        let text = self.to_string();
        format!("({})", &text[1..text.len() - 1])
    }
}

impl fmt::Display for Cardinality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        match self.min {
            CardinalityMin::Count(n) => write!(f, "{n}")?,
            CardinalityMin::Any => f.write_str("*")?,
        }
        match self.max {
            CardinalityMax::Count(n) => write!(f, ",{n}]"),
            CardinalityMax::Many => f.write_str(",n]"),
            CardinalityMax::ManyM => f.write_str(",m]"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid cardinality: {0:?}")]
pub struct ParseCardinalityError(pub String);

impl FromStr for Cardinality {
    type Err = ParseCardinalityError;

    /// Reads `[0,n]` (brackets optional, or parentheses; `*` as the minimum for any; `n`, `N` or
    /// `*` as the maximum for many, `m` or `M` for the second many) and UML's `1..*`, `0..1`,
    /// `1` and `*`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseCardinalityError(s.to_owned());
        let text = s.trim();
        let text = text
            .strip_prefix('[')
            .and_then(|t| t.strip_suffix(']'))
            .or_else(|| text.strip_prefix('(').and_then(|t| t.strip_suffix(')')))
            .unwrap_or(text);
        let count = |b: &str| b.trim().parse::<u32>().map_err(|_| err());
        let max = |b: &str| match b.trim() {
            "n" | "N" | "*" => Ok(CardinalityMax::Many),
            "m" | "M" => Ok(CardinalityMax::ManyM),
            n => count(n).map(CardinalityMax::Count),
        };
        let cardinality = match text.split_once(',').or_else(|| text.split_once("..")) {
            Some((min, upper)) => {
                let min = match min.trim() {
                    "*" => CardinalityMin::Any,
                    n => CardinalityMin::Count(count(n)?),
                };
                Self { min, max: max(upper)? }
            }
            None if text.trim() == "*" => Self::ZERO_MANY,
            None => {
                let n = count(text)?;
                Self::new(n, CardinalityMax::Count(n))
            }
        };
        match (cardinality.min, cardinality.max) {
            (_, CardinalityMax::Count(0)) => Err(err()),
            (CardinalityMin::Count(min), CardinalityMax::Count(max)) if max < min => Err(err()),
            _ => Ok(cardinality),
        }
    }
}

impl Serialize for Cardinality {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Cardinality {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// An entity taking part in a relationship (or an associative entity), drawn as a line with
/// the cardinality at the entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErParticipationProps {
    pub relationship: Id,
    pub entity: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cardinality: Option<Cardinality>,
    /// How often each instance of the entity takes part, read at the entity, as diagrams with
    /// [`DiagramProps::look_here`](crate::DiagramProps::look_here) show it. Only for
    /// relationships without exactly two participations: for those it is the other end's
    /// `cardinality`, otherwise it does not follow from the cardinalities.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look_here: Option<Cardinality>,
    /// The entity's role, needed when it takes part twice.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Every instance of the entity takes part: a double line.
    #[serde(default, skip_serializing_if = "is_default")]
    pub total: bool,
}

impl ErParticipationProps {
    pub fn new(relationship: Id, entity: Id) -> Self {
        Self {
            relationship,
            entity,
            cardinality: Some(Cardinality::ZERO_MANY),
            look_here: None,
            role: String::new(),
            total: false,
        }
    }
}

/// The line from an IS-A to its supertype or to one of its subtypes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErSubtypingProps {
    pub is_a: Id,
    pub entity: Id,
    #[serde(default, skip_serializing_if = "is_default")]
    pub supertype: bool,
}

#[cfg(test)]
mod tests {
    use super::{Cardinality, ErAction};

    #[test]
    fn actions_read_their_sql_words() {
        for a in ErAction::ALL {
            assert_eq!(a.sql().parse::<ErAction>(), Ok(a));
        }
        assert_eq!("set_null".parse::<ErAction>(), Ok(ErAction::SetNull));
        assert_eq!("noAction".parse::<ErAction>(), Ok(ErAction::NoAction));
        assert!("drop".parse::<ErAction>().is_err());
    }

    #[test]
    fn cardinalities_read_chen_and_uml_forms() {
        let read = |s: &str| s.parse::<Cardinality>().unwrap().to_string();
        assert_eq!(read("[0,n]"), "[0,n]");
        assert_eq!(read("1,1"), "[1,1]");
        assert_eq!(read(" [1, *] "), "[1,n]");
        assert_eq!(read("1..*"), "[1,n]");
        assert_eq!(read("0..1"), "[0,1]");
        assert_eq!(read("1"), "[1,1]");
        assert_eq!(read("*"), "[0,n]");
        assert_eq!(read("[2,5]"), "[2,5]");
        assert_eq!(read("[0,m]"), "[0,m]");
        assert_eq!(read("[*,M]"), "[*,m]");
        assert_eq!(read("*,n"), "[*,n]");
        // As read at the entity, in parentheses.
        assert_eq!(read("(1,n)"), "[1,n]");
        assert_eq!(Cardinality::ZERO_MANY.parenthesized(), "(0,n)");
        for bad in ["", "[2,1]", "[0,0]", "x", "[n,1]", "[1,2", "[*,0]"] {
            assert!(bad.parse::<Cardinality>().is_err(), "{bad}");
        }
    }
}
