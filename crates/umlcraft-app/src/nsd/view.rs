//! The editor of one structogram of the project: its selection, editing and export. It shows
//! the structogram as the model holds it and changes it through the workspace, so every change
//! is a step of the project's undo history.
//!
//! It has no surface of its own: the canvas shows the structogram (zoom, panning, the grid)
//! and hands it the mouse in diagram pixels and the commands (`bind_actions`). The diagram is
//! painted from `umlcraft_nsd`'s primitives with GPUI (`super::paint`), the selection and the
//! place a drag would drop to over it, so they never reach an export. In the dark theme it is
//! drawn in the diagrams' dark colours, while exports keep Structorizer's black on white.

use gpui_kit::component::menu::PopupMenu;
use gpui_kit::component::{ActiveTheme, Theme, v_flex};
use gpui_kit::*;
use rust_i18n::t;
use umlcraft_model::Id;
use umlcraft_nsd::draw::Primitive;
use umlcraft_nsd::layout::Layout;
use umlcraft_nsd::path::{self, Path as NsdPath, Selection, Slot, Span};
use umlcraft_nsd::svg::Palette;
use umlcraft_nsd::{Color, Diagram, EditError, ElementKind, Keywords, RootKind, clip, edit};

use super::paint::{self, Place};
use super::{
    AddBranch, EditElement, ExportStructogram, ExtendDown, ExtendUp, InsertElement, MoveDown,
    MoveUp, PALETTE, RemoveBranch, SelectDown, SelectLeft, SelectRight, SelectUp, SetColor,
    ToggleDisabled, kind_name,
};
use crate::workspace::Workspace;
use crate::{Cancel, Copy, Cut, DeleteSelection, ExportPng, ExportSvg, Paste, Redo, Undo};

/// How far the mouse moves, in screen pixels, before pressing on an element becomes dragging
/// it.
const DRAG_START: f32 = 4.0;

/// Pressed on selected elements: where, whether the mouse has gone far enough to drag them,
/// and the place they would drop to. `pressed` is what was hit inside a selection that stays
/// for dragging; a click without dragging selects it after all.
struct Drag {
    span: Span,
    start: Point<Pixels>,
    moving: bool,
    slot: Option<Slot>,
    pressed: Option<Selection>,
}

/// What the properties panel edits: one element or the diagram itself.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Element(NsdPath),
    Root,
}

/// The selected element or the diagram as the properties panel shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub target: Target,
    /// The element's kind; `None` for the diagram.
    pub kind: Option<ElementKind>,
    pub text: Vec<String>,
    pub comment: Vec<String>,
    pub disabled: bool,
    /// The diagram's kind and keywords.
    pub root_kind: RootKind,
    pub keywords: Keywords,
}

/// Asks the properties panel to put the cursor into the shown text. `inserted` is the undo
/// depth before the element was inserted for it, so that its text joins that step.
pub struct EditText {
    pub inserted: Option<usize>,
}

impl EventEmitter<EditText> for NsdView {}

/// What the selection allows, for the ribbon and the context menu.
#[derive(Clone, Copy, Default)]
pub struct Allowed {
    /// One element or the diagram: the properties panel edits it.
    pub edit: bool,
    /// A run of elements: cut, delete, move, colour, disable.
    pub span: bool,
    /// Every selected element is disabled.
    pub disabled: bool,
    /// A case or parallel element, or one of its branches.
    pub branching: bool,
}

pub struct NsdView {
    workspace: Entity<Workspace>,
    /// The structogram shown.
    id: Id,
    /// The structogram as the model held it at `revision`.
    diagram: Diagram,
    revision: u64,
    selection: Selection,
    /// What an import could not show as written, and whether that list is shown.
    notes: Vec<String>,
    notes_open: bool,
    layout: Layout,
    /// What the diagram draws, again after each change.
    primitives: Vec<Primitive>,
    /// The colours of the theme it is shown in.
    palette: Palette,
    /// The diagram's size at 100 %, the frame's right and bottom lines included.
    size: (f32, f32),
    drag: Option<Drag>,
    /// Where a selection extended with Shift began; that end stays put.
    anchor: Option<NsdPath>,
    /// The canvas's, which shows the structogram and gets its commands.
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Structogram `id` as the model holds it, if it is one.
fn structogram(workspace: &Entity<Workspace>, id: Id, cx: &App) -> Option<Diagram> {
    let model = workspace.read(cx).doc.model();
    Some(model.get(id)?.kind().as_structogram()?.structogram.clone())
}

impl NsdView {
    /// The editor of structogram `id`, with what its import could not show (`notes`), shown
    /// on the canvas whose focus is `focus`.
    pub fn new(
        workspace: Entity<Workspace>,
        id: Id,
        notes: Vec<String>,
        focus: FocusHandle,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            // Undo, redo and other edits of the project may change the structogram.
            cx.observe(&workspace, |this, _, cx| {
                if this.sync(cx) {
                    cx.notify();
                }
            }),
            // The diagram and the area around it follow the theme.
            cx.observe_global::<Theme>(|this, cx| {
                let palette = palette(cx.theme());
                if palette != this.palette {
                    this.palette = palette;
                    this.draw();
                }
                cx.notify();
            }),
        ];
        let diagram = structogram(&workspace, id, cx).unwrap_or_else(|| {
            let made = umlcraft_nsd::Provenance { author: "", date: "", origin: "" };
            Diagram::new(umlcraft_nsd::RootKind::Program, &[], &made)
        });
        let revision = workspace.read(cx).doc.revision();
        let layout = Layout::of(&diagram, umlcraft_nsd::Metrics::for_platform());
        let mut view = NsdView {
            workspace,
            id,
            diagram,
            revision,
            selection: Selection::Nothing,
            notes,
            notes_open: false,
            layout,
            primitives: Vec::new(),
            palette: palette(cx.theme()),
            size: (0.0, 0.0),
            drag: None,
            anchor: None,
            focus,
            _subscriptions: subscriptions,
        };
        view.draw();
        view
    }

