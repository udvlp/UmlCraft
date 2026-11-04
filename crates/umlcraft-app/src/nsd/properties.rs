//! The structogram's part of the properties panel: the selected element's text, comment and
//! whether it is disabled, or the diagram's signature, comment and kind.
//!
//! Typing changes the structogram at once, so the drawing follows the text; what is typed into
//! one field in a row is one undo step, together with the insertion of the element it was
//! typed for. Shift+Enter and Ctrl+Enter give the keys back to the structogram.

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{ActiveTheme, Sizable, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::Id;
use umlcraft_nsd::{ElementKind, Keywords, RootKind, edit};

use super::kind_name;
use super::view::{EditText, NsdView, Shown, Target};
use crate::workspace::Workspace;

const ROOT_KINDS: [(RootKind, &str); 3] = [
    (RootKind::Program, "nsd.root_program"),
    (RootKind::Sub, "nsd.root_sub"),
    (RootKind::Includable, "nsd.root_includable"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Text,
    Comment,
}

/// The undo step typing goes into: the field and what it edits, the undo depth before the
/// step and after its last change.
struct Step {
    field: Field,
    target: Target,
    start: usize,
    end: usize,
}

pub struct NsdProperties {
    workspace: Entity<Workspace>,
    /// The editor of the structogram shown, if one is shown.
    editor: Option<Entity<NsdView>>,
    text: Entity<TextareaState>,
    comment: Entity<TextareaState>,
    /// The structogram and what of it the fields show.
    shown: Option<(Id, Shown)>,
    /// The document's revision after the last change typed here: the fields hold it already.
    own_revision: Option<u64>,
    step: Option<Step>,
    /// An element just inserted for typing: its target and the undo depth before and after
    /// the insertion, so that its first text joins that step.
    inserted: Option<(Target, usize, usize)>,
    _editor_subscriptions: Vec<Subscription>,
    _subscriptions: Vec<Subscription>,
}

/// A field's text as lines, without the line breaks it ends with.
fn lines(value: &str) -> Vec<String> {
    let v = value.replace("\r\n", "\n");
    v.trim_end_matches('\n').split('\n').map(str::to_owned).collect()
}

impl NsdProperties {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 12));
        let comment = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 8));
        let mut subscriptions = Vec::new();
        for (state, field) in [(&text, Field::Text), (&comment, Field::Comment)] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, ev: &InputEvent, _, cx| {
                    // Loading values emits no events, so `Change` means the user typed.
                    if matches!(ev, InputEvent::Change) {
                        this.typed(field, cx);
                    }
                },
            ));
        }
        // Shift+Enter and Ctrl+Enter leave the fields, which take Enter with any modifier as
        // a line break; so they are caught before them.
        let this = cx.entity().downgrade();
        subscriptions.push(cx.intercept_keystrokes(move |event, window, cx| {
            let k = &event.keystroke;
            let leaves = k.key == "enter"
                && (k.modifiers.shift || k.modifiers.control || k.modifiers.platform);
            if !leaves {
                return;
            }
            let Some(this) = this.upgrade() else { return };
            let this = this.read(cx);
            let in_field = [&this.text, &this.comment]
                .iter()
                .any(|f| f.read(cx).focus_handle(cx).is_focused(window));
            if let (true, Some(editor)) = (in_field, this.editor.clone()) {
                cx.stop_propagation();
                editor.read(cx).focus_handle(cx).focus(window, cx);
            }
        }));
        NsdProperties {
            workspace,
            editor: None,
            text,
            comment,
            shown: None,
            own_revision: None,
            step: None,
            inserted: None,
            _editor_subscriptions: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Follows the editor of the structogram shown, or none.
    pub fn show(
        &mut self,
        editor: Option<Entity<NsdView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor == editor {
            return;
        }
        self._editor_subscriptions = match &editor {
            Some(e) => vec![
                cx.observe_in(e, window, |this, _, window, cx| this.sync(window, cx)),
                cx.subscribe_in(e, window, |this, _, ev: &EditText, window, cx| {
                    this.edit(ev.inserted, window, cx)
                }),
            ],
            None => Vec::new(),
        };
        self.editor = editor;
        self.step = None;
        self.inserted = None;
        self.sync(window, cx);
    }

    /// Whether the panel shows the structogram's selection: something of it is selected, and
    /// the model tree has not selected anything else since.
    pub fn is_active(&self, cx: &App) -> bool {
        let selected = self.workspace.read(cx).selected_element;
        self.shown.as_ref().is_some_and(|(id, _)| selected.is_none_or(|s| s == *id))
    }

    /// What the panel's heading names: the element's kind, or the diagram.
    pub fn heading(&self) -> Option<String> {
        let (_, shown) = self.shown.as_ref()?;
        Some(match shown.kind {
            Some(kind) => kind_name(kind),
            None => t!("nsd.edit_diagram").to_string(),
        })
    }

    /// Loads what the editor has selected into the fields, unless they hold it already.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.editor.as_ref().and_then(|e| {
            let e = e.read(cx);
            Some((e.id(), e.shown()?))
        });
        let revision = self.workspace.read(cx).doc.revision();
        let same_target = match (&self.shown, &shown) {
            (Some((a, s)), Some((b, t))) => a == b && s.target == t.target,
            _ => false,
        };
        if !same_target {
            self.step = None;
        }
        // What was typed here is in the fields already; reloading would move the cursor.
        let typed_here = same_target && self.own_revision == Some(revision);
        if let Some((_, s)) = &shown
            && !typed_here
            && self.shown.as_ref().map(|(_, old)| old) != Some(s)
        {
            let (text, comment) = (s.text.join("\n"), s.comment.join("\n"));
            self.text.update(cx, |f, cx| f.set_value(text, window, cx));
            self.comment.update(cx, |f, cx| f.set_value(comment, window, cx));
        }
        if self.shown != shown {
            self.shown = shown;
            cx.notify();
        }
    }

    /// Puts the cursor into the text (the comment for an element without one); an element
    /// inserted for it has its first `?` selected, ready for typing over.
    fn edit(&mut self, inserted: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.sync(window, cx);
        let Some((_, shown)) = &self.shown else { return };
        let depth = self.workspace.read(cx).doc.undo_depth();
        self.inserted = inserted.map(|before| (shown.target.clone(), before, depth));
        let has_text = shown.kind.is_none_or(edit::has_text);
        if has_text {
            let question = inserted.and(shown.text.join("\n").find('?'));
            self.text.update(cx, |f, cx| {
                if let Some(at) = question {
                    f.set_selected_range(at..at + 1, cx);
                }
                f.focus(window, cx);
            });
        } else {
            self.comment.update(cx, |f, cx| f.focus(window, cx));
        }
    }

    /// Writes what was typed into `field` to the structogram, as part of the step typing
    /// into it began.
    fn typed(&mut self, field: Field, cx: &mut Context<Self>) {
        let (Some(editor), Some((_, shown))) = (self.editor.clone(), &self.shown) else {
            return;
        };
        let target = shown.target.clone();
        let state = match field {
            Field::Text => &self.text,
            Field::Comment => &self.comment,
        };
        let value = lines(&state.read(cx).value());
        let depth = self.workspace.read(cx).doc.undo_depth();
        let start = match (&self.step, &self.inserted) {
            (Some(s), _) if s.field == field && s.target == target && s.end == depth => s.start,
            (_, Some((t, before, after))) if *t == target && *after == depth => *before,
            _ => depth,
        };
        self.inserted = None;
        editor.update(cx, |e, cx| match field {
            Field::Text => e.set_text(&target, &value, cx),
            Field::Comment => e.set_comment(&target, &value, cx),
        });
        let (end, revision) = self.workspace.update(cx, |ws, cx| {
            ws.doc.squash_since(start, t!("edit.structogram"));
            cx.notify();
            (ws.doc.undo_depth(), ws.doc.revision())
        });
        self.step = Some(Step { field, target, start, end });
        self.own_revision = Some(revision);
    }
}

