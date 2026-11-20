//! Sequence diagrams: lifelines, messages, bars and fragments.

use super::*;

#[test]
fn a_sequence_diagram_gets_lifelines_and_messages() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Order" }));
    let d = id(&d["diagram"]);
    let a = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "lifeline", "name": "shop" }),
    );
    let b = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "lifeline", "name": "bank" }),
    );
    let call_ = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "message", "source": a["element"], "target": b["element"] }),
    );
    assert_eq!(call_["kind"], "message");
    let reply = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "reply", "source": b["element"], "target": a["element"] }),
    );
    host.document().model().check().unwrap();

    // Message settings.
    let uid = |v: &Value| parse_id(v.as_str().unwrap()).unwrap();
    let pay = call_["element"].clone();
    let update = json!({ "id": pay, "name": "pay", "arguments": "total", "guard": "[open]",
                         "sort": "asynchronous" });
    run(&mut host, "update_element", update);
    run(&mut host, "update_element", json!({ "id": reply["element"], "return_value": "ok" }));
    let message = |host: &Headless, id: &Value| match host.document().model().get(uid(id)) {
        Some(e) => match e.kind() {
            ElementKind::Message(p) => p.clone(),
            _ => panic!("no message"),
        },
        None => panic!("gone"),
    };
    let p = message(&host, &pay);
    assert_eq!(
        (p.sort, p.arguments.as_str(), p.guard.as_str()),
        (MessageSort::Asynchronous, "total", "open")
    );
    assert_eq!(message(&host, &reply["element"]).return_value, "ok");
    let lifeline = json!({ "id": a["element"], "guard": "x" });
    assert!(call(&mut host, &Access::default(), "update_element", &lifeline).is_error);
    let bogus = json!({ "id": pay, "sort": "sometimes" });
    assert!(call(&mut host, &Access::default(), "update_element", &bogus).is_error);

    // Fragments and their operands.
    let guards = |host: &Headless, fragment: Id| -> Vec<String> {
        let model = host.document().model();
        model
            .children(fragment)
            .iter()
            .filter_map(|&o| match model.get(o).map(|e| e.kind()) {
                Some(ElementKind::InteractionOperand(p)) => Some(p.guard.clone()),
                _ => None,
            })
            .collect()
    };
    let wrap = json!({ "diagram": d, "items": [pay, reply["element"]], "operator": "alt",
                       "guard": "paid" });
    let alt = run(&mut host, "wrap_in_fragment", wrap);
    let fragment = uid(&alt["id"]);
    assert_eq!(alt["operands"].as_array().map(Vec::len), Some(2));
    assert_eq!(guards(&host, fragment), ["paid", "else"]);
    let late = run(&mut host, "add_operand", json!({ "fragment": fragment, "guard": "[late]" }));
    assert_eq!(guards(&host, fragment), ["paid", "else", "late"]);
    run(&mut host, "delete_element", json!({ "id": late["id"] }));
    assert_eq!(guards(&host, fragment), ["paid", "else"]);
    run(&mut host, "update_element", json!({ "id": fragment, "operator": "loop" }));
    assert!(matches!(
        host.document().model().get(fragment).map(|e| e.kind()),
        Some(ElementKind::CombinedFragment(p)) if p.operator == InteractionOperator::Loop
    ));
    let pay_owner = host.document().model().get(uid(&pay)).and_then(|e| e.owner());
    assert_eq!(pay_owner, host.document().model().children(fragment).first().copied());
    host.document().model().check().unwrap();
}

#[test]
fn appended_messages_continue_the_call_their_sender_is_busy_with() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Order" }));
    let d = d["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
    };
    let (a, b, c) = (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
    let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str| {
        let m = json!({ "diagram": d, "tool": tool, "source": from, "target": to,
                        "name": name });
        run(host, "connect", m)
    };
    send(&mut host, "message", &a["element"], &b["element"], "order");
    let check = send(&mut host, "message", &b["element"], &b["element"], "check");
    send(&mut host, "message", &b["element"], &c["element"], "pay");
    send(&mut host, "reply", &c["element"], &b["element"], "");
    send(&mut host, "message", &b["element"], &a["element"], "done");
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let y = |name: &str| {
        let lines = diagram["lines"].as_array().unwrap();
        lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
    };
    assert_eq!(diagram["lines"][1]["name"], "check");
    // b works in one bar from the call it gets to the call it makes last.
    let outer = diagram["bars"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|bar| bar["lifeline"] == b["element"])
        .map(|bar| (bar["top"].as_f64().unwrap(), bar["bottom"].as_f64().unwrap()))
        .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
        .unwrap();
    assert!((outer.0 - y("order")).abs() < 0.5, "{outer:?}");
    assert!(outer.1 > y("done"), "{outer:?}");

    // A loop around the call to self stays above the call that follows.
    let wrap = json!({ "diagram": d, "items": [check["element"]], "operator": "loop" });
    let lp = run(&mut host, "wrap_in_fragment", wrap);
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let frame = diagram["nodes"].as_array().unwrap().iter().find(|n| n["element"] == lp["id"]);
    let bounds = &frame.unwrap()["bounds"];
    let bottom = bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap();
    let pay = diagram["lines"].as_array().unwrap().iter().find(|l| l["name"] == "pay");
    assert!(bottom < pay.unwrap()["y"].as_f64().unwrap());

    // An alt around the next call stays beside the loop, not inside it.
    let pay = pay.unwrap()["element"].clone();
    let alt = run(
        &mut host,
        "wrap_in_fragment",
        json!({ "diagram": d, "items": [pay], "operator": "alt" }),
    );
    let model = host.document().model();
    let owner =
        |id: &Value| model.get(parse_id(id.as_str().unwrap()).unwrap()).and_then(|e| e.owner());
    assert_eq!(owner(&alt["id"]), owner(&lp["id"]));
    model.check().unwrap();
    // Its else pushes the call after it down instead of taking it in.
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let frame = diagram["nodes"].as_array().unwrap().iter().find(|n| n["element"] == alt["id"]);
    let bounds = &frame.unwrap()["bounds"];
    let bottom = bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap();
    let done = diagram["lines"].as_array().unwrap().iter().find(|l| l["name"] == "done");
    assert!(bottom < done.unwrap()["y"].as_f64().unwrap(), "{bottom}");
}

#[test]
fn a_reply_goes_where_y_says() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Calc" }));
    let d = d["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
    };
    let (a, b, c) = (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
    let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str, y| {
        let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                        "target": to["element"], "name": name, "y": y });
        run(host, "connect", m)
    };
    send(&mut host, "message", &a, &b, "rechne", 100);
    send(&mut host, "message", &b, &c, "hole", 140);
    send(&mut host, "reply", &c, &b, "x", 170);
    send(&mut host, "reply", &b, &a, "summe", 300);
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let y = |name: &str| {
        let lines = diagram["lines"].as_array().unwrap();
        lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
    };
    assert_eq!((y("x"), y("summe")), (170.0, 300.0), "{diagram}");
    // b's bar runs down to the reply it gives; a waits for it.
    let bottom = |lifeline: &Value| {
        diagram["bars"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|bar| bar["lifeline"] == lifeline["element"])
            .map(|bar| bar["bottom"].as_f64().unwrap())
            .fold(0.0, f64::max)
    };
    assert!((bottom(&b) - 300.0).abs() < 0.5, "{}", bottom(&b));
    assert!(bottom(&a) >= 300.0, "{}", bottom(&a));
    host.document().model().check().unwrap();
}

