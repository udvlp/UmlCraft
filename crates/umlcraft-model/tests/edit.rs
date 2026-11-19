use pretty_assertions::assert_eq;
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    DiagramKind, Document, ElementData, ElementKind, Id, Model, ModelError, Op, Rect, TypeRef,
};

fn class(name: &str) -> ElementData {
    ElementData::new(name, ElementKind::class())
}

#[test]
fn sample_model_is_consistent() {
    let (doc, ids) = order_system();
    doc.model().check().unwrap();
    assert!(!doc.is_dirty());
    let view = doc.model().view(ids.class_diagram).unwrap();
    assert_eq!(view.presentations().len(), 5);
    assert_eq!(doc.model().shown_on(ids.customer).len(), 1);
}

#[test]
fn undo_redo_restores_exact_state() {
    let (mut doc, ids) = order_system();
    let before = doc.model().clone();

    doc.edit("Rename", |tx| tx.rename(ids.customer, "Client")).unwrap();
    doc.edit("Delete order", |tx| tx.delete(ids.order)).unwrap();
    let after = doc.model().clone();
    assert!(doc.is_dirty());
    after.check().unwrap();

    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(doc.model().to_records(), before.to_records());
    assert_eq!(*doc.model(), before);
    assert!(!doc.is_dirty());
    assert!(!doc.undo(), "the sample starts without history");

    assert!(doc.redo());
    assert!(doc.redo());
    assert_eq!(doc.model().to_records(), after.to_records());
    assert!(!doc.redo());
}

#[test]
fn an_unsaved_document_stays_dirty_until_saved() {
    let (mut doc, ids) = order_system();
    doc.mark_unsaved();
    assert!(doc.is_dirty());
    doc.edit("a", |tx| tx.rename(ids.customer, "A")).unwrap();
    doc.undo();
    assert!(doc.is_dirty());
    doc.mark_saved();
    assert!(!doc.is_dirty());
}

#[test]
fn dirty_tracking_follows_the_save_point() {
    let (mut doc, ids) = order_system();
    doc.edit("a", |tx| tx.rename(ids.customer, "A")).unwrap();
    doc.mark_saved();
    doc.edit("b", |tx| tx.rename(ids.customer, "B")).unwrap();
    assert!(doc.is_dirty());
    doc.undo();
    assert!(!doc.is_dirty());
    doc.undo();
    assert!(doc.is_dirty());
    doc.redo();
    assert!(!doc.is_dirty());
    // Unchanged edits record nothing.
    doc.edit("same", |tx| tx.rename(ids.customer, "A")).unwrap();
    assert_eq!(doc.undo_label(), Some("a"));
    // Diverging from the saved state after undo stays dirty.
    doc.undo();
    doc.edit("c", |tx| tx.rename(ids.customer, "C")).unwrap();
    assert!(doc.is_dirty());
    assert!(!doc.can_redo());
}

#[test]
fn ids_are_unique_and_time_ordered() {
    let a = Id::generate();
    let b = Id::generate();
    assert!(a < b);
}

#[test]
fn delete_cascades_to_relationships_and_presentations() {
    let (mut doc, ids) = order_system();
    doc.edit("Delete customer", |tx| tx.delete(ids.customer)).unwrap();
    let model = doc.model();
    model.check().unwrap();

    assert!(!model.contains(ids.customer));
    // Strong referrers are gone ...
    assert!(!model.contains(ids.order_customer));
    assert!(!model.contains(ids.premium_generalization));
    // ... other classes stay.
    assert!(model.contains(ids.order));
    assert!(model.contains(ids.premium));
    // Customer's node and both edges left the class diagram.
    let view = model.view(ids.class_diagram).unwrap();
    assert_eq!(view.presentations().len(), 2);
    assert!(view.presentations().iter().all(|p| !p.is_edge()));
    // The lifeline only referred to Customer weakly; it survives, unlinked.
    let lifeline = model.get(ids.customer_lifeline).unwrap();
    assert_eq!(
        lifeline.kind(),
        &ElementKind::Lifeline(umlcraft_model::LifelineProps { represents: None })
    );
}

