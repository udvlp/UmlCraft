//! The MCP tools: their descriptions and input schemas ([`list`]), and running one against a
//! [`Host`] ([`call`]). Ids are the model's UUIDs as strings. Every tool that changes the
//! project is one undo step, made as the app makes its edits.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use umlcraft_model::{
    DiagramKind, ElementData, ElementKind, Id, Model, ModelError, Point, Rect, Shape, Tx,
    Visibility,
};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Preview};
use umlcraft_notation::{autoroute, edit, layout, member, sequence_edit, svg};

use crate::base64;
use crate::host::{Access, Host, first_diagram, new_document};

/// Part of a tool's reply.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Text(String),
    /// A PNG image.
    Image(Vec<u8>),
}

/// What a tool returns: MCP's `CallToolResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl Reply {
    pub fn text(text: impl Into<String>) -> Self {
        Self { content: vec![Content::Text(text.into())], is_error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { content: vec![Content::Text(text.into())], is_error: true }
    }

    fn json(value: Value) -> Self {
        Self::text(value.to_string())
    }

    pub fn to_json(&self) -> Value {
        let content: Vec<Value> = self
            .content
            .iter()
            .map(|c| match c {
                Content::Text(text) => json!({ "type": "text", "text": text }),
                Content::Image(png) => {
                    json!({ "type": "image", "data": base64::encode(png), "mimeType": "image/png" })
                }
            })
            .collect();
        json!({ "content": content, "isError": self.is_error })
    }

    pub fn from_json(value: &Value) -> Self {
        let content = value["content"].as_array().map_or_else(Vec::new, |content| {
            content
                .iter()
                .filter_map(|c| match c["type"].as_str()? {
                    "text" => Some(Content::Text(c["text"].as_str()?.to_owned())),
                    "image" => Some(Content::Image(base64::decode(c["data"].as_str()?)?)),
                    _ => None,
                })
                .collect()
        });
        Self { content, is_error: value["isError"].as_bool().unwrap_or(false) }
    }
}

/// How the server explains itself to the client (MCP `instructions`).
pub const INSTRUCTIONS: &str = "UmlCraft is a UML modeling tool. These tools read and edit \
the open UML project: when the UmlCraft app is running, the project open there (the user sees \
every change and can undo it with Ctrl+Z); otherwise a project of the server's own, which \
`open_file` and `save_file` load and store. A project holds a model tree (packages, classes, \
…) and diagrams showing parts of it. Start with `get_document`. To draw a diagram: \
`create_diagram`, then `create_element` for each box (no position needed), `add_member` for \
attributes and operations (`- name : String`, `+ total() : int`), `connect` for relationships, \
then `arrange_diagram` (class, component, deployment diagrams) and `render_diagram` to look at \
the result. `get_diagram` lists what a diagram shows and which element and relationship tools \
its kind offers. Ids are UUID strings returned by the tools.";

const DIAGRAM_KINDS: [(&str, DiagramKind); 7] = [
    ("class", DiagramKind::Class),
    ("use_case", DiagramKind::UseCase),
    ("sequence", DiagramKind::Sequence),
    ("activity", DiagramKind::Activity),
    ("state_machine", DiagramKind::StateMachine),
    ("component", DiagramKind::Component),
    ("deployment", DiagramKind::Deployment),
];

fn diagram_key(kind: DiagramKind) -> &'static str {
    DIAGRAM_KINDS.iter().find(|(_, k)| *k == kind).map_or("class", |(key, _)| key)
}