#[test]
fn nested_fragments_are_inset_and_keep_to_their_messages() {
    // The inner fragment made first and wrapped, or the outer one first.
    for outer_first in [false, true] {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "C" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            let args = json!({ "diagram": d, "tool": "lifeline", "name": name });
            run(host, "create_element", args)
        };
        let (a, b, c) =
            (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
        let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str| {
            let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                            "target": to["element"], "name": name });
            run(host, "connect", m)
        };
        send(&mut host, "message", &a, &b, "rechne");
        let hole = send(&mut host, "message", &b, &c, "hole");
        let x = send(&mut host, "reply", &c, &b, "x");
        send(&mut host, "reply", &b, &a, "summe");
        let wrap = |host: &mut Headless, items: Value, operator: &str| {
            let args = json!({ "diagram": d, "items": items, "operator": operator });
            run(host, "wrap_in_fragment", args)
        };
        let messages = json!([hole["element"], x["element"]]);
        let (opt, lp) = if outer_first {
            let lp = wrap(&mut host, messages.clone(), "loop");
            (wrap(&mut host, messages, "opt"), lp)
        } else {
            let opt = wrap(&mut host, messages, "opt");
            let lp = wrap(&mut host, json!([opt["id"]]), "loop");
            (opt, lp)
        };
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let frame = |f: &Value| {
            let nodes = diagram["nodes"].as_array().unwrap();
            let b = &nodes.iter().find(|n| n["element"] == f["id"]).unwrap()["bounds"];
            let n = |k: &str| b[k].as_f64().unwrap();
            (n("x"), n("y"), n("x") + n("width"), n("y") + n("height"))
        };
        let y = |name: &str| {
            let lines = diagram["lines"].as_array().unwrap();
            lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
        };
        let (inner, outer) = (frame(&opt), frame(&lp));
        // The loop goes around the opt with room on every side, its label above the opt's.
        assert!(outer.0 < inner.0 - 5.0 && outer.2 > inner.2 + 5.0, "{outer:?} {inner:?}");
        assert!(outer.1 < inner.1 - 15.0 && outer.3 > inner.3 + 5.0, "{outer:?} {inner:?}");
        // The call before and the reply after stay outside.
        assert!(y("rechne") < outer.1 && y("summe") > outer.3, "{diagram}");
        host.document().model().check().unwrap();
    }
}

#[test]
fn a_created_head_follows_its_message_into_fragments() {
    let mut host = Headless::new();
    let args = json!({ "kind": "sequence", "name": "Ausleihen", "message_numbers": false,
                       "frame": false });
    let d = run(&mut host, "create_diagram", args)["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        let args = json!({ "diagram": d, "tool": "lifeline", "name": name });
        run(host, "create_element", args)
    };
    let (main, t, g) = (
        lifeline(&mut host, "main"),
        lifeline(&mut host, "t : Theke"),
        lifeline(&mut host, "g : Geraet"),
    );
    let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str, y: f32| {
        let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                        "target": to["element"], "name": name, "y": y });
        run(host, "connect", m)
    };
    send(&mut host, "message", &main, &t, "ausleihen(g, \"Eva\")", 120.0);
    let free = send(&mut host, "message", &t, &g, "istFrei()", 150.0);
    let yes = send(&mut host, "reply", &g, &t, "frei", 175.0);
    let lend = send(&mut host, "message", &t, &g, "verleihen()", 220.0);
    let back = send(&mut host, "reply", &t, &main, "true", 300.0);
    let a = lifeline(&mut host, "a : Ausleihe");
    let create = send(&mut host, "createMessage", &t, &a, "Ausleihe(g, \"Eva\")", 260.0);
    let wrap = |host: &mut Headless, items: Value, operator: &str| {
        let args = json!({ "diagram": d, "items": items, "operator": operator });
        run(host, "wrap_in_fragment", args)
    };
    let opt = wrap(&mut host, json!([lend["element"], create["element"]]), "opt");
    let lp = wrap(&mut host, json!([free["element"], yes["element"], opt["id"]]), "loop");

    let model = host.document().model();
    let view = model.view(id(&d).parse().unwrap()).unwrap();
    let p = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap();
    let Shape::Lifeline { head, .. } = p(&a).shape else { panic!("no lifeline") };
    let Shape::Message { y, .. } = p(&create).shape else { panic!("no message") };
    // The message still ends at the middle of the head.
    assert!((head.center().y - y).abs() < 1.0, "{head:?} {y}");
    // The reply after the loop stays below both frames.
    let Shape::Message { y: after, .. } = p(&back).shape else { panic!("no message") };
    for f in [&opt, &lp] {
        let frame = view.presentations_of(id(&f["id"]).parse().unwrap()).next().unwrap();
        let bottom = frame.bounds().unwrap().bottom();
        assert!(bottom < after, "{bottom} {after}");
    }
    model.check().unwrap();
}

#[test]
fn a_call_drawn_further_down_belongs_to_the_activation_above() {
    let cases: [&[Step]; 2] = [
        &[("message", 0, 1, "call", 100.0), ("message", 1, 2, "x", 180.0)],
        &[
            ("message", 0, 1, "call", 100.0),
            ("message", 1, 2, "q", 130.0),
            ("reply", 2, 1, "q", 160.0),
            ("message", 1, 2, "x", 230.0),
        ],
    ];
    for (k, steps) in cases.iter().enumerate() {
        let mut host = Headless::new();
        let mut steps = steps.to_vec();
        steps.push(("reply", 1, 0, "r", 300.0));
        let (d, messages) = sequence_script(&mut host, &steps);
        let x = &messages[messages.len() - 2];
        let r = &messages[messages.len() - 1];
        // One bar on t, from the call down to its reply: `x` is sent within it.
        let bars = bars_of(&host, &d, 1);
        let (rx, _) = message_y(&host, &d, r);
        assert_eq!(bars.len(), 1, "{k}: {bars:?}");
        assert!(bars[0].0 <= 100.5 && bars[0].1 >= rx - 0.5, "{k}: {bars:?} {rx}");
        assert!((rx - 300.0).abs() < 0.5, "{k}: {rx}");
        // A fragment around `x` leaves the reply outside.
        let args = json!({ "diagram": d, "items": [x["element"]], "operator": "alt" });
        let alt = run(&mut host, "wrap_in_fragment", args);
        let frame = frame_of(&host, &d, &alt);
        let (rx, _) = message_y(&host, &d, r);
        assert!(frame.bottom() < rx, "{k}: {frame:?} {rx}");
        assert_eq!(bars_of(&host, &d, 1).len(), 1, "{k}");
        host.document().model().check().unwrap();
    }
}

