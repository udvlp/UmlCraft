//! The sample project that File › Open sample project opens: Velo, the bike sharing of a city.
//! It has a well-filled diagram of every kind, an object and a package diagram, and
//! structograms of three methods. Class boxes take the size their content needs with the
//! app's text widths, and the sequence diagram is drawn with the editor's own operations, so
//! bars, lifelines, fragments and frame come out as when drawn by hand.

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, AssociationProps, AttributeProps,
    ClassifierProps, CombinedFragmentProps, CommentProps, DiagramKind, DirectedProps, Document,
    ElementData, ElementKind, ExtendProps, Id, IncludeProps, InstanceKind, InstanceProps,
    InteractionOperandProps, InteractionOperator, InternalTransitionProps, LifelineProps,
    LinkProps, MessageSort, Model, ModelError, Multiplicity, Navigability, OperationProps,
    ParameterProps, Point, Presentation, PseudostateKind, Rect, Regions, Result, Shape, SlotProps,
    StateProps, StructogramProps, TransitionProps, Tx, TypeRef, Visibility,
};
use umlcraft_notation::scene::{self, TextMeasure};
use umlcraft_notation::sequence_edit;
use umlcraft_nsd::{Color, Element as Block, ForLoop, Provenance, RootKind, Sequence, lines};

/// The sample project, its boxes measured with `measure`.
pub fn velo(measure: &dyn TextMeasure) -> Document {
    let mut doc = Document::new(Model::new("Velo"));
    doc.edit("Create sample", |tx| Builder { tx, measure }.project())
        .expect("the sample project is valid");
    doc.clear_history();
    doc
}

fn text(s: &str) -> Option<TypeRef> {
    Some(TypeRef::Text(s.into()))
}

fn of(id: Id) -> Option<TypeRef> {
    Some(TypeRef::Element(id))
}

fn classifier() -> ClassifierProps {
    ClassifierProps::default()
}

/// The center of node `presentation` on `diagram`.
fn center(model: &Model, diagram: Id, presentation: Id) -> Point {
    let bounds = model.view(diagram).and_then(|v| v.get(presentation)).and_then(|p| p.bounds());
    bounds.map_or(Point::new(0.0, 0.0), |r| r.center())
}

/// One line of a structogram element.
fn line(s: &str) -> Vec<String> {
    lines(&[s])
}

struct Builder<'a, 'b> {
    tx: &'a mut Tx<'b>,
    measure: &'a dyn TextMeasure,
}

/// What the class diagram shows and the other diagrams refer to.
struct Domain {
    station: Id,
    dock: Id,
    vehicle: Id,
    bike: Id,
    ebike: Id,
    rider: Id,
    rental: Id,
    station_docks: Id,
    dock_vehicle: Id,
    rider_rentals: Id,
    rental_vehicle: Id,
    release: Id,
}

/// The use cases that own the interaction and the activity.
struct UseCases {
    rider: Id,
    rent: Id,
    give_back: Id,
}

/// The components the sequence diagram's lifelines stand for.
struct Architecture {
    app: Id,
    rental_service: Id,
    payment: Id,
}

impl Builder<'_, '_> {
    fn project(&mut self) -> Result<()> {
        let root = self.tx.model().root();
        // First in the tree, so that it is the diagram the project opens with.
        let domain_diagram = self.diagram(root, "Domain model", DiagramKind::Class)?;
        let overview = self.diagram(root, "Overview", DiagramKind::Class)?;
        let fleet = self.create(root, "fleet", ElementKind::Package)?;
        let rental = self.create(root, "rental", ElementKind::Package)?;
        let use_cases = self.create(root, "use cases", ElementKind::Package)?;
        let architecture = self.create(root, "architecture", ElementKind::Package)?;

        let domain = self.domain(fleet, rental, domain_diagram)?;
        self.overview(overview, [fleet, rental, use_cases, architecture])?;
        self.objects(rental, &domain)?;
        let cases = self.use_cases(use_cases)?;
        let parts = self.architecture(architecture)?;
        self.deployment(architecture)?;
        self.renting(&domain, &cases, &parts)?;
        self.returning(&cases)?;
        self.lifecycle(&domain)?;
        Ok(())
    }

    fn create(&mut self, owner: Id, name: &str, kind: ElementKind) -> Result<Id> {
        self.tx.create(owner, ElementData::new(name, kind))
    }

    fn stereotyped(&mut self, owner: Id, name: &str, kind: ElementKind, st: &str) -> Result<Id> {
        self.tx.create(owner, ElementData::new(name, kind).with_stereotype(st))
    }

    fn diagram(&mut self, owner: Id, name: &str, kind: DiagramKind) -> Result<Id> {
        self.create(owner, name, ElementKind::diagram(kind))
    }

    fn attribute(
        &mut self,
        owner: Id,
        name: &str,
        type_ref: Option<TypeRef>,
        f: impl FnOnce(&mut AttributeProps),
    ) -> Result<Id> {
        let mut props = AttributeProps { type_ref, ..Default::default() };
        f(&mut props);
        let data = ElementData::new(name, ElementKind::Attribute(props))
            .with_visibility(Visibility::Private);
        self.tx.create(owner, data)
    }

    fn field(&mut self, owner: Id, name: &str, type_ref: Option<TypeRef>) -> Result<Id> {
        self.attribute(owner, name, type_ref, |_| {})
    }

    fn operation(
        &mut self,
        owner: Id,
        name: &str,
        parameters: &[(&str, Option<TypeRef>)],
        return_type: Option<TypeRef>,
        f: impl FnOnce(&mut OperationProps),
    ) -> Result<Id> {
        let mut props = OperationProps { return_type, ..Default::default() };
        f(&mut props);
        let op = self.create(owner, name, ElementKind::Operation(props))?;
        for (name, type_ref) in parameters {
            let props = ParameterProps { type_ref: type_ref.clone(), ..Default::default() };
            self.create(op, name, ElementKind::Parameter(props))?;
        }
        Ok(op)
    }

    fn method(
        &mut self,
        owner: Id,
        name: &str,
        parameters: &[(&str, Option<TypeRef>)],
        return_type: Option<TypeRef>,
    ) -> Result<Id> {
        self.operation(owner, name, parameters, return_type, |_| {})
    }

    fn literals(&mut self, owner: Id, names: &[&str]) -> Result<()> {
        for name in names {
            self.create(owner, name, ElementKind::EnumerationLiteral)?;
        }
        Ok(())
    }

    fn node(&mut self, diagram: Id, element: Id, (x, y, w, h): (f32, f32, f32, f32)) -> Result<Id> {
        self.tx.add_node(diagram, Some(element), Rect::new(x, y, w, h))
    }

    /// A class box at `(x, y)`, sized to its content as new boxes are.
    fn class_box(&mut self, diagram: Id, element: Id, (x, y): (f32, f32)) -> Result<Id> {
        let mut p = Presentation::node(Some(element), Rect::new(x, y, 160.0, 70.0));
        p.style.auto_size = true;
        if let Some(size) = scene::fitted_size(self.tx.model(), &p, self.measure) {
            p.shape = Shape::Node {
                bounds: Rect::from_origin_size(Point::new(x, y), size),
                regions: None,
            };
        }
        self.tx.add_presentation(diagram, p)
    }

    /// An interface drawn as a circle.
    fn lollipop(&mut self, diagram: Id, element: Id, (x, y): (f32, f32)) -> Result<Id> {
        let mut p = Presentation::node(Some(element), Rect::new(x, y, 24.0, 24.0));
        p.style.iconic = true;
        self.tx.add_presentation(diagram, p)
    }

    fn edge(
        &mut self,
        diagram: Id,
        element: Option<Id>,
        from: Id,
        to: Id,
        waypoints: &[(f32, f32)],
    ) -> Result<Id> {
        let waypoints = waypoints.iter().map(|&(x, y)| Point::new(x, y)).collect();
        self.tx.add_edge(diagram, element, from, to, waypoints)
    }

