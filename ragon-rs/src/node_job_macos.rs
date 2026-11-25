//! `ragon node serve-job` on a Mac node: the forced command's job operations
//! (specs/DREAMFERENCE_MAC_NODE.md §4.5). Linux runs the same protocol in Python
//! (`dreamference/node/node_serve.py`, `node_job.py`); the Mac has no Python for the node half (MN1),
//! so this module speaks it and hands every job to the node supervisor over its local socket
//! (`ragon-supervisor jobs serve`, supervisor/src/jobs.rs). The supervisor runs the job in its own
//! process group under a Seatbelt profile, with the footprint cap and the time limit, so a dropped
//! SSH session leaves the job running.
//!
//! Served: `job-submit <base64 JSON>` (and its output), `job-logs <id> [offset]`, `job-list`,
//! `job-fetched <id>`, `job-cancel <id>`. Anything else is refused with exit 2, as on Linux.
//! Not served yet: `job-out` and the git services a job's push and fetch use.
//!
//! The module compiles on every Unix so its tests run on Linux too; the launcher declares it on
//! macOS only.

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use base64::Engine;
use ragon_supervisor::jobs::{self, JobSpec, Record};
use serde_json::Value;

/// The refusal Linux prints (node_serve.py `REFUSAL`), word for word.
pub const REFUSAL: &str = "ragon-admin node serve-job: this key may only ask for Ragon node operations \
     (info, status, start, stop, set-model <key>, model-receive <key>, adapter-receive, \
     unpair, jobs, night tasks and the forge's).";

/// The node's jobs folder (the supervisor's root and the senders' bare repositories).
pub const DEFAULT_JOBS_DIR: &str = "/Library/Application Support/Ragon/jobs";
/// The supervisor's socket inside the jobs folder.
pub const SOCKET_NAME: &str = "supervisor.sock";
/// For tests: the forced command's environment comes from sshd, which does not take a user's.
pub const JOBS_DIR_ENV: &str = "RAGON_JOBS_DIR";
pub const SOCKET_ENV: &str = "RAGON_JOB_SOCKET";

const FOLLOW_EVERY: Duration = Duration::from_millis(500);

/// One request of the forced command, checked as node_serve.py checks it.
#[derive(Debug, PartialEq, Eq)]
pub enum Operation {
    Submit(String),
    List,
    Fetched(String),
    Cancel(String),
    Logs(String, u64),
    /// Part of the Linux protocol, not served by a Mac node yet.
    NotYet(String),
    Refused,
}

/// `node_job.JOB_ID`: `\d{8}-\d{4}-[0-9a-f]{3}`.
pub fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 17
        && b[8] == b'-'
        && b[13] == b'-'
        && b[..8].iter().chain(&b[9..13]).all(u8::is_ascii_digit)
        && b[14..].iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// `node_job.REPO_SLUG` without `..`: `[A-Za-z0-9][A-Za-z0-9._-]{0,80}`.
pub fn valid_slug(slug: &str) -> bool {
    let b = slug.as_bytes();
    !b.is_empty()
        && b.len() <= 81
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
        && !slug.contains("..")
}

