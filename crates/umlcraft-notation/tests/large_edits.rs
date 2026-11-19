//! Edits on large projects: the work an edit does follows what it touches, not the size of the
//! project times itself. Each case would take minutes with a lookup through the whole model or
//! diagram per item; here they take a moment, also in debug builds.

use std::time::Instant;

use umlcraft_model::{
    ActivityNodeKind, DiagramKind, Document, ElementData, ElementKind, Id, Model, Point, Rect,
    Result,
};
use umlcraft_notation::scene::ApproxMeasure;
use umlcraft_notation::{edit, invariants, palette};

/// Showing classes of a model with thousands of associations finds each class's relationships
/// through what refers to it, and what the diagram shows through one look at it.
#[test]
fn showing_classes_of_a_large_model() {
    const CLASSES: usize = 3000;
    const SHOWN: usize = 300;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let (diagram, classes) = doc
        .edit("setup", |tx| {
            let p = tx.create(root, ElementData::new("p", ElementKind::Package))?;
            let classes = (0..CLASSES)
                .map(|i| tx.create(p, ElementData::new(format!("C{i}"), ElementKind::class())))
                .collect::<Result<Vec<Id>>>()?;
            for pair in classes.windows(2) {
                let kind = ElementKind::association(pair[0], pair[1]);
                tx.create(p, ElementData::new("", kind))?;
            }
            let kind = ElementKind::diagram(DiagramKind::Class);
            Ok((tx.create(p, ElementData::new("d", kind))?, classes))
        })
        .unwrap();
    let start = Instant::now();
    doc.edit("show", |tx| {
        for (i, &class) in classes.iter().take(SHOWN).enumerate() {
            let at = Point::new((i % 20) as f32 * 150.0, (i / 20) as f32 * 120.0);
            palette::show_element(tx, diagram, class, at)?;
        }
        Ok(())
    })
    .unwrap();
    eprintln!("showing {SHOWN} classes took {:?}", start.elapsed());
    // Each class but the first came with the association to the one before it.
    let view = doc.model().view(diagram).unwrap();
    assert_eq!(view.presentations().len(), SHOWN + SHOWN - 1);
}

/// Moving an action with hundreds of pins: each pin finds its action and its place before the
/// edit by an index, not by a look through the diagram.
#[test]
fn an_action_with_many_pins_takes_them_along() {
    const PINS: usize = 400;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let action_bounds = Rect::new(0.0, 0.0, 4000.0, 4000.0);
    let (diagram, action) = doc
        .edit("setup", |tx| {
            let diagram =
                edit::create_diagram(tx, root, DiagramKind::Activity, "Long running process")?;
            let activity = tx.model().get(diagram).and_then(|d| d.owner()).expect("an activity");
            let kind = ElementKind::activity_node(ActivityNodeKind::Action);
            let action = tx.create(activity, ElementData::new("work", kind))?;
            let node = tx.add_node(diagram, Some(action), action_bounds)?;
            for i in 0..PINS {
                let kind = ElementKind::activity_node(ActivityNodeKind::InputPin);
                let pin = tx.create(action, ElementData::new(format!("p{i}"), kind))?;
                // Along the left border, centered just outside it.
                let y = 5.0 + i as f32 * 9.0;
                tx.add_node(diagram, Some(pin), Rect::new(-14.0, y, 10.0, 10.0))?;
            }
            Ok((diagram, node))
        })
        .unwrap();
    let before = invariants::check(doc.model(), &ApproxMeasure);
    let start = Instant::now();
    edit::apply(&mut doc, "move", &ApproxMeasure, |tx| {
        tx.set_bounds(diagram, action, action_bounds.translate(100.0, 50.0))
    })
    .unwrap();
    eprintln!("moving an action with {PINS} pins took {:?}", start.elapsed());
    let view = doc.model().view(diagram).unwrap();
    let moved = view
        .presentations()
        .iter()
        .filter(|p| p.id != action)
        .filter_map(|p| p.bounds())
        .filter(|b| b.x >= 85.0)
        .count();
    assert_eq!(moved, PINS, "every pin went along");
    let after = invariants::check(doc.model(), &ApproxMeasure);
    assert!(invariants::new_violations(&before, &after).is_empty());
}

/// A rename in a project of many diagrams: the passes of `edit::apply` over every diagram's
/// sized boxes (to see whose text changed), routes and borders are linear in what the
/// diagrams show. 2,000 boxes take about 2.5 ms per edit in a release build and 65 ms in a
/// debug build, which also checks the rules before and after; a lookup per box through the
/// project would take seconds.
#[test]
fn a_rename_in_a_project_of_many_diagrams() {
    const DIAGRAMS: usize = 40;
    const CLASSES: usize = 50;
    let mut doc = Document::new(Model::new("m"));
    let root = doc.model().root();
    let first = doc
        .edit("setup", |tx| {
            let mut first = None;
            for d in 0..DIAGRAMS {
                let p = tx.create(root, ElementData::new(format!("p{d}"), ElementKind::Package))?;
                let kind = ElementKind::diagram(DiagramKind::Class);
                let diagram = tx.create(p, ElementData::new(format!("d{d}"), kind))?;
                for c in 0..CLASSES {
                    let class =
                        tx.create(p, ElementData::new(format!("C{d}_{c}"), ElementKind::class()))?;
                    for m in 0..3 {
                        umlcraft_notation::member::apply_member_text(
                            tx,
                            class,
                            &format!("- a{m} : String"),
                        )?;
                    }
                    let at = Point::new((c % 10) as f32 * 200.0, (c / 10) as f32 * 200.0);
                    palette::show_element(tx, diagram, class, at)?;
                    first.get_or_insert(class);
                }
            }
            Ok(first.expect("a class"))
        })
        .unwrap();
    let shown: usize = doc.model().views().map(|(_, v)| v.presentations().len()).sum();
    assert_eq!(shown, DIAGRAMS * CLASSES);
    let start = Instant::now();
    for i in 0..10 {
        edit::apply(&mut doc, "rename", &ApproxMeasure, |tx| tx.rename(first, format!("R{i}")))
            .unwrap();
    }
    let elapsed = start.elapsed();
    eprintln!("10 renames among {shown} boxes took {elapsed:?}");
    assert!(elapsed.as_secs() < 10, "{elapsed:?}");
}
