//! Properties panel for the selected element and, if one node is selected, its presentation.
//!
//! Text fields commit on Enter or when they lose focus, so typing is one undo step, and always
//! into the element shown when editing began. Selects and checkboxes apply immediately.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme, IndexPath, Sizable, StyledExt, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{
    Aggregation, ElementData, ElementKind, Id, InteractionOperator, MessageSort, Model,
    Multiplicity, Navigability, Style,
};
use umlcraft_notation::member::member_text;
use umlcraft_notation::sequence::operator_keyword;

use crate::labels::{self, VISIBILITIES};
use crate::workspace::Workspace;

type Choice = SelectState<Vec<SharedString>>;

const NAVIGABILITIES: [Navigability; 3] =
    [Navigability::Unspecified, Navigability::Navigable, Navigability::NonNavigable];
const AGGREGATIONS: [Aggregation; 3] =
    [Aggregation::None, Aggregation::Shared, Aggregation::Composite];
const SORTS: [MessageSort; 5] = [
    MessageSort::Synchronous,
    MessageSort::Asynchronous,
    MessageSort::Reply,
    MessageSort::Create,
    MessageSort::Destroy,
];
const OPERATORS: [InteractionOperator; 12] = [
    InteractionOperator::Alt,
    InteractionOperator::Opt,
    InteractionOperator::Loop,
    InteractionOperator::Par,
    InteractionOperator::Break,
    InteractionOperator::Critical,
    InteractionOperator::Neg,
    InteractionOperator::Assert,
    InteractionOperator::Strict,
    InteractionOperator::Seq,
    InteractionOperator::Ignore,
    InteractionOperator::Consider,
];

#[derive(Clone, Copy)]
enum Field {
    Name,
    Declaration,
    Stereotypes,
    Text,
    Role(usize),
    Multiplicity(usize),
    Condition,
    Arguments,
    ReturnValue,
    Guard,
}

const TEXT_FIELDS: [Field; 8] = [
    Field::Name,
    Field::Declaration,
    Field::Stereotypes,
    Field::Text,
    Field::Condition,
    Field::Arguments,
    Field::ReturnValue,
    Field::Guard,
];

/// Inputs for one association end.
struct EndFields {
    role: Entity<InputState>,
    multiplicity: Entity<InputState>,
    navigability: Entity<Choice>,
    aggregation: Entity<Choice>,
}

pub struct PropertiesPanel {
    workspace: Entity<Workspace>,
    name: Entity<InputState>,
    declaration: Entity<InputState>,
    condition: Entity<InputState>,
    stereotypes: Entity<InputState>,
    text: Entity<TextareaState>,
    visibility: Entity<Choice>,
    ends: [EndFields; 2],
    arguments: Entity<InputState>,
    return_value: Entity<InputState>,
    guard: Entity<InputState>,
    sort: Entity<Choice>,
    operator: Entity<Choice>,
    /// Operations a message can invoke, with their ids (`None` for "no operation").
    operation: Entity<Choice>,
    operation_ids: Vec<Option<Id>>,
    /// Classifiers a lifeline can represent, with their ids (`None` for "none").
    represents: Entity<Choice>,
    represents_ids: Vec<Option<Id>>,
    /// Element, document revision and locale the fields currently show.
    shown: Option<(Id, u64, String)>,
    /// Element whose text fields are being edited. Edits always go back to this element, even
    /// if the selection has changed by the time the field loses focus.
    editing: Option<Id>,
    _subscriptions: Vec<Subscription>,
}

