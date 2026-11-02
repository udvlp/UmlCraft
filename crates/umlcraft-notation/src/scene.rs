//! Turns a diagram view into drawable shapes in logical coordinates. The scene also carries the
//! layout of every text row, so painting, hit testing and inline editing agree on positions.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd, Color, DiagramKind,
    EdgeLabels, Element, ElementKind, Id, InstanceKind, LineShape, LinkEnd, MessageSort, Model,
    Navigability, Point, Presentation, PseudostateKind, Qualifier, Rect, Regions, Shape, Size,
    StateProps, Style, TransitionProps,
};

use crate::labels::{self, FreeLabel, LabelKind};
use crate::member::{member_label, member_shown};
use crate::route::{self, Route, anchor_point};
use crate::sequence::{self, SequenceLayout};

/// Measures text, so nodes can grow with their content. The app measures with the real font;
/// [`ApproxMeasure`] is good enough for tests and headless use.
pub trait TextMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32;

    /// The width with every character's advance in whole pixels, as Java measures Astah's font
    /// ([`metrics`](crate::metrics)). Astah centers the names in boxes by this width and sizes
    /// tabs by it, though it draws the texts with exact advances.
    fn rounded_width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        crate::metrics::java_width(text, font_size, bold)
            .unwrap_or_else(|| self.width(text, font_size, bold))
    }

    /// The width Astah sizes boxes, labels and tabs by: the larger of the whole-pixel width
    /// and the exact one.
    fn astah_width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        self.width(text, font_size, bold).max(self.rounded_width(text, font_size, bold))
    }

    /// Whether members draw their visibility in UmlCraft's own column
    /// ([`visibility_column`]), a setting. Off, they look as in Astah.
    fn visibility_column(&self) -> bool {
        false
    }

    /// Whether abstract classes say so with the stereotype «abstract» above their name instead
    /// of UML's `{abstract}` below it, a setting.
    fn abstract_stereotype(&self) -> bool {
        false
    }

    /// Whether read-only attributes end in UML's `{readOnly}`, a setting.
    fn read_only_constraint(&self) -> bool {
        false
    }
}

pub struct ApproxMeasure;

impl TextMeasure for ApproxMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        text.chars().count() as f32 * font_size * if bold { 0.62 } else { 0.55 }
    }
}

pub struct Scene {
    pub nodes: Vec<NodeShape>,
    pub edges: Vec<EdgeShape>,
    /// Activations, insertion slots etc. of a sequence diagram.
    pub sequence: Option<SequenceLayout>,
}

impl Scene {
    pub fn empty() -> Self {
        Self { nodes: Vec::new(), edges: Vec::new(), sequence: None }
    }

    pub fn node(&self, id: Id) -> Option<&NodeShape> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn edge(&self, id: Id) -> Option<&EdgeShape> {
        self.edges.iter().find(|e| e.id == id)
    }

    /// Bounding box of all nodes and lines as drawn, so that a diagram of free lines or a loop
    /// to self fits too.
    pub fn bounds(&self) -> Option<Rect> {
        let nodes = self.nodes.iter().map(NodeShape::extent);
        let lines =
            self.edges.iter().flat_map(|e| e.path().into_owned()).map(|p| Rect::from_corners(p, p));
        nodes.chain(lines).reduce(|a, b| a.union(&b))
    }
}

pub struct NodeShape {
    pub id: Id,
    pub element: Option<Id>,
    /// Bounds as drawn: the stored bounds, grown to fit the content.
    pub rect: Rect,
    pub figure: Figure,
    pub rows: Vec<Row>,
    /// Fill color chosen for this presentation; unset, the default fill.
    pub fill: Option<Color>,
}

impl NodeShape {
    /// The rectangle lines end at: the drawn bounds, for an actor grown down over the name
    /// below the figure, so that a line from below stops under the name instead of crossing it.
    pub fn clip_rect(&self) -> Rect {
        let r = self.rect;
        if !matches!(self.figure, Figure::Actor) {
            return r;
        }
        let bottom = self.rows.iter().map(|row| row.rect.bottom()).fold(r.bottom(), f32::max);
        Rect::new(r.x, r.y, r.width, bottom - r.y)
    }

    /// Whether `p` hits the node. Hollow frames (system boundaries, combined fragments) only
    /// count at their border and texts, so clicks inside reach what they contain or the empty
    /// canvas. Lifelines count at their head and along their dashed line.
    pub fn hit(&self, p: Point, tolerance: f32) -> bool {
        if let Figure::Classifier { template: Some(t), .. } = &self.figure
            && t.inflate(tolerance).contains(p)
        {
            return true;
        }
        // Names drawn outside the figure (below a lollipop or an actor) belong to it.
        if self.rows.iter().any(|r| r.rect.contains(p)) {
            return true;
        }
        if !self.rect.inflate(tolerance).contains(p) {
            return false;
        }
        match &self.figure {
            Figure::Boundary
            | Figure::Fragment { .. }
            | Figure::Partition { .. }
            | Figure::Frame { .. } => {
                let inner = self.rect.inflate(-2.0 * tolerance.max(3.0));
                !inner.contains(p) || self.rows.iter().any(|r| r.rect.contains(p))
            }
            Figure::Lifeline { head, .. } => {
                head.contains(p) || (p.x - head.center().x).abs() <= tolerance + 4.0
            }
            _ => self.rect.contains(p),
        }
    }

    /// What the node covers as drawn: its bounds, a template box sticking out of them and
    /// names drawn outside them.
    pub fn extent(&self) -> Rect {
        let rect = match &self.figure {
            Figure::Classifier { template: Some(t), .. } => self.rect.union(t),
            _ => self.rect,
        };
        self.rows.iter().fold(rect, |r, row| r.union(&row.rect))
    }

    /// Whether the user may resize the node. Lifelines, activation bars and diagram frames
    /// are sized by the sequence layout.
    pub fn resizable(&self) -> bool {
        !matches!(self.figure, Figure::Lifeline { .. } | Figure::Activation | Figure::Frame { .. })
    }
}

/// What a compartment of a classifier box lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compartment {
    Attributes,
    Operations,
    Literals,
    /// An object's slots, one per attribute of its class.
    Slots,
}

/// A compartment of a classifier box under a point, for adding a member to it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompartmentHit {
    /// The box's presentation and the element it shows.
    pub node: Id,
    pub element: Id,
    pub compartment: Compartment,
    /// The last member listed there, if any: a new one goes after it.
    pub last: Option<Id>,
    /// Where a row added at its end goes.
    pub new_row: Rect,
}

/// The compartment of a classifier box at `p` that takes new members: attributes, operations
/// or literals, not an object's slots.
pub fn compartment_at(scene: &Scene, p: Point) -> Option<CompartmentHit> {
    scene.nodes.iter().rev().filter(|n| n.rect.contains(p)).find_map(|n| {
        let Figure::Classifier { separators, .. } = &n.figure else { return None };
        compartment_hit(n, separators.iter().rposition(|&y| y <= p.y)?)
    })
}

/// The compartment `compartment` of the box `node` shows, if it shows one.
pub fn compartment_of(scene: &Scene, node: Id, compartment: Compartment) -> Option<CompartmentHit> {
    let n = scene.node(node)?;
    let Figure::Classifier { compartments, .. } = &n.figure else { return None };
    compartment_hit(n, compartments.iter().position(|&c| c == compartment)?)
}

/// The `i`th compartment of the box `n`, unless it lists an object's slots.
fn compartment_hit(n: &NodeShape, i: usize) -> Option<CompartmentHit> {
    let Figure::Classifier { separators, compartments, .. } = &n.figure else { return None };
    let compartment = *compartments.get(i)?;
    if compartment == Compartment::Slots {
        return None;
    }
    let top = *separators.get(i)?;
    let bottom = separators.get(i + 1).copied().unwrap_or(n.rect.bottom());
    let rows: Vec<&Row> = n
        .rows
        .iter()
        .filter(|r| r.role == RowRole::Member && r.rect.y >= top && r.rect.y < bottom)
        .collect();
    let height = line_height(rows.first().map_or(FONT_SIZE, |r| r.size));
    let y = rows.last().map_or(top + BOX_PAD_Y, |r| r.rect.bottom());
    Some(CompartmentHit {
        node: n.id,
        element: n.element?,
        compartment,
        last: rows.last().and_then(|r| r.element),
        new_row: Rect::new(n.rect.x + BOX_PAD_X, y, n.rect.width - 2.0 * BOX_PAD_X, height),
    })
}

/// The outline of a node; its texts are in [`NodeShape::rows`].
pub enum Figure {
    /// Box with compartment separators at the given y positions, each above the compartment of
    /// the same index in `compartments`; a template's parameters are in a dashed box on its top
    /// right corner.
    Classifier {
        separators: Vec<f32>,
        compartments: Vec<Compartment>,
        template: Option<Rect>,
    },
    /// Interface drawn as a small circle.
    Lollipop,
    /// Package, with Astah's fork icon in the tab for a subsystem.
    Package {
        tab: Rect,
        subsystem: bool,
    },
    Actor,
    /// Ellipse; with extension points, a line at `separator` divides name and points.
    UseCase {
        separator: Option<f32>,
    },
    /// System boundary: a frame around the use cases of a system.
    Boundary,
    /// A lifeline: its head (a box or an icon) and the dashed line from `line.0` down to
    /// `line.1`.
    Lifeline {
        head: Rect,
        icon: HeadIcon,
        line: (f32, f32),
    },
    /// Activation bar on a lifeline.
    Activation,
    /// The frame around a diagram, with its kind and name in a tab at the top left.
    Frame {
        tab: Rect,
    },
    /// Combined fragment or interaction use: a frame with the operator in a tab at the top
    /// left and dashed lines between the operands. Interaction uses are filled.
    Fragment {
        tab: Rect,
        separators: Vec<f32>,
        filled: bool,
    },
    Note,
    /// Activity diagrams: an action, drawn with rounded corners.
    Action,
    /// Initial node: a filled circle.
    Initial,
    /// Activity final node: a ring around a filled circle.
    ActivityFinal,
    /// Flow final node: a crossed-out circle.
    FlowFinal,
    /// Decision or merge node.
    Diamond,
    /// Fork or join node: a filled bar.
    Bar,
    /// Object node.
    Rectangle,
    /// Swimlane with its name above a line at `header`. Hollow like a system boundary.
    Partition {
        header: f32,
    },
    /// State machines: a state with rounded corners; a line at `separator` divides the name
    /// from the internal activities or substates. An orthogonal state has its regions in order,
    /// divided by dashed lines: horizontal ones, or `vertical` ones from the separator down.
    State {
        separator: Option<f32>,
        regions: Vec<RegionShape>,
        vertical: bool,
    },
    /// History pseudostate: a circle around its letter.
    Circle,
    /// Component: a box with the component icon in `icon`.
    Component {
        icon: Rect,
    },
    /// Artifact: a box with a document icon in `icon`.
    Artifact {
        icon: Rect,
    },
    /// Deployment node: a box seen from the front right, `depth` deep.
    Cube {
        depth: f32,
    },
    /// A free text: only its rows are drawn.
    Text,
    Other,
}

/// The area of one region of an orthogonal state. Regions one below the other span the state's
/// width, the first one including the name; regions side by side start below the name and the
/// internal activities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionShape {
    pub region: Id,
    pub rect: Rect,
}

/// How a lifeline's head is drawn: a box with the name inside, or an icon with the name below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadIcon {
    Box,
    Actor,
    Boundary,
    Control,
    Entity,
}

impl Figure {
    /// The figure's outline, where edges end.
    fn outline(&self) -> Outline {
        match self {
            Self::Initial
            | Self::ActivityFinal
            | Self::FlowFinal
            | Self::Circle
            | Self::Lollipop
            | Self::UseCase { .. } => Outline::Ellipse,
            Self::Diamond => Outline::Diamond,
            _ => Outline::Rect,
        }
    }
}

/// Shapes whose border edges are clipped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outline {
    Rect,
    Ellipse,
    Diamond,
}

impl Outline {
    fn contains(self, rect: Rect, p: Point) -> bool {
        let c = rect.center();
        let (dx, dy) = ((p.x - c.x) / (rect.width / 2.0), (p.y - c.y) / (rect.height / 2.0));
        match self {
            Self::Rect => dx.abs() <= 1.0 && dy.abs() <= 1.0,
            Self::Ellipse => dx * dx + dy * dy <= 1.0,
            Self::Diamond => dx.abs() + dy.abs() <= 1.0,
        }
    }

    /// Where the line from `inside` toward `toward` leaves the outline.
    pub fn border_point(self, rect: Rect, inside: Point, toward: Point) -> Point {
        if self == Self::Rect || rect.width <= 0.0 || rect.height <= 0.0 {
            return border_point_from(rect, inside, toward);
        }
        if self.contains(rect, toward) || !self.contains(rect, inside) {
            return inside;
        }
        // Both outlines are convex, so the line crosses them once: bisect.
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        let at = |t: f32| {
            Point::new(inside.x + (toward.x - inside.x) * t, inside.y + (toward.y - inside.y) * t)
        };
        for _ in 0..24 {
            let mid = (lo + hi) / 2.0;
            if self.contains(rect, at(mid)) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        at(lo)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowRole {
    Stereotype,
    /// The node element's name (or a note's text).
    Name,
    /// A member element (attribute, operation, literal).
    Member,
    /// A slot of an object; the row's element is the slot's attribute, the node's the object.
    Slot,
}

/// One line of text inside or below a node.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub text: String,
    pub rect: Rect,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: LabelAlign,
    pub role: RowRole,
    /// Element this row edits: the node's element for names, the member for members.
    pub element: Option<Id>,
    /// Drawn with whole-pixel advances, as Astah draws the texts in fragment tabs.
    pub hinted: bool,
    /// A text color of its own; unset, the theme's.
    pub color: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    /// Astah's dashes for replies and create messages.
    Dashed,
    /// Lifelines.
    LongDashed,
    /// Dependencies, realizations, include, extend, note anchors; separators between operands.
    ShortDashed,
    /// Template boxes.
    Dotted,
}

impl LineStyle {
    /// Lengths of dashes and gaps, in logical units; empty for a solid line.
    pub fn dashes(self) -> &'static [f32] {
        match self {
            Self::Solid => &[],
            Self::Dashed => &[10.0, 5.0],
            Self::LongDashed => &[12.0, 6.0],
            Self::ShortDashed => &[5.0, 5.0],
            Self::Dotted => &[3.0, 3.0],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndMarker {
    None,
    OpenArrow,
    FilledArrow,
    HollowTriangle,
    HollowDiamond,
    FilledDiamond,
    /// A diamond at an end that is not navigable either: the cross right behind it, as Astah
    /// draws it.
    DiamondCross {
        filled: bool,
    },
    /// A diamond at a navigable end: an open arrow behind it. Astah draws only the arrow there,
    /// which hides the aggregation.
    DiamondArrow {
        filled: bool,
    },
    /// A small x across the line near the end: the end is not navigable.
    Cross,
    /// A circle with a cross inside at the end: the owner of a nested element.
    Containment,
}

#[derive(Clone, Debug)]
pub struct EdgeShape {
    pub id: Id,
    /// Polyline from the source border through the waypoints to the target border. For curves,
    /// the control polygon; [`EdgeShape::path`] is the line as drawn.
    pub points: Vec<Point>,
    pub line: LineStyle,
    /// Straight, right-angled or curved.
    pub shape: LineShape,
    pub source_marker: EndMarker,
    pub target_marker: EndMarker,
    pub labels: Vec<EdgeLabel>,
    /// The route follows from the layout (sequence messages); it has no waypoints to drag.
    pub fixed: bool,
    /// Bounds of the source and target node, and where the line aims inside them as stored.
    pub ends: [Rect; 2],
    pub anchors: [Option<Point>; 2],
    /// The filled triangle next to an association's name that shows which way it reads.
    pub name_arrow: Option<[Point; 3]>,
    /// The boxes of qualified association ends, against their node, with one row per qualifier.
    pub qualifiers: Vec<(Rect, Vec<Row>)>,
    /// Where the line meets the source and target node. A qualifier box moves the end of the
    /// line away from it; placed end labels stay relative to it, as in Astah's files.
    pub node_tips: [Point; 2],
    /// The labels not placed by hand, which [`labels::place`](crate::labels::place) moves to
    /// where they cover least.
    pub free_labels: Vec<FreeLabel>,
    /// The line stands for an association; its role and multiplicity labels are in
    /// `end_labels`.
    pub association: bool,
    pub end_labels: Vec<EndLabel>,
    /// A color of its own for the line and its arrow heads; unset, the theme's.
    pub stroke: Option<Color>,
}

/// A role name or multiplicity among an edge's labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndLabel {
    /// Its index in [`EdgeShape::labels`].
    pub index: usize,
    /// The association end it belongs to: 0 for the source, 1 for the target.
    pub end: usize,
    pub multiplicity: bool,
}

/// A role name or multiplicity to edit in place, from [`end_label_at`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EndLabelHit {
    pub edge: Id,
    pub end: usize,
    pub multiplicity: bool,
    /// Where the label is, or would go for a role not given yet.
    pub rect: Rect,
}

/// A grip on a selected edge: an existing waypoint, the middle of a segment where dragging
/// inserts a new waypoint, or (on right-angled lines) a segment that moves across.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeHandle {
    Waypoint(usize),
    Insert(usize),
    Segment(usize),
}

fn middle(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

impl EdgeShape {
    pub fn waypoints(&self) -> &[Point] {
        &self.points[1..self.points.len() - 1]
    }

    /// The line as drawn: the points, or for curves many points along the curve.
    pub fn path(&self) -> Cow<'_, [Point]> {
        if self.shape.is_curve() {
            Cow::Owned(route::flatten(&route::bezier(&self.points)))
        } else {
            Cow::Borrowed(&self.points)
        }
    }

    /// The route as drawn, with the anchors as stored.
    pub fn route(&self) -> Route {
        Route {
            waypoints: self.waypoints().to_vec(),
            source_anchor: self.anchors[0],
            target_anchor: self.anchors[1],
        }
    }

    /// Grips in drawing order: waypoints first, then segment middles. Right-angled lines are
    /// changed by moving their segments.
    pub fn handles(&self) -> Vec<(EdgeHandle, Point)> {
        if self.fixed {
            return Vec::new();
        }
        if self.shape.is_right_angle() {
            return self
                .points
                .windows(2)
                .enumerate()
                .map(|(i, w)| (EdgeHandle::Segment(i), middle(w[0], w[1])))
                .collect();
        }
        let waypoints =
            self.waypoints().iter().enumerate().map(|(i, &p)| (EdgeHandle::Waypoint(i), p));
        let inserts = self
            .points
            .windows(2)
            .enumerate()
            .map(|(i, w)| (EdgeHandle::Insert(i), middle(w[0], w[1])));
        waypoints.chain(inserts).collect()
    }

    pub fn handle_at(&self, p: Point, tolerance: f32) -> Option<EdgeHandle> {
        self.handles()
            .into_iter()
            .find(|(_, hp)| (hp.x - p.x).abs() <= tolerance && (hp.y - p.y).abs() <= tolerance)
            .map(|(h, _)| h)
    }
}

/// Waypoints after dragging `handle` to `to`.
pub fn drag_waypoint(waypoints: &[Point], handle: EdgeHandle, to: Point) -> Vec<Point> {
    let mut result = waypoints.to_vec();
    match handle {
        EdgeHandle::Waypoint(i) if i < result.len() => result[i] = to,
        EdgeHandle::Insert(i) => result.insert(i.min(result.len()), to),
        EdgeHandle::Waypoint(_) | EdgeHandle::Segment(_) => {}
    }
    result
}

/// The route of `edge` (as drawn) when it gets the line style `shape`, as Astah changes it: a
/// straight line loses its waypoints, a curve keeps them, and a line that gets right angles
/// has its waypoints turned into corners.
pub fn restyle(edge: &EdgeShape, shape: LineShape) -> Route {
    let route = edge.route();
    let [source, target] = edge.ends;
    let (from, to) =
        (anchor_point(source, route.source_anchor), anchor_point(target, route.target_anchor));
    let waypoints = match shape {
        LineShape::Line => Vec::new(),
        LineShape::Curve => route.waypoints.clone(),
        _ if edge.shape.is_right_angle() => route.waypoints.clone(),
        _ => route::orthogonalize(source, from, &route.waypoints, target, to),
    };
    Route { waypoints, ..route }
}

/// The route of `edge` (as drawn when the drag started) after dragging `handle` to `to`.
pub fn drag_handle(edge: &EdgeShape, handle: EdgeHandle, to: Point) -> Route {
    let route = edge.route();
    match handle {
        EdgeHandle::Segment(i) => route::drag_segment(&edge.points, &route, edge.ends, i, to),
        _ => Route { waypoints: drag_waypoint(&route.waypoints, handle, to), ..route },
    }
}

/// Waypoints of a loop at the top right corner, for edges from a node to itself.
pub fn self_loop(r: Rect) -> Vec<Point> {
    vec![
        Point::new(r.right() + 30.0, r.y + 20.0),
        Point::new(r.right() + 30.0, r.y - 30.0),
        Point::new(r.right() - 20.0, r.y - 30.0),
    ]
}

/// Size of the diagram font. Astah draws all class diagram texts in 12 px "Dialog" (Arial).
pub const FONT_SIZE: f32 = 12.0;
/// Line height relative to the font size; 15.09375 for 12 px, as Java lays out "Dialog".
pub const LINE_SPACING: f32 = 1.2578125;
/// Height of an edge label line in logical units.
pub const LABEL_HEIGHT: f32 = FONT_SIZE * LINE_SPACING;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelAlign {
    Left,
    Center,
    Right,
}

/// A text placed next to an edge. `anchor.y` is the top of the text; `anchor.x` is its left
/// edge, center or right edge depending on `align`.
#[derive(Clone, Debug, PartialEq)]
pub struct EdgeLabel {
    pub text: String,
    pub anchor: Point,
    pub align: LabelAlign,
    /// Static association ends show their role underlined.
    pub underline: bool,
    /// «include» or «extend», which can be dragged away from the line.
    pub keyword: bool,
    /// A text color of its own; unset, the theme's.
    pub color: Option<Color>,
}

/// An uncommitted change shown while the user drags.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub moved: HashSet<Id>,
    pub dx: f32,
    pub dy: f32,
    pub resized: Option<(Id, Rect)>,
    pub route: Option<(Id, Route)>,
    /// A free line with one end (0 start, 1 end) dragged to a point.
    pub line_end: Option<(Id, usize, Point)>,
}

