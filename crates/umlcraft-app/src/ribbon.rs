//! The ribbon: tabs with labelled groups of large and small buttons, floating above the
//! panels. Tabs, groups and buttons are data (`tabs`, `groups`), built on every render from a
//! [`State`]; [`render`] draws them. Every button dispatches an action, so the ribbon holds no
//! state of its own besides the open tab, which the main view keeps.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, StyledExt, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::DiagramKind;
use umlcraft_notation::layout::{self, Direction};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_reverse::SourceLanguage;

use crate::icons::{self, fluent};
use crate::shell;
use crate::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RibbonTab {
    #[default]
    Home,
    /// The tools of the active diagram's kind, or the elements of the structogram shown;
    /// there only while one is open.
    Diagram,
    View,
    Tools,
}

/// What the buttons show, read once per render from the workspace.
pub struct State {
    pub diagram: Option<DiagramKind>,
    /// Whether the class diagram shown is an object diagram.
    pub objects: bool,
    /// What the selection of the structogram shown allows, if one is shown.
    pub structogram: Option<crate::nsd::Allowed>,
    pub tool: Tool,
    pub has_selection: bool,
    pub can_align: bool,
    pub can_paste: bool,
    pub can_wrap: bool,
    /// On a sequence diagram: whether it shows message numbers, and whether flat ones.
    pub numbering: Option<(bool, bool)>,
    /// The chosen theme; `None` follows the system's appearance.
    pub theme: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Icon above the label, the full height of the ribbon.
    Large,
    /// Icon beside the label, stacked three to a column.
    Small,
    /// Icon only, stacked like small buttons; the label is the tooltip.
    Icon,
}

/// A line of a button's menu.
pub enum MenuEntry {
    Command {
        label: String,
        icon: Option<SharedString>,
        action: Box<dyn Action>,
    },
    /// One of a choice, checked if it is the current one.
    Check {
        label: String,
        checked: bool,
        action: Box<dyn Action>,
    },
    Separator,
}

impl Clone for MenuEntry {
    fn clone(&self) -> Self {
        match self {
            Self::Command { label, icon, action } => Self::Command {
                label: label.clone(),
                icon: icon.clone(),
                action: action.boxed_clone(),
            },
            Self::Check { label, checked, action } => Self::Check {
                label: label.clone(),
                checked: *checked,
                action: action.boxed_clone(),
            },
            Self::Separator => Self::Separator,
        }
    }
}

pub struct Item {
    pub id: SharedString,
    pub icon: SharedString,
    pub label: String,
    /// Shown on hover; without one, the label.
    pub tooltip: Option<String>,
    pub size: Size,
    /// What a click does; without one, a click opens `menu`.
    pub action: Option<Box<dyn Action>>,
    /// More commands; with an action as well, the button is split in two.
    pub menu: Vec<MenuEntry>,
    pub enabled: bool,
    pub selected: bool,
}

