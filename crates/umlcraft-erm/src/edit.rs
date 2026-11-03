//! Changing a diagram, for the editor. Every operation keeps the error rules of [`check`]: it
//! works on a copy and only takes it over when no new error appeared, so a refused operation
//! leaves the diagram as it was. Unfinished states (a relationship without participants, an
//! attribute without owner) are only warnings and allowed on the way.
//!
//! Elements are addressed by [`Item`]: a node, a link, a free text, or the label of a link.

use std::collections::HashSet;

use crate::check::{IssueKind, Severity, allowed, check};
use crate::create::SizeSet;
use crate::model::{
    Cardinality, Diagram, Id, Label, LabelPos, LabelText, Line, Link, Max, Min, NodeKind, Point,
    Props, Rect, Style, TextAlign,
};

/// The smallest box a node or text can be given, so that it stays grabbable.
pub const MIN_SIZE: (f64, f64) = (20.0, 10.0);
/// Where the cardinality of a new participation sits: in the middle of the line, 15 px beside
/// it (`docs/plan-erm-phase-4.md` D5).
pub const NEW_LABEL_POS: LabelPos = LabelPos { x: 0.0, y: 15.0, dx: 0.0, dy: 0.0 };
/// The cardinality a new participation gets.
pub const NEW_CARDINALITY: Cardinality = Cardinality::new(0, Max::Many);

/// The cardinality of a new line from `relationship`: `[0,n]`, or `[0,m]` if one of its
/// lines has `n` already and none `m` (`[0,n]` to `[0,m]`).
pub fn new_cardinality(d: &Diagram, relationship: &Id) -> Cardinality {
    let maxes: Vec<Max> = d
        .links
        .iter()
        .filter(|l| l.source == *relationship)
        .filter_map(|l| match &l.label.as_ref()?.text {
            LabelText::Cardinality(c) => Some(c.max),
            LabelText::Text(_) => None,
        })
        .collect();
    if maxes.contains(&Max::Many) && !maxes.contains(&Max::ManyM) {
        Cardinality::new(0, Max::ManyM)
    } else {
        NEW_CARDINALITY
    }
}

/// Something that can be selected and changed.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Item {
    Node(Id),
    Link(Id),
    Text(Id),
    /// The label (cardinality or text) of the link.
    Label(Id),
}

impl Item {
    pub fn id(&self) -> &Id {
        match self {
            Self::Node(id) | Self::Link(id) | Self::Text(id) | Self::Label(id) => id,
        }
    }

    /// Whether the element is in the diagram.
    pub fn exists(&self, d: &Diagram) -> bool {
        match self {
            Self::Node(id) => d.node(id).is_some(),
            Self::Link(id) => d.link(id).is_some(),
            Self::Text(id) => d.texts.iter().any(|t| t.id == *id),
            Self::Label(id) => d.link(id).is_some_and(|l| l.label.is_some()),
        }
    }
}

/// One end of a link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum End {
    Source,
    Target,
}

/// Why an operation was refused.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EditError {
    #[error("{0} does not exist")]
    Missing(Id),
    /// The kinds of the two nodes (`type_name`).
    #[error("{0} and {1} cannot be joined")]
    NotAllowed(&'static str, &'static str),
    /// The change would break an error rule of [`check`].
    #[error("{0}")]
    Rule(IssueKind),
    /// The change does not apply to this kind of element (e.g. a key on an entity).
    #[error("not possible here")]
    NotApplicable,
}

type Result<T> = std::result::Result<T, EditError>;

/// Runs `f` on a copy and takes it over unless it failed or made a new error.
fn guarded<T>(d: &mut Diagram, f: impl FnOnce(&mut Diagram) -> Result<T>) -> Result<T> {
    let errors = |d: &Diagram| -> Vec<IssueKind> {
        check(d).into_iter().filter(|i| i.severity() == Severity::Error).map(|i| i.kind).collect()
    };
    let before = errors(d);
    let mut copy = d.clone();
    let value = f(&mut copy)?;
    if let Some(new) = errors(&copy).into_iter().find(|e| !before.contains(e)) {
        return Err(EditError::Rule(new));
    }
    *d = copy;
    Ok(value)
}

