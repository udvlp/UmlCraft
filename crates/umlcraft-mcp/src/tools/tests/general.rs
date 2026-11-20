//! The tool list, batches, files, display settings and robustness.

use super::*;

#[test]
fn every_tool_is_listed_and_known() {
    let mut host = Headless::new();
    for tool in list() {
        let name = tool["name"].as_str().unwrap();
        let reply = call(&mut host, &Access::default(), name, &json!({}));
        if let Content::Text(text) = &reply.content[0] {
            assert!(!text.starts_with("unknown tool"), "{name}");
        }
    }
}

#[test]
fn a_screenshot_needs_the_app() {
    let mut host = Headless::new();
    let reply = call(&mut host, &Access::default(), "screenshot", &json!({}));
    assert!(reply.is_error);
    assert_eq!(screenshot_scale(&json!({ "scale": 0.1 })), 0.25);
    assert_eq!(screenshot_scale(&json!({})), 1.0);
    for tool in ["set_window", "set_view"] {
        assert!(call(&mut host, &Access::default(), tool, &json!({})).is_error, "{tool}");
    }
    let sample = call(&mut host, &Access::default(), "new_document", &json!({ "sample": true }));
    assert!(sample.is_error);
}

#[test]
fn select_takes_elements_and_presentations() {
    let mut host = Headless::new();
    let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
    let d = id(&d["diagram"]);
    let order =
        run(&mut host, "create_element", json!({ "diagram": d, "tool": "class", "name": "Order" }));
    let by_element = run(&mut host, "select", json!({ "ids": [order["element"]] }));
    assert_eq!(by_element["selection"], json!([order["presentation"]]));
    let by_presentation =
        run(&mut host, "select", json!({ "ids": [order["presentation"]], "diagram": d }));
    assert_eq!(by_presentation["selection"], by_element["selection"]);
    let unknown = call(
        &mut host,
        &Access::default(),
        "select",
        &json!({ "ids": [Id::generate().to_string()] }),
    );
    assert!(unknown.is_error);
}

