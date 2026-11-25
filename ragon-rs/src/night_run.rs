//! `ragon night run`, `ragon night schedule` and `ragon night unschedule`: the Night Shift runner
//! in the launcher, for nodes without Python (the Windows node). It runs the queue the launcher's
//! `night add` writes (`$CODEX_HOME/night/tasks/<id>.json`), in the format the Python runner
//! (`dreamference/night_shift/`) writes, byte for byte once paths and times are set aside
//! (`vectors/night-run/`, checked by both sides).
//!
//! Each task runs as best of N (`[night] best_of`, 3 by default): every attempt in a worktree and
//! on a branch of its own, `ragon exec` and the tests under a memory cap and a time limit (a Job
//! Object on Windows, a systemd scope on Linux), then the chooser (chooser v1) picks one attempt,
//! whose commit becomes the task's branch `night/<id>`. Nothing is merged into the user's branches
//! and nothing is pushed: the git verbs a run can use are a closed list (`capped::Verb`).
//!
//! Left to the Python runner, where it exists: refine, lanes, remote nodes, recurring tasks, the
//! code index, compactions. Tasks run one after another.

mod best_of;
mod capped;
mod chooser;
mod report;
mod task_run;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{Seek, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{Local, NaiveTime, TimeZone};
use serde_json::{Map, Value, json};

pub use best_of::BestOfN;
pub use capped::{Git, Verb};
pub use chooser::Candidate;
pub use task_run::TaskRun;

/// Set in every child's environment, so `ragon exec` knows it runs unattended.
pub const NIGHT_RUN_ENV: &str = "RAGON_NIGHT_RUN";
/// Emptied in every child's environment: the night run picks its own adapter.
pub const ADAPTER_ENV: &str = "DREAMFERENCE_RAGON_ADAPTER";
/// The statuses a night run starts.
pub const RUNNABLE_STATUSES: [&str; 2] = ["queued", "interrupted"];
/// The statuses a cancel leaves.
pub const CANCELLING_STATUSES: [&str; 2] = ["cancel-requested", "cancelled"];
/// The scheduled task's name in Task Scheduler.
pub const SCHEDULED_TASK: &str = "Ragon Night Shift";
/// Where the runner lock lives on Windows, under `%ProgramData%`.
// TODO(win-node-state-layout): take this from the node state layout once that branch lands; the
// supervisor keeps its state in `%ProgramData%\Ragon\node`, and this path follows it.
pub const WINDOWS_LOCK_DIR: [&str; 2] = ["Ragon", "night"];

const DEFAULT_WINDOW: &str = "01:00-07:00";
const ADMISSION_TIMEOUT: Duration = Duration::from_secs(10);

/// The strategies `[night] best_of_hints = true` gives attempts 2 and later.
pub const DEFAULT_HINTS: [&str; 6] = [
    "Before changing any code, write a small test or script that reproduces the problem and fails; then make the change and check that it passes.",
    "Before you edit a function, find every caller and every other place that uses it, and make sure your change is right for all of them.",
    "Find where the wrong behaviour starts, not just where it shows, and fix it at its cause.",
    "When your fix works, look for the same mistake elsewhere in the file and in its sibling functions, and fix it there too where it is the same bug.",
    "Make the smallest change that fixes the problem; leave unrelated code, names and formatting as they are.",
    "Read the existing tests of the code you will change before you change it, and follow what they show about how the code is meant to behave.",
];

/// The `[night]` table of the configuration, with the Python runner's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub window: String,
    pub task_timeout_s: u64,
    pub test_timeout_s: u64,
    pub task_memory: String,
    pub nudges: u32,
    pub best_of: u32,
    pub best_of_hints: Vec<String>,
    pub test: Option<String>,
    pub test_sandbox: bool,
    pub airgapped: Option<Value>,
    pub prompt: Option<String>,
    pub compact_at: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::from_table(&Value::Object(Map::new()))
    }
}