#[test]
fn a_structogram_outlives_its_operation_and_undo_takes_back_its_changes() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let made = umlcraft_nsd::Provenance { author: "a", date: "2026-10-30", origin: "t" };
    let empty = umlcraft_nsd::Diagram::new(umlcraft_nsd::RootKind::Program, &["f()".into()], &made);
    let (op, sg) = doc
        .edit("setup", |tx| {
            let class = tx.create(root, class("A"))?;
            let op = tx.create(class, ElementData::new("f", ElementKind::operation(None)))?;
            let mut props = umlcraft_model::StructogramProps::new(&empty);
            props.operation = Some(op);
            let sg = tx.create(class, ElementData::new("", ElementKind::Structogram(props)))?;
            Ok((op, sg))
        })
        .unwrap();
    // A structogram may not be owned by an operation, and nothing may depend on it.
    let in_op = ElementKind::Structogram(umlcraft_model::StructogramProps::new(&empty));
    assert!(doc.edit("bad", |tx| tx.create(op, ElementData::new("", in_op))).is_err());

    let mut changed = empty.clone();
    let slot = umlcraft_nsd::Slot { seq: vec![], index: 0 };
    let x = umlcraft_nsd::Element::instruction(&["x := 1".into()]);
    umlcraft_nsd::edit::insert(&mut changed, &slot, vec![x]).unwrap();
    doc.edit("insert", |tx| tx.set_structogram(sg, &changed)).unwrap();
    let body = |doc: &Document| match doc.model().get(sg).unwrap().kind() {
        ElementKind::Structogram(p) => (p.operation, p.structogram.children.elements.len()),
        _ => unreachable!(),
    };
    assert_eq!(body(&doc), (Some(op), 1));
    doc.undo();
    assert_eq!(body(&doc), (Some(op), 0));

    doc.edit("delete", |tx| tx.delete(op)).unwrap();
    doc.model().check().unwrap();
    assert_eq!(body(&doc), (None, 0));
}

#[test]
fn deleting_a_type_turns_attribute_types_into_text() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (address, attr) = doc
        .edit("setup", |tx| {
            let address = tx.create(root, class("Address"))?;
            let person = tx.create(root, class("Person"))?;
            let attr = tx.create(
                person,
                ElementData::new("home", ElementKind::attribute(Some(TypeRef::Element(address)))),
            )?;
            Ok((address, attr))
        })
        .unwrap();
    doc.edit("delete", |tx| tx.delete(address)).unwrap();
    assert_eq!(
        doc.model().get(attr).unwrap().kind(),
        &ElementKind::attribute(Some(TypeRef::Text("Address".into())))
    );
}

#[test]
fn template_parameters_type_members_and_unlink_when_deleted() {
    use umlcraft_model::{TemplateParameterProps, TypeExpr};
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (list, t, element, holder) = doc
        .edit("setup", |tx| {
            let list = tx.create(root, class("List"))?;
            let parameter = ElementKind::TemplateParameter(TemplateParameterProps::default());
            let t = tx.create(list, ElementData::new("T", parameter))?;
            let element = tx.create(
                list,
                ElementData::new("e", ElementKind::attribute(Some(TypeRef::Element(t)))),
            )?;
            let holder = tx.create(root, class("Holder"))?;
            let items = TypeRef::Expr(Box::new(TypeExpr {
                base: TypeRef::Element(list),
                arguments: vec![TypeRef::Element(holder)],
                prefix: String::new(),
                modifier: String::new(),
            }));
            tx.create(holder, ElementData::new("items", ElementKind::attribute(Some(items))))?;
            Ok((list, t, element, holder))
        })
        .unwrap();
    doc.model().check().unwrap();
    assert_eq!(doc.model().referrers(t).len(), 1);
    assert_eq!(doc.model().referrers(list).len(), 1, "the base of List<Holder>");

    doc.edit("delete T", |tx| tx.delete(t)).unwrap();
    assert_eq!(
        doc.model().get(element).unwrap().kind(),
        &ElementKind::attribute(Some(TypeRef::Text("T".into())))
    );
    doc.edit("delete List", |tx| tx.delete(list)).unwrap();
    let model = doc.model();
    model.check().unwrap();
    let items = model.children(holder)[0];
    let ElementKind::Attribute(p) = model.get(items).unwrap().kind() else { panic!() };
    let Some(TypeRef::Expr(e)) = &p.type_ref else { panic!("{p:?}") };
    assert_eq!(e.base, TypeRef::Text("List".into()));
    assert_eq!(e.arguments, vec![TypeRef::Element(holder)]);
}

