# Changelog

What changed from release to release. The [README](README.md) describes what UmlCraft can do
today.

## Unreleased

- **Calls drawn further down**: a message an AI client draws at a given height from a
  lifeline that was called and has not answered yet belongs to that call, however far below
  it lies: it is numbered within it and starts no second activation, and a fragment around it
  leaves the reply outside.
- **Fragments below loops to self**: putting the messages after a message to self into a
  fragment keeps the frame below its loop and the activation it starts. The text of a long
  message to self stands right of the activation it leaves instead of across it.
- **Replies to self**: a reply placed close below a call to self leaves the activation that
  call started at its full height, so it reads as returning from there into the outer one.

## 0.21.1 – 2026-11-11

- **Loops beside other lines**: a new loop goes to a corner whose sides other lines leave
  free, also when it is drawn rather than arranged. At the left corners a loop comes back
  through the left side, so two loops at the bottom (or top) of a box end on different sides.
  Their roles and multiplicities stand beside their own ends, a short one inside its loop.
- **Lines beside a generalization tree**: arranging keeps other lines off the lines of a
  generalization tree, neither crossing them nor running close beside them, and their ends
  clear of the triangle. An association beside such a tree keeps its role and multiplicity on
  its outer side, at its end, instead of next to another class.
- **Specialized actors side by side**: arranging a use case diagram puts the actors that
  specialize the same actor next to each other below it, each with its own line up to it.
- **Created lifelines follow their message**: when a fragment put around messages moves them
  down, the head of a lifeline a create message makes moves along, and the frames no longer
  reach over the reply that follows.
- **Message texts clear of bars**: a message text that would lie on the activation bar of a
  lifeline its line crosses moves to where the line is free.
- **AI clients** show only the slots of an object that have a value with
  `empty_slots: false`.
- **The AppImage** is named after its architecture, `UmlCraft-0.21.1-x86_64.AppImage`. The
  release also carries it under its former name, so that older versions still update.

## 0.21.0 – 2026-11-11

- **The AppImage starts on more systems**: it brings the xkbcommon libraries along, which a
  desktop without them (e.g. Wayland only) lacked.
- **New diagram** lists the diagram kinds and the structogram in alphabetical order of their
  names in the language shown, without a separator before the structogram.
- **AI clients keep to their folders**: a path that leaves an allowed folder with `..` after a
  folder that does not exist yet is refused on Linux and macOS too, and folders given to
  `--allow-path` are made absolute, so that they mean the same wherever the client starts the
  server. `--connect PORT` takes the token from the app's announcement; `--token` is gone,
  as other users of the computer could read it on the command line. A connection has five
  seconds and 64 KiB to show the token, and at most 16 may wait for that at once; those that
  showed it stay as long as their client runs, however many.
- **`mcp setup` keeps the clients' settings**: a configuration it cannot read or that is no
  JSON object is left as it is instead of being replaced, it is written in one go, and an
  existing Claude Code entry is replaced only when the new one could not be added.
- **Deeply nested structograms**: a `.nsd` file nested deeper than 200 levels is refused
  with a message instead of crashing UmlCraft.
- **Regions stay put**: when a state with regions grows to the left or upwards around a
  substate, the lines between its regions keep their place.
- **Updates**: the downloaded package is checked once more right before it is started, and
  held so that nothing can replace it meanwhile.
- **Messages in your language**: refused edits (an element already on the diagram, lines
  that cannot connect, typing errors in attributes and operations …) are told in German or
  English instead of in English after a German introduction.
- **Lines between the same states** are routed together, but moving a node no longer
  reroutes the lines of its neighbours to other nodes.
- **Several loops on one node**: a second association or transition from a node to itself
  goes to the next corner, clockwise from the top right one, instead of on top of the first;
  arranging the diagram puts loops that lie on each other apart.
- **No package along with a new diagram**: a diagram made at the top of the model goes right
  into it instead of into a new package of its own name. Packages are made on purpose, in
  the model tree or with the Package tool; a diagram made in one stays there.
- **Use case diagrams arranged as in textbooks**: the use cases of a system boundary stand
  one below the other, those of one actor next to each other; the actors stand left and
  right of the boundary, level with their use cases, a specialized actor below its general
  one, and their lines run straight.
- **Arranging from left to right** leaves room for long roles and multiplicities between the
  boxes, and puts boxes without lines in one row instead of a column.
- **Arranging keeps generalizations apart** that were chosen to be drawn separately instead
  of joining them into a shared tree again.
- **Roles keep to their own line**: on a loop the role and multiplicity of each end stay
  beside that end, and on lines that run side by side between the same two boxes they go to
  the outer side.
- **Objects** are as large as they are drawn, so that links between them stay straight after
  arranging.
- **Messages with arguments**: typing `name(a, b)` for a message takes what is in the
  parentheses as its arguments instead of as part of its name.
- **Replies** moved below the end of their activation make it longer instead of stopping at
  its end.
- **Combined fragments** put around messages keep clear of the messages above and below
  them, and a fragment inside another one stands inset from its edges.
- **Associations between two actors or two use cases** are no longer offered.
- **AI clients**:
  - make object diagrams, and see them as such instead of as class diagrams;
  - set the role, multiplicity, navigability and aggregation of association ends,
    `abstract` and `static`, the class of an object and the values of its slots;
  - turn message numbers, return values and the frame of sequence diagrams on and off;
  - put a reply where `y` says;
  - draw a line the palette would not offer with `allow_invalid`, for exercises in finding
    mistakes;
  - measure texts like Arial without the app, so that arranged diagrams look the same with
    and without it.

## 0.20.0 – 2026-11-07

- **AI clients select in structograms**: `select` takes a `path`, named as for
  `edit_structogram` (`body.1.then.0`), and selects that element of the structogram shown;
  the structogram's own id selects it as a whole, an empty list clears the selection. The
  properties panel then edits what is selected, as after a click.
- **Empty `finally` by click**: a click into the bottom band of a try without a `finally`
  selects the empty `finally`, so that elements can be pasted or inserted into it.
- **Internal transitions inline**: double-clicking an internal transition in a state opens
  it with its text (`trigger [guard] / effect`) instead of an empty field.
- **Names right after placing**: a new class, object or other box opens its name for typing
  as soon as it is placed.
- **Regions**: dividing a small state into regions enlarges it so that each region has
  room, and a selected region is outlined by itself rather than by its whole state.
- **States moved into composite states** drop the bends of their transitions to and from
  outside, which otherwise ran from where the state was before.
- **Swimlanes** move only sideways and within their row. Dropped past a neighbour's middle, a
  lane takes its place, and the lanes close up in the new order, each with its contents.
- **Properties after switching diagrams**: the properties panel no longer shows an element of
  the diagram shown before.
