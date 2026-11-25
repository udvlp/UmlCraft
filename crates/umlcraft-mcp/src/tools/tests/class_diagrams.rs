//! Class, object, use case and deployment diagrams, and how their labels and lines are drawn.

use super::*;

#[test]
fn a_class_diagram_is_built_step_by_step() {
    let mut host = Headless::new();
    let document = run(&mut host, "get_document", json!({}));
    assert_eq!(document["live"], false);
    let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
    let d = id(&d["diagram"]);
    let order =
        run(&mut host, "create_element", json!({ "diagram": d, "tool": "class", "name": "Order" }));
    let customer = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "class", "name": "Customer" }),
    );
    let premium = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "class", "name": "Premium" }),
    );
    assert_eq!(order["name"], "Order");
    let total = run(
        &mut host,
        "add_member",
        json!({ "owner": order["element"], "text": "+ total() : int" }),
    );
    assert_eq!(total["kind"], "operation");
    let name = run(
        &mut host,
        "add_member",
        json!({ "owner": customer["element"], "text": "- name : String" }),
    );
    assert_eq!(name["text"], "- name : String");
    let association = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "association", "source": customer["element"], "target": order["element"] }),
    );
    assert_eq!(association["kind"], "association");
    run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "generalization", "source": premium["element"], "target": customer["element"] }),
    );
    // UML forbids it, so UmlCraft refuses it and nothing changes.
    let revision = host.document().revision();
    refusal(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "realization", "source": order["element"], "target": customer["element"] }),
    );
    assert_eq!(host.document().revision(), revision);

    let arranged = run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    assert!(arranged["moved"].as_u64().unwrap() > 0);
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    assert_eq!(diagram["nodes"].as_array().unwrap().len(), 3);
    assert_eq!(diagram["lines"].as_array().unwrap().len(), 2);
    assert!(diagram["element_tools"].as_array().unwrap().contains(&json!("class")));
    let image = call(&mut host, &Access::default(), "render_diagram", &json!({ "diagram": d }));
    assert!(matches!(&image.content[0], Content::Image(png) if png.starts_with(b"\x89PNG")));
    host.document().model().check().unwrap();

    // One undo step per change.
    run(&mut host, "delete_element", json!({ "id": order["element"] }));
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    assert_eq!(diagram["nodes"].as_array().unwrap().len(), 2);
    run(&mut host, "undo", json!({}));
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    assert_eq!(diagram["nodes"].as_array().unwrap().len(), 3);
}

#[test]
fn objects_leave_out_slots_without_a_value_on_request() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let buch = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "class",
                                                        "name": "Buch" }),
    );
    for text in ["- titel : String", "- ausgeliehen : boolean"] {
        run(&mut host, "add_member", json!({ "owner": buch["element"], "text": text }));
    }
    let o = run(&mut host, "create_diagram", json!({ "kind": "object", "name": "Objekte" }));
    let o = id(&o["diagram"]);
    let args = json!({ "diagram": o, "tool": "object", "name": "a", "classifier": "Buch",
                       "slots": { "titel": "\"Momo\"" }, "empty_slots": false });
    let a = run(&mut host, "create_element", args);
    let slots = |host: &mut Headless| -> Vec<String> {
        drawn(host, &o)
            .nodes
            .iter()
            .flat_map(|n| n.rows.iter())
            .filter(|r| r.role == scene::RowRole::Slot)
            .map(|r| r.text.clone())
            .collect()
    };
    assert_eq!(slots(&mut host), ["titel = \"Momo\""]);
    run(&mut host, "update_element", json!({ "id": a["element"], "empty_slots": true }));
    assert_eq!(slots(&mut host), ["titel = \"Momo\"", "ausgeliehen"]);
    // Only objects have slots.
    let wrong = json!({ "id": buch["element"], "empty_slots": false });
    refusal(&mut host, "update_element", wrong);
}

#[test]
fn ends_abstract_static_classifier_and_slots_are_set() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let node = |host: &mut Headless, args: Value| {
        let mut args = args;
        args["diagram"] = json!(d);
        run(host, "create_element", args)
    };
    let lager = node(&mut host, json!({ "tool": "class", "name": "Lager", "abstract": true }));
    let geraet = node(&mut host, json!({ "tool": "class", "name": "Geraet" }));
    let args = json!({ "diagram": d, "tool": "association", "source": lager["element"],
                       "target": geraet["element"],
                       "source_end": { "aggregation": "composite" },
                       "target_end": { "role": "- geraete {ordered}", "multiplicity": "*",
                                       "navigability": "navigable" } });
    let line = run(&mut host, "connect", args);
    let kind = |host: &Headless, v: &Value| {
        host.document().model().get(id(v).parse().unwrap()).unwrap().kind().clone()
    };
    let ElementKind::Association(p) = kind(&host, &line["element"]) else { panic!() };
    assert_eq!(p.ends[0].aggregation, umlcraft_model::Aggregation::Composite);
    let end = &p.ends[1];
    assert_eq!((end.role.as_str(), end.visibility), ("geraete", Visibility::Private));
    assert_eq!(end.constraints, ["ordered"]);
    assert_eq!(end.multiplicity.map(|m| m.to_string()), Some("*".into()));
    assert_eq!(end.navigability, umlcraft_model::Navigability::Navigable);
    let ElementKind::Class(c) = kind(&host, &lager["element"]) else { panic!() };
    assert!(c.is_abstract);
    let bad = json!({ "id": line["element"], "target_end": { "multiplicity": "viele" } });
    refusal(&mut host, "update_element", bad);

    // Static members, written so or flagged.
    let add = |host: &mut Headless, text: &str| {
        run(host, "add_member", json!({ "owner": geraet["element"], "text": text }))
    };
    let anzahl = add(&mut host, "- anzahl : int {static}");
    assert_eq!(anzahl["text"], "- anzahl : int");
    let ElementKind::Attribute(a) = kind(&host, &anzahl["id"]) else { panic!() };
    assert!(a.is_static);
    let get = add(&mut host, "+ getAnzahl() : int");
    run(&mut host, "update_element", json!({ "id": get["id"], "static": true }));
    let ElementKind::Operation(o) = kind(&host, &get["id"]) else { panic!() };
    assert!(o.is_static);

    // An object of a class, with values for its attributes.
    let hund = node(&mut host, json!({ "tool": "class", "name": "Hund" }));
    run(&mut host, "add_member", json!({ "owner": hund["element"], "text": "- name : String" }));
    let rex = node(
        &mut host,
        json!({ "tool": "object", "name": "rex", "classifier": "Hund",
                "slots": { "name": "\"Rex\"" } }),
    );
    let ElementKind::Instance(i) = kind(&host, &rex["element"]) else { panic!() };
    assert_eq!(i.classifier, Some(id(&hund["element"]).parse().unwrap()));
    let model = host.document().model();
    let slots = umlcraft_notation::object::slots(model, id(&rex["element"]).parse().unwrap());
    assert_eq!(slots.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), ["\"Rex\""]);
    let wrong = json!({ "id": rex["element"], "slots": { "alter": "3" } });
    refusal(&mut host, "update_element", wrong);
    host.document().model().check().unwrap();
}