#[test]
fn types_cannot_refer_to_non_classifiers() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let result = doc.edit("bad", |tx| {
        let p = tx.create(root, ElementData::new("p", ElementKind::Package))?;
        let c = tx.create(root, class("C"))?;
        tx.create(c, ElementData::new("a", ElementKind::attribute(Some(TypeRef::Element(p)))))
    });
    assert!(matches!(result, Err(ModelError::InvalidReference { .. })), "{result:?}");
}

#[test]
fn deleting_a_package_removes_its_diagrams() {
    let (mut doc, ids) = order_system();
    doc.edit("Delete domain", |tx| tx.delete(ids.domain)).unwrap();
    let model = doc.model();
    model.check().unwrap();
    assert!(model.view(ids.class_diagram).is_none());
    assert!(model.view(ids.use_case_diagram).is_some());
}

#[test]
fn self_typed_attribute_does_not_block_delete() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let node = doc
        .edit("setup", |tx| {
            let node = tx.create(root, class("Node"))?;
            tx.create(
                node,
                ElementData::new("next", ElementKind::attribute(Some(TypeRef::Element(node)))),
            )?;
            Ok(node)
        })
        .unwrap();
    doc.edit("delete", |tx| tx.delete(node)).unwrap();
    assert_eq!(doc.model().len(), 1);
}

#[test]
fn failed_edit_rolls_back_everything() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let before = doc.model().clone();
    let result = doc.edit("bad", |tx| {
        let a = tx.create(root, class("A"))?;
        // Classes cannot own packages.
        tx.create(a, ElementData::new("p", ElementKind::Package))?;
        Ok(())
    });
    assert!(matches!(result, Err(ModelError::InvalidOwner { .. })));
    assert_eq!(*doc.model(), before);
    assert!(!doc.can_undo());
}

#[test]
fn generalization_cycles_are_rejected() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (a, b, c) = doc
        .edit("setup", |tx| {
            let a = tx.create(root, class("A"))?;
            let b = tx.create(root, class("B"))?;
            let c = tx.create(root, class("C"))?;
            tx.create(root, ElementData::new("", ElementKind::generalization(a, b)))?;
            tx.create(root, ElementData::new("", ElementKind::generalization(b, c)))?;
            Ok((a, b, c))
        })
        .unwrap();
    for (specific, general) in [(c, a), (a, a), (c, b)] {
        let result = doc.edit("cycle", |tx| {
            tx.create(root, ElementData::new("", ElementKind::generalization(specific, general)))
        });
        assert!(
            matches!(result, Err(ModelError::GeneralizationCycle(_))),
            "{specific:?} -> {general:?}"
        );
    }
    // A second path that is not a cycle is fine.
    let ac = doc
        .edit("ok", |tx| tx.create(root, ElementData::new("", ElementKind::generalization(a, c))))
        .unwrap();
    doc.model().check().unwrap();

    // Rewiring a generalization is checked like a new one: B → A and C → B close cycles through
    // A → B and B → C. Deleting, undoing and redoing keep the index in step (`check`).
    doc.edit("rewire", |tx| tx.update(ac, |d| d.kind = ElementKind::generalization(b, a)))
        .unwrap_err();
    doc.edit("delete", |tx| tx.delete(ac)).unwrap();
    doc.model().check().unwrap();
    assert!(doc.undo());
    doc.model().check().unwrap();
    assert!(doc.redo());
    doc.model().check().unwrap();
    assert!(doc.undo());
    let rewired =
        doc.edit("rewire", |tx| tx.update(ac, |d| d.kind = ElementKind::generalization(c, b)));
    assert!(matches!(rewired, Err(ModelError::GeneralizationCycle(_))), "{rewired:?}");
    doc.model().check().unwrap();
}

#[test]
fn references_must_have_the_right_kind() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let result = doc.edit("bad", |tx| {
        let pkg = tx.create(root, ElementData::new("p", ElementKind::Package))?;
        let a = tx.create(root, class("A"))?;
        tx.create(root, ElementData::new("", ElementKind::association(a, pkg)))
    });
    assert!(matches!(result, Err(ModelError::InvalidReference { .. })));
}

