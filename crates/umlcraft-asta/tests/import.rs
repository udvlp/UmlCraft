//! Imports the sample projects that ship with Astah Professional, and a local reference project
//! under `docs/`. Each test is skipped when its file is missing.

use std::path::{Path, PathBuf};

use umlcraft_model::{
    ActivityNodeKind, DiagramKind, ElementKind, InstanceKind, MessageGate, Model, Point,
    PseudostateKind, Shape, TransitionProps, TypeRef,
};

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
fn sample_templates() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let model = umlcraft_asta::import(&path).unwrap().model;
    model.check().unwrap();
    let pair = find(&model, "Pair", |k| matches!(k, ElementKind::Class(_)));
    let parameters = umlcraft_notation::member::template_parameters(&model, pair.id());
    let names: Vec<&str> = parameters.iter().map(|&p| model.get(p).unwrap().name()).collect();
    assert_eq!(names, ["T1", "T2"]);
    // Members typed by a template parameter refer to it.
    let first = find(&model, "first", |k| matches!(k, ElementKind::Attribute(_)));
    let ElementKind::Attribute(a) = first.kind() else { unreachable!() };
    assert_eq!(a.type_ref, Some(TypeRef::Element(parameters[0])));
    // A bound template is an expression whose parts are linked.
    let a_pair = find(&model, "aPair", |k| matches!(k, ElementKind::Attribute(_)));
    let ElementKind::Attribute(a) = a_pair.kind() else { unreachable!() };
    let Some(TypeRef::Expr(e)) = &a.type_ref else { panic!("{:?}", a.type_ref) };
    assert_eq!(e.base, TypeRef::Element(pair.id()));
    assert_eq!(e.arguments.len(), 2);
    assert!(e.arguments.iter().all(|t| matches!(t, TypeRef::Element(_))), "{e:?}");
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
    // Ten between lifelines, a found and a lost message, and the destroy message behind the
    // cross on ob2, which Astah does not draw.
    assert_eq!(messages.len(), 12);
    let drawn = messages.iter().filter(|&&m| view.presentations_of(m).any(|p| p.is_edge()));
    assert_eq!(drawn.count(), 11);
    let fragment = interaction
        .children()
        .iter()
        .map(|&c| model.get(c).unwrap())
        .find(|e| matches!(e.kind(), ElementKind::CombinedFragment(_)))
        .unwrap();
    assert_eq!(fragment.children().len(), 2, "two operands");
    // Astah's geometry: the frame at the back, the alt fragment with one separator, a lifeline
    // ending in a cross, and bars on the lifelines.
    assert_eq!(view.presentations()[0].element, Some(example.id()));
    let alt = view.presentations_of(fragment.id()).next().unwrap();
    let Shape::Fragment { separators, .. } = &alt.shape else { panic!("{alt:?}") };
    assert_eq!(separators.len(), 1);
    assert!(
        view.presentations()
            .iter()
            .any(|p| matches!(p.shape, Shape::Lifeline { destroyed: true, .. }))
    );
    let bars = view.presentations().iter().filter(|p| matches!(p.shape, Shape::Activation { .. }));
    // Astah also keeps the two gates on the frame as bars; those are not drawn.
    assert_eq!(bars.count(), 6);

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

#[test]
fn sample_state_machine_diagram() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    let [diagram] = diagrams(model, DiagramKind::StateMachine)[..] else { panic!("one diagram") };
    let machine = model.get(diagram.owner().unwrap()).unwrap();
    assert!(matches!(machine.kind(), ElementKind::StateMachine));
    // The state machine describes the class "Statemachine".
    assert!(matches!(model.get(machine.owner().unwrap()).unwrap().kind(), ElementKind::Class(_)));

    let state = |name: &str| find(model, name, |k| matches!(k, ElementKind::State(_)));
    // "Black" and "White" are substates of "OnCourse", with its own initial pseudostate.
    let on_course = state("OnCourse");
    for name in ["Black", "White"] {
        assert_eq!(state(name).owner(), Some(on_course.id()), "{name}");
    }
    let initial = on_course
        .children()
        .iter()
        .map(|&c| model.get(c).unwrap())
        .find(|e| matches!(e.kind(), ElementKind::Pseudostate(p) if p.pseudostate == PseudostateKind::Initial))
        .expect("initial pseudostate in OnCourse");
    assert_eq!(initial.name(), "", "Astah's default name is dropped");
    let ElementKind::State(white) = state("White").kind() else { unreachable!() };
    assert_eq!(
        (white.entry.as_str(), white.do_activity.as_str(), white.exit.as_str()),
        ("start timer", "turn left", "clear timer")
    );

    let transitions: Vec<&TransitionProps> = model
        .subtree(machine.id())
        .iter()
        .filter_map(|&id| match model.get(id).unwrap().kind() {
            ElementKind::Transition(p) => Some(p),
            _ => None,
        })
        .collect();
    assert_eq!(transitions.len(), 9);
    assert!(
        transitions.iter().any(|t| t.label() == "changed lightness [lightness < 40]"),
        "{:?}",
        transitions.iter().map(|t| t.label()).collect::<Vec<_>>()
    );
    // Names that only repeat the label are dropped.
    for id in model.subtree(machine.id()) {
        let e = model.get(id).unwrap();
        if matches!(e.kind(), ElementKind::Transition(_)) {
            assert_eq!(e.name(), "");
        }
    }
    let view = model.view(diagram.id()).unwrap();
    assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 10, "and an anchor");
}