#[test]
fn roles_keep_to_their_own_ends() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = |host: &mut Headless, name: &str, members: &[&str]| {
        let c = run(host, "create_element", json!({ "diagram": d, "tool": "class", "name": name }));
        for member in members {
            run(host, "add_member", json!({ "owner": c["element"], "text": member }));
        }
        c
    };
    // The doubly linked list of #21.
    let operations = [
        "+ Playlist()",
        "+ hintenAnfuegen(lied : Lied)",
        "+ ausgeben()",
        "+ rueckwaertsAusgeben()",
        "+ loeschen(titel : String) : boolean",
    ];
    let playlist = class(&mut host, "Playlist", &operations);
    let knoten = class(&mut host, "Knoten", &["+ Knoten(inhalt : Lied)"]);
    let associate = |host: &mut Headless, s: &Value, t: &Value, role: &str| {
        let args = json!({ "diagram": d, "tool": "association", "source": s["element"],
                           "target": t["element"],
                           "target_end": { "role": role, "multiplicity": "0..1",
                                           "navigability": "navigable" } });
        run(host, "connect", args);
    };
    associate(&mut host, &knoten, &knoten, "- nachfolger");
    associate(&mut host, &knoten, &knoten, "- vorgaenger");
    associate(&mut host, &playlist, &knoten, "- kopf");
    associate(&mut host, &playlist, &knoten, "- ende");
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let (lines, _) = drawn_labels(&mut host, &d);
    keep_to_own_lines(&lines, "");
    let mut roles = Vec::new();
    for (points, labels) in &lines {
        let target = points[points.len() - 1];
        let source = points[0];
        for (text, r) in labels.iter().filter(|(t, _)| t.starts_with('-')) {
            // Nearer the end it belongs to than the other one, and close to it.
            let (to_target, to_source) = (target.distance(r.center()), source.distance(r.center()));
            assert!(to_target < 80.0, "{text} {r:?} {points:?}");
            if source != target {
                assert!(to_target < to_source, "{text} {r:?} {points:?}");
            }
            roles.push(*r);
        }
    }
    assert_eq!(roles.len(), 4);
    for (i, a) in roles.iter().enumerate() {
        assert!(roles[i + 1..].iter().all(|b| !a.intersects(b)), "{roles:?}");
    }
}

#[test]
fn a_long_role_fits_between_classes_laid_out_left_to_right() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = |host: &mut Headless, name: &str| {
        let args = json!({ "diagram": d, "tool": "class", "name": name, "abstract": true });
        run(host, "create_element", args)
    };
    let (gatter, baustein) = (class(&mut host, "Gatter"), class(&mut host, "Baustein"));
    let args = json!({ "diagram": d, "tool": "association", "source": gatter["element"],
                       "target": baustein["element"],
                       "target_end": { "role": "# eingaenge", "multiplicity": "*",
                                       "navigability": "navigable" } });
    run(&mut host, "connect", args);
    run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
    let (lines, nodes) = drawn_labels(&mut host, &d);
    let (points, labels) = &lines[0];
    for (text, r) in labels {
        assert!(nodes.iter().all(|n| !n.intersects(r)), "{text} {r:?} {nodes:?}");
        let crossed = points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
        assert!(!crossed, "{text} {r:?} {points:?}");
    }
}

#[test]
fn labels_of_both_ends_fit_between_classes_laid_out_left_to_right() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = |host: &mut Headless, name: &str, member: &str| {
        let args = json!({ "diagram": d, "tool": "class", "name": name });
        let c = run(host, "create_element", args);
        run(host, "add_member", json!({ "owner": c["element"], "text": member }));
        c
    };
    let leser = class(&mut host, "Leser", "- name : String");
    let buch = class(&mut host, "Buch", "- titel : String");
    let end = |role: &str| {
        json!({ "role": role, "multiplicity": "0..1",
                                   "navigability": "navigable" })
    };
    let args = json!({ "diagram": d, "tool": "association", "source": leser["element"],
                       "target": buch["element"], "source_end": end("- leser"),
                       "target_end": end("- buch") });
    run(&mut host, "connect", args);
    run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
    let (lines, nodes) = drawn_labels(&mut host, &d);
    let (points, labels) = &lines[0];
    for (k, (text, r)) in labels.iter().enumerate() {
        assert!(nodes.iter().all(|n| !n.intersects(r)), "{text} {r:?} {nodes:?}");
        let crossed = points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
        assert!(!crossed, "{text} {r:?} {points:?}");
        for (other, o) in &labels[k + 1..] {
            assert!(!r.intersects(o), "{text} {r:?} {other} {o:?}");
        }
    }
}

#[test]
fn a_role_beside_a_generalization_keeps_to_its_target() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = |host: &mut Headless, name: &str, is_abstract: bool| {
        let args = json!({ "diagram": d, "tool": "class", "name": name, "abstract": is_abstract });
        run(host, "create_element", args)
    };
    let baustein = class(&mut host, "Baustein", true);
    let schalter = class(&mut host, "Schalter", false);
    let gatter = class(&mut host, "Gatter", true);
    let und = class(&mut host, "Und", false);
    let oder = class(&mut host, "Oder", false);
    let inherit = |host: &mut Headless, from: &Value, to: &Value| {
        let args = json!({ "diagram": d, "tool": "generalization",
                           "source": from["element"], "target": to["element"] });
        run(host, "connect", args)
    };
    inherit(&mut host, &schalter, &baustein);
    inherit(&mut host, &gatter, &baustein);
    inherit(&mut host, &und, &gatter);
    inherit(&mut host, &oder, &gatter);
    let args = json!({ "diagram": d, "tool": "association", "source": gatter["element"],
                       "target": baustein["element"],
                       "target_end": { "role": "# eingaenge", "multiplicity": "*",
                                       "navigability": "navigable" } });
    run(&mut host, "connect", args);
    run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "down" }));
    let (lines, _) = drawn_labels(&mut host, &d);
    keep_to_own_lines(&lines, "");
    // Clear of the generalization trees: no stretch within a port's spacing of their lines
    // (all lines here run straight across or down, so their gap is that of their bounds).
    let (association, _) = lines.iter().find(|(_, l)| !l.is_empty()).expect("the association");
    for (tree, _) in lines.iter().filter(|(_, l)| l.is_empty()) {
        for (w, v) in association.windows(2).flat_map(|w| tree.windows(2).map(move |v| (w, v))) {
            let (a, b) = (Rect::from_corners(w[0], w[1]), Rect::from_corners(v[0], v[1]));
            let dx = (a.x - b.right()).max(b.x - a.right()).max(0.0);
            let dy = (a.y - b.bottom()).max(b.y - a.bottom()).max(0.0);
            assert!(dx.hypot(dy) >= 20.0, "{w:?} {v:?}");
        }
    }
    // The role at the association's end at Baustein, not beside Schalter.
    let (points, labels) = lines
        .iter()
        .find(|(_, labels)| labels.iter().any(|(t, _)| t == "# eingaenge"))
        .expect("the association's labels");
    let target = points[points.len() - 1];
    for (text, r) in labels {
        assert!(r.center().distance(target) < 80.0, "{text} {r:?} {points:?}");
    }
    let (_, role) = labels.iter().find(|(t, _)| t == "# eingaenge").unwrap();
    let nearest =
        Point::new(target.x.clamp(role.x, role.right()), target.y.clamp(role.y, role.bottom()));
    assert!(nearest.distance(target) < 40.0, "{role:?} {points:?}");
    // Its arrow clear of the triangle where the generalizations leave Baustein.
    let (stem, _) = lines.iter().find(|(_, l)| l.is_empty()).expect("a generalization");
    let triangle = stem[stem.len() - 1];
    assert!(triangle.distance(target) > 30.0, "{triangle:?} {points:?}");
    let (_, many) = labels.iter().find(|(t, _)| t == "*").unwrap();
    assert!(!many.intersects(role), "{many:?} {role:?}");
}

#[test]
fn two_loops_keep_clear_of_incoming_lines() {
    for arrange in [true, false] {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str, y: f32| {
            let args = json!({ "diagram": d, "tool": "class", "name": name, "x": 100.0,
                               "y": y });
            run(host, "create_element", args)
        };
        let (playlist, knoten) =
            (class(&mut host, "Playlist", 40.0), class(&mut host, "Knoten", 220.0));
        let associate = |host: &mut Headless, from: &Value, to: &Value, role: &str| {
            let args = json!({ "diagram": d, "tool": "association", "source": from["element"],
                               "target": to["element"],
                               "target_end": { "role": role, "multiplicity": "0..1",
                                               "navigability": "navigable" } });
            run(host, "connect", args)
        };
        associate(&mut host, &playlist, &knoten, "- kopf");
        associate(&mut host, &playlist, &knoten, "- ende");
        associate(&mut host, &knoten, &knoten, "- nachfolger");
        associate(&mut host, &knoten, &knoten, "- vorgaenger");
        if arrange {
            run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        }
        let (lines, nodes) = drawn_labels(&mut host, &d);
        let all: Vec<(&String, Rect)> =
            lines.iter().flat_map(|(_, l)| l.iter().map(|(t, r)| (t, *r))).collect();
        for (k, (text, r)) in all.iter().enumerate() {
            // No label on another one, on a class or on a line.
            for (other, o) in &all[k + 1..] {
                assert!(!r.intersects(o), "{arrange}: {text} {r:?} {other} {o:?}");
            }
            assert!(nodes.iter().all(|n| !n.intersects(r)), "{arrange}: {text} {r:?}");
            for (points, _) in &lines {
                let crossed =
                    points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
                assert!(!crossed, "{arrange}: {text} {r:?} {points:?}");
            }
        }
        // Each label beside its own line, near its target end.
        keep_to_own_lines(&lines, if arrange { "arranged" } else { "as drawn" });
        for (points, labels) in &lines {
            let target = points[points.len() - 1];
            for (text, r) in labels {
                let d = r.center().distance(target);
                assert!(d < 80.0, "{arrange}: {text} {r:?} {points:?}");
            }
        }
        // The lines' ends apart from each other.
        let ends: Vec<Point> = lines.iter().flat_map(|(p, _)| [p[0], p[p.len() - 1]]).collect();
        for (k, a) in ends.iter().enumerate() {
            for b in &ends[k + 1..] {
                assert!(a.distance(*b) > 5.0, "{arrange}: {a:?} {b:?}");
            }
        }
    }
}