/// `node_serve.COUNT`: 1 to 19 ASCII digits.
fn count(text: &str) -> Option<u64> {
    (!text.is_empty() && text.len() <= 19 && text.bytes().all(|c| c.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Reads `SSH_ORIGINAL_COMMAND`.
pub fn parse_operation(original: &str) -> Operation {
    let words: Vec<&str> = original.split_whitespace().collect();
    let Some((op, args)) = words.split_first() else { return Operation::Refused };
    match (*op, args) {
        ("job-submit", [encoded]) => Operation::Submit((*encoded).to_string()),
        ("job-list", []) => Operation::List,
        ("job-fetched", [id]) if valid_id(id) => Operation::Fetched((*id).to_string()),
        ("job-cancel", [id]) if valid_id(id) => Operation::Cancel((*id).to_string()),
        ("job-logs", [id]) if valid_id(id) => Operation::Logs((*id).to_string(), 0),
        ("job-logs", [id, offset]) if valid_id(id) => match count(offset) {
            Some(o) => Operation::Logs((*id).to_string(), o),
            None => Operation::Refused,
        },
        ("job-out", _) | ("git-receive-pack", _) | ("git-upload-pack", _) => Operation::NotYet((*op).to_string()),
        _ => Operation::Refused,
    }
}

/// A job request after the checks of `NodeJob.validate`, and the texts the started line shows.
#[derive(Debug, PartialEq, Eq)]
pub struct Accepted {
    pub spec: JobSpec,
    pub memory: String,
    pub time: String,
}

fn text_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

/// `job-submit`'s argument: JSON in URL-safe base64 (padded or not).
pub fn decode_request(encoded: &str) -> Result<Value, String> {
    let bytes = base64::engine::general_purpose::URL_SAFE
        .decode(encoded)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded))
        .map_err(|_| "the request is not URL-safe base64".to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| format!("the request is not JSON: {e}"))
}

/// The checks of `NodeJob.validate` for what a Mac node runs. `node_airgapped_on` is the node's
/// own level; the job gets the stricter of it and the request's. The memory cap comes from the
/// request; the time limit defaults to [`jobs::DEFAULT_TIME_S`] where Linux requires one.
pub fn validate(request: &Value, jobs_dir: &Path, node_airgapped_on: bool) -> Result<Accepted, String> {
    let Value::Object(map) = request else { return Err("a job is a JSON object".into()) };
    let id = text_of(map.get("id")).filter(|i| valid_id(i)).ok_or("a job has an id such as 20261125-1200-abc")?;
    let slug = text_of(map.get("repo")).unwrap_or_default();
    if !valid_slug(&slug) {
        return Err(format!("not a repository name: {slug:?}"));
    }
    let commit = text_of(map.get("commit")).unwrap_or_default();
    if commit.len() != 40 || !commit.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err("a job names the commit it runs at, as a full hash".into());
    }
    let command: Vec<String> = match map.get("command") {
        Some(Value::Array(words)) if !words.is_empty() => words
            .iter()
            .map(|w| w.as_str().filter(|s| !s.is_empty()).map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or("a job has a command")?,
        _ => return Err("a job has a command".into()),
    };
    let memory = text_of(map.get("memory")).ok_or("a job needs a memory cap")?;
    let memory_bytes = jobs::parse_size(&memory).filter(|m| *m > 0).ok_or("a job needs a memory cap")?;
    let time = text_of(map.get("time")).unwrap_or_else(|| format!("{}s", jobs::DEFAULT_TIME_S));
    let time_s = jobs::parse_duration(&time)
        .filter(|s| *s > 0)
        .ok_or("a job's time limit is a duration such as 90m")?;
    if memory_bytes > jobs::MAX_MEMORY_BYTES {
        return Err(format!("this node allows a job at most {}G of memory", jobs::MAX_MEMORY_BYTES >> 30));
    }
    if time_s > jobs::MAX_TIME_S {
        return Err(format!("this node allows a job at most {}h", jobs::MAX_TIME_S / 3600));
    }
    if truthy(map.get("gpu")) {
        return Err("GPU jobs are not allowed on a node: a footprint cap does not bound GPU memory, \
                    which is the pool the model server uses"
            .into());
    }
    for key in ["test", "setup", "out", "binds"] {
        if truthy(map.get(key)) {
            return Err(format!("this Mac node does not run a job's `{key}` yet"));
        }
    }
    // The stricter level: a request's unknown level counts as `on`, as the supervisor reads it.
    let requested_off = matches!(text_of(map.get("airgapped")).as_deref(), None | Some("off"));
    let network = requested_off && !node_airgapped_on;
    let spec = JobSpec {
        id,
        command,
        memory_bytes,
        time_s,
        network,
        repo: Some(jobs_dir.join("repos").join(format!("{slug}.git"))),
        commit: Some(commit),
    };
    Ok(Accepted { spec, memory, time })
}

/// One line of `job-list`, with Linux's keys; `branch`, `out_files` and `prune_after` stay null
/// until the Mac node commits results and prunes.
pub fn list_line(record: &Record) -> String {
    let text = |k: &str| record.get(k).map_or(Value::Null, |v| Value::String(v.clone()));
    let command: Vec<Value> = record
        .get("command")
        .map(|c| c.split(' ').filter(|w| !w.is_empty()).map(|w| Value::String(jobs::decode(w))).collect())
        .unwrap_or_default();
    let exit_code = record.get("exit_code").and_then(|c| c.parse::<i64>().ok()).map_or(Value::Null, Value::from);
    serde_json::json!({
        "id": text("id"),
        "status": text("status"),
        "command": command,
        "branch": Value::Null,
        "submitted": text("submitted"),
        "finished": text("finished"),
        "exit_code": exit_code,
        "note": text("note"),
        "fetched": text("fetched"),
        "out_files": Value::Null,
        "prune_after": Value::Null,
    })
    .to_string()
}

/// `follow`'s exit code: 0 when done, else the job's exit code, else 1.
pub fn final_code(record: &Record) -> i32 {
    if record.get("status").map(String::as_str) == Some(jobs::STATUS_DONE) {
        return 0;
    }
    record.get("exit_code").and_then(|c| c.parse::<i32>().ok()).filter(|c| *c != 0).unwrap_or(1)
}

fn ask(socket: &Path, words: Vec<String>) -> Result<Record, String> {
    let answer = jobs::request(socket, &words).map_err(|e| format!("the node's supervisor is not answering: {e}"))?;
    jobs::parse_answer(&answer)
}

/// Prints the job's output from `offset` until it ends; the note goes to stderr.
pub fn follow(socket: &Path, id: &str, mut offset: u64, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    loop {
        let record = match ask(socket, vec!["status".into(), id.into()]) {
            Ok(r) => r,
            Err(e) => {
                let _ = writeln!(err, "❌ {e}");
                return 1;
            }
        };
        let finished = record.contains_key("finished");
        let answer = match jobs::request(socket, &["logs".into(), id.into(), offset.to_string()]) {
            Ok(a) => a,
            Err(e) => {
                let _ = writeln!(err, "❌ the node's supervisor is not answering: {e}");
                return 1;
            }
        };
        let body = answer.splitn(2, |b| *b == b'\n').nth(1).unwrap_or_default();
        let chunk = match body.windows(2).position(|w| w == b"\n\n") {
            Some(at) if answer.starts_with(b"ok\n") => &body[at + 2..],
            _ => &[][..],
        };
        if !chunk.is_empty() {
            let _ = out.write_all(chunk);
            let _ = out.flush();
            offset += chunk.len() as u64;
            continue;
        }
        if finished {
            if let Some(note) = record.get("note") {
                let _ = writeln!(err, "{note}");
            }
            return final_code(&record);
        }
        std::thread::sleep(FOLLOW_EVERY);
    }
}

fn hostname() -> String {
    unsafe extern "C" {
        fn gethostname(name: *mut std::ffi::c_char, len: usize) -> i32;
    }
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its length; the result is NUL-terminated or truncated.
    if unsafe { gethostname(buf.as_mut_ptr().cast(), buf.len()) } != 0 {
        return "this node".into();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).into_owned()
}

/// Where this node keeps its jobs and the supervisor's socket.
pub struct Paths {
    pub jobs_dir: PathBuf,
    pub socket: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        let jobs_dir = std::env::var_os(JOBS_DIR_ENV).map_or_else(|| PathBuf::from(DEFAULT_JOBS_DIR), PathBuf::from);
        let socket = std::env::var_os(SOCKET_ENV).map_or_else(|| jobs_dir.join(SOCKET_NAME), PathBuf::from);
        Self { jobs_dir, socket }
    }
}

