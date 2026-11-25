//! `server job <submit|list|status|logs|fetched|cancel|exec|runner|prune> [--jobs-dir D] ...`:
//! the node's job commands (Python's `NodeJob` on the Linux node, `ragon-admin node job-exec`
//! there). Records and output are the Linux node's, so the sender reads both alike.
//!
//! - `submit` reads the request (JSON) from stdin and prints the record;
//! - `list` prints every record (a JSON array), `status <id>` one;
//! - `logs <id> [--offset N] [--follow]` prints the output, following while the job runs, and
//!   exits with the job's code;
//! - `fetched <id>` starts the short keep, `cancel <id>` stops a job;
//! - `exec [--sandbox none|unelevated] [--ragon P] [--permission-profile N] <id>` runs one job
//!   (the runner starts it in the job's Job Object);
//! - `runner [--once]` runs the queue in this process (the supervisor runs it as a thread with
//!   `server run --jobs-dir D`);
//! - `prune` removes the jobs whose keep is over.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::job_exec::{self, DEFAULT_PERMISSION_PROFILE, Sandbox};
use crate::job_runner::{self, ProcessSpawner, Runner};
use crate::jobs::{self, NodePolicy, Store};
use crate::json::{self, Value};

const USAGE: &str = "usage: server job <submit|list|status|logs|fetched|cancel|exec|runner|prune> \
[--jobs-dir D] [--offset N] [--follow] [--once] [--sandbox none|unelevated] [--ragon P] \
[--permission-profile N] [<id>]";

/// The node's jobs folder: `%ProgramData%\Ragon\jobs` on Windows (the service's, readable by the
/// account that submits), `$CODEX_HOME/jobs` or `~/.ragon/jobs` elsewhere (the Linux node's).
pub fn default_jobs_dir() -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return base.join("Ragon").join("jobs");
    }
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(home).join("jobs");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".ragon").join("jobs")
}

/// The default sandbox of `exec`: the unelevated `ragon sandbox` when this program is the
/// launcher on Windows; none otherwise (the drill binary; Unix nodes use the Python node).
pub fn default_sandbox() -> Sandbox {
    let exe = std::env::current_exe().unwrap_or_default();
    let is_launcher = exe.file_stem().is_some_and(|s| s.eq_ignore_ascii_case("ragon"));
    if cfg!(windows) && is_launcher {
        Sandbox::Unelevated { ragon: exe, profile: DEFAULT_PERMISSION_PROFILE.into() }
    } else {
        Sandbox::None
    }
}

#[derive(Debug, Default)]
struct JobOptions {
    jobs_dir: Option<PathBuf>,
    offset: u64,
    follow: bool,
    once: bool,
    sandbox: Option<String>,
    ragon: Option<PathBuf>,
    profile: Option<String>,
    id: Option<String>,
}

fn parse(args: &[String]) -> Result<JobOptions, String> {
    let mut opts = JobOptions::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--jobs-dir" => opts.jobs_dir = Some(PathBuf::from(value("--jobs-dir")?)),
            "--offset" => opts.offset = value("--offset")?.parse().map_err(|_| "--offset needs a number".to_string())?,
            "--follow" | "-f" => opts.follow = true,
            "--once" => opts.once = true,
            "--sandbox" => opts.sandbox = Some(value("--sandbox")?),
            "--ragon" => opts.ragon = Some(PathBuf::from(value("--ragon")?)),
            "--permission-profile" => opts.profile = Some(value("--permission-profile")?),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other if other.starts_with('-') => return Err(format!("unknown option {other:?}\n{USAGE}")),
            other if opts.id.is_none() => opts.id = Some(other.to_string()),
            other => return Err(format!("unexpected argument {other:?}\n{USAGE}")),
        }
    }
    Ok(opts)
}

fn sandbox_of(opts: &JobOptions) -> Result<Sandbox, String> {
    match opts.sandbox.as_deref() {
        None => Ok(default_sandbox()),
        Some("none") => Ok(Sandbox::None),
        Some("unelevated") => {
            let ragon = match &opts.ragon {
                Some(path) => path.clone(),
                None => std::env::current_exe().map_err(|e| format!("--sandbox unelevated needs --ragon: {e}"))?,
            };
            Ok(Sandbox::Unelevated { ragon, profile: opts.profile.clone().unwrap_or_else(|| DEFAULT_PERMISSION_PROFILE.into()) })
        }
        Some(other) => Err(format!("unknown sandbox {other:?} (none or unelevated)")),
    }
}

