//! Counts the presentation style keys and values of `.asta` files, to see which of Astah's
//! presentation properties occur: `asta-styles a.asta b.asta …`. With `--key <key>` it lists
//! each value of that key with the presentation class and the files it occurs in.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use umlcraft_asta::jser::{Graph, ObjId, Value};

fn styles(graph: &Graph) -> Vec<(ObjId, Vec<(String, String)>)> {
    (0..graph.objects.len())
        .map(ObjId)
        .filter_map(|id| {
            let map = graph.field(id, "styleMap")?.as_object()?;
            let entries = graph.collection(map);
            let pairs = entries
                .chunks(2)
                .filter_map(|pair| {
                    let [k, v] = pair else { return None };
                    let text = |v: &Value| graph.string(v.as_object()?).map(str::to_owned);
                    Some((text(k)?, text(v)?))
                })
                .collect();
            Some((id, pairs))
        })
        .collect()
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let key = args.iter().position(|a| a == "--key").map(|i| {
        args.remove(i);
        args.remove(i)
    });
    if args.is_empty() {
        eprintln!("usage: asta-styles [--key <key>] a.asta …");
        std::process::exit(2);
    }
    // key -> value -> (count, files)
    let mut seen: BTreeMap<String, BTreeMap<String, (usize, BTreeSet<String>)>> = BTreeMap::new();
    for file in args.iter().map(PathBuf::from) {
        let bytes = umlcraft_asta::read_entity_store(&file).expect("read");
        let graph = umlcraft_asta::parse_entity_store(&bytes).expect("parse");
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        for (id, pairs) in styles(&graph) {
            let class = graph.class_name(id).unwrap_or("?");
            let class = class.rsplit('.').next().unwrap_or(class);
            for (k, v) in pairs {
                if key.as_ref().is_some_and(|key| *key != k) {
                    continue;
                }
                let v = if key.is_some() { format!("{v}  [{class}]") } else { v };
                let entry = seen.entry(k).or_default().entry(v).or_default();
                entry.0 += 1;
                entry.1.insert(name.clone());
            }
        }
    }
    for (k, values) in &seen {
        let total: usize = values.values().map(|(n, _)| n).sum();
        println!("{k}  ({total})");
        for (v, (n, files)) in values.iter().take(if key.is_some() { usize::MAX } else { 12 }) {
            let files: Vec<_> = files.iter().take(4).map(String::as_str).collect();
            println!("    {n:>5}  {v}  {}", files.join(", "));
        }
        if key.is_none() && values.len() > 12 {
            println!("    … {} values", values.len());
        }
    }
}