/// Carries out one forced-command request; the exit code as on Linux (2 for a refused request).
pub fn serve(original: &str, paths: &Paths, node_airgapped_on: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match parse_operation(original) {
        Operation::Submit(encoded) => {
            let accepted = decode_request(&encoded).and_then(|r| validate(&r, &paths.jobs_dir, node_airgapped_on));
            let accepted = match accepted.and_then(|a| {
                let mut words = vec!["submit".to_string()];
                words.extend(a.spec.to_words());
                ask(&paths.socket, words).map(|_| a)
            }) {
                Ok(a) => a,
                Err(e) => {
                    let _ = writeln!(err, "❌ The node refused the job: {e}");
                    return 2;
                }
            };
            let _ = writeln!(
                out,
                "🚀 Job {} started on {} (memory {}, time {}, network {}).",
                accepted.spec.id,
                hostname(),
                accepted.memory,
                accepted.time,
                if accepted.spec.network { "on" } else { "off" }
            );
            let _ = out.flush();
            follow(&paths.socket, &accepted.spec.id, 0, out, err)
        }
        Operation::List => {
            let listed = match ask(&paths.socket, vec!["list".into()]) {
                Ok(ids) => ids,
                Err(e) => {
                    let _ = writeln!(err, "❌ {e}");
                    return 1;
                }
            };
            for id in listed.keys() {
                if let Ok(record) = ask(&paths.socket, vec!["status".into(), id.clone()]) {
                    let _ = writeln!(out, "{}", list_line(&record));
                }
            }
            0
        }
        Operation::Fetched(id) => match ask(&paths.socket, vec!["fetched".into(), id]) {
            Ok(_) => 0,
            Err(e) if e == "no such job" => refuse(err),
            Err(_) => 1,
        },
        Operation::Cancel(id) => match ask(&paths.socket, vec!["cancel".into(), id]) {
            Ok(_) => {
                let _ = writeln!(out, "cancelled");
                0
            }
            Err(e) if e == "no such job" => refuse(err),
            Err(_) => {
                let _ = writeln!(out, "the job is not running");
                1
            }
        },
        Operation::Logs(id, offset) => match ask(&paths.socket, vec!["status".into(), id.clone()]) {
            Ok(_) => follow(&paths.socket, &id, offset, out, err),
            Err(_) => refuse(err),
        },
        Operation::NotYet(op) => {
            let _ = writeln!(err, "{REFUSAL}");
            let _ = writeln!(err, "This Mac node does not serve `{op}` yet.");
            2
        }
        Operation::Refused => refuse(err),
    }
}

