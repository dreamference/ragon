//! The job drills against the real binaries, on Linux and on the Windows node:
//! - a job sent through a process that exits (the SSH session) is run by the detached supervisor;
//! - a job runs in a worktree, its test runs, what it changed is committed on `job/<id>`;
//! - lanes: one job per lane, the next in a lane only after the first;
//! - the time limit and the memory cap: the stop file first (the job's process ends its command
//!   and records the reason), the kill of the whole job after the grace when it does not;
//! - `cancel` and `fetched` as on the Linux node.
//!
//! The job's commands are the fake `ragon` (ragon-job-standin); a job process that does not stop
//! is the same binary in place of `server job exec`.

use std::io::Write;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use ragon_supervisor::job_exec::Sandbox;
use ragon_supervisor::job_runner::{self, ProcessSpawner, Runner};
use ragon_supervisor::jobs::{self, Store};
use ragon_supervisor::json::{self, Value};

const SUPERVISOR: &str = env!("CARGO_BIN_EXE_ragon-supervisor");
const STANDIN: &str = env!("CARGO_BIN_EXE_ragon-job-standin");
const ENGINE: &str = env!("CARGO_BIN_EXE_ragon-standin-engine");

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=Drill", "-c", "user.email=drill@localhost", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A jobs folder with `repos/demo.git` holding one commit; returns (jobs dir, commit).
fn jobs_dir(name: &str) -> (PathBuf, String) {
    let root = temp(name);
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["init", "-q"]);
    std::fs::write(work.join("README"), "demo\n").unwrap();
    git(&work, &["add", "README"]);
    git(&work, &["commit", "-q", "-m", "init"]);
    let commit = git(&work, &["rev-parse", "HEAD"]);
    let jobs = root.join("jobs");
    std::fs::create_dir_all(jobs.join("repos")).unwrap();
    git(&root, &["clone", "-q", "--bare", work.to_str().unwrap(), jobs.join("repos").join("demo.git").to_str().unwrap()]);
    (jobs, commit)
}

fn quote(path: &str) -> String {
    if cfg!(windows) { format!("\"{path}\"") } else { format!("'{path}'") }
}

static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn request(commit: &str, command: &[&str], extra: &[(&str, &str)]) -> Value {
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut request = Value::Object(vec![
        ("id".into(), Value::str(format!("20261125-2000-{:03x}", n + 0x100 * (std::process::id() % 15)))),
        ("repo".into(), Value::str("demo")),
        ("commit".into(), Value::str(commit)),
        ("command".into(), Value::Array(command.iter().map(|w| Value::str(*w)).collect())),
        ("memory".into(), Value::str("1G")),
    ]);
    for (key, value) in extra {
        request.set(key, Value::str(*value));
    }
    request
}

fn job_cmd(jobs: &Path, words: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(SUPERVISOR)
        .args(["server", "job"])
        .args(words)
        .arg("--jobs-dir")
        .arg(jobs)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(text) = stdin {
        child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

/// Submits through `server job submit` (a process that exits, like the SSH session); returns the id.
fn submit(jobs: &Path, request: &Value) -> String {
    let out = job_cmd(jobs, &["submit"], Some(&request.to_json()));
    assert!(out.status.success(), "submit: {}", String::from_utf8_lossy(&out.stderr));
    let record = json::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    jobs::id_of(&record).to_string()
}

fn record(jobs: &Path, id: &str) -> Value {
    Store::new(jobs).read(id).unwrap()
}

fn text<'a>(record: &'a Value, key: &str) -> &'a str {
    record.get(key).and_then(Value::as_str).unwrap_or("")
}