impl Preview {
    pub fn rect(&self, presentation: Id, rect: Rect) -> Rect {
        match self.resized {
            Some((id, r)) if id == presentation => r,
            _ if self.moved.contains(&presentation) => rect.translate(self.dx, self.dy),
            _ => rect,
        }
    }
}

pub fn line_height(size: f32) -> f32 {
    size * LINE_SPACING
}

/// Stereotypes as Astah writes them: `<<entity, control>>`.
pub fn stereotype_text(names: &[String]) -> String {
    format!("<<{}>>", names.join(", "))
}

/// The visibility symbol a member's text starts with, as drawn, and the rest of the text from
/// the space on. Unlike Astah, UmlCraft can draw the symbol centered in the width of a `+`, so
/// all names of a compartment start in one column, and a minus as `−`, as wide as the plus
/// ([`TextMeasure::visibility_column`]).
pub fn visibility_column(text: &str) -> Option<(&'static str, &str)> {
    let rest = text.get(1..).filter(|rest| rest.starts_with(' '))?;
    let symbol = match &text[..1] {
        "+" => "+",
        "-" => "\u{2212}",
        "#" => "#",
        "~" => "~",
        _ => return None,
    };
    Some((symbol, rest))
}

/// Width of a row's text as drawn: a member's visibility symbol in its column, where the
/// column is on ([`visibility_column`]). `rounded` measures in whole pixels.
pub fn row_text_width(
    text: &str,
    role: RowRole,
    size: f32,
    bold: bool,
    measure: &dyn TextMeasure,
    rounded: bool,
) -> f32 {
    let width = |text: &str| {
        if rounded {
            measure.rounded_width(text, size, bold)
        } else {
            measure.width(text, size, bold)
        }
    };
    match visibility_column(text).filter(|_| role == RowRole::Member && measure.visibility_column())
    {
        Some((_, rest)) => width("+") + width(rest),
        None => width(text),
    }
}

/// A row before it is positioned.
#[derive(Clone)]
struct Line {
    text: String,
    size: f32,
    bold: bool,
    italic: bool,
    underline: bool,
    role: RowRole,
    element: Option<Id>,
    color: Option<Color>,
}

/// `text` as one line: line breaks and tabs become spaces, other control characters are
/// dropped. Rows and labels are drawn as single lines; texts that may have several (notes, free
/// texts) are split before.
pub fn single_line(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_owned();
    }
    let text = text.replace("\r\n", " ");
    text.chars()
        .filter_map(|c| match c {
            '\n' | '\r' | '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

impl Line {
    fn new(text: impl Into<String>, size: f32, role: RowRole, element: Option<Id>) -> Self {
        Self {
            text: single_line(&text.into()),
            size,
            bold: false,
            italic: false,
            underline: false,
            role,
            element,
            color: None,
        }
    }

    fn width(&self, measure: &dyn TextMeasure) -> f32 {
        self.width_of(&self.text, measure, false)
    }

    /// Width of `text` in this line's font, a member's visibility in its column.
    fn width_of(&self, text: &str, measure: &dyn TextMeasure, rounded: bool) -> f32 {
        row_text_width(text, self.role, self.size, self.bold, measure, rounded)
    }

    /// Width of `text` as Astah measures it to size and break boxes: as plain text, without
    /// UmlCraft's column for the visibility, the larger of Java's whole-pixel width and the
    /// exact one.
    fn astah_width_of(&self, text: &str, measure: &dyn TextMeasure) -> f32 {
        measure.astah_width(text, self.size, self.bold)
    }

    /// Pieces the text may break between. A leading visibility symbol stays with the next word.
    fn words(&self) -> Vec<String> {
        let mut words: Vec<String> = self.text.split(' ').map(str::to_owned).collect();
        if words.len() > 1 && matches!(words[0].as_str(), "+" | "-" | "#" | "~") {
            let first = words.remove(0);
            words[0] = format!("{first} {}", words[0]);
        }
        words
    }

    /// Width of the longest piece that cannot break.
    fn widest_word(&self, measure: &dyn TextMeasure) -> f32 {
        self.words().iter().map(|w| self.width_of(w, measure, false)).fold(0.0, f32::max)
    }

    /// This line, broken at spaces into lines no wider than `max`.
    fn wrapped(self, max: f32, measure: &dyn TextMeasure) -> Vec<Line> {
        if self.width(measure) <= max {
            return vec![self];
        }
        let mut texts = Vec::new();
        let mut current = String::new();
        // The line's width so far, as the sum of its words and spaces: measuring each growing
        // line exactly would shape it again for every word. Only a sum close to `max`, where
        // kerning could tip it, is checked exactly.
        let space = measure.width(" ", self.size, self.bold);
        let mut estimate = 0.0;
        for word in self.words() {
            let word_width = measure.width(&word, self.size, self.bold);
            if current.is_empty() {
                current = word;
                estimate = word_width;
                continue;
            }
            let guess = estimate + space + word_width;
            let candidate = format!("{current} {word}");
            // Break where Astah breaks: by the exact width of the plain text, not by the
            // visibility column's nor Java's whole-pixel width.
            let too_wide = if guess > max * 1.1 {
                true
            } else if guess < max * 0.9 {
                false
            } else {
                measure.width(&candidate, self.size, self.bold) > max
            };
            if too_wide {
                texts.push(std::mem::replace(&mut current, word));
                estimate = word_width;
            } else {
                current = candidate;
                estimate = guess;
            }
        }
        texts.push(current);
        texts.into_iter().map(|text| Line { text, ..self.clone_style() }).collect()
    }

    fn clone_style(&self) -> Line {
        Line { text: String::new(), ..*self }
    }

    fn place(self, rect: Rect, align: LabelAlign) -> Row {
        Row {
            text: self.text,
            rect,
            size: self.size,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            align,
            role: self.role,
            element: self.element,
            hinted: false,
            color: self.color,
        }
    }
}

/// Builds the scene of `diagram`, with `preview` applied.
pub fn build(model: &Model, diagram: Id, preview: &Preview, measure: &dyn TextMeasure) -> Scene {
    let Some(view) = model.view(diagram) else { return Scene::empty() };
    let mut nodes: Vec<NodeShape> = view
        .presentations()
        .iter()
        .filter_map(|p| {
            let bounds = preview.rect(p.id, p.bounds()?);
            Some(layout_node(model, p, bounds, measure))
        })
        .collect();
    let is_sequence =
        model.get(diagram).and_then(|d| d.kind().as_diagram()) == Some(DiagramKind::Sequence);
    let (sequence, messages) = if is_sequence {
        let (layout, messages) = sequence::arrange(model, diagram, view, &mut nodes, measure);
        if matches!(model.get(diagram).map(|d| d.kind()), Some(ElementKind::Diagram(p)) if p.hide_frame)
        {
            nodes.retain(|n| n.element != Some(diagram));
        }
        (Some(layout), messages)
    } else {
        package_frame(model, diagram, &mut nodes, measure);
        (None, Vec::new())
    };
    let mut rects: HashMap<Id, Rect> = nodes.iter().map(|n| (n.id, n.rect)).collect();
    // Anchors are relative to `rects`; lines are clipped at these.
    let clips: HashMap<Id, Rect> = nodes.iter().map(|n| (n.id, n.clip_rect())).collect();
    let mut outlines: HashMap<Id, Outline> =
        nodes.iter().map(|n| (n.id, n.figure.outline())).collect();
    let is_message = |p: &Presentation| {
        p.element
            .and_then(|e| model.get(e))
            .is_some_and(|e| matches!(e.kind(), ElementKind::Message(_)))
    };

    let iconic_interfaces: HashSet<Id> = view
        .presentations()
        .iter()
        .filter(|p| p.style.iconic && is_interface(model, p.element))
        .map(|p| p.id)
        .collect();
    let candidates: Vec<&Presentation> = view
        .presentations()
        .iter()
        // On sequence diagrams the layout draws the messages.
        .filter(|p| !(is_sequence && is_message(p)))
        .collect();
    let edge_shape =
        |p: &Presentation, rects: &HashMap<Id, Rect>, outlines: &HashMap<Id, Outline>| {
            let Shape::Edge { source, target, waypoints, source_anchor, target_anchor, labels } =
                &p.shape
            else {
                return None;
            };
            let (source_rect, target_rect) = (*rects.get(source)?, *rects.get(target)?);
            let (mut points, anchors) = match &preview.route {
                Some((id, r)) if *id == p.id => {
                    (r.waypoints.clone(), [r.source_anchor, r.target_anchor])
                }
                _ => (waypoints.clone(), [*source_anchor, *target_anchor]),
            };
            if preview.moved.contains(source) && preview.moved.contains(target) {
                points.iter_mut().for_each(|w| *w = w.offset(preview.dx, preview.dy));
            }
            if source == target && points.is_empty() {
                points = self_loop(source_rect);
            }
            let element = p.element.and_then(|e| model.get(e));
            // A bound class's «bind» stands above the line like «include», what it
            // substitutes below it in place of a name.
            let binding = element.and_then(|e| match e.kind() {
                ElementKind::Dependency(d) if e.data().stereotypes.iter().any(|s| s == "bind") => {
                    crate::member::binding_text(model, d.client, d.supplier)
                }
                _ => None,
            });
            let keyword = binding.is_some().then(|| "<<bind>>".to_owned());
            let label = element.and_then(|e| {
                let stereotypes: Vec<&str> = e
                    .data()
                    .stereotypes
                    .iter()
                    .map(String::as_str)
                    .filter(|s| binding.is_none() || *s != "bind")
                    .collect();
                let stereotypes = stereotypes.join(", ");
                let parts: Vec<String> = [
                    (!stereotypes.is_empty()).then(|| format!("<<{stereotypes}>>")),
                    (!e.name().is_empty()).then(|| e.name().to_owned()),
                    binding.clone(),
                ]
                .into_iter()
                .flatten()
                .collect();
                (!parts.is_empty()).then(|| parts.join(" "))
            });
            let to_lollipop = iconic_interfaces.contains(target);
            let outline = |id| outlines.get(id).copied().unwrap_or(Outline::Rect);
            let clip = |id, rect| clips.get(id).copied().unwrap_or(rect);
            let qualifiers = match element.map(|e| e.kind()) {
                Some(ElementKind::Association(a)) => a
                    .ends
                    .each_ref()
                    .map(|end| end.qualifiers.iter().map(|q| qualifier_text(model, q)).collect()),
                _ => Default::default(),
            };
            Some(edge_for(
                p.id,
                element.map(|e| e.kind()),
                &qualifiers,
                (
                    (source_rect, clip(source, source_rect), outline(source)),
                    (target_rect, clip(target, target_rect), outline(target)),
                ),
                anchors,
                p.style.line_shape,
                &points,
                (label, element.is_some_and(|e| !e.name().is_empty()), keyword),
                to_lollipop,
                labels,
                &p.style,
                measure,
            ))
        };
    let mut edges: Vec<EdgeShape> =
        candidates.iter().filter_map(|p| edge_shape(p, &rects, &outlines)).collect();
    // Lines that end on another line (notes anchored to a relationship) end in the middle of
    // that line, as in Astah.
    let built: HashSet<Id> = edges.iter().map(|e| e.id).collect();
    for e in &edges {
        let (a, b) = route::middle_segment(&e.points, e.shape.is_curve());
        let c = middle(a, b);
        rects.insert(e.id, Rect::new(c.x, c.y, 0.0, 0.0));
        outlines.insert(e.id, Outline::Rect);
    }
    edges.extend(
        candidates
            .iter()
            .filter(|p| !built.contains(&p.id))
            .filter_map(|p| edge_shape(p, &rects, &outlines)),
    );
    keywords_away_from_notes(&candidates, &built, &mut edges, measure);
    edges.extend(view.presentations().iter().filter_map(|p| free_line(p, preview)));
    edges.extend(messages);
    for label in edges.iter_mut().flat_map(|e| &mut e.labels) {
        label.text = single_line(&label.text);
    }
    labels::place(&nodes, &mut edges, measure);
    Scene { nodes, edges, sequence }
}

/// A free line, moved along while it is dragged.
fn free_line(p: &Presentation, preview: &Preview) -> Option<EdgeShape> {
    let Shape::Line { from, to, dashed, start_arrow, end_arrow } = p.shape else { return None };
    let (dx, dy) =
        if preview.moved.contains(&p.id) { (preview.dx, preview.dy) } else { (0.0, 0.0) };
    let (mut from, mut to) = (from.offset(dx, dy), to.offset(dx, dy));
    match preview.line_end {
        Some((id, 0, at)) if id == p.id => from = at,
        Some((id, _, at)) if id == p.id => to = at,
        _ => {}
    }
    let marker = |on: bool| if on { EndMarker::OpenArrow } else { EndMarker::None };
    Some(EdgeShape {
        id: p.id,
        points: vec![from, to],
        line: if dashed { LineStyle::ShortDashed } else { LineStyle::Solid },
        shape: LineShape::Line,
        source_marker: marker(start_arrow),
        target_marker: marker(end_arrow),
        labels: Vec::new(),
        fixed: true,
        ends: [Rect::new(from.x, from.y, 0.0, 0.0), Rect::new(to.x, to.y, 0.0, 0.0)],
        anchors: [None, None],
        name_arrow: None,
        qualifiers: Vec::new(),
        node_tips: [from, to],
        free_labels: Vec::new(),
        association: false,
        end_labels: Vec::new(),
        stroke: None,
    })
}

/// A note anchored to the middle of an include or extend comes from one side of its line; the
/// «include» or «extend» goes to the other, unless it was placed by hand.
fn keywords_away_from_notes(
    candidates: &[&Presentation],
    lines: &HashSet<Id>,
    edges: &mut [EdgeShape],
    measure: &dyn TextMeasure,
) {
    let placed = |id: Id| {
        candidates.iter().any(|p| {
            p.id == id && matches!(&p.shape, Shape::Edge { labels, .. } if labels.keyword.is_some())
        })
    };
    let index: HashMap<Id, usize> = edges.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    // Each anchored line with the point the note's line comes from.
    let notes: Vec<(Id, Point)> = candidates
        .iter()
        .filter_map(|p| {
            let Shape::Edge { source, target, .. } = &p.shape else { return None };
            let line = if lines.contains(target) {
                *target
            } else if lines.contains(source) {
                *source
            } else {
                return None;
            };
            let pts = &edges[*index.get(&p.id)?].points;
            Some((line, if line == *target { pts[pts.len() - 2] } else { pts[1] }))
        })
        .filter(|(line, _)| !placed(*line))
        .collect();
    for (line, from) in notes {
        let Some(&i) = index.get(&line) else { continue };
        let edge = &mut edges[i];
        let (a, b) = route::middle_segment(&edge.points, edge.shape.is_curve());
        let center = middle(a, b);
        let Some(label) = edge.labels.iter_mut().find(|l| l.keyword) else { continue };
        let (half_w, half_h) = (label_width(&label.text, measure) / 2.0, LABEL_HEIGHT / 2.0);
        let mid = label.anchor.offset(half_w, half_h);
        let (lx, ly) = (mid.x - center.x, mid.y - center.y);
        if lx * (from.x - center.x) + ly * (from.y - center.y) > 0.0 {
            label.anchor = center.offset(-lx - half_w, -ly - half_h);
        }
    }
}

/// The frame Astah may draw around a class or use case diagram (a node presentation of the
/// diagram itself), reaching 15 beyond the other nodes. Its tab holds "pkg" in bold and the
/// name of the diagram's package, on use case diagrams "uc" and the diagram's own name.
fn package_frame(model: &Model, diagram: Id, nodes: &mut [NodeShape], measure: &dyn TextMeasure) {
    const MARGIN: f32 = 15.0;
    let Some(index) = nodes.iter().position(|n| n.element == Some(diagram)) else { return };
    let content = nodes
        .iter()
        .filter(|n| n.element != Some(diagram))
        .map(|n| n.rect)
        .reduce(|a, b| a.union(&b));
    let frame = &mut nodes[index];
    let mut r = frame.rect;
    if let Some(c) = content {
        r.width = r.width.max(c.right() + MARGIN - r.x);
        r.height = r.height.max(c.bottom() + MARGIN - r.y);
    }
    let d = model.get(diagram);
    let own_name = || d.map(|d| d.name()).unwrap_or_default();
    let owner_name = || {
        d.and_then(|d| d.owner()).and_then(|o| model.get(o)).map(|o| o.name()).unwrap_or_default()
    };
    // Astah's keywords and where the name after them starts, measured.
    let (keyword, name_x, name) = match d.and_then(|d| d.kind().as_diagram()) {
        Some(DiagramKind::UseCase) => ("uc", 26.0, own_name()),
        Some(DiagramKind::Activity) => ("act", 29.34, own_name()),
        Some(DiagramKind::StateMachine) => ("stm", 34.0, own_name()),
        Some(DiagramKind::Component) => ("cmp", 37.0, owner_name()),
        _ => ("pkg", 33.0, owner_name()),
    };
    frame.rect = r;
    (frame.figure, frame.rows) =
        sequence::frame_with_tab(keyword, name_x, name, r, diagram, measure);
}

fn is_interface(model: &Model, element: Option<Id>) -> bool {
    element
        .and_then(|e| model.get(e))
        .is_some_and(|e| matches!(e.kind(), ElementKind::Interface(_)))
}

fn layout_node(
    model: &Model,
    p: &Presentation,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> NodeShape {
    let element = p.element.and_then(|e| model.get(e));
    let (rect, figure, rows) = match (element, &p.shape) {
        (_, Shape::Text { text, .. }) => layout_text(text, &p.style, bounds, measure),
        (Some(e), _) => layout_element(model, e, &p.style, p.regions(), bounds, measure),
        (None, _) => (bounds, Figure::Other, Vec::new()),
    };
    NodeShape {
        id: p.id,
        element: p.element,
        rect,
        figure,
        rows: colored(rows, &p.style),
        fill: p.style.fill,
    }
}

/// The rows with the name in the presentation's font color, as Astah colors only the name.
pub(crate) fn colored(mut rows: Vec<Row>, style: &Style) -> Vec<Row> {
    for row in rows.iter_mut().filter(|r| r.role == RowRole::Name) {
        row.color = style.font_color;
    }
    rows
}

/// A free text as Astah draws it: no outline, each line 10 right of the box's left edge, the
/// first baseline 17 below its top. The box grows to hold the text.
fn layout_text(
    text: &str,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const FIRST_BASELINE: f32 = 17.0;
    let size = style.font_size.unwrap_or(FONT_SIZE);
    let height = line_height(size);
    let top = bounds.y + FIRST_BASELINE - crate::draw::baseline(size);
    let rows: Vec<Row> = text
        .split('\n')
        .enumerate()
        .map(|(i, text)| {
            let line = Line::new(text, size, RowRole::Name, None);
            let w = line.width(measure);
            let y = top + i as f32 * height;
            line.place(Rect::new(bounds.x + 10.0, y, w, height), LabelAlign::Left)
        })
        .collect();
    let width = rows.iter().map(|r| r.rect.width + 20.0).fold(bounds.width, f32::max);
    let bottom = rows.last().map_or(0.0, |r| r.rect.bottom() - bounds.y);
    let rect = Rect::new(bounds.x, bounds.y, width, bounds.height.max(bottom));
    (rect, Figure::Text, rows)
}

fn layout_element(
    model: &Model,
    e: &Element,
    style: &Style,
    regions: Option<&Regions>,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    let id = e.id();
    let name_line = |size: f32, bold: bool| {
        let mut line = Line::new(e.name(), size, RowRole::Name, Some(id));
        line.bold = bold;
        line
    };
    match e.kind() {
        ElementKind::Interface(_) if style.iconic => {
            // Astah: a circle of radius 10 in a 24 × 24 box, the bold name right below it.
            let r = Rect::new(bounds.x, bounds.y, 24.0, 24.0);
            let line = name_line(12.0, true);
            let w = line.width(measure) + 8.0;
            let top = r.center().y + LOLLIPOP_RADIUS;
            let row = line.place(
                Rect::new(r.center().x - w / 2.0, top, w, line_height(12.0)),
                LabelAlign::Center,
            );
            (r, Figure::Lollipop, vec![row])
        }
        kind if is_boxed_classifier(kind) => {
            layout_classifier(model, e, style, bounds, false, measure)
        }
        ElementKind::Package | ElementKind::Model => {
            // Astah: the bold name centered 5 below a tab 15 high.
            let line = name_line(FONT_SIZE, true);
            let w = bounds.width.max(line.width(measure) + 20.0);
            let r = Rect::new(bounds.x, bounds.y, w, bounds.height.max(40.0));
            let tab = Rect::new(r.x, r.y, (r.width * 0.4).min(80.0), 15.0);
            let row = line.place(
                Rect::new(r.x, r.y + 20.0, r.width, line_height(FONT_SIZE)),
                LabelAlign::Center,
            );
            let subsystem = crate::palette::is_subsystem(e);
            (r, Figure::Package { tab, subsystem }, vec![row])
        }
        ElementKind::Actor(_) => {
            // Astah: a stick figure of fixed size in the middle of the box, the bold name
            // centered below it, its baseline 13 below the feet.
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(ACTOR_FIGURE.width),
                bounds.height.max(ACTOR_FIGURE.height),
            );
            let feet = r.center().y + ACTOR_FIGURE.height / 2.0;
            let line = name_line(FONT_SIZE, true);
            let w = line.width(measure).max(r.width) + 8.0;
            let top = feet + 13.0 - crate::draw::baseline(FONT_SIZE);
            let row = line.place(
                Rect::new(r.center().x - w / 2.0, top, w, line_height(FONT_SIZE)),
                LabelAlign::Center,
            );
            (r, Figure::Actor, vec![row])
        }
        ElementKind::UseCase(_) => layout_use_case(model, e, style, bounds, measure),
        // Astah has no system boundary of its own; it is drawn as a rectangle with a text: the
        // name in regular type, centered, its baseline 24 below the top.
        ElementKind::SystemBoundary => {
            let line = name_line(FONT_SIZE, false);
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(line.width(measure) + 20.0).max(120.0),
                bounds.height.max(80.0),
            );
            let row = match style.name_offset {
                // Astah's free text: 10 right of its box, the baseline 17 below the box's top.
                Some(at) => {
                    let top = r.y + at.y + 17.0 - crate::draw::baseline(FONT_SIZE);
                    let w = line.width(measure);
                    line.place(
                        Rect::new(r.x + at.x + 10.0, top, w, line_height(FONT_SIZE)),
                        LabelAlign::Left,
                    )
                }
                None => {
                    let top = r.y + 24.0 - crate::draw::baseline(FONT_SIZE);
                    line.place(
                        Rect::new(r.x, top, r.width, line_height(FONT_SIZE)),
                        LabelAlign::Center,
                    )
                }
            };
            (r, Figure::Boundary, vec![row])
        }
        // The sequence layout places lifelines.
        ElementKind::Lifeline(_) => (bounds, Figure::Other, Vec::new()),
        ElementKind::ActivityNode(p) => layout_activity_node(model, e, p, bounds, measure),
        ElementKind::State(p) => layout_state(model, e, p, regions, bounds, measure),
        ElementKind::Component(_) | ElementKind::Artifact(_) | ElementKind::Node(_) => {
            layout_box(model, e, e.kind(), None, style, bounds, measure)
        }
        ElementKind::Instance(p) if p.instance == InstanceKind::Object => {
            layout_object(model, e, p.classifier, style, bounds, measure)
        }
        ElementKind::Instance(p) => {
            let figure = match p.instance {
                InstanceKind::Object => ElementKind::Class(Default::default()),
                InstanceKind::Component => ElementKind::Component(Default::default()),
                InstanceKind::Node => ElementKind::Node(Default::default()),
            };
            layout_box(model, e, &figure, Some(p.classifier), style, bounds, measure)
        }
        ElementKind::FinalState => (bounds, Figure::ActivityFinal, Vec::new()),
        ElementKind::Pseudostate(p) => {
            let figure = match p.pseudostate {
                PseudostateKind::Initial | PseudostateKind::Junction => Figure::Initial,
                PseudostateKind::Choice => Figure::Diamond,
                PseudostateKind::Fork | PseudostateKind::Join => Figure::Bar,
                PseudostateKind::ShallowHistory | PseudostateKind::DeepHistory => Figure::Circle,
            };
            let letter = match p.pseudostate {
                PseudostateKind::ShallowHistory => "H",
                PseudostateKind::DeepHistory => "H*",
                _ => return (bounds, figure, Vec::new()),
            };
            let h = line_height(FONT_SIZE);
            let row = Line::new(letter, FONT_SIZE, RowRole::Stereotype, Some(id)).place(
                Rect::new(bounds.x, bounds.center().y - h / 2.0, bounds.width, h),
                LabelAlign::Center,
            );
            (bounds, figure, vec![row])
        }
        ElementKind::Partition => {
            let line = name_line(FONT_SIZE, false);
            let h = line_height(FONT_SIZE);
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(line.width(measure) + 20.0).max(40.0),
                bounds.height.max(PARTITION_HEADER),
            );
            let row = line.place(
                Rect::new(r.x, r.y + (PARTITION_HEADER - h) / 2.0, r.width, h),
                LabelAlign::Center,
            );
            (r, Figure::Partition { header: r.y + PARTITION_HEADER }, vec![row])
        }
        ElementKind::Comment(p) => {
            let lines: Vec<Line> = p
                .body
                .split('\n')
                .map(|l| Line::new(l.trim_end_matches('\r'), 12.0, RowRole::Name, Some(id)))
                .collect();
            // Astah starts the text 2 from the top left corner and wraps it at the right edge;
            // the note only grows wider for a word that does not fit.
            let h = line_height(12.0);
            let word_w = lines.iter().map(|l| l.widest_word(measure)).fold(0.0, f32::max);
            let width = bounds.width.max(word_w + 4.0);
            let lines: Vec<Line> =
                lines.into_iter().flat_map(|l| l.wrapped(width - 4.0, measure)).collect();
            let r = Rect::new(
                bounds.x,
                bounds.y,
                width,
                bounds.height.max(4.0 + h * lines.len() as f32),
            );
            let rows = lines
                .into_iter()
                .enumerate()
                .map(|(i, l)| {
                    l.place(
                        Rect::new(r.x + 2.0, r.y + 2.0 + h * i as f32, r.width - 4.0, h),
                        LabelAlign::Left,
                    )
                })
                .collect();
            (r, Figure::Note, rows)
        }
        _ => {
            let line = name_line(12.0, false);
            let row = line.place(
                Rect::new(bounds.x, bounds.y + 6.0, bounds.width, line_height(12.0)),
                LabelAlign::Center,
            );
            (bounds, Figure::Other, vec![row])
        }
    }
}

