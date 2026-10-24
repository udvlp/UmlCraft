# Handoff: source import for C#, C++ and Python

For Steven. Goal: Tools › C# / C++ / Python import declarations from source files into the open
project, the same way Tools › Java does today. The Java import is finished, tested and released
(0.4.0). Everything you need is already in place for more languages: you add front-ends and
rules, not a new mapping.

You work on your own machine with a fresh clone of `github.com/udvlp/UmlCraft`. Besides code,
`README.md` and `CHANGELOG.md`, the clone holds only two docs: `docs/architecture.md` and this
file. The project's `CLAUDE.md` and the other notes in `docs/` are kept local on Ulf's machine
(`.gitignore`). Everything you need from them is in this document.

Read this first, then in this order:

1. "Project rules" and "Development environment" below.
2. `docs/architecture.md`, section "Source import": how the import works.
3. `crates/umlcraft-reverse/src/{lib,lang,ir}.rs` and `src/java/`. The Java front-end is the
   template for yours.

## Project rules

These come from the project's `CLAUDE.md`, which is not in the repository. They are binding.

- UmlCraft rebuilds Astah Professional 11 in Rust with GPUI.
- Commands:
  - build and start: `cargo run -p umlcraft-app`
  - tests: `cargo test --workspace`
  - lints: `cargo clippy --workspace --all-targets -- -D warnings` and
    `cargo fmt --all --check`
- Identifiers, code comments, commit messages and all project docs (README, `docs/`) are in
  English.
- UI texts go only through `locales/de.yml` and `locales/en.yml` (`t!("…")`), never hard-coded.
  Every new key goes into both files.
- The model changes only through `Document::edit` / `Tx`, never around it.
- GPUI comes in through `gpui_kit::*`. Use `gpui_kit`'s `actions!` macro, not gpui's.
- Astah's `.asta` files and anything else from Astah's installation never go into the
  repository.
- Ulf's own rules:
  - Address him as "du" in German.
  - LF line endings only.
  - No `Co-Authored-By` lines and no Claude mention in commits or pull requests.
  - End reports with the result: no closing offers or "shall I …?" questions; state open items
    as facts.

## Development environment (Linux)

Ulf develops on Windows; CI builds and tests on Windows, Ubuntu 22.04 and macOS. Everything
you need for this task works on Linux.

- **Rust:** install through rustup (`sudo dnf install rustup && rustup-init`), stable
  toolchain with `clippy` and `rustfmt`. The workspace uses edition 2024; Ulf is on 1.99. There
  is no `rust-toolchain` file, so CI uses current stable. Don't use Fedora's packaged `rust`;
  it may lag behind.
- **C/C++ compiler:** tree-sitter and its grammars build from C through `cc`
  (`sudo dnf install gcc gcc-c++`).
- **GPUI's system libraries:** `packaging/linux/install-build-deps.sh` is for Debian/Ubuntu
  only. On Fedora, install the equivalents:
  ```bash
  sudo dnf install pkgconf-pkg-config fontconfig-devel freetype-devel vulkan-loader-devel wayland-devel libxcb-devel libX11-devel libxkbcommon-devel libxkbcommon-x11-devel
  ```
  If a build script still reports a missing `pkg-config` package, install its `-devel`
  package. Don't change the Debian script for Fedora.
- **Running the app:** needs a desktop session (Wayland or X11) with a working Vulkan driver
  (`vulkaninfo` from `vulkan-tools` shows whether one is there). It may not start in a VM or a
  container without GPU access. Everything except the manual menu check also works headless.
- **Fonts:** diagrams use Arial, as Astah does on Windows. Without it, the app falls back to
  another font and boxes come out slightly different in size. That doesn't matter for the
  import. Optionally install Liberation Sans (metric-compatible with Arial):
  `sudo dnf install liberation-sans-fonts`.
- **Astah:** not needed. The tests that read Astah's sample files or export through Astah's
  API skip themselves when Astah is missing (they look in Windows paths, or in `ASTAH_HOME` for
  the export). The Java comparison `tests/astah_compare.rs` skips without its reference files.
  Ask Ulf for Astah's C#/C++ tag keys (see "Tags").
- **Java:** only needed if you regenerate the JDK table (`tools/jdk-types/JdkTypes.java`, Java
  21+, `sudo dnf install java-21-openjdk-devel`). Your own table generators may be written in
  any language, as long as they live under `tools/`, run outside the build, and their output
  is committed.