impl Item {
    fn new(id: impl Into<SharedString>, icon: SharedString, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            icon,
            label: label.into(),
            tooltip: None,
            size: Size::Large,
            action: None,
            menu: Vec::new(),
            enabled: true,
            selected: false,
        }
    }

    fn action(mut self, action: impl Action) -> Self {
        self.action = Some(Box::new(action));
        self
    }

    fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    fn menu_entry(mut self, label: impl Into<String>, action: impl Action) -> Self {
        self.menu.push(MenuEntry::Command {
            label: label.into(),
            icon: None,
            action: Box::new(action),
        });
        self
    }

    fn menu_icon_entry(
        mut self,
        label: impl Into<String>,
        icon: SharedString,
        action: impl Action,
    ) -> Self {
        self.menu.push(MenuEntry::Command {
            label: label.into(),
            icon: Some(icon),
            action: Box::new(action),
        });
        self
    }

    fn menu_check_entry(
        mut self,
        label: impl Into<String>,
        checked: bool,
        action: impl Action,
    ) -> Self {
        self.menu.push(MenuEntry::Check { label: label.into(), checked, action: Box::new(action) });
        self
    }

    fn menu_separator(mut self) -> Self {
        self.menu.push(MenuEntry::Separator);
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

pub struct Group {
    pub id: &'static str,
    pub caption: String,
    pub items: Vec<Item>,
}

fn group(id: &'static str, items: Vec<Item>) -> Group {
    Group { id, caption: t!(format!("ribbon.group.{id}")).to_string(), items }
}

/// The tabs in order, with their labels; the diagram tab is named after the diagram's kind.
pub fn tabs(state: &State) -> Vec<(RibbonTab, String)> {
    let mut tabs = vec![(RibbonTab::Home, t!("ribbon.tab.home").to_string())];
    if let Some(kind) = state.diagram {
        let key = match state.objects {
            true => crate::workspace::NewItem::ObjectDiagram.key(),
            false => diagram_key(kind),
        };
        tabs.push((RibbonTab::Diagram, t!(key).to_string()));
    } else if state.structogram.is_some() {
        tabs.push((RibbonTab::Diagram, t!("kind.structogram").to_string()));
    }
    tabs.push((RibbonTab::View, t!("ribbon.tab.view").to_string()));
    tabs.push((RibbonTab::Tools, t!("ribbon.tab.tools").to_string()));
    tabs
}

pub fn diagram_key(kind: DiagramKind) -> String {
    crate::workspace::NewItem::Diagram(kind).key()
}

pub const DIAGRAM_KINDS: [DiagramKind; 7] = [
    DiagramKind::Class,
    DiagramKind::UseCase,
    DiagramKind::Sequence,
    DiagramKind::Activity,
    DiagramKind::StateMachine,
    DiagramKind::Component,
    DiagramKind::Deployment,
];

pub fn groups(tab: RibbonTab, state: &State) -> Vec<Group> {
    match tab {
        RibbonTab::Home => home(state),
        RibbonTab::Diagram => match (state.diagram, state.structogram) {
            (Some(kind), _) => diagram(kind, state.objects, state.tool, state.numbering),
            (None, Some(allowed)) => structogram(allowed),
            (None, None) => Vec::new(),
        },
        RibbonTab::View => view(state),
        RibbonTab::Tools => tools(state),
    }
}

fn home(state: &State) -> Vec<Group> {
    let selected = state.has_selection;
    let aligns = [
        ("align-left", "align_left", "menu.align_left", AlignLeft.boxed_clone()),
        ("align-center", "align_center_horizontal", "menu.align_center", AlignCenter.boxed_clone()),
        ("align-right", "align_right", "menu.align_right", AlignRight.boxed_clone()),
        ("align-top", "align_top", "menu.align_top", AlignTop.boxed_clone()),
        ("align-middle", "align_center_vertical", "menu.align_middle", AlignMiddle.boxed_clone()),
        ("align-bottom", "align_bottom", "menu.align_bottom", AlignBottom.boxed_clone()),
    ]
    .map(|(id, icon, label, action)| Item {
        action: Some(action),
        ..Item::new(id, fluent(icon), t!(label)).size(Size::Icon).enabled(state.can_align)
    });
    let mut line_style = Item::new("line-style", fluent("line_style"), t!("menu.line_style"))
        .size(Size::Small)
        .enabled(selected);
    for (shape, key) in LINE_SHAPES {
        line_style = line_style.menu_entry(t!(key), SetLineShape { shape });
    }
    vec![
        group("new", vec![new_diagram()]),
        group(
            "clipboard",
            vec![
                Item::new("paste", fluent("clipboard_paste"), t!("ribbon.paste"))
                    .action(Paste)
                    .menu_entry(t!("menu.paste_reference"), PasteAsReference)
                    .enabled(state.can_paste),
                Item::new("cut", fluent("cut"), t!("ribbon.cut"))
                    .action(Cut)
                    .size(Size::Small)
                    .enabled(selected),
                Item::new("copy", fluent("copy"), t!("ribbon.copy"))
                    .action(Copy)
                    .size(Size::Small)
                    .enabled(selected),
            ],
        ),
        group(
            "edit",
            vec![
                Item::new("delete", fluent("delete"), t!("ribbon.delete"))
                    .action(DeleteSelection)
                    .enabled(selected),
                Item::new("remove", fluent("dismiss"), t!("ribbon.remove"))
                    .action(RemoveFromDiagram)
                    .size(Size::Small)
                    .enabled(selected),
                Item::new("select-all", fluent("select_all_on"), t!("ribbon.select_all"))
                    .action(SelectAll)
                    .size(Size::Small)
                    .enabled(state.diagram.is_some()),
            ],
        ),
        group(
            "arrange",
            aligns
                .into_iter()
                .chain([
                    line_style,
                    Item::new("wrap", fluent("group"), t!("ribbon.wrap_in_fragment"))
                        .action(WrapInFragment)
                        .size(Size::Small)
                        .enabled(state.can_wrap),
                ])
                .collect(),
        ),
        group(
            "layout",
            vec![
                Item::new("auto-arrange", fluent("flowchart"), t!("ribbon.auto_arrange"))
                    .action(Arrange { direction: None })
                    .menu_entry(
                        t!("menu.arrange_down"),
                        Arrange { direction: Some(Direction::Down) },
                    )
                    .menu_entry(
                        t!("menu.arrange_right"),
                        Arrange { direction: Some(Direction::Right) },
                    )
                    .enabled(state.diagram.is_some_and(layout::can_arrange)),
                Item::new("route-lines", fluent("arrow_routing"), t!("ribbon.route_lines"))
                    .action(RouteLines)
                    .size(Size::Small)
                    .enabled(state.diagram.is_some_and(|k| k != DiagramKind::Sequence)),
            ],
        ),
    ]
}

/// A button whose menu creates a diagram of the chosen kind, or a structogram.
fn new_diagram() -> Item {
    let mut item = Item::new("new-diagram", fluent("document_add"), t!("ribbon.new_diagram"));
    for kind in DIAGRAM_KINDS {
        item = item.menu_icon_entry(
            t!(diagram_key(kind)),
            icons::diagram_icon(kind),
            NewDiagram { kind, objects: false },
        );
        if kind == DiagramKind::Class {
            item = item.menu_icon_entry(
                t!("diagram.object"),
                icons::object_diagram_icon(),
                NewDiagram { kind, objects: true },
            );
        }
    }
    item.menu_separator().menu_icon_entry(
        t!("kind.structogram"),
        icons::structogram_icon(),
        NewStructogram,
    )
}

/// Which group of the diagram tab a tool belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolGroup {
    Select,
    Elements,
    Relations,
    Notes,
}

