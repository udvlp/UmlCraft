//! State machine and activity diagrams.

use super::*;

#[test]
fn activity_pins_parameters_and_calls() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
    let d = id(&d["diagram"]);
    let make = |host: &mut Headless, tool: &str, extra: Value| {
        let mut args = json!({ "diagram": d, "tool": tool });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(host, "create_element", args)
    };
    let pack = make(&mut host, "callBehavior", json!({ "name": "Pack", "x": 200, "y": 100 }));
    let pin_in = make(&mut host, "inputPin", json!({ "on": pack["presentation"] }));
    let pin_out = make(&mut host, "outputPin", json!({ "on": pack["presentation"] }));
    let send = make(&mut host, "sendSignal", json!({ "name": "Notify", "x": 200, "y": 250 }));
    let param = make(&mut host, "activityParameter", json!({ "name": "order", "x": 0, "y": 0 }));
    make(&mut host, "acceptTimeEvent", json!({ "name": "Daily" }));
    make(&mut host, "activityConnector", json!({ "name": "A" }));
    // A pin away from every action is refused.
    let lone = call(
        &mut host,
        &Access::default(),
        "create_element",
        &json!({ "diagram": d, "tool": "inputPin", "x": 900, "y": 900 }),
    );
    assert!(lone.is_error);
    run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "flow", "source": pin_out["element"], "target": send["element"] }),
    );
    let activity = host.document().model().element(parse_id(&d).unwrap()).unwrap().owner();
    let activity = activity.unwrap().to_string();
    // The activity called may be given by its diagram.
    run(&mut host, "update_element", json!({ "id": pack["element"], "calls": d }));
    run(
        &mut host,
        "update_element",
        json!({ "id": pin_in["element"], "ordering": "LIFO", "upper_bound": "2",
                "stream": true, "object_type": "Order" }),
    );
    for wrong in [
        json!({ "id": send["element"], "ordering": "FIFO" }),
        json!({ "id": pin_in["element"], "calls": activity }),
        json!({ "id": param["element"], "ordering": "sideways" }),
        json!({ "id": pack["element"], "calls": pin_in["element"] }),
    ] {
        refusal(&mut host, "update_element", wrong);
    }
    let model = host.document().model();
    model.check().unwrap();
    let node = |value: &Value| match model.element(parse_id(&id(&value["element"])).unwrap()) {
        Ok(e) => match e.kind() {
            ElementKind::ActivityNode(p) => p.clone(),
            other => panic!("{other:?}"),
        },
        Err(e) => panic!("{e}"),
    };
    assert_eq!(node(&pack).behavior.map(|b| b.to_string()), Some(activity.clone()));
    let pin = node(&pin_in);
    assert_eq!(pin.ordering, Some(umlcraft_model::ObjectOrdering::Lifo));
    assert_eq!(pin.upper_bound, "2");
    assert!(pin.is_stream);
    assert!(pin.type_ref.is_some());
    let pin_owner = model.element(parse_id(&id(&pin_in["element"])).unwrap()).unwrap().owner();
    assert_eq!(pin_owner.map(|o| o.to_string()), Some(id(&pack["element"])));
    assert_eq!(node(&param).node, ActivityNodeKind::ActivityParameter);
    // The output pin found room on the right of the action.
    let view = model.view(parse_id(&d).unwrap()).unwrap();
    let bounds = |value: &Value| {
        view.get(parse_id(&id(&value["presentation"])).unwrap()).unwrap().bounds().unwrap()
    };
    assert!(bounds(&pin_out).center().x > bounds(&pack).center().x);
    assert!(bounds(&pin_in).center().x < bounds(&pack).center().x);
}