impl Settings {
    /// From the `[night]` table as JSON; a missing or unreadable key takes its default.
    pub fn from_table(table: &Value) -> Self {
        let get = |key: &str| table.get(key).filter(|v| !v.is_null());
        let text = |key: &str| get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from);
        let count = |key: &str, default: u64, least: u64| {
            get(key).and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64))).unwrap_or(default).max(least)
        };
        Settings {
            window: text("window").unwrap_or_else(|| DEFAULT_WINDOW.to_string()),
            task_timeout_s: get("task_timeout").and_then(parse_duration).unwrap_or(90 * 60),
            test_timeout_s: get("test_timeout").and_then(parse_duration).unwrap_or(20 * 60),
            task_memory: text("task_memory").unwrap_or_else(|| "8G".to_string()),
            nudges: count("nudges", 2, 0) as u32,
            best_of: count("best_of", 3, 1) as u32,
            best_of_hints: resolve_hints(get("best_of_hints")),
            test: text("test"),
            test_sandbox: get("test_sandbox").and_then(Value::as_bool).unwrap_or(true),
            airgapped: get("airgapped").cloned(),
            prompt: text("prompt"),
            compact_at: get("compact_at").and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).map(|n| n.max(0) as u64),
        }
    }

    /// The `[night]` table of the configuration file; the defaults when there is none.
    pub fn load() -> Self {
        let table = crate::config_file()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|config| config.get("night").cloned())
            .and_then(|night| serde_json::to_value(night).ok())
            .unwrap_or(Value::Object(Map::new()));
        Settings::from_table(&table)
    }
}

/// `90m`, `1.5h`, `600s`, `20` (minutes) or a number of minutes, in seconds.
pub fn parse_duration(value: &Value) -> Option<u64> {
    if let Some(minutes) = value.as_f64() {
        return Some((minutes * 60.0) as u64);
    }
    let pattern = regex_lite::Regex::new(r"^\s*(\d+(?:\.\d+)?)\s*([smh]?)\s*$").ok()?;
    let found = pattern.captures(value.as_str()?)?;
    let number: f64 = found[1].parse().ok()?;
    let unit = match &found[2] {
        "s" => 1.0,
        "h" => 3600.0,
        _ => 60.0,
    };
    Some((number * unit) as u64)
}

