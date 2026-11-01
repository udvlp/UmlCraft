//! The element editor: one dialog for an element's text, comment and whether it is disabled,
//! and for the diagram's signature, comment and kind. A new element is only inserted when the
//! dialog is confirmed, so cancelling leaves nothing to undo. Like Structorizer's, the dialog
//! selects the first `?` of a default text. Enter breaks the line; Shift+Enter and Ctrl+Enter
//! confirm, so the dialog can be left without the mouse.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, WindowExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_nsd::path::{Path, Slot};
use umlcraft_nsd::{ElementKind, RootKind};

use super::kind_name;
use super::view::NsdView;

/// What the dialog edits.
#[derive(Clone, Debug)]
pub enum Target {
    Element(Path),
    /// A new element of `kind`, inserted at `slot` on OK.
    New(Slot, ElementKind),
    Root,
}

/// What the dialog hands back on OK.
#[derive(Clone)]
pub struct Edited {
    pub text: Vec<String>,
    pub comment: Vec<String>,
    pub disabled: bool,
    pub kind: RootKind,
}

const ROOT_KINDS: [(RootKind, &str); 3] = [
    (RootKind::Program, "nsd.root_program"),
    (RootKind::Sub, "nsd.root_sub"),
    (RootKind::Includable, "nsd.root_includable"),
];

/// What the open dialog would hand back on OK, or `None` while it holds what it started with.
pub type Pending = Rc<dyn Fn(&App) -> Option<(Target, Edited)>>;

/// A field's text as lines, without the line breaks it ends with.
fn lines(value: &str) -> Vec<String> {
    let v = value.replace("\r\n", "\n");
    v.trim_end_matches('\n').split('\n').map(str::to_owned).collect()
}