/// Pins put `on` an action find room where their texts stay clear of the other pins'
/// texts, the action growing to hold them, and keep it when the diagram is arranged.
#[test]
fn pins_on_an_action_keep_their_texts_apart() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
    let d = id(&d["diagram"]);
    let action = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "action", "name": "Ship", "x": 200, "y": 100 }),
    );
    let pins = [
        json!({ "tool": "inputPin", "name": "order", "object_type": "Order",
                "in_state": "paid", "ordering": "FIFO", "upper_bound": "2" }),
        json!({ "tool": "inputPin", "name": "address", "control": true }),
        json!({ "tool": "inputPin", "name": "voucher", "exception": true,
                "object_effect": "read" }),
        json!({ "tool": "outputPin", "name": "parcel", "stream": true }),
        json!({ "tool": "outputPin", "name": "receipt", "in_state": "printed" }),
    ];
    let action_node = parse_id(&id(&action["presentation"])).unwrap();
    let diagram = parse_id(&d).unwrap();
    // Every text of a pin of the action, and its square, by pin.
    let shapes = |host: &mut Headless| -> Vec<(Rect, Vec<Rect>)> {
        let (doc, measure) = host.parts();
        let model = doc.model();
        let scene = scene::build(model, diagram, &Preview::default(), measure);
        let borne = composite::borne_nodes(model, diagram);
        scene
            .nodes
            .iter()
            .filter(|n| borne.get(&n.id).copied().flatten() == Some(action_node))
            .map(|n| (n.rect, n.outside()))
            .collect()
    };
    let clear = |shapes: &[(Rect, Vec<Rect>)]| {
        let overlap = |a: &Rect, b: &Rect| {
            a.x < b.right() - 0.01
                && b.x < a.right() - 0.01
                && a.y < b.bottom() - 0.01
                && b.y < a.bottom() - 0.01
        };
        for (i, (_, texts)) in shapes.iter().enumerate() {
            for (j, (square, others)) in shapes.iter().enumerate().filter(|(j, _)| *j != i) {
                for t in texts {
                    assert!(!overlap(t, square), "pin {i}: {t:?} on pin {j} {square:?}");
                    for o in others {
                        assert!(!overlap(t, o), "pin {i}: {t:?} on {o:?} of pin {j}");
                    }
                }
            }
        }
    };
    let size = |host: &Headless| {
        let view = host.document().model().view(diagram).unwrap();
        view.get(action_node).unwrap().bounds().unwrap()
    };
    let before = size(&host);
    for pin in &pins {
        let mut args = json!({ "diagram": d, "on": action["presentation"] });
        args.as_object_mut().unwrap().extend(pin.as_object().unwrap().clone());
        run(&mut host, "create_element", args);
        clear(&shapes(&mut host));
    }
    assert_eq!(shapes(&mut host).len(), pins.len());
    let grown = size(&host);
    assert!(grown.height > before.height, "{grown:?}");
    // Input pins on the left, output pins on the right.
    let squares: Vec<Rect> = shapes(&mut host).into_iter().map(|(r, _)| r).collect();
    assert_eq!(squares.iter().filter(|r| r.right() <= grown.x + 0.01).count(), 3);
    assert_eq!(squares.iter().filter(|r| r.x >= grown.right() - 0.01).count(), 2);
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    clear(&shapes(&mut host));
    host.document().model().check().unwrap();
}

#[test]
fn control_exception_and_effect_of_object_nodes() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
    let d = id(&d["diagram"]);
    let action = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "action", "name": "Ship", "x": 200, "y": 100 }),
    );
    // `create_element` takes the settings along: a control pin, an exception pin.
    let control = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "inputPin", "on": action["presentation"],
                "control": true, "object_effect": "read" }),
    );
    let exception = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "outputPin", "on": action["presentation"],
                "exception": true }),
    );
    let object = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "objectNode", "name": "Box", "x": 200, "y": 300 }),
    );
    run(
        &mut host,
        "update_element",
        json!({ "id": object["element"], "control": true, "exception": true,
                "object_effect": "delete" }),
    );
    run(&mut host, "update_element", json!({ "id": control["element"], "object_effect": "" }));
    for (tool, wrong) in [
        ("update_element", json!({ "id": action["element"], "exception": true })),
        ("update_element", json!({ "id": object["element"], "object_effect": "eat" })),
        ("update_element", json!({ "id": object["element"], "control": "yes" })),
        ("create_element", json!({ "diagram": d, "tool": "action", "name": "X", "control": true })),
    ] {
        refusal(&mut host, tool, wrong);
    }
    let model = host.document().model();
    model.check().unwrap();
    // The refused action was not created.
    assert!(model.elements().all(|e| e.name() != "X"));
    let node = |value: &Value| match model.element(parse_id(&id(&value["element"])).unwrap()) {
        Ok(e) => match e.kind() {
            ElementKind::ActivityNode(p) => p.clone(),
            other => panic!("{other:?}"),
        },
        Err(e) => panic!("{e}"),
    };
    let pin = node(&control);
    assert!(pin.is_control && !pin.is_exception && pin.effect.is_none());
    assert!(node(&exception).is_exception);
    let o = node(&object);
    assert!(o.is_control && o.is_exception);
    assert_eq!(o.effect, Some(umlcraft_model::ObjectEffect::Delete));
    assert_eq!(o.property_lines(), ["{delete}", "{control}"]);
}

