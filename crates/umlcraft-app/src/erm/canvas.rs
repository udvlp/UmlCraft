//! The canvas of an ER diagram: the SVG image of `umlcraft_erm::svg` rasterized with resvg, so
//! that it shows exactly what an export holds, with zoom and panning. The image is made again
//! when the zoom, the screen's scale or the grid changes, in the background while the old one
//! is scaled meanwhile, and at most 8192 px on its longer side (larger is scaled up from it).
//! The sheet is white in the dark theme too, as in draw.io; the area around it follows the
//! theme. The grid (10 px, every fourth line stronger, like draw.io's) is part of the image
//! shown, never of an export.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use resvg::usvg;
use umlcraft_erm::Diagram;

use super::ToggleGrid;
use crate::nsd::{OPTIONS, rasterize};
use crate::{ResetZoom, ZoomIn, ZoomOut, ZoomToFit};

pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 4.0;
/// One step of the zoom buttons and keys.
const ZOOM_STEP: f32 = 1.25;
/// The longest side of the image, within GPU texture limits.
const MAX_RASTER: f32 = 8192.0;
/// Space around the sheet at 100 % and when fitted.
const MARGIN: f32 = 20.0;
/// draw.io's grid: 10 px apart, every fourth line in full colour, the others at 20 %.
const GRID_SIZE: f64 = 10.0;
const GRID_STEPS: u32 = 4;
const GRID_COLOR: &str = "#e6e6e6";

/// The diagram rasterized for one scale.
struct Raster {
    scale: f32,
    image: Arc<RenderImage>,
    /// Its size in pixels of the image.
    size: (u32, u32),
}

/// Panning with the middle button, or the left one while Space is held: where it started, in
/// window coordinates, and the pan then.
struct Pan {
    start: Point<Pixels>,
    pan: Point<Pixels>,
}

pub struct ErmCanvas {
    diagram: Diagram,
    grid: bool,
    /// What is shown: the export's SVG, with the grid if it is on.
    tree: Option<Arc<usvg::Tree>>,
    /// Counts the images asked for, so that one rasterized meanwhile for an older state is
    /// dropped.
    generation: u64,
    /// The sheet's size at 100 %.
    size: (f32, f32),
    zoom: f32,
    /// Where the sheet's top left corner is in the drawing area.
    pan: Point<Pixels>,
    /// Until the user zooms or pans, the sheet is fitted into the area on each paint (at most
    /// at 100 %), so that it stays fitted while a new window gets its size.
    auto_fit: bool,
    raster: Option<Raster>,
    /// The raster shows an older state (another grid); it goes on the next paint.
    outdated: bool,
    rasterizing: bool,
    /// The drawing area, as painted last.
    area: Rc<Cell<Bounds<Pixels>>>,
    drag: Option<Pan>,
    /// Space is held: the left button pans.
    space: bool,
    focus: FocusHandle,
}

impl ErmCanvas {
    pub fn new(diagram: Diagram, cx: &mut Context<Self>) -> Self {
        let mut canvas = Self {
            diagram,
            grid: false,
            tree: None,
            generation: 0,
            size: (1.0, 1.0),
            zoom: 1.0,
            pan: point(px(MARGIN), px(MARGIN)),
            auto_fit: true,
            raster: None,
            outdated: false,
            rasterizing: false,
            area: Rc::default(),
            drag: None,
            space: false,
            focus: cx.focus_handle(),
        };
        canvas.draw();
        canvas
    }

    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn grid(&self) -> bool {
        self.grid
    }

    /// Makes the image to show from the diagram and the grid setting.
    fn draw(&mut self) {
        let mut svg = umlcraft_erm::svg::render(&self.diagram);
        if self.grid {
            svg = with_grid(&svg);
        }
        self.tree = OPTIONS.with(|options| usvg::Tree::from_str(&svg, options)).ok().map(Arc::new);
        if let Some(tree) = &self.tree {
            let size = tree.size();
            self.size = (size.width().max(1.0), size.height().max(1.0));
        }
        self.generation += 1;
        self.outdated = self.raster.is_some();
    }

    fn toggle_grid(&mut self, _: &ToggleGrid, _: &mut Window, cx: &mut Context<Self>) {
        self.grid = !self.grid;
        self.draw();
        cx.notify();
    }

    // ----- Raster ----------------------------------------------------------------------------

