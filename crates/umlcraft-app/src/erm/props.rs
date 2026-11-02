//! The properties beside the canvas (like draw.io's "Format" panel): for the selection its
//! name or label, the switches of its kind, the line's style, colours and font, and the
//! database properties (kept, not drawn); with nothing selected the diagram's name and origin.
//! A field takes effect with Enter or when it is left, a switch at once; each is one undo step.

use std::collections::HashMap;

use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::RadioGroup;
use gpui_kit::component::{
    ActiveTheme, Disableable, Selectable, Sizable, StyledExt, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_erm::edit::{self, Item};
use umlcraft_erm::{Diagram, Key, Line, NodeKind, Props, Style, TextAlign};

use super::canvas::ErmCanvas;

/// draw.io's first colours (fill, line, font); empty is the standard look.
const FILLS: [&str; 8] =
    ["", "#dae8fc", "#d5e8d4", "#ffe6cc", "#fff2cc", "#f8cecc", "#e1d5e7", "#f5f5f5"];
const STROKES: [&str; 8] =
    ["", "#6c8ebf", "#82b366", "#d79b00", "#d6b656", "#b85450", "#9673a6", "#666666"];
const FONTS: [&str; 6] = ["", "#ff0000", "#0000ff", "#008000", "#d79b00", "#666666"];

/// The text fields of the panel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    DiagramName,
    Name,
    Label,
    Role,
    FontSize,
    PhysicalName,
    Datatype,
    Length,
    Default,
    Comment,
}

/// Which colour a swatch sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    Fill,
    Stroke,
    Font,
}

pub struct PropsPanel {
    canvas: Entity<ErmCanvas>,
    /// The canvas's revision the fields were made for.
    seen: Option<u64>,
    fields: HashMap<Field, Entity<InputState>>,
    _subscriptions: Vec<Subscription>,
}

