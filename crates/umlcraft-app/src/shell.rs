//! The window's frame: the backdrop the panels float on, the cards they are drawn as, and
//! buttons that show an icon.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Icon, Sizable};
use gpui_kit::*;

/// The part of the window, in window pixels, that the floating panels leave free over the
/// diagram view that fills the window; `None` until the first layout. The main view writes it
/// on every frame, the diagram views read it to zoom, center and place what they show.
pub type ClearArea = std::rc::Rc<std::cell::Cell<Option<Bounds<Pixels>>>>;

/// `clear` inside `bounds`, relative to `bounds`; all of `bounds` if nothing is left free there.
pub fn clear_in(clear: &ClearArea, bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    let all = Bounds::new(Point::default(), bounds.size);
    match clear.get().map(|c| c.intersect(&bounds)) {
        Some(c) if c.size.width > px(40.0) && c.size.height > px(40.0) => {
            Bounds::new(c.origin - bounds.origin, c.size)
        }
        _ => all,
    }
}

/// Space between the cards and around them.
pub const GAP: Pixels = px(8.0);
pub const RADIUS: Pixels = px(10.0);

/// The surface the cards float on: a grey under white cards in the light theme; in the dark
/// one the theme's background, with the cards a shade lighter (`card_background`).
///
/// Always opaque. A system backdrop (Mica, the blurred desktop) was tried and dropped: with a
/// non-opaque window GPUI renders text without ClearType, and on Windows its antialiased
/// edges come out too dark over the backdrop.
pub fn backdrop(cx: &App) -> Hsla {
    let theme = cx.theme();
    if theme.is_dark() {
        theme.background
    } else {
        theme.background.blend(theme.foreground.opacity(0.07))
    }
}

/// The title bar: a grey strip of its own above the diagram, #d8d8d8 in the light theme,
/// #303030 in the dark one.
pub fn title_bar_background(cx: &App) -> Hsla {
    if cx.theme().is_dark() { rgb(0x303030).into() } else { rgb(0xd8d8d8).into() }
}

pub fn card_background(cx: &App) -> Hsla {
    let theme = cx.theme();
    if theme.is_dark() { theme.background.blend(white().opacity(0.06)) } else { theme.background }
}

/// A panel floating over the diagram: a very light grey (#f8f8f8) over the white diagram in
/// the light theme, a shade lighter than the diagram in the dark one.
pub fn floating_background(cx: &App) -> Hsla {
    if cx.theme().is_dark() { card_background(cx) } else { rgb(0xf8f8f8).into() }
}

fn shadow(cx: &App, alpha: f32, y: f32, blur: f32) -> BoxShadow {
    BoxShadow {
        color: black().opacity(if cx.theme().is_dark() { alpha * 3.0 } else { alpha }),
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
        inset: false,
    }
}

/// A rounded panel with a hairline border and a soft shadow: a tight, darker one at the edge
/// and a wide, faint one.
pub fn card(cx: &App) -> Div {
    div()
        .bg(card_background(cx))
        .rounded(RADIUS)
        .border_1()
        .border_color(cx.theme().border.opacity(0.8))
        .shadow(vec![shadow(cx, 0.08, 1.0, 2.0), shadow(cx, 0.06, 2.0, 8.0)])
}

/// A card floating over the diagram, with a narrow, fairly dark shadow that sets it off the
/// white; it keeps the mouse from the diagram below.
pub fn floating_card(cx: &App) -> Div {
    div()
        .occlude()
        .bg(floating_background(cx))
        .rounded(RADIUS)
        .border_1()
        .border_color(cx.theme().border.opacity(0.9))
        .shadow(vec![shadow(cx, 0.16, 1.0, 2.0), shadow(cx, 0.12, 2.0, 6.0)])
}

/// The context menu of a diagram view, opened on releasing the right button: where, in window
/// pixels, and the menu once built (on the next render, which has the window).
pub struct MenuAt {
    pub position: Point<Pixels>,
    pub menu: Option<(Entity<gpui_kit::component::menu::PopupMenu>, Subscription)>,
    /// The menu is being built after the frame: its builder reads the view, which must not be
    /// in the middle of an update (its render) then.
    pub building: bool,
}

impl MenuAt {
    pub fn new(position: Point<Pixels>) -> Self {
        Self { position, menu: None, building: false }
    }

    /// The built menu at its position, above everything else.
    pub fn overlay(&self) -> Option<AnyElement> {
        let (menu, _) = self.menu.as_ref()?;
        let menu = anchored()
            .position(self.position)
            .snap_to_window_with_margin(px(8.0))
            .child(menu.clone());
        Some(deferred(menu).with_priority(1).into_any_element())
    }
}

/// An icon from the app's assets, e.g. `icons::fluent("save")`.
pub fn icon(path: impl Into<SharedString>) -> Icon {
    Icon::empty().path(path)
}

/// A borderless button with an icon that dispatches `action`; `tooltip` names it.
/// Wraps a button of the title bar. To Windows the whole bar is the caption: a press nobody
/// handles starts moving the window, and the button never sees the release. Handling the press
/// here keeps it in the app; the button below has seen it already.
pub fn in_title_bar(button: impl IntoElement) -> Div {
    div().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(button)
}

pub fn icon_button(
    id: impl Into<ElementId>,
    path: impl Into<SharedString>,
    tooltip: impl Into<SharedString>,
    action: Box<dyn Action>,
) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon(path))
        .tooltip(tooltip)
        .on_click(move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx))
}
