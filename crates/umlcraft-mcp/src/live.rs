//! The connection between `umlcraft-cli mcp` and the running app. The app listens on a free
//! port of 127.0.0.1 ([`LiveServer`]) and writes the port, a random token and a random proof
//! into a file of [`control_dir`]; the CLI finds it there ([`LiveClient`]). Both send one JSON
//! object per line: a request `{token, name, arguments, access}`, answered by an MCP tool
//! result. A request without the right token closes the connection; the app's answer to the
//! first request, `_ping`, holds the proof, so the CLI knows it talks to the app that wrote the
//! file and not to another program that took the port of an app that was killed.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::host::Access;
use crate::tools::{Content, Reply};

/// How long the CLI waits for the app to take up one call: long enough for a large diagram,
/// short enough to report a dialog that blocks the app.
const ANSWER: Duration = Duration::from_secs(120);

/// Where the running app leaves its port and token. On Windows a folder in the user's profile
/// (`%USERPROFILE%\.umlcraft`), not in `%LOCALAPPDATA%`: clients installed as MSIX packages
/// (and the MCP servers they start) see a private copy of that. There no access rights are set;
/// the folder has those the profile passes on, which let only the user (and administrators)
/// read it. Elsewhere the folder and file are made readable by the user alone.
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

/// Where a request is: waiting for the app, taken up by it, or given up by the CLI.
const WAITING: u8 = 0;
const TAKEN: u8 = 1;
const ABANDONED: u8 = 2;

/// A call from the CLI, for the app to run on its main thread and [`answer`](Self::answer).
pub struct Request {
    pub name: String,
    pub arguments: Value,
    /// The folders the CLI was allowed to use, which the call keeps to.
    pub access: Access,
    reply: mpsc::Sender<Reply>,
    state: Arc<AtomicU8>,
}

impl Request {
    /// Takes the call up before running it: `false` when the CLI stopped waiting for it and
    /// told the client that it was not carried out, so it must not run any more.
    pub fn take(&self) -> bool {
        self.state.compare_exchange(WAITING, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_ok()
    }

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
        Self::start_waiting(handler, ANSWER)
    }

