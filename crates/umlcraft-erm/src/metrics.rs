//! Text widths and line breaks as draw.io's labels get them in the browser: Liberation Sans
//! (metric-compatible with the Helvetica and Arial draw.io names), advances plus the pairs of
//! the font's `kern` table, lines 1.2 × the font size apart, broken at spaces and never inside
//! a word. The table comes from `tools/erm-metrics/liberation.py`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The font size of node texts and of line labels (draw.io's `defaultVertex`/`defaultEdge`).
pub const NODE_FONT_SIZE: f64 = 12.0;
pub const LABEL_FONT_SIZE: f64 = 11.0;
/// Line height as a multiple of the font size (`mxConstants.LINE_HEIGHT`).
pub const LINE_HEIGHT: f64 = 1.2;
/// The baseline below the middle of its line, per px of font size: 3.5 px at 12 px, measured
/// in draw.io's exports.
pub const BASELINE: f64 = 3.5 / 12.0;
/// Underlines: their middle below the baseline and their thickness, per px of font size
/// (1.5 px and 1 px at 12 px, measured like the baseline).
pub const UNDERLINE_OFFSET: f64 = 1.5 / 12.0;
pub const UNDERLINE_THICKNESS: f64 = 1.0 / 12.0;
/// Characters missing from the table count as wide as this, in em.
const FALLBACK: f64 = 0.6;

const TABLE: &str = include_str!("metrics/liberation-sans.txt");

/// The widths of Liberation Sans Regular and Bold.
#[derive(Debug)]
pub struct Metrics {
    units: f64,
    ascent: f64,
    descent: f64,
    faces: [Face; 2],
}

#[derive(Debug, Default)]
struct Face {
    advances: HashMap<char, f64>,
    kerning: HashMap<(char, char), f64>,
}

impl Metrics {
    /// The embedded table, parsed once.
    pub fn get() -> &'static Metrics {
        static METRICS: OnceLock<Metrics> = OnceLock::new();
        METRICS.get_or_init(|| Metrics::parse(TABLE))
    }

    fn parse(table: &str) -> Self {
        let mut m = Metrics {
            units: 2048.0,
            ascent: 0.0,
            descent: 0.0,
            faces: [Face::default(), Face::default()],
        };
        let char_at = |hex: &str| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
        for line in table.lines().filter(|l| !l.starts_with('#')) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let number = |i: usize| parts.get(i).and_then(|p| p.parse::<f64>().ok());
            match parts.as_slice() {
                ["units", _] => m.units = number(1).unwrap_or(2048.0),
                ["ascent", _] => m.ascent = number(1).unwrap_or(0.0),
                ["descent", _] => m.descent = number(1).unwrap_or(0.0),
                [face, "kern", a, b, _] => {
                    if let (Some(a), Some(b), Some(v)) = (char_at(a), char_at(b), number(4)) {
                        m.faces[usize::from(*face == "bold")].kerning.insert((a, b), v);
                    }
                }
                [face, c, _] => {
                    if let (Some(c), Some(v)) = (char_at(c), number(2)) {
                        m.faces[usize::from(*face == "bold")].advances.insert(c, v);
                    }
                }
                _ => {}
            }
        }
        m
    }

    /// The width of a line of text in px.
    pub fn width(&self, text: &str, size: f64, bold: bool) -> f64 {
        let face = &self.faces[usize::from(bold)];
        let mut units = 0.0;
        let mut previous = None;
        for c in text.chars() {
            units += face.advances.get(&c).copied().unwrap_or(FALLBACK * self.units);
            if let Some(p) = previous {
                units += face.kerning.get(&(p, c)).copied().unwrap_or(0.0);
            }
            previous = Some(c);
        }
        units * size / self.units
    }

    /// Ascent and descent in px (for the extent of text, not for line placement).
    pub fn ascent(&self, size: f64) -> f64 {
        self.ascent * size / self.units
    }

    pub fn descent(&self, size: f64) -> f64 {
        self.descent * size / self.units
    }

    /// Breaks a text into lines no wider than `max_width` where possible: `\n` always breaks;
    /// otherwise a line breaks at the space before the word that would make it too wide. A
    /// word wider than `max_width` stays whole and sticks out.
    pub fn wrap(&self, text: &str, max_width: f64, size: f64, bold: bool) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
                if line.is_empty() {
                    line.push_str(word);
                    continue;
                }
                let longer = format!("{line} {word}");
                // The browser compares with a little slack against rounding.
                if self.width(&longer, size, bold) <= max_width + 0.01 {
                    line = longer;
                } else {
                    lines.push(std::mem::replace(&mut line, word.to_owned()));
                }
            }
            lines.push(line);
        }
        lines
    }
}

/// The distance of two lines of text in px.
pub fn line_height(size: f64) -> f64 {
    size * LINE_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ink widths draw.io gave these words (Liberation Sans, measured in its exports) are the
    /// advances minus the side bearings; the advances are a little wider.
    #[test]
    fn widths_match_draw_io() {
        let m = Metrics::get();
        let w = |t: &str, bold| (m.width(t, 12.0, bold) * 100.0).round() / 100.0;
        assert_eq!(w("geschwindigkeit", false), 84.71);
        assert_eq!(w("Fluggesellschaft", false), 86.71);
        assert_eq!(w("Fluggesellschaft", true), 94.69);
        assert_eq!(w("ID", false), 12.0);
        // Kerning: draw.io draws "AVAVAVAVAV" 72 px wide (ink), 80 px without the pairs.
        assert_eq!(w("AVAVAVAVAV", false), 72.02);
    }

    #[test]
    fn lines_break_as_in_draw_io() {
        let m = Metrics::get();
        let text = "ein sehr langer Name mit vielen Wörtern";
        // Entity (text box 98 px), ellipse 70 × 28 (68 px), rhombus 60 × 40 (58 px).
        assert_eq!(
            m.wrap(text, 98.0, 12.0, false),
            ["ein sehr langer", "Name mit vielen", "Wörtern"]
        );
        let narrow = ["ein sehr", "langer", "Name mit", "vielen", "Wörtern"];
        assert_eq!(m.wrap(text, 68.0, 12.0, false), narrow);
        assert_eq!(m.wrap(text, 58.0, 12.0, false), narrow);
        assert_eq!(m.wrap("Straße Hausnummer", 98.0, 12.0, false), ["Straße", "Hausnummer"]);
        assert_eq!(m.wrap("Fluggesellschaft", 98.0, 12.0, false), ["Fluggesellschaft"]);
        let long = "Donaudampfschifffahrtsgesellschaft";
        assert_eq!(m.wrap(long, 58.0, 12.0, false), [long]);
        assert_eq!(m.wrap("zwei\nZeilen", 98.0, 12.0, false), ["zwei", "Zeilen"]);
    }
}
