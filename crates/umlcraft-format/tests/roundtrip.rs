use pretty_assertions::assert_eq;
use umlcraft_format::{FORMAT_VERSION, FormatError, from_str, load, save, to_string};
use umlcraft_model::sample::order_system;
use umlcraft_model::{ElementData, ElementKind, Multiplicity, Point, Shape, TypeRef, Visibility};

#[test]
fn sample_roundtrips_byte_for_byte() {
    let (doc, _) = order_system();
    let json = to_string(doc.model());
    let model = from_str(&json).unwrap();
    assert_eq!(model, *doc.model());
    assert_eq!(to_string(&model), json);
}

#[test]
fn every_property_survives() {
    let (mut doc, ids) = order_system();
    doc.edit("decorate", |tx| {
        tx.update(ids.customer, |d| {
            d.visibility = Visibility::Package;
            d.stereotypes = vec!["entity".into()];
            d.tagged_values =
                vec![umlcraft_model::TaggedValue { key: "table".into(), value: "CUSTOMER".into() }];
            d.documentation = "Someone who orders.\nMultiline \"quoted\" ä".into();
        })?;
        let mut attr =
            ElementData::new("orders", ElementKind::attribute(Some(TypeRef::Element(ids.order))));
        if let ElementKind::Attribute(p) = &mut attr.kind {
            p.multiplicity = Some(Multiplicity::ONE_OR_MORE);
            p.default_value = "[]".into();
            p.is_static = true;
            p.is_read_only = true;
        }
        tx.create(ids.customer, attr)?;
        let node = tx.model().shown_on(ids.customer)[0].1;
        tx.update_presentation(ids.class_diagram, node, |p| {
            p.style.fill = Some("#ffeecc".parse().unwrap());
            p.style.font_size = Some(13.5);
        })?;
        let edge = tx.model().shown_on(ids.order_customer)[0].1;
        tx.update_presentation(ids.class_diagram, edge, |p| {
            if let Shape::Edge { source_anchor, labels, .. } = &mut p.shape {
                *source_anchor = Some(Point::new(0.25, 1.0));
                labels.target_role = Some(Point::new(9.5, -23.25));
            }
        })
    })
    .unwrap();
    let json = to_string(doc.model());
    assert_eq!(from_str(&json).unwrap(), *doc.model());
    // Edge fields are camelCase like everything else.
    assert!(json.contains("\"sourceAnchor\"") && json.contains("\"targetRole\""), "{json}");
    assert!(!json.contains("source_anchor"));
}

#[test]
fn output_is_stable_and_readable() {
    let (doc, ids) = order_system();
    let json = to_string(doc.model());
    assert!(json.starts_with("{\n  \"format\": \"umlcraft\",\n  \"formatVersion\": 1,\n"));
    assert!(json.ends_with("}\n"));
    // Defaults are omitted, enums use readable names.
    assert!(json.contains("\"type\": \"class\""));
    assert!(json.contains("\"multiplicity\": \"*\""));
    assert!(json.contains("\"visibility\": \"private\""));
    assert!(!json.contains("\"isAbstract\""));
    assert!(json.contains(&ids.class_diagram.to_string()));
}

#[test]
fn rejects_foreign_newer_and_broken_files() {
    assert!(matches!(from_str("{}"), Err(FormatError::NotAProject)));
    assert!(matches!(from_str("[1,2]"), Err(FormatError::NotAProject)));
    assert!(matches!(from_str("not json"), Err(FormatError::Json(_))));
    let newer = format!(
        r#"{{"format":"umlcraft","formatVersion":{},"root":"00000000-0000-0000-0000-000000000000","elements":[]}}"#,
        FORMAT_VERSION + 1
    );
    assert!(matches!(from_str(&newer), Err(FormatError::TooNew { .. })));

    // Valid JSON, invalid model: the owner still lists a child whose record is gone.
    let (doc, ids) = order_system();
    let mut value: serde_json::Value = serde_json::from_str(&to_string(doc.model())).unwrap();
    let premium = ids.premium.to_string();
    value["elements"].as_array_mut().unwrap().retain(|e| e["id"] != premium.as_str());
    assert!(matches!(from_str(&value.to_string()), Err(FormatError::Model(_))));
}

#[test]
fn save_and_load_files() {
    let (doc, _) = order_system();
    let dir = std::env::temp_dir().join(format!("umlcraft-format-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("order.ucraft");
    save(doc.model(), &path).unwrap();
    save(doc.model(), &path).unwrap(); // overwriting works too
    assert_eq!(load(&path).unwrap(), *doc.model());
    assert!(!dir.join("order.ucraft.tmp").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
