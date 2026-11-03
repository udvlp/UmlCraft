//! The canvas of an ER diagram: the SVG image of `umlcraft_erm::svg` rasterized with resvg, so
//! that it shows exactly what an export holds, with zoom, panning and the editor's marks over
//! it (selection, grips, connection points, rubber band, outlines while dragging), which never
//! reach an export. The image is made again after each change – while dragging in the
//! background, one at a time – and at most 8192 px on its longer side. The paper is white in
//! the dark theme too, with a 10 px grid; selection and grips are in the theme's blue, as on
//! the main window's canvas.
//!
//! Mouse and keys are in `input.rs`, in-place editing in `text.rs`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use resvg::usvg;
use rust_i18n::t;
use umlcraft_erm::edit::Item;
use umlcraft_erm::history::Editor;
use umlcraft_erm::hit::{self, Handle};
use umlcraft_erm::{Diagram, Issue, Rect as DRect, check};

use super::input::Drag;
use super::text::TextEdit;
use super::{
    AddAttribute, Duplicate, EditText, FitToText, Grow, Nudge, RelationshipTo, ReleaseEnds,
    ToggleGrid,
};
use crate::nsd::{OPTIONS, rasterize};
use crate::{
    Cancel, Copy, Cut, DeleteSelection, Paste, Redo, ResetZoom, SelectAll, Undo, ZoomIn, ZoomOut,
    ZoomToFit,
};

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 4.0;
/// One step of the zoom buttons and keys.
const ZOOM_STEP: f32 = 1.25;
/// The longest side of the image, within GPU texture limits.
const MAX_RASTER: f32 = 8192.0;
/// Space around the drawing at 100 % and when fitted.
const MARGIN: f32 = 20.0;
/// The grid the editor snaps to; its lines are hidden when they come closer than
/// `GRID_MIN_STEP` on screen.
pub const GRID: f32 = 10.0;
const GRID_MIN_STEP: f32 = 5.0;
/// Size of a grip on screen, and how near the mouse has to be to take it.
pub const HANDLE_SIZE: f32 = 7.0;
pub const HANDLE_GRAB: f32 = 6.0;

/// The diagram rasterized for one scale.
struct Raster {
    scale: f32,
    image: Arc<RenderImage>,
    /// Its size in pixels of the image, and the part of the diagram it shows.
    size: (u32, u32),
    view: DRect,
}

/// The image to paint: its pixels, size, scale and the part of the diagram it shows.
struct Shown {
    image: Arc<RenderImage>,
    size: (u32, u32),
    scale: f32,
    view: DRect,
}

/// What a change does to the image.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Redraw {
    /// Right away (an edit: the image matches before the next paint).
    Now,
    /// In the background (dragging); the old image stands in.
    Later,
}

pub struct ErmCanvas {
    pub(super) editor: Editor,
    /// While dragging: the diagram as it would be after letting go.
    pub(super) preview: Option<Diagram>,
    /// Counts the changes of the diagram and the selection (not of a drag's preview), for the
    /// properties panel.
    pub(super) revision: u64,
    /// The warnings of the diagram as it is.
    pub(super) issues: Vec<Issue>,
    pub(super) grid: bool,
    /// What is shown, parsed, and the part of the diagram it covers.
    tree: Option<(Arc<usvg::Tree>, DRect)>,
    /// Why there is no tree: the SVG did not parse. Shown instead of the sheet.
    failure: Option<String>,
    generation: u64,
    raster: Option<Raster>,
    /// The raster shows an older state; it goes on the next paint.
    outdated: bool,
    /// How the next image is to be made.
    redraw: Redraw,
    rasterizing: bool,
    pub(super) zoom: f32,
    /// Where the diagram's origin is in the drawing area.
    pub(super) pan: Point<Pixels>,
    /// Until the user zooms or pans, the drawing is fitted into the area on each paint (at
    /// most at 100 %), so that it stays fitted while a new window gets its size.
    pub(super) auto_fit: bool,
    /// The drawing area, as painted last.
    pub(super) area: Rc<Cell<Bounds<Pixels>>>,
    pub(super) drag: Option<Drag>,
    /// The node under the mouse (its connection points are shown).
    pub(super) hover: Option<umlcraft_erm::Id>,
    /// Space is held: the left button pans.
    pub(super) space: bool,
    pub(super) editing: Option<TextEdit>,
    /// "Relationship to …": the entity waiting for its partner.
    pub(super) relating: Option<umlcraft_erm::Id>,
    /// How often the clipboard was pasted since the last copy (each paste goes 10 px further).
    pub(super) pastes: u32,
    /// Why the last edit was refused, shown until the next one.
    pub(super) refused: Option<String>,
    pub(super) focus: FocusHandle,
}

