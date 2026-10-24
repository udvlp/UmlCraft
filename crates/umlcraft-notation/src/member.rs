//! Text form of class members, as shown in compartments and typed when editing:
//! `- name : Type [0..*] = init` for attributes, `+ op(in p : T = 1) : R` for operations.

use std::fmt;

use umlcraft_model::{
    ElementData, ElementKind, Id, Model, ModelError, Multiplicity, ParameterDirection,
    ParameterProps, Result, TemplateParameterKind, TemplateParameterProps, Tx, TypeExpr, TypeRef,
    Visibility,
};

pub fn type_label(model: &Model, type_ref: &Option<TypeRef>) -> Option<String> {
    match type_ref {
        Some(TypeRef::Element(id)) => model.get(*id).map(|t| t.name().to_owned()),
        Some(t) => Some(type_text(model, t)),
        None => None,
    }
}

/// A type as written: `Foo`, `List<Foo>`, `? extends Number`, `Foo[]`. Arguments are separated
/// by a bare comma (`Map<String,Foo>`), as Astah writes them.
pub fn type_text(model: &Model, type_ref: &TypeRef) -> String {
    match type_ref {
        TypeRef::Element(id) => model.get(*id).map(|t| t.name().to_owned()).unwrap_or_default(),
        TypeRef::Text(text) => text.clone(),
        TypeRef::Expr(e) => {
            let mut s = format!("{}{}", e.prefix, type_text(model, &e.base));
            if !e.arguments.is_empty() {
                let args: Vec<String> = e.arguments.iter().map(|a| type_text(model, a)).collect();
                s.push_str(&format!("<{}>", args.join(",")));
            }
            s.push_str(&e.modifier);
            s
        }
    }
}

/// Template parameters owned by `owner`, in order.
pub fn template_parameters(model: &Model, owner: Id) -> Vec<Id> {
    model
        .children(owner)
        .iter()
        .copied()
        .filter(|&c| {
            model.get(c).is_some_and(|e| matches!(e.kind(), ElementKind::TemplateParameter(_)))
        })
        .collect()
}

/// A template parameter as written in an operation's signature: `T`, `Ts...`.
fn template_parameter_name(model: &Model, id: Id) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    match e.kind() {
        ElementKind::TemplateParameter(p) if p.is_variadic => format!("{}...", e.name()),
        _ => e.name().to_owned(),
    }
}

/// A template parameter as Astah writes it in a classifier's template box: `T>Number` for a
/// bound, `N:int` for a value parameter, `=default` after either.
pub fn template_parameter_text(model: &Model, id: Id) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    let ElementKind::TemplateParameter(p) = e.kind() else { return e.name().to_owned() };
    let mut s = template_parameter_name(model, id);
    if !p.bounds.is_empty() {
        let bounds: Vec<String> = p.bounds.iter().map(|b| type_text(model, b)).collect();
        s.push_str(&format!(">{}", bounds.join(",")));
    }
    if p.parameter == TemplateParameterKind::Value
        && let Some(t) = type_label(model, &p.type_ref)
    {
        s.push_str(&format!(":{t}"));
    }
    if let Some(d) = type_label(model, &p.default) {
        s.push_str(&format!("={d}"));
    }
    s
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
            let templates: Vec<String> = template_parameters(model, id)
                .into_iter()
                .map(|t| template_parameter_name(model, t))
                .collect();
            let templates = if templates.is_empty() {
                String::new()
            } else {
                format!("<{}>", templates.join(", "))
            };
            let mut s = format!("{vis} {name}{templates}({})", params.join(", "));
            if let Some(t) = type_label(model, &p.return_type) {
                s.push_str(&format!(" : {t}"));
            }
            s
        }
        ElementKind::TemplateParameter(_) => template_parameter_text(model, id),
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
    /// Names of the operation's own template parameters, as in `max<T>(a : T) : T`.
    pub template_parameters: Vec<String>,
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
    // `name : Foo [0..*]` has a multiplicity; in `name : Foo[]` the brackets are part of the
    // type (an array).
    if head.ends_with(']')
        && let Some(open) = head.rfind('[')
        && (find_top(head, ':').is_none() || head[..open].ends_with(char::is_whitespace))
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateParameterSpec {
    pub name: String,
    pub variadic: bool,
    pub bounds: Vec<String>,
    /// Set for a value parameter: the type of the value.
    pub value_type: Option<String>,
    pub default: Option<String>,
}