fn node_kind(d: &Diagram, id: &Id) -> Result<NodeKind> {
    d.node(id).map(|n| n.kind).ok_or_else(|| EditError::Missing(id.clone()))
}

fn link_mut<'a>(d: &'a mut Diagram, id: &Id) -> Result<&'a mut Link> {
    d.links.iter_mut().find(|l| l.id == *id).ok_or_else(|| EditError::Missing(id.clone()))
}

// ----- Adding -----------------------------------------------------------------------------------

/// Adds a node of `kind` centred at `center`, with the sizes of the flight example.
pub fn add_node(d: &mut Diagram, kind: NodeKind, name: &str, center: Point) -> Id {
    d.add_node(kind, name, center, SizeSet::Compact)
}

/// Adds an attribute to `owner` (above it, joined to it).
pub fn add_attribute(d: &mut Diagram, owner: &Id, name: &str) -> Result<Id> {
    guarded(d, |d| {
        d.add_attribute(owner, name, SizeSet::Compact).map_err(|e| match e {
            crate::create::ConnectError::Missing(id) => EditError::Missing(id),
            crate::create::ConnectError::NotAllowed(a, b) => EditError::NotAllowed(a, b),
        })
    })
}

/// Adds a free text with its top left corner at `at`.
pub fn add_text(d: &mut Diagram, text: &str, at: Point) -> Id {
    d.add_text(text, at)
}

/// The direction a link between `a` and `b` is stored in: attribute → owner (between two
/// attributes, `a` is the part), relationship → participant, IS-A → its types.
fn direction(a: &NodeKind, b: &NodeKind) -> bool {
    let rank = |k: &NodeKind| match k {
        NodeKind::Attribute { .. } => 0,
        NodeKind::Relationship { .. } | NodeKind::IsA { .. } => 1,
        NodeKind::AssociativeEntity => 2,
        NodeKind::Entity { .. } => 3,
    };
    rank(a) <= rank(b)
}

/// Joins two nodes, in whichever order they were picked; `ports` are the fixed connection
/// points at `a` and `b` (`None`: the end floats). A relationship and an entity get the
/// cardinality `[0,n]`; the first link from an IS-A to an entity above it is the supertype.
pub fn connect(
    d: &mut Diagram,
    a: &Id,
    b: &Id,
    ports: (Option<Point>, Option<Point>),
) -> Result<Id> {
    guarded(d, |d| {
        let (ka, kb) = (node_kind(d, a)?, node_kind(d, b)?);
        if !allowed(&ka, &kb) || a == b && !matches!(ka, NodeKind::Relationship { .. }) {
            return Err(EditError::NotAllowed(ka.type_name(), kb.type_name()));
        }
        let ((source, sp, sk), (target, tp, tk)) = if direction(&ka, &kb) {
            ((a, ports.0, ka), (b, ports.1, kb))
        } else {
            ((b, ports.1, kb), (a, ports.0, ka))
        };
        let mut link = Link::new(source.clone(), target.clone());
        link.source_port = sp;
        link.target_port = tp;
        let relationship_like =
            matches!(sk, NodeKind::Relationship { .. } | NodeKind::AssociativeEntity);
        if relationship_like && tk.is_entity_like() && sk != tk {
            let c = new_cardinality(d, source);
            link.label = Some(Label { text: LabelText::Cardinality(c), pos: NEW_LABEL_POS });
        }
        if matches!(sk, NodeKind::IsA { .. }) {
            let has_super = d.links_of(source).any(|l| l.super_type && l.source == *source);
            let above = |id: &Id| d.node(id).map(|n| n.bounds.center().y);
            link.super_type = !has_super && above(target) < above(source);
        }
        let id = link.id.clone();
        d.links.push(link);
        Ok(id)
    })
}

