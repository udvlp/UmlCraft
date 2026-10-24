//! Main window: menu bar, toolbar, model tree, diagram tabs with palette and canvas,
//! properties panel and status bar. Also owns file handling (new, open, save, recent files).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{AppMenuBar, PopupMenu};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{
    ActiveTheme, Disableable, GlobalState, Selectable, Sizable, StyledExt, Theme, ThemeMode,
    h_flex, h_resizable, resizable_panel, v_flex,
};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_format::FILE_EXTENSION;
use umlcraft_model::sample::order_system;
use umlcraft_model::{DiagramKind, Document, ElementData, ElementKind, Id, Model};
use umlcraft_notation::clipboard::Clip;
use umlcraft_notation::palette::{self, NodeTool, Tool};
use umlcraft_notation::scene::{self, Alignment, Preview};

use crate::canvas::{DiagramCanvas, DraggedElement, FontMeasure};
use crate::labels;
use crate::properties::PropertiesPanel;
use crate::settings::Settings;
use crate::workspace::{NewItem, Workspace};
use crate::*;

/// Something to do once the user has dealt with unsaved changes.
type Continuation = Box<dyn FnOnce(&mut MainView, &mut Window, &mut Context<MainView>)>;

/// What the model tree needs to know about an item to build its context menu.
struct TreeMeta {
    id: Id,
    kind: ElementKind,
    is_root: bool,
}

pub struct MainView {
    workspace: Entity<Workspace>,
    canvas: Entity<DiagramCanvas>,
    properties: Entity<PropertiesPanel>,
    tree: Entity<TreeState>,
    menu_bar: Entity<AppMenuBar>,
    settings: Settings,
    /// Copied content and the diagram it came from.
    clipboard: Option<(Id, Clip)>,
    paste_count: u32,
    tree_meta: Rc<HashMap<SharedString, TreeMeta>>,
    collapsed: HashSet<SharedString>,
    tree_revision: Option<(u64, String)>,
    tree_selection: Option<SharedString>,
    /// Set once the user agreed to close despite unsaved changes.
    close_confirmed: bool,
    /// An export to Astah is running in the background.
    exporting: bool,
    _subscriptions: Vec<Subscription>,
}

impl MainView {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        if settings.dark == Some(true) {
            Theme::change(ThemeMode::Dark, Some(window), cx);
        }
        let workspace = cx.new(|_| Workspace::new(blank_document(), None));
        let canvas = cx.new(|cx| DiagramCanvas::new(workspace.clone(), cx));
        let properties = cx.new(|cx| PropertiesPanel::new(workspace.clone(), window, cx));
        let tree = cx.new(|cx| TreeState::new(cx));
        install_menus(&settings, cx);
        let menu_bar = AppMenuBar::new(cx);

