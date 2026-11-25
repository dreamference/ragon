//! Node jobs, the platform-neutral part (WN1, backlog row 20; the Linux model is
//! `dreamference/node/node_job.py`). A job is a command a sender runs on this node at a commit of
//! one of its repositories, with a memory cap and a time limit; its result is a branch.
//!
//! This module holds what needs no operating system call: the limits and their parsing, the
//! request check ([`validate`]), the job folder and its `record.json` ([`Store`]), the lane queue
//! ([`next_to_start`]) and the limit policy ([`Watch`]). `job_exec` runs one job, `job_runner`
//! holds the running ones (a Job Object each on Windows), `job_cli` is `server job ...`.
//!
//! Records are the Linux node's records (same keys, same statuses), so a sender's tool reads both;
//! timestamps are UTC (`2026-11-25T10:00:00Z`). Two keys are new: `lane` (one job runs per lane at
//! a time, the others wait `queued` in submission order) and `stopped` (why the runner ended it).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::json::{self, Value};

/// The most a sender may ask for; this node's ceilings, not the sender's.
pub const MAX_MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const MAX_TIME_S: u64 = 8 * 3600;
/// The time limit of a request that names none (the Linux node refuses it instead).
pub const DEFAULT_TIME_S: u64 = 3600;
/// Processes a job may hold at once, so a fork bomb stops at its own job.
pub const MAX_PROCESSES: u32 = 512;
pub const DEFAULT_LANE: &str = "default";
pub const FINAL_STATUSES: [&str; 4] = ["done", "failed", "refused", "cancelled"];
pub const RECORD_FILE: &str = "record.json";
pub const OUTPUT_LOG: &str = "output.log";
pub const TREE_DIR: &str = "tree";
/// Asking a running job to stop: the job's own process sees this file and ends its child.
pub const JOB_STOP_FILE: &str = "stop";
/// How long a job that was asked to stop gets before it is killed.
pub const STOP_GRACE: Duration = Duration::from_secs(10);
/// A finished job is kept a day after the sender fetched it, 14 days when nobody did.
pub const KEEP_AFTER_FETCH_S: u64 = 86_400;
pub const KEEP_UNFETCHED_S: u64 = 14 * 86_400;
pub const AIRGAP_LEVELS: [&str; 2] = ["off", "on"];
const MAX_OUT_LEN: usize = 201;

pub fn is_final(status: &str) -> bool {
    FINAL_STATUSES.contains(&status)
}

/// `8G`, `512M`, `1.5GiB`, `1024` (bytes): Night Shift's sizes, powers of 1024.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let split = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
    let (number, unit) = t.split_at(split);
    let number: f64 = number.parse().map_err(|_| format!("not a size: {text:?}"))?;
    let mut unit = unit.trim().to_ascii_uppercase();
    if unit.ends_with('B') {
        unit.pop();
    }
    if unit.ends_with('I') {
        unit.pop();
    }
    let power = match unit.as_str() {
        "" => 0,
        "K" => 1,
        "M" => 2,
        "G" => 3,
        "T" => 4,
        _ => return Err(format!("not a size: {text:?}")),
    };
    Ok((number * 1024f64.powi(power)) as u64)
}

/// `90m`, `2h`, `45s`; a bare number is minutes (Night Shift's rule).
pub fn parse_duration(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let (number, factor) = match t.chars().last() {
        Some('s' | 'S') => (&t[..t.len() - 1], 1.0),
        Some('m' | 'M') => (&t[..t.len() - 1], 60.0),
        Some('h' | 'H') => (&t[..t.len() - 1], 3600.0),
        _ => (t, 60.0),
    };
    let number: f64 = number.trim().parse().map_err(|_| format!("not a duration: {text:?}"))?;
    if !number.is_finite() || number < 0.0 {
        return Err(format!("not a duration: {text:?}"));
    }
    Ok((number * factor) as u64)
}

/// `20261125-1130-abc`.
pub fn is_job_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 17
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..13].iter().all(u8::is_ascii_digit)
        && b[13] == b'-'
        && b[14..].iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// A repository or lane name: `[A-Za-z0-9][A-Za-z0-9._-]{0,80}` without `..`.
