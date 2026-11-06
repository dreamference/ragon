//! The user's `ragon app-server`, reached over its Unix socket (specs/DREAMFERENCE_RAGON_ASK.md
//! §2.1). In `--listen unix://PATH` mode the server takes any number of clients, one WebSocket
//! each, so every browser tab gets a connection of its own and none of them owns the server.
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
use tokio::net::UnixStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_tungstenite::WebSocketStream;

/// A connection to the app-server.
pub type Upstream = WebSocketStream<UnixStream>;

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
pub fn default_socket(state_dir: &Path) -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|dir| !dir.is_empty()) {
        Some(dir) => PathBuf::from(dir).join("ragon").join("app-server.sock"),
        None => state_dir.join("run").join("app-server.sock"),
    }
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
        let listen = format!("unix://{}", self.socket.display());
        let mut command = Command::new(&self.launch.ragon);
        command
            .args(["-c", "features.code_mode_host=true", "app-server", "--listen", &listen])
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
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                let mut last = None;
                while let Ok(Some(line)) = lines.next_line().await {
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

    #[test]
    fn default_socket_prefers_the_runtime_dir() {
        let socket = with_runtime_dir(Some("/run/user/1"), || default_socket(Path::new("/state")));
        assert_eq!(socket, PathBuf::from("/run/user/1/ragon/app-server.sock"));
    }

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
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);
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

    #[tokio::test]
    async fn stop_child_without_a_child_is_harmless() {
        let root = scratch("stop");
        let server = AppServer::new(root.join("s.sock"), launch("/bin/true", root.join("log")));
        server.stop_child().await;
        let _rx = server.stderr();
        let _ = std::fs::remove_dir_all(root);
    }
}
