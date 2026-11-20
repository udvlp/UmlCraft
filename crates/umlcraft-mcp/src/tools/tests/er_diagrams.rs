//! ER diagrams: keys, notations, SQL and how their lines and labels are drawn.

use super::*;

#[test]
fn an_ie_diagram_copies_keys_and_switches_notation() {
    let mut host = Headless::new();
    let d = run(
        &mut host,
        "create_diagram",
        json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
    );
    let d = id(&d["diagram"]);
    let shown = run(&mut host, "get_diagram", json!({ "diagram": d }));
    let tools = shown.to_string();
    assert!(tools.contains("erIdentifying") && !tools.contains("erLine"), "{tools}");
    let entity = |host: &mut Headless, name: &str| {
        let e =
            run(host, "create_element", json!({ "diagram": d, "tool": "erEntity", "name": name }));
        id(&e["element"])
    };
    let customer = entity(&mut host, "Customer");
    let order = entity(&mut host, "Order");
    run(&mut host, "add_member", json!({ "owner": customer, "text": "id : INTEGER", "key": true }));
    let places = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "erNonIdentifying", "source": customer, "target": order }),
    );
    let places = id(&places["element"]);
    let model = host.document().model();
    let fk = |model: &Model| -> Vec<(String, Option<ErKey>)> {
        model
            .children(parse_id(&order).unwrap())
            .iter()
            .filter_map(|&c| model.get(c))
            .filter_map(|c| match c.kind() {
                ElementKind::ErAttribute(p) if p.foreign_key.is_some() => {
                    Some((c.name().to_owned(), p.key))
                }
                _ => None,
            })
            .collect()
    };
    assert_eq!(fk(model), [("id".to_owned(), None)]);
    run(&mut host, "update_element", json!({ "id": places, "identifying": true }));
    assert_eq!(fk(host.document().model()), [("id".to_owned(), Some(ErKey::Primary))]);
    run(&mut host, "update_element", json!({ "id": d, "notation": "chen", "er_display": "keys" }));
    let chen = run(&mut host, "get_diagram", json!({ "diagram": d })).to_string();
    assert!(chen.contains("erLine") && !chen.contains("erIdentifying\""), "{chen}");
    let bad = call(
        &mut host,
        &Access::default(),
        "update_element",
        &json!({ "id": customer, "notation": "ie" }),
    );
    assert!(bad.is_error);
    // An associative entity that entities take part in stays one.
    let enrolment = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "erAssociativeEntity", "name": "Enrolment" }),
    );
    let enrolment = id(&enrolment["element"]);
    run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "erLine", "source": enrolment, "target": customer }),
    );
    let plain = |host: &mut Headless, entity: &str| {
        call(
            host,
            &Access::default(),
            "update_element",
            &json!({ "id": entity, "associative": false }),
        )
    };
    assert!(plain(&mut host, &enrolment).is_error);
    assert!(!plain(&mut host, &customer).is_error);
}

#[test]
fn an_er_diagram_comes_out_as_sql() {
    let mut host = Headless::new();
    let d = run(
        &mut host,
        "create_diagram",
        json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
    );
    let d = id(&d["diagram"]);
    let mut entity = |name: &str, key: &str| {
        let e = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erEntity", "name": name }),
        );
        let e = id(&e["element"]);
        run(&mut host, "add_member", json!({ "owner": e, "text": key, "key": true }));
        e
    };
    let (customer, order) = (entity("Customer", "id : INTEGER"), entity("Order", "no : INTEGER"));
    let link =
        json!({ "diagram": d, "tool": "erNonIdentifying", "source": customer, "target": order });
    run(&mut host, "connect", link);
    let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "postgres" }));
    let sql = out["sql"].as_str().unwrap();
    assert!(sql.contains("CREATE TABLE \"Order\" ("), "{sql}");
    assert!(sql.contains("REFERENCES \"Customer\" (id);"), "{sql}");
    assert_eq!((out["tables"].as_u64(), out["foreign_keys"].as_u64()), (Some(2), Some(1)));
    let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
    let wrong =
        call(&mut host, &Access::default(), "export_sql", &json!({ "diagram": class["diagram"] }));
    assert!(wrong.is_error);
}

