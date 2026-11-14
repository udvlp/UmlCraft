//! Which project a call goes to. While nothing has been done headless yet, every call first
//! looks for the running app and goes there; once the headless project has been changed or a
//! file opened, the session stays with it, so work is not split between two projects. A
//! session that worked with the app and loses it says so once, then looks again.

use serde_json::Value;

use crate::host::{Access, Headless};
use crate::live::LiveClient;
use crate::protocol::Backend;
use crate::tools::{self, Reply};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Undecided,
    Live,
    Headless,
}

/// The app when it runs, else a headless project.
pub struct Auto {
    access: Access,
    headless: Headless,
    live: Option<LiveClient>,
    mode: Mode,
    /// The app's port, given on the command line instead of looked up; its token comes from
    /// the app's announcement.
    connect: Option<u16>,
}

impl Auto {
    /// Without the app, the project of the server's own is drawn as the app's settings say.
    pub fn new(access: Access) -> Self {
        let headless = Headless::new().drawn_as(crate::host::app_display());
        Self { access, headless, live: None, mode: Mode::Undecided, connect: None }
    }

    /// Never the app: works on `headless` alone (`--headless`, `--in`).
    pub fn headless(access: Access, headless: Headless) -> Self {
        Self { mode: Mode::Headless, headless, ..Self::new(access) }
    }

    /// Always the app at `port`, never headless (`--connect`).
    pub fn connect(access: Access, port: u16) -> Self {
        Self { connect: Some(port), ..Self::new(access) }
    }

    fn find_app(&mut self) -> Option<&mut LiveClient> {
        if self.live.is_none() {
            self.live = match self.connect {
                Some(port) => LiveClient::at_port(port),
                None => LiveClient::discover(),
            };
        }
        self.live.as_mut()
    }
}

impl Backend for Auto {
    fn call(&mut self, name: &str, args: &Value) -> Reply {
        if self.mode != Mode::Headless {
            let access = self.access.clone();
            if let Some(app) = self.find_app() {
                match app.call(name, args, &access) {
                    Ok(reply) => {
                        self.mode = Mode::Live;
                        return reply;
                    }
                    Err(err) => {
                        self.live = None;
                        if self.mode == Mode::Live || self.connect.is_some() {
                            self.mode = Mode::Undecided;
                            return Reply::error(format!(
                                "The connection to the UmlCraft app was lost ({err}); it may \
                                 have been closed. Nothing was done. The next call goes to the \
                                 app again if it runs, else to a project of the server's own."
                            ));
                        }
                    }
                }
            } else if self.connect.is_some() {
                return Reply::error("The UmlCraft app given with --connect does not answer.");
            } else if self.mode == Mode::Live {
                self.mode = Mode::Undecided;
                return Reply::error(
                    "The UmlCraft app was closed. Nothing was done. The next call works on a \
                     project of the server's own, unless the app runs again.",
                );
            }
        }
        let reply = tools::call(&mut self.headless, &self.access, name, args);
        if self.headless.touched() {
            self.mode = Mode::Headless;
        }
        reply
    }
}
