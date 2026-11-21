//! The tools that read the project and draw diagrams.

use super::*;

pub(super) fn get_document(host: &mut dyn Host, access: &Access) -> Outcome {
    let display = display_json(host.display());
    let model = host.document().model();
    let mut diagrams = Vec::new();
    let mut structograms = Vec::new();
    let mut stack = vec![model.root()];
    while let Some(id) = stack.pop() {
        if let Some(p) = model.get(id).and_then(|e| e.kind().as_structogram()) {
            structograms.push(json!({
                "id": id,
                "title": p.structogram.text().join(" "),
                "owner": model.get(id).and_then(|e| e.owner()),
                "operation": p.operation,
            }));
        }
        if model.get(id).and_then(|e| e.kind().as_diagram()).is_some() {
            let owner = model.get(id).and_then(|e| e.owner());
            diagrams.push(json!({
                "id": id,
                "name": model.get(id).map_or("", |e| e.name()),
                "kind": diagram_key_of(model, id),
                "owner": owner,
            }));
        }
        stack.extend(model.children(id).iter().rev());
    }
    let root = model.root();
    let mut reply = json!({
        "project": model.get(root).map_or("", |e| e.name()),
        "root": root,
        "file": host.path(),
        "unsaved_changes": host.document().is_dirty(),
        "live": host.live(),
        "can_undo": host.document().can_undo(),
        "can_redo": host.document().can_redo(),
        "diagrams": diagrams,
        "structograms": structograms,
        "shown_diagram": host.active_diagram(),
        "selection": host.selection(),
        "allowed_folders": if access.any {
            json!("any")
        } else {
            json!({ "read": access.read, "write": access.write })
        },
        "display": display,
    });
    // The user may be looking at an app this server cannot reach: say so rather than work on
    // a project of its own unnoticed.
    if !host.live() && crate::live::sandboxed_app_announced() {
        reply["hint"] = json!(
            "A UmlCraft Flatpak is running, but this server cannot reach it: that Flatpak \
             lacks the permissions the current one has (network and xdg-run/umlcraft); update \
             it. Until then changes go to a project of the server's own."
        );
    }
    Ok(Reply::json(reply))
}

pub(super) fn list_elements(host: &mut dyn Host, args: &Value) -> Outcome {
    let model = host.document().model();
    let under = opt_id(args, "under")?.unwrap_or(model.root());
    if !model.contains(under) {
        return Err(format!("no element {under}"));
    }
    let depth = opt_num(args, "depth").map_or(usize::MAX, |d| d.max(1.0) as usize);
    fn tree(model: &Model, id: Id, depth: usize) -> Value {
        let mut node = summary(model, id);
        let children = model.children(id);
        if !children.is_empty() {
            node["children"] = if depth > 1 {
                children.iter().map(|&c| tree(model, c, depth - 1)).collect()
            } else {
                json!(children.len())
            };
        }
        node
    }
    Ok(Reply::json(tree(model, under, depth.saturating_add(1))))
}

pub(super) fn get_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let model = host.document().model();
    let id = id_arg(args, "id")?;
    let element = model.get(id).ok_or_else(|| format!("no element {id}"))?;
    let mut shown_on = Vec::new();
    for (diagram, view) in model.views() {
        for p in view.presentations_of(id) {
            shown_on.push(json!({ "diagram": diagram, "presentation": p.id }));
        }
    }
    let mut value = json!({
        "id": id,
        "kind": element.kind().type_name(),
        "name": element.name(),
        "owner": element.owner(),
        "properties": serde_json::to_value(element.data()).unwrap_or(Value::Null),
        "children": model.children(id).iter().map(|&c| {
            let mut child = summary(model, c);
            if matches!(
                model.get(c).map(|e| e.kind()),
                Some(ElementKind::Attribute(_) | ElementKind::Operation(_))
            ) {
                child["text"] = json!(member::member_text(model, c));
            }
            child
        }).collect::<Vec<_>>(),
        "shown_on": shown_on,
    });
    if matches!(element.kind(), ElementKind::Attribute(_) | ElementKind::Operation(_)) {
        value["text"] = json!(member::member_text(model, id));
    }
    Ok(Reply::json(value))
}

