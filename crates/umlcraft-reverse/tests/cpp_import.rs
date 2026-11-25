//! C++ imports into a fresh model, one per fixture: the model as text is compared with
//! `expected.model` (write it with `UPDATE_SNAPSHOTS=1` and review the diff), and the rules each
//! fixture is about are checked directly.

mod common;

use common::{at, attribute_type, links_to};
use umlcraft_model::{Document, ElementKind, Id, Model, Multiplicity};
use umlcraft_reverse::lang::Skip;
use umlcraft_reverse::{Report, SourceLanguage};

const CPP: SourceLanguage = SourceLanguage::Cpp;

fn import(doc: &mut Document, case: &str) -> Report {
    common::import(doc, CPP, case)
}

fn fresh(case: &str) -> (Document, Report) {
    common::fresh(CPP, case)
}

fn check(case: &str, model: &Model) {
    common::check_model(CPP, case, model)
}

fn kind(model: &Model, path: &str) -> &'static str {
    model.get(at(model, path)).unwrap().kind().type_name()
}

fn skipped(report: &Report, skip: Skip) -> usize {
    report.skipped.get(&skip).copied().unwrap_or(0)
}

/// The far end of the association that replaced `attribute` of `class`.
fn association_end(model: &Model, class: &str, attribute: &str) -> umlcraft_model::AssociationEnd {
    let owner = at(model, class);
    model
        .elements()
        .find_map(|e| match e.kind() {
            ElementKind::Association(p) => (0..2)
                .find(|&i| p.ends[i].role == attribute && p.ends[1 - i].participant == owner)
                .map(|i| p.ends[i].clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no association for {class}.{attribute}"))
}

/// The operations of `class`, in order.
fn operations(model: &Model, class: &str) -> Vec<Id> {
    model
        .children(at(model, class))
        .iter()
        .copied()
        .filter(|&c| matches!(model.get(c).unwrap().kind(), ElementKind::Operation(_)))
        .collect()
}

#[test]
fn namespaces_nest_and_anonymous_ones_vanish() {
    let (doc, _) = fresh("namespaces");
    let model = doc.model();
    at(model, "shop.Order");
    at(model, "shop.billing.Invoice");
    at(model, "shop.billing.tax.Rate");
    at(model, "Hidden");
    at(model, "CHandle");
    check("namespaces", model);
}

#[test]
fn kinds_map_to_classes_and_enumerations() {
    let (doc, report) = fresh("kinds");
    let model = doc.model();
    assert_eq!(kind(model, "Point"), "class");
    assert_eq!(kind(model, "Pair"), "class");
    assert_eq!(kind(model, "Value"), "class");
    assert_eq!(kind(model, "Color"), "enumeration");
    assert_eq!(kind(model, "Shape"), "enumeration");
    // The forward declaration and the definition are one class.
    at(model, "Later.n");
    at(model, "Opaque");
    assert_eq!(skipped(&report, Skip::AnonymousClass), 1);
    check("kinds", model);
}

#[test]
fn access_sections() {
    let (doc, _) = fresh("access");
    check("access", doc.model());
}

#[test]
fn modifiers() {
    let (doc, report) = fresh("modifiers");
    let model = doc.model();
    let shape = model.get(at(model, "Shape")).unwrap();
    assert!(matches!(shape.kind(), ElementKind::Class(p) if p.is_abstract));
    assert_eq!(skipped(&report, Skip::DeletedFunction), 1);
    assert_eq!(skipped(&report, Skip::Friend), 1);
    check("modifiers", model);
}

#[test]
fn documentation_comments() {
    let (doc, _) = fresh("docs");
    let model = doc.model();
    let documentation =
        |path: &str| model.get(at(model, path)).unwrap().data().documentation.clone();
    assert_eq!(documentation("Document.save"), "Doxygen with an exclamation mark.");
    assert_eq!(documentation("Document.pages"), "");
    check("docs", model);
}

#[test]
fn base_classes_are_generalizations() {
    let (doc, _) = fresh("inheritance");
    check("inheritance", doc.model());
}

#[test]
fn templates() {
    let (doc, report) = fresh("templates");
    assert_eq!(skipped(&report, Skip::Specialization), 2);
    check("templates", doc.model());
}

#[test]
fn pointers_references_and_arrays_keep_their_modifiers() {
    let (doc, _) = fresh("pointers_arrays");
    let model = doc.model();
    assert_eq!(attribute_type(model, "Node", "buffer"), "int[16]");
    assert_eq!(attribute_type(model, "Node", "label"), "const char*");
    check("pointers_arrays", model);
}

#[test]
fn definitions_join_their_declarations() {
    let (doc, report) = fresh("definitions");
    let model = doc.model();
    let documentation =
        |path: &str| model.get(at(model, path)).unwrap().data().documentation.clone();
    assert_eq!(documentation("shop.Order.ship"), "Ships it.");
    // `label` joins its declaration through the definition's own `using`; `note` is added.
    assert_eq!(operations(model, "shop.Order").len(), 6);
    assert_eq!(skipped(&report, Skip::UnknownOwner), 1);
    // `shop::helper` and `main`.
    assert_eq!(skipped(&report, Skip::FreeMember), 2);
    check("definitions", model);
}

#[test]
fn aliases_stand_for_their_targets() {
    let (doc, _) = fresh("typedefs");
    let model = doc.model();
    let items = association_end(model, "Store", "items");
    assert_eq!(items.multiplicity, Some(Multiplicity::MANY));
    assert_eq!(association_end(model, "Store", "best").multiplicity, Some(Multiplicity::ONE));
    assert_eq!(attribute_type(model, "Store", "id"), "unsigned long");
    assert!(links_to(model, "Store", "origin", "Point"));
    assert!(links_to(model, "Store", "size", "Size"));
    check("typedefs", model);
}

#[test]
fn names_resolve_through_usings_and_namespaces() {
    let (doc, _) = fresh("resolution");
    let model = doc.model();
    check("resolution", model);
    assert!(links_to(model, "acme.app.Shop", "price", "acme.lib.Money"));
    assert!(links_to(model, "acme.app.Shop", "buyer", "acme.lib.Customer"));
    assert!(links_to(model, "acme.app.Shop", "address", "acme.lib.Customer.Address"));
    assert!(links_to(model, "acme.app.Shop", "root", "acme.Root"));
    assert!(links_to(model, "acme.app.Shop", "second", "acme.lib.Money"));
    assert!(links_to(model, "acme.app.Shop", "third", "acme.lib.Money"));
    assert!(links_to(model, "acme.app.Shop", "fourth", "acme.lib.Money"));
    assert!(links_to(model, "acme.app.Shop", "missing", "Missing"));
    assert!(links_to(model, "acme.app.Shop", "widget", "other.Widget"));
    at(model, "acme.app.Shop.name");
}

#[test]
fn attributes_of_class_types_become_associations() {
    let (doc, report) = fresh("associations");
    let model = doc.model();
    let end = |attribute: &str| association_end(model, "shop.Order", attribute);
    check("associations", model);
    assert_eq!(end("customer").multiplicity, None);
    assert_eq!(end("payer").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("reader").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("lines").multiplicity, Some(Multiplicity::MANY));
    assert_eq!(end("lines").constraints, ["ordered"]);
    assert_eq!(end("pointers").multiplicity, Some(Multiplicity::MANY));
    assert_eq!(end("sorted").constraints, ["ordered", "unique"]);
    assert_eq!(end("tags").constraints, ["unique"]);
    assert_eq!(end("account").multiplicity, Some(Multiplicity::ONE));
    assert_eq!(end("shared").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("maybe").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("extra").multiplicity, Some(Multiplicity::MANY));
    // References, values and built-in types stay attributes.
    at(model, "shop.Order.owner");
    at(model, "shop.Order.note");
    at(model, "shop.Order.count");
    assert_eq!(report.associations, 12);
}

#[test]
fn preprocessor_reads_the_first_branches() {
    let (doc, report) = fresh("preprocessor");
    let model = doc.model();
    at(model, "WinOnly");
    at(model, "Service.fast");
    assert_eq!(skipped(&report, Skip::PreprocessorBranch), 3);
    assert_eq!(skipped(&report, Skip::Macro), 3);
    check("preprocessor", model);
}

#[test]
fn syntax_errors_skip_only_their_file() {
    let (doc, report) = fresh("syntax_error");
    at(doc.model(), "broken.Fine");
    assert_eq!(report.failed.len(), 1);
}

#[test]
fn encodings_are_detected() {
    let (doc, report) = fresh("encoding");
    let model = doc.model();
    assert_eq!(model.get(at(model, "Legacy")).unwrap().data().documentation, "Grüße aus München.");
    assert_eq!(report.guessed_encodings.len(), 1);
}

#[test]
fn every_import_is_one_undo_step_and_repeats_without_change() {
    let cases = [
        "namespaces",
        "kinds",
        "access",
        "modifiers",
        "docs",
        "inheritance",
        "templates",
        "pointers_arrays",
        "definitions",
        "typedefs",
        "resolution",
        "associations",
        "preprocessor",
    ];
    for case in cases {
        let mut doc = Document::new(Model::new("t"));
        let before = doc.model().clone();
        import(&mut doc, case);
        let after = doc.model().clone();
        assert!(doc.undo());
        assert_eq!(*doc.model(), before, "{case}: undo");
        assert!(doc.redo());
        import(&mut doc, case);
        assert_eq!(*doc.model(), after, "{case}: second import");
    }
}

#[test]
fn a_new_import_matches_what_is_there() {
    let mut doc = Document::new(Model::new("t"));
    import(&mut doc, "reimport/v1");
    let model = doc.model();
    let order = at(model, "model.Order");
    let number = at(model, "model.Order.number");
    let ship = at(model, "model.Order.ship");
    import(&mut doc, "reimport/v2");
    let model = doc.model();
    assert_eq!(at(model, "model.Order"), order);
    assert_eq!(at(model, "model.Order.number"), number);
    // `ship(int)` keeps its element; `ship(int) const` is a second operation.
    let ops = operations(model, "model.Order");
    assert_eq!(ops.len(), 2);
    assert!(ops.contains(&ship));
    check("reimport", model);
}

#[test]
fn a_forward_declaration_keeps_the_types_associations() {
    let mut doc = Document::new(Model::new("t"));
    import(&mut doc, "forward/full");
    let roles = |model: &Model| -> Vec<String> {
        let mut roles: Vec<String> = model
            .elements()
            .filter_map(|e| match e.kind() {
                ElementKind::Association(p) => Some(p.ends.iter().map(|e| e.role.clone())),
                _ => None,
            })
            .flatten()
            .filter(|r| !r.is_empty())
            .collect();
        roles.sort();
        roles
    };
    assert_eq!(roles(doc.model()), ["customer"]);
    // Only Customer's header: `class Order;` there says nothing about Order's fields.
    import(&mut doc, "forward/partial");
    assert_eq!(roles(doc.model()), ["customer", "order"]);
}