#[test]
fn object_diagrams_are_made_and_named_so() {
    let mut host = Headless::new();
    let args = json!({ "kind": "object", "name": "Snapshot" });
    let d = run(&mut host, "create_diagram", args)["diagram"].clone();
    let document = run(&mut host, "get_document", json!({}));
    let shown = document["diagrams"].as_array().unwrap().iter().find(|x| x["id"] == d);
    assert_eq!(shown.unwrap()["kind"], "object");
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    assert_eq!(diagram["kind"], "object");
    assert_eq!(diagram["element_tools"][0], "object");
}

#[test]
fn links_between_objects_run_straight_after_arranging() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = json!({ "diagram": d, "tool": "class", "name": "Hund" });
    let hund = run(&mut host, "create_element", class);
    for text in ["- name : String", "- alter : int"] {
        run(&mut host, "add_member", json!({ "owner": hund["element"], "text": text }));
    }
    // One object shows two slots, the others none: their boxes differ in height.
    let object = |host: &mut Headless, name: &str, slots: Value| {
        let args = json!({ "diagram": d, "tool": "object", "name": name,
                           "classifier": "Hund", "slots": slots });
        run(host, "create_element", args)
    };
    let rex = object(&mut host, "rex", json!({ "name": "\"Rex\"", "alter": "3" }));
    let p1 = object(&mut host, "p1", json!({}));
    let p2 = object(&mut host, "p2", json!({}));
    for (s, t) in [(&rex, &p1), (&p1, &p2)] {
        let args = json!({ "diagram": d, "tool": "link", "source": s["element"],
                           "target": t["element"] });
        run(&mut host, "connect", args);
    }
    for direction in ["right", "down"] {
        let args = json!({ "diagram": d, "direction": direction });
        run(&mut host, "arrange_diagram", args);
        for e in &drawn(&mut host, &d).edges {
            assert_eq!(e.points.len(), 2, "{direction}: {:?}", e.points);
        }
    }
}

#[test]
fn actors_and_use_cases_are_not_associated_among_themselves() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
    let d = d["diagram"].clone();
    let node = |host: &mut Headless, tool: &str, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
    };
    let (a, b) = (node(&mut host, "actor", "A"), node(&mut host, "actor", "B"));
    let (u, v) = (node(&mut host, "useCase", "U"), node(&mut host, "useCase", "V"));
    let associate = |host: &mut Headless, x: &Value, y: &Value, anyway: bool| {
        let args = json!({ "diagram": d, "tool": "association", "source": x["element"],
                           "target": y["element"], "allow_invalid": anyway });
        !call(host, &Access::default(), "connect", &args).is_error
    };
    assert!(associate(&mut host, &a, &u, false));
    assert!(!associate(&mut host, &a, &b, false));
    assert!(!associate(&mut host, &u, &v, false));
    // For an exercise in finding mistakes.
    assert!(associate(&mut host, &a, &b, true));
    assert!(associate(&mut host, &u, &v, true));
    host.document().model().check().unwrap();
}

#[test]
fn actors_specializing_one_actor_stand_side_by_side_below_it() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
    let d = d["diagram"].clone();
    let node = |host: &mut Headless, tool: &str, name: &str, x: f32, y: f32| {
        let args = json!({ "diagram": d, "tool": tool, "name": name, "x": x, "y": y });
        run(host, "create_element", args)
    };
    node(&mut host, "systemBoundary", "Selbstlernzentrum", 200.0, 40.0);
    let lend = node(&mut host, "useCase", "Gerät ausleihen", 230.0, 90.0);
    let check = node(&mut host, "useCase", "Ausweis prüfen", 230.0, 170.0);
    let reserve = node(&mut host, "useCase", "Gerät reservieren", 230.0, 250.0);
    let manage = node(&mut host, "useCase", "Geräte verwalten", 230.0, 330.0);
    let user = node(&mut host, "actor", "Nutzer", 40.0, 60.0);
    let pupil = node(&mut host, "actor", "Schüler", 10.0, 200.0);
    let teacher = node(&mut host, "actor", "Lehrkraft", 90.0, 200.0);
    let guard = node(&mut host, "actor", "Aufsicht", 600.0, 340.0);
    let connect = |host: &mut Headless, tool: &str, from: &Value, to: &Value| {
        let args = json!({ "diagram": d, "tool": tool, "source": from["element"],
                           "target": to["element"] });
        run(host, "connect", args)
    };
    connect(&mut host, "generalization", &pupil, &user);
    connect(&mut host, "generalization", &teacher, &user);
    connect(&mut host, "association", &user, &lend);
    connect(&mut host, "association", &teacher, &reserve);
    connect(&mut host, "association", &guard, &manage);
    connect(&mut host, "include", &lend, &check);
    connect(&mut host, "extend", &reserve, &lend);
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));

    let model = host.document().model();
    let view = model.view(id(&d).parse().unwrap()).unwrap();
    let rect = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap().bounds();
    let (user, pupil, teacher) =
        (rect(&user).unwrap(), rect(&pupil).unwrap(), rect(&teacher).unwrap());
    // Side by side, below Nutzer, each with a line of its own up to it.
    assert!((pupil.y - teacher.y).abs() < 0.5, "{pupil:?} {teacher:?}");
    assert!(!pupil.intersects(&teacher));
    assert!(pupil.y > user.bottom() && teacher.y > user.bottom());
    let (lo, hi) =
        (pupil.center().x.min(teacher.center().x), pupil.center().x.max(teacher.center().x));
    assert!(lo < user.center().x && user.center().x < hi, "{user:?} {pupil:?} {teacher:?}");
    model.check().unwrap();
}

