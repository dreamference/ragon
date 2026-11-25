//! Jobs on a Mac node (specs/DREAMFERENCE_MAC_NODE.md §4.5), modelled on
//! dreamference/node/node_job.py: `serve-job` (src/node_job_macos.rs in the launcher) hands a job to
//! the node's supervisor over a local socket, and the supervisor runs it
//!
//! - in its own process group, as a child of the supervisor and not of the sender's SSH session,
//!   so a dropped session does not stop it;
//! - under a memory cap: the job's physical footprint (src/footprint.rs) is sampled once a second
//!   and the job is stopped over the cap;
//! - under a time limit;
//! - inside a Seatbelt profile generated for it (src/seatbelt.rs) on macOS.
//!
//! Stopping a job (cap, limit or `cancel`) is `SIGTERM` to the group, then `SIGKILL` after a
//! grace period. `fetched` and `cancel` behave as on Linux: `fetched` marks a finished job so its
//! shorter keep starts, `cancel` marks a running job `cancelled` and stops it.
//!
//! The socket speaks one request line and one answer: words separated by spaces, each
//! percent-encoded ([`encode`]); the answer is `ok` or `error <reason>` followed by `key: value`
//! lines ([`crate::core::render_status`]). Only connections from the job user's uid are served
//! (peer credentials), whatever the socket file's mode.
//!
//! The job's record is `record.txt` in its folder, next to `output.log`, the profile
//! `sandbox.sb`, its `tmp/` and its `tree/`.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::{parse_status, render_status, utc_timestamp};
use crate::footprint::{signal_job, Footprint, SIGKILL, SIGTERM};
use crate::seatbelt;

/// The most a sender may ask for: this node's ceilings (node_job.py's).
pub const MAX_MEMORY_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub const MAX_TIME_S: u64 = 8 * 3600;
/// The time limit when a request names none (the brief's default for the Mac node).
pub const DEFAULT_TIME_S: u64 = 3600;

pub const STATUS_QUEUED: &str = "queued";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_DONE: &str = "done";
pub const STATUS_FAILED: &str = "failed";
pub const STATUS_CANCELLED: &str = "cancelled";
pub const FINAL_STATUSES: &[&str] = &[STATUS_DONE, STATUS_FAILED, "refused", STATUS_CANCELLED];

/// What a job runs with when it is not sandboxed by the caller's PATH.
const JOB_PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin";

/// Refusal when the Command Line Tools are missing: macOS's `/usr/bin/git` would pop a dialog in
/// front of nobody.
pub const NO_COMMAND_LINE_TOOLS: &str = "the Command Line Tools are missing, so this node has no git \
     (`/usr/bin/git` is a stub that opens an install dialog): run `xcode-select --install` on the node, \
     then send the job again";

// -- encoding ------------------------------------------------------------------------------------

/// Percent-encodes everything but a safe set, so a word never contains a space or a new line.
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"._/-=:,+@".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The inverse of [`encode`]; malformed escapes are kept as they are.
pub fn decode(word: &str) -> String {
    let bytes = word.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A job id as node_job.py's `JOB_ID`: `\d{8}-\d{4}-[0-9a-f]{3}`.
pub fn valid_job_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 17
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..13].iter().all(u8::is_ascii_digit)
        && b[13] == b'-'
        && b[14..].iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
}

/// `8G`, `512M`, `1048576` (bytes) as node_job.py's sizes.
pub fn parse_size(text: &str) -> Option<u64> {
    let t = text.trim();
    let (num, mult) = match t.chars().last()? {
        'K' | 'k' => (&t[..t.len() - 1], 1u64 << 10),
        'M' | 'm' => (&t[..t.len() - 1], 1 << 20),
        'G' | 'g' => (&t[..t.len() - 1], 1 << 30),
        'T' | 't' => (&t[..t.len() - 1], 1 << 40),
        _ => (t, 1),
    };
    num.parse::<u64>().ok()?.checked_mul(mult)
}

/// `90m`, `2h`, `45s`, `3600` (seconds).
pub fn parse_duration(text: &str) -> Option<u64> {
    let t = text.trim();
    let (num, mult) = match t.chars().last()? {
        's' => (&t[..t.len() - 1], 1u64),
        'm' => (&t[..t.len() - 1], 60),
        'h' => (&t[..t.len() - 1], 3600),
        _ => (t, 1),
    };
    num.parse::<u64>().ok()?.checked_mul(mult)
}