/// `best_of_hints`: `true` or `"on"` for the default strategies, a list for one's own, anything
/// else for none.
pub fn resolve_hints(value: Option<&Value>) -> Vec<String> {
    let defaults = || DEFAULT_HINTS.iter().map(|h| (*h).to_string()).collect();
    match value {
        Some(Value::Bool(true)) => defaults(),
        Some(Value::String(word)) if ["on", "true", "yes", "default"].contains(&word.trim().to_lowercase().as_str()) => defaults(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    }
}

/// The strategy attempt `k` is given: none for the first, then the hints in turn.
pub fn hint_for(number: u32, hints: &[String]) -> Option<String> {
    if number <= 1 || hints.is_empty() {
        return None;
    }
    Some(hints[(number as usize - 2) % hints.len()].clone())
}

/// A judge the tests put in place of the model server; it sees the issue too.
pub type Judge = Box<dyn FnMut(&Candidate, &Candidate) -> Option<(f64, f64)>>;

/// What every task of one night shares.
pub struct Ctx {
    pub night: PathBuf,
    pub ragon: PathBuf,
    pub settings: Settings,
    pub model_host: Option<String>,
    pub model_node: Option<String>,
    pub context_budget: Option<u64>,
    pub window_end: Option<Instant>,
    /// Wrap commands in a systemd scope (Linux; off in tests).
    pub use_scope: bool,
    /// Run the tests through `ragon sandbox`.
    pub use_sandbox: bool,
    /// Added to every child's environment after the night run's own (tests point the stand-in
    /// `ragon` here).
    pub extra_env: Vec<(String, String)>,
    pub stop: Arc<AtomicBool>,
    pub git: Git,
    pub judge: RefCell<Option<Judge>>,
}

impl Ctx {
    pub fn new(night: &Path, ragon: &Path, settings: Settings) -> Self {
        Ctx {
            night: night.to_path_buf(),
            ragon: ragon.to_path_buf(),
            settings,
            model_host: None,
            model_node: None,
            context_budget: None,
            window_end: None,
            use_scope: cfg!(target_os = "linux"),
            use_sandbox: true,
            extra_env: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
            git: Git::new(&hooks_off(night)),
            judge: RefCell::new(None),
        }
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    pub fn window_over(&self) -> bool {
        self.stopped() || self.window_end.is_some_and(|end| Instant::now() >= end)
    }

    /// The environment every child gets on top of the night run's own.
    pub fn child_env(&self, level: Option<&str>, extra: &[(String, String)]) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = vec![(NIGHT_RUN_ENV.into(), "1".into())];
        if let Some(host) = &self.model_host {
            env.push(("DREAMFERENCE_VLLM_HOST".into(), host.clone()));
        }
        if let Some((name, value)) = crate::auth::local::child_env() {
            env.push((name.into(), value));
        }
        env.push((ADAPTER_ENV.into(), String::new()));
        let hosts: Vec<String> = self.model_host.iter().cloned().collect();
        let upper = std::env::var("NO_PROXY").ok();
        let lower = std::env::var("no_proxy").ok();
        let bypass = crate::proxy::merged(upper.as_deref(), lower.as_deref(), &hosts);
        env.push(("NO_PROXY".into(), bypass.clone()));
        env.push(("no_proxy".into(), bypass));
        env.push(("GIT_TERMINAL_PROMPT".into(), "0".into()));
        env.push(("DREAMFERENCE_RAGON_REFINE".into(), "off".into()));
        env.push(("DREAMFERENCE_RAGON_WORKLOAD".into(), "night".into()));
        if let Some(level) = level {
            env.push((ragon_airgapped::ENV_VAR.into(), level.into()));
        }
        if let Some(prompt) = &self.settings.prompt {
            env.push(("DREAMFERENCE_RAGON_PROMPT".into(), prompt.clone()));
        }
        env.extend(self.extra_env.iter().cloned());
        env.extend(extra.iter().cloned());
        env
    }
}

/// The `core.hooksPath` that holds no hooks: `/dev/null` on Unix; an empty folder on Windows.
fn hooks_off(night: &Path) -> PathBuf {
    if cfg!(windows) {
        let dir = night.join("hooks-off");
        let _ = std::fs::create_dir_all(&dir);
        dir
    } else {
        PathBuf::from("/dev/null")
    }
}

// ---- the queue ----

fn task_path(night: &Path, id: &str) -> PathBuf {
    night.join("tasks").join(format!("{id}.json"))
}

/// One task's record.
pub fn read(night: &Path, id: &str) -> Option<Value> {
    let text = std::fs::read_to_string(task_path(night, id)).ok()?;
    serde_json::from_str(&text).ok().filter(Value::is_object)
}

/// Every task, oldest first (by id, which starts with the time it was added).
pub fn tasks(night: &Path) -> Vec<Value> {
    let Ok(entries) = std::fs::read_dir(night.join("tasks")) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from))
        .collect();
    ids.sort();
    ids.iter().filter_map(|id| read(night, id)).collect()
}

pub fn is_cancelled(night: &Path, id: &str) -> bool {
    read(night, id).is_some_and(|task| CANCELLING_STATUSES.contains(&task["status"].as_str().unwrap_or("")))
}

pub fn now_text() -> String {
    Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
}

/// Moves a task to `status` unless it was cancelled meanwhile (then to `cancelled`), setting
/// `fields` first; returns the status it has. A history entry is added when the status changes
/// or there is a note, as the Python queue does.
pub fn transition(night: &Path, id: &str, status: &str, note: Option<&str>, fields: Vec<(&str, Value)>) -> String {
    let outcome = crate::night::update_task(night, id, |record| {
        let object = record.as_object_mut().ok_or("not an object")?;
        for (key, value) in fields {
            object.insert(key.to_string(), value);
        }
        let current = object.get("status").and_then(Value::as_str).unwrap_or("").to_string();
        let wanted = if CANCELLING_STATUSES.contains(&current.as_str()) && status != "cancelled" { "cancelled" } else { status };
        if current != wanted || note.is_some() {
            object.insert("status".into(), json!(wanted));
            let mut entry = Map::new();
            entry.insert("at".into(), json!(now_text()));
            entry.insert("status".into(), json!(wanted));
            if let Some(note) = note {
                entry.insert("note".into(), json!(note));
            }
            let history = object.entry("history").or_insert_with(|| json!([]));
            if let Some(list) = history.as_array_mut() {
                list.push(Value::Object(entry));
            }
        }
        Ok(wanted.to_string())
    });
    outcome.unwrap_or_else(|_| status.to_string())
}

