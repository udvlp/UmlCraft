use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rust_i18n::t;
use umlcraft_model::{
    DiagramKind, Document, ElementData, ElementKind, Id, Model, ModelError, Point, Rect, Shape,
    Style, Visibility,
};
use umlcraft_notation::clipboard::{self, Clip};
use umlcraft_notation::member;
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Alignment};
use umlcraft_notation::sequence::Slot;

/// The open project and what the user is looking at. Shared by all panels as a GPUI entity.
pub struct Workspace {
    pub doc: Document,
    pub path: Option<PathBuf>,
    /// Diagrams open as tabs, in tab order.
    pub open_diagrams: Vec<Id>,
    pub active_diagram: Option<Id>,
    /// Element shown in the properties panel and highlighted in the model tree.
    pub selected_element: Option<Id>,
    /// Selected presentations of the active diagram; the first one is the alignment reference.
    pub selection: Vec<Id>,
    pub tool: Tool,
    /// Last problem to show in the status bar, e.g. a refused connection.
    pub message: Option<String>,
}

impl Workspace {
    pub fn new(doc: Document, path: Option<PathBuf>) -> Self {
        let first_diagram = first_diagram(doc.model());
        Self {
            doc,
            path,
            open_diagrams: first_diagram.into_iter().collect(),
            active_diagram: first_diagram,
            selected_element: None,
            selection: Vec::new(),
            tool: Tool::Select,
            message: None,
        }
    }

