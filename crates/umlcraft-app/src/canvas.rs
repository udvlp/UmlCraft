//! The diagram editor surface: paints a [`Scene`] and handles tools, selection, dragging,
//! waypoints, in-place editing and elements dropped from the model tree.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::{
    Aggregation, AssociationEnd, ElementKind, Id, LineShape, Model, Multiplicity, Navigability,
    Point as MPoint, Rect,
};
use umlcraft_notation::draw::{self, Paint, Primitive};
use umlcraft_notation::member::member_text;
use umlcraft_notation::palette::{EdgeTool, NodeTool, Tool};
use umlcraft_notation::route::{self, Route};
use umlcraft_notation::scene::{
    self, EdgeHandle, EdgeShape, FONT_SIZE, Figure, Handle, LabelAlign, LineStyle, NodeShape,
    Preview, Row, RowRole, Scene, TextMeasure,
};
use umlcraft_notation::sequence::SequenceLayout;

use crate::workspace::{EndChange, Workspace};
use crate::{
    Cancel, DeleteSelection, LINE_SHAPES, RemoveFromDiagram, ResetLabels, ResetLineEnds, ResetZoom,
    SelectAll, SetAssociationEnd, SetLineShape, ZoomToFit,
};

const MIN_ZOOM: f32 = 0.25;
/// The font of diagram texts. Astah uses Java's "Dialog", which is Arial on Windows.
const DIAGRAM_FONT: &str = "Arial";
/// How far below the top of its line Java puts the baseline of 12 px "Dialog" (12.06) minus
/// where GPUI puts it (11.70), relative to the font size.
const BASELINE_SHIFT: f32 = 0.36 / 12.0;
const MAX_ZOOM: f32 = 4.0;
const GRID: f32 = 20.0;
/// Smallest node edge when resizing.
const MIN_NODE: f32 = 20.0;
/// How close (in screen pixels) the pointer must be to grab a handle.
const HANDLE_GRAB: f32 = 6.0;

/// Snaps a logical coordinate or distance to half the grid.
fn snap(v: f32) -> f32 {
    (v / GRID * 2.0).round() * GRID / 2.0
}

fn snap_point(p: MPoint) -> MPoint {
    MPoint::new(snap(p.x), snap(p.y))
}

/// One end of an association as the context menu shows it.
struct EndEntry {
    label: String,
    end: AssociationEnd,
}

/// The association edge `presentation` shows, with a menu label for each end: the participant's
/// name and its role, if any.
fn association_ends(
    model: &Model,
    diagram: Option<Id>,
    presentation: Id,
) -> Option<(Id, Vec<EndEntry>)> {
    let element = model.view(diagram?)?.get(presentation)?.element?;
    let ElementKind::Association(p) = model.get(element)?.kind() else { return None };
    let ends = p
        .ends
        .iter()
        .map(|end| {
            let name = model.get(end.participant).map(|e| e.name().to_owned()).unwrap_or_default();
            let label = if end.role.is_empty() { name } else { format!("{name} ({})", end.role) };
            EndEntry { label, end: end.clone() }
        })
        .collect();
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
                    let action = SetAssociationEnd { association, end: i, change };
                    menu.menu_with_check(label, checked, Box::new(action))
                })
            })
        })
    })
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
    Move {
        start: MPoint,
        dx: f32,
        dy: f32,
        reselect: Option<Id>,
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
    Pan {
        last: Point<Pixels>,
    },
}

enum Editor {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

/// Text being edited in place, over the row it replaces.
struct InlineEdit {
    element: Id,
    /// Logical rectangle of the edited text.
    rect: Rect,
    editor: Editor,
    _subscription: Subscription,
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
    editing: Option<InlineEdit>,
}

/// Measures text with the window's font system, at 100 % zoom.
pub struct FontMeasure<'a> {
    window: &'a Window,
    family: SharedString,
}

impl<'a> FontMeasure<'a> {
    /// Measures in the diagram font.
    pub fn new(window: &'a Window) -> Self {
        Self { window, family: DIAGRAM_FONT.into() }
    }
}

impl TextMeasure for FontMeasure<'_> {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        if text.is_empty() {
            return 0.0;
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
        f32::from(line.width())
    }
}

/// Measures text with the app's font system by adding up the advances of the characters, as
/// Java does without kerning. It needs no window, so the workspace keeps one for sizing boxes.
pub struct AppMeasure {
    text: Arc<TextSystem>,
    regular: FontId,
    bold: FontId,
}

impl AppMeasure {
    pub fn new(cx: &App) -> Self {
        let text = cx.text_system().clone();
        let regular = text.resolve_font(&font(DIAGRAM_FONT));
        let bold = text.resolve_font(&font(DIAGRAM_FONT).bold());
        Self { text, regular, bold }
    }
}

