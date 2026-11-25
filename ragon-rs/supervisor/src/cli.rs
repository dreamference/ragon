//! `server <start|stop|status|run|install|uninstall>`: the command line of the supervisor, shared by
//! the launcher (`ragon server ...` on Windows) and the `ragon-supervisor` drill binary.
//!
//! - `run [--service] [options] -- <engine> [args]` supervises in the foreground; `--service` is the
//!   form the Windows service starts (src/windows_service.rs). On Unix SIGTERM and SIGINT stop it
//!   cleanly (the engine first), which is how launchd stops the Mac daemon.
//! - `start` starts the `ragon-node` service when it is installed (the Windows service, the macOS
//!   LaunchDaemon), otherwise a detached `run` with the same options; `stop` stops the service,
//!   otherwise asks the running supervisor to stop through a `stop` file in the state directory.
//! - `status` prints stable `key: value` lines (see [`status_report`]); exit 0 when healthy, 3 not.
//!   On macOS one more line follows the shared ones: `power: mains|battery|unknown`. On battery the
//!   state of a serving engine is `battery` (exit 0 too) and `charge` and `admission` follow
//!   `updated` (src/macos_power.rs).
//! - `install` / `uninstall` (administrator / root): on Windows the service under
//!   `NT SERVICE\ragon-node`, a virtual account that is not an administrator; on macOS the
//!   LaunchDaemon `ai.dreamference.ragon-node` as the role account `_ragon` (src/macos_launchd.rs),
//!   with the mains power settings (src/macos_power.rs) set at install and restored at uninstall.
//!
//! The running supervisor writes `status.txt` in the state directory on every tick; the engine's
//! output goes to `engine.log` there.

use std::path::{Path, PathBuf};
#[cfg(not(unix))]
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::{self, Policy, Snapshot, Supervisor};
use crate::probe::HttpProbe;
use crate::process::ChildEngine;

pub const DEFAULT_SERVICE_NAME: &str = "ragon-node";
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8000;
pub const STATUS_FILE: &str = "status.txt";
pub const STOP_FILE: &str = "stop";
pub const ENGINE_LOG: &str = "engine.log";
pub const SUPERVISOR_LOG: &str = "supervisor.log";
/// A status file older than this, from a supervisor that did not say it stopped, is stale.
pub const STALE_AFTER_S: u64 = 15;

const USAGE: &str = "usage: server <start|stop|status|run|install|uninstall> [--name N] [--state-dir D] \
[--host H] [--port P] [--wait S] [--start-timeout S] [--account A] [--service] [--launchd daemon|agent] \
[-- <engine> [args...]]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub command: String,
    pub name: String,
    pub state_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub wait_s: u64,
    pub start_timeout_s: Option<u64>,
    pub account: Option<String>,
    pub service: bool,
    /// macOS `install`: `daemon` (MN4 a) or `agent` (MN4 b); `None` follows `MN4_DAEMON`.
    pub launchd: Option<String>,
    pub engine: Vec<String>,
}

pub fn default_state_dir() -> PathBuf {
    // macOS: the daemon's directory for root (install) and whenever the daemon is installed.
    #[cfg(target_os = "macos")]
    if mac::is_root() || crate::macos_launchd::installed_kind(DEFAULT_SERVICE_NAME, |p| p.exists(), None).is_some() {
        return PathBuf::from(crate::macos_launchd::STATE_DIR);
    }
    if cfg!(windows) {
        let base = std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        return base.join("Ragon").join("node");
    }
    if let Some(dir) = std::env::var_os("XDG_STATE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(dir).join("ragon").join("node");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(".local").join("state").join("ragon").join("node")
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut it = args.iter();
    let command = it.next().ok_or_else(|| USAGE.to_string())?.clone();
    if !["start", "stop", "status", "run", "install", "uninstall"].contains(&command.as_str()) {
        return Err(format!("unknown server command {command:?}\n{USAGE}"));
    }
    let mut opts = Options {
        command,
        name: DEFAULT_SERVICE_NAME.to_string(),
        state_dir: default_state_dir(),
        host: DEFAULT_HOST.to_string(),
        port: DEFAULT_PORT,
        wait_s: 10,
        start_timeout_s: None,
        account: None,
        service: false,
        launchd: None,
        engine: Vec::new(),
    };
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--" => {
                opts.engine = it.by_ref().cloned().collect();
                break;
            }
            "--name" => opts.name = value("--name")?,
            "--state-dir" => opts.state_dir = PathBuf::from(value("--state-dir")?),
            "--host" => opts.host = value("--host")?,
            "--port" => opts.port = value("--port")?.parse().map_err(|_| "--port needs a number".to_string())?,
            "--wait" => opts.wait_s = value("--wait")?.parse().map_err(|_| "--wait needs seconds".to_string())?,
            "--start-timeout" => {
                opts.start_timeout_s =
                    Some(value("--start-timeout")?.parse().map_err(|_| "--start-timeout needs seconds".to_string())?)
            }
            "--account" => opts.account = Some(value("--account")?),
            "--service" => opts.service = true,
            "--launchd" => opts.launchd = Some(value("--launchd")?),
            "-h" | "--help" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option {other:?}\n{USAGE}")),
        }
    }
    let needs_engine = matches!(opts.command.as_str(), "run" | "install");
    if needs_engine && opts.engine.is_empty() {
        return Err(format!("`server {}` needs the engine after `--`\n{USAGE}", opts.command));
    }
    Ok(opts)
}

