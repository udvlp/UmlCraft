//! MCP over stdio: JSON-RPC 2.0 messages, one per line, on stdin and stdout. Only what a tool
//! server needs: `initialize`, `ping`, `tools/list` and `tools/call`.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

use crate::tools::{self, Reply};

/// The protocol versions this server speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// What runs the tools: the app or a headless project ([`crate::backend::Auto`]).
pub trait Backend {
    fn call(&mut self, name: &str, args: &Value) -> Reply;
}

/// Answers requests from `input` on `output` until `input` ends. A line that is not UTF-8 or
/// not JSON gets a parse error, and the next line is read.
pub fn serve(
    backend: &mut dyn Backend,
    mut input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        if input.read_until(b'\n', &mut bytes)? == 0 {
            return Ok(());
        }
        let Ok(line) = std::str::from_utf8(&bytes) else {
            let response = error(Value::Null, -32700, "parse error: the line is not UTF-8");
            writeln!(output, "{response}")?;
            output.flush()?;
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        // As deep as a project's structograms, beyond serde_json's limit of 128.
        let response = match umlcraft_format::parse_value(line) {
            Ok(Value::Array(batch)) if batch.is_empty() => {
                Some(error(Value::Null, -32600, "invalid request: an empty batch"))
            }
            Ok(Value::Array(batch)) => {
                let answers: Vec<Value> =
                    batch.iter().filter_map(|message| answer(backend, message)).collect();
                (!answers.is_empty()).then(|| Value::Array(answers))
            }
            Ok(message) => answer(backend, &message),
            Err(err) => Some(error(Value::Null, -32700, &format!("parse error: {err}"))),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
}

/// The response to one message; none for notifications and responses. A request without a
/// method name gets an error, so the client does not wait for it.
fn answer(backend: &mut dyn Backend, message: &Value) -> Option<Value> {
    let Some(fields) = message.as_object() else {
        return Some(error(Value::Null, -32600, "invalid request: not an object"));
    };
    let id = fields.get("id").cloned();
    let method = match fields.get("method") {
        Some(Value::String(method)) => method.as_str(),
        // A response to a request of ours (none are sent) needs no answer.
        None if fields.contains_key("result") || fields.contains_key("error") => return None,
        _ => {
            let why = "invalid request: `method` is missing or not a string";
            return Some(error(id.unwrap_or(Value::Null), -32600, why));
        }
    };
    // A notification gets no answer.
    let id = id?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or(VERSIONS[0]);
            let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "umlcraft",
                    "title": "UmlCraft",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": tools::INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tools::list() }),
        "tools/call" => {
            let Some(name) = params["name"].as_str() else {
                return Some(error(id, -32602, "tools/call needs a tool name"));
            };
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            backend.call(name, &args).to_json()
        }
        _ => return Some(error(id, -32601, &format!("unknown method `{method}`"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Access, Headless};

    struct Local(Headless);

    impl Backend for Local {
        fn call(&mut self, name: &str, args: &Value) -> Reply {
            tools::call(&mut self.0, &Access::default(), name, args)
        }
    }

    fn exchange(lines: &[Value]) -> Vec<Value> {
        let input: String = lines.iter().map(|l| format!("{l}\n")).collect();
        exchange_bytes(input.as_bytes())
    }

    fn exchange_bytes(input: &[u8]) -> Vec<Value> {
        let mut output = Vec::new();
        serve(&mut Local(Headless::new()), input, &mut output).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn malformed_messages_get_errors_and_the_server_goes_on() {
        let mut input = Vec::new();
        input.extend_from_slice(b"\xff\xfe not utf-8\n");
        input.extend_from_slice(b"[]\n");
        input.extend_from_slice(b"{ not json\n");
        for message in [
            json!({ "jsonrpc": "2.0", "id": 1 }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": 7 }),
            json!(5),
            // A response and a notification: no answer.
            json!({ "jsonrpc": "2.0", "id": 3, "result": {} }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "ping" }),
        ] {
            input.extend_from_slice(format!("{message}\n").as_bytes());
        }
        let answers = exchange_bytes(&input);
        let codes: Vec<Value> = answers.iter().map(|a| a["error"]["code"].clone()).collect();
        assert_eq!(
            Value::Array(codes),
            json!([-32700, -32600, -32700, -32600, -32600, -32600, null])
        );
        assert_eq!(answers[3]["id"], 1);
        assert_eq!(answers[4]["id"], 2);
        assert_eq!(answers[6], json!({ "jsonrpc": "2.0", "id": 4, "result": {} }));
    }

    /// A structogram as deep as a project may hold nests far deeper than serde_json's limit
    /// of 128 in a request; it goes in and comes out whole.
    #[test]
    fn a_structogram_as_deep_as_a_project_holds_goes_through() {
        let mut body = json!([{ "kind": "instruction", "text": "x := 1" }]);
        for _ in 1..umlcraft_nsd::read::MAX_DEPTH {
            body = json!([{ "kind": "while", "text": "x > 0", "body": body }]);
        }
        let tree = json!({ "title": "deep", "body": body });
        let calls = json!([
            { "tool": "create_structogram", "arguments": { "structogram": tree } },
            { "tool": "get_structogram", "arguments": { "id": "$0.element" } },
        ]);
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                              "params": { "name": "batch", "arguments": { "calls": calls } } });
        let mut output = Vec::new();
        serve(&mut Local(Headless::new()), format!("{request}\n").as_bytes(), &mut output).unwrap();
        // The answer is as deep: read back without serde_json's limit too.
        let answer = umlcraft_format::parse_value(std::str::from_utf8(&output).unwrap()).unwrap();
        assert_eq!(answer["result"]["isError"], false, "{answer}");
        let text = answer["result"]["content"][0]["text"].as_str().unwrap();
        let done = umlcraft_format::parse_value(text).unwrap();
        assert_eq!(done["results"][1]["structogram"]["body"], tree["body"]);
    }

    #[test]
    fn a_session_initializes_lists_and_calls() {
        let answers = exchange(&[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": { "protocolVersion": "2025-03-26", "capabilities": {},
                                "clientInfo": { "name": "test", "version": "1" } } }),
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                    "params": { "name": "get_document", "arguments": {} } }),
            json!({ "jsonrpc": "2.0", "id": 4, "method": "nope" }),
        ]);
        // The notification gets no answer.
        assert_eq!(answers.len(), 4);
        assert_eq!(answers[0]["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(answers[0]["result"]["serverInfo"]["name"], "umlcraft");
        assert!(answers[1]["result"]["tools"].as_array().unwrap().len() > 20);
        assert_eq!(answers[2]["result"]["isError"], false);
        assert_eq!(answers[3]["error"]["code"], -32601);
    }
}
