# UmlCraft

A UML modeling tool written in Rust with [GPUI](https://gpui.rs), built from scratch after the
model of Astah Professional. It draws diagrams the way Astah does, opens and writes Astah
projects, and imports Java source code.

![UmlCraft with the sample project's state machine diagram](assets/screenshot.png)

## Download

Releases for Windows (x64), macOS (universal) and Linux (x64) are on the
[releases page](https://github.com/udvlp/UmlCraft/releases). The builds are not signed yet; the
release notes explain how to start them anyway. What changed between releases is in the
[changelog](CHANGELOG.md).

## Diagrams

| Diagram | What it covers |
| --- | --- |
| Class | Classes, interfaces (also as a circle), enumerations and packages; attributes, operations and template parameters with bounds and defaults; associations with roles, multiplicities, navigability, aggregation, qualifiers and static ends; generalization, realization, dependency; bound templates such as `Box<Holder, 20>` |
| Object | Objects `name : Class` with a slot for every attribute of the class, inherited ones included; links with roles, navigability and aggregation, as instances of associations. As in Astah, they are drawn on class diagrams |
| Use case | Actors, use cases with extension points, system boundary, include and extend with condition |
| Sequence | Lifelines, synchronous, asynchronous, reply, create, destroy, self, found and lost messages, activation bars, message numbers, combined fragments (alt, opt, loop, par, …) with operands and guards, interaction uses |
| Activity | Actions, initial and final nodes, decision, merge, fork and join, object nodes, flows with guards, partitions (also nested) |
| State machine | States with entry, do and exit, composite states, final states, initial, choice, junction, fork, join and history pseudostates, transitions labeled `trigger [guard] / effect` |
| Component and deployment | Components, artifacts and nodes with their icons, objects and instances, deployments |

Diagrams are laid out and drawn with Astah's measures, so an imported Astah diagram looks the
same in UmlCraft; for sequence diagrams the SVG exports match Astah's practically pixel for pixel.

## Working with Astah

- **Open `.asta` files** (File › Import Astah project, or simply open the file): the model with
  all diagram kinds above, positions, sizes, waypoints, line styles, notes and colors. Anything
  UmlCraft cannot represent yet is skipped and reported.
- **Export to Astah** (File › Export › Astah project …): writes `.asta` files through Astah's
  official API, so Astah must be installed; without it the menu item explains what is missing.
  What the API cannot express is reported. See
  [docs/architecture.md](docs/architecture.md#astah-export).
- **Images** (File › Export › Diagram as SVG … / Diagram as PNG …): the open diagram the way
  Astah exports it, in Astah's colors and at its size. SVG keeps real text; PNG is rasterized at
  twice the canvas size.

## Importing source code

Tools › Java › Import Java files … / Import Java folder … reads declarations from `.java` sources
into the open project, as Astah's own import does: packages, classes, interfaces, enumerations,
records and annotation types with attributes, operations, template parameters, supertypes and
Javadoc. Library types become placeholders at their package path (`java › util › List`), and
fields typed by a model type become associations (`*`, `{ordered}`, `{unique}`, a qualifier for
map keys). Method bodies are skipped, files with syntax errors are reported, and importing again
updates the model instead of duplicating it. The whole import is one undo step; a project of
6,300 files takes about ten seconds.

## Editing

Inline editing by double-click with a parser for `+ name : Type [0..*] = init` and
`+ op(p : T) : R`; a properties panel for everything else. Undo and redo for every change, copy,
cut and paste (as a copy or as the same elements), rubber-band and multiple selection, alignment,
waypoints, zoom to fit, drag and drop within the model tree and from the tree onto the canvas.
Light and dark theme, German and English.

## Not yet supported

- Composite structure diagrams (ports, parts, connectors)
- Orthogonal regions and internal transitions in state machines; moving states into other states
- Astah's data flow diagrams and flowcharts (skipped on import)
- Component and deployment diagrams are exported to Astah as class diagrams, because Astah's API
  has no editors for them
- Importing C# and C++ (the source import is built for them, the front-ends are still missing)
- Code generation
- Signed builds

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

1. Set the version in `Cargo.toml` (`workspace.package.version`), add the release to
   [CHANGELOG.md](CHANGELOG.md) and commit.
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
| `crates/umlcraft-reverse` | Source import (Java; C# and C++ planned): tree-sitter front-ends, a language-neutral IR, resolution and merging into the model |
| `tools/jdk-types` | The Java tool that writes the table of JDK types used by the Java import |
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
