//! Turns a diagram view into drawable shapes in logical coordinates. The scene also carries the
//! layout of every text row, so painting, hit testing and inline editing agree on positions.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    Aggregation, DiagramKind, ElementKind, Id, MessageSort, Model, Navigability, Point,
    Presentation, Rect, Shape, Style,
};

use crate::member::member_text;
use crate::sequence::{self, SequenceLayout};

/// Measures text, so nodes can grow with their content. The app measures with the real font;
/// [`ApproxMeasure`] is good enough for tests and headless use.
pub trait TextMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32;
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
            Figure::Boundary | Figure::Fragment { .. } => {
                let inner = self.rect.inflate(-2.0 * tolerance.max(3.0));
                !inner.contains(p) || self.rows.iter().any(|r| r.rect.contains(p))
            }
            Figure::Lifeline { head } => {
                head.contains(p) || (p.x - head.center().x).abs() <= tolerance + 4.0
            }
            _ => self.rect.contains(p),
        }
    }

    /// Whether the user may resize the node. Lifelines and fragments size themselves.
    pub fn resizable(&self) -> bool {
        !matches!(self.figure, Figure::Lifeline { .. } | Figure::Fragment { .. })
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
    Lifeline {
        head: Rect,
    },
    /// Combined fragment: a frame with the operator in a tab at the top left and dashed lines
    /// between the operands.
    Fragment {
        tab: Rect,
        separators: Vec<f32>,
    },
    Note,
    Other,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndMarker {
    None,
    OpenArrow,
    FilledArrow,
    HollowTriangle,
    HollowDiamond,
    FilledDiamond,
}

pub struct EdgeShape {
    pub id: Id,
    /// Polyline from the source border through the waypoints to the target border.
    pub points: Vec<Point>,
    pub line: LineStyle,
    pub source_marker: EndMarker,
    pub target_marker: EndMarker,
    pub labels: Vec<EdgeLabel>,
    /// The route follows from the layout (sequence messages); it has no waypoints to drag.
    pub fixed: bool,
}

/// A grip on a selected edge: an existing waypoint, or the middle of a segment where dragging
/// inserts a new waypoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeHandle {
    Waypoint(usize),
    Insert(usize),
}

impl EdgeShape {
    pub fn waypoints(&self) -> &[Point] {
        &self.points[1..self.points.len() - 1]
    }

    /// Grips in drawing order: waypoints first, then segment middles.
    pub fn handles(&self) -> Vec<(EdgeHandle, Point)> {
        if self.fixed {
            return Vec::new();
        }
        let waypoints =
            self.waypoints().iter().enumerate().map(|(i, &p)| (EdgeHandle::Waypoint(i), p));
        let inserts = self.points.windows(2).enumerate().map(|(i, w)| {
            (EdgeHandle::Insert(i), Point::new((w[0].x + w[1].x) / 2.0, (w[0].y + w[1].y) / 2.0))
        });
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
        EdgeHandle::Waypoint(_) => {}
    }
    result
}

/// Waypoints of a loop at the top right corner, for edges from a node to itself.
pub fn self_loop(r: Rect) -> Vec<Point> {
    vec![
        Point::new(r.right() + 30.0, r.y + 20.0),
        Point::new(r.right() + 30.0, r.y - 30.0),
        Point::new(r.right() - 20.0, r.y - 30.0),
    ]
}

/// Height of an edge label line (11 pt text) in logical units.
pub const LABEL_HEIGHT: f32 = 14.0;

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
    pub waypoints: Option<(Id, Vec<Point>)>,
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

fn line_height(size: f32) -> f32 {
    (size * 1.3).ceil()
}

