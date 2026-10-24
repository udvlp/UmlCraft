# Architecture

## Model and view

Like UML (and Astah), `umlcraft-model` separates **elements** from **presentations**:

- An `Element` (class, association, lifeline, diagram …) has an owner and ordered children. The
  root is exactly one `ElementKind::Model`. `ElementKind::can_own` defines which kind may own which.
- References between elements (`ElementKind::references`) have a role (`RefRole`). **Strong**
  references, such as the ends of an association, delete the referencing element along with the
  target. **Weak** references, such as attribute types, are cleared; a type then becomes free text
  holding the old name.
- A type (`TypeRef`) is a classifier or template parameter, free text, or an expression built from
  other types (`TypeExpr`: base, template arguments, a prefix such as `? extends ` and a modifier
  such as `[]` or `*`). In `List<Foo>[]` both `List` and `Foo` stay references of their own;
  deleting one turns only that part into text. `member::parse_type` reads the written form and
  links what it finds, as seen from the member: template parameters of the member's owners
  first, then qualified names (`java.util.List`), then a classifier whose name is unique.
- Template parameters (`ElementKind::TemplateParameter`) belong to classes, interfaces, data types
  or operations and stand for a type (with bounds) or a value (with its type, C++'s `int N`),
  optionally with a default and variadic.
- Every diagram element has a `View` with presentations in z-order. A node has `bounds`; an edge
  connects two presentations of the same view.

## Changes

The model changes only through `Model::apply(Op) -> inverse Op`. Every operation is validated
(ownership rules, reference targets, cycles in generalizations and in the ownership tree, attached
edges).

`Document::edit(label, |tx| …)` groups operations into one undo step. If the closure fails,
everything is rolled back. `Tx` offers the composite edits:

- `delete` deletes in cascade: subtree, strongly referencing elements, presentations and attached
  edges.
- `remove_presentation` removes something from the diagram only.
- `set_bounds`, `bring_to_front` etc. change presentations.

`Document::is_dirty` compares the topmost undo step with the save point.

## File format

`.ucraft` is pretty-printed JSON. Elements are sorted by ID (UUIDv7, i.e. by creation time), so new
elements end up at the end. Ownership is stored only through the ordered `children`. Default values
are omitted. `formatVersion` and the `MIGRATIONS` list in `umlcraft-format` handle future format
changes. On load, `Model::check` verifies all invariants. Version 2 added the geometry of
sequence diagrams; version 1 files load unchanged and their sequence diagrams are laid out once
when the app opens them (`sequence_edit::upgrade_document`, not an undoable edit).

## User interface

`umlcraft-app` uses `gpui-kit` 0.8, which brings the matching GPUI release (`gpui-pre` 0.3.9) and
`gpui-component`:

- `Workspace` (a GPUI entity) holds the document, file path, open diagrams (tabs), selection and
  the active tool. All edits from the UI go through its methods. Each of them ends by fitting
  the classifier boxes sized to their content (`Style::auto_size`) whose texts the edit changed
  (`scene::fitted_box`), as Astah does when content changes; for that the workspace measures
  with the app's font system (`AppMeasure`, the advances summed like Java's). Resizing a box by
  hand ends its automatic size.
- `canvas.rs` paints the scene's primitives (`umlcraft_notation::draw`) and texts with
  `paint_quad`/`PathBuilder`/`shape_line` and handles pan and
  zoom itself, separately for each diagram. Drag operations (move, resize, rubber band, connect)
  are shown only as a `Preview` until the mouse is released and are then applied as one undo step.
  A right click selects what it hits and opens the context menu: for an association Astah's
  navigability, aggregation and multiplicity per end, for edges the line style, ends back to
  the centers and labels back to their default places, then deleting.
- `properties.rs` commits text fields on Enter or when the field loses focus, always to the element
  that was shown when the field got focus.
- `main_view.rs` contains menus, tabs, palette, model tree and the file operations.
- `png.rs` rasterizes the SVG export (`umlcraft_notation::svg`) with `resvg` for PNG export, at
  twice the canvas size and with the system's fonts, on a background thread.
- `settings.rs` stores recent files, language and theme in `%APPDATA%\UmlCraft\settings.json`.
- Texts come from `locales/{de,en}.yml` (rust-i18n). Without a setting, the system language is used.

