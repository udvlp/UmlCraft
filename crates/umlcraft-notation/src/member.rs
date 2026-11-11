//! Text form of class members, as shown in compartments and typed when editing:
//! `- name : Type [0..*] = init` for attributes, `+ op(in p : T = 1) : R` for operations.

use std::collections::HashMap;
use std::fmt;

use umlcraft_model::Refusal;

use umlcraft_model::{
    AssociationEnd, ElementData, ElementKind, Id, Model, ModelError, Multiplicity,
    ParameterDirection, ParameterDirections, ParameterProps, Result, Style, TemplateParameterKind,
    TemplateParameterProps, Tx, TypeExpr, TypeRef, Visibility,
};

pub fn type_label(model: &Model, type_ref: &Option<TypeRef>) -> Option<String> {
    match type_ref {
        Some(TypeRef::Element(id)) => model.contains(*id).then(|| display_name(model, *id)),
        Some(t) => Some(type_text(model, t)),
        None => None,
    }
}

/// A type as written: `Foo`, `List<Foo>`, `? extends Number`, `Foo[]`. Arguments are separated
/// by a bare comma (`Map<String,Foo>`), as Astah writes them.
pub fn type_text(model: &Model, type_ref: &TypeRef) -> String {
    match type_ref {
        // A bound class without a name reads as what it binds: `Pair<int,Holder>`.
        TypeRef::Element(id) => display_name(model, *id),
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

/// The name an element is shown by: its own, or for a bound class without one what it binds
/// (`Pair<int,Holder>`), as Astah shows it.
pub fn display_name(model: &Model, id: Id) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    match e.kind() {
        ElementKind::Class(p) if e.name().is_empty() => {
            p.binding.as_ref().map(|b| type_text(model, b)).unwrap_or_default()
        }
        _ => e.name().to_owned(),
    }
}

/// What a «bind» dependency of a bound class on its template substitutes, as Astah writes it
/// below the line: `<T1->int,T2->Holder>`. `None` unless `client` binds `template`.
pub fn binding_text(model: &Model, client: Id, template: Id) -> Option<String> {
    let ElementKind::Class(p) = model.get(client)?.kind() else { return None };
    if p.bound_template() != Some(template) {
        return None;
    }
    let Some(TypeRef::Expr(e)) = &p.binding else { return None };
    let parameters = template_parameters(model, template);
    let pairs: Vec<String> = e
        .arguments
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let formal = parameters.get(i).and_then(|&t| model.get(t)).map(|t| t.name());
            format!("{}->{}", formal.unwrap_or_default(), type_text(model, a))
        })
        .collect();
    Some(format!("<{}>", pairs.join(",")))
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

/// The text of a member as typed when editing it, and as a class box shows it unless its style
/// leaves something out.
pub fn member_text(model: &Model, id: Id) -> String {
    member_body(model, id, &Style::default())
}

/// Whether a classifier box with `style` shows the member: its visibility may be filtered out.
pub fn member_shown(model: &Model, id: Id, style: &Style) -> bool {
    let Some(e) = model.get(id) else { return false };
    let hidden = match e.kind() {
        ElementKind::Attribute(_) => &style.hidden_attributes,
        ElementKind::Operation(_) => &style.hidden_operations,
        _ => return true,
    };
    !hidden.contains(&e.data().visibility)
}

/// The text of a member as a classifier box with `style` shows it: what the box leaves out is
/// left out, and the member's stereotypes follow its visibility, as in Astah
/// (`+ <<enum constant>> PLAIN : Terrain`).
pub fn member_label(model: &Model, id: Id, style: &Style) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    let mut text = member_body(model, id, style);
    let stereotypes = &e.data().stereotypes;
    let shown = match e.kind() {
        ElementKind::Attribute(_) => !style.hide_attribute_stereotypes,
        ElementKind::Operation(_) => !style.hide_operation_stereotypes,
        _ => false,
    };
    if shown && !stereotypes.is_empty() {
        let at = if style.hide_member_visibility { 0 } else { text.find(' ').map_or(0, |i| i + 1) };
        text.insert_str(at, &format!("{} ", crate::scene::stereotype_text(stereotypes)));
    }
    text
}

