//! The diagram editor surface: paints a [`Scene`] and handles tools, selection, dragging,
//! waypoints, in-place editing and elements dropped from the model tree.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;
use umlcraft_model::{ElementKind, Id, Point as MPoint, Rect};
use umlcraft_notation::member::member_text;
use umlcraft_notation::palette::{EdgeTool, Tool};
use umlcraft_notation::scene::{
    self, EdgeHandle, EdgeShape, EndMarker, Figure, Handle, LabelAlign, LineStyle, NodeShape,
    Preview, Row, RowRole, Scene, TextMeasure,
};

use crate::workspace::Workspace;
use crate::{Cancel, DeleteSelection, RemoveFromDiagram, ResetZoom, SelectAll, ZoomToFit};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
const GRID: f32 = 20.0;
/// Smallest node edge when resizing.
const MIN_NODE: f32 = 20.0;
/// How close (in screen pixels) the pointer must be to grab a handle.
const HANDLE_GRAB: f32 = 6.0;

/// Snaps a logical coordinate or distance to half the grid.
fn snap(v: f32) -> f32 {
    (v / GRID * 2.0).round() * GRID / 2.0
}

fn snap_point(p: MPoint) -> MPoint {
    MPoint::new(snap(p.x), snap(p.y))
}

/// An element dragged from the model tree.
#[derive(Clone)]
pub struct DraggedElement {
    pub id: Id,
    pub label: SharedString,
}

impl Render for DraggedElement {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .text_sm()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .child(self.label.clone())
    }
}

#[derive(Clone)]
enum Drag {
    /// Moving the selected nodes; `start` is the logical grab point. `reselect` is a node that
    /// was already part of a multi-selection: a click without movement selects just it.
    Move {
        start: MPoint,
        dx: f32,
        dy: f32,
        reselect: Option<Id>,
    },
    Resize {
        presentation: Id,
        handle: Handle,
        original: Rect,
        start: MPoint,
        rect: Rect,
    },
    Waypoint {
        edge: Id,
        handle: EdgeHandle,
        original: Vec<MPoint>,
        current: MPoint,
    },
    RubberBand {
        start: MPoint,
        current: MPoint,
        extend: bool,
    },
    /// Drawing a new connection from `source`. Survives a click on the source itself, so both
    /// drag-and-drop and click-click work.
    Connect {
        source: Id,
        current: MPoint,
    },
    Pan {
        last: Point<Pixels>,
    },
}

enum Editor {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

/// Text being edited in place, over the row it replaces.
struct InlineEdit {
    element: Id,
    /// Logical rectangle of the edited text.
    rect: Rect,
    editor: Editor,
    _subscription: Subscription,
}

pub struct DiagramCanvas {
    workspace: Entity<Workspace>,
    focus_handle: FocusHandle,
    zoom: f32,
    /// Screen offset of the logical origin, relative to the canvas bounds.
    pan: MPoint,
    /// Zoom and pan of diagrams that are not active right now.
    viewports: HashMap<Id, (f32, MPoint)>,
    shown_diagram: Option<Id>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The scene as last painted; mouse handling works on what the user sees.
    scene: Rc<Scene>,
    drag: Option<Drag>,
    editing: Option<InlineEdit>,
}

/// Measures text with the window's font system, at 100 % zoom.
struct FontMeasure<'a> {
    window: &'a Window,
    family: SharedString,
}

impl TextMeasure for FontMeasure<'_> {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let mut f = font(self.family.clone());
        if bold {
            f = f.bold();
        }
        let run = TextRun { len: text.len(), font: f, ..Default::default() };
        let line = self.window.text_system().shape_line(
            text.to_owned().into(),
            px(font_size),
            &[run],
            None,
        );
        f32::from(line.width())
    }
}