#[test]
fn actors_stand_on_both_sides_of_the_boundary_with_straight_lines() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
    let d = d["diagram"].clone();
    // Use cases inside the boundary, actors below it.
    let node = |host: &mut Headless, tool: &str, name: &str, x: f32, y: f32| {
        let args = json!({ "diagram": d, "tool": tool, "name": name, "x": x, "y": y });
        run(host, "create_element", args)
    };
    let boundary = node(&mut host, "systemBoundary", "Selbstlernzentrum", 100.0, 100.0);
    let lend = node(&mut host, "useCase", "Gerät ausleihen", 130.0, 150.0);
    let back = node(&mut host, "useCase", "Gerät zurückgeben", 130.0, 230.0);
    let check = node(&mut host, "useCase", "Ausweis prüfen", 130.0, 310.0);
    let reserve = node(&mut host, "useCase", "Gerät vormerken", 130.0, 390.0);
    let user = node(&mut host, "actor", "Nutzer", 100.0, 600.0);
    let teacher = node(&mut host, "actor", "Lehrkraft", 200.0, 600.0);
    let librarian = node(&mut host, "actor", "Bibliothekarin", 300.0, 600.0);
    let connect = |host: &mut Headless, tool: &str, from: &Value, to: &Value| {
        let args = json!({ "diagram": d, "tool": tool, "source": from["element"],
                           "target": to["element"] });
        run(host, "connect", args)
    };
    let lines = [
        connect(&mut host, "generalization", &teacher, &user),
        connect(&mut host, "association", &user, &lend),
        connect(&mut host, "association", &user, &back),
        connect(&mut host, "association", &teacher, &reserve),
        connect(&mut host, "association", &librarian, &lend),
    ];
    connect(&mut host, "include", &lend, &check);
    connect(&mut host, "extend", &reserve, &lend);
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));

    let model = host.document().model();
    let view = model.view(id(&d).parse().unwrap()).unwrap();
    let rect = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap().bounds();
    let frame = rect(&boundary).unwrap();
    let (user, teacher, librarian) =
        (rect(&user).unwrap(), rect(&teacher).unwrap(), rect(&librarian).unwrap());
    for case in [&lend, &back, &check, &reserve] {
        assert!(frame.contains_rect(&rect(case).unwrap()), "{:?} {frame:?}", rect(case));
    }
    // The use cases one below the other, those of Nutzer next to each other, so that the
    // lines from the left don't cross.
    let middle = |v: &Value| rect(v).unwrap().center();
    assert!([&back, &check, &reserve].iter().all(|v| (middle(v).x - middle(&lend).x).abs() < 0.5));
    let order = |v: &Value| middle(v).y;
    assert!(order(&lend) < order(&back) && order(&back) < order(&reserve));
    // Nutzer and Lehrkraft on one side, Lehrkraft below; Bibliothekarin on the other.
    let left = |r: Rect| r.right() <= frame.x;
    let right = |r: Rect| r.x >= frame.right();
    assert!(left(user) || right(user), "{user:?} {frame:?}");
    assert_eq!(left(user), left(teacher));
    assert!((user.center().x - teacher.center().x).abs() < 0.5);
    assert!(teacher.y > user.bottom());
    assert!(if left(user) { right(librarian) } else { left(librarian) }, "{librarian:?}");
    // Straight lines.
    for line in &lines {
        let p = view.get(id(&line["presentation"]).parse().unwrap()).unwrap();
        let Shape::Edge { waypoints, .. } = &p.shape else { panic!() };
        assert!(waypoints.is_empty());
        assert_eq!(p.style.line_shape, umlcraft_model::LineShape::Line);
    }
    model.check().unwrap();
}

#[test]
fn arranging_keeps_generalizations_chosen_separate() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let class = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "class", "name": name }))
    };
    let (b, s, g) =
        (class(&mut host, "Baustein"), class(&mut host, "Schalter"), class(&mut host, "Gatter"));
    let inherit = |host: &mut Headless, from: &Value| {
        let args = json!({ "diagram": d, "tool": "generalization",
                           "source": from["element"], "target": b["element"] });
        run(host, "connect", args)
    };
    let lines = [inherit(&mut host, &s), inherit(&mut host, &g)];
    let styles = |host: &Headless| -> Vec<SharedStyle> {
        let view = host.document().model().view(d.parse().unwrap()).unwrap();
        let style =
            |l: &Value| view.get(id(&l["presentation"]).parse().unwrap()).unwrap().style.shared;
        lines.iter().map(style).collect()
    };
    // Arranged as they come, they share a triangle.
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    assert_eq!(styles(&host), [SharedStyle::Vertical; 2]);
    // Chosen separate, they stay so.
    for line in &lines {
        let args = json!({ "id": line["element"], "shared_style": "separate" });
        run(&mut host, "update_element", args);
    }
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
    assert_eq!(styles(&host), [SharedStyle::Separate; 2]);
}

#[test]
fn association_classes_constraints_shared_generalizations_and_robustness_icons() {
    use umlcraft_model::SharedStyle;
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "School" }));
    let d = d["diagram"].clone();
    let class = |host: &mut Headless, tool: &str, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
    };
    let student = class(&mut host, "class", "Student");
    let course = class(&mut host, "class", "Course");
    let person = class(&mut host, "class", "Person");
    let teacher = class(&mut host, "class", "Teacher");
    let pupil = class(&mut host, "class", "Pupil");
    let screen = class(&mut host, "boundary", "Screen");
    let enrolment = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "associationClass", "source": student["element"],
                "target": course["element"], "name": "Enrolment" }),
    );
    let generalize = |host: &mut Headless, specific: &Value| {
        let g = json!({ "diagram": d, "tool": "generalization", "source": specific["element"],
                        "target": person["element"] });
        run(host, "connect", g)
    };
    let (g1, g2) = (generalize(&mut host, &teacher), generalize(&mut host, &pupil));
    run(
        &mut host,
        "update_element",
        json!({ "id": enrolment["element"], "constraints": ["{ordered}", "unique"] }),
    );
    run(
        &mut host,
        "update_element",
        json!({ "id": g1["element"], "shared_style": "vertical", "constraints": ["complete"] }),
    );
    run(&mut host, "update_element", json!({ "id": screen["element"], "icon": false }));
    // Only generalizations share, only robustness classes have an icon.
    let wrong = json!({ "id": enrolment["element"], "shared_style": "vertical" });
    refusal(&mut host, "update_element", wrong);
    let wrong = json!({ "id": student["element"], "icon": true });
    refusal(&mut host, "update_element", wrong);

    let model = host.document().model();
    model.check().unwrap();
    let id = |v: &Value| v.as_str().unwrap().parse::<Id>().unwrap();
    let association = model.get(id(&enrolment["element"])).unwrap();
    let ElementKind::Association(a) = association.kind() else { panic!() };
    let class = model.get(a.class.expect("the association class")).unwrap();
    assert_eq!(class.name(), "Enrolment");
    assert_eq!(association.data().constraints, ["ordered", "unique"]);
    assert_eq!(model.get(id(&g1["element"])).unwrap().data().constraints, ["complete"]);
    let view = model.view(id(&d)).unwrap();
    for g in [&g1, &g2] {
        let p = view.get(id(&g["presentation"])).unwrap();
        assert_eq!(p.style.shared, SharedStyle::Vertical);
    }
    assert!(!view.get(id(&screen["presentation"])).unwrap().style.iconic);
    assert_eq!(model.get(id(&screen["element"])).unwrap().data().stereotypes, ["boundary"]);
    // The class box and the dashed line to the association's line.
    assert_eq!(model.shown_on(class.id()).len(), 1);
    let class = class.id();
    let element = run(&mut host, "get_element", json!({ "id": enrolment["element"] }));
    assert_eq!(element["properties"]["class"], json!(class));
    assert_eq!(element["properties"]["constraints"], json!(["ordered", "unique"]));
}

#[test]
fn use_cases_made_without_a_place_go_into_the_boundary() {
    let mut host = Headless::new();
    let (d, made) = mensa(&mut host, [None, None, None]);
    let rects = drawn_rects(&mut host, &d, &made[..4]);
    let (boundary, cases) = (rects[0], &rects[1..]);
    for case in cases {
        assert!(boundary.contains_rect(case), "{case:?} outside {boundary:?}");
    }
    for pair in cases.windows(2) {
        assert!(pair[0].bottom() < pair[1].y, "one below the other: {pair:?}");
    }
    // The actor, too, is placed without a position, and stays outside.
    let actor = drawn_rects(&mut host, &d, &made[4..])[0];
    assert!(!boundary.intersects(&actor), "{actor:?} in {boundary:?}");
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let rects = drawn_rects(&mut host, &d, &made[..4]);
    for case in &rects[1..] {
        assert!(rects[0].contains_rect(case), "{case:?} outside {:?}", rects[0]);
    }
}

