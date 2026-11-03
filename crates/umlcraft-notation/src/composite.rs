//! Composite structure diagrams: a structured class (or component) with its parts drawn inside
//! it, ports on the border of the class or of a part, and connectors between parts and ports.
//!
//! Like the contents of a system boundary, what lies where follows from the drawing: a part
//! belongs to the class drawn around it when it is created, and a port sits on the border of the
//! class or part whose border is nearest. Ports stay on that border: when the class or part moves
//! or changes its size, its ports keep their place along the border ([`follow`]); a port moved by
//! itself goes onto the border nearest to where it was dropped.
//!
//! Activity diagrams have nodes on borders too, which [`follow`] keeps there the same way: pins
//! just outside the border of their action, and activity parameter nodes on the diagram's
//! frame (see [`crate::activity`]). So do state machine diagrams: entry and exit points on the
//! border of their state or of the frame (see [`crate::state_edit`]).

use umlcraft_model::{
    ActivityNodeKind, ConnectorEnd, DiagramKind, ElementData, ElementKind, Id, Model, ModelError,
    Point, Presentation, Rect, Result, Tx, TypeRef, View, Visibility,
};

use crate::palette::{NodeTool, unique_name};

/// Width and height of a port's square.
pub const PORT_SIZE: f32 = 14.0;

/// Why a part or port could not be placed, as [`ModelError::Invalid`] says it; the app words
/// these in the language of its user.
pub const PART_OUTSIDE: &str = "a part goes inside a class";
pub const PORT_OUTSIDE: &str = "a port goes on a class or a part";
pub const PORT_ON_UNTYPED_PART: &str = "a port on a part needs a class as the part's type";

/// Whether `diagram` is a composite structure diagram.
pub fn is_composite(model: &Model, diagram: Id) -> bool {
    model.get(diagram).and_then(|d| d.kind().as_diagram()) == Some(DiagramKind::CompositeStructure)
}

/// Classes and components: drawn as structured classes, with their parts inside.
pub fn is_structured(kind: &ElementKind) -> bool {
    matches!(kind, ElementKind::Class(_) | ElementKind::Component(_))
}

/// What ports sit on: structured classes and parts.
fn is_host(kind: &ElementKind) -> bool {
    is_structured(kind) || matches!(kind, ElementKind::Attribute(_))
}

fn element_kind<'m>(model: &'m Model, p: &Presentation) -> Option<&'m ElementKind> {
    p.element.and_then(|e| model.get(e)).map(|e| e.kind())
}

/// The class or component a part is typed by, whose ports it shows.
pub fn part_type(model: &Model, part: Id) -> Option<Id> {
    let ElementKind::Attribute(p) = model.get(part)?.kind() else { return None };
    let id = match p.type_ref.as_ref()? {
        TypeRef::Element(id) => *id,
        TypeRef::Expr(e) => match &e.base {
            TypeRef::Element(id) => *id,
            _ => return None,
        },
        TypeRef::Text(_) => return None,
    };
    model.get(id).filter(|e| is_structured(e.kind())).map(|e| e.id())
}

/// The innermost node of `view` of an element `pick` accepts whose bounds contain `at`.
fn innermost_at<'v>(
    model: &Model,
    view: &'v View,
    at: Point,
    pick: impl Fn(&ElementKind) -> bool,
) -> Option<&'v Presentation> {
    view.presentations()
        .iter()
        .filter(|p| element_kind(model, p).is_some_and(&pick))
        .filter_map(|p| Some((p, p.bounds()?)))
        .filter(|(_, r)| r.contains(at))
        .min_by(|(_, a), (_, b)| (a.width * a.height).total_cmp(&(b.width * b.height)))
        .map(|(p, _)| p)
}

/// How far `c` is from the border of `r`.
pub(crate) fn border_distance(r: Rect, c: Point) -> f32 {
    let dx = (r.x - c.x).max(c.x - r.right()).max(0.0);
    let dy = (r.y - c.y).max(c.y - r.bottom()).max(0.0);
    if dx > 0.0 || dy > 0.0 {
        dx.hypot(dy)
    } else {
        (c.x - r.x).min(r.right() - c.x).min(c.y - r.y).min(r.bottom() - c.y)
    }
}

