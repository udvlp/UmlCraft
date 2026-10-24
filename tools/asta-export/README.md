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
"C:\Program Files\astah-professional\jre\bin\java" -Xshare:off -Djava.awt.headless=true -cp "C:\Program Files\astah-professional\astah-api.jar;C:\Program Files\astah-professional\astah-pro.jar" AstaExport.java project.ucraft layout.json out.asta report.txt
```

`layout.json` is written by `umlcraft-asta-export` (see `layout.rs`): node bounds as UmlCraft
draws them, and the message rows and fragment frames of sequence diagrams, which UmlCraft
derives instead of storing. Without an entry for a diagram, the tool falls back to the stored
bounds. `report.txt` gets one line per finding, tab-separated:

- `dropped <key> <count>`: something the API cannot express, e.g. `query`, `extendCondition`,
  `templateParameterBounds` (Astah keeps one bound per template parameter),
  `variadicTemplateParameter`, or `templateParameter` for those of operations;
- `problem <text>`: a single element that failed;
- `error <text>`: the export failed as a whole.

The quickest way to try it is the example, which writes these files itself:

```bash
cargo run -p umlcraft-asta-export --example asta-export -- project.ucraft out.asta
```

## Rules

- Only the public API (`com.change_vision.jude.api.inf.*`) is used; no Astah code or files go
  into this repository.
- The tool runs everything in one transaction and saves only if it completes. Single elements
  that fail are dropped and reported, so one odd element does not stop the export.
- Astah hides exceptions that escape `main`, so the tool catches everything and writes the
  report itself.