- **Undo in structograms** selects the element it brought back or changed.
- **Substates stay visible**: a state, package or other container placed inside another one
  is drawn in front of it; before, every new container went to the very back, so a substate
  added to a composite state vanished behind it.
- **Stereotypes on states**: a state shows its stereotypes above its name.
- **AI clients write state activities**: `add_member` on a state takes `entry / …`,
  `do / …` and `exit / …`, or an internal transition `trigger [guard] / effect`;
  `update_element` changes or clears `entry`, `do` and `exit` and rewrites an internal
  transition's `text`.
- **AI clients move as a drag does**: `move_node` takes what lies in a container along, and a
  state moved into another one becomes its substate, drawn in front of it.
- **Transition labels** typed in place or given by AI clients (`connect`, `update_element`)
  are split into trigger, guard and effect, as `e2 [x > 0] / z++` reads.
- **States grow for their activities** when one is added or made longer, and a loop from a
  state to itself keeps to the state's corner when the state grows or moves, so its label
  stays by the loop.
- **Substates stay inside**: a state dropped into a smaller one, or moved over its state's
  border, makes that state grow around it, and arranging a state machine diagram grows each
  composite state around its substates, however far they reached out of it.
- **Opposite transitions** between two states side by side no longer lie on one line: a new
  or moved line is routed together with the lines that share one of its states.
- **Empty canvas without a diagram**: with no diagram open, the window shows the canvas with
  its grid and the zoom instead of a blank white area with a hint. It only pans and zooms
  until a diagram is opened.
- **Turned calls and jumps** in case elements with many branches keep the room for the bars
  of a call and the point of a jump, as in Structorizer: their text no longer runs over the
  bar.

## 0.19.0 – 2026-11-04

- **Structograms on the diagram canvas**: structograms are drawn like the other diagrams,
  sharp at every zoom and on the same grid, and open, zoom and scroll without delay even when
  large.
- **Structogram elements in the properties panel**: text, comment and whether an element is
  disabled, and the structogram's signature, comment and kind, are edited in the properties
  panel instead of a dialog, and the drawing follows the typing. A new element goes in at
  once, ready for its text; one undo takes back the element together with what was typed.
- **ER diagrams in IE and IDEF1X notation**: besides Chen, an ER diagram can be drawn with
  crow's feet (IE) or in IDEF1X, chosen per diagram in the ribbon or the properties;
  switching converts the diagram and keeps the entities where they are. Entities are tables
  with the primary key above a divider and the other columns below, foreign key columns
  marked `(FK)`, with or without types, with logical or physical names, all columns, keys
  only or the name alone. Identifying, non-identifying and many-to-many relationships are
  drawn from parent to child and copy the parent's key into the child; making a relationship
  identifying moves those columns into the child's key. Subtypes have their symbol and a
  discriminator. The sample project shows its database in both notations.
- **Astah's ER diagrams**: `.asta` files bring their ER models along (entities, columns with
  their keys, types, lengths, defaults and foreign keys, relationships with cardinalities and
  verb phrases, subtypes and their groups, the diagrams in IE or IDEF1X with their layout),
  and the `.asta` export writes them back. Chen diagrams go out in IE notation.
- **SQL dumps** keep their foreign key columns as columns of the tables, and the import
  dialog offers the notation.
- **ER diagrams as SQL**: File › Export › "ER diagram as SQL …" writes `CREATE TABLE`
  statements for MySQL/MariaDB or PostgreSQL. Relationships become foreign keys,
  many-to-many relationships tables of their own, subtypes take their supertype's key. A
  dump of one database written for the other gets that one's types.
- **Counted columns, virtual columns and referential actions**: `AUTO_INCREMENT`, identity
  and `serial` columns, virtual generated columns and the `ON DELETE`/`ON UPDATE` of foreign
  keys come along from SQL dumps and go back into the SQL; the properties panel sets them on
  attributes and relationships.
- **Domains, `CHECK` and indexes**: SQL dumps bring along their domains and enumerated
  types, conditions on columns and tables and their indexes, the export writes them, and the
  properties panel edits them. They go into `.asta` files and come back from them as far as
  Astah has them (domains with type, length, `NOT NULL` and default; indexes).
- **Abstract classes** whose name already says so (`Person {abstract}`, `Person
  <<abstract>>`) no longer show `{abstract}` a second time below it.
- **`.asta` files**: tables show the type of a column's domain, their names sit where Astah
  puts them, and boxes that an older Astah sized for its smaller line spacing keep their
  size.
- **Arranging ER diagrams**: lines that go around a table keep clear of the other lines
  where they can and no longer run on top of each other.
- **ER diagrams in Chen** can read cardinalities at the entity, as `(min,max)` of how often
  each instance takes part, instead of across the relationship (still the default): a switch
  in the diagram's properties. Between two entities the diagram only swaps the ends; for
  relationships of three or more entities the value at the entity is entered on its own.
- **Routing lines**: a line leaves and enters a box where it runs straight, not only in the
  middle of its side, and lines that meet a box on one side keep apart, so that the roles and
  multiplicities at their ends have room; those labels also move around a bend close to the
  box instead of onto the line, and keep off arrowheads, diamonds and crosses. Lines keep
  clear of a box's line to itself, which stays at the box's corner when the diagram is
  arranged, and generalizations share one line only where it runs past no other box.
- **AI clients** create ER diagrams in a notation, switch it with `update_element`, add
  columns with `add_member`, set `unique`, `foreign_key`, `check`, `auto_increment`,
  `virtual`, a table's `checks` and `indexes`, a relationship's `on_delete` and `on_update`
  and the discriminator, create domains with `create_model_element`, and get an ER
  diagram as SQL with `export_sql`.

## 0.18.0 – 2026-11-03

- **ER diagrams**: a new diagram kind for entity-relationship diagrams in Chen notation, in
  the project like the UML diagrams and drawn on the same canvas: entities, weak and
  associative entities, attributes (keys, partial keys, derived, multivalued, composite),
  relationships (binary, n-ary, identifying, with attributes) and IS-A, with `[min,max]` on
  the lines. They are edited in place, in the properties and from the context menu, arranged
  automatically, and exported as SVG and PNG; the sample project has one. File › Import SQL
  dump as ER diagram … turns a dump of MySQL, MariaDB or PostgreSQL into a laid-out diagram
  in the open project, with all attributes, keys only or none.
- **Light or dark as the system**: without a theme of its own, UmlCraft follows the system's
  appearance and switches along with it. The View tab and the settings choose between System,
  Light and Dark.
- **Messages in UmlCraft's own dialog**: errors and questions such as the one before unsaved
  changes are discarded show in a dialog of UmlCraft's own on every platform, in its theme.
  Long texts wrap instead of being cut off, as they were on Linux.
