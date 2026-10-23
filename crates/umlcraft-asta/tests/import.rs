//! Imports the sample projects that ship with Astah Professional. Skipped when Astah is not
//! installed.

use std::path::{Path, PathBuf};

use umlcraft_model::{DiagramKind, ElementKind, Model, TypeRef};

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
    assert_eq!(sequences.len(), 3);
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
    let fragment = interaction
        .children()
        .iter()
        .map(|&c| model.get(c).unwrap())
        .find(|e| matches!(e.kind(), ElementKind::CombinedFragment(_)))
        .unwrap();
    assert_eq!(fragment.children().len(), 2, "two operands");

    // The project survives a round trip through the UmlCraft format.
    let json = umlcraft_format::to_string(model);
    assert_eq!(umlcraft_format::from_str(&json).unwrap(), *model);
}
