//! What the tests share: a diagram as a readable tree.

use std::fmt::Write;

use umlcraft_nsd::{Diagram, Element, ElementKind, Sequence};
use umlcraft_nsd_code::Made;

pub const MADE: Made = Made {
    author: "tester",
    date: "2026-10-29",
    origin: "UmlCraft test",
    keywords: umlcraft_nsd::Keywords::English,
};

pub fn tree(d: &Diagram) -> String {
    fn element(out: &mut String, e: &Element, depth: usize) {
        let pad = "  ".repeat(depth);
        let mut head = format!("{pad}{}", e.kind.tag());
        if e.color() != umlcraft_nsd::Color::WHITE {
            head.push_str(&format!(" color={}", e.color().to_hex()));
        }
        if e.disabled() {
            head.push_str(" disabled");
        }
        if e.kind == ElementKind::For {
            head.push_str(&format!(" style={}", e.attr("style").unwrap_or("")));
        }
        writeln!(out, "{head}").unwrap();
        for line in e.text() {
            writeln!(out, "{pad}  | {line}").unwrap();
        }
        for line in e.comment() {
            writeln!(out, "{pad}  # {line}").unwrap();
        }
        for b in &e.branches {
            writeln!(out, "{pad}  -").unwrap();
            sequence(out, b, depth + 2);
        }
    }
    fn sequence(out: &mut String, s: &Sequence, depth: usize) {
        for e in &s.elements {
            element(out, e, depth);
        }
    }
    let mut out = String::new();
    for line in d.text() {
        writeln!(out, "| {line}").unwrap();
    }
    for line in d.comment() {
        writeln!(out, "# {line}").unwrap();
    }
    sequence(&mut out, &d.children, 1);
    out
}
