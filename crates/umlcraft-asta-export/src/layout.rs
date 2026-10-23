//! The layout sidecar: what UmlCraft derives when drawing instead of storing it. Nodes grow with
//! their content, and on sequence diagrams message rows and fragment frames follow from the
//! order of the interaction.

use std::collections::BTreeMap;

use serde::Serialize;
use umlcraft_model::{EdgeLabels, Id, Model, Point, Rect, Shape};
use umlcraft_notation::scene::{self, Figure, Preview, TextMeasure};
use umlcraft_notation::sequence::HEAD_HEIGHT;

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
    /// Sequence diagrams: the bottom of the lifeline heads, where the rows start.
    #[serde(skip_serializing_if = "Option::is_none")]
    head_bottom: Option<f32>,
}

pub(crate) fn layout(model: &Model, measure: &dyn TextMeasure) -> Layout {
    let mut diagrams = BTreeMap::new();
    for (diagram, view) in model.views() {
        let scene = scene::build(model, diagram, &Preview::default(), measure);
        let head_bottom = scene
            .nodes
            .iter()
            .filter_map(|n| match n.figure {
                Figure::Lifeline { head } => Some(head.y),
                _ => None,
            })
            .reduce(f32::min)
            .map(|top| top + HEAD_HEIGHT);
        let mut labels = BTreeMap::new();
        for p in view.presentations() {
            let Shape::Edge { labels: placed, .. } = &p.shape else { continue };
            if let Some(edge) = scene.edge(p.id).filter(|_| !placed.is_empty()) {
                labels.insert(p.id, absolute(placed, &edge.points));
            }
        }
        diagrams.insert(
            diagram,
            DiagramLayout {
                nodes: scene.nodes.iter().map(|n| (n.id, n.rect)).collect(),
                edges: scene.edges.iter().map(|e| (e.id, e.points.clone())).collect(),
                labels,
                head_bottom,
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
