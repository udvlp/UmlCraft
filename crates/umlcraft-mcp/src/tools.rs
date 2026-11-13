//! The MCP tools: their descriptions and input schemas ([`list`]), and running one against a
//! [`Host`] ([`call`]). Ids are the model's UUIDs as strings. Every tool that changes the
//! project is one undo step, made as the app makes its edits.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use umlcraft_erm_sql::{Dialect, LeftOut};
use umlcraft_model::{
    ActivityNodeKind, DiagramKind, ElementData, ElementKind, ErAction, ErDisplay, ErKey,
    ErNotation, Id, InteractionOperator, InternalTransitionProps, MessageSort, Model, ModelError,
    Point, Rect, Shape, SharedStyle, StateInvariantProps, TerminationProps, Tx, Visibility,
};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Preview, TextMeasure};
use umlcraft_notation::{
    activity, autoroute, composite, edit, er, layout, member, sequence_edit, state_edit, svg,
};

use crate::base64;
use crate::host::{Access, Host, first_diagram, new_document};
use crate::structogram;
use crate::uml;

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

const DIAGRAM_KINDS: [(&str, DiagramKind); 9] = [
    ("class", DiagramKind::Class),
    ("use_case", DiagramKind::UseCase),
    ("sequence", DiagramKind::Sequence),
    ("activity", DiagramKind::Activity),
    ("state_machine", DiagramKind::StateMachine),
    ("component", DiagramKind::Component),
    ("deployment", DiagramKind::Deployment),
    ("composite_structure", DiagramKind::CompositeStructure),
    ("entity_relationship", DiagramKind::EntityRelationship),
];

fn diagram_key(kind: DiagramKind) -> &'static str {
    DIAGRAM_KINDS.iter().find(|(_, k)| *k == kind).map_or("class", |(key, _)| key)
}

