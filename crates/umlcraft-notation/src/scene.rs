//! Turns a diagram view into drawable shapes in logical coordinates. The scene also carries the
//! layout of every text row, so painting, hit testing and inline editing agree on positions.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationEnd, Color, DiagramKind,
    EdgeLabels, ElementKind, Id, InstanceKind, LineShape, MessageSort, Model, Navigability, Point,
    Presentation, PseudostateKind, Rect, Shape, Size, StateProps, Style,
};

use crate::member::member_text;
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

    /// Bounding box of all nodes as drawn.
    pub fn bounds(&self) -> Option<Rect> {
        self.nodes.iter().map(|n| n.rect).reduce(|a, b| a.union(&b))
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
    /// Whether `p` hits the node. Hollow frames (system boundaries, combined fragments) only
    /// count at their border and texts, so clicks inside reach what they contain or the empty
    /// canvas. Lifelines count at their head and along their dashed line.
    pub fn hit(&self, p: Point, tolerance: f32) -> bool {
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

    /// Whether the user may resize the node. Lifelines, activation bars and diagram frames
    /// are sized by the sequence layout.
    pub fn resizable(&self) -> bool {
        !matches!(self.figure, Figure::Lifeline { .. } | Figure::Activation | Figure::Frame { .. })
    }
}

/// The outline of a node; its texts are in [`NodeShape::rows`].
pub enum Figure {
    /// Box with compartment separators at the given y positions.
    Classifier {
        separators: Vec<f32>,
    },
    /// Interface drawn as a small circle.
    Lollipop,
    Package {
        tab: Rect,
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
    /// from the internal activities or substates.
    State {
        separator: Option<f32>,
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
    Other,
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
            | Self::Lollipop => Outline::Ellipse,
            Self::Diamond => Outline::Diamond,
            _ => Outline::Rect,
        }
    }
}

/// Shapes whose border edges are clipped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outline {
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
    fn border_point(self, rect: Rect, inside: Point, toward: Point) -> Point {
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
}

impl LineStyle {
    /// Lengths of dashes and gaps, in logical units; empty for a solid line.
    pub fn dashes(self) -> &'static [f32] {
        match self {
            Self::Solid => &[],
            Self::Dashed => &[10.0, 5.0],
            Self::LongDashed => &[12.0, 6.0],
            Self::ShortDashed => &[5.0, 5.0],
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
    /// A small x across the line near the end: the end is not navigable.
    Cross,
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
    pub fn path(&self) -> Vec<Point> {
        if self.shape.is_curve() {
            route::flatten(&route::bezier(&self.points))
        } else {
            self.points.clone()
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
}

/// An uncommitted change shown while the user drags.
#[derive(Clone, Debug, Default)]
pub struct Preview {
    pub moved: HashSet<Id>,
    pub dx: f32,
    pub dy: f32,
    pub resized: Option<(Id, Rect)>,
    pub route: Option<(Id, Route)>,
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
}

impl Line {
    fn new(text: impl Into<String>, size: f32, role: RowRole, element: Option<Id>) -> Self {
        Self {
            text: text.into(),
            size,
            bold: false,
            italic: false,
            underline: false,
            role,
            element,
        }
    }

    fn width(&self, measure: &dyn TextMeasure) -> f32 {
        measure.width(&self.text, self.size, self.bold)
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
        self.words().iter().map(|w| measure.width(w, self.size, self.bold)).fold(0.0, f32::max)
    }

    /// This line, broken at spaces into lines no wider than `max`.
    fn wrapped(self, max: f32, measure: &dyn TextMeasure) -> Vec<Line> {
        if self.width(measure) <= max {
            return vec![self];
        }
        let mut texts = Vec::new();
        let mut current = String::new();
        for word in self.words() {
            let candidate =
                if current.is_empty() { word.clone() } else { format!("{current} {word}") };
            if !current.is_empty() && measure.width(&candidate, self.size, self.bold) > max {
                texts.push(std::mem::replace(&mut current, word));
            } else {
                current = candidate;
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
        (Some(layout), messages)
    } else {
        package_frame(model, diagram, &mut nodes, measure);
        (None, Vec::new())
    };
    let rects: HashMap<Id, Rect> = nodes.iter().map(|n| (n.id, n.rect)).collect();
    let outlines: HashMap<Id, Outline> = nodes.iter().map(|n| (n.id, n.figure.outline())).collect();
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
    let mut edges: Vec<EdgeShape> = view
        .presentations()
        .iter()
        // On sequence diagrams the layout draws the messages.
        .filter(|p| !(is_sequence && is_message(p)))
        .filter_map(|p| {
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
            let label = element.and_then(|e| {
                let stereotypes = e.data().stereotypes.join(", ");
                let parts: Vec<String> = [
                    (!stereotypes.is_empty()).then(|| format!("<<{stereotypes}>>")),
                    (!e.name().is_empty()).then(|| e.name().to_owned()),
                ]
                .into_iter()
                .flatten()
                .collect();
                (!parts.is_empty()).then(|| parts.join(" "))
            });
            let to_lollipop = iconic_interfaces.contains(target);
            let outline = |id| outlines.get(id).copied().unwrap_or(Outline::Rect);
            Some(edge_for(
                p.id,
                element.map(|e| e.kind()),
                ((source_rect, outline(source)), (target_rect, outline(target))),
                anchors,
                p.style.line_shape,
                &points,
                (label, element.is_some_and(|e| !e.name().is_empty())),
                to_lollipop,
                labels,
                measure,
            ))
        })
        .collect();
    edges.extend(messages);
    Scene { nodes, edges, sequence }
}

/// The frame Astah may draw around a class diagram (a node presentation of the diagram
/// itself): "pkg" in bold and the name of the diagram's package in its tab, reaching 15 beyond
/// the other nodes.
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
    // The tab names the package the diagram belongs to.
    let owner = model.get(diagram).and_then(|d| d.owner()).and_then(|o| model.get(o));
    let name = owner.map(|o| o.name()).unwrap_or_default();
    frame.rect = r;
    (frame.figure, frame.rows) = sequence::frame_with_tab("pkg", 33.0, name, r, diagram, measure);
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
    let (rect, figure, rows) = match element {
        Some(e) => layout_element(model, e.id(), &p.style, bounds, measure),
        None => (bounds, Figure::Other, Vec::new()),
    };
    NodeShape { id: p.id, element: p.element, rect, figure, rows, fill: p.style.fill }
}

fn layout_element(
    model: &Model,
    id: Id,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    let e = model.get(id).expect("caller checked");
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
            layout_classifier(model, id, style, bounds, false, measure)
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
            (r, Figure::Package { tab }, vec![row])
        }
        ElementKind::Actor(_) => {
            let r = Rect::new(bounds.x, bounds.y, bounds.width.max(30.0), bounds.height.max(60.0));
            let line = name_line(12.0, false);
            let w = line.width(measure).max(r.width) + 8.0;
            let row = line.place(
                Rect::new(r.center().x - w / 2.0, r.bottom() + 4.0, w, line_height(12.0)),
                LabelAlign::Center,
            );
            (r, Figure::Actor, vec![row])
        }
        ElementKind::UseCase(_) => layout_use_case(model, id, bounds, measure),
        ElementKind::SystemBoundary => {
            let line = name_line(13.0, true);
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(line.width(measure) + 20.0).max(120.0),
                bounds.height.max(80.0),
            );
            let row = line
                .place(Rect::new(r.x, r.y + 6.0, r.width, line_height(13.0)), LabelAlign::Center);
            (r, Figure::Boundary, vec![row])
        }
        // The sequence layout places lifelines.
        ElementKind::Lifeline(_) => (bounds, Figure::Other, Vec::new()),
        ElementKind::ActivityNode(p) => layout_activity_node(model, id, p, bounds, measure),
        ElementKind::State(p) => layout_state(model, id, p, bounds, measure),
        ElementKind::Component(_) | ElementKind::Artifact(_) | ElementKind::Node(_) => {
            layout_box(model, id, e.kind(), None, bounds, measure)
        }
        ElementKind::Instance(p) => {
            let figure = match p.instance {
                InstanceKind::Object => ElementKind::Class(Default::default()),
                InstanceKind::Component => ElementKind::Component(Default::default()),
                InstanceKind::Node => ElementKind::Node(Default::default()),
            };
            layout_box(model, id, &figure, Some(p.classifier), bounds, measure)
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
            // Astah starts the text 2 from the top left corner.
            let h = line_height(12.0);
            let text_w = lines.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(text_w + 4.0),
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
    id: Id,
    props: &ActivityNodeProps,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD_X: f32 = 10.0;
    const PAD_Y: f32 = 4.0;
    let e = model.get(id).expect("caller checked");
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
    let h = line_height(FONT_SIZE);
    let widest = names.iter().map(|n| measure.width(n, FONT_SIZE, false)).fold(0.0, f32::max);
    let block = h * names.len() as f32;
    let r = Rect::new(
        bounds.x,
        bounds.y,
        width.max(widest + 2.0 * PAD_X),
        bounds.height.max(block + 2.0 * PAD_Y),
    );
    let top = r.center().y - block / 2.0;
    let rows = names
        .into_iter()
        .enumerate()
        .map(|(i, n)| {
            Line::new(n, FONT_SIZE, RowRole::Name, Some(id))
                .place(Rect::new(r.x, top + h * i as f32, r.width, h), LabelAlign::Center)
        })
        .collect();
    (r, figure, rows)
}

/// States: the name, centered; below a line the internal activities (`entry / …`), or room for
/// the substates of a composite state.
/// Depth of a deployment node's cube.
const CUBE_DEPTH: f32 = 12.0;

/// Components, artifacts, nodes and instances: a box (or cube) with stereotypes and the name at
/// the top. `instance_of` is set for instances, which show `name : Type` underlined; `figure`
/// then is the kind of element they are an instance of.
fn layout_box(
    model: &Model,
    id: Id,
    figure: &ElementKind,
    instance_of: Option<Option<Id>>,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD: f32 = 6.0;
    const ICON: Size = Size { width: 12.0, height: 16.0 };
    let e = model.get(id).expect("caller checked");
    let mut lines = Vec::new();
    let stereotypes = &e.data().stereotypes;
    if !stereotypes.is_empty() {
        lines.push(Line::new(
            stereotype_text(stereotypes),
            FONT_SIZE,
            RowRole::Stereotype,
            Some(id),
        ));
    }
    let mut name = match instance_of {
        Some(classifier) => {
            let type_name = classifier.and_then(|c| model.get(c)).map(|c| c.name());
            let text = match type_name {
                Some(t) if e.name().is_empty() => format!(": {t}"),
                Some(t) => format!("{} : {t}", e.name()),
                None => e.name().to_owned(),
            };
            let mut line = Line::new(text, FONT_SIZE, RowRole::Name, Some(id));
            line.underline = true;
            line
        }
        None => {
            let mut line = Line::new(e.name(), FONT_SIZE, RowRole::Name, Some(id));
            line.bold = true;
            line
        }
    };
    name.italic = matches!(figure, ElementKind::Component(p) if p.is_abstract);
    lines.push(name);

    let depth = if matches!(figure, ElementKind::Node(_)) { CUBE_DEPTH } else { 0.0 };
    let has_icon = matches!(figure, ElementKind::Component(_) | ElementKind::Artifact(_));
    let icon_room = if has_icon { ICON.width + PAD } else { 0.0 };
    let widest = lines.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
    let h = line_height(FONT_SIZE);
    let text_h = h * lines.len() as f32;
    let r = Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(widest + 2.0 * (PAD + icon_room) + depth).max(40.0),
        bounds.height.max(text_h + 2.0 * PAD + depth).max(30.0),
    );
    // The front face of a cube lies below its top and left of its side.
    let front = Rect::new(r.x, r.y + depth, r.width - depth, r.height - depth);
    let rows = lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let y = front.y + PAD + h * i as f32;
            line.place(Rect::new(front.x, y, front.width, h), LabelAlign::Center)
        })
        .collect();
    let icon = Rect::new(front.right() - PAD - ICON.width, front.y + PAD, ICON.width, ICON.height);
    let figure = match figure {
        ElementKind::Component(_) => Figure::Component { icon },
        ElementKind::Artifact(_) => Figure::Artifact { icon },
        ElementKind::Node(_) => Figure::Cube { depth },
        _ => Figure::Rectangle,
    };
    (r, figure, rows)
}

fn layout_state(
    model: &Model,
    id: Id,
    props: &StateProps,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD_X: f32 = 8.0;
    const PAD_Y: f32 = 4.0;
    let e = model.get(id).expect("caller checked");
    let composite =
        e.children().iter().any(|&c| model.get(c).is_some_and(|c| c.kind().is_vertex()));
    let activities: Vec<Line> =
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
    let name = Line::new(e.name(), FONT_SIZE, RowRole::Name, Some(id));
    let h = line_height(FONT_SIZE);
    let widest = activities.iter().chain([&name]).map(|l| l.width(measure)).fold(0.0, f32::max);
    let header = h + 2.0 * PAD_Y;
    let body = if activities.is_empty() { 0.0 } else { h * activities.len() as f32 + 2.0 * PAD_Y };
    let r = Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(widest + 2.0 * PAD_X).max(40.0),
        bounds.height.max(header + body).max(30.0),
    );
    // A plain state shows its name in the middle; otherwise the name heads the state.
    let divided = composite || !activities.is_empty();
    let name_y = if divided { r.y + PAD_Y } else { r.center().y - h / 2.0 };
    let mut rows = vec![name.place(Rect::new(r.x, name_y, r.width, h), LabelAlign::Center)];
    let separator = divided.then_some(r.y + header);
    for (i, line) in activities.into_iter().enumerate() {
        let y = r.y + header + PAD_Y + h * i as f32;
        rows.push(
            line.place(Rect::new(r.x + PAD_X, y, r.width - 2.0 * PAD_X, h), LabelAlign::Left),
        );
    }
    (r, Figure::State { separator }, rows)
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

/// Use case ellipse: the wrapped name and, below a line, its extension points.
fn layout_use_case(
    model: &Model,
    id: Id,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    /// Share of the ellipse's width and height that text may use.
    const INNER: f32 = 0.72;
    let e = model.get(id).expect("caller checked");
    let extension_points: Vec<(Id, &str)> = e
        .children()
        .iter()
        .filter_map(|&c| model.get(c))
        .filter(|c| matches!(c.kind(), ElementKind::ExtensionPoint))
        .map(|c| (c.id(), c.name()))
        .collect();

    let width = bounds.width.max(80.0);
    let names = wrap(e.name(), width * INNER, 12.0, measure);
    let mut lines: Vec<Line> =
        names.into_iter().map(|n| Line::new(n, 12.0, RowRole::Name, Some(id))).collect();
    let name_count = lines.len();
    if !extension_points.is_empty() {
        let mut heading = Line::new("extension points", 10.0, RowRole::Stereotype, Some(id));
        heading.italic = true;
        lines.push(heading);
        lines.extend(
            extension_points
                .iter()
                .map(|&(ep, name)| Line::new(name, 11.0, RowRole::Member, Some(ep))),
        );
    }
    let gap = if extension_points.is_empty() { 0.0 } else { 4.0 };
    let block_h: f32 = lines.iter().map(|l| line_height(l.size)).sum::<f32>() + gap;
    let widest = lines.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
    let r = Rect::new(
        bounds.x,
        bounds.y,
        width.max(widest / INNER + 10.0),
        bounds.height.max(40.0).max(block_h / INNER + 10.0),
    );

    let mut y = r.center().y - block_h / 2.0;
    let mut separator = None;
    let mut rows = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        if i == name_count {
            separator = Some(y + gap / 2.0);
            y += gap;
        }
        let h = line_height(line.size);
        rows.push(line.place(Rect::new(r.x, y, r.width, h), LabelAlign::Center));
        y += h;
    }
    (r, Figure::UseCase { separator }, rows)
}

