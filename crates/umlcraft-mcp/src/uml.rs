//! Settings of class and object diagrams the tools take beyond a name: the ends of
//! associations and links, abstract and static elements, and an object's classifier and slots.

use serde_json::Value;
use umlcraft_model::{Aggregation, ElementKind, Id, Model, Multiplicity, Navigability, Tx};
use umlcraft_notation::{member, object};

/// The keys of [`set_ends`], by the index of the end.
pub const END_KEYS: [&str; 2] = ["source_end", "target_end"];

/// `text` without `{static}` and `{abstract}`, and which of them it had: a member written as
/// `- count : int {static}` is a static attribute, not one typed `int {static}`.
pub fn member_flags(text: &str) -> (String, bool, bool) {
    let mut rest = String::new();
    let (mut is_static, mut is_abstract) = (false, false);
    let mut tail = text;
    while let Some(open) = tail.find('{') {
        let Some(close) = tail[open..].find('}').map(|c| open + c) else { break };
        let word = tail[open + 1..close].trim();
        rest.push_str(&tail[..open]);
        if word.eq_ignore_ascii_case("static") {
            is_static = true;
        } else if word.eq_ignore_ascii_case("abstract") {
            is_abstract = true;
        } else {
            rest.push_str(&tail[open..=close]);
        }
        tail = &tail[close + 1..];
    }
    rest.push_str(tail);
    (rest.trim().to_owned(), is_static, is_abstract)
}

/// Sets `static` and `abstract` from `args` (or the flags written in a member's text) on
/// `kind`: static attributes and operations, abstract classifiers and operations.
pub fn set_flags(
    kind: &mut ElementKind,
    is_static: Option<bool>,
    is_abstract: Option<bool>,
) -> Result<(), String> {
    let type_name = kind.type_name();
    if let Some(on) = is_static {
        match kind {
            ElementKind::Attribute(p) => p.is_static = on,
            ElementKind::Operation(p) => p.is_static = on,
            _ => return Err(format!("a {type_name} has no `static`")),
        }
    }
    if let Some(on) = is_abstract {
        match kind {
            ElementKind::Class(p)
            | ElementKind::Interface(p)
            | ElementKind::Enumeration(p)
            | ElementKind::DataType(p) => p.is_abstract = on,
            ElementKind::Operation(p) => p.is_abstract = on,
            _ => return Err(format!("a {type_name} has no `abstract`")),
        }
    }
    Ok(())
}

/// The flag `key` of `args`, if given.
pub fn flag(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("`{key}` is true or false")),
    }
}

/// Sets the ends of an association or link from `source_end` and `target_end` in `args`:
/// `role` (as the line shows it, `- items {ordered}`), `multiplicity`, `navigability` and
/// `aggregation` (a diamond at that end, the whole).
pub fn set_ends(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    let type_name = kind.type_name();
    for (i, key) in END_KEYS.into_iter().enumerate() {
        let Some(end) = args.get(key) else { continue };
        if !end.is_object() {
            return Err(format!(
                "`{key}` is an object: role, multiplicity, navigability, \
                                aggregation"
            ));
        }
        let text = |field: &str| -> Result<Option<&str>, String> {
            match end.get(field) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(s)) => Ok(Some(s.as_str())),
                Some(_) => Err(format!("`{key}.{field}` is text")),
            }
        };
        let navigability = match text("navigability")? {
            None => None,
            Some("") | Some("unspecified") => Some(Navigability::Unspecified),
            Some("navigable") => Some(Navigability::Navigable),
            Some("non_navigable") => Some(Navigability::NonNavigable),
            Some(other) => {
                return Err(format!(
                    "`{key}.navigability` is one of navigable, non_navigable, \
                                    \"\", not `{other}`"
                ));
            }
        };
        let aggregation = match text("aggregation")? {
            None => None,
            Some("") | Some("none") => Some(Aggregation::None),
            Some("shared") => Some(Aggregation::Shared),
            Some("composite") => Some(Aggregation::Composite),
            Some(other) => {
                return Err(format!(
                    "`{key}.aggregation` is one of none, shared, composite, \
                                    not `{other}`"
                ));
            }
        };
        let multiplicity = match text("multiplicity")? {
            None => None,
            Some(m) if m.trim().is_empty() => Some(None),
            Some(m) => Some(Some(m.trim().parse::<Multiplicity>().map_err(|_| {
                format!("`{key}.multiplicity` `{m}` is no multiplicity, e.g. 1, 0..1, *, 1..*")
            })?)),
        };
        let role = text("role")?;
        match kind {
            ElementKind::Association(p) => {
                let e = &mut p.ends[i];
                if let Some(role) = role {
                    let (visibility, name, constraints) = member::parse_role(role);
                    if let Some(v) = visibility {
                        e.visibility = v;
                    }
                    e.role = name;
                    e.constraints = constraints;
                }
                if let Some(m) = multiplicity {
                    e.multiplicity = m;
                }
                if let Some(n) = navigability {
                    e.navigability = n;
                }
                if let Some(a) = aggregation {
                    e.aggregation = a;
                }
            }
            ElementKind::Link(p) => {
                if multiplicity.is_some() {
                    return Err(format!("the ends of a link have no multiplicity (`{key}`)"));
                }
                let e = &mut p.ends[i];
                if let Some(role) = role {
                    e.role = role.trim().to_owned();
                }
                if let Some(n) = navigability {
                    e.navigability = n;
                }
                if let Some(a) = aggregation {
                    e.aggregation = a;
                }
            }
            _ => return Err(format!("a {type_name} has no `{key}`")),
        }
    }
    Ok(())
}

