#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas;
mod labels;
mod main_view;
mod properties;
mod settings;
mod workspace;

use gpui_kit::*;
use umlcraft_model::Id;

use crate::workspace::NewItem;

rust_i18n::i18n!("../../locales", fallback = "en");

actions!(
    umlcraft,
    [
        NewProject,
        OpenProject,
        ImportAstah,
        ExportAstah,
        ExportSvg,
        SaveProject,
        SaveProjectAs,
        Quit,
        Undo,
        Redo,
        DeleteSelection,
        RemoveFromDiagram,
        SelectAll,
        Cancel,
        ResetZoom,
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        AlignLeft,
        AlignCenter,
        AlignRight,
        AlignTop,
        AlignMiddle,
        AlignBottom,
        ToggleTheme,
        ToggleLanguage,
        OpenSample,
        Copy,
        Cut,
        Paste,
        PasteAsReference,
        WrapInFragment,
    ]
);

/// Opens an entry of the "recent files" list.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct OpenRecent {
    pub index: usize,
}

/// Commands of the model tree's context menu.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct TreeCommand {
    pub target: Id,
    pub command: TreeCommandKind,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TreeCommandKind {
    New(NewItem),
    Open,
    Rename,
    Delete,
}

fn main() {
    let settings = settings::Settings::load();
    let language = settings.language.clone().unwrap_or_else(|| {
        let system = sys_locale::get_locale().unwrap_or_default();
        if system.starts_with("de") { "de".into() } else { "en".into() }
    });
    rust_i18n::set_locale(&language);

    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("ctrl-n", NewProject, None),
            KeyBinding::new("ctrl-o", OpenProject, None),
            KeyBinding::new("ctrl-s", SaveProject, None),
            KeyBinding::new("ctrl-shift-s", SaveProjectAs, None),
            KeyBinding::new("ctrl-z", Undo, None),
            KeyBinding::new("ctrl-y", Redo, None),
            KeyBinding::new("ctrl-shift-z", Redo, None),
            KeyBinding::new("delete", DeleteSelection, Some("Canvas")),
            KeyBinding::new("ctrl-delete", RemoveFromDiagram, Some("Canvas")),
            KeyBinding::new("ctrl-a", SelectAll, Some("Canvas")),
            KeyBinding::new("ctrl-c", Copy, Some("Canvas")),
            KeyBinding::new("ctrl-x", Cut, Some("Canvas")),
            KeyBinding::new("ctrl-v", Paste, Some("Canvas")),
            KeyBinding::new("ctrl-shift-v", PasteAsReference, Some("Canvas")),
            KeyBinding::new("escape", Cancel, Some("Canvas")),
            KeyBinding::new("ctrl-g", WrapInFragment, Some("Canvas")),
            KeyBinding::new("ctrl-0", ResetZoom, None),
            KeyBinding::new("ctrl-=", ZoomIn, None),
            KeyBinding::new("ctrl--", ZoomOut, None),
            KeyBinding::new("ctrl-shift-0", ZoomToFit, None),
        ]);

        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("UmlCraft".into()),
                    ..Default::default()
                }),
                window_min_size: Some(size(px(800.0), px(520.0))),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    let mut view = main_view::MainView::new(settings, window, cx);
                    // A file given on the command line, e.g. by "Open with".
                    if let Some(path) = std::env::args_os().nth(1) {
                        view.open(path.into(), window, cx);
                    }
                    view
                })
            },
        )
        .expect("failed to open the main window");
        cx.activate(true);
    });
}
