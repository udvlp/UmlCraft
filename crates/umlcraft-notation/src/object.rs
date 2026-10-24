//! Objects of object diagrams: which slots they show and how a slot's value is set.
//!
//! As in Astah, an object shows a slot for every attribute of its classifier, inherited ones
//! included, with or without a value. Only slots with a value are stored, as [`ElementKind::Slot`]
//! children of the object; the others follow from the classifier, so they stay in step when
//! attributes are added, renamed or removed.

use std::collections::HashSet;

use umlcraft_model::{ElementData, ElementKind, Id, Model, Result, SlotProps, Tx};

/// The attributes an instance of `classifier` has slots for: its own, then those of its
/// generals, nearest first, each once.
pub fn slot_attributes(model: &Model, classifier: Id) -> Vec<Id> {
    let mut attributes = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = vec![classifier];
    while let Some(c) = queue.pop() {
        if !seen.insert(c) {
            continue;
        }
        let Some(e) = model.get(c) else { continue };
        attributes.extend(e.children().iter().copied().filter(|&a| {
            model.get(a).is_some_and(|a| matches!(a.kind(), ElementKind::Attribute(_)))
        }));
        let generals = model.elements().filter_map(|g| match g.kind() {
            ElementKind::Generalization(p) if p.specific == c => Some(p.general),
            _ => None,
        });
        // Depth first, nearest general first.
        let mut generals: Vec<Id> = generals.collect();
        generals.reverse();
        queue.extend(generals);
    }
    attributes
}

/// The slots `instance` shows, as (attribute, value): one per attribute of its classifier, with
/// the stored value or an empty one.
pub fn slots(model: &Model, instance: Id) -> Vec<(Id, String)> {
    let Some(e) = model.get(instance) else { return Vec::new() };
    let ElementKind::Instance(p) = e.kind() else { return Vec::new() };
    let Some(classifier) = p.classifier else { return Vec::new() };
    slot_attributes(model, classifier)
        .into_iter()
        .map(|a| (a, stored_slot(model, instance, a).map(|(_, v)| v).unwrap_or_default()))
        .collect()
}

/// The stored slot of `instance` for `attribute`, with its value.
fn stored_slot(model: &Model, instance: Id, attribute: Id) -> Option<(Id, String)> {
    model.children(instance).iter().find_map(|&c| match model.get(c)?.kind() {
        ElementKind::Slot(s) if s.attribute == Some(attribute) => Some((c, s.value.clone())),
        _ => None,
    })
}

/// A slot as it reads in the object's box: `total = 42`, or just `total` without a value.
pub fn slot_text(model: &Model, attribute: Id, value: &str) -> String {
    let name = model.get(attribute).map(|a| a.name()).unwrap_or_default();
    if value.is_empty() { name.to_owned() } else { format!("{name} = {value}") }
}

/// Sets the value `instance` gives `attribute`. An empty value removes the stored slot. Text
/// typed as `name = value` keeps only the value.
pub fn set_slot(tx: &mut Tx<'_>, instance: Id, attribute: Id, text: &str) -> Result<()> {
    let value = match text.split_once('=') {
        Some((_, value)) => value.trim(),
        None => text.trim(),
    };
    let name = tx.model().element(attribute)?.name().to_owned();
    match stored_slot(tx.model(), instance, attribute) {
        Some((slot, _)) if value.is_empty() => tx.delete(slot),
        Some((_, old)) if old == value => Ok(()),
        Some((slot, _)) => tx.update(slot, |data| {
            data.name = name;
            if let ElementKind::Slot(s) = &mut data.kind {
                s.value = value.to_owned();
            }
        }),
        None if value.is_empty() => Ok(()),
        None => {
            let kind = ElementKind::Slot(SlotProps {
                attribute: Some(attribute),
                value: value.to_owned(),
            });
            tx.create(instance, ElementData::new(name, kind)).map(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{InstanceKind, InstanceProps};

    fn names(model: &Model, ids: &[Id]) -> Vec<String> {
        ids.iter().map(|&a| model.get(a).unwrap().name().to_owned()).collect()
    }

    #[test]
    fn slots_include_inherited_attributes() {
        let (mut doc, ids) = order_system();
        doc.edit("level", |tx| {
            tx.create(ids.premium, ElementData::new("level", ElementKind::attribute(None)))
                .map(|_| ())
        })
        .unwrap();
        let model = doc.model();
        assert_eq!(names(model, &slot_attributes(model, ids.premium)), ["level", "name"]);
        assert_eq!(names(model, &slot_attributes(model, ids.customer)), ["name"]);
    }

    #[test]
    fn set_slot_stores_only_values() {
        let (mut doc, ids) = order_system();
        let name = doc.model().children(ids.customer)[0];
        let kind = ElementKind::Instance(InstanceProps {
            instance: InstanceKind::Object,
            classifier: Some(ids.premium),
        });
        let domain = doc.model().get(ids.customer).unwrap().owner().unwrap();
        let bob = doc.edit("bob", |tx| tx.create(domain, ElementData::new("bob", kind))).unwrap();
        // Shown, but nothing stored yet.
        assert_eq!(slots(doc.model(), bob), vec![(name, String::new())]);
        assert!(doc.model().children(bob).is_empty());

        doc.edit("set", |tx| set_slot(tx, bob, name, "name = \"Bob\"")).unwrap();
        assert_eq!(slots(doc.model(), bob), vec![(name, "\"Bob\"".to_owned())]);
        assert_eq!(slot_text(doc.model(), name, "\"Bob\""), "name = \"Bob\"");
        doc.edit("change", |tx| set_slot(tx, bob, name, "\"Rob\"")).unwrap();
        assert_eq!(doc.model().children(bob).len(), 1);
        assert_eq!(slots(doc.model(), bob)[0].1, "\"Rob\"");

        doc.edit("clear", |tx| set_slot(tx, bob, name, "name = ")).unwrap();
        assert!(doc.model().children(bob).is_empty());
        assert_eq!(slot_text(doc.model(), name, ""), "name");

        // Deleting the attribute takes its slots along.
        doc.edit("set", |tx| set_slot(tx, bob, name, "1")).unwrap();
        doc.edit("drop", |tx| tx.delete(name)).unwrap();
        assert!(doc.model().children(bob).is_empty());
        doc.model().check().unwrap();
    }
}