impl ErmCanvas {
    /// `saved`: the diagram is what a file holds; else it is new.
    pub fn new(diagram: Diagram, saved: bool, cx: &mut Context<Self>) -> Self {
        let mut canvas = Self {
            editor: Editor::new(diagram, saved),
            preview: None,
            revision: 0,
            issues: Vec::new(),
            grid: true,
            tree: None,
            generation: 0,
            raster: None,
            outdated: false,
            redraw: Redraw::Now,
            rasterizing: false,
            zoom: 1.0,
            pan: point(px(MARGIN), px(MARGIN)),
            auto_fit: true,
            area: Rc::default(),
            drag: None,
            hover: None,
            space: false,
            editing: None,
            relating: None,
            pastes: 0,
            refused: None,
            failure: None,
            focus: cx.focus_handle(),
        };
        canvas.changed(Redraw::Now);
        canvas
    }

    /// The diagram as it is (not the preview of a drag).
    pub fn diagram(&self) -> &Diagram {
        self.editor.diagram()
    }

    /// What is drawn: the preview while dragging, else the diagram.
    pub(super) fn shown(&self) -> &Diagram {
        self.preview.as_ref().unwrap_or(self.editor.diagram())
    }

    pub fn selection(&self) -> &[Item] {
        self.editor.selection()
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn grid(&self) -> bool {
        self.grid
    }

    pub fn can_undo(&self) -> bool {
        self.editor.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.editor.can_redo()
    }

    /// Whether closing would lose something: a changed file, or a new diagram with content.
    pub fn unsaved(&self) -> bool {
        if self.editor.is_new() {
            self.editor.can_undo() || !self.diagram().nodes.is_empty()
        } else {
            self.editor.is_dirty()
        }
    }

    /// What is shown changed: a new image (now or in the background), new warnings.
    pub(super) fn changed(&mut self, redraw: Redraw) {
        let options = umlcraft_erm::svg::Options { background: None, ..Default::default() };
        let svg = umlcraft_erm::svg::render_with(self.shown(), &options);
        let view = view_box(&svg).unwrap_or_default();
        match OPTIONS.with(|options| usvg::Tree::from_str(&svg, options)) {
            Ok(t) => (self.tree, self.failure) = (Some((Arc::new(t), view)), None),
            Err(err) => (self.tree, self.failure) = (None, Some(err.to_string())),
        }
        self.generation += 1;
        self.outdated = self.raster.is_some();
        self.redraw = redraw;
        if self.preview.is_none() {
            self.revision += 1;
            self.issues = check(self.diagram());
        }
    }

    /// Makes one undoable change; a refused one is reported in the window. Whether it was
    /// made.
    pub(super) fn apply(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Diagram) -> Result<Option<Vec<Item>>, umlcraft_erm::EditError>,
    ) -> bool {
        self.preview = None;
        let done = match self.editor.apply(change) {
            Ok(()) => {
                self.refused = None;
                true
            }
            Err(e) => {
                self.refused = Some(super::edit_error(&e));
                false
            }
        };
        self.changed(Redraw::Now);
        cx.notify();
        done
    }

    pub(super) fn select(&mut self, items: Vec<Item>, cx: &mut Context<Self>) {
        self.editor.select(items);
        self.revision += 1;
        cx.notify();
    }

    /// Takes the diagram as written to its file (stamped with the date).
    pub fn saved(&mut self, written: Diagram, cx: &mut Context<Self>) {
        self.editor.saved(written);
        self.revision += 1;
        cx.notify();
    }

