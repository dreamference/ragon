//! `ragon night run`: Night Shift's runner in Rust, for the Windows node (specs/DREAMFERENCE_WINDOWS_NODE.md,
//! decision WN1; specs/DREAMFERENCE_RAGON_WINDOWS_ARM.md §13).
//!
//! On Linux the runner is `ragon-admin night run` (Python), beside host safety and the model
//! server's health checks. This is the same run for a node without the Python half: it reads and
//! writes the same files under `$CODEX_HOME/night/` (night.rs), holds the same `runner.lock`, and
//! records a task's attempts the way `NightShiftBestOfN` does, so `ragon night show`, `apply`,
//! `apply --attempt N` and `report` read what it left exactly as they read the Python runner's.
//!
//! A task runs `best_of` attempts, one after another, each in its own worktree on its own branch
//! (`night/<id>-a<k>`); each attempt is one `ragon exec` in a Job Object that caps its memory and
//! takes its whole process tree down with it. Chooser v1 (`night_shift_chooser.py`) picks one:
//! the regression filter, then the pairwise judge on the model server. The chosen attempt is
//! committed and its branch renamed `night/<id>`; the others are discarded, their diffs kept under
//! `logs/`. Nothing is merged or pushed.
//!
//! Left to the Python runner: tasks for another node (`on`), resumed sessions and kept worktrees,
//! refine, the commit inside the sandbox with the repository's hooks (here the commit runs with no
//! hook, through `ragon_safegit`), and the window and the scheduling of parallel tasks.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use rand::Rng as _;
use rand::SeedableRng as _;
use rand::rngs::StdRng;
use serde_json::Value;
use serde_json::json;
use sha2::Digest as _;
use sha2::Sha256;

use crate::night;

/// Statuses a run picks up: new tasks, and tasks a previous window cut off.
const RUNNABLE: &[&str] = &["queued", "interrupted"];
/// Statuses of a task a `/night drop` cancelled or asked to cancel.
const CANCELLING: &[&str] = &["cancel-requested", "cancelled"];
/// What the lock file says while a run holds it (`NightShiftQueue.runner_lock`).
const HOLDER: &str = "a Night Shift run";
const HELD: &str = "Another night run holds the runner lock.";

const NUDGE: &str = "Go ahead and make the change now.";
/// The lines of a test run's output kept in the record.
const TEST_TAIL_LINES: usize = 200;
/// `[night]` defaults (night_shift_settings.py).
const DEFAULT_BEST_OF: u32 = 3;
const DEFAULT_NUDGES: u32 = 2;
const DEFAULT_TASK_TIMEOUT: Duration = Duration::from_secs(90 * 60);
const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const DEFAULT_TASK_MEMORY: u64 = 8 << 30;
/// How often a running child is looked at for its time limit.
const POLL: Duration = Duration::from_millis(100);

/// Chooser v1's constants (night_shift_chooser.py).
const MIN_COMPARISONS: u32 = 2;
const SWISS_WINDOW: usize = 8;
const SWISS_ROUNDS: u32 = 1;
const WEIGHT_FLOOR: f64 = 0.1;
const MAX_PATCH_CHARS: usize = 40_000;
const MAX_ISSUE_CHARS: usize = 20_000;
const JUDGE_MAX_TOKENS: u32 = 16_384;
const JUDGE_TEMPERATURE: f64 = 0.6;
const JUDGE_TOP_P: f64 = 0.95;
const JUDGE_TIMEOUT: Duration = Duration::from_secs(900);
const SYSTEM_PROMPT: &str = "You are an expert code reviewer. Compare two solutions to a programming problem and rate their correctness. The problem is a GitHub issue in a Python repository; each solution is a patch (a diff) written to fix it. Neither patch has been run. A correct patch fixes the behaviour the issue describes and breaks no existing behaviour. If both look correct, prefer the more robust and fault-tolerant one. Ignore style, comments and the tests a patch adds.";
const NO_JUDGE: &str = "judge skipped: no model server";

const PREAMBLE: &str = "This is a Night Shift task. It runs unattended overnight, in a git worktree of {repo} on branch {branch}, and\nnobody will answer questions: where something is unclear, make the reasonable choice and say which you made.\n\n- Work only inside this worktree.\n- Make the change itself; do not stop at describing or planning it.\n- Do not commit, push, merge or switch branches. The night run commits your changes.\n{test_line}- End by listing the files you changed.\n\nTask:\n{task}";

const USAGE: &str = "Usage: ragon night run [--all] [--best-of N]";

/// One judge call: (issue, patch A, patch B) -> (rating of A, rating of B), or None when the
/// answer could not be read.
pub type Judge = dyn Fn(&str, &str, &str) -> Option<(f64, f64)>;

/// What a run is given. Tests build it by hand; `run_cli` from the config and the environment.
pub struct Options {
    /// The `ragon` that runs each attempt (`ragon exec`) and its tests (`ragon sandbox`).
    pub ragon: PathBuf,
    /// Only this repository's tasks; None runs every repository's.
    pub repo: Option<PathBuf>,
    pub best_of: u32,
    pub nudges: u32,
    pub task_timeout: Duration,
    pub test_timeout: Duration,
    /// The Job Object's memory cap per attempt, in bytes (Windows only).
    pub task_memory: u64,
    /// The judge; None when no model server answers, and the tie-break decides.
    pub judge: Option<Box<Judge>>,
}

impl Options {
    pub fn new(ragon: PathBuf) -> Options {
        Options {
            ragon,
            repo: None,
            best_of: DEFAULT_BEST_OF,
            nudges: DEFAULT_NUDGES,
            task_timeout: DEFAULT_TASK_TIMEOUT,
            test_timeout: DEFAULT_TEST_TIMEOUT,
            task_memory: DEFAULT_TASK_MEMORY,
            judge: None,
        }
    }
}

/// `ragon night run [--all] [--best-of N]`. Returns the exit code.
pub fn run_cli(dir: &Path, args: &[String]) -> i32 {
    if !cfg!(windows) {
        println!(
            "On Linux and macOS the night runner is `ragon-admin night run`, beside host safety; \
             `ragon night run` is the Windows node's runner."
        );
        return 2;
    }
    let Ok(ragon) = std::env::current_exe() else {
        println!("Night Shift: could not find this program's path");
        return 1;
    };
    let mut options = Options::new(ragon);
    apply_config(&mut options);
    let mut all = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--all" => all = true,
            "--best-of" => match rest.next().and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=9).contains(n)) {
                Some(n) => options.best_of = n,
                None => {
                    println!("--best-of needs a number from 1 to 9");
                    return 2;
                }
            },
            _ => {
                println!("{USAGE}");
                return 2;
            }
        }
    }
    if !all {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match night::repo_root(&cwd) {
            Some(repo) => options.repo = Some(repo),
            None => {
                println!("Not in a git repository; `ragon night run --all` runs every repository's tasks.");
                return 2;
            }
        }
    }
    options.judge = server_judge(&crate::vllm_host(), &dir.join("logs"));
    let outcome = run(dir, &options);
    for line in &outcome.lines {
        println!("{line}");
    }
    outcome.code
}