fn navigability_items() -> Vec<SharedString> {
    ["navigability.unspecified", "navigability.navigable", "navigability.non_navigable"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

fn aggregation_items() -> Vec<SharedString> {
    ["aggregation.none", "aggregation.shared", "aggregation.composite"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

fn visibility_items() -> Vec<SharedString> {
    VISIBILITIES.iter().map(|&v| labels::visibility_label(v).into()).collect()
}

fn sort_items() -> Vec<SharedString> {
    ["synchronous", "asynchronous", "reply", "create", "destroy"]
        .map(|k| t!(format!("message_sort.{k}")).to_string().into())
        .to_vec()
}

fn operator_items() -> Vec<SharedString> {
    OPERATORS.iter().map(|&o| operator_keyword(o).into()).collect()
}

/// Choices for a reference: "—" for none, then `targets` by name.
fn reference_items(model: &Model, targets: &[Option<Id>]) -> Vec<SharedString> {
    targets
        .iter()
        .map(|t| match t.and_then(|t| model.get(t)) {
            Some(e) => labels::display_name(e).into(),
            None => "—".into(),
        })
        .collect()
}

/// The operations a message can call: those of the classifier its receiver represents.
fn operations_for(model: &Model, message: Id) -> Vec<Option<Id>> {
    let classifier = model.get(message).and_then(|m| match m.kind() {
        ElementKind::Message(p) => model.get(p.to).and_then(|l| match l.kind() {
            ElementKind::Lifeline(l) => l.represents,
            _ => None,
        }),
        _ => None,
    });
    let operations = classifier.into_iter().flat_map(|c| model.children(c).iter().copied());
    std::iter::once(None)
        .chain(
            operations
                .filter(|&o| {
                    model.get(o).is_some_and(|e| matches!(e.kind(), ElementKind::Operation(_)))
                })
                .map(Some),
        )
        .collect()
}

/// The classifiers a lifeline can represent, by name.
fn classifiers(model: &Model) -> Vec<Option<Id>> {
    let mut all: Vec<(&str, Id)> =
        model.elements().filter(|e| e.kind().is_classifier()).map(|e| (e.name(), e.id())).collect();
    all.sort_by(|a, b| a.0.cmp(b.0));
    std::iter::once(None).chain(all.into_iter().map(|(_, id)| Some(id))).collect()
}

impl PropertiesPanel {
    pub fn new(workspace: Entity<Workspace>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input =
            |window: &mut Window, cx: &mut Context<Self>| cx.new(|cx| InputState::new(window, cx));
        let name = input(window, cx);
        let declaration = input(window, cx);
        let condition = input(window, cx);
        let stereotypes = cx.new(|cx| InputState::new(window, cx).placeholder("entity, control"));
        let text = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 12));
        let visibility = cx.new(|cx| SelectState::new(visibility_items(), None, window, cx));
        let end = |window: &mut Window, cx: &mut Context<Self>| EndFields {
            role: cx.new(|cx| InputState::new(window, cx)),
            multiplicity: cx.new(|cx| InputState::new(window, cx).placeholder("0..*")),
            navigability: cx.new(|cx| SelectState::new(navigability_items(), None, window, cx)),
            aggregation: cx.new(|cx| SelectState::new(aggregation_items(), None, window, cx)),
        };
        let ends = [end(window, cx), end(window, cx)];
        let arguments = input(window, cx);
        let return_value = input(window, cx);
        let guard = input(window, cx);
        let sort = cx.new(|cx| SelectState::new(sort_items(), None, window, cx));
        let operator = cx.new(|cx| SelectState::new(operator_items(), None, window, cx));
        let operation = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let represents = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));

        let mut subscriptions =
            vec![cx.observe_in(&workspace, window, |this, _, window, cx| this.sync(window, cx))];
        let mut watch = |state: &Entity<InputState>, field: Field, cx: &mut Context<Self>| {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, ev: &InputEvent, _, cx| {
                    this.on_input(field, ev, cx);
                },
            ));
        };
        watch(&name, Field::Name, cx);
        watch(&declaration, Field::Declaration, cx);
        watch(&condition, Field::Condition, cx);
        watch(&stereotypes, Field::Stereotypes, cx);
        watch(&arguments, Field::Arguments, cx);
        watch(&return_value, Field::ReturnValue, cx);
        watch(&guard, Field::Guard, cx);
        for (i, end) in ends.iter().enumerate() {
            watch(&end.role, Field::Role(i), cx);
            watch(&end.multiplicity, Field::Multiplicity(i), cx);
        }
        subscriptions.push(cx.subscribe_in(&text, window, |this, _, ev: &InputEvent, _, cx| {
            // Enter makes a new line in the text area; only leaving it commits.
            if !matches!(ev, InputEvent::PressEnter { .. }) {
                this.on_input(Field::Text, ev, cx);
            }
        }));
        subscriptions.push(cx.subscribe_in(
            &visibility,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(v) = selected(state, cx).and_then(|ix| VISIBILITIES.get(ix).copied())
                else {
                    return;
                };
                this.apply_shown(cx, move |data| data.visibility = v);
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &sort,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(s) = selected(state, cx).and_then(|ix| SORTS.get(ix).copied()) else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::Message(p) = &mut data.kind {
                        p.sort = s;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &operator,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(o) = selected(state, cx).and_then(|ix| OPERATORS.get(ix).copied()) else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::CombinedFragment(p) = &mut data.kind {
                        p.operator = o;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &operation,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(&target) = selected(state, cx).and_then(|ix| this.operation_ids.get(ix))
                else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::Message(p) = &mut data.kind {
                        p.operation = target;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &represents,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(&target) = selected(state, cx).and_then(|ix| this.represents_ids.get(ix))
                else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::Lifeline(p) = &mut data.kind {
                        p.represents = target;
                    }
                });
            },
        ));
        for (i, end) in ends.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                &end.navigability,
                window,
                move |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                    let Some(n) =
                        selected(state, cx).and_then(|ix| NAVIGABILITIES.get(ix).copied())
                    else {
                        return;
                    };
                    this.apply_shown(cx, move |data| {
                        if let ElementKind::Association(p) = &mut data.kind {
                            p.ends[i].navigability = n;
                        }
                    });
                },
            ));
            subscriptions.push(cx.subscribe_in(
                &end.aggregation,
                window,
                move |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                    let Some(a) = selected(state, cx).and_then(|ix| AGGREGATIONS.get(ix).copied())
                    else {
                        return;
                    };
                    this.apply_shown(cx, move |data| {
                        if let ElementKind::Association(p) = &mut data.kind {
                            p.ends[i].aggregation = a;
                        }
                    });
                },
            ));
        }

        let mut panel = Self {
            workspace,
            name,
            declaration,
            condition,
            stereotypes,
            text,
            visibility,
            ends,
            arguments,
            return_value,
            guard,
            sort,
            operator,
            operation,
            operation_ids: Vec::new(),
            represents,
            represents_ids: Vec::new(),
            shown: None,
            editing: None,
            _subscriptions: subscriptions,
        };
        panel.sync(window, cx);
        panel
    }

    /// Puts the cursor into the name (or declaration) field, e.g. for "Rename".
    pub fn focus_name(&self, window: &mut Window, cx: &mut Context<Self>) {
        let is_member = self
            .shown_id()
            .and_then(|id| self.workspace.read(cx).doc.model().get(id))
            .is_some_and(|e| is_member(e.kind()));
        let field = if is_member { &self.declaration } else { &self.name };
        field.update(cx, |state, cx| state.focus(window, cx));
    }

    fn shown_id(&self) -> Option<Id> {
        self.shown.as_ref().map(|(id, _, _)| *id)
    }

    fn on_input(&mut self, field: Field, ev: &InputEvent, cx: &mut Context<Self>) {
        match ev {
            InputEvent::Focus => self.editing = self.editing.or(self.shown_id()),
            InputEvent::PressEnter { .. } => self.commit(field, cx),
            InputEvent::Blur => {
                self.commit(field, cx);
                self.editing = None;
            }
            InputEvent::Change => {}
        }
    }

    fn value(state: &Entity<InputState>, cx: &App) -> String {
        state.read(cx).value().trim().to_owned()
    }

    /// Writes one field back to the element being edited.
    fn commit(&mut self, field: Field, cx: &mut Context<Self>) {
        let Some(id) = self.editing else { return };
        match field {
            Field::Name => {
                let value = Self::value(&self.name, cx);
                self.apply(id, cx, move |data| data.name = value);
            }
            Field::Declaration => {
                let value = Self::value(&self.declaration, cx);
                let current = member_text(self.workspace.read(cx).doc.model(), id);
                if value != current {
                    self.workspace.update(cx, |ws, cx| {
                        ws.apply_text(id, &value);
                        cx.notify();
                    });
                }
            }
            Field::Stereotypes => {
                let list: Vec<String> = Self::value(&self.stereotypes, cx)
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                self.apply(id, cx, move |data| data.stereotypes = list);
            }
            Field::Text => {
                let value = self.text.read(cx).value().to_string();
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::Comment(p) => p.body = value,
                    _ => data.documentation = value,
                });
            }
            Field::Condition => {
                let value = Self::value(&self.condition, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::Extend(p) = &mut data.kind {
                        p.condition = value;
                    }
                });
            }
            Field::Arguments => {
                let value = Self::value(&self.arguments, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::Message(p) = &mut data.kind {
                        p.arguments = value;
                    }
                });
            }
            Field::ReturnValue => {
                let value = Self::value(&self.return_value, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::Message(p) = &mut data.kind {
                        p.return_value = value;
                    }
                });
            }
            Field::Guard => {
                let value = Self::value(&self.guard, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::InteractionOperand(p) = &mut data.kind {
                        p.guard = value;
                    }
                });
            }
            Field::Role(i) => {
                let value = Self::value(&self.ends[i].role, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::Association(p) = &mut data.kind {
                        p.ends[i].role = value;
                    }
                });
            }
            Field::Multiplicity(i) => {
                let text = Self::value(&self.ends[i].multiplicity, cx);
                let parsed =
                    if text.is_empty() { Ok(None) } else { text.parse::<Multiplicity>().map(Some) };
                match parsed {
                    Ok(m) => self.apply(id, cx, move |data| {
                        if let ElementKind::Association(p) = &mut data.kind {
                            p.ends[i].multiplicity = m;
                        }
                    }),
                    Err(err) => self.workspace.update(cx, |ws, cx| {
                        ws.complain(err.to_string());
                        cx.notify();
                    }),
                }
            }
        }
    }

    fn apply_shown(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut ElementData)) {
        if let Some(id) = self.shown_id() {
            self.apply(id, cx, f);
        }
    }

    /// Changes element `id` as one undo step, if the change changes anything.
    fn apply(&mut self, id: Id, cx: &mut Context<Self>, f: impl FnOnce(&mut ElementData)) {
        self.workspace.update(cx, |ws, cx| {
            let Some(element) = ws.doc.model().get(id) else { return };
            let mut data = element.data().clone();
            f(&mut data);
            if data != *element.data() {
                ws.update_element(id, |d| *d = data);
                cx.notify();
            }
        });
    }

    /// Loads the selected element into the fields when it (or the document) changed.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (selected, revision) = {
            let ws = self.workspace.read(cx);
            (ws.selected_element.filter(|&id| ws.doc.model().contains(id)), ws.doc.revision())
        };
        // The selection moved away while a field was being edited: save that edit first.
        if let Some(editing) = self.editing
            && selected != Some(editing)
        {
            let ends = (0..2).flat_map(|i| [Field::Role(i), Field::Multiplicity(i)]);
            for field in TEXT_FIELDS.into_iter().chain(ends) {
                self.commit(field, cx);
            }
            self.editing = None;
        }
        let Some(id) = selected else {
            self.shown = None;
            cx.notify();
            return;
        };
        let locale = rust_i18n::locale().to_string();
        let key = (id, revision, locale.clone());
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        let locale_changed = self.shown.as_ref().is_none_or(|(_, _, l)| *l != locale);
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let data = model.get(id).expect("checked above").data().clone();
        let declaration = member_text(model, id);
        // What a message or lifeline may refer to.
        let references = match &data.kind {
            ElementKind::Message(_) => operations_for(model, id),
            ElementKind::Lifeline(_) => classifiers(model),
            _ => Vec::new(),
        };
        let reference_labels = reference_items(model, &references);
        self.shown = Some(key);

        let set = |state: &Entity<InputState>,
                   value: String,
                   window: &mut Window,
                   cx: &mut Context<Self>| {
            state.update(cx, |s, cx| s.set_value(value, window, cx));
        };
        set(&self.name, data.name.clone(), window, cx);
        set(&self.declaration, declaration, window, cx);
        let condition = match &data.kind {
            ElementKind::Extend(p) => p.condition.clone(),
            _ => String::new(),
        };
        set(&self.condition, condition, window, cx);
        set(&self.stereotypes, data.stereotypes.join(", "), window, cx);
        let text = match &data.kind {
            ElementKind::Comment(p) => p.body.clone(),
            _ => data.documentation.clone(),
        };
        self.text.update(cx, |s, cx| s.set_value(text, window, cx));
        let choose = |state: &Entity<Choice>,
                      ix: Option<usize>,
                      items: Option<Vec<SharedString>>,
                      window: &mut Window,
                      cx: &mut Context<Self>| {
            state.update(cx, |s, cx| {
                if let Some(items) = items {
                    s.set_items(items, window, cx);
                }
                s.set_selected_index(ix.map(IndexPath::new), window, cx);
            });
        };
        let vis = VISIBILITIES.iter().position(|&v| v == data.visibility);
        choose(&self.visibility, vis, locale_changed.then(visibility_items), window, cx);
        match &data.kind {
            ElementKind::Message(p) => {
                set(&self.arguments, p.arguments.clone(), window, cx);
                set(&self.return_value, p.return_value.clone(), window, cx);
                let s = SORTS.iter().position(|&s| s == p.sort);
                choose(&self.sort, s, locale_changed.then(sort_items), window, cx);
                let ix = references.iter().position(|&o| o == p.operation);
                choose(&self.operation, ix, Some(reference_labels), window, cx);
                self.operation_ids = references;
            }
            ElementKind::Lifeline(p) => {
                let ix = references.iter().position(|&c| c == p.represents);
                choose(&self.represents, ix, Some(reference_labels), window, cx);
                self.represents_ids = references;
            }
            ElementKind::CombinedFragment(p) => {
                let ix = OPERATORS.iter().position(|&o| o == p.operator);
                choose(&self.operator, ix, None, window, cx);
            }
            ElementKind::InteractionOperand(p) => set(&self.guard, p.guard.clone(), window, cx),
            _ => {}
        }
        if let ElementKind::Association(p) = &data.kind {
            for (i, end) in p.ends.iter().enumerate() {
                set(&self.ends[i].role, end.role.clone(), window, cx);
                let m = end.multiplicity.map(|m| m.to_string()).unwrap_or_default();
                set(&self.ends[i].multiplicity, m, window, cx);
                let n = NAVIGABILITIES.iter().position(|&x| x == end.navigability);
                choose(
                    &self.ends[i].navigability,
                    n,
                    locale_changed.then(navigability_items),
                    window,
                    cx,
                );
                let a = AGGREGATIONS.iter().position(|&x| x == end.aggregation);
                choose(
                    &self.ends[i].aggregation,
                    a,
                    locale_changed.then(aggregation_items),
                    window,
                    cx,
                );
            }
        }
        cx.notify();
    }
}

