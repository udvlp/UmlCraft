//! The window of one ER diagram: a title bar like the main window's, a toolbar (save, undo,
//! zoom, grid, export, properties), the palette, the canvas and the properties on cards, and
//! under the canvas the diagram's warnings (a line that opens into their list; a click selects
//! the element). Closing with unsaved changes asks first.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::DropdownMenu;
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable, StyledExt, TitleBar, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::Diagram;
use umlcraft_erm::edit::Item;

use super::canvas::ErmCanvas;
use super::props::PropsPanel;
use super::{
    Arrange, ExportErmPng, ExportErmPngTransparent, ExportErmSvg, PlaceShape, ToggleGrid,
    ToggleProperties, file_stem,
};
use crate::{
    ExportPng, ExportSvg, Quit, Redo, ResetZoom, SaveProject, SaveProjectAs, Undo, ZoomIn, ZoomOut,
    ZoomToFit, icons, shell,
};

/// Opens a window for `diagram`, read from `path` (`None`: a new diagram).
pub fn open(
    path: Option<PathBuf>,
    diagram: Diagram,
    cx: &mut App,
) -> Option<(AnyWindowHandle, WeakEntity<ErmWindow>)> {
    let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // As the main window.
        window_min_size: Some(size(px(800.0), px(520.0))),
        app_id: Some("com.umlcraft.app".into()),
        window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
        ..TitleBar::window_options()
    };
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        cx.new(|cx| ErmWindow::new(path, diagram, window, cx))
    });
    opened.ok().map(|(handle, view)| (handle, view.downgrade()))
}

/// What happens once a diagram was saved under a new name (closing the window).
type AfterSave = Box<dyn FnOnce(&mut Window)>;

/// What an export writes.
#[derive(Clone, Copy)]
enum Export {
    Svg,
    Png { transparent: bool },
}

pub struct ErmWindow {
    /// The file; `None` until a new diagram is saved.
    path: Option<PathBuf>,
    canvas: Entity<ErmCanvas>,
    props: Entity<PropsPanel>,
    show_props: bool,
    notes_open: bool,
    /// What the import said, while it is shown.
    import: Option<super::import::ImportNote>,
    import_open: bool,
    /// Where an imported diagram is saved by default: next to its dump.
    import_dir: Option<PathBuf>,
    /// The window title set last.
    title: String,
    /// Closing was confirmed (saved or discarded).
    close_confirmed: bool,
    _subscriptions: Vec<Subscription>,
}