/// `[night] best_of`, `nudges`, `task_timeout`, `test_timeout` and `task_memory` from the config file.
fn apply_config(options: &mut Options) {
    let Some(text) = crate::config_file().and_then(|path| std::fs::read_to_string(path).ok()) else {
        return;
    };
    let Ok(doc) = text.parse::<toml::Table>() else {
        return;
    };
    let Some(table) = doc.get("night").and_then(toml::Value::as_table) else {
        return;
    };
    let int = |key: &str| table.get(key).and_then(toml::Value::as_integer).and_then(|n| u32::try_from(n).ok());
    if let Some(n) = int("best_of").filter(|n| *n >= 1) {
        options.best_of = n;
    }
    if let Some(n) = int("nudges") {
        options.nudges = n;
    }
    let text = |key: &str| table.get(key).and_then(toml::Value::as_str).map(str::to_string);
    if let Some(timeout) = text("task_timeout").as_deref().and_then(duration) {
        options.task_timeout = timeout;
    }
    if let Some(timeout) = text("test_timeout").as_deref().and_then(duration) {
        options.test_timeout = timeout;
    }
    if let Some(bytes) = text("task_memory").as_deref().and_then(bytes) {
        options.task_memory = bytes;
    }
}

/// `90m`, `2h`, `45s` or plain seconds.
fn duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let n: u64 = number.parse().ok()?;
    let seconds = match unit.trim() {
        "" | "s" => n,
        "m" => n * 60,
        "h" => n * 3600,
        _ => return None,
    };
    Some(Duration::from_secs(seconds))
}

/// `8G`, `512M` or plain bytes.
fn bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len()));
    let n: u64 = number.parse().ok()?;
    let shift = match unit.trim().to_ascii_uppercase().as_str() {
        "" => 0,
        "K" => 10,
        "M" => 20,
        "G" => 30,
        _ => return None,
    };
    Some(n << shift)
}

/// What a run printed, and its exit code.
pub struct Outcome {
    pub lines: Vec<String>,
    pub code: i32,
    pub report: Option<PathBuf>,
}

/// Runs every runnable task (of `options.repo`, or of every repository) under `runner.lock`, and
/// writes the morning report.
pub fn run(dir: &Path, options: &Options) -> Outcome {
    let lock = match runner_lock(dir) {
        Ok(Some(lock)) => lock,
        Ok(None) => return Outcome { lines: vec![HELD.to_string()], code: 1, report: None },
        Err(error) => return Outcome { lines: vec![format!("Night Shift: {error}")], code: 1, report: None },
    };
    let started = chrono::Local::now();
    let tasks = runnable(dir, options.repo.as_deref());
    if tasks.is_empty() {
        release(lock);
        return Outcome { lines: vec!["Nothing queued.".to_string()], code: 0, report: None };
    }
    let mut lines = Vec::new();
    let mut ended = Vec::new();
    for task in tasks {
        let id = night::text(&task, "id");
        let record = run_task(dir, &task, options);
        lines.push(format!("{id}: {}", night::text(&record, "status")));
        ended.push(record);
    }
    let report = write_report(dir, started, &ended);
    match &report {
        Ok(path) => lines.push(format!("Report: {}", path.display())),
        Err(error) => lines.push(format!("The report could not be written: {error}")),
    }
    release(lock);
    Outcome { lines, code: 0, report: report.ok() }
}

/// Lets go of `runner.lock`: unlocked first, so a child still holding a copy of the descriptor
/// (between its fork and its exec) cannot keep it.
fn release(lock: File) {
    let _ = lock.unlock();
}

/// Takes `runner.lock` without waiting and names the holder in it; None when another run holds it.
fn runner_lock(dir: &Path) -> Result<Option<File>, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("could not create {}: {error}", dir.display()))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("runner.lock"))
        .map_err(|error| format!("could not open the runner lock: {error}"))?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(None),
        Err(std::fs::TryLockError::Error(error)) => return Err(format!("could not take the runner lock: {error}")),
    }
    let _ = file.set_len(0);
    let _ = file.write_all(HOLDER.as_bytes());
    Ok(Some(file))
}

/// The tasks this run can work, oldest first: runnable, here (no `on`), with no session to resume
/// and no kept worktree, which the Python runner resumes.
fn runnable(dir: &Path, repo: Option<&Path>) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(dir.join("tasks")) else {
        return Vec::new();
    };
    let repo = repo.map(|repo| repo.to_string_lossy().into_owned());
    let mut tasks: Vec<Value> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| night::read_task(&path))
        .filter(|task| RUNNABLE.contains(&task["status"].as_str().unwrap_or_default()))
        .filter(|task| repo.as_deref().is_none_or(|repo| task["repo"].as_str() == Some(repo)))
        .filter(|task| ["on", "session", "remote"].iter().all(|key| !truthy(&task[*key])))
        .filter(|task| !dir.join("worktrees").join(night::text(task, "id")).exists())
        .collect();
    tasks.sort_by_key(|task| night::text(task, "id"));
    tasks
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => true,
    }
}

/// Moves a task to `status` unless it was cancelled meanwhile, then it becomes `cancelled`.
fn transition(dir: &Path, id: &str, status: &str, change: impl FnOnce(&mut Value)) -> Value {
    let updated = night::update_task(dir, id, |task| {
        let cancelling = CANCELLING.contains(&task["status"].as_str().unwrap_or_default());
        if cancelling && status != "cancelled" {
            night::set_status(task, "cancelled", None);
        } else {
            change(task);
            night::set_status(task, status, None);
        }
        Ok(task.clone())
    });
    updated.unwrap_or_else(|_| json!({"id": id, "status": status}))
}

fn cancel_requested(dir: &Path, id: &str) -> bool {
    night::read_task(&dir.join("tasks").join(format!("{id}.json")))
        .is_none_or(|task| CANCELLING.contains(&task["status"].as_str().unwrap_or_default()))
}