pub fn tool_group(tool: Tool) -> ToolGroup {
    match tool {
        Tool::Select => ToolGroup::Select,
        Tool::Node(NodeTool::Note) | Tool::Edge(EdgeTool::NoteAnchor) | Tool::Text | Tool::Line => {
            ToolGroup::Notes
        }
        Tool::Node(_) => ToolGroup::Elements,
        Tool::Edge(_) => ToolGroup::Relations,
    }
}

pub fn tool_label(tool: Tool) -> String {
    match tool {
        Tool::Node(NodeTool::Note) => t!("tool.note").to_string(),
        _ => t!(format!("tool.{}", tool.key())).to_string(),
    }
}

fn diagram(
    kind: DiagramKind,
    objects: bool,
    current: Tool,
    numbering: Option<(bool, bool)>,
) -> Vec<Group> {
    let tools = match objects {
        true => palette::object_palette(),
        false => palette::palette(kind),
    };
    let items = |which: ToolGroup, size: Size| -> Vec<Item> {
        tools
            .iter()
            .filter(|&&tool| tool_group(tool) == which)
            .map(|&tool| {
                let item = Item::new(
                    format!("tool-{}", tool.key()),
                    icons::tool_icon(tool),
                    tool_label(tool),
                )
                .action(SelectTool { tool })
                .size(size)
                .selected(tool == current);
                // Tools whose name alone does not say how they are used.
                match tool {
                    Tool::Edge(EdgeTool::FoundMessage | EdgeTool::LostMessage) => {
                        item.tooltip(t!(format!("tool_tip.{}", tool.key())))
                    }
                    _ => item,
                }
            })
            .collect()
    };
    let numbering = numbering.map_or_else(Vec::new, |(shown, flat)| {
        vec![
            Item::new(
                "message-numbers",
                icons::own_icon("messageNumbers"),
                t!("ribbon.message_numbers"),
            )
            .tooltip(t!("prop.message_numbers"))
            .action(crate::ToggleMessageNumbers)
            .size(Size::Small)
            .selected(shown),
            Item::new(
                "flat-numbers",
                icons::own_icon("flatMessageNumbers"),
                t!("ribbon.flat_numbers"),
            )
            .tooltip(t!("prop.flat_message_numbers"))
            .action(crate::ToggleFlatNumbers)
            .size(Size::Small)
            .selected(flat)
            .enabled(shown),
        ]
    });
    [
        ("select", items(ToolGroup::Select, Size::Large)),
        ("elements", items(ToolGroup::Elements, Size::Large)),
        ("relations", items(ToolGroup::Relations, Size::Small)),
        ("notes", items(ToolGroup::Notes, Size::Small)),
        ("numbering", numbering),
    ]
    .into_iter()
    .filter(|(_, items)| !items.is_empty())
    .map(|(id, items)| group(id, items))
    .collect()
}