pub fn is_slug(slug: &str) -> bool {
    let b = slug.as_bytes();
    !b.is_empty()
        && b.len() <= 81
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c))
        && !slug.contains("..")
}

fn is_commit(commit: &str) -> bool {
    commit.len() == 40 && commit.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn is_out_path(out: &str) -> bool {
    let b = out.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_OUT_LEN
        && (b[0].is_ascii_alphanumeric() || b"._-".contains(&b[0]))
        && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._/-".contains(c))
        && !out.split('/').any(|part| part == "..")
}

/// The stricter of two `/airgapped` levels; an unknown name counts as `on`.
pub fn stricter(first: &str, second: &str) -> &'static str {
    let rank = |level: &str| AIRGAP_LEVELS.iter().position(|l| *l == level).unwrap_or(AIRGAP_LEVELS.len() - 1);
    AIRGAP_LEVELS[rank(first).max(rank(second))]
}

/// What this node allows, beside the fixed ceilings.
#[derive(Clone, Debug)]
pub struct NodePolicy {
    /// The node's own `/airgapped` level; a job runs at the stricter of it and its own.
    pub airgap: String,
    /// Whether this node can cut a job's network. The Windows node cannot yet (its job sandbox
    /// is the unelevated one, see job_exec.rs), so it refuses an air-gapped job instead of
    /// running it with the network.
    pub can_airgap: bool,
}

impl Default for NodePolicy {
    fn default() -> Self {
        NodePolicy { airgap: "off".into(), can_airgap: !cfg!(windows) }
    }
}