/// The host (structured class or part) of `view` whose border is nearest to `c`, if one is
/// within `reach` of it; ties go to the smaller one, so a port on a part sticking out of its
/// class stays on the part.
fn nearest_host<'v>(
    model: &Model,
    view: &'v View,
    c: Point,
    reach: f32,
    skip: Option<Id>,
) -> Option<&'v Presentation> {
    view.presentations()
        .iter()
        .filter(|p| Some(p.id) != skip && element_kind(model, p).is_some_and(is_host))
        .filter_map(|p| Some((p, p.bounds()?)))
        .map(|(p, r)| (p, border_distance(r, c), r.width * r.height))
        .filter(|(_, d, _)| *d <= reach)
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)))
        .map(|(p, _, _)| p)
}

/// The host whose border the port node `port` of `view` sits on.
pub fn port_host<'v>(
    model: &Model,
    view: &'v View,
    port: &Presentation,
) -> Option<&'v Presentation> {
    let c = port.bounds()?.center();
    nearest_host(model, view, c, PORT_SIZE / 2.0 + 0.5, Some(port.id))
}

/// Where along the border of `r` the point nearest to `c` lies: the side (0 top, 1 right,
/// 2 bottom, 3 left) and how far from the side's top or left end. A port keeps that distance
/// when its host grows, so it stays level with what it connects to.
pub(crate) fn border_place(r: Rect, c: Point) -> (u8, f32) {
    let to = [c.y - r.y, r.right() - c.x, r.bottom() - c.y, c.x - r.x];
    let side = (0..4).min_by(|&a, &b| to[a].abs().total_cmp(&to[b].abs())).unwrap_or(0) as u8;
    let t = match side {
        0 | 2 => (c.x - r.x).clamp(0.0, r.width),
        _ => (c.y - r.y).clamp(0.0, r.height),
    };
    (side, t)
}

/// The side (see [`border_place`]) of the one among `hosts` whose border a port centered at `c`
/// sits on.
pub fn side_on(hosts: &[Rect], c: Point) -> Option<u8> {
    hosts
        .iter()
        .map(|&r| (r, border_distance(r, c)))
        .filter(|(_, d)| *d <= PORT_SIZE / 2.0 + 0.5)
        .min_by(|a, b| {
            a.1.total_cmp(&b.1).then((a.0.width * a.0.height).total_cmp(&(b.0.width * b.0.height)))
        })
        .map(|(r, _)| border_place(r, c).0)
}

/// The point on the border of `r` at `place` (see [`border_place`]).
pub(crate) fn border_point(r: Rect, (side, t): (u8, f32)) -> Point {
    match side {
        0 => Point::new(r.x + t.min(r.width), r.y),
        1 => Point::new(r.right(), r.y + t.min(r.height)),
        2 => Point::new(r.x + t.min(r.width), r.bottom()),
        _ => Point::new(r.x, r.y + t.min(r.height)),
    }
}

/// `port` moved so that its center lies on the border of `host`, where the border is nearest.
pub fn snap(port: Rect, host: Rect) -> Rect {
    let at = border_point(host, border_place(host, port.center()));
    centered(at, port)
}

pub(crate) fn centered(at: Point, r: Rect) -> Rect {
    Rect::new(at.x - r.width / 2.0, at.y - r.height / 2.0, r.width, r.height)
}

/// Room left around a part inside its class.
const MARGIN: f32 = 20.0;

/// How close to a border a new port must be dropped to go on it rather than on the class or
/// part drawn around the point.
const NEAR_BORDER: f32 = 10.0;

