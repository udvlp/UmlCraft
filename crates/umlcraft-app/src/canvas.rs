//! The diagram editor surface: paints a [`Scene`] and handles tools, selection, dragging,
//! waypoints, in-place editing and elements dropped from the model tree.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{
    Aggregation, AssociationEnd, ElementKind, Id, LineShape, Model, Multiplicity, Navigability,
    Point as MPoint, Rect, Shape, TypeRef,
};
use umlcraft_notation::draw::{self, Paint, Primitive};
use umlcraft_notation::member::{self, member_text};
use umlcraft_notation::object;
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::route::{self, Route};
use umlcraft_notation::scene::{
    self, Compartment, CompartmentHit, EdgeHandle, EdgeShape, EndLabelHit, FONT_SIZE, Figure,
    Handle, LabelAlign, LineStyle, NodeShape, Preview, Row, RowRole, Scene, TextMeasure,
};
use umlcraft_notation::sequence::SequenceLayout;

use crate::workspace::{EndChange, NewItem, Workspace};
use crate::{
    AddAttribute, AddExtensionPoint, AddLiteral, AddOperation, Cancel, ChangeRelationship,
    ConnectAs, DeleteMember, DeleteSelection, LINE_SHAPES, RemoveFromDiagram, ResetLabels,
    ResetLineEnds, ResetZoom, SelectAll, SetAssociationEnd, SetLineShape, SetMemberType, ZoomToFit,
};

const MIN_ZOOM: f32 = 0.25;
/// The fonts for diagram texts, by preference. Astah uses Java's "Dialog", which is Arial on
/// Windows; Liberation Sans and Arimo have Arial's metrics, for Linux without Arial.
const DIAGRAM_FONTS: [&str; 3] = ["Arial", "Liberation Sans", "Arimo"];
static DIAGRAM_FONT: OnceLock<SharedString> = OnceLock::new();

/// Picks the diagram font from the installed families, once at startup. False when none of
/// [`DIAGRAM_FONTS`] is installed: the text system then substitutes another font, and texts
/// are measured wider or narrower than Astah draws them.
pub fn choose_diagram_font(cx: &App) -> bool {
    let installed = cx.text_system().all_font_names();
    let found = DIAGRAM_FONTS.iter().find(|f| installed.iter().any(|i| i == *f));
    DIAGRAM_FONT.get_or_init(|| found.copied().unwrap_or(DIAGRAM_FONTS[0]).into());
    found.is_some()
}

fn diagram_font() -> SharedString {
    DIAGRAM_FONT.get().cloned().unwrap_or(DIAGRAM_FONTS[0].into())
}
/// Where GPUI puts the baseline below the top of its line, relative to the font size. It
/// centers ascent plus descent in the line: (15.09375 − 10.86328 − 2.54297) / 2 + 10.86328 =
/// 11.70703 for Arial's 1854 and 434 units per 2048 at 12 px in a line of
/// `scene::line_height(12)`. Texts are moved down to Astah's baseline (`draw::baseline`).
const GPUI_BASELINE: f32 = 11.70703 / 12.0;
const MAX_ZOOM: f32 = 4.0;
/// Height of a hover handle and the gap between handles, in screen pixels.
const HANDLE_SIZE: f32 = 20.0;
const HANDLE_GAP: f32 = 3.0;
/// How wide the column of handles right of a box is taken to be: the mouse keeps the handles
/// while it moves onto them.
const HANDLE_COLUMN: f32 = 120.0;
/// How far, in screen pixels, the mouse may move between press and release for a click.
const CLICK_SLOP: f32 = 6.0;
/// A top left corner (x, y) and an area (left, top, right, bottom), in canvas-local pixels.
type Corner = (f32, f32);
type Area = (f32, f32, f32, f32);
const GRID: f32 = 20.0;
/// Smallest node edge when resizing.
const MIN_NODE: f32 = 20.0;
/// How close (in screen pixels) the pointer must be to grab a handle.
const HANDLE_GRAB: f32 = 6.0;
/// How near, in screen pixels, the pointer must come to a line or figure to hit it.
const HIT_TOLERANCE: f32 = 4.0;

/// Snaps a logical coordinate or distance to half the grid.
fn snap(v: f32) -> f32 {
    (v / GRID * 2.0).round() * GRID / 2.0
}

fn snap_point(p: MPoint) -> MPoint {
    MPoint::new(snap(p.x), snap(p.y))
}

/// One end of an association as the context menu shows it.
struct EndEntry {
    /// The end's index in the association.
    index: usize,
    label: String,
    end: AssociationEnd,
}

/// The association `edge` shows, with a menu label for each end: the participant's name and its
/// role, if any. The end drawn nearer to `click` comes first and is marked as the clicked one,
/// so that a change made from the menu lands where the user clicked.
fn association_ends(
    model: &Model,
    diagram: Option<Id>,
    edge: &EdgeShape,
    click: Option<MPoint>,
) -> Option<(Id, Vec<EndEntry>)> {
    let view = model.view(diagram?)?;
    let presentation = view.get(edge.id)?;
    let element = presentation.element?;
    let ElementKind::Association(p) = model.get(element)?.kind() else { return None };
    let Shape::Edge { source, target, .. } = &presentation.shape else { return None };
    let mut ends: Vec<EndEntry> = p
        .ends
        .iter()
        .enumerate()
        .map(|(index, end)| {
            let name = model.get(end.participant).map(|e| e.name().to_owned()).unwrap_or_default();
            let label = if end.role.is_empty() { name } else { format!("{name} ({})", end.role) };
            EndEntry { index, label, end: end.clone() }
        })
        .collect();
    if let Some(click) = click {
        let [from, to] = edge.node_tips;
        let near_target = click.distance(to) < click.distance(from);
        let node = if near_target { *target } else { *source };
        let participant = view.get(node).and_then(|n| n.element);
        // A reflexive association has the same participant at both ends: the line's own end
        // then decides, as the source shows the first end.
        let near = match p.ends.iter().position(|e| Some(e.participant) == participant) {
            Some(i) if p.ends[0].participant != p.ends[1].participant => i,
            _ => usize::from(near_target),
        };
        ends.swap(0, near);
        ends[0].label = t!("menu.clicked_end", end = ends[0].label).to_string();
    }
    Some((element, ends))
}

/// The choices of one submenu for an end: label, whether the end has it, the change.
type EndChoices = fn(&AssociationEnd) -> Vec<(String, bool, EndChange)>;

fn navigability_choices(end: &AssociationEnd) -> Vec<(String, bool, EndChange)> {
    [
        (Navigability::Unspecified, "navigability.unspecified"),
        (Navigability::Navigable, "navigability.navigable"),
        (Navigability::NonNavigable, "navigability.non_navigable"),
    ]
    .into_iter()
    .map(|(n, key)| (t!(key).to_string(), end.navigability == n, EndChange::Navigability(n)))
    .collect()
}

fn aggregation_choices(end: &AssociationEnd) -> Vec<(String, bool, EndChange)> {
    [
        (Aggregation::None, "aggregation.none"),
        (Aggregation::Shared, "aggregation.shared"),
        (Aggregation::Composite, "aggregation.composite"),
    ]
    .into_iter()
    .map(|(a, key)| (t!(key).to_string(), end.aggregation == a, EndChange::Aggregation(a)))
    .collect()
}

fn multiplicity_choices(end: &AssociationEnd) -> Vec<(String, bool, EndChange)> {
    [
        None,
        Some(Multiplicity::OPTIONAL),
        Some(Multiplicity::ONE),
        Some(Multiplicity::MANY),
        Some(Multiplicity::ONE_OR_MORE),
    ]
    .into_iter()
    .map(|m| {
        let label = m.map_or_else(|| t!("menu.unspecified").to_string(), |m| m.to_string());
        (label, end.multiplicity == m, EndChange::Multiplicity(m))
    })
    .collect()
}

/// Astah's submenus for an association: navigability, aggregation and multiplicity, each with
/// a section per end.
fn association_menu(
    menu: PopupMenu,
    association: Id,
    ends: Vec<EndEntry>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let ends = Rc::new(ends);
    let submenus: [(&str, EndChoices); 3] = [
        ("menu.navigability", navigability_choices),
        ("menu.aggregation", aggregation_choices),
        ("menu.multiplicity", multiplicity_choices),
    ];
    submenus.into_iter().fold(menu, |menu, (title, choices)| {
        let ends = ends.clone();
        menu.submenu(t!(title), window, cx, move |menu, _, _| {
            ends.iter().enumerate().fold(menu, |mut menu, (i, entry)| {
                if i > 0 {
                    menu = menu.separator();
                }
                menu = menu.label(entry.label.clone());
                choices(&entry.end).into_iter().fold(menu, |menu, (label, checked, change)| {
                    let action = SetAssociationEnd { association, end: entry.index, change };
                    menu.menu_with_check(label, checked, Box::new(action))
                })
            })
        })
    })
}

/// Where a lifeline's line runs: its x, top and bottom; `None` for other nodes.
fn lifeline_end(node: &NodeShape) -> Option<(f32, f32, f32)> {
    match node.figure {
        Figure::Lifeline { head, line: (top, bottom), .. } => Some((head.center().x, top, bottom)),
        _ => None,
    }
}

/// The types the "Type" submenu offers for an attribute or operation `member`: basic types,
/// then the model's classes, interfaces, enumerations and data types by name (a `None` between
/// them), each with whether the member has it.
fn member_types(model: &Model, member: Id) -> Option<Vec<Option<(String, TypeRef, bool)>>> {
    let (current, operation) = match model.get(member)?.kind() {
        ElementKind::Attribute(p) => (p.type_ref.clone(), false),
        ElementKind::Operation(p) => (p.return_type.clone(), true),
        _ => return None,
    };
    let entry = |label: String, type_ref: TypeRef| {
        let checked = current.as_ref() == Some(&type_ref);
        Some((label, type_ref, checked))
    };
    let basic = member::basic_types(model, operation);
    let mut types: Vec<_> =
        basic.iter().map(|name| entry(name.clone(), member::resolve_type(model, name))).collect();
    let mut own: Vec<(String, Id)> = model
        .elements()
        .filter(|e| {
            matches!(
                e.kind(),
                ElementKind::Class(_)
                    | ElementKind::Interface(_)
                    | ElementKind::Enumeration(_)
                    | ElementKind::DataType(_)
            )
        })
        .filter(|e| !e.name().is_empty() && !basic.iter().any(|b| b == e.name()))
        .map(|e| (e.name().to_owned(), e.id()))
        .collect();
    own.sort();
    if !own.is_empty() {
        types.push(None);
        types.extend(own.into_iter().map(|(name, id)| entry(name, TypeRef::Element(id))));
    }
    Some(types)
}

/// The context menu's entry that deletes a member of kind `kind`, if it is one.
fn delete_member_key(kind: &ElementKind) -> Option<&'static str> {
    match kind {
        ElementKind::Attribute(_) => Some("menu.delete_attribute"),
        ElementKind::Operation(_) => Some("menu.delete_operation"),
        ElementKind::EnumerationLiteral => Some("menu.delete_literal"),
        _ => None,
    }
}

/// An element dragged from the model tree.
#[derive(Clone)]
pub struct DraggedElement {
    pub id: Id,
    pub label: SharedString,
}

impl Render for DraggedElement {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .text_sm()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .child(self.label.clone())
    }
}