/// The kind of the diagram `id` as the tools name it: an object diagram is a class diagram
/// whose palette starts with Object and Link.
fn diagram_key_of(model: &Model, id: Id) -> &'static str {
    match model.get(id).map(|e| e.kind()) {
        Some(ElementKind::Diagram(p)) if p.objects => "object",
        Some(ElementKind::Diagram(p)) => diagram_key(p.diagram),
        _ => "class",
    }
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
    let mut kinds: Vec<&str> = DIAGRAM_KINDS.iter().map(|(key, _)| *key).collect();
    kinds.insert(1, "object");
    // The settings of activity nodes, which both `create_element` and `update_element` take.
    let activity_node = json!({
        "object_type": text("Object nodes, pins, activity parameters: the type of the \
                             objects, a class name (\"\" for none)."),
        "in_state": text("Object nodes, pins, activity parameters: the state of the \
                          objects, without brackets."),
        "ordering": { "type": "string", "enum": ["", "unordered", "ordered", "LIFO", "FIFO"],
                      "description": "Object nodes, pins, activity parameters: the \
                                      order objects leave in, \"\" for none." },
        "upper_bound": text("Object nodes, pins, activity parameters: how many objects \
                             they hold at most, e.g. `3` or `*` (\"\" for none)."),
        "stream": { "type": "boolean",
                    "description": "Object nodes, pins, activity parameters: objects \
                                    stream while the action runs." },
        "control": { "type": "boolean",
                     "description": "Object nodes, pins, activity parameters: the \
                                     objects are control tokens (`{control}`; a \
                                     control pin)." },
        "exception": { "type": "boolean",
                       "description": "Object nodes, pins, activity parameters: the \
                                       objects are exceptions (an exception pin, \
                                       marked by a small triangle)." },
        "object_effect": { "type": "string",
                           "enum": ["", "create", "read", "update", "delete"],
                           "description": "Object nodes, pins, activity parameters: \
                                           what the action does to the objects \
                                           (`{create}`), \"\" for nothing said." },
        "calls": text("Call behavior actions: the id of the activity called or of its \
                       diagram, or \"\" for none."),
    });
    let with = |mut properties: Value, more: &Value| {
        if let (Some(p), Some(m)) = (properties.as_object_mut(), more.as_object()) {
            p.extend(m.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        properties
    };
    let flag = |description: &str| json!({ "type": "boolean", "description": description });
    // The ends of associations and links, which `connect` and `update_element` take.
    let end = |which: &str| {
        json!({ "type": "object",
                "description": format!("Associations and links: the {which} end. `role` as the \
                                        line shows it, with visibility and constraints \
                                        (`- items {{ordered}}`); `multiplicity` (`0..1`, `*`; \
                                        not for links); `navigability` (an arrow or a cross at \
                                        this end); `aggregation` (a diamond at this end, the \
                                        whole)."),
                "properties": {
                    "role": { "type": "string" },
                    "multiplicity": { "type": "string" },
                    "navigability": { "type": "string",
                                      "enum": ["", "navigable", "non_navigable"] },
                    "aggregation": { "type": "string", "enum": ["none", "shared", "composite"] },
                } })
    };
    let ends = json!({ "source_end": end("source"), "target_end": end("target") });
    // The settings of classes and objects, which `create_element` and `update_element` take.
    let classes = json!({
        "abstract": flag("Classes, interfaces, operations: abstract (the name in italics)."),
        "classifier": text("Objects: the class they are an instance of (`rex : Hund`), by id \
                            or name; \"\" for none."),
        "slots": { "type": "object", "additionalProperties": { "type": "string" },
                   "description": "Objects: values of the attributes of their class, by \
                                   attribute name, e.g. {\"name\": \"\\\"Rex\\\"\", \"alter\": \
                                   \"3\"}; \"\" clears one." },
        "empty_slots": flag("Objects: list the attributes they give no value as bare names; \
                             true by default, false shows only the slots with a value."),
    });
    // The settings of sequence diagrams, which `create_diagram` and `update_element` take.
    let sequence = json!({
        "message_numbers": flag("Sequence diagrams: number the messages (`1:`, `1.1:`); \
                                 true by default."),
        "flat_numbers": flag("Sequence diagrams: number the messages 1, 2, 3 from top to \
                              bottom instead of 1, 1.1, 1.2 by the call they belong to."),
        "return_values": flag("Sequence diagrams: show return values after the calls \
                               (`submit() : ok`); true by default."),
        "frame": flag("Sequence diagrams: draw the `sd` frame around the interaction; true \
                       by default."),
    });
    // The settings of ER elements, which `update_element` takes.
    let er = json!({
        "weak": flag("ER entities: weak, identified through an owner entity (double frame)."),
        "associative": flag("ER entities: associative, a relationship that is also an entity."),
        "key": { "type": "string", "enum": ["", "primary", "partial"],
                 "description": "ER attributes: key (underlined) or partial key of a weak \
                                 entity (dotted)." },
        "derived": flag("ER attributes: derived (dashed ellipse)."),
        "multivalued": flag("ER attributes: multivalued (double ellipse)."),
        "identifying": flag("ER relationships: identifying a weak entity (double rhombus)."),
        "total": flag("ER participations: every entity takes part (double line). IS-A: every \
                       instance of the supertype is one of the subtypes."),
        "disjoint": { "type": "string", "enum": ["", "disjoint", "overlapping"],
                      "description": "IS-A: whether the subtypes exclude each other (d, o)." },
        "cardinality": text("ER participations: how often the entity takes part, read \
                             look-across, e.g. [0,1], [1,n], [0,m]; \"\" for none."),
        "cardinality_here": text("ER participations of relationships of one or of three and \
                                  more entities: how often each instance of the entity takes \
                                  part, read at the entity, e.g. (1,n), as Chen diagrams with \
                                  `look_here` show it. Between two entities that is the other \
                                  participation's `cardinality`."),
        "look_here": flag("ER diagrams in Chen: show cardinalities read at their entity, \
                           (min,max) of how often each instance takes part, instead of \
                           look-across."),
        "role": text("ER participations: the entity's role, when it takes part twice."),
        "physical_name": text("ER entities and attributes: the name in the database."),
        "unique": flag("ER attributes: the column is UNIQUE."),
        "foreign_key": text("ER attributes: the id of the relationship or subtyping whose \
                             foreign key the column is (IE, IDEF1X: marked (FK)), or \"\"."),
        "discriminator": text("IS-A: the id of the supertype's attribute that tells the \
                               subtypes apart, or \"\"."),
        "notation": { "type": "string", "enum": ["chen", "ie", "idef1x"],
                      "description": "ER diagrams: how they are drawn. Switching converts what \
                                      the diagram shows and keeps its entities in place: IE \
                                      and IDEF1X list the attributes in the tables and draw \
                                      relationships between two entities as lines." },
        "er_display": { "type": "string", "enum": ["attributes", "keys", "name"],
                        "description": "ER diagrams in IE or IDEF1X: what the tables list." },
        "physical_names": flag("ER diagrams in IE or IDEF1X: the names in the database."),
        "datatype": text("ER attributes: the column's type, e.g. VARCHAR, or the name of an \
                          ER domain. ER domains: the type they are based on."),
        "length_precision": text("ER attributes and domains: length or precision, e.g. 10,2."),
        "not_null": flag("ER attributes and domains: NOT NULL."),
        "default_value": text("ER attributes and domains: the default."),
        "check": text("ER attributes and domains: a condition as SQL (`preis >= 0`; a domain's \
                       with VALUE), or \"\"."),
        "auto_increment": flag("ER attributes: the database numbers new rows (AUTO_INCREMENT, \
                                an identity column)."),
        "virtual": flag("ER attributes: a derived column whose expression (its default_value) \
                         is computed when read (VIRTUAL), not stored."),
        "on_delete": { "type": "string",
                       "enum": ["", "cascade", "set null", "set default", "restrict", "no action"],
                       "description": "ER relationships: what their foreign keys do when a \
                                       referenced row is deleted; \"\" leaves it to the \
                                       database." },
        "on_update": { "type": "string",
                       "enum": ["", "cascade", "set null", "set default", "restrict", "no action"],
                       "description": "ER relationships: what their foreign keys do when a \
                                       referenced key changes; \"\" leaves it to the \
                                       database." },
        "values": { "type": "array", "items": { "type": "string" },
                    "description": "ER domains: the values of an enumerated type." },
        "checks": { "type": "array", "items": { "type": "string" },
                    "description": "ER entities: conditions on the table's rows as SQL." },
        "indexes": { "type": "array",
                     "items": { "type": "object", "properties": {
                         "name": { "type": "string" },
                         "columns": { "type": "array", "items": { "type": "string" } },
                         "unique": { "type": "boolean" } } },
                     "description": "ER entities: the table's indexes by column names." },
    });
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
                "theme": { "type": "string", "enum": ["light", "dark", "system"] },
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
             shows the first. An empty list clears the selection. In a structogram `path` \
             selects one element, named as for `edit_structogram` (`body.1.then.0`), and the \
             structogram's own id in `ids` selects it as a whole; the properties panel then \
             edits it.",
            json!({
                "ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Elements or their presentations on the diagram.",
                },
                "path": text("In a structogram: the element to select, e.g. `body.1.then.0`."),
                "diagram": id("The diagram or structogram, shown first; the one shown if \
                               omitted."),
            }),
            &[],
        ),
        tool(
            "create_diagram",
            "Creates a diagram (sequence, activity and state machine diagrams inside a new \
             interaction, activity or state machine of the same name) and shows it. An \
             `object` diagram is a class diagram whose tools start with `object` and `link` \
             (every class diagram has them).",
            with(
                json!({
                    "kind": { "type": "string", "enum": kinds },
                    "name": text("Its name."),
                    "owner": id("Package (or class) to put it in; the model's root if omitted."),
                    "notation": { "type": "string", "enum": ["chen", "ie", "idef1x"],
                                  "description": "ER diagrams: Chen (by default), IE with \
                                                  crow's feet, or IDEF1X. IE and IDEF1X have \
                                                  identifying, non-identifying, many-to-many \
                                                  and subtype lines drawn from parent to \
                                                  child, which copy the parent's key into the \
                                                  child." },
                }),
                &sequence,
            ),
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
             first (`update_element` with the member `text`, e.g. `engine : Engine`). On \
             activity diagrams an `inputPin` or `outputPin` belongs to the action at or near \
             `x`, `y` (or `on`) and sits just outside its border; with `on` it goes where its \
             name and properties stay clear of the other pins' texts (the left side for input \
             pins, the right for output pins), the action growing if needed; an \
             `activityParameter` sits \
             on the diagram's frame, which is added when missing. A `callBehavior` action calls \
             the activity `update_element` gives it as `calls`; `activityConnector`s of the \
             same name continue a flow. On state machine diagrams a `submachineState` stands \
             for another state machine (the first there is, else a new one; `update_element` \
             changes it as `submachine`); an `entryPoint` or `exitPoint` sits on the border of \
             the state at or near `x`, `y` (or `on`), else on the diagram's frame; a \
             `stubState` goes into the submachine state there (or `on`). On sequence diagrams \
             a `stateInvariant` (its `name` is the condition, e.g. `x > 0`) or a `termination` \
             (the × that ends a lifeline) goes on the lifeline `on`, with its top at `y` or \
             below everything; a termination stays below what happens on its lifeline. \
             Duration and time constraints are drawn with `connect`.",
            with(
                json!({
                    "diagram": id("The diagram."),
                    "tool": text("The diagram tool."),
                    "name": text("Its name; a free default name if omitted."),
                    "x": number("Left edge."),
                    "y": number("Top edge."),
                    "on": id("Composite structure diagrams: the class (or part) a part goes into or \
                              a port goes on; activity diagrams: the action a pin goes on; state \
                              machine diagrams: the state an entry or exit point goes on, the \
                              submachine state a stub state goes into; when `x` and `y` are not \
                              given. Sequence diagrams: the lifeline a state invariant or \
                              termination goes on, with or without `y`."),
                }),
                &with(activity_node.clone(), &classes),
            ),
            &["diagram", "tool"],
        ),
        tool(
            "create_model_element",
            "Creates an element in the model tree only, without showing it, e.g. a class or \
             package under `owner`. `tool` is a diagram tool name as for `create_element`, or \
             `erDomain` for a named column type of ER diagrams (a domain or enumeration, set \
             with `update_element`), which columns take by its name as their datatype.",
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
             `update_element` gives it another `sort`. A `durationConstraint` runs from the \
             message `source` to the message `target` (`name` is the constraint without \
             braces, e.g. `d..3*d`); a `timeConstraint` goes on the one message `source` \
             (e.g. `t..t+3`).",
            json!({
                "diagram": id("The diagram."),
                "tool": text("The relationship tool."),
                "source": id("Element or presentation it starts at."),
                "target": id("Element or presentation it ends at; not for found and lost messages and time constraints."),
                "y": number("Sequence diagrams: the height of the message."),
                "name": text("Its name, e.g. a message's or an association's; a call's may be `name(arguments)`; a transition's label `trigger [guard] / effect`."),
                "allow_invalid": flag("Draw an association UML does not allow, between two \
                                       actors or two use cases, e.g. for an exercise in \
                                       finding mistakes."),
                "source_end": ends["source_end"],
                "target_end": ends["target_end"],
            }),
            &["diagram", "tool", "source"],
        ),
        tool(
            "add_member",
            "Adds an attribute, operation or enumeration literal to a classifier, written as \
             in a class box: `- name : String [0..1] = \"x\"`, `+ total(tax : double) : int`, \
             `RED`. Types name classes of the model or are kept as text. ER entities take \
             columns, `name` or `name : TYPE(length)`: rows of their tables in IE and IDEF1X. \
             States take their internal activities and transitions as listed in the state: \
             `entry / light on`, `do / blink`, `exit / light off` (each replaces the one \
             before), else an internal transition `trigger [guard] / effect`.",
            json!({
                "owner": id("The class, interface, enumeration or data type, the ER entity, or \
                             the state."),
                "text": text("The member as it reads in the box; `{static}` or \
                              `{abstract}` in it make it static or abstract."),
                "key": flag("ER columns: part of the primary key."),
                "static": flag("Attributes and operations: static (underlined)."),
                "abstract": flag("Operations: abstract (in italics)."),
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
             spans them); the guard of an operand, flow or transition; the type, state, \
             ordering, upper bound, stream, control, effect and exception of an object node, \
             pin or activity parameter (a control pin, an exception pin); \
             the activity a call behavior action calls; the state machine a submachine state \
             stands for; a state's entry, do and exit activities and the `trigger [guard] / \
             effect` of a transition or internal transition (as `text`); the constraints of an element (members show them after their text, \
             lines below their name); on every diagram showing it, the shared style of a \
             generalization (one triangle for all generalizations to its general element) \
             and whether a class «boundary», «control» or «entity» is drawn as its icon; \
             whether a sequence diagram shows message numbers, return values and its frame; \
             the ends of associations and links (role, multiplicity, navigability, \
             aggregation); abstract classes and operations, static attributes and operations; \
             an object's class and the values of its slots; the settings of ER entities, attributes, relationships, IS-As and participations \
             (weak, key, cardinality, …).",
            with(
                json!({
                    "id": id("The element."),
                    "name": text("New name."),
                    "text": text("Attributes and operations: the whole member as text; \
                                  transitions and internal transitions: `trigger [guard] / \
                                  effect`."),
                    "entry": text("States: the entry activity, without `entry /`; \"\" for none."),
                    "do": text("States: the do activity, without `do /`; \"\" for none."),
                    "exit": text("States: the exit activity, without `exit /`; \"\" for none."),
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
                    "submachine": text("Submachine states: the id of the state machine they stand \
                                        for, or of its diagram."),
                    "constraints": { "type": "array", "items": { "type": "string" },
                                     "description": "The element's constraints without braces, \
                                                     e.g. [\"age >= 18\"]; [] for none." },
                    "shared_style": { "type": "string",
                                      "enum": ["separate", "vertical", "horizontal"],
                                      "description": "Generalizations: share one triangle with \
                                                      the other generalizations to the same \
                                                      general element, joining on a horizontal \
                                                      (vertical style) or vertical line." },
                    "icon": { "type": "boolean",
                              "description": "Classes whose first stereotype is boundary, control \
                                              or entity: drawn as the robustness icon." },
                }),
                &with(
                    with(with(activity_node.clone(), &er), &sequence),
                    &with(
                        with(ends.clone(), &classes),
                        &json!({ "static": flag("Attributes and operations: static \
                                                 (underlined).") }),
                    ),
                ),
            ),
            &["id"],
        ),
        tool(
            "move_node",
            "Moves a node of a diagram, or gives it another size, as dragging does: what lies \
             in it (substates, the contents of a package or swimlane) moves along, and on \
             state machine diagrams a state moved into another one becomes its substate (out \
             of it, no longer). In a sequence diagram it also moves a message (to height `y`) or a fragment (its top to `y`) as dragging \
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
             others aside. Without a direction, state machines, use case and ER diagrams run \
             to the right and the others down.",
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
        tool(
            "export_sql",
            "Writes the entities of an ER diagram as CREATE TABLE statements: relationships \
             become foreign keys, many-to-many relationships tables of their own, subtypes take \
             their supertype's key. Returns the SQL, or writes it to `path` in an allowed \
             folder.",
            json!({
                "diagram": id("The ER diagram; the one shown if omitted."),
                "dialect": { "type": "string", "enum": ["mysql", "mariadb", "postgres"],
                             "description": "The SQL dialect; the one the diagram was \
                                             imported from if omitted, else mysql." },
                "path": text("The .sql file; the SQL comes back in the reply if omitted."),
            }),
            &[],
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
        "export_sql" => export_sql(host, access, args),
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

/// The tools `diagram` offers: its kind's, an object diagram's, or those of an ER notation.
fn tools_of(model: &Model, diagram: Id, kind: DiagramKind) -> Vec<Tool> {
    match model.get(diagram).map(|d| d.kind()) {
        Some(ElementKind::Diagram(p)) => palette::diagram_palette(p),
        _ => palette::palette(kind),
    }
}

/// The tool of `diagram`'s palette with `key`.
fn diagram_tool(model: &Model, diagram: Id, kind: DiagramKind, key: &str) -> Result<Tool, String> {
    let tools = tools_of(model, diagram, kind);
    tools.iter().copied().find(|t| t.key() == key).ok_or_else(|| {
        let keys: Vec<&str> = tools.iter().map(|t| t.key()).filter(|k| *k != "select").collect();
        format!("a {} diagram has no tool `{key}`; it has: {}", diagram_key(kind), keys.join(", "))
    })
}

fn node_tool(model: &Model, diagram: Id, kind: DiagramKind, key: &str) -> Result<NodeTool, String> {
    match diagram_tool(model, diagram, kind, key)? {
        Tool::Node(tool) => Ok(tool),
        _ => Err(format!("`{key}` draws no element; `connect` draws relationships")),
    }
}

fn edge_tool(model: &Model, diagram: Id, kind: DiagramKind, key: &str) -> Result<EdgeTool, String> {
    match diagram_tool(model, diagram, kind, key)? {
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

/// [`change`] for a change that measures text.
fn change_measured<R>(
    host: &mut dyn Host,
    tool: &str,
    f: impl FnOnce(&mut Tx<'_>, &dyn TextMeasure) -> umlcraft_model::Result<R>,
) -> Result<R, String> {
    let label = host.label(tool);
    let (doc, measure) = host.parts();
    edit::apply(doc, label, measure, |tx| f(tx, measure)).map_err(model_error)
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

/// A structogram as SVG, as the app shows and exports it.
fn structogram_svg(d: &umlcraft_nsd::Diagram) -> String {
    let m = umlcraft_nsd::Metrics::for_platform();
    let layout = umlcraft_nsd::layout::Layout::of(d, m);
    umlcraft_nsd::svg::render_with(d, &layout, m, &umlcraft_nsd::svg::Palette::PLAIN)
}

fn render_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let scale = opt_num(args, "scale").unwrap_or(1.0).clamp(0.5, 2.0);
    let shown = opt_id(args, "diagram")?.or(host.active_diagram());
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

// ----- Changing ------------------------------------------------------------------------------

fn create_diagram(host: &mut dyn Host, args: &Value) -> Outcome {
    let key = str_arg(args, "kind")?;
    // An object diagram is a class diagram whose palette starts with Object and Link.
    let objects = key == "object";
    let kind = DIAGRAM_KINDS
        .iter()
        .find(|(k, _)| *k == key || (objects && *k == "class"))
        .map(|(_, kind)| *kind)
        .ok_or_else(|| format!("no diagram kind `{key}`"))?;
    let name = str_arg(args, "name")?.to_owned();
    let owner = opt_id(args, "owner")?.unwrap_or(host.document().model().root());
    let notation = pick(args, "notation", &ER_NOTATIONS)?;
    if notation.is_some() && kind != DiagramKind::EntityRelationship {
        return Err("only ER diagrams take `notation`".into());
    }
    let mut settings = ElementKind::diagram(kind);
    set_sequence_diagram(&mut settings, args)?;
    let diagram = change_measured(host, "create_diagram", |tx, measure| {
        let diagram = if objects {
            edit::create_object_diagram(tx, owner, &name)?
        } else {
            edit::create_diagram(tx, owner, kind, &name)?
        };
        if let ElementKind::Diagram(wanted) = &settings {
            tx.update(diagram, |d| {
                if let ElementKind::Diagram(p) = &mut d.kind {
                    p.hide_message_numbers = wanted.hide_message_numbers;
                    p.flat_message_numbers = wanted.flat_message_numbers;
                    p.hide_return_values = wanted.hide_return_values;
                    p.hide_frame = wanted.hide_frame;
                }
            })?;
        }
        if let Some(notation) = notation {
            er::set_notation(tx, diagram, notation, measure)?;
        }
        Ok(diagram)
    })?;
    host.show_diagram(diagram);
    Ok(Reply::json(json!({ "diagram": diagram, "kind": key, "name": name })))
}

fn create_element(host: &mut dyn Host, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    let tool = node_tool(host.document().model(), diagram, kind, str_arg(args, "tool")?)?;
    let model = host.document().model();
    let on_lifeline = matches!(tool, NodeTool::StateInvariant | NodeTool::Termination);
    let origin = match (opt_num(args, "x"), opt_num(args, "y"), opt_id(args, "on")?) {
        // A state invariant or termination goes on the lifeline `on`, at `y` or below the rest.
        (_, y, Some(on)) if on_lifeline => {
            let view = model.view(diagram).ok_or("not a diagram")?;
            let lane = palette::lifeline_of(model, view, on_diagram(model, diagram, on)?);
            sequence_edit::spot_on_lifeline(model, diagram, lane, y)
                .map_err(|_| "`on` is not a lifeline")?
        }
        (_, _, None) if on_lifeline && opt_num(args, "x").is_none() => {
            return Err("`on` (the lifeline) is missing".into());
        }
        (Some(x), Some(y), _) => Point::new(x, y),
        (_, _, Some(on)) => {
            let on = on_diagram(model, diagram, on)?;
            composite::free_spot(model, diagram, on, tool).ok_or("`on` is not a node")?
        }
        _ => free_origin(model, diagram),
    };
    let name = opt_str(args, "name").map(str::to_owned);
    let base = name.clone().unwrap_or_else(|| default_base(tool));
    // Object node settings (`control`, `exception`, …) apply to the new node; wrong ones are
    // refused before anything is drawn.
    let settings = ACTIVITY_NODE_KEYS.iter().any(|k| args.get(k).is_some());
    if settings {
        set_activity_node(model, diagram, &mut tool.element_kind(), args)?;
    }
    let is_abstract = uml::flag(args, "abstract")?;
    let classifier = uml::classifier(model, args)?;
    let slots = uml::slots(args)?;
    let mut trial = tool.element_kind();
    let empty_slots = uml::empty_slots(&trial, args)?;
    uml::set_flags(&mut trial, None, is_abstract)?;
    if let Some(classifier) = classifier {
        uml::set_classifier(&mut trial, classifier)?;
    }
    // A pin put on an action looks for room once its texts are known.
    let place_pin = matches!(tool, NodeTool::InputPin | NodeTool::OutputPin)
        && opt_id(args, "on")?.is_some()
        && (opt_num(args, "x").is_none() || opt_num(args, "y").is_none());
    let (element, node) = change_measured(host, "create_element", |tx, measure| {
        let (element, node) = palette::create_node(tx, diagram, tool, origin, &base)?;
        if let Some(name) = &name
            && tool.is_named()
        {
            tx.rename(element, name.clone())?;
        }
        if settings {
            let mut kind = tx.model().element(element)?.kind().clone();
            set_activity_node(tx.model(), element, &mut kind, args)
                .map_err(umlcraft_model::ModelError::Invalid)?;
            tx.update(element, |data| data.kind = kind)?;
        }
        if is_abstract.is_some() || classifier.is_some() {
            // Checked above.
            tx.update(element, |data| {
                drop(uml::set_flags(&mut data.kind, None, is_abstract));
                if let Some(classifier) = classifier {
                    drop(uml::set_classifier(&mut data.kind, classifier));
                }
            })?;
        }
        if let Some(slots) = &slots {
            uml::set_slots(tx, element, slots)?;
        }
        if let Some(shown) = empty_slots {
            tx.update_presentation(diagram, node, |p| p.style.hide_empty_slots = !shown)?;
        }
        if place_pin {
            activity::place_pin(tx, diagram, node, measure)?;
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
    if key == "erDomain" {
        let model = host.document().model();
        let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
        let data = ElementData::new(name, ElementKind::ErDomain(Default::default()));
        if !owner_kind.can_own(&data.kind) {
            return Err(format!("a {} cannot own an erDomain", owner_kind.type_name()));
        }
        let element = change(host, "create_model_element", |tx| tx.create(owner, data))?;
        return Ok(Reply::json(summary(host.document().model(), element)));
    }
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
    let tool = edge_tool(host.document().model(), diagram, kind, str_arg(args, "tool")?)?;
    let name = opt_str(args, "name").map(str::to_owned);
    let anyway = args.get("allow_invalid").and_then(Value::as_bool).unwrap_or(false);
    let with_ends = uml::END_KEYS.iter().any(|k| args.get(k).is_some());
    if with_ends {
        let mut trial = match tool {
            EdgeTool::Link => ElementKind::link(Id::generate(), Id::generate()),
            EdgeTool::Association | EdgeTool::AssociationClass => {
                ElementKind::association(Id::generate(), Id::generate())
            }
            _ => return Err(format!("a {} has no ends to set", tool.key())),
        };
        uml::set_ends(&mut trial, args)?;
    }
    let model = host.document().model();
    let source = on_diagram(model, diagram, id_arg(args, "source")?)?;
    // Found and lost messages are drawn from their lifeline to itself.
    let target = match (tool, opt_id(args, "target")?) {
        (EdgeTool::FoundMessage | EdgeTool::LostMessage | EdgeTool::TimeConstraint, _) => source,
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
            // What a lifeline sends after it was called belongs to that call until it answers,
            // however far below the call it is drawn: a client gives the heights in the order
            // of the interaction, not to start new activations.
            let sends = !matches!(tool, EdgeTool::Reply | EdgeTool::FoundMessage);
            if let Some(slot) = slot.filter(|_| sends) {
                let model = tx.model();
                let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
                let lane = palette::lifeline_of(model, view, source);
                sequence_edit::continue_open_bar(tx, diagram, lane, slot.y)?;
            }
            palette::connect_message(tx, diagram, tool, source, target, slot)?
        } else if anyway {
            palette::connect_anyway(tx, diagram, tool, source, target)?
        } else {
            palette::connect(tx, diagram, tool, source, target)?
        };
        if tool != EdgeTool::NoteAnchor {
            autoroute::route_new_lines(tx, diagram, &[edge])?;
        }
        let element = tx.model().view(diagram).and_then(|v| v.get(edge)).and_then(|p| p.element);
        // A reply starts where its bar ends; one given a height goes there, as dragged, and
        // its bar goes on to it.
        if let (EdgeTool::Reply, Some(y), Some(reply)) = (tool, opt_num(args, "y"), element) {
            sequence_edit::move_item(tx, diagram, reply, y)?;
        }
        // An association class's name is its class's.
        let class = match element.and_then(|e| tx.model().get(e)).map(|e| e.kind()) {
            Some(ElementKind::Association(a)) if tool == EdgeTool::AssociationClass => a.class,
            _ => None,
        };
        let transition = element.filter(|&e| {
            matches!(tx.model().get(e).map(|e| e.kind()), Some(ElementKind::Transition(_)))
        });
        let call = element.filter(|&e| {
            matches!(tx.model().get(e).map(|e| e.kind()),
                Some(ElementKind::Message(p)) if p.sort != MessageSort::Reply)
        });
        if let (Some(name), Some(element)) = (&name, transition) {
            // A transition's name is its label: trigger, guard and effect, as UML has them.
            tx.update(element, |d| {
                if let ElementKind::Transition(p) = &mut d.kind {
                    p.set_label(name);
                }
            })?;
        } else if let (Some(name), Some(element)) = (&name, call) {
            set_call(tx, element, name)?;
        } else if let (Some(name), Some(element)) = (&name, class.or(element)) {
            tx.rename(element, name.clone())?;
        }
        if let Some(element) = element.filter(|_| with_ends) {
            // Checked above.
            tx.update(element, |d| drop(uml::set_ends(&mut d.kind, args)))?;
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

/// Names the call `message` as written: `rechne(a, 2)` is the name `rechne` with the arguments
/// `a, 2`, so that the parentheses are not drawn twice.
fn set_call(tx: &mut Tx<'_>, message: Id, text: &str) -> Result<(), ModelError> {
    let (name, arguments) = umlcraft_notation::sequence::split_call(text);
    tx.rename(message, name.to_owned())?;
    if let Some(arguments) = arguments {
        tx.update(message, |d| {
            if let ElementKind::Message(p) = &mut d.kind {
                p.arguments = arguments.to_owned();
            }
        })?;
    }
    Ok(())
}

fn add_member(host: &mut dyn Host, args: &Value) -> Outcome {
    let owner = id_arg(args, "owner")?;
    let text = str_arg(args, "text")?.trim().to_owned();
    let model = host.document().model();
    let owner_kind = model.get(owner).ok_or_else(|| format!("no element {owner}"))?.kind();
    let is_class_member = !matches!(
        owner_kind,
        ElementKind::State(_) | ElementKind::ErEntity(_) | ElementKind::ErRelationship(_)
    );
    let (text, written_static, written_abstract) =
        if is_class_member { uml::member_flags(&text) } else { (text, false, false) };
    let is_static = uml::flag(args, "static")?.or(written_static.then_some(true));
    let is_abstract = uml::flag(args, "abstract")?.or(written_abstract.then_some(true));
    if let ElementKind::State(props) = owner_kind {
        let props = props.clone();
        return add_state_member(host, owner, props, &text);
    }
    let literal = matches!(owner_kind, ElementKind::Enumeration(_))
        && !text.contains('(')
        && !text.contains(':');
    let column = matches!(owner_kind, ElementKind::ErEntity(_) | ElementKind::ErRelationship(_));
    let data = if column {
        let key = args.get("key").and_then(Value::as_bool).unwrap_or(false);
        let (name, props) = er::parse_column(&text, key);
        ElementData::new(name, ElementKind::ErAttribute(props))
    } else if literal {
        ElementData::new(text.clone(), ElementKind::EnumerationLiteral)
    } else if text.contains('(') {
        ElementData::new("", ElementKind::operation(None))
    } else {
        ElementData::new("", ElementKind::attribute(None)).with_visibility(Visibility::Private)
    };
    if !owner_kind.can_own(&data.kind) {
        return Err(format!("a {} cannot own a {}", owner_kind.type_name(), data.kind.type_name()));
    }
    uml::set_flags(&mut data.kind.clone(), is_static, is_abstract)?;
    let member = change(host, "add_member", |tx| {
        let member = tx.create(owner, data)?;
        if !literal && !column {
            member::apply_member_text(tx, member, &text)?;
        }
        if is_static.is_some() || is_abstract.is_some() {
            // Checked above.
            tx.update(member, |d| drop(uml::set_flags(&mut d.kind, is_static, is_abstract)))?;
        }
        Ok(member)
    })?;
    let model = host.document().model();
    let shown = if literal || column { text } else { member::member_text(model, member) };
    Ok(Reply::json(json!({
        "id": member,
        "kind": kind_name(model, member),
        "text": shown,
    })))
}

/// `entry / …`, `do / …` or `exit / …` sets that activity of `state`; any other text adds an
/// internal transition `trigger [guard] / effect`.
fn add_state_member(
    host: &mut dyn Host,
    state: Id,
    mut props: umlcraft_model::StateProps,
    text: &str,
) -> Outcome {
    if let Some((label, activity)) = state_activity(text) {
        let activity = activity.to_owned();
        match label {
            "entry" => props.entry = activity,
            "do" => props.do_activity = activity,
            _ => props.exit = activity,
        }
        change(host, "add_member", |tx| tx.update(state, |d| d.kind = ElementKind::State(props)))?;
        return Ok(Reply::json(json!({ "id": state, "kind": "state", "text": text })));
    }
    if text.is_empty() {
        return Err("an internal transition needs a trigger, a guard or an effect".into());
    }
    let props = InternalTransitionProps::parse(text);
    let member = change(host, "add_member", |tx| {
        tx.create(state, ElementData::new("", ElementKind::InternalTransition(props)))
    })?;
    let model = host.document().model();
    let shown = match model.get(member).map(|e| e.kind()) {
        Some(ElementKind::InternalTransition(t)) => t.label(),
        _ => String::new(),
    };
    Ok(Reply::json(json!({
        "id": member,
        "kind": kind_name(model, member),
        "text": shown,
    })))
}

/// `entry / light on` as (`entry`, `light on`); likewise `do` and `exit`, in any case.
fn state_activity(text: &str) -> Option<(&'static str, &str)> {
    let (head, rest) = text.split_once('/')?;
    let label =
        ["entry", "do", "exit"].into_iter().find(|l| head.trim().eq_ignore_ascii_case(l))?;
    Some((label, rest.trim()))
}

/// The `entry`, `do` and `exit` arguments of a state.
fn set_state_activities(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    for key in ["entry", "do", "exit"] {
        let Some(text) = opt_str(args, key) else { continue };
        let ElementKind::State(p) = kind else {
            return Err(format!("a {} has no `{key}`", kind.type_name()));
        };
        let text = text.trim().to_owned();
        match key {
            "entry" => p.entry = text,
            "do" => p.do_activity = text,
            _ => p.exit = text,
        }
    }
    Ok(())
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
    let transition =
        matches!(kind, ElementKind::InternalTransition(_) | ElementKind::Transition(_));
    if text.is_some()
        && !transition
        && !matches!(kind, ElementKind::Attribute(_) | ElementKind::Operation(_))
    {
        return Err(format!(
            "only attributes, operations and transitions take `text`, not a {}",
            kind.type_name()
        ));
    }
    let constraints: Option<Vec<String>> =
        args.get("constraints").and_then(Value::as_array).map(|c| {
            c.iter()
                .filter_map(Value::as_str)
                .map(|c| c.trim().trim_start_matches('{').trim_end_matches('}').trim().to_owned())
                .filter(|c| !c.is_empty())
                .collect()
        });
    let shared = pick(args, "shared_style", &SHARED_STYLES)?;
    if shared.is_some() && !matches!(kind, ElementKind::Generalization(_)) {
        return Err(format!("a {} has no `shared_style`", kind.type_name()));
    }
    let icon = args.get("icon").and_then(Value::as_bool);
    let robustness = model.get(id).and_then(scene::robustness_icon).is_some();
    if icon.is_some() && !robustness {
        return Err("only classes whose first stereotype is boundary, control or entity take \
                    `icon`"
            .into());
    }
    let shown = model.shown_on(id);
    let mut settings = kind.clone();
    set_properties(model, &mut settings, args)?;
    set_sequence_diagram(&mut settings, args)?;
    uml::set_ends(&mut settings, args)?;
    if let Some(classifier) = uml::classifier(model, args)? {
        uml::set_classifier(&mut settings, classifier)?;
    }
    let slots = uml::slots(args)?;
    let empty_slots = uml::empty_slots(&kind, args)?;
    // `{static}` and `{abstract}` in a member's text, or given as flags. Set after the text,
    // which changes the member on its own.
    let member = matches!(kind, ElementKind::Attribute(_) | ElementKind::Operation(_));
    let (text, written_static, written_abstract) = match text {
        Some(text) if member => {
            let (text, s, a) = uml::member_flags(&text);
            (Some(text), s, a)
        }
        text => (text, false, false),
    };
    let is_static = uml::flag(args, "static")?.or(written_static.then_some(true));
    let is_abstract = uml::flag(args, "abstract")?.or(written_abstract.then_some(true));
    let flags_later = member && text.is_some();
    if flags_later {
        uml::set_flags(&mut kind.clone(), is_static, is_abstract)?;
    } else {
        uml::set_flags(&mut settings, is_static, is_abstract)?;
    }
    set_activity_node(model, id, &mut settings, args)?;
    set_submachine(model, id, &mut settings, args)?;
    set_state_activities(&mut settings, args)?;
    if let Some(text) = &text {
        match &mut settings {
            ElementKind::InternalTransition(p) => *p = InternalTransitionProps::parse(text),
            ElementKind::Transition(p) => p.set_label(text),
            _ => {}
        }
    }
    set_er(&mut settings, args)?;
    // A call named `rechne(a)` is named `rechne` with the arguments `a`, unless they are given.
    let name = match (&mut settings, name) {
        (ElementKind::Message(p), Some(name)) if p.sort != MessageSort::Reply => {
            let (call, arguments) = umlcraft_notation::sequence::split_call(&name);
            if let Some(arguments) = arguments.filter(|_| args.get("arguments").is_none()) {
                p.arguments = arguments.to_owned();
            }
            Some(call.to_owned())
        }
        (_, name) => name,
    };
    if let ElementKind::ErParticipation(p) = &kind
        && args.get("cardinality_here").is_some()
        && umlcraft_notation::er::participations(model, p.relationship).count() == 2
    {
        return Err("between two entities, the cardinality read at one is the other \
                    participation's `cardinality`; set that instead"
            .into());
    }
    let notation = pick(args, "notation", &ER_NOTATIONS)?;
    let is_er_diagram =
        matches!(&kind, ElementKind::Diagram(p) if p.diagram == DiagramKind::EntityRelationship);
    if notation.is_some() && !is_er_diagram {
        return Err(format!("a {} has no `notation`", kind.type_name()));
    }
    let identifying = match &settings {
        ElementKind::ErRelationship(p) if args.get("identifying").is_some() => Some(p.identifying),
        _ => None,
    };
    let associative = match &settings {
        ElementKind::ErEntity(p) if args.get("associative").is_some() => Some(p.associative),
        _ => None,
    };
    let is_alt = |kind: &ElementKind| matches!(kind, ElementKind::CombinedFragment(p) if p.operator == InteractionOperator::Alt);
    let becomes_alt = is_alt(&settings) && !is_alt(&kind);
    let recovered = args.get("covered").is_some();
    change_measured(host, "update_element", |tx, measure| {
        // Refused while entities take part in the associative entity.
        if let Some(associative) = associative {
            er::set_associative(tx, id, associative)?;
        }
        if let Some(text) = text.as_ref().filter(|_| !transition) {
            member::apply_member_text(tx, id, text)?;
        }
        if let Some(name) = name {
            tx.rename(id, name)?;
        }
        if documentation.is_some()
            || stereotypes.is_some()
            || constraints.is_some()
            || settings != kind
        {
            tx.update(id, |data| {
                if let Some(documentation) = documentation {
                    data.documentation = documentation;
                }
                if let Some(stereotypes) = stereotypes {
                    data.stereotypes = stereotypes;
                }
                if let Some(constraints) = constraints {
                    data.constraints = constraints;
                }
                data.kind = settings;
            })?;
        }
        if flags_later && (is_static.is_some() || is_abstract.is_some()) {
            // Checked above.
            tx.update(id, |d| drop(uml::set_flags(&mut d.kind, is_static, is_abstract)))?;
        }
        if let Some(slots) = &slots {
            uml::set_slots(tx, id, slots)?;
        }
        for &(diagram, presentation) in &shown {
            if let Some(style) = shared {
                umlcraft_notation::shared::set_style(tx, diagram, presentation, style)?;
            }
            if let Some(icon) = icon {
                tx.update_presentation(diagram, presentation, |p| p.style.iconic = icon)?;
            }
            if let Some(shown) = empty_slots {
                tx.update_presentation(diagram, presentation, |p| {
                    p.style.hide_empty_slots = !shown;
                })?;
            }
        }
        // A fragment turned into an alt gets its else, as in the app.
        if becomes_alt {
            palette::add_else(tx, id)?;
        }
        // Frames span the lifelines they cover once these are given.
        if recovered {
            sequence_edit::cover(tx, id)?;
        }
        // Foreign key columns move into or out of the child's key.
        if let Some(identifying) = identifying {
            er::set_identifying(tx, id, identifying)?;
        }
        if let Some(notation) = notation {
            er::set_notation(tx, id, notation, measure)?;
        }
        sequence_edit::tidy_all(tx)
    })?;
    Ok(Reply::json(summary(host.document().model(), id)))
}

const SHARED_STYLES: [(&str, SharedStyle); 3] = [
    ("separate", SharedStyle::Separate),
    ("vertical", SharedStyle::Vertical),
    ("horizontal", SharedStyle::Horizontal),
];

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

const ER_NOTATIONS: [(&str, ErNotation); 3] =
    [("chen", ErNotation::Chen), ("ie", ErNotation::Ie), ("idef1x", ErNotation::Idef1x)];

/// The settings of the elements of ER diagrams `update_element` takes; `notation` is set on
/// its own, as switching converts the diagram.
const ER_KEYS: [&str; 30] = [
    "unique",
    "foreign_key",
    "discriminator",
    "er_display",
    "physical_names",
    "weak",
    "associative",
    "key",
    "derived",
    "multivalued",
    "identifying",
    "total",
    "disjoint",
    "cardinality",
    "cardinality_here",
    "look_here",
    "role",
    "physical_name",
    "datatype",
    "length_precision",
    "not_null",
    "default_value",
    "check",
    "values",
    "checks",
    "indexes",
    "auto_increment",
    "virtual",
    "on_delete",
    "on_update",
];

/// Writes the ER settings of `args` into `kind`, an entity, attribute, relationship, IS-A or
/// participation.
fn set_er(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    let type_name = kind.type_name();
    for key in ER_KEYS.into_iter().filter(|k| args.get(k).is_some()) {
        let value = &args[key];
        let flag = || value.as_bool().ok_or_else(|| format!("`{key}` is true or false"));
        let text = || {
            value.as_str().map(|s| s.trim().to_owned()).ok_or_else(|| format!("`{key}` is text"))
        };
        let strings = || -> Result<Vec<String>, String> {
            serde_json::from_value(value.clone()).map_err(|_| format!("`{key}` is a list of texts"))
        };
        // A referential action, or "" for none.
        let action = || -> Result<Option<ErAction>, String> {
            let text = text()?;
            if text.is_empty() {
                return Ok(None);
            }
            text.parse().map(Some).map_err(|_| {
                format!(
                    "`{key}` is one of cascade, set null, set default, restrict, no action, \"\", \
                     not `{text}`"
                )
            })
        };
        // An id, or "" for none.
        let reference = || -> Result<Option<Id>, String> {
            let text = text()?;
            if text.is_empty() { Ok(None) } else { parse_id(&text).map(Some) }
        };
        match (key, &mut *kind) {
            ("unique", ElementKind::ErAttribute(p)) => p.unique = flag()?,
            ("foreign_key", ElementKind::ErAttribute(p)) => p.foreign_key = reference()?,
            ("discriminator", ElementKind::ErIsA(p)) => p.discriminator = reference()?,
            ("er_display", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                p.er_display = match text()?.as_str() {
                    "attributes" => ErDisplay::Attributes,
                    "keys" => ErDisplay::Keys,
                    "name" => ErDisplay::Name,
                    other => {
                        return Err(format!(
                            "`er_display` is one of attributes, keys, name, not `{other}`"
                        ));
                    }
                }
            }
            ("physical_names", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                p.physical_names = flag()?
            }
            ("weak", ElementKind::ErEntity(p)) => p.weak = flag()?,
            ("associative", ElementKind::ErEntity(p)) => p.associative = flag()?,
            ("physical_name", ElementKind::ErEntity(p)) => p.physical_name = text()?,
            ("key", ElementKind::ErAttribute(p)) => {
                p.key = match text()?.as_str() {
                    "" => None,
                    "primary" => Some(ErKey::Primary),
                    "partial" => Some(ErKey::Partial),
                    other => {
                        return Err(format!(
                            "`key` is one of primary, partial, \"\", not `{other}`"
                        ));
                    }
                }
            }
            ("derived", ElementKind::ErAttribute(p)) => p.derived = flag()?,
            ("multivalued", ElementKind::ErAttribute(p)) => p.multivalued = flag()?,
            ("physical_name", ElementKind::ErAttribute(p)) => p.physical_name = text()?,
            ("datatype", ElementKind::ErAttribute(p)) => p.datatype = text()?,
            ("length_precision", ElementKind::ErAttribute(p)) => p.length_precision = text()?,
            ("not_null", ElementKind::ErAttribute(p)) => p.not_null = flag()?,
            ("default_value", ElementKind::ErAttribute(p)) => p.default_value = text()?,
            ("check", ElementKind::ErAttribute(p)) => p.check = text()?,
            ("auto_increment", ElementKind::ErAttribute(p)) => p.auto_increment = flag()?,
            ("virtual", ElementKind::ErAttribute(p)) => p.virtual_column = flag()?,
            ("datatype", ElementKind::ErDomain(p)) => p.datatype = text()?,
            ("length_precision", ElementKind::ErDomain(p)) => p.length_precision = text()?,
            ("not_null", ElementKind::ErDomain(p)) => p.not_null = flag()?,
            ("default_value", ElementKind::ErDomain(p)) => p.default_value = text()?,
            ("check", ElementKind::ErDomain(p)) => p.check = text()?,
            ("values", ElementKind::ErDomain(p)) => p.values = strings()?,
            ("checks", ElementKind::ErEntity(p)) => p.checks = strings()?,
            ("indexes", ElementKind::ErEntity(p)) => {
                p.indexes = serde_json::from_value(value.clone())
                    .map_err(|_| "`indexes` is a list of { name, columns, unique }".to_owned())?;
            }
            ("identifying", ElementKind::ErRelationship(p)) => p.identifying = flag()?,
            ("on_delete", ElementKind::ErRelationship(p)) => p.on_delete = action()?,
            ("on_update", ElementKind::ErRelationship(p)) => p.on_update = action()?,
            ("total", ElementKind::ErIsA(p)) => p.total = flag()?,
            ("disjoint", ElementKind::ErIsA(p)) => {
                p.disjoint = match text()?.as_str() {
                    "" => None,
                    "disjoint" => Some(true),
                    "overlapping" => Some(false),
                    other => {
                        return Err(format!(
                            "`disjoint` is one of disjoint, overlapping, \"\", not `{other}`"
                        ));
                    }
                }
            }
            ("total", ElementKind::ErParticipation(p)) => p.total = flag()?,
            ("role", ElementKind::ErParticipation(p)) => p.role = text()?,
            ("cardinality" | "cardinality_here", ElementKind::ErParticipation(p)) => {
                let text = text()?;
                let c = if text.is_empty() {
                    None
                } else {
                    Some(text.parse().map_err(|e: umlcraft_model::ParseCardinalityError| {
                        format!("`{}` is no cardinality, e.g. [0,1], [1,n], 0..*", e.0)
                    })?)
                };
                if key == "cardinality" {
                    p.cardinality = c;
                } else {
                    p.look_here = c;
                }
            }
            ("look_here", ElementKind::Diagram(p))
                if p.diagram == DiagramKind::EntityRelationship =>
            {
                p.look_here = flag()?
            }
            _ => return Err(format!("a {type_name} has no `{key}`")),
        }
    }
    Ok(())
}

/// The settings of activity nodes `update_element` and `create_element` take.
const ACTIVITY_NODE_KEYS: [&str; 9] = [
    "object_type",
    "in_state",
    "ordering",
    "upper_bound",
    "stream",
    "control",
    "exception",
    "object_effect",
    "calls",
];

/// Writes the object node settings and the activity called of `args` into the activity node
/// `kind`.
fn set_activity_node(
    model: &Model,
    id: Id,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let Some(key) = ACTIVITY_NODE_KEYS.into_iter().find(|k| args.get(k).is_some()) else {
        return Ok(());
    };
    let type_name = kind.type_name();
    let ElementKind::ActivityNode(p) = kind else {
        return Err(format!("a {type_name} has no `{key}`"));
    };
    let object = p.node.is_object();
    let only_objects =
        |key: &str| format!("only object nodes, pins and activity parameters have `{key}`");
    if let Some(text) = opt_str(args, "object_type") {
        if !object {
            return Err(only_objects("object_type"));
        }
        let text = text.trim();
        // Names resolve as in a declaration, from the node outward.
        p.type_ref = (!text.is_empty()).then(|| member::parse_type(model, id, text));
    }
    if let Some(state) = opt_str(args, "in_state") {
        if !object {
            return Err(only_objects("in_state"));
        }
        let state = state.trim();
        let state = state.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(state);
        p.in_state = state.trim().to_owned();
    }
    if let Some(ordering) = opt_str(args, "ordering") {
        if !object {
            return Err(only_objects("ordering"));
        }
        p.ordering = match ordering.trim() {
            "" => None,
            o => Some(umlcraft_model::ObjectOrdering::parse(o).ok_or_else(|| {
                format!("`ordering` is one of unordered, ordered, LIFO, FIFO, not `{o}`")
            })?),
        };
    }
    if let Some(bound) = opt_str(args, "upper_bound") {
        if !object {
            return Err(only_objects("upper_bound"));
        }
        p.upper_bound = bound.trim().to_owned();
    }
    if let Some(stream) = args.get("stream") {
        if !object {
            return Err(only_objects("stream"));
        }
        p.is_stream = stream.as_bool().ok_or("`stream` is true or false")?;
    }
    if let Some(control) = args.get("control") {
        if !object {
            return Err(only_objects("control"));
        }
        p.is_control = control.as_bool().ok_or("`control` is true or false")?;
    }
    if let Some(exception) = args.get("exception") {
        if !object {
            return Err(only_objects("exception"));
        }
        p.is_exception = exception.as_bool().ok_or("`exception` is true or false")?;
    }
    if let Some(effect) = opt_str(args, "object_effect") {
        if !object {
            return Err(only_objects("object_effect"));
        }
        p.effect = match effect.trim() {
            "" => None,
            e => Some(umlcraft_model::ObjectEffect::parse(e).ok_or_else(|| {
                format!("`object_effect` is one of create, read, update, delete, not `{e}`")
            })?),
        };
    }
    if args.get("calls").is_some() {
        if p.node != ActivityNodeKind::CallBehavior {
            return Err("only call behavior actions have `calls`".into());
        }
        // The activity is given as itself or as one of its diagrams.
        let calls = opt_str(args, "calls").map(str::trim).filter(|c| !c.is_empty());
        p.behavior = match calls.map(parse_id).transpose()? {
            Some(id) => match model.get(id).map(|e| e.kind()) {
                Some(ElementKind::Activity) => Some(id),
                Some(k) if k.as_diagram() == Some(DiagramKind::Activity) => Some(
                    model.get(id).and_then(|d| d.owner()).ok_or("the diagram has no activity")?,
                ),
                _ => return Err(format!("{id} is no activity")),
            },
            None => None,
        };
    }
    Ok(())
}

/// Writes the state machine of `args`' `submachine` into the submachine state `kind`. The
/// machine is given as itself or as one of its diagrams.
fn set_submachine(
    model: &Model,
    id: Id,
    kind: &mut ElementKind,
    args: &Value,
) -> Result<(), String> {
    let Some(text) = opt_str(args, "submachine") else { return Ok(()) };
    let ElementKind::State(p) = kind else {
        return Err(format!("a {} has no `submachine`", kind.type_name()));
    };
    if p.submachine.is_none() {
        return Err("only submachine states have `submachine`".into());
    }
    let given = parse_id(text.trim())?;
    let machine = match model.get(given).map(|e| e.kind()) {
        Some(ElementKind::StateMachine) => given,
        Some(k) if k.as_diagram() == Some(DiagramKind::StateMachine) => {
            model.get(given).and_then(|d| d.owner()).ok_or("the diagram has no state machine")?
        }
        _ => return Err(format!("{given} is no state machine")),
    };
    if model.ancestors(id).contains(&machine) {
        return Err("a submachine state cannot stand for the state machine it is part of".into());
    }
    p.submachine = Some(machine);
    Ok(())
}

/// Writes the settings of `args` that belong to the element's kind into `kind`: a guard, a
/// message's sort, arguments, return value and operation, a fragment's operator.
/// The settings of a sequence diagram in `args`: what it numbers, shows and frames.
fn set_sequence_diagram(kind: &mut ElementKind, args: &Value) -> Result<(), String> {
    let keys = ["message_numbers", "flat_numbers", "return_values", "frame"];
    let Some(key) = keys.into_iter().find(|k| args.get(k).is_some()) else { return Ok(()) };
    let p = match kind {
        ElementKind::Diagram(p) if p.diagram == DiagramKind::Sequence => p,
        _ => return Err(format!("only sequence diagrams take `{key}`")),
    };
    let flag = |key: &str| -> Result<Option<bool>, String> {
        match args.get(key) {
            None => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("`{key}` is true or false")),
        }
    };
    if let Some(on) = flag("message_numbers")? {
        p.hide_message_numbers = !on;
    }
    if let Some(on) = flag("flat_numbers")? {
        p.flat_message_numbers = on;
    }
    if let Some(on) = flag("return_values")? {
        p.hide_return_values = !on;
    }
    if let Some(on) = flag("frame")? {
        p.hide_frame = !on;
    }
    Ok(())
}

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
    let (dx, dy) = (x - bounds.x, y - bounds.y);
    let model = host.document().model();
    let sequence = is_sequence(model, diagram);
    // As when dragged: what lies in a container (substates, the nodes of a package or swimlane)
    // moves along, and so do the bends of lines between what moves.
    let contents: Vec<Id> = if sequence || (dx == 0.0 && dy == 0.0) {
        Vec::new()
    } else {
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), diagram, &Preview::default(), measure);
        scene::with_contents(doc.model(), &scene, &[node])
            .into_iter()
            .filter(|&p| p != node)
            .collect()
    };
    let view = host.document().model().view(diagram);
    let nodes: Vec<(Id, Rect)> = std::iter::once((node, Rect::new(x, y, width, height)))
        .chain(
            contents.iter().filter_map(|&p| Some((p, view?.get(p)?.bounds()?.translate(dx, dy)))),
        )
        .collect();
    change(host, "move_node", |tx| {
        if let (Some(item), Some(slot)) = (item, slot) {
            palette::move_to_slot(tx, item, slot)?;
        }
        // Lines between what moves keep their shape; a state moved into another one (or out of
        // it) becomes its substate.
        edit::set_node_bounds(tx, diagram, &nodes)
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
    let target = match opt_id(args, "diagram")? {
        Some(id) => Some(id),
        None => host.active_diagram(),
    };
    if let Some(id) = target
        && structogram_of(host.document().model(), id).is_ok()
    {
        return select_in_structogram(host, id, args);
    }
    if opt_str(args, "path").is_some() {
        return Err("`path` selects in a structogram; give one as `diagram`, or show one".into());
    }
    let (diagram, _) = diagram_arg(host, args, "diagram")?;
    let ids = ids_arg(args, "ids")?.ok_or("`ids` is missing")?;
    let model = host.document().model();
    let presentations =
        ids.into_iter().map(|id| on_diagram(model, diagram, id)).collect::<Result<Vec<_>, _>>()?;
    host.show_diagram(diagram);
    host.select(presentations.clone());
    Ok(Reply::json(json!({ "diagram": diagram, "selection": presentations })))
}

/// `select` in a structogram: the element at `path`, the structogram itself (its id in `ids`)
/// or nothing (no `path`, no `ids`).
fn select_in_structogram(host: &mut dyn Host, id: Id, args: &Value) -> Outcome {
    let path = opt_str(args, "path");
    let ids = ids_arg(args, "ids")?.unwrap_or_default();
    let d = &structogram_of(host.document().model(), id)?.structogram;
    let selection = match (path, ids.as_slice()) {
        (Some(path), []) => umlcraft_nsd::Selection::element(&structogram::locate(d, path)?),
        (None, []) => umlcraft_nsd::Selection::Nothing,
        (None, [only]) if *only == id => umlcraft_nsd::Selection::Root,
        _ => {
            return Err("In a structogram `path` names the element to select (`body.1.then.0`), \
                        the structogram's own id in `ids` selects it as a whole, and no `path` \
                        and an empty `ids` clear the selection."
                .into());
        }
    };
    let whole = selection == umlcraft_nsd::Selection::Root;
    host.show_diagram(id);
    host.select_in_structogram(id, selection);
    Ok(Reply::json(json!({ "structogram": id, "path": path, "whole": whole })))
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
            let svg = structogram_svg(&p.structogram);
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

const DIALECTS: [(&str, Dialect); 3] =
    [("mysql", Dialect::MySql), ("mariadb", Dialect::MariaDb), ("postgres", Dialect::Postgres)];

fn export_sql(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let (diagram, kind) = diagram_arg(host, args, "diagram")?;
    if kind != DiagramKind::EntityRelationship {
        return Err(format!("{diagram} is not an ER diagram"));
    }
    let model = host.document().model();
    let dialect = pick(args, "dialect", &DIALECTS)?
        .or_else(|| umlcraft_erm_sql::imported_dialect(model, diagram))
        .unwrap_or(Dialect::MySql);
    let out = umlcraft_erm_sql::ddl(model, diagram, dialect).map_err(|e| e.to_string())?;
    let left_out: Vec<String> = out
        .left_out
        .iter()
        .map(|l| match l {
            LeftOut::Multivalued { table, attribute } => {
                format!("{table}.{attribute}: multivalued, no column")
            }
            LeftOut::NoKey { table, target } => {
                format!("{table} → {target}: {target} has no primary key, no foreign key")
            }
            LeftOut::Check { table, check } => {
                format!("{table}: CHECK ({check}) cannot be written in this dialect")
            }
            LeftOut::Generated { table, column } => {
                format!("{table}.{column}: generated in the other dialect, a plain column here")
            }
            LeftOut::AutoIncrement { table, column } => format!(
                "{table}.{column}: MySQL only counts a column that leads the key or is unique, \
                 no AUTO_INCREMENT"
            ),
        })
        .collect();
    let mut reply = json!({
        "dialect": dialect.name(),
        "tables": out.tables,
        "foreign_keys": out.foreign_keys,
        "left_out": left_out,
    });
    match opt_str(args, "path") {
        Some(path) => {
            let path = access.check(Path::new(path), None)?;
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)
                    .map_err(|e| format!("{}: {e}", folder.display()))?;
            }
            std::fs::write(&path, &out.sql).map_err(|e| format!("{}: {e}", path.display()))?;
            reply["written"] = json!(path);
        }
        None => reply["sql"] = json!(out.sql),
    }
    Ok(Reply::json(reply))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Headless;
    use umlcraft_model::PseudostateKind;

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
    fn an_ie_diagram_copies_keys_and_switches_notation() {
        let mut host = Headless::new();
        let d = run(
            &mut host,
            "create_diagram",
            json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
        );
        let d = id(&d["diagram"]);
        let shown = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let tools = shown.to_string();
        assert!(tools.contains("erIdentifying") && !tools.contains("erLine"), "{tools}");
        let entity = |host: &mut Headless, name: &str| {
            let e = run(
                host,
                "create_element",
                json!({ "diagram": d, "tool": "erEntity", "name": name }),
            );
            id(&e["element"])
        };
        let customer = entity(&mut host, "Customer");
        let order = entity(&mut host, "Order");
        run(
            &mut host,
            "add_member",
            json!({ "owner": customer, "text": "id : INTEGER", "key": true }),
        );
        let places = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "erNonIdentifying", "source": customer, "target": order }),
        );
        let places = id(&places["element"]);
        let model = host.document().model();
        let fk = |model: &Model| -> Vec<(String, Option<ErKey>)> {
            model
                .children(parse_id(&order).unwrap())
                .iter()
                .filter_map(|&c| model.get(c))
                .filter_map(|c| match c.kind() {
                    ElementKind::ErAttribute(p) if p.foreign_key.is_some() => {
                        Some((c.name().to_owned(), p.key))
                    }
                    _ => None,
                })
                .collect()
        };
        assert_eq!(fk(model), [("id".to_owned(), None)]);
        run(&mut host, "update_element", json!({ "id": places, "identifying": true }));
        assert_eq!(fk(host.document().model()), [("id".to_owned(), Some(ErKey::Primary))]);
        run(
            &mut host,
            "update_element",
            json!({ "id": d, "notation": "chen", "er_display": "keys" }),
        );
        let chen = run(&mut host, "get_diagram", json!({ "diagram": d })).to_string();
        assert!(chen.contains("erLine") && !chen.contains("erIdentifying\""), "{chen}");
        let bad = call(
            &mut host,
            &Access::default(),
            "update_element",
            &json!({ "id": customer, "notation": "ie" }),
        );
        assert!(bad.is_error);
        // An associative entity that entities take part in stays one.
        let enrolment = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erAssociativeEntity", "name": "Enrolment" }),
        );
        let enrolment = id(&enrolment["element"]);
        run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "erLine", "source": enrolment, "target": customer }),
        );
        let plain = |host: &mut Headless, entity: &str| {
            call(
                host,
                &Access::default(),
                "update_element",
                &json!({ "id": entity, "associative": false }),
            )
        };
        assert!(plain(&mut host, &enrolment).is_error);
        assert!(!plain(&mut host, &customer).is_error);
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
    fn objects_leave_out_slots_without_a_value_on_request() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let buch = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "class",
                                                            "name": "Buch" }),
        );
        for text in ["- titel : String", "- ausgeliehen : boolean"] {
            run(&mut host, "add_member", json!({ "owner": buch["element"], "text": text }));
        }
        let o = run(&mut host, "create_diagram", json!({ "kind": "object", "name": "Objekte" }));
        let o = id(&o["diagram"]);
        let args = json!({ "diagram": o, "tool": "object", "name": "a", "classifier": "Buch",
                           "slots": { "titel": "\"Momo\"" }, "empty_slots": false });
        let a = run(&mut host, "create_element", args);
        let slots = |host: &mut Headless| -> Vec<String> {
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), o.parse().unwrap(), &Default::default(), measure);
            scene
                .nodes
                .iter()
                .flat_map(|n| n.rows.iter())
                .filter(|r| r.role == scene::RowRole::Slot)
                .map(|r| r.text.clone())
                .collect()
        };
        assert_eq!(slots(&mut host), ["titel = \"Momo\""]);
        run(&mut host, "update_element", json!({ "id": a["element"], "empty_slots": true }));
        assert_eq!(slots(&mut host), ["titel = \"Momo\"", "ausgeliehen"]);
        // Only objects have slots.
        let wrong = json!({ "id": buch["element"], "empty_slots": false });
        assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);
    }

    #[test]
    fn ends_abstract_static_classifier_and_slots_are_set() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let node = |host: &mut Headless, args: Value| {
            let mut args = args;
            args["diagram"] = json!(d);
            run(host, "create_element", args)
        };
        let lager = node(&mut host, json!({ "tool": "class", "name": "Lager", "abstract": true }));
        let geraet = node(&mut host, json!({ "tool": "class", "name": "Geraet" }));
        let args = json!({ "diagram": d, "tool": "association", "source": lager["element"],
                           "target": geraet["element"],
                           "source_end": { "aggregation": "composite" },
                           "target_end": { "role": "- geraete {ordered}", "multiplicity": "*",
                                           "navigability": "navigable" } });
        let line = run(&mut host, "connect", args);
        let kind = |host: &Headless, v: &Value| {
            host.document().model().get(id(v).parse().unwrap()).unwrap().kind().clone()
        };
        let ElementKind::Association(p) = kind(&host, &line["element"]) else { panic!() };
        assert_eq!(p.ends[0].aggregation, umlcraft_model::Aggregation::Composite);
        let end = &p.ends[1];
        assert_eq!((end.role.as_str(), end.visibility), ("geraete", Visibility::Private));
        assert_eq!(end.constraints, ["ordered"]);
        assert_eq!(end.multiplicity.map(|m| m.to_string()), Some("*".into()));
        assert_eq!(end.navigability, umlcraft_model::Navigability::Navigable);
        let ElementKind::Class(c) = kind(&host, &lager["element"]) else { panic!() };
        assert!(c.is_abstract);
        let bad = json!({ "id": line["element"], "target_end": { "multiplicity": "viele" } });
        assert!(call(&mut host, &Access::default(), "update_element", &bad).is_error);

        // Static members, written so or flagged.
        let add = |host: &mut Headless, text: &str| {
            run(host, "add_member", json!({ "owner": geraet["element"], "text": text }))
        };
        let anzahl = add(&mut host, "- anzahl : int {static}");
        assert_eq!(anzahl["text"], "- anzahl : int");
        let ElementKind::Attribute(a) = kind(&host, &anzahl["id"]) else { panic!() };
        assert!(a.is_static);
        let get = add(&mut host, "+ getAnzahl() : int");
        run(&mut host, "update_element", json!({ "id": get["id"], "static": true }));
        let ElementKind::Operation(o) = kind(&host, &get["id"]) else { panic!() };
        assert!(o.is_static);

        // An object of a class, with values for its attributes.
        let hund = node(&mut host, json!({ "tool": "class", "name": "Hund" }));
        run(
            &mut host,
            "add_member",
            json!({ "owner": hund["element"], "text": "- name : String" }),
        );
        let rex = node(
            &mut host,
            json!({ "tool": "object", "name": "rex", "classifier": "Hund",
                    "slots": { "name": "\"Rex\"" } }),
        );
        let ElementKind::Instance(i) = kind(&host, &rex["element"]) else { panic!() };
        assert_eq!(i.classifier, Some(id(&hund["element"]).parse().unwrap()));
        let model = host.document().model();
        let slots = umlcraft_notation::object::slots(model, id(&rex["element"]).parse().unwrap());
        assert_eq!(slots.iter().map(|s| s.1.as_str()).collect::<Vec<_>>(), ["\"Rex\""]);
        let wrong = json!({ "id": rex["element"], "slots": { "alter": "3" } });
        assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);
        host.document().model().check().unwrap();
    }

    /// A line's points and its labels with their rectangles.
    type DrawnLine = (Vec<Point>, Vec<(String, Rect)>);
    /// A message a test draws: tool, source and target (by index), name, height.
    type Step = (&'static str, usize, usize, &'static str, f32);

    /// The labels of each line of diagram `d` as drawn, with their rectangles, and the lines'
    /// points and the nodes.
    fn drawn_labels(host: &mut Headless, d: &str) -> (Vec<DrawnLine>, Vec<Rect>) {
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let lines = scene
            .edges
            .iter()
            .map(|e| {
                let labels = e
                    .labels
                    .iter()
                    .map(|l| {
                        let w = scene::label_width(&l.text, measure);
                        (l.text.clone(), Rect::new(l.anchor.x, l.anchor.y, w, scene::LABEL_HEIGHT))
                    })
                    .collect();
                (e.points.clone(), labels)
            })
            .collect();
        (lines, scene.nodes.iter().map(|n| n.rect).collect())
    }

    #[test]
    fn roles_keep_to_their_own_ends() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str, members: &[&str]| {
            let c =
                run(host, "create_element", json!({ "diagram": d, "tool": "class", "name": name }));
            for member in members {
                run(host, "add_member", json!({ "owner": c["element"], "text": member }));
            }
            c
        };
        // The doubly linked list of #21.
        let operations = [
            "+ Playlist()",
            "+ hintenAnfuegen(lied : Lied)",
            "+ ausgeben()",
            "+ rueckwaertsAusgeben()",
            "+ loeschen(titel : String) : boolean",
        ];
        let playlist = class(&mut host, "Playlist", &operations);
        let knoten = class(&mut host, "Knoten", &["+ Knoten(inhalt : Lied)"]);
        let associate = |host: &mut Headless, s: &Value, t: &Value, role: &str| {
            let args = json!({ "diagram": d, "tool": "association", "source": s["element"],
                               "target": t["element"],
                               "target_end": { "role": role, "multiplicity": "0..1",
                                               "navigability": "navigable" } });
            run(host, "connect", args);
        };
        associate(&mut host, &knoten, &knoten, "- nachfolger");
        associate(&mut host, &knoten, &knoten, "- vorgaenger");
        associate(&mut host, &playlist, &knoten, "- kopf");
        associate(&mut host, &playlist, &knoten, "- ende");
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        let (lines, _) = drawn_labels(&mut host, &d);
        keep_to_own_lines(&lines, "");
        let mut roles = Vec::new();
        for (points, labels) in &lines {
            let target = points[points.len() - 1];
            let source = points[0];
            for (text, r) in labels.iter().filter(|(t, _)| t.starts_with('-')) {
                // Nearer the end it belongs to than the other one, and close to it.
                let (to_target, to_source) =
                    (target.distance(r.center()), source.distance(r.center()));
                assert!(to_target < 80.0, "{text} {r:?} {points:?}");
                if source != target {
                    assert!(to_target < to_source, "{text} {r:?} {points:?}");
                }
                roles.push(*r);
            }
        }
        assert_eq!(roles.len(), 4);
        for (i, a) in roles.iter().enumerate() {
            assert!(roles[i + 1..].iter().all(|b| !a.intersects(b)), "{roles:?}");
        }
    }

    #[test]
    fn a_long_role_fits_between_classes_laid_out_left_to_right() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str| {
            let args = json!({ "diagram": d, "tool": "class", "name": name, "abstract": true });
            run(host, "create_element", args)
        };
        let (gatter, baustein) = (class(&mut host, "Gatter"), class(&mut host, "Baustein"));
        let args = json!({ "diagram": d, "tool": "association", "source": gatter["element"],
                           "target": baustein["element"],
                           "target_end": { "role": "# eingaenge", "multiplicity": "*",
                                           "navigability": "navigable" } });
        run(&mut host, "connect", args);
        run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
        let (lines, nodes) = drawn_labels(&mut host, &d);
        let (points, labels) = &lines[0];
        for (text, r) in labels {
            assert!(nodes.iter().all(|n| !n.intersects(r)), "{text} {r:?} {nodes:?}");
            let crossed = points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
            assert!(!crossed, "{text} {r:?} {points:?}");
        }
    }

    #[test]
    fn labels_of_both_ends_fit_between_classes_laid_out_left_to_right() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str, member: &str| {
            let args = json!({ "diagram": d, "tool": "class", "name": name });
            let c = run(host, "create_element", args);
            run(host, "add_member", json!({ "owner": c["element"], "text": member }));
            c
        };
        let leser = class(&mut host, "Leser", "- name : String");
        let buch = class(&mut host, "Buch", "- titel : String");
        let end = |role: &str| {
            json!({ "role": role, "multiplicity": "0..1",
                                       "navigability": "navigable" })
        };
        let args = json!({ "diagram": d, "tool": "association", "source": leser["element"],
                           "target": buch["element"], "source_end": end("- leser"),
                           "target_end": end("- buch") });
        run(&mut host, "connect", args);
        run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
        let (lines, nodes) = drawn_labels(&mut host, &d);
        let (points, labels) = &lines[0];
        for (k, (text, r)) in labels.iter().enumerate() {
            assert!(nodes.iter().all(|n| !n.intersects(r)), "{text} {r:?} {nodes:?}");
            let crossed = points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
            assert!(!crossed, "{text} {r:?} {points:?}");
            for (other, o) in &labels[k + 1..] {
                assert!(!r.intersects(o), "{text} {r:?} {other} {o:?}");
            }
        }
    }

    #[test]
    fn a_role_beside_a_generalization_keeps_to_its_target() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str, is_abstract: bool| {
            let args =
                json!({ "diagram": d, "tool": "class", "name": name, "abstract": is_abstract });
            run(host, "create_element", args)
        };
        let baustein = class(&mut host, "Baustein", true);
        let schalter = class(&mut host, "Schalter", false);
        let gatter = class(&mut host, "Gatter", true);
        let und = class(&mut host, "Und", false);
        let oder = class(&mut host, "Oder", false);
        let inherit = |host: &mut Headless, from: &Value, to: &Value| {
            let args = json!({ "diagram": d, "tool": "generalization",
                               "source": from["element"], "target": to["element"] });
            run(host, "connect", args)
        };
        inherit(&mut host, &schalter, &baustein);
        inherit(&mut host, &gatter, &baustein);
        inherit(&mut host, &und, &gatter);
        inherit(&mut host, &oder, &gatter);
        let args = json!({ "diagram": d, "tool": "association", "source": gatter["element"],
                           "target": baustein["element"],
                           "target_end": { "role": "# eingaenge", "multiplicity": "*",
                                           "navigability": "navigable" } });
        run(&mut host, "connect", args);
        run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "down" }));
        let (lines, _) = drawn_labels(&mut host, &d);
        keep_to_own_lines(&lines, "");
        // Clear of the generalization trees: no stretch within a port's spacing of their lines
        // (all lines here run straight across or down, so their gap is that of their bounds).
        let (association, _) = lines.iter().find(|(_, l)| !l.is_empty()).expect("the association");
        for (tree, _) in lines.iter().filter(|(_, l)| l.is_empty()) {
            for (w, v) in association.windows(2).flat_map(|w| tree.windows(2).map(move |v| (w, v)))
            {
                let (a, b) = (Rect::from_corners(w[0], w[1]), Rect::from_corners(v[0], v[1]));
                let dx = (a.x - b.right()).max(b.x - a.right()).max(0.0);
                let dy = (a.y - b.bottom()).max(b.y - a.bottom()).max(0.0);
                assert!(dx.hypot(dy) >= 20.0, "{w:?} {v:?}");
            }
        }
        // The role at the association's end at Baustein, not beside Schalter.
        let (points, labels) = lines
            .iter()
            .find(|(_, labels)| labels.iter().any(|(t, _)| t == "# eingaenge"))
            .expect("the association's labels");
        let target = points[points.len() - 1];
        for (text, r) in labels {
            assert!(r.center().distance(target) < 80.0, "{text} {r:?} {points:?}");
        }
        let (_, role) = labels.iter().find(|(t, _)| t == "# eingaenge").unwrap();
        let nearest =
            Point::new(target.x.clamp(role.x, role.right()), target.y.clamp(role.y, role.bottom()));
        assert!(nearest.distance(target) < 40.0, "{role:?} {points:?}");
        // Its arrow clear of the triangle where the generalizations leave Baustein.
        let (stem, _) = lines.iter().find(|(_, l)| l.is_empty()).expect("a generalization");
        let triangle = stem[stem.len() - 1];
        assert!(triangle.distance(target) > 30.0, "{triangle:?} {points:?}");
        let (_, many) = labels.iter().find(|(t, _)| t == "*").unwrap();
        assert!(!many.intersects(role), "{many:?} {role:?}");
    }

    /// Asserts that each label of `lines` (as from [`drawn_labels`]) reads as its own line's:
    /// right beside it, or with its middle nearer it than any other line.
    fn keep_to_own_lines(lines: &[DrawnLine], case: &str) {
        let distance = |points: &[Point], p: Point| {
            points
                .windows(2)
                .map(|w| umlcraft_notation::scene::distance_to_segment(p, w[0], w[1]))
                .fold(f32::INFINITY, f32::min)
        };
        for (k, (points, labels)) in lines.iter().enumerate() {
            for (text, r) in labels {
                // Within a few units of its own line along a side.
                let beside = points
                    .windows(2)
                    .any(|w| r.inflate(6.0).intersects(&Rect::from_corners(w[0], w[1])));
                if beside {
                    continue;
                }
                let own = distance(points, r.center());
                for (j, (other, _)) in lines.iter().enumerate().filter(|&(j, _)| j != k) {
                    let d = distance(other, r.center());
                    assert!(own < d, "{case}: {text} {r:?} {own} to {points:?}, {d} to line {j}");
                }
            }
        }
    }

    #[test]
    fn two_loops_keep_clear_of_incoming_lines() {
        for arrange in [true, false] {
            let mut host = Headless::new();
            let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
            let class = |host: &mut Headless, name: &str, y: f32| {
                let args = json!({ "diagram": d, "tool": "class", "name": name, "x": 100.0,
                                   "y": y });
                run(host, "create_element", args)
            };
            let (playlist, knoten) =
                (class(&mut host, "Playlist", 40.0), class(&mut host, "Knoten", 220.0));
            let associate = |host: &mut Headless, from: &Value, to: &Value, role: &str| {
                let args = json!({ "diagram": d, "tool": "association", "source": from["element"],
                                   "target": to["element"],
                                   "target_end": { "role": role, "multiplicity": "0..1",
                                                   "navigability": "navigable" } });
                run(host, "connect", args)
            };
            associate(&mut host, &playlist, &knoten, "- kopf");
            associate(&mut host, &playlist, &knoten, "- ende");
            associate(&mut host, &knoten, &knoten, "- nachfolger");
            associate(&mut host, &knoten, &knoten, "- vorgaenger");
            if arrange {
                run(&mut host, "arrange_diagram", json!({ "diagram": d }));
            }
            let (lines, nodes) = drawn_labels(&mut host, &d);
            let all: Vec<(&String, Rect)> =
                lines.iter().flat_map(|(_, l)| l.iter().map(|(t, r)| (t, *r))).collect();
            for (k, (text, r)) in all.iter().enumerate() {
                // No label on another one, on a class or on a line.
                for (other, o) in &all[k + 1..] {
                    assert!(!r.intersects(o), "{arrange}: {text} {r:?} {other} {o:?}");
                }
                assert!(nodes.iter().all(|n| !n.intersects(r)), "{arrange}: {text} {r:?}");
                for (points, _) in &lines {
                    let crossed =
                        points.windows(2).any(|w| r.intersects(&Rect::from_corners(w[0], w[1])));
                    assert!(!crossed, "{arrange}: {text} {r:?} {points:?}");
                }
            }
            // Each label beside its own line, near its target end.
            keep_to_own_lines(&lines, if arrange { "arranged" } else { "as drawn" });
            for (points, labels) in &lines {
                let target = points[points.len() - 1];
                for (text, r) in labels {
                    let d = r.center().distance(target);
                    assert!(d < 80.0, "{arrange}: {text} {r:?} {points:?}");
                }
            }
            // The lines' ends apart from each other.
            let ends: Vec<Point> = lines.iter().flat_map(|(p, _)| [p[0], p[p.len() - 1]]).collect();
            for (k, a) in ends.iter().enumerate() {
                for b in &ends[k + 1..] {
                    assert!(a.distance(*b) > 5.0, "{arrange}: {a:?} {b:?}");
                }
            }
        }
    }

    #[test]
    fn object_diagrams_are_made_and_named_so() {
        let mut host = Headless::new();
        let args = json!({ "kind": "object", "name": "Snapshot" });
        let d = run(&mut host, "create_diagram", args)["diagram"].clone();
        let document = run(&mut host, "get_document", json!({}));
        let shown = document["diagrams"].as_array().unwrap().iter().find(|x| x["id"] == d);
        assert_eq!(shown.unwrap()["kind"], "object");
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        assert_eq!(diagram["kind"], "object");
        assert_eq!(diagram["element_tools"][0], "object");
    }

    #[test]
    fn links_between_objects_run_straight_after_arranging() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = json!({ "diagram": d, "tool": "class", "name": "Hund" });
        let hund = run(&mut host, "create_element", class);
        for text in ["- name : String", "- alter : int"] {
            run(&mut host, "add_member", json!({ "owner": hund["element"], "text": text }));
        }
        // One object shows two slots, the others none: their boxes differ in height.
        let object = |host: &mut Headless, name: &str, slots: Value| {
            let args = json!({ "diagram": d, "tool": "object", "name": name,
                               "classifier": "Hund", "slots": slots });
            run(host, "create_element", args)
        };
        let rex = object(&mut host, "rex", json!({ "name": "\"Rex\"", "alter": "3" }));
        let p1 = object(&mut host, "p1", json!({}));
        let p2 = object(&mut host, "p2", json!({}));
        for (s, t) in [(&rex, &p1), (&p1, &p2)] {
            let args = json!({ "diagram": d, "tool": "link", "source": s["element"],
                               "target": t["element"] });
            run(&mut host, "connect", args);
        }
        for direction in ["right", "down"] {
            let args = json!({ "diagram": d, "direction": direction });
            run(&mut host, "arrange_diagram", args);
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
            for e in &scene.edges {
                assert_eq!(e.points.len(), 2, "{direction}: {:?}", e.points);
            }
        }
    }

    #[test]
    fn actors_and_use_cases_are_not_associated_among_themselves() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
        let d = d["diagram"].clone();
        let node = |host: &mut Headless, tool: &str, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
        };
        let (a, b) = (node(&mut host, "actor", "A"), node(&mut host, "actor", "B"));
        let (u, v) = (node(&mut host, "useCase", "U"), node(&mut host, "useCase", "V"));
        let associate = |host: &mut Headless, x: &Value, y: &Value, anyway: bool| {
            let args = json!({ "diagram": d, "tool": "association", "source": x["element"],
                               "target": y["element"], "allow_invalid": anyway });
            !call(host, &Access::default(), "connect", &args).is_error
        };
        assert!(associate(&mut host, &a, &u, false));
        assert!(!associate(&mut host, &a, &b, false));
        assert!(!associate(&mut host, &u, &v, false));
        // For an exercise in finding mistakes.
        assert!(associate(&mut host, &a, &b, true));
        assert!(associate(&mut host, &u, &v, true));
        host.document().model().check().unwrap();
    }

    #[test]
    fn actors_specializing_one_actor_stand_side_by_side_below_it() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
        let d = d["diagram"].clone();
        let node = |host: &mut Headless, tool: &str, name: &str, x: f32, y: f32| {
            let args = json!({ "diagram": d, "tool": tool, "name": name, "x": x, "y": y });
            run(host, "create_element", args)
        };
        node(&mut host, "systemBoundary", "Selbstlernzentrum", 200.0, 40.0);
        let lend = node(&mut host, "useCase", "Gerät ausleihen", 230.0, 90.0);
        let check = node(&mut host, "useCase", "Ausweis prüfen", 230.0, 170.0);
        let reserve = node(&mut host, "useCase", "Gerät reservieren", 230.0, 250.0);
        let manage = node(&mut host, "useCase", "Geräte verwalten", 230.0, 330.0);
        let user = node(&mut host, "actor", "Nutzer", 40.0, 60.0);
        let pupil = node(&mut host, "actor", "Schüler", 10.0, 200.0);
        let teacher = node(&mut host, "actor", "Lehrkraft", 90.0, 200.0);
        let guard = node(&mut host, "actor", "Aufsicht", 600.0, 340.0);
        let connect = |host: &mut Headless, tool: &str, from: &Value, to: &Value| {
            let args = json!({ "diagram": d, "tool": tool, "source": from["element"],
                               "target": to["element"] });
            run(host, "connect", args)
        };
        connect(&mut host, "generalization", &pupil, &user);
        connect(&mut host, "generalization", &teacher, &user);
        connect(&mut host, "association", &user, &lend);
        connect(&mut host, "association", &teacher, &reserve);
        connect(&mut host, "association", &guard, &manage);
        connect(&mut host, "include", &lend, &check);
        connect(&mut host, "extend", &reserve, &lend);
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));

        let model = host.document().model();
        let view = model.view(id(&d).parse().unwrap()).unwrap();
        let rect = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap().bounds();
        let (user, pupil, teacher) =
            (rect(&user).unwrap(), rect(&pupil).unwrap(), rect(&teacher).unwrap());
        // Side by side, below Nutzer, each with a line of its own up to it.
        assert!((pupil.y - teacher.y).abs() < 0.5, "{pupil:?} {teacher:?}");
        assert!(!pupil.intersects(&teacher));
        assert!(pupil.y > user.bottom() && teacher.y > user.bottom());
        let (lo, hi) =
            (pupil.center().x.min(teacher.center().x), pupil.center().x.max(teacher.center().x));
        assert!(lo < user.center().x && user.center().x < hi, "{user:?} {pupil:?} {teacher:?}");
        model.check().unwrap();
    }

    #[test]
    fn actors_stand_on_both_sides_of_the_boundary_with_straight_lines() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "use_case", "name": "U" }));
        let d = d["diagram"].clone();
        // Use cases inside the boundary, actors below it.
        let node = |host: &mut Headless, tool: &str, name: &str, x: f32, y: f32| {
            let args = json!({ "diagram": d, "tool": tool, "name": name, "x": x, "y": y });
            run(host, "create_element", args)
        };
        let boundary = node(&mut host, "systemBoundary", "Selbstlernzentrum", 100.0, 100.0);
        let lend = node(&mut host, "useCase", "Gerät ausleihen", 130.0, 150.0);
        let back = node(&mut host, "useCase", "Gerät zurückgeben", 130.0, 230.0);
        let check = node(&mut host, "useCase", "Ausweis prüfen", 130.0, 310.0);
        let reserve = node(&mut host, "useCase", "Gerät vormerken", 130.0, 390.0);
        let user = node(&mut host, "actor", "Nutzer", 100.0, 600.0);
        let teacher = node(&mut host, "actor", "Lehrkraft", 200.0, 600.0);
        let librarian = node(&mut host, "actor", "Bibliothekarin", 300.0, 600.0);
        let connect = |host: &mut Headless, tool: &str, from: &Value, to: &Value| {
            let args = json!({ "diagram": d, "tool": tool, "source": from["element"],
                               "target": to["element"] });
            run(host, "connect", args)
        };
        let lines = [
            connect(&mut host, "generalization", &teacher, &user),
            connect(&mut host, "association", &user, &lend),
            connect(&mut host, "association", &user, &back),
            connect(&mut host, "association", &teacher, &reserve),
            connect(&mut host, "association", &librarian, &lend),
        ];
        connect(&mut host, "include", &lend, &check);
        connect(&mut host, "extend", &reserve, &lend);
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));

        let model = host.document().model();
        let view = model.view(id(&d).parse().unwrap()).unwrap();
        let rect = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap().bounds();
        let frame = rect(&boundary).unwrap();
        let (user, teacher, librarian) =
            (rect(&user).unwrap(), rect(&teacher).unwrap(), rect(&librarian).unwrap());
        for case in [&lend, &back, &check, &reserve] {
            assert!(frame.contains_rect(&rect(case).unwrap()), "{:?} {frame:?}", rect(case));
        }
        // The use cases one below the other, those of Nutzer next to each other, so that the
        // lines from the left don't cross.
        let middle = |v: &Value| rect(v).unwrap().center();
        assert!(
            [&back, &check, &reserve].iter().all(|v| (middle(v).x - middle(&lend).x).abs() < 0.5)
        );
        let order = |v: &Value| middle(v).y;
        assert!(order(&lend) < order(&back) && order(&back) < order(&reserve));
        // Nutzer and Lehrkraft on one side, Lehrkraft below; Bibliothekarin on the other.
        let left = |r: Rect| r.right() <= frame.x;
        let right = |r: Rect| r.x >= frame.right();
        assert!(left(user) || right(user), "{user:?} {frame:?}");
        assert_eq!(left(user), left(teacher));
        assert!((user.center().x - teacher.center().x).abs() < 0.5);
        assert!(teacher.y > user.bottom());
        assert!(if left(user) { right(librarian) } else { left(librarian) }, "{librarian:?}");
        // Straight lines.
        for line in &lines {
            let p = view.get(id(&line["presentation"]).parse().unwrap()).unwrap();
            let Shape::Edge { waypoints, .. } = &p.shape else { panic!() };
            assert!(waypoints.is_empty());
            assert_eq!(p.style.line_shape, umlcraft_model::LineShape::Line);
        }
        model.check().unwrap();
    }

    #[test]
    fn arranging_keeps_generalizations_chosen_separate() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let class = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "class", "name": name }))
        };
        let (b, s, g) = (
            class(&mut host, "Baustein"),
            class(&mut host, "Schalter"),
            class(&mut host, "Gatter"),
        );
        let inherit = |host: &mut Headless, from: &Value| {
            let args = json!({ "diagram": d, "tool": "generalization",
                               "source": from["element"], "target": b["element"] });
            run(host, "connect", args)
        };
        let lines = [inherit(&mut host, &s), inherit(&mut host, &g)];
        let styles = |host: &Headless| -> Vec<SharedStyle> {
            let view = host.document().model().view(d.parse().unwrap()).unwrap();
            let style =
                |l: &Value| view.get(id(&l["presentation"]).parse().unwrap()).unwrap().style.shared;
            lines.iter().map(style).collect()
        };
        // Arranged as they come, they share a triangle.
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        assert_eq!(styles(&host), [SharedStyle::Vertical; 2]);
        // Chosen separate, they stay so.
        for line in &lines {
            let args = json!({ "id": line["element"], "shared_style": "separate" });
            run(&mut host, "update_element", args);
        }
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        run(&mut host, "arrange_diagram", json!({ "diagram": d, "direction": "right" }));
        assert_eq!(styles(&host), [SharedStyle::Separate; 2]);
    }

    #[test]
    fn a_reply_goes_where_y_says() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Calc" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
        };
        let (a, b, c) =
            (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
        let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str, y| {
            let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                            "target": to["element"], "name": name, "y": y });
            run(host, "connect", m)
        };
        send(&mut host, "message", &a, &b, "rechne", 100);
        send(&mut host, "message", &b, &c, "hole", 140);
        send(&mut host, "reply", &c, &b, "x", 170);
        send(&mut host, "reply", &b, &a, "summe", 300);
        let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let y = |name: &str| {
            let lines = diagram["lines"].as_array().unwrap();
            lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
        };
        assert_eq!((y("x"), y("summe")), (170.0, 300.0), "{diagram}");
        // b's bar runs down to the reply it gives; a waits for it.
        let bottom = |lifeline: &Value| {
            diagram["bars"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|bar| bar["lifeline"] == lifeline["element"])
                .map(|bar| bar["bottom"].as_f64().unwrap())
                .fold(0.0, f64::max)
        };
        assert!((bottom(&b) - 300.0).abs() < 0.5, "{}", bottom(&b));
        assert!(bottom(&a) >= 300.0, "{}", bottom(&a));
        host.document().model().check().unwrap();
    }

    #[test]
    fn nested_fragments_are_inset_and_keep_to_their_messages() {
        // The inner fragment made first and wrapped, or the outer one first.
        for outer_first in [false, true] {
            let mut host = Headless::new();
            let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "C" }));
            let d = d["diagram"].clone();
            let lifeline = |host: &mut Headless, name: &str| {
                let args = json!({ "diagram": d, "tool": "lifeline", "name": name });
                run(host, "create_element", args)
            };
            let (a, b, c) =
                (lifeline(&mut host, "a"), lifeline(&mut host, "b"), lifeline(&mut host, "c"));
            let send = |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str| {
                let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                                "target": to["element"], "name": name });
                run(host, "connect", m)
            };
            send(&mut host, "message", &a, &b, "rechne");
            let hole = send(&mut host, "message", &b, &c, "hole");
            let x = send(&mut host, "reply", &c, &b, "x");
            send(&mut host, "reply", &b, &a, "summe");
            let wrap = |host: &mut Headless, items: Value, operator: &str| {
                let args = json!({ "diagram": d, "items": items, "operator": operator });
                run(host, "wrap_in_fragment", args)
            };
            let messages = json!([hole["element"], x["element"]]);
            let (opt, lp) = if outer_first {
                let lp = wrap(&mut host, messages.clone(), "loop");
                (wrap(&mut host, messages, "opt"), lp)
            } else {
                let opt = wrap(&mut host, messages, "opt");
                let lp = wrap(&mut host, json!([opt["id"]]), "loop");
                (opt, lp)
            };
            let diagram = run(&mut host, "get_diagram", json!({ "diagram": d }));
            let frame = |f: &Value| {
                let nodes = diagram["nodes"].as_array().unwrap();
                let b = &nodes.iter().find(|n| n["element"] == f["id"]).unwrap()["bounds"];
                let n = |k: &str| b[k].as_f64().unwrap();
                (n("x"), n("y"), n("x") + n("width"), n("y") + n("height"))
            };
            let y = |name: &str| {
                let lines = diagram["lines"].as_array().unwrap();
                lines.iter().find(|l| l["name"] == name).unwrap()["y"].as_f64().unwrap()
            };
            let (inner, outer) = (frame(&opt), frame(&lp));
            // The loop goes around the opt with room on every side, its label above the opt's.
            assert!(outer.0 < inner.0 - 5.0 && outer.2 > inner.2 + 5.0, "{outer:?} {inner:?}");
            assert!(outer.1 < inner.1 - 15.0 && outer.3 > inner.3 + 5.0, "{outer:?} {inner:?}");
            // The call before and the reply after stay outside.
            assert!(y("rechne") < outer.1 && y("summe") > outer.3, "{diagram}");
            host.document().model().check().unwrap();
        }
    }

    #[test]
    fn a_created_head_follows_its_message_into_fragments() {
        let mut host = Headless::new();
        let args = json!({ "kind": "sequence", "name": "Ausleihen", "message_numbers": false,
                           "frame": false });
        let d = run(&mut host, "create_diagram", args)["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            let args = json!({ "diagram": d, "tool": "lifeline", "name": name });
            run(host, "create_element", args)
        };
        let (main, t, g) = (
            lifeline(&mut host, "main"),
            lifeline(&mut host, "t : Theke"),
            lifeline(&mut host, "g : Geraet"),
        );
        let send =
            |host: &mut Headless, tool: &str, from: &Value, to: &Value, name: &str, y: f32| {
                let m = json!({ "diagram": d, "tool": tool, "source": from["element"],
                            "target": to["element"], "name": name, "y": y });
                run(host, "connect", m)
            };
        send(&mut host, "message", &main, &t, "ausleihen(g, \"Eva\")", 120.0);
        let free = send(&mut host, "message", &t, &g, "istFrei()", 150.0);
        let yes = send(&mut host, "reply", &g, &t, "frei", 175.0);
        let lend = send(&mut host, "message", &t, &g, "verleihen()", 220.0);
        let back = send(&mut host, "reply", &t, &main, "true", 300.0);
        let a = lifeline(&mut host, "a : Ausleihe");
        let create = send(&mut host, "createMessage", &t, &a, "Ausleihe(g, \"Eva\")", 260.0);
        let wrap = |host: &mut Headless, items: Value, operator: &str| {
            let args = json!({ "diagram": d, "items": items, "operator": operator });
            run(host, "wrap_in_fragment", args)
        };
        let opt = wrap(&mut host, json!([lend["element"], create["element"]]), "opt");
        let lp = wrap(&mut host, json!([free["element"], yes["element"], opt["id"]]), "loop");

        let model = host.document().model();
        let view = model.view(id(&d).parse().unwrap()).unwrap();
        let p = |v: &Value| view.get(id(&v["presentation"]).parse().unwrap()).unwrap();
        let Shape::Lifeline { head, .. } = p(&a).shape else { panic!("no lifeline") };
        let Shape::Message { y, .. } = p(&create).shape else { panic!("no message") };
        // The message still ends at the middle of the head.
        assert!((head.center().y - y).abs() < 1.0, "{head:?} {y}");
        // The reply after the loop stays below both frames.
        let Shape::Message { y: after, .. } = p(&back).shape else { panic!("no message") };
        for f in [&opt, &lp] {
            let frame = view.presentations_of(id(&f["id"]).parse().unwrap()).next().unwrap();
            let bottom = frame.bounds().unwrap().bottom();
            assert!(bottom < after, "{bottom} {after}");
        }
        model.check().unwrap();
    }

    /// A sequence diagram of the lifelines p, t and g with the messages `steps` (tool, source
    /// and target by index, name, height), as a client draws it. Returns the diagram and the
    /// messages' replies.
    fn sequence_script(host: &mut Headless, steps: &[Step]) -> (String, Vec<Value>) {
        let d = run(host, "create_diagram", json!({ "kind": "sequence", "name": "S" }));
        let d = id(&d["diagram"]);
        let lifelines: Vec<Value> = ["p", "t", "g"]
            .iter()
            .map(|n| {
                let args = json!({ "diagram": d, "tool": "lifeline", "name": n });
                run(host, "create_element", args)
            })
            .collect();
        let messages = steps
            .iter()
            .map(|&(tool, from, to, name, y)| {
                let args = json!({ "diagram": d, "tool": tool, "name": name, "y": y,
                                   "source": lifelines[from]["element"],
                                   "target": lifelines[to]["element"] });
                run(host, "connect", args)
            })
            .collect();
        (d, messages)
    }

    /// The bars of `lifeline` (by its index among p, t, g) on `d`, as (top, bottom).
    fn bars_of(host: &Headless, d: &str, lifeline: usize) -> Vec<(f32, f32)> {
        let model = host.document().model();
        let view = model.view(d.parse().unwrap()).unwrap();
        let lanes: Vec<Id> = view
            .presentations()
            .iter()
            .filter(|p| matches!(p.shape, Shape::Lifeline { .. }))
            .map(|p| p.id)
            .collect();
        view.presentations()
            .iter()
            .filter_map(|p| match p.shape {
                Shape::Activation { lifeline: l, top, height, .. } if l == lanes[lifeline] => {
                    Some((top, top + height))
                }
                _ => None,
            })
            .collect()
    }

    /// The height of the message presentation `m` (a reply of `connect`), and its end.
    fn message_y(host: &Headless, d: &str, m: &Value) -> (f32, f32) {
        let model = host.document().model();
        let view = model.view(d.parse().unwrap()).unwrap();
        match view.get(id(&m["presentation"]).parse().unwrap()).unwrap().shape {
            Shape::Message { y, end_y, .. } => (y, end_y.unwrap_or(y)),
            _ => panic!("no message"),
        }
    }

    /// The bounds of the fragment `f` (a reply of `wrap_in_fragment`) on `d`.
    fn frame_of(host: &Headless, d: &str, f: &Value) -> Rect {
        let model = host.document().model();
        let view = model.view(d.parse().unwrap()).unwrap();
        view.presentations_of(id(&f["id"]).parse().unwrap()).next().unwrap().bounds().unwrap()
    }

    #[test]
    fn a_call_drawn_further_down_belongs_to_the_activation_above() {
        let cases: [&[Step]; 2] = [
            &[("message", 0, 1, "call", 100.0), ("message", 1, 2, "x", 180.0)],
            &[
                ("message", 0, 1, "call", 100.0),
                ("message", 1, 2, "q", 130.0),
                ("reply", 2, 1, "q", 160.0),
                ("message", 1, 2, "x", 230.0),
            ],
        ];
        for (k, steps) in cases.iter().enumerate() {
            let mut host = Headless::new();
            let mut steps = steps.to_vec();
            steps.push(("reply", 1, 0, "r", 300.0));
            let (d, messages) = sequence_script(&mut host, &steps);
            let x = &messages[messages.len() - 2];
            let r = &messages[messages.len() - 1];
            // One bar on t, from the call down to its reply: `x` is sent within it.
            let bars = bars_of(&host, &d, 1);
            let (rx, _) = message_y(&host, &d, r);
            assert_eq!(bars.len(), 1, "{k}: {bars:?}");
            assert!(bars[0].0 <= 100.5 && bars[0].1 >= rx - 0.5, "{k}: {bars:?} {rx}");
            assert!((rx - 300.0).abs() < 0.5, "{k}: {rx}");
            // A fragment around `x` leaves the reply outside.
            let args = json!({ "diagram": d, "items": [x["element"]], "operator": "alt" });
            let alt = run(&mut host, "wrap_in_fragment", args);
            let frame = frame_of(&host, &d, &alt);
            let (rx, _) = message_y(&host, &d, r);
            assert!(frame.bottom() < rx, "{k}: {frame:?} {rx}");
            assert_eq!(bars_of(&host, &d, 1).len(), 1, "{k}");
            host.document().model().check().unwrap();
        }
    }

    #[test]
    fn fragments_keep_below_loops_to_self() {
        let cases: [&[Step]; 2] = [
            &[
                ("message", 0, 1, "call", 100.0),
                ("message", 1, 1, "suchen(8)", 140.0),
                ("reply", 1, 1, "g", 170.0),
                ("message", 1, 2, "x", 230.0),
            ],
            &[
                ("message", 0, 1, "call", 100.0),
                ("message", 1, 1, "ok = auszahlen(300)", 140.0),
                ("message", 1, 2, "x", 200.0),
            ],
        ];
        for (k, steps) in cases.iter().enumerate() {
            let mut host = Headless::new();
            let mut steps = steps.to_vec();
            steps.push(("reply", 1, 0, "r", 300.0));
            let (d, messages) = sequence_script(&mut host, &steps);
            let x = &messages[messages.len() - 2];
            let args = json!({ "diagram": d, "items": [x["element"]], "operator": "alt" });
            let alt = run(&mut host, "wrap_in_fragment", args);
            let frame = frame_of(&host, &d, &alt);
            // Below the lowest loop to self and the bars on t above the frame.
            let lowest = messages[1..messages.len() - 2]
                .iter()
                .map(|m| message_y(&host, &d, m))
                .map(|(y, end)| end.max(y + umlcraft_notation::sequence::SELF_DROP))
                .fold(f32::MIN, f32::max);
            let inner = bars_of(&host, &d, 1)
                .into_iter()
                .filter(|&(top, _)| top > 100.5 && top < frame.y)
                .map(|(_, bottom)| bottom)
                .fold(f32::MIN, f32::max);
            assert!(frame.y > lowest && frame.y > inner, "{k}: {frame:?} {lowest} {inner}");
            // The bar the loop leaves runs beside its label, not through it.
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
            let edge = scene
                .edges
                .iter()
                .find(|e| e.labels.iter().any(|l| l.text.contains(steps[1].3)))
                .unwrap();
            let label = edge.labels.iter().find(|l| l.text.contains(steps[1].3)).unwrap();
            let width = scene::label_width(&label.text, measure);
            assert!(label.anchor.x - width / 2.0 >= edge.points[0].x, "{k}: {label:?} {edge:?}");
            host.document().model().check().unwrap();
        }
    }

    #[test]
    fn a_reply_to_self_leaves_the_bar_its_call_started() {
        let mut host = Headless::new();
        let steps: Vec<Step> = vec![
            ("message", 0, 1, "call", 100.0),
            ("message", 1, 1, "suchen(8)", 140.0),
            ("reply", 1, 1, "g", 170.0),
            ("message", 1, 2, "x", 230.0),
            ("reply", 1, 0, "r", 300.0),
        ];
        let (d, messages) = sequence_script(&mut host, &steps);
        // The inner bar keeps the height a call gives it, and the reply leaves at its end.
        let (_, arrived) = message_y(&host, &d, &messages[1]);
        let (g, _) = message_y(&host, &d, &messages[2]);
        let inner = bars_of(&host, &d, 1).into_iter().find(|&(top, _)| (top - arrived).abs() < 0.5);
        let (top, bottom) = inner.expect("a bar for the call to self");
        assert!(bottom - top >= sequence_edit::RECEIVER_BAR - 0.01, "{top} {bottom}");
        assert!((g - bottom).abs() < 0.5, "{g} {bottom}");
        // From the inner bar's side back into the outer one.
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let points = |text: &str| {
            let e = scene.edges.iter().find(|e| e.labels.iter().any(|l| l.text == text)).unwrap();
            e.points.clone()
        };
        let (call, reply) = (points("1.1: suchen(8)"), points("g"));
        assert!((reply[0].x - call[3].x).abs() < 0.5, "{reply:?} {call:?}");
        assert!((reply[3].x - call[0].x).abs() < 0.5, "{reply:?} {call:?}");
        host.document().model().check().unwrap();
    }

    #[test]
    fn sequence_diagrams_go_without_numbers_and_frame() {
        let mut host = Headless::new();
        let args = json!({ "kind": "sequence", "name": "Calc", "message_numbers": false,
                           "frame": false });
        let d = id(&run(&mut host, "create_diagram", args)["diagram"]);
        let props = |host: &Headless| match host.document().model().get(d.parse().unwrap()) {
            Some(e) => match e.kind() {
                ElementKind::Diagram(p) => *p,
                _ => panic!("no diagram"),
            },
            None => panic!("gone"),
        };
        let p = props(&host);
        assert!(p.hide_message_numbers && p.hide_frame && !p.hide_return_values);
        let args = json!({ "id": d, "message_numbers": true, "return_values": false });
        run(&mut host, "update_element", args);
        let p = props(&host);
        assert!(!p.hide_message_numbers && p.hide_frame && p.hide_return_values);
        let class = json!({ "kind": "class", "name": "C", "frame": false });
        assert!(call(&mut host, &Access::default(), "create_diagram", &class).is_error);
    }

    #[test]
    fn a_call_named_with_parentheses_takes_them_as_arguments() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Calc" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
        };
        let (a, b) = (lifeline(&mut host, "a"), lifeline(&mut host, "b"));
        let send = |host: &mut Headless, name: &str| {
            let m = json!({ "diagram": d, "tool": "message", "source": a["element"],
                            "target": b["element"], "name": name });
            run(host, "connect", m)
        };
        let label = |host: &Headless, m: &Value| {
            let id = parse_id(m["element"].as_str().unwrap()).unwrap();
            umlcraft_notation::sequence::message_text(host.document().model(), id)
        };
        let rechne = send(&mut host, "rechne()");
        assert_eq!(label(&host, &rechne), "rechne()");
        let leihe = send(&mut host, r#"ausleihen(g, "Ada")"#);
        assert_eq!(label(&host, &leihe), r#"ausleihen(g, "Ada")"#);
        let update = json!({ "id": rechne["element"], "name": "summe(a, b)" });
        run(&mut host, "update_element", update);
        assert_eq!(label(&host, &rechne), "summe(a, b)");
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

        // Selecting in it: an element by its path, the whole, nothing.
        let s = &made["id"];
        let element = run(&mut host, "select", json!({ "diagram": s, "path": "body.1.body.0" }));
        assert_eq!(element["path"], "body.1.body.0");
        let whole = run(&mut host, "select", json!({ "diagram": s, "ids": [s] }));
        assert_eq!(whole["whole"], true);
        let none = run(&mut host, "select", json!({ "diagram": s, "ids": [] }));
        assert_eq!(none["whole"], false);
        let refused = |host: &mut Headless, args: Value| {
            call(host, &Access::default(), "select", &args).is_error
        };
        assert!(refused(&mut host, json!({ "diagram": s, "path": "body.7" })));
        assert!(refused(&mut host, json!({ "diagram": s, "ids": [class["id"]] })));
        let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
        assert!(refused(&mut host, json!({ "diagram": d["diagram"], "path": "body.0" })));

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
    fn activity_pins_parameters_and_calls() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
        let d = id(&d["diagram"]);
        let make = |host: &mut Headless, tool: &str, extra: Value| {
            let mut args = json!({ "diagram": d, "tool": tool });
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            run(host, "create_element", args)
        };
        let pack = make(&mut host, "callBehavior", json!({ "name": "Pack", "x": 200, "y": 100 }));
        let pin_in = make(&mut host, "inputPin", json!({ "on": pack["presentation"] }));
        let pin_out = make(&mut host, "outputPin", json!({ "on": pack["presentation"] }));
        let send = make(&mut host, "sendSignal", json!({ "name": "Notify", "x": 200, "y": 250 }));
        let param =
            make(&mut host, "activityParameter", json!({ "name": "order", "x": 0, "y": 0 }));
        make(&mut host, "acceptTimeEvent", json!({ "name": "Daily" }));
        make(&mut host, "activityConnector", json!({ "name": "A" }));
        // A pin away from every action is refused.
        let lone = call(
            &mut host,
            &Access::default(),
            "create_element",
            &json!({ "diagram": d, "tool": "inputPin", "x": 900, "y": 900 }),
        );
        assert!(lone.is_error);
        run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "flow", "source": pin_out["element"], "target": send["element"] }),
        );
        let activity = host.document().model().element(parse_id(&d).unwrap()).unwrap().owner();
        let activity = activity.unwrap().to_string();
        // The activity called may be given by its diagram.
        run(&mut host, "update_element", json!({ "id": pack["element"], "calls": d }));
        run(
            &mut host,
            "update_element",
            json!({ "id": pin_in["element"], "ordering": "LIFO", "upper_bound": "2",
                    "stream": true, "object_type": "Order" }),
        );
        for wrong in [
            json!({ "id": send["element"], "ordering": "FIFO" }),
            json!({ "id": pin_in["element"], "calls": activity }),
            json!({ "id": param["element"], "ordering": "sideways" }),
            json!({ "id": pack["element"], "calls": pin_in["element"] }),
        ] {
            assert!(
                call(&mut host, &Access::default(), "update_element", &wrong).is_error,
                "{wrong}"
            );
        }
        let model = host.document().model();
        model.check().unwrap();
        let node = |value: &Value| match model.element(parse_id(&id(&value["element"])).unwrap()) {
            Ok(e) => match e.kind() {
                ElementKind::ActivityNode(p) => p.clone(),
                other => panic!("{other:?}"),
            },
            Err(e) => panic!("{e}"),
        };
        assert_eq!(node(&pack).behavior.map(|b| b.to_string()), Some(activity.clone()));
        let pin = node(&pin_in);
        assert_eq!(pin.ordering, Some(umlcraft_model::ObjectOrdering::Lifo));
        assert_eq!(pin.upper_bound, "2");
        assert!(pin.is_stream);
        assert!(pin.type_ref.is_some());
        let pin_owner = model.element(parse_id(&id(&pin_in["element"])).unwrap()).unwrap().owner();
        assert_eq!(pin_owner.map(|o| o.to_string()), Some(id(&pack["element"])));
        assert_eq!(node(&param).node, ActivityNodeKind::ActivityParameter);
        // The output pin found room on the right of the action.
        let view = model.view(parse_id(&d).unwrap()).unwrap();
        let bounds = |value: &Value| {
            view.get(parse_id(&id(&value["presentation"])).unwrap()).unwrap().bounds().unwrap()
        };
        assert!(bounds(&pin_out).center().x > bounds(&pack).center().x);
        assert!(bounds(&pin_in).center().x < bounds(&pack).center().x);
    }

    /// Pins put `on` an action find room where their texts stay clear of the other pins'
    /// texts, the action growing to hold them, and keep it when the diagram is arranged.
    #[test]
    fn pins_on_an_action_keep_their_texts_apart() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
        let d = id(&d["diagram"]);
        let action = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "action", "name": "Ship", "x": 200, "y": 100 }),
        );
        let pins = [
            json!({ "tool": "inputPin", "name": "order", "object_type": "Order",
                    "in_state": "paid", "ordering": "FIFO", "upper_bound": "2" }),
            json!({ "tool": "inputPin", "name": "address", "control": true }),
            json!({ "tool": "inputPin", "name": "voucher", "exception": true,
                    "object_effect": "read" }),
            json!({ "tool": "outputPin", "name": "parcel", "stream": true }),
            json!({ "tool": "outputPin", "name": "receipt", "in_state": "printed" }),
        ];
        let action_node = parse_id(&id(&action["presentation"])).unwrap();
        let diagram = parse_id(&d).unwrap();
        // Every text of a pin of the action, and its square, by pin.
        let shapes = |host: &mut Headless| -> Vec<(Rect, Vec<Rect>)> {
            let (doc, measure) = host.parts();
            let model = doc.model();
            let scene = scene::build(model, diagram, &Preview::default(), measure);
            let borne = composite::borne_nodes(model, diagram);
            scene
                .nodes
                .iter()
                .filter(|n| borne.get(&n.id).copied().flatten() == Some(action_node))
                .map(|n| (n.rect, n.outside()))
                .collect()
        };
        let clear = |shapes: &[(Rect, Vec<Rect>)]| {
            let overlap = |a: &Rect, b: &Rect| {
                a.x < b.right() - 0.01
                    && b.x < a.right() - 0.01
                    && a.y < b.bottom() - 0.01
                    && b.y < a.bottom() - 0.01
            };
            for (i, (_, texts)) in shapes.iter().enumerate() {
                for (j, (square, others)) in shapes.iter().enumerate().filter(|(j, _)| *j != i) {
                    for t in texts {
                        assert!(!overlap(t, square), "pin {i}: {t:?} on pin {j} {square:?}");
                        for o in others {
                            assert!(!overlap(t, o), "pin {i}: {t:?} on {o:?} of pin {j}");
                        }
                    }
                }
            }
        };
        let size = |host: &Headless| {
            let view = host.document().model().view(diagram).unwrap();
            view.get(action_node).unwrap().bounds().unwrap()
        };
        let before = size(&host);
        for pin in &pins {
            let mut args = json!({ "diagram": d, "on": action["presentation"] });
            args.as_object_mut().unwrap().extend(pin.as_object().unwrap().clone());
            run(&mut host, "create_element", args);
            clear(&shapes(&mut host));
        }
        assert_eq!(shapes(&mut host).len(), pins.len());
        let grown = size(&host);
        assert!(grown.height > before.height, "{grown:?}");
        // Input pins on the left, output pins on the right.
        let squares: Vec<Rect> = shapes(&mut host).into_iter().map(|(r, _)| r).collect();
        assert_eq!(squares.iter().filter(|r| r.right() <= grown.x + 0.01).count(), 3);
        assert_eq!(squares.iter().filter(|r| r.x >= grown.right() - 0.01).count(), 2);
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        clear(&shapes(&mut host));
        host.document().model().check().unwrap();
    }

    #[test]
    fn control_exception_and_effect_of_object_nodes() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "activity", "name": "Order" }));
        let d = id(&d["diagram"]);
        let action = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "action", "name": "Ship", "x": 200, "y": 100 }),
        );
        // `create_element` takes the settings along: a control pin, an exception pin.
        let control = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "inputPin", "on": action["presentation"],
                    "control": true, "object_effect": "read" }),
        );
        let exception = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "outputPin", "on": action["presentation"],
                    "exception": true }),
        );
        let object = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "objectNode", "name": "Box", "x": 200, "y": 300 }),
        );
        run(
            &mut host,
            "update_element",
            json!({ "id": object["element"], "control": true, "exception": true,
                    "object_effect": "delete" }),
        );
        run(&mut host, "update_element", json!({ "id": control["element"], "object_effect": "" }));
        for (tool, wrong) in [
            ("update_element", json!({ "id": action["element"], "exception": true })),
            ("update_element", json!({ "id": object["element"], "object_effect": "eat" })),
            ("update_element", json!({ "id": object["element"], "control": "yes" })),
            (
                "create_element",
                json!({ "diagram": d, "tool": "action", "name": "X", "control": true }),
            ),
        ] {
            assert!(call(&mut host, &Access::default(), tool, &wrong).is_error, "{wrong}");
        }
        let model = host.document().model();
        model.check().unwrap();
        // The refused action was not created.
        assert!(model.elements().all(|e| e.name() != "X"));
        let node = |value: &Value| match model.element(parse_id(&id(&value["element"])).unwrap()) {
            Ok(e) => match e.kind() {
                ElementKind::ActivityNode(p) => p.clone(),
                other => panic!("{other:?}"),
            },
            Err(e) => panic!("{e}"),
        };
        let pin = node(&control);
        assert!(pin.is_control && !pin.is_exception && pin.effect.is_none());
        assert!(node(&exception).is_exception);
        let o = node(&object);
        assert!(o.is_control && o.is_exception);
        assert_eq!(o.effect, Some(umlcraft_model::ObjectEffect::Delete));
        assert_eq!(o.property_lines(), ["{delete}", "{control}"]);
    }

    #[test]
    fn moved_states_become_substates_and_take_theirs_along() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
        let d = id(&d["diagram"]);
        let make = |host: &mut Headless, name: &str, x: f32, y: f32| {
            let args = json!({ "diagram": d, "tool": "state", "name": name, "x": x, "y": y });
            run(host, "create_element", args)
        };
        // A state made first, then a larger one, drawn later, moved over it.
        let inner = make(&mut host, "z1", 600.0, 100.0);
        let deeper = make(&mut host, "z2", 100.0, 400.0);
        let outer = make(&mut host, "Outer", 100.0, 100.0);
        let size = json!({ "diagram": d, "node": outer["presentation"], "x": 100, "y": 100,
                           "width": 360, "height": 220 });
        run(&mut host, "move_node", size);
        for (node, x, y) in [(&inner, 140, 160), (&deeper, 300, 160)] {
            let args = json!({ "diagram": d, "node": node["presentation"], "x": x, "y": y });
            run(&mut host, "move_node", args);
        }
        let model = host.document().model();
        let owner = |e: &Value| model.get(id(e).parse().unwrap()).unwrap().owner();
        let outer_id: Id = id(&outer["element"]).parse().unwrap();
        assert_eq!(owner(&inner["element"]), Some(outer_id));
        assert_eq!(owner(&deeper["element"]), Some(outer_id));
        // Both are drawn after the state they are in, so it does not hide them.
        let view = model.view(d.parse().unwrap()).unwrap();
        let at = |p: &Value| {
            let p: Id = id(p).parse().unwrap();
            view.presentations().iter().position(|q| q.id == p).unwrap()
        };
        assert!(at(&inner["presentation"]) > at(&outer["presentation"]));
        assert!(at(&deeper["presentation"]) > at(&outer["presentation"]));
        // Moving the composite state takes its substates along.
        let args = json!({ "diagram": d, "node": outer["presentation"], "x": 200, "y": 100 });
        run(&mut host, "move_node", args);
        let model = host.document().model();
        let view = model.view(d.parse().unwrap()).unwrap();
        let x = |p: &Value| view.get(id(p).parse().unwrap()).unwrap().bounds().unwrap().x;
        assert_eq!(x(&inner["presentation"]), 240.0);
        assert_eq!(
            model.get(id(&inner["element"]).parse().unwrap()).unwrap().owner(),
            Some(outer_id)
        );
    }

    #[test]
    fn opposite_transitions_between_states_side_by_side_run_apart() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "S" }));
        let d = id(&d["diagram"]);
        let make = |host: &mut Headless, x: f32| {
            run(host, "create_element", json!({ "diagram": d, "tool": "state", "x": x, "y": 100 }))
        };
        let (a, b) = (make(&mut host, 100.0), make(&mut host, 400.0));
        let connect = |host: &mut Headless, s: &Value, t: &Value| {
            let args = json!({ "diagram": d, "tool": "transition", "source": s["element"],
                               "target": t["element"] });
            id(&run(host, "connect", args)["presentation"])
        };
        let there = connect(&mut host, &a, &b);
        let back = connect(&mut host, &b, &a);
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let points = |e: &str| scene.edge(e.parse().unwrap()).unwrap().points.clone();
        let ys = |e: &str| points(e).iter().map(|p| p.y).collect::<Vec<_>>();
        // The second line, routed with the first, does not lie on it.
        assert!(
            ys(&there).iter().all(|y| !ys(&back).contains(y)),
            "{:?} {:?}",
            points(&there),
            points(&back)
        );
        // A line from B to a third state is routed with neither: it shares only B with them.
        let c = make(&mut host, 700.0);
        let onward = connect(&mut host, &b, &c);
        let model = host.document().model();
        let ids = |e: &[&String]| e.iter().map(|e| e.parse().unwrap()).collect::<Vec<_>>();
        let along = umlcraft_notation::autoroute::with_neighbours(
            model,
            d.parse().unwrap(),
            &ids(&[&there]),
        );
        assert_eq!(along, ids(&[&there, &back]));
        let along = umlcraft_notation::autoroute::with_neighbours(
            model,
            d.parse().unwrap(),
            &ids(&[&onward]),
        );
        assert_eq!(along, ids(&[&onward]));
    }

    #[test]
    fn names_of_opposite_transitions_stand_outside_their_lines() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "S" }));
        let d = id(&d["diagram"]);
        let make = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "state", "name": name }))
        };
        let (a, b) = (make(&mut host, "A"), make(&mut host, "B"));
        let connect = |host: &mut Headless, s: &Value, t: &Value, name: &str| {
            let args = json!({ "diagram": d, "tool": "transition", "source": s["element"],
                               "target": t["element"], "name": name });
            id(&run(host, "connect", args)["presentation"])
        };
        let there = connect(&mut host, &a, &b, "A to B");
        let back = connect(&mut host, &b, &a, "B to A");
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        // Where the label's middle lies across the line's middle segment, and where the other
        // line does.
        let across = |e: &str, p: Point| {
            let edge = scene.edge(e.parse().unwrap()).unwrap();
            let (s, t) = umlcraft_notation::route::middle_segment(&edge.points, false);
            let len = s.distance(t);
            ((t.x - s.x) * (p.y - s.y) - (t.y - s.y) * (p.x - s.x)) / len
        };
        let label_middle = |e: &str| {
            let edge = scene.edge(e.parse().unwrap()).unwrap();
            let label = &edge.labels[0];
            let width = scene::label_width(&label.text, measure);
            label.anchor.offset(width / 2.0, scene::LABEL_HEIGHT / 2.0)
        };
        let line_middle = |e: &str| {
            let edge = scene.edge(e.parse().unwrap()).unwrap();
            let (s, t) = umlcraft_notation::route::middle_segment(&edge.points, false);
            Point::new((s.x + t.x) / 2.0, (s.y + t.y) / 2.0)
        };
        for (own, other) in [(&there, &back), (&back, &there)] {
            let label = across(own, label_middle(own));
            let line = across(own, line_middle(other));
            assert!(label * line < 0.0, "{own}: label {label}, other line {line}");
        }
    }

    #[test]
    fn states_grow_around_their_substates() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
        let d = id(&d["diagram"]);
        let make = |host: &mut Headless, name: &str, x: f32, y: f32| {
            let args = json!({ "diagram": d, "tool": "state", "name": name, "x": x, "y": y });
            run(host, "create_element", args)
        };
        let bounds = |host: &Headless, node: &Value| {
            let view = host.document().model().view(d.parse().unwrap()).unwrap();
            view.get(id(&node["presentation"]).parse().unwrap()).unwrap().bounds().unwrap()
        };
        // Resizes in place, so that nothing moves along.
        let resize = |host: &mut Headless, node: &Value, w: f32, h: f32| {
            let r = bounds(host, node);
            let args = json!({ "diagram": d, "node": node["presentation"], "x": r.x, "y": r.y,
                               "width": w, "height": h });
            run(host, "move_node", args);
        };
        let u = make(&mut host, "U", 20.0, 300.0);
        resize(&mut host, &u, 200.0, 100.0);
        let dd = make(&mut host, "D", 40.0, 340.0);
        let e = make(&mut host, "E", 600.0, 340.0);
        let args = json!({ "diagram": d, "tool": "transition", "source": e["element"],
                           "target": e["element"], "name": "e2 [z > 0] / z = z * 2" });
        run(&mut host, "connect", args);
        let lines = run(&mut host, "get_diagram", json!({ "diagram": d }))["lines"].clone();
        assert_eq!(lines[0]["label"], "e2 [z > 0] / z = z * 2");
        // Dropped half over the right border, E becomes a substate and U grows around it.
        let args = json!({ "diagram": d, "node": e["presentation"], "x": 150, "y": 340 });
        run(&mut host, "move_node", args);
        let model = host.document().model();
        let owner = model.get(id(&e["element"]).parse().unwrap()).unwrap().owner();
        assert_eq!(owner, Some(id(&u["element"]).parse().unwrap()));
        assert!(bounds(&host, &u).contains_rect(&bounds(&host, &e)), "{:?}", bounds(&host, &u));
        // So does the loop at E's top right corner, with room for its label.
        let corner = scene::self_loop(bounds(&host, &e))[1];
        assert!(bounds(&host, &u).contains(corner.offset(0.0, -scene::LABEL_HEIGHT)));
        // Shrunk below its substates, yet around their middles so that they stay in it, U grows
        // around them again when the diagram is arranged.
        let (r, de, ee) = (bounds(&host, &u), bounds(&host, &dd), bounds(&host, &e));
        let width = de.center().x.max(ee.center().x) - r.x + 10.0;
        let height = de.center().y.max(ee.center().y) - r.y + 10.0;
        resize(&mut host, &u, width, height);
        assert!(!bounds(&host, &u).contains_rect(&bounds(&host, &e)));
        let model = host.document().model();
        let owner = model.get(id(&e["element"]).parse().unwrap()).unwrap().owner();
        assert_eq!(owner, Some(id(&u["element"]).parse().unwrap()));
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        for inner in [&dd, &e] {
            assert!(
                bounds(&host, &u).contains_rect(&bounds(&host, inner)),
                "{:?} {:?}",
                bounds(&host, &u),
                bounds(&host, inner)
            );
        }
    }

    #[test]
    fn a_loop_keeps_to_the_corner_of_its_state() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
        let d = id(&d["diagram"]);
        let args = json!({ "diagram": d, "tool": "state", "name": "C", "x": 100, "y": 100 });
        let state = run(&mut host, "create_element", args);
        let args = json!({ "diagram": d, "tool": "transition", "source": state["element"],
                           "target": state["element"], "name": "e2 / z = z * 2" });
        let line = run(&mut host, "connect", args);
        let at_corner = |host: &Headless| {
            let view = host.document().model().view(d.parse().unwrap()).unwrap();
            let r =
                view.get(id(&state["presentation"]).parse().unwrap()).unwrap().bounds().unwrap();
            let Shape::Edge { waypoints, .. } =
                &view.get(id(&line["presentation"]).parse().unwrap()).unwrap().shape
            else {
                panic!("not a line")
            };
            assert_eq!(waypoints, &scene::self_loop(r), "{r:?}");
            r
        };
        let first = at_corner(&host);
        // The state grows for a long activity: the loop stays at its corner.
        let args = json!({ "owner": state["element"], "text": "entry / x = x + 2 and a lot more" });
        run(&mut host, "add_member", args);
        let grown = at_corner(&host);
        assert!(grown.width > first.width, "{grown:?}");
        let args = json!({ "diagram": d, "node": state["presentation"], "x": 300, "y": 250 });
        run(&mut host, "move_node", args);
        at_corner(&host);
    }

    #[test]
    fn loops_of_one_node_keep_to_different_corners() {
        let mut host = Headless::new();
        let d = id(&run(&mut host, "get_document", json!({}))["diagrams"][0]["id"]);
        let args = json!({ "diagram": d, "tool": "class", "name": "Node", "x": 100, "y": 100 });
        let class = run(&mut host, "create_element", args);
        let lines: Vec<Value> = (0..2)
            .map(|_| {
                let args = json!({ "diagram": d, "tool": "association",
                                   "source": class["element"], "target": class["element"] });
                run(&mut host, "connect", args)
            })
            .collect();
        let at_corners = |host: &Headless| {
            let view = host.document().model().view(d.parse().unwrap()).unwrap();
            let r =
                view.get(id(&class["presentation"]).parse().unwrap()).unwrap().bounds().unwrap();
            for (slot, line) in lines.iter().enumerate() {
                let Shape::Edge { waypoints, source_anchor, target_anchor, .. } =
                    &view.get(id(&line["presentation"]).parse().unwrap()).unwrap().shape
                else {
                    panic!("not a line")
                };
                assert_eq!(waypoints, &scene::self_loop_at(r, slot), "{slot} {r:?}");
                let (from, to) = scene::self_loop_anchors_at(r, slot);
                if source_anchor.is_some() {
                    assert_eq!((*source_anchor, *target_anchor), (Some(from), Some(to)));
                }
            }
            r
        };
        let first = at_corners(&host);
        // The class grows for an attribute: each loop stays at its corner.
        run(&mut host, "add_member", json!({ "owner": class["element"], "text": "- value : int" }));
        assert!(at_corners(&host).height > first.height);
        run(&mut host, "arrange_diagram", json!({ "diagram": d }));
        at_corners(&host);
    }

    #[test]
    fn states_take_activities_and_internal_transitions() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "TU" }));
        let d = id(&d["diagram"]);
        let state = run(&mut host, "create_element", json!({ "diagram": d, "tool": "state" }));
        let state = id(&state["element"]);
        let add = |host: &mut Headless, text: &str| {
            run(host, "add_member", json!({ "owner": state, "text": text }))
        };
        add(&mut host, "entry / light on");
        add(&mut host, "DO / blink");
        add(&mut host, "exit / light off");
        let t = add(&mut host, "coin [credit < 2] / count");
        assert_eq!(t["kind"], "internalTransition");
        assert_eq!(t["text"], "coin [credit < 2] / count");
        let props = |host: &Headless| match host.document().model().get(state.parse().unwrap()) {
            Some(e) => match e.kind() {
                ElementKind::State(p) => p.clone(),
                _ => panic!(),
            },
            None => panic!(),
        };
        let p = props(&host);
        assert_eq!((p.entry.as_str(), p.do_activity.as_str()), ("light on", "blink"));
        assert_eq!(p.exit, "light off");
        // update_element changes and clears them, and rewrites the transition.
        run(&mut host, "update_element", json!({ "id": state, "entry": "", "do": "flash" }));
        let p = props(&host);
        assert_eq!((p.entry.as_str(), p.do_activity.as_str()), ("", "flash"));
        let t = run(&mut host, "update_element", json!({ "id": t["id"], "text": "reset / clear" }));
        let changed = host.document().model().get(id(&t["id"]).parse().unwrap()).unwrap();
        let ElementKind::InternalTransition(t) = changed.kind() else { panic!() };
        assert_eq!(
            (t.trigger.as_str(), t.guard.as_str(), t.effect.as_str()),
            ("reset", "", "clear")
        );
        let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
        let class = run(
            &mut host,
            "create_element",
            json!({ "diagram": class["diagram"], "tool": "class", "name": "K" }),
        );
        let wrong = json!({ "id": class["element"], "entry": "x" });
        assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);

        // A transition's name is read as `trigger [guard] / effect`, as the app shows it.
        let other = run(&mut host, "create_element", json!({ "diagram": d, "tool": "state" }));
        let args = json!({ "diagram": d, "tool": "transition", "source": state,
                           "target": other["element"], "name": "e8 [x > 0] / z++" });
        let line = run(&mut host, "connect", args);
        let transition = |host: &Headless| {
            let e = host.document().model().get(id(&line["element"]).parse().unwrap()).unwrap();
            let ElementKind::Transition(p) = e.kind() else { panic!() };
            (e.name().to_owned(), p.trigger.clone(), p.guard.clone(), p.effect.clone())
        };
        let parts = |a: &str, b: &str, c: &str| (String::new(), a.into(), b.into(), c.into());
        assert_eq!(transition(&host), parts("e8", "x > 0", "z++"));
        run(&mut host, "update_element", json!({ "id": line["element"], "text": "e9 / z--" }));
        assert_eq!(transition(&host), parts("e9", "", "z--"));
    }

    #[test]
    fn submachine_states_points_and_stubs() {
        let mut host = Headless::new();
        let car =
            run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "Car" }));
        let car = id(&car["diagram"]);
        let engine =
            run(&mut host, "create_diagram", json!({ "kind": "state_machine", "name": "Engine" }));
        let engine = id(&engine["diagram"]);
        let make = |host: &mut Headless, tool: &str, extra: Value| {
            let mut args = json!({ "diagram": car, "tool": tool });
            args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            run(host, "create_element", args)
        };
        let drive =
            make(&mut host, "submachineState", json!({ "name": "Drive", "x": 200, "y": 100 }));
        let parked = make(&mut host, "state", json!({ "name": "Parked", "x": 40, "y": 100 }));
        let entry =
            make(&mut host, "entryPoint", json!({ "name": "start", "on": drive["presentation"] }));
        let exit =
            make(&mut host, "exitPoint", json!({ "name": "stop", "on": drive["presentation"] }));
        let stub =
            make(&mut host, "stubState", json!({ "name": "Halt", "on": drive["presentation"] }));
        // On a state too low for two points a side, they go to the middle of its sides, not
        // side by side along the top, where their names would run into each other.
        let busy = make(&mut host, "state", json!({ "name": "Busy", "x": 500, "y": 100 }));
        let resume =
            make(&mut host, "entryPoint", json!({ "name": "resume", "on": busy["presentation"] }));
        let abort =
            make(&mut host, "exitPoint", json!({ "name": "abort", "on": busy["presentation"] }));
        // Away from every state an entry point goes on the diagram's frame.
        let outer = make(&mut host, "entryPoint", json!({ "name": "in" }));
        // A stub state outside a submachine state is refused.
        let lone = call(
            &mut host,
            &Access::default(),
            "create_element",
            &json!({ "diagram": car, "tool": "stubState", "on": parked["presentation"] }),
        );
        assert!(lone.is_error);
        for (source, target) in [(&parked, &entry), (&exit, &parked), (&outer, &parked)] {
            run(
                &mut host,
                "connect",
                json!({ "diagram": car, "tool": "transition", "source": source["element"],
                        "target": target["element"] }),
            );
        }
        run(&mut host, "update_element", json!({ "id": drive["element"], "submachine": engine }));
        for wrong in [
            json!({ "id": parked["element"], "submachine": engine }),
            json!({ "id": drive["element"], "submachine": car }),
            json!({ "id": drive["element"], "submachine": parked["element"] }),
        ] {
            assert!(
                call(&mut host, &Access::default(), "update_element", &wrong).is_error,
                "{wrong}"
            );
        }
        let model = host.document().model();
        model.check().unwrap();
        let element =
            |value: &Value| model.element(parse_id(&id(&value["element"])).unwrap()).unwrap();
        let machine_of = |diagram: &str| model.element(parse_id(diagram).unwrap()).unwrap().owner();
        let ElementKind::State(p) = element(&drive).kind() else { panic!() };
        assert_eq!(p.submachine, machine_of(&engine));
        for (point, kind) in [
            (&entry, PseudostateKind::EntryPoint),
            (&exit, PseudostateKind::ExitPoint),
            (&stub, PseudostateKind::StubState),
        ] {
            let ElementKind::Pseudostate(p) = element(point).kind() else { panic!() };
            assert_eq!(p.pseudostate, kind);
            assert_eq!(element(point).owner(), Some(element(&drive).id()));
        }
        assert_eq!(element(&outer).owner(), machine_of(&car));
        // Points sit on the state's border, the exit point on the right; the stub inside.
        let view = model.view(parse_id(&car).unwrap()).unwrap();
        let bounds = |value: &Value| {
            view.get(parse_id(&id(&value["presentation"])).unwrap()).unwrap().bounds().unwrap()
        };
        let r = bounds(&drive);
        assert!((bounds(&entry).center().x - r.x).abs() < 0.5);
        assert!((bounds(&exit).center().x - r.right()).abs() < 0.5);
        assert!(r.contains_rect(&bounds(&stub)));
        let r = bounds(&busy);
        assert_eq!(bounds(&resume).center(), Point::new(r.x, r.center().y));
        assert_eq!(bounds(&abort).center(), Point::new(r.right(), r.center().y));
    }

    #[test]
    fn state_invariants_terminations_and_constraints() {
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "sequence", "name": "Timing" }));
        let d = d["diagram"].clone();
        let lifeline = |host: &mut Headless, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": "lifeline", "name": name }))
        };
        let (shop, store) = (lifeline(&mut host, "shop"), lifeline(&mut host, "store"));
        let send = |host: &mut Headless, name: &str| {
            let m = json!({ "diagram": d, "tool": "asyncMessage", "source": shop["element"],
                            "target": store["element"], "name": name });
            run(host, "connect", m)
        };
        let (check, save) = (send(&mut host, "check"), send(&mut host, "save"));
        let invariant = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "stateInvariant", "name": "x > 0",
                    "on": store["element"] }),
        );
        assert_eq!(invariant["kind"], "stateInvariant");
        assert_eq!(invariant["name"], "x > 0");
        // A longer condition wraps, and its box grows to hold it.
        let waiting = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "stateInvariant", "name": "waiting for the store",
                    "on": shop["element"], "y": 200 }),
        );
        run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "termination", "on": store["presentation"], "y": 60 }),
        );
        // A lifeline ends once, and off a lifeline nothing goes.
        let again = json!({ "diagram": d, "tool": "termination", "on": store["element"] });
        assert!(call(&mut host, &Access::default(), "create_element", &again).is_error);
        let nowhere = json!({ "diagram": d, "tool": "stateInvariant" });
        assert!(call(&mut host, &Access::default(), "create_element", &nowhere).is_error);
        let duration = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "durationConstraint", "source": check["element"],
                    "target": save["element"], "name": "d..3*d" }),
        );
        assert_eq!(duration["kind"], "durationConstraint");
        let time = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "timeConstraint", "source": save["element"] }),
        );
        run(&mut host, "update_element", json!({ "id": time["element"], "name": "t..t+3" }));
        // Constraints go on messages only.
        let wrong = json!({ "diagram": d, "tool": "timeConstraint", "source": shop["element"] });
        assert!(call(&mut host, &Access::default(), "connect", &wrong).is_error);
        host.document().model().check().unwrap();

        let shown = run(&mut host, "get_diagram", json!({ "diagram": d }));
        let tools = shown["element_tools"].as_array().unwrap();
        assert!(tools.contains(&json!("stateInvariant")) && tools.contains(&json!("termination")));
        let tools = shown["relationship_tools"].as_array().unwrap();
        assert!(tools.contains(&json!("durationConstraint")));
        let node = |kind: &str| -> Value {
            let nodes = shown["nodes"].as_array().unwrap();
            nodes.iter().find(|n| n["kind"] == kind).cloned().unwrap()
        };
        assert_eq!(node("stateInvariant")["lifeline"], store["element"]);
        assert_eq!(node("termination")["lifeline"], store["element"]);
        assert_eq!(
            node("durationConstraint")["messages"],
            json!([check["element"], save["element"]])
        );
        assert_eq!(node("timeConstraint")["messages"], json!([save["element"]]));
        assert_eq!(node("timeConstraint")["name"], "t..t+3");
        let of = |element: &Value| -> Value {
            let nodes = shown["nodes"].as_array().unwrap();
            nodes.iter().find(|n| n["element"] == *element).cloned().unwrap()
        };
        let num = |v: &Value, key: &str| v["bounds"][key].as_f64().unwrap();
        let tall = of(&waiting["element"]);
        assert!(num(&tall, "height") >= 21.0 + 15.0, "{tall}");
        // Three lines (by the estimated widths): 15.09 each and 5.
        assert_eq!(num(&tall, "height"), 3.0 * 15.093_75 + 5.0);
        assert_eq!(num(&node("stateInvariant"), "height"), 21.0);
        // The frame holds the time constraint's label: it starts where the line ends, 30 on
        // from the arrowhead (the node's left is 5 before it).
        let frame = of(&d);
        let text = umlcraft_notation::metrics::java_width("{t..t+3}", 12.0, false).unwrap();
        let label_end = num(&node("timeConstraint"), "x") + 5.0 + 30.0 + f64::from(text);
        assert!(num(&frame, "x") + num(&frame, "width") >= label_end + 10.0, "{frame}");
        // The cross ends the lifeline below everything on it, the invariant included; the
        // duration's line reaches from one message to the other.
        let bottom = |v: &Value| {
            v["bounds"]["y"].as_f64().unwrap() + v["bounds"]["height"].as_f64().unwrap()
        };
        let cross = &node("termination")["bounds"];
        let cross_y = cross["y"].as_f64().unwrap() + 10.0;
        assert!(cross_y > bottom(&node("stateInvariant")), "{cross}");
        let line_y = |m: &Value| {
            let lines = shown["lines"].as_array().unwrap();
            lines.iter().find(|l| l["element"] == m["element"]).unwrap()["y"].as_f64().unwrap()
        };
        let span = &node("durationConstraint")["bounds"];
        assert!((span["y"].as_f64().unwrap() - line_y(&check)).abs() < 1.0, "{span}");
        assert!(bottom(&node("durationConstraint")) >= line_y(&save) - 1.0, "{span}");
        // Deleting a message takes its constraints along.
        run(&mut host, "delete_element", json!({ "id": save["element"] }));
        let model = host.document().model();
        model.check().unwrap();
        let left = model.elements().filter(|e| {
            matches!(e.kind(), ElementKind::DurationConstraint(_) | ElementKind::TimeConstraint(_))
        });
        assert_eq!(left.count(), 0);
    }

    #[test]
    fn association_classes_constraints_shared_generalizations_and_robustness_icons() {
        use umlcraft_model::SharedStyle;
        let mut host = Headless::new();
        let d = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "School" }));
        let d = d["diagram"].clone();
        let class = |host: &mut Headless, tool: &str, name: &str| {
            run(host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
        };
        let student = class(&mut host, "class", "Student");
        let course = class(&mut host, "class", "Course");
        let person = class(&mut host, "class", "Person");
        let teacher = class(&mut host, "class", "Teacher");
        let pupil = class(&mut host, "class", "Pupil");
        let screen = class(&mut host, "boundary", "Screen");
        let enrolment = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "associationClass", "source": student["element"],
                    "target": course["element"], "name": "Enrolment" }),
        );
        let generalize = |host: &mut Headless, specific: &Value| {
            let g = json!({ "diagram": d, "tool": "generalization", "source": specific["element"],
                            "target": person["element"] });
            run(host, "connect", g)
        };
        let (g1, g2) = (generalize(&mut host, &teacher), generalize(&mut host, &pupil));
        run(
            &mut host,
            "update_element",
            json!({ "id": enrolment["element"], "constraints": ["{ordered}", "unique"] }),
        );
        run(
            &mut host,
            "update_element",
            json!({ "id": g1["element"], "shared_style": "vertical", "constraints": ["complete"] }),
        );
        run(&mut host, "update_element", json!({ "id": screen["element"], "icon": false }));
        // Only generalizations share, only robustness classes have an icon.
        let wrong = json!({ "id": enrolment["element"], "shared_style": "vertical" });
        assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);
        let wrong = json!({ "id": student["element"], "icon": true });
        assert!(call(&mut host, &Access::default(), "update_element", &wrong).is_error);

        let model = host.document().model();
        model.check().unwrap();
        let id = |v: &Value| v.as_str().unwrap().parse::<Id>().unwrap();
        let association = model.get(id(&enrolment["element"])).unwrap();
        let ElementKind::Association(a) = association.kind() else { panic!() };
        let class = model.get(a.class.expect("the association class")).unwrap();
        assert_eq!(class.name(), "Enrolment");
        assert_eq!(association.data().constraints, ["ordered", "unique"]);
        assert_eq!(model.get(id(&g1["element"])).unwrap().data().constraints, ["complete"]);
        let view = model.view(id(&d)).unwrap();
        for g in [&g1, &g2] {
            let p = view.get(id(&g["presentation"])).unwrap();
            assert_eq!(p.style.shared, SharedStyle::Vertical);
        }
        assert!(!view.get(id(&screen["presentation"])).unwrap().style.iconic);
        assert_eq!(model.get(id(&screen["element"])).unwrap().data().stereotypes, ["boundary"]);
        // The class box and the dashed line to the association's line.
        assert_eq!(model.shown_on(class.id()).len(), 1);
        let class = class.id();
        let element = run(&mut host, "get_element", json!({ "id": enrolment["element"] }));
        assert_eq!(element["properties"]["class"], json!(class));
        assert_eq!(element["properties"]["constraints"], json!(["ordered", "unique"]));
    }

    #[test]
    fn replies_survive_the_trip_through_json() {
        let reply = Reply {
            content: vec![Content::Text("a".into()), Content::Image(vec![1, 2, 3])],
            is_error: true,
        };
        assert_eq!(Reply::from_json(&reply.to_json()), reply);
    }

    #[test]
    fn an_er_diagram_comes_out_as_sql() {
        let mut host = Headless::new();
        let d = run(
            &mut host,
            "create_diagram",
            json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
        );
        let d = id(&d["diagram"]);
        let mut entity = |name: &str, key: &str| {
            let e = run(
                &mut host,
                "create_element",
                json!({ "diagram": d, "tool": "erEntity", "name": name }),
            );
            let e = id(&e["element"]);
            run(&mut host, "add_member", json!({ "owner": e, "text": key, "key": true }));
            e
        };
        let (customer, order) =
            (entity("Customer", "id : INTEGER"), entity("Order", "no : INTEGER"));
        let link = json!({ "diagram": d, "tool": "erNonIdentifying", "source": customer, "target": order });
        run(&mut host, "connect", link);
        let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "postgres" }));
        let sql = out["sql"].as_str().unwrap();
        assert!(sql.contains("CREATE TABLE \"Order\" ("), "{sql}");
        assert!(sql.contains("REFERENCES \"Customer\" (id);"), "{sql}");
        assert_eq!((out["tables"].as_u64(), out["foreign_keys"].as_u64()), (Some(2), Some(1)));
        let class = run(&mut host, "create_diagram", json!({ "kind": "class", "name": "C" }));
        let wrong = call(
            &mut host,
            &Access::default(),
            "export_sql",
            &json!({ "diagram": class["diagram"] }),
        );
        assert!(wrong.is_error);
    }

    #[test]
    fn domains_checks_and_indexes_go_into_the_sql() {
        let mut host = Headless::new();
        let d = run(
            &mut host,
            "create_diagram",
            json!({ "kind": "entity_relationship", "name": "Shop", "notation": "ie" }),
        );
        let d = id(&d["diagram"]);
        let model = host.document().model();
        let package = model.get(parse_id(&d).unwrap()).unwrap().owner().unwrap().to_string();
        let state = run(
            &mut host,
            "create_model_element",
            json!({ "owner": package, "tool": "erDomain", "name": "state" }),
        );
        let state = id(&state["id"]);
        run(&mut host, "update_element", json!({ "id": state, "values": ["open", "paid"] }));
        let order = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erEntity", "name": "orders" }),
        );
        let order = id(&order["element"]);
        let no = run(
            &mut host,
            "add_member",
            json!({ "owner": order, "text": "no : int", "key": true }),
        );
        let status = run(&mut host, "add_member", json!({ "owner": order, "text": "status" }));
        let total = run(&mut host, "add_member", json!({ "owner": order, "text": "total : int" }));
        run(&mut host, "update_element", json!({ "id": id(&no["id"]), "auto_increment": true }));
        let line = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erEntity", "name": "lines" }),
        );
        let line = id(&line["element"]);
        let has = run(
            &mut host,
            "connect",
            json!({ "diagram": d, "tool": "erNonIdentifying", "source": order, "target": line }),
        );
        let has = id(&has["element"]);
        let actions = json!({ "id": has, "on_delete": "cascade", "on_update": "SET NULL" });
        run(&mut host, "update_element", actions);
        let bad = json!({ "id": has, "on_delete": "drop" });
        assert!(call(&mut host, &Access::default(), "update_element", &bad).is_error);
        run(&mut host, "update_element", json!({ "id": id(&status["id"]), "datatype": "state" }));
        run(&mut host, "update_element", json!({ "id": id(&total["id"]), "check": "total >= 0" }));
        let indexes = json!([{ "name": "ix_status", "columns": ["status"] }]);
        run(
            &mut host,
            "update_element",
            json!({ "id": order, "indexes": indexes, "checks": ["no > 0"] }),
        );
        let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "postgres" }));
        let sql = out["sql"].as_str().unwrap();
        assert!(sql.contains("CREATE TYPE state AS ENUM ('open', 'paid');"), "{sql}");
        assert!(sql.contains("status state,"), "{sql}");
        assert!(sql.contains("total int CHECK (total >= 0)"), "{sql}");
        assert!(sql.contains("CHECK (no > 0)"), "{sql}");
        assert!(sql.contains("CREATE INDEX ix_status ON orders (status);"), "{sql}");
        assert!(sql.contains("no int NOT NULL GENERATED BY DEFAULT AS IDENTITY"), "{sql}");
        assert!(
            sql.contains("REFERENCES orders (no) ON DELETE CASCADE ON UPDATE SET NULL;"),
            "{sql}"
        );
        let out = run(&mut host, "export_sql", json!({ "diagram": d, "dialect": "mysql" }));
        let sql = out["sql"].as_str().unwrap();
        assert!(sql.contains("status enum('open','paid'),"), "{sql}");
        assert!(sql.contains("no int NOT NULL AUTO_INCREMENT"), "{sql}");
    }

    /// An ER diagram with the entity `User` and the relationship `refers`, joined twice: a
    /// recursive relationship. Returns the diagram, the relationship's presentation and the
    /// two participations' presentations.
    fn recursive_relationship(host: &mut Headless) -> (String, String, [String; 2]) {
        let d = run(host, "create_diagram", json!({ "kind": "entity_relationship", "name": "R" }));
        let d = id(&d["diagram"]);
        let user = run(
            host,
            "create_element",
            json!({ "diagram": d, "tool": "erEntity", "name": "User", "x": 20, "y": 100 }),
        );
        let refers = run(
            host,
            "create_element",
            json!({ "diagram": d, "tool": "erRelationship", "name": "refers", "x": 260, "y": 95 }),
        );
        let mut join = |cardinality: &str, role: &str| {
            let args = json!({ "diagram": d, "tool": "erLine", "source": refers["element"],
                               "target": user["element"] });
            let line = run(host, "connect", args);
            let args = json!({ "id": line["element"], "cardinality": cardinality, "role": role });
            run(host, "update_element", args);
            id(&line["presentation"])
        };
        let lines = [join("[0,1]", "referrer"), join("[0,n]", "referred")];
        (d, id(&refers["presentation"]), lines)
    }

    #[test]
    fn participations_of_a_recursive_relationship_run_apart() {
        let mut host = Headless::new();
        let (d, _, lines) = recursive_relationship(&mut host);
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let edge = |e: &String| scene.edge(e.parse().unwrap()).unwrap();
        let distance = |p: Point, e: &String| {
            edge(e)
                .points
                .windows(2)
                .map(|w| scene::distance_to_segment(p, w[0], w[1]))
                .fold(f32::INFINITY, f32::min)
        };
        // The middle of each line keeps well away from the other line.
        for (own, other) in [(&lines[0], &lines[1]), (&lines[1], &lines[0])] {
            let (s, t) = umlcraft_notation::route::middle_segment(&edge(own).points, false);
            let middle = Point::new((s.x + t.x) / 2.0, (s.y + t.y) / 2.0);
            assert!(distance(middle, other) > 20.0, "{:?}", edge(own).points);
            // Its cardinality and role stand nearer to it than to the other line.
            for label in &edge(own).labels {
                let width = scene::label_width(&label.text, measure);
                let at = label.anchor.offset(width / 2.0, scene::LABEL_HEIGHT / 2.0);
                assert!(distance(at, own) < distance(at, other), "{} at {at:?}", label.text);
            }
        }
    }

    #[test]
    fn spread_lines_follow_a_moved_node() {
        let mut host = Headless::new();
        // Drawn where it ends up, and drawn elsewhere and moved there (#41).
        let (placed, _, placed_lines) = recursive_relationship(&mut host);
        let d = run(
            &mut host,
            "create_diagram",
            json!({ "kind": "entity_relationship", "name": "moved" }),
        );
        let d = id(&d["diagram"]);
        let user = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erEntity", "name": "User", "x": 20, "y": 100 }),
        );
        let refers = run(
            &mut host,
            "create_element",
            json!({ "diagram": d, "tool": "erRelationship", "name": "refers", "x": 600, "y": 300 }),
        );
        let lines = [(); 2].map(|_| {
            let args = json!({ "diagram": d, "tool": "erLine", "source": refers["element"],
                               "target": user["element"] });
            id(&run(&mut host, "connect", args)["presentation"])
        });
        let args = json!({ "diagram": d, "node": refers["presentation"], "x": 260, "y": 95 });
        run(&mut host, "move_node", args);
        let (doc, measure) = host.parts();
        let points = |d: &String, line: &String| {
            let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
            scene.edge(line.parse().unwrap()).unwrap().points.clone()
        };
        for (moved, placed_line) in lines.iter().zip(&placed_lines) {
            let (got, want) = (points(&d, moved), points(&placed, placed_line));
            assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
            for (g, w) in got.iter().zip(&want) {
                assert!(g.distance(*w) < 1.5, "{got:?} vs {want:?}");
            }
        }
    }

    #[test]
    fn routed_lines_end_on_the_rhombus() {
        let mut host = Headless::new();
        let (d, refers, lines) = recursive_relationship(&mut host);
        run(&mut host, "route_lines", json!({ "diagram": d }));
        let (doc, measure) = host.parts();
        let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
        let rhombus = scene.node(refers.parse().unwrap()).unwrap().rect;
        let c = rhombus.center();
        for line in &lines {
            let points = &scene.edge(line.parse().unwrap()).unwrap().points;
            let end = [points[0], points[points.len() - 1]]
                .into_iter()
                .min_by(|a, b| a.distance(c).total_cmp(&b.distance(c)))
                .unwrap();
            // On the outline: |dx| / (w / 2) + |dy| / (h / 2) = 1.
            let on = (end.x - c.x).abs() / (rhombus.width / 2.0)
                + (end.y - c.y).abs() / (rhombus.height / 2.0);
            assert!((on - 1.0).abs() < 0.02, "{end:?} on {rhombus:?}: {on}");
        }
    }

    #[test]
    fn er_diagrams_arrange_to_the_right_by_default_and_down_when_asked() {
        let mut host = Headless::new();
        let d = run(
            &mut host,
            "create_diagram",
            json!({ "kind": "entity_relationship", "name": "arrange" }),
        );
        let d = id(&d["diagram"]);
        let mut make = |tool: &str, name: &str| {
            run(&mut host, "create_element", json!({ "diagram": d, "tool": tool, "name": name }))
        };
        let person = make("erEntity", "Person");
        let vaccine = make("erEntity", "Vaccine");
        let gets = make("erRelationship", "gets");
        for (s, t) in [(&person, &gets), (&gets, &vaccine)] {
            let args = json!({ "diagram": d, "tool": "erLine", "source": s["element"],
                               "target": t["element"] });
            run(&mut host, "connect", args);
        }
        let centers = |host: &mut Headless| {
            let (doc, measure) = host.parts();
            let scene = scene::build(doc.model(), d.parse().unwrap(), &Preview::default(), measure);
            [&person, &gets, &vaccine]
                .map(|n| scene.node(id(&n["presentation"]).parse().unwrap()).unwrap().rect.center())
        };
        for direction in [None, Some("right"), Some("down")] {
            let mut args = json!({ "diagram": d });
            if let Some(direction) = direction {
                args["direction"] = json!(direction);
            }
            run(&mut host, "arrange_diagram", args);
            let [p, g, v] = centers(&mut host);
            if direction == Some("down") {
                assert!(p.y < g.y && g.y < v.y, "{direction:?}: {p:?} {g:?} {v:?}");
                assert!((p.x - g.x).abs() < 1.0 && (g.x - v.x).abs() < 1.0);
            } else {
                assert!(p.x < g.x && g.x < v.x, "{direction:?}: {p:?} {g:?} {v:?}");
                assert!((p.y - g.y).abs() < 1.0 && (g.y - v.y).abs() < 1.0, "{p:?} {g:?} {v:?}");
            }
        }
    }
}