// ----- Removing ---------------------------------------------------------------------------------

/// Removes the items; a node takes its own attributes (and their parts) and every line at it
/// along, a label item removes only the label. Relationships stay, even without participants.
pub fn delete(d: &mut Diagram, items: &[Item]) -> Result<()> {
    guarded(d, |d| {
        let mut nodes: HashSet<Id> = HashSet::new();
        for item in items {
            match item {
                Item::Node(id) => {
                    nodes.insert(id.clone());
                }
                Item::Link(id) => d.links.retain(|l| l.id != *id),
                Item::Text(id) => d.texts.retain(|t| t.id != *id),
                Item::Label(id) => {
                    if let Ok(link) = link_mut(d, id) {
                        link.label = None;
                    }
                }
            }
        }
        // Attributes whose owner goes, and their parts.
        loop {
            let more: Vec<Id> = d
                .nodes
                .iter()
                .filter(|n| n.kind.is_attribute() && !nodes.contains(&n.id))
                .filter(|n| d.owner_of(&n.id).is_some_and(|o| nodes.contains(&o.id)))
                .map(|n| n.id.clone())
                .collect();
            if more.is_empty() {
                break;
            }
            nodes.extend(more);
        }
        d.nodes.retain(|n| !nodes.contains(&n.id));
        d.links.retain(|l| !nodes.contains(&l.source) && !nodes.contains(&l.target));
        Ok(())
    })
}

// ----- Place and size ---------------------------------------------------------------------------

/// Moves nodes and texts by `(dx, dy)`; the waypoints of links between two moved nodes go
/// along (as in draw.io). Links and labels in `items` are ignored here.
pub fn move_items(d: &mut Diagram, items: &[Item], dx: f64, dy: f64) -> Result<()> {
    guarded(d, |d| {
        let ids: HashSet<&Id> = items
            .iter()
            .filter(|i| matches!(i, Item::Node(_) | Item::Text(_)))
            .map(Item::id)
            .collect();
        for n in d.nodes.iter_mut().filter(|n| ids.contains(&n.id)) {
            n.bounds.x += dx;
            n.bounds.y += dy;
        }
        for t in d.texts.iter_mut().filter(|t| ids.contains(&t.id)) {
            t.bounds.x += dx;
            t.bounds.y += dy;
        }
        for l in d.links.iter_mut().filter(|l| ids.contains(&l.source) && ids.contains(&l.target)) {
            for p in &mut l.waypoints {
                p.x += dx;
                p.y += dy;
            }
        }
        Ok(())
    })
}

/// Gives a node or a text a new box, at least [`MIN_SIZE`].
pub fn set_bounds(d: &mut Diagram, item: &Item, bounds: Rect) -> Result<()> {
    let bounds =
        Rect::new(bounds.x, bounds.y, bounds.width.max(MIN_SIZE.0), bounds.height.max(MIN_SIZE.1));
    guarded(d, |d| {
        match item {
            Item::Node(id) => {
                d.node_mut(id).ok_or_else(|| EditError::Missing(id.clone()))?.bounds = bounds
            }
            Item::Text(id) => {
                d.texts
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .ok_or_else(|| EditError::Missing(id.clone()))?
                    .bounds = bounds
            }
            _ => return Err(EditError::NotApplicable),
        }
        Ok(())
    })
}

