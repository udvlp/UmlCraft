# Getting started with UmlCraft

[Deutsch](GETTING-STARTED.de.md)

This guide is for you if you have never used UmlCraft, or a UML tool of its kind. It takes
about fifteen minutes. What UmlCraft can do in full is in the [README](README.md).

## The one idea to understand first

UmlCraft looks like a drawing program, but it is not one. A drawing program keeps shapes: a
box with some text in it, a line between two boxes. UmlCraft keeps a **model**: the classes,
actors, states and other elements of what you describe, and the relationships between them.
A **diagram** is a view of part of that model.

Think of the people in a photo album. A person exists once; each photo shows some of them.
The same person can be in many photos. Taking someone out of one photo does not remove them
from the others, let alone from the world.

In UmlCraft:

- A class `Customer` exists **once** in the model, however many diagrams show it.
- Every diagram that shows `Customer` shows the **same** class. Rename it or give it an
  attribute on one diagram, and every other diagram shows the change.
- A line between two classes is a **relationship** of the model, not just a line. It belongs
  to the two classes, not to the drawing.
- The **model tree** on the left lists everything the model holds, also what no diagram
  shows. It is the real content of your project; the diagrams are views of it.

So the question "what is on my diagram?" has two parts: what exists in the model, and which
of it this diagram shows, where and how.

| What you want | In a drawing program | In UmlCraft |
| --- | --- | --- |
| Show a class on a second diagram | Copy and paste the box | Drag the class from the model tree onto the diagram, or copy it and paste it with Ctrl+Shift+V |
| Rename a class everywhere | Edit every box | Rename it once |
| Take a box off one diagram | Delete it | **Remove from diagram** (Ctrl+Delete): it stays in the model and on other diagrams |
| Get rid of a class altogether | Delete it on every page | **Delete from model** (Delete): it disappears from every diagram, with its lines |
| Make a second, independent class | Copy and paste | Copy and paste (Ctrl+C, Ctrl+V): the copy is a new element |

**Delete** and **Remove from diagram** are the two commands to remember. The Delete key
deletes from the model. If you only meant to tidy one diagram, press Ctrl+Z and use
Ctrl+Delete instead. Every change can be undone.

## The window

- **Title bar**, at the very top: the File menu, Save, Undo, Redo, and on the right About
  UmlCraft and the settings (the gear).
- **Ribbon**, below it, with tabs: **Home** (clipboard, delete, alignment, line style,
  layout, New diagram), a tab for the kind of diagram that is open with its elements and
  relationships, **View** (zoom, light or dark theme, language) and **Tools** (source code
  import, exchange with other files, images).
- **Model**, the panel on the left: the model tree. Clicking a diagram opens it; clicking an
  element selects it.
- **Canvas**, in the middle: the open diagram.
- **Properties**, the panel on the right: everything about what is selected, such as its
  name, its attributes or the color of its box.

The panels float over the canvas. Drag their inner edge to make them wider or narrower.

Moving around the canvas: the mouse wheel scrolls, Ctrl+wheel (or two fingers on a touchpad)
zooms, and holding the middle or right mouse button down pans. The zoom box at the bottom
has a button that fits the whole diagram into the window (Ctrl+Shift+0).

## A look at the sample project

**File › Open sample project** opens a finished project, a bike rental called Velo, with a
diagram of every kind. It opens with the class diagram *Domain model*. Click through the
diagrams in the model tree. Then try the idea above:

1. Click the class `Station` on *Domain model*. The properties panel shows it.
2. Open the diagram *Saturday, 9:12* in the package *rental*. It shows objects, among them
   `central : Station`: an object of that very class, with a slot for each of its attributes.
3. Go back to *Domain model* and rename `Station` to `Hub` (double-click its name). On
   *Saturday, 9:12* the object now reads `central : Hub`. Give the class another attribute,
   and the object gets a slot for it.
4. Undo with Ctrl+Z.