    /// The image to paint at `scale` (zoom × screen scale), its size and the scale it was made
    /// for. The first image is made right away; one for another zoom or state in the
    /// background, while the one there is stands in.
    fn raster(
        &mut self,
        scale: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Arc<RenderImage>, (u32, u32), f32)> {
        let scale = scale.min(MAX_RASTER / self.size.0.max(self.size.1));
        let fits = self.raster.as_ref().map(|r| !self.outdated && (r.scale - scale).abs() <= 1e-4);
        match fits {
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
                if this.generation == generation {
                    this.replace_raster(image, scale, window);
                    this.outdated = false;
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
        image: Option<(Arc<RenderImage>, (u32, u32))>,
        scale: f32,
        window: &mut Window,
    ) {
        // GPUI keeps every image it painted in the window's atlas until it is dropped there.
        if let Some(old) = self.raster.take() {
            window.drop_image(old.image).ok();
        }
        self.raster = image.map(|(image, size)| Raster { scale, image, size });
    }

    // ----- Zoom and panning ------------------------------------------------------------------

    fn set_zoom(&mut self, zoom: f32, anchor: Point<Pixels>, cx: &mut Context<Self>) {
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

    fn zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * ZOOM_STEP, self.centre(), cx);
    }

    fn zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom / ZOOM_STEP, self.centre(), cx);
    }

    fn reset_zoom(&mut self, _: &ResetZoom, _: &mut Window, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = point(px(MARGIN), px(MARGIN));
        self.auto_fit = false;
        cx.notify();
    }

    fn zoom_to_fit(&mut self, _: &ZoomToFit, _: &mut Window, cx: &mut Context<Self>) {
        self.fit(MAX_ZOOM);
        self.auto_fit = false;
        cx.notify();
    }

    /// Fits the sheet into the area, at most at `max`.
    fn fit(&mut self, max: f32) {
        let area = self.area.get().size;
        let (zoom, pan) = fit((f32::from(area.width), f32::from(area.height)), self.size, max);
        self.zoom = zoom;
        self.pan = point(px(pan.0), px(pan.1));
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    fn start_pan(&mut self, position: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.drag = Some(Pan { start: position, pan: self.pan });
        cx.notify();
    }

    fn on_left_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.space {
            self.start_pan(ev.position, window, cx);
        } else {
            window.focus(&self.focus, cx);
        }
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else { return };
        if ev.pressed_button.is_none() {
            self.drag = None;
            cx.notify();
            return;
        }
        self.pan = point(
            drag.pan.x + ev.position.x - drag.start.x,
            drag.pan.y + ev.position.y - drag.start.y,
        );
        self.auto_fit = false;
        cx.notify();
    }

    fn end_pan(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "space" && !self.space {
            self.space = true;
            cx.notify();
        }
    }

    fn on_key_up(&mut self, ev: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if ev.keystroke.key == "space" {
            self.space = false;
            cx.notify();
        }
    }
}

impl Render for ErmCanvas {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity();
        let area = self.area.clone();
        let cursor = match (&self.drag, self.space) {
            (Some(_), _) => CursorStyle::ClosedHand,
            (None, true) => CursorStyle::OpenHand,
            (None, false) => CursorStyle::Arrow,
        };
        div()
            .id("erm-canvas")
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().muted)
            .track_focus(&self.focus)
            .key_context("ErmCanvas")
            .cursor(cursor)
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::reset_zoom))
            .on_action(cx.listener(Self::zoom_to_fit))
            .on_action(cx.listener(Self::toggle_grid))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.start_pan(ev.position, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::end_pan))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::end_pan))
            .child(
                canvas(
                    move |bounds, _, _| area.set(bounds),
                    move |bounds, _, window, cx| {
                        let scale = window.scale_factor();
                        let (pan, zoom, raster) = this.update(cx, |this, cx| {
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
                            (this.pan, this.zoom, raster)
                        });
                        let Some((image, (w, h), raster_scale)) = raster else { return };
                        let origin = point(bounds.origin.x + pan.x, bounds.origin.y + pan.y);
                        // The image covers the sheet at the zoom, whatever its scale.
                        let shown = zoom / raster_scale;
                        let image_bounds =
                            Bounds::new(origin, size(px(w as f32 * shown), px(h as f32 * shown)));
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
                        });
                    },
                )
                .size_full(),
            )
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

