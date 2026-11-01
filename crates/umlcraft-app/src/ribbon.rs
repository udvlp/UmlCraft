//! The ribbon: tabs with labelled groups of large and small buttons, floating above the
//! panels. Tabs, groups and buttons are data (`tabs`, `groups`), built on every render from a
//! [`State`]; [`render`] draws them. Every button dispatches an action, so the ribbon holds no
//! state of its own besides the open tab, which the main view keeps.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenu};
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable, StyledExt, h_flex, v_flex,
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
    /// What the selection of the structogram shown allows, if one is shown.
    pub structogram: Option<crate::nsd::Allowed>,
    pub tool: Tool,
    pub has_selection: bool,
    pub can_align: bool,
    pub can_paste: bool,
    pub can_wrap: bool,
    /// On a sequence diagram: whether it shows message numbers, and whether flat ones.
    pub numbering: Option<(bool, bool)>,
    pub dark: bool,
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
    pub menu: Vec<(String, Box<dyn Action>)>,
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
        self.menu.push((label.into(), Box::new(action)));
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
        tabs.push((RibbonTab::Diagram, t!(diagram_key(kind)).to_string()));
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
            (Some(kind), _) => diagram(kind, state.tool, state.numbering),
            (None, Some(allowed)) => structogram(allowed),
            (None, None) => Vec::new(),
        },
        RibbonTab::View => view(state),
        RibbonTab::Tools => tools(),
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
                    .action(Arrange { direction: Direction::Down })
                    .menu_entry(t!("menu.arrange_down"), Arrange { direction: Direction::Down })
                    .menu_entry(t!("menu.arrange_right"), Arrange { direction: Direction::Right })
                    .enabled(state.diagram.is_some_and(layout::can_arrange)),
                Item::new("route-lines", fluent("arrow_routing"), t!("ribbon.route_lines"))
                    .action(RouteLines)
                    .size(Size::Small)
                    .enabled(state.diagram.is_some_and(|k| k != DiagramKind::Sequence)),
            ],
        ),
    ]
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

fn diagram(kind: DiagramKind, current: Tool, numbering: Option<(bool, bool)>) -> Vec<Group> {
    let tools = palette::palette(kind);
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
        group(
            "nsd_export",
            vec![
                Item::new("nsd-export", fluent("arrow_export_ltr"), t!("ribbon.export_nsd"))
                    .tooltip(t!("ribbon.export_nsd_tip"))
                    .action(crate::nsd::ExportStructogram),
            ],
        ),
    ]
}

fn view(state: &State) -> Vec<Group> {
    let (theme_icon, theme_label) = if state.dark {
        ("weather_sunny", t!("ribbon.light"))
    } else {
        ("weather_moon", t!("ribbon.dark"))
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
                Item::new("theme", fluent(theme_icon), theme_label).action(ToggleTheme),
                Item::new("language", fluent("local_language"), t!("toolbar.language"))
                    .action(ToggleLanguage),
                Item::new("settings", fluent("settings"), t!("toolbar.settings"))
                    .action(OpenSettings),
            ],
        ),
    ]
}