#[test]
fn domains_checks_and_indexes_go_into_the_sql() {
    let mut host = Headless::new();
    let d = run(
        &mut host,
        "create_diagram",
        json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
    );
    let d = id(&d["diagram"]);
    let model = host.document().model();
    let package = model.get(parse_id(&d).unwrap()).unwrap().owner().unwrap().to_string();
    let state = run(
        &mut host,
        "create_model_element",
        json!({ "owner": package, "tool": "erDomain", "name": "state" }),
    );
    let state = id(&state["id"]);
    run(&mut host, "update_element", json!({ "id": state, "values": ["open", "paid"] }));
    let order = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "erEntity", "name": "orders" }),
    );
    let order = id(&order["element"]);
    let no =
        run(&mut host, "add_member", json!({ "owner": order, "text": "no : int", "key": true }));
    let status = run(&mut host, "add_member", json!({ "owner": order, "text": "status" }));
    let total = run(&mut host, "add_member", json!({ "owner": order, "text": "total : int" }));
    run(&mut host, "update_element", json!({ "id": id(&no["id"]), "auto_increment": true }));
    let line = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "erEntity", "name": "lines" }),
    );
    let line = id(&line["element"]);
    let has = run(
        &mut host,
        "connect",
        json!({ "diagram": d, "tool": "erNonIdentifying", "source": order, "target": line }),
    );
    let has = id(&has["element"]);
    let actions = json!({ "id": has, "on_delete": "cascade", "on_update": "SET NULL" });
    run(&mut host, "update_element", actions);
    let bad = json!({ "id": has, "on_delete": "drop" });
    assert!(call(&mut host, &Access::default(), "update_element", &bad).is_error);
    run(&mut host, "update_element", json!({ "id": id(&status["id"]), "datatype": "state" }));
    run(&mut host, "update_element", json!({ "id": id(&total["id"]), "check": "total >= 0" }));
    let indexes = json!([{ "name": "ix_status", "columns": ["status"] }]);
    run(
        &mut host,
        "update_element",
        json!({ "id": order, "indexes": indexes, "checks": ["no > 0"] }),
    );
    let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "postgres" }));
    let sql = out["sql"].as_str().unwrap();
    assert!(sql.contains("CREATE TYPE state AS ENUM ('open', 'paid');"), "{sql}");
    assert!(sql.contains("status state,"), "{sql}");
    assert!(sql.contains("total int CHECK (total >= 0)"), "{sql}");
    assert!(sql.contains("CHECK (no > 0)"), "{sql}");
    assert!(sql.contains("CREATE INDEX ix_status ON orders (status);"), "{sql}");
    assert!(sql.contains("no int NOT NULL GENERATED BY DEFAULT AS IDENTITY"), "{sql}");
    assert!(sql.contains("REFERENCES orders (no) ON DELETE CASCADE ON UPDATE SET NULL;"), "{sql}");
    let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "mysql" }));
    let sql = out["sql"].as_str().unwrap();
    assert!(sql.contains("status enum('open','paid'),"), "{sql}");
    assert!(sql.contains("no int NOT NULL AUTO_INCREMENT"), "{sql}");
}

#[test]
fn participations_of_a_recursive_relationship_run_apart() {
    let mut host = Headless::new();
    let (d, _, lines) = recursive_relationship(&mut host);
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    let edge = |e: &String| scene.edge(e.parse().unwrap()).unwrap();
    let distance = |p: Point, e: &String| {
        edge(e)
            .points
            .windows(2)
            .map(|w| scene::distance_to_segment(p, w[0], w[1]))
            .fold(f32::INFINITY, f32::min)
    };
    // The middle of each line keeps well away from the other line.
    for (own, other) in [(&lines[0], &lines[1]), (&lines[1], &lines[0])] {
        let (s, t) = umlcraft_notation::route::middle_segment(&edge(own).points, false);
        let middle = Point::new((s.x + t.x) / 2.0, (s.y + t.y) / 2.0);
        assert!(distance(middle, other) > 20.0, "{:?}", edge(own).points);
        // Its cardinality and role stand nearer to it than to the other line.
        for label in &edge(own).labels {
            let width = scene::label_width(&label.text, measure);
            let at = label.anchor.offset(width / 2.0, scene::LABEL_HEIGHT / 2.0);
            assert!(distance(at, own) < distance(at, other), "{} at {at:?}", label.text);
        }
    }
}

