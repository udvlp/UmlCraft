//! The route of a line as `connect` and `update_element` take it and `get_diagram` tells it: its
//! waypoints, where it leaves and enters its nodes, its line style and whether the router keeps
//! it.

use serde_json::{Value, json};
use umlcraft_model::{Id, LineShape, ModelError, Point, Presentation, Shape, Tx};
use umlcraft_notation::{autoroute, shared};

/// The keys of [`Route`].
pub const KEYS: [&str; 5] =
    ["waypoints", "source_anchor", "target_anchor", "line_shape", "auto_route"];

/// The names of the line shapes `line_shape` takes.
pub const SHAPES: [(&str, LineShape); 4] = [
    ("line", LineShape::Line),
    ("right_angle", LineShape::RightAngle),
    ("curve", LineShape::Curve),
    ("curve_right_angle", LineShape::CurveRightAngle),
];

/// What a call sets of a line's route; `None` keeps it.
#[derive(Debug, Default, PartialEq)]
pub struct Route {
    waypoints: Option<Vec<Point>>,
    /// `Some(None)`: back to the node's center.
    source_anchor: Option<Option<Point>>,
    target_anchor: Option<Option<Point>>,
    line_shape: Option<LineShape>,
    auto_route: Option<bool>,
}

/// A point written as `[x, y]` or `{"x": …, "y": …}`.
fn point(value: &Value, key: &str) -> Result<Point, String> {
    let (x, y) = match value {
        Value::Array(items) if items.len() == 2 => (items[0].as_f64(), items[1].as_f64()),
        Value::Object(map) => {
            (map.get("x").and_then(Value::as_f64), map.get("y").and_then(Value::as_f64))
        }
        _ => (None, None),
    };
    match (x, y) {
        (Some(x), Some(y)) => Ok(Point::new(x as f32, y as f32)),
        _ => Err(format!("`{key}` takes points written as [x, y]")),
    }
}

/// An anchor: a point relative to its node, from (0, 0) top left to (1, 1) bottom right, or
/// null for the center.
fn anchor(value: &Value, key: &str) -> Result<Option<Point>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let p = point(value, key)?;
    if !(0.0..=1.0).contains(&p.x) || !(0.0..=1.0).contains(&p.y) {
        return Err(format!(
            "`{key}` is relative to the node, from [0, 0] top left to [1, 1] bottom right"
        ));
    }
    Ok(Some(p))
}

impl Route {
    /// The route `args` give, or `None` if they give none of [`KEYS`].
    pub fn parse(args: &Value) -> Result<Option<Self>, String> {
        if !KEYS.iter().any(|k| args.get(k).is_some()) {
            return Ok(None);
        }
        let waypoints = match args.get("waypoints") {
            None => None,
            Some(Value::Array(points)) => {
                Some(points.iter().map(|p| point(p, "waypoints")).collect::<Result<_, _>>()?)
            }
            Some(_) => {
                return Err("`waypoints` is a list of points, e.g. [[290, 50], [70, 50]]".into());
            }
        };
        let source_anchor =
            args.get("source_anchor").map(|v| anchor(v, "source_anchor")).transpose()?;
        let target_anchor =
            args.get("target_anchor").map(|v| anchor(v, "target_anchor")).transpose()?;
        let line_shape = match args.get("line_shape") {
            None => None,
            Some(v) => {
                let name = v.as_str().unwrap_or_default();
                let shape = SHAPES.iter().find(|(n, _)| *n == name).map(|(_, s)| *s);
                Some(shape.ok_or_else(|| {
                    let names: Vec<&str> = SHAPES.iter().map(|(n, _)| *n).collect();
                    format!("`line_shape` is one of {}", names.join(", "))
                })?)
            }
        };
        let auto_route = crate::tools::bool_arg(args, "auto_route")?;
        let by_hand = waypoints.is_some()
            || source_anchor.is_some()
            || target_anchor.is_some()
            || line_shape.is_some();
        if auto_route == Some(true) && by_hand {
            return Err("`auto_route` lets the router lay out the line; it goes without \
                        `waypoints`, anchors and `line_shape`"
                .into());
        }
        Ok(Some(Self { waypoints, source_anchor, target_anchor, line_shape, auto_route }))
    }

