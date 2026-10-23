# UmlCraft

UML-Modellierungswerkzeug in Rust mit [GPUI](https://gpui.rs), als Neubau nach dem Vorbild von
Astah Professional. Meilenstein 1 umfasst Klassen-, Use-Case- und Sequenzdiagramme sowie den Import
von `.asta`-Dateien (nur lesen).

## Stand

- **M0** Workspace und GPUI-Spike: Modellbaum, Zeichenfläche mit Pan/Zoom, Auswahl, Ziehen mit
  Rasterfang, Undo/Redo, Hell/Dunkel, DE/EN.
- **M1** Modellkern (`umlcraft-model`) mit validierten, umkehrbaren Operationen und
  Transaktionen; JSON-Projektformat (`umlcraft-format`).
- **M2** Editor-Shell: Menüleiste, Diagramm-Tabs, Werkzeugpalette je Diagrammart mit
  Verbindungsregeln, Rahmen- und Mehrfachauswahl, Größe ändern, Ausrichten, Einpassen,
  bearbeitbares Eigenschaftenpanel, Kontextmenü im Modellbaum, Neu/Öffnen/Speichern/Zuletzt
  geöffnet mit Rückfrage bei ungespeicherten Änderungen; Kopieren/Ausschneiden/Einfügen (als Kopie
  oder als dieselben Elemente), Knickpunkte an Kanten, Drag & Drop im Modellbaum und vom Baum auf
  die Zeichenfläche.
- **M3** Klassendiagramm: Klassen wachsen mit ihrem Inhalt; Inline-Bearbeitung per Doppelklick
  mit Parser für `+ name : Typ [0..*] = init` und `+ op(p : T) : R`; Attribute, Operationen und
  Aufzählungswerte mit Statisch/Abstrakt/Abfrage/Schreibgeschützt; Assoziationsenden mit Rolle,
  Multiplizität, Navigierbarkeit und Aggregation/Komposition; Selbstassoziationen; Abschnitte
  ausblenden; Schnittstelle als Kreis („Lollipop“).

Nächste Schritte: Use-Case- und Sequenzdiagramm im Detail (M4–M5) und der `.asta`-Import (M6).

## Bauen und starten

```bash
cargo run -p umlcraft-app
```

```bash
cargo test --workspace
```

## Aufbau

| Crate | Aufgabe |
| --- | --- |
| `crates/umlcraft-model` | Metamodell, Diagramme/Presentations, `Op`, `Document`/`Tx` (Undo/Redo) |
| `crates/umlcraft-format` | `.ucraft`-Dateien: JSON, stabil sortiert, versioniert |
| `crates/umlcraft-notation` | UML-Notation ohne UI: Szene, Treffertest, Paletten, Verbindungsregeln |
| `crates/umlcraft-app` | GPUI-Anwendung (`gpui-kit` 0.8) |

Details: [docs/architektur.md](docs/architektur.md).
