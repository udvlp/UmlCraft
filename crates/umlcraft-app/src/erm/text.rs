//! Editing a text in place: a node's name, a free text or a line's label, in a field over it.
//! Enter (or leaving the field) takes the text over, Escape drops it. A free text, or a name
//! with line breaks, gets a field of several lines (leaving it takes it over). A label offers
//! the usual cardinalities beside the field.

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Sizable, h_flex, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::edit::{self, Item};
use umlcraft_erm::{Rect as DRect, hit};

use super::canvas::ErmCanvas;

/// The cardinalities offered beside a label's field.
const CARDINALITIES: [&str; 4] = ["[0,1]", "[1,1]", "[0,n]", "[1,n]"];

enum Field {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

pub struct TextEdit {
    item: Item,
    /// Where the text is, in the diagram.
    rect: DRect,
    original: String,
    field: Field,
    _subscription: Subscription,
}

impl ErmCanvas {
    /// Opens the field for the text of `item` (a line edits its label).
    pub(super) fn start_edit(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        let d = self.diagram();
        let (item, text, rect, lines) = match &item {
            Item::Node(id) => {
                let Some(n) = d.node(id) else { return };
                let lines = n.name.contains('\n');
                (item.clone(), n.name.clone(), umlcraft_erm::size::text_box(n), lines)
            }
            Item::Text(id) => {
                let Some(t) = d.texts.iter().find(|t| t.id == *id) else { return };
                (item.clone(), t.text.clone(), t.bounds, true)
            }
            Item::Link(id) | Item::Label(id) => {
                let Some(l) = d.link(id) else { return };
                let text = l.label.as_ref().map(|l| l.display()).unwrap_or_default();
                let rect = hit::label_boxes(d, l).0.or_else(|| {
                    let (a, b) = hit::ends(d, l)?;
                    let c = umlcraft_erm::Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                    Some(DRect::new(c.x - 20.0, c.y - 7.0, 40.0, 14.0))
                });
                (Item::Label(id.clone()), text, rect.unwrap_or_default(), false)
            }
        };
        let (field, subscription) = if lines {
            let state = cx.new(|cx| TextareaState::new(window, cx).default_value(text.clone()));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
            let sub = cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::Blur) {
                    this.commit_edit(window, cx);
                }
            });
            (Field::Area(state), sub)
        } else {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(text.clone()));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
            let sub = cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, window, cx| {
                if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    this.commit_edit(window, cx);
                }
            });
            (Field::Line(state), sub)
        };
        self.editing =
            Some(TextEdit { item, rect, original: text, field, _subscription: subscription });
        cx.notify();
    }

    /// Takes the text of the open field over, if one is open.
    pub(super) fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        let text = match &edit.field {
            Field::Line(s) => s.read(cx).value().to_string(),
            Field::Area(s) => s.read(cx).value().to_string(),
        };
        window.focus(&self.focus, cx);
        if text != edit.original {
            self.apply(cx, |d| edit::set_text(d, &edit.item, &text).map(|()| None));
        }
        cx.notify();
    }

    /// Closes the field without taking its text.
    pub(super) fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    /// Sets the label of `item` to `text` and closes the field (which may have closed
    /// already, when the button took the focus).
    fn choose_label(
        &mut self,
        item: &Item,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editing = None;
        window.focus(&self.focus, cx);
        let text = text.to_owned();
        self.apply(cx, |d| edit::set_text(d, item, &text).map(|()| None));
    }

    /// Where the field is on screen (relative to the canvas), and its width.
    fn field_place(&self, edit: &TextEdit) -> (Point<Pixels>, Pixels) {
        let r = edit.rect;
        let corner = self.to_area(umlcraft_erm::Point::new(r.x, r.y));
        let width = px((r.width as f32 * self.zoom).max(140.0));
        (corner, width)
    }

    /// Whether a click went into the open field.
    pub(super) fn over_text_field(&self, position: Point<Pixels>) -> bool {
        let Some(edit) = &self.editing else { return false };
        let (corner, width) = self.field_place(edit);
        let origin = self.area.get().origin;
        let height = match edit.field {
            Field::Line(_) => px(56.0),
            Field::Area(_) => px((edit.rect.height as f32 * self.zoom).max(60.0) + 8.0),
        };
        let b =
            Bounds::new(origin + corner - point(px(4.0), px(4.0)), size(width + px(8.0), height));
        b.contains(&position)
    }

    /// The open field, placed over its text.
    pub(super) fn text_field(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let edit = self.editing.as_ref()?;
        let (corner, width) = self.field_place(edit);
        let field = match &edit.field {
            Field::Line(s) => Input::new(s).xsmall().into_any_element(),
            Field::Area(s) => Textarea::new(s)
                .xsmall()
                .h(px((edit.rect.height as f32 * self.zoom).max(60.0)))
                .into_any_element(),
        };
        let label = matches!(edit.item, Item::Label(_)).then(|| edit.item.clone());
        let choices = label.map(|item| {
            let none = item.clone();
            h_flex()
                .gap_0p5()
                .children(CARDINALITIES.map(|c| {
                    let item = item.clone();
                    Button::new(SharedString::from(format!("erm-card-{c}")))
                        .xsmall()
                        .outline()
                        .label(c)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_label(&item, c, window, cx)
                        }))
                }))
                .child(
                    Button::new("erm-card-none")
                        .xsmall()
                        .outline()
                        .label(t!("erm.no_label").to_string())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.choose_label(&none, "", window, cx)
                        })),
                )
        });
        Some(
            v_flex()
                .absolute()
                .left(corner.x - px(4.0))
                .top(corner.y - px(4.0))
                .w(width + px(8.0))
                .gap_0p5()
                // Escape drops the text, before the field would only clear it.
                .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                    this.cancel_edit(window, cx);
                    cx.stop_propagation();
                }))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(field)
                .children(choices)
                .into_any_element(),
        )
    }
}
