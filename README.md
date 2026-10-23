# UmlCraft

A UML modeling tool written in Rust with [GPUI](https://gpui.rs), built from scratch after the
model of Astah Professional. Milestone 1 covers class, use case and sequence diagrams, plus
importing `.asta` files (read only).

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
  (also into and out of fragments); labels and guards are edited in place.
- **M6** `.asta` import (`umlcraft-asta`, File › Import Astah project, or simply open an `.asta`
  file): packages, classes, interfaces, enumerations, actors and use cases with attributes,
  operations, parameters and extension points; associations, generalizations, dependencies,
  realizations, include and extend; class and use case diagrams with positions, sizes, waypoints,
  notes and colors; sequence diagrams with lifelines, messages in their order and combined
  fragments with operands and guards. Anything UmlCraft cannot represent yet is skipped and
  reported.

This completes milestone 1. Next: further diagram kinds (activity, state) and image export.

## Build and run

```bash
cargo run -p umlcraft-app
```

```bash
cargo test --workspace
```

## Structure

| Crate | Purpose |
| --- | --- |
| `crates/umlcraft-model` | Metamodel, diagrams/presentations, `Op`, `Document`/`Tx` (undo/redo) |
| `crates/umlcraft-format` | `.ucraft` files: JSON, stably sorted, versioned |
| `crates/umlcraft-notation` | UML notation without UI: scene, hit testing, palettes, connection rules |
| `crates/umlcraft-asta` | Read-only import of Astah `.asta` files |
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