- **Git and GitHub:**
  - Install `git` and `gh` (`sudo dnf install git gh`) and run `gh auth login`.
  - Use your own `user.name`/`user.email`.
  - If you have no write access to `udvlp/UmlCraft`, fork it and open the pull request from
    your fork. Ask Ulf for access only if he prefers that.
- **Line endings:** native LF on Linux. Make sure your editor and generators don't write
  CRLF or a BOM.
- **CI:** CI also runs on Windows and macOS. Code that touches paths, file names or line
  endings must not assume Linux. Look at how `source.rs` handles CRLF and how the tests turn
  `\` into `/`.

## Where things are

| What | Where |
| --- | --- |
| Language list, file extensions, front-end and rules per language, tag prefix | `crates/umlcraft-reverse/src/lib.rs` (`SourceLanguage`) |
| Front-end trait, `Rules`, `Library`, `Container`, `Skip`, syntax errors | `src/lang.rs` |
| Language-neutral IR (namespaces, types, members, definitions, aliases, free members) | `src/ir.rs` |
| Java front-end (tree-sitter walk, Javadoc, rules, JDK table) | `src/java/` |
| Name resolution (lookup order, placeholders) | `src/resolve.rs` |
| Merge into the model, re-import matching, `retype` | `src/merge.rs` |
| Attribute → association | `src/associations.rs` |
| File collection and decoding (BOM, UTF-8, Windows-1252 fallback, CRLF) | `src/source.rs` |
| Tests: IR snapshots, model snapshots, Astah comparison | `tests/ir.rs`, `tests/import.rs`, `tests/astah_compare.rs`, `tests/fixtures/java/<case>/` |
| Real-project smoke test (import, check, undo, redo, re-import) | `cargo run --release -p umlcraft-reverse --example java-import -- <folder>` |
| App: action, menu, background parse, report | `crates/umlcraft-app/src/main.rs` (`ImportSource`), `main_view.rs` (`import_source`, `apply_source`, `menus` → Tools), `workspace.rs` (`import_source`) |
| UI texts | `locales/en.yml`, `locales/de.yml` (`menu.*`, `skipped.*`, `dialog.import_source_*`, `status.*`) |

## Decisions that hold for every language

Ulf made these for the Java import; they apply to C#, C++ and Python as well:

- **Attributes become associations:** an attribute whose type is a type of the model (directly,
  or as the element of a collection or the value of a map) **always** becomes an association.
  Astah shows a chooser for this; UmlCraft has none, and you don't add one.
- **Library types become placeholders at their package path** (`java › util › List`, so for
  example `System › Collections › Generic › List`, `std › vector`, `typing › Optional`). They
  take their kind and type parameter names from the library table.
- **Re-import matches members.** It does not empty and refill them, so IDs, presentations and
  hand-drawn dependencies survive (Astah deviates here).
- **Declarations only:** no method bodies, no diagrams. The result appears in the model tree.

## How to add a language

Repeat this per language, finishing one language completely (including the app menu and docs)
before you start the next. Suggested order: **C#**, then **C++**, then **Python**. C# is closest
to Java. C++ needs the IR parts that the mapping does not use yet. Ulf has already answered the
design questions; see "Ulf's decisions" below.

1. **Dependency:** add the grammar to `[workspace.dependencies]` in the root `Cargo.toml` next
   to `tree-sitter-java`, using a version that fits `tree-sitter = "0.25"`. Add it to
   `crates/umlcraft-reverse/Cargo.toml`. Grammars build from C through `cc`; CI (Windows, macOS,
   Linux) already has compilers. Make it a plain dependency, not an optional Cargo feature. The
   original plan had features in mind, but the app always offers every language, so features
   would only add build combinations nobody uses.
2. **`SourceLanguage`:** add the variant and fill all four matches: `extensions`, `frontend`,
   `rules` and `tag_prefix`. C#: `cs`. C++: `h`, `hh`, `hpp`, `hxx`, `cpp`, `cc`, `cxx`.
   Python: `py`, `pyi`.
3. **Front-end** `src/<lang>/mod.rs` implementing `lang::Frontend`:
   - Parse with tree-sitter.
   - Return `first_error` as `Err`, so the file is reported and the others still import.
   - Walk the tree into `ir::Scope`.
   - Count everything you leave out in `Skipped`.
   - Lower language sugar here, never in the mapping. Examples:
     - default visibilities
     - C# properties → attributes or accessor operations (decision 1)
     - C# `struct`/`record` → `TypeKind::Struct`/Class
     - C++ `struct` default public
     - `sealed`/`final` → `is_leaf`
     - `partial` → `is_partial`
     - `class A;` → `is_forward`
     - base access and `virtual` → `Supertype.access` / `is_virtual`
     - `template<int N>` → `TemplateParam.value_type`
     - `typename...` → `is_variadic`
     - pointer and reference modifiers → `TypeExpr.modifier`
     - doc comments → `documentation`: C# `///` XML docs, Doxygen `/** */` and `///`, Python
       docstrings