pub(super) fn get_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let (doc, measure) = host.parts();
    let model = doc.model();
    let scene = scene::build(model, diagram, &Preview::default(), measure);
    let view = model.view(diagram).ok_or("no view")?;
    let rect = |r: Rect| json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height });
    let mut nodes = Vec::new();
    let mut lines = Vec::new();
    let mut bars = Vec::new();
    for p in view.presentations() {
        // Activation bars, so that a client can send a message from inside one (at its `y`).
        if let Shape::Activation { lifeline, top, height, .. } = p.shape {
            let element = view.get(lifeline).and_then(|l| l.element);
            bars.push(json!({
                "presentation": p.id,
                "lifeline": element,
                "top": top,
                "bottom": top + height,
            }));
            continue;
        }
        let kind = match (p.element, &p.shape) {
            (Some(e), _) => kind_name(model, e),
            (None, Shape::Text { .. }) => "text",
            (None, Shape::Line { .. }) => "line",
            (None, Shape::Edge { .. }) => "noteAnchor",
            (None, _) => "",
        };
        let name = p.element.map(|e| member::display_name(model, e)).unwrap_or_default();
        if let Some((source, target)) = p.endpoints() {
            let mut line = json!({
                "presentation": p.id,
                "element": p.element,
                "kind": kind,
                "name": name,
                "source": source,
                "target": target,
            });
            if let Shape::Message { y, .. } = p.shape {
                line["y"] = json!(y);
            }
            line::describe(p, &mut line);
            // A transition's label as drawn: `trigger [guard] / effect`.
            if let Some(ElementKind::Transition(t)) =
                p.element.and_then(|e| model.get(e)).map(|e| e.kind())
            {
                let label = [name.as_str(), &t.label()]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                line["label"] = json!(label);
            }
            lines.push(line);
        } else if let Some(bounds) = scene.node(p.id).map(|n| n.rect).or(p.bounds()) {
            let mut node = json!({
                "presentation": p.id,
                "element": p.element,
                "kind": kind,
                "name": name,
                "bounds": rect(bounds),
            });
            // Where each operand lies, for moving messages into it.
            if let (Shape::Fragment { separators, .. }, Some(fragment)) = (&p.shape, p.element) {
                let operands = model.children(fragment);
                let tops = std::iter::once(bounds.y).chain(separators.iter().map(|s| bounds.y + s));
                let bottoms = separators.iter().map(|s| bounds.y + s).chain([bounds.bottom()]);
                let operands: Vec<Value> = operands
                    .iter()
                    .zip(tops.zip(bottoms))
                    .map(|(o, (top, bottom))| json!({ "id": o, "top": top, "bottom": bottom }))
                    .collect();
                node["operands"] = json!(operands);
            }
            // Where each region of a state lies, for putting substates into it.
            let state = p.element.and_then(|e| model.get(e));
            let regions = match state.map(|e| e.kind()) {
                Some(ElementKind::State(_)) => scene::state_regions(model, p),
                _ => Vec::new(),
            };
            if !regions.is_empty() {
                let regions: Vec<Value> = regions
                    .iter()
                    .map(|r| json!({ "id": r.region, "bounds": rect(r.rect) }))
                    .collect();
                node["regions"] = json!(regions);
            }
            // What a state invariant, termination or constraint of a sequence diagram is on.
            match p.element.and_then(|e| model.get(e)).map(|e| e.kind()) {
                Some(
                    ElementKind::StateInvariant(StateInvariantProps { lifeline })
                    | ElementKind::Termination(TerminationProps { lifeline }),
                ) => node["lifeline"] = json!(lifeline),
                Some(ElementKind::DurationConstraint(c)) => {
                    node["messages"] = json!([c.from, c.to])
                }
                Some(ElementKind::TimeConstraint(c)) => node["messages"] = json!([c.message]),
                // What a note says, and the extension points a use case lists.
                Some(ElementKind::Comment(c)) => node["text"] = json!(c.body),
                Some(ElementKind::UseCase(_)) => {
                    node["extension_points"] = json!(extension_points(model, p.element))
                }
                _ => {}
            }
            nodes.push(node);
        }
    }
    let tools = tools_of(model, diagram, kind);
    let keys = |node: bool| -> Vec<&str> {
        tools
            .iter()
            .filter(|t| {
                matches!(t, Tool::Node(_)) == node && matches!(t, Tool::Node(_) | Tool::Edge(_))
            })
            .map(|t| t.key())
            .collect()
    };
    let mut reply = json!({
        "id": diagram,
        "name": model.get(diagram).map_or("", |e| e.name()),
        "kind": diagram_key_of(model, diagram),
        "nodes": nodes,
        "lines": lines,
        "element_tools": keys(true),
        "relationship_tools": keys(false),
    });
    if kind == DiagramKind::Sequence {
        reply["bars"] = json!(bars);
    }
    Ok(Reply::json(reply))
}

