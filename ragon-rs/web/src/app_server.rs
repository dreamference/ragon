//! The user's `ragon app-server`, reached over its Unix socket (specs/DREAMFERENCE_RAGON_ASK.md
//! §2.1). In `--listen unix://PATH` mode the server takes any number of clients, one WebSocket
//! each, so every browser tab gets a connection of its own and none of them owns the server.
//!
//! On Windows, which tokio gives no Unix socket, the server listens on loopback instead
//! (`--listen ws://127.0.0.1:PORT`) and takes only clients that bring its capability token
//! (`--ws-auth capability-token`): `socket` then names an endpoint file holding the port, with the
//! token in a private file beside it, so another user on the machine cannot reach the server.
//!
//! When nothing answers on the socket, `ragon web` starts the server itself, as its child: the
//! same command the desktop app runs, with the model server named in `DREAMFERENCE_VLLM_HOST` so
//! the child never browses for a node (an mDNS query is egress). Its stderr, where the launcher
//! says it is waiting for a model server, goes to every tab and to `app-server.log`.

use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;

/// The byte stream under a connection to the app-server: its Unix socket.
#[cfg(unix)]
pub type Stream = UnixStream;
/// The byte stream under a connection to the app-server: its loopback port, with the token.
#[cfg(not(unix))]
pub type Stream = tokio::net::TcpStream;

/// A connection to the app-server.
pub type Upstream = WebSocketStream<Stream>;

/// A stand-in app-server's listener for tests: on the socket, or on loopback with the endpoint
/// file written, as the real server would be reached on this platform.
pub struct TestListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(not(unix))]
    inner: tokio::net::TcpListener,
}

impl TestListener {
    pub fn bind(socket: &Path) -> std::io::Result<TestListener> {
        prepare_socket_dir(socket)?;
        #[cfg(unix)]
        let inner = tokio::net::UnixListener::bind(socket)?;
        #[cfg(not(unix))]
        let inner = {
            let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
            listener.set_nonblocking(true)?;
            write_endpoint(socket, listener.local_addr()?.port())?;
            tokio::net::TcpListener::from_std(listener)?
        };
        Ok(TestListener { inner })
    }

    pub async fn accept(&self) -> std::io::Result<Stream> {
        Ok(self.inner.accept().await?.0)
    }
}

/// How long a start may take: the launcher first waits for a model server that may still be loading.
pub const START_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// What starts `ragon app-server` and where it listens.
#[derive(Clone, Debug)]
pub struct Launch {
    /// The `ragon` binary.
    pub ragon: PathBuf,
    /// Environment added to the child's.
    pub env: Vec<(String, String)>,
    /// Where its stderr is kept.
    pub log: PathBuf,
}

/// A server this process started, and the task copying its stderr, which ends with its last line.
struct Running {
    child: Child,
    stderr: Option<JoinHandle<Option<String>>>,
}

pub struct AppServer {
    socket: PathBuf,
    launch: Launch,
    child: Mutex<Option<Running>>,
    stderr: broadcast::Sender<String>,
}

/// Creates the socket's folder, private to the user: the app-server refuses a parent others could
/// write to.
pub fn prepare_socket_dir(socket: &Path) -> std::io::Result<()> {
    match socket.parent() {
        Some(parent) => crate::auth::private_dir(parent),
        None => Ok(()),
    }
}

/// The socket to use: `$XDG_RUNTIME_DIR/ragon/app-server.sock`, else one in the state folder.
#[cfg(unix)]
pub fn default_socket(state_dir: &Path) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("ragon").join("app-server.sock"),
        None => state_dir.join("run").join("app-server.sock"),
    }
}

/// The endpoint file to use on Windows: the loopback port, in the state folder.
#[cfg(not(unix))]
pub fn default_socket(state_dir: &Path) -> PathBuf {
    state_dir.join("run").join("app-server.endpoint")
}

/// The capability token's file, beside the endpoint file.
pub fn token_file(endpoint: &Path) -> PathBuf {
    endpoint.with_extension("token")
}

/// Writes a fresh token and the port the server is to listen on, both private to the user, and
/// returns the `--listen` URL.
pub fn write_endpoint(endpoint: &Path, port: u16) -> std::io::Result<String> {
    crate::auth::write_private(&token_file(endpoint), &crate::auth::random_hex(32))?;
    crate::auth::write_private(endpoint, &port.to_string())?;
    Ok(format!("ws://127.0.0.1:{port}"))
}