/// The tools with their descriptions and JSON schemas, as `tools/list` returns them.
pub fn list() -> Vec<Value> {
    let id = |description: &str| json!({ "type": "string", "description": description });
    let text = |description: &str| json!({ "type": "string", "description": description });
    let number = |description: &str| json!({ "type": "number", "description": description });
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
        })
    };
    let read_only = |mut tool: Value| {
        tool["annotations"] = json!({ "readOnlyHint": true });
        tool
    };
    let kinds: Vec<&str> = DIAGRAM_KINDS.iter().map(|(key, _)| *key).collect();
    vec![
        read_only(tool(
            "get_document",
            "The open project: its file, whether it has unsaved changes, whether it is the one \
             open in the UmlCraft app (live), its diagrams, the diagram shown and the selection, \
             and the folders files may be opened from and saved to.",
            json!({}),
            &[],
        )),
        read_only(tool(
            "list_elements",
            "The model tree, or the part under `under`: id, kind and name of each element.",
            json!({
                "under": id("Element to list the contents of; the whole model if omitted."),
                "depth": number("Levels to list; all if omitted."),
            }),
            &[],
        )),
        read_only(tool(
            "get_element",
            "One element: kind, name, owner, its properties as stored, its members' text, its \
             children and the diagrams that show it.",
            json!({ "id": id("The element.") }),
            &["id"],
        )),
        read_only(tool(
            "get_diagram",
            "What a diagram shows: nodes with their element, kind, name and bounds as drawn; \
             lines with their element and ends; and the tools its kind offers for \
             `create_element` and `connect`.",
            json!({ "diagram": id("The diagram; the one shown if omitted.") }),
            &[],
        )),
        read_only(tool(
            "render_diagram",
            "A diagram as a PNG image, as UmlCraft draws it.",
            json!({
                "diagram": id("The diagram; the one shown if omitted."),
                "scale": number("Pixels per unit, 0.5 to 2; default 1."),
            }),
            &[],
        )),
        tool(
            "create_diagram",
            "Creates a diagram (sequence, activity and state machine diagrams inside a new \
             interaction, activity or state machine of the same name) and shows it.",
            json!({
                "kind": { "type": "string", "enum": kinds },
                "name": text("Its name."),
                "owner": id("Package (or class) to put it in; the model's root if omitted."),
            }),
            &["kind", "name"],
        ),
        tool(
            "create_element",
            "Creates an element with a diagram tool (`class`, `interface`, `actor`, `useCase`, \
             `lifeline`, `action`, `state`, `component`, `node`, `note`, … as `get_diagram` \
             lists them) and shows it on the diagram, at `x`, `y` or right of what is there. \
             It goes into the package that owns the diagram (states into the state drawn \
             around them).",
            json!({
                "diagram": id("The diagram."),
                "tool": text("The diagram tool."),
                "name": text("Its name; a free default name if omitted."),
                "x": number("Left edge."),
                "y": number("Top edge."),
            }),
            &["diagram", "tool"],
        ),
        tool(
            "create_model_element",
            "Creates an element in the model tree only, without showing it, e.g. a class or \
             package under `owner`. `tool` is a diagram tool name as for `create_element`.",
            json!({
                "owner": id("The package or element to put it in."),
                "tool": text("The kind, as a diagram tool name (`class`, `package`, …)."),
                "name": text("Its name."),
            }),
            &["owner", "tool", "name"],
        ),
        tool(
            "show_element",
            "Shows an existing element on a diagram, with the lines to elements the diagram \
             already shows.",
            json!({
                "diagram": id("The diagram."),
                "element": id("The element."),
                "x": number("Left edge."),
                "y": number("Top edge."),
            }),
            &["diagram", "element"],
        ),
        tool(
            "connect",
            "Draws a relationship between two elements shown on a diagram (`association`, \
             `generalization`, `realization`, `dependency`, `include`, `extend`, `flow`, \
             `transition`, `message`, `reply`, `noteAnchor`, … as `get_diagram` lists them); \
             UmlCraft refuses what UML does not allow. `source` and `target` are elements or \
             their presentations. Generalization runs from the specific to the general, \
             realization from the class to the interface. Messages go below the others unless \
             `y` is given.",
            json!({
                "diagram": id("The diagram."),
                "tool": text("The relationship tool."),
                "source": id("Element or presentation it starts at."),
                "target": id("Element or presentation it ends at."),
                "y": number("Sequence diagrams: the height of the message."),
            }),
            &["diagram", "tool", "source", "target"],
        ),
        tool(
            "add_member",
            "Adds an attribute, operation or enumeration literal to a classifier, written as \
             in a class box: `- name : String [0..1] = \"x\"`, `+ total(tax : double) : int`, \
             `RED`. Types name classes of the model or are kept as text.",
            json!({
                "owner": id("The class, interface, enumeration or data type."),
                "text": text("The member as it reads in the box."),
            }),
            &["owner", "text"],
        ),
        tool(
            "update_element",
            "Changes an element: its name, a member's text (as for `add_member`), its \
             documentation or its stereotypes.",
            json!({
                "id": id("The element."),
                "name": text("New name."),
                "text": text("Attributes and operations: the whole member as text."),
                "documentation": text("New documentation."),
                "stereotypes": { "type": "array", "items": { "type": "string" } },
            }),
            &["id"],
        ),
        tool(
            "move_node",
            "Moves a node of a diagram, or gives it another size.",
            json!({
                "diagram": id("The diagram."),
                "node": id("The element or its presentation."),
                "x": number("Left edge."),
                "y": number("Top edge."),
                "width": number("New width."),
                "height": number("New height."),
            }),
            &["diagram", "node", "x", "y"],
        ),
        tool(
            "arrange_diagram",
            "Lays out a class, component or deployment diagram in layers (generals above \
             their specifics, wholes above their parts) and routes its lines; with `nodes` only \
             those, where they are, pushing others aside.",
            json!({
                "diagram": id("The diagram."),
                "direction": { "type": "string", "enum": ["down", "right"] },
                "nodes": { "type": "array", "items": { "type": "string" },
                           "description": "Elements or presentations to arrange; all if omitted." },
            }),
            &["diagram"],
        ),
        tool(
            "route_lines",
            "Routes lines of a diagram at right angles around the nodes; they follow their \
             nodes from then on.",
            json!({
                "diagram": id("The diagram."),
                "lines": { "type": "array", "items": { "type": "string" },
                           "description": "Line presentations or relationships; all if omitted." },
            }),
            &["diagram"],
        ),
        tool(
            "delete_element",
            "Deletes an element from the model, with what it owns and the relationships to it, \
             on every diagram. A presentation without element (note anchor, free text) is \
             removed from its diagram.",
            json!({ "id": id("The element or presentation.") }),
            &["id"],
        ),
        tool(
            "remove_from_diagram",
            "Removes a node or line from a diagram; the element stays in the model.",
            json!({
                "diagram": id("The diagram."),
                "id": id("The element or its presentation."),
            }),
            &["diagram", "id"],
        ),
        tool("undo", "Undoes the last change.", json!({}), &[]),
        tool("redo", "Redoes the last undone change.", json!({}), &[]),
        tool(
            "open_diagram",
            "Shows a diagram (in the app: brings its tab to the front).",
            json!({ "diagram": id("The diagram.") }),
            &["diagram"],
        ),
        tool(
            "new_document",
            "Starts a new project with one class diagram. In the app only when the open \
             project has no unsaved changes.",
            json!({ "name": text("The project's name.") }),
            &[],
        ),
        tool(
            "open_file",
            "Opens a UmlCraft project (.ucraft) from an allowed folder. In the app only when \
             the open project has no unsaved changes.",
            json!({ "path": text("The file.") }),
            &["path"],
        ),
        tool(
            "save_file",
            "Saves the project as .ucraft: to `path` in an allowed folder, or to its own file.",
            json!({ "path": text("Where to save; the project's file if omitted.") }),
            &[],
        ),
        tool(
            "export_image",
            "Writes a diagram as an SVG or PNG image to an allowed folder (by the extension).",
            json!({
                "diagram": id("The diagram."),
                "path": text("The image file, .svg or .png."),
            }),
            &["diagram", "path"],
        ),
    ]
}