impl PropsPanel {
    pub fn new(canvas: Entity<ErmCanvas>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&canvas, |_, _, cx| cx.notify());
        Self { canvas, seen: None, fields: HashMap::new(), _subscriptions: vec![observe] }
    }

    /// Makes the fields anew for the selection as it is.
    fn rebuild(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let canvas = self.canvas.read(cx);
        let d = canvas.diagram().clone();
        let selection = canvas.selection().to_vec();
        let revision = canvas.revision;
        let mut values: Vec<(Field, String)> = Vec::new();
        match selection.as_slice() {
            [] => values.push((Field::DiagramName, d.meta.name.clone())),
            [Item::Node(id)] => {
                if let Some(n) = d.node(id) {
                    values.push((Field::Name, n.name.clone()));
                    values.push((Field::FontSize, font_size(&n.style)));
                    if matches!(
                        n.kind,
                        NodeKind::Entity { .. }
                            | NodeKind::AssociativeEntity
                            | NodeKind::Attribute { .. }
                    ) {
                        let p = &n.props;
                        let opt = |v: &Option<String>| v.clone().unwrap_or_default();
                        values.extend([
                            (Field::PhysicalName, opt(&p.physical_name)),
                            (Field::Datatype, opt(&p.datatype)),
                            (Field::Length, opt(&p.length_precision)),
                            (Field::Default, opt(&p.default_value)),
                            (Field::Comment, opt(&p.comment)),
                        ]);
                    }
                }
            }
            [Item::Link(id)] | [Item::Label(id)] => {
                if let Some(l) = d.link(id) {
                    values.push((
                        Field::Label,
                        l.label.as_ref().map(|l| l.display()).unwrap_or_default(),
                    ));
                    values.push((
                        Field::Role,
                        l.role.as_ref().map(|r| r.text.clone()).unwrap_or_default(),
                    ));
                }
            }
            [Item::Text(id)] => {
                if let Some(t) = d.texts.iter().find(|t| t.id == *id) {
                    values.push((Field::FontSize, font_size(&t.style)));
                }
            }
            _ => values.push((Field::FontSize, String::new())),
        }
        self.fields.clear();
        self._subscriptions.truncate(1);
        for (field, value) in values {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(value));
            let sub =
                cx.subscribe_in(&state, window, move |this, state, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let value = state.read(cx).value().to_string();
                        this.commit(field, value, cx);
                    }
                });
            self.fields.insert(field, state);
            self._subscriptions.push(sub);
        }
        self.seen = Some(revision);
    }

    /// Takes a field's value over, if it changed.
    fn commit(&mut self, field: Field, value: String, cx: &mut Context<Self>) {
        let canvas = self.canvas.read(cx);
        let d = canvas.diagram();
        let selection = canvas.selection().to_vec();
        let one = selection.first().cloned();
        let node_props = match &one {
            Some(Item::Node(id)) => d.node(id).map(|n| n.props.clone()),
            _ => None,
        };
        let current = current_value(d, &selection, field);
        if current.as_deref() == Some(value.as_str()) {
            return;
        }
        let opt = |v: &str| (!v.trim().is_empty()).then(|| v.trim().to_owned());
        self.canvas.update(cx, |c, cx| {
            c.apply(cx, |d| {
                match (field, &one) {
                    (Field::DiagramName, _) => edit::set_name(d, value.trim()),
                    (Field::Name, Some(item)) => edit::set_text(d, item, &value)?,
                    (Field::Label, Some(Item::Link(id) | Item::Label(id))) => {
                        edit::set_text(d, &Item::Label(id.clone()), &value)?
                    }
                    (Field::Role, Some(Item::Link(id) | Item::Label(id))) => {
                        edit::set_role(d, id, &value)?
                    }
                    (Field::FontSize, _) => {
                        let size = value.trim().replace(',', ".").parse::<f64>().ok();
                        let size = size.filter(|s| *s > 0.0 && *s != 12.0);
                        edit::set_style(d, &selection, |s| s.font_size = size)?
                    }
                    (_, Some(Item::Node(id))) => {
                        let mut p: Props = node_props.clone().unwrap_or_default();
                        match field {
                            Field::PhysicalName => p.physical_name = opt(&value),
                            Field::Datatype => p.datatype = opt(&value),
                            Field::Length => p.length_precision = opt(&value),
                            Field::Default => p.default_value = opt(&value),
                            Field::Comment => p.comment = opt(&value),
                            _ => {}
                        }
                        edit::set_props(d, id, p)?
                    }
                    _ => {}
                }
                Ok(None)
            });
        });
    }

    /// Makes a change of the selection at once (a switch).
    fn change(
        canvas: &Entity<ErmCanvas>,
        cx: &mut App,
        f: impl FnOnce(&mut Diagram, &[Item]) -> Result<(), umlcraft_erm::EditError>,
    ) {
        canvas.update(cx, |c, cx| {
            let selection = c.selection().to_vec();
            c.apply(cx, |d| f(d, &selection).map(|()| None));
        });
    }

    fn input(&self, field: Field, label: &str) -> Option<AnyElement> {
        let state = self.fields.get(&field)?;
        Some(
            v_flex()
                .gap_0p5()
                .child(div().text_xs().child(t!(label).to_string()))
                .child(Input::new(state).xsmall())
                .into_any_element(),
        )
    }

    fn kind_switches(&self, kind: NodeKind, node: umlcraft_erm::Id, cx: &App) -> AnyElement {
        let canvas = self.canvas.clone();
        let set = move |kind: NodeKind| {
            let canvas = canvas.clone();
            let node = node.clone();
            move |cx: &mut App| Self::change(&canvas, cx, |d, _| edit::set_kind(d, &node, kind))
        };
        let check = |id: &'static str, label: &str, on: bool, then: Box<dyn Fn(&mut App)>| {
            Checkbox::new(id)
                .label(t!(label).to_string())
                .checked(on)
                .on_click(move |_, _, cx| then(cx))
                .into_any_element()
        };
        let mut col = v_flex().gap_1();
        match kind {
            NodeKind::Entity { weak } => {
                let f = set(NodeKind::Entity { weak: !weak });
                col = col.child(check("erm-weak", "erm.prop.weak", weak, Box::new(f)));
            }
            NodeKind::Attribute { key, derived, multivalued } => {
                let keys = [None, Some(Key::Primary), Some(Key::Partial)];
                let index = keys.iter().position(|k| *k == key).unwrap_or(0);
                let setter = set.clone();
                col = col.child(div().text_xs().child(t!("erm.prop.key").to_string())).child(
                    RadioGroup::horizontal("erm-key")
                        .children([
                            t!("erm.prop.key_none").to_string(),
                            t!("erm.prop.key_primary").to_string(),
                            t!("erm.prop.key_partial").to_string(),
                        ])
                        .selected_index(Some(index))
                        .on_change(move |&ix, _, cx| {
                            setter(NodeKind::Attribute { key: keys[ix], derived, multivalued })(cx)
                        }),
                );
                let f = set(NodeKind::Attribute { key, derived: !derived, multivalued });
                col = col.child(check("erm-derived", "erm.prop.derived", derived, Box::new(f)));
                let f = set(NodeKind::Attribute { key, derived, multivalued: !multivalued });
                col =
                    col.child(check("erm-multi", "erm.prop.multivalued", multivalued, Box::new(f)));
            }
            NodeKind::Relationship { identifying } => {
                let f = set(NodeKind::Relationship { identifying: !identifying });
                col =
                    col.child(check("erm-ident", "erm.prop.identifying", identifying, Box::new(f)));
            }
            NodeKind::IsA { total, disjoint } => {
                let f = set(NodeKind::IsA { total: !total, disjoint });
                col = col.child(check("erm-total", "erm.prop.total", total, Box::new(f)));
                let options = [None, Some(true), Some(false)];
                let index = options.iter().position(|o| *o == disjoint).unwrap_or(0);
                let setter = set.clone();
                col = col.child(div().text_xs().child(t!("erm.prop.disjoint").to_string())).child(
                    RadioGroup::horizontal("erm-disjoint")
                        .children([
                            t!("erm.prop.disjoint_open").to_string(),
                            t!("erm.prop.disjoint_d").to_string(),
                            t!("erm.prop.disjoint_o").to_string(),
                        ])
                        .selected_index(Some(index))
                        .on_change(move |&ix, _, cx| {
                            setter(NodeKind::IsA { total, disjoint: options[ix] })(cx)
                        }),
                );
            }
            NodeKind::AssociativeEntity => {}
        }
        let _ = cx;
        col.into_any_element()
    }

    /// Colours, font and corners of the selected nodes and texts.
    fn style_section(&self, style: Style, entities: bool, cx: &App) -> AnyElement {
        let canvas = self.canvas.clone();
        let flag = |id: &'static str, label: &str, on: bool, f: fn(&mut Style, bool)| {
            let canvas = canvas.clone();
            Checkbox::new(id).label(t!(label).to_string()).checked(on).on_click(move |&v, _, cx| {
                Self::change(&canvas, cx, |d, items| edit::set_style(d, items, |s| f(s, v)))
            })
        };
        let swatches = |paint: Paint, colors: &'static [&'static str], current: &Option<String>| {
            let current = current.clone().unwrap_or_default();
            h_flex().gap_0p5().flex_wrap().children(colors.iter().map(|&c| {
                let canvas = canvas.clone();
                let fill = if c.is_empty() { cx.theme().background } else { rgb(hex(c)).into() };
                Button::new(SharedString::from(format!("erm-{paint:?}-{c}")))
                    .xsmall()
                    .outline()
                    .selected(current == c)
                    .when(c.is_empty(), |b| b.label("–"))
                    .child(div().size(px(12.0)).rounded_sm().bg(fill))
                    .tooltip(if c.is_empty() {
                        t!("erm.prop.standard").to_string()
                    } else {
                        c.to_owned()
                    })
                    .on_click(move |_, _, cx| {
                        let value = (!c.is_empty()).then(|| c.to_owned());
                        Self::change(&canvas, cx, |d, items| {
                            edit::set_style(d, items, |s| match paint {
                                Paint::Fill => s.fill = value.clone(),
                                Paint::Stroke => s.stroke = value.clone(),
                                Paint::Font => s.font_color = value.clone(),
                            })
                        })
                    })
            }))
        };
        v_flex()
            .gap_1()
            .child(section_title(cx, "erm.prop.style"))
            .child(div().text_xs().child(t!("erm.prop.fill").to_string()))
            .child(swatches(Paint::Fill, &FILLS, &style.fill))
            .child(div().text_xs().child(t!("erm.prop.stroke").to_string()))
            .child(swatches(Paint::Stroke, &STROKES, &style.stroke))
            .child(div().text_xs().child(t!("erm.prop.font_color").to_string()))
            .child(swatches(Paint::Font, &FONTS, &style.font_color))
            .children(self.input(Field::FontSize, "erm.prop.font_size"))
            .child(flag("erm-bold", "erm.prop.bold", style.bold, |s, v| s.bold = v))
            .child(flag("erm-italic", "erm.prop.italic", style.italic, |s, v| s.italic = v))
            .when(entities, |c| {
                c.child(flag("erm-rounded", "erm.prop.rounded", style.rounded, |s, v| {
                    s.rounded = v
                }))
            })
            .into_any_element()
    }
}

