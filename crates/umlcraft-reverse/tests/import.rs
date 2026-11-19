//! Java imports into a fresh model, one per fixture: the model as text is compared with
//! `expected.model` (write it with `UPDATE_SNAPSHOTS=1` and review the diff), and the rules each
//! fixture is about are checked directly.

mod common;

use common::{at, attribute_type, links_to};
use umlcraft_model::{Document, ElementKind, Id, Model, Navigability, Rect, TypeRef};
use umlcraft_reverse::{Report, SourceLanguage, import_into};

const JAVA: SourceLanguage = SourceLanguage::Java;

fn import(doc: &mut Document, case: &str) -> Report {
    common::import(doc, JAVA, case)
}

fn fresh(case: &str) -> (Document, Report) {
    common::fresh(JAVA, case)
}

fn check(case: &str, model: &Model) {
    common::check_model(JAVA, case, model)
}

#[test]
fn packages_nest_and_the_default_package_is_the_root() {
    let (doc, _) = fresh("packages");
    let model = doc.model();
    at(model, "a.b.Inner");
    at(model, "a.Outer");
    at(model, "Root");
    check("packages", model);
}

#[test]
fn kinds_map_to_classes_interfaces_and_enumerations() {
    let (doc, _) = fresh("kinds");
    let model = doc.model();
    let kind = |path: &str| model.get(at(model, path)).unwrap().kind().type_name();
    assert_eq!(kind("kinds.Kinds.Shape"), "interface");
    assert_eq!(kind("kinds.Kinds.Color"), "enumeration");
    assert_eq!(kind("kinds.Kinds.Point"), "class");
    // Astah makes annotation types classes.
    assert_eq!(kind("kinds.Kinds.Marker"), "class");
    assert_eq!(kind("kinds.Kinds.Nested.Deeper"), "class");
    check("kinds", model);
}

#[test]
fn modifiers() {
    let (doc, _) = fresh("modifiers");
    check("modifiers", doc.model());
}

#[test]
fn javadoc_becomes_documentation() {
    let (doc, _) = fresh("javadoc");
    let model = doc.model();
    let documentation =
        |path: &str| model.get(at(model, path)).unwrap().data().documentation.clone();
    assert_eq!(documentation("javadoc.Documented.reset"), "This one.");
    assert_eq!(documentation("javadoc.Unit.METRE"), "Metres.");
    check("javadoc", model);
}

#[test]
fn annotations() {
    let (doc, _) = fresh("annotations");
    check("annotations", doc.model());
}

#[test]
fn initializers_become_default_values() {
    let (doc, report) = fresh("initializers");
    assert_eq!(report.skipped.values().sum::<usize>(), 6);
    check("initializers", doc.model());
}

#[test]
fn inheritance_follows_extends_and_implements() {
    let (doc, _) = fresh("inheritance");
    let model = doc.model();
    // Library types the sources implement are interfaces at their package path.
    let kind = |path: &str| model.get(at(model, path)).unwrap().kind().type_name();
    assert_eq!(kind("java.io.Serializable"), "interface");
    assert_eq!(kind("java.lang.Cloneable"), "interface");
    check("inheritance", model);
}

#[test]
fn generics_link_their_arguments() {
    let (doc, _) = fresh("generics");
    let model = doc.model();
    assert!(links_to(model, "generics.Generics", "index", "generics.Foo"));
    assert!(links_to(model, "generics.Generics", "index", "java.util.List"));
    assert_eq!(attribute_type(model, "generics.Generics", "upper"), "List<? extends T>");
    assert!(links_to(model, "generics.Generics", "entry", "java.util.Map.Entry"));
    check("generics", model);
}

#[test]
fn arrays_and_varargs_keep_their_modifiers() {
    let (doc, _) = fresh("arrays_varargs");
    let model = doc.model();
    assert_eq!(attribute_type(model, "arrays_varargs.Arrays", "b"), "int[]");
    assert_eq!(attribute_type(model, "arrays_varargs.Arrays", "grid"), "int[][]");
    check("arrays_varargs", model);
}

