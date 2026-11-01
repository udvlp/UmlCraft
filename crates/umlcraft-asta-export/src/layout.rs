//! The layout sidecar: what UmlCraft derives when drawing instead of storing it. Nodes grow with
//! their content and edges run between their borders. Sequence diagrams store all of their
//! geometry, so the tool reads them from the project.

use std::collections::BTreeMap;

use serde::Serialize;
use umlcraft_model::{EdgeLabels, Id, Model, Point, Rect, Shape};
use umlcraft_notation::labels::LabelKind;
use umlcraft_notation::route;
use umlcraft_notation::scene::{self, EdgeShape, Preview, TextMeasure};

#[derive(Serialize)]
pub(crate) struct Layout {
    diagrams: BTreeMap<Id, DiagramLayout>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagramLayout {
    /// Node presentations as drawn.
    nodes: BTreeMap<Id, Rect>,
    /// Edge presentations as drawn, from end to end.
    edges: BTreeMap<Id, Vec<Point>>,
    /// Labels placed by hand or moved out of the way by the drawing, as the top left corner on
    /// the diagram.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<Id, EdgeLabels>,
    /// Sequence messages whose label is placed by hand: where Astah puts the label by itself
    /// (its left edge), which its `name.point` is relative to. Astah centers it on the last
    /// segment by Java's whole-pixel width of the text, at most 200.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    label_defaults: BTreeMap<Id, f32>,
    /// Right-angled edges: their corners as drawn. The drawing keeps the first and last segment
    /// straight when nodes move or grow, so the stored waypoints may lag behind.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    corners: BTreeMap<Id, Vec<Point>>,
}

pub(crate) fn layout(model: &Model, measure: &dyn TextMeasure) -> Layout {
    let mut diagrams = BTreeMap::new();
    for (diagram, view) in model.views() {
        let scene = scene::build(model, diagram, &Preview::default(), measure);
        let mut labels = BTreeMap::new();
        let mut label_defaults = BTreeMap::new();
        let mut corners = BTreeMap::new();
        for p in view.presentations() {
            match &p.shape {
                Shape::Edge { labels: placed, .. } => {
                    let Some(edge) = scene.edge(p.id) else { continue };
                    let at = absolute(placed, edge);
                    if !at.is_empty() {
                        labels.insert(p.id, at);
                    }
                    if edge.shape.is_right_angle() {
                        corners.insert(p.id, edge.waypoints().to_vec());
                    }
                }
                Shape::Message { labels: placed, .. } if placed.name.is_some() => {
                    let Some(edge) = scene.edge(p.id) else { continue };
                    let Some(text) = edge.labels.first().map(|l| l.text.as_str()) else {
                        continue;
                    };
                    let n = edge.points.len();
                    // A message to self has its label beside the loop.
                    let middle = if n == 4 {
                        edge.points[1].x + 7.45
                    } else {
                        (edge.points[n - 2].x + edge.points[n - 1].x) / 2.0
                    };
                    let width = measure.rounded_width(text, scene::FONT_SIZE, false).min(200.0);
                    label_defaults.insert(p.id, middle - width / 2.0);
                }
                _ => {}
            }
        }
        diagrams.insert(
            diagram,
            DiagramLayout {
                nodes: scene.nodes.iter().map(|n| (n.id, n.rect)).collect(),
                edges: scene.edges.iter().map(|e| (e.id, e.points.clone())).collect(),
                labels,
                label_defaults,
                corners,
            },
        );
    }
    Layout { diagrams }
}

/// Label offsets turned into positions: roles and multiplicities are relative to where the line
/// meets their node, the name to the middle of its middle segment, as `scene` places them.
/// Labels the drawing moved out of the way go where it drew them; Astah would put them back.
fn absolute(placed: &EdgeLabels, edge: &EdgeShape) -> EdgeLabels {
    let (a, b) = route::middle_segment(&edge.points, edge.shape.is_curve());
    let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let at = |base: Point, offset: Option<Point>| offset.map(|o| base.offset(o.x, o.y));
    let [source, target] = edge.node_tips;
    let mut labels = EdgeLabels {
        name: at(middle, placed.name),
        source_role: at(source, placed.source_role),
        source_multiplicity: at(source, placed.source_multiplicity),
        target_role: at(target, placed.target_role),
        target_multiplicity: at(target, placed.target_multiplicity),
        // The API has no property for where «include» and «extend» go; Astah places them.
        keyword: None,
    };
    for free in edge.free_labels.iter().filter(|f| f.moved) {
        let at = Some(edge.labels[free.index].anchor);
        match free.kind {
            LabelKind::Name => labels.name = at,
            LabelKind::Keyword => {}
            LabelKind::Role { source: true } => labels.source_role = at,
            LabelKind::Role { source: false } => labels.target_role = at,
            LabelKind::Multiplicity { source: true } => labels.source_multiplicity = at,
            LabelKind::Multiplicity { source: false } => labels.target_multiplicity = at,
        }
    }
    labels
}
