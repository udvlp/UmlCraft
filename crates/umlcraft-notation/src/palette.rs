//! Diagram tools: which elements a diagram offers, which connections are allowed, and the edits
//! that create them.

use umlcraft_model::{
    ClassifierProps, CommentProps, DiagramKind, DirectedProps, ElementData, ElementKind,
    ExtendProps, Id, IncludeProps, LifelineProps, Model, ModelError, Point, Rect, Result, Size, Tx,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NodeTool {
    Class,
    Interface,
    Enumeration,
    Package,
    Note,
    Actor,
    UseCase,
    Lifeline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeTool {
    Association,
    Generalization,
    Realization,
    Dependency,
    Include,
    Extend,
    NoteAnchor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Tool {
    #[default]
    Select,
    Node(NodeTool),
    Edge(EdgeTool),
}

impl Tool {
    /// Stable key, e.g. for translations (`tool.<key>`).
    pub fn key(self) -> &'static str {
        match self {
            Self::Select => "select",
            Self::Node(n) => n.key(),
            Self::Edge(e) => e.key(),
        }
    }
}

/// Tools offered for a diagram kind, in palette order.
pub fn palette(kind: DiagramKind) -> Vec<Tool> {
    use EdgeTool as E;
    use NodeTool as N;
    let (nodes, edges): (&[NodeTool], &[EdgeTool]) = match kind {
        DiagramKind::Class => (
            &[N::Class, N::Interface, N::Enumeration, N::Package, N::Note],
            &[E::Association, E::Generalization, E::Realization, E::Dependency, E::NoteAnchor],
        ),
        DiagramKind::UseCase => (
            &[N::Actor, N::UseCase, N::Note],
            &[
                E::Association,
                E::Generalization,
                E::Include,
                E::Extend,
                E::Dependency,
                E::NoteAnchor,
            ],
        ),
        DiagramKind::Sequence => (&[N::Lifeline, N::Note], &[E::NoteAnchor]),
    };
    std::iter::once(Tool::Select)
        .chain(nodes.iter().map(|&n| Tool::Node(n)))
        .chain(edges.iter().map(|&e| Tool::Edge(e)))
        .collect()
}

impl NodeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Enumeration => "enumeration",
            Self::Package => "package",
            Self::Note => "comment",
            Self::Actor => "actor",
            Self::UseCase => "useCase",
            Self::Lifeline => "lifeline",
        }
    }

    pub fn element_kind(self) -> ElementKind {
        let classifier = ClassifierProps::default();
        match self {
            Self::Class => ElementKind::Class(classifier),
            Self::Interface => ElementKind::Interface(classifier),
            Self::Enumeration => ElementKind::Enumeration(classifier),
            Self::Package => ElementKind::Package,
            Self::Note => ElementKind::Comment(CommentProps::default()),
            Self::Actor => ElementKind::Actor(classifier),
            Self::UseCase => ElementKind::UseCase(classifier),
            Self::Lifeline => ElementKind::Lifeline(LifelineProps::default()),
        }
    }

    pub fn default_size(self) -> Size {
        match self {
            Self::Class => Size::new(160.0, 70.0),
            Self::Interface | Self::Enumeration => Size::new(160.0, 60.0),
            Self::Package => Size::new(200.0, 140.0),
            Self::Note => Size::new(160.0, 60.0),
            Self::Actor => Size::new(40.0, 80.0),
            Self::UseCase => Size::new(140.0, 60.0),
            Self::Lifeline => Size::new(100.0, 300.0),
        }
    }
}

fn is_class_like(kind: &ElementKind) -> bool {
    matches!(
        kind,
        ElementKind::Class(_)
            | ElementKind::Interface(_)
            | ElementKind::Enumeration(_)
            | ElementKind::DataType(_)
    )
}

fn is_dependable(kind: &ElementKind) -> bool {
    kind.is_classifier() || matches!(kind, ElementKind::Package)
}

