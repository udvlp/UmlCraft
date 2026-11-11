//! Text widths in Arial, the diagram font, without the font at hand: the advance of each
//! character added up, as Java lays out "Dialog" without kerning. For drawing without the app
//! (the MCP server's own projects, exports from the command line), where an estimate per
//! character would make boxes and underlines too wide.

use crate::scene::TextMeasure;

/// Advances of the printable ASCII characters from the space on, in thousandths of the font
/// size: Arial's, which match Helvetica's.
const REGULAR: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, // ' '../
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0..9
    278, 278, 584, 584, 584, 556, 1015, // :..@
    667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, // A..M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // N..Z
    278, 278, 278, 469, 556, 333, // [..`
    556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, // a..m
    556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, // n..z
    334, 260, 334, 584, // {..~
];

const BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278, // ' '../
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, // 0..9
    333, 333, 584, 584, 584, 611, 975, // :..@
    722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, // A..M
    722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, // N..Z
    333, 278, 333, 584, 556, 333, // [..`
    556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, // a..m
    611, 611, 611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, // n..z
    389, 280, 389, 584, // {..~
];

/// Measures text in Arial.
#[derive(Clone, Copy, Debug, Default)]
pub struct ArialMeasure;

/// The advance of `c` in thousandths of the font size.
fn advance(c: char, bold: bool) -> u16 {
    let table = if bold { &BOLD } else { &REGULAR };
    let ascii = |c: char| table[c as usize - 0x20];
    match c {
        ' '..='~' => ascii(c),
        // Letters with accents and umlauts are as wide as the letters they are made of.
        'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => ascii('a'),
        'ö' | 'ò' | 'ó' | 'ô' | 'õ' => ascii('o'),
        'ü' | 'ù' | 'ú' | 'û' => ascii('u'),
        'é' | 'è' | 'ê' | 'ë' => ascii('e'),
        'í' | 'ì' | 'î' | 'ï' => ascii('i'),
        'ç' => ascii('c'),
        'ñ' => ascii('n'),
        'Ä' | 'À' | 'Á' | 'Â' | 'Ã' | 'Å' => ascii('A'),
        'Ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' => ascii('O'),
        'Ü' | 'Ù' | 'Ú' | 'Û' => ascii('U'),
        'É' | 'È' | 'Ê' | 'Ë' => ascii('E'),
        'ß' => 611,
        '«' | '»' => 556,
        '…' => 1000,
        '–' => 556,
        '—' => 1000,
        '×' => 584,
        '°' => 400,
        '\u{00a0}' => ascii(' '),
        // Wide scripts take a whole em, anything else about a letter's width.
        c if ('\u{2e80}'..='\u{9fff}').contains(&c) || ('\u{ac00}'..='\u{d7af}').contains(&c) => {
            1000
        }
        _ if bold => 611,
        _ => 556,
    }
}

impl TextMeasure for ArialMeasure {
    fn width(&self, text: &str, font_size: f32, bold: bool) -> f32 {
        let thousandths: u32 = text.chars().map(|c| u32::from(advance(c, bold))).sum();
        thousandths as f32 * font_size / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_are_arials() {
        // Measured in exported SVGs (Arial 12).
        let width = |text: &str| ArialMeasure.width(text, 12.0, false);
        assert!((width("- naechste : int") - 79.0).abs() < 0.5, "{}", width("- naechste : int"));
        assert!((width("+ getNaechste() : int") - 109.0).abs() < 1.0);
        assert!(ArialMeasure.width("Gerät", 12.0, true) > width("Gerät"));
    }
}
