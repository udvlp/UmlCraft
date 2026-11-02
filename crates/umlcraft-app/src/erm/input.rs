//! Mouse and keys of the ER canvas: selecting (click, Shift/Ctrl+click, rubber band), moving and
//! resizing with snapping to the grid (Alt: free), dragging labels, drawing lines from the
//! connection points and hanging line ends elsewhere, panning, zooming, and the commands of
//! keys, palette and context menu. Each mouse action is one undo step: while dragging, the
//! canvas shows a preview, the change is made when the button is let go.

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::edit::{self, End, Item, MIN_SIZE};
use umlcraft_erm::hit::{self, Handle};
use umlcraft_erm::{Diagram, Id, NodeKind, Point as DPoint, Rect as DRect, clip};

use super::canvas::{ErmCanvas, GRID, HANDLE_GRAB, Redraw, band};
use super::palette::{DraggedShape, Shape};
use super::{
    AddAttribute, Duplicate, EditText, FitToText, Grow, Nudge, RelationshipTo, ReleaseEnds,
};
use crate::{Copy, Cut, DeleteSelection, Paste, SelectAll};

/// How far the mouse moves (screen px) before pressing becomes dragging.
const DRAG_START: f32 = 3.0;

/// Where a line being drawn would end.
#[derive(Clone, Debug)]
pub struct Target {
    pub node: Id,
    pub port: Option<DPoint>,
    /// Where the end would be drawn.
    pub point: DPoint,
    pub allowed: bool,
}

pub enum Drag {
    /// Panning: where it started, in window coordinates, and the pan then.
    Pan {
        start: Point<Pixels>,
        pan: Point<Pixels>,
    },
    /// Rubber band; `add` keeps the selection there was (Shift/Ctrl).
    Band {
        start: DPoint,
        current: DPoint,
        add: bool,
    },
    /// The selected nodes and texts, moved by `(dx, dy)` so far. `clicked`: pressed on a
    /// selected element; a click without moving selects it alone.
    Move {
        start: DPoint,
        dx: f64,
        dy: f64,
        moved: bool,
        clicked: Option<Item>,
    },
    Resize {
        item: Item,
        handle: Handle,
        start: DPoint,
        original: DRect,
        bounds: DRect,
    },
    Label {
        link: Id,
        start: DPoint,
        dx: f64,
        dy: f64,
    },
    /// A new line from a connection point (`from_port`) of `from`.
    Connect {
        from: Id,
        from_port: Option<DPoint>,
        from_point: DPoint,
        current: DPoint,
        target: Option<Target>,
    },
    /// One end of a selected line; `fixed` is where the other end is.
    End {
        link: Id,
        end: End,
        fixed: DPoint,
        current: DPoint,
        target: Option<Target>,
    },
}

/// A coordinate on the grid.
pub fn snap(v: f64) -> f64 {
    (v / f64::from(GRID)).round() * f64::from(GRID)
}

/// The box after a grip was dragged, with the moving sides on the grid.
pub fn snap_resized(handle: Handle, b: DRect) -> DRect {
    let (fx, fy) = handle.at();
    let (mut x0, mut y0, mut x1, mut y1) = (b.x, b.y, b.x + b.width, b.y + b.height);
    if fx == 0.0 {
        x0 = snap(x0).min(x1 - MIN_SIZE.0);
    } else if fx == 1.0 {
        x1 = snap(x1).max(x0 + MIN_SIZE.0);
    }
    if fy == 0.0 {
        y0 = snap(y0).min(y1 - MIN_SIZE.1);
    } else if fy == 1.0 {
        y1 = snap(y1).max(y0 + MIN_SIZE.1);
    }
    DRect::new(x0, y0, x1 - x0, y1 - y0)
}

/// The move that puts the top left corner of `b` on the grid after moving it by `(dx, dy)`.
pub fn snap_move(b: &DRect, dx: f64, dy: f64) -> (f64, f64) {
    (snap(b.x + dx) - b.x, snap(b.y + dy) - b.y)
}

