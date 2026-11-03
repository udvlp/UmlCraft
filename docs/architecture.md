# Architecture

## Model and view

Like UML, `umlcraft-model` separates **elements** from **presentations**:

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
  connects two presentations of the same view. Free texts (`Shape::Text`) and free lines
  (`Shape::Line`, dashed and with arrowheads as chosen) show no element.
- A structogram (`ElementKind::Structogram`) sits where a diagram may (`can_own` as for
  diagrams) and holds its whole `umlcraft_nsd::Diagram` in content form (see
  [Nassi-Shneiderman diagrams](#nassi-shneiderman-diagrams)), with no view. It may name the
  operation it describes (a weak `RefRole::Operation`: deleting the operation leaves the
  structogram). Its name stays empty; tree and tab show the text of its root.
- An association class is a Class plus an Association whose `class` names it (a weak
  `RefRole::AssociationClass`: deleting the class leaves a plain association). The class keeps
  name, members and constraints; the scene draws the name and constraints on the line and
  derives the dashed line from the class box to the line's middle. Any element may carry
  constraints (`ElementData::constraints`, texts without braces); members and lines show them.

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

`umlcraft_notation::invariants::check` lists the rules a model breaks that every edit should keep:
finite coordinates, lines that start and end at their nodes, right-angled lines without slanted
segments (beyond Astah's 2 px), substates inside the state they belong to (and its region), and
on sequence diagrams bars and messages on their lifelines,
bars nested or apart, operand lines in order, and nothing left for `sequence_edit::tidy` to do.
Astah's files do not always keep them, so what counts is what an edit adds
(`invariants::new_violations`). Debug builds check before and after every `edit::apply` and print
what an edit broke to stderr. The app's `fuzz` test makes random edits the way the mouse makes
them (moving and resizing boxes, bending lines, dragging messages, gates, lifeline ends, labels
and region lines, deleting, undo and redo) on the sample project, the order system and Astah's
sample, and checks the rules and that undo and redo restore the model exactly.
`UMLCRAFT_FUZZ_SEED`, `UMLCRAFT_FUZZ_STEPS` and `UMLCRAFT_FUZZ_DUMP` (a folder for the projects
before and after a failing step) replay and widen a run.

## File format

`.ucraft` is pretty-printed JSON. Elements are sorted by ID (UUIDv7, i.e. by creation time), so new
elements end up at the end. Ownership is stored only through the ordered `children`. Default values
are omitted. `formatVersion` and the `MIGRATIONS` list in `umlcraft-format` handle future format
changes. On load, `Model::check` verifies all invariants. Version 2 added the geometry of
sequence diagrams; version 1 files load unchanged and their sequence diagrams are laid out once
when the app opens them (`sequence_edit::upgrade_document`, not an undoable edit; `load_versioned`
tells the app the file's version). Version 3 keeps the tagged values of languages under
UmlCraft's keys (`java.`, `csharp.`, `cpp.`); the migration renames Astah's `jude.` keys.
Version 4 adds structograms; nothing changes in older files, so its migration does nothing,
but a file with structograms does not open in UmlCraft 0.10 or earlier. A structogram is
stored as structured JSON (`kind`, `text` as a list of lines, branches as lists of items) so
that a diff shows which element changed.

## User interface

`umlcraft-app` uses `gpui-kit` 0.8, which brings the matching GPUI release (`gpui-pre` 0.3.10) and
`gpui-component`:

- `Workspace` (a GPUI entity) holds the document, file path, open diagrams (tabs), selection and
  the active tool. All edits from the UI go through its methods. Each of them ends by fitting
  the classifier boxes sized to their content (`Style::auto_size`) whose texts the edit changed
  (`scene::fitted_box`); for that the workspace measures
  with the app's font system (`AppMeasure`, the advances summed like Java's). Resizing a box by
  hand ends its automatic size.
- `canvas.rs` paints the scene's primitives (`umlcraft_notation::draw`) and texts with
  `paint_quad`/`PathBuilder`/`shape_line` and handles pan and
  zoom itself, separately for each diagram. Drag operations (move, resize, rubber band, connect)
  are shown only as a `Preview` until the mouse is released and are then applied as one undo step.
  A right click selects what it hits and opens the context menu: for an association the
  navigability, aggregation and multiplicity per end, for edges the line style, ends back to
  the centers and labels back to their default places, then deleting.
- `properties.rs` commits text fields on Enter or when the field loses focus, always to the element
  that was shown when the field got focus.
- `main_view.rs` lays out the window and holds the file operations: the title bar (gpui-component's
  `TitleBar` with the File menu, save, undo and redo; the window draws its own title bar), the
  ribbon, model tree and properties. The diagram view fills the window below the title bar and
  the others float over it as cards; the empty middle between them is measured into a
  `shell::ClearArea` that the diagram views use to zoom, center and paste. There is no status
  line: `Workspace::message` (what went wrong in an edit) is taken by an observer and shown as
  a toast (`toast`, gpui-component's notifications), and running work as a card above the zoom
  (`busy_card`). The platform menu bar (`cx.set_menus`) stays for macOS.
- The diagram views open their context menu themselves (`shell::MenuAt`) on releasing the
  right button without moving; held and moved, it pans. gpui-component's `context_menu`
  opens on the press and so cannot tell a click from a drag.
- `ribbon.rs` describes the ribbon as data (tabs, groups, large and small buttons, each with an
  action) built on every render from a `ribbon::State` read once from the workspace, and draws
  it. The diagram tab holds the palette of the active diagram's kind, or for a structogram its
  elements and commands, and opens by itself when another diagram becomes active. Buttons send
  their action to the view shown (canvas or structogram), wherever the keyboard focus is. Where the estimated width does not fit, buttons drop their
  labels; a double click on a tab folds the ribbon (`ribbonCollapsed` in the settings).
- `shell.rs` has the opaque backdrop the cards float on, the card styles (the floating one
  over the diagram is light grey and catches the mouse with `occlude()`) and icon buttons
  (`in_title_bar` keeps a press on a title bar button from moving the window on Windows). A
  system backdrop (Mica, macOS blur) was tried and dropped: with a non-opaque window GPUI
  renders text without subpixel antialiasing, and on Windows its antialiased edges come out
  too dark over the backdrop. `icons.rs` embeds
  `assets/icons/` (`AppAssets`, falling back to gpui-kit's Lucide icons) and maps tools, diagram
  kinds and model elements to their icons.
- `png.rs` rasterizes the SVG export (`umlcraft_notation::svg`) with `resvg` for PNG export, at
  twice the canvas size and with the system's fonts, on a background thread. `png::copy` puts
  both on the clipboard in one item, as the formats `image/svg+xml` and `PNG` (on Windows):
  Word and PowerPoint take the SVG as a vector image, other programs the PNG.
- `settings.rs` stores recent files, language, theme, the visibility column
  (`visibilityColumn`) and whether abstract classes show «abstract» instead of `{abstract}`
  (`abstractStereotype`) in `UmlCraft/settings.json` in the platform's settings folder (`%APPDATA%` on Windows,
  `~/Library/Application Support` on macOS, `$XDG_CONFIG_HOME` or `~/.config` elsewhere). Both
  reach layout and drawing through `TextMeasure::visibility_column` and
  `TextMeasure::abstract_stereotype` of the measures the canvas and the workspace use (built
  from `canvas::Display`). `settings_dialog.rs` shows language, theme and both in a dialog
  (`gpui-component`'s `Dialog`), which applies each change at once through `MainView`; for the
  column it sets the canvas's flag and gives the workspace a new measure.
- `update.rs` asks GitHub's API for the latest release (`ureq` over rustls with the platform's
  certificates, on a background thread). It is UmlCraft's only network access: the automatic
  check runs only after the user agreed (`updateCheck` in the settings, asked once in a bar
  below the title bar) and at most once a day; About UmlCraft checks on request. A newer
  version found (`latestVersion`) shows as a button in the title bar. `privacy_dialog.rs` shows
  what the check sends from the locales, so reading it stays offline.
- `prompt.rs` replaces the system's message dialogs on every platform (`cx.set_prompt_builder`):
  every `window.prompt` (errors, the question before unsaved changes are discarded) shows the
  app's own dialog, which wraps its text, scrolls a long detail, follows the theme and answers
  Enter with the first button and Escape with Cancel or the last one. GPUI's fallback on Linux
  was 288 px wide and cut off what did not fit on one line.
- Texts come from `locales/{de,en}.yml` (rust-i18n). Without a setting, the system language is used.
- Shortcuts are bound through `key` in `main.rs`, which resolves `secondary-` (Cmd on macOS,
  Ctrl elsewhere) with `KeyBinding::load`; since `gpui-pre` 0.3.10 `KeyBinding::new` no longer
  does and panics on it.

## Notation

`umlcraft-notation` knows UML notation but no UI:

- `scene` turns a view into shapes in logical coordinates. Each node carries its text rows (`Row`)
  with position, font and the element the row edits. Painting, hit testing and inline editing share
  the same layout. Nodes grow with their content; text width is measured by a `TextMeasure` (the
  real font in the app, an estimate in tests). `scene` also places edge labels and provides
  resize handles, edge handles (waypoints), rubber-band selection and alignment. Containers (system
  boundaries, packages, states, components) graphically contain whatever lies fully inside them (`with_contents`);
  system boundaries are "hollow" and clickable only on their border and title.
- `labels` places the edge labels that were not placed by hand (`EdgeShape::free_labels`). Each
  starts where Astah's rule puts it; if that covers a node, a line or another label, it moves to
  the spot nearby that covers least: the other side of its line, further along it, or far enough
  off it not to cross it, with a small cost per step away from Astah's spot. Nodes with others
  inside only count by their border and name. Two greedy rounds, the second among all labels.
  Labels placed by hand, and so all labels from `.asta` files, stay and count as obstacles.
- `route` runs edges in the line styles of `.asta` files (`Style::line_shape`): straight through the
  waypoints, at right angles with the waypoints as corners, or as one cubic Bézier curve whose
  control points are the first and the last waypoint. Right-angled lines are fitted when drawn,
  so their first and last segment stay straight while nodes move or grow. Without waypoints
  they get Astah's default corners: none when the ends are less than 2 apart across (the line
  then runs slightly slanted), else a step halfway between the nodes. The drawn corners are
  what gets dragged (a segment moves across, the end segments slide their anchor inside the
  node) and exported. Changing the style changes the route (`scene::restyle`):
  "line" drops the waypoints, right angles turn them into corners.
- `autoroute` routes edges around the nodes, orthogonally, after Wybrow, Marriott and Stuckey
  (2009): a grid of the padded node sides, their centers and the middles of the gaps between
  them; A* over (grid point, direction) with costs for length, bends and stretches other lines
  use already; Z-shaped routes with a short middle stretch are straightened where the boxes
  overlap; then lines on one stretch are spread apart within their channel, and end stretches
  slide their port along the node side. Generalizations and realizations join the sides the
  boxes turn to each other, so they form trees. Boxes around an end (packages, components,
  nodes) can be crossed, swimlanes are ignored, sequence diagrams are left alone. A routed
  edge is a right-angled line with `Style::auto_route` set; `Workspace::edit` takes
  `autoroute::snapshots` before every edit and routes such edges again afterwards where their
  nodes moved or a node moved onto them (`autoroute::follow`). Editing the route by hand
  (waypoints, line style, line ends) clears the flag. New lines but note anchors are routed
  when drawn. Shared generalizations (below) are left alone.
- `shared` draws Astah's shared style of generalizations: all generalizations to one general
  node with the same `Style::shared` (vertical or horizontal) meet on a common line and end in
  one triangle. Each edge keeps the common line in its waypoints; the scene takes the first
  edge's (`common_lines`) and runs every edge of the group up from its specific, along the
  common line and up to the general's middle. `set_style` switches a whole group,
  `move_common_line` moves it when one of its lines is dragged.
- `layout` arranges class, component and deployment diagrams in layers, after Sugiyama:
  generals above their specifics, suppliers above clients and wholes above their parts (other
  edges give way where they would close a cycle); then barycenter sweeps for few crossings, and
  places along each layer by isotonic regression toward the middle of the neighbors, ending
  bottom up so that generals are centered over theirs. Parts nothing connects are packed into
  rows, larger ones first. Containers are laid out inside out and sized around what they
  hold. Generals with two or more specifics beyond them in the layout direction get the
  shared style. Arranging a selection keeps its top left corner; nodes it grows into move right or
  down. All its lines are routed afterwards. The other kinds but sequence diagrams use the
  same layers: flows and transitions point from layer to layer (cycles give way at the edges
  met last going out from the initial node), include and extend from the base use case to
  the other. State machines and use case diagrams run left to right unless told otherwise
  (`default_direction`). An orthogonal state lays out each region on its own and stacks them
  as its `Regions` say, with new separators. Use case diagrams put actors beside the system
  boundary (secondary actors, whose end alone is navigable, after it) and level with their
  use cases. A whole activity is laid out in rows from the layers: partitions stay columns
  side by side, every node in the one it was in, and fork and join bars span what they join,
  with straight lines to the nodes above and below.
- `member` produces and parses the text form of attributes and operations and writes it back to
  the model; known classifiers become real type references.
- `object` derives an object's slots from its classifier: one per attribute, inherited ones
  included (the generals' first, depth first, then its own). Only slots with a
  value are stored, as `Slot` children of the object that refer to their attribute; deleting the
  attribute deletes them. So slots follow the class when attributes are added, renamed or
  removed. An object's box shows `name : Class` underlined, and below it
  always a slot compartment, 8 high when empty. Links are `Link` elements with two ends
  (instance, role, navigability, aggregation) and optionally the association they instantiate;
  they are drawn like associations, with plain role names.
- `clipboard` copies the selection including subtrees and the relationships among them, and pastes
  it as a duplicate (new IDs, free names) or as another presentation of the same elements.
- `palette` defines which tools a diagram type offers, which connections are allowed (e.g.
  realization only from class to interface) and where new elements land in the ownership tree.
- `sequence` draws sequence diagrams, `sequence_edit` edits them (see below).
- `draw` turns a scene's figures and end markers into a few primitives (polyline, polygon,
  ellipse, frame, curve) with color roles (`Paint`) and says where text baselines go. The canvas paints
  them with its theme colors (and a node's own fill color on a light background); `svg` writes
  them as the canvas shows them in the light theme: black lines, white where a node has no color
  of its own, a margin of 10 and 4/3 pixels per unit. The app's image export writes coordinates
  and sizes ten times larger (`render_scaled`, same image size): Word's PDF export puts the
  letters of SVG text on whole units, which at 12 units per letter height spaces them unevenly. So the figure geometry exists once,
  for both. Texts in nodes are centered by Java's whole-pixel width or the exact one, whichever
  is larger (`TextMeasure::astah_width`, with the tables for 12 and 14 px in `metrics`), so imported diagrams keep their text
  positions, and written at a fixed x. Message labels are centered by that width too; those
  moved by hand get the offset Astah's stored box implies on import.

## Sequence diagrams

Sequence diagrams store their whole geometry, as `.asta` files do, so that they import from and
export to `.asta` and back without change:

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
- State invariants and terminations (`ElementKind::StateInvariant`, `Termination`) name their
  lifeline (strong `RefRole::OnLifeline`) and are items of the interaction or an operand like
  messages; duration and time constraints (`DurationConstraint`, `TimeConstraint`) name their
  messages (strong `RefRole::Constrained`) and belong to the interaction. All four are node
  presentations: an invariant centered on its lifeline (its name is the condition), a
  termination's 20 × 20 cross where its lifeline ends, a duration constraint's line at its x
  from one message's height to the other's, a time constraint on from its message's arrowhead
  (placed by `sequence`, which reads its message's line). A termination is not the `destroyed`
  flag, which stays for destroy messages. `sequence_edit::create_on_lifeline` and `constrain`
  create them, `add_mark` and `add_constraint` show existing ones.
- Message numbers ("2.1") are not stored: a message sent from a bar is numbered below the call
  that started the bar; messages leaving the same point branch ("-A", "-B"). A reply's return
  value is shown after the call it answers. Both can be switched off per diagram
  (`DiagramProps::hide_message_numbers`, `hide_return_values`).

The interaction's order follows the drawing: `sequence_edit::tidy`, run after every edit of a
sequence diagram, grows bars to end 20 below what lies in them and lifelines to reach 30 below
what is on them, centers state invariants on their lifelines, puts each termination's cross where
its lifeline ends (at its own height, or 30 below what is on the lifeline when that is lower),
lets duration and time constraints follow their messages, grows the frame around everything, and
puts every message, fragment, state invariant and termination into the operand of the innermost
fragment drawn around it, ordered by height (`sync_order`). Messages stay 20 above a
termination's cross, as above a destroy message's. Edits follow
the rules measured through the `.asta` export API: a call gets a bar of 25 on the receiver (a create message none) and, where the
sender has none, a bar of 45 on the sender (60 for create, 30 for lost messages); a reply leaves
at the end of the bar it returns from; a message to self comes back 20 lower into a bar of its
own; a create message brings the new lifeline's head down to it; a destroy message ends the
lifeline in a cross below its bar. UmlCraft's own rules on top: a message sent at most 45 below
a sender's bar continues that bar instead of starting one, and a further call to the same
receiver goes 20 below the bar the last one started there (`clear_of_calls`). A message drawn
below everything from a lifeline whose call is not answered yet continues that call's bar
(`continue_open_bar`), and a new operand moves what lies below its fragment down
(`make_room`). Dragging a
message moves it with the bar it starts and everything sent from that bar (`move_item`); the
sender's bar it leaves shrinks to what is still in it (`shrink_bar`), the only time a bar gets
shorter. A message's label can be dragged on its own
(`place_label`). Combined fragments grow to hold the bars and messages that start in them, and
bars that no message reaches any more are removed. The layout's `Slot`s carry the height the
user pointed at.

## State machines

A state owns its substates directly, or, as an orthogonal state, `Region` elements that own
them; internal transitions (`InternalTransition`, `trigger [guard] / effect`) are children of
the state and listed in it below entry, do and exit. Which region a substate is drawn in is up
to the presentation: a state's node keeps the layout of its regions (`Regions`: one below the
other or side by side, and the lines' distances from the top or left edge, as `.asta` files
keep them).
Lines it lacks divide the room after the last one evenly, and every region keeps at least 20.

`state_edit` keeps the model in line with the drawing: a vertex moved on the canvas
(`reparent_vertices`) or created there belongs to the innermost state drawn around its center,
or to the region of it that holds the center, else to the state machine; a state never goes
into itself or what it holds. Dividing a state the first time makes two regions and puts its
substates into the one they are drawn in; further regions halve the last one. Moving a line
between regions moves the substates on the other side into their new region; deleting a region
deletes what it holds and gives its room to the neighbor before it.

A submachine state is a `State` whose `StateProps::submachine` names another state machine (a
weak `RefRole::Submachine`); `scene` draws it with `include / Machine` between two lines and
the submachine icon (`Figure::State { submachine }`). A new one stands for the first other
machine of the project, else for a new machine with its own diagram
(`state_edit::submachine_for`). Entry and exit points and stub states are `Pseudostate`s
(`PseudostateKind::EntryPoint`, `ExitPoint`, `StubState`). `state_edit::create_node` puts a
point on the border of the state at or within 20 of the click, owned by that state, else on the
diagram's frame, owned by the machine (adding the frame when missing); a stub state goes into
the submachine state around the click and is refused elsewhere (`STUB_OUTSIDE`). Points are
borne nodes like pins (`composite::Borne::Point`, host from `composite::owner_host`), so
`composite::follow` keeps them on their border; `reparent_vertices` leaves them and stubs where
they belong and puts no ordinary vertex into a submachine state. `scene::state_borders` maps
points onto the drawn border and puts the names of top-side points above them.

## Composite structures

A composite structure diagram shows the inside of a class or component. Its parts are the
class's `Attribute` elements, drawn as nodes inside the class (`Figure::Part`, `name : Type
[m]`); the class itself is drawn as a structured class (`Figure::Structured`), not sized to
its content and drawn behind the other nodes. `Port` elements belong to a class or component;
a port on a part is a port of the part's type. A `Connector` owns two `ConnectorEnd`s: the part
or port it ends at (a strong reference) and, for a port on a part, the part it is reached
through (`part_with_port`, weak).

As with system boundaries, what goes where follows from the drawing (`composite`): a new part
belongs to the innermost class drawn around it, which grows to hold it; a new port goes on the
border nearest to where it is dropped (within 10 of a border that one, else the class or part
around the point) and to the class or the part's type. A connector's ends and owner follow
from the nodes it joins. `edit::apply` takes a `composite::Snapshot` before every edit and
`composite::follow` corrects the drawing afterwards: a class grows around a part that moved or
changed size beyond it, a port that did not move keeps its side and its distance along it when
its host moves or changes size, a port that moved snaps onto the nearest border, and a port
goes along when its host is removed from the diagram. Lines on these diagrams start straight
and Arrange is not offered. An iconic interface with `Style::socket` is drawn as a socket
(`Figure::Socket`), its opening turned away from its first line; the properties panel sets it
for an interface shown as a circle, the `.asta` import for interfaces a port requires. The `.asta` import reads parts, ports, connectors, ball and socket;
the export recreates them through `CompositeStructureDiagramEditor` and writes a component's
parts as association ends, as Astah takes no attributes on components.

## Pins and activity parameters

Pins and activity parameter nodes are `ActivityNode` elements that live on a border, like ports.
An `InputPin` or `OutputPin` is owned by its action (`can_own`), an `ActivityParameter` by the
activity; a call behavior action names the activity it calls (`ActivityNodeProps::behavior`, a
weak `RefRole::Calls`). Object nodes, pins and parameters carry ordering, upper bound and stream.
`activity::create_node` puts a new pin on the border of the action at or within 20 of the point
(refused elsewhere, `PIN_OUTSIDE`) and a parameter on the diagram's frame, adding the frame when
missing. `composite::follow` keeps them there: its snapshot covers ports, pins (host: the
owning action's presentation) and parameters (host: the frame), so a pin keeps its side and
place when its action moves or changes size, goes away with its action, and a moved pin or
parameter snaps back onto its border. `layout::arrange` leaves borne nodes alone and treats a
line at a pin as a line at its action. Since an action or the frame can be drawn larger than
stored, `scene::activity_borders` maps pins and parameters onto the drawn border at layout time,
puts the labels of top-side pins above them and gives pins without flows their arrow.

## .asta import

`umlcraft-asta` reads `.asta` files without any Java: `jser` is a generic reader for the Java
object serialization stream and turns it into a graph of objects addressed by class and field
name; `map` translates that graph into a new `Model`, element by element through `Model::apply`,
so every invariant is checked. What has no counterpart in UmlCraft is skipped and counted
(`Imported::skipped`). Template parameters, bound templates (as type expressions), array
modifiers, qualifiers and the tagged values of Astah's language profiles are imported too.
Objects bring their slot values (`UAttributeLinkImp`, whose value is an instance named by the
value text); links (`ULinkImp`) have no owner in Astah and go into the package of their first
object, their ends' navigability and aggregation come from the tagged values
`object_navigable_type` and `object_aggregation_kind`.
Astah's ER model (`map/er.rs`) becomes a package with its schemas and ER packages; its
datatype and domain models are left out (a column typed by a domain gets the domain's
datatype). Entities and columns bring their logical and physical names, keys, `NOT NULL`,
defaults, `UNIQUE` and lengths from the entity's constraints and tags; relationships their
participations from the ends' multiplicities, verb phrases as roles, `identifying` (the
child weak); subtype relationships one IS-A per supertype and group (`conclusive` → total,
discriminator). Foreign key columns are resolved once all relationships exist. ER diagrams
keep notation, display level and model type; entity bounds lose the name band again, and a
subtype group becomes the IS-A node at its symbol with right-angled lines to the subtypes.
The format details are kept in local notes (`docs/asta-format.md`) that are not published.
Two examples help when exploring files: `asta-dump` (raw objects) and `asta-import` (the
resulting model).

## Source import

`umlcraft-reverse` reads declarations from source code into the open project, so that the
result matches what a Java import into an `.asta` project gives (see the comparison test below):
types, members, template parameters, supertypes and
documentation, but no method bodies and no diagrams. It reads Java, C#, C++ and Python: a
front-end per language lowers its syntax to a language-neutral IR (`ir`), and one
mapping for all languages puts the IR into the model.

- **Reading and parsing** is the base the structograms from source code share (see "From
  source code" below), so that a file is read and parsed once, the same way, for both.
  `SourceLanguage` names the languages and their file extensions (`SourceLanguage::of`; C and
  headers count as C++). `source` collects files (recursively, sorted) and reads them
  (`source::read`): none larger than `MAX_FILE_SIZE`, and decoded by a byte order mark, else as
  UTF-8, else as Windows-1252, which the report mentions (`Decoded::guessed`). `parse_tree`
  runs tree-sitter and returns a `SourceTree`: the tree, the text as parsed and the text as
  written. C# and C++ are parsed as the first branch of each `#if`/`#ifdef`/`#ifndef` …
  `#else` reads them (`preprocessor`): the other branches are blanked out before parsing,
  keeping every byte's place, so conditionals that cut a declaration apart still parse, and
  the text as written still finds the left-out lines. Macros are not expanded, so a C++ file
  that uses one in a declaration (`class API Name`, `int x GUARDED_BY(mu);`) fails to parse and
  is reported. `comment` holds each language's documentation rules (`comment::documentation`:
  the Java block comment before a declaration, runs of `///` and `//!` in C# and C++, Python
  docstrings cleaned like `inspect.cleandoc`), which give an operation and a structogram of the
  same method the same text.
- **Front-ends** (`lang::Frontend`) lower a `SourceTree` without syntax errors (`lower`; a file
  with one is reported with line and column and left out, the others are imported). They
  resolve nothing; they lower language sugar instead (Java's implicit `public abstract` in
  interfaces, records; C#'s properties, which become attributes or accessor operations as .NET
  names them, `get_Count`). C++ adds members defined outside their class
  (`ir::Item::Definition`, `void A::f() {}`) and typedefs (`ir::Item::Alias`,
  `ir::Member::Alias`), which the mapping resolves. A Python file is a module: `lower` gets its
  module path (`source::python_module`, from the folders with an `__init__.py`), and what has
  no annotation gets the type `ir::TypeName::Unwritten`, which the mapping leaves without type
  and association. What they leave out (method bodies, local and anonymous classes, C#
  attributes, events, C++ macros, specializations, friends, the left-out branches of `#if` …)
  is counted (`lang::Skip`).
- **Rules** (`lang::Rules`) are what the mapping needs to know about a language: built-in
  types, implicit imports, value types, how to split a qualified name, and the standard
  library (`lang::Library`, read from a table by `library::TableLibrary`). For Java that is a
  table of the JDK's public types (`java/jdk.txt`, written by `tools/jdk-types/JdkTypes.java`)
  with their kind, type parameters and whether they are collections or maps; for C# a table
  kept by hand of the base class library's containers and value types (`csharp/bcl.txt`); for
  C++ one of the standard containers and smart pointers (`cpp/std.txt`, `unique_ptr` owns
  exactly one); for Python one of `builtins`, `typing` and `collections` (`python/stdlib.txt`).
  Where languages differ in how names resolve, a rule says so instead of the mapping asking
  which language it is: C# looks in the namespaces around a type's own
  (`enclosing_namespaces`), its base list does not say which entries are interfaces
  (`base_list_by_target`: a known target's kind decides, the front-end's guess by C#'s rules
  only gives unknown ones their kind), and
  `Order?` (C#) and `Order*` (C++) are optional (`optional_modifiers`); qualifiers such as
  C++'s `const ` that associations look through are `type_qualifiers`.
- **Resolution** (`resolve`) links names in Astah's order (template parameters, member types,
  imports, own package, on-demand imports, `java.lang`). Names that stand for nothing in the
  model become placeholder types: library types at their package path (`java › util › List`,
  `System › Collections › Generic › List`), qualified names under their path, unknown simple
  names in the root (Astah's default package). An alias (`using M = A.B.Map;`) is a single-type
  import by another name; a qualified name that starts with what a single import makes visible
  continues the import's path (`Gen.List` after `using Gen = System.Collections.Generic;`).
  With `enclosing_namespaces`, import paths are first relative to the namespaces around
  (C++'s `using namespace lib;` inside `acme`). On-demand imports find library types only in
  the library's turn, so whether a placeholder exists yet does not decide between two
  packages. With `Rules::reexports` (Python), a package re-exports what its own scope imports:
  `shop.Order` is `shop.orders.Order` after `from .orders import Order` in `shop/__init__.py`.
  A re-export of a name under its module's own name (`from .repair import repair`) does not
  apply to the other names of that module (`shop.repair.Setting`), and one being followed is
  not followed again, against cycles. An import of a module alone (`import numpy as np`) makes
  no placeholder. The qualified path of a single import is resolved without the imports
  (`resolve_import`): after `from socket import AddressFamily, socket`, `socket.AddressFamily`
  stays itself instead of becoming `socket.socket.AddressFamily`. A qualified name ends at a
  type, never at a package, so a module written as a type (`x: os.path`) links a placeholder
  class. With these rules the CPython 3.14 standard library imports as a whole. A typedef is
  replaced by its target where it is used (`Ptr*` is
  `shared_ptr<Order>*`); the model has no element for it.
- **Merging** (`merge`) finds packages and types by name or creates them. Members are matched
  with what the model has (attributes and literals by name, operations by name and parameter
  types, template parameters and parameters by position): matched ones are updated, new ones
  created, the rest deleted, and the members put in source order within literals, attributes
  and operations, which come in that order. A type whose kind changed is
  replaced by a new element that takes over its children, references and presentations
  (`merge::retype`). A name declared with two kinds in one import (`class Foo` in one file,
  `interface Foo` in another) keeps the kind of its first declaration. Generalizations and
  realizations follow the sources, so one drawn by hand between types of the sources is
  deleted; dependencies drawn by hand stay. A type declared in parts
  (C# `partial`) is one element with the members of all parts, documented by the first part
  that has documentation. A member defined outside its class joins the class's members: it fills
  in parameter names, documentation and a field's default the declaration leaves out, or adds
  an operation the class does not declare. Operations match by being a query too (C++'s
  `f() const`), so `f()` and `f() const` are two operations.
- **Associations** (`associations`): attributes typed by a type of the model become
  associations, collections and maps included (`*`, `{ordered}`, `{unique}`, a qualifier for
  the key, `0..1` for optionals). The import marks the associations it makes with the tagged
  value `umlcraft.imported`. On a new import an association with the same role is reused, and
  marked associations whose field is gone are removed; those drawn by hand stay.
- **Where things went** (`Placed`, from `apply_placed`): per file, the operations by the bytes
  their declarations stand on (`ir::Operation::span`; a C++ definition outside its class
  counts for the operation it joins), the types by their names in the file, and the package of
  what the file declares outside types. A method's structogram finds its operation by the
  overlap of those bytes with its own (`MethodRef::start..end`), so neither side has to name
  the method as the other does (`tests/placed.rs` in `umlcraft-nsd-code` checks every method
  of its fixtures).

Parsing needs only the files, so the app runs it in the background (`parse_sources`); `apply`
then runs on the UI thread inside one `Document::edit`, so one undo takes the import back.
In the app one import serves classes and structograms (`umlcraft-app/src/nsd/source.rs`):
`source::parse` reads and parses each file once and keeps its units and its methods, a file
with a syntax error giving methods all the same; a dialog lists the methods (none ticked,
those with a structogram from an earlier import ticked, those edited by hand ticked and
locked); the structograms of the ticked ones are built in the background from the files read
again, and one edit applies the classes (`apply_placed`) and files the structograms
(`file_structograms`): under the class of their operation and linked to it, else under their
class or their file's package, else in the project. They carry `umlcraft.imported` until edited
by hand (`Tx::set_structogram` removes it) and `umlcraft.method` (file name, classes,
signature) always; a new import renews the untouched ones ticked, deletes the untouched ones
not ticked or whose method is gone, and leaves edited ones alone. For
large imports `Tx::delete_many` deletes in one pass what `Tx::delete` would look for one by
one, and undo replays removals without looking through the model for references, which the
recorded operations rule out; 6300 files (225,000 elements) import in about ten seconds.
Qualified names and used single-type imports are resolved before any simple name, so the
result does not depend on the order of the files and importing again changes nothing.
`cargo run --release -p umlcraft-reverse --example source-import -- <java|csharp|cpp|python> <folder>`
imports a real project, checks the model, undoes, redoes and imports again, and reports any
difference; Newtonsoft.Json (951 C# files, 22,600 elements) imports in about 1.2 seconds,
leveldb (132 C++ files, 99 of them without macros in declarations, 1,357 elements) in 0.2
seconds, requests (37 Python files, 1,396 elements) in 40 milliseconds. The
tests import fixtures per rule (`crates/umlcraft-reverse/tests/fixtures/<language>`) and
compare the IR and the resulting model with snapshots (`expected.ir`, `expected.model`;
`UPDATE_SNAPSHOTS=1` rewrites them); `tests/common` holds what the languages' tests share.
`tests/astah_compare.rs` imports the Java fixtures and compares them with what Astah's own
import made of them (reference `.asta` files kept outside the repository). Where UmlCraft
deliberately differs, because Astah loses what the sources say (wildcards, varargs,
operation templates, more than one bound …), `tests/astah_deviations/<fixture>.txt` lists and
explains the differing lines; the test fails when they change.

## .asta export

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
- Free texts and lines go through `createText` and `createLine` of the diagram's editor. Astah
  adds the line but reports an exception; the export then takes the line the diagram did not
  have before and sets `line.type` and `line.arrow.type` on it.
- Sequence diagrams: Astah puts lifeline heads at y = 30, so the whole drawing moves vertically
  by as much; x stays. Messages are created from top to bottom at their stored height, from the
  Astah bar made for the UmlCraft bar at that height (or the lifeline). Astah makes the bars
  itself by the same rules UmlCraft follows, and refuses to resize them (`setHeight`, reported as
  `barHeight` where Astah's height differs); a reply (`createReturnMessage`) goes to the end of the bar of the call it
  answers, where UmlCraft has it too. Lifeline lengths are set through `lifeline_length`; Astah
  refuses lengths shorter than what lies on a lifeline, interaction uses included. A reply's
  return value is stored on the call it answers, as Astah does. The round-trip test checks that
  heads and message heights come back unchanged. State invariants (`createStateInvariant`, top
  at the stored height) and then terminations (`createTermination`, at the lifeline's end) come
  after the lengths, which are set again after each, as Astah refits the lifelines; duration and
  time constraints are reported as dropped, the API cannot create them.
- Activity diagrams: `createActivityDiagram` brings its own activity, so an activity exports its
  first diagram only. Astah creates activity nodes only on a diagram; nodes, flows and
  partitions not shown are dropped. Partitions are created left to right with
  `createPartition(superPartition, previous, …)`; Astah wraps top-level lanes in a "dimension"
  header of its own and lines them up below it, so the header is moved above UmlCraft's lanes
  before the lanes are placed. The API creates decision and merge nodes as one kind; the import
  tells them apart by their flows. Pins are created after all other nodes, on their action's
  presentation; the activities called by call behavior actions are set once all diagrams
  exist, so an activity without a diagram (none in Astah) is reported as dropped. Unnamed
  control nodes get Astah-like default names
  (`InitialNode0`), which the import drops again, together with Astah's own (`ForkNode1`) and the
  default dimension (`Dimension0`).
- Component and deployment diagrams: the API creates components, artifacts and nodes
  (`createComponent`, `createArtifact`, `createNode`, in packages only) but has no editors for
  these diagrams. They are exported as class diagrams, which show the same elements; instance
  specifications exist only there (`createInstanceSpecification`), always as objects, placed
  when created (they cannot be moved) and sized by Astah. UmlCraft's class diagrams accept
  components, nodes and instances, so such a file comes back intact apart from the diagram
  kind. A usage (`createUsage`) is an `IUsage`, not an `IDependency`.
- Object diagrams are class diagrams. Slot values are set with `getSlot(name).setValue`; links
  are created between two presentations (`createInstanceSpecificationLink`), and their ends are
  matched to UmlCraft's by instance. The API cannot set the association a link instantiates,
  which is reported as `linkAssociation`.
- State machine diagrams: `createStatemachineDiagram` brings its own state machine. States are
  created from the outside in, each composite state at its size before its substates go into it
  (`createState(name, parent, …)`). The API gives a transition leaving a state an event called
  "Trigger"; `setEvent(null)` removes it again. Astah keeps room for entry, do and exit, which
  makes a state at least 81 high, so states without them have that compartment hidden
  (`action_visibility`). Astah routes the first and last segment of a self-transition itself.
  `addRegion(state, horizontal)` divides a state once into two regions (calling it again does
  nothing), so further regions are reported as `region` and their contents go into the second.
  Astah picks a new substate's region by where it is created, so substates start inside their
  region and are moved to their place afterwards. Ending the transaction, Astah moves the line
  between the regions next to what the first one holds and shifts the second one's contents
  along; a second transaction puts the substates of orthogonal states back. Internal
  transitions are created with `addInternalTransition(trigger, guard, effect)`.
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
- Message display options: `parameter_visibility`, `parameter_type_visibility` and
  `return_value_visibility` (a call without arguments shows its operation's parameters,
  `Style::hide_parameter_types`, `Style::show_return_type`), and labels moved by hand through
  `name.point.x/y`. Astah moves a label by as much as `name.point` differs from its own value,
  so the layout sidecar says where Astah puts each such label by itself (`labelDefaults`: the
  last segment's middle minus half of Java's whole-pixel width, at most 200). The diagram's
  `message_index_visibility` and `message_return_value_initial_visibility` carry
  `DiagramProps::hide_message_numbers` and `hide_return_values`. The API makes bars white;
  they get their UmlCraft fill (`fill.color`, Astah's yellow by default).
- Not available in the API and reported as dropped: the query flag of operations, conditions and
  extension points of extends, return parameters, parameter multiplicity and default values.
  Structograms, which Astah does not have, are taken out of the model before writing
  (`without_structograms`) and counted as dropped.
- ER diagrams: packages holding only ER content go into Astah's ER model
  (`ERModelEditor.createERModel`, numbered when a UML package already has its name), entities
  into its default schema. Columns are created with their key, `NOT NULL`, default, length and
  datatype (Astah's, found by name, or a new one); foreign key columns are not created, Astah
  makes them when the relationship is (`createIdentifyingRelationship`,
  `createNonIdentifyingRelationship`, `createMultiToMultiRelationship`, with parent required,
  cardinality and verb phrases), and the export gives them UmlCraft's names. Subtypes become
  subtype relationships (`conclusive`, discriminator), several of one supertype a group
  (`createSubtypeRelationshipGroup`). Diagrams get notation, display level and model type;
  nodes are placed one name band higher and that much taller, as Astah's bounds include the
  name. Chen diagrams are exported in IE (`erChenNotation`); relationships of more than two
  entities or with attributes, `UNIQUE` (no API call) and disjointness are reported as
  dropped.
- System boundaries, which Astah does not have, become what Astah users draw instead: a
  rectangle (`createRect`) and the name as a text (`createText`) centered at its top. The
  importer turns a rectangle on a use case diagram back into a boundary, named by the text
  that starts within 40 of its top edge. Boundaries no diagram shows are dropped.
- Line styles: `line.shape` is set before the points (Astah would otherwise drop or replace
  them); right-angled lines send their corners as drawn (the sidecar's `corners`).
- Edges: `setAllPoints` gets the source anchor, the waypoints and the target anchor as points;
  Astah derives its relative ends (`sourceEndX/Y`, `targetEndX/Y`) from the first and last
  point. Astah stores some links the other way round (a dependency starts at the supplier, a
  generalization at the general class) and rejects points in the wrong order; the current first
  point (`getAllPoints()[0]`) tells which node comes first, `getSource()` does not always.
  Labels placed by hand are set as properties with their position on the diagram (`name.point.x`,
  `end_a.name.point.x`, `end_b.multiplicity.point.y` …); the sidecar turns UmlCraft's offsets into
  those positions, and adds the labels the drawing moved out of the way (`FreeLabel::moved`). `setProperty` silently ignores keys it does not know. Constraints of
  association ends become constraints on the end.

## Nassi-Shneiderman diagrams

`umlcraft-nsd` holds structograms in the `.nsd` format of
[Structorizer](https://structorizer.fisch.lu) 3.32-35. In the app they are part of the
project, stored in `.ucraft` next to the UML diagrams; `.nsd` files are imported and exported.
Structorizer is GPL-licensed; the format and the way
Structorizer writes it were worked out from its XML schema, its user guide and the files it
writes, and none of its code is used.

- **Model.** A `Diagram` has the attributes of `<root>` (header: version, parser keywords,
  author, dates, licence; the root's own text, comment, colour, `type` program/sub/includable,
  `style` nice/abbr) and a `Sequence` of `Element`s. An element has a kind (instruction, call,
  jump, alternative, case, for, while, repeat, forever, try, parallel), its attributes and its
  branches (`qTrue`/`qFalse`, one `qCase` per case …). Every node keeps its attributes as an
  ordered list exactly as read and offers typed access on top (`text`, `comment`, `color`,
  `disabled`, `branch_colors` …). Texts are lists of lines, stored as `"line1","line2"` with
  doubled quotes (`text`). Setting an attribute a node lacks puts it where Structorizer writes
  it (`ElementKind::order`, `ROOT_ORDER`).
- **Reading** (`read`) uses quick-xml. Attribute values are unescaped but not normalized: a
  tab Structorizer wrote stays a tab, where an XML parser (Structorizer's too) would read a
  space. Unknown attributes are kept; unknown elements, wrong branches and other encodings
  than UTF-8 are errors with line and column.
- **Writing** (`write`) is done by hand, because Structorizer escapes differently from XML
  libraries (`&#34;`, `&#60;`, `&#62;`, `&amp;`, `&#10;`): the declaration, a tab per level,
  elements without branches on one line, branch tags on lines of their own, LF line ends and
  none after `</root>`. Empty sequences carry `color="ffffff"`, as in Structorizer's files.
- **New diagrams and elements** (`create`) get the attributes Structorizer gives them: the
  header of a fresh installation's GUI (its keywords `"until "` and `"while "` end in a space),
  `branch_colors` with one slot per branch, a parallel element's branch count as its text,
  the attributes of counting, traversing and free-text for loops.
- **Editing.** `path` addresses elements (odd paths) and sequences (even paths; `[]` is the
  body), places between elements (`Slot`), runs of elements in one sequence (`Span`) and what
  is selected (`Selection`). `edit` holds the changes as functions on a `Diagram` that return
  the selection afterwards: insert, remove, move (never into itself), shift up or down, text,
  comment, colour, disabling, case and parallel branches, the root's signature and kind. Each
  keeps the shape Structorizer writes: empty sequences carry `color="ffffff"`, a case has a
  line per branch plus its selector, a parallel element's text is its branch count, an empty
  element text is written as `text=""`, and a for loop's attributes are derived from its text
  as Structorizer derives them when it loads a file (checked against a file it saved,
  `fixtures/structorizer/for-headers.nsd`). `history::Editor` makes each change one undo step
  over snapshots of the whole diagram (structograms are small) and tells whether the diagram
  differs from the file. `save` stamps a changed diagram with version, user and date as
  Structorizer does on saving; an unchanged one is written as read, byte for byte. `clip` puts
  elements on the clipboard as a whole `.nsd` file, the form in which Structorizer puts a
  diagram there; other text pastes as one instruction.
- **Content form** (`content`). A project keeps only what a structogram says, not the file it
  came from: `normalize` drops the header (version, author, dates, licence, `origin`), unknown
  attributes and the parser keywords that equal Structorizer's defaults, and keeps the root's
  text, comment, colour, `type`, `style`, include list and namespace and each element's
  attributes. Keywords someone changed stay (`custom_keywords`), since Structorizer's parser
  reads the diagram with them. `to_file` makes a file of it again for the export: a fresh
  header (`Diagram::new`, user, date, `origin`) plus the kept keywords. With the `serde`
  feature the content form reads and writes as structured JSON (`records`); reading checks
  kinds, branch counts, colours and keyword names, and derives a for loop's attributes from its
  text instead of storing them.
- **Text metrics** (`metrics`). Structorizer measures with Java's logical font Dialog 12,
  antialiased with integer metrics: a line is as wide as its characters' advances summed and
  rounded, the bold title's widths are cut off instead. The font behind Dialog differs by
  platform; `tools/nsd-metrics/DialogMetrics.java` writes a table per platform
  (`src/metrics/dialog12-<platform>.txt`). Linux (DejaVu Sans) and Windows (Arial, whole-pixel
  advances, title lines 16 px apart instead of 17) are measured; Linux uses its own, every
  other platform the Windows table, Structorizer's most common one. The include list's box
  above the title uses a smaller font whose widths are not tabled: its height and caption
  width were measured in Structorizer's images and stand in each table (`includes`).
- **Layout** (`layout`) works in whole pixels, with the rules read off Structorizer's images
  (GUI defaults, syntax highlighting off): `measure` gives each element its natural size from
  the inside out, `place` its box. A box is given by its lines; neighbours share theirs.
  Texts get a padding of 20 and 13 px per line, the frame 20 around the body and 17 per title
  line. An alternative's head is `13 n + 39` high and wide enough that every line of the
  condition, with its padding, fits between the diagonals at its baseline; spare width goes to
  the TRUE branch. Case columns are as wide as their contents or labels plus 10, spare width
  to the last; with more than 8 branches a branch of one simple element is turned by 90°
  where that is narrower. Loops have a bar of 19 and a head or footer; try elements rows of
  33 with bodies indented 20 (catch 40); parallel elements bands of 20. A sequence's last
  element takes spare height, a structured one hands it to its branches. Every element box
  has its path (element and branch indices); `Layout::hit` tells what a click selects and
  `Layout::slot_at` where a drop goes.
- Where the texts in triangular heads sit (an alternative's condition, a case's
  discriminator) follows no rule we found; a fit to Structorizer's positions puts them within
  a few pixels. Lines are exact.
- **Drawing** (`draw`) turns a layout into areas, 1 px lines and texts, fills first. Colours
  are roles (`Ink`) so that a painter can use its theme, except the elements' and branch heads'
  own colours. The hatching of disabled elements repeats Structorizer's 5×10 pixel tile from
  the image's origin. `svg::render` writes it as an SVG image as large as Structorizer's PNG
  export; the app rasterizes it with `png::from_svg` like UML diagrams.
- **In the app** (`umlcraft-app/src/nsd`), a structogram opens like a diagram, in the main
  window's diagram card. `MainView` keeps an `NsdView` per structogram shown and puts it where
  the canvas would be; the zoom pill, the zoom commands and the image export go to it.
  `NsdView` reads the structogram from the model and changes it only through
  `Workspace::edit_structogram` (one `Document::edit` with the whole changed diagram), so
  structograms share the project's undo history; after an undo the selection is kept only if
  it still fits. The ribbon's diagram tab becomes a structogram tab (insert by kind, colour,
  disabling, branches, `.nsd` export), enabled by what the selection allows (`Allowed`). It
  shows the SVG export rasterized with resvg, made again when the diagram, the zoom or the
  screen's scale changes, so the card holds exactly what an export does; GPUI cannot turn
  text, which turned case branches need. Selection and the place a drag would drop to are
  painted over the image and never reach an export. Text, comment and the root's signature
  are edited in a dialog (`nsd/dialog.rs`), as in Structorizer; a new element is only inserted
  when its dialog is confirmed, and an open dialog is taken over before saving or closing the
  project. The diagram stays a white sheet in the dark theme; the area around it follows the
  theme. Opening a `.nsd` (menu, command line, system, recent files) starts a new unsaved
  project holding just that structogram, offered for saving as `<name>.ucraft` next to it;
  *Import › Structogram* adds `.nsd` files to the open project, *Export › Structogram* writes
  the one shown.
- **From source code** (`umlcraft-nsd-code`; `java`, `csharp`, `python`, `cpp`): a Java, C#,
  Python or C++ file is read and parsed by the base of the class import (see "Source import":
  `SourceLanguage`, `source::read`, `parse_tree`; C and headers with the C++ grammar, C# and C++
  with only the first branch of each `#if`). A `Source` holds that `SourceTree`
  (`Source::new`, `Source::parse` as a shortcut, `Source::tree` for `umlcraft_reverse::lower`),
  so one parse can give a file's classes and its methods' structograms; a method's
  documentation is the diagram's comment, found and cleaned by `comment::documentation` (or the
  docstring) as for its operation. `Source::methods` lists
  methods and constructors with a body (nested types included, not classes inside methods) with
  their signatures as written, in C# also finalizers, operators (`op_Addition`), the accessors
  of properties, indexers and events as .NET names them (`get_Count`, `set_Item`) and the
  top-level statements as `Program.Main`; in Python the functions of the module (without a
  class), methods without `self`/`cls` in the title, the docstring as the diagram's comment, and
  the code of the module as `__main__`; in C++ free functions, methods defined in the class or
  outside it (grouped as written, `shop::Cart`, without template arguments), constructors with
  their initializer list as instructions, destructors, operators and templates;
  `Source::diagram` builds one with the elements of `umlcraft-nsd` (`edit::new_element`, so
  every rule of the editor holds). Each language reads its own nodes and hands the parts to a
  shared builder (`core`: comments, conditions and loops, the cases of a switch, the parts of a
  try), which a `Grammar` tells the node kinds that differ. Both languages give the same
  structogram for the same method (`tests/parity.rs`, differences on purpose in
  `tests/parity_deviations.txt`). Texts are written as structograms write them (`pseudo`): no
  types (the root is `name(a, b)`, a declaration without a value has no element), assignments as
  `x := value` (`total += a` as `total := total + a`, `i++` as `i := i + 1`), no `;`;
  expressions and conditions stay as in the code, conditions without their outer parentheses and
  ending in `?` (`a > 0?`, also a case's discriminator and a loop's condition); a returned value
  reads `gib x` (`return;` stays `return`). `f(x)` and `this.f(x)` are calls, other statements
  instructions (in Python and C++ only calls of functions the file declares or defines, and
  `self.f(x)`/`this->f(x)`, so that `print(x)` and `printf(x)` stay instructions); an expression
  body (`=> a + b`) is a `return` or, where nothing is returned, a statement. A for loop that
  counts one variable by a constant step is a counting loop (`for i := 0 to n - 1`, `by 2`),
  `for (T x : c)` a traversal (`foreach x in c`), Python's `for i in range(a, b, s)` a counting
  loop with a constant step, C++'s `for (auto& x : c)` a traversal; Structorizer reads both with
  the attributes we write. Any other for loop becomes its initialization and a while loop with
  the update at the end of its body; without a condition, and `while (true)`, it is an endless
  loop. `switch` becomes a case element: labels sharing a body are joined, the default goes last
  (`%` without one), a case that runs on gets the following statements copied (Java only: C#
  forbids it, so a C# case never runs on), and a switch expression's values (C#'s arms too, `_`
  as the default, `throw` as a jump) are assigned or returned where the expression stood.
  Several catches become a case on the exception's type, C#'s `when` filters in its labels.
  Python's `elif` chains are nested alternatives, `match` a case element (`case _` the default,
  a `break` there leaves the loop around it), `with` assigns its contexts like C#'s `using`, and
  the `else` of a loop follows it, the `else` of a `try` ends the try block (both named in the
  report). C++ declarations assign whatever form they take (`int x(1)`, `int x{1}`,
  `Foo f(a, b)` as `f := Foo(a, b)`), the initialization in `if (init; c)` comes before the
  alternative, and `catch (...)` is a label. Comments go to the element of the next statement,
  or of the one they end on. What has no element is kept as near as possible (`continue` and
  unreadable code as red, disabled instructions, a `synchronized` or `lock` body in place with
  the lock as a comment, a `using` resource assigned with `using` as a comment, `yield` and
  `goto` as written; labels and local functions left out) and named in the `Report`.
  `tests/snapshots.rs` holds every fixture method as a readable tree;
  `tests/structorizer_structure.rs` compares the nesting with Structorizer's own import of the
  same files (with `UMLCRAFT_NSD_REFS`), the deliberate differences listed in
  `tests/structorizer_deviations.txt`; the `nsd-import` example runs a whole source tree. The
  comparison covers Structorizer's C import too (`tests/fixtures/cpp/Plain.c`, with
  `UMLCRAFT_NSD_REFS`). Structorizer has no C# or Python import to compare with; besides the
  parity test, the example ran over Newtonsoft.Json (951 files, 8438 methods), requests and
  httpx (97 files, 1745 functions), and leveldb and yaml-cpp without its bundled googletest (232
  files, 2546 functions; 17 places tree-sitter cannot read, such as calls through member
  pointers), and Structorizer read the diagrams back as written (apart from its own keywords
  `input`/`output`, which it capitalizes).
  In the app the source import makes them, next to the classes (see "Source import"); each
  shows its report in a bar at the bottom of its view.

Every file Structorizer 3.32-35 wrote that we have reads and writes back byte for byte. The
tests check our own diagrams, one feature each, as Structorizer saved them
(`tests/fixtures/structorizer`), and a snapshot of a new diagram with every kind
(`tests/fixtures/new`, `UPDATE_SNAPSHOTS=1` rewrites it). With `UMLCRAFT_NSD_REFS` pointing at a
folder of further reference files kept outside the repository (Structorizer's samples and
Java imports, our measuring diagrams, a file saved by Structorizer's GUI on Windows, which
is byte for byte the Linux one; 258 files), `reference_files_round_trip` checks those too,
and `structorizer_drawing` draws all diagrams Structorizer can draw (389), rasterizes them with
resvg and compares their straight lines with Structorizer's PNG exports (`plain/…`, exported
with the GUI's settings and highlighting off); with the Windows table it does the same for the
exports made on Windows (`windows/…`, 187). Differences we understand are listed with their
reason in `tests/structorizer_deviations.txt`. Snapshots of the fixtures' layout and of one
SVG (`tests/drawing.rs`) guard the rest. `cargo run -p umlcraft-nsd --example nsd-svg --
FILE.nsd` draws a file.

## Entity-relationship diagrams

ER diagrams are a diagram kind of the project like the UML ones
(`DiagramKind::EntityRelationship`): their elements live in the model tree, they open as tabs
of the main window and are edited on the same canvas, with the same undo, clipboard, export
and file. Each diagram has a notation, Chen, IE (crow's feet) or IDEF1X (see "IE and IDEF1X"
below); the model is the same for all three. Entities (also weak and associative ones), attributes (keys, partial keys, derived,
multivalued, composite), relationships (binary, n-ary, identifying, with attributes) and
generalization (IS-A), with `[min,max]` on the lines read look-across (`[1,1]` at `Kunde` on
`Kunde – bucht – Ticket`: every ticket has one customer). The shapes are Chen's as draw.io
draws them; the text and lines follow UmlCraft's canvas. `umlcraft-erm-sql` makes such
diagrams from SQL dumps and writes them back as SQL.

- **Model** (`umlcraft-model/src/er.rs`, element kinds `erEntity`, `erAttribute`,
  `erRelationship`, `erIsA`, `erParticipation`, `erSubtyping`). An entity has `weak`,
  `associative` and its `physicalName`; an attribute its `key` (primary or partial),
  `derived`, `multivalued` and the database properties named as in Astah's ER model
  (`physicalName`, `datatype`, `lengthPrecision`, `notNull`, `defaultValue`; kept, not drawn);
  a relationship `identifying`; an IS-A `total` and `disjoint` (none, d, o). An attribute is
  owned by what it describes (an entity, a relationship or, composite, another attribute), so
  deleting the owner takes it along; its line is an edge showing the attribute, drawn from
  the owner, like a containment line. A participation is owned by its relationship (or
  associative entity) and refers to it and the entity (`cardinality`, `role`, `total` for the
  double line); a subtyping is owned by its IS-A and refers to it and the entity, the first
  one is the `supertype`. Both ends are strong references: deleting either deletes the line.
  `Cardinality` has a minimum (a number or `*`) and a maximum (a number, `n`, or `m` for the
  second "many" side of a relationship), written `[0,n]` and read also as `0..*`, `1`, `*`.
  The file format went to version 5 for these kinds and to 6 for the IE and IDEF1X
  properties (no migration needed; older versions refuse the newer files).
- **Connecting** (`notation/src/er.rs`). The palette has one line tool, `erLine`; what it makes
  follows from its ends (`joins`): an attribute and what it describes (the attribute moves
  under its owner), a relationship or associative entity and an entity (a participation with
  `[0,n]`), an IS-A and a plain entity (a subtyping). Unfinished diagrams are fine (a
  relationship with one entity, an IS-A without subtypes); what contradicts itself is refused
  with a message in the UI's language: an attribute that already has an owner, a second weak
  entity at an identifying relationship, pairs that cannot be joined. `add_attribute` gives an
  owner a new attribute above it, joined by its line (the canvas's "Add attribute").
- **Drawing** (`scene::layout_er`, `draw`). Rectangle (a second frame inside for weak
  entities), rectangle with a rhombus (associative), ellipse (dashed when derived, a second
  one inside when multivalued, the name underlined for a key and dotted for a partial key),
  rhombus (a second one inside when identifying), triangle (`Outline::Triangle`) with "IS-A"
  and `d` or `o`. Names wrap and the box grows to fit them. A total participation and the
  supertype line of a total IS-A are double lines (`EdgeShape::double`,
  `draw::double_line`); a participation shows its cardinality and role at the entity, placed
  and dragged like an association's multiplicity and role.
- **Layout** (`er_layout`), for "Arrange" and the SQL import. Entities, relationships and
  IS-As form the core graph; a relationship goes below the entity on its "one" side (the line
  whose cardinality has the maximum 1, which is the referenced table after an import) and
  above the others, an IS-A below its supertype. Each core node reserves room for its
  attributes, the boxes go to `layout::layered`, and a second round reserves the room on the
  sides that turned out to be free of lines. Attributes then go around their owner, keys
  first: rows of four above and below, columns of three at the sides, 30 px away, the free
  sides first and further out ring by ring; one that would cover a node, a line or a label
  slides along its side, moves out or takes the nearest free place around its owner, and
  when a label moved onto an attribute, the attributes are placed again. Lines are straight;
  several lines between the same two nodes get a waypoint each. A line that would run
  through another entity, relationship or IS-A (one that skips a layer passes a node there)
  goes around it (`detour`): one bend beside the middle of a side of the node or two along
  that side, halfway into the gap to the next node (at least 20 px, at most 30); of the eight
  ways the one passing the fewest other nodes, then the shortest with 120 px added for each
  line it crosses (twice that for one it would run on top of), node after node; bends that
  can go without passing a node or crossing more lines go afterwards. A spreading waypoint
  that fell into a node moves out to its nearer side, so parallel lines pass on both sides. A
  self-reference in IE and IDEF1X goes round the first corner of its table (top right, top
  left, bottom right, bottom left) where the loop and its labels keep clear of the other
  nodes (`place_loops`). Everything ends on the 10 px grid from (20, 20). `overlaps` lists
  what still covers what, lines through nodes included (the tests require none, the SQL
  fixtures in all three notations).
- **App.** The ribbon's diagram tab offers the palette (entity, weak and associative entity,
  the attribute kinds, relationship, identifying relationship, IS-A, the line, notes). The
  properties panel has the switches of each kind, the key, the database properties, and for
  a participation its cardinality, role and total participation; the canvas's context menu
  sets a participation's cardinality (`[0,1]`, `[1,1]`, `[0,n]`, `[1,n]`) and total
  participation and adds an attribute to an entity, relationship or attribute; a double
  click on a cardinality or role edits it in place. The sample project has a "Database"
  diagram. File › "Import SQL dump as ER diagram …" (`sql.rs`) reads the dump in the
  background, asks in a small dialog for the dialect (the detected one preselected), the
  attributes (all, keys only, none) and, if there are several, the schemas, and imports into
  the project as one undo step: a package named after the database with an ER diagram, which
  opens. The counts go to a notice; tables without a primary key, placeholder entities and
  statements that could not be read are listed in a message. File › Export › "ER diagram as
  SQL …" (and "As SQL" in the ribbon's tools tab, on an ER diagram) asks for the dialect,
  the one the diagram's package was imported from preselected (`imported_dialect`), then
  for the file.
- **Elsewhere.** The MCP tools know the kind (`entity_relationship`), create a diagram in a
  notation (`create_diagram`'s `notation`), switch it and set the ER properties with
  `update_element`, and add columns with `add_member` (`name : TYPE(length)`, `key`);
  `get_diagram` lists the tools of the diagram's notation. The `.asta` import and export
  carry Astah's ER models (see their sections).

### IE and IDEF1X

- **Model.** `DiagramProps` has `notation` (`ErNotation`), `erDisplay` (all columns, keys,
  the name; Astah's display level) and `physicalNames` (Astah's physical model), all skipped
  when default. A foreign key column is an `ErAttribute` of the child with `foreignKey`, a
  weak reference to the relationship or subtyping it comes from, so SQL dumps and Astah keep
  their real column names; Chen leaves such columns out. `unique` is a column's `UNIQUE`, an
  IS-A's `discriminator` a weak reference to an attribute of the supertype; `total` is
  Astah's "complete", the child of an identifying relationship is `weak` (a dependent
  entity).
- **Drawing** (`scene`, figure `ErTable`). The name above the box (centered inside when only
  the name is shown), primary key rows above a divider, the others below, compartments
  `ErKeys` and `ErColumns` for hit tests and new rows; `(FK)` after foreign key columns,
  aligned in a column (`ER_COLUMN_GAP`, `ER_FOREIGN_KEY_GAP`); rounded corners for weak
  entities. As in Astah, the node's bounds include the name band (`er_name_band`), so lines
  end above the name. A binary relationship without attributes of its own is one edge on the
  `ErRelationship` from the parent (the "one" side) to the child, dashed when it is not
  identifying and the parent is optional; its participations stay in the model and give the
  ends: IE markers for one, zero or one, one or more, zero or more; IDEF1X a diamond at an
  optional parent and a dot at the child with `P`, `Z` or a number. Other cardinalities keep
  their text. An IS-A is the subtype symbol (IE: a half circle on a bar with a cross when
  complete; IDEF1X: a circle over one or two bars) with the discriminator beside it;
  attributes are rows, not nodes.
- **Editing** (`notation/src/er.rs`). `palette::diagram_palette` gives IE and IDEF1X the
  tools identifying, non-identifying, many-to-many and subtype instead of `erLine`. Drawing
  one from parent to child creates the relationship, both participations (parent `[1,1]`,
  child `[0,n]`, many-to-many `[0,n]` both) and the edge, and copies the parent's primary key
  into the child as foreign key columns, into its key when identifying; a subtype line
  copies the supertype's key into the subtype's key. `set_identifying` moves those columns
  into the child's key or out of it. `set_notation` converts a diagram in one `Tx`: Chen →
  IE/IDEF1X removes the attribute nodes and their lines and replaces each binary
  relationship node and its two participation edges by one edge; the way back puts the
  relationship node between its entities and the attributes around their owners with
  `er_layout`'s attribute pass. Entities and IS-As keep their places. `parse_column` reads a
  row typed into a table (`name : TYPE(length)`).
- **App.** The ribbon's diagram tab has a Notation group; the properties panel of an ER
  diagram the notation, and in IE and IDEF1X what the tables show and physical names; a
  column's properties show `unique` and the relationship its foreign key belongs to. A double
  click below a table's last row adds a column. The sample project has "Database (IE)", a copy
  of "Database" in IE. The SQL import's dialog offers the notation.

### SQL import

`umlcraft-erm-sql` turns a dump of `mysqldump`, `mariadb-dump` or `pg_dump` (plain format)
into ER elements of the model. Its rules are written down in the comments of `map.rs`.

- **Dialect** (`dialect`): the dump tools' header in the first 2 KB, else backticks or
  `ENGINE=` outside comments mean MySQL, else PostgreSQL; the database's name from
  mysqldump's header line. `read` refuses `pg_dump`'s archive format and compressed files.
- **Splitter** (`split`): statements from bytes, because data may be binary and need not be
  UTF-8 and lines may be megabytes long. It knows strings (backslash escapes in MySQL and in
  PostgreSQL's `E'…'`), quoted names, `--`, `#` and `/* */` comments, MySQL's and MariaDB's
  version comments (their content is code; MariaDB's sandbox line is dropped), `DELIMITER`,
  `$tag$` bodies, the data after `COPY … FROM stdin;` and psql's backslash commands.
- **Parser** (`parse`, `ir`): its own small tokenizer instead of a full SQL parser, because
  only a few statements matter and a dump's many options must not cost a table. It reads
  `CREATE TABLE` (columns with their type as written, NOT NULL, default, comment, generated;
  inline and table keys and references; `PARTITION OF`), `ALTER TABLE … ADD` keys, references
  and columns, `ATTACH PARTITION`, `CREATE UNIQUE INDEX` on plain columns, `COMMENT ON TABLE |
  COLUMN`, `USE`/`CREATE DATABASE` for the name and `CREATE VIEW` (older mysqldumps write a
  placeholder table for each view first). Everything else is counted by its first words;
  what cannot be read becomes a `Problem` with its line and costs only itself. A statement
  that is not UTF-8 is read as Windows-1252. Partitions are folded into their parent.
- **Mapping** (`map`): junction tables (key = two or more foreign keys, not referred to)
  become relationships between the tables they join, with their other columns as
  attributes; a table whose key is a foreign key plus columns of its own a weak entity with
  an identifying relationship and partial keys; every other table an entity named with a
  capital first letter (schemas other than `public` in front). Foreign key columns are not
  attributes, they are the relationships: `[1,1]` or `[0,1]` (nullable) at the referenced
  table, `[0,n]` or `[0,1]` (unique) at the referencing one; a junction table's second end
  is `[0,m]`. A relationship is named after
  the foreign key column without `_id` and the like, "hat" if that only repeats the target
  table; a self reference carries a role. A missing target becomes a placeholder entity.
  Options: all attributes, keys only or none, and the schemas to take. The `Report` counts
  tables, entities, relationships, attributes and skipped statements and lists hints and
  problems. `map` creates a package named after the database (or the file) with an ER
  diagram of the same name, in a `Tx`, so the app's import is one undo step; `import` also
  lays it out with `er_layout`.
- **Tests.** The test schema (every import case, `tests/fixtures/schema`) as dumped by MySQL
  8.4 and 9.7, MariaDB 11.8 and PostgreSQL 16 and 17.6, three dumps each (without data, with
  data, with routines): the splitter finds as many statements as the prototype the rules
  were checked with, every dump gives the expected table (`tests/expected/*.md`, the form of
  the specification), the sources give the same, the laid-out result passes the model's
  check, has no overlaps and survives the file format, and its picture stays as it is
  (`tests/snapshots`, `UMLCRAFT_ERM_BLESS=1`). With
  `UMLCRAFT_ERM_REFS`, `tests/corpus.rs` imports Sakila, Pagila, Employees, Chinook and
  Northwind (27 dumps, outside the repository for their licences) and compares them with the
  prototype's results. `cargo run --release -p umlcraft-erm-sql --example sql-import --
  DUMP… [--attributes keys] [--out DIR]` prints counts, size, overlaps and times (a 40 MB
  dump of INSERTs reads in well under a second) and writes `.ucraft` and SVG.

### SQL export

`umlcraft_erm_sql::ddl(model, diagram, dialect)` writes the entities, relationships and
IS-As an ER diagram shows (in any notation) as SQL for MySQL/MariaDB or PostgreSQL; `Ddl`
counts the tables and foreign keys and lists what it `left_out`.

- **Tables.** An entity is a table named by its physical name, else its name (`schema.table`
  becomes a schema of its own in PostgreSQL, created first). Attributes are columns, a
  composite attribute's parts instead of it; a multivalued one has no column. Types and
  lengths go together again as the import took them apart (`int(10) unsigned`,
  `timestamp(3) without time zone`); no type is `varchar(255)`. Keys and partial keys make
  the primary key and are `NOT NULL`. Defaults that are numbers, keywords
  (`CURRENT_TIMESTAMP` …) or calls stay as they are, others become strings; PostgreSQL's
  `nextval(…)` of the dump's own sequence becomes an identity column. A derived attribute
  with an expression (the import keeps a generated column's expression as its default) is a
  generated column. Comments go inline in MySQL and as `COMMENT ON` in PostgreSQL. Names
  are quoted only where they must be: not plain, a reserved word, and in PostgreSQL with
  capitals.
- **Foreign keys** come last as `ALTER TABLE … ADD CONSTRAINT`, so the order of the tables
  does not matter. Foreign key columns the model has (from a dump, Astah or the IE and IDEF1X
  tools) reference the columns of the other table they are named after (`gate_nummer` →
  `nummer`, keys first), else its key in order; a referenced set that is not the key gets a
  `UNIQUE`. Without such columns, a binary relationship puts new ones into the table across
  from the side with `[x,1]` (look-across), named after the parent's role, else the
  relationship when the two tables share several or it is a self-reference, else the parent
  table (`kunde_id`); required when that side's minimum is 1, in the key when identifying or
  when a one-to-one child has no key of its own, `UNIQUE` for one-to-one. A relationship of
  more than two entities, one with attributes, a many-to-many one and an associative entity
  are tables of their own whose columns reference each entity and make the key (unless an
  associative entity has one). A subtype references its supertype, with the supertype's key
  as its own when it has none.
- **Tests.** Each of the five fixture systems goes dump → ER → SQL → ER and gives the same
  model again (`tests/ddl.rs`); a drawn Chen diagram without foreign key columns gets each
  kind of foreign key (`ddl::tests`), and the SQL reads back without a problem.

## MCP server

AI clients (Claude Code, Claude Desktop, Cursor) read and edit UML models through the Model
Context Protocol. `umlcraft-cli mcp` is the server they start; it speaks JSON-RPC 2.0 on stdin
and stdout, one message per line (`umlcraft_mcp::protocol::serve`: `initialize`, `ping`,
`tools/list`, `tools/call`; protocol versions 2025-06-18, 2025-03-26 and 2024-11-05).

### Tools and hosts

`umlcraft_mcp::tools` lists 32 tools and runs them against a `Host`, which hands out the
`Document`, a `TextMeasure`, the path and, in the app, the active diagram and the selection:

- reading: `get_document`, `list_elements`, `get_element`, `get_structogram`, `get_diagram`,
  `render_diagram` (a PNG through resvg, as MCP image content, so the client sees what it
  drew; of a structogram too), `screenshot` (the app's window, see below);
- editing: `create_diagram`, `create_element`, `create_model_element`, `show_element`,
  `connect`, `add_member`, `create_structogram`, `update_structogram`, `edit_structogram`,
  `wrap_in_fragment`,
  `add_operand`, `update_element`, `move_node`, `arrange_diagram`, `route_lines`,
  `delete_element`, `remove_from_diagram`, `batch`, `undo`, `redo`;
- the project: `open_diagram`, `new_document`, `open_file`, `save_file`, `export_image`,
  `export_sql` (an ER diagram as `CREATE TABLE` statements, in the reply or to a file).

Every editing tool is one undo step made the way the app makes it:
`umlcraft_notation::edit::apply` fits auto-sized boxes and lets routed lines follow, the same
function `Workspace::edit` uses; diagrams come from `edit::create_diagram`, nodes and lines
from the palettes (`palette::create_node`, `palette::connect`), members from
`member::apply_member_text`, fragments and operands from `palette::wrap_in_fragment` and
`palette::add_operand`; `update_element` gives a fragment that becomes an `alt` its `else`
(`palette::add_else`), and `delete_element` takes regions and operands out with
`state_edit::delete_region` and `sequence_edit::delete_operand`, as the app does. A tool that fails rolls back and answers with the error, which
for an unknown tool key lists the valid ones. Ids are UUID strings; kinds are the element
kinds' `type_name`s. A node without a position goes to a free place right of the drawing
(`free_origin`).

`batch` runs a list of calls as one undo step, all or nothing. A string argument
`$<index>.<field>` takes that field from the reply of an earlier call in the list, so a batch
can create a class and add members to it. The tools run as they would alone; afterwards
`Document::squash_since` joins their steps into one, and a failing call ends the batch with
`Document::roll_back_to`, which takes the steps back without leaving them to redo. Undo, redo,
nested batches and opening or starting a project are refused inside a batch.

Structograms travel as a JSON tree instead of Structorizer's XML (`umlcraft_mcp::structogram`):
the root's title, kind and keywords, and a `body` of elements, each with a `kind` (`instruction`,
`call`, `jump`, `if`, `case`, `for`, `while`, `repeat`, `forever`, `try`, `parallel`), its
`text` (lines joined by `\n`) and its parts (`body`, `then`/`else`, `catch`/`finally`, labeled
`branches` for a case, the last one the default). `get_structogram` writes what
`create_structogram` reads, so a client can read a structogram, change the tree and hand it to
`update_structogram`, which keeps what the tree leaves out (title, kind, header) and goes
through `Tx::set_structogram`, so an imported structogram counts as edited by hand. Elements
are made with `umlcraft_nsd::edit::new_element`, as in the editor. `edit_structogram` inserts,
replaces or removes one element at a path into that tree (`body.1.then.0`) and writes the
whole tree back the same way. `render_diagram` and `export_image` draw structograms with
`umlcraft_nsd::svg`; `export_image` also writes a `.nsd` file (`content::to_file`).

In sequence diagrams `connect` makes found and lost messages from one lifeline,
`update_element` sets what a fragment covers and spans its frame (`sequence_edit::cover`), and
`move_node` moves messages and fragments by height as dragging does (`palette::move_to_slot`
at the scene's `slot_at`), so they change operands; `get_diagram` gives messages their `y`,
fragments the range of each operand and lists the bars.

Two hosts exist. `Headless` holds a project of its own, measured with `ApproxMeasure`; in the
app `AppHost` (`crates/umlcraft-app/src/live.rs`) wraps the `Workspace`, labels each step
"AI: tool" and leaves opening another file or a new project to the window, which refuses it
while the open project has unsaved changes.

### The live bridge

When the app runs, the server forwards every call to it, so the user watches each change and
undoes it with Ctrl+Z. The app listens on a free port of 127.0.0.1 (`live::LiveServer`) and
writes `live-<pid>.json` with port, token (two random UUIDs), pid and version into
`live::control_dir()`, and removes it when it closes; on start it also removes the files of
apps that were killed and no longer answer. The folder is `~/.umlcraft/control` on Windows
(not `%LOCALAPPDATA%`: clients installed as MSIX packages, and the servers they start, see a
private copy of that), `~/Library/Application Support/UmlCraft/control` on macOS and
`$XDG_RUNTIME_DIR/umlcraft/control` on Linux (else the state folder), readable by the user
alone (0700 and 0600 on Unix). A request is a line `{token, name, arguments, access}`, the
answer an MCP tool result; a wrong token closes the connection. The connection's thread hands
the call over an `async-channel` to `MainView::serve_mcp`, which runs it on the main thread
inside `Workspace::update` and so never races a user edit. `_ping` is answered on the server
thread, and a call the app has not answered after 120 s (a modal dialog) comes back as an
error.

`screenshot` is the one call `serve_mcp` does not hand to `tools::call`: it needs the window,
not the project (`live::screenshot`). It takes the window's handle
(`umlcraft_screenshot::Target`) and captures on a background thread 100 ms later, so the
window has drawn the last call's change and its own thread is free when Windows asks it to
draw for the capture. `umlcraft-screenshot` calls `PrintWindow` with `PW_RENDERFULLCONTENT`,
which gets GPUI's DirectX content as composed, also under other windows, at physical pixels
(the thread is per-monitor DPI aware) and crops the invisible resize borders
(`DWMWA_EXTENDED_FRAME_BOUNDS`); `png::from_rgba` shrinks it to `scale` and encodes it. That
crate is the only one with `unsafe` code, which the workspace forbids elsewhere; on other
systems it answers that screenshots are not supported yet. Headless, or inside `batch`, the
tool answers with an error.

`backend::Auto` picks per call: the app when one answers (`LiveClient::discover`, newest file
first), otherwise the headless project. Once the headless project was changed or a file
opened in it, the server stays headless, so a model is never split between two places; a lost
connection to the app gives one error and the next call looks for the app again.
`--headless`, `--in FILE` and `--connect PORT --token T` fix the choice.

### Files

The server opens and saves projects and writes images only in the folders it was allowed:
`--allow-path DIR` (several times), `--allow-any-path`, and the folder of `--in`. `Access`
travels with each live call, so the app keeps to the folders of the server that asked.
Paths are canonicalized before the check; saving to the open project's own path is always
allowed. `umlcraft-cli mcp setup` registers the server with each client it finds (Claude
Code through `claude mcp add --scope user`, Claude Desktop's `claude_desktop_config.json`,
Cursor's `~/.cursor/mcp.json`, merged into what is there) and allows the Documents folder
unless told otherwise; `--dry-run` shows what it would do.

`umlcraft-cli` ships in the Windows zip and installer, the Linux tarball, `.deb` and `.rpm`
and inside the macOS app (`UmlCraft.app/Contents/MacOS/umlcraft-cli`). Flatpak, Snap and
AppImage carry none: a client outside their sandbox could not start it.
