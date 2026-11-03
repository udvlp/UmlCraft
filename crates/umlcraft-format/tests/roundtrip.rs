use pretty_assertions::assert_eq;
use umlcraft_format::{
    FORMAT_VERSION, FormatError, from_str, from_str_versioned, load, save, to_string,
};
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    ElementData, ElementKind, MAX_TYPE_DEPTH, ModelError, Multiplicity, Point, Rect, Shape,
    TypeExpr, TypeRef, Visibility,
};

#[test]
fn sample_roundtrips_byte_for_byte() {
    let (doc, _) = order_system();
    let json = to_string(doc.model());
    let model = from_str(&json).unwrap();
    assert_eq!(model, *doc.model());
    assert_eq!(to_string(&model), json);
}

#[test]
fn er_diagrams_survive_and_deleting_an_entity_takes_its_lines() {
    use umlcraft_model::{DiagramKind, er::*};
    let (mut doc, _) = order_system();
    let root = doc.model().root();
    let mut ids = (root, root, root, root);
    doc.edit("er", |tx| {
        let package = tx.create(root, ElementData::new("Flights", ElementKind::Package))?;
        let diagram = tx.create(
            package,
            ElementData::new("Booking", ElementKind::diagram(DiagramKind::EntityRelationship)),
        )?;
        let mut ie = ElementKind::diagram(DiagramKind::EntityRelationship);
        if let ElementKind::Diagram(p) = &mut ie {
            p.notation = ErNotation::Idef1x;
            p.er_display = ErDisplay::Keys;
            p.physical_names = true;
        }
        tx.create(package, ElementData::new("Tables", ie))?;
        let weak =
            ErEntityProps { weak: true, physical_name: "TICKET".into(), ..Default::default() };
        let customer = tx.create(
            package,
            ElementData::new("Kunde", ElementKind::ErEntity(Default::default())),
        )?;
        let ticket = tx.create(package, ElementData::new("Ticket", ElementKind::ErEntity(weak)))?;
        let key = ErAttributeProps {
            key: Some(ErKey::Partial),
            datatype: "INT".into(),
            not_null: true,
            ..Default::default()
        };
        let id = tx.create(ticket, ElementData::new("ID", ElementKind::ErAttribute(key)))?;
        let books = tx.create(
            package,
            ElementData::new(
                "bucht",
                ElementKind::ErRelationship(ErRelationshipProps { identifying: true }),
            ),
        )?;
        let mut part = ErParticipationProps::new(books, customer);
        part.cardinality = Some(Cardinality::ONE_ONE);
        part.total = true;
        let part = tx.create(books, ElementData::new("", ElementKind::ErParticipation(part)))?;
        let column = ErAttributeProps {
            foreign_key: Some(books),
            unique: true,
            physical_name: "CUSTOMER_ID".into(),
            ..Default::default()
        };
        tx.create(ticket, ElementData::new("customer", ElementKind::ErAttribute(column)))?;
        tx.create(
            books,
            ElementData::new(
                "",
                ElementKind::ErParticipation(ErParticipationProps::new(books, ticket)),
            ),
        )?;
        let is_a = tx.create(
            package,
            ElementData::new(
                "",
                ElementKind::ErIsA(ErIsAProps {
                    total: true,
                    disjoint: Some(true),
                    ..Default::default()
                }),
            ),
        )?;
        tx.create(
            is_a,
            ElementData::new(
                "",
                ElementKind::ErSubtyping(ErSubtypingProps {
                    is_a,
                    entity: customer,
                    supertype: true,
                }),
            ),
        )?;
        let node = |tx: &mut umlcraft_model::Tx<'_>, e, x| {
            tx.add_node(diagram, Some(e), Rect::new(x, 0.0, 100.0, 40.0))
        };
        let (c, t, i, b) = (
            node(tx, customer, 0.0)?,
            node(tx, ticket, 300.0)?,
            node(tx, id, 300.0)?,
            node(tx, books, 150.0)?,
        );
        tx.add_edge(diagram, Some(part), b, c, Vec::new())?;
        tx.add_edge(diagram, Some(id), t, i, Vec::new())?;
        ids = (customer, part, is_a, ticket);
        Ok(())
    })
    .unwrap();
    let json = to_string(doc.model());
    assert_eq!(from_str(&json).unwrap(), *doc.model());
    for written in [
        "\"type\": \"erEntity\"",
        "\"weak\": true",
        "\"physicalName\": \"TICKET\"",
        "\"key\": \"partial\"",
        "\"notNull\": true",
        "\"identifying\": true",
        "\"cardinality\": \"[1,1]\"",
        "\"type\": \"erIsA\"",
        "\"supertype\": true",
        "\"diagram\": \"entityRelationship\"",
        "\"notation\": \"idef1x\"",
        "\"erDisplay\": \"keys\"",
        "\"physicalNames\": true",
        "\"foreignKey\": ",
        "\"unique\": true",
    ] {
        assert!(json.contains(written), "{written} missing in {json}");
    }
    let (customer, part, is_a, ticket) = ids;
    doc.edit("delete", |tx| tx.delete(customer)).unwrap();
    let model = doc.model();
    assert!(model.get(part).is_none(), "the participation goes with its entity");
    assert!(model.children(is_a).is_empty(), "so does the subtyping");
    assert!(model.get(ticket).is_some());
    // Deleting the relationship keeps its foreign key column, as a plain column.
    let books = model.elements().find(|e| e.name() == "bucht").unwrap().id();
    doc.edit("delete", |tx| tx.delete(books)).unwrap();
    let column = doc.model().elements().find(|e| e.name() == "customer").unwrap();
    assert!(matches!(column.kind(), ElementKind::ErAttribute(p) if p.foreign_key.is_none()));
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
fn templates_and_type_expressions_survive() {
    use umlcraft_model::{Qualifier, TemplateParameterKind, TemplateParameterProps, TypeExpr};
    let (mut doc, ids) = order_system();
    doc.edit("generics", |tx| {
        let bound = TemplateParameterProps {
            bounds: vec![TypeRef::Element(ids.customer)],
            ..Default::default()
        };
        let t =
            tx.create(ids.order, ElementData::new("T", ElementKind::TemplateParameter(bound)))?;
        let size = TemplateParameterProps {
            parameter: TemplateParameterKind::Value,
            type_ref: Some(TypeRef::Text("int".into())),
            default: Some(TypeRef::Text("10".into())),
            is_variadic: true,
            ..Default::default()
        };
        tx.create(ids.order, ElementData::new("N", ElementKind::TemplateParameter(size)))?;
        let items = TypeRef::Expr(Box::new(TypeExpr {
            base: TypeRef::Text("List".into()),
            arguments: vec![TypeRef::Element(t)],
            prefix: String::new(),
            modifier: "[]".into(),
        }));
        tx.create(ids.order, ElementData::new("items", ElementKind::attribute(Some(items))))?;
        let mut op = ElementKind::operation(None);
        if let ElementKind::Operation(p) = &mut op {
            p.is_leaf = true;
        }
        tx.create(ids.order, ElementData::new("total", op))?;
        tx.update(ids.order_customer, |d| {
            if let ElementKind::Association(p) = &mut d.kind {
                let end = &mut p.ends[1];
                end.is_read_only = true;
                end.is_static = true;
                end.default_value = "none".into();
                end.qualifiers = vec![Qualifier {
                    name: "key".into(),
                    type_ref: Some(TypeRef::Element(ids.order)),
                }];
            }
        })
    })
    .unwrap();
    let json = to_string(doc.model());
    assert_eq!(from_str(&json).unwrap(), *doc.model());
    for key in ["\"templateParameter\"", "\"expr\"", "\"bounds\"", "\"isLeaf\"", "\"qualifiers\""] {
        assert!(json.contains(key), "{key} in {json}");
    }
}

