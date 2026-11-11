//! Diagrams and presentations: how elements are shown, independent of what they mean.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::element::Visibility;
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
    /// The inside of a structured class: its parts, ports and the connectors between them.
    CompositeStructure,
    /// Entities, attributes, relationships and IS-As in Chen notation.
    EntityRelationship,
}

/// One visible item on a diagram. Presentations of a diagram are kept in z-order (back to front).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Presentation {
    pub id: Id,
    /// The model element shown. `None` for purely graphical items such as free lines, texts
    /// and the lines anchoring a note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<Id>,
    #[serde(flatten)]
    pub shape: Shape,
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
}

impl Presentation {
    pub fn node(element: Option<Id>, bounds: Rect) -> Self {
        Self {
            id: Id::generate(),
            element,
            shape: Shape::Node { bounds, regions: None },
            style: Style::default(),
        }
    }

    /// How the regions of an orthogonal state are laid out, for a node showing one.
    pub fn regions(&self) -> Option<&Regions> {
        match &self.shape {
            Shape::Node { regions, .. } => regions.as_ref(),
            _ => None,
        }
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
            shape: Shape::Activation { lifeline, top, height, width: None },
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
                loop_x: None,
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

    /// A free text: `text` with its box at `bounds`.
    pub fn text(bounds: Rect, text: String) -> Self {
        Self {
            id: Id::generate(),
            element: None,
            shape: Shape::Text { bounds, text },
            style: Style::default(),
        }
    }

    /// A free line from `from` to `to`, solid and without arrowheads.
    pub fn line(from: Point, to: Point) -> Self {
        Self {
            id: Id::generate(),
            element: None,
            shape: Shape::Line { from, to, dashed: false, start_arrow: false, end_arrow: false },
            style: Style::default(),
        }
    }

    /// The box of a node, fragment or free text; the head of a lifeline.
    pub fn bounds(&self) -> Option<Rect> {
        match self.shape {
            Shape::Node { bounds, .. }
            | Shape::Fragment { bounds, .. }
            | Shape::Text { bounds, .. } => Some(bounds),
            Shape::Lifeline { head, .. } => Some(head),
            Shape::Edge { .. }
            | Shape::Activation { .. }
            | Shape::Message { .. }
            | Shape::Line { .. } => None,
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
            | Shape::Fragment { .. }
            | Shape::Text { .. }
            | Shape::Line { .. } => None,
        }
    }

    /// Whether this edge, message or activation hangs on `presentation` and goes with it.
    pub fn is_attached_to(&self, presentation: Id) -> bool {
        match self.shape {
            Shape::Activation { lifeline, .. } => lifeline == presentation,
            _ => self.endpoints().is_some_and(|(s, t)| s == presentation || t == presentation),
        }
    }

    /// Whether every coordinate, size and font size is a finite number. JSON has no NaN or
    /// infinity, so a project with one could be saved but not opened again.
    pub fn is_finite(&self) -> bool {
        let rect = |r: &Rect| [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite());
        let point = |p: &Point| p.x.is_finite() && p.y.is_finite();
        let points = |p: &[Option<Point>]| p.iter().flatten().all(point);
        let numbers = |n: &[Option<f32>]| n.iter().flatten().all(|v| v.is_finite());
        let labels = |l: &EdgeLabels| {
            points(&[
                l.name,
                l.source_role,
                l.source_multiplicity,
                l.target_role,
                l.target_multiplicity,
                l.keyword,
            ])
        };
        let shape = match &self.shape {
            Shape::Node { bounds, regions } => {
                rect(bounds) && regions.iter().flat_map(|r| &r.separators).all(|v| v.is_finite())
            }
            Shape::Edge { waypoints, source_anchor, target_anchor, labels: l, .. } => {
                waypoints.iter().all(point)
                    && points(&[*source_anchor, *target_anchor])
                    && labels(l)
            }
            Shape::Lifeline { head, length, .. } => rect(head) && length.is_finite(),
            Shape::Fragment { bounds, separators, guards } => {
                rect(bounds) && separators.iter().all(|v| v.is_finite()) && points(guards)
            }
            Shape::Activation { top, height, width, .. } => {
                numbers(&[Some(*top), Some(*height), *width])
            }
            Shape::Message { y, end_y, gate_x, loop_x, labels: l, .. } => {
                numbers(&[Some(*y), *end_y, *gate_x, *loop_x]) && labels(l)
            }
            Shape::Text { bounds, .. } => rect(bounds),
            Shape::Line { from, to, .. } => point(from) && point(to),
        };
        shape && numbers(&[self.style.font_size]) && points(&[self.style.name_offset])
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "shape", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Shape {
    Node {
        bounds: Rect,
        /// Orthogonal states: where the lines between their regions go.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        regions: Option<Regions>,
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
    /// follows from the lifeline and from the bars it lies within. `width` is unset for the
    /// usual 10; Astah's files may have bars of width 0, drawn as a line.
    Activation {
        lifeline: Id,
        top: f32,
        height: f32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<f32>,
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
        /// Messages to self: x of the loop's far side relative to the lifeline's center, where
        /// it was dragged away from its usual place 40 right of the bar.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        loop_x: Option<f32>,
        /// The name label, if placed by hand.
        #[serde(default, skip_serializing_if = "EdgeLabels::is_empty")]
        labels: EdgeLabels,
    },
    /// A text of its own on the diagram, not tied to a model element (Astah's free text). It
    /// starts 10 right of the box's left edge, its first baseline 17 below the top.
    Text { bounds: Rect, text: String },
    /// A line of its own from `from` to `to`, not tied to a model element (Astah's free line).
    Line {
        from: Point,
        to: Point,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        dashed: bool,
        /// An open arrowhead at `from`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        start_arrow: bool,
        /// An open arrowhead at `to`.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        end_arrow: bool,
    },
}

/// How an orthogonal state shows its regions: one below the other, divided by horizontal
/// lines, or side by side (`vertical` lines). `separators` are the lines' distances from the
/// state's top (or left) edge, one between each two regions, as Astah keeps them; the first
/// region includes the state's name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Regions {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vertical: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub separators: Vec<f32>,
}

/// Positions of edge labels placed by hand: the top left corner of each label, relative to the
/// point it belongs to (the end of the line for role and multiplicity, the middle of the line for
/// the name and for «include» or «extend»). Unset labels go where the layout puts them.
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
    /// «include» or «extend».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyword: Option<Point>,
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
    /// Edges: color of the line and its arrow heads; unset, the theme's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Color>,
    /// Color of the name, of a node or on an edge; unset, the theme's text color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    /// Nodes: leave out the stereotypes above the name. Keywords such as «interface» stay.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_stereotypes: bool,
    /// Classifier boxes: leave out the members' visibility (`+`, `-`, …).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_member_visibility: bool,
    /// Classifier boxes: leave out the attributes' types.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_attribute_types: bool,
    /// Classifier boxes: leave out the attributes' initial values.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_initial_values: bool,
    /// Classifier boxes: leave out the attributes' stereotypes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_attribute_stereotypes: bool,
    /// Classifier boxes: leave out the operations' stereotypes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_operation_stereotypes: bool,
    /// Classifier boxes: leave out the operations' return types.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_return_types: bool,
    /// Classifier boxes: leave out the attributes' constraints (`{age > 0}`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_attribute_constraints: bool,
    /// Classifier boxes: leave out the operations' constraints.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_operation_constraints: bool,
    /// Edges: leave out the constraints below the name.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_constraints: bool,
    /// Edges: how far each constraint label (`{…}`, by the constraint's index) was moved by
    /// hand from where the layout puts it, as Astah's `localMovement`; unset or missing, not
    /// moved.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraint_offsets: Vec<Option<Point>>,
    /// Generalizations: drawn in Astah's shared style, one triangle at the general element
    /// for all generalizations of the same style that end at its node, their lines joining
    /// on a common line across (vertical style) or up and down (horizontal style). The
    /// waypoints keep where that common line is.
    #[serde(default, skip_serializing_if = "SharedStyle::is_separate")]
    pub shared: SharedStyle,
    /// Generalizations: `shared` was chosen rather than given by arranging the diagram, which
    /// then keeps lines chosen separate apart.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shared_chosen: bool,
    /// Classifier boxes: which parameter directions the operations show.
    #[serde(default, skip_serializing_if = "ParameterDirections::is_default")]
    pub parameter_directions: ParameterDirections,
    /// Classifier boxes: the attributes of these visibilities are left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_attributes: Vec<Visibility>,
    /// Classifier boxes: the operations of these visibilities are left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_operations: Vec<Visibility>,
    /// Classifier boxes: hide the attribute compartment; objects: the slot compartment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_attributes: bool,
    /// Instances and lifelines: leave out the name (`: Client`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_instance_name: bool,
    /// Instances and lifelines: leave out what they are an instance of or represent (`client`
    /// instead of `client : Client`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_instance_type: bool,
    /// Objects: leave out the slots without a value.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_empty_slots: bool,
    /// Classifier boxes: hide the operation compartment.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_operations: bool,
    /// Classifier boxes: sized by their content as Astah does (its "auto resize"), as wide as
    /// the widest text and as high as the compartments, instead of keeping their bounds. Use
    /// cases: as Astah draws them, the name broken only at its line breaks and the ellipse
    /// widened around its center to hold it (set for use cases from `.asta` files). Objects: as
    /// wide as their widest text, around their center.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_size: bool,
    /// Interfaces: draw as a small circle ("lollipop") instead of a box. Lifelines: draw the
    /// head as the icon of the represented classifier.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub iconic: bool,
    /// Interfaces drawn iconic: as a socket, a half circle open away from the line that ends
    /// at it, the notation of an interface a class or port requires.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub socket: bool,
    /// Messages: leave out the parentheses with the arguments. Classifier boxes: leave out the
    /// operations' parameters (`total() : int`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_parameters: bool,
    /// Messages that show the parameters of the operation they call (calls without arguments)
    /// and classifier boxes: leave out the parameters' types.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_parameter_types: bool,
    /// Messages that show the parameters of the operation they call: add its return type.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub show_return_type: bool,
    /// System boundaries: where the box of the name's text lies relative to the boundary's top
    /// left, as Astah keeps a free text (the text starts 10 right of the box, its baseline 17
    /// below the box's top). Unset: centered at the top. Ports: where the box of the name lies
    /// relative to the port's top left, as Astah keeps it; unset, outside the border the port
    /// sits on. Duration and time constraints: how far their label was moved by hand from where
    /// the layout puts it, as Astah's `localMovement`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_offset: Option<Point>,
    /// Edges: how the line runs through its waypoints.
    #[serde(default, skip_serializing_if = "LineShape::is_line")]
    pub line_shape: LineShape,
    /// Edges: laid out by the router (`umlcraft_notation::autoroute`), right-angled around the
    /// nodes, and laid out again when its nodes move. Editing the route by hand turns it off.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_route: bool,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Which parameter directions an operation shows in a classifier box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ParameterDirections {
    /// `out` and `inout`; `in` goes without saying.
    #[default]
    Explicit,
    /// None, as Astah shows them unless told otherwise.
    Hidden,
    /// All, `in` as well, as Astah shows them when told to.
    All,
}

impl ParameterDirections {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Whether generalizations to one element share their triangle, Astah's "shared style".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SharedStyle {
    /// Each line has its own triangle.
    #[default]
    Separate,
    /// The lines leave the general element vertically and meet on a horizontal line.
    Vertical,
    /// The lines leave the general element horizontally and meet on a vertical line.
    Horizontal,
}

impl SharedStyle {
    pub fn is_separate(&self) -> bool {
        *self == Self::Separate
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
    presentations: Vec<Presentation>,
    /// Where each presentation is in `presentations`, so lookups by id do not scan the list.
    positions: HashMap<Id, usize>,
}

impl View {
    pub(crate) fn new(presentations: Vec<Presentation>) -> Self {
        let mut view = Self { presentations, positions: HashMap::new() };
        view.reindex(0);
        view
    }

    pub fn presentations(&self) -> &[Presentation] {
        &self.presentations
    }

    pub fn get(&self, id: Id) -> Option<&Presentation> {
        self.index_of(id).map(|i| &self.presentations[i])
    }

    pub(crate) fn index_of(&self, id: Id) -> Option<usize> {
        self.positions.get(&id).copied()
    }

    pub(crate) fn insert(&mut self, index: usize, presentation: Presentation) {
        self.presentations.insert(index, presentation);
        self.reindex(index);
    }

    pub(crate) fn remove(&mut self, index: usize) -> Presentation {
        let presentation = self.presentations.remove(index);
        self.positions.remove(&presentation.id);
        self.reindex(index);
        presentation
    }

    /// Puts `presentation` in the place of the one with its id and returns that one.
    pub(crate) fn replace(&mut self, index: usize, presentation: Presentation) -> Presentation {
        std::mem::replace(&mut self.presentations[index], presentation)
    }

    pub(crate) fn reorder(&mut self, from: usize, to: usize) {
        let presentation = self.presentations.remove(from);
        self.presentations.insert(to, presentation);
        self.reindex(from.min(to));
    }

    /// Updates the positions of the presentations from `from` on.
    fn reindex(&mut self, from: usize) {
        for (i, p) in self.presentations.iter().enumerate().skip(from) {
            self.positions.insert(p.id, i);
        }
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