#[test]
fn move_rejects_cycles_and_keeps_order() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (p, q, a, b) = doc
        .edit("setup", |tx| {
            let p = tx.create(root, ElementData::new("p", ElementKind::Package))?;
            let q = tx.create(p, ElementData::new("q", ElementKind::Package))?;
            let a = tx.create(root, class("A"))?;
            let b = tx.create(root, class("B"))?;
            Ok((p, q, a, b))
        })
        .unwrap();
    assert!(matches!(
        doc.edit("cycle", |tx| tx.move_element(p, q, 0)),
        Err(ModelError::OwnershipCycle(_))
    ));
    doc.edit("reorder", |tx| tx.move_element(b, root, 0)).unwrap();
    assert_eq!(doc.model().children(root), &[b, p, a]);
    doc.edit("into q", |tx| tx.move_element(a, q, 0)).unwrap();
    assert_eq!(doc.model().children(q), &[a]);
    doc.undo();
    doc.undo();
    assert_eq!(doc.model().children(root), &[p, a, b]);
}

#[test]
fn remove_from_diagram_keeps_the_element() {
    let (mut doc, ids) = order_system();
    let node = doc.model().shown_on(ids.customer)[0].1;
    doc.edit("remove", |tx| tx.remove_presentation(ids.class_diagram, node)).unwrap();
    let model = doc.model();
    assert!(model.contains(ids.customer));
    assert!(model.contains(ids.order_customer));
    let view = model.view(ids.class_diagram).unwrap();
    assert_eq!(view.presentations().len(), 2, "node and both attached edges removed");
}

#[test]
fn presentation_edits_and_z_order() {
    let (mut doc, ids) = order_system();
    let d = ids.class_diagram;
    let node = doc.model().shown_on(ids.customer)[0].1;
    doc.edit("move", |tx| tx.set_bounds(d, node, Rect::new(1.0, 2.0, 3.0, 4.0))).unwrap();
    doc.edit("front", |tx| tx.bring_to_front(d, node)).unwrap();
    let view = doc.model().view(d).unwrap();
    assert_eq!(view.presentations().last().unwrap().id, node);
    assert_eq!(view.get(node).unwrap().bounds(), Some(Rect::new(1.0, 2.0, 3.0, 4.0)));
    doc.undo();
    doc.undo();
    assert_eq!(doc.model().view(d).unwrap().presentations()[0].id, node);
}

#[test]
fn diagram_kind_and_metaclass_are_fixed() {
    let (mut doc, ids) = order_system();
    let result = doc.edit("change kind", |tx| {
        tx.update(ids.class_diagram, |d| d.kind = ElementKind::diagram(DiagramKind::Sequence))
    });
    assert!(matches!(result, Err(ModelError::KindChanged(_))));
    let result = doc.edit("change metaclass", |tx| {
        tx.update(ids.customer, |d| d.kind = ElementKind::interface())
    });
    assert!(matches!(result, Err(ModelError::KindChanged(_))));
}

/// An update keeps the metaclass but may change what decides where an element can be and what
/// may refer to it; it is checked against its owner, its children and its referrers.
#[test]
fn updates_keep_owners_children_and_referrers_valid() {
    use umlcraft_model::er::{ErEntityProps, ErParticipationProps};
    use umlcraft_model::{ActivityNodeKind, PseudostateKind};
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let mut ids = (root, root, root);
    doc.edit("build", |tx| {
        let machine = tx.create(root, ElementData::new("sm", ElementKind::StateMachine))?;
        let state =
            tx.create(machine, ElementData::new("s", ElementKind::State(Default::default())))?;
        let region = tx.create(state, ElementData::new("", ElementKind::Region))?;
        let initial = ElementKind::pseudostate(PseudostateKind::Initial);
        let initial = tx.create(region, ElementData::new("", initial))?;
        let activity = tx.create(root, ElementData::new("a", ElementKind::Activity))?;
        let action = ElementKind::activity_node(ActivityNodeKind::Action);
        let action = tx.create(activity, ElementData::new("do", action))?;
        let pin = ElementKind::activity_node(ActivityNodeKind::InputPin);
        tx.create(action, ElementData::new("in", pin))?;
        let associative = ErEntityProps { associative: true, ..Default::default() };
        let order =
            tx.create(root, ElementData::new("Order", ElementKind::ErEntity(associative)))?;
        let item =
            tx.create(root, ElementData::new("Item", ElementKind::ErEntity(Default::default())))?;
        let part = ErParticipationProps::new(order, item);
        tx.create(order, ElementData::new("", ElementKind::ErParticipation(part)))?;
        ids = (initial, action, order);
        Ok(())
    })
    .unwrap();
    let (initial, action, order) = ids;
    // A region holds no entry point: that sits on the state's border.
    let result = doc.edit("entry", |tx| {
        tx.update(initial, |d| d.kind = ElementKind::pseudostate(PseudostateKind::EntryPoint))
    });
    assert!(matches!(result, Err(ModelError::InvalidOwner { child, .. }) if child == initial));
    // A decision has no pins.
    let result = doc.edit("decision", |tx| {
        tx.update(action, |d| d.kind = ElementKind::activity_node(ActivityNodeKind::Decision))
    });
    assert!(matches!(result, Err(ModelError::InvalidOwner { owner, .. }) if owner == action));
    // Entities take part in an associative entity only.
    let result = doc.edit("plain", |tx| {
        tx.update(order, |d| d.kind = ElementKind::ErEntity(Default::default()))
    });
    assert!(matches!(result, Err(ModelError::InvalidReference { to, .. }) if to == order));
    // What changes nothing of that goes through.
    let weak = ErEntityProps { associative: true, weak: true, ..Default::default() };
    doc.edit("weak", |tx| tx.update(order, |d| d.kind = ElementKind::ErEntity(weak))).unwrap();
    doc.model().check().unwrap();
}