#[test]
fn output_is_stable_and_readable() {
    let (doc, ids) = order_system();
    let json = to_string(doc.model());
    let head = format!("{{\n  \"format\": \"umlcraft\",\n  \"formatVersion\": {FORMAT_VERSION},\n");
    assert!(json.starts_with(&head));
    assert!(json.contains("\"shape\": \"lifeline\""));
    assert!(json.ends_with("}\n"));
    // Defaults are omitted, enums use readable names.
    assert!(json.contains("\"type\": \"class\""));
    assert!(json.contains("\"multiplicity\": \"*\""));
    assert!(json.contains("\"visibility\": \"private\""));
    assert!(!json.contains("\"isAbstract\""));
    assert!(json.contains(&ids.class_diagram.to_string()));
}

/// A structogram is its content, element by element, next to the operation it shows.
#[test]
fn structograms_survive_and_read_line_by_line() {
    let (mut doc, ids) = order_system();
    let nsd = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../umlcraft-nsd/tests/fixtures/structorizer/43-bubblesort.nsd"),
    )
    .unwrap();
    let diagram = umlcraft_nsd::read(&nsd).unwrap();
    let model = doc.model();
    let order = model.get(ids.order).unwrap();
    let submit = *order
        .children()
        .iter()
        .find(|&&c| matches!(model.get(c).unwrap().kind(), ElementKind::Operation(_)))
        .unwrap();
    doc.edit("structogram", |tx| {
        let mut props = umlcraft_model::StructogramProps::new(&diagram);
        props.operation = Some(submit);
        tx.create(ids.order, ElementData::new("", ElementKind::Structogram(props)))
    })
    .unwrap();
    let json = to_string(doc.model());
    assert_eq!(from_str(&json).unwrap(), *doc.model());
    for key in ["\"type\": \"structogram\"", "\"operation\"", "\"structogram\": {", "\"body\": ["] {
        assert!(json.contains(key), "{key} in {json}");
    }
    // The file's header stays out of the project.
    assert!(!json.contains("\"author\"") && !json.contains("3.32-35"), "{json}");
}

