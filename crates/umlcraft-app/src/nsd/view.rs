//! The view and editor of one structogram of the project: its drawing, zoom and panning,
//! selection, editing and export. It shows the structogram as the model holds it and changes
//! it through the workspace, so every change is a step of the project's undo history.
//!
//! The diagram is drawn by `umlcraft_nsd`'s SVG export and rasterized with resvg, so that the
//! view shows exactly what an export holds, turned texts and hatching included. The image is
//! made again when the diagram, the zoom or the screen's scale changes. Selection and the place
//! a drag would drop to are drawn over it, so they never reach an export. The diagram stays a
//! white sheet in the dark theme too, as in Structorizer; the area around it follows the theme.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::{ActiveTheme, Theme, WindowExt, v_flex};
use gpui_kit::*;
use resvg::{tiny_skia, usvg};
use rust_i18n::t;
use umlcraft_model::Id;
use umlcraft_nsd::layout::Layout;
use umlcraft_nsd::path::{self, Path as NsdPath, Selection, Slot, Span};
use umlcraft_nsd::{Color, Diagram, EditError, ElementKind, clip, edit};

use super::dialog::{self, Edited, Target};
use super::{
    AddBranch, EditElement, ExportStructogram, ExtendDown, ExtendUp, InsertElement, MoveDown,
    MoveUp, PALETTE, RemoveBranch, SelectDown, SelectLeft, SelectRight, SelectUp, SetColor,
    ToggleDisabled, kind_name,
};
use crate::workspace::Workspace;
use crate::{
    Cancel, Copy, Cut, DeleteSelection, ExportPng, ExportSvg, Paste, Redo, ResetZoom, Undo, ZoomIn,
    ZoomOut, ZoomToFit,
};

const MIN_ZOOM: f32 = 0.25;
const MAX_ZOOM: f32 = 4.0;
/// The longest side of the image a diagram is rasterized to, within GPU texture limits; a
/// larger diagram at a high zoom is scaled up from it.
const MAX_RASTER: f32 = 8192.0;
/// Space around the diagram at the start and when fitted.
const MARGIN: f32 = 20.0;
/// How far the mouse moves before pressing on an element becomes dragging it.
const DRAG_START: f32 = 4.0;

thread_local! {
    /// The system's fonts, loaded once for all rasterizing.
    static OPTIONS: Rc<usvg::Options<'static>> = {
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        Rc::new(options)
    };
}

/// The diagram rasterized for one scale.
struct Raster {
    scale: f32,
    image: Arc<RenderImage>,
    /// Its size in pixels of the image.
    size: (u32, u32),
    /// The diagram changed since; the image goes on the next paint, which has the window.
    outdated: bool,
}

enum Drag {
    /// Panning: where it started, in window coordinates, and the pan then.
    Pan { start: Point<Pixels>, pan: Point<Pixels> },
    /// Pressed on selected elements: where, whether the mouse has gone far enough to drag
    /// them, and the place they would drop to. `pressed` is what was hit inside a selection
    /// that stays for dragging; a click without dragging selects it after all.
    Elements {
        span: Span,
        start: Point<Pixels>,
        moving: bool,
        slot: Option<Slot>,
        pressed: Option<Selection>,
    },
}

/// What the selection allows, for the ribbon and the context menu.
#[derive(Clone, Copy, Default)]
pub struct Allowed {
    /// One element or the diagram: its dialog opens.
    pub edit: bool,
    /// A run of elements: cut, delete, move, colour, disable.
    pub span: bool,
    /// Every selected element is disabled.
    pub disabled: bool,
    /// A case or parallel element, or one of its branches.
    pub branching: bool,
}