#[test]
fn arranged_use_cases_stay_inside_their_boundary() {
    let mut host = Headless::new();
    let at = [Some((250.0, 60.0)), Some((250.0, 140.0)), Some((450.0, 60.0))];
    let (d, made) = mensa(&mut host, at);
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let scene = drawn(&mut host, &d);
    let measure = host.parts().1;
    let rect = |v: &Value| scene.node(id(&v["presentation"]).parse().unwrap()).unwrap().rect;
    let boundary = rect(&made[0]);
    let cases: Vec<Rect> = made[1..4].iter().map(rect).collect();
    for case in &cases {
        assert!(boundary.contains_rect(case), "{case:?} sticks out of {boundary:?}");
    }
    // «include» runs from the first use case to the third without crossing the second,
    // its label inside the boundary.
    let include = scene
        .edges
        .iter()
        .find(|e| e.labels.iter().any(|l| l.text.contains("include")))
        .expect("the include");
    for pair in include.points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        for k in 0..=20 {
            let t = k as f32 / 20.0;
            let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            assert!(!cases[1].contains(p), "{:?} crosses {:?}", include.points, cases[1]);
        }
    }
    let label = &include.labels[0];
    let width = scene::label_width(&label.text, measure);
    let left = match label.align {
        scene::LabelAlign::Left => label.anchor.x,
        scene::LabelAlign::Center => label.anchor.x - width / 2.0,
        scene::LabelAlign::Right => label.anchor.x - width,
    };
    let label = Rect::new(left, label.anchor.y, width, scene::LABEL_HEIGHT);
    assert!(boundary.contains_rect(&label), "{label:?} outside {boundary:?}");
}

#[test]
fn roles_keep_off_the_class_they_name() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "rolle" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Bibliothek" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Buch" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
              "tool": "association", "source": "$1.element", "target": "$2.element",
              "target_end": { "navigability": "navigable", "role": "- buecher",
                              "multiplicity": "*" } } },
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram",
              "direction": "right" } },
        ]),
    );
    let (doc, measure) = host.parts();
    let d = id(&r[0]["diagram"]).parse().unwrap();
    let scene = scene::build(doc.model(), d, &Preview::default(), measure);
    let book = scene.node(id(&r[2]["presentation"]).parse().unwrap()).unwrap().rect;
    let edge = scene.edge(id(&r[3]["presentation"]).parse().unwrap()).unwrap();
    // A gap as the multiplicity keeps, not touching the box (#51).
    for label in &edge.labels {
        let drawn = labels::label_rect(label, measure);
        assert!(drawn.right() + 4.0 <= book.x, "{} at {drawn:?}, box {book:?}", label.text);
    }
}

#[test]
fn roles_at_one_class_do_not_run_into_one() {
    let mut host = Headless::new();
    let class = |name: &str| {
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "class", "name": name } })
    };
    let general = |from: usize| {
        json!({ "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "generalization", "source": format!("${from}.element"),
                "target": "$1.element" } })
    };
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "gatter" } },
            class("Gatter"), class("Und"), class("Oder"), class("Nicht"),
            general(2), general(3), general(4),
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
              "tool": "association", "source": "$1.element", "target": "$1.element",
              "target_end": { "role": "- eingaenge", "multiplicity": "1..2",
                              "navigability": "navigable" } } },
            class("Schaltung"),
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
              "tool": "association", "source": "$9.element", "target": "$1.element",
              "source_end": { "aggregation": "composite" },
              "target_end": { "role": "- gatter", "multiplicity": "*",
                              "navigability": "navigable" } } },
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
        ]),
    );
    // No two labels side by side without a gap, where they read as one (#52).
    let (doc, measure) = host.parts();
    let d = id(&r[0]["diagram"]).parse().unwrap();
    let scene = scene::build(doc.model(), d, &Preview::default(), measure);
    let drawn: Vec<(String, Rect)> = [&r[8], &r[10]]
        .iter()
        .flat_map(|line| {
            let edge = scene.edge(id(&line["presentation"]).parse().unwrap()).unwrap();
            edge.labels.iter().map(|l| (l.text.clone(), labels::label_rect(l, measure)))
        })
        .collect();
    for (i, (a, ra)) in drawn.iter().enumerate() {
        for (b, rb) in &drawn[i + 1..] {
            let rows = ra.y < rb.bottom() - 1.0 && rb.y < ra.bottom() - 1.0;
            let apart = ra.right() + 3.0 <= rb.x || rb.right() + 3.0 <= ra.x;
            assert!(!rows || apart, "{a} at {ra:?}, {b} at {rb:?}");
        }
    }
}

#[test]
fn lines_keep_a_last_stretch_longer_than_their_marker() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "tiere" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Tier" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Hund" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Katze" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
              "tool": "generalization", "source": "$2.element", "target": "$1.element" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
              "tool": "generalization", "source": "$3.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$4.element",
              "shared_style": "separate" } },
            { "tool": "update_element", "arguments": { "id": "$5.element",
              "shared_style": "separate" } },
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
        ]),
    );
    // Not bent right before the triangle, where the bend meets it (#53).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    for line in [&r[4], &r[5]] {
        let points = &scene.edge(id(&line["presentation"]).parse().unwrap()).unwrap().points;
        let n = points.len();
        assert!(n == 2 || points[n - 1].distance(points[n - 2]) > 20.0, "{points:?}");
    }
}

#[test]
fn a_multiplicity_stays_on_its_side_of_a_line_above_it() {
    let mut host = Headless::new();
    let members: [(&str, &[&str]); 4] = [
        ("Betrieb", &["- name : String", "+ einstellen(m : Mitarbeiter)", "+ ausgeben()"]),
        ("Mitarbeiter", &["- name : String", "- krank : boolean", "+ istKrank() : boolean"]),
        ("Auftrag", &["- nummer : String", "+ findeBearbeiter()"]),
        ("Maschine", &["- name : String", "+ schulen(m : Mitarbeiter)"]),
    ];
    let mut calls = vec![json!({ "tool": "create_diagram",
        "arguments": { "kind": "class", "name": "krank" } })];
    for (name, _) in members {
        calls.push(json!({ "tool": "create_element", "arguments": {
            "diagram": "$0.diagram", "tool": "class", "name": name } }));
    }
    for (k, (_, texts)) in members.iter().enumerate() {
        for text in *texts {
            calls.push(json!({ "tool": "add_member", "arguments": {
                "owner": format!("${}.element", k + 1), "text": text } }));
        }
    }
    let n = calls.len();
    let assoc = |s: usize, t: usize, role: &str, target: &str, source: &str| {
        json!({ "tool": "connect", "arguments": { "diagram": "$0.diagram",
            "tool": "association", "source": format!("${s}.element"),
            "target": format!("${t}.element"),
            "target_end": { "role": role, "multiplicity": target, "navigability": "navigable" },
            "source_end": { "multiplicity": source } } })
    };
    let at = |k: usize, x: f64, y: f64| {
        json!({ "tool": "move_node", "arguments": { "diagram": "$0.diagram",
            "node": format!("${k}.presentation"), "x": x, "y": y } })
    };
    calls.extend([
        assoc(1, 2, "- belegschaft", "*", "1"),
        assoc(2, 3, "- auftraege", "*", "1"),
        assoc(3, 4, "- maschine", "1", "*"),
        assoc(4, 2, "- geschulte", "*", "*"),
        json!({ "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram",
            "direction": "right" } }),
        at(1, 40.0, 40.0),
        at(2, 400.0, 25.0),
        at(3, 470.0, 250.0),
        at(4, 40.0, 243.0),
    ]);
    let r = build(&mut host, json!(calls));
    // The `1` of the lower line at `Maschine` stays below the upper line (#62).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let edge = |k: usize| scene.edge(id(&r[k]["presentation"]).parse().unwrap()).unwrap();
    let (lower, upper) = (edge(n + 2), edge(n + 3));
    let upper_y = upper.points[0].y;
    assert_eq!(upper_y, upper.points[1].y, "{:?}", upper.points);
    let one = lower.labels.iter().find(|l| l.text == "1").unwrap();
    let drawn = labels::label_rect(one, measure);
    assert!(drawn.y >= upper_y, "{drawn:?} above the line at {upper_y}");
}