/// Makes nodes as large as their names need (`size::fit`), keeping their centres, and free
/// texts as large as their lines (`size::fit_text`).
pub fn fit_to_text(d: &mut Diagram, ids: &[Id]) -> Result<()> {
    guarded(d, |d| {
        for id in ids {
            if let Some(n) = d.node_mut(id) {
                let (w, h) = crate::size::fit(&n.kind, &n.name, (n.bounds.width, n.bounds.height));
                let c = n.bounds.center();
                n.bounds = Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h);
                continue;
            }
            // A free text keeps the side it is aligned to, and its middle height.
            let t = d.texts.iter_mut().find(|t| t.id == *id);
            let t = t.ok_or_else(|| EditError::Missing(id.clone()))?;
            let (w, h) = crate::size::fit_text(t);
            let b = t.bounds;
            let x = match t.style.align.unwrap_or(TextAlign::Center) {
                TextAlign::Left => b.x,
                TextAlign::Center => b.center().x - w / 2.0,
                TextAlign::Right => b.x + b.width - w,
            };
            t.bounds = Rect::new(x, b.center().y - h / 2.0, w, h);
        }
        Ok(())
    })
}

/// Moves a label by `(dx, dy)` from where it is.
pub fn move_label(d: &mut Diagram, link: &Id, dx: f64, dy: f64) -> Result<()> {
    guarded(d, |d| {
        let label =
            link_mut(d, link)?.label.as_mut().ok_or_else(|| EditError::Missing(link.clone()))?;
        label.pos.dx += dx;
        label.pos.dy += dy;
        Ok(())
    })
}

// ----- Texts ------------------------------------------------------------------------------------

/// Gives a node its name, a free text its text, a link (or its label) the label `text` reads
/// as (see [`parse_label`]).
pub fn set_text(d: &mut Diagram, item: &Item, text: &str) -> Result<()> {
    guarded(d, |d| {
        match item {
            Item::Node(id) => {
                d.node_mut(id).ok_or_else(|| EditError::Missing(id.clone()))?.name = text.into()
            }
            Item::Text(id) => {
                d.texts
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .ok_or_else(|| EditError::Missing(id.clone()))?
                    .text = text.into()
            }
            Item::Link(id) | Item::Label(id) => {
                let link = link_mut(d, id)?;
                let pos = link.label.as_ref().map_or(NEW_LABEL_POS, |l| l.pos);
                link.label = parse_label(text).map(|text| Label { text, pos });
            }
        }
        Ok(())
    })
}

/// What a typed label is: nothing (empty), a cardinality (`[0,n]`, `[ 1 , 5 ]`, `[*,m]`,
/// `0..n`, `1..*`) or a free text.
pub fn parse_label(text: &str) -> Option<LabelText> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let cardinality = || {
        let inner = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(t);
        let (min, max) = inner.split_once(',').or_else(|| inner.split_once(".."))?;
        let min = match min.trim() {
            "*" => Min::Any,
            n => Min::Count(n.parse().ok()?),
        };
        let max = match max.trim() {
            "n" | "N" | "*" => Max::Many,
            "m" | "M" => Max::ManyM,
            m => Max::Count(m.parse().ok()?),
        };
        if let (Min::Count(min), Max::Count(max)) = (min, max)
            && max < min
        {
            return None;
        }
        Some(Cardinality { min, max })
    };
    Some(match cardinality() {
        Some(c) => LabelText::Cardinality(c),
        None => LabelText::Text(t.to_owned()),
    })
}

/// Sets the role, the second label of a link (self-references); empty removes it.
pub fn set_role(d: &mut Diagram, link: &Id, role: &str) -> Result<()> {
    guarded(d, |d| {
        let l = link_mut(d, link)?;
        let role = role.trim();
        l.role = (!role.is_empty()).then(|| crate::model::Caption {
            text: role.to_owned(),
            pos: l.role.as_ref().map_or(LabelPos { y: -15.0, ..NEW_LABEL_POS }, |r| r.pos),
        });
        Ok(())
    })
}

/// Names the diagram.
pub fn set_name(d: &mut Diagram, name: &str) {
    d.meta.name = name.to_owned();
}

// ----- Kinds, lines, styles -------------------------------------------------------------------

