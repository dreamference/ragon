//! The kill drills against the real binaries: the supervisor supervising the stand-in engine.
//! - the stand-in killed: restarted after 1 s, then (killed again) after 2 s;
//! - the supervisor killed (Windows): the engine dies with it (Job Object);
//! - `server start|status|stop` without a service: a detached supervisor, stable status output;
//! - an engine that refuses (exit 3) is not restarted;
//! - `--advertise`: a browse sees the node record (TXT, `loading` then `ready`) and sees it go when
//!   the supervisor stops (needs multicast, so it is `#[ignore]`: `cargo test -- --ignored`).
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
    assert_eq!(
        keys,
        [
            "service", "service_state", "supervisor", "state", "supervisor_pid", "engine_pid", "url", "probe",
            "restarts", "last_exit", "next_start_s", "last_error", "updated"
        ]
    );
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

#[test]
#[ignore = "needs a multicast-capable interface"]
fn the_advertisement_follows_the_engine_and_goes_with_the_supervisor() {
    use mdns_sd::{ServiceDaemon, ServiceEvent};
    let dir = state_dir("advertise");
    let port = free_port();
    let node = format!("drill-{}", std::process::id());
    let browser = ServiceDaemon::new().unwrap();
    let rx = browser.browse(ragon_supervisor::advertise::NODE_TYPE).unwrap();
    let mut sup = Command::new(SUPERVISOR)
        .args(["server", "run", "--state-dir"])
        .arg(&dir)
        .args(["--port", &port.to_string(), "--advertise", &node, "--node-version", "9.9.9", "--web-port", "3100", "--main"])
        .args(engine_args(port, &[]))
        .spawn()
        .unwrap();
    let mut states = Vec::new();
    let until = Instant::now() + Duration::from_secs(30);
    while !states.contains(&"ready".to_string()) && Instant::now() < until {
        if let Ok(ServiceEvent::ServiceResolved(info)) = rx.recv_timeout(Duration::from_millis(500))
            && info.get_property_val_str("node") == Some(node.as_str())
        {
            assert_eq!(info.get_port(), port);
            assert_eq!(info.get_property_val_str("proto"), Some("1"));
            assert_eq!(info.get_property_val_str("version"), Some("9.9.9"));
            assert_eq!(info.get_property_val_str("web"), Some("3100"));
            assert_eq!(info.get_property_val_str("main"), Some("1"));
            states.push(info.get_property_val_str("state").unwrap().to_string());
        }
    }
    assert!(states.contains(&"ready".to_string()), "never saw ready: {states:?}");
    assert!(server(&dir, port, &["stop"], false).status.success());
    let _ = sup.wait();
    let until = Instant::now() + Duration::from_secs(10);
    let mut gone = false;
    while !gone && Instant::now() < until {
        if let Ok(ServiceEvent::ServiceRemoved(..)) = rx.recv_timeout(Duration::from_millis(500)) {
            gone = true;
        }
    }
    assert!(gone, "the record was not withdrawn when the supervisor stopped");
    let _ = browser.shutdown();
}