#[test]
fn spread_lines_follow_a_moved_node() {
    let mut host = Headless::new();
    // Drawn where it ends up, and drawn elsewhere and moved there (#41).
    let (placed, _, placed_lines) = recursive_relationship(&mut host);
    let d =
        run(&mut host, "create_diagram", json!({ "kind": "entity_relationship", "name": "moved" }));
    let d = id(&d["diagram"]);
    let user = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "erEntity", "name": "User", "x": 20, "y": 100 }),
    );
    let refers = run(
        &mut host,
        "create_element",
        json!({ "diagram": d, "tool": "erRelationship", "name": "refers", "x": 600, "y": 300 }),
    );
    let lines = [(); 2].map(|_| {
        let args = json!({ "diagram": d, "tool": "erLine", "source": refers["element"],
                           "target": user["element"] });
        id(&run(&mut host, "connect", args)["presentation"])
    });
    let args = json!({ "diagram": d, "node": refers["presentation"], "x": 260, "y": 95 });
    run(&mut host, "move_node", args);
    let (doc, measure) = host.parts();
    let points = |d: &String, line: &String| {
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        scene.edge(line.parse().unwrap()).unwrap().points.clone()
    };
    for (moved, placed_line) in lines.iter().zip(&placed_lines) {
        let (got, want) = (points(&d, moved), points(&placed, placed_line));
        assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!(g.distance(*w) < 1.5, "{got:?} vs {want:?}");
        }
    }
}

#[test]
fn routed_lines_end_on_the_rhombus() {
    let mut host = Headless::new();
    let (d, refers, lines) = recursive_relationship(&mut host);
    run(&mut host, "route_lines", json!({ "diagram": d }));
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    let rhombus = scene.node(refers.parse().unwrap()).unwrap().rect;
    let c = rhombus.center();
    for line in &lines {
        let points = &scene.edge(line.parse().unwrap()).unwrap().points;
        let end = [points[0], points[points.len() - 1]]
            .into_iter()
            .min_by(|a, b| a.distance(c).total_cmp(&b.distance(c)))
            .unwrap();
        // On the outline: |dx| / (w / 2) + |dy| / (h / 2) = 1.
        let on = (end.x - c.x).abs() / (rhombus.width / 2.0)
            + (end.y - c.y).abs() / (rhombus.height / 2.0);
        assert!((on - 1.0).abs() < 0.02, "{end:?} on {rhombus:?}: {on}");
    }
}

