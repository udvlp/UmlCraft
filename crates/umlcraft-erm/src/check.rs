//! The rules a valid diagram keeps (`docs/erm-spec.md` § 5, P1–P12).
//!
//! Errors make a file unreadable, warnings are reported and the file still opens. The editor
//! keeps every error rule with each operation.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::model::{Diagram, Id, LabelPos, LabelText, Max, NodeKind, Point, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    Error,
    Warning,
}

/// A broken rule and the element it was found at.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Issue {
    pub element: Id,
    pub kind: IssueKind,
}

impl Issue {
    pub fn severity(&self) -> Severity {
        self.kind.severity()
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.kind.code(), self.element, self.kind)
    }
}

/// The rules. Texts for users come from the app's locales; `Display` is the English
/// developer text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum IssueKind {
    /// P1
    DuplicateId,
    /// P2: `end` is the missing node's id.
    MissingEnd { end: Id },
    /// P3
    NotAllowed { source: &'static str, target: &'static str },
    /// P4
    NoOwner,
    /// P4
    SeveralOwners,
    /// P5
    OwnPart,
    /// P6
    TooFewParticipants,
    /// P7
    IdentifyingWithoutWeak,
    /// P7
    IdentifyingWithSeveralWeak,
    /// P8 (warning)
    WeakWithoutIdentifying,
    /// P9: an IS-A without exactly one supertype link and at least one subtype link.
    IsAEnds,
    /// P9: `superType` on a link that does not start at an IS-A.
    SuperTypeOutsideIsA,
    /// P10 (warning)
    CardinalityOutsideParticipation,
    /// P11
    BadGeometry,
    /// P12
    BadCardinality,
}

impl IssueKind {
    pub fn severity(&self) -> Severity {
        match self {
            Self::WeakWithoutIdentifying | Self::CardinalityOutsideParticipation => {
                Severity::Warning
            }
            _ => Severity::Error,
        }
    }

    /// The rule's number in the specification.
    pub fn code(&self) -> &'static str {
        match self {
            Self::DuplicateId => "P1",
            Self::MissingEnd { .. } => "P2",
            Self::NotAllowed { .. } => "P3",
            Self::NoOwner | Self::SeveralOwners => "P4",
            Self::OwnPart => "P5",
            Self::TooFewParticipants => "P6",
            Self::IdentifyingWithoutWeak | Self::IdentifyingWithSeveralWeak => "P7",
            Self::WeakWithoutIdentifying => "P8",
            Self::IsAEnds | Self::SuperTypeOutsideIsA => "P9",
            Self::CardinalityOutsideParticipation => "P10",
            Self::BadGeometry => "P11",
            Self::BadCardinality => "P12",
        }
    }
}

impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId => f.write_str("the id is used more than once"),
            Self::MissingEnd { end } => write!(f, "the link's end {end} does not exist"),
            Self::NotAllowed { source, target } => {
                write!(f, "{source} and {target} cannot be joined")
            }
            Self::NoOwner => f.write_str("the attribute belongs to nothing"),
            Self::SeveralOwners => f.write_str("the attribute belongs to more than one node"),
            Self::OwnPart => f.write_str("the attribute is a part of itself"),
            Self::TooFewParticipants => {
                f.write_str("a relationship needs at least two participants")
            }
            Self::IdentifyingWithoutWeak => {
                f.write_str("an identifying relationship needs a weak entity")
            }
            Self::IdentifyingWithSeveralWeak => {
                f.write_str("an identifying relationship identifies one weak entity")
            }
            Self::WeakWithoutIdentifying => {
                f.write_str("the weak entity has no identifying relationship")
            }
            Self::IsAEnds => f.write_str("an IS-A needs one supertype and at least one subtype"),
            Self::SuperTypeOutsideIsA => f.write_str("only links of an IS-A lead to a supertype"),
            Self::CardinalityOutsideParticipation => {
                f.write_str("cardinalities belong between a relationship and an entity")
            }
            Self::BadGeometry => f.write_str("invalid position or size"),
            Self::BadCardinality => f.write_str("invalid cardinality"),
        }
    }
}

/// Whether a link from a node of kind `source` to one of kind `target` is allowed. The
/// direction does not matter, except that between two attributes the source is the part.
pub fn allowed(source: &NodeKind, target: &NodeKind) -> bool {
    fn one_way(a: &NodeKind, b: &NodeKind) -> bool {
        match a {
            NodeKind::Attribute { .. } => !matches!(b, NodeKind::IsA { .. }),
            NodeKind::Relationship { .. } => b.is_entity_like(),
            // An associative entity is also a relationship: entities take part in it.
            NodeKind::AssociativeEntity => matches!(b, NodeKind::Entity { .. }),
            NodeKind::IsA { .. } => matches!(b, NodeKind::Entity { .. }),
            _ => false,
        }
    }
    one_way(source, target) || one_way(target, source)
}