// -- the job -------------------------------------------------------------------------------------

/// A job as the supervisor runs it: already validated by [`JobSpec::from_words`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSpec {
    pub id: String,
    pub command: Vec<String>,
    pub memory_bytes: u64,
    pub time_s: u64,
    /// False at `/airgapped on`.
    pub network: bool,
    /// A bare git repository and a full commit hash: the job runs in a worktree of it. Without
    /// them the job runs in an empty `tree/`.
    pub repo: Option<PathBuf>,
    pub commit: Option<String>,
}

impl JobSpec {
    /// The words of a `submit` request after the operation: `id=`, `memory=`, `time=`,
    /// `airgapped=`, `repo=`, `commit=` and one `arg=` per word of the command.
    pub fn from_words(words: &[String]) -> Result<Self, String> {
        let mut id = None;
        let mut memory = None;
        let mut time = None;
        let mut airgapped = "off".to_string();
        let mut repo = None;
        let mut commit = None;
        let mut command = Vec::new();
        for word in words {
            let (key, value) = word.split_once('=').ok_or_else(|| format!("not a key=value word: {word}"))?;
            let value = decode(value);
            match key {
                "id" => id = Some(value),
                "memory" => memory = Some(value),
                "time" => time = Some(value),
                "airgapped" => airgapped = value,
                "repo" => repo = Some(PathBuf::from(value)),
                "commit" => commit = Some(value),
                "arg" => command.push(value),
                "gpu" if value != "0" && !value.is_empty() => {
                    return Err("GPU jobs are not allowed on a node: a footprint cap does not bound GPU memory, which is the pool the model server uses".into());
                }
                "gpu" => {}
                _ => return Err(format!("unknown field {key}")),
            }
        }
        let id = id.filter(|i| valid_job_id(i)).ok_or("a job has an id such as 20261125-1200-abc")?;
        if command.is_empty() || command.iter().any(String::is_empty) {
            return Err("a job has a command".into());
        }
        // The cap comes from the request: there is no default a sender can omit it into.
        let memory_bytes = memory
            .as_deref()
            .and_then(parse_size)
            .filter(|m| *m > 0)
            .ok_or("a job needs a memory cap")?;
        let time_s = match time.as_deref() {
            None | Some("") => DEFAULT_TIME_S,
            Some(t) => parse_duration(t).filter(|s| *s > 0).ok_or("a job's time limit is a duration such as 90m")?,
        };
        if memory_bytes > MAX_MEMORY_BYTES {
            return Err(format!("this node allows a job at most {}G of memory", MAX_MEMORY_BYTES >> 30));
        }
        if time_s > MAX_TIME_S {
            return Err(format!("this node allows a job at most {}h", MAX_TIME_S / 3600));
        }
        // An unknown level counts as `on`, the stricter.
        let network = airgapped == "off";
        if repo.is_some() != commit.is_some() {
            return Err("a job names both its repository and its commit, or neither".into());
        }
        if let Some(c) = &commit
            && (c.len() != 40 || !c.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        {
            return Err("a job names the commit it runs at, as a full hash".into());
        }
        Ok(Self { id, command, memory_bytes, time_s, network, repo, commit })
    }

    /// The request words for this spec (the client half of [`JobSpec::from_words`]).
    pub fn to_words(&self) -> Vec<String> {
        let mut w = vec![
            format!("id={}", encode(&self.id)),
            format!("memory={}", self.memory_bytes),
            format!("time={}", self.time_s),
            format!("airgapped={}", if self.network { "off" } else { "on" }),
        ];
        if let (Some(r), Some(c)) = (&self.repo, &self.commit) {
            w.push(format!("repo={}", encode(&r.to_string_lossy())));
            w.push(format!("commit={}", encode(c)));
        }
        w.extend(self.command.iter().map(|a| format!("arg={}", encode(a))));
        w
    }
}

/// How often the runner samples and how long `SIGTERM` gets before `SIGKILL`.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub sample: Duration,
    pub term_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { sample: Duration::from_secs(1), term_grace: Duration::from_secs(10) }
    }
}

/// Why a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    Exited(i32),
    OverMemory { footprint: u64 },
    OverTime,
    Cancelled,
}