#[derive(Clone)]
enum Drag {
    /// Moving the selected nodes; `start` is the logical grab point. `reselect` is a node that
    /// was already part of a multi-selection: a click without movement selects just it.
    /// `moved` is the selection with the contents of its containers, taken from the scene as it
    /// was before the drag, so nodes a container passes over are not picked up.
    Move {
        start: MPoint,
        dx: f32,
        dy: f32,
        reselect: Option<Id>,
        moved: Vec<Id>,
    },
    Resize {
        presentation: Id,
        handle: Handle,
        original: Rect,
        start: MPoint,
        rect: Rect,
    },
    /// Dragging a grip of an edge, `original` as drawn when the drag started.
    Waypoint {
        handle: EdgeHandle,
        original: Rc<EdgeShape>,
        current: MPoint,
    },
    RubberBand {
        start: MPoint,
        current: MPoint,
        extend: bool,
    },
    /// Drawing a new connection from `source`. Survives a click on the source itself, so both
    /// drag-and-drop and click-click work.
    Connect {
        source: Id,
        start: MPoint,
        current: MPoint,
    },
    /// Dragging a message or fragment of a sequence diagram up or down to another place.
    Reorder {
        element: Id,
        start: MPoint,
        current: MPoint,
    },
    /// Dragging the label of a sequence message; `origin` is its top left corner at the start.
    MessageLabel {
        message: Id,
        origin: MPoint,
        start: MPoint,
        current: MPoint,
    },
    /// Dragging the «include» or «extend» of `edge`; `origin` is its top left corner at the
    /// start, `middle` the middle of the line.
    KeywordLabel {
        edge: Id,
        origin: MPoint,
        middle: MPoint,
        start: MPoint,
        current: MPoint,
    },
    /// Dragging line `index` between the regions of the orthogonal state `presentation`, drawn
    /// at `state`; `vertical` lines move sideways.
    RegionLine {
        presentation: Id,
        index: usize,
        state: Rect,
        vertical: bool,
        current: MPoint,
    },
    Pan {
        last: Point<Pixels>,
    },
    /// Drawing a free line from `start`.
    FreeLine {
        start: MPoint,
        current: MPoint,
    },
    /// Dragging end `end` (0 start, 1 end) of the free line `line`.
    LineEnd {
        line: Id,
        end: usize,
        current: MPoint,
    },
    /// Dragging the open end of the found or lost message `message`, at height `y` beside the
    /// lifeline at `x`.
    Gate {
        message: Id,
        x: f32,
        y: f32,
        current: MPoint,
    },
    /// Dragging the end of the lifeline `presentation`, whose line runs down from `top` at `x`.
    LifelineEnd {
        presentation: Id,
        x: f32,
        top: f32,
        current: MPoint,
    },
    /// Drawing a line from a hover handle of `source`, or a message from its lifeline.
    HandleLine {
        source: Id,
        start: MPoint,
        current: MPoint,
        message: bool,
    },
}

enum Editor {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

/// Text being edited in place, over the row it replaces.
struct InlineEdit {
    /// The element edited; for a free text, its presentation.
    element: Id,
    /// For a slot row: the object whose slot for the attribute `element` is edited.
    slot_of: Option<Id>,
    /// `element` is a free text's presentation.
    free_text: bool,
    /// Logical rectangle of the edited text.
    rect: Rect,
    /// The text the editor opened with; committing it unchanged changes nothing.
    original: String,
    /// A member not made yet: the text becomes one on committing.
    new_member: Option<NewMember>,
    /// An association end of `element`, by index, whose role (or, if true, multiplicity) is
    /// edited.
    end: Option<(usize, bool)>,
    editor: Editor,
    _subscription: Subscription,
}

/// Where a member typed into an empty row goes.
#[derive(Clone, Copy)]
struct NewMember {
    /// The classifier's box and the classifier.
    node: Id,
    owner: Id,
    compartment: Compartment,
    /// The member it follows; none puts it at the end.
    after: Option<Id>,
}

/// What a handle beside the node under the mouse does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum HoverHandle {
    /// Dragged to another node, draws a line to it.
    Connect,
    /// Opens an empty row for a new member at the end of the compartment.
    AddMember(Compartment),
    /// Dragged to a lifeline, draws a message from `lifeline` at the height `y`.
    Message { lifeline: Id, y: f32 },
}

/// The node under the mouse and the handles it shows beside it.
#[derive(Clone, Debug, PartialEq)]
struct Hover {
    node: Id,
    handles: Vec<HoverHandle>,
}

/// A line dragged from a handle onto a node it can reach in several ways: the menu offering
/// them, at `position` (window pixels), built on the next render.
struct Offer {
    source: Id,
    target: Id,
    start: MPoint,
    tools: Vec<EdgeTool>,
    position: Point<Pixels>,
    menu: Option<(Entity<PopupMenu>, Subscription)>,
}

pub struct DiagramCanvas {
    workspace: Entity<Workspace>,
    focus_handle: FocusHandle,
    zoom: f32,
    /// Screen offset of the logical origin, relative to the canvas bounds.
    pan: MPoint,
    /// Zoom and pan of diagrams that are not active right now.
    viewports: HashMap<Id, (f32, MPoint)>,
    shown_diagram: Option<Id>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
    /// The scene as last painted; mouse handling works on what the user sees.
    scene: Rc<Scene>,
    drag: Option<Drag>,
    /// Where the last right click went, for the context menu.
    right_click: Option<MPoint>,
    /// The member row the last right click hit, for the context menu.
    right_member: Option<Id>,
    editing: Option<InlineEdit>,
    /// An element whose row is edited inline once the next scene shows it.
    edit_when_drawn: Option<Id>,
    /// The next editor opens with its text selected.
    select_when_drawn: bool,
    /// A compartment of a box (its presentation) that gets an empty row for the next member
    /// once the next scene shows the one just added.
    new_member_when_drawn: Option<(Id, Compartment)>,
    /// How the diagram is drawn beyond what it stores (settings).
    pub display: Display,
    /// The corner radius of the card the canvas fills; background and grid keep inside the
    /// corners.
    pub corner_radius: f32,
    /// What the scene was built from: revision, diagram, display settings and locale. Without a
    /// drag the scene is only built again when one of them changes, not on every pan, zoom or
    /// caret blink.
    scene_key: Option<(u64, Id, Display, String)>,
    /// The node under the mouse with its handles (Select tool, nothing dragged or edited).
    hover: Option<Hover>,
    /// The kinds of line a line dragged from a handle can become.
    offer: Option<Offer>,
}

thread_local! {
    /// Widths measured by [`FontMeasure`], by font size and boldness, then text: shaping is the
    /// expensive part of building a scene, and the texts stay the same from frame to frame.
    /// Looking a text up needs no copy of it.
    static MEASURED: RefCell<HashMap<(u32, bool), HashMap<String, f32>>> =
        RefCell::new(HashMap::new());
}

/// Measured widths kept at most, so that typing in long texts cannot fill the memory.
const MAX_MEASURED: usize = 50_000;

/// How diagrams are drawn beyond what they store: the user's settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Display {
    /// Members show their visibility in UmlCraft's own column.
    pub visibility_column: bool,
    /// Abstract classes show «abstract» above their name instead of `{abstract}` below it.
    pub abstract_stereotype: bool,
    /// Read-only attributes end in `{readOnly}`.
    pub read_only_constraint: bool,
}

/// Measures text with the window's font system, at 100 % zoom.
pub struct FontMeasure<'a> {
    window: &'a Window,
    family: SharedString,
    display: Display,
}

impl<'a> FontMeasure<'a> {
    /// Measures in the diagram font, for drawing as `display` says.
    pub fn new(window: &'a Window, display: Display) -> Self {
        Self { window, family: diagram_font(), display }
    }
}

impl TextMeasure for FontMeasure<'_> {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let font_key = (font_size.to_bits(), bold);
        let cached =
            MEASURED.with(|m| m.borrow().get(&font_key).and_then(|t| t.get(text)).copied());
        if let Some(width) = cached {
            return width;
        }
        let mut f = font(self.family.clone());
        if bold {
            f = f.bold();
        }
        let run = TextRun { len: text.len(), font: f, ..Default::default() };
        let line = self.window.text_system().shape_line(
            text.to_owned().into(),
            px(font_size),
            &[run],
            None,
        );
        let width = f32::from(line.width());
        MEASURED.with(|m| {
            let mut m = m.borrow_mut();
            if m.values().map(HashMap::len).sum::<usize>() >= MAX_MEASURED {
                m.clear();
            }
            m.entry(font_key).or_default().insert(text.to_owned(), width);
        });
        width
    }

    fn visibility_column(&self) -> bool {
        self.display.visibility_column
    }

    fn abstract_stereotype(&self) -> bool {
        self.display.abstract_stereotype
    }

    fn read_only_constraint(&self) -> bool {
        self.display.read_only_constraint
    }
}

/// Measures text with the app's font system by adding up the advances of the characters, as
/// Java does without kerning. It needs no window, so the workspace keeps one for sizing boxes.
pub struct AppMeasure {
    text: Arc<TextSystem>,
    regular: FontId,
    bold: FontId,
    display: Display,
}

impl AppMeasure {
    /// Measures for drawing as `display` says.
    pub fn new(cx: &App, display: Display) -> Self {
        let text = cx.text_system().clone();
        let regular = text.resolve_font(&font(diagram_font()));
        let bold = text.resolve_font(&font(diagram_font()).bold());
        Self { text, regular, bold, display }
    }
}

impl TextMeasure for AppMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        let font = if bold { self.bold } else { self.regular };
        let advance =
            |c| self.text.advance(font, px(font_size), c).map_or(0.0, |s| f32::from(s.width));
        text.chars().map(advance).sum()
    }

    fn visibility_column(&self) -> bool {
        self.display.visibility_column
    }

    fn abstract_stereotype(&self) -> bool {
        self.display.abstract_stereotype
    }

    fn read_only_constraint(&self) -> bool {
        self.display.read_only_constraint
    }
}