/// One task: its attempts, the chooser, the commit. Returns the record as it ended.
fn run_task(dir: &Path, task: &Value, options: &Options) -> Value {
    let id = night::text(task, "id");
    let started = night::update_task(dir, &id, |task| {
        if CANCELLING.contains(&task["status"].as_str().unwrap_or_default()) {
            night::set_status(task, "cancelled", None);
            return Ok(false);
        }
        task["attempts"] = json!(task["attempts"].as_u64().unwrap_or(0) + 1);
        night::set_status(task, "running", None);
        Ok(true)
    });
    if started != Ok(true) {
        return night::read_task(&dir.join("tasks").join(format!("{id}.json"))).unwrap_or_else(|| task.clone());
    }
    let n = task["best_of"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= 1)
        .unwrap_or(options.best_of);
    let _ = std::fs::create_dir_all(dir.join("logs"));
    let _ = std::fs::create_dir_all(dir.join("worktrees"));
    let mut attempts = Vec::new();
    for k in 1..=n {
        if cancel_requested(dir, &id) {
            break;
        }
        attempts.push(run_attempt(dir, task, k, options));
    }
    if cancel_requested(dir, &id) {
        for attempt in &attempts {
            discard(task, attempt);
        }
        return transition(dir, &id, "cancelled", |_| {});
    }
    let candidates: Vec<&Attempt> = attempts.iter().filter(|a| a.status == "candidate").collect();
    if candidates.is_empty() {
        return no_candidate(dir, task, n, &attempts);
    }
    let choice = choose(task, &candidates, options);
    let chosen = candidates
        .iter()
        .find(|a| a.k == choice.chosen)
        .copied()
        .unwrap_or(candidates[0]);
    for attempt in attempts.iter().filter(|a| a.k != chosen.k) {
        discard(task, attempt);
    }
    match commit(task, chosen) {
        Ok(branch) => {
            let repo = PathBuf::from(night::text(task, "repo"));
            let range = format!("{}..{branch}", night::text(task, "base"));
            let diff_stat = git(&repo, &["diff", "--stat", &range]).unwrap_or_default();
            let best = best_of_record(dir, task, n, &attempts, Some(&choice));
            let mut result = attempt_result(chosen);
            result["branch"] = json!(branch);
            result["diff_stat"] = json!(diff_stat);
            result["attempts"] = json!(attempts.len());
            result["nudges"] = json!(attempts.iter().map(|a| a.nudges).sum::<u32>());
            result["best_of"] = best;
            if let Some(object) = result.as_object_mut() {
                object.remove("worktree");
            }
            transition(dir, &id, "done", |task| task["result"] = result)
        }
        Err(error) => {
            let best = best_of_record(dir, task, n, &attempts, Some(&choice));
            let result = json!({"last_message": format!("the chosen attempt could not be committed: {error}"), "best_of": best});
            transition(dir, &id, "failed", |task| task["result"] = result)
        }
    }
}

/// One attempt's end.
#[derive(Debug, Clone)]
struct Attempt {
    k: u32,
    /// `candidate`, `no-change`, `stalled`, `timed-out` or `failed`.
    status: String,
    worktree: PathBuf,
    branch: String,
    wall_s: u64,
    nudges: u32,
    timed_out: bool,
    last_message: String,
    log: PathBuf,
    diff: Option<PathBuf>,
    diff_stat: String,
    test_command: String,
    test_result: Option<String>,
    test_tail: String,
    test_log: Option<PathBuf>,
    hint: Option<String>,
}

impl Attempt {
    /// The regression filter's verdict: Some(true) when its tests failed or ran out of time (or the
    /// attempt itself did), Some(false) when they passed, None when there were none.
    fn breaks(&self) -> Option<bool> {
        if self.timed_out {
            return Some(true);
        }
        match self.test_result.as_deref() {
            Some(result) if result.starts_with("failed") || result.starts_with("timed out") => Some(true),
            Some("passed") => Some(false),
            _ => None,
        }
    }
}

fn run_attempt(dir: &Path, task: &Value, k: u32, options: &Options) -> Attempt {
    let id = night::text(task, "id");
    let repo = PathBuf::from(night::text(task, "repo"));
    let base = night::text(task, "base");
    let tag = format!("{id}-a{k}");
    let worktree = dir.join("worktrees").join(&tag);
    let branch = format!("night/{tag}");
    let logs = dir.join("logs");
    let started = Instant::now();
    let mut attempt = Attempt {
        k,
        status: "failed".to_string(),
        worktree: worktree.clone(),
        branch: branch.clone(),
        wall_s: 0,
        nudges: 0,
        timed_out: false,
        last_message: String::new(),
        log: logs.join(format!("{tag}.jsonl")),
        diff: None,
        diff_stat: String::new(),
        test_command: night::text(task, "test"),
        test_result: None,
        test_tail: String::new(),
        test_log: None,
        hint: None,
    };
    remove_worktree(&repo, &worktree, &branch);
    let worktree_text = worktree.to_string_lossy().into_owned();
    if let Err(error) = git_checked(&repo, &["worktree", "add", "-q", &worktree_text, "-b", &branch, &base]) {
        attempt.hint = Some(format!("the worktree could not be made: {error}"));
        return attempt;
    }
    let last = logs.join(format!("{tag}.last.txt"));
    let test_line = if attempt.test_command.is_empty() {
        String::new()
    } else {
        format!("- When the change is made, run `{}` and fix what it reports.\n", attempt.test_command)
    };
    let prompt = PREAMBLE
        .replace("{repo}", &repo.to_string_lossy())
        .replace("{branch}", &format!("night/{id}"))
        .replace("{test_line}", &test_line)
        .replace("{task}", &night::text(task, "task"));
    let deadline = started + options.task_timeout;
    let mut end = exec(options, &worktree, &last, &attempt.log, &prompt, None, deadline);
    let mut session = thread_id(&attempt.log);
    while end == End::Exited(0)
        && attempt.nudges < options.nudges
        && !has_changes(&worktree)
        && announces_work(&read(&last))
        && let Some(thread) = session.clone()
    {
        attempt.nudges += 1;
        end = exec(options, &worktree, &last, &attempt.log, NUDGE, Some(&thread), deadline);
        session = thread_id(&attempt.log).or(session);
    }
    attempt.last_message = read(&last);
    attempt.timed_out = end == End::TimedOut;
    let changed = has_changes(&worktree);
    if !changed {
        attempt.status = match end {
            End::TimedOut => "timed-out",
            End::Exited(0) if announces_work(&attempt.last_message) => "stalled",
            End::Exited(0) => "no-change",
            _ => "failed",
        }
        .to_string();
        if attempt.status == "failed" {
            attempt.hint = Some(match end {
                End::Spawn(error) => format!("ragon exec could not start: {error}"),
                _ => "ragon exec exited with an error; see the log".to_string(),
            });
        }
        attempt.wall_s = started.elapsed().as_secs();
        remove_worktree(&repo, &worktree, &branch);
        return attempt;
    }
    let _ = git(&worktree, &["add", "-A"]);
    if !attempt.test_command.is_empty() {
        let test_log = logs.join(format!("{tag}.test.log"));
        let (result, tail) = run_tests(options, &worktree, &attempt.test_command, &test_log, session.as_deref());
        attempt.test_result = Some(result);
        attempt.test_tail = tail;
        attempt.test_log = Some(test_log);
        // A test run may leave files behind; what is staged is the agent's change.
    }
    let diff_path = logs.join(format!("{tag}.diff"));
    let diff = git_bytes(&worktree, &["diff", "--cached", "--binary"]).unwrap_or_default();
    let _ = std::fs::write(&diff_path, diff);
    attempt.diff = Some(diff_path);
    attempt.diff_stat = git(&worktree, &["diff", "--cached", "--stat"]).unwrap_or_default();
    attempt.status = "candidate".to_string();
    attempt.wall_s = started.elapsed().as_secs();
    attempt
}