impl DiagramCanvas {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace,
            focus_handle: cx.focus_handle(),
            zoom: 1.0,
            pan: MPoint::new(0.0, 0.0),
            viewports: HashMap::new(),
            shown_diagram: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            scene: Rc::new(Scene::empty()),
            drag: None,
            editing: None,
        }
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = MPoint::new(0.0, 0.0);
        cx.notify();
    }

    /// Zooms by `factor`, keeping the canvas center fixed.
    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let b = self.bounds.get();
        let center = point(b.size.width / 2.0, b.size.height / 2.0) + b.origin;
        self.zoom_at(center, factor);
        cx.notify();
    }

    /// Fits the whole diagram into the visible area (at most 150 %).
    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        let b = self.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        let Some(content) = self.scene.bounds().filter(|_| w > 0.0 && h > 0.0) else { return };
        let margin = 40.0;
        let fit_w = (w - 2.0 * margin) / content.width.max(1.0);
        let fit_h = (h - 2.0 * margin) / content.height.max(1.0);
        self.zoom = fit_w.min(fit_h).clamp(MIN_ZOOM, 1.5);
        let c = content.center();
        self.pan = MPoint::new(w / 2.0 - c.x * self.zoom, h / 2.0 - c.y * self.zoom);
        cx.notify();
    }

    /// Where a paste lands: under the pointer if it is over the canvas, else the view center.
    pub fn paste_position(&self, window: &Window) -> MPoint {
        let mouse = window.mouse_position();
        if self.bounds.get().contains(&mouse) {
            return snap_point(self.to_logical(mouse));
        }
        let b = self.bounds.get();
        snap_point(self.to_logical(point(b.size.width / 2.0, b.size.height / 2.0) + b.origin))
    }

    fn zoom_at(&mut self, screen: Point<Pixels>, factor: f32) {
        let anchor = self.to_logical(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let local = self.local(screen);
        self.pan = MPoint::new(local.x - anchor.x * self.zoom, local.y - anchor.y * self.zoom);
    }

    fn local(&self, screen: Point<Pixels>) -> MPoint {
        let origin = self.bounds.get().origin;
        MPoint::new(f32::from(screen.x - origin.x), f32::from(screen.y - origin.y))
    }

    fn to_logical(&self, screen: Point<Pixels>) -> MPoint {
        let local = self.local(screen);
        MPoint::new((local.x - self.pan.x) / self.zoom, (local.y - self.pan.y) / self.zoom)
    }

    /// Keeps a separate zoom and pan per diagram.
    fn switch_viewport(&mut self, diagram: Option<Id>) {
        if diagram == self.shown_diagram {
            return;
        }
        if let Some(old) = self.shown_diagram {
            self.viewports.insert(old, (self.zoom, self.pan));
        }
        (self.zoom, self.pan) = diagram
            .and_then(|d| self.viewports.get(&d).copied())
            .unwrap_or((1.0, MPoint::new(0.0, 0.0)));
        self.shown_diagram = diagram;
        self.drag = None;
        self.editing = None;
    }

    fn preview(&self, ws: &Workspace) -> Preview {
        match &self.drag {
            Some(Drag::Move { dx, dy, .. }) => Preview {
                moved: scene::with_contents(&self.scene, &ws.selection).into_iter().collect(),
                dx: *dx,
                dy: *dy,
                ..Preview::default()
            },
            Some(Drag::Resize { presentation, rect, .. }) => {
                Preview { resized: Some((*presentation, *rect)), ..Preview::default() }
            }
            Some(Drag::Waypoint { edge, handle, original, current }) => Preview {
                waypoints: Some((*edge, scene::drag_waypoint(original, *handle, *current))),
                ..Preview::default()
            },
            _ => Preview::default(),
        }
    }

    fn node_at(&self, p: MPoint) -> Option<Id> {
        self.scene.nodes.iter().rev().find(|n| n.rect.contains(p)).map(|n| n.id)
    }

    fn on_left_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.focus_handle.focus(window, cx);
        let p = self.to_logical(ev.position);
        let (tool, selection) = {
            let ws = self.workspace.read(cx);
            (ws.tool, ws.selection.clone())
        };
        let extend = ev.modifiers.shift || ev.modifiers.control;

        match tool {
            Tool::Node(node_tool) => {
                let origin = snap_point(p);
                self.workspace.update(cx, |ws, cx| {
                    ws.create_node(node_tool, origin);
                    ws.tool = Tool::Select;
                    cx.notify();
                });
            }
            Tool::Edge(edge_tool) => match (self.drag.clone(), self.node_at(p)) {
                // Second click of click-click mode; a second click on the source makes a loop.
                (Some(Drag::Connect { source, .. }), Some(target)) => {
                    self.drag = None;
                    self.finish_connect(edge_tool, source, target, cx);
                }
                (_, Some(source)) => self.drag = Some(Drag::Connect { source, current: p }),
                (_, None) => self.drag = None,
            },
            Tool::Select => {
                if self.grab_handle(&selection, p, ev.click_count, cx) {
                    cx.notify();
                    return;
                }
                if ev.click_count == 2 && self.start_edit_at(p, window, cx) {
                    return;
                }
                match scene::hit_test(&self.scene, p, 4.0 / self.zoom) {
                    Some(id) => {
                        let already = selection.contains(&id);
                        // A click on a member row selects the member for the properties panel.
                        let member = scene::row_at(&self.scene, p)
                            .filter(|(n, r)| n.id == id && r.role == RowRole::Member)
                            .and_then(|(_, r)| r.element);
                        self.workspace.update(cx, |ws, cx| {
                            if extend {
                                ws.toggle(id);
                            } else if !already {
                                ws.select(vec![id]);
                            }
                            if let Some(member) = member.filter(|_| !extend) {
                                ws.selected_element = Some(member);
                            }
                            cx.notify();
                        });
                        let is_node = self.scene.node(id).is_some();
                        if is_node && self.workspace.read(cx).selection.contains(&id) {
                            let reselect =
                                (already && !extend && selection.len() > 1).then_some(id);
                            self.drag = Some(Drag::Move { start: p, dx: 0.0, dy: 0.0, reselect });
                        }
                    }
                    None => {
                        if !extend {
                            self.workspace.update(cx, |ws, cx| {
                                ws.clear_selection();
                                cx.notify();
                            });
                        }
                        self.drag = Some(Drag::RubberBand { start: p, current: p, extend });
                    }
                }
            }
        }
        cx.notify();
    }

    /// Starts dragging a resize handle or an edge grip of the single selected item. A double
    /// click on a waypoint removes it.
    fn grab_handle(
        &mut self,
        selection: &[Id],
        p: MPoint,
        clicks: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let [single] = selection[..] else { return false };
        let tolerance = HANDLE_GRAB / self.zoom;
        if let Some(node) = self.scene.node(single)
            && let Some(handle) = Handle::at(node.rect, p, tolerance)
        {
            self.drag = Some(Drag::Resize {
                presentation: single,
                handle,
                original: node.rect,
                start: p,
                rect: node.rect,
            });
            return true;
        }
        let Some(edge) = self.scene.edge(single) else { return false };
        let Some(handle) = edge.handle_at(p, tolerance) else { return false };
        let original = edge.waypoints().to_vec();
        match (handle, clicks) {
            (EdgeHandle::Waypoint(i), 2) => {
                let mut waypoints = original;
                waypoints.remove(i);
                self.workspace.update(cx, |ws, cx| {
                    ws.set_waypoints(single, waypoints);
                    cx.notify();
                });
            }
            _ => self.drag = Some(Drag::Waypoint { edge: single, handle, original, current: p }),
        }
        true
    }

    fn finish_connect(&mut self, tool: EdgeTool, source: Id, target: Id, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            ws.connect(tool, source, target);
            ws.tool = Tool::Select;
            cx.notify();
        });
    }

    fn on_middle_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        self.drag = Some(Drag::Pan { last: ev.position });
        cx.notify();
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.to_logical(position);
        let Some(drag) = self.drag.as_mut() else { return };
        match drag {
            Drag::Move { start, dx, dy, .. } => {
                *dx = snap(p.x - start.x);
                *dy = snap(p.y - start.y);
            }
            Drag::Resize { handle, original, start, rect, .. } => {
                *rect =
                    handle.resize(*original, snap(p.x - start.x), snap(p.y - start.y), MIN_NODE);
            }
            Drag::Waypoint { current, .. } => *current = snap_point(p),
            Drag::RubberBand { current, .. } | Drag::Connect { current, .. } => *current = p,
            Drag::Pan { last } => {
                let delta = position - *last;
                *last = position;
                self.pan = self.pan.offset(f32::from(delta.x), f32::from(delta.y));
            }
        }
        cx.notify();
    }

    fn end_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.to_logical(position);
        match self.drag.take() {
            Some(Drag::Move { dx: 0.0, dy: 0.0, reselect: Some(id), .. }) => {
                self.update_workspace(cx, |ws| ws.select(vec![id]));
            }
            Some(Drag::Move { dx, dy, .. }) => {
                // Containers (boundaries, packages) take the nodes drawn inside them along.
                let selection = self.workspace.read(cx).selection.clone();
                let moved = scene::with_contents(&self.scene, &selection);
                self.update_workspace(cx, |ws| ws.move_nodes(&moved, dx, dy))
            }
            Some(Drag::Resize { presentation, original, rect, .. }) if rect != original => {
                self.update_workspace(cx, |ws| ws.set_bounds(presentation, rect));
            }
            Some(Drag::Waypoint { edge, handle, original, current }) => {
                let waypoints = scene::drag_waypoint(&original, handle, current);
                if waypoints != original {
                    self.update_workspace(cx, |ws| ws.set_waypoints(edge, waypoints));
                }
            }
            Some(Drag::RubberBand { start, extend, .. }) => {
                let hits = scene::nodes_in(&self.scene, Rect::from_corners(start, p));
                self.update_workspace(cx, |ws| {
                    if extend {
                        let mut selection = ws.selection.clone();
                        selection.extend(hits.into_iter().filter(|h| !ws.selection.contains(h)));
                        ws.select(selection);
                    } else {
                        ws.select(hits);
                    }
                });
            }
            Some(Drag::Connect { source, current }) => {
                let tool = self.workspace.read(cx).tool;
                match (tool, self.node_at(p)) {
                    (Tool::Edge(edge_tool), Some(target)) if target != source => {
                        self.finish_connect(edge_tool, source, target, cx);
                    }
                    // Released on the source: wait for the second click.
                    (Tool::Edge(_), Some(_)) => {
                        self.drag = Some(Drag::Connect { source, current });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(16.0));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if ev.modifiers.control {
            self.zoom_at(ev.position, (1.0 + dy * 0.002).clamp(0.5, 2.0));
        } else if ev.modifiers.shift {
            self.pan = self.pan.offset(dy, dx);
        } else {
            self.pan = self.pan.offset(dx, dy);
        }
        cx.notify();
    }

    fn on_drop(&mut self, dragged: &DraggedElement, window: &mut Window, cx: &mut Context<Self>) {
        let at = snap_point(self.to_logical(window.mouse_position()));
        let id = dragged.id;
        self.update_workspace(cx, |ws| ws.show_element(id, at));
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        self.editing = None;
        self.update_workspace(cx, |ws| ws.tool = Tool::Select);
    }

    fn update_workspace(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Workspace)) {
        self.workspace.update(cx, |ws, cx| {
            f(ws);
            cx.notify();
        });
        cx.notify();
    }

    // ----- In-place editing ------------------------------------------------------------------

    /// Starts editing the name or member row under `p`. Returns whether there was one.
    fn start_edit_at(&mut self, p: MPoint, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((node, row)) = scene::row_at(&self.scene, p) else { return false };
        let Some(element) = row.element else { return false };
        let model = self.workspace.read(cx).doc.model();
        let Some(e) = model.get(element) else { return false };
        let (text, rect, multi_line) = match (e.kind(), row.role) {
            (ElementKind::Comment(c), _) => (c.body.clone(), node.rect.inflate(-4.0), true),
            (_, RowRole::Member) => (member_text(model, element), row.rect, false),
            _ => (e.name().to_owned(), row.rect, false),
        };
        let node_id = node.id;
        self.workspace.update(cx, |ws, cx| {
            ws.select(vec![node_id]);
            ws.selected_element = Some(element);
            cx.notify();
        });
        // Focus once the editor is on screen; the click itself still focuses the canvas.
        let editor = if multi_line {
            let state = cx.new(|cx| TextareaState::new(window, cx).default_value(text));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
            let subscription =
                cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::Blur) {
                        this.commit_edit(cx);
                    }
                });
            (Editor::Area(state), subscription)
        } else {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
            let subscription =
                cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.commit_edit(cx);
                    }
                });
            (Editor::Line(state), subscription)
        };
        self.editing =
            Some(InlineEdit { element, rect, editor: editor.0, _subscription: editor.1 });
        cx.notify();
        true
    }

    /// Writes the in-place edit back, if one is open.
    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        let text = match &edit.editor {
            Editor::Line(s) => s.read(cx).value().to_string(),
            Editor::Area(s) => s.read(cx).value().to_string(),
        };
        let element = edit.element;
        self.update_workspace(cx, |ws| ws.apply_text(element, &text));
    }
}