#[test]
fn er_diagrams_arrange_to_the_right_by_default_and_down_when_asked() {
    let mut host = Headless::new();
    let d = run(
        &mut host,
        "create_diagram",
        json!({ "kind": "entity_relationship", "name": "arrange" }),
    );
    let d = id(&d["diagram"]);
    let mut make = |tool: &str, name: &str| {
        run(&mut host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
    };
    let person = make("erEntity", "Person");
    let vaccine = make("erEntity", "Vaccine");
    let gets = make("erRelationship", "gets");
    for (s, t) in [(&person, &gets), (&gets, &vaccine)] {
        let args = json!({ "diagram": d, "tool": "erLine", "source": s["element"],
                           "target": t["element"] });
        run(&mut host, "connect", args);
    }
    let centers = |host: &mut Headless| {
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        [&person, &gets, &vaccine]
            .map(|n| scene.node(id(&n["presentation"]).parse().unwrap()).unwrap().rect.center())
    };
    for direction in [None, Some("right"), Some("down")] {
        let mut args = json!({ "diagram": d });
        if let Some(direction) = direction {
            args["direction"] = json!(direction);
        }
        run(&mut host, "arrange_diagram", args);
        let [p, g, v] = centers(&mut host);
        if direction == Some("down") {
            assert!(p.y < g.y && g.y < v.y, "{direction:?}: {p:?} {g:?} {v:?}");
            assert!((p.x - g.x).abs() < 1.0 && (g.x - v.x).abs() < 1.0);
        } else {
            assert!(p.x < g.x && g.x < v.x, "{direction:?}: {p:?} {g:?} {v:?}");
            assert!((p.y - g.y).abs() < 1.0 && (g.y - v.y).abs() < 1.0, "{p:?} {g:?} {v:?}");
        }
    }
}

#[test]
fn arranged_er_lines_keep_clear_of_each_other_and_of_other_lines_labels() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "entity_relationship", "name": "m" }));
    let d = id(&d["diagram"]);
    let mut ids = std::collections::HashMap::new();
    let make = |host: &mut Headless, tool: &str, name: &str| {
        let r = run(host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }));
        r["element"].clone()
    };
    let entities: [(&str, &[&str]); 5] = [
        ("Interpret", &["interpretID", "name", "land", "gruendung"]),
        ("Album", &["albumID", "titel", "jahr"]),
        ("Lied", &["liedID", "titel", "dauer", "genre"]),
        ("Nutzer", &["nutzerID", "vorname", "nachname", "gebDatum", "ort", "abo", "registriert"]),
        ("Playlist", &["playlistID", "name", "oeffentlich"]),
    ];
    let line = |host: &mut Headless, s: &Value, t: &Value| {
        let args = json!({ "diagram": d, "tool": "erLine", "source": s, "target": t });
        run(host, "connect", args)["element"].clone()
    };
    for (entity, attributes) in entities {
        let e = make(&mut host, "erEntity", entity);
        for (k, a) in attributes.iter().enumerate() {
            let tool = if k == 0 { "erKeyAttribute" } else { "erAttribute" };
            let a = make(&mut host, tool, a);
            line(&mut host, &a, &e);
        }
        ids.insert(entity, e);
    }
    let relationships = [
        ("veröffentlicht", "Interpret", "Album", "[1,1]", "[0,n]", None),
        ("enthält", "Album", "Lied", "[1,1]", "[1,n]", Some("trackNr")),
        ("hört", "Nutzer", "Lied", "[0,n]", "[0,m]", Some("zeitpunkt")),
        ("erstellt", "Nutzer", "Playlist", "[1,1]", "[0,n]", None),
        ("umfasst", "Playlist", "Lied", "[0,n]", "[0,m]", Some("position")),
        ("wirbt", "Nutzer", "Nutzer", "[0,1]", "[0,n]", None),
    ];
    for (name, from, to, at_from, at_to, attribute) in relationships {
        let r = make(&mut host, "erRelationship", name);
        for (end, cardinality) in [(from, at_from), (to, at_to)] {
            let l = line(&mut host, &r, &ids[end]);
            run(&mut host, "update_element", json!({ "id": l, "cardinality": cardinality }));
        }
        if let Some(a) = attribute {
            let a = make(&mut host, "erAttribute", a);
            line(&mut host, &a, &r);
        }
    }
    run(&mut host, "arrange_diagram", json!({ "diagram": d }));
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
    let segments: Vec<(usize, Point, Point)> = scene
        .edges
        .iter()
        .enumerate()
        .flat_map(|(k, e)| e.points.windows(2).map(move |w| (k, w[0], w[1])))
        .collect();
    // No line crosses another: an attribute's line goes round a line between other nodes
    // rather than across it (#81).
    for (k, a, b) in &segments {
        for (j, c, e) in &segments {
            assert!(k == j || !cross(*a, *b, *c, *e), "{a:?}-{b:?} crosses {c:?}-{e:?}");
        }
    }
    // A cardinality stands on its own line's side of the lines beside it: where lines meet a
    // node close together, it goes further along, where they have run apart (#82).
    for (k, edge) in scene.edges.iter().enumerate() {
        for label in &edge.labels {
            let r = labels::label_rect(label, measure);
            let c = r.center();
            let near = edge
                .points
                .windows(2)
                .map(|w| {
                    let (dx, dy) = (w[1].x - w[0].x, w[1].y - w[0].y);
                    let len = dx * dx + dy * dy;
                    let t = if len > 0.0 {
                        (((c.x - w[0].x) * dx + (c.y - w[0].y) * dy) / len).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    Point::new(w[0].x + t * dx, w[0].y + t * dy)
                })
                .min_by(|p, q| c.distance(*p).total_cmp(&c.distance(*q)))
                .unwrap();
            for (j, a, b) in &segments {
                assert!(
                    *j == k || !cross(c, near, *a, *b),
                    "{:?} at {c:?} lies beyond {a:?}-{b:?}",
                    label.text
                );
            }
        }
    }
}