/// Nodes drawn completely inside container nodes (system boundaries, packages) among `ids`,
/// recursively, together with `ids` themselves. Moving a container moves its contents.
pub fn with_contents(scene: &Scene, ids: &[Id]) -> Vec<Id> {
    let mut result: Vec<Id> = ids.to_vec();
    let mut i = 0;
    while i < result.len() {
        if let Some(container) = scene.node(result[i])
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
            for n in &scene.nodes {
                if n.id != container.id
                    && !result.contains(&n.id)
                    && container.rect.contains_rect(&n.rect)
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
    let (r, _, rows) = layout_classifier(model, element.id(), &p.style, bounds, true, measure);
    let texts: Vec<&str> = rows.iter().map(|row| row.text.as_str()).collect();
    Some((r.size(), texts.join("\n")))
}

/// Radius of an interface drawn as a circle.
pub const LOLLIPOP_RADIUS: f32 = 10.0;

/// Astah's narrowest classifier box.
const MIN_CLASSIFIER_WIDTH: f32 = 40.0;
/// Astah makes automatically sized classifier boxes this much higher than their content.
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
    id: Id,
    style: &Style,
    bounds: Rect,
    fit: bool,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    // Astah's metrics: 10 px left and right of the texts, 4 px above and below the content of
    // each compartment; an empty compartment is 8 px high.
    const PAD_X: f32 = 10.0;
    const PAD_Y: f32 = 4.0;
    let e = model.get(id).expect("caller checked");
    let kind = e.kind();

    let mut stereotypes: Vec<String> = match kind {
        ElementKind::Interface(_) => vec!["interface".into()],
        ElementKind::Enumeration(_) => vec!["enumeration".into()],
        ElementKind::DataType(_) => vec!["dataType".into()],
        _ => Vec::new(),
    };
    stereotypes.extend(e.data().stereotypes.iter().cloned());
    let mut header = Vec::new();
    if !stereotypes.is_empty() {
        header.push(Line::new(
            stereotype_text(&stereotypes),
            FONT_SIZE,
            RowRole::Stereotype,
            Some(id),
        ));
    }
    let mut name = Line::new(e.name(), FONT_SIZE, RowRole::Name, Some(id));
    name.bold = true;
    name.italic = matches!(kind, ElementKind::Class(p) if p.is_abstract);
    header.push(name);

    let members = |pick: fn(&ElementKind) -> bool| -> Vec<Line> {
        e.children()
            .iter()
            .filter_map(|&c| model.get(c))
            .filter(|c| pick(c.kind()))
            .map(|c| {
                let mut line =
                    Line::new(member_text(model, c.id()), FONT_SIZE, RowRole::Member, Some(c.id()));
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
    match kind {
        ElementKind::Enumeration(_) => {
            compartments.push(members(|k| matches!(k, ElementKind::EnumerationLiteral)));
            if !attributes.is_empty() && !style.hide_attributes {
                compartments.push(attributes);
            }
            if !operations.is_empty() && !style.hide_operations {
                compartments.push(operations);
            }
        }
        _ => {
            if !style.hide_attributes {
                compartments.push(attributes);
            }
            if !style.hide_operations {
                compartments.push(operations);
            }
        }
    }

    // Fitted, the box is as wide as its widest text, taken as Astah measures it (Java's
    // whole-pixel width or the exact one, whichever is larger), and 3 higher than its content.
    // Otherwise it keeps its width and only grows for the header and for words that cannot
    // break; longer members continue on further lines, as in Astah.
    let width = if fit {
        let widest = header
            .iter()
            .chain(compartments.iter().flatten())
            .map(|l| l.width(measure).max(measure.rounded_width(&l.text, l.size, l.bold)))
            .fold(0.0, f32::max);
        widest + 2.0 * PAD_X
    } else {
        let header_w = header.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
        let word_w =
            compartments.iter().flatten().map(|l| l.widest_word(measure)).fold(0.0, f32::max);
        bounds.width.max(header_w.max(word_w) + 2.0 * PAD_X)
    }
    .max(MIN_CLASSIFIER_WIDTH);
    let compartments: Vec<Vec<Line>> = compartments
        .into_iter()
        .map(|c| c.into_iter().flat_map(|l| l.wrapped(width - 2.0 * PAD_X, measure)).collect())
        .collect();
    let block =
        |lines: &[Line]| 2.0 * PAD_Y + lines.iter().map(|l| line_height(l.size)).sum::<f32>();
    let header_h = block(&header);
    let content_h = header_h + compartments.iter().map(|c| block(c)).sum::<f32>();
    let height =
        if fit { content_h + AUTO_SIZE_EXTRA_HEIGHT } else { bounds.height.max(content_h) };
    let r = Rect::new(bounds.x, bounds.y, width, height);

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
    (r, Figure::Classifier { separators }, rows)
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
    ((source, source_outline), (target, target_outline)): ((Rect, Outline), (Rect, Outline)),
    anchors: [Option<Point>; 2],
    shape: LineShape,
    waypoints: &[Point],
    (name_label, has_name): (Option<String>, bool),
    to_lollipop: bool,
    placed: &EdgeLabels,
    measure: &dyn TextMeasure,
) -> EdgeShape {
    // The line runs from the source anchor through the waypoints to the target anchor; it is
    // drawn from where it leaves the source to where it enters the target. Right-angled lines
    // keep their first and last segment straight as the ends move.
    let (from, to) = (anchor_point(source, anchors[0]), anchor_point(target, anchors[1]));
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

    let mut edge = EdgeShape {
        id,
        points,
        line: LineStyle::Solid,
        shape,
        source_marker: EndMarker::None,
        target_marker: EndMarker::None,
        labels: Vec::new(),
        fixed: false,
        ends: [source, target],
        anchors,
        name_arrow: None,
    };
    let mut ends: [(Option<String>, Option<String>); 2] = [(None, None), (None, None)];
    let mut label = name_label;
    let end_marker = |end: &AssociationEnd| match (end.aggregation, end.navigability) {
        (Aggregation::Shared, _) => EndMarker::HollowDiamond,
        (Aggregation::Composite, _) => EndMarker::FilledDiamond,
        (Aggregation::None, Navigability::Navigable) => EndMarker::OpenArrow,
        (Aggregation::None, Navigability::NonNavigable) => EndMarker::Cross,
        (Aggregation::None, Navigability::Unspecified) => EndMarker::None,
    };
    match kind {
        Some(ElementKind::Association(p)) => {
            let [a, b] = &p.ends;
            edge.source_marker = end_marker(a);
            edge.target_marker = end_marker(b);
            // Role names show their visibility, as Astah does: "- customer". Constraints follow
            // in the same label: "- customer {ordered}".
            let texts = |end: &AssociationEnd| {
                let role = (!end.role.is_empty())
                    .then(|| format!("{} {}", end.visibility.symbol(), end.role));
                let constraints = end.constraints.iter().map(|c| format!("{{{c}}}"));
                let role = role.into_iter().chain(constraints).collect::<Vec<_>>().join(" ");
                ((!role.is_empty()).then_some(role), end.multiplicity.map(|m| m.to_string()))
            };
            ends = [texts(a), texts(b)];
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
            label = Some("<<include>>".to_owned());
        }
        Some(ElementKind::Extend(p)) => {
            edge.line = LineStyle::ShortDashed;
            edge.target_marker = EndMarker::OpenArrow;
            label = Some(match p.condition.trim() {
                "" => "<<extend>>".to_owned(),
                condition => format!("<<extend>> [{condition}]"),
            });
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
            let text = p.label();
            if !text.is_empty() {
                label = join_label(label, text);
            }
        }
        _ => edge.line = LineStyle::ShortDashed,
    }

    let pts = &edge.points;
    let n = pts.len();
    let at = |text: String, base: Point, offset: Point| EdgeLabel {
        text,
        anchor: base.offset(offset.x, offset.y),
        align: LabelAlign::Left,
    };
    let (a, b) = route::middle_segment(pts, shape.is_curve());
    let center = middle(a, b);
    let association = matches!(kind, Some(ElementKind::Association(_)));
    // Measured for these; other edges keep UmlCraft's own place.
    let astah_name = association
        || matches!(
            kind,
            Some(
                ElementKind::Dependency(_)
                    | ElementKind::Realization(_)
                    | ElementKind::Generalization(_)
            )
        );
    let name = label.map(|text| match placed.name {
        Some(offset) => at(text, center, offset),
        None if astah_name => association_name(text, center, measure),
        None => middle_label(text, a, b),
    });
    // Astah shows which way an association's name reads, from its first end to its second,
    // with a small triangle next to the name.
    if association
        && has_name
        && let Some(name) = &name
    {
        let width = label_width(&name.text, measure);
        edge.name_arrow = Some(name_arrow(name.anchor, width, (a, b)));
    }
    edge.labels.extend(name);
    let [(source_role, source_mult), (target_role, target_mult)] = ends;
    let (source_tip, source_from) = (pts[0], pts[1]);
    let (target_tip, target_from) = (pts[n - 1], pts[n - 2]);
    let place = |text: Option<String>, offset: Option<Point>, end: End, role| {
        text.map(|text| match offset {
            Some(offset) => at(text, end.tip, offset),
            None => end_label(text, end, role, measure),
        })
    };
    let source = End { tip: source_tip, next: source_from, source: true };
    let target = End { tip: target_tip, next: target_from, source: false };
    let labels = [
        place(source_role, placed.source_role, source, true),
        place(source_mult, placed.source_multiplicity, source, false),
        place(target_role, placed.target_role, target, true),
        place(target_mult, placed.target_multiplicity, target, false),
    ];
    edge.labels.extend(labels.into_iter().flatten());
    edge
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
    let exact = measure.width(text, FONT_SIZE, false);
    exact.max(measure.rounded_width(text, FONT_SIZE, false)).max(10.0)
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
    EdgeLabel { text, anchor, align: LabelAlign::Left }
}

/// Places the name of an association, dependency, realization or generalization as Astah does:
/// centered on the middle of the line, its center 9 below it.
fn association_name(text: String, middle: Point, measure: &dyn TextMeasure) -> EdgeLabel {
    let width = label_width(&text, measure);
    let anchor = middle.offset(-width / 2.0, 9.0 - LABEL_HEIGHT / 2.0);
    EdgeLabel { text, anchor, align: LabelAlign::Left }
}

/// Astah's triangle that shows which way a name reads, next to the name's box (top left `at`,
/// `width` wide): it points along the line `a`–`b`, horizontally or vertically, whichever is
/// closer, and sits 4 to the right or left of the box, or below or above it.
fn name_arrow(at: Point, width: f32, (a, b): (Point, Point)) -> [Point; 3] {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let (left, top, right) = (at.x, at.y, at.x + width);
    let cx = at.x + width / 2.0;
    let p = Point::new;
    if dx.abs() >= dy.abs() {
        let (base, tip) =
            if dx >= 0.0 { (right + 4.0, right + 14.0) } else { (left - 4.0, left - 14.0) };
        [p(base, top + 2.5), p(base, top + 12.5), p(tip, top + 7.5)]
    } else {
        let (base, tip) =
            if dy >= 0.0 { (top + 15.5, top + 25.5) } else { (top - 3.5, top - 13.5) };
        [p(cx - 5.0, base), p(cx + 5.0, base), p(cx, tip)]
    }
}

/// Places a name or stereotype label at the middle of the segment `a`–`b`.
pub(crate) fn middle_label(text: String, a: Point, b: Point) -> EdgeLabel {
    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    if (b.x - a.x).abs() >= (b.y - a.y).abs() {
        EdgeLabel {
            text,
            anchor: Point::new(mid.x, mid.y - LABEL_HEIGHT - 3.0),
            align: LabelAlign::Center,
        }
    } else {
        EdgeLabel {
            text,
            anchor: Point::new(mid.x + 6.0, mid.y - LABEL_HEIGHT / 2.0),
            align: LabelAlign::Left,
        }
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
pub fn hit_test(scene: &Scene, p: Point, tolerance: f32) -> Option<Id> {
    let edge = scene
        .edges
        .iter()
        .rev()
        .find(|e| e.path().windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= tolerance));
    if let Some(edge) = edge {
        return Some(edge.id);
    }
    scene.nodes.iter().rev().find(|n| n.hit(p, tolerance)).map(|n| n.id)
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
        let Figure::Classifier { separators } = &customer.figure else { panic!() };
        assert_eq!(separators.len(), 2, "attributes and operations");
        let texts: Vec<(&str, RowRole)> =
            customer.rows.iter().map(|r| (r.text.as_str(), r.role)).collect();
        assert_eq!(texts, vec![("Customer", RowRole::Name), ("- name : String", RowRole::Member)]);
        // The stored bounds are big enough, so the box keeps them.
        assert_eq!(customer.rect, Rect::new(80.0, 80.0, 160.0, 90.0));
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
            matches!(&order.figure, Figure::Classifier { separators } if separators.is_empty())
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
        // Inside the frame, away from border and title: not the boundary.
        let inside = Point::new(frame.rect.x + 40.0, frame.rect.bottom() - 40.0);
        assert_ne!(hit_test(&scene, inside, 4.0), Some(boundary));
        assert_eq!(hit_test(&scene, Point::new(frame.rect.x + 1.0, inside.y), 4.0), Some(boundary));
        assert_eq!(hit_test(&scene, frame.rows[0].rect.center(), 4.0), Some(boundary));

        // The "Place order" use case (220,110 140x60) lies inside the boundary (180,60 300x360).
        let use_case = doc.model().shown_on(doc.model().children(doc.model().root())[2])[0].1;
        let moved = with_contents(&scene, &[boundary]);
        assert!(moved.contains(&boundary) && moved.contains(&use_case), "{moved:?}");
        assert_eq!(moved.len(), 2, "the actor at x=60 is outside");
    }

    #[test]
    fn use_cases_wrap_names_and_list_extension_points() {
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        let place_order = doc.model().children(doc.model().root())[2];
        doc.edit("edit", |tx| {
            tx.rename(place_order, "Place an order with express delivery and gift wrapping")?;
            tx.create(
                place_order,
                umlcraft_model::ElementData::new("payment", ElementKind::ExtensionPoint),
            )?;
            tx.create(
                place_order,
                umlcraft_model::ElementData::new("shipping", ElementKind::ExtensionPoint),
            )
        })
        .unwrap();
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let node = scene.nodes.iter().find(|n| n.element == Some(place_order)).unwrap();
        let Figure::UseCase { separator: Some(sep) } = node.figure else { panic!("separator") };
        let names: Vec<&Row> = node.rows.iter().filter(|r| r.role == RowRole::Name).collect();
        assert!(names.len() >= 3, "wrapped into {} lines", names.len());
        let members: Vec<&str> = node
            .rows
            .iter()
            .filter(|r| r.role == RowRole::Member)
            .map(|r| r.text.as_str())
            .collect();
        assert_eq!(members, ["payment", "shipping"]);
        assert!(names.iter().all(|r| r.rect.bottom() <= sep));
        assert!(node.rect.height > 60.0, "the ellipse grew: {:?}", node.rect);
    }

    #[test]
    fn extend_shows_its_condition() {
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
        assert_eq!(scene.edge(edge).unwrap().labels[0].text, "<<extend>> [express]");
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
        let text = || "1".to_owned();
        // Middle labels: centered above horizontal, beside vertical segments.
        let m = middle_label(text(), Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        assert_eq!(m.align, LabelAlign::Center);
        assert!((m.anchor.y + LABEL_HEIGHT + 3.0).abs() < 0.01);
        let m = middle_label(text(), Point::new(0.0, 0.0), Point::new(0.0, 100.0));
        assert_eq!((m.align, m.anchor.x), (LabelAlign::Left, 6.0));
    }

    /// Sizes measured in an SVG export of Astah 12.
    #[test]
    fn classifier_boxes_use_astahs_metrics() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let customer = scene.nodes.iter().find(|n| n.element == Some(ids.customer)).unwrap();
        let Figure::Classifier { separators } = &customer.figure else { panic!() };
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
            .map(|r| {
                let exact = ApproxMeasure.width(&r.text, r.size, r.bold);
                exact.max(ApproxMeasure.rounded_width(&r.text, r.size, r.bold))
            })
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
    fn state_machine_scene() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.state_diagram, &Preview::default(), &ApproxMeasure);
        let node = |name: &str| {
            scene.nodes.iter().find(|n| n.rows.first().is_some_and(|r| r.text == name)).unwrap()
        };
        // A composite state heads its substates with its name; a state with an entry action
        // lists it below a line; a plain state shows only its name, in the middle.
        let open = node("Open");
        let Figure::State { separator: Some(line) } = open.figure else { panic!() };
        assert!(open.rows[0].rect.bottom() <= line, "name above the line");
        let submitted = node("Submitted");
        assert!(matches!(submitted.figure, Figure::State { separator: Some(_) }));
        assert_eq!(submitted.rows[1].text, "entry / check stock");
        let shipped = node("Shipped");
        assert!(matches!(shipped.figure, Figure::State { separator: None }));
        assert!((shipped.rows[0].rect.center().y - shipped.rect.center().y).abs() < 0.5);

        let labels: Vec<&str> =
            scene.edges.iter().flat_map(|e| &e.labels).map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"[in stock] / ship"), "{labels:?}");
        assert_eq!(labels.iter().filter(|&&l| l == "add item").count(), 2);
        // Moving the composite state moves its substates.
        let moved = with_contents(&scene, &[open.id]);
        assert_eq!(moved.len(), 4, "the state, its initial pseudostate and two substates");
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
        let labels: Vec<&str> =
            scene.edges.iter().flat_map(|e| &e.labels).map(|l| l.text.as_str()).collect();
        assert!(labels.contains(&"<<use>>") && labels.contains(&"<<manifest>>"), "{labels:?}");

        let scene = build(model, ids.deployment_diagram, &Preview::default(), &ApproxMeasure);
        assert!(matches!(figure(&scene, "AppServer"), Figure::Cube { .. }));
        // Instances show their classifier, underlined, in the figure of their kind.
        let web1 = node(&scene, "web1 : AppServer");
        assert!(matches!(web1.figure, Figure::Cube { .. }));
        assert!(web1.rows[0].underline);
        assert!(matches!(figure(&scene, ": Shop"), Figure::Component { .. }));
        // A node takes what is deployed on it along.
        assert_eq!(with_contents(&scene, &[web1.id]).len(), 2);
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
