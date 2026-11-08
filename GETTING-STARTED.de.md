# Erste Schritte mit UmlCraft

[English](GETTING-STARTED.md)

Diese Anleitung ist für dich, wenn du UmlCraft oder ein UML-Werkzeug dieser Art noch nicht
kennst. Sie dauert etwa eine Viertelstunde. Was UmlCraft insgesamt kann, steht im
[README](README.md) (auf Englisch).

## Der eine Gedanke, den du zuerst verstehen solltest

UmlCraft sieht aus wie ein Zeichenprogramm, ist aber keins. Ein Zeichenprogramm merkt sich
Formen: einen Kasten mit etwas Text darin, eine Linie zwischen zwei Kästen. UmlCraft merkt
sich ein **Modell**: die Klassen, Akteure, Zustände und anderen Elemente dessen, was du
beschreibst, und die Beziehungen zwischen ihnen. Ein **Diagramm** ist eine Ansicht auf einen
Teil dieses Modells.

Denk an die Menschen in einem Fotoalbum. Jeden Menschen gibt es einmal; jedes Foto zeigt einige
von ihnen. Derselbe Mensch kann auf vielen Fotos sein. Schneidest du ihn aus einem Foto
heraus, verschwindet er nicht von den anderen und schon gar nicht aus der Welt.

In UmlCraft heißt das:

- Eine Klasse `Kunde` gibt es **einmal** im Modell, egal wie viele Diagramme sie zeigen.
- Jedes Diagramm, das `Kunde` zeigt, zeigt **dieselbe** Klasse. Benennst du sie auf einem
  Diagramm um oder gibst ihr ein Attribut, zeigen alle anderen Diagramme die Änderung.
- Eine Linie zwischen zwei Klassen ist eine **Beziehung** im Modell, nicht bloß ein Strich.
  Sie gehört zu den beiden Klassen, nicht zur Zeichnung.
- Der **Modellbaum** links listet alles auf, was das Modell enthält, auch das, was kein
  Diagramm zeigt. Er ist der eigentliche Inhalt deines Projekts; die Diagramme sind Ansichten
  darauf.

Die Frage „Was ist auf meinem Diagramm?“ hat also zwei Teile: was es im Modell gibt, und was
davon dieses Diagramm zeigt, wo und wie.

| Was du willst | Im Zeichenprogramm | In UmlCraft |
| --- | --- | --- |
| Eine Klasse auf einem zweiten Diagramm zeigen | Kasten kopieren und einfügen | Die Klasse aus dem Modellbaum aufs Diagramm ziehen, oder kopieren und mit Strg+Umschalt+V einfügen |
| Eine Klasse überall umbenennen | Jeden Kasten ändern | Einmal umbenennen |
| Einen Kasten von einem Diagramm nehmen | Löschen | **Aus dem Diagramm entfernen** (Strg+Entf): Sie bleibt im Modell und auf anderen Diagrammen |
| Eine Klasse ganz loswerden | Auf jeder Seite löschen | **Aus dem Modell löschen** (Entf): Sie verschwindet von allen Diagrammen, mit ihren Linien |
| Eine zweite, eigenständige Klasse anlegen | Kopieren und einfügen | Kopieren und einfügen (Strg+C, Strg+V): Die Kopie ist ein neues Element |

**Löschen** und **Aus dem Diagramm entfernen** sind die beiden Befehle, die du dir merken
solltest. Die Entf-Taste löscht aus dem Modell. Wolltest du nur ein Diagramm aufräumen,
drück Strg+Z und nimm stattdessen Strg+Entf. Jede Änderung lässt sich rückgängig machen.

## Das Fenster

- **Titelleiste**, ganz oben: das Menü Datei, Speichern, Rückgängig, Wiederholen, rechts
  „Über UmlCraft“ und die Einstellungen (das Zahnrad).