/// A template parameter as typed: `T`, `T>Number`, `T>A,B`, `N:int=10`, `D=Number`, `Ts...`.
pub fn parse_template_parameter(text: &str) -> Result<TemplateParameterSpec, ParseError> {
    // The bound starts at the first `>` that closes no `<`.
    let mut depth = 0;
    let bound_at = text.char_indices().find_map(|(i, c)| {
        match c {
            '<' => depth += 1,
            '>' if depth == 0 => return Some(i),
            '>' => depth -= 1,
            _ => {}
        }
        None
    });
    let (name, bounds, value_type, default) = match bound_at {
        Some(i) => {
            let (rest, default) = split_top(&text[i + 1..], '=');
            let (bounds, value_type) = split_top(rest, ':');
            (&text[..i], Some(bounds), value_type, default)
        }
        None => {
            let (head, default) = split_top(text, '=');
            let (name, value_type) = split_top(head, ':');
            (name, None, value_type, default)
        }
    };
    let name = name.trim();
    let (name, variadic) = match name.strip_suffix("...") {
        Some(name) => (name, true),
        None => (name, false),
    };
    let mut bound_list = Vec::new();
    let mut remaining = bounds.unwrap_or("");
    while !remaining.trim().is_empty() {
        let (bound, rest) = split_top(remaining, ',');
        bound_list.extend(non_empty(Some(bound)));
        remaining = rest.unwrap_or("");
    }
    Ok(TemplateParameterSpec {
        name: check_name(name, text)?,
        variadic,
        bounds: bound_list,
        value_type: non_empty(value_type),
        default: non_empty(default),
    })
}

pub fn parse_operation(text: &str) -> Result<OperationSpec, ParseError> {
    let (visibility, rest) = split_visibility(text);
    let open = rest.find('(').ok_or_else(|| ParseError(format!("missing \"(\" in {text:?}")))?;
    let (name, template_parameters) = match rest[..open].split_once('<') {
        Some((name, templates)) => {
            let templates = templates
                .trim_end()
                .strip_suffix('>')
                .ok_or_else(|| ParseError(format!("missing \">\" in {text:?}")))?;
            let names =
                templates.split(',').map(|t| check_name(t, text)).collect::<Result<Vec<_>, _>>()?;
            (name, names)
        }
        None => (&rest[..open], Vec::new()),
    };
    let name = check_name(name, text)?;
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
    Ok(OperationSpec { visibility, name, template_parameters, parameters, return_type })
}

/// A classifier of that name if there is exactly one, else the name as free text.
pub fn resolve_type(model: &Model, text: &str) -> TypeRef {
    let mut matches = model.elements().filter(|e| e.kind().is_classifier() && e.name() == text);
    match (matches.next(), matches.next()) {
        (Some(e), None) => TypeRef::Element(e.id()),
        _ => TypeRef::Text(text.to_owned()),
    }
}

/// A type name as seen from `scope` (the member that uses it): a template parameter of the
/// scope or of an element owning it, else a classifier by its qualified name (`java.util.List`,
/// `std::vector`), else [`resolve_type`].
pub fn resolve_type_in(model: &Model, scope: Id, name: &str) -> TypeRef {
    let name = name.trim();
    for owner in std::iter::once(scope).chain(model.ancestors(scope)) {
        if let Some(t) = template_parameters(model, owner)
            .into_iter()
            .find(|&t| model.get(t).is_some_and(|t| t.name() == name))
        {
            return TypeRef::Element(t);
        }
    }
    let path: Vec<&str> =
        if name.contains("::") { name.split("::").collect() } else { name.split('.').collect() };
    if path.len() > 1 {
        let mut current = model.root();
        for segment in &path {
            let child = model.children(current).iter().copied().find(|&c| {
                model.get(c).is_some_and(|e| {
                    e.name() == segment.trim()
                        && (matches!(e.kind(), ElementKind::Package) || e.kind().is_classifier())
                })
            });
            match child {
                Some(c) => current = c,
                None => return resolve_type(model, name),
            }
        }
        if model.get(current).is_some_and(|e| e.kind().is_classifier()) {
            return TypeRef::Element(current);
        }
    }
    resolve_type(model, name)
}

/// Parses a type as written (`List<Foo>`, `Map<K, List<V>>`, `Foo[]`, `? extends Number`) and
/// links its parts to the model as seen from `scope`. Text that links to nothing stays text.
pub fn parse_type(model: &Model, scope: Id, text: &str) -> TypeRef {
    let text = text.trim();
    let parsed = parse_type_expr(model, scope, text);
    if parsed.ids().is_empty() { TypeRef::Text(text.to_owned()) } else { parsed }
}

