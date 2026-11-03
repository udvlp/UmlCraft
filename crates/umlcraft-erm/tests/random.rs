//! Random valid diagrams (fixed seeds, every field of the format) survive writing and reading:
//! the diagram read is the one written, and writing it again gives the same bytes.

use pretty_assertions::assert_eq;
use umlcraft_erm::{
    Caption, Cardinality, Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Max, Meta,
    Node, NodeKind, Point, Props, Rect, Severity, Style, Text, TextAlign, check, read, write,
};

/// xorshift64*: enough randomness for test data, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    /// Coordinates as people and draw.io write them: mostly whole, sometimes fractions.
    fn coord(&mut self) -> f64 {
        let whole = self.below(2000) as f64 - 500.0;
        if self.chance(20) { whole + self.below(10_000) as f64 / 10_000.0 } else { whole }
    }

    fn unit(&mut self) -> f64 {
        [0.0, 0.25, 0.5, 0.75, 1.0][self.below(5) as usize]
    }

    fn text(&mut self) -> String {
        const WORDS: [&str; 8] = ["Kunde", "bucht", "Straße", "id", "ÄÖÜ ß", "a\nb", "\"q\"", "x"];
        WORDS[self.below(WORDS.len() as u64) as usize].to_owned()
    }

    fn maybe(&mut self, percent: u64) -> Option<String> {
        self.chance(percent).then(|| self.text())
    }
}

struct Builder {
    rng: Rng,
    next: u64,
    d: Diagram,
}

impl Builder {
    fn id(&mut self) -> Id {
        self.next += 1;
        // Not in creation order, so that sorting by id is exercised.
        Id(format!("{:016x}", self.rng.next() ^ self.next))
    }

    fn style(&mut self) -> Style {
        let r = &mut self.rng;
        Style {
            fill: r.chance(20).then(|| "#dae8fc".to_owned()),
            stroke: r.chance(10).then(|| "#6c8ebf".to_owned()),
            font_color: r.chance(10).then(|| "#ff0000".to_owned()),
            font_size: r.chance(10).then(|| [10.0, 14.0, 11.5][r.below(3) as usize]),
            bold: r.chance(10),
            italic: r.chance(10),
            rounded: r.chance(10),
            align: r.chance(10).then_some(TextAlign::Right),
        }
    }

    fn node(&mut self, kind: NodeKind) -> Id {
        let id = self.id();
        let r = &mut self.rng;
        let bounds = Rect::new(r.coord(), r.coord(), 10.0 + r.below(200) as f64, 28.0);
        let props = Props {
            physical_name: r.maybe(20),
            datatype: r.maybe(20),
            length_precision: r.chance(10).then(|| "8,2".to_owned()),
            not_null: r.chance(20),
            default_value: r.maybe(10),
            comment: r.maybe(10),
        };
        let name = r.text();
        let style = self.style();
        self.d.nodes.push(Node { id: id.clone(), kind, name, bounds, style, props });
        id
    }

    fn pos(&mut self) -> LabelPos {
        let r = &mut self.rng;
        if r.chance(30) {
            return LabelPos::default();
        }
        LabelPos {
            x: [-1.0, -0.0154, 0.0, 0.5, 1.0][r.below(5) as usize],
            y: r.below(41) as f64 - 20.0,
            dx: if r.chance(20) { r.coord() } else { 0.0 },
            dy: if r.chance(20) { r.coord() } else { 0.0 },
        }
    }

    fn link(&mut self, source: &Id, target: &Id) -> usize {
        let mut l = Link::new(source.clone(), target.clone());
        l.id = self.id();
        if self.rng.chance(40) {
            l.source_port = Some(Point::new(self.rng.unit(), self.rng.unit()));
        }
        if self.rng.chance(40) {
            l.target_port = Some(Point::new(self.rng.unit(), self.rng.unit()));
        }
        l.line = [Line::Solid, Line::Dotted, Line::Double][self.rng.below(3) as usize];
        for _ in 0..self.rng.below(3) {
            l.waypoints.push(Point::new(self.rng.coord(), self.rng.coord()));
        }
        self.d.links.push(l);
        self.d.links.len() - 1
    }