impl EdgeTool {
    pub fn key(self) -> &'static str {
        match self {
            Self::Association => "association",
            Self::Generalization => "generalization",
            Self::Realization => "realization",
            Self::Dependency => "dependency",
            Self::Include => "include",
            Self::Extend => "extend",
            Self::NoteAnchor => "noteAnchor",
        }
    }

    /// Whether this tool may connect an element of kind `source` to one of kind `target`.
    /// `same` tells whether both ends are the same element.
    pub fn accepts(self, source: &ElementKind, target: &ElementKind, same: bool) -> bool {
        use ElementKind as K;
        match self {
            Self::Association => source.is_classifier() && target.is_classifier(),
            Self::Generalization => {
                !same
                    && std::mem::discriminant(source) == std::mem::discriminant(target)
                    && (is_class_like(source) || matches!(source, K::Actor(_) | K::UseCase(_)))
            }
            Self::Realization => {
                !same
                    && matches!(source, K::Class(_) | K::Enumeration(_) | K::DataType(_))
                    && matches!(target, K::Interface(_))
            }
            Self::Dependency => !same && is_dependable(source) && is_dependable(target),
            Self::Include | Self::Extend => {
                !same && matches!(source, K::UseCase(_)) && matches!(target, K::UseCase(_))
            }
            Self::NoteAnchor => {
                matches!(source, K::Comment(_)) != matches!(target, K::Comment(_))
                    && !matches!(source, K::Diagram(_) | K::Model)
                    && !matches!(target, K::Diagram(_) | K::Model)
            }
        }
    }

    /// The relationship element this tool creates, `None` for purely graphical note anchors.
    pub fn element_kind(self, source: Id, target: Id) -> Option<ElementKind> {
        let directed = DirectedProps { client: source, supplier: target };
        match self {
            Self::Association => Some(ElementKind::association(source, target)),
            Self::Generalization => Some(ElementKind::generalization(source, target)),
            Self::Realization => Some(ElementKind::Realization(directed)),
            Self::Dependency => Some(ElementKind::Dependency(directed)),
            Self::Include => {
                Some(ElementKind::Include(IncludeProps { including: source, addition: target }))
            }
            Self::Extend => Some(ElementKind::Extend(ExtendProps {
                extension: source,
                extended: target,
                extension_points: Vec::new(),
                condition: String::new(),
            })),
            Self::NoteAnchor => None,
        }
    }
}

/// Whether an existing element of `kind` may be shown on a diagram of kind `diagram`.
pub fn can_show(diagram: DiagramKind, kind: &ElementKind) -> bool {
    match diagram {
        DiagramKind::Class => {
            is_class_like(kind) || matches!(kind, ElementKind::Package | ElementKind::Comment(_))
        }
        DiagramKind::UseCase => matches!(
            kind,
            ElementKind::Actor(_) | ElementKind::UseCase(_) | ElementKind::Comment(_)
        ),
        DiagramKind::Sequence => {
            matches!(kind, ElementKind::Lifeline(_) | ElementKind::Comment(_))
        }
    }
}

/// Default node size for showing an existing element.
pub fn default_size_for(kind: &ElementKind) -> Size {
    let tool = match kind {
        ElementKind::Interface(_) => NodeTool::Interface,
        ElementKind::Enumeration(_) => NodeTool::Enumeration,
        ElementKind::Package => NodeTool::Package,
        ElementKind::Comment(_) => NodeTool::Note,
        ElementKind::Actor(_) => NodeTool::Actor,
        ElementKind::UseCase(_) => NodeTool::UseCase,
        ElementKind::Lifeline(_) => NodeTool::Lifeline,
        _ => NodeTool::Class,
    };
    tool.default_size()
}

