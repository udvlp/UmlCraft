//! The presentations of diagrams other than sequence diagrams: nodes, notes, boundaries and
//! edges with their routes and labels.

use super::*;

impl Importer<'_> {
    /// The diagram's presentations from back to front; a greater depth lies further back.
    pub(super) fn presentations_back_to_front(&self, object: ObjId) -> Vec<ObjId> {
        let mut sources = self.r.list(object, "presentation");
        let depth = |p: ObjId| self.r.g.field(p, "depth").and_then(Value::as_i64).unwrap_or(0);
        sources.sort_by_key(|&p| std::cmp::Reverse(depth(p)));
        sources
    }

    /// Location and size of a node presentation as stored.
    pub(super) fn rect(&self, p: ObjId) -> Option<Rect> {
        let r = self.r;
        let at = r.obj(p, "location").and_then(|l| r.point(l))?;
        Some(Rect::new(at.x, at.y, r.num(p, "width")? as f32, r.num(p, "height")? as f32))
    }

    /// Adds a presentation, at the back or in front of everything so far.
    pub(super) fn insert(
        &mut self,
        diagram: Id,
        presentation: Presentation,
        back: bool,
    ) -> Option<Id> {
        let index = if back { 0 } else { self.model.view(diagram)?.presentations().len() };
        let id = presentation.id;
        if self.model.apply(Op::InsertPresentation { diagram, index, presentation }).is_err() {
            self.lost_presentations += 1;
            return None;
        }
        Some(id)
    }

    pub(super) fn fill(&self, p: ObjId) -> Style {
        let mut style = Style::default();
        if let Some(fill) =
            self.r.style(p, "fill.color").filter(|c| !c.eq_ignore_ascii_case(DEFAULT_FILL))
        {
            style.fill = fill.to_ascii_lowercase().parse().ok();
        }
        // Astah stores a font size of its own with `font.size.auto` false.
        style.font_size = self
            .r
            .style(p, "font.size")
            .filter(|_| self.r.style(p, "font.size.auto").as_deref() == Some("false"))
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|&size| size.is_finite() && size > 0.0 && size != scene::FONT_SIZE);
        style.font_color = self.ink(p, "font.color");
        style.hide_stereotypes = self.r.g.bool_field(p, "stereotypeVisibility") == Some(false);
        style
    }

    /// What a classifier box leaves out of its members. Astah keeps the switches as fields of
    /// the presentation, the visibilities filtered out and the parameter directions in its
    /// style map (`attribute.filter_by_visibility`: `,privateattribute,packageattribute`).
    fn member_filters(&self, p: ObjId, style: &mut Style) {
        let off = |field: &str| self.r.g.bool_field(p, field) == Some(false);
        style.hide_member_visibility = off("visibilityKindVisibility");
        style.hide_attribute_types = off("attributeTypeVisibility");
        style.hide_initial_values = off("attributeInitValueVisibility");
        style.hide_attribute_stereotypes = off("attributeStereotypeVisibility");
        style.hide_operation_stereotypes = off("operationStereotypeVisibility");
        style.hide_parameters = off("parameterVisibility");
        style.hide_parameter_types = off("parameterTypeVisibility");
        style.hide_return_types = off("returnTypeVisibility");
        // Astah shows no directions unless told to, and then `in` as well.
        style.parameter_directions =
            match self.r.style(p, "operation.parameter.directionkind.visibility").as_deref() {
                Some("true") => ParameterDirections::All,
                _ => ParameterDirections::Hidden,
            };
        let hidden = |key: &str, suffix: &str| -> Vec<Visibility> {
            let filter = self.r.style(p, key).unwrap_or_default();
            [
                ("public", Visibility::Public),
                ("protected", Visibility::Protected),
                ("package", Visibility::Package),
                ("private", Visibility::Private),
            ]
            .into_iter()
            .filter(|(name, _)| filter.split(',').any(|f| f == format!("{name}{suffix}")))
            .map(|(_, v)| v)
            .collect()
        };
        style.hidden_attributes = hidden("attribute.filter_by_visibility", "attribute");
        style.hidden_operations = hidden("operation.filter_by_visibility", "operation");
    }

    /// A text or line color other than Astah's black (`font.color`, `line.color`).
    pub(super) fn ink(&self, p: ObjId, key: &str) -> Option<Color> {
        let color = self.r.style(p, key).filter(|c| !c.eq_ignore_ascii_case("#000000"))?;
        color.to_ascii_lowercase().parse().ok()
    }

    /// Recreates the diagram's nodes and edges.
    pub(super) fn presentations(&mut self, diagram: Id, kind: DiagramKind, object: ObjId) {
        if kind == DiagramKind::Sequence {
            self.sequence_presentations(diagram, object);
            return;
        }
        let sources = self.presentations_back_to_front(object);
        let (edges, nodes): (Vec<ObjId>, Vec<ObjId>) = sources.into_iter().partition(|&p| {
            self.r.obj(p, "allPoints").is_some() || self.r.class(p) == "NoteAnchorPresentation"
        });

        // Texts that name a system boundary are part of it.
        let names: HashSet<ObjId> = if kind == DiagramKind::UseCase {
            nodes
                .iter()
                .filter(|&&p| self.r.class(p) == "RectPresentation")
                .filter_map(|&p| self.boundary_text(p).map(|(t, _)| t))
                .collect()
        } else {
            HashSet::new()
        };
        // Nodes, and free texts and lines (without node data) among them in Astah's z-order.
        let mut placed: Vec<(ObjId, Option<NodeData>)> = Vec::new();
        for p in nodes {
            if matches!(self.r.class(p), "TextPresentation" | "LinePresentation") {
                if !names.contains(&p) {
                    placed.push((p, None));
                }
            } else if let Some((p, node)) = self.node(diagram, kind, p) {
                placed.push((p, Some(node)));
            }
        }
        let min = placed
            .iter()
            .filter_map(|(p, n)| n.as_ref().map(|n| n.bounds).or_else(|| self.rect(*p)))
            .fold((f32::MAX, f32::MAX), |(x, y), b| (x.min(b.x), y.min(b.y)));
        let shift = Point::new((MARGIN - min.0).max(0.0), (MARGIN - min.1).max(0.0));

        let mut presentation_of = HashMap::new();
        for (source, node) in placed {
            let Some(mut node) = node else {
                if let Some(presentation) = self.free_shape(source, shift) {
                    self.insert(diagram, presentation, false);
                }
                continue;
            };
            node.bounds.x += shift.x;
            node.bounds.y += shift.y;
            // Containers, fragments and structured classes go to the back, as when they are
            // drawn in UmlCraft.
            let container = node.element.and_then(|e| self.model.get(e)).is_some_and(|e| {
                palette::is_container(e.kind())
                    || matches!(e.kind(), ElementKind::CombinedFragment(_))
                    || kind == DiagramKind::CompositeStructure && composite::is_structured(e.kind())
            });
            let presentation = Presentation {
                id: Id::generate(),
                element: node.element,
                shape: Shape::Node { bounds: node.bounds, regions: node.regions },
                style: node.style,
            };
            let index = if container {
                0
            } else {
                self.model.view(diagram).map_or(0, |v| v.presentations().len())
            };
            let id = presentation.id;
            if self.model.apply(Op::InsertPresentation { diagram, index, presentation }).is_ok() {
                presentation_of.insert(source, id);
            }
        }
        // Note anchors come last: they may end on another line.
        let (anchors, lines): (Vec<ObjId>, Vec<ObjId>) =
            edges.into_iter().partition(|&p| self.r.class(p) == "NoteAnchorPresentation");
        for p in lines.into_iter().chain(anchors) {
            if let Some(id) = self.edge(diagram, p, &presentation_of, shift) {
                presentation_of.insert(p, id);
            }
        }
    }

    pub(super) fn node(
        &mut self,
        diagram: Id,
        kind: DiagramKind,
        p: ObjId,
    ) -> Option<(ObjId, NodeData)> {
        let r = self.r;
        let class = r.class(p);
        let element = match class {
            // The frame of a diagram other than a sequence diagram, when shown, is a node
            // presentation of the diagram.
            "FramePresentation"
                if kind != DiagramKind::Sequence
                    && r.g.bool_field(p, "visibility") == Some(true) =>
            {
                diagram
            }
            // Header cells belong to Astah's tables and point at the diagram too.
            "FramePresentation" | "LabelPresentation" | "HeaderCellPresentation" => return None,
            "NotePresentation" => self.note(diagram, p)?,
            "RectPresentation" if kind == DiagramKind::UseCase => {
                self.system_boundary(diagram, p)?
            }
            _ => {
                let model = r.obj(p, "model")?;
                match self.ids.get(&model) {
                    Some(&id) => id,
                    None => return None,
                }
            }
        };
        let element_kind = self.model.get(element)?.kind().clone();
        if element != diagram && !palette::can_show(kind, &element_kind) {
            return None;
        }
        let r = self.r;
        let origin = r.obj(p, "location").and_then(|l| r.point(l)).unwrap_or_default();
        let default = palette::default_size_for(&element_kind);
        let width = r.num(p, "width").filter(|w| *w > 0.0).map_or(default.width, |w| w as f32);
        let height = r.num(p, "height").filter(|h| *h > 0.0).map_or(default.height, |h| h as f32);
        let mut style = self.fill(p);
        if class == "ClassifierPresentation" {
            style.hide_attributes = r.g.bool_field(p, "allAttributeVisibility") == Some(false);
            style.hide_operations = r.g.bool_field(p, "allOperationVisibility") == Some(false);
            // Astah's notation 1 is the circle, 3 the socket of a required interface.
            let notation = r.g.field(p, "notationType").and_then(Value::as_i64);
            style.iconic = matches!(element_kind, ElementKind::Interface(_))
                && matches!(notation, Some(1 | 3));
            style.socket = style.iconic && notation == Some(3);
            // Structured classes hold their parts; they are never sized by their name.
            style.auto_size = scene::is_boxed_classifier(&element_kind)
                && r.g.bool_field(p, "doAutoResize") == Some(true)
                && !(kind == DiagramKind::CompositeStructure
                    && composite::is_structured(&element_kind));
            self.member_filters(p, &mut style);
        }
        if class == "RectPresentation" {
            style.name_offset = self.boundary_name(p).map(|(_, at)| at);
        }
        // A port's name stands where Astah keeps its label: the text starts at the label's
        // left edge, its baseline 13 below the label's top.
        if class == "PortPresentation" {
            style.name_offset = r.obj(p, "namePresentation").and_then(|label| {
                let at = r.obj(label, "location").and_then(|l| r.point(l))?;
                let by = r.obj(label, "localMovement").and_then(|l| r.point(l)).unwrap_or_default();
                let top = at.y + by.y + 13.0 - umlcraft_notation::draw::baseline(scene::FONT_SIZE);
                Some(Point::new(at.x + by.x - origin.x, top - origin.y))
            });
        }
        // Astah never breaks a use case's name at spaces; it widens the ellipse instead.
        if class == "UseCasePresentation" {
            style.auto_size = true;
        }
        // Instances may hide their name or classifier, objects their slots.
        if matches!(
            class,
            "ObjectClassPresentation"
                | "ComponentInstancePresentation"
                | "NodeInstancePresentation"
        ) {
            let off = |field: &str| r.g.bool_field(p, field) == Some(false);
            style.hide_instance_name = off("instanceNameVisibility");
            style.hide_instance_type = off("instanceTypeVisibility");
            style.hide_attributes =
                class == "ObjectClassPresentation" && off("allAttributeVisibility");
            style.hide_empty_slots =
                class == "ObjectClassPresentation" && off("noValueAttributeVisibility");
            style.auto_size = class == "ObjectClassPresentation"
                && r.g.bool_field(p, "doAutoResize") == Some(true);
        }
        // Orthogonal states keep where the lines between their regions go, as distances from
        // their top edge (regions one below the other) or left edge (side by side).
        let has_regions =
            self.model.children(element).iter().any(|&c| {
                self.model.get(c).is_some_and(|c| matches!(c.kind(), ElementKind::Region))
            });
        let regions = has_regions.then(|| {
            let (stacked, side) =
                (r.numbers(p, "horizontalRegionOffsets"), r.numbers(p, "verticalRegionOffsets"));
            let vertical = stacked.is_empty() && !side.is_empty();
            let offsets = if vertical { side } else { stacked };
            Regions { vertical, separators: offsets.into_iter().map(|o| o as f32).collect() }
        });
        Some((
            p,
            NodeData {
                element: Some(element),
                bounds: Rect::new(origin.x, origin.y, width, height),
                style,
                regions,
            },
        ))
    }

    /// The comment shown by a note, created on first use next to the diagram.
    pub(super) fn note(&mut self, diagram: Id, p: ObjId) -> Option<Id> {
        let comment = self.r.obj(p, "model")?;
        if let Some(&id) = self.ids.get(&comment) {
            return Some(id);
        }
        // Astah lists the package a note belongs to among its annotated elements. Packages that
        // a note really annotates are connected by an anchor, which `edge` adds.
        let (owners, annotated): (Vec<Id>, Vec<Id>) = self
            .r
            .list(comment, "annotatedElement")
            .iter()
            .filter_map(|a| self.ids.get(a).copied())
            .partition(|&id| {
                matches!(
                    self.model.get(id).map(|e| e.kind()),
                    Some(ElementKind::Package | ElementKind::Model)
                )
            });
        let body = self.r.text(comment, "body");
        let data = self.data(comment, ElementKind::Comment(CommentProps { body, annotated }), &[]);
        let owner = match owners.first() {
            Some(&package) => package,
            None => self.model.get(diagram)?.owner()?,
        };
        self.create(owner, data, Some(comment))
    }

    /// Astah has no system boundary: on a use case diagram it is a rectangle, and a text at its
    /// top edge gives the name.
    pub(super) fn system_boundary(&mut self, diagram: Id, p: ObjId) -> Option<Id> {
        let name = self.boundary_name(p).map(|(text, _)| text).unwrap_or_default();
        let owner = self.model.get(diagram)?.owner()?;
        self.create(owner, ElementData::new(name, ElementKind::SystemBoundary), None)
    }

    /// The text that names the rectangle `p`, and where its box lies relative to the
    /// rectangle's top left: the topmost text over the rectangle that starts at most 40 above
    /// or below its top.
    pub(super) fn boundary_name(&self, p: ObjId) -> Option<(String, Point)> {
        let (t, at) = self.boundary_text(p)?;
        Some((self.r.text(t, "text"), at))
    }

    /// The text presentation that names the rectangle `p`, see [`Self::boundary_name`].
    pub(super) fn boundary_text(&self, p: ObjId) -> Option<(ObjId, Point)> {
        const NAME_ZONE: f32 = 40.0;
        let rect = self.rect(p)?;
        let texts = self.r.obj(p, "diagram").map(|d| self.r.list(d, "presentation"));
        texts
            .unwrap_or_default()
            .into_iter()
            .filter(|&t| self.r.class(t) == "TextPresentation")
            .filter_map(|t| Some((t, self.rect(t)?)))
            .filter(|(_, b)| {
                (rect.x..=rect.right()).contains(&b.center().x) && (b.y - rect.y).abs() < NAME_ZONE
            })
            .min_by(|(_, a), (_, b)| a.y.total_cmp(&b.y))
            .map(|(t, b)| (t, Point::new(b.x - rect.x, b.y - rect.y)))
    }

    /// A free text or line, moved by `shift`. A line runs from its location by its
    /// `localMovement`; `line.type` dashes it, `line.arrow.type` puts arrowheads on it.
    pub(super) fn free_shape(&self, p: ObjId, shift: Point) -> Option<Presentation> {
        let r = self.r;
        match r.class(p) {
            "TextPresentation" => {
                let bounds = self.rect(p)?.translate(shift.x, shift.y);
                let mut presentation = Presentation::text(bounds, r.text(p, "text"));
                presentation.style.font_size = self.fill(p).font_size;
                Some(presentation)
            }
            "LinePresentation" => {
                let from = r.obj(p, "location").and_then(|l| r.point(l))?.offset(shift.x, shift.y);
                let by = r.obj(p, "localMovement").and_then(|l| r.point(l)).unwrap_or_default();
                let mut presentation = Presentation::line(from, from.offset(by.x, by.y));
                if let Shape::Line { dashed, start_arrow, end_arrow, .. } = &mut presentation.shape
                {
                    let line = r.style(p, "line.type").unwrap_or_default();
                    *dashed = line.starts_with("dash") || line.starts_with("dot");
                    let arrows = r.style(p, "line.arrow.type").unwrap_or_default();
                    *start_arrow = arrows.contains("start") || arrows.contains("both");
                    *end_arrow = arrows.contains("end") || arrows.contains("both");
                }
                Some(presentation)
            }
            _ => None,
        }
    }

    /// Adds the edge presentation `p`, between presentations added before; returns its id.
    pub(super) fn edge(
        &mut self,
        diagram: Id,
        p: ObjId,
        presentation_of: &HashMap<ObjId, Id>,
        shift: Point,
    ) -> Option<Id> {
        let r = self.r;
        let ends: Vec<Id> =
            r.list(p, "servers").iter().filter_map(|s| presentation_of.get(s).copied()).collect();
        let [mut source, mut target] = ends[..] else { return None };
        // Astah's "source" end and the A labels belong to the first server.
        let mut swapped = false;
        let element_of = |presentation: Id| {
            self.model.view(diagram).and_then(|v| v.get(presentation)).and_then(|p| p.element)
        };
        let element = if r.class(p) == "NoteAnchorPresentation" {
            // Anchors run from the note to the annotated element.
            let is_note = |id: Id| {
                element_of(id)
                    .and_then(|e| self.model.get(e))
                    .is_some_and(|e| matches!(e.kind(), ElementKind::Comment(_)))
            };
            if !is_note(source) && is_note(target) {
                std::mem::swap(&mut source, &mut target);
                swapped = true;
            }
            if let (Some(comment), Some(annotated)) = (element_of(source), element_of(target)) {
                self.annotate(comment, annotated);
            }
            None
        } else if r.class(p) == "ContainmentPresentation" {
            // A containment line has no model of its own; it stands for the nested element and
            // runs from its owner.
            let (a, b) = (element_of(source)?, element_of(target)?);
            let owner = |e: Id| self.model.get(e).and_then(|e| e.owner());
            if owner(a) == Some(b) {
                std::mem::swap(&mut source, &mut target);
                swapped = true;
                Some(a)
            } else {
                (owner(b) == Some(a)).then_some(b)?;
                Some(b)
            }
        } else {
            let &element = r.obj(p, "model").and_then(|m| self.ids.get(&m))?;
            // Message edges are added from the model, see `message_edges`.
            if matches!(self.model.get(element).map(|e| e.kind()), Some(ElementKind::Message(_))) {
                return None;
            }
            // A line from an element to itself keeps Astah's order of servers.
            if let Some((from, to)) = self.model.get(element).and_then(|e| edge_ends(e.kind()))
                && from != to
                && element_of(source) == Some(to)
                && element_of(target) == Some(from)
            {
                std::mem::swap(&mut source, &mut target);
                swapped = true;
            }
            Some(element)
        };
        let mut waypoints: Vec<Point> =
            self.r.points(p, "points").into_iter().map(|pt| pt.offset(shift.x, shift.y)).collect();
        // Astah lists the waypoints from its first server to the second.
        if swapped {
            waypoints.reverse();
        }
        let mut presentation = Presentation::edge(element, source, target, waypoints.clone());
        presentation.style.line_shape = match self.r.style(p, "line.shape").as_deref() {
            Some("line_right_angle") => LineShape::RightAngle,
            Some("curve") => LineShape::Curve,
            Some("curve_right_angle") => LineShape::CurveRightAngle,
            _ => LineShape::Line,
        };
        // Astah draws the line in its color and the name in the font's.
        presentation.style.stroke = self.ink(p, "line.color");
        presentation.style.font_color = self.ink(p, "font.color");
        let curve = presentation.style.line_shape.is_curve();
        let bounds = |id: Id| self.model.view(diagram)?.get(id)?.bounds();
        // Lines end at the node's ellipse or diamond, as the scene draws them.
        let outline = |id: Id| {
            let p = self.model.view(diagram).and_then(|v| v.get(id));
            p.map_or(Outline::Rect, |p| scene::outline_of(&self.model, p))
        };
        if let (Some(source_rect), Some(target_rect)) = (bounds(source), bounds(target)) {
            let geometry = EdgeGeometry {
                source_rect,
                target_rect,
                outlines: [outline(source), outline(target)],
                waypoints,
                shift,
                swapped,
                curve,
            };
            if let Shape::Edge { source_anchor, target_anchor, labels, .. } =
                &mut presentation.shape
            {
                (*source_anchor, *target_anchor, *labels) = self.edge_layout(p, &geometry);
            }
        }
        let index = self.model.view(diagram).map_or(0, |v| v.presentations().len());
        // An edge that does not fit its ends is left out, like a presentation of a skipped
        // element.
        let id = presentation.id;
        if self.model.apply(Op::InsertPresentation { diagram, index, presentation }).is_err() {
            self.lost_presentations += 1;
            return None;
        }
        Some(id)
    }

    /// Where Astah's line aims inside its nodes, and where its labels sit. Labels are stored
    /// relative to the point they belong to, as the scene computes it: the end of the line for
    /// roles and multiplicities, the middle of the middle segment for the name.
    pub(super) fn edge_layout(
        &self,
        p: ObjId,
        g: &EdgeGeometry,
    ) -> (Option<Point>, Option<Point>, EdgeLabels) {
        let r = self.r;
        // The center is where UmlCraft aims anyway; only other anchors are worth keeping.
        let anchor = |x, y| {
            Some(Point::new(r.num(p, x)? as f32, r.num(p, y)? as f32))
                .filter(|a| (a.x - 0.5).abs() > 1e-4 || (a.y - 0.5).abs() > 1e-4)
        };
        let (mut source_anchor, mut target_anchor) =
            (anchor("sourceEndX", "sourceEndY"), anchor("targetEndX", "targetEndY"));
        if g.swapped {
            std::mem::swap(&mut source_anchor, &mut target_anchor);
        }
        let inside = |rect: Rect, a: Option<Point>| match a {
            Some(a) => Point::new(rect.x + a.x * rect.width, rect.y + a.y * rect.height),
            None => rect.center(),
        };
        let (from, to) =
            (inside(g.source_rect, source_anchor), inside(g.target_rect, target_anchor));
        let [source_outline, target_outline] = g.outlines;
        let start = source_outline.border_point(
            g.source_rect,
            from,
            g.waypoints.first().copied().unwrap_or(to),
        );
        let end = target_outline.border_point(
            g.target_rect,
            to,
            g.waypoints.last().copied().unwrap_or(from),
        );
        let mut line = vec![start];
        line.extend(&g.waypoints);
        line.push(end);
        let (a, b) = route::middle_segment(&line, g.curve);
        let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);

        // The top left corner of a label presentation, relative to `origin`.
        let place = |label: ObjId, origin: Point| {
            let at = r.obj(label, "location").and_then(|l| r.point(l))?;
            let moved = r.obj(label, "localMovement").and_then(|l| r.point(l)).unwrap_or_default();
            Some(Point::new(
                at.x + moved.x + g.shift.x - origin.x,
                at.y + moved.y + g.shift.y - origin.y,
            ))
        };
        let label = |field: &str, origin: Point| place(r.obj(p, field)?, origin);
        let (a_end, b_end) = if g.swapped { (end, start) } else { (start, end) };
        let (role_a, mult_a) =
            (label("roleAPresentation", a_end), label("multiplicityAPresentation", a_end));
        let (role_b, mult_b) =
            (label("roleBPresentation", b_end), label("multiplicityBPresentation", b_end));
        // «include» and «extend» are the stereotype label of the line.
        let keyword = r.list(p, "stereotypePresentations").first().and_then(|&k| place(k, middle));
        // A binding's substitutions (`<T->int>`) take the place of a name.
        let binding = r.class(p) == "TemplateBindingPresentation";
        let own_keyword = matches!(
            r.class(p),
            "IncludePresentation" | "ExtendPresentation" | "TemplateBindingPresentation"
        );
        let mut name =
            label(if binding { "boundInfoPresentation" } else { "namePresentation" }, middle);
        // Other lines show their stereotypes with the name; without a name they stand where
        // Astah keeps the stereotype label.
        let unnamed = r.obj(p, "model").is_none_or(|m| r.name(m).is_empty());
        if !own_keyword && unnamed && keyword.is_some() {
            name = keyword;
        }
        let mut labels =
            EdgeLabels { name, keyword: keyword.filter(|_| own_keyword), ..Default::default() };
        (
            labels.source_role,
            labels.source_multiplicity,
            labels.target_role,
            labels.target_multiplicity,
        ) = if g.swapped {
            (role_b, mult_b, role_a, mult_a)
        } else {
            (role_a, mult_a, role_b, mult_b)
        };
        (source_anchor, target_anchor, labels)
    }

    pub(super) fn annotate(&mut self, comment: Id, annotated: Id) {
        let Some(e) = self.model.get(comment) else { return };
        let mut data = e.data().clone();
        if let ElementKind::Comment(props) = &mut data.kind
            && !props.annotated.contains(&annotated)
        {
            props.annotated.push(annotated);
            let _ = self.model.apply(Op::UpdateElement { id: comment, data });
        }
    }
}

