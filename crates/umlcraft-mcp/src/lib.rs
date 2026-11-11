//! UmlCraft's MCP server: tools with which an AI client reads and edits UML models.
//!
//! - [`tools`] lists the tools and runs them against a [`Host`]: the project open in the
//!   running app, or a headless one ([`Headless`]). Every change is one undo step made the way
//!   the app makes it (`umlcraft_notation::edit::apply`).
//! - [`protocol`] speaks MCP (JSON-RPC 2.0 over stdin and stdout) for `umlcraft-cli mcp`.
//! - [`live`] connects that process to the running app: the app listens on localhost and leaves
//!   its port and a token in a folder of the user's ([`live::control_dir`]).
//! - [`backend`] decides per call whether the app or the headless project serves it.

pub mod backend;
mod base64;
pub mod host;
pub mod live;
pub mod protocol;
mod structogram;
pub mod tools;
pub mod uml;

pub use host::{Access, Headless, Host};
pub use tools::{Content, Reply};
