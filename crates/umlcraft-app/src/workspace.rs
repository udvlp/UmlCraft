use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use umlcraft_model::{
    Aggregation, DiagramKind, Document, EdgeLabels, ElementData, ElementKind, Id, LineShape, Model,
    ModelError, Multiplicity, Navigability, Point, Presentation, Rect, Shape, StructogramProps,
    Style, TemplateParameterProps, Tx, TypeRef, Visibility,
};
use umlcraft_notation::autoroute;
use umlcraft_notation::clipboard::{self, Clip};
use umlcraft_notation::edit;
use umlcraft_notation::layout;
use umlcraft_notation::member;
use umlcraft_notation::object;
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::route::Route;
use umlcraft_notation::scene::{self, Alignment, ApproxMeasure, TextMeasure};
use umlcraft_notation::sequence::Slot;
use umlcraft_notation::state_edit;

/// The open project and what the user is looking at. Shared by all panels as a GPUI entity.
pub struct Workspace {
    pub doc: Document,
    pub path: Option<PathBuf>,
    /// Where a project never saved is suggested to go, e.g. next to the `.nsd` file it holds.
    pub draft: Option<PathBuf>,
    /// Diagrams and structograms open as tabs, in tab order.
    pub open_diagrams: Vec<Id>,
    pub active_diagram: Option<Id>,
    /// Element shown in the properties panel and highlighted in the model tree.
    pub selected_element: Option<Id>,
    /// Selected presentations of the active diagram; the first one is the alignment reference.
    pub selection: Vec<Id>,
    pub tool: Tool,
    /// Last problem to show in the status bar, e.g. a refused connection.
    pub message: Option<String>,
    /// Measures texts for boxes sized to their content.
    pub measure: Box<dyn TextMeasure>,
}