impl Focusable for DiagramCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Logical → screen mapping for one frame.
#[derive(Clone, Copy)]
struct Transform {
    origin: Point<Pixels>,
    pan: MPoint,
    zoom: f32,
}

impl Transform {
    fn pt(&self, p: MPoint) -> Point<Pixels> {
        self.origin + point(px(self.pan.x + p.x * self.zoom), px(self.pan.y + p.y * self.zoom))
    }

    fn len(&self, v: f32) -> Pixels {
        px(v * self.zoom)
    }

    fn rect(&self, r: Rect) -> Bounds<Pixels> {
        Bounds::new(self.pt(r.origin()), size(self.len(r.width), self.len(r.height)))
    }

    fn to_logical(self, p: Point<Pixels>) -> MPoint {
        MPoint::new(
            (f32::from(p.x - self.origin.x) - self.pan.x) / self.zoom,
            (f32::from(p.y - self.origin.y) - self.pan.y) / self.zoom,
        )
    }
}

#[derive(Clone, Copy)]
struct Colors {
    background: Hsla,
    grid: Hsla,
    fill: Hsla,
    line: Hsla,
    text: Hsla,
    muted: Hsla,
    selection: Hsla,
}

struct Painter<'a> {
    t: Transform,
    c: Colors,
    font_family: SharedString,
    window: &'a mut Window,
    cx: &'a mut App,
}