pub struct NsdView {
    workspace: Entity<Workspace>,
    /// The structogram shown.
    id: Id,
    /// The structogram as the model held it at `revision`.
    diagram: Diagram,
    revision: u64,
    selection: Selection,
    /// What an import could not show as written, and whether that list is shown.
    notes: Vec<String>,
    notes_open: bool,
    layout: Layout,
    /// The diagram as SVG, drawn again after each change.
    svg: String,
    /// The SVG parsed, again only after a change.
    tree: Option<Arc<usvg::Tree>>,
    /// Counts the changes, so that an image rasterized meanwhile in the background is dropped.
    generation: u64,
    /// The diagram's size at 100 %.
    size: (f32, f32),
    zoom: f32,
    /// Where the diagram's top left corner is in the drawing area.
    pan: Point<Pixels>,
    raster: Option<Raster>,
    /// An image for another zoom is being rasterized in the background.
    rasterizing: bool,
    /// The drawing area, as painted last.
    area: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Drag>,
    /// What the open element dialog holds, taken over when the project is saved or closed.
    dialog: Option<dialog::Pending>,
    /// Where a selection extended with Shift began; that end stays put.
    anchor: Option<NsdPath>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Structogram `id` as the model holds it, if it is one.
fn structogram(workspace: &Entity<Workspace>, id: Id, cx: &App) -> Option<Diagram> {
    let model = workspace.read(cx).doc.model();
    Some(model.get(id)?.kind().as_structogram()?.structogram.clone())
}

impl NsdView {
    /// The view of structogram `id`, with what its import could not show (`notes`).
    pub fn new(
        workspace: Entity<Workspace>,
        id: Id,
        notes: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            // Undo, redo and other edits of the project may change the structogram.
            cx.observe(&workspace, |this, _, cx| {
                if this.sync(cx) {
                    cx.notify();
                }
            }),
            // The area around the diagram follows the theme.
            cx.observe_global::<Theme>(|_, cx| cx.notify()),
        ];
        let diagram = structogram(&workspace, id, cx).unwrap_or_else(|| {
            let made = umlcraft_nsd::Provenance { author: "", date: "", origin: "" };
            Diagram::new(umlcraft_nsd::RootKind::Program, &[], &made)
        });
        let revision = workspace.read(cx).doc.revision();
        let layout = Layout::of(&diagram, umlcraft_nsd::Metrics::for_platform());
        let mut view = NsdView {
            workspace,
            id,
            diagram,
            revision,
            selection: Selection::Nothing,
            notes,
            notes_open: false,
            layout,
            svg: String::new(),
            tree: None,
            generation: 0,
            size: (0.0, 0.0),
            zoom: 1.0,
            pan: point(px(MARGIN), px(MARGIN)),
            raster: None,
            rasterizing: false,
            area: Rc::default(),
            drag: None,
            dialog: None,
            anchor: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        view.draw();
        view
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Takes over a changed structogram from the model; whether anything changed.
    fn sync(&mut self, cx: &mut Context<Self>) -> bool {
        let revision = self.workspace.read(cx).doc.revision();
        if revision == self.revision {
            return false;
        }
        self.revision = revision;
        let Some(diagram) = structogram(&self.workspace, self.id, cx) else { return false };
        if diagram == self.diagram {
            return false;
        }
        self.diagram = diagram;
        // The change has made a selection of its own, or left one that no longer fits.
        self.anchor = None;
        if !self.selection.is_valid(&self.diagram) {
            self.selection = Selection::Nothing;
        }
        self.layout = Layout::of(&self.diagram, umlcraft_nsd::Metrics::for_platform());
        self.draw();
        true
    }

    /// Draws the diagram as laid out.
    fn draw(&mut self) {
        let metrics = umlcraft_nsd::Metrics::for_platform();
        self.svg = umlcraft_nsd::svg::render_layout(&self.diagram, &self.layout, metrics);
        self.tree =
            OPTIONS.with(|options| usvg::Tree::from_str(&self.svg, options)).ok().map(Arc::new);
        self.generation += 1;
        let (w, h) = self.layout.size;
        self.size = ((w + 1) as f32, (h + 1) as f32);
        if let Some(raster) = &mut self.raster {
            raster.outdated = true;
        }
    }

    /// Makes one undoable change of the project and shows it. Changes that are not possible
    /// (e.g. removing a case's last branch) do nothing.
    fn change(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Diagram) -> Result<Selection, EditError>,
    ) {
        let mut changed = self.diagram.clone();
        let Ok(selection) = f(&mut changed) else {
            cx.notify();
            return;
        };
        if changed != self.diagram {
            let id = self.id;
            self.workspace.update(cx, |ws, cx| {
                ws.edit_structogram(id, &changed);
                cx.notify();
            });
            self.sync(cx);
        }
        self.select(selection, cx);
    }

    fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.anchor = None;
        if selection.is_valid(&self.diagram) {
            self.selection = selection;
        }
        cx.notify();
    }

    /// Selects `span`, stretched with Shift from its end at `anchor`.
    fn stretch(&mut self, span: Span, anchor: NsdPath, cx: &mut Context<Self>) {
        if Selection::Elements(span.clone()).is_valid(&self.diagram) {
            self.selection = Selection::Elements(span);
        }
        self.anchor = Some(anchor);
        cx.notify();
    }

    /// What the selection allows.
    pub fn allowed(&self) -> Allowed {
        let d = &self.diagram;
        let span = self.span();
        Allowed {
            edit: matches!(self.selection, Selection::Root) || self.selection.single().is_some(),
            span: span.is_some(),
            disabled: span.as_ref().is_some_and(|s| {
                (s.start..s.end)
                    .all(|i| path::element(d, &s.element(i)).is_some_and(|e| e.disabled()))
            }),
            branching: self.branching().is_some(),
        }
    }

    // ----- Zoom and pan ----------------------------------------------------------------------

    /// The diagram as an image at `scale` pixels per diagram pixel (less for a large diagram,
    /// see [`MAX_RASTER`]), its size in pixels and the scale it has.
    ///
    /// After a change, and for the first paint, the image is made right away, so that it
    /// matches the selection painted over it. Another zoom only scales the image there is
    /// until one for the new zoom is rasterized in the background, one at a time: a large
    /// diagram takes long enough to make zooming stutter.
    fn raster(
        &mut self,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Arc<RenderImage>, (u32, u32), f32)> {
        let scale = scale.min(MAX_RASTER / self.size.0.max(self.size.1));
        // Whether the image there is shows the diagram as it is, and at this scale.
        match self.raster.as_ref().filter(|r| !r.outdated).map(|r| (r.scale - scale).abs() <= 1e-4)
        {
            Some(true) => {}
            Some(false) => self.rasterize_later(scale, window, cx),
            None => {
                let image = self.tree.as_deref().and_then(|tree| rasterize(tree, scale));
                self.replace_raster(image, scale, window);
            }
        }
        self.raster.as_ref().map(|r| (r.image.clone(), r.size, r.scale))
    }

