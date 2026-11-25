//! The MCP tools: their descriptions and input schemas ([`list`]), and running one against a
//! [`Host`] ([`call`]). Ids are the model's UUIDs as strings. Every tool that changes the
//! project is one undo step, made as the app makes its edits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};
use umlcraft_erm_sql::{Dialect, LeftOut};
use umlcraft_model::{
    ActivityNodeKind, DiagramKind, ElementData, ElementKind, ErAction, ErDisplay, ErKey,
    ErNotation, Id, InteractionOperator, InternalTransitionProps, MessageSort, Model, ModelError,
    Point, Rect, Shape, SharedStyle, StateInvariantProps, TerminationProps, Tx, Visibility,
};
use umlcraft_notation::palette::{self, EdgeTool, NodeTool, Tool};
use umlcraft_notation::scene::{self, Display, Preview, TextMeasure};
use umlcraft_notation::{
    activity, autoroute, composite, edit, er, layout, member, sequence_edit, state_edit, svg,
};

use crate::base64;
use crate::host::{Access, Host, first_diagram, new_document};
use crate::line;
use crate::structogram;
use crate::uml;

mod args;
mod arrange;
mod batch;
mod create;
mod delete;
mod export;
mod fragments;
mod lookup;
mod project;
mod read;
#[cfg(test)]
pub(crate) use read::png;
mod schema;
mod structograms;
mod update;
mod update_activity;
mod update_er;

use args::*;
use arrange::*;
use batch::*;
use create::*;
use delete::*;
use export::*;
use fragments::*;
use lookup::*;
use project::*;
use read::*;
use schema::*;
use structograms::*;
use update::*;
use update_activity::*;
use update_er::*;

pub(crate) use args::bool_arg;
pub use args::screenshot_scale;
pub use schema::list;

/// Part of a tool's reply.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Text(String),
    /// A PNG image.
    Image(Vec<u8>),
}

/// What a tool returns: MCP's `CallToolResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl Reply {
    pub fn text(text: impl Into<String>) -> Self {
        Self { content: vec![Content::Text(text.into())], is_error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { content: vec![Content::Text(text.into())], is_error: true }
    }

    pub fn json(value: Value) -> Self {
        Self::text(value.to_string())
    }

    /// A PNG image.
    pub fn image(png: Vec<u8>) -> Self {
        Self { content: vec![Content::Image(png)], is_error: false }
    }

    pub fn to_json(&self) -> Value {
        let content: Vec<Value> = self
            .content
            .iter()
            .map(|c| match c {
                Content::Text(text) => json!({ "type": "text", "text": text }),
                Content::Image(png) => {
                    json!({ "type": "image", "data": base64::encode(png), "mimeType": "image/png" })
                }
            })
            .collect();
        json!({ "content": content, "isError": self.is_error })
    }

    pub fn from_json(value: &Value) -> Self {
        let content = value["content"].as_array().map_or_else(Vec::new, |content| {
            content
                .iter()
                .filter_map(|c| match c["type"].as_str()? {
                    "text" => Some(Content::Text(c["text"].as_str()?.to_owned())),
                    "image" => Some(Content::Image(base64::decode(c["data"].as_str()?)?)),
                    _ => None,
                })
                .collect()
        });
        Self { content, is_error: value["isError"].as_bool().unwrap_or(false) }
    }
}