#[test]
fn sample_component_and_deployment_diagrams() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    for class in ["UComponentImp", "UArtifactImp", "UNodeImp", "UNodeInstanceImp"] {
        assert!(!imported.skipped.contains_key(class), "{class}: {:?}", imported.skipped);
    }

    let catalog = find(model, "Catalog", |k| matches!(k, ElementKind::Component(_)));
    assert_eq!(catalog.data().stereotypes, vec!["ejbEntity"]);
    find(model, "CatalogJAR", |k| matches!(k, ElementKind::Artifact(_)));
    let [components] = diagrams(model, DiagramKind::Component)[..] else { panic!("one") };
    let view = model.view(components.id()).unwrap();
    assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 6);

    let deployments = diagrams(model, DiagramKind::Deployment);
    assert_eq!(deployments.len(), 2);
    let server = find(model, "videoStoreServer", |k| matches!(k, ElementKind::Instance(_)));
    let ElementKind::Instance(p) = server.kind() else { unreachable!() };
    assert_eq!(p.instance, InstanceKind::Node);
    assert_eq!(model.get(p.classifier.unwrap()).unwrap().name(), "AppServer");
    // Instances depend on interfaces; a component providing one is a realization, not a usage.
    let realizations: Vec<_> = model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Realization(_)))
        .filter(|e| e.data().stereotypes.iter().any(|s| s == "use"))
        .collect();
    assert!(realizations.is_empty(), "realizations marked «use»");
    let from_instances = model
        .elements()
        .filter(|e| match e.kind() {
            ElementKind::Dependency(d) | ElementKind::Realization(d) => {
                matches!(model.get(d.client).unwrap().kind(), ElementKind::Instance(_))
            }
            _ => false,
        })
        .count();
    assert_eq!(from_instances, 13);
}

#[test]
fn sample_interaction_uses_and_gate_messages() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    for class in ["UInteractionUseImp", "UMessageImp"] {
        assert!(!imported.skipped.contains_key(class), "{class}: {:?}", imported.skipped);
    }

    let uses: Vec<_> =
        model.elements().filter(|e| matches!(e.kind(), ElementKind::InteractionUse(_))).collect();
    assert_eq!(uses.len(), 2);
    let establish = uses.iter().find(|e| e.name() == "EstablishAccess").unwrap();
    let ElementKind::InteractionUse(p) = establish.kind() else { unreachable!() };
    assert_eq!(p.arguments, "\"Illegal PIN\"");
    assert_eq!(p.covered.len(), 2);
    // It refers to the interaction of another sequence diagram.
    let referred = model.get(p.refers_to.expect("refers to an interaction")).unwrap();
    assert!(matches!(referred.kind(), ElementKind::Interaction));
    assert_ne!(Some(referred.id()), establish.owner());
    // "OpenDoor" sits in an operand of an opt fragment.
    let open_door = uses.iter().find(|e| e.name() == "OpenDoor").unwrap();
    let operand = model.get(open_door.owner().unwrap()).unwrap();
    assert!(matches!(operand.kind(), ElementKind::InteractionOperand(_)));

    let gates: Vec<MessageGate> = model
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Message(p) if e.name() == "opti" => Some(p.gate),
            _ => None,
        })
        .collect();
    assert_eq!(gates.len(), 2);
    assert!(gates.contains(&MessageGate::Found) && gates.contains(&MessageGate::Lost));
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

    // Bound templates become type expressions.
    let aenderungen = find(&model, "aenderungen", |k| matches!(k, ElementKind::Attribute(_)));
    let ElementKind::Attribute(a) = aenderungen.kind() else { unreachable!() };
    let label = umlcraft_notation::member::type_label(&model, &a.type_ref);
    assert_eq!(label.as_deref(), Some("Stack<Aufgabe>"));
    assert!(matches!(a.type_ref, Some(TypeRef::Expr(_))), "{:?}", a.type_ref);

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