/// Height of a swimlane's name block; Astah's is 35 px.
pub const PARTITION_HEADER: f32 = 35.0;

/// Activity nodes: control nodes keep their size and show no name; actions and object nodes
/// grow around their wrapped name.
fn layout_activity_node(
    model: &Model,
    e: &Element,
    props: &ActivityNodeProps,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD_X: f32 = 10.0;
    const PAD_Y: f32 = 4.0;
    let id = e.id();
    let figure = match props.node {
        ActivityNodeKind::Action => Figure::Action,
        ActivityNodeKind::Object => Figure::Rectangle,
        ActivityNodeKind::Initial => Figure::Initial,
        ActivityNodeKind::ActivityFinal => Figure::ActivityFinal,
        ActivityNodeKind::FlowFinal => Figure::FlowFinal,
        ActivityNodeKind::Decision | ActivityNodeKind::Merge => Figure::Diamond,
        ActivityNodeKind::Fork | ActivityNodeKind::Join => Figure::Bar,
    };
    if !matches!(figure, Figure::Action | Figure::Rectangle) {
        return (bounds, figure, Vec::new());
    }
    let text = match props.type_ref.as_ref() {
        Some(t) if props.node == ActivityNodeKind::Object => {
            let type_name = crate::member::type_label(model, &Some(t.clone()));
            match type_name.filter(|t| !t.is_empty()) {
                Some(t) if e.name().is_empty() => format!(": {t}"),
                Some(t) => format!("{} : {t}", e.name()),
                None => e.name().to_owned(),
            }
        }
        _ => e.name().to_owned(),
    };
    let width = bounds.width.max(40.0);
    let names = wrap(&text, width - 2.0 * PAD_X, FONT_SIZE, measure);
    // Object nodes in a state show it as `[state]` in a line of its own below the name; the
    // line belongs to no element, so editing the name inline leaves the state alone.
    let state = (props.node == ActivityNodeKind::Object && !props.in_state.is_empty())
        .then(|| format!("[{}]", props.in_state));
    let h = line_height(FONT_SIZE);
    let widest =
        names.iter().chain(&state).map(|n| measure.width(n, FONT_SIZE, false)).fold(0.0, f32::max);
    let block = h * (names.len() + state.iter().len()) as f32;
    let r = Rect::new(
        bounds.x,
        bounds.y,
        width.max(widest + 2.0 * PAD_X),
        bounds.height.max(block + 2.0 * PAD_Y),
    );
    let top = r.center().y - block / 2.0;
    let lines = names
        .into_iter()
        .map(|n| (n, RowRole::Name, Some(id)))
        .chain(state.map(|s| (s, RowRole::Member, None)));
    let rows = lines
        .enumerate()
        .map(|(i, (n, role, element))| {
            Line::new(n, FONT_SIZE, role, element)
                .place(Rect::new(r.x, top + h * i as f32, r.width, h), LabelAlign::Center)
        })
        .collect();
    (r, figure, rows)
}

/// Depth of a deployment node's cube.
const CUBE_DEPTH: f32 = 4.0;
/// Astah's boxes (classes, objects, components, nodes): at least 10 beside the widest text, 4
/// above and below the content of each compartment.
const BOX_PAD_X: f32 = 10.0;
const BOX_PAD_Y: f32 = 4.0;
/// The smallest box Astah draws for these.
const BOX_MIN_WIDTH: f32 = 40.0;
const BOX_MIN_HEIGHT: f32 = 30.0;

/// The stereotype rows of a box's header: one per non-empty set, such as an element's own and
/// then its classifier's.
fn stereotype_lines<'a>(
    sets: impl IntoIterator<Item = &'a Vec<String>>,
    size: f32,
    id: Id,
) -> Vec<Line> {
    sets.into_iter()
        .filter(|s| !s.is_empty())
        .map(|s| Line::new(stereotype_text(s), size, RowRole::Stereotype, Some(id)))
        .collect()
}

/// Components, artifacts, nodes and instances as Astah draws them: a box (or cube) with
/// stereotypes and the name at the top. `instance_of` is set for instances, which show
/// `name : Type` underlined and their classifier's stereotypes below their own; `figure` then is
/// the kind of element they are an instance of.
///
/// Astah's rows start 4 below the top of a box or of a cube's front face.
/// Component and artifact names are regular; texts are centered left of the icon, which takes
/// 15 off the right of a component and 11 off an artifact.
fn layout_box(
    model: &Model,
    e: &Element,
    figure: &ElementKind,
    instance_of: Option<Option<Id>>,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    let id = e.id();
    let classifier = instance_of.flatten().and_then(|c| model.get(c));
    let own = (!style.hide_stereotypes).then_some(&e.data().stereotypes);
    let base = classifier.map(|c| &c.data().stereotypes).filter(|_| !style.hide_stereotypes);
    let mut lines = stereotype_lines([own, base].into_iter().flatten(), FONT_SIZE, id);
    let mut name = match instance_of {
        Some(classifier) => {
            let (text, _) = instance_text(model, e, classifier, style);
            let mut line = Line::new(text, FONT_SIZE, RowRole::Name, Some(id));
            line.underline = true;
            line
        }
        None => {
            let mut line = Line::new(e.name(), FONT_SIZE, RowRole::Name, Some(id));
            line.bold = matches!(figure, ElementKind::Node(_));
            line
        }
    };
    name.italic = matches!(figure, ElementKind::Component(p) if p.is_abstract);
    lines.push(name);

    let (depth, icon_room) = match figure {
        ElementKind::Node(_) => (CUBE_DEPTH, 0.0),
        ElementKind::Component(_) => (0.0, 15.0),
        ElementKind::Artifact(_) => (0.0, 11.0),
        _ => (0.0, 0.0),
    };
    let widest = lines.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
    let h = line_height(FONT_SIZE);
    let text_h = h * lines.len() as f32;
    // The bounds are a cube's front face; its top and side stick out above and to the right
    // (see `draw`).
    let r = Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(widest + 2.0 * BOX_PAD_X + icon_room).max(BOX_MIN_WIDTH),
        bounds.height.max(text_h + 2.0 * BOX_PAD_Y).max(BOX_MIN_HEIGHT),
    );
    let front = r;
    let rows = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let y = front.y + BOX_PAD_Y + h * i as f32;
            line.place(Rect::new(front.x, y, front.width - icon_room, h), LabelAlign::Center)
        })
        .collect();
    let figure = match figure {
        // The icon 6 below the top: a component's 12 × 14 with its tabs 2 further left, an
        // artifact's 10 × 14.
        ElementKind::Component(_) => {
            Figure::Component { icon: Rect::new(front.right() - 20.0, front.y + 6.0, 14.0, 14.0) }
        }
        ElementKind::Artifact(_) => {
            Figure::Artifact { icon: Rect::new(front.right() - 16.0, front.y + 6.0, 10.0, 14.0) }
        }
        ElementKind::Node(_) => Figure::Cube { depth },
        _ => Figure::Rectangle,
    };
    (r, figure, rows)
}

/// What an instance shows: `name : Type`, `: Type` without a name, less where `style` hides
/// the name or the type. Also whether it starts with the colon.
fn instance_text(
    model: &Model,
    e: &Element,
    classifier: Option<Id>,
    style: &Style,
) -> (String, bool) {
    let name = if style.hide_instance_name { "" } else { e.name() };
    let type_name = classifier
        .and_then(|c| model.get(c))
        .map(|c| c.name())
        .filter(|_| !style.hide_instance_type);
    match type_name {
        Some(t) if name.is_empty() => (format!(": {t}"), true),
        Some(t) => (format!("{name} : {t}"), false),
        None => (name.to_owned(), false),
    }
}

/// An object as Astah draws it: a box like a class's, with `name : Class` underlined in the
/// header and one compartment of slots (`total = 42`) below, 8 px high when empty, unless the
/// style hides it.
fn layout_object(
    model: &Model,
    e: &Element,
    classifier: Option<Id>,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD_X: f32 = BOX_PAD_X;
    const PAD_Y: f32 = BOX_PAD_Y;
    let id = e.id();
    // Its own stereotypes, then its class's, as Astah shows them.
    let own = (!style.hide_stereotypes).then_some(&e.data().stereotypes);
    let base = classifier
        .and_then(|c| model.get(c))
        .map(|c| &c.data().stereotypes)
        .filter(|_| !style.hide_stereotypes);
    let mut header = stereotype_lines([own, base].into_iter().flatten(), FONT_SIZE, id);
    let (text, colon_first) = instance_text(model, e, classifier, style);
    let mut name = Line::new(text, FONT_SIZE, RowRole::Name, Some(id));
    name.underline = true;
    // Without a name Astah centers " : Class" but draws it from the colon on: the text sits
    // as far left as the missing space is wide.
    let shift = if colon_first {
        let astah = |l: &Line, text: &str| {
            l.width_of(text, measure, false).max(l.width_of(text, measure, true))
        };
        astah(&name, &format!(" {}", name.text)) - astah(&name, &name.text)
    } else {
        0.0
    };
    header.push(name);
    let slots: Vec<Line> = crate::object::slots(model, id)
        .into_iter()
        .filter(|(_, value)| {
            !style.hide_attributes && !(style.hide_empty_slots && value.is_empty())
        })
        .map(|(attribute, value)| {
            let text = crate::object::slot_text(model, attribute, &value);
            Line::new(text, FONT_SIZE, RowRole::Slot, Some(attribute))
        })
        .collect();

    let block =
        |lines: &[Line]| 2.0 * PAD_Y + lines.iter().map(|l| line_height(l.size)).sum::<f32>();
    let slots_h = if style.hide_attributes { 0.0 } else { block(&slots) };
    let content_h = block(&header) + slots_h;
    // Sized automatically, Astah makes the box as wide as its widest text as it measures it
    // (Java's whole-pixel width or the exact one, whichever is larger), around its center.
    let r = if style.auto_size {
        let widest = header
            .iter()
            .chain(&slots)
            .map(|l| l.width(measure).max(l.width_of(&l.text, measure, true)))
            .fold(0.0, f32::max);
        let width = (widest + 2.0 * PAD_X).max(BOX_MIN_WIDTH);
        Rect::new(bounds.center().x - width / 2.0, bounds.y, width, bounds.height.max(content_h))
    } else {
        let widest = header.iter().chain(&slots).map(|l| l.width(measure)).fold(0.0, f32::max);
        Rect::new(
            bounds.x,
            bounds.y,
            bounds.width.max(widest + 2.0 * PAD_X).max(BOX_MIN_WIDTH),
            bounds.height.max(content_h),
        )
    };
    let mut rows = Vec::new();
    let mut y = r.y + PAD_Y;
    for line in header {
        let h = line_height(line.size);
        let width = if line.role == RowRole::Name { r.width - shift } else { r.width };
        rows.push(line.place(Rect::new(r.x, y, width, h), LabelAlign::Center));
        y += h;
    }
    y += PAD_Y;
    let (separators, compartments) = if style.hide_attributes {
        (Vec::new(), Vec::new())
    } else {
        (vec![y], vec![Compartment::Slots])
    };
    y += PAD_Y;
    for line in slots {
        let h = line_height(line.size);
        rows.push(
            line.place(Rect::new(r.x + PAD_X, y, r.width - 2.0 * PAD_X, h), LabelAlign::Left),
        );
        y += h;
    }
    (r, Figure::Classifier { separators, compartments, template: None }, rows)
}

/// States: the name, centered; below a line the internal activities (`entry / …`), or room for
/// the substates of a composite state.
fn layout_state(
    model: &Model,
    e: &Element,
    props: &StateProps,
    layout: Option<&Regions>,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD_X: f32 = 8.0;
    const PAD_Y: f32 = 4.0;
    let id = e.id();
    let kind_of = |c: Id| model.get(c).map(|c| c.kind());
    let regions: Vec<Id> = e
        .children()
        .iter()
        .copied()
        .filter(|&c| matches!(kind_of(c), Some(ElementKind::Region)))
        .collect();
    // Entry, do and exit, then the internal transitions, each of which edits its own element.
    let mut activities: Vec<Line> =
        [("entry", &props.entry), ("do", &props.do_activity), ("exit", &props.exit)]
            .into_iter()
            .filter(|(_, text)| !text.trim().is_empty())
            .map(|(label, text)| {
                Line::new(
                    format!("{label} / {}", text.trim()),
                    FONT_SIZE,
                    RowRole::Stereotype,
                    Some(id),
                )
            })
            .collect();
    activities.extend(e.children().iter().filter_map(|&c| match kind_of(c)? {
        ElementKind::InternalTransition(t) => {
            Some(Line::new(t.label(), FONT_SIZE, RowRole::Member, Some(c)))
        }
        _ => None,
    }));
    let name = Line::new(e.name(), FONT_SIZE, RowRole::Name, Some(id));
    let h = line_height(FONT_SIZE);
    let widest = activities.iter().chain([&name]).map(|l| l.width(measure)).fold(0.0, f32::max);
    let header = h + 2.0 * PAD_Y;
    let body = if activities.is_empty() { 0.0 } else { h * activities.len() as f32 + 2.0 * PAD_Y };
    // Every region keeps at least `MIN_REGION` across the lines, however small the state is
    // stored.
    let across = if regions.len() >= 2 { MIN_REGION * regions.len() as f32 } else { 0.0 };
    let vertical = layout.is_some_and(|l| l.vertical);
    let (min_width, min_height) = if vertical { (across, 0.0) } else { (0.0, across) };
    let r = Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(widest + 2.0 * PAD_X).max(BOX_MIN_WIDTH).max(min_width),
        bounds.height.max(header + body + min_height).max(BOX_MIN_HEIGHT),
    );
    // As in Astah, the name heads every state, above a line.
    let mut rows = vec![name.place(Rect::new(r.x, r.y + PAD_Y, r.width, h), LabelAlign::Center)];
    let separator = Some(r.y + header);
    for (i, line) in activities.into_iter().enumerate() {
        let y = r.y + header + PAD_Y + h * i as f32;
        rows.push(
            line.place(Rect::new(r.x + PAD_X, y, r.width - 2.0 * PAD_X, h), LabelAlign::Left),
        );
    }
    let default_layout = Regions::default();
    let layout = layout.unwrap_or(&default_layout);
    let regions = region_shapes(r, r.y + header + body, layout, &regions);
    (r, Figure::State { separator, regions, vertical: layout.vertical }, rows)
}

/// The smallest extent of a region across its dividing lines.
pub const MIN_REGION: f32 = 20.0;

/// The areas of `regions` in a state drawn at `r` whose regions start below `top`. The lines
/// between them follow `layout`; lines it lacks divide the space after the last one evenly.
/// One region or none has no lines.
pub fn region_shapes(r: Rect, top: f32, layout: &Regions, regions: &[Id]) -> Vec<RegionShape> {
    if regions.len() < 2 {
        return Vec::new();
    }
    // Along the axis across the lines: where the regions may start and end, from the edge
    // the stored offsets are measured from.
    let top = top.min(r.bottom() - MIN_REGION).max(r.y);
    let (origin, start, end) = if layout.vertical {
        (r.x, r.x, r.right())
    } else {
        (r.y, top.min(r.bottom() - MIN_REGION * regions.len() as f32).max(r.y), r.bottom())
    };
    let mut lines = Vec::with_capacity(regions.len() - 1);
    let mut previous = start;
    for i in 0..regions.len() - 1 {
        let left = (regions.len() - 1 - i) as f32;
        let at = match layout.separators.get(i) {
            Some(&offset) => origin + offset,
            None => previous + (end - previous) / (left + 1.0),
        };
        let at = at.max(previous + MIN_REGION).min(end - MIN_REGION * left);
        lines.push(at);
        previous = at;
    }
    let mut bounds = vec![if layout.vertical { r.x } else { r.y }];
    bounds.extend(lines);
    bounds.push(end);
    regions
        .iter()
        .zip(bounds.windows(2))
        .map(|(&region, w)| RegionShape {
            region,
            rect: if layout.vertical {
                Rect::new(w[0], top, w[1] - w[0], r.bottom() - top)
            } else {
                Rect::new(r.x, w[0], r.width, w[1] - w[0])
            },
        })
        .collect()
}

/// The regions of the orthogonal state shown by node `p`, where its stored bounds put them.
/// Heights do not depend on the font, so an estimate of text widths will do.
pub fn state_regions(model: &Model, p: &Presentation) -> Vec<RegionShape> {
    let Some(bounds) = p.bounds() else { return Vec::new() };
    match layout_node(model, p, bounds, &ApproxMeasure).figure {
        Figure::State { regions, .. } => regions,
        _ => Vec::new(),
    }
}

/// The outline lines to node `p` end at. It does not depend on the font.
pub fn outline_of(model: &Model, p: &Presentation) -> Outline {
    let Some(bounds) = p.bounds() else { return Outline::Rect };
    layout_node(model, p, bounds, &ApproxMeasure).figure.outline()
}

/// Region shapes' dividing lines, as offsets from the state's top (or left) edge in the form
/// [`Regions::separators`] keeps them.
pub fn region_separators(r: Rect, regions: &[RegionShape], vertical: bool) -> Vec<f32> {
    regions.iter().skip(1).map(|s| if vertical { s.rect.x - r.x } else { s.rect.y - r.y }).collect()
}