#[test]
fn a_batch_is_one_step_and_all_or_nothing() {
    let mut host = Headless::new();
    let before = host.document().undo_depth();
    let calls = json!([
        { "tool": "create_diagram", "arguments": { "kind": "class", "name": "Shop" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "Order" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "Customer" } },
        { "tool": "add_member", "arguments": { "owner": "$1.element", "text": "+ total() : int" } },
        { "tool": "connect", "arguments": {
            "diagram": "$0.diagram", "tool": "association",
            "source": "$1.element", "target": "$2.element" } },
        { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
    ]);
    let done = run(&mut host, "batch", json!({ "calls": calls, "label": "Shop" }));
    let d = id(&done["results"][0]["diagram"]);
    let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
    assert_eq!(diagram["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(diagram["lines"].as_array().unwrap().len(), 1);
    assert_eq!(host.document().undo_depth(), before + 1);
    assert_eq!(host.document().undo_label(), Some("Shop"));
    host.document().model().check().unwrap();
    run(&mut host, "undo", json!({}));
    assert!(host.document().model().get(parse_id(&d).unwrap()).is_none());

    // A failing call takes back what the calls before it did.
    let revision = host.document().revision();
    let model = format!("{:?}", host.document().model().get(host.document().model().root()));
    let calls = json!([
        { "tool": "create_diagram", "arguments": { "kind": "class", "name": "Shop" } },
        { "tool": "create_element", "arguments": { "diagram": "$0.diagram", "tool": "nonsense" } },
    ]);
    let refused = call(&mut host, &Access::default(), "batch", &json!({ "calls": calls }));
    assert!(refused.is_error);
    assert_ne!(host.document().revision(), revision);
    assert_eq!(format!("{:?}", host.document().model().get(host.document().model().root())), model);
    let undo = json!([{ "tool": "undo" }]);
    assert!(call(&mut host, &Access::default(), "batch", &json!({ "calls": undo })).is_error);
    let dangling = json!([{ "tool": "get_element", "arguments": { "id": "$3.element" } }]);
    assert!(call(&mut host, &Access::default(), "batch", &json!({ "calls": dangling })).is_error);
}

#[test]
fn every_tool_that_makes_an_element_names_it_element() {
    // So `$n.element` works in a batch whatever made the element, not only after
    // create_element and connect; `id` stays for the clients that read it.
    let mut host = Headless::new();
    let root = run(&mut host, "get_document", json!({}))["root"].clone();
    let calls = json!([
        { "tool": "create_model_element",
          "arguments": { "owner": root, "tool": "class", "name": "Konto" } },
        { "tool": "add_member", "arguments": { "owner": "$0.element", "text": "- saldo : double" } },
        { "tool": "update_element", "arguments": { "id": "$1.element", "static": true } },
        { "tool": "create_diagram", "arguments": { "kind": "sequence", "name": "S" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$3.diagram", "tool": "lifeline", "name": "a" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$3.diagram", "tool": "lifeline", "name": "b" } },
        { "tool": "connect", "arguments": { "diagram": "$3.diagram", "tool": "message",
            "source": "$4.element", "target": "$5.element", "name": "m" } },
        { "tool": "wrap_in_fragment",
          "arguments": { "diagram": "$3.diagram", "items": ["$6.element"], "operator": "alt" } },
        { "tool": "add_operand", "arguments": { "fragment": "$7.element", "guard": "x" } },
        { "tool": "update_element", "arguments": { "id": "$8.element", "guard": "y" } },
        { "tool": "create_structogram", "arguments": { "structogram": {
            "title": "t", "body": [{ "kind": "instruction", "text": "x := 1" }] } } },
        { "tool": "get_structogram", "arguments": { "id": "$10.element" } },
        { "tool": "get_structogram", "arguments": { "id": "$10.diagram" } },
    ]);
    let done = run(&mut host, "batch", json!({ "calls": calls }));
    let results = done["results"].as_array().unwrap();
    for i in [0, 1, 7, 8, 10] {
        assert!(results[i]["element"].is_string(), "call {i}: {}", results[i]);
        assert_eq!(results[i]["element"], results[i]["id"], "call {i}");
    }
    assert_eq!(results[10]["diagram"], results[10]["id"]);
}

#[test]
fn files_only_in_allowed_folders() {
    let dir = std::env::temp_dir().join("umlcraft-mcp-files");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // The server keeps canonical paths: on CI runners the temporary folder is a short
    // name (Windows) or a symbolic link (macOS).
    let dir = crate::host::absolute(&dir);
    let access = Access { folders: vec![dir.clone()], any: false };
    let mut host = Headless::new();
    let file = dir.join("shop");
    let saved = call(&mut host, &access, "save_file", &json!({ "path": file }));
    assert!(!saved.is_error, "{saved:?}");
    assert!(dir.join("shop.ucraft").exists());
    assert!(!host.document().is_dirty());
    let outside = std::env::temp_dir().join("elsewhere.ucraft");
    assert!(call(&mut host, &access, "save_file", &json!({ "path": outside })).is_error);
    // Missing folders are made, as for images, but only in an allowed folder.
    let nested = dir.join("new").join("sub").join("p.ucraft");
    let saved = call(&mut host, &access, "save_file", &json!({ "path": nested }));
    assert!(!saved.is_error, "{saved:?}");
    assert!(nested.exists());
    let elsewhere = std::env::temp_dir().join("umlcraft-mcp-not-allowed");
    let _ = std::fs::remove_dir_all(&elsewhere);
    let outside = elsewhere.join("p.ucraft");
    assert!(call(&mut host, &access, "save_file", &json!({ "path": outside })).is_error);
    assert!(!elsewhere.exists());
    // A folder is no file.
    let folder = format!("{}{}", dir.join("folder").display(), std::path::MAIN_SEPARATOR);
    assert!(call(&mut host, &access, "save_file", &json!({ "path": folder })).is_error);
    assert!(!dir.join("folder").exists());
    // The project's own file may lie outside, but its folder is not made again there.
    let mut own = Headless::with(new_document("Own"), Some(outside.clone()));
    assert!(call(&mut own, &access, "save_file", &json!({})).is_error);
    assert!(!elsewhere.exists());
    let d = host.active_diagram().unwrap().to_string();
    let png = call(
        &mut host,
        &access,
        "export_image",
        &json!({ "diagram": d, "path": dir.join("a.svg") }),
    );
    assert!(!png.is_error, "{png:?}");
    let opened = call(&mut host, &access, "open_file", &json!({ "path": dir.join("shop.ucraft") }));
    assert!(!opened.is_error, "{opened:?}");
    assert_eq!(host.path(), Some(dir.join("shop.ucraft").as_path()));
}

#[test]
fn replies_survive_the_trip_through_json() {
    let reply = Reply {
        content: vec![Content::Text("a".into()), Content::Image(vec![1, 2, 3])],
        is_error: true,
    };
    assert_eq!(Reply::from_json(&reply.to_json()), reply);
}

#[test]
fn clients_set_how_diagrams_are_drawn() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "c" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
              "tool": "class", "name": "Konto", "abstract": true } },
            { "tool": "add_member", "arguments": { "owner": "$1.element",
              "text": "- stand : int" } },
            { "tool": "add_member", "arguments": { "owner": "$1.element",
              "text": "+ zahle(betrag : int) : void" } },
        ]),
    );
    let svg = |host: &mut Headless| {
        let (doc, measure) = host.parts();
        let d = id(&r[0]["diagram"]).parse().unwrap();
        svg::render(&scene::build(doc.model(), d, &Preview::default(), measure), measure)
    };
    // As in Astah until a client says otherwise.
    let off = json!({ "visibility_column": false, "abstract_stereotype": false,
                      "read_only_constraint": false, "guillemets": false });
    assert_eq!(run(&mut host, "get_document", json!({}))["display"], off);
    assert!(svg(&mut host).contains(">- stand : int<"));
    // Then the visibility in its own column, a minus as wide as a plus, and «abstract».
    let on = run(
        &mut host,
        "set_display",
        json!({ "visibility_column": true, "abstract_stereotype": true }),
    );
    assert_eq!(on["visibility_column"], true);
    assert_eq!(on["read_only_constraint"], false);
    let drawn = svg(&mut host);
    assert!(drawn.contains(">\u{2212}<") && drawn.contains(">stand : int<"), "{drawn}");
    assert!(drawn.contains("abstract&gt;&gt;") || drawn.contains("«abstract»"), "{drawn}");
    // Omitted flags stay, also for a new project.
    let kept = run(&mut host, "set_display", json!({ "read_only_constraint": true }));
    assert_eq!(kept["visibility_column"], true);
    run(&mut host, "new_document", json!({}));
    assert_eq!(run(&mut host, "get_document", json!({}))["display"]["visibility_column"], true);
    assert!(
        call(&mut host, &Access::default(), "set_display", &json!({ "visibility_column": 1 }))
            .is_error
    );
}