fn section_title(cx: &App, key: &str) -> Div {
    div()
        .pt_2()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(t!(key).to_string())
}

fn hex(c: &str) -> u32 {
    u32::from_str_radix(c.trim_start_matches('#'), 16).unwrap_or(0xffffff)
}

fn font_size(style: &Style) -> String {
    style.font_size.map(|s| s.to_string()).unwrap_or_default()
}

/// What a field shows for the selection now, to leave unchanged fields alone.
fn current_value(d: &Diagram, selection: &[Item], field: Field) -> Option<String> {
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    match (field, selection) {
        (Field::DiagramName, _) => Some(d.meta.name.clone()),
        (Field::Name, [Item::Node(id)]) => d.node(id).map(|n| n.name.clone()),
        (Field::Label, [Item::Link(id) | Item::Label(id)]) => {
            d.link(id).map(|l| l.label.as_ref().map(|l| l.display()).unwrap_or_default())
        }
        (Field::Role, [Item::Link(id) | Item::Label(id)]) => {
            d.link(id).map(|l| l.role.as_ref().map(|r| r.text.clone()).unwrap_or_default())
        }
        (Field::FontSize, [Item::Node(id)]) => d.node(id).map(|n| font_size(&n.style)),
        (Field::FontSize, [Item::Text(id)]) => {
            d.texts.iter().find(|t| t.id == *id).map(|t| font_size(&t.style))
        }
        (_, [Item::Node(id)]) => d.node(id).map(|n| {
            let p = &n.props;
            match field {
                Field::PhysicalName => opt(&p.physical_name),
                Field::Datatype => opt(&p.datatype),
                Field::Length => opt(&p.length_precision),
                Field::Default => opt(&p.default_value),
                Field::Comment => opt(&p.comment),
                _ => String::new(),
            }
        }),
        _ => None,
    }
}

