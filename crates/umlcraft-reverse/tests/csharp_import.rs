//! C# imports into a fresh model, one per fixture: the model as text is compared with
//! `expected.model` (write it with `UPDATE_SNAPSHOTS=1` and review the diff), and the rules each
//! fixture is about are checked directly.

mod common;

use common::{at, attribute_type, links_to, qualified};
use umlcraft_model::{Document, ElementKind, Id, Model, Multiplicity, Rect};
use umlcraft_reverse::{Report, SourceLanguage};

const CSHARP: SourceLanguage = SourceLanguage::CSharp;

fn import(doc: &mut Document, case: &str) -> Report {
    common::import(doc, CSHARP, case)
}

fn fresh(case: &str) -> (Document, Report) {
    common::fresh(CSHARP, case)
}

fn check(case: &str, model: &Model) {
    common::check_model(CSHARP, case, model)
}

fn kind(model: &Model, path: &str) -> &'static str {
    model.get(at(model, path)).unwrap().kind().type_name()
}

/// The supertype links of `class` as `generalization Base` / `realization IFoo`.
fn supertypes(model: &Model, class: &str) -> Vec<String> {
    let id = at(model, class);
    model
        .children(id)
        .iter()
        .filter_map(|&c| match model.get(c)?.kind() {
            ElementKind::Generalization(p) => {
                Some(format!("generalization {}", qualified(model, p.general)))
            }
            ElementKind::Realization(p) => {
                Some(format!("realization {}", qualified(model, p.supplier)))
            }
            _ => None,
        })
        .collect()
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

#[test]
fn namespaces_nest_and_resolve_outwards() {
    let (doc, _) = fresh("namespaces");
    let model = doc.model();
    at(model, "Shop.Orders.Order");
    at(model, "Shop.Billing.Invoice");
    at(model, "Shop.Delivery.Parcel");
    at(model, "Global");
    // `Catalog` from `Shop`, around `Shop.Orders` and `Shop.Delivery`.
    assert!(links_to(model, "Shop.Orders.Order", "catalog", "Shop.Catalog"));
    assert!(links_to(model, "Shop.Delivery.Parcel", "catalog", "Shop.Catalog"));
    // `Orders.Order` relative to `Shop`.
    assert!(links_to(model, "Shop.Billing.Invoice", "order", "Shop.Orders.Order"));
    check("namespaces", model);
}

#[test]
fn kinds_map_to_classes_interfaces_and_enumerations() {
    let (doc, report) = fresh("kinds");
    let model = doc.model();
    assert_eq!(kind(model, "Kinds.IShape"), "interface");
    assert_eq!(kind(model, "Kinds.Point"), "class");
    assert_eq!(kind(model, "Kinds.Color"), "enumeration");
    assert_eq!(kind(model, "Kinds.Money"), "class");
    assert_eq!(report.skipped.len(), 2);
    check("kinds", model);
}

#[test]
fn modifiers() {
    let (doc, _) = fresh("modifiers");
    check("modifiers", doc.model());
}

#[test]
fn documentation_comments() {
    let (doc, _) = fresh("docs");
    let model = doc.model();
    let order = model.get(at(model, "Docs.Order")).unwrap();
    assert!(order.data().documentation.contains("An order."));
    check("docs", model);
}

#[test]
fn base_lists_follow_what_the_targets_are() {
    let (doc, _) = fresh("inheritance");
    let model = doc.model();
    assert_eq!(
        supertypes(model, "Inheritance.Derived"),
        [
            "generalization Inheritance.Base",
            "realization Inheritance.ILabeled",
            "realization System.ICloneable"
        ]
    );
    // `Shape` comes first but is an interface of the sources.
    assert_eq!(supertypes(model, "Inheritance.Circle"), ["realization Inheritance.Shape"]);
    // Unknown names: by the naming rule.
    assert_eq!(
        supertypes(model, "Inheritance.Customer"),
        ["generalization Entity", "realization IAudited"]
    );
    assert_eq!(kind(model, "Entity"), "class");
    assert_eq!(kind(model, "IAudited"), "interface");
    assert_eq!(supertypes(model, "Inheritance.Admin"), ["realization IUser"]);
    // Library types: by the table.
    assert_eq!(
        supertypes(model, "Inheritance.Orders"),
        [
            "generalization System.Collections.Generic.List",
            "realization System.Collections.Generic.IEnumerable"
        ]
    );
    // Interfaces generalize interfaces; structs realize them.
    assert_eq!(
        supertypes(model, "Inheritance.ILabeled"),
        ["generalization Inheritance.INamed", "generalization System.IComparable"]
    );
    assert_eq!(kind(model, "Comparable"), "interface");
    check("inheritance", model);
}

#[test]
fn generics_link_their_arguments() {
    let (doc, _) = fresh("generics");
    let model = doc.model();
    assert_eq!(attribute_type(model, "Generics.Box", "index"), "Dictionary<string,List<T>>");
    check("generics", model);
}

#[test]
fn arrays_nullables_and_parameter_directions() {
    let (doc, _) = fresh("arrays_nullable");
    let model = doc.model();
    assert_eq!(attribute_type(model, "Arrays.Grid", "matrix"), "int[,]");
    assert_eq!(attribute_type(model, "Arrays.Grid", "maybe"), "int?");
    let next = association_end(model, "Arrays.Grid", "next");
    assert_eq!(next.multiplicity, Some(Multiplicity::OPTIONAL));
    check("arrays_nullable", model);
}

#[test]
fn properties_become_attributes_or_accessors() {
    let (doc, _) = fresh("properties");
    let model = doc.model();
    at(model, "Properties.Account.Id");
    at(model, "Properties.Account.get_Balance");
    at(model, "Properties.Account.set_Balance");
    at(model, "Properties.Account.get_Item");
    check("properties", model);
}

#[test]
fn partial_types_are_one_type() {
    let (doc, report) = fresh("partial");
    let model = doc.model();
    // Counted once, not as created in one file and updated in the other.
    assert_eq!((report.created, report.updated), (1, 0));
    assert_eq!(
        supertypes(model, "Partial.Order"),
        ["realization System.IDisposable", "realization System.IComparable"]
    );
    let order = model.get(at(model, "Partial.Order")).unwrap();
    assert_eq!(order.data().documentation, "<summary>An order, in two files.</summary>");
    check("partial", model);
}

/// A new import of one file of a partial type leaves what the other parts declare: members,
/// supertypes, documentation.
#[test]
fn importing_one_part_of_a_partial_type_keeps_the_others() {
    let (mut doc, _) = fresh("partial");
    let before = doc.model().clone();
    for file in ["Order.cs", "Order.Lines.cs"] {
        let path = common::fixture(CSHARP, "partial").join(file);
        umlcraft_reverse::import_into(&mut doc, CSHARP, &[path], "Part").unwrap();
        assert_eq!(*doc.model(), before, "{file}");
    }
}

/// `partial void OnSaving();` and its implementation in another part are one operation, with
/// the documentation of either.
#[test]
fn a_partial_method_is_one_operation() {
    let files = [
        ("Order.cs", "partial class Order { partial void OnSaving(); }"),
        (
            "Order.Save.cs",
            "partial class Order {\n  /// <summary>Saves.</summary>\n  partial void OnSaving() { }\n}",
        ),
    ];
    let mut parsed = umlcraft_reverse::Parsed::default();
    for (name, text) in files {
        let path = std::path::Path::new(name);
        let unit = umlcraft_reverse::parse_text(CSHARP, path, text, &mut Default::default());
        parsed.units.push(unit.unwrap());
    }
    let mut doc = Document::new(Model::new("t"));
    for _ in 0..2 {
        doc.edit("Import", |tx| umlcraft_reverse::apply(tx, CSHARP, &parsed)).unwrap();
        let model = doc.model();
        let order = at(model, "Order");
        let operations: Vec<&umlcraft_model::Element> = model
            .children(order)
            .iter()
            .map(|&c| model.get(c).unwrap())
            .filter(|e| matches!(e.kind(), ElementKind::Operation(_)))
            .collect();
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].data().documentation, "<summary>Saves.</summary>");
    }
}