#[test]
fn records_roundtrip() {
    let (doc, _) = order_system();
    let records = doc.model().to_records();
    let restored = Model::from_records(records.clone()).unwrap();
    assert_eq!(restored, *doc.model());
    assert_eq!(restored.to_records(), records);
}

#[test]
fn from_records_rejects_orphans() {
    let (doc, ids) = order_system();
    let mut records = doc.model().to_records();
    for r in &mut records.elements {
        r.children.retain(|&c| c != ids.premium);
    }
    assert!(Model::from_records(records).is_err());
}

#[test]
fn cycles_that_would_block_deleting_are_refused() {
    let (mut doc, ids) = order_system();
    // A dependency naming itself.
    let result = doc.edit("dependency", |tx| {
        let id = tx.create(ids.domain, class("tmp"))?;
        let dependency = ElementKind::Dependency(umlcraft_model::DirectedProps {
            client: id,
            supplier: ids.order,
        });
        let d = tx.create(ids.domain, ElementData::new("", dependency))?;
        tx.update(d, |data| {
            if let ElementKind::Dependency(p) = &mut data.kind {
                p.client = d;
            }
        })
    });
    assert!(matches!(result, Err(ModelError::ReferenceCycle(_))), "{result:?}");

    // Two edges ending on each other.
    let d = ids.class_diagram;
    let association = doc.model().shown_on(ids.order_customer)[0].1;
    let generalization = doc.model().shown_on(ids.premium_generalization)[0].1;
    let end_on = |target: Id| {
        move |p: &mut umlcraft_model::Presentation| {
            if let umlcraft_model::Shape::Edge { target: t, .. } = &mut p.shape {
                *t = target;
            }
        }
    };
    doc.edit("one", |tx| tx.update_presentation(d, association, end_on(generalization))).unwrap();
    let result =
        doc.edit("two", |tx| tx.update_presentation(d, generalization, end_on(association)));
    assert!(matches!(result, Err(ModelError::EdgeCycle(_))), "{result:?}");
    doc.model().check().unwrap();
}

#[test]
fn removing_many_presentations_takes_their_edges_along() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let d = doc
        .edit("setup", |tx| {
            let d =
                tx.create(root, ElementData::new("d", ElementKind::diagram(DiagramKind::Class)))?;
            let nodes: Vec<Id> = (0..1000)
                .map(|i| tx.add_node(d, None, Rect::new(i as f32 * 10.0, 0.0, 5.0, 5.0)))
                .collect::<Result<_, _>>()?;
            for (i, &n) in nodes.iter().enumerate() {
                let next = nodes[(i + 1) % nodes.len()];
                let edge = tx.add_presentation(
                    d,
                    umlcraft_model::Presentation::edge(None, n, next, Vec::new()),
                )?;
                // An edge on an edge, which must go before the edge it hangs on.
                tx.add_presentation(
                    d,
                    umlcraft_model::Presentation::edge(None, edge, nodes[0], Vec::new()),
                )?;
            }
            Ok(d)
        })
        .unwrap();
    let start = std::time::Instant::now();
    let nodes: indexmap::IndexSet<Id> = doc
        .model()
        .view(d)
        .unwrap()
        .presentations()
        .iter()
        .filter(|p| p.bounds().is_some())
        .map(|p| p.id)
        .take(500)
        .collect();
    doc.edit("remove", |tx| tx.remove_presentations(d, nodes)).unwrap();
    let left = doc.model().view(d).unwrap().presentations().len();
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    doc.model().check().unwrap();
    assert!(left < 3000 && left > 0, "{left}");
    assert!(doc.undo());
    assert_eq!(doc.model().view(d).unwrap().presentations().len(), 3000);
}