/// Changes the switches of a node's kind (weak, key, derived, multivalued, identifying, total,
/// disjoint); the kind itself stays.
pub fn set_kind(d: &mut Diagram, node: &Id, kind: NodeKind) -> Result<()> {
    guarded(d, |d| {
        let n = d.node_mut(node).ok_or_else(|| EditError::Missing(node.clone()))?;
        if n.kind.type_name() != kind.type_name() {
            return Err(EditError::NotApplicable);
        }
        n.kind = kind;
        Ok(())
    })
}

pub fn set_line(d: &mut Diagram, link: &Id, line: Line) -> Result<()> {
    guarded(d, |d| {
        link_mut(d, link)?.line = line;
        Ok(())
    })
}

/// Marks the link as the one to the supertype of its IS-A (the IS-A's other links lose the
/// mark), or removes the mark.
pub fn set_super_type(d: &mut Diagram, link: &Id, on: bool) -> Result<()> {
    guarded(d, |d| {
        let source = d.link(link).ok_or_else(|| EditError::Missing(link.clone()))?.source.clone();
        if !matches!(node_kind(d, &source)?, NodeKind::IsA { .. }) {
            return Err(EditError::NotApplicable);
        }
        for l in d.links.iter_mut().filter(|l| l.source == source) {
            l.super_type = on && l.id == *link || !on && l.super_type && l.id != *link;
        }
        Ok(())
    })
}

/// Changes the style of the items (nodes and texts) with `f`.
pub fn set_style(d: &mut Diagram, items: &[Item], f: impl Fn(&mut Style)) -> Result<()> {
    guarded(d, |d| {
        for item in items {
            match item {
                Item::Node(id) => {
                    f(&mut d.node_mut(id).ok_or_else(|| EditError::Missing(id.clone()))?.style)
                }
                Item::Text(id) => f(&mut d
                    .texts
                    .iter_mut()
                    .find(|t| t.id == *id)
                    .ok_or_else(|| EditError::Missing(id.clone()))?
                    .style),
                _ => {}
            }
        }
        Ok(())
    })
}

/// Sets the alignment of free texts.
pub fn set_align(d: &mut Diagram, items: &[Item], align: TextAlign) -> Result<()> {
    set_style(d, items, |s| s.align = (align != TextAlign::Center).then_some(align))
}

/// Sets the database properties of a node.
pub fn set_props(d: &mut Diagram, node: &Id, props: Props) -> Result<()> {
    guarded(d, |d| {
        d.node_mut(node).ok_or_else(|| EditError::Missing(node.clone()))?.props = props;
        Ok(())
    })
}

// ----- Link ends --------------------------------------------------------------------------------

/// Sets or clears (`None`: floating) the fixed connection point of one end.
pub fn set_port(d: &mut Diagram, link: &Id, end: End, port: Option<Point>) -> Result<()> {
    guarded(d, |d| {
        let l = link_mut(d, link)?;
        match end {
            End::Source => l.source_port = port,
            End::Target => l.target_port = port,
        }
        Ok(())
    })
}

/// Lets both ends of the links float.
pub fn release_ends(d: &mut Diagram, links: &[Id]) -> Result<()> {
    guarded(d, |d| {
        for id in links {
            let l = link_mut(d, id)?;
            l.source_port = None;
            l.target_port = None;
        }
        Ok(())
    })
}