- **Menüband** darunter, mit Registerkarten: **Start** (Zwischenablage, Löschen, Ausrichten,
  Linienstil, Layout, Neues Diagramm), eine Karte für die Art des offenen Diagramms mit
  dessen Elementen und Beziehungen, **Ansicht** (Zoom, helles oder dunkles Design, Sprache)
  und **Werkzeuge** (Quellcode importieren, Austausch mit anderen Dateien, Bilder).
- **Modell**, die Leiste links: der Modellbaum. Ein Klick auf ein Diagramm öffnet es, ein
  Klick auf ein Element wählt es aus.
- **Zeichenfläche** in der Mitte: das offene Diagramm.
- **Eigenschaften**, die Leiste rechts: alles über das Ausgewählte, etwa seinen Namen, seine
  Attribute oder die Farbe seines Kastens.

Die Leisten schweben über der Zeichenfläche. Ziehst du an ihrem inneren Rand, werden sie
breiter oder schmaler.

Bewegen auf der Zeichenfläche: Das Mausrad scrollt, Strg+Mausrad (oder zwei Finger auf dem
Touchpad) zoomt, und mit gedrückter mittlerer oder rechter Maustaste verschiebst du die
Ansicht. Das Zoomfeld unten hat einen Knopf, der das ganze Diagramm ins Fenster einpasst
(Strg+Umschalt+0).

## Ein Blick ins Beispielprojekt

**Datei › Beispielprojekt öffnen** öffnet ein fertiges Projekt, einen Fahrradverleih namens
Velo, mit einem Diagramm jeder Art. Es öffnet sich mit dem Klassendiagramm *Domain model*.
Klick dich im Modellbaum durch die Diagramme. Dann probier den Gedanken von oben aus:

1. Klick auf *Domain model* die Klasse `Station` an. Die Eigenschaften zeigen sie.
2. Öffne das Diagramm *Saturday, 9:12* im Paket *rental*. Es zeigt Objekte, darunter
   `central : Station`: ein Objekt genau dieser Klasse, mit einem Feld für jedes ihrer
   Attribute.
3. Geh zurück zu *Domain model* und benenne `Station` in `Hub` um (Doppelklick auf den
   Namen). Auf *Saturday, 9:12* steht jetzt `central : Hub`. Gib der Klasse ein weiteres
   Attribut, und das Objekt bekommt ein Feld dafür.
4. Mach alles mit Strg+Z rückgängig.

## Dein erstes Diagramm

1. **Datei › Neues Projekt** (Strg+N). Ein neues Projekt ist leer, ohne Diagramm.
2. **Start › Neues Diagramm › Klassendiagramm**. Das Menüband hat jetzt eine Karte
   *Klassendiagramm* mit ihren Werkzeugen, und der Modellbaum zeigt das neue Diagramm.
3. Wähl im Menüband **Klasse** und klick in das Diagramm. Die neue Klasse öffnet ihren Namen
   zum Tippen: Schreib `Kunde` und drück Enter.
4. Leg auf dieselbe Weise eine zweite Klasse `Bestellung` an.
5. **Attribute und Operationen**: Doppelklick unter die Einträge einer Klasse, oder
   Rechtsklick und *Attribut hinzufügen* bzw. *Operation hinzufügen*. Schreib sie, wie UML
   sie schreibt: `- name : String` oder `+ summe() : int`. Enter öffnet die nächste Zeile,
   Escape hört auf.
6. **Eine Beziehung**: Fahr mit der Maus über `Kunde`. Daneben erscheint ein Pfeil-Anfasser;
   zieh ihn auf `Bestellung` und wähl im Menü *Assoziation*. Oder wähl im Menüband ein
   Beziehungswerkzeug wie *Assoziation* und zieh von einer Klasse zur anderen. Ein
   Doppelklick nahe einem Linienende gibt ihm einen Rollennamen oder eine Multiplizität wie
   `0..*`.
7. **Speichern** mit Strg+S. UmlCraft-Projekte sind `.ucraft`-Dateien.

Sieh dir jetzt den Modellbaum an: `Kunde`, `Bestellung`, ihre Attribute und die Assoziation
stehen alle dort, neben dem Diagramm.