fn refuse(err: &mut dyn Write) -> i32 {
    let _ = writeln!(err, "{REFUSAL}");
    2
}

/// `ragon node serve-job`, the forced command of a pairing key.
pub fn run_serve_job() -> i32 {
    let original = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
    // The node's level; the resolver counts a file it cannot read as `on` (AIRGAPPED §6.5), as
    // node_job.py's `node_airgap_level` does.
    let node_on = crate::airgapped::resolve(None).level == crate::airgapped::Level::On;
    serve(&original, &Paths::from_env(), node_on, &mut std::io::stdout(), &mut std::io::stderr())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn request(extra: serde_json::Value) -> Value {
        let mut r = serde_json::json!({
            "id": "20261125-1200-abc", "repo": "proj", "commit": COMMIT,
            "command": ["make", "test"], "memory": "8G",
        });
        if let (Value::Object(r), Value::Object(e)) = (&mut r, extra) {
            r.extend(e);
        }
        r
    }

    #[test]
    fn operations_are_checked_as_on_linux() {
        assert_eq!(parse_operation("job-list"), Operation::List);
        assert_eq!(parse_operation("job-list x"), Operation::Refused);
        assert_eq!(parse_operation("job-cancel 20261125-1200-abc"), Operation::Cancel("20261125-1200-abc".into()));
        assert_eq!(parse_operation("job-cancel 20261125-1200-ABC"), Operation::Refused);
        assert_eq!(parse_operation("job-cancel ../x"), Operation::Refused);
        assert_eq!(parse_operation("job-logs 20261125-1200-abc 42"), Operation::Logs("20261125-1200-abc".into(), 42));
        assert_eq!(parse_operation("job-logs 20261125-1200-abc -1"), Operation::Refused);
        assert_eq!(parse_operation("job-logs 20261125-1200-abc ²"), Operation::Refused);
        assert_eq!(parse_operation("job-fetched 20261125-1200-abc"), Operation::Fetched("20261125-1200-abc".into()));
        assert_eq!(parse_operation("job-submit e30="), Operation::Submit("e30=".into()));
        assert_eq!(parse_operation("job-submit a b"), Operation::Refused);
        assert_eq!(parse_operation("job-out 20261125-1200-abc"), Operation::NotYet("job-out".into()));
        assert_eq!(parse_operation("git-upload-pack 'jobs/x.git'"), Operation::NotYet("git-upload-pack".into()));
        assert_eq!(parse_operation("bash -c id"), Operation::Refused);
        assert_eq!(parse_operation(""), Operation::Refused);
    }

    #[test]
    fn a_request_becomes_the_supervisors_spec() {
        let a = validate(&request(serde_json::json!({"time": "90m"})), Path::new("/j"), false).unwrap();
        assert_eq!(a.spec.memory_bytes, 8 << 30);
        assert_eq!(a.spec.time_s, 5400);
        assert_eq!(a.spec.repo, Some(PathBuf::from("/j/repos/proj.git")));
        assert_eq!(a.spec.commit.as_deref(), Some(COMMIT));
        assert!(a.spec.network);
        // The supervisor reads back what the glue sends.
        assert_eq!(JobSpec::from_words(&a.spec.to_words()).unwrap(), a.spec);
    }

    #[test]
    fn the_time_limit_defaults_to_an_hour_and_the_cap_is_required() {
        let a = validate(&request(serde_json::json!({})), Path::new("/j"), false).unwrap();
        assert_eq!(a.spec.time_s, 3600);
        let mut r = request(serde_json::json!({}));
        r.as_object_mut().unwrap().remove("memory");
        assert_eq!(validate(&r, Path::new("/j"), false).unwrap_err(), "a job needs a memory cap");
    }

    #[test]
    fn the_stricter_airgap_level_wins() {
        let at = |requested: Value, node_on: bool| {
            validate(&request(serde_json::json!({ "airgapped": requested })), Path::new("/j"), node_on).unwrap().spec.network
        };
        assert!(at(Value::from("off"), false));
        assert!(!at(Value::from("on"), false));
        assert!(!at(Value::from("off"), true));
        assert!(!at(Value::from("sideways"), false));
        assert!(at(Value::Null, false));
    }

    #[test]
    fn what_a_mac_node_does_not_run_is_refused() {
        let bad = |extra: Value, why: &str| {
            let e = validate(&request(extra), Path::new("/j"), false).unwrap_err();
            assert!(e.contains(why), "{e}");
        };
        bad(serde_json::json!({"gpu": 1}), "GPU");
        bad(serde_json::json!({"repo": "../etc"}), "repository name");
        bad(serde_json::json!({"repo": "a..b"}), "repository name");
        bad(serde_json::json!({"commit": "abc"}), "full hash");
        bad(serde_json::json!({"command": []}), "command");
        bad(serde_json::json!({"command": ["ok", ""]}), "command");
        bad(serde_json::json!({"command": "make"}), "command");
        bad(serde_json::json!({"memory": "64G"}), "at most 32G");
        bad(serde_json::json!({"time": "9h"}), "at most 8h");
        bad(serde_json::json!({"test": "pytest"}), "`test`");
        bad(serde_json::json!({"id": "x"}), "an id");
        assert!(validate(&Value::from("x"), Path::new("/j"), false).is_err());
    }

    #[test]
    fn the_request_is_url_safe_base64_json() {
        let json = br#"{"a":"??>"}"#;
        let padded = base64::engine::general_purpose::URL_SAFE.encode(json);
        let bare = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        assert_eq!(decode_request(&padded).unwrap()["a"], "??>");
        assert_eq!(decode_request(&bare).unwrap()["a"], "??>");
        assert!(decode_request("!!").is_err());
    }

    #[test]
    fn list_lines_carry_linuxs_keys() {
        let mut r = Record::new();
        r.insert("id".into(), "20261125-1200-abc".into());
        r.insert("status".into(), "failed".into());
        r.insert("command".into(), format!("{} -c", jobs::encode("a b")));
        r.insert("exit_code".into(), "3".into());
        let v: Value = serde_json::from_str(&list_line(&r)).unwrap();
        assert_eq!(v["command"], serde_json::json!(["a b", "-c"]));
        assert_eq!(v["exit_code"], 3);
        assert_eq!(v["finished"], Value::Null);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 11);
        assert_eq!(final_code(&r), 3);
        r.insert("status".into(), "done".into());
        assert_eq!(final_code(&r), 0);
        r.insert("status".into(), "cancelled".into());
        r.remove("exit_code");
        assert_eq!(final_code(&r), 1);
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-c", "user.name=job", "-c", "user.email=job@example.invalid", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn run(original: &str, paths: &Paths) -> (i32, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = serve(original, paths, false, &mut out, &mut err);
        (code, String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned())
    }

    fn submit_line(id: &str, commit: &str, command: &[&str]) -> String {
        let json = serde_json::json!({"id": id, "repo": "proj", "commit": commit, "command": command, "memory": "256M", "time": "60"});
        format!("job-submit {}", base64::engine::general_purpose::URL_SAFE.encode(json.to_string()))
    }

    /// The forced command against a real supervisor: a job runs in a worktree of the sender's
    /// repository, its output and exit code come back, and list, logs, fetched and cancel answer
    /// as on Linux. On macOS the job runs under its Seatbelt profile.
    #[test]
    fn the_forced_command_drives_the_supervisor() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        // Outside every home: the Seatbelt profile hides them.
        let dir = PathBuf::from("/tmp").canonicalize().unwrap().join(format!("rnj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q"]);
        std::fs::write(work.join("README"), "from the repository\n").unwrap();
        git(&work, &["add", "README"]);
        git(&work, &["commit", "-qm", "one"]);
        let commit = git(&work, &["rev-parse", "HEAD"]);
        let jobs_dir = dir.join("jobs");
        std::fs::create_dir_all(jobs_dir.join("repos")).unwrap();
        git(&dir, &["clone", "-q", "--bare", "work", "jobs/repos/proj.git"]);
        let paths = Paths { socket: jobs_dir.join(SOCKET_NAME), jobs_dir: jobs_dir.clone() };
        let mut server = jobs::JobServer::new(jobs_dir.clone(), Arc::new(ragon_supervisor::footprint::SystemFootprint));
        server.limits.sample = Duration::from_millis(100);
        let server = Arc::new(server);
        let stop = Arc::new(AtomicBool::new(false));
        let serving = {
            let (server, stop, socket) = (server.clone(), stop.clone(), paths.socket.clone());
            std::thread::spawn(move || server.serve(&socket, &stop))
        };
        while !paths.socket.exists() {
            std::thread::sleep(Duration::from_millis(20));
        }

        let a = "20261125-1200-a01";
        let (code, out, err) = run(&submit_line(a, &commit, &["/bin/sh", "-c", "echo hello; cat README; exit 3"]), &paths);
        assert_eq!(code, 3, "{out}{err}");
        assert!(out.starts_with(&format!("🚀 Job {a} started on ")), "{out}");
        assert!(out.contains("(memory 256M, time 60, network on)."), "{out}");
        assert!(out.contains("hello\nfrom the repository\n"), "{out}");
        let (code, out, _) = run(&format!("job-logs {a} 6"), &paths);
        assert_eq!((code, out.as_str()), (3, "from the repository\n"));
        let (code, out, _) = run("job-list", &paths);
        assert_eq!(code, 0);
        let line: Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!((line["id"].as_str(), line["status"].as_str(), line["exit_code"].as_i64()), (Some(a), Some("failed"), Some(3)));
        assert_eq!(line["command"][0], "/bin/sh");
        assert_eq!(run(&format!("job-fetched {a}"), &paths).0, 0);
        assert_eq!(run(&format!("job-cancel {a}"), &paths), (1, "the job is not running\n".into(), String::new()));
        assert_eq!(run("job-logs 20261125-1200-fff", &paths).0, 2);
        // A job already given is refused, not run twice.
        let (code, _, err) = run(&submit_line(a, &commit, &["/bin/true"]), &paths);
        assert_eq!(code, 2);
        assert!(err.contains("The node refused the job: job 20261125-1200-a01 exists already"), "{err}");

        // A long job is cancelled from a second connection while the first follows it.
        let b = "20261125-1200-a02";
        let following = {
            let (line, paths) = (submit_line(b, &commit, &["/bin/sh", "-c", "echo started; sleep 60"]), Paths { jobs_dir: paths.jobs_dir.clone(), socket: paths.socket.clone() });
            std::thread::spawn(move || run(&line, &paths))
        };
        while !std::fs::read_to_string(jobs_dir.join(b).join("output.log")).unwrap_or_default().contains("started") {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(run(&format!("job-cancel {b}"), &paths), (0, "cancelled\n".into(), String::new()));
        let (code, out, _) = following.join().unwrap();
        assert_eq!(code, 1, "{out}");
        assert!(out.contains("started"));
        assert_eq!(run(&format!("job-fetched {b}"), &paths).0, 0);

        stop.store(true, Ordering::SeqCst);
        serving.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_operations_get_linuxs_refusal() {
        let paths = Paths { jobs_dir: "/nonexistent".into(), socket: "/nonexistent/s".into() };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(serve("rm -rf /", &paths, true, &mut out, &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains(REFUSAL));
        let mut err = Vec::new();
        assert_eq!(serve("job-out 20261125-1200-abc", &paths, true, &mut out, &mut err), 2);
        assert!(String::from_utf8_lossy(&err).contains("does not serve `job-out` yet"));
    }
}
