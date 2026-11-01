//! The info dialog (the i in the title bar, File › About UmlCraft): the version and a check for
//! a newer one, where UmlCraft is at home on the web, and its licence.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, StyledExt, WindowExt, h_flex, v_flex,
};
use gpui_kit::*;
use rust_i18n::t;

use crate::icons;
use crate::main_view::{MainView, ManualUpdate, UpdateDownload};
use crate::update;

/// UmlCraft's web site.
pub const WEBSITE: &str = "https://umlcraft.com/";
/// Its source code and issues.
pub const REPOSITORY: &str = "https://github.com/udvlp/UmlCraft";

/// Opens the dialog for the window of `view`, unless a dialog is open already.
pub fn open(view: WeakEntity<MainView>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    // The builder runs again whenever the window is drawn, so the check's result shows.
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        let link = |id: &'static str, label: &'static str, url: &'static str| {
            Button::new(id).link().label(label).on_click(move |_, _, cx| cx.open_url(url))
        };
        let main = view.upgrade();
        let state =
            main.as_ref().map_or(ManualUpdate::Idle, |v| v.read(cx).manual_update().clone());
        let checker = view.clone();
        let check = Button::new("about-check")
            .small()
            .label(t!("about.check").to_string())
            .disabled(state == ManualUpdate::Checking)
            .on_click(move |_, window, cx| {
                checker.update(cx, |this, cx| this.check_for_update(true, window, cx)).ok();
            });
        let result = match &state {
            ManualUpdate::Idle => None,
            ManualUpdate::Checking => Some(div().child(t!("about.checking").to_string())),
            ManualUpdate::UpToDate => Some(div().child(t!("about.up_to_date").to_string())),
            ManualUpdate::Failed => Some(div().child(t!("about.check_failed").to_string())),
            ManualUpdate::Found(version) => {
                let url = update::release_url(version);
                Some(
                    div().child(
                        Button::new("about-new-version")
                            .link()
                            .label(t!("about.new_version", version = version).to_string())
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
                )
            }
        };
        // Installing it right away, where UmlCraft can do that itself.
        let install = match (&state, &main, update::install()) {
            (ManualUpdate::Found(version), Some(main), Some(_)) => {
                let main = main.read(cx);
                let running = matches!(main.update_download(), UpdateDownload::Running(_));
                let failed = main.update_download() == UpdateDownload::Failed;
                let label = main.update_label(version, t!("about.install").to_string());
                let (version, installer) = (version.clone(), view.clone());
                (!failed).then(|| {
                    Button::new("about-install")
                        .small()
                        .primary()
                        .label(label)
                        .disabled(running)
                        .on_click(move |_, window, cx| {
                            installer
                                .update(cx, |this, cx| {
                                    this.install_update(version.clone(), window, cx)
                                })
                                .ok();
                        })
                })
            }
            _ => None,
        };
        let updates = h_flex()
            .flex_wrap()
            .gap_2()
            .text_sm()
            .text_color(muted)
            .child(check)
            .children(result)
            .children(install);
        dialog
            .title(t!("about.title").to_string())
            .width(px(380.0))
            .child(
                v_flex()
                    .gap_3()
                    .pb_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(img(icons::LOGO).size(px(40.0)))
                            .child(
                                v_flex()
                                    .child(div().text_lg().font_semibold().child("UmlCraft"))
                                    .child(
                                        div().text_sm().text_color(muted).child(
                                            t!(
                                                "about.version",
                                                version = env!("CARGO_PKG_VERSION")
                                            )
                                            .to_string(),
                                        ),
                                    ),
                            ),
                    )
                    .child(updates)
                    .child(div().text_sm().child(t!("about.summary").to_string()))
                    .child(
                        v_flex()
                            .items_start()
                            .child(link("about-website", "umlcraft.com", WEBSITE))
                            .child(link(
                                "about-repository",
                                "github.com/udvlp/UmlCraft",
                                REPOSITORY,
                            ))
                            .child(
                                Button::new("about-privacy")
                                    .link()
                                    .label(t!("update.privacy").to_string())
                                    .on_click(|_, window, cx| {
                                        crate::privacy_dialog::open(window, cx)
                                    }),
                            ),
                    )
                    .child(
                        div().text_xs().text_color(muted).child(t!("about.license").to_string()),
                    ),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("about-close")
                        .primary()
                        .label(t!("about.close").to_string())
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
