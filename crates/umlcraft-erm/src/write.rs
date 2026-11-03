//! Writing `.ucerm`: pretty-printed JSON (two spaces), nodes, links and texts sorted by id,
//! default values left out, LF line ends and a final newline. Writing what was read gives the
//! same bytes.

use serde_json::Value;

use crate::model::{
    Caption, Diagram, Key, Label, LabelPos, LabelText, Line, Link, Max, Node, NodeKind, Point,
    Props, Rect, Style, Text, TextAlign,
};
use crate::record::{
    FileRecord, LabelRecord, LinkRecord, MaxRecord, MetaRecord, NodeRecord, PointRecord,
    PropsRecord, RectRecord, StyleRecord, TextRecord,
};
use crate::{FORMAT_ID, FORMAT_VERSION};

/// The diagram as file text.
pub fn write(d: &Diagram) -> String {
    let mut json = serde_json::to_string_pretty(&record(d)).expect("diagrams always serialize");
    json.push('\n');
    json
}

/// The diagram as JSON value, for embedding it into other documents.
pub fn to_value(d: &Diagram) -> Value {
    serde_json::to_value(record(d)).expect("diagrams always serialize")
}

fn record(d: &Diagram) -> FileRecord {
    let mut d = d.clone();
    d.sort();
    let m = &d.meta;
    FileRecord {
        format: FORMAT_ID.to_owned(),
        format_version: FORMAT_VERSION,
        meta: MetaRecord {
            name: m.name.clone(),
            author: m.author.clone(),
            created: m.created.clone(),
            modified: m.modified.clone(),
            origin: m.origin.clone(),
            notation: m.notation.clone(),
        },
        nodes: d.nodes.iter().map(node).collect(),
        links: d.links.iter().map(link).collect(),
        texts: d.texts.iter().map(text).collect(),
    }
}

fn rect(r: &Rect) -> RectRecord {
    RectRecord { x: r.x, y: r.y, width: r.width, height: r.height }
}

fn point(p: &Point) -> PointRecord {
    PointRecord { x: p.x, y: p.y }
}

fn node(n: &Node) -> NodeRecord {
    let mut r = NodeRecord {
        id: n.id.0.clone(),
        kind: n.kind.type_name().to_owned(),
        name: n.name.clone(),
        bounds: rect(&n.bounds),
        weak: false,
        key: None,
        derived: false,
        multivalued: false,
        identifying: false,
        total: false,
        disjoint: None,
        style: style(&n.style),
        props: props(&n.props),
    };
    match n.kind {
        NodeKind::Entity { weak } => r.weak = weak,
        NodeKind::AssociativeEntity => {}
        NodeKind::Attribute { key, derived, multivalued } => {
            r.key = key.map(|k| match k {
                Key::Primary => "primary".to_owned(),
                Key::Partial => "partial".to_owned(),
            });
            r.derived = derived;
            r.multivalued = multivalued;
        }
        NodeKind::Relationship { identifying } => r.identifying = identifying,
        NodeKind::IsA { total, disjoint } => {
            r.total = total;
            r.disjoint = disjoint;
        }
    }
    r
}

fn style(s: &Style) -> StyleRecord {
    StyleRecord {
        fill: s.fill.clone(),
        stroke: s.stroke.clone(),
        font_color: s.font_color.clone(),
        font_size: s.font_size,
        bold: s.bold,
        italic: s.italic,
        rounded: s.rounded,
        align: s.align.map(|a| {
            match a {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
            }
            .to_owned()
        }),
    }
}

fn props(p: &Props) -> PropsRecord {
    PropsRecord {
        physical_name: p.physical_name.clone(),
        datatype: p.datatype.clone(),
        length_precision: p.length_precision.clone(),
        not_null: p.not_null,
        default_value: p.default_value.clone(),
        comment: p.comment.clone(),
    }
}

fn position(r: &mut LabelRecord, p: &LabelPos) {
    (r.x, r.y, r.dx, r.dy) = (p.x, p.y, p.dx, p.dy);
}

