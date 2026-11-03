//! UmlCraft's own message dialog, in place of the system's: every `window.prompt` (errors,
//! the question before unsaved changes are discarded, …) shows it on every platform, in the
//! app's theme. GPUI's fallback on Linux is 288 px wide and cuts off what does not fit on
//! one line; this one wraps its text and scrolls a long detail.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, StyledExt, h_flex, v_flex};
use gpui_kit::*;

/// Shows the prompts of every window with [`build`] instead of the system's dialogs.
pub fn init(cx: &mut App) {
    cx.set_prompt_builder(build);
}

/// A prompt view for `window.prompt`'s arguments.
fn build(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let view = cx.new(|cx| MessageDialog {
        level,
        message: message.into(),
        detail: detail.filter(|d| !d.is_empty()).map(SharedString::from),
        buttons: buttons.iter().map(|b| b.label().clone()).collect(),
        cancel: cancel_index(buttons),
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}

/// The answer Escape gives: a cancel button's, else the last one's (the dialogs list Cancel
/// last, and a lone OK only acknowledges).
fn cancel_index(buttons: &[PromptButton]) -> usize {
    buttons.iter().position(PromptButton::is_cancel).unwrap_or(buttons.len().saturating_sub(1))
}

struct MessageDialog {
    level: PromptLevel,
    message: SharedString,
    detail: Option<SharedString>,
    buttons: Vec<SharedString>,
    cancel: usize,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for MessageDialog {}

impl Focusable for MessageDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MessageDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let title_color = match self.level {
            PromptLevel::Critical => theme.danger,
            _ => theme.foreground,
        };
        // The first button is the one Enter presses, as in the system's dialogs.
        let buttons = self.buttons.iter().enumerate().map(|(ix, label)| {
            let button = Button::new(("prompt-button", ix)).label(label.clone()).on_click(
                cx.listener(move |_, _, _, cx| {
                    cx.emit(PromptResponse(ix));
                    cx.stop_propagation();
                }),
            );
            if ix == 0 { button.primary() } else { button }
        });
        let card = v_flex()
            .id("prompt")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let answer = match event.keystroke.key.as_str() {
                    "enter" => Some(0),
                    "escape" => Some(this.cancel),
                    _ => None,
                };
                if let Some(answer) = answer.filter(|_| !this.buttons.is_empty()) {
                    cx.emit(PromptResponse(answer));
                    cx.stop_propagation();
                }
            }))
            .occlude()
            .w(px(420.0))
            .gap_3()
            .p_5()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius_lg)
            .shadow_lg()
            .child(
                div().w_full().font_semibold().text_color(title_color).child(self.message.clone()),
            )
            .children(self.detail.clone().map(|detail| {
                div()
                    .id("prompt-detail")
                    .w_full()
                    .max_h(px(320.0))
                    .overflow_y_scroll()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(detail)
            }))
            .child(h_flex().w_full().pt_2().justify_end().gap_2().children(buttons));
        div().size_full().flex().items_center().justify_center().bg(theme.overlay).child(card)
    }
}

#[cfg(test)]
mod tests {
    use super::cancel_index;
    use gpui_kit::PromptButton;

    #[test]
    fn escape_answers_with_cancel_or_the_last_button() {
        let save = PromptButton::new("Save");
        let discard = PromptButton::new("Discard");
        assert_eq!(cancel_index(&[save.clone(), discard.clone(), PromptButton::new("x")]), 2);
        assert_eq!(cancel_index(&[PromptButton::cancel("Cancel"), save.clone(), discard]), 0);
        assert_eq!(cancel_index(&[PromptButton::ok("OK")]), 0);
        assert_eq!(cancel_index(&[]), 0);
    }
}