fn bounds_of(d: &Diagram, item: &Item) -> Option<DRect> {
    match item {
        Item::Node(id) => d.node(id).map(|n| n.bounds),
        Item::Text(id) => d.texts.iter().find(|t| t.id == *id).map(|t| t.bounds),
        _ => None,
    }
}

impl ErmCanvas {
    fn tolerance(&self) -> f64 {
        f64::from(HANDLE_GRAB / self.zoom)
    }

    /// The nodes and texts of the selection (what moves).
    fn movable(&self) -> Vec<Item> {
        self.selection()
            .iter()
            .filter(|i| matches!(i, Item::Node(_) | Item::Text(_)))
            .cloned()
            .collect()
    }

    /// The node whose box (grown by the tolerance) is under `p`, for connection points.
    fn node_near(&self, p: DPoint) -> Option<Id> {
        let t = self.tolerance();
        self.diagram()
            .nodes
            .iter()
            .rev()
            .find(|n| {
                let b = &n.bounds;
                p.x >= b.x - t
                    && p.y >= b.y - t
                    && p.x <= b.x + b.width + t
                    && p.y <= b.y + b.height + t
            })
            .map(|n| n.id.clone())
    }

    /// Where a line to `p` would end, and whether it may (`trial` makes it on a copy).
    fn target_at(
        &self,
        p: DPoint,
        from: DPoint,
        trial: impl Fn(&mut Diagram, &Id, Option<DPoint>) -> bool,
    ) -> Option<Target> {
        let d = self.diagram();
        let node = hit::node_at(d, p).or_else(|| self.node_near(p).and_then(|id| d.node(&id)))?;
        let (port, point) = match hit::nearest_port(node, p, self.tolerance()) {
            Some((port, at)) => (Some(port), at),
            None => (None, hit::outline_toward(node, from)),
        };
        let mut copy = d.clone();
        let allowed = trial(&mut copy, &node.id, port);
        Some(Target { node: node.id.clone(), port, point, allowed })
    }

    // ----- Mouse -----------------------------------------------------------------------------

    pub(super) fn on_left_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing.is_some() {
            if self.over_text_field(ev.position) {
                return;
            }
            self.commit_edit(window, cx);
        }
        window.focus(&self.focus, cx);
        if self.space {
            self.drag = Some(Drag::Pan { start: ev.position, pan: self.pan });
            cx.notify();
            return;
        }
        let p = self.to_diagram(ev.position);
        let tol = self.tolerance();
        let d = self.diagram().clone();
        let adding = ev.modifiers.shift || ev.modifiers.secondary(cx);