fn label(l: &Label) -> LabelRecord {
    let mut r = LabelRecord { text: None, min: None, max: None, x: 0.0, y: 0.0, dx: 0.0, dy: 0.0 };
    match &l.text {
        LabelText::Text(t) => r.text = Some(t.clone()),
        LabelText::Cardinality(c) => {
            r.min = Some(c.min);
            r.max = Some(match c.max {
                Max::Count(n) => MaxRecord::Count(n),
                Max::Many => MaxRecord::Word("n".to_owned()),
            });
        }
    }
    position(&mut r, &l.pos);
    r
}

fn caption(c: &Caption) -> LabelRecord {
    let mut r = LabelRecord {
        text: Some(c.text.clone()),
        min: None,
        max: None,
        x: 0.0,
        y: 0.0,
        dx: 0.0,
        dy: 0.0,
    };
    position(&mut r, &c.pos);
    r
}

fn link(l: &Link) -> LinkRecord {
    LinkRecord {
        id: l.id.0.clone(),
        source: l.source.0.clone(),
        target: l.target.0.clone(),
        source_port: l.source_port.as_ref().map(point),
        target_port: l.target_port.as_ref().map(point),
        label: l.label.as_ref().map(label),
        role: l.role.as_ref().map(caption),
        line: match l.line {
            Line::Solid => None,
            Line::Dotted => Some("dotted".to_owned()),
            Line::Double => Some("double".to_owned()),
        },
        super_type: l.super_type,
        waypoints: l.waypoints.iter().map(point).collect(),
    }
}

fn text(t: &Text) -> TextRecord {
    TextRecord {
        id: t.id.0.clone(),
        bounds: rect(&t.bounds),
        text: t.text.clone(),
        style: style(&t.style),
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;
    use crate::model::{Cardinality, Id};

    #[test]
    fn writes_sorted_with_defaults_left_out() {
        let mut d = Diagram::new("Beispiel");
        let node = |id: &str, kind, name: &str, x| Node {
            id: Id::from(id),
            kind,
            name: name.to_owned(),
            bounds: Rect::new(x, 0.0, 100.0, 40.0),
            style: Style::default(),
            props: Props::default(),
        };
        let mut e = node("b", NodeKind::entity(), "Kunde", 0.0);
        e.style.fill = Some("#dae8fc".to_owned());
        e.props.physical_name = Some("kunde".to_owned());
        e.props.not_null = true;
        d.nodes.push(e);
        d.nodes.push(node("a", NodeKind::relationship(), "leitet", 150.5));
        let mut l = Link::new(Id::from("a"), Id::from("b"));
        l.id = Id::from("l");
        l.line = Line::Dotted;
        l.label = Some(Label {
            text: LabelText::Cardinality(Cardinality::new(0, Max::Many)),
            pos: LabelPos { x: 0.0, y: 14.0, dx: 0.0, dy: 0.0 },
        });
        l.role = Some(Caption { text: "chef".to_owned(), pos: LabelPos::default() });
        d.links.push(l);
        let mut m = Link::new(Id::from("a"), Id::from("b"));
        m.id = Id::from("m");
        d.links.push(m);
        let expected = r##"{
  "format": "umlcraft-erm",
  "formatVersion": 1,
  "meta": {
    "name": "Beispiel"
  },
  "nodes": [
    {
      "id": "a",
      "type": "relationship",
      "name": "leitet",
      "bounds": {
        "x": 150.5,
        "y": 0,
        "width": 100,
        "height": 40
      }
    },
    {
      "id": "b",
      "type": "entity",
      "name": "Kunde",
      "bounds": {
        "x": 0,
        "y": 0,
        "width": 100,
        "height": 40
      },
      "style": {
        "fill": "#dae8fc"
      },
      "props": {
        "physicalName": "kunde",
        "notNull": true
      }
    }
  ],
  "links": [
    {
      "id": "l",
      "source": "a",
      "target": "b",
      "label": {
        "min": 0,
        "max": "n",
        "y": 14
      },
      "role": {
        "text": "chef"
      },
      "line": "dotted"
    },
    {
      "id": "m",
      "source": "a",
      "target": "b"
    }
  ],
  "texts": []
}
"##;
        assert_eq!(write(&d), expected);
        let (back, _) = crate::read(expected).unwrap();
        assert_eq!(write(&back), expected);
        assert_eq!(crate::read_value(to_value(&d)).unwrap().0, back);
    }
}
