//! Explores the entity store of an `.asta` file, to learn Astah's data structures.
//!
//! - `asta-dump file.asta` prints every object, one per line.
//! - `asta-dump file.asta <id> [depth]` prints the object `#id` as a tree.
//! - `asta-dump file.asta root [depth]` starts the tree at the project model.

use std::fmt::Write as _;
use std::path::PathBuf;

use umlcraft_asta::jser::{Annotation, Content, Graph, ObjId, Object, Value};

fn short(class: &str) -> &str {
    class.rsplit('.').next().unwrap_or(class)
}

/// A one-line rendering for values that read best inline.
fn inline(graph: &Graph, v: &Value) -> Option<String> {
    let id = match v {
        Value::Null => return Some("null".into()),
        Value::Object(id) => *id,
        other => return Some(format!("{other:?}")),
    };
    match graph.object(id) {
        Object::String(s) => return Some(format!("{s:?}")),
        Object::Enum { constant, .. } => return Some(constant.clone()),
        _ => {}
    }
    let class = short(graph.class_name(id)?);
    let field = |name| graph.field(id, name).and_then(|v| inline(graph, v));
    match class {
        "UName" | "UUninterpreted" | "UExpression" | "UBooleanExpression" => field("body"),
        "UVisibilityKind"
        | "UAggregationKind"
        | "UScopeKind"
        | "UPseudostateKind"
        | "UChangeableKind"
        | "UParameterDirectionKind"
        | "UOrderingKind"
        | "UCallConcurrencyKind" => field("label").or_else(|| Some(format!("{class}?"))),
        "Pnt2d" | "Vec2d" => Some(format!("({}, {})", field("x")?, field("y")?)),
        "Long" | "Integer" | "Double" | "Boolean" | "Float" => field("value"),
        "ArrayList" | "HashMap" | "LinkedHashMap" | "HashSet" | "Hashtable" | "Vector"
            if items(graph, id).is_empty() =>
        {
            Some("[]".into())
        }
        _ => None,
    }
}

/// Elements of a collection; map entries come as key, value, key, value …
fn items(graph: &Graph, id: ObjId) -> Vec<Value> {
    let class = graph.class_name(id).unwrap_or_default();
    if let Some(array) = graph.array(id) {
        return array.to_vec();
    }
    let Some(annotation) = graph.annotation(id, class) else {
        // Subclasses such as LinkedHashMap keep the data in the superclass annotation.
        return match class {
            "java.util.LinkedHashMap" => collection(graph, id, "java.util.HashMap"),
            "java.util.LinkedHashSet" => collection(graph, id, "java.util.HashSet"),
            _ => Vec::new(),
        };
    };
    let _ = annotation;
    collection(graph, id, class)
}

fn collection(graph: &Graph, id: ObjId, class: &str) -> Vec<Value> {
    let Some(annotation) = graph.annotation(id, class) else { return Vec::new() };
    let mut a = Annotation::new(annotation);
    a.int();
    let mut out = Vec::new();
    while let Some(v) = a.value() {
        out.push(v.clone());
    }
    out
}

const NOISE: &[&str] = &["ex_changed", "unsolvedFlag", "version", "uid", "changeId", "id"];