impl DiagramCanvas {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
        Self {
            workspace,
            focus_handle: cx.focus_handle(),
            zoom: 1.0,
            pan: MPoint::new(0.0, 0.0),
            viewports: HashMap::new(),
            shown_diagram: None,
            bounds: Rc::new(Cell::new(Bounds::default())),
            scene: Rc::new(Scene::empty()),
            drag: None,
            right_click: None,
            right_member: None,
            editing: None,
            edit_when_drawn: None,
            select_when_drawn: false,
            new_member_when_drawn: None,
            display: Display::default(),
            corner_radius: 0.0,
            scene_key: None,
            hover: None,
            offer: None,
        }
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = MPoint::new(0.0, 0.0);
        cx.notify();
    }

    /// Zooms by `factor`, keeping the canvas center fixed.
    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let b = self.bounds.get();
        let center = point(b.size.width / 2.0, b.size.height / 2.0) + b.origin;
        self.zoom_at(center, factor);
        cx.notify();
    }

    /// Fits the whole diagram into the visible area (at most 150 %).
    pub fn zoom_to_fit(&mut self, cx: &mut Context<Self>) {
        let b = self.bounds.get();
        let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
        let Some(content) = self.scene.bounds().filter(|_| w > 0.0 && h > 0.0) else { return };
        let margin = 40.0;
        let fit_w = (w - 2.0 * margin) / content.width.max(1.0);
        let fit_h = (h - 2.0 * margin) / content.height.max(1.0);
        self.zoom = fit_w.min(fit_h).clamp(MIN_ZOOM, 1.5);
        let c = content.center();
        self.pan = MPoint::new(w / 2.0 - c.x * self.zoom, h / 2.0 - c.y * self.zoom);
        cx.notify();
    }

    /// Where a paste lands: under the pointer if it is over the canvas, else the view center.
    pub fn paste_position(&self, window: &Window) -> MPoint {
        let mouse = window.mouse_position();
        if self.bounds.get().contains(&mouse) {
            return snap_point(self.to_logical(mouse));
        }
        let b = self.bounds.get();
        snap_point(self.to_logical(point(b.size.width / 2.0, b.size.height / 2.0) + b.origin))
    }

    fn zoom_at(&mut self, screen: Point<Pixels>, factor: f32) {
        let anchor = self.to_logical(screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let local = self.local(screen);
        self.pan = MPoint::new(local.x - anchor.x * self.zoom, local.y - anchor.y * self.zoom);
    }

    fn local(&self, screen: Point<Pixels>) -> MPoint {
        let origin = self.bounds.get().origin;
        MPoint::new(f32::from(screen.x - origin.x), f32::from(screen.y - origin.y))
    }

    fn to_logical(&self, screen: Point<Pixels>) -> MPoint {
        Transform { origin: self.bounds.get().origin, pan: self.pan, zoom: self.zoom }
            .to_logical(screen)
    }

    /// [`HIT_TOLERANCE`] in diagram units at the current zoom.
    fn hit_tolerance(&self) -> f32 {
        HIT_TOLERANCE / self.zoom
    }

    /// Keeps a separate zoom and pan per diagram.
    fn switch_viewport(&mut self, diagram: Option<Id>) {
        if diagram == self.shown_diagram {
            return;
        }
        if let Some(old) = self.shown_diagram {
            self.viewports.insert(old, (self.zoom, self.pan));
        }
        (self.zoom, self.pan) = diagram
            .and_then(|d| self.viewports.get(&d).copied())
            .unwrap_or((1.0, MPoint::new(0.0, 0.0)));
        self.shown_diagram = diagram;
        self.drag = None;
        self.editing = None;
    }

    fn preview(&self) -> Preview {
        match &self.drag {
            Some(Drag::Move { dx, dy, moved, .. }) => Preview {
                moved: moved.iter().copied().collect(),
                dx: *dx,
                dy: *dy,
                ..Preview::default()
            },
            Some(Drag::Resize { presentation, rect, .. }) => {
                Preview { resized: Some((*presentation, *rect)), ..Preview::default() }
            }
            Some(Drag::Waypoint { handle, original, current }) => Preview {
                route: Some((original.id, scene::drag_handle(original, *handle, *current))),
                ..Preview::default()
            },
            Some(Drag::LineEnd { line, end, current }) => {
                Preview { line_end: Some((*line, *end, *current)), ..Preview::default() }
            }
            _ => Preview::default(),
        }
    }

    /// The node under `p` for connecting. Fragment frames and swimlanes only count at their
    /// border, so messages and flows can be drawn between what lies inside them.
    fn node_at(&self, p: MPoint) -> Option<Id> {
        let tolerance = self.hit_tolerance();
        self.scene
            .nodes
            .iter()
            .rev()
            .find(|n| match n.figure {
                Figure::Fragment { .. } | Figure::Partition { .. } => n.hit(p, tolerance),
                _ => n.rect.contains(p),
            })
            .map(|n| n.id)
    }

    /// The use case of the one selected node, if it shows one.
    /// The compartments that take members in the one selected box, with the box.
    fn selected_compartments(&self, cx: &App) -> Option<(Id, Vec<Compartment>)> {
        let ws = self.workspace.read(cx);
        let [single] = ws.selection[..] else { return None };
        let Figure::Classifier { compartments, .. } = &self.scene.node(single)?.figure else {
            return None;
        };
        let takes = compartments.iter().copied().filter(|&c| c != Compartment::Slots).collect();
        Some((single, takes))
    }

    /// Opens an empty row at the end of `compartment` of the selected box for a new member.
    fn add_member_row(
        &mut self,
        compartment: Compartment,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((node, _)) = self.selected_compartments(cx) else { return };
        if let Some(hit) = scene::compartment_of(&self.scene, node, compartment) {
            self.open_new_member(hit, window, cx);
        }
    }

    /// The extend the one selected line stands for.
    fn selected_extend(&self, cx: &App) -> Option<Id> {
        let ws = self.workspace.read(cx);
        let [single] = ws.selection[..] else { return None };
        let view = ws.doc.model().view(ws.active_diagram?)?;
        let element = view.get(single)?.element?;
        matches!(ws.doc.model().get(element)?.kind(), ElementKind::Extend(_)).then_some(element)
    }

    fn selected_use_case(&self, cx: &App) -> Option<Id> {
        let ws = self.workspace.read(cx);
        let [single] = ws.selection[..] else { return None };
        let view = ws.doc.model().view(ws.active_diagram?)?;
        let element = view.get(single)?.element?;
        matches!(ws.doc.model().get(element)?.kind(), ElementKind::UseCase(_)).then_some(element)
    }

    /// What a line drawn with `tool` connects to at `p`: a node, or for note anchors also a
    /// relationship's line (not another anchor), as in Astah. An activation bar stands for its
    /// lifeline.
    fn connect_target(&self, tool: EdgeTool, p: MPoint, cx: &App) -> Option<Id> {
        let ws = self.workspace.read(cx);
        let view = ws.active_diagram.and_then(|d| ws.doc.model().view(d))?;
        let line = (tool == EdgeTool::NoteAnchor)
            .then(|| scene::edge_at(&self.scene, p, self.hit_tolerance()))
            .flatten()
            .filter(|&edge| view.get(edge).is_some_and(|p| p.element.is_some()));
        line.or_else(|| self.node_at(p)).map(|id| palette::lifeline_of(view, id))
    }

    /// The message behind a message edge, if `presentation` is one: messages move up and down
    /// instead of freely.
    fn reorderable(&self, presentation: Id, cx: &App) -> Option<Id> {
        self.scene.sequence.as_ref()?;
        if !self.scene.edge(presentation).is_some_and(|e| e.fixed) {
            return None;
        }
        let ws = self.workspace.read(cx);
        ws.doc.model().view(ws.active_diagram?)?.get(presentation)?.element
    }

    /// Whether `presentation` is a free line, which moves like a node.
    fn is_free_line(&self, presentation: Id, cx: &App) -> bool {
        let ws = self.workspace.read(cx);
        ws.active_diagram
            .and_then(|d| ws.doc.model().view(d)?.get(presentation))
            .is_some_and(|p| matches!(p.shape, umlcraft_model::Shape::Line { .. }))
    }

    fn on_left_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.focus_handle.focus(window, cx);
        let p = self.to_logical(ev.position);
        // The second click of a line started from a handle: on an element it connects (on the
        // source itself, a loop or a message to self), elsewhere it gives up.
        if let Some(Drag::HandleLine { source, start, message, .. }) = self.drag.clone() {
            self.drag = None;
            let tool = if message { EdgeTool::Message } else { EdgeTool::Association };
            if let Some(target) = self.connect_target(tool, p, cx) {
                self.offer_connections(source, target, start, ev.position, cx);
            }
            cx.notify();
            return;
        }
        let (tool, selection) = {
            let ws = self.workspace.read(cx);
            (ws.tool, ws.selection.clone())
        };
        let extend = ev.modifiers.shift || ev.modifiers.control;

        match tool {
            Tool::Node(node_tool) => {
                let origin = snap_point(p);
                // A fragment goes where it is placed in the sequence, around the lifelines there.
                let place = self
                    .scene
                    .sequence
                    .as_ref()
                    .filter(|_| {
                        matches!(node_tool, NodeTool::CombinedFragment | NodeTool::InteractionUse)
                    })
                    .map(|l| (l.slot_at(p.y), l.lanes_around(p.x)));
                self.workspace.update(cx, |ws, cx| {
                    match place {
                        Some((slot, covered)) => {
                            ws.create_fragment(node_tool, slot, covered, origin)
                        }
                        None => ws.create_node(node_tool, origin),
                    }
                    ws.tool = Tool::Select;
                    cx.notify();
                });
            }
            Tool::Text => {
                let origin = snap_point(p);
                let text = self.workspace.update(cx, |ws, cx| {
                    ws.tool = Tool::Select;
                    cx.notify();
                    ws.create_free_text(origin)
                });
                self.edit_when_drawn = text;
                cx.notify();
            }
            Tool::Line => {
                let start = snap_point(p);
                self.drag = Some(Drag::FreeLine { start, current: start });
            }
            // Found and lost messages have one lifeline: a click on it draws them.
            Tool::Edge(edge_tool @ (EdgeTool::FoundMessage | EdgeTool::LostMessage)) => {
                self.drag = None;
                if let Some(lifeline) = self.connect_target(edge_tool, p, cx) {
                    self.finish_connect(edge_tool, lifeline, lifeline, p, cx);
                }
            }
            Tool::Edge(edge_tool) => {
                match (self.drag.clone(), self.connect_target(edge_tool, p, cx)) {
                    // Second click of click-click mode; a second click on the source makes a loop.
                    (Some(Drag::Connect { source, start, .. }), Some(target)) => {
                        self.drag = None;
                        self.finish_connect(edge_tool, source, target, start, cx);
                    }
                    (_, Some(source)) => {
                        self.drag = Some(Drag::Connect { source, start: p, current: p });
                    }
                    (_, None) => self.drag = None,
                }
            }
            Tool::Select => {
                if self.grab_handle(&selection, p, ev.click_count, cx) {
                    cx.notify();
                    return;
                }
                // Role names and multiplicities lie over everything else.
                if ev.click_count == 2
                    && let Some(hit) = scene::end_label_at(
                        &self.scene,
                        p,
                        self.hit_tolerance(),
                        &FontMeasure::new(window, self.display),
                    )
                {
                    self.open_end_editor(hit, window, cx);
                    return;
                }
                if ev.click_count == 2 && self.start_edit_at(p, window, cx) {
                    return;
                }
                // Below the members of a compartment, a double click starts a new one.
                if ev.click_count == 2
                    && let Some(hit) = scene::compartment_at(&self.scene, p)
                {
                    self.open_new_member(hit, window, cx);
                    return;
                }
                // The lines between the regions of a state can be moved.
                let line = scene::region_line_at(&self.scene, p, self.hit_tolerance())
                    .filter(|_| !extend)
                    .map(|(n, index, vertical)| (n.id, index, n.rect, vertical));
                if let Some((presentation, index, state, vertical)) = line {
                    self.workspace.update(cx, |ws, _| ws.select(vec![presentation]));
                    self.drag =
                        Some(Drag::RegionLine { presentation, index, state, vertical, current: p });
                    cx.notify();
                    return;
                }
                // A message's label can be moved away from its line.
                let label = scene::message_label_at(
                    &self.scene,
                    p,
                    &FontMeasure::new(window, self.display),
                );
                if let Some((message, origin)) = label.filter(|_| !extend) {
                    self.workspace.update(cx, |ws, _| ws.select(vec![message]));
                    self.drag = Some(Drag::MessageLabel { message, origin, start: p, current: p });
                    cx.notify();
                    return;
                }
                // So can «include» and «extend».
                let label = scene::keyword_label_at(
                    &self.scene,
                    p,
                    &FontMeasure::new(window, self.display),
                );
                if let Some((edge, origin, middle)) = label.filter(|_| !extend) {
                    self.workspace.update(cx, |ws, _| ws.select(vec![edge]));
                    self.drag =
                        Some(Drag::KeywordLabel { edge, origin, middle, start: p, current: p });
                    cx.notify();
                    return;
                }
                match scene::hit_test(&self.scene, p, self.hit_tolerance()) {
                    Some(id) => {
                        let already = selection.contains(&id);
                        // A click on a member row selects the member for the properties panel.
                        let member = scene::row_at(&self.scene, p)
                            .filter(|(n, r)| n.id == id && r.role == RowRole::Member)
                            .and_then(|(_, r)| r.element);
                        self.workspace.update(cx, |ws, cx| {
                            if extend {
                                ws.toggle(id);
                            } else if !already {
                                ws.select(vec![id]);
                            }
                            if let Some(member) = member.filter(|_| !extend) {
                                ws.selected_element = Some(member);
                            }
                            cx.notify();
                        });
                        let selected = self.workspace.read(cx).selection.contains(&id);
                        let reorder = self.reorderable(id, cx).filter(|_| selected && !extend);
                        if let Some(element) = reorder {
                            self.drag = Some(Drag::Reorder { element, start: p, current: p });
                        } else if (self.scene.node(id).is_some() || self.is_free_line(id, cx))
                            && selected
                        {
                            let reselect =
                                (already && !extend && selection.len() > 1).then_some(id);
                            // Containers (boundaries, packages) take the nodes drawn inside
                            // them along.
                            let selection = &self.workspace.read(cx).selection;
                            let moved = scene::with_contents(&self.scene, selection);
                            self.drag =
                                Some(Drag::Move { start: p, dx: 0.0, dy: 0.0, reselect, moved });
                        }
                    }
                    None => {
                        if !extend {
                            self.workspace.update(cx, |ws, cx| {
                                ws.clear_selection();
                                cx.notify();
                            });
                        }
                        self.drag = Some(Drag::RubberBand { start: p, current: p, extend });
                    }
                }
            }
        }
        cx.notify();
    }

    /// Starts dragging a resize handle or an edge grip of the single selected item. A double
    /// click on a waypoint removes it.
    fn grab_handle(
        &mut self,
        selection: &[Id],
        p: MPoint,
        clicks: usize,
        cx: &mut Context<Self>,
    ) -> bool {
        let [single] = selection[..] else { return false };
        let tolerance = HANDLE_GRAB / self.zoom;
        if let Some((x, top, bottom)) = self.scene.node(single).and_then(lifeline_end)
            && (p.x - x).abs() <= tolerance
            && (p.y - bottom).abs() <= tolerance
        {
            self.drag = Some(Drag::LifelineEnd { presentation: single, x, top, current: p });
            return true;
        }
        if let Some(node) = self.scene.node(single).filter(|n| n.resizable())
            && let Some(handle) = Handle::at(node.rect, p, tolerance)
        {
            self.drag = Some(Drag::Resize {
                presentation: single,
                handle,
                original: node.rect,
                start: p,
                rect: node.rect,
            });
            return true;
        }
        if let Some((gate, x)) = self.gate_of(single, cx)
            && (p.x - gate.x).abs() <= tolerance
            && (p.y - gate.y).abs() <= tolerance
        {
            self.drag = Some(Drag::Gate { message: single, x, y: gate.y, current: gate });
            return true;
        }
        let Some(edge) = self.scene.edge(single) else { return false };
        if self.is_free_line(single, cx) {
            let near =
                |q: &MPoint| (q.x - p.x).abs() <= tolerance && (q.y - p.y).abs() <= tolerance;
            let Some(end) = edge.points.iter().position(near) else { return false };
            self.drag = Some(Drag::LineEnd { line: single, end, current: edge.points[end] });
            return true;
        }
        let Some(handle) = edge.handle_at(p, tolerance) else { return false };
        match (handle, clicks) {
            (EdgeHandle::Waypoint(i), 2) => {
                let mut route = edge.route();
                route.waypoints.remove(i);
                self.workspace.update(cx, |ws, cx| {
                    ws.set_route(single, route);
                    cx.notify();
                });
            }
            _ => {
                let original = Rc::new(edge.clone());
                self.drag = Some(Drag::Waypoint { handle, original, current: p });
            }
        }
        true
    }

    /// The open end of the found or lost message `presentation` on the active diagram, and the
    /// x of its lifeline.
    fn gate_of(&self, presentation: Id, cx: &App) -> Option<(MPoint, f32)> {
        let ws = self.workspace.read(cx);
        let view = ws.doc.model().view(ws.active_diagram?)?;
        let Shape::Message { source, y, gate_x: Some(gx), .. } = view.get(presentation)?.shape
        else {
            return None;
        };
        let (x, _, _) = self.scene.node(source).and_then(lifeline_end)?;
        Some((MPoint::new(gx, y), x))
    }

    // ----- Hover handles -------------------------------------------------------------------

    /// Shows the handles of the node under the mouse, if they change.
    fn update_hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let hover = self.hover_at(position, cx);
        if hover != self.hover {
            self.hover = hover;
            cx.notify();
        }
    }

    /// The node under `position` and its handles: a line handle on classes, interfaces,
    /// enumerations, data types, actors, use cases and objects, one for each compartment that
    /// takes members, and on a sequence diagram a message handle on activation bars and
    /// lifelines. Over the handles shown, they stay.
    fn hover_at(&self, position: Point<Pixels>, cx: &App) -> Option<Hover> {
        let ws = self.workspace.read(cx);
        if ws.tool != Tool::Select
            || self.drag.is_some()
            || self.editing.is_some()
            || self.offer.is_some()
        {
            return None;
        }
        let origin = self.bounds.get().origin;
        let (x, y) = (f32::from(position.x - origin.x), f32::from(position.y - origin.y));
        if let Some(old) = &self.hover
            && let Some((_, (left, top, right, bottom))) = self.hover_layout(old)
            && (left..=right).contains(&x)
            && (top..=bottom).contains(&y)
        {
            return Some(old.clone());
        }
        let p = self.to_logical(position);
        let model = ws.doc.model();
        let view = model.view(ws.active_diagram?)?;
        let tolerance = self.hit_tolerance();
        if self.scene.sequence.is_some() {
            let bars = self.scene.nodes.iter().rev();
            let bar = bars
                .filter(|n| matches!(n.figure, Figure::Activation))
                .find(|n| n.rect.inflate(tolerance).contains(p));
            let line = || {
                self.scene.nodes.iter().rev().find(|n| {
                    lifeline_end(n).is_some_and(|(lx, top, bottom)| {
                        (p.x - lx).abs() <= tolerance && p.y > top && p.y < bottom
                    })
                })
            };
            let node = bar.or_else(line)?;
            let lifeline = palette::lifeline_of(view, node.id);
            let handles = vec![HoverHandle::Message { lifeline, y: p.y }];
            return Some(Hover { node: node.id, handles });
        }
        let node = self.scene.nodes.iter().rev().find(|n| n.hit(p, tolerance))?;
        use ElementKind as K;
        let connects = matches!(
            model.get(node.element?)?.kind(),
            K::Class(_)
                | K::Interface(_)
                | K::Enumeration(_)
                | K::DataType(_)
                | K::Actor(_)
                | K::UseCase(_)
                | K::Instance(_)
        );
        let mut handles = Vec::new();
        if connects {
            handles.push(HoverHandle::Connect);
        }
        if let Figure::Classifier { compartments, .. } = &node.figure {
            let takes = compartments.iter().filter(|&&c| c != Compartment::Slots);
            handles.extend(takes.map(|&c| HoverHandle::AddMember(c)));
        }
        (!handles.is_empty()).then_some(Hover { node: node.id, handles })
    }

    /// Where the handles of `hover` go, in canvas-local pixels: the top left corner of the
    /// first, and the area (left, top, right, bottom) that keeps them while the mouse is on
    /// it. A box has them in a column at its right, a lifeline one beside its line or bar.
    fn hover_layout(&self, hover: &Hover) -> Option<(Corner, Area)> {
        let node = self.scene.node(hover.node)?;
        let screen = |q: MPoint| (self.pan.x + q.x * self.zoom, self.pan.y + q.y * self.zoom);
        if let [HoverHandle::Message { y, .. }] = hover.handles[..] {
            let right = match node.figure {
                Figure::Activation => node.rect.right(),
                _ => lifeline_end(node)?.0,
            };
            let (x, y) = screen(MPoint::new(right, y));
            let at = (x + HANDLE_GAP, y - HANDLE_SIZE / 2.0);
            let area = (x, at.1 - 2.0, at.0 + HANDLE_SIZE + 2.0, at.1 + HANDLE_SIZE + 2.0);
            return Some((at, area));
        }
        let (x, y) = screen(MPoint::new(node.rect.right(), node.rect.y));
        let at = (x + HANDLE_GAP, y);
        let height = hover.handles.len() as f32 * (HANDLE_SIZE + HANDLE_GAP);
        Some((at, (x, y, at.0 + HANDLE_COLUMN, y + height)))
    }

    /// The handles of the node under the mouse, as buttons over the canvas.
    fn hover_handles(&self, cx: &mut Context<Self>) -> Option<Div> {
        let hover = self.hover.clone()?;
        if self.drag.is_some() || self.editing.is_some() || self.offer.is_some() {
            return None;
        }
        let ((x, y), _) = self.hover_layout(&hover)?;
        let theme = cx.theme();
        let (border, fill, ink, accent) =
            (theme.input, theme.popover, theme.popover_foreground, theme.accent);
        let mut column = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .flex()
            .flex_col()
            .items_start()
            .gap(px(HANDLE_GAP));
        for (i, handle) in hover.handles.iter().copied().enumerate() {
            let (icon, label, tip) = match handle {
                HoverHandle::Connect => ("arrow_right", None, t!("canvas.handle_connect")),
                HoverHandle::Message { .. } => ("arrow_right", None, t!("canvas.handle_message")),
                HoverHandle::AddMember(c) => {
                    let (kind, tip) = match c {
                        Compartment::Operations => ("kind.operation", "menu.add_operation"),
                        Compartment::Literals => ("kind.enumerationLiteral", "menu.add_literal"),
                        _ => ("kind.attribute", "menu.add_attribute"),
                    };
                    ("add", Some(t!(kind)), t!(tip))
                }
            };
            let tip = SharedString::from(tip.to_string());
            let button = div()
                .id(("hover-handle", i))
                .h(px(HANDLE_SIZE))
                .min_w(px(HANDLE_SIZE))
                .px(px(3.0))
                .flex()
                .items_center()
                .justify_center()
                .gap_1()
                .rounded(px(4.0))
                .border_1()
                .border_color(border)
                .bg(fill)
                .text_color(ink)
                .text_xs()
                .cursor_pointer()
                .hover(move |s| s.bg(accent))
                .child(crate::shell::icon(crate::icons::fluent(icon)).xsmall())
                .when_some(label, |b, label| b.pr(px(6.0)).child(label.to_string()))
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.grab_hover_handle(handle, ev.position, window, cx);
                    }),
                );
            column = column.child(button);
        }
        Some(column)
    }

    /// A press on a handle: a new member row, or a line or message drawn from there.
    fn grab_hover_handle(
        &mut self,
        handle: HoverHandle,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_edit(cx);
        self.focus_handle.focus(window, cx);
        let Some(hover) = self.hover.take() else { return };
        let p = self.to_logical(position);
        match handle {
            HoverHandle::AddMember(compartment) => {
                if let Some(hit) = scene::compartment_of(&self.scene, hover.node, compartment) {
                    self.open_new_member(hit, window, cx);
                }
            }
            HoverHandle::Connect => {
                let source = hover.node;
                self.drag = Some(Drag::HandleLine { source, start: p, current: p, message: false });
            }
            HoverHandle::Message { lifeline, y } => {
                let start = MPoint::new(p.x, y);
                self.drag =
                    Some(Drag::HandleLine { source: lifeline, start, current: p, message: true });
            }
        }
        cx.notify();
    }

    /// A line dragged from a handle of `source` dropped on `target`: drawn at once if only one
    /// kind of line fits, else a menu at `position` offers the kinds that do.
    fn offer_connections(
        &mut self,
        source: Id,
        target: Id,
        start: MPoint,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let tools = {
            let ws = self.workspace.read(cx);
            let model = ws.doc.model();
            let Some(diagram) = ws.active_diagram else { return };
            let Some(ElementKind::Diagram(props)) = model.get(diagram).map(|d| d.kind()) else {
                return;
            };
            let Some(view) = model.view(diagram) else { return };
            let element = |p: Id| view.get(p).and_then(|p| p.element);
            let (Some(s), Some(t)) = (element(source), element(target)) else { return };
            let (Some(sk), Some(tk)) = (model.get(s), model.get(t)) else { return };
            palette::connections(props, sk.kind(), tk.kind(), s == t)
        };
        match tools[..] {
            [] => {}
            [only] => self.finish_connect(only, source, target, start, cx),
            _ => {
                self.offer = Some(Offer { source, target, start, tools, position, menu: None });
                cx.notify();
            }
        }
    }

    /// The menu of [`Offer`], built on the first render after the drop.
    fn offer_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let focus = self.focus_handle.clone();
        let offer = self.offer.as_mut()?;
        if offer.menu.is_none() {
            let (source, target, start) = (offer.source, offer.target, offer.start);
            let tools = offer.tools.clone();
            let menu = PopupMenu::build(window, cx, move |menu, _, _| {
                tools
                    .iter()
                    .fold(menu, |menu, &tool| {
                        let label = crate::ribbon::tool_label(Tool::Edge(tool));
                        let icon = crate::shell::icon(crate::icons::tool_icon(Tool::Edge(tool)));
                        let action = Box::new(ConnectAs { tool, source, target, start });
                        menu.menu_with_icon(label, icon, action)
                    })
                    .action_context(focus)
            });
            let subscription =
                cx.subscribe_in(&menu, window, |this, _, _: &DismissEvent, window, cx| {
                    this.offer = None;
                    this.focus_handle.focus(window, cx);
                    cx.notify();
                });
            let focus = menu.focus_handle(cx);
            cx.defer_in(window, move |_, window, cx| focus.focus(window, cx));
            offer.menu = Some((menu, subscription));
        }
        let (menu, _) = offer.menu.as_ref()?;
        Some(
            deferred(
                anchored()
                    .position(offer.position)
                    .snap_to_window_with_margin(px(8.0))
                    .child(menu.clone()),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// Connects `source` to `target`. Messages go where the drawing started (`start`).
    fn finish_connect(
        &mut self,
        tool: EdgeTool,
        source: Id,
        target: Id,
        start: MPoint,
        cx: &mut Context<Self>,
    ) {
        let slot = self.scene.sequence.as_ref().and_then(|l| l.slot_at(start.y));
        self.workspace.update(cx, |ws, cx| {
            if tool.message_sort().is_some() {
                ws.connect_message(tool, source, target, slot);
            } else {
                ws.connect(tool, source, target);
            }
            ws.tool = Tool::Select;
            cx.notify();
        });
    }

    /// A right click selects what it hits (unless that is selected already), so the context
    /// menu acts on it.
    fn on_right_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.focus_handle.focus(window, cx);
        if self.drag.is_some() {
            return;
        }
        let p = self.to_logical(ev.position);
        self.right_click = Some(p);
        let hit = scene::hit_test(&self.scene, p, self.hit_tolerance());
        // On a member row, the member is selected too, for the properties panel and the menu.
        let member = scene::row_at(&self.scene, p)
            .filter(|(n, r)| Some(n.id) == hit && r.role == RowRole::Member)
            .and_then(|(_, r)| r.element);
        self.right_member = member;
        self.update_workspace(cx, |ws| {
            match hit {
                Some(id) if ws.selection.contains(&id) => {}
                Some(id) => ws.select(vec![id]),
                None => ws.clear_selection(),
            }
            if member.is_some() {
                ws.selected_element = member;
            }
        });
    }

    /// The selected edges that can be routed by hand (not sequence messages).
    fn selected_edges(&self, cx: &App) -> Vec<&EdgeShape> {
        let ws = self.workspace.read(cx);
        ws.selection.iter().filter_map(|&id| self.scene.edge(id)).filter(|e| !e.fixed).collect()
    }

    /// Gives the selected edges a line style.
    pub fn set_line_shape(&mut self, shape: LineShape, cx: &mut Context<Self>) {
        let edges: Vec<(Id, Route)> =
            self.selected_edges(cx).into_iter().map(|e| (e.id, scene::restyle(e, shape))).collect();
        self.update_workspace(cx, |ws| ws.set_line_shape(shape, edges));
    }

    /// The canvas's context menu: line styles for selected edges, then deleting.
    fn context_menu(
        canvas: &Entity<Self>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (current, labeled, association, use_case, compartments, member, kinds, nothing, focus) = {
            let this = canvas.read(cx);
            let ws = this.workspace.read(cx);
            let kinds = match (&ws.selection[..], ws.active_diagram) {
                (&[single], Some(diagram)) => {
                    palette::relationship_kinds(ws.doc.model(), diagram, single)
                        .map(|kinds| (single, kinds))
                }
                _ => None,
            };
            // The member right-clicked: its types and how deleting it is labeled.
            let model = ws.doc.model();
            let member = this.right_member.and_then(|m| {
                let delete = delete_member_key(model.get(m)?.kind())?;
                Some((m, member_types(model, m), delete))
            });
            // A use case, or an extend for the use case it extends.
            let use_case =
                this.selected_use_case(cx).is_some() || this.selected_extend(cx).is_some();
            let compartments = this.selected_compartments(cx).map(|(_, c)| c).unwrap_or_default();
            let current: Vec<LineShape> = this.selected_edges(cx).iter().map(|e| e.shape).collect();
            let labeled = ws.selection.iter().any(|&id| this.scene.edge(id).is_some());
            let association = match ws.selection[..] {
                [single] => this.scene.edge(single).filter(|e| !e.fixed).and_then(|edge| {
                    association_ends(ws.doc.model(), ws.active_diagram, edge, this.right_click)
                }),
                _ => None,
            };
            let nothing = ws.selection.is_empty();
            let focus = this.focus_handle.clone();
            (current, labeled, association, use_case, compartments, member, kinds, nothing, focus)
        };
        let mut menu = menu.action_context(focus);
        if let Some((member, types, delete)) = member {
            if let Some(types) = types {
                menu = menu.submenu(t!("menu.type"), window, cx, move |menu, _, _| {
                    types.iter().fold(menu, |menu, choice| match choice {
                        None => menu.separator(),
                        Some((label, type_ref, checked)) => {
                            let action = SetMemberType { member, type_ref: type_ref.clone() };
                            menu.menu_with_check(label.clone(), *checked, Box::new(action))
                        }
                    })
                });
            }
            menu = menu.menu(t!(delete), Box::new(DeleteMember { member })).separator();
        }
        if !compartments.is_empty() {
            for compartment in &compartments {
                menu = match compartment {
                    Compartment::Attributes => {
                        menu.menu(t!("menu.add_attribute"), Box::new(AddAttribute))
                    }
                    Compartment::Operations => {
                        menu.menu(t!("menu.add_operation"), Box::new(AddOperation))
                    }
                    Compartment::Literals => {
                        menu.menu(t!("menu.add_literal"), Box::new(AddLiteral))
                    }
                    Compartment::Slots => menu,
                };
            }
            menu = menu.separator();
        }
        if use_case {
            menu =
                menu.menu(t!("menu.add_extension_point"), Box::new(AddExtensionPoint)).separator();
        }
        if let Some((edge, (current, kinds))) = kinds {
            menu = menu
                .submenu(t!("menu.change_kind"), window, cx, move |menu, _, _| {
                    kinds.iter().fold(menu, |menu, &(tool, allowed)| {
                        let label = t!(format!("tool.{}", tool.key()));
                        let action = Box::new(ChangeRelationship { edge, tool });
                        if tool == current {
                            menu.menu_with_check(label, true, action)
                        } else {
                            menu.menu_with_disabled(label, action, !allowed)
                        }
                    })
                })
                .separator();
        }
        if let Some((association, ends)) = association {
            menu = association_menu(menu, association, ends, window, cx).separator();
        }
        let routed = !current.is_empty();
        if routed {
            // Checked when all selected edges have that style.
            menu = menu.submenu(t!("menu.line_style"), window, cx, move |menu, _, _| {
                LINE_SHAPES.iter().fold(menu, |menu, &(shape, key)| {
                    let checked = current.iter().all(|&s| s == shape);
                    menu.menu_with_check(t!(key), checked, Box::new(SetLineShape { shape }))
                })
            });
            menu = menu.menu(t!("menu.reset_line_ends"), Box::new(ResetLineEnds));
        }
        if labeled {
            menu = menu.menu(t!("menu.reset_labels"), Box::new(ResetLabels));
        }
        if labeled || routed {
            menu = menu.separator();
        }
        menu.menu_with_disabled(t!("menu.delete"), Box::new(DeleteSelection), nothing)
            .menu_with_disabled(
                t!("menu.remove_from_diagram"),
                Box::new(RemoveFromDiagram),
                nothing,
            )
            .separator()
            .menu(t!("menu.select_all"), Box::new(SelectAll))
    }

    fn on_middle_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_handle.focus(window, cx);
        self.drag = Some(Drag::Pan { last: ev.position });
        cx.notify();
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.to_logical(position);
        let Some(drag) = self.drag.as_mut() else { return };
        match drag {
            Drag::Move { start, dx, dy, .. } => {
                *dx = snap(p.x - start.x);
                *dy = snap(p.y - start.y);
            }
            Drag::Resize { handle, original, start, rect, .. } => {
                *rect =
                    handle.resize(*original, snap(p.x - start.x), snap(p.y - start.y), MIN_NODE);
            }
            Drag::Waypoint { current, .. } => *current = snap_point(p),
            Drag::RubberBand { current, .. }
            | Drag::Connect { current, .. }
            | Drag::Reorder { current, .. }
            | Drag::MessageLabel { current, .. }
            | Drag::KeywordLabel { current, .. }
            | Drag::HandleLine { current, .. } => *current = p,
            Drag::RegionLine { current, .. }
            | Drag::FreeLine { current, .. }
            | Drag::LineEnd { current, .. }
            | Drag::LifelineEnd { current, .. }
            | Drag::Gate { current, .. } => *current = snap_point(p),
            Drag::Pan { last } => {
                let delta = position - *last;
                *last = position;
                self.pan = self.pan.offset(f32::from(delta.x), f32::from(delta.y));
            }
        }
        cx.notify();
    }

    fn end_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let p = self.to_logical(position);
        match self.drag.take() {
            Some(Drag::Move { dx: 0.0, dy: 0.0, reselect: Some(id), .. }) => {
                self.update_workspace(cx, |ws| ws.select(vec![id]));
            }
            Some(Drag::Move { dx, dy, moved, .. }) => {
                self.update_workspace(cx, |ws| ws.move_nodes(&moved, dx, dy))
            }
            Some(Drag::Resize { presentation, original, rect, .. }) if rect != original => {
                self.update_workspace(cx, |ws| ws.set_bounds(presentation, rect));
            }
            Some(Drag::Waypoint { handle, original, current }) => {
                let route = scene::drag_handle(&original, handle, current);
                if route != original.route() {
                    self.update_workspace(cx, |ws| ws.set_route(original.id, route));
                }
            }
            Some(Drag::FreeLine { start, .. }) => {
                let end = snap_point(p);
                // A click without dragging draws nothing.
                if (end.x - start.x).abs() + (end.y - start.y).abs() > 4.0 {
                    self.update_workspace(cx, |ws| {
                        ws.create_free_line(start, end);
                        ws.tool = Tool::Select;
                    });
                }
            }
            Some(Drag::LineEnd { line, end, current }) => {
                self.update_workspace(cx, |ws| ws.set_free_line_end(line, end, current));
            }
            Some(Drag::LifelineEnd { presentation, current, .. }) => {
                self.update_workspace(cx, |ws| ws.set_lifeline_end(presentation, current.y));
            }
            Some(Drag::Gate { message, current, .. }) => {
                self.update_workspace(cx, |ws| ws.set_gate(message, current.x));
            }
            Some(Drag::RubberBand { start, extend, .. }) => {
                let area = Rect::from_corners(start, p);
                let mut hits = scene::nodes_in(&self.scene, area);
                // Free lines count like nodes when both ends lie inside.
                let lines: Vec<Id> = self
                    .scene
                    .edges
                    .iter()
                    .filter(|e| e.points.iter().all(|&q| area.contains(q)))
                    .filter(|e| self.is_free_line(e.id, cx))
                    .map(|e| e.id)
                    .collect();
                hits.extend(lines);
                self.update_workspace(cx, |ws| {
                    if extend {
                        let mut selection = ws.selection.clone();
                        selection.extend(hits.into_iter().filter(|h| !ws.selection.contains(h)));
                        ws.select(selection);
                    } else {
                        ws.select(hits);
                    }
                });
            }
            Some(Drag::Connect { source, start, current }) => {
                let tool = self.workspace.read(cx).tool;
                let target = match tool {
                    Tool::Edge(edge_tool) => self.connect_target(edge_tool, p, cx),
                    _ => None,
                };
                match (tool, target) {
                    (Tool::Edge(edge_tool), Some(target)) if target != source => {
                        self.finish_connect(edge_tool, source, target, start, cx);
                    }
                    // Released on the source: wait for the second click.
                    (Tool::Edge(_), Some(_)) => {
                        self.drag = Some(Drag::Connect { source, start, current });
                    }
                    _ => {}
                }
            }
            Some(Drag::HandleLine { source, start, current, message }) => {
                let tool = if message { EdgeTool::Message } else { EdgeTool::Association };
                match self.connect_target(tool, p, cx) {
                    Some(target) if target != source => {
                        self.offer_connections(source, target, start, position, cx);
                    }
                    // Released on its own element, or clicked without dragging: the next click
                    // picks the target, as with the tools.
                    Some(_) => {
                        self.drag = Some(Drag::HandleLine { source, start, current, message });
                    }
                    None if start.distance(p) * self.zoom < CLICK_SLOP => {
                        self.drag = Some(Drag::HandleLine { source, start, current, message });
                    }
                    None => {}
                }
            }
            Some(Drag::Reorder { element, start, current })
                if (current.y - start.y).abs() > 6.0 =>
            {
                if let Some(slot) = self.scene.sequence.as_ref().and_then(|l| l.slot_at(current.y))
                {
                    self.update_workspace(cx, |ws| ws.move_to_slot(element, slot));
                }
            }
            Some(Drag::MessageLabel { message, origin, start, current })
                if start.distance(current) > 2.0 =>
            {
                let to = origin.offset(current.x - start.x, current.y - start.y);
                self.update_workspace(cx, |ws| ws.place_message_label(message, to));
            }
            Some(Drag::KeywordLabel { edge, origin, middle, start, current })
                if start.distance(current) > 2.0 =>
            {
                let offset = MPoint::new(
                    origin.x + current.x - start.x - middle.x,
                    origin.y + current.y - start.y - middle.y,
                );
                self.update_workspace(cx, |ws| ws.place_keyword_label(edge, offset));
            }
            Some(Drag::RegionLine { presentation, index, state, vertical, current }) => {
                let offset = if vertical { current.x - state.x } else { current.y - state.y };
                self.update_workspace(cx, |ws| ws.move_region_line(presentation, index, offset));
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.hover = None;
        let delta = ev.delta.pixel_delta(px(16.0));
        let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
        if ev.modifiers.control {
            self.zoom_at(ev.position, (1.0 + dy * 0.002).clamp(0.5, 2.0));
        } else if ev.modifiers.shift {
            self.pan = self.pan.offset(dy, dx);
        } else {
            self.pan = self.pan.offset(dx, dy);
        }
        cx.notify();
    }

    fn on_drop(&mut self, dragged: &DraggedElement, window: &mut Window, cx: &mut Context<Self>) {
        let at = snap_point(self.to_logical(window.mouse_position()));
        let id = dragged.id;
        self.update_workspace(cx, |ws| ws.show_element(id, at));
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        self.editing = None;
        self.offer = None;
        self.update_workspace(cx, |ws| ws.tool = Tool::Select);
    }

    fn update_workspace(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Workspace)) {
        self.workspace.update(cx, |ws, cx| {
            f(ws);
            cx.notify();
        });
        cx.notify();
    }

    // ----- In-place editing ------------------------------------------------------------------

    /// Starts editing the name or member row under `p`. Returns whether there was one.
    fn start_edit_at(&mut self, p: MPoint, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let row = scene::row_at(&self.scene, p);
        let mut slot_of = None;
        let (presentation, element, role, rect) = match row {
            Some((node, _)) if matches!(node.figure, Figure::Text) => {
                let text = self.free_text(node.id, cx).unwrap_or_default();
                let rect = node.rect;
                self.workspace.update(cx, |ws, cx| {
                    ws.select(vec![node.id]);
                    cx.notify();
                });
                self.open_editor(node.id, None, true, rect, text, true, window, cx);
                return true;
            }
            Some((node, row)) => {
                let Some(element) = row.element else { return false };
                let rect = if row.role == RowRole::Name && matches!(node.figure, Figure::Note) {
                    node.rect.inflate(-4.0)
                } else {
                    row.rect
                };
                if row.role == RowRole::Slot {
                    slot_of = node.element;
                }
                (node.id, element, row.role, rect)
            }
            // A message of a sequence diagram: edit its label.
            None => {
                let Some(id) = scene::hit_test(&self.scene, p, self.hit_tolerance()) else {
                    return false;
                };
                let Some(edge) = self.scene.edge(id).filter(|e| e.fixed) else { return false };
                let Some(element) = self.reorderable(id, cx) else { return false };
                let (a, b) = (edge.points[0], edge.points[1]);
                let x = if edge.points.len() > 2 { a.x } else { (a.x + b.x) / 2.0 - 70.0 };
                (id, element, RowRole::Name, Rect::new(x, a.y - 20.0, 140.0, 16.0))
            }
        };
        let model = self.workspace.read(cx).doc.model();
        let Some(e) = model.get(element) else { return false };
        let (text, multi_line) = match e.kind() {
            ElementKind::Comment(c) => (c.body.clone(), true),
            ElementKind::InteractionOperand(o) => (o.guard.clone(), false),
            ElementKind::Message(m) if m.sort == umlcraft_model::MessageSort::Reply => {
                (m.return_value.clone(), false)
            }
            ElementKind::Message(m) if m.arguments.is_empty() => (e.name().to_owned(), false),
            ElementKind::Message(m) => (format!("{}({})", e.name(), m.arguments), false),
            _ if role == RowRole::Member => (member_text(model, element), false),
            // A slot shows as `name = value`; only the value is taken back.
            _ if role == RowRole::Slot => {
                let value = slot_of
                    .and_then(|o| object::slots(model, o).into_iter().find(|(a, _)| *a == element))
                    .map(|(_, v)| v)
                    .unwrap_or_default();
                (format!("{} = {value}", e.name()), false)
            }
            _ => (e.name().to_owned(), false),
        };
        let node_id = presentation;
        self.workspace.update(cx, |ws, cx| {
            ws.select(vec![node_id]);
            ws.selected_element = Some(element);
            cx.notify();
        });
        self.open_editor(element, slot_of, false, rect, text, multi_line, window, cx);
        true
    }

    /// The text of the free text `presentation` on the active diagram.
    fn free_text(&self, presentation: Id, cx: &App) -> Option<String> {
        let ws = self.workspace.read(cx);
        match &ws.doc.model().view(ws.active_diagram?)?.get(presentation)?.shape {
            umlcraft_model::Shape::Text { text, .. } => Some(text.clone()),
            _ => None,
        }
    }

    /// Opens the in-place editor for `element` over `rect`, holding `text`.
    #[allow(clippy::too_many_arguments)]
    fn open_editor(
        &mut self,
        element: Id,
        slot_of: Option<Id>,
        free_text: bool,
        rect: Rect,
        text: String,
        multi_line: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Focus once the editor is on screen; the click itself still focuses the canvas. The
        // default name of something just made is selected, so typing replaces it.
        let original = text.clone();
        let select = std::mem::take(&mut self.select_when_drawn);
        let editor = if multi_line {
            let state = cx.new(|cx| TextareaState::new(window, cx).default_value(text));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| {
                focus.update(cx, |s, cx| {
                    s.focus(window, cx);
                    if select {
                        s.select_all(window, cx);
                    }
                });
            });
            let subscription =
                cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::Blur) {
                        this.commit_edit(cx);
                    }
                });
            (Editor::Area(state), subscription)
        } else {
            let state = cx.new(|cx| InputState::new(window, cx).default_value(text));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| {
                focus.update(cx, |s, cx| {
                    s.focus(window, cx);
                    if select {
                        s.select_all(window, cx);
                    }
                });
            });
            let subscription =
                cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| match ev {
                    InputEvent::PressEnter { .. } => this.finish_edit(true, cx),
                    InputEvent::Blur => this.commit_edit(cx),
                    _ => {}
                });
            (Editor::Line(state), subscription)
        };
        self.editing = Some(InlineEdit {
            element,
            slot_of,
            free_text,
            rect,
            original,
            new_member: None,
            end: None,
            editor: editor.0,
            _subscription: editor.1,
        });
        cx.notify();
    }

    /// Opens an editor for the role or multiplicity of the association end `hit` names.
    fn open_end_editor(&mut self, hit: EndLabelHit, window: &mut Window, cx: &mut Context<Self>) {
        let ws = self.workspace.read(cx);
        let model = ws.doc.model();
        let Some(view) = ws.active_diagram.and_then(|d| model.view(d)) else { return };
        let Some(association) = view.get(hit.edge).and_then(|p| p.element) else { return };
        let Some(ElementKind::Association(p)) = model.get(association).map(|e| e.kind()) else {
            return;
        };
        let Some(end) = p.ends.get(hit.end) else { return };
        let text = if hit.multiplicity {
            end.multiplicity.map(|m| m.to_string()).unwrap_or_default()
        } else {
            member::role_text(end)
        };
        self.workspace.update(cx, |ws, cx| {
            ws.select(vec![hit.edge]);
            ws.selected_element = Some(association);
            cx.notify();
        });
        let rect = Rect::new(hit.rect.x, hit.rect.y, hit.rect.width.max(80.0), hit.rect.height);
        self.open_editor(association, None, false, rect, text, false, window, cx);
        if let Some(edit) = &mut self.editing {
            edit.end = Some((hit.end, hit.multiplicity));
        }
    }

    /// Opens an empty row where `hit` says a new member goes; what is typed there becomes the
    /// member on committing, nothing if the row stays empty.
    fn open_new_member(
        &mut self,
        hit: CompartmentHit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.update(cx, |ws, cx| {
            ws.select(vec![hit.node]);
            cx.notify();
        });
        self.open_editor(hit.element, None, false, hit.new_row, String::new(), false, window, cx);
        if let Some(edit) = &mut self.editing {
            edit.new_member = Some(NewMember {
                node: hit.node,
                owner: hit.element,
                compartment: hit.compartment,
                after: hit.last,
            });
        }
    }

    /// Writes the in-place edit back, if one is open.
    pub fn commit_edit(&mut self, cx: &mut Context<Self>) {
        self.finish_edit(false, cx);
    }

    /// Writes the in-place edit back; with `enter`, a new member is followed by an empty row
    /// for the next one, as long as rows are filled in.
    fn finish_edit(&mut self, enter: bool, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        let text = match &edit.editor {
            Editor::Line(s) => s.read(cx).value().to_string(),
            Editor::Area(s) => s.read(cx).value().to_string(),
        };
        if let Some((end, multiplicity)) = edit.end {
            if text != edit.original {
                self.update_workspace(cx, |ws| {
                    ws.set_end_text(edit.element, end, multiplicity, &text)
                });
            }
            cx.notify();
            return;
        }
        if let Some(new) = edit.new_member {
            let added = self.workspace.update(cx, |ws, cx| {
                let added = ws.add_member(new.owner, new.compartment, new.after, &text);
                cx.notify();
                added
            });
            if enter && added.is_some() {
                self.new_member_when_drawn = Some((new.node, new.compartment));
            }
            cx.notify();
            return;
        }
        if text == edit.original {
            cx.notify();
            return;
        }
        let element = edit.element;
        match edit.slot_of {
            _ if edit.free_text => self.update_workspace(cx, |ws| ws.set_free_text(element, &text)),
            Some(object) => self.update_workspace(cx, |ws| ws.set_slot(object, element, &text)),
            None => self.update_workspace(cx, |ws| ws.apply_text(element, &text)),
        }
    }
}