#[test]
fn roles_of_a_moved_recursive_relationship_keep_off_the_rhombus() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Mitglied" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erKeyAttribute", "name": "mitgliedID" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$2.element", "target": "$1.element" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erRelationship", "name": "wirbt" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$4.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$5.element",
              "cardinality": "[0,n]", "role": "Werber" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$4.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$7.element",
              "cardinality": "[0,1]", "role": "Geworbener" } },
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
            { "tool": "move_node", "arguments": { "diagram": "$0.diagram",
              "node": "$1.presentation", "x": 100, "y": 140 } },
        ]),
    );
    // Clear of the rhombus and the entity after the move (#57).
    let (doc, measure) = host.parts();
    let d = id(&r[0]["diagram"]).parse().unwrap();
    let scene = scene::build(doc.model(), d, &Preview::default(), measure);
    let rect =
        |reply: &Value| scene.node(id(&reply["presentation"]).parse().unwrap()).unwrap().rect;
    let (rhombus, entity) = (rect(&r[4]), rect(&r[1]));
    for line in [&r[5], &r[7]] {
        for label in &scene.edge(id(&line["presentation"]).parse().unwrap()).unwrap().labels {
            let drawn = labels::label_rect(label, measure);
            for node in [rhombus, entity] {
                assert!(!drawn.intersects(&node), "{} at {drawn:?} on {node:?}", label.text);
            }
        }
    }
}

#[test]
fn a_cardinality_on_a_short_line_stays_beside_it() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Interpret", "x": 0, "y": 0 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Album", "x": 340, "y": 0 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erRelationship", "name": "veröffentlicht", "x": 170, "y": -5 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$3.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$4.element",
              "cardinality": "[1,1]" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$3.element", "target": "$2.element" } },
            { "tool": "update_element", "arguments": { "id": "$6.element",
              "cardinality": "[0,n]" } },
        ]),
    );
    // Beside the line, not below the entity (#60).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let album = scene.node(id(&r[2]["presentation"]).parse().unwrap()).unwrap().rect;
    let edge = scene.edge(id(&r[6]["presentation"]).parse().unwrap()).unwrap();
    let label = edge.labels.iter().find(|l| l.text == "[0,n]").unwrap();
    let drawn = labels::label_rect(label, measure);
    assert!(drawn.y < album.bottom(), "{drawn:?} below {album:?}");
    assert!(!drawn.intersects(&album), "{drawn:?} on {album:?}");
}

#[test]
fn a_cardinality_on_a_very_short_line_stays_on_its_side_of_the_rhombus() {
    // The rhombus close to the entity, or touching it: the `[0,n]` of the short line stays
    // beside it rather than go past the rhombus, onto the side of the other line (#97).
    for gap in [12.0, 10.0, 5.0, 2.0, 0.0] {
        let mut host = Headless::new();
        let r = build(
            &mut host,
            json!([
                { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
                  "name": "er" } },
                { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                  "tool": "erEntity", "name": "A", "x": 40, "y": 40 } },
                { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                  "tool": "erRelationship", "name": "r", "x": 140.0 + gap, "y": 35 } },
                { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                  "tool": "erEntity", "name": "B", "x": 350, "y": 40 } },
                { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
                  "source": "$2.element", "target": "$1.element" } },
                { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
                  "source": "$2.element", "target": "$3.element" } },
                { "tool": "update_element", "arguments": { "id": "$4.element",
                  "cardinality": "[0,n]" } },
                { "tool": "update_element", "arguments": { "id": "$5.element",
                  "cardinality": "[1,1]" } },
            ]),
        );
        let scene = drawn(&mut host, &r[0]["diagram"]);
        let (_, measure) = host.parts();
        let rhombus = scene.node(id(&r[2]["presentation"]).parse().unwrap()).unwrap().rect;
        let edge = |k: usize| scene.edge(id(&r[k]["presentation"]).parse().unwrap()).unwrap();
        let (own, other) = (edge(4), edge(5));
        let distance = |p: Point, points: &[Point]| {
            points
                .windows(2)
                .map(|w| scene::distance_to_segment(p, w[0], w[1]))
                .fold(f32::INFINITY, f32::min)
        };
        let label = own.labels.iter().find(|l| l.text == "[0,n]").unwrap();
        let drawn = labels::label_rect(label, measure);
        let c = drawn.center();
        assert!(
            distance(c, &own.points) < distance(c, &other.points),
            "gap {gap}: {drawn:?} nearer {:?} than {:?}",
            other.points,
            own.points
        );
        assert!(c.x < rhombus.center().x, "gap {gap}: {drawn:?} past the middle of {rhombus:?}");
    }
}

