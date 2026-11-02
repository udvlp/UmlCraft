//! The window of one ER diagram: a title bar like the main window's, a toolbar (zoom, grid,
//! export), the canvas on a card and, if the file broke rules that only warn, a line that
//! opens into their list. It shows the diagram only; nothing in it changes the file.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, StyledExt, TitleBar, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::Diagram;

use super::canvas::ErmCanvas;
use super::{ExportErmPng, ExportErmPngTransparent, ExportErmSvg, ToggleGrid, file_stem};
use crate::{ResetZoom, ZoomIn, ZoomOut, ZoomToFit, icons, shell};

/// Opens a window for `diagram`, read from `path`; `notes` are the warnings found in it.
pub fn open(
    path: PathBuf,
    diagram: Diagram,
    notes: Vec<String>,
    cx: &mut App,
) -> Option<AnyWindowHandle> {
    let bounds = Bounds::centered(None, size(px(1100.0), px(760.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // As the main window.
        window_min_size: Some(size(px(800.0), px(520.0))),
        app_id: Some("com.umlcraft.app".into()),
        window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        window.set_window_title(&format!("{name} – UmlCraft"));
        cx.new(|cx| ErmWindow::new(path, diagram, notes, cx))
    });
    opened.ok().map(|(handle, _)| handle)
}

/// What an export writes.
#[derive(Clone, Copy)]
enum Export {
    Svg,
    Png { transparent: bool },
}

pub struct ErmWindow {
    path: PathBuf,
    canvas: Entity<ErmCanvas>,
    /// The warnings of the file, and whether their list is open.
    notes: Vec<String>,
    notes_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl ErmWindow {
    fn new(path: PathBuf, diagram: Diagram, notes: Vec<String>, cx: &mut Context<Self>) -> Self {
        let canvas = cx.new(|cx| ErmCanvas::new(diagram, cx));
        // The toolbar shows the zoom and the grid.
        let subscriptions = vec![cx.observe(&canvas, |_, _, cx| cx.notify())];
        Self { path, canvas, notes, notes_open: false, _subscriptions: subscriptions }
    }

    // ----- Export ----------------------------------------------------------------------------

    /// Asks where to write the image (next to the diagram's file), then writes it in the
    /// background; a failure is reported in a dialog.
    fn export(&mut self, export: Export, window: &mut Window, cx: &mut Context<Self>) {
        let diagram = self.canvas.read(cx).diagram().clone();
        let extension = match export {
            Export::Svg => "svg",
            Export::Png { .. } => "png",
        };
        let directory = crate::main_view::start_directory(Some(&self.path));
        let suggested = format!("{}.{extension}", file_stem(&diagram, &self.path));
        let rx = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = crate::main_view::with_extension(path, extension);
            let written = cx
                .background_executor()
                .spawn(async move {
                    let bytes = image(&diagram, export)?;
                    anyhow::Ok(crate::nsd::write_file(&path, &bytes)?)
                })
                .await;
            if let Err(err) = written {
                cx.update(|window, cx| {
                    let rx = window.prompt(
                        PromptLevel::Critical,
                        &t!("dialog.image_failed"),
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

    fn export_svg(&mut self, _: &ExportErmSvg, window: &mut Window, cx: &mut Context<Self>) {
        self.export(Export::Svg, window, cx);
    }

    fn export_png(&mut self, _: &ExportErmPng, window: &mut Window, cx: &mut Context<Self>) {
        self.export(Export::Png { transparent: false }, window, cx);
    }

    fn export_png_transparent(
        &mut self,
        _: &ExportErmPngTransparent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.export(Export::Png { transparent: true }, window, cx);
    }

    // ----- Rendering -------------------------------------------------------------------------

    /// Logo and the file's name; the window's controls on the right are the title bar's own.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let name =
            self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let divider = div().w(px(1.0)).h(px(16.0)).mx_1().bg(cx.theme().border);
        TitleBar::new().bg(transparent_black()).border_b_0().child(
            h_flex()
                .flex_1()
                .gap_1()
                .child(img(icons::LOGO).size(px(18.0)).mr_1())
                .child(div().text_sm().font_semibold().mr_2().child("UmlCraft"))
                .child(divider)
                .child(div().text_sm().ml_1().child(name))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("erm.window_kind").to_string()),
                ),
        )
    }

    /// Zoom, grid and export.
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = self.canvas.read(cx);
        let zoom = (canvas.zoom() * 100.0).round();
        let grid = canvas.grid();
        // Menu commands go where the toolbar's do: to the canvas, then up to this window.
        let focus = canvas.focus_handle(cx);
        let divider = || div().w(px(1.0)).h(px(16.0)).mx_1().bg(cx.theme().border);
        h_flex()
            .gap_0p5()
            .px(shell::GAP + px(2.0))
            .pb(shell::GAP / 2.0)
            .child(shell::icon_button(
                "zoom-out",
                icons::fluent("zoom_out"),
                t!("menu.zoom_out"),
                Box::new(ZoomOut),
            ))
            .child(
                Button::new("zoom-reset")
                    .ghost()
                    .small()
                    .label(format!("{zoom} %"))
                    .tooltip(t!("toolbar.zoom_reset"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ResetZoom), cx)),
            )
            .child(shell::icon_button(
                "zoom-in",
                icons::fluent("zoom_in"),
                t!("menu.zoom_in"),
                Box::new(ZoomIn),
            ))
            .child(shell::icon_button(
                "zoom-fit",
                icons::fluent("zoom_fit"),
                t!("erm.zoom_fit_tip"),
                Box::new(ZoomToFit),
            ))
            .child(divider())
            .child(
                Button::new("grid")
                    .ghost()
                    .small()
                    .label(t!("erm.grid"))
                    .tooltip(t!("erm.grid_tip"))
                    .selected(grid)
                    .toggled(grid)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ToggleGrid), cx)),
            )
            .child(divider())
            .child(
                Button::new("export-svg")
                    .ghost()
                    .small()
                    .icon(shell::icon(icons::fluent("image_arrow_forward")))
                    .label(t!("erm.export_svg"))
                    .tooltip(t!("erm.export_svg_tip"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(ExportErmSvg), cx)),
            )
            .child(
                Button::new("export-png")
                    .ghost()
                    .small()
                    .icon(shell::icon(icons::fluent("image_arrow_forward")))
                    .label(t!("erm.export_png"))
                    .dropdown_caret(true)
                    .dropdown_menu(move |menu, _, _| {
                        menu.menu(t!("erm.export_png_white"), Box::new(ExportErmPng))
                            .menu(
                                t!("erm.export_png_transparent"),
                                Box::new(ExportErmPngTransparent),
                            )
                            .action_context(focus.clone())
                    }),
            )
    }

    /// The warnings of the file: a line that opens into the list.
    fn notes_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if self.notes.is_empty() {
            return None;
        }
        let summary = t!("erm.notes", count = self.notes.len()).to_string();
        let arrow = if self.notes_open { "▾" } else { "▸" };
        let list = self.notes_open.then(|| {
            v_flex()
                .id("erm-notes")
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
                        .id("erm-notes-toggle")
                        .cursor_pointer()
                        .text_color(cx.theme().warning)
                        .child(format!("{arrow} {summary}"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notes_open = !this.notes_open;
                            cx.notify();
                        })),
                )
                .children(list),
        )
    }
}

