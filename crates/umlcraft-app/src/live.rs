//! The running app's side of the MCP server (`umlcraft-cli mcp`): calls arrive from
//! [`umlcraft_mcp::live::LiveServer`] on its threads, cross to the main thread through a
//! channel and run there against the open project, like the user's own edits: one undo step
//! each, shown at once.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, Window};
use rust_i18n::t;
use serde_json::{Value, json};
use umlcraft_mcp::live::{LiveServer, Request};
use umlcraft_mcp::{Content, Host, Reply};
use umlcraft_model::{Document, Id};
use umlcraft_notation::scene::TextMeasure;

use crate::workspace::Workspace;

/// Starts listening for the MCP server and returns the server (which stops announcing itself
/// when dropped) and where its calls arrive. `None` where there is no folder for local data.
pub fn start() -> Option<(LiveServer, async_channel::Receiver<Request>)> {
    let (tx, rx) = async_channel::unbounded();
    let server = LiveServer::start(move |request| {
        // The app is quitting when nobody receives.
        let _ = tx.send_blocking(request);
    })
    .ok()?;
    Some((server, rx))
}

/// Answers the `screenshot` tool with the app's window. Captures on a background thread a
/// moment later: the window's own thread has to draw what a call just changed first, and
/// Windows asks it to draw the window again for the capture.
pub fn screenshot<V: 'static>(request: Request, window: &Window, cx: &mut Context<V>) {
    let target = match umlcraft_screenshot::Target::of(window) {
        Ok(target) => target,
        Err(err) => return request.answer(Reply::error(err)),
    };
    let scale = umlcraft_mcp::tools::screenshot_scale(&request.arguments);
    let executor = cx.background_executor().clone();
    executor
        .spawn({
            let executor = executor.clone();
            async move {
                executor.timer(FRAME).await;
                let png = umlcraft_screenshot::capture(target)
                    .and_then(|image| crate::png::from_rgba(image, scale));
                request.answer(match png {
                    Ok(png) => Reply { content: vec![Content::Image(png)], is_error: false },
                    Err(err) => Reply::error(err),
                });
            }
        })
        .detach();
}

/// Answers the `set_window` tool: sizes and places the window from a background thread, since
/// Windows asks the window's own thread to take the new size, and waits until it has.
pub fn set_window<V: 'static>(request: Request, window: &Window, cx: &mut Context<V>) {
    let target = match umlcraft_screenshot::Target::of(window) {
        Ok(target) => target,
        Err(err) => return request.answer(Reply::error(err)),
    };
    let args = request.arguments.clone();
    let size = |key: &str| args.get(key).and_then(Value::as_f64).map(|v| v.round().max(1.0) as u32);
    let (width, height) = (size("width"), size("height"));
    let placement = match args.get("placement").and_then(Value::as_str) {
        None => None,
        Some(name) => match umlcraft_screenshot::Placement::parse(name) {
            Some(placement) => Some(placement),
            None => {
                return request.answer(Reply::error(format!(
                    "`placement` is screen, offscreen or minimized, not `{name}`"
                )));
            }
        },
    };
    let executor = cx.background_executor().clone();
    executor
        .spawn({
            let executor = executor.clone();
            async move {
                let state = umlcraft_screenshot::arrange(target, width, height, placement);
                executor.timer(FRAME).await;
                request.answer(match state {
                    Ok(state) => Reply::json(json!({
                        "width": state.width,
                        "height": state.height,
                        "placement": state.placement.name(),
                        "scale": state.scale,
                    })),
                    Err(err) => Reply::error(err),
                });
            }
        })
        .detach();
}

/// Long enough for the window to show the last call's change.
const FRAME: Duration = Duration::from_millis(100);

/// What a call asks of the main view beyond the workspace.
#[derive(Default)]
pub struct Effects {
    /// A project to open instead of the current one.
    pub replace: Option<(Document, Option<PathBuf>)>,
    /// The file the project was saved to, for the recent files.
    pub saved: Option<PathBuf>,
    /// What to select in a structogram; its editor holds the selection.
    pub structogram_selection: Option<(Id, umlcraft_nsd::Selection)>,
}

/// The open project as the tools see it.
pub struct AppHost<'a> {
    pub ws: &'a mut Workspace,
    pub effects: &'a mut Effects,
}

impl Host for AppHost<'_> {
    fn document(&self) -> &Document {
        &self.ws.doc
    }

    fn parts(&mut self) -> (&mut Document, &dyn TextMeasure) {
        (&mut self.ws.doc, &*self.ws.measure)
    }

    fn label(&self, tool: &str) -> String {
        t!("edit.mcp", tool = tool).to_string()
    }

    fn path(&self) -> Option<&Path> {
        self.ws.path.as_deref()
    }

    fn live(&self) -> bool {
        true
    }

    fn active_diagram(&self) -> Option<Id> {
        self.ws.active_diagram
    }

    fn selection(&self) -> Vec<Id> {
        self.ws.selection.clone()
    }

    fn show_diagram(&mut self, diagram: Id) {
        self.ws.open_diagram(diagram);
    }

    fn replace(&mut self, doc: Document, path: Option<PathBuf>) -> Result<(), String> {
        if self.ws.doc.is_dirty() {
            return Err("The project open in UmlCraft has unsaved changes. Ask the user to save \
                        or discard them first."
                .into());
        }
        self.effects.replace = Some((doc, path));
        Ok(())
    }

    fn saved(&mut self, path: PathBuf) {
        self.ws.path = Some(path.clone());
        self.effects.saved = Some(path);
    }

    fn select(&mut self, presentations: Vec<Id>) {
        self.ws.select(presentations);
    }

    fn select_in_structogram(&mut self, structogram: Id, selection: umlcraft_nsd::Selection) {
        self.effects.structogram_selection = Some((structogram, selection));
    }

    fn sample(&self) -> Result<Document, String> {
        Ok(crate::demo::velo(&*self.ws.measure))
    }
}