impl Focusable for DiagramCanvas {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Logical → screen mapping for one frame.
#[derive(Clone, Copy)]
struct Transform {
    origin: Point<Pixels>,
    pan: MPoint,
    zoom: f32,
}

impl Transform {
    fn pt(&self, p: MPoint) -> Point<Pixels> {
        self.origin + point(px(self.pan.x + p.x * self.zoom), px(self.pan.y + p.y * self.zoom))
    }

    fn len(&self, v: f32) -> Pixels {
        px(v * self.zoom)
    }

    fn rect(&self, r: Rect) -> Bounds<Pixels> {
        Bounds::new(self.pt(r.origin()), size(self.len(r.width), self.len(r.height)))
    }

    fn to_logical(self, p: Point<Pixels>) -> MPoint {
        MPoint::new(
            (f32::from(p.x - self.origin.x) - self.pan.x) / self.zoom,
            (f32::from(p.y - self.origin.y) - self.pan.y) / self.zoom,
        )
    }
}

#[derive(Clone, Copy)]
struct Colors {
    background: Hsla,
    grid: Hsla,
    fill: Hsla,
    line: Hsla,
    text: Hsla,
    muted: Hsla,
    selection: Hsla,
}

struct Painter<'a> {
    t: Transform,
    c: Colors,
    font_family: SharedString,
    window: &'a mut Window,
    cx: &'a mut App,
    /// The fill color chosen for the node being painted; only on a light background, where the
    /// texts stay readable on it.
    node_fill: Option<Hsla>,
    /// Members show their visibility in UmlCraft's own column.
    column: bool,
}