/// A job record: ordered `key: value` pairs.
pub type Record = BTreeMap<String, String>;

fn now_text() -> String {
    utc_timestamp(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
}

fn render(record: &Record) -> String {
    let fields: Vec<(&str, String)> = record.iter().map(|(k, v)| (k.as_str(), encode(v))).collect();
    render_status(&fields)
}

fn parse(text: &str) -> Record {
    parse_status(text).into_iter().map(|(k, v)| (k, if v == "none" { String::new() } else { decode(&v) })).collect()
}

/// Checks that git can run without a dialog: `xcode-select -p` must name the developer folder.
pub fn git_prerequisite(xcode_select: &Path) -> Result<(), String> {
    match Command::new(xcode_select).arg("-p").stdin(Stdio::null()).output() {
        Ok(out) if out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty() => Ok(()),
        _ => Err(NO_COMMAND_LINE_TOOLS.to_string()),
    }
}

/// The job runner and its socket server; one per supervisor.
pub struct JobServer {
    /// Where job folders live (`<root>/<id>/`).
    pub root: PathBuf,
    /// The only uid served on the socket; None serves the supervisor's own uid.
    pub job_uid: Option<u32>,
    /// Wrap every job in its Seatbelt profile (macOS).
    pub sandbox: bool,
    /// `xcode-select` to ask before a job that needs git; None skips the check (Linux).
    pub xcode_select: Option<PathBuf>,
    pub limits: Limits,
    pub footprint: Arc<dyn Footprint>,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Serialises record writes.
    records: Mutex<()>,
}

impl JobServer {
    pub fn new(root: PathBuf, footprint: Arc<dyn Footprint>) -> Self {
        Self {
            root,
            job_uid: None,
            sandbox: cfg!(target_os = "macos"),
            xcode_select: cfg!(target_os = "macos").then(|| PathBuf::from("/usr/bin/xcode-select")),
            limits: Limits::default(),
            footprint,
            cancels: Mutex::new(HashMap::new()),
            records: Mutex::new(()),
        }
    }

    pub fn job_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    pub fn read(&self, id: &str) -> Option<Record> {
        if !valid_job_id(id) {
            return None;
        }
        fs::read_to_string(self.job_dir(id).join("record.txt")).ok().map(|t| parse(&t))
    }

    fn write(&self, record: &Record) -> io::Result<()> {
        let dir = self.job_dir(&record["id"]);
        let tmp = dir.join("record.txt.tmp");
        fs::write(&tmp, render(record))?;
        fs::rename(tmp, dir.join("record.txt"))
    }

    /// Changes a record under the lock; returns the new record.
    fn update(&self, id: &str, change: impl FnOnce(&mut Record)) -> Option<Record> {
        let _guard = self.records.lock().unwrap_or_else(|e| e.into_inner());
        let mut record = self.read(id)?;
        change(&mut record);
        self.write(&record).ok()?;
        Some(record)
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = fs::read_dir(&self.root)
            .map(|d| d.filter_map(|e| e.ok()?.file_name().into_string().ok()).filter(|n| valid_job_id(n)).collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    /// After a supervisor restart nothing runs any more: a record still `queued` or `running` is
    /// settled as failed, as node_job.py's `reconcile` does for a unit that is gone.
    pub fn settle_after_restart(&self) {
        for id in self.ids() {
            self.update(&id, |r| {
                if [STATUS_QUEUED, STATUS_RUNNING].contains(&r.get("status").map_or("", String::as_str)) {
                    r.insert("status".into(), STATUS_FAILED.into());
                    r.insert("finished".into(), now_text());
                    r.insert("note".into(), "the job was stopped without a result: the node's supervisor was restarted".into());
                }
            });
        }
    }

    /// Accepts a job, prepares its folder and starts it on a thread of its own.
    pub fn submit(self: &Arc<Self>, spec: JobSpec) -> Result<Record, String> {
        let dir = self.job_dir(&spec.id);
        if dir.exists() {
            return Err(format!("job {} exists already", spec.id));
        }
        if spec.repo.is_some()
            && let Some(xs) = &self.xcode_select
        {
            git_prerequisite(xs)?;
        }
        fs::create_dir_all(dir.join("tmp")).map_err(|e| format!("cannot create the job's folder: {e}"))?;
        let mut record = Record::new();
        for (k, v) in [
            ("id", spec.id.clone()),
            ("status", STATUS_QUEUED.to_string()),
            ("command", spec.command.iter().map(|a| encode(a)).collect::<Vec<_>>().join(" ")),
            ("memory_bytes", spec.memory_bytes.to_string()),
            ("time_s", spec.time_s.to_string()),
            ("airgapped", if spec.network { "off" } else { "on" }.to_string()),
            ("submitted", now_text()),
        ] {
            record.insert(k.into(), v);
        }
        self.write(&record).map_err(|e| format!("cannot write the job's record: {e}"))?;
        let cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().unwrap_or_else(|e| e.into_inner()).insert(spec.id.clone(), cancel.clone());
        let server = Arc::clone(self);
        std::thread::spawn(move || server.execute(spec, &cancel));
        Ok(record)
    }

    /// Runs one job to its end and writes the result into its record.
    pub fn execute(&self, spec: JobSpec, cancel: &AtomicBool) {
        let id = spec.id.clone();
        let outcome = self.run(&spec, cancel);
        self.cancels.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        self.update(&id, |r| {
            r.insert("finished".into(), now_text());
            let already_cancelled = r.get("status").map(String::as_str) == Some(STATUS_CANCELLED);
            match outcome {
                Ok((Ending::Exited(code), peak)) => {
                    r.insert("exit_code".into(), code.to_string());
                    r.insert("peak_footprint".into(), peak.to_string());
                    if !already_cancelled {
                        r.insert("status".into(), if code == 0 { STATUS_DONE } else { STATUS_FAILED }.into());
                    }
                }
                Ok((Ending::OverMemory { footprint }, peak)) => {
                    r.insert("status".into(), STATUS_FAILED.into());
                    r.insert("stopped_by".into(), "memory".into());
                    r.insert("peak_footprint".into(), peak.to_string());
                    r.insert(
                        "note".into(),
                        format!("the job passed its memory cap ({} bytes): footprint {footprint} bytes", spec.memory_bytes),
                    );
                }
                Ok((Ending::OverTime, peak)) => {
                    r.insert("status".into(), STATUS_FAILED.into());
                    r.insert("stopped_by".into(), "time".into());
                    r.insert("peak_footprint".into(), peak.to_string());
                    r.insert("note".into(), format!("the job passed its time limit ({} s)", spec.time_s));
                }
                Ok((Ending::Cancelled, peak)) => {
                    r.insert("status".into(), STATUS_CANCELLED.into());
                    r.insert("stopped_by".into(), "cancel".into());
                    r.insert("peak_footprint".into(), peak.to_string());
                }
                Err(e) => {
                    if !already_cancelled {
                        r.insert("status".into(), STATUS_FAILED.into());
                    }
                    r.insert("note".into(), format!("job runner error: {e}"));
                }
            }
        });
    }

    fn prepare_tree(&self, spec: &JobSpec, dir: &Path) -> Result<PathBuf, String> {
        let tree = dir.join("tree");
        match (&spec.repo, &spec.commit) {
            (Some(repo), Some(commit)) => {
                let out = Command::new("git")
                    .arg("--git-dir")
                    .arg(repo)
                    .args(["worktree", "add", "--detach"])
                    .arg(&tree)
                    .arg(commit)
                    .env("GIT_TERMINAL_PROMPT", "0")
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("git: {e}"))?;
                if !out.status.success() {
                    let err = String::from_utf8_lossy(&out.stderr);
                    let tail: String = err.trim().chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
                    return Err(format!("git worktree add failed: {tail}"));
                }
            }
            _ => fs::create_dir_all(&tree).map_err(|e| e.to_string())?,
        }
        Ok(tree)
    }

    /// Starts the job and watches it; returns how it ended and its peak footprint.
    fn run(&self, spec: &JobSpec, cancel: &AtomicBool) -> Result<(Ending, u64), String> {
        let dir = self.job_dir(&spec.id);
        let tree = self.prepare_tree(spec, &dir)?;
        let tmp = dir.join("tmp");
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/var/empty"));
        let argv = if self.sandbox {
            let real = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
            let profile = seatbelt::profile(&seatbelt::JobSandbox {
                tree: real(&tree),
                tmp: real(&tmp),
                home: real(&home),
                readable: Vec::new(),
                network: spec.network,
            });
            let path = dir.join("sandbox.sb");
            fs::write(&path, profile).map_err(|e| e.to_string())?;
            seatbelt::wrap(&path, &spec.command)
        } else {
            spec.command.clone()
        };
        let log = OpenOptions::new().create(true).append(true).open(dir.join("output.log")).map_err(|e| e.to_string())?;
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .current_dir(&tree)
            .stdin(Stdio::null())
            .stdout(log.try_clone().map_err(|e| e.to_string())?)
            .stderr(log)
            .env_clear()
            .env("PATH", JOB_PATH)
            .env("HOME", &home)
            .env("TMPDIR", &tmp)
            .env("LANG", std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()))
            .env("TERM", "dumb")
            .env("RAGON_JOB", &spec.id)
            .env("GIT_TERMINAL_PROMPT", "0")
            // Its own group: the job is not in the sender's session and the group is what a stop
            // signals.
            .process_group(0);
        let mut child = command.spawn().map_err(|e| format!("cannot start {}: {e}", argv[0]))?;
        let pid = child.id() as i32;
        self.update(&spec.id, |r| {
            if r.get("status").map(String::as_str) == Some(STATUS_QUEUED) {
                r.insert("status".into(), STATUS_RUNNING.into());
            }
            r.insert("started".into(), now_text());
            r.insert("pid".into(), pid.to_string());
        });
        let started = Instant::now();
        let limit = Duration::from_secs(spec.time_s);
        let mut peak = 0u64;
        let mut stopping: Option<(Ending, Instant)> = None;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                // Whatever the leader left behind in its group goes with it.
                let rest = self.footprint.pids(pid, 0);
                signal_job(pid, &rest, SIGKILL);
                let ending = match stopping {
                    Some((ending, _)) => ending,
                    None => Ending::Exited(exit_code(status)),
                };
                return Ok((ending, peak));
            }
            let pids = self.footprint.pids(pid, pid);
            let footprint: u64 = pids.iter().map(|p| self.footprint.bytes(*p)).sum();
            peak = peak.max(footprint);
            match &stopping {
                None => {
                    let reason = if cancel.load(Ordering::SeqCst) {
                        Some(Ending::Cancelled)
                    } else if footprint > spec.memory_bytes {
                        Some(Ending::OverMemory { footprint })
                    } else if started.elapsed() >= limit {
                        Some(Ending::OverTime)
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        signal_job(pid, &pids, SIGTERM);
                        stopping = Some((reason, Instant::now()));
                    }
                }
                Some((_, since)) if since.elapsed() >= self.limits.term_grace => {
                    signal_job(pid, &pids, SIGKILL);
                }
                Some(_) => {}
            }
            let step = if stopping.is_some() { Duration::from_millis(50) } else { self.limits.sample };
            std::thread::sleep(step.min(limit.saturating_sub(started.elapsed()).max(Duration::from_millis(10))));
        }
    }

    /// Marks a running job cancelled and asks its runner to stop it.
    pub fn cancel(&self, id: &str) -> bool {
        let Some(flag) = self.cancels.lock().unwrap_or_else(|e| e.into_inner()).get(id).cloned() else {
            return false;
        };
        let marked = self.update(id, |r| {
            if !FINAL_STATUSES.contains(&r.get("status").map_or("", String::as_str)) {
                r.insert("status".into(), STATUS_CANCELLED.into());
            }
        });
        flag.store(true, Ordering::SeqCst);
        marked.is_some()
    }

    /// Records that the sender has a finished job's result.
    pub fn mark_fetched(&self, id: &str) -> bool {
        let mut finished = false;
        self.update(id, |r| {
            if FINAL_STATUSES.contains(&r.get("status").map_or("", String::as_str)) {
                finished = true;
                r.entry("fetched".into()).or_insert_with(now_text);
            }
        });
        finished
    }

    /// The job's output from `offset` on.
    pub fn logs(&self, id: &str, offset: u64) -> io::Result<Vec<u8>> {
        let mut file = fs::File::open(self.job_dir(id).join("output.log"))?;
        file.seek(SeekFrom::Start(offset))?;
        let mut out = Vec::new();
        file.take(1 << 20).read_to_end(&mut out)?;
        Ok(out)
    }

    /// Answers one request line (without its new line).
    pub fn handle(self: &Arc<Self>, line: &str) -> Vec<u8> {
        let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        let Some((op, rest)) = words.split_first() else { return answer_error("empty request") };
        let id = rest.first().map(|w| decode(w)).unwrap_or_default();
        let known = || self.read(&id).is_some();
        match op.as_str() {
            "submit" => match JobSpec::from_words(rest).and_then(|spec| self.submit(spec)) {
                Ok(record) => answer_ok(&record),
                Err(e) => answer_error(&e),
            },
            "status" if known() => answer_ok(&self.read(&id).unwrap_or_default()),
            "cancel" if known() => {
                if self.cancel(&id) {
                    answer_ok(&self.read(&id).unwrap_or_default())
                } else {
                    answer_error("the job is not running")
                }
            }
            "fetched" if known() => {
                if self.mark_fetched(&id) {
                    answer_ok(&self.read(&id).unwrap_or_default())
                } else {
                    answer_error("the job has not finished")
                }
            }
            "logs" if known() => {
                let offset = rest.get(1).and_then(|o| o.parse().ok()).unwrap_or(0);
                match self.logs(&id, offset) {
                    Ok(bytes) => {
                        let mut out = format!("ok\nsize: {}\n\n", bytes.len()).into_bytes();
                        out.extend(bytes);
                        out
                    }
                    Err(e) => answer_error(&e.to_string()),
                }
            }
            "list" => {
                let mut out = b"ok\n".to_vec();
                for id in self.ids() {
                    if let Some(r) = self.read(&id) {
                        out.extend(format!("{}: {}\n", id, r.get("status").cloned().unwrap_or_default()).into_bytes());
                    }
                }
                out
            }
            "status" | "cancel" | "fetched" | "logs" => answer_error("no such job"),
            _ => answer_error("the job socket takes submit, status, logs, list, cancel and fetched"),
        }
    }

    /// Serves the socket until `stop` is set; one thread per connection.
    pub fn serve(self: &Arc<Self>, socket: &Path, stop: &AtomicBool) -> io::Result<()> {
        let _ = fs::remove_file(socket);
        let listener = UnixListener::bind(socket)?;
        use std::os::unix::fs::PermissionsExt;
        // Owner and group only; the peer check below is what admits the job user.
        fs::set_permissions(socket, fs::Permissions::from_mode(0o660))?;
        listener.set_nonblocking(true)?;
        let allowed = self.job_uid.unwrap_or_else(current_uid);
        while !stop.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let server = Arc::clone(self);
                    std::thread::spawn(move || {
                        let _ = server.serve_one(stream, allowed);
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(e),
            }
        }
        let _ = fs::remove_file(socket);
        Ok(())
    }

    fn serve_one(self: &Arc<Self>, mut stream: UnixStream, allowed: u32) -> io::Result<()> {
        stream.set_nonblocking(false)?;
        if peer_uid(&stream) != Some(allowed) {
            return stream.write_all(&answer_error("this socket serves the node's job user only"));
        }
        let mut line = String::new();
        BufReader::new(stream.try_clone()?).take(1 << 20).read_line(&mut line)?;
        stream.write_all(&self.handle(line.trim_end()))
    }
}

