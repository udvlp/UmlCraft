# Changelog

What changed from release to release. The [README](README.md) describes what UmlCraft can do
today.

## Unreleased

- **Messages stay on their lifelines**: a message drawn or dragged above the heads goes right
  below them, together with the bar it is sent from, and never below a lifeline's cross; a
  destroy message goes below what happens on its lifeline, also when dragged up, and a create
  message stays above it. A create message takes the head and the constructor's bar along, also
  when it moves with the call it is sent from, and a lifeline is created only once. No bar
  reaches past a lifeline's cross, not even the one of a caller waiting for the object that
  destroys it.
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