#[test]
fn fragments_keep_below_loops_to_self() {
    let cases: [&[Step]; 2] = [
        &[
            ("message", 0, 1, "call", 100.0),
            ("message", 1, 1, "suchen(8)", 140.0),
            ("reply", 1, 1, "g", 170.0),
            ("message", 1, 2, "x", 230.0),
        ],
        &[
            ("message", 0, 1, "call", 100.0),
            ("message", 1, 1, "ok = auszahlen(300)", 140.0),
            ("message", 1, 2, "x", 200.0),
        ],
    ];
    for (k, steps) in cases.iter().enumerate() {
        let mut host = Headless::new();
        let mut steps = steps.to_vec();
        steps.push(("reply", 1, 0, "r", 300.0));
        let (d, messages) = sequence_script(&mut host, &steps);
        let x = &messages[messages.len() - 2];
        let args = json!({ "diagram": d, "items": [x["element"]], "operator": "alt" });
        let alt = run(&mut host, "wrap_in_fragment", args);
        let frame = frame_of(&host, &d, &alt);
        // Below the lowest loop to self and the bars on t above the frame.
        let lowest = messages[1..messages.len() - 2]
            .iter()
            .map(|m| message_y(&host, &d, m))
            .map(|(y, end)| end.max(y + umlcraft_notation::sequence::SELF_DROP))
            .fold(f32::MIN, f32::max);
        let inner = bars_of(&host, &d, 1)
            .into_iter()
            .filter(|&(top, _)| top > 100.5 && top < frame.y)
            .map(|(_, bottom)| bottom)
            .fold(f32::MIN, f32::max);
        assert!(frame.y > lowest && frame.y > inner, "{k}: {frame:?} {lowest} {inner}");
        // The bar the loop leaves runs beside its label, not through it.
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let edge = scene
            .edges
            .iter()
            .find(|e| e.labels.iter().any(|l| l.text.contains(steps[1].3)))
            .unwrap();
        let label = edge.labels.iter().find(|l| l.text.contains(steps[1].3)).unwrap();
        let width = scene::label_width(&label.text, measure);
        assert!(label.anchor.x - width / 2.0 >= edge.points[0].x, "{k}: {label:?} {edge:?}");
        host.document().model().check().unwrap();
    }
}

#[test]
fn a_fragment_around_a_create_message_keeps_what_follows_below_it() {
    // The head the create message brings down and the bar below it belong to the message,
    // so what follows keeps the same distance to the frame as after an ordinary call.
    let gap = |tool: &'static str| {
        let mut host = Headless::new();
        let steps: Vec<Step> = vec![
            ("message", 0, 1, "ausleihen", 100.0),
            (tool, 1, 2, "Ausleihe", 180.0),
            ("reply", 1, 0, "true", 230.0),
        ];
        let (d, messages) = sequence_script(&mut host, &steps);
        let args = json!({ "diagram": d, "items": [messages[1]["element"]], "operator": "opt" });
        let opt = run(&mut host, "wrap_in_fragment", args);
        let frame = frame_of(&host, &d, &opt);
        let (reply, _) = message_y(&host, &d, &messages[2]);
        host.document().model().check().unwrap();
        reply - frame.bottom()
    };
    let (call, create) = (gap("message"), gap("createMessage"));
    assert!(call >= 30.0, "{call}");
    assert!((create - call).abs() < 0.5, "create {create}, call {call}");
}

#[test]
fn a_reply_to_self_leaves_the_bar_its_call_started() {
    let mut host = Headless::new();
    let steps: Vec<Step> = vec![
        ("message", 0, 1, "call", 100.0),
        ("message", 1, 1, "suchen(8)", 140.0),
        ("reply", 1, 1, "g", 170.0),
        ("message", 1, 2, "x", 230.0),
        ("reply", 1, 0, "r", 300.0),
    ];
    let (d, messages) = sequence_script(&mut host, &steps);
    // The inner bar keeps the height a call gives it, and the reply leaves at its end.
    let (_, arrived) = message_y(&host, &d, &messages[1]);
    let (g, _) = message_y(&host, &d, &messages[2]);
    let inner = bars_of(&host, &d, 1).into_iter().find(|&(top, _)| (top - arrived).abs() < 0.5);
    let (top, bottom) = inner.expect("a bar for the call to self");
    assert!(bottom - top >= sequence_edit::RECEIVER_BAR - 0.01, "{top} {bottom}");
    assert!((g - bottom).abs() < 0.5, "{g} {bottom}");
    // From the inner bar's side back into the outer one.
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    let points = |text: &str| {
        let e = scene.edges.iter().find(|e| e.labels.iter().any(|l| l.text == text)).unwrap();
        e.points.clone()
    };
    let (call, reply) = (points("1.1: suchen(8)"), points("g"));
    assert!((reply[0].x - call[3].x).abs() < 0.5, "{reply:?} {call:?}");
    assert!((reply[3].x - call[0].x).abs() < 0.5, "{reply:?} {call:?}");
    host.document().model().check().unwrap();
}

#[test]
fn sequence_diagrams_go_without_numbers_and_frame() {
    let mut host = Headless::new();
    let args = json!({ "kind": "sequence", "name": "Calc", "message_numbers": false,
                       "frame": false });
    let d = id(&run(&mut host, "create_diagram", args)["diagram"]);
    let props = |host: &Headless| match host.document().model().get(d.parse().unwrap()) {
        Some(e) => match e.kind() {
            ElementKind::Diagram(p) => *p,
            _ => panic!("no diagram"),
        },
        None => panic!("gone"),
    };
    let p = props(&host);
    assert!(p.hide_message_numbers && p.hide_frame && !p.hide_return_values);
    let args = json!({ "id": d, "message_numbers": true, "return_values": false });
    run(&mut host, "update_element", args);
    let p = props(&host);
    assert!(!p.hide_message_numbers && p.hide_frame && p.hide_return_values);
    let class = json!({ "kind": "class", "name": "C", "frame": false });
    assert!(call(&mut host, &Access::default(), "create_diagram", &class).is_error);
}

