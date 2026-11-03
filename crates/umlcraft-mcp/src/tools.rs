//! The MCP tools: their descriptions and input schemas ([`list`]), and running one against a
//! [`Host`] ([`call`]). Ids are the model's UUIDs as strings. Every tool that changes the
//! project is one undo step, made as the app makes its edits.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use umlcraft_model::{
    DiagramKind, ElementData, ElementKind, Id, InteractionOperator, MessageSort, Model, ModelError,
    Point, Rect, Shape, Tx, Visibility,
};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Preview};
use umlcraft_notation::{
    autoroute, composite, edit, layout, member, sequence_edit, state_edit, svg,
};

use crate::base64;
use crate::host::{Access, Host, first_diagram, new_document};
use crate::structogram;

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

    pub fn json(value: Value) -> Self {
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
then `arrange_diagram` (any kind but sequence diagrams) and `render_diagram` to look at \
the result (`screenshot` shows the app's whole window while it runs). `get_diagram` lists what a diagram shows and which element and relationship tools \
its kind offers. In a sequence diagram, `connect` draws a message between lifelines below the others \
unless `y` says where, `wrap_in_fragment` puts messages into an `alt`, `loop`, … and `add_operand` adds a \
case. Structograms (Nassi-Shneiderman diagrams) are JSON trees: `create_structogram`, \
`get_structogram`, `update_structogram`. `batch` runs several calls as one undo step; a string `$0.id` in a later call's \
arguments takes the `id` from the first call's reply. Ids are UUID strings returned by the \
tools.";

/// The description of `create_structogram`, with the tree it takes.
const STRUCTOGRAM_FORMAT: &str = "Creates a structogram (Nassi-Shneiderman diagram) from a \
tree: {\"title\": \"total(a, n)\", \"kind\": \"sub\", \"body\": [{\"kind\": \"instruction\", \
\"text\": \"s := 0\"}, {\"kind\": \"for\", \"text\": \"for i := 1 to n\", \"body\": [...]}, \
{\"kind\": \"if\", \"text\": \"s > 100?\", \"then\": [...], \"else\": [...]}, {\"kind\": \
\"case\", \"text\": \"unit?\", \"branches\": [{\"label\": \"1\", \"body\": [...]}, {\"label\": \
\"default\", \"body\": [...]}]}, {\"kind\": \"while\", \"text\": \"while x > 0\", \"body\": \
[...]}, {\"kind\": \"repeat\", \"text\": \"until x = 0\", \"body\": [...]}, {\"kind\": \
\"forever\", \"body\": [...]}, {\"kind\": \"call\", \"text\": \"sort(a)\"}, {\"kind\": \
\"jump\", \"text\": \"return s\"}, {\"kind\": \"try\", \"text\": \"e\", \"body\": [...], \
\"catch\": [...], \"finally\": [...]}, {\"kind\": \"parallel\", \"branches\": [[...], \
[...]]}]}. Texts are written as structograms write them: no types, `x := value`, no `;`, \
conditions as questions; several lines are joined with \\n. A case's last branch is its \
default (label `%` for none). Elements may have `comment`, `disabled` and `color` (six hex \
digits). `kind` of the root: program (default), sub for a method, includable; `keywords`: \
english (default) or german (`für`, `solange`, `bis`, `gib`).";

const DIAGRAM_KINDS: [(&str, DiagramKind); 8] = [
    ("class", DiagramKind::Class),
    ("use_case", DiagramKind::UseCase),
    ("sequence", DiagramKind::Sequence),
    ("activity", DiagramKind::Activity),
    ("state_machine", DiagramKind::StateMachine),
    ("component", DiagramKind::Component),
    ("deployment", DiagramKind::Deployment),
    ("composite_structure", DiagramKind::CompositeStructure),
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
    let structogram_schema = json!({
        "type": "object",
        "description": format!(
            "title (string), kind (program, sub, includable), keywords (english, german), \
             comment, body (list of elements, each with `kind`: {})",
            structogram::kind_names().join(", ")
        ),
    });
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
            "get_structogram",
            "A structogram (Nassi-Shneiderman diagram) as the tree `create_structogram` takes, \
             with its owner and the operation whose method it shows.",
            json!({ "id": id("The structogram.") }),
            &["id"],
        )),
        read_only(tool(
            "get_diagram",
            "What a diagram shows: nodes with their element, kind, name and bounds as drawn; \
             lines with their element and ends; and the tools its kind offers for \
             `create_element` and `connect`. Sequence diagrams: messages with their height \
             `y`, fragments with the range of each operand, and the activation bars of each \
             lifeline (`top`, `bottom`).",
            json!({ "diagram": id("The diagram; the one shown if omitted.") }),
            &[],
        )),
        read_only(tool(
            "render_diagram",
            "A diagram or structogram as a PNG image, as UmlCraft draws it.",
            json!({
                "diagram": id("The diagram or structogram; the one shown if omitted."),
                "scale": number("Pixels per unit, 0.5 to 2; default 1."),
            }),
            &[],
        )),
        read_only(tool(
            "screenshot",
            "The UmlCraft app's window as a PNG image, as the user sees it: title bar, ribbon, \
             model tree, properties, diagram tabs and canvas, also while other windows cover \
             it, while it is minimized or placed off screen (`set_window`). Only while the app \
             runs (on Windows); `render_diagram` draws a diagram without it.",
            json!({
                "scale": number("Size relative to the window's pixels, 0.25 to 1; default 1."),
            }),
            &[],
        )),
        tool(
            "set_window",
            "Sizes and places the UmlCraft app's window, for `screenshot`s that should not \
             disturb the user: `offscreen` puts it beyond the screens, where it keeps drawing \
             but nobody sees it, `minimized` to the taskbar, `screen` back where the user sees \
             it. Never takes the focus. Without arguments it only tells the window's size, \
             place and scale (2 at 200 %). Only while the app runs (on Windows).",
            json!({
                "width": number("Width in the pixels of `screenshot` (physical pixels)."),
                "height": number("Height in the pixels of `screenshot`."),
                "placement": { "type": "string", "enum": ["screen", "offscreen", "minimized"] },
            }),
            &[],
        ),
        tool(
            "set_view",
            "Changes how the UmlCraft app looks: its theme and language (both are the user's \
             settings, which the app keeps; set them back when done) and the zoom of the \
             diagram or structogram shown. Only while the app runs.",
            json!({
                "theme": { "type": "string", "enum": ["light", "dark"] },
                "language": { "type": "string", "enum": ["en", "de", "system"] },
                "zoom": {
                    "description": "\"fit\" for the whole diagram, or a percentage such as 100.",
                    "oneOf": [{ "type": "string", "enum": ["fit"] }, { "type": "number" }],
                },
            }),
            &[],
        ),
        tool(
            "select",
            "Selects elements on the diagram shown, as a click would; the properties panel \
             shows the first. An empty list clears the selection.",
            json!({
                "ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Elements or their presentations on the diagram.",
                },
                "diagram": id("The diagram, shown first; the one shown if omitted."),
            }),
            &["ids"],
        ),
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
             around them). On composite structure diagrams a `part` becomes an attribute of \
             the class drawn around it, which grows to hold it, and a `port` sits on the \
             border of a class or a part; `on` names that class or part instead of `x`, `y`. \
             A port on a part belongs to the class that is the part's type, so type the part \
             first (`update_element` with the member `text`, e.g. `engine : Engine`).",
            json!({
                "diagram": id("The diagram."),
                "tool": text("The diagram tool."),
                "name": text("Its name; a free default name if omitted."),
                "x": number("Left edge."),
                "y": number("Top edge."),
                "on": id("Composite structure diagrams: the class (or part) a part goes into or \
                          a port goes on, when `x` and `y` are not given."),
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
             `y` is given; those sent from a lifeline that is busy with a call it has not \
             answered yet continue that call's bar. `foundMessage` (from outside the \
             interaction) and `lostMessage` (to outside) take one lifeline, as `source`; a \
             lost message is drawn as a reply (it mostly answers a found one) until \
             `update_element` gives it another `sort`.",
            json!({
                "diagram": id("The diagram."),
                "tool": text("The relationship tool."),
                "source": id("Element or presentation it starts at."),
                "target": id("Element or presentation it ends at; not for found and lost messages."),
                "y": number("Sequence diagrams: the height of the message."),
                "name": text("Its name, e.g. a message's or an association's."),
            }),
            &["diagram", "tool", "source"],
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
            "wrap_in_fragment",
            "Puts messages of a sequence diagram into a new combined fragment, together with \
             everything drawn between them; they must lie in the same interaction or operand. \
             An `alt` gets an `else` operand. The reply lists the fragment's operands.",
            json!({
                "diagram": id("The sequence diagram."),
                "items": { "type": "array", "items": { "type": "string" },
                           "description": "The messages, fragments or interaction uses." },
                "operator": { "type": "string",
                              "enum": ["alt", "opt", "loop", "par", "break", "critical", "neg",
                                       "assert", "strict", "seq", "ignore", "consider"],
                              "description": "`opt` if omitted." },
                "guard": text("The first operand's guard, without brackets."),
            }),
            &["diagram", "items"],
        ),
        tool(
            "create_structogram",
            STRUCTOGRAM_FORMAT,
            json!({
                "structogram": structogram_schema.clone(),
                "owner": id("Package or class to put it in; the operation's class, else the \
                             project, if omitted."),
                "operation": id("The operation whose method it shows."),
            }),
            &["structogram"],
        ),
        tool(
            "update_structogram",
            "Changes a structogram: what the given tree sets (as for `create_structogram`; \
             `body` replaces the whole content), and the operation it shows (\"\" for none).",
            json!({
                "id": id("The structogram."),
                "structogram": structogram_schema,
                "operation": id("The operation whose method it shows, or \"\" for none."),
            }),
            &["id"],
        ),
        tool(
            "edit_structogram",
            "Changes one element of a structogram: inserts `element` (an element of the tree \
             `create_structogram` takes) at `path`, replaces the element there or removes it. \
             A path names a list and an index, as `get_structogram` nests them: `body.2`, \
             `body.1.then.0`, `body.0.else.3`, `body.4.body.0` (in a loop or try), \
             `body.0.branches.1.body.0` (in a case), `body.0.branches.1.0` (in a parallel \
             element); the index may be the list's length to append.",
            json!({
                "id": id("The structogram."),
                "action": { "type": "string", "enum": ["insert", "replace", "remove"] },
                "path": text("Where, e.g. `body.1.then.0`."),
                "element": { "type": "object",
                             "description": "For insert and replace: the element, with `kind`." },
            }),
            &["id", "action", "path"],
        ),
        tool(
            "add_operand",
            "Adds an operand at the bottom of a combined fragment (`alt`, `par`, …) of a \
             sequence diagram, with room for it in the fragment's frames.",
            json!({
                "fragment": id("The combined fragment."),
                "guard": text("The operand's guard, without brackets, e.g. `x > 0` or `else`."),
            }),
            &["fragment"],
        ),
        tool(
            "update_element",
            "Changes an element: its name, a member's text (as for `add_member`), its \
             documentation or its stereotypes; a message's sort, guard, arguments, return \
             value and operation; a combined fragment's operator (an `alt` gets an `else` \
             operand) and the lifelines a fragment or interaction use covers (its frame then \
             spans them); the guard of an operand, flow or transition.",
            json!({
                "id": id("The element."),
                "name": text("New name."),
                "text": text("Attributes and operations: the whole member as text."),
                "documentation": text("New documentation."),
                "stereotypes": { "type": "array", "items": { "type": "string" } },
                "guard": text("Messages, operands, flows, transitions: the guard, without brackets."),
                "sort": { "type": "string",
                          "enum": ["synchronous", "asynchronous", "reply", "create", "destroy"] },
                "arguments": text("Messages: the arguments, written between the parentheses."),
                "return_value": text("Replies: the value they return."),
                "operation": text("Messages: the id of the operation it calls, or \"\" for none."),
                "operator": { "type": "string",
                              "enum": ["alt", "opt", "loop", "par", "break", "critical", "neg",
                                       "assert", "strict", "seq", "ignore", "consider"] },
                "covered": { "type": "array", "items": { "type": "string" },
                             "description": "Fragments and interaction uses: the lifelines \
                                             covered; all if empty." },
            }),
            &["id"],
        ),
        tool(
            "move_node",
            "Moves a node of a diagram, or gives it another size. In a sequence diagram it \
             also moves a message (to height `y`) or a fragment (its top to `y`) as dragging \
             does: into the interaction or operand found there, with the bar a message starts; \
             `get_diagram` gives the heights of messages and operands.",
            json!({
                "diagram": id("The diagram."),
                "node": id("The element or its presentation: a node, or a message."),
                "x": number("Left edge; kept if omitted."),
                "y": number("Top edge, or a message's height."),
                "width": number("New width."),
                "height": number("New height."),
            }),
            &["diagram", "node", "y"],
        ),
        tool(
            "arrange_diagram",
            "Lays out a diagram other than a sequence diagram in layers (generals above their \
             specifics, wholes above their parts, flows and transitions from step to step, \
             actors beside the system boundary; partitions stay columns and regions keep what \
             they hold) and routes its lines; with `nodes` only those, where they are, pushing \
             others aside. Without a direction, state machines and use case diagrams run to \
             the right and the others down.",
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
        tool(
            "batch",
            "Runs several tools one after the other as one change, which one undo takes back. \
             If one fails, nothing is changed. A string argument `$2.element` stands for the \
             field `element` of the reply of the call at index 2 (from 0), so later calls can \
             use what earlier ones created. Not for undo, redo, batch, open_file and \
             new_document.",
            json!({
                "calls": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": text("The tool's name."),
                            "arguments": { "type": "object" },
                        },
                        "required": ["tool"],
                    },
                },
                "label": text("What the change is called in the undo history."),
            }),
            &["calls"],
        ),
        tool("undo", "Undoes the last change.", json!({}), &[]),
        tool("redo", "Redoes the last undone change.", json!({}), &[]),
        tool(
            "open_diagram",
            "Shows a diagram or structogram (in the app: brings its tab to the front).",
            json!({ "diagram": id("The diagram or structogram.") }),
            &["diagram"],
        ),
        tool(
            "new_document",
            "Starts a new project with one class diagram, or with `sample` the app's sample \
             project (Velo, every diagram kind; File › Open sample project). In the app only \
             when the open project has no unsaved changes.",
            json!({
                "name": text("The project's name."),
                "sample": { "type": "boolean", "description": "The sample project (only in the app)." },
            }),
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
            "Writes a diagram or structogram as an SVG or PNG image to an allowed folder (by the \
             extension); a structogram also as a Structorizer file (.nsd).",
            json!({
                "diagram": id("The diagram or structogram."),
                "path": text("The file: .svg, .png, or .nsd for a structogram."),
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
        // The app answers this one itself, with its window; it never comes here from the app
        // but inside `batch`.
        "screenshot" | "set_window" | "set_view" => Err(if host.live() {
            format!("`{name}` cannot run inside `batch`; call it on its own.")
        } else {
            format!(
                "`{name}` needs the UmlCraft window: this session works on a project of the \
                 server's own (the app is not running, or this session started without it). \
                 `render_diagram` draws a diagram without the app."
            )
        }),
        "select" => select(host, args),
        "create_diagram" => create_diagram(host, args),
        "create_element" => create_element(host, args),
        "create_model_element" => create_model_element(host, args),
        "show_element" => show_element(host, args),
        "connect" => connect(host, args),
        "add_member" => add_member(host, args),
        "wrap_in_fragment" => wrap_in_fragment(host, args),
        "add_operand" => add_operand(host, args),
        "get_structogram" => get_structogram(host, args),
        "create_structogram" => create_structogram(host, args),
        "update_structogram" => update_structogram(host, args),
        "edit_structogram" => edit_structogram(host, args),
        "update_element" => update_element(host, args),
        "move_node" => move_node(host, args),
        "arrange_diagram" => arrange_diagram(host, args),
        "route_lines" => route_lines(host, args),
        "delete_element" => delete_element(host, args),
        "remove_from_diagram" => remove_from_diagram(host, args),
        "batch" => batch(host, access, args),
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

/// The size of `screenshot`'s image relative to the window, from its arguments.
pub fn screenshot_scale(args: &Value) -> f32 {
    opt_num(args, "scale").unwrap_or(1.0).clamp(0.25, 1.0)
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
        "structograms": structograms,
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
    let mut bars = Vec::new();
    for p in view.presentations() {
        // Activation bars, so that a client can send a message from inside one (at its `y`).
        if let Shape::Activation { lifeline, top, height, .. } = p.shape {
            let element = view.get(lifeline).and_then(|l| l.element);
            bars.push(json!({ "lifeline": element, "top": top, "bottom": top + height }));
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
            nodes.push(node);
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
    let mut reply = json!({
        "id": diagram,
        "name": model.get(diagram).map_or("", |e| e.name()),
        "kind": diagram_key(kind),
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
    let scale = opt_num(args, "scale").unwrap_or(1.0).clamp(0.5, 2.0);
    let shown = opt_id(args, "diagram")?.or(host.active_diagram());
    let model = host.document().model();
    if let Some(p) = shown.and_then(|id| model.get(id)).and_then(|e| e.kind().as_structogram()) {
        let svg = umlcraft_nsd::svg::render(&p.structogram, umlcraft_nsd::Metrics::for_platform());
        // Structogram images are in pixels already.
        let image = png(&svg, scale * svg::SCALE)?;
        return Ok(Reply { content: vec![Content::Image(image)], is_error: false });
    }
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
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
    let origin = match (opt_num(args, "x"), opt_num(args, "y"), opt_id(args, "on")?) {
        (Some(x), Some(y), _) => Point::new(x, y),
        (_, _, Some(on)) => {
            let on = on_diagram(model, diagram, on)?;
            composite::free_spot(model, diagram, on, tool).ok_or("`on` is not a node")?
        }
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
    let name = opt_str(args, "name").map(str::to_owned);
    let model = host.document().model();
    let source = on_diagram(model, diagram, id_arg(args, "source")?)?;
    // Found and lost messages are drawn from their lifeline to itself.
    let target = match (tool, opt_id(args, "target")?) {
        (EdgeTool::FoundMessage | EdgeTool::LostMessage, _) => source,
        (_, Some(target)) => on_diagram(model, diagram, target)?,
        (_, None) => return Err("`target` is missing".into()),
    };
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
        let element = tx.model().view(diagram).and_then(|v| v.get(edge)).and_then(|p| p.element);
        if let (Some(name), Some(element)) = (&name, element) {
            tx.rename(element, name.clone())?;
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
    let mut settings = kind.clone();
    set_properties(model, &mut settings, args)?;
    let is_alt = |kind: &ElementKind| matches!(kind, ElementKind::CombinedFragment(p) if p.operator == InteractionOperator::Alt);
    let becomes_alt = is_alt(&settings) && !is_alt(&kind);
    let recovered = args.get("covered").is_some();
    change(host, "update_element", |tx| {
        if let Some(text) = &text {
            member::apply_member_text(tx, id, text)?;
        }
        if let Some(name) = name {
            tx.rename(id, name)?;
        }
        if documentation.is_some() || stereotypes.is_some() || settings != kind {
            tx.update(id, |data| {
                if let Some(documentation) = documentation {
                    data.documentation = documentation;
                }
                if let Some(stereotypes) = stereotypes {
                    data.stereotypes = stereotypes;
                }
                data.kind = settings;
            })?;
        }
        // A fragment turned into an alt gets its else, as in the app.
        if becomes_alt {
            palette::add_else(tx, id)?;
        }
        // Frames span the lifelines they cover once these are given.
        if recovered {
            sequence_edit::cover(tx, id)?;
        }
        sequence_edit::tidy_all(tx)
    })?;
    Ok(Reply::json(summary(host.document().model(), id)))
}

const SORTS: [(&str, MessageSort); 5] = [
    ("synchronous", MessageSort::Synchronous),
    ("asynchronous", MessageSort::Asynchronous),
    ("reply", MessageSort::Reply),
    ("create", MessageSort::Create),
    ("destroy", MessageSort::Destroy),
];

const OPERATORS: [(&str, InteractionOperator); 12] = [
    ("alt", InteractionOperator::Alt),
    ("opt", InteractionOperator::Opt),
    ("loop", InteractionOperator::Loop),
    ("par", InteractionOperator::Par),
    ("break", InteractionOperator::Break),
    ("critical", InteractionOperator::Critical),
    ("neg", InteractionOperator::Neg),
    ("assert", InteractionOperator::Assert),
    ("strict", InteractionOperator::Strict),
    ("seq", InteractionOperator::Seq),
    ("ignore", InteractionOperator::Ignore),
    ("consider", InteractionOperator::Consider),
];

/// The value named by the string `key` of `args`, out of `names`.
fn pick<T: Copy>(args: &Value, key: &str, names: &[(&str, T)]) -> Result<Option<T>, String> {
    let Some(name) = opt_str(args, key) else { return Ok(None) };
    match names.iter().find(|(n, _)| *n == name) {
        Some(&(_, value)) => Ok(Some(value)),
        None => {
            let names: Vec<&str> = names.iter().map(|(n, _)| *n).collect();
            Err(format!("`{key}` is one of {}, not `{name}`", names.join(", ")))
        }
    }
}

/// Writes the settings of `args` that belong to the element's kind into `kind`: a guard, a
/// message's sort, arguments, return value and operation, a fragment's operator.
fn set_properties(model: &Model, kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    let type_name = kind.type_name();
    let refuse = |key: &str| format!("a {type_name} has no `{key}`");
    if let Some(guard) = opt_str(args, "guard") {
        let guard = guard.trim();
        let guard = guard.strip_prefix('[').and_then(|g| g.strip_suffix(']')).unwrap_or(guard);
        let guard = guard.trim().to_owned();
        match kind {
            ElementKind::Message(p) => p.guard = guard,
            ElementKind::InteractionOperand(p) => p.guard = guard,
            ElementKind::ActivityEdge(p) => p.guard = guard,
            ElementKind::Transition(p) => p.guard = guard,
            ElementKind::InternalTransition(p) => p.guard = guard,
            _ => return Err(refuse("guard")),
        }
    }
    if let Some(operator) = pick(args, "operator", &OPERATORS)? {
        let ElementKind::CombinedFragment(p) = kind else { return Err(refuse("operator")) };
        p.operator = operator;
    }
    if let Some(ids) = ids_arg(args, "covered")? {
        // Presentations stand for their lifelines.
        let lifeline = |id: Id| -> Result<Id, String> {
            let id = model.views().find_map(|(_, v)| v.get(id)?.element).unwrap_or(id);
            match model.get(id).map(|e| e.kind()) {
                Some(ElementKind::Lifeline(_)) => Ok(id),
                _ => Err(format!("{id} is no lifeline")),
            }
        };
        let covered = ids.into_iter().map(lifeline).collect::<Result<Vec<_>, _>>()?;
        match kind {
            ElementKind::CombinedFragment(p) => p.covered = covered,
            ElementKind::InteractionUse(p) => p.covered = covered,
            _ => return Err(refuse("covered")),
        }
    }
    let message_keys = ["sort", "arguments", "return_value", "operation"];
    let Some(key) = message_keys.into_iter().find(|k| args.get(k).is_some()) else {
        return Ok(());
    };
    let ElementKind::Message(p) = kind else { return Err(refuse(key)) };
    if let Some(sort) = pick(args, "sort", &SORTS)? {
        p.sort = sort;
    }
    if let Some(arguments) = opt_str(args, "arguments") {
        p.arguments = arguments.trim().to_owned();
    }
    if let Some(value) = opt_str(args, "return_value") {
        p.return_value = value.trim().to_owned();
    }
    if let Some(operation) = opt_str(args, "operation") {
        p.operation = match operation.trim() {
            "" => None,
            text => {
                let operation = parse_id(text)?;
                if !matches!(
                    model.get(operation).map(|e| e.kind()),
                    Some(ElementKind::Operation(_))
                ) {
                    return Err(format!("{operation} is no operation"));
                }
                Some(operation)
            }
        };
    }
    Ok(())
}

fn wrap_in_fragment(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if kind != DiagramKind::Sequence {
        return Err("fragments belong to sequence diagrams".into());
    }
    let model = host.document().model();
    let view = model.view(diagram).ok_or("no view")?;
    // Presentations stand for their messages.
    let items: Vec<Id> = ids_arg(args, "items")?
        .ok_or("`items` is missing")?
        .into_iter()
        .map(|i| view.get(i).and_then(|p| p.element).unwrap_or(i))
        .collect();
    let operator = pick(args, "operator", &OPERATORS)?.unwrap_or(InteractionOperator::Opt);
    let mut guard = ElementKind::InteractionOperand(Default::default());
    set_properties(model, &mut guard, &json!({ "guard": opt_str(args, "guard").unwrap_or("") }))?;
    let fragment = change(host, "wrap_in_fragment", |tx| {
        let (fragment, _) = palette::wrap_in_fragment(tx, diagram, &items)?;
        tx.update(fragment, |data| {
            if let ElementKind::CombinedFragment(p) = &mut data.kind {
                p.operator = operator;
            }
        })?;
        if let Some(&operand) = tx.model().children(fragment).first() {
            tx.update(operand, |data| data.kind = guard)?;
        }
        if operator == InteractionOperator::Alt {
            palette::add_else(tx, fragment)?;
        }
        sequence_edit::tidy(tx, diagram)?;
        Ok(fragment)
    })?;
    let model = host.document().model();
    let mut reply = summary(model, fragment);
    reply["operands"] = json!(model.children(fragment));
    Ok(Reply::json(reply))
}

/// The structogram `id` names.
fn structogram_of(model: &Model, id: Id) -> Result<&umlcraft_model::StructogramProps, String> {
    model
        .get(id)
        .and_then(|e| e.kind().as_structogram())
        .ok_or_else(|| format!("{id} is no structogram"))
}

/// `operation` if it names an operation.
fn operation_arg(model: &Model, operation: Id) -> Result<Id, String> {
    match model.get(operation).map(|e| e.kind()) {
        Some(ElementKind::Operation(_)) => Ok(operation),
        _ => Err(format!("{operation} is no operation")),
    }
}

fn structogram_reply(model: &Model, id: Id) -> Result<Value, String> {
    let p = structogram_of(model, id)?;
    Ok(json!({
        "id": id,
        "kind": "structogram",
        "owner": model.get(id).and_then(|e| e.owner()),
        "operation": p.operation,
        "structogram": structogram::to_json(&p.structogram),
    }))
}

fn get_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    Ok(Reply::json(structogram_reply(host.document().model(), id_arg(args, "id")?)?))
}

fn create_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let model = host.document().model();
    let tree = args.get("structogram").ok_or("`structogram` is missing")?;
    let diagram = structogram::from_json(tree, None)?;
    let operation = opt_id(args, "operation")?.map(|o| operation_arg(model, o)).transpose()?;
    let owner = match opt_id(args, "owner")? {
        Some(owner) => owner,
        None => operation.and_then(|o| model.get(o)?.owner()).unwrap_or(model.root()),
    };
    let id = change(host, "create_structogram", |tx| {
        let mut props = umlcraft_model::StructogramProps::new(&diagram);
        props.operation = operation;
        tx.create(owner, ElementData::new("", ElementKind::Structogram(props)))
    })?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}

fn update_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let model = host.document().model();
    let before = structogram_of(model, id)?;
    let diagram = match args.get("structogram") {
        Some(tree) => Some(structogram::from_json(tree, Some(&before.structogram))?),
        None => None,
    };
    let operation = match opt_str(args, "operation").map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(text) => Some(Some(operation_arg(model, parse_id(text)?)?)),
    };
    change(host, "update_structogram", |tx| {
        if let Some(diagram) = &diagram {
            tx.set_structogram(id, diagram)?;
        }
        if let Some(operation) = operation {
            tx.update(id, |data| {
                if let ElementKind::Structogram(p) = &mut data.kind {
                    p.operation = operation;
                }
            })?;
        }
        Ok(())
    })?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}

fn edit_structogram(host: &mut dyn Host, args: &Value) -> Outcome {
    let id = id_arg(args, "id")?;
    let before = structogram_of(host.document().model(), id)?;
    let mut tree = structogram::to_json(&before.structogram);
    let action = str_arg(args, "action")?;
    structogram::edit(&mut tree, action, str_arg(args, "path")?, args.get("element"))?;
    let diagram = structogram::from_json(&tree, Some(&before.structogram))?;
    change(host, "edit_structogram", |tx| tx.set_structogram(id, &diagram))?;
    Ok(Reply::json(structogram_reply(host.document().model(), id)?))
}

fn add_operand(host: &mut dyn Host, args: &Value) -> Outcome {
    let fragment = id_arg(args, "fragment")?;
    let model = host.document().model();
    if !matches!(model.get(fragment).map(|e| e.kind()), Some(ElementKind::CombinedFragment(_))) {
        return Err(format!("{fragment} is no combined fragment"));
    }
    let mut guard = ElementKind::InteractionOperand(Default::default());
    set_properties(model, &mut guard, &json!({ "guard": opt_str(args, "guard").unwrap_or("") }))?;
    let operand = change(host, "add_operand", |tx| {
        let operand = palette::add_operand(tx, fragment)?;
        tx.update(operand, |data| data.kind = guard)?;
        sequence_edit::tidy_all(tx)?;
        Ok(operand)
    })?;
    Ok(Reply::json(summary(host.document().model(), operand)))
}

fn move_node(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let model = host.document().model();
    let node = on_diagram(model, diagram, id_arg(args, "node")?)?;
    let presentation = model.view(diagram).and_then(|v| v.get(node)).ok_or("no presentation")?;
    // Messages and fragments of a sequence diagram move as when dragged in the app: to height
    // `y`, into the interaction or operand found there.
    let item = presentation.element.filter(|&e| {
        is_sequence(model, diagram)
            && matches!(
                model.get(e).map(|e| e.kind()),
                Some(
                    ElementKind::Message(_)
                        | ElementKind::CombinedFragment(_)
                        | ElementKind::InteractionUse(_)
                )
            )
    });
    let y = opt_num(args, "y").ok_or("`y` is missing")?;
    let bounds = presentation.bounds();
    let slot = item.and_then(|_| {
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
        scene.sequence.as_ref().and_then(|l| l.slot_at(y))
    });
    if let (Some(item), Some(slot), None) = (item, slot, bounds) {
        change(host, "move_node", |tx| palette::move_to_slot(tx, item, slot))?;
        return Ok(Reply::json(json!({ "presentation": node, "y": y })));
    }
    let bounds = bounds.ok_or("only nodes and messages can be moved")?;
    let x = opt_num(args, "x").unwrap_or(bounds.x);
    let width = opt_num(args, "width").unwrap_or(bounds.width).max(4.0);
    let height = opt_num(args, "height").unwrap_or(bounds.height).max(4.0);
    change(host, "move_node", |tx| {
        if let (Some(item), Some(slot)) = (item, slot) {
            palette::move_to_slot(tx, item, slot)?;
        }
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
        return Err(format!("a {} diagram cannot be arranged", diagram_key(kind)));
    }
    let direction = match opt_str(args, "direction") {
        Some("right") => layout::Direction::Right,
        Some("down") => layout::Direction::Down,
        _ => layout::default_direction(kind),
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
        let kind = model.get(id).map(|e| e.kind().clone());
        change(host, "delete_element", |tx| {
            // As in the app: the neighbors of a region take its room, and an operand takes the
            // line above it and its guard's place along.
            match kind {
                Some(ElementKind::Region) => state_edit::delete_region(tx, id)?,
                Some(ElementKind::InteractionOperand(_)) => sequence_edit::delete_operand(tx, id)?,
                _ => tx.delete(id)?,
            }
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

/// Tools a batch may not run: they change the history or replace the project.
const NOT_IN_BATCH: [&str; 5] = ["undo", "redo", "batch", "open_file", "new_document"];

fn batch(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let calls = args.get("calls").and_then(Value::as_array).ok_or("`calls` must be an array")?;
    let depth = host.document().undo_depth();
    let mut results: Vec<Value> = Vec::with_capacity(calls.len());
    let failed = |host: &mut dyn Host, i: usize, why: String| {
        host.parts().0.roll_back_to(depth);
        Err(format!("call {i} failed, nothing was changed: {why}"))
    };
    for (i, c) in calls.iter().enumerate() {
        let Some(name) = c.get("tool").and_then(Value::as_str) else {
            return failed(host, i, "`tool` is missing".into());
        };
        if NOT_IN_BATCH.contains(&name) {
            return failed(host, i, format!("`{name}` cannot run in a batch"));
        }
        let arguments = match resolve(c.get("arguments").unwrap_or(&json!({})), &results) {
            Ok(arguments) => arguments,
            Err(why) => return failed(host, i, why),
        };
        let reply = call(host, access, name, &arguments);
        let text: Vec<&str> = reply
            .content
            .iter()
            .filter_map(|c| match c {
                Content::Text(text) => Some(text.as_str()),
                Content::Image(_) => None,
            })
            .collect();
        let text = text.join("\n");
        if reply.is_error {
            return failed(host, i, format!("{name}: {text}"));
        }
        results.push(serde_json::from_str(&text).unwrap_or(Value::String(text)));
    }
    let label = host.label(opt_str(args, "label").unwrap_or("batch"));
    host.parts().0.squash_since(depth, label);
    Ok(Reply::json(json!({ "results": results })))
}

/// `arguments` with each string `$<index>.<field>[.<field>…]` replaced by that field of the
/// reply of an earlier call.
fn resolve(arguments: &Value, results: &[Value]) -> Result<Value, String> {
    Ok(match arguments {
        Value::String(s) if s.starts_with('$') => {
            let mut path = s[1..].split('.');
            let index = path.next().and_then(|i| i.parse::<usize>().ok());
            let Some(reply) = index.and_then(|i| results.get(i)) else {
                return Err(format!("`{s}` names no earlier call"));
            };
            let mut value = reply;
            for field in path {
                value = match value {
                    Value::Array(items) => field.parse::<usize>().ok().and_then(|i| items.get(i)),
                    _ => value.get(field),
                }
                .ok_or_else(|| format!("the reply of call {} has no `{s}`", index.unwrap_or(0)))?;
            }
            value.clone()
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| resolve(v, results)).collect::<Result<_, _>>()?)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, results)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

fn undo(host: &mut dyn Host, undo: bool) -> Outcome {
    let (doc, _) = host.parts();
    let done = if undo { doc.undo() } else { doc.redo() };
    let word = if undo { "undo" } else { "redo" };
    if done { Ok(Reply::text(format!("{word} done"))) } else { Err(format!("nothing to {word}")) }
}

fn open_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let diagram = id_arg(args, "diagram")?;
    let model = host.document().model();
    let structogram = model.get(diagram).is_some_and(|e| e.kind().as_structogram().is_some());
    if !structogram {
        diagram_of(model, diagram)?;
    }
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "shown_diagram": diagram })))
}

// ----- Files ---------------------------------------------------------------------------------

fn select(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let ids = ids_arg(args, "ids")?.ok_or("`ids` is missing")?;
    let model = host.document().model();
    let presentations =
        ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect::<Result<Vec<_>, _>>()?;
    host.show_diagram(diagram);
    host.select(presentations.clone());
    Ok(Reply::json(json!({ "diagram": diagram, "selection": presentations })))
}

fn new_project(host: &mut dyn Host, args: &Value) -> Outcome {
    if args.get("sample").and_then(Value::as_bool) == Some(true) {
        let doc = host.sample()?;
        let diagram = first_diagram(doc.model());
        host.replace(doc, None)?;
        return Ok(Reply::json(json!({ "diagram": diagram })));
    }
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
    let path = access.check(Path::new(str_arg(args, "path")?), None)?;
    let shown = opt_id(args, "diagram")?.or(host.active_diagram());
    let model = host.document().model();
    let structogram = shown.and_then(|id| model.get(id)).and_then(|e| e.kind().as_structogram());
    let bytes = match (structogram, extension(&path).as_str()) {
        (Some(p), "nsd") => {
            let origin = format!("UmlCraft {}", env!("CARGO_PKG_VERSION"));
            let made = umlcraft_nsd::Provenance { author: "", date: "", origin: &origin };
            umlcraft_nsd::write(&umlcraft_nsd::content::to_file(&p.structogram, &made)).into_bytes()
        }
        (Some(p), extension) => {
            let svg =
                umlcraft_nsd::svg::render(&p.structogram, umlcraft_nsd::Metrics::for_platform());
            match extension {
                "svg" => svg.into_bytes(),
                // Two pixels per pixel of the structogram, as diagrams get two per unit.
                "png" => png(&svg, 2.0 * svg::SCALE)?,
                _ => return Err("a structogram goes to .svg, .png or .nsd".into()),
            }
        }
        (None, extension) => {
            let (diagram, _) = diagram_arg(host, args, "diagram")?;
            let svg = diagram_svg(host, diagram);
            match extension {
                "svg" => svg.into_bytes(),
                "png" => png(&svg, 2.0)?,
                _ => return Err("the image file needs the extension .svg or .png".into()),
            }
        }
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
    fn a_screenshot_needs_the_app() {
        let mut host = Headless::new();
        let reply = call(&mut host, &Access::default(), "screenshot", &json!({}));
        assert!(reply.is_error);
        assert_eq!(screenshot_scale(&json!({ "scale": 0.1 })), 0.25);
        assert_eq!(screenshot_scale(&json!({})), 1.0);
        for tool in ["set_window", "set_view"] {
            assert!(call(&mut host, &Access::default(), tool, &json!({})).is_error, "{tool}");
        }
        let sample =
            call(&mut host, &Access::default(), "new_document", &json!({ "sample": true }));
        assert!(sample.is_error);
    }

    #[test]
    fn select_takes_elements_and_presentations() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "Shop" }));
        let d = id(&d["diagram"]);
        let order = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "class", "name": "Order" }),
        );
        let by_element = run(&mut host, "select", json!({ "ids": [order["element"]] }));
        assert_eq!(by_element["selection"], json!([order["presentation"]]));
        let by_presentation =
            run(&mut host, "select", json!({ "ids": [order["presentation"]], "diagram": d }));
        assert_eq!(by_presentation["selection"], by_element["selection"]);
        let unknown = call(
            &mut host,
            &Access::default(),
            "select",
            &json!({ "ids": [Id::generate().to_string()] }),
        );
        assert!(unknown.is_error);
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
    fn a_batch_is_one_step_and_all_or_nothing() {
        let mut host = Headless::new();
        let before = host.document().undo_depth();
        let calls = json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "Shop" } },
            { "tool": "create_element",
              "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "Order" } },
            { "tool": "create_element",
              "arguments": { "diagram": "$0.diagram", "tool": "class", "name": "Customer" } },
            { "tool": "add_member", "arguments": { "owner": "$1.element", "text": "+ total() : int" } },
            { "tool": "connect", "arguments": {
                "diagram": "$0.diagram", "tool": "association",
                "source": "$1.element", "target": "$2.element" } },
            { "tool": "arrange_diagram", "arguments": { "diagram": "$0.diagram" } },
        ]);
        let done = run(&mut host, "batch", json!({ "calls": calls, "label": "Shop" }));
        let d = id(&done["results"][0]["diagram"]);
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        assert_eq!(diagram["nodes"].as_array().unwrap().len(), 2);
        assert_eq!(diagram["lines"].as_array().unwrap().len(), 1);
        assert_eq!(host.document().undo_depth(), before + 1);
        assert_eq!(host.document().undo_label(), Some("Shop"));
        host.document().model().check().unwrap();
        run(&mut host, "undo", json!({}));
        assert!(host.document().model().get(parse_id(&d).unwrap()).is_none());

        // A failing call takes back what the calls before it did.
        let revision = host.document().revision();
        let model = format!("{:?}", host.document().model().get(host.document().model().root()));
        let calls = json!([
            { "tool": "create_diagram", "arguments": { "kind": "class", "name": "Shop" } },
            { "tool": "create_element", "arguments": { "diagram": "$0.diagram", "tool": "nonsense" } },
        ]);
        let refused = call(&mut host, &Access::default(), "batch", &json!({ "calls": calls }));
        assert!(refused.is_error);
        assert_ne!(host.document().revision(), revision);
        assert_eq!(
            format!("{:?}", host.document().model().get(host.document().model().root())),
            model
        );
        let undo = json!([{ "tool": "undo" }]);
        assert!(call(&mut host, &Access::default(), "batch", &json!({ "calls": undo })).is_error);
        let dangling = json!([{ "tool": "get_element", "arguments": { "id": "$3.element" } }]);
        assert!(
            call(&mut host, &Access::default(), "batch", &json!({ "calls": dangling })).is_error
        );
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
        let reply = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "reply", "source": b["element"], "target": a["element"] }),
        );
        host.document().model().check().unwrap();

        // Message settings.
        let uid = |v: &Value| parse_id(v.as_str().unwrap()).unwrap();
        let pay = call_["element"].clone();
        let update = json!({ "id": pay, "name": "pay", "arguments": "total", "guard": "[open]",
                             "sort": "asynchronous" });
        run(&mut host, "update_element", update);
        run(&mut host, "update_element", json!({ "id": reply["element"], "return_value": "ok" }));
        let message = |host: &Headless, id: &Value| match host.document().model().get(uid(id)) {
            Some(e) => match e.kind() {
                ElementKind::Message(p) => p.clone(),
                _ => panic!("no message"),
            },
            None => panic!("gone"),
        };
        let p = message(&host, &pay);
        assert_eq!(
            (p.sort, p.arguments.as_str(), p.guard.as_str()),
            (MessageSort::Asynchronous, "total", "open")
        );
        assert_eq!(message(&host, &reply["element"]).return_value, "ok");
        let lifeline = json!({ "id": a["element"], "guard": "x" });
        assert!(call(&mut host, &Access::default(), "update_element", &lifeline).is_error);
        let bogus = json!({ "id": pay, "sort": "sometimes" });
        assert!(call(&mut host, &Access::default(), "update_element", &bogus).is_error);

        // Fragments and their operands.
        let guards = |host: &Headless, fragment: Id| -> Vec<String> {
            let model = host.document().model();
            model
                .children(fragment)
                .iter()
                .filter_map(|&o| match model.get(o).map(|e| e.kind()) {
                    Some(ElementKind::InteractionOperand(p)) => Some(p.guard.clone()),
                    _ => None,
                })
                .collect()
        };
        let wrap = json!({ "diagram": d, "items": [pay, reply["element"]], "operator": "alt",
                           "guard": "paid" });
        let alt = run(&mut host, "wrap_in_fragment", wrap);
        let fragment = uid(&alt["id"]);
        assert_eq!(alt["operands"].as_array().map(Vec::len), Some(2));
        assert_eq!(guards(&host, fragment), ["paid", "else"]);
        let late =
            run(&mut host, "add_operand", json!({ "fragment": fragment, "guard": "[late]" }));
        assert_eq!(guards(&host, fragment), ["paid", "else", "late"]);
        run(&mut host, "delete_element", json!({ "id": late["id"] }));
        assert_eq!(guards(&host, fragment), ["paid", "else"]);
        run(&mut host, "update_element", json!({ "id": fragment, "operator": "loop" }));
        assert!(matches!(
            host.document().model().get(fragment).map(|e| e.kind()),
            Some(ElementKind::CombinedFragment(p)) if p.operator == InteractionOperator::Loop
        ));
        let pay_owner = host.document().model().get(uid(&pay)).and_then(|e| e.owner());
        assert_eq!(pay_owner, host.document().model().children(fragment).first().copied());
        host.document().model().check().unwrap();
    }

    #[test]
    fn appended_messages_continue_the_call_their_sender_is_busy_with() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Order" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
        };
        let (a, b, c) =
            (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
        let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str| {
            let m = json!({ "diagram": d, "tool": tool, "source": from, "target": to,
                            "name": name });
            run(host, "connect", m)
        };
        send(&mut host, "message", &a["element"], &b["element"], "order");
        let check = send(&mut host, "message", &b["element"], &b["element"], "check");
        send(&mut host, "message", &b["element"], &c["element"], "pay");
        send(&mut host, "reply", &c["element"], &b["element"], "");
        send(&mut host, "message", &b["element"], &a["element"], "done");
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let y = |name: &str| {
            let lines = diagram["lines"].as_array().unwrap();
            lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
        };
        assert_eq!(diagram["lines"][1]["name"], "check");
        // b works in one bar from the call it gets to the call it makes last.
        let outer = diagram["bars"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|bar| bar["lifeline"] == b["element"])
            .map(|bar| (bar["top"].as_f64().unwrap(), bar["bottom"].as_f64().unwrap()))
            .max_by(|x, y| (x.1 - x.0).total_cmp(&(y.1 - y.0)))
            .unwrap();
        assert!((outer.0 - y("order")).abs() < 0.5, "{outer:?}");
        assert!(outer.1 > y("done"), "{outer:?}");

        // A loop around the call to self stays above the call that follows.
        let wrap = json!({ "diagram": d, "items": [check["element"]], "operator": "loop" });
        let lp = run(&mut host, "wrap_in_fragment", wrap);
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let frame = diagram["nodes"].as_array().unwrap().iter().find(|n| n["element"] == lp["id"]);
        let bounds = &frame.unwrap()["bounds"];
        let bottom = bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap();
        let pay = diagram["lines"].as_array().unwrap().iter().find(|l| l["name"] == "pay");
        assert!(bottom < pay.unwrap()["y"].as_f64().unwrap());

        // An alt around the next call stays beside the loop, not inside it.
        let pay = pay.unwrap()["element"].clone();
        let alt = run(
            &mut host,
            "wrap_in_fragment",
            json!({ "diagram": d, "items": [pay], "operator": "alt" }),
        );
        let model = host.document().model();
        let owner =
            |id: &Value| model.get(parse_id(id.as_str().unwrap()).unwrap()).and_then(|e| e.owner());
        assert_eq!(owner(&alt["id"]), owner(&lp["id"]));
        model.check().unwrap();
        // Its else pushes the call after it down instead of taking it in.
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let frame = diagram["nodes"].as_array().unwrap().iter().find(|n| n["element"] == alt["id"]);
        let bounds = &frame.unwrap()["bounds"];
        let bottom = bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap();
        let done = diagram["lines"].as_array().unwrap().iter().find(|l| l["name"] == "done");
        assert!(bottom < done.unwrap()["y"].as_f64().unwrap(), "{bottom}");
    }

    #[test]
    fn messages_move_between_operands() {
        let mut host = Headless::new();
        let uid = |v: &Value| parse_id(v.as_str().unwrap()).unwrap();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Pay" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
        };
        let (a, b, c) =
            (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
        let found = json!({ "diagram": d, "tool": "foundMessage", "source": a["element"] });
        let found = run(&mut host, "connect", found);
        let lost = json!({ "diagram": d, "tool": "lostMessage", "source": b["element"] });
        run(&mut host, "connect", lost);
        let gates: Vec<_> = host
            .document()
            .model()
            .elements()
            .filter_map(|e| match e.kind() {
                ElementKind::Message(p) => Some(p.gate),
                _ => None,
            })
            .collect();
        assert_eq!(gates, [umlcraft_model::MessageGate::Found, umlcraft_model::MessageGate::Lost]);
        let missing = json!({ "diagram": d, "tool": "message", "source": a["element"] });
        assert!(call(&mut host, &Access::default(), "connect", &missing).is_error);

        let send = |host: &mut Headless, from: &Value, to: &Value| {
            let m = json!({ "diagram": d, "tool": "message", "source": from, "target": to });
            run(host, "connect", m)
        };
        let first = send(&mut host, &a["element"], &b["element"]);
        let second = send(&mut host, &a["element"], &b["element"]);
        let wrap = json!({ "diagram": d, "items": [first["element"]], "operator": "alt" });
        let alt = run(&mut host, "wrap_in_fragment", wrap);

        // Covering c as well widens the frame to it.
        let frame = |host: &mut Headless| {
            let diagram = run(host, "get_diagram", json!({ "diagram": d }));
            let nodes = diagram["nodes"].as_array().unwrap().clone();
            nodes.into_iter().find(|n| n["element"] == alt["id"]).unwrap()
        };
        let narrow = frame(&mut host)["bounds"]["width"].as_f64().unwrap();
        let cover = json!({ "id": alt["id"],
                            "covered": [a["element"], b["element"], c["presentation"]] });
        run(&mut host, "update_element", cover);
        let wide = frame(&mut host);
        assert!(wide["bounds"]["width"].as_f64().unwrap() > narrow);
        let covered = match host.document().model().get(uid(&alt["id"])).map(|e| e.kind()) {
            Some(ElementKind::CombinedFragment(p)) => p.covered.clone(),
            _ => panic!("no fragment"),
        };
        assert_eq!(covered, [uid(&a["element"]), uid(&b["element"]), uid(&c["element"])]);

        // The second message into the else.
        let operands = wide["operands"].as_array().unwrap();
        assert_eq!(operands.len(), 2);
        let otherwise = &operands[1];
        let y = (otherwise["top"].as_f64().unwrap() + otherwise["bottom"].as_f64().unwrap()) / 2.0;
        run(&mut host, "move_node", json!({ "diagram": d, "node": second["element"], "y": y }));
        let owner = host.document().model().get(uid(&second["element"])).and_then(|e| e.owner());
        assert_eq!(owner, Some(uid(&otherwise["id"])));
        let _ = found;
        host.document().model().check().unwrap();
    }

    #[test]
    fn structograms_are_made_read_and_changed() {
        let mut host = Headless::new();
        let root = run(&mut host, "get_document", json!({}))["root"].clone();
        let class = json!({ "owner": root, "tool": "class", "name": "Cart" });
        let class = run(&mut host, "create_model_element", class);
        let op = run(
            &mut host,
            "add_member",
            json!({ "owner": class["id"], "text": "+ total(n : int) : int" }),
        );
        let tree = json!({ "title": "total(n)", "kind": "sub", "body": [
            { "kind": "instruction", "text": "s := 0" },
            { "kind": "for", "text": "for i := 1 to n", "body": [
                { "kind": "instruction", "text": "s := s + i" } ] },
            { "kind": "jump", "text": "return s" } ] });
        let made = run(
            &mut host,
            "create_structogram",
            json!({ "structogram": tree, "operation": op["id"] }),
        );
        assert_eq!(made["owner"], class["id"]);
        assert_eq!(made["operation"], op["id"]);
        let got = run(&mut host, "get_structogram", json!({ "id": made["id"] }));
        assert_eq!(got["structogram"]["body"], tree["body"]);
        let document = run(&mut host, "get_document", json!({}));
        assert_eq!(document["structograms"][0]["title"], "total(n)");

        let image = call(
            &mut host,
            &Access::default(),
            "render_diagram",
            &json!({ "diagram": made["id"] }),
        );
        assert!(matches!(&image.content[0], Content::Image(png) if png.starts_with(b"\x89PNG")));

        // A new body, the title and keywords kept; then no operation.
        let body = json!([{ "kind": "if", "text": "n > 0?", "then": [], "else": [] }]);
        let changed = run(
            &mut host,
            "update_structogram",
            json!({ "id": made["id"], "structogram": { "body": body } }),
        );
        assert_eq!(changed["structogram"]["title"], "total(n)");
        assert_eq!(changed["structogram"]["kind"], "sub");
        assert_eq!(changed["structogram"]["body"], body);
        let unlinked =
            run(&mut host, "update_structogram", json!({ "id": made["id"], "operation": "" }));
        assert_eq!(unlinked["operation"], Value::Null);
        run(&mut host, "undo", json!({}));
        run(&mut host, "undo", json!({}));
        let back = run(&mut host, "get_structogram", json!({ "id": made["id"] }));
        assert_eq!(back["structogram"]["body"], tree["body"]);
        assert_eq!(back["operation"], op["id"]);

        // One element by its path.
        let element = json!({ "kind": "jump", "text": "return 0" });
        let edit = json!({ "id": made["id"], "action": "insert", "path": "body.1.body.0",
                           "element": element });
        let edited = run(&mut host, "edit_structogram", edit);
        assert_eq!(edited["structogram"]["body"][1]["body"][0], element);
        let remove = json!({ "id": made["id"], "action": "remove", "path": "body.9" });
        assert!(call(&mut host, &Access::default(), "edit_structogram", &remove).is_error);

        // As image and as Structorizer file.
        let dir = std::env::temp_dir().join("umlcraft-mcp-structogram");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = crate::host::absolute(&dir);
        let access = Access { folders: vec![dir.clone()], any: false };
        for name in ["total.png", "total.svg", "total.nsd"] {
            let export = json!({ "diagram": made["id"], "path": dir.join(name) });
            let written = call(&mut host, &access, "export_image", &export);
            assert!(!written.is_error, "{written:?}");
        }
        let nsd = std::fs::read_to_string(dir.join("total.nsd")).unwrap();
        assert!(umlcraft_nsd::read(&nsd).unwrap().text() == ["total(n)"]);
        let txt = json!({ "diagram": made["id"], "path": dir.join("total.txt") });
        assert!(call(&mut host, &access, "export_image", &txt).is_error);

        let wrong = json!({ "id": class["id"], "structogram": { "body": [] } });
        assert!(call(&mut host, &Access::default(), "update_structogram", &wrong).is_error);
        let broken = json!({ "structogram": { "title": "x", "body": [{ "kind": "goto" }] } });
        assert!(call(&mut host, &Access::default(), "create_structogram", &broken).is_error);
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