#[test]
fn moved_states_become_substates_and_take_theirs_along() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
    let d = id(&d["diagram"]);
    let make = |host: &mut Headless, name: &str, x: f32, y: f32| {
        let args = json!({ "diagram": d, "tool": "state", "name": name, "x": x, "y": y });
        run(host, "create_element", args)
    };
    // A state made first, then a larger one, drawn later, moved over it.
    let inner = make(&mut host, "z1", 600.0, 100.0);
    let deeper = make(&mut host, "z2", 100.0, 400.0);
    let outer = make(&mut host, "Outer", 100.0, 100.0);
    let size = json!({ "diagram": d, "node": outer["presentation"], "x": 100, "y": 100,
                       "width": 360, "height": 220 });
    run(&mut host, "move_node", size);
    for (node, x, y) in [(&inner, 140, 160), (&deeper, 300, 160)] {
        let args = json!({ "diagram": d, "node": node["presentation"], "x": x, "y": y });
        run(&mut host, "move_node", args);
    }
    let model = host.document().model();
    let owner = |e: &Value| model.get(id(e).parse().unwrap()).unwrap().owner();
    let outer_id: Id = id(&outer["element"]).parse().unwrap();
    assert_eq!(owner(&inner["element"]), Some(outer_id));
    assert_eq!(owner(&deeper["element"]), Some(outer_id));
    // Both are drawn after the state they are in, so it does not hide them.
    let view = model.view(d.parse().unwrap()).unwrap();
    let at = |p: &Value| {
        let p: Id = id(p).parse().unwrap();
        view.presentations().iter().position(|q| q.id == p).unwrap()
    };
    assert!(at(&inner["presentation"]) > at(&outer["presentation"]));
    assert!(at(&deeper["presentation"]) > at(&outer["presentation"]));
    // Moving the composite state takes its substates along.
    let args = json!({ "diagram": d, "node": outer["presentation"], "x": 200, "y": 100 });
    run(&mut host, "move_node", args);
    let model = host.document().model();
    let view = model.view(d.parse().unwrap()).unwrap();
    let x = |p: &Value| view.get(id(p).parse().unwrap()).unwrap().bounds().unwrap().x;
    assert_eq!(x(&inner["presentation"]), 240.0);
    assert_eq!(model.get(id(&inner["element"]).parse().unwrap()).unwrap().owner(), Some(outer_id));
}

#[test]
fn opposite_transitions_between_states_side_by_side_run_apart() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "S" }));
    let d = id(&d["diagram"]);
    let make = |host: &mut Headless, x: f32| {
        run(host, "create_element", json!({ "diagram": d, "tool": "state", "x": x, "y": 100 }))
    };
    let (a, b) = (make(&mut host, 100.0), make(&mut host, 400.0));
    let connect = |host: &mut Headless, s: &Value, t: &Value| {
        let args = json!({ "diagram": d, "tool": "transition", "source": s["element"],
                           "target": t["element"] });
        id(&run(host, "connect", args)["presentation"])
    };
    let there = connect(&mut host, &a, &b);
    let back = connect(&mut host, &b, &a);
    let scene = drawn(&mut host, &d);
    let points = |e: &str| scene.edge(e.parse().unwrap()).unwrap().points.clone();
    let ys = |e: &str| points(e).iter().map(|p| p.y).collect::<Vec<_>>();
    // The second line, routed with the first, does not lie on it.
    assert!(
        ys(&there).iter().all(|y| !ys(&back).contains(y)),
        "{:?} {:?}",
        points(&there),
        points(&back)
    );
    // A line from B to a third state is routed with neither: it shares only B with them.
    let c = make(&mut host, 700.0);
    let onward = connect(&mut host, &b, &c);
    let model = host.document().model();
    let ids = |e: &[&String]| e.iter().map(|e| e.parse().unwrap()).collect::<Vec<_>>();
    let along =
        umlcraft_notation::autoroute::with_neighbours(model, d.parse().unwrap(), &ids(&[&there]));
    assert_eq!(along, ids(&[&there, &back]));
    let along =
        umlcraft_notation::autoroute::with_neighbours(model, d.parse().unwrap(), &ids(&[&onward]));
    assert_eq!(along, ids(&[&onward]));
}

#[test]
fn names_of_opposite_transitions_stand_outside_their_lines() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "S" }));
    let d = id(&d["diagram"]);
    let make = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "state", "name": name }))
    };
    let (a, b) = (make(&mut host, "A"), make(&mut host, "B"));
    let connect = |host: &mut Headless, s: &Value, t: &Value, name: &str| {
        let args = json!({ "diagram": d, "tool": "transition", "source": s["element"],
                           "target": t["element"], "name": name });
        id(&run(host, "connect", args)["presentation"])
    };
    let there = connect(&mut host, &a, &b, "A to B");
    let back = connect(&mut host, &b, &a, "B to A");
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    // Where the label's middle lies across the line's middle segment, and where the other
    // line does.
    let across = |e: &str, p: Point| {
        let edge = scene.edge(e.parse().unwrap()).unwrap();
        let (s, t) = umlcraft_notation::route::middle_segment(&edge.points, false);
        let len = s.distance(t);
        ((t.x - s.x) * (p.y - s.y) - (t.y - s.y) * (p.x - s.x)) / len
    };
    let label_middle = |e: &str| {
        let edge = scene.edge(e.parse().unwrap()).unwrap();
        let label = &edge.labels[0];
        let width = scene::label_width(&label.text, measure);
        label.anchor.offset(width / 2.0, scene::LABEL_HEIGHT / 2.0)
    };
    let line_middle = |e: &str| {
        let edge = scene.edge(e.parse().unwrap()).unwrap();
        let (s, t) = umlcraft_notation::route::middle_segment(&edge.points, false);
        Point::new((s.x + t.x) / 2.0, (s.y + t.y) / 2.0)
    };
    for (own, other) in [(&there, &back), (&back, &there)] {
        let label = across(own, label_middle(own));
        let line = across(own, line_middle(other));
        assert!(label * line < 0.0, "{own}: label {label}, other line {line}");
    }
}

