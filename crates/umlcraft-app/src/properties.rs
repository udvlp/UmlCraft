//! Properties panel for the selected element and, if one node is selected, its presentation.
//!
//! Text fields commit on Enter or when they lose focus, so typing is one undo step, and always
//! into the element shown when editing began. Selects and checkboxes apply immediately.

use std::collections::HashMap;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState, ColorSelect};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme, IndexPath, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{
    ActivityNodeKind, Aggregation, AssociationEnd, AssociationProps, Cardinality, Color,
    DiagramKind, ElementData, ElementKind, ErAction, ErDisplay, ErIndex, ErKey, ErNotation, Id,
    InstanceKind, InstanceProps, InteractionOperator, LifelineProps, LinkEnd, LinkProps,
    MessageSort, Model, Multiplicity, Navigability, ObjectEffect, ObjectOrdering,
    ParameterDirections, Presentation, Shape, Style, TemplateParameterKind, TypeRef, Visibility,
};
use umlcraft_notation::member::{self, member_text};
use umlcraft_notation::object;
use umlcraft_notation::sequence::operator_keyword;

use crate::labels::{self, VISIBILITIES};
use crate::nsd::{NsdProperties, NsdView};
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
const PARAMETER_KINDS: [TemplateParameterKind; 2] =
    [TemplateParameterKind::Type, TemplateParameterKind::Value];
const DIRECTIONS: [ParameterDirections; 3] =
    [ParameterDirections::Explicit, ParameterDirections::Hidden, ParameterDirections::All];
/// Colors offered first for names and lines: black, grays and the plain colors of diagrams.
pub(crate) const FEATURED_COLORS: [Color; 12] = [
    Color::rgb(0x00, 0x00, 0x00),
    Color::rgb(0x80, 0x80, 0x80),
    Color::rgb(0xc0, 0xc0, 0xc0),
    Color::rgb(0xff, 0x00, 0x00),
    Color::rgb(0x80, 0x00, 0x00),
    Color::rgb(0xff, 0x80, 0x00),
    Color::rgb(0x00, 0x80, 0x00),
    Color::rgb(0x00, 0x80, 0x80),
    Color::rgb(0x00, 0x00, 0xff),
    Color::rgb(0x00, 0x00, 0x80),
    Color::rgb(0x80, 0x00, 0x80),
    Color::rgb(0xff, 0x00, 0xff),
];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Field {
    Name,
    Declaration,
    Stereotypes,
    Text,
    Role(usize),
    Multiplicity(usize),
    Constraints(usize),
    Condition,
    Arguments,
    ReturnValue,
    Guard,
    Trigger,
    Effect,
    Entry,
    DoActivity,
    Exit,
    ValueType,
    Bounds,
    Default,
    MemberMultiplicity,
    Slots,
    /// The element's own constraints, one per line.
    ElementConstraints,
    InState,
    UpperBound,
    Binding,
    PhysicalName,
    Datatype,
    LengthPrecision,
    Cardinality,
    /// An ER attribute's or domain's condition.
    ErCheck,
    /// An ER domain's values, comma-separated.
    ErValues,
    /// An ER entity's conditions and indexes, one per line.
    ErChecks,
    ErIndexes,
}

