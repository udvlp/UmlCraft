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
    StateMachine,
    Component,
    Deployment,
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

    /// A lifeline: its head at `head`, the line `length` long below it.
    pub fn lifeline(element: Id, head: Rect, length: f32) -> Self {
        Self {
            id: Id::generate(),
            element: Some(element),
            shape: Shape::Lifeline { head, length, destroyed: false },
            style: Style::default(),
        }
    }

    pub fn activation(lifeline: Id, top: f32, height: f32) -> Self {
        Self {
            id: Id::generate(),
            element: None,
            shape: Shape::Activation { lifeline, top, height },
            style: Style::default(),
        }
    }

    pub fn message(element: Id, source: Id, target: Id, y: f32) -> Self {
        Self {
            id: Id::generate(),
            element: Some(element),
            shape: Shape::Message {
                source,
                target,
                y,
                end_y: None,
                gate_x: None,
                labels: EdgeLabels::default(),
            },
            style: Style::default(),
        }
    }

    pub fn fragment(element: Id, bounds: Rect) -> Self {
        Self {
            id: Id::generate(),
            element: Some(element),
            shape: Shape::Fragment { bounds, separators: Vec::new(), guards: Vec::new() },
            style: Style::default(),
        }
    }

    /// The box of a node or fragment; the head of a lifeline.
    pub fn bounds(&self) -> Option<Rect> {
        match self.shape {
            Shape::Node { bounds } | Shape::Fragment { bounds, .. } => Some(bounds),
            Shape::Lifeline { head, .. } => Some(head),
            Shape::Edge { .. } | Shape::Activation { .. } | Shape::Message { .. } => None,
        }
    }

    pub fn is_edge(&self) -> bool {
        matches!(self.shape, Shape::Edge { .. } | Shape::Message { .. })
    }

    /// Source and target presentation of an edge or message.
    pub fn endpoints(&self) -> Option<(Id, Id)> {
        match self.shape {
            Shape::Edge { source, target, .. } | Shape::Message { source, target, .. } => {
                Some((source, target))
            }
            Shape::Node { .. }
            | Shape::Lifeline { .. }
            | Shape::Activation { .. }
            | Shape::Fragment { .. } => None,
        }
    }

    /// Whether this edge, message or activation hangs on `presentation` and goes with it.
    pub fn is_attached_to(&self, presentation: Id) -> bool {
        match self.shape {
            Shape::Activation { lifeline, .. } => lifeline == presentation,
            _ => self.endpoints().is_some_and(|(s, t)| s == presentation || t == presentation),
        }
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
    /// Sequence diagrams: the head of a lifeline and the length of its dashed line, which
    /// starts below the head (below the name, for heads drawn as icons).
    Lifeline {
        head: Rect,
        length: f32,
        /// The line ends in a cross.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        destroyed: bool,
    },
    /// Sequence diagrams: a combined fragment's frame. Operands after the first start
    /// `separators` below the top; `guards` are hand-placed offsets of the operands' guards
    /// from where the layout puts them.
    Fragment {
        bounds: Rect,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        separators: Vec<f32>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        guards: Vec<Option<Point>>,
    },
    /// Sequence diagrams: an activation bar on the lifeline presentation `lifeline`. Its x
    /// follows from the lifeline and from the bars it lies within.
    Activation {
        lifeline: Id,
        top: f32,
        height: f32,
    },
    /// Sequence diagrams: a message from lifeline presentation `source` to `target` (the same
    /// one for messages to self and for found and lost messages). It leaves the sender at `y`
    /// and reaches the receiver at `end_y` if that differs (messages to self, messages that
    /// branch off another). The ends' x follow from the lifelines and their activations.
    Message {
        source: Id,
        target: Id,
        y: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_y: Option<f32>,
        /// Found and lost messages: x of the open end (a dot, or a gate on the frame).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        gate_x: Option<f32>,
        /// The name label, if placed by hand.
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
    /// Classifier boxes: sized by their content as Astah does (its "auto resize"), as wide as
    /// the widest text and as high as the compartments, instead of keeping their bounds.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_size: bool,
    /// Interfaces: draw as a small circle ("lollipop") instead of a box. Lifelines: draw the
    /// head as the icon of the represented classifier.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub iconic: bool,
    /// Messages: leave out the parentheses with the arguments.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_parameters: bool,
    /// Edges: how the line runs through its waypoints.
    #[serde(default, skip_serializing_if = "LineShape::is_line")]
    pub line_shape: LineShape,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// How an edge runs from end to end, Astah's "line style" (`line.shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LineShape {
    /// Straight segments through the waypoints.
    #[default]
    Line,
    /// Horizontal and vertical segments; the waypoints are the corners.
    RightAngle,
    /// One smooth curve. As in Astah, a cubic Bézier curve from end to end whose control points
    /// are the first and the last waypoint.
    Curve,
    /// A curve over corners kept at right angles.
    CurveRightAngle,
}

impl LineShape {
    pub fn is_line(&self) -> bool {
        *self == Self::Line
    }

    /// The waypoints are corners of horizontal and vertical segments.
    pub fn is_right_angle(self) -> bool {
        matches!(self, Self::RightAngle | Self::CurveRightAngle)
    }

    pub fn is_curve(self) -> bool {
        matches!(self, Self::Curve | Self::CurveRightAngle)
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

    /// Bounding box of all nodes and lifelines, if there are any.
    pub fn content_bounds(&self) -> Option<Rect> {
        let area = |p: &Presentation| match p.shape {
            Shape::Lifeline { head, length, .. } => {
                Some(Rect::new(head.x, head.y, head.width, head.height + length))
            }
            _ => p.bounds(),
        };
        self.presentations.iter().filter_map(area).reduce(|a, b| a.union(&b))
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