    /// Makes `edge` aim at the points `source` and `target` inside its end nodes instead of
    /// their centers.
    fn aim(
        &mut self,
        diagram: Id,
        edge: Id,
        source: Option<(f32, f32)>,
        target: Option<(f32, f32)>,
    ) -> Result<()> {
        let view = self.tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
        let bounds = |id: Id| view.get(id).and_then(|p| p.bounds()).ok_or(ModelError::NotFound(id));
        let (from, to) =
            view.get(edge).and_then(|p| p.endpoints()).ok_or(ModelError::NotFound(edge))?;
        let relative =
            |r: Rect, (x, y): (f32, f32)| Point::new((x - r.x) / r.width, (y - r.y) / r.height);
        let source = source.map(|p| bounds(from).map(|r| relative(r, p))).transpose()?;
        let target = target.map(|p| bounds(to).map(|r| relative(r, p))).transpose()?;
        self.tx.update_presentation(diagram, edge, |p| {
            if let Shape::Edge { source_anchor, target_anchor, .. } = &mut p.shape {
                (*source_anchor, *target_anchor) = (source, target);
            }
        })
    }

    fn width(&self, text: &str) -> f32 {
        self.measure.width(text, scene::FONT_SIZE, false)
    }

    /// A note with `body` at `(x, y)`, anchored to the elements of `anchors` (element, node).
    fn note(
        &mut self,
        owner: Id,
        diagram: Id,
        body: &str,
        (x, y): (f32, f32),
        anchors: &[(Id, Id)],
    ) -> Result<Id> {
        let annotated = anchors.iter().map(|(e, _)| *e).collect();
        let props = CommentProps { body: body.into(), annotated };
        let note = self.create(owner, "", ElementKind::Comment(props))?;
        let width = body.lines().map(|l| self.width(l)).fold(0.0, f32::max) + 30.0;
        let height = body.lines().count() as f32 * 16.0 + 20.0;
        let n = self.node(diagram, note, (x, y, width.max(120.0), height))?;
        for &(_, target) in anchors {
            self.edge(diagram, None, n, target, &[])?;
        }
        Ok(n)
    }

    fn association(
        &mut self,
        owner: Id,
        a: Id,
        b: Id,
        f: impl FnOnce(&mut AssociationProps),
    ) -> Result<Id> {
        let mut kind = ElementKind::association(a, b);
        if let ElementKind::Association(p) = &mut kind {
            // References are private fields, as in Java.
            for end in &mut p.ends {
                end.visibility = Visibility::Private;
            }
            f(p);
        }
        self.create(owner, "", kind)
    }

    /// A structogram of `operation`, filed under the operation's class.
    fn structogram(&mut self, operation: Id, text: &[&str], body: Vec<Block>) -> Result<Id> {
        let made = Provenance { author: "UmlCraft", date: "2026-10-31", origin: "UmlCraft" };
        let mut diagram = umlcraft_nsd::Diagram::new(RootKind::Sub, &lines(text), &made);
        diagram.children = Sequence::new(body);
        let class = self.tx.model().element(operation)?.owner().unwrap_or(self.tx.model().root());
        let mut props = StructogramProps::new(&diagram);
        props.operation = Some(operation);
        self.create(class, "", ElementKind::Structogram(props))
    }