/// The zoom that fits `sheet` with its margin into `area`, at most `max`, and the pan that
/// centres it.
fn fit(area: (f32, f32), sheet: (f32, f32), max: f32) -> (f32, (f32, f32)) {
    let fit_w = (area.0 - 2.0 * MARGIN) / sheet.0;
    let fit_h = (area.1 - 2.0 * MARGIN) / sheet.1;
    let zoom = fit_w.min(fit_h).min(max).clamp(MIN_ZOOM, MAX_ZOOM);
    (zoom, ((area.0 - sheet.0 * zoom) / 2.0, (area.1 - sheet.1 * zoom) / 2.0))
}

/// The SVG image with draw.io's grid between the white background and the drawing: lines
/// every 10 px from the diagram's origin, every fourth in full colour, on the pixel grid like
/// the shapes.
fn with_grid(svg: &str) -> String {
    // The image's first rectangle is its background (`svg::render` always has one).
    let Some(start) = svg.find("\n<rect ") else { return svg.to_owned() };
    let Some(end) = svg[start..].find("/>\n").map(|i| start + i + 3) else {
        return svg.to_owned();
    };
    let major = GRID_SIZE * f64::from(GRID_STEPS);
    let mut minor = String::new();
    for i in 1..GRID_STEPS {
        let at = GRID_SIZE * f64::from(i);
        minor.push_str(&format!("M 0 {at} L {major} {at} M {at} 0 L {at} {major} "));
    }
    let grid = format!(
        "<defs><pattern id=\"erm-grid\" patternUnits=\"userSpaceOnUse\" x=\"0.5\" y=\"0.5\" \
         width=\"{major}\" height=\"{major}\">\
         <path d=\"{minor}\" fill=\"none\" stroke=\"{GRID_COLOR}\" opacity=\"0.2\"/>\
         <path d=\"M {major} 0 L 0 0 0 {major}\" fill=\"none\" stroke=\"{GRID_COLOR}\"/>\
         </pattern></defs>\n<rect x=\"-100000\" y=\"-100000\" width=\"200000\" \
         height=\"200000\" fill=\"url(#erm-grid)\"/>\n",
        minor = minor.trim_end(),
    );
    format!("{}{grid}{}", &svg[..end], &svg[end..])
}

#[cfg(test)]
mod tests {
    use gpui_kit::{point, px};

    use super::{MARGIN, MAX_ZOOM, MIN_ZOOM, fit, with_grid, zoom_about};

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
    fn opening_fits_the_sheet_but_does_not_enlarge_it() {
        // Small diagram in a large window: 100 %, centred.
        let (zoom, pan) = fit((1000.0, 800.0), (400.0, 200.0), 1.0);
        assert_eq!(zoom, 1.0);
        assert_eq!(pan, (300.0, 300.0));
        // Fitting on demand enlarges it.
        let (zoom, _) = fit((1000.0, 800.0), (400.0, 200.0), MAX_ZOOM);
        assert_eq!(zoom, (1000.0 - 2.0 * MARGIN) / 400.0);
        // Large diagram: shrunk so that the margin stays on the tighter side.
        let (zoom, pan) = fit((1000.0, 800.0), (1920.0, 760.0), 1.0);
        assert_eq!(zoom, 0.5);
        assert_eq!(pan.0, MARGIN);
        // Never below the smallest zoom.
        assert_eq!(fit((100.0, 100.0), (10000.0, 10000.0), 1.0).0, MIN_ZOOM);
    }

    #[test]
    fn the_grid_lies_between_background_and_drawing() {
        let path =
            concat!(env!("CARGO_MANIFEST_DIR"), "/../umlcraft-erm/tests/fixtures/flug.ucerm");
        let (d, _) = umlcraft_erm::read(&std::fs::read_to_string(path).unwrap()).unwrap();
        let plain = umlcraft_erm::svg::render(&d);
        let svg = with_grid(&plain);
        let background = svg.find("fill=\"#ffffff\"").unwrap();
        let grid = svg.find("url(#erm-grid)").unwrap();
        let drawing = svg.find("<g ").unwrap();
        assert!(background < grid && grid < drawing);
        // Nothing else changes, and the image still parses.
        let tail = "url(#erm-grid)\"/>\n";
        let end = svg.find(tail).unwrap() + tail.len();
        assert_eq!(svg.replace(&svg[svg.find("<defs>").unwrap()..end], ""), plain);
        resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).unwrap();
    }
}
