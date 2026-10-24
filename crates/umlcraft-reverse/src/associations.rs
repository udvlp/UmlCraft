//! Attributes typed by classes become associations, as Astah offers after an import.
//!
//! An attribute is a candidate if its type is a type of the model that is no template parameter
//! and not a value type of the language (`java.lang`). A collection counts if its element type
//! counts, a map if its value type does, an array or an optional if its element does. Which
//! library types are containers is up to the language (Java's `Optional` is none for Astah).
//!
//! The association runs from the attribute's class, whose end is not navigable, to the target,
//! whose end is navigable and takes the attribute's name, visibility, read-only, static and
//! default value. Collections and arrays give the end `*` (lists and sorted sets `{ordered}`,
//! sets `{unique}`), optionals `0..1`; a map's key becomes a qualifier at the attribute's
//! class, without a multiplicity. The attribute is deleted.
//!
//! On a new import an association with the same role between the same classes is reused.
//! Navigable ends from a type of the sources whose field is gone are made non-navigable, and an
//! association left without any navigable end is deleted.

use std::collections::{HashMap, HashSet};

use umlcraft_model::{
    AssociationEnd, AssociationProps, ElementData, ElementKind, Id, Model, Multiplicity,
    Navigability, Qualifier, Result, Tx, TypeRef, Visibility,
};

use crate::lang::Container;
use crate::merge::Importer;
use crate::resolve::is_type;

/// What an attribute's type makes of the far end.
struct Target {
    participant: Id,
    multiplicity: Option<Multiplicity>,
    constraints: Vec<String>,
    qualifier: Option<TypeRef>,
}