/// Runs `server <args>` and returns the exit code.
/// What the launcher adds to `server run` on a Mac node (its src/supervisor_macos.rs): the
/// processes held beside the engine and what follows a change of the reported state (the Bonjour
/// registration). The drill binary and the Windows service use [`NoHooks`].
pub trait NodeHooks {
    /// `(name, command line)` of each process held beside the engine (`ragon web serve --lan`):
    /// started with it, under the same guard, restarted with the core's backoff (exit 3 is not
    /// retried), logging to `<name>.log`. Its state never changes the engine's.
    fn sidecars(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
    /// The reported state (`healthy`, `battery`, `starting`, `waiting`, `refused`, `stopped`) and
    /// whether new sessions are admitted; called once per change, and with `stopped` at the end.
    fn on_state(&mut self, _state: &str, _admitting: bool) {}
    /// Whether to run the tool-call canary ([`crate::canary`]) after each start of the engine: in a
    /// thread, never gating it; the verdict is logged (`server run: canary: ...`).
    fn canary(&self) -> bool {
        false
    }
}

/// No sidecars, nothing on a state change.
pub struct NoHooks;

impl NodeHooks for NoHooks {}

pub fn run(args: &[String]) -> i32 {
    run_with(args, &mut NoHooks)
}

/// [`run`] with the launcher's hooks (used by `run` only; the other commands ignore them).
pub fn run_with(args: &[String], hooks: &mut dyn NodeHooks) -> i32 {
    let opts = match parse(args) {
        Ok(opts) => opts,
        Err(message) => {
            eprintln!("{message}");
            return 2;
        }
    };
    match opts.command.as_str() {
        "run" => run_command(&opts, hooks),
        "start" => start(&opts),
        "stop" => stop(&opts),
        "status" => status(&opts),
        "install" => install(&opts),
        "uninstall" => uninstall(&opts),
        _ => 2,
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The options `run` needs, as arguments (what `start` passes to a detached `run` and what the
/// service's command line carries).
pub fn run_args(opts: &Options, service: bool) -> Vec<String> {
    let mut args = vec!["server".to_string(), "run".to_string()];
    if service {
        args.push("--service".into());
    }
    args.extend(["--name".into(), opts.name.clone()]);
    args.extend(["--state-dir".into(), opts.state_dir.display().to_string()]);
    args.extend(["--host".into(), opts.host.clone(), "--port".into(), opts.port.to_string()]);
    if let Some(s) = opts.start_timeout_s {
        args.extend(["--start-timeout".into(), s.to_string()]);
    }
    args.push("--".into());
    args.extend(opts.engine.iter().cloned());
    args
}

fn run_command(opts: &Options, hooks: &mut dyn NodeHooks) -> i32 {
    if opts.service {
        #[cfg(windows)]
        {
            let owned = opts.clone();
            let body = Box::new(move |stop: Arc<AtomicBool>| supervise(&owned, &stop));
            return match crate::windows_service::run_as_service(&opts.name, body) {
                Ok(code) => code,
                Err(e) => {
                    eprintln!("server run --service: not started by the service manager: {e}");
                    1
                }
            };
        }
        #[cfg(not(windows))]
        {
            eprintln!("server run --service is Windows only; run it without --service");
            return 2;
        }
    }
    #[cfg(unix)]
    {
        unix_stop::install();
        supervise_with(opts, &unix_stop::STOP, hooks)
    }
    #[cfg(not(unix))]
    supervise_with(opts, &Arc::new(AtomicBool::new(false)), hooks)
}

/// SIGTERM (launchd's stop, `kill`) and SIGINT (Ctrl-C) set a flag the supervision loop polls, so
/// the supervisor stops the engine before it exits instead of dying with the engine still running.
#[cfg(unix)]
mod unix_stop {
    use std::sync::atomic::{AtomicBool, Ordering};

    pub static STOP: AtomicBool = AtomicBool::new(false);
    const SIGINT: i32 = 2;
    const SIGTERM: i32 = 15;

    unsafe extern "C" {
        fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
    }

    extern "C" fn on_stop(_sig: i32) {
        // An atomic store is async-signal-safe.
        STOP.store(true, Ordering::SeqCst);
    }

    pub fn install() {
        // SAFETY: the handler only stores to an atomic.
        unsafe {
            signal(SIGTERM, on_stop);
            signal(SIGINT, on_stop);
        }
    }
}

/// The supervision loop: tick, write the status file, sleep until the next tick or a stop request
/// (the flag, set by the service's stop control, or the `stop` file).
pub fn supervise(opts: &Options, stop: &AtomicBool) -> i32 {
    supervise_with(opts, stop, &mut NoHooks)
}

/// A sidecar is up while its process runs: it has no health endpoint the supervisor knows.
struct Alive;

impl core::Probe for Alive {
    fn healthy(&mut self) -> bool {
        true
    }
}

/// [`supervise`] with the processes and the state hook of [`NodeHooks`].
pub fn supervise_with(opts: &Options, stop: &AtomicBool, hooks: &mut dyn NodeHooks) -> i32 {
    if let Err(e) = std::fs::create_dir_all(&opts.state_dir) {
        eprintln!("server run: cannot create {}: {e}", opts.state_dir.display());
        return 1;
    }
    let stop_file = opts.state_dir.join(STOP_FILE);
    let _ = std::fs::remove_file(&stop_file);
    let program = PathBuf::from(&opts.engine[0]);
    let engine = match ChildEngine::new(program, opts.engine[1..].to_vec(), Some(opts.state_dir.join(ENGINE_LOG))) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("server run: {e}");
            return 1;
        }
    };
    let probe = HttpProbe::models(&opts.host, opts.port);
    let url = probe.url();
    let mut policy = Policy::default();
    if let Some(s) = opts.start_timeout_s {
        policy.start_timeout = Duration::from_secs(s);
    }
    let mut sup = Supervisor::new(engine, probe, policy);
    let me = std::process::id();
    let mut power = PowerWatch::default();
    let mut sidecars = Vec::new();
    for (name, argv) in hooks.sidecars() {
        let Some((program, args)) = argv.split_first() else { continue };
        let log = opts.state_dir.join(format!("{name}.log"));
        match ChildEngine::new(PathBuf::from(program), args.to_vec(), Some(log)) {
            Ok(child) => sidecars.push((name, Supervisor::new(child, Alive, Policy::default()), String::new())),
            Err(e) => eprintln!("server run: {name}: {e}"),
        }
    }
    let mut reported = (String::new(), true);
    let mut engine_was_healthy = false;
    let mut canary: Option<std::sync::mpsc::Receiver<Result<Duration, String>>> = None;
    loop {
        if stop.load(Ordering::SeqCst) || stop_file.exists() {
            for (_, sidecar, _) in &mut sidecars {
                sidecar.stop();
            }
            sup.stop();
            let _ = std::fs::remove_file(&stop_file);
            write_status(opts, &url, &sup.snapshot(Instant::now()), None, None);
            hooks.on_state("stopped", false);
            return 0;
        }
        let mut wait = sup.tick(Instant::now());
        for (name, sidecar, last) in &mut sidecars {
            wait = wait.min(sidecar.tick(Instant::now()));
            let snap = sidecar.snapshot(Instant::now());
            if snap.state.as_str() != last.as_str() {
                *last = snap.state.as_str().to_string();
                eprintln!("server run: {name}: {last} (pid {})", opt(snap.engine_pid));
            }
        }
        let battery = power.poll(Instant::now());
        let snap = sup.snapshot(Instant::now());
        let healthy = snap.state.as_str() == "healthy";
        if healthy && !engine_was_healthy && hooks.canary() && canary.is_none() {
            let (tx, rx) = std::sync::mpsc::channel();
            let (host, port) = (opts.host.clone(), opts.port);
            std::thread::spawn(move || {
                let _ = tx.send(crate::canary::run(&host, port, Duration::from_secs(crate::canary::TIMEOUT_S)));
            });
            canary = Some(rx);
        }
        engine_was_healthy = healthy;
        if let Some(verdict) = canary.as_ref().and_then(|rx| rx.try_recv().ok()) {
            canary = None;
            match verdict {
                Ok(took) => eprintln!("server run: canary: ok ({} ms)", took.as_millis()),
                Err(e) => eprintln!("server run: canary: failed: {e}"),
            }
        }
        let now_reported = (
            crate::macos_power::reported_state(snap.state.as_str(), battery.as_ref()).to_string(),
            battery.is_none_or(|b| b.admitting),
        );
        if now_reported != reported {
            reported = now_reported;
            hooks.on_state(&reported.0, reported.1);
        }
        write_status(opts, &url, &snap, Some(me), battery.as_ref());
        let due = Instant::now() + wait;
        while Instant::now() < due && !stop.load(Ordering::SeqCst) && !stop_file.exists() {
            std::thread::sleep(due.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
        }
    }
}

/// The power source as the supervision loop sees it: `pmset -g batt` every
/// [`POLL_S`](crate::macos_power::POLL_S) on macOS, never elsewhere. A change is logged once.
#[derive(Default)]
struct PowerWatch {
    read_at: Option<Instant>,
    battery: Option<crate::macos_power::Battery>,
}

impl PowerWatch {
    fn poll(&mut self, now: Instant) -> Option<crate::macos_power::Battery> {
        #[cfg(target_os = "macos")]
        if self.read_at.is_none_or(|t| now.duration_since(t).as_secs() >= crate::macos_power::POLL_S) {
            use crate::macos_power as power;
            self.read_at = Some(now);
            let read = mac::output("pmset", &["-g".into(), "batt".into()])
                .and_then(|text| power::battery(&text, power::MIN_CHARGE_PCT));
            if read != self.battery {
                match read {
                    Some(b) => eprintln!(
                        "server run: on battery ({}%): admission {}",
                        b.charge.map(|c| c.to_string()).unwrap_or_else(|| "?".into()),
                        if b.admitting { "open" } else { "closed" }
                    ),
                    None => eprintln!("server run: off battery"),
                }
            }
            self.battery = read;
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (now, &mut self.read_at);
        self.battery
    }
}

fn opt<T: ToString>(value: Option<T>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

/// The status file's text.
pub fn status_file_text(name: &str, url: &str, snap: &Snapshot, supervisor_pid: Option<u32>, now_unix: u64) -> String {
    core::render_status(&[
        ("name", name.to_string()),
        ("state", snap.state.as_str().to_string()),
        ("supervisor_pid", opt(supervisor_pid)),
        ("engine_pid", opt(snap.engine_pid)),
        ("url", url.to_string()),
        ("restarts", snap.restarts.to_string()),
        ("last_exit", opt(snap.last_exit)),
        ("next_start_s", opt(snap.next_start_s)),
        ("last_error", snap.last_error.clone().unwrap_or_default().replace(['\r', '\n'], " ")),
        ("updated", core::utc_timestamp(now_unix)),
        ("updated_unix", now_unix.to_string()),
    ])
}

fn write_status(
    opts: &Options,
    url: &str,
    snap: &Snapshot,
    supervisor_pid: Option<u32>,
    battery: Option<&crate::macos_power::Battery>,
) {
    let mut text = status_file_text(&opts.name, url, snap, supervisor_pid, unix_now());
    if let Some(b) = battery {
        text = crate::macos_power::status_on_battery(&text, b);
    }
    let path = opts.state_dir.join(STATUS_FILE);
    let tmp = opts.state_dir.join(format!("{STATUS_FILE}.tmp"));
    if std::fs::write(&tmp, text).is_ok() {
        // On Windows a reader holding the file open can make the rename fail; the next tick retries.
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// What `server status` prints. `service_state` is the service manager's view (`not installed`,
/// or `n/a` off Windows), `file` the supervisor's status file, `probe` a live probe's HTTP code.
/// Always the same 13 keys in the same order; a running supervisor on battery adds `charge` and
/// `admission` after them. Exit code 0 when the engine is healthy (or serving on battery), 3 otherwise.
pub fn status_report(
    name: &str,
    service_state: &str,
    file: Option<&str>,
    now_unix: u64,
    probe: Option<u16>,
    default_url: &str,
) -> (String, i32) {
    let fields = file.map(core::parse_status).unwrap_or_default();
    let get = |key: &str| -> String {
        fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).filter(|v| v != "none").unwrap_or_default()
    };
    let updated_unix: Option<u64> = get("updated_unix").parse().ok();
    let supervisor = match (file, get("supervisor_pid").is_empty(), updated_unix) {
        (None, _, _) => "absent",
        (Some(_), true, _) => "stopped",
        (Some(_), false, Some(t)) if now_unix.saturating_sub(t) <= STALE_AFTER_S => "running",
        _ => "stale",
    };
    let running = supervisor == "running";
    let state = if running { get("state") } else { "stopped".to_string() };
    let url = Some(get("url")).filter(|u| !u.is_empty()).unwrap_or_else(|| default_url.to_string());
    let probe_text = match probe {
        Some(200) => "ok".to_string(),
        Some(code) => format!("http {code}"),
        None => "no answer".to_string(),
    };
    let pick = |key: &str| if running { get(key) } else { String::new() };
    let text = core::render_status(&[
        ("service", name.to_string()),
        ("service_state", service_state.to_string()),
        ("supervisor", supervisor.to_string()),
        ("state", state.clone()),
        ("supervisor_pid", pick("supervisor_pid")),
        ("engine_pid", pick("engine_pid")),
        ("url", url),
        ("probe", probe_text),
        ("restarts", get("restarts")),
        ("last_exit", get("last_exit")),
        ("next_start_s", pick("next_start_s")),
        ("last_error", get("last_error")),
        ("updated", get("updated")),
    ]);
    let mut text = text;
    if running && fields.iter().any(|(k, _)| k == "admission") {
        text.push_str(&core::render_status(&[("charge", get("charge")), ("admission", get("admission"))]));
    }
    let serving = state == "healthy" || state == "battery";
    let code = if serving && probe == Some(200) { 0 } else { 3 };
    (text, code)
}

fn read_status_file(opts: &Options) -> Option<String> {
    std::fs::read_to_string(opts.state_dir.join(STATUS_FILE)).ok()
}

fn service_state(opts: &Options) -> String {
    #[cfg(windows)]
    {
        match crate::windows_service::query(&opts.name) {
            Ok(Some(q)) => q.state.to_string(),
            Ok(None) => "not installed".to_string(),
            Err(e) => format!("unknown ({e})"),
        }
    }
    #[cfg(target_os = "macos")]
    {
        let Some(kind) = mac::installed(opts) else {
            return "not installed".to_string();
        };
        match mac::output("launchctl", &crate::macos_launchd::print_args(kind, &opts.name)) {
            Some(text) => crate::macos_launchd::parse_print_state(&text).to_string(),
            None => "unknown".to_string(),
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = opts;
        "n/a".to_string()
    }
}

fn status(opts: &Options) -> i32 {
    let probe = HttpProbe::models(&opts.host, opts.port);
    let file = read_status_file(opts);
    // Probe the port the running supervisor watches, when it says which.
    let url_port = file
        .as_deref()
        .map(core::parse_status)
        .and_then(|f| f.into_iter().find(|(k, _)| k == "url"))
        .and_then(|(_, url)| url.rsplit(':').next().and_then(|tail| tail.split('/').next()?.parse::<u16>().ok()));
    let live = HttpProbe { port: url_port.unwrap_or(opts.port), ..probe.clone() };
    let (text, code) =
        status_report(&opts.name, &service_state(opts), file.as_deref(), unix_now(), live.status_code(), &probe.url());
    print!("{text}");
    #[cfg(target_os = "macos")]
    {
        let source = mac::output("pmset", &["-g".into(), "batt".into()])
            .map(|t| crate::macos_power::parse_batt(&t))
            .unwrap_or(crate::macos_power::PowerSource::Unknown);
        println!("power: {}", source.as_str());
    }
    code
}

/// True when the service is installed (the Windows service, the macOS LaunchDaemon's plist);
/// `start`/`stop` then go through the service manager.
fn service_installed(opts: &Options) -> bool {
    #[cfg(windows)]
    {
        matches!(crate::windows_service::query(&opts.name), Ok(Some(_)))
    }
    #[cfg(target_os = "macos")]
    {
        mac::installed(opts).is_some()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = opts;
        false
    }
}

fn supervisor_running(opts: &Options) -> bool {
    let (text, _) = status_report(&opts.name, "", read_status_file(opts).as_deref(), unix_now(), None, "");
    text.contains("\nsupervisor: running\n")
}

/// Waits up to `secs` for `done`, checking every 200 ms.
fn wait_for(secs: u64, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn start(opts: &Options) -> i32 {
    if service_installed(opts) {
        #[cfg(windows)]
        if let Err(e) = crate::windows_service::start(&opts.name) {
            eprintln!("server start: {e}");
            return 1;
        }
        #[cfg(target_os = "macos")]
        if let Err(code) = mac::start_daemon(opts) {
            return code;
        }
    } else {
        if supervisor_running(opts) {
            println!("server start: already running");
            return status(opts);
        }
        if opts.engine.is_empty() {
            eprintln!("server start: the {} service is not installed; give the engine after `--`", opts.name);
            return 2;
        }
        if let Err(e) = spawn_detached(opts) {
            eprintln!("server start: {e}");
            return 1;
        }
    }
    wait_for(opts.wait_s.max(5), || supervisor_running(opts));
    let healthy = wait_for(opts.wait_s, || {
        read_status_file(opts).is_some_and(|t| t.contains("\nstate: healthy\n") || t.contains("\nstate: battery\n"))
    });
    let code = status(opts);
    if healthy || supervisor_running(opts) { 0 } else { code.max(1) }
}

fn spawn_detached(opts: &Options) -> std::io::Result<()> {
    std::fs::create_dir_all(&opts.state_dir)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(opts.state_dir.join(SUPERVISOR_LOG))?;
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command
        .args(run_args(opts, false))
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB);
        keep_std_handles_from_children();
        if command.spawn().is_ok() {
            return Ok(());
        }
        // The caller's job forbids breakaway (some remote shells): start inside it.
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(|_| ())
}

/// Windows hands every inheritable handle to a child, not only its standard ones: the detached
/// supervisor would hold the caller's stdout pipe open for as long as it runs, so a caller reading
/// `server start`'s output to the end (a script, `$x = ragon server start`) would wait for the
/// server to stop. Our own standard handles are made non-inheritable before the spawn.
#[cfg(windows)]
fn keep_std_handles_from_children() {
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        // SAFETY: plain Win32 calls on this process's own standard handles.
        unsafe {
            let handle = GetStdHandle(which);
            if handle != 0 && handle != INVALID_HANDLE_VALUE {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

fn stop(opts: &Options) -> i32 {
    if service_installed(opts) {
        #[cfg(windows)]
        if let Err(e) = crate::windows_service::stop(&opts.name, Duration::from_secs(opts.wait_s.max(30))) {
            eprintln!("server stop: {e}");
            return 1;
        }
        #[cfg(target_os = "macos")]
        if let Err(code) = mac::stop_daemon(opts) {
            return code;
        }
    } else {
        if !supervisor_running(opts) {
            println!("server stop: not running");
            status(opts);
            return 0;
        }
        if let Err(e) = std::fs::write(opts.state_dir.join(STOP_FILE), b"stop\n") {
            eprintln!("server stop: {e}");
            return 1;
        }
        if !wait_for(opts.wait_s.max(30), || !supervisor_running(opts)) {
            eprintln!("server stop: the supervisor did not stop");
            return 1;
        }
    }
    status(opts);
    0
}

fn install(opts: &Options) -> i32 {
    #[cfg(windows)]
    {
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                eprintln!("server install: {e}");
                return 1;
            }
        };
        let mut words = vec![exe.display().to_string()];
        words.extend(run_args(opts, true));
        let command_line = windows_command_line(&words);
        let account = opts.account.clone().unwrap_or_else(|| format!(r"NT SERVICE\{}", opts.name));
        match crate::windows_service::install(&opts.name, &account, &command_line, &opts.state_dir) {
            Ok(()) => {
                println!("installed service {} as {account}", opts.name);
                println!("command line: {command_line}");
                0
            }
            Err(e) => {
                eprintln!("server install: {e}");
                1
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        mac::install(opts)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = opts;
        eprintln!("server install is Windows and macOS only");
        2
    }
}

fn uninstall(opts: &Options) -> i32 {
    #[cfg(windows)]
    {
        match crate::windows_service::uninstall(&opts.name) {
            Ok(()) => {
                println!("removed service {}", opts.name);
                0
            }
            Err(e) => {
                eprintln!("server uninstall: {e}");
                1
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        mac::uninstall(opts)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = opts;
        eprintln!("server uninstall is Windows and macOS only");
        2
    }
}

/// macOS: the LaunchDaemon and the power settings through `launchctl` and `pmset`. Changing either
/// needs root; reading them (`server status`) does not.
#[cfg(target_os = "macos")]
mod mac {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    use super::{Options, run_args, supervisor_running, wait_for};
    use crate::macos_launchd::{self as launchd, Kind};
    use crate::macos_power as power;

    unsafe extern "C" {
        fn geteuid() -> u32;
    }

    pub fn is_root() -> bool {
        // SAFETY: a plain system call.
        unsafe { geteuid() == 0 }
    }

    /// A command's stdout and stderr together, or `None` when it cannot be run.
    pub fn output(program: &str, args: &[String]) -> Option<String> {
        let out = Command::new(program).args(args).output().ok()?;
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        Some(text)
    }

    fn run(program: &str, args: &[String]) -> Result<(), String> {
        let out = Command::new(program).args(args).output().map_err(|e| format!("{program}: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Err(format!("{program} {} failed: {}", args.join(" "), said.trim()))
    }

    /// The uid of the account logged in at the console (an agent's session), if any.
    fn console_uid() -> Option<u32> {
        output("stat", &["-f".into(), "%u".into(), "/dev/console".into()])?.trim().parse().ok()
    }

    /// The installed kind (MN4 a or b), from the plist that exists.
    pub fn installed(opts: &Options) -> Option<Kind> {
        launchd::installed_kind(&opts.name, |p| p.exists(), console_uid())
    }

    fn uid_of(account: &str) -> Option<u32> {
        output("id", &["-u".into(), account.to_string()])?.trim().parse().ok()
    }

    fn loaded(kind: Kind, opts: &Options) -> bool {
        output("launchctl", &launchd::print_args(kind, &opts.name))
            .is_some_and(|t| launchd::parse_print_state(&t) != "not loaded")
    }

    fn need_root(kind: Kind, opts: &Options, what: &str) -> Result<(), String> {
        // SAFETY: a plain system call.
        let me = unsafe { geteuid() };
        if me == 0 || kind == Kind::Agent(me) {
            return Ok(());
        }
        Err(format!(
            "the {} daemon is installed and only root can {what} it: sudo ragon server {what}",
            launchd::label_for(&opts.name)
        ))
    }

    /// `server start` with the daemon installed: load it (RunAtLoad starts it), or start a loaded one.
    pub fn start_daemon(opts: &Options) -> Result<(), i32> {
        let kind = installed(opts).unwrap_or(Kind::Daemon);
        let started = need_root(kind, opts, "start").and_then(|()| {
            let args = if loaded(kind, opts) {
                launchd::kickstart_args(kind, &opts.name)
            } else {
                launchd::bootstrap_args(kind, &opts.name)
            };
            run("launchctl", &args)
        });
        started.map_err(|e| {
            eprintln!("server start: {e}");
            1
        })
    }

    /// `server stop` with the daemon installed: unload it. launchd sends the supervisor SIGTERM and
    /// it stops the engine; the daemon stays stopped until `server start` or the next boot.
    pub fn stop_daemon(opts: &Options) -> Result<(), i32> {
        let kind = installed(opts).unwrap_or(Kind::Daemon);
        let stopped = need_root(kind, opts, "stop").and_then(|()| {
            if loaded(kind, opts) { run("launchctl", &launchd::bootout_args(kind, &opts.name)) } else { Ok(()) }
        });
        if let Err(e) = stopped {
            eprintln!("server stop: {e}");
            return Err(1);
        }
        if !wait_for(opts.wait_s.max(30), || !supervisor_running(opts)) {
            eprintln!("server stop: the supervisor did not stop");
            return Err(1);
        }
        Ok(())
    }

    pub fn install(opts: &Options) -> i32 {
        match install_steps(opts) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("server install: {e}");
                1
            }
        }
    }

    fn install_steps(opts: &Options) -> Result<(), String> {
        if !is_root() {
            return Err("needs root: sudo ragon server install -- <engine> [args...]".into());
        }
        // MN4 (a): the role account, never a person's (the installer creates it, a hidden system
        // user). MN4 (b): the login account whose session runs the agent (automatic login).
        let agent = opts.launchd.as_deref() == Some("agent") || (opts.launchd.is_none() && !launchd::MN4_DAEMON);
        let default_account =
            if agent { std::env::var("SUDO_USER").unwrap_or_default() } else { launchd::ROLE_ACCOUNT.to_string() };
        let account = opts.account.clone().unwrap_or(default_account);
        let Some(uid) = uid_of(&account) else {
            return Err(format!("the account {account:?} does not exist; the installer creates the role account"));
        };
        let kind = launchd::kind_to_install(opts.launchd.as_deref(), Some(uid))?;
        let state = &opts.state_dir;
        let fail = |what: &str, e: std::io::Error| format!("{what} {}: {e}", state.display());
        std::fs::create_dir_all(state).map_err(|e| fail("cannot create", e))?;
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o755)).map_err(|e| fail("cannot chmod", e))?;
        run("chown", &[account.clone(), state.display().to_string()])?;
        // The mains settings from before the first install, kept (root's file) for the uninstall;
        // a reinstall keeps the original values.
        let saved = state.join(power::SAVED_FILE);
        if !saved.exists() {
            let current = output("pmset", &["-g".into(), "custom".into()]).ok_or("pmset -g custom failed")?;
            std::fs::write(&saved, power::saved_text(&power::parse_custom_ac(&current)))
                .map_err(|e| fail("cannot save the power settings in", e))?;
        }
        run("pmset", &power::set_args())?;
        let exe = std::env::current_exe().map_err(|e| format!("cannot find this program: {e}"))?;
        let mut words = vec![exe.display().to_string()];
        words.extend(run_args(opts, false));
        let state_text = state.display().to_string();
        let daemon = match kind {
            Kind::Daemon => launchd::Daemon { user_name: account.clone(), ..launchd::Daemon::new(&opts.name, words, &state_text) },
            Kind::Agent(_) => launchd::Daemon::agent(&opts.name, words, &state_text),
        };
        // A reinstall, also from the other kind: unload and remove what is there first.
        if let Some(old) = installed(opts) {
            if loaded(old, opts) {
                run("launchctl", &launchd::bootout_args(old, &opts.name))?;
            }
            if old != kind {
                let _ = std::fs::remove_file(launchd::plist_path(old, &opts.name));
            }
        }
        let path = launchd::plist_path(kind, &opts.name);
        std::fs::write(&path, launchd::plist(&daemon)).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .map_err(|e| format!("cannot chmod {}: {e}", path.display()))?;
        if kind != Kind::Agent(uid) || console_uid() == Some(uid) {
            run("launchctl", &launchd::bootstrap_args(kind, &opts.name))?;
        } else {
            println!("{account} is not logged in: the agent starts at its next (automatic) login");
        }
        println!("installed {} as {account}: {}", daemon.label, path.display());
        println!("power on mains: pmset {}", power::set_args().join(" "));
        Ok(())
    }

    pub fn uninstall(opts: &Options) -> i32 {
        match uninstall_steps(opts) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("server uninstall: {e}");
                1
            }
        }
    }

    fn uninstall_steps(opts: &Options) -> Result<(), String> {
        if !is_root() {
            return Err("needs root: sudo ragon server uninstall".into());
        }
        // Whichever kind is installed (MN4 a or b); the saved power settings are restored either way.
        if let Some(kind) = installed(opts) {
            if loaded(kind, opts) {
                run("launchctl", &launchd::bootout_args(kind, &opts.name))?;
            }
            let path = launchd::plist_path(kind, &opts.name);
            std::fs::remove_file(&path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
        }
        let saved = opts.state_dir.join(power::SAVED_FILE);
        if let Ok(text) = std::fs::read_to_string(&saved) {
            if let Some(args) = power::restore_args(&power::parse_saved(&text)) {
                run("pmset", &args)?;
                println!("power on mains restored: pmset {}", args.join(" "));
            }
            let _ = std::fs::remove_file(&saved);
        }
        println!("removed {}; the state directory {} is kept", launchd::label_for(&opts.name), opts.state_dir.display());
        Ok(())
    }
}

/// One argument quoted the way `CommandLineToArgvW` and the MSVC runtime read it back.
pub fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\u{b}', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// A service command line: the program path always quoted (a path with spaces and no quotes is the
/// classic unquoted-service-path hole), the arguments quoted when they need it.
pub fn windows_command_line(words: &[String]) -> String {
    let mut out = String::new();
    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if i == 0 {
            out.push('"');
            out.push_str(word);
            out.push('"');
        } else {
            out.push_str(&quote_windows_arg(word));
        }
    }
    out
}

/// The state directory of a supervisor, for the drills.
pub fn status_path(state_dir: &Path) -> PathBuf {
    state_dir.join(STATUS_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::State;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn parses_options_and_the_engine_after_the_separator() {
        let o = parse(&args(&["run", "--state-dir", "/tmp/s", "--port", "18123", "--", "engine", "--port", "18123"]))
            .unwrap();
        assert_eq!(o.command, "run");
        assert_eq!(o.name, "ragon-node");
        assert_eq!(o.port, 18123);
        assert_eq!(o.state_dir, PathBuf::from("/tmp/s"));
        assert_eq!(o.engine, args(&["engine", "--port", "18123"]));
        assert!(parse(&args(&["run", "--port", "1"])).unwrap_err().contains("needs the engine"));
        assert!(parse(&args(&["bogus"])).is_err());
        assert!(parse(&args(&["status", "--port"])).unwrap_err().contains("needs a value"));
        assert!(parse(&args(&["status"])).is_ok());
    }

    #[test]
    fn run_args_round_trip_through_parse() {
        let o = parse(&args(&["start", "--state-dir", "/x y/s", "--port", "9", "--start-timeout", "20", "--", "e", "a b"]))
            .unwrap();
        let again = parse(&run_args(&o, true)[1..]).unwrap();
        assert_eq!(again.command, "run");
        assert!(again.service);
        assert_eq!(again.state_dir, o.state_dir);
        assert_eq!(again.port, 9);
        assert_eq!(again.start_timeout_s, Some(20));
        assert_eq!(again.engine, o.engine);
    }

    #[test]
    fn windows_quoting() {
        assert_eq!(quote_windows_arg("plain"), "plain");
        assert_eq!(quote_windows_arg(""), "\"\"");
        assert_eq!(quote_windows_arg("a b"), "\"a b\"");
        assert_eq!(quote_windows_arg(r"C:\Program Data\x\"), r#""C:\Program Data\x\\""#);
        assert_eq!(quote_windows_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(
            windows_command_line(&args(&[r"C:\Ragon\bin\ragon.exe", "server", "run", "--state-dir", r"C:\Program Data\R"])),
            r#""C:\Ragon\bin\ragon.exe" server run --state-dir "C:\Program Data\R""#
        );
    }

    fn file(state: State, supervisor_pid: Option<u32>, updated: u64) -> String {
        let snap = Snapshot {
            state,
            engine_pid: Some(4242),
            restarts: 2,
            last_exit: Some(-1),
            next_start_s: None,
            last_error: None,
        };
        status_file_text("ragon-node", "http://127.0.0.1:18123/v1/models", &snap, supervisor_pid, updated)
    }

    const NOW: u64 = 1_795_600_800;

    #[test]
    fn status_output_is_stable() {
        let (text, code) =
            status_report("ragon-node", "running", Some(&file(State::Healthy, Some(77), NOW - 2)), NOW, Some(200), "unused");
        assert_eq!(
            text,
            "service: ragon-node\n\
             service_state: running\n\
             supervisor: running\n\
             state: healthy\n\
             supervisor_pid: 77\n\
             engine_pid: 4242\n\
             url: http://127.0.0.1:18123/v1/models\n\
             probe: ok\n\
             restarts: 2\n\
             last_exit: -1\n\
             next_start_s: none\n\
             last_error: none\n\
             updated: 2026-11-25T09:59:58Z\n"
        );
        assert_eq!(code, 0);
    }

    #[test]
    fn status_without_a_supervisor_has_the_same_keys() {
        let keys = |t: &str| core::parse_status(t).into_iter().map(|(k, _)| k).collect::<Vec<_>>();
        let (live, _) = status_report("ragon-node", "n/a", Some(&file(State::Healthy, Some(77), NOW)), NOW, Some(200), "u");
        let (absent, code) = status_report("ragon-node", "not installed", None, NOW, None, "http://127.0.0.1:8000/v1/models");
        assert_eq!(keys(&live), keys(&absent));
        assert_eq!(code, 3);
        assert!(absent.contains("\nsupervisor: absent\nstate: stopped\nsupervisor_pid: none\nengine_pid: none\n"));
        assert!(absent.contains("\nurl: http://127.0.0.1:8000/v1/models\nprobe: no answer\n"));
        let (stale, code) = status_report("ragon-node", "n/a", Some(&file(State::Healthy, Some(77), NOW - 60)), NOW, None, "u");
        assert!(stale.contains("\nsupervisor: stale\nstate: stopped\n"));
        assert_eq!(code, 3);
        let (stopped, _) = status_report("ragon-node", "n/a", Some(&file(State::Stopped, None, NOW)), NOW, None, "u");
        assert!(stopped.contains("\nsupervisor: stopped\nstate: stopped\n"));
        let (unhealthy_probe, code) =
            status_report("ragon-node", "n/a", Some(&file(State::Healthy, Some(77), NOW)), NOW, Some(401), "u");
        assert!(unhealthy_probe.contains("\nprobe: http 401\n"));
        assert_eq!(code, 3);
    }

    #[test]
    fn on_battery_the_engine_still_serves_and_the_status_says_so() {
        use crate::macos_power::{Battery, status_on_battery};
        let low = Battery { charge: Some(12), admitting: false };
        let on_battery = status_on_battery(&file(State::Healthy, Some(77), NOW), &low);
        let (text, code) = status_report("ragon-node", "running", Some(&on_battery), NOW, Some(200), "u");
        assert_eq!(code, 0, "serving on battery is not a failure");
        assert!(text.contains("\nsupervisor: running\nstate: battery\n"), "{text}");
        assert!(text.ends_with("\nupdated: 2026-11-25T10:00:00Z\ncharge: 12\nadmission: closed\n"), "{text}");
        let (_, code) = status_report("ragon-node", "running", Some(&on_battery), NOW, None, "u");
        assert_eq!(code, 3, "the probe still decides");
        // A stale file from a supervisor on battery: the 13 shared keys only.
        let (stale, _) = status_report("ragon-node", "n/a", Some(&on_battery), NOW + 60, None, "u");
        assert!(stale.ends_with("\nupdated: 2026-11-25T10:00:00Z\n"), "{stale}");
    }
}
