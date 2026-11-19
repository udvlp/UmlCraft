# asta-export

`AstaExport.java` builds an Astah project (`.asta`) from an UmlCraft project through Astah's
official Java API (`astah-api.jar`). UmlCraft runs it for File › Export › Astah project …; the
crate `umlcraft-asta-export` embeds this file, so nothing here has to be built or installed.

Writing `.asta` directly is not an option: the format is Astah's Java-serialized object graph,
which is fragile to write and changes between Astah versions. The API is what Astah offers for
creating projects from outside.

## Running it by hand

Astah's bundled runtime includes the Java compiler, so the tool runs in source-file mode:

```bash
"C:\Program Files\astah-professional\jre\bin\java" -Xshare:off -Xmx1024m -Djava.awt.headless=true -cp "C:\Program Files\astah-professional\astah-api.jar;C:\Program Files\astah-professional\astah-pro.jar" AstaExport.java project.ucraft layout.json out.asta report.txt
```

`layout.json` is written by `umlcraft-asta-export` (see `layout.rs`): node bounds as UmlCraft
draws them and where it puts labels, which UmlCraft derives instead of storing. Sequence
diagrams store all of their geometry and need no entry. Without an entry for a diagram, the tool falls back to the stored
bounds. `report.txt` gets one line per finding, tab-separated:

- `dropped <key> <count>`: something the API cannot express, e.g. `query`, `extendCondition`,
  `templateParameterBounds` (Astah keeps one bound per template parameter),
  `variadicTemplateParameter`, or `templateParameter` for those of operations; for ER
  diagrams `erChenNotation` (a Chen diagram written in IE), `erRelationship`, `erUnique`,
  `erDisjoint`, `erCardinality`, `erForeignKey` (a foreign key column whose relationship was
  not made, written as a plain column) and the like; `partialDiagram` for a diagram that failed
  after it was created and is in the file as far as it got;
- `problem <text>`: a single element that failed;
- `error <text>`: the export failed as a whole.

The quickest way to try it is the example, which writes these files itself:

```bash
cargo run -p umlcraft-asta-export --example asta-export -- project.ucraft out.asta
```

## Rules

- Only the public API (`com.change_vision.jude.api.inf.*`) is used; no Astah code or files go
  into this repository.
- The tool builds the project in one transaction (a second one only puts the substates of
  orthogonal states back) and saves only if it completes. Single elements and details that
  fail are dropped and reported, so one odd element does not stop the export.
- Astah hides exceptions that escape `main`, so the tool catches everything and writes the
  report itself.
- CI compiles the tool against `stubs/`: hand-written declarations of exactly the API classes
  and methods it calls, without Astah's jars (`.github/workflows/ci.yml`, job "Astah export
  tool"). A call to an API method the stubs lack fails that job; add the method to the stub
  as the API declares it (Astah's API reference), and check the tool against the real
  `astah-api.jar` with the round-trip tests (`cargo test -p umlcraft-asta-export` with
  `UMLCRAFT_REQUIRE_ASTAH=1` on a machine with Astah).