    /// The diagram's size at 100 %.
    pub fn size(&self) -> (f32, f32) {
        self.size
    }

    /// Takes over a changed structogram from the model; whether anything changed.
    fn sync(&mut self, cx: &mut Context<Self>) -> bool {
        let revision = self.workspace.read(cx).doc.revision();
        if revision == self.revision {
            return false;
        }
        self.revision = revision;
        let Some(diagram) = structogram(&self.workspace, self.id, cx) else { return false };
        if diagram == self.diagram {
            return false;
        }
        self.diagram = diagram;
        // The change has made a selection of its own, or left one that no longer fits.
        self.anchor = None;
        if !self.selection.is_valid(&self.diagram) {
            self.selection = Selection::Nothing;
        }
        self.layout = Layout::of(&self.diagram, umlcraft_nsd::Metrics::for_platform());
        self.draw();
        true
    }

    /// Draws the diagram as laid out.
    fn draw(&mut self) {
        let metrics = umlcraft_nsd::Metrics::for_platform();
        self.primitives = umlcraft_nsd::draw::draw(&self.diagram, &self.layout, metrics);
        let (w, h) = self.layout.size;
        self.size = ((w + 1) as f32, (h + 1) as f32);
    }

    /// The diagram as exports have it: an SVG image in Structorizer's colours.
    fn svg(&self) -> String {
        let metrics = umlcraft_nsd::Metrics::for_platform();
        umlcraft_nsd::svg::render_with(&self.diagram, &self.layout, metrics, &LIGHT)
    }