#[test]
fn names_resolve_through_usings_aliases_and_namespaces() {
    let (doc, _) = fresh("resolution");
    let model = doc.model();
    check("resolution", model);
    assert!(links_to(model, "Acme.App.Shop", "price", "Acme.Lib.Money"));
    assert!(links_to(model, "Acme.App.Shop", "formatter", "Acme.Lib.Text.Formatter"));
    assert!(links_to(model, "Acme.App.Shop", "aliased", "Acme.Lib.Text.Formatter"));
    assert!(links_to(model, "Acme.App.Shop", "address", "Acme.Lib.Customer.Address"));
    assert!(links_to(model, "Acme.App.Shop", "second", "Acme.Lib.Text.Formatter"));
    assert!(links_to(model, "Acme.App.Shop", "third", "Acme.Lib.Money"));
    assert!(links_to(model, "Acme.App.Shop", "prices", "Acme.Lib.Money"));
    assert!(links_to(model, "Acme.App.Shop", "opened", "System.DateTime"));
    assert!(links_to(model, "Acme.App.Shop", "missing", "Missing"));
    assert!(links_to(model, "Acme.App.Shop", "widget", "Other.Things.Widget"));
    assert!(links_to(model, "Acme.App.Inner.Clerk", "shop", "Acme.App.Shop"));
}