impl Painter<'_> {
    fn line(&mut self, points: &[MPoint], width: f32, color: Hsla, style: LineStyle) {
        if points.len() < 2 {
            return;
        }
        let mut path = PathBuilder::stroke(px(width));
        if style == LineStyle::Dashed {
            path = path.dash_array(&[px(6.0 * self.t.zoom), px(4.0 * self.t.zoom)]);
        }
        path.move_to(self.t.pt(points[0]));
        for &p in &points[1..] {
            path.line_to(self.t.pt(p));
        }
        if let Ok(path) = path.build() {
            self.window.paint_path(path, color);
        }
    }

    fn polygon(&mut self, points: &[MPoint], fill: Option<Hsla>, stroke: Hsla) {
        let screen: Vec<_> = points.iter().map(|&p| self.t.pt(p)).collect();
        if let Some(fill) = fill {
            let mut path = PathBuilder::fill();
            path.add_polygon(&screen, true);
            if let Ok(path) = path.build() {
                self.window.paint_path(path, fill);
            }
        }
        let mut path = PathBuilder::stroke(px(1.0));
        path.add_polygon(&screen, true);
        if let Ok(path) = path.build() {
            self.window.paint_path(path, stroke);
        }
    }

    fn ellipse(&mut self, center: MPoint, rx: f32, ry: f32, fill: Option<Hsla>, stroke: Hsla) {
        let build = |mut path: PathBuilder, t: &Transform| {
            let radii = point(t.len(rx), t.len(ry));
            path.move_to(t.pt(center.offset(rx, 0.0)));
            path.arc_to(radii, px(0.0), false, true, t.pt(center.offset(-rx, 0.0)));
            path.arc_to(radii, px(0.0), false, true, t.pt(center.offset(rx, 0.0)));
            path.close();
            path.build()
        };
        if let Some(fill) = fill
            && let Ok(path) = build(PathBuilder::fill(), &self.t)
        {
            self.window.paint_path(path, fill);
        }
        if let Ok(path) = build(PathBuilder::stroke(px(1.0)), &self.t) {
            self.window.paint_path(path, stroke);
        }
    }

    fn rect(&mut self, r: Rect, fill: Hsla, border: Hsla, style: BorderStyle) {
        self.window.paint_quad(quad(self.t.rect(r), px(0.0), fill, px(1.0), border, style));
    }

    /// Draws one line of text inside `area` and returns the logical text width.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        area: Rect,
        font_size: f32,
        bold: bool,
        italic: bool,
        color: Hsla,
        align: LabelAlign,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let mut f = font(self.font_family.clone());
        if bold {
            f = f.bold();
        }
        if italic {
            f = f.italic();
        }
        let text: SharedString = text.to_owned().into();
        let run = TextRun { len: text.len(), font: f, color, ..Default::default() };
        let line = self.window.text_system().shape_line(text, self.t.len(font_size), &[run], None);
        let line_height = self.t.len(font_size * 1.3);
        let measured = f32::from(line.width()) / self.t.zoom;
        let (align, width) = match align {
            LabelAlign::Left => (TextAlign::Left, None),
            LabelAlign::Center => (TextAlign::Center, Some(self.t.len(area.width))),
            LabelAlign::Right => (TextAlign::Right, Some(self.t.len(area.width))),
        };
        line.paint(self.t.pt(area.origin()), line_height, align, width, self.window, self.cx).ok();
        measured
    }

    fn row(&mut self, row: &Row) {
        let color = self.c.text;
        let width =
            self.text(&row.text, row.rect, row.size, row.bold, row.italic, color, row.align);
        if row.underline {
            let x = match row.align {
                LabelAlign::Left => row.rect.x,
                LabelAlign::Center => row.rect.x + (row.rect.width - width) / 2.0,
                LabelAlign::Right => row.rect.right() - width,
            };
            let y = row.rect.y + row.size * 1.15;
            self.line(
                &[MPoint::new(x, y), MPoint::new(x + width, y)],
                1.0,
                color,
                LineStyle::Solid,
            );
        }
    }

    fn grid(&mut self, bounds: Bounds<Pixels>) {
        let step = GRID * self.t.zoom;
        if step < 8.0 {
            return;
        }
        let top_left = self.t.to_logical(bounds.origin);
        let bottom_right = self.t.to_logical(bounds.bottom_right());
        let mut x = (top_left.x / GRID).floor() * GRID;
        while x <= bottom_right.x {
            let sx = self.t.pt(MPoint::new(x, 0.0)).x;
            self.window.paint_quad(fill(
                Bounds::new(point(sx, bounds.origin.y), size(px(1.0), bounds.size.height)),
                self.c.grid,
            ));
            x += GRID;
        }
        let mut y = (top_left.y / GRID).floor() * GRID;
        while y <= bottom_right.y {
            let sy = self.t.pt(MPoint::new(0.0, y)).y;
            self.window.paint_quad(fill(
                Bounds::new(point(bounds.origin.x, sy), size(bounds.size.width, px(1.0))),
                self.c.grid,
            ));
            y += GRID;
        }
    }

    /// Paints a node's outline and its rows, except the row being edited in place.
    fn node(&mut self, node: &NodeShape, editing: Option<Id>) {
        let r = node.rect;
        let c = self.c;
        let solid = LineStyle::Solid;
        match &node.figure {
            Figure::Classifier { separators } => {
                self.rect(r, c.fill, c.line, BorderStyle::Solid);
                for &y in separators {
                    self.line(
                        &[MPoint::new(r.x, y), MPoint::new(r.right(), y)],
                        1.0,
                        c.line,
                        solid,
                    );
                }
            }
            Figure::Lollipop => {
                self.ellipse(r.center(), r.width / 2.0, r.height / 2.0, Some(c.fill), c.line);
            }
            Figure::Package { tab } => {
                self.rect(*tab, c.fill, c.line, BorderStyle::Solid);
                let body = Rect::new(r.x, r.y + tab.height, r.width, r.height - tab.height);
                self.rect(body, c.fill, c.line, BorderStyle::Solid);
            }
            Figure::Actor => {
                let cx = r.x + r.width / 2.0;
                let head = (r.width.min(r.height) * 0.18).max(6.0);
                let neck = r.y + head * 2.0;
                let hip = r.y + r.height * 0.62;
                let arms = r.y + r.height * 0.38;
                self.ellipse(MPoint::new(cx, r.y + head), head, head, Some(c.fill), c.line);
                self.line(&[MPoint::new(cx, neck), MPoint::new(cx, hip)], 1.0, c.line, solid);
                self.line(
                    &[MPoint::new(r.x, arms), MPoint::new(r.right(), arms)],
                    1.0,
                    c.line,
                    solid,
                );
                let legs = [
                    MPoint::new(r.x, r.bottom()),
                    MPoint::new(cx, hip),
                    MPoint::new(r.right(), r.bottom()),
                ];
                self.line(&legs, 1.0, c.line, solid);
            }
            Figure::UseCase { separator } => {
                let (rx, ry) = (r.width / 2.0, r.height / 2.0);
                let center = r.center();
                self.ellipse(center, rx, ry, Some(c.fill), c.line);
                // A chord across the ellipse at the separator's height.
                if let Some(y) = *separator {
                    let t = ((y - center.y) / ry).clamp(-1.0, 1.0);
                    let half = rx * (1.0 - t * t).sqrt();
                    let chord = [MPoint::new(center.x - half, y), MPoint::new(center.x + half, y)];
                    self.line(&chord, 1.0, c.line, solid);
                }
            }
            Figure::Boundary => {
                self.rect(r, Hsla { a: 0.35, ..c.fill }, c.line, BorderStyle::Solid)
            }
            Figure::Lifeline { head } => {
                self.rect(*head, c.fill, c.line, BorderStyle::Solid);
                let x = r.x + r.width / 2.0;
                let stem = [MPoint::new(x, head.bottom()), MPoint::new(x, r.bottom())];
                self.line(&stem, 1.0, c.line, LineStyle::Dashed);
            }
            Figure::Note => {
                let fold = 10.0;
                self.polygon(
                    &[
                        r.origin(),
                        MPoint::new(r.right() - fold, r.y),
                        MPoint::new(r.right(), r.y + fold),
                        MPoint::new(r.right(), r.bottom()),
                        MPoint::new(r.x, r.bottom()),
                    ],
                    Some(c.fill),
                    c.line,
                );
            }
            Figure::Other => self.rect(r, c.fill, c.muted, BorderStyle::Dashed),
        }
        for row in &node.rows {
            if editing.is_some() && row.element == editing {
                continue;
            }
            self.row(row);
        }
    }

    fn marker(&mut self, tip: MPoint, from: MPoint, marker: EndMarker) {
        let (dx, dy) = (tip.x - from.x, tip.y - from.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 || marker == EndMarker::None {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let at = |back: f32, side: f32| {
            MPoint::new(tip.x - ux * back - uy * side, tip.y - uy * back + ux * side)
        };
        let c = self.c;
        match marker {
            EndMarker::None => {}
            EndMarker::OpenArrow => {
                self.line(&[at(10.0, 5.0), tip, at(10.0, -5.0)], 1.0, c.line, LineStyle::Solid);
            }
            EndMarker::FilledArrow => {
                self.polygon(&[tip, at(10.0, 5.0), at(10.0, -5.0)], Some(c.line), c.line)
            }
            EndMarker::HollowTriangle => {
                self.polygon(&[tip, at(14.0, 8.0), at(14.0, -8.0)], Some(c.background), c.line)
            }
            EndMarker::HollowDiamond | EndMarker::FilledDiamond => {
                let fill = if marker == EndMarker::FilledDiamond { c.line } else { c.background };
                self.polygon(
                    &[tip, at(8.0, 5.0), at(16.0, 0.0), at(8.0, -5.0)],
                    Some(fill),
                    c.line,
                );
            }
        }
    }

    fn edge(&mut self, edge: &EdgeShape, selected: bool) {
        let pts = &edge.points;
        let color = if selected { self.c.selection } else { self.c.line };
        self.line(pts, if selected { 2.0 } else { 1.0 }, color, edge.line);
        let n = pts.len();
        self.marker(pts[n - 1], pts[n - 2], edge.target_marker);
        self.marker(pts[0], pts[1], edge.source_marker);
        // Wide enough for any role name; alignment does the actual placement.
        const BOX: f32 = 200.0;
        let text = self.c.text;
        for label in &edge.labels {
            let MPoint { x, y } = label.anchor;
            let area = match label.align {
                LabelAlign::Left => Rect::new(x, y, BOX, 14.0),
                LabelAlign::Center => Rect::new(x - BOX / 2.0, y, BOX, 14.0),
                LabelAlign::Right => Rect::new(x - BOX, y, BOX, 14.0),
            };
            self.text(&label.text, area, 11.0, false, false, text, label.align);
        }
    }

    fn edge_handles(&mut self, edge: &EdgeShape) {
        let s = self.c.selection;
        let half = 3.5 / self.t.zoom;
        for (handle, p) in edge.handles() {
            let square = Rect::new(p.x - half, p.y - half, 2.0 * half, 2.0 * half);
            let fill = match handle {
                EdgeHandle::Waypoint(_) => s,
                EdgeHandle::Insert(_) => self.c.background,
            };
            self.window.paint_quad(quad(
                self.t.rect(square),
                px(1.0),
                fill,
                px(1.0),
                s,
                BorderStyle::Solid,
            ));
        }
    }

    fn selection(&mut self, r: Rect, handles: bool) {
        let s = self.c.selection;
        self.window.paint_quad(quad(
            self.t.rect(r.inflate(3.0)),
            px(2.0),
            transparent_black(),
            px(1.5),
            s,
            BorderStyle::Solid,
        ));
        if handles {
            let half = 3.5 / self.t.zoom;
            for handle in Handle::ALL {
                let p = handle.position(r);
                let square = Rect::new(p.x - half, p.y - half, 2.0 * half, 2.0 * half);
                self.window.paint_quad(quad(
                    self.t.rect(square),
                    px(1.0),
                    self.c.background,
                    px(1.5),
                    s,
                    BorderStyle::Solid,
                ));
            }
        }
    }

    fn rubber_band(&mut self, area: Rect) {
        let s = self.c.selection;
        self.window.paint_quad(quad(
            self.t.rect(area),
            px(0.0),
            Hsla { a: 0.08, ..s },
            px(1.0),
            s,
            BorderStyle::Dashed,
        ));
    }
}

impl Render for DiagramCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, tool) = {
            let ws = self.workspace.read(cx);
            (ws.active_diagram, ws.tool)
        };
        self.switch_viewport(active);
        let theme = cx.theme();
        let font_family = theme.font_family.clone();
        let scene = {
            let ws = self.workspace.read(cx);
            let measure = FontMeasure { window, family: font_family.clone() };
            match ws.active_diagram {
                Some(d) => scene::build(ws.doc.model(), d, &self.preview(ws), &measure),
                None => Scene::empty(),
            }
        };
        let scene = Rc::new(scene);
        self.scene = scene.clone();
        let selection = self.workspace.read(cx).selection.clone();
        let theme = cx.theme();
        let colors = Colors {
            background: theme.background,
            grid: Hsla { a: if theme.is_dark() { 0.25 } else { 0.45 }, ..theme.border },
            fill: if theme.is_dark() {
                theme.secondary
            } else {
                hsla(48.0 / 360.0, 1.0, 0.95, 1.0)
            },
            line: theme.foreground,
            text: theme.foreground,
            muted: theme.muted_foreground,
            selection: theme.blue,
        };
        let (pan, zoom, drag) = (self.pan, self.zoom, self.drag.clone());
        let editing_element = self.editing.as_ref().map(|e| e.element);
        let bounds_cell = self.bounds.clone();
        let this = cx.entity();
        let cursor = if tool == Tool::Select { CursorStyle::Arrow } else { CursorStyle::Crosshair };

        // The in-place editor sits over the edited text, in canvas-local pixels.
        let editor = self.editing.as_ref().map(|edit| {
            let r = edit.rect;
            let (x, y) = (pan.x + r.x * zoom, pan.y + r.y * zoom);
            let width = (r.width * zoom).max(120.0);
            let field = match &edit.editor {
                Editor::Line(state) => Input::new(state).xsmall().into_any_element(),
                Editor::Area(state) => Textarea::new(state).xsmall().into_any_element(),
            };
            div().absolute().left(px(x - 4.0)).top(px(y - 4.0)).w(px(width + 8.0)).child(field)
        });

        div()
            .id("diagram-canvas")
            .key_context("Canvas")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(colors.background)
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            // Window-wide listeners (see the paint closure) follow drags outside the canvas,
            // but only exist from the next frame on; these catch very quick drags.
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    this.drag_to(ev.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| this.end_drag(ev.position, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| this.end_drag(ev.position, cx)),
            )
            .on_scroll_wheel(cx.listener(Self::on_scroll))
            .on_drop(cx.listener(Self::on_drop))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(|this, _: &DeleteSelection, _, cx| {
                this.update_workspace(cx, Workspace::delete_selection);
            }))
            .on_action(cx.listener(|this, _: &RemoveFromDiagram, _, cx| {
                this.update_workspace(cx, Workspace::remove_selection_from_diagram);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.update_workspace(cx, Workspace::select_all);
            }))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.reset_zoom(cx)))
            .on_action(cx.listener(|this, _: &ZoomToFit, _, cx| this.zoom_to_fit(cx)))
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(bounds),
                    move |bounds, (), window, cx| {
                        if drag.is_some() {
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    target.update(cx, |c, cx| c.drag_to(ev.position, cx));
                                }
                            });
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble
                                    && matches!(ev.button, MouseButton::Left | MouseButton::Middle)
                                {
                                    target.update(cx, |c, cx| c.end_drag(ev.position, cx));
                                }
                            });
                        }
                        let t = Transform { origin: bounds.origin, pan, zoom };
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            let mut p = Painter { t, c: colors, font_family, window, cx };
                            p.grid(bounds);
                            for node in &scene.nodes {
                                p.node(node, editing_element);
                            }
                            for edge in &scene.edges {
                                p.edge(edge, selection.contains(&edge.id));
                            }
                            let selected_nodes: Vec<&NodeShape> =
                                scene.nodes.iter().filter(|n| selection.contains(&n.id)).collect();
                            let single = selection.len() == 1;
                            for node in &selected_nodes {
                                p.selection(node.rect, single);
                            }
                            if single && let Some(edge) = scene.edge(selection[0]) {
                                p.edge_handles(edge);
                            }
                            match &drag {
                                Some(Drag::RubberBand { start, current, .. }) => {
                                    p.rubber_band(Rect::from_corners(*start, *current));
                                }
                                Some(Drag::Connect { source, current }) => {
                                    if let Some(node) = scene.node(*source) {
                                        let from = scene::border_point(node.rect, *current);
                                        let s = p.c.selection;
                                        p.line(&[from, *current], 1.5, s, LineStyle::Dashed);
                                    }
                                }
                                _ => {}
                            }
                        });
                    },
                )
                .size_full(),
            )
            .children(editor)
    }
}
