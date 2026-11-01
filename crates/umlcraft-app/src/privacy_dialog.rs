//! What the check for new versions tells GitHub, shown in the app itself: reading it must not
//! go online. The README's "Privacy" section says the same.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{StyledExt, WindowExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;

/// The paragraphs, each a heading and its text, by their keys under `privacy`.
const PARAGRAPHS: [&str; 5] = ["when", "what", "who", "basis", "responsible"];

/// Opens the dialog, above another one if it is opened from there (settings, About).
pub fn open(window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, |dialog, _, _| {
        let paragraphs = PARAGRAPHS.map(|key| {
            v_flex()
                .gap_1()
                .child(div().font_semibold().child(t!(format!("privacy.{key}_title")).to_string()))
                .child(div().child(t!(format!("privacy.{key}")).to_string()))
        });
        dialog
            .title(t!("privacy.title").to_string())
            .width(px(480.0))
            .child(
                v_flex()
                    .gap_3()
                    .pb_1()
                    .text_sm()
                    .child(div().child(t!("privacy.intro").to_string()))
                    .children(paragraphs),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("privacy-close")
                        .primary()
                        .label(t!("privacy.close").to_string())
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}