impl ErmWindow {
    fn new(
        path: Option<PathBuf>,
        diagram: Diagram,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = path.is_some();
        let canvas = cx.new(|cx| ErmCanvas::new(diagram, saved, cx));
        let props = cx.new(|cx| PropsPanel::new(canvas.clone(), cx));
        // The toolbar shows zoom, grid, undo and the warnings.
        let subscriptions = vec![cx.observe(&canvas, |_, _, cx| cx.notify())];
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.should_close(window, cx)).unwrap_or(true)
        });
        Self {
            path,
            canvas,
            props,
            show_props: true,
            notes_open: false,
            import: None,
            import_open: false,
            import_dir: None,
            title: String::new(),
            close_confirmed: false,
            _subscriptions: subscriptions,
        }
    }

    /// Shows what an import found under the canvas; saving starts in `dir`.
    pub fn imported(
        &mut self,
        note: super::import::ImportNote,
        dir: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.import_open = !note.lines.is_empty();
        self.import = Some(note);
        self.import_dir = dir;
        cx.notify();
    }

    /// Whether closing would lose changes.
    pub fn unsaved(&self, cx: &App) -> bool {
        self.canvas.read(cx).unsaved()
    }

    /// The file's name, or the diagram's for a new one.
    pub fn title(&self) -> String {
        match &self.path {
            Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            None => t!("erm.new_title").to_string(),
        }
    }

    // ----- Saving and closing ----------------------------------------------------------------

    fn save(&mut self, _: &SaveProject, window: &mut Window, cx: &mut Context<Self>) {
        match self.path.clone() {
            Some(path) => {
                self.write(&path, window, cx);
            }
            None => self.save_as(&SaveProjectAs, window, cx),
        }
    }

    fn save_as(&mut self, _: &SaveProjectAs, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_save(window, cx, None);
    }

    /// Asks where to save, saves, then runs `then` (closing the window after "Save").
    fn prompt_save(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: Option<AfterSave>,
    ) {
        let directory = match (&self.path, &self.import_dir) {
            (None, Some(dir)) => dir.clone(),
            _ => crate::main_view::start_directory(self.path.as_deref()),
        };
        let diagram = self.canvas.read(cx).diagram().clone();
        let suggested = format!(
            "{}.{}",
            file_stem(&diagram, self.path.as_deref()),
            umlcraft_erm::FILE_EXTENSION
        );
        let rx = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = crate::main_view::with_extension(path, umlcraft_erm::FILE_EXTENSION);
            this.update_in(cx, |this, window, cx| {
                if this.write(&path, window, cx) {
                    this.path = Some(path.clone());
                    super::rekey(window.window_handle(), &path);
                    if let Some(then) = then {
                        then(window);
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes the diagram to `path`; whether it worked (a failure is shown).
    fn write(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        let diagram = self.canvas.read(cx).to_save(&crate::nsd::user_name(), &now);
        match crate::nsd::write_file(path, umlcraft_erm::write(&diagram).as_bytes()) {
            Ok(()) => {
                self.canvas.update(cx, |c, cx| c.saved(diagram, cx));
                true
            }
            Err(err) => {
                let rx = window.prompt(
                    PromptLevel::Critical,
                    &t!("erm.save_failed"),
                    Some(&err.to_string()),
                    &[t!("dialog.ok").as_ref()],
                    cx,
                );
                cx.spawn(async move |_, _| rx.await.ok()).detach();
                false
            }
        }
    }

    /// Asks before closing with unsaved changes: save, discard or cancel.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.canvas.update(cx, |c, cx| c.commit_edit(window, cx));
        if self.close_confirmed || !self.unsaved(cx) {
            return true;
        }
        let answers = [
            t!("dialog.save").to_string(),
            t!("dialog.discard").to_string(),
            t!("dialog.cancel").to_string(),
        ];
        let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
        let message = t!("erm.unsaved", name = self.title()).to_string();
        let rx = window.prompt(
            PromptLevel::Warning,
            &message,
            Some(&t!("dialog.unsaved_detail")),
            &answers,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            this.update_in(cx, move |this, window, cx| match answer {
                0 => match this.path.clone() {
                    Some(path) => {
                        if this.write(&path, window, cx) {
                            this.close_confirmed = true;
                            window.remove_window();
                        }
                    }
                    None => this.prompt_save(
                        window,
                        cx,
                        Some(Box::new(|window: &mut Window| window.remove_window())),
                    ),
                },
                1 => {
                    this.close_confirmed = true;
                    window.remove_window();
                }
                _ => {}
            })
            .ok();
        })
        .detach();
        false
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
        let directory = match (&self.path, &self.import_dir) {
            (None, Some(dir)) => dir.clone(),
            _ => crate::main_view::start_directory(self.path.as_deref()),
        };
        let suggested = format!("{}.{extension}", file_stem(&diagram, self.path.as_deref()));
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

    // ----- Rendering -------------------------------------------------------------------------

    /// Logo and the file's name (• when changed); the window's controls are the title bar's.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let divider = div().w(px(1.0)).h(px(16.0)).mx_1().bg(cx.theme().border);
        let muted = cx.theme().muted_foreground;
        TitleBar::new().bg(transparent_black()).border_b_0().child(
            h_flex()
                .flex_1()
                .gap_1()
                .child(img(icons::LOGO).size(px(18.0)).mr_1())
                .child(div().text_sm().font_semibold().mr_2().child("UmlCraft"))
                .child(divider)
                .child(div().text_sm().ml_1().child(self.title()))
                .when(self.unsaved(cx), |this| {
                    this.child(div().text_sm().text_color(muted).child("•"))
                })
                .child(div().text_sm().text_color(muted).child(t!("erm.window_kind").to_string())),
        )
    }

    /// Save, undo, zoom, grid, export and the properties.
    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let canvas = self.canvas.read(cx);
        let zoom = (canvas.zoom() * 100.0).round();
        let grid = canvas.grid();
        let (can_undo, can_redo) = (canvas.can_undo(), canvas.can_redo());
        // Menu commands go where the toolbar's do: to the canvas, then up to this window.
        let focus = canvas.focus_handle(cx);
        let divider = || div().w(px(1.0)).h(px(16.0)).mx_1().bg(cx.theme().border);
        let icon_button = |id: &'static str, icon: &str, tip: String, action: Box<dyn Action>| {
            shell::icon_button(id, icons::fluent(icon), tip, action)
        };
        h_flex()
            .gap_0p5()
            .px(shell::GAP + px(2.0))
            .pb(shell::GAP / 2.0)
            .child(icon_button(
                "erm-save",
                "save",
                t!("erm.save_tip").to_string(),
                Box::new(SaveProject),
            ))
            .child(
                icon_button(
                    "erm-undo",
                    "arrow_undo",
                    t!("toolbar.undo").to_string(),
                    Box::new(Undo),
                )
                .disabled(!can_undo),
            )
            .child(
                icon_button(
                    "erm-redo",
                    "arrow_redo",
                    t!("toolbar.redo").to_string(),
                    Box::new(Redo),
                )
                .disabled(!can_redo),
            )
            .child(divider())
            .child(icon_button(
                "zoom-out",
                "zoom_out",
                t!("menu.zoom_out").to_string(),
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
            .child(icon_button(
                "zoom-in",
                "zoom_in",
                t!("menu.zoom_in").to_string(),
                Box::new(ZoomIn),
            ))
            .child(icon_button(
                "zoom-fit",
                "zoom_fit",
                t!("erm.zoom_fit_tip").to_string(),
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
            .child(
                Button::new("erm-arrange")
                    .ghost()
                    .small()
                    .label(t!("erm.arrange"))
                    .tooltip(t!("erm.arrange_tip"))
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Arrange), cx)),
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
            .child(div().flex_1())
            .child(
                Button::new("erm-props-toggle")
                    .ghost()
                    .small()
                    .label(t!("erm.properties"))
                    .selected(self.show_props)
                    .toggled(self.show_props)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ToggleProperties), cx)
                    }),
            )
    }

    /// Under the canvas: a refused edit, the hint of "relationship to …", and the warnings (a
    /// line that opens into their list; a click selects the element).
    fn notes_bar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let canvas = self.canvas.read(cx);
        let d = canvas.diagram();
        let notes: Vec<(Option<umlcraft_erm::Id>, String)> = canvas
            .issues
            .iter()
            .map(|i| (Some(i.element.clone()), super::issue_text(Some(d), i)))
            .collect();
        let refused = canvas.refused.clone();
        let relating = canvas.relating.is_some();
        if notes.is_empty() && refused.is_none() && !relating && self.import.is_none() {
            return None;
        }
        let summary = t!("erm.notes", count = notes.len()).to_string();
        let arrow = |open: bool| if open { "▾" } else { "▸" };
        let list =
            (self.notes_open && !notes.is_empty()).then(|| self.note_list("erm-notes", &notes));
        let import = self.import.as_ref().map(|note| {
            let toggle = div()
                .id("erm-import-toggle")
                .flex_1()
                .when(!note.lines.is_empty(), |t| t.cursor_pointer())
                .child(if note.lines.is_empty() {
                    note.summary.clone()
                } else {
                    format!("{} {}", arrow(self.import_open), note.summary)
                })
                .on_click(cx.listener(|this, _, _, cx| {
                    this.import_open = !this.import_open;
                    cx.notify();
                }));
            let close = div()
                .id("erm-import-close")
                .px_1()
                .cursor_pointer()
                .text_color(cx.theme().muted_foreground)
                .child("×")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.import = None;
                    cx.notify();
                }));
            v_flex()
                .child(h_flex().gap_2().child(toggle).child(close))
                .when(self.import_open && !note.lines.is_empty(), |c| {
                    c.child(self.note_list("erm-import-notes", &note.lines))
                })
        });
        Some(
            v_flex()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_sm()
                .children(import)
                .children(refused.map(|r| div().text_color(cx.theme().danger).child(r)))
                .when(relating, |c| c.child(div().child(t!("erm.relationship_hint").to_string())))
                .when(!notes.is_empty(), |c| {
                    c.child(
                        div()
                            .id("erm-notes-toggle")
                            .cursor_pointer()
                            .text_color(cx.theme().warning)
                            .child(format!("{} {summary}", arrow(self.notes_open)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.notes_open = !this.notes_open;
                                cx.notify();
                            })),
                    )
                })
                .children(list),
        )
    }

    /// Lines under the canvas; one about an element selects it when clicked.
    fn note_list(
        &self,
        id: &'static str,
        lines: &[(Option<umlcraft_erm::Id>, String)],
    ) -> Stateful<Div> {
        let canvas = self.canvas.clone();
        v_flex().id(id).max_h(px(140.0)).overflow_y_scroll().pl_4().children(
            lines.iter().enumerate().map(|(i, (element, text))| {
                let canvas = canvas.clone();
                let line = div().id((id, i)).text_xs().child(text.clone());
                let Some(element) = element.clone() else { return line };
                line.cursor_pointer().hover(|s| s.underline()).on_click(move |_, _, cx| {
                    let element = element.clone();
                    canvas.update(cx, |c, cx| {
                        let d = c.diagram();
                        let item = if d.node(&element).is_some() {
                            Item::Node(element)
                        } else if d.link(&element).is_some() {
                            Item::Link(element)
                        } else {
                            Item::Text(element)
                        };
                        c.select(vec![item], cx);
                    });
                })
            }),
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
        let title =
            format!("{}{} – UmlCraft", self.title(), if self.unsaved(cx) { " •" } else { "" });
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        let corner = shell::RADIUS - px(1.0);
        let palette = super::palette::palette(cx);
        v_flex()
            .id("erm-window")
            .key_context("ErmWindow")
            .size_full()
            .bg(shell::backdrop(cx))
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(|this, _: &ExportErmSvg, w, cx| this.export(Export::Svg, w, cx)))
            .on_action(cx.listener(|this, _: &ExportErmPng, w, cx| {
                this.export(Export::Png { transparent: false }, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ExportErmPngTransparent, w, cx| {
                this.export(Export::Png { transparent: true }, w, cx)
            }))
            // The toolbar's and the palette's commands reach the canvas also when a field of
            // the properties has the focus.
            .on_action(
                cx.listener(|this, a: &Undo, w, cx| {
                    this.canvas.update(cx, |c, cx| c.undo(a, w, cx))
                }),
            )
            .on_action(
                cx.listener(|this, a: &Redo, w, cx| {
                    this.canvas.update(cx, |c, cx| c.redo(a, w, cx))
                }),
            )
            .on_action(cx.listener(|this, a: &ZoomIn, w, cx| {
                this.canvas.update(cx, |c, cx| c.zoom_in(a, w, cx))
            }))
            .on_action(cx.listener(|this, a: &ZoomOut, w, cx| {
                this.canvas.update(cx, |c, cx| c.zoom_out(a, w, cx))
            }))
            .on_action(cx.listener(|this, a: &ResetZoom, w, cx| {
                this.canvas.update(cx, |c, cx| c.reset_zoom(a, w, cx))
            }))
            .on_action(cx.listener(|this, a: &ZoomToFit, w, cx| {
                this.canvas.update(cx, |c, cx| c.zoom_to_fit(a, w, cx))
            }))
            .on_action(cx.listener(|this, a: &ToggleGrid, w, cx| {
                this.canvas.update(cx, |c, cx| c.toggle_grid(a, w, cx))
            }))
            .on_action(cx.listener(|this, _: &Arrange, w, cx| {
                this.canvas.update(cx, |c, cx| c.arrange(w, cx))
            }))
            .on_action(cx.listener(|this, a: &PlaceShape, w, cx| {
                this.canvas.update(cx, |c, cx| c.place_shape(a.shape, w, cx))
            }))
            // The platform's menu bar (macOS) is the main window's; its commands that make
            // sense here act on this window or the app.
            .on_action(
                cx.listener(|this, _: &ExportSvg, window, cx| this.export(Export::Svg, window, cx)),
            )
            .on_action(cx.listener(|this, _: &ExportPng, window, cx| {
                this.export(Export::Png { transparent: false }, window, cx)
            }))
            .on_action(|_: &super::OpenErm, window, cx| super::prompt_open(window, cx))
            .on_action(|_: &Quit, _, cx| crate::main_view::quit_from_other_window(cx))
            .on_action(cx.listener(|this, _: &ToggleProperties, _, cx| {
                this.show_props = !this.show_props;
                cx.notify();
            }))
            .child(self.title_bar(cx))
            .child(self.toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap(shell::GAP)
                    .px(shell::GAP)
                    .pb(shell::GAP)
                    .child(shell::card(cx).w(px(132.0)).h_full().overflow_hidden().child(palette))
                    .child(
                        shell::card(cx).flex_1().h_full().min_w_0().overflow_hidden().child(
                            v_flex()
                                .size_full()
                                .rounded(corner)
                                .overflow_hidden()
                                .child(div().flex_1().min_h_0().child(self.canvas.clone()))
                                .children(self.notes_bar(cx)),
                        ),
                    )
                    .when(self.show_props, |row| {
                        row.child(
                            shell::card(cx)
                                .w(px(260.0))
                                .h_full()
                                .overflow_hidden()
                                .child(self.props.clone()),
                        )
                    }),
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
        let attribute = |name: &str| -> f32 {
            let pattern = format!(" {name}=\"");
            svg.split(&pattern).nth(1).unwrap().split('"').next().unwrap().parse().unwrap()
        };
        let (width, height) = (attribute("width"), attribute("height"));
        let (w, h, alpha) = png_facts(&image(&d, Export::Png { transparent: false }).unwrap());
        assert!(w.abs_diff((width * 2.0) as u32) <= 2, "{w} for {width}");
        assert!(h.abs_diff((height * 2.0) as u32) <= 2, "{h} for {height}");
        assert_eq!(alpha, 255, "white background");
        let (tw, th, alpha) = png_facts(&image(&d, Export::Png { transparent: true }).unwrap());
        assert_eq!((tw, th), (w, h));
        assert_eq!(alpha, 0, "transparent background");
    }
}