/// Opens the dialog for `target` (an element of `element_kind`, or the diagram for `None`)
/// with the values it starts from; the hints name the structogram's `keywords`. `focus` is
/// the structogram's, which gets the focus back. Gives what the dialog holds, for closing and
/// quitting while it is open; `None` when another dialog is open already.
#[allow(clippy::too_many_arguments)]
pub fn open(
    view: WeakEntity<NsdView>,
    focus: &FocusHandle,
    target: Target,
    element_kind: Option<ElementKind>,
    keywords: umlcraft_nsd::Keywords,
    start: Edited,
    window: &mut Window,
    cx: &mut App,
) -> Option<Pending> {
    if window.has_active_dialog(cx) {
        return None;
    }
    let has_text = element_kind != Some(ElementKind::Forever);
    let text_value = start.text.join("\n");
    // A new element's default text has its first gap selected, ready for typing.
    let question = matches!(target, Target::New(..)).then(|| text_value.find('?')).flatten();
    let text = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 12));
    let comment = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
    text.update(cx, |s, cx| s.set_value(text_value, window, cx));
    comment.update(cx, |s, cx| s.set_value(start.comment.join("\n"), window, cx));
    // Closing gives the focus back to what had it on opening. That must be the structogram
    // (`focus`), not the toolbar button that opened the dialog nor the dialog's own fields,
    // which are gone then and would leave keys and toolbar without a target. The fields get
    // the focus once the dialog is open.
    window.focus(focus, cx);
    let (first_text, first_comment) = (text.clone(), comment.clone());
    let disabled = Rc::new(Cell::new(start.disabled));
    let kind = Rc::new(Cell::new(start.kind));
    let collect: Rc<dyn Fn(&App) -> Edited> = {
        let (text, comment, disabled, kind) =
            (text.clone(), comment.clone(), disabled.clone(), kind.clone());
        Rc::new(move |cx| Edited {
            text: lines(&text.read(cx).value()),
            comment: lines(&comment.read(cx).value()),
            disabled: disabled.get(),
            kind: kind.get(),
        })
    };
    let pending: Pending = {
        let (collect, target) = (collect.clone(), target.clone());
        // Compared as the fields hold them, so that an untouched dialog counts as unchanged.
        let (text, comment) = (lines(&start.text.join("\n")), lines(&start.comment.join("\n")));
        let (was_disabled, was_kind) = (start.disabled, start.kind);
        Rc::new(move |cx| {
            let now = collect(cx);
            let changed = now.text != text
                || now.comment != comment
                || now.disabled != was_disabled
                || now.kind != was_kind;
            changed.then(|| (target.clone(), now))
        })
    };

    let title = match (&target, element_kind) {
        (Target::Root, _) => t!("nsd.edit_diagram").to_string(),
        (_, Some(k)) => kind_name(k),
        _ => String::new(),
    };
    let (text_label, hint) = match element_kind {
        None => (t!("nsd.signature"), None),
        Some(ElementKind::Case) => (t!("nsd.text"), Some(t!("nsd.case_hint"))),
        Some(ElementKind::Parallel) => (t!("nsd.branches"), None),
        Some(ElementKind::Try) => (t!("nsd.exception"), None),
        Some(ElementKind::For) => {
            let hint = match keywords {
                umlcraft_nsd::Keywords::German => t!("nsd.for_hint_german"),
                umlcraft_nsd::Keywords::English => t!("nsd.for_hint"),
            };
            (t!("nsd.text"), Some(hint))
        }
        Some(_) => (t!("nsd.text"), None),
    };

    let ok = {
        let (target, collect) = (target.clone(), collect.clone());
        // Once only: the dialog stays a moment while it closes.
        let done = Cell::new(false);
        Rc::new(move |window: &mut Window, cx: &mut App| {
            if done.replace(true) {
                return;
            }
            let edited = collect(cx);
            let target = target.clone();
            view.update(cx, |v, cx| v.apply_dialog(target, edited, window, cx)).ok();
            window.close_dialog(cx);
        })
    };
    // The text fields take Enter with any modifier as a line break, so the keys that confirm
    // are caught before them. The dialog's builder holds the subscription: it ends with it.
    let confirm_keys = {
        let (ok, handle) = (ok.clone(), window.window_handle());
        Rc::new(cx.intercept_keystrokes(move |event, window, cx| {
            let k = &event.keystroke;
            let confirms = k.key == "enter"
                && (k.modifiers.shift || k.modifiers.control || k.modifiers.platform);
            if confirms && window.window_handle() == handle && window.has_active_dialog(cx) {
                cx.stop_propagation();
                ok(window, cx);
            }
        }))
    };

    window.open_dialog(cx, move |dialog, _, cx| {
        let _keys = &confirm_keys;
        let muted = cx.theme().muted_foreground;
        let (ok, ok_click) = (ok.clone(), ok.clone());

        let mut body = v_flex().gap_3().pb_1();
        if has_text {
            let mut field = v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(text_label.to_string()))
                .child(Textarea::new(&text).small());
            if let Some(hint) = &hint {
                field = field.child(div().text_xs().text_color(muted).child(hint.to_string()));
            }
            body = body.child(field);
        }
        body = body.child(
            v_flex()
                .gap_1()
                .child(div().text_sm().font_semibold().child(t!("nsd.comment").to_string()))
                .child(Textarea::new(&comment).small()),
        );
        if matches!(target, Target::Root) {
            let current = ROOT_KINDS.iter().position(|(k, _)| *k == kind.get());
            let kind = kind.clone();
            body = body.child(
                RadioGroup::horizontal("nsd-root-kind")
                    .children(ROOT_KINDS.map(|(_, key)| t!(key).to_string()))
                    .selected_index(current)
                    .on_change(move |&ix, window, _| {
                        kind.set(ROOT_KINDS[ix].0);
                        window.refresh();
                    }),
            );
        } else {
            let disabled = disabled.clone();
            body = body.child(
                Checkbox::new("nsd-disabled")
                    .label(t!("nsd.disabled").to_string())
                    .checked(disabled.get())
                    .on_click(move |&on, window, _| {
                        disabled.set(on);
                        window.refresh();
                    }),
            );
        }

        dialog
            .title(title.clone())
            .width(px(480.0))
            .child(body)
            .on_ok(move |_, window, cx| {
                ok(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("nsd-dialog-cancel")
                            .label(t!("dialog.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("nsd-dialog-ok")
                            .primary()
                            .label(t!("dialog.ok").to_string())
                            .on_click(move |_, window, cx| ok_click(window, cx)),
                    ),
            )
    });
    if has_text {
        first_text.update(cx, |s, cx| {
            if let Some(at) = question {
                s.set_selected_range(at..at + 1, cx);
            }
            s.focus(window, cx);
        });
    } else {
        first_comment.update(cx, |s, cx| s.focus(window, cx));
    }
    Some(pending)
}