#[test]
fn a_call_named_with_parentheses_takes_them_as_arguments() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Calc" }));
    let d = d["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
    };
    let (a, b) = (lifeline(&mut host, "a"), lifeline(&mut host, "b"));
    let send = |host: &mut Headless, name: &str| {
        let m = json!({ "diagram": d, "tool": "message", "source": a["element"],
                        "target": b["element"], "name": name });
        run(host, "connect", m)
    };
    let label = |host: &Headless, m: &Value| {
        let id = parse_id(m["element"].as_str().unwrap()).unwrap();
        umlcraft_notation::sequence::message_text(host.document().model(), id)
    };
    let rechne = send(&mut host, "rechne()");
    assert_eq!(label(&host, &rechne), "rechne()");
    let leihe = send(&mut host, r#"ausleihen(g, "Ada")"#);
    assert_eq!(label(&host, &leihe), r#"ausleihen(g, "Ada")"#);
    let update = json!({ "id": rechne["element"], "name": "summe(a, b)" });
    run(&mut host, "update_element", update);
    assert_eq!(label(&host, &rechne), "summe(a, b)");
}

#[test]
fn messages_move_between_operands() {
    let mut host = Headless::new();
    let uid = |v: &Value| parse_id(v.as_str().unwrap()).unwrap();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Pay" }));
    let d = d["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
    };
    let (a, b, c) = (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
    let found = json!({ "diagram": d, "tool": "foundMessage", "source": a["element"] });
    let found = run(&mut host, "connect", found);
    let lost = json!({ "diagram": d, "tool": "lostMessage", "source": b["element"] });
    run(&mut host, "connect", lost);
    let gates: Vec<_> = host
        .document()
        .model()
        .elements()
        .filter_map(|e| match e.kind() {
            ElementKind::Message(p) => Some(p.gate),
            _ => None,
        })
        .collect();
    assert_eq!(gates, [umlcraft_model::MessageGate::Found, umlcraft_model::MessageGate::Lost]);
    let missing = json!({ "diagram": d, "tool": "message", "source": a["element"] });
    assert!(call(&mut host, &Access::default(), "connect", &missing).is_error);

    let send = |host: &mut Headless, from: &Value, to: &Value| {
        let m = json!({ "diagram": d, "tool": "message", "source": from, "target": to });
        run(host, "connect", m)
    };
    let first = send(&mut host, &a["element"], &b["element"]);
    let second = send(&mut host, &a["element"], &b["element"]);
    let wrap = json!({ "diagram": d, "items": [first["element"]], "operator": "alt" });
    let alt = run(&mut host, "wrap_in_fragment", wrap);

    // Covering c as well widens the frame to it.
    let frame = |host: &mut Headless| {
        let diagram = run(host, "get_diagram", json!({ "diagram": d }));
        let nodes = diagram["nodes"].as_array().unwrap().clone();
        nodes.into_iter().find(|n| n["element"] == alt["id"]).unwrap()
    };
    let narrow = frame(&mut host)["bounds"]["width"].as_f64().unwrap();
    let cover = json!({ "id": alt["id"],
                        "covered": [a["element"], b["element"], c["presentation"]] });
    run(&mut host, "update_element", cover);
    let wide = frame(&mut host);
    assert!(wide["bounds"]["width"].as_f64().unwrap() > narrow);
    let covered = match host.document().model().get(uid(&alt["id"])).map(|e| e.kind()) {
        Some(ElementKind::CombinedFragment(p)) => p.covered.clone(),
        _ => panic!("no fragment"),
    };
    assert_eq!(covered, [uid(&a["element"]), uid(&b["element"]), uid(&c["element"])]);

    // The second message into the else.
    let operands = wide["operands"].as_array().unwrap();
    assert_eq!(operands.len(), 2);
    let otherwise = &operands[1];
    let y = (otherwise["top"].as_f64().unwrap() + otherwise["bottom"].as_f64().unwrap()) / 2.0;
    run(&mut host, "move_node", json!({ "diagram": d, "node": second["element"], "y": y }));
    let owner = host.document().model().get(uid(&second["element"])).and_then(|e| e.owner());
    assert_eq!(owner, Some(uid(&otherwise["id"])));
    let _ = found;
    host.document().model().check().unwrap();
}

#[test]
fn state_invariants_terminations_and_constraints() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Timing" }));
    let d = d["diagram"].clone();
    let lifeline = |host: &mut Headless, name: &str| {
        run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
    };
    let (shop, store) = (lifeline(&mut host, "shop"), lifeline(&mut host, "store"));
    let send = |host: &mut Headless, name: &str| {
        let m = json!({ "diagram": d, "tool": "asyncMessage", "source": shop["element"],
                        "target": store["element"], "name": name });
        run(host, "connect", m)
    };
    let (check, save) = (send(&mut host, "check"), send(&mut host, "save"));
    let invariant = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "stateInvariant", "name": "x > 0",
                "on": store["element"] }),
    );
    assert_eq!(invariant["kind"], "stateInvariant");
    assert_eq!(invariant["name"], "x > 0");
    // A longer condition wraps, and its box grows to hold it.
    let waiting = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "stateInvariant", "name": "waiting for the store",
                "on": shop["element"], "y": 200 }),
    );
    run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "termination", "on": store["presentation"], "y": 60 }),
    );
    // A lifeline ends once, and off a lifeline nothing goes.
    let again = json!({ "diagram": d, "tool": "termination", "on": store["element"] });
    assert!(call(&mut host, &Access::default(), "create_element", &again).is_error);
    let nowhere = json!({ "diagram": d, "tool": "stateInvariant" });
    assert!(call(&mut host, &Access::default(), "create_element", &nowhere).is_error);
    let duration = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "durationConstraint", "source": check["element"],
                "target": save["element"], "name": "d..3*d" }),
    );
    assert_eq!(duration["kind"], "durationConstraint");
    let time = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "timeConstraint", "source": save["element"] }),
    );
    run(&mut host, "update_element", json!({ "id": time["element"], "name": "t..t+3" }));
    // Constraints go on messages only.
    let wrong = json!({ "diagram": d, "tool": "timeConstraint", "source": shop["element"] });
    assert!(call(&mut host, &Access::default(), "connect", &wrong).is_error);
    host.document().model().check().unwrap();

    let shown = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let tools = shown["element_tools"].as_array().unwrap();
    assert!(tools.contains(&json!("stateInvariant")) && tools.contains(&json!("termination")));
    let tools = shown["relationship_tools"].as_array().unwrap();
    assert!(tools.contains(&json!("durationConstraint")));
    let node = |kind: &str| -> Value {
        let nodes = shown["nodes"].as_array().unwrap();
        nodes.iter().find(|n| n["kind"] == kind).cloned().unwrap()
    };
    assert_eq!(node("stateInvariant")["lifeline"], store["element"]);
    assert_eq!(node("termination")["lifeline"], store["element"]);
    assert_eq!(node("durationConstraint")["messages"], json!([check["element"], save["element"]]));
    assert_eq!(node("timeConstraint")["messages"], json!([save["element"]]));
    assert_eq!(node("timeConstraint")["name"], "t..t+3");
    let of = |element: &Value| -> Value {
        let nodes = shown["nodes"].as_array().unwrap();
        nodes.iter().find(|n| n["element"] == *element).cloned().unwrap()
    };
    let num = |v: &Value, key: &str| v["bounds"][key].as_f64().unwrap();
    let tall = of(&waiting["element"]);
    assert!(num(&tall, "height") >= 21.0 + 15.0, "{tall}");
    // Three lines (by the estimated widths): 15.09 each and 5.
    assert_eq!(num(&tall, "height"), 3.0 * 15.093_75 + 5.0);
    assert_eq!(num(&node("stateInvariant"), "height"), 21.0);
    // The frame holds the time constraint's label: it starts where the line ends, 30 on
    // from the arrowhead (the node's left is 5 before it).
    let frame = of(&d);
    let text = umlcraft_notation::metrics::java_width("{t..t+3}", 12.0, false).unwrap();
    let label_end = num(&node("timeConstraint"), "x") + 5.0 + 30.0 + f64::from(text);
    assert!(num(&frame, "x") + num(&frame, "width") >= label_end + 10.0, "{frame}");
    // The cross ends the lifeline below everything on it, the invariant included; the
    // duration's line reaches from one message to the other.
    let bottom =
        |v: &Value| v["bounds"]["y"].as_f64().unwrap() + v["bounds"]["height"].as_f64().unwrap();
    let cross = &node("termination")["bounds"];
    let cross_y = cross["y"].as_f64().unwrap() + 10.0;
    assert!(cross_y > bottom(&node("stateInvariant")), "{cross}");
    let line_y = |m: &Value| {
        let lines = shown["lines"].as_array().unwrap();
        lines.iter().find(|l| l["element"] == m["element"]).unwrap()["y"].as_f64().unwrap()
    };
    let span = &node("durationConstraint")["bounds"];
    assert!((span["y"].as_f64().unwrap() - line_y(&check)).abs() < 1.0, "{span}");
    assert!(bottom(&node("durationConstraint")) >= line_y(&save) - 1.0, "{span}");
    // Deleting a message takes its constraints along.
    run(&mut host, "delete_element", json!({ "id": save["element"] }));
    let model = host.document().model();
    model.check().unwrap();
    let left = model.elements().filter(|e| {
        matches!(e.kind(), ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_))
    });
    assert_eq!(left.count(), 0);
}