- **Association classes, constraints, shared generalizations and robustness icons** in class
  diagrams: an association class hangs on a dashed line from the middle of its association and
  shows its name on the line; draw one with its tool or turn an association into one from the
  line's context menu. Attributes, operations and lines show their constraints (`{age > 0}`),
  edited one per line in the properties panel; a line's constraint labels are dragged
  together, as in Astah, and keep their distance when the line moves. Generalizations to one
  class can share a single triangle, joined on a common line (context menu › Shared style);
  arranging a diagram does this for class trees. «boundary», «control» and «entity» classes
  are drawn as their icons, with their own tools, or as boxes. AI clients set all of this with `connect` and
  `update_element`. `.asta` files bring them along, constraint labels moved by hand included;
  the export to `.asta` keeps everything but the shared triangle and constraint labels on a
  line that moved apart from each other or sit on a line other than an association.
- **State invariants, terminations, duration and time constraints** in sequence diagrams: a
  state invariant shows a condition on a lifeline in a rounded box, a termination ends a
  lifeline with a cross, which stays below what happens on it. A duration constraint
  (`{d..3*d}`) is dragged from one message to another and keeps reaching from one to the other
  when they move, its label beside the line; a time constraint (`{t..t+3}`) goes on a message
  with one click, and the diagram's frame grows to hold its label. Both labels can be dragged
  to another place. A longer condition of a state invariant wraps, and its box grows and
  shrinks with it. The properties panel edits their text. AI clients create them with
  `create_element` and `connect`. `.asta` files bring all four along, labels moved by hand
  included; the export to `.asta` keeps state invariants and terminations but leaves out
  duration and time constraints.
- **Submachine states, entry and exit points, stub states**: a submachine state stands for
  another state machine, chosen in its properties, and shows `include / Machine` and the
  submachine icon. Entry and exit points sit on the border of a state or of the diagram's
  frame and stay there when it moves or changes size; stub states go into submachine states.
  Transitions start and end at all of them. AI clients create them with `create_element` and
  choose the machine with `update_element`. `.asta` files bring them along; the export to
  `.asta` keeps submachine states but leaves out points and stub states.
- **More of the activity diagram**: send signal, accept event and accept time event actions,
  call behavior actions that call another activity, input and output pins on the border of an
  action, activity parameter nodes on the diagram's frame, and connectors that continue a flow
  elsewhere. Object nodes, pins and parameters have an ordering, an upper bound, a stream
  flag, an effect (create, read, update, delete) and a control flag, shown below them, which
  makes control pins; exception pins and object nodes are marked by a small triangle. Pins and
  parameters stay on their border when the action or the frame moves or changes size. AI
  clients create them with `create_element` and set them with `create_element` or
  `update_element`; Astah's `.asta` files bring them along both ways. Arranging an activity
  diagram leaves room for pin names and the properties below object nodes, keeps the lines off
  them where it can, puts a pin alone on a side of its action level with the action's bottom so
  its texts stand below it, and draws the frame around the texts.
- **Composite structure diagrams**: the inside of a class or component, with parts drawn in
  it, ports on the border of the class and of typed parts, and connectors between parts and
  ports. A part dragged over the class's border takes the border along; ports stay on their
  border when it moves or changes size. The sample project shows the dock controller of the
  bike rental this way. AI clients create them as `composite_structure` and place parts and
  ports with `create_element` and `on`. Astah's `.asta` files bring them along both ways, with
  the interfaces a port provides drawn as a ball and those it requires as a socket.
- **Required interfaces as sockets**: an interface shown as a circle can be shown as a socket
  instead (properties panel, "Show as socket"), the half circle of an interface a class or
  port requires. Its opening turns away from its line, and a dependency to it is drawn as a
  plain line without «use».
- **Structograms in the dark theme**: the editor draws structograms in the dark colours of the
  diagrams, light lines and texts on dark grey, with the elements' own colours dimmed, instead
  of a bright white sheet. Exports and the clipboard keep Structorizer's black on white.
- **No comment markers in structograms**: elements with a comment no longer carry
  Structorizer's grey bar, in the editor, in SVG and PNG exports and in images for AI clients.
  The comments themselves stay, also in `.nsd` files.
- **AI clients take screenshots without disturbing**: the MCP server's new `set_window` tool
  sizes the UmlCraft window in the pixels of `screenshot` and moves it beyond the screens,
  where it keeps drawing but nobody sees it, to the taskbar or back onto a screen, without
  taking the focus. `screenshot` now also captures a minimized window, which the user does not
  see come back.
- **AI clients set the view**: `set_view` switches theme and language and zooms the diagram or
  structogram shown (to fit, or to a percentage); `select` selects elements on the diagram
  shown; `new_document` with `sample` opens the sample project; `open_diagram` also opens
  structograms.

## 0.17.0 – 2026-11-02

- **NixOS**: a Nix flake builds UmlCraft from source, with its launcher, icon and file types
  (`nix run github:udvlp/UmlCraft`).
- **The diagram fills the window**: ribbon, model tree, properties and zoom float over the
  diagram as light grey cards, so the diagram uses the whole window. Drag a card's inner edge
  to make it wider or narrower. Zoom to fit, 100 % and paste use the part of the diagram the
  cards leave free.
- **A calmer ribbon**: larger icons and labels, more room between buttons, short lines
  between groups, and the open tab and the selected tool in a pale blue.
- **Pan with the right button, zoom with two fingers**: holding the right button and moving
  the mouse pans diagrams and structograms; the context menu opens when the button is
  released without moving. Spreading or pinching two fingers on a touchpad zooms.
- **No status line**: errors and notices appear as toasts at the bottom right; opening,
  importing and exporting show what is running above the zoom.
- **New projects start empty**: no class diagram of their own, on the Home tab.
- **What a class box shows of its members**: per box, the properties can hide stereotypes,
  the members' visibility, attribute types, initial values, parameters, parameter types,
  return types and the stereotypes of attributes and operations, choose which parameter
  directions the operations show and which visibilities of attributes and operations the box
  shows. Astah projects keep these options in both directions of the import and export.
- **Colored names and lines**: names in their own font color and lines in their own color, as
  in Astah projects, are drawn and exported that way. The properties panel picks both colors
  for the selected box or line, or resets them to the default.
- **Arrange automatically for more diagrams**: activity, state machine and use case diagrams
  can be arranged too. Activities run top to bottom along their flows, partitions stay columns
  with every step in its own, and fork and join bars span what they join. State machines run
  left to right along their transitions, composite states hold their substates and each region
  is laid out on its own. Use cases go into the system boundary with actors beside it, level
  with their use cases. The Arrange button picks the direction that suits the diagram; its menu
  still offers both.