/// Where the runner lock is: `<night>/runner.lock`, or under `%ProgramData%` on Windows.
pub fn runner_lock_path(night: &Path) -> PathBuf {
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return WINDOWS_LOCK_DIR.iter().fold(base, |path, part| path.join(part)).join("runner.lock");
    }
    night.join("runner.lock")
}

/// Takes the runner lock without waiting and writes the holder into it; `None` when another
/// night run holds it. The lock goes with the file.
pub fn runner_lock(path: &Path, holder: &str) -> Option<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let mut file = OpenOptions::new().create(true).truncate(false).write(true).open(path).ok()?;
    file.try_lock().ok()?;
    let _ = file.set_len(0);
    let _ = file.rewind();
    let _ = writeln!(file, "{holder}");
    Some(file)
}

/// When the night ends: `minutes` from now, `until` (HH:MM), or the end of `[night] window`;
/// a time already past today is tomorrow's.
pub fn window_end(window: &str, until: Option<&str>, minutes: Option<u64>) -> Option<chrono::DateTime<Local>> {
    let now = Local::now();
    if let Some(minutes) = minutes {
        return Some(now + chrono::Duration::minutes(minutes as i64));
    }
    let hhmm = match until {
        Some(until) => until.trim().to_string(),
        None => {
            let pattern = regex_lite::Regex::new(r"^\s*(\d{1,2}):(\d{2})\s*-\s*(\d{1,2}):(\d{2})\s*$").ok()?;
            let found = pattern.captures(window)?;
            format!("{}:{}", &found[3], &found[4])
        }
    };
    let time = NaiveTime::parse_from_str(&hhmm, "%H:%M").ok()?;
    let mut end = Local.from_local_datetime(&now.date_naive().and_time(time)).earliest()?;
    if end <= now {
        end += chrono::Duration::days(1);
    }
    Some(end)
}

/// The start of `[night] window` (HH:MM), for the schedule.
pub fn window_start(window: &str) -> Option<String> {
    let pattern = regex_lite::Regex::new(r"^\s*(\d{1,2}):(\d{2})\s*-").ok()?;
    let found = pattern.captures(window)?;
    Some(format!("{:02}:{}", found[1].parse::<u32>().ok()?, &found[2]))
}

/// Tasks in turn by repository, so one busy repository cannot take the whole night.
pub fn round_robin(tasks: Vec<Value>) -> Vec<Value> {
    let mut repos: Vec<(String, Vec<Value>)> = Vec::new();
    for task in tasks {
        let repo = task["repo"].as_str().unwrap_or("?").to_string();
        match repos.iter_mut().find(|(r, _)| *r == repo) {
            Some((_, list)) => list.push(task),
            None => repos.push((repo, vec![task])),
        }
    }
    let mut ordered = Vec::new();
    let mut index = 0;
    loop {
        let mut any = false;
        for (_, list) in &repos {
            if let Some(task) = list.get(index) {
                ordered.push(task.clone());
                any = true;
            }
        }
        if !any {
            return ordered;
        }
        index += 1;
    }
}

/// How many attempts a task gets: one when it resumes a session or its worktree is kept.
pub fn attempts_for(task: &Value, settings: &Settings, night: &Path) -> u32 {
    if task.get("session").is_some_and(|s| !s.is_null() && s != "") || task.get("remote").is_some_and(Value::is_object) {
        return 1;
    }
    if night.join("worktrees").join(task["id"].as_str().unwrap_or("")).is_dir() {
        return 1;
    }
    let wanted = task.get("best_of").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())));
    wanted.filter(|n| *n > 0).map(|n| n as u32).unwrap_or(settings.best_of).max(1)
}

/// Runs one task to its end: best of N, or one attempt.
pub fn run_task(ctx: &Ctx, task: &Value, deadline: Instant) -> String {
    if attempts_for(task, &ctx.settings, &ctx.night) <= 1 {
        TaskRun::new(ctx, task, deadline, None, None).run()
    } else {
        BestOfN::new(ctx, task, deadline).run()
    }
}