        let subscriptions = vec![
            cx.subscribe(&tree, |this, _, event: &TreeEvent, _| match event {
                TreeEvent::Expanded(id) => {
                    this.collapsed.remove(id);
                }
                TreeEvent::Collapsed(id) => {
                    this.collapsed.insert(id.clone());
                }
            }),
            cx.observe(&tree, |this, tree, cx| this.on_tree_changed(tree, cx)),
            // Toolbar and status bar show the zoom.
            cx.observe(&canvas, |_, _, cx| cx.notify()),
            cx.observe_in(&workspace, window, |this, _, window, cx| {
                this.sync_from_workspace(window, cx);
            }),
        ];

        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.should_close(window, cx)).unwrap_or(true)
        });

        let mut view = Self {
            workspace,
            canvas,
            properties,
            tree,
            menu_bar,
            settings,
            tree_meta: Rc::default(),
            collapsed: HashSet::new(),
            tree_revision: None,
            tree_selection: None,
            close_confirmed: false,
            exporting: false,
            clipboard: None,
            paste_count: 0,
            _subscriptions: subscriptions,
        };
        view.sync_from_workspace(window, cx);
        view
    }

    // ----- Workspace ↔ tree and window title -------------------------------------------------

    fn sync_from_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, _| ws.validate());
        let ws = self.workspace.read(cx);
        let revision = (ws.doc.revision(), rust_i18n::locale().to_string());
        if self.tree_revision.as_ref() != Some(&revision) {
            let model = ws.doc.model();
            let mut meta = HashMap::new();
            let root = tree_item(model, model.root(), &self.collapsed, &mut meta);
            self.tree_meta = Rc::new(meta);
            self.tree_selection = None;
            self.tree.update(cx, |tree, cx| tree.set_items(vec![root], cx));
            self.tree_revision = Some(revision);
        }

        let ws = self.workspace.read(cx);
        let wanted =
            ws.selected_element.or(ws.active_diagram).map(|id| SharedString::from(id.to_string()));
        if wanted != self.tree_selection {
            self.tree_selection = wanted.clone();
            self.tree.update(cx, |tree, cx| {
                let item = wanted.map(|id| TreeItem::new(id, ""));
                tree.set_selected_item(item.as_ref(), cx);
            });
        }

        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let name = ws.file_name().unwrap_or_else(|| {
            model.element(model.root()).map(|e| e.name().to_owned()).unwrap_or_default()
        });
        let dirty = if ws.doc.is_dirty() { "*" } else { "" };
        window.set_window_title(&format!("{name}{dirty} – UmlCraft"));
        cx.notify();
    }

    fn on_tree_changed(&mut self, tree: Entity<TreeState>, cx: &mut Context<Self>) {
        let selected = tree.read(cx).selected_item().map(|item| item.id.clone());
        if selected == self.tree_selection {
            return;
        }
        self.tree_selection = selected.clone();
        let Some(id) = selected.and_then(|s| s.parse::<Id>().ok()) else { return };
        self.workspace.update(cx, |ws, cx| {
            if ws.doc.model().view(id).is_some() {
                ws.open_diagram(id);
                ws.clear_selection();
            } else {
                ws.select_element(id);
            }
            cx.notify();
        });
    }

    fn tree_command(&mut self, cmd: &TreeCommand, window: &mut Window, cx: &mut Context<Self>) {
        let target = cmd.target;
        match cmd.command {
            TreeCommandKind::New(item) => self.workspace.update(cx, |ws, cx| {
                ws.add_item(target, item);
                cx.notify();
            }),
            TreeCommandKind::Open => self.workspace.update(cx, |ws, cx| {
                ws.open_diagram(target);
                cx.notify();
            }),
            TreeCommandKind::Rename => {
                self.workspace.update(cx, |ws, cx| {
                    ws.select_element(target);
                    cx.notify();
                });
                self.properties.update(cx, |p, cx| p.focus_name(window, cx));
            }
            TreeCommandKind::Delete => self.workspace.update(cx, |ws, cx| {
                ws.delete_element(target);
                cx.notify();
            }),
        }
    }

    // ----- Files ---------------------------------------------------------------------------

    /// Runs `then` right away, or after asking what to do with unsaved changes.
    fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>, then: Continuation) {
        if !self.workspace.read(cx).doc.is_dirty() {
            then(self, window, cx);
            return;
        }
        let message = t!("dialog.unsaved").to_string();
        let detail = t!("dialog.unsaved_detail").to_string();
        let answers = [
            t!("dialog.save").to_string(),
            t!("dialog.discard").to_string(),
            t!("dialog.cancel").to_string(),
        ];
        let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
        let rx = window.prompt(PromptLevel::Warning, &message, Some(&detail), &answers, cx);
        cx.spawn_in(window, async move |this, cx| {
            let Ok(answer) = rx.await else { return };
            this.update_in(cx, move |this, window, cx| match answer {
                0 => this.save(false, window, cx, Some(then)),
                1 => then(this, window, cx),
                _ => {}
            })
            .ok();
        })
        .detach();
    }

    fn replace_document(&mut self, doc: Document, path: Option<PathBuf>, cx: &mut Context<Self>) {
        self.workspace.update(cx, |ws, cx| {
            *ws = Workspace::new(doc, path);
            cx.notify();
        });
        self.collapsed.clear();
        self.tree_revision = None;
    }

    fn new_project(&mut self, _: &NewProject, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(
            window,
            cx,
            Box::new(|this, _, cx| this.replace_document(blank_document(), None, cx)),
        );
    }

    fn open_sample(&mut self, _: &OpenSample, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(
            window,
            cx,
            Box::new(|this, _, cx| this.replace_document(order_system().0, None, cx)),
        );
    }

    fn open_project(&mut self, _: &OpenProject, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(window, cx);
    }

    /// Opening an `.asta` file imports it, so this is the same as "Open".
    fn import_astah(&mut self, _: &ImportAstah, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_and_open(window, cx);
    }

    fn prompt_and_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(
            window,
            cx,
            Box::new(|_, window, cx| {
                let rx = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: None,
                });
                cx.spawn_in(window, async move |this, cx| {
                    let Ok(Ok(Some(paths))) = rx.await else { return };
                    let Some(path) = paths.into_iter().next() else { return };
                    this.update_in(cx, |this, window, cx| this.open(path, window, cx)).ok();
                })
                .detach();
            }),
        );
    }

    fn open_recent(&mut self, action: &OpenRecent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.settings.recent.get(action.index).cloned() else { return };
        self.confirm_discard(
            window,
            cx,
            Box::new(move |this, window, cx| this.open(path, window, cx)),
        );
    }

    /// Opens a project, or imports an `.asta` file.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asta")) {
            self.import(&path, window, cx);
            return;
        }
        match umlcraft_format::load(&path) {
            Ok(model) => {
                let mut doc = Document::new(model);
                umlcraft_notation::sequence_edit::upgrade_document(&mut doc);
                self.replace_document(doc, Some(path.clone()), cx);
                self.remember(&path, cx);
            }
            Err(err) => self.show_error(&t!("dialog.open_failed"), &err.to_string(), window, cx),
        }
    }

    /// Imports an Astah project as a new, not yet saved project. Says so when parts of it
    /// could not be carried over.
    fn import(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        match umlcraft_asta::import(path) {
            Ok(imported) => {
                self.replace_document(Document::new(imported.model), None, cx);
                let count: usize = imported.skipped.values().sum();
                if count > 0 {
                    let kinds: Vec<String> = imported
                        .skipped
                        .iter()
                        .map(|(class, n)| format!("{} ({n})", astah_class_label(class)))
                        .collect();
                    let message = t!("dialog.import_partial", count = count);
                    let detail = t!("dialog.import_partial_detail", kinds = kinds.join(", "));
                    self.inform(PromptLevel::Info, &message, &detail, window, cx);
                }
            }
            Err(err) => self.show_error(&t!("dialog.import_failed"), &err.to_string(), window, cx),
        }
    }

    /// Asks for a file, then has the installed Astah build it in the background. Exporting
    /// needs Astah, whose API writes the file; without it, says how to provide one.
    fn export_astah(&mut self, _: &ExportAstah, window: &mut Window, cx: &mut Context<Self>) {
        let Some(astah) = astah() else {
            let (message, detail) = (t!("dialog.astah_missing"), t!("dialog.astah_missing_detail"));
            self.inform(PromptLevel::Info, &message, &detail, window, cx);
            return;
        };
        if self.exporting {
            return;
        }
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let project = model.element(model.root()).map(|e| e.name().to_owned()).unwrap_or_default();
        let suggested = ws
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| sanitize_file_name(&project));
        let directory = start_directory(ws.path.as_deref());
        let export = umlcraft_asta_export::Export::new(model);
        let rx = cx.prompt_for_new_path(&directory, Some(&format!("{suggested}.asta")));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = with_extension(path, "asta");
            let started = this.update(cx, |this, cx| {
                this.exporting = true;
                cx.notify();
            });
            if started.is_err() {
                return;
            }
            let target = path.clone();
            let result =
                cx.background_executor().spawn(async move { export.run(astah, &target) }).await;
            this.update_in(cx, |this, window, cx| {
                this.exporting = false;
                cx.notify();
                this.report_export(result, &path, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Writes the open diagram as an SVG image, drawn as on the canvas in the light theme.
    fn export_svg(&mut self, _: &ExportSvg, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(false, window, cx);
    }

    fn export_png(&mut self, _: &ExportPng, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(true, window, cx);
    }

    /// Writes the open diagram as an SVG image, or rasterized as PNG, drawn as on the canvas
    /// in the light theme.
    fn export_image(&mut self, png: bool, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.read(cx);
        let Some(diagram) = ws.active_diagram else { return };
        let model = ws.doc.model();
        let measure = FontMeasure::new(window);
        let scene = scene::build(model, diagram, &Preview::default(), &measure);
        let svg = umlcraft_notation::svg::render(&scene, &measure);
        let extension = if png { "png" } else { "svg" };
        let name = model.element(diagram).map(|e| e.name().to_owned()).unwrap_or_default();
        let suggested = format!("{}.{extension}", sanitize_file_name(&name));
        let rx = cx.prompt_for_new_path(&start_directory(ws.path.as_deref()), Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = with_extension(path, extension);
            let written = cx
                .background_executor()
                .spawn(async move {
                    let bytes = if png {
                        crate::png::from_svg(&svg, crate::png::SCALE)?
                    } else {
                        svg.into()
                    };
                    anyhow::Ok(std::fs::write(&path, bytes)?)
                })
                .await;
            if let Err(err) = written {
                this.update_in(cx, |this, window, cx| {
                    this.show_error(&t!("dialog.image_failed"), &err.to_string(), window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    fn report_export(
        &mut self,
        result: Result<umlcraft_asta_export::Report, umlcraft_asta_export::ExportError>,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(report) if report.is_complete() => {
                let detail = path.display().to_string();
                self.inform(PromptLevel::Info, &t!("dialog.export_done"), &detail, window, cx);
            }
            Ok(report) => {
                let count: usize = report.dropped.values().sum();
                let mut lines: Vec<String> = report
                    .dropped
                    .iter()
                    .map(|(key, n)| format!("{} ({n})", export_dropped_label(key)))
                    .collect();
                lines.extend(report.problems);
                let message = t!("dialog.export_partial", count = count);
                let detail = t!("dialog.export_partial_detail", items = lines.join("\n"));
                self.inform(PromptLevel::Info, &message, &detail, window, cx);
            }
            Err(umlcraft_asta_export::ExportError::License) => {
                let detail = t!("dialog.astah_unlicensed");
                self.show_error(&t!("dialog.export_failed"), &detail, window, cx);
            }
            Err(err) => self.show_error(&t!("dialog.export_failed"), &err.to_string(), window, cx),
        }
    }

    fn save_project(&mut self, _: &SaveProject, window: &mut Window, cx: &mut Context<Self>) {
        self.save(false, window, cx, None);
    }

    fn save_project_as(&mut self, _: &SaveProjectAs, window: &mut Window, cx: &mut Context<Self>) {
        self.save(true, window, cx, None);
    }

    /// Saves to the current file, or asks for one (always with `save_as`). Runs `then` after a
    /// successful save.
    fn save(
        &mut self,
        save_as: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: Option<Continuation>,
    ) {
        let ws = self.workspace.read(cx);
        let current = ws.path.clone();
        if let Some(path) = current.clone().filter(|_| !save_as) {
            if self.write(path, window, cx)
                && let Some(then) = then
            {
                then(self, window, cx);
            }
            return;
        }
        let model = ws.doc.model();
        let project = model.element(model.root()).map(|e| e.name().to_owned()).unwrap_or_default();
        let suggested = current
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{FILE_EXTENSION}", sanitize_file_name(&project)));
        let rx = cx.prompt_for_new_path(&start_directory(current.as_deref()), Some(&suggested));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = with_project_extension(path);
            this.update_in(cx, move |this, window, cx| {
                if this.write(path, window, cx)
                    && let Some(then) = then
                {
                    then(this, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn write(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match umlcraft_format::save(self.workspace.read(cx).doc.model(), &path) {
            Ok(()) => {
                self.workspace.update(cx, |ws, cx| {
                    ws.path = Some(path.clone());
                    ws.doc.mark_saved();
                    cx.notify();
                });
                self.remember(&path, cx);
                true
            }
            Err(err) => {
                self.show_error(&t!("dialog.save_failed"), &err.to_string(), window, cx);
                false
            }
        }
    }

    fn remember(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.settings.add_recent(path);
        self.settings.save();
        self.reload_menus(cx);
    }

    fn show_error(&self, message: &str, detail: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.inform(PromptLevel::Critical, message, detail, window, cx);
    }

    /// Shows a message with an OK button. The answer has to be awaited even though nobody
    /// needs it: dropping the receiver cancels the dialog before it appears.
    fn inform(
        &self,
        level: PromptLevel,
        message: &str,
        detail: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = window.prompt(level, message, Some(detail), &["OK"], cx);
        cx.spawn_in(window, async move |_, _| {
            rx.await.ok();
        })
        .detach();
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, Box::new(|_, _, cx| cx.quit()));
    }

    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_confirmed || !self.workspace.read(cx).doc.is_dirty() {
            return true;
        }
        self.confirm_discard(
            window,
            cx,
            Box::new(|this, window, _| {
                this.close_confirmed = true;
                window.remove_window();
            }),
        );
        false
    }

    // ----- Edit and view ---------------------------------------------------------------------

    fn with_workspace(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Workspace)) {
        self.workspace.update(cx, |ws, cx| {
            f(ws);
            cx.notify();
        });
    }

    fn align(&mut self, alignment: Alignment, cx: &mut Context<Self>) {
        self.with_workspace(cx, |ws| ws.align_selection(alignment));
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(copied) = self.workspace.read(cx).copy_selection() {
            self.clipboard = Some(copied);
            self.paste_count = 0;
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&Copy, window, cx);
        self.with_workspace(cx, Workspace::delete_selection);
    }

    /// Pastes duplicates. In the diagram copied from, each paste lands a bit further down and
    /// right; elsewhere at the pointer.
    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        self.paste_clip(false, window, cx);
    }

    fn paste_as_reference(
        &mut self,
        _: &PasteAsReference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste_clip(true, window, cx);
    }

    fn paste_clip(&mut self, as_reference: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((source, clip)) = self.clipboard.clone() else { return };
        let Some(origin) = clip.nodes.iter().map(|n| n.bounds).reduce(|a, b| a.union(&b)) else {
            return;
        };
        let same_diagram = self.workspace.read(cx).active_diagram == Some(source);
        let offset = if same_diagram && !as_reference {
            self.paste_count += 1;
            let step = 20.0 * self.paste_count as f32;
            umlcraft_model::Point::new(step, step)
        } else {
            let at = self.canvas.read(cx).paste_position(window);
            umlcraft_model::Point::new(at.x - origin.x, at.y - origin.y)
        };
        self.with_workspace(cx, |ws| ws.paste(&clip, offset, as_reference));
    }

    fn toggle_theme(&mut self, _: &ToggleTheme, window: &mut Window, cx: &mut Context<Self>) {
        let dark = !cx.theme().is_dark();
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        self.settings.dark = Some(dark);
        self.settings.save();
    }

    fn toggle_language(&mut self, _: &ToggleLanguage, window: &mut Window, cx: &mut Context<Self>) {
        let next = if &*rust_i18n::locale() == "de" { "en" } else { "de" };
        rust_i18n::set_locale(next);
        self.settings.language = Some(next.to_owned());
        self.settings.save();
        self.reload_menus(cx);
        self.sync_from_workspace(window, cx);
        self.properties.update(cx, |_, cx| cx.notify());
        window.refresh();
    }

    fn reload_menus(&mut self, cx: &mut Context<Self>) {
        install_menus(&self.settings, cx);
        self.menu_bar.update(cx, |bar, cx| bar.reload(cx));
    }

    // ----- Rendering -------------------------------------------------------------------------

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let (can_undo, can_redo) = (ws.doc.can_undo(), ws.doc.can_redo());
        let can_align = ws.selected_nodes().len() > 1;
        let zoom = (self.canvas.read(cx).zoom() * 100.0).round();
        let separator = || div().w(px(1.0)).h(px(18.0)).mx_1().bg(cx.theme().border);
        let action_button = |id: &'static str, label: String, action: Box<dyn Action>| {
            Button::new(id).ghost().small().label(label).on_click(move |_, window, cx| {
                window.dispatch_action(action.boxed_clone(), cx);
            })
        };
        h_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(action_button("save", t!("toolbar.save").into(), Box::new(SaveProject)))
            .child(separator())
            .child(
                action_button("undo", t!("toolbar.undo").into(), Box::new(Undo))
                    .disabled(!can_undo),
            )
            .child(
                action_button("redo", t!("toolbar.redo").into(), Box::new(Redo))
                    .disabled(!can_redo),
            )
            .child(separator())
            .child(action_button("zoom-out", "−".into(), Box::new(ZoomOut)))
            .child(
                action_button("zoom-reset", format!("{zoom} %"), Box::new(ResetZoom))
                    .tooltip(t!("toolbar.zoom_reset")),
            )
            .child(action_button("zoom-in", "+".into(), Box::new(ZoomIn)))
            .child(action_button("zoom-fit", t!("toolbar.zoom_fit").into(), Box::new(ZoomToFit)))
            .child(separator())
            .children(
                [
                    ("align-left", "⇤", AlignLeft.boxed_clone(), "menu.align_left"),
                    ("align-center", "↔", AlignCenter.boxed_clone(), "menu.align_center"),
                    ("align-right", "⇥", AlignRight.boxed_clone(), "menu.align_right"),
                    ("align-top", "⤒", AlignTop.boxed_clone(), "menu.align_top"),
                    ("align-middle", "↕", AlignMiddle.boxed_clone(), "menu.align_middle"),
                    ("align-bottom", "⤓", AlignBottom.boxed_clone(), "menu.align_bottom"),
                ]
                .map(|(id, symbol, action, tip)| {
                    action_button(id, symbol.into(), action).disabled(!can_align).tooltip(t!(tip))
                }),
            )
            .child(div().flex_1())
            .child(action_button("theme", t!("toolbar.theme").into(), Box::new(ToggleTheme)))
            .child(action_button(
                "language",
                t!("toolbar.language").into(),
                Box::new(ToggleLanguage),
            ))
    }

    fn model_browser(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let meta = self.tree_meta.clone();
        let menu_meta = self.tree_meta.clone();
        let muted = cx.theme().muted_foreground;
        let drop_highlight = cx.theme().list_active;
        let workspace = self.workspace.clone();
        v_flex().size_full().child(panel_header(t!("panel.model"), cx)).child(
            div().flex_1().min_h_0().child(
                tree(&self.tree, move |ix, entry, selected, _, _| {
                    let item = entry.item();
                    let meta = meta.get(&item.id);
                    let kind = meta.map(|m| m.kind.type_name()).unwrap_or_default();
                    let mut row = div().id(("tree-row", ix)).w_full().child(
                        h_flex()
                            .gap_2()
                            .overflow_hidden()
                            .child(div().text_sm().child(item.label.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t!(format!("kind.{kind}")).to_string()),
                            ),
                    );
                    // Every element but the root can be dragged: into another tree item (to
                    // change its owner) or onto the canvas (to show it there).
                    if let Some(meta) = meta.filter(|m| !m.is_root) {
                        let dragged = DraggedElement { id: meta.id, label: item.label.clone() };
                        row = row.on_drag(dragged, |d, _, _, cx| cx.new(|_| d.clone()));
                    }
                    if let Some(target) = meta.map(|m| m.id) {
                        let workspace = workspace.clone();
                        row = row
                            .drag_over::<DraggedElement>(move |style, _, _, _| {
                                style.bg(drop_highlight)
                            })
                            .on_drop(move |d: &DraggedElement, _, cx| {
                                let id = d.id;
                                workspace.update(cx, |ws, cx| {
                                    ws.move_into(id, target);
                                    cx.notify();
                                });
                            });
                    }
                    ListItem::new(ix)
                        .selected(selected)
                        .pl(px(8.0 + 14.0 * entry.depth() as f32))
                        .child(row)
                })
                .context_menu(move |_, entry, menu, _, _| {
                    match menu_meta.get(&entry.item().id) {
                        Some(meta) => tree_menu(meta, menu),
                        None => menu,
                    }
                }),
            ),
        )
    }

    fn diagram_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let active_ix =
            ws.active_diagram.and_then(|a| ws.open_diagrams.iter().position(|&d| d == a));
        let tabs: Vec<Tab> = ws
            .open_diagrams
            .iter()
            .enumerate()
            .map(|(ix, &d)| {
                let name = model.get(d).map(labels::display_name).unwrap_or_default();
                let workspace = self.workspace.clone();
                Tab::new().label(name).suffix(
                    Button::new(("close-tab", ix)).ghost().xsmall().label("×").on_click(
                        move |_, _, cx| {
                            cx.stop_propagation();
                            workspace.update(cx, |ws, cx| {
                                ws.close_diagram(d);
                                cx.notify();
                            });
                        },
                    ),
                )
            })
            .collect();
        let open = ws.open_diagrams.clone();
        let workspace = self.workspace.clone();
        let tab_bar = TabBar::new("diagram-tabs")
            .small()
            .children(tabs)
            .selected_index(active_ix.unwrap_or(0))
            .on_click(move |ix: &usize, _, cx| {
                if let Some(&d) = open.get(*ix) {
                    workspace.update(cx, |ws, cx| {
                        ws.open_diagram(d);
                        cx.notify();
                    });
                }
            });

        let kind = ws.active_diagram.and_then(|d| model.get(d)).and_then(|e| e.kind().as_diagram());
        let current_tool = ws.tool;
        let palette_row =
            h_flex().gap_1().px_2().py_1().border_b_1().border_color(cx.theme().border).children(
                kind.map(palette::palette).unwrap_or_default().into_iter().enumerate().map(
                    |(ix, tool)| {
                        let workspace = self.workspace.clone();
                        let label = match tool {
                            Tool::Node(NodeTool::Note) => t!("tool.note").to_string(),
                            _ => t!(format!("tool.{}", tool.key())).to_string(),
                        };
                        Button::new(("tool", ix))
                            .ghost()
                            .xsmall()
                            .label(label)
                            .selected(current_tool == tool)
                            .on_click(move |_, _, cx| {
                                workspace.update(cx, |ws, cx| {
                                    ws.tool = tool;
                                    cx.notify();
                                });
                            })
                    },
                ),
            );

        let body = if ws.active_diagram.is_some() {
            div().flex_1().min_h_0().child(self.canvas.clone()).into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t!("canvas.no_diagram").to_string())
                .into_any_element()
        };
        v_flex().size_full().child(tab_bar).child(palette_row).child(body)
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let zoom = (self.canvas.read(cx).zoom() * 100.0).round();
        let state = if ws.doc.is_dirty() { t!("status.modified") } else { t!("status.saved") };
        let selection = match ws.selection.len() {
            0 => String::new(),
            n => format!("{n} {}", t!("status.selected")),
        };
        let right = match &ws.message {
            _ if self.exporting => div().child(t!("status.exporting").to_string()),
            Some(message) => div().text_color(cx.theme().danger).child(message.clone()),
            None => div().child(match ws.tool {
                Tool::Select => t!("status.hint").to_string(),
                Tool::Node(_) => t!("status.hint_node").to_string(),
                Tool::Edge(_) => t!("status.hint_edge").to_string(),
            }),
        };
        h_flex()
            .gap_4()
            .px_3()
            .py_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(ws.file_name().unwrap_or_else(|| t!("status.unsaved_file").to_string()))
            .child(format!("{zoom} %"))
            .child(state.to_string())
            .child(selection)
            .child(div().flex_1())
            .child(right)
    }
}

fn tree_menu(meta: &TreeMeta, menu: PopupMenu) -> PopupMenu {
    let target = meta.id;
    let command = |command| Box::new(TreeCommand { target, command });
    let mut menu = menu;
    if meta.kind.as_diagram().is_some() {
        menu = menu.menu(t!("menu.open_diagram"), command(TreeCommandKind::Open)).separator();
    }
    let items = [
        NewItem::Element(NodeTool::Package),
        NewItem::Element(NodeTool::Class),
        NewItem::Element(NodeTool::Interface),
        NewItem::Element(NodeTool::Enumeration),
        NewItem::Element(NodeTool::Actor),
        NewItem::Element(NodeTool::UseCase),
        NewItem::Element(NodeTool::SystemBoundary),
        NewItem::Element(NodeTool::Partition),
        NewItem::Element(NodeTool::State),
        NewItem::Element(NodeTool::Component),
        NewItem::Element(NodeTool::Artifact),
        NewItem::Element(NodeTool::DeploymentNode),
        NewItem::Attribute,
        NewItem::Operation,
        NewItem::Literal,
        NewItem::ExtensionPoint,
    ];
    let mut any = false;
    for item in items.into_iter().filter(|i| i.applies_to(&meta.kind)) {
        let key = item.key();
        let label = format!("{} {}", t!("menu.new_prefix"), t!(key.as_str()));
        menu = menu.menu(label, command(TreeCommandKind::New(item)));
        any = true;
    }
    if any {
        menu = menu.separator();
    }
    let mut any = false;
    for kind in [
        DiagramKind::Class,
        DiagramKind::UseCase,
        DiagramKind::Sequence,
        DiagramKind::Activity,
        DiagramKind::StateMachine,
        DiagramKind::Component,
        DiagramKind::Deployment,
    ] {
        let item = NewItem::Diagram(kind);
        if item.applies_to(&meta.kind) {
            let key = item.key();
            let label = format!("{} {}", t!("menu.new_prefix"), t!(key.as_str()));
            menu = menu.menu(label, command(TreeCommandKind::New(item)));
            any = true;
        }
    }
    if any {
        menu = menu.separator();
    }
    menu = menu.menu(t!("menu.rename"), command(TreeCommandKind::Rename));
    if !meta.is_root {
        menu = menu.menu(t!("menu.delete"), command(TreeCommandKind::Delete));
    }
    menu
}

fn tree_item(
    model: &Model,
    id: Id,
    collapsed: &HashSet<SharedString>,
    meta: &mut HashMap<SharedString, TreeMeta>,
) -> TreeItem {
    let e = model.get(id).expect("tree only visits existing elements");
    let key = SharedString::from(id.to_string());
    meta.insert(key.clone(), TreeMeta { id, kind: e.kind().clone(), is_root: id == model.root() });
    let children: Vec<TreeItem> =
        e.children().iter().map(|&c| tree_item(model, c, collapsed, meta)).collect();
    TreeItem::new(key.clone(), labels::tree_name(model, e))
        .expanded(!collapsed.contains(&key))
        .children(children)
}

fn panel_header(title: impl Into<SharedString>, cx: &App) -> impl IntoElement {
    div()
        .px_3()
        .py_1p5()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .border_b_1()
        .border_color(cx.theme().border)
        .child(title.into())
}

/// A readable name for an Astah metaclass, e.g. `UMatrixDiagramImp` → `MatrixDiagram`.
fn astah_class_label(class: &str) -> &str {
    let name = class.strip_suffix("Imp").unwrap_or(class);
    name.strip_prefix('U').filter(|rest| rest.starts_with(char::is_uppercase)).unwrap_or(name)
}

fn menu_item(key: &str, action: impl Action) -> MenuItem {
    MenuItem::action(t!(key).to_string(), action)
}

fn menus(settings: &Settings) -> Vec<Menu> {
    let mut recent: Vec<MenuItem> = settings
        .recent
        .iter()
        .enumerate()
        .map(|(index, path)| MenuItem::action(recent_label(path), OpenRecent { index }))
        .collect();
    if recent.is_empty() {
        recent.push(MenuItem::action(t!("menu.no_recent").to_string(), Cancel).disabled(true));
    }
    let mut file = vec![
        menu_item("menu.new", NewProject),
        menu_item("menu.open", OpenProject),
        MenuItem::submenu(Menu::new(t!("menu.recent").to_string()).items(recent)),
        menu_item("menu.import_astah", ImportAstah),
    ];
    file.push(MenuItem::submenu(Menu::new(t!("menu.export").to_string()).items([
        menu_item("menu.export_svg", ExportSvg),
        menu_item("menu.export_png", ExportPng),
        menu_item("menu.export_astah", ExportAstah),
    ])));
    file.extend([
        MenuItem::separator(),
        menu_item("menu.save", SaveProject),
        menu_item("menu.save_as", SaveProjectAs),
        MenuItem::separator(),
        menu_item("menu.quit", Quit),
    ]);
    vec![
        Menu::new(t!("menu.file").to_string()).items(file),
        Menu::new(t!("menu.edit").to_string()).items([
            menu_item("toolbar.undo", Undo),
            menu_item("toolbar.redo", Redo),
            MenuItem::separator(),
            menu_item("menu.delete", DeleteSelection),
            menu_item("menu.remove_from_diagram", RemoveFromDiagram),
            menu_item("menu.select_all", SelectAll),
            MenuItem::separator(),
            menu_item("menu.copy", Copy),
            menu_item("menu.cut", Cut),
            menu_item("menu.paste", Paste),
            menu_item("menu.paste_reference", PasteAsReference),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new(t!("menu.align").to_string()).items([
                menu_item("menu.align_left", AlignLeft),
                menu_item("menu.align_center", AlignCenter),
                menu_item("menu.align_right", AlignRight),
                menu_item("menu.align_top", AlignTop),
                menu_item("menu.align_middle", AlignMiddle),
                menu_item("menu.align_bottom", AlignBottom),
            ])),
            MenuItem::submenu(
                Menu::new(t!("menu.line_style").to_string())
                    .items(LINE_SHAPES.map(|(shape, key)| menu_item(key, SetLineShape { shape }))),
            ),
            menu_item("menu.wrap_in_fragment", WrapInFragment),
        ]),
        Menu::new(t!("menu.view").to_string()).items([
            menu_item("menu.zoom_in", ZoomIn),
            menu_item("menu.zoom_out", ZoomOut),
            menu_item("menu.zoom_reset", ResetZoom),
            menu_item("toolbar.zoom_fit", ZoomToFit),
            MenuItem::separator(),
            menu_item("toolbar.theme", ToggleTheme),
            menu_item("toolbar.language", ToggleLanguage),
        ]),
        Menu::new(t!("menu.help").to_string()).items([menu_item("menu.open_sample", OpenSample)]),
    ]
}

fn install_menus(settings: &Settings, cx: &mut App) {
    cx.set_menus(menus(settings));
    let owned = menus(settings).into_iter().map(|m| m.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
}

/// A new project with one empty class diagram.
fn blank_document() -> Document {
    let mut doc = Document::new(Model::new(t!("project.untitled").to_string()));
    let root = doc.model().root();
    let name = format!("{}1", crate::workspace::name_base("diagram.class", true));
    doc.edit("", |tx| {
        tx.create(root, ElementData::new(name, ElementKind::diagram(DiagramKind::Class)))
    })
    .expect("a model can always own a class diagram");
    doc.clear_history();
    doc
}

fn with_project_extension(path: PathBuf) -> PathBuf {
    with_extension(path, FILE_EXTENSION)
}

fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(extension)) {
        return path;
    }
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{extension}"));
    path.with_file_name(name)
}

/// The installed Astah used for exporting, looked up once.
fn astah() -> Option<&'static umlcraft_asta_export::Astah> {
    static ASTAH: std::sync::OnceLock<Option<umlcraft_asta_export::Astah>> =
        std::sync::OnceLock::new();
    ASTAH.get_or_init(umlcraft_asta_export::Astah::find).as_ref()
}

/// What the export reports as left out: an element kind, or something Astah cannot express.
fn export_dropped_label(key: &str) -> String {
    for prefix in ["export_dropped", "kind"] {
        let path = format!("{prefix}.{key}");
        let label = t!(path.as_str()).to_string();
        if label != path {
            return label;
        }
    }
    key.to_owned()
}

/// "name.ucraft — C:\Users\…\folder": file name first, the folder shortened in the middle.
fn recent_label(path: &Path) -> String {
    const MAX: usize = 40;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let chars: Vec<char> = dir.chars().collect();
    let dir = if chars.len() > MAX {
        let head: String = chars[..12].iter().collect();
        let tail: String = chars[chars.len() - (MAX - 13)..].iter().collect();
        format!("{head}…{tail}")
    } else {
        dir
    };
    format!("{name} — {dir}")
}

fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { "project".to_owned() } else { cleaned.to_owned() }
}

