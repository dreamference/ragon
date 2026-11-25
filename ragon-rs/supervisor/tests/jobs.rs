//! Job drills (specs/DREAMFERENCE_MAC_NODE.md §4.5): a real `ragon-supervisor jobs serve` runs the
//! stand-in job; a client process submits it and is killed; the memory cap, the time limit, the
//! SIGTERM-then-SIGKILL escalation, `cancel` and `fetched` are checked through the socket.
//!
//! The runner is unsandboxed here (`--no-sandbox`) so the drills run the same on Linux and macOS;
//! the Seatbelt probes are tests/mac_sandbox.rs.
#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use ragon_supervisor::jobs::{parse_answer, request, Record};

const SUPERVISOR: &str = env!("CARGO_BIN_EXE_ragon-supervisor");
const STANDIN: &str = env!("CARGO_BIN_EXE_ragon-standin-job");

struct Runner {
    child: Child,
    socket: PathBuf,
    root: PathBuf,
    _dir: TempDir,
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TempDir(PathBuf);
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(name: &str) -> TempDir {
    // Short: a unix socket path is limited to about 100 bytes.
    let dir = std::env::temp_dir().join(format!("rsj-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    TempDir(dir)
}

fn wait_until(what: &str, timeout: Duration, mut ok: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ok() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn start(name: &str) -> Runner {
    start_with(name, &["--no-sandbox"])
}

fn start_with(name: &str, extra: &[&str]) -> Runner {
    let dir = temp_dir(name);
    let root = dir.0.join("jobs");
    let socket = dir.0.join("s");
    let child = Command::new(SUPERVISOR)
        .args(["jobs", "serve", "--sample-ms", "100", "--grace-ms", "1000"])
        .args(extra)
        .arg("--root")
        .arg(&root)
        .arg("--socket")
        .arg(&socket)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("the job socket", Duration::from_secs(10), || socket.exists());
    Runner { child, socket, root, _dir: dir }
}

fn ask(r: &Runner, words: &[&str]) -> Result<Record, String> {
    let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    parse_answer(&request(&r.socket, &words).unwrap())
}

fn submit(r: &Runner, id: &str, memory: &str, time: &str, cmd: &[&str]) -> Record {
    let mut words = vec!["submit".to_string(), format!("id={id}"), format!("memory={memory}"), format!("time={time}")];
    words.push(format!("arg={}", ragon_supervisor::jobs::encode(STANDIN)));
    words.extend(cmd.iter().map(|a| format!("arg={}", ragon_supervisor::jobs::encode(a))));
    parse_answer(&request(&r.socket, &words).unwrap()).unwrap()
}

fn status(r: &Runner, id: &str) -> Record {
    ask(r, &["status", id]).unwrap()
}

fn wait_final(r: &Runner, id: &str, timeout: Duration) -> Record {
    let mut last = Record::new();
    wait_until(&format!("{id} to finish"), timeout, || {
        last = status(r, id);
        last.contains_key("finished")
    });
    last
}

fn alive(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    // Signal 0 checks existence; a zombie of a reaped job does not linger (the runner waits).
    unsafe { kill(pid, 0) == 0 }
}

fn log(r: &Runner, id: &str) -> String {
    std::fs::read_to_string(r.root.join(id).join("output.log")).unwrap_or_default()
}

#[test]
fn a_job_finishes_and_fetched_is_recorded_once() {
    let r = start("done");
    let id = "20261125-1200-a01";
    submit(&r, id, "64M", "30", &["print", "hello from the job"]);
    let rec = wait_final(&r, id, Duration::from_secs(20));
    assert_eq!(rec["status"], "done", "{rec:?}");
    assert_eq!(rec["exit_code"], "0");
    assert!(log(&r, id).contains("hello from the job"));
    // logs over the socket
    let logs = request(&r.socket, &["logs".into(), id.into()]).unwrap();
    assert!(String::from_utf8_lossy(&logs).contains("hello from the job"));
    let first = ask(&r, &["fetched", id]).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    let second = ask(&r, &["fetched", id]).unwrap();
    assert_eq!(first["fetched"], second["fetched"], "fetched keeps its first time");
    assert_eq!(ask(&r, &["cancel", id]).unwrap_err(), "the job is not running");
    assert_eq!(ask(&r, &["status", "20261125-1200-fff"]).unwrap_err(), "no such job");
}

#[test]
fn the_memory_cap_trips() {
    let r = start("mem");
    let id = "20261125-1200-a02";
    submit(&r, id, "64M", "60", &["hog", "200", "60"]);
    let rec = wait_final(&r, id, Duration::from_secs(30));
    assert_eq!(rec["status"], "failed", "{rec:?}");
    assert_eq!(rec["stopped_by"], "memory", "{rec:?}");
    assert!(rec["peak_footprint"].parse::<u64>().unwrap() > 64 << 20, "{rec:?}");
}

#[test]
fn the_time_limit_trips() {
    let r = start("time");
    let id = "20261125-1200-a03";
    let t0 = Instant::now();
    submit(&r, id, "64M", "2", &["sleep", "60"]);
    let rec = wait_final(&r, id, Duration::from_secs(20));
    assert_eq!(rec["stopped_by"], "time", "{rec:?}");
    assert_eq!(rec["status"], "failed");
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
}

#[test]
fn a_job_that_ignores_sigterm_gets_sigkill_after_the_grace() {
    let r = start("kill");
    let id = "20261125-1200-a04";
    let t0 = Instant::now();
    submit(&r, id, "64M", "1", &["stubborn", "60"]);
    wait_until("the stubborn job to say it ignores SIGTERM", Duration::from_secs(10), || {
        log(&r, id).contains("ignoring SIGTERM")
    });
    let rec = wait_final(&r, id, Duration::from_secs(20));
    let took = t0.elapsed();
    assert_eq!(rec["stopped_by"], "time", "{rec:?}");
    // Killed, not exited: 128 + 9 never appears as exit_code because the ending is the limit.
    assert!(!rec.contains_key("exit_code"), "{rec:?}");
    // 1 s limit + 1 s grace, then SIGKILL.
    assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(10), "{took:?}");
    assert!(!alive(rec["pid"].parse().unwrap()));
}

#[test]
fn cancel_stops_the_whole_group_and_stays_cancelled() {
    let r = start("cancel");
    let id = "20261125-1200-a05";
    submit(&r, id, "64M", "60", &["spawn", "60"]);
    let mut child_pid = 0;
    wait_until("the grandchild", Duration::from_secs(10), || {
        child_pid = log(&r, id).lines().find_map(|l| l.strip_prefix("child ")?.trim().parse().ok()).unwrap_or(0);
        child_pid > 0
    });
    let rec = ask(&r, &["cancel", id]).unwrap();
    assert_eq!(rec["status"], "cancelled");
    let rec = wait_final(&r, id, Duration::from_secs(20));
    assert_eq!(rec["status"], "cancelled", "{rec:?}");
    assert_eq!(rec["stopped_by"], "cancel");
    wait_until("the grandchild to be gone", Duration::from_secs(5), || !alive(child_pid));
    assert_eq!(ask(&r, &["fetched", id]).unwrap()["status"], "cancelled");
}

/// The brief's "job survives the end of the SSH session": the job is the runner's child in a
/// group of its own, so killing the client (and its whole process group, as sshd's hang-up does)
/// leaves it running.
#[test]
fn a_job_outlives_the_client_that_sent_it() {
    let r = start("ssh");
    let id = "20261125-1200-a06";
    // The client runs in a group of its own, as an SSH session's shell does.
    let mut client = Command::new(SUPERVISOR)
        .args(["jobs", "submit", "--socket"])
        .arg(&r.socket)
        .args([&format!("id={id}"), "memory=64M", "time=60", &format!("arg={STANDIN}"), "arg=sleep", "arg=30"])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("the job to run", Duration::from_secs(10), || {
        r.root.join(id).join("record.txt").exists() && status(&r, id)["status"] == "running"
    });
    let _ = client.kill();
    let _ = client.wait();
    let pid: i32 = status(&r, id)["pid"].parse().unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(alive(pid), "the job died with its client");
    assert_eq!(status(&r, id)["status"], "running");
    assert_eq!(ask(&r, &["cancel", id]).unwrap()["status"], "cancelled");
    wait_final(&r, id, Duration::from_secs(20));
}

#[test]
fn a_restarted_runner_settles_running_records_as_failed() {
    let dir = temp_dir("restart");
    let root = dir.0.join("jobs");
    let id = "20261125-1200-a07";
    std::fs::create_dir_all(root.join(id)).unwrap();
    std::fs::write(root.join(id).join("record.txt"), format!("id: {id}\nstatus: running\n")).unwrap();
    let server = ragon_supervisor::jobs::JobServer::new(root, std::sync::Arc::new(ragon_supervisor::footprint::SystemFootprint));
    server.settle_after_restart();
    let rec = server.read(id).unwrap();
    assert_eq!(rec["status"], "failed");
    assert!(rec["note"].contains("restarted"));
}

/// Without the Command Line Tools a job that needs git is refused with the command to run
/// (`/usr/bin/false` stands in for an `xcode-select -p` that finds no developer folder).
#[test]
fn a_git_job_without_the_command_line_tools_is_refused_with_the_command_to_run() {
    let r = start_with("xcode", &["--no-sandbox", "--xcode-select", "/usr/bin/false"]);
    let err = ask(
        &r,
        &[
            "submit",
            "id=20261125-1200-a08",
            "memory=64M",
            "repo=/nonexistent.git",
            "commit=0123456789abcdef0123456789abcdef01234567",
            "arg=true",
        ],
    )
    .unwrap_err();
    assert!(err.contains("xcode-select --install"), "{err}");
    assert!(!r.root.join("20261125-1200-a08").exists());
}

/// The runner wraps the job in its profile on macOS: the job's own record shows the denial.
#[cfg(target_os = "macos")]
#[test]
fn the_runner_sandboxes_the_job() {
    let r = start_with("sandbox", &[]);
    let home = std::env::var("HOME").unwrap();
    let id = "20261125-1200-a09";
    submit(&r, id, "64M", "30", &["list", &home]);
    let rec = wait_final(&r, id, Duration::from_secs(20));
    assert_eq!(rec["status"], "failed", "{rec:?}");
    assert!(log(&r, id).contains("denied: list"), "{}", log(&r, id));
    let profile = std::fs::read_to_string(r.root.join(id).join("sandbox.sb")).unwrap();
    assert!(profile.contains("(deny default)"));
    let id = "20261125-1200-a0a";
    submit(&r, id, "64M", "30", &["print", "inside"]);
    assert_eq!(wait_final(&r, id, Duration::from_secs(20))["status"], "done");
}