fn selected(state: &Entity<Choice>, cx: &App) -> Option<usize> {
    state.read(cx).selected_index(cx).map(|ix| ix.row)
}

fn is_member(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Attribute(_) | ElementKind::Operation(_) | ElementKind::EnumerationLiteral
    )
}

/// A checkbox that flips one boolean of the element via `set`.
fn flag(
    id: &'static str,
    label: String,
    checked: bool,
    workspace: Entity<Workspace>,
    element: Id,
    set: fn(&mut ElementData, bool),
) -> Checkbox {
    Checkbox::new(id).label(label).checked(checked).on_click(move |checked: &bool, _, cx| {
        let checked = *checked;
        workspace.update(cx, |ws, cx| {
            ws.update_element(element, |d| set(d, checked));
            cx.notify();
        });
    })
}

/// A checkbox that flips one display option of a presentation via `set`.
fn style_flag(
    id: &'static str,
    label: String,
    checked: bool,
    workspace: Entity<Workspace>,
    presentation: Id,
    set: fn(&mut Style, bool),
) -> Checkbox {
    Checkbox::new(id).label(label).checked(checked).on_click(move |checked: &bool, _, cx| {
        let checked = *checked;
        workspace.update(cx, |ws, cx| {
            ws.update_style(presentation, |s| set(s, checked));
            cx.notify();
        });
    })
}