    /// Gives the line `edge` of `diagram` this route, as drawing it by hand in the app does: the
    /// router leaves it alone from then on. With `auto_route` the router lays it out and keeps
    /// it laid out.
    pub fn apply(&self, tx: &mut Tx<'_>, diagram: Id, edge: Id) -> Result<(), ModelError> {
        let p = tx.model().view(diagram).and_then(|v| v.get(edge));
        let Some(Shape::Edge { waypoints, source_anchor, target_anchor, .. }) = p.map(|p| &p.shape)
        else {
            return Err(ModelError::Invalid(
                "only lines between nodes have a route; a message's is its height `y`".into(),
            ));
        };
        if self.auto_route == Some(true) {
            autoroute::route_lines(tx, diagram, &[edge])?;
            return Ok(());
        }
        let old = waypoints.clone();
        let points = self.waypoints.clone().unwrap_or_else(|| old.clone());
        let anchors = (
            self.source_anchor.unwrap_or(*source_anchor),
            self.target_anchor.unwrap_or(*target_anchor),
        );
        tx.set_route(diagram, edge, points, anchors)?;
        let shape = self.line_shape;
        tx.update_presentation(diagram, edge, |p| {
            if let Some(shape) = shape {
                p.style.line_shape = shape;
            }
            p.style.auto_route = false;
        })?;
        // A shared generalization takes its whole group along, as in the app.
        if self.waypoints.is_some() {
            shared::move_common_line(tx, diagram, edge, &old)?;
        }
        Ok(())
    }
}

/// The route of the line `p` for `get_diagram`, as [`Route`] takes it.
pub fn describe(p: &Presentation, line: &mut Value) {
    let Shape::Edge { waypoints, source_anchor, target_anchor, .. } = &p.shape else { return };
    let xy = |p: &Point| json!([p.x, p.y]);
    line["waypoints"] = json!(waypoints.iter().map(xy).collect::<Vec<_>>());
    line["source_anchor"] = source_anchor.as_ref().map_or(Value::Null, xy);
    line["target_anchor"] = target_anchor.as_ref().map_or(Value::Null, xy);
    let shape = SHAPES.iter().find(|(_, s)| *s == p.style.line_shape).map_or("line", |(n, _)| n);
    line["line_shape"] = json!(shape);
    line["auto_route"] = json!(p.style.auto_route);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_are_read_from_points_and_names() {
        let args = json!({
            "waypoints": [[290, 50], { "x": 70, "y": 50 }],
            "source_anchor": [0.5, 0],
            "target_anchor": null,
            "line_shape": "right_angle",
        });
        let route = Route::parse(&args).unwrap().unwrap();
        assert_eq!(route.waypoints, Some(vec![Point::new(290.0, 50.0), Point::new(70.0, 50.0)]));
        assert_eq!(route.source_anchor, Some(Some(Point::new(0.5, 0.0))));
        assert_eq!(route.target_anchor, Some(None));
        assert_eq!(route.line_shape, Some(LineShape::RightAngle));
        assert_eq!(Route::parse(&json!({ "name": "x" })).unwrap(), None);
        for wrong in [
            json!({ "waypoints": [[1, 2, 3]] }),
            json!({ "waypoints": "1,2" }),
            json!({ "source_anchor": [2, 0] }),
            json!({ "line_shape": "zigzag" }),
            json!({ "auto_route": "yes" }),
            json!({ "auto_route": true, "waypoints": [] }),
        ] {
            assert!(Route::parse(&wrong).is_err(), "{wrong}");
        }
    }
}