/// Turns what a sender asked for into a job record (status `queued`), or refuses it with the
/// reason. Everything in `request` came from another machine.
pub fn validate(request: &Value, node: &NodePolicy, now: &str) -> Result<Value, String> {
    if !matches!(request, Value::Object(_)) {
        return Err("a job is a JSON object".into());
    }
    let text = |key: &str| request.get(key).and_then(Value::as_str);
    let id = text("id").unwrap_or_default();
    if !is_job_id(id) {
        return Err(format!("not a job id: {id:?}"));
    }
    let repo = text("repo").unwrap_or_default();
    if !is_slug(repo) {
        return Err(format!("not a repository name: {repo:?}"));
    }
    let commit = text("commit").unwrap_or_default();
    if !is_commit(commit) {
        return Err("a job names the commit it runs at, as a full hash".into());
    }
    let command = request.get("command").and_then(Value::as_array);
    let words: Option<Vec<&str>> = command.and_then(|c| c.iter().map(Value::as_str).collect::<Option<Vec<_>>>());
    let command = match words {
        Some(words) if !words.is_empty() && words.iter().all(|w| !w.is_empty()) => words,
        _ => return Err("a job has a command".into()),
    };
    let field = |key: &str| -> Option<String> {
        request.get(key).filter(|v| v.truthy()).map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_json(),
        })
    };
    let memory = field("memory").ok_or("a job needs a memory cap")?;
    let time = field("time").unwrap_or_else(|| format!("{DEFAULT_TIME_S}s"));
    let memory_bytes = parse_size(&memory)?;
    let time_s = parse_duration(&time)?;
    if memory_bytes == 0 || time_s == 0 {
        return Err("a job needs a memory cap and a time limit".into());
    }
    if memory_bytes > MAX_MEMORY_BYTES {
        return Err(format!("this node allows a job at most {}G of memory", MAX_MEMORY_BYTES >> 30));
    }
    if time_s > MAX_TIME_S {
        return Err(format!("this node allows a job at most {}h", MAX_TIME_S / 3600));
    }
    if request.get("gpu").is_some_and(Value::truthy) {
        return Err("GPU jobs are not allowed on a node for now: a memory cap does not bound GPU memory".into());
    }
    let optional_text = |key: &str, message: &str| -> Result<Option<String>, String> {
        match request.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.is_empty() => Ok(None),
            Some(Value::String(s)) if key != "setup" || !s.trim().is_empty() => Ok(Some(s.clone())),
            _ => Err(message.to_string()),
        }
    };
    let test = optional_text("test", "the test command is a string")?;
    let setup = optional_text("setup", "the setup command is a string")?;
    let out = optional_text("out", "--out names a folder inside the repository, such as `checkpoints`")?;
    if let Some(out) = &out {
        if !is_out_path(out) || out.starts_with('/') {
            return Err("--out names a folder inside the repository, such as `checkpoints`".into());
        }
        // Not ported from the Linux node yet: the folder would be kept out of the commit and sent.
        return Err("this node does not keep --out folders yet".into());
    }
    if request.get("binds").is_some_and(Value::truthy) {
        return Err("this node does not allow binding paths into a job".into());
    }
    let lane = optional_text("lane", "a lane is a name")?.unwrap_or_else(|| DEFAULT_LANE.to_string());
    if !is_slug(&lane) {
        return Err(format!("not a lane name: {lane:?}"));
    }
    let asked = field("airgapped").unwrap_or_else(|| "off".into());
    let airgapped = stricter(&asked, &node.airgap);
    if airgapped == "on" && !node.can_airgap {
        return Err("this node cannot cut a job's network yet, so it does not take air-gapped jobs".into());
    }
    let author = request.get("author");
    let author_field = |key: &str, default: &str, max: usize| -> String {
        let value = author.and_then(|a| a.get(key)).filter(|v| v.truthy()).and_then(Value::as_str).unwrap_or(default);
        value.chars().take(max).collect()
    };
    let sender: String = text("sender").unwrap_or_default().chars().take(100).collect();
    Ok(Value::Object(vec![
        ("id".into(), Value::str(id)),
        ("repo".into(), Value::str(repo)),
        ("commit".into(), Value::str(commit)),
        ("command".into(), Value::Array(command.iter().map(|w| Value::str(*w)).collect())),
        ("memory".into(), Value::str(memory)),
        ("memory_bytes".into(), Value::num(memory_bytes as f64)),
        ("time".into(), Value::str(time)),
        ("time_s".into(), Value::num(time_s as f64)),
        ("test".into(), Value::opt_str(test)),
        ("setup".into(), Value::opt_str(setup)),
        ("out".into(), Value::opt_str(out.map(|o| o.trim_matches('/').to_string()))),
        ("binds".into(), Value::Array(vec![])),
        ("airgapped".into(), Value::str(airgapped)),
        (
            "author".into(),
            Value::Object(vec![
                ("name".into(), Value::str(author_field("name", "Ragon Job", 100))),
                ("email".into(), Value::str(author_field("email", "ragon-job@localhost", 200))),
            ]),
        ),
        ("sender".into(), Value::str(sender)),
        ("lane".into(), Value::str(lane)),
        ("status".into(), Value::str("queued")),
        ("submitted".into(), Value::str(now)),
        ("branch".into(), Value::Null),
        ("exit_code".into(), Value::Null),
        ("note".into(), Value::Null),
    ]))
}

/// A record the runner is about to act on is checked again: another account writes the queue
/// folder, so a record is not trusted because it sits there. Returns the reason when it is bad.
pub fn recheck(record: &Value) -> Result<(), String> {
    let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
    let mut request = record.clone();
    // The record is the validated request; `validate` must accept it back unchanged in substance.
    request.set("binds", Value::Null);
    let again = validate(&request, &NodePolicy { airgap: "off".into(), can_airgap: true }, "")?;
    for key in ["id", "repo", "commit", "command", "memory_bytes", "time_s", "lane"] {
        if again.get(key) != record.get(key) {
            return Err(format!("job {id}: its record's {key} does not match its request"));
        }
    }
    Ok(())
}

pub fn status_of(record: &Value) -> &str {
    record.get("status").and_then(Value::as_str).unwrap_or("")
}

pub fn lane_of(record: &Value) -> &str {
    record.get("lane").and_then(Value::as_str).filter(|l| !l.is_empty()).unwrap_or(DEFAULT_LANE)
}

pub fn id_of(record: &Value) -> &str {
    record.get("id").and_then(Value::as_str).unwrap_or("")
}