#[test]
fn states_grow_around_their_substates() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
    let d = id(&d["diagram"]);
    let make = |host: &mut Headless, name: &str, x: f32, y: f32| {
        let args = json!({ "diagram": d, "tool": "state", "name": name, "x": x, "y": y });
        run(host, "create_element", args)
    };
    let bounds = |host: &Headless, node: &Value| {
        let view = host.document().model().view(d.parse().unwrap()).unwrap();
        view.get(id(&node["presentation"]).parse().unwrap()).unwrap().bounds().unwrap()
    };
    // Resizes in place, so that nothing moves along.
    let resize = |host: &mut Headless, node: &Value, w: f32, h: f32| {
        let r = bounds(host, node);
        let args = json!({ "diagram": d, "node": node["presentation"], "x": r.x, "y": r.y,
                           "width": w, "height": h });
        run(host, "move_node", args);
    };
    let u = make(&mut host, "U", 20.0, 300.0);
    resize(&mut host, &u, 200.0, 100.0);
    let dd = make(&mut host, "D", 40.0, 340.0);
    let e = make(&mut host, "E", 600.0, 340.0);
    let args = json!({ "diagram": d, "tool": "transition", "source": e["element"],
                       "target": e["element"], "name": "e2 [z > 0] / z = z * 2" });
    run(&mut host, "connect", args);
    let lines = run(&mut host, "get_diagram", json!({ "diagram": d }))["lines"].clone();
    assert_eq!(lines[0]["label"], "e2 [z > 0] / z = z * 2");
    // Dropped half over the right border, E becomes a substate and U grows around it.
    let args = json!({ "diagram": d, "node": e["presentation"], "x": 150, "y": 340 });
    run(&mut host, "move_node", args);
    let model = host.document().model();
    let owner = model.get(id(&e["element"]).parse().unwrap()).unwrap().owner();
    assert_eq!(owner, Some(id(&u["element"]).parse().unwrap()));
    assert!(bounds(&host, &u).contains_rect(&bounds(&host, &e)), "{:?}", bounds(&host, &u));
    // So does the loop at E's top right corner, with room for its label.
    let corner = scene::self_loop(bounds(&host, &e))[1];
    assert!(bounds(&host, &u).contains(corner.offset(0.0, -scene::LABEL_HEIGHT)));
    // Shrunk below its substates, yet around their middles so that they stay in it, U grows
    // around them again when the diagram is arranged.
    let (r, de, ee) = (bounds(&host, &u), bounds(&host, &dd), bounds(&host, &e));
    let width = de.center().x.max(ee.center().x) - r.x + 10.0;
    let height = de.center().y.max(ee.center().y) - r.y + 10.0;
    resize(&mut host, &u, width, height);
    assert!(!bounds(&host, &u).contains_rect(&bounds(&host, &e)));
    let model = host.document().model();
    let owner = model.get(id(&e["element"]).parse().unwrap()).unwrap().owner();
    assert_eq!(owner, Some(id(&u["element"]).parse().unwrap()));
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    for inner in [&dd, &e] {
        assert!(
            bounds(&host, &u).contains_rect(&bounds(&host, inner)),
            "{:?} {:?}",
            bounds(&host, &u),
            bounds(&host, inner)
        );
    }
}

#[test]
fn a_loop_keeps_to_the_corner_of_its_state() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
    let d = id(&d["diagram"]);
    let args = json!({ "diagram": d, "tool": "state", "name": "C", "x": 100, "y": 100 });
    let state = run(&mut host, "create_element", args);
    let args = json!({ "diagram": d, "tool": "transition", "source": state["element"],
                       "target": state["element"], "name": "e2 / z = z * 2" });
    let line = run(&mut host, "connect", args);
    let at_corner = |host: &Headless| {
        let view = host.document().model().view(d.parse().unwrap()).unwrap();
        let r = view.get(id(&state["presentation"]).parse().unwrap()).unwrap().bounds().unwrap();
        let Shape::Edge { waypoints, .. } =
            &view.get(id(&line["presentation"]).parse().unwrap()).unwrap().shape
        else {
            panic!("not a line")
        };
        assert_eq!(waypoints, &scene::self_loop(r), "{r:?}");
        r
    };
    let first = at_corner(&host);
    // The state grows for a long activity: the loop stays at its corner.
    let args = json!({ "owner": state["element"], "text": "entry / x = x + 2 and a lot more" });
    run(&mut host, "add_member", args);
    let grown = at_corner(&host);
    assert!(grown.width > first.width, "{grown:?}");
    let args = json!({ "diagram": d, "node": state["presentation"], "x": 300, "y": 250 });
    run(&mut host, "move_node", args);
    at_corner(&host);
}