/// Shows an existing element on `diagram` at `origin`, together with edges for its binary
/// relationships to elements already on the diagram. Returns the new node.
pub fn show_element(tx: &mut Tx<'_>, diagram: Id, element: Id, origin: Point) -> Result<Id> {
    let model = tx.model();
    let diagram_kind =
        model.element(diagram)?.kind().as_diagram().ok_or(ModelError::NotADiagram(diagram))?;
    let kind = model.element(element)?.kind().clone();
    if !can_show(diagram_kind, &kind) {
        return Err(ModelError::Invalid(format!(
            "a {} cannot be shown on this diagram",
            kind.type_name()
        )));
    }
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    if view.presentations_of(element).next().is_some() {
        return Err(ModelError::Invalid("the element is already on this diagram".into()));
    }
    // Relationships connecting the element to something shown, as (relationship, source, target)
    // with `None` standing for the new node.
    let mut edges = Vec::new();
    for e in model.elements().filter(|e| e.kind().is_relationship()) {
        let ends: Vec<Id> = e
            .kind()
            .references()
            .iter()
            .filter(|(_, r)| r.is_strong())
            .map(|(id, _)| *id)
            .collect();
        let [a, b] = ends[..] else { continue };
        if view.presentations_of(e.id()).next().is_some() || (a != element && b != element) {
            continue;
        }
        let end = |id: Id| -> Option<Option<Id>> {
            if id == element {
                Some(None)
            } else {
                view.presentations_of(id).next().map(|p| Some(p.id))
            }
        };
        if let (Some(source), Some(target)) = (end(a), end(b)) {
            edges.push((e.id(), source, target));
        }
    }
    let node = tx.add_node(
        diagram,
        Some(element),
        Rect::from_origin_size(origin, default_size_for(&kind)),
    )?;
    for (relationship, source, target) in edges {
        let waypoints = if source == target {
            crate::scene::self_loop(Rect::from_origin_size(origin, default_size_for(&kind)))
        } else {
            Vec::new()
        };
        tx.add_edge(
            diagram,
            Some(relationship),
            source.unwrap_or(node),
            target.unwrap_or(node),
            waypoints,
        )?;
    }
    Ok(node)
}

/// The closest element, starting at `start` and walking up the owners, that may own `kind`.
pub fn owner_for(model: &Model, start: Id, kind: &ElementKind) -> Option<Id> {
    let mut current = Some(start);
    while let Some(id) = current {
        let element = model.get(id)?;
        if element.kind().can_own(kind) {
            return Some(id);
        }
        current = element.owner();
    }
    None
}

/// `base` followed by the smallest number ≥ 1 not yet used by a sibling under `owner`.
pub fn unique_name(model: &Model, owner: Id, base: &str) -> String {
    let taken = |name: &str| {
        model.children(owner).iter().any(|&c| model.get(c).is_some_and(|e| e.name() == name))
    };
    (1..).map(|n| format!("{base}{n}")).find(|name| !taken(name)).expect("names are unbounded")
}

/// Creates a new element for `tool` and shows it on `diagram` at `origin`.
/// Returns (element, presentation).
pub fn create_node(
    tx: &mut Tx<'_>,
    diagram: Id,
    tool: NodeTool,
    origin: Point,
    base_name: &str,
) -> Result<(Id, Id)> {
    let kind = tool.element_kind();
    let diagram_owner = tx.model().element(diagram)?.owner().ok_or(ModelError::Root)?;
    let owner = owner_for(tx.model(), diagram_owner, &kind)
        .ok_or_else(|| ModelError::Invalid(format!("no owner for a new {}", kind.type_name())))?;
    let name = if tool == NodeTool::Note {
        String::new()
    } else {
        unique_name(tx.model(), owner, base_name)
    };
    let element = tx.create(owner, ElementData::new(name, kind))?;
    let bounds = Rect::from_origin_size(origin, tool.default_size());
    let node = tx.add_node(diagram, Some(element), bounds)?;
    Ok((element, node))
}

