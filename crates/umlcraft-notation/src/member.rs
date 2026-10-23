//! Text form of class members, as shown in compartments and typed when editing:
//! `- name : Type [0..*] = init` for attributes, `+ op(in p : T = 1) : R` for operations.

use std::fmt;

use umlcraft_model::{
    ElementData, ElementKind, Id, Model, ModelError, Multiplicity, ParameterDirection,
    ParameterProps, Result, Tx, TypeRef, Visibility,
};

pub fn type_label(model: &Model, type_ref: &Option<TypeRef>) -> Option<String> {
    match type_ref {
        Some(TypeRef::Element(id)) => model.get(*id).map(|t| t.name().to_owned()),
        Some(TypeRef::Text(text)) => Some(text.clone()),
        None => None,
    }
}

/// The text of a member as shown in a compartment (and offered for editing).
pub fn member_text(model: &Model, id: Id) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    let vis = e.data().visibility.symbol();
    let name = e.name();
    match e.kind() {
        ElementKind::Attribute(p) => {
            let mut s = format!("{vis} {}{name}", if p.is_derived { "/" } else { "" });
            if let Some(t) = type_label(model, &p.type_ref) {
                s.push_str(&format!(" : {t}"));
            }
            if let Some(m) = p.multiplicity {
                s.push_str(&format!(" [{m}]"));
            }
            if !p.default_value.is_empty() {
                s.push_str(&format!(" = {}", p.default_value));
            }
            s
        }
        ElementKind::Operation(p) => {
            let params: Vec<String> = e
                .children()
                .iter()
                .filter_map(|&c| model.get(c))
                .filter_map(|c| match c.kind() {
                    ElementKind::Parameter(pp) if pp.direction != ParameterDirection::Return => {
                        Some(parameter_text(model, c.name(), pp))
                    }
                    _ => None,
                })
                .collect();
            let mut s = format!("{vis} {name}({})", params.join(", "));
            if let Some(t) = type_label(model, &p.return_type) {
                s.push_str(&format!(" : {t}"));
            }
            s
        }
        _ => name.to_owned(),
    }
}