/// Where a new node of `tool` goes on `host` (a node of `diagram`) when no place is given: a
/// part below the class's name, right of the parts there or in a new row; a stub state along
/// the bottom of its submachine state; a port on the first free place along the left border,
/// then the right, top and bottom ones (exit points and output pins start on the right). A pin
/// placed here knows nothing of the texts it will show; once it has its name and properties,
/// [`crate::activity::place_pin`] finds it a place clear of the other pins' texts.
pub fn free_spot(model: &Model, diagram: Id, host: Id, tool: NodeTool) -> Option<Point> {
    let view = model.view(diagram)?;
    let r = view.get(host)?.bounds()?;
    let others: Vec<Rect> =
        view.presentations().iter().filter(|p| p.id != host).filter_map(|p| p.bounds()).collect();
    let size = tool.default_size();
    match tool {
        NodeTool::StubState => {
            let y = r.bottom() - size.height - 5.0;
            let right = others
                .iter()
                .filter(|o| r.contains_rect(o) && o.bottom() > y)
                .map(|o| o.right())
                .fold(r.x, f32::max);
            Some(Point::new(right + 10.0, y))
        }
        NodeTool::Part => {
            let inside: Vec<&Rect> = others.iter().filter(|o| r.contains_rect(o)).collect();
            let top = r.y + 30.0 + MARGIN;
            let row = inside.iter().map(|o| o.y).fold(top, f32::max);
            let right =
                inside.iter().filter(|o| o.y >= row - 0.5).map(|o| o.right()).fold(r.x, f32::max);
            if right + MARGIN + size.width <= r.right() - MARGIN || right == r.x {
                Some(Point::new(right + MARGIN, row))
            } else {
                let bottom = inside.iter().map(|o| o.bottom()).fold(top, f32::max);
                Some(Point::new(r.x + MARGIN, bottom + MARGIN))
            }
        }
        _ => {
            // One port every `step` along the border, away from the ports there.
            let step = PORT_SIZE + 16.0;
            let ports: Vec<Point> = others
                .iter()
                .filter(|o| o.width <= crate::scene::POINT_SIZE + 0.5)
                .map(|o| o.center())
                .collect();
            let free = |c: Point| ports.iter().all(|p| p.distance(c) >= step - 0.5);
            let along = |from: f32, to: f32| {
                let n = ((to - from - step) / step).floor().max(0.0) as usize;
                (1..=n).map(move |i| from + i as f32 * step)
            };
            // Output pins and exit points look for room on the right first, everything else on
            // the left. Pins and entry and exit points try the middle of a side first, as
            // actions and states are often too low for two, and names side by side along the
            // top would run into each other.
            let (first, second) = match tool {
                NodeTool::OutputPin | NodeTool::ExitPoint => (r.right(), r.x),
                _ => (r.x, r.right()),
            };
            let pin = matches!(
                tool,
                NodeTool::InputPin
                    | NodeTool::OutputPin
                    | NodeTool::EntryPoint
                    | NodeTool::ExitPoint
            );
            let middles = [Point::new(first, r.center().y), Point::new(second, r.center().y)];
            let spots = middles
                .into_iter()
                .filter(|_| pin)
                .chain(along(r.y, r.bottom()).map(|y| Point::new(first, y)))
                .chain(along(r.y, r.bottom()).map(|y| Point::new(second, y)))
                .chain(along(r.x, r.right()).map(|x| Point::new(x, r.y)))
                .chain(along(r.x, r.right()).map(|x| Point::new(x, r.bottom())));
            let c = spots.into_iter().find(|&c| free(c)).unwrap_or(Point::new(first, r.center().y));
            Some(Point::new(c.x - size.width / 2.0, c.y - size.height / 2.0))
        }
    }
}

/// Creates a part or a port with `tool` at `origin` on the composite structure diagram
/// `diagram`. A part becomes an attribute of the class drawn around the point, which grows to
/// hold it; a port goes on the border of the class or part there, and belongs to the class or
/// to the part's type.
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
    let mut grow = None;
    let owner = match tool {
        NodeTool::Part => {
            let class = innermost_at(model, view, at, is_structured)
                .ok_or_else(|| ModelError::Invalid(PART_OUTSIDE.into()))?;
            if let Some(r) = class.bounds() {
                let around = r.union(&bounds.inflate(MARGIN));
                grow = (around != r).then_some((class.id, around));
            }
            class.element.ok_or(ModelError::NotFound(class.id))?
        }
        _ => {
            // A border near the point first, so a port dropped on the edge of a part goes on
            // the part and not on the class around it.
            let host = nearest_host(model, view, at, NEAR_BORDER, None)
                .or_else(|| innermost_at(model, view, at, is_host))
                .or_else(|| nearest_host(model, view, at, f32::INFINITY, None))
                .ok_or_else(|| ModelError::Invalid(PORT_OUTSIDE.into()))?;
            let element = host.element.ok_or(ModelError::NotFound(host.id))?;
            if let Some(r) = host.bounds() {
                bounds = snap(bounds, r);
            }
            match model.element(element)?.kind() {
                ElementKind::Attribute(_) => part_type(model, element)
                    .ok_or_else(|| ModelError::Invalid(PORT_ON_UNTYPED_PART.into()))?,
                _ => element,
            }
        }
    };
    let name = unique_name(model, owner, base_name);
    let mut data = ElementData::new(name, tool.element_kind());
    // Parts are private, like the ends of associations and Astah's parts.
    if tool == NodeTool::Part {
        data.visibility = Visibility::Private;
    }
    let element = tx.create(owner, data)?;
    if let Some((class, around)) = grow {
        tx.set_bounds(diagram, class, around)?;
    }
    let node = tx.add_node(diagram, Some(element), bounds)?;
    Ok((element, node))
}