/// A row before it is positioned.
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
        (None, Vec::new())
    };
    let rects: HashMap<Id, Rect> = nodes.iter().map(|n| (n.id, n.rect)).collect();
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
            let Shape::Edge { source, target, waypoints } = &p.shape else { return None };
            let (source_rect, target_rect) = (*rects.get(source)?, *rects.get(target)?);
            let mut points = match &preview.waypoints {
                Some((id, w)) if *id == p.id => w.clone(),
                _ => waypoints.clone(),
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
                    (!stereotypes.is_empty()).then(|| format!("«{stereotypes}»")),
                    (!e.name().is_empty()).then(|| e.name().to_owned()),
                ]
                .into_iter()
                .flatten()
                .collect();
                (!parts.is_empty()).then(|| parts.join(" "))
            });
            let to_lollipop = iconic_interfaces.contains(target);
            Some(edge_for(
                p.id,
                element.map(|e| e.kind()),
                source_rect,
                target_rect,
                &points,
                label,
                to_lollipop,
            ))
        })
        .collect();
    edges.extend(messages);
    Scene { nodes, edges, sequence }
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
    NodeShape { id: p.id, element: p.element, rect, figure, rows }
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
            let r = Rect::new(bounds.x, bounds.y, 24.0, 24.0);
            let line = name_line(12.0, false);
            let w = line.width(measure) + 8.0;
            let row = line.place(
                Rect::new(r.center().x - w / 2.0, r.bottom() + 2.0, w, line_height(12.0)),
                LabelAlign::Center,
            );
            (r, Figure::Lollipop, vec![row])
        }
        kind if is_boxed_classifier(kind) => layout_classifier(model, id, style, bounds, measure),
        ElementKind::Package | ElementKind::Model => {
            let line = name_line(13.0, true);
            let w = bounds.width.max(line.width(measure) + 20.0);
            let r = Rect::new(bounds.x, bounds.y, w, bounds.height.max(40.0));
            let tab = Rect::new(r.x, r.y, (r.width * 0.4).min(80.0), 14.0);
            let row = line
                .place(Rect::new(r.x, r.y + 20.0, r.width, line_height(13.0)), LabelAlign::Center);
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
        ElementKind::Lifeline(p) => {
            let type_name = p.represents.and_then(|t| model.get(t)).map(|t| t.name());
            let header = match type_name {
                Some(t) => format!("{} : {t}", e.name()),
                None => e.name().to_owned(),
            };
            let line = Line::new(header, 12.0, RowRole::Name, Some(id));
            let w = bounds.width.max(line.width(measure) + 16.0);
            let r = Rect::new(bounds.x, bounds.y, w, bounds.height.max(60.0));
            let head = Rect::new(r.x, r.y, r.width, sequence::HEAD_HEIGHT);
            let h = line_height(12.0);
            let row = line
                .place(Rect::new(r.x, head.center().y - h / 2.0, r.width, h), LabelAlign::Center);
            (r, Figure::Lifeline { head }, vec![row])
        }
        ElementKind::Comment(p) => {
            let lines: Vec<Line> = p
                .body
                .split('\n')
                .map(|l| Line::new(l.trim_end_matches('\r'), 12.0, RowRole::Name, Some(id)))
                .collect();
            let h = line_height(12.0);
            let text_w = lines.iter().map(|l| l.width(measure)).fold(0.0, f32::max);
            let r = Rect::new(
                bounds.x,
                bounds.y,
                bounds.width.max(text_w + 22.0),
                bounds.height.max(12.0 + h * lines.len() as f32),
            );
            let rows = lines
                .into_iter()
                .enumerate()
                .map(|(i, l)| {
                    l.place(
                        Rect::new(r.x + 6.0, r.y + 6.0 + h * i as f32, r.width - 22.0, h),
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
            && matches!(container.figure, Figure::Boundary | Figure::Package { .. })
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

fn is_boxed_classifier(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

fn layout_classifier(
    model: &Model,
    id: Id,
    style: &Style,
    bounds: Rect,
    measure: &dyn TextMeasure,
) -> (Rect, Figure, Vec<Row>) {
    const PAD: f32 = 6.0;
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
            format!("«{}»", stereotypes.join(", ")),
            11.0,
            RowRole::Stereotype,
            Some(id),
        ));
    }
    let mut name = Line::new(e.name(), 13.0, RowRole::Name, Some(id));
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
                    Line::new(member_text(model, c.id()), 12.0, RowRole::Member, Some(c.id()));
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
        ElementKind::Interface(_) => {
            if !attributes.is_empty() && !style.hide_attributes {
                compartments.push(attributes);
            }
            if !style.hide_operations {
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

    let all_widths =
        header.iter().chain(compartments.iter().flatten()).map(|l| l.width(measure) + 2.0 * PAD);
    let width = all_widths.fold(60.0, f32::max);
    let header_h: f32 = 4.0 + header.iter().map(|l| line_height(l.size)).sum::<f32>() + 4.0;
    let compartments_h: f32 = compartments
        .iter()
        .map(|c| (6.0 + c.iter().map(|l| line_height(l.size)).sum::<f32>()).max(8.0))
        .sum();
    let r = Rect::new(
        bounds.x,
        bounds.y,
        bounds.width.max(width),
        bounds.height.max(header_h + compartments_h),
    );

    let mut rows = Vec::new();
    let mut y = r.y + 4.0;
    for line in header {
        let h = line_height(line.size);
        rows.push(line.place(Rect::new(r.x, y, r.width, h), LabelAlign::Center));
        y += h;
    }
    y += 4.0;
    let mut separators = Vec::new();
    for compartment in compartments {
        let top = y;
        separators.push(top);
        y += 3.0;
        for line in compartment {
            let h = line_height(line.size);
            rows.push(
                line.place(Rect::new(r.x + PAD, y, r.width - 2.0 * PAD, h), LabelAlign::Left),
            );
            y += h;
        }
        y = (y + 3.0).max(top + 8.0);
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

fn edge_for(
    id: Id,
    kind: Option<&ElementKind>,
    source: Rect,
    target: Rect,
    waypoints: &[Point],
    name_label: Option<String>,
    to_lollipop: bool,
) -> EdgeShape {
    let mut points = Vec::with_capacity(waypoints.len() + 2);
    let first_toward = waypoints.first().copied().unwrap_or(target.center());
    let last_from = waypoints.last().copied().unwrap_or(source.center());
    points.push(border_point(source, first_toward));
    points.extend_from_slice(waypoints);
    points.push(border_point(target, last_from));

    let mut edge = EdgeShape {
        id,
        points,
        line: LineStyle::Solid,
        source_marker: EndMarker::None,
        target_marker: EndMarker::None,
        labels: Vec::new(),
        fixed: false,
    };
    let (mut source_label, mut target_label) = (None, None);
    let mut label = name_label;
    let diamond = |a: Aggregation| match a {
        Aggregation::None => EndMarker::None,
        Aggregation::Shared => EndMarker::HollowDiamond,
        Aggregation::Composite => EndMarker::FilledDiamond,
    };
    match kind {
        Some(ElementKind::Association(p)) => {
            let [a, b] = &p.ends;
            edge.source_marker = diamond(a.aggregation);
            edge.target_marker = diamond(b.aggregation);
            if b.navigability == Navigability::Navigable && edge.target_marker == EndMarker::None {
                edge.target_marker = EndMarker::OpenArrow;
            }
            if a.navigability == Navigability::Navigable && edge.source_marker == EndMarker::None {
                edge.source_marker = EndMarker::OpenArrow;
            }
            let end_label = |role: &str, m: Option<umlcraft_model::Multiplicity>| {
                let text = [role.to_owned(), m.map(|m| m.to_string()).unwrap_or_default()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!text.is_empty()).then_some(text)
            };
            source_label = end_label(&a.role, a.multiplicity);
            target_label = end_label(&b.role, b.multiplicity);
        }
        Some(ElementKind::Generalization(_)) => edge.target_marker = EndMarker::HollowTriangle,
        // Realizing an interface drawn as a circle is a plain line ("lollipop").
        Some(ElementKind::Realization(_)) if to_lollipop => {}
        Some(ElementKind::Realization(_)) => {
            edge.line = LineStyle::Dashed;
            edge.target_marker = EndMarker::HollowTriangle;
        }
        Some(ElementKind::Dependency(_)) => {
            edge.line = LineStyle::Dashed;
            edge.target_marker = EndMarker::OpenArrow;
        }
        Some(ElementKind::Include(_)) => {
            edge.line = LineStyle::Dashed;
            edge.target_marker = EndMarker::OpenArrow;
            label = Some("«include»".to_owned());
        }
        Some(ElementKind::Extend(p)) => {
            edge.line = LineStyle::Dashed;
            edge.target_marker = EndMarker::OpenArrow;
            label = Some(match p.condition.trim() {
                "" => "«extend»".to_owned(),
                condition => format!("«extend» [{condition}]"),
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
        _ => edge.line = LineStyle::Dashed,
    }

    let pts = &edge.points;
    let n = pts.len();
    edge.labels.extend(label.map(|text| middle_label(text, pts[n / 2 - 1], pts[n / 2])));
    edge.labels.extend(source_label.map(|text| end_label(text, pts[0], pts[1])));
    edge.labels.extend(target_label.map(|text| end_label(text, pts[n - 1], pts[n - 2])));
    edge
}

/// Below this slope a segment counts as horizontal (resp. vertical) for label placement.
const STRAIGHT: f32 = 0.2;

/// Places an end label (role, multiplicity) next to the segment `tip`–`from`, a little away
/// from the node at `tip`, on the side of the line it does not run into.
fn end_label(text: String, tip: Point, from: Point) -> EdgeLabel {
    const GAP: f32 = 10.0;
    let len = tip.distance(from).max(1.0);
    let (ux, uy) = ((from.x - tip.x) / len, (from.y - tip.y) / len);
    let base = Point::new(tip.x + ux * GAP, tip.y + uy * GAP);
    if ux.abs() >= uy.abs() {
        // Mostly horizontal: the text extends away from the node, above the line unless the
        // line rises noticeably in that direction.
        let align = if ux >= 0.0 { LabelAlign::Left } else { LabelAlign::Right };
        let y = if uy < -STRAIGHT { base.y + 3.0 } else { base.y - LABEL_HEIGHT - 3.0 };
        EdgeLabel { text, anchor: Point::new(base.x, y), align }
    } else {
        // Mostly vertical: beside the line, right of it unless the line leans to the right.
        let (x, align) = if ux > STRAIGHT {
            (base.x - 6.0, LabelAlign::Right)
        } else {
            (base.x + 6.0, LabelAlign::Left)
        };
        let y = if uy >= 0.0 { base.y } else { base.y - LABEL_HEIGHT };
        EdgeLabel { text, anchor: Point::new(x, y), align }
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
    let c = rect.center();
    let (dx, dy) = (toward.x - c.x, toward.y - c.y);
    if dx == 0.0 && dy == 0.0 {
        return c;
    }
    let sx = if dx != 0.0 { (rect.width / 2.0) / dx.abs() } else { f32::INFINITY };
    let sy = if dy != 0.0 { (rect.height / 2.0) / dy.abs() } else { f32::INFINITY };
    let s = sx.min(sy).min(1.0);
    Point::new(c.x + dx * s, c.y + dy * s)
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
        .find(|e| e.points.windows(2).any(|w| distance_to_segment(p, w[0], w[1]) <= tolerance));
    if let Some(edge) = edge {
        return Some(edge.id);
    }
    scene.nodes.iter().rev().find(|n| n.hit(p, tolerance)).map(|n| n.id)
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
        assert_eq!(scene.edge(edge).unwrap().labels[0].text, "«extend» [express]");
    }

    #[test]
    fn waypoints_handles_and_previews() {
        let (doc, ids) = order_system();
        let d = ids.class_diagram;
        let scene = build(doc.model(), d, &Preview::default(), &ApproxMeasure);
        let edge = scene.edges.iter().find(|e| e.labels.len() == 2).unwrap();
        assert!(edge.waypoints().is_empty());
        let handles = edge.handles();
        assert_eq!(handles.len(), 1, "one segment, one insert grip");
        let (handle, at) = handles[0];
        assert_eq!(edge.handle_at(at, 3.0), Some(handle));
        let waypoints = drag_waypoint(edge.waypoints(), handle, Point::new(300.0, 30.0));
        assert_eq!(waypoints, vec![Point::new(300.0, 30.0)]);
        let moved = drag_waypoint(&waypoints, EdgeHandle::Waypoint(0), Point::new(310.0, 20.0));
        assert_eq!(moved, vec![Point::new(310.0, 20.0)]);

        let preview = Preview { waypoints: Some((edge.id, waypoints)), ..Preview::default() };
        let scene = build(doc.model(), d, &preview, &ApproxMeasure);
        assert_eq!(scene.edge(edge.id).unwrap().points.len(), 3);
    }

    #[test]
    fn association_end_labels_sit_above_a_horizontal_line() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let association = scene.edges.iter().find(|e| e.labels.len() == 2).unwrap();
        // Order (source, right) → Customer (target, left): the line slopes from y ≈ 128.7 to 126.3.
        let pts = &association.points;
        let [star, role] = [&association.labels[0], &association.labels[1]];
        assert_eq!(star.text, "*");
        assert_eq!(role.text, "customer 1");
        for (label, end) in [(star, pts[0]), (role, pts[pts.len() - 1])] {
            let clearance = end.y - (label.anchor.y + LABEL_HEIGHT);
            assert!(clearance >= 2.5, "{label:?} is only {clearance} above the line");
        }
        // Each label extends from its node into the gap between the classes.
        assert_eq!(role.align, LabelAlign::Left);
        assert!((role.anchor.x - 250.0).abs() < 0.5);
        assert_eq!(star.align, LabelAlign::Right);
        assert!((star.anchor.x - 370.0).abs() < 0.5);
    }

    #[test]
    fn end_labels_avoid_sloped_and_vertical_lines() {
        let text = || "1".to_owned();
        // Line rising to the right of the node: label goes below it.
        let l = end_label(text(), Point::new(0.0, 100.0), Point::new(100.0, 50.0));
        assert_eq!(l.align, LabelAlign::Left);
        assert!(l.anchor.y > 100.0 - 10.0 * 0.45);
        // Vertical line going down: label right of it, starting below the tip.
        let l = end_label(text(), Point::new(50.0, 0.0), Point::new(50.0, 100.0));
        assert_eq!((l.align, l.anchor), (LabelAlign::Left, Point::new(56.0, 10.0)));
        // Vertical line leaning right: label moves to the left side.
        let l = end_label(text(), Point::new(50.0, 0.0), Point::new(90.0, 100.0));
        assert_eq!(l.align, LabelAlign::Right);
        assert!(l.anchor.x < 50.0 + 10.0 * 0.38);
        // Middle labels: centered above horizontal, beside vertical segments.
        let m = middle_label(text(), Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        assert_eq!((m.align, m.anchor), (LabelAlign::Center, Point::new(50.0, -17.0)));
        let m = middle_label(text(), Point::new(0.0, 0.0), Point::new(0.0, 100.0));
        assert_eq!((m.align, m.anchor), (LabelAlign::Left, Point::new(6.0, 43.0)));
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
}
