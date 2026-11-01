//! The settings dialog (Tools › Settings …, Ctrl+,): language, theme, how classes show
//! their members and that they are abstract, the keywords of new structograms, and whether to
//! look for new versions, in one place. Every change applies at once and is saved, so the dialog
//! only has to be closed.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme, StyledExt, WindowExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;

use crate::main_view::MainView;
use crate::settings::LANGUAGES;

/// Opens the dialog on the settings of `view`, unless a dialog is open already.
pub fn open(view: WeakEntity<MainView>, window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        return;
    }
    // The builder runs again whenever the window is drawn, so the dialog always shows the
    // current settings, in the current language and theme.
    window.open_dialog(cx, move |dialog, _, cx| {
        let Some(main) = view.upgrade() else { return dialog };
        let settings = main.read(cx).settings();
        // 0 is the system's language, then those of `LANGUAGES`.
        let language = settings
            .language
            .as_deref()
            .and_then(|l| LANGUAGES.iter().position(|&(code, _)| code == l))
            .map_or(0, |ix| ix + 1);
        let column = settings.visibility_column;
        let stereotype = settings.abstract_stereotype;
        let read_only = settings.read_only_constraint;
        // 0 follows the user interface, then those of `LANGUAGES`.
        let keywords = settings
            .structogram_keywords
            .as_deref()
            .and_then(|l| LANGUAGES.iter().position(|&(code, _)| code == l))
            .map_or(0, |ix| ix + 1);
        let update_check = settings.update_check == Some(true);
        let dark = cx.theme().is_dark();
        let muted = cx.theme().muted_foreground;

        let (v1, v2, v3, v4) = (view.clone(), view.clone(), view.clone(), view.clone());
        let (v5, v6, v7) = (view.clone(), view.clone(), view.clone());
        let languages = RadioGroup::horizontal("settings-language")
            .child(t!("settings.system_language").to_string())
            .children(LANGUAGES.map(|(_, name)| name))
            .selected_index(Some(language))
            .on_change(move |&ix, window, cx| {
                let chosen = ix.checked_sub(1).and_then(|i| LANGUAGES.get(i));
                let chosen = chosen.map(|&(code, _)| code.to_owned());
                v1.update(cx, |this, cx| {
                    if this.settings().language != chosen {
                        this.set_language(chosen, window, cx);
                    }
                })
                .ok();
            });
        let theme = RadioGroup::horizontal("settings-theme")
            .children([t!("settings.light").to_string(), t!("settings.dark").to_string()])
            .selected_index(Some(usize::from(dark)))
            .on_change(move |&ix, window, cx| {
                let dark = ix == 1;
                v2.update(cx, |this, cx| {
                    if cx.theme().is_dark() != dark {
                        this.set_dark(dark, window, cx);
                    }
                })
                .ok();
            });
        let visibility_column = v_flex()
            .gap_1()
            .child(
                Switch::new("settings-visibility-column")
                    .checked(column)
                    .label(t!("settings.visibility_column").to_string())
                    .on_change(move |&on, _, cx| {
                        v3.update(cx, |this, cx| this.set_visibility_column(on, cx)).ok();
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("settings.visibility_column_detail").to_string()),
            );
        let abstract_mark = v_flex()
            .gap_1()
            .child(div().text_sm().child(t!("settings.abstract").to_string()))
            .child(
                RadioGroup::horizontal("settings-abstract")
                    .children([
                        t!("settings.abstract_keyword").to_string(),
                        t!("settings.abstract_stereotype").to_string(),
                    ])
                    .selected_index(Some(usize::from(stereotype)))
                    .on_change(move |&ix, _, cx| {
                        let on = ix == 1;
                        v4.update(cx, |this, cx| {
                            if this.settings().abstract_stereotype != on {
                                this.set_abstract_stereotype(on, cx);
                            }
                        })
                        .ok();
                    }),
            );
        let read_only_mark = Switch::new("settings-read-only")
            .checked(read_only)
            .label(t!("settings.read_only").to_string())
            .on_change(move |&on, _, cx| {
                v5.update(cx, |this, cx| this.set_read_only_constraint(on, cx)).ok();
            });
        let classes =
            v_flex().gap_3().child(visibility_column).child(abstract_mark).child(read_only_mark);
        let structograms = v_flex()
            .gap_1()
            .child(div().text_sm().child(t!("settings.keywords").to_string()))
            .child(
                RadioGroup::vertical("settings-keywords")
                    .child(t!("settings.keywords_ui").to_string())
                    .children([
                        t!("settings.keywords_de").to_string(),
                        t!("settings.keywords_en").to_string(),
                    ])
                    .selected_index(Some(keywords))
                    .on_change(move |&ix, _, cx| {
                        let chosen = ix.checked_sub(1).and_then(|i| LANGUAGES.get(i));
                        let chosen = chosen.map(|&(code, _)| code.to_owned());
                        v6.update(cx, |this, cx| {
                            if this.settings().structogram_keywords != chosen {
                                this.set_structogram_keywords(chosen, cx);
                            }
                        })
                        .ok();
                    }),
            );
        let updates = v_flex()
            .gap_1()
            .child(
                Switch::new("settings-update-check")
                    .checked(update_check)
                    .label(t!("settings.update_check").to_string())
                    .on_change(move |&on, window, cx| {
                        v7.update(cx, |this, cx| this.set_update_check(on, window, cx)).ok();
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("settings.update_check_detail").to_string()),
            )
            .child(
                Button::new("settings-privacy")
                    .link()
                    .label(t!("update.privacy").to_string())
                    .on_click(|_, window, cx| crate::privacy_dialog::open(window, cx)),
            );

        dialog
            .title(t!("settings.title").to_string())
            .width(px(440.0))
            .child(
                v_flex()
                    .gap_5()
                    .pb_1()
                    .child(section(t!("settings.language").to_string(), languages))
                    .child(section(t!("settings.theme").to_string(), theme))
                    .child(section(t!("settings.classes").to_string(), classes))
                    .child(section(t!("settings.structograms").to_string(), structograms))
                    .child(section(t!("settings.updates").to_string(), updates)),
            )
            .footer(
                DialogFooter::new().child(
                    Button::new("settings-close")
                        .primary()
                        .label(t!("settings.close").to_string())
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                ),
            )
    });
}

/// A heading with what it sets below.
fn section(title: String, body: impl IntoElement) -> impl IntoElement {
    v_flex().gap_2().child(div().text_sm().font_semibold().child(title)).child(body)
}