/// Splits `text` into lines no wider than `max`; words longer than a line stay whole.
fn wrap(text: &str, max: f32, size: f32, measure: &dyn TextMeasure) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate =
            if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if !current.is_empty() && measure.width(&candidate, size, false) > max {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// Use case ellipse as Astah draws it: the bold name centered, and with extension points a line
/// below it, then "extension points" and the points, left-aligned on the rectangle inscribed in
/// the ellipse. The ellipse is at least as large as Astah makes it for its content.
///
/// Unlike Astah, which only breaks names where they contain line breaks and widens the ellipse
/// instead, the name also breaks at spaces to fit the ellipse's width less Astah's padding; the
/// ellipse widens only for a word that does not fit, and grows in height until the name lies
/// inside it. With `auto_size` (use cases from `.asta` files) it is as Astah draws it: the name
/// breaks only at its line breaks, and the ellipse widens around its center for the longest
/// line plus the padding.
fn layout_use_case(
    model: &Model,
    e: &Element,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    /// Width beyond the name.
    const NAME_PAD: f32 = 62.0;
    const MIN_SIZE: Size = Size { width: 90.0, height: 40.0 };
    /// From the top of the last name row to the line below it.
    const SEPARATOR: f32 = 17.09;
    /// From the line to the baseline of "extension points".
    const FIRST_BASELINE: f32 = 15.0;
    /// From the last baseline to the end of the text block.
    const DESCENT: f32 = 2.09;
    /// Above and below the text block when the ellipse has extension points.
    const MARGIN: f32 = 18.2214;
    /// Space kept between a wrapped name and the ellipse.
    const NAME_GAP: f32 = 4.0;
    let id = e.id();
    let extension_points: Vec<(Id, &str)> = e
        .children()
        .iter()
        .filter_map(|&c| model.get(c))
        .filter(|c| matches!(c.kind(), ElementKind::ExtensionPoint))
        .map(|c| (c.id(), c.name()))
        .collect();
    let width = |line: &Line| line.width(measure).max(line.width_of(&line.text, measure, true));

    let h = line_height(FONT_SIZE);
    let paragraphs: Vec<Line> = e
        .name()
        .split('\n')
        .map(|n| {
            let mut line = Line::new(n, FONT_SIZE, RowRole::Name, Some(id));
            line.bold = true;
            line
        })
        .collect();
    let points: Vec<Line> = if extension_points.is_empty() {
        Vec::new()
    } else {
        std::iter::once(Line::new("extension points", FONT_SIZE, RowRole::Stereotype, Some(id)))
            .chain(
                extension_points
                    .iter()
                    .map(|&(ep, name)| Line::new(name, FONT_SIZE, RowRole::Member, Some(ep))),
            )
            .collect()
    };
    let auto_size = style.auto_size;
    // As wide as stored, at least as wide as the longest word plus the padding and the points.
    let r_width = paragraphs
        .iter()
        .map(|l| if auto_size { width(l) } else { l.widest_word(measure) } + NAME_PAD)
        .chain(points.iter().map(width))
        .fold(MIN_SIZE.width.max(bounds.width), f32::max);
    let names: Vec<Line> =
        paragraphs.into_iter().flat_map(|l| l.wrapped(r_width - NAME_PAD, measure)).collect();
    // Lines that still do not fit (Astah measures by the larger of two widths) widen it too.
    let r_width = names.iter().map(|l| width(l) + NAME_PAD).fold(r_width, f32::max);
    let names_h = h * names.len() as f32;
    let block = if points.is_empty() {
        names_h
    } else {
        names_h - h + SEPARATOR + FIRST_BASELINE + h * (points.len() - 1) as f32 + DESCENT
    };
    // Astah keeps the stored height.
    let height = if auto_size {
        MIN_SIZE.height
    } else if points.is_empty() && names.len() > 1 {
        // The name block with a gap around it inside the ellipse: its corners on or within it.
        // A single line keeps Astah's height.
        let widest = names.iter().map(width).fold(0.0, f32::max);
        let across = ((widest + 2.0 * NAME_GAP) / r_width).min(0.99);
        MIN_SIZE.height.max((names_h + 2.0 * NAME_GAP) / (1.0 - across * across).sqrt())
    } else if points.is_empty() {
        MIN_SIZE.height
    } else {
        block + 2.0 * MARGIN
    };
    let x = if auto_size { bounds.center().x - r_width / 2.0 } else { bounds.x };
    let r = Rect::new(x, bounds.y, r_width, bounds.height.max(height));

    let top = r.center().y - block / 2.0;
    let mut rows: Vec<Row> = names
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            line.place(Rect::new(r.x, top + h * i as f32, r.width, h), LabelAlign::Center)
        })
        .collect();
    let separator = (!points.is_empty()).then_some(top + names_h - h + SEPARATOR);
    if let Some(separator) = separator {
        let inner = r.width / std::f32::consts::SQRT_2;
        let left = r.center().x - inner / 2.0;
        let first = separator + FIRST_BASELINE - crate::draw::baseline(FONT_SIZE);
        rows.extend(points.into_iter().enumerate().map(|(i, line)| {
            line.place(Rect::new(left, first + h * i as f32, inner, h), LabelAlign::Left)
        }));
    }
    (r, Figure::UseCase { separator }, rows)
}

/// Nodes drawn completely inside container nodes (system boundaries, packages) among `ids`,
/// recursively, together with `ids` themselves; for states also their substates, which may reach
/// out of them (a vertex belongs to the state its center lies in). Moving a container moves its
/// contents.
pub fn with_contents(model: &Model, scene: &Scene, ids: &[Id]) -> Vec<Id> {
    let nodes: HashMap<Id, &NodeShape> = scene.nodes.iter().map(|n| (n.id, n)).collect();
    let mut result: Vec<Id> = ids.to_vec();
    let mut taken: HashSet<Id> = result.iter().copied().collect();
    let mut i = 0;
    while i < result.len() {
        if let Some(container) = nodes.get(&result[i]).copied()
            && matches!(
                container.figure,
                Figure::Boundary
                    | Figure::Package { .. }
                    | Figure::Partition { .. }
                    | Figure::State { .. }
                    | Figure::Cube { .. }
                    | Figure::Component { .. }
            )
        {
            let substate = |n: &NodeShape| {
                matches!(container.figure, Figure::State { .. })
                    && container
                        .element
                        .zip(n.element)
                        .is_some_and(|(s, e)| model.is_ancestor(s, e))
            };
            for n in &scene.nodes {
                if n.id != container.id
                    && (container.rect.contains_rect(&n.rect) || substate(n))
                    && taken.insert(n.id)
                {
                    result.push(n.id);
                }
            }
        }
        i += 1;
    }
    result
}

/// The size Astah gives the box of presentation `p` when it sizes it to its content, for
/// classifier boxes that are sized automatically (`Style::auto_size`). Astah does that when the
/// content changes, not when it draws, so the bounds are only fitted while editing.
pub fn fitted_size(model: &Model, p: &Presentation, measure: &dyn TextMeasure) -> Option<Size> {
    fitted_box(model, p, measure).map(|(size, _)| size)
}

/// [`fitted_size`] and the texts of the box, one per line, to tell when its content changed.
pub fn fitted_box(
    model: &Model,
    p: &Presentation,
    measure: &dyn TextMeasure,
) -> Option<(Size, String)> {
    let bounds = p.bounds()?;
    let element = model.get(p.element?)?;
    if !p.style.auto_size || p.style.iconic || !is_boxed_classifier(element.kind()) {
        return None;
    }
    let (r, _, rows) = layout_classifier(model, element, &p.style, bounds, true, measure);
    let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
    Some((r.size(), texts.join("\n")))
}

/// Radius of an interface drawn as a circle.
pub const LOLLIPOP_RADIUS: f32 = 10.0;

/// Astah's stick figure of an actor: from hand to hand and from head to feet. It keeps this
/// size in boxes of any size.
pub const ACTOR_FIGURE: Size = Size { width: 28.0, height: 55.0 };

/// Astah's narrowest classifier box.
const MIN_CLASSIFIER_WIDTH: f32 = 40.0;
/// Astah's lowest classifier box, sized automatically (one with only a name).
const MIN_CLASSIFIER_HEIGHT: f32 = 30.0;
/// Astah makes automatically sized classifier boxes this much higher than their content when
/// they show a compartment below the name; a box with only its name gets nothing extra.
const AUTO_SIZE_EXTRA_HEIGHT: f32 = 3.0;

/// Classes, interfaces, enumerations and data types, drawn as boxes with compartments.
pub fn is_boxed_classifier(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

/// Lays out a classifier box in `bounds`, or with `fit` at the size Astah gives it when it
/// sizes the box automatically.
fn layout_classifier(
    model: &Model,
    e: &Element,
    style: &Style,
    bounds: Rect,
    fit: bool,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    // An empty compartment is 2 × `BOX_PAD_Y` high.
    const PAD_X: f32 = BOX_PAD_X;
    const PAD_Y: f32 = BOX_PAD_Y;
    let id = e.id();
    let kind = e.kind();
    let size = style.font_size.unwrap_or(FONT_SIZE);

    // Italics alone are easy to miss, so abstract classes say it in words as well: UML's
    // `{abstract}` below the name or, as a setting, «abstract» above it. A class with a
    // stereotype «abstract» of its own says it once, as in Astah.
    let is_abstract = matches!(kind, ElementKind::Class(p) if p.is_abstract);
    let says_so = !style.hide_stereotypes && e.data().stereotypes.iter().any(|s| s == "abstract");
    let keyword = is_abstract && !measure.abstract_stereotype() && !says_so;
    let mut stereotypes: Vec<String> = match kind {
        ElementKind::Interface(_) => vec!["interface".into()],
        ElementKind::Enumeration(_) => vec!["enumeration".into()],
        ElementKind::DataType(_) => vec!["dataType".into()],
        _ if is_abstract && !keyword && !says_so => vec!["abstract".into()],
        _ => Vec::new(),
    };
    if !style.hide_stereotypes {
        stereotypes.extend(e.data().stereotypes.iter().cloned());
    }
    let mut header = stereotype_lines([&stereotypes], size, id);
    let mut name = Line::new(crate::member::display_name(model, id), size, RowRole::Name, Some(id));
    name.bold = true;
    name.italic = is_abstract;
    header.push(name);
    if keyword {
        header.push(Line::new("{abstract}", size, RowRole::Stereotype, Some(id)));
    }

    let members = |pick: fn(&ElementKind) -> bool| -> Vec<Line> {
        e.children()
            .iter()
            .filter_map(|&c| model.get(c))
            .filter(|c| pick(c.kind()) && member_shown(model, c.id(), style))
            .map(|c| {
                let mut text = member_label(model, c.id(), style);
                if matches!(c.kind(), ElementKind::Attribute(p) if p.is_read_only)
                    && measure.read_only_constraint()
                {
                    text.push_str(" {readOnly}");
                }
                let mut line = Line::new(text, size, RowRole::Member, Some(c.id()));
                match c.kind() {
                    ElementKind::Attribute(p) => line.underline = p.is_static,
                    ElementKind::Operation(p) => {
                        line.underline = p.is_static;
                        line.italic = p.is_abstract;
                    }
                    _ => {}
                }
                line
            })
            .collect()
    };
    let attributes = members(|k| matches!(k, ElementKind::Attribute(_)));
    let operations = members(|k| matches!(k, ElementKind::Operation(_)));
    let mut compartments: Vec<Vec<Line>> = Vec::new();
    let mut kinds = Vec::new();
    if !style.hide_attributes {
        compartments.push(attributes);
        kinds.push(Compartment::Attributes);
    }
    if !style.hide_operations {
        compartments.push(operations);
        kinds.push(Compartment::Operations);
    }
    // Astah lists an enumeration's literals in a compartment of their own at the bottom.
    if let ElementKind::Enumeration(_) = kind {
        compartments.push(members(|k| matches!(k, ElementKind::EnumerationLiteral)));
        kinds.push(Compartment::Literals);
    }

    // A line needs its width as Astah measures it (Java's whole-pixel width or the exact one,
    // whichever is larger), or more where UmlCraft's visibility column takes more room.
    let needs = |l: &Line| l.astah_width_of(&l.text, measure).max(l.width(measure));
    // Fitted, the box is as wide as its widest line and 3 higher than its content. Sized
    // automatically, Astah keeps the stored size but widens the box around its center when a
    // line does not fit. Otherwise it keeps its width and only grows for the header and for
    // words that cannot break; longer members continue on further lines where Astah breaks
    // them, and the box grows where only UmlCraft's visibility column does not fit.
    let refit = fit || style.auto_size;
    let width = if refit {
        let widest =
            header.iter().chain(compartments.iter().flatten()).map(needs).fold(0.0, f32::max);
        widest + 2.0 * PAD_X
    } else {
        let header_w = header.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
        let word_w =
            compartments.iter().flatten().map(|l| l.widest_word(measure)).fold(0.0, f32::max);
        bounds.width.max(header_w.max(word_w) + 2.0 * PAD_X)
    }
    .max(MIN_CLASSIFIER_WIDTH);
    let compartments: Vec<Vec<Line>> = if refit {
        compartments
    } else {
        compartments
            .into_iter()
            .map(|c| c.into_iter().flat_map(|l| l.wrapped(width - 2.0 * PAD_X, measure)).collect())
            .collect()
    };
    let width =
        compartments.iter().flatten().map(|l| l.width(measure) + 2.0 * PAD_X).fold(width, f32::max);
    let block =
        |lines: &[Line]| 2.0 * PAD_Y + lines.iter().map(|l| line_height(l.size)).sum::<f32>();
    let header_h = block(&header);
    let content_h = header_h + compartments.iter().map(|c| block(c)).sum::<f32>();
    let extra = if compartments.is_empty() { 0.0 } else { AUTO_SIZE_EXTRA_HEIGHT };
    let fitted_h = (content_h + extra).max(MIN_CLASSIFIER_HEIGHT);
    // A box sized automatically whose stored height is that of its content keeps its stored
    // width and only widens around its center. One whose content changed since (other
    // members, another font size) Astah fits anew, also narrower, around its stored center.
    let stale = style.auto_size && !fit && (fitted_h - bounds.height).abs() > 0.5;
    let width = if refit && !fit && !stale { bounds.width.max(width) } else { width };
    // A box whose content no longer fits its stored height grows as if fitted: 3 below the
    // content (`CD_Dokus.asta`, `Serial_kurz` at 14 px).
    let height =
        if fit || stale || content_h > bounds.height + 0.5 { fitted_h } else { bounds.height };
    let c = bounds.center();
    let r = if stale {
        Rect::new(c.x - width / 2.0, c.y - height / 2.0, width, height)
    } else if refit && !fit && width > bounds.width {
        Rect::new(c.x - width / 2.0, bounds.y, width, height)
    } else {
        Rect::new(bounds.x, bounds.y, width, height)
    };

    let mut rows = Vec::new();
    let mut y = r.y + PAD_Y;
    for line in header {
        let h = line_height(line.size);
        rows.push(line.place(Rect::new(r.x, y, r.width, h), LabelAlign::Center));
        y += h;
    }
    y += PAD_Y;
    let mut separators = Vec::new();
    for compartment in compartments {
        separators.push(y);
        y += PAD_Y;
        for line in compartment {
            let h = line_height(line.size);
            rows.push(
                line.place(Rect::new(r.x + PAD_X, y, r.width - 2.0 * PAD_X, h), LabelAlign::Left),
            );
            y += h;
        }
        y += PAD_Y;
    }
    let template = template_box(model, id, r, measure, &mut rows);
    (r, Figure::Classifier { separators, compartments: kinds, template }, rows)
}

/// The dashed box with a template's parameters (`T>Number, N:int=10`), as Astah draws it: 4 px
/// around the text, which runs in one line with exact advances, its bottom 5 below the
/// classifier's top and a fifth of it sticking out on the right. Each parameter is a row of its
/// own, so it can be edited.
fn template_box(
    model: &Model,
    id: Id,
    r: Rect,
    measure: &dyn TextMeasure,
    rows: &mut Vec<Row>,
) -> Option<Rect> {
    const PAD: f32 = 4.0;
    const BELOW_TOP: f32 = 5.0;
    const BASELINE_BELOW_TOP: f32 = 15.0;
    let parameters = crate::member::template_parameters(model, id);
    if parameters.is_empty() {
        return None;
    }
    let count = parameters.len();
    let texts: Vec<(Id, String)> = parameters
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let text = crate::member::template_parameter_text(model, p);
            (p, if i + 1 < count { format!("{text}, ") } else { text })
        })
        .collect();
    let all: String = texts.iter().map(|(_, t)| t.as_str()).collect();
    let width = measure.width(&all, FONT_SIZE, false) + 2.0 * PAD;
    let height = line_height(FONT_SIZE) + PAD;
    let rect = Rect::new(r.right() - 0.8 * width, r.y + BELOW_TOP - height, width, height);
    let top = rect.y + BASELINE_BELOW_TOP - crate::draw::baseline(FONT_SIZE);
    let mut x = rect.x + PAD;
    for (parameter, text) in texts {
        let w = measure.width(&text, FONT_SIZE, false);
        let line = Line::new(text, FONT_SIZE, RowRole::Member, Some(parameter));
        rows.push(line.place(Rect::new(x, top, w, line_height(FONT_SIZE)), LabelAlign::Left));
        x += w;
    }
    Some(rect)
}

/// The text row under `p` on the topmost node there, e.g. for inline editing.
pub fn row_at(scene: &Scene, p: Point) -> Option<(&NodeShape, &Row)> {
    scene.nodes.iter().rev().find_map(|n| {
        let row = n.rows.iter().find(|r| r.role != RowRole::Stereotype && r.rect.contains(p))?;
        Some((n, row))
    })
}