## Your first diagram

1. **File › New project** (Ctrl+N). A new project is empty, without a diagram.
2. **Home › New diagram › Class diagram**. The ribbon now has a tab *Class diagram* with its
   tools, and the model tree shows the new diagram.
3. Choose **Class** in the ribbon and click into the diagram. The new class opens its name
   for typing: type `Customer` and press Enter.
4. Add a second class `Order` the same way.
5. **Attributes and operations**: double-click below the members of a class, or right-click
   it and choose *Add attribute* or *Add operation*. Type them as UML writes them:
   `- name : String` or `+ total() : int`. Enter opens the next row; Escape stops.
6. **A relationship**: hover over `Customer`. An arrow handle appears beside it; drag it onto
   `Order` and choose *Association* from the menu. Alternatively choose a relationship tool
   in the ribbon, such as *Association*, and drag from one class to the other. Double-click near an end of the line
   to give it a role name or a multiplicity such as `0..*`.
7. **Save** with Ctrl+S. UmlCraft projects are `.ucraft` files.

Now look at the model tree: `Customer`, `Order`, their attributes and the association are
all there, next to the diagram.

## The same class on a second diagram

1. **Home › New diagram › Class diagram** again.
2. Drag `Customer` from the model tree onto the new diagram, then `Order`. The association
   comes along by itself, because it exists between the two classes in the model.
3. Rename `Order` to `Purchase` on this diagram (double-click its name), then open the first
   diagram: it says `Purchase` too.
4. Select `Customer` and press **Ctrl+Delete** (Remove from diagram). It is gone from this
   diagram, but still in the model tree and on the first diagram.
5. Select `Purchase` and press **Delete**. It is gone from both diagrams and from the model
   tree, with the association. Press Ctrl+Z to bring it back.

The same holds for every diagram kind: use cases and actors, states, activities, components,
lifelines. Each kind has its own tab in the ribbon with the elements and relationships that
make sense for it.

## Letting UmlCraft do the layout

You do not have to place every box and bend every line by hand.

- **Arrange automatically** (Ctrl+Shift+L) lays out the whole diagram, or only what is
  selected. Its menu offers top to bottom or left to right.
- **Route lines** (Ctrl+Shift+R) leads lines around the boxes at right angles.
- New lines are routed for you and follow when you move their boxes, until you bend one by
  hand.

## Getting it out of UmlCraft

- **File › Export › Diagram as SVG …** or **Diagram as PNG …** writes the open diagram as an
  image.
- **Ctrl+Shift+C** copies the diagram to the clipboard: Word and PowerPoint paste it as a
  vector image, other programs as a picture.

## Keys worth knowing

| Key | What it does |
| --- | --- |
| Ctrl+N, Ctrl+O, Ctrl+S | New project, open, save |
| Ctrl+Z, Ctrl+Y | Undo, redo |
| Delete | Delete the selection from the model |
| Ctrl+Delete | Remove the selection from this diagram only |
| Ctrl+C, Ctrl+X, Ctrl+V | Copy, cut, paste as new elements |
| Ctrl+Shift+V | Paste the same elements, for instance onto another diagram |
| Ctrl+Shift+L | Arrange automatically |
| Ctrl+Shift+R | Route lines |
| Ctrl+0, Ctrl+Shift+0 | Zoom to 100 %, fit the diagram |
| Ctrl+Shift+C | Copy the diagram as an image |
| Ctrl+, | Settings |

On macOS, Cmd takes the place of Ctrl.

## Where to go next

- The other diagram kinds: **Home › New diagram** lists them all. The sample project shows
  an example of each.
- **Structograms** (Nassi-Shneiderman diagrams): **File › New structogram**.
- **Source code**: **Tools › Import source code** reads Java, C#, C++ or Python into the
  model. That gives you classes in the model tree but no diagram yet: drag them onto one.
- The [README](README.md) describes everything UmlCraft can do.
