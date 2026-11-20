//! The tools' descriptions and input schemas, and the arguments each one takes.

use super::*;

/// The description of `create_structogram`, with the tree it takes.
pub(super) const STRUCTOGRAM_FORMAT: &str = "Creates a structogram (Nassi-Shneiderman diagram) from a \
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
default (label `%` for none); a parallel element has at most 99 branches. Elements may have `comment`, `disabled` and `color` (six hex \
digits). `kind` of the root: program (default), sub for a method, includable; `keywords`: \
english (default) or german (`für`, `solange`, `bis`, `gib`).";

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
    // The route of a line, which `connect` and `update_element` take.
    let point = json!({ "type": "array", "items": { "type": "number" }, "minItems": 2,
                        "maxItems": 2 });
    let anchor = |which: &str| {
        json!({ "type": ["array", "null"], "items": { "type": "number" }, "minItems": 2,
                "maxItems": 2,
                "description": format!("Lines: where the line leaves or enters its {which} \
                                        node, relative to it: [0, 0] top left, [1, 1] bottom \
                                        right, [0.5, 0] the middle of its top; null for its \
                                        center.") })
    };
    let route = json!({
        "waypoints": { "type": "array", "items": point,
                       "description": "Lines: the points the line runs through between its \
                                       ends, e.g. [[290, 50], [70, 50]]; [] for a straight \
                                       line. A route given by hand turns `auto_route` off." },
        "source_anchor": anchor("source"),
        "target_anchor": anchor("target"),
        "line_shape": { "type": "string",
                        "enum": ["line", "right_angle", "curve", "curve_right_angle"],
                        "description": "Lines: straight segments, horizontal and vertical ones \
                                        (the waypoints are the corners), a curve, or a curve \
                                        over right-angled corners." },
        "auto_route": flag("Lines: true lets the router lay the line out at right angles \
                            around the nodes and again when they move (without the keys \
                            above); false keeps the route as it is."),
    });
    // The condition and extension points of an «extend», which `connect` and
    // `update_element` take.
    let extend = json!({
        "condition": text("Extends: when the extension applies, e.g. `device is lent`; \"\" \
                           for none."),
        "extension_points": { "type": "array", "items": { "type": "string" },
                              "description": "Extends: the extension points of the extended \
                                              use case (the target) where it applies, by id \
                                              or name; `add_member` on the use case adds \
                                              them." },
    });
    // The settings of classes and objects, which `create_element` and `update_element` take.
    let classes = json!({
        "abstract": flag("Classes, interfaces, operations: abstract (the name in italics)."),
        "classifier": text("Objects: the class they are an instance of (`rex : Hund`); \
                            lifelines: the class or actor they stand for (`k : Kunde`), an \
                            actor drawing the lifeline as a stick figure. By id or name; \"\" \
                            for none."),
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
        "simple_states": flag("State machine diagrams: a state with no activities, internal \
                               transitions, regions or substates shows only its name, \
                               centered, as UML draws a simple state; false by default (a \
                               line below the name, as Astah draws every state)."),
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
                             look-across, e.g. [0,1], [1,n], [0,m]; \"\" for none. The \
                             maximum is a number or one letter for many: n (also * or N), m \
                             for the second many, any other to tell more apart ([0,p], \
                             [1,q]), kept in lowercase. Chen diagrams with `ratios` show its \
                             maximum alone (1, n, m, q)."),
        "cardinality_here": text("ER participations of relationships of one or of three and \
                                  more entities: how often each instance of the entity takes \
                                  part, read at the entity, e.g. (1,n), as Chen diagrams with \
                                  `look_here` show it. Between two entities that is the other \
                                  participation's `cardinality`."),
        "role": text("ER participations: the entity's role, when it takes part twice."),
        "physical_name": text("ER entities and attributes: the name in the database."),
        "unique": flag("ER attributes: the column is UNIQUE."),
        "foreign_key": text("ER attributes: the id of the relationship or subtyping whose \
                             foreign key the column is (IE, IDEF1X: marked (FK)), or \"\"."),
        "discriminator": text("IS-A: the id of the supertype's attribute that tells the \
                               subtypes apart, or \"\"."),
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
    // The settings of ER diagrams, which `update_element` takes.
    let er_diagram = json!({
        "notation": { "type": "string", "enum": ["chen", "ie", "idef1x"],
                      "description": "ER diagrams: how they are drawn. Switching converts what \
                                      the diagram shows and keeps its entities in place: IE \
                                      and IDEF1X list the attributes in the tables and draw \
                                      relationships between two entities as lines." },
        "er_display": { "type": "string", "enum": ["attributes", "keys", "name"],
                        "description": "ER diagrams in IE or IDEF1X: what the tables list." },
        "physical_names": flag("ER diagrams in IE or IDEF1X: the names in the database."),
        "look_here": flag("ER diagrams in Chen: show cardinalities read at their entity, \
                           (min,max) of how often each instance takes part, instead of \
                           look-across. Turns `ratios` off."),
        "ratios": flag("ER diagrams in Chen: show cardinalities as Chen's ratios (1:1, 1:n, \
                        n:m), the maximum of each participation's `cardinality` alone \
                        ([1,1] as 1, [0,n] as n, [1,q] as q), instead of [min,max]. Turns \
                        `look_here` off."),
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
             lines with their element, ends and route (`waypoints`, `source_anchor`, \
             `target_anchor`, `line_shape`, `auto_route`); and the tools its kind offers for \
             `create_element` and `connect`. Sequence diagrams: messages with their height \
             `y`, fragments with the range of each operand, and the activation bars of each \
             lifeline (`bars`: `presentation`, `lifeline`, `top`, `bottom`), which `move_node` \
             moves and stretches and `remove_from_diagram` removes. Notes with their `text`, \
             use cases with their `extension_points`.",
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
            "set_display",
            "Sets how diagrams are drawn beyond what the project stores, for drawing, sizing \
             boxes and exporting alike, so that diagrams look the same wherever they are made. \
             All off, they look as in Astah. In the app these are the user's settings, which it \
             keeps; without the app the session starts with the app's saved settings. Omitted \
             flags stay; the reply gives all of them.",
            json!({
                "visibility_column": flag("Members show their visibility (+ − # ~) in a column \
                                           of its own, so that all names of a compartment start \
                                           in one line, and a minus as wide as a plus."),
                "abstract_stereotype": flag("Abstract classes show «abstract» above their \
                                             name instead of {abstract} below it."),
                "read_only_constraint": flag("Read-only attributes end in {readOnly}."),
                "guillemets": flag("Stereotypes and keywords stand in guillemets, as UML \
                                    writes them: «include», «interface», «create» instead of \
                                    <<include>>."),
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
                    "name": text("Its name; a free default name if omitted. A `comment` \
                                  (note) shows it as its text, lines joined with \\n."),
                    "x": number("Left edge; without `y`, the top is that of a free spot (a \
                                 lifeline's `x` alone places it)."),
                    "y": number("Top edge; without `x`, the left edge is that of a free spot."),
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
             with `update_element`), which columns take by its name as their datatype, or \
             `extensionPoint` for an extension point of the use case `owner`.",
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
             (e.g. `t..t+3`). A line other than a message may be given its route by hand \
             (`waypoints`, anchors, `line_shape`) instead of the one UmlCraft lays out, e.g. \
             a recursive relationship around its entity. An `extend` runs from the extending \
             use case to the extended one and may take its `condition` and \
             `extension_points`. A `constraint` (class diagrams) joins two associations, \
             `source` and `target`, with a dashed line between the middles of their lines; \
             `name` is the constraint, `xor` unless given (drawn `{xor}`).",
            with(
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
                &with(route.clone(), &extend),
            ),
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
             before), else an internal transition `trigger [guard] / effect`. Use cases take \
             extension points, by name (`Device lent`), which they list below their name and \
             «extend»s name in `extension_points`.",
            json!({
                "owner": id("The class, interface, enumeration or data type, the ER entity, \
                             the state, or the use case."),
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
             stands for; a state's entry, do and exit activities and its orthogonal regions \
             (`regions`; `get_diagram` gives their bounds, a state created at `x`, `y` in one \
             goes into it); the `trigger [guard] / \
             effect` of a transition or internal transition (as `text`); the constraints of an element (members show them after their text, \
             lines below their name); on every diagram showing it, the shared style of a \
             generalization (one triangle for all generalizations to its general element) \
             and whether a class «boundary», «control» or «entity» is drawn as its icon; \
             whether a sequence diagram shows message numbers, return values and its frame; \
             whether a state machine diagram draws simple states with their name only; \
             the ends of associations and links (role, multiplicity, navigability, \
             aggregation); abstract classes and operations, static attributes and operations; \
             an object's class and the values of its slots; the settings of ER entities, attributes, relationships, IS-As and participations \
             (weak, key, cardinality, …); the route of a line (`waypoints`, anchors, \
             `line_shape`, `auto_route`, as `get_diagram` lists them), also of a line given \
             by its presentation, such as a note anchor; the condition and extension points of \
             an extend; the text of a comment (note).",
            with(
                json!({
                    "id": id("The element."),
                    "name": text("New name."),
                    "text": text("Attributes and operations: the whole member as text; \
                                  transitions and internal transitions: `trigger [guard] / \
                                  effect`; comments (notes): the text they show, lines \
                                  joined with \\n (`name` sets it too)."),
                    "entry": text("States: the entry activity, without `entry /`; \"\" for none."),
                    "do": text("States: the do activity, without `do /`; \"\" for none."),
                    "exit": text("States: the exit activity, without `exit /`; \"\" for none."),
                    "regions": { "type": "integer", "minimum": 2,
                                 "description": "States: divide into this many orthogonal \
                                                 regions (more than it has; `delete_element` \
                                                 removes one). Its substates go into the region \
                                                 they are drawn in. The reply lists the \
                                                 regions." },
                    "side_by_side": flag("States divided into regions for the first time: the \
                                          regions side by side instead of one below the other."),
                    "documentation": text("New documentation."),
                    "stereotypes": { "type": "array", "items": { "type": "string" } },
                    "guard": text("Messages, operands, flows, transitions: the guard, without brackets."),
                    "sort": { "type": "string",
                              "enum": ["synchronous", "asynchronous", "reply", "create", "destroy"] },
                    "arguments": { "type": ["string", "null"],
                                   "description": "Messages: the arguments, written between the \
                                                   parentheses; null for no parentheses, as a \
                                                   signal is written (an asynchronous message \
                                                   named without them has none)." },
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
                                              or entity: drawn as the robustness icon. \
                                              Lifelines standing for an actor or such a \
                                              class: the head drawn as its icon (an actor's \
                                              is on by default)." },
                }),
                &with(
                    with(with(with(activity_node.clone(), &er), &er_diagram), &sequence),
                    &with(
                        with(with(with(ends.clone(), &classes), &route), &extend),
                        &json!({
                            "static": flag("Attributes and operations: static (underlined)."),
                            "diagram": id("Lines: the diagram whose drawing of the line takes \
                                           the route; needed only when several diagrams show \
                                           it."),
                        }),
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
             `get_diagram` gives the heights of messages and operands. An activation bar (its \
             presentation from `get_diagram`) takes a new top `y` and `height`, e.g. one bar \
             from a lifeline's first message to its last; bars it then covers stay nested in \
             it until `remove_from_diagram` removes them, and messages added later start their \
             own bars as usual. A box given a `width` or `height` keeps that size instead of \
             fitting its content.",
            json!({
                "diagram": id("The diagram."),
                "node": id("The element or its presentation: a node, a message, or an \
                            activation bar's presentation."),
                "x": number("Left edge; kept if omitted. Not for activation bars."),
                "y": number("Top edge, or a message's height."),
                "width": number("New width. Not for activation bars."),
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
            "Removes a node or line from a diagram, or an activation bar of a sequence diagram; \
             the element stays in the model.",
            json!({
                "diagram": id("The diagram."),
                "id": id("The element or its presentation (bars: from `get_diagram`)."),
            }),
            &["diagram", "id"],
        ),
        tool(
            "batch",
            "Runs several tools one after the other as one change, which one undo takes back. \
             If one fails, the model is not changed (files exported before stay). A string \
             argument `$2.element` stands for the field `element` of the reply of the call at \
             index 2 (from 0), so later calls can use what earlier ones created; a leading \
             `$$` is a plain `$` (`$$0` is the text `$0`). Not for undo, \
             redo, batch, open_file, new_document, save_file and set_display.",
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
            "Saves the project as .ucraft: to `path` in an allowed folder (making the folders \
             on the way that are missing), or to its own file.",
            json!({ "path": text("Where to save; the project's file if omitted.") }),
            &[],
        ),
        tool(
            "export_image",
            "Writes a diagram or structogram as an SVG or PNG image to an allowed folder (by the \
             extension); a structogram also as a Structorizer file (.nsd). A very large \
             diagram comes out as a smaller PNG. An existing file is replaced only with \
             `overwrite`.",
            json!({
                "diagram": id("The diagram or structogram."),
                "path": text("The file: .svg, .png, or .nsd for a structogram."),
                "overwrite": flag("Replace the file if it exists."),
            }),
            &["diagram", "path"],
        ),
        tool(
            "export_sql",
            "Writes the entities of an ER diagram as CREATE TABLE statements: relationships \
             become foreign keys, many-to-many relationships tables of their own, subtypes take \
             their supertype's key. Returns the SQL, or writes it to `path` in an allowed \
             folder (an existing file only with `overwrite`).",
            json!({
                "diagram": id("The ER diagram; the one shown if omitted."),
                "dialect": { "type": "string", "enum": ["mysql", "mariadb", "postgres"],
                             "description": "The SQL dialect; the one the diagram was \
                                             imported from if omitted, else mysql." },
                "path": text("The .sql file (`.sql` is added if missing); the SQL comes back \
                              in the reply if omitted."),
                "overwrite": flag("Replace the file if it exists."),
            }),
            &[],
        ),
    ]
}

/// The arguments each tool's schema names, by tool.
pub(super) fn known_arguments() -> &'static HashMap<String, Vec<String>> {
    static KNOWN: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    KNOWN.get_or_init(|| {
        list()
            .iter()
            .filter_map(|tool| {
                let name = tool["name"].as_str()?.to_owned();
                let properties = tool["inputSchema"]["properties"].as_object()?;
                Some((name, properties.keys().cloned().collect()))
            })
            .collect()
    })
}

/// Why `args` do not suit tool `name`: an argument its schema does not name, which would
/// otherwise be ignored without a word (a misspelt key, or a setting the tool does not have).
pub(super) fn unknown_arguments(name: &str, args: &Value) -> Option<String> {
    let known = known_arguments().get(name)?;
    let unknown: Vec<&str> = args
        .as_object()?
        .keys()
        .map(String::as_str)
        .filter(|k| !known.iter().any(|n| n == k))
        .collect();
    let list = |keys: &[&str]| keys.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
    match unknown[..] {
        [] => None,
        [key] => Some(format!("`{name}` has no argument `{key}`")),
        _ => Some(format!("`{name}` has no arguments {}", list(&unknown))),
    }
    .map(|why| {
        let mut known: Vec<&str> = known.iter().map(String::as_str).collect();
        known.sort_unstable();
        format!("{why}; it takes {}", list(&known))
    })
}