#[allow(clippy::too_many_arguments)]
fn edge_for(
    id: Id,
    kind: Option<&ElementKind>,
    // An association's qualifiers per end, as text.
    qualifiers: &[Vec<String>; 2],
    // Each end's rectangle the anchors are relative to, the one the line is clipped at, and
    // its outline.
    ((source_anchors, source, source_outline), (target_anchors, target, target_outline)): (
        (Rect, Rect, Outline),
        (Rect, Rect, Outline),
    ),
    anchors: [Option<Point>; 2],
    shape: LineShape,
    waypoints: &[Point],
    (name_label, has_name, bind_keyword): (Option<String>, bool, Option<String>),
    to_lollipop: bool,
    placed: &EdgeLabels,
    style: &Style,
    measure: &dyn TextMeasure,
) -> EdgeShape {
    // The line runs from the source anchor through the waypoints to the target anchor; it is
    // drawn from where it leaves the source to where it enters the target. Right-angled lines
    // keep their first and last segment straight as the ends move.
    let (from, to) =
        (anchor_point(source_anchors, anchors[0]), anchor_point(target_anchors, anchors[1]));
    let fitted;
    let waypoints = if shape.is_right_angle() {
        fitted = route::fit_corners(source, from, waypoints, target, to);
        &fitted[..]
    } else {
        waypoints
    };
    // Waypoints inside an end node are hidden: the line starts where it last leaves the source
    // and ends where it first enters the target.
    let skip = waypoints.iter().take_while(|&&p| source_outline.contains(source, p)).count();
    let keep = skip
        + waypoints[skip..].iter().take_while(|&&p| !target_outline.contains(target, p)).count();
    let start = if skip > 0 { waypoints[skip - 1] } else { from };
    let end = waypoints.get(keep).copied().unwrap_or(to);
    let waypoints = &waypoints[skip..keep];
    let mut points = Vec::with_capacity(waypoints.len() + 2);
    let first_toward = waypoints.first().copied().unwrap_or(end);
    let last_from = waypoints.last().copied().unwrap_or(start);
    points.push(source_outline.border_point(source, start, first_toward));
    points.extend_from_slice(waypoints);
    points.push(target_outline.border_point(target, end, last_from));

    let node_tips = [points[0], points[points.len() - 1]];
    let mut edge = EdgeShape {
        id,
        points,
        node_tips,
        line: LineStyle::Solid,
        shape,
        source_marker: EndMarker::None,
        target_marker: EndMarker::None,
        labels: Vec::new(),
        fixed: false,
        ends: [source, target],
        anchors,
        name_arrow: None,
        qualifiers: Vec::new(),
        free_labels: Vec::new(),
        association: false,
        end_labels: Vec::new(),
        stroke: style.stroke,
    };
    // Role, multiplicity and whether the role is underlined (static), per end.
    let mut ends: [(Option<String>, Option<String>, bool); 2] = Default::default();
    let mut label = name_label;
    let mut second_line: Option<String> = None;
    // «include», «extend» and «bind» stand above the middle of the line, apart from any name.
    // Astah centers «bind» right above the middle however the line runs.
    let bind = bind_keyword.is_some();
    let mut keyword = bind_keyword;
    let end_marker = |end: &AssociationEnd| match (end.aggregation, end.navigability) {
        (Aggregation::None, Navigability::Navigable) => EndMarker::OpenArrow,
        (Aggregation::None, Navigability::NonNavigable) => EndMarker::Cross,
        (Aggregation::None, Navigability::Unspecified) => EndMarker::None,
        // A diamond keeps the navigability behind it.
        (a, Navigability::Navigable) => {
            EndMarker::DiamondArrow { filled: a == Aggregation::Composite }
        }
        (a, Navigability::NonNavigable) => {
            EndMarker::DiamondCross { filled: a == Aggregation::Composite }
        }
        (Aggregation::Shared, Navigability::Unspecified) => EndMarker::HollowDiamond,
        (Aggregation::Composite, Navigability::Unspecified) => EndMarker::FilledDiamond,
    };
    match kind {
        Some(ElementKind::Association(p)) => {
            let [a, b] = &p.ends;
            edge.association = true;
            edge.source_marker = end_marker(a);
            edge.target_marker = end_marker(b);
            // Role names show their visibility, as Astah does: "- customer". Constraints follow
            // in the same label: "- customer {ordered}".
            let texts = |end: &AssociationEnd| {
                // A minus as wide as the plus, as in compartments (`visibility_column`).
                let role = (!end.role.is_empty()).then(|| {
                    let text = format!("{} {}", end.visibility.symbol(), end.role);
                    match visibility_column(&text).filter(|_| measure.visibility_column()) {
                        Some((symbol, rest)) => format!("{symbol}{rest}"),
                        None => text,
                    }
                });
                let constraints = end.constraints.iter().map(|c| format!("{{{c}}}"));
                let role = role.into_iter().chain(constraints).collect::<Vec<_>>().join(" ");
                let multiplicity = end.multiplicity.map(|m| m.to_string());
                ((!role.is_empty()).then_some(role), multiplicity, end.is_static)
            };
            ends = [texts(a), texts(b)];
            // Qualifiers sit in a box against the end's node; the line then ends at the box.
            for (i, texts) in qualifiers.iter().enumerate().filter(|(_, t)| !t.is_empty()) {
                let n = edge.points.len();
                let (tip, next) = if i == 0 { (0, 1) } else { (n - 1, n - 2) };
                let rect = edge.ends[i];
                let (b, rows) = qualifier_box(rect, edge.points[tip], texts, measure);
                edge.points[tip] = border_point_from(b, edge.points[tip], edge.points[next]);
                edge.qualifiers.push((b, rows));
            }
        }
        // A link looks like an association between objects, with plain role names.
        Some(ElementKind::Link(p)) => {
            let marker = |end: &LinkEnd| {
                end_marker(&AssociationEnd {
                    navigability: end.navigability,
                    aggregation: end.aggregation,
                    ..AssociationEnd::new(end.instance)
                })
            };
            let [a, b] = &p.ends;
            edge.source_marker = marker(a);
            edge.target_marker = marker(b);
            let role = |end: &LinkEnd| (!end.role.is_empty()).then(|| end.role.clone());
            ends = [(role(a), None, false), (role(b), None, false)];
        }
        Some(ElementKind::Generalization(_)) => edge.target_marker = EndMarker::HollowTriangle,
        // Realizing an interface drawn as a circle is a plain line ("lollipop").
        Some(ElementKind::Realization(_)) if to_lollipop => {}
        Some(ElementKind::Realization(_)) => {
            edge.line = LineStyle::ShortDashed;
            edge.target_marker = EndMarker::HollowTriangle;
        }
        Some(ElementKind::Dependency(_)) => {
            edge.line = LineStyle::ShortDashed;
            edge.target_marker = EndMarker::OpenArrow;
        }
        Some(ElementKind::Include(_)) => {
            edge.line = LineStyle::ShortDashed;
            edge.target_marker = EndMarker::OpenArrow;
            keyword = Some("<<include>>".to_owned());
        }
        Some(ElementKind::Extend(_)) => {
            // Astah shows an extend's condition in its properties only, not at the line.
            edge.line = LineStyle::ShortDashed;
            edge.target_marker = EndMarker::OpenArrow;
            keyword = Some("<<extend>>".to_owned());
        }
        Some(ElementKind::Message(p)) => match p.sort {
            MessageSort::Asynchronous => edge.target_marker = EndMarker::OpenArrow,
            MessageSort::Reply => {
                edge.line = LineStyle::Dashed;
                edge.target_marker = EndMarker::OpenArrow;
            }
            _ => edge.target_marker = EndMarker::FilledArrow,
        },
        Some(ElementKind::ActivityEdge(p)) => {
            edge.target_marker = EndMarker::OpenArrow;
            let guard = p.guard.trim();
            if !guard.is_empty() {
                label = join_label(label, format!("[{guard}]"));
            }
        }
        Some(ElementKind::Transition(p)) => {
            edge.target_marker = EndMarker::OpenArrow;
            // Astah puts the guard (and what follows) on a line below the trigger.
            let trigger = p.trigger.trim();
            let text = if trigger.is_empty() || p.guard.trim().is_empty() {
                p.label()
            } else {
                let rest = TransitionProps { trigger: String::new(), ..p.clone() }.label();
                second_line = Some(rest);
                trigger.to_owned()
            };
            if !text.is_empty() {
                label = join_label(label, text);
            }
        }
        // A containment line runs from the owner to the nested element the edge stands for.
        Some(k) if k.is_classifier() => {
            edge.source_marker = EndMarker::Containment;
            label = None;
        }
        _ => edge.line = LineStyle::ShortDashed,
    }

    let pts = &edge.points;
    let n = pts.len();
    let at = |text: String, base: Point, offset: Point| EdgeLabel {
        text,
        anchor: base.offset(offset.x, offset.y),
        align: LabelAlign::Left,
        underline: false,
        keyword: false,
        color: None,
    };
    let (a, b) = route::middle_segment(pts, shape.is_curve());
    let center = middle(a, b);
    let association = matches!(kind, Some(ElementKind::Association(_)));
    let name = label.map(|text| match placed.name {
        Some(offset) => at(text, center, offset),
        None => association_name(text, center, measure),
    });
    let across = (center.offset(-1.0, 0.0), center.offset(1.0, 0.0));
    let segment = if bind { across } else { (a, b) };
    if let Some(text) = keyword {
        let index = edge.labels.len();
        note_free(&mut edge.free_labels, index, placed.keyword.is_none(), LabelKind::Keyword, 1);
        edge.labels.push(keyword_label(text, segment, placed.keyword, measure));
    }
    // Astah shows which way an association's name reads, from its first end to its second,
    // with a small triangle next to the name.
    if association
        && has_name
        && let Some(name) = &name
    {
        let width = label_width(&name.text, measure);
        edge.name_arrow = Some(name_arrow(name.anchor, width, (a, b)));
    }
    let below = second_line.zip(name.as_ref()).map(|(text, first)| {
        let anchor = first.anchor.offset(0.0, line_height(FONT_SIZE));
        EdgeLabel { text, anchor, ..first.clone() }
    });
    if name.is_some() {
        let (index, lines) = (edge.labels.len(), 1 + usize::from(below.is_some()));
        note_free(&mut edge.free_labels, index, placed.name.is_none(), LabelKind::Name, lines);
    }
    // Astah colors only the name, not the ends' roles and multiplicities.
    let colored = |l: EdgeLabel| EdgeLabel { color: style.font_color, ..l };
    edge.labels.extend(name.map(colored));
    edge.labels.extend(below.map(colored));
    let [(source_role, source_mult, source_static), (target_role, target_mult, target_static)] =
        ends;
    let (source_tip, source_from) = (pts[0], pts[1]);
    let (target_tip, target_from) = (pts[n - 1], pts[n - 2]);
    let place = |text: Option<String>, offset: Option<Point>, end: End, role, underline| {
        text.map(|text| {
            let node_tip = node_tips[usize::from(!end.source)];
            let label = match offset {
                Some(offset) => at(text, node_tip, offset),
                None => end_label(text, end, role, measure),
            };
            EdgeLabel { underline, ..label }
        })
    };
    let source = End { tip: source_tip, next: source_from, source: true };
    let target = End { tip: target_tip, next: target_from, source: false };
    let labels = [
        (source_role, placed.source_role, source, true, source_static),
        (source_mult, placed.source_multiplicity, source, false, false),
        (target_role, placed.target_role, target, true, target_static),
        (target_mult, placed.target_multiplicity, target, false, false),
    ];
    for (text, offset, end, role, underline) in labels {
        let Some(label) = place(text, offset, end, role, underline) else { continue };
        let kind = if role {
            LabelKind::Role { source: end.source }
        } else {
            LabelKind::Multiplicity { source: end.source }
        };
        note_free(&mut edge.free_labels, edge.labels.len(), offset.is_none(), kind, 1);
        if edge.association {
            let (index, end) = (edge.labels.len(), usize::from(!end.source));
            edge.end_labels.push(EndLabel { index, end, multiplicity: !role });
        }
        edge.labels.push(label);
    }
    edge
}

/// Notes the label at `index`, with the `lines - 1` after it that move along, as free if it
/// was not placed by hand.
fn note_free(
    free_labels: &mut Vec<FreeLabel>,
    index: usize,
    free: bool,
    kind: LabelKind,
    lines: usize,
) {
    if free {
        free_labels.push(FreeLabel { kind, index, lines, moved: false });
    }
}

/// A qualifier as Astah shows it: `key : Key`, or just the name if it has no type.
fn qualifier_text(model: &Model, q: &Qualifier) -> String {
    match crate::member::type_label(model, &q.type_ref) {
        Some(ty) if !ty.is_empty() => format!("{} : {ty}", q.name),
        _ => q.name.clone(),
    }
}

/// The box of qualifiers at the end `tip` of a line on the border of `node`, as Astah draws it:
/// outside the side `tip` lies on and centered on `tip`, 20 wider than the widest text, rows
/// 4 apart, at least 30 high. Texts start 10 from the left, their first baseline 13 below the top.
fn qualifier_box(
    node: Rect,
    tip: Point,
    texts: &[String],
    measure: &dyn TextMeasure,
) -> (Rect, Vec<Row>) {
    const PAD: f32 = 10.0;
    const FIRST_BASELINE: f32 = 13.0;
    const MIN_HEIGHT: f32 = 30.0;
    let step = LABEL_HEIGHT + 4.0;
    let widths: Vec<f32> = texts.iter().map(|t| measure.width(t, FONT_SIZE, false)).collect();
    let width = widths.iter().copied().fold(0.0, f32::max) + 2.0 * PAD;
    let height = (texts.len() as f32 * step - 4.0).max(MIN_HEIGHT);
    // The side `tip` is closest to.
    let sides = [
        (tip.x - node.x).abs(),
        (node.right() - tip.x).abs(),
        (tip.y - node.y).abs(),
        (node.bottom() - tip.y).abs(),
    ];
    let side = (0..4).min_by(|&a, &b| sides[a].total_cmp(&sides[b])).unwrap_or(0);
    let rect = match side {
        0 => Rect::new(node.x - width, tip.y - height / 2.0, width, height),
        1 => Rect::new(node.right(), tip.y - height / 2.0, width, height),
        2 => Rect::new(tip.x - width / 2.0, node.y - height, width, height),
        _ => Rect::new(tip.x - width / 2.0, node.bottom(), width, height),
    };
    let rows = texts
        .iter()
        .zip(widths)
        .enumerate()
        .map(|(i, (text, w))| {
            let top = rect.y + FIRST_BASELINE + i as f32 * step - crate::draw::baseline(FONT_SIZE);
            let line = Line::new(text.clone(), FONT_SIZE, RowRole::Member, None);
            line.place(Rect::new(rect.x + PAD, top, w, line_height(FONT_SIZE)), LabelAlign::Left)
        })
        .collect();
    (rect, rows)
}

/// An edge's name followed by `text`, e.g. a flow's guard.
fn join_label(name: Option<String>, text: String) -> Option<String> {
    Some(match name {
        Some(name) => format!("{name} {text}"),
        None => text,
    })
}

/// One end of a drawn line: where it meets the node, the next point along it, and whether it is
/// the end the line starts at.
#[derive(Clone, Copy)]
struct End {
    tip: Point,
    next: Point,
    source: bool,
}

/// The width of the box Astah gives a label: the text's whole-pixel width or its exact one,
/// whichever is larger, and at least 10.
pub fn label_width(text: &str, measure: &dyn TextMeasure) -> f32 {
    measure.astah_width(text, FONT_SIZE, false).max(10.0)
}

/// Places a role name (`role`) or multiplicity at an end of the line as Astah does: the
/// label's center lies 27 (roles) or 18 (multiplicities) along the line from its end and 18
/// across it, roles to the left of the direction the line runs in (from source to target),
/// multiplicities to the right. The text starts at the left of the label.
fn end_label(text: String, end: End, role: bool, measure: &dyn TextMeasure) -> EdgeLabel {
    const ROLE: (f32, f32) = (27.0, 18.0);
    const MULTIPLICITY: (f32, f32) = (18.0, -18.0);
    let End { tip, next, source } = end;
    let len = tip.distance(next).max(1e-6);
    let (ux, uy) = ((next.x - tip.x) / len, (next.y - tip.y) / len);
    let (tx, ty) = if source { (ux, uy) } else { (-ux, -uy) };
    let (lx, ly) = (ty, -tx);
    let (along, left) = if role { ROLE } else { MULTIPLICITY };
    let center = Point::new(tip.x + along * ux + left * lx, tip.y + along * uy + left * ly);
    let width = label_width(&text, measure);
    let anchor = center.offset(-width / 2.0, -LABEL_HEIGHT / 2.0);
    EdgeLabel {
        text,
        anchor,
        align: LabelAlign::Left,
        underline: false,
        keyword: false,
        color: None,
    }
}

/// Places the name of an edge as Astah does, whatever the edge (associations, dependencies,
/// transitions, flows …) and whichever way it runs: centered on the middle of the line, its
/// center 9 below it. A flow's guard and a transition's `trigger [guard] / effect` count as
/// names.
fn association_name(text: String, middle: Point, measure: &dyn TextMeasure) -> EdgeLabel {
    let width = label_width(&text, measure);
    let anchor = middle.offset(-width / 2.0, 9.0 - LABEL_HEIGHT / 2.0);
    EdgeLabel {
        text,
        anchor,
        align: LabelAlign::Left,
        underline: false,
        keyword: false,
        color: None,
    }
}

/// Astah's triangle that shows which way a name reads, next to the name's box (top left `at`,
/// `width` wide): it points along the line `a`–`b`, horizontally or vertically, whichever is
/// closer, and sits 4 to the right or left of the box, or below or above it.
pub(crate) fn name_arrow(at: Point, width: f32, (a, b): (Point, Point)) -> [Point; 3] {
    // The triangle is 10 long and 10 wide; beside the box its middle is 7.5 below the box's
    // top, below the box its base 15.5 below the top, above it 3.5 above.
    const GAP: f32 = 4.0;
    const LENGTH: f32 = 10.0;
    const HALF: f32 = 5.0;
    const BESIDE_MIDDLE: f32 = 7.5;
    const BELOW_BASE: f32 = 15.5;
    const ABOVE_BASE: f32 = 3.5;
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (left, top, right) = (at.x, at.y, at.x + width);
    let cx = at.x + width / 2.0;
    let p = Point::new;
    if dx.abs() >= dy.abs() {
        let (base, tip) = if dx >= 0.0 {
            (right + GAP, right + GAP + LENGTH)
        } else {
            (left - GAP, left - GAP - LENGTH)
        };
        let middle = top + BESIDE_MIDDLE;
        [p(base, middle - HALF), p(base, middle + HALF), p(tip, middle)]
    } else {
        let (base, tip) = if dy >= 0.0 {
            (top + BELOW_BASE, top + BELOW_BASE + LENGTH)
        } else {
            (top - ABOVE_BASE, top - ABOVE_BASE - LENGTH)
        };
        [p(cx - HALF, base), p(cx + HALF, base), p(cx, tip)]
    }
}

/// Places «include» or «extend» next to the middle of the segment `a`–`b`, so that it does not
/// cover the line: above lines that run across, centered and right on top as Astah does; to the
/// right of lines that run up or down, 4 away; in between on the side above or to the right.
/// `placed` is a position set by hand, relative to the middle.
fn keyword_label(
    text: String,
    (a, b): (Point, Point),
    placed: Option<Point>,
    measure: &dyn TextMeasure,
) -> EdgeLabel {
    let center = middle(a, b);
    let anchor = match placed {
        Some(offset) => center.offset(offset.x, offset.y),
        None => {
            let (half_w, half_h) = (label_width(&text, measure) / 2.0, LABEL_HEIGHT / 2.0);
            // The line's normal, pointing up or to the right.
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let length = dx.hypot(dy);
            let (nx, ny) = if length > 0.0 { (dy / length, -dx / length) } else { (0.0, -1.0) };
            let (nx, ny) =
                if nx - ny > 0.0 || (nx - ny == 0.0 && ny < 0.0) { (nx, ny) } else { (-nx, -ny) };
            // The label's middle as far out along the normal as the label reaches back.
            let distance = 4.0 * nx.abs() + half_w * nx.abs() + half_h * ny.abs();
            center.offset(nx * distance - half_w, ny * distance - half_h)
        }
    };
    EdgeLabel {
        text,
        anchor,
        align: LabelAlign::Left,
        underline: false,
        keyword: true,
        color: None,
    }
}

/// Where the line from the center of `rect` toward `toward` leaves the rectangle.
pub fn border_point(rect: Rect, toward: Point) -> Point {
    border_point_from(rect, rect.center(), toward)
}

/// Where the line from `inside` (a point in `rect`) toward `toward` leaves the rectangle; `toward`
/// itself if it lies inside too.
pub fn border_point_from(rect: Rect, inside: Point, toward: Point) -> Point {
    let (dx, dy) = (toward.x - inside.x, toward.y - inside.y);
    let exit = |d: f32, pos: f32, low: f32, high: f32| {
        if d > 0.0 {
            (high - pos) / d
        } else if d < 0.0 {
            (low - pos) / d
        } else {
            f32::INFINITY
        }
    };
    let t = exit(dx, inside.x, rect.x, rect.right())
        .min(exit(dy, inside.y, rect.y, rect.bottom()))
        .clamp(0.0, 1.0);
    Point::new(inside.x + dx * t, inside.y + dy * t)
}

/// Distance from `p` to the segment `a`–`b`.
pub fn distance_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0)
    };
    p.distance(Point::new(a.x + t * vx, a.y + t * vy))
}

/// Topmost node or edge under `p` (logical coordinates), with `tolerance` for thin lines.
/// Lines are drawn above all nodes, so they are hit first.
pub fn hit_test(scene: &Scene, p: Point, tolerance: f32) -> Option<Id> {
    edge_at(scene, p, tolerance)
        .or_else(|| scene.nodes.iter().rev().find(|n| n.hit(p, tolerance)).map(|n| n.id))
}

/// Topmost edge under `p`: its line within `tolerance`, or one of its qualifier boxes.
pub fn edge_at(scene: &Scene, p: Point, tolerance: f32) -> Option<Id> {
    let edge = scene.edges.iter().rev().find(|e| {
        e.path().windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= tolerance)
            || e.qualifiers.iter().any(|(rect, _)| rect.contains(p))
    });
    edge.map(|e| e.id)
}

/// A line between two regions of an orthogonal state under `p`, the topmost one: the state's
/// node, the index of the line (0 between the first two regions) and whether it is vertical.
pub fn region_line_at(
    scene: &Scene,
    p: Point,
    tolerance: f32,
) -> Option<(&NodeShape, usize, bool)> {
    scene.nodes.iter().rev().find_map(|n| {
        let Figure::State { regions, vertical, .. } = &n.figure else { return None };
        let index = regions.iter().skip(1).position(|region| {
            let a = region.rect;
            if *vertical {
                (p.x - a.x).abs() <= tolerance && p.y >= a.y && p.y <= a.bottom()
            } else {
                (p.y - a.y).abs() <= tolerance && p.x >= a.x && p.x <= a.right()
            }
        })?;
        Some((n, index, *vertical))
    })
}

/// The label of a sequence message under `p`: the message's presentation and the top left corner
/// of its name label (the first one; a stereotype above it moves with it).
pub fn message_label_at(scene: &Scene, p: Point, measure: &dyn TextMeasure) -> Option<(Id, Point)> {
    scene.edges.iter().rev().filter(|e| e.fixed).find_map(|e| {
        let hit = e.labels.iter().any(|l| {
            let area = crate::draw::label_area(l);
            let width = measure.width(&l.text, FONT_SIZE, false);
            let left = crate::draw::text_left(area, width, l.align);
            Rect::new(left, l.anchor.y, width, LABEL_HEIGHT).contains(p)
        });
        let name = e.labels.first()?;
        let width = measure.width(&name.text, FONT_SIZE, false);
        let left = crate::draw::text_left(crate::draw::label_area(name), width, name.align);
        hit.then_some((e.id, Point::new(left, name.anchor.y)))
    })
}

/// The «include» or «extend» label under `p`: the edge's presentation, the label's top left
/// corner and the middle of the line, which a label placed by hand is kept relative to.
pub fn keyword_label_at(
    scene: &Scene,
    p: Point,
    measure: &dyn TextMeasure,
) -> Option<(Id, Point, Point)> {
    scene.edges.iter().rev().filter(|e| !e.fixed).find_map(|e| {
        let label = e.labels.iter().find(|l| l.keyword)?;
        let width = measure.width(&label.text, FONT_SIZE, false);
        if !Rect::new(label.anchor.x, label.anchor.y, width, LABEL_HEIGHT).contains(p) {
            return None;
        }
        let (a, b) = route::middle_segment(&e.points, e.shape.is_curve());
        Some((e.id, label.anchor, middle(a, b)))
    })
}

/// The role name or multiplicity of an association end at `p`, for editing it in place: a label
/// under `p`, else the role of the end whose tip lies near `p` on the line, even if it has none
/// yet.
pub fn end_label_at(
    scene: &Scene,
    p: Point,
    tolerance: f32,
    measure: &dyn TextMeasure,
) -> Option<EndLabelHit> {
    // How far from its tip a click on the line still means the end.
    const REACH: f32 = 40.0;
    let rect = |e: &EdgeShape, l: &EndLabel| {
        let label = &e.labels[l.index];
        let width = label_width(&label.text, measure);
        Rect::new(label.anchor.x, label.anchor.y, width, LABEL_HEIGHT)
    };
    let hit = |e: &EdgeShape, l: &EndLabel, rect: Rect| EndLabelHit {
        edge: e.id,
        end: l.end,
        multiplicity: l.multiplicity,
        rect,
    };
    let associations = || scene.edges.iter().rev().filter(|e| e.association);
    if let Some(found) = associations().find_map(|e| {
        e.end_labels
            .iter()
            .map(|l| (l, rect(e, l)))
            .find(|(_, r)| r.contains(p))
            .map(|(l, r)| hit(e, l, r))
    }) {
        return Some(found);
    }
    let e = scene.edge(edge_at(scene, p, tolerance)?).filter(|e| e.association)?;
    let n = e.points.len();
    let ends = [
        End { tip: e.points[0], next: e.points[1], source: true },
        End { tip: e.points[n - 1], next: e.points[n - 2], source: false },
    ];
    let end = (0..2).find(|&i| ends[i].tip.distance(p) <= REACH)?;
    if let Some(l) = e.end_labels.iter().find(|l| l.end == end && !l.multiplicity) {
        return Some(hit(e, l, rect(e, l)));
    }
    let role = EndLabel { index: usize::MAX, end, multiplicity: false };
    let label = end_label("role".into(), ends[end], true, measure);
    Some(hit(e, &role, Rect::new(label.anchor.x, label.anchor.y, 80.0, LABEL_HEIGHT)))
}

/// Nodes lying completely inside `area`, e.g. a rubber band.
pub fn nodes_in(scene: &Scene, area: Rect) -> Vec<Id> {
    scene.nodes.iter().filter(|n| area.contains_rect(&n.rect)).map(|n| n.id).collect()
}

/// One of the eight resize handles around a selected node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    /// Relative position on the rectangle: (0, 0) top left … (1, 1) bottom right.
    fn factors(self) -> (f32, f32) {
        match self {
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::Left => (0.0, 0.5),
        }
    }

    pub fn position(self, r: Rect) -> Point {
        let (fx, fy) = self.factors();
        Point::new(r.x + r.width * fx, r.y + r.height * fy)
    }

    /// The handle of `r` within `tolerance` of `p`, if any.
    pub fn at(r: Rect, p: Point, tolerance: f32) -> Option<Handle> {
        Self::ALL.into_iter().find(|h| {
            let hp = h.position(r);
            (hp.x - p.x).abs() <= tolerance && (hp.y - p.y).abs() <= tolerance
        })
    }

    /// `r` resized by dragging this handle by (`dx`, `dy`), never smaller than `min`.
    pub fn resize(self, r: Rect, dx: f32, dy: f32, min: f32) -> Rect {
        let (fx, fy) = self.factors();
        let (mut left, mut top, mut right, mut bottom) = (r.x, r.y, r.right(), r.bottom());
        if fx == 0.0 {
            left = (left + dx).min(right - min);
        } else if fx == 1.0 {
            right = (right + dx).max(left + min);
        }
        if fy == 0.0 {
            top = (top + dy).min(bottom - min);
        } else if fy == 1.0 {
            bottom = (bottom + dy).max(top + min);
        }
        Rect::new(left, top, right - left, bottom - top)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Left,
    HorizontalCenter,
    Right,
    Top,
    VerticalCenter,
    Bottom,
}

