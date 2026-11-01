//! The info dialog (the i in the title bar, File › About UmlCraft): the version, where
//! UmlCraft is at home on the web, and its licence.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme, StyledExt, WindowExt, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::icons;

/// UmlCraft's web site.
pub const WEBSITE: &str = "https://umlcraft.com/";
/// Its source code and issues.
pub const REPOSITORY: &str = "https://github.com/udvlp/UmlCraft";

/// Opens the dialog, unless a dialog is open already.
pub fn open(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    window.open_dialog(cx, move |dialog, _, cx| {
        let muted = cx.theme().muted_foreground;
        let link = |id: &'static str, label: &'static str, url: &'static str| {
            Button::new(id).link().label(label).on_click(move |_, _, cx| cx.open_url(url))
        };
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
                    .child(div().text_sm().child(t!("about.summary").to_string()))
                    .child(
                        v_flex()
                            .items_start()
                            .child(link("about-website", "umlcraft.com", WEBSITE))
                            .child(link(
                                "about-repository",
                                "github.com/udvlp/UmlCraft",
                                REPOSITORY,
                            )),
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
