//! Random edits as the mouse and keyboard make them (moving, resizing and aligning boxes,
//! bending and drawing lines with the diagram's own palette, moving messages, gates, lifeline
//! ends, labels and region lines, putting fragments around messages, adding and deleting
//! operands and regions, creating nodes on and in others, copy and paste, showing elements,
//! typing member text and names, ER notations and identifying relationships, removing from the
//! diagram, changing a relationship's kind), with undo, redo and deletes in between, on the
//! sample project, the order system and Astah's sample. After every edit the rules of
//! [`invariants`] must hold as far as they held before, undo and redo must give back exactly
//! the model before and after it, and member text must give back the member it shows.
//!
//! A failure names the seed and the step. To replay one seed, or to run longer:
//! `UMLCRAFT_FUZZ_SEED=7 UMLCRAFT_FUZZ_STEPS=2000 cargo test -p umlcraft-app fuzz`.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use umlcraft_model::{
    DiagramKind, Document, ElementKind, ErNotation, Id, LineShape, Model, Point, Rect, Shape,
};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Alignment, ApproxMeasure, Scene};
use umlcraft_notation::{invariants, member, state_edit};

use crate::workspace::Workspace;

/// xorshift64*: small, and the same on every machine.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    /// A distance as the canvas snaps it: a multiple of 5 within ±`max`.
    fn offset(&mut self, max: f32) -> f32 {
        let steps = (max / 5.0) as u64;
        (self.next() % (2 * steps + 1)) as f32 * 5.0 - max
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[self.below(items.len())])
    }
}

fn documents() -> Vec<(&'static str, Document)> {
    let mut docs = vec![
        ("sample project", crate::demo::velo(&ApproxMeasure)),
        ("order system", umlcraft_model::sample::order_system().0),
    ];
    let home = std::env::var_os("ASTAH_HOME")
        .map_or_else(|| r"C:\Program Files\astah-professional".into(), std::path::PathBuf::from);
    match umlcraft_asta::import(&home.join("Sample.asta")) {
        Ok(imported) => docs.push(("Astah's sample", Document::new(imported.model))),
        Err(err) => eprintln!("skipped Astah's sample: {err}"),
    }
    docs
}

fn diagrams(model: &Model) -> Vec<Id> {
    let mut ids: Vec<Id> = model
        .views()
        .filter(|(d, _)| model.get(*d).and_then(|e| e.kind().as_diagram()).is_some())
        .map(|(d, _)| d)
        .collect();
    // Views come out of a map; the order must not depend on it.
    ids.sort();
    ids
}

