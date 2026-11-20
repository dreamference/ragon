//! Ragon Digest inside Ragon (specs/DREAMFERENCE_RAGON_OUTPUT_PREPROCESSING.md, answers 7-101 of
//! the owner's round): when a shell command's output is long, the model is given a digest of at
//! most 60 lines and the full output is kept in a file it reads by line numbers.
//!
//! Codex's core calls [`inside`] on the output of every `exec_command` and `write_stdin` (patch
//! `0043`). Nothing happens until the launcher calls [`configure`], which it does only when
//! `[digest] inside` or `DREAMFERENCE_DIGEST_INSIDE` switches the feature on; until then the output
//! is handed back as it came, the same bytes in the same buffer.
//!
//! Stage A, here, uses no model: the selection is [`select::digest`]. The raw outputs live in
//! `$CODEX_HOME/digest/raw/` for a day, within a size cap. With [`Options::last_good`] the lines
//! the last successful run of the same command printed too are left out ([`good`]).

pub mod command;
pub mod good;
pub mod select;
pub mod text;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Outputs shorter than this many lines reach the model as before (answer 7).
pub const MIN_LINES: usize = 200;

/// How long a raw output is kept (answer 12).
pub const KEEP_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// The most the raw outputs may take together; the oldest go first (answer 12).
pub const CAP_BYTES: u64 = 256 * 1024 * 1024;

/// A binary output at least this large is replaced by a note (answer 81).
const BINARY_MIN_BYTES: usize = 4096;

/// Raw reads in a row after which a command kind is no longer digested this session (answer 50).
pub const READS_TO_SWITCH_OFF: u32 = 3;

/// How often the raw folder is pruned while a session runs.
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);

static DIGESTER: OnceLock<Digester> = OnceLock::new();

/// The settings of one process's digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Outputs shorter than this many lines are handed back as they are.
    pub min_lines: usize,
    /// Leave out the lines the last successful run of the same command printed too (owner's
    /// answer 2 of 2026-11-20). Off by default.
    pub last_good: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { min_lines: MIN_LINES, last_good: false }
    }
}

/// Switches the digest on for this process, keeping raw outputs under `codex_home`. Returns false
/// when it was already on.
pub fn configure(codex_home: &Path, min_lines: usize) -> bool {
    configure_with(codex_home, Options { min_lines, ..Options::default() })
}

/// [`configure`] with every setting.
pub fn configure_with(codex_home: &Path, options: Options) -> bool {
    DIGESTER.set(Digester::with_options(codex_home, options)).is_ok()
}

/// Whether [`configure`] has run.
pub fn enabled() -> bool {
    DIGESTER.get().is_some()
}

/// What the model is given for `output`, the bytes `command` printed so far, with `exit_code` set
/// once it has finished. Off, this is `output` itself.
pub async fn inside(command: &str, output: Vec<u8>, exit_code: Option<i32>) -> Vec<u8> {
    match DIGESTER.get() {
        None => output,
        Some(digester) => digester.process(command, output, exit_code),
    }
}

/// The streak of one command kind: how many of its digests in a row were followed by a raw read.
#[derive(Default)]
struct Streak {
    reads: u32,
    last_read: bool,
}

#[derive(Default)]
struct State {
    /// Raw file name to the kind of the command that wrote it.
    kinds: HashMap<String, String>,
    streaks: HashMap<String, Streak>,
    off: HashSet<String>,
    pruned: Option<SystemTime>,
}

/// The digest of one session: where raw outputs go, and the per-kind switch-off.
pub struct Digester {
    dir: PathBuf,
    options: Options,
    state: Mutex<State>,
}

impl Digester {
    /// A digester keeping raw outputs in `codex_home/digest/raw`.
    pub fn new(codex_home: &Path, min_lines: usize) -> Self {
        Self::with_options(codex_home, Options { min_lines, ..Options::default() })
    }

    /// A digester with every setting.
    pub fn with_options(codex_home: &Path, options: Options) -> Self {
        Self { dir: codex_home.join("digest"), options, state: Mutex::new(State::default()) }
    }

