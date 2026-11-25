//! The file page the File tab opens over the window below the title bar, as Office and
//! MailCraft do: on the left a column of pages and commands, on the right the open page with
//! its commands as cards.

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{DiagramKind, ElementKind};
use umlcraft_reverse::SourceLanguage;

use super::MainView;
use crate::icons::{self, fluent};
use crate::shell;
use crate::*;

/// The pages of the file page; commands such as Save are entries that act at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilePage {
    Info,
    New,
    Open,
    Import,
    Export,
}

/// What a click does: runs a command and closes the page, or shows a page.
type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

/// One of gpui-kit's Lucide icons by its file name, e.g. `lucide("folder-open")`.
fn lucide(name: &str) -> SharedString {
    format!("icons/{name}.svg").into()
}

impl MainView {
    pub(super) fn open_file_page(
        &mut self,
        _: &OpenFilePage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_page = Some(FilePage::Info);
        cx.notify();
    }

    pub(super) fn close_file_page(&mut self, cx: &mut Context<Self>) {
        if self.file_page.take().is_some() {
            cx.notify();
        }
    }

    /// Closes the page and sends `action` where the ribbon's commands go, the view shown, so
    /// that a structogram's export reaches the structogram.
    fn run(&self, action: Box<dyn Action>, cx: &mut Context<Self>) -> OnClick {
        let view = cx.entity().downgrade();
        let focus = self.view_focus(cx);
        Box::new(move |_, window, cx| {
            view.update(cx, |this, cx| this.close_file_page(cx)).ok();
            focus.dispatch_action(action.as_ref(), window, cx);
        })
    }

    fn show(&self, page: FilePage, cx: &mut Context<Self>) -> OnClick {
        Box::new(cx.listener(move |this, _, _, cx| {
            this.file_page = Some(page);
            cx.notify();
        }))
    }

    /// The whole page, covering the tabs, the ribbon and the panels.
    pub(super) fn file_page_view(&self, page: FilePage, cx: &mut Context<Self>) -> AnyElement {
        let back = nav_item("file-back", lucide("chevron-left"), t!("file_page.back"), false, cx)
            .mb_4()
            .on_click(cx.listener(|this, _, _, cx| this.close_file_page(cx)));
        let pages = [
            (FilePage::Info, fluent("info"), t!("file_page.info")),
            (FilePage::New, fluent("document_add"), t!("file_page.new")),
            (FilePage::Open, lucide("folder-open"), t!("file_page.open")),
        ];
        let saving = [
            ("file-save", t!("file_page.save"), Box::new(SaveProject) as Box<dyn Action>),
            ("file-save-as", t!("file_page.save_as"), Box::new(SaveProjectAs)),
        ];
        let transfer = [
            (FilePage::Import, fluent("arrow_import"), t!("file_page.import")),
            (FilePage::Export, fluent("arrow_export_ltr"), t!("file_page.export")),
        ];
        let bottom = [
            (
                "file-settings",
                fluent("settings"),
                t!("file_page.settings"),
                Box::new(OpenSettings) as Box<dyn Action>,
            ),
            ("file-about", fluent("info"), t!("file_page.about"), Box::new(OpenAbout)),
            ("file-quit", fluent("dismiss"), t!("file_page.quit"), Box::new(Quit)),
        ];
        let mut nav = v_flex()
            .flex_none()
            .w(px(260.0))
            .h_full()
            .gap_1()
            .px_3()
            .py_4()
            .bg(shell::panel_background(cx))
            .border_r_1()
            .border_color(shell::panel_border(cx))
            .child(back);
        for (p, icon, label) in pages {
            let id = SharedString::from(format!("file-page-{p:?}"));
            nav = nav.child(nav_item(id, icon, label, p == page, cx).on_click(self.show(p, cx)));
        }
        for (id, label, action) in saving {
            nav = nav.child(
                nav_item(id, fluent("save"), label, false, cx).on_click(self.run(action, cx)),
            );
        }
        for (p, icon, label) in transfer {
            let id = SharedString::from(format!("file-page-{p:?}"));
            nav = nav.child(nav_item(id, icon, label, p == page, cx).on_click(self.show(p, cx)));
        }
        nav = nav.child(div().flex_1());
        for (id, icon, label, action) in bottom {
            nav = nav.child(nav_item(id, icon, label, false, cx).on_click(self.run(action, cx)));
        }
        let content = match page {
            FilePage::Info => self.info_page(cx),
            FilePage::New => self.new_page(cx),
            FilePage::Open => self.open_page(cx),
            FilePage::Import => self.import_page(cx),
            FilePage::Export => self.export_page(cx),
        };
        h_flex()
            .id("file-page")
            .size_full()
            .occlude()
            .text_color(cx.theme().foreground)
            .child(nav)
            .child(
                div()
                    .id("file-page-content")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .bg(shell::card_background(cx))
                    .px(px(56.0))
                    .py_8()
                    .child(content),
            )
            .into_any_element()
    }

