//! Localized names for model elements.

use rust_i18n::t;
use umlcraft_model::{Element, ElementKind, Id, Model, Visibility};
use umlcraft_notation::palette::NodeTool;
use umlcraft_notation::sequence::{interaction_use_text, message_text, operator_keyword};

/// The element's metaclass; activity nodes, pseudostates and instances say which kind they are.
pub fn kind_label(e: &Element) -> String {
    let key = match e.kind() {
        ElementKind::ActivityNode(p) => NodeTool::for_activity_node(p.node).key(),
        ElementKind::Pseudostate(p) => NodeTool::for_pseudostate(p.pseudostate).key(),
        ElementKind::Instance(p) => NodeTool::for_instance(p.instance).key(),
        _ if umlcraft_notation::palette::is_subsystem(e) => NodeTool::Subsystem.key(),
        kind => kind.type_name(),
    };
    t!(format!("kind.{key}")).to_string()
}

/// The element's name on one line, or its kind in parentheses when it has none.
pub fn display_name(e: &Element) -> String {
    if e.name().is_empty() { format!("({})", kind_label(e)) } else { one_line(e.name()) }
}

/// Like [`display_name`], but unnamed sequence elements and flows show what the diagram shows:
/// a message its label, a fragment its operator, an operand or flow its guard. Always one line:
/// line breaks, as in use case names, become spaces.
pub fn tree_name(model: &Model, e: &Element) -> String {
    one_line(&shown_name(model, e))
}

/// The children of `e` in the model tree. In the model and in packages they are sorted:
/// packages, then diagrams and structograms, interactions, activities and state machines, then
/// the other elements by kind and name, relationships last. Elsewhere their order means
/// something (members, the parts of an interaction) and stays.
pub fn tree_children(model: &Model, e: &Element) -> Vec<Id> {
    let mut children = e.children().to_vec();
    if matches!(e.kind(), ElementKind::Model | ElementKind::Package) {
        children.sort_by_cached_key(|&c| match model.get(c) {
            Some(child) => (tree_rank(child.kind()), tree_name(model, child).to_lowercase()),
            None => ((u8::MAX, u8::MAX), String::new()),
        });
    }
    children
}

/// Where an element of `kind` goes among its siblings in the model tree: its group, then its
/// place in the group.
fn tree_rank(kind: &ElementKind) -> (u8, u8) {
    use ElementKind as K;
    match kind {
        K::Package => (0, 0),
        K::Diagram(_) => (1, 0),
        K::Structogram(_) => (1, 1),
        K::Interaction => (2, 0),
        K::Activity => (2, 1),
        K::StateMachine => (2, 2),
        k if k.is_relationship() => (4, 0),
        K::Class(_) => (3, 0),
        K::Interface(_) => (3, 1),
        K::Enumeration(_) => (3, 2),
        K::DataType(_) => (3, 3),
        K::Component(_) => (3, 4),
        K::Artifact(_) => (3, 5),
        K::Node(_) => (3, 6),
        K::Actor(_) => (3, 7),
        K::UseCase(_) => (3, 8),
        K::SystemBoundary => (3, 9),
        K::Instance(_) => (3, 10),
        K::Comment(_) => (3, 12),
        _ => (3, 11),
    }
}

fn shown_name(model: &Model, e: &Element) -> String {
    let guard =
        |g: &str| if g.trim().is_empty() { String::new() } else { format!("[{}]", g.trim()) };
    let shown = match e.kind() {
        ElementKind::Slot(p) => format!("{} = {}", e.name(), p.value),
        _ if !e.name().is_empty() => String::new(),
        ElementKind::Message(_) => message_text(model, e.id()),
        ElementKind::CombinedFragment(p) => operator_keyword(p.operator).to_owned(),
        ElementKind::InteractionUse(_) => interaction_use_text(model, e.id()),
        ElementKind::InteractionOperand(p) => guard(&p.guard),
        ElementKind::ActivityEdge(p) => guard(&p.guard),
        ElementKind::Transition(p) => p.label(),
        ElementKind::InternalTransition(p) => p.label(),
        ElementKind::Class(_) => umlcraft_notation::member::display_name(model, e.id()),
        // A structogram is named by its root's text (the signature).
        ElementKind::Structogram(p) => p.structogram.text().join("\n"),
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

/// `text` with every line break turned into a space.
fn one_line(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{DiagramKind, ElementData, ElementKind};

    use super::{one_line, tree_children};

    #[test]
    fn packages_list_their_contents_in_order() {
        let (mut doc, ids) = order_system();
        doc.edit("more", |tx| {
            tx.create(ids.domain, ElementData::new("Zone", ElementKind::Package))?;
            tx.create(ids.domain, ElementData::new("Address", ElementKind::class()))?;
            tx.create(ids.domain, ElementData::new("Paid", ElementKind::interface()))?;
            let diagram = ElementKind::diagram(DiagramKind::Class);
            tx.create(ids.domain, ElementData::new("A first look", diagram))
        })
        .unwrap();
        let model = doc.model();
        let domain = model.get(ids.domain).unwrap();
        let kinds: Vec<(u8, String)> = tree_children(model, domain)
            .into_iter()
            .map(|c| model.get(c).unwrap())
            .map(|c| (super::tree_rank(c.kind()).0, c.name().to_owned()))
            .collect();
        // Groups never go backwards; within classes, names run alphabetically.
        assert!(kinds.windows(2).all(|w| w[0].0 <= w[1].0), "{kinds:?}");
        assert_eq!(kinds[0], (0, "Zone".to_owned()));
        let classes: Vec<&str> = model
            .get(ids.domain)
            .map(|d| tree_children(model, d))
            .unwrap()
            .into_iter()
            .filter_map(|c| model.get(c))
            .filter(|c| matches!(c.kind(), ElementKind::Class(_)))
            .map(|c| c.name())
            .collect();
        let mut sorted = classes.clone();
        sorted.sort_by_key(|n| n.to_lowercase());
        assert_eq!(classes, sorted);
        // A class keeps its members as they are.
        let customer = model.get(ids.customer).unwrap();
        assert_eq!(tree_children(model, customer), customer.children());
    }

    #[test]
    fn line_breaks_become_spaces() {
        assert_eq!(
            one_line("Wareneingang\naus Einkauf \r\n\nbearbeiten"),
            "Wareneingang aus Einkauf bearbeiten"
        );
        assert_eq!(one_line("Kunde"), "Kunde");
    }
}