/// Where file dialogs start: next to the project file, else in the user's documents.
fn start_directory(project: Option<&Path>) -> PathBuf {
    project
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(documents_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn documents_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?);
    let documents = home.join("Documents");
    Some(if documents.is_dir() { documents } else { home })
}

impl Render for MainView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let canvas = self.canvas.clone();
        let (c1, c2, c3, c4) = (canvas.clone(), canvas.clone(), canvas.clone(), canvas.clone());
        v_flex()
            .id("main")
            .key_context("UmlCraft")
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::import_astah))
            .on_action(cx.listener(Self::export_astah))
            .on_action(cx.listener(Self::export_svg))
            .on_action(cx.listener(Self::export_png))
            .on_action(cx.listener(Self::open_recent))
            .on_action(cx.listener(Self::open_sample))
            .on_action(cx.listener(Self::save_project))
            .on_action(cx.listener(Self::save_project_as))
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::tree_command))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::paste_as_reference))
            .on_action(cx.listener(Self::toggle_theme))
            .on_action(cx.listener(Self::toggle_language))
            .on_action(cx.listener(|this, _: &Undo, _, cx| {
                this.with_workspace(cx, |ws| {
                    ws.doc.undo();
                })
            }))
            .on_action(cx.listener(|this, _: &Redo, _, cx| {
                this.with_workspace(cx, |ws| {
                    ws.doc.redo();
                })
            }))
            .on_action(cx.listener(|this, _: &DeleteSelection, _, cx| {
                this.with_workspace(cx, Workspace::delete_selection)
            }))
            .on_action(cx.listener(|this, _: &RemoveFromDiagram, _, cx| {
                this.with_workspace(cx, Workspace::remove_selection_from_diagram)
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.with_workspace(cx, Workspace::select_all)
            }))
            .on_action(cx.listener(|this, _: &WrapInFragment, _, cx| {
                this.with_workspace(cx, Workspace::wrap_selection_in_fragment)
            }))
            .on_action(cx.listener(|this, _: &AlignLeft, _, cx| this.align(Alignment::Left, cx)))
            .on_action(cx.listener(|this, _: &AlignCenter, _, cx| {
                this.align(Alignment::HorizontalCenter, cx)
            }))
            .on_action(cx.listener(|this, _: &AlignRight, _, cx| this.align(Alignment::Right, cx)))
            .on_action(cx.listener(|this, _: &AlignTop, _, cx| this.align(Alignment::Top, cx)))
            .on_action(
                cx.listener(|this, _: &AlignMiddle, _, cx| {
                    this.align(Alignment::VerticalCenter, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &AlignBottom, _, cx| this.align(Alignment::Bottom, cx)),
            )
            .on_action(move |_: &ZoomIn, _, cx| c1.update(cx, |c, cx| c.zoom_by(1.2, cx)))
            .on_action(move |_: &ZoomOut, _, cx| c2.update(cx, |c, cx| c.zoom_by(1.0 / 1.2, cx)))
            .on_action(move |_: &ResetZoom, _, cx| c3.update(cx, |c, cx| c.reset_zoom(cx)))
            .on_action(move |a: &SetLineShape, _, cx| {
                c4.update(cx, |c, cx| c.set_line_shape(a.shape, cx))
            })
            .on_action(move |_: &ZoomToFit, _, cx| canvas.update(cx, |c, cx| c.zoom_to_fit(cx)))
            .child(
                div()
                    .h(px(30.0))
                    .border_b_1()
                    .border_color(theme.border)
                    .child(self.menu_bar.clone()),
            )
            .child(self.toolbar(cx))
            .child(
                div().flex_1().min_h_0().child(
                    h_resizable("main-split")
                        .child(
                            resizable_panel()
                                .size(px(260.0))
                                .size_range(px(160.0)..px(520.0))
                                .child(self.model_browser(cx)),
                        )
                        .child(resizable_panel().child(self.diagram_area(cx)))
                        .child(
                            resizable_panel()
                                .size(px(280.0))
                                .size_range(px(180.0)..px(560.0))
                                .child(
                                    v_flex()
                                        .size_full()
                                        .child(panel_header(t!("panel.properties"), cx))
                                        .child(
                                            div()
                                                .id("properties-scroll")
                                                .flex_1()
                                                .min_h_0()
                                                .overflow_y_scroll()
                                                .child(self.properties.clone()),
                                        ),
                                ),
                        ),
                ),
            )
            .child(self.status_bar(cx))
    }
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use std::path::PathBuf;

    use super::{sanitize_file_name, with_project_extension};

    #[test]
    fn file_names_get_the_project_extension() {
        assert_eq!(with_project_extension("a/b.ucraft".into()), PathBuf::from("a/b.ucraft"));
        assert_eq!(with_project_extension("a/b".into()), PathBuf::from("a/b.ucraft"));
        assert_eq!(with_project_extension("a/v1.2".into()), PathBuf::from("a/v1.2.ucraft"));
        assert_eq!(sanitize_file_name("Order: System?"), "Order_ System_");
        assert_eq!(sanitize_file_name("  "), "project");
    }

    // Windows paths: elsewhere the backslashes are not separators.
    #[cfg(windows)]
    #[test]
    fn recent_labels_put_the_file_name_first_and_shorten_folders() {
        use super::recent_label;

        let short = PathBuf::from(r"C:\Projekte\shop.ucraft");
        assert_eq!(recent_label(&short), r"shop.ucraft — C:\Projekte");
        let long =
            PathBuf::from(r"C:\Users\ulf\AppData\Local\Temp\claude\abcdef\scratchpad\m2.ucraft");
        let label = recent_label(&long);
        assert!(label.starts_with(r"m2.ucraft — C:\Users\ulf…"), "{label}");
        assert!(label.ends_with(r"abcdef\scratchpad"), "{label}");
        assert!(label.chars().count() <= "m2.ucraft — ".len() + 40, "{label}");
    }
}