    /// The diagram to write (see `Editor::to_save`).
    pub fn to_save(&self, user: &str, now: &str) -> Diagram {
        self.editor.to_save(user, now)
    }

    pub(super) fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_edit(window, cx);
        if self.editor.undo() {
            self.changed(Redraw::Now);
            cx.notify();
        }
    }

    pub(super) fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_edit(window, cx);
        if self.editor.redo() {
            self.changed(Redraw::Now);
            cx.notify();
        }
    }

    pub(super) fn toggle_grid(&mut self, _: &ToggleGrid, _: &mut Window, cx: &mut Context<Self>) {
        self.grid = !self.grid;
        cx.notify();
    }

    // ----- Raster ----------------------------------------------------------------------------

    /// The image to paint at `scale` (zoom × screen scale): image, size, scale and view. The
    /// first image and those after an edit are made right away; others in the background,
    /// while the one there is stands in.
    fn raster(&mut self, scale: f32, window: &mut Window, cx: &mut Context<Self>) -> Option<Shown> {
        let (w, h) = self.tree.as_ref().map(|(t, _)| (t.size().width(), t.size().height()))?;
        let scale = scale.min(MAX_RASTER / w.max(h).max(1.0));
        let fits = self.raster.as_ref().map(|r| !self.outdated && (r.scale - scale).abs() <= 1e-4);
        match fits {
            Some(true) => {}
            Some(false) if !self.outdated || self.redraw == Redraw::Later => {
                self.rasterize_later(scale, window, cx)
            }
            _ => {
                let image = self.tree.as_ref().and_then(|(t, view)| {
                    rasterize(t, scale).map(|(image, size)| (image, size, *view))
                });
                self.replace_raster(image, scale, window);
                self.outdated = false;
            }
        }
        self.raster.as_ref().map(|r| Shown {
            image: r.image.clone(),
            size: r.size,
            scale: r.scale,
            view: r.view,
        })
    }

    fn rasterize_later(&mut self, scale: f32, window: &mut Window, cx: &mut Context<Self>) {
        if self.rasterizing {
            return;
        }
        let Some((tree, view)) = self.tree.clone() else { return };
        self.rasterizing = true;
        let generation = self.generation;
        cx.spawn_in(window, async move |this, cx| {
            let image = cx
                .background_executor()
                .spawn(async move { rasterize(&tree, scale).map(|(i, s)| (i, s, view)) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.rasterizing = false;
                if this.generation == generation {
                    this.replace_raster(image, scale, window);
                    this.outdated = false;
                } else if image.is_some() && this.redraw == Redraw::Later {
                    // Dragging: an image of a moment ago is better than an older one.
                    this.replace_raster(image, scale, window);
                }
                // Paints again, which asks for the state of now if it has moved on.
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn replace_raster(
        &mut self,
        image: Option<(Arc<RenderImage>, (u32, u32), DRect)>,
        scale: f32,
        window: &mut Window,
    ) {
        // GPUI keeps every image it painted in the window's atlas until it is dropped there.
        if let Some(old) = self.raster.take() {
            window.drop_image(old.image).ok();
        }
        self.raster = image.map(|(image, size, view)| Raster { scale, image, size, view });
    }

    // ----- Zoom and panning ------------------------------------------------------------------

    pub(super) fn set_zoom(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        let (zoom, pan) = zoom_about(self.zoom, self.pan, anchor, zoom);
        self.zoom = zoom;
        self.pan = pan;
        self.auto_fit = false;
        cx.notify();
    }

    fn centre(&self) -> Point<Pixels> {
        let area = self.area.get();
        point(area.size.width / 2.0, area.size.height / 2.0)
    }

    pub(super) fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * ZOOM_STEP, self.centre(), cx);
    }

    pub(super) fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom / ZOOM_STEP, self.centre(), cx);
    }

    pub(super) fn reset_zoom(&mut self, _: &ResetZoom, _: &mut Window, cx: &mut Context<Self>) {
        let view = self.tree.as_ref().map(|(_, v)| *v).unwrap_or_default();
        self.zoom = 1.0;
        self.pan = point(px(MARGIN - view.x as f32), px(MARGIN - view.y as f32));
        self.auto_fit = false;
        cx.notify();
    }

    pub(super) fn zoom_to_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.fit(MAX_ZOOM);
        self.auto_fit = false;
        cx.notify();
    }

    /// Fits the drawing into the area, at most at `max`.
    fn fit(&mut self, max: f32) {
        let Some((_, view)) = &self.tree else { return };
        let area = self.area.get().size;
        let sheet = (view.width as f32, view.height as f32);
        let (zoom, pan) = fit((f32::from(area.width), f32::from(area.height)), sheet, max);
        self.zoom = zoom;
        self.pan = point(px(pan.0 - view.x as f32 * zoom), px(pan.1 - view.y as f32 * zoom));
    }

    /// A position in the window as a point of the diagram.
    pub(super) fn to_diagram(&self, position: Point<Pixels>) -> umlcraft_erm::Point {
        let origin = self.area.get().origin;
        umlcraft_erm::Point::new(
            f64::from(f32::from(position.x - origin.x - self.pan.x) / self.zoom),
            f64::from(f32::from(position.y - origin.y - self.pan.y) / self.zoom),
        )
    }

    /// A point of the diagram on screen, relative to the drawing area.
    pub(super) fn to_area(&self, p: umlcraft_erm::Point) -> Point<Pixels> {
        point(self.pan.x + px(p.x as f32 * self.zoom), self.pan.y + px(p.y as f32 * self.zoom))
    }

    /// The middle of what is visible, in the diagram.
    pub(super) fn visible_centre(&self) -> umlcraft_erm::Point {
        let c = self.centre();
        umlcraft_erm::Point::new(
            f64::from(f32::from(c.x - self.pan.x) / self.zoom),
            f64::from(f32::from(c.y - self.pan.y) / self.zoom),
        )
    }

    /// The marks over the drawing, for one paint.
    fn marks(&self) -> Marks {
        let d = self.shown();
        let mut marks = Marks::default();
        let single = self.editor.selection().len() == 1;
        for item in self.editor.selection() {
            match item {
                Item::Node(id) => {
                    if let Some(n) = d.node(id) {
                        marks.boxes.push(n.bounds);
                        if single {
                            marks.handles.extend(Handle::ALL.map(|h| h.point(&n.bounds)));
                        }
                    }
                }
                Item::Text(id) => {
                    if let Some(t) = d.texts.iter().find(|t| t.id == *id) {
                        marks.boxes.push(t.bounds);
                        if single {
                            marks.handles.extend(Handle::ALL.map(|h| h.point(&t.bounds)));
                        }
                    }
                }
                Item::Link(id) => {
                    if let Some(l) = d.link(id)
                        && let Some(points) = umlcraft_erm::geometry::points_of(d, l)
                    {
                        if let (Some(a), Some(b)) = (points.first(), points.last()) {
                            marks.handles.extend([*a, *b]);
                        }
                        marks.lines.push(points);
                    }
                }
                Item::Label(id) => {
                    if let Some(r) = d.link(id).and_then(|l| hit::label_boxes(d, l).0) {
                        marks.boxes.push(r);
                        marks.label_handles.push(r.center());
                    }
                }
            }
        }
        // Connection points of the node under the mouse, or of a connection's target.
        let target = match &self.drag {
            Some(Drag::Connect { target, .. } | Drag::End { target, .. }) => {
                target.as_ref().map(|t| (t.node.clone(), t.allowed))
            }
            _ => None,
        };
        let idle = matches!(self.drag, None | Some(Drag::Pan { .. }));
        let shown = target
            .clone()
            .map(|(id, _)| id)
            .or_else(|| if idle { self.hover.clone() } else { None });
        if let Some(n) = shown.and_then(|id| d.node(&id).cloned()) {
            marks.points = hit::connection_ports(&n.kind)
                .into_iter()
                .map(|port| umlcraft_erm::geometry::port_point(&n, port))
                .collect();
        }
        if let Some((id, allowed)) = target
            && let Some(n) = d.node(&id)
        {
            marks.target = Some((n.bounds, allowed));
        }
        match &self.drag {
            Some(Drag::Band { start, current, .. }) => marks.band = Some(band(*start, *current)),
            Some(Drag::Connect { from_point, current, target, .. }) => {
                let to = target.as_ref().map_or(*current, |t| t.point);
                marks.rubber = Some((*from_point, to));
            }
            Some(Drag::End { fixed, current, target, .. }) => {
                let to = target.as_ref().map_or(*current, |t| t.point);
                marks.rubber = Some((*fixed, to));
            }
            Some(Drag::Move { .. } | Drag::Resize { .. }) => {
                // Outlines where the shapes go: at once, while the image follows.
                if let Some(p) = &self.preview {
                    for item in self.editor.selection() {
                        let b = match item {
                            Item::Node(id) => p.node(id).map(|n| n.bounds),
                            Item::Text(id) => {
                                p.texts.iter().find(|t| t.id == *id).map(|t| t.bounds)
                            }
                            _ => None,
                        };
                        marks.outlines.extend(b);
                    }
                }
            }
            _ => {}
        }
        marks
    }
}

