use pretty_assertions::assert_eq;
use umlcraft_model::sample::order_system;
use umlcraft_model::{
    DiagramKind, Document, ElementData, ElementKind, Id, Model, ModelError, Rect, TypeRef,
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
    doc.edit("ok", |tx| tx.create(root, ElementData::new("", ElementKind::generalization(a, c))))
        .unwrap();
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