/// All broken rules, errors and warnings, in the order of the diagram.
pub fn check(d: &Diagram) -> Vec<Issue> {
    let mut out = Vec::new();
    let mut push = |element: &Id, kind| out.push(Issue { element: element.clone(), kind });

    // P1
    let mut seen = HashSet::new();
    let ids = d.nodes.iter().map(|n| &n.id);
    for id in ids.chain(d.links.iter().map(|l| &l.id)).chain(d.texts.iter().map(|t| &t.id)) {
        if !seen.insert(id) {
            push(id, IssueKind::DuplicateId);
        }
    }

    let kinds: HashMap<&Id, &NodeKind> = d.nodes.iter().map(|n| (&n.id, &n.kind)).collect();
    // P2, P3, P9 (superType), P10, P11 (links), P12
    for l in &d.links {
        let ends = [&l.source, &l.target];
        let missing: Vec<&Id> = ends.into_iter().filter(|e| !kinds.contains_key(e)).collect();
        for end in &missing {
            push(&l.id, IssueKind::MissingEnd { end: (*end).clone() });
        }
        if !missing.is_empty() {
            continue;
        }
        let (a, b) = (kinds[&l.source], kinds[&l.target]);
        if !allowed(a, b) {
            push(&l.id, IssueKind::NotAllowed { source: a.type_name(), target: b.type_name() });
        }
        if l.super_type && !matches!(a, NodeKind::IsA { .. }) {
            push(&l.id, IssueKind::SuperTypeOutsideIsA);
        }
        let relationship_like =
            |k: &NodeKind| matches!(k, NodeKind::Relationship { .. } | NodeKind::AssociativeEntity);
        let participation = relationship_like(a) && b.is_entity_like() && a != b
            || relationship_like(b) && a.is_entity_like() && a != b;
        if let Some(label) = &l.label
            && let LabelText::Cardinality(c) = &label.text
        {
            if !participation {
                push(&l.id, IssueKind::CardinalityOutsideParticipation);
            }
            if matches!(c.max, Max::Count(max) if max < c.min) {
                push(&l.id, IssueKind::BadCardinality);
            }
        }
        let ports = [l.source_port, l.target_port].into_iter().flatten();
        let ports_ok =
            ports.into_iter().all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y));
        let positions = l.label.iter().map(|x| &x.pos).chain(l.role.iter().map(|r| &r.pos));
        if !ports_ok || !positions.into_iter().all(pos_ok) || !l.waypoints.iter().all(point_ok) {
            push(&l.id, IssueKind::BadGeometry);
        }
    }

    for n in &d.nodes {
        // P11
        if !rect_ok(&n.bounds) {
            push(&n.id, IssueKind::BadGeometry);
        }
        match n.kind {
            NodeKind::Attribute { .. } => {
                // P4, P5
                match d.owners_of(&n.id).count() {
                    0 => push(&n.id, IssueKind::NoOwner),
                    1 => {
                        if is_own_part(d, &n.id) {
                            push(&n.id, IssueKind::OwnPart);
                        }
                    }
                    _ => push(&n.id, IssueKind::SeveralOwners),
                }
            }
            NodeKind::Relationship { identifying } => {
                // P6, P7
                let participants: Vec<&Id> = d
                    .links_of(&n.id)
                    .filter_map(|l| l.other(&n.id))
                    .filter(|o| kinds.get(o).is_some_and(|k| k.is_entity_like()))
                    .collect();
                if participants.len() < 2 {
                    push(&n.id, IssueKind::TooFewParticipants);
                }
                if identifying {
                    let weak: HashSet<&Id> = participants
                        .iter()
                        .copied()
                        .filter(|o| matches!(kinds[o], NodeKind::Entity { weak: true }))
                        .collect();
                    match weak.len() {
                        0 => push(&n.id, IssueKind::IdentifyingWithoutWeak),
                        1 => {}
                        _ => push(&n.id, IssueKind::IdentifyingWithSeveralWeak),
                    }
                }
            }
            NodeKind::Entity { weak: true } => {
                // P8
                let identified = d.links_of(&n.id).filter_map(|l| l.other(&n.id)).any(|o| {
                    matches!(kinds.get(o), Some(NodeKind::Relationship { identifying: true }))
                });
                if !identified {
                    push(&n.id, IssueKind::WeakWithoutIdentifying);
                }
            }
            NodeKind::IsA { .. } => {
                // P9
                let links: Vec<_> = d.links_of(&n.id).collect();
                let supers = links.iter().filter(|l| l.super_type && l.source == n.id).count();
                if supers != 1 || links.len() < 2 {
                    push(&n.id, IssueKind::IsAEnds);
                }
            }
            NodeKind::Entity { weak: false } | NodeKind::AssociativeEntity => {}
        }
    }

    for t in &d.texts {
        if !rect_ok(&t.bounds) {
            push(&t.id, IssueKind::BadGeometry);
        }
    }
    out
}