        if ev.click_count >= 2 {
            if let Some(item) = hit::at(&d, p, tol) {
                self.select(vec![item.clone()], cx);
                self.start_edit(item, window, cx);
            }
            return;
        }
        if let Some(first) = self.relating.take() {
            self.finish_relationship(&first, p, cx);
            return;
        }
        // Grips of a single selected box, ends of a single selected line.
        if let [item] = self.selection() {
            let item = item.clone();
            if let Some(b) = bounds_of(&d, &item) {
                let grabbed = Handle::ALL
                    .into_iter()
                    .find(|h| umlcraft_erm::geometry::distance(h.point(&b), p) <= tol);
                if let Some(handle) = grabbed {
                    self.drag =
                        Some(Drag::Resize { item, handle, start: p, original: b, bounds: b });
                    return;
                }
            }
            if let Item::Link(id) = &item
                && let Some((a, b)) = d.link(id).and_then(|l| hit::ends(&d, l))
            {
                let end = if umlcraft_erm::geometry::distance(a, p) <= tol {
                    Some((End::Source, b))
                } else if umlcraft_erm::geometry::distance(b, p) <= tol {
                    Some((End::Target, a))
                } else {
                    None
                };
                if let Some((end, fixed)) = end {
                    self.drag =
                        Some(Drag::End { link: id.clone(), end, fixed, current: p, target: None });
                    return;
                }
            }
        }
        // A connection point starts a line.
        if let Some(id) = self.node_near(p)
            && let Some(node) = d.node(&id)
            && let Some((port, at)) = hit::nearest_port(node, p, tol)
        {
            self.drag = Some(Drag::Connect {
                from: id,
                from_port: Some(port),
                from_point: at,
                current: p,
                target: None,
            });
            cx.notify();
            return;
        }
        match hit::at(&d, p, tol) {
            Some(Item::Label(link)) if !adding => {
                self.select(vec![Item::Label(link.clone())], cx);
                self.drag = Some(Drag::Label { link, start: p, dx: 0.0, dy: 0.0 });
            }
            Some(item) if adding => {
                let mut items = self.selection().to_vec();
                match items.iter().position(|i| *i == item) {
                    Some(at) => {
                        items.remove(at);
                    }
                    None => items.push(item),
                }
                self.select(items, cx);
            }
            Some(item) => {
                let clicked = if self.selection().contains(&item) {
                    Some(item)
                } else {
                    self.select(vec![item], cx);
                    None
                };
                if !self.movable().is_empty() {
                    self.drag =
                        Some(Drag::Move { start: p, dx: 0.0, dy: 0.0, moved: false, clicked });
                }
            }
            None => {
                if !adding {
                    self.select(Vec::new(), cx);
                }
                self.drag = Some(Drag::Band { start: p, current: p, add: adding });
            }
        }
        cx.notify();
    }

    pub(super) fn on_right_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_edit(window, cx);
        window.focus(&self.focus, cx);
        let p = self.to_diagram(ev.position);
        let hit = hit::at(self.diagram(), p, self.tolerance());
        match hit {
            Some(item) if !self.selection().contains(&item) => self.select(vec![item], cx),
            None => self.select(Vec::new(), cx),
            _ => {}
        }
    }

    pub(super) fn on_middle_down(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.drag = Some(Drag::Pan { start: ev.position, pan: self.pan });
        cx.notify();
    }

    pub(super) fn on_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.drag.is_some() {
            self.drag_to(ev, window, cx);
            return;
        }
        let hover = self.node_near(self.to_diagram(ev.position));
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    pub(super) fn drag_to(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let p = self.to_diagram(ev.position);
        let free = ev.modifiers.alt;
        let screen = |dx: f64, dy: f64, zoom: f32| (dx.abs().max(dy.abs()) as f32) * zoom;
        let zoom = self.zoom;
        let d = self.diagram().clone();
        let mut preview: Option<Diagram> = None;
        let selection = self.movable();
        // Computed apart from `self.drag` where they need `self`.
        let connect_target = match &self.drag {
            Some(Drag::Connect { from, from_port, from_point, .. }) => {
                let (from, from_port, from_point) = (from.clone(), *from_port, *from_point);
                Some(self.target_at(p, from_point, |d, node, port| {
                    edit::connect(d, &from, node, (from_port, port)).is_ok()
                }))
            }
            Some(Drag::End { link, end, fixed, .. }) => {
                let (link, end, fixed) = (link.clone(), *end, *fixed);
                Some(self.target_at(p, fixed, |d, node, port| {
                    edit::reconnect(d, &link, end, node, port).is_ok()
                }))
            }
            _ => None,
        };
        match &mut self.drag {
            None => return,
            Some(Drag::Pan { start, pan }) => {
                self.pan = point(pan.x + ev.position.x - start.x, pan.y + ev.position.y - start.y);
                self.auto_fit = false;
            }
            Some(Drag::Band { current, .. }) => *current = p,
            Some(Drag::Move { start, dx, dy, moved, .. }) => {
                let (rx, ry) = (p.x - start.x, p.y - start.y);
                if !*moved && screen(rx, ry, zoom) < DRAG_START {
                    return;
                }
                *moved = true;
                let (mx, my) = match hit::bounds_of(&d, &selection) {
                    Some(b) if !free => snap_move(&b, rx, ry),
                    _ => (rx, ry),
                };
                (*dx, *dy) = (mx, my);
                let mut p = d.clone();
                if edit::move_items(&mut p, &selection, mx, my).is_ok() {
                    preview = Some(p);
                }
            }
            Some(Drag::Resize { item, handle, start, original, bounds }) => {
                let shift = ev.modifiers.shift;
                let r = handle.resize(original, p.x - start.x, p.y - start.y, MIN_SIZE, shift);
                *bounds = if free || shift { r } else { snap_resized(*handle, r) };
                let mut p = d.clone();
                if edit::set_bounds(&mut p, item, *bounds).is_ok() {
                    preview = Some(p);
                }
            }
            Some(Drag::Label { link, start, dx, dy }) => {
                (*dx, *dy) = ((p.x - start.x).round(), (p.y - start.y).round());
                let mut p = d.clone();
                if edit::move_label(&mut p, link, *dx, *dy).is_ok() {
                    preview = Some(p);
                }
            }
            Some(Drag::Connect { current, target, .. } | Drag::End { current, target, .. }) => {
                *current = p;
                *target = connect_target.flatten();
            }
        }
        if let Some(p) = preview {
            self.preview = Some(p);
            self.changed(Redraw::Later);
        }
        cx.notify();
    }

    pub(super) fn on_mouse_up(
        &mut self,
        ev: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.end_drag(ev, window, cx);
    }

    pub(super) fn end_drag(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else { return };
        let had_preview = self.preview.take().is_some();
        let selection = self.movable();
        match drag {
            Drag::Pan { .. } => {}
            Drag::Band { start, current, add } => {
                let mut items = if add { self.selection().to_vec() } else { Vec::new() };
                items.extend(hit::inside(self.diagram(), &band(start, current)));
                self.select(items, cx);
            }
            Drag::Move { moved: false, clicked: Some(item), .. } => self.select(vec![item], cx),
            Drag::Move { moved: true, dx, dy, .. } if dx != 0.0 || dy != 0.0 => {
                self.apply(cx, |d| edit::move_items(d, &selection, dx, dy).map(|()| None));
            }
            Drag::Resize { item, original, bounds, .. } if bounds != original => {
                self.apply(cx, |d| edit::set_bounds(d, &item, bounds).map(|()| None));
            }
            Drag::Label { link, dx, dy, .. } if dx != 0.0 || dy != 0.0 => {
                self.apply(cx, |d| edit::move_label(d, &link, dx, dy).map(|()| None));
            }
            Drag::Connect { from, from_port, target: Some(t), .. } if t.allowed => {
                self.apply(cx, |d| {
                    let id = edit::connect(d, &from, &t.node, (from_port, t.port))?;
                    Ok(Some(vec![Item::Link(id)]))
                });
            }
            Drag::Connect { target: Some(t), .. } | Drag::End { target: Some(t), .. }
                if !t.allowed =>
            {
                self.refused = Some(t!("erm.cannot_join").to_string());
            }
            Drag::End { link, end, target: Some(t), .. } => {
                self.apply(cx, |d| edit::reconnect(d, &link, end, &t.node, t.port).map(|()| None));
            }
            _ => {}
        }
        if had_preview {
            self.changed(Redraw::Now);
        }
        cx.notify();
    }

    fn kind_of(&self, id: &Id) -> Option<NodeKind> {
        self.diagram().node(id).map(|n| n.kind)
    }

    pub(super) fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let mut delta = ev.delta.pixel_delta(px(20.0));
        if ev.modifiers.secondary(cx) {
            let origin = self.area.get().origin;
            let anchor = point(ev.position.x - origin.x, ev.position.y - origin.y);
            let factor = (1.0 + f32::from(delta.y) / 200.0).clamp(0.5, 2.0);
            self.set_zoom(self.zoom * factor, anchor, cx);
            return;
        }
        // Mice without a horizontal wheel scroll sideways with Shift.
        if ev.modifiers.shift && delta.x == px(0.0) {
            delta = point(delta.y, px(0.0));
        }
        self.pan = point(self.pan.x + delta.x, self.pan.y + delta.y);
        self.auto_fit = false;
        cx.notify();
    }

    pub(super) fn on_key_down(
        &mut self,
        ev: &KeyDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if ev.keystroke.key == "space" && !self.space && self.editing.is_none() {
            self.space = true;
            cx.notify();
        }
    }

    pub(super) fn on_key_up(&mut self, ev: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "space" {
            self.space = false;
            cx.notify();
        }
    }

    /// A shape dropped from the palette.
    pub(super) fn on_drop(
        &mut self,
        dragged: &DraggedShape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let p = self.to_diagram(window.mouse_position());
        self.add_shape(dragged.0, DPoint::new(snap(p.x), snap(p.y)), false, window, cx);
    }

    // ----- Commands ----------------------------------------------------------------------------

    /// Puts a new shape at `center`; an attribute clicked in the palette goes to the selected
    /// node instead (`to_selection`).
    fn add_shape(
        &mut self,
        shape: Shape,
        center: DPoint,
        to_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let name = shape.default_name();
        let owner = match (shape.kind(), self.selection()) {
            (Some(NodeKind::Attribute { .. }), [Item::Node(id)]) if to_selection => {
                Some(id.clone())
            }
            _ => None,
        };
        self.apply(cx, |d| {
            let id = match (shape.kind(), owner) {
                (Some(kind), Some(owner)) => {
                    let id = edit::add_attribute(d, &owner, &name)?;
                    edit::set_kind(d, &id, kind)?;
                    Item::Node(id)
                }
                (Some(kind), None) => Item::Node(edit::add_node(d, kind, &name, center)),
                (None, _) => {
                    let (w, h) = umlcraft_erm::create::TEXT_SIZE;
                    let at = DPoint::new(center.x - w / 2.0, center.y - h / 2.0);
                    Item::Text(edit::add_text(d, &name, at))
                }
            };
            Ok(Some(vec![id]))
        });
    }

    /// The palette's click: into the middle of the view, beside what is there already.
    pub(super) fn place_shape(
        &mut self,
        shape: Shape,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let c = self.visible_centre();
        let mut at = DPoint::new(snap(c.x), snap(c.y));
        for _ in 0..20 {
            if hit::node_at(self.diagram(), at).is_none() {
                break;
            }
            at = DPoint::new(at.x + 2.0 * f64::from(GRID), at.y + 2.0 * f64::from(GRID));
        }
        self.add_shape(shape, at, true, window, cx);
    }

    pub(super) fn delete(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let items = self.selection().to_vec();
        if items.is_empty() {
            return;
        }
        self.apply(cx, |d| edit::delete(d, &items).map(|()| Some(Vec::new())));
    }

    pub(super) fn select_all(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let d = self.diagram();
        let mut items: Vec<Item> = d.nodes.iter().map(|n| Item::Node(n.id.clone())).collect();
        items.extend(d.links.iter().map(|l| Item::Link(l.id.clone())));
        items.extend(d.texts.iter().map(|t| Item::Text(t.id.clone())));
        self.select(items, cx);
    }

    /// Puts the selection on the clipboard; whether there was something to put.
    fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(part) = clip::copy(self.diagram(), self.selection()) else { return false };
        cx.write_to_clipboard(ClipboardItem::new_string(clip::to_text(&part)));
        self.pastes = 0;
        true
    }

    pub(super) fn copy(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() {
            self.copy_selection(cx);
        }
    }

    pub(super) fn cut(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_none() && self.copy_selection(cx) {
            self.delete(window, cx);
        }
    }

    pub(super) fn paste(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let Some(part) = clip::from_text(&text) else { return };
        self.pastes += 1;
        let step = f64::from(GRID) * f64::from(self.pastes);
        self.apply(cx, |d| clip::paste(d, &part, DPoint::new(step, step)).map(Some));
    }

    pub(super) fn duplicate(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        let Some(part) = clip::copy(self.diagram(), self.selection()) else { return };
        let step = f64::from(GRID);
        self.apply(cx, |d| clip::paste(d, &part, DPoint::new(step, step)).map(Some));
    }

    pub(super) fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            if self.preview.take().is_some() {
                self.changed(Redraw::Now);
            }
        } else if self.relating.take().is_some() {
        } else if self.editing.is_some() {
            self.cancel_edit(window, cx);
        } else {
            self.select(Vec::new(), cx);
        }
        self.refused = None;
        cx.notify();
    }

    pub(super) fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        if let [item] = self.selection() {
            let item = item.clone();
            self.start_edit(item, window, cx);
        }
    }

    pub(super) fn fit_to_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let nodes: Vec<Id> = self
            .selection()
            .iter()
            .filter_map(|i| if let Item::Node(id) = i { Some(id.clone()) } else { None })
            .collect();
        if !nodes.is_empty() {
            self.apply(cx, |d| edit::fit_to_text(d, &nodes).map(|()| None));
        }
    }

    pub(super) fn add_attribute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let [Item::Node(owner)] = self.selection() else { return };
        let owner = owner.clone();
        let name = Shape::Attribute.default_name();
        let mut added = None;
        self.apply(cx, |d| {
            let id = edit::add_attribute(d, &owner, &name)?;
            added = Some(id.clone());
            Ok(Some(vec![Item::Node(id)]))
        });
        if let Some(id) = added {
            self.start_edit(Item::Node(id), window, cx);
        }
    }

    pub(super) fn relationship_to(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if let [Item::Node(id)] = self.selection()
            && self.kind_of(id).is_some_and(|k| k.is_entity_like())
        {
            self.relating = Some(id.clone());
            cx.notify();
        }
    }

    /// "Relationship to …" ends with a click: on an entity, a relationship between both.
    fn finish_relationship(&mut self, first: &Id, p: DPoint, cx: &mut Context<Self>) {
        let d = self.diagram();
        let Some(second) = hit::node_at(d, p).filter(|n| n.kind.is_entity_like()) else {
            cx.notify();
            return;
        };
        let (Some(a), b) = (d.node(first).map(|n| n.bounds.center()), second.bounds.center())
        else {
            return;
        };
        let second = second.id.clone();
        let first = first.clone();
        let middle = DPoint::new(snap((a.x + b.x) / 2.0), snap((a.y + b.y) / 2.0));
        let name = Shape::Relationship.default_name();
        self.apply(cx, |d| {
            let r = edit::add_node(d, NodeKind::relationship(), &name, middle);
            edit::connect(d, &r, &first, (None, None))?;
            edit::connect(d, &r, &second, (None, None))?;
            Ok(Some(vec![Item::Node(r)]))
        });
    }

    pub(super) fn release_ends(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let links: Vec<Id> = self
            .selection()
            .iter()
            .filter_map(|i| if let Item::Link(id) = i { Some(id.clone()) } else { None })
            .collect();
        if !links.is_empty() {
            self.apply(cx, |d| edit::release_ends(d, &links).map(|()| None));
        }
    }

    pub(super) fn nudge(&mut self, a: &Nudge, _: &mut Window, cx: &mut Context<Self>) {
        let items = self.movable();
        if self.editing.is_some() || items.is_empty() {
            return;
        }
        let (dx, dy) = (f64::from(a.dx), f64::from(a.dy));
        self.apply(cx, |d| edit::move_items(d, &items, dx, dy).map(|()| None));
    }

    pub(super) fn grow(&mut self, a: &Grow, _: &mut Window, cx: &mut Context<Self>) {
        let [item] = self.selection() else { return };
        let item = item.clone();
        let Some(b) = bounds_of(self.diagram(), &item) else { return };
        if self.editing.is_some() {
            return;
        }
        let r = DRect::new(b.x, b.y, b.width + f64::from(a.dw), b.height + f64::from(a.dh));
        self.apply(cx, |d| edit::set_bounds(d, &item, r).map(|()| None));
    }
}

