//! Automatic placement of class-like diagrams (class, object, package, component and
//! deployment diagrams), after Sugiyama et al.: the nodes go into layers so that generals sit
//! above their specifics and wholes above their parts, the order within each layer is chosen
//! for few crossings (barycenters), and each node is moved as close to the middle of its
//! neighbors as its layer allows. Parts of the diagram that nothing connects are laid out one
//! by one and packed into rows. Packages, components and nodes with something drawn inside
//! are laid out inside first and then placed as one box.
//!
//! [`layered`] works on sizes and edges alone; [`arrange`] reads a diagram, moves its nodes
//! and routes its lines ([`crate::autoroute`]).

use std::collections::{HashMap, HashSet, VecDeque};

use umlcraft_model::{
    ActivityNodeKind, ActivityNodeProps, Aggregation, DiagramKind, Element, ElementKind, Id, Model,
    ModelError, Navigability, Point, PseudostateKind, Rect, Regions, Result, Shape, Size, Tx,
};

use crate::scene::{self, TextMeasure};

/// Which way the layers run: generals above specifics, or to their left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Down,
    Right,
}

/// An edge between two nodes (indices): `upper` should go into an earlier layer than `lower`.
/// Hierarchy edges (generalization, realization, composition) keep that order; others give
/// way where the order would make a cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub upper: usize,
    pub lower: usize,
    pub hierarchy: bool,
}

/// Space between neighbors in a layer.
const NODE_GAP: f32 = 60.0;
/// Space next to the points where long edges pass a layer.
const DUMMY_GAP: f32 = 16.0;
/// Space between layers, and more for each edge between them (up to `MAX_EDGE_ROOM` of them),
/// so that their lines have room to turn.
const LAYER_GAP: f32 = 60.0;
const EDGE_ROOM: f32 = 8.0;
const MAX_EDGE_ROOM: usize = 6;
/// Space between parts of the diagram packed into rows.
const PART_GAP: f32 = 60.0;
/// Room inside a container around what it holds, and above it for the name.
const PADDING: f32 = 20.0;
const HEADER: f32 = 40.0;
/// How often the order of the layers and the places in them are improved.
const ORDER_SWEEPS: usize = 24;
/// The narrowest partition and the smallest region the layout makes.
const MIN_LANE: f32 = 120.0;
const EMPTY_REGION: f32 = 40.0;
/// How far fork and join bars reach beyond the middle of the outermost node they join.
const BAR_REACH: f32 = 40.0;
/// Space between actors one below the other.
const ACTOR_GAP: f32 = 20.0;
const PLACE_SWEEPS: usize = 12;

/// Places nodes of the given `sizes`: returns the top left corner of each, the whole starting
/// at (0, 0).
pub fn layered(sizes: &[Size], edges: &[Edge], direction: Direction) -> Vec<Point> {
    let n = sizes.len();
    // Parts that nothing connects, each laid out on its own.
    let mut part = (0..n).collect::<Vec<_>>();
    fn find(part: &mut [usize], i: usize) -> usize {
        let mut root = i;
        while part[root] != root {
            root = part[root];
        }
        part[i] = root;
        root
    }
    for e in edges {
        let (a, b) = (find(&mut part, e.upper), find(&mut part, e.lower));
        if a != b {
            part[a.max(b)] = a.min(b);
        }
    }
    let mut parts: Vec<Vec<usize>> = Vec::new();
    let mut part_of: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        let root = find(&mut part, i);
        let index = *part_of.entry(root).or_insert_with(|| {
            parts.push(Vec::new());
            parts.len() - 1
        });
        parts[index].push(i);
    }
    let mut placed: Vec<(Vec<usize>, Vec<Point>, Size)> = parts
        .into_iter()
        .map(|nodes| {
            let local: HashMap<usize, usize> =
                nodes.iter().enumerate().map(|(l, &g)| (g, l)).collect();
            let part_edges: Vec<Edge> = edges
                .iter()
                .filter(|e| e.upper != e.lower && local.contains_key(&e.upper))
                .map(|e| Edge { upper: local[&e.upper], lower: local[&e.lower], ..*e })
                .collect();
            let part_sizes: Vec<Size> = nodes.iter().map(|&g| sizes[g]).collect();
            let (points, size) = layered_part(&part_sizes, &part_edges, direction);
            (nodes, points, size)
        })
        .collect();
    // Larger parts first, lone nodes last; ties in the order of the nodes.
    placed.sort_by(|a, b| {
        let area = |s: &Size| s.width * s.height;
        (a.0.len() == 1)
            .cmp(&(b.0.len() == 1))
            .then(area(&b.2).total_cmp(&area(&a.2)))
            .then(a.0[0].cmp(&b.0[0]))
    });
    let total: f32 = placed.iter().map(|p| (p.2.width + PART_GAP) * (p.2.height + PART_GAP)).sum();
    let widest = placed.iter().map(|p| p.2.width).fold(0.0, f32::max);
    let row_width = widest.max(total.sqrt() * 1.4);
    let mut result = vec![Point::new(0.0, 0.0); n];
    let (mut x, mut y, mut row_height) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (nodes, points, size) in placed {
        if x > 0.0 && x + size.width > row_width {
            (x, y, row_height) = (0.0, y + row_height + PART_GAP, 0.0);
        }
        for (&g, p) in nodes.iter().zip(points) {
            result[g] = p.offset(x, y);
        }
        x += size.width + PART_GAP;
        row_height = row_height.max(size.height);
    }
    result
}

/// Whether `from` reaches `to` along `next`.
fn reaches(next: &[Vec<usize>], from: usize, to: usize) -> bool {
    let mut seen = vec![false; next.len()];
    let mut stack = vec![from];
    while let Some(v) = stack.pop() {
        if v == to {
            return true;
        }
        if !std::mem::replace(&mut seen[v], true) {
            stack.extend(&next[v]);
        }
    }
    false
}