impl Workspace {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        let first_diagram = first_diagram(doc.model());
        Self {
            doc,
            path,
            draft: None,
            open_diagrams: first_diagram.into_iter().collect(),
            active_diagram: first_diagram,
            selected_element: None,
            selection: Vec::new(),
            tool: Tool::Select,
            message: None,
            measure: Box::new(ApproxMeasure),
        }
    }

    /// Measures texts with `measure` instead of an estimate.
    pub fn with_measure(mut self, measure: impl TextMeasure + 'static) -> Self {
        self.measure = Box::new(measure);
        self
    }

    pub fn file_name(&self) -> Option<String> {
        self.path.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned())
    }

    /// Opens `diagram` (a diagram or a structogram) as a tab (if needed) and activates it.
    pub fn open_diagram(&mut self, diagram: Id) {
        if !is_shown(self.doc.model(), diagram) {
            return;
        }
        if !self.open_diagrams.contains(&diagram) {
            self.open_diagrams.push(diagram);
        }
        if self.active_diagram != Some(diagram) {
            self.active_diagram = Some(diagram);
            self.selection.clear();
            self.tool = Tool::Select;
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected_element = None;
        self.selection.clear();
    }

    fn element_of(&self, presentation: Id) -> Option<Id> {
        let view = self.doc.model().view(self.active_diagram?)?;
        view.get(presentation)?.element
    }

    /// Replaces the selection with the given presentations of the active diagram.
    pub fn select(&mut self, presentations: Vec<Id>) {
        self.selected_element = presentations.first().and_then(|&p| self.element_of(p));
        self.selection = presentations;
    }

    /// Adds or removes one presentation (Ctrl/Shift click).
    pub fn toggle(&mut self, presentation: Id) {
        if let Some(ix) = self.selection.iter().position(|&p| p == presentation) {
            self.selection.remove(ix);
        } else {
            self.selection.push(presentation);
        }
        self.selected_element = self.selection.first().and_then(|&p| self.element_of(p));
    }

    /// Selects an element (e.g. from the tree) and its presentation on the active diagram.
    pub fn select_element(&mut self, element: Id) {
        self.selected_element = Some(element);
        self.selection = self
            .active_diagram
            .and_then(|d| self.doc.model().view(d))
            .and_then(|v| v.presentations_of(element).next())
            .map(|p| vec![p.id])
            .unwrap_or_default();
    }

    pub fn select_all(&mut self) {
        let all = self
            .active_diagram
            .and_then(|d| self.doc.model().view(d))
            .map(|v| v.presentations().iter().filter(|p| !p.is_edge()).map(|p| p.id).collect())
            .unwrap_or_default();
        self.select(all);
    }

    /// Selected node presentations with their bounds, in selection order.
    pub fn selected_nodes(&self) -> Vec<(Id, Rect)> {
        let Some(view) = self.active_diagram.and_then(|d| self.doc.model().view(d)) else {
            return Vec::new();
        };
        self.selection
            .iter()
            .filter_map(|&p| view.get(p).and_then(|pr| pr.bounds()).map(|b| (p, b)))
            .collect()
    }

    /// Drops tabs and selections that no longer exist, e.g. after undo.
    pub fn validate(&mut self) {
        let model = self.doc.model();
        self.open_diagrams.retain(|&d| is_shown(model, d));
        if self.active_diagram.is_none_or(|d| !is_shown(model, d)) {
            self.active_diagram = self.open_diagrams.first().copied();
        }
        if self.selected_element.is_some_and(|e| !model.contains(e)) {
            self.selected_element = None;
        }
        let view = self.active_diagram.and_then(|d| model.view(d));
        self.selection.retain(|&p| view.is_some_and(|v| v.get(p).is_some()));
    }

    /// One undo step, with boxes fitted to their content and kept lines routed again
    /// afterwards ([`edit::apply`]).
    fn edit<R>(
        &mut self,
        label: String,
        f: impl FnOnce(&mut Tx<'_>) -> Result<R, ModelError>,
    ) -> Result<R, ModelError> {
        edit::apply(&mut self.doc, label, &*self.measure, f)
    }

    fn report<T>(&mut self, result: Result<T, ModelError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.message = None;
                Some(value)
            }
            Err(err) => {
                self.message = Some(crate::errors::model_error(Some(self.doc.model()), &err));
                None
            }
        }
    }

    /// Creates an element with `tool` at `origin` on the active diagram and selects it.
    pub fn create_node(&mut self, tool: NodeTool, origin: Point) {
        let Some(diagram) = self.active_diagram else { return };
        // A system boundary is named for the system it bounds, not for itself.
        let key = match tool {
            NodeTool::SystemBoundary => "new_name.systemBoundary".to_string(),
            _ => format!("kind.{}", tool.key()),
        };
        let base = name_base(&key, keeps_words(tool));
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| palette::create_node(tx, diagram, tool, origin, &base));
        if let Some((element, node)) = self.report(result) {
            self.selection = vec![node];
            self.selected_element = Some(element);
        }
    }

    /// The box Astah gives a free text: as wide as its widest line plus 20, 8 higher than its
    /// lines.
    fn free_text_bounds(&self, origin: Point, text: &str) -> Rect {
        let lines: Vec<&str> = text.split('\n').collect();
        let width = lines
            .iter()
            .map(|l| self.measure.width(l, scene::FONT_SIZE, false))
            .fold(0.0, f32::max);
        let height = 8.0 + lines.len() as f32 * scene::line_height(scene::FONT_SIZE);
        Rect::new(origin.x, origin.y, width + 20.0, height)
    }

    /// Adds a free text at `origin` to the active diagram and selects it.
    pub fn create_free_text(&mut self, origin: Point) -> Option<Id> {
        let diagram = self.active_diagram?;
        let text = t!("kind.text").to_string();
        let bounds = self.free_text_bounds(origin, &text);
        let label = t!("edit.create").to_string();
        let result =
            self.edit(label, |tx| tx.add_presentation(diagram, Presentation::text(bounds, text)));
        let id = self.report(result)?;
        self.select(vec![id]);
        Some(id)
    }

    /// Adds a free line from `from` to `to` to the active diagram and selects it.
    pub fn create_free_line(&mut self, from: Point, to: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.create").to_string();
        let result =
            self.edit(label, |tx| tx.add_presentation(diagram, Presentation::line(from, to)));
        if let Some(id) = self.report(result) {
            self.select(vec![id]);
        }
    }

    /// Moves end `end` (0 start, 1 end) of the free line `presentation` to `to`.
    pub fn set_free_line_end(&mut self, presentation: Id, end: usize, to: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let view = self.doc.model().view(diagram);
        let Some(&Shape::Line { from, to: target, .. }) =
            view.and_then(|v| v.get(presentation)).map(|p| &p.shape)
        else {
            return;
        };
        // A click on a grip without dragging changes nothing.
        if [from, target][end.min(1)] == to {
            return;
        }
        let label = t!("edit.move").to_string();
        let result = self.edit(label, |tx| {
            tx.update_presentation(diagram, presentation, |p| {
                if let Shape::Line { from, to: target, .. } = &mut p.shape {
                    *if end == 0 { from } else { target } = to;
                }
            })
        });
        self.report(result);
    }

    /// Changes the text of the free text `presentation`; its box fits the new text.
    pub fn set_free_text(&mut self, presentation: Id, text: &str) {
        let Some(diagram) = self.active_diagram else { return };
        let view = self.doc.model().view(diagram);
        let Some(Shape::Text { bounds, .. }) =
            view.and_then(|v| v.get(presentation)).map(|p| &p.shape)
        else {
            return;
        };
        let bounds = self.free_text_bounds(bounds.origin(), text);
        let label = t!("edit.text").to_string();
        let result = self.edit(label, |tx| {
            tx.update_presentation(diagram, presentation, |p| {
                p.shape = Shape::Text { bounds, text: text.to_owned() };
            })
        });
        self.report(result);
    }

    /// Changes how the free line `presentation` is drawn.
    pub fn update_free_line(
        &mut self,
        presentation: Id,
        f: impl FnOnce(&mut bool, &mut bool, &mut bool),
    ) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.properties").to_string();
        let result = self.edit(label, |tx| {
            tx.update_presentation(diagram, presentation, |p| {
                if let Shape::Line { dashed, start_arrow, end_arrow, .. } = &mut p.shape {
                    f(dashed, start_arrow, end_arrow);
                }
            })
        });
        self.report(result);
    }

    /// Draws a line with `tool` and selects it. Lines but note anchors are routed around the
    /// nodes and kept routed, except on use case diagrams (see [`autoroute::route_new_lines`]).
    pub fn connect(&mut self, tool: EdgeTool, source: Id, target: Id) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.connect").to_string();
        let result = self.edit(label, |tx| {
            let edge = palette::connect(tx, diagram, tool, source, target)?;
            if tool != EdgeTool::NoteAnchor {
                autoroute::route_new_lines(tx, diagram, &[edge])?;
            }
            Ok(edge)
        });
        if let Some(edge) = self.report(result) {
            self.select(vec![edge]);
        }
    }

    /// Draws a message on the active sequence diagram at `slot` and selects it.
    pub fn connect_message(&mut self, tool: EdgeTool, source: Id, target: Id, slot: Option<Slot>) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.connect").to_string();
        let result = self
            .edit(label, |tx| palette::connect_message(tx, diagram, tool, source, target, slot));
        if let Some(edge) = self.report(result) {
            self.select(vec![edge]);
        }
    }

    /// Adds a combined fragment or interaction use (`tool`) to the active sequence diagram and
    /// selects it.
    pub fn create_fragment(
        &mut self,
        tool: NodeTool,
        slot: Option<Slot>,
        covered: Vec<Id>,
        origin: Point,
    ) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| {
            palette::create_fragment(tx, diagram, tool, slot, covered, Some(origin))
        });
        if let Some((fragment, node)) = self.report(result) {
            self.selection = vec![node];
            self.selected_element = Some(fragment);
        }
    }

    /// Puts the selected messages and fragments into a new combined fragment.
    pub fn wrap_selection_in_fragment(&mut self) {
        let Some(diagram) = self.active_diagram else { return };
        let Some(view) = self.doc.model().view(diagram) else { return };
        let items: Vec<Id> = self.selection.iter().filter_map(|&p| view.get(p)?.element).collect();
        let label = t!("edit.wrap_in_fragment").to_string();
        let result = self.edit(label, |tx| palette::wrap_in_fragment(tx, diagram, &items));
        if let Some((fragment, node)) = self.report(result) {
            self.selection = vec![node];
            self.selected_element = Some(fragment);
        }
    }

    /// Moves a message or fragment to another place in its interaction.
    pub fn move_to_slot(&mut self, item: Id, slot: Slot) {
        let label = t!("edit.move_element").to_string();
        let result = self.edit(label, |tx| palette::move_to_slot(tx, item, slot));
        self.report(result);
    }

    /// Shows or hides the message numbers of the open sequence diagram, or with `flat`
    /// switches between numbers 1, 2, 3 and 2.1.
    pub fn toggle_numbering(&mut self, flat: bool) {
        let Some(diagram) = self.active_diagram else { return };
        self.update_element(diagram, |d| {
            if let ElementKind::Diagram(p) = &mut d.kind {
                if flat {
                    p.flat_message_numbers = !p.flat_message_numbers;
                } else {
                    p.hide_message_numbers = !p.hide_message_numbers;
                }
            }
        });
    }

    /// Adds an operand to a combined fragment and selects it.
    pub fn add_operand(&mut self, fragment: Id) {
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| palette::add_operand(tx, fragment));
        if let Some(operand) = self.report(result) {
            self.selected_element = Some(operand);
        }
    }

    /// Divides a state into one more region (the first time into two, laid out side by side
    /// if `vertical`) and selects the new region.
    pub fn add_region(&mut self, state: Id, vertical: bool) {
        let label = t!("edit.create").to_string();
        let diagram = self.active_diagram;
        let result = self.edit(label, |tx| state_edit::add_region(tx, state, vertical, diagram));
        if let Some(region) = self.report(result) {
            self.selected_element = Some(region);
        }
    }

    /// Adds an internal transition to a state and selects it.
    pub fn add_internal_transition(&mut self, state: Id) {
        let label = t!("edit.create").to_string();
        let trigger = t!("prop.new_trigger").to_string();
        let result =
            self.edit(label, |tx| state_edit::add_internal_transition(tx, state, &trigger));
        if let Some(transition) = self.report(result) {
            self.selected_element = Some(transition);
        }
    }

    /// Moves line `index` between the regions of a state on the active diagram to `offset`
    /// from the state's edge.
    pub fn move_region_line(&mut self, presentation: Id, index: usize, offset: f32) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.move").to_string();
        let result = self.edit(label, |tx| {
            state_edit::move_region_line(tx, diagram, presentation, index, offset)
        });
        self.report(result);
    }

    /// Moves the given nodes of the active diagram by (`dx`, `dy`) as one undo step.
    pub fn move_nodes(&mut self, presentations: &[Id], dx: f32, dy: f32) {
        let Some(diagram) = self.active_diagram else { return };
        let model = self.doc.model();
        let Some(view) = model.view(diagram) else { return };
        let kind_of = |p: &umlcraft_model::Presentation| p.element.and_then(|e| model.get(e));
        // New bounds per node. On sequence diagrams lifelines only move sideways; what moves
        // into or out of a fragment changes its place in the interaction.
        let nodes: Vec<(Id, Rect)> = presentations
            .iter()
            .filter_map(|&p| view.get(p))
            .filter_map(|p| {
                let bounds = p.bounds()?;
                match kind_of(p).map(|e| e.kind()) {
                    Some(ElementKind::Lifeline(_)) => Some((p.id, bounds.translate(dx, 0.0))),
                    _ => Some((p.id, bounds.translate(dx, dy))),
                }
            })
            .collect();
        let lines: Vec<Id> = presentations
            .iter()
            .filter_map(|&p| view.get(p))
            .filter(|p| matches!(p.shape, Shape::Line { .. }))
            .map(|p| p.id)
            .collect();
        let diagram_kind = model.get(diagram).and_then(|d| d.kind().as_diagram());
        let sequence = diagram_kind == Some(umlcraft_model::DiagramKind::Sequence);
        let states = diagram_kind == Some(umlcraft_model::DiagramKind::StateMachine);
        if (nodes.is_empty() && lines.is_empty()) || (dx == 0.0 && dy == 0.0) {
            return;
        }
        // Edges between two moved nodes keep their shape: their waypoints move along.
        let moved: HashSet<Id> = nodes.iter().map(|&(p, _)| p).collect();
        let edges: Vec<(Id, Vec<Point>)> = self
            .doc
            .model()
            .view(diagram)
            .map(|v| {
                v.presentations()
                    .iter()
                    .filter_map(|p| match &p.shape {
                        Shape::Edge { source, target, waypoints, .. }
                            if !waypoints.is_empty()
                                && moved.contains(source)
                                && moved.contains(target) =>
                        {
                            Some((p.id, waypoints.iter().map(|w| w.offset(dx, dy)).collect()))
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let label = t!("edit.move").to_string();
        let result = self.edit(label, |tx| {
            nodes.iter().try_for_each(|&(p, b)| tx.set_bounds(diagram, p, b))?;
            lines.iter().try_for_each(|&l| tx.move_line(diagram, l, dx, dy))?;
            edges.into_iter().try_for_each(|(e, w)| tx.set_waypoints(diagram, e, w))?;
            if sequence {
                umlcraft_notation::sequence_edit::tidy(tx, diagram)?;
            }
            // A state dropped into another one (or out of it) becomes its substate.
            if states {
                state_edit::reparent_vertices(tx, diagram, presentations)?;
            }
            Ok(())
        });
        self.report(result);
    }

    /// Changes the display options of one presentation on the active diagram.
    pub fn update_style(&mut self, presentation: Id, f: impl FnOnce(&mut Style)) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.properties").to_string();
        let result = self
            .edit(label, |tx| tx.update_presentation(diagram, presentation, |p| f(&mut p.style)));
        self.report(result);
    }

    /// Merges parsed source files into the model and files the structograms `built` from their
    /// methods, as one undo step (Tools › Import). Returns the import's report and each
    /// structogram made or replaced with the report of its method.
    pub fn import_source(
        &mut self,
        found: &crate::nsd::source::Found,
        built: Vec<crate::nsd::source::Built>,
    ) -> Result<(umlcraft_reverse::Report, Vec<(Id, umlcraft_nsd_code::Report)>), ModelError> {
        let label = t!("edit.import_source").to_string();
        let result = self.edit(label, |tx| {
            let (report, placed) =
                umlcraft_reverse::apply_placed(tx, found.language, &found.parsed)?;
            let made = crate::nsd::source::file_structograms(tx, found, &placed, built)?;
            Ok((report, made))
        });
        if result.is_ok() {
            self.message = None;
        }
        result
    }

    /// Reports a problem in the status bar.
    pub fn complain(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    /// Gives an edge a route drawn by hand; the router leaves it alone from then on.
    pub fn set_route(&mut self, edge: Id, route: Route) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.waypoints").to_string();
        let anchors = (route.source_anchor, route.target_anchor);
        let result = self.edit(label, |tx| {
            tx.set_route(diagram, edge, route.waypoints, anchors)?;
            tx.update_presentation(diagram, edge, |p| p.style.auto_route = false)
        });
        self.report(result);
    }

    /// Lets the given edges aim at the centers of their nodes again, without the router.
    pub fn reset_line_ends(&mut self, edges: &[Id]) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.line_ends").to_string();
        let result = self.edit(label, |tx| {
            edges.iter().try_for_each(|&edge| {
                tx.update_presentation(diagram, edge, |p| {
                    if let Shape::Edge { source_anchor, target_anchor, .. } = &mut p.shape {
                        (*source_anchor, *target_anchor) = (None, None);
                    }
                    p.style.auto_route = false;
                })
            })
        });
        self.report(result);
    }

    /// Routes the selected lines of the active diagram around the nodes, or without a selected
    /// line all of them, and keeps them routed. Note anchors, which have no element, stay
    /// straight.
    pub fn route_lines(&mut self) {
        let Some(diagram) = self.active_diagram else { return };
        let Some(view) = self.doc.model().view(diagram) else { return };
        let lines =
            |p: &&Presentation| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some();
        let mut edges: Vec<Id> = self
            .selection
            .iter()
            .filter_map(|&id| view.get(id))
            .filter(lines)
            .map(|p| p.id)
            .collect();
        if edges.is_empty() {
            edges = view.presentations().iter().filter(lines).map(|p| p.id).collect();
        }
        if edges.is_empty() {
            return;
        }
        let label = t!("edit.route_lines").to_string();
        let result = self.edit(label, |tx| autoroute::route_lines(tx, diagram, &edges));
        self.report(result);
    }

    /// Arranges the active diagram, or with a selection only that, and routes its lines.
    /// Nodes in the way of the arranged selection move aside.
    pub fn arrange(&mut self, direction: layout::Direction) {
        let Some(diagram) = self.active_diagram else { return };
        let selected: Vec<Id> = self.selected_nodes().into_iter().map(|(id, _)| id).collect();
        let scope = (!selected.is_empty()).then_some(&selected[..]);
        let label = t!("edit.arrange").to_string();
        let measure = &*self.measure;
        let result = self
            .doc
            .edit(label, |tx| layout::arrange(tx, diagram, scope, direction, measure).map(|_| ()));
        self.report(result);
    }

    /// Puts the labels of the given edges and messages back where the layout puts them.
    pub fn reset_labels(&mut self, edges: &[Id]) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.labels").to_string();
        let result = self.edit(label, |tx| {
            edges.iter().try_for_each(|&edge| {
                tx.update_presentation(diagram, edge, |p| match &mut p.shape {
                    Shape::Edge { labels, .. } | Shape::Message { labels, .. } => {
                        *labels = EdgeLabels::default();
                    }
                    _ => {}
                })
            })
        });
        self.report(result);
    }

    /// Changes end `end` of an association.
    pub fn set_association_end(&mut self, association: Id, end: usize, change: EndChange) {
        self.update_element(association, |data| {
            if let ElementKind::Association(p) = &mut data.kind
                && let Some(end) = p.ends.get_mut(end)
            {
                match change {
                    EndChange::Navigability(n) => end.navigability = n,
                    EndChange::Aggregation(a) => end.aggregation = a,
                    EndChange::Multiplicity(m) => end.multiplicity = m,
                }
            }
        });
    }

    /// Lets a lifeline of the active diagram end at `bottom`.
    pub fn set_lifeline_end(&mut self, presentation: Id, bottom: f32) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.resize").to_string();
        let result = self.edit(label, |tx| {
            umlcraft_notation::sequence_edit::set_lifeline_end(tx, diagram, presentation, bottom)
        });
        self.report(result);
    }

    /// Turns the relationship the edge `edge` of the active diagram shows into one `tool`
    /// makes, as one undo step.
    pub fn change_relationship(&mut self, edge: Id, tool: EdgeTool) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.change_kind").to_string();
        let result = self.edit(label, |tx| palette::change_relationship(tx, diagram, edge, tool));
        if let Some(new) = self.report(result) {
            // The panel shows the new element, which the line now stands for.
            self.selected_element = Some(new);
        }
    }

    /// Sets the role name or the multiplicity of an association end from text typed in place.
    pub fn set_end_text(&mut self, association: Id, end: usize, multiplicity: bool, text: &str) {
        let text = text.trim();
        if multiplicity {
            let parsed =
                if text.is_empty() { Ok(None) } else { text.parse::<Multiplicity>().map(Some) };
            match parsed {
                Ok(m) => self.set_association_end(association, end, EndChange::Multiplicity(m)),
                Err(err) => self.complain(crate::errors::multiplicity_error(&err)),
            }
            return;
        }
        // As the label shows it: `- buyer {ordered}`; without a sign the visibility stays.
        let (visibility, role, constraints) = member::parse_role(text);
        self.update_element(association, |data| {
            if let ElementKind::Association(p) = &mut data.kind
                && let Some(end) = p.ends.get_mut(end)
            {
                end.role = role;
                end.constraints = constraints;
                end.visibility = visibility.unwrap_or(end.visibility);
            }
        });
    }

    /// Gives an attribute its type or an operation its return type.
    pub fn set_member_type(&mut self, member: Id, type_ref: TypeRef) {
        self.update_element(member, |data| match &mut data.kind {
            ElementKind::Attribute(p) => p.type_ref = Some(type_ref),
            ElementKind::Operation(p) => p.return_type = Some(type_ref),
            _ => {}
        });
    }

    /// Gives edges of the active diagram a line style, each with its route for that style, as
    /// one undo step.
    pub fn set_line_shape(&mut self, shape: LineShape, edges: Vec<(Id, Route)>) {
        let Some(diagram) = self.active_diagram else { return };
        if edges.is_empty() {
            return;
        }
        let label = t!("edit.line_style").to_string();
        let result = self.edit(label, |tx| {
            edges.into_iter().try_for_each(|(edge, route)| {
                let anchors = (route.source_anchor, route.target_anchor);
                tx.set_route(diagram, edge, route.waypoints, anchors)?;
                tx.update_presentation(diagram, edge, |p| {
                    p.style.line_shape = shape;
                    p.style.auto_route = false;
                })
            })
        });
        self.report(result);
    }

    /// Applies text typed in place: member declarations are parsed, note texts, guards and names
    /// set. A message takes `name(arguments)`, a reply its return value.
    pub fn apply_text(&mut self, element: Id, text: &str) {
        let label = t!("edit.properties").to_string();
        let Some(kind) = self.doc.model().get(element).map(|e| e.kind().clone()) else { return };
        let result = self.edit(label, |tx| match kind {
            ElementKind::Comment(_) => tx.update(element, |d| {
                if let ElementKind::Comment(p) = &mut d.kind {
                    p.body = text.to_owned();
                }
            }),
            ElementKind::InteractionOperand(_) => tx.update(element, |d| {
                if let ElementKind::InteractionOperand(p) = &mut d.kind {
                    let guard = text.trim();
                    let guard = guard.strip_prefix('[').unwrap_or(guard);
                    p.guard = guard.strip_suffix(']').unwrap_or(guard).trim().to_owned();
                }
            }),
            ElementKind::InternalTransition(_) => tx.update(element, |d| {
                d.kind = ElementKind::InternalTransition(
                    umlcraft_model::InternalTransitionProps::parse(text),
                );
            }),
            ElementKind::Message(_) => tx.update(element, |d| {
                let text = text.trim();
                let ElementKind::Message(p) = &mut d.kind else { return };
                if p.sort == umlcraft_model::MessageSort::Reply {
                    p.return_value = text.to_owned();
                } else if let Some((name, rest)) = text.split_once('(') {
                    d.name = name.trim().to_owned();
                    p.arguments = rest.strip_suffix(')').unwrap_or(rest).trim().to_owned();
                } else {
                    d.name = text.to_owned();
                }
            }),
            _ => member::apply_member_text(tx, element, text),
        });
        self.report(result);
    }

    /// Sets the value `instance` gives `attribute`, typed in place as `name = value`.
    pub fn set_slot(&mut self, instance: Id, attribute: Id, text: &str) {
        let label = t!("edit.set_slots").to_string();
        let result = self.edit(label, |tx| object::set_slot(tx, instance, attribute, text));
        self.report(result);
    }

    /// Sets the values of an object's slots by attribute name, as one undo step. Names that are
    /// no slot of the object are left out.
    pub fn set_slots(&mut self, instance: Id, values: &[(String, String)]) {
        let model = self.doc.model();
        let changes: Vec<(Id, &str)> = object::slots(model, instance)
            .into_iter()
            .filter_map(|(attribute, old)| {
                let name = model.get(attribute)?.name();
                let (_, value) = values.iter().find(|(n, _)| n == name)?;
                (*value != old).then_some((attribute, value.as_str()))
            })
            .collect();
        if changes.is_empty() {
            return;
        }
        let label = t!("edit.set_slots").to_string();
        let result = self.edit(label, |tx| {
            changes.iter().try_for_each(|&(attribute, value)| {
                object::set_slot(tx, instance, attribute, value)
            })
        });
        self.report(result);
    }

    /// Shows an existing element on the active diagram at `at` and selects it.
    pub fn show_element(&mut self, element: Id, at: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.show").to_string();
        let result = self.edit(label, |tx| {
            // Lines to what the diagram shows already come along, and are routed.
            let edges = |tx: &Tx<'_>| -> HashSet<Id> {
                tx.model().view(diagram).map_or_else(HashSet::new, |v| {
                    v.presentations()
                        .iter()
                        .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
                        .map(|p| p.id)
                        .collect()
                })
            };
            let before = edges(tx);
            let node = palette::show_element(tx, diagram, element, at)?;
            let mut new: Vec<Id> = edges(tx).difference(&before).copied().collect();
            new.sort();
            autoroute::route_new_lines(tx, diagram, &new)?;
            Ok(node)
        });
        if let Some(node) = self.report(result) {
            self.select(vec![node]);
        }
    }

    /// Moves an element into another owner (model tree drag and drop).
    pub fn move_into(&mut self, element: Id, owner: Id) {
        let model = self.doc.model();
        if element == owner || model.get(element).and_then(|e| e.owner()) == Some(owner) {
            return;
        }
        let index = model.children(owner).len();
        let label = t!("edit.move_element").to_string();
        let result = self.edit(label, |tx| tx.move_element(element, owner, index));
        self.report(result);
    }

    /// Copies the selection of the active diagram.
    pub fn copy_selection(&self) -> Option<(Id, Clip)> {
        let diagram = self.active_diagram?;
        clipboard::copy(self.doc.model(), diagram, &self.selection).map(|clip| (diagram, clip))
    }

    /// Pastes `clip` into the active diagram and selects the result.
    pub fn paste(&mut self, clip: &Clip, offset: Point, as_reference: bool) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.paste").to_string();
        let result =
            self.edit(label, |tx| clipboard::paste(tx, diagram, clip, offset, as_reference));
        if let Some(created) = self.report(result) {
            self.select(created);
        }
    }

    /// Puts a message's label where the user dragged it.
    pub fn place_message_label(&mut self, presentation: Id, top_left: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.move").to_string();
        let result = self.edit(label, |tx| {
            umlcraft_notation::sequence_edit::place_label(tx, diagram, presentation, top_left)
        });
        self.report(result);
    }

    /// Puts an edge's «include» or «extend» where the user dragged it, `offset` from the middle
    /// of the line.
    pub fn place_keyword_label(&mut self, presentation: Id, offset: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.move").to_string();
        let result = self.edit(label, |tx| {
            tx.update_presentation(diagram, presentation, |p| {
                if let Shape::Edge { labels, .. } = &mut p.shape {
                    labels.keyword = Some(offset);
                }
            })
        });
        self.report(result);
    }

    pub fn set_bounds(&mut self, presentation: Id, bounds: Rect) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.resize").to_string();
        let sequence = self.doc.model().get(diagram).and_then(|d| d.kind().as_diagram())
            == Some(umlcraft_model::DiagramKind::Sequence);
        let result = self.edit(label, |tx| {
            tx.set_bounds(diagram, presentation, bounds)?;
            // A box sized by hand keeps that size, as in Astah.
            tx.update_presentation(diagram, presentation, |p| p.style.auto_size = false)?;
            // A fragment grown over messages takes them in.
            if sequence {
                umlcraft_notation::sequence_edit::tidy(tx, diagram)?;
            }
            Ok(())
        });
        self.report(result);
    }

    pub fn align_selection(&mut self, alignment: Alignment) {
        let Some(diagram) = self.active_diagram else { return };
        let nodes = self.selected_nodes();
        if nodes.len() < 2 {
            return;
        }
        // Aligned as drawn: a box may be drawn larger than stored, e.g. around a long name.
        let drawn =
            scene::build(self.doc.model(), diagram, &scene::Preview::default(), &*self.measure);
        let rects: Vec<Rect> =
            nodes.iter().map(|&(p, r)| drawn.node(p).map_or(r, |n| n.rect)).collect();
        let aligned = scene::align(&rects, alignment);
        let label = t!("edit.align").to_string();
        let result = self.edit(label, |tx| {
            nodes.iter().zip(rects.iter().zip(aligned)).try_for_each(|(&(p, stored), (d, a))| {
                let moved = stored.translate(a.x - d.x, a.y - d.y);
                tx.set_bounds(diagram, p, moved)
            })
        });
        self.report(result);
    }

    /// Deletes the elements behind the selection from the model; purely graphical items
    /// (note anchors) are removed from the diagram.
    pub fn delete_selection(&mut self) {
        let Some(diagram) = self.active_diagram else { return };
        let Some(view) = self.doc.model().view(diagram) else { return };
        let mut elements = Vec::new();
        let mut graphics = Vec::new();
        for &p in &self.selection {
            match view.get(p).map(|pr| pr.element) {
                Some(Some(e)) => elements.push(e),
                Some(None) => graphics.push(p),
                None => {}
            }
        }
        if elements.is_empty() && graphics.is_empty() {
            return;
        }
        let label = t!("edit.delete").to_string();
        let result = self.edit(label, |tx| {
            for p in graphics {
                if tx.model().view(diagram).and_then(|v| v.get(p)).is_some() {
                    tx.remove_presentation(diagram, p)?;
                }
            }
            for e in elements {
                // An earlier delete may have cascaded to this one already.
                if tx.model().contains(e) {
                    tx.delete(e)?;
                }
            }
            // Bars of deleted messages go too.
            umlcraft_notation::sequence_edit::tidy_all(tx)
        });
        if self.report(result).is_some() {
            self.clear_selection();
        }
    }

    /// Removes the selected presentations from the diagram only; the elements stay.
    pub fn remove_selection_from_diagram(&mut self) {
        let Some(diagram) = self.active_diagram else { return };
        let selection = self.selection.clone();
        let label = t!("edit.remove_from_diagram").to_string();
        let result = self.edit(label, |tx| {
            for p in selection {
                if tx.model().view(diagram).and_then(|v| v.get(p)).is_some() {
                    tx.remove_presentation(diagram, p)?;
                }
            }
            umlcraft_notation::sequence_edit::tidy_all(tx)
        });
        if self.report(result).is_some() {
            self.clear_selection();
        }
    }
}

