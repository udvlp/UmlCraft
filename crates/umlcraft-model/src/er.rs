//! The elements of entity-relationship diagrams in Chen notation: entities, their attributes,
//! relationships with the entities taking part in them, and IS-A generalizations.
//!
//! Entities, relationships and IS-As belong to a package; attributes to what they describe (an
//! entity, a relationship or a composite attribute), so that the line to the owner follows from
//! the ownership tree. A participation joins a relationship (or an associative entity) to an
//! entity and belongs to the relationship; a subtyping joins an IS-A to one of its entities and
//! belongs to the IS-A.

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
}

/// A rhombus; an identifying relationship (of a weak entity) has a second one inside.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErRelationshipProps {
    #[serde(default, skip_serializing_if = "is_default")]
    pub identifying: bool,
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

    /// Reads `[0,n]` (brackets optional; `*` as the minimum for any; `n`, `N` or `*` as the
    /// maximum for many, `m` or `M` for the second many) and UML's `1..*`, `0..1`, `1` and `*`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseCardinalityError(s.to_owned());
        let text = s.trim();
        let text = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')).unwrap_or(text);
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
    use super::Cardinality;

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
        for bad in ["", "[2,1]", "[0,0]", "x", "[n,1]", "[1,2", "[*,0]"] {
            assert!(bad.parse::<Cardinality>().is_err(), "{bad}");
        }
    }
}