## Dieselbe Klasse auf einem zweiten Diagramm

1. Noch einmal **Start › Neues Diagramm › Klassendiagramm**.
2. Zieh `Kunde` aus dem Modellbaum auf das neue Diagramm, dann `Bestellung`. Die Assoziation
   kommt von selbst mit, weil sie im Modell zwischen den beiden Klassen besteht.
3. Benenne `Bestellung` auf diesem Diagramm in `Auftrag` um (Doppelklick auf den Namen) und
   öffne dann das erste Diagramm: Dort steht auch `Auftrag`.
4. Wähl `Kunde` aus und drück **Strg+Entf** (Aus dem Diagramm entfernen). Die Klasse ist von
   diesem Diagramm verschwunden, steht aber noch im Modellbaum und auf dem ersten Diagramm.
5. Wähl `Auftrag` aus und drück **Entf**. Die Klasse ist von beiden Diagrammen und aus dem
   Modellbaum verschwunden, mit der Assoziation. Strg+Z holt sie zurück.

So ist es bei jeder Diagrammart: Anwendungsfälle und Akteure, Zustände, Aktivitäten,
Komponenten, Lebenslinien. Jede Art hat im Menüband eine eigene Karte mit den Elementen und
Beziehungen, die zu ihr passen.

## UmlCraft das Anordnen überlassen

Du musst nicht jeden Kasten selbst platzieren und jede Linie selbst knicken.

- **Automatisch anordnen** (Strg+Umschalt+L) ordnet das ganze Diagramm an, oder nur das
  Ausgewählte. Sein Menü bietet „Von oben nach unten“ und „Von links nach rechts“.
- **Linien führen** (Strg+Umschalt+R) führt Linien rechtwinklig um die Kästen herum.
- Neue Linien werden für dich geführt und folgen, wenn du ihre Kästen verschiebst, bis du
  eine selbst knickst.

## Aus UmlCraft heraus

- **Datei › Exportieren › Diagramm als SVG …** oder **Diagramm als PNG …** schreibt das
  offene Diagramm als Bild.
- **Strg+Umschalt+C** kopiert das Diagramm in die Zwischenablage: Word und PowerPoint fügen
  es als Vektorgrafik ein, andere Programme als Bild.

## Tasten, die sich lohnen

| Taste | Was sie tut |
| --- | --- |
| Strg+N, Strg+O, Strg+S | Neues Projekt, öffnen, speichern |
| Strg+Z, Strg+Y | Rückgängig, wiederholen |
| Entf | Das Ausgewählte aus dem Modell löschen |
| Strg+Entf | Das Ausgewählte nur aus diesem Diagramm entfernen |
| Strg+C, Strg+X, Strg+V | Kopieren, ausschneiden, als neue Elemente einfügen |
| Strg+Umschalt+V | Dieselben Elemente einfügen, etwa auf ein anderes Diagramm |
| Strg+Umschalt+L | Automatisch anordnen |
| Strg+Umschalt+R | Linien führen |
| Strg+0, Strg+Umschalt+0 | Zoom auf 100 %, Diagramm einpassen |
| Strg+Umschalt+C | Diagramm als Bild kopieren |
| Strg+, | Einstellungen |

Auf dem Mac tritt Cmd an die Stelle von Strg.

## Wie es weitergeht

- Die anderen Diagrammarten: **Start › Neues Diagramm** listet sie alle. Das Beispielprojekt
  zeigt zu jeder ein Beispiel.
- **Struktogramme** (Nassi-Shneiderman-Diagramme): **Datei › Neues Struktogramm**.
- **Quellcode**: **Werkzeuge › Quellcode importieren** liest Java, C#, C++ oder Python ins
  Modell. Dann hast du Klassen im Modellbaum, aber noch kein Diagramm: Zieh sie auf eins.
- Das [README](README.md) beschreibt alles, was UmlCraft kann (auf Englisch).