#[test]
fn deleting_a_package_of_referenced_classes_unlinks_the_rest() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (package, users) = doc
        .edit("setup", |tx| {
            let package = tx.create(root, ElementData::new("p", ElementKind::Package))?;
            let mut users = Vec::new();
            for i in 0..2000 {
                let target = tx.create(package, class(&format!("T{i}")))?;
                let user = tx.create(root, class(&format!("U{i}")))?;
                let attribute = ElementKind::attribute(Some(TypeRef::Element(target)));
                users.push(tx.create(user, ElementData::new("t", attribute))?);
            }
            Ok((package, users))
        })
        .unwrap();
    let start = std::time::Instant::now();
    doc.edit("delete", |tx| tx.delete(package)).unwrap();
    assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
    doc.model().check().unwrap();
    let ElementKind::Attribute(p) = doc.model().get(users[7]).unwrap().kind() else { panic!() };
    assert_eq!(p.type_ref, Some(TypeRef::Text("T7".into())));
}

#[test]
fn children_are_found_by_name_through_renames_moves_and_undo() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (p, a, b) = doc
        .edit("setup", |tx| {
            let p = tx.create(root, ElementData::new("p", ElementKind::Package))?;
            let a = tx.create(root, class("A"))?;
            let b = tx.create(root, class("A"))?;
            Ok((p, a, b))
        })
        .unwrap();
    assert_eq!(doc.model().children_named(root, "A"), [a, b], "in the owner's order");
    doc.edit("rename", |tx| tx.rename(a, "B")).unwrap();
    doc.edit("move", |tx| tx.move_element(b, p, 0)).unwrap();
    let model = doc.model();
    model.check().unwrap();
    assert_eq!(model.children_named(root, "B"), [a]);
    assert!(model.children_named(root, "A").is_empty());
    assert_eq!(model.children_named(p, "A"), [b]);
    assert!(doc.undo() && doc.undo());
    doc.model().check().unwrap();
    assert_eq!(doc.model().children_named(root, "A"), [a, b]);
}

#[test]
fn a_failed_attempt_takes_back_only_its_own_part() {
    let (mut doc, ids) = order_system();
    let root = doc.model().root();
    let (kept, dropped) = doc
        .edit("partly", |tx| {
            let kept = tx.create(root, class("Kept"))?;
            let mut dropped = None;
            let failed = tx.attempt(|tx| {
                dropped = Some(tx.create(root, class("Dropped"))?);
                // A package cannot go into one of its own classes.
                tx.move_element(root, ids.customer, 0)
            });
            assert!(failed.is_err());
            Ok((kept, dropped.unwrap()))
        })
        .unwrap();
    let model = doc.model();
    assert!(model.get(kept).is_some() && model.get(dropped).is_none());
    model.check().unwrap();
    assert!(doc.undo());
    assert!(doc.model().get(kept).is_none());
}

#[test]
fn a_panicking_edit_leaves_the_model_as_it_was() {
    let (mut doc, _) = order_system();
    let before = doc.model().clone();
    let root = before.root();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = doc.edit("boom", |tx| -> Result<(), ModelError> {
            tx.create(root, class("Half"))?;
            panic!("an edit that breaks off");
        });
    }));
    assert!(caught.is_err());
    assert_eq!(doc.model(), &before);
    assert!(!doc.can_undo() || doc.undo_label() != Some("boom"));
}

#[test]
fn presentations_are_found_by_id_after_every_change() {
    let (mut doc, ids) = order_system();
    let d = ids.class_diagram;
    let all = |doc: &Document| {
        let view = doc.model().view(d).unwrap();
        view.presentations().iter().all(|p| view.get(p.id).map(|q| q.id) == Some(p.id))
    };
    let first = doc.model().view(d).unwrap().presentations()[0].id;
    let node = doc.edit("add", |tx| tx.add_node(d, None, Rect::new(0.0, 0.0, 10.0, 10.0))).unwrap();
    doc.edit("front", |tx| tx.bring_to_front(d, first)).unwrap();
    assert!(all(&doc));
    assert_eq!(doc.model().view(d).unwrap().presentations().last().map(|p| p.id), Some(first));
    doc.edit("remove", |tx| tx.remove_presentation(d, node)).unwrap();
    assert!(all(&doc) && doc.model().view(d).unwrap().get(node).is_none());
    assert!(doc.undo() && doc.undo() && doc.undo());
    assert!(all(&doc) && doc.model().view(d).unwrap().get(node).is_none());
    doc.model().check().unwrap();
}