/// A free loopback port, for a server about to be started.
pub fn free_loopback_port() -> std::io::Result<u16> {
    Ok(std::net::TcpListener::bind(("127.0.0.1", 0))?.local_addr()?.port())
}

/// Connects to the server the endpoint file names, on loopback, with its token.
pub async fn connect_loopback(endpoint: &Path) -> std::io::Result<WebSocketStream<tokio::net::TcpStream>> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let port: u16 = std::fs::read_to_string(endpoint)?
        .trim()
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "no port in the endpoint file"))?;
    let token = std::fs::read_to_string(token_file(endpoint))?;
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", port)).await?;
    let mut request = format!("ws://127.0.0.1:{port}/")
        .into_client_request()
        .map_err(|err| std::io::Error::other(err.to_string()))?;
    let bearer = format!("Bearer {}", token.trim())
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "the token is not a header value"))?;
    request.headers_mut().insert("Authorization", bearer);
    let (upstream, _) =
        tokio_tungstenite::client_async(request, stream).await.map_err(|err| std::io::Error::other(err.to_string()))?;
    Ok(upstream)
}

impl AppServer {
    pub fn new(socket: PathBuf, launch: Launch) -> AppServer {
        let (stderr, _) = broadcast::channel(256);
        AppServer { socket, launch, child: Mutex::new(None), stderr }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The launcher's lines from a server this process started, for the tabs' start-up screen.
    pub fn stderr(&self) -> broadcast::Receiver<String> {
        self.stderr.subscribe()
    }

    /// One attempt to connect.
    #[cfg(not(unix))]
    pub async fn connect_once(&self) -> std::io::Result<Upstream> {
        connect_loopback(&self.socket).await
    }

    /// One attempt to connect.
    #[cfg(unix)]
    pub async fn connect_once(&self) -> std::io::Result<Upstream> {
        let stream = UnixStream::connect(&self.socket).await?;
        let (upstream, _) = tokio_tungstenite::client_async("ws://localhost/", stream)
            .await
            .map_err(|err| std::io::Error::other(err.to_string()))?;
        Ok(upstream)
    }

    /// Connects, starting the server first when nothing answers. Returns the connection and
    /// whether this call started the server.
    pub async fn connect(&self) -> Result<(Upstream, bool), String> {
        if let Ok(upstream) = self.connect_once().await {
            return Ok((upstream, false));
        }
        let mut child = self.child.lock().await;
        // Another tab may have started it while this one waited for the lock.
        if let Ok(upstream) = self.connect_once().await {
            return Ok((upstream, false));
        }
        let running = match child.as_mut() {
            Some(running) => matches!(running.child.try_wait(), Ok(None)),
            None => false,
        };
        if !running {
            *child = Some(self.spawn()?);
        }
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            if let Ok(upstream) = self.connect_once().await {
                return Ok((upstream, true));
            }
            if let Some(running) = child.as_mut()
                && let Ok(Some(status)) = running.child.try_wait()
            {
                // Its last words say why; the copy ends with the pipe, soon after the exit.
                let last = match running.stderr.take() {
                    Some(task) => tokio::time::timeout(Duration::from_secs(5), task).await.ok().and_then(Result::ok).flatten(),
                    None => None,
                };
                *child = None;
                let last = last.map(|line| format!(": {line}")).unwrap_or_default();
                return Err(format!(
                    "ragon app-server exited ({status}) before it listened{last} (see {})",
                    self.launch.log.display()
                ));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("ragon app-server did not start listening in time".to_string());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    fn spawn(&self) -> Result<Running, String> {
        prepare_socket_dir(&self.socket).map_err(|err| format!("could not prepare {}: {err}", self.socket.display()))?;
        let mut command = Command::new(&self.launch.ragon);
        command.args(["-c", "features.code_mode_host=true", "app-server", "--listen"]);
        #[cfg(unix)]
        command.arg(format!("unix://{}", self.socket.display()));
        #[cfg(not(unix))]
        {
            let listen = free_loopback_port()
                .and_then(|port| write_endpoint(&self.socket, port))
                .map_err(|err| format!("could not prepare {}: {err}", self.socket.display()))?;
            command.arg(listen).args(["--ws-auth", "capability-token", "--ws-token-file"]).arg(token_file(&self.socket));
        }
        command
            .envs(self.launch.env.iter().map(|(key, value)| (key.as_str(), value.as_str())))
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child =
            command.spawn().map_err(|err| format!("could not start {} app-server: {err}", self.launch.ragon.display()))?;
        let stderr = child.stderr.take().map(|stderr| {
            let sender = self.stderr.clone();
            let log = self.launch.log.clone();
            tokio::spawn(async move {
                let mut file = tokio::fs::OpenOptions::new().create(true).append(true).open(&log).await.ok();
                let redactor = ragon_log::Redactor::new(ragon_log::SECRETS_TXT, ragon_log::Env::from_system());
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                let mut last = None;
                while let Ok(Some(line)) = lines.next_line().await {
                    // Redaction at the writer (specs/DREAMFERENCE_LOGGING.md §7): the file, every
                    // tab and the exit message only ever see the masked line.
                    let line = redactor.redact(&line, true);
                    if let Some(file) = file.as_mut() {
                        let _ = file.write_all(format!("{line}\n").as_bytes()).await;
                    }
                    let _ = sender.send(line.clone());
                    last = Some(line);
                }
                if let Some(file) = file.as_mut() {
                    let _ = file.flush().await;
                }
                last
            })
        });
        Ok(Running { child, stderr })
    }

    /// Stops a server this process started; one started elsewhere is left alone.
    pub async fn stop_child(&self) {
        if let Some(mut running) = self.child.lock().await.take() {
            let _ = running.child.kill().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static ENV: StdMutex<()> = StdMutex::new(());

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ragon-app-server-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn launch(ragon: &str, log: PathBuf) -> Launch {
        Launch { ragon: PathBuf::from(ragon), env: Vec::new(), log }
    }

    #[cfg(unix)]
    fn with_runtime_dir<T>(value: Option<&str>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let before = std::env::var_os("XDG_RUNTIME_DIR");
        // SAFETY: serialised by ENV; no other test in this module reads the variable.
        unsafe {
            match value {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
        let out = body();
        unsafe {
            match before {
                Some(v) => std::env::set_var("XDG_RUNTIME_DIR", v),
                None => std::env::remove_var("XDG_RUNTIME_DIR"),
            }
        }
        out
    }

    #[cfg(unix)]
    #[test]
    fn default_socket_prefers_the_runtime_dir() {
        let socket = with_runtime_dir(Some("/run/user/1"), || default_socket(Path::new("/state")));
        assert_eq!(socket, PathBuf::from("/run/user/1/ragon/app-server.sock"));
    }

    #[cfg(unix)]
    #[test]
    fn default_socket_falls_back_to_the_state_dir_when_unset_or_empty() {
        let unset = with_runtime_dir(None, || default_socket(Path::new("/state")));
        let empty = with_runtime_dir(Some(""), || default_socket(Path::new("/state")));
        assert_eq!(unset, PathBuf::from("/state/run/app-server.sock"));
        assert_eq!(empty, unset);
    }

    #[test]
    fn prepare_socket_dir_creates_a_private_parent() {
        let root = scratch("prepare");
        let socket = root.join("a").join("b").join("s.sock");
        prepare_socket_dir(&socket).unwrap();
        let meta = std::fs::metadata(root.join("a").join("b")).unwrap();
        assert!(meta.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(meta.permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(not(unix))]
    #[test]
    fn default_socket_on_windows_is_an_endpoint_file_in_the_state_dir() {
        let endpoint = default_socket(Path::new("state"));
        assert_eq!(endpoint, Path::new("state").join("run").join("app-server.endpoint"));
        assert_eq!(token_file(&endpoint), Path::new("state").join("run").join("app-server.token"));
    }

    #[test]
    fn write_endpoint_keeps_the_port_and_a_fresh_private_token() {
        let root = scratch("endpoint");
        let endpoint = root.join("run").join("app-server.endpoint");
        assert_eq!(write_endpoint(&endpoint, 4321).unwrap(), "ws://127.0.0.1:4321");
        assert_eq!(std::fs::read_to_string(&endpoint).unwrap(), "4321");
        let first = std::fs::read_to_string(token_file(&endpoint)).unwrap();
        assert_eq!(first.len(), 64, "{first}");
        write_endpoint(&endpoint, 4321).unwrap();
        assert_ne!(std::fs::read_to_string(token_file(&endpoint)).unwrap(), first);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_file(&endpoint)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_free_loopback_port_is_not_zero() {
        assert_ne!(free_loopback_port().unwrap(), 0);
    }

    #[tokio::test]
    async fn connect_loopback_without_an_endpoint_fails() {
        let root = scratch("noendpoint");
        assert!(connect_loopback(&root.join("app-server.endpoint")).await.is_err());
        std::fs::write(root.join("app-server.endpoint"), "not a port").unwrap();
        assert!(connect_loopback(&root.join("app-server.endpoint")).await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn connect_loopback_brings_the_token() {
        use tokio_tungstenite::tungstenite::handshake::server::Request;
        use tokio_tungstenite::tungstenite::handshake::server::Response;
        let root = scratch("token");
        let endpoint = root.join("app-server.endpoint");
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        write_endpoint(&endpoint, listener.local_addr().unwrap().port()).unwrap();
        let token = std::fs::read_to_string(token_file(&endpoint)).unwrap();
        let seen = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut header = None;
            let _ = tokio_tungstenite::accept_hdr_async(stream, |request: &Request, response: Response| {
                header = request.headers().get("Authorization").and_then(|value| value.to_str().ok()).map(str::to_string);
                Ok(response)
            })
            .await;
            header
        });
        assert!(connect_loopback(&endpoint).await.is_ok());
        assert_eq!(seen.await.unwrap(), Some(format!("Bearer {token}")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_once_on_unix_uses_the_socket_not_an_endpoint_file() {
        let root = scratch("unixonly");
        let socket = root.join("s.sock");
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        write_endpoint(&socket, listener.local_addr().unwrap().port()).unwrap();
        let server = AppServer::new(socket, launch("/bin/true", root.join("log")));
        assert!(server.connect_once().await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn prepare_socket_dir_with_no_parent_is_a_no_op() {
        assert!(prepare_socket_dir(Path::new("/")).is_ok());
    }

    #[tokio::test]
    async fn connect_once_to_a_missing_socket_fails() {
        let root = scratch("missing");
        let server = AppServer::new(root.join("none.sock"), launch("/bin/true", root.join("log")));
        assert!(server.connect_once().await.is_err());
        assert_eq!(server.socket(), root.join("none.sock"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn connect_once_to_a_non_websocket_listener_fails() {
        let root = scratch("plain");
        let path = root.join("s.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            }
        });
        let server = AppServer::new(path, launch("/bin/true", root.join("log")));
        assert!(server.connect_once().await.is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_ragon_that_cannot_be_started_is_reported() {
        let root = scratch("nospawn");
        let server = AppServer::new(root.join("run").join("s.sock"), launch("/nonexistent/ragon", root.join("log")));
        let err = server.connect().await.err().expect("must fail");
        assert!(err.contains("could not start"), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_server_that_exits_early_reports_its_last_line_and_the_log() {
        let root = scratch("exits");
        let script = root.join("fake-ragon");
        std::fs::write(&script, "#!/bin/sh\necho 'model server missing' >&2\nexit 3\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = root.join("app-server.log");
        let server = AppServer::new(root.join("run").join("s.sock"), launch(script.to_str().unwrap(), log.clone()));
        let err = server.connect().await.err().expect("must fail");
        assert!(err.contains("exited"), "{err}");
        assert!(err.contains("model server missing"), "{err}");
        assert!(err.contains(log.to_str().unwrap()), "{err}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stderr_lines_are_redacted_before_they_are_written() {
        let root = scratch("redact");
        let script = root.join("ragon");
        // A token of the `github` shape in vectors/logging/secrets.txt.
        let token = format!("ghp_{}", "a1".repeat(18));
        std::fs::write(&script, format!("#!/bin/sh\necho \"saw {token} here\" >&2\nexit 1\n")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let log = root.join("app-server.log");
        let server = AppServer::new(root.join("run").join("s.sock"), launch(script.to_str().unwrap(), log.clone()));
        let err = server.connect().await.err().expect("must fail");
        let kept = std::fs::read_to_string(&log).unwrap();
        assert!(!kept.contains(&token), "{kept}");
        assert!(kept.contains("saw [secret] here"), "{kept}");
        assert!(!err.contains(&token), "{err}");
        assert!(ragon_log::SECRETS_TXT.contains("github|gh|"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn stop_child_without_a_child_is_harmless() {
        let root = scratch("stop");
        let server = AppServer::new(root.join("s.sock"), launch("/bin/true", root.join("log")));
        server.stop_child().await;
        let _rx = server.stderr();
        let _ = std::fs::remove_dir_all(root);
    }
}