impl TextMeasure for AppMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        let font = if bold { self.bold } else { self.regular };
        let advance =
            |c| self.text.advance(font, px(font_size), c).map_or(0.0, |s| f32::from(s.width));
        text.chars().map(advance).sum()
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
            editing: None,
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
        let local = self.local(screen);
        MPoint::new((local.x - self.pan.x) / self.zoom, (local.y - self.pan.y) / self.zoom)
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

    fn preview(&self, ws: &Workspace) -> Preview {
        match &self.drag {
            Some(Drag::Move { dx, dy, .. }) => Preview {
                moved: scene::with_contents(&self.scene, &ws.selection).into_iter().collect(),
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
            _ => Preview::default(),
        }
    }

    /// The node under `p` for connecting. Fragment frames and swimlanes only count at their
    /// border, so messages and flows can be drawn between what lies inside them.
    fn node_at(&self, p: MPoint) -> Option<Id> {
        let tolerance = 4.0 / self.zoom;
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

    fn on_left_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_edit(cx);
        self.focus_handle.focus(window, cx);
        let p = self.to_logical(ev.position);
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
            Tool::Edge(edge_tool) => match (self.drag.clone(), self.node_at(p)) {
                // Second click of click-click mode; a second click on the source makes a loop.
                (Some(Drag::Connect { source, start, .. }), Some(target)) => {
                    self.drag = None;
                    self.finish_connect(edge_tool, source, target, start, cx);
                }
                (_, Some(source)) => {
                    self.drag = Some(Drag::Connect { source, start: p, current: p });
                }
                (_, None) => self.drag = None,
            },
            Tool::Select => {
                if self.grab_handle(&selection, p, ev.click_count, cx) {
                    cx.notify();
                    return;
                }
                if ev.click_count == 2 && self.start_edit_at(p, window, cx) {
                    return;
                }
                // A message's label can be moved away from its line.
                let label = scene::message_label_at(&self.scene, p, &FontMeasure::new(window));
                if let Some((message, origin)) = label.filter(|_| !extend) {
                    self.workspace.update(cx, |ws, _| ws.select(vec![message]));
                    self.drag = Some(Drag::MessageLabel { message, origin, start: p, current: p });
                    cx.notify();
                    return;
                }
                match scene::hit_test(&self.scene, p, 4.0 / self.zoom) {
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
                        } else if self.scene.node(id).is_some() && selected {
                            let reselect =
                                (already && !extend && selection.len() > 1).then_some(id);
                            self.drag = Some(Drag::Move { start: p, dx: 0.0, dy: 0.0, reselect });
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
        let Some(edge) = self.scene.edge(single) else { return false };
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
        let hit = scene::hit_test(&self.scene, p, 4.0 / self.zoom);
        self.update_workspace(cx, |ws| match hit {
            Some(id) if ws.selection.contains(&id) => {}
            Some(id) => ws.select(vec![id]),
            None => ws.clear_selection(),
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
        let (current, labeled, association, nothing, focus) = {
            let this = canvas.read(cx);
            let ws = this.workspace.read(cx);
            let current: Vec<LineShape> = this.selected_edges(cx).iter().map(|e| e.shape).collect();
            let labeled = ws.selection.iter().any(|&id| this.scene.edge(id).is_some());
            let association = match ws.selection[..] {
                [single] if this.scene.edge(single).is_some_and(|e| !e.fixed) => {
                    association_ends(ws.doc.model(), ws.active_diagram, single)
                }
                _ => None,
            };
            (current, labeled, association, ws.selection.is_empty(), this.focus_handle.clone())
        };
        let mut menu = menu.action_context(focus);
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
            | Drag::MessageLabel { current, .. } => *current = p,
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
            Some(Drag::Move { dx, dy, .. }) => {
                // Containers (boundaries, packages) take the nodes drawn inside them along.
                let selection = self.workspace.read(cx).selection.clone();
                let moved = scene::with_contents(&self.scene, &selection);
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
            Some(Drag::RubberBand { start, extend, .. }) => {
                let hits = scene::nodes_in(&self.scene, Rect::from_corners(start, p));
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
                match (tool, self.node_at(p)) {
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
            _ => {}
        }
        cx.notify();
    }

    fn on_scroll(&mut self, ev: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
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
        let (presentation, element, role, rect) = match row {
            Some((node, row)) => {
                let Some(element) = row.element else { return false };
                let rect = if row.role == RowRole::Name && matches!(node.figure, Figure::Note) {
                    node.rect.inflate(-4.0)
                } else {
                    row.rect
                };
                (node.id, element, row.role, rect)
            }
            // A message of a sequence diagram: edit its label.
            None => {
                let Some(id) = scene::hit_test(&self.scene, p, 4.0 / self.zoom) else {
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
            _ => (e.name().to_owned(), false),
        };
        let node_id = presentation;
        self.workspace.update(cx, |ws, cx| {
            ws.select(vec![node_id]);
            ws.selected_element = Some(element);
            cx.notify();
        });
        // Focus once the editor is on screen; the click itself still focuses the canvas.
        let editor = if multi_line {
            let state = cx.new(|cx| TextareaState::new(window, cx).default_value(text));
            let focus = state.clone();
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
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
            window.on_next_frame(move |window, cx| focus.update(cx, |s, cx| s.focus(window, cx)));
            let subscription =
                cx.subscribe_in(&state, window, |this, _, ev: &InputEvent, _, cx| {
                    if matches!(ev, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.commit_edit(cx);
                    }
                });
            (Editor::Line(state), subscription)
        };
        self.editing =
            Some(InlineEdit { element, rect, editor: editor.0, _subscription: editor.1 });
        cx.notify();
        true
    }

    /// Writes the in-place edit back, if one is open.
    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.editing.take() else { return };
        let text = match &edit.editor {
            Editor::Line(s) => s.read(cx).value().to_string(),
            Editor::Area(s) => s.read(cx).value().to_string(),
        };
        let element = edit.element;
        self.update_workspace(cx, |ws| ws.apply_text(element, &text));
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
        let origin = area.origin().offset(0.0, font_size * BASELINE_SHIFT);
        line.paint(self.t.pt(origin), line_height, align, width, self.window, self.cx).ok();
        measured
    }

    fn row(&mut self, row: &Row) {
        let color = self.c.text;
        let column = scene::visibility_column(&row.text)
            .filter(|_| row.role == RowRole::Member && row.align == LabelAlign::Left);
        let width = match column {
            // The visibility symbol centered in a column as wide as a "+", the rest after it.
            Some((symbol, rest)) => {
                let slot = AppMeasure::new(self.cx).width("+", row.size, row.bold);
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

    fn grid(&mut self, bounds: Bounds<Pixels>) {
        let step = GRID * self.t.zoom;
        if step < 8.0 {
            return;
        }
        let top_left = self.t.to_logical(bounds.origin);
        let bottom_right = self.t.to_logical(bounds.bottom_right());
        let mut x = (top_left.x / GRID).floor() * GRID;
        while x <= bottom_right.x {
            let sx = self.t.pt(MPoint::new(x, 0.0)).x;
            self.window.paint_quad(fill(
                Bounds::new(point(sx, bounds.origin.y), size(px(1.0), bounds.size.height)),
                self.c.grid,
            ));
            x += GRID;
        }
        let mut y = (top_left.y / GRID).floor() * GRID;
        while y <= bottom_right.y {
            let sy = self.t.pt(MPoint::new(0.0, y)).y;
            self.window.paint_quad(fill(
                Bounds::new(point(bounds.origin.x, sy), size(bounds.size.width, px(1.0))),
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
        let font_family: SharedString = DIAGRAM_FONT.into();
        let scene = {
            let ws = self.workspace.read(cx);
            let measure = FontMeasure::new(window);
            match ws.active_diagram {
                Some(d) => scene::build(ws.doc.model(), d, &self.preview(ws), &measure),
                None => Scene::empty(),
            }
        };
        let scene = Rc::new(scene);
        self.scene = scene.clone();
        let selection = self.workspace.read(cx).selection.clone();
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
        let editing_element = self.editing.as_ref().map(|e| e.element);
        let bounds_cell = self.bounds.clone();
        let this = cx.entity();
        let menu_owner = this.clone();
        let cursor = if tool == Tool::Select { CursorStyle::Arrow } else { CursorStyle::Crosshair };

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
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            // Window-wide listeners (see the paint closure) follow drags outside the canvas,
            // but only exist from the next frame on; these catch very quick drags.
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    this.drag_to(ev.position, cx);
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
            .on_scroll_wheel(cx.listener(Self::on_scroll))
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
            .on_action(cx.listener(|this, _: &ResetLabels, _, cx| {
                let selection = this.workspace.read(cx).selection.clone();
                let edges: Vec<Id> =
                    selection.into_iter().filter(|&id| this.scene.edge(id).is_some()).collect();
                this.update_workspace(cx, |ws| ws.reset_labels(&edges));
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
                            let mut p =
                                Painter { t, c: colors, font_family, window, cx, node_fill: None };
                            p.grid(bounds);
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
                                Some(Drag::MessageLabel { message, start, current, .. }) => {
                                    // The labels where they will go.
                                    let (dx, dy) = (current.x - start.x, current.y - start.y);
                                    let s = p.c.selection;
                                    for label in scene.edge(*message).iter().flat_map(|e| &e.labels)
                                    {
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
            .context_menu(move |menu, window, cx| Self::context_menu(&menu_owner, menu, window, cx))
    }
}
