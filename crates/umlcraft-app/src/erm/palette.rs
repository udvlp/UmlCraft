//! The palette on the left: the shapes of an ER diagram, each shown as our own drawing of it.
//! A click puts one into the middle of the view (an attribute goes to the selected node), a
//! drag drops it where the mouse lets go.

use std::sync::Arc;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::{Diagram, Key, NodeKind, Point};

use super::PlaceShape;
use crate::nsd::{OPTIONS, rasterize};

/// What the palette offers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Shape {
    Entity,
    WeakEntity,
    AssociativeEntity,
    Attribute,
    KeyAttribute,
    PartialKey,
    DerivedAttribute,
    MultivaluedAttribute,
    Relationship,
    IdentifyingRelationship,
    IsA,
    Text,
}

impl Shape {
    pub const ALL: [Shape; 12] = [
        Self::Entity,
        Self::WeakEntity,
        Self::AssociativeEntity,
        Self::Attribute,
        Self::KeyAttribute,
        Self::PartialKey,
        Self::DerivedAttribute,
        Self::MultivaluedAttribute,
        Self::Relationship,
        Self::IdentifyingRelationship,
        Self::IsA,
        Self::Text,
    ];

    /// The node it makes; `None` for a free text.
    pub fn kind(self) -> Option<NodeKind> {
        let attribute =
            |key, derived, multivalued| NodeKind::Attribute { key, derived, multivalued };
        Some(match self {
            Self::Entity => NodeKind::entity(),
            Self::WeakEntity => NodeKind::Entity { weak: true },
            Self::AssociativeEntity => NodeKind::AssociativeEntity,
            Self::Attribute => attribute(None, false, false),
            Self::KeyAttribute => attribute(Some(Key::Primary), false, false),
            Self::PartialKey => attribute(Some(Key::Partial), false, false),
            Self::DerivedAttribute => attribute(None, true, false),
            Self::MultivaluedAttribute => attribute(None, false, true),
            Self::Relationship => NodeKind::relationship(),
            Self::IdentifyingRelationship => NodeKind::Relationship { identifying: true },
            Self::IsA => NodeKind::is_a(),
            Self::Text => return None,
        })
    }

    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Entity => "entity",
            Self::WeakEntity => "weak_entity",
            Self::AssociativeEntity => "associative_entity",
            Self::Attribute => "attribute",
            Self::KeyAttribute => "key_attribute",
            Self::PartialKey => "partial_key",
            Self::DerivedAttribute => "derived_attribute",
            Self::MultivaluedAttribute => "multivalued_attribute",
            Self::Relationship => "relationship",
            Self::IdentifyingRelationship => "identifying_relationship",
            Self::IsA => "is_a",
            Self::Text => "text",
        }
    }

    /// Its name in the palette.
    pub fn label(self) -> String {
        t!(format!("erm.shape.{}", self.key())).to_string()
    }

    /// The name a new one gets (in the UI's language; IS-A always "IS-A").
    pub fn default_name(self) -> String {
        match self.kind() {
            None => t!("erm.new_text").to_string(),
            Some(NodeKind::IsA { .. }) => "IS-A".to_owned(),
            Some(NodeKind::Attribute { .. }) => t!("erm.new_attribute").to_string(),
            Some(NodeKind::Relationship { .. }) => t!("erm.new_relationship").to_string(),
            Some(_) => t!("erm.new_entity").to_string(),
        }
    }
}

/// A shape dragged from the palette onto the canvas.
#[derive(Clone)]
pub struct DraggedShape(pub Shape);

impl Render for DraggedShape {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(cx.theme().background)
            .border_1()
            .border_color(cx.theme().border)
            .text_xs()
            .child(self.0.label())
    }
}

thread_local! {
    /// The preview images, drawn once.
    static PREVIEWS: std::cell::RefCell<Vec<(Shape, Arc<RenderImage>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Pixels per unit of the previews (sharp on high-resolution screens).
const PREVIEW_SCALE: f32 = 2.0;

/// The shape drawn by our own renderer at half its size (keys with a name, to show their
/// underline); none for a free text.
fn preview(shape: Shape) -> Option<Arc<RenderImage>> {
    if let Some(found) =
        PREVIEWS.with_borrow(|p| p.iter().find(|(s, _)| *s == shape).map(|(_, i)| i.clone()))
    {
        return Some(found);
    }
    // A text has nothing to show but itself; keys show their underline under a name.
    let kind = shape.kind()?;
    let name = match shape {
        Shape::KeyAttribute | Shape::PartialKey => "ID",
        _ => "",
    };
    let mut d = Diagram::new("");
    d.add_node(kind, name, Point::new(0.0, 0.0), umlcraft_erm::create::SizeSet::Compact);
    let options = umlcraft_erm::svg::Options { background: None, margin: 2.0, view_box: None };
    let svg = umlcraft_erm::svg::render_with(&d, &options);
    let tree = OPTIONS.with(|o| resvg::usvg::Tree::from_str(&svg, o)).ok()?;
    let (image, _) = rasterize(&tree, PREVIEW_SCALE * 0.5)?;
    PREVIEWS.with_borrow_mut(|p| p.push((shape, image.clone())));
    Some(image)
}

/// The palette's column.
pub fn palette(cx: &App) -> impl IntoElement + use<> {
    let muted = cx.theme().muted_foreground;
    let hover = cx.theme().secondary;
    v_flex()
        .id("erm-palette")
        .size_full()
        .overflow_y_scroll()
        .p_1()
        .gap_0p5()
        .child(div().px_1().pb_1().text_xs().text_color(muted).child(t!("erm.palette").to_string()))
        .children(Shape::ALL.into_iter().map(move |shape| {
            let image = preview(shape);
            v_flex()
                .id(SharedString::from(format!("erm-shape-{}", shape.key())))
                .items_center()
                .gap_0p5()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .children(image.map(|image| {
                    let size = image.size(0);
                    let (w, h) = (size.width.0 as f32, size.height.0 as f32);
                    img(ImageSource::Render(image))
                        .w(px(w / PREVIEW_SCALE))
                        .h(px(h / PREVIEW_SCALE))
                }))
                .child(div().text_xs().child(shape.label()))
                .on_click(move |_, window, cx| {
                    window.dispatch_action(Box::new(PlaceShape { shape }), cx)
                })
                .on_drag(DraggedShape(shape), |dragged, _, _, cx| cx.new(|_| dragged.clone()))
        }))
}
