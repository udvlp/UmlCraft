# UmlCraft

A UML modeling tool written in Rust with [GPUI](https://gpui.rs), built from scratch after the
model of Astah Professional. Milestone 1 covers class, use case and sequence diagrams, plus
importing `.asta` files and exporting them through an installed Astah.

## Status

- **M0** Workspace and GPUI spike: model tree, canvas with pan/zoom, selection, dragging with grid
  snapping, undo/redo, light/dark theme, German/English.
- **M1** Model core (`umlcraft-model`) with validated, invertible operations and transactions;
  JSON project format (`umlcraft-format`).
- **M2** Editor shell: menu bar, diagram tabs, tool palette per diagram type with connection rules,
  rubber-band and multiple selection, resizing, alignment, zoom to fit, editable properties panel,
  context menu in the model tree, New/Open/Save/Recent files with a prompt for unsaved changes;
  copy/cut/paste (as a copy or as the same elements), waypoints on edges, drag and drop within the
  model tree and from the tree onto the canvas.
- **M3** Class diagram: classes grow with their content; inline editing by double-click with a
  parser for `+ name : Type [0..*] = init` and `+ op(p : T) : R`; attributes, operations and
  enumeration literals with static/abstract/query/read-only; association ends with role,
  multiplicity, navigability and aggregation/composition; self-associations; hiding compartments;
  interface as a circle ("lollipop").
- **M4** Use case diagram: system boundary (clickable only on its border and title, moves the
  contained elements along, as packages do); use cases with wrapped names and extension points;
  extend with condition and choice of extension points.
- **M5** Sequence diagram: messages run from top to bottom in the order of the interaction, so
  inserting or moving one moves the rest along; synchronous, asynchronous, reply, create and
  destroy messages, self messages, activation bars derived from calls and replies; combined
  fragments (alt, opt, loop, par, …) with operands, guards and covered lifelines, placed with the
  palette or around selected messages (Ctrl+G); messages and fragments are dragged up and down
  (also into and out of fragments); labels and guards are edited in place. Interaction uses
  ("ref" frames referring to another interaction, with arguments) and found and lost messages
  were added later.
- **M6** `.asta` import (`umlcraft-asta`, File › Import Astah project, or simply open an `.asta`
  file): packages, classes, interfaces, enumerations, actors and use cases with attributes,
  operations, parameters and extension points; associations, generalizations, dependencies,
  realizations, include and extend; class and use case diagrams with positions, sizes, waypoints,
  notes and colors; sequence diagrams with lifelines, messages in their order and combined
  fragments with operands and guards. Anything UmlCraft cannot represent yet is skipped and
  reported.
- **Astah export** (`umlcraft-asta-export`, File › Export › Astah project …, needs an installed
  Astah; without one the menu item explains what is missing): writes `.asta` files through
  Astah's official API instead of its fragile file format. A small Java tool
  (`tools/asta-export`) runs on the JRE that comes with Astah and rebuilds packages,
  classifiers, features, relationships, notes and the class, use case, sequence and activity
  diagrams with positions, sizes, waypoints, message order and combined fragments. What the API
  cannot express is reported. In a test with Astah Professional 12 this worked without a license
  file; see [docs/architecture.md](docs/architecture.md#astah-export).
- **Image export** (File › Export › Diagram as SVG … / Diagram as PNG …): the open diagram
  drawn like the canvas in black on white. SVG keeps real text, so it stays sharp and
  searchable; PNG is the same image rasterized at twice the canvas size.

This completes milestone 1.

- **M7** Activity diagrams: actions, initial, activity final and flow final nodes, decision,
  merge, fork and join, object nodes, flows with guards and partitions (swimlanes, nested as
  well). New activity diagrams bring their own activity. Imported from and exported to Astah,
  whose UML 1.4 activity graphs are translated; data flow diagrams and flowcharts stay skipped.
- **M8** State machine diagrams: states with entry, do and exit, composite states with
  substates, final states, initial, choice, junction, fork, join and history pseudostates, and
  transitions labeled `trigger [guard] / effect`. Imported from and exported to Astah;
  orthogonal regions and internal transitions are not supported yet.
- **M9** Component and deployment diagrams: components, artifacts and nodes with their icons
  (nodes as cubes), objects, component and node instances (`name : Type`), dependencies,
  realizations and associations; nodes take what is deployed on them along. Imported from
  Astah; Astah's API has no editors for these diagrams, so the export writes them as class
  diagrams and every instance as an object.

Next: composite structure diagrams (ports, parts, connectors), orthogonal regions, moving
states into other states.

## Download

Releases for Windows (x64), macOS (universal) and Linux (x64) are on the
[releases page](https://github.com/udvlp/UmlCraft/releases). The builds are not signed yet; the
release notes explain how to start them anyway.

## Build and run

```bash
cargo run -p umlcraft-app
```

```bash
cargo test --workspace
```

On Debian/Ubuntu, GPUI needs some system libraries first:
`bash packaging/linux/install-build-deps.sh`.

## Releasing

1. Set the version in `Cargo.toml` (`workspace.package.version`) and commit.
2. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
3. The [release workflow](.github/workflows/release.yml) checks that the tag matches the version,
   builds the Windows `.zip`, Linux `.tar.gz` and macOS `.dmg`, adds the third-party license
   notices (`cargo about`, configured in `about.toml`) and creates a draft release.
4. Review the draft on GitHub and publish it.

Running the workflow by hand (Actions › Release › Run workflow) only builds the packages and
keeps them as workflow artifacts.

## Structure

| Crate | Purpose |
| --- | --- |
| `crates/umlcraft-model` | Metamodel, diagrams/presentations, `Op`, `Document`/`Tx` (undo/redo) |
| `crates/umlcraft-format` | `.ucraft` files: JSON, stably sorted, versioned |
| `crates/umlcraft-notation` | UML notation without UI: scene, hit testing, palettes, connection rules |
| `crates/umlcraft-asta` | Read-only import of Astah `.asta` files |
| `crates/umlcraft-asta-export` | Export to `.asta` through an installed Astah and its API |
| `tools/asta-export` | The Java tool that drives the Astah API |
| `crates/umlcraft-app` | GPUI application (`gpui-kit` 0.8) |

Details: [docs/architecture.md](docs/architecture.md).

## License

UmlCraft is licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
UmlCraft by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

UmlCraft is an independent reimplementation and is not affiliated with Change Vision, Inc., the
maker of Astah. "Astah" is a trademark of Change Vision, Inc.