fn parse_type_expr(model: &Model, scope: Id, text: &str) -> TypeRef {
    let text = text.trim();
    let (prefix, rest) = ["? extends ", "? super ", "const "]
        .into_iter()
        .find_map(|p| text.strip_prefix(p).map(|r| (p, r.trim_start())))
        .unwrap_or(("", text));
    // The modifier: trailing `[]`, `[10]`, `...`, `*`, `&`, `?` after the base and arguments.
    let mut end = rest.len();
    loop {
        let head = rest[..end].trim_end();
        if let Some(h) = head.strip_suffix("...") {
            end = h.len();
        } else if let Some(h) = head.strip_suffix(['*', '&', '?']) {
            end = h.len();
        } else if head.ends_with(']')
            && let Some(open) = head.rfind('[')
        {
            end = open;
        } else {
            end = head.len();
            break;
        }
    }
    let modifier = rest[end..].trim().to_owned();
    let core = &rest[..end];
    let (base, arguments) = match core.find('<') {
        Some(open) if core.ends_with('>') => {
            let inner = &core[open + 1..core.len() - 1];
            let mut arguments = Vec::new();
            let mut remaining = inner;
            while !remaining.trim().is_empty() {
                let (argument, rest) = split_top(remaining, ',');
                arguments.push(parse_type_expr(model, scope, argument));
                remaining = rest.unwrap_or("");
            }
            (&core[..open], arguments)
        }
        _ => (core, Vec::new()),
    };
    let base = base.trim();
    let base = if base.is_empty() || base == "?" {
        TypeRef::Text(base.to_owned())
    } else {
        resolve_type_in(model, scope, base)
    };
    if prefix.is_empty() && modifier.is_empty() && arguments.is_empty() {
        return base;
    }
    TypeRef::Expr(Box::new(TypeExpr { base, arguments, prefix: prefix.to_owned(), modifier }))
}