impl Importer<'_> {
    pub(crate) fn associations(&mut self, tx: &mut Tx<'_>) -> Result<()> {
        let fields = std::mem::take(&mut self.fields);
        // Associations kept or made by this import, by id.
        let mut used: HashSet<Id> = HashSet::new();
        let sources: HashSet<Id> = self.entries.iter().map(|e| e.id).collect();
        // The associations there are, by participant.
        let mut by_participant: HashMap<Id, Vec<Id>> = HashMap::new();
        for e in tx.model().elements() {
            if let ElementKind::Association(p) = e.kind() {
                for end in &p.ends {
                    by_participant.entry(end.participant).or_default().push(e.id());
                }
            }
        }
        let mut converted = Vec::new();
        for (attribute, _, _) in &fields {
            let model = tx.model();
            let Some(element) = model.get(*attribute) else { continue };
            let ElementKind::Attribute(props) = element.kind() else { continue };
            let Some(owner) = element.owner() else { continue };
            let Some(target) = props.type_ref.as_ref().and_then(|t| self.target(model, t)) else {
                continue;
            };
            let data = element.data().clone();
            let props = props.clone();
            let far = AssociationEnd {
                role: data.name.clone(),
                multiplicity: target.multiplicity.or(props.multiplicity),
                navigability: Navigability::Navigable,
                visibility: data.visibility,
                constraints: target.constraints,
                is_read_only: props.is_read_only,
                is_static: props.is_static,
                default_value: props.default_value.clone(),
                ..AssociationEnd::new(target.participant)
            };
            let near = AssociationEnd {
                navigability: Navigability::NonNavigable,
                visibility: Visibility::Private,
                qualifiers: target
                    .qualifier
                    .map(|t| vec![Qualifier { name: "key".into(), type_ref: Some(t) }])
                    .unwrap_or_default(),
                ..AssociationEnd::new(owner)
            };
            let candidates = by_participant.get(&owner).map_or(&[][..], |c| &c[..]);
            let existing = find(model, candidates, owner, &far);
            let association = match existing {
                Some((id, far_index)) => {
                    tx.update(id, |d| {
                        if let ElementKind::Association(p) = &mut d.kind {
                            let near_index = 1 - far_index;
                            let keep_near = &p.ends[near_index];
                            let near = AssociationEnd {
                                aggregation: keep_near.aggregation,
                                constraints: keep_near.constraints.clone(),
                                multiplicity: keep_near.multiplicity,
                                role: keep_near.role.clone(),
                                ..near
                            };
                            let aggregation = p.ends[far_index].aggregation;
                            p.ends[far_index] = AssociationEnd { aggregation, ..far };
                            p.ends[near_index] = near;
                        }
                    })?;
                    id
                }
                None => {
                    let kind = ElementKind::Association(AssociationProps { ends: [near, far] });
                    self.report.associations += 1;
                    tx.create(owner, ElementData::new("", kind))?
                }
            };
            used.insert(association);
            converted.push(*attribute);
        }
        tx.delete_many(&converted)?;
        // Ends of fields that are gone.
        let model = tx.model();
        let stale: Vec<Id> = model
            .elements()
            .filter(|e| !used.contains(&e.id()))
            .filter_map(|e| match e.kind() {
                ElementKind::Association(p) => p
                    .ends
                    .iter()
                    .enumerate()
                    .any(|(i, end)| {
                        end.navigability == Navigability::Navigable
                            && sources.contains(&p.ends[1 - i].participant)
                            && !end.role.is_empty()
                    })
                    .then_some(e.id()),
                _ => None,
            })
            .collect();
        for id in stale {
            let mut gone = false;
            tx.update(id, |d| {
                if let ElementKind::Association(p) = &mut d.kind {
                    let participants = [p.ends[0].participant, p.ends[1].participant];
                    for (i, end) in p.ends.iter_mut().enumerate() {
                        if end.navigability == Navigability::Navigable
                            && sources.contains(&participants[1 - i])
                            && !end.role.is_empty()
                        {
                            end.navigability = Navigability::NonNavigable;
                        }
                    }
                    gone = p.ends.iter().all(|e| e.navigability != Navigability::Navigable);
                }
            })?;
            if gone {
                tx.delete(id)?;
            }
        }
        Ok(())
    }

    /// The far end an attribute of type `t` makes, if any.
    fn target(&self, model: &Model, t: &TypeRef) -> Option<Target> {
        let counts = |id: Id| {
            model.get(id).is_some_and(|e| is_type(e.kind())) && !self.is_value_type(model, id)
        };
        let single = |t: &TypeRef| match t {
            TypeRef::Element(id) if counts(*id) => Some(*id),
            _ => None,
        };
        match t {
            TypeRef::Element(id) => counts(*id).then(|| Target {
                participant: *id,
                multiplicity: None,
                constraints: Vec::new(),
                qualifier: None,
            }),
            TypeRef::Expr(e) if e.prefix.is_empty() => {
                // An array of a class: `Item[]`.
                if e.arguments.is_empty() && e.modifier.starts_with('[') {
                    return single(&e.base).map(|participant| Target {
                        participant,
                        multiplicity: Some(Multiplicity::MANY),
                        constraints: Vec::new(),
                        qualifier: None,
                    });
                }
                if !e.modifier.is_empty() {
                    return None;
                }
                let TypeRef::Element(base) = e.base else { return None };
                let container = self.library_type(model, base)?.container;
                match (container, e.arguments.as_slice()) {
                    (Container::Collection { ordered, unique }, [element]) => {
                        let constraints = [(ordered, "ordered"), (unique, "unique")]
                            .into_iter()
                            .filter(|(on, _)| *on)
                            .map(|(_, c)| c.to_owned())
                            .collect();
                        single(element).map(|participant| Target {
                            participant,
                            multiplicity: Some(Multiplicity::MANY),
                            constraints,
                            qualifier: None,
                        })
                    }
                    // Astah gives the end only the qualifier, no multiplicity.
                    (Container::Map, [key, value]) => single(value).map(|participant| Target {
                        participant,
                        multiplicity: None,
                        constraints: Vec::new(),
                        qualifier: Some(key.clone()),
                    }),
                    (Container::Optional, [element]) => single(element).map(|participant| Target {
                        participant,
                        multiplicity: Some(Multiplicity::OPTIONAL),
                        constraints: Vec::new(),
                        qualifier: None,
                    }),
                    (Container::Owning, [element]) => single(element).map(|participant| Target {
                        participant,
                        multiplicity: Some(Multiplicity::ONE),
                        constraints: Vec::new(),
                        qualifier: None,
                    }),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// Among `candidates`, an association between `owner` and the far end's participant whose end at
/// the participant has the far end's role, with the index of that end.
fn find(model: &Model, candidates: &[Id], owner: Id, far: &AssociationEnd) -> Option<(Id, usize)> {
    candidates.iter().filter_map(|&c| model.get(c)).find_map(|e| match e.kind() {
        ElementKind::Association(p) => (0..2)
            .find(|&i| {
                p.ends[i].participant == far.participant
                    && p.ends[i].role == far.role
                    && p.ends[1 - i].participant == owner
            })
            .map(|i| (e.id(), i)),
        _ => None,
    })
}