    fn info_page(&self, cx: &mut Context<Self>) -> Div {
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let name = ws.project_name();
        let (mut diagrams, mut structograms, mut elements) = (0, 0, 0);
        for element in model.elements() {
            match element.kind() {
                ElementKind::Model => {}
                ElementKind::Diagram(_) => diagrams += 1,
                ElementKind::Structogram(_) => structograms += 1,
                _ => elements += 1,
            }
        }
        let path = match &ws.path {
            Some(path) => path.display().to_string(),
            None => t!("file_page.not_saved_yet").to_string(),
        };
        let state = if ws.doc.is_dirty() { t!("file_page.unsaved") } else { t!("file_page.saved") };
        let muted = cx.theme().muted_foreground;
        let counts = format!(
            "{}: {diagrams} · {}: {structograms} · {}: {elements}",
            t!("file_page.diagrams"),
            t!("file_page.structograms"),
            t!("file_page.elements"),
        );
        page(t!("file_page.info"))
            .child(section(t!("file_page.project"), cx))
            .child(div().text_lg().font_semibold().child(name))
            .child(div().text_sm().text_color(muted).child(path))
            .child(div().text_sm().mt_1().child(state.to_string()))
            .child(section(t!("file_page.contents"), cx))
            .child(div().child(counts))
    }

    fn new_page(&self, cx: &mut Context<Self>) -> Div {
        page(t!("file_page.new"))
            .child(
                card("new-project", fluent("document_add"), "new_project", true, cx)
                    .on_click(self.run(Box::new(NewProject), cx)),
            )
            .child(
                card("open-sample", lucide("book-open"), "sample", true, cx)
                    .on_click(self.run(Box::new(OpenSample), cx)),
            )
    }

    fn open_page(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let hover = cx.theme().secondary_hover;
        let mut list = v_flex().max_w(CARD_WIDTH).gap_0p5();
        if self.settings.recent.is_empty() {
            list = list.child(div().text_sm().text_color(muted).child(t!("file_page.no_recent")));
        }
        for (index, path) in self.settings.recent.clone().into_iter().enumerate() {
            let name =
                path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let folder = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("recent-{index}")))
                    .gap_4()
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .child(shell::icon(lucide("file")).size(px(20.0)).flex_none())
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().font_semibold().truncate().child(name))
                            .child(div().text_sm().text_color(muted).truncate().child(folder)),
                    )
                    .on_click(self.run(Box::new(OpenRecent { index }), cx)),
            );
        }
        page(t!("file_page.open"))
            .child(
                card("open-browse", lucide("folder-open"), "browse", true, cx)
                    .on_click(self.run(Box::new(OpenProject), cx)),
            )
            .child(section(t!("file_page.recent"), cx))
            .child(list)
    }

    fn import_page(&self, cx: &mut Context<Self>) -> Div {
        page(t!("file_page.import"))
            .child(
                card("import-astah", fluent("arrow_import"), "import_astah", true, cx)
                    .on_click(self.run(Box::new(ImportAstah), cx)),
            )
            .child(
                card("import-nsd", fluent("flowchart"), "import_structogram", true, cx)
                    .on_click(self.run(Box::new(ImportStructogram), cx)),
            )
            .child(
                card("import-sql", fluent("code"), "import_sql", true, cx)
                    .on_click(self.run(Box::new(ImportSql), cx)),
            )
            .child(section(t!("file_page.source"), cx))
            .children(SOURCE_LANGUAGES.map(|(language, id, label)| {
                // Files or a whole folder, searched through: a button for each.
                let button = |folder: bool, cx: &mut Context<Self>| {
                    let run = self.run(Box::new(ImportSource { language, folder }), cx);
                    let (suffix, label) = match folder {
                        false => ("files", t!("file_page.files")),
                        true => ("folder", t!("file_page.folder")),
                    };
                    Button::new(SharedString::from(format!("import-{id}-{suffix}")))
                        .outline()
                        .small()
                        .label(label)
                        .on_click(move |event, window, cx| run(event, window, cx))
                };
                card_frame(
                    SharedString::from(format!("import-{id}")),
                    icons::language_icon(language),
                    t!(label),
                    t!("file_page.source_note"),
                    cx,
                )
                .child(
                    h_flex().flex_none().gap_2().child(button(false, cx)).child(button(true, cx)),
                )
            }))
    }

    fn export_page(&self, cx: &mut Context<Self>) -> Div {
        let ws = self.workspace.read(cx);
        let shown = ws.active_diagram.is_some();
        let er = ws
            .active_diagram
            .and_then(|d| ws.doc.model().get(d))
            .is_some_and(|e| matches!(e.kind(), ElementKind::Diagram(p) if p.diagram == DiagramKind::EntityRelationship));
        let structogram = self.active_structogram(cx).is_some();
        let entry = |id: &'static str,
                     icon: SharedString,
                     key: &str,
                     enabled: bool,
                     action: Box<dyn Action>,
                     cx: &mut Context<Self>| {
            let card = card(id, icon, key, enabled, cx);
            if enabled { card.on_click(self.run(action, cx)) } else { card }
        };
        page(t!("file_page.export"))
            .child(entry(
                "export-svg",
                fluent("image"),
                "export_svg",
                shown,
                Box::new(ExportSvg),
                cx,
            ))
            .child(entry(
                "export-png",
                fluent("image"),
                "export_png",
                shown,
                Box::new(ExportPng),
                cx,
            ))
            .child(entry(
                "export-astah",
                fluent("arrow_export_ltr"),
                "export_astah",
                true,
                Box::new(ExportAstah),
                cx,
            ))
            .child(entry("export-sql", fluent("code"), "export_sql", er, Box::new(ExportSql), cx))
            .child(entry(
                "export-nsd",
                fluent("flowchart"),
                "export_structogram",
                structogram,
                Box::new(crate::nsd::ExportStructogram),
                cx,
            ))
    }
}