type Outcome = Result<Reply, String>;

/// Runs tool `name` with `args` against `host`; files only in the folders `access` allows.
pub fn call(host: &mut dyn Host, access: &Access, name: &str, args: &Value) -> Reply {
    let outcome: Outcome = match name {
        "get_document" => get_document(host, access),
        "list_elements" => list_elements(host, args),
        "get_element" => get_element(host, args),
        "get_diagram" => get_diagram(host, args),
        "render_diagram" => render_diagram(host, args),
        "create_diagram" => create_diagram(host, args),
        "create_element" => create_element(host, args),
        "create_model_element" => create_model_element(host, args),
        "show_element" => show_element(host, args),
        "connect" => connect(host, args),
        "add_member" => add_member(host, args),
        "update_element" => update_element(host, args),
        "move_node" => move_node(host, args),
        "arrange_diagram" => arrange_diagram(host, args),
        "route_lines" => route_lines(host, args),
        "delete_element" => delete_element(host, args),
        "remove_from_diagram" => remove_from_diagram(host, args),
        "undo" => undo(host, true),
        "redo" => undo(host, false),
        "open_diagram" => open_diagram(host, args),
        "new_document" => new_project(host, args),
        "open_file" => open_file(host, access, args),
        "save_file" => save_file(host, access, args),
        "export_image" => export_image(host, access, args),
        _ => Err(format!("unknown tool `{name}`")),
    };
    outcome.unwrap_or_else(Reply::error)
}

// ----- Arguments -----------------------------------------------------------------------------

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(args, key).ok_or_else(|| format!("`{key}` is missing"))
}