#[test]
fn heads_of_lifelines_grow_with_their_names() {
    let mut host = Headless::new();
    let calls = lifelines(&["e:EchoClientVerbindung", "k"]);
    let r = build(&mut host, json!(calls));
    let scene = drawn(&mut host, &r[0]["diagram"]);
    // As wide as the name with room on either side, around the line where it was; a short
    // name keeps the width Astah gives a head (#59).
    for (reply, at_least) in [(&r[1], 150.0), (&r[2], sequence::HEAD_WIDTH)] {
        let node = scene.node(id(&reply["presentation"]).parse().unwrap()).unwrap();
        let Figure::Lifeline { head, .. } = node.figure else { panic!("a lifeline") };
        let name = node.rows[0].rect;
        assert!(head.width >= at_least, "{head:?}");
        assert!(head.x + 8.0 <= name.x && name.right() + 8.0 <= head.right(), "{head:?}");
    }
}

#[test]
fn lifelines_end_together_below_the_last_message() {
    let mut host = Headless::new();
    let mut calls = lifelines(&[":Program", "t:Theke", "g:Geraet"]);
    calls.push(message("message", 1, 2, "ausleihen()", Some(100.0)));
    calls.push(message("message", 2, 3, "istFrei()", Some(130.0)));
    calls.push(message("reply", 3, 2, "frei", Some(160.0)));
    calls.push(message("reply", 2, 1, "frei", Some(450.0)));
    let r = build(&mut host, json!(calls));
    // All three as long, also the one the last message does not reach (#56).
    let g = run(&mut host, "get_diagram", json!({ "diagram": r[0]["diagram"] }));
    let bottoms: Vec<f64> = g["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["name"].as_str().is_some_and(|n| n.contains(':')))
        .map(|n| n["bounds"]["y"].as_f64().unwrap() + n["bounds"]["height"].as_f64().unwrap())
        .collect();
    assert_eq!(bottoms.len(), 3);
    assert!(bottoms.iter().all(|&b| (b - bottoms[0]).abs() < 0.01 && b > 450.0), "{bottoms:?}");
}

#[test]
fn a_reply_without_height_goes_below_a_fragment_it_is_not_in() {
    let mut host = Headless::new();
    let mut calls = lifelines(&["b:Betrieb", "a:Auftrag", "e:Mitarbeiter"]);
    calls.push(message("message", 1, 2, "finde()", Some(100.0)));
    calls.push(message("message", 2, 3, "neuerAuftrag(a)", Some(170.0)));
    calls.push(json!({ "tool": "wrap_in_fragment", "arguments": { "diagram": "$0.diagram",
        "items": ["$5.element"], "operator": "opt", "guard": "e != null" } }));
    calls.push(message("reply", 2, 1, "e", None));
    let r = build(&mut host, json!(calls));
    let frame = drawn(&mut host, &r[0]["diagram"])
        .nodes
        .iter()
        .find(|n| matches!(n.figure, Figure::Fragment { .. }))
        .unwrap()
        .rect;
    // Below the frame, not at the end of the bar inside it (#55).
    let heights = message_heights(&mut host, &r[0]["diagram"]);
    let reply = heights.iter().find(|(name, _)| name == "e").unwrap().1;
    assert!(reply > f64::from(frame.bottom()), "{reply} in {frame:?}");
    // A reply whose call is in the fragment stays in it.
    let mut calls = lifelines(&["b", "a"]);
    calls.push(message("message", 1, 2, "x()", Some(100.0)));
    calls.push(json!({ "tool": "wrap_in_fragment", "arguments": { "diagram": "$0.diagram",
        "items": ["$3.element"], "operator": "loop", "guard": "i" } }));
    calls.push(message("reply", 2, 1, "r", None));
    let r = build(&mut host, json!(calls));
    let frame = drawn(&mut host, &r[0]["diagram"])
        .nodes
        .iter()
        .find(|n| matches!(n.figure, Figure::Fragment { .. }))
        .unwrap()
        .rect;
    let heights = message_heights(&mut host, &r[0]["diagram"]);
    let reply = heights.iter().find(|(name, _)| name == "r").unwrap().1;
    assert!(reply < f64::from(frame.bottom()), "{reply} below {frame:?}");
}

#[test]
fn a_message_below_a_fragment_does_not_continue_a_bar_inside_it() {
    for y in [Some(230.0), None] {
        let mut host = Headless::new();
        let mut calls = lifelines(&["a", "b"]);
        calls.push(message("message", 1, 2, "m1", Some(120.0)));
        calls.push(message("reply", 2, 1, "r1", Some(150.0)));
        calls.push(json!({ "tool": "wrap_in_fragment", "arguments": { "diagram": "$0.diagram",
            "items": ["$3.element", "$4.element"], "operator": "loop", "guard": "x" } }));
        let r = build(&mut host, json!(calls));
        let frame = |host: &mut Headless| {
            drawn(host, &r[0]["diagram"])
                .nodes
                .iter()
                .find(|n| matches!(n.figure, Figure::Fragment { .. }))
                .unwrap()
                .rect
        };
        let before = frame(&mut host);
        let mut args = json!({ "diagram": r[0]["diagram"], "tool": "message", "name": "m2",
                               "source": r[1]["element"], "target": r[2]["element"] });
        if let Some(y) = y {
            args["y"] = json!(y);
        }
        run(&mut host, "connect", args);
        // The caller's bar in the loop ends there; the message below starts a new one
        // instead of pulling the loop down around itself (#79).
        let after = frame(&mut host);
        assert!((after.bottom() - before.bottom()).abs() < 0.01, "{before:?} -> {after:?}");
        let heights = message_heights(&mut host, &r[0]["diagram"]);
        let m2 = heights.iter().find(|(name, _)| name == "m2").unwrap().1;
        assert!(m2 > f64::from(after.bottom()), "{m2} in {after:?}");
    }
}

#[test]
fn a_reply_in_one_case_of_an_alt_does_not_end_the_bar_in_the_other() {
    let mut host = Headless::new();
    let mut calls = lifelines(&["p", "k", "a"]);
    calls.push(message("message", 1, 2, "kaufen", Some(100.0)));
    calls.push(message("reply", 2, 1, "kt", Some(170.0)));
    calls.push(json!({ "tool": "wrap_in_fragment", "arguments": { "diagram": "$0.diagram",
        "items": ["$5.element"], "operator": "alt", "guard": "frei" } }));
    let r = build(&mut host, json!(calls));
    let d = &r[0]["diagram"];
    let g = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let alt = g["nodes"].as_array().unwrap().iter().find(|n| n["kind"] == "combinedFragment");
    let operands = alt.unwrap()["operands"].as_array().unwrap().clone();
    assert_eq!(operands.len(), 2, "{operands:?}");
    let y = operands[1]["top"].as_f64().unwrap() + 40.0;
    let k = |e: &str| r[2][e].clone();
    run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "message", "name": "zeigen",
        "source": k("element"), "target": r[3]["element"], "y": y }),
    );
    run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "reply", "name": "null",
        "source": k("element"), "target": r[1]["element"], "y": y + 30.0 }),
    );
    // The else case goes on in the bar `kaufen` started, and its reply ends that bar, as the
    // reply in the first case does: no second bar starts inside it (#80).
    let g = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let lane = k("element");
    let bars: Vec<(f64, f64)> = g["bars"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|b| b["lifeline"] == lane)
        .map(|b| (b["top"].as_f64().unwrap(), b["bottom"].as_f64().unwrap()))
        .collect();
    assert_eq!(bars.len(), 1, "{bars:?}");
    assert!((bars[0].0 - 100.0).abs() < 0.01 && (bars[0].1 - (y + 30.0)).abs() < 0.01, "{bars:?}");
}

