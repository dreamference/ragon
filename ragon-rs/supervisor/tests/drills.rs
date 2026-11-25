//! The kill drills against the real binaries: the supervisor supervising the stand-in engine.
//! - the stand-in killed: restarted after 1 s, then (killed again) after 2 s;
//! - the supervisor killed (Windows: Job Object; macOS: the engine's process group and its guard):
//!   the engine dies with it;
//! - the supervisor sent SIGTERM (Unix, how launchd stops the daemon): it stops the engine and exits 0;
//! - `server start|status|stop` without a service: a detached supervisor, stable status output;
//! - an engine that refuses (exit 3) is not restarted;
//! - the node's hooks (Unix, in process): each state reported once, a killed sidecar restarted
//!   without touching the engine, both stopped with the supervisor;
//! - the tool-call canary passes against the stand-in's streamed call.
//!
//! Ports come from the OS (port 0), never the node's.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::time::{Duration, Instant};

const SUPERVISOR: &str = env!("CARGO_BIN_EXE_ragon-supervisor");
const STANDIN: &str = env!("CARGO_BIN_EXE_ragon-standin-engine");

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn state_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-supervisor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn field(dir: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("status.txt")).ok()?;
    ragon_supervisor::core::parse_status(&text).into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn wait_until(secs: u64, what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !done() {
        assert!(Instant::now() < until, "timed out after {secs} s waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn engine_args(port: u16, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["--".to_string(), STANDIN.to_string(), "--port".to_string(), port.to_string()];
    args.extend(extra.iter().map(|s| s.to_string()));
    args
}

fn run_supervisor(dir: &Path, port: u16, extra: &[&str]) -> Child {
    Command::new(SUPERVISOR)
        .args(["server", "run", "--state-dir"])
        .arg(dir)
        .args(["--port", &port.to_string()])
        .args(engine_args(port, extra))
        .spawn()
        .unwrap()
}

fn server(dir: &Path, port: u16, words: &[&str], engine: bool) -> Output {
    let mut command = Command::new(SUPERVISOR);
    command.arg("server").args(words).arg("--state-dir").arg(dir).args(["--port", &port.to_string()]);
    if engine {
        command.args(engine_args(port, &[]));
    }
    command.output().unwrap()
}

fn kill_pid(pid: u32) {
    let status = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", &pid.to_string()]).output().unwrap().status
    } else {
        Command::new("kill").args(["-9", &pid.to_string()]).output().unwrap().status
    };
    assert!(status.success(), "could not kill {pid}");
}

fn healthy_pid(dir: &Path) -> Option<u32> {
    (field(dir, "state").as_deref() == Some("healthy")).then(|| field(dir, "engine_pid")?.parse().ok())?
}

#[test]
fn a_killed_stand_in_is_restarted_with_backoff() {
    let dir = state_dir("restart");
    let port = free_port();
    let mut sup = run_supervisor(&dir, port, &[]);
    wait_until(30, "first healthy engine", || healthy_pid(&dir).is_some());
    let mut pid = healthy_pid(&dir).unwrap();
    for (round, backoff_s) in [1u64, 2].into_iter().enumerate() {
        let killed_at = Instant::now();
        kill_pid(pid);
        wait_until(30, "the restarted engine", || healthy_pid(&dir).is_some_and(|p| p != pid));
        let took = killed_at.elapsed();
        // The supervisor sees the exit within one tick (at most 2 s), then waits the backoff.
        assert!(took >= Duration::from_millis(backoff_s * 1000 - 100), "round {round}: restarted after {took:?}");
        assert!(took < Duration::from_secs(backoff_s + 15), "round {round}: restarted after {took:?}");
        assert_eq!(field(&dir, "restarts").as_deref(), Some((round + 1).to_string().as_str()));
        assert_ne!(field(&dir, "last_exit").as_deref(), Some("none"));
        pid = healthy_pid(&dir).unwrap();
    }
    std::fs::write(dir.join("stop"), "stop\n").unwrap();
    let status = sup.wait().unwrap();
    assert!(status.success());
    assert_eq!(field(&dir, "state").as_deref(), Some("stopped"));
    assert_eq!(field(&dir, "supervisor_pid").as_deref(), Some("none"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn start_status_stop_without_a_service() {
    let dir = state_dir("startstop");
    let port = free_port();
    let out = server(&dir, port, &["start", "--wait", "20"], true);
    assert!(out.status.success(), "start: {}", String::from_utf8_lossy(&out.stderr));
    let out = server(&dir, port, &["status"], false);
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{text}");
    let keys: Vec<String> = ragon_supervisor::core::parse_status(&text).into_iter().map(|(k, _)| k).collect();
    let mut expected = vec![
        "service", "service_state", "supervisor", "state", "supervisor_pid", "engine_pid", "url", "probe", "restarts",
        "last_exit", "next_start_s", "last_error", "updated",
    ];
    // macOS: the power source follows the shared keys.
    if cfg!(target_os = "macos") {
        expected.push("power");
        assert!(text.contains("\nservice_state: not installed\n"), "{text}");
    }
    assert_eq!(keys, expected);
    assert!(text.contains("\nsupervisor: running\nstate: healthy\n"), "{text}");
    assert!(text.contains(&format!("\nurl: http://127.0.0.1:{port}/v1/models\nprobe: ok\n")), "{text}");
    let engine: u32 = field(&dir, "engine_pid").unwrap().parse().unwrap();
    let out = server(&dir, port, &["stop"], false);
    assert!(out.status.success(), "stop: {}", String::from_utf8_lossy(&out.stderr));
    let out = server(&dir, port, &["status"], false);
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(text.contains("\nsupervisor: stopped\nstate: stopped\nsupervisor_pid: none\nengine_pid: none\n"), "{text}");
    assert!(text.contains("\nprobe: no answer\n"), "the engine still answers after stop: {text}");
    let _ = engine;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refusing_engine_is_not_restarted() {
    let dir = state_dir("refuse");
    let port = free_port();
    let mut sup = run_supervisor(&dir, port, &["--refuse"]);
    wait_until(20, "refused", || field(&dir, "state").as_deref() == Some("refused"));
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(field(&dir, "state").as_deref(), Some("refused"));
    assert_eq!(field(&dir, "restarts").as_deref(), Some("0"));
    assert_eq!(field(&dir, "last_exit").as_deref(), Some("3"));
    std::fs::write(dir.join("stop"), "stop\n").unwrap();
    assert!(sup.wait().unwrap().success());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Windows: the supervisor is killed outright (TerminateProcess, no stop request) and the engine,
/// in the supervisor's Job Object, dies with it.
#[cfg(windows)]
#[test]
fn a_killed_supervisor_takes_the_engine_with_it() {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};

    let dir = state_dir("supkill");
    let port = free_port();
    let mut sup = run_supervisor(&dir, port, &[]);
    wait_until(30, "healthy engine", || healthy_pid(&dir).is_some());
    let engine = healthy_pid(&dir).unwrap();
    // SAFETY: plain Win32 calls on a pid we own; the handle is closed below.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, engine) };
    assert_ne!(handle, 0, "engine {engine} not found");
    sup.kill().unwrap(); // TerminateProcess
    sup.wait().unwrap();
    let waited = unsafe { WaitForSingleObject(handle, 10_000) };
    unsafe { CloseHandle(handle) };
    assert_eq!(waited, WAIT_OBJECT_0, "the engine outlived the supervisor");
    let probe = ragon_supervisor::probe::HttpProbe::models("127.0.0.1", port);
    assert_eq!(probe.status_code(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// True while a process with this pid exists (Unix: `kill -0`).
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).output().unwrap().status.success()
}

/// macOS: the supervisor is killed outright (SIGKILL, no stop request) and the engine, in a
/// process group of its own, is killed by its guard. Runs as a normal user.
#[cfg(target_os = "macos")]
#[test]
fn a_killed_supervisor_takes_the_engine_with_it() {
    let dir = state_dir("supkill");
    let port = free_port();
    let mut sup = run_supervisor(&dir, port, &[]);
    wait_until(30, "healthy engine", || healthy_pid(&dir).is_some());
    let engine = healthy_pid(&dir).unwrap();
    assert!(alive(engine));
    kill_pid(sup.id()); // SIGKILL
    sup.wait().unwrap();
    wait_until(10, "the engine to die with the supervisor", || !alive(engine));
    let probe = ragon_supervisor::probe::HttpProbe::models("127.0.0.1", port);
    assert_eq!(probe.status_code(), None);
    // The killed supervisor left a status file that says running; `status` calls it stale soon.
    let _ = std::fs::remove_dir_all(&dir);
}

/// Unix: SIGTERM (launchd's stop) is a clean stop: the engine first, then exit 0.
#[cfg(unix)]
#[test]
fn a_terminated_supervisor_stops_the_engine_and_exits_cleanly() {
    let dir = state_dir("sigterm");
    let port = free_port();
    let mut sup = run_supervisor(&dir, port, &[]);
    wait_until(30, "healthy engine", || healthy_pid(&dir).is_some());
    let engine = healthy_pid(&dir).unwrap();
    let sent = Command::new("kill").args(["-TERM", &sup.id().to_string()]).status().unwrap();
    assert!(sent.success());
    let status = sup.wait().unwrap();
    assert!(status.success(), "the supervisor exited with {status:?}");
    assert!(!alive(engine), "the engine outlived a clean stop");
    assert_eq!(field(&dir, "state").as_deref(), Some("stopped"));
    assert_eq!(field(&dir, "supervisor_pid").as_deref(), Some("none"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The node's hooks, recorded: one sidecar (a second stand-in, as `ragon web --lan` on a Mac) and
/// every state the supervisor reports.
#[cfg(unix)]
struct Recorder {
    sidecar: Vec<String>,
    states: std::sync::Arc<std::sync::Mutex<Vec<(String, bool)>>>,
}

#[cfg(unix)]
impl ragon_supervisor::cli::NodeHooks for Recorder {
    fn sidecars(&self) -> Vec<(String, Vec<String>)> {
        vec![("web".to_string(), self.sidecar.clone())]
    }
    fn on_state(&mut self, state: &str, admitting: bool) {
        self.states.lock().unwrap().push((state.to_string(), admitting));
    }
}

#[cfg(unix)]
fn pid_in(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(unix)]
#[test]
fn the_hooks_see_each_state_and_a_killed_sidecar_is_restarted_without_touching_the_engine() {
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    let dir = state_dir("hooks");
    let port = free_port();
    let web_port = free_port();
    let web_pid = dir.join("web.pid");
    let mut args: Vec<String> = ["run", "--state-dir"].iter().map(|s| s.to_string()).collect();
    args.push(dir.display().to_string());
    args.extend(["--port".to_string(), port.to_string()]);
    args.extend(engine_args(port, &[]));
    let opts = ragon_supervisor::cli::parse(&args).unwrap();
    let states = Arc::new(Mutex::new(Vec::new()));
    let mut hooks = Recorder {
        sidecar: [STANDIN, "--port", &web_port.to_string(), "--pid-file", &web_pid.display().to_string()]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        states: states.clone(),
    };
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let runner = std::thread::spawn(move || ragon_supervisor::cli::supervise_with(&opts, &thread_stop, &mut hooks));

    wait_until(30, "a healthy engine", || healthy_pid(&dir).is_some());
    let engine = healthy_pid(&dir).unwrap();
    wait_until(30, "the sidecar's pid file", || pid_in(&web_pid).is_some_and(alive));
    let first = pid_in(&web_pid).unwrap();
    wait_until(10, "the healthy state reported", || states.lock().unwrap().iter().any(|(s, _)| s == "healthy"));
    {
        let seen = states.lock().unwrap();
        assert_eq!(seen[0], ("starting".to_string(), true), "{seen:?}");
        assert!(seen.iter().all(|(_, admitting)| *admitting), "on mains admission stays open: {seen:?}");
    }

    let _ = std::fs::remove_file(&web_pid);
    kill_pid(first);
    wait_until(30, "the sidecar restarted", || pid_in(&web_pid).is_some_and(|p| p != first && alive(p)));
    let second = pid_in(&web_pid).unwrap();
    assert_eq!(healthy_pid(&dir), Some(engine), "a sidecar's restart never touches the engine");

    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(runner.join().unwrap(), 0);
    assert_eq!(states.lock().unwrap().last().cloned(), Some(("stopped".to_string(), false)));
    wait_until(10, "the sidecar stopped", || !alive(second));
    wait_until(10, "the engine stopped", || !alive(engine));
    assert_eq!(field(&dir, "state").as_deref(), Some("stopped"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_canary_passes_against_the_stand_in_and_fails_where_nothing_answers() {
    use ragon_supervisor::canary;
    let port = free_port();
    let mut standin = Command::new(STANDIN).args(["--port", &port.to_string()]).spawn().unwrap();
    wait_until(10, "the stand-in to listen", || std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    let verdict = canary::run("127.0.0.1", port, Duration::from_secs(10));
    let _ = standin.kill();
    let _ = standin.wait();
    assert!(verdict.is_ok(), "{verdict:?}");
    assert!(canary::run("127.0.0.1", free_port(), Duration::from_secs(5)).is_err());
}