fn opt_num(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

fn parse_id(text: &str) -> Result<Id, String> {
    Id::from_str(text.trim()).map_err(|_| format!("`{text}` is not an id"))
}

fn id_arg(args: &Value, key: &str) -> Result<Id, String> {
    parse_id(str_arg(args, key)?)
}

fn opt_id(args: &Value, key: &str) -> Result<Option<Id>, String> {
    opt_str(args, key).map(parse_id).transpose()
}

fn ids_arg(args: &Value, key: &str) -> Result<Option<Vec<Id>>, String> {
    let Some(items) = args.get(key).and_then(Value::as_array) else { return Ok(None) };
    items
        .iter()
        .map(|item| item.as_str().ok_or_else(|| format!("`{key}` holds ids")).and_then(parse_id))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn model_error(err: ModelError) -> String {
    err.to_string()
}

// ----- Looking things up ---------------------------------------------------------------------

fn kind_name(model: &Model, id: Id) -> &'static str {
    model.get(id).map_or("", |e| e.kind().type_name())
}

fn summary(model: &Model, id: Id) -> Value {
    let element = model.get(id);
    json!({
        "id": id,
        "kind": element.map_or("", |e| e.kind().type_name()),
        "name": element.map_or_else(String::new, |_| member::display_name(model, id)),
    })
}

/// `id` if it names a diagram, else an error.
fn diagram_of(model: &Model, id: Id) -> Result<DiagramKind, String> {
    model
        .get(id)
        .and_then(|e| e.kind().as_diagram())
        .filter(|_| model.view(id).is_some())
        .ok_or_else(|| format!("{id} is not a diagram"))
}

/// The diagram given as `key`, else the one shown.
fn diagram_arg(host: &dyn Host, args: &Value, key: &str) -> Result<(Id, DiagramKind), String> {
    let diagram = match opt_id(args, key)? {
        Some(diagram) => diagram,
        None => host.active_diagram().ok_or("no diagram is shown; give `diagram`")?,
    };
    Ok((diagram, diagram_of(host.document().model(), diagram)?))
}

/// The presentation on `diagram` that `id` names: `id` itself, or the node (else the first
/// presentation) showing the element `id`.
fn on_diagram(model: &Model, diagram: Id, id: Id) -> Result<Id, String> {
    let view = model.view(diagram).ok_or_else(|| format!("{diagram} is not a diagram"))?;
    if view.get(id).is_some() {
        return Ok(id);
    }
    let mut shown = view.presentations_of(id);
    let first = shown.next().map(|p| p.id);
    view.presentations_of(id).find(|p| p.bounds().is_some()).map(|p| p.id).or(first).ok_or_else(
        || {
            let what = model.get(id).map_or_else(
                || id.to_string(),
                |e| format!("{} {}", e.kind().type_name(), e.name()),
            );
            format!("{what} is not shown on this diagram; `show_element` puts it there")
        },
    )
}

/// The tool of `kind`'s palette with `key`.
fn diagram_tool(kind: DiagramKind, key: &str) -> Result<Tool, String> {
    let tools = palette::palette(kind);
    tools.iter().copied().find(|t| t.key() == key).ok_or_else(|| {
        let keys: Vec<&str> = tools.iter().map(|t| t.key()).filter(|k| *k != "select").collect();
        format!("a {} diagram has no tool `{key}`; it has: {}", diagram_key(kind), keys.join(", "))
    })
}

fn node_tool(kind: DiagramKind, key: &str) -> Result<NodeTool, String> {
    match diagram_tool(kind, key)? {
        Tool::Node(tool) => Ok(tool),
        _ => Err(format!("`{key}` draws no element; `connect` draws relationships")),
    }
}

fn edge_tool(kind: DiagramKind, key: &str) -> Result<EdgeTool, String> {
    match diagram_tool(kind, key)? {
        Tool::Edge(tool) => Ok(tool),
        _ => Err(format!("`{key}` draws no relationship; `create_element` draws elements")),
    }
}

/// A free place for a new node: right of what the diagram shows, or below it once that is wide.
fn free_origin(model: &Model, diagram: Id) -> Point {
    let snap = |v: f32| (v / 10.0).round() * 10.0;
    match model.view(diagram).and_then(|v| v.content_bounds()) {
        None => Point::new(40.0, 40.0),
        Some(r) if r.right() < 1000.0 => Point::new(snap(r.right() + 60.0), snap(r.y.max(40.0))),
        Some(r) => Point::new(snap(r.x), snap(r.bottom() + 60.0)),
    }
}

fn is_sequence(model: &Model, diagram: Id) -> bool {
    model.get(diagram).and_then(|e| e.kind().as_diagram()) == Some(DiagramKind::Sequence)
}

/// One undo step on the host's document.
fn change<R>(
    host: &mut dyn Host,
    tool: &str,
    f: impl FnOnce(&mut Tx<'_>) -> umlcraft_model::Result<R>,
) -> Result<R, String> {
    let label = host.label(tool);
    let (doc, measure) = host.parts();
    edit::apply(doc, label, measure, f).map_err(model_error)
}

// ----- Reading -------------------------------------------------------------------------------

fn get_document(host: &mut dyn Host, access: &Access) -> Outcome {
    let model = host.document().model();
    let mut diagrams = Vec::new();
    let mut stack = vec![model.root()];
    while let Some(id) = stack.pop() {
        if let Some(kind) = model.get(id).and_then(|e| e.kind().as_diagram()) {
            let owner = model.get(id).and_then(|e| e.owner());
            diagrams.push(json!({
                "id": id,
                "name": model.get(id).map_or("", |e| e.name()),
                "kind": diagram_key(kind),
                "owner": owner,
            }));
        }
        stack.extend(model.children(id).iter().rev());
    }
    let root = model.root();
    Ok(Reply::json(json!({
        "project": model.get(root).map_or("", |e| e.name()),
        "root": root,
        "file": host.path(),
        "unsaved_changes": host.document().is_dirty(),
        "live": host.live(),
        "can_undo": host.document().can_undo(),
        "can_redo": host.document().can_redo(),
        "diagrams": diagrams,
        "shown_diagram": host.active_diagram(),
        "selection": host.selection(),
        "allowed_folders": if access.any { json!("any") } else { json!(access.folders) },
    })))
}

fn list_elements(host: &mut dyn Host, args: &Value) -> Outcome {
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

fn get_element(host: &mut dyn Host, args: &Value) -> Outcome {
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

fn get_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let (doc, measure) = host.parts();
    let model = doc.model();
    let scene = scene::build(model, diagram, &Preview::default(), measure);
    let view = model.view(diagram).ok_or("no view")?;
    let rect = |r: Rect| json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height });
    let mut nodes = Vec::new();
    let mut lines = Vec::new();
    for p in view.presentations() {
        let kind = match (p.element, &p.shape) {
            (_, Shape::Activation { .. }) => continue,
            (Some(e), _) => kind_name(model, e),
            (None, Shape::Text { .. }) => "text",
            (None, Shape::Line { .. }) => "line",
            (None, Shape::Edge { .. }) => "noteAnchor",
            (None, _) => "",
        };
        let name = p.element.map(|e| member::display_name(model, e)).unwrap_or_default();
        if let Some((source, target)) = p.endpoints() {
            lines.push(json!({
                "presentation": p.id,
                "element": p.element,
                "kind": kind,
                "name": name,
                "source": source,
                "target": target,
            }));
        } else if let Some(bounds) = scene.node(p.id).map(|n| n.rect).or(p.bounds()) {
            nodes.push(json!({
                "presentation": p.id,
                "element": p.element,
                "kind": kind,
                "name": name,
                "bounds": rect(bounds),
            }));
        }
    }
    let tools = palette::palette(kind);
    let keys = |node: bool| -> Vec<&str> {
        tools
            .iter()
            .filter(|t| {
                matches!(t, Tool::Node(_)) == node && matches!(t, Tool::Node(_) | Tool::Edge(_))
            })
            .map(|t| t.key())
            .collect()
    };
    Ok(Reply::json(json!({
        "id": diagram,
        "name": model.get(diagram).map_or("", |e| e.name()),
        "kind": diagram_key(kind),
        "nodes": nodes,
        "lines": lines,
        "element_tools": keys(true),
        "relationship_tools": keys(false),
    })))
}

/// Fonts for rasterizing, loaded once.
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = resvg::usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

/// The SVG export rasterized at `scale` pixels per logical unit.
fn png(svg: &str, scale: f32) -> Result<Vec<u8>, String> {
    use resvg::{tiny_skia, usvg};
    let options = usvg::Options { fontdb: fonts(), ..usvg::Options::default() };
    let tree = usvg::Tree::from_str(svg, &options).map_err(|e| e.to_string())?;
    let scale = scale / svg::SCALE;
    let size = tree.size().to_int_size().scale_by(scale).ok_or("the diagram is empty")?;
    let mut pixmap =
        tiny_skia::Pixmap::new(size.width(), size.height()).ok_or("the image is too large")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    pixmap.encode_png().map_err(|e| e.to_string())
}

fn diagram_svg(host: &mut dyn Host, diagram: Id) -> String {
    let (doc, measure) = host.parts();
    let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
    svg::render(&scene, measure)
}

fn render_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let scale = opt_num(args, "scale").unwrap_or(1.0).clamp(0.5, 2.0);
    let image = png(&diagram_svg(host, diagram), scale)?;
    Ok(Reply { content: vec![Content::Image(image)], is_error: false })
}