    fn rasterize_later(&mut self, scale: f32, window: &mut Window, cx: &mut Context<Self>) {
        if self.rasterizing {
            return;
        }
        let Some(tree) = self.tree.clone() else { return };
        self.rasterizing = true;
        let generation = self.generation;
        cx.spawn_in(window, async move |this, cx| {
            let image =
                cx.background_executor().spawn(async move { rasterize(&tree, scale) }).await;
            this.update_in(cx, |this, window, cx| {
                this.rasterizing = false;
                // A change meanwhile has made an image of its own.
                if this.generation == generation {
                    this.replace_raster(image, scale, window);
                }
                // Paints again, which asks for the zoom of now if it has moved on.
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn replace_raster(
        &mut self,
        image: Option<(Arc<RenderImage>, (u32, u32))>,
        scale: f32,
        window: &mut Window,
    ) {
        // GPUI keeps every image it painted in the window's atlas until it is dropped there;
        // without this each zoom step and each change would leave its image behind.
        if let Some(old) = self.raster.take() {
            window.drop_image(old.image).ok();
        }
        self.raster = image.map(|(image, size)| Raster { scale, image, size, outdated: false });
    }

    fn set_zoom(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        // Keep the point under `anchor` (relative to the area) where it is.
        let factor = zoom / self.zoom;
        self.pan = point(
            anchor.x - (anchor.x - self.pan.x) * factor,
            anchor.y - (anchor.y - self.pan.y) * factor,
        );
        self.zoom = zoom;
        cx.notify();
    }

    fn centre(&self) -> Point<Pixels> {
        let area = self.area.get();
        point(area.size.width / 2.0, area.size.height / 2.0)
    }

    /// Zooms about the middle of the area; the main window's zoom commands end up here too.
    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * factor, self.centre(), cx);
    }

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1.25, cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_by(1.0 / 1.25, cx);
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = point(px(MARGIN), px(MARGIN));
        cx.notify();
    }

    fn on_reset_zoom(&mut self, _: &ResetZoom, _: &mut Window, cx: &mut Context<Self>) {
        self.reset_zoom(cx);
    }

    fn on_zoom_to_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to_fit(cx);
    }

    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        let area = self.area.get().size;
        let fit_w = (f32::from(area.width) - 2.0 * MARGIN) / self.size.0;
        let fit_h = (f32::from(area.height) - 2.0 * MARGIN) / self.size.1;
        self.zoom = fit_w.min(fit_h).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = point(
            (area.width - px(self.size.0 * self.zoom)) / 2.0,
            (area.height - px(self.size.1 * self.zoom)) / 2.0,
        );
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = ev.delta.pixel_delta(px(20.0));
        if ev.modifiers.secondary(cx) {
            let origin = self.area.get().origin;
            let anchor = point(ev.position.x - origin.x, ev.position.y - origin.y);
            let factor = (1.0 + f32::from(delta.y) / 200.0).clamp(0.5, 2.0);
            self.set_zoom(self.zoom * factor, anchor, cx);
        } else {
            self.pan = point(self.pan.x + delta.x, self.pan.y + delta.y);
            cx.notify();
        }
    }

    // ----- Mouse ------------------------------------------------------------------------------

    /// A window position in diagram pixels.
    fn to_diagram(&self, position: Point<Pixels>) -> (i32, i32) {
        let origin = self.area.get().origin;
        let x = f32::from(position.x - origin.x - self.pan.x) / self.zoom;
        let y = f32::from(position.y - origin.y - self.pan.y) / self.zoom;
        (x.floor() as i32, y.floor() as i32)
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (x, y) = self.to_diagram(ev.position);
        let Some(hit) = self.layout.hit(x, y) else {
            self.drag = Some(Drag::Pan { start: ev.position, pan: self.pan });
            self.select(Selection::Nothing, cx);
            return;
        };
        if ev.click_count >= 2 {
            self.drag = None;
            match hit {
                Selection::Empty(seq) => {
                    // As in Structorizer: a double click into an empty branch adds an
                    // instruction there.
                    let slot = Slot { seq, index: 0 };
                    self.open_new(slot, ElementKind::Instruction, window, cx);
                }
                hit => {
                    self.select(hit, cx);
                    self.edit_element(&EditElement, window, cx);
                }
            }
            return;
        }
        let current = self.selection.clone();
        if ev.modifiers.shift
            && let (Some(span), Some(path)) = (current.span(), hit.single())
            && path.len() == span.seq.len() + 1
            && path.starts_with(&span.seq)
        {
            let to = *path.last().unwrap_or(&0);
            let anchor = span.element(span.fixed(self.anchor.as_deref()));
            let span = span.to(Some(&anchor), to);
            self.stretch(span, anchor, cx);
            return;
        }
        // Pressing on what is selected keeps it, so that several elements can be dragged.
        let keep = match (&current, &hit) {
            (Selection::Elements(span), Selection::Elements(h)) => span.covers(&h.first()),
            _ => false,
        };
        let (selection, pressed) =
            if keep { (current.clone(), (hit != current).then_some(hit)) } else { (hit, None) };
        if let Selection::Elements(span) = &selection {
            self.drag = Some(Drag::Elements {
                span: span.clone(),
                start: ev.position,
                moving: false,
                slot: None,
                pressed,
            });
        }
        self.select(selection, cx);
    }

