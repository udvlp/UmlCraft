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
        ElementKind::State(p) if p.submachine.is_some() => NodeTool::SubmachineState.key(),
        ElementKind::Instance(p) => NodeTool::for_instance(p.instance).key(),
        ElementKind::Association(p) if p.class.is_some() => "associationClass",
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

/// Whether the model tree shows `e` folded until it is unfolded by hand, as it does after a
/// project is opened: everything holding details, such as a class with its members and
/// structograms, an IS-A with its specializations, a relationship with its participations or
/// an interaction with its diagram and lifelines. Only the project and packages stay unfolded;
/// nothing to fold is not folded.
pub fn tree_folded(e: &Element) -> bool {
    !e.children().is_empty() && !matches!(e.kind(), ElementKind::Model | ElementKind::Package)
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

/// What the Astah export reports as left out (`dropped <key>`): its own text, else the name of
/// the element kind, else the key itself (a newer export tool may report keys this build does
/// not know).
pub fn export_dropped(key: &str) -> String {
    export_dropped_in(&rust_i18n::locale(), key)
}

fn export_dropped_in(locale: &str, key: &str) -> String {
    // `t!` answers a missing key with "<locale>.<key>", so ask whether there is a text instead.
    ["export_dropped", "kind"]
        .iter()
        .find_map(|prefix| crate::_rust_i18n_try_translate(locale, format!("{prefix}.{key}")))
        .map_or_else(|| key.to_owned(), |text| text.into_owned())
}

/// `text` with every line break turned into a space.
fn one_line(text: &str) -> String {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Locale keys the app builds at runtime (`kind.{tool}`, `export_dropped.{key}`, …), which a
/// comparison of the literal `t!("…")` keys cannot see. Each must have a text in both
/// languages, or the UI shows (and a new element is named) "en.kind.boundary".
#[cfg(test)]
mod runtime_keys {
    use std::collections::BTreeSet;

    use super::export_dropped_in;

    const LOCALES: [&str; 2] = ["de", "en"];
    const PALETTE: &str = include_str!("../../umlcraft-notation/src/palette.rs");
    const ELEMENT: &str = include_str!("../../umlcraft-model/src/element.rs");
    const REVERSE: &str = include_str!("../../umlcraft-reverse/src/lang.rs");
    const ASTA_EXPORT: &str = include_str!("../../../tools/asta-export/AstaExport.java");

    /// Whether `locale` itself (not the fallback language) has a text for `key`.
    fn has(locale: &str, key: &str) -> bool {
        crate::_rust_i18n_backend().translate(locale, key).is_some()
    }

    /// The keys of `keys` that some language has no text for, with the language.
    fn missing(keys: impl IntoIterator<Item = String>) -> Vec<String> {
        let keys: Vec<String> = keys.into_iter().collect();
        LOCALES
            .iter()
            .flat_map(|l| keys.iter().filter(|k| !has(l, k)).map(move |k| format!("{l}.{k}")))
            .collect()
    }

    /// The string literals the match arms return in the function of `src` that starts with
    /// `start`, e.g. every key of `NodeTool::key`.
    fn returned(src: &str, start: &str) -> Vec<String> {
        let body = &src[src.find(start).unwrap_or_else(|| panic!("{start} not found"))..];
        let body = &body[..body.find("\n    }\n").expect("end of the function")];
        let values: Vec<String> = body
            .lines()
            .filter_map(|l| l.split_once("=> \""))
            .map(|(_, rest)| rest[..rest.find('"').expect("closing quote")].to_owned())
            .collect();
        assert!(values.len() > 10, "{start}: {values:?}");
        values
    }

    fn node_tools() -> Vec<String> {
        returned(PALETTE, "impl NodeTool {\n    pub fn key(self)")
    }

    fn type_names() -> Vec<String> {
        returned(ELEMENT, "pub fn type_name(&self)")
    }

    #[test]
    fn tools_and_element_kinds_have_names() {
        // New elements are named `kind.{tool}` (`Workspace::create_node`, the tree's "New"
        // menu); the ribbon and menus say `tool.{tool}`, the note tool `tool.note`.
        let nodes = node_tools();
        let mut keys: Vec<String> = nodes.iter().map(|k| format!("kind.{k}")).collect();
        keys.extend(nodes.iter().filter(|k| *k != "comment").map(|k| format!("tool.{k}")));
        let edges = returned(PALETTE, "impl EdgeTool {\n    pub fn key(self)");
        keys.extend(edges.iter().map(|k| format!("tool.{k}")));
        // The tree, the properties panel and error messages say `kind.{type_name}`.
        keys.extend(type_names().iter().map(|k| format!("kind.{k}")));
        assert_eq!(missing(keys), Vec::<String>::new());
    }

    #[test]
    fn import_skips_and_structogram_kinds_have_names() {
        let mut keys: Vec<String> =
            returned(REVERSE, "impl Skip {").iter().map(|k| format!("skipped.{k}")).collect();
        keys.extend(umlcraft_nsd::ElementKind::ALL.iter().map(|k| format!("nsd.kind_{}", k.tag())));
        assert_eq!(missing(keys), Vec::<String>::new());
    }

    /// The keys `AstaExport.java` passes to `drop("…")` as literals. Keys it puts together
    /// (`"er" + …`, `type + "Diagram"`) and receivers of a call (`"port".equals(…)`) are left
    /// out; the test lists those itself.
    fn dropped_literals() -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        for line in ASTA_EXPORT.lines() {
            let Some((_, call)) = line.split_once("drop(") else { continue };
            let pieces: Vec<&str> = call.split('"').collect();
            for i in (1..pieces.len()).step_by(2) {
                let before = pieces[i - 1].trim_end();
                let after = pieces.get(i + 1).map_or("", |p| p.trim_start());
                if before.ends_with('+') || after.starts_with('+') || after.starts_with('.') {
                    continue;
                }
                assert!(pieces[i].chars().all(|c| c.is_ascii_alphanumeric()), "{line}");
                keys.insert(pieces[i].to_owned());
            }
        }
        assert!(keys.len() > 50, "{keys:?}");
        keys
    }

    #[test]
    fn everything_the_astah_export_leaves_out_has_a_label() {
        let mut keys = dropped_literals();
        // `"er" + Flag`, `type + "Diagram"` and `kind + "Diagram"`.
        for built in ["erUnique", "erDerived", "erMultivalued"] {
            keys.insert(built.to_owned());
        }
        for diagram in ["activity", "stateMachine", "component", "deployment"] {
            keys.insert(format!("{diagram}Diagram"));
        }
        // `drop(type)`: any element kind.
        keys.extend(type_names());
        let unlabeled: Vec<String> = LOCALES
            .iter()
            .flat_map(|l| {
                keys.iter()
                    .filter(|k| {
                        !has(l, &format!("export_dropped.{k}")) && !has(l, &format!("kind.{k}"))
                    })
                    .map(move |k| format!("{l}: {k}"))
            })
            .collect();
        assert_eq!(unlabeled, Vec::<String>::new());
    }

    #[test]
    fn export_labels_fall_back_to_the_kind_and_then_the_key() {
        assert_eq!(export_dropped_in("en", "erUnique"), "UNIQUE of columns");
        // Only `kind.slot` exists.
        assert_eq!(export_dropped_in("en", "slot"), "Slot");
        assert_eq!(export_dropped_in("de", "slot"), "Slot");
        assert_eq!(export_dropped_in("de", "erUnique"), "UNIQUE von Spalten");
        assert_eq!(export_dropped_in("en", "somethingNewer"), "somethingNewer");
    }
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;
    use umlcraft_model::{DiagramKind, ElementData, ElementKind};

    use super::{one_line, tree_children, tree_folded};

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
    fn details_start_folded() {
        let (mut doc, ids) = order_system();
        let (empty, outer, interaction) = doc
            .edit("more", |tx| {
                let empty =
                    tx.create(ids.domain, ElementData::new("Empty", ElementKind::class()))?;
                let outer =
                    tx.create(ids.domain, ElementData::new("Outer", ElementKind::class()))?;
                tx.create(outer, ElementData::new("Inner", ElementKind::class()))?;
                let interaction =
                    tx.create(ids.domain, ElementData::new("Order", ElementKind::Interaction))?;
                let diagram = ElementKind::diagram(DiagramKind::Sequence);
                tx.create(interaction, ElementData::new("Order", diagram))?;
                Ok((empty, outer, interaction))
            })
            .unwrap();
        let model = doc.model();
        let folded = |id| tree_folded(model.get(id).unwrap());
        assert!(folded(ids.customer), "a class with its members");
        assert!(!folded(ids.domain), "a package");
        assert!(!folded(model.root()), "the project");
        assert!(!folded(empty), "nothing to fold");
        assert!(folded(outer), "a class with a nested class");
        assert!(folded(interaction), "an interaction holding its diagram");
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