// ----- Changing ------------------------------------------------------------------------------

fn create_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let key = str_arg(args, "kind")?;
    let kind = DIAGRAM_KINDS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| format!("no diagram kind `{key}`"))?;
    let name = str_arg(args, "name")?.to_owned();
    let owner = opt_id(args, "owner")?.unwrap_or(host.document().model().root());
    let diagram =
        change(host, "create_diagram", |tx| edit::create_diagram(tx, owner, kind, &name))?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "diagram": diagram, "kind": key, "name": name })))
}

fn create_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let tool = node_tool(kind, str_arg(args, "tool")?)?;
    let model = host.document().model();
    let origin = match (opt_num(args, "x"), opt_num(args, "y")) {
        (Some(x), Some(y)) => Point::new(x, y),
        _ => free_origin(model, diagram),
    };
    let name = opt_str(args, "name").map(str::to_owned);
    let base = name.clone().unwrap_or_else(|| default_base(tool));
    let (element, node) = change(host, "create_element", |tx| {
        let (element, node) = palette::create_node(tx, diagram, tool, origin, &base)?;
        if let Some(name) = &name
            && tool.is_named()
        {
            tx.rename(element, name.clone())?;
        }
        if is_sequence(tx.model(), diagram) {
            sequence_edit::tidy(tx, diagram)?;
        }
        Ok((element, node))
    })?;
    host.show_diagram(diagram);
    let model = host.document().model();
    Ok(Reply::json(json!({
        "element": element,
        "presentation": node,
        "kind": kind_name(model, element),
        "name": member::display_name(model, element),
    })))
}

/// The start of a free default name for `tool`, e.g. `Class` for `Class1`.
fn default_base(tool: NodeTool) -> String {
    let key = tool.key();
    let mut chars = key.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn create_model_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let owner = id_arg(args, "owner")?;
    let key = str_arg(args, "tool")?;
    let name = str_arg(args, "name")?.to_owned();
    let tool = DIAGRAM_KINDS
        .iter()
        .flat_map(|(_, kind)| palette::palette(*kind))
        .find_map(|t| match t {
            Tool::Node(tool) if tool.key() == key => Some(tool),
            _ => None,
        })
        .ok_or_else(|| format!("no element tool `{key}`"))?;
    let model = host.document().model();
    let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
    let data = tool.element_data(if tool.is_named() { name } else { String::new() });
    if !owner_kind.can_own(&data.kind) {
        return Err(format!("a {} cannot own a {}", owner_kind.type_name(), data.kind.type_name()));
    }
    let element = change(host, "create_model_element", |tx| tx.create(owner, data))?;
    Ok(Reply::json(summary(host.document().model(), element)))
}

