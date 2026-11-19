//! The settings of ER diagram elements.

use super::*;

pub(super) const ER_NOTATIONS: [(&str, ErNotation); 3] =
    [("chen", ErNotation::Chen), ("ie", ErNotation::Ie), ("idef1x", ErNotation::Idef1x)];

/// The settings of the elements of ER diagrams `update_element` takes; `notation` is set on
/// its own, as switching converts the diagram.
pub(super) const ER_KEYS: [&str; 31] = [
    "unique",
    "foreign_key",
    "discriminator",
    "er_display",
    "physical_names",
    "weak",
    "associative",
    "key",
    "derived",
    "multivalued",
    "identifying",
    "total",
    "disjoint",
    "cardinality",
    "cardinality_here",
    "look_here",
    "ratios",
    "role",
    "physical_name",
    "datatype",
    "length_precision",
    "not_null",
    "default_value",
    "check",
    "values",
    "checks",
    "indexes",
    "auto_increment",
    "virtual",
    "on_delete",
    "on_update",
];

/// Writes the ER settings of `args` into `kind`, an entity, attribute, relationship, IS-A or
/// participation.
pub(super) fn set_er(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    let type_name = kind.type_name();
    for key in ER_KEYS.into_iter().filter(|k| args.get(k).is_some()) {
        let value = &args[key];
        let flag = || value.as_bool().ok_or_else(|| format!("`{key}` is true or false"));
        let text = || {
            value.as_str().map(|s| s.trim().to_owned()).ok_or_else(|| format!("`{key}` is text"))
        };
        let strings = || -> Result<Vec<String>, String> {
            serde_json::from_value(value.clone()).map_err(|_| format!("`{key}` is a list of texts"))
        };
        // A referential action, or "" for none.
        let action = || -> Result<Option<ErAction>, String> {
            let text = text()?;
            if text.is_empty() {
                return Ok(None);
            }
            text.parse().map(Some).map_err(|_| {
                format!(
                    "`{key}` is one of cascade, set null, set default, restrict, no action, \"\", \
                     not `{text}`"
                )
            })
        };
        // An id, or "" for none.
        let reference = || -> Result<Option<Id>, String> {
            let text = text()?;
            if text.is_empty() { Ok(None) } else { parse_id(&text).map(Some) }
        };
        match (key, &mut *kind) {
            ("unique", ElementKind::ErAttribute(p)) => p.unique = flag()?,
            ("foreign_key", ElementKind::ErAttribute(p)) => p.foreign_key = reference()?,
            ("discriminator", ElementKind::ErIsA(p)) => p.discriminator = reference()?,
            ("er_display", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                p.er_display = match text()?.as_str() {
                    "attributes" => ErDisplay::Attributes,
                    "keys" => ErDisplay::Keys,
                    "name" => ErDisplay::Name,
                    other => {
                        return Err(format!(
                            "`er_display` is one of attributes, keys, name, not `{other}`"
                        ));
                    }
                }
            }
            ("physical_names", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                p.physical_names = flag()?
            }
            ("weak", ElementKind::ErEntity(p)) => p.weak = flag()?,
            ("associative", ElementKind::ErEntity(p)) => p.associative = flag()?,
            ("physical_name", ElementKind::ErEntity(p)) => p.physical_name = text()?,
            ("key", ElementKind::ErAttribute(p)) => {
                p.key = match text()?.as_str() {
                    "" => None,
                    "primary" => Some(ErKey::Primary),
                    "partial" => Some(ErKey::Partial),
                    other => {
                        return Err(format!(
                            "`key` is one of primary, partial, \"\", not `{other}`"
                        ));
                    }
                }
            }
            ("derived", ElementKind::ErAttribute(p)) => p.derived = flag()?,
            ("multivalued", ElementKind::ErAttribute(p)) => p.multivalued = flag()?,
            ("physical_name", ElementKind::ErAttribute(p)) => p.physical_name = text()?,
            ("datatype", ElementKind::ErAttribute(p)) => p.datatype = text()?,
            ("length_precision", ElementKind::ErAttribute(p)) => p.length_precision = text()?,
            ("not_null", ElementKind::ErAttribute(p)) => p.not_null = flag()?,
            ("default_value", ElementKind::ErAttribute(p)) => p.default_value = text()?,
            ("check", ElementKind::ErAttribute(p)) => p.check = text()?,
            ("auto_increment", ElementKind::ErAttribute(p)) => p.auto_increment = flag()?,
            ("virtual", ElementKind::ErAttribute(p)) => p.virtual_column = flag()?,
            ("datatype", ElementKind::ErDomain(p)) => p.datatype = text()?,
            ("length_precision", ElementKind::ErDomain(p)) => p.length_precision = text()?,
            ("not_null", ElementKind::ErDomain(p)) => p.not_null = flag()?,
            ("default_value", ElementKind::ErDomain(p)) => p.default_value = text()?,
            ("check", ElementKind::ErDomain(p)) => p.check = text()?,
            ("values", ElementKind::ErDomain(p)) => p.values = strings()?,
            ("checks", ElementKind::ErEntity(p)) => p.checks = strings()?,
            ("indexes", ElementKind::ErEntity(p)) => {
                p.indexes = serde_json::from_value(value.clone())
                    .map_err(|_| "`indexes` is a list of { name, columns, unique }".to_owned())?;
            }
            ("identifying", ElementKind::ErRelationship(p)) => p.identifying = flag()?,
            ("on_delete", ElementKind::ErRelationship(p)) => p.on_delete = action()?,
            ("on_update", ElementKind::ErRelationship(p)) => p.on_update = action()?,
            ("total", ElementKind::ErIsA(p)) => p.total = flag()?,
            ("disjoint", ElementKind::ErIsA(p)) => {
                p.disjoint = match text()?.as_str() {
                    "" => None,
                    "disjoint" => Some(true),
                    "overlapping" => Some(false),
                    other => {
                        return Err(format!(
                            "`disjoint` is one of disjoint, overlapping, \"\", not `{other}`"
                        ));
                    }
                }
            }
            ("total", ElementKind::ErParticipation(p)) => p.total = flag()?,
            ("role", ElementKind::ErParticipation(p)) => p.role = text()?,
            ("cardinality" | "cardinality_here", ElementKind::ErParticipation(p)) => {
                let text = text()?;
                let c = if text.is_empty() {
                    None
                } else {
                    Some(text.parse().map_err(|e: umlcraft_model::ParseCardinalityError| {
                        format!("`{}` is no cardinality, e.g. [0,1], [1,n], 0..*", e.0)
                    })?)
                };
                if key == "cardinality" {
                    p.cardinality = c;
                } else {
                    p.look_here = c;
                }
            }
            ("look_here" | "ratios", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                if args["look_here"] == true && args["ratios"] == true {
                    return Err("`look_here` and `ratios` exclude each other".into());
                }
                // Turning one on turns the other off, as the properties panel does.
                let on = flag()?;
                if key == "look_here" {
                    p.look_here = on;
                    p.ratios &= !on;
                } else {
                    p.ratios = on;
                    p.look_here &= !on;
                }
            }
            _ => return Err(format!("a {type_name} has no `{key}`")),
        }
    }
    Ok(())
}