    fn domain(&mut self, fleet: Id, rental: Id, d: Id) -> Result<Domain> {
        let (string, integer) = (|| text("String"), || text("int"));

        let geo = self.create(fleet, "GeoPoint", ElementKind::DataType(classifier()))?;
        self.field(geo, "latitude", text("double"))?;
        self.field(geo, "longitude", text("double"))?;
        let states = self.create(fleet, "VehicleState", ElementKind::Enumeration(classifier()))?;
        self.literals(states, &["Available", "Reserved", "Rented", "InRepair", "Retired"])?;

        let station = self.create(fleet, "Station", ElementKind::class())?;
        self.field(station, "name", string())?;
        self.field(station, "location", of(geo))?;
        self.field(station, "capacity", integer())?;
        let place = [("name", string()), ("location", of(geo)), ("capacity", integer())];
        self.method(station, "Station", &place, None)?;
        self.method(station, "freeDocks", &[], integer())?;
        let rebalance = self.method(station, "rebalance", &[], None)?;

        let dock = self.create(fleet, "Dock", ElementKind::class())?;
        self.field(dock, "number", integer())?;
        self.method(dock, "Dock", &[("number", integer())], None)?;
        let release = self.method(dock, "release", &[], text("boolean"))?;
        self.operation(dock, "isEmpty", &[], text("boolean"), |p| p.is_query = true)?;

        let abstract_class = ClassifierProps { is_abstract: true, ..classifier() };
        let vehicle = self.create(fleet, "Vehicle", ElementKind::Class(abstract_class))?;
        self.field(vehicle, "id", string())?;
        self.field(vehicle, "state", of(states))?;
        self.attribute(vehicle, "kilometres", integer(), |a| a.default_value = "0".into())?;
        // Only subclasses make vehicles.
        let made = self.method(vehicle, "Vehicle", &[("id", string())], None)?;
        self.tx.update(made, |d| d.visibility = Visibility::Protected)?;
        self.method(vehicle, "unlock", &[], text("boolean"))?;
        self.method(vehicle, "lock", &[("dock", of(dock))], None)?;

        let bike = self.create(fleet, "Bike", ElementKind::class())?;
        self.field(bike, "gears", integer())?;
        self.method(bike, "Bike", &[("id", string()), ("gears", integer())], None)?;
        let ebike = self.create(fleet, "EBike", ElementKind::class())?;
        self.attribute(ebike, "charge", integer(), |a| a.default_value = "100".into())?;
        self.method(ebike, "EBike", &[("id", string())], None)?;
        self.operation(ebike, "range", &[], text("double"), |p| p.is_query = true)?;

        let money = self.create(rental, "Money", ElementKind::DataType(classifier()))?;
        self.field(money, "amount", text("BigDecimal"))?;
        self.field(money, "currency", string())?;
        let membership =
            self.create(rental, "Membership", ElementKind::Enumeration(classifier()))?;
        self.literals(membership, &["Basic", "Plus", "Student"])?;

        let rider = self.create(rental, "Rider", ElementKind::class())?;
        self.field(rider, "name", string())?;
        self.field(rider, "email", string())?;
        self.field(rider, "membership", of(membership))?;
        self.method(rider, "Rider", &[("name", string()), ("email", string())], None)?;
        self.method(rider, "rent", &[("vehicle", of(vehicle))], None)?;

        let rental_class = self.create(rental, "Rental", ElementKind::class())?;
        self.field(rental_class, "start", text("LocalDateTime"))?;
        self.attribute(rental_class, "end", text("LocalDateTime"), |a| {
            a.multiplicity = Some(Multiplicity::OPTIONAL)
        })?;
        self.attribute(rental_class, "duration", text("Duration"), |a| a.is_derived = true)?;
        let parties = [("rider", of(rider)), ("vehicle", of(vehicle))];
        self.method(rental_class, "Rental", &parties, None)?;
        self.method(rental_class, "close", &[("dock", of(dock))], None)?;
        self.operation(rental_class, "cost", &[], of(money), |p| p.is_query = true)?;

        let policy = self.create(rental, "PricingPolicy", ElementKind::interface())?;
        self.operation(policy, "price", &[("rental", of(rental_class))], of(money), |p| {
            p.is_abstract = true
        })?;
        let flat = self.create(rental, "FlatRate", ElementKind::class())?;
        self.field(flat, "perRide", of(money))?;
        self.method(flat, "price", &[("rental", of(rental_class))], of(money))?;
        let minute = self.create(rental, "MinuteRate", ElementKind::class())?;
        self.field(minute, "unlockFee", of(money))?;
        self.field(minute, "perMinute", of(money))?;
        self.attribute(minute, "freeMinutes", integer(), |a| a.default_value = "30".into())?;
        self.attribute(minute, "DAILY_CAP", of(money), |a| {
            a.is_static = true;
            a.is_read_only = true;
            a.default_value = "12 EUR".into();
        })?;
        let price = self.method(minute, "price", &[("rental", of(rental_class))], of(money))?;

        // Relationships, with the ends as the diagram reads them.
        let station_docks = self.association(fleet, station, dock, |p| {
            p.ends[0].aggregation = Aggregation::Composite;
            p.ends[0].navigability = Navigability::NonNavigable;
            p.ends[1].role = "docks".into();
            p.ends[1].multiplicity = Some(Multiplicity::MANY);
            p.ends[1].navigability = Navigability::Navigable;
        })?;
        let dock_vehicle = self.association(fleet, dock, vehicle, |p| {
            p.ends[0].role = "dock".into();
            p.ends[0].multiplicity = Some(Multiplicity::OPTIONAL);
            p.ends[0].navigability = Navigability::Navigable;
            p.ends[1].role = "parked".into();
            p.ends[1].multiplicity = Some(Multiplicity::OPTIONAL);
            p.ends[1].navigability = Navigability::Navigable;
        })?;
        let rider_rentals = self.association(rental, rider, rental_class, |p| {
            p.ends[0].navigability = Navigability::NonNavigable;
            p.ends[1].role = "rentals".into();
            p.ends[1].multiplicity = Some(Multiplicity::MANY);
            p.ends[1].navigability = Navigability::Navigable;
            p.ends[1].constraints = vec!["ordered".into()];
        })?;
        let rental_vehicle = self.association(rental, rental_class, vehicle, |p| {
            p.ends[0].navigability = Navigability::NonNavigable;
            p.ends[1].role = "vehicle".into();
            p.ends[1].multiplicity = Some(Multiplicity::ONE);
            p.ends[1].navigability = Navigability::Navigable;
        })?;
        let rental_policy = self.association(rental, rental_class, policy, |p| {
            p.ends[0].navigability = Navigability::NonNavigable;
            p.ends[1].role = "tariff".into();
            p.ends[1].multiplicity = Some(Multiplicity::ONE);
            p.ends[1].navigability = Navigability::Navigable;
        })?;
        let bike_general = self.create(fleet, "", ElementKind::generalization(bike, vehicle))?;
        let ebike_general = self.create(fleet, "", ElementKind::generalization(ebike, vehicle))?;
        let realizes =
            |client, supplier| ElementKind::Realization(DirectedProps { client, supplier });
        let flat_policy = self.create(rental, "", realizes(flat, policy))?;
        let minute_policy = self.create(rental, "", realizes(minute, policy))?;

        let n_geo = self.class_box(d, geo, (40.0, 40.0))?;
        let n_station = self.class_box(d, station, (230.0, 40.0))?;
        let n_dock = self.class_box(d, dock, (700.0, 40.0))?;
        let n_vehicle = self.class_box(d, vehicle, (980.0, 40.0))?;
        let n_states = self.class_box(d, states, (1290.0, 40.0))?;
        let n_bike = self.class_box(d, bike, (1000.0, 260.0))?;
        let n_ebike = self.class_box(d, ebike, (1190.0, 260.0))?;
        let n_membership = self.class_box(d, membership, (40.0, 360.0))?;
        let n_rider = self.class_box(d, rider, (180.0, 360.0))?;
        let n_rental = self.class_box(d, rental_class, (620.0, 360.0))?;
        let n_money = self.class_box(d, money, (40.0, 580.0))?;
        let n_policy = self.class_box(d, policy, (620.0, 580.0))?;
        let n_flat = self.class_box(d, flat, (440.0, 740.0))?;
        let n_minute = self.class_box(d, minute, (760.0, 740.0))?;
        let _ = (n_geo, n_states, n_membership, n_money);
        self.edge(d, Some(station_docks), n_station, n_dock, &[])?;
        self.edge(d, Some(dock_vehicle), n_dock, n_vehicle, &[])?;
        self.edge(d, Some(rider_rentals), n_rider, n_rental, &[])?;
        self.edge(d, Some(rental_vehicle), n_rental, n_vehicle, &[])?;
        self.edge(d, Some(rental_policy), n_rental, n_policy, &[])?;
        self.edge(d, Some(bike_general), n_bike, n_vehicle, &[])?;
        self.edge(d, Some(ebike_general), n_ebike, n_vehicle, &[])?;
        self.edge(d, Some(flat_policy), n_flat, n_policy, &[])?;
        self.edge(d, Some(minute_policy), n_minute, n_policy, &[])?;
        let body = "The first 30 minutes are free,\nthen every started minute counts.\nNo ride costs more than DAILY_CAP.";
        self.note(rental, d, body, (1100.0, 740.0), &[(minute, n_minute)])?;

        self.structogram(price, &["price(rental: Rental): Money"], price_body())?;
        self.structogram(rebalance, &["rebalance()"], rebalance_body())?;
        self.structogram(release, &["release(): boolean"], release_body())?;

        Ok(Domain {
            station,
            dock,
            vehicle,
            bike,
            ebike,
            rider,
            rental: rental_class,
            station_docks,
            dock_vehicle,
            rider_rentals,
            rental_vehicle,
            release,
        })
    }

    /// The packages and how they depend on each other.
    fn overview(&mut self, d: Id, [fleet, rental, use_cases, architecture]: [Id; 4]) -> Result<()> {
        let root = self.tx.model().root();
        let n_cases = self.node(d, use_cases, (40.0, 60.0, 170.0, 100.0))?;
        let n_rental = self.node(d, rental, (320.0, 60.0, 170.0, 100.0))?;
        let n_fleet = self.node(d, fleet, (600.0, 60.0, 170.0, 100.0))?;
        let n_arch = self.node(d, architecture, (320.0, 260.0, 170.0, 100.0))?;
        let depends =
            |client, supplier| ElementKind::Dependency(DirectedProps { client, supplier });
        for (client, supplier, n_client, n_supplier, stereotype) in [
            (rental, fleet, n_rental, n_fleet, "import"),
            (use_cases, rental, n_cases, n_rental, "trace"),
            (architecture, rental, n_arch, n_rental, "realize"),
        ] {
            let dep = self.stereotyped(root, "", depends(client, supplier), stereotype)?;
            self.edge(d, Some(dep), n_client, n_supplier, &[])?;
        }
        let body = "Velo rents bikes and e-bikes from\n120 stations across the city.\nRiders unlock them with the app and\npay by the minute or per ride.";
        self.note(root, d, body, (580.0, 270.0), &[])?;
        Ok(())
    }