fn show_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let element = id_arg(args, "element")?;
    let model = host.document().model();
    let origin = match (opt_num(args, "x"), opt_num(args, "y")) {
        (Some(x), Some(y)) => Point::new(x, y),
        _ => free_origin(model, diagram),
    };
    let lines = |model: &Model| -> Vec<Id> {
        model.view(diagram).map_or_else(Vec::new, |v| {
            v.presentations()
                .iter()
                .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
                .map(|p| p.id)
                .collect()
        })
    };
    let node = change(host, "show_element", |tx| {
        let before = lines(tx.model());
        let node = palette::show_element(tx, diagram, element, origin)?;
        let new: Vec<Id> = lines(tx.model()).into_iter().filter(|l| !before.contains(l)).collect();
        autoroute::route_new_lines(tx, diagram, &new)?;
        if is_sequence(tx.model(), diagram) {
            sequence_edit::tidy(tx, diagram)?;
        }
        Ok(node)
    })?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "presentation": node })))
}

fn connect(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let tool = edge_tool(kind, str_arg(args, "tool")?)?;
    let model = host.document().model();
    let source = on_diagram(model, diagram, id_arg(args, "source")?)?;
    let target = on_diagram(model, diagram, id_arg(args, "target")?)?;
    let slot = match opt_num(args, "y") {
        Some(y) if tool.message_sort().is_some() => {
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
            scene.sequence.as_ref().and_then(|l| l.slot_at(y))
        }
        _ => None,
    };
    let edge = change(host, "connect", |tx| {
        let edge = if tool.message_sort().is_some() {
            palette::connect_message(tx, diagram, tool, source, target, slot)?
        } else {
            palette::connect(tx, diagram, tool, source, target)?
        };
        if tool != EdgeTool::NoteAnchor {
            autoroute::route_new_lines(tx, diagram, &[edge])?;
        }
        Ok(edge)
    })?;
    let model = host.document().model();
    let element = model.view(diagram).and_then(|v| v.get(edge)).and_then(|p| p.element);
    Ok(Reply::json(json!({
        "presentation": edge,
        "element": element,
        "kind": element.map_or("noteAnchor", |e| kind_name(model, e)),
    })))
}

fn add_member(host: &mut dyn Host, args: &Value) -> Outcome {
    let owner = id_arg(args, "owner")?;
    let text = str_arg(args, "text")?.trim().to_owned();
    let model = host.document().model();
    let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
    let literal = matches!(owner_kind, ElementKind::Enumeration(_))
        && !text.contains('(')
        && !text.contains(':');
    let data = if literal {
        ElementData::new(text.clone(), ElementKind::EnumerationLiteral)
    } else if text.contains('(') {
        ElementData::new("", ElementKind::operation(None))
    } else {
        ElementData::new("", ElementKind::attribute(None)).with_visibility(Visibility::Private)
    };
    if !owner_kind.can_own(&data.kind) {
        return Err(format!("a {} cannot own a {}", owner_kind.type_name(), data.kind.type_name()));
    }
    let member = change(host, "add_member", |tx| {
        let member = tx.create(owner, data)?;
        if !literal {
            member::apply_member_text(tx, member, &text)?;
        }
        Ok(member)
    })?;
    let model = host.document().model();
    Ok(Reply::json(json!({
        "id": member,
        "kind": kind_name(model, member),
        "text": if literal { text } else { member::member_text(model, member) },
    })))
}

fn update_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let model = host.document().model();
    let kind = model.get(id).ok_or_else(|| format!("no element {id}"))?.kind().clone();
    let name = opt_str(args, "name").map(str::to_owned);
    let text = opt_str(args, "text").map(str::to_owned);
    let documentation = opt_str(args, "documentation").map(str::to_owned);
    let stereotypes: Option<Vec<String>> = args
        .get("stereotypes")
        .and_then(Value::as_array)
        .map(|s| s.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    if text.is_some() && !matches!(kind, ElementKind::Attribute(_) | ElementKind::Operation(_)) {
        return Err(format!(
            "only attributes and operations take `text`, not a {}",
            kind.type_name()
        ));
    }
    change(host, "update_element", |tx| {
        if let Some(text) = &text {
            member::apply_member_text(tx, id, text)?;
        }
        if let Some(name) = name {
            tx.rename(id, name)?;
        }
        if documentation.is_some() || stereotypes.is_some() {
            tx.update(id, |data| {
                if let Some(documentation) = documentation {
                    data.documentation = documentation;
                }
                if let Some(stereotypes) = stereotypes {
                    data.stereotypes = stereotypes;
                }
            })?;
        }
        sequence_edit::tidy_all(tx)
    })?;
    Ok(Reply::json(summary(host.document().model(), id)))
}

fn move_node(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let node = on_diagram(model, diagram, id_arg(args, "node")?)?;
    let bounds = model
        .view(diagram)
        .and_then(|v| v.get(node))
        .and_then(|p| p.bounds())
        .ok_or("only nodes can be moved")?;
    let x = opt_num(args, "x").ok_or("`x` is missing")?;
    let y = opt_num(args, "y").ok_or("`y` is missing")?;
    let width = opt_num(args, "width").unwrap_or(bounds.width).max(4.0);
    let height = opt_num(args, "height").unwrap_or(bounds.height).max(4.0);
    change(host, "move_node", |tx| {
        tx.set_bounds(diagram, node, Rect::new(x, y, width, height))?;
        if is_sequence(tx.model(), diagram) {
            sequence_edit::tidy(tx, diagram)?;
        }
        Ok(())
    })?;
    Ok(Reply::json(
        json!({ "presentation": node, "x": x, "y": y, "width": width, "height": height }),
    ))
}