impl Render for PropsPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let revision = self.canvas.read(cx).revision;
        if self.seen != Some(revision) {
            self.rebuild(window, cx);
        }
        let canvas = self.canvas.read(cx);
        let d = canvas.diagram().clone();
        let selection = canvas.selection().to_vec();
        let muted = cx.theme().muted_foreground;
        let mut body = v_flex().gap_2();
        match selection.as_slice() {
            [] => {
                body = body
                    .child(section_title(cx, "erm.prop.diagram"))
                    .children(self.input(Field::DiagramName, "erm.prop.diagram_name"));
                let meta = [
                    ("erm.prop.author", d.meta.author.clone()),
                    ("erm.prop.created", d.meta.created.clone()),
                    ("erm.prop.modified", d.meta.modified.clone()),
                    ("erm.prop.origin", d.meta.origin.clone()),
                ];
                for (key, value) in meta {
                    if let Some(value) = value {
                        body = body.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("{}: {value}", t!(key))),
                        );
                    }
                }
            }
            [Item::Node(id)] => {
                if let Some(n) = d.node(id) {
                    let kind = n.kind;
                    let entities =
                        matches!(kind, NodeKind::Entity { .. } | NodeKind::AssociativeEntity);
                    let sql = entities || kind.is_attribute();
                    body = body
                        .child(section_title(cx, "erm.prop.element"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(super::kind_name(kind.type_name())),
                        )
                        .children(self.input(Field::Name, "erm.prop.name"))
                        .child(self.kind_switches(kind, id.clone(), cx))
                        .child(self.style_section(n.style.clone(), entities, cx));
                    if sql {
                        body = body
                            .child(section_title(cx, "erm.prop.sql"))
                            .children(self.input(Field::PhysicalName, "erm.prop.physical_name"))
                            .children(self.input(Field::Datatype, "erm.prop.datatype"))
                            .children(self.input(Field::Length, "erm.prop.length"))
                            .children(self.input(Field::Default, "erm.prop.default"))
                            .children(self.input(Field::Comment, "erm.prop.comment"));
                        if kind.is_attribute() {
                            let canvas = self.canvas.clone();
                            let node = id.clone();
                            let props = n.props.clone();
                            body = body.child(
                                Checkbox::new("erm-not-null")
                                    .label(t!("erm.prop.not_null").to_string())
                                    .checked(props.not_null)
                                    .on_click(move |&v, _, cx| {
                                        let p = Props { not_null: v, ..props.clone() };
                                        Self::change(&canvas, cx, |d, _| {
                                            edit::set_props(d, &node, p)
                                        })
                                    }),
                            );
                        }
                    }
                }
            }
            [Item::Link(id)] | [Item::Label(id)] => {
                if let Some(l) = d.link(id) {
                    let canvas = self.canvas.clone();
                    let link = id.clone();
                    let lines = [Line::Solid, Line::Dotted, Line::Double];
                    let index = lines.iter().position(|x| *x == l.line).unwrap_or(0);
                    let from_is_a =
                        d.node(&l.source).is_some_and(|n| matches!(n.kind, NodeKind::IsA { .. }));
                    body = body
                        .child(section_title(cx, "erm.prop.line"))
                        .children(self.input(Field::Label, "erm.prop.label"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(t!("erm.prop.label_hint").to_string()),
                        )
                        .children(self.input(Field::Role, "erm.prop.role"))
                        .child(div().text_xs().child(t!("erm.prop.line_style").to_string()))
                        .child({
                            let canvas = canvas.clone();
                            let link = link.clone();
                            RadioGroup::vertical("erm-line")
                                .children([
                                    t!("erm.prop.line_solid").to_string(),
                                    t!("erm.prop.line_dotted").to_string(),
                                    t!("erm.prop.line_double").to_string(),
                                ])
                                .selected_index(Some(index))
                                .on_change(move |&ix, _, cx| {
                                    let line = lines[ix];
                                    Self::change(&canvas, cx, |d, _| edit::set_line(d, &link, line))
                                })
                        });
                    if from_is_a {
                        let canvas = canvas.clone();
                        let link = link.clone();
                        let on = l.super_type;
                        body = body.child(
                            Checkbox::new("erm-super")
                                .label(t!("erm.prop.super_type").to_string())
                                .checked(on)
                                .on_click(move |&v, _, cx| {
                                    Self::change(&canvas, cx, |d, _| {
                                        edit::set_super_type(d, &link, v)
                                    })
                                }),
                        );
                    }
                    let fixed = l.source_port.is_some() || l.target_port.is_some();
                    body = body.child(
                        Button::new("erm-release")
                            .xsmall()
                            .outline()
                            .label(t!("erm.release_ends").to_string())
                            .disabled(!fixed)
                            .on_click(move |_, _, cx| {
                                let link = link.clone();
                                Self::change(&canvas, cx, move |d, _| {
                                    edit::release_ends(d, &[link])
                                })
                            }),
                    );
                }
            }
            [Item::Text(id)] => {
                if let Some(text) = d.texts.iter().find(|t| t.id == *id) {
                    let canvas = self.canvas.clone();
                    let aligns = [TextAlign::Left, TextAlign::Center, TextAlign::Right];
                    let current = text.style.align.unwrap_or(TextAlign::Center);
                    let index = aligns.iter().position(|a| *a == current).unwrap_or(1);
                    body = body
                        .child(section_title(cx, "erm.prop.text"))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(t!("erm.prop.text_hint").to_string()),
                        )
                        .child(div().text_xs().child(t!("erm.prop.align").to_string()))
                        .child(
                            RadioGroup::horizontal("erm-align")
                                .children([
                                    t!("erm.prop.align_left").to_string(),
                                    t!("erm.prop.align_center").to_string(),
                                    t!("erm.prop.align_right").to_string(),
                                ])
                                .selected_index(Some(index))
                                .on_change(move |&ix, _, cx| {
                                    let align = aligns[ix];
                                    Self::change(&canvas, cx, |d, items| {
                                        edit::set_align(d, items, align)
                                    })
                                }),
                        )
                        .child(self.style_section(text.style.clone(), false, cx));
                }
            }
            items => {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(t!("erm.prop.several", count = items.len()).to_string()),
                );
                if items.iter().any(|i| matches!(i, Item::Node(_) | Item::Text(_))) {
                    body = body.child(self.style_section(Style::default(), true, cx));
                }
            }
        }
        v_flex().id("erm-props").size_full().overflow_y_scroll().p_2().text_sm().child(body)
    }
}