/// The marks over the drawing, in diagram coordinates.
#[derive(Default)]
struct Marks {
    /// Dashed frames: selected nodes, texts and labels.
    boxes: Vec<DRect>,
    handles: Vec<umlcraft_erm::Point>,
    label_handles: Vec<umlcraft_erm::Point>,
    /// Selected lines.
    lines: Vec<Vec<umlcraft_erm::Point>>,
    /// Connection points shown.
    points: Vec<umlcraft_erm::Point>,
    /// The node a connection would go to, and whether it may.
    target: Option<(DRect, bool)>,
    band: Option<DRect>,
    /// The line being drawn.
    rubber: Option<(umlcraft_erm::Point, umlcraft_erm::Point)>,
    /// Where dragged shapes go.
    outlines: Vec<DRect>,
}

/// The rectangle between two corners.
pub(super) fn band(a: umlcraft_erm::Point, b: umlcraft_erm::Point) -> DRect {
    DRect::new(a.x.min(b.x), a.y.min(b.y), (a.x - b.x).abs(), (a.y - b.y).abs())
}

/// The view box of an SVG image.
fn view_box(svg: &str) -> Option<DRect> {
    let v = svg.split("viewBox=\"").nth(1)?.split('"').next()?;
    let n: Vec<f64> = v.split(' ').filter_map(|x| x.parse().ok()).collect();
    (n.len() == 4).then(|| DRect::new(n[0], n[1], n[2], n[3]))
}

