# Architecture

## Model and view

Like UML (and Astah), `umlcraft-model` separates **elements** from **presentations**:

- An `Element` (class, association, lifeline, diagram …) has an owner and ordered children. The
  root is exactly one `ElementKind::Model`. `ElementKind::can_own` defines which kind may own which.
- References between elements (`ElementKind::references`) have a role (`RefRole`). **Strong**
  references, such as the ends of an association, delete the referencing element along with the
  target. **Weak** references, such as attribute types, are cleared; a type then becomes free text
  holding the old name.
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
changes. On load, `Model::check` verifies all invariants.

## User interface

`umlcraft-app` uses `gpui-kit` 0.8, which brings the matching GPUI release (`gpui-pre` 0.3.9) and
`gpui-component`:

- `Workspace` (a GPUI entity) holds the document, file path, open diagrams (tabs), selection and
  the active tool. All edits from the UI go through its methods.
- `canvas.rs` paints the scene's primitives (`umlcraft_notation::draw`) and texts with
  `paint_quad`/`PathBuilder`/`shape_line` and handles pan and
  zoom itself, separately for each diagram. Drag operations (move, resize, rubber band, connect)
  are shown only as a `Preview` until the mouse is released and are then applied as one undo step.
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
- `member` produces and parses the text form of attributes and operations and writes it back to
  the model; known classifiers become real type references.
- `clipboard` copies the selection including subtrees and the relationships among them, and pastes
  it as a duplicate (new IDs, free names) or as another presentation of the same elements.
- `palette` defines which tools a diagram type offers, which connections are allowed (e.g.
  realization only from class to interface) and where new elements land in the ownership tree.
- `sequence` lays out sequence diagrams (see below).
- `draw` turns a scene's figures, end markers and activation bars into a few primitives
  (polyline, polygon, ellipse, frame) with color roles (`Paint`) and says where text baselines
  go. The canvas paints them with its theme colors; `svg` writes them as an SVG image in Astah's
  black on white. So the figure geometry exists once, for both.

## Sequence diagrams

A sequence diagram belongs to an `Interaction`. Vertical positions are never stored; they follow
from the order of the interaction's children, recursing through `CombinedFragment` →
`InteractionOperand` → children. Inserting or moving a message is therefore a `MoveElement` or
`InsertElement` at the right index, and everything below moves along by itself.

- A lifeline is a node presentation; only x and width count. All heads share one top line (the
  smallest stored y), except lifelines created by a create message, whose head sits at that
  message. Moving a lifeline only moves it sideways.
- A message is an edge presentation from the sender's lifeline node to the receiver's, without
  waypoints (`EdgeShape::fixed`). Only messages shown on the diagram take up a row.
- A combined fragment is a node presentation whose stored bounds are not used: its frame spans
  the covered lifelines (or, if none are ticked, those its messages touch) and its operands'
  content. Fragment nodes go to the back.
- Activation bars are derived: a synchronous call opens one on the receiver, the matching reply
  closes it, and a lifeline that sends a new call regains control, which ends what it called
  before without a reply.
- The layout reports `Slot`s, the gaps between items (owner and index plus a y position). The
  canvas uses them to insert new messages and fragments where the user clicked and to move
  dragged messages and fragments (`palette::move_to_slot`).

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
- `Export::new` serializes the model as `.ucraft` and adds a layout sidecar: node bounds as drawn
  (nodes grow with their content) and, for sequence diagrams, the rows of the messages and the
  frames of the fragments, which UmlCraft derives instead of storing. The app prepares this on
  the UI thread and runs the rest in the background.
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
- Sequence diagrams: rows are mapped to Astah's coordinates relative to the bottom of the
  lifeline heads and stretched by 1.25, because Astah spaces messages further apart. Messages are
  created from top to bottom. A message from a lifeline with an open activation is sent from that
  activation, which makes Astah extend it, so a reply (`createReturnMessage`, placed by Astah at
  the end of the activation) ends up below the calls it follows. Activations are tracked the way
  `sequence` derives them. A reply's return value is stored on the call it answers, as Astah
  does.
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
  covered lifelines, all of them when none are given); since Astah refers to sequence diagrams,
  the reference is set once every diagram exists. Found and lost messages use
  `createFoundMessage` and `createLostMessage` with a point 120 px beside the lifeline.
- Combined fragments are built around their messages: `createCombinedFragment(operator,
  messages)` for the first operand, `addInteractionOperand` for the others, outer fragments
  before inner ones. Astah sizes the frames itself and they cannot be moved afterwards, so a
  nested fragment that spans the same lifelines is drawn on top of its parent's border. Empty
  operands are dropped; a fragment without any message gets UmlCraft's frame.
- Not available in the API and reported as dropped: the query flag of operations, conditions and
  extension points of extends, return parameters, parameter multiplicity and default values,
  system boundaries.
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