    /// A Saturday morning: Ada rides e-bike E-42, bike B-7 waits in dock 4.
    fn objects(&mut self, owner: Id, dm: &Domain) -> Result<()> {
        let d = self.diagram(owner, "Saturday, 9:12", DiagramKind::Class)?;
        let object = |classifier| {
            ElementKind::Instance(InstanceProps {
                instance: InstanceKind::Object,
                classifier: Some(classifier),
            })
        };
        let mut nodes = Vec::new();
        for (name, class, slots, (x, y)) in [
            (
                "central",
                dm.station,
                &[("name", "\"Central Station\""), ("capacity", "12")][..],
                (40.0, 60.0),
            ),
            ("d3", dm.dock, &[("number", "3")][..], (360.0, 20.0)),
            ("d4", dm.dock, &[("number", "4")][..], (360.0, 160.0)),
            (
                "b7",
                dm.bike,
                &[("id", "\"B-7\""), ("state", "Available"), ("gears", "7")][..],
                (640.0, 150.0),
            ),
            ("ada", dm.rider, &[("name", "\"Ada\""), ("membership", "Plus")][..], (40.0, 300.0)),
            ("r1", dm.rental, &[("start", "2026-10-31 09:12")][..], (360.0, 300.0)),
            (
                "e42",
                dm.ebike,
                &[("id", "\"E-42\""), ("state", "Rented"), ("charge", "81")][..],
                (640.0, 300.0),
            ),
        ] {
            let instance = self.create(owner, name, object(class))?;
            let class_name = self.tx.model().element(class)?.name().to_owned();
            let mut widest = self.measure.width(&format!("{name} : {class_name}"), 12.0, true);
            for (attribute, value) in slots {
                let attribute = umlcraft_notation::object::slot_attributes(self.tx.model(), class)
                    .into_iter()
                    .find(|&a| self.tx.model().get(a).is_some_and(|a| a.name() == *attribute))
                    .expect("the sample's classes have these attributes");
                let props = SlotProps { attribute: Some(attribute), value: (*value).into() };
                let text = umlcraft_notation::object::slot_text(self.tx.model(), attribute, value);
                widest = widest.max(self.width(&text));
                self.create(instance, "", ElementKind::Slot(props))?;
            }
            let height = 32.0 + slots.len() as f32 * 16.0;
            let mut p = Presentation::node(
                Some(instance),
                Rect::new(x, y, (widest + 24.0).max(110.0), height),
            );
            p.style.hide_empty_slots = true;
            nodes.push((instance, self.tx.add_presentation(d, p)?));
        }
        let [central, d3, d4, b7, ada, r1, e42] = nodes[..] else { unreachable!() };
        for (a, b, association, composite) in [
            (central, d3, dm.station_docks, true),
            (central, d4, dm.station_docks, true),
            (d4, b7, dm.dock_vehicle, false),
            (ada, r1, dm.rider_rentals, false),
            (r1, e42, dm.rental_vehicle, false),
        ] {
            let mut kind = ElementKind::link(a.0, b.0);
            if let ElementKind::Link(p) = &mut kind {
                *p = LinkProps { association: Some(association), ..p.clone() };
                if composite {
                    p.ends[0].aggregation = Aggregation::Composite;
                }
            }
            let link = self.create(owner, "", kind)?;
            self.edge(d, Some(link), a.1, b.1, &[])?;
        }
        Ok(())
    }

    fn use_cases(&mut self, owner: Id) -> Result<UseCases> {
        let d = self.diagram(owner, "Riding with Velo", DiagramKind::UseCase)?;
        let system = self.create(owner, "Velo", ElementKind::SystemBoundary)?;
        self.node(d, system, (200.0, 10.0, 560.0, 660.0))?;

        let mut actors = Vec::new();
        for (name, (x, y)) in [
            ("Subscriber", (60.0, 80.0)),
            ("Rider", (60.0, 450.0)),
            ("Fleet manager", (880.0, 70.0)),
            ("Payment provider", (880.0, 380.0)),
            ("Technician", (880.0, 520.0)),
        ] {
            let actor = self.create(owner, name, ElementKind::actor())?;
            actors.push((actor, self.node(d, actor, (x, y, 40.0, 80.0))?));
        }
        let [subscriber, rider, manager, provider, technician] = actors[..] else { unreachable!() };

        let mut cases = Vec::new();
        for (name, point, (x, y)) in [
            ("Manage subscription", "", (240.0, 40.0)),
            ("Reserve a bike", "", (240.0, 140.0)),
            ("Rent a bike", "before unlocking", (240.0, 240.0)),
            ("Find a bike", "", (240.0, 360.0)),
            ("Return a bike", "after locking", (240.0, 450.0)),
            ("Report damage", "", (240.0, 590.0)),
            ("Rebalance stations", "", (520.0, 60.0)),
            ("Unlock the bike", "", (520.0, 250.0)),
            ("Pay for the ride", "", (520.0, 460.0)),
            ("Repair a vehicle", "", (520.0, 590.0)),
        ] {
            let case = self.create(owner, name, ElementKind::use_case())?;
            // The name goes in the ellipse's inner box, about 60 % of its width.
            let mut width =
                (self.measure.width(name, scene::FONT_SIZE, true) / 0.6 + 30.0).max(150.0);
            let mut height = 46.0;
            if !point.is_empty() {
                self.create(case, point, ElementKind::ExtensionPoint)?;
                let points = self.width("extension points").max(self.width(point));
                width = width.max(points / 0.7 + 20.0);
                height = 80.0;
            }
            cases.push((case, self.node(d, case, (x, y, width, height))?));
        }
        let [subscription, reserve, rent, find, give_back, damage, rebalance, unlock, pay, repair] =
            cases[..]
        else {
            unreachable!()
        };

        for (actor, case) in [
            (subscriber, subscription),
            (subscriber, reserve),
            (rider, rent),
            (rider, find),
            (rider, give_back),
            (rider, damage),
            (manager, rebalance),
            (provider, pay),
            (technician, repair),
        ] {
            let association = self.create(owner, "", ElementKind::association(actor.0, case.0))?;
            self.edge(d, Some(association), actor.1, case.1, &[])?;
        }
        let general = self.create(owner, "", ElementKind::generalization(subscriber.0, rider.0))?;
        self.edge(d, Some(general), subscriber.1, rider.1, &[])?;
        for (including, addition) in [(rent, unlock), (give_back, pay)] {
            let props = IncludeProps { including: including.0, addition: addition.0 };
            let include = self.create(owner, "", ElementKind::Include(props))?;
            self.edge(d, Some(include), including.1, addition.1, &[])?;
        }
        for (extension, extended, condition) in
            [(reserve, rent, ""), (damage, give_back, "damage noticed")]
        {
            let points = self.tx.model().children(extended.0).to_vec();
            let props = ExtendProps {
                extension: extension.0,
                extended: extended.0,
                extension_points: points,
                condition: condition.into(),
            };
            let extend = self.create(owner, "", ElementKind::Extend(props))?;
            self.edge(d, Some(extend), extension.1, extended.1, &[])?;
        }
        let body = "Opens the lock by QR code\nor with the Velo card.";
        self.note(owner, d, body, (540.0, 340.0), &[unlock])?;
        Ok(UseCases { rider: rider.0, rent: rent.0, give_back: give_back.0 })
    }

    fn architecture(&mut self, owner: Id) -> Result<Architecture> {
        let d = self.diagram(owner, "Architecture", DiagramKind::Component)?;
        let component = |c| ElementKind::Component(c);
        let app = self.create(owner, "MobileApp", component(classifier()))?;
        let service = self.create(owner, "RentalService", component(classifier()))?;
        let pricing = self.create(service, "Pricing", component(classifier()))?;
        let accounts = self.create(service, "Accounts", component(classifier()))?;
        let payment = self.create(owner, "PaymentGateway", component(classifier()))?;
        let fleet = self.create(owner, "FleetService", component(classifier()))?;
        let controller = self.create(owner, "DockController", component(classifier()))?;
        let api = self.create(owner, "RentalAPI", ElementKind::interface())?;
        let payments = self.create(owner, "Payments", ElementKind::interface())?;
        let telemetry = self.create(owner, "Telemetry", ElementKind::interface())?;
        let artifact = |c| ElementKind::Artifact(c);
        let service_jar = self.create(owner, "rental-service.jar", artifact(classifier()))?;
        let fleet_jar = self.create(owner, "fleet-service.jar", artifact(classifier()))?;
        let firmware = self.create(owner, "dock-firmware.bin", artifact(classifier()))?;

        let n_app = self.node(d, app, (40.0, 80.0, 150.0, 64.0))?;
        let n_service = self.node(d, service, (380.0, 40.0, 330.0, 170.0))?;
        self.node(d, pricing, (400.0, 100.0, 140.0, 64.0))?;
        self.node(d, accounts, (555.0, 100.0, 140.0, 64.0))?;
        let n_payment = self.node(d, payment, (930.0, 80.0, 180.0, 64.0))?;
        let n_fleet = self.node(d, fleet, (430.0, 340.0, 230.0, 64.0))?;
        let n_controller = self.node(d, controller, (40.0, 340.0, 170.0, 64.0))?;
        let n_api = self.lollipop(d, api, (290.0, 100.0))?;
        let n_payments = self.lollipop(d, payments, (820.0, 100.0))?;
        let n_telemetry = self.lollipop(d, telemetry, (330.0, 360.0))?;
        let n_service_jar = self.node(d, service_jar, (930.0, 290.0, 180.0, 64.0))?;
        let n_fleet_jar = self.node(d, fleet_jar, (455.0, 490.0, 180.0, 64.0))?;
        let n_firmware = self.node(d, firmware, (35.0, 490.0, 180.0, 64.0))?;

        for (client, supplier, n_client, n_supplier) in [
            (service, api, n_service, n_api),
            (payment, payments, n_payment, n_payments),
            (fleet, telemetry, n_fleet, n_telemetry),
        ] {
            let realization = ElementKind::Realization(DirectedProps { client, supplier });
            let realization = self.create(owner, "", realization)?;
            self.edge(d, Some(realization), n_client, n_supplier, &[])?;
        }
        for (client, supplier, n_client, n_supplier, stereotype) in [
            (app, api, n_app, n_api, "use"),
            (service, payments, n_service, n_payments, "use"),
            (controller, telemetry, n_controller, n_telemetry, "use"),
            (service, fleet, n_service, n_fleet, "use"),
            (service_jar, service, n_service_jar, n_service, "manifest"),
            (fleet_jar, fleet, n_fleet_jar, n_fleet, "manifest"),
            (firmware, controller, n_firmware, n_controller, "manifest"),
        ] {
            let dependency = ElementKind::Dependency(DirectedProps { client, supplier });
            let dependency = self.stereotyped(owner, "", dependency, stereotype)?;
            self.edge(d, Some(dependency), n_client, n_supplier, &[])?;
        }
        Ok(Architecture { app, rental_service: service, payment })
    }