impl Render for NsdProperties {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = v_flex().gap_3();
        let (Some((_, shown)), Some(editor)) = (&self.shown, &self.editor) else { return body };
        let muted = cx.theme().muted_foreground;
        let label = |text: String| div().text_xs().text_color(muted).child(text);

        let (text_label, hint) = match shown.kind {
            None => (t!("nsd.signature"), None),
            Some(ElementKind::Case) => (t!("nsd.text"), Some(t!("nsd.case_hint"))),
            Some(ElementKind::Parallel) => (t!("nsd.branches"), None),
            Some(ElementKind::Try) => (t!("nsd.exception"), None),
            Some(ElementKind::For) => {
                let hint = match shown.keywords {
                    Keywords::German => t!("nsd.for_hint_german"),
                    Keywords::English => t!("nsd.for_hint"),
                };
                (t!("nsd.text"), Some(hint))
            }
            Some(_) => (t!("nsd.text"), None),
        };
        let mut body = body;
        if shown.kind.is_none_or(edit::has_text) {
            let mut field = v_flex()
                .gap_1()
                .child(label(text_label.to_string()))
                .child(Textarea::new(&self.text).small());
            if let Some(hint) = hint {
                field = field.child(div().text_xs().text_color(muted).child(hint.to_string()));
            }
            body = body.child(field);
        }
        body = body.child(
            v_flex()
                .gap_1()
                .child(label(t!("nsd.comment").to_string()))
                .child(Textarea::new(&self.comment).small()),
        );
        match &shown.target {
            Target::Root => {
                let current = ROOT_KINDS.iter().position(|(k, _)| *k == shown.root_kind);
                let editor = editor.clone();
                body.child(
                    RadioGroup::vertical("nsd-root-kind")
                        .children(ROOT_KINDS.map(|(_, key)| t!(key).to_string()))
                        .selected_index(current)
                        .on_change(move |&ix, _, cx| {
                            editor.update(cx, |e, cx| e.set_root_kind(ROOT_KINDS[ix].0, cx));
                        }),
                )
            }
            Target::Element(at) => {
                let (editor, at) = (editor.clone(), at.clone());
                body.child(
                    Checkbox::new("nsd-disabled")
                        .label(t!("nsd.disabled").to_string())
                        .checked(shown.disabled)
                        .on_click(move |&on, _, cx| {
                            editor.update(cx, |e, cx| e.set_disabled(&at, on, cx));
                        }),
                )
            }
        }
    }
}
