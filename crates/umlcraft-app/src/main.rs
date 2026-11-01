#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod about_dialog;
mod canvas;
mod demo;
mod errors;
mod icons;
mod labels;
mod live;
mod main_view;
mod nsd;
mod png;
mod properties;
mod ribbon;
mod settings;
mod settings_dialog;
mod shell;
mod workspace;

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::*;
use umlcraft_model::{Id, LineShape, TypeRef};
use umlcraft_notation::layout::Direction;
use umlcraft_notation::palette::EdgeTool;
use umlcraft_reverse::SourceLanguage;

use crate::workspace::{EndChange, NewItem};

rust_i18n::i18n!("../../locales", fallback = "en");

actions!(
    umlcraft,
    [
        NewProject,
        OpenProject,
        ImportStructogram,
        NewStructogram,
        ImportAstah,
        ExportAstah,
        ExportSvg,
        ExportPng,
        // Puts the open diagram on the clipboard as SVG and PNG.
        CopyImage,
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
        OpenSettings,
        OpenSample,
        OpenAbout,
        Copy,
        Cut,
        Paste,
        PasteAsReference,
        WrapInFragment,
        ResetLineEnds,
        ResetLabels,
        // Routes the selected lines, or all of the diagram, around the nodes.
        RouteLines,
        // Sequence diagrams: shows or hides the message numbers, numbers them 1, 2, 3 or 2.1.
        ToggleMessageNumbers,
        ToggleFlatNumbers,
        AddExtensionPoint,
        // An empty row at the end of the selected class's attributes, operations or literals.
        AddAttribute,
        AddOperation,
        AddLiteral,
        // Folds the ribbon to its tabs, or unfolds it.
        ToggleRibbon,
    ]
);

/// Changes one end of an association, from the canvas's context menu.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct SetAssociationEnd {
    pub association: Id,
    pub end: usize,
    pub change: EndChange,
}

/// Opens an entry of the "recent files" list.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct OpenRecent {
    pub index: usize,
}

/// Gives an attribute its type or an operation its return type, from the canvas's context
/// menu.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct SetMemberType {
    pub member: Id,
    pub type_ref: TypeRef,
}

/// Deletes the member right-clicked on the canvas.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct DeleteMember {
    pub member: Id,
}

/// Connects `source` to `target` with a line of `tool`, from the menu a line dragged from a
/// handle offers; a message goes where the drag started (`start`).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct ConnectAs {
    pub tool: EdgeTool,
    pub source: Id,
    pub target: Id,
    pub start: umlcraft_model::Point,
}

/// Turns the selected relationship into another kind, from the canvas's context menu.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct ChangeRelationship {
    pub edge: Id,
    pub tool: EdgeTool,
}

/// Gives the selected edges a line style.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct SetLineShape {
    pub shape: LineShape,
}

/// Arranges the active diagram, or the selection on it, in layers.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct Arrange {
    pub direction: Direction,
}

/// Imports source files, or a folder of them, into the open project (Tools menu).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct ImportSource {
    pub language: SourceLanguage,
    /// Ask for a folder, searched recursively, instead of files.
    pub folder: bool,
}

/// Chooses a diagram tool, from the ribbon.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct SelectTool {
    pub tool: umlcraft_notation::palette::Tool,
}

/// Creates a diagram in the selected package, or else in the model, and opens it.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct NewDiagram {
    pub kind: umlcraft_model::DiagramKind,
    /// A class diagram drawn as an object diagram.
    pub objects: bool,
}

/// Opens a tab of the ribbon.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = umlcraft, no_json)]
pub struct SelectRibbonTab {
    pub tab: ribbon::RibbonTab,
}

/// The line styles in menu order, with their labels.
pub const LINE_SHAPES: [(LineShape, &str); 4] = [
    (LineShape::Line, "menu.line_straight"),
    (LineShape::RightAngle, "menu.line_right_angle"),
    (LineShape::Curve, "menu.line_curve"),
    (LineShape::CurveRightAngle, "menu.line_curve_right_angle"),
];

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

/// A key binding; `secondary-` is Cmd on macOS and Ctrl elsewhere, which `KeyBinding::new` no
/// longer resolves.
pub(crate) fn key<A: Action>(cx: &App, keys: &str, action: A, context: Option<&str>) -> KeyBinding {
    let context = context.map(|c| Rc::new(KeyBindingContextPredicate::parse(c).expect(c)));
    KeyBinding::load(keys, Box::new(action), context, false, None, cx).expect(keys)
}