4. **Rules** `src/<lang>/rules.rs`: `primitives`, `implicit_imports`, `library`,
   `value_packages`, `split_qualified`. The library is a table like `java/jdk.txt`
   (package, name, kind, type parameters, container flag), with a small `Library` impl like
   `java/jdk.rs`. Containers decide associations:
   - collections → `*` with `{ordered}`/`{unique}`
   - maps → qualifier
   - `Optional` → `0..1`
   - `Owning` (C++ `unique_ptr`) → `1`

   Generate the table with a script under `tools/` that runs outside the build. Commit the
   generated table, not the generator's output directory.
5. **Mapping gaps you will have to close** (in the shared code, generically). Today these IR
   parts exist but are ignored or Java-shaped:
   - `Item::Definition` and `Item::Alias` are ignored in `merge.rs` (`declare_scope`). C++
     out-of-class definitions (`void A::f() {}`) must be matched to the declaration in the
     class body. A definition without a declaration should add the operation. Aliases
     (`typedef`, `using X = …`) must resolve as names.
   - `Supertype.access` and `Supertype.is_virtual` are not stored yet. Store them as tagged
     values with the language prefix (see "Tags" below).
   - `resolve.rs` documents Java's lookup order, and `TypeExpr`'s `Display` joins with `.`.
     C++ uses `::`, and `using namespace` behaves like a wildcard import. Before you change the
     lookup order, check whether a `Rules` field can express the difference. If it cannot, add
     a field to `Rules`. Never add `if language == …` in the mapping.
   - The kind of a placeholder after a supertype: in Java, an unknown type after `implements`
     becomes an interface, after `extends` a class. C#'s base list (`class A : B, IC`) does not
     say which entry is the base class and which are interfaces. For types the sources and the
     library don't know, Ulf's rule for it is decision 2. C++ and Python have
     base classes only. Implement it in `Rules` or the front-end, not as a special case in the
     mapping.
   - `Report`, `Skip` and `skipped.*` locale keys: add a new `Skip` variant (and its key in both
     locale files) for each new kind of omission. Examples: C++ macros and preprocessor
     branches, Python statements outside classes.
6. **Tests** (see "Testing" below).
7. **App:**
   - Add a submenu per language under Tools with the existing `ImportSource` action:
     `menu.csharp` + `menu.import_csharp_files` / `menu.import_csharp_folder`, likewise for C++
     and Python.
   - Keep the two entries per language: files (several at once) and a folder (searched
     recursively). The Windows file dialog cannot offer files and folders together, so don't
     merge them into one entry, even though some Linux dialogs could.
   - Add the keys in **both** locale files, then touch `crates/umlcraft-app/src/main.rs` so
     rust-i18n picks them up.
   - Nothing else in the app should need to change. If it does, keep it generic over
     `SourceLanguage`.
8. **Docs:**
   - `docs/architecture.md`, section "Source import": mention the new front-end and anything
     the mapping learned.
   - README: the "Importing source code" section and the "Not yet supported" list.
   - CHANGELOG: under "Unreleased", user-facing only. Ulf's rule: no error-message details
     there, nor in the README or release notes.

## Tags

Astah's language profiles use these prefixes:

- `jude.java.`
- `jude.c_sharp.`
- `jude.c_plus.`

The Astah import already passes them through (`crates/umlcraft-asta/src/map.rs`,
`is_internal_tag`). Use them for C# and C++, so a project round-trips through Astah.