/// The jobs to start now: per lane with nothing running, its oldest queued job (submission time,
/// then id). `records` is every job; `busy_lanes` the lanes the runner holds a job in.
pub fn next_to_start<'a>(records: &'a [Value], busy_lanes: &[String]) -> Vec<&'a Value> {
    let mut queued: Vec<&Value> = records.iter().filter(|r| status_of(r) == "queued").collect();
    queued.sort_by(|a, b| {
        let key = |r: &&Value| (r.get("submitted").and_then(Value::as_str).unwrap_or("").to_string(), id_of(r).to_string());
        key(a).cmp(&key(b))
    });
    let mut taken: Vec<&str> = busy_lanes.iter().map(String::as_str).collect();
    // A record marked running is in its lane too (the runner set it so before starting it).
    taken.extend(records.iter().filter(|r| status_of(r) == "running").map(lane_of));
    let mut start = Vec::new();
    for record in queued {
        let lane = lane_of(record);
        if !taken.contains(&lane) {
            taken.push(lane);
            start.push(record);
        }
    }
    start
}

/// Why the runner ended a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    TimeLimit,
    MemoryCap,
    Cancelled,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::TimeLimit => "time limit",
            Reason::MemoryCap => "memory cap",
            Reason::Cancelled => "cancelled",
        }
    }

    /// The record's note for a job ended for this reason.
    pub fn note(self, record: &Value) -> String {
        let field = |key: &str| record.get(key).and_then(Value::as_str).unwrap_or("?").to_string();
        match self {
            Reason::TimeLimit => format!("the job passed its time limit ({}) and was stopped", field("time")),
            Reason::MemoryCap => format!("the job passed its memory cap ({}) and was stopped", field("memory")),
            Reason::Cancelled => "the job was cancelled".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do.
    Run,
    /// Ask the job to stop (the SIGTERM of a Windows job: the stop file; the job's process ends
    /// its child and records what it has).
    SoftStop(Reason),
    /// The grace is over: kill everything in the job.
    Kill(Reason),
}

/// The limit policy of one running job, on a caller-supplied clock (elapsed time since start), so
/// it is tested without waiting. The first reason seen wins; its soft stop is asked once, the kill
/// comes `grace` later.
#[derive(Clone, Debug)]
pub struct Watch {
    pub limit: Duration,
    pub grace: Duration,
    stopping: Option<(Reason, Duration)>,
    killed: bool,
}

impl Watch {
    pub fn new(limit: Duration, grace: Duration) -> Self {
        Watch { limit, grace, stopping: None, killed: false }
    }

    pub fn reason(&self) -> Option<Reason> {
        self.stopping.map(|(r, _)| r)
    }

    pub fn tick(&mut self, elapsed: Duration, memory_tripped: bool, cancelled: bool) -> Action {
        if self.killed {
            return Action::Run;
        }
        if let Some((reason, since)) = self.stopping {
            if elapsed.saturating_sub(since) >= self.grace {
                self.killed = true;
                return Action::Kill(reason);
            }
            return Action::Run;
        }
        let reason = if cancelled {
            Reason::Cancelled
        } else if memory_tripped {
            Reason::MemoryCap
        } else if elapsed >= self.limit {
            Reason::TimeLimit
        } else {
            return Action::Run;
        };
        self.stopping = Some((reason, elapsed));
        if self.grace.is_zero() {
            self.killed = true;
            return Action::Kill(reason);
        }
        Action::SoftStop(reason)
    }

    /// How long until this watch next has something to do.
    pub fn due_in(&self, elapsed: Duration) -> Duration {
        match self.stopping {
            _ if self.killed => Duration::from_secs(3600),
            Some((_, since)) => (since + self.grace).saturating_sub(elapsed),
            None => self.limit.saturating_sub(elapsed),
        }
    }
}

/// Seconds since the epoch from [`crate::core::utc_timestamp`]'s form (`Z` or `+00:00`); None for
/// anything else, which [`Store::prune`] then keeps.
pub fn parse_utc_timestamp(text: &str) -> Option<u64> {
    let t = text.trim();
    let t = t.strip_suffix('Z').or_else(|| t.strip_suffix("+00:00"))?;
    let (date, time) = t.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut c = time.split(':').map(|p| p.parse::<i64>().ok());
    let (hh, mm, ss) = (c.next()??, c.next()??, c.next()??);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// The jobs folder: `repos/<slug>.git` (bare repositories senders push to) and one folder per job
/// (`record.json`, `output.log`, `tree/` while it runs).
#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Store { root: root.into() }
    }

    pub fn job_dir(&self, id: &str) -> Result<PathBuf, String> {
        if !is_job_id(id) {
            return Err(format!("not a job id: {id:?}"));
        }
        Ok(self.root.join(id))
    }

    pub fn repo_path(&self, slug: &str) -> Result<PathBuf, String> {
        if !is_slug(slug) {
            return Err(format!("not a repository name: {slug:?}"));
        }
        Ok(self.root.join("repos").join(format!("{slug}.git")))
    }

    pub fn read(&self, id: &str) -> Option<Value> {
        let text = std::fs::read_to_string(self.job_dir(id).ok()?.join(RECORD_FILE)).ok()?;
        json::parse(&text).ok().filter(|v| id_of(v) == id)
    }

    /// Writes a record through a temporary file and a rename, so a reader never sees half of it.
    pub fn write(&self, record: &Value) -> Result<(), String> {
        let dir = self.job_dir(id_of(record))?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let tmp = dir.join(format!("{RECORD_FILE}.{}.tmp", std::process::id()));
        std::fs::write(&tmp, record.to_json_pretty()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        // On Windows a reader holding the file open makes the rename fail for a moment: retry.
        let target = dir.join(RECORD_FILE);
        let mut last = None;
        for _ in 0..50 {
            match std::fs::rename(&tmp, &target) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_file(&tmp);
        Err(format!("cannot write {}: {}", target.display(), last.map(|e| e.to_string()).unwrap_or_default()))
    }

    /// Reads, changes and writes a record under the job's lock (a folder, created atomically),
    /// so the runner, the job's own process and a `cancel` never lose each other's change.
    /// Returns the record as written, or None when there is no such job.
    pub fn update<F: FnOnce(&mut Value)>(&self, id: &str, change: F) -> Result<Option<Value>, String> {
        let dir = self.job_dir(id)?;
        let _lock = DirLock::take(&dir.join("record.lock"))?;
        let Some(mut record) = self.read(id) else { return Ok(None) };
        change(&mut record);
        self.write(&record)?;
        Ok(Some(record))
    }

    /// The ids of every job folder, oldest first.
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| is_job_id(n))
            .collect();
        ids.sort();
        ids
    }

    pub fn records(&self) -> Vec<Value> {
        self.ids().iter().filter_map(|id| self.read(id)).collect()
    }

    /// Accepts a request: checks it and writes its record, `queued`. Refuses an id that exists.
    pub fn submit(&self, request: &Value, node: &NodePolicy, now: &str) -> Result<Value, String> {
        let record = validate(request, node, now)?;
        let dir = self.job_dir(id_of(&record))?;
        std::fs::create_dir_all(&self.root).map_err(|e| format!("cannot create {}: {e}", self.root.display()))?;
        // Creating the folder is the claim on the id.
        std::fs::create_dir(&dir).map_err(|e| match e.kind() {
            std::io::ErrorKind::AlreadyExists => format!("job {} exists already", id_of(&record)),
            _ => format!("cannot create {}: {e}", dir.display()),
        })?;
        self.write(&record)?;
        Ok(record)
    }

    /// Marks a job that is not finished `cancelled` and asks it to stop; the runner ends it.
    /// True if it was not finished.
    pub fn cancel(&self, id: &str, now: &str) -> Result<bool, String> {
        let mut changed = false;
        self.update(id, |record| {
            if !is_final(status_of(record)) {
                record.set("status", Value::str("cancelled"));
                record.set("finished", Value::str(now));
                changed = true;
            }
        })?;
        if changed {
            let _ = std::fs::write(self.job_dir(id)?.join(JOB_STOP_FILE), b"cancelled\n");
        }
        Ok(changed)
    }

    /// The sender has the result: start the shorter keep. Only a finished job; the first time wins.
    pub fn mark_fetched(&self, id: &str, now: &str) -> Result<bool, String> {
        let mut changed = false;
        self.update(id, |record| {
            if is_final(status_of(record)) {
                if record.get("fetched").is_none_or(Value::is_null) {
                    record.set("fetched", Value::str(now));
                }
                changed = true;
            }
        })?;
        Ok(changed)
    }

    /// A finished job whose keep is over, by its record (`fetched` + a day, `finished` + 14 days).
    pub fn expired(record: &Value, now_unix: u64) -> bool {
        if !is_final(status_of(record)) {
            return false;
        }
        let at = |key: &str| record.get(key).and_then(Value::as_str).and_then(parse_utc_timestamp);
        match (at("fetched"), at("finished")) {
            (Some(fetched), _) => now_unix >= fetched + KEEP_AFTER_FETCH_S,
            (None, Some(finished)) => now_unix >= finished + KEEP_UNFETCHED_S,
            (None, None) => false,
        }
    }

    /// Ends every job left `running` by a runner that is gone (the node restarted, the service
    /// was stopped): its process died with the runner's Job Object, so nothing will finish it.
    /// Returns the ids it settled. Only the runner calls this, at its start.
    pub fn settle_orphans(&self, now: &str) -> Vec<String> {
        let mut settled = Vec::new();
        for id in self.ids() {
            let _ = self.update(&id, |record| {
                if status_of(record) == "running" {
                    record.set("status", Value::str("failed"));
                    record.set("finished", Value::str(now));
                    record.set(
                        "note",
                        Value::str("the job was stopped without a result: the node or its service was restarted"),
                    );
                    settled.push(id.clone());
                }
            });
        }
        settled
    }
}