/// The end of a connector at the node `p` of `view`: its part or port, and for a port on a part
/// that part.
fn connector_end(model: &Model, view: &View, p: &Presentation) -> Result<ConnectorEnd> {
    let role = p.element.ok_or(ModelError::NotFound(p.id))?;
    let part_with_port = match model.element(role)?.kind() {
        ElementKind::Port(_) => port_host(model, view, p)
            .and_then(|host| host.element)
            .filter(|&e| matches!(model.get(e).map(|e| e.kind()), Some(ElementKind::Attribute(_)))),
        ElementKind::Attribute(_) => None,
        other => {
            return Err(ModelError::Invalid(format!(
                "a connector cannot connect a {}",
                other.type_name()
            )));
        }
    };
    Ok(ConnectorEnd::new(role, part_with_port))
}

/// Connects the part or port nodes `source` and `target` of `diagram` with a connector, owned by
/// the class that holds the parts. Returns the new edge.
pub fn connect(tx: &mut Tx<'_>, diagram: Id, source: Id, target: Id) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let node = |id: Id| view.get(id).ok_or(ModelError::NotFound(id));
    let ends =
        [connector_end(model, view, node(source)?)?, connector_end(model, view, node(target)?)?];
    if ends[0].role == ends[1].role && ends[0].part_with_port == ends[1].part_with_port {
        return Err(ModelError::Invalid("a connector needs two different ends".into()));
    }
    // The class that holds the parts: the owner of a part, of the part a port is reached
    // through, or of a port of the class itself.
    let kind = ElementKind::connector(ends[0].clone(), ends[1].clone());
    let owner = ends
        .iter()
        .filter_map(|end| model.get(end.part_with_port.unwrap_or(end.role))?.owner())
        .find(|&o| model.get(o).is_some_and(|o| o.kind().can_own(&kind)))
        .ok_or_else(|| ModelError::Invalid("no class holds both ends of the connector".into()))?;
    let element = tx.create(owner, ElementData::new("", kind))?;
    tx.add_edge(diagram, Some(element), source, target, Vec::new())
}

/// What keeps to a border: a port to the border of its class or part, a pin just outside the
/// border of its action, an activity parameter node to the diagram's frame, an entry or exit
/// point to the border of its state or of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Borne {
    Port,
    Pin,
    Parameter,
    Point,
}

impl Borne {
    /// How far outside its host's border the node's center lies.
    fn reach(self) -> f32 {
        match self {
            Self::Pin => crate::scene::PIN_SIZE / 2.0,
            Self::Port | Self::Parameter | Self::Point => 0.0,
        }
    }
}

/// What kind of border node `p` of a diagram of `kind` is, if any.
fn borne_kind(model: &Model, kind: Option<DiagramKind>, p: &Presentation) -> Option<Borne> {
    match (kind?, element_kind(model, p)?) {
        (DiagramKind::CompositeStructure, ElementKind::Port(_)) => Some(Borne::Port),
        (DiagramKind::Activity, ElementKind::ActivityNode(n)) if n.node.is_pin() => {
            Some(Borne::Pin)
        }
        (DiagramKind::Activity, ElementKind::ActivityNode(n))
            if n.node == ActivityNodeKind::ActivityParameter =>
        {
            Some(Borne::Parameter)
        }
        (DiagramKind::StateMachine, ElementKind::Pseudostate(s))
            if s.pseudostate.is_border_point() =>
        {
            Some(Borne::Point)
        }
        _ => None,
    }
}