impl Painter<'_> {
    fn line(&mut self, points: &[MPoint], width: f32, color: Hsla, style: LineStyle) {
        if points.len() < 2 {
            return;
        }
        let mut path = PathBuilder::stroke(self.t.len(width));
        let dashes: Vec<Pixels> = style.dashes().iter().map(|&d| px(d * self.t.zoom)).collect();
        if !dashes.is_empty() {
            path = path.dash_array(&dashes);
        }
        path.move_to(self.t.pt(points[0]));
        for &p in &points[1..] {
            path.line_to(self.t.pt(p));
        }
        if let Ok(path) = path.build() {
            self.window.paint_path(path, color);
        }
    }

    fn polygon(&mut self, points: &[MPoint], fill: Hsla, stroke: Hsla) {
        let screen: Vec<_> = points.iter().map(|&p| self.t.pt(p)).collect();
        let mut path = PathBuilder::fill();
        path.add_polygon(&screen, true);
        if let Ok(path) = path.build() {
            self.window.paint_path(path, fill);
        }
        let mut path = PathBuilder::stroke(self.t.len(1.0));
        path.add_polygon(&screen, true);
        if let Ok(path) = path.build() {
            self.window.paint_path(path, stroke);
        }
    }

    fn ellipse(&mut self, center: MPoint, rx: f32, ry: f32, fill: Hsla, stroke: Hsla, width: f32) {
        let build = |mut path: PathBuilder, t: &Transform| {
            let radii = point(t.len(rx), t.len(ry));
            path.move_to(t.pt(center.offset(rx, 0.0)));
            path.arc_to(radii, px(0.0), false, true, t.pt(center.offset(-rx, 0.0)));
            path.arc_to(radii, px(0.0), false, true, t.pt(center.offset(rx, 0.0)));
            path.close();
            path.build()
        };
        if let Ok(path) = build(PathBuilder::fill(), &self.t) {
            self.window.paint_path(path, fill);
        }
        if let Ok(path) = build(PathBuilder::stroke(self.t.len(width)), &self.t) {
            self.window.paint_path(path, stroke);
        }
    }

    fn paint(&self, paint: Paint) -> Hsla {
        let c = self.c;
        match paint {
            Paint::Fill => self.node_fill.unwrap_or(c.fill),
            Paint::Line => c.line,
            Paint::Background => c.background,
            Paint::Translucent => Hsla { a: 0.35, ..c.fill },
            Paint::Muted => c.muted,
            Paint::None => transparent_black(),
        }
    }

    fn primitive(&mut self, primitive: &Primitive) {
        let line = self.c.line;
        match primitive {
            Primitive::Polyline { points, width, style } => self.line(points, *width, line, *style),
            Primitive::Curve { points, width, style } => {
                self.line(&route::flatten(points), *width, line, *style)
            }
            Primitive::Polygon { points, fill } => self.polygon(points, self.paint(*fill), line),
            Primitive::Ellipse { center, rx, ry, fill, width } => {
                self.ellipse(*center, *rx, *ry, self.paint(*fill), line, *width)
            }
            Primitive::Frame { rect, width, radius, fill, border, style } => {
                let style = match style {
                    LineStyle::Solid => BorderStyle::Solid,
                    _ => BorderStyle::Dashed,
                };
                let (fill, border) = (self.paint(*fill), self.paint(*border));
                self.frame(*rect, *width, *radius, fill, border, style);
            }
        }
    }

    /// A rectangle with a border of `width`, centered on its outline like a Java stroke, and
    /// corners rounded by `radius`.
    fn frame(
        &mut self,
        r: Rect,
        width: f32,
        radius: f32,
        fill: Hsla,
        border: Hsla,
        style: BorderStyle,
    ) {
        let outer = self.t.rect(r.inflate(width / 2.0));
        let corners = if radius > 0.0 { self.t.len(radius + width / 2.0) } else { px(0.0) };
        self.window.paint_quad(quad(outer, corners, fill, self.t.len(width), border, style));
    }

    /// Draws one line of text inside `area` and returns the logical text width.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        text: &str,
        area: Rect,
        font_size: f32,
        bold: bool,
        italic: bool,
        color: Hsla,
        align: LabelAlign,
    ) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let mut f = font(self.font_family.clone());
        if bold {
            f = f.bold();
        }
        if italic {
            f = f.italic();
        }
        let text: SharedString = text.to_owned().into();
        let run = TextRun { len: text.len(), font: f, color, ..Default::default() };
        let line = self.window.text_system().shape_line(text, self.t.len(font_size), &[run], None);
        let line_height = self.t.len(scene::line_height(font_size));
        let measured = f32::from(line.width()) / self.t.zoom;
        let (align, width) = match align {
            LabelAlign::Left => (TextAlign::Left, None),
            LabelAlign::Center => (TextAlign::Center, Some(self.t.len(area.width))),
            LabelAlign::Right => (TextAlign::Right, Some(self.t.len(area.width))),
        };
        let shift = umlcraft_notation::draw::baseline(font_size) - font_size * GPUI_BASELINE;
        let origin = area.origin().offset(0.0, shift);
        line.paint(self.t.pt(origin), line_height, align, width, self.window, self.cx).ok();
        measured
    }

    fn row(&mut self, row: &Row) {
        let color = self.c.text;
        let column = scene::visibility_column(&row.text).filter(|_| {
            self.column && row.role == RowRole::Member && row.align == LabelAlign::Left
        });
        let width = match column {
            // The visibility symbol centered in a column as wide as a "+", the rest after it.
            Some((symbol, rest)) => {
                let column = Display { visibility_column: true, ..Display::default() };
                let slot = AppMeasure::new(self.cx, column).width("+", row.size, row.bold);
                let r = row.rect;
                let cell = Rect::new(r.x, r.y, slot, r.height);
                self.text(symbol, cell, row.size, row.bold, row.italic, color, LabelAlign::Center);
                let after = Rect::new(r.x + slot, r.y, r.width - slot, r.height);
                slot + self.text(
                    rest,
                    after,
                    row.size,
                    row.bold,
                    row.italic,
                    color,
                    LabelAlign::Left,
                )
            }
            None => {
                self.text(&row.text, row.rect, row.size, row.bold, row.italic, color, row.align)
            }
        };
        if row.underline {
            let x = draw::text_left(row.rect, width, row.align);
            let y = row.rect.y + row.size * draw::UNDERLINE;
            self.line(
                &[MPoint::new(x, y), MPoint::new(x + width, y)],
                1.0,
                color,
                LineStyle::Solid,
            );
        }
    }

    /// The grid, kept inside the corners rounded by `corner`.
    fn grid(&mut self, bounds: Bounds<Pixels>, corner: f32) {
        let step = GRID * self.t.zoom;
        if step < 8.0 {
            return;
        }
        // How far a line `inset` from an edge must stay off the perpendicular edge to keep
        // inside the corner's arc.
        let cut = |inset: f32| {
            if inset >= corner {
                0.0
            } else {
                let d = corner - inset.max(0.0);
                corner - (corner * corner - d * d).sqrt()
            }
        };
        let (left, right) = (f32::from(bounds.origin.x), f32::from(bounds.right()));
        let (top, bottom) = (f32::from(bounds.origin.y), f32::from(bounds.bottom()));
        let top_left = self.t.to_logical(bounds.origin);
        let bottom_right = self.t.to_logical(bounds.bottom_right());
        let mut x = (top_left.x / GRID).floor() * GRID;
        while x <= bottom_right.x {
            let sx = f32::from(self.t.pt(MPoint::new(x, 0.0)).x);
            let shorten = cut((sx - left).min(right - sx - 1.0));
            self.window.paint_quad(fill(
                Bounds::new(
                    point(px(sx), px(top + shorten)),
                    size(px(1.0), px(bottom - top - 2.0 * shorten)),
                ),
                self.c.grid,
            ));
            x += GRID;
        }
        let mut y = (top_left.y / GRID).floor() * GRID;
        while y <= bottom_right.y {
            let sy = f32::from(self.t.pt(MPoint::new(0.0, y)).y);
            let shorten = cut((sy - top).min(bottom - sy - 1.0));
            self.window.paint_quad(fill(
                Bounds::new(
                    point(px(left + shorten), px(sy)),
                    size(px(right - left - 2.0 * shorten), px(1.0)),
                ),
                self.c.grid,
            ));
            y += GRID;
        }
    }

    /// Paints a node's outline and its rows, except the row being edited in place.
    fn node(&mut self, node: &NodeShape, editing: Option<Id>) {
        let light = self.c.background.l > 0.5;
        self.node_fill = node.fill.filter(|_| light).map(|c| {
            Hsla::from(Rgba {
                r: f32::from(c.r) / 255.0,
                g: f32::from(c.g) / 255.0,
                b: f32::from(c.b) / 255.0,
                a: f32::from(c.a) / 255.0,
            })
        });
        for primitive in draw::figure(node) {
            self.primitive(&primitive);
        }
        self.node_fill = None;
        for row in &node.rows {
            if editing.is_some() && row.element == editing {
                continue;
            }
            self.row(row);
        }
    }

    fn edge(&mut self, edge: &EdgeShape, selected: bool) {
        let color = if selected { self.c.selection } else { self.c.line };
        self.line(&edge.path(), if selected { 2.0 } else { 1.0 }, color, edge.line);
        for primitive in draw::markers(edge) {
            self.primitive(&primitive);
        }
        let text = self.c.text;
        for label in &edge.labels {
            let area = draw::label_area(label);
            let width = self.text(&label.text, area, FONT_SIZE, false, false, text, label.align);
            if label.underline {
                let x = draw::text_left(area, width, label.align);
                let y = area.y + FONT_SIZE * draw::UNDERLINE;
                let points = [MPoint::new(x, y), MPoint::new(x + width, y)];
                self.line(&points, 1.0, text, LineStyle::Solid);
            }
        }
        for (rect, rows) in &edge.qualifiers {
            self.primitive(&draw::qualifier_box(*rect));
            for row in rows {
                self.row(row);
            }
        }
    }

    fn edge_handles(&mut self, edge: &EdgeShape) {
        let s = self.c.selection;
        let half = 3.5 / self.t.zoom;
        for (handle, p) in edge.handles() {
            let square = Rect::new(p.x - half, p.y - half, 2.0 * half, 2.0 * half);
            let fill = match handle {
                EdgeHandle::Waypoint(_) | EdgeHandle::Segment(_) => s,
                EdgeHandle::Insert(_) => self.c.background,
            };
            self.window.paint_quad(quad(
                self.t.rect(square),
                px(1.0),
                fill,
                px(1.0),
                s,
                BorderStyle::Solid,
            ));
        }
    }

    /// Filled square grips at `points`.
    fn grips(&mut self, points: &[MPoint]) {
        let s = self.c.selection;
        let half = 3.5 / self.t.zoom;
        for p in points {
            let square = Rect::new(p.x - half, p.y - half, 2.0 * half, 2.0 * half);
            self.window.paint_quad(quad(
                self.t.rect(square),
                px(1.0),
                s,
                px(1.0),
                s,
                BorderStyle::Solid,
            ));
        }
    }

    fn selection(&mut self, r: Rect, handles: bool) {
        let s = self.c.selection;
        self.window.paint_quad(quad(
            self.t.rect(r.inflate(3.0)),
            px(2.0),
            transparent_black(),
            px(1.5),
            s,
            BorderStyle::Solid,
        ));
        if handles {
            let half = 3.5 / self.t.zoom;
            for handle in Handle::ALL {
                let p = handle.position(r);
                let square = Rect::new(p.x - half, p.y - half, 2.0 * half, 2.0 * half);
                self.window.paint_quad(quad(
                    self.t.rect(square),
                    px(1.0),
                    self.c.background,
                    px(1.5),
                    s,
                    BorderStyle::Solid,
                ));
            }
        }
    }

    /// Activation bars and the crosses of destroyed lifelines.
    fn sequence(&mut self, layout: &SequenceLayout) {
        for primitive in draw::sequence(layout) {
            self.primitive(&primitive);
        }
    }

    /// Where a dragged message or fragment would land.
    fn insertion_line(&mut self, layout: &SequenceLayout, y: f32) {
        let line = [MPoint::new(layout.left - 20.0, y), MPoint::new(layout.right + 20.0, y)];
        let s = self.c.selection;
        self.line(&line, 2.0, s, LineStyle::Solid);
    }

    fn rubber_band(&mut self, area: Rect) {
        let s = self.c.selection;
        self.window.paint_quad(quad(
            self.t.rect(area),
            px(0.0),
            Hsla { a: 0.08, ..s },
            px(1.0),
            s,
            BorderStyle::Dashed,
        ));
    }
}