#[test]
fn the_labels_of_a_loop_into_the_top_of_an_entity_stay_at_the_entity() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Nutzer", "x": 0, "y": 0 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erRelationship", "name": "befreundet", "x": 220, "y": -5 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$2.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$3.element",
              "cardinality": "[0,n]", "role": "Anfragender" } },
        ]),
    );
    // Out of the top of the rhombus, left above the entity and down into its top, for
    // several heights of the stretch above the entity (#65, #72).
    let d: Id = id(&r[0]["diagram"]).parse().unwrap();
    let line: Id = id(&r[3]["presentation"]).parse().unwrap();
    let top = (Some(Point::new(0.5, 0.0)), Some(Point::new(0.5, 0.0)));
    for k in [25.0, 30.0, 34.5, 35.0, 35.5, 36.0, 42.0, 60.0] {
        let (doc, _) = host.parts();
        let over = vec![Point::new(281.37, -k), Point::new(50.0, -k)];
        doc.edit("route", |tx| tx.set_route(d, line, over, top)).unwrap();
        let scene = drawn(&mut host, &r[0]["diagram"]);
        let edge = scene.edge(line).unwrap();
        let (_, measure) = host.parts();
        // Both beside the last segment, below the stretch that leads away, one on either
        // side.
        let rects: Vec<Rect> = edge.labels.iter().map(|l| labels::label_rect(l, measure)).collect();
        assert_eq!(rects.len(), 2);
        for (label, r) in edge.labels.iter().zip(&rects) {
            assert!(r.y > -k - 2.0 && r.bottom() <= 0.0, "{k}: {} at {r:?}", label.text);
        }
        assert!(
            rects.iter().any(|r| r.right() < 50.0) && rects.iter().any(|r| r.x > 50.0),
            "{k}: {rects:?}"
        );
    }
}

#[test]
fn a_line_running_off_at_an_angle_leaves_a_cardinality_between() {
    let mut host = Headless::new();
    let relationship = |name: &str, x: f64, y: f64| {
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
            "tool": "erRelationship", "name": name, "x": x, "y": y } })
    };
    let line = |from: usize, text: &str| {
        [
            json!({ "tool": "connect", "arguments": { "diagram": "$0.diagram",
                "tool": "erLine", "source": format!("${from}.element"),
                "target": "$1.element" } }),
            json!({ "tool": "update_element", "arguments": {
                "id": format!("${}.element", from + 1), "cardinality": text } }),
        ]
    };
    let mut calls = vec![
        json!({ "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
            "name": "er" } }),
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
            "tool": "erEntity", "name": "Kurse", "x": 300, "y": 100 } }),
        relationship("leitet", 170.0, 95.0),
    ];
    calls.extend(line(2, "[1,1]"));
    calls.push(relationship("findet statt", 170.0, 220.0));
    calls.extend(line(5, "[0,n]"));
    calls.push(relationship("belegt", 310.0, 220.0));
    calls.extend(line(8, "[1,m]"));
    let r = build(&mut host, json!(calls));
    // The `[0,n]` of the diagonal stays right of it, toward the vertical line, rather than
    // go under the `[1,1]` at the corner (#73).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let label = |k: usize| {
        let edge = scene.edge(id(&r[k]["presentation"]).parse().unwrap()).unwrap();
        labels::label_rect(&edge.labels[0], measure)
    };
    let (one, many) = (label(3), label(6));
    assert!(many.x > one.right(), "{many:?} under {one:?}");
}

#[test]
fn a_line_at_another_side_leaves_a_cardinality_where_it_is() {
    let mut host = Headless::new();
    let entity = |name: &str, x: f64| {
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
            "tool": "erEntity", "name": name, "x": x, "y": 230 } })
    };
    let relationship = |name: &str, x: f64, y: f64| {
        json!({ "tool": "create_element", "arguments": { "diagram": "$0.diagram",
            "tool": "erRelationship", "name": name, "x": x, "y": y } })
    };
    let line = |from: usize, to: usize| {
        json!({ "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
            "source": format!("${from}.element"), "target": format!("${to}.element") } })
    };
    let cardinality = |of: usize, text: &str| {
        json!({ "tool": "update_element", "arguments": {
            "id": format!("${of}.element"), "cardinality": text } })
    };
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            entity("Mitarbeiter", 0.0),
            entity("Projekt", 380.0),
            relationship("arbeitet mit", 190.0, 225.0),
            line(3, 1),
            cardinality(4, "[0,n]"),
            line(3, 2),
            cardinality(6, "[1,m]"),
            relationship("leitet", 70.0, 110.0),
            line(8, 1),
            cardinality(9, "[1,1]"),
        ]),
    );
    // `leitet` meets the top of `Mitarbeiter`, the other line its right side: the `[0,n]`
    // stays above its line (#66).
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let (_, measure) = host.parts();
    let edge = scene.edge(id(&r[4]["presentation"]).parse().unwrap()).unwrap();
    let end = edge.points[edge.points.len() - 1];
    let drawn = labels::label_rect(&edge.labels[0], measure);
    assert!(drawn.bottom() <= end.y + 2.0, "{drawn:?} below the line at {end:?}");
}