- **Parameter directions as in Astah**: boxes from Astah projects show `out` and `inout` only
  where Astah does (when the box shows directions, then with `in` as well); boxes made in
  UmlCraft keep showing `out` and `inout`.
- **"abstract" once**: an abstract class that has the stereotype «abstract» of its own no
  longer shows `{abstract}` below its name as well, nor «abstract» twice.
- **AI clients make several changes as one step**: the MCP server's `batch` tool runs a list
  of calls as one undo step, all or nothing, and later calls can use the ids earlier ones
  returned.
- **AI clients build whole sequence diagrams**: they can put messages into fragments such as
  `alt` and `loop`, add and remove operands with their guards, and set a message's sort,
  guard, arguments, return value and the operation it calls, draw found and lost messages,
  choose the lifelines a fragment covers and move messages and fragments up or down, also into
  another operand. Deleting a region or an operand through the MCP server now leaves the
  diagram as tidy as deleting it in the app.
- **AI clients write structograms**: they create a structogram from a tree of instructions,
  branches and loops, optionally for an operation, read it back in the same form, change it and
  look at it as an image. The project overview lists the structograms. They can also change
  single elements of a structogram and export it as an image or a Structorizer file.
- **AI clients see more of a sequence diagram**: `connect` names the message it draws, and
  `get_diagram` lists the activation bars with their lifeline, top and bottom.
- **AI clients see the whole window**: the MCP server's `screenshot` tool returns an image of
  the UmlCraft window with ribbon, panels and diagram, also while other windows cover it
  (Windows only so far, and only while the app runs).
- **Sequence diagrams keep their shape as they grow**: a message drawn below the others from a
  lifeline still busy with a call it has not answered continues that call's bar instead of
  starting a new one. A fragment put around a message keeps the message's label clear of its
  tab, and a new operand makes room below the fragment instead of taking in what follows it.
- **Project tree with chevrons**: as in the Windows Explorer, a chevron in front of an item shows
  whether it can be opened (pointing right) or is open (pointing down). A click on the chevron
  opens or closes the item, a click on its name only selects it.
- **An icon for every item in the project tree**: the project, attributes, operations,
  parameters, enumeration literals, slots, template parameters, extension points, interactions,
  activities, state machines, operands and regions have icons of their own, so the tree reads
  evenly.
- **Names and notes on one baseline**: the version beside the name in the title bar and the kind
  beside each name in the project tree stand on the name's baseline. The title bar's icons are
  as large as the ribbon's small ones.
- **Structograms on white**: the structogram editor is white around the structogram, as around
  a diagram, and an opened structogram starts beside the floating cards. Save for Structorizer
  moved to Tools › Exchange, next to Import and Export, and is greyed out unless a structogram is
  open.

## 0.16.0 – 2026-11-01

- **Messages stay on their lifelines**: a message drawn or dragged above the heads goes right
  below them, together with the bar it is sent from, and never below a lifeline's cross; a
  destroy message goes below what happens on its lifeline, also when dragged up, and a create
  message stays above it. A create message takes the head and the constructor's bar along, also
  when it moves with the call it is sent from, and a lifeline is created only once. No bar
  reaches past a lifeline's cross, not even the one of a caller waiting for the object that
  destroys it.
- **Aligning follows the diagram's rules**: aligned lifelines only move sideways and the
  sequence diagram is tidied after them; aligned containers take what lies inside along, and
  aligned states go into the state they now lie in.
- **Operands keep their room**: an alt or another fragment with several operands made smaller
  stops below its last dashed line instead of leaving it outside. A fragment dragged down into
  the one it held goes inside it right away.
- **Substates go along**: moving a state takes its substates along, also those that reach out
  of it; resizing a state puts it and its substates into the state or region they now lie in,
  as moving them does.
- **New app icon**: a provided interface in its socket, the ball-and-socket notation of
  components, which also forms a U. It replaces the state machine motif, which looked like any
  flow icon in the taskbar.