/// How the server explains itself to the client (MCP `instructions`).
pub const INSTRUCTIONS: &str = "UmlCraft is a UML modeling tool. These tools read and edit \
the open UML project: when the UmlCraft app is running, the project open there (the user sees \
every change and can undo it with Ctrl+Z); otherwise a project of the server's own, which \
`open_file` and `save_file` load and store. A project holds a model tree (packages, classes, \
…) and diagrams showing parts of it. Start with `get_document`. To draw a diagram: \
`create_diagram`, then `create_element` for each box (no position needed), `add_member` for \
attributes and operations (`- name : String`, `+ total() : int`), `connect` for relationships, \
then `arrange_diagram` (any kind but sequence diagrams) and `render_diagram` to look at \
the result (`screenshot` shows the app's whole window while it runs). `get_diagram` lists what a diagram shows and which element and relationship tools \
its kind offers. In a sequence diagram, `connect` draws a message between lifelines below the others \
unless `y` says where, `wrap_in_fragment` puts messages into an `alt`, `loop`, … and `add_operand` adds a \
case. Structograms (Nassi-Shneiderman diagrams) are JSON trees: `create_structogram`, \
`get_structogram`, `update_structogram`. `batch` runs several calls as one undo step; a string `$0.element` in a later call's \
arguments takes the field `element` from the first call's reply (`create_diagram` replies \
with `diagram`, `create_element` and `connect` with `element`). Ids are UUID strings returned by the \
tools; a `diagram` may also be given by its name (a structogram by its title).";

type Outcome = Result<Reply, String>;

/// Runs tool `name` with `args` against `host`; files only in the folders `access` allows.
pub fn call(host: &mut dyn Host, access: &Access, name: &str, args: &Value) -> Reply {
    if let Some(why) = unknown_arguments(name, args) {
        return Reply::error(why);
    }
    let outcome: Outcome = match name {
        "get_document" => get_document(host, access),
        "list_elements" => list_elements(host, args),
        "get_element" => get_element(host, args),
        "get_diagram" => get_diagram(host, args),
        "render_diagram" => render_diagram(host, args),
        "set_display" => set_display(host, args),
        // The app answers this one itself, with its window; it never comes here from the app
        // but inside `batch`.
        "screenshot" | "set_window" | "set_view" => Err(if host.live() {
            format!("`{name}` cannot run inside `batch`; call it on its own.")
        } else {
            format!(
                "`{name}` needs the UmlCraft window: this session works on a project of the \
                 server's own (the app is not running, or this session started without it). \
                 `render_diagram` draws a diagram without the app."
            )
        }),
        "select" => select(host, args),
        "create_diagram" => create_diagram(host, args),
        "create_element" => create_element(host, args),
        "create_model_element" => create_model_element(host, args),
        "show_element" => show_element(host, args),
        "connect" => connect(host, args),
        "add_member" => add_member(host, args),
        "wrap_in_fragment" => wrap_in_fragment(host, args),
        "add_operand" => add_operand(host, args),
        "get_structogram" => get_structogram(host, args),
        "create_structogram" => create_structogram(host, args),
        "update_structogram" => update_structogram(host, args),
        "edit_structogram" => edit_structogram(host, args),
        "update_element" => update_element(host, args),
        "move_node" => move_node(host, args),
        "arrange_diagram" => arrange_diagram(host, args),
        "route_lines" => route_lines(host, args),
        "delete_element" => delete_element(host, args),
        "remove_from_diagram" => remove_from_diagram(host, args),
        "batch" => batch(host, access, args),
        "undo" => undo(host, true),
        "redo" => undo(host, false),
        "open_diagram" => open_diagram(host, args),
        "new_document" => new_project(host, args),
        "open_file" => open_file(host, access, args),
        "save_file" => save_file(host, access, args),
        "export_image" => export_image(host, access, args),
        "export_sql" => export_sql(host, access, args),
        _ => Err(format!("unknown tool `{name}`")),
    };
    outcome.unwrap_or_else(Reply::error)
}

/// [`call`], with a panic in the tool answered as an error instead of ending the process, so
/// that a malformed call does not cost the app's user the open project. `Document::edit` has
/// taken back the edit that panicked; the steps the call made before it (a batch's earlier
/// calls) are taken back here.
pub fn call_guarded(host: &mut dyn Host, access: &Access, name: &str, args: &Value) -> Reply {
    let mark = host.document().mark();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        call(&mut *host, access, name, args)
    }));
    match outcome {
        Ok(reply) => reply,
        Err(panic) => {
            host.parts().0.roll_back(mark);
            let why = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("a panic");
            Reply::error(format!(
                "internal error in `{name}`: {why}. Nothing was changed. This is a bug in \
                 UmlCraft; try the change another way."
            ))
        }
    }
}

#[cfg(test)]
mod tests;
