//! A small example project with one diagram of each supported kind.
//! Used by tests, the format round trip and as the app's start document.

use crate::diagram::{DiagramKind, Presentation};
use crate::document::Document;
use crate::element::{
    ActivityNodeKind, Aggregation, ClassifierProps, CombinedFragmentProps, DirectedProps,
    ElementData, ElementKind, InstanceKind, InstanceProps, InteractionOperandProps,
    InteractionOperator, LifelineProps, MessageSort, Multiplicity, PseudostateKind, StateProps,
    TransitionProps, TypeRef, Visibility,
};
use crate::geometry::{Point, Rect};
use crate::id::Id;
use crate::model::Model;

/// Ids of the interesting elements in [`order_system`].
#[derive(Clone, Copy, Debug)]
pub struct OrderSystem {
    pub domain: Id,
    pub customer: Id,
    pub order: Id,
    pub order_customer: Id,
    pub premium: Id,
    pub premium_generalization: Id,
    pub class_diagram: Id,
    pub use_case_diagram: Id,
    pub sequence_diagram: Id,
    pub customer_lifeline: Id,
    pub activity_diagram: Id,
    pub state_diagram: Id,
    pub component_diagram: Id,
    pub deployment_diagram: Id,
}

pub fn order_system() -> (Document, OrderSystem) {
    let mut doc = Document::new(Model::new("Order System"));
    let root = doc.model().root();
    let ids = doc
        .edit("Create sample", |tx| {
            let domain = tx.create(root, ElementData::new("domain", ElementKind::Package))?;

            let customer = tx.create(domain, ElementData::new("Customer", ElementKind::class()))?;
            tx.create(
                customer,
                ElementData::new(
                    "name",
                    ElementKind::attribute(Some(TypeRef::Text("String".into()))),
                )
                .with_visibility(Visibility::Private),
            )?;
            let order = tx.create(domain, ElementData::new("Order", ElementKind::class()))?;
            tx.create(
                order,
                ElementData::new(
                    "total",
                    ElementKind::attribute(Some(TypeRef::Text("Money".into()))),
                )
                .with_visibility(Visibility::Private),
            )?;
            tx.create(order, ElementData::new("submit", ElementKind::operation(None)))?;
            let premium =
                tx.create(domain, ElementData::new("PremiumCustomer", ElementKind::class()))?;

            let mut association = ElementKind::association(order, customer);
            if let ElementKind::Association(p) = &mut association {
                p.ends[0].multiplicity = Some(Multiplicity::MANY);
                p.ends[1].multiplicity = Some(Multiplicity::ONE);
                p.ends[1].role = "customer".into();
                p.ends[0].aggregation = Aggregation::None;
            }
            let order_customer = tx.create(domain, ElementData::new("", association))?;
            let premium_generalization = tx.create(
                domain,
                ElementData::new("", ElementKind::generalization(premium, customer)),
            )?;

            let class_diagram = tx.create(
                domain,
                ElementData::new("Domain", ElementKind::diagram(DiagramKind::Class)),
            )?;
            let n_customer =
                tx.add_node(class_diagram, Some(customer), Rect::new(80.0, 80.0, 160.0, 90.0))?;
            let n_order =
                tx.add_node(class_diagram, Some(order), Rect::new(380.0, 80.0, 160.0, 100.0))?;
            let n_premium =
                tx.add_node(class_diagram, Some(premium), Rect::new(80.0, 260.0, 160.0, 60.0))?;
            tx.add_edge(class_diagram, Some(order_customer), n_order, n_customer, vec![])?;
            tx.add_edge(
                class_diagram,
                Some(premium_generalization),
                n_premium,
                n_customer,
                vec![],
            )?;

            let actor = tx.create(root, ElementData::new("Clerk", ElementKind::actor()))?;
            let place_order =
                tx.create(root, ElementData::new("Place order", ElementKind::use_case()))?;
            let actor_link = tx
                .create(root, ElementData::new("", ElementKind::association(actor, place_order)))?;
            let use_case_diagram = tx.create(
                root,
                ElementData::new("Use cases", ElementKind::diagram(DiagramKind::UseCase)),
            )?;
            let n_actor =
                tx.add_node(use_case_diagram, Some(actor), Rect::new(60.0, 100.0, 40.0, 80.0))?;
            let n_uc = tx.add_node(
                use_case_diagram,
                Some(place_order),
                Rect::new(220.0, 110.0, 140.0, 60.0),
            )?;
            tx.add_edge(use_case_diagram, Some(actor_link), n_actor, n_uc, vec![])?;

            let interaction =
                tx.create(place_order, ElementData::new("Place order", ElementKind::Interaction))?;
            let customer_lifeline = tx.create(
                interaction,
                ElementData::new(
                    "c",
                    ElementKind::Lifeline(LifelineProps { represents: Some(customer) }),
                ),
            )?;
            let order_lifeline = tx.create(
                interaction,
                ElementData::new(
                    "o",
                    ElementKind::Lifeline(LifelineProps { represents: Some(order) }),
                ),
            )?;
            // Messages and fragments in the order they happen.
            let items = tx.create(
                interaction,
                ElementData::new(
                    "",
                    ElementKind::CombinedFragment(CombinedFragmentProps {
                        operator: InteractionOperator::Loop,
                        covered: Vec::new(),
                    }),
                ),
            )?;
            let each_item = tx.create(
                items,
                ElementData::new(
                    "",
                    ElementKind::InteractionOperand(InteractionOperandProps {
                        guard: "more items".into(),
                    }),
                ),
            )?;
            let add_item = tx.create(each_item, {
                let mut kind = ElementKind::message(
                    MessageSort::Synchronous,
                    customer_lifeline,
                    order_lifeline,
                );
                if let ElementKind::Message(p) = &mut kind {
                    p.arguments = "item".into();
                }
                ElementData::new("addItem", kind)
            })?;
            let submit = tx.create(
                interaction,
                ElementData::new(
                    "submit",
                    ElementKind::message(
                        MessageSort::Synchronous,
                        customer_lifeline,
                        order_lifeline,
                    ),
                ),
            )?;
            let confirmation = tx.create(interaction, {
                let mut kind =
                    ElementKind::message(MessageSort::Reply, order_lifeline, customer_lifeline);
                if let ElementKind::Message(p) = &mut kind {
                    p.return_value = "confirmation".into();
                }
                ElementData::new("", kind)
            })?;
            let sequence_diagram = tx.create(
                interaction,
                ElementData::new("Place order", ElementKind::diagram(DiagramKind::Sequence)),
            )?;
            // Laid out as Astah would: the frame, lifeline heads 110 × 30, the loop around the
            // first call, bars of 25 for the calls, the reply at the end of the second one.
            let d = sequence_diagram;
            tx.add_node(d, Some(d), Rect::new(35.0, 5.0, 360.0, 330.0))?;
            tx.add_presentation(
                d,
                Presentation::fragment(items, Rect::new(50.0, 120.0, 330.0, 70.0)),
            )?;
            let n_customer = tx.add_presentation(
                d,
                Presentation::lifeline(
                    customer_lifeline,
                    Rect::new(60.0, 50.0, 110.0, 30.0),
                    240.0,
                ),
            )?;
            let n_order = tx.add_presentation(
                d,
                Presentation::lifeline(order_lifeline, Rect::new(260.0, 50.0, 110.0, 30.0), 240.0),
            )?;
            tx.add_presentation(d, Presentation::activation(n_customer, 150.0, 115.0))?;
            tx.add_presentation(d, Presentation::activation(n_order, 150.0, 25.0))?;
            tx.add_presentation(d, Presentation::activation(n_order, 220.0, 25.0))?;
            for (message, from, to, y) in [
                (add_item, n_customer, n_order, 150.0),
                (submit, n_customer, n_order, 220.0),
                (confirmation, n_order, n_customer, 245.0),
            ] {
                tx.add_presentation(d, Presentation::message(message, from, to, y))?;
            }

            // The activity: the clerk checks the stock, the warehouse ships or the clerk cancels.
            let activity =
                tx.create(place_order, ElementData::new("Handle order", ElementKind::Activity))?;
            let activity_diagram = tx.create(
                activity,
                ElementData::new("Handle order", ElementKind::diagram(DiagramKind::Activity)),
            )?;
            let node = ElementKind::activity_node;
            let mut nodes = Vec::new();
            for (name, kind, (x, y, w, h)) in [
                ("Clerk", ElementKind::Partition, (40.0, 40.0, 220.0, 420.0)),
                ("Warehouse", ElementKind::Partition, (260.0, 40.0, 220.0, 420.0)),
                ("", node(ActivityNodeKind::Initial), (140.0, 100.0, 20.0, 20.0)),
                ("Check stock", node(ActivityNodeKind::Action), (90.0, 150.0, 120.0, 36.0)),
                ("", node(ActivityNodeKind::Decision), (138.0, 226.0, 24.0, 24.0)),
                ("Ship order", node(ActivityNodeKind::Action), (310.0, 300.0, 120.0, 36.0)),
                ("Cancel order", node(ActivityNodeKind::Action), (90.0, 300.0, 120.0, 36.0)),
                ("", node(ActivityNodeKind::ActivityFinal), (360.0, 400.0, 20.0, 20.0)),
                ("", node(ActivityNodeKind::ActivityFinal), (140.0, 400.0, 20.0, 20.0)),
            ] {
                let bounds = Rect::new(x, y, w, h);
                let element = tx.create(activity, ElementData::new(name, kind))?;
                let presentation = tx.add_node(activity_diagram, Some(element), bounds)?;
                nodes.push((element, presentation));
            }
            let [_, _, initial, check, decision, ship, cancel, shipped, cancelled] = nodes[..]
            else {
                unreachable!()
            };
            for (from, to, guard, waypoints) in [
                (initial, check, "", vec![]),
                (check, decision, "", vec![]),
                (decision, ship, "in stock", vec![Point::new(370.0, 238.0)]),
                (decision, cancel, "else", vec![]),
                (ship, shipped, "", vec![]),
                (cancel, cancelled, "", vec![]),
            ] {
                let mut kind = ElementKind::flow(from.0, to.0);
                if let ElementKind::ActivityEdge(p) = &mut kind {
                    p.guard = guard.into();
                }
                let flow = tx.create(activity, ElementData::new("", kind))?;
                tx.add_edge(activity_diagram, Some(flow), from.1, to.1, waypoints)?;
            }

            // Components: the shop uses payment, which billing provides; shop.jar holds the shop.
            let classifier = ClassifierProps::default();
            let shop =
                tx.create(domain, ElementData::new("Shop", ElementKind::Component(classifier)))?;
            let billing =
                tx.create(domain, ElementData::new("Billing", ElementKind::Component(classifier)))?;
            let payment =
                tx.create(domain, ElementData::new("Payment", ElementKind::interface()))?;
            let jar =
                tx.create(domain, ElementData::new("shop.jar", ElementKind::Artifact(classifier)))?;
            let provides = tx.create(
                domain,
                ElementData::new(
                    "",
                    ElementKind::Realization(DirectedProps { client: billing, supplier: payment }),
                ),
            )?;
            let uses = tx.create(
                domain,
                ElementData::new(
                    "",
                    ElementKind::Dependency(DirectedProps { client: shop, supplier: payment }),
                )
                .with_stereotype("use"),
            )?;
            let manifests = tx.create(
                domain,
                ElementData::new(
                    "",
                    ElementKind::Dependency(DirectedProps { client: jar, supplier: shop }),
                )
                .with_stereotype("manifest"),
            )?;
            let component_diagram = tx.create(
                domain,
                ElementData::new("Components", ElementKind::diagram(DiagramKind::Component)),
            )?;
            let node = |tx: &mut crate::Tx<'_>, element, (x, y, w, h)| {
                tx.add_node(component_diagram, Some(element), Rect::new(x, y, w, h))
            };
            let n_shop = node(tx, shop, (60.0, 60.0, 140.0, 60.0))?;
            let n_payment = node(tx, payment, (300.0, 60.0, 140.0, 60.0))?;
            let n_billing = node(tx, billing, (540.0, 60.0, 140.0, 60.0))?;
            let n_jar = node(tx, jar, (60.0, 220.0, 140.0, 60.0))?;
            tx.add_edge(component_diagram, Some(uses), n_shop, n_payment, vec![])?;
            tx.add_edge(component_diagram, Some(provides), n_billing, n_payment, vec![])?;
            tx.add_edge(component_diagram, Some(manifests), n_jar, n_shop, vec![])?;

            // Deployment: the shop runs on web1, an application server, next to the database.
            let app_server =
                tx.create(domain, ElementData::new("AppServer", ElementKind::Node(classifier)))?;
            let db_server =
                tx.create(domain, ElementData::new("DBServer", ElementKind::Node(classifier)))?;
            let network = tx.create(
                domain,
                ElementData::new("", ElementKind::association(app_server, db_server)),
            )?;
            let web1 = tx.create(
                domain,
                ElementData::new(
                    "web1",
                    ElementKind::Instance(InstanceProps {
                        instance: InstanceKind::Node,
                        classifier: Some(app_server),
                    }),
                ),
            )?;
            let running = tx.create(
                domain,
                ElementData::new(
                    "",
                    ElementKind::Instance(InstanceProps {
                        instance: InstanceKind::Component,
                        classifier: Some(shop),
                    }),
                ),
            )?;
            let deployment_diagram = tx.create(
                domain,
                ElementData::new("Deployment", ElementKind::diagram(DiagramKind::Deployment)),
            )?;
            let place = |tx: &mut crate::Tx<'_>, element, (x, y, w, h)| {
                tx.add_node(deployment_diagram, Some(element), Rect::new(x, y, w, h))
            };
            let n_web1 = place(tx, web1, (60.0, 60.0, 220.0, 140.0))?;
            place(tx, running, (90.0, 110.0, 140.0, 60.0))?;
            let n_app = place(tx, app_server, (60.0, 260.0, 160.0, 100.0))?;
            let n_db = place(tx, db_server, (360.0, 260.0, 160.0, 100.0))?;
            tx.add_edge(deployment_diagram, Some(network), n_app, n_db, vec![])?;
            let _ = n_web1;

            // The order's life: filled while open, then shipped or cancelled.
            let machine =
                tx.create(order, ElementData::new("Order lifecycle", ElementKind::StateMachine))?;
            let state_diagram = tx.create(
                machine,
                ElementData::new(
                    "Order lifecycle",
                    ElementKind::diagram(DiagramKind::StateMachine),
                ),
            )?;
            let state = |entry: &str| {
                ElementKind::State(StateProps { entry: entry.into(), ..Default::default() })
            };
            let initial = || ElementKind::pseudostate(PseudostateKind::Initial);
            let mut vertices: Vec<(Id, Id)> = Vec::new();
            // (owner index in `vertices`, name, kind, bounds); composite states come first.
            for (owner, name, kind, (x, y, w, h)) in [
                (None, "", initial(), (40.0, 135.0, 20.0, 20.0)),
                (None, "Open", state(""), (100.0, 60.0, 300.0, 170.0)),
                (Some(1), "", initial(), (115.0, 130.0, 20.0, 20.0)),
                (Some(1), "Empty", state(""), (160.0, 120.0, 80.0, 40.0)),
                (Some(1), "Filled", state(""), (290.0, 120.0, 80.0, 40.0)),
                (None, "Submitted", state("check stock"), (480.0, 115.0, 120.0, 60.0)),
                (None, "Shipped", state(""), (680.0, 60.0, 100.0, 40.0)),
                (None, "Cancelled", state(""), (680.0, 180.0, 100.0, 40.0)),
                (None, "", ElementKind::FinalState, (850.0, 135.0, 20.0, 20.0)),
            ] {
                let owner = owner.map_or(machine, |i: usize| vertices[i].0);
                let element = tx.create(owner, ElementData::new(name, kind))?;
                let presentation =
                    tx.add_node(state_diagram, Some(element), Rect::new(x, y, w, h))?;
                vertices.push((element, presentation));
            }
            let [start, open, inner, empty, filled, submitted, shipped, cancelled, done] =
                vertices[..]
            else {
                unreachable!()
            };
            for (from, to, (trigger, guard, effect), waypoints) in [
                (start, open, ("", "", ""), vec![]),
                (inner, empty, ("", "", ""), vec![]),
                (empty, filled, ("add item", "", ""), vec![]),
                // A self-transition loops at the top right corner.
                (
                    filled,
                    filled,
                    ("add item", "", ""),
                    vec![
                        Point::new(388.0, 140.0),
                        Point::new(388.0, 102.0),
                        Point::new(345.0, 102.0),
                    ],
                ),
                (open, submitted, ("submit", "", ""), vec![]),
                (submitted, shipped, ("", "in stock", "ship"), vec![]),
                (submitted, cancelled, ("cancel", "", ""), vec![]),
                (shipped, done, ("", "", ""), vec![]),
                (cancelled, done, ("", "", ""), vec![]),
            ] {
                let kind = ElementKind::Transition(TransitionProps {
                    source: from.0,
                    target: to.0,
                    trigger: trigger.into(),
                    guard: guard.into(),
                    effect: effect.into(),
                });
                let transition = tx.create(machine, ElementData::new("", kind))?;
                tx.add_edge(state_diagram, Some(transition), from.1, to.1, waypoints)?;
            }

            Ok(OrderSystem {
                domain,
                customer,
                order,
                order_customer,
                premium,
                premium_generalization,
                class_diagram,
                use_case_diagram,
                sequence_diagram,
                customer_lifeline,
                activity_diagram,
                state_diagram,
                component_diagram,
                deployment_diagram,
            })
        })
        .expect("sample model is valid");
    doc.clear_history();
    (doc, ids)
}