/// Inserting elements after the selection (before it with Shift in the context menu), and
/// what the selection allows.
fn structogram(allowed: crate::nsd::Allowed) -> Vec<Group> {
    use umlcraft_nsd::ElementKind as K;
    let insert = |kinds: &[K]| -> Vec<Item> {
        kinds
            .iter()
            .map(|&kind| {
                Item::new(
                    format!("nsd-{}", kind.tag()),
                    icons::nsd_kind_icon(kind),
                    crate::nsd::kind_name(kind),
                )
                .action(crate::nsd::InsertElement { kind, after: true })
                .size(Size::Small)
            })
            .collect()
    };
    let mut color = Item::new("nsd-color", icons::nsd_icon("color"), t!("nsd.color"))
        .size(Size::Small)
        .enabled(allowed.span);
    for (i, c) in crate::nsd::PALETTE.into_iter().enumerate() {
        color = color.menu_entry(t!(format!("nsd.color_{i}")), crate::nsd::SetColor { color: c });
    }
    vec![
        group("nsd_simple", insert(&[K::Instruction, K::Call, K::Jump])),
        group("nsd_branches", insert(&[K::Alternative, K::Case, K::Try, K::Parallel])),
        group("nsd_loops", insert(&[K::For, K::While, K::Repeat, K::Forever])),
        group(
            "nsd_element",
            vec![
                Item::new("nsd-edit", fluent("pen"), t!("nsd.edit"))
                    .action(crate::nsd::EditElement)
                    .enabled(allowed.edit),
                color,
                Item::new("nsd-disabled", icons::nsd_icon("disabled"), t!("nsd.disabled"))
                    .action(crate::nsd::ToggleDisabled)
                    .size(Size::Small)
                    .enabled(allowed.span)
                    .selected(allowed.disabled),
                Item::new("nsd-add-branch", icons::nsd_icon("addBranch"), t!("nsd.add_branch"))
                    .action(crate::nsd::AddBranch)
                    .size(Size::Small)
                    .enabled(allowed.branching),
                Item::new(
                    "nsd-remove-branch",
                    icons::nsd_icon("removeBranch"),
                    t!("nsd.remove_branch"),
                )
                .action(crate::nsd::RemoveBranch)
                .size(Size::Small)
                .enabled(allowed.branching),
            ],
        ),
    ]
}

fn view(state: &State) -> Vec<Group> {
    // The theme chosen, not the one a click switches to.
    let (theme_icon, theme_label) = match state.theme {
        None => ("dark_theme", t!("settings.system_theme")),
        Some(false) => ("weather_sunny", t!("ribbon.light")),
        Some(true) => ("weather_moon", t!("ribbon.dark")),
    };
    vec![
        group(
            "zoom",
            vec![
                Item::new("zoom-fit", fluent("zoom_fit"), t!("toolbar.zoom_fit")).action(ZoomToFit),
                Item::new("zoom-in", fluent("zoom_in"), t!("menu.zoom_in"))
                    .action(ZoomIn)
                    .size(Size::Small),
                Item::new("zoom-out", fluent("zoom_out"), t!("menu.zoom_out"))
                    .action(ZoomOut)
                    .size(Size::Small),
                Item::new("zoom-reset", fluent("ratio_one_to_one"), t!("menu.zoom_reset"))
                    .action(ResetZoom)
                    .size(Size::Small),
            ],
        ),
        group(
            "appearance",
            vec![
                Item::new("theme", fluent(theme_icon), theme_label)
                    .menu_check_entry(
                        t!("settings.system_theme"),
                        state.theme.is_none(),
                        SetTheme { dark: None },
                    )
                    .menu_check_entry(
                        t!("ribbon.light"),
                        state.theme == Some(false),
                        SetTheme { dark: Some(false) },
                    )
                    .menu_check_entry(
                        t!("ribbon.dark"),
                        state.theme == Some(true),
                        SetTheme { dark: Some(true) },
                    ),
                Item::new("language", fluent("local_language"), t!("toolbar.language"))
                    .action(ToggleLanguage),
                Item::new("settings", fluent("settings"), t!("toolbar.settings"))
                    .action(OpenSettings),
            ],
        ),
    ]
}