fn exit_code(status: std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0))
}

fn answer_ok(record: &Record) -> Vec<u8> {
    let mut out = b"ok\n".to_vec();
    out.extend(render(record).into_bytes());
    out
}

fn answer_error(reason: &str) -> Vec<u8> {
    format!("error {}\n", encode(reason)).into_bytes()
}

/// Sends one request and reads the whole answer.
pub fn request(socket: &Path, words: &[String]) -> io::Result<Vec<u8>> {
    let mut stream = UnixStream::connect(socket)?;
    stream.write_all(words.join(" ").as_bytes())?;
    stream.write_all(b"\n")?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut out = Vec::new();
    stream.read_to_end(&mut out)?;
    Ok(out)
}

/// Splits an answer: Ok(record) for `ok`, Err(reason) for `error`.
pub fn parse_answer(answer: &[u8]) -> Result<Record, String> {
    let text = String::from_utf8_lossy(answer);
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    match first.split_once(' ') {
        Some(("error", reason)) => Err(decode(reason)),
        _ if first == "ok" => Ok(parse(rest)),
        _ => Err(format!("unexpected answer: {first}")),
    }
}

// -- command line ---------------------------------------------------------------------------------

const JOBS_USAGE: &str = "usage: ragon-supervisor jobs serve --root DIR --socket PATH [--job-uid UID] [--no-sandbox] \
     [--xcode-select PATH] [--sample-ms N] [--grace-ms N]\n       ragon-supervisor jobs <submit|status|logs|list|cancel|fetched> --socket PATH [WORD ...]";

