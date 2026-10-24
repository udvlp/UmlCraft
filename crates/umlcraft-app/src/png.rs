//! Diagrams as PNG images: the SVG export, rasterized.

use resvg::{tiny_skia, usvg};

/// Pixels per logical unit. Twice the canvas at 100 %, so images stay sharp on high-resolution
/// screens and in print.
pub const SCALE: f32 = 2.0;

/// Rasterizes an SVG export at `scale` pixels per logical unit with the system's fonts.
pub fn from_svg(svg: &str, scale: f32) -> anyhow::Result<Vec<u8>> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(svg, &options)?;
    // The export's size is in pixels at 96 dpi.
    let scale = scale / umlcraft_notation::svg::SCALE;
    let size = tree
        .size()
        .to_int_size()
        .scale_by(scale)
        .ok_or_else(|| anyhow::anyhow!("the image would be empty"))?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow::anyhow!("the image is too large"))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

#[cfg(test)]
mod tests {
    use umlcraft_model::sample::order_system;
    use umlcraft_notation::scene::{ApproxMeasure, Preview, build};
    use umlcraft_notation::svg;

    #[test]
    fn diagrams_become_png_images_at_twice_their_size() {
        let (doc, ids) = order_system();
        let scene = build(doc.model(), ids.class_diagram, &Preview::default(), &ApproxMeasure);
        let svg = svg::render(&scene, &ApproxMeasure);
        let png = super::from_svg(&svg, super::SCALE).unwrap();
        assert_eq!(&png[1..4], b"PNG");
        // The IHDR chunk holds width and height right after the signature.
        let dimension = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
        let view_box = svg.split("viewBox=\"").nth(1).unwrap().split('"').next().unwrap();
        let v: Vec<f32> = view_box.split(' ').map(|n| n.parse().unwrap()).collect();
        // Within the pixel the export's own size was rounded up to.
        assert!(dimension(16).abs_diff((v[2] * 2.0).round() as u32) <= 2);
        assert!(dimension(20).abs_diff((v[3] * 2.0).round() as u32) <= 2);
    }
}