    fn deployment(&mut self, owner: Id) -> Result<()> {
        let d = self.diagram(owner, "Production", DiagramKind::Deployment)?;
        let model = self.tx.model();
        let named = |name: &str| {
            model
                .children(owner)
                .iter()
                .copied()
                .find(|&c| model.get(c).is_some_and(|e| e.name() == name))
        };
        let (service_jar, fleet_jar, firmware) = (
            named("rental-service.jar").expect("made with the architecture"),
            named("fleet-service.jar").expect("made with the architecture"),
            named("dock-firmware.bin").expect("made with the architecture"),
        );
        let app = self.create(owner, "velo-app.apk", ElementKind::Artifact(classifier()))?;
        let node = |c| ElementKind::Node(c);
        let phone = self.stereotyped(owner, "Smartphone", node(classifier()), "device")?;
        let cloud = self.stereotyped(owner, "Cloud", node(classifier()), "Kubernetes")?;
        let rental_pod = self.create(cloud, "rental-pod", node(classifier()))?;
        let fleet_pod = self.create(cloud, "fleet-pod", node(classifier()))?;
        let database = self.stereotyped(cloud, "PostgreSQL", node(classifier()), "database")?;
        let dock = self.stereotyped(owner, "Dock unit", node(classifier()), "device")?;
        let provider =
            self.stereotyped(owner, "Payment provider", node(classifier()), "external")?;

        let n_phone = self.node(d, phone, (40.0, 60.0, 220.0, 140.0))?;
        self.node(d, app, (75.0, 115.0, 150.0, 60.0))?;
        self.node(d, cloud, (330.0, 40.0, 480.0, 400.0))?;
        let n_rental = self.node(d, rental_pod, (355.0, 95.0, 200.0, 120.0))?;
        self.node(d, service_jar, (370.0, 140.0, 170.0, 56.0))?;
        let n_fleet = self.node(d, fleet_pod, (585.0, 95.0, 200.0, 120.0))?;
        self.node(d, fleet_jar, (600.0, 140.0, 170.0, 56.0))?;
        let n_database = self.node(d, database, (470.0, 290.0, 200.0, 90.0))?;
        let n_dock = self.node(d, dock, (40.0, 330.0, 220.0, 130.0))?;
        self.node(d, firmware, (65.0, 380.0, 170.0, 56.0))?;
        let n_provider = self.node(d, provider, (880.0, 95.0, 200.0, 90.0))?;

        for (a, b, n_a, n_b, stereotype, waypoints) in [
            (phone, rental_pod, n_phone, n_rental, "HTTPS", &[][..]),
            // Below the database, then straight up into the pod.
            (dock, fleet_pod, n_dock, n_fleet, "MQTT", &[(735.0, 395.0)][..]),
            (rental_pod, database, n_rental, n_database, "JDBC", &[][..]),
            (fleet_pod, database, n_fleet, n_database, "JDBC", &[][..]),
            (
                rental_pod,
                provider,
                n_rental,
                n_provider,
                "HTTPS",
                &[(455.0, 20.0), (980.0, 20.0)][..],
            ),
        ] {
            let association = ElementKind::association(a, b);
            let path = self.stereotyped(owner, "", association, stereotype)?;
            let edge = self.edge(d, Some(path), n_a, n_b, waypoints)?;
            if stereotype == "MQTT" {
                let y = center(self.tx.model(), d, n_fleet).y;
                self.aim(d, edge, None, Some((735.0, y)))?;
            }
        }
        Ok(())
    }