#[test]
fn older_files_get_language_tags_under_umlcraft_names() {
    let (mut doc, ids) = order_system();
    doc.edit("tag", |tx| {
        tx.update(ids.customer, |d| {
            d.tagged_values = [("jude.java.transient", "true"), ("table", "CUSTOMER")]
                .map(|(key, value)| umlcraft_model::TaggedValue {
                    key: key.into(),
                    value: value.into(),
                })
                .to_vec();
        })
    })
    .unwrap();
    let v2 = to_string(doc.model()).replacen(
        &format!("\"formatVersion\": {FORMAT_VERSION}"),
        "\"formatVersion\": 2",
        1,
    );
    let model = from_str(&v2).unwrap();
    let keys: Vec<&str> = model
        .get(ids.customer)
        .unwrap()
        .data()
        .tagged_values
        .iter()
        .map(|t| t.key.as_str())
        .collect();
    assert_eq!(keys, ["java.transient", "table"]);
}

#[test]
fn only_older_files_get_their_language_tags_renamed() {
    let file = |version: u32| {
        format!(
            r#"{{"format":"umlcraft","formatVersion":{version},"root":"00000000-0000-0000-0000-000000000000","elements":[{{"id":"00000000-0000-0000-0000-000000000000","name":"m","type":"model","taggedValues":[{{"key":"jude.java.mine","value":"x"}}]}}]}}"#
        )
    };
    let key = |version| {
        let (model, read) = from_str_versioned(&file(version)).unwrap();
        assert_eq!(read, version);
        model.get(model.root()).unwrap().data().tagged_values[0].key.clone()
    };
    assert_eq!(key(2), "java.mine");
    assert_eq!(key(3), "jude.java.mine", "a current file keeps its own tags");
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
    // Version 1 files still load.
    let v1 = r#"{"format":"umlcraft","formatVersion":1,"root":"00000000-0000-0000-0000-000000000000","elements":[{"id":"00000000-0000-0000-0000-000000000000","name":"m","type":"model"}]}"#;
    assert!(from_str(v1).is_ok(), "{:?}", from_str(v1).err());
    assert_eq!(from_str_versioned(v1).unwrap().1, 1, "the file's own version");

    // Valid JSON, invalid model: the owner still lists a child whose record is gone.
    let (doc, ids) = order_system();
    let mut value: serde_json::Value = serde_json::from_str(&to_string(doc.model())).unwrap();
    let premium = ids.premium.to_string();
    value["elements"].as_array_mut().unwrap().retain(|e| e["id"] != premium.as_str());
    assert!(matches!(from_str(&value.to_string()), Err(FormatError::Model(_))));
}

/// A project file as UmlCraft writes it today must stay readable, field for field: renaming a
/// field or changing how a value is written fails here, unlike the round trips above. After a
/// deliberate format change, run with `UPDATE_GOLDEN=1` and commit the new file together with
/// a migration for the old one.
#[test]
fn golden_file_loads_unchanged() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/order-system.ucraft");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let (doc, _) = order_system();
        std::fs::write(&path, to_string(doc.model())).unwrap();
    }
    // A checkout with `core.autocrlf` may have turned the line ends into CRLF.
    let golden = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
    let model = from_str(&golden).unwrap();
    assert_eq!(to_string(&model), golden);
    // The sample's ids are new each time, so it is compared by its shape.
    let (doc, _) = order_system();
    let names = |m: &umlcraft_model::Model| {
        let mut names: Vec<(String, &'static str)> = m
            .to_records()
            .elements
            .iter()
            .map(|e| (e.data.name.clone(), e.data.kind.type_name()))
            .collect();
        names.sort();
        names
    };
    assert_eq!(names(&model), names(doc.model()));
}