/// New bounds that align `rects` to the first one, like most diagram editors do.
pub fn align(rects: &[Rect], alignment: Alignment) -> Vec<Rect> {
    let Some(reference) = rects.first().copied() else { return Vec::new() };
    rects
        .iter()
        .map(|r| {
            let (x, y) = match alignment {
                Alignment::Left => (reference.x, r.y),
                Alignment::HorizontalCenter => (reference.center().x - r.width / 2.0, r.y),
                Alignment::Right => (reference.right() - r.width, r.y),
                Alignment::Top => (r.x, reference.y),
                Alignment::VerticalCenter => (r.x, reference.center().y - r.height / 2.0),
                Alignment::Bottom => (r.x, reference.bottom() - r.height),
            };
            Rect::new(x, y, r.width, r.height)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{ElementData, TypeRef};

    #[test]
    fn border_point_leaves_through_the_right_side() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(border_point(r, Point::new(300.0, 25.0)), Point::new(100.0, 25.0));
        assert_eq!(border_point(r, Point::new(50.0, -100.0)), Point::new(50.0, 0.0));
        assert_eq!(border_point(r, Point::new(50.0, 25.0)), Point::new(50.0, 25.0));
    }

    #[test]
    fn sample_class_diagram_scene() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        assert_eq!(scene.nodes.len(), 3);
        assert_eq!(scene.edges.len(), 2);
        let generalization = scene
            .edges
            .iter()
            .find(|e| e.target_marker == EndMarker::HollowTriangle)
            .expect("generalization edge");
        // Premium (80,260 160x60) → Customer (80,80 160x90): straight up between the boxes.
        assert_eq!(generalization.points, vec![Point::new(160.0, 260.0), Point::new(160.0, 170.0)]);
        let customer = scene.nodes.iter().find(|n| n.element == Some(ids.customer)).unwrap();
        let Figure::Classifier { separators, .. } = &customer.figure else { panic!() };
        assert_eq!(separators.len(), 2, "attributes and operations");
        let texts: Vec<(&str, RowRole)> =
            customer.rows.iter().map(|r| (r.text.as_str(), r.role)).collect();
        assert_eq!(texts, vec![("Customer", RowRole::Name), ("- name : String", RowRole::Member)]);
        // The stored bounds are big enough, so the box keeps them.
        assert_eq!(customer.rect, Rect::new(80.0, 80.0, 160.0, 90.0));
    }

    #[test]
    fn free_texts_and_lines_are_drawn_as_astah_draws_them() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let mut line = Presentation::line(Point::new(10.0, 300.0), Point::new(150.0, 300.0));
        if let Shape::Line { dashed, start_arrow, .. } = &mut line.shape {
            (*dashed, *start_arrow) = (true, true);
        }
        let (text, line) = doc
            .edit("free", |tx| {
                let text = Presentation::text(Rect::new(40.0, 400.0, 30.0, 23.0), "a\nb c".into());
                Ok((tx.add_presentation(d, text)?, tx.add_presentation(d, line)?))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = scene.node(text).unwrap();
        assert!(matches!(node.figure, Figure::Text));
        let rows: Vec<(&str, f32)> =
            node.rows.iter().map(|r| (r.text.as_str(), r.rect.x)).collect();
        assert_eq!(rows, vec![("a", 50.0), ("b c", 50.0)]);
        // The first baseline lies 17 below the top; the box grows to hold both lines.
        let baseline = node.rows[0].rect.y + crate::draw::baseline(FONT_SIZE);
        assert!((baseline - 417.0).abs() < 1e-3, "{baseline}");
        assert!(node.rect.bottom() >= node.rows[1].rect.bottom());
        let edge = scene.edge(line).unwrap();
        assert_eq!(edge.points, vec![Point::new(10.0, 300.0), Point::new(150.0, 300.0)]);
        assert_eq!((edge.line, edge.source_marker), (LineStyle::ShortDashed, EndMarker::OpenArrow));
        assert_eq!(edge.target_marker, EndMarker::None);
        // Both move with the selection.
        let preview =
            Preview { moved: [text, line].into(), dx: 5.0, dy: 7.0, ..Preview::default() };
        let scene = build(doc.model(), d, &preview, &ApproxMeasure);
        assert_eq!(scene.node(text).unwrap().rect.x, 45.0);
        assert_eq!(scene.edge(line).unwrap().points[0], Point::new(15.0, 307.0));
    }

    #[test]
    fn objects_show_slots_and_links_their_roles() {
        let (mut doc, ids) = order_system();
        let d = ids.object_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        assert_eq!(scene.nodes.len(), 2);
        let ada = &scene.nodes[0];
        let Figure::Classifier { separators, .. } = &ada.figure else { panic!() };
        assert_eq!(separators.len(), 1);
        let texts: Vec<(&str, RowRole, bool)> =
            ada.rows.iter().map(|r| (r.text.as_str(), r.role, r.underline)).collect();
        assert_eq!(
            texts,
            vec![("ada : Customer", RowRole::Name, true), ("name = \"Ada\"", RowRole::Slot, false)]
        );
        assert_eq!(ada.rows[1].element, Some(doc.model().children(ids.customer)[0]));

        let link = &scene.edges[0];
        assert_eq!(link.source_marker, EndMarker::None);
        assert_eq!(link.target_marker, EndMarker::OpenArrow);
        let labels: Vec<&str> = link.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(labels, ["customer"]);

        // An unnamed object of a class without attributes keeps an empty slot compartment.
        let node = doc
            .edit("object", |tx| {
                crate::palette::create_node(
                    tx,
                    d,
                    crate::palette::NodeTool::Object,
                    Point::new(100.0, 300.0),
                    "",
                )
                .map(|(_, n)| n)
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let object = scene.node(node).unwrap();
        let Figure::Classifier { separators, .. } = &object.figure else { panic!() };
        assert_eq!(separators.len(), 1);
        assert_eq!(object.rows.len(), 1);
        assert!(object.rows[0].underline);

        // Astah's presentations may hide the name, the class or the slots.
        let ada_node = scene.nodes[0].id;
        let texts = |doc: &umlcraft_model::Document, hide: fn(&mut Style)| {
            let mut doc = doc.clone();
            doc.edit("hide", |tx| tx.update_presentation(d, ada_node, |p| hide(&mut p.style)))
                .unwrap();
            let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
            let ada = scene.node(ada_node).unwrap();
            let Figure::Classifier { separators, .. } = &ada.figure else { panic!() };
            let rows: Vec<String> = ada.rows.iter().map(|r| r.text.clone()).collect();
            (rows, separators.len())
        };
        let name = texts(&doc, |s| s.hide_instance_name = true);
        assert_eq!(name, (vec![": Customer".to_owned(), "name = \"Ada\"".to_owned()], 1));
        let class = texts(&doc, |s| s.hide_instance_type = true);
        assert_eq!(class.0[0], "ada");
        assert_eq!(texts(&doc, |s| s.hide_attributes = true), (vec!["ada : Customer".into()], 0));
    }

    #[test]
    fn boxes_grow_with_their_content_and_rows_are_editable() {
        let (mut doc, ids) = order_system();
        let customer = ids.customer;
        doc.edit("grow", |tx| {
            for i in 0..6 {
                let name = format!("aVeryLongAttributeName{i}");
                tx.create(
                    customer,
                    umlcraft_model::ElementData::new(name, ElementKind::attribute(None)),
                )?;
            }
            Ok(())
        })
        .unwrap();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let node = scene.nodes.iter().find(|n| n.element == Some(customer)).unwrap();
        assert!(node.rect.width > 160.0 && node.rect.height > 90.0, "{:?}", node.rect);
        let last = node.rows.last().unwrap();
        assert!(node.rect.contains_rect(&last.rect));
        let (hit, row) = row_at(&scene, last.rect.center()).unwrap();
        assert_eq!(hit.id, node.id);
        assert_eq!(row.role, RowRole::Member);
        assert_eq!(row.text, "+ aVeryLongAttributeName5");
    }

    #[test]
    fn hidden_compartments_and_lollipops() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let node = doc.model().shown_on(ids.order)[0].1;
        doc.edit("style", |tx| {
            tx.update_presentation(d, node, |p| {
                p.style.hide_attributes = true;
                p.style.hide_operations = true;
            })
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let order = scene.node(node).unwrap();
        assert!(
            matches!(&order.figure, Figure::Classifier { separators, .. } if separators.is_empty())
        );
        assert_eq!(order.rows.len(), 1);

        // An interface drawn as a circle, realized by Order: plain line, no triangle.
        let (interface, realization) = doc
            .edit("lollipop", |tx| {
                let (i, ni) = crate::palette::create_node(
                    tx,
                    d,
                    crate::palette::NodeTool::Interface,
                    Point::new(600.0, 100.0),
                    "I",
                )?;
                tx.update_presentation(d, ni, |p| p.style.iconic = true)?;
                let r = crate::palette::connect(
                    tx,
                    d,
                    crate::palette::EdgeTool::Realization,
                    node,
                    ni,
                )?;
                Ok((i, r))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let circle = scene.nodes.iter().find(|n| n.element == Some(interface)).unwrap();
        assert!(matches!(circle.figure, Figure::Lollipop));
        assert_eq!(circle.rect.size(), umlcraft_model::Size::new(24.0, 24.0));
        let edge = scene.edge(realization).unwrap();
        assert!(edge.line == LineStyle::Solid && edge.target_marker == EndMarker::None);
    }

    #[test]
    fn system_boundaries_are_hollow_and_carry_their_contents() {
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let boundary = doc
            .edit("boundary", |tx| {
                let (_, node) = crate::palette::create_node(
                    tx,
                    d,
                    crate::palette::NodeTool::SystemBoundary,
                    Point::new(180.0, 60.0),
                    "System",
                )?;
                Ok(node)
            })
            .unwrap();
        let view = doc.model().view(d).unwrap();
        assert_eq!(view.presentations()[0].id, boundary, "boundaries go to the back");

        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let frame = scene.node(boundary).unwrap();
        assert!(matches!(frame.figure, Figure::Boundary));
        // As in Astah: the name regular, centered, its baseline 24 below the top.
        let name = &frame.rows[0];
        assert!(!name.bold && name.size == FONT_SIZE);
        let baseline = name.rect.y + crate::draw::baseline(FONT_SIZE);
        assert!((baseline - (frame.rect.y + 24.0)).abs() < 0.01, "{baseline}");
        assert!((name.rect.center().x - frame.rect.center().x).abs() < 0.01);
        // Inside the frame, away from border and title: not the boundary.
        let inside = Point::new(frame.rect.x + 40.0, frame.rect.bottom() - 40.0);
        assert_ne!(hit_test(&scene, inside, 4.0), Some(boundary));
        assert_eq!(hit_test(&scene, Point::new(frame.rect.x + 1.0, inside.y), 4.0), Some(boundary));
        assert_eq!(hit_test(&scene, frame.rows[0].rect.center(), 4.0), Some(boundary));

        // The "Place order" use case (220,110 140x60) lies inside the boundary (180,60 300x360).
        let use_case = doc.model().shown_on(doc.model().children(doc.model().root())[2])[0].1;
        let moved = with_contents(doc.model(), &scene, &[boundary]);
        assert!(moved.contains(&boundary) && moved.contains(&use_case), "{moved:?}");
        assert_eq!(moved.len(), 2, "the actor at x=60 is outside");
    }

    #[test]
    fn use_cases_list_extension_points_as_astah_does() {
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        let node = doc.model().shown_on(place_order)[0].1;
        doc.edit("edit", |tx| {
            tx.create(
                place_order,
                umlcraft_model::ElementData::new("payment", ElementKind::ExtensionPoint),
            )?;
            tx.update_presentation(d, node, |p| {
                p.shape = Shape::Node { bounds: Rect::new(50.0, 50.0, 90.0, 40.0), regions: None };
            })
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &Fixed(76.6875));
        let node = scene.nodes.iter().find(|n| n.element == Some(place_order)).unwrap();
        // Astah's probe: the name plus 62 wide; one point makes it 85.72 high.
        assert!((node.rect.width - 138.6875).abs() < 0.01, "{:?}", node.rect);
        assert!((node.rect.height - 85.7208).abs() < 0.01, "{:?}", node.rect);
        let Figure::UseCase { separator: Some(sep) } = node.figure else { panic!("separator") };
        assert!((sep - 85.31).abs() < 0.02, "line at {sep}");
        let texts: Vec<(&str, bool, LabelAlign)> =
            node.rows.iter().map(|r| (r.text.as_str(), r.bold, r.align)).collect();
        assert_eq!(
            texts,
            [
                (doc.model().get(place_order).unwrap().name(), true, LabelAlign::Center),
                ("extension points", false, LabelAlign::Left),
                ("payment", false, LabelAlign::Left),
            ]
        );
        // Baselines and the left edge of the inscribed rectangle.
        let baseline = |i: usize| node.rows[i].rect.y + crate::draw::baseline(FONT_SIZE);
        assert!((baseline(1) - 100.31).abs() < 0.02 && (baseline(2) - 115.41).abs() < 0.02);
        assert!((node.rows[0].rect.y - 68.22).abs() < 0.02, "{:?}", node.rows[0].rect);
        assert!((node.rows[1].rect.x - 70.31).abs() < 0.02, "{:?}", node.rows[1].rect);
    }

    #[test]
    fn extend_keeps_its_condition_off_the_line() {
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        let node = doc.model().shown_on(place_order)[0].1;
        let edge = doc
            .edit("extend", |tx| {
                let (_, other) = crate::palette::create_node(
                    tx,
                    d,
                    crate::palette::NodeTool::UseCase,
                    Point::new(220.0, 300.0),
                    "Use case ",
                )?;
                let edge =
                    crate::palette::connect(tx, d, crate::palette::EdgeTool::Extend, other, node)?;
                let relationship = tx.model().view(d).unwrap().get(edge).unwrap().element.unwrap();
                tx.update(relationship, |data| {
                    if let ElementKind::Extend(p) = &mut data.kind {
                        p.condition = "express".into();
                    }
                })?;
                Ok(edge)
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        assert_eq!(scene.edge(edge).unwrap().labels[0].text, "<<extend>>");
    }

    #[test]
    fn notes_anchor_to_the_middle_of_a_line() {
        use crate::palette::{EdgeTool, NodeTool, connect, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        let node = doc.model().shown_on(place_order)[0].1;
        let (extend, anchor, note) = doc
            .edit("note", |tx| {
                let (_, other) =
                    create_node(tx, d, NodeTool::UseCase, Point::new(220.0, 300.0), "Use case ")?;
                let extend = connect(tx, d, EdgeTool::Extend, other, node)?;
                let (note, n_note) =
                    create_node(tx, d, NodeTool::Note, Point::new(500.0, 300.0), "")?;
                let anchor = connect(tx, d, EdgeTool::NoteAnchor, n_note, extend)?;
                Ok((extend, anchor, note))
            })
            .unwrap();
        let relationship = doc.model().view(d).unwrap().get(extend).unwrap().element.unwrap();
        let ElementKind::Comment(p) = doc.model().get(note).unwrap().kind() else { panic!() };
        assert_eq!(p.annotated, vec![relationship]);

        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let line = scene.edge(extend).unwrap();
        let (a, b) = route::middle_segment(&line.points, false);
        let tip = *scene.edge(anchor).expect("the anchor is drawn").points.last().unwrap();
        assert!(tip.distance(middle(a, b)) < 0.01, "{tip:?}");

        // The anchor goes with the line.
        doc.edit("delete", |tx| tx.remove_presentations(d, [extend].into_iter().collect()))
            .unwrap();
        assert!(doc.model().view(d).unwrap().get(anchor).is_none());
    }

    #[test]
    fn abstract_classes_say_so_in_words() {
        use crate::palette::{NodeTool, create_node};
        struct Stereotype;
        impl TextMeasure for Stereotype {
            fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
                ApproxMeasure.width(text, font_size, bold)
            }
            fn abstract_stereotype(&self) -> bool {
                true
            }
        }
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (class, node) = doc
            .edit("class", |tx| {
                create_node(tx, d, NodeTool::Class, Point::new(600.0, 400.0), "Shape")
            })
            .unwrap();
        doc.edit("abstract", |tx| {
            tx.update(class, |data| {
                if let ElementKind::Class(p) = &mut data.kind {
                    p.is_abstract = true;
                }
            })
        })
        .unwrap();
        let header = |model: &Model, measure: &dyn TextMeasure| -> Vec<(String, bool)> {
            let scene = build(model, d, &Preview::default(), measure);
            let rows = &scene.node(node).unwrap().rows;
            rows.iter().take(2).map(|r| (r.text.clone(), r.italic)).collect()
        };
        assert_eq!(
            header(doc.model(), &ApproxMeasure),
            [("Shape1".to_owned(), true), ("{abstract}".to_owned(), false)]
        );
        assert_eq!(
            header(doc.model(), &Stereotype),
            [("<<abstract>>".to_owned(), false), ("Shape1".to_owned(), true)]
        );
        // A stereotype «abstract» of its own says it once, either way.
        doc.edit("stereotype", |tx| {
            tx.update(class, |data| data.stereotypes = vec!["abstract".into()])
        })
        .unwrap();
        let once = [("<<abstract>>".to_owned(), false), ("Shape1".to_owned(), true)];
        assert_eq!(header(doc.model(), &ApproxMeasure), once);
        assert_eq!(header(doc.model(), &Stereotype), once);
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        assert!(!scene.node(node).unwrap().rows.iter().any(|r| r.text == "{abstract}"));
    }

    #[test]
    fn read_only_attributes_say_so_when_asked() {
        struct ReadOnly;
        impl TextMeasure for ReadOnly {
            fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
                ApproxMeasure.width(text, font_size, bold)
            }
            fn read_only_constraint(&self) -> bool {
                true
            }
        }
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let attribute = doc
            .edit("cap", |tx| {
                let kind = ElementKind::attribute(Some(TypeRef::Text("int".into())));
                let id = tx.create(ids.customer, ElementData::new("CAP", kind))?;
                tx.update(id, |data| {
                    if let ElementKind::Attribute(p) = &mut data.kind {
                        p.is_read_only = true;
                    }
                })?;
                Ok(id)
            })
            .unwrap();
        let row = |measure: &dyn TextMeasure| {
            let scene = build(doc.model(), d, &Preview::default(), measure);
            // A row too long for the box wraps at a space.
            let rows = scene.nodes.iter().flat_map(|n| &n.rows);
            let texts: Vec<&str> =
                rows.filter(|r| r.element == Some(attribute)).map(|r| r.text.as_str()).collect();
            texts.join(" ")
        };
        assert_eq!(row(&ApproxMeasure), "+ CAP : int");
        assert_eq!(row(&ReadOnly), "+ CAP : int {readOnly}");
    }

    #[test]
    fn association_ends_are_found_by_their_labels_and_near_their_tips() {
        use crate::palette::{EdgeTool, NodeTool, connect, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (association, edge) = doc
            .edit("association", |tx| {
                let (_, a) = create_node(tx, d, NodeTool::Class, Point::new(100.0, 900.0), "A")?;
                let (_, b) = create_node(tx, d, NodeTool::Class, Point::new(500.0, 900.0), "B")?;
                let edge = connect(tx, d, EdgeTool::Association, a, b)?;
                let view = tx.model().view(d).unwrap();
                Ok((view.get(edge).unwrap().element.unwrap(), edge))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let shape = scene.edge(edge).unwrap();
        assert!(shape.association && shape.end_labels.is_empty());
        // No role yet: near the target's tip, its role is offered where it would go.
        let tip = *shape.points.last().unwrap();
        let near = end_label_at(&scene, tip.offset(-20.0, 0.0), 3.0, &ApproxMeasure).unwrap();
        assert_eq!((near.edge, near.end, near.multiplicity), (edge, 1, false));
        let middle = middle(shape.points[0], tip);
        assert_eq!(end_label_at(&scene, middle, 3.0, &ApproxMeasure), None);

        doc.edit("ends", |tx| {
            tx.update(association, |data| {
                if let ElementKind::Association(p) = &mut data.kind {
                    p.ends[0].role = "owner".into();
                    p.ends[0].multiplicity = Some("1".parse().unwrap());
                }
            })
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let shape = scene.edge(edge).unwrap();
        assert_eq!(shape.end_labels.len(), 2);
        for l in &shape.end_labels {
            let label = &shape.labels[l.index];
            let inside = label.anchor.offset(2.0, LABEL_HEIGHT / 2.0);
            let hit = end_label_at(&scene, inside, 3.0, &ApproxMeasure).unwrap();
            assert_eq!((hit.end, hit.multiplicity), (0, l.multiplicity), "{}", label.text);
            assert_eq!(label.text.contains("owner"), !l.multiplicity);
        }
    }

    #[test]
    fn a_new_member_goes_below_the_last_one_of_its_compartment() {
        use crate::palette::{NodeTool, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (class, node) = doc
            .edit("class", |tx| {
                create_node(tx, d, NodeTool::Class, Point::new(600.0, 400.0), "Shape")
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let empty = compartment_of(&scene, node, Compartment::Attributes).unwrap();
        assert_eq!((empty.element, empty.last), (class, None));
        let inside = Point::new(empty.new_row.x + 5.0, empty.new_row.y + 2.0);
        assert_eq!(compartment_at(&scene, inside), Some(empty));
        let operations = compartment_of(&scene, node, Compartment::Operations).unwrap();
        assert!(operations.new_row.y > empty.new_row.y);
        assert_eq!(compartment_of(&scene, node, Compartment::Literals), None);

        let attribute = doc
            .edit("attribute", |tx| {
                tx.create(class, ElementData::new("area", ElementKind::attribute(None)))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let one = compartment_of(&scene, node, Compartment::Attributes).unwrap();
        assert_eq!(one.last, Some(attribute));
        assert!(one.new_row.y > empty.new_row.y);
        // The class's name is no compartment for members.
        let rect = scene.node(node).unwrap().rect;
        let name = Point::new(rect.x + 10.0, rect.y + 5.0);
        assert_eq!(compartment_at(&scene, name), None);
    }

    #[test]
    fn lines_from_below_an_actor_stop_under_its_name() {
        use crate::palette::{EdgeTool, NodeTool, connect, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let (upper, edge) = doc
            .edit("actors", |tx| {
                let (_, upper) =
                    create_node(tx, d, NodeTool::Actor, Point::new(600.0, 40.0), "Actor")?;
                let (_, lower) =
                    create_node(tx, d, NodeTool::Actor, Point::new(600.0, 300.0), "Actor")?;
                Ok((upper, connect(tx, d, EdgeTool::Generalization, upper, lower)?))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let actor = scene.nodes.iter().find(|n| n.id == upper).unwrap();
        let name = actor.rows[0].rect;
        assert!(name.bottom() > actor.rect.bottom(), "the name hangs below the box");
        let start = scene.edge(edge).unwrap().points[0];
        assert!(start.y >= name.bottom() - 0.01, "{start:?} crosses {name:?}");
    }

    #[test]
    fn bound_classes_show_what_they_bind() {
        use umlcraft_model::{DirectedProps, TemplateParameterProps, TypeExpr};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (bound, bind) = doc
            .edit("bound", |tx| {
                let root = tx.model().root();
                let pair = tx.create(root, ElementData::new("Pair", ElementKind::class()))?;
                for name in ["T1", "T2"] {
                    let parameter =
                        ElementKind::TemplateParameter(TemplateParameterProps::default());
                    tx.create(pair, ElementData::new(name, parameter))?;
                }
                let mut kind = ElementKind::class();
                if let ElementKind::Class(p) = &mut kind {
                    p.binding = Some(TypeRef::Expr(Box::new(TypeExpr {
                        base: TypeRef::Element(pair),
                        arguments: vec![TypeRef::Text("int".into()), TypeRef::Text("int".into())],
                        prefix: String::new(),
                        modifier: String::new(),
                    })));
                }
                let class = tx.create(root, ElementData::new("", kind))?;
                let mut data = ElementData::new(
                    "",
                    ElementKind::Dependency(DirectedProps { client: class, supplier: pair }),
                );
                data.stereotypes = vec!["bind".into()];
                let dependency = tx.create(root, data)?;
                let n_pair = tx.add_node(d, Some(pair), Rect::new(400.0, 400.0, 80.0, 50.0))?;
                let n_class = tx.add_node(d, Some(class), Rect::new(700.0, 400.0, 120.0, 50.0))?;
                let edge = tx.add_edge(d, Some(dependency), n_class, n_pair, Vec::new())?;
                Ok((n_class, edge))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = scene.nodes.iter().find(|n| n.id == bound).unwrap();
        assert!(node.rows.iter().any(|r| r.text == "Pair<int,int>"), "{:?}", node.rows);
        let edge = scene.edge(bind).unwrap();
        let texts: Vec<(&str, bool)> =
            edge.labels.iter().map(|l| (l.text.as_str(), l.keyword)).collect();
        assert_eq!(texts, [("<<bind>>", true), ("<T1->int,T2->int>", false)]);
        // Astah centers «bind» above the middle and the substitutions below it.
        let (a, b) = route::middle_segment(&edge.points, false);
        let center = middle(a, b);
        for label in &edge.labels {
            let w = label_width(&label.text, &ApproxMeasure);
            assert!((label.anchor.x + w / 2.0 - center.x).abs() < 0.01, "{label:?}");
        }
    }

    #[test]
    fn notes_wrap_their_text() {
        use crate::palette::{NodeTool, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (note, node) = doc
            .edit("note", |tx| {
                let (note, node) =
                    create_node(tx, d, NodeTool::Note, Point::new(500.0, 300.0), "")?;
                tx.update(note, |data| {
                    if let ElementKind::Comment(p) = &mut data.kind {
                        p.body = "one two three four five six seven eight nine ten\nend".into();
                    }
                })?;
                Ok((note, node))
            })
            .unwrap();
        let width = doc.model().view(d).unwrap().get(node).unwrap().bounds().unwrap().width;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let shape = scene.nodes.iter().find(|n| n.element == Some(note)).unwrap();
        // Astah keeps the note's width and breaks the long line at spaces.
        assert_eq!(shape.rect.width, width);
        assert!(shape.rows.len() > 2, "{:?}", shape.rows);
        assert_eq!(shape.rows.last().unwrap().text, "end");
    }

    #[test]
    fn subsystems_are_packages_with_a_fork() {
        use crate::palette::{NodeTool, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (sub, pkg) = doc
            .edit("packages", |tx| {
                let (sub, _) =
                    create_node(tx, d, NodeTool::Subsystem, Point::new(500.0, 300.0), "Billing")?;
                let (pkg, _) =
                    create_node(tx, d, NodeTool::Package, Point::new(700.0, 300.0), "Shop")?;
                Ok((sub, pkg))
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let shape = |id| scene.nodes.iter().find(|n| n.element == Some(id)).unwrap();
        assert!(matches!(shape(sub).figure, Figure::Package { subsystem: true, .. }));
        assert!(matches!(shape(pkg).figure, Figure::Package { subsystem: false, .. }));
        // Astah shows the fork, not the stereotype.
        let texts: Vec<_> = shape(sub).rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["Billing1"]);
    }

    #[test]
    fn keywords_give_way_to_notes() {
        use crate::palette::{EdgeTool, NodeTool, connect, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        let node = doc.model().shown_on(place_order)[0].1;
        let extend = doc
            .edit("extend", |tx| {
                let (_, other) =
                    create_node(tx, d, NodeTool::UseCase, Point::new(220.0, 300.0), "Use case ")?;
                connect(tx, d, EdgeTool::Extend, other, node)
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let line = scene.edge(extend).unwrap();
        let (a, b) = route::middle_segment(&line.points, false);
        let center = middle(a, b);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let normal = (dy / dx.hypot(dy), -dx / dx.hypot(dy));
        // A note on either side of the line; the keyword stays on the other.
        for side in [1.0, -1.0] {
            let mut doc = doc.clone();
            let at = center.offset(side * normal.0 * 150.0 - 40.0, side * normal.1 * 150.0 - 20.0);
            let anchor = doc
                .edit("note", |tx| {
                    let (_, n_note) = create_node(tx, d, NodeTool::Note, at, "")?;
                    connect(tx, d, EdgeTool::NoteAnchor, n_note, extend)
                })
                .unwrap();
            let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
            let pts = &scene.edge(anchor).unwrap().points;
            let from = pts[pts.len() - 2];
            let label = scene.edge(extend).unwrap().labels.iter().find(|l| l.keyword).unwrap();
            let width = label_width(&label.text, &ApproxMeasure);
            let mid = label.anchor.offset(width / 2.0, LABEL_HEIGHT / 2.0);
            let toward =
                (mid.x - center.x) * (from.x - center.x) + (mid.y - center.y) * (from.y - center.y);
            assert!(toward < 0.0, "side {side}: {mid:?}, note from {from:?}");
        }
    }

    #[test]
    fn use_case_names_wrap_inside_the_ellipse() {
        use crate::palette::{NodeTool, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let (element, node) = doc
            .edit("use case", |tx| {
                create_node(tx, d, NodeTool::UseCase, Point::new(220.0, 300.0), "Use case ")
            })
            .unwrap();
        let bounds = Rect::new(220.0, 300.0, 160.0, 40.0);
        let name = "Wareneingang aus Einkauf bearbeiten";
        doc.edit("name", |tx| {
            tx.set_bounds(d, node, bounds)?;
            tx.rename(element, name)
        })
        .unwrap();
        // 7 per character.
        struct PerChar;
        impl TextMeasure for PerChar {
            fn width(&self, text: &str, _: f32, _: bool) -> f32 {
                7.0 * text.chars().count() as f32
            }
        }
        let measure = PerChar;
        let scene = build(doc.model(), d, &Preview::default(), &measure);
        let n = scene.node(node).unwrap();
        let names: Vec<&str> = n.rows.iter().map(|r| r.text.as_str()).collect();
        // 160 less Astah's 62 leaves 98, 14 characters of 7.
        assert_eq!(names, ["Wareneingang", "aus Einkauf", "bearbeiten"]);
        assert_eq!(n.rect.width, 160.0);
        // Taller, so that the name block's corners lie inside the ellipse.
        assert!(n.rect.height > 40.0, "{:?}", n.rect);
        let (c, a, b) = (n.rect.center(), n.rect.width / 2.0, n.rect.height / 2.0);
        for row in &n.rows {
            let w = measure.width(&row.text, FONT_SIZE, true);
            for (x, y) in [(c.x + w / 2.0, row.rect.y), (c.x + w / 2.0, row.rect.bottom())] {
                let (dx, dy) = ((x - c.x) / a, (y - c.y) / b);
                assert!(dx * dx + dy * dy <= 1.0, "{} at {y}", row.text);
            }
        }

        // A word wider than the space widens the ellipse; line breaks in the name stay.
        doc.edit("name", |tx| tx.rename(element, "Pinnwandeintragsverwaltung\nöffnen")).unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &measure);
        let n = scene.node(node).unwrap();
        let names: Vec<&str> = n.rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(names, ["Pinnwandeintragsverwaltung", "öffnen"]);
        assert_eq!(n.rect.width, 26.0 * 7.0 + 62.0);
    }

    #[test]
    fn lines_end_at_the_ellipse_of_a_use_case() {
        use crate::palette::{EdgeTool, NodeTool, connect, create_node};
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        let node = doc.model().shown_on(place_order)[0].1;
        let extend = doc
            .edit("extend", |tx| {
                let (_, other) =
                    create_node(tx, d, NodeTool::UseCase, Point::new(220.0, 300.0), "Use case ")?;
                connect(tx, d, EdgeTool::Extend, other, node)
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let line = scene.edge(extend).unwrap();
        for (tip, rect) in
            [(line.points[0], line.ends[0]), (*line.points.last().unwrap(), line.ends[1])]
        {
            let c = rect.center();
            let (dx, dy) =
                ((tip.x - c.x) / (rect.width / 2.0), (tip.y - c.y) / (rect.height / 2.0));
            assert!((dx * dx + dy * dy - 1.0).abs() < 0.01, "{tip:?} on {rect:?}");
        }
    }

    #[test]
    fn waypoints_handles_and_previews() {
        let (doc, ids) = order_system();
        let d = ids.class_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edges.iter().find(|e| e.labels.len() == 3).unwrap();
        assert!(edge.waypoints().is_empty());
        let handles = edge.handles();
        assert_eq!(handles.len(), 1, "one segment, one insert grip");
        let (handle, at) = handles[0];
        assert_eq!(edge.handle_at(at, 3.0), Some(handle));
        let waypoints = drag_waypoint(edge.waypoints(), handle, Point::new(300.0, 30.0));
        assert_eq!(waypoints, vec![Point::new(300.0, 30.0)]);
        let moved = drag_waypoint(&waypoints, EdgeHandle::Waypoint(0), Point::new(310.0, 20.0));
        assert_eq!(moved, vec![Point::new(310.0, 20.0)]);

        let route = Route { waypoints, ..edge.route() };
        let preview = Preview { route: Some((edge.id, route)), ..Preview::default() };
        let scene = build(doc.model(), d, &preview, &ApproxMeasure);
        assert_eq!(scene.edge(edge.id).unwrap().points.len(), 3);
    }

    #[test]
    fn waypoints_inside_end_nodes_are_hidden() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edges.iter().find(|e| e.labels.len() == 3).unwrap();
        let (id, [a, b]) = (edge.id, edge.ends);
        // A box grown over a waypoint: the line leaves the box beside it, not from inside.
        let inside = Point::new(a.x + 5.0, a.center().y);
        let outside = Point::new(a.right() + 10.0, a.y - 40.0);
        assert!(!b.contains(outside));
        doc.edit("route", |tx| tx.set_waypoints(d, id, vec![inside, outside])).unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let points = &scene.edge(id).unwrap().points;
        assert_eq!(points.len(), 3);
        assert_eq!(points[0], border_point_from(a, inside, outside));
        assert_eq!(points[1], outside);
    }

    #[test]
    fn line_shapes() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edges.iter().find(|e| e.labels.len() == 3).unwrap();
        let (id, [a, b]) = (edge.id, edge.ends);
        let (from, to) = (a.center(), b.center());
        assert!(from.x != to.x && from.y != to.y, "the sample's ends do not line up");
        // Right angles without waypoints: Astah's corners, segments to drag, no waypoint grips.
        doc.edit("shape", |tx| {
            tx.update_presentation(d, id, |p| p.style.line_shape = LineShape::RightAngle)
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edge(id).unwrap();
        assert_eq!(edge.waypoints(), &route::default_corners(a, from, b, to)[..]);
        assert!(edge.points.windows(2).all(|w| w[0].x == w[1].x || w[0].y == w[1].y));
        assert!(edge.handles().iter().all(|(h, _)| matches!(h, EdgeHandle::Segment(_))));
        // Curves: drawn along the curve, hit along it too.
        doc.edit("shape", |tx| {
            tx.update_presentation(d, id, |p| p.style.line_shape = LineShape::CurveRightAngle)
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edge(id).unwrap();
        let path = edge.path();
        assert!(path.len() > edge.points.len());
        assert_eq!(hit_test(&scene, path[path.len() / 2], 1.0), Some(id));
    }

    #[test]
    fn roles_and_multiplicities_go_to_opposite_sides() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let association = scene.edges.iter().find(|e| e.labels.len() == 3).unwrap();
        // Order (source, right) → Customer (target, left), roughly horizontal.
        let pts = &association.points;
        let texts: Vec<&str> = association.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["*", "+ customer", "1"], "roles show their visibility");
        let [star, role, one] = [0, 1, 2].map(|i| &association.labels[i]);
        // The line runs to the left: roles go left of that direction (below the line),
        // multiplicities right of it (above), at both ends.
        let end = pts[pts.len() - 1];
        assert!(role.anchor.y > end.y, "the role goes below the line");
        assert!(one.anchor.y + LABEL_HEIGHT < end.y, "the multiplicity above it");
        assert!(star.anchor.y + LABEL_HEIGHT < pts[0].y);
        // The multiplicities' centers lie 18 along the line from its ends.
        let center = |l: &EdgeLabel| l.anchor.x + label_width(&l.text, &ApproxMeasure) / 2.0;
        assert!((center(one) - end.x - 18.0).abs() < 0.5);
        assert!((pts[0].x - center(star) - 18.0).abs() < 0.5);
    }

    #[test]
    fn end_constraints_join_the_role() {
        let (mut doc, ids) = order_system();
        doc.edit("Constrain", |tx| {
            tx.update(ids.order_customer, |data| {
                if let ElementKind::Association(p) = &mut data.kind {
                    p.ends[0].constraints = vec!["ordered".into()];
                    p.ends[1].constraints = vec!["ordered".into(), "unique".into()];
                }
            })
        })
        .unwrap();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let association = scene.edges.iter().find(|e| e.labels.len() == 4).unwrap();
        let texts: Vec<&str> = association.labels.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["{ordered}", "*", "+ customer {ordered} {unique}", "1"]);
    }

    #[test]
    fn qualifiers_sit_in_a_box_against_their_node() {
        let (mut doc, ids) = order_system();
        doc.edit("Qualify", |tx| {
            tx.update(ids.order_customer, |data| {
                if let ElementKind::Association(p) = &mut data.kind {
                    p.ends[1].qualifiers = vec![Qualifier { name: "id".into(), type_ref: None }];
                    p.ends[1].is_static = true;
                }
            })
        })
        .unwrap();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let edge = scene.edges.iter().find(|e| !e.qualifiers.is_empty()).unwrap();
        let (rect, rows) = &edge.qualifiers[0];
        let node = edge.ends[1];
        let tip = edge.node_tips[1];
        let end = *edge.points.last().unwrap();
        // One row: the minimal height, the text 10 in, the box centered on where the line met
        // the node and flush with it.
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["id"]);
        assert_eq!(rect.height, 30.0);
        assert!((rows[0].rect.x - rect.x - 10.0).abs() < 1e-3);
        let touches = (rect.right() - node.x).abs() < 1e-3
            || (rect.x - node.right()).abs() < 1e-3
            || (rect.bottom() - node.y).abs() < 1e-3
            || (rect.y - node.bottom()).abs() < 1e-3;
        assert!(touches, "{rect:?} against {node:?}");
        let center = rect.center();
        assert!((center.x - tip.x).abs() < 1e-3 || (center.y - tip.y).abs() < 1e-3);
        // The line ends on the box's border.
        let on_border = (end.x - rect.x).abs() < 1e-3
            || (end.x - rect.right()).abs() < 1e-3
            || (end.y - rect.y).abs() < 1e-3
            || (end.y - rect.bottom()).abs() < 1e-3;
        assert!(on_border, "{end:?} on {rect:?}");
        assert!(hit_test(&scene, center, 0.0) == Some(edge.id));
        // A static end's role is underlined.
        let role = edge.labels.iter().find(|l| l.text.ends_with("customer")).unwrap();
        assert!(role.underline);
    }

    /// Measures every text as `0` wide, exactly and in whole pixels.
    struct Fixed(f32);

    impl TextMeasure for Fixed {
        fn width(&self, _: &str, _: f32, _: bool) -> f32 {
            self.0
        }

        fn rounded_width(&self, _: &str, _: f32, _: bool) -> f32 {
            self.0
        }
    }

    #[test]
    fn end_labels_go_where_astah_puts_them() {
        // A line Astah drew from (456.68, 335.15) to (720, 325.93) and where it put the labels
        // (top left corners), with their widths.
        let (from, to) = (Point::new(456.68, 335.15), Point::new(720.0, 325.93));
        let source = End { tip: from, next: to, source: true };
        let target = End { tip: to, next: from, source: false };
        let cases = [
            (source, true, 28.0, Point::new(469.03, 308.67)),
            (source, false, 10.0, Point::new(470.30, 344.96)),
            (target, true, 31.0, Point::new(676.89, 301.34)),
            (target, false, 18.01, Point::new(693.63, 337.00)),
        ];
        for (end, role, width, expected) in cases {
            let label = end_label(String::new(), end, role, &Fixed(width));
            assert_eq!(label.align, LabelAlign::Left);
            assert!(label.anchor.distance(expected) < 0.05, "{:?} vs {expected:?}", label.anchor);
        }
        // Labels are at least 10 wide.
        assert_eq!(label_width("1", &Fixed(6.67)), 10.0);
        // Names 9 below the middle, «include» right above it (Astah 12, probe through its API
        // and its SVG), centered by the label's width.
        let middle = Point::new(300.0, 325.0);
        let name = association_name("e".into(), middle, &Fixed(6.67));
        assert!(name.anchor.distance(Point::new(295.0, 326.45312)) < 0.01, "{:?}", name.anchor);
        let across = (Point::new(250.0, 325.0), Point::new(350.0, 325.0));
        let keyword = keyword_label("<<include>>".into(), across, None, &Fixed(68.0));
        assert!(keyword.anchor.distance(Point::new(266.0, 309.90625)) < 0.01);
    }

    #[test]
    fn keywords_stand_beside_their_line() {
        let measure = Fixed(68.0);
        let label = |a: Point, b: Point| keyword_label("<<extend>>".into(), (a, b), None, &measure);
        let area = |l: &EdgeLabel| Rect::new(l.anchor.x, l.anchor.y, 68.0, LABEL_HEIGHT);
        // Up or down: to the right, 4 away, centered on the middle's height.
        for (a, b) in [((100.0, 0.0), (100.0, 200.0)), ((100.0, 200.0), (100.0, 0.0))] {
            let k = label(Point::new(a.0, a.1), Point::new(b.0, b.1));
            assert_eq!(k.anchor, Point::new(104.0, 100.0 - LABEL_HEIGHT / 2.0), "{a:?}");
            assert!(k.keyword);
        }
        // Slanted either way: clear of the line.
        for (a, b) in [((0.0, 0.0), (100.0, 200.0)), ((0.0, 200.0), (100.0, 0.0))] {
            let (a, b) = (Point::new(a.0, a.1), Point::new(b.0, b.1));
            let r = area(&label(a, b));
            let crosses = (0..=100).any(|i| {
                let t = i as f32 / 100.0;
                r.contains(Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t))
            });
            assert!(!crosses, "{r:?}");
        }
        // Placed by hand: relative to the middle.
        let placed = keyword_label(
            "<<extend>>".into(),
            (Point::new(0.0, 0.0), Point::new(0.0, 100.0)),
            Some(Point::new(-80.0, 10.0)),
            &measure,
        );
        assert_eq!(placed.anchor, Point::new(-80.0, 60.0));
    }

    /// Sizes measured in an SVG export of Astah 12.
    #[test]
    fn classifier_boxes_use_astahs_metrics() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let customer = scene.nodes.iter().find(|n| n.element == Some(ids.customer)).unwrap();
        let Figure::Classifier { separators, .. } = &customer.figure else { panic!() };
        let (top, x) = (customer.rect.y, customer.rect.x);
        // Header: 4 px, one 15.09 px line, 4 px; then the attributes the same way; operations empty.
        assert!((separators[0] - (top + 23.09375)).abs() < 1e-3);
        assert!((separators[1] - (separators[0] + 23.09375)).abs() < 1e-3);
        let name = &customer.rows[0];
        assert_eq!((name.size, name.bold, name.rect.y), (12.0, true, top + 4.0));
        let attribute = &customer.rows[1];
        let row_top = separators[0] + 4.0;
        assert_eq!((attribute.rect.x, attribute.rect.y), (x + 10.0, row_top));
        assert!((attribute.rect.height - 15.09375).abs() < 1e-4);
    }

    #[test]
    fn class_diagram_frames() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let frame = Presentation::node(Some(d), Rect::new(10.0, 10.0, 50.0, 50.0));
        let id = frame.id;
        doc.edit("frame", |tx| {
            tx.apply(umlcraft_model::Op::InsertPresentation {
                diagram: d,
                index: 0,
                presentation: frame,
            })
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = scene.node(id).unwrap();
        let Figure::Frame { tab } = node.figure else { panic!("a frame") };
        // 15 beyond the other nodes.
        let content =
            scene.nodes.iter().filter(|n| n.id != id).map(|n| n.rect).reduce(|a, b| a.union(&b));
        let content = content.unwrap();
        assert_eq!(
            (node.rect.right(), node.rect.bottom()),
            (content.right() + 15.0, content.bottom() + 15.0)
        );
        // "pkg" in bold, the package name at 33, the tab ending 10 after it.
        let texts: Vec<&str> = node.rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["pkg", "domain"]);
        assert!(node.rows[0].bold);
        assert_eq!(node.rows[1].rect.x, 43.0);
        let name = ApproxMeasure.rounded_width("domain", 12.0, false);
        assert_eq!(tab.width, 43.0 + name.max(ApproxMeasure.width("domain", 12.0, false)));
        // Clicks inside reach what it surrounds.
        let inside = scene.nodes.iter().find(|n| n.id != id).unwrap().rect.center();
        assert_ne!(hit_test(&scene, inside, 1.0), Some(id));
    }

    #[test]
    fn automatic_sizes_follow_astah() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let view = doc.model().view(d).unwrap();
        let p = view.presentations_of(ids.customer).next().unwrap().clone();
        assert_eq!(fitted_size(doc.model(), &p, &ApproxMeasure), None, "not sized automatically");
        let fitted = |doc: &umlcraft_model::Document| {
            let p = doc.model().view(d).unwrap().get(p.id).unwrap().clone();
            fitted_size(doc.model(), &p, &ApproxMeasure).unwrap()
        };
        doc.edit("auto", |tx| tx.update_presentation(d, p.id, |p| p.style.auto_size = true))
            .unwrap();
        // The widest text (its whole-pixel width, if larger) plus 10 on each side; the content
        // plus 3 high.
        let rows = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = rows.node(p.id).unwrap();
        let widest = node
            .rows
            .iter()
            .map(|r| ApproxMeasure.astah_width(&r.text, r.size, r.bold))
            .fold(0.0, f32::max);
        let size = fitted(&doc);
        assert!((size.width - (widest + 20.0)).abs() < 1e-3);
        let content = 23.09375 + (8.0 + node.rows.len() as f32 * 15.09375 - 15.09375) + 8.0;
        assert!((size.height - (content + 3.0)).abs() < 1e-3, "{size:?}");
        // An empty class is Astah's smallest box, 40 × 42.09.
        doc.edit("rename", |tx| {
            tx.rename(ids.customer, "A")?;
            for child in tx.model().get(ids.customer).unwrap().children().to_vec() {
                tx.delete(child)?;
            }
            Ok(())
        })
        .unwrap();
        let size = fitted(&doc);
        assert_eq!((size.width, size.height), (40.0, 42.09375));
    }

    #[test]
    fn long_members_wrap_and_keep_the_box_width() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        doc.edit("long", |tx| {
            crate::member::apply_member_text(
                tx,
                doc_operation(tx.model(), ids.order),
                "+ submit(first : String, second : String, third : String) : Boolean",
            )
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let order = scene.nodes.iter().find(|n| n.element == Some(ids.order)).unwrap();
        let stored = doc.model().view(d).unwrap().presentations_of(ids.order).next().unwrap();
        assert_eq!(order.rect.width, stored.bounds().unwrap().width);
        let operation = doc_operation(doc.model(), ids.order);
        let lines: Vec<&Row> = order.rows.iter().filter(|r| r.element == Some(operation)).collect();
        assert!(lines.len() >= 2, "wrapped into {} lines", lines.len());
        assert!(lines[0].text.starts_with("+ submit(first"), "{:?}", lines[0].text);
        assert!(lines.iter().all(|l| l.rect.x == order.rect.x + 10.0));
    }

    #[test]
    fn template_parameters_sit_in_a_box_on_the_top_right_corner() {
        use umlcraft_model::{ElementData, TemplateParameterProps};
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let parameters = doc
            .edit("templates", |tx| {
                let mut created = Vec::new();
                for name in ["T1", "T2"] {
                    let kind = ElementKind::TemplateParameter(TemplateParameterProps::default());
                    created.push(tx.create(ids.order, ElementData::new(name, kind))?);
                }
                Ok(created)
            })
            .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let order = scene.nodes.iter().find(|n| n.element == Some(ids.order)).unwrap();
        let Figure::Classifier { template: Some(t), .. } = order.figure else { panic!() };
        // As in Astah's `Pair<T1, T2>`: the text plus 4 on each side, one line plus 4 high, the
        // bottom 5 below the class's top, a fifth sticking out.
        let text_w = ApproxMeasure.width("T1, T2", FONT_SIZE, false);
        assert!((t.width - (text_w + 8.0)).abs() < 1e-3, "{t:?}");
        assert!((t.height - (line_height(FONT_SIZE) + 4.0)).abs() < 1e-3);
        assert!((t.bottom() - (order.rect.y + 5.0)).abs() < 1e-3);
        assert!((t.x - (order.rect.right() - 0.8 * t.width)).abs() < 1e-3);
        let rows: Vec<&Row> =
            order.rows.iter().filter(|r| parameters.contains(&r.element.unwrap())).collect();
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), ["T1, ", "T2"]);
        assert_eq!(rows[0].rect.x, t.x + 4.0);
        assert!(
            (rows[1].rect.x - (t.x + 4.0 + ApproxMeasure.width("T1, ", 12.0, false))).abs() < 1e-3
        );
        // The box belongs to the node: it is hit and counts for the drawing's extent.
        let corner = Point::new(t.right() - 1.0, t.y + 1.0);
        assert!(!order.rect.contains(corner) && order.hit(corner, 0.0));
        assert!(scene.bounds().unwrap().contains(corner));
    }

    fn doc_operation(model: &Model, class: Id) -> Id {
        model
            .children(class)
            .iter()
            .copied()
            .find(|&c| matches!(model.get(c).unwrap().kind(), ElementKind::Operation(_)))
            .unwrap()
    }

    #[test]
    fn anchors_and_placed_labels_follow_astah() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let edge = doc.model().shown_on(ids.order_customer)[0].1;
        let order = doc.model().shown_on(ids.order)[0].1;
        doc.edit("astah", |tx| {
            // Put Order right below Customer and aim both ends at x = 120.
            tx.set_bounds(d, order, Rect::new(80.0, 300.0, 160.0, 60.0))?;
            tx.update_presentation(d, edge, |p| {
                if let Shape::Edge { source_anchor, target_anchor, labels, .. } = &mut p.shape {
                    *source_anchor = Some(Point::new(0.25, 0.5));
                    *target_anchor = Some(Point::new(0.25, 0.5));
                    labels.target_role = Some(Point::new(5.0, 6.0));
                }
            })?;
            tx.update(ids.order_customer, |data| {
                if let ElementKind::Association(p) = &mut data.kind {
                    p.ends[0].navigability = Navigability::NonNavigable;
                }
            })
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let e = scene.edge(edge).unwrap();
        // Straight up from Order's top to Customer's bottom at x = 80 + 0.25 · 160.
        assert_eq!(e.points, vec![Point::new(120.0, 300.0), Point::new(120.0, 170.0)]);
        assert_eq!(e.source_marker, EndMarker::Cross);
        let role = e.labels.iter().find(|l| l.text == "+ customer").unwrap();
        assert_eq!((role.anchor, role.align), (Point::new(125.0, 176.0), LabelAlign::Left));

        // A diamond keeps the end's navigability behind it.
        let marker = |doc: &mut umlcraft_model::Document, aggregation, navigability| {
            doc.edit("end", |tx| {
                tx.update(ids.order_customer, |data| {
                    if let ElementKind::Association(p) = &mut data.kind {
                        (p.ends[0].aggregation, p.ends[0].navigability) =
                            (aggregation, navigability);
                    }
                })
            })
            .unwrap();
            let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
            scene.edge(edge).unwrap().source_marker
        };
        let cases = [
            (
                Aggregation::Composite,
                Navigability::NonNavigable,
                EndMarker::DiamondCross { filled: true },
            ),
            (
                Aggregation::Shared,
                Navigability::NonNavigable,
                EndMarker::DiamondCross { filled: false },
            ),
            (
                Aggregation::Composite,
                Navigability::Navigable,
                EndMarker::DiamondArrow { filled: true },
            ),
            (Aggregation::Shared, Navigability::Unspecified, EndMarker::HollowDiamond),
            (Aggregation::None, Navigability::Navigable, EndMarker::OpenArrow),
        ];
        for (aggregation, navigability, expected) in cases {
            assert_eq!(marker(&mut doc, aggregation, navigability), expected, "{aggregation:?}");
        }
    }

    #[test]
    fn free_labels_move_off_their_line_and_placed_ones_stay() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let edge = doc.model().shown_on(ids.order_customer)[0].1;
        let order = doc.model().shown_on(ids.order)[0].1;
        doc.edit("vertical", |tx| {
            tx.set_bounds(d, order, Rect::new(80.0, 300.0, 160.0, 60.0))?;
            tx.update_presentation(d, edge, |p| {
                if let Shape::Edge { source_anchor, target_anchor, labels, .. } = &mut p.shape {
                    *source_anchor = Some(Point::new(0.25, 0.5));
                    *target_anchor = Some(Point::new(0.25, 0.5));
                    labels.target_role = Some(Point::new(5.0, 6.0));
                }
            })?;
            tx.update(ids.order_customer, |data| data.name = "places".into())
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let e = scene.edge(edge).unwrap();
        // Astah centers the name on the line, which runs up at x = 120; it moves beside it.
        let name = e.labels.iter().find(|l| l.text == "places").unwrap();
        let width = label_width("places", &ApproxMeasure);
        assert!(name.anchor.x > 120.0 || name.anchor.x + width < 120.0, "{name:?}");
        let free = e.free_labels.iter().find(|f| f.kind == LabelKind::Name).unwrap();
        assert!(free.moved);
        // The role placed by hand is not free and stays.
        assert!(!e.free_labels.iter().any(|f| f.kind == LabelKind::Role { source: false }));
        let role = e.labels.iter().find(|l| l.text == "+ customer").unwrap();
        assert_eq!(role.anchor, Point::new(125.0, 176.0));
    }

    #[test]
    fn dragging_moves_node_and_attached_edges() {
        let (doc, ids) = order_system();
        let node = doc.model().shown_on(ids.premium)[0].1;
        let preview = Preview { moved: HashSet::from([node]), dy: 40.0, ..Preview::default() };
        let scene = build(doc.model(), ids.class_diagram, &preview, &ApproxMeasure);
        let moved = scene.nodes.iter().find(|n| n.id == node).unwrap();
        assert_eq!(moved.rect.y, 300.0);
        assert!(hit_test(&scene, Point::new(100.0, 310.0), 4.0) == Some(node));
        assert!(hit_test(&scene, Point::new(600.0, 600.0), 4.0).is_none());
    }

    #[test]
    fn handles_resize_and_respect_minimum() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert_eq!(Handle::at(r, Point::new(111.0, 59.0), 4.0), Some(Handle::BottomRight));
        assert_eq!(Handle::at(r, Point::new(60.0, 10.0), 4.0), Some(Handle::Top));
        assert_eq!(Handle::at(r, Point::new(60.0, 35.0), 4.0), None);
        assert_eq!(
            Handle::BottomRight.resize(r, 20.0, 10.0, 20.0),
            Rect::new(10.0, 10.0, 120.0, 60.0)
        );
        assert_eq!(Handle::TopLeft.resize(r, -5.0, 100.0, 20.0), Rect::new(5.0, 40.0, 105.0, 20.0));
        assert_eq!(Handle::Top.resize(r, 30.0, 0.0, 20.0), r, "edge handles ignore the other axis");
    }

    #[test]
    fn rubber_band_and_alignment() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        assert_eq!(nodes_in(&scene, Rect::new(0.0, 0.0, 300.0, 400.0)).len(), 2);
        let rects = [Rect::new(0.0, 0.0, 100.0, 50.0), Rect::new(200.0, 80.0, 60.0, 30.0)];
        assert_eq!(align(&rects, Alignment::Right)[1], Rect::new(40.0, 80.0, 60.0, 30.0));
        assert_eq!(align(&rects, Alignment::VerticalCenter)[1], Rect::new(200.0, 10.0, 60.0, 30.0));
        assert_eq!(align(&rects, Alignment::Top)[0], rects[0]);
    }

    #[test]
    fn activity_diagram_scene() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.activity_diagram, &Preview::default(), &ApproxMeasure);
        let figure = |name: &str| {
            scene
                .nodes
                .iter()
                .find(|n| n.rows.first().is_some_and(|r| r.text == name))
                .map(|n| &n.figure)
        };
        assert!(matches!(figure("Check stock"), Some(Figure::Action)));
        assert!(matches!(figure("Clerk"), Some(Figure::Partition { .. })));
        // Control nodes show no name.
        let initial = scene.nodes.iter().find(|n| matches!(n.figure, Figure::Initial)).unwrap();
        assert!(initial.rows.is_empty());
        assert_eq!(initial.rect, Rect::new(140.0, 100.0, 20.0, 20.0));

        assert_eq!(scene.edges.len(), 6);
        assert!(scene.edges.iter().all(|e| e.target_marker == EndMarker::OpenArrow));
        let guards: Vec<&str> =
            scene.edges.iter().flat_map(|e| &e.labels).map(|l| l.text.as_str()).collect();
        assert_eq!(guards, vec!["[in stock]", "[else]"]);
        // The flow from the initial node starts on its circle, straight below the center.
        let first = scene.edges.iter().find(|e| e.points[0].y < 125.0).unwrap();
        assert!((first.points[0].x - 150.0).abs() < 0.01, "{:?}", first.points);
        assert!((first.points[0].y - 120.0).abs() < 0.01, "{:?}", first.points);
    }

    #[test]
    fn object_nodes_show_their_state_below_the_name() {
        let (mut doc, ids) = order_system();
        let d = ids.activity_diagram;
        doc.edit("object", |tx| {
            let activity = tx.model().get(d).unwrap().owner().unwrap();
            let kind = ElementKind::ActivityNode(ActivityNodeProps {
                node: ActivityNodeKind::Object,
                type_ref: Some(TypeRef::Element(ids.order)),
                in_state: "filled".into(),
            });
            let object = tx.create(activity, ElementData::new("o", kind))?;
            tx.add_node(d, Some(object), Rect::new(500.0, 100.0, 100.0, 20.0))
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = scene.nodes.iter().find(|n| n.rows.len() == 2).unwrap();
        let texts: Vec<&str> = node.rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["o : Order", "[filled]"]);
        // The state is a line of its own, centered, and not the node's name.
        assert_eq!(node.rows[1].element, None);
        assert!(node.rows[1].rect.y >= node.rows[0].rect.bottom() - 0.01);
        assert!(node.rect.height >= 2.0 * line_height(FONT_SIZE));
        assert!((node.rows[0].rect.center().x - node.rows[1].rect.center().x).abs() < 0.01);
    }

    #[test]
    fn state_machine_scene() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.state_diagram, &Preview::default(), &ApproxMeasure);
        let node = |name: &str| {
            scene.nodes.iter().find(|n| n.rows.first().is_some_and(|r| r.text == name)).unwrap()
        };
        // A composite state heads its substates with its name; a state with an entry action
        // lists it below a line.
        let open = node("Open");
        let Figure::State { separator: Some(line), .. } = open.figure else { panic!() };
        assert!(open.rows[0].rect.bottom() <= line, "name above the line");
        let submitted = node("Submitted");
        assert!(matches!(submitted.figure, Figure::State { separator: Some(_), .. }));
        assert_eq!(submitted.rows[1].text, "entry / check stock");
        // A plain state, too, shows its name above a line.
        let shipped = node("Shipped");
        let Figure::State { separator: Some(line), .. } = shipped.figure else { panic!() };
        assert!(shipped.rows[0].rect.bottom() <= line);

        let labels: Vec<&str> =
            scene.edges.iter().flat_map(|e| &e.labels).map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"[in stock] / ship"), "{labels:?}");
        assert_eq!(labels.iter().filter(|&&l| l == "add item").count(), 2);
        // Moving the composite state moves its substates.
        let moved = with_contents(doc.model(), &scene, &[open.id]);
        assert_eq!(moved.len(), 4, "the state, its initial pseudostate and two substates");
    }

    #[test]
    fn substates_reaching_out_of_their_state_go_along() {
        let (mut doc, ids) = order_system();
        let d = ids.state_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        fn node(scene: &Scene, name: &str) -> (Id, Rect) {
            let found = scene.nodes.iter().find(|n| n.rows.first().is_some_and(|r| r.text == name));
            found.map(|n| (n.id, n.rect)).unwrap()
        }
        let ((open, outline), (empty, rect)) = (node(&scene, "Open"), node(&scene, "Empty"));
        // Half out of its state, its center still inside: still a substate.
        let x = outline.right() - rect.width * 0.75;
        let bounds = doc.model().view(d).unwrap().get(empty).unwrap().bounds().unwrap();
        doc.edit("out", |tx| tx.set_bounds(d, empty, Rect { x, ..bounds })).unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        assert!(!outline.contains_rect(&node(&scene, "Empty").1));
        assert!(with_contents(doc.model(), &scene, &[open]).contains(&empty));
    }

    #[test]
    fn orthogonal_states_and_internal_transitions() {
        let (mut doc, ids) = order_system();
        let named = |model: &Model, name: &str| {
            model.elements().find(|e| e.name() == name).map(|e| e.id()).unwrap()
        };
        let (open, submitted) = (named(doc.model(), "Open"), named(doc.model(), "Submitted"));
        doc.edit("regions", |tx| {
            crate::state_edit::add_region(tx, open, false, None)?;
            crate::state_edit::add_internal_transition(tx, submitted, "retry")?;
            tx.update(submitted, |d| d.name = "Submitted".into())
        })
        .unwrap();
        let scene = build(doc.model(), ids.state_diagram, &Preview::default(), &ApproxMeasure);
        let node = |name: &str| {
            scene.nodes.iter().find(|n| n.rows.first().is_some_and(|r| r.text == name)).unwrap()
        };
        let open = node("Open");
        let Figure::State { separator: Some(name_line), regions, vertical: false } = &open.figure
        else {
            panic!()
        };
        // One below the other, the first one including the name; the room below the name is
        // shared evenly.
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].rect.y, open.rect.y);
        let below = open.rect.bottom() - name_line;
        assert!((regions[1].rect.y - (name_line + below / 2.0)).abs() < 0.01);
        assert_eq!(regions[1].rect.bottom(), open.rect.bottom());
        let dashed = crate::draw::figure(open)
            .into_iter()
            .filter(|p| {
                matches!(p, crate::draw::Primitive::Polyline { style: LineStyle::ShortDashed, .. })
            })
            .count();
        assert_eq!(dashed, 1, "a dashed line between the regions");
        let hit = region_line_at(&scene, Point::new(200.0, regions[1].rect.y + 1.0), 3.0);
        assert!(hit.is_some_and(|(n, index, vertical)| n.id == open.id && index == 0 && !vertical));

        // The internal transition is listed below the entry action and edits its element.
        let submitted = node("Submitted");
        assert_eq!(submitted.rows[2].text, "retry");
        assert_eq!(submitted.rows[2].role, RowRole::Member);
        assert_ne!(submitted.rows[2].element, submitted.element);
    }

    #[test]
    fn region_lines_keep_their_order_and_room() {
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        let ids = [Id::from_u128(1), Id::from_u128(2), Id::from_u128(3)];
        let layout =
            |vertical, separators: &[f32]| Regions { vertical, separators: separators.to_vec() };
        // Lines out of order or too close move apart; missing ones share what is left.
        let shapes = region_shapes(r, 20.0, &layout(true, &[150.0, 10.0]), &ids);
        let xs: Vec<f32> = shapes.iter().map(|s| s.rect.x).collect();
        assert_eq!(xs, vec![0.0, 150.0, 170.0]);
        let shapes = region_shapes(r, 20.0, &layout(false, &[]), &ids[..2]);
        assert_eq!(shapes[1].rect.y, 60.0);
        assert_eq!(region_separators(r, &shapes, false), vec![60.0]);
        assert!(region_shapes(r, 20.0, &layout(false, &[]), &ids[..1]).is_empty());
    }

    #[test]
    fn components_nodes_and_instances() {
        let (doc, ids) = order_system();
        let model = doc.model();
        let scene = build(model, ids.component_diagram, &Preview::default(), &ApproxMeasure);
        fn node<'s>(scene: &'s Scene, name: &str) -> &'s NodeShape {
            scene.nodes.iter().find(|n| n.rows.iter().any(|r| r.text == name)).unwrap()
        }
        fn figure<'s>(scene: &'s Scene, name: &str) -> &'s Figure {
            &node(scene, name).figure
        }
        assert!(matches!(figure(&scene, "Shop"), Figure::Component { .. }));
        assert!(matches!(figure(&scene, "shop.jar"), Figure::Artifact { .. }));
        // Astah: regular names, rows from 4 below the top, the icon 6 below it.
        let shop = node(&scene, "Shop");
        let row = shop.rows.iter().find(|r| r.text == "Shop").unwrap();
        assert!(!row.bold);
        let Figure::Component { icon } = shop.figure else { unreachable!() };
        assert_eq!((icon.y - shop.rect.y, shop.rect.right() - icon.x), (6.0, 20.0));
        let labels: Vec<&str> =
            scene.edges.iter().flat_map(|e| &e.labels).map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"<<use>>") && labels.contains(&"<<manifest>>"), "{labels:?}");

        let scene = build(model, ids.deployment_diagram, &Preview::default(), &ApproxMeasure);
        assert!(matches!(figure(&scene, "AppServer"), Figure::Cube { .. }));
        // Instances show their classifier, underlined, in the figure of their kind.
        let web1 = node(&scene, "web1 : AppServer");
        assert!(matches!(web1.figure, Figure::Cube { depth: 4.0 }));
        assert!(web1.rows[0].underline);
        // An instance's row starts at the top of the cube's front face.
        assert_eq!(web1.rows[0].rect.y, web1.rect.y + 4.0);
        assert!(matches!(figure(&scene, ": Shop"), Figure::Component { .. }));
        // A node takes what is deployed on it along.
        assert_eq!(with_contents(model, &scene, &[web1.id]).len(), 2);
    }

    #[test]
    fn edges_end_on_circles_and_diamonds() {
        let r = Rect::new(0.0, 0.0, 20.0, 20.0);
        let center = r.center();
        let p = Outline::Ellipse.border_point(r, center, Point::new(100.0, 100.0));
        assert!((p.distance(center) - 10.0).abs() < 0.01, "{p:?}");
        let p = Outline::Diamond.border_point(r, center, Point::new(100.0, 10.0));
        assert!((p.x - 20.0).abs() < 0.01 && (p.y - 10.0).abs() < 0.01, "{p:?}");
        // On the diagonal the diamond's edge is closer than the circle.
        let p = Outline::Diamond.border_point(r, center, Point::new(100.0, 100.0));
        assert!((p.x - 15.0).abs() < 0.01, "{p:?}");
    }
}