    /// Renting a bike, from scanning the code to the ride.
    fn renting(&mut self, dm: &Domain, cases: &UseCases, parts: &Architecture) -> Result<()> {
        let interaction = self.create(cases.rent, "Rent a bike", ElementKind::Interaction)?;
        let mut lifelines = Vec::new();
        for (name, represents) in [
            ("ada", cases.rider),
            ("app", parts.app),
            ("service", parts.rental_service),
            ("r", dm.rental),
            ("d4", dm.dock),
            ("gateway", parts.payment),
        ] {
            let props = LifelineProps { represents: Some(represents) };
            lifelines.push(self.create(interaction, name, ElementKind::Lifeline(props))?);
        }
        let [ada, app, service, rental, dock, gateway] = lifelines[..] else { unreachable!() };
        // The diagram comes first: what it does not show stays before what it shows.
        let d = self.diagram(interaction, "Rent a bike", DiagramKind::Sequence)?;
        let mut lanes = Vec::new();
        // The rental right of the dock, so that the call to the dock and its label run free.
        for (lifeline, x) in lifelines.iter().zip([40.0, 200.0, 380.0, 760.0, 590.0, 920.0]) {
            let lane = sequence_edit::add_lifeline(self.tx, d, *lifeline, x, 50.0)?;
            // Heads as wide as `name : Type`.
            let model = self.tx.model();
            let ElementKind::Lifeline(props) = model.element(*lifeline)?.kind() else {
                unreachable!()
            };
            let represents = props.represents.and_then(|r| model.get(r)).map(|r| r.name());
            let head =
                format!("{} : {}", model.element(*lifeline)?.name(), represents.unwrap_or(""));
            let width = (self.width(&head) + 24.0).max(scene::FONT_SIZE * 9.0);
            self.tx.update_presentation(d, lane, |p| {
                if let Shape::Lifeline { head, .. } = &mut p.shape {
                    head.width = width;
                }
            })?;
            lanes.push(lane);
        }
        self.tx.update_presentation(d, lanes[0], |p| p.style.iconic = true)?;
        let lane = |lifeline: Id| lanes[lifelines.iter().position(|&l| l == lifeline).unwrap()];

        // The fragments first, their operands as high as what goes in them.
        let fragment = |operator, covered: &[Id]| {
            ElementKind::CombinedFragment(CombinedFragmentProps {
                operator,
                covered: covered.to_vec(),
            })
        };
        let operand = |guard: &str| {
            ElementKind::InteractionOperand(InteractionOperandProps { guard: guard.into() })
        };
        let covered = [app, service, rental, dock];
        let alt = self.create(interaction, "", fragment(InteractionOperator::Alt, &covered))?;
        self.create(alt, "", operand("approved"))?;
        self.create(alt, "", operand("else"))?;
        let n_alt = sequence_edit::add_fragment(self.tx, d, alt, &covered, 320.0)?;
        self.tx.update_presentation(d, n_alt, |p| {
            if let Shape::Fragment { bounds, separators, .. } = &mut p.shape {
                bounds.height = 240.0;
                separators.push(175.0);
            }
        })?;
        let riding = [app, service];
        let repeat = self.create(interaction, "", fragment(InteractionOperator::Loop, &riding))?;
        self.create(repeat, "", operand("every 30 s while riding"))?;
        let n_loop = sequence_edit::add_fragment(self.tx, d, repeat, &riding, 600.0)?;
        self.tx.update_presentation(d, n_loop, |p| {
            if let Shape::Fragment { bounds, .. } = &mut p.shape {
                bounds.height = 80.0;
            }
        })?;

        use MessageSort::{Asynchronous, Create, Reply, Synchronous};
        for (name, sort, from, to, arguments, y) in [
            ("scan", Synchronous, ada, app, "code", 120.0),
            ("startRental", Asynchronous, app, service, "ada, d4", 160.0),
            ("checkAccount", Synchronous, service, service, "ada", 200.0),
            ("authorize", Synchronous, service, gateway, "deposit", 270.0),
            ("", Reply, gateway, service, "approved", 295.0),
            ("new", Create, service, rental, "ada, e42", 360.0),
            ("release", Synchronous, service, dock, "", 420.0),
            ("", Reply, dock, service, "true", 445.0),
            ("rentalStarted", Asynchronous, service, app, "r", 475.0),
            ("declined", Asynchronous, service, app, "reason", 530.0),
            ("track", Asynchronous, app, service, "position", 650.0),
        ] {
            let mut kind = ElementKind::message(sort, from, to);
            if let ElementKind::Message(p) = &mut kind {
                if sort == Reply {
                    p.return_value = arguments.into();
                } else {
                    p.arguments = arguments.into();
                }
                if name == "release" {
                    p.operation = Some(dm.release);
                }
            }
            let message = self.create(interaction, name, kind)?;
            sequence_edit::add_message(self.tx, d, message, lane(from), lane(to), y)?;
            // Calls that last: the app runs `scan` until the ride is over, so the rental's news
            // and the tracking happen inside it; the service works on the rental until it
            // answers the app. What either sends meanwhile starts inside that bar.
            let bottom = match name {
                "scan" => Some(700.0),
                "startRental" => Some(545.0),
                _ => None,
            };
            if let Some(bottom) = bottom {
                self.lengthen_bar(d, lane(to), y, bottom)?;
            }
        }
        // All lifelines run down to the same height.
        let view = self.tx.model().view(d).ok_or(ModelError::NotADiagram(d))?;
        let ends: Vec<(Id, f32)> = lanes
            .iter()
            .filter_map(|&lane| match view.get(lane)?.shape {
                Shape::Lifeline { head, length, .. } => Some((lane, head.bottom() + length)),
                _ => None,
            })
            .collect();
        let bottom = ends.iter().map(|(_, end)| *end).fold(0.0, f32::max);
        for (lane, end) in ends {
            self.tx.update_presentation(d, lane, |p| {
                if let Shape::Lifeline { length, .. } = &mut p.shape {
                    *length += bottom - end;
                }
            })?;
        }
        sequence_edit::tidy(self.tx, d)
    }

    /// Lets the bar on `lane` that starts at `top` run down to `bottom`.
    fn lengthen_bar(&mut self, d: Id, lane: Id, top: f32, bottom: f32) -> Result<()> {
        let view = self.tx.model().view(d).ok_or(ModelError::NotADiagram(d))?;
        let bar = view.presentations().iter().find_map(|p| match p.shape {
            Shape::Activation { lifeline, top: t, .. }
                if lifeline == lane && (t - top).abs() < 0.5 =>
            {
                Some(p.id)
            }
            _ => None,
        });
        let Some(bar) = bar else { return Ok(()) };
        self.tx.update_presentation(d, bar, |p| {
            if let Shape::Activation { height, .. } = &mut p.shape {
                *height = bottom - top;
            }
        })
    }

    /// Bringing a bike back: the dock locks it, the backend closes the rental.
    fn returning(&mut self, cases: &UseCases) -> Result<()> {
        let activity = self.create(cases.give_back, "Return a bike", ElementKind::Activity)?;
        let d = self.diagram(activity, "Return a bike", DiagramKind::Activity)?;
        let node = ElementKind::activity_node;
        use ActivityNodeKind::*;
        let invoice = ElementKind::ActivityNode(ActivityNodeProps {
            node: Object,
            type_ref: None,
            in_state: "sent".into(),
        });
        let mut nodes = Vec::new();
        for (name, kind, (x, y, w, h)) in [
            ("Rider", ElementKind::Partition, (40.0, 40.0, 240.0, 820.0)),
            ("Dock", ElementKind::Partition, (280.0, 40.0, 240.0, 820.0)),
            ("Backend", ElementKind::Partition, (520.0, 40.0, 380.0, 820.0)),
            ("", node(Initial), (150.0, 90.0, 20.0, 20.0)),
            ("Ride to a station", node(Action), (90.0, 140.0, 140.0, 36.0)),
            ("Push the bike into a dock", node(Action), (60.0, 215.0, 200.0, 36.0)),
            ("Lock the bike", node(Action), (340.0, 215.0, 120.0, 36.0)),
            ("", node(Fork), (340.0, 300.0, 500.0, 5.0)),
            ("Run a self-test", node(Action), (335.0, 350.0, 130.0, 36.0)),
            ("Close the rental", node(Action), (560.0, 350.0, 140.0, 36.0)),
            ("Update the map", node(Action), (730.0, 350.0, 140.0, 36.0)),
            ("", node(FlowFinal), (790.0, 430.0, 20.0, 20.0)),
            ("", node(Join), (340.0, 430.0, 350.0, 5.0)),
            ("", node(Decision), (623.0, 470.0, 24.0, 24.0)),
            ("Open a repair ticket", node(Action), (545.0, 545.0, 180.0, 36.0)),
            ("", node(Merge), (623.0, 615.0, 24.0, 24.0)),
            ("Charge the rider", node(Action), (565.0, 675.0, 140.0, 36.0)),
            ("Invoice", invoice, (575.0, 750.0, 120.0, 40.0)),
            ("Read the receipt", node(Action), (90.0, 750.0, 140.0, 36.0)),
            ("", node(ActivityFinal), (150.0, 820.0, 20.0, 20.0)),
        ] {
            let element = self.create(activity, name, kind)?;
            nodes.push((element, self.node(d, element, (x, y, w, h))?));
        }
        let [
            _,
            _,
            _,
            initial,
            ride,
            push,
            lock,
            fork,
            test,
            close,
            map,
            map_done,
            join,
            damaged,
            ticket,
            merge,
            charge,
            invoice,
            receipt,
            done,
        ] = nodes[..]
        else {
            unreachable!()
        };
        for (from, to, guard, waypoints) in [
            (initial, ride, "", &[][..]),
            (ride, push, "", &[][..]),
            (push, lock, "", &[][..]),
            (lock, fork, "", &[][..]),
            (fork, test, "", &[][..]),
            (fork, close, "", &[][..]),
            (fork, map, "", &[][..]),
            (map, map_done, "", &[][..]),
            (test, join, "", &[][..]),
            (close, join, "", &[][..]),
            (join, damaged, "", &[][..]),
            (damaged, ticket, "damage found", &[][..]),
            (damaged, merge, "else", &[(760.0, 482.0), (760.0, 627.0)][..]),
            (ticket, merge, "", &[][..]),
            (merge, charge, "", &[][..]),
            (charge, invoice, "", &[][..]),
            (invoice, receipt, "", &[][..]),
            (receipt, done, "", &[][..]),
        ] {
            let mut kind = ElementKind::flow(from.0, to.0);
            if let ElementKind::ActivityEdge(p) = &mut kind {
                p.guard = guard.into();
            }
            let flow = self.create(activity, "", kind)?;
            let edge = self.edge(d, Some(flow), from.1, to.1, waypoints)?;
            // Flows meet the fork and the join straight above or below their other end.
            let c = |p: Id| center(self.tx.model(), d, p);
            let (source, target) = if from == fork || from == join {
                (Some((c(to.1).x, c(from.1).y)), None)
            } else if to == fork || to == join {
                (None, Some((c(from.1).x, c(to.1).y)))
            } else {
                (None, None)
            };
            if source.is_some() || target.is_some() {
                self.aim(d, edge, source, target)?;
            }
        }
        Ok(())
    }