fn member_body(model: &Model, id: Id, style: &Style) -> String {
    let Some(e) = model.get(id) else { return String::new() };
    let symbol = e.data().visibility.symbol();
    let vis = if style.hide_member_visibility { String::new() } else { format!("{symbol} ") };
    let name = e.name();
    match e.kind() {
        ElementKind::Attribute(p) => {
            let mut s = format!("{vis}{}{name}", if p.is_derived { "/" } else { "" });
            if let Some(t) = type_label(model, &p.type_ref).filter(|_| !style.hide_attribute_types)
            {
                s.push_str(&format!(" : {t}"));
            }
            if let Some(m) = p.multiplicity {
                s.push_str(&format!(" [{m}]"));
            }
            if !p.default_value.is_empty() && !style.hide_initial_values {
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
                        Some(parameter_text(model, c.name(), pp, style))
                    }
                    _ => None,
                })
                .filter(|_| !style.hide_parameters)
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
            let mut s = format!("{vis}{name}{templates}({})", params.join(", "));
            if let Some(t) = type_label(model, &p.return_type).filter(|_| !style.hide_return_types)
            {
                s.push_str(&format!(" : {t}"));
            }
            s
        }
        ElementKind::TemplateParameter(_) => template_parameter_text(model, id),
        _ => name.to_owned(),
    }
}

