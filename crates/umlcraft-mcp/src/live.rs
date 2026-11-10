//! The connection between `umlcraft-cli mcp` and the running app. The app listens on a free
//! port of 127.0.0.1 ([`LiveServer`]) and writes the port and a random token into a file of
//! [`control_dir`], which only the user can read; the CLI finds it there ([`LiveClient`]). Both
//! send one JSON object per line: a request `{token, name, arguments, access}`, answered by an
//! MCP tool result. A request without the right token closes the connection.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::host::Access;
use crate::tools::Reply;

/// How long the CLI waits for the app to answer one call: long enough for a large diagram,
/// short enough to report a dialog that blocks the app.
const ANSWER: Duration = Duration::from_secs(120);

/// Where the running app leaves its port and token, readable by the user alone. On Windows a
/// folder in the user's profile, not in `%LOCALAPPDATA%`: clients installed as MSIX packages
/// (and the MCP servers they start) see a private copy of that.
pub fn control_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        var("USERPROFILE")?.join(".umlcraft")
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library/Application Support/UmlCraft")
    } else {
        var("XDG_RUNTIME_DIR")
            .map(|dir| dir.join("umlcraft"))
            .or_else(|| var("XDG_STATE_HOME").map(|dir| dir.join("umlcraft")))
            .or_else(|| var("HOME").map(|home| home.join(".local/state/umlcraft")))?
    };
    Some(dir.join("control"))
}

/// A call from the CLI, for the app to run on its main thread and [`answer`](Self::answer).
pub struct Request {
    pub name: String,
    pub arguments: Value,
    /// The folders the CLI was allowed to use, which the call keeps to.
    pub access: Access,
    reply: mpsc::Sender<Reply>,
}

impl Request {
    pub fn answer(self, reply: Reply) {
        // The CLI may have given up waiting.
        let _ = self.reply.send(reply);
    }
}

/// The app's end: listens while it lives and removes its control file when dropped.
pub struct LiveServer {
    file: PathBuf,
    port: u16,
}

