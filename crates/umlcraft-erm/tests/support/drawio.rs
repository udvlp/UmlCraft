//! Test helper, not a feature: reads uncompressed draw.io files with ER shapes into our model
//! (as the script that made the fixtures does), and the SVG images draw.io exports of them.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};
use umlcraft_erm::{
    Diagram, Id, Key, Label, LabelPos, LabelText, Line, Link, Node, NodeKind, Point, Props, Rect,
    Style, Text,
};

fn attrs(e: &BytesStart) -> HashMap<String, String> {
    e.attributes()
        .filter_map(Result::ok)
        .map(|a| {
            let key = a.key.as_ref().to_owned();
            let value = unescape(&a.value).unwrap().into_owned();
            (key, value)
        })
        .collect()
}

fn style(s: &str) -> HashMap<String, String> {
    s.split(';')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.to_owned(), v.to_owned()),
            None => (p.to_owned(), String::new()),
        })
        .collect()
}

fn plain(html: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    let html = html.replace("<br>", "\n").replace("&nbsp;", " ");
    for c in html.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            c if !tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
}

#[derive(Default, Clone)]
struct Cell {
    id: String,
    value: String,
    style: HashMap<String, String>,
    vertex: bool,
    edge: bool,
    parent: String,
    source: String,
    target: String,
    geometry: HashMap<String, String>,
    offset: Option<Point>,
    points: Vec<Point>,
}

fn number(map: &HashMap<String, String>, key: &str) -> f64 {
    map.get(key).and_then(|v| v.parse().ok()).unwrap_or(0.0)
}

/// What the comparison skips: lines draw.io routes itself and child labels at line ends.
pub struct Read {
    pub diagram: Diagram,
    /// Links drawn with `edgeStyle=orthogonalEdgeStyle` (we draw them straight).
    pub routed: Vec<Id>,
    /// The cell holding each link's label in the SVG: the link or its `edgeLabel` child.
    pub label_cell: HashMap<Id, String>,
}