    fn attributes(&mut self, owner: &Id) {
        for _ in 0..self.rng.below(4) {
            let kind = NodeKind::Attribute {
                key: [None, Some(Key::Primary), Some(Key::Partial)][self.rng.below(3) as usize],
                derived: self.rng.chance(20),
                multivalued: self.rng.chance(20),
            };
            let a = self.node(kind);
            self.link(&a, owner);
            if self.rng.chance(15) {
                let part = self.node(NodeKind::attribute());
                self.link(&part, &a);
            }
        }
    }
}

fn diagram(seed: u64) -> Diagram {
    let mut b = Builder { rng: Rng(seed), next: 0, d: Diagram::default() };
    b.d.meta = Meta {
        name: b.rng.text(),
        author: b.rng.maybe(30),
        created: b.rng.chance(30).then(|| "2026-10-31".to_owned()),
        modified: None,
        origin: b.rng.chance(30).then(|| "SQL: flug.sql".to_owned()),
        notation: None,
    };
    let entities: Vec<Id> = (0..2 + b.rng.below(5))
        .map(|_| {
            let kind = if b.rng.chance(15) {
                NodeKind::AssociativeEntity
            } else {
                NodeKind::Entity { weak: false }
            };
            b.node(kind)
        })
        .collect();
    for e in &entities.clone() {
        b.attributes(e);
    }
    for _ in 0..b.rng.below(4) {
        let r = b.node(NodeKind::relationship());
        b.attributes(&r);
        let ends = 2 + b.rng.below(2);
        for _ in 0..ends {
            let e = entities[b.rng.below(entities.len() as u64) as usize].clone();
            let i = b.link(&r, &e);
            if b.rng.chance(70) {
                let text = if b.rng.chance(80) {
                    let min = b.rng.below(2) as u32;
                    let max = if b.rng.chance(50) { Max::Many } else { Max::Count(1 + min) };
                    LabelText::Cardinality(Cardinality::new(min, max))
                } else {
                    LabelText::Text(b.rng.text())
                };
                let pos = b.pos();
                b.d.links[i].label = Some(Label { text, pos });
            }
            if b.rng.chance(15) {
                let pos = b.pos();
                b.d.links[i].role = Some(Caption { text: b.rng.text(), pos });
            }
        }
    }
    if b.rng.chance(40) {
        let kind = NodeKind::IsA {
            total: b.rng.chance(50),
            disjoint: [None, Some(true), Some(false)][b.rng.below(3) as usize],
        };
        let i = b.node(kind);
        let sup = b.node(NodeKind::entity());
        let up = b.link(&i, &sup);
        b.d.links[up].super_type = true;
        let sub = b.node(NodeKind::entity());
        b.link(&i, &sub);
    }
    for _ in 0..b.rng.below(3) {
        let id = b.id();
        let bounds = Rect::new(b.rng.coord(), b.rng.coord(), 200.0, 30.0);
        let style = b.style();
        let text = b.rng.text();
        b.d.texts.push(Text { id, bounds, text, style });
    }
    b.d
}

#[test]
fn random_diagrams_survive_writing_and_reading() {
    for seed in 1..=300_u64 {
        let mut d = diagram(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let errors: Vec<_> =
            check(&d).into_iter().filter(|i| i.severity() == Severity::Error).collect();
        assert_eq!(errors, vec![], "seed {seed}: the generator made an invalid diagram");
        let file = write(&d);
        let (back, _) = read(&file).unwrap_or_else(|e| panic!("seed {seed}: {e}\n{file}"));
        d.sort();
        assert_eq!(back, d, "seed {seed}");
        assert_eq!(write(&back), file, "seed {seed}");
    }
}
