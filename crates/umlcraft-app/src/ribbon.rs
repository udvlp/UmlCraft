//! The ribbon: tabs with labelled groups of large and small buttons, a panel above
//! the others. Tabs, groups and buttons are data (`tabs`, `groups`), built on every render from a
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
use umlcraft_model::{DiagramKind, DiagramProps, ErNotation};
use umlcraft_notation::layout::{self, Direction};
use umlcraft_notation::palette::{EdgeTool, NodeTool, Tool};

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
}

/// What the buttons show, read once per render from the workspace.
pub struct State {
    /// The diagram shown, if one is.
    pub props: Option<DiagramProps>,
    /// What the selection of the structogram shown allows, if one is shown.
    pub structogram: Option<crate::nsd::Allowed>,
    pub tool: Tool,
    pub has_selection: bool,
    pub can_align: bool,
    pub can_paste: bool,
    pub can_wrap: bool,
    /// The chosen theme; `None` follows the system's appearance.
    pub theme: Option<bool>,
}

impl State {
    /// The kind of the diagram shown.
    pub fn diagram(&self) -> Option<DiagramKind> {
        self.props.map(|p| p.diagram)
    }
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
        action: Box<dyn Action>,
    ) -> Self {
        self.menu.push(MenuEntry::Command { label: label.into(), icon: Some(icon), action });
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

/// The tabs in order, with their labels: Edit, View, then the diagram tab, named after the
/// diagram's kind.
pub fn tabs(state: &State) -> Vec<(RibbonTab, String)> {
    let mut tabs = vec![
        (RibbonTab::Home, t!("ribbon.tab.home").to_string()),
        (RibbonTab::View, t!("ribbon.tab.view").to_string()),
    ];
    if let Some(p) = state.props {
        let key = match p.objects {
            true => crate::workspace::NewItem::ObjectDiagram.key(),
            false => diagram_key(p.diagram),
        };
        tabs.push((RibbonTab::Diagram, t!(key).to_string()));
    } else if state.structogram.is_some() {
        tabs.push((RibbonTab::Diagram, t!("kind.structogram").to_string()));
    }
    tabs
}

pub fn diagram_key(kind: DiagramKind) -> String {
    crate::workspace::NewItem::Diagram(kind).key()
}

pub fn groups(tab: RibbonTab, state: &State) -> Vec<Group> {
    match tab {
        RibbonTab::Home => home(state),
        RibbonTab::Diagram => match (state.props, state.structogram) {
            (Some(p), _) => {
                // On a sequence diagram: whether it shows message numbers, and whether flat ones.
                let numbering = (p.diagram == DiagramKind::Sequence)
                    .then_some((!p.hide_message_numbers, p.flat_message_numbers));
                let notation = (p.diagram == DiagramKind::EntityRelationship).then_some(p.notation);
                let palette = umlcraft_notation::palette::diagram_palette(&p);
                diagram(&palette, state.tool, numbering, notation)
            }
            (None, Some(allowed)) => structogram(allowed),
            (None, None) => Vec::new(),
        },
        RibbonTab::View => view(state),
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
                // The diagram shown as SVG and PNG.
                Item::new("copy-image", fluent("image_copy"), t!("ribbon.copy_as_image"))
                    .action(CopyImage)
                    .size(Size::Small)
                    .enabled(state.props.is_some() || state.structogram.is_some()),
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
                    .enabled(state.props.is_some()),
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
                    .enabled(state.diagram().is_some_and(layout::can_arrange)),
                Item::new("route-lines", fluent("arrow_routing"), t!("ribbon.route_lines"))
                    .action(RouteLines)
                    .size(Size::Small)
                    .enabled(state.diagram().is_some_and(|k| k != DiagramKind::Sequence)),
            ],
        ),
    ]
}

/// A button whose menu creates a diagram of the chosen kind or a structogram, in the
/// alphabetical order of their names in the language shown.
fn new_diagram() -> Item {
    let mut entries: Vec<(String, SharedString, Box<dyn Action>)> = Vec::new();
    for kind in DiagramKind::ALL {
        let action = NewDiagram { kind, objects: false };
        entries.push((t!(diagram_key(kind)).into(), icons::diagram_icon(kind), Box::new(action)));
        if kind == DiagramKind::Class {
            let action = NewDiagram { kind, objects: true };
            entries.push((
                t!("diagram.object").into(),
                icons::object_diagram_icon(),
                Box::new(action),
            ));
        }
    }
    entries.push((
        t!("kind.structogram").into(),
        icons::structogram_icon(),
        Box::new(NewStructogram),
    ));
    entries.sort_by_cached_key(|(label, ..)| sort_key(label));
    let item = Item::new("new-diagram", fluent("document_add"), t!("ribbon.new_diagram"));
    entries
        .into_iter()
        .fold(item, |item, (label, icon, action)| item.menu_icon_entry(label, icon, action))
}

