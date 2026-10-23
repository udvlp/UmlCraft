//! Imports the sample projects that ship with Astah Professional, and a local reference project
//! under `docs/`. Each test is skipped when its file is missing.

use std::path::{Path, PathBuf};

use umlcraft_model::{ActivityNodeKind, DiagramKind, ElementKind, Model, Point, Shape, TypeRef};

const ASTAH: &str = r"C:\Program Files\astah-professional";

fn astah_file(relative: &str) -> Option<PathBuf> {
    let path = Path::new(ASTAH).join(relative);
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipped: {} not found", path.display());
        None
    }
}

fn find<'m>(
    model: &'m Model,
    name: &str,
    pred: impl Fn(&ElementKind) -> bool,
) -> &'m umlcraft_model::Element {
    model
        .elements()
        .find(|e| e.name() == name && pred(e.kind()))
        .unwrap_or_else(|| panic!("no element {name:?}"))
}

fn diagrams(model: &Model, kind: DiagramKind) -> Vec<&umlcraft_model::Element> {
    model.elements().filter(|e| e.kind().as_diagram() == Some(kind)).collect()
}

#[test]
fn sample_model() {
    let Some(path) = astah_file(r"api\sample\simpleDgmRead\SampleModel.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    model.check().unwrap();
    assert_eq!(model.element(model.root()).unwrap().name(), "SampleModel");
    let class0 = find(model, "Class0", |k| matches!(k, ElementKind::Class(_)));
    let class1 = find(model, "Class1", |k| matches!(k, ElementKind::Class(_)));
    let association = find(model, "association", |k| matches!(k, ElementKind::Association(_)));
    let ElementKind::Association(p) = association.kind() else { unreachable!() };
    let participants = [p.ends[0].participant, p.ends[1].participant];
    assert!(participants.contains(&class0.id()) && participants.contains(&class1.id()));
    find(model, "generalization", |k| matches!(k, ElementKind::Generalization(_)));
    find(model, "dependency", |k| matches!(k, ElementKind::Dependency(_)));

    let [diagram] = diagrams(model, DiagramKind::Class)[..] else { panic!("one class diagram") };
    let view = model.view(diagram.id()).unwrap();
    let shown = |id| view.presentations_of(id).count();
    // Class0 is drawn twice on this diagram.
    assert_eq!(shown(class0.id()), 2);
    assert_eq!(shown(association.id()), 1);
    for p in view.presentations() {
        if let Some(b) = p.bounds() {
            assert!(b.x >= 0.0 && b.y >= 0.0 && b.width > 0.0 && b.height > 0.0, "{b:?}");
        }
    }
    // The matrix and the mind map have no counterpart.
    assert!(imported.skipped.contains_key("UMatrixDiagramImp"));
}

#[test]
fn sample_project() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    model.check().unwrap();
    eprintln!("skipped: {:#?}", imported.skipped);

    // Class diagram: classes with features and typed attributes.
    let monitor = find(model, "Monitor", |k| matches!(k, ElementKind::Class(_)));
    let threshold = find(model, "Threshold", |k| matches!(k, ElementKind::Attribute(_)));
    assert_eq!(model.get(threshold.id()).unwrap().owner(), Some(monitor.id()));
    let ElementKind::Attribute(a) = threshold.kind() else { unreachable!() };
    assert_eq!(a.type_ref, Some(TypeRef::Text("boolean".into())));
    let msg = find(model, "Msg", |k| matches!(k, ElementKind::Operation(_)));
    let params: Vec<&str> = msg.children().iter().map(|&c| model.get(c).unwrap().name()).collect();
    assert_eq!(params, ["message"]);

    let [class_diagram, ..] = diagrams(model, DiagramKind::Class)[..] else { panic!() };
    let view = model.view(class_diagram.id()).unwrap();
    assert!(view.presentations().iter().filter(|p| p.is_edge()).count() >= 9);
    assert!(view.presentations().iter().filter(|p| !p.is_edge()).count() >= 9);

    // Use cases: actor, include in UML direction, extend with its extension point.
    find(model, "Salesperson", |k| matches!(k, ElementKind::Actor(_)));
    let place_order = find(model, "Place Order", |k| matches!(k, ElementKind::UseCase(_)));
    let supply = find(model, "Supply Customer Data", |k| matches!(k, ElementKind::UseCase(_)));
    let includes = model.elements().filter_map(|e| match e.kind() {
        ElementKind::Include(p) => Some(*p),
        _ => None,
    });
    assert!(
        includes.into_iter().any(|p| p.including == place_order.id() && p.addition == supply.id())
    );
    let extend = model
        .elements()
        .find_map(|e| match e.kind() {
            ElementKind::Extend(p) => Some(p.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(extend.extended, place_order.id());
    assert_eq!(extend.extension_points.len(), 1);
    let [use_case_diagram] = diagrams(model, DiagramKind::UseCase)[..] else { panic!() };
    let view = model.view(use_case_diagram.id()).unwrap();
    assert_eq!(view.presentations_of(place_order.id()).count(), 1);
    let note = model.elements().find(|e| matches!(e.kind(), ElementKind::Comment(p) if p.body.contains("Use Case Relationships"))).unwrap();
    assert_eq!(view.presentations_of(note.id()).count(), 1);

    // Sequence diagrams: lifelines in an interaction, messages in order.
    let sequences = diagrams(model, DiagramKind::Sequence);
    assert_eq!(sequences.len(), 4);
    // This one belongs to the operation DigitalTimer.update, so its interaction goes to the class.
    let timer = sequences.iter().find(|d| d.name() == "Sequence Diagram").unwrap();
    let interaction = model.get(timer.owner().unwrap()).unwrap();
    assert_eq!(model.get(interaction.owner().unwrap()).unwrap().name(), "DigitalTimer");
    for diagram in &sequences {
        let interaction = model.get(diagram.owner().unwrap()).unwrap();
        assert!(matches!(interaction.kind(), ElementKind::Interaction));
        let lifelines = interaction
            .children()
            .iter()
            .filter(|&&c| matches!(model.get(c).unwrap().kind(), ElementKind::Lifeline(_)))
            .count();
        assert!(lifelines >= 2, "{}", diagram.name());
        let shown = model.view(diagram.id()).unwrap().presentations().iter().filter(|p| {
            p.element
                .is_some_and(|e| matches!(model.get(e).unwrap().kind(), ElementKind::Lifeline(_)))
        });
        assert_eq!(shown.count(), lifelines, "{}", diagram.name());
    }
    let example = sequences.iter().find(|d| d.name() == "example").unwrap();
    let interaction = model.get(example.owner().unwrap()).unwrap();
    let view = model.view(example.id()).unwrap();
    let messages = model
        .subtree(interaction.id())
        .into_iter()
        .filter(|&id| matches!(model.get(id).unwrap().kind(), ElementKind::Message(_)))
        .collect::<Vec<_>>();
    assert_eq!(messages.len(), 10);
    for &message in &messages {
        assert_eq!(view.presentations_of(message).filter(|p| p.is_edge()).count(), 1);
    }
    let fragment = interaction
        .children()
        .iter()
        .map(|&c| model.get(c).unwrap())
        .find(|e| matches!(e.kind(), ElementKind::CombinedFragment(_)))
        .unwrap();
    assert_eq!(fragment.children().len(), 2, "two operands");
    // Fragments lie behind the lifelines.
    assert_eq!(view.presentations()[0].element, Some(fragment.id()));

    // The project survives a round trip through the UmlCraft format.
    let json = umlcraft_format::to_string(model);
    assert_eq!(umlcraft_format::from_str(&json).unwrap(), *model);
}

#[test]
fn sample_activity_diagram() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    // Data flow diagrams and flowcharts are activity diagrams to Astah, but other notations.
    assert_eq!(imported.skipped.get("DataFlowDiagram"), Some(&4));
    assert_eq!(imported.skipped.get("Flowchart"), Some(&1));

    let [diagram] = diagrams(model, DiagramKind::Activity)[..] else { panic!("one activity") };
    let activity = model.get(diagram.owner().unwrap()).unwrap();
    assert!(matches!(activity.kind(), ElementKind::Activity));
    // The activity belongs to the use case it describes.
    let sales = model.get(activity.owner().unwrap()).unwrap();
    assert!(matches!(sales.kind(), ElementKind::UseCase(_)), "{}", sales.name());

    let count = |pred: &dyn Fn(&ElementKind) -> bool| {
        model
            .subtree(activity.id())
            .iter()
            .filter(|&&id| pred(model.get(id).unwrap().kind()))
            .count()
    };
    let node = |kind: ActivityNodeKind| move |k: &ElementKind| matches!(k, ElementKind::ActivityNode(p) if p.node == kind);
    assert_eq!(count(&node(ActivityNodeKind::Action)), 6);
    assert_eq!(count(&node(ActivityNodeKind::Object)), 4);
    assert_eq!(count(&node(ActivityNodeKind::Initial)), 1);
    assert_eq!(count(&node(ActivityNodeKind::Fork)), 1);
    assert_eq!(count(&node(ActivityNodeKind::Join)), 1);
    assert_eq!(count(&node(ActivityNodeKind::ActivityFinal)), 1);
    assert_eq!(count(&|k| matches!(k, ElementKind::ActivityEdge(_))), 14);
    // Astah's dimension "Dimension0" around the lanes is left out; the lanes belong to the
    // activity.
    let lanes: Vec<&str> = activity
        .children()
        .iter()
        .map(|&id| model.get(id).unwrap())
        .filter(|e| matches!(e.kind(), ElementKind::Partition))
        .map(|e| e.name())
        .collect();
    assert_eq!(lanes, vec!["Sales", "Stockroom ", "Customer"]);
    // Control nodes lose the names Astah gave them.
    let fork = model.subtree(activity.id()).into_iter().map(|id| model.get(id).unwrap()).find(
        |e| matches!(e.kind(), ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::Fork),
    );
    assert_eq!(fork.unwrap().name(), "");

    // Everything is shown: 14 activity nodes, 3 lanes, a note and 14 flows.
    let view = model.view(diagram.id()).unwrap();
    assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 14);
    assert_eq!(view.presentations().iter().filter(|p| !p.is_edge()).count(), 18);
}

/// Ulf's reference class diagram (`.asta` files are not in Git); its layout must match Astah's.
#[test]
fn reference_class_diagram() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/Klassendiagramm.asta");
    if !path.exists() {
        eprintln!("skipped: {} not found", path.display());
        return;
    }
    let imported = umlcraft_asta::import(&path).unwrap();
    assert!(imported.skipped.is_empty(), "{:?}", imported.skipped);
    let model = imported.model;
    model.check().unwrap();

    // {ordered} on the end "aufgaben" of Terminplan – Aufgabe.
    let ordered = model.elements().find_map(|e| match e.kind() {
        ElementKind::Association(p) => p.ends.iter().find(|end| !end.constraints.is_empty()),
        _ => None,
    });
    let ordered = ordered.expect("an end with a constraint");
    assert_eq!(
        (ordered.role.as_str(), &ordered.constraints[..]),
        ("aufgaben", &["ordered".to_owned()][..])
    );

    // Astah's language markers are not stereotypes to show.
    assert!(model.elements().all(|e| e.data().stereotypes.iter().all(|s| !s.starts_with("Java "))));

    // Members in the order Astah displays them.
    let projekt = find(&model, "Projekt", |k| matches!(k, ElementKind::Class(_)));
    let names = |pred: fn(&ElementKind) -> bool| -> Vec<&str> {
        projekt
            .children()
            .iter()
            .map(|&c| model.get(c).unwrap())
            .filter(|e| pred(e.kind()))
            .map(|e| e.name())
            .collect()
    };
    assert_eq!(
        names(|k| matches!(k, ElementKind::Attribute(_)))[..5],
        [
            "projektnr",
            "beschreibung",
            "geplanterStundenumfang",
            "aktuellerStundenumfang",
            "abgeschlossen"
        ]
    );
    assert_eq!(
        names(|k| matches!(k, ElementKind::Operation(_))),
        ["Projekt", "einfuegenAufgabe", "loescheAufgabe", "sucheAufgabe", "ersetzeAufgabe"]
    );

    // Bound templates become their textual name.
    let aenderungen = find(&model, "aenderungen", |k| matches!(k, ElementKind::Attribute(_)));
    let ElementKind::Attribute(a) = aenderungen.kind() else { unreachable!() };
    assert_eq!(a.type_ref, Some(TypeRef::Text("Stack<Aufgabe>".into())));

    // The edge to "- projekte" aims where Astah's line does, and the role sits where Astah put it.
    let association = model
        .elements()
        .find(|e| matches!(e.kind(), ElementKind::Association(p) if p.ends[1].role == "projekte"))
        .unwrap();
    let [diagram] = diagrams(&model, DiagramKind::Class)[..] else { panic!("one class diagram") };
    let edge = model.view(diagram.id()).unwrap().presentations_of(association.id()).next().unwrap();
    let Shape::Edge { source_anchor, target_anchor, labels, .. } = &edge.shape else { panic!() };
    let close = |p: Option<Point>, x: f32, y: f32| {
        p.is_some_and(|p| (p.x - x).abs() < 0.01 && (p.y - y).abs() < 0.01)
    };
    assert!(close(*source_anchor, 0.7604, 0.2432), "{source_anchor:?}");
    assert!(close(*target_anchor, 0.4867, 0.2453), "{target_anchor:?}");
    assert!(close(labels.target_role, 9.79, -23.35), "{:?}", labels.target_role);
}