fn arrange_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if !layout::can_arrange(kind) {
        return Err(format!(
            "only class, component and deployment diagrams can be arranged, not a {} diagram",
            diagram_key(kind)
        ));
    }
    let direction = match opt_str(args, "direction") {
        Some("right") => layout::Direction::Right,
        _ => layout::Direction::Down,
    };
    let model = host.document().model();
    let scope: Option<Vec<Id>> = ids_arg(args, "nodes")?
        .map(|ids| ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect())
        .transpose()?;
    let label = host.label("arrange_diagram");
    let (doc, measure) = host.parts();
    let moved = edit::apply(doc, label, measure, |tx| {
        layout::arrange(tx, diagram, scope.as_deref(), direction, measure)
    })
    .map_err(model_error)?;
    Ok(Reply::json(json!({ "moved": moved.len() })))
}

fn route_lines(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let view = model.view(diagram).ok_or("no view")?;
    let lines: Vec<Id> = match ids_arg(args, "lines")? {
        Some(ids) => {
            ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect::<Result<_, _>>()?
        }
        None => view
            .presentations()
            .iter()
            .filter(|p| matches!(p.shape, Shape::Edge { .. }) && p.element.is_some())
            .map(|p| p.id)
            .collect(),
    };
    let routed = change(host, "route_lines", |tx| autoroute::route_lines(tx, diagram, &lines))?;
    Ok(Reply::json(json!({ "routed": routed })))
}

fn delete_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let model = host.document().model();
    if model.contains(id) {
        if id == model.root() {
            return Err("the model's root cannot be deleted".into());
        }
        let what = summary(model, id);
        change(host, "delete_element", |tx| {
            tx.delete(id)?;
            sequence_edit::tidy_all(tx)
        })?;
        return Ok(Reply::json(json!({ "deleted": what })));
    }
    let shown = model.views().find_map(|(d, v)| v.get(id).map(|p| (d, p.element)));
    match shown {
        Some((diagram, None)) => {
            change(host, "delete_element", |tx| {
                tx.remove_presentation(diagram, id)?;
                sequence_edit::tidy_all(tx)
            })?;
            Ok(Reply::json(json!({ "removed": id })))
        }
        Some((_, Some(element))) => delete_element(host, &json!({ "id": element.to_string() })),
        None => Err(format!("no element or presentation {id}")),
    }
}

fn remove_from_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let presentation = on_diagram(model, diagram, id_arg(args, "id")?)?;
    change(host, "remove_from_diagram", |tx| {
        tx.remove_presentation(diagram, presentation)?;
        sequence_edit::tidy_all(tx)
    })?;
    Ok(Reply::json(json!({ "removed": presentation })))
}

fn undo(host: &mut dyn Host, undo: bool) -> Outcome {
    let (doc, _) = host.parts();
    let done = if undo { doc.undo() } else { doc.redo() };
    let word = if undo { "undo" } else { "redo" };
    if done { Ok(Reply::text(format!("{word} done"))) } else { Err(format!("nothing to {word}")) }
}

fn open_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let diagram = id_arg(args, "diagram")?;
    diagram_of(host.document().model(), diagram)?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "shown_diagram": diagram })))
}

// ----- Files ---------------------------------------------------------------------------------

