//! Imports the sample projects that ship with Astah Professional, and a local reference project
//! under `docs/`. Each test is skipped when its file is missing.

use std::path::{Path, PathBuf};

use umlcraft_model::{
    ActivityNodeKind, DiagramKind, ElementKind, InstanceKind, MessageGate, Model, Point,
    PseudostateKind, Shape, TransitionProps, TypeRef,
};

/// Astah's install folder: `ASTAH_HOME`, else where Astah Professional installs on Windows.
fn astah_home() -> PathBuf {
    std::env::var_os("ASTAH_HOME")
        .map_or_else(|| PathBuf::from(r"C:\Program Files\astah-professional"), PathBuf::from)
}

/// Reports a test that cannot run here. With `UMLCRAFT_REQUIRE_ASTAH` set, as on a machine that
/// has Astah, a skip fails instead, so a missing installation cannot pass for a green run.
fn skip(reason: &str) {
    assert!(std::env::var_os("UMLCRAFT_REQUIRE_ASTAH").is_none(), "not skipped: {reason}");
    eprintln!("skipped: {reason}");
}

fn astah_file(relative: &str) -> Option<PathBuf> {
    let path = astah_home().join(relative);
    if path.exists() {
        Some(path)
    } else {
        skip(&format!("{} not found", path.display()));
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
    // Ten between lifelines and a found and a lost message. The destroy message from ob2 to
    // itself that Astah draws as a cross only is a termination.
    assert_eq!(messages.len(), 11);
    let drawn = messages.iter().filter(|&&m| view.presentations_of(m).any(|p| p.is_edge()));
    assert_eq!(drawn.count(), 11);
    let termination = model
        .subtree(interaction.id())
        .into_iter()
        .find_map(|id| match model.get(id).unwrap().kind() {
            ElementKind::Termination(p) => Some((id, p.lifeline)),
            _ => None,
        })
        .expect("a termination");
    assert_eq!(model.get(termination.1).unwrap().name(), "ob2");
    let cross = view.presentations_of(termination.0).next().and_then(|p| p.bounds()).unwrap();
    assert_eq!((cross.width, cross.height), (20.0, 20.0));
    let fragment = interaction
        .children()
        .iter()
        .map(|&c| model.get(c).unwrap())
        .find(|e| matches!(e.kind(), ElementKind::CombinedFragment(_)))
        .unwrap();
    assert_eq!(fragment.children().len(), 2, "two operands");
    // Astah's geometry: the frame at the back, the alt fragment with one separator, the
    // lifeline of the termination ending at its cross, and bars on the lifelines.
    assert_eq!(view.presentations()[0].element, Some(example.id()));
    let alt = view.presentations_of(fragment.id()).next().unwrap();
    let Shape::Fragment { separators, .. } = &alt.shape else { panic!("{alt:?}") };
    assert_eq!(separators.len(), 1);
    let lane = view.presentations_of(termination.1).next().unwrap();
    let Shape::Lifeline { head, length, destroyed } = lane.shape else { panic!("{lane:?}") };
    assert!(!destroyed);
    assert!((head.bottom() + length - cross.center().y).abs() < 1.0, "{cross:?}");
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

    // Everything is shown: 14 activity nodes, 3 lanes, a note, the frame and 14 flows.
    let view = model.view(diagram.id()).unwrap();
    assert_eq!(view.presentations().iter().filter(|p| p.is_edge()).count(), 14);
    assert_eq!(view.presentations().iter().filter(|p| !p.is_edge()).count(), 19);
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

/// «include» and «extend» stay where Astah drew them. The import moves the whole diagram, so
/// the labels are compared with the first one: in Astah their tops are at (121.54, 122.16),
/// (247.25, 122.16), (366.03, 122.16) and, for the extend, (252.80, 306.91).
#[test]
fn sample_keywords_where_astah_draws_them() {
    use umlcraft_notation::scene::{self, ApproxMeasure, Preview};
    let Some(path) = astah_file("Sample.asta") else { return };
    let model = umlcraft_asta::import(&path).unwrap().model;
    let [diagram, ..] = diagrams(&model, DiagramKind::UseCase)[..] else { panic!() };
    let built = scene::build(&model, diagram.id(), &Preview::default(), &ApproxMeasure);
    let mut tops: Vec<(String, Point)> = built
        .edges
        .iter()
        .flat_map(|e| e.labels.iter().filter(|l| l.keyword))
        .map(|l| (l.text.clone(), l.anchor))
        .collect();
    // By row, then from left to right; a row's labels may differ by a fraction in height.
    tops.sort_by(|a, b| (a.1.y.round(), a.1.x).partial_cmp(&(b.1.y.round(), b.1.x)).unwrap());
    let first = tops[0].1;
    let relative: Vec<(&str, Point)> = tops
        .iter()
        .map(|(text, at)| (text.as_str(), Point::new(at.x - first.x, at.y - first.y)))
        .collect();
    let expected = [
        ("<<include>>", Point::new(0.0, 0.0)),
        ("<<include>>", Point::new(125.71, 0.0)),
        ("<<include>>", Point::new(244.49, 0.0)),
        ("<<extend>>", Point::new(131.25, 184.75)),
    ];
    assert_eq!(relative.len(), expected.len(), "{relative:?}");
    for ((text, at), (want_text, want)) in relative.iter().zip(expected) {
        assert_eq!(*text, want_text);
        // ApproxMeasure takes names wider than Arial, which widens some use cases a little.
        assert!(at.distance(want) < 0.5, "{text} at {at:?}, not {want:?}");
    }
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

#[test]
fn sample_bound_classes() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    use umlcraft_notation::member::{binding_text, display_name};
    // Bound classes a diagram shows are classes of their own, named by what they bind.
    let mut bound: Vec<String> = model
        .elements()
        .filter(|e| matches!(e.kind(), ElementKind::Class(p) if p.binding.is_some()))
        .map(|e| display_name(model, e.id()))
        .collect();
    bound.sort();
    assert_eq!(bound, ["ArrayList<String>", "Pair<String,Boolean>", "Pair<integer,String>"]);
    // Their «bind» lines say what they substitute.
    let mut binds: Vec<String> = model
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Dependency(d) if e.data().stereotypes == ["bind"] => {
                binding_text(model, d.client, d.supplier)
            }
            _ => None,
        })
        .collect();
    binds.sort();
    assert_eq!(binds, ["<E->String>", "<T1->String,T2->Boolean>", "<T1->integer,T2->String>"]);
    // Attributes keep their types as expressions.
    let a_pair = find(model, "aPair", |k| matches!(k, ElementKind::Attribute(_)));
    let ElementKind::Attribute(p) = a_pair.kind() else { unreachable!() };
    assert!(matches!(p.type_ref, Some(TypeRef::Expr(_))), "{:?}", p.type_ref);
}

