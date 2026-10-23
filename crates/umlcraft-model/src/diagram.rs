//! Diagrams and presentations: how elements are shown, independent of what they mean.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::geometry::{Point, Rect};
use crate::id::Id;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagramKind {
    Class,
    UseCase,
    Sequence,
    Activity,
}

/// One visible item on a diagram. Presentations of a diagram are kept in z-order (back to front).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presentation {
    pub id: Id,
    /// The model element shown. `None` for purely graphical items such as note anchors.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<Id>,
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

impl Presentation {
    pub fn node(element: Option<Id>, bounds: Rect) -> Self {
        Self { id: Id::generate(), element, shape: Shape::Node { bounds }, style: Style::default() }
    }

    pub fn edge(element: Option<Id>, source: Id, target: Id, waypoints: Vec<Point>) -> Self {
        Self {
            id: Id::generate(),
            element,
            shape: Shape::Edge {
                source,
                target,
                waypoints,
                source_anchor: None,
                target_anchor: None,
                labels: EdgeLabels::default(),
            },
            style: Style::default(),
        }
    }

    pub fn bounds(&self) -> Option<Rect> {
        match self.shape {
            Shape::Node { bounds } => Some(bounds),
            Shape::Edge { .. } => None,
        }
    }

    pub fn is_edge(&self) -> bool {
        matches!(self.shape, Shape::Edge { .. })
    }

    /// Source and target presentation of an edge.
    pub fn endpoints(&self) -> Option<(Id, Id)> {
        match self.shape {
            Shape::Edge { source, target, .. } => Some((source, target)),
            Shape::Node { .. } => None,
        }
    }

    pub fn is_attached_to(&self, presentation: Id) -> bool {
        self.endpoints().is_some_and(|(s, t)| s == presentation || t == presentation)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Shape {
    Node {
        bounds: Rect,
    },
    /// Connects two presentations of the same diagram (nodes or other edges).
    Edge {
        source: Id,
        target: Id,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        waypoints: Vec<Point>,
        /// Where the line aims inside the source node, relative to its bounds ((0, 0) top left,
        /// (1, 1) bottom right). The line starts where it leaves the node. Unset: the center.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_anchor: Option<Point>,
        /// Like `source_anchor`, for the target node.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_anchor: Option<Point>,
        /// Labels placed by hand instead of by the layout.
        #[serde(default, skip_serializing_if = "EdgeLabels::is_empty")]
        labels: EdgeLabels,
    },
}

/// Positions of edge labels placed by hand: the top left corner of each label, relative to the
/// point it belongs to (the end of the line for role and multiplicity, the middle of the line for
/// the name). Unset labels go where the layout puts them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeLabels {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_role: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_multiplicity: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_role: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_multiplicity: Option<Point>,
}

impl EdgeLabels {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Classifier boxes: hide the attribute compartment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_attributes: bool,
    /// Classifier boxes: hide the operation compartment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_operations: bool,
    /// Interfaces: draw as a small circle ("lollipop") instead of a box.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub iconic: bool,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// RGBA color, written as `#rrggbb` or `#rrggbbaa` in files.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)?;
        if self.a != 255 {
            write!(f, "{:02x}", self.a)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("invalid color: {0:?}")]
pub struct ParseColorError(pub String);

impl FromStr for Color {
    type Err = ParseColorError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseColorError(s.to_owned());
        let hex = s.strip_prefix('#').ok_or_else(err)?;
        if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
            return Err(err());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| err());
        Ok(Self {
            r: byte(0)?,
            g: byte(2)?,
            b: byte(4)?,
            a: if hex.len() == 8 { byte(6)? } else { 255 },
        })
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// The presentations of one diagram, in z-order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub(crate) presentations: Vec<Presentation>,
}

impl View {
    pub fn presentations(&self) -> &[Presentation] {
        &self.presentations
    }

    pub fn get(&self, id: Id) -> Option<&Presentation> {
        self.presentations.iter().find(|p| p.id == id)
    }

    pub(crate) fn index_of(&self, id: Id) -> Option<usize> {
        self.presentations.iter().position(|p| p.id == id)
    }

    /// Edges whose source or target is `presentation`.
    pub fn edges_attached_to(&self, presentation: Id) -> impl Iterator<Item = &Presentation> {
        self.presentations.iter().filter(move |p| p.is_attached_to(presentation))
    }

    /// Presentations showing `element`.
    pub fn presentations_of(&self, element: Id) -> impl Iterator<Item = &Presentation> {
        self.presentations.iter().filter(move |p| p.element == Some(element))
    }

    /// Bounding box of all nodes, if there are any.
    pub fn content_bounds(&self) -> Option<Rect> {
        self.presentations.iter().filter_map(Presentation::bounds).reduce(|a, b| a.union(&b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_roundtrip() {
        let c: Color = "#ff8000".parse().unwrap();
        assert_eq!(c, Color::rgb(255, 128, 0));
        assert_eq!(c.to_string(), "#ff8000");
        let c: Color = "#01020380".parse().unwrap();
        assert_eq!(c.to_string(), "#01020380");
        assert!("ff8000".parse::<Color>().is_err());
        assert!("#ff80".parse::<Color>().is_err());
        assert!("#gg0000".parse::<Color>().is_err());
    }
}