impl Render for ErmCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let menu_owner = cx.entity();
        let area = self.area.clone();
        let cursor = match (&self.drag, self.space) {
            (Some(Drag::Pan { .. }), _) => CursorStyle::ClosedHand,
            (None, true) => CursorStyle::OpenHand,
            (Some(Drag::Connect { .. } | Drag::End { .. }), _) => CursorStyle::Crosshair,
            _ if self.relating.is_some() => CursorStyle::Crosshair,
            _ => CursorStyle::Arrow,
        };
        let editor = self.text_field(window, cx);
        div()
            .id("erm-canvas")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(0xffffff))
            .track_focus(&self.focus)
            .key_context("ErmCanvas")
            .cursor(cursor)
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::reset_zoom))
            .on_action(cx.listener(Self::zoom_to_fit))
            .on_action(cx.listener(Self::toggle_grid))
            .on_action(cx.listener(Self::undo))
            .on_action(cx.listener(Self::redo))
            .on_action(cx.listener(|this, _: &DeleteSelection, w, cx| this.delete(w, cx)))
            .on_action(cx.listener(|this, _: &SelectAll, w, cx| this.select_all(w, cx)))
            .on_action(cx.listener(|this, _: &Copy, w, cx| this.copy(w, cx)))
            .on_action(cx.listener(|this, _: &Cut, w, cx| this.cut(w, cx)))
            .on_action(cx.listener(|this, _: &Paste, w, cx| this.paste(w, cx)))
            .on_action(cx.listener(|this, _: &Duplicate, w, cx| this.duplicate(w, cx)))
            .on_action(cx.listener(|this, _: &Cancel, w, cx| this.cancel(w, cx)))
            .on_action(cx.listener(|this, _: &EditText, w, cx| this.edit_selected(w, cx)))
            .on_action(cx.listener(|this, _: &FitToText, w, cx| this.fit_to_text(w, cx)))
            .on_action(cx.listener(|this, _: &AddAttribute, w, cx| this.add_attribute(w, cx)))
            .on_action(cx.listener(|this, _: &RelationshipTo, w, cx| this.relationship_to(w, cx)))
            .on_action(cx.listener(|this, _: &ReleaseEnds, w, cx| this.release_ends(w, cx)))
            .on_action(cx.listener(|this, a: &Nudge, w, cx| this.nudge(a, w, cx)))
            .on_action(cx.listener(|this, a: &Grow, w, cx| this.grow(a, w, cx)))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::on_mouse_up))
            .on_drop(cx.listener(Self::on_drop))
            .child(
                canvas(
                    move |bounds, _, _| area.set(bounds),
                    move |bounds, _, window, cx| {
                        let colors = MarkColors::of(cx);
                        let scale = window.scale_factor();
                        if this.read(cx).drag.is_some() {
                            // Follows drags outside the canvas too.
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, w, cx| {
                                if phase == DispatchPhase::Bubble {
                                    target.update(cx, |c, cx| c.drag_to(ev, w, cx));
                                }
                            });
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, w, cx| {
                                if phase == DispatchPhase::Bubble {
                                    target.update(cx, |c, cx| c.end_drag(ev, w, cx));
                                }
                            });
                        }
                        let (pan, zoom, raster, marks, grid) = this.update(cx, |this, cx| {
                            // Opened: the whole diagram, but not larger than it is.
                            if this.auto_fit && bounds.size.width > px(0.0) {
                                let before = (this.zoom, this.pan);
                                this.fit(1.0);
                                if before != (this.zoom, this.pan) {
                                    // The toolbar shows the zoom.
                                    cx.notify();
                                }
                            }
                            let raster = this.raster(this.zoom * scale, window, cx);
                            (this.pan, this.zoom, raster, this.marks(), this.grid)
                        });
                        let origin = point(bounds.origin.x + pan.x, bounds.origin.y + pan.y);
                        let at = |p: umlcraft_erm::Point| {
                            point(
                                origin.x + px(p.x as f32 * zoom),
                                origin.y + px(p.y as f32 * zoom),
                            )
                        };
                        let rect = |r: &DRect| {
                            Bounds::new(
                                at(umlcraft_erm::Point::new(r.x, r.y)),
                                size(px(r.width as f32 * zoom), px(r.height as f32 * zoom)),
                            )
                        };
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            if grid {
                                paint_grid(window, bounds, origin, zoom, colors.grid);
                            }
                            if let Some(Shown { image, size: (w, h), scale: raster_scale, view }) =
                                raster
                            {
                                // The image covers its view at the zoom, whatever its scale.
                                let shown = zoom / raster_scale;
                                let corner = at(umlcraft_erm::Point::new(view.x, view.y));
                                let b = Bounds::new(
                                    corner,
                                    size(px(w as f32 * shown), px(h as f32 * shown)),
                                );
                                window.paint_image(b, b, Corners::default(), image, 0, false).ok();
                            }
                            paint_marks(window, &marks, &rect, &at, &colors);
                        });
                    },
                )
                .size_full(),
            )
            .children(editor)
            .when_some(self.failure.clone(), |this, error| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .p_8()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(cx.theme().danger)
                        .child(t!("erm.draw_failed", error = error).to_string()),
                )
            })
            .context_menu(move |menu, window, cx| {
                super::input::context_menu(&menu_owner, menu, window, cx)
            })
    }
}