fn new_project(host: &mut dyn Host, args: &Value) -> Outcome {
    let doc = new_document(opt_str(args, "name").unwrap_or("Project"));
    let diagram = first_diagram(doc.model());
    host.replace(doc, None)?;
    Ok(Reply::json(json!({ "diagram": diagram })))
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

fn open_file(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let path = access.check(Path::new(str_arg(args, "path")?), None)?;
    if extension(&path) != "ucraft" {
        return Err("only UmlCraft projects (.ucraft) can be opened".into());
    }
    let doc = crate::host::open(&path)?;
    let project = doc.model().get(doc.model().root()).map(|e| e.name().to_owned());
    host.replace(doc, Some(path.clone()))?;
    Ok(Reply::json(json!({ "opened": path, "project": project })))
}

fn save_file(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let own = host.path().map(Path::to_path_buf);
    let path: PathBuf = match (opt_str(args, "path"), &own) {
        (Some(path), _) => {
            let mut path = PathBuf::from(path);
            if extension(&path) != "ucraft" {
                path.as_mut_os_string().push(".ucraft");
            }
            access.check(&path, own.as_deref())?
        }
        (None, Some(own)) => own.clone(),
        (None, None) => return Err("the project has no file yet; give `path`".into()),
    };
    umlcraft_format::save(host.document().model(), &path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let (doc, _) = host.parts();
    doc.mark_saved();
    host.saved(path.clone());
    Ok(Reply::json(json!({ "saved": path })))
}

fn export_image(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let path = access.check(Path::new(str_arg(args, "path")?), None)?;
    let svg = diagram_svg(host, diagram);
    let bytes = match extension(&path).as_str() {
        "svg" => svg.into_bytes(),
        "png" => png(&svg, 2.0)?,
        _ => return Err("the image file needs the extension .svg or .png".into()),
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    }
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Reply::json(json!({ "written": path })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Headless;

    fn ok(reply: Reply) -> Value {
        assert!(!reply.is_error, "{reply:?}");
        match &reply.content[0] {
            Content::Text(text) => serde_json::from_str(text).unwrap_or(json!(text)),
            Content::Image(_) => json!("image"),
        }
    }

    fn run(host: &mut Headless, name: &str, args: Value) -> Value {
        ok(call(host, &Access::default(), name, &args))
    }

    fn id(value: &Value) -> String {
        value.as_str().unwrap().to_owned()
    }

    #[test]
    fn every_tool_is_listed_and_known() {
        let mut host = Headless::new();
        for tool in list() {
            let name = tool["name"].as_str().unwrap();
            let reply = call(&mut host, &Access::default(), name, &json!({}));
            if let Content::Text(text) = &reply.content[0] {
                assert!(!text.starts_with("unknown tool"), "{name}");
            }
        }
    }

    #[test]
    fn a_class_diagram_is_built_step_by_step() {
        let mut host = Headless::new();
        let document = run(&mut host, "get_document", json!({}));
        assert_eq!(document["live"], false);
        let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
        let d = id(&d["diagram"]);
        let order = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "class", "name": "Order" }),
        );
        let customer = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "class", "name": "Customer" }),
        );
        let premium = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "class", "name": "Premium" }),
        );
        assert_eq!(order["name"], "Order");
        let total = run(
            &mut host,
            "add_member",
            json!({ "owner": order["element"], "text": "+ total() : int" }),
        );
        assert_eq!(total["kind"], "operation");
        let name = run(
            &mut host,
            "add_member",
            json!({ "owner": customer["element"], "text": "- name : String" }),
        );
        assert_eq!(name["text"], "- name : String");
        let association = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "association", "source": customer["element"], "target": order["element"] }),
        );
        assert_eq!(association["kind"], "association");
        run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "generalization", "source": premium["element"], "target": customer["element"] }),
        );
        // UML forbids it, so UmlCraft refuses it and nothing changes.
        let revision = host.document().revision();
        let refused = call(
            &mut host,
            &Access::default(),
            "connect",
            &json!({ "diagram": d, "tool": "realization", "source": order["element"], "target": customer["element"] }),
        );
        assert!(refused.is_error);
        assert_eq!(host.document().revision(), revision);

        let arranged = run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        assert!(arranged["moved"].as_u64().unwrap() > 0);
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        assert_eq!(diagram["nodes"].as_array().unwrap().len(), 3);
        assert_eq!(diagram["lines"].as_array().unwrap().len(), 2);
        assert!(diagram["element_tools"].as_array().unwrap().contains(&json!("class")));
        let image = call(&mut host, &Access::default(), "render_diagram", &json!({ "diagram": d }));
        assert!(matches!(&image.content[0], Content::Image(png) if png.starts_with(b"\x89PNG")));
        host.document().model().check().unwrap();

        // One undo step per change.
        run(&mut host, "delete_element", json!({ "id": order["element"] }));
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        assert_eq!(diagram["nodes"].as_array().unwrap().len(), 2);
        run(&mut host, "undo", json!({}));
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        assert_eq!(diagram["nodes"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn a_sequence_diagram_gets_lifelines_and_messages() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Order" }));
        let d = id(&d["diagram"]);
        let a = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "lifeline", "name": "shop" }),
        );
        let b = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "lifeline", "name": "bank" }),
        );
        let call_ = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "message", "source": a["element"], "target": b["element"] }),
        );
        assert_eq!(call_["kind"], "message");
        run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "reply", "source": b["element"], "target": a["element"] }),
        );
        host.document().model().check().unwrap();
    }

    #[test]
    fn files_only_in_allowed_folders() {
        let dir = std::env::temp_dir().join("umlcraft-mcp-files");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The server keeps canonical paths: on CI runners the temporary folder is a short
        // name (Windows) or a symbolic link (macOS).
        let dir = crate::host::absolute(&dir);
        let access = Access { folders: vec![dir.clone()], any: false };
        let mut host = Headless::new();
        let file = dir.join("shop");
        let saved = call(&mut host, &access, "save_file", &json!({ "path": file }));
        assert!(!saved.is_error, "{saved:?}");
        assert!(dir.join("shop.ucraft").exists());
        assert!(!host.document().is_dirty());
        let outside = std::env::temp_dir().join("elsewhere.ucraft");
        assert!(call(&mut host, &access, "save_file", &json!({ "path": outside })).is_error);
        let d = host.active_diagram().unwrap().to_string();
        let png = call(
            &mut host,
            &access,
            "export_image",
            &json!({ "diagram": d, "path": dir.join("a.svg") }),
        );
        assert!(!png.is_error, "{png:?}");
        let opened =
            call(&mut host, &access, "open_file", &json!({ "path": dir.join("shop.ucraft") }));
        assert!(!opened.is_error, "{opened:?}");
        assert_eq!(host.path(), Some(dir.join("shop.ucraft").as_path()));
    }

    #[test]
    fn replies_survive_the_trip_through_json() {
        let reply = Reply {
            content: vec![Content::Text("a".into()), Content::Image(vec![1, 2, 3])],
            is_error: true,
        };
        assert_eq!(Reply::from_json(&reply.to_json()), reply);
    }
}