#[test]
fn sample_instances_hide_names_types_and_slots() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    let style = |name: &str| {
        let e = find(model, name, |k| matches!(k, ElementKind::Instance(_)));
        let (_, node) = model.shown_on(e.id())[0];
        let diagram = model.shown_on(e.id())[0].0;
        model.view(diagram).unwrap().get(node).unwrap().style.clone()
    };
    // `: Client`, `VideoStoreApplication` and `videoStoreServer : AppServer` in Astah.
    assert!(style("client").hide_instance_name);
    assert!(!style("client").hide_instance_type);
    let application = style("VideoStoreApplication");
    assert!(!application.hide_instance_name && application.hide_instance_type);
    let server = style("videoStoreServer");
    assert!(!server.hide_instance_name && !server.hide_instance_type);
    // The objects inside the components show no slots.
    let objects: Vec<_> = model
        .elements()
        .filter(
            |e| matches!(e.kind(), ElementKind::Instance(p) if p.instance == InstanceKind::Object),
        )
        .flat_map(|e| model.shown_on(e.id()))
        .map(|(d, n)| model.view(d).unwrap().get(n).unwrap().style.hide_attributes)
        .collect();
    assert!(!objects.is_empty() && objects.iter().all(|&hidden| hidden), "{objects:?}");
}

/// Every sample that ships with Astah: what UmlCraft knows is placed in full, with all shapes.
#[test]
fn known_elements_and_their_shapes_are_never_lost() {
    fn asta_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                asta_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "asta") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    asta_files(&astah_home(), &mut files);
    if files.is_empty() {
        skip(&format!("no samples in {}", astah_home().display()));
        return;
    }
    for path in files {
        let imported = umlcraft_asta::import(&path).unwrap();
        let name = path.display();
        assert!(imported.failed.is_empty(), "{name}: {:?}", imported.failed);
        assert_eq!(imported.lost_presentations, 0, "{name}");
    }
}

#[test]
fn sample_composite_structure_diagram() {
    let Some(path) = astah_file("Sample.asta") else { return };
    let imported = umlcraft_asta::import(&path).unwrap();
    let model = &imported.model;
    model.check().unwrap();
    let [diagram] = diagrams(model, DiagramKind::CompositeStructure)[..] else {
        panic!("one composite structure diagram")
    };
    let view = model.view(diagram.id()).unwrap();
    let shown = |pred: &dyn Fn(&ElementKind) -> bool| {
        view.presentations()
            .iter()
            .filter(|p| p.element.and_then(|e| model.get(e)).is_some_and(|e| pred(e.kind())))
            .count()
    };
    assert_eq!(shown(&|k| matches!(k, ElementKind::Port(_))), 4);
    assert_eq!(shown(&|k| matches!(k, ElementKind::Attribute(_))), 4, "parts");
    assert_eq!(shown(&|k| matches!(k, ElementKind::Connector(_))), 2);

    // Car's rear wheels are the end of a composite association that no diagram shows; they
    // become a part of Car, and the connector reaches the engine's port through its part.
    let car = find(model, "Car", |k| matches!(k, ElementKind::Class(_)));
    let rear = find(model, "rear", |k| matches!(k, ElementKind::Attribute(_)));
    assert_eq!(rear.owner(), Some(car.id()));
    let wheel = find(model, "Wheel", |k| matches!(k, ElementKind::Class(_)));
    let ElementKind::Attribute(p) = rear.kind() else { unreachable!() };
    assert_eq!(p.type_ref, Some(TypeRef::Element(wheel.id())));
    let axle = find(model, "axle", |k| matches!(k, ElementKind::Connector(_)));
    assert_eq!(axle.owner(), Some(car.id()));
    let ElementKind::Connector(c) = axle.kind() else { unreachable!() };
    let ends: Vec<_> = c.ends.iter().map(|e| (e.role, e.part_with_port)).collect();
    assert!(ends.contains(&(rear.id(), None)), "{ends:?}");
    assert!(ends.iter().any(|&(_, through)| through.is_some()), "{ends:?}");

    // The required interface is a socket, the provided one a circle.
    let icon = |name: &str| {
        let interface = find(model, name, |k| matches!(k, ElementKind::Interface(_)));
        view.presentations()
            .iter()
            .filter(|p| p.element == Some(interface.id()) && p.style.iconic)
            .map(|p| p.style.socket)
            .collect::<Vec<_>>()
    };
    assert_eq!(icon("power"), [true]);
    assert_eq!(icon("powertrain"), [false]);
    // Ports keep where Astah put their names.
    assert!(view.presentations().iter().filter(|p| p.style.name_offset.is_some()).count() >= 4);
}
