# Changelog

What changed from release to release. The [README](README.md) describes what UmlCraft can do
today.

## 0.4.0 – 2026-10-24

- **Java import** (Tools › Java › Import Java files … / Import Java folder …): reads declarations
  from `.java` sources into the open project as Astah's own import does: packages, classes,
  interfaces, enumerations, records and annotation types with attributes, operations, template
  parameters, supertypes and Javadoc. Library types become placeholders at their package path
  (`java › util › List`), fields typed by a model type become associations. Method bodies are
  skipped, files with syntax errors are reported, and importing again updates the model instead
  of duplicating it. The whole import is one undo step. New crate `umlcraft-reverse`, built to
  take C# and C++ later.
- **Class diagrams as in Astah**: template parameters (type and value parameters with bounds and
  defaults) in a dashed box, bound templates such as `Box<Holder, 20>`, qualifiers in a box at
  the association end, static ends underlined, leaf classes and operations, visibility symbols
  in a column of their own, Astah's four line styles for edges. All of it imported from and
  exported to Astah.
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
  labeled `trigger [guard] / effect`. Imported from and exported to Astah.
- **Component and deployment diagrams**: components, artifacts and nodes with their icons,
  objects, component and node instances (`name : Type`), dependencies, realizations and
  associations; nodes take what is deployed on them along. Imported from Astah; exported as
  class diagrams, because Astah's API has no editors for them.
- **Sequence diagrams as in Astah**: stored and drawn with Astah's geometry and measures (the
  "sd" frame, lifeline heads and lengths, activation bars, message heights and numbers, fragment
  frames); imported diagrams and their SVG exports match Astah's practically pixel for pixel.
  Interaction uses ("ref" frames) and found and lost messages. Files from earlier versions are
  laid out once when opened.
- **PNG export** of the open diagram, at twice the canvas size.

## 0.2.0 – 2026-10-23

- **Activity diagrams**: actions, initial, activity final and flow final nodes, decision, merge,
  fork and join, object nodes, flows with guards, partitions (also nested). Imported from and
  exported to Astah, whose UML 1.4 activity graphs are translated.
- **SVG export** of the open diagram, drawn the way Astah exports it.
- The Astah export is always offered and explains what is missing when Astah is not installed.

## 0.1.0 – 2026-10-23

The first release: the editor with class, use case and sequence diagrams, and the way to and
from Astah.

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