impl Render for PropertiesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let field = |label: SharedString, content: AnyElement| {
            v_flex().gap_1().child(div().text_xs().text_color(muted).child(label)).child(content)
        };
        let heading =
            |label: String| div().pt_2().text_xs().font_semibold().text_color(muted).child(label);
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let body = v_flex().gap_3().p_3();
        let Some(e) = ws.selected_element.and_then(|id| model.get(id)) else {
            return body
                .child(div().text_sm().text_color(muted).child(t!("prop.nothing_selected")));
        };
        let id = e.id();
        let kind = e.kind().clone();
        let workspace = self.workspace.clone();

        let mut body = body.child(field(
            t!("prop.type").into(),
            div().text_sm().child(labels::kind_label(e)).into_any_element(),
        ));
        let unnamed = matches!(
            kind,
            ElementKind::Comment(_)
                | ElementKind::CombinedFragment(_)
                | ElementKind::InteractionOperand(_)
        );
        match &kind {
            _ if unnamed => {}
            k if is_member(k) => {
                body = body.child(field(
                    t!("prop.declaration").into(),
                    Input::new(&self.declaration).small().into_any_element(),
                ));
            }
            ElementKind::Message(_) | ElementKind::Lifeline(_) => {
                body = body.child(field(
                    t!("prop.name").into(),
                    Input::new(&self.name).small().into_any_element(),
                ));
            }
            _ => {
                body = body
                    .child(field(
                        t!("prop.name").into(),
                        Input::new(&self.name).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.visibility").into(),
                        Select::new(&self.visibility).small().into_any_element(),
                    ));
            }
        }
        if !unnamed {
            body = body.child(field(
                t!("prop.stereotypes").into(),
                Input::new(&self.stereotypes).small().into_any_element(),
            ));
        }

        match &kind {
            ElementKind::Class(p) => {
                body = body.child(flag(
                    "abstract",
                    t!("prop.abstract").into(),
                    p.is_abstract,
                    workspace.clone(),
                    id,
                    |d, v| {
                        if let ElementKind::Class(p) = &mut d.kind {
                            p.is_abstract = v;
                        }
                    },
                ));
            }
            ElementKind::Attribute(p) => {
                body = body
                    .child(flag(
                        "static",
                        t!("prop.static").into(),
                        p.is_static,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Attribute(p) = &mut d.kind {
                                p.is_static = v;
                            }
                        },
                    ))
                    .child(flag(
                        "read-only",
                        t!("prop.read_only").into(),
                        p.is_read_only,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Attribute(p) = &mut d.kind {
                                p.is_read_only = v;
                            }
                        },
                    ));
            }
            ElementKind::Operation(p) => {
                body = body
                    .child(flag(
                        "static",
                        t!("prop.static").into(),
                        p.is_static,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Operation(p) = &mut d.kind {
                                p.is_static = v;
                            }
                        },
                    ))
                    .child(flag(
                        "abstract",
                        t!("prop.abstract").into(),
                        p.is_abstract,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Operation(p) = &mut d.kind {
                                p.is_abstract = v;
                            }
                        },
                    ))
                    .child(flag(
                        "query",
                        t!("prop.query").into(),
                        p.is_query,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Operation(p) = &mut d.kind {
                                p.is_query = v;
                            }
                        },
                    ));
            }
            ElementKind::Extend(p) => {
                body = body.child(field(
                    t!("prop.condition").into(),
                    Input::new(&self.condition).small().into_any_element(),
                ));
                // The extension points of the extended use case, ticked where this extends.
                let points: Vec<(Id, String)> = model
                    .children(p.extended)
                    .iter()
                    .filter_map(|&c| model.get(c))
                    .filter(|c| matches!(c.kind(), ElementKind::ExtensionPoint))
                    .map(|c| (c.id(), c.name().to_owned()))
                    .collect();
                if !points.is_empty() {
                    body = body.child(heading(t!("prop.extension_points").to_string()));
                }
                for (ix, (point, name)) in points.into_iter().enumerate() {
                    let workspace = workspace.clone();
                    let checked = p.extension_points.contains(&point);
                    body = body.child(
                        Checkbox::new(("extension-point", ix))
                            .label(name)
                            .checked(checked)
                            .on_click(move |checked: &bool, _, cx| {
                                let checked = *checked;
                                workspace.update(cx, |ws, cx| {
                                    ws.update_element(id, |d| {
                                        if let ElementKind::Extend(p) = &mut d.kind {
                                            p.extension_points.retain(|&e| e != point);
                                            if checked {
                                                p.extension_points.push(point);
                                            }
                                        }
                                    });
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            ElementKind::Association(p) => {
                for (i, end) in p.ends.iter().enumerate() {
                    let participant =
                        model.get(end.participant).map(labels::display_name).unwrap_or_default();
                    let fields = &self.ends[i];
                    body = body
                        .child(heading(format!(
                            "{} {} – {participant}",
                            t!("prop.end"),
                            ["A", "B"][i]
                        )))
                        .child(field(
                            t!("prop.role").into(),
                            Input::new(&fields.role).small().into_any_element(),
                        ))
                        .child(field(
                            t!("prop.multiplicity").into(),
                            Input::new(&fields.multiplicity).small().into_any_element(),
                        ))
                        .child(field(
                            t!("prop.navigability").into(),
                            Select::new(&fields.navigability).small().into_any_element(),
                        ))
                        .child(field(
                            t!("prop.aggregation").into(),
                            Select::new(&fields.aggregation).small().into_any_element(),
                        ));
                }
            }
            ElementKind::Message(_) => {
                body = body
                    .child(field(
                        t!("prop.message_sort").into(),
                        Select::new(&self.sort).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.arguments").into(),
                        Input::new(&self.arguments).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.return_value").into(),
                        Input::new(&self.return_value).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.operation").into(),
                        Select::new(&self.operation).small().into_any_element(),
                    ));
            }
            ElementKind::Lifeline(_) => {
                body = body.child(field(
                    t!("prop.represents").into(),
                    Select::new(&self.represents).small().into_any_element(),
                ));
            }
            ElementKind::CombinedFragment(p) => {
                body = body
                    .child(field(
                        t!("prop.operator").into(),
                        Select::new(&self.operator).small().into_any_element(),
                    ))
                    .child(heading(t!("prop.operands").to_string()));
                for (ix, &operand) in model.children(id).iter().enumerate() {
                    let Some(ElementKind::InteractionOperand(o)) =
                        model.get(operand).map(|e| e.kind())
                    else {
                        continue;
                    };
                    let label = match o.guard.trim() {
                        "" => format!("{} {}", t!("kind.interactionOperand"), ix + 1),
                        guard => format!("[{guard}]"),
                    };
                    let workspace = workspace.clone();
                    body = body.child(
                        Button::new(("operand", ix)).ghost().small().label(label).on_click(
                            move |_, _, cx| {
                                workspace.update(cx, |ws, cx| {
                                    ws.selected_element = Some(operand);
                                    cx.notify();
                                });
                            },
                        ),
                    );
                }
                let add = workspace.clone();
                body = body.child(
                    Button::new("add-operand").small().label(t!("prop.add_operand")).on_click(
                        move |_, _, cx| {
                            add.update(cx, |ws, cx| {
                                ws.add_operand(id);
                                cx.notify();
                            });
                        },
                    ),
                );
                // The lifelines of the interaction; none ticked means "as the messages need".
                let interaction = model.ancestors(id).into_iter().find(|&a| {
                    model.get(a).is_some_and(|e| matches!(e.kind(), ElementKind::Interaction))
                });
                let lifelines: Vec<(Id, String)> = interaction
                    .into_iter()
                    .flat_map(|i| model.children(i).iter().copied())
                    .filter_map(|c| model.get(c))
                    .filter(|c| matches!(c.kind(), ElementKind::Lifeline(_)))
                    .map(|c| (c.id(), labels::display_name(c)))
                    .collect();
                body = body.child(heading(t!("prop.covered").to_string()));
                if p.covered.is_empty() {
                    body = body.child(
                        div().text_xs().text_color(muted).child(t!("prop.covered_automatic")),
                    );
                }
                for (ix, (lifeline, name)) in lifelines.into_iter().enumerate() {
                    let workspace = workspace.clone();
                    body = body.child(
                        Checkbox::new(("covered", ix))
                            .label(name)
                            .checked(p.covered.contains(&lifeline))
                            .on_click(move |checked: &bool, _, cx| {
                                let checked = *checked;
                                workspace.update(cx, |ws, cx| {
                                    ws.update_element(id, |d| {
                                        if let ElementKind::CombinedFragment(p) = &mut d.kind {
                                            p.covered.retain(|&l| l != lifeline);
                                            if checked {
                                                p.covered.push(lifeline);
                                            }
                                        }
                                    });
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            ElementKind::InteractionOperand(_) => {
                let delete = workspace.clone();
                body = body
                    .child(field(
                        t!("prop.guard").into(),
                        Input::new(&self.guard).small().into_any_element(),
                    ))
                    .child(
                        Button::new("delete-operand")
                            .small()
                            .label(t!("prop.delete_operand"))
                            .on_click(move |_, _, cx| {
                                delete.update(cx, |ws, cx| {
                                    ws.delete_element(id);
                                    cx.notify();
                                });
                            }),
                    );
            }
            _ => {}
        }

        let text_label = if matches!(kind, ElementKind::Comment(_)) {
            t!("prop.text")
        } else {
            t!("prop.documentation")
        };
        body = body
            .child(field(text_label.into(), Textarea::new(&self.text).small().into_any_element()))
            .child(field(
                t!("prop.owner").into(),
                div().text_sm().child(labels::owner_path(model, id)).into_any_element(),
            ));

        // Display options of the one selected node.
        // Fragments size themselves, so they have no display options.
        let is_fragment = |p: &umlcraft_model::Presentation| {
            p.element
                .and_then(|e| model.get(e))
                .is_some_and(|e| matches!(e.kind(), ElementKind::CombinedFragment(_)))
        };
        let presentation = match (ws.active_diagram, ws.selection.as_slice()) {
            (Some(d), [p]) => model
                .view(d)
                .and_then(|v| v.get(*p))
                .filter(|p| !p.is_edge() && !is_fragment(p))
                .cloned(),
            _ => None,
        };
        if let Some(p) = presentation {
            let shown = p.element.and_then(|e| model.get(e)).map(|e| e.kind().clone());
            body = body.child(heading(t!("prop.display").to_string()));
            let boxed = matches!(
                shown,
                Some(
                    ElementKind::Class(_)
                        | ElementKind::Interface(_)
                        | ElementKind::Enumeration(_)
                        | ElementKind::DataType(_)
                )
            );
            if boxed && !p.style.iconic {
                body = body
                    .child(style_flag(
                        "hide-attributes",
                        t!("prop.hide_attributes").into(),
                        p.style.hide_attributes,
                        workspace.clone(),
                        p.id,
                        |s, v| s.hide_attributes = v,
                    ))
                    .child(style_flag(
                        "hide-operations",
                        t!("prop.hide_operations").into(),
                        p.style.hide_operations,
                        workspace.clone(),
                        p.id,
                        |s, v| s.hide_operations = v,
                    ));
            }
            if matches!(shown, Some(ElementKind::Interface(_))) {
                body = body.child(style_flag(
                    "iconic",
                    t!("prop.iconic").into(),
                    p.style.iconic,
                    workspace.clone(),
                    p.id,
                    |s, v| s.iconic = v,
                ));
            }
            if let Some(b) = p.bounds() {
                body = body.child(field(
                    t!("prop.bounds").into(),
                    div()
                        .text_sm()
                        .child(format!("x {}, y {}, {} × {}", b.x, b.y, b.width, b.height))
                        .into_any_element(),
                ));
            }
        }
        body
    }
}