    /// Makes one undoable change of the project and shows it. Changes that are not possible
    /// (e.g. removing a case's last branch) do nothing.
    fn change(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Diagram) -> Result<Selection, EditError>,
    ) {
        let mut changed = self.diagram.clone();
        let Ok(selection) = f(&mut changed) else {
            cx.notify();
            return;
        };
        if changed != self.diagram {
            let id = self.id;
            self.workspace.update(cx, |ws, cx| {
                ws.edit_structogram(id, &changed);
                cx.notify();
            });
            self.sync(cx);
        }
        self.select(selection, cx);
    }

    /// Changes the structogram as `change` does, but keeps the selection (as far as it still
    /// fits): for the properties panel, which edits what is selected.
    fn change_in_place(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Diagram) -> Result<(), EditError>,
    ) {
        let mut changed = self.diagram.clone();
        if f(&mut changed).is_ok() && changed != self.diagram {
            let id = self.id;
            self.workspace.update(cx, |ws, cx| {
                ws.edit_structogram(id, &changed);
                cx.notify();
            });
            self.sync(cx);
        }
        cx.notify();
    }

    fn select(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.anchor = None;
        if selection.is_valid(&self.diagram) {
            self.selection = selection;
        }
        self.claim_properties(cx);
        cx.notify();
    }

    /// Selects what an AI client named (the MCP tool `select`), in the structogram as the
    /// model holds it after the client's call.
    pub fn select_for_client(&mut self, selection: Selection, cx: &mut Context<Self>) {
        self.sync(cx);
        self.select(selection, cx);
    }

    /// Selects `span`, stretched with Shift from its end at `anchor`.
    fn stretch(&mut self, span: Span, anchor: NsdPath, cx: &mut Context<Self>) {
        if Selection::Elements(span.clone()).is_valid(&self.diagram) {
            self.selection = Selection::Elements(span);
        }
        self.anchor = Some(anchor);
        self.claim_properties(cx);
        cx.notify();
    }

    /// Something of the structogram is selected: the properties panel shows it rather than
    /// what the model tree selected before.
    fn claim_properties(&self, cx: &mut Context<Self>) {
        if matches!(self.selection, Selection::Nothing) {
            return;
        }
        let id = self.id;
        self.workspace.update(cx, |ws, cx| {
            if ws.selected_element != Some(id) || !ws.selection.is_empty() {
                ws.selected_element = Some(id);
                ws.selection.clear();
                cx.notify();
            }
        });
    }

    /// The structogram's id in the model.
    pub fn id(&self) -> Id {
        self.id
    }

    /// The selected element or the diagram, if one of them is selected alone.
    pub fn shown(&self) -> Option<Shown> {
        let d = &self.diagram;
        let (root_kind, keywords) = (d.kind(), Keywords::of(d));
        match &self.selection {
            Selection::Root => Some(Shown {
                target: Target::Root,
                kind: None,
                text: d.text(),
                comment: d.comment(),
                disabled: false,
                root_kind,
                keywords,
            }),
            selection => {
                let p = selection.single()?;
                let e = path::element(d, &p)?;
                Some(Shown {
                    target: Target::Element(p.clone()),
                    kind: Some(e.kind),
                    text: e.text(),
                    comment: e.comment(),
                    disabled: e.disabled(),
                    root_kind,
                    keywords,
                })
            }
        }
    }

    /// Sets the text of an element or the signature of the diagram.
    pub fn set_text(&mut self, target: &Target, text: &[String], cx: &mut Context<Self>) {
        match target {
            Target::Element(at) => self.change_in_place(cx, |d| {
                if edit::has_text(path::element(d, at).ok_or(EditError::NoSuchPlace)?.kind) {
                    edit::set_text(d, at, text)?;
                }
                Ok(())
            }),
            Target::Root => self.change_in_place(cx, |d| {
                let (comment, kind) = (d.comment(), d.kind());
                edit::set_root(d, text, &comment, kind).map(|_| ())
            }),
        }
    }

    /// Sets the comment of an element or of the diagram.
    pub fn set_comment(&mut self, target: &Target, comment: &[String], cx: &mut Context<Self>) {
        match target {
            Target::Element(at) => {
                self.change_in_place(cx, |d| edit::set_comment(d, at, comment).map(|_| ()))
            }
            Target::Root => self.change_in_place(cx, |d| {
                let (text, kind) = (d.text(), d.kind());
                edit::set_root(d, &text, comment, kind).map(|_| ())
            }),
        }
    }

    /// Disables an element or enables it again.
    pub fn set_disabled(&mut self, at: &NsdPath, disabled: bool, cx: &mut Context<Self>) {
        self.change_in_place(cx, |d| {
            path::element_mut(d, at).ok_or(EditError::NoSuchPlace)?.set_disabled(disabled);
            Ok(())
        });
    }

    /// Makes the diagram a program, a subroutine or an includable diagram.
    pub fn set_root_kind(&mut self, kind: RootKind, cx: &mut Context<Self>) {
        self.change_in_place(cx, |d| {
            let (text, comment) = (d.text(), d.comment());
            edit::set_root(d, &text, &comment, kind).map(|_| ())
        });
    }

    /// What the selection allows.
    pub fn allowed(&self) -> Allowed {
        let d = &self.diagram;
        let span = self.span();
        Allowed {
            edit: matches!(self.selection, Selection::Root) || self.selection.single().is_some(),
            span: span.is_some(),
            disabled: span.as_ref().is_some_and(|s| {
                (s.start..s.end)
                    .all(|i| path::element(d, &s.element(i)).is_some_and(|e| e.disabled()))
            }),
            branching: self.branching().is_some(),
        }
    }

    // ----- Mouse ------------------------------------------------------------------------------
    //
    // The canvas hands the mouse over in diagram pixels (`at`) and in window coordinates
    // (`position`, for how far it moved); it pans and zooms itself.

    /// A press of the left button; whether it hit the diagram (otherwise the canvas pans and
    /// nothing is selected).
    pub fn mouse_down(
        &mut self,
        at: (i32, i32),
        position: Point<Pixels>,
        click_count: usize,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(hit) = self.layout.hit(at.0, at.1) else {
            self.drag = None;
            self.select(Selection::Nothing, cx);
            return false;
        };
        if click_count >= 2 {
            self.drag = None;
            match hit {
                Selection::Empty(seq) => {
                    // As in Structorizer: a double click into an empty branch adds an
                    // instruction there.
                    let slot = Slot { seq, index: 0 };
                    self.open_new(slot, ElementKind::Instruction, cx);
                }
                hit => {
                    self.select(hit, cx);
                    self.edit_element(&EditElement, window, cx);
                }
            }
            return true;
        }
        let current = self.selection.clone();
        if shift
            && let (Some(span), Some(path)) = (current.span(), hit.single())
            && path.len() == span.seq.len() + 1
            && path.starts_with(&span.seq)
        {
            let to = *path.last().unwrap_or(&0);
            let anchor = span.element(span.fixed(self.anchor.as_deref()));
            let span = span.to(Some(&anchor), to);
            self.stretch(span, anchor, cx);
            return true;
        }
        // Pressing on what is selected keeps it, so that several elements can be dragged.
        let keep = match (&current, &hit) {
            (Selection::Elements(span), Selection::Elements(h)) => span.covers(&h.first()),
            _ => false,
        };
        let (selection, pressed) =
            if keep { (current.clone(), (hit != current).then_some(hit)) } else { (hit, None) };
        if let Selection::Elements(span) = &selection {
            self.drag = Some(Drag {
                span: span.clone(),
                start: position,
                moving: false,
                slot: None,
                pressed,
            });
        }
        self.select(selection, cx);
        true
    }

    /// A right click at `at` selects what it hits, unless that is selected already, so that
    /// the context menu acts on it.
    pub fn select_for_menu(&mut self, at: (i32, i32), cx: &mut Context<Self>) {
        let hit = self.layout.hit(at.0, at.1).unwrap_or_default();
        let inside = match (&self.selection, &hit) {
            (Selection::Elements(span), Selection::Elements(h)) => span.covers(&h.first()),
            (current, hit) => current == hit,
        };
        if !inside {
            self.select(hit, cx);
        }
    }

    /// The mouse moved to `at`, with the left button `pressed`; `copy` with Ctrl (Cmd) held.
    pub fn mouse_move(
        &mut self,
        at: (i32, i32),
        position: Point<Pixels>,
        pressed: bool,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        if !pressed {
            if self.drag.take().is_some() {
                cx.notify();
            }
            return;
        }
        let Some(drag) = &mut self.drag else { return };
        let moved = position - drag.start;
        if !drag.moving && f32::from(moved.x.abs().max(moved.y.abs())) < DRAG_START {
            return;
        }
        drag.moving = true;
        drag.slot =
            self.layout.slot_at(at.0, at.1).filter(|s| drops_somewhere(&drag.span, s, copy));
        cx.notify();
    }

    /// The left button went up; with `copy` (Ctrl or Cmd held) dragged elements are copied,
    /// otherwise moved.
    pub fn mouse_up(&mut self, copy: bool, cx: &mut Context<Self>) {
        let (span, slot) = match self.drag.take() {
            Some(Drag { span, moving: true, slot: Some(slot), .. }) => (span, slot),
            // A click into a selection selects what it hit, e.g. an element nested in it.
            Some(Drag { moving: false, pressed: Some(hit), .. }) => {
                self.select(hit, cx);
                return;
            }
            _ => {
                cx.notify();
                return;
            }
        };
        if !drops_somewhere(&span, &slot, copy) {
            cx.notify();
            return;
        }
        if copy {
            self.change(cx, |d| {
                let copies = edit::copy(d, &span)?;
                edit::insert(d, &slot, copies)
            });
        } else {
            self.change(cx, |d| edit::move_span(d, &span, &slot));
        }
    }

    // ----- Editing ---------------------------------------------------------------------------

    fn span(&self) -> Option<Span> {
        self.selection.span().cloned()
    }

    /// Where something new goes: after the selection, or at the end of the diagram.
    fn slot(&self, after: bool) -> Slot {
        self.selection.slot(after).unwrap_or_else(|| Slot {
            seq: Vec::new(),
            index: self.diagram.children.elements.len(),
        })
    }

    /// Inserts a new element of `kind` at `slot` with its default text, selects it and has the
    /// properties panel take the text.
    fn open_new(&mut self, slot: Slot, kind: ElementKind, cx: &mut Context<Self>) {
        let text = edit::default_text(kind, Keywords::of(&self.diagram));
        let depth = self.workspace.read(cx).doc.undo_depth();
        self.change(cx, |d| {
            let e = edit::new_element(d, kind, &text);
            edit::insert(d, &slot, vec![e])
        });
        if self.workspace.read(cx).doc.undo_depth() > depth {
            cx.emit(EditText { inserted: Some(depth) });
        }
    }

    /// Puts the cursor into the properties panel's text of what is selected.
    fn edit_element(&mut self, _: &EditElement, _: &mut Window, cx: &mut Context<Self>) {
        if self.shown().is_some() {
            cx.emit(EditText { inserted: None });
        }
    }

    fn insert_element(&mut self, a: &InsertElement, _: &mut Window, cx: &mut Context<Self>) {
        let slot = self.slot(a.after);
        self.open_new(slot, a.kind, cx);
    }

    fn delete(&mut self, _: &DeleteSelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::remove(d, &span).map(|(_, selection)| selection));
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_selection(cx);
    }

    /// Puts the selection on the clipboard; whether there was something to put there.
    fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        let d = &self.diagram;
        let text = match &self.selection {
            Selection::Root => clip::copy_diagram(d),
            Selection::Elements(span) => match edit::copy(d, span) {
                Ok(elements) => clip::copy(d, elements),
                Err(_) => return false,
            },
            _ => return false,
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        // Only what is safe on the clipboard goes.
        if self.span().is_some() && self.copy_selection(cx) {
            self.delete(&DeleteSelection, window, cx);
        }
    }

    /// Pastes after the selection (into an empty branch when that is selected).
    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
        let elements = clip::paste(&text);
        if elements.is_empty() {
            return;
        }
        let slot = self.slot(true);
        self.change(cx, |d| edit::insert(d, &slot, elements));
    }

    fn cancel(&mut self, _: &Cancel, _: &mut Window, cx: &mut Context<Self>) {
        self.drag = None;
        self.select(Selection::Nothing, cx);
    }

    fn move_up(&mut self, _: &MoveUp, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::shift(d, &span, true));
        }
    }

    fn move_down(&mut self, _: &MoveDown, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::shift(d, &span, false));
        }
    }

    fn set_color(&mut self, a: &SetColor, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(span) = self.span() {
            self.change(cx, |d| edit::set_color(d, &span, Color(a.color)));
        }
    }

    fn toggle_disabled(&mut self, _: &ToggleDisabled, _: &mut Window, cx: &mut Context<Self>) {
        let Some(span) = self.span() else { return };
        let d = &self.diagram;
        let all = (span.start..span.end)
            .all(|i| path::element(d, &span.element(i)).is_some_and(|e| e.disabled()));
        self.change(cx, |d| edit::set_disabled(d, &span, !all));
    }

    /// The case or parallel element whose branches the selection is about, and the branch
    /// meant: a selected empty branch, or none for the element itself.
    fn branching(&self) -> Option<(NsdPath, Option<usize>)> {
        let d = &self.diagram;
        let branching = |p: &[usize]| {
            path::element(d, p)
                .is_some_and(|e| matches!(e.kind, ElementKind::Case | ElementKind::Parallel))
        };
        match &self.selection {
            Selection::Empty(seq) => {
                let owner = edit::owner(seq)?;
                branching(&owner).then(|| (owner, seq.last().copied()))
            }
            selection => selection.single().filter(|p| branching(p)).map(|p| (p, None)),
        }
    }

    fn add_branch(&mut self, _: &AddBranch, _: &mut Window, cx: &mut Context<Self>) {
        let Some((at, _)) = self.branching() else { return };
        let Some(e) = path::element(&self.diagram, &at) else { return };
        // A new case goes before the default branch.
        let index = match e.kind {
            ElementKind::Case => e.branches.len() - 1,
            _ => e.branches.len(),
        };
        self.change(cx, |d| edit::add_branch(d, &at, index));
    }

    fn remove_branch(&mut self, _: &RemoveBranch, _: &mut Window, cx: &mut Context<Self>) {
        let Some((at, branch)) = self.branching() else { return };
        let Some(e) = path::element(&self.diagram, &at) else { return };
        let last = match e.kind {
            ElementKind::Case => e.branches.len().saturating_sub(2),
            _ => e.branches.len().saturating_sub(1),
        };
        let index = branch.unwrap_or(last);
        self.change(cx, |d| edit::remove_branch(d, &at, index));
    }

    // ----- Keyboard selection ----------------------------------------------------------------

    /// The first element of a sequence, or the sequence when it is empty.
    fn first_in(&self, seq: NsdPath) -> Selection {
        let empty = path::sequence(&self.diagram, &seq).is_none_or(|s| s.elements.is_empty());
        if empty {
            Selection::Empty(seq)
        } else {
            let mut p = seq;
            p.push(0);
            Selection::element(&p)
        }
    }

    /// The element after `p`, climbing out of branches at their end.
    fn after(&self, p: &[usize]) -> Option<Selection> {
        let (last, seq) = p.split_last()?;
        let len = path::sequence(&self.diagram, seq)?.elements.len();
        if last + 1 < len {
            let mut next = seq.to_vec();
            next.push(last + 1);
            return Some(Selection::element(&next));
        }
        self.after(&edit::owner(seq)?)
    }

    /// The branch next to the one holding `seq` (`step` -1 or 1).
    fn beside(&self, seq: &[usize], step: isize) -> Option<Selection> {
        let owner = edit::owner(seq)?;
        let branch = seq.last()?.checked_add_signed(step)?;
        let e = path::element(&self.diagram, &owner)?;
        // A case without default hides its last branch.
        let shown = match e.kind {
            ElementKind::Case if e.text().last().is_some_and(|l| l.trim() == "%") => {
                e.branches.len() - 1
            }
            _ => e.branches.len(),
        };
        (branch < shown).then(|| {
            let mut s = owner;
            s.push(branch);
            self.first_in(s)
        })
    }

    fn select_up(&mut self, _: &SelectUp, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) if span.start > 0 => {
                Some(Selection::element(&span.element(span.start - 1)))
            }
            Selection::Elements(Span { seq, .. }) | Selection::Empty(seq) => {
                edit::owner(seq).map(|o| Selection::element(&o))
            }
            Selection::Nothing => Some(self.first_in(Vec::new())),
            Selection::Root => None,
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_down(&mut self, _: &SelectDown, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) => self.after(&span.element(span.end - 1)),
            Selection::Empty(seq) => edit::owner(seq).and_then(|o| self.after(&o)),
            Selection::Nothing | Selection::Root => Some(self.first_in(Vec::new())),
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        let seq = match &self.selection {
            Selection::Elements(span) => span.seq.clone(),
            Selection::Empty(seq) => seq.clone(),
            _ => return,
        };
        let next =
            self.beside(&seq, -1).or_else(|| edit::owner(&seq).map(|o| Selection::element(&o)));
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        let next = match &self.selection {
            Selection::Elements(span) => {
                let p = span.first();
                let branches = path::element(&self.diagram, &p).map_or(0, |e| e.branches.len());
                if span.len() == 1 && branches > 0 {
                    let mut s = p;
                    s.push(0);
                    Some(self.first_in(s))
                } else {
                    self.beside(&span.seq, 1)
                }
            }
            Selection::Empty(seq) => self.beside(seq, 1),
            _ => None,
        };
        if let Some(next) = next {
            self.select(next, cx);
        }
    }

    fn extend(&mut self, up: bool, cx: &mut Context<Self>) {
        let Some(span) = self.span() else { return };
        let len = path::sequence(&self.diagram, &span.seq).map_or(0, |s| s.elements.len());
        let anchor = span.element(span.fixed(self.anchor.as_deref()));
        if let Some(span) = span.step(Some(&anchor), up, len) {
            self.stretch(span, anchor, cx);
        }
    }

    fn extend_up(&mut self, _: &ExtendUp, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(true, cx);
    }

    fn extend_down(&mut self, _: &ExtendDown, _: &mut Window, cx: &mut Context<Self>) {
        self.extend(false, cx);
    }

    // ----- Export ----------------------------------------------------------------------------

    /// The file name an export suggests: the structogram's name, as its signature begins.
    fn file_stem(&self) -> String {
        let text = self.diagram.text().join(" ");
        let name = text.split('(').next().unwrap_or_default().trim();
        let name = if name.is_empty() { t!("nsd.untitled_stem").to_string() } else { name.into() };
        crate::main_view::sanitize_file_name(&name)
    }

    /// Asks where to write the export with `extension` (next to the project), then writes it
    /// in the background; a failure is reported with `failed`.
    fn write_export(
        &self,
        extension: &'static str,
        failed: &'static str,
        bytes: impl FnOnce() -> anyhow::Result<Vec<u8>> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let directory = crate::main_view::start_directory(self.workspace.read(cx).path.as_deref());
        let suggested = format!("{}.{extension}", self.file_stem());
        let rx = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(path))) = rx.await else { return };
            let path = crate::main_view::with_extension(path, extension);
            let written = cx
                .background_executor()
                .spawn(async move { anyhow::Ok(write_file(&path, &bytes()?)?) })
                .await;
            if let Err(err) = written {
                cx.update(|window, cx| {
                    let rx = window.prompt(
                        PromptLevel::Critical,
                        &t!(failed),
                        Some(&err.to_string()),
                        &[t!("dialog.ok").as_ref()],
                        cx,
                    );
                    cx.spawn(async move |_| rx.await.ok()).detach();
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn export_image(&mut self, png: bool, window: &mut Window, cx: &mut Context<Self>) {
        let svg = self.svg();
        let bytes = move || {
            Ok(if png {
                // `from_svg` takes images made at Astah's 4/3 pixels per unit; ours have one.
                let scale = crate::png::SCALE * umlcraft_notation::svg::SCALE;
                crate::png::from_svg(&svg, scale)?
            } else {
                svg.into_bytes()
            })
        };
        let extension = if png { "png" } else { "svg" };
        self.write_export(extension, "dialog.image_failed", bytes, window, cx);
    }

    /// Puts the structogram on the clipboard as SVG and as PNG.
    pub fn copy_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let scale = crate::png::SCALE * umlcraft_notation::svg::SCALE;
        crate::png::copy(self.svg(), scale, window, cx);
    }

    fn export_svg(&mut self, _: &ExportSvg, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(false, window, cx);
    }

    fn export_png(&mut self, _: &ExportPng, window: &mut Window, cx: &mut Context<Self>) {
        self.export_image(true, window, cx);
    }

    /// Writes the structogram as a Structorizer file, with a header of today.
    fn export_nsd(&mut self, _: &ExportStructogram, window: &mut Window, cx: &mut Context<Self>) {
        let (author, date, origin) = (super::user_name(), super::today(), super::origin());
        let made = umlcraft_nsd::Provenance { author: &author, date: &date, origin: &origin };
        let text = umlcraft_nsd::write(&umlcraft_nsd::content::to_file(&self.diagram, &made));
        let bytes = move || Ok(text.into_bytes());
        self.write_export("nsd", "nsd.save_failed", bytes, window, cx);
    }

    // ----- Drawing ---------------------------------------------------------------------------

    /// What to draw over the diagram, in diagram pixels: the selection's boxes, and where a
    /// drag would drop (a line between elements, or a box for an empty branch).
    fn overlay(&self) -> (Vec<[i32; 4]>, Option<[i32; 4]>) {
        let l = &self.layout;
        let boxes = match &self.selection {
            Selection::Nothing => Vec::new(),
            Selection::Root => vec![[0, 0, l.size.0, l.size.1]],
            // A try's empty `finally` is not drawn; it shows where it would go.
            Selection::Empty(seq) => l
                .sequence(seq)
                .map(|s| s.rect)
                .or_else(|| l.hidden_finally(seq))
                .map(|r| vec![rect(r)])
                .unwrap_or_default(),
            Selection::Elements(span) => {
                let first = l.element(&span.first());
                let last = l.element(&span.element(span.end - 1));
                match (first, last) {
                    (Some(a), Some(b)) => {
                        vec![[a.rect.x, a.rect.y, a.rect.w, b.rect.bottom() - a.rect.y]]
                    }
                    _ => Vec::new(),
                }
            }
        };
        let drop = match &self.drag {
            Some(Drag { moving: true, slot: Some(slot), .. }) => l
                .sequence(&slot.seq)
                .map(|s| match s.elements.get(slot.index) {
                    _ if s.elements.is_empty() => rect(s.rect),
                    Some(e) => [s.rect.x, e.rect.y - 1, s.rect.w, 3],
                    None => {
                        let bottom = s.elements.last().map_or(s.rect.y, |e| e.rect.bottom());
                        [s.rect.x, bottom - 1, s.rect.w, 3]
                    }
                })
                .or_else(|| l.hidden_finally(&slot.seq).map(rect)),
            _ => None,
        };
        (boxes, drop)
    }

    /// Paints the structogram with its origin at `origin` on the screen, at `zoom`, and the
    /// selection over it in `accent`.
    pub fn paint(
        &self,
        origin: Point<Pixels>,
        zoom: f32,
        accent: Hsla,
        window: &mut Window,
        cx: &mut App,
    ) {
        let place = Place { origin, zoom };
        let family: SharedString = umlcraft_nsd::Metrics::for_platform().font_family().into();
        paint::paint(&self.primitives, &self.palette, &family, place, window, cx);
        let (boxes, drop) = self.overlay();
        let on_screen = |[x, y, w, h]: [i32; 4]| {
            Bounds::new(
                origin + point(px(x as f32 * zoom), px(y as f32 * zoom)),
                size(px(w as f32 * zoom), px(h as f32 * zoom)),
            )
        };
        for b in boxes {
            window.paint_quad(quad(
                on_screen(b),
                px(0.0),
                accent.opacity(0.12),
                px(2.0),
                accent,
                BorderStyle::Solid,
            ));
        }
        if let Some(d) = drop {
            window.paint_quad(fill(on_screen(d), accent));
        }
    }

    /// What an import could not show as written: a line that opens into the list.
    pub fn notes_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.notes.is_empty() {
            return None;
        }
        let summary = t!("nsd.notes", count = self.notes.len()).to_string();
        let arrow = if self.notes_open { "▾" } else { "▸" };
        let list = self.notes_open.then(|| {
            v_flex()
                .id("nsd-notes")
                .max_h(px(140.0))
                .overflow_y_scroll()
                .pl_4()
                .children(self.notes.iter().map(|n| div().text_xs().child(n.clone())))
        });
        Some(
            v_flex()
                .px_2()
                .py_1()
                .border_t_1()
                .border_color(cx.theme().border)
                .text_sm()
                .child(
                    div()
                        .id("nsd-notes-toggle")
                        .cursor_pointer()
                        .child(format!("{arrow} {summary}"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.notes_open = !this.notes_open;
                            cx.notify();
                        })),
                )
                .children(list)
                .into_any_element(),
        )
    }

    /// The context menu: what the selection allows.
    pub fn context_menu(
        view: &Entity<Self>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let (selection, allowed, can_undo, can_redo, focus) = {
            let this = view.read(cx);
            let doc = &this.workspace.read(cx).doc;
            (
                this.selection.clone(),
                this.allowed(),
                doc.can_undo(),
                doc.can_redo(),
                this.focus.clone(),
            )
        };
        let (branching, disabled) = (allowed.branching, allowed.disabled);
        let editable = matches!(selection, Selection::Root) || selection.single().is_some();
        let has_span = selection.span().is_some();
        let copyable = has_span || matches!(selection, Selection::Root);
        let mut menu = menu
            .action_context(focus)
            .menu_with_disabled(t!("nsd.edit"), Box::new(EditElement), !editable)
            .separator();
        for (title, after) in [("nsd.insert_after", true), ("nsd.insert_before", false)] {
            menu = menu.submenu(t!(title), window, cx, move |menu, _, _| {
                ElementKind::ALL.into_iter().fold(menu, |menu, kind| {
                    menu.menu(kind_name(kind), Box::new(InsertElement { kind, after }))
                })
            });
        }
        menu = menu
            .separator()
            .menu_with_disabled(t!("menu.cut"), Box::new(Cut), !has_span)
            .menu_with_disabled(t!("menu.copy"), Box::new(Copy), !copyable)
            .menu(t!("nsd.paste"), Box::new(Paste))
            .menu_with_disabled(t!("nsd.delete"), Box::new(DeleteSelection), !has_span)
            .separator()
            .menu_with_disabled(t!("nsd.move_up"), Box::new(MoveUp), !has_span)
            .menu_with_disabled(t!("nsd.move_down"), Box::new(MoveDown), !has_span)
            .separator();
        if has_span {
            menu = menu.submenu(t!("nsd.color"), window, cx, |menu, _, _| {
                PALETTE.into_iter().enumerate().fold(menu, |menu, (i, color)| {
                    menu.menu(t!(format!("nsd.color_{i}")), Box::new(SetColor { color }))
                })
            });
            menu = menu.menu_with_check(t!("nsd.disabled"), disabled, Box::new(ToggleDisabled));
        }
        if branching {
            menu = menu
                .menu(t!("nsd.add_branch"), Box::new(AddBranch))
                .menu(t!("nsd.remove_branch"), Box::new(RemoveBranch));
        }
        menu.separator()
            .menu_with_disabled(t!("toolbar.undo"), Box::new(Undo), !can_undo)
            .menu_with_disabled(t!("toolbar.redo"), Box::new(Redo), !can_redo)
    }
}