/// Reads `<mxfile>` with a plain `<mxGraphModel>`, or a bare `<mxGraphModel>`.
pub fn read(xml: &str) -> Read {
    let mut reader = Reader::from_str(xml);
    let mut cells: Vec<Cell> = Vec::new();
    let mut in_points = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let a = attrs(&e);
                match e.name().as_ref() {
                    "mxCell" => cells.push(Cell {
                        id: a.get("id").cloned().unwrap_or_default(),
                        value: a.get("value").cloned().unwrap_or_default(),
                        style: style(a.get("style").map_or("", String::as_str)),
                        vertex: a.get("vertex").is_some_and(|v| v == "1"),
                        edge: a.get("edge").is_some_and(|v| v == "1"),
                        parent: a.get("parent").cloned().unwrap_or_default(),
                        source: a.get("source").cloned().unwrap_or_default(),
                        target: a.get("target").cloned().unwrap_or_default(),
                        ..Cell::default()
                    }),
                    "mxGeometry" => {
                        if let Some(c) = cells.last_mut() {
                            c.geometry = a;
                        }
                    }
                    "Array" => in_points = a.get("as").is_some_and(|v| v == "points"),
                    "mxPoint" => {
                        if let Some(c) = cells.last_mut() {
                            let p = Point::new(number(&a, "x"), number(&a, "y"));
                            if a.get("as").is_some_and(|v| v == "offset") {
                                c.offset = Some(p);
                            } else if in_points {
                                c.points.push(p);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.name().as_ref() == "Array" => in_points = false,
            _ => {}
        }
    }
    let by_id: HashMap<String, Cell> = cells.iter().map(|c| (c.id.clone(), c.clone())).collect();
    let mut d = Diagram::new("drawio");
    let mut routed = Vec::new();
    let mut label_cell = HashMap::new();
    for c in &cells {
        let s = &c.style;
        if c.vertex && by_id.get(&c.parent).is_some_and(|p| p.edge) {
            continue; // labels of edges, below
        }
        if c.vertex {
            let g = &c.geometry;
            let bounds =
                Rect::new(number(g, "x"), number(g, "y"), number(g, "width"), number(g, "height"));
            let font_style = number(s, "fontStyle") as u32;
            // Formatting inside HTML values (the flight example's free text:
            // <font color><b>…</b></font>).
            let html_color = c.value.split("color=\"").nth(1).and_then(|r| r.split('"').next());
            let st = Style {
                fill: s.get("fillColor").filter(|v| v.starts_with('#')).cloned(),
                stroke: s.get("strokeColor").filter(|v| v.starts_with('#')).cloned(),
                font_color: s.get("fontColor").cloned().or(html_color.map(str::to_owned)),
                font_size: s.get("fontSize").and_then(|v| v.parse().ok()),
                bold: font_style & 1 != 0 || c.value.contains("<b>"),
                italic: font_style & 2 != 0 || c.value.contains("<i>"),
                rounded: s.get("rounded").is_some_and(|v| v == "1"),
                align: None,
            };
            if s.contains_key("text") {
                d.texts.push(Text {
                    id: Id(c.id.clone()),
                    bounds,
                    text: plain(&c.value),
                    style: st,
                });
                continue;
            }
            let double = s.get("double").is_some_and(|v| v == "1");
            let kind = if s.get("shape").is_some_and(|v| v == "rhombus") {
                NodeKind::Relationship { identifying: double }
            } else if s.contains_key("ellipse") {
                NodeKind::Attribute {
                    key: (font_style & 4 != 0).then_some(if font_style & 16 != 0 {
                        Key::Partial
                    } else {
                        Key::Primary
                    }),
                    derived: s.get("dashed").is_some_and(|v| v == "1"),
                    multivalued: s.get("shape").is_some_and(|v| v == "doubleEllipse"),
                }
            } else if s.get("shape").is_some_and(|v| v == "associativeEntity") {
                NodeKind::AssociativeEntity
            } else if s.contains_key("triangle") {
                NodeKind::IsA { total: false, disjoint: None }
            } else {
                NodeKind::Entity { weak: double }
            };
            d.nodes.push(Node {
                id: Id(c.id.clone()),
                kind,
                name: plain(&c.value),
                bounds,
                style: st,
                props: Props::default(),
            });
        } else if c.edge {
            let mut link = Link::new(Id(c.source.clone()), Id(c.target.clone()));
            link.id = Id(c.id.clone());
            let port = |x: &str, y: &str| {
                s.contains_key(x).then(|| Point::new(number(s, x), number(s, y)))
            };
            link.source_port = port("exitX", "exitY");
            link.target_port = port("entryX", "entryY");
            link.waypoints = c.points.clone();
            link.line = if s.get("shape").is_some_and(|v| v == "link") {
                Line::Double
            } else if s.get("dashed").is_some_and(|v| v == "1") {
                Line::Dotted
            } else {
                Line::Solid
            };
            let label = |text: &str, g: &HashMap<String, String>, off: Option<Point>| Label {
                text: LabelText::Text(text.to_owned()),
                pos: LabelPos {
                    x: number(g, "x"),
                    y: number(g, "y"),
                    dx: off.map_or(0.0, |p| p.x),
                    dy: off.map_or(0.0, |p| p.y),
                },
            };
            if !plain(&c.value).is_empty() {
                link.label = Some(label(&plain(&c.value), &c.geometry, c.offset));
                label_cell.insert(link.id.clone(), c.id.clone());
            }
            // Labels added in today's GUI: children with the style `edgeLabel`.
            for child in
                cells.iter().filter(|x| x.parent == c.id && x.style.contains_key("edgeLabel"))
            {
                link.label = Some(label(&plain(&child.value), &child.geometry, child.offset));
                label_cell.insert(link.id.clone(), child.id.clone());
            }
            if s.get("edgeStyle").is_some_and(|v| v != "none") {
                routed.push(link.id.clone());
            }
            d.links.push(link);
        }
    }
    Read { diagram: d, routed, label_cell }
}

/// What draw.io's SVG export says about one cell.
#[derive(Default, Debug)]
pub struct SvgCell {
    /// Corner of the first rectangle (vertices).
    pub rect: Option<Point>,
    /// First and last point of the first path (edges).
    pub path: Option<(Point, Point)>,
    /// Center of an edge label: its 1 px wide HTML box.
    pub label: Option<Point>,
}

pub struct Svg {
    pub width: f64,
    pub height: f64,
    pub cells: HashMap<String, SvgCell>,
}

fn css(style: &str, key: &str) -> Option<f64> {
    style.split(';').find_map(|p| {
        let (k, v) = p.split_once(':')?;
        (k.trim() == key).then(|| v.trim().trim_end_matches("px").parse().ok())?
    })
}

pub fn read_svg(xml: &str) -> Svg {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut svg = Svg { width: 0.0, height: 0.0, cells: HashMap::new() };
    loop {
        let event = reader.read_event().unwrap();
        let (e, empty) = match &event {
            Event::Start(e) => (e.clone(), false),
            Event::Empty(e) => (e.clone(), true),
            Event::End(_) => {
                stack.pop();
                continue;
            }
            Event::Eof => break,
            _ => continue,
        };
        let a = attrs(&e);
        let cell = a.get("data-cell-id").cloned().or_else(|| stack.last().cloned().flatten());
        let name = e.name().as_ref().to_owned();
        let name = name.as_str();
        if name == "svg" {
            let view: Vec<f64> = a["viewBox"].split(' ').filter_map(|v| v.parse().ok()).collect();
            (svg.width, svg.height) = (view[2], view[3]);
        }
        if let Some(id) = &cell {
            let entry = svg.cells.entry(id.clone()).or_default();
            match name {
                "rect" if entry.rect.is_none() => {
                    entry.rect = Some(Point::new(number(&a, "x"), number(&a, "y")));
                }
                "path" if entry.path.is_none() => {
                    let n: Vec<f64> = a["d"]
                        .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    if n.len() >= 4 {
                        let k = n.len();
                        entry.path = Some((Point::new(n[0], n[1]), Point::new(n[k - 2], n[k - 1])));
                    }
                }
                "div" => {
                    let st = a.get("style").cloned().unwrap_or_default();
                    if css(&st, "width") == Some(1.0)
                        && entry.label.is_none()
                        && let (Some(x), Some(y)) =
                            (css(&st, "margin-left"), css(&st, "padding-top"))
                    {
                        entry.label = Some(Point::new(x, y));
                    }
                }
                _ => {}
            }
        }
        if !empty {
            stack.push(if a.contains_key("data-cell-id") {
                a.get("data-cell-id").cloned()
            } else {
                cell
            });
        }
    }
    svg
}