fn tools(state: &State) -> Vec<Group> {
    let languages = [
        (SourceLanguage::Java, "java", "menu.java"),
        (SourceLanguage::CSharp, "csharp", "menu.csharp"),
        (SourceLanguage::Cpp, "cpp", "menu.cpp"),
        (SourceLanguage::Python, "python", "menu.python"),
    ];
    vec![
        group(
            "source",
            languages
                .map(|(language, id, label)| {
                    Item::new(format!("import-{id}"), fluent("code"), t!(label))
                        .action(ImportSource { language, folder: false })
                        .menu_entry(t!("ribbon.files"), ImportSource { language, folder: false })
                        .menu_entry(t!("ribbon.folder"), ImportSource { language, folder: true })
                })
                .into(),
        ),
        group(
            "exchange",
            vec![
                Item::new("astah-import", fluent("arrow_import"), t!("ribbon.import"))
                    .action(ImportAstah),
                Item::new("astah-export", fluent("arrow_export_ltr"), t!("ribbon.export"))
                    .action(ExportAstah),
                // Goes to the structogram shown, so only with one.
                Item::new("nsd-export", fluent("arrow_export_ltr"), t!("ribbon.export_nsd"))
                    .tooltip(t!("ribbon.export_nsd_tip"))
                    .action(crate::nsd::ExportStructogram)
                    .enabled(state.structogram.is_some()),
            ],
        ),
        group(
            "image",
            vec![
                Item::new("export-svg", fluent("image_arrow_forward"), "SVG").action(ExportSvg),
                Item::new("export-png", fluent("image_arrow_forward"), "PNG").action(ExportPng),
                Item::new("copy-image", fluent("image_copy"), t!("ribbon.copy_image"))
                    .action(CopyImage),
            ],
        ),
    ]
}

// ----- Width --------------------------------------------------------------------------------

/// About the mean width of a character of a button's label ([`LABEL`]).
const CHAR: f32 = 7.0;
const LARGE_MIN: f32 = 56.0;
const SMALL_ICON: f32 = 34.0;
/// A group's padding on both sides and the line after it.
const GROUP_PADDING: f32 = 25.0;
const CARET: f32 = 14.0;

fn label_width(label: &str) -> f32 {
    label.chars().count() as f32 * CHAR
}

/// A large button's label on two lines, broken as Office does: a single word stays on the
/// first line, a longer label breaks at the space that leaves its longer line shortest.
pub fn label_lines(label: &str) -> (&str, &str) {
    label
        .match_indices(' ')
        .map(|(i, _)| (&label[..i], &label[i + 1..]))
        .min_by_key(|(first, second)| first.chars().count().max(second.chars().count()))
        .unwrap_or((label, ""))
}

/// About how wide the groups are with labels; with less room the ribbon shows icons only.
pub fn width(groups: &[Group]) -> f32 {
    groups
        .iter()
        .map(|g| {
            let mut width = 0.0;
            let mut column: Vec<f32> = Vec::new();
            let flush = |column: &mut Vec<f32>, width: &mut f32| {
                if let Some(widest) = column.iter().copied().reduce(f32::max) {
                    *width += widest;
                }
                column.clear();
            };
            for item in &g.items {
                match item.size {
                    Size::Large => {
                        flush(&mut column, &mut width);
                        let (first, second) = label_lines(&item.label);
                        let caret = if item.menu.is_empty() { 0.0 } else { CARET };
                        let label = label_width(first).max(label_width(second) + caret);
                        width += (label + 20.0).max(LARGE_MIN);
                    }
                    Size::Small | Size::Icon => {
                        let w = match item.size {
                            Size::Small => SMALL_ICON + label_width(&item.label) + 12.0,
                            _ => SMALL_ICON,
                        };
                        column.push(w);
                        if column.len() == 3 {
                            flush(&mut column, &mut width);
                        }
                    }
                }
            }
            flush(&mut column, &mut width);
            width.max(label_width(&g.caption) + 20.0) + GROUP_PADDING
        })
        .sum()
}

// ----- Drawing ------------------------------------------------------------------------------

/// Room for a large button: its icon and two lines of label.
const HEIGHT: Pixels = px(84.0);
/// The tab strip.
const STRIP: Pixels = px(40.0);
/// The size of the buttons' labels, between the tabs' and the captions'.
const LABEL: Pixels = px(13.0);
/// A line of a large button's label.
const LABEL_LINE: Pixels = px(16.0);
/// The lower part of a split button, with the label.
const SPLIT_LOWER: Pixels = px(36.0);
/// A large button's icon; with icons only, a smaller one.
const LARGE_ICON: Pixels = px(28.0);
const COMPACT_ICON: Pixels = px(22.0);
/// A small button's icon, and the title bar's.
pub const SMALL_ICON_SIZE: Pixels = px(20.0);