/// Applies edited text to an attribute, operation or other element (which is just renamed).
pub fn apply_member_text(tx: &mut Tx<'_>, id: Id, text: &str) -> Result<()> {
    let invalid = |e: ParseError| ModelError::Invalid(e.to_string());
    let kind = tx.model().element(id)?.kind().clone();
    match kind {
        ElementKind::Attribute(_) => {
            let spec = parse_attribute(text).map_err(invalid)?;
            let type_ref = spec.type_text.as_deref().map(|t| parse_type(tx.model(), id, t));
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
            // The operation's own template parameters, matched by position, come first so that
            // the types below can refer to them.
            let templates = template_parameters(tx.model(), id);
            for (i, name) in spec.template_parameters.iter().enumerate() {
                match templates.get(i) {
                    Some(&t) => tx.rename(t, name.as_str())?,
                    None => {
                        let kind =
                            ElementKind::TemplateParameter(TemplateParameterProps::default());
                        tx.create_at(id, i, ElementData::new(name.as_str(), kind))?;
                    }
                }
            }
            for &t in templates.iter().skip(spec.template_parameters.len()) {
                tx.delete(t)?;
            }
            let return_type = spec.return_type.as_deref().map(|t| parse_type(tx.model(), id, t));
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
                let type_ref = p.type_text.as_deref().map(|t| parse_type(tx.model(), id, t));
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
        ElementKind::TemplateParameter(_) => {
            let spec = parse_template_parameter(text).map_err(invalid)?;
            let parse = |t: &str| parse_type(tx.model(), id, t);
            let bounds = spec.bounds.iter().map(|b| parse(b)).collect();
            let type_ref = spec.value_type.as_deref().map(parse);
            let default = spec.default.as_deref().map(parse);
            tx.update(id, |data| {
                data.name = spec.name;
                if let ElementKind::TemplateParameter(p) = &mut data.kind {
                    p.parameter = if type_ref.is_some() {
                        TemplateParameterKind::Value
                    } else {
                        TemplateParameterKind::Type
                    };
                    p.type_ref = type_ref;
                    p.bounds = bounds;
                    p.default = default;
                    p.is_variadic = spec.variadic;
                }
            })
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
        assert_eq!(a.multiplicity, Some(Multiplicity::ZERO_OR_MORE));
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
    fn parses_type_expressions() {
        let (doc, ids) = order_system();
        let model = doc.model();
        let scope = ids.order;
        let customer = TypeRef::Element(ids.customer);
        let expr = |base, arguments, prefix: &str, modifier: &str| {
            TypeRef::Expr(Box::new(TypeExpr {
                base,
                arguments,
                prefix: prefix.into(),
                modifier: modifier.into(),
            }))
        };
        let text = |s: &str| TypeRef::Text(s.into());
        assert_eq!(parse_type(model, scope, "Customer"), customer);
        assert_eq!(parse_type(model, scope, " domain.Customer "), customer, "qualified");
        assert_eq!(
            parse_type(model, scope, "List<Customer>[]"),
            expr(text("List"), vec![customer.clone()], "", "[]")
        );
        assert_eq!(
            parse_type(model, scope, "Map<String, List<? extends Customer>>"),
            expr(
                text("Map"),
                vec![
                    text("String"),
                    expr(
                        text("List"),
                        vec![expr(customer.clone(), vec![], "? extends ", "")],
                        "",
                        ""
                    )
                ],
                "",
                ""
            )
        );
        assert_eq!(
            parse_type(model, scope, "Customer..."),
            expr(customer.clone(), vec![], "", "...")
        );
        // Nothing modeled: the text stays as it was typed.
        assert_eq!(parse_type(model, scope, "Map<String,Item>"), text("Map<String,Item>"));
        assert_eq!(parse_type(model, scope, "int[]"), text("int[]"));
        let t = parse_type(model, scope, "List<Customer>[]");
        assert_eq!(type_label(model, &Some(t)).as_deref(), Some("List<Customer>[]"));
    }

    #[test]
    fn array_brackets_are_not_a_multiplicity() {
        let a = parse_attribute("items : Item[]").unwrap();
        assert_eq!((a.type_text.as_deref(), a.multiplicity), (Some("Item[]"), None));
        let a = parse_attribute("items : Item [1..*]").unwrap();
        assert_eq!(
            (a.type_text.as_deref(), a.multiplicity),
            (Some("Item"), Some(Multiplicity::ONE_OR_MORE))
        );
        let a = parse_attribute("items [2]").unwrap();
        assert_eq!(a.multiplicity.map(|m| m.to_string()).as_deref(), Some("2"));
    }

    #[test]
    fn generic_operations_own_their_template_parameters() {
        let (mut doc, ids) = order_system();
        let op = doc
            .edit("op", |tx| {
                tx.create(ids.order, ElementData::new("max", ElementKind::operation(None)))
            })
            .unwrap();
        let text = "+ max<T>(a : T, b : List<T>) : T";
        doc.edit("edit", |tx| apply_member_text(tx, op, text)).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert_eq!(member_text(model, op), text);
        let templates = template_parameters(model, op);
        assert_eq!(templates.len(), 1);
        let t = templates[0];
        let ElementKind::Operation(p) = model.get(op).unwrap().kind() else { panic!() };
        assert_eq!(p.return_type, Some(TypeRef::Element(t)));
        assert_eq!(model.referrers(t).len(), 3, "return type, a, and the argument of b");

        doc.edit("edit", |tx| apply_member_text(tx, op, "+ max(a : T) : T")).unwrap();
        let model = doc.model();
        model.check().unwrap();
        assert!(template_parameters(model, op).is_empty());
        assert_eq!(member_text(model, op), "+ max(a : T) : T");
        assert!(parse_operation("f<T(x)").is_err());
    }

    #[test]
    fn template_parameters_read_and_write_astahs_form() {
        let spec = parse_template_parameter("T>Number,Comparable<T>=Integer").unwrap();
        assert_eq!(spec.name, "T");
        assert_eq!(spec.bounds, ["Number", "Comparable<T>"]);
        assert_eq!((spec.value_type, spec.default.as_deref()), (None, Some("Integer")));
        let spec = parse_template_parameter("N:int=10").unwrap();
        assert_eq!(
            (spec.value_type.as_deref(), spec.default.as_deref()),
            (Some("int"), Some("10"))
        );
        let spec = parse_template_parameter("Ts...").unwrap();
        assert!(spec.variadic && spec.name == "Ts");
        assert!(parse_template_parameter(">Number").is_err());

        let (mut doc, ids) = order_system();
        let t = doc
            .edit("setup", |tx| {
                let kind = ElementKind::TemplateParameter(TemplateParameterProps::default());
                tx.create(ids.order, ElementData::new("T", kind))
            })
            .unwrap();
        for text in ["T>Customer=Customer", "N:int=10", "E"] {
            doc.edit("edit", |tx| apply_member_text(tx, t, text)).unwrap();
            assert_eq!(member_text(doc.model(), t), text);
        }
        doc.edit("edit", |tx| apply_member_text(tx, t, "T>Customer")).unwrap();
        let ElementKind::TemplateParameter(p) = doc.model().get(t).unwrap().kind() else {
            panic!()
        };
        assert_eq!(p.bounds, [TypeRef::Element(ids.customer)]);
    }

    #[test]
    fn class_template_parameters_are_in_scope_for_members() {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let (t, attr) = doc
            .edit("setup", |tx| {
                let c = tx.create(root, ElementData::new("Box", ElementKind::class()))?;
                let parameter = ElementKind::TemplateParameter(TemplateParameterProps::default());
                let t = tx.create(c, ElementData::new("T", parameter))?;
                let a = tx.create(c, ElementData::new("a", ElementKind::attribute(None)))?;
                Ok((t, a))
            })
            .unwrap();
        doc.edit("edit", |tx| apply_member_text(tx, attr, "- value : T")).unwrap();
        let ElementKind::Attribute(p) = doc.model().get(attr).unwrap().kind() else { panic!() };
        assert_eq!(p.type_ref, Some(TypeRef::Element(t)));
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