    /// A right click selects what it hits, unless that is selected already, so that the
    /// context menu acts on it.
    fn on_right_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (x, y) = self.to_diagram(ev.position);
        let hit = self.layout.hit(x, y).unwrap_or_default();
        let inside = match (&self.selection, &hit) {
            (Selection::Elements(span), Selection::Elements(h)) => span.covers(&h.first()),
            (current, hit) => current == hit,
        };
        if !inside {
            self.select(hit, cx);
        }
    }

    fn on_middle_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.drag = Some(Drag::Pan { start: ev.position, pan: self.pan });
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.pressed_button.is_none() {
            if self.drag.take().is_some() {
                cx.notify();
            }
            return;
        }
        let target = self.to_diagram(ev.position);
        let layout = &self.layout;
        match &mut self.drag {
            Some(Drag::Pan { start, pan }) => {
                self.pan = point(pan.x + ev.position.x - start.x, pan.y + ev.position.y - start.y);
                cx.notify();
            }
            Some(Drag::Elements { span, start, moving, slot, .. }) => {
                let moved = ev.position - *start;
                if !*moving && f32::from(moved.x.abs().max(moved.y.abs())) < DRAG_START {
                    return;
                }
                *moving = true;
                let copy = ev.modifiers.secondary(cx);
                *slot =
                    layout.slot_at(target.0, target.1).filter(|s| drops_somewhere(span, s, copy));
                cx.notify();
            }
            None => {}
        }
    }

    fn on_mouse_up(&mut self, ev: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        // With Ctrl (Cmd) held the elements are copied, otherwise moved.
        let copy = ev.modifiers.secondary(cx);
        let (span, slot) = match self.drag.take() {
            Some(Drag::Elements { span, moving: true, slot: Some(slot), .. }) => (span, slot),
            // A click into a selection selects what it hit, e.g. an element nested in it.
            Some(Drag::Elements { moving: false, pressed: Some(hit), .. }) => {
                self.select(hit, cx);
                return;
            }
            _ => {
                cx.notify();
                return;
            }
        };
        if !drops_somewhere(&span, &slot, copy) {
            cx.notify();
            return;
        }
        if copy {
            self.change(cx, |d| {
                let copies = edit::copy(d, &span)?;
                edit::insert(d, &slot, copies)
            });
        } else {
            self.change(cx, |d| edit::move_span(d, &span, &slot));
        }
    }

    // ----- Editing ---------------------------------------------------------------------------

    fn span(&self) -> Option<Span> {
        self.selection.span().cloned()
    }

    /// Where something new goes: after the selection, or at the end of the diagram.
    fn slot(&self, after: bool) -> Slot {
        self.selection.slot(after).unwrap_or_else(|| Slot {
            seq: Vec::new(),
            index: self.diagram.children.elements.len(),
        })
    }

    fn open_new(
        &mut self,
        slot: Slot,
        kind: ElementKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keywords = umlcraft_nsd::Keywords::of(&self.diagram);
        let start = Edited {
            text: edit::default_text(kind, keywords),
            comment: Vec::new(),
            disabled: false,
            kind: self.diagram.kind(),
        };
        let target = Target::New(slot, kind);
        let view = cx.entity().downgrade();
        if let Some(pending) =
            dialog::open(view, &self.focus, target, Some(kind), keywords, start, window, cx)
        {
            self.dialog = Some(pending);
        }
    }

    fn edit_element(&mut self, _: &EditElement, window: &mut Window, cx: &mut Context<Self>) {
        let d = &self.diagram;
        let (target, kind, start) = match &self.selection {
            Selection::Root => (
                Target::Root,
                None,
                Edited { text: d.text(), comment: d.comment(), disabled: false, kind: d.kind() },
            ),
            selection => {
                let Some(p) = selection.single() else { return };
                let Some(e) = path::element(d, &p) else { return };
                let start = Edited {
                    text: e.text(),
                    comment: e.comment(),
                    disabled: e.disabled(),
                    kind: d.kind(),
                };
                (Target::Element(p), Some(e.kind), start)
            }
        };
        let view = cx.entity().downgrade();
        let keywords = umlcraft_nsd::Keywords::of(&self.diagram);
        if let Some(pending) =
            dialog::open(view, &self.focus, target, kind, keywords, start, window, cx)
        {
            self.dialog = Some(pending);
        }
    }

    /// Takes what the element dialog hands back.
    pub fn apply_dialog(
        &mut self,
        target: Target,
        edited: Edited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dialog = None;
        let Edited { text, comment, disabled, kind } = edited;
        match target {
            Target::New(slot, element_kind) => self.change(cx, |d| {
                let mut e = edit::new_element(d, element_kind, &text);
                e.set_comment(&comment);
                e.set_disabled(disabled);
                edit::insert(d, &slot, vec![e])
            }),
            Target::Element(at) => self.change(cx, |d| {
                if edit::has_text(path::element(d, &at).ok_or(EditError::NoSuchPlace)?.kind) {
                    edit::set_text(d, &at, &text)?;
                }
                edit::set_comment(d, &at, &comment)?;
                path::element_mut(d, &at).ok_or(EditError::NoSuchPlace)?.set_disabled(disabled);
                Ok(Selection::element(&at))
            }),
            Target::Root => self.change(cx, |d| edit::set_root(d, &text, &comment, kind)),
        }
        window.focus(&self.focus, cx);
    }

    fn insert_element(&mut self, a: &InsertElement, window: &mut Window, cx: &mut Context<Self>) {
        let slot = self.slot(a.after);
        self.open_new(slot, a.kind, window, cx);
    }

    fn delete(&mut self, _: &DeleteSelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::remove(d, &span).map(|(_, selection)| selection));
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    /// Puts the selection on the clipboard; whether there was something to put there.
    fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        let d = &self.diagram;
        let text = match &self.selection {
            Selection::Root => clip::copy_diagram(d),
            Selection::Elements(span) => match edit::copy(d, span) {
                Ok(elements) => clip::copy(d, elements),
                Err(_) => return false,
            },
            _ => return false,
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        // Only what is safe on the clipboard goes.
        if self.span().is_some() && self.copy_selection(cx) {
            self.delete(&DeleteSelection, window, cx);
        }
    }

    /// Pastes after the selection (into an empty branch when that is selected).
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let elements = clip::paste(&text);
        if elements.is_empty() {
            return;
        }
        let slot = self.slot(true);
        self.change(cx, |d| edit::insert(d, &slot, elements));
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        self.select(Selection::Nothing, cx);
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::shift(d, &span, true));
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::shift(d, &span, false));
        }
    }

    fn set_color(&mut self, a: &SetColor, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::set_color(d, &span, Color(a.color)));
        }
    }

    fn toggle_disabled(&mut self, _: &ToggleDisabled, _: &mut Window, cx: &mut Context<Self>) {
        let Some(span) = self.span() else { return };
        let d = &self.diagram;
        let all = (span.start..span.end)
            .all(|i| path::element(d, &span.element(i)).is_some_and(|e| e.disabled()));
        self.change(cx, |d| edit::set_disabled(d, &span, !all));
    }

    /// The case or parallel element whose branches the selection is about, and the branch
    /// meant: a selected empty branch, or none for the element itself.
    fn branching(&self) -> Option<(NsdPath, Option<usize>)> {
        let d = &self.diagram;
        let branching = |p: &[usize]| {
            path::element(d, p)
                .is_some_and(|e| matches!(e.kind, ElementKind::Case | ElementKind::Parallel))
        };
        match &self.selection {
            Selection::Empty(seq) => {
                let owner = edit::owner(seq)?;
                branching(&owner).then(|| (owner, seq.last().copied()))
            }
            selection => selection.single().filter(|p| branching(p)).map(|p| (p, None)),
        }
    }

    fn add_branch(&mut self, _: &AddBranch, _: &mut Window, cx: &mut Context<Self>) {
        let Some((at, _)) = self.branching() else { return };
        let Some(e) = path::element(&self.diagram, &at) else { return };
        // A new case goes before the default branch.
        let index = match e.kind {
            ElementKind::Case => e.branches.len() - 1,
            _ => e.branches.len(),
        };
        self.change(cx, |d| edit::add_branch(d, &at, index));
    }

    fn remove_branch(&mut self, _: &RemoveBranch, _: &mut Window, cx: &mut Context<Self>) {
        let Some((at, branch)) = self.branching() else { return };
        let Some(e) = path::element(&self.diagram, &at) else { return };
        let last = match e.kind {
            ElementKind::Case => e.branches.len().saturating_sub(2),
            _ => e.branches.len().saturating_sub(1),
        };
        let index = branch.unwrap_or(last);
        self.change(cx, |d| edit::remove_branch(d, &at, index));
    }

    // ----- Keyboard selection ----------------------------------------------------------------

    /// The first element of a sequence, or the sequence when it is empty.
    fn first_in(&self, seq: NsdPath) -> Selection {
        let empty = path::sequence(&self.diagram, &seq).is_none_or(|s| s.elements.is_empty());
        if empty {
            Selection::Empty(seq)
        } else {
            let mut p = seq;
            p.push(0);
            Selection::element(&p)
        }
    }

    /// The element after `p`, climbing out of branches at their end.
    fn after(&self, p: &[usize]) -> Option<Selection> {
        let (last, seq) = p.split_last()?;
        let len = path::sequence(&self.diagram, seq)?.elements.len();
        if last + 1 < len {
            let mut next = seq.to_vec();
            next.push(last + 1);
            return Some(Selection::element(&next));
        }
        self.after(&edit::owner(seq)?)
    }

    /// The branch next to the one holding `seq` (`step` -1 or 1).
    fn beside(&self, seq: &[usize], step: isize) -> Option<Selection> {
        let owner = edit::owner(seq)?;
        let branch = seq.last()?.checked_add_signed(step)?;
        let e = path::element(&self.diagram, &owner)?;
        // A case without default hides its last branch.
        let shown = match e.kind {
            ElementKind::Case if e.text().last().is_some_and(|l| l.trim() == "%") => {
                e.branches.len() - 1
            }
            _ => e.branches.len(),
        };
        (branch < shown).then(|| {
            let mut s = owner;
            s.push(branch);
            self.first_in(s)
        })
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) if span.start > 0 => {
                Some(Selection::element(&span.element(span.start - 1)))
            }
            Selection::Elements(Span { seq, .. }) | Selection::Empty(seq) => {
                edit::owner(seq).map(|o| Selection::element(&o))
            }
            Selection::Nothing => Some(self.first_in(Vec::new())),
            Selection::Root => None,
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) => self.after(&span.element(span.end - 1)),
            Selection::Empty(seq) => edit::owner(seq).and_then(|o| self.after(&o)),
            Selection::Nothing | Selection::Root => Some(self.first_in(Vec::new())),
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let seq = match &self.selection {
            Selection::Elements(span) => span.seq.clone(),
            Selection::Empty(seq) => seq.clone(),
            _ => return,
        };
        let next =
            self.beside(&seq, -1).or_else(|| edit::owner(&seq).map(|o| Selection::element(&o)));
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) => {
                let p = span.first();
                let branches = path::element(&self.diagram, &p).map_or(0, |e| e.branches.len());
                if span.len() == 1 && branches > 0 {
                    let mut s = p;
                    s.push(0);
                    Some(self.first_in(s))
                } else {
                    self.beside(&span.seq, 1)
                }
            }
            Selection::Empty(seq) => self.beside(seq, 1),
            _ => None,
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn extend(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(span) = self.span() else { return };
        let len = path::sequence(&self.diagram, &span.seq).map_or(0, |s| s.elements.len());
        let anchor = span.element(span.fixed(self.anchor.as_deref()));
        if let Some(span) = span.step(Some(&anchor), up, len) {
            self.stretch(span, anchor, cx);
        }
    }

    fn extend_up(&mut self, _: &ExtendUp, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(true, cx);
    }

    fn extend_down(&mut self, _: &ExtendDown, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(false, cx);
    }

    // ----- Export ----------------------------------------------------------------------------

    /// The file name an export suggests: the structogram's name, as its signature begins.
    fn file_stem(&self) -> String {
        let text = self.diagram.text().join(" ");
        let name = text.split('(').next().unwrap_or_default().trim();
        let name = if name.is_empty() { t!("nsd.untitled_stem").to_string() } else { name.into() };
        crate::main_view::sanitize_file_name(&name)
    }

    /// Asks where to write the export with `extension` (next to the project), then writes it
    /// in the background; a failure is reported with `failed`.
    fn write_export(
        &self,
        extension: &'static str,
        failed: &'static str,
        bytes: impl FnOnce() -> anyhow::Result<Vec<u8>> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directory = crate::main_view::start_directory(self.workspace.read(cx).path.as_deref());
        let suggested = format!("{}.{extension}", self.file_stem());
        let rx = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = crate::main_view::with_extension(path, extension);
            let written = cx
                .background_executor()
                .spawn(async move { anyhow::Ok(write_file(&path, &bytes()?)?) })
                .await;
            if let Err(err) = written {
                cx.update(|window, cx| {
                    let rx = window.prompt(
                        PromptLevel::Critical,
                        &t!(failed),
                        Some(&err.to_string()),
                        &[t!("dialog.ok").as_ref()],
                        cx,
                    );
                    cx.spawn(async move |_| rx.await.ok()).detach();
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn export_image(&mut self, png: bool, window: &mut Window, cx: &mut Context<Self>) {
        let svg = self.svg.clone();
        let bytes = move || {
            Ok(if png {
                // `from_svg` takes images made at Astah's 4/3 pixels per unit; ours have one.
                let scale = crate::png::SCALE * umlcraft_notation::svg::SCALE;
                crate::png::from_svg(&svg, scale)?
            } else {
                svg.into_bytes()
            })
        };
        let extension = if png { "png" } else { "svg" };
        self.write_export(extension, "dialog.image_failed", bytes, window, cx);
    }

    /// Puts the structogram on the clipboard as SVG and as PNG.
    pub fn copy_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scale = crate::png::SCALE * umlcraft_notation::svg::SCALE;
        crate::png::copy(self.svg.clone(), scale, window, cx);
    }

    fn export_svg(&mut self, _: &ExportSvg, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(false, window, cx);
    }

    fn export_png(&mut self, _: &ExportPng, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(true, window, cx);
    }

    /// Writes the structogram as a Structorizer file, with a header of today.
    fn export_nsd(&mut self, _: &ExportStructogram, window: &mut Window, cx: &mut Context<Self>) {
        self.flush_dialog(window, cx);
        let (author, date, origin) = (super::user_name(), super::today(), super::origin());
        let made = umlcraft_nsd::Provenance { author: &author, date: &date, origin: &origin };
        let text = umlcraft_nsd::write(&umlcraft_nsd::content::to_file(&self.diagram, &made));
        let bytes = move || Ok(text.into_bytes());
        self.write_export("nsd", "nsd.save_failed", bytes, window, cx);
    }

    /// Takes over what the open element dialog holds, as OK would, so that saving and closing
    /// the project keep it instead of losing it. An untouched dialog just closes.
    pub fn flush_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pending) = self.dialog.take() else { return };
        if !window.has_active_dialog(cx) {
            return;
        }
        let edited = pending(cx);
        window.close_dialog(cx);
        if let Some((target, edited)) = edited {
            self.apply_dialog(target, edited, window, cx);
        }
    }

    // ----- Drawing ---------------------------------------------------------------------------

    /// What to draw over the diagram, in diagram pixels: the selection's boxes, and where a
    /// drag would drop (a line between elements, or a box for an empty branch).
    fn overlay(&self) -> (Vec<[i32; 4]>, Option<[i32; 4]>) {
        let l = &self.layout;
        let boxes = match &self.selection {
            Selection::Nothing => Vec::new(),
            Selection::Root => vec![[0, 0, l.size.0, l.size.1]],
            // A try's empty `finally` is not drawn; it shows where it would go.
            Selection::Empty(seq) => l
                .sequence(seq)
                .map(|s| s.rect)
                .or_else(|| l.hidden_finally(seq))
                .map(|r| vec![rect(r)])
                .unwrap_or_default(),
            Selection::Elements(span) => {
                let first = l.element(&span.first());
                let last = l.element(&span.element(span.end - 1));
                match (first, last) {
                    (Some(a), Some(b)) => {
                        vec![[a.rect.x, a.rect.y, a.rect.w, b.rect.bottom() - a.rect.y]]
                    }
                    _ => Vec::new(),
                }
            }
        };
        let drop = match &self.drag {
            Some(Drag::Elements { moving: true, slot: Some(slot), .. }) => l
                .sequence(&slot.seq)
                .map(|s| match s.elements.get(slot.index) {
                    _ if s.elements.is_empty() => rect(s.rect),
                    Some(e) => [s.rect.x, e.rect.y - 1, s.rect.w, 3],
                    None => {
                        let bottom = s.elements.last().map_or(s.rect.y, |e| e.rect.bottom());
                        [s.rect.x, bottom - 1, s.rect.w, 3]
                    }
                })
                .or_else(|| l.hidden_finally(&slot.seq).map(rect)),
            _ => None,
        };
        (boxes, drop)
    }

    /// What an import could not show as written: a line that opens into the list.
    fn notes_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.notes.is_empty() {
            return None;
        }
        let summary = t!("nsd.notes", count = self.notes.len()).to_string();
        let arrow = if self.notes_open { "▾" } else { "▸" };
        let list = self.notes_open.then(|| {
            v_flex()
                .id("nsd-notes")
                .max_h(px(140.0))
                .overflow_y_scroll()
                .pl_4()
                .children(self.notes.iter().map(|n| div().text_xs().child(n.clone())))
        });
        Some(
            v_flex()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_sm()
                .child(
                    div()
                        .id("nsd-notes-toggle")
                        .cursor_pointer()
                        .child(format!("{arrow} {summary}"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notes_open = !this.notes_open;
                            cx.notify();
                        })),
                )
                .children(list),
        )
    }

    /// The context menu: what the selection allows.
    fn context_menu(
        view: &Entity<Self>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (selection, allowed, can_undo, can_redo, focus) = {
            let this = view.read(cx);
            let doc = &this.workspace.read(cx).doc;
            (
                this.selection.clone(),
                this.allowed(),
                doc.can_undo(),
                doc.can_redo(),
                this.focus.clone(),
            )
        };
        let (branching, disabled) = (allowed.branching, allowed.disabled);
        let editable = matches!(selection, Selection::Root) || selection.single().is_some();
        let has_span = selection.span().is_some();
        let copyable = has_span || matches!(selection, Selection::Root);
        let mut menu = menu
            .action_context(focus)
            .menu_with_disabled(t!("nsd.edit"), Box::new(EditElement), !editable)
            .separator();
        for (title, after) in [("nsd.insert_after", true), ("nsd.insert_before", false)] {
            menu = menu.submenu(t!(title), window, cx, move |menu, _, _| {
                ElementKind::ALL.into_iter().fold(menu, |menu, kind| {
                    menu.menu(kind_name(kind), Box::new(InsertElement { kind, after }))
                })
            });
        }
        menu = menu
            .separator()
            .menu_with_disabled(t!("menu.cut"), Box::new(Cut), !has_span)
            .menu_with_disabled(t!("menu.copy"), Box::new(Copy), !copyable)
            .menu(t!("nsd.paste"), Box::new(Paste))
            .menu_with_disabled(t!("nsd.delete"), Box::new(DeleteSelection), !has_span)
            .separator()
            .menu_with_disabled(t!("nsd.move_up"), Box::new(MoveUp), !has_span)
            .menu_with_disabled(t!("nsd.move_down"), Box::new(MoveDown), !has_span)
            .separator();
        if has_span {
            menu = menu.submenu(t!("nsd.color"), window, cx, |menu, _, _| {
                PALETTE.into_iter().enumerate().fold(menu, |menu, (i, color)| {
                    menu.menu(t!(format!("nsd.color_{i}")), Box::new(SetColor { color }))
                })
            });
            menu = menu.menu_with_check(t!("nsd.disabled"), disabled, Box::new(ToggleDisabled));
        }
        if branching {
            menu = menu
                .menu(t!("nsd.add_branch"), Box::new(AddBranch))
                .menu(t!("nsd.remove_branch"), Box::new(RemoveBranch));
        }
        menu.separator()
            .menu_with_disabled(t!("toolbar.undo"), Box::new(Undo), !can_undo)
            .menu_with_disabled(t!("toolbar.redo"), Box::new(Redo), !can_redo)
    }
}