/// Lays out one connected part: positions of its nodes and the size of the whole.
fn layered_part(sizes: &[Size], edges: &[Edge], direction: Direction) -> (Vec<Point>, Size) {
    let n = sizes.len();
    // Across the layers (`major`) and along them (`minor`).
    let (major, minor): (Vec<f32>, Vec<f32>) = sizes
        .iter()
        .map(|s| match direction {
            Direction::Down => (s.height, s.width),
            Direction::Right => (s.width, s.height),
        })
        .unzip();

    // 1. Directions without cycles: hierarchy edges first, then the others where they fit.
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut ordered: Vec<&Edge> = edges.iter().filter(|e| e.hierarchy).collect();
    ordered.extend(edges.iter().filter(|e| !e.hierarchy));
    for e in ordered {
        let (u, v) =
            if reaches(&next, e.lower, e.upper) { (e.lower, e.upper) } else { (e.upper, e.lower) };
        if !next[u].contains(&v) {
            next[u].push(v);
        }
    }
    let mut previous: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (u, vs) in next.iter().enumerate() {
        for &v in vs {
            previous[v].push(u);
        }
    }

    // 2. Layers: the longest path from the top, then nodes without anything above them move
    // down next to what they point at.
    let mut indegree: Vec<usize> = previous.iter().map(Vec::len).collect();
    let mut queue: VecDeque<usize> = (0..n).filter(|&v| indegree[v] == 0).collect();
    let mut topological = Vec::with_capacity(n);
    while let Some(v) = queue.pop_front() {
        topological.push(v);
        for &w in &next[v] {
            indegree[w] -= 1;
            if indegree[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    let mut layer = vec![0usize; n];
    for &v in &topological {
        for &w in &next[v] {
            layer[w] = layer[w].max(layer[v] + 1);
        }
    }
    for &v in topological.iter().rev() {
        if previous[v].is_empty()
            && let Some(lowest) = next[v].iter().map(|&w| layer[w]).min()
        {
            layer[v] = lowest.saturating_sub(1).max(layer[v]);
        }
    }
    let layers_count = layer.iter().max().map_or(0, |m| m + 1);

    // 3. Points where edges pass layers they skip; they take part in ordering and placing.
    let mut width: Vec<f32> = minor.clone();
    let mut node_layer = layer.clone();
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut between = vec![0usize; layers_count];
    for (u, vs) in next.iter().enumerate() {
        for &v in vs {
            let mut from = u;
            for l in layer[u] + 1..layer[v] {
                let dummy = width.len();
                width.push(0.0);
                node_layer.push(l);
                up.push(vec![from]);
                down.push(Vec::new());
                down[from].push(dummy);
                from = dummy;
            }
            down[from].push(v);
            up[v].push(from);
            between[layer[u]] += 1;
        }
    }
    let total = width.len();
    let real = |v: usize| v < n;

    // 4. Order within the layers: first as met going down from the top, then by barycenters.
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); layers_count];
    let mut seen = vec![false; total];
    let mut queue: VecDeque<usize> = (0..total).filter(|&v| up[v].is_empty()).collect();
    while let Some(v) = queue.pop_front() {
        if std::mem::replace(&mut seen[v], true) {
            continue;
        }
        layers[node_layer[v]].push(v);
        queue.extend(down[v].iter().copied());
    }
    let crossings = |layers: &[Vec<usize>]| -> usize {
        let mut position = vec![0usize; total];
        for l in layers {
            for (i, &v) in l.iter().enumerate() {
                position[v] = i;
            }
        }
        let mut count = 0;
        for l in layers {
            let pairs: Vec<(usize, usize)> = l
                .iter()
                .flat_map(|&u| down[u].iter().map(move |&v| (u, v)))
                .map(|(u, v)| (position[u], position[v]))
                .collect();
            for (i, a) in pairs.iter().enumerate() {
                for b in &pairs[i + 1..] {
                    if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                        count += 1;
                    }
                }
            }
        }
        count
    };
    let mut best = layers.clone();
    let mut fewest = crossings(&layers);
    for sweep in 0..ORDER_SWEEPS {
        let downward = sweep % 2 == 0;
        let order: Vec<usize> = if downward {
            (1..layers_count).collect()
        } else {
            (0..layers_count.saturating_sub(1)).rev().collect()
        };
        for l in order {
            let reference = if downward { l - 1 } else { l + 1 };
            let position: HashMap<usize, f32> =
                layers[reference].iter().enumerate().map(|(i, &v)| (v, i as f32)).collect();
            let mut keyed: Vec<(f32, usize)> = layers[l]
                .iter()
                .enumerate()
                .map(|(i, &v)| {
                    let neighbors = if downward { &up[v] } else { &down[v] };
                    let at: Vec<f32> =
                        neighbors.iter().filter_map(|w| position.get(w).copied()).collect();
                    let key = if at.is_empty() {
                        i as f32
                    } else {
                        at.iter().sum::<f32>() / at.len() as f32
                    };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
            layers[l] = keyed.into_iter().map(|(_, v)| v).collect();
        }
        let count = crossings(&layers);
        if count < fewest {
            (fewest, best) = (count, layers.clone());
        }
    }
    let layers = best;

    // 5. Places along the layers: each node as close to the middle of its neighbors as the
    // order and the gaps allow (isotonic regression, pool adjacent violators).
    let gap = |a: usize, b: usize| if real(a) && real(b) { NODE_GAP } else { DUMMY_GAP };
    let mut center = vec![0.0f32; total];
    for l in &layers {
        let mut at = 0.0;
        for (i, &v) in l.iter().enumerate() {
            if i > 0 {
                at += gap(l[i - 1], v);
            }
            center[v] = at + width[v] / 2.0;
            at += width[v];
        }
    }
    let place = |center: &mut Vec<f32>, l: &[usize], neighbors: &dyn Fn(usize) -> Vec<usize>| {
        if l.is_empty() {
            return;
        }
        let mut offsets = vec![0.0f32; l.len()];
        for i in 1..l.len() {
            offsets[i] =
                offsets[i - 1] + (width[l[i - 1]] + width[l[i]]) / 2.0 + gap(l[i - 1], l[i]);
        }
        let wanted: Vec<f32> = l
            .iter()
            .map(|&v| {
                let ns = neighbors(v);
                if ns.is_empty() {
                    center[v]
                } else {
                    ns.iter().map(|&w| center[w]).sum::<f32>() / ns.len() as f32
                }
            })
            .collect();
        // Blocks of (sum of weighted targets, weight, count), each block one level.
        let mut blocks: Vec<(f32, f32, usize)> = Vec::new();
        for (i, &v) in l.iter().enumerate() {
            let weight = if real(v) { 1.0 } else { 0.5 };
            blocks.push(((wanted[i] - offsets[i]) * weight, weight, 1));
            while blocks.len() >= 2 {
                let (b, a) = (blocks[blocks.len() - 1], blocks[blocks.len() - 2]);
                if a.0 / a.1 <= b.0 / b.1 {
                    break;
                }
                blocks.pop();
                *blocks.last_mut().expect("two blocks") = (a.0 + b.0, a.1 + b.1, a.2 + b.2);
            }
        }
        let mut i = 0;
        for (sum, weight, count) in blocks {
            for _ in 0..count {
                center[l[i]] = sum / weight + offsets[i];
                i += 1;
            }
        }
    };
    for sweep in 0..PLACE_SWEEPS {
        let downward = sweep % 2 == 0;
        let order: Vec<usize> =
            if downward { (0..layers_count).collect() } else { (0..layers_count).rev().collect() };
        for l in order {
            let neighbors = |v: usize| if downward { up[v].clone() } else { down[v].clone() };
            place(&mut center, &layers[l], &neighbors);
        }
    }
    // Last, from the bottom up, so that generals and wholes end up centered over theirs.
    for l in layers.iter().rev() {
        place(&mut center, l, &|v: usize| down[v].clone());
    }
    let left = (0..total).map(|v| center[v] - width[v] / 2.0).fold(f32::INFINITY, f32::min);

    // 6. Layers one after the other, as deep as their deepest node.
    let mut layer_start = vec![0.0f32; layers_count];
    let mut at = 0.0;
    for l in 0..layers_count {
        layer_start[l] = at;
        let depth = (0..n).filter(|&v| layer[v] == l).map(|v| major[v]).fold(0.0, f32::max);
        at += depth + LAYER_GAP + EDGE_ROOM * between[l].min(MAX_EDGE_ROOM) as f32;
    }
    let mut points = Vec::with_capacity(n);
    let (mut right, mut bottom) = (0.0f32, 0.0f32);
    for v in 0..n {
        let along = center[v] - width[v] / 2.0 - left;
        let across = layer_start[layer[v]];
        right = right.max(along + minor[v]);
        bottom = bottom.max(across + major[v]);
        points.push(match direction {
            Direction::Down => Point::new(along, across),
            Direction::Right => Point::new(across, along),
        });
    }
    let size = match direction {
        Direction::Down => Size::new(right, bottom),
        Direction::Right => Size::new(bottom, right),
    };
    (points, size)
}

/// The diagram kinds [`arrange`] lays out. Sequence diagrams have a layout of their own;
/// composite structure diagrams keep parts in their class and ports on borders, which the
/// layers do not know.
pub fn can_arrange(kind: DiagramKind) -> bool {
    !matches!(kind, DiagramKind::Sequence | DiagramKind::CompositeStructure)
}

/// How an edge between two shown elements orders them: (the source goes above, hierarchy).
fn order_of(model: &Model, element: Option<&Element>, source: Option<Id>) -> (bool, bool) {
    let Some(element) = element else { return (true, false) };
    let is = |id: Id| source == Some(id);
    match element.kind() {
        ElementKind::Generalization(p) => (is(p.general), true),
        ElementKind::Realization(p) => (is(p.supplier), true),
        ElementKind::Association(p) => {
            match p.ends.iter().position(|e| e.aggregation != Aggregation::None) {
                Some(whole) => (is(p.ends[whole].participant), true),
                None => (true, false),
            }
        }
        ElementKind::Link(p) => {
            match p.ends.iter().position(|e| e.aggregation != Aggregation::None) {
                Some(whole) => (is(p.ends[whole].instance), true),
                None => (true, false),
            }
        }
        ElementKind::Dependency(p) => (is(p.supplier) || model.get(p.supplier).is_none(), false),
        // Flows and transitions run from layer to layer.
        ElementKind::ActivityEdge(p) => (is(p.source), true),
        ElementKind::Transition(p) => (is(p.source), true),
        // The base use case before what it includes, the extended one before its extension.
        ElementKind::Include(p) => (is(p.including), true),
        ElementKind::Extend(p) => (is(p.extended), true),
        _ => (true, false),
    }
}

/// How an association between an actor and a use case orders them: the actor first, unless
/// only the actor's end is navigable (the system calls on it: a secondary actor, which goes
/// after the use cases). `None` for other associations.
fn actor_first(model: &Model, element: &Element, source: Option<Id>) -> Option<bool> {
    let ElementKind::Association(p) = element.kind() else { return None };
    let actor = p.ends.iter().position(|e| {
        model.get(e.participant).is_some_and(|e| matches!(e.kind(), ElementKind::Actor(_)))
    })?;
    let other = 1 - actor;
    if !model
        .get(p.ends[other].participant)
        .is_some_and(|e| matches!(e.kind(), ElementKind::UseCase(_)))
    {
        return None;
    }
    let secondary = p.ends[actor].navigability == Navigability::Navigable
        && p.ends[other].navigability != Navigability::Navigable;
    let source_is_actor = source == Some(p.ends[actor].participant);
    Some(source_is_actor != secondary)
}

/// Initial nodes of activities and initial pseudostates of state machines.
fn is_start(kind: &ElementKind) -> bool {
    match kind {
        ElementKind::ActivityNode(p) => p.node == ActivityNodeKind::Initial,
        ElementKind::Pseudostate(p) => p.pseudostate == PseudostateKind::Initial,
        _ => false,
    }
}

/// For each of `n` nodes, when a breadth-first walk along the edges (`(source, target,
/// source first, _)`) meets it: from `starts` first, then from the nodes nothing leads to, then
/// from the rest in order.
fn discovery(n: usize, edges: &[(usize, usize, bool, bool)], starts: &[usize]) -> Vec<usize> {
    let mut next: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut entered = vec![false; n];
    for &(s, t, above, _) in edges {
        let (u, v) = if above { (s, t) } else { (t, s) };
        next[u].push(v);
        entered[v] = true;
    }
    let mut order = vec![usize::MAX; n];
    let mut count = 0;
    let roots = starts.iter().copied().chain((0..n).filter(|&i| !entered[i])).chain(0..n);
    for root in roots {
        if order[root] != usize::MAX {
            continue;
        }
        order[root] = count;
        count += 1;
        let mut queue = VecDeque::from([root]);
        while let Some(u) = queue.pop_front() {
            for &v in &next[u] {
                if order[v] == usize::MAX {
                    order[v] = count;
                    count += 1;
                    queue.push_back(v);
                }
            }
        }
    }
    order
}

/// The direction [`arrange`] lays out a kind of diagram in unless told otherwise: state
/// machines and use case diagrams from left to right, the others from the top down.
pub fn default_direction(kind: DiagramKind) -> Direction {
    match kind {
        DiagramKind::StateMachine | DiagramKind::UseCase => Direction::Right,
        _ => Direction::Down,
    }
}

/// Arranges the nodes of a class, component or deployment diagram, or with `scope` only those
/// nodes (and what they hold), and routes the lines of what moved. A scope keeps its place
/// (the top left of what it covered); nodes in the way of what it grew into are moved right
/// or down. Returns the nodes that moved.
pub fn arrange(
    tx: &mut Tx<'_>,
    diagram: Id,
    scope: Option<&[Id]>,
    direction: Direction,
    measure: &dyn TextMeasure,
) -> Result<Vec<Id>> {
    let model = tx.model();
    let kind =
        model.element(diagram)?.kind().as_diagram().ok_or(ModelError::NotADiagram(diagram))?;
    if !can_arrange(kind) {
        return Err(ModelError::Invalid("sequence diagrams are not arranged".into()));
    }
    let view = model.view(diagram).ok_or(ModelError::NotADiagram(diagram))?;

    // The nodes, their sizes and what holds them.
    struct Node {
        id: Id,
        bounds: Rect,
        size: Size,
        container: bool,
        name_width: f32,
        element: Option<Id>,
        /// An orthogonal state's regions as drawn.
        regions: Option<Regions>,
        partition: bool,
        /// A fork or join bar.
        bar: bool,
    }
    let mut nodes: Vec<Node> = Vec::new();
    for p in view.presentations() {
        let Shape::Node { bounds, .. } = p.shape else { continue };
        let element = p.element.and_then(|e| model.get(e));
        let kind = element.map(|e| e.kind());
        let lollipop = p.style.iconic && matches!(kind, Some(ElementKind::Interface(_)));
        let size = if lollipop {
            // The circle and the name below it.
            let name = element.map_or(0.0, |e| measure.width(e.name(), scene::FONT_SIZE, true));
            Size::new(name.max(24.0), 24.0 + scene::line_height(scene::FONT_SIZE))
        } else {
            scene::fitted_size(model, p, measure).unwrap_or(bounds.size())
        };
        let name_width = element.map_or(0.0, |e| measure.width(e.name(), scene::FONT_SIZE, true));
        nodes.push(Node {
            id: p.id,
            bounds,
            size,
            container: kind.is_some_and(crate::palette::is_container),
            name_width,
            element: p.element,
            regions: p.regions().cloned(),
            partition: matches!(kind, Some(ElementKind::Partition)),
            bar: matches!(
                kind,
                Some(ElementKind::ActivityNode(ActivityNodeProps {
                    node: ActivityNodeKind::Fork | ActivityNodeKind::Join,
                    ..
                }))
            ),
        });
    }
    let index: HashMap<Id, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    // Each node's container: the smallest one drawn around it.
    let parent: Vec<Option<usize>> = (0..nodes.len())
        .map(|i| {
            (0..nodes.len())
                .filter(|&c| {
                    c != i
                        && nodes[c].container
                        && nodes[c].bounds.contains_rect(&nodes[i].bounds)
                        && nodes[c].bounds != nodes[i].bounds
                })
                .min_by(|&a, &b| {
                    let area = |r: Rect| r.width * r.height;
                    area(nodes[a].bounds).total_cmp(&area(nodes[b].bounds))
                })
        })
        .collect();
    let ancestors = |mut i: usize| {
        let mut chain = vec![i];
        while let Some(p) = parent[i] {
            chain.push(p);
            i = p;
        }
        chain
    };

    // What to lay out: the scope's nodes that nothing else in it holds, and what they hold.
    let all: Vec<usize> = (0..nodes.len()).collect();
    let roots: Vec<usize> = match scope {
        None => all.iter().copied().filter(|&i| parent[i].is_none()).collect(),
        Some(ids) => {
            let picked: HashSet<usize> =
                ids.iter().filter_map(|id| index.get(id).copied()).collect();
            picked
                .iter()
                .copied()
                .filter(|&i| !ancestors(i)[1..].iter().any(|a| picked.contains(a)))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect()
        }
    };
    if roots.is_empty() {
        return Ok(Vec::new());
    }
    let in_scope: HashSet<usize> =
        all.iter().copied().filter(|&i| ancestors(i).iter().any(|a| roots.contains(a))).collect();

    // The edges between nodes in scope.
    let mut edges: Vec<(usize, usize, bool, bool)> = Vec::new();
    for p in view.presentations() {
        let Shape::Edge { source, target, .. } = &p.shape else { continue };
        let (Some(&s), Some(&t)) = (index.get(source), index.get(target)) else { continue };
        if s == t || !in_scope.contains(&s) || !in_scope.contains(&t) {
            continue;
        }
        let element = p.element.and_then(|e| model.get(e));
        let source_element = view.get(*source).and_then(|p| p.element);
        let (source_above, hierarchy) =
            match element.and_then(|e| actor_first(model, e, source_element)) {
                Some(first) => (first, false),
                None => order_of(model, element, source_element),
            };
        edges.push((s, t, source_above, hierarchy));
    }
    // Where flows and transitions make cycles, the edges met first going out from the start
    // keep their direction and the ones that close a cycle give way.
    if matches!(kind, DiagramKind::Activity | DiagramKind::StateMachine) {
        let starts: Vec<usize> = (0..nodes.len())
            .filter(|&i| {
                let element = view.get(nodes[i].id).and_then(|p| p.element);
                element.and_then(|e| model.get(e)).is_some_and(|e| is_start(e.kind()))
            })
            .collect();
        let order = discovery(nodes.len(), &edges, &starts);
        edges.sort_by_key(|&(s, t, above, _)| order[if above { s } else { t }]);
    }

    // Activities: the flows give the rows, partitions stay columns side by side and each node
    // stays in its own.
    let partitions: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].partition).collect();
    let lanes: Option<Vec<(usize, Rect)>> = if kind == DiagramKind::Activity && scope.is_none() {
        let leaves: Vec<usize> = {
            let mut leaves: Vec<usize> = partitions
                .iter()
                .copied()
                .filter(|&q| !partitions.iter().any(|&r| parent[r] == Some(q)))
                .collect();
            leaves.sort_by(|&a, &b| nodes[a].bounds.x.total_cmp(&nodes[b].bounds.x));
            leaves
        };
        let side_by_side = leaves.windows(2).all(|w| {
            let (a, b) = (nodes[w[0]].bounds, nodes[w[1]].bounds);
            a.right().min(b.right()) - b.x <= a.width.min(b.width) / 2.0
        });
        if !side_by_side {
            return Err(ModelError::Invalid("partitions one below the other".into()));
        }
        let movable: Vec<usize> = (0..nodes.len()).filter(|&i| !nodes[i].partition).collect();
        let local: HashMap<usize, usize> =
            movable.iter().enumerate().map(|(l, &g)| (g, l)).collect();
        let flows: Vec<Edge> = edges
            .iter()
            .filter_map(|&(s, t, above, hierarchy)| {
                let (a, b) = (*local.get(&s)?, *local.get(&t)?);
                let (upper, lower) = if above { (a, b) } else { (b, a) };
                Some(Edge { upper, lower, hierarchy })
            })
            .collect();
        // Fork and join bars get as wide as what they join after the others are placed; they
        // may reach across partitions.
        let sizes: Vec<Size> = movable
            .iter()
            .map(|&i| {
                if nodes[i].bar {
                    Size::new(BAR_REACH, nodes[i].size.height)
                } else {
                    nodes[i].size
                }
            })
            .collect();
        let points = layered(&sizes, &flows, Direction::Down);
        // Rows from the layers (nodes of a layer share their top), columns from the partitions.
        let mut tops: Vec<f32> = points.iter().map(|p| p.y).collect();
        tops.sort_by(f32::total_cmp);
        tops.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let row_of =
            |l: usize| tops.iter().position(|t| (t - points[l].y).abs() < 0.5).unwrap_or(0);
        let lane_of = |i: usize| parent[i].and_then(|p| leaves.iter().position(|&q| q == p));
        // Column 0 holds what lies in no partition, the partitions follow.
        let columns = leaves.len() + 1;
        let mut cells: Vec<Vec<Vec<usize>>> = vec![vec![Vec::new(); tops.len()]; columns];
        let mut row_depth = vec![0.0f32; tops.len()];
        let mut bars: Vec<usize> = Vec::new();
        for (l, &i) in movable.iter().enumerate() {
            row_depth[row_of(l)] = row_depth[row_of(l)].max(sizes[l].height);
            if nodes[i].bar {
                bars.push(l);
                continue;
            }
            let column = lane_of(i).map_or(0, |k| k + 1);
            cells[column][row_of(l)].push(l);
        }
        for column in &mut cells {
            for cell in column.iter_mut() {
                cell.sort_by(|&a, &b| points[a].x.total_cmp(&points[b].x));
            }
        }
        let needed = |cell: &[usize]| {
            cell.iter().map(|&l| sizes[l].width).sum::<f32>()
                + NODE_GAP * cell.len().saturating_sub(1) as f32
        };
        let widths: Vec<f32> = (0..columns)
            .map(|c| {
                let content = cells[c].iter().map(|cell| needed(cell)).fold(0.0, f32::max);
                if c == 0 {
                    content
                } else {
                    let name = nodes[leaves[c - 1]].name_width;
                    (content.max(name) + 2.0 * PADDING).max(MIN_LANE)
                }
            })
            .collect();
        let depth_of = |q: usize| ancestors(q)[1..].iter().filter(|&&a| nodes[a].partition).count();
        let deepest = leaves.iter().map(|&q| depth_of(q)).max().unwrap_or(0);
        let origin = (0..nodes.len())
            .map(|i| nodes[i].bounds)
            .reduce(|a, b| a.union(&b))
            .expect("a diagram with flows has nodes")
            .origin();
        let mut row_top = Vec::with_capacity(tops.len());
        let mut y = origin.y + (deepest + 1) as f32 * scene::PARTITION_HEADER + PADDING;
        for depth in &row_depth {
            row_top.push(y);
            y += depth + LAYER_GAP;
        }
        let bottom = y - LAYER_GAP + PADDING;
        let mut x = origin.x;
        let mut placed: Vec<(usize, Rect)> = Vec::new();
        for c in 0..columns {
            if c == 0 && widths[0] == 0.0 {
                continue;
            }
            for (row, cell) in cells[c].iter().enumerate() {
                let mut at = x + (widths[c] - needed(cell)) / 2.0;
                for &l in cell {
                    let top = row_top[row] + (row_depth[row] - sizes[l].height) / 2.0;
                    placed
                        .push((movable[l], Rect::from_origin_size(Point::new(at, top), sizes[l])));
                    at += sizes[l].width + NODE_GAP;
                }
            }
            if c > 0 {
                let q = leaves[c - 1];
                let top = origin.y + depth_of(q) as f32 * scene::PARTITION_HEADER;
                placed.push((q, Rect::new(x, top, widths[c], bottom - top)));
            }
            x += widths[c] + if c == 0 { NODE_GAP } else { 0.0 };
        }
        // The bars from the leftmost to the rightmost of what they join, and a little beyond.
        for l in bars {
            // A fork spans where it leads, a join what leads to it: the side with more flows.
            let after: Vec<usize> =
                flows.iter().filter(|e| e.upper == l).map(|e| e.lower).collect();
            let before: Vec<usize> =
                flows.iter().filter(|e| e.lower == l).map(|e| e.upper).collect();
            let joined = if after.len() >= before.len() { after } else { before };
            let centers: Vec<f32> = joined
                .into_iter()
                .filter_map(|o| placed.iter().find(|(i, _)| *i == movable[o]))
                .map(|(_, r)| r.center().x)
                .collect();
            let (lo, hi) = centers
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &c| (lo.min(c), hi.max(c)));
            let (left, width) = if lo.is_finite() {
                (lo - BAR_REACH / 2.0, hi - lo + BAR_REACH)
            } else {
                (points[l].x + origin.x, BAR_REACH)
            };
            let row = row_of(l);
            let top = row_top[row] + (row_depth[row] - sizes[l].height) / 2.0;
            placed.push((movable[l], Rect::new(left, top, width, sizes[l].height)));
        }
        // Partitions that hold others span them.
        let mut outer: Vec<usize> =
            partitions.iter().copied().filter(|q| !leaves.contains(q)).collect();
        outer.sort_by_key(|&q| std::cmp::Reverse(depth_of(q)));
        for q in outer {
            let inner = placed
                .iter()
                .filter(|(i, _)| nodes[*i].partition && parent[*i] == Some(q))
                .map(|(_, r)| *r)
                .reduce(|a, b| a.union(&b));
            if let Some(inner) = inner {
                let top = origin.y + depth_of(q) as f32 * scene::PARTITION_HEADER;
                placed.push((q, Rect::new(inner.x, top, inner.width, bottom - top)));
            }
        }
        Some(placed)
    } else {
        None
    };

    // Containers inside out: lay out what each holds, then size it around that.
    let mut new_regions: Vec<(usize, Regions)> = Vec::new();
    let mut moved: Vec<(usize, Rect)> = if let Some(lanes) = lanes {
        lanes
    } else {
        let mut size: Vec<Size> = nodes.iter().map(|n| n.size).collect();
        let mut offset: Vec<Point> = vec![Point::new(0.0, 0.0); nodes.len()];
        let depth = |i: usize| ancestors(i).len();
        let mut containers: Vec<usize> = in_scope
            .iter()
            .copied()
            .filter(|&c| (0..nodes.len()).any(|i| parent[i] == Some(c)))
            .collect();
        containers.sort_by(|&a, &b| depth(b).cmp(&depth(a)).then(a.cmp(&b)));
        // Lays out nodes of which none holds another.
        let lay_out = |children: &[usize], size: &[Size]| -> Vec<Point> {
            let local: HashMap<usize, usize> =
                children.iter().enumerate().map(|(l, &g)| (g, l)).collect();
            // An edge counts between the children that hold its ends.
            let representative =
                |i: usize| ancestors(i).into_iter().find(|a| local.contains_key(a));
            let level_edges: Vec<Edge> = edges
                .iter()
                .filter_map(|&(s, t, source_above, hierarchy)| {
                    let (a, b) = (representative(s)?, representative(t)?);
                    (a != b).then(|| {
                        let (upper, lower) = if source_above { (a, b) } else { (b, a) };
                        Edge { upper: local[&upper], lower: local[&lower], hierarchy }
                    })
                })
                .collect();
            let sizes: Vec<Size> = children.iter().map(|&c| size[c]).collect();
            layered(&sizes, &level_edges, direction)
        };
        for &c in &containers {
            let mut children: Vec<usize> =
                (0..nodes.len()).filter(|&i| parent[i] == Some(c)).collect();
            children.sort();
            if let (Some(regions), Some(state)) = (&nodes[c].regions, nodes[c].element) {
                // Each region on its own, the regions one below the other or side by side; the
                // first one holds the name.
                let owners = crate::state_edit::regions_of(model, state);
                let count = owners.len().max(regions.separators.len() + 1);
                let mut groups: Vec<Vec<usize>> = vec![Vec::new(); count];
                for &child in &children {
                    let owner =
                        nodes[child].element.and_then(|e| model.get(e)).and_then(|e| e.owner());
                    let k = owners.iter().position(|&r| Some(r) == owner).unwrap_or(0);
                    groups[k].push(child);
                }
                let (mut along, mut deepest, mut separators) = (0.0f32, 0.0f32, Vec::new());
                for (k, group) in groups.iter().enumerate() {
                    let points = lay_out(group, &size);
                    let (mut w, mut h) = (0.0f32, 0.0f32);
                    for (&child, p) in group.iter().zip(&points) {
                        w = w.max(p.x + size[child].width);
                        h = h.max(p.y + size[child].height);
                    }
                    let (w, h) = (w.max(EMPTY_REGION), h.max(EMPTY_REGION));
                    let at = if regions.vertical {
                        if k > 0 {
                            separators.push(along);
                        }
                        let at = Point::new(along + PADDING, HEADER);
                        (along, deepest) = (along + w + 2.0 * PADDING, deepest.max(h));
                        at
                    } else {
                        let top = if k == 0 { HEADER } else { along + PADDING };
                        if k > 0 {
                            separators.push(along);
                        }
                        (along, deepest) = (top + h + PADDING, deepest.max(w));
                        Point::new(PADDING, top)
                    };
                    for (&child, p) in group.iter().zip(&points) {
                        offset[child] = p.offset(at.x, at.y);
                    }
                }
                let name = nodes[c].name_width + 2.0 * PADDING;
                size[c] = if regions.vertical {
                    Size::new(along.max(name), HEADER + deepest + PADDING)
                } else {
                    Size::new((deepest + 2.0 * PADDING).max(name), along)
                };
                new_regions.push((c, Regions { vertical: regions.vertical, separators }));
                continue;
            }
            let points = lay_out(&children, &size);
            let (mut right, mut bottom) = (0.0f32, 0.0f32);
            for (&child, p) in children.iter().zip(&points) {
                offset[child] = p.offset(PADDING, HEADER);
                right = right.max(p.x + size[child].width);
                bottom = bottom.max(p.y + size[child].height);
            }
            let width = (right + 2.0 * PADDING).max(nodes[c].name_width + 2.0 * PADDING);
            size[c] = Size::new(width, bottom + HEADER + PADDING);
        }
        let mut top: Vec<usize> = roots.clone();
        top.sort();
        // With a scope, the roots may sit at different levels; they are laid out among themselves.
        let points = lay_out(&top, &size);

        // Where the result goes: where the laid out nodes were.
        let before = top
            .iter()
            .map(|&i| nodes[i].bounds)
            .reduce(|a, b| a.union(&b))
            .expect("roots are not empty");
        let origin = before.origin();
        let mut bounds: HashMap<usize, Rect> = HashMap::new();
        for (&root, p) in top.iter().zip(&points) {
            bounds.insert(root, Rect::from_origin_size(p.offset(origin.x, origin.y), size[root]));
        }
        // Then what each container holds, outside in.
        let mut pending: Vec<usize> = top.clone();
        while let Some(c) = pending.pop() {
            let at = bounds[&c].origin();
            for i in (0..nodes.len()).filter(|&i| parent[i] == Some(c)) {
                bounds.insert(
                    i,
                    Rect::from_origin_size(at.offset(offset[i].x, offset[i].y), size[i]),
                );
                pending.push(i);
            }
        }
        // Actors beside a system boundary go level with the use cases they take part in.
        if kind == DiagramKind::UseCase {
            let actor = |i: usize| {
                let element = nodes[i].element.and_then(|e| model.get(e));
                element.is_some_and(|e| matches!(e.kind(), ElementKind::Actor(_)))
            };
            let mut wanted: Vec<(usize, f32)> = top
                .iter()
                .copied()
                .filter(|&i| actor(i))
                .filter_map(|a| {
                    let ys: Vec<f32> = edges
                        .iter()
                        .filter_map(|&(s, t, _, _)| match (s == a, t == a) {
                            (true, _) => Some(t),
                            (_, true) => Some(s),
                            _ => None,
                        })
                        .filter(|&o| !actor(o))
                        .filter_map(|o| bounds.get(&o).map(|r| r.center().y))
                        .collect();
                    (!ys.is_empty()).then(|| (a, ys.iter().sum::<f32>() / ys.len() as f32))
                })
                .collect();
            wanted.sort_by(|a, b| {
                bounds[&a.0].x.total_cmp(&bounds[&b.0].x).then(a.1.total_cmp(&b.1))
            });
            let mut column: Option<(f32, f32)> = None;
            for (a, y) in wanted {
                let r = bounds[&a];
                let below = match column {
                    Some((x, bottom)) if (x - r.x).abs() < 0.5 => bottom + ACTOR_GAP,
                    _ => f32::NEG_INFINITY,
                };
                let top = (y - r.height / 2.0).max(below);
                bounds.insert(a, Rect::new(r.x, top, r.width, r.height));
                column = Some((r.x, top + r.height));
            }
        }
        let after = top.iter().map(|i| bounds[i]).reduce(|a, b| a.union(&b)).expect("not empty");

        // Make room: nodes right of or below the old place move by what the result grew; others
        // in its way move right. Containers around the scope grow to hold it.
        let mut moved: Vec<(usize, Rect)> = bounds.into_iter().collect();
        if scope.is_some() {
            let grow = (after.right() - before.right()).max(0.0);
            let grow_down = (after.bottom() - before.bottom()).max(0.0);
            let outside: Vec<usize> = (0..nodes.len())
                .filter(|i| {
                    !in_scope.contains(i) && parent[*i].is_none_or(|p| !in_scope.contains(&p))
                })
                .collect();
            for i in outside {
                let r = nodes[i].bounds;
                if r.contains_rect(&before) {
                    let wanted = r.union(&after.inflate(PADDING));
                    if wanted != r {
                        moved.push((i, wanted));
                    }
                    continue;
                }
                if parent[i].is_some() {
                    continue;
                }
                let (dx, dy) = if r.x >= before.right() - 0.5 {
                    (grow, 0.0)
                } else if r.y >= before.bottom() - 0.5 {
                    (0.0, grow_down)
                } else if r.intersects(&after.inflate(PADDING)) {
                    (after.right() + NODE_GAP - r.x, 0.0)
                } else {
                    (0.0, 0.0)
                };
                if dx != 0.0 || dy != 0.0 {
                    // What it holds moves along.
                    for j in (0..nodes.len()).filter(|&j| ancestors(j).contains(&i)) {
                        moved.push((j, nodes[j].bounds.translate(dx, dy)));
                    }
                }
            }
        }
        moved
    };
    moved.sort_by_key(|(i, _)| *i);
    let ids: Vec<Id> = nodes.iter().map(|n| n.id).collect();
    let bars: Vec<Id> = nodes.iter().filter(|n| n.bar).map(|n| n.id).collect();
    let old: Vec<Rect> = nodes.iter().map(|n| n.bounds).collect();
    let mut changed = Vec::new();
    for (i, rect) in moved {
        if rect != old[i] {
            tx.set_bounds(diagram, ids[i], rect)?;
            changed.push(ids[i]);
        }
    }
    // Orthogonal states keep where the lines between their regions go.
    for (i, regions) in new_regions {
        tx.update_presentation(diagram, ids[i], |p| {
            if let Shape::Node { regions: r, .. } = &mut p.shape {
                *r = Some(regions);
            }
        })?;
    }

    // Loops follow their node; the lines of what moved are routed.
    let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let mut loops = Vec::new();
    let mut lines = Vec::new();
    for p in view.presentations() {
        let Shape::Edge { source, target, .. } = &p.shape else { continue };
        let touched = changed.contains(source) || changed.contains(target) || scope.is_none();
        if !touched {
            continue;
        }
        if source == target {
            if let Some(r) = view.get(*source).and_then(|n| n.bounds()) {
                loops.push((p.id, scene::self_loop(r)));
            }
        } else {
            lines.push(p.id);
        }
    }
    for (edge, waypoints) in loops {
        tx.set_route(diagram, edge, waypoints, (None, None))?;
    }
    crate::autoroute::route_lines(tx, diagram, &lines)?;
    straighten_at_bars(tx, diagram, &bars, &lines)?;
    Ok(changed)
}