    /// The life of a vehicle: available, reserved, rented (riding and draining its battery at
    /// once), repaired, retired.
    fn lifecycle(&mut self, dm: &Domain) -> Result<()> {
        let machine = self.create(dm.vehicle, "Vehicle lifecycle", ElementKind::StateMachine)?;
        let d = self.diagram(machine, "Vehicle lifecycle", DiagramKind::StateMachine)?;
        let state = |entry: &str, activity: &str, exit: &str| {
            ElementKind::State(StateProps {
                entry: entry.into(),
                do_activity: activity.into(),
                exit: exit.into(),
            })
        };
        let pseudo = ElementKind::pseudostate;
        use PseudostateKind::*;

        // Vertices as (owner, name, kind, bounds); owners come before what they hold. Owner
        // `None` is the state machine, `Some(i)` the vertex or region with index `i`.
        let rented = state("", "", "");
        let mut vertices: Vec<(Id, Option<Id>)> = Vec::new();
        let regions_of_rented = 3;
        for (owner, name, kind, bounds) in [
            (None, "", pseudo(Initial), Some((40.0, 255.0, 20.0, 20.0))),
            (None, "Available", state("light green", "", ""), Some((90.0, 215.0, 200.0, 90.0))),
            (
                None,
                "Reserved",
                state("light blue", "", "light off"),
                Some((370.0, 40.0, 170.0, 76.0)),
            ),
            (None, "Rented", rented, Some((640.0, 170.0, 420.0, 270.0))),
            (Some(regions_of_rented), "", ElementKind::Region, None),
            (Some(regions_of_rented), "", ElementKind::Region, None),
            (Some(4), "", pseudo(Initial), Some((660.0, 240.0, 20.0, 20.0))),
            (Some(4), "Riding", state("", "", ""), Some((710.0, 225.0, 110.0, 50.0))),
            (Some(4), "Paused", state("", "", ""), Some((915.0, 225.0, 110.0, 50.0))),
            (Some(5), "", pseudo(Initial), Some((660.0, 370.0, 20.0, 20.0))),
            (Some(5), "Charged", state("", "", ""), Some((710.0, 355.0, 100.0, 50.0))),
            (
                Some(5),
                "Low battery",
                state("warn the rider", "", ""),
                Some((905.0, 350.0, 135.0, 60.0)),
            ),
            (None, "", pseudo(Choice), Some((1200.0, 293.0, 24.0, 24.0))),
            (None, "In repair", state("", "", ""), Some((330.0, 500.0, 520.0, 240.0))),
            (Some(13), "", pseudo(DeepHistory), Some((355.0, 560.0, 24.0, 24.0))),
            (
                Some(13),
                "Diagnosing",
                state("", "run diagnostics", ""),
                Some((410.0, 545.0, 160.0, 60.0)),
            ),
            (Some(13), "Fixing", state("", "", ""), Some((620.0, 550.0, 110.0, 50.0))),
            (Some(13), "Testing", state("", "", ""), Some((620.0, 640.0, 110.0, 50.0))),
            (Some(13), "", ElementKind::FinalState, Some((790.0, 655.0, 20.0, 20.0))),
            (
                None,
                "Waiting for parts",
                state("order parts", "", ""),
                Some((60.0, 560.0, 190.0, 66.0)),
            ),
            (
                None,
                "Retired",
                state("remove from the app", "", ""),
                Some((395.0, 800.0, 190.0, 66.0)),
            ),
            (None, "", ElementKind::FinalState, Some((680.0, 823.0, 20.0, 20.0))),
        ] {
            let owner = owner.map_or(machine, |i: usize| vertices[i].0);
            let element = self.create(owner, name, kind)?;
            let node = match bounds {
                Some(b) => Some(self.node(d, element, b)?),
                None => None,
            };
            vertices.push((element, node));
        }
        let n_rented = vertices[3].1.expect("shown");
        self.tx.update_presentation(d, n_rented, |p| {
            if let Shape::Node { regions, .. } = &mut p.shape {
                *regions = Some(Regions { vertical: false, separators: vec![150.0] });
            }
        })?;
        let charging = InternalTransitionProps {
            trigger: "battery low".into(),
            effect: "request charging".into(),
            ..Default::default()
        };
        self.create(vertices[1].0, "", ElementKind::InternalTransition(charging))?;

        let v = |i: usize| (vertices[i].0, vertices[i].1.expect("shown"));
        let [initial, available, reserved, rented] = [v(0), v(1), v(2), v(3)];
        let [ride_start, riding, paused, charge_start, charged, low] =
            [v(6), v(7), v(8), v(9), v(10), v(11)];
        let [choice, repair, history, diagnosing, fixing, testing, repaired] =
            [v(12), v(13), v(14), v(15), v(16), v(17), v(18)];
        let [waiting, retired, end] = [v(19), v(20), v(21)];
        for (from, to, (trigger, guard, effect), waypoints) in [
            (initial, available, ("", "", ""), &[][..]),
            (available, reserved, ("reserve", "", "hold for 15 min"), &[][..]),
            (reserved, available, ("after 15 min", "", ""), &[(190.0, 78.0)][..]),
            (reserved, rented, ("unlock", "", ""), &[][..]),
            (available, rented, ("unlock", "account ok", ""), &[][..]),
            (ride_start, riding, ("", "", ""), &[][..]),
            (riding, paused, ("pause", "", ""), &[][..]),
            (paused, riding, ("resume", "", ""), &[(970.0, 300.0), (765.0, 300.0)][..]),
            (charge_start, charged, ("", "", ""), &[][..]),
            (charged, low, ("", "charge < 20 %", ""), &[][..]),
            (rented, choice, ("lock", "", "end rental"), &[][..]),
            (choice, available, ("", "no damage", ""), &[(1212.0, 15.0), (140.0, 15.0)][..]),
            (choice, repair, ("", "damage reported", ""), &[(1212.0, 620.0)][..]),
            (history, diagnosing, ("", "", ""), &[][..]),
            (diagnosing, fixing, ("", "fault found", ""), &[][..]),
            (fixing, testing, ("", "", ""), &[][..]),
            (testing, fixing, ("", "failed", ""), &[(760.0, 665.0), (760.0, 575.0)][..]),
            (testing, repaired, ("", "passed", ""), &[(675.0, 710.0), (800.0, 710.0)][..]),
            (repair, available, ("", "", "back to the fleet"), &[][..]),
            (repair, waiting, ("parts missing", "", ""), &[][..]),
            (waiting, history, ("parts arrived", "", ""), &[(155.0, 680.0), (367.0, 680.0)][..]),
            (diagnosing, retired, ("", "beyond repair", ""), &[][..]),
            (retired, end, ("", "", ""), &[][..]),
        ] {
            let kind = ElementKind::Transition(TransitionProps {
                source: from.0,
                target: to.0,
                trigger: trigger.into(),
                guard: guard.into(),
                effect: effect.into(),
            });
            let transition = self.create(machine, "", kind)?;
            let edge = self.edge(d, Some(transition), from.1, to.1, waypoints)?;
            // The way back from the choice comes straight down into the state.
            if (from, to) == (choice, available) {
                let y = center(self.tx.model(), d, available.1).y;
                self.aim(d, edge, None, Some((140.0, y)))?;
            }
        }
        Ok(())
    }
}