/// Whether dropping `span` at `slot` would change anything and is possible. A copy goes
/// anywhere, also next to or into the elements it copies; they cannot move there.
fn drops_somewhere(span: &Span, slot: &Slot, copy: bool) -> bool {
    let in_place = slot.seq == span.seq && (span.start..=span.end).contains(&slot.index);
    copy || (!in_place && !span.covers(&slot.seq))
}

fn rect(r: umlcraft_nsd::layout::Rect) -> [i32; 4] {
    [r.x, r.y, r.w, r.h]
}

/// Writes `bytes` through a temporary file, so that a failed write leaves the old file whole.
fn write_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = std::path::PathBuf::from(temporary);
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        std::fs::remove_file(&temporary).ok();
    })
}

/// The parsed SVG rasterized at `scale`, as GPUI takes images (BGRA, not premultiplied).
fn rasterize(tree: &usvg::Tree, scale: f32) -> Option<(Arc<RenderImage>, (u32, u32))> {
    let size = tree.size().to_int_size().scale_by(scale)?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())?;
    resvg::render(tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut buffer = image::ImageBuffer::from_raw(w, h, pixmap.take())?;
    for pixel in buffer.as_chunks_mut::<4>().0 {
        // The sheet is opaque nearly everywhere, and there premultiplied is straight already:
        // swapping red and blue is enough and several times faster than dividing.
        if pixel[3] == 255 {
            pixel.swap(0, 2);
        } else {
            swap_rgba_pa_to_bgra(pixel);
        }
    }
    Some((Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])), (w, h)))
}

