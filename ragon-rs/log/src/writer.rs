//! The queue and the writer thread (§5.4, §7.5): a call copies its record into a bounded queue and
//! returns; the thread formats, samples and writes. A full queue drops and counts; no log call
//! blocks on a file or a pipe.

use crate::format::{Field, Formatter, Kind, Value};
use crate::LogLevelSpec;
use crate::level::Level;
use crate::redact::Env;
use crate::sampler::{Decision, Request, Sampler, Synthetic};
use crate::sink::{FileSink, JournaldSink, Rotation, Sink, StderrSink, under_journald};
use crate::time::now_ms;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

/// The queue holds at most this many records (§5.4).
pub const QUEUE: usize = 8192;
/// The shared component table.
pub const COMPONENTS_TXT: &str = include_str!("../../vectors/logging/components.txt");

/// The components, one per line of `components.txt` (`#` lines are comments).
pub fn components() -> Vec<&'static str> {
    COMPONENTS_TXT
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect()
}

enum Msg {
    Record { ts_ms: u64, level: Level, message: &'static str, fields: Vec<Field> },
    Flush(mpsc::Sender<()>),
}

/// A handle to one component's writer. Cloning is cheap; the thread ends when the last clone drops.
#[derive(Clone)]
pub struct Logger {
    tx: SyncSender<Msg>,
    dropped: Arc<AtomicU64>,
    level: Option<Level>,
    thread: Arc<std::sync::Mutex<Option<JoinHandle<()>>>>,
}

/// `~/.local/state/dreamference/logs` (`$XDG_STATE_HOME/dreamference/logs`).
pub fn default_log_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("dreamference").join("logs")
}

fn is_container() -> bool {
    Path::new("/.dockerenv").exists() || std::env::var_os("RAGON_IN_CONTAINER").is_some()
}

impl Logger {
    /// Starts the writer for `component` with the sink §3.1 names: journald, else stderr in a
    /// container, else the file. The level comes from `RAGON_LOG`, then `log.env`, then `info`.
    pub fn init(component: &str) -> Result<Logger, String> {
        let table = components();
        if !table.contains(&component) {
            return Err(format!("unknown component {component}"));
        }
        let config = std::env::var_os("HOME").map(|h| Path::new(&h).join(".config/dreamference/log.env"));
        let level = LogLevelSpec::from_env(config.as_deref()).level_for(component);
        let salt = std::env::var_os("HOME")
            .and_then(|h| std::fs::read(Path::new(&h).join(".config/dreamference/log-salt")).ok())
            .unwrap_or_default();
        let sink: Box<dyn Sink> = if under_journald() {
            Box::new(JournaldSink)
        } else if is_container() {
            Box::new(StderrSink)
        } else {
            Box::new(FileSink::new(&default_log_dir(), component, Rotation::default()))
        };
        Ok(Self::with_sink(component, level, Env::from_system(), salt, sink))
    }

    /// Starts a writer on any sink (tests, and callers that choose their own).
    pub fn with_sink(component: &str, level: Option<Level>, env: Env, salt: Vec<u8>, sink: Box<dyn Sink>) -> Logger {
        let (tx, rx) = sync_channel(QUEUE);
        let dropped = Arc::new(AtomicU64::new(0));
        let formatter = Formatter::new(env, &salt);
        let sampler = Sampler::new(level.unwrap_or(Level::Info));
        let component = component.to_string();
        let counter = dropped.clone();
        let handle = std::thread::spawn(move || run(rx, formatter, sampler, sink, &component, &counter));
        Logger { tx, dropped, level, thread: Arc::new(std::sync::Mutex::new(Some(handle))) }
    }

    /// True when a record at `level` would be written.
    pub fn enabled(&self, level: Level) -> bool {
        self.level.is_some_and(|max| level <= max)
    }