/// An edge's ends and waypoints in UmlCraft coordinates, and whether its ends are in the
/// opposite order of Astah's.
pub(super) struct EdgeGeometry {
    pub(super) source_rect: Rect,
    pub(super) target_rect: Rect,
    /// Where the line ends at each node, in UmlCraft's order.
    pub(super) outlines: [Outline; 2],
    pub(super) waypoints: Vec<Point>,
    pub(super) shift: Point,
    pub(super) swapped: bool,
    pub(super) curve: bool,
}

pub(super) struct NodeData {
    pub(super) element: Option<Id>,
    pub(super) bounds: Rect,
    pub(super) style: Style,
    pub(super) regions: Option<Regions>,
}

/// The elements an edge of this relationship runs from and to, as UmlCraft draws it.
pub(super) fn edge_ends(kind: &ElementKind) -> Option<(Id, Id)> {
    match kind {
        ElementKind::Association(p) => Some((p.ends[0].participant, p.ends[1].participant)),
        ElementKind::Link(p) => Some((p.ends[0].instance, p.ends[1].instance)),
        ElementKind::Generalization(p) => Some((p.specific, p.general)),
        ElementKind::Realization(p) | ElementKind::Dependency(p) => Some((p.client, p.supplier)),
        ElementKind::Include(p) => Some((p.including, p.addition)),
        ElementKind::Extend(p) => Some((p.extension, p.extended)),
        ElementKind::Message(p) => Some((p.from, p.to)),
        ElementKind::ActivityEdge(p) => Some((p.source, p.target)),
        _ => None,
    }
}