## Notation

`umlcraft-notation` knows UML notation but no UI:

- `scene` turns a view into shapes in logical coordinates. Each node carries its text rows (`Row`)
  with position, font and the element the row edits. Painting, hit testing and inline editing share
  the same layout. Nodes grow with their content; text width is measured by a `TextMeasure` (the
  real font in the app, an estimate in tests). `scene` also places edge labels and provides
  resize handles, edge handles (waypoints), rubber-band selection and alignment. Containers (system
  boundaries, packages) graphically contain whatever lies fully inside them (`with_contents`);
  system boundaries are "hollow" and clickable only on their border and title.
- `route` runs edges in Astah's line styles (`Style::line_shape`): straight through the
  waypoints, at right angles with the waypoints as corners, or as one cubic Bézier curve whose
  control points are the first and the last waypoint. Right-angled lines are fitted when drawn,
  so their first and last segment stay straight while nodes move or grow; the drawn corners are
  what gets dragged (a segment moves across, the end segments slide their anchor inside the
  node) and exported. Changing the style changes the route as Astah does (`scene::restyle`):
  "line" drops the waypoints, right angles turn them into corners.
- `member` produces and parses the text form of attributes and operations and writes it back to
  the model; known classifiers become real type references.
- `clipboard` copies the selection including subtrees and the relationships among them, and pastes
  it as a duplicate (new IDs, free names) or as another presentation of the same elements.
- `palette` defines which tools a diagram type offers, which connections are allowed (e.g.
  realization only from class to interface) and where new elements land in the ownership tree.
- `sequence` draws sequence diagrams, `sequence_edit` edits them (see below).
- `draw` turns a scene's figures and end markers into a few primitives (polyline, polygon,
  ellipse, frame) with color roles (`Paint`) and says where text baselines go. The canvas paints
  them with its theme colors (and a node's own fill color on a light background); `svg` writes
  them as Astah exports SVG: black lines, Astah's yellow `#FFFFCC` where a node has no color of
  its own, a margin of 10 and 4/3 pixels per unit. So the figure geometry exists once, for both.
  Texts in nodes are centered by Java's whole-pixel width (`TextMeasure::rounded_width`, from
  the table in `metrics`), as Astah does, and written at a fixed x.

## Sequence diagrams

Sequence diagrams store their whole geometry, the way Astah does, so that they look like Astah's
and export to Astah and back without change:

- The diagram frame ("sd" and the name in a tab) is a node presentation of the diagram itself.
- A lifeline is `Shape::Lifeline`: its head (a box, or with `Style::iconic` the icon of the
  represented classifier's robustness stereotype) and the length of its dashed line, which can
  end in a cross (`destroyed`).
- An activation bar is `Shape::Activation`: a lifeline presentation, a top and a height. Its x
  follows from the lifeline and from how many bars on it it lies within (10 further right each).
- A message is `Shape::Message`: the two lifeline presentations, the height where it leaves the
  sender and, for messages to self and messages that branch off another one, where it arrives.
  Found and lost messages keep the x of their open end (`gate_x`); on the frame's border it is a
  gate without a dot. The ends meet the bars at those heights.
- A combined fragment is `Shape::Fragment` with its bounds, the offsets of the operand
  separators and hand-placed guard offsets; an interaction use is a node presentation.
- Message numbers ("2.1") are not stored: a message sent from a bar is numbered below the call
  that started the bar; messages leaving the same point branch ("-A", "-B"). A reply's return
  value is shown after the call it answers, as Astah does. Both can be switched off per diagram
  (`DiagramProps::hide_message_numbers`, `hide_return_values`).

The interaction's order follows the drawing: `sequence_edit::tidy`, run after every edit of a
sequence diagram, grows bars to end 20 below what lies in them and lifelines to reach 30 below
what is on them, grows the frame around everything, and puts every message and fragment into the
operand of the innermost fragment drawn around it, ordered by height (`sync_order`). Edits follow
Astah's rules, measured through its API: a call gets a bar of 25 on the receiver and, where the
sender has none, a bar of 45 on the sender (60 for create, 30 for lost messages); a reply leaves
at the end of the bar it returns from; a message to self comes back 20 lower into a bar of its
own; a create message brings the new lifeline's head down to it; a destroy message ends the
lifeline in a cross below its bar. Dragging a message moves it with the bar it starts and
everything sent from that bar (`move_item`); a message's label can be dragged on its own
(`place_label`). Combined fragments grow to hold the bars and messages that start in them, and
bars that no message reaches any more are removed. The layout's `Slot`s carry the height the
user pointed at.

## Astah import

`umlcraft-asta` reads `.asta` files without any Java: `jser` is a generic reader for the Java
object serialization stream and turns it into a graph of objects addressed by class and field
name; `map` translates that graph into a new `Model`, element by element through `Model::apply`,
so every invariant is checked. What has no counterpart in UmlCraft is skipped and counted
(`Imported::skipped`). The format details are described in [asta-format.md](asta-format.md).
Two examples help when exploring files: `asta-dump` (raw objects) and `asta-import` (the
resulting model).

## Astah export

Writing `.asta` ourselves would mean producing Astah's Java object graph, which is fragile and
changes between Astah versions. So `umlcraft-asta-export` has an installed Astah write the file
through its official API (`astah-api.jar`):

- `Astah::find` looks for an installation (`ASTAH_HOME`, else `astah-professional`, `astah-uml`
  or `astah-sysml` under "Program Files") with `astah-api.jar` and an edition jar. Without one,
  File › Export › Astah project … shows a dialog saying what is needed.
- `Export::new` serializes the model as `.ucraft` and adds a layout sidecar: node bounds and
  edges as drawn (nodes grow with their content). Sequence diagrams need none; the tool reads
  their geometry from the project. The app prepares this on the UI thread and runs the rest in
  the background.
- `Export::run` writes both files and the Java tool [`tools/asta-export/AstaExport.java`](../tools/asta-export/README.md)
  (embedded with `include_str!`) to a temporary folder, and runs the tool with Astah's bundled
  JRE in source-file mode, so no JDK and no build step are needed. The tool creates packages,
  classifiers, features and relationships with `BasicModelEditor` and `UseCaseModelEditor`, then
  the diagrams with the class, use case, sequence and activity diagram editors, all in one
  transaction. It
  reports what the API cannot express (`Report::dropped`) and single failures
  (`Report::problems`).

### How the model maps onto the API

- Astah has no free-text types. A type name that is neither a primitive (`int`, `boolean` …)
  nor a modeled classifier becomes a class in the attribute's package; the API does this on its
  own. Attributes and parameters without a type get `void`, because the API demands a type.
- Astah rejects empty and duplicate names among siblings; such elements get a numbered name and
  are reported as renamed. A dependency with the stereotype «use» becomes a usage.
- Node sizes are set explicitly with Astah's auto-resize turned off. Astah may still grow a node
  to fit it, e.g. a use case with extension points. Nodes without a fill color get Astah's usual
  `#FFFFCC` instead of the API's white. Notes are created per presentation, since Astah keeps
  comments as notes on diagrams; comments that are not on any diagram are dropped.
- Sequence diagrams: Astah puts lifeline heads at y = 30, so the whole drawing moves vertically
  by as much; x stays. Messages are created from top to bottom at their stored height, from the
  Astah bar made for the UmlCraft bar at that height (or the lifeline). Astah makes the bars
  itself by the same rules UmlCraft follows, and refuses to resize them (`setHeight`, reported as
  `barHeight`); a reply (`createReturnMessage`) goes to the end of the bar of the call it
  answers, where UmlCraft has it too. Lifeline lengths are set through `lifeline_length`; Astah
  refuses lengths shorter than what lies on a lifeline, interaction uses included. A reply's
  return value is stored on the call it answers, as Astah does. The round-trip test checks that
  heads and message heights come back unchanged.
- Activity diagrams: `createActivityDiagram` brings its own activity, so an activity exports its
  first diagram only. Astah creates activity nodes only on a diagram; nodes, flows and
  partitions not shown are dropped. Partitions are created left to right with
  `createPartition(superPartition, previous, …)`; Astah wraps top-level lanes in a "dimension"
  header of its own and lines them up below it, so the header is moved above UmlCraft's lanes
  before the lanes are placed. The API creates decision and merge nodes as one kind; the import
  tells them apart by their flows. Unnamed control nodes get Astah-like default names
  (`InitialNode0`), which the import drops again, together with Astah's own (`ForkNode1`) and the
  default dimension (`Dimension0`).
- Component and deployment diagrams: the API creates components, artifacts and nodes
  (`createComponent`, `createArtifact`, `createNode`, in packages only) but has no editors for
  these diagrams. They are exported as class diagrams, which show the same elements; instance
  specifications exist only there (`createInstanceSpecification`), always as objects, placed
  when created (they cannot be moved) and sized by Astah. UmlCraft's class diagrams accept
  components, nodes and instances, so such a file comes back intact apart from the diagram
  kind. A usage (`createUsage`) is an `IUsage`, not an `IDependency`.
- State machine diagrams: `createStatemachineDiagram` brings its own state machine. States are
  created from the outside in, each composite state at its size before its substates go into it
  (`createState(name, parent, …)`). The API gives a transition leaving a state an event called
  "Trigger"; `setEvent(null)` removes it again. Astah keeps room for entry, do and exit, which
  makes a state at least 81 high, so states without them have that compartment hidden
  (`action_visibility`). Astah routes the first and last segment of a self-transition itself.
- Interaction uses are created in the order of the interaction (`createInteractionUse` with the
  covered lifelines, all of them when none are given) at their height and then sized; since
  Astah refers to sequence diagrams, the reference is set once every diagram exists. Found and
  lost messages use `createFoundMessage` and `createLostMessage` at the stored `gate_x`.
- Combined fragments are drawn where UmlCraft has them, outer ones before inner ones
  (`createCombinedFragment(name, operator, location, width, height)`); Astah takes the messages
  inside into the first operand. `addInteractionOperand` moves the messages of each further
  operand into it; Astah refuses that for empty operands and for operators with a single
  operand (reported as `emptyOperand`, `extraOperand`). The separators go to their places
  through `operand.N.length`, guards moved by hand through `guard.N.point.x/y`.
- Message display options: `parameter_visibility`, and labels moved by hand through
  `name.point.x/y`. Astah moves a label by as much as `name.point` differs from its own value,
  so the layout sidecar says where Astah puts each such label by itself (`labelDefaults`: the
  last segment's middle minus half of Java's whole-pixel width, at most 200). The diagram's
  `message_index_visibility` and `message_return_value_initial_visibility` carry
  `DiagramProps::hide_message_numbers` and `hide_return_values`. The API makes bars white;
  they get their UmlCraft fill (`fill.color`, Astah's yellow by default).
- Not available in the API and reported as dropped: the query flag of operations, conditions and
  extension points of extends, return parameters, parameter multiplicity and default values,
  system boundaries.
- Line styles: `line.shape` is set before the points (Astah would otherwise drop or replace
  them); right-angled lines send their corners as drawn (the sidecar's `corners`).
- Edges: `setAllPoints` gets the source anchor, the waypoints and the target anchor as points;
  Astah derives its relative ends (`sourceEndX/Y`, `targetEndX/Y`) from the first and last
  point. Astah stores some links the other way round (a dependency starts at the supplier, a
  generalization at the general class) and rejects points in the wrong order; the current first
  point (`getAllPoints()[0]`) tells which node comes first, `getSource()` does not always.
  Labels placed by hand are set as properties with their position on the diagram (`name.point.x`,
  `end_a.name.point.x`, `end_b.multiplicity.point.y` …); the sidecar turns UmlCraft's offsets into
  those positions. `setProperty` silently ignores keys it does not know. Constraints of
  association ends become constraints on the end.

### License

In a test with Astah Professional 12.0.0 the API ran headless and saved projects without a
license: the copied jars, with no license file next to them and an empty user home
(`-Duser.home`), created and saved a project. The API still declares `LicenseNotFoundException`
on `open` and `save`, so other editions or versions may check. Either way, the export needs an
installed Astah, and Astah's license terms apply to using its API.

The round-trip test (`crates/umlcraft-asta-export/tests/roundtrip.rs`) exports the sample
project, extended by an interface, a dependency, an enumeration, a note, include and extend,
imports the file again with `umlcraft-asta` and compares names, ownership, relationships,
diagram contents and the interactions. It is skipped when Astah is not installed.

## Deviation from the original plan

The plan was `gpui` 0.2.2. `gpui-component` 0.8, however, requires `gpui-pre` 0.3.9, so the app
depends on the bundle crate `gpui-kit`.