#[test]
fn stereotypes_and_keywords_stand_in_guillemets_when_asked() {
    let mut host = Headless::new();
    let r = build(
        &mut host,
        json!([
            { "tool": "create_diagram", "arguments": { "kind": "use_case", "name": "u" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "useCase", "name": "Ausleihen" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram",
                "tool": "useCase", "name": "Ausweis prüfen" } },
            { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "include",
                "source": "$1.element", "target": "$2.element" } },
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "c" } },
            { "tool": "create_element", "arguments": { "diagram": "$4.diagram",
                "tool": "interface", "name": "Zahlbar" } },
            { "tool": "add_member", "arguments": { "owner": "$5.element",
                "text": "+ zahle() : void" } },
            { "tool": "update_element", "arguments": { "id": "$6.id",
                "stereotypes": ["query"] } },
            { "tool": "create_diagram", "arguments": { "kind": "sequence", "name": "s" } },
            { "tool": "create_element", "arguments": { "diagram": "$8.diagram",
                "tool": "lifeline", "name": "a" } },
            { "tool": "create_element", "arguments": { "diagram": "$8.diagram",
                "tool": "lifeline", "name": "b" } },
            { "tool": "connect", "arguments": { "diagram": "$8.diagram",
                "tool": "createMessage", "source": "$9.element", "target": "$10.element" } },
        ]),
    );
    let svg = |host: &mut Headless, d: &Value| {
        let (doc, measure) = host.parts();
        let d = id(d).parse().unwrap();
        svg::render(&scene::build(doc.model(), d, &Preview::default(), measure), measure)
    };
    let texts = ["include", "interface", "query", "create"];
    let drawn = |host: &mut Headless| {
        [&r[0]["diagram"], &r[4]["diagram"], &r[8]["diagram"]].map(|d| svg(host, d)).join("")
    };
    // As in Astah by default.
    let astah = drawn(&mut host);
    for text in texts {
        assert!(astah.contains(&format!("&lt;&lt;{text}&gt;&gt;")), "{text}: {astah}");
    }
    // In guillemets once asked for (#86).
    let on = run(&mut host, "set_display", json!({ "guillemets": true }));
    assert_eq!(on["guillemets"], true);
    let uml = drawn(&mut host);
    for text in texts {
        assert!(uml.contains(&format!("«{text}»")), "{text}: {uml}");
    }
    assert!(!uml.contains("&lt;&lt;"), "{uml}");
}

