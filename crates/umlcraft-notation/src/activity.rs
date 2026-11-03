//! Activity diagrams: pins on the border of their action and activity parameter nodes on the
//! diagram's frame.
//!
//! As with ports ([`crate::composite`]), where they go follows from the drawing: a new pin
//! belongs to the action whose border is nearest to where it is dropped and sits just outside
//! that border, as Astah draws pins; an activity parameter node belongs to the activity and sits
//! on the frame, which is added around the drawing when the diagram has none.
//! [`composite::follow`](crate::composite::follow) keeps both there after every edit.

use umlcraft_model::{
    ElementKind, Id, Model, ModelError, Point, Presentation, Rect, Result, Tx, View,
};

use crate::composite::{border_distance, snap};
use crate::palette::{NodeTool, owner_for, unique_name};
use crate::scene::PIN_SIZE;

/// Why a pin could not be placed, as [`ModelError::Invalid`] says it; the app words it in the
/// language of its user.
pub const PIN_OUTSIDE: &str = "a pin goes on an action";

/// How close to an action's border a new pin must be dropped to go on it, when not dropped on
/// the action itself.
const NEAR_ACTION: f32 = 20.0;

/// Room the frame Astah draws leaves around the drawing: below its tab, and on the other sides.
const FRAME_TOP: f32 = 40.0;
const FRAME_MARGIN: f32 = 20.0;

fn is_action(model: &Model, p: &Presentation) -> bool {
    p.element
        .and_then(|e| model.get(e))
        .is_some_and(|e| matches!(e.kind(), ElementKind::ActivityNode(n) if n.node.is_action()))
}

/// The action node of `view` a pin dropped with its center at `at` goes on: the one drawn
/// around the point, else the one whose border is nearest, within reach.
fn action_at<'v>(model: &Model, view: &'v View, at: Point) -> Option<&'v Presentation> {
    view.presentations()
        .iter()
        .filter(|p| is_action(model, p))
        .filter_map(|p| Some((p, p.bounds()?)))
        .map(|(p, r)| (p, if r.contains(at) { -1.0 } else { border_distance(r, at) }))
        .filter(|(_, d)| *d <= NEAR_ACTION)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// The frame of `diagram` (a node of the diagram itself), if it has one.
pub fn frame(view: &View, diagram: Id) -> Option<&Presentation> {
    view.presentations().iter().find(|p| p.element == Some(diagram) && p.bounds().is_some())
}

/// The bounds of a frame drawn around `content`, with room for its tab.
pub(crate) fn frame_around(content: Rect) -> Rect {
    Rect::new(
        content.x - FRAME_MARGIN,
        content.y - FRAME_TOP,
        content.width + 2.0 * FRAME_MARGIN,
        content.height + FRAME_TOP + FRAME_MARGIN,
    )
}

/// Adds a frame to `diagram` around everything drawn on it and `at`, behind the rest, and
/// returns its bounds.
pub(crate) fn add_frame(tx: &mut Tx<'_>, diagram: Id, at: Point) -> Result<Rect> {
    let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let content = view
        .presentations()
        .iter()
        .filter_map(|p| p.bounds())
        .fold(Rect::new(at.x, at.y, 0.0, 0.0), |a, b| a.union(&b));
    let bounds = frame_around(content);
    let presentation = umlcraft_model::Presentation::node(Some(diagram), bounds);
    tx.apply(umlcraft_model::Op::InsertPresentation { diagram, index: 0, presentation })?;
    Ok(bounds)
}