- **New versions in the title bar**: at its first start UmlCraft asks whether it may look for
  new versions on GitHub. If you agree, it checks at most once a day, and a new release shows
  as a button in the title bar that opens its page. About UmlCraft can check at any time. What
  the check sends is shown in the app, offline, and under [Privacy](README.md#privacy).
- **Updates install themselves**: installed with the Windows installer, as an AppImage or as the
  macOS app, UmlCraft downloads the new release when you click the update button, checks it
  against the release's checksums, asks to save your changes, installs it and starts again.
  Other packages keep opening the release page.
- **Bars stay in their operand**: in an alt or another fragment with several operands, the
  dashed line between two operands moves down below the activation bars of the one above, as
  long as there is room before what comes next.
- **Destroy messages take their cross along**: dragged up or down, a destroy message moves the
  end of the lifeline it destroys with it.
- **New diagram in the Home tab**: the button that creates diagrams moves from its own box left
  of the ribbon into the Home tab, as its first group.
- **Ribbon labels on two lines**: labels of large ribbon buttons break onto two lines as in
  Office, with the arrow of a menu after the second line.

## 0.15.2 – 2026-11-01

- **See-through fragment tabs**: the tab of a loop, alt and the like no longer hides the
  lifelines and activation bars below it; the fragment stays on top, with nothing filled.
- **Found and lost messages**: one click on the lifeline draws them, no second click any more.
  Dragging the dot at their open end makes them longer or shorter and turns them to the other
  side; they keep their length when the lifeline moves. A lost message starts as a reply,
  leaving the bar at its end.
- **Back from a member**: with an attribute or operation selected, a click on the class name
  (or elsewhere in the box) selects the class again.
- **Sample project**: all calls in "Rent a bike" are synchronous.

## 0.15.1 – 2026-11-01

- **Constructor bars**: a create message gives the new lifeline an activation bar right below
  its head, where the constructor runs.
- **About UmlCraft**: the i in the title bar (and File › About UmlCraft) shows the version, the
  web site, the source code and the licence.
- **Sample project**: no multiplicities at the ends that are not navigable; in "Rent a bike"
  the app's `scan` lasts until the end of the ride, so the tracking runs inside it.
- **Fragments on top**: combined fragments (loop, alt, …) are drawn over lifelines and
  activation bars, and their filled tab keeps the operator and condition readable.
- **Callers wait**: the bar a synchronous call is sent from lasts at least until the bar the
  call started has ended, whether a reply is drawn or not.
- **Handles by click**: a click on the arrow handle without dragging waits for a click on the
  element to connect to, as the tools do; before, on a lifeline it always offered messages to
  itself.
- **Replies keep to their bar**: a reply moved below the bar it returns from takes the end of
  that bar along, so it never starts outside an activation; moved back up, it shortens the
  bar again.

## 0.15.0 – 2026-11-01

- **Handles beside the element under the mouse**: classes, interfaces, enumerations and data
  types show buttons that open an empty row for a new attribute, operation or literal. Classes,
  interfaces, enumerations, data types, actors, use cases and objects have an arrow handle:
  dragged onto another element it draws a line, and a menu offers the kinds UML allows between
  the two (a single one is drawn at once). On sequence diagrams, lifelines and activation bars
  have an arrow handle at the height of the mouse that draws a message to another lifeline;
  the menu offers call, asynchronous message, reply, create and destroy.
- **Sorted model tree**: packages and models list packages first, then diagrams and
  structograms, interactions, activities and state machines, then the other elements by kind
  and name, relationships last. Members and the parts of an interaction keep their order.
- **Diagrams at the top get a package**: a new diagram at the top of the model comes in a
  package of the same name, so what is drawn on it has a place of its own.
- **Object diagrams**: New diagram › Object diagram makes a class diagram whose ribbon starts
  with Object and Link; its tab and model tree entry say Object diagram.
- **Structogram keywords in German or English**: the settings choose the keywords of new
  structograms, by default as the user interface. German ones start loops as `solange ?`,
  `bis ?` and `für ? := ? bis ?` and put `solange`, `für`, `bis`, `Schritt`, `für jedes` and
  `gib` into the header, so Structorizer reads them. Methods imported from source code follow
  the setting: `für i := 0 bis n - 1` and `gib x`, or `for i := 0 to n - 1` and `return x`
  (before, `gib x` in every language). New elements of a structogram take the keywords it is
  written in.

## 0.14.0 – 2026-11-01

- **New members without the properties panel**: a double click below the members of a
  compartment opens an empty row; what is typed there becomes an attribute, operation or literal,
  and Enter opens the next row. The class's context menu adds them too.
- **Member types**: a right click on a member selects it and offers a Type submenu and an entry
  that deletes just that member. The submenu lists the types the model already writes as text,
  most used first (`string` and `bool` in a C# model), filled up with Java's basic types, then
  the model's classes, interfaces, enumerations and data types. The properties panel shows an
  attribute's name, type, multiplicity and initial value as fields of their own, an operation's
  name and return type.
- **Association ends in place**: a double click on a role name or multiplicity edits it on the
  diagram; near the end of the line it adds a role name that is not there yet. A role is typed
  as it reads, `# buyer {ordered}`, with visibility and constraints.
- **Change kind**: a relationship's context menu turns an association, generalization,
  realization or dependency into another of these. The line's source becomes the specific end
  or the client; kinds UML does not allow between the two elements are greyed out. Name,
  stereotypes and documentation stay, and so do the line's route and labels.
- **Longer lifelines**: a selected lifeline shows a grip at its end that drags it longer, never
  shorter than its messages.
- **Extension points from an extend**: the extend's context menu adds an extension point to the
  extended use case and lets the extend use it.
- **New names selected**: when an inline editor opens for something just made, its default name
  is selected, so typing replaces it.
- **Diamonds and navigability**: an aggregation or composition end that is not navigable shows
  its × right behind the diamond, a navigable one its arrow.
- **`{readOnly}`**: in the settings, read-only attributes can end in `{readOnly}`.

## 0.13.0 – 2026-11-01

- **Labels out of the way**: names, stereotypes, guards, role names and multiplicities of lines
  no longer cover nodes, lines or each other where there is room nearby. A label stays where it
  always went while that place is free; otherwise it moves to the other side of its line,
  further along it or just beside it. Labels placed by hand stay where they are.
- **Window title**: the title bar shows the version.
- **Diagram to the clipboard**: Edit › Copy diagram as image (Ctrl+Shift+C, or Tools › To
  clipboard) copies the open diagram or structogram as SVG and PNG at once. Word and PowerPoint
  paste it as a vector image that stays sharp at any size; other programs take the PNG.
- **Sequence diagrams**: calls one after the other from the same lifeline share its bar. A call
  drawn just below the sender's bar continues that bar instead of starting a second one, and a
  further call to the same receiver starts 20 below the bar the last one started there. A
  message dragged off a bar no longer leaves it longer than what is still in it, and dragged
  back below it continues it.
- **alt with else**: a combined fragment switched to `alt` gets its second operand, `[else]`.
- **Numbering**: the Sequence tab switches message numbers on and off and between 1, 2, 3 and
  2.1; the diagram's properties have the second switch too. The found and lost message tools
  are named in full and say on hover how they are used.
- **Abstract classes** say so in words besides the italic name: `{abstract}` below it, or, in
  the settings, «abstract» above it.
- **Associations**: the context menu lists the end nearer the click first and marks it, so a
  change of aggregation, navigability or multiplicity lands on the side clicked.
- **Use case diagrams**: new lines are straight; Route lines still routes them. A new system
  boundary is named "System". Lines from below an actor end under its name instead of crossing
  it.
- **Visibility** is offered for packages, classes, interfaces, enumerations, data types and
  components only, no longer for objects, actors, use cases, states and lines.
- **Structograms**: Shift+Enter and Ctrl+Enter confirm the element dialog, Enter still breaks
  the line. New conditions start as `?` instead of Structorizer's `(?)` (`while ?`,
  `until ?`). The export button is called "Save for Structorizer" and explains itself.
- **Sample project**: a station holds its docks in a plain composition, `docks` with `*`; the
  qualifier `number` is gone from the domain model. UmlCraft still draws and edits qualifiers.
  Classes have constructors, `EBike.charge` lost its `[1]`, every end that cannot be navigated
  shows its ×, and dock and vehicle know each other, with arrows at both ends.

## 0.12.1 – 2026-10-31

0.12.0 was tagged but never released; 0.12.1 is the first release with these changes.

- **AI assistants (MCP)**: the new `umlcraft-cli mcp` is an MCP server with which AI clients
  such as Claude Code, Claude Desktop or Cursor read and draw UML models: diagrams, elements,
  lines and members, moving, arranging and deleting, rendering a diagram as an image, opening,
  saving and exporting. While UmlCraft runs they work in its open project, every change one
  undo step ("AI: …"); otherwise in a project of their own. `umlcraft-cli mcp setup` registers
  the server with the installed clients. Files only in allowed folders (Documents unless
  `--allow-path` names others). `umlcraft-cli` comes with the Windows, `.deb`, `.rpm`, tarball
  and macOS packages.

## 0.11.2 – 2026-10-31

- **Automatic layout**: new lines are routed around the nodes at right angles and are routed
  again when their nodes move or something is moved into their way, until their route is
  edited by hand. Home › Layout › Route lines (Ctrl+Shift+R) does that for the selected lines,
  or all of the diagram. Arrange automatically (Ctrl+Shift+L) lays out class, component and
  deployment diagrams in layers, top to bottom or left to right: generals above their
  specifics, wholes above their parts, few crossings, containers around what they hold, parts
  nothing connects packed into rows. With a selection only that is arranged, in its place, and
  the nodes in its way move aside.
- **Sequence diagrams**: a message, reply included, can be drawn from and to an activation
  bar; it goes between the bars' lifelines. Before, only the lifeline itself took it. A reply
  now always returns into the bar its call was sent from, which grows to take it; before, it
  could arrive below the caller's bar.
- **Sample project**: the domain model uses Java's types (`int`, `boolean`, `double`,
  `BigDecimal`, `LocalDateTime`) and private references; in the sequence diagram the service
  handles the rental in one activation, numbered 2.1 to 2.6.