#[test]
fn links_run_straight_where_objects_overlap_and_keep_off_separators() {
    let mut host = Headless::new();
    let root = run(&mut host, "get_document", json!({}))["root"].clone();
    let class = |name: &str, members: &[&str]| {
        let mut calls = vec![json!({ "tool": "create_model_element", "arguments": {
            "owner": root, "tool": "class", "name": name } })];
        calls.extend(members.iter().map(|text| {
            json!({ "tool": "add_member", "arguments": { "owner": "$0.element", "text": text } })
        }));
        calls
    };
    let ids: Vec<Value> = [
        class("Mitglied", &["- name : String"]),
        class("Zeitschrift", &["- titel : String", "- jahr : int", "- ausgabe : int"]),
        class("Buch", &["- titel : String", "- jahr : int"]),
    ]
    .into_iter()
    .map(|calls| build(&mut host, json!(calls))[0]["element"].clone())
    .collect();
    let object = |name: &str, classifier: &Value| {
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
            "tool": "object", "name": name, "classifier": classifier } })
    };
    let at = |k: usize, x: f64, y: f64, width: f64| {
        json!({ "tool": "move_node", "arguments": { "diagram": "$0.diagram",
            "node": format!("${k}.presentation"), "x": x, "y": y, "width": width } })
    };
    let link = |t: usize| {
        json!({ "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "link",
            "source": "$1.element", "target": format!("${t}.element") } })
    };
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "object", "name": "objekte" } },
            object("ben", &ids[0]),
            object("z", &ids[1]),
            object("b", &ids[2]),
            link(2),
            link(3),
            at(1, 119.0, 40.0, 95.0),
            at(2, 40.0, 162.19, 96.0),
            at(3, 196.0, 162.19, 98.0),
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
        ]),
    );
    // `ben` overlaps both below it: straight down, not out of its side at the height of its
    // compartment separator (#63).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let ben = scene.node(id(&r[1]["presentation"]).parse().unwrap()).unwrap();
    for line in [&r[4], &r[5]] {
        let points = &scene.edge(id(&line["presentation"]).parse().unwrap()).unwrap().points;
        assert_eq!(points.len(), 2, "{points:?}");
        assert_eq!(points[0].y, ben.rect.bottom(), "{points:?}");
        assert_eq!(points[0].x, points[1].x, "{points:?}");
    }
}

#[test]
fn a_line_drops_its_detour_when_the_node_it_went_around_moves_away() {
    // #76: one object right above the other, a third one lying on the upper one while
    // the lower one moves into place; then the third moves away.
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "object", "name": "o" }));
    let d = id(&d["diagram"]);
    let object = |host: &mut Headless, name: &str| {
        let o =
            run(host, "create_element", json!({ "diagram": d, "tool": "object", "name": name }));
        id(&o["element"])
    };
    let (upper, lower, other) =
        (object(&mut host, "a2"), object(&mut host, "jana"), object(&mut host, "digital"));
    let link = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "link", "source": lower, "target": upper }),
    );
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let place = |host: &mut Headless, node: &str, x: f32, y: f32, w: f32, h: f32| {
        run(
            host,
            "move_node",
            json!({ "diagram": d, "node": node, "x": x, "y": y, "width": w, "height": h }),
        );
    };
    place(&mut host, &upper, 280.0, 20.0, 120.0, 46.19);
    place(&mut host, &other, 274.5, 40.0, 134.9, 46.19);
    place(&mut host, &lower, 180.0, 130.0, 120.0, 61.28);
    let bends = |host: &Headless| {
        let model = host.document().model();
        let view = model.view(d.parse().unwrap()).unwrap();
        let p = view.get(id(&link["presentation"]).parse().unwrap()).unwrap();
        let Shape::Edge { waypoints, .. } = &p.shape else { panic!() };
        waypoints.len()
    };
    // Around the object that lies on the upper one.
    assert!(bends(&host) >= 2, "{}", bends(&host));
    place(&mut host, &other, 660.0, 415.0, 134.9, 46.19);
    // Straight through where the two overlap, or one bend.
    assert!(bends(&host) <= 1, "{}", bends(&host));
}

#[test]
fn a_rerouted_line_swaps_ports_with_a_line_that_stays_instead_of_crossing_it() {
    // #78: two links leave the bottom of an object, one bending off toward an object
    // below left, the other straight down; they get their routes one after the other.
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "object", "name": "o" }));
    let d = id(&d["diagram"]);
    let object = |host: &mut Headless, name: &str| {
        let o =
            run(host, "create_element", json!({ "diagram": d, "tool": "object", "name": name }));
        id(&o["element"])
    };
    let (b, tom, digital) =
        (object(&mut host, "b"), object(&mut host, "tom"), object(&mut host, "digital"));
    let connect = |host: &mut Headless, source: &str, target: &str| {
        let l = run(
            host,
            "connect",
            json!({ "diagram": d, "tool": "link", "source": source, "target": target }),
        );
        id(&l["presentation"])
    };
    // A machine to the left of the upper object, linked to it, keeps the bending link off
    // that side.
    let schneider = object(&mut host, "schneider");
    let beside = connect(&mut host, &schneider, &tom);
    let first = connect(&mut host, &b, &tom);
    let second = connect(&mut host, &tom, &digital);
    let place = |host: &mut Headless, node: &str, x: f32, y: f32, w: f32, h: f32| {
        run(
            host,
            "move_node",
            json!({ "diagram": d, "node": node, "x": x, "y": y, "width": w, "height": h }),
        );
    };
    place(&mut host, &tom, 690.0, 130.0, 107.0, 61.28);
    place(&mut host, &schneider, 400.0, 145.0, 180.0, 46.19);
    place(&mut host, &b, 400.0, 285.0, 176.0, 46.19);
    place(&mut host, &digital, 660.0, 415.0, 138.0, 46.19);
    run(&mut host, "route_lines", json!({ "diagram": d, "lines": [beside] }));
    // The bending link as an earlier edit left it: out of the middle of the upper object's
    // bottom, down and left into the right side of the object below left.
    let (diagram, link): (Id, Id) = (d.parse().unwrap(), first.parse().unwrap());
    host.parts()
        .0
        .edit("route", |tx| {
            let waypoints = vec![Point::new(743.5, 298.09)];
            let anchors = (Some(Point::new(1.0, 13.09 / 46.19)), Some(Point::new(0.5, 1.0)));
            tx.set_route(diagram, link, waypoints, anchors)?;
            tx.update_presentation(diagram, link, |p| {
                p.style.line_shape = umlcraft_model::LineShape::RightAngle;
                p.style.auto_route = true;
            })
        })
        .unwrap();
    run(&mut host, "route_lines", json!({ "diagram": d, "lines": [second] }));
    let model = host.document().model();
    let view = model.view(d.parse().unwrap()).unwrap();
    let points = |line: &str| {
        let p = view.get(line.parse().unwrap()).unwrap();
        let Shape::Edge { source, target, waypoints, source_anchor, target_anchor, .. } = &p.shape
        else {
            panic!()
        };
        let bounds = |n: &Id| match view.get(*n).unwrap().shape {
            Shape::Node { bounds, .. } => bounds,
            _ => panic!(),
        };
        let mut out = vec![umlcraft_notation::route::anchor_point(bounds(source), *source_anchor)];
        out.extend(waypoints.iter().copied());
        out.push(umlcraft_notation::route::anchor_point(bounds(target), *target_anchor));
        out
    };
    let (a, c) = (points(&first), points(&second));
    for s in a.windows(2) {
        for t in c.windows(2) {
            let vertical = (s[0].x - s[1].x).abs() < 0.5;
            let (v, h) = if vertical { (s, t) } else { (t, s) };
            let crosses = (h[0].y - h[1].y).abs() < 0.5
                && (v[0].x - v[1].x).abs() < 0.5
                && v[0].x > h[0].x.min(h[1].x) + 0.5
                && v[0].x < h[0].x.max(h[1].x) - 0.5
                && h[0].y > v[0].y.min(v[1].y) + 0.5
                && h[0].y < v[0].y.max(v[1].y) - 0.5;
            assert!(!crosses, "{a:?} {c:?}");
        }
    }
}