/// How a child ended.
#[derive(Debug, Clone, PartialEq, Eq)]
enum End {
    Exited(i32),
    TimedOut,
    Spawn(String),
}

/// One `ragon exec` (or its resume, for a nudge) in the worktree, its events appended to `log`.
fn exec(options: &Options, worktree: &Path, last: &Path, log: &Path, prompt: &str, resume: Option<&str>, deadline: Instant) -> End {
    let mut command = Command::new(&options.ragon);
    command
        .arg("exec")
        .arg("--json")
        .arg("-o")
        .arg(last)
        .arg("-C")
        .arg(worktree)
        .args(["-s", "workspace-write", "--skip-git-repo-check", "-c", "sandbox_workspace_write.writable_roots=[]"]);
    match resume {
        Some(thread) => command.args(["resume", thread, prompt]),
        None => command.arg(prompt),
    };
    command.current_dir(worktree);
    let Ok(out) = OpenOptions::new().create(true).append(true).open(log) else {
        return End::Spawn(format!("could not open {}", log.display()));
    };
    let err = log.with_extension("stderr.txt");
    let Ok(err) = OpenOptions::new().create(true).append(true).open(err) else {
        return End::Spawn("could not open the stderr log".to_string());
    };
    command.stdin(Stdio::null()).stdout(out).stderr(err);
    capped_run(command, options.task_memory, deadline)
}

/// Runs `command` in a Job Object (memory cap, the tree killed with it) until it exits or the
/// deadline passes.
fn capped_run(mut command: Command, memory: u64, deadline: Instant) -> End {
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => return End::Spawn(error.to_string()),
    };
    let job = job::Job::cap(&child, memory);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return End::Exited(status.code().unwrap_or(-1)),
            Ok(None) if Instant::now() >= deadline => {
                drop(job);
                let _ = child.kill();
                let _ = child.wait();
                return End::TimedOut;
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(error) => return End::Spawn(error.to_string()),
        }
    }
}

/// The task's tests in the worktree, in `ragon sandbox` (the agent wrote the code they run).
/// Returns the result word and the output's last lines.
fn run_tests(options: &Options, worktree: &Path, test: &str, log: &Path, session: Option<&str>) -> (String, String) {
    let mut command = Command::new(&options.ragon);
    command.args([
        "sandbox",
        "-c",
        "sandbox_mode=\"workspace-write\"",
        "-c",
        "sandbox_workspace_write.writable_roots=[]",
        "-c",
        "sandbox_workspace_write.network_access=true",
        "--",
    ]);
    if cfg!(windows) {
        command.args(["cmd", "/C", test]);
    } else {
        command.args(["bash", "-c", test]);
    }
    if let Some(session) = session {
        // The sandbox helper resolves the air-gap level again by itself from this (patch 0019).
        command.env("CODEX_THREAD_ID", session);
    }
    command.current_dir(worktree).stdin(Stdio::null());
    let Ok(out) = File::create(log) else {
        return ("untested".to_string(), String::new());
    };
    let Ok(err) = out.try_clone() else {
        return ("untested".to_string(), String::new());
    };
    command.stdout(out).stderr(err);
    let end = capped_run(command, options.task_memory, Instant::now() + options.test_timeout);
    let output = read(log);
    let lines: Vec<&str> = output.lines().collect();
    let tail = lines[lines.len().saturating_sub(TEST_TAIL_LINES)..].join("\n");
    let result = match end {
        End::Exited(0) => "passed".to_string(),
        End::Exited(code) => format!("failed (exit {code})"),
        End::TimedOut => format!("timed out after {} min", options.test_timeout.as_secs() / 60),
        End::Spawn(_) => "untested".to_string(),
    };
    (result, tail)
}

/// The session id `ragon exec --json` announced, the last one in the log.
fn thread_id(log: &Path) -> Option<String> {
    read(log)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["type"] == "thread.started")
        .filter_map(|event| event["thread_id"].as_str().map(str::to_string))
        .last()
}

/// Whether the last message promises work it did not do (`ANNOUNCES_WORK` in the Python runner).
fn announces_work(message: &str) -> bool {
    const PHRASES: &[&str] = &[
        "i'll", "i will", "i'm going to", "i am going to", "let me", "next, i", "next i", "i can now", "i'm about to",
        "i shall", "now i'll", "i'll now",
    ];
    const VERBS: &[&str] = &["make", "add", "update", "implement", "write", "create", "fix"];
    let lower = message.to_lowercase().replace('\u{2019}', "'");
    let at_word = |phrase: &str| {
        lower.match_indices(phrase).any(|(at, _)| {
            let before = lower[..at].chars().next_back();
            let after = lower[at + phrase.len()..].chars().next();
            !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
        })
    };
    PHRASES.iter().any(|phrase| at_word(phrase)) || VERBS.iter().any(|verb| at_word(&format!("going to {verb}")))
}

fn has_changes(worktree: &Path) -> bool {
    git(worktree, &["status", "--porcelain", "--untracked-files=all"]).is_some_and(|out| !out.trim().is_empty())
}

/// What the chooser decided.
#[derive(Debug, Clone)]
struct Choice {
    stage: String,
    survivors: Vec<u32>,
    points: Vec<(u32, f64)>,
    /// The ids at the top, before the tie-break.
    top: Vec<u32>,
    chosen: u32,
    judge_calls: u32,
    judge_errors: u32,
}