/// One random edit on a random diagram of `ws`, as a sentence for the report.
fn edit(ws: &mut Workspace, rng: &mut Rng) -> String {
    let model = ws.doc.model();
    let Some(diagram) = rng.pick(&diagrams(model)) else { return "nothing".into() };
    ws.open_diagram(diagram);
    let model = ws.doc.model();
    let kind = model.get(diagram).and_then(|d| d.kind().as_diagram());
    let props = match model.get(diagram).map(|d| d.kind()) {
        Some(ElementKind::Diagram(p)) => Some(p),
        _ => None,
    };
    let name = |id: Id| -> String {
        let view = model.view(diagram);
        let element = view.and_then(|v| v.get(id)).and_then(|p| p.element);
        element.and_then(|e| model.get(e)).map_or_else(|| id.to_string(), |e| e.name().to_string())
    };
    let where_ = format!("on {:?}", model.get(diagram).map_or("", |d| d.name()));
    let drawn: Scene = scene::build(model, diagram, &scene::Preview::default(), &*ws.measure);
    let view = model.view(diagram).expect("a diagram");
    // The nodes as drawn, with whether they have resize handles.
    let nodes: Vec<(Id, Rect, bool)> = drawn
        .nodes
        .iter()
        .filter(|n| n.element != Some(diagram) && view.get(n.id).and_then(|p| p.bounds()).is_some())
        .map(|n| (n.id, n.rect, n.resizable()))
        .collect();
    let lines: Vec<&scene::EdgeShape> = drawn.edges.iter().filter(|e| !e.fixed).collect();
    let messages: Vec<Id> = drawn.edges.iter().filter(|e| e.fixed).map(|e| e.id).collect();

    match rng.below(28) {
        0..=3 => {
            let Some((node, _, _)) = rng.pick(&nodes) else { return "nothing".into() };
            let mut selection = vec![node];
            if rng.chance(20)
                && let Some((other, _, _)) = rng.pick(&nodes)
                && other != node
            {
                selection.push(other);
            }
            let (dx, dy) = (rng.offset(150.0), rng.offset(150.0));
            let names: Vec<String> = selection.iter().map(|&n| name(n)).collect();
            // As the canvas drags them: containers take what is drawn inside along.
            let moved = scene::with_contents(ws.doc.model(), &drawn, &selection);
            ws.select(selection);
            ws.move_nodes(&moved, dx, dy);
            format!("move {names:?} by ({dx}, {dy}) {where_}")
        }
        4 => {
            let resizable: Vec<(Id, Rect)> =
                nodes.iter().filter(|n| n.2).map(|&(id, r, _)| (id, r)).collect();
            let Some((node, r)) = rng.pick(&resizable) else { return "nothing".into() };
            let (dx, dy) = (rng.offset(40.0), rng.offset(40.0));
            let width = (r.width + rng.offset(60.0)).max(10.0);
            let height = (r.height + rng.offset(60.0)).max(10.0);
            let rect = Rect::new(r.x + dx, r.y + dy, width, height);
            let what = name(node);
            ws.set_bounds(node, rect);
            format!("resize {what:?} from {r:?} to {rect:?} {where_}")
        }
        5 | 6 => {
            let Some(line) = rng.pick(&lines) else { return "nothing".into() };
            let Some((handle, at)) = rng.pick(&line.handles()) else { return "nothing".into() };
            let to = at.offset(rng.offset(120.0), rng.offset(120.0));
            let route = scene::drag_handle(line, handle, to);
            let what = name(line.id);
            if route != line.route() {
                ws.set_route(line.id, route);
            }
            format!("drag {handle:?} of line {what:?} from {at:?} to {to:?} {where_}")
        }
        7 => {
            let Some(line) = rng.pick(&lines) else { return "nothing".into() };
            let shapes = [LineShape::Line, LineShape::RightAngle, LineShape::Curve];
            let shape = rng.pick(&shapes).expect("shapes");
            let what = name(line.id);
            ws.set_line_shape(shape, vec![(line.id, scene::restyle(line, shape))]);
            format!("make line {what:?} {shape:?} {where_}")
        }
        8 if kind == Some(DiagramKind::Sequence) => {
            let Some(layout) = drawn.sequence.as_ref() else { return "nothing".into() };
            // Only messages are dragged up and down; fragments move like boxes.
            let items: Vec<(Id, Option<Id>)> = view
                .presentations()
                .iter()
                .filter(|p| matches!(p.shape, Shape::Message { .. }))
                .map(|p| (p.id, p.element))
                .collect();
            let Some((p, Some(element))) = rng.pick(&items) else { return "nothing".into() };
            let Some(Shape::Message { y, .. }) = view.get(p).map(|p| &p.shape) else {
                return "nothing".into();
            };
            let y = *y;
            let to = y + rng.offset(150.0);
            let Some(slot) = layout.slot_at(to) else { return "nothing".into() };
            let what = name(p);
            ws.move_to_slot(element, slot);
            format!("move {what:?} from y {y} to {to} {where_}")
        }
        9 if kind == Some(DiagramKind::Sequence) => {
            let gates: Vec<(Id, f32)> = view
                .presentations()
                .iter()
                .filter_map(|p| match p.shape {
                    Shape::Message { gate_x: Some(x), .. } => Some((p.id, x)),
                    _ => None,
                })
                .collect();
            if let Some((message, x)) = rng.pick(&gates) {
                let to = x + rng.offset(200.0);
                let what = name(message);
                ws.set_gate(message, to);
                return format!("drag the gate of {what:?} from x {x} to {to} {where_}");
            }
            let lanes: Vec<(Id, f32)> = view
                .presentations()
                .iter()
                .filter_map(|p| match p.shape {
                    Shape::Lifeline { head, length, .. } => Some((p.id, head.bottom() + length)),
                    _ => None,
                })
                .collect();
            let Some((lane, bottom)) = rng.pick(&lanes) else { return "nothing".into() };
            let to = bottom + rng.offset(200.0);
            let what = name(lane);
            ws.set_lifeline_end(lane, to);
            format!("drag the end of {what:?} from y {bottom} to {to} {where_}")
        }
        10 if !messages.is_empty() => {
            let message = rng.pick(&messages).expect("messages");
            let Some(edge) = drawn.edge(message) else { return "nothing".into() };
            let Some(label) = edge.labels.first() else { return "nothing".into() };
            let to = label.anchor.offset(rng.offset(80.0), rng.offset(80.0));
            let what = name(message);
            ws.place_message_label(message, to);
            format!("drag the label of {what:?} to {to:?} {where_}")
        }
        11 => {
            let states: Vec<(Id, Vec<f32>)> = view
                .presentations()
                .iter()
                .filter_map(|p| match &p.shape {
                    Shape::Node { regions: Some(r), .. } if !r.separators.is_empty() => {
                        Some((p.id, r.separators.clone()))
                    }
                    _ => None,
                })
                .collect();
            if states.is_empty() {
                return "nothing".into();
            }
            let (state, lines) = states[rng.below(states.len())].clone();
            let index = rng.below(lines.len());
            let offset = lines[index] + rng.offset(60.0);
            let what = name(state);
            ws.move_region_line(state, index, offset);
            format!("move region line {index} of {what:?} to {offset} {where_}")
        }
        12 if rng.chance(30) => {
            let ids: Vec<Id> = nodes.iter().map(|&(n, _, _)| n).chain(messages).collect();
            let Some(item) = rng.pick(&ids) else { return "nothing".into() };
            let mut selection = vec![item];
            if rng.chance(30)
                && let Some(other) = rng.pick(&ids)
                && other != item
            {
                selection.push(other);
            }
            let what: Vec<String> = selection.iter().map(|&n| name(n)).collect();
            ws.select(selection);
            ws.delete_selection();
            format!("delete {what:?} {where_}")
        }
        13 if rng.chance(20) => {
            ws.select(Vec::new());
            ws.route_lines();
            format!("route all lines {where_}")
        }
        // Drawing new lines: messages at a height between two lifelines (or one, for found and
        // lost ones and calls to itself), other lines between two nodes.
        14 | 15 => {
            // The diagram's own palette: IE and IDEF1X draw their own lines.
            let Some(props) = props else { return "nothing".into() };
            let tools: Vec<EdgeTool> = palette::diagram_palette(props)
                .into_iter()
                .filter_map(|t| match t {
                    Tool::Edge(e) => Some(e),
                    _ => None,
                })
                .collect();
            let Some(tool) = rng.pick(&tools) else { return "nothing".into() };
            if let Some(layout) = drawn.sequence.as_ref() {
                let lanes: Vec<Id> = layout.lanes.iter().map(|l| l.presentation).collect();
                let (Some(from), Some(to)) = (rng.pick(&lanes), rng.pick(&lanes)) else {
                    return "nothing".into();
                };
                let to = if matches!(tool, EdgeTool::FoundMessage | EdgeTool::LostMessage) {
                    from
                } else {
                    to
                };
                let bottom = layout.slots.iter().map(|s| s.y).fold(100.0, f32::max);
                let y = 60.0 + rng.below(((bottom - 20.0) / 5.0) as usize) as f32 * 5.0;
                let slot = layout.slot_at(y);
                let names = (name(from), name(to));
                ws.connect_message(tool, from, to, slot);
                return format!(
                    "draw {tool:?} from {:?} to {:?} at y {y} {where_}",
                    names.0, names.1
                );
            }
            let (Some((from, _, _)), Some((to, _, _))) = (rng.pick(&nodes), rng.pick(&nodes))
            else {
                return "nothing".into();
            };
            let names = (name(from), name(to));
            ws.connect(tool, from, to);
            format!("draw {tool:?} from {:?} to {:?} {where_}", names.0, names.1)
        }
        16 if kind == Some(DiagramKind::Sequence) => {
            if rng.chance(50) {
                let mut selection: Vec<Id> = Vec::new();
                for _ in 0..1 + rng.below(3) {
                    if let Some(m) = rng.pick(&messages) {
                        selection.push(m);
                    }
                }
                selection.dedup();
                let names: Vec<String> = selection.iter().map(|&m| name(m)).collect();
                ws.select(selection);
                ws.wrap_selection_in_fragment();
                return format!("wrap {names:?} in a fragment {where_}");
            }
            let fragments: Vec<Id> = view
                .presentations()
                .iter()
                .filter(|p| matches!(p.shape, Shape::Fragment { .. }))
                .filter_map(|p| p.element)
                .collect();
            let Some(fragment) = rng.pick(&fragments) else { return "nothing".into() };
            ws.add_operand(fragment);
            format!("add an operand to {fragment} {where_}")
        }
        17 => {
            let mut selection: Vec<Id> = Vec::new();
            for _ in 0..2 + rng.below(2) {
                if let Some((n, _, _)) = rng.pick(&nodes) {
                    selection.push(n);
                }
            }
            selection.dedup();
            let alignments = [
                Alignment::Left,
                Alignment::HorizontalCenter,
                Alignment::Right,
                Alignment::Top,
                Alignment::VerticalCenter,
                Alignment::Bottom,
            ];
            let alignment = rng.pick(&alignments).expect("alignments");
            let names: Vec<String> = selection.iter().map(|&n| name(n)).collect();
            ws.select(selection);
            ws.align_selection(alignment);
            format!("align {names:?} {alignment:?} {where_}")
        }
        // New nodes from the diagram's palette, half of the time on or in a node, where parts,
        // ports, pins, entry and exit points, state invariants and terminations go.
        18 | 19 => {
            let Some(props) = props else { return "nothing".into() };
            let tools: Vec<NodeTool> = palette::diagram_palette(props)
                .into_iter()
                .filter_map(|t| match t {
                    Tool::Node(n) => Some(n),
                    _ => None,
                })
                .collect();
            let Some(tool) = rng.pick(&tools) else { return "nothing".into() };
            let at = match rng.pick(&nodes).filter(|_| rng.chance(60)) {
                Some((_, r, _)) => {
                    let (fx, fy) = (rng.below(5) as f32 / 4.0, rng.below(5) as f32 / 4.0);
                    Point::new(r.x + r.width * fx - 7.0, r.y + r.height * fy - 7.0)
                }
                None => Point::new(rng.below(200) as f32 * 5.0, rng.below(160) as f32 * 5.0),
            };
            ws.create_node(tool, at);
            format!("create {tool:?} at {at:?} {where_}")
        }
        // Copy and paste, as copies or as references, on this diagram or another.
        20 => {
            let mut selection: Vec<Id> = Vec::new();
            for _ in 0..1 + rng.below(3) {
                if let Some((n, _, _)) = rng.pick(&nodes) {
                    selection.push(n);
                }
            }
            selection.dedup();
            let names: Vec<String> = selection.iter().map(|&n| name(n)).collect();
            ws.select(selection);
            let Some((_, clip)) = ws.copy_selection() else { return "nothing".into() };
            let offset = Point::new(rng.offset(100.0), rng.offset(100.0));
            let as_reference = rng.chance(30);
            let mut target = diagram;
            if rng.chance(30)
                && let Some(other) = rng.pick(&diagrams(ws.doc.model()))
            {
                target = other;
                ws.open_diagram(other);
            }
            let onto = ws.doc.model().get(target).map_or("", |d| d.name()).to_string();
            ws.paste(&clip, offset, as_reference);
            format!(
                "paste {names:?} {where_} onto {onto:?} by {offset:?}, as reference: {as_reference}"
            )
        }
        // Showing an element the diagram does not show yet.
        21 => {
            let Some(kind) = kind else { return "nothing".into() };
            let shown: HashSet<Id> =
                view.presentations().iter().filter_map(|p| p.element).collect();
            let mut candidates: Vec<Id> = model
                .elements()
                .filter(|e| palette::can_show(kind, e.kind()) && !shown.contains(&e.id()))
                .map(|e| e.id())
                .collect();
            candidates.sort();
            let Some(element) = rng.pick(&candidates) else { return "nothing".into() };
            let at = Point::new(rng.below(200) as f32 * 5.0, rng.below(160) as f32 * 5.0);
            let what = model.get(element).map_or("", |e| e.name()).to_string();
            ws.show_element(element, at);
            format!("show {what:?} at {at:?} {where_}")
        }
        // Member text typed in place, among it parameters named like directions; what the box
        // then offers for editing must give back the same member.
        22 => {
            let shown: HashSet<Id> =
                view.presentations().iter().filter_map(|p| p.element).collect();
            let mut members: Vec<Id> = model
                .elements()
                .filter(|e| {
                    matches!(e.kind(), ElementKind::Attribute(_) | ElementKind::Operation(_))
                })
                .filter(|e| e.owner().is_some_and(|o| shown.contains(&o)))
                .map(|e| e.id())
                .collect();
            members.sort();
            let Some(member) = rng.pick(&members) else { return "nothing".into() };
            let operation =
                matches!(model.get(member).map(|e| e.kind()), Some(ElementKind::Operation(_)));
            let current = member::member_text(model, member);
            let texts: &[&str] = if operation {
                &[
                    "+ load(in : InputStream, out : OutputStream) : void",
                    "# copy(inout buffer : byte[], out count : int = 0) : boolean",
                    "- out(in in : int, out out : int)",
                ]
            } else {
                &["- out : int = 1", "+ in : String", "# inout : double[]"]
            };
            let text = match rng.below(3) {
                0 => current.replacen('(', "2(", 1).replacen(" :", "2 :", usize::from(!operation)),
                _ => rng.pick(texts).expect("texts").to_string(),
            };
            ws.apply_text(member, &text);
            let shown = member::member_text(ws.doc.model(), member);
            let mut again = ws.doc.clone();
            let same = again
                .edit("again", |tx| member::apply_member_text(tx, member, &shown))
                .is_ok_and(|()| again.model() == ws.doc.model());
            assert!(same, "{shown:?} does not give back the member it shows");
            format!("type {text:?} for {current:?} {where_}")
        }
        // Renaming what a node shows: boxes sized to their text grow or shrink.
        23 => {
            let named: Vec<Id> =
                nodes.iter().filter_map(|&(n, _, _)| view.get(n).and_then(|p| p.element)).collect();
            let Some(element) = rng.pick(&named) else { return "nothing".into() };
            let old = model.get(element).map_or(String::new(), |e| e.name().to_string());
            let new = if rng.chance(50) {
                format!("{old} with a much longer name than before")
            } else {
                old.chars().take(1).collect()
            };
            if new.is_empty() {
                return "nothing".into();
            }
            ws.update_element(element, |d| d.name = new.clone());
            format!("rename {old:?} to {new:?} {where_}")
        }
        // Regions of states, and operands of fragments, added and deleted.
        24 => {
            let states: Vec<Id> = view
                .presentations()
                .iter()
                .filter_map(|p| p.element)
                .filter(|&e| matches!(model.get(e).map(|e| e.kind()), Some(ElementKind::State(_))))
                .collect();
            let operands: Vec<Id> = view
                .presentations()
                .iter()
                .filter(|p| matches!(&p.shape, Shape::Fragment { separators, .. } if !separators.is_empty()))
                .filter_map(|p| p.element)
                .flat_map(|f| model.children(f).to_vec())
                .filter(|&o| matches!(model.get(o).map(|e| e.kind()), Some(ElementKind::InteractionOperand(_))))
                .collect();
            if let Some(operand) = rng.pick(&operands).filter(|_| rng.chance(50)) {
                ws.delete_element(operand);
                return format!("delete operand {operand} {where_}");
            }
            let Some(state) = rng.pick(&states) else { return "nothing".into() };
            let what = model.get(state).map_or("", |e| e.name()).to_string();
            let regions = state_edit::regions_of(model, state);
            if let Some(region) = rng.pick(&regions).filter(|_| rng.chance(30)) {
                ws.delete_element(region);
                return format!("delete a region of {what:?} {where_}");
            }
            let vertical = rng.chance(50);
            ws.add_region(state, vertical);
            format!("add a region to {what:?}, vertical: {vertical} {where_}")
        }
        // ER diagrams: another notation, or a relationship made identifying or not.
        25 if kind == Some(DiagramKind::EntityRelationship) => {
            if rng.chance(40) {
                let notations = [ErNotation::Chen, ErNotation::Ie, ErNotation::Idef1x];
                let notation = rng.pick(&notations).expect("notations");
                ws.set_er_notation(diagram, notation);
                return format!("draw {where_} in {notation:?}");
            }
            let mut relationships: Vec<(Id, bool)> = model
                .elements()
                .filter_map(|e| match e.kind() {
                    ElementKind::ErRelationship(p) => Some((e.id(), p.identifying)),
                    _ => None,
                })
                .collect();
            relationships.sort();
            let Some((relationship, identifying)) = rng.pick(&relationships) else {
                return "nothing".into();
            };
            ws.set_er_identifying(relationship, !identifying);
            format!("make {relationship} identifying: {} {where_}", !identifying)
        }
        // Removing nodes and lines from the diagram only.
        26 => {
            let ids: Vec<Id> = nodes
                .iter()
                .map(|&(n, _, _)| n)
                .chain(messages)
                .chain(lines.iter().map(|l| l.id))
                .collect();
            let mut selection = Vec::new();
            for _ in 0..1 + rng.below(2) {
                if let Some(item) = rng.pick(&ids) {
                    selection.push(item);
                }
            }
            selection.dedup();
            let names: Vec<String> = selection.iter().map(|&n| name(n)).collect();
            ws.select(selection);
            ws.remove_selection_from_diagram();
            format!("remove {names:?} from {where_}")
        }
        // "Change kind" of a relationship.
        27 => {
            let Some(line) = rng.pick(&lines) else { return "nothing".into() };
            let Some((_, kinds)) = palette::relationship_kinds(model, diagram, line.id) else {
                return "nothing".into();
            };
            let offered: Vec<EdgeTool> = kinds.into_iter().filter(|k| k.1).map(|k| k.0).collect();
            let Some(tool) = rng.pick(&offered) else { return "nothing".into() };
            let what = name(line.id);
            ws.change_relationship(line.id, tool);
            format!("make {what:?} a {tool:?} {where_}")
        }
        _ => "nothing".into(),
    }
}

