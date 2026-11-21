//! A diagram's own colors in the dark theme.
//!
//! The fill, font and line colors a presentation stores are meant for a light background: they
//! are what Astah shows and what the exports draw, and they stay so. The dark canvas draws a
//! counterpart of each instead, worked out when painting: the same hue, the lightness mirrored,
//! so that white turns into the dark theme's default fill and black into its default ink, and a
//! color as far from those defaults in the dark theme as it was in the light one.

use gpui_kit::{Hsla, Rgba};

/// The contrast texts keep to what they stand on (WCAG AA for normal text).
pub(crate) const TEXT_CONTRAST: f32 = 4.5;
/// The contrast lines keep to the background (WCAG for graphical objects).
pub(crate) const LINE_CONTRAST: f32 = 3.0;

/// How much of the light theme's range of lightness a color's distance from its default spans
/// in the dark theme.
const SPAN: f32 = 0.6;
/// How much of its saturation a fill keeps: full colors would glare on a dark canvas.
const FILL_SATURATION: f32 = 0.5;
/// The step in lightness when a color is moved to reach a contrast.
const STEP: f32 = 0.01;

/// The dark theme's counterpart of a fill chosen for the light theme. `default` is the dark
/// theme's own fill, and `text` the ink that must stay readable on the result.
pub(crate) fn fill(chosen: Hsla, default: Hsla, text: Hsla) -> Hsla {
    let mut out = Hsla {
        h: chosen.h,
        s: chosen.s * FILL_SATURATION,
        l: (default.l + (1.0 - chosen.l) * SPAN).min(1.0),
        a: chosen.a,
    };
    // Darker until the text stands out.
    while out.l > 0.0 && contrast(text, out) < TEXT_CONTRAST {
        out.l = (out.l - STEP).max(0.0);
    }
    out
}

/// The dark theme's counterpart of a font or line color chosen for the light theme. `default`
/// is the dark theme's own ink; the result keeps `min` contrast to each of `grounds`.
pub(crate) fn ink(chosen: Hsla, default: Hsla, grounds: &[Hsla], min: f32) -> Hsla {
    let mut out = Hsla { l: (default.l - chosen.l * SPAN).max(0.0), ..chosen };
    // Lighter until it stands out from everything it is drawn on.
    while out.l < 1.0 && grounds.iter().any(|&g| contrast(out, g) < min) {
        out.l = (out.l + STEP).min(1.0);
    }
    out
}

/// The WCAG contrast ratio of two opaque colors, from 1 to 21.
pub(crate) fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The relative luminance of a color, as WCAG defines it.
fn luminance(c: Hsla) -> f32 {
    let Rgba { r, g, b, .. } = Rgba::from(c);
    let linear = |v: f32| if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(l: f32) -> Hsla {
        Hsla { h: 0.0, s: 0.0, l, a: 1.0 }
    }

    fn rgb(hex: u32) -> Hsla {
        let channel = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.0;
        Hsla::from(Rgba { r: channel(16), g: channel(8), b: channel(0), a: 1.0 })
    }

    // A dark theme much like the app's: a dark gray box on a darker canvas, light text.
    const BACKGROUND: f32 = 0.09;
    const FILL: f32 = 0.16;
    const TEXT: f32 = 0.98;

    fn dark_fill(chosen: Hsla) -> Hsla {
        fill(chosen, gray(FILL), gray(TEXT))
    }

    fn dark_ink(chosen: Hsla) -> Hsla {
        ink(chosen, gray(TEXT), &[gray(FILL), gray(BACKGROUND)], TEXT_CONTRAST)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn white_fill_is_the_default_fill() {
        assert!(close(dark_fill(gray(1.0)).l, FILL));
    }

    #[test]
    fn black_ink_is_the_default_ink() {
        assert!(close(dark_ink(gray(0.0)).l, TEXT));
    }

    #[test]
    fn gray_fill_stands_out_from_the_default() {
        // What a diagram showed before, set off in gray from what is new.
        let old = dark_fill(rgb(0xc0c0c0));
        assert_eq!(old.s, 0.0);
        assert!(old.l > FILL + 0.1, "{old:?}");
        assert!(contrast(gray(TEXT), old) >= TEXT_CONTRAST);
    }

    #[test]
    fn fills_keep_their_hue_and_order() {
        let blue = rgb(0xccddff);
        let pale = dark_fill(blue);
        assert!(close(pale.h, blue.h));
        let strong = dark_fill(rgb(0x99bbee));
        assert!(strong.l > pale.l, "farther from white in light, farther from the default in dark");
    }

    #[test]
    fn every_fill_keeps_the_text_readable() {
        for hex in [0x000000, 0xff0000, 0xffff00, 0x00ff00, 0x0000ff, 0x808080, 0x336699] {
            let out = dark_fill(rgb(hex));
            assert!(contrast(gray(TEXT), out) >= TEXT_CONTRAST, "{hex:06x}: {out:?}");
        }
    }

    #[test]
    fn every_ink_stays_readable() {
        for hex in [0xffffff, 0xffff99, 0x800000, 0x0000ff, 0x008000, 0xc0c0c0] {
            let out = dark_ink(rgb(hex));
            assert!(contrast(out, gray(FILL)) >= TEXT_CONTRAST, "{hex:06x}: {out:?}");
            assert!(contrast(out, gray(BACKGROUND)) >= TEXT_CONTRAST, "{hex:06x}: {out:?}");
        }
    }

    #[test]
    fn ink_stays_readable_on_a_fill_of_its_hue() {
        let fill = dark_fill(rgb(0x99ccff));
        let out = ink(rgb(0x0000ff), gray(TEXT), &[fill, gray(BACKGROUND)], TEXT_CONTRAST);
        assert!(contrast(out, fill) >= TEXT_CONTRAST, "{out:?} on {fill:?}");
    }

    #[test]
    fn dark_red_turns_light_red() {
        let red = rgb(0x990000);
        let out = dark_ink(red);
        assert!(close(out.h, red.h));
        assert!(out.l > 0.6, "{out:?}");
    }

    #[test]
    fn contrast_spans_one_to_twenty_one() {
        assert!(close(contrast(gray(0.0), gray(1.0)), 21.0));
        assert!(close(contrast(gray(0.5), gray(0.5)), 1.0));
    }
}