/// The context menu: what the selection allows.
pub fn context_menu(
    view: &Entity<ErmCanvas>,
    menu: PopupMenu,
    _: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = view.read(cx);
    let selection = this.selection().to_vec();
    let d = this.diagram();
    let focus = this.focus.clone();
    let single_node = match selection.as_slice() {
        [Item::Node(id)] => d.node(id).map(|n| n.kind),
        _ => None,
    };
    let has_nodes = selection.iter().any(|i| matches!(i, Item::Node(_)));
    let has_links = selection.iter().any(|i| matches!(i, Item::Link(_)));
    let copyable = selection.iter().any(|i| matches!(i, Item::Node(_) | Item::Text(_)));
    let owner = single_node.is_some_and(|k| !matches!(k, NodeKind::IsA { .. }));
    let entity = single_node.is_some_and(|k| k.is_entity_like());
    menu.action_context(focus)
        .menu_with_disabled(t!("erm.edit_text"), Box::new(EditText), selection.len() != 1)
        .separator()
        .menu_with_disabled(t!("menu.cut"), Box::new(Cut), !copyable)
        .menu_with_disabled(t!("menu.copy"), Box::new(Copy), !copyable)
        .menu(t!("menu.paste"), Box::new(Paste))
        .menu_with_disabled(t!("erm.duplicate"), Box::new(Duplicate), !copyable)
        .menu_with_disabled(t!("menu.delete"), Box::new(DeleteSelection), selection.is_empty())
        .separator()
        .menu_with_disabled(t!("erm.add_attribute"), Box::new(AddAttribute), !owner)
        .menu_with_disabled(t!("erm.relationship_to"), Box::new(RelationshipTo), !entity)
        .menu_with_disabled(t!("erm.fit_to_text"), Box::new(FitToText), !has_nodes)
        .menu_with_disabled(t!("erm.release_ends"), Box::new(ReleaseEnds), !has_links)
        .separator()
        .menu(t!("menu.select_all"), Box::new(SelectAll))
}