/// Inputs for one association end.
struct EndFields {
    role: Entity<InputState>,
    multiplicity: Entity<InputState>,
    constraints: Entity<InputState>,
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
    /// An object's slots, one `name = value` per line.
    slots: Entity<TextareaState>,
    /// The element's constraints, one per line, without braces.
    constraints: Entity<TextareaState>,
    visibility: Entity<Choice>,
    ends: [EndFields; 2],
    arguments: Entity<InputState>,
    return_value: Entity<InputState>,
    guard: Entity<InputState>,
    trigger: Entity<InputState>,
    effect: Entity<InputState>,
    entry: Entity<InputState>,
    do_activity: Entity<InputState>,
    exit: Entity<InputState>,
    /// Template parameters: the type of a value parameter, the bounds and the default. Object
    /// nodes: the type of their objects. Attributes: their type and initial value; operations:
    /// their return type.
    value_type: Entity<InputState>,
    /// An attribute's multiplicity.
    member_multiplicity: Entity<InputState>,
    /// Object nodes: the states their objects are in, without brackets.
    in_state: Entity<InputState>,
    /// Object nodes: how many objects they hold at most.
    upper_bound: Entity<InputState>,
    /// Object nodes: the order objects leave in, or none.
    ordering: Entity<Choice>,
    /// Object nodes: what the action does to the objects, or nothing said.
    object_effect: Entity<Choice>,
    bounds: Entity<InputState>,
    default: Entity<InputState>,
    /// Classes: the template they bind, with its arguments (`Pair<int,Holder>`).
    binding: Entity<InputState>,
    /// ER entities and attributes: their names in the database, and an attribute's column.
    physical_name: Entity<InputState>,
    datatype: Entity<InputState>,
    length_precision: Entity<InputState>,
    /// ER participations: how often the entity takes part, `[0,n]`.
    cardinality: Entity<InputState>,
    /// ER attributes and domains: their condition; domains: their values.
    er_check: Entity<InputState>,
    er_values: Entity<InputState>,
    /// ER entities: their conditions and indexes, one per line.
    er_checks: Entity<TextareaState>,
    er_indexes: Entity<TextareaState>,
    /// ER attributes: no key, key or partial key.
    er_key: Entity<Choice>,
    /// IS-A: subtypes not stated, disjoint or overlapping.
    disjoint: Entity<Choice>,
    /// ER relationships: what their foreign keys do on deleting and updating.
    on_delete: Entity<Choice>,
    on_update: Entity<Choice>,
    /// ER diagrams: Chen, IE or IDEF1X, and what IE and IDEF1X tables list.
    er_notation: Entity<Choice>,
    er_display: Entity<Choice>,
    parameter_kind: Entity<Choice>,
    sort: Entity<Choice>,
    operator: Entity<Choice>,
    /// Operations a message can invoke, with their ids (`None` for "no operation").
    operation: Entity<Choice>,
    operation_ids: Vec<Option<Id>>,
    /// Classifiers a lifeline can represent, with their ids (`None` for "none").
    represents: Entity<Choice>,
    represents_ids: Vec<Option<Id>>,
    /// Associations a link can be an instance of, with their ids (`None` for "none").
    association: Entity<Choice>,
    association_ids: Vec<Option<Id>>,
    /// Classifier boxes: which parameter directions the operations show.
    directions: Entity<Choice>,
    /// The name's color of the selected node or line, and the line's color.
    font_color: Entity<ColorPickerState>,
    line_color: Entity<ColorPickerState>,
    /// Undo depth before a color was picked and after its last change: while the picker stays
    /// open, its further changes (dragging a slider) join that one undo step.
    color_step: Option<(usize, usize)>,
    /// Element, selected presentation, document revision and locale the fields currently show.
    shown: Option<(Id, Option<Id>, u64, String)>,
    /// Text fields the user has typed into, each with the element shown at the first keystroke.
    /// An edit always goes back to that element, even if the selection has changed by the time
    /// the field loses focus; fields nobody typed into are never written back.
    editing: HashMap<Field, Id>,
    /// The structogram's fields, shown instead while something of the structogram shown is
    /// selected.
    structogram: Entity<NsdProperties>,
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

fn parameter_kind_items() -> Vec<SharedString> {
    ["template_kind.type", "template_kind.value"].map(|k| t!(k).to_string().into()).to_vec()
}

fn direction_items() -> Vec<SharedString> {
    ["explicit", "hidden", "all"]
        .map(|k| t!(format!("prop.parameter_directions_{k}")).to_string().into())
        .to_vec()
}

const ER_KEYS: [Option<ErKey>; 3] = [None, Some(ErKey::Primary), Some(ErKey::Partial)];
/// An IS-A's subtypes: not stated, disjoint, overlapping.
const DISJOINT: [Option<bool>; 3] = [None, Some(true), Some(false)];

fn er_key_items() -> Vec<SharedString> {
    ["prop.er_key_none", "prop.er_key_primary", "prop.er_key_partial"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

const ER_NOTATIONS: [ErNotation; 3] = [ErNotation::Chen, ErNotation::Ie, ErNotation::Idef1x];
const ER_DISPLAYS: [ErDisplay; 3] = [ErDisplay::Attributes, ErDisplay::Keys, ErDisplay::Name];

fn er_notation_items() -> Vec<SharedString> {
    ["ribbon.notation.chen", "ribbon.notation.ie", "ribbon.notation.idef1x"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

fn er_display_items() -> Vec<SharedString> {
    ["prop.er_display_attributes", "prop.er_display_keys", "prop.er_display_name"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

/// A foreign key's actions: the database's default, then SQL's.
const ER_ACTIONS: [Option<ErAction>; 6] = [
    None,
    Some(ErAction::Cascade),
    Some(ErAction::SetNull),
    Some(ErAction::SetDefault),
    Some(ErAction::Restrict),
    Some(ErAction::NoAction),
];

fn er_action_items() -> Vec<SharedString> {
    [
        "prop.er_action_default",
        "prop.er_action_cascade",
        "prop.er_action_set_null",
        "prop.er_action_set_default",
        "prop.er_action_restrict",
        "prop.er_action_no_action",
    ]
    .map(|k| t!(k).to_string().into())
    .to_vec()
}

fn disjoint_items() -> Vec<SharedString> {
    ["prop.er_disjoint_unspecified", "prop.er_disjoint_yes", "prop.er_disjoint_no"]
        .map(|k| t!(k).to_string().into())
        .to_vec()
}

/// A picked color as the model keeps it.
fn model_color(color: Hsla) -> Color {
    let c = Rgba::from(color);
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color { r: byte(c.r), g: byte(c.g), b: byte(c.b), a: byte(c.a) }
}

/// The one node or line selected on the diagram shown.
fn selected_presentation(ws: &Workspace) -> Option<&Presentation> {
    match (ws.active_diagram, ws.selection.as_slice()) {
        (Some(d), [p]) => ws.doc.model().view(d)?.get(*p),
        _ => None,
    }
}

/// Splits `text` at the commas that are not inside `<…>`: "A, Map<K, V>" is two types.
fn split_types(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts.into_iter().map(str::trim).filter(|p| !p.is_empty()).collect()
}

/// "—" for no ordering, then UML's orderings as UML writes them.
fn ordering_items() -> Vec<SharedString> {
    std::iter::once("—")
        .chain(ObjectOrdering::ALL.iter().map(|o| o.as_str()))
        .map(SharedString::from)
        .collect()
}

/// "—" for no effect, then UML's effects as UML writes them.
fn effect_items() -> Vec<SharedString> {
    std::iter::once("—")
        .chain(ObjectEffect::ALL.iter().map(|o| o.as_str()))
        .map(SharedString::from)
        .collect()
}

fn operator_items() -> Vec<SharedString> {
    OPERATORS.iter().map(|&o| operator_keyword(o).into()).collect()
}

/// Choices for a reference: "—" for none, then `targets` by name.
fn reference_items(model: &Model, targets: &[Option<Id>]) -> Vec<SharedString> {
    targets
        .iter()
        .map(|t| match t.and_then(|t| model.get(t)) {
            // An unnamed association reads as the classes it connects: "Customer – Order".
            Some(e) if e.name().is_empty() && matches!(e.kind(), ElementKind::Association(_)) => {
                let ElementKind::Association(p) = e.kind() else { unreachable!() };
                let name = |end: &AssociationEnd| {
                    model.get(end.participant).map(labels::display_name).unwrap_or_default()
                };
                format!("{} – {}", name(&p.ends[0]), name(&p.ends[1])).into()
            }
            Some(e) => labels::display_name(e).into(),
            None => "—".into(),
        })
        .collect()
}

/// The associations a link can be an instance of: those between the classifiers of its
/// objects, either way round.
fn associations_for(model: &Model, link: &LinkProps) -> Vec<Option<Id>> {
    let classifier = |end: &LinkEnd| match model.get(end.instance).map(|e| e.kind()) {
        Some(ElementKind::Instance(p)) => p.classifier,
        _ => None,
    };
    let (a, b) = (classifier(&link.ends[0]), classifier(&link.ends[1]));
    let fits = |p: &AssociationProps| {
        let (x, y) = (Some(p.ends[0].participant), Some(p.ends[1].participant));
        (x == a && y == b) || (x == b && y == a)
    };
    let found = model.elements().filter_map(|e| match e.kind() {
        ElementKind::Association(p) if fits(p) => Some(Some(e.id())),
        _ => None,
    });
    std::iter::once(None).chain(found).collect()
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

/// The lifelines a combined fragment or interaction use covers.
fn covered_of(kind: &ElementKind) -> Option<&[Id]> {
    match kind {
        ElementKind::CombinedFragment(p) => Some(&p.covered),
        ElementKind::InteractionUse(p) => Some(&p.covered),
        _ => None,
    }
}

fn covered_of_mut(kind: &mut ElementKind) -> Option<&mut Vec<Id>> {
    match kind {
        ElementKind::CombinedFragment(p) => Some(&mut p.covered),
        ElementKind::InteractionUse(p) => Some(&mut p.covered),
        _ => None,
    }
}

/// The interactions an interaction use can refer to, or the activities a call behavior action
/// can call: the elements whose kind `pick` accepts, by name.
fn behaviors(model: &Model, pick: impl Fn(&ElementKind) -> bool) -> Vec<Option<Id>> {
    let mut all: Vec<(&str, Id)> =
        model.elements().filter(|e| pick(e.kind())).map(|e| (e.name(), e.id())).collect();
    all.sort_by(|a, b| a.0.cmp(b.0));
    std::iter::once(None).chain(all.into_iter().map(|(_, id)| Some(id))).collect()
}

/// The state machines the submachine state `state` can stand for, by name: all but the one it
/// belongs to.
fn submachines(model: &Model, state: Id) -> Vec<Option<Id>> {
    let own = model
        .ancestors(state)
        .into_iter()
        .find(|&a| model.get(a).is_some_and(|a| matches!(a.kind(), ElementKind::StateMachine)));
    behaviors(model, |k| matches!(k, ElementKind::StateMachine))
        .into_iter()
        .filter(|m| m.is_some() && *m != own)
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
        let stereotypes = cx
            .new(|cx| InputState::new(window, cx).placeholder(t!("prop.stereotypes_placeholder")));
        let text = cx.new(|cx| TextareaState::new(window, cx).auto_grow(3, 12));
        let slots = cx.new(|cx| TextareaState::new(window, cx).auto_grow(2, 12));
        let constraints = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .placeholder(t!("prop.element_constraints_placeholder"))
        });
        let visibility = cx.new(|cx| SelectState::new(visibility_items(), None, window, cx));
        let end = |window: &mut Window, cx: &mut Context<Self>| EndFields {
            role: cx.new(|cx| InputState::new(window, cx)),
            multiplicity: cx.new(|cx| InputState::new(window, cx).placeholder("0..*")),
            constraints: cx.new(|cx| {
                InputState::new(window, cx).placeholder(t!("prop.constraints_placeholder"))
            }),
            navigability: cx.new(|cx| SelectState::new(navigability_items(), None, window, cx)),
            aggregation: cx.new(|cx| SelectState::new(aggregation_items(), None, window, cx)),
        };
        let ends = [end(window, cx), end(window, cx)];
        let arguments = input(window, cx);
        let return_value = input(window, cx);
        let guard = input(window, cx);
        let trigger = input(window, cx);
        let effect = input(window, cx);
        let entry = input(window, cx);
        let do_activity = input(window, cx);
        let exit = input(window, cx);
        let value_type = input(window, cx);
        let in_state =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("prop.in_state_placeholder")));
        let upper_bound = cx.new(|cx| InputState::new(window, cx).placeholder("*"));
        let ordering = cx.new(|cx| SelectState::new(ordering_items(), None, window, cx));
        let object_effect = cx.new(|cx| SelectState::new(effect_items(), None, window, cx));
        let bounds = cx.new(|cx| InputState::new(window, cx).placeholder("Number, Comparable<T>"));
        let default = input(window, cx);
        let member_multiplicity = cx.new(|cx| InputState::new(window, cx).placeholder("0..*"));
        let binding = cx.new(|cx| InputState::new(window, cx).placeholder("List<String>"));
        let physical_name = input(window, cx);
        let datatype = cx.new(|cx| InputState::new(window, cx).placeholder("VARCHAR"));
        let length_precision = cx.new(|cx| InputState::new(window, cx).placeholder("10,2"));
        let cardinality = cx.new(|cx| InputState::new(window, cx).placeholder("[0,n]"));
        let er_check = cx.new(|cx| InputState::new(window, cx).placeholder("preis >= 0"));
        let er_values = cx.new(|cx| InputState::new(window, cx).placeholder("rot, grün, blau"));
        let er_checks = cx
            .new(|cx| TextareaState::new(window, cx).auto_grow(1, 8).placeholder("ende > beginn"));
        let er_indexes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .placeholder(t!("prop.er_indexes_placeholder"))
        });
        let er_key = cx.new(|cx| SelectState::new(er_key_items(), None, window, cx));
        let disjoint = cx.new(|cx| SelectState::new(disjoint_items(), None, window, cx));
        let on_delete = cx.new(|cx| SelectState::new(er_action_items(), None, window, cx));
        let on_update = cx.new(|cx| SelectState::new(er_action_items(), None, window, cx));
        let er_notation = cx.new(|cx| SelectState::new(er_notation_items(), None, window, cx));
        let er_display = cx.new(|cx| SelectState::new(er_display_items(), None, window, cx));
        let parameter_kind =
            cx.new(|cx| SelectState::new(parameter_kind_items(), None, window, cx));
        let sort = cx.new(|cx| SelectState::new(sort_items(), None, window, cx));
        let operator = cx.new(|cx| SelectState::new(operator_items(), None, window, cx));
        let operation = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let represents = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let association = cx.new(|cx| SelectState::new(Vec::new(), None, window, cx));
        let directions = cx.new(|cx| SelectState::new(direction_items(), None, window, cx));
        let font_color = cx.new(|cx| ColorPickerState::new(window, cx));
        let line_color = cx.new(|cx| ColorPickerState::new(window, cx));

        let structogram = cx.new(|cx| NsdProperties::new(workspace.clone(), window, cx));
        let mut subscriptions =
            vec![cx.observe_in(&workspace, window, |this, _, window, cx| this.sync(window, cx))];
        // The structogram's selection decides what the panel shows.
        subscriptions.push(cx.observe(&structogram, |_, _, cx| cx.notify()));
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
        watch(&trigger, Field::Trigger, cx);
        watch(&effect, Field::Effect, cx);
        watch(&entry, Field::Entry, cx);
        watch(&do_activity, Field::DoActivity, cx);
        watch(&exit, Field::Exit, cx);
        watch(&value_type, Field::ValueType, cx);
        watch(&in_state, Field::InState, cx);
        watch(&upper_bound, Field::UpperBound, cx);
        watch(&bounds, Field::Bounds, cx);
        watch(&default, Field::Default, cx);
        watch(&member_multiplicity, Field::MemberMultiplicity, cx);
        watch(&binding, Field::Binding, cx);
        watch(&physical_name, Field::PhysicalName, cx);
        watch(&datatype, Field::Datatype, cx);
        watch(&length_precision, Field::LengthPrecision, cx);
        watch(&cardinality, Field::Cardinality, cx);
        watch(&er_check, Field::ErCheck, cx);
        watch(&er_values, Field::ErValues, cx);
        for (i, end) in ends.iter().enumerate() {
            watch(&end.role, Field::Role(i), cx);
            watch(&end.multiplicity, Field::Multiplicity(i), cx);
            watch(&end.constraints, Field::Constraints(i), cx);
        }
        for (state, field) in [(&er_checks, Field::ErChecks), (&er_indexes, Field::ErIndexes)] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, ev: &InputEvent, _, cx| {
                    if !matches!(ev, InputEvent::PressEnter { .. }) {
                        this.on_input(field, ev, cx);
                    }
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(&text, window, |this, _, ev: &InputEvent, _, cx| {
            // Enter makes a new line in the text area; only leaving it commits.
            if !matches!(ev, InputEvent::PressEnter { .. }) {
                this.on_input(Field::Text, ev, cx);
            }
        }));
        subscriptions.push(cx.subscribe_in(&slots, window, |this, _, ev: &InputEvent, _, cx| {
            if !matches!(ev, InputEvent::PressEnter { .. }) {
                this.on_input(Field::Slots, ev, cx);
            }
        }));
        subscriptions.push(cx.subscribe_in(
            &constraints,
            window,
            |this, _, ev: &InputEvent, _, cx| {
                if !matches!(ev, InputEvent::PressEnter { .. }) {
                    this.on_input(Field::ElementConstraints, ev, cx);
                }
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &association,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(&target) = selected(state, cx).and_then(|ix| this.association_ids.get(ix))
                else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::Link(p) = &mut data.kind {
                        p.association = target;
                    }
                });
            },
        ));
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
            &parameter_kind,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(k) = selected(state, cx).and_then(|ix| PARAMETER_KINDS.get(ix).copied())
                else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::TemplateParameter(p) = &mut data.kind {
                        p.parameter = k;
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
            &ordering,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(ix) = selected(state, cx) else { return };
                let ordering = ix.checked_sub(1).and_then(|i| ObjectOrdering::ALL.get(i).copied());
                this.apply_shown(cx, move |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.ordering = ordering;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &object_effect,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(ix) = selected(state, cx) else { return };
                let effect = ix.checked_sub(1).and_then(|i| ObjectEffect::ALL.get(i).copied());
                this.apply_shown(cx, move |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.effect = effect;
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
                this.apply_shown(cx, move |data| match &mut data.kind {
                    ElementKind::Lifeline(p) => p.represents = target,
                    ElementKind::Instance(p) => p.classifier = target,
                    ElementKind::InteractionUse(p) => p.refers_to = target,
                    ElementKind::ActivityNode(p) => p.behavior = target,
                    // A submachine state always stands for a state machine.
                    ElementKind::State(p) if target.is_some() => p.submachine = target,
                    _ => {}
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &er_key,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(key) = selected(state, cx).and_then(|ix| ER_KEYS.get(ix).copied()) else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::ErAttribute(p) = &mut data.kind {
                        p.key = key;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &er_notation,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(n) = selected(state, cx).and_then(|ix| ER_NOTATIONS.get(ix).copied())
                else {
                    return;
                };
                let Some(diagram) = this.shown_id() else { return };
                // Switching converts what the diagram shows, which a plain change does not.
                this.workspace.update(cx, |ws, cx| {
                    ws.set_er_notation(diagram, n);
                    cx.notify();
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &er_display,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(d) = selected(state, cx).and_then(|ix| ER_DISPLAYS.get(ix).copied())
                else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::Diagram(p) = &mut data.kind {
                        p.er_display = d;
                    }
                });
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &disjoint,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(d) = selected(state, cx).and_then(|ix| DISJOINT.get(ix).copied()) else {
                    return;
                };
                this.apply_shown(cx, move |data| {
                    if let ElementKind::ErIsA(p) = &mut data.kind {
                        p.disjoint = d;
                    }
                });
            },
        ));
        for (state, delete) in [(&on_delete, true), (&on_update, false)] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                    let Some(a) = selected(state, cx).and_then(|ix| ER_ACTIONS.get(ix).copied())
                    else {
                        return;
                    };
                    this.apply_shown(cx, move |data| {
                        if let ElementKind::ErRelationship(p) = &mut data.kind {
                            if delete {
                                p.on_delete = a;
                            } else {
                                p.on_update = a;
                            }
                        }
                    });
                },
            ));
        }
        subscriptions.push(cx.subscribe_in(
            &directions,
            window,
            |this, state, _: &SelectEvent<Vec<SharedString>>, _, cx| {
                let Some(d) = selected(state, cx).and_then(|ix| DIRECTIONS.get(ix).copied()) else {
                    return;
                };
                this.update_style(cx, move |s| s.parameter_directions = d);
            },
        ));
        for (state, line) in [(&font_color, false), (&line_color, true)] {
            subscriptions.push(cx.subscribe_in(
                state,
                window,
                move |this, _, ColorPickerEvent::Change(color), _, cx| {
                    this.set_color(line, color.map(model_color), cx);
                },
            ));
            // Closing the picker ends its undo step.
            subscriptions.push(cx.observe(state, |this, state, cx| {
                if !state.read(cx).is_open() {
                    this.color_step = None;
                }
            }));
        }
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
                    this.apply_shown(cx, move |data| match &mut data.kind {
                        ElementKind::Association(p) => p.ends[i].navigability = n,
                        ElementKind::Link(p) => p.ends[i].navigability = n,
                        _ => {}
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
                    this.apply_shown(cx, move |data| match &mut data.kind {
                        ElementKind::Association(p) => p.ends[i].aggregation = a,
                        ElementKind::Link(p) => p.ends[i].aggregation = a,
                        _ => {}
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
            slots,
            constraints,
            visibility,
            ends,
            arguments,
            return_value,
            guard,
            trigger,
            effect,
            entry,
            do_activity,
            exit,
            value_type,
            member_multiplicity,
            in_state,
            upper_bound,
            ordering,
            object_effect,
            bounds,
            default,
            binding,
            physical_name,
            datatype,
            length_precision,
            cardinality,
            er_check,
            er_values,
            er_checks,
            er_indexes,
            er_key,
            disjoint,
            on_delete,
            on_update,
            er_notation,
            er_display,
            parameter_kind,
            sort,
            operator,
            operation,
            operation_ids: Vec::new(),
            represents,
            represents_ids: Vec::new(),
            association,
            association_ids: Vec::new(),
            directions,
            font_color,
            line_color,
            color_step: None,
            shown: None,
            editing: HashMap::new(),
            structogram,
            _subscriptions: subscriptions,
        };
        panel.sync(window, cx);
        panel
    }

    /// Edits the structogram `editor` edits while it is shown.
    pub fn show_structogram(
        &mut self,
        editor: Option<Entity<NsdView>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.structogram.update(cx, |s, cx| s.show(editor, window, cx));
    }

    /// What the panel's heading names while it shows the structogram's selection.
    pub fn structogram_heading(&self, cx: &App) -> Option<String> {
        let s = self.structogram.read(cx);
        s.is_active(cx).then(|| s.heading()).flatten()
    }

    /// Puts the cursor into the name (or declaration) field of `element`, e.g. for "Rename".
    /// Decided by the element, as the panel may still show the one selected before.
    pub fn focus_name(&self, element: Id, window: &mut Window, cx: &mut Context<Self>) {
        let is_member =
            self.workspace.read(cx).doc.model().get(element).is_some_and(|e| is_member(e.kind()));
        let field = if is_member { &self.declaration } else { &self.name };
        field.update(cx, |state, cx| state.focus(window, cx));
    }

    fn shown_id(&self) -> Option<Id> {
        self.shown.as_ref().map(|(id, ..)| *id)
    }

    /// Changes the display options of the selected node or line as one undo step.
    fn update_style(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Style)) {
        self.workspace.update(cx, |ws, cx| {
            let Some(p) = selected_presentation(ws).map(|p| p.id) else { return };
            ws.update_style(p, f);
            cx.notify();
        });
    }

    /// Sets the name's color (or with `line` the line's) of the selected node or line; `None`
    /// goes back to the theme's. What one opened picker changes is one undo step.
    fn set_color(&mut self, line: bool, color: Option<Color>, cx: &mut Context<Self>) {
        let step = self.color_step;
        self.color_step = self.workspace.update(cx, |ws, cx| {
            let p = selected_presentation(ws)?.id;
            let depth = ws.doc.undo_depth();
            let start = match step {
                Some((start, end)) if end == depth => start,
                _ => depth,
            };
            ws.update_style(p, |s| *if line { &mut s.stroke } else { &mut s.font_color } = color);
            ws.doc.squash_since(start, t!("edit.properties"));
            cx.notify();
            Some((start, ws.doc.undo_depth()))
        });
    }

    fn on_input(&mut self, field: Field, ev: &InputEvent, cx: &mut Context<Self>) {
        // Loading values into the fields emits no events, so `Change` means the user typed.
        match ev {
            InputEvent::Change => {
                if let Some(id) = self.shown_id() {
                    self.editing.entry(field).or_insert(id);
                }
            }
            InputEvent::PressEnter { .. } | InputEvent::Blur => {
                if let Some(id) = self.editing.remove(&field) {
                    self.commit(field, id, cx);
                }
            }
            InputEvent::Focus => {}
        }
    }

    /// Writes back whatever has been typed but not yet committed, e.g. before saving.
    pub fn flush(&mut self, cx: &mut Context<Self>) {
        for (field, id) in std::mem::take(&mut self.editing) {
            self.commit(field, id, cx);
        }
    }

    fn value(state: &Entity<InputState>, cx: &App) -> String {
        state.read(cx).value().trim().to_owned()
    }

    /// Writes one field back to element `id`.
    fn commit(&mut self, field: Field, id: Id, cx: &mut Context<Self>) {
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
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::Message(p) => p.arguments = value,
                    ElementKind::InteractionUse(p) => p.arguments = value,
                    _ => {}
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
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::InteractionOperand(p) => p.guard = value,
                    ElementKind::ActivityEdge(p) => p.guard = value,
                    ElementKind::Transition(p) => p.guard = value,
                    ElementKind::InternalTransition(p) => p.guard = value,
                    ElementKind::Message(p) => p.guard = value,
                    _ => {}
                });
            }
            Field::Trigger | Field::Effect => {
                let state = if field == Field::Trigger { &self.trigger } else { &self.effect };
                let value = Self::value(state, cx);
                self.apply(id, cx, move |data| {
                    let (trigger, effect) = match &mut data.kind {
                        ElementKind::Transition(p) => (&mut p.trigger, &mut p.effect),
                        ElementKind::InternalTransition(p) => (&mut p.trigger, &mut p.effect),
                        _ => return,
                    };
                    *(if field == Field::Trigger { trigger } else { effect }) = value;
                });
            }
            Field::Entry | Field::DoActivity | Field::Exit => {
                let state = match field {
                    Field::Entry => &self.entry,
                    Field::DoActivity => &self.do_activity,
                    _ => &self.exit,
                };
                let value = Self::value(state, cx);
                self.apply(id, cx, move |data| {
                    if let ElementKind::State(p) = &mut data.kind {
                        let target = match field {
                            Field::Entry => &mut p.entry,
                            Field::DoActivity => &mut p.do_activity,
                            _ => &mut p.exit,
                        };
                        *target = value;
                    }
                });
            }
            Field::ValueType | Field::Bounds | Field::Default => {
                let state = match field {
                    Field::ValueType => &self.value_type,
                    Field::Bounds => &self.bounds,
                    _ => &self.default,
                };
                let text = Self::value(state, cx);
                // Names resolve as in a declaration, so `T` finds a sibling parameter.
                let model = self.workspace.read(cx).doc.model();
                let types: Vec<TypeRef> = split_types(&text)
                    .into_iter()
                    .map(|t| member::parse_type(model, id, t))
                    .collect();
                let single = (!text.is_empty()).then(|| member::parse_type(model, id, &text));
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::TemplateParameter(p) => match field {
                        Field::ValueType => p.type_ref = single,
                        Field::Bounds => p.bounds = types,
                        _ => p.default = single,
                    },
                    ElementKind::ActivityNode(p) if field == Field::ValueType => {
                        p.type_ref = single;
                    }
                    ElementKind::Attribute(p) => match field {
                        Field::ValueType => p.type_ref = single,
                        Field::Default => p.default_value = text,
                        _ => {}
                    },
                    ElementKind::Operation(p) if field == Field::ValueType => {
                        p.return_type = single;
                    }
                    ElementKind::ErAttribute(p) if field == Field::Default => {
                        p.default_value = text;
                    }
                    ElementKind::ErDomain(p) if field == Field::Default => {
                        p.default_value = text;
                    }
                    _ => {}
                });
            }
            Field::PhysicalName | Field::Datatype | Field::LengthPrecision => {
                let state = match field {
                    Field::PhysicalName => &self.physical_name,
                    Field::Datatype => &self.datatype,
                    _ => &self.length_precision,
                };
                let value = Self::value(state, cx);
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::ErEntity(p) if field == Field::PhysicalName => {
                        p.physical_name = value;
                    }
                    ElementKind::ErAttribute(p) => match field {
                        Field::PhysicalName => p.physical_name = value,
                        Field::Datatype => p.datatype = value,
                        _ => p.length_precision = value,
                    },
                    ElementKind::ErDomain(p) => match field {
                        Field::Datatype => p.datatype = value,
                        Field::LengthPrecision => p.length_precision = value,
                        _ => {}
                    },
                    _ => {}
                });
            }
            Field::ErCheck | Field::ErValues => {
                let state = if field == Field::ErCheck { &self.er_check } else { &self.er_values };
                let value = Self::value(state, cx);
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::ErAttribute(p) if field == Field::ErCheck => p.check = value,
                    ElementKind::ErDomain(p) if field == Field::ErCheck => p.check = value,
                    ElementKind::ErDomain(p) => {
                        p.values = value
                            .split(',')
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(str::to_owned)
                            .collect();
                    }
                    _ => {}
                });
            }
            Field::ErChecks | Field::ErIndexes => {
                let state =
                    if field == Field::ErChecks { &self.er_checks } else { &self.er_indexes };
                let text = state.read(cx).value().to_string();
                let lines: Vec<&str> =
                    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
                let checks: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
                let indexes: Vec<ErIndex> = lines.iter().filter_map(|l| parse_index(l)).collect();
                self.apply(id, cx, move |data| {
                    if let ElementKind::ErEntity(p) = &mut data.kind {
                        if field == Field::ErChecks {
                            p.checks = checks;
                        } else {
                            p.indexes = indexes;
                        }
                    }
                });
            }
            Field::Cardinality => {
                let text = Self::value(&self.cardinality, cx);
                let parsed =
                    if text.is_empty() { Ok(None) } else { text.parse::<Cardinality>().map(Some) };
                self.workspace.update(cx, |ws, cx| {
                    match parsed {
                        Ok(c) => ws.set_association_end(id, 0, crate::EndChange::Cardinality(c)),
                        Err(err) => ws.complain(crate::errors::cardinality_error(&err)),
                    }
                    cx.notify();
                });
            }
            Field::MemberMultiplicity => {
                let text = Self::value(&self.member_multiplicity, cx);
                let parsed =
                    if text.is_empty() { Ok(None) } else { text.parse::<Multiplicity>().map(Some) };
                match parsed {
                    Ok(m) => self.apply(id, cx, move |data| {
                        if let ElementKind::Attribute(p) = &mut data.kind {
                            p.multiplicity = m;
                        }
                    }),
                    Err(err) => self.workspace.update(cx, |ws, cx| {
                        ws.complain(crate::errors::multiplicity_error(&err));
                        cx.notify();
                    }),
                }
            }
            Field::Binding => {
                // A template with its arguments, linked as a declaration links its types; text
                // that names no template is ignored.
                let text = Self::value(&self.binding, cx);
                let model = self.workspace.read(cx).doc.model();
                let current = match model.get(id).map(|e| e.kind()) {
                    Some(ElementKind::Class(p)) => p.binding.clone(),
                    _ => return,
                };
                let binding = (!text.is_empty()).then(|| member::parse_type(model, id, &text));
                let names_template = matches!(&binding, Some(TypeRef::Expr(e)) if matches!(e.base, TypeRef::Element(_)));
                if binding != current && (binding.is_none() || names_template) {
                    self.workspace.update(cx, |ws, cx| {
                        ws.set_binding(id, binding);
                        cx.notify();
                    });
                }
            }
            Field::InState => {
                let value = Self::value(&self.in_state, cx);
                let value = value.strip_prefix('[').unwrap_or(&value);
                let value = value.strip_suffix(']').unwrap_or(value).trim().to_owned();
                self.apply(id, cx, move |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.in_state = value;
                    }
                });
            }
            Field::UpperBound => {
                let value = Self::value(&self.upper_bound, cx).trim().to_owned();
                self.apply(id, cx, move |data| {
                    if let ElementKind::ActivityNode(p) = &mut data.kind {
                        p.upper_bound = value;
                    }
                });
            }
            Field::Role(i) => {
                let value = Self::value(&self.ends[i].role, cx);
                self.apply(id, cx, move |data| match &mut data.kind {
                    ElementKind::Association(p) => p.ends[i].role = value,
                    ElementKind::Link(p) => p.ends[i].role = value,
                    ElementKind::ErParticipation(p) => p.role = value,
                    _ => {}
                });
            }
            Field::ElementConstraints => {
                // One per line; braces typed around one are dropped.
                let text = self.constraints.read(cx).value().to_string();
                let list: Vec<String> = text
                    .lines()
                    .map(|line| line.trim().trim_start_matches('{').trim_end_matches('}').trim())
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect();
                self.apply(id, cx, move |data| data.constraints = list);
            }
            Field::Slots => {
                // One `name = value` per line; names that are no slot of the object are ignored.
                let text = self.slots.read(cx).value().to_string();
                let values: Vec<(String, String)> = text
                    .lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
                    .collect();
                self.workspace.update(cx, |ws, cx| {
                    ws.set_slots(id, &values);
                    cx.notify();
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
                        ws.complain(crate::errors::multiplicity_error(&err));
                        cx.notify();
                    }),
                }
            }
            Field::Constraints(i) => {
                // Written without braces, separated by commas: "ordered, unique".
                let list: Vec<String> = Self::value(&self.ends[i].constraints, cx)
                    .split(',')
                    .map(|c| {
                        c.trim().trim_start_matches('{').trim_end_matches('}').trim().to_owned()
                    })
                    .filter(|c| !c.is_empty())
                    .collect();
                self.apply(id, cx, move |data| {
                    if let ElementKind::Association(p) = &mut data.kind {
                        p.ends[i].constraints = list;
                    }
                });
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

    /// Loads the selected element into the fields when it, the document or the language
    /// changed.
    pub fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = |this: &Self, cx: &App| {
            let ws = this.workspace.read(cx);
            let presentation = selected_presentation(ws).map(|p| p.id);
            let element = ws.selected_element.filter(|&id| ws.doc.model().contains(id));
            (element, presentation, ws.doc.revision())
        };
        // The selection moved away while a field was being edited: save that edit first.
        let (before, ..) = selected(self, cx);
        let pending: Vec<(Field, Id)> = self
            .editing
            .iter()
            .filter(|(_, id)| before != Some(**id))
            .map(|(f, id)| (*f, *id))
            .collect();
        for (field, id) in pending {
            self.editing.remove(&field);
            self.commit(field, id, cx);
        }
        // Those commits may have deleted the selection, e.g. a «bind» dependency.
        let (selected, presentation, revision) = selected(self, cx);
        let Some(id) = selected else {
            self.shown = None;
            cx.notify();
            return;
        };
        let locale = rust_i18n::locale().to_string();
        let key = (id, presentation, revision, locale.clone());
        if self.shown.as_ref() == Some(&key) {
            return;
        }
        let locale_changed = self.shown.as_ref().is_none_or(|(.., l)| *l != locale);
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let data = model.get(id).expect("checked above").data().clone();
        let style = selected_presentation(ws).map(|p| p.style.clone()).unwrap_or_default();
        let declaration = member_text(model, id);
        // An ER participation's cardinality as the active diagram reads it.
        let cardinality = ws.shown_cardinality(id).map_or_else(String::new, |c| {
            if ws.reads_look_here() { c.parenthesized() } else { c.to_string() }
        });
        // A template parameter's value type, bounds and default as text.
        let parameter_texts = match &data.kind {
            ElementKind::TemplateParameter(p) => {
                let text = |t: &Option<TypeRef>| member::type_label(model, t).unwrap_or_default();
                let bounds: Vec<String> =
                    p.bounds.iter().map(|b| member::type_text(model, b)).collect();
                [text(&p.type_ref), bounds.join(", "), text(&p.default)]
            }
            ElementKind::ActivityNode(p) => {
                let text = member::type_label(model, &p.type_ref).unwrap_or_default();
                [text, String::new(), String::new()]
            }
            // An attribute's type, multiplicity and initial value; an operation's return type.
            ElementKind::Attribute(p) => [
                member::type_label(model, &p.type_ref).unwrap_or_default(),
                p.multiplicity.map(|m| m.to_string()).unwrap_or_default(),
                p.default_value.clone(),
            ],
            ElementKind::Operation(p) => {
                let text = member::type_label(model, &p.return_type).unwrap_or_default();
                [text, String::new(), String::new()]
            }
            _ => Default::default(),
        };
        let binding_text = match &data.kind {
            ElementKind::Class(p) => {
                p.binding.as_ref().map(|b| member::type_text(model, b)).unwrap_or_default()
            }
            _ => String::new(),
        };
        // What a message, lifeline or instance may refer to.
        let references = match &data.kind {
            ElementKind::Message(_) => operations_for(model, id),
            ElementKind::Lifeline(_) | ElementKind::Instance(_) => classifiers(model),
            ElementKind::InteractionUse(_) => {
                behaviors(model, |k| matches!(k, ElementKind::Interaction))
            }
            ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::CallBehavior => {
                behaviors(model, |k| matches!(k, ElementKind::Activity))
            }
            ElementKind::State(p) if p.submachine.is_some() => submachines(model, id),
            ElementKind::Link(p) => associations_for(model, p),
            _ => Vec::new(),
        };
        let slots = match &data.kind {
            ElementKind::Instance(_) => object::slots(model, id)
                .into_iter()
                .map(|(attribute, value)| {
                    let name = model.get(attribute).map(|a| a.name()).unwrap_or_default();
                    format!("{name} = {value}")
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        };
        let reference_labels = reference_items(model, &references);
        // The same element, changed elsewhere (undo, a background import) while something
        // typed is not written back yet: the field being typed in keeps its text and stays
        // pending, the others take the new values. After Enter nothing is pending, so the field
        // shows the text as written back.
        let same = self.shown_id() == Some(id);
        let typing = same && !self.editing.is_empty();
        self.shown = Some(key);
        if !same {
            // The fields are overwritten below, so nothing typed into them is pending any more.
            self.editing.clear();
        }

        let set = |state: &Entity<InputState>,
                   value: String,
                   window: &mut Window,
                   cx: &mut Context<Self>| {
            if typing && state.focus_handle(cx).is_focused(window) {
                return;
            }
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
        self.slots.update(cx, |s, cx| s.set_value(slots, window, cx));
        let constraints = data.constraints.join("\n");
        self.constraints.update(cx, |s, cx| s.set_value(constraints, window, cx));
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
        let d = DIRECTIONS.iter().position(|&d| d == style.parameter_directions);
        choose(&self.directions, d, locale_changed.then(direction_items), window, cx);
        // Only a color that differs: writing back the one just picked would fight a slider drag.
        for (state, color) in
            [(&self.font_color, style.font_color), (&self.line_color, style.stroke)]
        {
            state.update(cx, |s, cx| {
                if s.value().map(model_color) != color {
                    match color {
                        Some(c) => s.set_value(crate::canvas::hsla(c), window, cx),
                        None => s.clear_value(window, cx),
                    }
                }
            });
        }
        match &data.kind {
            ElementKind::Message(p) => {
                set(&self.arguments, p.arguments.clone(), window, cx);
                set(&self.return_value, p.return_value.clone(), window, cx);
                set(&self.guard, p.guard.clone(), window, cx);
                let s = SORTS.iter().position(|&s| s == p.sort);
                choose(&self.sort, s, locale_changed.then(sort_items), window, cx);
                let ix = references.iter().position(|&o| o == p.operation);
                choose(&self.operation, ix, Some(reference_labels), window, cx);
                self.operation_ids = references;
            }
            ElementKind::InteractionUse(p) => {
                set(&self.arguments, p.arguments.clone(), window, cx);
                let ix = references.iter().position(|r| *r == p.refers_to);
                choose(&self.represents, ix, Some(reference_labels), window, cx);
                self.represents_ids = references;
            }
            ElementKind::Lifeline(LifelineProps { represents: classifier })
            | ElementKind::Instance(InstanceProps { classifier, .. }) => {
                let ix = references.iter().position(|c| c == classifier);
                choose(&self.represents, ix, Some(reference_labels), window, cx);
                self.represents_ids = references;
            }
            ElementKind::Link(p) => {
                let ix = references.iter().position(|a| *a == p.association);
                choose(&self.association, ix, Some(reference_labels), window, cx);
                self.association_ids = references;
                for (i, end) in p.ends.iter().enumerate() {
                    set(&self.ends[i].role, end.role.clone(), window, cx);
                    let n = NAVIGABILITIES.iter().position(|&x| x == end.navigability);
                    let items = locale_changed.then(navigability_items);
                    choose(&self.ends[i].navigability, n, items, window, cx);
                    let a = AGGREGATIONS.iter().position(|&x| x == end.aggregation);
                    let items = locale_changed.then(aggregation_items);
                    choose(&self.ends[i].aggregation, a, items, window, cx);
                }
            }
            ElementKind::CombinedFragment(p) => {
                let ix = OPERATORS.iter().position(|&o| o == p.operator);
                choose(&self.operator, ix, None, window, cx);
            }
            ElementKind::InteractionOperand(p) => set(&self.guard, p.guard.clone(), window, cx),
            ElementKind::ActivityEdge(p) => set(&self.guard, p.guard.clone(), window, cx),
            ElementKind::Transition(p) => {
                set(&self.trigger, p.trigger.clone(), window, cx);
                set(&self.guard, p.guard.clone(), window, cx);
                set(&self.effect, p.effect.clone(), window, cx);
            }
            ElementKind::InternalTransition(p) => {
                set(&self.trigger, p.trigger.clone(), window, cx);
                set(&self.guard, p.guard.clone(), window, cx);
                set(&self.effect, p.effect.clone(), window, cx);
            }
            ElementKind::State(p) => {
                set(&self.entry, p.entry.clone(), window, cx);
                set(&self.do_activity, p.do_activity.clone(), window, cx);
                set(&self.exit, p.exit.clone(), window, cx);
                if p.submachine.is_some() {
                    let ix = references.iter().position(|r| *r == p.submachine);
                    choose(&self.represents, ix, Some(reference_labels), window, cx);
                    self.represents_ids = references;
                }
            }
            ElementKind::ActivityNode(p) => {
                let [object_type, _, _] = parameter_texts;
                set(&self.value_type, object_type, window, cx);
                set(&self.in_state, p.in_state.clone(), window, cx);
                set(&self.upper_bound, p.upper_bound.clone(), window, cx);
                let ordering =
                    p.ordering.and_then(|o| ObjectOrdering::ALL.iter().position(|&x| x == o));
                choose(&self.ordering, Some(ordering.map_or(0, |i| i + 1)), None, window, cx);
                let effect = p.effect.and_then(|o| ObjectEffect::ALL.iter().position(|&x| x == o));
                choose(&self.object_effect, Some(effect.map_or(0, |i| i + 1)), None, window, cx);
                if p.node == ActivityNodeKind::CallBehavior {
                    let ix = references.iter().position(|r| *r == p.behavior);
                    choose(&self.represents, ix, Some(reference_labels), window, cx);
                    self.represents_ids = references;
                }
            }
            ElementKind::Class(_) => set(&self.binding, binding_text, window, cx),
            ElementKind::Attribute(_) => {
                let [type_text, multiplicity, default] = parameter_texts;
                set(&self.value_type, type_text, window, cx);
                set(&self.member_multiplicity, multiplicity, window, cx);
                set(&self.default, default, window, cx);
            }
            ElementKind::Operation(_) => {
                let [return_type, _, _] = parameter_texts;
                set(&self.value_type, return_type, window, cx);
            }
            ElementKind::TemplateParameter(p) => {
                let [value_type, bounds, default] = parameter_texts;
                set(&self.value_type, value_type, window, cx);
                set(&self.bounds, bounds, window, cx);
                set(&self.default, default, window, cx);
                let k = PARAMETER_KINDS.iter().position(|&k| k == p.parameter);
                choose(
                    &self.parameter_kind,
                    k,
                    locale_changed.then(parameter_kind_items),
                    window,
                    cx,
                );
            }
            ElementKind::ErEntity(p) => {
                set(&self.physical_name, p.physical_name.clone(), window, cx);
                let checks = p.checks.join("\n");
                let indexes: Vec<String> = p.indexes.iter().map(index_text).collect();
                let indexes = indexes.join("\n");
                self.er_checks.update(cx, |s, cx| s.set_value(checks, window, cx));
                self.er_indexes.update(cx, |s, cx| s.set_value(indexes, window, cx));
            }
            ElementKind::ErDomain(p) => {
                set(&self.datatype, p.datatype.clone(), window, cx);
                set(&self.length_precision, p.length_precision.clone(), window, cx);
                set(&self.default, p.default_value.clone(), window, cx);
                set(&self.er_check, p.check.clone(), window, cx);
                set(&self.er_values, p.values.join(", "), window, cx);
            }
            ElementKind::ErAttribute(p) => {
                set(&self.er_check, p.check.clone(), window, cx);
                set(&self.physical_name, p.physical_name.clone(), window, cx);
                set(&self.datatype, p.datatype.clone(), window, cx);
                set(&self.length_precision, p.length_precision.clone(), window, cx);
                set(&self.default, p.default_value.clone(), window, cx);
                let k = ER_KEYS.iter().position(|&k| k == p.key);
                choose(&self.er_key, k, locale_changed.then(er_key_items), window, cx);
            }
            ElementKind::Diagram(p) if p.diagram == DiagramKind::EntityRelationship => {
                let n = ER_NOTATIONS.iter().position(|&n| n == p.notation);
                choose(&self.er_notation, n, locale_changed.then(er_notation_items), window, cx);
                let d = ER_DISPLAYS.iter().position(|&d| d == p.er_display);
                choose(&self.er_display, d, locale_changed.then(er_display_items), window, cx);
            }
            ElementKind::ErIsA(p) => {
                let d = DISJOINT.iter().position(|&d| d == p.disjoint);
                choose(&self.disjoint, d, locale_changed.then(disjoint_items), window, cx);
            }
            ElementKind::ErRelationship(p) => {
                for (state, action) in
                    [(&self.on_delete, p.on_delete), (&self.on_update, p.on_update)]
                {
                    let a = ER_ACTIONS.iter().position(|&a| a == action);
                    choose(state, a, locale_changed.then(er_action_items), window, cx);
                }
            }
            ElementKind::ErParticipation(p) => {
                set(&self.cardinality, cardinality.clone(), window, cx);
                set(&self.ends[0].role, p.role.clone(), window, cx);
            }
            _ => {}
        }
        if let ElementKind::Association(p) = &data.kind {
            for (i, end) in p.ends.iter().enumerate() {
                set(&self.ends[i].role, end.role.clone(), window, cx);
                let m = end.multiplicity.map(|m| m.to_string()).unwrap_or_default();
                set(&self.ends[i].multiplicity, m, window, cx);
                set(&self.ends[i].constraints, end.constraints.join(", "), window, cx);
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

/// Elements whose own constraints the panel offers: what class diagrams show constraints of
/// (members and lines) and the classifiers and packages Astah keeps them for.
fn has_constraints(kind: &ElementKind) -> bool {
    kind.is_classifier()
        || matches!(
            kind,
            ElementKind::Package
                | ElementKind::Attribute(_)
                | ElementKind::Operation(_)
                | ElementKind::Association(_)
                | ElementKind::Generalization(_)
                | ElementKind::Realization(_)
                | ElementKind::Dependency(_)
                | ElementKind::Include(_)
                | ElementKind::Extend(_)
        )
}

fn is_member(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Attribute(_) | ElementKind::Operation(_) | ElementKind::EnumerationLiteral
    )
}

/// A link-like button that shows `element` in the panel, for the parts of an element.
fn select_button(
    id: impl Into<ElementId>,
    label: String,
    workspace: Entity<Workspace>,
    element: Id,
) -> Button {
    Button::new(id).ghost().small().label(label).on_click(move |_, _, cx| {
        workspace.update(cx, |ws, cx| {
            ws.selected_element = Some(element);
            cx.notify();
        });
    })
}

/// A button that deletes `element`, for parts that have no place on the canvas.
fn delete_button(
    id: &'static str,
    label: String,
    workspace: Entity<Workspace>,
    element: Id,
) -> Button {
    Button::new(id).small().label(label).on_click(move |_, _, cx| {
        workspace.update(cx, |ws, cx| {
            ws.delete_element(element);
            cx.notify();
        });
    })
}

/// A checkbox that flips one boolean of the element via `set`.
/// An index as the panel lists it: `UNIQUE name (a, b)`.
fn index_text(i: &ErIndex) -> String {
    let unique = if i.unique { "UNIQUE " } else { "" };
    let name = if i.name.is_empty() { String::new() } else { format!("{} ", i.name) };
    format!("{unique}{name}({})", i.columns.join(", "))
}

/// A line of the indexes field: `[UNIQUE] [name] (a, b)`; without columns it is none.
fn parse_index(line: &str) -> Option<ErIndex> {
    let (head, rest) = line.split_once('(')?;
    let columns: Vec<String> = rest
        .trim_end()
        .trim_end_matches(')')
        .split(',')
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect();
    if columns.is_empty() {
        return None;
    }
    let mut head = head.trim();
    let unique = head.len() >= 6 && head[..6].eq_ignore_ascii_case("unique");
    if unique {
        head = head[6..].trim();
    }
    Some(ErIndex { name: head.to_owned(), columns, unique })
}

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
        let body = v_flex().gap_3().p_3();
        if self.structogram.read(cx).is_active(cx) {
            return body.child(self.structogram.clone());
        }
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        // A free text or line has no element; its own options are shown instead.
        let free = match &ws.selection[..] {
            [only] => ws.active_diagram.and_then(|d| model.view(d)?.get(*only)),
            _ => None,
        };
        match free.map(|p| (p.id, &p.shape)) {
            Some((_, Shape::Text { text, .. })) => {
                return body
                    .child(field(
                        t!("prop.type").into(),
                        div().text_sm().child(t!("kind.text").to_string()).into_any_element(),
                    ))
                    .child(div().text_sm().whitespace_normal().child(text.clone()));
            }
            Some((presentation, &Shape::Line { dashed, start_arrow, end_arrow, .. })) => {
                let workspace = self.workspace.clone();
                let flag = move |id: &'static str, label: String, checked: bool, which: usize| {
                    let workspace = workspace.clone();
                    Checkbox::new(id).label(label).checked(checked).on_click(
                        move |checked: &bool, _, cx| {
                            let checked = *checked;
                            workspace.update(cx, |ws, cx| {
                                ws.update_free_line(presentation, |dashed, start, end| {
                                    *[dashed, start, end][which] = checked;
                                });
                                cx.notify();
                            });
                        },
                    )
                };
                return body
                    .child(field(
                        t!("prop.type").into(),
                        div().text_sm().child(t!("tool.line").to_string()).into_any_element(),
                    ))
                    .child(flag("line-dashed", t!("prop.dashed").into(), dashed, 0))
                    .child(flag("line-start-arrow", t!("prop.start_arrow").into(), start_arrow, 1))
                    .child(flag("line-end-arrow", t!("prop.end_arrow").into(), end_arrow, 2));
            }
            _ => {}
        }
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
                | ElementKind::InternalTransition(_)
                | ElementKind::Termination(_)
                // Its title is the text of its root, edited in the structogram itself.
                | ElementKind::Structogram(_)
        );
        match &kind {
            _ if unnamed => {}
            k if is_member(k) => {
                body = body.child(field(
                    t!("prop.declaration").into(),
                    Input::new(&self.declaration).small().into_any_element(),
                ));
            }
            // Visibility matters where code is made of it: packages and classifiers. Objects,
            // actors, use cases, states and lines have it in the metamodel only.
            ElementKind::Package
            | ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
            | ElementKind::Component(_) => {
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
            // Their name is the text they show.
            ElementKind::StateInvariant(_) => {
                body = body.child(field(
                    t!("prop.condition").into(),
                    Input::new(&self.name).small().into_any_element(),
                ));
            }
            ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_) => {
                body = body.child(field(
                    t!("prop.constraint").into(),
                    Input::new(&self.name).small().into_any_element(),
                ));
            }
            _ => {
                body = body.child(field(
                    t!("prop.name").into(),
                    Input::new(&self.name).small().into_any_element(),
                ));
            }
        }
        if !unnamed {
            body = body.child(field(
                t!("prop.stereotypes").into(),
                Input::new(&self.stereotypes).small().into_any_element(),
            ));
        }

        body = self.kind_options(body, &kind, id, model, cx);

        // The lifelines a frame covers; none ticked means "as its messages need".
        if let Some(covered) = covered_of(&kind) {
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
            if covered.is_empty() {
                body = body
                    .child(div().text_xs().text_color(muted).child(t!("prop.covered_automatic")));
            }
            for (ix, (lifeline, name)) in lifelines.into_iter().enumerate() {
                let workspace = workspace.clone();
                body = body.child(
                    Checkbox::new(("covered", ix))
                        .label(name)
                        .checked(covered.contains(&lifeline))
                        .on_click(move |checked: &bool, _, cx| {
                            let checked = *checked;
                            workspace.update(cx, |ws, cx| {
                                ws.update_element(id, |d| {
                                    if let Some(covered) = covered_of_mut(&mut d.kind) {
                                        covered.retain(|&l| l != lifeline);
                                        if checked {
                                            covered.push(lifeline);
                                        }
                                    }
                                });
                                cx.notify();
                            });
                        }),
                );
            }
        }

        if has_constraints(&kind) {
            body = body.child(field(
                t!("prop.element_constraints").into(),
                Textarea::new(&self.constraints).small().into_any_element(),
            ));
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
        let presentation = selected_presentation(ws).filter(|p| !is_fragment(p)).cloned();
        if let Some(p) = presentation.as_ref().filter(|p| matches!(p.shape, Shape::Edge { .. })) {
            body = body
                .child(heading(t!("prop.display").to_string()))
                .child(field(
                    t!("prop.font_color").into(),
                    self.color_select(&self.font_color, p.style.font_color.is_some(), false, cx),
                ))
                .child(field(
                    t!("prop.line_color").into(),
                    self.color_select(&self.line_color, p.style.stroke.is_some(), true, cx),
                ));
        }
        if let Some(p) = presentation.filter(|p| !p.is_edge()) {
            body = self.node_options(body, p, model, cx);
        }
        body
    }
}

// The render function is split up: built in one piece, its frame in a debug build outgrew the
// main thread's 1 MB stack on Windows.
impl PropertiesPanel {
    /// The fields of the element's kind.
    fn kind_options(
        &self,
        body: Div,
        kind: &ElementKind,
        id: Id,
        model: &Model,
        cx: &Context<Self>,
    ) -> Div {
        match kind {
            ElementKind::Diagram(_)
            | ElementKind::Class(_)
            | ElementKind::TemplateParameter(_)
            | ElementKind::Attribute(_)
            | ElementKind::Operation(_)
            | ElementKind::Extend(_)
            | ElementKind::Association(_) => self.structure_options(body, kind, id, model, cx),
            ElementKind::ErEntity(_)
            | ElementKind::ErAttribute(_)
            | ElementKind::ErRelationship(_)
            | ElementKind::ErIsA(_)
            | ElementKind::ErDomain(_)
            | ElementKind::ErParticipation(_) => self.er_options(body, kind, id, cx),
            _ => self.behavior_options(body, kind, id, model, cx),
        }
    }

    /// Fields of classifiers, members and their lines.
    fn structure_options(
        &self,
        mut body: Div,
        kind: &ElementKind,
        id: Id,
        model: &Model,
        cx: &Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let field = |label: SharedString, content: AnyElement| {
            v_flex().gap_1().child(div().text_xs().text_color(muted).child(label)).child(content)
        };
        let heading =
            |label: String| div().pt_2().text_xs().font_semibold().text_color(muted).child(label);
        let workspace = self.workspace.clone();
        match kind {
            ElementKind::Diagram(p) if p.diagram == DiagramKind::EntityRelationship => {
                body = body.child(field(
                    t!("prop.er_notation").into(),
                    Select::new(&self.er_notation).small().into_any_element(),
                ));
                if p.notation.lists_attributes() {
                    body = body
                        .child(field(
                            t!("prop.er_display").into(),
                            Select::new(&self.er_display).small().into_any_element(),
                        ))
                        .child(flag(
                            "er-physical-names",
                            t!("prop.er_physical_names").into(),
                            p.physical_names,
                            workspace.clone(),
                            id,
                            |d, v| {
                                if let ElementKind::Diagram(p) = &mut d.kind {
                                    p.physical_names = v;
                                }
                            },
                        ));
                } else {
                    body = body.child(flag(
                        "er-look-here",
                        t!("prop.er_look_here").into(),
                        p.look_here,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Diagram(p) = &mut d.kind {
                                p.look_here = v;
                            }
                        },
                    ));
                }
            }
            ElementKind::Diagram(p) if p.diagram == DiagramKind::Sequence => {
                body = body
                    .child(flag(
                        "message-numbers",
                        t!("prop.message_numbers").into(),
                        !p.hide_message_numbers,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Diagram(p) = &mut d.kind {
                                p.hide_message_numbers = !v;
                            }
                        },
                    ))
                    .child(flag(
                        "flat-message-numbers",
                        t!("prop.flat_message_numbers").into(),
                        p.flat_message_numbers,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Diagram(p) = &mut d.kind {
                                p.flat_message_numbers = v;
                            }
                        },
                    ))
                    .child(flag(
                        "return-values",
                        t!("prop.return_values").into(),
                        !p.hide_return_values,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Diagram(p) = &mut d.kind {
                                p.hide_return_values = !v;
                            }
                        },
                    ));
            }
            ElementKind::Diagram(p) if p.diagram == DiagramKind::StateMachine => {
                body = body.child(flag(
                    "simple-states",
                    t!("prop.simple_states").into(),
                    p.simple_states,
                    workspace.clone(),
                    id,
                    |d, v| {
                        if let ElementKind::Diagram(p) = &mut d.kind {
                            p.simple_states = v;
                        }
                    },
                ));
            }
            ElementKind::Class(p) => {
                body = body
                    .child(flag(
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
                    ))
                    .child(flag(
                        "leaf",
                        t!("prop.leaf").into(),
                        p.is_leaf,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Class(p) = &mut d.kind {
                                p.is_leaf = v;
                            }
                        },
                    ))
                    .child(field(
                        t!("prop.binding").into(),
                        Input::new(&self.binding).small().into_any_element(),
                    ));
            }
            ElementKind::TemplateParameter(p) => {
                body = body.child(field(
                    t!("prop.parameter_kind").into(),
                    Select::new(&self.parameter_kind).small().into_any_element(),
                ));
                if p.parameter == TemplateParameterKind::Value {
                    body = body.child(field(
                        t!("prop.value_type").into(),
                        Input::new(&self.value_type).small().into_any_element(),
                    ));
                }
                body = body
                    .child(field(
                        t!("prop.type_bounds").into(),
                        Input::new(&self.bounds).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.default").into(),
                        Input::new(&self.default).small().into_any_element(),
                    ))
                    .child(flag(
                        "variadic",
                        t!("prop.variadic").into(),
                        p.is_variadic,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::TemplateParameter(p) = &mut d.kind {
                                p.is_variadic = v;
                            }
                        },
                    ));
            }
            ElementKind::Attribute(p) => {
                body = body
                    .child(field(
                        t!("prop.name").into(),
                        Input::new(&self.name).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.object_type").into(),
                        Input::new(&self.value_type).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.multiplicity").into(),
                        Input::new(&self.member_multiplicity).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.initial_value").into(),
                        Input::new(&self.default).small().into_any_element(),
                    ))
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
                    .child(field(
                        t!("prop.name").into(),
                        Input::new(&self.name).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.return_type").into(),
                        Input::new(&self.value_type).small().into_any_element(),
                    ))
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
                        "leaf",
                        t!("prop.leaf").into(),
                        p.is_leaf,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::Operation(p) = &mut d.kind {
                                p.is_leaf = v;
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
                            t!("prop.constraints").into(),
                            Input::new(&fields.constraints).small().into_any_element(),
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
            _ => {}
        }
        body
    }

    /// Fields of the elements of ER diagrams.
    fn er_options(&self, mut body: Div, kind: &ElementKind, id: Id, cx: &Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let field = |label: SharedString, content: AnyElement| {
            v_flex().gap_1().child(div().text_xs().text_color(muted).child(label)).child(content)
        };
        let heading =
            |label: String| div().pt_2().text_xs().font_semibold().text_color(muted).child(label);
        let input = |label: &str, state: &Entity<InputState>| {
            field(t!(label).into(), Input::new(state).small().into_any_element())
        };
        let workspace = self.workspace.clone();
        match kind {
            ElementKind::ErEntity(p) => {
                body = body
                    .child(flag(
                        "er-weak",
                        t!("prop.er_weak").into(),
                        p.weak,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErEntity(p) = &mut d.kind {
                                p.weak = v;
                            }
                        },
                    ))
                    .child({
                        // Refused while entities take part in it.
                        let workspace = workspace.clone();
                        Checkbox::new("er-associative")
                            .label(t!("prop.er_associative").to_string())
                            .checked(p.associative)
                            .on_click(move |checked: &bool, _, cx| {
                                let checked = *checked;
                                workspace.update(cx, |ws, cx| {
                                    ws.set_er_associative(id, checked);
                                    cx.notify();
                                });
                            })
                    })
                    .child(heading(t!("prop.er_database").to_string()))
                    .child(input("prop.er_physical_name", &self.physical_name))
                    .child(field(
                        t!("prop.er_checks").into(),
                        Textarea::new(&self.er_checks).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.er_indexes").into(),
                        Textarea::new(&self.er_indexes).small().into_any_element(),
                    ));
            }
            ElementKind::ErDomain(p) => {
                body = body
                    .child(input("prop.er_datatype", &self.datatype))
                    .child(input("prop.er_length_precision", &self.length_precision))
                    .child(input("prop.er_values", &self.er_values))
                    .child(input("prop.er_default_value", &self.default))
                    .child(flag(
                        "er-domain-not-null",
                        t!("prop.er_not_null").into(),
                        p.not_null,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErDomain(p) = &mut d.kind {
                                p.not_null = v;
                            }
                        },
                    ))
                    .child(input("prop.er_check", &self.er_check));
            }
            ElementKind::ErAttribute(p) => {
                body = body
                    .child(field(
                        t!("prop.er_key").into(),
                        Select::new(&self.er_key).small().into_any_element(),
                    ))
                    .child(flag(
                        "er-derived",
                        t!("prop.er_derived").into(),
                        p.derived,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.derived = v;
                            }
                        },
                    ))
                    .child(flag(
                        "er-multivalued",
                        t!("prop.er_multivalued").into(),
                        p.multivalued,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.multivalued = v;
                            }
                        },
                    ))
                    .child(heading(t!("prop.er_database").to_string()))
                    .child(input("prop.er_physical_name", &self.physical_name))
                    .child(input("prop.er_datatype", &self.datatype))
                    .child(input("prop.er_length_precision", &self.length_precision))
                    .child(input("prop.er_default_value", &self.default))
                    .child(flag(
                        "er-not-null",
                        t!("prop.er_not_null").into(),
                        p.not_null,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.not_null = v;
                            }
                        },
                    ))
                    .child(flag(
                        "er-unique",
                        t!("prop.er_unique").into(),
                        p.unique,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.unique = v;
                            }
                        },
                    ))
                    .child(flag(
                        "er-auto-increment",
                        t!("prop.er_auto_increment").into(),
                        p.auto_increment,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.auto_increment = v;
                            }
                        },
                    ))
                    .child(input("prop.er_check", &self.er_check));
                // A derived column's expression is its default: stored or computed when read.
                if p.derived {
                    body = body.child(flag(
                        "er-virtual",
                        t!("prop.er_virtual").into(),
                        p.virtual_column,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErAttribute(p) = &mut d.kind {
                                p.virtual_column = v;
                            }
                        },
                    ));
                }
                // A foreign key column: of which relationship, by its name.
                let model = self.workspace.read(cx).doc.model();
                if let Some(by) = p.foreign_key.and_then(|r| model.get(r)) {
                    let name = match by.name() {
                        "" => t!("prop.er_unnamed").to_string(),
                        name => name.to_owned(),
                    };
                    body = body.child(field(
                        t!("prop.er_foreign_key").into(),
                        div().text_sm().child(name).into_any_element(),
                    ));
                }
            }
            ElementKind::ErRelationship(p) => {
                // Its foreign key columns move into or out of the child's key with it.
                let identifying = Checkbox::new("er-identifying")
                    .label(t!("prop.er_identifying").to_string())
                    .checked(p.identifying)
                    .on_click(move |checked: &bool, _, cx| {
                        let checked = *checked;
                        workspace.update(cx, |ws, cx| {
                            ws.set_er_identifying(id, checked);
                            cx.notify();
                        });
                    });
                body = body
                    .child(identifying)
                    .child(heading(t!("prop.er_database").to_string()))
                    .child(field(
                        t!("prop.er_on_delete").into(),
                        Select::new(&self.on_delete).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.er_on_update").into(),
                        Select::new(&self.on_update).small().into_any_element(),
                    ));
            }
            ElementKind::ErIsA(p) => {
                body = body
                    .child(field(
                        t!("prop.er_disjoint").into(),
                        Select::new(&self.disjoint).small().into_any_element(),
                    ))
                    .child(flag(
                        "er-total-specialization",
                        t!("prop.er_total_specialization").into(),
                        p.total,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErIsA(p) = &mut d.kind {
                                p.total = v;
                            }
                        },
                    ));
            }
            ElementKind::ErParticipation(p) => {
                body = body
                    .child(input("prop.er_cardinality", &self.cardinality))
                    .child(input("prop.role", &self.ends[0].role))
                    .child(flag(
                        "er-total-participation",
                        t!("prop.er_total_participation").into(),
                        p.total,
                        workspace.clone(),
                        id,
                        |d, v| {
                            if let ElementKind::ErParticipation(p) = &mut d.kind {
                                p.total = v;
                            }
                        },
                    ));
            }
            _ => {}
        }
        body
    }

    /// Fields of interactions, activities and state machines.
    fn behavior_options(
        &self,
        mut body: Div,
        kind: &ElementKind,
        id: Id,
        model: &Model,
        cx: &Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let field = |label: SharedString, content: AnyElement| {
            v_flex().gap_1().child(div().text_xs().text_color(muted).child(label)).child(content)
        };
        let heading =
            |label: String| div().pt_2().text_xs().font_semibold().text_color(muted).child(label);
        let workspace = self.workspace.clone();
        match kind {
            ElementKind::Message(_) => {
                body = body
                    .child(field(
                        t!("prop.message_sort").into(),
                        Select::new(&self.sort).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.guard").into(),
                        Input::new(&self.guard).small().into_any_element(),
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
            ElementKind::Instance(p) => {
                body = body.child(field(
                    t!("prop.classifier").into(),
                    Select::new(&self.represents).small().into_any_element(),
                ));
                // Objects give their classifier's attributes values.
                if p.instance == InstanceKind::Object && p.classifier.is_some() {
                    body = body.child(field(
                        t!("prop.slots").into(),
                        Textarea::new(&self.slots).small().into_any_element(),
                    ));
                }
            }
            ElementKind::Link(p) => {
                body = body.child(field(
                    t!("prop.instance_of_association").into(),
                    Select::new(&self.association).small().into_any_element(),
                ));
                for (i, end) in p.ends.iter().enumerate() {
                    let object =
                        model.get(end.instance).map(labels::display_name).unwrap_or_default();
                    let fields = &self.ends[i];
                    body = body
                        .child(heading(format!("{} {} – {object}", t!("prop.end"), ["A", "B"][i])))
                        .child(field(
                            t!("prop.role").into(),
                            Input::new(&fields.role).small().into_any_element(),
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
            ElementKind::InteractionUse(_) => {
                body = body
                    .child(field(
                        t!("prop.refers_to").into(),
                        Select::new(&self.represents).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.arguments").into(),
                        Input::new(&self.arguments).small().into_any_element(),
                    ));
            }
            ElementKind::CombinedFragment(_) => {
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
            ElementKind::ActivityNode(p) if p.node.is_object() => {
                body = body
                    .child(field(
                        t!("prop.object_type").into(),
                        Input::new(&self.value_type).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.in_state").into(),
                        Input::new(&self.in_state).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.ordering").into(),
                        Select::new(&self.ordering).small().into_any_element(),
                    ))
                    .child(field(
                        t!("prop.upper_bound").into(),
                        Input::new(&self.upper_bound).small().into_any_element(),
                    ))
                    .child(flag(
                        "object-stream",
                        t!("prop.stream").into(),
                        p.is_stream,
                        workspace.clone(),
                        id,
                        |d, on| {
                            if let ElementKind::ActivityNode(p) = &mut d.kind {
                                p.is_stream = on;
                            }
                        },
                    ))
                    .child(field(
                        t!("prop.object_effect").into(),
                        Select::new(&self.object_effect).small().into_any_element(),
                    ))
                    .child(flag(
                        "object-control",
                        t!("prop.object_control").into(),
                        p.is_control,
                        workspace.clone(),
                        id,
                        |d, on| {
                            if let ElementKind::ActivityNode(p) = &mut d.kind {
                                p.is_control = on;
                            }
                        },
                    ))
                    .child(flag(
                        "object-exception",
                        t!("prop.object_exception").into(),
                        p.is_exception,
                        workspace.clone(),
                        id,
                        |d, on| {
                            if let ElementKind::ActivityNode(p) = &mut d.kind {
                                p.is_exception = on;
                            }
                        },
                    ));
            }
            ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::CallBehavior => {
                body = body.child(field(
                    t!("prop.calls").into(),
                    Select::new(&self.represents).small().into_any_element(),
                ));
            }
            ElementKind::ActivityEdge(_) => {
                body = body.child(field(
                    t!("prop.guard").into(),
                    Input::new(&self.guard).small().into_any_element(),
                ));
            }
            ElementKind::Transition(_) | ElementKind::InternalTransition(_) => {
                for (label, state) in [
                    ("prop.trigger", &self.trigger),
                    ("prop.guard", &self.guard),
                    ("prop.effect", &self.effect),
                ] {
                    body = body.child(field(
                        t!(label).into(),
                        Input::new(state).small().into_any_element(),
                    ));
                }
                if matches!(kind, ElementKind::InternalTransition(_)) {
                    body = body.child(delete_button(
                        "delete-internal-transition",
                        t!("prop.delete_internal_transition").into(),
                        workspace.clone(),
                        id,
                    ));
                }
            }
            ElementKind::Region => {
                body = body.child(delete_button(
                    "delete-region",
                    t!("prop.delete_region").into(),
                    workspace.clone(),
                    id,
                ));
            }
            ElementKind::State(p) => {
                if p.submachine.is_some() {
                    body = body.child(field(
                        t!("prop.submachine").into(),
                        Select::new(&self.represents).small().into_any_element(),
                    ));
                }
                for (label, state) in [
                    ("prop.entry", &self.entry),
                    ("prop.do_activity", &self.do_activity),
                    ("prop.exit", &self.exit),
                ] {
                    body = body.child(field(
                        t!(label).into(),
                        Input::new(state).small().into_any_element(),
                    ));
                }
                // Internal transitions and regions, each selectable, and buttons to add more.
                let children = |pick: fn(&ElementKind) -> bool| -> Vec<(Id, String)> {
                    model
                        .children(id)
                        .iter()
                        .filter_map(|&c| model.get(c))
                        .filter(|c| pick(c.kind()))
                        .map(|c| (c.id(), labels::tree_name(model, c)))
                        .collect()
                };
                let internal = children(|k| matches!(k, ElementKind::InternalTransition(_)));
                let regions = children(|k| matches!(k, ElementKind::Region));
                body = body.child(heading(t!("prop.internal_transitions").to_string()));
                for (ix, (transition, label)) in internal.into_iter().enumerate() {
                    body = body.child(select_button(
                        ("internal-transition", ix),
                        label,
                        workspace.clone(),
                        transition,
                    ));
                }
                let add = workspace.clone();
                body = body.child(
                    Button::new("add-internal-transition")
                        .small()
                        .label(t!("prop.add_internal_transition"))
                        .on_click(move |_, _, cx| {
                            add.update(cx, |ws, cx| {
                                ws.add_internal_transition(id);
                                cx.notify();
                            });
                        }),
                );
                body = body.child(heading(t!("prop.regions").to_string()));
                let has_regions = !regions.is_empty();
                for (ix, (region, label)) in regions.into_iter().enumerate() {
                    let label = if model.get(region).is_some_and(|r| r.name().is_empty()) {
                        format!("{} {}", t!("kind.region"), ix + 1)
                    } else {
                        label
                    };
                    body =
                        body.child(select_button(("region", ix), label, workspace.clone(), region));
                }
                // The first division picks the direction; later regions follow it.
                let directions: &[(bool, &str)] = if has_regions {
                    &[(false, "prop.add_region")]
                } else {
                    &[(false, "prop.add_regions_stacked"), (true, "prop.add_regions_side_by_side")]
                };
                for &(vertical, key) in directions {
                    let add = workspace.clone();
                    body = body.child(
                        Button::new(("add-region", usize::from(vertical)))
                            .small()
                            .label(t!(key))
                            .on_click(move |_, _, cx| {
                                add.update(cx, |ws, cx| {
                                    ws.add_region(id, vertical);
                                    cx.notify();
                                });
                            }),
                    );
                }
            }
            _ => {}
        }
        body
    }

    /// The display options of the one selected node.
    fn node_options(
        &self,
        mut body: Div,
        p: Presentation,
        model: &Model,
        cx: &Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let field = |label: SharedString, content: AnyElement| {
            v_flex().gap_1().child(div().text_xs().text_color(muted).child(label)).child(content)
        };
        let heading =
            |label: String| div().pt_2().text_xs().font_semibold().text_color(muted).child(label);
        let workspace = self.workspace.clone();
        let shown = p.element.and_then(|e| model.get(e)).map(|e| e.kind().clone());
        body = body.child(heading(t!("prop.display").to_string()));
        if matches!(p.shape, Shape::Node { .. }) {
            body = body.child(field(
                t!("prop.font_color").into(),
                self.color_select(&self.font_color, p.style.font_color.is_some(), false, cx),
            ));
        }
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
            // What the box leaves out of its members, as Astah offers it per box.
            type Flag = (&'static str, &'static str, fn(&Style) -> bool, fn(&mut Style, bool));
            const MEMBERS: [Flag; 9] = [
                (
                    "hide-stereotypes",
                    "prop.hide_stereotypes",
                    |s| s.hide_stereotypes,
                    |s, v| s.hide_stereotypes = v,
                ),
                (
                    "hide-member-visibility",
                    "prop.hide_member_visibility",
                    |s| s.hide_member_visibility,
                    |s, v| s.hide_member_visibility = v,
                ),
                (
                    "hide-attribute-types",
                    "prop.hide_attribute_types",
                    |s| s.hide_attribute_types,
                    |s, v| s.hide_attribute_types = v,
                ),
                (
                    "hide-initial-values",
                    "prop.hide_initial_values",
                    |s| s.hide_initial_values,
                    |s, v| s.hide_initial_values = v,
                ),
                (
                    "hide-parameters",
                    "prop.hide_parameters",
                    |s| s.hide_parameters,
                    |s, v| s.hide_parameters = v,
                ),
                (
                    "hide-parameter-types",
                    "prop.hide_parameter_types",
                    |s| s.hide_parameter_types,
                    |s, v| s.hide_parameter_types = v,
                ),
                (
                    "hide-return-types",
                    "prop.hide_return_types",
                    |s| s.hide_return_types,
                    |s, v| s.hide_return_types = v,
                ),
                (
                    "hide-attribute-stereotypes",
                    "prop.hide_attribute_stereotypes",
                    |s| s.hide_attribute_stereotypes,
                    |s, v| s.hide_attribute_stereotypes = v,
                ),
                (
                    "hide-operation-stereotypes",
                    "prop.hide_operation_stereotypes",
                    |s| s.hide_operation_stereotypes,
                    |s, v| s.hide_operation_stereotypes = v,
                ),
            ];
            for (id, key, get, set) in MEMBERS {
                body = body.child(style_flag(
                    id,
                    t!(key).into(),
                    get(&p.style),
                    workspace.clone(),
                    p.id,
                    set,
                ));
            }
            body = body.child(field(
                t!("prop.parameter_directions").into(),
                Select::new(&self.directions).small().into_any_element(),
            ));
            // The members shown by visibility; the box leaves out those not ticked.
            type Hidden = fn(&mut Style) -> &mut Vec<Visibility>;
            let filters: [(&str, &str, &[Visibility], Hidden); 2] = [
                ("shown-attributes", "prop.shown_attributes", &p.style.hidden_attributes, |s| {
                    &mut s.hidden_attributes
                }),
                ("shown-operations", "prop.shown_operations", &p.style.hidden_operations, |s| {
                    &mut s.hidden_operations
                }),
            ];
            for (id, key, hidden, list) in filters {
                let boxes = VISIBILITIES.iter().map(|&v| {
                    let workspace = workspace.clone();
                    let presentation = p.id;
                    Checkbox::new(SharedString::from(format!("{id}-{}", v.symbol())))
                        .label(labels::visibility_label(v))
                        .checked(!hidden.contains(&v))
                        .on_click(move |checked: &bool, _, cx| {
                            let shown = *checked;
                            workspace.update(cx, |ws, cx| {
                                ws.update_style(presentation, |s| {
                                    let hidden = list(s);
                                    hidden.retain(|&h| h != v);
                                    if !shown {
                                        hidden.push(v);
                                        hidden.sort_by_key(|h| {
                                            VISIBILITIES.iter().position(|x| x == h)
                                        });
                                    }
                                });
                                cx.notify();
                            });
                        })
                });
                body = body.child(field(
                    t!(key).into(),
                    h_flex().flex_wrap().gap_x_3().gap_y_1().children(boxes).into_any_element(),
                ));
            }
        }
        if matches!(&shown, Some(ElementKind::Lifeline(_))) {
            body = body
                .child(style_flag(
                    "hide-instance-name",
                    t!("prop.hide_instance_name").into(),
                    p.style.hide_instance_name,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_instance_name = v,
                ))
                .child(style_flag(
                    "hide-instance-type",
                    t!("prop.hide_instance_type").into(),
                    p.style.hide_instance_type,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_instance_type = v,
                ));
        }
        if let Some(ElementKind::Instance(instance)) = &shown {
            body = body
                .child(style_flag(
                    "hide-instance-name",
                    t!("prop.hide_instance_name").into(),
                    p.style.hide_instance_name,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_instance_name = v,
                ))
                .child(style_flag(
                    "hide-instance-type",
                    t!("prop.hide_instance_type").into(),
                    p.style.hide_instance_type,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_instance_type = v,
                ));
            if instance.instance == umlcraft_model::InstanceKind::Object {
                body = body
                    .child(style_flag(
                        "hide-slots",
                        t!("prop.hide_slots").into(),
                        p.style.hide_attributes,
                        workspace.clone(),
                        p.id,
                        |s, v| s.hide_attributes = v,
                    ))
                    .child(style_flag(
                        "hide-empty-slots",
                        t!("prop.hide_empty_slots").into(),
                        p.style.hide_empty_slots,
                        workspace.clone(),
                        p.id,
                        |s, v| s.hide_empty_slots = v,
                    ));
            }
        }
        // A class whose first stereotype is «boundary», «control» or «entity».
        let robustness = p
            .element
            .and_then(|e| model.get(e))
            .is_some_and(|e| umlcraft_notation::scene::robustness_icon(e).is_some());
        if robustness {
            body = body.child(style_flag(
                "robustness-icon",
                t!("prop.robustness_icon").into(),
                p.style.iconic,
                workspace.clone(),
                p.id,
                |s, v| s.iconic = v,
            ));
        }
        if matches!(shown, Some(ElementKind::Class(_) | ElementKind::Interface(_)))
            && !p.style.iconic
        {
            body = body
                .child(style_flag(
                    "hide-attribute-constraints",
                    t!("prop.hide_attribute_constraints").into(),
                    p.style.hide_attribute_constraints,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_attribute_constraints = v,
                ))
                .child(style_flag(
                    "hide-operation-constraints",
                    t!("prop.hide_operation_constraints").into(),
                    p.style.hide_operation_constraints,
                    workspace.clone(),
                    p.id,
                    |s, v| s.hide_operation_constraints = v,
                ));
        }
        if matches!(p.shape, umlcraft_model::Shape::Edge { .. })
            && shown.as_ref().is_some_and(has_constraints)
        {
            body = body.child(style_flag(
                "hide-constraints",
                t!("prop.hide_constraints").into(),
                p.style.hide_constraints,
                workspace.clone(),
                p.id,
                |s, v| s.hide_constraints = v,
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
            // A required interface: a half circle instead of the circle.
            if p.style.iconic {
                body = body.child(style_flag(
                    "socket",
                    t!("prop.socket").into(),
                    p.style.socket,
                    workspace.clone(),
                    p.id,
                    |s, v| s.socket = v,
                ));
            }
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
        body
    }

    /// A color picker with a button back to the theme's color.
    fn color_select(
        &self,
        state: &Entity<ColorPickerState>,
        set: bool,
        line: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let panel = cx.entity();
        let featured = FEATURED_COLORS.map(crate::canvas::hsla).to_vec();
        h_flex()
            .gap_1()
            .child(
                ColorSelect::new(state)
                    .small()
                    .featured_colors(featured.clone())
                    .placeholder(t!("prop.default_color"))
                    .flex_1(),
            )
            .when(set, |row| {
                row.child(
                    Button::new(("reset-color", usize::from(line)))
                        .ghost()
                        .small()
                        .label(t!("prop.reset_color"))
                        .on_click(move |_, _, cx| {
                            panel.update(cx, |this, cx| this.set_color(line, None, cx));
                        }),
                )
            })
            .into_any_element()
    }
}