/// What JSON cannot hold or read back is refused when it enters the model.
#[test]
fn unreadable_states_are_refused() {
    let (mut doc, ids) = order_system();
    let node = doc.model().shown_on(ids.customer)[0].1;
    for bad in [f32::NAN, f32::INFINITY] {
        let rect = Rect::new(bad, 0.0, 100.0, 50.0);
        let result = doc.edit("bounds", |tx| tx.set_bounds(ids.class_diagram, node, rect));
        assert!(matches!(result, Err(ModelError::NotFinite(_))), "{result:?}");
    }

    let nested = |depth: usize| {
        (1..depth).fold(TypeRef::Element(ids.order), |inner, _| {
            TypeRef::Expr(Box::new(TypeExpr {
                base: TypeRef::Text("List".into()),
                arguments: vec![inner],
                prefix: String::new(),
                modifier: String::new(),
            }))
        })
    };
    // The deepest type the model allows still loads.
    let deepest = ElementData::new("deep", ElementKind::attribute(Some(nested(MAX_TYPE_DEPTH))));
    doc.edit("deep", |tx| tx.create(ids.customer, deepest)).unwrap();
    assert_eq!(from_str(&to_string(doc.model())).unwrap(), *doc.model());
    let deeper =
        ElementData::new("deeper", ElementKind::attribute(Some(nested(MAX_TYPE_DEPTH + 1))));
    let result = doc.edit("deeper", |tx| tx.create(ids.customer, deeper));
    assert!(matches!(result, Err(ModelError::TypeTooDeep(_))), "{result:?}");
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

#[test]
fn a_byte_order_mark_and_a_missing_empty_view_are_fine() {
    let (doc, ids) = order_system();
    let json = to_string(doc.model());
    assert_eq!(from_str(&format!("\u{feff}{json}")).unwrap(), *doc.model());

    // An empty diagram whose view entry a merge dropped.
    let mut doc = doc;
    let d = doc
        .edit("diagram", |tx| {
            let kind = umlcraft_model::ElementKind::diagram(umlcraft_model::DiagramKind::Class);
            tx.create(ids.order, umlcraft_model::ElementData::new("empty", kind))
        })
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&to_string(doc.model())).unwrap();
    if let Some(views) = value["views"].as_object_mut() {
        views.remove(&d.to_string());
    }
    let model = from_str(&value.to_string()).unwrap();
    assert!(model.view(d).is_some_and(|v| v.presentations().is_empty()));
}

/// Every leaf of the sample project, replaced in turn by a few wrong values (another element's
/// id, a negative or huge number, null, an empty string): the file either loads into a
/// consistent model or is refused, and never panics. A deterministic stand-in for fuzzing.
#[test]
fn damaged_files_load_consistently_or_are_refused() {
    let (doc, ids) = order_system();
    let original: serde_json::Value = serde_json::from_str(&to_string(doc.model())).unwrap();
    let other = serde_json::Value::String(ids.order.to_string());
    let wrong = [
        other,
        serde_json::json!(-1),
        serde_json::json!(1e30),
        serde_json::Value::Null,
        serde_json::json!(""),
    ];
    // Paths to every leaf.
    fn leaves(value: &serde_json::Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        match value {
            serde_json::Value::Object(map) => {
                for (key, v) in map {
                    path.push(key.clone());
                    leaves(v, path, out);
                    path.pop();
                }
            }
            serde_json::Value::Array(items) => {
                for (i, v) in items.iter().enumerate() {
                    path.push(i.to_string());
                    leaves(v, path, out);
                    path.pop();
                }
            }
            _ => out.push(path.clone()),
        }
    }
    let mut paths = Vec::new();
    leaves(&original, &mut Vec::new(), &mut paths);
    assert!(paths.len() > 500, "{} leaves", paths.len());
    let (mut loaded, mut refused) = (0, 0);
    for path in &paths {
        for value in &wrong {
            let mut damaged = original.clone();
            let pointer: String = path
                .iter()
                .map(|p| format!("/{}", p.replace('~', "~0").replace('/', "~1")))
                .collect();
            *damaged.pointer_mut(&pointer).unwrap() = value.clone();
            match from_str(&damaged.to_string()) {
                Ok(model) => {
                    model.check().unwrap_or_else(|e| panic!("{pointer} = {value}: {e}"));
                    loaded += 1;
                }
                Err(_) => refused += 1,
            }
        }
    }
    assert!(loaded > 0 && refused > 0, "{loaded} loaded, {refused} refused");
}