/// Fonts for rasterizing, loaded once.
pub(super) fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

/// The most pixels a rasterized diagram gets (160 MB of pixmap). A larger diagram comes out
/// at a smaller scale, so a far-flung node cannot make one call allocate gigabytes.
pub(super) const MAX_PIXELS: f64 = 40_000_000.0;

/// The SVG export rasterized at `scale` pixels per logical unit, or at less if that would
/// exceed `MAX_PIXELS`.
pub(super) fn png(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let options = usvg::Options { fontdb: fonts(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let (width, height) = (f64::from(tree.size().width()), f64::from(tree.size().height()));
    let mut scale = f64::from(scale / svg::SCALE);
    let pixels = width * height * scale * scale;
    if pixels > MAX_PIXELS {
        scale *= (MAX_PIXELS / pixels).sqrt();
    }
    let side = |units: f64| (units * scale).ceil();
    let (w, h) = (side(width), side(height));
    if w < 1.0 || h < 1.0 {
        return Err(if width * height > 0.0 {
            "the diagram is too large to show as an image; export it as SVG".into()
        } else {
            "the diagram is empty".into()
        });
    }
    // Both sides are at most about MAX_PIXELS here, well inside u32.
    let mut pixmap = tiny_skia::Pixmap::new(w as u32, h as u32).ok_or("the image is too large")?;
    let scale = scale as f32;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

pub(super) fn diagram_svg(host: &mut dyn Host, diagram: Id) -> String {
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
    svg::render(&scene, measure)
}

/// A structogram as SVG, as the app shows and exports it.
pub(super) fn structogram_svg(d: &umlcraft_nsd::Diagram) -> String {
    let m = umlcraft_nsd::Metrics::for_platform();
    let layout = umlcraft_nsd::layout::Layout::of(d, m);
    umlcraft_nsd::svg::render_with(d, &layout, m, &umlcraft_nsd::svg::Palette::PLAIN)
}

pub(super) fn set_display(host: &mut dyn Host, args: &Value) -> Outcome {
    let mut display = host.display();
    for (key, setting) in [
        ("visibility_column", &mut display.visibility_column),
        ("abstract_stereotype", &mut display.abstract_stereotype),
        ("read_only_constraint", &mut display.read_only_constraint),
        ("guillemets", &mut display.guillemets),
    ] {
        if let Some(value) = args.get(key) {
            *setting = value.as_bool().ok_or_else(|| format!("`{key}` is true or false"))?;
        }
    }
    if display != host.display() {
        host.set_display(display);
    }
    Ok(Reply::json(display_json(display)))
}

pub(super) fn display_json(display: Display) -> Value {
    json!({
        "visibility_column": display.visibility_column,
        "abstract_stereotype": display.abstract_stereotype,
        "read_only_constraint": display.read_only_constraint,
        "guillemets": display.guillemets,
    })
}

pub(super) fn render_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let scale = opt_num(args, "scale").unwrap_or(1.0).clamp(0.5, 2.0);
    let shown =
        opt_diagram(host.document().model(), args, "diagram", true)?.or(host.active_diagram());
    let model = host.document().model();
    if let Some(p) = shown.and_then(|id| model.get(id)).and_then(|e| e.kind().as_structogram()) {
        let svg = structogram_svg(&p.structogram);
        // Structogram images are in pixels already.
        let image = png(&svg, scale * svg::SCALE)?;
        return Ok(Reply { content: vec![Content::Image(image)], is_error: false });
    }
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let image = png(&diagram_svg(host, diagram), scale)?;
    Ok(Reply { content: vec![Content::Image(image)], is_error: false })
}