#[test]
fn loops_of_one_node_keep_to_different_corners() {
    let mut host = Headless::new();
    let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
    let args = json!({ "diagram": d, "tool": "class", "name": "Node", "x": 100, "y": 100 });
    let class = run(&mut host, "create_element", args);
    let lines: Vec<Value> = (0..2)
        .map(|_| {
            let args = json!({ "diagram": d, "tool": "association",
                               "source": class["element"], "target": class["element"] });
            run(&mut host, "connect", args)
        })
        .collect();
    let at_corners = |host: &Headless| {
        let view = host.document().model().view(d.parse().unwrap()).unwrap();
        let r = view.get(id(&class["presentation"]).parse().unwrap()).unwrap().bounds().unwrap();
        for (slot, line) in lines.iter().enumerate() {
            let Shape::Edge { waypoints, source_anchor, target_anchor, .. } =
                &view.get(id(&line["presentation"]).parse().unwrap()).unwrap().shape
            else {
                panic!("not a line")
            };
            assert_eq!(waypoints, &scene::self_loop_at(r, slot), "{slot} {r:?}");
            let (from, to) = scene::self_loop_anchors_at(r, slot);
            if source_anchor.is_some() {
                assert_eq!((*source_anchor, *target_anchor), (Some(from), Some(to)));
            }
        }
        r
    };
    let first = at_corners(&host);
    // The class grows for an attribute: each loop stays at its corner.
    run(&mut host, "add_member", json!({ "owner": class["element"], "text": "- value : int" }));
    assert!(at_corners(&host).height > first.height);
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    at_corners(&host);
}

#[test]
fn states_take_activities_and_internal_transitions() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
    let d = id(&d["diagram"]);
    let state = run(&mut host, "create_element", json!({ "diagram": d, "tool": "state" }));
    let state = id(&state["element"]);
    let add = |host: &mut Headless, text: &str| {
        run(host, "add_member", json!({ "owner": state, "text": text }))
    };
    add(&mut host, "entry / light on");
    add(&mut host, "DO / blink");
    add(&mut host, "exit / light off");
    let t = add(&mut host, "coin [credit < 2] / count");
    assert_eq!(t["kind"], "internalTransition");
    assert_eq!(t["text"], "coin [credit < 2] / count");
    let props = |host: &Headless| match host.document().model().get(state.parse().unwrap()) {
        Some(e) => match e.kind() {
            ElementKind::State(p) => p.clone(),
            _ => panic!(),
        },
        None => panic!(),
    };
    let p = props(&host);
    assert_eq!((p.entry.as_str(), p.do_activity.as_str()), ("light on", "blink"));
    assert_eq!(p.exit, "light off");
    // update_element changes and clears them, and rewrites the transition.
    run(&mut host, "update_element", json!({ "id": state, "entry": "", "do": "flash" }));
    let p = props(&host);
    assert_eq!((p.entry.as_str(), p.do_activity.as_str()), ("", "flash"));
    let t = run(&mut host, "update_element", json!({ "id": t["id"], "text": "reset / clear" }));
    let changed = host.document().model().get(id(&t["id"]).parse().unwrap()).unwrap();
    let ElementKind::InternalTransition(t) = changed.kind() else { panic!() };
    assert_eq!((t.trigger.as_str(), t.guard.as_str(), t.effect.as_str()), ("reset", "", "clear"));
    let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
    let class = run(
        &mut host,
        "create_element",
        json!({ "diagram": class["diagram"], "tool": "class", "name": "K" }),
    );
    let wrong = json!({ "id": class["element"], "entry": "x" });
    refusal(&mut host, "update_element", wrong);

    // A transition's name is read as `trigger [guard] / effect`, as the app shows it.
    let other = run(&mut host, "create_element", json!({ "diagram": d, "tool": "state" }));
    let args = json!({ "diagram": d, "tool": "transition", "source": state,
                       "target": other["element"], "name": "e8 [x > 0] / z++" });
    let line = run(&mut host, "connect", args);
    let transition = |host: &Headless| {
        let e = host.document().model().get(id(&line["element"]).parse().unwrap()).unwrap();
        let ElementKind::Transition(p) = e.kind() else { panic!() };
        (e.name().to_owned(), p.trigger.clone(), p.guard.clone(), p.effect.clone())
    };
    let parts = |a: &str, b: &str, c: &str| (String::new(), a.into(), b.into(), c.into());
    assert_eq!(transition(&host), parts("e8", "x > 0", "z++"));
    run(&mut host, "update_element", json!({ "id": line["element"], "text": "e9 / z--" }));
    assert_eq!(transition(&host), parts("e9", "", "z--"));
}