/// Background and text of the open tab and of a selected button: navy on a pale blue in the
/// light theme, pale blue on a dark blue in the dark one (the theme's own blue is too loud).
fn selected_colors(cx: &App) -> (Hsla, Hsla) {
    let hue = 213.0 / 360.0;
    if cx.theme().is_dark() {
        (hsla(hue, 0.45, 0.27, 1.0), hsla(hue, 0.85, 0.86, 1.0))
    } else {
        (hsla(hue, 0.80, 0.91, 1.0), hsla(hue, 0.62, 0.30, 1.0))
    }
}

/// Sends `action` to the view shown (`focus`), wherever the keyboard focus is, as the menus do:
/// after a click in the tree a structogram's commands still reach the structogram.
fn dispatch(
    action: &dyn Action,
    focus: &FocusHandle,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let action = action.boxed_clone();
    let focus = focus.clone();
    move |_, window, cx| focus.dispatch_action(action.as_ref(), window, cx)
}

type Entries = Rc<Vec<MenuEntry>>;

fn entries_menu(
    entries: Entries,
    focus: FocusHandle,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |mut menu, _, _| {
        for entry in entries.iter() {
            menu = match entry {
                MenuEntry::Command { label, icon: Some(icon), action } => menu.menu_with_icon(
                    label.clone(),
                    shell::icon(icon.clone()),
                    action.boxed_clone(),
                ),
                MenuEntry::Command { label, icon: None, action } => {
                    menu.menu(label.clone(), action.boxed_clone())
                }
                MenuEntry::Check { label, checked, action } => {
                    menu.menu_with_check(label.clone(), *checked, action.boxed_clone())
                }
                MenuEntry::Separator => menu.separator(),
            };
        }
        menu.action_context(focus.clone())
    }
}

/// The menu of a group caption: per button its command, or a submenu with the button's menu.
fn group_menu(
    entries: Rc<Vec<(String, Entries)>>,
    focus: FocusHandle,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |mut menu, window, cx| {
        for (label, commands) in entries.iter() {
            menu = match commands.as_slice() {
                [MenuEntry::Command { action, .. }] => {
                    menu.menu(label.clone(), action.boxed_clone())
                }
                _ => menu.submenu(
                    label.clone(),
                    window,
                    cx,
                    entries_menu(commands.clone(), focus.clone()),
                ),
            };
        }
        menu.action_context(focus.clone())
    }
}

fn large_button(item: &Item, compact: bool, focus: &FocusHandle, cx: &App) -> AnyElement {
    let icon = shell::icon(item.icon.clone()).size(if compact { COMPACT_ICON } else { LARGE_ICON });
    let menu: Entries = Rc::new(item.menu.clone());
    let caret = (!menu.is_empty())
        .then(|| Icon::new(IconName::ChevronDown).size_3().text_color(cx.theme().muted_foreground));
    // Two lines, a caret after the second, as Office labels its large buttons, so that their
    // icons stay level; with icons only, the caret alone.
    let label = if compact {
        v_flex().items_center().children(caret)
    } else {
        let (first, second) = label_lines(&item.label);
        v_flex()
            .items_center()
            .text_size(LABEL)
            .line_height(LABEL_LINE)
            .child(div().h(LABEL_LINE).child(first.to_string()))
            .child(h_flex().h(LABEL_LINE).gap_0p5().child(second.to_string()).children(caret))
    };
    let content = |label: Div| v_flex().items_center().gap_1().child(icon.clone()).child(label);
    let (selected_bg, selected_text) = selected_colors(cx);
    let base = |id: SharedString| {
        Button::new(id)
            .ghost()
            .min_w(px(if compact { 38.0 } else { LARGE_MIN }))
            .px_1p5()
            .disabled(!item.enabled)
            .selected(item.selected)
            .when(item.selected, |b| b.bg(selected_bg).text_color(selected_text))
            .tooltip(item.tooltip.clone().unwrap_or_else(|| item.label.clone()))
    };
    match &item.action {
        Some(action) if menu.is_empty() => base(item.id.clone())
            .h(HEIGHT - px(8.0))
            .child(content(label))
            .on_click(dispatch(action.as_ref(), focus))
            .into_any_element(),
        // Split: the upper part runs the action, the lower one, the label, opens the menu.
        Some(action) => v_flex()
            .child(
                base(item.id.clone())
                    .h(HEIGHT - px(8.0) - SPLIT_LOWER)
                    .child(icon.clone())
                    .on_click(dispatch(action.as_ref(), focus)),
            )
            .child(
                base(format!("{}-menu", item.id).into())
                    .h(SPLIT_LOWER)
                    .child(label)
                    .dropdown_menu(entries_menu(menu, focus.clone())),
            )
            .into_any_element(),
        None => base(item.id.clone())
            .h(HEIGHT - px(8.0))
            .child(content(label))
            .dropdown_menu(entries_menu(menu, focus.clone()))
            .into_any_element(),
    }
}