/// `MinuteRate.price`: free minutes, the membership's discount, the daily cap.
fn price_body() -> Vec<Block> {
    let mut free = Block::alternative(&line("minutes <= freeMinutes"));
    free.branches[0] = Sequence::new(vec![Block::jump(&line("return 0"))]);
    let mut discount =
        Block::case(&lines(&["rental.rider.membership", "Plus", "Student", "default"]));
    discount.branches[0] = Sequence::new(vec![Block::instruction(&line("cost <- cost * 0.8"))]);
    discount.branches[1] = Sequence::new(vec![Block::instruction(&line("cost <- cost * 0.5"))]);
    let mut cap = Block::alternative(&line("cost > DAILY_CAP"));
    let mut capped = Block::instruction(&line("cost <- DAILY_CAP"));
    capped.set_color(Color([0xff, 0xf2, 0xcc]));
    cap.branches[0] = Sequence::new(vec![capped]);
    vec![
        Block::instruction(&line("minutes <- ceil(rental.duration() / 1 min)")),
        free,
        Block::instruction(&lines(&[
            "cost <- unlockFee",
            "cost <- cost + (minutes - freeMinutes) * perMinute",
        ])),
        discount,
        cap,
        Block::jump(&line("return cost")),
    ]
}

/// `Station.rebalance`: asks for bikes when too many docks are empty, for a pickup when too
/// few are.
fn rebalance_body() -> Vec<Block> {
    let mut count = Block::alternative(&line("dock.isEmpty()"));
    count.branches[0] = Sequence::new(vec![Block::instruction(&line("empty <- empty + 1"))]);
    let mut each =
        Block::for_loop(&line("foreach dock in docks"), ForLoop::Traversing { var: "dock" });
    each.branches[0] = Sequence::new(vec![count]);
    let mut few = Block::alternative(&line("empty < 2"));
    let mut pickup = Block::call(&line("requestPickup(2 - empty)"));
    pickup.set_comment(&line("A van takes bikes to a station that needs them."));
    few.branches[0] = Sequence::new(vec![pickup]);
    let mut many = Block::alternative(&line("empty > capacity / 2"));
    many.branches[0] =
        Sequence::new(vec![Block::call(&line("requestDelivery(empty - capacity / 2)"))]);
    many.branches[1] = Sequence::new(vec![few]);
    let mut wait = Block::while_loop(&line("while deliveryPending()"));
    wait.branches[0] = Sequence::new(vec![Block::call(&line("sleep(5 min)"))]);
    vec![Block::instruction(&line("empty <- 0")), each, many, wait]
}

/// `Dock.release`: pulls the latch up to three times, then lights up and beeps at once.
fn release_body() -> Vec<Block> {
    let mut pull = Block::repeat_loop(&line("until latch.isOpen() or tries = 3"));
    pull.branches[0] = Sequence::new(vec![
        Block::call(&line("latch.pull()")),
        Block::instruction(&line("tries <- tries + 1")),
    ]);
    let mut signal = Block::parallel(2);
    signal.branches[0] = Sequence::new(vec![Block::call(&line("led.show(GREEN)"))]);
    signal.branches[1] = Sequence::new(vec![Block::call(&line("speaker.beep()"))]);
    let mut attempt = Block::try_block("e");
    attempt.branches[0] = Sequence::new(vec![
        Block::instruction(&line("tries <- 0")),
        pull,
        signal,
        Block::jump(&line("return latch.isOpen()")),
    ]);
    let mut report = Block::call(&line("reportFault(e)"));
    report.set_color(Color([0xff, 0xd6, 0xd6]));
    attempt.branches[1] = Sequence::new(vec![report, Block::jump(&line("return false"))]);
    attempt.branches[2] = Sequence::new(vec![Block::call(&line("log(\"release\", tries)"))]);
    vec![attempt]
}

#[cfg(test)]
mod tests {
    // No glob import: gpui_kit's `test` macro would shadow the built-in `#[test]`.
    use umlcraft_model::{DiagramKind, Document, ElementKind, Id, Shape};
    use umlcraft_notation::scene::{self, ApproxMeasure};

    use super::velo;

    #[test]
    fn the_sample_has_every_kind_of_diagram_and_structograms() {
        let doc = velo(&ApproxMeasure);
        let model = doc.model();
        model.check().unwrap();
        let kinds: Vec<DiagramKind> =
            model.elements().filter_map(|e| e.kind().as_diagram()).collect();
        for kind in [
            DiagramKind::Class,
            DiagramKind::UseCase,
            DiagramKind::Sequence,
            DiagramKind::Activity,
            DiagramKind::StateMachine,
            DiagramKind::Component,
            DiagramKind::Deployment,
        ] {
            assert!(kinds.contains(&kind), "no {kind:?} diagram");
        }
        let structograms = model
            .elements()
            .filter(|e| matches!(e.kind(), ElementKind::Structogram(p) if p.operation.is_some()))
            .count();
        assert_eq!(structograms, 3);
        assert!(!doc.can_undo());
        // Every diagram draws.
        for (diagram, _) in model.views() {
            scene::build(model, diagram, &scene::Preview::default(), &ApproxMeasure);
        }
    }

    #[test]
    fn the_sample_survives_a_round_trip_through_the_file_format() {
        let doc = velo(&ApproxMeasure);
        let text = umlcraft_format::to_string(doc.model());
        let back = umlcraft_format::from_str(&text).unwrap();
        assert_eq!(umlcraft_format::to_string(&back), text);
    }

    /// Writes every diagram of the sample as a PNG to the folder `UMLCRAFT_SAMPLE_PNG` names,
    /// to look at the layout: `cargo test -p umlcraft-app sample_png -- --ignored`.
    #[test]
    #[ignore]
    fn sample_png() {
        let Ok(dir) = std::env::var("UMLCRAFT_SAMPLE_PNG") else { return };
        write_pngs(&velo(&ApproxMeasure), std::path::Path::new(&dir), "");
    }

    /// Like `sample_png`, after arranging every diagram that can be and routing the lines of
    /// the others, with `arranged_` before the names.
    #[test]
    #[ignore]
    fn sample_arranged_png() {
        use umlcraft_notation::{autoroute, layout};
        let Ok(dir) = std::env::var("UMLCRAFT_SAMPLE_PNG") else { return };
        let mut doc = velo(&ApproxMeasure);
        let diagrams: Vec<(Id, DiagramKind)> = doc
            .model()
            .views()
            .filter_map(|(d, _)| Some((d, doc.model().get(d)?.kind().as_diagram()?)))
            .collect();
        for (diagram, kind) in diagrams {
            doc.edit("layout", |tx| {
                if layout::can_arrange(kind) {
                    let down = layout::Direction::Down;
                    layout::arrange(tx, diagram, None, down, &ApproxMeasure).map(|_| ())
                } else {
                    let view = tx.model().view(diagram).unwrap();
                    let edges: Vec<Id> = view
                        .presentations()
                        .iter()
                        .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
                        .map(|p| p.id)
                        .collect();
                    autoroute::route_lines(tx, diagram, &edges).map(|_| ())
                }
            })
            .unwrap();
        }
        doc.model().check().unwrap();
        write_pngs(&doc, std::path::Path::new(&dir), "arranged_");
    }

    fn write_pngs(doc: &Document, dir: &std::path::Path, prefix: &str) {
        let model = doc.model();
        for (diagram, _) in model.views() {
            let scene = scene::build(model, diagram, &scene::Preview::default(), &ApproxMeasure);
            let svg = umlcraft_notation::svg::render(&scene, &ApproxMeasure);
            let png = crate::png::from_svg(&svg, 1.0).unwrap();
            let name = model.element(diagram).unwrap().name().replace([',', ':', ' '], "_");
            std::fs::write(dir.join(format!("{prefix}{name}.png")), png).unwrap();
        }
    }
}