/// The classifier `args` names for an object, by id or name: `Some(None)` for "" (none),
/// `None` if not given.
pub fn classifier(model: &Model, args: &Value) -> Result<Option<Option<Id>>, String> {
    let Some(value) = args.get("classifier") else { return Ok(None) };
    let text = value.as_str().ok_or("`classifier` is an id or a name")?.trim();
    if text.is_empty() {
        return Ok(Some(None));
    }
    let is_classifier = |id: Id| model.get(id).is_some_and(|e| e.kind().is_classifier());
    if let Ok(id) = text.parse::<Id>() {
        return if is_classifier(id) {
            Ok(Some(Some(id)))
        } else {
            Err(format!("{id} is no class"))
        };
    }
    let named: Vec<Id> = model
        .elements()
        .filter(|e| e.name() == text && e.kind().is_classifier())
        .map(|e| e.id())
        .collect();
    match named[..] {
        [id] => Ok(Some(Some(id))),
        [] => Err(format!("no class `{text}`; create it first, or give its id")),
        _ => Err(format!("more than one class is named `{text}`; give its id")),
    }
}

/// Sets the classifier of the object `kind`.
pub fn set_classifier(kind: &mut ElementKind, classifier: Option<Id>) -> Result<(), String> {
    match kind {
        ElementKind::Instance(p) => {
            p.classifier = classifier;
            Ok(())
        }
        _ => Err(format!("a {} has no `classifier`", kind.type_name())),
    }
}

/// The values `args` gives the slots of an object, by attribute name, in order.
pub fn slots(args: &Value) -> Result<Option<Vec<(String, String)>>, String> {
    let Some(value) = args.get("slots") else { return Ok(None) };
    let map = value.as_object().ok_or("`slots` maps attribute names to values")?;
    map.iter()
        .map(|(name, v)| {
            let value = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            Ok((name.clone(), value))
        })
        .collect::<Result<Vec<_>, String>>()
        .map(Some)
}

/// Sets the slots of `instance` by attribute name: an attribute of its classifier, inherited
/// ones included. An empty value clears a slot.
pub fn set_slots(
    tx: &mut Tx<'_>,
    instance: Id,
    values: &[(String, String)],
) -> umlcraft_model::Result<()> {
    let invalid = umlcraft_model::ModelError::Invalid;
    let model = tx.model();
    let classifier = match model.element(instance)?.kind() {
        ElementKind::Instance(p) => p.classifier,
        other => return Err(invalid(format!("a {} has no slots", other.type_name()))),
    };
    let Some(classifier) = classifier else {
        return Err(invalid(
            "the object has no classifier whose attributes it has slots \
                            for; give its `classifier`"
                .into(),
        ));
    };
    let attributes = object::slot_attributes(model, classifier);
    let mut found = Vec::new();
    for (name, value) in values {
        let attribute = attributes
            .iter()
            .copied()
            .find(|&a| model.get(a).is_some_and(|a| a.name() == name))
            .ok_or_else(|| {
                let class = model.get(classifier).map_or("", |c| c.name());
                invalid(format!("{class} has no attribute `{name}`"))
            })?;
        found.push((attribute, value));
    }
    for (attribute, value) in found {
        object::set_slot(tx, instance, attribute, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_come_off_the_member_text() {
        assert_eq!(member_flags("- count : int {static}"), ("- count : int".into(), true, false));
        assert_eq!(
            member_flags("+ area() : double {abstract} {query}"),
            ("+ area() : double  {query}".into(), false, true)
        );
        assert_eq!(
            member_flags("- age : int {age > 0}"),
            ("- age : int {age > 0}".into(), false, false)
        );
    }
}