/// `label` for sorting by the alphabet: without case, an umlaut as its vowel and ß as ss, as
/// German dictionaries sort.
fn sort_key(label: &str) -> String {
    label
        .to_lowercase()
        .chars()
        .flat_map(|c| match c {
            'ä' => vec!['a'],
            'ö' => vec!['o'],
            'ü' => vec!['u'],
            'ß' => vec!['s', 's'],
            c => vec![c],
        })
        .collect()
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
    tools: &[Tool],
    current: Tool,
    numbering: Option<(bool, bool)>,
    notation: Option<ErNotation>,
) -> Vec<Group> {
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
                // Tools whose name alone does not say how they are used have a `tool_tip` text.
                let tip = format!("tool_tip.{}", tool.key());
                match crate::_rust_i18n_try_translate(&rust_i18n::locale(), tip) {
                    Some(tip) => item.tooltip(tip.into_owned()),
                    None => item,
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
        ("notation", notation.map_or_else(Vec::new, notation_items)),
    ]
    .into_iter()
    .filter(|(_, items)| !items.is_empty())
    .map(|(id, items)| group(id, items))
    .collect()
}

/// The notations an ER diagram can be drawn in, the current one checked.
fn notation_items(current: ErNotation) -> Vec<Item> {
    [
        (ErNotation::Chen, "chen", "notationChen"),
        (ErNotation::Ie, "ie", "notationIe"),
        (ErNotation::Idef1x, "idef1x", "notationIdef1x"),
    ]
    .into_iter()
    .map(|(notation, key, icon)| {
        Item::new(
            format!("notation-{key}"),
            icons::own_icon(icon),
            t!(format!("ribbon.notation.{key}")),
        )
        .tooltip(t!(format!("tool_tip.notation_{key}")))
        .action(crate::SetErNotation { notation })
        .size(Size::Small)
        .selected(notation == current)
    })
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
/// The tab strip above the ribbon card.
const STRIP: Pixels = px(28.0);
/// The size of the buttons' labels, between the tabs' and the captions'.
const LABEL: Pixels = px(13.0);
/// The size of the groups' captions below their buttons.
const CAPTION: Pixels = px(11.0);
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
pub(crate) fn selected_colors(cx: &App) -> (Hsla, Hsla) {
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
            };
        }
        menu.action_context(focus.clone())
    }
}

fn large_button(mut item: Item, compact: bool, focus: &FocusHandle, cx: &App) -> AnyElement {
    let icon = shell::icon(item.icon.clone()).size(if compact { COMPACT_ICON } else { LARGE_ICON });
    let menu: Entries = Rc::new(std::mem::take(&mut item.menu));
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

fn small_button(mut item: Item, compact: bool, focus: &FocusHandle, cx: &App) -> AnyElement {
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
            let menu: Entries = Rc::new(std::mem::take(&mut item.menu));
            button
                .dropdown_caret(true)
                .dropdown_menu(entries_menu(menu, focus.clone()))
                .into_any_element()
        }
    }
}

fn group_view(group: Group, compact: bool, focus: &FocusHandle, cx: &App) -> impl IntoElement {
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
    for item in group.items {
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

    let caption = div()
        .h(px(18.0))
        .flex()
        .items_center()
        .text_size(CAPTION)
        .text_color(cx.theme().muted_foreground)
        .child(group.caption);
    v_flex()
        .id(SharedString::from(format!("group-{}", group.id)))
        .flex_shrink_0()
        .items_center()
        .px_3()
        .child(row)
        .child(caption)
}

/// A tab of the strip: its label and below it a short bar, blue under the open tab and grey
/// under the one the mouse is over.
fn tab_view(id: SharedString, label: &str, selected: bool, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (_, accent) = selected_colors(cx);
    v_flex()
        .id(id)
        .group("ribbon-tab")
        .px_2()
        .pt_1()
        .text_sm()
        .cursor_pointer()
        .when(selected, |this| this.text_color(accent))
        // Every tab is as wide as its label in bold, an invisible copy holding the place, so
        // that selecting one does not shift the others.
        .child(
            div()
                .relative()
                .child(
                    div().font_semibold().text_color(transparent_black()).child(label.to_owned()),
                )
                .child(
                    h_flex()
                        .absolute()
                        .inset_0()
                        .justify_center()
                        .when(selected, |this| this.font_semibold())
                        .child(label.to_owned()),
                ),
        )
        .child(
            div()
                .mt(px(1.0))
                .h(px(2.0))
                .rounded_full()
                .when(selected, |this| this.bg(accent))
                .when(!selected, |this| this.group_hover("ribbon-tab", |s| s.bg(theme.border))),
        )
}

/// The tabs above the ribbon card, on the window's backdrop: File, which opens the file page,
/// then the ribbon's own tabs.
pub fn tab_strip(
    file_label: &str,
    tabs: &[(RibbonTab, String)],
    active: RibbonTab,
    collapsed: bool,
    cx: &App,
) -> Div {
    let file = tab_view("ribbon-tab-file".into(), file_label, false, cx)
        .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenFilePage), cx));
    h_flex().h(STRIP).items_end().gap_2().px_1().child(file).children(tabs.iter().map(
        |(tab, label)| {
            let tab = *tab;
            let id = SharedString::from(format!("ribbon-tab-{tab:?}"));
            tab_view(id, label, tab == active && !collapsed, cx)
                // A double click folds the ribbon to its tabs, or unfolds it.
                .on_click(move |event, window, cx| {
                    if event.click_count() >= 2 {
                        window.dispatch_action(Box::new(ToggleRibbon), cx);
                    } else {
                        window.dispatch_action(Box::new(SelectRibbonTab { tab }), cx);
                    }
                })
        },
    ))
}