fn tree(
    graph: &Graph,
    v: &Value,
    depth: usize,
    indent: usize,
    out: &mut String,
    seen: &mut Vec<ObjId>,
) {
    if let Some(s) = inline(graph, v) {
        out.push_str(&s);
        return;
    }
    let Value::Object(id) = *v else { return };
    let class = short(graph.class_name(id).unwrap_or("?"));
    let _ = write!(out, "#{} {class}", id.0);
    if depth == 0 || seen.contains(&id) {
        return;
    }
    seen.push(id);
    let pad = "  ".repeat(indent + 1);
    match graph.object(id) {
        Object::Instance { data, .. }
            if !matches!(
                class,
                "ArrayList" | "HashMap" | "LinkedHashMap" | "HashSet" | "Hashtable" | "Vector"
            ) =>
        {
            for class_data in data {
                for (name, value) in &class_data.fields {
                    if NOISE.contains(&name.as_str())
                        || matches!(inline(graph, value).as_deref(), Some("null" | "[]"))
                    {
                        continue;
                    }
                    let _ = write!(out, "\n{pad}{name} = ");
                    tree(graph, value, depth - 1, indent + 1, out, seen);
                }
                if !class_data.annotation.is_empty() && class_data.fields.is_empty() {
                    for item in &class_data.annotation {
                        let _ = write!(out, "\n{pad}[{}] ", short(&class_data.class));
                        match item {
                            Content::Block(b) => {
                                let _ = write!(out, "block{b:02x?}");
                            }
                            Content::Value(value) => {
                                tree(graph, value, depth - 1, indent + 1, out, seen)
                            }
                        }
                    }
                }
            }
        }
        _ => {
            for item in items(graph, id) {
                let _ = write!(out, "\n{pad}- ");
                tree(graph, &item, depth - 1, indent + 1, out, seen);
            }
        }
    }
}

/// The ownership tree with class and name of every element, plus what each element keeps in
/// its non-empty list fields.
fn outline(graph: &Graph, id: ObjId, indent: usize, out: &mut String) {
    let name = graph.field(id, "name").and_then(|v| inline(graph, v)).unwrap_or_default();
    let _ = write!(
        out,
        "{}#{} {} {name}",
        "  ".repeat(indent),
        id.0,
        short(graph.class_name(id).unwrap_or("?"))
    );
    if let Some(Value::Object(type_name)) = graph.field(id, "type") {
        let _ = write!(out, " type={:?}", graph.string(*type_name).unwrap_or_default());
    }
    out.push('\n');
    let mut children = Vec::new();
    if let Some(Value::Object(list)) = graph.field(id, "ownedElementOwnership") {
        for ownership in items(graph, *list) {
            if let Some(child) = ownership
                .as_object()
                .and_then(|o| graph.field(o, "ownedElement"))
                .and_then(Value::as_object)
            {
                children.push(child);
            }
        }
    }
    for child in children {
        outline(graph, child, indent + 1, out);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path: PathBuf = args.next().expect("usage: asta-dump <file.asta> [id|root] [depth]").into();
    let bytes = umlcraft_asta::read_entity_store(&path).expect("read");
    let graph = umlcraft_asta::parse_entity_store(&bytes).expect("parse");
    let mut out = String::new();
    let model = (0..graph.objects.len())
        .map(ObjId)
        .find(|&id| graph.class_name(id).is_some_and(|c| c.ends_with("UModelImp")))
        .expect("no model");
    if let Some(start) = args.next() {
        if start == "outline" {
            outline(&graph, model, 0, &mut out);
            print!("{out}");
            return;
        }
        let depth = args.next().map_or(3, |d| d.parse().expect("depth"));
        let id = if start == "root" {
            (0..graph.objects.len())
                .map(ObjId)
                .find(|&id| graph.class_name(id).is_some_and(|c| c.ends_with("UModelImp")))
                .expect("no model")
        } else {
            ObjId(start.trim_start_matches('#').parse().expect("object id"))
        };
        tree(&graph, &Value::Object(id), depth, 0, &mut out, &mut Vec::new());
        out.push('\n');
        print!("{out}");
        return;
    }
    for (i, object) in graph.objects.iter().enumerate() {
        let id = ObjId(i);
        let Object::Instance { data, .. } = object else { continue };
        let _ = write!(out, "#{i} {}", graph.class_name(id).unwrap_or("?"));
        for class_data in data {
            for (name, v) in &class_data.fields {
                let shown = inline(&graph, v).unwrap_or_else(|| match v {
                    Value::Object(o) => {
                        format!("#{} {}", o.0, short(graph.class_name(*o).unwrap_or("?")))
                    }
                    other => format!("{other:?}"),
                });
                let _ = write!(out, " {name}={shown}");
            }
        }
        out.push('\n');
    }
    print!("{out}");
}