#[test]
fn signals_keep_their_height_and_go_without_parentheses() {
    let mut host = Headless::new();
    let mut calls = lifelines(&[":PC", ":Arduino"]);
    calls.push(message("asyncMessage", 1, 2, "START", Some(100.0)));
    calls.push(message("asyncMessage", 1, 2, "DATEN(17)", Some(140.0)));
    calls.push(message("asyncMessage", 1, 2, "STOP", Some(180.0)));
    calls.push(message("message", 1, 2, "frage", Some(240.0)));
    let r = build(&mut host, json!(calls));
    // Where they were drawn, not below the bars the ones before start (#54).
    let heights = message_heights(&mut host, &r[0]["diagram"]);
    let ys: Vec<f64> = heights.iter().map(|(_, y)| *y).collect();
    assert_eq!(ys, [100.0, 140.0, 180.0, 240.0]);
    // Signals named without parentheses show none; calls keep theirs.
    let texts = |host: &mut Headless| -> Vec<String> {
        let scene = drawn(host, &r[0]["diagram"]);
        scene.edges.iter().flat_map(|e| e.labels.iter().map(|l| l.text.clone())).collect()
    };
    assert_eq!(texts(&mut host), ["START", "DATEN(17)", "STOP", "frage()"]);
    // Arguments bring them back, `null` takes them away.
    run(&mut host, "update_element", json!({ "id": r[5]["element"], "arguments": "x" }));
    run(&mut host, "update_element", json!({ "id": r[6]["element"], "arguments": null }));
    assert_eq!(texts(&mut host), ["START", "DATEN(17)", "STOP(x)", "frage"]);
}

#[test]
fn guards_keep_off_lifelines_bars_and_message_labels() {
    for guard in ["g != null", "nicht voll und betrag >= preis"] {
        let mut host = Headless::new();
        let mut calls = lifelines(&[":Program", "t:Theke"]);
        calls.push(message("message", 1, 2, "pruefen()", Some(100.0)));
        calls.push(message("message", 2, 1, "ok", Some(200.0)));
        calls.push(message("reply", 2, 1, "fertig", Some(260.0)));
        calls.push(json!({ "tool": "wrap_in_fragment", "arguments": {
            "diagram": "$0.diagram", "items": ["$4.element"], "operator": "alt",
            "guard": guard } }));
        let r = build(&mut host, json!(calls));
        let scene = drawn(&mut host, &r[0]["diagram"]);
        let (_, measure) = host.parts();
        let fragment =
            scene.nodes.iter().find(|n| matches!(n.figure, Figure::Fragment { .. })).unwrap();
        let row = fragment.rows.iter().find(|row| row.text.contains(guard)).unwrap().rect;
        // Clear of the bars and lifelines beneath it (#69)...
        for node in &scene.nodes {
            match node.figure {
                Figure::Activation => {
                    assert!(!row.inflate(1.0).intersects(&node.rect), "{row:?} {:?}", node.rect)
                }
                Figure::Lifeline { head, .. } => {
                    let x = head.center().x;
                    assert!(x < row.x - 1.0 || x > row.right() + 1.0, "{row:?} over {x}");
                }
                _ => {}
            }
        }
        // ...and above the name of the message below it.
        let edge = scene.edge(id(&r[4]["presentation"]).parse().unwrap()).unwrap();
        let name = labels::label_rect(&edge.labels[0], measure);
        assert!(!row.intersects(&name), "{row:?} on {name:?}");
    }
}

