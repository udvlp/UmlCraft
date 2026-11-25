//! `batch`, `undo` and `redo`.

use super::*;

/// Tools a batch may not run: they change the history, replace the project, or change what a
/// rollback cannot take back (the project's file, the display settings).
pub(super) const NOT_IN_BATCH: [&str; 7] =
    ["undo", "redo", "batch", "open_file", "new_document", "save_file", "set_display"];

pub(super) fn batch(host: &mut dyn Host, access: &Access, args: &Value) -> Outcome {
    let calls = args.get("calls").and_then(Value::as_array).ok_or("`calls` must be an array")?;
    let depth = host.document().undo_depth();
    // A failed batch leaves the history as it was, what could be redone included.
    let mark = host.document().mark();
    let mut results: Vec<Value> = Vec::with_capacity(calls.len());
    // Files the exports wrote, which stay when a later call fails.
    let mut written: Vec<String> = Vec::new();
    let failed = |host: &mut dyn Host, written: &[String], i: usize, why: String| {
        host.parts().0.roll_back(mark.clone());
        if written.is_empty() {
            Err(format!("call {i} failed, nothing was changed: {why}"))
        } else {
            Err(format!(
                "call {i} failed, the model was not changed (the files {} stay written): {why}",
                written.join(", ")
            ))
        }
    };
    for (i, c) in calls.iter().enumerate() {
        let Some(name) = c.get("tool").and_then(Value::as_str) else {
            return failed(host, &written, i, "`tool` is missing".into());
        };
        if NOT_IN_BATCH.contains(&name) {
            return failed(host, &written, i, format!("`{name}` cannot run in a batch"));
        }
        let arguments = match resolve(c.get("arguments").unwrap_or(&json!({})), &results) {
            Ok(arguments) => arguments,
            Err(why) => return failed(host, &written, i, why),
        };
        let reply = call(host, access, name, &arguments);
        let text: Vec<&str> = reply
            .content
            .iter()
            .filter_map(|c| match c {
                Content::Text(text) => Some(text.as_str()),
                Content::Image(_) => None,
            })
            .collect();
        let text = text.join("\n");
        if reply.is_error {
            return failed(host, &written, i, format!("{name}: {text}"));
        }
        // A reply may hold a structogram as deep as a project holds.
        let result = umlcraft_format::parse_value(&text).unwrap_or(Value::String(text));
        if let Some(path) = result.get("written").and_then(Value::as_str) {
            written.push(path.to_owned());
        }
        results.push(result);
    }
    let label = host.label(opt_str(args, "label").unwrap_or("batch"));
    host.parts().0.squash_since(depth, label);
    Ok(Reply::json(json!({ "results": results })))
}

/// Whether `s` is a reference `$<index>[.<field>…]`: other strings that start with `$` (a
/// member `$price : int`, a structogram's `$x := 0`) are text.
pub(super) fn is_reference(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('$') else { return false };
    let mut parts = rest.split('.');
    let index = parts.next().unwrap_or_default();
    !index.is_empty()
        && index.bytes().all(|b| b.is_ascii_digit())
        && parts.all(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
}

/// `arguments` with each string `$<index>.<field>[.<field>…]` replaced by that field of the
/// reply of an earlier call; `$$` at the start of a string stands for a plain `$`.
pub(super) fn resolve(arguments: &Value, results: &[Value]) -> Result<Value, String> {
    Ok(match arguments {
        Value::String(s) if s.starts_with("$$") => Value::String(s[1..].to_owned()),
        Value::String(s) if is_reference(s) => {
            let mut path = s[1..].split('.');
            let index = path.next().and_then(|i| i.parse::<usize>().ok());
            let Some(reply) = index.and_then(|i| results.get(i)) else {
                return Err(format!("`{s}` names no earlier call"));
            };
            let mut value = reply;
            for field in path {
                value = match value {
                    Value::Array(items) => field.parse::<usize>().ok().and_then(|i| items.get(i)),
                    _ => value.get(field),
                }
                .ok_or_else(|| format!("the reply of call {} has no `{s}`", index.unwrap_or(0)))?;
            }
            value.clone()
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|v| resolve(v, results)).collect::<Result<_, _>>()?)
        }
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, results)?)))
                .collect::<Result<_, String>>()?,
        ),
        other => other.clone(),
    })
}

pub(super) fn undo(host: &mut dyn Host, undo: bool) -> Outcome {
    let (doc, _) = host.parts();
    let done = if undo { doc.undo() } else { doc.redo() };
    let word = if undo { "undo" } else { "redo" };
    if done { Ok(Reply::text(format!("{word} done"))) } else { Err(format!("nothing to {word}")) }
}