impl LiveServer {
    /// Listens on a free local port and announces it in [`control_dir`]. `handler` gets every
    /// call, on the connection's thread.
    pub fn start(handler: impl Fn(Request) + Send + Sync + 'static) -> io::Result<Self> {
        let dir = control_dir().ok_or_else(|| io::Error::other("no folder for local data"))?;
        create_private_dir(&dir)?;
        remove_stale(&dir);
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let file = dir.join(format!("live-{}.json", std::process::id()));
        let announcement = json!({
            "port": port,
            "token": token,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        });
        write_private(&file, announcement.to_string().as_bytes())?;
        let handler: Arc<dyn Fn(Request) + Send + Sync> = Arc::new(handler);
        let open = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new().name("umlcraft-live".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                // Any program of the computer may connect, also before it shows a token.
                if open.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    continue;
                }
                open.fetch_add(1, Ordering::SeqCst);
                let (handler, token, count) = (handler.clone(), token.clone(), open.clone());
                let spawned = std::thread::Builder::new()
                    .name("umlcraft-live-connection".into())
                    .spawn(move || {
                        serve(stream, &token, &*handler);
                        count.fetch_sub(1, Ordering::SeqCst);
                    });
                if spawned.is_err() {
                    open.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
        Ok(Self { file, port })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for LiveServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Removes the announcements of apps that no longer listen: an app that was killed could not
/// remove its own.
fn remove_stale(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if !entry.file_name().to_string_lossy().starts_with("live-") {
            continue;
        }
        let port = std::fs::read(entry.path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|a| u16::try_from(a["port"].as_u64()?).ok());
        let listening = port.is_some_and(|port| {
            let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
            TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok()
        });
        if !listening {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn create_private_dir(dir: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `file` readable by the user alone (on Windows the local data folder is the user's).
fn write_private(file: &std::path::Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(file)?.write_all(bytes)
}

/// How many connections the app serves at once; more are closed right away.
const MAX_CONNECTIONS: usize = 16;
/// The longest request before the token was shown: the CLI's first is a short `_ping`.
const FIRST_LINE: u64 = 64 * 1024;
/// The longest request after it, room for a large structogram.
const LINE: u64 = 256 * 1024 * 1024;

/// The next line of `reader` without its newline, at most `limit` bytes long; `None` at the
/// end, or for a longer line or one that is no UTF-8.
fn read_line(reader: &mut impl BufRead, limit: u64) -> Option<String> {
    let mut bytes = Vec::new();
    let read = reader.take(limit + 1).read_until(b'\n', &mut bytes).ok()?;
    if read == 0 || (bytes.last() != Some(&b'\n') && read as u64 > limit) {
        return None;
    }
    String::from_utf8(bytes).ok().map(|line| line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Answers the requests of one connection.
fn serve(stream: TcpStream, token: &str, handler: &(dyn Fn(Request) + Send + Sync)) {
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut limit = FIRST_LINE;
    while let Some(line) = read_line(&mut reader, limit) {
        let Ok(request) = serde_json::from_str::<Value>(&line) else { return };
        if request["token"].as_str() != Some(token) {
            let _ = writeln!(writer, "{}", Reply::error("wrong token").to_json());
            return;
        }
        limit = LINE;
        let name = request["name"].as_str().unwrap_or_default().to_owned();
        let reply = if name == "_ping" {
            Reply::text(format!("UmlCraft {}", env!("CARGO_PKG_VERSION")))
        } else {
            let (tx, rx) = mpsc::channel();
            handler(Request {
                name,
                arguments: request["arguments"].clone(),
                access: Access::from_json(&request["access"]),
                reply: tx,
            });
            rx.recv_timeout(ANSWER).unwrap_or_else(|_| {
                Reply::error(
                    "The UmlCraft app did not answer; a dialog may be open there. Ask the user \
                     to close it.",
                )
            })
        };
        if writeln!(writer, "{}", reply.to_json()).is_err() {
            return;
        }
    }
}

/// The CLI's end: a connection to the running app.
pub struct LiveClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    token: String,
}

impl LiveClient {
    /// Connects to the app at `port` and checks the token.
    pub fn connect(port: u16, token: &str) -> io::Result<Self> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
        stream.set_read_timeout(Some(ANSWER + Duration::from_secs(10)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut client = Self { writer: stream, reader, token: token.to_owned() };
        let pong = client.call("_ping", &json!({}), &Access::default())?;
        if pong.is_error {
            return Err(io::Error::other("the app refused the token"));
        }
        Ok(client)
    }

    /// The app that announced itself last and answers.
    pub fn discover() -> Option<Self> {
        Self::announced(None)
    }

    /// The app listening at `port`, with the token it announced: the token never goes on a
    /// command line, where other users of the computer could read it.
    pub fn at_port(port: u16) -> Option<Self> {
        Self::announced(Some(port))
    }

    /// The app that announced itself last (at `port`, if given) and answers.
    fn announced(port: Option<u16>) -> Option<Self> {
        let dir = control_dir()?;
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("live-"))
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        files.into_iter().find_map(|(_, file)| {
            let announcement: Value = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
            let announced = u16::try_from(announcement["port"].as_u64()?).ok()?;
            if port.is_some_and(|port| port != announced) {
                return None;
            }
            Self::connect(announced, announcement["token"].as_str()?).ok()
        })
    }

    pub fn call(&mut self, name: &str, args: &Value, access: &Access) -> io::Result<Reply> {
        let request = json!({
            "token": self.token,
            "name": name,
            "arguments": args,
            "access": access.to_json(),
        });
        writeln!(self.writer, "{request}")?;
        self.writer.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the app closed the connection",
            ));
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        Ok(Reply::from_json(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_reach_the_handler_with_the_right_token_only() {
        let server = LiveServer::start(|request| {
            let text = format!("{} {}", request.name, request.arguments["x"]);
            request.answer(Reply::text(text));
        })
        .unwrap();
        let file = control_dir().unwrap().join(format!("live-{}.json", std::process::id()));
        let announcement: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        let token = announcement["token"].as_str().unwrap();

        let mut client = LiveClient::connect(server.port(), token).unwrap();
        let reply = client.call("echo", &json!({ "x": 7 }), &Access::default()).unwrap();
        assert_eq!(reply, Reply::text("echo 7"));
        assert!(LiveClient::connect(server.port(), "guess").is_err());

        drop(server);
        assert!(!file.exists());
    }

    #[test]
    fn lines_are_read_up_to_their_limit() {
        let mut reader = BufReader::new(&b"short\r\nlonger line\nlast"[..]);
        assert_eq!(read_line(&mut reader, 8).as_deref(), Some("short"));
        // Longer than allowed: the connection ends.
        assert_eq!(read_line(&mut reader, 8), None);
        let mut reader = BufReader::new(&b"exactly8\nlast"[..]);
        assert_eq!(read_line(&mut reader, 8).as_deref(), Some("exactly8"));
        assert_eq!(read_line(&mut reader, 8).as_deref(), Some("last"));
        assert_eq!(read_line(&mut reader, 8), None);
    }
}