/// Runs `server job <args>` and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let Some((verb, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return 2;
    };
    let opts = match parse(rest) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    let store = Store::new(opts.jobs_dir.clone().unwrap_or_else(default_jobs_dir));
    let needs_id = matches!(verb.as_str(), "status" | "logs" | "fetched" | "cancel" | "exec");
    let id = opts.id.clone().unwrap_or_default();
    if needs_id && !jobs::is_job_id(&id) {
        eprintln!("server job {verb} needs a job id\n{USAGE}");
        return 2;
    }
    match verb.as_str() {
        "submit" => submit(&store),
        "list" => {
            let mut records = store.records();
            records.sort_by(|a, b| {
                let key = |r: &Value| r.get("submitted").and_then(Value::as_str).unwrap_or("").to_string();
                key(a).cmp(&key(b))
            });
            println!("{}", Value::Array(records).to_json_pretty().trim_end());
            0
        }
        "status" => match store.read(&id) {
            Some(record) => {
                print!("{}", record.to_json_pretty());
                0
            }
            None => {
                eprintln!("no job {id}");
                1
            }
        },
        "logs" => logs(&store, &id, opts.offset, opts.follow),
        "fetched" => done(store.mark_fetched(&id, &job_exec::now()), &id, "is not finished"),
        "cancel" => done(store.cancel(&id, &job_exec::now()), &id, "has already finished"),
        "exec" => match sandbox_of(&opts) {
            Ok(sandbox) => job_exec::execute(&store, &id, &sandbox),
            Err(message) => {
                eprintln!("{message}");
                2
            }
        },
        "runner" => runner(store, &opts),
        "prune" => {
            for id in job_exec::prune(&store, job_exec::unix_now()) {
                println!("{id}");
            }
            0
        }
        other => {
            eprintln!("unknown job command {other:?}\n{USAGE}");
            2
        }
    }
}

fn done(result: Result<bool, String>, id: &str, otherwise: &str) -> i32 {
    match result {
        Ok(true) => 0,
        Ok(false) => {
            eprintln!("job {id} {otherwise}");
            1
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn submit(store: &Store) -> i32 {
    let mut text = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut text) {
        eprintln!("cannot read the request: {e}");
        return 2;
    }
    let request = match json::parse(&text) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("the request is not JSON: {e}");
            return 2;
        }
    };
    match store.submit(&request, &NodePolicy::default(), &job_exec::now()) {
        Ok(record) => {
            print!("{}", record.to_json_pretty());
            0
        }
        Err(reason) => {
            eprintln!("{reason}");
            1
        }
    }
}

