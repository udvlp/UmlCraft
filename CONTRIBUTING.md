# Contributing to UmlCraft

UmlCraft is a UML modeling tool in Rust with GPUI that reads and writes Astah's `.asta` files.
How it is built: [`docs/architecture.md`](docs/architecture.md).

## Commands

- Build and start: `cargo run -p umlcraft-app`
- Tests: `cargo test --workspace`
- Lints: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`
- Layers: `cargo xtask layers` checks that crates depend only downwards and that only the app
  knows GPUI (also part of the tests); a new crate goes into its table, see
  [Crates and layers](docs/architecture.md#crates-and-layers)

CI runs all three on Windows, Ubuntu 22.04 and macOS, plus `cargo deny check` (advisories,
licenses and sources, `deny.toml`). On Debian and Ubuntu,
`packaging/linux/install-build-deps.sh` installs GPUI's system libraries.

## Rules

- Identifiers, code comments, commit messages and all project docs are in English.
- UI texts go only through the files in `locales/` (`t!("…")`), never hard-coded. Every new
  key goes into `en.yml` and `de.yml`; the other languages may lag behind and show the
  English text until someone translates it. A test fails when `de.yml` or `en.yml` lacks a
  key of the other, or when any file has a key `en.yml` has not. rust-i18n reads the locales
  at compile time: after changing them, touch `crates/umlcraft-app/src/main.rs`.
- The model changes only through `Document::edit` / `Tx`, never around it. Edits of sequence
  diagrams end with `sequence_edit::tidy` (or `tidy_all`), so that bars, lifelines, fragments,
  frames and the order of the interaction match the drawing.
- GPUI comes in through `gpui_kit::*`. Use `gpui_kit`'s `actions!` macro, not gpui's.
- Astah's `.asta` files and anything else from Astah's installation never go into the
  repository.
- LF line endings, no BOM. Code that touches paths, file names or line endings must not assume
  one platform.

## Adding a language

The languages are the files in `locales/`. Copy `locales/en.yml` to `locales/<code>.yml` and
translate it. Set `language.name` to the language's own name; the app lists it in the settings
and in the language menu of the View tab. Texts left out show in English. The `structogram:`
section holds the keywords of structograms (`while`, `for`, `return` …). Translate it too, or
leave it out to use the English keywords. A counting loop must keep `<-` or `:=` after its
variable, because Structorizer recognises only those. Run `cargo test --workspace`; it checks
that the new file has no key `en.yml` has not, and the same `%{…}` placeholders in each text
it has.

## Tests

- Snapshot tests of the source import: `UPDATE_SNAPSHOTS=1 cargo test …` accepts changed
  output; review the diff before committing it.
- The golden project file (`crates/umlcraft-format/tests/order-system.ucraft`):
  `UPDATE_GOLDEN=1` rewrites it. A change there means older files read differently, so it
  usually needs a new format version and a migration.
- Tests that read Astah's sample projects or export through Astah's API find Astah through
  `ASTAH_HOME`, else in `C:\Program Files\astah-professional`, and skip themselves without it.
  Set `UMLCRAFT_REQUIRE_ASTAH=1` on a machine with Astah to turn a skip into a failure.
