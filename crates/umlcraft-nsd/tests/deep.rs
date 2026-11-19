//! Structograms nested as deep as a file may (`read::MAX_DEPTH`) are read, written, laid out,
//! drawn and kept as content within a 1 MB stack, the main thread's on Windows, also in a
//! debug build.

use umlcraft_nsd::layout::Layout;
use umlcraft_nsd::read::MAX_DEPTH;
use umlcraft_nsd::{Metrics, content, read, svg, write};

const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<root xmlns:nsd="https://structorizer.fisch.lu" version="3.32-35" text="&#34;deep&#34;" comment="" color="ffffff" type="program" style="nice">
	<children>
"#;
const TAIL: &str = "\t</children>\n</root>\n";

/// `depth` levels: `depth - 1` structured elements, taking turns between a loop, an
/// alternative (nested in its `else`) and a case (nested in its last branch), with an
/// instruction at the bottom.
fn nested(depth: usize) -> String {
    let mut open = String::new();
    let mut close = Vec::new();
    for level in 0..depth - 1 {
        match level % 3 {
            0 => {
                open.push_str(
                    r#"<while text="&#34;c&#34;" comment="" color="ffffff" disabled="0"><qWhile>"#,
                );
                close.push("</qWhile></while>");
            }
            1 => {
                open.push_str(r#"<alternative text="&#34;a&#34;" comment="" color="ffffff" branch_colors="," disabled="0"><qTrue color="ffffff"></qTrue><qFalse>"#);
                close.push("</qFalse></alternative>");
            }
            _ => {
                open.push_str(r#"<case text="&#34;x&#34;,&#34;1&#34;,&#34;default&#34;" comment="" color="ffffff" branch_colors="," disabled="0"><qCase color="ffffff"></qCase><qCase>"#);
                close.push("</qCase></case>");
            }
        }
    }
    let bottom = r#"<instruction text="&#34;x &#60;- 1&#34;" comment="" color="ffffff" rotated="0" disabled="0"></instruction>"#;
    close.reverse();
    format!("{HEAD}{open}{bottom}{}{TAIL}", close.concat())
}

/// Runs `f` on a thread with a 1 MB stack.
fn on_small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new().stack_size(1 << 20).spawn(f).unwrap().join().unwrap();
}

#[test]
fn the_deepest_diagram_is_read_on_a_small_stack() {
    on_small_stack(|| {
        let d = read(&nested(MAX_DEPTH)).unwrap();
        assert_eq!(d.depth(), MAX_DEPTH);
        assert!(matches!(
            read(&nested(MAX_DEPTH + 1)),
            Err(umlcraft_nsd::ReadError::TooDeep { max: MAX_DEPTH, .. })
        ));
    });
}

#[test]
fn the_deepest_diagram_is_written_and_kept_on_a_small_stack() {
    on_small_stack(|| {
        let file = nested(MAX_DEPTH);
        let d = read(&file).unwrap();
        assert_eq!(read(&write(&d)).unwrap(), d);
        let n = content::normalize(&d);
        assert_eq!(n.depth(), MAX_DEPTH);
        assert_eq!(n.clone(), n);
    });
}

#[test]
fn the_deepest_diagram_is_laid_out_and_drawn_on_a_small_stack() {
    on_small_stack(|| {
        let d = read(&nested(MAX_DEPTH)).unwrap();
        let m = Metrics::for_platform();
        let layout = Layout::of(&d, m);
        assert!(layout.size.1 > 0);
        assert!(svg::render_layout(&d, &layout, m).contains("x &lt;- 1"));
    });
}

#[cfg(feature = "serde")]
#[test]
fn the_deepest_diagram_survives_json() {
    // serde_json's parser stops at 128 levels unless told otherwise (a project file is read by
    // `umlcraft-format`, which bounds the depth itself); a `Value` has no such limit.
    let d = content::normalize(&read(&nested(MAX_DEPTH)).unwrap());
    let json = serde_json::to_value(&d).unwrap();
    let back: umlcraft_nsd::Diagram = serde_json::from_value(json).unwrap();
    assert_eq!(back, d);
}