/// Whether the model server answers; the reason when it does not.
fn admission(host: &str) -> Result<(), String> {
    let trimmed = host.trim_end_matches('/');
    let url = format!("{}/v1/models", trimmed.strip_suffix("/v1").unwrap_or(trimmed));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder().no_proxy().timeout(ADMISSION_TIMEOUT).build().map_err(|e| e.to_string())?;
    runtime.block_on(async {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => Err(format!("the model server at {host} answered {}", response.status().as_u16())),
            Err(_) => Err(format!("the model server at {host} does not answer")),
        }
    })
}

/// One night: takes the runner lock, runs what is queued until the window ends, writes the
/// report. Returns the exit code and the lines to print.
pub fn run_night(ctx: &Ctx, lock: &Path, end: Option<chrono::DateTime<Local>>) -> (i32, Vec<String>) {
    let started = Local::now();
    let holder = format!("ragon night run, pid {}", std::process::id());
    let Some(_lock) = runner_lock(lock, &holder) else {
        return (1, vec!["⚠️  Another night run holds the runner lock.".to_string()]);
    };
    let pending: Vec<Value> = tasks(&ctx.night)
        .into_iter()
        .filter(|t| RUNNABLE_STATUSES.contains(&t["status"].as_str().unwrap_or("")))
        .collect();
    if pending.is_empty() {
        return (0, vec!["✅ Night Shift: nothing queued.".to_string()]);
    }
    let mut lines = Vec::new();
    let end_text = end.map(|e| e.format("%H:%M").to_string()).unwrap_or_else(|| "—".into());
    lines.push(format!("🌙 Night Shift: {} task(s) queued; window ends {end_text}.", pending.len()));
    let mut notes: Vec<String> = Vec::new();
    let ids: Vec<String> = pending.iter().filter_map(|t| t["id"].as_str().map(String::from)).collect();
    let admitted = match &ctx.model_host {
        Some(host) => admission(host),
        None => Ok(()),
    };
    match admitted {
        Err(reason) => {
            notes.push(format!("Not run: {reason}. The tasks stay queued for the next night."));
            lines.push(format!("⚠️  {reason}"));
        }
        Ok(()) => {
            let ordered = round_robin(pending);
            let mut not_started = 0;
            for task in &ordered {
                if ctx.window_over() {
                    not_started += 1;
                    continue;
                }
                let limit = Instant::now() + Duration::from_secs(ctx.settings.task_timeout_s);
                let deadline = ctx.window_end.map_or(limit, |end| end.min(limit));
                run_task(ctx, task, deadline);
            }
            if not_started > 0 {
                notes.push(format!("The window closed with {not_started} task(s) not started; they stay queued."));
            }
        }
    }
    let finished: Vec<Value> = ids.iter().filter_map(|id| read(&ctx.night, id)).collect();
    let mut written = None;
    let text = report::render(started, Local::now(), &finished, &notes);
    match report::write(&ctx.night, started, &text) {
        Ok(path) => written = Some(path),
        Err(error) => {
            notes.push(format!("The report could not be written: {error}"));
        }
    }
    let mut counts: Vec<(String, usize)> = Vec::new();
    for task in &finished {
        let status = task["status"].as_str().unwrap_or("?").to_string();
        match counts.iter_mut().find(|(s, _)| *s == status) {
            Some((_, n)) => *n += 1,
            None => counts.push((status, 1)),
        }
    }
    let summary: Vec<String> = counts.iter().map(|(s, n)| format!("{n} {s}")).collect();
    let place = written
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| format!("not written, see {}", ctx.night.join("tasks").display()));
    lines.push(format!("✅ Night Shift finished: {}. Report: {place}", summary.join(", ")));
    (if written.is_some() { 0 } else { 1 }, lines)
}

// ---- Task Scheduler ----

