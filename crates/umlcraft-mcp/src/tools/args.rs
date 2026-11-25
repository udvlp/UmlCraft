//! Reading the arguments of a call.

use super::*;

pub(super) fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(super) fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    opt_str(args, key).ok_or_else(|| format!("`{key}` is missing"))
}

/// The boolean `key` of `args`, if given; anything else than true or false (null too) is a
/// mistake.
pub(crate) fn bool_arg(args: &Value, key: &str) -> Result<Option<bool>, String> {
    args.get(key)
        .map(|v| v.as_bool().ok_or_else(|| format!("`{key}` is true or false")))
        .transpose()
}

pub(super) fn opt_num(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

/// The size of `screenshot`'s image relative to the window, from its arguments.
pub fn screenshot_scale(args: &Value) -> f32 {
    opt_num(args, "scale").unwrap_or(1.0).clamp(0.25, 1.0)
}

pub(super) fn parse_id(text: &str) -> Result<Id, String> {
    Id::from_str(text.trim()).map_err(|_| format!("`{text}` is not an id"))
}

pub(super) fn id_arg(args: &Value, key: &str) -> Result<Id, String> {
    parse_id(str_arg(args, key)?)
}

pub(super) fn opt_id(args: &Value, key: &str) -> Result<Option<Id>, String> {
    opt_str(args, key).map(parse_id).transpose()
}

pub(super) fn ids_arg(args: &Value, key: &str) -> Result<Option<Vec<Id>>, String> {
    let Some(items) = args.get(key).and_then(Value::as_array) else { return Ok(None) };
    items
        .iter()
        .map(|item| item.as_str().ok_or_else(|| format!("`{key}` holds ids")).and_then(parse_id))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

pub(super) fn model_error(err: ModelError) -> String {
    err.to_string()
}

/// The value named by the string `key` of `args`, out of `names`.
pub(super) fn pick<T: Copy>(
    args: &Value,
    key: &str,
    names: &[(&str, T)],
) -> Result<Option<T>, String> {
    let Some(name) = opt_str(args, key) else { return Ok(None) };
    match names.iter().find(|(n, _)| *n == name) {
        Some(&(_, value)) => Ok(Some(value)),
        None => {
            let names: Vec<&str> = names.iter().map(|(n, _)| *n).collect();
            Err(format!("`{key}` is one of {}, not `{name}`", names.join(", ")))
        }
    }
}