fn main() {
    let settings = settings::Settings::load();
    rust_i18n::set_locale(&settings.ui_language());

    let app = gpui_kit::application().with_assets(icons::AppAssets);
    // Files the system asks to open while UmlCraft runs (on macOS also at launch, instead of
    // the command line), picked up by the main view.
    let requested: Rc<RefCell<Vec<PathBuf>>> = Rc::default();
    let sink = requested.clone();
    app.on_open_urls(move |urls| {
        sink.borrow_mut().extend(urls.iter().filter_map(|u| file_path(u)))
    });
    app.run(move |cx| {
        gpui_kit::init(cx);
        // "secondary" is Cmd on macOS and Ctrl elsewhere (`key`).
        cx.bind_keys([
            key(cx, "secondary-n", NewProject, None),
            key(cx, "secondary-o", OpenProject, None),
            key(cx, "secondary-s", SaveProject, None),
            key(cx, "secondary-shift-s", SaveProjectAs, None),
            key(cx, "secondary-z", Undo, None),
            key(cx, "secondary-y", Redo, None),
            key(cx, "secondary-shift-z", Redo, None),
            // Fields bind only Ctrl+Y for their own redo there; without this, Ctrl+Shift+Z in a
            // field would redo a model change and reload the field being typed in.
            #[cfg(not(target_os = "macos"))]
            key(cx, "ctrl-shift-z", gpui_kit::component::input::Redo, Some("Input")),
            key(cx, "delete", DeleteSelection, Some("Canvas")),
            key(cx, "secondary-delete", RemoveFromDiagram, Some("Canvas")),
            key(cx, "secondary-a", SelectAll, Some("Canvas")),
            key(cx, "secondary-c", Copy, Some("Canvas")),
            key(cx, "secondary-shift-c", CopyImage, None),
            key(cx, "secondary-x", Cut, Some("Canvas")),
            key(cx, "secondary-v", Paste, Some("Canvas")),
            key(cx, "secondary-shift-v", PasteAsReference, Some("Canvas")),
            key(cx, "escape", Cancel, Some("Canvas")),
            key(cx, "secondary-g", WrapInFragment, Some("Canvas")),
            // Astah's shortcuts for the line styles.
            key(cx, "secondary-l", SetLineShape { shape: LineShape::Line }, Some("Canvas")),
            key(cx, "secondary-e", SetLineShape { shape: LineShape::RightAngle }, Some("Canvas")),
            key(cx, "secondary-shift-l", Arrange { direction: Direction::Down }, Some("Canvas")),
            key(cx, "secondary-shift-r", RouteLines, Some("Canvas")),
            key(cx, "secondary-0", ResetZoom, None),
            key(cx, "secondary-=", ZoomIn, None),
            key(cx, "secondary--", ZoomOut, None),
            key(cx, "secondary-shift-0", ZoomToFit, None),
            key(cx, "secondary-,", OpenSettings, None),
        ]);
        nsd::bind_keys(cx);

        let bounds = Bounds::centered(None, size(px(1280.0), px(820.0)), cx);
        gpui_kit::open_window(
            // The main view draws the title bar itself (`TitleBar`), with the window's controls.
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(800.0), px(520.0))),
                // Matches packaging/linux/com.umlcraft.app.desktop, so that Wayland
                // desktops show the window with the app's icon.
                app_id: Some("com.umlcraft.app".into()),
                window_decorations: cfg!(target_os = "linux").then_some(WindowDecorations::Client),
                ..gpui_kit::component::TitleBar::window_options()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    let mut view = main_view::MainView::new(settings, window, cx);
                    // A file given on the command line, e.g. by "Open with".
                    if let Some(path) = std::env::args_os().nth(1) {
                        view.open(path.into(), window, cx);
                    }
                    cx.spawn_in(window, async move |this, cx| {
                        loop {
                            cx.background_executor().timer(Duration::from_millis(250)).await;
                            let Some(path) = requested.borrow_mut().pop() else { continue };
                            let opened = this.update_in(cx, |view, window, cx| {
                                view.ask_to_open(path, window, cx)
                            });
                            if opened.is_err() {
                                break;
                            }
                        }
                    })
                    .detach();
                    view
                })
            },
        )
        .expect("failed to open the main window");
        cx.activate(true);
    });
}

/// The local path of a `file://` URL, with `%XX` escapes decoded.
fn file_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let bytes = rest.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (
            bytes[i],
            bytes.get(i + 1).copied().and_then(hex),
            bytes.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(h), Some(l)) => {
                decoded.push((h * 16 + l) as u8);
                i += 3;
            }
            (b, _, _) => {
                decoded.push(b);
                i += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8(decoded).ok()?))
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_urls_become_paths() {
        use super::file_path;
        use std::path::PathBuf;

        assert_eq!(
            file_path("file:///Users/ulf/My%20Shop.ucraft"),
            Some(PathBuf::from("/Users/ulf/My Shop.ucraft"))
        );
        assert_eq!(
            file_path("file:///tmp/%C3%9Cbung.asta"),
            Some(PathBuf::from("/tmp/Übung.asta"))
        );
        assert_eq!(file_path("https://example.com/a.ucraft"), None);
    }
}