#[test]
fn message_names_keep_off_crossed_lifelines_and_arrowheads() {
    // A short name beside the lifeline its line crosses, not on it (#70).
    let mut host = Headless::new();
    let mut calls = lifelines(&["a:A", "b:B", "c:C"]);
    calls.push(message("message", 1, 3, "anzahl", Some(100.0)));
    calls.push(message("reply", 3, 1, "2", Some(130.0)));
    let r = build(&mut host, json!(calls));
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let b = scene.node(id(&r[2]["presentation"]).parse().unwrap()).unwrap();
    let Figure::Lifeline { head, .. } = b.figure else { panic!("a lifeline") };
    let crossed = head.center().x;
    for line in [&r[4], &r[5]] {
        let edge = scene.edge(id(&line["presentation"]).parse().unwrap()).unwrap();
        let name = labels::label_rect(&edge.labels[0], measure);
        assert!(name.right() + 3.0 <= crossed || crossed + 3.0 <= name.x, "{name:?} at {crossed}");
    }
    // Nor right at a bar on that lifeline: in the middle of the room beside it (#74).
    let mut host = Headless::new();
    let mut calls = lifelines(&["h:Bahnhof", "ben:Spieler", "ada:Spieler"]);
    calls.push(message("message", 1, 2, "bezahlen(100)", Some(100.0)));
    calls.push(message("message", 1, 3, "erhalten(100)", Some(130.0)));
    let r = build(&mut host, json!(calls));
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let edge = scene.edge(id(&r[5]["presentation"]).parse().unwrap()).unwrap();
    let name = labels::label_rect(&edge.labels[0], measure);
    let bars = scene.nodes.iter().filter(|n| matches!(n.figure, Figure::Activation));
    for bar in bars.map(|n| n.rect).filter(|b| b.y < name.bottom() && b.bottom() > name.y) {
        assert!(name.right() + 4.0 <= bar.x || bar.right() + 4.0 <= name.x, "{name:?} at {bar:?}");
    }
    // A name about as long as its line ends before the arrowhead; a longer one goes up
    // (#71).
    let top_of = |text: &str| {
        let mut host = Headless::new();
        let mut calls = lifelines(&[":Program", "s:Socket"]);
        calls.push(message("message", 1, 2, text, Some(100.0)));
        let r = build(&mut host, json!(calls));
        let scene = drawn(&mut host, &r[0]["diagram"]);
        let (_, measure) = host.parts();
        let edge = scene.edge(id(&r[3]["presentation"]).parse().unwrap()).unwrap();
        let tip = edge.points[edge.points.len() - 1].x;
        let name = labels::label_rect(&edge.labels[0], measure);
        (name.y, name.right() <= tip - 11.0, name.x >= edge.points[0].x)
    };
    let (usual, _, _) = top_of("write()");
    let (top, clear, from_bar) = top_of("write(\"SUCHE Demir, Aylin\\n\")");
    assert!(top == usual && clear && from_bar);
    let (top, clear, _) = top_of("write(\"SUCHE Demir, Aylin, geboren in Ulm\\n\")");
    assert!(top < usual && !clear);
}

#[test]
fn texts_stay_readable_over_lifelines() {
    let mut host = Headless::new();
    let mut calls = lifelines(&["k:Kasse", "f:Film", "s:Saal"]);
    calls.push(message("message", 1, 3, "istAusverkauft()", Some(100.0)));
    calls.push(message("message", 1, 2, "laufe()", Some(160.0)));
    calls.push(json!({ "tool": "wrap_in_fragment", "arguments": { "diagram": "$0.diagram",
        "items": ["$5.element"], "operator": "loop", "guard": "bis RICHTIG" } }));
    let r = build(&mut host, json!(calls));
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let image = svg::render(&scene, &umlcraft_notation::scene::ApproxMeasure);
    // A fragment's label stands on a white box drawn right before it, in a tab filled white,
    // above the lifelines and bars (#58, #64).
    let lines: Vec<&str> = image.lines().collect();
    let white = |l: &str, tag: &str| l.starts_with(tag) && l.contains("fill=\"#ffffff\"");
    let label = lines.iter().position(|l| l.contains(">loop [bis RICHTIG]</text>")).unwrap();
    assert!(white(lines[label - 1], "<rect"), "{:?}", &lines[label - 2..=label]);
    assert!(white(lines[label - 2], "<polygon"), "{:?}", &lines[label - 2..=label]);
    // A message's name stands on one too, above the message line and drawn before the
    // arrowheads, so that it covers none (#64).
    let y = |l: &str, name: &str| -> f32 {
        let at = l.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
        l[at..].split('"').next().unwrap().parse().unwrap()
    };
    let backdrop = lines
        .iter()
        .position(|l| white(l, "<rect") && (80.0..100.0).contains(&(y(l, "y") + y(l, "height"))))
        .unwrap_or_else(|| panic!("{image}"));
    let head = lines
        .iter()
        .position(|l| l.starts_with("<polygon") && l.contains("fill=\"#000000\""))
        .unwrap();
    assert!(backdrop < head, "{image}");
}

#[test]
fn activation_bars_are_listed_moved_and_joined() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "sequence", "name": "sd",
                "message_numbers": false, "frame": false } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "lifeline", "name": "pc : PC" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "lifeline", "name": "w : Wetterstation" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "asyncMessage", "source": "$1.element", "target": "$2.element",
                "name": "start", "y": 100 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "reply",
                "source": "$2.element", "target": "$1.element", "name": "OK", "y": 140 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "asyncMessage", "source": "$2.element", "target": "$1.element",
                "name": "temp", "y": 180 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "asyncMessage", "source": "$1.element", "target": "$2.element",
                "name": "xoff", "y": 220 } },
        ]),
    );
    let d = r[0]["diagram"].clone();
    let bars = |host: &mut Headless| {
        run(host, "get_diagram", json!({ "diagram": d }))["bars"].as_array().unwrap().clone()
    };
    // Each bar with its presentation (#85).
    let before = bars(&mut host);
    assert!(before.len() > 2, "{before:?}");
    assert!(before.iter().all(|b| b["presentation"].is_string()), "{before:?}");
    // One bar per lifeline, from its first message to the end of its last.
    for lifeline in [&r[1]["element"], &r[2]["element"]] {
        let mine: Vec<&Value> = before.iter().filter(|b| &b["lifeline"] == lifeline).collect();
        let top = mine.iter().map(|b| b["top"].as_f64().unwrap()).fold(f64::MAX, f64::min);
        let bottom = mine.iter().map(|b| b["bottom"].as_f64().unwrap()).fold(0.0, f64::max);
        let first = mine.iter().find(|b| b["top"].as_f64() == Some(top)).unwrap();
        let moved = run(
            &mut host,
            "move_node",
            json!({ "diagram": d, "node": first["presentation"], "y": top,
                    "height": bottom - top }),
        );
        assert_eq!(moved["top"].as_f64(), Some(top));
        assert_eq!(moved["bottom"].as_f64(), Some(bottom));
        // The bars it reached are taken in; one nested in them stays, until removed.
        let left: Vec<Value> = bars(&mut host)
            .into_iter()
            .filter(|b| &b["lifeline"] == lifeline && b["presentation"] != first["presentation"])
            .collect();
        assert!(left.len() < mine.len() - 1, "{left:?}");
        for other in left {
            let args = json!({ "diagram": d, "id": other["presentation"] });
            run(&mut host, "remove_from_diagram", args);
        }
    }
    let after = bars(&mut host);
    assert_eq!(after.len(), 2, "{after:?}");
    let ends: Vec<(f64, f64)> =
        after.iter().map(|b| (b["top"].as_f64().unwrap(), b["bottom"].as_f64().unwrap())).collect();
    let last = |b: &[&Value]| b.iter().map(|b| b["bottom"].as_f64().unwrap()).fold(0.0, f64::max);
    let pc: Vec<&Value> = before.iter().filter(|b| b["lifeline"] == r[1]["element"]).collect();
    let w: Vec<&Value> = before.iter().filter(|b| b["lifeline"] == r[2]["element"]).collect();
    assert_eq!(ends, [(100.0, last(&pc)), (100.0, last(&w))]);
    // Later edits keep them.
    run(&mut host, "update_element", json!({ "id": r[6]["element"], "name": "xon" }));
    assert_eq!(bars(&mut host), after);
    // A bar no message reaches, or one given a place across, is refused.
    let bar = &after[0]["presentation"];
    for args in [
        json!({ "diagram": d, "node": bar, "y": 1000 }),
        json!({ "diagram": d, "node": bar, "y": 100, "x": 5 }),
        json!({ "diagram": d, "node": bar, "y": 100, "height": 0 }),
    ] {
        assert!(call(&mut host, &Access::default(), "move_node", &args).is_error, "{args}");
    }
    assert_eq!(bars(&mut host), after);
}

