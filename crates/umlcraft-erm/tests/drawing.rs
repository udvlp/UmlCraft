//! The SVG images of the fixtures stay as they are (`tests/fixtures/*.svg`). After an intended
//! change to the drawing, `UMLCRAFT_ERM_BLESS=1 cargo test -p umlcraft-erm --test drawing`
//! writes them anew; look at the difference before committing.

use std::path::Path;

use pretty_assertions::assert_eq;
use quick_xml::Reader;
use quick_xml::events::Event;
use umlcraft_erm::{read, svg};

fn check(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let (diagram, _) =
        read(&std::fs::read_to_string(dir.join(format!("{name}.ucerm"))).unwrap()).unwrap();
    let ours = svg::render(&diagram);
    // Well-formed XML.
    let mut reader = Reader::from_str(&ours);
    while !matches!(reader.read_event().unwrap(), Event::Eof) {}
    let path = dir.join(format!("{name}.svg"));
    if std::env::var_os("UMLCRAFT_ERM_BLESS").is_some() {
        std::fs::write(&path, &ours).unwrap();
    }
    assert_eq!(ours, std::fs::read_to_string(&path).unwrap(), "{name}.svg");
}

#[test]
fn flight_example() {
    check("flug");
}

#[test]
fn all_chen_shapes() {
    check("chen-extra");
}