/// `ragon-supervisor jobs ...`: the runner (`serve`) and a client for the drills.
pub fn cli(args: &[String]) -> i32 {
    let Some((op, rest)) = args.split_first() else {
        eprintln!("{JOBS_USAGE}");
        return 2;
    };
    let mut root = None;
    let mut socket = None;
    let mut job_uid = None;
    let mut sandbox = None;
    let mut xcode_select = None;
    let mut limits = Limits::default();
    let mut words = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        let parsed: Result<(), String> = match a.as_str() {
            "--root" => value().map(|v| root = Some(PathBuf::from(v))),
            "--socket" => value().map(|v| socket = Some(PathBuf::from(v))),
            "--job-uid" => value().and_then(|v| v.parse().map(|u| job_uid = Some(u)).map_err(|_| "--job-uid takes a number".into())),
            "--xcode-select" => value().map(|v| xcode_select = Some(PathBuf::from(v))),
            "--no-sandbox" => {
                sandbox = Some(false);
                Ok(())
            }
            "--sample-ms" => value().and_then(|v| v.parse().map(|n| limits.sample = Duration::from_millis(n)).map_err(|_| "--sample-ms takes a number".into())),
            "--grace-ms" => value().and_then(|v| v.parse().map(|n| limits.term_grace = Duration::from_millis(n)).map_err(|_| "--grace-ms takes a number".into())),
            _ => {
                // key=value words carry their value encoded; a bare word (a job id) as it is.
                words.push(match a.split_once('=') {
                    Some((k, v)) => format!("{k}={}", encode(v)),
                    None => encode(a),
                });
                Ok(())
            }
        };
        if let Err(e) = parsed {
            eprintln!("{e}\n{JOBS_USAGE}");
            return 2;
        }
    }
    let Some(socket) = socket else {
        eprintln!("{JOBS_USAGE}");
        return 2;
    };
    if op == "serve" {
        let Some(root) = root else {
            eprintln!("{JOBS_USAGE}");
            return 2;
        };
        if let Err(e) = fs::create_dir_all(&root) {
            eprintln!("cannot create {}: {e}", root.display());
            return 1;
        }
        let mut server = JobServer::new(root, Arc::new(crate::footprint::SystemFootprint));
        server.job_uid = job_uid;
        server.limits = limits;
        if let Some(s) = sandbox {
            server.sandbox = s;
        }
        if xcode_select.is_some() {
            server.xcode_select = xcode_select;
        }
        let server = Arc::new(server);
        server.settle_after_restart();
        let stop = AtomicBool::new(false);
        return match server.serve(&socket, &stop) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("job socket {}: {e}", socket.display());
                1
            }
        };
    }
    let mut request_words = vec![op.clone()];
    request_words.extend(words);
    match request(&socket, &request_words) {
        Ok(answer) => {
            let _ = io::stdout().write_all(&answer);
            if answer.starts_with(b"ok") { 0 } else { 1 }
        }
        Err(e) => {
            eprintln!("job socket {}: {e}", socket.display());
            1
        }
    }
}

