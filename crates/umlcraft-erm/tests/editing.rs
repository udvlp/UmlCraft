//! Random sequences of editor operations (fixed seeds): after every operation the diagram
//! keeps all error rules, a refused operation changes nothing, and the result survives writing
//! and reading.

use pretty_assertions::assert_eq;
use umlcraft_erm::edit::{self, End, Item};
use umlcraft_erm::{Diagram, Id, Key, Line, NodeKind, Point, Rect, Severity, check, read, write};

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
        self.next() % n.max(1)
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn coord(&mut self) -> f64 {
        self.below(1200) as f64 - 200.0
    }

    fn port(&mut self) -> Option<Point> {
        self.chance(40).then(|| {
            let u = [0.0, 0.25, 0.5, 0.75, 1.0];
            Point::new(u[self.below(5) as usize], u[self.below(5) as usize])
        })
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len() as u64) as usize])
    }
}

fn kind(rng: &mut Rng) -> NodeKind {
    match rng.below(5) {
        0 => NodeKind::Entity { weak: rng.chance(30) },
        1 => NodeKind::AssociativeEntity,
        2 => NodeKind::Attribute {
            key: [None, Some(Key::Primary), Some(Key::Partial)][rng.below(3) as usize],
            derived: rng.chance(20),
            multivalued: rng.chance(20),
        },
        3 => NodeKind::Relationship { identifying: rng.chance(30) },
        _ => NodeKind::IsA {
            total: rng.chance(50),
            disjoint: [None, Some(true), Some(false)][rng.below(3) as usize],
        },
    }
}

fn items(d: &Diagram) -> Vec<Item> {
    let mut out: Vec<Item> = d.nodes.iter().map(|n| Item::Node(n.id.clone())).collect();
    out.extend(d.links.iter().map(|l| Item::Link(l.id.clone())));
    out.extend(d.links.iter().filter(|l| l.label.is_some()).map(|l| Item::Label(l.id.clone())));
    out.extend(d.texts.iter().map(|t| Item::Text(t.id.clone())));
    out
}

/// One random operation; whether it was carried out.
fn step(d: &mut Diagram, rng: &mut Rng) -> bool {
    let nodes: Vec<Id> = d.nodes.iter().map(|n| n.id.clone()).collect();
    let links: Vec<Id> = d.links.iter().map(|l| l.id.clone()).collect();
    let all = items(d);
    let at = Point::new(rng.coord(), rng.coord());
    let result = match rng.below(17) {
        0 | 1 => {
            let k = kind(rng);
            edit::add_node(d, k, "N", at);
            Ok(())
        }
        2 => match rng.pick(&nodes) {
            Some(owner) => edit::add_attribute(d, &owner.clone(), "a").map(drop),
            None => Ok(()),
        },
        3..=5 => match (rng.pick(&nodes).cloned(), rng.pick(&nodes).cloned()) {
            (Some(a), Some(b)) => {
                let ports = (rng.port(), rng.port());
                edit::connect(d, &a, &b, ports).map(drop)
            }
            _ => Ok(()),
        },
        6 => {
            let chosen: Vec<Item> = all.iter().filter(|_| rng.chance(15)).cloned().collect();
            edit::delete(d, &chosen)
        }
        7 => {
            let chosen: Vec<Item> = all.iter().filter(|_| rng.chance(40)).cloned().collect();
            edit::move_items(d, &chosen, rng.coord() / 10.0, rng.coord() / 10.0)
        }
        8 => match rng.pick(&all).cloned() {
            Some(item) => {
                let r = Rect::new(at.x, at.y, rng.below(200) as f64, rng.below(80) as f64);
                edit::set_bounds(d, &item, r)
            }
            None => Ok(()),
        },
        9 => match rng.pick(&all).cloned() {
            Some(item) => {
                let texts = ["Kunde", "[1,n]", "[0,1]", "", "leitet", "1..*"];
                let text = texts[rng.below(texts.len() as u64) as usize];
                edit::set_text(d, &item, text)
            }
            None => Ok(()),
        },
        10 => match rng.pick(&nodes).cloned() {
            Some(n) => {
                let k = kind(rng);
                edit::set_kind(d, &n, k)
            }
            None => Ok(()),
        },
        11 => match rng.pick(&links).cloned() {
            Some(l) => {
                let line = [Line::Solid, Line::Dotted, Line::Double][rng.below(3) as usize];
                edit::set_line(d, &l, line)
            }
            None => Ok(()),
        },
        12 => match rng.pick(&links).cloned() {
            Some(l) => edit::set_super_type(d, &l, rng.chance(70)),
            None => Ok(()),
        },
        13 => match (rng.pick(&links).cloned(), rng.pick(&nodes).cloned()) {
            (Some(l), Some(n)) => {
                let end = if rng.chance(50) { End::Source } else { End::Target };
                let port = rng.port();
                edit::reconnect(d, &l, end, &n, port)
            }
            _ => Ok(()),
        },
        14 => match rng.pick(&links).cloned() {
            Some(l) if rng.chance(50) => edit::release_ends(d, &[l]),
            Some(l) => edit::move_label(d, &l, 3.0, -2.0),
            None => Ok(()),
        },
        15 => {
            edit::add_text(d, "Hinweis", at);
            edit::fit_to_text(d, &nodes)
        }
        _ => {
            let chosen: Vec<Item> = all.iter().filter(|_| rng.chance(30)).cloned().collect();
            edit::set_style(d, &chosen, |s| s.bold = !s.bold)
        }
    };
    result.is_ok()
}

#[test]
fn random_editing_keeps_the_rules_and_the_file() {
    let mut refused = 0;
    let mut done = 0;
    for seed in 1..=50u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut d = Diagram::new("zufall");
        for i in 0..200 {
            let before = d.clone();
            if step(&mut d, &mut rng) {
                done += 1;
            } else {
                refused += 1;
                assert_eq!(d, before, "seed {seed} step {i}: a refused operation changed things");
            }
            let errors: Vec<_> =
                check(&d).into_iter().filter(|i| i.severity() == Severity::Error).collect();
            assert!(errors.is_empty(), "seed {seed} step {i}: {errors:?}");
        }
        let text = write(&d);
        let (back, _) = read(&text).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
        let mut sorted = d.clone();
        sorted.sort();
        assert_eq!(back, sorted, "seed {seed}");
    }
    // Both kinds happen, so the test exercises the guard.
    assert!(done > 5000 && refused > 100, "{done} done, {refused} refused");
}
