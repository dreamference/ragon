//! The profile window (specs/DREAMFERENCE_PROFILING.md §5.1): refuse, take the GPU lock, log, stop
//! production, run the steps, restore production and check it, log the end.
//!
//! Every command the window runs (`ragon-admin server stop`, the configuration start through
//! `start_server()`, `ragon-admin server start`, the unit, safety and GPU probes) is a configured
//! argument list, so a test fakes each one. Nothing is retried: a failed production start ends the
//! window as "not restored" (exit 2).
use crate::http;
use crate::metrics::{self as m, Metrics};
use serde_json::{Value, json};
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A window lasts at most this long (§5.1 step 5).
pub const WINDOW_MAX: Duration = Duration::from_secs(40 * 60);

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
mod sig {
    unsafe extern "C" {
        fn signal(signum: i32, handler: usize) -> usize;
    }
    extern "C" fn on_signal(_: i32) {
        super::INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub fn install() {
        // SIGINT and SIGTERM only set a flag; the window notices it between steps and restores.
        unsafe {
            signal(2, on_signal as extern "C" fn(i32) as usize);
            signal(15, on_signal as extern "C" fn(i32) as usize);
        }
    }
}

/// Makes SIGINT and SIGTERM end the window through the restore path.
pub fn install_interrupt_handler() {
    #[cfg(unix)]
    sig::install();
}

/// Marks the window as interrupted (what the signal handler does; tests call it too).
pub fn interrupt() {
    INTERRUPTED.store(true, Ordering::SeqCst);
}

fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

/// A command as an argument list; empty means "not configured".
pub type Cmd = Vec<String>;

#[derive(Clone, Debug)]
pub struct WindowConfig {
    /// Production's model server (loopback).
    pub url: String,
    /// The production model id `/v1/models` must answer with after the restore; any id when `None`.
    pub model: Option<String>,
    pub lock_path: PathBuf,
    /// The SWE-bench gate's `run.json`: present means the hold is closed.
    pub gate_run: Option<PathBuf>,
    /// Night Shift's `runner.lock`: held by a flock while a run is in progress.
    pub night_lock: Option<PathBuf>,
    /// The marker of an open window.
    pub open_marker: PathBuf,
    pub windows_log: PathBuf,
    pub label: String,
    /// Prints one line per active `ragon-swe-*` unit.
    pub units_cmd: Cmd,
    /// Exits 0 when `check_host_safety()` passes (`ragon-admin host check`).
    pub safety_cmd: Cmd,
    /// `ragon-admin server stop`.
    pub stop_cmd: Cmd,
    /// The configuration's start through `start_server()` with the shape's recipe overrides.
    pub config_start_cmd: Cmd,
    /// `ragon-admin server start`.
    pub restore_cmd: Cmd,
    /// Exits 0 when the GPU is idle (no compute process).
    pub gpu_idle_cmd: Cmd,
    /// Tells the node's alert channel; gets the window's one-line verdict as its last argument.
    pub alert_cmd: Cmd,
    /// Whether the port must be seen free after a stop (off only for a fake server in tests).
    pub check_port: bool,
    pub timeout: Duration,
    pub max_len: Duration,
}

#[derive(Debug, PartialEq)]
pub enum Refusal {
    SweUnit(String),
    GateHeld,
    NightRun,
    LockHeld,
    WindowOpen,
    Busy(String),
    Unsafe,
    Unreachable(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, fm: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Refusal::SweUnit(u) => write!(
                fm,
                "❌ A SWE-bench unit is active ({u}); nothing was stopped."
            ),
            Refusal::GateHeld => write!(
                fm,
                "❌ The SWE-bench gate hold is closed; nothing was stopped."
            ),
            Refusal::NightRun => write!(
                fm,
                "❌ A Night Shift run is in progress; nothing was stopped."
            ),
            Refusal::LockHeld => write!(fm, "❌ The GPU lock is held; nothing was stopped."),
            Refusal::WindowOpen => write!(fm, "❌ Another window is open; nothing was stopped."),
            Refusal::Busy(e) => write!(fm, "❌ {e}; nothing was stopped."),
            Refusal::Unsafe => write!(fm, "❌ The host safety check fails; nothing was stopped."),
            Refusal::Unreachable(e) => write!(
                fm,
                "❌ The model server does not answer ({e}); nothing was stopped."
            ),
        }
    }
}