/// A change to one end of an association, as the canvas's context menu offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndChange {
    Navigability(Navigability),
    Aggregation(Aggregation),
    Multiplicity(Option<Multiplicity>),
}

/// What the model tree's "New" menu can add to an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewItem {
    Element(NodeTool),
    Attribute,
    Operation,
    Literal,
    ExtensionPoint,
    TemplateParameter,
    Diagram(DiagramKind),
    Structogram,
}

impl NewItem {
    /// Whether the menu should offer this item for an element of `kind`.
    pub fn applies_to(self, kind: &ElementKind) -> bool {
        match self {
            Self::Element(tool) => kind.can_own(&tool.element_kind()),
            Self::Attribute => kind.can_own(&ElementKind::attribute(None)),
            Self::Operation => kind.can_own(&ElementKind::operation(None)),
            Self::Literal => kind.can_own(&ElementKind::EnumerationLiteral),
            Self::ExtensionPoint => kind.can_own(&ElementKind::ExtensionPoint),
            Self::TemplateParameter => {
                kind.can_own(&ElementKind::TemplateParameter(TemplateParameterProps::default()))
            }
            Self::Diagram(d) => match edit::behavior_of(d) {
                Some(behavior) => kind.can_own(&behavior),
                None => kind.can_own(&ElementKind::diagram(d)),
            },
            Self::Structogram => kind.can_own(&ElementKind::diagram(DiagramKind::Class)),
        }
    }