#[test]
fn new_objects_fit_their_content_until_sized_by_hand() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "cd" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "class", "name": "P", "x": 20, "y": 20 } },
            { "tool": "add_member", "arguments": { "owner": "$1.element", "text": "- n : int" } },
            { "tool": "create_diagram", "arguments": { "kind": "object", "name": "od" } },
            { "tool": "create_element", "arguments": { "diagram": "$3.diagram",
                "tool": "object", "name": "p", "classifier": "P", "slots": { "n": "1" },
                "x": 20, "y": 20 } },
        ]),
    );
    // As narrow as its texts (#83), where it was put.
    let rect = |host: &mut Headless| {
        let scene = drawn(host, &r[3]["diagram"]);
        scene.node(id(&r[4]["presentation"]).parse().unwrap()).unwrap().rect
    };
    let fitted = rect(&mut host);
    assert!(fitted.width < 80.0, "{fitted:?}");
    assert_eq!(fitted.x, 20.0);
    let node = |host: &mut Headless| {
        let d: Id = id(&r[3]["diagram"]).parse().unwrap();
        let p: Id = id(&r[4]["presentation"]).parse().unwrap();
        host.document().model().view(d).unwrap().get(p).unwrap().clone()
    };
    assert!(node(&mut host).style.auto_size);
    assert_eq!(node(&mut host).bounds().unwrap().width, fitted.width);
    // A longer value widens it.
    run(
        &mut host,
        "update_element",
        json!({ "id": r[4]["element"], "slots": { "n": "123456789" } }),
    );
    assert!(rect(&mut host).width > fitted.width);
    // Sized by hand, it keeps that size.
    let args = json!({ "diagram": r[3]["diagram"], "node": r[4]["presentation"], "y": 20,
                       "width": 150 });
    run(&mut host, "move_node", args);
    assert!(!node(&mut host).style.auto_size);
    assert_eq!(rect(&mut host).width, 150.0);
}

#[test]
fn lines_take_a_route_given_by_hand() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
                "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "erEntity", "name": "User", "x": 20, "y": 100 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "erRelationship", "name": "refers", "x": 260, "y": 95 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
                "source": "$2.element", "target": "$1.element",
                "waypoints": [[290, 50], [70, 50]], "source_anchor": [0.5, 0],
                "target_anchor": [0.5, 0], "line_shape": "right_angle" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
                "source": "$2.element", "target": "$1.element" } },
        ]),
    );
    let d: Id = id(&r[0]["diagram"]).parse().unwrap();
    let route = |host: &mut Headless, line: &Value| {
        let p: Id = id(line).parse().unwrap();
        let p = host.document().model().view(d).unwrap().get(p).unwrap().clone();
        let Shape::Edge { waypoints, source_anchor, target_anchor, .. } = p.shape else {
            panic!("{p:?}")
        };
        (waypoints, source_anchor, target_anchor, p.style.line_shape, p.style.auto_route)
    };
    // Given with the line (#84).
    let (points, from, to, shape, auto) = route(&mut host, &r[3]["presentation"]);
    assert_eq!(points, vec![Point::new(290.0, 50.0), Point::new(70.0, 50.0)]);
    assert_eq!((from, to), (Some(Point::new(0.5, 0.0)), Some(Point::new(0.5, 0.0))));
    assert_eq!((shape, auto), (LineShape::RightAngle, false));
    // Changed later, by the line's element or its presentation.
    let below = json!({ "id": r[4]["element"], "waypoints": [[290, 200], [70, 200]],
                        "source_anchor": [0.5, 1], "target_anchor": [0.5, 1],
                        "line_shape": "right_angle", "auto_route": false });
    run(&mut host, "update_element", below);
    let (points, from, ..) = route(&mut host, &r[4]["presentation"]);
    assert_eq!(points, vec![Point::new(290.0, 200.0), Point::new(70.0, 200.0)]);
    assert_eq!(from, Some(Point::new(0.5, 1.0)));
    let straight = json!({ "id": r[4]["presentation"], "waypoints": [],
                           "source_anchor": null, "line_shape": "line" });
    run(&mut host, "update_element", straight);
    let (points, from, to, shape, _) = route(&mut host, &r[4]["presentation"]);
    assert_eq!((points, from, to), (vec![], None, Some(Point::new(0.5, 1.0))));
    assert_eq!(shape, LineShape::Line);
    // `get_diagram` tells the route as the tools take it.
    let lines = run(&mut host, "get_diagram", json!({ "diagram": d.to_string() }))["lines"].clone();
    let first = lines
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["presentation"] == r[3]["presentation"])
        .unwrap()
        .clone();
    assert_eq!(first["waypoints"], json!([[290.0, 50.0], [70.0, 50.0]]));
    assert_eq!(first["source_anchor"], json!([0.5, 0.0]));
    assert_eq!(first["line_shape"], "right_angle");
    assert_eq!(first["auto_route"], false);
    // Moving a node keeps a route given by hand.
    let args = json!({ "diagram": d.to_string(), "node": r[1]["element"], "x": 30, "y": 100 });
    run(&mut host, "move_node", args);
    assert_eq!(route(&mut host, &r[3]["presentation"]).0[0], Point::new(290.0, 50.0));
    // Wrong routes, and arguments a tool does not take, are refused.
    for (tool, args) in [
        ("update_element", json!({ "id": r[3]["element"], "waypoints": [[1, 2, 3]] })),
        ("update_element", json!({ "id": r[3]["element"], "target_anchor": [1.5, 0] })),
        (
            "update_element",
            json!({ "id": r[3]["element"], "auto_route": true,
                                   "waypoints": [] }),
        ),
        ("update_element", json!({ "id": r[1]["element"], "waypoints": [] })),
        ("update_element", json!({ "id": r[3]["presentation"], "name": "x" })),
        ("update_element", json!({ "id": r[1]["element"], "diagram": d.to_string() })),
        ("update_element", json!({ "id": r[3]["element"], "waypionts": [] })),
        (
            "connect",
            json!({ "diagram": d.to_string(), "tool": "erLine",
                            "source": r[2]["element"], "target": r[1]["element"],
                            "bends": [[1, 2]] }),
        ),
    ] {
        let reply = call(&mut host, &Access::default(), tool, &args);
        assert!(reply.is_error, "{tool} {args}");
    }
    let reply = call(
        &mut host,
        &Access::default(),
        "move_node",
        &json!({ "diagram": d.to_string(), "node": r[1]["element"], "y": 1, "z": 2 }),
    );
    let Content::Text(text) = &reply.content[0] else { panic!() };
    assert!(text.starts_with("`move_node` has no argument `z`; it takes"), "{text}");
    // The router takes a line back.
    run(&mut host, "update_element", json!({ "id": r[3]["element"], "auto_route": true }));
    assert!(route(&mut host, &r[3]["presentation"]).4);
}

#[test]
fn use_cases_take_extension_points_and_extends_name_them() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "use_case", "name": "u" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "useCase", "name": "Fach mieten", "x": 100, "y": 60 } },
            { "tool": "add_member", "arguments": { "owner": "$1.element",
                "text": "Fachwahl" } },
            { "tool": "create_model_element", "arguments": { "owner": "$1.element",
                "tool": "extensionPoint", "name": "Zahlung" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "useCase", "name": "Fach reservieren", "x": 100, "y": 260 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "extend",
                "source": "$4.element", "target": "$1.element",
                "condition": "Fach frei", "extension_points": ["Fachwahl"] } },
        ]),
    );
    // An extension point, not a hidden attribute (#87).
    assert_eq!(r[2]["kind"], "extensionPoint");
    assert_eq!(r[3]["kind"], "extensionPoint");
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": r[0]["diagram"] }));
    let case = diagram["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["element"] == r[1]["element"])
        .unwrap()
        .clone();
    let names: Vec<&str> = case["extension_points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Fachwahl", "Zahlung"]);
    // The extend's condition and points (#88).
    let extend = |host: &mut Headless| {
        let e: Id = id(&r[5]["element"]).parse().unwrap();
        match host.document().model().get(e).unwrap().kind() {
            ElementKind::Extend(p) => p.clone(),
            other => panic!("{other:?}"),
        }
    };
    let fachwahl: Id = id(&r[2]["id"]).parse().unwrap();
    let zahlung: Id = id(&r[3]["id"]).parse().unwrap();
    assert_eq!(extend(&mut host).condition, "Fach frei");
    assert_eq!(extend(&mut host).extension_points, [fachwahl]);
    let both = json!({ "id": r[5]["element"], "condition": "",
                       "extension_points": [r[3]["id"], "Fachwahl"] });
    let reply = run(&mut host, "update_element", both);
    assert_eq!(reply["extension_points"], json!([zahlung, fachwahl]));
    assert_eq!(extend(&mut host).condition, "");
    for (tool, args) in [
        ("update_element", json!({ "id": r[5]["element"], "extension_points": ["Kasse"] })),
        ("update_element", json!({ "id": r[1]["element"], "condition": "x" })),
        (
            "connect",
            json!({ "diagram": r[0]["diagram"], "tool": "include",
                            "source": r[4]["element"], "target": r[1]["element"],
                            "condition": "x" }),
        ),
        ("add_member", json!({ "owner": r[1]["element"], "text": " " })),
    ] {
        refusal(&mut host, tool, args);
    }
    // The points stay inside the ellipse of a use case with a short name (#90).
    let (doc, measure) = host.parts();
    let d: Id = id(&r[0]["diagram"]).parse().unwrap();
    let scene = scene::build(doc.model(), d, &Preview::default(), measure);
    let node = scene.node(id(&r[1]["presentation"]).parse().unwrap()).unwrap();
    let (c, a, b) = (node.rect.center(), node.rect.width / 2.0, node.rect.height / 2.0);
    let inside = |x: f32, y: f32| ((x - c.x) / a).powi(2) + ((y - c.y) / b).powi(2) <= 1.0;
    let points: Vec<_> =
        node.rows.iter().filter(|row| row.align == scene::LabelAlign::Left).collect();
    assert_eq!(points.len(), 3, "{:?}", node.rows);
    for row in points {
        let right = row.rect.x + measure.width(&row.text, scene::FONT_SIZE, false);
        for (x, y) in [
            (row.rect.x, row.rect.y),
            (right, row.rect.y),
            (row.rect.x, row.rect.bottom()),
            (right, row.rect.bottom()),
        ] {
            assert!(inside(x, y), "{} at {x}, {y} out of {:?}", row.text, node.rect);
        }
    }
}