/// Chooser v1: the regression filter, then the pairwise judge over the survivors; a tie goes to
/// an attempt that did not break, then to the earliest.
fn choose(task: &Value, candidates: &[&Attempt], options: &Options) -> Choice {
    let mut choice = Choice {
        stage: String::new(),
        survivors: Vec::new(),
        points: Vec::new(),
        top: Vec::new(),
        chosen: candidates[0].k,
        judge_calls: 0,
        judge_errors: 0,
    };
    if candidates.len() == 1 {
        choice.stage = "only candidate".to_string();
        choice.survivors = vec![candidates[0].k];
        choice.top = choice.survivors.clone();
        return choice;
    }
    let survivors: Vec<&Attempt> = candidates.iter().filter(|a| a.breaks() != Some(true)).copied().collect();
    choice.survivors = survivors.iter().map(|a| a.k).collect();
    if survivors.len() == 1 {
        choice.stage = "regression filter".to_string();
        choice.top = choice.survivors.clone();
        choice.chosen = survivors[0].k;
        return choice;
    }
    let pool: Vec<&Attempt> = if survivors.is_empty() { candidates.to_vec() } else { survivors };
    match &options.judge {
        None => {
            choice.stage = NO_JUDGE.to_string();
            choice.top = pool.iter().map(|a| a.k).collect();
        }
        Some(judge) => {
            choice.stage = if choice.survivors.is_empty() { "judge, all broke" } else { "judge" }.to_string();
            let issue = clip(&night::text(task, "task"), MAX_ISSUE_CHARS);
            let patches: Vec<String> = pool
                .iter()
                .map(|a| clip(&a.diff.as_deref().map(read).unwrap_or_default(), MAX_PATCH_CHARS))
                .collect();
            let mut rng = StdRng::seed_from_u64(seed(&night::text(task, "id")));
            let (mut calls, mut errors) = (0, 0);
            let points = tournament(pool.len(), &mut rng, |a, b| {
                calls += 1;
                let rated = judge(&issue, &patches[a], &patches[b]);
                errors += u32::from(rated.is_none());
                rated
            });
            choice.judge_calls = calls;
            choice.judge_errors = errors;
            choice.points = pool.iter().map(|a| a.k).zip(points.iter().copied()).collect();
            let best = points.iter().copied().fold(f64::MIN, f64::max);
            choice.top = choice.points.iter().filter(|(_, p)| *p == best).map(|(k, _)| *k).collect();
        }
    }
    choice.chosen = pool
        .iter()
        .filter(|a| choice.top.contains(&a.k))
        .min_by_key(|a| (a.breaks() == Some(true), a.k))
        .map_or(pool[0].k, |a| a.k);
    choice
}

/// The seed of a task's pairing: the first eight bytes of its id's SHA-256.
fn seed(id: &str) -> u64 {
    let digest = Sha256::digest(id.as_bytes());
    u64::from_be_bytes(digest[..8].try_into().unwrap_or([0; 8]))
}

/// V1's tournament over `n` candidates (indices): random disjoint pairs until each has two
/// comparisons, then one Swiss round (neighbours in points, within a window of 8); a pair is never
/// played twice. Per comparison 1 / 0.5 / 0 for a win, tie or loss, times max(|gap| / 9, 0.1); an
/// unreadable answer scores 0.5 each at the floor.
fn tournament(n: usize, rng: &mut StdRng, mut compare: impl FnMut(usize, usize) -> Option<(f64, f64)>) -> Vec<f64> {
    let mut scores = vec![0.0; n];
    let mut counts = vec![0u32; n];
    let mut played = std::collections::BTreeSet::new();
    let mut swiss = 0;
    while n > 1 {
        let before_swiss = counts.iter().any(|c| *c < MIN_COMPARISONS);
        if !before_swiss {
            if swiss >= SWISS_ROUNDS {
                break;
            }
            swiss += 1;
        }
        let pairs = pairings(&scores, &counts, &played, rng);
        if pairs.is_empty() {
            break;
        }
        for (a, b) in pairs {
            played.insert((a.min(b), a.max(b)));
            counts[a] += 1;
            counts[b] += 1;
            match compare(a, b) {
                None => {
                    scores[a] += 0.5 * WEIGHT_FLOOR;
                    scores[b] += 0.5 * WEIGHT_FLOOR;
                }
                Some((ra, rb)) => {
                    let weight = ((ra - rb).abs() / 9.0).max(WEIGHT_FLOOR);
                    let outcome = if ra > rb {
                        1.0
                    } else if ra < rb {
                        0.0
                    } else {
                        0.5
                    };
                    scores[a] += weight * outcome;
                    scores[b] += weight * (1.0 - outcome);
                }
            }
        }
    }
    scores
}

fn pairings(
    scores: &[f64],
    counts: &[u32],
    played: &std::collections::BTreeSet<(usize, usize)>,
    rng: &mut StdRng,
) -> Vec<(usize, usize)> {
    let unplayed = |a: usize, b: usize| !played.contains(&(a.min(b), a.max(b)));
    let n = scores.len();
    let mut pairs = Vec::new();
    let mut used = vec![false; n];
    if counts.iter().any(|c| *c < MIN_COMPARISONS) {
        let mut order: Vec<(u32, f64, usize)> = (0..n).map(|i| (counts[i], rng.random::<f64>(), i)).collect();
        order.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
        let order: Vec<usize> = order.into_iter().map(|(_, _, i)| i).collect();
        for &a in &order {
            if used[a] || counts[a] >= MIN_COMPARISONS {
                continue;
            }
            if let Some(&b) = order.iter().find(|&&b| b != a && !used[b] && unplayed(a, b)) {
                pairs.push((a, b));
                used[a] = true;
                used[b] = true;
            }
        }
        if !pairs.is_empty() {
            return pairs;
        }
    }
    let mut ranked: Vec<usize> = (0..n).collect();
    ranked.sort_by(|x, y| scores[*y].total_cmp(&scores[*x]));
    for (position, &a) in ranked.iter().enumerate() {
        if used[a] {
            continue;
        }
        let window = &ranked[position + 1..(position + 1 + SWISS_WINDOW).min(n)];
        if let Some(&b) = window.iter().find(|&&b| !used[b] && unplayed(a, b)) {
            pairs.push((a, b));
            used[a] = true;
            used[b] = true;
        }
    }
    pairs
}

fn clip(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}\n[... cut: {} more characters]", count - limit)
}

/// The ratings at the end of a judge's answer (`RATING_A: 7`), each clamped to 1..10.
fn ratings(answer: &str) -> Option<(f64, f64)> {
    let mut a = None;
    let mut b = None;
    for line in answer.lines() {
        for (side, slot) in [("RATING_A:", &mut a), ("RATING_B:", &mut b)] {
            if let Some(at) = line.find(side) {
                let rest = line[at + side.len()..].trim_start().trim_start_matches('*').trim_start();
                let number: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
                if let Ok(value) = number.parse::<f64>() {
                    *slot = Some(value.clamp(1.0, 10.0));
                }
            }
        }
    }
    Some((a?, b?))
}