#[test]
fn a_far_flung_node_does_not_make_a_huge_image() {
    let mut host = Headless::new();
    let d = host.active_diagram().unwrap().to_string();
    let near = json!({ "diagram": d, "tool": "class", "name": "Near", "x": 0, "y": 0 });
    run(&mut host, "create_element", near);
    let far = json!({ "diagram": d, "tool": "class", "name": "Far" });
    let far = run(&mut host, "create_element", far);
    let away = json!({ "diagram": d, "node": far["presentation"], "x": 40000, "y": 40000 });
    run(&mut host, "move_node", away);
    let image = call(&mut host, &Access::default(), "render_diagram", &json!({ "diagram": d }));
    let Content::Image(png) = &image.content[0] else { panic!("{image:?}") };
    let (w, h) = png_size(png);
    assert!(f64::from(w) * f64::from(h) <= MAX_PIXELS * 1.01, "{w} x {h}");
    assert!(w > 1000 && (w as i64 - h as i64).abs() < 50, "{w} x {h}");

    let dir = std::env::temp_dir().join("umlcraft-mcp-far");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::host::absolute(&dir);
    let access = Access { folders: vec![dir.clone()], any: false };
    let export = json!({ "diagram": d, "path": dir.join("far.png") });
    let written = call(&mut host, &access, "export_image", &export);
    assert!(!written.is_error, "{written:?}");
    let (w, h) = png_size(&std::fs::read(dir.join("far.png")).unwrap());
    assert!(f64::from(w) * f64::from(h) <= MAX_PIXELS * 1.01, "{w} x {h}");

    // Small diagrams keep their scale: two pixels per unit.
    let mut host = Headless::new();
    let d = host.active_diagram().unwrap().to_string();
    let one = json!({ "diagram": d, "tool": "class", "name": "One", "x": 0, "y": 0 });
    run(&mut host, "create_element", one);
    let args = json!({ "diagram": d, "scale": 2 });
    let image = call(&mut host, &Access::default(), "render_diagram", &args);
    let Content::Image(big) = &image.content[0] else { panic!("{image:?}") };
    let image = call(&mut host, &Access::default(), "render_diagram", &json!({ "diagram": d }));
    let Content::Image(small) = &image.content[0] else { panic!("{image:?}") };
    let ((bw, _), (sw, _)) = (png_size(big), png_size(small));
    assert!((bw as i64 - 2 * sw as i64).abs() <= 2, "{bw} {sw}");
}

#[test]
fn exports_replace_files_only_when_asked() {
    let dir = std::env::temp_dir().join("umlcraft-mcp-overwrite");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::host::absolute(&dir);
    let access = Access { folders: vec![dir.clone()], any: false };
    let mut host = Headless::new();
    let d = host.active_diagram().unwrap().to_string();
    let svg = dir.join("a.svg");
    std::fs::write(&svg, "mine").unwrap();
    let export = json!({ "diagram": d, "path": svg });
    assert!(call(&mut host, &access, "export_image", &export).is_error);
    assert_eq!(std::fs::read_to_string(&svg).unwrap(), "mine");
    let export = json!({ "diagram": d, "path": svg, "overwrite": true });
    assert!(!call(&mut host, &access, "export_image", &export).is_error);
    assert!(std::fs::read_to_string(&svg).unwrap().contains("<svg"));

    let er = json!({ "kind": "entity_relationship", "name": "Shop" });
    let er = run(&mut host, "create_diagram", er);
    let entity = json!({ "diagram": er["diagram"], "tool": "erEntity", "name": "Customer" });
    run(&mut host, "create_element", entity);
    // A document is never written over: the SQL goes into a .sql file beside it.
    let thesis = dir.join("thesis.docx");
    std::fs::write(&thesis, "my thesis").unwrap();
    let export = json!({ "diagram": er["diagram"], "path": thesis });
    let out = call(&mut host, &access, "export_sql", &export);
    assert!(!out.is_error, "{out:?}");
    let out = ok(out);
    assert_eq!(std::fs::read_to_string(&thesis).unwrap(), "my thesis");
    let sql = dir.join("thesis.docx.sql");
    assert_eq!(out["written"], json!(sql));
    assert!(std::fs::read_to_string(&sql).unwrap().contains("CREATE TABLE"));
    let again = json!({ "diagram": er["diagram"], "path": sql });
    assert!(call(&mut host, &access, "export_sql", &again).is_error);
    let again = json!({ "diagram": er["diagram"], "path": sql, "overwrite": true });
    assert!(!call(&mut host, &access, "export_sql", &again).is_error);
}