Astah has code generation for C# and C++, so it has keys for modifiers such as `override`,
`virtual`, `const`, `inline` or `struct`. Ask Ulf for the exact keys before you use them. He
can set the options in Astah on a model element, save the project, and read the keys with
`cargo run -p umlcraft-asta --example asta-dump -- file.asta`. Don't guess keys; until you
have them, keep such modifiers out of the tags rather than invent names.

Python has no Astah profile; its prefix is `umlcraft.python.` (decision 10).

## Testing

- Use fixtures per rule under `crates/umlcraft-reverse/tests/fixtures/<lang>/<case>/`, mirroring
  the Java cases where they apply:
  - packages/namespaces, kinds, modifiers, docs, inheritance, generics/templates, arrays and
    pointers, resolution, associations, reimport (`v1/` → `v2/`), syntax_error, encoding
  - plus language-specific cases: C# partial and properties; C++ out-of-class definitions,
    forward declarations, typedefs, multiple and virtual inheritance; Python type hints and
    `__init__`
- `tests/ir.rs` and `tests/import.rs` are written for Java; generalize them over the language
  (a `fixture(lang, case)` helper) rather than copying them.
- Snapshots: `expected.ir` and `expected.model`. Write them with `UPDATE_SNAPSHOTS=1` and
  **read the diff before accepting it**. A snapshot is only as good as your review.
- Every import test runs `model.check()`. Also test that undo restores the model exactly and
  that a second import of the same sources changes nothing.
- There is no Astah reference for C#, C++ or Python, because Astah imports only Java. Do not
  extend `astah_compare.rs` for them. The Java Astah comparison must stay green: it runs when
  the references in `%USERPROFILE%\UmlCraft-reference\java` exist, and leave it skipped otherwise.
- Do a real-project smoke test per language: generalize the `java-import` example (or add one
  per language). Run it on a mid-sized open-source project and record the timing in
  `docs/architecture.md`.
- Before every commit, all three must pass:
  ```
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
  cargo fmt --all --check
  ```

## Do

- Keep the mapping language-neutral. Language knowledge belongs in the front-end (syntax and
  sugar) or in `Rules` (names, library, values).
- Keep the Java import's behaviour byte-for-byte: Java snapshots and `astah_compare` must not
  change unless you mean it and explain why in the commit.
- Change the model only through `Document::edit` / `Tx`. The whole import must stay one
  undo step.
- Keep parsing free of the model (`Send`, background thread). Resolution and merge run on the
  UI thread.
- Count what you skip and report it; never drop source silently.
- On re-import, keep IDs, presentations and hand-drawn dependencies (the merge already does
  this; make sure your front-end produces stable names and orders).
- Write identifiers, comments, commit messages and docs in English. Write UI text only via
  `t!()` with keys in both locale files.
- Use LF line endings only. Match the surrounding code's style and comment density.
- Change files with the Edit tool. Any script that contains backslashes or quotes goes into
  the scratchpad as a file and runs from there (`python -I file`). Never use inline heredocs or
  `-c "…"`.
- Work on a branch of your own (e.g. `source-import-cs-cpp-py`), not on `main`. Other agents
  keep committing to `main` on Ulf's machine, so bring your branch up to date with `main` now
  and then. Shared files such as `Cargo.toml`, `main_view.rs` and the locale files may
  conflict. Commit in small, self-contained steps (one language feature or mapping change each, each
  green).
- When a design question has no obvious answer from this document or the code, ask Ulf
  before you build it.

## Don't

- Don't push to `main`, merge your branch, tag or publish a release. Pushing your own branch
  and opening the pull request at the end is fine (see "Done means").
- Don't add `Co-Authored-By` lines or Claude mentions to commits or PRs.
- Don't write a second mapping or fork `merge.rs`/`resolve.rs` per language.
- Don't resolve names in a front-end, and don't create model elements there.
- Don't put `.asta` files, anything from Astah's installation or third-party source projects
  into the repo.
  Fixtures are small hand-written files.
- Don't parse method bodies. The one exception is Python's `self.x = …` in `__init__`
  (decision 8).
- Don't bump the file format version; model changes stay additive and serde-default, with the
  `umlcraft-format` round-trip test extended.
- Don't touch the class diagram, object diagram or sequence diagram code. If you need a model
  change (new field or kind), keep it minimal, extend `element.rs` with references and `unlink`
  handled, and mention it in the commit.
- Don't add a regex crate for doc-comment cleanup; the Java Javadoc code shows the manual way.