/// The judge on the model server at `host`, its answers appended to `logs/<id>.judge.jsonl`'s
/// sibling `logs/judge.jsonl`; None when the server names no model.
fn server_judge(host: &str, logs: &Path) -> Option<Box<Judge>> {
    let host = host.trim_end_matches('/').to_string();
    let model = on_runtime({
        let host = host.clone();
        async move {
            let client = reqwest::Client::builder().timeout(Duration::from_secs(10)).build().ok()?;
            crate::served_model(&client, &host).await.map(|model| model.id)
        }
    })?;
    let log = logs.join("judge.jsonl");
    Some(Box::new(move |issue: &str, a: &str, b: &str| {
        let user = format!(
            "<issue>\n{issue}\n</issue>\n\n<solution_A>\n{a}\n</solution_A>\n\n<solution_B>\n{b}\n</solution_B>\n\nRate the correctness of each solution from 1 (wrong) to 10 (certainly correct). End your answer with exactly these two lines:\nRATING_A: <1-10>\nRATING_B: <1-10>"
        );
        let body = json!({
            "model": model,
            "messages": [{"role": "system", "content": SYSTEM_PROMPT}, {"role": "user", "content": user}],
            "max_tokens": JUDGE_MAX_TOKENS,
            "temperature": JUDGE_TEMPERATURE,
            "top_p": JUDGE_TOP_P,
            "seed": seed(issue) % (1 << 31),
        });
        let url = format!("{host}/v1/chat/completions");
        let answer = on_runtime(async move {
            let client = reqwest::Client::builder().timeout(JUDGE_TIMEOUT).build().ok()?;
            let reply: Value = client.post(url).json(&body).send().await.ok()?.json().await.ok()?;
            reply["choices"][0]["message"]["content"].as_str().map(str::to_string)
        });
        let rated = answer.as_deref().and_then(ratings);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log) {
            let entry = json!({"answer": answer, "ratings": rated.map(|(a, b)| [a, b])});
            let _ = writeln!(file, "{entry}");
        }
        rated
    }))
}

/// Runs a future to its end on a runtime of its own, in a thread of its own: `ragon` itself
/// already runs on one, which cannot be blocked on from inside.
fn on_runtime<T: Send + 'static>(future: impl std::future::Future<Output = Option<T>> + Send + 'static) -> Option<T> {
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?
            .block_on(future)
    })
    .join()
    .ok()
    .flatten()
}

/// Commits the chosen attempt in its worktree as `night: <first line>`, removes the worktree and
/// renames its branch `night/<id>`. Returns the branch.
fn commit(task: &Value, attempt: &Attempt) -> Result<String, String> {
    let repo = PathBuf::from(night::text(task, "repo"));
    let first: String = night::text(task, "task").trim().lines().next().unwrap_or("task").chars().take(72).collect();
    git_checked(&attempt.worktree, &["commit", "-q", "--no-verify", "-m", &format!("night: {first}")])?;
    let branch = format!("night/{}", night::text(task, "id"));
    let worktree = attempt.worktree.to_string_lossy().into_owned();
    let _ = git(&repo, &["worktree", "remove", "--force", &worktree]);
    let _ = std::fs::remove_dir_all(&attempt.worktree);
    let _ = git(&repo, &["worktree", "prune"]);
    git_checked(&repo, &["branch", "-M", &attempt.branch, &branch])?;
    Ok(branch)
}

fn discard(task: &Value, attempt: &Attempt) {
    remove_worktree(&PathBuf::from(night::text(task, "repo")), &attempt.worktree, &attempt.branch);
}

fn remove_worktree(repo: &Path, worktree: &Path, branch: &str) {
    let text = worktree.to_string_lossy().into_owned();
    let _ = git(repo, &["worktree", "remove", "--force", &text]);
    let _ = std::fs::remove_dir_all(worktree);
    let _ = git(repo, &["worktree", "prune"]);
    let _ = git(repo, &["branch", "-D", branch]);
}

/// A task none of whose attempts left a change: stalled, then no-change (a time-out counts as
/// one), then failed.
fn no_candidate(dir: &Path, task: &Value, n: u32, attempts: &[Attempt]) -> Value {
    let has = |status: &str| attempts.iter().any(|a| a.status == status);
    let status = if has("stalled") {
        "stalled"
    } else if has("no-change") || has("timed-out") {
        "no-change"
    } else {
        "failed"
    };
    let last = attempts.iter().rev().find(|a| !a.last_message.is_empty());
    let best = best_of_record(dir, task, n, attempts, None);
    let result = json!({
        "attempts": attempts.len(),
        "nudges": attempts.iter().map(|a| a.nudges).sum::<u32>(),
        "wall_s": attempts.iter().map(|a| a.wall_s).sum::<u64>(),
        "last_message": last.map(|a| a.last_message.clone()).unwrap_or_default(),
        "best_of": best,
    });
    transition(dir, &night::text(task, "id"), status, |task| task["result"] = result)
}

fn attempt_result(attempt: &Attempt) -> Value {
    let mut result = json!({
        "worktree": attempt.worktree.to_string_lossy(),
        "wall_s": attempt.wall_s,
        "nudges": attempt.nudges,
        "last_message": attempt.last_message,
    });
    if !attempt.test_command.is_empty() {
        result["test_command"] = json!(attempt.test_command);
        result["test_source"] = json!("task");
        result["test_result"] = json!(attempt.test_result.clone().unwrap_or_else(|| "untested".to_string()));
        result["test_tail"] = json!(attempt.test_tail);
    }
    if attempt.timed_out {
        result["timed_out"] = json!(true);
    }
    result
}

/// `result.best_of`, as `NightShiftBestOfN._record` writes it.
fn best_of_record(dir: &Path, task: &Value, n: u32, attempts: &[Attempt], choice: Option<&Choice>) -> Value {
    let entries: Vec<Value> = attempts
        .iter()
        .map(|a| {
            let mut entry = json!({"attempt": a.k, "status": a.status, "wall_s": a.wall_s, "log": a.log.to_string_lossy()});
            if let Some(result) = &a.test_result {
                entry["test_result"] = json!(result);
            }
            if let Some(diff) = &a.diff {
                entry["diff"] = json!(diff.to_string_lossy());
            }
            if let Some(log) = &a.test_log {
                entry["test_log"] = json!(log.to_string_lossy());
            }
            if a.timed_out {
                entry["timed_out"] = json!(true);
            }
            if let Some(hint) = &a.hint {
                entry["hint"] = json!(hint);
            }
            if let Some(changes) = a.diff_stat.lines().last() {
                entry["changes"] = json!(changes.trim());
            }
            entry
        })
        .collect();
    let mut record = json!({
        "n": n,
        "ran": attempts.len(),
        "chosen": choice.map(|c| c.chosen),
        "stage": choice.map_or("none".to_string(), |c| c.stage.clone()),
        "points": choice.map_or_else(|| json!({}), |c| {
            c.points.iter().map(|(k, p)| (format!("a{k}"), json!(p))).collect::<serde_json::Map<_, _>>().into()
        }),
        "survivors": choice.map_or_else(Vec::new, |c| c.survivors.iter().map(|k| format!("a{k}")).collect()),
        "judge_calls": choice.map_or(0, |c| c.judge_calls),
        "judge_errors": choice.map_or(0, |c| c.judge_errors),
        "test_command": night::text(task, "test"),
        "attempts": entries,
    });
    if choice.is_some_and(|c| c.judge_calls > 0) {
        record["judge_log"] = json!(dir.join("logs").join("judge.jsonl").to_string_lossy());
    }
    if (attempts.len() as u32) < n {
        record["why"] = json!("the task was cancelled");
    }
    if let Some(choice) = choice {
        record["reason"] = json!(reason(choice, attempts.len(), !night::text(task, "test").is_empty()));
    }
    record
}