    /// Translation key of the item's name.
    pub fn key(self) -> String {
        match self {
            Self::Element(tool) => format!("kind.{}", tool.key()),
            Self::Attribute => "kind.attribute".into(),
            Self::Operation => "kind.operation".into(),
            Self::Literal => "kind.enumerationLiteral".into(),
            Self::ExtensionPoint => "kind.extensionPoint".into(),
            Self::TemplateParameter => "kind.templateParameter".into(),
            Self::Diagram(DiagramKind::Class) => "diagram.class".into(),
            Self::Diagram(DiagramKind::UseCase) => "diagram.useCase".into(),
            Self::Diagram(DiagramKind::Sequence) => "diagram.sequence".into(),
            Self::Diagram(DiagramKind::Activity) => "diagram.activity".into(),
            Self::Diagram(DiagramKind::StateMachine) => "diagram.stateMachine".into(),
            Self::Diagram(DiagramKind::Component) => "diagram.component".into(),
            Self::Diagram(DiagramKind::Deployment) => "diagram.deployment".into(),
            Self::Structogram => "kind.structogram".into(),
        }
    }
}

impl Workspace {
    /// Adds a member written as `text` (`- name : String`, `+ total() : int`, a literal's name)
    /// to `owner`'s `compartment`, right after `after` or else at the end, as one undo step,
    /// and selects it. Returns the new member; nothing is added for an empty text.
    pub fn add_member(
        &mut self,
        owner: Id,
        compartment: scene::Compartment,
        after: Option<Id>,
        text: &str,
    ) -> Option<Id> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let data = match compartment {
            scene::Compartment::Attributes => ElementData::new("", ElementKind::attribute(None))
                .with_visibility(Visibility::Private),
            scene::Compartment::Operations => ElementData::new("", ElementKind::operation(None)),
            scene::Compartment::Literals => ElementData::new(text, ElementKind::EnumerationLiteral),
            scene::Compartment::Slots => return None,
        };
        let literal = compartment == scene::Compartment::Literals;
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| {
            let index = after
                .and_then(|a| tx.model().children(owner).iter().position(|&c| c == a))
                .map(|i| i + 1);
            let member = match index {
                Some(index) => tx.create_at(owner, index, data)?,
                None => tx.create(owner, data)?,
            };
            if !literal {
                member::apply_member_text(tx, member, text)?;
            }
            Ok(member)
        });
        let member = self.report(result)?;
        self.selected_element = Some(member);
        Some(member)
    }

    /// Adds an extension point to the use case `extend` extends and lets `extend` use it, as one
    /// undo step, and selects it. Returns the extension point.
    pub fn add_extension_point_for(&mut self, extend: Id) -> Option<Id> {
        let ElementKind::Extend(p) = self.doc.model().get(extend)?.kind() else { return None };
        let use_case = p.extended;
        let base = name_base(&NewItem::ExtensionPoint.key(), true);
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| {
            let name = palette::unique_name(tx.model(), use_case, &base);
            let point = tx.create(use_case, ElementData::new(name, ElementKind::ExtensionPoint))?;
            tx.update(extend, |data| {
                if let ElementKind::Extend(p) = &mut data.kind {
                    p.extension_points.push(point);
                }
            })?;
            Ok(point)
        });
        let point = self.report(result)?;
        self.selected_element = Some(point);
        Some(point)
    }

    /// Adds a new element below `owner` (model only, no diagram) and selects it. Diagrams are
    /// opened right away.
    /// Creates `item` in `owner` as one undo step and selects it; returns the new element.
    pub fn add_item(&mut self, owner: Id, item: NewItem) -> Option<Id> {
        let keep_words = match item {
            NewItem::Diagram(_) | NewItem::ExtensionPoint => true,
            NewItem::Structogram => false,
            NewItem::Element(tool) => keeps_words(tool),
            _ => false,
        };
        let base = name_base(&item.key(), keep_words);
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| {
            let name = palette::unique_name(tx.model(), owner, &base);
            match item {
                NewItem::Element(tool) => {
                    let name = if tool.is_named() { name } else { String::new() };
                    tx.create(owner, tool.element_data(name))
                }
                NewItem::Attribute => tx.create(
                    owner,
                    ElementData::new(name, ElementKind::attribute(None))
                        .with_visibility(Visibility::Private),
                ),
                NewItem::ExtensionPoint => {
                    tx.create(owner, ElementData::new(name, ElementKind::ExtensionPoint))
                }
                NewItem::Literal => tx.create(
                    owner,
                    ElementData::new(name.to_uppercase(), ElementKind::EnumerationLiteral),
                ),
                NewItem::Operation => {
                    tx.create(owner, ElementData::new(name, ElementKind::operation(None)))
                }
                // Named like type parameters usually are: T, T2, …
                NewItem::TemplateParameter => {
                    let name = clipboard::free_name(tx.model(), owner, "T");
                    let kind = ElementKind::TemplateParameter(TemplateParameterProps::default());
                    tx.create(owner, ElementData::new(name, kind))
                }
                NewItem::Diagram(kind) => edit::create_diagram(tx, owner, kind, &name),
                NewItem::Structogram => {
                    let text = free_structogram_text(tx.model(), owner, &base);
                    let made = umlcraft_nsd::Provenance { author: "", date: "", origin: "" };
                    let program = umlcraft_nsd::RootKind::Program;
                    let structogram = umlcraft_nsd::Diagram::new(program, &[text], &made);
                    let kind = ElementKind::Structogram(StructogramProps::new(&structogram));
                    tx.create(owner, ElementData::new("", kind))
                }
            }
        });
        let id = self.report(result)?;
        if matches!(item, NewItem::Diagram(_) | NewItem::Structogram) {
            self.open_diagram(id);
        }
        self.select_element(id);
        Some(id)
    }

    /// Adds structograms (from `.nsd` files or code) to `owner` as one undo step and opens the
    /// last one. Returns their ids.
    pub fn add_structograms(
        &mut self,
        owner: Id,
        structograms: &[umlcraft_nsd::Diagram],
    ) -> Vec<Id> {
        let label = t!("edit.create").to_string();
        let result = self.edit(label, |tx| {
            structograms
                .iter()
                .map(|d| {
                    let kind = ElementKind::Structogram(StructogramProps::new(d));
                    tx.create(owner, ElementData::new("", kind))
                })
                .collect::<Result<Vec<_>, _>>()
        });
        let ids = self.report(result).unwrap_or_default();
        if let Some(&last) = ids.last() {
            self.open_diagram(last);
            self.select_element(last);
        }
        ids
    }

    /// Replaces the content of structogram `id` as one undo step.
    pub fn edit_structogram(&mut self, id: Id, structogram: &umlcraft_nsd::Diagram) {
        let label = t!("edit.structogram").to_string();
        let result = self.edit(label, |tx| tx.set_structogram(id, structogram));
        self.report(result);
    }

    /// Changes an element's data as one undo step, e.g. from the properties panel.
    pub fn update_element(&mut self, id: Id, f: impl FnOnce(&mut ElementData)) {
        let label = t!("edit.properties").to_string();
        let covered = |model: &umlcraft_model::Model| match model.get(id).map(|e| e.kind()) {
            Some(ElementKind::CombinedFragment(p)) => Some(p.covered.clone()),
            Some(ElementKind::InteractionUse(p)) => Some(p.covered.clone()),
            _ => None,
        };
        let before = covered(self.doc.model());
        let is_alt = |model: &umlcraft_model::Model| {
            matches!(model.get(id).map(|e| e.kind()), Some(ElementKind::CombinedFragment(p))
                if p.operator == umlcraft_model::InteractionOperator::Alt)
        };
        let was_alt = is_alt(self.doc.model());
        let result = self.edit(label, |tx| {
            tx.update(id, f)?;
            // A fragment turned into an alt gets its else.
            if !was_alt && is_alt(tx.model()) {
                palette::add_else(tx, id)?;
            }
            // Frames span the lifelines they cover once the user changes them.
            if covered(tx.model()) != before {
                umlcraft_notation::sequence_edit::cover(tx, id)?;
            }
            // A message's sort decides how bars are fitted around it.
            let sequence = |d: &Id| {
                tx.model().get(*d).and_then(|e| e.kind().as_diagram())
                    == Some(umlcraft_model::DiagramKind::Sequence)
            };
            let diagrams: Vec<Id> =
                tx.model().shown_on(id).into_iter().map(|(d, _)| d).filter(sequence).collect();
            diagrams.into_iter().try_for_each(|d| umlcraft_notation::sequence_edit::tidy(tx, d))
        });
        self.report(result);
    }

    /// Makes a class bind a template (`Pair<int,Holder>`), or nothing, with its «bind» line on
    /// the open diagram.
    pub fn set_binding(&mut self, class: Id, binding: Option<TypeRef>) {
        let label = t!("edit.properties").to_string();
        let diagram = self.active_diagram;
        let result = self.edit(label, |tx| palette::set_binding(tx, diagram, class, binding));
        self.report(result);
    }

    /// Deletes one element from the model (with everything that depends on it).
    pub fn delete_element(&mut self, id: Id) {
        let label = t!("edit.delete").to_string();
        let kind = self.doc.model().get(id).map(|e| e.kind().clone());
        let result = self.edit(label, |tx| {
            match kind {
                // The neighbors of a region take its room.
                Some(ElementKind::Region) => state_edit::delete_region(tx, id)?,
                Some(ElementKind::InteractionOperand(_)) => {
                    umlcraft_notation::sequence_edit::delete_operand(tx, id)?
                }
                _ => tx.delete(id)?,
            }
            umlcraft_notation::sequence_edit::tidy_all(tx)
        });
        if self.report(result).is_some() {
            self.validate();
            if self.selected_element.is_none() {
                self.selection.clear();
            }
        }
    }
}