/// Whether the attribute is reached again by following owners from it.
fn is_own_part(d: &Diagram, start: &Id) -> bool {
    let mut seen = HashSet::new();
    let mut current = start.clone();
    while let Some(owner) = d.owner_of(&current) {
        if !owner.kind.is_attribute() {
            return false;
        }
        if owner.id == *start || !seen.insert(owner.id.clone()) {
            return true;
        }
        current = owner.id.clone();
    }
    false
}

fn rect_ok(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite()) && r.width > 0.0 && r.height > 0.0
}

fn point_ok(p: &Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

fn pos_ok(p: &LabelPos) -> bool {
    [p.x, p.y, p.dx, p.dy].iter().all(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::create::SizeSet;
    use crate::model::{Cardinality, Label, LabelText, Link};

    fn kinds() -> [NodeKind; 5] {
        [
            NodeKind::entity(),
            NodeKind::AssociativeEntity,
            NodeKind::attribute(),
            NodeKind::relationship(),
            NodeKind::is_a(),
        ]
    }

    #[test]
    fn allowed_connections_follow_the_table() {
        // Rows: source; columns: target, in the order of kinds() (spec § 3.2).
        let table = [
            [false, true, true, true, true],
            [true, false, true, true, false],
            [true, true, true, true, false],
            [true, true, true, false, false],
            [true, false, false, false, false],
        ];
        for (i, a) in kinds().iter().enumerate() {
            for (j, b) in kinds().iter().enumerate() {
                assert_eq!(allowed(a, b), table[i][j], "{} → {}", a.type_name(), b.type_name());
            }
        }
    }

    fn binary() -> (Diagram, Id, Id, Id) {
        let mut d = Diagram::new("t");
        let a = d.add_entity("A", Point::new(0.0, 0.0), SizeSet::Compact);
        let b = d.add_entity("B", Point::new(300.0, 0.0), SizeSet::Compact);
        let r = d.add_relationship("r", Point::new(150.0, 0.0), SizeSet::Compact);
        d.connect(&r, &a).unwrap();
        d.connect(&r, &b).unwrap();
        (d, a, b, r)
    }

    #[test]
    fn a_binary_relationship_is_valid() {
        assert_eq!(check(&binary().0), vec![]);
    }

    #[test]
    fn a_weak_entity_without_identifying_relationship_is_a_warning() {
        let (mut d, a, ..) = binary();
        d.node_mut(&a).unwrap().kind = NodeKind::Entity { weak: true };
        let issues = check(&d);
        assert_eq!(issues, vec![Issue { element: a, kind: IssueKind::WeakWithoutIdentifying }]);
        assert_eq!(issues[0].severity(), Severity::Warning);
    }

    #[test]
    fn identifying_relationship_with_its_weak_entity_is_valid() {
        let (mut d, a, _, r) = binary();
        d.node_mut(&a).unwrap().kind = NodeKind::Entity { weak: true };
        d.node_mut(&r).unwrap().kind = NodeKind::Relationship { identifying: true };
        assert_eq!(check(&d), vec![]);
    }

    #[test]
    fn a_cardinality_on_an_attribute_line_is_a_warning() {
        let (mut d, a, ..) = binary();
        let x = d.add_attribute(&a, "x", SizeSet::Compact).unwrap();
        let link = d.links.iter_mut().find(|l| l.source == x).unwrap();
        link.label = Some(Label {
            text: LabelText::Cardinality(Cardinality::new(0, crate::Max::Many)),
            pos: LabelPos::default(),
        });
        let id = link.id.clone();
        assert_eq!(
            check(&d),
            vec![Issue { element: id, kind: IssueKind::CardinalityOutsideParticipation }]
        );
    }

    #[test]
    fn a_self_reference_counts_as_two_participations() {
        let mut d = Diagram::new("t");
        let a = d.add_entity("Mitarbeiter", Point::new(0.0, 0.0), SizeSet::Compact);
        let r = d.add_relationship("leitet", Point::new(150.0, 0.0), SizeSet::Compact);
        d.connect(&r, &a).unwrap();
        d.connect(&r, &a).unwrap();
        assert_eq!(check(&d), vec![]);
    }

    #[test]
    fn an_is_a_needs_its_supertype_link() {
        let mut d = Diagram::new("t");
        let sup = d.add_entity("Person", Point::new(0.0, 0.0), SizeSet::Compact);
        let sub = d.add_entity("Kunde", Point::new(0.0, 200.0), SizeSet::Compact);
        let i = d.add_is_a(Point::new(0.0, 100.0), SizeSet::Compact);
        let up = d.connect(&i, &sup).unwrap();
        d.links.push(Link::new(i.clone(), sub));
        assert_eq!(check(&d), vec![Issue { element: i, kind: IssueKind::IsAEnds }]);
        d.links.iter_mut().find(|l| l.id == up).unwrap().super_type = true;
        assert_eq!(check(&d), vec![]);
    }
}