/// The colours of the grid and of the editor's marks, from the theme (as on the main window's
/// canvas: selection and grips in the theme's blue).
struct MarkColors {
    grid: Hsla,
    selection: Hsla,
    /// Inside grips that are not filled (labels).
    paper: Hsla,
    /// A target a line may not join.
    refused: Hsla,
}

impl MarkColors {
    fn of(cx: &App) -> Self {
        let theme = cx.theme();
        // On the white paper, in either theme.
        let a = if theme.is_dark() { 0.2 } else { 0.45 };
        Self {
            grid: Hsla { a, ..theme.border },
            selection: theme.blue,
            paper: white(),
            refused: theme.danger,
        }
    }
}

/// The grid over the visible area, aligned to the diagram's origin; none when its lines would
/// come too close to tell apart.
fn paint_grid(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    origin: Point<Pixels>,
    zoom: f32,
    color: Hsla,
) {
    let step = GRID * zoom;
    if step < GRID_MIN_STEP {
        return;
    }
    let first = |o: Pixels, start: Pixels| (f32::from(start - o) / step).floor() as i32;
    let (x0, y0) = (first(origin.x, bounds.origin.x), first(origin.y, bounds.origin.y));
    let columns = (f32::from(bounds.size.width) / step).ceil() as i32 + 1;
    let rows = (f32::from(bounds.size.height) / step).ceil() as i32 + 1;
    for i in x0..=x0 + columns {
        let x = (origin.x + px(i as f32 * step)).floor();
        let line = Bounds::new(point(x, bounds.origin.y), size(px(1.0), bounds.size.height));
        window.paint_quad(fill(line, color));
    }
    for j in y0..=y0 + rows {
        let y = (origin.y + px(j as f32 * step)).floor();
        let line = Bounds::new(point(bounds.origin.x, y), size(bounds.size.width, px(1.0)));
        window.paint_quad(fill(line, color));
    }
}