/// Creates a pin or an activity parameter node with `tool` at `origin` on the activity diagram
/// `diagram`. A pin goes on the action at or nearest to the point and belongs to it; an
/// activity parameter node belongs to the activity and goes on the frame's border nearest to
/// the point, the frame being added first if the diagram has none. Returns (element,
/// presentation).
pub fn create_node(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    origin: Point,
    base_name: &str,
) -> Result<(Id, Id)> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let mut bounds = Rect::from_origin_size(origin, tool.default_size());
    let at = bounds.center();
    let kind = tool.element_kind();
    let owner = match tool {
        NodeTool::InputPin | NodeTool::OutputPin => {
            let action = action_at(model, view, at)
                .ok_or_else(|| ModelError::Invalid(PIN_OUTSIDE.into()))?;
            let r = action.bounds().ok_or(ModelError::NotFound(action.id))?;
            bounds = snap(bounds, r.inflate(PIN_SIZE / 2.0));
            action.element.ok_or(ModelError::NotFound(action.id))?
        }
        _ => {
            let start = model.element(diagram)?.owner().ok_or(ModelError::Root)?;
            let owner = owner_for(model, start, &kind)
                .ok_or_else(|| ModelError::Invalid("no activity for the parameter".into()))?;
            let framed = frame(view, diagram).and_then(|f| f.bounds());
            let r = match framed {
                Some(r) => r,
                None => add_frame(tx, diagram, at)?,
            };
            bounds = snap(bounds, r);
            owner
        }
    };
    let name = unique_name(tx.model(), owner, base_name);
    let element = tx.create(owner, tool.element_data(name))?;
    let node = tx.add_node(diagram, Some(element), bounds)?;
    Ok((element, node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{self, EdgeTool};
    use crate::scene::{self, ApproxMeasure, Figure, Preview};
    use umlcraft_model::{ActivityNodeKind, DiagramKind, Document};

    fn diagram() -> (Document, Id, Id) {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let diagram = doc
            .edit("d", |tx| crate::edit::create_diagram(tx, root, DiagramKind::Activity, "Order"))
            .unwrap();
        let activity = doc.model().element(diagram).unwrap().owner().unwrap();
        (doc, activity, diagram)
    }

    fn create(doc: &mut Document, diagram: Id, tool: NodeTool, at: Point) -> Result<(Id, Id)> {
        crate::edit::apply(doc, "create", &ApproxMeasure, |tx| {
            palette::create_node(tx, diagram, tool, at, "n")
        })
    }

    fn bounds(doc: &Document, diagram: Id, node: Id) -> Rect {
        doc.model().view(diagram).unwrap().get(node).unwrap().bounds().unwrap()
    }

    #[test]
    fn pins_sit_outside_their_action_and_follow_it() {
        let (mut doc, _, diagram) = diagram();
        let (action, action_node) =
            create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 100.0)).unwrap();
        assert!(
            create(&mut doc, diagram, NodeTool::InputPin, Point::new(600.0, 600.0)).is_err(),
            "a pin needs an action"
        );
        let a = bounds(&doc, diagram, action_node);
        // Dropped on the left edge, the pin goes just outside it and belongs to the action.
        let (pin, pin_node) =
            create(&mut doc, diagram, NodeTool::InputPin, Point::new(a.x - 5.0, a.y + 5.0))
                .unwrap();
        assert_eq!(doc.model().element(pin).unwrap().owner(), Some(action));
        let p = bounds(&doc, diagram, pin_node);
        assert_eq!(p.size(), umlcraft_model::Size::new(PIN_SIZE, PIN_SIZE));
        assert_eq!(p.right(), a.x, "outside the left edge");
        // An output pin dropped on the action's right half lands on its right edge.
        let (_, out_node) = create(
            &mut doc,
            diagram,
            NodeTool::OutputPin,
            Point::new(a.right() - 10.0, a.center().y - 7.5),
        )
        .unwrap();
        assert_eq!(bounds(&doc, diagram, out_node).x, a.right());

        // Moving and widening the action takes its pins along.
        let moved = Rect::new(a.x + 50.0, a.y + 30.0, a.width + 40.0, a.height);
        crate::edit::apply(&mut doc, "move", &ApproxMeasure, |tx| {
            tx.set_bounds(diagram, action_node, moved)
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, pin_node).right(), moved.x);
        assert_eq!(bounds(&doc, diagram, out_node).x, moved.right());

        // A pin dragged away comes back to its action's border, on the side nearest.
        let away = bounds(&doc, diagram, out_node).translate(0.0, -200.0);
        crate::edit::apply(&mut doc, "drag", &ApproxMeasure, |tx| {
            tx.set_bounds(diagram, out_node, away)
        })
        .unwrap();
        assert_eq!(bounds(&doc, diagram, out_node).bottom(), moved.y, "on top");

        // Flows connect pins; a flow into an output pin is refused.
        let (_, object_node) =
            create(&mut doc, diagram, NodeTool::ObjectNode, Point::new(100.0, 300.0)).unwrap();
        let flow = crate::edit::apply(&mut doc, "flow", &ApproxMeasure, |tx| {
            palette::connect(tx, diagram, EdgeTool::Flow, out_node, object_node)
        });
        assert!(flow.is_ok());
        let wrong = crate::edit::apply(&mut doc, "flow", &ApproxMeasure, |tx| {
            palette::connect(tx, diagram, EdgeTool::Flow, object_node, out_node)
        });
        assert!(wrong.is_err());
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        // The connected pin shows no arrow, the other one points into the action.
        let figure = |id| &scene.node(id).unwrap().figure;
        assert!(matches!(figure(out_node), Figure::Pin { arrow: None }));
        let Figure::Pin { arrow: Some([from, to]) } = figure(pin_node) else { panic!() };
        assert!(to.x > from.x, "into the action: {from:?} {to:?}");

        // Removing the action from the diagram takes its pins along; deleting it, the pins.
        crate::edit::apply(&mut doc, "remove", &ApproxMeasure, |tx| {
            tx.remove_presentation(diagram, action_node)
        })
        .unwrap();
        assert!(doc.model().view(diagram).unwrap().get(pin_node).is_none());
        crate::edit::apply(&mut doc, "delete", &ApproxMeasure, |tx| tx.delete(action)).unwrap();
        assert!(doc.model().get(pin).is_none());
        assert!(doc.model().check().is_ok());
    }

    #[test]
    fn activity_parameters_sit_on_the_frame() {
        let (mut doc, activity, diagram) = diagram();
        create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 100.0)).unwrap();
        let (parameter, node) =
            create(&mut doc, diagram, NodeTool::ActivityParameter, Point::new(60.0, 110.0))
                .unwrap();
        assert_eq!(doc.model().element(parameter).unwrap().owner(), Some(activity));
        let view = doc.model().view(diagram).unwrap();
        let framed = frame(view, diagram).expect("a frame was added").bounds().unwrap();
        assert_eq!(bounds(&doc, diagram, node).center().x, framed.x, "on the left border");
        // The frame grows around what is drawn; the parameter stays on its border.
        create(&mut doc, diagram, NodeTool::Action, Point::new(100.0, 700.0)).unwrap();
        let scene = scene::build(doc.model(), diagram, &Preview::default(), &ApproxMeasure);
        let frame_rect = scene.nodes.iter().find(|n| n.element == Some(diagram)).unwrap().rect;
        assert!(frame_rect.bottom() > 700.0);
        let drawn = scene.node(node).unwrap().rect;
        assert_eq!(drawn.center().x, frame_rect.x);
        let kind = doc.model().element(parameter).unwrap().kind();
        assert!(
            matches!(kind, ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::ActivityParameter)
        );
    }
}