    /// The folder the last good runs' samples are kept in.
    pub fn good_dir(&self) -> PathBuf {
        self.dir.join("good")
    }

    /// The folder raw outputs are kept in.
    pub fn raw_dir(&self) -> PathBuf {
        self.dir.join("raw")
    }

    /// The local journal of raw reads and switch-offs (answers 40, 50).
    pub fn journal_path(&self) -> PathBuf {
        self.dir.join("journal.jsonl")
    }

    /// Whether `kind` was switched off for this session.
    pub fn is_off(&self, kind: &str) -> bool {
        self.lock().off.contains(kind)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// What the model is given for `output`. Any failure to keep the raw output hands it back
    /// unchanged.
    pub fn process(&self, command: &str, output: Vec<u8>, exit_code: Option<i32>) -> Vec<u8> {
        if output.is_empty() {
            return output;
        }
        let raw_dir = self.raw_dir();
        if mentions(command, &raw_dir) {
            self.note_raw_read(command);
            return output;
        }
        if command::reads(command) {
            return output;
        }
        let kind = command::kind(command);
        if self.is_off(&kind) {
            return output;
        }
        let binary = output.len() >= BINARY_MIN_BYTES && text::is_binary(&output);
        // A successful run is sampled whatever its length, so a short good run counts too.
        let good = self.options.last_good && !binary && exit_code == Some(0);
        if !binary && (text::line_count(&output) < self.options.min_lines || structured(&output)) {
            if good {
                let _ = good::record(&self.good_dir(), command, &text::lines(&output));
            }
            return output;
        }
        self.prune_now_and_then();
        let Ok(path) = self.keep(&output) else {
            return output;
        };
        self.note_digest(&kind, &path);
        if binary {
            return select::binary_note(output.len(), exit_code, &path).into_bytes();
        }
        let lines = text::lines(&output);
        let known = match self.options.last_good.then(|| good::load(&self.good_dir(), command, SystemTime::now())) {
            Some(Some(samples)) => samples.mark(&lines),
            _ => Vec::new(),
        };
        if good {
            let _ = good::record(&self.good_dir(), command, &lines);
        }
        select::digest_with(&lines, exit_code, &path, &known).into_bytes()
    }

    /// Writes the raw output, named by its content so the same output is one file.
    fn keep(&self, output: &[u8]) -> std::io::Result<PathBuf> {
        let dir = self.raw_dir();
        fs::create_dir_all(&dir)?;
        let mut hasher = DefaultHasher::new();
        output.hash(&mut hasher);
        let path = dir.join(format!("{:016x}.log", hasher.finish()));
        if path.metadata().is_ok_and(|meta| meta.len() == output.len() as u64) {
            fs::File::options().append(true).open(&path)?.set_modified(SystemTime::now())?;
        } else {
            let partial = path.with_extension("part");
            fs::write(&partial, output)?;
            fs::rename(&partial, &path)?;
        }
        Ok(path)
    }

    fn note_digest(&self, kind: &str, path: &Path) {
        let mut state = self.lock();
        if let Some(name) = path.file_name() {
            state.kinds.insert(name.to_string_lossy().into_owned(), kind.to_string());
        }
        let streak = state.streaks.entry(kind.to_string()).or_default();
        if !streak.last_read {
            streak.reads = 0;
        }
        streak.last_read = false;
    }

    fn note_raw_read(&self, command: &str) {
        let mut events = Vec::new();
        {
            let mut state = self.lock();
            let kinds: Vec<(String, String)> = state
                .kinds
                .iter()
                .filter(|(name, _)| command.contains(name.as_str()))
                .map(|(name, kind)| (name.clone(), kind.clone()))
                .collect();
            for (name, kind) in kinds {
                let streak = state.streaks.entry(kind.clone()).or_default();
                if streak.last_read {
                    continue;
                }
                streak.last_read = true;
                streak.reads += 1;
                let reads = streak.reads;
                events.push(format!("\"event\":\"raw_read\",\"kind\":{},\"file\":{}", quote(&kind), quote(&name)));
                if reads >= READS_TO_SWITCH_OFF && state.off.insert(kind.clone()) {
                    events.push(format!("\"event\":\"switched_off\",\"kind\":{},\"reads\":{reads}", quote(&kind)));
                }
            }
        }
        self.journal(&events);
    }

    fn journal(&self, events: &[String]) {
        if events.is_empty() || fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let time = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        if let Ok(mut file) = fs::File::options().create(true).append(true).open(self.journal_path()) {
            for event in events {
                let _ = writeln!(file, "{{\"time\":{time},{event}}}");
            }
        }
    }

    fn prune_now_and_then(&self) {
        let now = SystemTime::now();
        {
            let mut state = self.lock();
            if state.pruned.is_some_and(|at| now.duration_since(at).unwrap_or_default() < PRUNE_EVERY) {
                return;
            }
            state.pruned = Some(now);
        }
        prune(&self.raw_dir(), now, CAP_BYTES);
        prune(&self.good_dir(), now, good::CAP_BYTES);
    }
}

/// Removes raw outputs older than [`KEEP_FOR`], then the oldest until the rest fit in `cap`.
/// Returns how many were removed.
pub fn prune(raw_dir: &Path, now: SystemTime, cap: u64) -> usize {
    let Ok(entries) = fs::read_dir(raw_dir) else { return 0 };
    let mut files: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            meta.is_file().then(|| (meta.modified().unwrap_or(UNIX_EPOCH), meta.len(), entry.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
    let mut removed = 0;
    for (modified, len, path) in files {
        let old = now.duration_since(modified).unwrap_or_default() > KEEP_FOR;
        if (old || total > cap) && fs::remove_file(&path).is_ok() {
            total -= len;
            removed += 1;
        }
    }
    removed
}

/// Whether `command` names the raw folder, in either slash style.
fn mentions(command: &str, raw_dir: &Path) -> bool {
    let dir = raw_dir.to_string_lossy();
    command.contains(dir.as_ref())
        || command.contains(&dir.replace('\\', "/"))
        || command.contains("digest/raw/")
        || command.contains("digest\\raw\\")
}

/// Whether the output is a machine format or a diff, which reach the model as before (answer 30).
pub fn structured(output: &[u8]) -> bool {
    let head = &output[..output.len().min(64 * 1024)];
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start();
    let last = output.iter().rev().find(|byte| !byte.is_ascii_whitespace()).copied();
    let after_bracket = trimmed.strip_prefix('[').map(str::trim_start).and_then(|rest| rest.chars().next());
    let json = (trimmed.starts_with('{') && last == Some(b'}'))
        || (matches!(after_bracket, Some('{' | '[' | '"' | ']' | '0'..='9' | '-')) && last == Some(b']'));
    let rows: Vec<&str> = trimmed.lines().filter(|line| !line.trim().is_empty()).take(20).collect();
    let json_lines = rows.iter().filter(|line| line.trim_start().starts_with('{') && line.trim_end().ends_with('}'));
    if json || (!rows.is_empty() && json_lines.count() * 10 >= rows.len() * 8) {
        return true;
    }
    let first: Vec<&str> = trimmed.lines().take(40).collect();
    if first.iter().any(|line| line.starts_with("diff --git") || line.starts_with("@@ -"))
        || first.windows(2).any(|pair| pair[0].starts_with("--- ") && pair[1].starts_with("+++ "))
    {
        return true;
    }
    // CSV or TSV: the first ten lines all have the same number (two or more) of separators.
    let rows: Vec<&str> = first.iter().copied().filter(|line| !line.trim().is_empty()).take(10).collect();
    rows.len() >= 5
        && [',', '\t'].iter().any(|sep| {
            let count = rows[0].matches(*sep).count();
            count >= 2 && rows.iter().all(|row| row.matches(*sep).count() == count)
        })
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