impl Render for NsdView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let menu_owner = cx.entity();
        let area = self.area.clone();
        let surround = cx.theme().muted;
        let accent = cx.theme().blue;
        // The view fills a card of the main window: its backgrounds keep inside the card's
        // rounded corners (inside its 1 px border), as the diagram canvas does.
        let corner = crate::shell::RADIUS - px(1.0);
        v_flex()
            .size_full()
            .rounded(corner)
            .bg(cx.theme().background)
            .track_focus(&self.focus)
            .key_context("NsdView")
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::on_reset_zoom))
            .on_action(cx.listener(Self::on_zoom_to_fit))
            .on_action(cx.listener(Self::export_svg))
            .on_action(cx.listener(Self::export_png))
            .on_action(cx.listener(Self::export_nsd))
            .on_action(cx.listener(Self::edit_element))
            .on_action(cx.listener(Self::insert_element))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::move_up))
            .on_action(cx.listener(Self::move_down))
            .on_action(cx.listener(Self::set_color))
            .on_action(cx.listener(Self::toggle_disabled))
            .on_action(cx.listener(Self::add_branch))
            .on_action(cx.listener(Self::remove_branch))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::extend_up))
            .on_action(cx.listener(Self::extend_down))
            .child(
                div()
                    .id("structogram")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .rounded(corner)
                    .bg(surround)
                    .on_scroll(cx.listener(Self::on_scroll))
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(|this, _: &MouseUpEvent, _, _| this.drag = None),
                    )
                    .child(
                        canvas(
                            move |bounds, _, _| area.set(bounds),
                            move |bounds, _, window, cx| {
                                let scale = window.scale_factor();
                                let (pan, zoom, raster, (boxes, drop)) =
                                    this.update(cx, |this, cx| {
                                        let raster = this.raster(this.zoom * scale, window, cx);
                                        (this.pan, this.zoom, raster, this.overlay())
                                    });
                                let Some((image, (w, h), raster_scale)) = raster else {
                                    return;
                                };
                                let origin =
                                    point(bounds.origin.x + pan.x, bounds.origin.y + pan.y);
                                // The image covers the diagram at the zoom, whatever its scale.
                                let shown = zoom / raster_scale;
                                let size = size(px(w as f32 * shown), px(h as f32 * shown));
                                let image_bounds = Bounds::new(origin, size);
                                let on_screen = |[x, y, w, h]: [i32; 4]| {
                                    Bounds::new(
                                        point(
                                            origin.x + px(x as f32 * zoom),
                                            origin.y + px(y as f32 * zoom),
                                        ),
                                        gpui_kit::size(px(w as f32 * zoom), px(h as f32 * zoom)),
                                    )
                                };
                                window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                    window
                                        .paint_image(
                                            image_bounds,
                                            image_bounds,
                                            Corners::default(),
                                            image,
                                            0,
                                            false,
                                        )
                                        .ok();
                                    for b in boxes {
                                        window.paint_quad(quad(
                                            on_screen(b),
                                            px(0.0),
                                            accent.opacity(0.12),
                                            px(2.0),
                                            accent,
                                            BorderStyle::Solid,
                                        ));
                                    }
                                    if let Some(d) = drop {
                                        window.paint_quad(fill(on_screen(d), accent));
                                    }
                                });
                            },
                        )
                        .size_full(),
                    )
                    .context_menu(move |menu, window, cx| {
                        Self::context_menu(&menu_owner, menu, window, cx)
                    }),
            )
            .children(self.notes_bar(cx))
    }
}

impl Focusable for NsdView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