fn parameter_text(model: &Model, name: &str, p: &ParameterProps) -> String {
    let mut s = match p.direction {
        ParameterDirection::Out => format!("out {name}"),
        ParameterDirection::InOut => format!("inout {name}"),
        _ => name.to_owned(),
    };
    if let Some(t) = type_label(model, &p.type_ref) {
        s.push_str(&format!(" : {t}"));
    }
    if !p.default_value.is_empty() {
        s.push_str(&format!(" = {}", p.default_value));
    }
    s
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttributeSpec {
    pub visibility: Option<Visibility>,
    pub derived: bool,
    pub name: String,
    pub type_text: Option<String>,
    pub multiplicity: Option<Multiplicity>,
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterSpec {
    pub direction: ParameterDirection,
    pub name: String,
    pub type_text: Option<String>,
    pub default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationSpec {
    pub visibility: Option<Visibility>,
    pub name: String,
    pub parameters: Vec<ParameterSpec>,
    pub return_type: Option<String>,
}

/// Byte index of the first `sep` outside of `<>`, `()` and `[]`. A `:` that is part of `::`
/// does not count.
fn find_top(s: &str, sep: char) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 => {
                if sep == ':' && (bytes.get(i + 1) == Some(&b':') || i > 0 && bytes[i - 1] == b':')
                {
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
    }
    None
}

/// Byte index of the `)` closing the `(` at byte index `open`.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn split_top(s: &str, sep: char) -> (&str, Option<&str>) {
    match find_top(s, sep) {
        Some(i) => (&s[..i], Some(&s[i + sep.len_utf8()..])),
        None => (s, None),
    }
}

fn split_visibility(s: &str) -> (Option<Visibility>, &str) {
    let s = s.trim();
    match s.chars().next().and_then(Visibility::from_symbol) {
        Some(v) => (Some(v), s[1..].trim_start()),
        None => (None, s),
    }
}

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn check_name(name: &str, text: &str) -> Result<String, ParseError> {
    let name = name.trim();
    if name.is_empty() || name.contains(['(', ')', ':', '=', '[', ']']) {
        return Err(ParseError(format!("no valid name in {text:?}")));
    }
    Ok(name.to_owned())
}

pub fn parse_attribute(text: &str) -> Result<AttributeSpec, ParseError> {
    let (visibility, rest) = split_visibility(text);
    let (derived, rest) = match rest.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (head, default) = split_top(rest, '=');
    let mut head = head.trim();
    let mut multiplicity = None;
    if head.ends_with(']')
        && let Some(open) = head.rfind('[')
    {
        let m = &head[open + 1..head.len() - 1];
        multiplicity =
            Some(m.parse().map_err(|_| ParseError(format!("invalid multiplicity {m:?}")))?);
        head = head[..open].trim_end();
    }
    let (name, type_text) = split_top(head, ':');
    Ok(AttributeSpec {
        visibility,
        derived,
        name: check_name(name, text)?,
        type_text: non_empty(type_text),
        multiplicity,
        default: non_empty(default),
    })
}

fn parse_parameter(text: &str) -> Result<ParameterSpec, ParseError> {
    let text = text.trim();
    let (direction, rest) = [
        ("inout ", ParameterDirection::InOut),
        ("out ", ParameterDirection::Out),
        ("in ", ParameterDirection::In),
    ]
    .into_iter()
    .find_map(|(prefix, d)| text.strip_prefix(prefix).map(|r| (d, r)))
    .unwrap_or((ParameterDirection::In, text));
    let (head, default) = split_top(rest, '=');
    let (name, type_text) = split_top(head, ':');
    Ok(ParameterSpec {
        direction,
        name: check_name(name, text)?,
        type_text: non_empty(type_text),
        default: non_empty(default),
    })
}

pub fn parse_operation(text: &str) -> Result<OperationSpec, ParseError> {
    let (visibility, rest) = split_visibility(text);
    let open = rest.find('(').ok_or_else(|| ParseError(format!("missing \"(\" in {text:?}")))?;
    let name = check_name(&rest[..open], text)?;
    let close = matching_paren(rest, open)
        .ok_or_else(|| ParseError(format!("missing \")\" in {text:?}")))?;
    let params_text = &rest[open + 1..close];
    let tail = rest[close + 1..].trim();
    let return_type = match tail.strip_prefix(':') {
        Some(t) => non_empty(Some(t)),
        None if tail.is_empty() => None,
        None => return Err(ParseError(format!("unexpected {tail:?} after \")\""))),
    };
    let mut parameters = Vec::new();
    let mut remaining = params_text;
    while !remaining.trim().is_empty() {
        let (param, rest) = split_top(remaining, ',');
        parameters.push(parse_parameter(param)?);
        remaining = rest.unwrap_or("");
    }
    Ok(OperationSpec { visibility, name, parameters, return_type })
}

/// A classifier of that name if there is exactly one, else the name as free text.
pub fn resolve_type(model: &Model, text: &str) -> TypeRef {
    let mut matches = model.elements().filter(|e| e.kind().is_classifier() && e.name() == text);
    match (matches.next(), matches.next()) {
        (Some(e), None) => TypeRef::Element(e.id()),
        _ => TypeRef::Text(text.to_owned()),
    }
}

/// Applies edited text to an attribute, operation or other element (which is just renamed).
pub fn apply_member_text(tx: &mut Tx<'_>, id: Id, text: &str) -> Result<()> {
    let invalid = |e: ParseError| ModelError::Invalid(e.to_string());
    let kind = tx.model().element(id)?.kind().clone();
    match kind {
        ElementKind::Attribute(_) => {
            let spec = parse_attribute(text).map_err(invalid)?;
            let type_ref = spec.type_text.as_deref().map(|t| resolve_type(tx.model(), t));
            tx.update(id, |data| {
                if let Some(v) = spec.visibility {
                    data.visibility = v;
                }
                data.name = spec.name;
                if let ElementKind::Attribute(p) = &mut data.kind {
                    p.is_derived = spec.derived;
                    p.type_ref = type_ref;
                    p.multiplicity = spec.multiplicity;
                    p.default_value = spec.default.unwrap_or_default();
                }
            })
        }
        ElementKind::Operation(_) => {
            let spec = parse_operation(text).map_err(invalid)?;
            let return_type = spec.return_type.as_deref().map(|t| resolve_type(tx.model(), t));
            tx.update(id, |data| {
                if let Some(v) = spec.visibility {
                    data.visibility = v;
                }
                data.name = spec.name;
                if let ElementKind::Operation(p) = &mut data.kind {
                    p.return_type = return_type;
                }
            })?;
            let old: Vec<Id> = tx
                .model()
                .children(id)
                .iter()
                .copied()
                .filter(|&c| {
                    tx.model().get(c).is_some_and(|e| matches!(e.kind(), ElementKind::Parameter(_)))
                })
                .collect();
            for p in old {
                tx.delete(p)?;
            }
            for p in spec.parameters {
                let type_ref = p.type_text.as_deref().map(|t| resolve_type(tx.model(), t));
                let props = ParameterProps {
                    type_ref,
                    direction: p.direction,
                    multiplicity: None,
                    default_value: p.default.unwrap_or_default(),
                };
                tx.create(id, ElementData::new(p.name, ElementKind::Parameter(props)))?;
            }
            Ok(())
        }
        _ => {
            let name = text.trim();
            if name.is_empty() && !matches!(kind, ElementKind::Comment(_)) {
                return Err(ModelError::Invalid("the name must not be empty".into()));
            }
            tx.rename(id, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{Document, Model};

    #[test]
    fn parses_attributes() {
        let a = parse_attribute("- name : String").unwrap();
        assert_eq!(a.visibility, Some(Visibility::Private));
        assert_eq!((a.name.as_str(), a.type_text.as_deref()), ("name", Some("String")));
        let a = parse_attribute("+ /items : Map<String, Item> [0..*] = new()").unwrap();
        assert!(a.derived);
        assert_eq!(a.type_text.as_deref(), Some("Map<String, Item>"));
        assert_eq!(a.multiplicity, Some(Multiplicity::MANY));
        assert_eq!(a.default.as_deref(), Some("new()"));
        let a = parse_attribute("count").unwrap();
        assert_eq!((a.visibility, a.type_text), (None, None));
        let a = parse_attribute("id : std::string = \"a:b\"").unwrap();
        assert_eq!(a.type_text.as_deref(), Some("std::string"));
        assert_eq!(a.default.as_deref(), Some("\"a:b\""));
        assert!(parse_attribute("- : int").is_err());
        assert!(parse_attribute("x [3..1]").is_err());
    }

    #[test]
    fn parses_operations() {
        let o =
            parse_operation("+ submit(in order : Order, out ok : bool = true) : Receipt").unwrap();
        assert_eq!(o.name, "submit");
        assert_eq!(o.return_type.as_deref(), Some("Receipt"));
        assert_eq!(o.parameters.len(), 2);
        assert_eq!(o.parameters[1].direction, ParameterDirection::Out);
        assert_eq!(o.parameters[1].default.as_deref(), Some("true"));
        let o = parse_operation("put(m : Map<K, V>)").unwrap();
        assert_eq!(o.parameters.len(), 1);
        assert_eq!(o.parameters[0].type_text.as_deref(), Some("Map<K, V>"));
        assert!(parse_operation("+ run").is_err());
        assert!(parse_operation("run() junk").is_err());
        assert_eq!(parse_operation("- run()").unwrap().parameters, vec![]);
    }

    #[test]
    fn member_text_roundtrips_through_the_model() {
        let (mut doc, ids) = order_system();
        let submit = doc
            .model()
            .children(ids.order)
            .iter()
            .copied()
            .find(|&c| doc.model().get(c).unwrap().name() == "submit")
            .unwrap();
        let text = "# submit(customer : Customer, out note : String = \"\") : Order";
        doc.edit("edit", |tx| apply_member_text(tx, submit, text)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(member_text(model, submit), text);
        // Known classifiers become real type references.
        let params = model.children(submit);
        let ElementKind::Parameter(p) = model.get(params[0]).unwrap().kind() else { panic!() };
        assert_eq!(p.type_ref, Some(TypeRef::Element(ids.customer)));

        // Re-editing replaces the parameters.
        doc.edit("edit", |tx| apply_member_text(tx, submit, "+ submit()")).unwrap();
        assert_eq!(member_text(doc.model(), submit), "+ submit()");
        assert!(doc.model().children(submit).is_empty());
    }

    #[test]
    fn invalid_text_changes_nothing() {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let (_, attr) = doc
            .edit("setup", |tx| {
                let c = tx.create(root, ElementData::new("C", ElementKind::class()))?;
                let a = tx.create(c, ElementData::new("a", ElementKind::attribute(None)))?;
                Ok((c, a))
            })
            .unwrap();
        let before = doc.model().clone();
        assert!(doc.edit("bad", |tx| apply_member_text(tx, attr, "- : int")).is_err());
        assert_eq!(*doc.model(), before);
    }
}
