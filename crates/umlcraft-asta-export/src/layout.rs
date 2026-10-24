//! The layout sidecar: what UmlCraft derives when drawing instead of storing it. Nodes grow with
//! their content and edges run between their borders. Sequence diagrams store all of their
//! geometry, so the tool reads them from the project.

use std::collections::BTreeMap;

use serde::Serialize;
use umlcraft_model::{EdgeLabels, Id, Model, Point, Rect, Shape};
use umlcraft_notation::scene::{self, Preview, TextMeasure};

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
    /// Labels placed by hand, as the top left corner on the diagram.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<Id, EdgeLabels>,
    /// Sequence messages whose label is placed by hand: where Astah puts the label by itself
    /// (its left edge), which its `name.point` is relative to. Astah centers it on the last
    /// segment by Java's whole-pixel width of the text, at most 200.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    label_defaults: BTreeMap<Id, f32>,
}

pub(crate) fn layout(model: &Model, measure: &dyn TextMeasure) -> Layout {
    let mut diagrams = BTreeMap::new();
    for (diagram, view) in model.views() {
        let scene = scene::build(model, diagram, &Preview::default(), measure);
        let mut labels = BTreeMap::new();
        let mut label_defaults = BTreeMap::new();
        for p in view.presentations() {
            match &p.shape {
                Shape::Edge { labels: placed, .. } => {
                    if let Some(edge) = scene.edge(p.id).filter(|_| !placed.is_empty()) {
                        labels.insert(p.id, absolute(placed, &edge.points));
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
                    let width = measure.rounded_width(text, 12.0, false).min(200.0);
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
            },
        );
    }
    Layout { diagrams }
}

/// Label offsets turned into positions: roles and multiplicities are relative to their end of
/// the line, the name to the middle of its middle segment, as `scene` places them.
fn absolute(placed: &EdgeLabels, points: &[Point]) -> EdgeLabels {
    let n = points.len();
    let (a, b) = (points[n / 2 - 1], points[n / 2]);
    let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    let at = |base: Point, offset: Option<Point>| offset.map(|o| base.offset(o.x, o.y));
    EdgeLabels {
        name: at(middle, placed.name),
        source_role: at(points[0], placed.source_role),
        source_multiplicity: at(points[0], placed.source_multiplicity),
        target_role: at(points[n - 1], placed.target_role),
        target_multiplicity: at(points[n - 1], placed.target_multiplicity),
    }
}
