//! Localized names for model elements.

use rust_i18n::t;
use umlcraft_model::{Element, ElementKind, Id, Model, Visibility};
use umlcraft_notation::sequence::{message_text, operator_keyword};

pub fn kind_label(e: &Element) -> String {
    t!(format!("kind.{}", e.kind().type_name())).to_string()
}

/// The element's name, or its kind in parentheses when it has none.
pub fn display_name(e: &Element) -> String {
    if e.name().is_empty() { format!("({})", kind_label(e)) } else { e.name().to_owned() }
}

/// Like [`display_name`], but unnamed sequence elements show what the diagram shows: a
/// message its label, a fragment its operator, an operand its guard.
pub fn tree_name(model: &Model, e: &Element) -> String {
    let shown = match e.kind() {
        _ if !e.name().is_empty() => String::new(),
        ElementKind::Message(_) => message_text(model, e.id()),
        ElementKind::CombinedFragment(p) => operator_keyword(p.operator).to_owned(),
        ElementKind::InteractionOperand(p) if !p.guard.trim().is_empty() => {
            format!("[{}]", p.guard.trim())
        }
        _ => String::new(),
    };
    if shown.is_empty() { display_name(e) } else { shown }
}

pub fn owner_path(model: &Model, id: Id) -> String {
    let names: Vec<String> = model
        .ancestors(id)
        .iter()
        .rev()
        .filter_map(|&a| model.get(a))
        .map(|e| tree_name(model, e))
        .collect();
    if names.is_empty() { "—".to_owned() } else { names.join(" › ") }
}

pub const VISIBILITIES: [Visibility; 4] =
    [Visibility::Public, Visibility::Protected, Visibility::Package, Visibility::Private];

pub fn visibility_label(v: Visibility) -> String {
    let key = match v {
        Visibility::Public => "visibility.public",
        Visibility::Protected => "visibility.protected",
        Visibility::Package => "visibility.package",
        Visibility::Private => "visibility.private",
    };
    format!("{} {}", v.symbol(), t!(key))
}