## 0.11.1 – 2026-10-31

- **Sample project** (File › Open sample project) is back on Windows and Linux, where the menu
  item had gone with the platform menu bar. It is now Velo, a city's bike sharing, with fuller
  diagrams of every kind: a domain model with a qualified composition, derived and static
  attributes; an object and a package diagram; use cases with a system boundary, include,
  extend and extension points; a sequence diagram with alt and loop, a create message and
  replies; an activity with partitions, fork, join, decision, an object node and a flow final;
  a state machine with an orthogonal state, a deep history and a choice; components with
  provided and required interfaces; a deployment with nested nodes and artifacts; and
  structograms of three operations.
- **Python import** of the whole standard library works: a class imported under its module's
  name (`from socket import socket`), a package that re-exports a name of one of its modules
  under the module's name (`from .repair import repair`), and a module written as a type no
  longer stop the import.

## 0.11.0 – 2026-10-30

- **Windows installer**: `UmlCraft-*-windows-x64-setup.exe` installs UmlCraft for the current
  user without administrator rights, or for all users, and makes it the program for `.ucraft`
  files; `.asta` and `.nsd` files get UmlCraft under "Open with". The `.zip` stays for use without
  installing.
- The mouse wheel over the ribbon switches its tabs.
- **Structograms**: Nassi-Shneiderman diagrams in the style of
  [Structorizer](https://structorizer.fisch.lu) are part of the project, saved in the
  `.ucraft` file next to the UML diagrams and shown in the main window like a diagram (New
  diagram › Structogram, or New structogram in the model tree's menu). They are drawn as
  Structorizer draws them with syntax highlighting off, with zoom, panning and export as SVG
  or PNG. The ribbon's Structogram tab inserts elements of every kind after the selection;
  elements are edited in a dialog (double click or Enter), cut, copied, pasted, dragged, moved,
  dyed with Structorizer's paint box, disabled and deleted; case and parallel elements get and
  lose branches. Undo and redo cover structograms and diagrams alike. File › Import ›
  Structogram (.nsd) … adds Structorizer files to the project, Export › Structogram as .nsd …
  writes one that Structorizer opens; parser keywords changed in Structorizer go along. Opening
  a `.nsd` file (menu, command line, system, recent files) starts a new project with that
  structogram. Files with structograms need this version of UmlCraft or a later one.
- **Structograms from Java, C#, Python and C++**: the source import (Tools › Import source
  code) lists the methods of the files read (grouped by file and class, with search, none
  ticked); each ticked method gets a structogram under its class, linked to its operation, in
  the same undo step as the classes. Importing again renews these structograms, deletes those
  no longer ticked or whose method is gone, and keeps those edited by hand. A file the class
  import cannot read still gives its methods. The structograms are without types, with
  assignments as `x := 42 + a`, conditions as questions
  (`a > 0?`), returns as `gib x` and counting loops as `for i := 0 to n - 1`: statements, calls
  of the class's own methods, if chains, every loop, `switch` in all its forms (arrow cases and
  switch expressions included), jumps and `try` with its catches; in C# also accessors,
  operators, expression bodies, switch expressions with patterns, `using`, `lock` and the
  top-level statements; in Python the functions of the module, `elif`, `for … in range(…)` as a
  counting loop, `match`, `with`, and the code of the module; in C++ free functions, methods
  defined outside the class, initializer lists, range-for and `catch (...)`. Comments in the
  method go to the element they belong to. What a structogram cannot show as written
  (`continue`, labels, local classes …) is listed below the structogram.

## 0.10.0 – 2026-10-30

- **New window layout**: the window draws its own title bar with the File menu, Save, Undo and
  Redo; below it a ribbon with the tabs Home, the open diagram's kind, View and Tools replaces
  the menu bar, the toolbar and the palette row. The diagram tab shows the tools of the open
  diagram with icons and opens by itself when another diagram becomes active. Model tree,
  diagram and properties are cards on a grey backdrop; zoom sits in a pill over the diagram.
  The diagram tabs are gone: the canvas shows the diagram chosen in the model tree. A card
  left of the ribbon creates new diagrams. In a narrow window the ribbon's buttons drop their
  labels; a double click on a tab folds the ribbon to its tabs. The model tree shows an icon
  per element, the properties panel names what it shows. On macOS the system menu bar stays
  as it was.
- The interface no longer names Astah: menus, messages and the ribbon speak of `.asta`
  projects, and the ribbon's group for import and export is called Exchange.

## 0.9.0 – 2026-10-29

- **Settings dialog** (Tools › Settings …, Ctrl+,): language, theme and the visibility column
  in one place. Changes apply at once. The language can also follow the system again, and the
  visibility column no longer needs editing `settings.json`. In the toolbar, a Settings button
  takes the place of the Light/Dark and language buttons; View keeps both toggles.
- Right-angled lines whose ends nearly line up run straight instead of making a tiny step, and
  the step of a right-angled line between two boxes lies halfway between them.
- Projects and `.asta` files open in the background, and an export to Astah can be cancelled;
  it stops on its own after ten minutes and no longer leaves a half-written file behind.
- Messages from the model, the file format and the Astah import and export are shown in the
  chosen language. The Astah import says which elements and shapes it could not carry over.
- Undo and Redo name their step in the tooltip. On macOS, shortcuts use Cmd.
- The tool palette of a diagram wraps onto a second line instead of drawing over the
  properties panel.
- Diagram texts fall back to Liberation Sans or Arimo where Arial is missing; without any of
  them, the status bar says so.
- Sequence diagrams: moving a message or lifeline keeps activation bars in place, and frames
  span the lifelines they cover.
- Faster editing of large models: names, generalizations and shapes are indexed, and deletes
  no longer scan the whole model.
- Source import: C++ and C# preprocessing, C++ operators and templates, imports per
  definition and Java names are handled more faithfully.
- On macOS and Linux, `.ucraft` and `.asta` files open in UmlCraft from the file manager.
- Recent files that no longer open drop out of the list.
- The file format is version 3: projects saved by 0.9.0 cannot be opened by UmlCraft 0.8.0
  and older.