/// Connects two presentations of `diagram` with `tool`. Creates the relationship element in the
/// namespace of the source element and returns the new edge presentation.
pub fn connect(tx: &mut Tx<'_>, diagram: Id, tool: EdgeTool, source: Id, target: Id) -> Result<Id> {
    let model = tx.model();
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let element_of = |p: Id| -> Result<Id> {
        view.get(p).ok_or(ModelError::NotFound(p))?.element.ok_or(ModelError::NotFound(p))
    };
    let (s, t) = (element_of(source)?, element_of(target)?);
    let (sk, tk) = (model.element(s)?.kind(), model.element(t)?.kind());
    // Only associations may loop back to the same node; they get a loop of waypoints.
    let self_loop = source == target;
    if (self_loop && tool != EdgeTool::Association) || !tool.accepts(sk, tk, s == t) {
        return Err(ModelError::Invalid(format!(
            "{} cannot connect {} to {}",
            tool.key(),
            sk.type_name(),
            tk.type_name()
        )));
    }

    match tool.element_kind(s, t) {
        Some(kind) => {
            let start = model.element(s)?.owner().unwrap_or(model.root());
            let owner = owner_for(model, start, &kind)
                .ok_or_else(|| ModelError::Invalid(format!("no owner for {}", kind.type_name())))?;
            let waypoints = match view.get(source).and_then(|p| p.bounds()) {
                Some(bounds) if self_loop => crate::scene::self_loop(bounds),
                _ => Vec::new(),
            };
            let element = tx.create(owner, ElementData::new("", kind))?;
            tx.add_edge(diagram, Some(element), source, target, waypoints)
        }
        None => {
            // Note anchor: the comment annotates the other element.
            let (note, other) = if matches!(sk, ElementKind::Comment(_)) { (s, t) } else { (t, s) };
            tx.update(note, |data| {
                if let ElementKind::Comment(p) = &mut data.kind
                    && !p.annotated.contains(&other)
                {
                    p.annotated.push(other);
                }
            })?;
            tx.add_edge(diagram, None, source, target, Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::sample::order_system;

    #[test]
    fn palettes_match_diagram_kinds() {
        let class = palette(DiagramKind::Class);
        assert_eq!(class[0], Tool::Select);
        assert!(class.contains(&Tool::Node(NodeTool::Interface)));
        assert!(!class.contains(&Tool::Node(NodeTool::Actor)));
        let use_case = palette(DiagramKind::UseCase);
        assert!(use_case.contains(&Tool::Edge(EdgeTool::Include)));
        assert!(palette(DiagramKind::Sequence).contains(&Tool::Node(NodeTool::Lifeline)));
    }

    #[test]
    fn connection_rules() {
        let class = ElementKind::class();
        let interface = ElementKind::interface();
        let actor = ElementKind::actor();
        let use_case = ElementKind::use_case();
        let note = ElementKind::Comment(CommentProps::default());
        assert!(EdgeTool::Association.accepts(&class, &class, true));
        assert!(EdgeTool::Association.accepts(&actor, &use_case, false));
        assert!(EdgeTool::Generalization.accepts(&class, &class, false));
        assert!(!EdgeTool::Generalization.accepts(&class, &class, true));
        assert!(!EdgeTool::Generalization.accepts(&class, &interface, false));
        assert!(EdgeTool::Realization.accepts(&class, &interface, false));
        assert!(!EdgeTool::Realization.accepts(&interface, &class, false));
        assert!(EdgeTool::Include.accepts(&use_case, &use_case, false));
        assert!(!EdgeTool::Include.accepts(&actor, &use_case, false));
        assert!(EdgeTool::NoteAnchor.accepts(&note, &class, false));
        assert!(EdgeTool::NoteAnchor.accepts(&class, &note, false));
        assert!(!EdgeTool::NoteAnchor.accepts(&note, &note, false));
    }

    #[test]
    fn create_and_connect_on_the_class_diagram() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let (a, b, edge) = doc
            .edit("create", |tx| {
                let (a, na) =
                    create_node(tx, d, NodeTool::Class, Point::new(600.0, 80.0), "Class")?;
                let (b, nb) =
                    create_node(tx, d, NodeTool::Interface, Point::new(600.0, 300.0), "Interface")?;
                let edge = connect(tx, d, EdgeTool::Realization, na, nb)?;
                Ok((a, b, edge))
            })
            .unwrap();
        let model = doc.model();
        model.check().unwrap();
        // New elements land in the diagram's package with generated names.
        assert_eq!(model.get(a).unwrap().owner(), Some(ids.domain));
        assert_eq!(model.get(a).unwrap().name(), "Class1");
        assert_eq!(model.get(b).unwrap().name(), "Interface1");
        let realization = model.view(d).unwrap().get(edge).unwrap().element.unwrap();
        assert_eq!(model.get(realization).unwrap().owner(), Some(ids.domain));

        // Wrong direction is refused and leaves nothing behind.
        let before = doc.model().clone();
        let (na, nb) = (doc.model().shown_on(a)[0].1, doc.model().shown_on(b)[0].1);
        assert!(doc.edit("bad", |tx| connect(tx, d, EdgeTool::Realization, nb, na)).is_err());
        assert_eq!(*doc.model(), before);
    }

    #[test]
    fn lifelines_go_into_the_interaction_and_notes_annotate() {
        let (mut doc, ids) = order_system();
        let d = ids.sequence_diagram;
        let interaction = doc.model().get(d).unwrap().owner().unwrap();
        let (lifeline, note) = doc
            .edit("create", |tx| {
                let (l, nl) =
                    create_node(tx, d, NodeTool::Lifeline, Point::new(460.0, 40.0), "lifeline")?;
                let (n, nn) = create_node(tx, d, NodeTool::Note, Point::new(460.0, 400.0), "")?;
                connect(tx, d, EdgeTool::NoteAnchor, nn, nl)?;
                Ok((l, n))
            })
            .unwrap();
        let model = doc.model();
        assert_eq!(model.get(lifeline).unwrap().owner(), Some(interaction));
        let ElementKind::Comment(p) = model.get(note).unwrap().kind() else { panic!() };
        assert_eq!(p.annotated, vec![lifeline]);
    }

    #[test]
    fn showing_an_element_brings_its_relationships() {
        let (mut doc, ids) = order_system();
        let other = doc
            .edit("diagram", |tx| {
                tx.create(
                    ids.domain,
                    ElementData::new("Other", ElementKind::diagram(DiagramKind::Class)),
                )
            })
            .unwrap();
        doc.edit("show", |tx| {
            show_element(tx, other, ids.customer, Point::new(0.0, 0.0))?;
            show_element(tx, other, ids.order, Point::new(300.0, 0.0))?;
            show_element(tx, other, ids.premium, Point::new(0.0, 200.0))
        })
        .unwrap();
        let view = doc.model().view(other).unwrap();
        assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 2);
        // Twice is refused, and so is the wrong diagram kind.
        assert!(
            doc.edit("again", |tx| show_element(tx, other, ids.order, Point::default())).is_err()
        );
        assert!(
            doc.edit("wrong", |tx| show_element(
                tx,
                ids.use_case_diagram,
                ids.order,
                Point::default()
            ))
            .is_err()
        );
    }

    #[test]
    fn self_associations_loop() {
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let node = doc.model().shown_on(ids.customer)[0].1;
        let edge =
            doc.edit("loop", |tx| connect(tx, d, EdgeTool::Association, node, node)).unwrap();
        let p = doc.model().view(d).unwrap().get(edge).unwrap().clone();
        let umlcraft_model::Shape::Edge { waypoints, .. } = p.shape else { panic!() };
        assert_eq!(waypoints.len(), 3);
        assert!(
            doc.edit("bad", |tx| connect(tx, d, EdgeTool::Generalization, node, node)).is_err()
        );
    }

    #[test]
    fn unique_names_fill_gaps() {
        let (doc, ids) = order_system();
        assert_eq!(unique_name(doc.model(), ids.domain, "Class"), "Class1");
        assert_eq!(unique_name(doc.model(), ids.domain, "Customer"), "Customer1");
    }
}