#[test]
fn submachine_states_points_and_stubs() {
    let mut host = Headless::new();
    let car = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "Car" }));
    let car = id(&car["diagram"]);
    let engine =
        run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "Engine" }));
    let engine = id(&engine["diagram"]);
    let make = |host: &mut Headless, tool: &str, extra: Value| {
        let mut args = json!({ "diagram": car, "tool": tool });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run(host, "create_element", args)
    };
    let drive = make(&mut host, "submachineState", json!({ "name": "Drive", "x": 200, "y": 100 }));
    let parked = make(&mut host, "state", json!({ "name": "Parked", "x": 40, "y": 100 }));
    let entry =
        make(&mut host, "entryPoint", json!({ "name": "start", "on": drive["presentation"] }));
    let exit = make(&mut host, "exitPoint", json!({ "name": "stop", "on": drive["presentation"] }));
    let stub = make(&mut host, "stubState", json!({ "name": "Halt", "on": drive["presentation"] }));
    // On a state too low for two points a side, they go to the middle of its sides, not
    // side by side along the top, where their names would run into each other.
    let busy = make(&mut host, "state", json!({ "name": "Busy", "x": 500, "y": 100 }));
    let resume =
        make(&mut host, "entryPoint", json!({ "name": "resume", "on": busy["presentation"] }));
    let abort =
        make(&mut host, "exitPoint", json!({ "name": "abort", "on": busy["presentation"] }));
    // Away from every state an entry point goes on the diagram's frame.
    let outer = make(&mut host, "entryPoint", json!({ "name": "in" }));
    // A stub state outside a submachine state is refused.
    let lone = call(
        &mut host,
        &Access::default(),
        "create_element",
        &json!({ "diagram": car, "tool": "stubState", "on": parked["presentation"] }),
    );
    assert!(lone.is_error);
    for (source, target) in [(&parked, &entry), (&exit, &parked), (&outer, &parked)] {
        run(
            &mut host,
            "connect",
            json!({ "diagram": car, "tool": "transition", "source": source["element"],
                    "target": target["element"] }),
        );
    }
    run(&mut host, "update_element", json!({ "id": drive["element"], "submachine": engine }));
    for wrong in [
        json!({ "id": parked["element"], "submachine": engine }),
        json!({ "id": drive["element"], "submachine": car }),
        json!({ "id": drive["element"], "submachine": parked["element"] }),
    ] {
        refusal(&mut host, "update_element", wrong);
    }
    let model = host.document().model();
    model.check().unwrap();
    let element = |value: &Value| model.element(parse_id(&id(&value["element"])).unwrap()).unwrap();
    let machine_of = |diagram: &str| model.element(parse_id(diagram).unwrap()).unwrap().owner();
    let ElementKind::State(p) = element(&drive).kind() else { panic!() };
    assert_eq!(p.submachine, machine_of(&engine));
    for (point, kind) in [
        (&entry, PseudostateKind::EntryPoint),
        (&exit, PseudostateKind::ExitPoint),
        (&stub, PseudostateKind::StubState),
    ] {
        let ElementKind::Pseudostate(p) = element(point).kind() else { panic!() };
        assert_eq!(p.pseudostate, kind);
        assert_eq!(element(point).owner(), Some(element(&drive).id()));
    }
    assert_eq!(element(&outer).owner(), machine_of(&car));
    // Points sit on the state's border, the exit point on the right; the stub inside.
    let view = model.view(parse_id(&car).unwrap()).unwrap();
    let bounds = |value: &Value| {
        view.get(parse_id(&id(&value["presentation"])).unwrap()).unwrap().bounds().unwrap()
    };
    let r = bounds(&drive);
    assert!((bounds(&entry).center().x - r.x).abs() < 0.5);
    assert!((bounds(&exit).center().x - r.right()).abs() < 0.5);
    assert!(r.contains_rect(&bounds(&stub)));
    let r = bounds(&busy);
    assert_eq!(bounds(&resume).center(), Point::new(r.x, r.center().y));
    assert_eq!(bounds(&abort).center(), Point::new(r.right(), r.center().y));
}