impl Render for DiagramCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (active, tool) = {
            let ws = self.workspace.read(cx);
            (ws.active_diagram, ws.tool)
        };
        self.switch_viewport(active);
        let font_family = diagram_font();
        let key = {
            let ws = self.workspace.read(cx);
            ws.active_diagram
                .map(|d| (ws.doc.revision(), d, self.display, rust_i18n::locale().to_string()))
        };
        // While dragging, the preview changes the scene on every mouse move.
        let reuse = self.drag.is_none() && key.is_some() && key == self.scene_key;
        let scene = if reuse {
            self.scene.clone()
        } else {
            let ws = self.workspace.read(cx);
            let measure = FontMeasure::new(window, self.display);
            let scene = match ws.active_diagram {
                Some(d) => scene::build(ws.doc.model(), d, &self.preview(), &measure),
                None => Scene::empty(),
            };
            Rc::new(scene)
        };
        // A scene with a drag preview is not the plain one of this key.
        self.scene_key = if self.drag.is_none() { key } else { None };
        self.scene = scene.clone();
        // After a member typed into an empty row, the next empty row opens below it.
        if let Some((node, compartment)) = self.new_member_when_drawn.take()
            && let Some(hit) = scene::compartment_of(&scene, node, compartment)
        {
            cx.defer_in(window, move |this, window, cx| this.open_new_member(hit, window, cx));
        }
        // A row just added from the context menu is edited as soon as it is drawn.
        if let Some(element) = self.edit_when_drawn.take() {
            let row = scene.nodes.iter().flat_map(|n| &n.rows).find(|r| r.element == Some(element));
            let text = scene.node(element).filter(|n| matches!(n.figure, Figure::Text));
            if let Some(at) = row.map(|r| r.rect.center()).or(text.map(|n| n.rect.center())) {
                cx.defer_in(window, move |this, window, cx| {
                    this.select_when_drawn = true;
                    this.start_edit_at(at, window, cx);
                    this.select_when_drawn = false;
                });
            }
        }
        let selection = self.workspace.read(cx).selection.clone();
        // A single selected free line shows grips at its ends, a lifeline one at its end.
        let line_ends = match selection[..] {
            [single] if self.is_free_line(single, cx) => {
                scene.edge(single).map(|e| e.points.clone()).unwrap_or_default()
            }
            [single] => {
                let end = scene.node(single).and_then(lifeline_end);
                let end = end.map(|(x, _, bottom)| MPoint::new(x, bottom));
                // The open end of a found or lost message can be dragged.
                let gate = self.gate_of(single, cx).map(|(gate, _)| gate);
                end.into_iter().chain(gate).collect()
            }
            _ => Vec::new(),
        };
        let theme = cx.theme();
        // In the light theme diagrams look like Astah's: black on white.
        let (fill, ink) = if theme.is_dark() {
            (theme.secondary, theme.foreground)
        } else {
            (gpui_kit::white(), gpui_kit::black())
        };
        let colors = Colors {
            background: theme.background,
            grid: Hsla { a: if theme.is_dark() { 0.25 } else { 0.45 }, ..theme.border },
            fill,
            line: ink,
            text: ink,
            muted: theme.muted_foreground,
            selection: theme.blue,
        };
        let (pan, zoom, drag) = (self.pan, self.zoom, self.drag.clone());
        let column = self.display.visibility_column;
        let corner = self.corner_radius;
        // The row under the editor is hidden; a new member's empty row covers none.
        let editing_element =
            self.editing.as_ref().filter(|e| e.new_member.is_none()).map(|e| e.element);
        let bounds_cell = self.bounds.clone();
        let this = cx.entity();
        let menu_owner = this.clone();
        let cursor = if tool == Tool::Select { CursorStyle::Arrow } else { CursorStyle::Crosshair };

        let handles = self.hover_handles(cx).filter(|_| tool == Tool::Select);
        let offer_menu = self.offer_menu(window, cx);

        // The in-place editor sits over the edited text, in canvas-local pixels.
        let editor = self.editing.as_ref().map(|edit| {
            let r = edit.rect;
            let (x, y) = (pan.x + r.x * zoom, pan.y + r.y * zoom);
            let width = (r.width * zoom).max(120.0);
            let field = match &edit.editor {
                Editor::Line(state) => Input::new(state).xsmall().into_any_element(),
                Editor::Area(state) => Textarea::new(state).xsmall().into_any_element(),
            };
            div().absolute().left(px(x - 4.0)).top(px(y - 4.0)).w(px(width + 8.0)).child(field)
        });

        div()
            .id("diagram-canvas")
            .key_context("Canvas")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(colors.background)
            .rounded(px(corner))
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            // Window-wide listeners (see the paint closure) follow drags outside the canvas,
            // but only exist from the next frame on; these catch very quick drags.
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    this.drag_to(ev.position, cx);
                } else {
                    this.update_hover(ev.position, cx);
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.hover.take().is_some() {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| this.end_drag(ev.position, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| this.end_drag(ev.position, cx)),
            )
            .on_scroll(cx.listener(Self::on_scroll))
            .on_drop(cx.listener(Self::on_drop))
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(|this, _: &DeleteSelection, _, cx| {
                this.update_workspace(cx, Workspace::delete_selection);
            }))
            .on_action(cx.listener(|this, _: &RemoveFromDiagram, _, cx| {
                this.update_workspace(cx, Workspace::remove_selection_from_diagram);
            }))
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.update_workspace(cx, Workspace::select_all);
            }))
            .on_action(
                cx.listener(|this, a: &SetLineShape, _, cx| this.set_line_shape(a.shape, cx)),
            )
            .on_action(cx.listener(|this, _: &ResetLineEnds, _, cx| {
                let edges: Vec<Id> = this.selected_edges(cx).iter().map(|e| e.id).collect();
                this.update_workspace(cx, |ws| ws.reset_line_ends(&edges));
            }))
            .on_action(cx.listener(|this, _: &AddAttribute, window, cx| {
                this.add_member_row(Compartment::Attributes, window, cx)
            }))
            .on_action(cx.listener(|this, _: &AddOperation, window, cx| {
                this.add_member_row(Compartment::Operations, window, cx)
            }))
            .on_action(cx.listener(|this, _: &AddLiteral, window, cx| {
                this.add_member_row(Compartment::Literals, window, cx)
            }))
            .on_action(cx.listener(|this, _: &AddExtensionPoint, _, cx| {
                let point = match (this.selected_use_case(cx), this.selected_extend(cx)) {
                    (Some(use_case), _) => this
                        .workspace
                        .update(cx, |ws, _| ws.add_item(use_case, NewItem::ExtensionPoint)),
                    (_, Some(extend)) => {
                        this.workspace.update(cx, |ws, _| ws.add_extension_point_for(extend))
                    }
                    _ => return,
                };
                this.edit_when_drawn = point;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ResetLabels, _, cx| {
                let selection = this.workspace.read(cx).selection.clone();
                let edges: Vec<Id> =
                    selection.into_iter().filter(|&id| this.scene.edge(id).is_some()).collect();
                this.update_workspace(cx, |ws| ws.reset_labels(&edges));
            }))
            .on_action(cx.listener(|this, a: &ConnectAs, _, cx| {
                this.offer = None;
                this.finish_connect(a.tool, a.source, a.target, a.start, cx);
            }))
            .on_action(cx.listener(|this, a: &ChangeRelationship, _, cx| {
                this.update_workspace(cx, |ws| ws.change_relationship(a.edge, a.tool));
            }))
            .on_action(cx.listener(|this, a: &DeleteMember, _, cx| {
                this.update_workspace(cx, |ws| ws.delete_element(a.member));
            }))
            .on_action(cx.listener(|this, a: &SetMemberType, _, cx| {
                this.update_workspace(cx, |ws| ws.set_member_type(a.member, a.type_ref.clone()));
            }))
            .on_action(cx.listener(|this, a: &SetAssociationEnd, _, cx| {
                this.update_workspace(cx, |ws| {
                    ws.set_association_end(a.association, a.end, a.change)
                });
            }))
            .on_action(cx.listener(|this, _: &ResetZoom, _, cx| this.reset_zoom(cx)))
            .on_action(cx.listener(|this, _: &ZoomToFit, _, cx| this.zoom_to_fit(cx)))
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(bounds),
                    move |bounds, (), window, cx| {
                        if drag.is_some() {
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble {
                                    target.update(cx, |c, cx| c.drag_to(ev.position, cx));
                                }
                            });
                            let target = this.clone();
                            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                                if phase == DispatchPhase::Bubble
                                    && matches!(ev.button, MouseButton::Left | MouseButton::Middle)
                                {
                                    target.update(cx, |c, cx| c.end_drag(ev.position, cx));
                                }
                            });
                        }
                        let t = Transform { origin: bounds.origin, pan, zoom };
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            let mut p = Painter {
                                t,
                                c: colors,
                                font_family,
                                window,
                                cx,
                                node_fill: None,
                                column,
                            };
                            p.grid(bounds, corner);
                            for node in &scene.nodes {
                                p.node(node, editing_element);
                            }
                            if let Some(layout) = &scene.sequence {
                                p.sequence(layout);
                            }
                            for edge in &scene.edges {
                                p.edge(edge, selection.contains(&edge.id));
                            }
                            let selected_nodes: Vec<&NodeShape> =
                                scene.nodes.iter().filter(|n| selection.contains(&n.id)).collect();
                            let single = selection.len() == 1;
                            for node in &selected_nodes {
                                p.selection(node.rect, single && node.resizable());
                            }
                            if single && let Some(edge) = scene.edge(selection[0]) {
                                p.edge_handles(edge);
                            }
                            p.grips(&line_ends);
                            match &drag {
                                Some(Drag::RubberBand { start, current, .. }) => {
                                    p.rubber_band(Rect::from_corners(*start, *current));
                                }
                                Some(Drag::Reorder { start, current, .. })
                                    if (current.y - start.y).abs() > 6.0 =>
                                {
                                    let layout = scene.sequence.as_ref();
                                    if let Some((layout, slot)) =
                                        layout.and_then(|l| l.slot_at(current.y).map(|s| (l, s)))
                                    {
                                        p.insertion_line(layout, slot.y);
                                    }
                                }
                                Some(
                                    Drag::MessageLabel { message: edge, start, current, .. }
                                    | Drag::KeywordLabel { edge, start, current, .. },
                                ) => {
                                    // The labels where they will go: a message's together, an
                                    // edge's «include» or «extend» alone.
                                    let keyword = matches!(&drag, Some(Drag::KeywordLabel { .. }));
                                    let (dx, dy) = (current.x - start.x, current.y - start.y);
                                    let s = p.c.selection;
                                    let labels = scene.edge(*edge).into_iter().flat_map(|e| {
                                        e.labels.iter().filter(|l| l.keyword || !keyword)
                                    });
                                    for label in labels {
                                        let moved = scene::EdgeLabel {
                                            anchor: label.anchor.offset(dx, dy),
                                            ..label.clone()
                                        };
                                        let area = draw::label_area(&moved);
                                        p.text(
                                            &moved.text,
                                            area,
                                            FONT_SIZE,
                                            false,
                                            false,
                                            s,
                                            moved.align,
                                        );
                                    }
                                }
                                Some(Drag::RegionLine { state, vertical, current, .. }) => {
                                    // The line where it will go.
                                    let (a, b) = if *vertical {
                                        let x = current.x.clamp(state.x, state.right());
                                        (MPoint::new(x, state.y), MPoint::new(x, state.bottom()))
                                    } else {
                                        let y = current.y.clamp(state.y, state.bottom());
                                        (MPoint::new(state.x, y), MPoint::new(state.right(), y))
                                    };
                                    let s = p.c.selection;
                                    p.line(&[a, b], 1.5, s, LineStyle::Dashed);
                                }
                                Some(Drag::FreeLine { start, current }) => {
                                    let s = p.c.selection;
                                    p.line(&[*start, *current], 1.5, s, LineStyle::Solid);
                                }
                                Some(Drag::LifelineEnd { x, top, current, .. }) => {
                                    // The line as long as it will be.
                                    let end = MPoint::new(*x, current.y.max(*top));
                                    let s = p.c.selection;
                                    p.line(
                                        &[MPoint::new(*x, *top), end],
                                        1.5,
                                        s,
                                        LineStyle::Dashed,
                                    );
                                    p.grips(&[end]);
                                }
                                Some(Drag::Gate { x, y, current, .. }) => {
                                    // The message as long as it will be, on the side it goes.
                                    let end = MPoint::new(current.x, *y);
                                    let s = p.c.selection;
                                    p.line(&[MPoint::new(*x, *y), end], 1.5, s, LineStyle::Dashed);
                                    p.grips(&[end]);
                                }
                                Some(Drag::HandleLine { source, start, current, message }) => {
                                    // A message runs from where it starts, a line from the
                                    // border of its node.
                                    let node = scene.node(*source).filter(|_| !*message);
                                    let from = node
                                        .map_or(*start, |n| scene::border_point(n.rect, *current));
                                    let s = p.c.selection;
                                    p.line(&[from, *current], 1.5, s, LineStyle::Dashed);
                                }
                                Some(Drag::Connect { source, current, .. }) => {
                                    if let Some(node) = scene.node(*source) {
                                        let from = scene::border_point(node.rect, *current);
                                        let s = p.c.selection;
                                        p.line(&[from, *current], 1.5, s, LineStyle::Dashed);
                                    }
                                }
                                _ => {}
                            }
                        });
                    },
                )
                .size_full(),
            )
            .children(editor)
            .children(handles)
            .children(offer_menu)
            .context_menu(move |menu, window, cx| Self::context_menu(&menu_owner, menu, window, cx))
    }
}