#[test]
fn a_batch_keeps_to_what_a_rollback_takes_back() {
    let dir = std::env::temp_dir().join("umlcraft-mcp-batch-files");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::host::absolute(&dir);
    let access = Access { folders: vec![dir.clone()], any: false };
    let mut host = Headless::new();
    let d = host.active_diagram().unwrap().to_string();
    for (tool, arguments) in [
        ("save_file", json!({ "path": dir.join("p.ucraft") })),
        ("set_display", json!({ "guillemets": true })),
    ] {
        let calls = json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "C" } },
            { "tool": tool, "arguments": arguments },
        ]);
        let refused = call(&mut host, &access, "batch", &json!({ "calls": calls }));
        assert!(refused.is_error, "{tool}");
    }
    assert!(!dir.join("p.ucraft").exists());
    assert!(!host.display().guillemets);
    assert!(host.path().is_none());

    // Files written before a failing call stay, and the error says so.
    let calls = json!([
        { "tool": "export_image", "arguments": { "diagram": d, "path": dir.join("a.svg") } },
        { "tool": "create_element", "arguments": { "diagram": d, "tool": "nonsense" } },
    ]);
    let refused = call(&mut host, &access, "batch", &json!({ "calls": calls }));
    let Content::Text(why) = &refused.content[0] else { panic!() };
    assert!(refused.is_error && why.contains("a.svg"), "{why}");
    assert!(dir.join("a.svg").exists());
}

#[test]
fn only_index_paths_are_references_in_a_batch() {
    assert!(is_reference("$0") && is_reference("$12.element") && is_reference("$1.items.0"));
    for text in ["$", "$x := 0", "$price : int", "$Config", "$0.", "$0.a b", "$1x"] {
        assert!(!is_reference(text), "{text}");
    }
    let mut host = Headless::new();
    let root = run(&mut host, "get_document", json!({}))["root"].clone();
    let calls = json!([
        { "tool": "create_model_element",
          "arguments": { "owner": root, "tool": "class", "name": "$Config" } },
        { "tool": "add_member",
          "arguments": { "owner": "$0.element", "text": "- $price : int" } },
        { "tool": "update_element", "arguments": { "id": "$1.element", "documentation": "$$1" } },
    ]);
    let done = run(&mut host, "batch", json!({ "calls": calls }));
    let model = host.document().model();
    let class = model.get(id(&done["results"][0]["element"]).parse().unwrap()).unwrap();
    assert_eq!(class.name(), "$Config");
    let price = model.get(id(&done["results"][1]["element"]).parse().unwrap()).unwrap();
    assert_eq!(price.name(), "$price");
    assert_eq!(price.data().documentation, "$1");
}

/// A host that panics when `tool` makes its change, as a bug in a tool would.
struct Panicking {
    inner: Headless,
    tool: &'static str,
}

impl Host for Panicking {
    fn document(&self) -> &umlcraft_model::Document {
        self.inner.document()
    }
    fn parts(&mut self) -> (&mut umlcraft_model::Document, &dyn TextMeasure) {
        self.inner.parts()
    }
    fn label(&self, tool: &str) -> String {
        assert_ne!(tool, self.tool, "a bug");
        tool.to_owned()
    }
    fn path(&self) -> Option<&Path> {
        self.inner.path()
    }
    fn live(&self) -> bool {
        false
    }
    fn active_diagram(&self) -> Option<Id> {
        self.inner.active_diagram()
    }
    fn selection(&self) -> Vec<Id> {
        Vec::new()
    }
    fn show_diagram(&mut self, diagram: Id) {
        self.inner.show_diagram(diagram);
    }
    fn replace(
        &mut self,
        doc: umlcraft_model::Document,
        path: Option<PathBuf>,
    ) -> Result<(), String> {
        self.inner.replace(doc, path)
    }
    fn saved(&mut self, path: PathBuf) {
        self.inner.saved(path);
    }
    fn select(&mut self, _presentations: Vec<Id>) {}
    fn set_display(&mut self, display: Display) {
        self.inner.set_display(display);
    }
}