/// How a finished window ended.
#[derive(Debug)]
pub struct Outcome {
    pub restored: bool,
    /// Whether the steps ran to the end.
    pub steps_ok: bool,
    pub record: Value,
}

impl Outcome {
    /// 0 clean, 1 a step failed or was interrupted (production restored), 2 production not restored.
    pub fn exit_code(&self) -> i32 {
        if !self.restored {
            2
        } else if self.steps_ok {
            0
        } else {
            1
        }
    }
}

fn run_cmd(cmd: &Cmd, timeout: Duration) -> Result<(bool, String), String> {
    let Some((prog, args)) = cmd.split_first() else {
        return Ok((true, String::new()));
    };
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{prog}: {e}"))?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                if let Some(mut o) = child.stdout.take() {
                    std::io::Read::read_to_string(&mut o, &mut out).ok();
                }
                return Ok((st.success(), out));
            }
            Ok(None) if start.elapsed() > timeout => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!("{prog} timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("{prog}: {e}")),
        }
    }
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

fn port_free(cfg: &WindowConfig) -> bool {
    if !cfg.check_port {
        return true;
    }
    let url = &cfg.url;
    let addr = http::host_port(url);
    match addr.to_socket_addrs() {
        Ok(mut a) => a.all(|s| TcpStream::connect_timeout(&s, Duration::from_millis(300)).is_err()),
        Err(_) => true,
    }
}

fn gpu_idle(cfg: &WindowConfig) -> bool {
    matches!(run_cmd(&cfg.gpu_idle_cmd, cfg.timeout), Ok((true, _)))
}

/// Refuses as §5.1 step 1 says; stops nothing.
pub fn check_refusals(cfg: &WindowConfig) -> Result<(), Refusal> {
    if let Ok((_, out)) = run_cmd(&cfg.units_cmd, cfg.timeout) {
        if let Some(l) = out.lines().map(str::trim).find(|l| !l.is_empty()) {
            let unit = l.split_whitespace().next().unwrap_or(l);
            return Err(Refusal::SweUnit(unit.to_string()));
        }
    }
    if cfg.gate_run.as_ref().is_some_and(|p| p.exists()) {
        return Err(Refusal::GateHeld);
    }
    if let Some(p) = &cfg.night_lock {
        if let Ok(f) = std::fs::OpenOptions::new().read(true).open(p) {
            if f.try_lock_shared().is_err() {
                return Err(Refusal::NightRun);
            }
        }
    }
    if cfg.open_marker.exists() {
        return Err(Refusal::WindowOpen);
    }
    let addr = http::host_port(&cfg.url);
    let met = http::get(&addr, "/metrics", cfg.timeout)
        .map(|t| Metrics::parse(&t))
        .map_err(Refusal::Unreachable)?;
    let (run, q) = (met.get(m::RUNNING), met.get(m::QUEUED));
    if run > 0.0 || q > 0.0 {
        return Err(Refusal::Busy(format!(
            "Production has {run} running and {q} queued requests"
        )));
    }
    if !matches!(run_cmd(&cfg.safety_cmd, cfg.timeout), Ok((true, _))) {
        return Err(Refusal::Unsafe);
    }
    Ok(())
}

fn take_lock(path: &Path) -> Result<std::fs::File, Refusal> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| Refusal::Unreachable(format!("lock file: {e}")))?;
    file.try_lock().map_err(|_| Refusal::LockHeld)?;
    Ok(file)
}

fn append(path: &Path, v: &Value) {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        writeln!(f, "{v}").ok();
    }
}

/// What the steps may do inside the window.
pub struct Ctl<'a> {
    cfg: &'a WindowConfig,
    started: Instant,
}