/// Whether dropping `span` at `slot` would change anything and is possible. A copy goes
/// anywhere, also next to or into the elements it copies; they cannot move there.
fn drops_somewhere(span: &Span, slot: &Slot, copy: bool) -> bool {
    let in_place = slot.seq == span.seq && (span.start..=span.end).contains(&slot.index);
    copy || (!in_place && !span.covers(&slot.seq))
}

fn rect(r: umlcraft_nsd::layout::Rect) -> [i32; 4] {
    [r.x, r.y, r.w, r.h]
}

/// Writes `bytes` through a temporary file, so that a failed write leaves the old file whole.
fn write_file(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = std::path::PathBuf::from(temporary);
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        std::fs::remove_file(&temporary).ok();
    })
}

/// What exports have and the light theme shows.
const LIGHT: Palette = Palette::PLAIN;

/// The colours a structogram is shown in: [`LIGHT`] in the light theme, those of the diagrams
/// in the dark one, with the elements' own colours dimmed. Neither marks elements with a
/// comment.
fn palette(theme: &Theme) -> Palette {
    if !theme.is_dark() {
        return LIGHT;
    }
    let color = |c: Hsla| {
        let c = c.to_rgb();
        let byte = |v: f32| (v * 255.0).round().clamp(0.0, 255.0) as u8;
        Color([byte(c.r), byte(c.g), byte(c.b)])
    };
    Palette {
        line: color(theme.foreground),
        background: color(theme.secondary),
        head: color(theme.muted),
        comment: None,
        hatch: color(theme.muted_foreground),
        includes: color(theme.muted_foreground),
        tint: 0.35,
    }
}