fn small_button(item: &Item, compact: bool, focus: &FocusHandle, cx: &App) -> AnyElement {
    let label = item.size == Size::Small && !compact;
    let (selected_bg, selected_text) = selected_colors(cx);
    let button = Button::new(item.id.clone())
        .ghost()
        .xsmall()
        .h(px(24.0))
        .px_1p5()
        .text_size(LABEL)
        // The icon as large as fits the row of 24 px, for the fine UML line ends. A child, not
        // `Button::icon`, which sizes it after the button.
        .child(
            h_flex()
                .gap_1p5()
                .child(shell::icon(item.icon.clone()).size(SMALL_ICON_SIZE))
                .when(label, |this| this.child(item.label.clone())),
        )
        .tooltip(item.tooltip.clone().unwrap_or_else(|| item.label.clone()))
        .disabled(!item.enabled)
        .selected(item.selected)
        .when(item.selected, |b| b.bg(selected_bg).text_color(selected_text));
    match &item.action {
        Some(action) => button.on_click(dispatch(action.as_ref(), focus)).into_any_element(),
        None => {
            let menu: Entries = Rc::new(item.menu.clone());
            button
                .dropdown_caret(true)
                .dropdown_menu(entries_menu(menu, focus.clone()))
                .into_any_element()
        }
    }
}

fn group_view(group: &Group, compact: bool, focus: &FocusHandle, cx: &App) -> impl IntoElement {
    let mut row = h_flex().gap_1().items_start().h(HEIGHT - px(8.0));
    let mut column: Vec<AnyElement> = Vec::new();
    let flush = |row: Div, column: &mut Vec<AnyElement>| {
        if column.is_empty() {
            row
        } else {
            // Icons line up at the left of the column, the column in the middle of the row.
            row.child(
                v_flex()
                    .h_full()
                    .gap_0p5()
                    .items_start()
                    .justify_center()
                    .children(column.drain(..)),
            )
        }
    };
    for item in &group.items {
        match item.size {
            Size::Large => {
                row = flush(row, &mut column);
                row = row.child(large_button(item, compact, focus, cx));
            }
            Size::Small | Size::Icon => {
                column.push(small_button(item, compact, focus, cx));
                if column.len() == 3 {
                    row = flush(row, &mut column);
                }
            }
        }
    }
    row = flush(row, &mut column);

    // The caption lists the group's commands, so that each stays reachable when the ribbon is
    // cut off at the right. A button with a menu of its own is a submenu there.
    let entries: Vec<(String, Entries)> = group
        .items
        .iter()
        .filter(|i| i.enabled)
        .filter_map(|item| {
            let commands: Vec<_> = if item.menu.is_empty() {
                item.action
                    .iter()
                    .map(|a| MenuEntry::Command {
                        label: item.label.clone(),
                        icon: None,
                        action: a.boxed_clone(),
                    })
                    .collect()
            } else {
                item.menu.clone()
            };
            (!commands.is_empty()).then(|| (item.label.clone(), Rc::new(commands)))
        })
        .collect();
    let caption = Button::new(SharedString::from(format!("group-{}", group.id)))
        .ghost()
        .xsmall()
        .label(group.caption.clone())
        .text_color(cx.theme().muted_foreground)
        .dropdown_caret(true)
        .dropdown_menu(group_menu(Rc::new(entries), focus.clone()));
    v_flex().flex_shrink_0().items_center().px_3().child(row).child(caption)
}