/// The languages the source import reads: what, the id of its card and its name's text.
const SOURCE_LANGUAGES: [(SourceLanguage, &str, &str); 4] = [
    (SourceLanguage::Java, "java", "menu.java"),
    (SourceLanguage::CSharp, "csharp", "menu.csharp"),
    (SourceLanguage::Cpp, "cpp", "menu.cpp"),
    (SourceLanguage::Python, "python", "menu.python"),
];

/// How wide cards and lists get on a wide window.
const CARD_WIDTH: Pixels = px(720.0);

/// A page: its title, large, and below it what `child` adds.
fn page(title: impl Into<SharedString>) -> Div {
    v_flex().gap_2().child(div().mb_2().text_size(px(28.0)).font_semibold().child(title.into()))
}

/// The heading of a section of a page, in the accent color.
fn section(title: impl Into<SharedString>, cx: &App) -> Div {
    let (_, accent) = crate::ribbon::selected_colors(cx);
    div().mt_4().text_lg().font_semibold().text_color(accent).child(title.into())
}

/// An entry of the column on the left: an icon and its label; the page shown has the pale blue
/// of a selected tab and a bar at its left edge.
fn nav_item(
    id: impl Into<ElementId>,
    icon: SharedString,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let (bg, text) = crate::ribbon::selected_colors(cx);
    let hover = cx.theme().secondary_hover;
    h_flex()
        .id(id)
        .relative()
        .flex_none()
        .h(px(40.0))
        .gap_3()
        .px_3()
        .rounded_md()
        .cursor_pointer()
        .child(shell::icon(icon).size(px(20.0)))
        .child(label.into())
        .when(selected, |this| {
            this.bg(bg).text_color(text).font_semibold().child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(10.0))
                    .bottom(px(10.0))
                    .w(px(3.0))
                    .rounded_full()
                    .bg(text),
            )
        })
        .when(!selected, |this| this.hover(move |s| s.bg(hover)))
}

/// The look of a card: an icon, the title and below it, muted, what it is about.
fn card_frame(
    id: impl Into<ElementId>,
    icon: SharedString,
    title: impl Into<SharedString>,
    note: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .max_w(CARD_WIDTH)
        .gap_4()
        .px_5()
        .py_3()
        .rounded(px(6.0))
        .border_1()
        .border_color(shell::panel_border(cx))
        .bg(shell::panel_background(cx))
        .child(shell::icon(icon).size(px(24.0)).flex_none())
        .child(v_flex().flex_1().min_w_0().child(div().font_semibold().child(title.into())).child(
            div().text_sm().text_color(cx.theme().muted_foreground).truncate().child(note.into()),
        ))
}

/// A command as a card, its title and note from the `file_page.<key>` and
/// `file_page.<key>_note` texts. A disabled one is faded and does nothing.
fn card(id: &'static str, icon: SharedString, key: &str, enabled: bool, cx: &App) -> Stateful<Div> {
    let hover = cx.theme().secondary_hover;
    let title = t!(format!("file_page.{key}")).to_string();
    let note = t!(format!("file_page.{key}_note")).to_string();
    card_frame(id, icon, title, note, cx)
        .when(enabled, |this| this.cursor_pointer().hover(move |s| s.bg(hover)))
        .when(!enabled, |this| this.opacity(0.5))
}