#[test]
fn attributes_of_class_types_become_associations() {
    let (doc, report) = fresh("associations");
    let model = doc.model();
    let end = |attribute: &str| association_end(model, "Shop.Order", attribute);
    check("associations", model);
    assert_eq!(end("customer").multiplicity, None);
    assert_eq!(end("payer").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("lines").multiplicity, Some(Multiplicity::MANY));
    assert_eq!(end("lines").constraints, ["ordered"]);
    assert_eq!(end("tags").constraints, ["unique"]);
    assert_eq!(end("sorted").constraints, ["ordered", "unique"]);
    assert_eq!(end("history").multiplicity, Some(Multiplicity::MANY));
    assert_eq!(end("frozen").multiplicity, Some(Multiplicity::MANY));
    assert_eq!(end("discount").multiplicity, Some(Multiplicity::OPTIONAL));
    assert_eq!(end("Owner").multiplicity, None);
    assert!(end("Items").is_read_only);
    // Values stay attributes.
    at(model, "Shop.Order.note");
    at(model, "Shop.Order.placed");
    at(model, "Shop.Order.count");
    assert_eq!(report.associations, 11);
}

#[test]
fn preprocessor_branches_after_the_first_are_left_out() {
    let (doc, report) = fresh("preprocessor");
    let model = doc.model();
    at(model, "Conditional.Diagnostics");
    at(model, "Conditional.Service.Modern");
    assert!(model.children(at(model, "Conditional")).len() == 2);
    assert_eq!(report.skipped.values().sum::<usize>(), 5);
}

#[test]
fn syntax_errors_skip_only_their_file() {
    let (doc, report) = fresh("syntax_error");
    at(doc.model(), "Broken.Fine");
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
        "modifiers",
        "docs",
        "inheritance",
        "generics",
        "arrays_nullable",
        "properties",
        "partial",
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
    let order = at(model, "Model.Order");
    let number = at(model, "Model.Order.number");
    let ship = at(model, "Model.Order.Ship");
    let kind_id = at(model, "Model.Kind");
    let diagram = doc
        .edit("Diagram", |tx| {
            let root = tx.model().root();
            let kind = ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            let d = tx.create(root, umlcraft_model::ElementData::new("d", kind))?;
            tx.add_node(d, Some(order), Rect::new(0.0, 0.0, 100.0, 60.0))?;
            tx.add_node(d, Some(kind_id), Rect::new(200.0, 0.0, 100.0, 60.0))?;
            Ok(d)
        })
        .unwrap();
    import(&mut doc, "reimport/v2");
    let model = doc.model();
    // Matched members keep their ids.
    assert_eq!(at(model, "Model.Order"), order);
    assert_eq!(at(model, "Model.Order.number"), number);
    assert_eq!(attribute_type(model, "Model.Order", "number"), "long");
    let operations: Vec<Id> = model
        .children(order)
        .iter()
        .copied()
        .filter(|&c| matches!(model.get(c).unwrap().kind(), ElementKind::Operation(_)))
        .collect();
    assert_eq!(operations.len(), 2);
    assert!(operations.contains(&ship));
    // `Kind` became an interface and stays on the diagram.
    assert_eq!(kind(model, "Model.Kind"), "interface");
    let shown: Vec<Id> =
        model.view(diagram).unwrap().presentations().iter().filter_map(|p| p.element).collect();
    assert!(shown.contains(&at(model, "Model.Kind")));
    check("reimport", model);
}

/// Aliases naming each other end in a placeholder instead of overflowing the stack.
#[test]
fn alias_cycles_end() {
    let (doc, _) = fresh("alias_cycle");
    at(doc.model(), "N.C");
}