/// The bytes of an export.
fn image(diagram: &Diagram, export: Export) -> anyhow::Result<Vec<u8>> {
    Ok(match export {
        Export::Svg => umlcraft_erm::svg::render(diagram).into_bytes(),
        Export::Png { transparent } => {
            let options = umlcraft_erm::svg::Options {
                background: if transparent {
                    None
                } else {
                    umlcraft_erm::svg::Options::default().background
                },
                ..Default::default()
            };
            let svg = umlcraft_erm::svg::render_with(diagram, &options);
            // `from_svg` takes images made at Astah's 4/3 pixels per unit; ours have one.
            crate::png::from_svg(&svg, crate::png::SCALE * umlcraft_notation::svg::SCALE)?
        }
    })
}

impl Render for ErmWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Keys and the toolbar's commands start at the focused element.
        if window.focused(cx).is_none() {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        let corner = shell::RADIUS - px(1.0);
        v_flex()
            .id("erm-window")
            .key_context("ErmWindow")
            .size_full()
            .bg(shell::backdrop(cx))
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::export_svg))
            .on_action(cx.listener(Self::export_png))
            .on_action(cx.listener(Self::export_png_transparent))
            .child(self.title_bar(cx))
            .child(self.toolbar(cx))
            .child(
                div().flex_1().min_h_0().px(shell::GAP).pb(shell::GAP).child(
                    shell::card(cx).size_full().overflow_hidden().child(
                        v_flex()
                            .size_full()
                            .rounded(corner)
                            .overflow_hidden()
                            .child(div().flex_1().min_h_0().child(self.canvas.clone()))
                            .children(self.notes_bar(cx)),
                    ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Export, image};

    fn flug() -> umlcraft_erm::Diagram {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../umlcraft-erm/tests/fixtures/flug.ucerm");
        umlcraft_erm::read(&std::fs::read_to_string(path).unwrap()).unwrap().0
    }

    /// Width, height and the first pixel's alpha of a PNG.
    fn png_facts(png: &[u8]) -> (u32, u32, u8) {
        let decoded = image::load_from_memory(png).unwrap().to_rgba8();
        (decoded.width(), decoded.height(), decoded.get_pixel(0, 0).0[3])
    }

    #[test]
    fn exports_are_the_svg_and_its_image_at_twice_the_size() {
        let d = flug();
        let svg = String::from_utf8(image(&d, Export::Svg).unwrap()).unwrap();
        assert_eq!(svg, umlcraft_erm::svg::render(&d));
        let width: f32 =
            svg.split("width=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        let height: f32 =
            svg.split("height=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        let (w, h, alpha) = png_facts(&image(&d, Export::Png { transparent: false }).unwrap());
        assert!(w.abs_diff((width * 2.0) as u32) <= 2, "{w} for {width}");
        assert!(h.abs_diff((height * 2.0) as u32) <= 2, "{h} for {height}");
        assert_eq!(alpha, 255, "white background");
        let (tw, th, alpha) = png_facts(&image(&d, Export::Png { transparent: true }).unwrap());
        assert_eq!((tw, th), (w, h));
        assert_eq!(alpha, 0, "transparent background");
    }
}