fn wait_until(secs: u64, what: &str, mut done: impl FnMut() -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    while !done() {
        assert!(Instant::now() < until, "timed out after {secs} s waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn final_record(jobs: &Path, id: &str, secs: u64) -> Value {
    wait_until(secs, &format!("job {id} to finish"), || jobs::is_final(jobs::status_of(&record(jobs, id))));
    record(jobs, id)
}

fn exec_runner(jobs: &Path) -> Runner<ProcessSpawner> {
    let mut runner =
        Runner::new(Store::new(jobs), ProcessSpawner, job_runner::exec_prefix(PathBuf::from(SUPERVISOR)), Sandbox::None);
    runner.grace = Duration::from_secs(5);
    runner.start();
    runner
}

/// Ticks until `done`, as the supervisor's thread does.
fn drive(runner: &mut Runner<ProcessSpawner>, secs: u64, what: &str, mut done: impl FnMut(&Runner<ProcessSpawner>) -> bool) {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        runner.tick();
        if done(runner) {
            return;
        }
        assert!(Instant::now() < until, "timed out after {secs} s waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_job_outlives_the_session_that_sent_it() {
    let (jobs, commit) = jobs_dir("detached");
    let state = jobs.parent().unwrap().join("state");
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let server = |words: &[&str], engine: bool| {
        let mut command = Command::new(SUPERVISOR);
        command.arg("server").args(words).arg("--state-dir").arg(&state).args(["--port", &port.to_string()]);
        if engine {
            command.arg("--jobs-dir").arg(&jobs).args(["--", ENGINE, "--port", &port.to_string()]);
        }
        command.output().unwrap()
    };
    let out = server(&["start", "--wait", "20"], true);
    assert!(out.status.success(), "start: {}", String::from_utf8_lossy(&out.stderr));
    // The sender is gone once submit returns; the supervisor runs the job.
    let id = submit(&jobs, &request(&commit, &[STANDIN, "sleep", "1500"], &[]));
    let rec = final_record(&jobs, &id, 60);
    assert_eq!(text(&rec, "status"), "done", "{}", rec.to_json());
    let out = server(&["stop"], false);
    assert!(out.status.success(), "stop: {}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}

#[test]
fn a_job_commits_its_changes_and_lanes_queue() {
    let (jobs, commit) = jobs_dir("lanes");
    let test = format!("{} print tested", quote(STANDIN));
    let a1 = submit(&jobs, &request(&commit, &[STANDIN, "write", "out.txt", "hello"], &[("lane", "a"), ("test", &test)]));
    let a2 = submit(&jobs, &request(&commit, &[STANDIN, "exit", "3"], &[("lane", "a")]));
    let b1 = submit(&jobs, &request(&commit, &[STANDIN, "sleep", "300"], &[("lane", "b")]));
    let mut runner = exec_runner(&jobs);
    runner.tick();
    let mut active = runner.active_ids();
    active.sort();
    let mut first = vec![a1.clone(), b1.clone()];
    first.sort();
    assert_eq!(active, first, "one job per lane");
    assert_eq!(jobs::status_of(&record(&jobs, &a2)), "queued");
    drive(&mut runner, 120, "every job", |r| {
        r.active_ids().is_empty() && Store::new(&jobs).records().iter().all(|r| jobs::is_final(jobs::status_of(r)))
    });
    let r1 = record(&jobs, &a1);
    assert_eq!(text(&r1, "status"), "done", "{}", r1.to_json());
    assert_eq!(text(&r1, "branch"), format!("job/{a1}"));
    assert_eq!(r1.get("test_exit_code").and_then(Value::as_u64), Some(0));
    let repo = jobs.join("repos").join("demo.git");
    let shown = git(&repo, &["show", &format!("job/{a1}:out.txt")]);
    assert_eq!(shown, "hello");
    let log = git(&repo, &["log", "-1", "--format=%an <%ae>|%s", &format!("job/{a1}")]);
    assert!(log.starts_with("Ragon Job <ragon-job@localhost>|job: "), "{log}");
    assert!(!jobs.join(&a1).join("tree").exists(), "the worktree was left behind");
    let r2 = record(&jobs, &a2);
    assert_eq!(text(&r2, "status"), "failed");
    assert_eq!(r2.get("exit_code").and_then(Value::as_u64), Some(3));
    assert!(text(&r2, "started") >= text(&r1, "finished"), "a2 started before a1 finished");
    let out = job_cmd(&jobs, &["logs", &a1, "--follow"], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("tested"));
    let out = job_cmd(&jobs, &["logs", &a2, "--follow"], None);
    assert_eq!(out.status.code(), Some(3));
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}

#[test]
fn the_time_limit_stops_the_job_then_kills_what_does_not_stop() {
    let (jobs, commit) = jobs_dir("time");
    // The job's process ends its command on the stop file and records the limit itself.
    let soft = submit(&jobs, &request(&commit, &[STANDIN, "sleep", "600000"], &[("time", "2s"), ("lane", "soft")]));
    let mut runner = exec_runner(&jobs);
    let started = Instant::now();
    drive(&mut runner, 60, "the soft stop", |r| r.active_ids().is_empty());
    let took = started.elapsed();
    let rec = record(&jobs, &soft);
    assert_eq!(text(&rec, "status"), "failed", "{}", rec.to_json());
    assert_eq!(text(&rec, "stop_reason"), "time limit");
    assert!(text(&rec, "note").contains("time limit (2s)"), "{}", rec.to_json());
    assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(2 + 5), "stopped after {took:?}");

    // A job process that ignores the stop file is killed whole after the grace.
    let hung = submit(&jobs, &request(&commit, &["hung"], &[("time", "1s"), ("lane", "hard")]));
    let mut runner = Runner::new(Store::new(&jobs), ProcessSpawner, vec![STANDIN.into(), "hang".into()], Sandbox::None);
    runner.grace = Duration::from_secs(2);
    let started = Instant::now();
    drive(&mut runner, 60, "the kill", |r| r.active_ids().is_empty());
    let took = started.elapsed();
    let rec = record(&jobs, &hung);
    assert_eq!(text(&rec, "status"), "failed", "{}", rec.to_json());
    assert_eq!(text(&rec, "stop_reason"), "time limit");
    assert_eq!(rec.get("stopped"), Some(&Value::Bool(true)));
    assert!(took >= Duration::from_secs(3) && took < Duration::from_secs(3 + 5), "killed after {took:?}");

    // A shell step whose program runs under the shell (`cmd.exe` on Windows, as in production
    // where it also runs under `ragon sandbox`): the stop ends the program too, and the job's
    // process records the limit itself, before the runner's grace, with the tree removed.
    let setup = format!("{q} hang && {q} print after", q = quote(STANDIN));
    let nested =
        submit(&jobs, &request(&commit, &[STANDIN, "print", "unreached"], &[("time", "2s"), ("lane", "shell"), ("setup", &setup)]));
    let mut runner = exec_runner(&jobs);
    let started = Instant::now();
    drive(&mut runner, 60, "the shell step's stop", |r| r.active_ids().is_empty());
    let took = started.elapsed();
    let rec = record(&jobs, &nested);
    assert_eq!(text(&rec, "status"), "failed", "{}", rec.to_json());
    assert_eq!(text(&rec, "stop_reason"), "time limit");
    assert!(text(&rec, "note").contains("time limit (2s)"), "{}", rec.to_json());
    assert!(took >= Duration::from_secs(2) && took < Duration::from_secs(2 + 5), "stopped after {took:?}");
    assert!(!jobs.join(&nested).join("tree").exists(), "the worktree was left behind");
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}

#[test]
fn the_memory_cap_stops_the_job_then_kills_what_does_not_stop() {
    let (jobs, commit) = jobs_dir("memory");
    let soft = submit(&jobs, &request(&commit, &[STANDIN, "alloc", "1024"], &[("memory", "256M"), ("lane", "soft")]));
    let mut runner = exec_runner(&jobs);
    drive(&mut runner, 120, "the soft stop", |r| r.active_ids().is_empty());
    let rec = record(&jobs, &soft);
    assert_eq!(text(&rec, "status"), "failed", "{}", rec.to_json());
    assert_eq!(text(&rec, "stop_reason"), "memory cap");
    assert!(text(&rec, "note").contains("memory cap (256M)"), "{}", rec.to_json());
    let peak = rec.get("peak_memory_bytes").and_then(Value::as_u64).unwrap_or(0);
    assert!(peak >= 200 << 20, "peak {peak}");
    if cfg!(windows) {
        // The Job Object refuses the allocations past the cap. Its peak can still pass the cap by
        // a few MiB (260.2 MiB at 256M on the drill box, two processes in nested jobs), never by
        // another 16 MiB chunk of the stand-in's.
        assert!(peak < (256 + 16) << 20, "peak {peak} over the cap");
    }

    let hung = submit(&jobs, &request(&commit, &["hog"], &[("memory", "128M"), ("lane", "hard")]));
    let mut runner =
        Runner::new(Store::new(&jobs), ProcessSpawner, vec![STANDIN.into(), "alloc".into(), "1024".into()], Sandbox::None);
    runner.grace = Duration::from_secs(2);
    drive(&mut runner, 120, "the kill", |r| r.active_ids().is_empty());
    let rec = record(&jobs, &hung);
    assert_eq!(text(&rec, "status"), "failed", "{}", rec.to_json());
    assert_eq!(text(&rec, "stop_reason"), "memory cap");
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}

#[test]
fn cancelled_and_fetched_behave_as_on_linux() {
    let (jobs, commit) = jobs_dir("cancel");
    let id = submit(&jobs, &request(&commit, &[STANDIN, "sleep", "600000"], &[]));
    let mut runner = exec_runner(&jobs);
    drive(&mut runner, 30, "the job to run", |_| jobs::status_of(&record(&jobs, &id)) == "running");
    // Not finished: no fetch yet.
    assert_eq!(job_cmd(&jobs, &["fetched", &id], None).status.code(), Some(1));
    let out = job_cmd(&jobs, &["cancel", &id], None);
    assert!(out.status.success(), "cancel: {}", String::from_utf8_lossy(&out.stderr));
    drive(&mut runner, 30, "the cancel", |r| r.active_ids().is_empty());
    let rec = record(&jobs, &id);
    assert_eq!(text(&rec, "status"), "cancelled", "{}", rec.to_json());
    assert_eq!(text(&rec, "note"), "the job was cancelled");
    assert!(rec.get("branch").is_none_or(Value::is_null));
    assert!(!jobs.join(&id).join("tree").exists());
    // A second cancel finds it finished.
    assert_eq!(job_cmd(&jobs, &["cancel", &id], None).status.code(), Some(1));
    assert!(job_cmd(&jobs, &["fetched", &id], None).status.success());
    let fetched = text(&record(&jobs, &id), "fetched").to_string();
    assert!(!fetched.is_empty());
    std::thread::sleep(Duration::from_millis(1100));
    assert!(job_cmd(&jobs, &["fetched", &id], None).status.success());
    assert_eq!(text(&record(&jobs, &id), "fetched"), fetched, "the first fetch wins");
    let out = job_cmd(&jobs, &["list"], None);
    let listed = json::parse(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(listed.as_array().map(Vec::len), Some(1));
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}

/// URL-safe base64 with padding, as the sender's `base64.urlsafe_b64encode`.
fn base64_url(bytes: &[u8]) -> String {
    const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            out.push(if i <= chunk.len() { DIGITS[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[test]
fn the_senders_operations_reach_the_same_jobs() {
    let (jobs, commit) = jobs_dir("wire");
    let stop = std::sync::atomic::AtomicBool::new(false);
    let store = Store::new(&jobs);
    let request = request(&commit, &[STANDIN, "print", "over the wire"], &[]);
    let id = jobs::id_of(&request).to_string();
    let encoded = base64_url(request.to_json().as_bytes());
    std::thread::scope(|scope| {
        // The supervisor's runner thread (job_cli::spawn_runner_thread, with this binary's `exec`).
        let runner = scope.spawn(|| exec_runner(&jobs).run(&stop));
        // job-submit queues the job, follows its output and exits with its code.
        let code = ragon_supervisor::job_cli::serve(&store, "job-submit", std::slice::from_ref(&encoded));
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        runner.join().unwrap();
        assert_eq!(code, Some(0));
    });
    assert_eq!(text(&record(&jobs, &id), "status"), "done");
    let serve = |op: &str, args: &[&str]| {
        ragon_supervisor::job_cli::serve(&store, op, &args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    };
    assert_eq!(serve("job-submit", &[&encoded]), Some(2), "the same id twice");
    assert_eq!(serve("job-list", &[]), Some(0));
    assert_eq!(serve("job-logs", &[&id, "3"]), Some(0));
    assert_eq!(serve("job-cancel", &[&id]), Some(1), "already finished");
    assert_eq!(serve("job-out", &[&id]), Some(1));
    assert_eq!(serve("job-fetched", &[&id]), Some(0));
    assert!(!text(&record(&jobs, &id), "fetched").is_empty());
    assert_eq!(serve("job-logs", &[&id, "x"]), None);
    let _ = std::fs::remove_dir_all(jobs.parent().unwrap());
}