/// The ribbon card: the tab strip and, unless collapsed, the open tab's groups.
pub fn render(
    tabs: &[(RibbonTab, String)],
    active: RibbonTab,
    groups: &[Group],
    compact: bool,
    collapsed: bool,
    focus: &FocusHandle,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (tab_bg, tab_text) = selected_colors(cx);
    let strip =
        h_flex().h(STRIP).gap_1().px_2().pt_1().children(tabs.iter().map(|(tab, label)| {
            let tab = *tab;
            let selected = tab == active && !collapsed;
            div()
                .id(SharedString::from(format!("ribbon-tab-{tab:?}")))
                .px_3()
                .py_1()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .when(selected, |this| this.bg(tab_bg).text_color(tab_text))
                .when(!selected, |this| this.hover(|s| s.bg(theme.secondary_hover)))
                // Every tab is as wide as its label in bold, an invisible copy holding the
                // place, so that selecting one does not shift the others.
                .child(
                    div()
                        .relative()
                        .child(
                            div()
                                .font_semibold()
                                .text_color(transparent_black())
                                .child(label.clone()),
                        )
                        .child(
                            h_flex()
                                .absolute()
                                .inset_0()
                                .justify_center()
                                .when(selected, |this| this.font_semibold())
                                .child(label.clone()),
                        ),
                )
                // A double click folds the ribbon to its tabs, or unfolds it.
                .on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        window.dispatch_action(Box::new(ToggleRibbon), cx);
                    } else {
                        window.dispatch_action(Box::new(SelectRibbonTab { tab }), cx);
                    }
                })
        }));
    // A short line between groups, from the buttons' top to the captions' bottom.
    let line = || div().flex_none().w(px(1.0)).my_1().bg(theme.border);
    let mut row = h_flex().px_1().pt_1().pb_0p5().items_stretch();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            row = row.child(line());
        }
        row = row.child(group_view(group, compact, focus, cx));
    }
    // The card keeps the mouse from the diagram below, and with it from the elements around
    // it: whoever handles the wheel over the ribbon attaches it to this card.
    shell::floating_card(cx)
        .id("ribbon")
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .child(strip)
        .when(!collapsed, |this| this.child(row))
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use super::{DIAGRAM_KINDS, RibbonTab, State, diagram, label_lines, tabs};
    use umlcraft_model::DiagramKind;
    use umlcraft_notation::palette::{self, Tool};

    #[test]
    fn every_tool_of_every_diagram_kind_is_in_one_group() {
        for kind in DIAGRAM_KINDS {
            let groups = diagram(kind, false, Tool::Select, None);
            for tool in palette::palette(kind) {
                let id = format!("tool-{}", tool.key());
                let found = groups.iter().flat_map(|g| &g.items).filter(|i| i.id == id).count();
                assert_eq!(found, 1, "{kind:?}: {tool:?}");
            }
        }
    }

    #[test]
    fn an_object_diagram_starts_its_groups_with_object_and_link() {
        let groups = diagram(DiagramKind::Class, true, Tool::Select, None);
        let first = |id: &str| groups.iter().find(|g| g.id == id).map(|g| g.items[0].id.clone());
        assert_eq!(first("elements").as_deref(), Some("tool-object"));
        assert_eq!(first("relations").as_deref(), Some("tool-link"));
    }

    #[test]
    fn labels_break_where_the_longer_line_is_shortest() {
        assert_eq!(label_lines("Einfügen"), ("Einfügen", ""));
        assert_eq!(label_lines("Neues Diagramm"), ("Neues", "Diagramm"));
        assert_eq!(label_lines("Wrap in a fragment"), ("Wrap in a", "fragment"));
        assert_eq!(label_lines("Linien neu führen"), ("Linien", "neu führen"));
    }

    #[test]
    fn the_diagram_tab_is_there_only_with_a_diagram() {
        let mut state = State {
            diagram: None,
            objects: false,
            structogram: None,
            tool: Tool::Select,
            has_selection: false,
            can_align: false,
            can_paste: false,
            can_wrap: false,
            numbering: None,
            theme: None,
        };
        assert!(!tabs(&state).iter().any(|(t, _)| *t == RibbonTab::Diagram));
        state.diagram = Some(DiagramKind::Class);
        assert!(tabs(&state).iter().any(|(t, _)| *t == RibbonTab::Diagram));
        state.diagram = None;
        state.structogram = Some(crate::nsd::Allowed::default());
        assert!(tabs(&state).iter().any(|(t, _)| *t == RibbonTab::Diagram));
    }

    #[test]
    fn ribbon_texts_exist_in_both_languages() {
        let de: serde_json::Value = serde_yaml_like(include_str!("../../../locales/de.yml"));
        let en: serde_json::Value = serde_yaml_like(include_str!("../../../locales/en.yml"));
        assert_eq!(de, en);
    }

    /// The `ribbon:` keys of a locale file, as nested names (values left out).
    fn serde_yaml_like(text: &str) -> serde_json::Value {
        let mut keys = Vec::new();
        let mut inside = false;
        for line in text.lines() {
            if !line.starts_with(' ') {
                inside = line.trim_end() == "ribbon:";
                continue;
            }
            if inside && let Some((key, _)) = line.split_once(':') {
                keys.push(key.to_string());
            }
        }
        assert!(!keys.is_empty());
        keys.into()
    }
}