unsafe extern "C" {
    fn getuid() -> u32;
}

pub fn current_uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { getuid() }
}

#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn getpeereid(fd: i32, uid: *mut u32, gid: *mut u32) -> i32;
    }
    let (mut uid, mut gid) = (u32::MAX, u32::MAX);
    // SAFETY: valid socket descriptor and out-pointers.
    (unsafe { getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } == 0).then_some(uid)
}

#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct Ucred {
        pid: i32,
        uid: u32,
        gid: u32,
    }
    unsafe extern "C" {
        fn getsockopt(fd: i32, level: i32, name: i32, value: *mut std::ffi::c_void, len: *mut u32) -> i32;
    }
    const SOL_SOCKET: i32 = 1;
    const SO_PEERCRED: i32 = 17;
    let mut cred = Ucred { pid: 0, uid: u32::MAX, gid: u32::MAX };
    let mut len = std::mem::size_of::<Ucred>() as u32;
    // SAFETY: valid descriptor, out-pointer sized as stated.
    let rc = unsafe { getsockopt(stream.as_raw_fd(), SOL_SOCKET, SO_PEERCRED, (&mut cred as *mut Ucred).cast(), &mut len) };
    (rc == 0).then_some(cred.uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(ws: &[&str]) -> Vec<String> {
        ws.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn encoding_round_trips_spaces_new_lines_and_percent() {
        for text in ["plain", "two words", "a\nb", "100%", "ünï", "%zz", "%4"] {
            let e = encode(text);
            assert!(!e.contains(' ') && !e.contains('\n'), "{e}");
            assert_eq!(decode(&e), text);
        }
        assert_eq!(decode("%zz"), "%zz");
    }

    #[test]
    fn job_ids_are_node_job_pys() {
        assert!(valid_job_id("20261125-1200-abc"));
        for bad in ["20261125-1200-ABC", "2026112-1200-abc", "../../etc", "20261125-1200-abcd", ""] {
            assert!(!valid_job_id(bad), "{bad}");
        }
    }

    #[test]
    fn a_request_without_a_cap_is_refused_and_the_time_limit_defaults_to_an_hour() {
        let base = ["id=20261125-1200-abc", "arg=make"];
        assert_eq!(JobSpec::from_words(&words(&base)).unwrap_err(), "a job needs a memory cap");
        let spec = JobSpec::from_words(&words(&[base[0], base[1], "memory=8G"])).unwrap();
        assert_eq!(spec.memory_bytes, 8 << 30);
        assert_eq!(spec.time_s, DEFAULT_TIME_S);
        assert!(spec.network);
    }

    #[test]
    fn ceilings_gpu_and_airgap_are_the_nodes() {
        let ok = |extra: &str| {
            JobSpec::from_words(&words(&["id=20261125-1200-abc", "arg=x", "memory=1G", extra]))
        };
        assert!(ok("memory=33G").unwrap_err().contains("at most 32G"));
        assert!(ok("time=9h").unwrap_err().contains("at most 8h"));
        assert!(ok("gpu=1").unwrap_err().contains("GPU jobs are not allowed"));
        assert!(!ok("airgapped=on").unwrap().network);
        assert!(!ok("airgapped=bogus").unwrap().network, "an unknown level counts as on");
        assert!(ok("repo=/r").unwrap_err().contains("both"));
        assert_eq!(ok("time=90m").unwrap().time_s, 5400);
    }

    #[test]
    fn a_spec_survives_its_words() {
        let spec = JobSpec {
            id: "20261125-1200-abc".into(),
            command: vec!["sh".into(), "-c".into(), "echo 'a b'; exit 3".into()],
            memory_bytes: 123 << 20,
            time_s: 77,
            network: false,
            repo: Some("/srv/jobs/x.git".into()),
            commit: Some("0123456789abcdef0123456789abcdef01234567".into()),
        };
        assert_eq!(JobSpec::from_words(&spec.to_words()).unwrap(), spec);
    }

    #[test]
    fn missing_command_line_tools_refuse_with_the_command_to_run() {
        let err = git_prerequisite(Path::new("/nonexistent/xcode-select")).unwrap_err();
        assert!(err.contains("xcode-select --install"), "{err}");
        assert!(git_prerequisite(Path::new("/bin/false")).is_err());
        // `echo -p` prints a path and succeeds, as xcode-select -p does when the tools are there.
        assert!(git_prerequisite(Path::new("/bin/echo")).is_ok());
    }

    #[test]
    fn sizes_and_durations() {
        assert_eq!(parse_size("512M"), Some(512 << 20));
        assert_eq!(parse_size("1048576"), Some(1 << 20));
        assert_eq!(parse_size("x"), None);
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("45s"), Some(45));
    }

    #[test]
    fn answers_parse() {
        let mut r = Record::new();
        r.insert("note".into(), "two words\nand a line".into());
        r.insert("id".into(), "20261125-1200-abc".into());
        assert_eq!(parse_answer(&answer_ok(&r)).unwrap(), r);
        assert_eq!(parse_answer(&answer_error("no such job")).unwrap_err(), "no such job");
    }
}