#[test]
fn a_panicking_tool_answers_with_an_error_and_changes_nothing() {
    let mut host = Panicking { inner: Headless::new(), tool: "connect" };
    let model = format!("{:?}", host.document().model().get(host.document().model().root()));
    let depth = host.document().undo_depth();
    let calls = json!([
        { "tool": "create_diagram", "arguments": { "kind": "class", "name": "Shop" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "A" } },
        { "tool": "create_element",
          "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "B" } },
        { "tool": "connect", "arguments": { "diagram": "$0.diagram", "tool": "association",
                                            "source": "$1.element", "target": "$2.element" } },
    ]);
    let reply = call_guarded(&mut host, &Access::default(), "batch", &json!({ "calls": calls }));
    let Content::Text(why) = &reply.content[0] else { panic!() };
    assert!(reply.is_error && why.contains("internal error in `batch`"), "{why}");
    assert_eq!(host.document().undo_depth(), depth);
    assert_eq!(format!("{:?}", host.document().model().get(host.document().model().root())), model);
    // The next call works.
    let made = call_guarded(
        &mut host,
        &Access::default(),
        "create_diagram",
        &json!({ "kind": "class", "name": "Next" }),
    );
    assert!(!made.is_error, "{made:?}");
    host.document().model().check().unwrap();

    // 100 parallel branches were such a panic.
    let branches: Vec<Value> = vec![json!([]); 100];
    let tree = json!({ "title": "t", "body": [{ "kind": "parallel", "branches": branches }] });
    let made = call_guarded(
        &mut host,
        &Access::default(),
        "create_structogram",
        &json!({ "structogram": tree }),
    );
    let Content::Text(why) = &made.content[0] else { panic!() };
    assert!(made.is_error && why.contains("at most 99"), "{why}");
}

/// The text of the error `name` answers `args` with.
fn refusal(host: &mut Headless, name: &str, args: Value) -> String {
    let reply = call(host, &Access::default(), name, &args);
    assert!(reply.is_error, "{name} {args}");
    match &reply.content[0] {
        Content::Text(text) => text.clone(),
        Content::Image(_) => unreachable!(),
    }
}

/// Where a tool takes a `diagram`, its name works as well as its id, a structogram's title
/// too; a name that several have is refused with their ids.
#[test]
fn diagrams_are_found_by_name() {
    let mut host = Headless::new();
    let document = run(&mut host, "get_document", json!({}));
    let first = document["diagrams"][0]["id"].clone();
    let name = document["diagrams"][0]["name"].as_str().unwrap().to_owned();
    let shown = run(&mut host, "get_diagram", json!({ "diagram": format!(" {name} ") }));
    assert_eq!(shown["id"], first);

    let shop = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
    let order = json!({ "diagram": "Shop", "tool": "class", "name": "Order" });
    let order = run(&mut host, "create_element", order);
    let shown = run(&mut host, "get_diagram", json!({ "diagram": shop["diagram"] }));
    assert_eq!(shown["nodes"][0]["element"], order["element"]);
    run(&mut host, "open_diagram", json!({ "diagram": name }));
    assert_eq!(json!(host.active_diagram()), first);

    let tree =
        json!({ "title": "total(n)", "body": [{ "kind": "instruction", "text": "s := 0" }] });
    let structogram = run(&mut host, "create_structogram", json!({ "structogram": tree }));
    run(&mut host, "render_diagram", json!({ "diagram": "total(n)" }));
    let opened = run(&mut host, "open_diagram", json!({ "diagram": "total(n)" }));
    assert_eq!(opened["shown_diagram"], structogram["id"]);

    assert!(
        refusal(&mut host, "get_diagram", json!({ "diagram": "Nowhere" })).contains("`Nowhere`")
    );
    let twin = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
    let why = refusal(&mut host, "get_diagram", json!({ "diagram": "Shop" }));
    for d in [&shop["diagram"], &twin["diagram"]] {
        assert!(why.contains(d.as_str().unwrap()), "{why}");
    }
    // An id still goes first, also for a diagram named like another's id.
    let renamed = json!({ "id": twin["diagram"], "name": first });
    run(&mut host, "update_element", renamed);
    let shown = run(&mut host, "get_diagram", json!({ "diagram": first }));
    assert_eq!(shown["id"], first);

    // Where only diagrams go, a structogram of the same name does not count; an empty text
    // names none, rather than one without a name.
    let same = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "total(n)" }));
    let shown = run(&mut host, "get_diagram", json!({ "diagram": "total(n)" }));
    assert_eq!(shown["id"], same["diagram"]);
    let why = refusal(&mut host, "render_diagram", json!({ "diagram": "total(n)" }));
    assert!(why.contains(structogram["id"].as_str().unwrap()), "{why}");
    run(&mut host, "create_diagram", json!({ "kind": "class", "name": "" }));
    refusal(&mut host, "render_diagram", json!({ "diagram": " " }));
}