#[test]
fn names_resolve_in_astahs_order() {
    let (doc, _) = fresh("resolution");
    let model = doc.model();
    let c = "app.App";
    let linked = |attribute: &str, target: &str| links_to(model, c, attribute, target);
    assert!(linked("value", "app.App.T"));
    assert!(linked("inner", "app.App.Inner"));
    assert!(linked("shared", "app.Base.Shared"));
    assert!(linked("helper", "lib.Helper"));
    assert!(linked("gone", "lib.missing.Gone"));
    assert!(linked("base", "app.Base"));
    assert!(linked("date", "java.util.Date"));
    assert!(linked("name", "java.lang.String"));
    assert!(linked("file", "java.io.File"));
    assert!(linked("qualified", "lib.Helper"));
    assert!(linked("entry", "java.util.Map.Entry"));
    assert!(linked("thing", "com.acme.Thing"));
    // Unknown simple names go to the root, Astah's default package.
    assert!(linked("unknown", "Unknown"));
    // An unknown name after `implements` is an interface.
    assert_eq!(model.get(at(model, "Unseen")).unwrap().kind().type_name(), "interface");
    // The operation's own template parameter.
    let first = at(model, "app.App.first");
    let return_type = match model.get(first).unwrap().kind() {
        ElementKind::Operation(p) => p.return_type.clone().unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(return_type, TypeRef::Element(at(model, "app.App.first.E")));
    check("resolution", model);
}

#[test]
fn attributes_of_class_types_become_associations() {
    let (doc, report) = fresh("associations");
    let model = doc.model();
    assert_eq!(report.associations, 7);
    // Values, template parameters and `Optional` (which Astah does not unwrap) stay attributes.
    for kept in ["name", "count", "tags", "notes", "current"] {
        at(model, &format!("shop.Shop.{kept}"));
    }
    at(model, "shop.Box.content");
    at(model, "shop.Box.more");
    check("associations", model);
}

/// The documentation and tags of a field that becomes an association go to the association,
/// on every import.
#[test]
fn an_association_keeps_its_fields_documentation_and_tags() {
    let text = "class Order {\n  /** Who placed it. */\n  private transient Customer customer;\n}\nclass Customer { }\n";
    let mut parsed = umlcraft_reverse::Parsed::default();
    let path = std::path::Path::new("Order.java");
    parsed
        .units
        .push(umlcraft_reverse::parse_text(JAVA, path, text, &mut Default::default()).unwrap());
    let mut doc = Document::new(Model::new("t"));
    for _ in 0..2 {
        doc.edit("Import", |tx| umlcraft_reverse::apply(tx, JAVA, &parsed)).unwrap();
        let model = doc.model();
        let associations: Vec<&umlcraft_model::Element> =
            model.elements().filter(|e| matches!(e.kind(), ElementKind::Association(_))).collect();
        assert_eq!(associations.len(), 1);
        let data = associations[0].data();
        assert_eq!(data.documentation, "Who placed it.");
        let tags: Vec<(&str, &str)> =
            data.tagged_values.iter().map(|t| (t.key.as_str(), t.value.as_str())).collect();
        assert_eq!(tags, [("umlcraft.imported", "true"), ("java.transient", "true")]);
    }
}

#[test]
fn syntax_errors_skip_only_their_file() {
    let (doc, report) = fresh("syntax_error");
    at(doc.model(), "syntax_error.Fine");
    assert_eq!(report.failed.len(), 1);
    assert!(report.failed[0].path.ends_with("Broken.java"));
}

#[test]
fn encodings_are_detected() {
    let (doc, report) = fresh("encoding");
    let model = doc.model();
    assert_eq!(model.get(at(model, "Legacy")).unwrap().data().documentation, "Grüße aus München.");
    at(model, "Legacy.straße");
    assert_eq!(report.guessed_encodings.len(), 1);
}

#[test]
fn the_import_is_one_undo_step() {
    let mut doc = Document::new(Model::new("t"));
    let before = doc.model().clone();
    import(&mut doc, "associations");
    assert!(doc.undo());
    assert_eq!(*doc.model(), before);
}

#[test]
fn a_type_that_changes_kind_keeps_its_place() {
    let dir = std::env::temp_dir().join(format!("umlcraft-retype-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("Shape.java");
    let mut doc = Document::new(Model::new("t"));
    std::fs::write(&file, "package geo;\nclass Shape {\n    int sides;\n}\n").unwrap();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    let shape = at(doc.model(), "geo.Shape");
    let diagram = doc
        .edit("Diagram", |tx| {
            let root = tx.model().root();
            let kind = ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            let d = tx.create(root, umlcraft_model::ElementData::new("d", kind))?;
            tx.add_node(d, Some(shape), Rect::new(0.0, 0.0, 100.0, 60.0))?;
            Ok(d)
        })
        .unwrap();
    std::fs::write(&file, "package geo;\ninterface Shape {\n    int sides();\n}\n").unwrap();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
    let model = doc.model();
    let shape = at(model, "geo.Shape");
    assert_eq!(model.get(shape).unwrap().kind().type_name(), "interface");
    at(model, "geo.Shape.sides");
    let shown: Vec<Id> =
        model.view(diagram).unwrap().presentations().iter().filter_map(|p| p.element).collect();
    assert_eq!(shown, [shape]);
    model.check().unwrap();
}

/// Imports 1000 generated files, then again. Run with `--release --ignored` to time it.
#[test]
#[ignore]
fn large_imports_are_fast() {
    let dir = std::env::temp_dir().join(format!("umlcraft-large-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for p in 0..20 {
        let package = dir.join(format!("p{p}"));
        std::fs::create_dir_all(&package).unwrap();
        for c in 0..50 {
            let other = format!("p{}.C{}", (p + 1) % 20, (c + 7) % 50);
            let source = format!(
                "package p{p};\nimport java.util.*;\n/** Class {c}. */\npublic class C{c} extends Base{c} implements Comparable<C{c}> {{\n    private int a; private String b; private List<{other}> items; private {other} link;\n    private Map<String, {other}> index;\n    public int f(int x, String y) {{ return 0; }}\n    public <T> T g(List<T> t) {{ return null; }}\n    public int compareTo(C{c} o) {{ return 0; }}\n}}\nclass Base{c} {{ protected long id; }}\n"
            );
            std::fs::write(package.join(format!("C{c}.java")), source).unwrap();
        }
    }
    let mut doc = Document::new(Model::new("t"));
    let start = std::time::Instant::now();
    let files =
        umlcraft_reverse::collect_sources_for(SourceLanguage::Java, std::slice::from_ref(&dir))
            .unwrap();
    let parsed = umlcraft_reverse::parse_sources(SourceLanguage::Java, &files);
    let parsing = start.elapsed();
    doc.edit("Import", |tx| umlcraft_reverse::apply(tx, SourceLanguage::Java, &parsed)).unwrap();
    let first = start.elapsed();
    eprintln!("parsing {parsing:?}");
    let start = std::time::Instant::now();
    import_into(&mut doc, SourceLanguage::Java, std::slice::from_ref(&dir), "Import").unwrap();
    let second = start.elapsed();
    eprintln!(
        "1000 files: first import {first:?}, again {second:?}, {} elements",
        doc.model().len()
    );
    doc.model().check().unwrap();
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_new_import_matches_what_is_there() {
    let mut doc = Document::new(Model::new("t"));
    import(&mut doc, "reimport/v1");
    let model = doc.model();
    let account = at(model, "model.Account");
    let deposit = at(model, "model.Account.deposit");
    let balance = at(model, "model.Account.balance");
    // `Missing` is unknown in v1: a placeholder in the root.
    let missing = at(model, "Missing");
    // Show the account and the placeholder on a diagram.
    let diagram = doc
        .edit("Diagram", |tx| {
            let root = tx.model().root();
            let kind = ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            let d = tx.create(root, umlcraft_model::ElementData::new("d", kind))?;
            tx.add_node(d, Some(account), Rect::new(0.0, 0.0, 100.0, 60.0))?;
            tx.add_node(d, Some(missing), Rect::new(200.0, 0.0, 100.0, 60.0))?;
            Ok(d)
        })
        .unwrap();
    import(&mut doc, "reimport/v2");
    let model = doc.model();
    // Matched members keep their ids.
    assert_eq!(at(model, "model.Account"), account);
    assert_eq!(at(model, "model.Account.balance"), balance);
    assert_eq!(at(model, "model.Account.deposit"), deposit);
    assert_eq!(attribute_type(model, "model.Account", "balance"), "long");
    // v2 declares `model.Missing`; the root placeholder stays, as in Astah, and on the diagram.
    assert_eq!(model.get(at(model, "model.Missing")).unwrap().kind().type_name(), "interface");
    assert_eq!(at(model, "Missing"), missing);
    let shown: Vec<Id> =
        model.view(diagram).unwrap().presentations().iter().filter_map(|p| p.element).collect();
    assert!(shown.contains(&missing));
    // New members take their place in source order: attributes before operations.
    let members: Vec<&str> = model
        .children(account)
        .iter()
        .filter_map(|&c| model.get(c))
        .filter(|e| matches!(e.kind(), ElementKind::Attribute(_) | ElementKind::Operation(_)))
        .map(|e| e.name())
        .collect();
    assert_eq!(members, ["balance", "note", "deposit", "deposit"]);
    // The association for the field that is gone is gone, the other one stays.
    let associations: Vec<String> = model
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Association(p) => Some(p.ends[1].role.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(associations, ["owner"]);
    let navigable = model.elements().any(|e| {
        matches!(e.kind(), ElementKind::Association(p) if p.ends[1].navigability == Navigability::Navigable)
    });
    assert!(navigable);
    check("reimport", model);
}

/// No Java front-end sets `is_query` (C++'s `f() const` will); the units are changed by hand.
#[test]
fn queries_are_part_of_the_signature() {
    use umlcraft_reverse::ir::{Item, Member};
    use umlcraft_reverse::{Parsed, apply, parse_text};

    // `f()` twice, the second one a query when `query` says so.
    let parsed = |query: [bool; 2]| {
        let mut skipped = Default::default();
        let path = std::path::Path::new("A.java");
        let mut unit =
            parse_text(JAVA, path, "class A { void f() {} void f() {} }", &mut skipped).unwrap();
        let Some(Item::Type(t)) = unit.scope.items.first_mut() else { panic!("no type") };
        let ops = t.members.iter_mut().filter_map(|m| match m {
            Member::Operation(o) => Some(o),
            _ => None,
        });
        for (o, q) in ops.zip(query) {
            o.is_query = q;
        }
        Parsed { units: vec![unit], ..Default::default() }
    };
    let operations = |doc: &Document| -> Vec<(Id, bool)> {
        let model = doc.model();
        model
            .children(at(model, "A"))
            .iter()
            .filter_map(|&c| match model.get(c)?.kind() {
                ElementKind::Operation(p) => Some((c, p.is_query)),
                _ => None,
            })
            .collect()
    };
    let mut doc = Document::new(Model::new("t"));
    doc.edit("import", |tx| apply(tx, JAVA, &parsed([false, true]))).unwrap();
    let first = operations(&doc);
    assert_eq!(first.iter().map(|o| o.1).collect::<Vec<_>>(), [false, true]);

    // The same again matches each operation with its own element.
    doc.edit("import", |tx| apply(tx, JAVA, &parsed([false, true]))).unwrap();
    assert_eq!(operations(&doc), first);

    // Both queries now: the query keeps its element, and the other takes the one left over
    // instead of a new one.
    doc.edit("import", |tx| apply(tx, JAVA, &parsed([true, true]))).unwrap();
    let ids: Vec<Id> = operations(&doc).iter().map(|o| o.0).collect();
    assert_eq!(ids, [first[1].0, first[0].0]);
    assert!(operations(&doc).iter().all(|o| o.1));
}

/// No Java front-end makes definitions outside a type (C++'s `void A::f() {}` will); the units
/// are made by hand from Java declarations.
#[test]
fn definitions_outside_a_type_join_its_members() {
    use umlcraft_reverse::ir::{Item, Member, Scope};
    use umlcraft_reverse::{Parsed, apply, parse_text};

    let parse = |text: &str| {
        let mut skipped = Default::default();
        parse_text(JAVA, std::path::Path::new("A.java"), text, &mut skipped).unwrap()
    };
    // The type in `package p;`.
    fn type_of(scope: &mut Scope) -> &mut umlcraft_reverse::ir::TypeDecl {
        match scope.items.first_mut() {
            Some(Item::Namespace { scope, .. }) => type_of(scope),
            Some(Item::Type(t)) => t,
            _ => panic!("no type"),
        }
    }
    // The declaration, without parameter names, documentation or default.
    let mut header = parse("package p; class A { int count; void f(int x); }");
    for m in &mut type_of(&mut header.scope).members {
        if let Member::Operation(o) = m {
            o.parameters[0].name.clear();
        }
    }
    // The definitions: `A`'s members, moved out of it, and one of a type nobody declares.
    let mut source = parse(
        "package p; class A { /** Counts. */ int count = 0; /** Does f. */ void f(int y) {} \
         void g() {} }",
    );
    let defined = type_of(&mut source.scope).clone();
    let mut items: Vec<Item> = defined
        .members
        .iter()
        .map(|m| Item::Definition { owner: vec!["A".into()], member: m.clone() })
        .collect();
    items.push(Item::Definition { owner: vec!["Gone".into()], member: defined.members[2].clone() });
    source.scope.items = vec![Item::Namespace {
        path: vec!["p".into()],
        scope: Scope { imports: Vec::new(), items },
    }];
    let parsed = Parsed { units: vec![header, source], ..Default::default() };

    let mut doc = Document::new(Model::new("t"));
    let report = doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    assert_eq!(report.skipped.get(&umlcraft_reverse::lang::Skip::UnknownOwner), Some(&1));
    let model = doc.model();
    // `f` and `count` filled in, `g` added after the declared members.
    pretty_assertions::assert_eq!(
        common::dump(model),
        "package p\n  class A Package\n    attribute ~ count : int = 0\n      doc \"Counts.\"\n    \
         operation ~ f(y : int) : void\n      doc \"Does f.\"\n    operation ~ g() : void\n"
    );
    let after = model.clone();

    // A second import changes nothing.
    doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    assert_eq!(*doc.model(), after);
}

/// No Java front-end makes aliases (C++'s `typedef` and `using X = …` will); they are added to
/// Java declarations by hand.
#[test]
fn aliases_stand_for_their_targets() {
    use umlcraft_reverse::ir::{Item, Member, TypeExpr};
    use umlcraft_reverse::{Parsed, apply, parse_text};

    let named = |path: &[&str]| TypeExpr::named(path.iter().map(|s| s.to_string()).collect());
    let text = "package p; import java.util.List; class Order {} \
                class Shop { Orders orders; Best best; Ptr[] ptrs; Node node; Loop loop; }";
    let mut skipped = Default::default();
    let mut unit = parse_text(JAVA, std::path::Path::new("A.java"), text, &mut skipped).unwrap();
    let Some(Item::Namespace { scope, .. }) = unit.scope.items.first_mut() else { panic!() };
    // In the namespace: a container, a self-alias, and a cycle.
    let alias = |name: &str, target: TypeExpr| Item::Alias { name: name.into(), target };
    let mut orders = named(&["List"]);
    orders.arguments.push(named(&["Order"]));
    let mut best = named(&["Ptr"]);
    best.prefix = "final ".into();
    scope.items.extend([
        alias("Orders", orders),
        alias("Best", best),
        alias("Node", named(&["Node"])),
        alias("Loop", named(&["Loop2"])),
        alias("Loop2", named(&["Loop"])),
    ]);
    // In the type: `Ptr` is an `Order`, written with a modifier.
    let Some(Item::Type(shop)) = scope.items.get_mut(1) else { panic!() };
    let mut ptr = named(&["Order"]);
    ptr.modifier = "*".into();
    shop.members.push(Member::Alias { name: "Ptr".into(), target: ptr });
    let parsed = Parsed { units: vec![unit], ..Default::default() };

    let mut doc = Document::new(Model::new("t"));
    doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    let model = doc.model();
    // The container's element becomes the association end.
    assert!(links_to(model, "p.Shop", "orders", "p.Order"));
    // `Best` names `Ptr`, which resolves where it is used, in `Shop`; prefixes and modifiers
    // add up.
    assert_eq!(attribute_type(model, "p.Shop", "best"), "final Order*");
    assert_eq!(attribute_type(model, "p.Shop", "ptrs"), "Order*[]");
    // A self-alias and a cycle stand for themselves: unknown names, placeholders in the root.
    assert!(links_to(model, "p.Shop", "node", "Node"));
    assert!(links_to(model, "p.Shop", "loop", "Loop"));
    let after = model.clone();
    doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    assert_eq!(*doc.model(), after);
}

/// No Java front-end leaves types out (Python without annotations will); they are removed from
/// Java declarations by hand.
#[test]
fn unwritten_types_give_no_type_and_no_association() {
    use umlcraft_reverse::ir::{Item, Member, TypeExpr};
    use umlcraft_reverse::{Parsed, apply, parse_text};

    let text = "class Order {} class Shop { Order order; Order take(Order o) { return o; } }";
    let mut skipped = Default::default();
    let mut unit = parse_text(JAVA, std::path::Path::new("A.java"), text, &mut skipped).unwrap();
    let Some(Item::Type(shop)) = unit.scope.items.get_mut(1) else { panic!("no Shop") };
    for m in &mut shop.members {
        match m {
            Member::Field(f) => f.ty = TypeExpr::unwritten(),
            Member::Operation(o) => {
                o.return_type = Some(TypeExpr::unwritten());
                o.parameters[0].ty = TypeExpr::unwritten();
            }
            _ => {}
        }
    }
    let parsed = Parsed { units: vec![unit], ..Default::default() };
    let mut doc = Document::new(Model::new("t"));
    let report = doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    assert_eq!(report.associations, 0);
    let model = doc.model();
    pretty_assertions::assert_eq!(
        common::dump(model),
        "class Order Package\nclass Shop Package\n  attribute ~ order\n  operation ~ take(o)\n"
    );
    let after = model.clone();
    doc.edit("import", |tx| apply(tx, JAVA, &parsed)).unwrap();
    assert_eq!(*doc.model(), after);
}

/// `class Foo` in one file and `interface Foo` in another make one type, of the kind declared
/// first, instead of aborting the import.
#[test]
fn one_name_declared_with_two_kinds_imports() {
    let (doc, _) = fresh("two_kinds");
    let model = doc.model();
    let foo = at(model, "p.Foo");
    assert_eq!(model.get(foo).unwrap().kind().type_name(), "class");
    // A second import keeps the result.
    let mut doc = doc;
    import(&mut doc, "two_kinds");
    assert_eq!(at(doc.model(), "p.Foo"), foo);
}

/// Generated code nests expressions thousands deep; reading it needs no deep stack.
#[test]
fn deeply_nested_method_bodies_parse_on_a_small_stack() {
    let terms: Vec<String> = (0..10_000).map(|i| format!("\"s{i}\"")).collect();
    let text = format!("class Table {{ String all() {{ return {}; }} }}", terms.join(" + "));
    let unit = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let path = std::path::Path::new("Table.java");
            umlcraft_reverse::parse_text(JAVA, path, &text, &mut Default::default()).is_ok()
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(unit);
}

/// An association drawn by hand between imported classes survives a new import, even with a
/// navigable end that has a role, as one made from a field would have.
#[test]
fn hand_drawn_associations_survive_a_new_import() {
    let (mut doc, _) = fresh("associations");
    let (customer, order) = (at(doc.model(), "shop.Customer"), at(doc.model(), "shop.Order"));
    let drawn = doc
        .edit("draw", |tx| {
            let mut kind = ElementKind::association(customer, order);
            if let ElementKind::Association(p) = &mut kind {
                p.ends[1].role = "lastOrder".into();
                p.ends[1].navigability = Navigability::Navigable;
            }
            tx.create(customer, umlcraft_model::ElementData::new("", kind))
        })
        .unwrap();
    import(&mut doc, "associations");
    let ElementKind::Association(p) = doc.model().get(drawn).unwrap().kind() else { panic!() };
    assert_eq!(p.ends[1].navigability, Navigability::Navigable);
}
