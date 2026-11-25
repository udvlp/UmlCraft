//! Structograms.

use super::*;

#[test]
fn structograms_are_made_read_and_changed() {
    let mut host = Headless::new();
    let root = run(&mut host, "get_document", json!({}))["root"].clone();
    let class = json!({ "owner": root, "tool": "class", "name": "Cart" });
    let class = run(&mut host, "create_model_element", class);
    let op = run(
        &mut host,
        "add_member",
        json!({ "owner": class["id"], "text": "+ total(n : int) : int" }),
    );
    let tree = json!({ "title": "total(n)", "kind": "sub", "body": [
        { "kind": "instruction", "text": "s := 0" },
        { "kind": "for", "text": "for i := 1 to n", "body": [
            { "kind": "instruction", "text": "s := s + i" } ] },
        { "kind": "jump", "text": "return s" } ] });
    let made =
        run(&mut host, "create_structogram", json!({ "structogram": tree, "operation": op["id"] }));
    assert_eq!(made["owner"], class["id"]);
    assert_eq!(made["operation"], op["id"]);
    let got = run(&mut host, "get_structogram", json!({ "id": made["id"] }));
    assert_eq!(got["structogram"]["body"], tree["body"]);
    let document = run(&mut host, "get_document", json!({}));
    assert_eq!(document["structograms"][0]["title"], "total(n)");

    // Selecting in it: an element by its path, the whole, nothing.
    let s = &made["id"];
    let element = run(&mut host, "select", json!({ "diagram": s, "path": "body.1.body.0" }));
    assert_eq!(element["path"], "body.1.body.0");
    let whole = run(&mut host, "select", json!({ "diagram": s, "ids": [s] }));
    assert_eq!(whole["whole"], true);
    let none = run(&mut host, "select", json!({ "diagram": s, "ids": [] }));
    assert_eq!(none["whole"], false);
    let refused =
        |host: &mut Headless, args: Value| call(host, &Access::default(), "select", &args).is_error;
    assert!(refused(&mut host, json!({ "diagram": s, "path": "body.7" })));
    assert!(refused(&mut host, json!({ "diagram": s, "ids": [class["id"]] })));
    let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
    assert!(refused(&mut host, json!({ "diagram": d["diagram"], "path": "body.0" })));

    let image =
        call(&mut host, &Access::default(), "render_diagram", &json!({ "diagram": made["id"] }));
    assert!(matches!(&image.content[0], Content::Image(png) if png.starts_with(b"\x89PNG")));

    // A new body, the title and keywords kept; then no operation.
    let body = json!([{ "kind": "if", "text": "n > 0?", "then": [], "else": [] }]);
    let changed = run(
        &mut host,
        "update_structogram",
        json!({ "id": made["id"], "structogram": { "body": body } }),
    );
    assert_eq!(changed["structogram"]["title"], "total(n)");
    assert_eq!(changed["structogram"]["kind"], "sub");
    assert_eq!(changed["structogram"]["body"], body);
    let unlinked =
        run(&mut host, "update_structogram", json!({ "id": made["id"], "operation": "" }));
    assert_eq!(unlinked["operation"], Value::Null);
    run(&mut host, "undo", json!({}));
    run(&mut host, "undo", json!({}));
    let back = run(&mut host, "get_structogram", json!({ "id": made["id"] }));
    assert_eq!(back["structogram"]["body"], tree["body"]);
    assert_eq!(back["operation"], op["id"]);

    // One element by its path.
    let element = json!({ "kind": "jump", "text": "return 0" });
    let edit = json!({ "id": made["id"], "action": "insert", "path": "body.1.body.0",
                       "element": element });
    let edited = run(&mut host, "edit_structogram", edit);
    assert_eq!(edited["structogram"]["body"][1]["body"][0], element);
    let remove = json!({ "id": made["id"], "action": "remove", "path": "body.9" });
    refusal(&mut host, "edit_structogram", remove);

    // As image and as Structorizer file.
    let dir = std::env::temp_dir().join("umlcraft-mcp-structogram");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::host::absolute(&dir);
    let access = Access::folders(vec![dir.clone()]);
    for name in ["total.png", "total.svg", "total.nsd"] {
        let export = json!({ "diagram": made["id"], "path": dir.join(name) });
        let written = call(&mut host, &access, "export_image", &export);
        assert!(!written.is_error, "{written:?}");
    }
    let nsd = std::fs::read_to_string(dir.join("total.nsd")).unwrap();
    assert!(umlcraft_nsd::read(&nsd).unwrap().text() == ["total(n)"]);
    let txt = json!({ "diagram": made["id"], "path": dir.join("total.txt") });
    assert!(call(&mut host, &access, "export_image", &txt).is_error);

    let wrong = json!({ "id": class["id"], "structogram": { "body": [] } });
    refusal(&mut host, "update_structogram", wrong);
    let broken = json!({ "structogram": { "title": "x", "body": [{ "kind": "goto" }] } });
    refusal(&mut host, "create_structogram", broken);
    host.document().model().check().unwrap();
}

#[test]
fn too_deep_structograms_are_refused_before_they_are_built() {
    let mut host = Headless::new();
    // 500 nested loops: the model takes 200 levels, the parser more than that.
    let mut body = json!([{ "kind": "instruction", "text": "x" }]);
    for _ in 0..500 {
        body = json!([{ "kind": "while", "text": "true", "body": body }]);
    }
    let tree = json!({ "title": "deep", "body": body });
    let reply =
        call(&mut host, &Access::default(), "create_structogram", &json!({ "structogram": tree }));
    assert!(reply.is_error);
    // 200 levels still go.
    let mut body = json!([{ "kind": "instruction", "text": "x" }]);
    for _ in 0..199 {
        body = json!([{ "kind": "while", "text": "true", "body": body }]);
    }
    run(&mut host, "create_structogram", json!({ "structogram": { "title": "ok", "body": body } }));
}
