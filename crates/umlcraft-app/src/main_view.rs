//! Main window: menu bar, toolbar, model tree, diagram tabs with palette and canvas,
//! properties panel and status bar. Also owns file handling (new, open, save, recent files).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, StyledExt, Theme, ThemeMode, TitleBar, h_flex, h_resizable,
    resizable_panel, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_format::FILE_EXTENSION;
use umlcraft_model::{DiagramKind, Document, ElementData, ElementKind, Id, Model};
use umlcraft_notation::clipboard::Clip;
use umlcraft_notation::palette::{NodeTool, Tool};
use umlcraft_notation::scene::{self, Alignment, Preview};
use umlcraft_reverse::{Problem, SourceLanguage};

use crate::canvas::{AppMeasure, DiagramCanvas, DraggedElement, FontMeasure};
use crate::labels;
use crate::nsd::{NsdView, source};
use crate::properties::PropertiesPanel;
use crate::ribbon::{self, RibbonTab};
use crate::settings::Settings;
use crate::workspace::{NewItem, Workspace};
use crate::*;

/// Something to do once the user has dealt with unsaved changes.
type Continuation = Box<dyn FnOnce(&mut MainView, &mut Window, &mut Context<MainView>)>;

/// Work running in the background, shown in the status bar.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Busy {
    Opening,
    ExportingAstah,
    ImportingAstah,
    ImportingSource,
}

/// At most this many problem files are listed after a source import.
const LISTED_PROBLEMS: usize = 20;

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
    busy: Option<Busy>,
    /// Set to stop the running export to Astah.
    cancel_export: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Counts replaced documents, so background work can tell whether its document is still
    /// the open one.
    generation: u64,
    /// A message for the status bar, shown while the document stays at the given revision.
    notice: Option<(String, u64)>,
    ribbon_tab: RibbonTab,
    /// The diagram the ribbon last saw active; opening another one shows its tools.
    ribbon_diagram: Option<Id>,
    /// The first click of a double click on a tab unfolded the ribbon.
    ribbon_unfolded_by_click: bool,
    /// Pixels a touchpad scrolled over the ribbon that did not yet make a tab.
    ribbon_wheel: f32,
    /// The views of the structograms opened so far, kept while they exist.
    structograms: HashMap<Id, Entity<NsdView>>,
    /// What the import of a structogram from code could not show, for its view.
    structogram_notes: HashMap<Id, Vec<String>>,
    /// The diagram or structogram shown last, to focus a structogram once it shows.
    shown: Option<Id>,
    /// Where `umlcraft-cli mcp` reaches the open project; announced while it lives.
    _live: Option<umlcraft_mcp::live::LiveServer>,
    _subscriptions: Vec<Subscription>,
}