/// Runs `steps` random edits on each document from `seed` on, and returns what went wrong.
fn run(seed: u64, steps: usize) -> Vec<String> {
    let mut problems = Vec::new();
    for (doc_name, doc) in documents() {
        let mut ws = Workspace::new(doc, None);
        let mut rng = Rng::new(seed);
        let mut rules = invariants::check(ws.doc.model(), &ApproxMeasure);
        for step in 0..steps {
            let at = |what: &str| format!("{doc_name}, seed {seed}, step {step} ({what})");
            // Now and then back and forth through the history.
            if rng.chance(8) {
                let redo = rng.chance(40);
                if redo {
                    ws.doc.redo()
                } else {
                    ws.doc.undo()
                };
                ws.validate();
                rules = invariants::check(ws.doc.model(), &ApproxMeasure);
                continue;
            }
            let before: Model = ws.doc.model().clone();
            let revision = ws.doc.revision();
            let done = catch_unwind(AssertUnwindSafe(|| edit(&mut ws, &mut rng)));
            let what = match done {
                Ok(what) => what,
                Err(panic) => {
                    let text = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    problems.push(format!("{}: panicked: {text}", at("an edit")));
                    break;
                }
            };
            if ws.doc.revision() == revision {
                continue;
            }
            let now = invariants::check(ws.doc.model(), &ApproxMeasure);
            let broken = invariants::new_violations(&rules, &now);
            // `UMLCRAFT_FUZZ_DUMP` names a folder for the projects before and after a step that
            // broke a rule, to look at them in UmlCraft.
            if let (false, Some(dir)) = (broken.is_empty(), std::env::var_os("UMLCRAFT_FUZZ_DUMP"))
            {
                let base = std::path::Path::new(&dir).join(format!("{doc_name}-{seed}-{step}"));
                for (suffix, model) in [("before", &before), ("after", ws.doc.model())] {
                    let path = base.with_extension(format!("{suffix}.ucraft"));
                    std::fs::write(path, umlcraft_format::to_string(model)).expect("dump");
                }
            }
            for rule in broken {
                problems.push(format!("{}: {rule}", at(&what)));
            }
            rules = now;
            // Undo gives back the model before, redo the one after.
            let after = ws.doc.model().clone();
            ws.doc.undo();
            if *ws.doc.model() != before {
                problems.push(format!("{}: undo does not restore the model", at(&what)));
            }
            ws.doc.redo();
            if *ws.doc.model() != after {
                problems.push(format!("{}: redo does not restore the edit", at(&what)));
            }
            ws.validate();
        }
    }
    problems
}

#[test]
fn random_edits_keep_the_rules() {
    let var = |name: &str| std::env::var(name).ok().and_then(|v| v.parse().ok());
    let steps = var("UMLCRAFT_FUZZ_STEPS").unwrap_or(150) as usize;
    let seeds: Vec<u64> = match var("UMLCRAFT_FUZZ_SEED") {
        Some(seed) => vec![seed],
        None => (1..=3).collect(),
    };
    let problems: Vec<String> = seeds.into_iter().flat_map(|seed| run(seed, steps)).collect();
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn the_dice_are_fair_enough() {
    let mut rng = Rng::new(1);
    let mut seen = [0usize; 14];
    for _ in 0..1400 {
        seen[rng.below(14)] += 1;
    }
    assert!(seen.iter().all(|&n| n > 50), "{seen:?}");
    let offsets: Vec<f32> = (0..200).map(|_| rng.offset(150.0)).collect();
    assert!(offsets.iter().all(|o| o.abs() <= 150.0 && o % 5.0 == 0.0));
    assert!(offsets.iter().any(|&o| o < 0.0) && offsets.iter().any(|&o| o > 0.0));
}