/// Hangs one end of a link onto another node (at `port`, or floating). The link keeps its
/// label; the kinds must allow the join.
pub fn reconnect(
    d: &mut Diagram,
    link: &Id,
    end: End,
    node: &Id,
    port: Option<Point>,
) -> Result<()> {
    guarded(d, |d| {
        let l = d.link(link).ok_or_else(|| EditError::Missing(link.clone()))?;
        let other = match end {
            End::Source => l.target.clone(),
            End::Target => l.source.clone(),
        };
        let (kn, ko) = (node_kind(d, node)?, node_kind(d, &other)?);
        if !allowed(&kn, &ko) || *node == other && !matches!(kn, NodeKind::Relationship { .. }) {
            return Err(EditError::NotAllowed(kn.type_name(), ko.type_name()));
        }
        let l = link_mut(d, link)?;
        match end {
            End::Source => (l.source, l.source_port) = (node.clone(), port),
            End::Target => (l.target, l.target_port) = (node.clone(), port),
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Key;

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    fn errors(d: &Diagram) -> Vec<IssueKind> {
        check(d).into_iter().filter(|i| i.severity() == Severity::Error).map(|i| i.kind).collect()
    }

    #[test]
    fn connecting_stores_the_files_direction_and_a_cardinality() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "Kunde", p(0.0, 0.0));
        let r = add_node(&mut d, NodeKind::relationship(), "bucht", p(200.0, 0.0));
        let a = add_node(&mut d, NodeKind::attribute(), "id", p(0.0, -60.0));
        // Picked entity first: still stored relationship → entity.
        let l = connect(&mut d, &e, &r, (Some(p(1.0, 0.5)), None)).unwrap();
        let link = d.link(&l).unwrap();
        assert_eq!((&link.source, &link.target), (&r, &e));
        assert_eq!((link.source_port, link.target_port), (None, Some(p(1.0, 0.5))));
        assert_eq!(link.label.as_ref().unwrap().display(), "[0,n]");
        let l = connect(&mut d, &e, &a, (None, None)).unwrap();
        let link = d.link(&l).unwrap();
        assert_eq!((&link.source, &link.target), (&a, &e));
        assert!(link.label.is_none());
        // Two entities do not join; nothing changes.
        let other = add_node(&mut d, NodeKind::entity(), "B", p(0.0, 200.0));
        let before = d.clone();
        assert!(matches!(
            connect(&mut d, &e, &other, (None, None)),
            Err(EditError::NotAllowed(..))
        ));
        assert_eq!(d, before);
    }

    #[test]
    fn an_attribute_gets_one_owner_only() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "A", p(0.0, 0.0));
        let f = add_node(&mut d, NodeKind::entity(), "B", p(300.0, 0.0));
        let a = add_attribute(&mut d, &e, "x").unwrap();
        assert_eq!(
            connect(&mut d, &a, &f, (None, None)),
            Err(EditError::Rule(IssueKind::SeveralOwners))
        );
    }

    #[test]
    fn a_weak_entity_may_own_a_weak_entity() {
        // order item → note: both weak; until a double line says which one is identified,
        // that is a warning, not a refusal.
        let mut d = Diagram::new("t");
        let r = add_node(&mut d, NodeKind::Relationship { identifying: true }, "r", p(0.0, 0.0));
        let w1 = add_node(&mut d, NodeKind::Entity { weak: true }, "W1", p(-200.0, 0.0));
        let w2 = add_node(&mut d, NodeKind::Entity { weak: true }, "W2", p(200.0, 0.0));
        connect(&mut d, &r, &w1, (None, None)).unwrap();
        connect(&mut d, &r, &w2, (None, None)).unwrap();
        let issues: Vec<_> = crate::check(&d).into_iter().filter(|i| i.element == r).collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, IssueKind::IdentifyingWithSeveralWeak);
        assert_eq!(issues[0].severity(), crate::Severity::Warning);
    }

    #[test]
    fn deleting_an_entity_takes_its_attributes_and_lines() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "A", p(0.0, 0.0));
        let f = add_node(&mut d, NodeKind::entity(), "B", p(300.0, 0.0));
        let r = add_node(&mut d, NodeKind::relationship(), "r", p(150.0, 0.0));
        connect(&mut d, &r, &e, (None, None)).unwrap();
        connect(&mut d, &r, &f, (None, None)).unwrap();
        let a = add_attribute(&mut d, &e, "x").unwrap();
        // A part of the composite attribute x.
        let part = add_node(&mut d, NodeKind::attribute(), "y", p(0.0, -120.0));
        connect(&mut d, &part, &a, (None, None)).unwrap();
        delete(&mut d, &[Item::Node(e)]).unwrap();
        let names: Vec<&str> = d.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["B", "r"]);
        assert_eq!(d.links.len(), 1);
        // The relationship is unfinished now: a warning, no error.
        assert!(errors(&d).is_empty());
        assert!(!check(&d).is_empty());
    }

    #[test]
    fn moving_takes_waypoints_between_moved_nodes_along() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "A", p(0.0, 0.0));
        let a = add_attribute(&mut d, &e, "x").unwrap();
        d.links[0].waypoints.push(p(5.0, -20.0));
        let t = add_text(&mut d, "note", p(100.0, 100.0));
        move_items(&mut d, &[Item::Node(e.clone()), Item::Node(a), Item::Text(t)], 10.0, 20.0)
            .unwrap();
        assert_eq!(d.node(&e).unwrap().bounds, Rect::new(-40.0, 0.0, 100.0, 40.0));
        assert_eq!(d.links[0].waypoints, [p(15.0, 0.0)]);
        assert_eq!(d.texts[0].bounds.x, 110.0);
    }

    #[test]
    fn sizes_keep_a_minimum_and_fit_names() {
        let mut d = Diagram::new("t");
        let a = add_node(&mut d, NodeKind::attribute(), "geschwindigkeit", p(0.0, 0.0));
        set_bounds(&mut d, &Item::Node(a.clone()), Rect::new(0.0, 0.0, 3.0, 2.0)).unwrap();
        assert_eq!(d.node(&a).unwrap().bounds, Rect::new(0.0, 0.0, 20.0, 10.0));
        fit_to_text(&mut d, std::slice::from_ref(&a)).unwrap();
        assert_eq!(d.node(&a).unwrap().bounds.width, 110.0);
        assert!(set_bounds(&mut d, &Item::Link(a), Rect::default()).is_err());
    }

    #[test]
    fn labels_are_read_as_cardinalities_or_texts() {
        let c = |min, max| Some(LabelText::Cardinality(Cardinality::new(min, max)));
        assert_eq!(parse_label("[0,n]"), c(0, Max::Many));
        assert_eq!(parse_label(" [ 1 , 5 ] "), c(1, Max::Count(5)));
        assert_eq!(parse_label("1..*"), c(1, Max::Many));
        assert_eq!(parse_label("0,1"), c(0, Max::Count(1)));
        assert_eq!(parse_label("[0,m]"), c(0, Max::ManyM));
        let any = |max| Some(LabelText::Cardinality(Cardinality { min: Min::Any, max }));
        assert_eq!(parse_label("[*,m]"), any(Max::ManyM));
        assert_eq!(parse_label("*..*"), any(Max::Many));
        assert_eq!(parse_label("[5,1]"), Some(LabelText::Text("[5,1]".into())));
        assert_eq!(parse_label("leitet"), Some(LabelText::Text("leitet".into())));
        assert_eq!(parse_label("  "), None);
    }

    #[test]
    fn the_second_many_end_of_a_relationship_is_m() {
        let mut d = Diagram::new("t");
        let r = add_node(&mut d, NodeKind::relationship(), "nimmt teil", Point::new(0.0, 0.0));
        let mut maxes = Vec::new();
        for name in ["A", "B", "C"] {
            let e = add_node(&mut d, NodeKind::entity(), name, Point::new(100.0, 0.0));
            let l = connect(&mut d, &r, &e, (None, None)).unwrap();
            let label = d.link(&l).unwrap().label.clone().unwrap();
            maxes.push(label.display());
        }
        assert_eq!(maxes, ["[0,n]", "[0,m]", "[0,n]"]);
    }

    #[test]
    fn texts_and_labels_change_with_one_call() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "A", p(0.0, 0.0));
        let f = add_node(&mut d, NodeKind::entity(), "B", p(300.0, 0.0));
        let r = add_node(&mut d, NodeKind::relationship(), "r", p(150.0, 0.0));
        let l = connect(&mut d, &r, &e, (None, None)).unwrap();
        connect(&mut d, &r, &f, (None, None)).unwrap();
        set_text(&mut d, &Item::Node(e.clone()), "Kunde").unwrap();
        set_text(&mut d, &Item::Label(l.clone()), "[1,1]").unwrap();
        assert_eq!(d.node(&e).unwrap().name, "Kunde");
        let label = d.link(&l).unwrap().label.clone().unwrap();
        assert_eq!((label.display(), label.pos), ("[1,1]".to_owned(), NEW_LABEL_POS));
        move_label(&mut d, &l, 3.0, -4.0).unwrap();
        assert_eq!(d.link(&l).unwrap().label.as_ref().unwrap().pos.dx, 3.0);
        set_text(&mut d, &Item::Label(l.clone()), "").unwrap();
        assert!(d.link(&l).unwrap().label.is_none());
        set_role(&mut d, &l, "Chef").unwrap();
        assert_eq!(d.link(&l).unwrap().role.as_ref().unwrap().text, "Chef");
    }

    #[test]
    fn kinds_change_their_switches_only() {
        let mut d = Diagram::new("t");
        let a = add_node(&mut d, NodeKind::attribute(), "id", p(0.0, 0.0));
        let key =
            NodeKind::Attribute { key: Some(Key::Primary), derived: false, multivalued: true };
        set_kind(&mut d, &a, key).unwrap();
        assert_eq!(d.node(&a).unwrap().kind, key);
        assert_eq!(set_kind(&mut d, &a, NodeKind::entity()), Err(EditError::NotApplicable));
    }

    #[test]
    fn an_is_a_has_one_supertype() {
        let mut d = Diagram::new("t");
        let sup = add_node(&mut d, NodeKind::entity(), "Person", p(0.0, 0.0));
        let i = add_node(&mut d, NodeKind::is_a(), "IS-A", p(0.0, 100.0));
        let s1 = add_node(&mut d, NodeKind::entity(), "Kunde", p(-100.0, 200.0));
        let s2 = add_node(&mut d, NodeKind::entity(), "Mitarbeiter", p(100.0, 200.0));
        let up = connect(&mut d, &sup, &i, (None, None)).unwrap();
        let down = connect(&mut d, &i, &s1, (None, None)).unwrap();
        connect(&mut d, &i, &s2, (None, None)).unwrap();
        assert!(d.link(&up).unwrap().super_type);
        assert!(!d.link(&down).unwrap().super_type);
        assert!(check(&d).is_empty(), "{:?}", check(&d));
        set_super_type(&mut d, &down, true).unwrap();
        assert!(!d.link(&up).unwrap().super_type && d.link(&down).unwrap().super_type);
    }

    #[test]
    fn ends_hang_onto_other_nodes_and_float() {
        let mut d = Diagram::new("t");
        let e = add_node(&mut d, NodeKind::entity(), "A", p(0.0, 0.0));
        let f = add_node(&mut d, NodeKind::entity(), "B", p(300.0, 0.0));
        let a = add_attribute(&mut d, &e, "x").unwrap();
        let l = d.links[0].id.clone();
        set_port(&mut d, &l, End::Target, Some(p(0.5, 0.0))).unwrap();
        reconnect(&mut d, &l, End::Target, &f, None).unwrap();
        assert_eq!(d.owner_of(&a).unwrap().id, f);
        assert_eq!(d.links[0].target_port, None);
        set_port(&mut d, &l, End::Source, Some(p(0.5, 1.0))).unwrap();
        release_ends(&mut d, std::slice::from_ref(&l)).unwrap();
        assert_eq!(d.links[0].source_port, None);
        // An attribute cannot hang on an IS-A.
        let i = add_node(&mut d, NodeKind::is_a(), "IS-A", p(0.0, 300.0));
        assert!(reconnect(&mut d, &l, End::Target, &i, None).is_err());
    }
}