    pub fn file_name(&self) -> Option<String> {
        self.path.as_deref().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned())
    }

    /// Opens `diagram` as a tab (if needed) and activates it.
    pub fn open_diagram(&mut self, diagram: Id) {
        if self.doc.model().view(diagram).is_none() {
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

    pub fn close_diagram(&mut self, diagram: Id) {
        let Some(ix) = self.open_diagrams.iter().position(|&d| d == diagram) else { return };
        self.open_diagrams.remove(ix);
        if self.active_diagram == Some(diagram) {
            let next = self.open_diagrams.get(ix).or(self.open_diagrams.last()).copied();
            self.active_diagram = next;
            self.selection.clear();
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
        self.open_diagrams.retain(|&d| model.view(d).is_some());
        if self.active_diagram.is_none_or(|d| model.view(d).is_none()) {
            self.active_diagram = self.open_diagrams.first().copied();
        }
        if self.selected_element.is_some_and(|e| !model.contains(e)) {
            self.selected_element = None;
        }
        let view = self.active_diagram.and_then(|d| model.view(d));
        self.selection.retain(|&p| view.is_some_and(|v| v.get(p).is_some()));
    }

    fn report<T>(&mut self, result: Result<T, ModelError>) -> Option<T> {
        match result {
            Ok(value) => {
                self.message = None;
                Some(value)
            }
            Err(err) => {
                self.message = Some(err.to_string());
                None
            }
        }
    }

    /// Creates an element with `tool` at `origin` on the active diagram and selects it.
    pub fn create_node(&mut self, tool: NodeTool, origin: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let base = name_base(&format!("kind.{}", tool.key()), keeps_words(tool));
        let label = t!("edit.create").to_string();
        let result =
            self.doc.edit(label, |tx| palette::create_node(tx, diagram, tool, origin, &base));
        if let Some((element, node)) = self.report(result) {
            self.selection = vec![node];
            self.selected_element = Some(element);
        }
    }

    pub fn connect(&mut self, tool: EdgeTool, source: Id, target: Id) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.connect").to_string();
        let result = self.doc.edit(label, |tx| palette::connect(tx, diagram, tool, source, target));
        if let Some(edge) = self.report(result) {
            self.select(vec![edge]);
        }
    }

    /// Draws a message on the active sequence diagram at `slot` and selects it.
    pub fn connect_message(&mut self, tool: EdgeTool, source: Id, target: Id, slot: Option<Slot>) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.connect").to_string();
        let result = self
            .doc
            .edit(label, |tx| palette::connect_message(tx, diagram, tool, source, target, slot));
        if let Some(edge) = self.report(result) {
            self.select(vec![edge]);
        }
    }

    /// Adds a combined fragment to the active sequence diagram and selects it.
    pub fn create_fragment(&mut self, slot: Option<Slot>, covered: Vec<Id>, origin: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.create").to_string();
        let result =
            self.doc.edit(label, |tx| palette::create_fragment(tx, diagram, slot, covered, origin));
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
        let result = self.doc.edit(label, |tx| palette::wrap_in_fragment(tx, diagram, &items));
        if let Some((fragment, node)) = self.report(result) {
            self.selection = vec![node];
            self.selected_element = Some(fragment);
        }
    }

    /// Moves a message or fragment to another place in its interaction.
    pub fn move_to_slot(&mut self, item: Id, slot: Slot) {
        let label = t!("edit.move_element").to_string();
        let result = self.doc.edit(label, |tx| palette::move_to_slot(tx, item, slot));
        self.report(result);
    }

    /// Adds an operand to a combined fragment and selects it.
    pub fn add_operand(&mut self, fragment: Id) {
        let label = t!("edit.create").to_string();
        let result = self.doc.edit(label, |tx| palette::add_operand(tx, fragment));
        if let Some(operand) = self.report(result) {
            self.selected_element = Some(operand);
        }
    }

    /// Moves the given nodes of the active diagram by (`dx`, `dy`) as one undo step.
    pub fn move_nodes(&mut self, presentations: &[Id], dx: f32, dy: f32) {
        let Some(diagram) = self.active_diagram else { return };
        let model = self.doc.model();
        let Some(view) = model.view(diagram) else { return };
        let kind_of = |p: &umlcraft_model::Presentation| p.element.and_then(|e| model.get(e));
        // New bounds per node. On sequence diagrams lifelines only move sideways, and fragments
        // follow their content.
        let nodes: Vec<(Id, Rect)> = presentations
            .iter()
            .filter_map(|&p| view.get(p))
            .filter_map(|p| {
                let bounds = p.bounds()?;
                match kind_of(p).map(|e| e.kind()) {
                    Some(ElementKind::CombinedFragment(_)) => None,
                    Some(ElementKind::Lifeline(_)) => Some((p.id, bounds.translate(dx, 0.0))),
                    _ => Some((p.id, bounds.translate(dx, dy))),
                }
            })
            .collect();
        if nodes.is_empty() || (dx == 0.0 && dy == 0.0) {
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
        let result = self.doc.edit(label, |tx| {
            nodes.iter().try_for_each(|&(p, b)| tx.set_bounds(diagram, p, b))?;
            edges.into_iter().try_for_each(|(e, w)| tx.set_waypoints(diagram, e, w))
        });
        self.report(result);
    }

    /// Changes the display options of one presentation on the active diagram.
    pub fn update_style(&mut self, presentation: Id, f: impl FnOnce(&mut Style)) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.properties").to_string();
        let result = self
            .doc
            .edit(label, |tx| tx.update_presentation(diagram, presentation, |p| f(&mut p.style)));
        self.report(result);
    }

    /// Reports a problem in the status bar.
    pub fn complain(&mut self, message: impl Into<String>) {
        self.message = Some(message.into());
    }

    pub fn set_waypoints(&mut self, edge: Id, waypoints: Vec<Point>) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.waypoints").to_string();
        let result = self.doc.edit(label, |tx| tx.set_waypoints(diagram, edge, waypoints));
        self.report(result);
    }

    /// Applies text typed in place: member declarations are parsed, note texts, guards and names
    /// set. A message takes `name(arguments)`, a reply its return value.
    pub fn apply_text(&mut self, element: Id, text: &str) {
        let label = t!("edit.properties").to_string();
        let Some(kind) = self.doc.model().get(element).map(|e| e.kind().clone()) else { return };
        let result = self.doc.edit(label, |tx| match kind {
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

    /// Shows an existing element on the active diagram at `at` and selects it.
    pub fn show_element(&mut self, element: Id, at: Point) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.show").to_string();
        let result = self.doc.edit(label, |tx| palette::show_element(tx, diagram, element, at));
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
        let result = self.doc.edit(label, |tx| tx.move_element(element, owner, index));
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
            self.doc.edit(label, |tx| clipboard::paste(tx, diagram, clip, offset, as_reference));
        if let Some(created) = self.report(result) {
            self.select(created);
        }
    }

    pub fn set_bounds(&mut self, presentation: Id, bounds: Rect) {
        let Some(diagram) = self.active_diagram else { return };
        let label = t!("edit.resize").to_string();
        let result = self.doc.edit(label, |tx| tx.set_bounds(diagram, presentation, bounds));
        self.report(result);
    }

    pub fn align_selection(&mut self, alignment: Alignment) {
        let Some(diagram) = self.active_diagram else { return };
        let nodes = self.selected_nodes();
        if nodes.len() < 2 {
            return;
        }
        let rects: Vec<Rect> = nodes.iter().map(|&(_, r)| r).collect();
        let aligned = scene::align(&rects, alignment);
        let label = t!("edit.align").to_string();
        let result = self.doc.edit(label, |tx| {
            nodes.iter().zip(aligned).try_for_each(|(&(p, _), r)| tx.set_bounds(diagram, p, r))
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
        let result = self.doc.edit(label, |tx| {
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
            Ok(())
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
        let result = self.doc.edit(label, |tx| {
            for p in selection {
                if tx.model().view(diagram).and_then(|v| v.get(p)).is_some() {
                    tx.remove_presentation(diagram, p)?;
                }
            }
            Ok(())
        });
        if self.report(result).is_some() {
            self.clear_selection();
        }
    }
}

/// What the model tree's "New" menu can add to an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewItem {
    Element(NodeTool),
    Attribute,
    Operation,
    Literal,
    ExtensionPoint,
    Diagram(DiagramKind),
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
            Self::Diagram(d) => match behavior_of(d) {
                Some(behavior) => kind.can_own(&behavior),
                None => kind.can_own(&ElementKind::diagram(d)),
            },
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
            Self::Diagram(DiagramKind::Class) => "diagram.class".into(),
            Self::Diagram(DiagramKind::UseCase) => "diagram.useCase".into(),
            Self::Diagram(DiagramKind::Sequence) => "diagram.sequence".into(),
            Self::Diagram(DiagramKind::Activity) => "diagram.activity".into(),
            Self::Diagram(DiagramKind::StateMachine) => "diagram.stateMachine".into(),
        }
    }
}

/// The behavior a new diagram of `kind` brings along and lives in: sequence diagrams an
/// interaction, activity diagrams an activity, state machine diagrams a state machine.
fn behavior_of(kind: DiagramKind) -> Option<ElementKind> {
    match kind {
        DiagramKind::Sequence => Some(ElementKind::Interaction),
        DiagramKind::Activity => Some(ElementKind::Activity),
        DiagramKind::StateMachine => Some(ElementKind::StateMachine),
        DiagramKind::Class | DiagramKind::UseCase => None,
    }
}

impl Workspace {
    /// Adds a new element below `owner` (model only, no diagram) and selects it. Diagrams are
    /// opened right away.
    pub fn add_item(&mut self, owner: Id, item: NewItem) {
        let keep_words = match item {
            NewItem::Diagram(_) | NewItem::ExtensionPoint => true,
            NewItem::Element(tool) => keeps_words(tool),
            _ => false,
        };
        let base = name_base(&item.key(), keep_words);
        let label = t!("edit.create").to_string();
        let result = self.doc.edit(label, |tx| {
            let name = palette::unique_name(tx.model(), owner, &base);
            match item {
                NewItem::Element(tool) => {
                    let name = if tool.is_named() { name } else { String::new() };
                    tx.create(owner, ElementData::new(name, tool.element_kind()))
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
                NewItem::Diagram(kind) => {
                    let diagram = ElementData::new(name.clone(), ElementKind::diagram(kind));
                    match behavior_of(kind) {
                        Some(behavior) => {
                            let behavior = tx.create(owner, ElementData::new(name, behavior))?;
                            tx.create(behavior, diagram)
                        }
                        None => tx.create(owner, diagram),
                    }
                }
            }
        });
        if let Some(id) = self.report(result) {
            if matches!(item, NewItem::Diagram(_)) {
                self.open_diagram(id);
            }
            self.select_element(id);
        }
    }

    /// Changes an element's data as one undo step, e.g. from the properties panel.
    pub fn update_element(&mut self, id: Id, f: impl FnOnce(&mut ElementData)) {
        let label = t!("edit.properties").to_string();
        let result = self.doc.edit(label, |tx| tx.update(id, f));
        self.report(result);
    }

    /// Deletes one element from the model (with everything that depends on it).
    pub fn delete_element(&mut self, id: Id) {
        let label = t!("edit.delete").to_string();
        let result = self.doc.edit(label, |tx| tx.delete(id));
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
    model.subtree(model.root()).into_iter().find(|&id| model.view(id).is_some())
}