/// Lines between a fork or join bar and a node above or below it run straight up or down.
fn straighten_at_bars(tx: &mut Tx<'_>, diagram: Id, bars: &[Id], lines: &[Id]) -> Result<()> {
    let view = tx.model().view(diagram).ok_or(ModelError::NotADiagram(diagram))?;
    let bounds = |id: Id| view.get(id).and_then(|p| p.bounds());
    let mut straight = Vec::new();
    for &line in lines {
        let Some(Shape::Edge { source, target, .. }) = view.get(line).map(|p| &p.shape) else {
            continue;
        };
        let (Some(s), Some(t)) = (bounds(*source), bounds(*target)) else { continue };
        let (bar, node, bar_is_source) = match (bars.contains(source), bars.contains(target)) {
            (true, false) => (s, t, true),
            (false, true) => (t, s, false),
            _ => continue,
        };
        let x = node.center().x;
        if x < bar.x || x > bar.right() {
            continue;
        }
        let at_bar = Point::new((x - bar.x) / bar.width, 0.5);
        let at_node = Point::new(0.5, 0.5);
        let anchors = if bar_is_source { (at_bar, at_node) } else { (at_node, at_bar) };
        straight.push((line, anchors));
    }
    for (line, (a, b)) in straight {
        tx.set_route(diagram, line, Vec::new(), (Some(a), Some(b)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxes(sizes: &[Size], points: &[Point]) -> Vec<Rect> {
        sizes.iter().zip(points).map(|(s, p)| Rect::from_origin_size(*p, *s)).collect()
    }

    fn overlap(rects: &[Rect]) -> bool {
        rects.iter().enumerate().any(|(i, a)| rects[i + 1..].iter().any(|b| a.intersects(b)))
    }

    #[test]
    fn generals_go_above_their_specifics() {
        // 0 is the general of 1 and 2; 3 is part of 1.
        let sizes = [Size::new(120.0, 60.0); 4];
        let edges = [
            Edge { upper: 0, lower: 1, hierarchy: true },
            Edge { upper: 0, lower: 2, hierarchy: true },
            Edge { upper: 1, lower: 3, hierarchy: true },
        ];
        let points = layered(&sizes, &edges, Direction::Down);
        assert!(points[0].y < points[1].y && points[1].y == points[2].y);
        assert!(points[3].y > points[1].y);
        assert!(!overlap(&boxes(&sizes, &points)));
        // The general is centered over its two specifics.
        let middle = (points[1].x + points[2].x) / 2.0;
        assert!((points[0].x - middle).abs() < 1.0, "{points:?}");
    }

    #[test]
    fn plain_edges_give_way_in_a_cycle() {
        let sizes = [Size::new(100.0, 40.0); 3];
        let edges = [
            Edge { upper: 0, lower: 1, hierarchy: true },
            Edge { upper: 1, lower: 2, hierarchy: true },
            Edge { upper: 2, lower: 0, hierarchy: false },
        ];
        let points = layered(&sizes, &edges, Direction::Down);
        assert!(points[0].y < points[1].y && points[1].y < points[2].y, "{points:?}");
    }

    #[test]
    fn unconnected_parts_are_packed_without_overlap() {
        let sizes: Vec<Size> =
            (0..9).map(|i| Size::new(80.0 + 10.0 * i as f32, 40.0 + 5.0 * i as f32)).collect();
        let edges = [Edge { upper: 0, lower: 1, hierarchy: false }];
        let points = layered(&sizes, &edges, Direction::Down);
        assert!(!overlap(&boxes(&sizes, &points)));
        assert!(points.iter().all(|p| p.x >= 0.0 && p.y >= 0.0));
    }

    #[test]
    fn left_to_right_puts_generals_left() {
        let sizes = [Size::new(120.0, 60.0); 2];
        let edges = [Edge { upper: 0, lower: 1, hierarchy: true }];
        let points = layered(&sizes, &edges, Direction::Right);
        assert!(points[0].x + 120.0 <= points[1].x && points[0].y == points[1].y, "{points:?}");
    }

    #[test]
    fn layouts_do_not_change_from_run_to_run() {
        let sizes: Vec<Size> = (0..12).map(|i| Size::new(60.0 + 7.0 * i as f32, 40.0)).collect();
        let edges: Vec<Edge> =
            (1..12).map(|i| Edge { upper: (i * 7) % i, lower: i, hierarchy: i % 3 == 0 }).collect();
        assert_eq!(
            layered(&sizes, &edges, Direction::Down),
            layered(&sizes, &edges, Direction::Down)
        );
    }

    /// The node rects of a diagram, without those drawn around others.
    fn leaves(model: &Model, diagram: Id) -> Vec<Rect> {
        let rects: Vec<Rect> = model
            .view(diagram)
            .unwrap()
            .presentations()
            .iter()
            .filter_map(|p| p.bounds())
            .collect();
        rects
            .iter()
            .copied()
            .filter(|r| !rects.iter().any(|o| o != r && r.contains_rect(o)))
            .collect()
    }

    #[test]
    fn a_whole_diagram_is_arranged_and_its_lines_routed() {
        use umlcraft_model::sample::order_system;
        let (mut doc, ids) = order_system();
        for diagram in [ids.class_diagram, ids.component_diagram, ids.deployment_diagram] {
            doc.edit("arrange", |tx| {
                arrange(tx, diagram, None, Direction::Down, &scene::ApproxMeasure)
            })
            .unwrap();
            let model = doc.model();
            assert!(!overlap(&leaves(model, diagram)), "{diagram:?}");
            let view = model.view(diagram).unwrap();
            for p in view.presentations() {
                if let Shape::Edge { source, target, .. } = p.shape
                    && source != target
                {
                    assert!(p.style.auto_route, "{p:?}");
                }
            }
            model.check().unwrap();
        }
    }

    #[test]
    fn arranging_a_selection_keeps_its_place_and_pushes_others_aside() {
        use umlcraft_model::sample::order_system;
        let (mut doc, ids) = order_system();
        let d = ids.class_diagram;
        let view = doc.model().view(d).unwrap();
        let nodes: Vec<(Id, Rect)> =
            view.presentations().iter().filter_map(|p| Some((p.id, p.bounds()?))).collect();
        let picked: Vec<Id> = nodes.iter().take(2).map(|n| n.0).collect();
        let corner = nodes[..2].iter().map(|n| n.1).reduce(|a, b| a.union(&b)).unwrap().origin();
        doc.edit("arrange", |tx| {
            arrange(tx, d, Some(&picked), Direction::Right, &scene::ApproxMeasure)
        })
        .unwrap();
        let view = doc.model().view(d).unwrap();
        let after = picked
            .iter()
            .map(|&id| view.get(id).unwrap().bounds().unwrap())
            .reduce(|a, b| a.union(&b))
            .unwrap();
        assert_eq!(after.origin(), corner);
        assert!(!overlap(&leaves(doc.model(), d)));
        doc.model().check().unwrap();
    }

    /// The presentation of `element` on `diagram`, its bounds.
    fn shown(model: &Model, diagram: Id, element: Id) -> Rect {
        let view = model.view(diagram).unwrap();
        view.presentations_of(element).next().unwrap().bounds().unwrap()
    }

    fn elements_of(model: &Model, diagram: Id, pick: fn(&ElementKind) -> bool) -> Vec<Id> {
        let view = model.view(diagram).unwrap();
        view.presentations()
            .iter()
            .filter_map(|p| p.element)
            .filter(|&e| model.get(e).is_some_and(|e| pick(e.kind())))
            .collect()
    }

    #[test]
    fn activities_run_down_and_keep_their_partitions() {
        use umlcraft_model::sample::order_system;
        let (mut doc, ids) = order_system();
        let d = ids.activity_diagram;
        let partitions = elements_of(doc.model(), d, |k| matches!(k, ElementKind::Partition));
        let steps = elements_of(doc.model(), d, |k| matches!(k, ElementKind::ActivityNode(_)));
        let lane = |model: &Model, step: Id| {
            let c = shown(model, d, step).center();
            partitions.iter().copied().find(|&q| shown(model, d, q).contains(c))
        };
        let before: Vec<Option<Id>> = steps.iter().map(|&s| lane(doc.model(), s)).collect();
        doc.edit("arrange", |tx| {
            arrange(tx, d, None, default_direction(DiagramKind::Activity), &scene::ApproxMeasure)
        })
        .unwrap();
        let model = doc.model();
        let after: Vec<Option<Id>> = steps.iter().map(|&s| lane(model, s)).collect();
        assert_eq!(after, before);
        let lanes: Vec<Rect> = partitions.iter().map(|&q| shown(model, d, q)).collect();
        assert!(lanes.windows(2).all(|w| w[0].y == w[1].y && w[0].height == w[1].height));
        assert!(!overlap(&lanes.iter().map(|r| r.inflate(-0.5)).collect::<Vec<_>>()));
        let rects: Vec<Rect> = steps.iter().map(|&s| shown(model, d, s)).collect();
        assert!(!overlap(&rects));
        // The start is above everything else.
        let start = steps
            .iter()
            .position(|&s| {
                matches!(model.get(s).unwrap().kind(), ElementKind::ActivityNode(p) if p.node == ActivityNodeKind::Initial)
            })
            .unwrap();
        assert!(rects.iter().all(|r| r.y >= rects[start].y));
        model.check().unwrap();
    }

    #[test]
    fn states_stay_in_their_states_and_regions() {
        use umlcraft_model::sample::order_system;
        let (mut doc, ids) = order_system();
        let d = ids.state_diagram;
        let states = elements_of(doc.model(), d, |k| matches!(k, ElementKind::State(_)));
        let open = *states.iter().find(|&&s| doc.model().get(s).unwrap().name() == "Open").unwrap();
        // Two regions, one below the other; the existing substates go into the first.
        doc.edit("regions", |tx| {
            crate::state_edit::add_region(tx, open, false, Some(d)).map(|_| ())
        })
        .unwrap();
        let regions = crate::state_edit::regions_of(doc.model(), open);
        let second = regions[1];
        doc.edit("into the second", |tx| {
            let state = tx.create(
                second,
                umlcraft_model::ElementData::new("Paid", ElementKind::State(Default::default())),
            )?;
            let r = shown(tx.model(), d, open);
            tx.add_node(d, Some(state), Rect::new(r.x + 20.0, r.bottom() - 50.0, 80.0, 40.0))
                .map(|_| ())
        })
        .unwrap();
        let owners = |model: &Model| -> Vec<Option<Id>> {
            elements_of(model, d, |k| k.is_vertex())
                .iter()
                .map(|&v| model.get(v).unwrap().owner())
                .collect()
        };
        let before = owners(doc.model());
        doc.edit("arrange", |tx| {
            arrange(
                tx,
                d,
                None,
                default_direction(DiagramKind::StateMachine),
                &scene::ApproxMeasure,
            )
        })
        .unwrap();
        let model = doc.model();
        assert_eq!(owners(model), before);
        let view = model.view(d).unwrap();
        let p = view.presentations_of(open).next().unwrap();
        let (outer, separators) = (p.bounds().unwrap(), p.regions().unwrap().separators.clone());
        assert_eq!(separators.len(), 1);
        for vertex in elements_of(model, d, |k| k.is_vertex()) {
            let owner = model.get(vertex).unwrap().owner().unwrap();
            let r = shown(model, d, vertex);
            if owner == regions[0] {
                assert!(outer.contains_rect(&r) && r.bottom() <= outer.y + separators[0], "{r:?}");
            } else if owner == second {
                assert!(outer.contains_rect(&r) && r.y >= outer.y + separators[0], "{r:?}");
            }
        }
        // Transitions run from left to right.
        let rect = |name: &str| {
            let state = states.iter().find(|&&s| model.get(s).unwrap().name() == name).unwrap();
            shown(model, d, *state)
        };
        assert!(rect("Open").right() < rect("Submitted").x);
        assert!(rect("Submitted").right() < rect("Shipped").x);
        model.check().unwrap();
    }

    #[test]
    fn actors_stand_beside_the_system_boundary() {
        use umlcraft_model::sample::order_system;
        let (mut doc, ids) = order_system();
        let d = ids.use_case_diagram;
        doc.edit("boundary", |tx| {
            let owner = tx.model().get(d).unwrap().owner().unwrap();
            let boundary = tx.create(
                owner,
                umlcraft_model::ElementData::new("Shop", ElementKind::SystemBoundary),
            )?;
            // Around all use cases.
            let view = tx.model().view(d).unwrap();
            let around = view
                .presentations()
                .iter()
                .filter(|p| {
                    p.element
                        .and_then(|e| tx.model().get(e))
                        .is_some_and(|e| matches!(e.kind(), ElementKind::UseCase(_)))
                })
                .filter_map(|p| p.bounds())
                .reduce(|a, b| a.union(&b))
                .unwrap()
                .inflate(30.0);
            tx.add_node(d, Some(boundary), around).map(|_| ())
        })
        .unwrap();
        doc.edit("arrange", |tx| {
            arrange(tx, d, None, default_direction(DiagramKind::UseCase), &scene::ApproxMeasure)
        })
        .unwrap();
        let model = doc.model();
        let boundary = elements_of(model, d, |k| matches!(k, ElementKind::SystemBoundary))[0];
        let frame = shown(model, d, boundary);
        let actors = elements_of(model, d, |k| matches!(k, ElementKind::Actor(_)));
        assert!(!actors.is_empty());
        for actor in actors {
            let r = shown(model, d, actor);
            assert!(r.right() <= frame.x || r.x >= frame.right(), "{r:?} {frame:?}");
        }
        for case in elements_of(model, d, |k| matches!(k, ElementKind::UseCase(_))) {
            assert!(frame.contains_rect(&shown(model, d, case)));
        }
        model.check().unwrap();
    }
}
