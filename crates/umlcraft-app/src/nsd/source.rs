//! The structograms of a source import (Tools › Import …): the dialog where the methods are
//! chosen (grouped by file and class, with a search field, none ticked) and the texts of the
//! import's report. Parsing, building and filing them is `umlcraft_nsd_code::import`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, StyledExt, WindowExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_nsd_code::import::paired;
pub use umlcraft_nsd_code::import::{
    Built, Found, Imported, build, file_structograms, imported, parse, skipped,
};
use umlcraft_nsd_code::{MethodRef, NoteKind, Report, SourceLanguage};

/// At most this many methods are listed at once; the search finds the others.
const LISTED: usize = 300;

/// The dialog listing the methods of `found`: those whose structogram the project has are
/// ticked, those edited by hand ticked and locked. `chosen` gets the methods ticked (indices
/// into `found.methods`) when the import goes on; cancelling ends the import.
pub fn offer(
    found: &Found,
    existing: &[Imported],
    chosen: impl Fn(Vec<usize>, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    struct Row {
        file: String,
        method: MethodRef,
        locked: bool,
    }
    let mut ticked = BTreeSet::new();
    let pairs = paired(found, existing, |_, _| false);
    let rows: Rc<Vec<Row>> = Rc::new(
        found
            .methods
            .iter()
            .enumerate()
            .map(|(i, (file, method))| {
                let state = pairs[i].map(|k| existing[k].untouched);
                if state.is_some() {
                    ticked.insert(i);
                }
                let file = found.shown(*file);
                Row { file, method: method.clone(), locked: state == Some(false) }
            })
            .collect(),
    );
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("nsd.source_search")));
    let first_search = search.clone();
    let ticked = Rc::new(RefCell::new(ticked));
    let chosen = Rc::new(chosen);

    window.open_dialog(cx, move |dialog, _, cx| {
        let filter = search.read(cx).value().to_lowercase();
        let muted = cx.theme().muted_foreground;
        let mut list = v_flex().gap_0p5();
        let mut heading: Option<(&str, String)> = None;
        let mut shown = 0;
        let mut more = 0;
        for (i, row) in rows.iter().enumerate() {
            let m = &row.method;
            let label = format!("{} {} {}", row.file, m.class_name(), m.signature).to_lowercase();
            if !filter.is_empty() && !label.contains(&filter) {
                continue;
            }
            if shown == LISTED {
                more += 1;
                continue;
            }
            shown += 1;
            let class = m.class_name();
            if heading.as_ref() != Some(&(row.file.as_str(), class.clone())) {
                // Functions outside classes (Python, C++) stand under the file's name.
                let text = match m.class.is_empty() {
                    true => row.file.clone(),
                    false => format!("{class}  ·  {}", row.file),
                };
                list = list.child(div().pt_2().text_sm().font_semibold().child(text));
                heading = Some((row.file.as_str(), class));
            }
            let checked = ticked.borrow().contains(&i);
            let toggle = ticked.clone();
            let mut line = div()
                .id(("source-method", i))
                .pl_3()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    Checkbox::new(("source-method-check", i))
                        .checked(checked)
                        .disabled(row.locked)
                        .on_click(move |&on, window, _| {
                            let mut set = toggle.borrow_mut();
                            if on {
                                set.insert(i);
                            } else {
                                set.remove(&i);
                            }
                            window.refresh();
                        }),
                )
                .child(div().text_sm().child(m.signature.clone()))
                .child(div().text_xs().text_color(muted).child(format!("{}", m.line)));
            if row.locked {
                line = line.child(
                    div().text_xs().text_color(muted).child(t!("nsd.source_edited").to_string()),
                );
            }
            list = list.child(line);
        }
        if more > 0 {
            list = list.child(
                div()
                    .pt_2()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("nsd.source_more", count = more).to_string()),
            );
        }
        let go = {
            let (ticked, chosen) = (ticked.clone(), chosen.clone());
            move |window: &mut Window, cx: &mut App| {
                window.close_dialog(cx);
                let picked = ticked.borrow().iter().copied().collect();
                chosen(picked, window, cx);
            }
        };
        let go_click = go.clone();
        dialog
            .title(t!("nsd.source_title").to_string())
            .width(px(600.0))
            .child(
                v_flex()
                    .gap_2()
                    .child(Input::new(&search).small())
                    .child(
                        div().text_xs().text_color(muted).child(t!("nsd.source_hint").to_string()),
                    )
                    .child(
                        div().id("source-methods").max_h(px(380.0)).overflow_y_scroll().child(list),
                    ),
            )
            .on_ok(move |_, window, cx| {
                go(window, cx);
                false
            })
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("source-cancel")
                            .label(t!("dialog.cancel").to_string())
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("source-import")
                            .primary()
                            .label(t!("nsd.source_import").to_string())
                            .on_click(move |_, window, cx| go_click(window, cx)),
                    ),
            )
    });
    // Only now: closing gives the focus back to what had it on opening, which must not be
    // the search field.
    first_search.update(cx, |s, cx| s.focus(window, cx));
}

/// How a structogram's file header names where it came from.
pub fn made_origin(language: SourceLanguage) -> String {
    format!("{} / {}", super::origin(), language.name())
}

/// The report in the UI's language, one line per note.
pub fn notes(report: &Report) -> Vec<String> {
    report
        .notes
        .iter()
        .map(|n| {
            let what = match n.kind {
                NoteKind::Continue => t!("nsd.note_continue"),
                NoteKind::Label => t!("nsd.note_label"),
                NoteKind::LocalClass => t!("nsd.note_local_class"),
                NoteKind::Synchronized => t!("nsd.note_synchronized"),
                NoteKind::Fallthrough => t!("nsd.note_fallthrough"),
                NoteKind::SyntaxError => t!("nsd.note_syntax_error"),
                NoteKind::Using => t!("nsd.note_using"),
                NoteKind::Lock => t!("nsd.note_lock"),
                NoteKind::Yield => t!("nsd.note_yield"),
                NoteKind::Goto => t!("nsd.note_goto"),
                NoteKind::LocalFunction => t!("nsd.note_local_function"),
                NoteKind::Conditional => t!("nsd.note_conditional_branch"),
                NoteKind::With => t!("nsd.note_with"),
                NoteKind::LoopElse => t!("nsd.note_loop_else"),
                NoteKind::TryElse => t!("nsd.note_try_else"),
                NoteKind::NestedFunction => t!("nsd.note_nested_function"),
                NoteKind::CaseBreak => t!("nsd.note_case_break"),
            };
            t!("nsd.note_line", line = n.line, what = what).to_string()
        })
        .collect()
}