#[test]
fn a_loop_to_the_same_entity_leaves_the_labels_of_a_straight_line_at_it() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Nutzer", "x": 20, "y": 100 } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erRelationship", "name": "befreundet", "x": 260, "y": 95 } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$2.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$3.element",
              "cardinality": "[0,n]", "role": "Anfragender" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$2.element", "target": "$1.element" } },
        ]),
    );
    // The straight line along the middle, the other as a loop over the top (#61).
    let (doc, _) = host.parts();
    let d: Id = id(&r[0]["diagram"]).parse().unwrap();
    let (straight, around): (Id, Id) =
        (id(&r[3]["presentation"]).parse().unwrap(), id(&r[5]["presentation"]).parse().unwrap());
    let middle = (Some(Point::new(0.0, 0.5)), Some(Point::new(1.0, 0.5)));
    let top = (Some(Point::new(0.5, 0.0)), Some(Point::new(0.5, 0.0)));
    let over = vec![Point::new(321.37, 50.0), Point::new(70.0, 50.0)];
    doc.edit("route", |tx| {
        tx.set_route(d, straight, Vec::new(), middle)?;
        tx.set_route(d, around, over, top)
    })
    .unwrap();
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let edge = scene.edge(straight).unwrap();
    let y = edge.points[0].y;
    assert!(edge.points.iter().all(|p| p.y == y), "{:?}", edge.points);
    let (_, measure) = host.parts();
    for label in &edge.labels {
        let drawn = labels::label_rect(label, measure);
        let off = (drawn.y - y).max(y - drawn.bottom());
        assert!(off < 12.0, "{} at {drawn:?}, line at {y}", label.text);
    }
}

#[test]
fn chen_ratios_show_the_maximum_of_each_cardinality() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "entity_relationship",
              "name": "er" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Kunde" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erEntity", "name": "Ticket" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "erRelationship", "name": "bucht" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$3.element", "target": "$1.element" } },
            { "tool": "update_element", "arguments": { "id": "$4.element",
              "cardinality": "[1,1]" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "erLine",
              "source": "$3.element", "target": "$2.element" } },
            { "tool": "update_element", "arguments": { "id": "$0.diagram", "ratios": true } },
        ]),
    );
    let scene = drawn(&mut host, &r[0]["diagram"]);
    let texts = |edge: &Value| -> Vec<String> {
        let edge = scene.edge(id(&edge["presentation"]).parse().unwrap()).unwrap();
        edge.labels.iter().map(|l| l.text.clone()).collect()
    };
    assert_eq!((texts(&r[4]), texts(&r[6])), (vec!["1".into()], vec!["n".into()]));
    let diagram = run(&mut host, "get_element", json!({ "id": r[0]["diagram"] }));
    assert!(diagram["properties"].to_string().contains("\"ratios\":true"), "{diagram}");
    // Reading at the entity turns ratios off; both at once are refused.
    run(&mut host, "update_element", json!({ "id": r[0]["diagram"], "look_here": true }));
    let diagram = run(&mut host, "get_element", json!({ "id": r[0]["diagram"] }));
    assert!(!diagram["properties"].to_string().contains("\"ratios\":true"), "{diagram}");
    let both = json!({ "id": r[0]["diagram"], "look_here": true, "ratios": true });
    assert!(call(&mut host, &Access::default(), "update_element", &both).is_error);
    // Only ER diagrams take it.
    let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "c" }));
    let args = json!({ "id": class["diagram"], "ratios": true });
    assert!(call(&mut host, &Access::default(), "update_element", &args).is_error);
}