impl Ctl<'_> {
    /// Whether a step of this length still fits in the window (a step that would overrun moves on).
    pub fn fits(&self, step: Duration) -> bool {
        !interrupted() && self.started.elapsed() + step <= self.cfg.max_len
    }
    /// Starts the configuration through `start_server()` (safety check and watchdog run there).
    pub fn start_config(&self) -> Result<(), String> {
        match run_cmd(&self.cfg.config_start_cmd, self.cfg.timeout)? {
            (true, _) => Ok(()),
            _ => Err("the configuration did not start".into()),
        }
    }
    /// Stops the configuration.
    pub fn stop_config(&self) -> Result<(), String> {
        match run_cmd(&self.cfg.stop_cmd, self.cfg.timeout)? {
            (true, _) => Ok(()),
            _ => Err("the configuration did not stop".into()),
        }
    }
}

fn mem_available_kib() -> Option<u64> {
    let t = std::fs::read_to_string("/proc/meminfo").ok()?;
    t.lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))
        .and_then(|r| r.split_whitespace().next()?.parse().ok())
}

fn wait_production(cfg: &WindowConfig) -> bool {
    let addr = http::host_port(&cfg.url);
    let deadline = Instant::now() + cfg.timeout;
    loop {
        if let Ok(body) = http::get(&addr, "/v1/models", Duration::from_secs(5)) {
            if cfg
                .model
                .as_ref()
                .is_none_or(|id| body.contains(id.as_str()))
            {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Runs one window: the whole of §5.1. The restore runs on success, on a step error, on a panic and
/// on an interrupt.
pub fn run_window<F>(cfg: &WindowConfig, steps: F) -> Result<Outcome, Refusal>
where
    F: FnOnce(&Ctl) -> Result<(), String>,
{
    check_refusals(cfg)?;
    let _lock = take_lock(&cfg.lock_path)?;
    INTERRUPTED.store(false, Ordering::SeqCst);
    let (t0, started) = (now(), Instant::now());
    std::fs::write(&cfg.open_marker, &t0).ok();
    append(
        &cfg.windows_log,
        &json!({"event": "start", "start": t0, "label": cfg.label}),
    );
    let port_before = port_free(cfg);
    let gpu_before = gpu_idle(cfg);
    let mut mem_min = mem_available_kib();
    let stopped = matches!(run_cmd(&cfg.stop_cmd, cfg.timeout), Ok((true, _)));
    let (port_ok, gpu_ok) = (port_free(cfg), gpu_idle(cfg));
    let ctl = Ctl { cfg, started };
    let step_err: Option<String>;
    if !stopped {
        step_err = Some("production did not stop".into());
    } else if !port_ok || !gpu_ok {
        step_err = Some("the port or the GPU is not free after the stop".into());
    } else if interrupted() {
        step_err = Some("interrupted".into());
    } else {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| steps(&ctl)));
        step_err = match r {
            Ok(Ok(())) if interrupted() => Some("interrupted".into()),
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some(e),
            Err(_) => Some("a step panicked".into()),
        };
    }
    mem_min = mem_min.min(mem_available_kib()).or(mem_min);
    // Restore: stop the configuration, check the port and the GPU, start production once.
    let cfg_stopped = !stopped || matches!(run_cmd(&cfg.stop_cmd, cfg.timeout), Ok((true, _)));
    let port_clean = port_free(cfg);
    let gpu_clean = gpu_idle(cfg);
    let restored = cfg_stopped
        && port_clean
        && gpu_clean
        && matches!(run_cmd(&cfg.restore_cmd, cfg.timeout), Ok((true, _)))
        && wait_production(cfg);
    let steps_ok = step_err.is_none();
    let record = json!({
        "event": "end",
        "start": t0,
        "end": now(),
        "label": cfg.label,
        "production_stopped": stopped,
        "restored_healthy": restored,
        "port_free_before": port_before,
        "port_free_after": port_clean,
        "gpu_free_before": gpu_before,
        "gpu_free_after": gpu_clean,
        "mem_available_min_kib": mem_min,
        "note": if restored { step_err.clone() } else { Some("not restored".to_string()) },
    });
    append(&cfg.windows_log, &record);
    std::fs::remove_file(&cfg.open_marker).ok();
    if !restored {
        let mut alert = cfg.alert_cmd.clone();
        if !alert.is_empty() {
            alert.push(format!("profile window {} not restored", cfg.label));
            run_cmd(&alert, cfg.timeout).ok();
        }
    }
    Ok(Outcome {
        restored,
        steps_ok,
        record,
    })
}