/// Whether names generated for `tool` keep their words apart, as phrases do ("Use case 1").
fn keeps_words(tool: NodeTool) -> bool {
    matches!(
        tool,
        NodeTool::UseCase
            | NodeTool::SystemBoundary
            | NodeTool::Action
            | NodeTool::ObjectNode
            | NodeTool::Partition
            | NodeTool::State
    )
}

/// Base for generated names. Class-like names join the words ("Class1"); use cases and diagrams
/// keep them and get a space before the number ("Use case 1"), the way people write them.
pub fn name_base(key: &str, keep_words: bool) -> String {
    let text = t!(key).to_string();
    if keep_words { format!("{text} ") } else { text.replace(' ', "") }
}

fn first_diagram(model: &Model) -> Option<Id> {
    model.subtree(model.root()).into_iter().find(|&id| is_shown(model, id))
}

/// Whether `id` opens as a tab: a diagram, or a structogram.
pub fn is_shown(model: &Model, id: Id) -> bool {
    model.view(id).is_some() || is_structogram(model, id)
}

pub fn is_structogram(model: &Model, id: Id) -> bool {
    model.get(id).is_some_and(|e| matches!(e.kind(), ElementKind::Structogram(_)))
}

/// A new structogram's text: the item's name and the first number no structogram of `owner`
/// has yet ("Structogram1"). Structograms have no names; their text names them.
fn free_structogram_text(model: &Model, owner: Id, base: &str) -> String {
    let taken: HashSet<String> = model
        .children(owner)
        .iter()
        .filter_map(|&c| model.get(c)?.kind().as_structogram())
        .map(|p| {
            p.structogram.text().join(
                "
",
            )
        })
        .collect();
    (1..)
        .map(|n| format!("{base}{n}"))
        .find(|t| !taken.contains(t))
        .expect("numbers do not run out")
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;

    use super::*;

    #[test]
    fn align_lines_up_the_boxes_as_drawn() {
        let (doc, ids) = order_system();
        let mut ws = Workspace::new(doc, None);
        let d = ids.class_diagram;
        ws.open_diagram(d);
        let node = |ws: &Workspace, element: Id| ws.doc.model().shown_on(element)[0].1;
        let (customer, order) = (node(&ws, ids.customer), node(&ws, ids.order));
        // A small stored box drawn wider around a long name.
        ws.select(vec![order]);
        ws.edit("tiny".into(), |tx| {
            tx.rename(ids.order, "AnOrderWithAVeryLongName")?;
            tx.update_presentation(d, order, |p| {
                p.style.auto_size = false;
                if let Shape::Node { bounds, .. } = &mut p.shape {
                    (bounds.width, bounds.height) = (40.0, 30.0);
                }
            })
        })
        .unwrap();
        let stored = ws.doc.model().view(d).unwrap().get(order).unwrap().bounds().unwrap();
        let drawn = scene::build(ws.doc.model(), d, &scene::Preview::default(), &*ws.measure);
        assert!(drawn.node(order).unwrap().rect.width > stored.width, "drawn wider than stored");
        ws.select(vec![customer, order]);
        ws.align_selection(Alignment::Right);
        let scene = scene::build(ws.doc.model(), d, &scene::Preview::default(), &*ws.measure);
        let right = |p: Id| scene.node(p).unwrap().rect.right();
        assert_eq!(right(order), right(customer));
    }

    #[test]
    fn boxes_sized_by_content_fit_when_edited() {
        let (doc, ids) = order_system();
        let mut ws = Workspace::new(doc, None);
        let d = ids.class_diagram;
        ws.open_diagram(d);
        let bounds = |ws: &Workspace, id: Id| ws.doc.model().view(d).unwrap().get(id)?.bounds();
        let fitted = |ws: &Workspace, id: Id| {
            let p = ws.doc.model().view(d).unwrap().get(id).unwrap();
            scene::fitted_size(ws.doc.model(), p, &ApproxMeasure).unwrap()
        };

        // A new class takes its fitted size instead of the palette's.
        ws.create_node(NodeTool::Class, Point::new(500.0, 400.0));
        let class = ws.selection[0];
        let element = ws.selected_element.unwrap();
        assert_eq!(bounds(&ws, class).unwrap().size(), fitted(&ws, class));
        // It grows to the right with a longer name.
        ws.apply_text(element, "AVeryMuchLongerClassName");
        let grown = bounds(&ws, class).unwrap();
        assert_eq!((grown.x, grown.size()), (500.0, fitted(&ws, class)));

        // A box sized by content whose stored size is off (as Astah may store it) stays as it
        // is until its own content changes.
        let customer = ws.doc.model().view(d).unwrap().presentations_of(ids.customer).next();
        let customer = customer.unwrap().id;
        ws.doc
            .edit("auto", |tx| tx.update_presentation(d, customer, |p| p.style.auto_size = true))
            .unwrap();
        let stale = bounds(&ws, customer).unwrap();
        ws.apply_text(element, "Short");
        assert_eq!(bounds(&ws, customer), Some(stale));
        ws.apply_text(ids.customer, "Client");
        assert_eq!(bounds(&ws, customer).unwrap().size(), fitted(&ws, customer));

        // Resizing by hand ends the automatic size.
        let manual = Rect::new(500.0, 400.0, 300.0, 120.0);
        ws.set_bounds(class, manual);
        ws.apply_text(element, "X");
        assert_eq!(bounds(&ws, class), Some(manual));
    }

    #[test]
    fn free_texts_and_lines_are_created_edited_and_moved() {
        let (doc, ids) = order_system();
        let mut ws = Workspace::new(doc, None);
        let d = ids.sequence_diagram;
        ws.open_diagram(d);
        let shape =
            |ws: &Workspace, id: Id| ws.doc.model().view(d).unwrap().get(id).unwrap().shape.clone();

        let text = ws.create_free_text(Point::new(40.0, 400.0)).unwrap();
        assert_eq!(ws.selection, vec![text]);
        ws.set_free_text(text, "first\nsecond line");
        let Shape::Text { bounds, text: written } = shape(&ws, text) else { panic!() };
        assert_eq!(written, "first\nsecond line");
        // As wide as the widest line plus 20, 8 higher than two lines.
        let widest = ApproxMeasure.width("second line", scene::FONT_SIZE, false);
        assert!((bounds.width - (widest + 20.0)).abs() < 1e-3, "{bounds:?}");
        assert!((bounds.height - (8.0 + 2.0 * scene::line_height(scene::FONT_SIZE))).abs() < 1e-3);

        ws.create_free_line(Point::new(10.0, 300.0), Point::new(150.0, 300.0));
        let line = ws.selection[0];
        ws.update_free_line(line, |dashed, start, _| (*dashed, *start) = (true, true));
        ws.move_nodes(&[text, line], 5.0, 10.0);
        let Shape::Line { from, to, dashed, start_arrow, end_arrow } = shape(&ws, line) else {
            panic!()
        };
        assert_eq!((from, to), (Point::new(15.0, 310.0), Point::new(155.0, 310.0)));
        assert_eq!((dashed, start_arrow, end_arrow), (true, true, false));
        assert_eq!(
            shape(&ws, text).clone(),
            Shape::Text {
                bounds: Rect::new(45.0, 410.0, bounds.width, bounds.height),
                text: "first\nsecond line".into(),
            }
        );
        // Undo takes back the move.
        ws.doc.undo();
        let Shape::Line { from, .. } = shape(&ws, line) else { panic!() };
        assert_eq!(from, Point::new(10.0, 300.0));
        // One end moves on its own; a grip clicked without dragging adds no undo step.
        ws.set_free_line_end(line, 1, Point::new(150.0, 360.0));
        let Shape::Line { from, to, .. } = shape(&ws, line) else { panic!() };
        assert_eq!((from, to), (Point::new(10.0, 300.0), Point::new(150.0, 360.0)));
        ws.set_free_line_end(line, 0, from);
        ws.doc.undo();
        let Shape::Line { to, .. } = shape(&ws, line) else { panic!() };
        assert_eq!(to, Point::new(150.0, 300.0));
    }

    #[test]
    fn context_menu_edits_of_edges() {
        let (doc, ids) = order_system();
        let mut ws = Workspace::new(doc, None);
        let d = ids.class_diagram;
        ws.open_diagram(d);
        let edge = |ws: &Workspace| {
            let view = ws.doc.model().view(d).unwrap();
            view.presentations_of(ids.order_customer).next().unwrap().clone()
        };
        let id = edge(&ws).id;
        ws.doc
            .edit("place", |tx| {
                tx.update_presentation(d, id, |p| {
                    if let Shape::Edge { source_anchor, labels, .. } = &mut p.shape {
                        *source_anchor = Some(Point::new(0.2, 0.8));
                        labels.name = Some(Point::new(5.0, 5.0));
                    }
                })
            })
            .unwrap();

        ws.reset_line_ends(&[id]);
        ws.reset_labels(&[id]);
        let Shape::Edge { source_anchor, labels, .. } = edge(&ws).shape else { unreachable!() };
        assert_eq!((source_anchor, labels), (None, EdgeLabels::default()));

        ws.set_association_end(ids.order_customer, 1, EndChange::Aggregation(Aggregation::Shared));
        ws.set_association_end(ids.order_customer, 1, EndChange::Multiplicity(None));
        let association = ws.doc.model().get(ids.order_customer).unwrap();
        let ElementKind::Association(p) = association.kind() else { unreachable!() };
        assert_eq!((p.ends[1].aggregation, p.ends[1].multiplicity), (Aggregation::Shared, None));
        // Two undo steps for the end, one each for labels and line ends.
        assert!(ws.doc.undo() && ws.doc.undo() && ws.doc.undo() && ws.doc.undo());
        let Shape::Edge { source_anchor, .. } = edge(&ws).shape else { unreachable!() };
        assert_eq!(source_anchor, Some(Point::new(0.2, 0.8)));
    }

    #[test]
    fn members_ends_and_extension_points_typed_on_the_diagram() {
        let (doc, ids) = order_system();
        let mut ws = Workspace::new(doc, None);
        ws.open_diagram(ids.class_diagram);
        let text = |ws: &Workspace, id: Id| member::member_text(ws.doc.model(), id);
        let attributes = scene::Compartment::Attributes;
        let first = ws.add_member(ids.customer, attributes, None, " email : String ").unwrap();
        assert_eq!(text(&ws, first), "- email : String");
        // Right after the one it follows, which here is the first.
        let second = ws.add_member(ids.customer, attributes, Some(first), "+ age : int").unwrap();
        let children = ws.doc.model().children(ids.customer);
        let at = |id| children.iter().position(|&c| c == id).unwrap();
        assert_eq!(at(second), at(first) + 1);
        assert_eq!(ws.selected_element, Some(second));
        // An empty row adds nothing.
        assert_eq!(ws.add_member(ids.customer, attributes, None, "  "), None);
        let operations = scene::Compartment::Operations;
        let total = ws.add_member(ids.customer, operations, None, "+ total() : int").unwrap();
        assert_eq!(text(&ws, total), "+ total() : int");

        ws.set_member_type(second, TypeRef::Element(ids.order));
        assert_eq!(text(&ws, second), "+ age : Order");

        ws.set_end_text(ids.order_customer, 0, false, "# orders {ordered}");
        let role = |ws: &Workspace| match ws.doc.model().get(ids.order_customer).unwrap().kind() {
            ElementKind::Association(p) => member::role_text(&p.ends[0]),
            _ => unreachable!(),
        };
        assert_eq!(role(&ws), "# orders {ordered}");
        ws.set_end_text(ids.order_customer, 0, false, " orders ");
        assert_eq!(role(&ws), "# orders");
        ws.set_end_text(ids.order_customer, 0, true, "0..*");
        let end = |ws: &Workspace| match ws.doc.model().get(ids.order_customer).unwrap().kind() {
            ElementKind::Association(p) => p.ends[0].clone(),
            _ => unreachable!(),
        };
        assert_eq!(
            (end(&ws).role.as_str(), end(&ws).multiplicity),
            ("orders", "0..*".parse().ok())
        );
        // Text that is no multiplicity is refused with a message.
        ws.set_end_text(ids.order_customer, 0, true, "many");
        assert!(ws.message.is_some());
        assert_eq!(end(&ws).multiplicity, "0..*".parse().ok());

        let (extend, use_case) = ws
            .doc
            .edit("extend", |tx| {
                let a = tx.create(ids.domain, ElementData::new("A", ElementKind::use_case()))?;
                let b = tx.create(ids.domain, ElementData::new("B", ElementKind::use_case()))?;
                let kind = palette::EdgeTool::Extend.element_kind(a, b).unwrap();
                Ok((tx.create(ids.domain, ElementData::new("", kind))?, b))
            })
            .unwrap();
        let point = ws.add_extension_point_for(extend).unwrap();
        let model = ws.doc.model();
        assert_eq!(model.get(point).unwrap().owner(), Some(use_case));
        let ElementKind::Extend(p) = model.get(extend).unwrap().kind() else { unreachable!() };
        assert_eq!(p.extension_points, [point]);
        assert!(ws.doc.undo());
        assert!(ws.doc.model().get(point).is_none());
    }
}