    /// [`LiveServer::start`], waiting `answer` for the app to take up a call.
    fn start_waiting(
        handler: impl Fn(Request) + Send + Sync + 'static,
        answer: Duration,
    ) -> io::Result<Self> {
        let dir = control_dir().ok_or_else(|| io::Error::other("no folder for local data"))?;
        create_private_dir(&dir)?;
        remove_stale(&dir);
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        let port = listener.local_addr()?.port();
        let secret =
            || format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let (token, proof) = (secret(), secret());
        let file = dir.join(format!("live-{}.json", std::process::id()));
        let announcement = json!({
            "port": port,
            "token": token,
            "proof": proof,
            "pid": std::process::id(),
            "version": env!("CARGO_PKG_VERSION"),
        });
        write_private(&file, announcement.to_string().as_bytes())?;
        let handler: Arc<dyn Fn(Request) + Send + Sync> = Arc::new(handler);
        let waiting = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new().name("umlcraft-live".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                // Any program of the computer may connect; only those that have not shown the
                // token yet count, and only for `FIRST_WAIT` in all, so that none can keep the
                // CLI out by holding connections.
                if waiting.load(Ordering::SeqCst) >= MAX_WAITING {
                    continue;
                }
                waiting.fetch_add(1, Ordering::SeqCst);
                let unknown = Waiting(Some(waiting.clone()));
                let secrets = Secrets { token: token.clone(), proof: proof.clone() };
                let handler = handler.clone();
                // A thread that cannot start drops `unknown` along with its closure.
                let _ = std::thread::Builder::new()
                    .name("umlcraft-live-connection".into())
                    .spawn(move || serve(stream, &secrets, &*handler, unknown, answer));
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

/// Writes `file` readable by the user alone on Unix; on Windows it has the access rights of
/// its folder ([`control_dir`]).
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

/// How many connections may wait to show the token at once; more are closed right away.
const MAX_WAITING: usize = 16;
/// How long a connection may take to show the token, however slowly its bytes come: the CLI
/// sends it right after connecting.
const FIRST_WAIT: Duration = Duration::from_secs(5);

/// A connection counted among those that have not shown the token yet, until it shows it
/// ([`Waiting::admit`]) or closes.
struct Waiting(Option<Arc<AtomicUsize>>);

impl Waiting {
    fn admit(&mut self) {
        if let Some(count) = self.0.take() {
            count.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        self.admit();
    }
}

/// What the app announced: the token the CLI shows, the proof the app answers `_ping` with.
struct Secrets {
    token: String,
    proof: String,
}

/// A connection whose reads end at `until` in all, not per read: a client that sends a byte
/// now and then cannot stretch its first line.
struct Timed {
    stream: TcpStream,
    until: Option<Instant>,
}

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(until) = self.until {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(left))?;
        }
        self.stream.read(buf)
    }
}

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
fn serve(
    stream: TcpStream,
    secrets: &Secrets,
    handler: &(dyn Fn(Request) + Send + Sync),
    mut unknown: Waiting,
    answer: Duration,
) {
    let Ok(mut writer) = stream.try_clone() else { return };
    let mut reader = BufReader::new(Timed { stream, until: Some(Instant::now() + FIRST_WAIT) });
    let mut limit = FIRST_LINE;
    while let Some(line) = read_line(&mut reader, limit) {
        let Ok(request) = umlcraft_format::parse_value(&line) else { return };
        if request["token"].as_str() != Some(secrets.token.as_str()) {
            let _ = writeln!(writer, "{}", Reply::error("wrong token").to_json());
            return;
        }
        if limit == FIRST_LINE {
            // Known now: it may stay as long as the CLI runs, idle between calls.
            let timed = reader.get_mut();
            timed.until = None;
            if timed.stream.set_read_timeout(None).is_err() {
                return;
            }
            unknown.admit();
            limit = LINE;
        }
        let name = request["name"].as_str().unwrap_or_default().to_owned();
        let reply = if name == "_ping" {
            Reply::json(json!({
                "app": "UmlCraft",
                "version": env!("CARGO_PKG_VERSION"),
                "proof": secrets.proof,
            }))
        } else {
            let (tx, rx) = mpsc::channel();
            let state = Arc::new(AtomicU8::new(WAITING));
            handler(Request {
                name,
                arguments: request["arguments"].clone(),
                access: Access::from_json(&request["access"]),
                reply: tx,
                state: state.clone(),
            });
            wait_for(&rx, &state, answer)
        };
        if writeln!(writer, "{}", reply.to_json()).is_err() {
            return;
        }
    }
}

/// The app's reply to a call. A call the app has not taken up within `answer` is given up, and
/// the app will not run it later: the client may safely try again. One that it took up is
/// running and is waited for as long again.
fn wait_for(rx: &mpsc::Receiver<Reply>, state: &AtomicU8, answer: Duration) -> Reply {
    let gone = || Reply::error("The UmlCraft app dropped the call without an answer.");
    match rx.recv_timeout(answer) {
        Ok(reply) => return reply,
        Err(mpsc::RecvTimeoutError::Disconnected) => return gone(),
        Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
    if state.compare_exchange(WAITING, ABANDONED, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        return Reply::error(
            "The UmlCraft app did not take up the call; a dialog may be open there. The call \
             was not carried out and will not be. Ask the user to close the dialog, then try \
             again.",
        );
    }
    match rx.recv_timeout(answer) {
        Ok(reply) => reply,
        Err(mpsc::RecvTimeoutError::Disconnected) => gone(),
        Err(mpsc::RecvTimeoutError::Timeout) => Reply::error(
            "The UmlCraft app is still carrying out the call; its result will show there. \
             Check with `get_document` or `get_diagram` before trying again.",
        ),
    }
}

/// The CLI's end: a connection to the running app.
pub struct LiveClient {
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    token: String,
    /// Said along with the first reply: the app is another version than this server.
    note: Option<String>,
}

impl LiveClient {
    /// Connects to the app at `port` and checks the token.
    pub fn connect(port: u16, token: &str) -> io::Result<Self> {
        Self::open(port, token, None)
    }

    /// Connects to the app at `port`, checks the token and, if given, that the app knows
    /// `proof`.
    fn open(port: u16, token: &str, proof: Option<&str>) -> io::Result<Self> {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let stream = TcpStream::connect_timeout(&address, Duration::from_millis(500))?;
        // The app waits up to `ANSWER` for a call to be taken up and as long for it to run.
        stream.set_read_timeout(Some(2 * ANSWER + Duration::from_secs(10)))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut client = Self { writer: stream, reader, token: token.to_owned(), note: None };
        let pong = client.call("_ping", &json!({}), &Access::default())?;
        if pong.is_error {
            return Err(io::Error::other("the app refused the token"));
        }
        // Older apps answer with plain text.
        let answer: Value = match pong.content.first() {
            Some(Content::Text(text)) => serde_json::from_str(text).unwrap_or_default(),
            _ => Value::Null,
        };
        if proof.is_some_and(|proof| answer["proof"].as_str() != Some(proof)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "another program listens on the app's port",
            ));
        }
        let ours = env!("CARGO_PKG_VERSION");
        client.note = match answer["version"].as_str() {
            Some(version) if version != ours => Some(format!(
                "Note: the UmlCraft app is version {version}, this MCP server {ours}. Restart \
                 the AI client so that both are the same version; tools may differ until then."
            )),
            _ => None,
        };
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

    /// The app that announced itself last (at `port`, if given) and answers. An announcement
    /// whose port another program holds is left over from an app that was killed: it is
    /// removed.
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
            let announcement: Value = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            let announced = u16::try_from(announcement["port"].as_u64()?).ok()?;
            if port.is_some_and(|port| port != announced) {
                return None;
            }
            let token = announcement["token"].as_str()?;
            let proof = announcement["proof"].as_str();
            match Self::open(announced, token, proof) {
                Ok(client) => Some(client),
                Err(err) => {
                    if err.kind() == io::ErrorKind::InvalidData && proof.is_some() {
                        let _ = std::fs::remove_file(&file);
                    }
                    None
                }
            }
        })
    }

    /// What to tell the client once about this connection, if anything.
    pub fn take_note(&mut self) -> Option<String> {
        self.note.take()
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
        let value = umlcraft_format::parse_value(&line).map_err(io::Error::other)?;
        Ok(Reply::from_json(&value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// This process's control file and what the server running in it announced there.
    fn announcement() -> (PathBuf, Value) {
        let file = control_dir().unwrap().join(format!("live-{}.json", std::process::id()));
        let announcement: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        (file, announcement)
    }

    // The tests that start a server share its control file (one per process): one at a time.
    static SERVER: Mutex<()> = Mutex::new(());

    #[test]
    fn calls_reach_the_handler_with_the_right_token_only() {
        let _one = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        let server = LiveServer::start(|request| {
            assert!(request.take());
            let text = format!("{} {}", request.name, request.arguments["x"]);
            request.answer(Reply::text(text));
        })
        .unwrap();
        let (file, announcement) = announcement();
        let token = announcement["token"].as_str().unwrap();
        let proof = announcement["proof"].as_str().unwrap();

        let mut client = LiveClient::open(server.port(), token, Some(proof)).unwrap();
        assert_eq!(client.take_note(), None);
        let reply = client.call("echo", &json!({ "x": 7 }), &Access::default()).unwrap();
        assert_eq!(reply, Reply::text("echo 7"));
        assert!(LiveClient::connect(server.port(), "guess").is_err());
        // An app that does not know the proof is not the one that wrote the file.
        assert!(LiveClient::open(server.port(), token, Some("other")).is_err());

        // Clients that showed the token stay connected without taking room from new ones.
        let known: Vec<LiveClient> = (0..MAX_WAITING + 4)
            .map(|_| LiveClient::connect(server.port(), token).unwrap())
            .collect();
        let mut last = LiveClient::connect(server.port(), token).unwrap();
        assert_eq!(
            last.call("echo", &json!({ "x": 8 }), &Access::default()).unwrap(),
            Reply::text("echo 8")
        );
        drop(known);

        drop(server);
        assert!(!file.exists());
    }

    #[test]
    fn a_call_the_app_did_not_take_up_in_time_never_runs() {
        let _one = SERVER.lock().unwrap_or_else(|e| e.into_inner());
        let held: Arc<Mutex<Vec<Request>>> = Arc::default();
        let server = LiveServer::start_waiting(
            {
                let held = held.clone();
                move |request| held.lock().unwrap().push(request)
            },
            Duration::from_millis(200),
        )
        .unwrap();
        let (_, announcement) = announcement();
        let mut client =
            LiveClient::connect(server.port(), announcement["token"].as_str().unwrap()).unwrap();
        let reply = client.call("create_diagram", &json!({}), &Access::default()).unwrap();
        assert!(reply.is_error);
        assert!(format!("{reply:?}").contains("not carried out"), "{reply:?}");
        // The app gets to the call later: it must leave it alone.
        let request = held.lock().unwrap().pop().unwrap();
        assert!(!request.take());
    }

    #[test]
    fn a_call_taken_up_is_waited_for() {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(AtomicU8::new(WAITING));
        let request = Request {
            name: "slow".into(),
            arguments: json!({}),
            access: Access::default(),
            reply: tx,
            state: state.clone(),
        };
        std::thread::spawn(move || {
            assert!(request.take());
            std::thread::sleep(Duration::from_millis(700));
            request.answer(Reply::text("done"));
        });
        assert_eq!(wait_for(&rx, &state, Duration::from_millis(500)), Reply::text("done"));
    }

    #[test]
    fn the_first_line_has_a_deadline_however_slowly_it_comes() {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).unwrap();
        let address = listener.local_addr().unwrap();
        let trickle = std::thread::spawn(move || {
            let mut client = TcpStream::connect(address).unwrap();
            // One byte every 50 ms, each well within a per-read timeout.
            for _ in 0..40 {
                if client.write_all(b"x").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let (stream, _) = listener.accept().unwrap();
        let start = Instant::now();
        let deadline = Some(start + Duration::from_millis(300));
        let mut reader = BufReader::new(Timed { stream, until: deadline });
        assert_eq!(read_line(&mut reader, FIRST_LINE), None);
        assert!(start.elapsed() < Duration::from_millis(1500), "{:?}", start.elapsed());
        drop(reader);
        trickle.join().unwrap();
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