/// The node of `view` that the pin, activity parameter node, entry or exit point `p` keeps
/// to: the node of the pin's action or the point's state nearest to it, or the diagram's
/// frame for a parameter and a point of the state machine itself.
fn owner_host(model: &Model, view: &View, diagram: Id, p: &Presentation) -> Option<Id> {
    let element = model.get(p.element?)?;
    let c = p.bounds()?.center();
    let host = match element.kind() {
        ElementKind::ActivityNode(n) if n.node.is_pin() => element.owner()?,
        ElementKind::ActivityNode(_) => diagram,
        ElementKind::Pseudostate(_) => {
            let owner = element.owner()?;
            match model.get(owner)?.kind() {
                ElementKind::State(_) => owner,
                _ => diagram,
            }
        }
        _ => return None,
    };
    view.presentations()
        .iter()
        .filter(|h| h.element == Some(host))
        .filter_map(|h| Some((h.id, h.bounds()?)))
        .min_by(|a, b| border_distance(a.1, c).total_cmp(&border_distance(b.1, c)))
        .map(|(id, _)| id)
}

/// The pins and activity parameter nodes of the activity diagram `diagram`, or the entry and
/// exit points of the state machine diagram `diagram`, each with the node it keeps to (its
/// action's or state's, or the frame), if that is shown.
pub fn borne_nodes(model: &Model, diagram: Id) -> std::collections::HashMap<Id, Option<Id>> {
    let kind = model.get(diagram).and_then(|d| d.kind().as_diagram());
    let bordered = matches!(kind, Some(DiagramKind::Activity | DiagramKind::StateMachine));
    let Some(view) = model.view(diagram).filter(|_| bordered) else {
        return Default::default();
    };
    view.presentations()
        .iter()
        .filter(|p| borne_kind(model, kind, p).is_some())
        .map(|p| (p.id, owner_host(model, view, diagram, p)))
        .collect()
}

/// The host of the border node `p` of `view` and the line its center keeps to.
fn track(
    model: &Model,
    view: &View,
    diagram: Id,
    p: &Presentation,
    borne: Borne,
) -> Option<(Id, Rect)> {
    let host = match borne {
        Borne::Port => port_host(model, view, p)?.id,
        Borne::Pin | Borne::Parameter | Borne::Point => owner_host(model, view, diagram, p)?,
    };
    Some((host, view.get(host)?.bounds()?.inflate(borne.reach())))
}

/// Where a border node sat before an edit: on which host, where along its border, and where
/// the node and the host's track were.
#[derive(Clone, Debug)]
pub struct PortPlace {
    diagram: Id,
    port: Id,
    borne: Borne,
    center: Point,
    host: Option<(Id, Rect)>,
}

/// Where the ports and parts of composite structure diagrams and the pins and activity
/// parameter nodes of activity diagrams were before an edit, to [`follow`] after it.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    ports: Vec<PortPlace>,
    parts: Vec<(Id, Id, Rect)>,
}

/// Whether `p` shows an entry or exit point of a state machine itself, which sits on the frame.
fn on_frame(model: &Model, p: &Presentation) -> bool {
    let owner = p.element.and_then(|e| model.get(e)).and_then(|e| e.owner());
    owner.and_then(|o| model.get(o)).is_some_and(|o| matches!(o.kind(), ElementKind::StateMachine))
}

fn is_part(model: &Model, p: &Presentation) -> bool {
    matches!(element_kind(model, p), Some(ElementKind::Attribute(_)))
}

/// Composite structure, activity and state machine diagrams, with their kind.
fn bordered_views(model: &Model) -> impl Iterator<Item = (Id, &View, Option<DiagramKind>)> {
    model.views().filter_map(|(d, view)| {
        let kind = model.get(d).and_then(|d| d.kind().as_diagram());
        matches!(
            kind,
            Some(
                DiagramKind::CompositeStructure | DiagramKind::Activity | DiagramKind::StateMachine
            )
        )
        .then_some((d, view, kind))
    })
}

/// The places of all ports and parts on composite structure diagrams, and of all pins and
/// activity parameter nodes on activity diagrams.
pub fn snapshots(model: &Model) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (diagram, view, kind) in bordered_views(model) {
        for p in view.presentations() {
            let Some(r) = p.bounds() else { continue };
            if let Some(borne) = borne_kind(model, kind, p) {
                let host = track(model, view, diagram, p, borne);
                snapshot.ports.push(PortPlace {
                    diagram,
                    port: p.id,
                    borne,
                    center: r.center(),
                    host,
                });
            } else if kind == Some(DiagramKind::CompositeStructure) && is_part(model, p) {
                snapshot.parts.push((diagram, p.id, r));
            }
        }
    }
    snapshot
}