/// The ribbon card: the open tab's groups.
pub fn render(groups: Vec<Group>, compact: bool, focus: &FocusHandle, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    // A short line between groups, from the buttons' top to the captions' bottom.
    let line = || div().flex_none().w(px(1.0)).my_1().bg(theme.border);
    let mut row = h_flex().px_1().pt_1().pb_0p5().items_stretch();
    for (i, group) in groups.into_iter().enumerate() {
        if i > 0 {
            row = row.child(line());
        }
        row = row.child(group_view(group, compact, focus, cx));
    }
    shell::panel_card(cx).id("ribbon").min_w_0().overflow_hidden().child(row)
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use super::{RibbonTab, State, diagram, diagram_key, label_lines, sort_key, tabs};
    use umlcraft_model::{DiagramKind, DiagramProps, ElementKind, ErNotation};
    use umlcraft_notation::palette::{self, Tool};

    fn props(kind: DiagramKind) -> DiagramProps {
        match ElementKind::diagram(kind) {
            ElementKind::Diagram(p) => p,
            _ => unreachable!(),
        }
    }

    #[test]
    fn new_diagram_lists_its_kinds_by_the_alphabet() {
        let sorted = |locale: &str| {
            let mut keys: Vec<String> = DiagramKind::ALL.into_iter().map(diagram_key).collect();
            keys.extend(["diagram.object".to_owned(), "kind.structogram".to_owned()]);
            let mut labels: Vec<String> =
                keys.iter().map(|k| rust_i18n::t!(k.as_str(), locale = locale).into()).collect();
            labels.sort_by_cached_key(|l| sort_key(l));
            labels
        };
        assert_eq!(
            sorted("de"),
            [
                "Aktivitätsdiagramm",
                "Anwendungsfalldiagramm",
                "ER-Diagramm",
                "Klassendiagramm",
                "Komponentendiagramm",
                "Kompositionsstrukturdiagramm",
                "Objektdiagramm",
                "Sequenzdiagramm",
                "Struktogramm",
                "Verteilungsdiagramm",
                "Zustandsdiagramm",
            ]
        );
        assert_eq!(
            sorted("en"),
            [
                "Activity diagram",
                "Class diagram",
                "Component diagram",
                "Composite structure diagram",
                "Deployment diagram",
                "ER diagram",
                "Object diagram",
                "Sequence diagram",
                "State machine diagram",
                "Structogram",
                "Use case diagram",
            ]
        );
    }

    #[test]
    fn every_tool_of_every_diagram_kind_is_in_one_group() {
        let every = DiagramKind::ALL.into_iter().flat_map(|kind| {
            ErNotation::ALL.map(|notation| DiagramProps { notation, ..props(kind) })
        });
        for p in every {
            let tools = palette::diagram_palette(&p);
            let groups = diagram(&tools, Tool::Select, None, Some(p.notation));
            for tool in tools {
                let id = format!("tool-{}", tool.key());
                let found = groups.iter().flat_map(|g| &g.items).filter(|i| i.id == id).count();
                assert_eq!(found, 1, "{:?} {:?}: {tool:?}", p.diagram, p.notation);
            }
        }
    }

    #[test]
    fn an_object_diagram_starts_its_groups_with_object_and_link() {
        let tools =
            palette::diagram_palette(&DiagramProps { objects: true, ..props(DiagramKind::Class) });
        let groups = diagram(&tools, Tool::Select, None, None);
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
            props: None,
            structogram: None,
            tool: Tool::Select,
            has_selection: false,
            can_align: false,
            can_paste: false,
            can_wrap: false,
            theme: None,
        };
        assert!(!tabs(&state).iter().any(|(t, _)| *t == RibbonTab::Diagram));
        state.props = Some(props(DiagramKind::Class));
        // Edit and View come first and keep their place; the diagram's tab follows them.
        let order: Vec<RibbonTab> = tabs(&state).into_iter().map(|(t, _)| t).collect();
        assert_eq!(order, [RibbonTab::Home, RibbonTab::View, RibbonTab::Diagram]);
        state.props = None;
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