#[test]
fn simple_states_show_only_their_name_when_asked() {
    let mut host = Headless::new();
    // Whether the state's line is drawn, and how far the name's center lies from the
    // state's.
    let shown = |host: &mut Headless, d: &str, state: &Value| {
        let scene = drawn(host, d);
        let node = scene.node(id(&state["presentation"]).parse().unwrap()).unwrap();
        let scene::Figure::State { separator, .. } = node.figure else { panic!("a state") };
        (separator.is_some(), (node.rows[0].rect.center().y - node.rect.center().y).abs())
    };
    for simple in [false, true] {
        let args = json!({ "kind": "state_machine", "name": "S", "simple_states": simple });
        let d = id(&run(&mut host, "create_diagram", args)["diagram"]);
        let mut state = |name: &str| {
            let args = json!({ "diagram": d, "tool": "state", "name": name });
            run(&mut host, "create_element", args)
        };
        let (ready, running) = (state("bereit"), state("aktiv"));
        let args = json!({ "owner": running["element"], "text": "entry / starten" });
        run(&mut host, "add_member", args);
        let (line, off) = shown(&mut host, &d, &ready);
        assert_eq!(line, !simple, "simple_states {simple}");
        if simple {
            assert!(off < 0.5, "the name centered: {off}");
        }
        assert!(shown(&mut host, &d, &running).0, "a state with an activity keeps its line");
        // Switched afterwards on the diagram.
        run(&mut host, "update_element", json!({ "id": d, "simple_states": !simple }));
        assert_eq!(shown(&mut host, &d, &ready).0, simple);
    }
    let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
    let args = json!({ "id": class["diagram"], "simple_states": true });
    refusal(&mut host, "update_element", args);
}