fn tools() -> Vec<Group> {
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

const CHAR: f32 = 6.5;
const LARGE_MIN: f32 = 52.0;
const SMALL_ICON: f32 = 28.0;
const GROUP_PADDING: f32 = 18.0;

fn label_width(label: &str) -> f32 {
    label.chars().count() as f32 * CHAR
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
                        width += (label_width(&item.label) + 16.0).max(LARGE_MIN);
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

const HEIGHT: Pixels = px(68.0);
/// The tab strip; the card left of the ribbon starts its button below this height.
const STRIP: Pixels = px(34.0);

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

type Entries = Rc<Vec<(String, Box<dyn Action>)>>;

fn entries_menu(
    entries: Entries,
    focus: FocusHandle,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |mut menu, _, _| {
        for (label, action) in entries.iter() {
            menu = menu.menu(label.clone(), action.boxed_clone());
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
                [(_, action)] => menu.menu(label.clone(), action.boxed_clone()),
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

fn large_button(item: &Item, compact: bool, focus: &FocusHandle) -> AnyElement {
    let icon = shell::icon(item.icon.clone()).size(px(if compact { 20.0 } else { 24.0 }));
    let menu: Entries =
        Rc::new(item.menu.iter().map(|(l, a)| (l.clone(), a.boxed_clone())).collect());
    let content = |label: bool| {
        v_flex()
            .items_center()
            .gap_1()
            .child(icon.clone())
            .when(label, |this| this.child(div().text_xs().child(item.label.clone())))
    };
    let base = |id: SharedString| {
        Button::new(id)
            .ghost()
            .min_w(px(if compact { 36.0 } else { LARGE_MIN }))
            .px_1()
            .disabled(!item.enabled)
            .selected(item.selected)
            .tooltip(item.tooltip.clone().unwrap_or_else(|| item.label.clone()))
    };
    match (&item.action, menu.is_empty()) {
        (Some(action), true) => base(item.id.clone())
            .h(HEIGHT - px(8.0))
            .child(content(!compact))
            .on_click(dispatch(action.as_ref(), focus))
            .into_any_element(),
        // Split: the upper part runs the action, the lower one opens the menu.
        (Some(action), false) => v_flex()
            .child(
                base(item.id.clone())
                    .h(HEIGHT - px(30.0))
                    .child(icon.clone())
                    .on_click(dispatch(action.as_ref(), focus)),
            )
            .child(
                base(format!("{}-menu", item.id).into())
                    .h(px(22.0))
                    .when(!compact, |this| this.child(div().text_xs().child(item.label.clone())))
                    .dropdown_caret(true)
                    .dropdown_menu(entries_menu(menu, focus.clone())),
            )
            .into_any_element(),
        (None, _) => base(item.id.clone())
            .h(HEIGHT - px(8.0))
            .child(content(!compact))
            .dropdown_caret(true)
            .dropdown_menu(entries_menu(menu, focus.clone()))
            .into_any_element(),
    }
}

fn small_button(item: &Item, compact: bool, focus: &FocusHandle) -> AnyElement {
    let label = item.size == Size::Small && !compact;
    let button = Button::new(item.id.clone())
        .ghost()
        .xsmall()
        .h(px(20.0))
        .icon(shell::icon(item.icon.clone()).size_4())
        .when(label, |this| this.label(item.label.clone()))
        .tooltip(item.tooltip.clone().unwrap_or_else(|| item.label.clone()))
        .disabled(!item.enabled)
        .selected(item.selected);
    match &item.action {
        Some(action) => button.on_click(dispatch(action.as_ref(), focus)).into_any_element(),
        None => {
            let menu: Entries =
                Rc::new(item.menu.iter().map(|(l, a)| (l.clone(), a.boxed_clone())).collect());
            button
                .dropdown_caret(true)
                .dropdown_menu(entries_menu(menu, focus.clone()))
                .into_any_element()
        }
    }
}

fn group_view(group: &Group, compact: bool, focus: &FocusHandle, cx: &App) -> impl IntoElement {
    let mut row = h_flex().gap_0p5().items_start().h(HEIGHT - px(8.0));
    let mut column: Vec<AnyElement> = Vec::new();
    let flush = |row: Div, column: &mut Vec<AnyElement>| {
        if column.is_empty() {
            row
        } else {
            // Icons line up at the left of the column.
            row.child(v_flex().gap_0p5().items_start().justify_center().children(column.drain(..)))
        }
    };
    for item in &group.items {
        match item.size {
            Size::Large => {
                row = flush(row, &mut column);
                row = row.child(large_button(item, compact, focus));
            }
            Size::Small | Size::Icon => {
                column.push(small_button(item, compact, focus));
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
                item.action.iter().map(|a| (item.label.clone(), a.boxed_clone())).collect()
            } else {
                item.menu.iter().map(|(l, a)| (l.clone(), a.boxed_clone())).collect()
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
    v_flex()
        .flex_shrink_0()
        .items_center()
        .px_2()
        .border_r_1()
        .border_color(cx.theme().border)
        .child(row)
        .child(caption)
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
) -> impl IntoElement {
    let theme = cx.theme();
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
                .when(selected, |this| this.bg(theme.primary.opacity(0.12)))
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
    shell::card(cx).flex_1().min_w_0().overflow_hidden().child(strip).when(!collapsed, |this| {
        this.child(
            h_flex()
                .px_1()
                .py_1()
                .items_start()
                .children(groups.iter().map(|g| group_view(g, compact, focus, cx))),
        )
    })
}

/// The card left of the ribbon: a button whose menu creates a diagram of the chosen kind.
pub fn new_diagram_card(collapsed: bool, focus: &FocusHandle, cx: &App) -> impl IntoElement {
    let focus = focus.clone();
    let menu = move |mut menu: PopupMenu, _: &mut Window, _: &mut Context<PopupMenu>| {
        for kind in DIAGRAM_KINDS {
            menu = menu.menu_with_icon(
                t!(diagram_key(kind)),
                shell::icon(icons::diagram_icon(kind)),
                Box::new(NewDiagram { kind }),
            );
        }
        menu.separator()
            .menu_with_icon(
                t!("kind.structogram"),
                shell::icon(icons::structogram_icon()),
                Box::new(NewStructogram),
            )
            .action_context(focus.clone())
    };
    let label = t!("ribbon.new_diagram").to_string();
    let button = Button::new("new-diagram").ghost().dropdown_caret(true);
    let card = shell::card(cx).flex_shrink_0().w(px(124.0)).px_1();
    if collapsed {
        let button = button.xsmall().icon(shell::icon(fluent("document_add"))).label(label);
        return card.h(STRIP).flex().items_center().child(button.dropdown_menu(menu));
    }
    // As high as the ribbon's large buttons and level with them, below the height of its tabs.
    let button = button.w_full().px_1().h(HEIGHT - px(8.0)).child(
        v_flex()
            .items_center()
            .gap_1()
            .child(shell::icon(fluent("document_add")).size(px(24.0)))
            .child(div().text_xs().text_center().child(label)),
    );
    card.pt(STRIP + px(4.0)).child(button.dropdown_menu(menu))
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use super::{DIAGRAM_KINDS, RibbonTab, State, diagram, tabs};
    use umlcraft_model::DiagramKind;
    use umlcraft_notation::palette::{self, Tool};

    #[test]
    fn every_tool_of_every_diagram_kind_is_in_one_group() {
        for kind in DIAGRAM_KINDS {
            let groups = diagram(kind, Tool::Select, None);
            for tool in palette::palette(kind) {
                let id = format!("tool-{}", tool.key());
                let found = groups.iter().flat_map(|g| &g.items).filter(|i| i.id == id).count();
                assert_eq!(found, 1, "{kind:?}: {tool:?}");
            }
        }
    }

    #[test]
    fn the_diagram_tab_is_there_only_with_a_diagram() {
        let mut state = State {
            diagram: None,
            structogram: None,
            tool: Tool::Select,
            has_selection: false,
            can_align: false,
            can_paste: false,
            can_wrap: false,
            numbering: None,
            dark: false,
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