/// Checking a loaded model, and undoing an import into one package, cost linear time: in a
/// package of many children, along a long chain of dependencies naming dependencies, and along
/// a long chain of edges ending on edges (as a damaged or hostile file may have them).
#[test]
fn large_packages_and_long_chains_stay_linear() {
    use umlcraft_model::{DirectedProps, ElementRecord, ModelRecords, Presentation};
    const N: usize = 30_000;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let start = std::time::Instant::now();
    doc.edit("import", |tx| {
        for i in 0..N {
            tx.create(root, class(&format!("C{i}")))?;
        }
        Ok(())
    })
    .unwrap();
    assert!(doc.undo() && doc.redo());
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 20, "import, undo and redo: {elapsed:?}");

    let mut records = doc.model().to_records();
    let first = records.elements[1].id;
    let diagram = Id::generate();
    let mut previous = first;
    let mut dependencies = Vec::new();
    for _ in 0..N {
        let kind = ElementKind::Dependency(DirectedProps { client: previous, supplier: first });
        let id = Id::generate();
        records.elements.push(ElementRecord {
            id,
            data: ElementData::new("", kind),
            children: Vec::new(),
        });
        dependencies.push(id);
        previous = id;
    }
    records.elements.push(ElementRecord {
        id: diagram,
        data: ElementData::new("d", ElementKind::diagram(DiagramKind::Class)),
        children: Vec::new(),
    });
    let node = Presentation::node(Some(first), Rect::new(0.0, 0.0, 10.0, 10.0));
    let mut presentations = vec![node.clone()];
    let mut end = node.id;
    for _ in 0..N {
        let edge = Presentation::edge(None, end, node.id, Vec::new());
        end = edge.id;
        presentations.push(edge);
    }
    records.views.insert(diagram, presentations);
    let root_record = records.elements.iter_mut().find(|r| r.id == root).unwrap();
    root_record.children.extend(dependencies.iter().copied().chain([diagram]));
    let ModelRecords { root, elements, views } = records;
    let start = std::time::Instant::now();
    let model = Model::from_records(ModelRecords { root, elements, views }).unwrap();
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 20, "checking: {elapsed:?}");
    assert_eq!(model.len(), 2 * N + 2);
}

/// Deleting a diagram with many presentations, undoing and redoing it take time linear in
/// their number: each removal and each insertion used to shift and reindex the rest of the
/// list and look through it for edges still attached.
#[test]
fn deleting_a_large_diagram_and_undoing_it_stay_fast() {
    const NODES: usize = 10_000;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let d = doc
        .edit("setup", |tx| {
            let d =
                tx.create(root, ElementData::new("d", ElementKind::diagram(DiagramKind::Class)))?;
            let nodes: Vec<Id> = (0..NODES)
                .map(|i| tx.add_node(d, None, Rect::new(i as f32 * 10.0, 0.0, 5.0, 5.0)))
                .collect::<Result<_, _>>()?;
            for pair in nodes.windows(2) {
                let edge = umlcraft_model::Presentation::edge(None, pair[0], pair[1], Vec::new());
                tx.add_presentation(d, edge)?;
            }
            Ok(d)
        })
        .unwrap();
    let all = doc.model().clone();
    let start = std::time::Instant::now();
    doc.edit("delete", |tx| tx.delete(d)).unwrap();
    assert!(doc.model().view(d).is_none());
    assert!(doc.undo());
    assert_eq!(*doc.model(), all);
    assert!(doc.redo());
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 10, "{elapsed:?}");
    doc.model().check().unwrap();
}