#[test]
fn arranged_states_leave_room_for_the_labels_of_their_transitions() {
    // The phone lock of #42, and three states in a row.
    let mut host = Headless::new();
    let calls = json!([
        {"tool": "create_diagram", "arguments": {"kind": "state_machine", "name": "handy"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "initialPseudostate"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Gesperrt"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Entsperrt"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Blockiert"}},
        {"tool": "add_member", "arguments": {"owner": "$2.element", "text": "entry / Sperrbildschirm zeigen"}},
        {"tool": "add_member", "arguments": {"owner": "$2.element", "text": "pin(p) [p != code && fehler < 2] / fehler++"}},
        {"tool": "add_member", "arguments": {"owner": "$3.element", "text": "entry / Startbildschirm zeigen"}},
        {"tool": "add_member", "arguments": {"owner": "$4.element", "text": "entry / Hinweis 30 s warten"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$1.element", "target": "$2.element", "name": "/ fehler = 0"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$3.element", "name": "pin(p) [p == code] / fehler = 0"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$3.element", "target": "$2.element", "name": "sperren, after(1 min)"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$4.element", "name": "pin(p) [p != code && fehler == 2]"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$4.element", "target": "$2.element", "name": "after(30 s) / fehler = 0"}},
        {"tool": "arrange_diagram", "arguments": {"diagram": "$0.diagram"}}
    ]);
    let replies = run(&mut host, "batch", json!({ "calls": calls }));
    let d = id(&replies["results"][0]["diagram"]);
    let covered = covered_by_labels(&mut host, &d);
    assert!(covered.is_empty(), "{covered:#?}");

    let calls = json!([
        {"tool": "create_diagram", "arguments": {"kind": "state_machine", "name": "reihe"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Aus"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Bereit"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Druckt"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$1.element", "target": "$2.element", "name": "einschalten / selbsttest()"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$3.element", "name": "auftrag(a) [papier > 0] / drucken(a)"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$3.element", "target": "$2.element", "name": "fertig / auswerfen()"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$1.element", "name": "after(10 min) / abschalten()"}},
        {"tool": "arrange_diagram", "arguments": {"diagram": "$0.diagram"}}
    ]);
    let replies = run(&mut host, "batch", json!({ "calls": calls }));
    let d = id(&replies["results"][0]["diagram"]);
    let covered = covered_by_labels(&mut host, &d);
    assert!(covered.is_empty(), "{covered:#?}");
}

#[test]
fn arranged_transitions_run_around_the_states_between() {
    // A traffic light: the line from Gelb back to Rot passes Gruen, which a line from Gruen
    // to Gelb leaves beside it.
    let mut host = Headless::new();
    let calls = json!([
        {"tool": "create_diagram", "arguments": {"kind": "state_machine", "name": "ampel"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "initialPseudostate"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Rot"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "RotGelb"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Gruen"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "state", "name": "Gelb"}},
        {"tool": "create_element", "arguments": {"diagram": "$0.diagram", "tool": "finalState"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$1.element", "target": "$2.element"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$3.element", "name": "after(20 s)"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$3.element", "target": "$4.element", "name": "after(2 s)"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$4.element", "target": "$5.element", "name": "after(30 s) [!fussgaenger]"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$4.element", "target": "$5.element", "name": "taster / anfordern()"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$5.element", "target": "$2.element", "name": "after(3 s)"}},
        {"tool": "connect", "arguments": {"diagram": "$0.diagram", "tool": "transition", "source": "$2.element", "target": "$6.element", "name": "ausschalten"}},
        {"tool": "arrange_diagram", "arguments": {"diagram": "$0.diagram"}}
    ]);
    let replies = run(&mut host, "batch", json!({ "calls": calls }));
    let d = id(&replies["results"][0]["diagram"]);
    let scene = drawn(&mut host, &d);
    let mut through = Vec::new();
    for e in &scene.edges {
        for n in scene.nodes.iter().filter(|n| !e.ends.contains(&n.rect)) {
            let r = n.rect.inflate(-1.0);
            let hits = |a: Point, b: Point| {
                a.x.min(b.x) < r.right()
                    && a.x.max(b.x) > r.x
                    && a.y.min(b.y) < r.bottom()
                    && a.y.max(b.y) > r.y
            };
            if e.points.windows(2).any(|w| hits(w[0], w[1])) {
                through.push(format!("{:?} through {:?}", e.points, n.rect));
            }
        }
    }
    assert!(through.is_empty(), "{through:#?}");
    // Gelb grows so that the labels of its three lines fit beside them.
    let covered = covered_by_labels(&mut host, &d);
    assert!(covered.is_empty(), "{covered:#?}");
    // Nor do two lines run on top of each other.
    let horizontal = |e: &scene::EdgeShape| -> Vec<(f32, f32, f32)> {
        e.points
            .windows(2)
            .filter(|w| (w[0].y - w[1].y).abs() < 0.5)
            .map(|w| (w[0].y, w[0].x.min(w[1].x), w[0].x.max(w[1].x)))
            .collect()
    };
    for (i, a) in scene.edges.iter().enumerate() {
        for b in &scene.edges[i + 1..] {
            for (ya, a0, a1) in horizontal(a) {
                for (yb, b0, b1) in horizontal(b) {
                    let shared = a1.min(b1) - a0.max(b0);
                    assert!(
                        (ya - yb).abs() >= 4.0 || shared <= 1.0,
                        "{:?} on {:?}",
                        a.points,
                        b.points
                    );
                }
            }
        }
    }
}

#[test]
fn a_state_is_divided_into_regions_that_take_substates() {
    // An air conditioner that, while on, is in a fan level and a mode at the same time.
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "K" }));
    let d = id(&d["diagram"]);
    let args = json!({ "diagram": d, "tool": "state", "name": "Ein", "x": 100, "y": 20 });
    let on = run(&mut host, "create_element", args);
    let args = json!({ "diagram": d, "node": on["presentation"], "x": 100, "y": 20,
                       "width": 400, "height": 200 });
    run(&mut host, "move_node", args);
    let args = json!({ "id": on["element"], "regions": 2 });
    let divided = run(&mut host, "update_element", args);
    let regions: Vec<String> = divided["regions"].as_array().unwrap().iter().map(id).collect();
    assert_eq!(regions.len(), 2, "{divided}");
    // get_diagram tells where each region lies.
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let node = diagram["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["element"] == on["element"])
        .unwrap();
    let shown = node["regions"].as_array().unwrap();
    assert_eq!(shown.iter().map(|r| id(&r["id"])).collect::<Vec<_>>(), regions);
    // One below the other: a state put into each goes into that region.
    for (k, (region, name)) in shown.iter().zip(["Stufe 1", "Kühlen"]).enumerate() {
        let b = &region["bounds"];
        let (x, y) = (b["x"].as_f64().unwrap() + 20.0, b["y"].as_f64().unwrap() + 30.0);
        let args = json!({ "diagram": d, "tool": "state", "name": name, "x": x, "y": y });
        let made = run(&mut host, "create_element", args);
        let element = run(&mut host, "get_element", json!({ "id": made["element"] }));
        assert_eq!(element["owner"], region["id"], "{k}: {element}");
    }
    // A third region, and refusals.
    let args = json!({ "id": on["element"], "regions": 3 });
    let three = run(&mut host, "update_element", args);
    assert_eq!(three["regions"].as_array().unwrap().len(), 3);
    for wrong in [
        json!({ "id": on["element"], "regions": 2 }),
        json!({ "id": on["element"], "regions": 1 }),
        json!({ "id": on["element"], "side_by_side": true }),
        json!({ "id": d, "regions": 2 }),
    ] {
        refusal(&mut host, "update_element", wrong);
    }
    host.document().model().check().unwrap();
}

#[test]
fn regions_side_by_side_stand_next_to_each_other() {
    let mut host = Headless::new();
    let host = &mut host;
    let d = run(host, "create_diagram", json!({ "kind": "state_machine", "name": "V" }));
    let d = id(&d["diagram"]);
    let args = json!({ "diagram": d, "tool": "state", "name": "Ein", "x": 40, "y": 20 });
    let on = run(host, "create_element", args);
    let args = json!({ "id": on["element"], "regions": 2, "side_by_side": true });
    run(host, "update_element", args);
    let diagram = run(host, "get_diagram", json!({ "diagram": d }));
    let node = diagram["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["element"] == on["element"])
        .unwrap()
        .clone();
    let r = node["regions"].as_array().unwrap();
    let (a, b) = (&r[0]["bounds"], &r[1]["bounds"]);
    assert!(a["x"].as_f64() < b["x"].as_f64(), "{node}");
    assert!((a["y"].as_f64().unwrap() - b["y"].as_f64().unwrap()).abs() < 0.5, "{node}");
}