/// Keeps parts inside their class, ports on the border of their hosts, pins on the border of
/// their actions and activity parameter nodes on the frame after an edit.
///
/// A class grows around a part moved or resized out of it. A port or pin that did not move
/// keeps its place along the border of a host that moved or changed size, and goes along when
/// its host is removed from the diagram; one that moved, or a new one, goes onto the nearest
/// border: a port onto that of any class or part, a pin onto its own action's, an activity
/// parameter node onto the frame.
pub fn follow(tx: &mut Tx<'_>, before: &Snapshot) -> Result<()> {
    hold_parts(tx, &before.parts)?;
    follow_ports(tx, &before.ports)
}

/// Grows the class shown around a part that moved or changed size so that it holds the part.
fn hold_parts(tx: &mut Tx<'_>, before: &[(Id, Id, Rect)]) -> Result<()> {
    let model = tx.model();
    let mut grow: Vec<(Id, Id, Rect)> = Vec::new();
    for (diagram, view) in model.views().filter(|(d, _)| is_composite(model, *d)) {
        for p in view.presentations().iter().filter(|p| is_part(model, p)) {
            let Some(r) = p.bounds() else { continue };
            if before.iter().any(|&(d, id, was)| d == diagram && id == p.id && was == r) {
                continue;
            }
            let Some(owner) = p.element.and_then(|e| model.get(e)?.owner()) else { continue };
            let class = view
                .presentations()
                .iter()
                .filter(|c| c.element == Some(owner))
                .filter_map(|c| Some((c.id, c.bounds()?)))
                .max_by(|a, b| overlap(a.1, r).total_cmp(&overlap(b.1, r)));
            let Some((class, bounds)) = class else { continue };
            let bounds =
                grow.iter().find(|g| g.0 == diagram && g.1 == class).map_or(bounds, |g| g.2);
            if !bounds.contains_rect(&r) {
                let around = bounds.union(&r.inflate(MARGIN));
                grow.retain(|g| !(g.0 == diagram && g.1 == class));
                grow.push((diagram, class, around));
            }
        }
    }
    for (diagram, class, bounds) in grow {
        tx.set_bounds(diagram, class, bounds)?;
    }
    Ok(())
}

/// How much of `a` and `b` overlap.
fn overlap(a: Rect, b: Rect) -> f32 {
    let w = a.right().min(b.right()) - a.x.max(b.x);
    let h = a.bottom().min(b.bottom()) - a.y.max(b.y);
    w.max(0.0) * h.max(0.0)
}