fn paint_marks(
    window: &mut Window,
    marks: &Marks,
    rect: &dyn Fn(&DRect) -> Bounds<Pixels>,
    at: &dyn Fn(umlcraft_erm::Point) -> Point<Pixels>,
    colors: &MarkColors,
) {
    let selection = colors.selection;
    let grip = |p: Point<Pixels>, fill_color: Hsla| {
        let half = px(HANDLE_SIZE / 2.0);
        let b = Bounds::new(point(p.x - half, p.y - half), size(half * 2.0, half * 2.0));
        quad(b, px(1.0), fill_color, px(1.0), selection, BorderStyle::Solid)
    };
    for points in &marks.lines {
        let mut path = PathBuilder::stroke(px(3.0));
        for (i, p) in points.iter().enumerate() {
            if i == 0 {
                path.move_to(at(*p));
            } else {
                path.line_to(at(*p));
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, selection.opacity(0.5));
        }
    }
    for b in &marks.outlines {
        let color = rgb(0x000000).opacity(0.6);
        window.paint_quad(quad(
            rect(b),
            px(0.0),
            transparent_black(),
            px(1.0),
            color,
            BorderStyle::Dashed,
        ));
    }
    for b in &marks.boxes {
        let b = rect(b).dilate(px(1.0));
        window.paint_quad(quad(
            b,
            px(0.0),
            transparent_black(),
            px(1.0),
            selection,
            BorderStyle::Dashed,
        ));
    }
    if let Some((b, allowed)) = marks.target {
        let color = if allowed { selection } else { colors.refused };
        let b = rect(&b).dilate(px(2.0));
        window.paint_quad(quad(
            b,
            px(0.0),
            color.opacity(0.08),
            px(2.0),
            color,
            BorderStyle::Solid,
        ));
    }
    for p in &marks.points {
        // Connection points: small crosses.
        let c = at(*p);
        let s = px(3.0);
        let mut path = PathBuilder::stroke(px(1.5));
        path.move_to(point(c.x - s, c.y - s));
        path.line_to(point(c.x + s, c.y + s));
        path.move_to(point(c.x + s, c.y - s));
        path.line_to(point(c.x - s, c.y + s));
        if let Ok(path) = path.build() {
            window.paint_path(path, selection);
        }
    }
    if let Some((a, b)) = marks.rubber {
        let mut path = PathBuilder::stroke(px(1.5)).dash_array(&[px(4.0), px(3.0)]);
        path.move_to(at(a));
        path.line_to(at(b));
        if let Ok(path) = path.build() {
            window.paint_path(path, selection);
        }
    }
    if let Some(b) = &marks.band {
        let b = rect(b);
        window.paint_quad(quad(
            b,
            px(0.0),
            selection.opacity(0.1),
            px(1.0),
            selection,
            BorderStyle::Solid,
        ));
    }
    for p in &marks.handles {
        window.paint_quad(grip(at(*p), selection));
    }
    for p in &marks.label_handles {
        window.paint_quad(grip(at(*p), colors.paper));
    }
}