#[cfg(test)]
mod tests {
    use umlcraft_erm::Rect;
    use umlcraft_erm::hit::Handle;

    use super::{snap, snap_move, snap_resized};

    #[test]
    fn moving_puts_the_corner_on_the_grid() {
        assert_eq!(snap(14.0), 10.0);
        assert_eq!(snap(-16.0), -20.0);
        let b = Rect::new(3.0, 7.0, 100.0, 40.0);
        assert_eq!(snap_move(&b, 11.0, 1.0), (7.0, 3.0));
    }

    #[test]
    fn resizing_puts_the_moving_sides_on_the_grid() {
        let r = Rect::new(0.0, 0.0, 103.0, 44.0);
        assert_eq!(snap_resized(Handle::BottomRight, r), Rect::new(0.0, 0.0, 100.0, 40.0));
        assert_eq!(snap_resized(Handle::Right, r), Rect::new(0.0, 0.0, 100.0, 44.0));
        let r = Rect::new(7.0, 0.0, 93.0, 40.0);
        assert_eq!(snap_resized(Handle::Left, r), Rect::new(10.0, 0.0, 90.0, 40.0));
        // Never below the smallest size.
        let r = Rect::new(0.0, 0.0, 21.0, 40.0);
        assert_eq!(snap_resized(Handle::Right, r).width, 20.0);
    }
}