/// `schtasks` arguments that run `ragon night run` every day at `start`, as the user who
/// registers it, without raised rights. `/F` replaces an entry of the same name.
pub fn schtasks_create(exe: &Path, start: &str) -> Vec<String> {
    [
        "/Create",
        "/TN",
        SCHEDULED_TASK,
        "/TR",
        &format!("\"{}\" night run", exe.display()),
        "/SC",
        "DAILY",
        "/ST",
        start,
        "/RL",
        "LIMITED",
        "/F",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect()
}

/// `schtasks` arguments that remove the entry.
pub fn schtasks_delete() -> Vec<String> {
    ["/Delete", "/TN", SCHEDULED_TASK, "/F"].iter().map(|s| (*s).to_string()).collect()
}

fn schtasks(args: &[String]) -> (i32, Vec<String>) {
    if !cfg!(windows) {
        return (
            2,
            vec![
                "Night Shift: the schedule is a Task Scheduler entry, on Windows; on Linux the night runs from ragon-admin's timer.".to_string(),
                format!("It would run: schtasks {}", args.join(" ")),
            ],
        );
    }
    match std::process::Command::new("schtasks").args(args).output() {
        Ok(output) => {
            let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(String::from).collect();
            lines.extend(String::from_utf8_lossy(&output.stderr).lines().map(String::from));
            (output.status.code().unwrap_or(1), lines)
        }
        Err(error) => (1, vec![format!("Night Shift: could not run schtasks: {error}")]),
    }
}

// ---- the command line ----

const USAGE: &str = "usage: ragon night run [--until HH:MM | --minutes N] | ragon night schedule [--at HH:MM] | ragon night unschedule";

/// Whether `ragon night <args>` is one of the commands here.
pub fn handles(args: &[String]) -> bool {
    matches!(args.first().map(String::as_str), Some("run" | "schedule" | "unschedule"))
}

fn option(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(None),
        Some(i) => args.get(i + 1).cloned().map(Some).ok_or_else(|| format!("{name} needs a value")),
    }
}

/// `ragon night run|schedule|unschedule …`; prints and returns the exit code.
pub fn cli(night: &Path, args: &[String]) -> i32 {
    let (code, lines) = cli_lines(night, args);
    for line in lines {
        println!("{line}");
    }
    code
}

fn cli_lines(night: &Path, args: &[String]) -> (i32, Vec<String>) {
    let settings = Settings::load();
    let rest = &args[1..];
    match args[0].as_str() {
        "unschedule" => schtasks(&schtasks_delete()),
        "schedule" => {
            let start = match option(rest, "--at") {
                Ok(Some(at)) => at,
                Ok(None) => window_start(&settings.window).unwrap_or_else(|| "01:00".into()),
                Err(e) => return (2, vec![e, USAGE.into()]),
            };
            if NaiveTime::parse_from_str(&start, "%H:%M").is_err() {
                return (2, vec![format!("--at wants HH:MM, not {start}"), USAGE.into()]);
            }
            let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ragon"));
            schtasks(&schtasks_create(&exe, &start))
        }
        _ => {
            let (until, minutes) = match (option(rest, "--until"), option(rest, "--minutes")) {
                (Ok(u), Ok(m)) => (u, m),
                (Err(e), _) | (_, Err(e)) => return (2, vec![e, USAGE.into()]),
            };
            let minutes = match minutes.map(|m| m.parse::<u64>()) {
                Some(Ok(m)) => Some(m),
                Some(Err(_)) => return (2, vec!["--minutes wants a number".into(), USAGE.into()]),
                None => None,
            };
            let Some(end) = window_end(&settings.window, until.as_deref(), minutes) else {
                return (2, vec![format!("could not read the window end from {:?} / {until:?}", settings.window), USAGE.into()]);
            };
            let ragon = std::env::var_os("RAGON_NIGHT_RAGON_BIN")
                .map(PathBuf::from)
                .or_else(|| std::env::current_exe().ok())
                .unwrap_or_else(|| PathBuf::from("ragon"));
            let mut ctx = Ctx::new(night, &ragon, settings);
            let host = crate::vllm_host();
            ctx.model_host = (!host.trim().is_empty()).then_some(host);
            ctx.context_budget = ctx.settings.compact_at.filter(|n| *n > 0);
            let left = (end - Local::now()).to_std().unwrap_or_default();
            ctx.window_end = Some(Instant::now() + left);
            run_night(&ctx, &runner_lock_path(night), Some(end))
        }
    }
}