impl MainView {
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        if settings.dark == Some(true) {
            Theme::change(ThemeMode::Dark, Some(window), cx);
        }
        let (display, keywords) = (settings.display(), settings.keywords());
        let workspace = cx.new(|cx| {
            Workspace::new(blank_document(), None)
                .with_measure(AppMeasure::new(cx, display))
                .with_keywords(keywords)
        });
        let canvas = cx.new(|cx| {
            let mut canvas = DiagramCanvas::new(workspace.clone(), cx);
            canvas.display = display;
            // Inside the card's 1 px border.
            canvas.corner_radius = f32::from(shell::RADIUS) - 1.0;
            canvas
        });
        let properties = cx.new(|cx| PropertiesPanel::new(workspace.clone(), window, cx));
        let tree = cx.new(|cx| TreeState::new(cx));
        install_menus(&settings, cx);

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
        // Calls of the MCP server, one after the other on the main thread.
        let live = crate::live::start().map(|(server, calls)| {
            cx.spawn_in(window, async move |this, cx| {
                while let Ok(request) = calls.recv().await {
                    if this.update(cx, |this, cx| this.serve_mcp(request, cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
            server
        });
        let mut view = Self {
            workspace,
            canvas,
            properties,
            tree,
            settings,
            tree_meta: Rc::default(),
            collapsed: HashSet::new(),
            tree_revision: None,
            tree_selection: None,
            close_confirmed: false,
            busy: None,
            cancel_export: None,
            generation: 0,
            notice: None,
            clipboard: None,
            paste_count: 0,
            ribbon_tab: RibbonTab::Home,
            ribbon_diagram: None,
            ribbon_unfolded_by_click: false,
            ribbon_wheel: 0.0,
            structograms: HashMap::new(),
            structogram_notes: HashMap::new(),
            shown: None,
            _live: live,
            _subscriptions: subscriptions,
        };
        if !crate::canvas::choose_diagram_font(cx) {
            let revision = view.workspace.read(cx).doc.revision();
            view.notice = Some((t!("status.no_diagram_font").to_string(), revision));
        }
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

        self.sync_structograms(window, cx);
        let ws = self.workspace.read(cx);
        if ws.active_diagram != self.ribbon_diagram {
            self.ribbon_diagram = ws.active_diagram;
            self.ribbon_tab = match ws.active_diagram {
                Some(_) => RibbonTab::Diagram,
                None if self.ribbon_tab == RibbonTab::Diagram => RibbonTab::Home,
                None => self.ribbon_tab,
            };
        }

        let model = ws.doc.model();
        let name = ws.file_name().unwrap_or_else(|| {
            model.element(model.root()).map(|e| e.name().to_owned()).unwrap_or_default()
        });
        let dirty = if ws.doc.is_dirty() { "*" } else { "" };
        let version = env!("CARGO_PKG_VERSION");
        window.set_window_title(&format!("{name}{dirty} – UmlCraft {version}"));
        cx.notify();
    }

    /// Makes a view for the structogram shown, focuses it when it comes up, and drops the
    /// views of structograms that are gone.
    fn sync_structograms(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let active = ws.active_diagram;
        self.structograms.retain(|&id, _| crate::workspace::is_structogram(model, id));
        let shown = active.filter(|&id| crate::workspace::is_structogram(model, id));
        if let Some(id) = shown
            && !self.structograms.contains_key(&id)
        {
            let notes = self.structogram_notes.remove(&id).unwrap_or_default();
            let workspace = self.workspace.clone();
            let view = cx.new(|cx| NsdView::new(workspace, id, notes, cx));
            self.structograms.insert(id, view);
        }
        if active != self.shown {
            self.shown = active;
            if let Some(view) = self.active_structogram(cx) {
                view.focus_handle(cx).focus(window, cx);
            }
        }
    }

    /// The view of the structogram shown, if a structogram is shown.
    fn active_structogram(&self, cx: &App) -> Option<Entity<NsdView>> {
        self.structograms.get(&self.workspace.read(cx).active_diagram?).cloned()
    }

    /// Where commands from the ribbon, the menus and shortcuts go: the structogram shown, else
    /// the canvas.
    fn view_focus(&self, cx: &App) -> FocusHandle {
        match self.active_structogram(cx) {
            Some(view) => view.focus_handle(cx),
            None => self.canvas.focus_handle(cx),
        }
    }

    fn on_tree_changed(&mut self, tree: Entity<TreeState>, cx: &mut Context<Self>) {
        let selected = tree.read(cx).selected_item().map(|item| item.id.clone());
        if selected == self.tree_selection {
            return;
        }
        self.tree_selection = selected.clone();
        let Some(id) = selected.and_then(|s| s.parse::<Id>().ok()) else { return };
        self.workspace.update(cx, |ws, cx| {
            if crate::workspace::is_shown(ws.doc.model(), id) {
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
                self.properties.update(cx, |p, cx| p.focus_name(target, window, cx));
            }
            TreeCommandKind::Delete => self.workspace.update(cx, |ws, cx| {
                ws.delete_element(target);
                cx.notify();
            }),
        }
    }

    // ----- Files ---------------------------------------------------------------------------

    /// Commits text typed into the canvas or the properties panel but not yet confirmed, so
    /// that saving, exporting and the unsaved-changes check see it.
    fn flush_edits(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.canvas.update(cx, |canvas, cx| canvas.commit_edit(cx));
        self.properties.update(cx, |properties, cx| properties.flush(cx));
        if let Some(view) = self.active_structogram(cx) {
            view.update(cx, |view, cx| view.flush_dialog(window, cx));
        }
    }

    /// Runs `then` right away, or after asking what to do with unsaved changes.
    fn confirm_discard(&mut self, window: &mut Window, cx: &mut Context<Self>, then: Continuation) {
        self.flush_edits(window, cx);
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

    /// Runs a call of the MCP server against the open project and answers it.
    fn serve_mcp(&mut self, request: umlcraft_mcp::live::Request, cx: &mut Context<Self>) {
        let mut effects = crate::live::Effects::default();
        let reply = self.workspace.update(cx, |ws, cx| {
            let mut host = crate::live::AppHost { ws, effects: &mut effects };
            let reply = umlcraft_mcp::tools::call(
                &mut host,
                &request.access,
                &request.name,
                &request.arguments,
            );
            ws.validate();
            cx.notify();
            reply
        });
        if let Some((doc, path)) = effects.replace {
            if let Some(path) = &path {
                self.remember(path, cx);
            }
            self.replace_document(doc, path, cx);
        }
        if let Some(path) = effects.saved {
            self.remember(&path, cx);
        }
        request.answer(reply);
    }

    fn replace_document(&mut self, doc: Document, path: Option<PathBuf>, cx: &mut Context<Self>) {
        let (display, keywords) = (self.settings.display(), self.settings.keywords());
        self.workspace.update(cx, |ws, cx| {
            *ws = Workspace::new(doc, path)
                .with_measure(AppMeasure::new(cx, display))
                .with_keywords(keywords);
            cx.notify();
        });
        self.collapsed.clear();
        self.tree_revision = None;
        self.generation += 1;
        // Neither belongs to the new document: a notice keyed by revision would come back, and
        // what was copied refers to elements that are gone.
        self.notice = None;
        self.clipboard = None;
        self.structograms.clear();
        self.structogram_notes.clear();
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
            Box::new(|this, _, cx| {
                let measure = AppMeasure::new(cx, this.settings.display());
                this.replace_document(crate::demo::velo(&measure), None, cx)
            }),
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

    /// Where new structograms go: the selected package, else the project.
    fn structogram_owner(&self, cx: &App) -> Id {
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        ws.selected_element
            .filter(|&id| model.get(id).is_some_and(|e| matches!(e.kind(), ElementKind::Package)))
            .unwrap_or(model.root())
    }

    fn new_structogram(&mut self, _: &NewStructogram, _: &mut Window, cx: &mut Context<Self>) {
        let owner = self.structogram_owner(cx);
        self.with_workspace(cx, |ws| {
            ws.add_item(owner, NewItem::Structogram);
        });
    }

    /// Adds structograms from Structorizer files to the project.
    fn import_structogram(
        &mut self,
        _: &ImportStructogram,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            this.update_in(cx, |this, window, cx| {
                let mut diagrams = Vec::new();
                let mut failed = Vec::new();
                for path in paths {
                    match crate::nsd::read(&path) {
                        Ok(diagram) => diagrams.push(diagram),
                        Err(reason) => failed.push(format!("{}: {reason}", path.display())),
                    }
                }
                let owner = this.structogram_owner(cx);
                this.with_workspace(cx, |ws| {
                    ws.add_structograms(owner, &diagrams);
                });
                if !failed.is_empty() {
                    this.show_error(&crate::nsd::open_failed(), &failed.join("\n"), window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Opens a Structorizer file as a new project that holds it, to be saved next to it.
    fn open_structogram(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let diagram = match crate::nsd::read(&path) {
            Ok(diagram) => diagram,
            Err(detail) => {
                // A recent file that no longer opens leaves the list.
                if self.settings.remove_recent(&path) {
                    self.settings.save();
                    self.reload_menus(cx);
                }
                self.show_error(&crate::nsd::open_failed(), &detail, window, cx);
                return;
            }
        };
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let mut doc = Document::new(Model::new(stem));
        let root = doc.model().root();
        let kind = ElementKind::Structogram(umlcraft_model::StructogramProps::new(&diagram));
        doc.edit("", |tx| tx.create(root, ElementData::new("", kind)))
            .expect("a model can always own a structogram");
        doc.clear_history();
        doc.mark_unsaved();
        self.replace_document(doc, None, cx);
        self.workspace.update(cx, |ws, cx| {
            ws.draft = Some(with_project_extension(path.with_extension("")));
            cx.notify();
        });
        self.remember(&path, cx);
    }

    /// Opens `path` once unsaved changes are saved or discarded, e.g. for a file the system
    /// hands over.
    pub fn ask_to_open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(
            window,
            cx,
            Box::new(move |this, window, cx| this.open(path, window, cx)),
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

    /// Opens a project, or imports an `.asta` or `.nsd` file as a new project.
    pub fn open(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        // Relative paths (from the command line) would not open from elsewhere later.
        let path = std::path::absolute(&path).unwrap_or(path);
        if crate::nsd::is_structogram(&path) {
            self.open_structogram(path, window, cx);
            return;
        }
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("asta")) {
            self.import(&path, window, cx);
            return;
        }
        if self.busy.is_some() {
            return;
        }
        // Read in the background, so a large file does not freeze the window.
        self.busy = Some(Busy::Opening);
        cx.notify();
        let generation = self.generation;
        cx.spawn_in(window, async move |this, cx| {
            let file = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let (model, version) = umlcraft_format::load_versioned(&file)?;
                    let mut doc = Document::new(model);
                    // Version 1 stored no geometry for sequence diagrams.
                    if version < 2 {
                        umlcraft_notation::sequence_edit::upgrade_document(&mut doc);
                    }
                    Ok(doc)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                cx.notify();
                // Another project opened meanwhile was the user's later choice.
                if this.generation == generation {
                    this.apply_open(path, result, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_open(
        &mut self,
        path: PathBuf,
        result: Result<Document, umlcraft_format::FormatError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(doc) => {
                self.replace_document(doc, Some(path.clone()), cx);
                self.remember(&path, cx);
            }
            Err(err) => {
                // A recent file that no longer opens leaves the list.
                if self.settings.remove_recent(&path) {
                    self.settings.save();
                    self.reload_menus(cx);
                }
                let detail = crate::errors::format_error(&err);
                self.show_error(&t!("dialog.open_failed"), &detail, window, cx)
            }
        }
    }

    /// Imports an Astah project in the background as a new, not yet saved project. Says so
    /// when parts of it could not be carried over.
    fn import(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some(Busy::ImportingAstah);
        cx.notify();
        let generation = self.generation;
        let path = path.to_owned();
        cx.spawn_in(window, async move |this, cx| {
            let result =
                cx.background_executor().spawn(async move { umlcraft_asta::import(&path) }).await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                cx.notify();
                // Another project opened meanwhile was the user's later choice.
                if this.generation == generation {
                    this.apply_import(result, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_import(
        &mut self,
        result: Result<umlcraft_asta::Imported, umlcraft_asta::AstaError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(imported) => {
                let mut doc = Document::new(imported.model);
                doc.mark_unsaved();
                self.replace_document(doc, None, cx);
                let kinds = |counts: &std::collections::BTreeMap<String, usize>| {
                    let list: Vec<String> = counts
                        .iter()
                        .map(|(class, n)| format!("{} ({n})", astah_class_label(class)))
                        .collect();
                    list.join(", ")
                };
                let count: usize = imported.skipped.values().sum::<usize>()
                    + imported.failed.values().sum::<usize>();
                let mut lines = Vec::new();
                if !imported.skipped.is_empty() {
                    lines
                        .push(t!("dialog.import_partial_detail", kinds = kinds(&imported.skipped)));
                }
                if !imported.failed.is_empty() {
                    lines.push(t!("dialog.import_failed_detail", kinds = kinds(&imported.failed)));
                }
                if imported.lost_presentations > 0 {
                    let lost = imported.lost_presentations;
                    lines.push(t!("dialog.import_lost_shapes", count = lost));
                }
                if !lines.is_empty() {
                    let message = if count > 0 {
                        t!("dialog.import_partial", count = count)
                    } else {
                        t!("dialog.import_shapes_partial")
                    };
                    let detail = lines.join("\n");
                    self.inform(PromptLevel::Info, &message, &detail, window, cx);
                }
            }
            Err(err) => {
                let detail = crate::errors::asta_error(&err);
                self.show_error(&t!("dialog.import_failed"), &detail, window, cx)
            }
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
        if self.busy.is_some() {
            return;
        }
        self.flush_edits(window, cx);
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
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let started = this.update(cx, |this, cx| {
                this.busy = Some(Busy::ExportingAstah);
                this.cancel_export = Some(cancel.clone());
                cx.notify();
            });
            if started.is_err() {
                return;
            }
            let target = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move { export.run_cancellable(astah, &target, &cancel) })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                this.cancel_export = None;
                cx.notify();
                // Cancelled by the user: nothing to report.
                if !matches!(result, Err(umlcraft_asta_export::ExportError::Cancelled)) {
                    this.report_export(result, &path, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks for source files or a folder, parses them in the background, offers their methods
    /// for structograms and merges classes and structograms into the open project as one undo
    /// step, as Astah's Tools › Java › Import Java does for the classes.
    fn import_source(
        &mut self,
        action: &ImportSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.busy.is_some() {
            return;
        }
        let language = action.language;
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: !action.folder,
            directories: action.folder,
            multiple: true,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else { return };
            let started = this.update(cx, |this, cx| {
                this.busy = Some(Busy::ImportingSource);
                cx.notify();
                this.generation
            });
            let Ok(generation) = started else { return };
            let found = cx
                .background_executor()
                .spawn(async move {
                    let files = umlcraft_reverse::collect_sources_for(language, &paths)?;
                    std::io::Result::Ok(source::parse(language, &files))
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                cx.notify();
                match found {
                    Ok(_) if this.generation != generation => this.import_discarded(window, cx),
                    Ok(found) => this.choose_structograms(found, generation, window, cx),
                    Err(err) => this.show_error(
                        &t!("dialog.import_source_failed"),
                        &err.to_string(),
                        window,
                        cx,
                    ),
                }
            })
            .ok();
        })
        .detach();
    }

    /// The project the import was meant for has been replaced in the meantime.
    fn import_discarded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = t!("dialog.import_source_discarded");
        let detail = t!("dialog.import_source_discarded_detail");
        self.inform(PromptLevel::Info, &message, &detail, window, cx);
    }

    /// Offers the methods of the files read for structograms; the import goes on with those
    /// chosen, or with the classes alone.
    fn choose_structograms(
        &mut self,
        found: source::Found,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if found.files.is_empty() && found.parsed.failed.is_empty() {
            let extensions = found.language.extensions().iter().map(|e| format!(".{e}"));
            let detail = t!(
                "dialog.import_source_none_detail",
                extensions = extensions.collect::<Vec<_>>().join(", ")
            );
            self.inform(PromptLevel::Info, &t!("dialog.import_source_none"), &detail, window, cx);
            return;
        }
        if found.methods.is_empty() {
            self.apply_source(&found, Vec::new(), window, cx);
            return;
        }
        let existing = source::imported(self.workspace.read(cx).doc.model());
        let found = Arc::new(found);
        let this = cx.entity().downgrade();
        let job = found.clone();
        let chosen = move |picked: Vec<usize>, window: &mut Window, cx: &mut App| {
            this.update(cx, |this, cx| {
                this.build_structograms(job.clone(), picked, generation, window, cx)
            })
            .ok();
        };
        source::offer(&found, &existing, chosen, window, cx);
    }

    /// Builds the structograms of the methods `picked` in the background, then imports.
    fn build_structograms(
        &mut self,
        found: Arc<source::Found>,
        picked: Vec<usize>,
        generation: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.generation != generation {
            self.import_discarded(window, cx);
            return;
        }
        if picked.is_empty() {
            self.apply_source(&found, Vec::new(), window, cx);
            return;
        }
        self.busy = Some(Busy::ImportingSource);
        cx.notify();
        let (author, date) = (crate::nsd::user_name(), crate::nsd::today());
        let origin = source::made_origin(found.language);
        let keywords = self.settings.keywords();
        cx.spawn_in(window, async move |this, cx| {
            let job = found.clone();
            let built = cx
                .background_executor()
                .spawn(async move {
                    let made = umlcraft_nsd_code::Made {
                        author: &author,
                        date: &date,
                        origin: &origin,
                        keywords,
                    };
                    source::build(&job, &picked, &made)
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                this.busy = None;
                cx.notify();
                if this.generation != generation {
                    this.import_discarded(window, cx);
                } else {
                    this.apply_source(&found, built, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_source(
        &mut self,
        found: &source::Found,
        built: Vec<source::Built>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self.workspace.update(cx, |ws, cx| {
            let result = ws.import_source(found, built);
            cx.notify();
            result
        });
        match result {
            Ok((report, made)) => {
                let structograms = made.len();
                for (id, report) in made {
                    // A view shown already keeps its notes; the next one shows the new ones.
                    self.structograms.remove(&id);
                    self.structogram_notes.insert(id, source::notes(&report));
                }
                self.report_source_import(&report, structograms, window, cx)
            }
            Err(err) => {
                let detail =
                    crate::errors::model_error(Some(self.workspace.read(cx).doc.model()), &err);
                self.show_error(&t!("dialog.import_source_failed"), &detail, window, cx)
            }
        }
    }

    /// Says what a source import did: in the status bar if all went well, else in a message
    /// listing the files that failed and those whose encoding was guessed.
    fn report_source_import(
        &mut self,
        report: &umlcraft_reverse::Report,
        structograms: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut summary = t!(
            "status.imported",
            files = report.files,
            created = report.created,
            updated = report.updated,
            associations = report.associations
        )
        .to_string();
        if structograms > 0 {
            summary.push(' ');
            summary.push_str(&t!("status.imported_structograms", count = structograms));
        }
        if !report.has_problems() {
            let revision = self.workspace.read(cx).doc.revision();
            self.notice = Some((summary, revision));
            cx.notify();
            return;
        }
        let name = |path: &Path| {
            path.file_name()
                .map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
        };
        let mut lines: Vec<String> = report
            .failed
            .iter()
            .map(|f| {
                let file = name(&f.path);
                match &f.problem {
                    Problem::Syntax(e) if e.missing => t!(
                        "dialog.syntax_missing",
                        file = file,
                        line = e.line,
                        column = e.column,
                        token = e.token
                    )
                    .to_string(),
                    Problem::Syntax(e) => t!(
                        "dialog.syntax_error",
                        file = file,
                        line = e.line,
                        column = e.column,
                        token = e.token
                    )
                    .to_string(),
                    Problem::Read(error) => {
                        t!("dialog.read_error", file = file, error = error).to_string()
                    }
                }
            })
            .chain(report.guessed_encodings.iter().map(|(path, encoding)| {
                t!("dialog.encoding_guessed", file = name(path), encoding = encoding).to_string()
            }))
            .collect();
        if lines.len() > LISTED_PROBLEMS {
            let more = lines.len() - LISTED_PROBLEMS;
            lines.truncate(LISTED_PROBLEMS);
            lines.push(t!("dialog.more_files", count = more).to_string());
        }
        if !report.skipped.is_empty() {
            let skipped: Vec<String> = report
                .skipped
                .iter()
                .map(|(skip, n)| format!("{} ({n})", t!(format!("skipped.{}", skip.key()))))
                .collect();
            lines.push(String::new());
            lines.push(t!("dialog.import_source_skipped", items = skipped.join(", ")).to_string());
        }
        let message = if report.failed.is_empty() {
            summary
        } else {
            t!("dialog.import_source_partial", count = report.failed.len()).to_string()
        };
        self.inform(PromptLevel::Info, &message, &lines.join("\n"), window, cx);
    }

    /// Writes the open diagram as an SVG image, drawn as on the canvas in the light theme.
    fn export_svg(&mut self, _: &ExportSvg, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_structogram(cx) {
            Some(view) => view.update(cx, |view, cx| view.export_image(false, window, cx)),
            None => self.export_image(false, window, cx),
        }
    }

    fn export_png(&mut self, _: &ExportPng, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_structogram(cx) {
            Some(view) => view.update(cx, |view, cx| view.export_image(true, window, cx)),
            None => self.export_image(true, window, cx),
        }
    }

    /// Puts the open diagram on the clipboard as SVG and as PNG, drawn as on the canvas in the
    /// light theme.
    fn copy_image(&mut self, _: &CopyImage, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_structogram(cx) {
            Some(view) => view.update(cx, |view, cx| view.copy_image(window, cx)),
            None => {
                let Some((svg, _)) = self.diagram_svg(window, cx) else { return };
                crate::png::copy(svg, crate::png::SCALE, window, cx);
            }
        }
    }

    /// The open diagram as an SVG image, drawn as on the canvas in the light theme, and the
    /// diagram's name.
    fn diagram_svg(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(String, String)> {
        self.flush_edits(window, cx);
        let ws = self.workspace.read(cx);
        let diagram = ws.active_diagram?;
        let model = ws.doc.model();
        let measure = FontMeasure::new(window, self.settings.display());
        let scene = scene::build(model, diagram, &Preview::default(), &measure);
        // Fine units keep the letters evenly spaced in programs such as Word.
        let svg = umlcraft_notation::svg::render_scaled(
            &scene,
            &measure,
            umlcraft_notation::svg::FINE_UNITS,
        );
        let name = model.element(diagram).map(|e| e.name().to_owned()).unwrap_or_default();
        Some((svg, name))
    }

    /// Writes the open diagram as an SVG image, or rasterized as PNG, drawn as on the canvas
    /// in the light theme.
    fn export_image(&mut self, png: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some((svg, name)) = self.diagram_svg(window, cx) else { return };
        let extension = if png { "png" } else { "svg" };
        let suggested = format!("{}.{extension}", sanitize_file_name(&name));
        let directory = start_directory(self.workspace.read(cx).path.as_deref());
        let rx = cx.prompt_for_new_path(&directory, Some(&suggested));
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
            Err(err) => {
                let detail = crate::errors::export_error(&err);
                self.show_error(&t!("dialog.export_failed"), &detail, window, cx)
            }
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
        self.flush_edits(window, cx);
        let ws = self.workspace.read(cx);
        let current = ws.path.clone();
        // A project never saved may know where it would go (next to the file it came from).
        let draft = current.clone().or_else(|| ws.draft.clone());
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
        let suggested = draft
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{FILE_EXTENSION}", sanitize_file_name(&project)));
        let rx = cx.prompt_for_new_path(&start_directory(draft.as_deref()), Some(&suggested));
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
                self.show_error(
                    &t!("dialog.save_failed"),
                    &crate::errors::format_error(&err),
                    window,
                    cx,
                );
                false
            }
        }
    }

    fn remember(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.settings.remember(path);
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
        let rx = window.prompt(level, message, Some(detail), &[t!("dialog.ok").as_ref()], cx);
        cx.spawn_in(window, async move |_, _| {
            rx.await.ok();
        })
        .detach();
    }

    fn quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.confirm_discard(window, cx, Box::new(|_, _, cx| cx.quit()));
    }

    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.flush_edits(window, cx);
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
        let Some(origin) = clip.bounds() else { return };
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
        self.set_dark(!cx.theme().is_dark(), window, cx);
    }

    fn toggle_language(&mut self, _: &ToggleLanguage, window: &mut Window, cx: &mut Context<Self>) {
        let next = if &*rust_i18n::locale() == "de" { "en" } else { "de" };
        self.set_language(Some(next.to_owned()), window, cx);
    }

    fn open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        crate::settings_dialog::open(cx.entity().downgrade(), window, cx);
    }

    // ----- Settings --------------------------------------------------------------------------

    fn select_tool(&mut self, action: &SelectTool, _: &mut Window, cx: &mut Context<Self>) {
        let tool = action.tool;
        self.workspace.update(cx, |ws, cx| {
            ws.tool = tool;
            cx.notify();
        });
    }

    fn new_diagram(&mut self, action: &NewDiagram, _: &mut Window, cx: &mut Context<Self>) {
        let item = match action.objects {
            true => NewItem::ObjectDiagram,
            false => NewItem::Diagram(action.kind),
        };
        self.workspace.update(cx, |ws, cx| {
            let model = ws.doc.model();
            // A selected package takes the diagram; anything else (a class, say) does not.
            let owner = ws
                .selected_element
                .filter(|&id| {
                    model.get(id).is_some_and(|e| {
                        matches!(e.kind(), ElementKind::Package) && item.applies_to(e.kind())
                    })
                })
                .unwrap_or(model.root());
            ws.add_item(owner, item);
            cx.notify();
        });
    }

    fn select_ribbon_tab(
        &mut self,
        action: &SelectRibbonTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.ribbon_tab = action.tab;
        // Choosing a tab of a folded ribbon unfolds it; the second click of a double click
        // must not fold it again.
        self.ribbon_unfolded_by_click = self.settings.ribbon_collapsed;
        if self.settings.ribbon_collapsed {
            self.settings.ribbon_collapsed = false;
            self.settings.save();
        }
        cx.notify();
    }

    fn toggle_ribbon(&mut self, _: &ToggleRibbon, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.ribbon_unfolded_by_click) {
            return;
        }
        self.settings.ribbon_collapsed = !self.settings.ribbon_collapsed;
        self.settings.save();
        cx.notify();
    }

    /// The ribbon and the diagram card left of it.
    fn ribbon(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let props =
            ws.active_diagram.and_then(|d| ws.doc.model().get(d)).and_then(|e| match e.kind() {
                ElementKind::Diagram(p) => Some(*p),
                _ => None,
            });
        let diagram = props.map(|p| p.diagram);
        let structogram = self.active_structogram(cx).map(|view| view.read(cx).allowed());
        // A structogram's selection is its own; it pastes what the system's clipboard holds.
        let has_selection = match structogram {
            Some(allowed) => allowed.span,
            None => !ws.selection.is_empty(),
        };
        let state = ribbon::State {
            diagram,
            objects: props.is_some_and(|p| p.objects),
            structogram,
            tool: ws.tool,
            has_selection,
            can_align: ws.selected_nodes().len() > 1,
            can_paste: structogram.is_some() || (self.clipboard.is_some() && diagram.is_some()),
            can_wrap: has_selection && diagram == Some(DiagramKind::Sequence),
            numbering: props
                .filter(|p| p.diagram == DiagramKind::Sequence)
                .map(|p| (!p.hide_message_numbers, p.flat_message_numbers)),
            dark: cx.theme().is_dark(),
        };
        let tabs = ribbon::tabs(&state);
        let active = if tabs.iter().any(|(t, _)| *t == self.ribbon_tab) {
            self.ribbon_tab
        } else {
            RibbonTab::Home
        };
        let groups = ribbon::groups(active, &state);
        // The diagram card, the gaps and the window's edges take about 140 px.
        let room = f32::from(window.viewport_size().width) - 140.0;
        let compact = ribbon::width(&groups) > room;
        let collapsed = self.settings.ribbon_collapsed;
        let focus = self.view_focus(cx);
        let order: Vec<RibbonTab> = tabs.iter().map(|(t, _)| *t).collect();
        h_flex()
            .id("ribbon-area")
            .items_stretch()
            .gap(shell::GAP)
            .px(shell::GAP)
            .pt(shell::GAP)
            // As in Word: the wheel over the ribbon steps through its tabs, down to the right.
            .on_scroll(cx.listener(move |this, event: &ScrollEvent, window, cx| {
                if collapsed || !window.is_window_active() {
                    return;
                }
                let steps = match event.delta {
                    // One notch, however many lines the system scrolls for it, is one tab.
                    ScrollDelta::Lines(lines) if lines.y == 0.0 => 0,
                    ScrollDelta::Lines(lines) => -lines.y.signum() as i32,
                    ScrollDelta::Pixels(pixels) => {
                        // A touchpad scrolls in small steps: one tab per line's height.
                        this.ribbon_wheel -= f32::from(pixels.y);
                        let line = f32::from(window.line_height());
                        let steps = (this.ribbon_wheel / line).trunc();
                        this.ribbon_wheel -= steps * line;
                        steps as i32
                    }
                };
                let Some(ix) = order.iter().position(|t| *t == active) else { return };
                let next = (ix as i32 + steps).clamp(0, order.len() as i32 - 1) as usize;
                if next != ix {
                    this.ribbon_tab = order[next];
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .child(ribbon::new_diagram_card(collapsed, &focus, cx))
            .child(ribbon::render(&tabs, active, &groups, compact, collapsed, &focus, cx))
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn set_dark(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        Theme::change(if dark { ThemeMode::Dark } else { ThemeMode::Light }, Some(window), cx);
        self.settings.dark = Some(dark);
        self.settings.save();
    }

    /// Switches the user interface to `language`, or with `None` to the system's language.
    pub fn set_language(
        &mut self,
        language: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings.language = language;
        rust_i18n::set_locale(&self.settings.ui_language());
        self.settings.save();
        self.apply_keywords(cx);
        self.reload_menus(cx);
        self.sync_from_workspace(window, cx);
        // The fields' lists (Select items) take the new language too.
        self.properties.update(cx, |p, cx| {
            p.sync(window, cx);
            cx.notify();
        });
        window.refresh();
    }

    /// Writes new structograms in German or English (`de`, `en`), or as the user interface.
    pub fn set_structogram_keywords(&mut self, language: Option<String>, cx: &mut Context<Self>) {
        self.settings.structogram_keywords = language;
        self.settings.save();
        self.apply_keywords(cx);
    }

    fn apply_keywords(&mut self, cx: &mut Context<Self>) {
        let keywords = self.settings.keywords();
        self.workspace.update(cx, |ws, _| ws.keywords = keywords);
    }

    /// Turns UmlCraft's visibility column on or off, for drawing, sizing boxes and exporting.
    pub fn set_visibility_column(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.visibility_column = on;
        self.apply_display(cx);
    }

    /// Marks abstract classes with «abstract» above their name, or with `{abstract}` below it.
    pub fn set_abstract_stereotype(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.abstract_stereotype = on;
        self.apply_display(cx);
    }

    /// Ends read-only attributes in `{readOnly}`, or not.
    pub fn set_read_only_constraint(&mut self, on: bool, cx: &mut Context<Self>) {
        self.settings.read_only_constraint = on;
        self.apply_display(cx);
    }

    /// Saves the settings and draws, sizes and exports diagrams as they say from now on.
    fn apply_display(&mut self, cx: &mut Context<Self>) {
        self.settings.save();
        let display = self.settings.display();
        self.canvas.update(cx, |canvas, cx| {
            canvas.display = display;
            cx.notify();
        });
        self.workspace.update(cx, |ws, cx| {
            ws.measure = Box::new(AppMeasure::new(cx, display));
            cx.notify();
        });
    }

    fn reload_menus(&mut self, cx: &mut Context<Self>) {
        install_menus(&self.settings, cx);
        cx.notify();
    }

    // ----- Rendering -------------------------------------------------------------------------

    /// Logo, File menu, save, undo and redo, and the project's name; the window's controls on
    /// the right are the title bar's own.
    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        // The step's name, e.g. "Undo Delete".
        let named = |label: Option<&str>, key: &str, plain: &str| match label {
            Some(what) if !what.is_empty() => t!(key, what = what).to_string(),
            _ => t!(plain).to_string(),
        };
        let undo_tip = named(ws.doc.undo_label(), "toolbar.undo_named", "toolbar.undo");
        let redo_tip = named(ws.doc.redo_label(), "toolbar.redo_named", "toolbar.redo");
        let model = ws.doc.model();
        let project = ws.file_name().unwrap_or_else(|| {
            model.element(model.root()).map(|e| e.name().to_owned()).unwrap_or_default()
        });
        let recent = self.settings.recent.clone();
        // Menu commands go where shortcuts go, so that this view handles them.
        let focus = self.view_focus(cx);
        let structogram = self.active_structogram(cx).is_some();
        let muted = cx.theme().muted_foreground;
        let divider = || div().w(px(1.0)).h(px(16.0)).mx_1().bg(cx.theme().border);
        // On the window's backdrop, like the cards, with no line of its own.
        TitleBar::new().bg(transparent_black()).border_b_0().child(
            h_flex()
                .flex_1()
                .gap_1()
                .child(img(icons::LOGO).size(px(18.0)).mr_1())
                .child(div().text_sm().font_semibold().child("UmlCraft"))
                .child(div().text_xs().text_color(muted).mr_2().child(env!("CARGO_PKG_VERSION")))
                .child(
                    Button::new("file-menu").ghost().small().label(t!("menu.file")).dropdown_menu(
                        move |menu, window, cx| {
                            file_menu(menu, &recent, structogram, window, cx)
                                .action_context(focus.clone())
                        },
                    ),
                )
                .child(divider())
                .child(shell::in_title_bar(shell::icon_button(
                    "save",
                    icons::fluent("save"),
                    t!("toolbar.save"),
                    Box::new(SaveProject),
                )))
                .child(shell::in_title_bar(
                    shell::icon_button(
                        "undo",
                        icons::fluent("arrow_undo"),
                        undo_tip,
                        Box::new(Undo),
                    )
                    .disabled(!ws.doc.can_undo()),
                ))
                .child(shell::in_title_bar(
                    shell::icon_button(
                        "redo",
                        icons::fluent("arrow_redo"),
                        redo_tip,
                        Box::new(Redo),
                    )
                    .disabled(!ws.doc.can_redo()),
                ))
                .child(divider())
                .child(div().text_sm().ml_1().child(project))
                .when(ws.doc.is_dirty(), |this| {
                    this.child(div().text_sm().text_color(muted).child("•"))
                })
                .child(div().flex_1())
                .child(
                    shell::in_title_bar(shell::icon_button(
                        "settings",
                        icons::fluent("settings"),
                        t!("toolbar.settings"),
                        Box::new(OpenSettings),
                    ))
                    .mr_1(),
                ),
        )
    }

    fn model_browser(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let meta = self.tree_meta.clone();
        let menu_meta = self.tree_meta.clone();
        let muted = cx.theme().muted_foreground;
        let drop_highlight = cx.theme().list_active;
        let workspace = self.workspace.clone();
        let count = self.workspace.read(cx).doc.model().len();
        let header = panel_header(None, t!("panel.model"), Some(count.to_string()), cx);
        v_flex().size_full().child(header).child(
            div().flex_1().min_h_0().py_1().child(
                tree(&self.tree, move |ix, entry, selected, _, _| {
                    let item = entry.item();
                    let meta = meta.get(&item.id);
                    let kind = meta.map(|m| m.kind.type_name()).unwrap_or_default();
                    // Members and other elements without an icon keep the names in line.
                    let icon = match meta.and_then(|m| icons::kind_icon(&m.kind)) {
                        Some(icon) => shell::icon(icon).size_4().into_any_element(),
                        None => div().size_4().into_any_element(),
                    };
                    let mut row = div().id(("tree-row", ix)).w_full().child(
                        h_flex()
                            .gap_2()
                            .overflow_hidden()
                            .child(icon)
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
        let structogram = self.active_structogram(cx);
        let body = if ws.active_diagram.is_some() {
            let zoom = match &structogram {
                Some(view) => view.read(cx).zoom(),
                None => self.canvas.read(cx).zoom(),
            };
            let content = match structogram {
                Some(view) => view.into_any_element(),
                None => self.canvas.clone().into_any_element(),
            };
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(content)
                .child(
                    div()
                        .absolute()
                        .bottom_3()
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(self.zoom_pill(zoom, cx)),
                )
                .into_any_element()
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
        // Nothing of the diagram area draws over the panels beside it.
        v_flex().size_full().overflow_hidden().child(body)
    }

    /// The properties panel under a heading that names what it shows, like "CLASS".
    fn properties_area(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let kind = ws.selected_element.and_then(|id| ws.doc.model().get(id)).map(|e| e.kind());
        let header = match kind {
            Some(kind) => panel_header(
                icons::kind_icon(kind),
                t!(format!("kind.{}", kind.type_name())).to_uppercase(),
                None,
                cx,
            ),
            None => panel_header(None, t!("panel.properties").to_string(), None, cx),
        };
        v_flex().size_full().child(header).child(
            div()
                .id("properties-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(self.properties.clone()),
        )
    }

    /// Zoom out, the zoom (resets it), zoom in and zoom to fit, floating over the canvas.
    fn zoom_pill(&self, zoom: f32, cx: &mut Context<Self>) -> impl IntoElement {
        let zoom = (zoom * 100.0).round();
        shell::card(cx)
            .rounded_full()
            .flex()
            .items_center()
            .gap_0p5()
            .px_1()
            .py_0p5()
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
                t!("toolbar.zoom_fit"),
                Box::new(ZoomToFit),
            ))
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ws = self.workspace.read(cx);
        let state = if ws.doc.is_dirty() { t!("status.modified") } else { t!("status.saved") };
        let selection = match ws.selection.len() {
            0 => String::new(),
            n => format!("{n} {}", t!("status.selected")),
        };
        let notice = self.notice.as_ref().filter(|(_, r)| *r == ws.doc.revision());
        let right = match &ws.message {
            _ if self.busy == Some(Busy::Opening) => div().child(t!("status.opening").to_string()),
            _ if self.busy == Some(Busy::ExportingAstah) => {
                let cancel = self.cancel_export.clone();
                h_flex().gap_2().child(t!("status.exporting").to_string()).child(
                    Button::new("cancel-export")
                        .ghost()
                        .xsmall()
                        .label(t!("status.cancel").to_string())
                        .on_click(move |_, _, _| {
                            if let Some(cancel) = &cancel {
                                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }),
                )
            }
            _ if self.busy == Some(Busy::ImportingSource) => {
                div().child(t!("status.importing").to_string())
            }
            _ if self.busy == Some(Busy::ImportingAstah) => {
                div().child(t!("status.importing_astah").to_string())
            }
            Some(message) => div().text_color(cx.theme().danger).child(message.clone()),
            None if notice.is_some() => {
                div().child(notice.map(|(n, _)| n.clone()).unwrap_or_default())
            }
            None => div().child(match ws.tool {
                Tool::Select => t!("status.hint").to_string(),
                Tool::Node(_) | Tool::Text => t!("status.hint_node").to_string(),
                Tool::Line => t!("status.hint_line").to_string(),
                Tool::Edge(_) => t!("status.hint_edge").to_string(),
            }),
        };
        h_flex()
            .gap_4()
            .px(shell::GAP + px(4.0))
            .pb_1()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(ws.file_name().unwrap_or_else(|| t!("status.unsaved_file").to_string()))
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
    if meta.kind.as_diagram().is_some() || matches!(meta.kind, ElementKind::Structogram(_)) {
        menu = menu.menu(t!("menu.open_diagram"), command(TreeCommandKind::Open)).separator();
    }
    let items = [
        NewItem::Element(NodeTool::Package),
        NewItem::Element(NodeTool::Subsystem),
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
        NewItem::TemplateParameter,
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
        let mut items = vec![NewItem::Diagram(kind)];
        if kind == DiagramKind::Class {
            items.push(NewItem::ObjectDiagram);
        }
        for item in items {
            if item.applies_to(&meta.kind) {
                let key = item.key();
                let label = format!("{} {}", t!("menu.new_prefix"), t!(key.as_str()));
                menu = menu.menu(label, command(TreeCommandKind::New(item)));
                any = true;
            }
        }
    }
    if NewItem::Structogram.applies_to(&meta.kind) {
        let label = format!("{} {}", t!("menu.new_prefix"), t!("kind.structogram"));
        menu = menu.menu(label, command(TreeCommandKind::New(NewItem::Structogram)));
        any = true;
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
    let children: Vec<TreeItem> = labels::tree_children(model, e)
        .into_iter()
        .map(|c| tree_item(model, c, collapsed, meta))
        .collect();
    TreeItem::new(key.clone(), labels::tree_name(model, e))
        .expanded(!collapsed.contains(&key))
        .children(children)
}

/// One of the three panels as a card, inset from the panel's edges so that the cards keep a gap
/// between them where the split handles are.
fn panel_card(content: impl IntoElement, cx: &App) -> impl IntoElement {
    div()
        .size_full()
        .px(shell::GAP / 2.0)
        .child(shell::card(cx).size_full().overflow_hidden().child(content))
}

/// A card's heading: an optional icon, the title and, muted, a detail such as a count.
fn panel_header(
    icon: Option<SharedString>,
    title: impl Into<SharedString>,
    detail: Option<String>,
    cx: &App,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    h_flex()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(cx.theme().border)
        .when_some(icon, |this, icon| this.child(shell::icon(icon).size_4().text_color(muted)))
        .child(div().text_sm().font_semibold().child(title.into()))
        .child(div().flex_1())
        .when_some(detail, |this, detail| {
            this.child(div().text_xs().text_color(muted).child(detail))
        })
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
        menu_item("menu.new_structogram", NewStructogram),
        MenuItem::submenu(Menu::new(t!("menu.recent").to_string()).items(recent)),
        menu_item("menu.import_astah", ImportAstah),
        menu_item("menu.import_structogram", ImportStructogram),
    ];
    file.push(MenuItem::submenu(Menu::new(t!("menu.export").to_string()).items([
        menu_item("menu.export_svg", ExportSvg),
        menu_item("menu.export_png", ExportPng),
        menu_item("menu.export_astah", ExportAstah),
        menu_item("menu.export_structogram", crate::nsd::ExportStructogram),
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
            menu_item("menu.copy_image", CopyImage),
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
        Menu::new(t!("menu.tools").to_string()).items([
            MenuItem::submenu(Menu::new(t!("menu.java").to_string()).items([
                menu_item(
                    "menu.import_java_files",
                    ImportSource { language: SourceLanguage::Java, folder: false },
                ),
                menu_item(
                    "menu.import_java_folder",
                    ImportSource { language: SourceLanguage::Java, folder: true },
                ),
            ])),
            MenuItem::submenu(Menu::new(t!("menu.csharp").to_string()).items([
                menu_item(
                    "menu.import_csharp_files",
                    ImportSource { language: SourceLanguage::CSharp, folder: false },
                ),
                menu_item(
                    "menu.import_csharp_folder",
                    ImportSource { language: SourceLanguage::CSharp, folder: true },
                ),
            ])),
            MenuItem::submenu(Menu::new(t!("menu.cpp").to_string()).items([
                menu_item(
                    "menu.import_cpp_files",
                    ImportSource { language: SourceLanguage::Cpp, folder: false },
                ),
                menu_item(
                    "menu.import_cpp_folder",
                    ImportSource { language: SourceLanguage::Cpp, folder: true },
                ),
            ])),
            MenuItem::submenu(Menu::new(t!("menu.python").to_string()).items([
                menu_item(
                    "menu.import_python_files",
                    ImportSource { language: SourceLanguage::Python, folder: false },
                ),
                menu_item(
                    "menu.import_python_folder",
                    ImportSource { language: SourceLanguage::Python, folder: true },
                ),
            ])),
            MenuItem::separator(),
            menu_item("menu.settings", OpenSettings),
        ]),
        Menu::new(t!("menu.help").to_string()).items([menu_item("menu.open_sample", OpenSample)]),
    ]
}

/// The platform's menu bar (macOS); in the window, the title bar's File menu and the ribbon
/// take its place.
fn install_menus(settings: &Settings, cx: &mut App) {
    cx.set_menus(menus(settings));
}

/// The title bar's File menu: the same entries as the platform's File menu. Exporting a
/// structogram needs one to be shown.
fn file_menu(
    menu: PopupMenu,
    recent: &[PathBuf],
    structogram: bool,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let recent = recent.to_vec();
    let item = |menu: PopupMenu, key: &str, action: Box<dyn Action>| menu.menu(t!(key), action);
    let mut menu = item(menu, "menu.new", Box::new(NewProject));
    menu = item(menu, "menu.open", Box::new(OpenProject));
    menu = item(menu, "menu.new_structogram", Box::new(NewStructogram));
    menu = menu.submenu(t!("menu.recent"), window, cx, move |mut menu, _, _| {
        if recent.is_empty() {
            return menu.menu_with_disabled(t!("menu.no_recent"), Box::new(Cancel), true);
        }
        for (index, path) in recent.iter().enumerate() {
            menu = menu.menu(recent_label(path), Box::new(OpenRecent { index }));
        }
        menu
    });
    menu = item(menu, "menu.open_sample", Box::new(OpenSample));
    menu = item(menu, "menu.import_astah", Box::new(ImportAstah));
    menu = item(menu, "menu.import_structogram", Box::new(ImportStructogram));
    menu = menu.submenu(t!("menu.export"), window, cx, move |menu, _, _| {
        menu.menu(t!("menu.export_svg"), Box::new(ExportSvg))
            .menu(t!("menu.export_png"), Box::new(ExportPng))
            .menu(t!("menu.export_astah"), Box::new(ExportAstah))
            .menu_with_disabled(
                t!("menu.export_structogram"),
                Box::new(crate::nsd::ExportStructogram),
                !structogram,
            )
    });
    menu = item(menu.separator(), "menu.save", Box::new(SaveProject));
    menu = item(menu, "menu.save_as", Box::new(SaveProjectAs));
    item(menu.separator(), "menu.quit", Box::new(Quit))
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

pub(crate) fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
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

pub(crate) fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || " -_".contains(c) { c } else { '_' })
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() { "project".to_owned() } else { cleaned.to_owned() }
}

/// Where file dialogs start: next to the project file, else in the user's documents.
pub(crate) fn start_directory(project: Option<&Path>) -> PathBuf {
    project
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .or_else(documents_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn documents_dir() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?);
    let documents = home.join("Documents");
    Some(if documents.is_dir() { documents } else { home })
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Actions start at the focused element; with nothing focused (at start, or after a
        // menu or dialog closed) the toolbar and shortcuts would not reach this view.
        if window.focused(cx).is_none() {
            self.view_focus(cx).focus(window, cx);
        }
        let backdrop = shell::backdrop(cx);
        let model_browser = self.model_browser(cx).into_any_element();
        let diagram_area = self.diagram_area(cx).into_any_element();
        let properties_area = self.properties_area(cx).into_any_element();
        let ribbon = self.ribbon(window, cx).into_any_element();
        let theme = cx.theme();
        let canvas = self.canvas.clone();
        let (c1, c2, c3, c4) = (canvas.clone(), canvas.clone(), canvas.clone(), canvas.clone());
        v_flex()
            .id("main")
            .key_context("UmlCraft")
            .size_full()
            .bg(backdrop)
            .text_color(theme.foreground)
            .on_action(cx.listener(Self::new_project))
            .on_action(cx.listener(Self::open_project))
            .on_action(cx.listener(Self::new_structogram))
            .on_action(cx.listener(Self::import_structogram))
            .on_action(cx.listener(Self::import_astah))
            .on_action(cx.listener(Self::export_astah))
            .on_action(cx.listener(Self::import_source))
            .on_action(cx.listener(Self::export_svg))
            .on_action(cx.listener(Self::export_png))
            .on_action(cx.listener(Self::copy_image))
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
            .on_action(cx.listener(Self::open_settings))
            .on_action(cx.listener(Self::select_tool))
            .on_action(cx.listener(Self::new_diagram))
            .on_action(cx.listener(Self::select_ribbon_tab))
            .on_action(
                cx.listener(|this, action: &ToggleRibbon, _, cx| this.toggle_ribbon(action, cx)),
            )
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
            // Zoom commands from outside the view (the tree has the focus) go to what is shown.
            .on_action(cx.listener(
                move |this, _: &ZoomIn, _, cx| match this.active_structogram(cx) {
                    Some(view) => view.update(cx, |v, cx| v.zoom_by(1.25, cx)),
                    None => c1.update(cx, |c, cx| c.zoom_by(1.2, cx)),
                },
            ))
            .on_action(cx.listener(
                move |this, _: &ZoomOut, _, cx| match this.active_structogram(cx) {
                    Some(view) => view.update(cx, |v, cx| v.zoom_by(1.0 / 1.25, cx)),
                    None => c2.update(cx, |c, cx| c.zoom_by(1.0 / 1.2, cx)),
                },
            ))
            .on_action(cx.listener(move |this, _: &ResetZoom, _, cx| {
                match this.active_structogram(cx) {
                    Some(view) => view.update(cx, |v, cx| v.reset_zoom(cx)),
                    None => c3.update(cx, |c, cx| c.reset_zoom(cx)),
                }
            }))
            .on_action(move |a: &SetLineShape, _, cx| {
                c4.update(cx, |c, cx| c.set_line_shape(a.shape, cx))
            })
            .on_action(cx.listener(|this, a: &Arrange, _, cx| {
                let direction = a.direction;
                this.with_workspace(cx, move |ws| ws.arrange(direction))
            }))
            .on_action(cx.listener(|this, _: &RouteLines, _, cx| {
                this.with_workspace(cx, Workspace::route_lines)
            }))
            .on_action(cx.listener(|this, _: &ToggleMessageNumbers, _, cx| {
                this.with_workspace(cx, |ws| ws.toggle_numbering(false))
            }))
            .on_action(cx.listener(|this, _: &ToggleFlatNumbers, _, cx| {
                this.with_workspace(cx, |ws| ws.toggle_numbering(true))
            }))
            .on_action(cx.listener(move |this, _: &ZoomToFit, _, cx| {
                match this.active_structogram(cx) {
                    Some(view) => view.update(cx, |v, cx| v.zoom_to_fit(cx)),
                    None => canvas.update(cx, |c, cx| c.zoom_to_fit(cx)),
                }
            }))
            .child(self.title_bar(cx))
            .child(ribbon)
            .child(
                div().flex_1().min_h_0().px(shell::GAP / 2.0).py(shell::GAP).child(
                    h_resizable("main-split")
                        .child(
                            resizable_panel()
                                .size(px(260.0))
                                .size_range(px(160.0)..px(520.0))
                                .child(panel_card(model_browser, cx)),
                        )
                        .child(resizable_panel().child(panel_card(diagram_area, cx)))
                        .child(
                            resizable_panel()
                                .size(px(300.0))
                                .size_range(px(180.0)..px(560.0))
                                .child(panel_card(properties_area, cx)),
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

    #[cfg(not(windows))]
    #[test]
    fn recent_labels_put_the_file_name_first_and_shorten_folders() {
        use super::recent_label;

        let short = PathBuf::from("/home/ulf/shop.ucraft");
        assert_eq!(recent_label(&short), "shop.ucraft — /home/ulf");
        let long = PathBuf::from("/home/ulf/projects/teaching/2026/autumn/uml/examples/m2.ucraft");
        let label = recent_label(&long);
        assert!(label.starts_with("m2.ucraft — /home/ulf"), "{label}");
        assert!(label.ends_with("uml/examples"), "{label}");
        assert!(label.chars().count() <= "m2.ucraft — ".len() + 40, "{label}");
    }
}