/// A lock held while a folder exists. A lock older than a minute is from a dead process and is
/// taken over.
pub struct DirLock(PathBuf);

impl DirLock {
    pub fn take(path: &Path) -> Result<DirLock, String> {
        for _ in 0..500 {
            match std::fs::create_dir(path) {
                Ok(()) => return Ok(DirLock(path.to_path_buf())),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let old = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > Duration::from_secs(60));
                    if old {
                        let _ = std::fs::remove_dir(path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(format!("cannot lock {}: {e}", path.display())),
            }
        }
        Err(format!("{} stayed locked", path.display()))
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn request(extra: &[(&str, Value)]) -> Value {
        let mut r = json::parse(&format!(
            r#"{{"id": "20261125-1130-abc", "repo": "demo", "commit": "{COMMIT}", "command": ["sh", "-c", "true"], "memory": "2G"}}"#
        ))
        .unwrap();
        for (k, v) in extra {
            r.set(k, v.clone());
        }
        r
    }

    fn node() -> NodePolicy {
        NodePolicy { airgap: "off".into(), can_airgap: true }
    }

    #[test]
    fn sizes_and_durations_follow_night_shift() {
        assert_eq!(parse_size("8G").unwrap(), 8 << 30);
        assert_eq!(parse_size(" 512 MiB ").unwrap(), 512 << 20);
        assert_eq!(parse_size("1.5g").unwrap(), 3 << 29);
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("2kb").unwrap(), 2048);
        assert!(parse_size("lots").is_err() && parse_size("5X").is_err());
        assert_eq!(parse_duration("90m").unwrap(), 5400);
        assert_eq!(parse_duration("2h").unwrap(), 7200);
        assert_eq!(parse_duration("45s").unwrap(), 45);
        assert_eq!(parse_duration("10").unwrap(), 600);
        assert!(parse_duration("soon").is_err());
    }

    #[test]
    fn ids_slugs_and_airgap() {
        assert!(is_job_id("20261125-1130-abc"));
        for bad in ["20261125-1130-abg", "2026112-1130-abc", "../../etc", "20261125-1130-abcd"] {
            assert!(!is_job_id(bad), "{bad}");
        }
        assert!(is_slug("my-repo.v2") && !is_slug(".hidden") && !is_slug("a..b") && !is_slug("a/b"));
        assert_eq!(stricter("off", "on"), "on");
        assert_eq!(stricter("off", "off"), "off");
        assert_eq!(stricter("weird", "off"), "on");
    }

    #[test]
    fn a_good_request_becomes_a_queued_record() {
        let r = validate(&request(&[]), &node(), "2026-11-25T11:30:00Z").unwrap();
        assert_eq!(status_of(&r), "queued");
        assert_eq!(r.get("memory_bytes").and_then(Value::as_u64), Some(2 << 30));
        assert_eq!(r.get("time_s").and_then(Value::as_u64), Some(DEFAULT_TIME_S), "time defaults to an hour");
        assert_eq!(lane_of(&r), "default");
        assert_eq!(r.get("author").unwrap().get("name").unwrap().as_str(), Some("Ragon Job"));
        assert!(recheck(&r).is_ok());
        let r = validate(&request(&[("time", Value::str("2h")), ("lane", Value::str("cpu"))]), &node(), "t").unwrap();
        assert_eq!(r.get("time_s").and_then(Value::as_u64), Some(7200));
        assert_eq!(lane_of(&r), "cpu");
    }

    #[test]
    fn bad_requests_are_refused_with_the_reason() {
        let cases: Vec<(Vec<(&str, Value)>, &str)> = vec![
            (vec![("id", Value::str("../x"))], "not a job id"),
            (vec![("repo", Value::str("../etc"))], "not a repository name"),
            (vec![("commit", Value::str("main"))], "full hash"),
            (vec![("command", Value::Array(vec![]))], "has a command"),
            (vec![("command", Value::str("ls"))], "has a command"),
            (vec![("memory", Value::Null)], "memory cap"),
            (vec![("memory", Value::str("64G"))], "at most 32G"),
            (vec![("time", Value::str("9h"))], "at most 8h"),
            (vec![("time", Value::str("0s"))], "time limit"),
            (vec![("gpu", Value::Bool(true))], "GPU"),
            (vec![("test", Value::num(3))], "test command"),
            (vec![("setup", Value::str("  "))], "setup command"),
            (vec![("out", Value::str("../up"))], "--out"),
            (vec![("binds", Value::Array(vec![Value::str("/data")]))], "binding"),
            (vec![("lane", Value::str("a/b"))], "lane"),
        ];
        for (extra, expected) in cases {
            let err = validate(&request(&extra), &node(), "t").unwrap_err();
            assert!(err.contains(expected), "{extra:?}: {err}");
        }
        let windows = NodePolicy { airgap: "off".into(), can_airgap: false };
        let err = validate(&request(&[("airgapped", Value::str("on"))]), &windows, "t").unwrap_err();
        assert!(err.contains("network"), "{err}");
        let airgapped_node = NodePolicy { airgap: "on".into(), can_airgap: true };
        let r = validate(&request(&[]), &airgapped_node, "t").unwrap();
        assert_eq!(r.get("airgapped").and_then(Value::as_str), Some("on"));
    }

    #[test]
    fn a_tampered_record_fails_the_recheck() {
        let mut r = validate(&request(&[]), &node(), "t").unwrap();
        r.set("memory_bytes", Value::num(1e15));
        assert!(recheck(&r).unwrap_err().contains("memory_bytes"));
        let mut r = validate(&request(&[]), &node(), "t").unwrap();
        r.set("repo", Value::str("../../x"));
        assert!(recheck(&r).is_err());
    }

    fn rec(id: &str, lane: &str, status: &str, submitted: &str) -> Value {
        Value::Object(vec![
            ("id".into(), Value::str(id)),
            ("lane".into(), Value::str(lane)),
            ("status".into(), Value::str(status)),
            ("submitted".into(), Value::str(submitted)),
        ])
    }

    #[test]
    fn one_job_runs_per_lane_in_submission_order() {
        let records = vec![
            rec("20261125-1130-003", "a", "queued", "2026-11-25T11:30:03Z"),
            rec("20261125-1130-001", "a", "queued", "2026-11-25T11:30:01Z"),
            rec("20261125-1130-002", "b", "queued", "2026-11-25T11:30:02Z"),
            rec("20261125-1130-004", "c", "running", "2026-11-25T11:30:00Z"),
            rec("20261125-1130-005", "c", "queued", "2026-11-25T11:30:00Z"),
            rec("20261125-1130-006", "d", "done", "2026-11-25T11:30:00Z"),
        ];
        let ids = |v: Vec<&Value>| v.into_iter().map(|r| id_of(r).to_string()).collect::<Vec<_>>();
        assert_eq!(ids(next_to_start(&records, &[])), ["20261125-1130-001", "20261125-1130-002"]);
        assert_eq!(ids(next_to_start(&records, &["b".to_string()])), ["20261125-1130-001"]);
    }

    #[test]
    fn the_watch_asks_then_kills() {
        let s = Duration::from_secs;
        let mut w = Watch::new(s(60), s(10));
        assert_eq!(w.tick(s(59), false, false), Action::Run);
        assert_eq!(w.due_in(s(59)), s(1));
        assert_eq!(w.tick(s(60), false, false), Action::SoftStop(Reason::TimeLimit));
        assert_eq!(w.tick(s(65), true, true), Action::Run, "the first reason wins");
        assert_eq!(w.due_in(s(65)), s(5));
        assert_eq!(w.tick(s(70), false, false), Action::Kill(Reason::TimeLimit));
        assert_eq!(w.tick(s(80), false, false), Action::Run, "killed once");

        let mut w = Watch::new(s(60), s(10));
        assert_eq!(w.tick(s(1), true, false), Action::SoftStop(Reason::MemoryCap));
        let mut w = Watch::new(s(60), s(10));
        assert_eq!(w.tick(s(1), true, true), Action::SoftStop(Reason::Cancelled));
        let mut w = Watch::new(s(60), Duration::ZERO);
        assert_eq!(w.tick(s(61), false, false), Action::Kill(Reason::TimeLimit));
    }

    #[test]
    fn timestamps_round_trip_and_keep_rules() {
        for t in [0u64, 1_764_070_200, 951_782_400, 4_102_444_799] {
            assert_eq!(parse_utc_timestamp(&crate::core::utc_timestamp(t)), Some(t));
        }
        assert_eq!(parse_utc_timestamp("2026-11-25T11:30:00+00:00"), parse_utc_timestamp("2026-11-25T11:30:00Z"));
        assert_eq!(parse_utc_timestamp("2026-11-25T11:30:00+01:00"), None);
        let base = parse_utc_timestamp("2026-11-25T00:00:00Z").unwrap();
        let mut r = rec("20261125-1130-001", "a", "done", "x");
        r.set("finished", Value::str("2026-11-25T00:00:00Z"));
        assert!(!Store::expired(&r, base + KEEP_UNFETCHED_S - 1));
        assert!(Store::expired(&r, base + KEEP_UNFETCHED_S));
        r.set("fetched", Value::str("2026-11-25T00:00:00Z"));
        assert!(Store::expired(&r, base + KEEP_AFTER_FETCH_S));
        r.set("status", Value::str("running"));
        assert!(!Store::expired(&r, base + KEEP_UNFETCHED_S));
    }

    fn temp_store(name: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("ragon-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Store::new(dir)
    }

    #[test]
    fn the_store_submits_cancels_fetches_and_settles() {
        let store = temp_store("store");
        let now = "2026-11-25T11:30:00Z";
        let r = store.submit(&request(&[]), &node(), now).unwrap();
        let id = id_of(&r).to_string();
        assert!(store.submit(&request(&[]), &node(), now).unwrap_err().contains("exists"));
        assert_eq!(store.ids(), std::slice::from_ref(&id));
        assert!(!store.mark_fetched(&id, now).unwrap(), "not finished yet");
        assert!(store.cancel(&id, now).unwrap());
        assert!(store.job_dir(&id).unwrap().join(JOB_STOP_FILE).exists());
        assert!(!store.cancel(&id, now).unwrap(), "cancelled once");
        assert_eq!(status_of(&store.read(&id).unwrap()), "cancelled");
        assert!(store.mark_fetched(&id, now).unwrap());
        assert!(store.mark_fetched(&id, "2026-11-26T00:00:00Z").unwrap());
        assert_eq!(store.read(&id).unwrap().get("fetched").and_then(Value::as_str), Some(now), "the first fetch wins");

        let mut other = request(&[("id", Value::str("20261125-1131-abc"))]);
        other.set("lane", Value::str("x"));
        store.submit(&other, &node(), now).unwrap();
        store.update("20261125-1131-abc", |r| r.set("status", Value::str("running"))).unwrap();
        assert_eq!(store.settle_orphans(now), ["20261125-1131-abc"]);
        let settled = store.read("20261125-1131-abc").unwrap();
        assert_eq!(status_of(&settled), "failed");
        assert!(settled.get("note").and_then(Value::as_str).unwrap().contains("restarted"));
        let _ = std::fs::remove_dir_all(&store.root);
    }
}