impl Focusable for ErmCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The zoom clamped to its range, and the pan that keeps the point under `anchor` (relative to
/// the area) where it is.
fn zoom_about(
    zoom: f32,
    pan: Point<Pixels>,
    anchor: Point<Pixels>,
    to: f32,
) -> (f32, Point<Pixels>) {
    let to = to.clamp(MIN_ZOOM, MAX_ZOOM);
    let factor = to / zoom;
    let pan = point(anchor.x - (anchor.x - pan.x) * factor, anchor.y - (anchor.y - pan.y) * factor);
    (to, pan)
}

/// The zoom that fits `sheet` with its margin into `area`, at most `max`, and where its top
/// left corner goes to be centred.
fn fit(area: (f32, f32), sheet: (f32, f32), max: f32) -> (f32, (f32, f32)) {
    let fit_w = (area.0 - 2.0 * MARGIN) / sheet.0.max(1.0);
    let fit_h = (area.1 - 2.0 * MARGIN) / sheet.1.max(1.0);
    let zoom = fit_w.min(fit_h).min(max).clamp(MIN_ZOOM, MAX_ZOOM);
    (zoom, ((area.0 - sheet.0 * zoom) / 2.0, (area.1 - sheet.1 * zoom) / 2.0))
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px};

    use super::{MARGIN, MAX_ZOOM, MIN_ZOOM, band, fit, view_box, zoom_about};

    #[test]
    fn zooming_keeps_the_point_under_the_mouse() {
        let pan = point(px(20.0), px(30.0));
        let anchor = point(px(220.0), px(130.0));
        let (zoom, moved) = zoom_about(1.0, pan, anchor, 2.0);
        assert_eq!(zoom, 2.0);
        // Diagram point (200, 100) was under the anchor and still is.
        assert_eq!(moved.x + px(200.0 * zoom), anchor.x);
        assert_eq!(moved.y + px(100.0 * zoom), anchor.y);
        assert_eq!(zoom_about(1.0, pan, anchor, 100.0).0, MAX_ZOOM);
        assert_eq!(zoom_about(1.0, pan, anchor, 0.01).0, MIN_ZOOM);
    }

    #[test]
    fn opening_fits_the_drawing_but_does_not_enlarge_it() {
        let (zoom, pan) = fit((1000.0, 800.0), (400.0, 200.0), 1.0);
        assert_eq!(zoom, 1.0);
        assert_eq!(pan, (300.0, 300.0));
        let (zoom, _) = fit((1000.0, 800.0), (400.0, 200.0), MAX_ZOOM);
        assert_eq!(zoom, (1000.0 - 2.0 * MARGIN) / 400.0);
        let (zoom, pan) = fit((1000.0, 800.0), (1920.0, 760.0), 1.0);
        assert_eq!(zoom, 0.5);
        assert_eq!(pan.0, MARGIN);
        assert_eq!(fit((100.0, 100.0), (10000.0, 10000.0), 1.0).0, MIN_ZOOM);
    }

    #[test]
    fn the_image_knows_which_part_it_shows() {
        let svg = "<svg width=\"956\" height=\"659\" viewBox=\"-75 20 956 659\">";
        assert_eq!(view_box(svg), Some(umlcraft_erm::Rect::new(-75.0, 20.0, 956.0, 659.0)));
        let a = umlcraft_erm::Point::new(10.0, 50.0);
        let b = umlcraft_erm::Point::new(-5.0, 20.0);
        assert_eq!(band(a, b), umlcraft_erm::Rect::new(-5.0, 20.0, 15.0, 30.0));
    }
}
