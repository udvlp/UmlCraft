//! Diagrams as PNG images: the SVG export, rasterized.

use gpui_kit::*;
use resvg::{tiny_skia, usvg};
use rust_i18n::t;

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
        .ok_or_else(|| anyhow::anyhow!(rust_i18n::t!("error.image_empty").to_string()))?;
    let mut pixmap = tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| anyhow::anyhow!(rust_i18n::t!("error.image_too_large").to_string()))?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok(pixmap.encode_png()?)
}

/// Puts an SVG export on the clipboard together with its PNG, rasterized at `scale` as
/// [`from_svg`] takes it. Word and PowerPoint paste the SVG as a vector image, programs without
/// SVG take the PNG.
pub fn copy<V: 'static>(svg: String, scale: f32, window: &mut Window, cx: &mut Context<V>) {
    cx.spawn_in(window, async move |_, cx| {
        let (svg, png) = cx
            .background_executor()
            .spawn(async move {
                let png = from_svg(&svg, scale);
                (svg, png)
            })
            .await;
        cx.update(|window, cx| match png {
            Ok(png) => cx.write_to_clipboard(ClipboardItem {
                entries: vec![
                    ClipboardEntry::Image(Image::from_bytes(ImageFormat::Svg, svg.into_bytes())),
                    ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, png)),
                ],
            }),
            Err(err) => {
                let rx = window.prompt(
                    PromptLevel::Critical,
                    &t!("dialog.copy_image_failed"),
                    Some(&err.to_string()),
                    &[t!("dialog.ok").as_ref()],
                    cx,
                );
                cx.spawn(async move |_| rx.await.ok()).detach();
            }
        })
        .ok();
    })
    .detach();
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