fn parameter_text(model: &Model, name: &str, p: &ParameterProps, style: &Style) -> String {
    let mut s = match (p.direction, style.parameter_directions) {
        (_, ParameterDirections::Hidden) => name.to_owned(),
        (ParameterDirection::Out, _) => format!("out {name}"),
        (ParameterDirection::InOut, _) => format!("inout {name}"),
        // Astah writes `in` with a space before and an extra one after: `total( in  a : int)`.
        (_, ParameterDirections::All) => format!(" in  {name}"),
        _ => name.to_owned(),
    };
    if let Some(t) = type_label(model, &p.type_ref).filter(|_| !style.hide_parameter_types) {
        s.push_str(&format!(" : {t}"));
    }
    if !p.default_value.is_empty() {
        s.push_str(&format!(" = {}", p.default_value));
    }
    s
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError(pub Refusal);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
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

/// Byte index of the first `sep` outside of `<>`, `()`, `[]` and quoted strings (`"a,b"`,
/// `','`). A `:` that is part of `::` does not count. A `<` opens a bracket only right after a
/// name (`List<`), so a comparison like `x < y` in a default does not; a `>` closes only an open
/// `<`.
fn find_top(s: &str, sep: char) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut angles = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut previous: Option<char> = None;
    for (i, c) in s.char_indices() {
        let before = previous.replace(c);
        if let Some(q) = quote {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                c if c == q => quote = None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '<' if before.is_some_and(|b| b.is_alphanumeric() || b == '_') => angles += 1,
            '>' if angles > 0 => angles -= 1,
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            c if c == sep && depth == 0 && angles == 0 => {
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

/// Byte index of the `)` closing the `(` at byte index `open`, quoted strings left aside.
fn matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (i, c) in s[open..].char_indices() {
        if let Some(q) = quote {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                c if c == q => quote = None,
                _ => {}
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
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

/// Basic types to offer for an attribute, or with `void` for an operation's result: the types
/// the model already uses as text, most used first (a C# model says `string` and `bool`), then
/// Java's, leaving out those the model spells its own way.
pub fn basic_types(model: &Model, operation: bool) -> Vec<String> {
    const DEFAULTS: [&str; 7] = ["int", "double", "boolean", "String", "char", "long", "float"];
    // At most this many, besides `void`.
    const MAX: usize = 8;
    // Spellings of the same type in different languages.
    const SAME: [&[&str]; 2] = [&["boolean", "bool"], &["String", "string", "str"]];
    let mut used: HashMap<String, usize> = HashMap::new();
    for e in model.elements() {
        e.kind().visit_type_refs(&mut |t| {
            if let TypeRef::Text(text) = t {
                let simple = text.chars().all(|c| c.is_alphanumeric() || "_:.".contains(c));
                if simple && !text.is_empty() && text != "void" {
                    *used.entry(text.clone()).or_default() += 1;
                }
            }
        });
    }
    let mut types: Vec<(String, usize)> = used.into_iter().collect();
    types.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut types: Vec<String> = types.into_iter().map(|(t, _)| t).take(MAX).collect();
    let alike = |a: &str, b: &str| {
        a.eq_ignore_ascii_case(b) || SAME.iter().any(|same| same.contains(&a) && same.contains(&b))
    };
    for default in DEFAULTS {
        if types.len() < MAX && !types.iter().any(|t| alike(t, default)) {
            types.push(default.to_owned());
        }
    }
    if operation {
        types.push("void".to_owned());
    }
    types
}

/// An association end's role as its label shows it and as it is edited in place: visibility,
/// name and constraints, `- buyer {ordered}`; only the constraints without a name.
pub fn role_text(end: &AssociationEnd) -> String {
    let role = (!end.role.is_empty()).then(|| format!("{} {}", end.visibility.symbol(), end.role));
    let constraints = end.constraints.iter().map(|c| format!("{{{c}}}"));
    role.into_iter().chain(constraints).collect::<Vec<_>>().join(" ")
}

/// A role typed as [`role_text`] shows it: the visibility if one is given, the name and the
/// constraints, which may also share braces (`{ordered, unique}`).
pub fn parse_role(text: &str) -> (Option<Visibility>, String, Vec<String>) {
    let (visibility, rest) = split_visibility(text);
    let (name, constraints) = rest.split_at(rest.find('{').unwrap_or(rest.len()));
    let constraints = constraints
        .split(['{', '}', ','])
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_owned)
        .collect();
    (visibility, name.trim().to_owned(), constraints)
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
    let mut bare = name;
    while let Some(rest) = bare.strip_suffix("[]") {
        bare = rest.trim_end();
    }
    if bare.is_empty() || bare.contains(['(', ')', ':', '=', '[', ']']) {
        return Err(ParseError(Refusal::MemberNoName(text.to_owned())));
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
        // `arr[]` without a type is a C array named so, not a multiplicity.
        && !(find_top(head, ':').is_none() && head[open + 1..head.len() - 1].trim().is_empty())
    {
        let m = &head[open + 1..head.len() - 1];
        multiplicity =
            Some(m.parse().map_err(|_| ParseError(Refusal::MemberMultiplicity(m.to_owned())))?);
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
    // C++ declares parameters by type alone: `f(: int)`.
    let name = if name.trim().is_empty() && type_text.is_some() {
        String::new()
    } else {
        check_name(name, text)?
    };
    Ok(ParameterSpec {
        direction,
        name,
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

/// The length of a C++ operator name at the start of `s` (`operator<`, `operator()`,
/// `operator bool`), up to the `(` of its parameters.
fn operator_name_len(s: &str) -> Option<usize> {
    let after = s.strip_prefix("operator")?;
    if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let symbol = after.trim_start();
    let skip = after.len() - symbol.len();
    let end = match symbol.strip_prefix("()") {
        Some(rest) => 2 + rest.find('(')?,
        None => symbol.find('(')?,
    };
    Some("operator".len() + skip + end)
}

pub fn parse_operation(text: &str) -> Result<OperationSpec, ParseError> {
    let (visibility, rest) = split_visibility(text);
    let missing =
        |c: char| ParseError(Refusal::MemberMissing { missing: c, text: text.to_owned() });
    let open = rest.find('(').ok_or_else(|| missing('('))?;
    if let Some(len) = operator_name_len(rest) {
        let name = rest[..len].trim_end().to_owned();
        return finish_operation(text, visibility, name, Vec::new(), &rest[len..]);
    }
    let (name, template_parameters) = match rest[..open].split_once('<') {
        Some((name, templates)) => {
            let templates = templates.trim_end().strip_suffix('>').ok_or_else(|| missing('>'))?;
            let names =
                templates.split(',').map(|t| check_name(t, text)).collect::<Result<Vec<_>, _>>()?;
            (name, names)
        }
        None => (&rest[..open], Vec::new()),
    };
    let name = check_name(name, text)?;
    finish_operation(text, visibility, name, template_parameters, &rest[open..])
}

/// Parses the parameters and the return type of an operation; `rest` starts at their `(`.
fn finish_operation(
    text: &str,
    visibility: Option<Visibility>,
    name: String,
    template_parameters: Vec<String>,
    rest: &str,
) -> Result<OperationSpec, ParseError> {
    let close = matching_paren(rest, 0).ok_or_else(|| {
        ParseError(Refusal::MemberMissing { missing: ')', text: text.to_owned() })
    })?;
    let params_text = &rest[1..close];
    let tail = rest[close + 1..].trim();
    let return_type = match tail.strip_prefix(':') {
        Some(t) => non_empty(Some(t)),
        None if tail.is_empty() => None,
        None => {
            return Err(ParseError(Refusal::MemberUnexpected(tail.to_owned())));
        }
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
    let parsed = parse_type_expr(model, scope, text, 1);
    if parsed.ids().is_empty() { TypeRef::Text(text.to_owned()) } else { parsed }
}

/// The type at nesting level `depth` (1 at the top). At the model's limit the rest stays text.
fn parse_type_expr(model: &Model, scope: Id, text: &str, depth: usize) -> TypeRef {
    let text = text.trim();
    if depth >= umlcraft_model::MAX_TYPE_DEPTH {
        return TypeRef::Text(text.to_owned());
    }
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
                arguments.push(parse_type_expr(model, scope, argument, depth + 1));
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

/// The type `text` stands for in `scope`. Text that reads as `old` does keeps `old`, so an
/// edit elsewhere in the member does not lose a link a name alone cannot find again (two
/// classes `Item`, an anonymous bound class).
fn retype_text(
    model: &Model,
    scope: Id,
    old: Option<&TypeRef>,
    text: Option<&str>,
) -> Option<TypeRef> {
    match (old, text) {
        (Some(old), Some(text)) if type_text(model, old) == text.trim() => Some(old.clone()),
        (_, text) => text.map(|t| parse_type(model, scope, t)),
    }
}

/// Applies edited text to an attribute, operation or other element (which is just renamed).
pub fn apply_member_text(tx: &mut Tx<'_>, id: Id, text: &str) -> Result<()> {
    let invalid = |e: ParseError| ModelError::Refused(e.0);
    let kind = tx.model().element(id)?.kind().clone();
    match kind {
        ElementKind::Attribute(p) => {
            let spec = parse_attribute(text).map_err(invalid)?;
            let type_ref =
                retype_text(tx.model(), id, p.type_ref.as_ref(), spec.type_text.as_deref());
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
        ElementKind::Operation(op) => {
            let spec = parse_operation(text).map_err(invalid)?;
            // The operation's own template parameters, matched by position, come first so that
            // the types below can refer to them.
            let templates = template_parameters(tx.model(), id);
            for (i, written) in spec.template_parameters.iter().enumerate() {
                // `Ts...` is the variadic parameter `Ts`.
                let (name, variadic) = match written.strip_suffix("...") {
                    Some(name) => (name.trim_end(), true),
                    None => (written.as_str(), false),
                };
                match templates.get(i) {
                    Some(&t) => tx.update(t, |data| {
                        data.name = name.to_owned();
                        if let ElementKind::TemplateParameter(p) = &mut data.kind {
                            p.is_variadic = variadic;
                        }
                    })?,
                    None => {
                        let props =
                            TemplateParameterProps { is_variadic: variadic, ..Default::default() };
                        let kind = ElementKind::TemplateParameter(props);
                        tx.create_at(id, i, ElementData::new(name, kind))?;
                    }
                }
            }
            for &t in templates.iter().skip(spec.template_parameters.len()) {
                tx.delete(t)?;
            }
            let return_type =
                retype_text(tx.model(), id, op.return_type.as_ref(), spec.return_type.as_deref());
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
                // A return parameter is not part of the text, so it stays as it is.
                .filter(|&c| {
                    tx.model().get(c).is_some_and(|e| {
                        matches!(e.kind(), ElementKind::Parameter(p)
                            if p.direction != ParameterDirection::Return)
                    })
                })
                .collect();
            // Parameters are matched by position, so that what the text does not show (their
            // multiplicity, stereotypes, comments on them) and their ids stay; a type written
            // as before keeps its link even where its name alone would be ambiguous.
            let wanted = spec.parameters.len();
            for (i, p) in spec.parameters.into_iter().enumerate() {
                let kept = old.get(i).and_then(|&o| match tx.model().get(o)?.kind() {
                    ElementKind::Parameter(props) => Some((o, props.type_ref.clone())),
                    _ => None,
                });
                let old_type = kept.as_ref().and_then(|(_, t)| t.as_ref());
                let type_ref = retype_text(tx.model(), id, old_type, p.type_text.as_deref());
                let default_value = p.default.unwrap_or_default();
                match kept {
                    Some((o, _)) => tx.update(o, |data| {
                        data.name = p.name;
                        if let ElementKind::Parameter(props) = &mut data.kind {
                            props.type_ref = type_ref;
                            props.direction = p.direction;
                            props.default_value = default_value;
                        }
                    })?,
                    None => {
                        let props = ParameterProps {
                            type_ref,
                            direction: p.direction,
                            multiplicity: None,
                            default_value,
                        };
                        tx.create(id, ElementData::new(p.name, ElementKind::Parameter(props)))?;
                    }
                }
            }
            for &p in old.iter().skip(wanted) {
                tx.delete(p)?;
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
                return Err(ModelError::Refused(Refusal::EmptyName));
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
    fn basic_types_follow_what_the_model_uses() {
        let (mut doc, ids) = order_system();
        let java = basic_types(doc.model(), false);
        // The sample's own text types first (`Money` is not modeled), then Java's.
        assert_eq!(java[..4], ["Money", "String", "int", "double"].map(String::from));
        assert!(java.contains(&"boolean".to_owned()));
        assert!(!java.contains(&"void".to_owned()) && java.len() <= 8);
        assert_eq!(basic_types(doc.model(), true).last().map(String::as_str), Some("void"));
        // A model that writes C# offers its spelling, and not Java's beside it.
        doc.edit("c#", |tx| {
            for (name, ty) in [("a", "string"), ("b", "string"), ("c", "bool"), ("d", "decimal")] {
                let kind = ElementKind::attribute(Some(TypeRef::Text(ty.into())));
                tx.create(ids.customer, ElementData::new(name, kind))?;
            }
            Ok(())
        })
        .unwrap();
        let types = basic_types(doc.model(), false);
        assert_eq!(types[0], "string");
        assert!(types.contains(&"bool".to_owned()) && types.contains(&"decimal".to_owned()));
        // `boolean` came only from Java's list; `String` the sample writes itself.
        assert!(!types.contains(&"boolean".to_owned()));
    }

    #[test]
    fn roles_read_back_as_shown() {
        let end = AssociationEnd {
            role: "buyer".into(),
            visibility: Visibility::Private,
            constraints: vec!["ordered".into(), "unique".into()],
            ..AssociationEnd::new(Id::generate())
        };
        assert_eq!(role_text(&end), "- buyer {ordered} {unique}");
        let parsed = parse_role(&role_text(&end));
        assert_eq!(parsed, (Some(Visibility::Private), "buyer".into(), end.constraints.clone()));
        assert_eq!(parse_role("# owner {ordered, unique}").2, end.constraints);
        assert_eq!(parse_role(" owner "), (None, "owner".into(), Vec::new()));
        assert_eq!(parse_role(""), (None, String::new(), Vec::new()));
        let unnamed = AssociationEnd { role: String::new(), ..end };
        assert_eq!(role_text(&unnamed), "{ordered} {unique}");
    }

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

    /// What a box leaves out, as Astah shows it (measured with its command line's images).
    #[test]
    fn member_labels_follow_the_box_style() {
        let (mut doc, ids) = order_system();
        let mut member = |text: &str, stereotype: &str| {
            let text = text.to_owned();
            let stereotype = stereotype.to_owned();
            doc.edit("member", |tx| {
                let id = if text.contains('(') {
                    tx.create(
                        ids.order,
                        ElementData::new("", ElementKind::Operation(Default::default())),
                    )?
                } else {
                    tx.create(
                        ids.order,
                        ElementData::new("", ElementKind::Attribute(Default::default())),
                    )?
                };
                apply_member_text(tx, id, &text)?;
                if !stereotype.is_empty() {
                    tx.update(id, |d| d.stereotypes = vec![stereotype])?;
                }
                Ok(id)
            })
            .unwrap()
        };
        let secret = member("- secret : String = \"x\"", "id");
        let swap = member("# swap(out x : int, inout y : int, z : int) : void", "op");
        let model = doc.model();
        let label = |id, f: fn(&mut Style)| {
            let mut style = Style::default();
            f(&mut style);
            member_label(model, id, &style)
        };
        assert_eq!(label(secret, |_| {}), "- <<id>> secret : String = \"x\"");
        assert_eq!(
            label(swap, |_| {}),
            "# <<op>> swap(out x : int, inout y : int, z : int) : void"
        );
        assert_eq!(
            label(secret, |s| s.hide_member_visibility = true),
            "<<id>> secret : String = \"x\""
        );
        assert_eq!(label(secret, |s| s.hide_attribute_types = true), "- <<id>> secret = \"x\"");
        assert_eq!(label(secret, |s| s.hide_initial_values = true), "- <<id>> secret : String");
        assert_eq!(
            label(secret, |s| s.hide_attribute_stereotypes = true),
            "- secret : String = \"x\""
        );
        let hidden = |s: &mut Style| s.parameter_directions = ParameterDirections::Hidden;
        assert_eq!(label(swap, hidden), "# <<op>> swap(x : int, y : int, z : int) : void");
        let all = |s: &mut Style| s.parameter_directions = ParameterDirections::All;
        assert_eq!(
            label(swap, all),
            "# <<op>> swap(out x : int, inout y : int,  in  z : int) : void"
        );
        assert_eq!(label(swap, |s| s.hide_parameters = true), "# <<op>> swap() : void");
        assert_eq!(
            label(swap, |s| s.hide_return_types = true),
            "# <<op>> swap(out x : int, inout y : int, z : int)"
        );
        let types = |s: &mut Style| {
            s.hide_parameter_types = true;
            s.parameter_directions = ParameterDirections::Hidden;
        };
        assert_eq!(label(swap, types), "# <<op>> swap(x, y, z) : void");
        assert_eq!(
            label(swap, |s| s.hide_operation_stereotypes = true),
            "# swap(out x : int, inout y : int, z : int) : void"
        );
        let private = Style { hidden_attributes: vec![Visibility::Private], ..Style::default() };
        assert!(!member_shown(model, secret, &private));
        assert!(member_shown(model, swap, &private));
        // Editing always offers the whole text.
        assert_eq!(member_text(model, secret), "- secret : String = \"x\"");
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
    fn editing_an_operation_keeps_its_parameters() {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let (op, param) = doc
            .edit("setup", |tx| {
                let c = tx.create(root, ElementData::new("C", ElementKind::class()))?;
                let op = tx.create(c, ElementData::new("f", ElementKind::operation(None)))?;
                let props = ParameterProps {
                    type_ref: Some(TypeRef::Text("int".into())),
                    multiplicity: Some(Multiplicity::ZERO_OR_MORE),
                    ..Default::default()
                };
                let mut data = ElementData::new("xs", ElementKind::Parameter(props));
                data.stereotypes = vec!["in".into()];
                let param = tx.create(op, data)?;
                Ok((op, param))
            })
            .unwrap();
        doc.edit("edit", |tx| apply_member_text(tx, op, "- f(ys : int, flag : bool)")).unwrap();
        let model = doc.model();
        let params = model.children(op);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0], param, "the first parameter keeps its id");
        let p = model.get(param).unwrap();
        assert_eq!(p.name(), "ys");
        assert_eq!(p.data().stereotypes, ["in"]);
        let ElementKind::Parameter(props) = p.kind() else { panic!() };
        assert_eq!(props.multiplicity, Some(Multiplicity::ZERO_OR_MORE));
        doc.edit("fewer", |tx| apply_member_text(tx, op, "- f()")).unwrap();
        assert!(doc.model().children(op).is_empty());
    }

    #[test]
    fn deeply_nested_types_end_in_text() {
        let model = Model::new("m");
        let text = format!("{}int{}", "List<".repeat(1000), ">".repeat(1000));
        let parsed = parse_type(&model, model.root(), &text);
        assert!(parsed.depth() <= umlcraft_model::MAX_TYPE_DEPTH, "{}", parsed.depth());
    }

    #[test]
    fn quoted_defaults_hide_their_commas_and_parens() {
        let o = parse_operation(r#"f(s : String = "a,b", c : char = ')', n : int)"#).unwrap();
        assert_eq!(o.parameters.len(), 3);
        assert_eq!(o.parameters[0].default.as_deref(), Some(r#""a,b""#));
        assert_eq!(o.parameters[1].default.as_deref(), Some("')'"));
        assert_eq!(o.parameters[2].name, "n");
        let o = parse_operation("f(a : bool = x < y, b : int = x > y, c : List<int>)").unwrap();
        assert_eq!(o.parameters.len(), 3);
        assert_eq!(o.parameters[0].default.as_deref(), Some("x < y"));
        assert_eq!(o.parameters[2].type_text.as_deref(), Some("List<int>"));
    }

    #[test]
    fn parses_operator_names_and_c_arrays() {
        for (text, name, count) in [
            ("+ operator=( : const Shape&) : Shape&", "operator=", 1),
            ("+ operator==(other : const Shape&) : bool", "operator==", 1),
            ("+ operator<(other : T) : bool", "operator<", 1),
            ("+ operator<<(out : ostream&) : ostream&", "operator<<", 1),
            ("+ operator()(a : int, b : int) : int", "operator()", 2),
            ("+ operator bool() : bool", "operator bool", 0),
            ("+ operator[](i : int) : T&", "operator[]", 1),
        ] {
            let o = parse_operation(text).unwrap();
            assert_eq!((o.name.as_str(), o.parameters.len()), (name, count), "{text}");
        }
        assert_eq!(parse_operation("+ operatorX()").unwrap().name, "operatorX");
        let a = parse_attribute("- arr[]").unwrap();
        assert_eq!((a.name.as_str(), a.multiplicity), ("arr[]", None));
    }

    #[test]
    fn edits_keep_what_the_text_does_not_show() {
        let mut doc = Document::new(Model::new("m"));
        let root = doc.model().root();
        let (item, op, result) = doc
            .edit("setup", |tx| {
                let a = tx.create(root, ElementData::new("a", ElementKind::Package))?;
                let b = tx.create(root, ElementData::new("b", ElementKind::Package))?;
                tx.create(a, ElementData::new("Item", ElementKind::class()))?;
                let item = tx.create(b, ElementData::new("Item", ElementKind::class()))?;
                let c = tx.create(root, ElementData::new("C", ElementKind::class()))?;
                let mut kind = ElementKind::operation(None);
                if let ElementKind::Operation(p) = &mut kind {
                    p.return_type = Some(TypeRef::Element(item));
                }
                let op = tx.create(c, ElementData::new("f", kind))?;
                let props =
                    ParameterProps { direction: ParameterDirection::Return, ..Default::default() };
                let result = tx.create(op, ElementData::new("", ElementKind::Parameter(props)))?;
                Ok((item, op, result))
            })
            .unwrap();
        // A rename keeps the link to the second `Item`, which the name alone would not find.
        doc.edit("edit", |tx| apply_member_text(tx, op, "+ g(: int) : Item")).unwrap();
        let model = doc.model();
        model.check().unwrap();
        let ElementKind::Operation(p) = model.get(op).unwrap().kind() else { panic!() };
        assert_eq!(p.return_type, Some(TypeRef::Element(item)));
        let children = model.children(op);
        assert!(children.contains(&result), "the return parameter stays");
        assert_eq!(children.len(), 2);

        // Variadic template parameters.
        doc.edit("edit", |tx| apply_member_text(tx, op, "+ g<Ts...>(xs : Ts...)")).unwrap();
        let model = doc.model();
        let templates = template_parameters(model, op);
        let ElementKind::TemplateParameter(t) = model.get(templates[0]).unwrap().kind() else {
            panic!()
        };
        assert!(t.is_variadic);
        assert_eq!(model.get(templates[0]).unwrap().name(), "Ts");
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