/// One sentence on why the chosen attempt won (`NightShiftBestOfN.reason`).
fn reason(choice: &Choice, ran: usize, tested: bool) -> String {
    let c = choice.chosen;
    let list = |ks: &[u32]| ks.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
    let mut sentence = match choice.stage.as_str() {
        "only candidate" => format!("attempt {c} was the only one that left a change"),
        "regression filter" => format!(
            "attempt {c} was the only one the regression filter kept: the other attempts' tests failed or ran out of time"
        ),
        "judge, all broke" => {
            format!("every attempt's tests failed, so the judge compared all {ran} in pairs and gave attempt {c} the most points")
        }
        "judge" if !tested => format!(
            "no test command, so the judge alone decided: it compared all {ran} attempts in pairs and gave attempt {c} the most points"
        ),
        "judge" => format!(
            "the judge compared the attempts the regression filter kept ({}) in pairs and gave attempt {c} the most points",
            list(&choice.survivors)
        ),
        _ => format!("no model server answered to judge the attempts, so the tie-break chose attempt {c}"),
    };
    if choice.top.len() > 1 && choice.stage != NO_JUDGE {
        sentence += &format!("; attempts {} tied at the top, and the tie went to {c}", list(&choice.top));
    }
    sentence
}

/// Writes `reports/<date>.md` (numbered `-2`, `-3`… when one exists), in the Python runner's format.
fn write_report(dir: &Path, started: chrono::DateTime<chrono::Local>, tasks: &[Value]) -> Result<PathBuf, String> {
    let reports = dir.join("reports");
    std::fs::create_dir_all(&reports).map_err(|error| error.to_string())?;
    let date = started.format("%Y-%m-%d").to_string();
    let mut path = reports.join(format!("{date}.md"));
    let mut counter = 2;
    while path.exists() {
        path = reports.join(format!("{date}-{counter}.md"));
        counter += 1;
    }
    std::fs::write(&path, render(started, tasks)).map_err(|error| error.to_string())?;
    Ok(path)
}

const VERDICTS: &[(&str, &str)] = &[
    ("done", "finished, its changes are on a branch"),
    ("no-change", "ran and changed nothing"),
    ("stalled", "stopped before it finished"),
    ("failed", "failed"),
    ("interrupted", "cut off by the end of the window"),
    ("cancelled", "cancelled"),
];

const STAGE_WORDS: &[(&str, &str)] = &[
    ("only candidate", "only one attempt left a change, so there was nothing to compare"),
    ("regression filter", "the tests removed every other attempt"),
    ("judge", "the judge compared the attempts that passed the tests"),
    ("judge, all broke", "every attempt failed the tests, so the judge compared them all"),
];

fn render(started: chrono::DateTime<chrono::Local>, tasks: &[Value]) -> String {
    let mut lines = vec![
        format!("# Night Shift — {}", started.format("%Y-%m-%d")),
        String::new(),
        format!("Run started {}, finished {}.", started.format("%H:%M"), chrono::Local::now().format("%H:%M")),
        String::new(),
    ];
    let status = |task: &Value| night::text(task, "status");
    let mut counts = Vec::new();
    for (name, _) in VERDICTS {
        let n = tasks.iter().filter(|task| status(task) == *name).count();
        if n > 0 {
            counts.push(format!("{n} {name}"));
        }
    }
    lines.extend(["## Summary".to_string(), String::new(), counts.join(", "), String::new()]);
    let group = |task: &Value| {
        if status(task) == "done" && !task["result"]["branch"].is_null() {
            "ready to apply"
        } else if ["no-change", "cancelled"].contains(&status(task).as_str()) {
            "nothing to do"
        } else {
            "needs a look"
        }
    };
    for wanted in ["ready to apply", "needs a look", "nothing to do"] {
        for task in tasks.iter().filter(|task| group(task) == wanted) {
            let first: String = first_line(task).chars().take(100).collect();
            let verb = if wanted == "ready to apply" { "apply" } else { "show" };
            let id = night::text(task, "id");
            lines.push(format!("- {id} ({wanted}): {first} — {}; `ragon night {verb} {id}`", verdict(&status(task))));
        }
    }
    lines.push(String::new());
    let mut repos: Vec<String> = Vec::new();
    for task in tasks {
        let repo = night::text(task, "repo");
        if !repos.contains(&repo) {
            repos.push(repo);
        }
    }
    for repo in &repos {
        lines.extend([format!("## {repo}"), String::new()]);
        for task in tasks.iter().filter(|task| night::text(task, "repo") == *repo) {
            lines.extend(task_block(task));
            lines.push(String::new());
        }
    }
    lines.extend(["## Review".to_string(), String::new()]);
    let branches: Vec<&Value> = tasks.iter().filter(|task| task["result"]["branch"].is_string()).collect();
    for task in &branches {
        let base: String = night::text(task, "base").chars().take(12).collect();
        lines.push(format!(
            "- `git -C {} diff {base}..{}`",
            night::text(task, "repo"),
            task["result"]["branch"].as_str().unwrap_or_default()
        ));
    }
    if branches.is_empty() {
        lines.push("- No branch to review.".to_string());
    } else {
        lines.push(
            "- Nothing was merged into your checkout. `ragon night apply <id>` (or `/night apply <id>`), run in the repository, \
             brings a task's changes there as uncommitted changes; it refuses, changing nothing, if they conflict with yours."
                .to_string(),
        );
    }
    lines.push("- `git worktree list` shows the worktrees kept for interrupted tasks.".to_string());
    let text: String = lines.join("\n").chars().filter(|c| !c.is_control() || matches!(c, '\t' | '\n')).collect();
    text.trim_end().to_string() + "\n"
}

fn first_line(task: &Value) -> String {
    night::text(task, "task").trim().lines().next().unwrap_or_default().to_string()
}

fn verdict(status: &str) -> String {
    VERDICTS
        .iter()
        .find(|(name, _)| *name == status)
        .map_or_else(|| format!("status {status}"), |(_, words)| words.to_string())
}

fn minutes(seconds: Option<u64>) -> String {
    match seconds {
        None => "—".to_string(),
        Some(s) if s >= 60 => format!("{} min {} s", s / 60, s % 60),
        Some(s) => format!("{s} s"),
    }
}