## 0.8.0 – 2026-10-28

- **Members look as in Astah**: the visibility stands right before the name again (`- name`).
  UmlCraft's own column, which lines up all names of a compartment, is now a setting
  (`visibilityColumn` in `settings.json`), off by default.
- **Closer to Astah**: classes at 14 px are as wide as in Astah, and a class whose members no
  longer fit its size grows as Astah's does. A call without arguments shows the parameters
  of the operation it calls (`check(item:Item)`), with their types and the return type as
  chosen in Astah; these choices go back to Astah too. Message labels sit where Astah puts
  them, and long ones of found messages break where Astah breaks them.
- **Bound classes**: a class such as `ArrayList<String>` that binds a template, with its
  «bind» line to the template and what it substitutes (`<E->String>`), is drawn as in Astah
  and goes to and from Astah as a binding. In the editor, the class's "Binds" field takes the
  template and its arguments, such as `Pair<int,Holder>`; the «bind» line follows.
- **Subsystems**: a new tool in class and component diagrams and in the model tree's menu. A
  subsystem is a package with Astah's fork in its tab; subsystems from `.asta` files no longer
  turn into plain packages.
- **Component and deployment diagrams look like Astah's**: component and artifact names in
  regular type, Astah's icons and text positions, 2 px lines, node cubes 4 deep. Instances
  show their classifier's stereotypes too.
- Instances and objects can hide their name (`: Client`) or classifier, and objects their
  slots, with switches in the properties panel; `.asta` files keep them too. Hidden slots go
  back to Astah.
- Component and deployment diagrams from `.asta` files keep their frame.
- **`.asta` files look more like they do in Astah**, measured against Astah's own SVG export
  for `Sample.asta` and 59 files from practice:
  - font sizes of single elements;
  - use cases and objects sized by their content grow around their center instead of
    wrapping; auto-sized classes widen when a member does not fit, and fit anew when their
    content or font changed since they were sized, as Astah does when it opens the file;
  - a system boundary's name keeps its place, also above the rectangle;
  - sequence diagrams: flat message numbers (`2:`, `3:`), hidden frames, message labels moved
    by hand, the width of messages to self, «create» over a moved label;
  - inherited slots come first;
  - containment lines with ⊕ at the owner of a nested class;
  - Java enums from Astah's reverse engineering, with their «enum constant»s; stereotypes of
    attributes and operations show in front of their name;
  - activity, state machine and component diagrams keep their frame, tabbed `act`, `stm` and
    `cmp`;
  - states show their name above a line, with 2 px outlines and Astah's corners; a guard
    stands on a line below its trigger;
  - a node's cube sticks out above and right of its bounds, as in Astah;
  - «reside», «implement» and other stereotypes of unnamed lines stay where Astah drew them;
  - bound classes that a dependency, generalization or association points to are kept, so
    associations to types such as `List<User>` are no longer left out;
  - a self-association keeps its ends where Astah has them;
  - long attributes and operations break where Astah breaks them;
  - combined fragments show their name in the tab (`loop : name`); «create» without a text
    stands where the label goes;
  - activation bars that Astah keeps with width 0 are drawn as a line;
  - lifelines can hide their classifier or name, as in Astah (a switch in the properties
    panel). Hidden ones are reported by the export to Astah.
- **Free texts and lines**: texts and lines drawn on a diagram without a model element, such
  as a guard written next to a fragment or a reply drawn by hand. The new Text and Line tools
  on every diagram create them; a double-click edits a text in place, the properties panel
  makes a line dashed or gives it arrowheads, and a line's ends can be dragged one by one.
  They copy and paste with the rest of a selection. They come along from `.asta` files, look as in
  Astah, move and delete like other items and go back to Astah. Projects with free texts or
  lines cannot be opened by UmlCraft 0.7.0 and older.
- Export to Astah no longer fails on sequence diagrams with a found or lost message whose
  label was moved.
- Tagged values of languages carry UmlCraft's own names: `java.`, `csharp.` and `cpp.`
  instead of Astah's `jude.java.` … The Astah import and older project files are renamed on
  the way in, the export names them back; Java annotations, which Astah does not take under
  its own name, go there under UmlCraft's and come back unchanged.
- Export to Astah no longer stumbles over guards moved above their operand; containment
  lines are reported as left out.
- Notes wrap long lines at their width instead of growing wider.
- Texts on the canvas sit a fraction of a pixel closer to where Astah puts them.

## 0.7.0 – 2026-10-26

- **New app icon**: an initial state with its transition into a state, as in a state machine
  diagram. `umlcraft.exe` now carries it in Explorer, the taskbar and its windows, and the
  macOS app shows it in the Dock and Finder.
- SVG and PNG images fill elements white, as the diagram shows them on screen, instead of
  Astah's yellow.
- Texts in SVG images keep even letter spacing in PDFs made with Word.
- Lines to a use case end at its ellipse, not at the rectangle around it.
- «include» and «extend» stand beside their line instead of on it, and can be dragged to
  another place on the diagram. A note anchored to the line keeps them on the other side.
- Use case names too wide for their ellipse wrap at spaces; the ellipse grows in height to
  hold them.
- The model tree and lists in the properties panel show names with line breaks on one line.
- «include» and «extend» from `.asta` files stay where Astah drew them, and names, roles and
  multiplicities of lines to use cases no longer shift on import.
- **System boundaries as in Astah**: a thin rectangle without fill, the name in regular type.
  Astah has no system boundary of its own; the rectangle with a text at its top that Astah
  users draw instead is imported as a boundary, and boundaries are exported that way.

## 0.6.0 – 2026-10-26

- **Linux packages:** besides the `.tar.gz`, each release has a `.deb` (Debian, Ubuntu), an
  `.rpm` (Fedora, openSUSE), an AppImage, a Flatpak and a Snap. They add UmlCraft with its icon
  to the application menu and open `.ucraft` files on double-click; the `.deb` and `.rpm`
  install the libraries UmlCraft needs. The Flatpak and the Snap cannot run the `.asta` export.
- **Use case diagrams redrawn**: bold names and 2 px ellipses; extension points under a line,
  left-aligned; use cases sized to their name and points; actors as a stick figure that keeps
  its size in larger boxes. The frame of a use case diagram reads `uc` and the diagram's name,
  and frames of use case diagrams in `.asta` files come along on import.
- **Notes on lines**: the note anchor connects a note to an association, include, extend or
  any other relationship, ending in the middle of its line. Imported from and exported to
  `.asta` files.
- Extension points can be added from a use case's context menu on the diagram.
- An extend's condition is no longer drawn at its line.
- Fixed in the `.asta` export: public attributes became private, and role names and
  multiplicities placed by hand went to the wrong end of an association whose line runs from
  its second end.