## Ulf's decisions

Ulf answered these before you started; build them as stated. Questions that come up beyond
them go to Ulf.

1. **C# properties:**
   - **Auto-properties** become attributes with the property's type, visibility and
     initializer:
     - `{ get; set; }` stays writable.
     - `{ get; }` and `{ get; init; }` become read-only.
     - `static` stays static.
   - **Properties with bodies** become operations, named as .NET compiles them:
     - `int Count { get { … } set { … } }` gives `get_Count() : int` and
       `set_Count(value : int)`.
     - An expression-bodied `int Count => …` gives only `get_Count`.
     - An accessor with its own visibility (`private set`) gets that visibility.
   - Bodies are skipped and counted as usual.
2. **C# base list:** in `class A : B, IC`, C# requires the base class to come first, so every
   entry after the first is an interface. The first entry is an interface if it is named `I`
   followed by a capital letter (`IDisposable`), otherwise a class. In an `interface` or
   `struct`, every entry is an interface. This only matters for types that neither the sources
   nor the library table know.
3. **C++ files:** read headers and sources. Out-of-class definitions (`void A::f() {}` in a
   `.cpp`) are matched to the declaration in the class. Parameter names and documentation from
   the definition fill gaps. A definition without a declaration adds the operation.
4. **C++ preprocessor:** nothing is expanded. Of each `#if`/`#ifdef`/`#ifndef`, only the first
   branch counts. `#elif` and `#else` branches are skipped and counted (a new `Skip` variant).
   Include guards keep working this way, and declarations don't appear twice.
5. **C++ free functions and globals:** skipped and counted, as `Item::Free` is today. No utility
   class.
6. **Python types:** attributes and parameters without an annotation get no type (`name`, not
   `name : Any`) and never become associations.
7. **Python modules:** folders and modules both become packages. `shop/orders.py` puts `Order`
   into `shop › orders`, and `shop/__init__.py` stands for the package `shop` itself. So
   `from shop.orders import Order` and qualified names resolve uniquely.
8. **Python `self.x`:** assignments to `self.x` directly in the body of `__init__` become
   attributes. A `self.x: T = …` annotation gives the type. This is the only exception to "no
   method bodies"; assignments in other methods are not read.
9. **Python visibility:**
   - `_x` → protected
   - `__x` (not ending in `__`) → private
   - everything else public, including dunder names such as `__init__`
10. **Python tags:** prefix `umlcraft.python.`, for decorators, `async` and the like.
11. **Library tables:** containers and value types only, maintained by hand:
    - **C#:** the BCL (`System.Collections.Generic`, `System.Collections.Immutable` …, plus
      `string`, `DateTime`, `Guid`, `decimal` …)
    - **C++:** the standard library (`std::vector`, `std::map`, `std::optional`,
      `std::unique_ptr`, `std::shared_ptr`, `std::string` …)
    - **Python:** `typing` and `collections` (`list`, `dict`, `set`, `Optional`, `Sequence` …)

    No full generated tables like the JDK's. A small generator under `tools/` is fine where it
    saves typing.
12. **Pull request:** one for all three languages, at the end (see "Handing in").

## Done means

Per language, done means all of the following:

- The menu entries work in the app on your machine; CI covers the build on Windows and
  macOS.
- A fixture set covers the rules above, with reviewed snapshots.
- Re-import is idempotent and undo is exact.
- A real-project smoke test runs without model check errors.
- The docs are updated.
- All checks are green, and the commits are on your branch.

## Handing in

When all three languages are done, send Ulf a pull request:

1. Bring your branch up to date with `main` and resolve conflicts on your branch. Don't rebase
   commits that are already pushed. Run the three checks again.
2. Push your branch to `origin` (`github.com/udvlp/UmlCraft`).
3. Open the pull request against `main` with `gh pr create`. Write the title and description in
   English (project convention), with no Claude mention and no attribution lines. The
   description covers:
   - what each language imports
   - where it deviates from the language's semantics, and why
   - how Ulf's decisions were implemented, and any questions he answered along the way
   - what is still open, stated as facts
4. Wait until CI on the pull request is green; fix failures on your branch.
5. Tell Ulf in German, addressing him with "du", that the pull request is ready, with its
   link and a short summary. Merging is his call; don't merge it yourself and don't enable
   auto-merge.

If Ulf asks for a pull request per language instead, follow that.