fn logs(store: &Store, id: &str, offset: u64, follow: bool) -> i32 {
    let mut position = offset;
    let mut stdout = std::io::stdout();
    loop {
        let record = store.read(id);
        let finished = record.as_ref().is_none_or(|r| jobs::is_final(jobs::status_of(r)));
        let before = position;
        match job_exec::copy_output(store, id, position, &mut stdout) {
            Ok(next) => position = next,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
        if !follow || (finished && position == before) {
            let Some(record) = record else {
                eprintln!("no job {id}");
                return 1;
            };
            if !follow {
                return 0;
            }
            if let Some(note) = record.get("note").and_then(Value::as_str) {
                let _ = stdout.flush();
                eprintln!("\n{note}");
            }
            return job_exec::result_code(&record);
        }
        if !finished {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
}

fn runner(store: Store, opts: &JobOptions) -> i32 {
    let sandbox = match sandbox_of(opts) {
        Ok(sandbox) => sandbox,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&store.root) {
        eprintln!("cannot create {}: {e}", store.root.display());
        return 1;
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut runner = Runner::new(store, ProcessSpawner, job_runner::exec_prefix(exe), sandbox);
    if opts.once {
        runner.start();
        loop {
            runner.tick();
            if runner.active_ids().is_empty() {
                return 0;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    runner.run(&AtomicBool::new(false));
    0
}

/// Starts the runner as a thread of the supervisor; it stops with `stop`.
pub fn spawn_runner_thread<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    jobs_dir: PathBuf,
    stop: &'scope AtomicBool,
) -> Option<std::thread::ScopedJoinHandle<'scope, ()>> {
    if let Err(e) = std::fs::create_dir_all(&jobs_dir) {
        eprintln!("server run: no job runner, cannot create {}: {e}", jobs_dir.display());
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let sandbox = default_sandbox();
    Some(scope.spawn(move || {
        let mut runner = Runner::new(Store::new(jobs_dir), ProcessSpawner, job_runner::exec_prefix(exe), sandbox);
        runner.run(stop);
    }))
}

/// What the node says to an operation it does not carry out (the Linux node's `serve-job` refusal,
/// for the job operations).
pub const REFUSAL: &str = "ragon server: this key may only ask for Ragon node operations \
(job-submit, job-list, job-logs, job-fetched, job-cancel).";

/// The keys `job-list` shows of each record, as the Linux node's `serve-job job-list`.
const LISTED: [&str; 10] =
    ["id", "status", "command", "branch", "submitted", "finished", "exit_code", "note", "fetched", "out_files"];

/// Carries out a job operation as the sender asks for it over SSH (`NodeJobSender`; the Linux
/// node's `ragon-admin node serve-job`), so the sender talks to a Windows node unchanged:
/// `job-submit <request, JSON in URL-safe base64>` (then follows the output), `job-list` (one JSON
/// line per job), `job-logs <id> [<offset>]`, `job-fetched <id>`, `job-cancel <id>` and
/// `job-out <id>` (refused: this node keeps no `--out` folders yet). Returns None for anything
/// else, an unknown id included; the caller prints [`REFUSAL`] and exits 2.
pub fn serve(store: &Store, operation: &str, args: &[String]) -> Option<i32> {
    match (operation, args) {
        ("job-submit", [encoded]) => return Some(serve_submit(store, encoded)),
        ("job-list", []) => return Some(serve_list(store)),
        _ => {}
    }
    let (id, rest) = args.split_first()?;
    if !jobs::is_job_id(id) || store.read(id).is_none() {
        return None;
    }
    match (operation, rest) {
        ("job-fetched", []) => Some(if store.mark_fetched(id, &job_exec::now()) == Ok(true) { 0 } else { 1 }),
        ("job-cancel", []) => {
            let cancelled = store.cancel(id, &job_exec::now()) == Ok(true);
            println!("{}", if cancelled { "cancelled" } else { "the job is not running" });
            Some(if cancelled { 0 } else { 1 })
        }
        ("job-out", []) => {
            eprintln!("this node does not keep --out folders yet");
            Some(1)
        }
        ("job-logs", []) => Some(logs(store, id, 0, true)),
        ("job-logs", [offset]) if !offset.is_empty() && offset.bytes().all(|b| b.is_ascii_digit()) => {
            Some(logs(store, id, offset.parse().ok()?, true))
        }
        _ => None,
    }
}

fn serve_submit(store: &Store, encoded: &str) -> i32 {
    let request = base64_url_decode(encoded)
        .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "the request is not UTF-8".to_string()))
        .and_then(|text| json::parse(&text).map_err(|e| format!("the request is not JSON: {e}")))
        .and_then(|request| store.submit(&request, &NodePolicy::default(), &job_exec::now()));
    let record = match request {
        Ok(record) => record,
        Err(reason) => {
            eprintln!("❌ The node refused the job: {reason}");
            return 2;
        }
    };
    let id = jobs::id_of(&record).to_string();
    let field = |key: &str| record.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "the node".into());
    let network = if field("airgapped") == "on" { "off" } else { "on" };
    println!(
        "🚀 Job {id} queued on {host} (memory {}, time {}, network {network}, lane {}).",
        field("memory"),
        field("time"),
        jobs::lane_of(&record)
    );
    logs(store, &id, 0, true)
}

fn serve_list(store: &Store) -> i32 {
    job_exec::prune(store, job_exec::unix_now());
    let mut records = store.records();
    records.sort_by(|a, b| {
        let key = |r: &Value| r.get("submitted").and_then(Value::as_str).unwrap_or("").to_string();
        key(a).cmp(&key(b))
    });
    for record in records {
        let mut listed: Vec<(String, Value)> =
            LISTED.iter().map(|key| (key.to_string(), record.get(key).cloned().unwrap_or(Value::Null))).collect();
        let after = prune_after(&record).map(Value::str).unwrap_or(Value::Null);
        listed.push(("prune_after".into(), after));
        println!("{}", Value::Object(listed).to_json());
    }
    0
}

/// When a finished job may be pruned (`fetched` + a day, `finished` + 14 days); None while it runs.
pub fn prune_after(record: &Value) -> Option<String> {
    if !jobs::is_final(jobs::status_of(record)) {
        return None;
    }
    let at = |key: &str| record.get(key).and_then(Value::as_str).and_then(jobs::parse_utc_timestamp);
    let when = match (at("fetched"), at("finished").or_else(|| at("submitted"))) {
        (Some(fetched), _) => fetched + jobs::KEEP_AFTER_FETCH_S,
        (None, Some(finished)) => finished + jobs::KEEP_UNFETCHED_S,
        (None, None) => return None,
    };
    Some(crate::core::utc_timestamp(when))
}

/// Decodes URL-safe base64, padded or not (Python's `urlsafe_b64encode`).
fn base64_url_decode(text: &str) -> Result<Vec<u8>, String> {
    let digits = text.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for &c in digits {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err("the request is not URL-safe base64".into()),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if digits.len() % 4 == 1 {
        return Err("the request is not URL-safe base64".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_parse() {
        let opts = parse(&args(&["--jobs-dir", "/j", "--follow", "--offset", "7", "abc"])).unwrap();
        assert_eq!(opts.jobs_dir, Some(PathBuf::from("/j")));
        assert!(opts.follow);
        assert_eq!(opts.offset, 7);
        assert_eq!(opts.id.as_deref(), Some("abc"));
        assert!(parse(&args(&["a", "b"])).is_err());
        assert!(parse(&args(&["--bogus"])).is_err());
        let opts = parse(&args(&["--sandbox", "unelevated", "--ragon", "r.exe", "x"])).unwrap();
        assert_eq!(
            sandbox_of(&opts).unwrap(),
            Sandbox::Unelevated { ragon: PathBuf::from("r.exe"), profile: DEFAULT_PERMISSION_PROFILE.into() }
        );
        assert!(sandbox_of(&parse(&args(&["--sandbox", "elevated"])).unwrap()).is_err());
    }

    #[test]
    fn verbs_need_an_id() {
        assert_eq!(run(&args(&["status", "--jobs-dir", "/nonexistent-ragon-jobs"])), 2);
        assert_eq!(run(&args(&["bogus"])), 2);
        assert_eq!(run(&[]), 2);
    }

    #[test]
    fn base64_decodes_as_python_encodes() {
        // base64.urlsafe_b64encode(b'{"a": "\xff~?"}') and the same without padding.
        assert_eq!(base64_url_decode("eyJhIjogIsO_fj8ifQ==").unwrap(), "{\"a\": \"\u{ff}~?\"}".as_bytes());
        assert_eq!(base64_url_decode("eyJhIjogIsO_fj8ifQ").unwrap(), "{\"a\": \"\u{ff}~?\"}".as_bytes());
        assert_eq!(base64_url_decode("").unwrap(), b"");
        assert!(base64_url_decode("ab+c").is_err());
        assert!(base64_url_decode("abcde").is_err());
    }

    #[test]
    fn operations_outside_the_job_ones_are_refused() {
        let store = Store::new(std::env::temp_dir().join(format!("ragon-serve-{}", std::process::id())));
        let a = |items: &[&str]| args(items);
        assert_eq!(serve(&store, "job-cancel", &a(&["20261125-1000-abc"])), None, "unknown id");
        assert_eq!(serve(&store, "job-cancel", &a(&["../x"])), None);
        assert_eq!(serve(&store, "job-list", &a(&["x"])), None);
        assert_eq!(serve(&store, "set-model", &a(&["k"])), None);
        assert_eq!(serve(&store, "job-submit", &a(&["%%%"])), Some(2));
        assert_eq!(serve(&store, "job-submit", &a(&["e30"])), Some(2), "{{}} is no job");
    }

    #[test]
    fn prune_after_follows_the_keep() {
        let mut r = json::parse(r#"{"status": "running", "finished": "2026-11-25T10:00:00Z"}"#).unwrap();
        assert_eq!(prune_after(&r), None);
        r.set("status", Value::str("done"));
        assert_eq!(prune_after(&r).as_deref(), Some("2026-12-09T10:00:00Z"));
        r.set("fetched", Value::str("2026-11-25T12:00:00Z"));
        assert_eq!(prune_after(&r).as_deref(), Some("2026-11-26T12:00:00Z"));
    }
}