#[test]
fn lifelines_standing_for_an_actor_are_stick_figures() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "use_case", "name": "u" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "actor", "name": "Kunde" } },
            { "tool": "create_diagram", "arguments": { "kind": "sequence", "name": "s" } },
            { "tool": "create_element", "arguments": { "diagram": "$2.diagram",
                "tool": "lifeline", "name": "k", "classifier": "Kunde" } },
            { "tool": "create_element", "arguments": { "diagram": "$2.diagram",
                "tool": "lifeline", "name": "b" } },
            { "tool": "connect", "arguments": { "diagram": "$2.diagram",
                "tool": "message", "source": "$3.presentation", "target": "$4.presentation",
                "name": "bestellen" } },
        ]),
    );
    let d: Id = id(&r[2]["diagram"]).parse().unwrap();
    let iconic = |host: &Headless, node: &Value| {
        let view = host.document().model().view(d).unwrap();
        view.get(id(node).parse().unwrap()).unwrap().style.iconic
    };
    // An actor as the type: a stick figure, named `k : Kunde` below it (#92).
    assert!(iconic(&host, &r[3]["presentation"]));
    assert!(!iconic(&host, &r[4]["presentation"]));
    let scene = drawn(&mut host, &r[2]["diagram"]);
    let head = scene.nodes.iter().find(|n| n.id.to_string() == id(&r[3]["presentation"]));
    let head = head.unwrap();
    assert!(matches!(head.figure, Figure::Lifeline { icon: scene::HeadIcon::Actor, .. }));
    assert_eq!(head.rows[0].text, "k : Kunde");
    host.document().model().check().unwrap();

    // No type, a box; the actor again, a stick figure; `icon` switches it off.
    let k = &r[3]["element"];
    run(&mut host, "update_element", json!({ "id": k, "classifier": "" }));
    assert!(!iconic(&host, &r[3]["presentation"]));
    let wrong = json!({ "id": k, "icon": true });
    assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);
    run(&mut host, "update_element", json!({ "id": k, "classifier": "Kunde" }));
    assert!(iconic(&host, &r[3]["presentation"]));
    run(&mut host, "update_element", json!({ "id": k, "icon": false }));
    assert!(!iconic(&host, &r[3]["presentation"]));

    // There is no `actor` tool on a sequence diagram; the refusal says what to do.
    let actor = json!({ "diagram": r[2]["diagram"], "tool": "actor", "name": "K" });
    let refused = call(&mut host, &Access::default(), "create_element", &actor);
    assert!(refused.is_error);
    assert!(format!("{refused:?}").contains("classifier"), "{refused:?}");
    host.document().model().check().unwrap();
}

#[test]
fn an_update_tidies_only_the_sequence_diagrams_it_touches() {
    let mut host = Headless::new();
    let (d, messages) = sequence_script(&mut host, &[("message", 0, 1, "a()", 120.0)]);
    let diagram: Id = d.parse().unwrap();
    // The bars moved away from their messages, as an imported diagram may have them.
    let bars: Vec<Id> = {
        let view = host.document().model().view(diagram).unwrap();
        view.presentations()
            .iter()
            .filter(|p| matches!(p.shape, Shape::Activation { .. }))
            .map(|p| p.id)
            .collect()
    };
    let tidy = bars_of(&host, &d, 1);
    let (doc, _) = host.parts();
    doc.edit("longer", |tx| {
        for &bar in &bars {
            tx.update_presentation(diagram, bar, |p| {
                if let Shape::Activation { top, .. } = &mut p.shape {
                    *top += 200.0;
                }
            })?;
        }
        Ok(())
    })
    .unwrap();
    let untidy = bars_of(&host, &d, 1);
    assert_ne!(untidy, tidy);

    // A class elsewhere renamed: the sequence diagram stays as it is.
    let root = run(&mut host, "get_document", json!({}))["root"].clone();
    let class = json!({ "owner": root, "tool": "class", "name": "Konto" });
    let class = run(&mut host, "create_model_element", class);
    run(&mut host, "update_element", json!({ "id": class["element"], "name": "Account" }));
    assert_eq!(bars_of(&host, &d, 1), untidy);

    // Its message renamed: the diagram is tidied.
    run(&mut host, "update_element", json!({ "id": messages[0]["element"], "name": "b()" }));
    assert_ne!(bars_of(&host, &d, 1), untidy);
}

/// `x` alone places a lifeline (and any node) there, with the top of a free spot; `y` alone
/// likewise (issue #93).
#[test]
fn x_alone_places_a_lifeline() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "S" }));
    let d = d["diagram"].clone();
    let a = json!({ "diagram": d, "tool": "lifeline", "name": "a" });
    let a = run(&mut host, "create_element", a);
    let b = json!({ "diagram": d, "tool": "lifeline", "name": "b", "x": 280 });
    let b = run(&mut host, "create_element", b);
    let shown = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let bounds = |id: &Value| {
        let nodes = shown["nodes"].as_array().unwrap();
        nodes.iter().find(|n| n["presentation"] == *id).unwrap()["bounds"].clone()
    };
    let a_top = bounds(&a["presentation"])["y"].clone();
    assert_eq!(
        (bounds(&b["presentation"])["x"].as_f64(), bounds(&b["presentation"])["y"].clone()),
        (Some(280.0), a_top)
    );

    let c = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
    let c = c["diagram"].clone();
    let k = run(&mut host, "create_element", json!({ "diagram": c, "tool": "class", "y": 300 }));
    let shown = run(&mut host, "get_diagram", json!({ "diagram": c }));
    let node =
        shown["nodes"].as_array().unwrap().iter().find(|n| n["presentation"] == k["presentation"]);
    assert_eq!(node.unwrap()["bounds"]["y"].as_f64(), Some(300.0));
}