fn task_block(task: &Value) -> Vec<String> {
    let result = &task["result"];
    let id = night::text(task, "id");
    let status = night::text(task, "status");
    let mut lines = vec![format!("### {id} — {status}"), String::new(), format!("- Task: {}", first_line(task))];
    if let Some(branch) = result["branch"].as_str() {
        lines.push(format!("- Branch: `{branch}`, not merged; `ragon night apply {id}` brings it into your checkout"));
    }
    if let Some(stat) = result["diff_stat"].as_str().filter(|s| !s.is_empty()) {
        lines.push(format!("- Changes: {}", stat.lines().last().unwrap_or_default().trim()));
    }
    if let Some(test) = result["test_command"].as_str() {
        lines.push(format!(
            "- Tests: `{test}` ({}): {}",
            result["test_source"].as_str().unwrap_or("?"),
            result["test_result"].as_str().unwrap_or("?")
        ));
    }
    lines.push(format!(
        "- Attempts: {}, nudges: {}, wall time: {}",
        result["attempts"].as_u64().or(task["attempts"].as_u64()).unwrap_or(0),
        result["nudges"].as_u64().unwrap_or(0),
        minutes(result["wall_s"].as_u64())
    ));
    if result["best_of"].is_object() {
        lines.extend(best_of_lines(&result["best_of"]));
    }
    if ["stalled", "failed", "interrupted"].contains(&status.as_str())
        && let Some(message) = result["last_message"].as_str().filter(|m| !m.is_empty())
    {
        let message: String = message.trim().replace('\n', " ").chars().take(400).collect();
        lines.push(format!("- Last message: {message}"));
    }
    lines
}

fn best_of_lines(best: &Value) -> Vec<String> {
    let n = best["n"].as_u64().map_or("?".to_string(), |n| n.to_string());
    let mut ran = format!("{} of {n} attempts ran", best["ran"].as_u64().unwrap_or(0));
    if let Some(why) = best["why"].as_str() {
        ran += &format!(" ({why})");
    }
    let chosen = best["chosen"].as_u64();
    let mut lines = vec![match chosen {
        None => format!("- Best of {n}: {ran}; none chosen"),
        Some(chosen) => {
            let points = best["points"].as_object().filter(|p| !p.is_empty()).map_or(String::new(), |points| {
                let parts: Vec<String> =
                    points.iter().map(|(key, value)| format!("{key} {:.2}", value.as_f64().unwrap_or(0.0))).collect();
                format!("; points {}", parts.join(", "))
            });
            format!("- Best of {n}: {ran}; chose attempt {chosen} by {}{points}", best["stage"].as_str().unwrap_or("?"))
        }
    }];
    if let Some(reason) = best["reason"].as_str() {
        let stage = best["stage"].as_str().unwrap_or_default();
        let tail = STAGE_WORDS
            .iter()
            .find(|(name, _)| *name == stage)
            .map_or(String::new(), |(_, words)| format!(" (deciding stage: {words})"));
        lines.push(format!("  - Why: {reason}{tail}"));
    }
    if let Some(calls) = best["judge_calls"].as_u64().filter(|c| *c > 0) {
        let errors = best["judge_errors"]
            .as_u64()
            .filter(|e| *e > 0)
            .map_or(String::new(), |e| format!(", {e} unreadable or failed"));
        let log = best["judge_log"].as_str().map_or(String::new(), |log| format!("; answers in `{log}`"));
        lines.push(format!("  - Judge: {calls} call(s){errors}{log}"));
    }
    for entry in best["attempts"].as_array().into_iter().flatten() {
        let mut parts = vec![entry["status"].as_str().unwrap_or("?").to_string()];
        if let Some(changes) = entry["changes"].as_str().filter(|c| !c.is_empty()) {
            parts.push(changes.to_string());
        }
        if let Some(result) = entry["test_result"].as_str() {
            parts.push(format!("tests {result}"));
        }
        parts.push(minutes(entry["wall_s"].as_u64()));
        if entry["attempt"].as_u64() == chosen {
            parts.push("chosen: the branch".to_string());
        } else if let Some(diff) = entry["diff"].as_str() {
            parts.push(format!("diff `{diff}`"));
        }
        for (key, label) in [("log", "log"), ("test_log", "test log")] {
            if let Some(path) = entry[key].as_str() {
                parts.push(format!("{label} `{path}`"));
            }
        }
        if let Some(hint) = entry["hint"].as_str() {
            parts.push(format!("hint: {hint}"));
        }
        lines.push(format!("  - Attempt {}: {}", entry["attempt"], parts.join(", ")));
    }
    lines
}

/// `git` through `ragon_safegit`: the worktree's config and attributes cannot make it run the
/// agent's programs, and no hook runs.
fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    git_checked(cwd, args).ok()
}

fn git_checked(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let mut all = vec!["-c", "core.hooksPath=", "-c", "commit.gpgSign=false"];
    all.extend_from_slice(args);
    let output = ragon_safegit::output(cwd, &all).map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn git_bytes(cwd: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let output = ragon_safegit::output(cwd, args).ok()?;
    output.status.success().then_some(output.stdout)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The Job Object around an attempt (Windows): a memory cap on the whole tree, and the tree killed
/// when the handle closes, so a timed-out attempt leaves no process behind. Elsewhere a no-op: the
/// Python runner caps a task with a systemd scope.
mod job {
    use std::process::Child;

    #[cfg_attr(not(windows), allow(dead_code))]
    pub struct Job {
        #[cfg(windows)]
        handle: windows_sys::Win32::Foundation::HANDLE,
    }

    impl Job {
        #[cfg(not(windows))]
        pub fn cap(_child: &Child, _memory: u64) -> Option<Job> {
            None
        }

        /// Puts `child` in a new Job Object limited to `memory` bytes. None when Windows refuses;
        /// the attempt then runs uncapped, as it would on a host without the call.
        #[cfg(windows)]
        pub fn cap(child: &Child, memory: u64) -> Option<Job> {
            use std::os::windows::io::AsRawHandle as _;
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
            use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
            use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_JOB_MEMORY;
            use windows_sys::Win32::System::JobObjects::JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            use windows_sys::Win32::System::JobObjects::JOBOBJECT_EXTENDED_LIMIT_INFORMATION;
            use windows_sys::Win32::System::JobObjects::JobObjectExtendedLimitInformation;
            use windows_sys::Win32::System::JobObjects::SetInformationJobObject;
            // SAFETY: plain Win32 calls on handles this function owns, with a zeroed and fully
            // sized information block.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle == 0 {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                info.JobMemoryLimit = usize::try_from(memory).unwrap_or(usize::MAX);
                let set = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
                );
                if set == 0 || AssignProcessToJobObject(handle, child.as_raw_handle() as isize) == 0 {
                    CloseHandle(handle);
                    return None;
                }
                Some(Job { handle })
            }
        }
    }

    #[cfg(windows)]
    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle is this Job's own, closed once.
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
        }
    }
}

#[cfg(test)]
#[path = "night_run_tests.rs"]
mod tests;