/// Hands the structogram's commands that reach `element` (the canvas showing it) to `editor`.
pub fn bind_actions<E: InteractiveElement>(element: E, editor: &Entity<NsdView>) -> E {
    // Each command goes to the editor's handler of the same name.
    macro_rules! bind {
        ($element:expr, $($action:ty => $handler:ident),* $(,)?) => {
            $element$(.on_action({
                let editor = editor.clone();
                move |a: &$action, window: &mut Window, cx: &mut App| {
                    editor.update(cx, |e, cx| e.$handler(a, window, cx));
                }
            }))*
        };
    }
    bind!(
        element,
        ExportSvg => export_svg,
        ExportPng => export_png,
        ExportStructogram => export_nsd,
        EditElement => edit_element,
        InsertElement => insert_element,
        DeleteSelection => delete,
        Copy => copy,
        Cut => cut,
        Paste => paste,
        Cancel => cancel,
        MoveUp => move_up,
        MoveDown => move_down,
        SetColor => set_color,
        ToggleDisabled => toggle_disabled,
        AddBranch => add_branch,
        RemoveBranch => remove_branch,
        SelectUp => select_up,
        SelectDown => select_down,
        SelectLeft => select_left,
        SelectRight => select_right,
        ExtendUp => extend_up,
        ExtendDown => extend_down,
    )
}

impl Focusable for NsdView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