## 0.5.0 – 2026-10-25

- **Python import** (Tools › Python › Import Python files … / Import Python folder …): reads
  `.py` and `.pyi` files as modules in their packages: classes (`Protocol` as interfaces,
  `Enum` as enumerations), annotated attributes and those `__init__` assigns to `self`,
  methods with their decorators, docstrings and template parameters. Attributes without an
  annotation get no type; typed containers and `X | None` make associations. Names resolve
  through imports and what a package's `__init__.py` imports.
- **C++ import** (Tools › C++ › Import C++ files … / Import C++ folder …): reads declarations
  from headers and sources: namespaces, classes, structs, unions and enumerations with fields,
  member functions, constructors, operators, template parameters, base classes and `///`,
  `//!` and `/** */` documentation. Pure virtual functions are abstract, `const` ones queries.
  Members defined outside their class join it, typedefs and `using` aliases stand for their
  targets, and standard containers and smart pointers make associations. Of `#if`/`#ifdef` …
  `#else`, only the first branch is read; files that use macros in declarations are reported
  and left out.
- **C# import** (Tools › C# › Import C# files … / Import C# folder …): reads declarations from
  `.cs` sources into the open project as the Java import does: namespaces, classes, interfaces,
  structs, enumerations and records with fields, methods, constructors, operators, template
  parameters, base classes and interfaces, and `///` documentation. Auto-properties become
  attributes, other properties and indexers accessor operations (`get_Count`, `set_Count`);
  partial classes become one class. Library types become placeholders at their namespace path,
  fields typed by a model type become associations (`Order?` with `0..1`). Of `#if` … `#else`,
  only the first branch is read.
- **Java import**: importing again no longer depends on the order in which library types were
  first seen when two on-demand imports offer the same name.
- **Object diagrams**: objects `name : Class` on class diagrams with one slot per attribute of
  the class, inherited ones included, edited inline or in the properties panel
  (`name = value`); links between objects with roles, navigability and aggregation, optionally
  as an instance of an association. Imported from and exported to `.asta` files.
- **Orthogonal states**: a state can be divided into regions, one below the other or side by
  side (properties panel); the lines between them can be dragged. **Internal transitions**
  (`trigger [guard] / effect`) are listed in the state below entry, do and exit and edited
  inline or in the properties panel. Both are imported from and exported to `.asta` files; the
  export keeps two regions per state, as the API allows no more.
- States dragged into another state or one of its regions become part of it, and dragged out
  of it, part of the state machine again. New states land in the region they are placed in.
- **Object nodes in a state**: `order : Order [filled]` shows the state in a line of its own
  below the name; type and state are set in the properties panel. Imported from and exported
  to `.asta` files.
- Transition labels, flow guards and `<<include>>`/`<<extend>>` are placed anew: names centered
  below the middle of the line, `<<include>>` and `<<extend>>` right above it.

Projects with slots, links, regions or internal transitions cannot be opened by UmlCraft 0.4.0
and older. Older versions open other projects of this version, but drop the state of object
nodes when they save.

## 0.4.0 – 2026-10-24

- **Java import** (Tools › Java › Import Java files … / Import Java folder …): reads declarations
  from `.java` sources into the open project: packages, classes, interfaces, enumerations,
  records and annotation types with attributes, operations, template parameters, supertypes and
  Javadoc. Library types become placeholders at their package path (`java › util › List`),
  fields typed by a model type become associations. Method bodies are skipped, files with
  syntax errors are reported, and importing again updates the model instead of duplicating it.
  The whole import is one undo step. New crate `umlcraft-reverse`, built to take C# and C++
  later.
- **Class diagrams**: template parameters (type and value parameters with bounds and defaults)
  in a dashed box, bound templates such as `Box<Holder, 20>`, qualifiers in a box at the
  association end, static ends underlined, leaf classes and operations, visibility symbols in a
  column of their own, four line styles for edges (straight or curved, each also right-angled).
  All of it imported from and exported to `.asta` files.
- Deleting and undoing large edits is much faster (undoing an import of 225,000 elements takes a
  moment instead of minutes).
- The toolbar and shortcuts such as Ctrl+Z work right after start and after a menu or dialog
  closed, without clicking into the diagram first.

Projects that contain template parameters or bound types such as `List<Order>` (the Java import
creates them) cannot be opened by UmlCraft 0.3.0 and older. Older versions open other projects
of this version, but drop qualifiers and the new details of association ends when they save.

## 0.3.0 – 2026-10-24

- **State machine diagrams**: states with entry, do and exit, composite states with substates,
  final states, initial, choice, junction, fork, join and history pseudostates, transitions
  labeled `trigger [guard] / effect`. Imported from and exported to `.asta` files.
- **Component and deployment diagrams**: components, artifacts and nodes with their icons,
  objects, component and node instances (`name : Type`), dependencies, realizations and
  associations; nodes take what is deployed on them along. Imported from `.asta` files;
  exported to them as class diagrams, because the API has no editors for them.
- **Sequence diagrams redrawn**: stored and drawn with the geometry of `.asta` files (the "sd"
  frame, lifeline heads and lengths, activation bars, message heights and numbers, fragment
  frames), so imported diagrams keep their layout. Interaction uses ("ref" frames) and found
  and lost messages. Files from earlier versions are laid out once when opened.
- **PNG export** of the open diagram, at twice the canvas size.

## 0.2.0 – 2026-10-23

- **Activity diagrams**: actions, initial, activity final and flow final nodes, decision, merge,
  fork and join, object nodes, flows with guards, partitions (also nested). Imported from and
  exported to `.asta` files, whose UML 1.4 activity graphs are translated.
- **SVG export** of the open diagram.
- The `.asta` export is always offered and explains what is missing when no Astah is installed.

## 0.1.0 – 2026-10-23

The first release: the editor with class, use case and sequence diagrams, and `.asta` import
and export.

- Model core with validated, invertible operations and transactions (undo/redo); JSON project
  format `.ucraft`.
- Editor: menu bar, diagram tabs, tool palette per diagram type with connection rules,
  rubber-band and multiple selection, resizing, alignment, zoom to fit, properties panel, model
  tree with context menu and drag and drop, copy/cut/paste, waypoints, light and dark theme,
  German and English.
- Class diagrams with inline editing (`+ name : Type [0..*] = init`, `+ op(p : T) : R`),
  association ends, self-associations, hidden compartments and interfaces as circles.
- Use case diagrams with system boundary, extension points, include and extend.
- Sequence diagrams with synchronous, asynchronous, reply, create, destroy and self messages,
  activation bars and combined fragments.
- Import of Astah `.asta` files and export to them through an installed Astah.