fn follow_ports(tx: &mut Tx<'_>, before: &[PortPlace]) -> Result<()> {
    let model = tx.model();
    let mut moves = Vec::new();
    let mut removed = Vec::new();
    let mut unplaced = Vec::new();
    for (diagram, view, kind) in bordered_views(model) {
        for p in view.presentations() {
            let Some(borne) = borne_kind(model, kind, p) else { continue };
            let Some(r) = p.bounds() else { continue };
            let c = r.center();
            let old = before.iter().find(|b| b.diagram == diagram && b.port == p.id);
            let still = old.is_some_and(|o| o.center.distance(c) < 0.01 && o.borne == borne);
            let target = match old.and_then(|o| o.host).filter(|_| still) {
                Some((host, was)) => {
                    match view.get(host).and_then(|h| h.bounds()).map(|h| h.inflate(borne.reach()))
                    {
                        Some(now) if now == was => continue,
                        Some(now) => centered(border_point(now, border_place(was, c)), r),
                        // An activity parameter node or an entry or exit point of the state
                        // machine stays where it is without its frame.
                        None if borne == Borne::Parameter || on_frame(model, p) => continue,
                        None => {
                            removed.push((diagram, p.id));
                            continue;
                        }
                    }
                }
                None => {
                    let host = match borne {
                        Borne::Port => nearest_host(model, view, c, f32::INFINITY, Some(p.id))
                            .and_then(|h| Some((h.id, h.bounds()?))),
                        Borne::Pin | Borne::Parameter | Borne::Point => {
                            track(model, view, diagram, p, borne)
                        }
                    };
                    match host {
                        Some((_, now)) => {
                            let target = snap(r, now);
                            // A name placed beside the old side would now stand in the way.
                            let side = |host: Rect, c: Point| border_place(host, c).0;
                            let turned = old.and_then(|o| Some((o.host?.1, o.center))).is_some_and(
                                |(was, at)| side(was, at) != side(now, target.center()),
                            );
                            if turned && p.style.name_offset.is_some() {
                                unplaced.push((diagram, p.id));
                            }
                            target
                        }
                        None => continue,
                    }
                }
            };
            if target.origin().distance(r.origin()) > 0.01 {
                moves.push((diagram, p.id, target));
            }
        }
    }
    for (diagram, port, bounds) in moves {
        tx.set_bounds(diagram, port, bounds)?;
    }
    for (diagram, port) in unplaced {
        tx.update_presentation(diagram, port, |p| p.style.name_offset = None)?;
    }
    for (diagram, port) in removed {
        tx.remove_presentation(diagram, port)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{self, EdgeTool};
    use umlcraft_model::{Document, Size};

    fn diagram() -> (Document, Id, Id) {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let diagram = doc
            .edit("d", |tx| {
                crate::edit::create_diagram(tx, root, DiagramKind::CompositeStructure, "Car")
            })
            .unwrap();
        (doc, root, diagram)
    }

    #[test]
    fn border_places_survive_a_resize() {
        let r = Rect::new(0.0, 0.0, 100.0, 50.0);
        assert_eq!(border_place(r, Point::new(100.0, 25.0)), (1, 25.0));
        assert_eq!(border_place(r, Point::new(30.0, 52.0)), (2, 30.0));
        // Wider and higher, a port on the right keeps its height; smaller, it stays on it.
        let wider = Rect::new(10.0, 0.0, 200.0, 100.0);
        assert_eq!(border_point(wider, (1, 25.0)), Point::new(210.0, 25.0));
        assert_eq!(
            border_point(Rect::new(0.0, 0.0, 50.0, 20.0), (1, 25.0)),
            Point::new(50.0, 20.0)
        );
        assert_eq!(snap(Rect::new(40.0, 10.0, 14.0, 14.0), r).center(), Point::new(47.0, 0.0));
    }

    #[test]
    fn parts_ports_and_connectors() {
        let (mut doc, _, diagram) = diagram();
        let measure = crate::scene::ApproxMeasure;
        let class = crate::edit::apply(&mut doc, "class", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Class, Point::new(0.0, 0.0), "Car")
        })
        .unwrap();
        let (car, car_node) = class;
        let view = doc.model().view(diagram).unwrap();
        let car_bounds = view.get(car_node).unwrap().bounds().unwrap();
        assert!(car_bounds.width >= 300.0, "room for parts: {car_bounds:?}");
        assert!(!view.get(car_node).unwrap().style.auto_size);

        let outside = crate::edit::apply(&mut doc, "part", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Part, Point::new(900.0, 0.0), "part")
        });
        assert!(outside.is_err(), "a part needs a class around it");

        let engine_class = doc
            .edit("Engine", |tx| {
                let root = tx.model().root();
                tx.create(root, ElementData::new("Engine", ElementKind::class()))
            })
            .unwrap();
        let (engine, engine_node) = crate::edit::apply(&mut doc, "part", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Part, Point::new(40.0, 60.0), "part")
        })
        .unwrap();
        assert_eq!(doc.model().element(engine).unwrap().owner(), Some(car));
        let unsupported = crate::edit::apply(&mut doc, "port", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Port, Point::new(45.0, 65.0), "port")
        });
        assert!(unsupported.is_err(), "an untyped part has no ports");
        doc.edit("type", |tx| {
            tx.update(engine, |d| {
                d.kind = ElementKind::attribute(Some(TypeRef::Element(engine_class)));
            })
        })
        .unwrap();

        // A port dropped near the part's left edge goes onto it and belongs to Engine.
        let engine_bounds = doc.model().view(diagram).unwrap().get(engine_node).unwrap().bounds();
        let engine_bounds = engine_bounds.unwrap();
        let near = Point::new(engine_bounds.x - 3.0, engine_bounds.y + 10.0);
        let (shaft, shaft_node) = crate::edit::apply(&mut doc, "port", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Port, near, "port")
        })
        .unwrap();
        assert_eq!(doc.model().element(shaft).unwrap().owner(), Some(engine_class));
        let shaft_bounds = doc.model().view(diagram).unwrap().get(shaft_node).unwrap().bounds();
        assert_eq!(shaft_bounds.unwrap().center().x, engine_bounds.x, "on the left edge");

        // Just outside the part's right edge, still within the class, is still the part's.
        let outside = Point::new(engine_bounds.right() + 3.0 - PORT_SIZE / 2.0, near.y);
        let (port, port_node) = crate::edit::apply(&mut doc, "port", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Port, outside, "port")
        })
        .unwrap();
        assert_eq!(doc.model().element(port).unwrap().owner(), Some(engine_class));
        let port_bounds = doc.model().view(diagram).unwrap().get(port_node).unwrap().bounds();
        assert_eq!(port_bounds.unwrap().center().x, engine_bounds.right());
        assert!(doc.undo());

        // A port of the car itself, on its right edge.
        let right = Point::new(car_bounds.right() - 2.0, car_bounds.y + 40.0);
        let (_, outer_node) = crate::edit::apply(&mut doc, "port", &measure, |tx| {
            palette::create_node(tx, diagram, NodeTool::Port, right, "port")
        })
        .unwrap();

        let edge = crate::edit::apply(&mut doc, "connect", &measure, |tx| {
            palette::connect(tx, diagram, EdgeTool::Connector, outer_node, shaft_node)
        })
        .unwrap();
        let model = doc.model();
        let connector = model.view(diagram).unwrap().get(edge).unwrap().element.unwrap();
        let connector = model.element(connector).unwrap();
        assert_eq!(connector.owner(), Some(car));
        let ElementKind::Connector(p) = connector.kind() else { panic!() };
        assert_eq!(p.ends[0].part_with_port, None);
        assert_eq!(p.ends[1].part_with_port, Some(engine), "reached through the part");

        // Moving and widening the part takes its port along, at the same place on its border.
        let moved = Rect::new(engine_bounds.x + 20.0, engine_bounds.y + 30.0, 200.0, 80.0);
        crate::edit::apply(&mut doc, "move", &measure, |tx| {
            tx.set_bounds(diagram, engine_node, moved)
        })
        .unwrap();
        let view = doc.model().view(diagram).unwrap();
        let shaft_bounds = view.get(shaft_node).unwrap().bounds().unwrap();
        assert_eq!(shaft_bounds.center().x, moved.x);
        assert_eq!(shaft_bounds.size(), Size::new(PORT_SIZE, PORT_SIZE));

        // A part moved out over the class's border takes the border along.
        let out = moved.translate(0.0, 100.0);
        crate::edit::apply(&mut doc, "move out", &measure, |tx| {
            tx.set_bounds(diagram, engine_node, out)
        })
        .unwrap();
        let grown = doc.model().view(diagram).unwrap().get(car_node).unwrap().bounds().unwrap();
        assert_eq!(grown.bottom(), out.bottom() + MARGIN);
        assert_eq!(grown.origin(), car_bounds.origin());
        crate::edit::apply(&mut doc, "move in", &measure, |tx| {
            tx.set_bounds(diagram, engine_node, moved)
        })
        .unwrap();
        let shaft_bounds = doc.model().view(diagram).unwrap().get(shaft_node).unwrap().bounds();
        let shaft_bounds = shaft_bounds.unwrap();

        // A port dragged away by itself lands on the nearest border.
        let away = shaft_bounds.translate(0.0, -500.0);
        crate::edit::apply(&mut doc, "drag", &measure, |tx| {
            tx.set_bounds(diagram, shaft_node, away)
        })
        .unwrap();
        let view = doc.model().view(diagram).unwrap();
        let landed = view.get(shaft_node).unwrap().bounds().unwrap().center();
        assert!(border_distance(view.get(car_node).unwrap().bounds().unwrap(), landed) < 0.01);

        // Removing the part from the diagram takes the ports shown on it along.
        crate::edit::apply(&mut doc, "move back", &measure, |tx| {
            tx.set_bounds(diagram, shaft_node, snap(away, moved))
        })
        .unwrap();
        crate::edit::apply(&mut doc, "remove", &measure, |tx| {
            tx.remove_presentation(diagram, engine_node)
        })
        .unwrap();
        assert!(doc.model().view(diagram).unwrap().get(shaft_node).is_none());
        assert!(doc.model().check().is_ok());
    }
}