/// Removing every other node of a large view, scattered through its list, with the edges
/// between them, and undoing and redoing that, compacts the list once, not once per node.
#[test]
fn removing_scattered_presentations_stays_fast_and_undoes_exactly() {
    const NODES: usize = 20_000;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (d, nodes) = doc
        .edit("setup", |tx| {
            let d =
                tx.create(root, ElementData::new("d", ElementKind::diagram(DiagramKind::Class)))?;
            let nodes: Vec<Id> = (0..NODES)
                .map(|i| tx.add_node(d, None, Rect::new(i as f32 * 10.0, 0.0, 5.0, 5.0)))
                .collect::<Result<_, _>>()?;
            for pair in nodes.windows(2) {
                let edge = umlcraft_model::Presentation::edge(None, pair[0], pair[1], Vec::new());
                tx.add_presentation(d, edge)?;
            }
            Ok((d, nodes))
        })
        .unwrap();
    let all = doc.model().clone();
    let every_other: indexmap::IndexSet<Id> = nodes.iter().step_by(2).copied().collect();
    let start = std::time::Instant::now();
    doc.edit("remove", |tx| tx.remove_presentations(d, every_other)).unwrap();
    // Every other node, and every edge (each touches one of them).
    let left = doc.model().view(d).unwrap().presentations().len();
    assert_eq!(left, NODES / 2);
    doc.model().check().unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.model(), all);
    assert!(doc.redo());
    let elapsed = start.elapsed();
    assert!(elapsed.as_secs() < 5, "{elapsed:?}");
    assert_eq!(doc.model().view(d).unwrap().presentations().len(), NODES / 2);
    doc.model().check().unwrap();
}

/// The batch operations check what they get like the single ones: nothing that stays may
/// hang on what goes, and what is put back must fit.
#[test]
fn removing_presentations_at_once_keeps_the_rules() {
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (d, a, b, edge) = doc
        .edit("setup", |tx| {
            let d =
                tx.create(root, ElementData::new("d", ElementKind::diagram(DiagramKind::Class)))?;
            let a = tx.add_node(d, None, Rect::new(0.0, 0.0, 5.0, 5.0))?;
            let b = tx.add_node(d, None, Rect::new(20.0, 0.0, 5.0, 5.0))?;
            let edge = umlcraft_model::Presentation::edge(None, a, b, Vec::new());
            let edge_id = edge.id;
            tx.add_presentation(d, edge)?;
            Ok((d, a, b, edge_id))
        })
        .unwrap();
    let mut model = doc.model().clone();
    let refused = model.apply(Op::RemovePresentations { diagram: d, ids: vec![a] });
    assert_eq!(refused, Err(ModelError::EdgesAttached(a)));
    assert_eq!(model, *doc.model());
    let inverse = model.apply(Op::RemovePresentations { diagram: d, ids: vec![edge, a] }).unwrap();
    assert_eq!(model.view(d).unwrap().presentations().len(), 1);
    // Putting the edge back without its end is refused, and changes nothing.
    let Op::InsertPresentations { entries, .. } = &inverse else { panic!("{inverse:?}") };
    let edge_alone: Vec<_> = entries.iter().filter(|(_, p)| p.id == edge).cloned().collect();
    let before = model.clone();
    let alone = model.apply(Op::InsertPresentations { diagram: d, entries: edge_alone });
    assert!(alone.is_err(), "{alone:?}");
    assert_eq!(model, before);
    model.apply(inverse).unwrap();
    assert_eq!(model, *doc.model());
    model.check().unwrap();
    let _ = b;
}

/// An edit made on a copy of the model applies to the original as one step that undoes and
/// redoes like the edit itself.
#[test]
fn an_edit_made_on_a_copy_applies_to_the_original() {
    let (mut doc, ids) = order_system();
    let mut copy = Document::new(doc.model().clone());
    let made = copy
        .edit("on the copy", |tx| {
            let class = tx.create(ids.domain, class("Invoice"))?;
            tx.rename(ids.customer, "Client")?;
            tx.delete(ids.order)?;
            Ok(class)
        })
        .unwrap();
    let expected = copy.model().clone();
    let ops = copy.take_last_step().unwrap();
    assert!(!copy.can_undo() && !copy.can_redo());
    assert_eq!(*copy.model(), *doc.model());
    let before = doc.model().clone();
    doc.edit("replayed", |tx| ops.into_iter().try_for_each(|op| tx.apply(op))).unwrap();
    assert_eq!(*doc.model(), expected);
    assert!(doc.model().get(made).is_some());
    assert!(doc.undo());
    assert_eq!(*doc.model(), before);
    assert!(doc.redo());
    assert_eq!(*doc.model(), expected);
    doc.model().check().unwrap();
}