#[test]
fn comments_show_the_text_they_are_given() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "use_case", "name": "u" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "comment", "name": "Hallo\nWelt", "x": 260, "y": 160 } },
        ]),
    );
    // The name is the note's text (#89).
    assert_eq!(r[1]["text"], "Hallo\nWelt");
    let drawn = drawn_svg(&mut host, &r[0]["diagram"]);
    assert!(drawn.contains(">Hallo<") && drawn.contains(">Welt<"), "{drawn}");
    let renamed = run(&mut host, "update_element", json!({ "id": r[1]["element"], "name": "Neu" }));
    assert_eq!(renamed["text"], "Neu");
    assert!(drawn_svg(&mut host, &r[0]["diagram"]).contains(">Neu<"));
    run(&mut host, "update_element", json!({ "id": r[1]["element"], "text": "Bedingung: frei" }));
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": r[0]["diagram"] }));
    assert_eq!(diagram["nodes"][0]["text"], "Bedingung: frei");
}

#[test]
fn a_constraint_joins_two_associations() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "c" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "class", "name": "Regal", "x": 40, "y": 40 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "class", "name": "Leser", "x": 40, "y": 300 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "class", "name": "Buch", "x": 400, "y": 170 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "association", "source": "$1.element", "target": "$3.element" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "association", "source": "$2.element", "target": "$3.element" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "constraint", "source": "$4.element", "target": "$5.element" } },
        ]),
    );
    // #91: `{xor}` beside a dashed line between the middles of the two lines.
    assert_eq!(r[6]["kind"], "constraint");
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let edge = |at: &Value| {
        let p = id(at);
        scene.edges.iter().find(|e| e.id.to_string() == p).unwrap().clone()
    };
    let line = edge(&r[6]["presentation"]);
    assert_eq!(line.line, scene::LineStyle::ShortDashed);
    assert_eq!(line.labels.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["{xor}"]);
    for (end, association) in [(0, &r[4]), (1, &r[5])] {
        let a = edge(&association["presentation"]);
        let (p, q) = umlcraft_notation::route::middle_segment(&a.points, false);
        let middle = Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0);
        let tip = if end == 0 { line.points[0] } else { *line.points.last().unwrap() };
        assert!((tip.x - middle.x).abs() < 1.0 && (tip.y - middle.y).abs() < 1.0);
    }
    let svg = drawn_svg(&mut host, &r[0]["diagram"]);
    assert!(svg.contains("{xor}"), "{svg}");

    // Given in braces, the name loses them.
    let renamed = json!({ "id": r[6]["element"], "name": "{or}" });
    run(&mut host, "update_element", renamed);
    let model = host.document().model();
    assert_eq!(model.get(id(&r[6]["element"]).parse().unwrap()).unwrap().name(), "or");

    // Only between associations; deleting one of them deletes the constraint.
    let wrong = json!({ "diagram": r[0]["diagram"], "tool": "constraint",
                        "source": r[1]["element"], "target": r[4]["element"] });
    refusal(&mut host, "connect", wrong);
    run(&mut host, "delete_element", json!({ "id": r[4]["element"] }));
    let model = host.document().model();
    assert!(model.get(id(&r[6]["element"]).parse().unwrap()).is_none());
    model.check().unwrap();
}

#[test]
fn a_members_text_stays_when_documentation_comes_with_it() {
    let mut host = Headless::new();
    let d = host.active_diagram().unwrap().to_string();
    let class = json!({ "diagram": d, "tool": "class", "name": "Order" });
    let class = run(&mut host, "create_element", class);
    let member = |host: &mut Headless, text: &str| {
        let args = json!({ "owner": class["element"], "text": text });
        id(&run(host, "add_member", args)["element"])
    };
    let (attribute, operation) = (member(&mut host, "- n : String"), member(&mut host, "+ f()"));
    let text_of = |host: &Headless, id: &str| {
        member::member_text(host.document().model(), id.parse().unwrap())
    };
    for extra in [
        json!({ "documentation": "How many." }),
        json!({ "stereotypes": ["counter"] }),
        json!({ "constraints": ["count >= 0"] }),
    ] {
        let mut args = json!({ "id": attribute, "text": "- count : int [0..*] = 5" });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(&mut host, "update_element", args);
        let text = text_of(&host, &attribute);
        assert!(
            text.contains("count : int[0..*] = 5") || text.contains("count : int [0..*] = 5"),
            "{extra}: {text}"
        );
        run(&mut host, "update_element", json!({ "id": attribute, "text": "- n : String" }));

        let mut args = json!({ "id": operation, "text": "+ total(a : int) : double" });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(&mut host, "update_element", args);
        let text = text_of(&host, &operation);
        assert!(text.contains("total(a : int) : double"), "{extra}: {text}");
        run(&mut host, "update_element", json!({ "id": operation, "text": "+ f()" }));
    }
    let model = host.document().model();
    let data = model.get(attribute.parse().unwrap()).unwrap().data();
    assert_eq!(data.documentation, "How many.");
    assert_eq!(data.constraints, ["count >= 0"]);
    model.check().unwrap();
}

/// An artifact drawn inside another belongs to it, as a node inside a node does: it moves with
/// it, and arranging lays it out inside.
#[test]
fn artifacts_hold_the_artifacts_drawn_inside_them() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "deployment", "name": "D" }));
    let d = id(&d["diagram"]);
    let node = |host: &mut Headless, tool: &str, name: &str, bounds: [f32; 4]| {
        let [x, y, width, height] = bounds;
        let made = run(
            host,
            "create_element",
            json!({ "diagram": d, "tool": tool, "name": name, "x": x, "y": y }),
        );
        let size = json!({ "diagram": d, "node": made["presentation"], "x": x, "y": y,
                           "width": width, "height": height });
        run(host, "move_node", size);
        made
    };
    let server = node(&mut host, "node", "Server", [40.0, 40.0, 400.0, 300.0]);
    let war = node(&mut host, "artifact", "app.war", [60.0, 90.0, 300.0, 180.0]);
    let jar = node(&mut host, "artifact", "lib.jar", [80.0, 150.0, 120.0, 60.0]);
    let made = [server, war, jar];

    let moved = json!({ "diagram": d, "node": made[1]["presentation"], "x": 60, "y": 120 });
    run(&mut host, "move_node", moved);
    assert_eq!(drawn_rects(&mut host, &d, &made)[2].y, 180.0);

    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let [server, war, jar] = drawn_rects(&mut host, &d, &made)[..] else { unreachable!() };
    assert!(war.contains_rect(&jar), "{jar:?} not in {war:?}");
    assert!(server.contains_rect(&war), "{war:?} not in {server:?}");
}