    /// Queues one record. The message is a literal; a full queue drops it and counts it.
    pub fn log(&self, level: Level, message: &'static str, fields: &[Field]) {
        if !self.enabled(level) {
            return;
        }
        let msg = Msg::Record { ts_ms: now_ms(), level, message, fields: fields.to_vec() };
        if self.tx.try_send(msg).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn error(&self, message: &'static str, fields: &[Field]) {
        self.log(Level::Error, message, fields);
    }
    pub fn warn(&self, message: &'static str, fields: &[Field]) {
        self.log(Level::Warn, message, fields);
    }
    pub fn info(&self, message: &'static str, fields: &[Field]) {
        self.log(Level::Info, message, fields);
    }
    pub fn debug(&self, message: &'static str, fields: &[Field]) {
        self.log(Level::Debug, message, fields);
    }

    /// Records dropped because the queue was full and not yet reported.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Waits until everything queued so far is written, for at most `timeout`.
    pub fn flush(&self, timeout: Duration) -> bool {
        let (ack_tx, ack_rx) = mpsc::channel();
        let deadline = std::time::Instant::now() + timeout;
        let mut msg = Msg::Flush(ack_tx);
        loop {
            match self.tx.try_send(msg) {
                Ok(()) => break,
                Err(mpsc::TrySendError::Full(m)) if std::time::Instant::now() < deadline => {
                    msg = m;
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return false,
            }
        }
        ack_rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).is_ok()
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        if Arc::strong_count(&self.thread) == 1 {
            // The last handle: closing the channel ends the thread; do not wait on a stalled sink.
            let _ = self.flush(Duration::from_millis(500));
        }
    }
}

fn is_request_close(message: &str) -> bool {
    message == "request done" || message == "request refused"
}

fn request_of(fields: &[Field]) -> Request {
    let ms = fields.iter().find_map(|f| match (f.key.as_str(), &f.value) {
        ("ms", Value::Int(n)) => Some(*n as f64),
        ("ms", Value::Float(x)) => Some(*x),
        _ => None,
    });
    let error = fields.iter().any(|f| f.key == "finish" && f.kind == Kind::Enum && f.value == Value::from("error"));
    Request { ms: ms.unwrap_or(0.0), error }
}

/// A drop reason: one of the sampler's and the sinks' own words.
fn reason(word: &str) -> Field {
    Field::enumeration("reason", word, &["rate", "queue_full", word])
}

fn emit(
    sink: &mut dyn Sink,
    formatter: &Formatter,
    component: &str,
    ts: u64,
    level: Level,
    message: &str,
    fields: &[Field],
) {
    let line = formatter.line(ts, component, level, message, fields);
    sink.write(level, &line, ts);
}

fn run(
    rx: Receiver<Msg>,
    formatter: Formatter,
    mut sampler: Sampler,
    mut sink: Box<dyn Sink>,
    component: &str,
    queue_dropped: &AtomicU64,
) {
    let count = |n: u64| Field::count("count", n);
    loop {
        let msg = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(m) => Some(m),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let now = now_ms();
        for synthetic in sampler.tick(now) {
            match synthetic {
                Synthetic::Suppressed { site, count: n } => {
                    // The site is a literal message the sampler saw, already in the grammar.
                    let line = format!(
                        "{} {component} WARN log lines suppressed site=\"{site}\" count={n}",
                        crate::time::iso_ms(now)
                    );
                    sink.write(Level::Warn, &line, now);
                }
                Synthetic::Dropped { count: n } => {
                    let fields = [count(n), reason("rate")];
                    emit(&mut *sink, &formatter, component, now, Level::Warn, "log lines dropped", &fields);
                }
                Synthetic::Summary { count: n, errors, p50_ms, p95_ms } => {
                    let fields = [
                        count(n),
                        Field::count("errors", errors),
                        Field::ms("p50_ms", p50_ms),
                        Field::ms("p95_ms", p95_ms),
                    ];
                    emit(&mut *sink, &formatter, component, now, Level::Info, "requests summary", &fields);
                }
            }
        }
        let queued = queue_dropped.swap(0, Ordering::Relaxed);
        if queued > 0 {
            let fields = [count(queued), reason("queue_full")];
            emit(&mut *sink, &formatter, component, now, Level::Warn, "log lines dropped", &fields);
        }
        if let Some((n, why)) = sink.pending_drop(now) {
            let fields = [count(n), reason(why)];
            let line = formatter.line(now, component, Level::Warn, "log lines dropped", &fields);
            if sink.write(Level::Warn, &line, now) {
                sink.clear_drop();
            }
        }
        match msg {
            Some(Msg::Record { ts_ms, level, message, fields }) => {
                let request = is_request_close(message).then(|| request_of(&fields));
                if sampler.admit(ts_ms, level, message, request) == Decision::Write {
                    emit(&mut *sink, &formatter, component, ts_ms, level, message, &fields);
                }
            }
            Some(Msg::Flush(ack)) => {
                loop {
                    match rx.try_recv() {
                        Ok(Msg::Record { ts_ms, level, message, fields }) => {
                            let request = is_request_close(message).then(|| request_of(&fields));
                            if sampler.admit(ts_ms, level, message, request) == Decision::Write {
                                emit(&mut *sink, &formatter, component, ts_ms, level, message, &fields);
                            }
                        }
                        Ok(Msg::Flush(other)) => {
                            let _ = other.send(());
                        }
                        Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
                    }
                }
                let _ = ack.send(());
            }
            None => {}
        }
    }
}
