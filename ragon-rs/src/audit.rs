//! `ragon audit egress` on Windows (specs/DREAMFERENCE_RAGON_WINDOWS_ARM.md §14,
//! specs/DREAMFERENCE_WINDOWS_NODE.md §7): one real `ragon exec` session in a throwaway repository
//! and `CODEX_HOME`, and the node this machine runs, if any, traced together over ETW and judged
//! as the node's `ragon-admin audit egress` judges its strace (specs/DREAMFERENCE_RAGON_EGRESS.md
//! §3.2). `--node` audits the running node alone, over a short window.
//!
//! Exit 0 is a pass, 1 a failure, 2 a trace that showed nothing (never a pass), 3 nothing to
//! audit yet: no model server known or answering, no node running. The events, the verdicts and
//! the recording are in [`trace`]; this file decides what to audit and prints the report. On
//! Linux the command points at `ragon-admin audit egress`.

mod trace;

use std::net::IpAddr;
use std::path::Path;
use std::path::PathBuf;

pub use trace::*;

#[cfg_attr(not(windows), allow(dead_code))]
const PROMPT: &str = "Reply with exactly: pong";
const USAGE: &str = "Usage: ragon audit egress [--host <model server URL>] [--node] [--state-dir <node state directory>] [--keep]";

/// Why a session cannot be audited on a machine that knows no model server.
const NO_HOST: &str = "no model server is known here: choose a node (ragon node use <address>), or pass --host <URL>";

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    host: Option<String>,
    /// The running node alone, without a session.
    node_only: bool,
    state_dir: Option<PathBuf>,
    keep: bool,
}

fn parse(args: &[String]) -> Option<Options> {
    let mut words = args.iter().map(String::as_str);
    if words.next() != Some("egress") {
        return None;
    }
    let mut options = Options::default();
    while let Some(word) = words.next() {
        match word {
            "--host" => options.host = Some(words.next()?.to_string()),
            "--node" => options.node_only = true,
            "--state-dir" => options.state_dir = Some(PathBuf::from(words.next()?)),
            "--keep" => options.keep = true,
            _ => return None,
        }
    }
    if options.host.as_deref().is_some_and(|host| model_endpoint(host).is_none()) || (options.node_only && options.host.is_some()) {
        return None;
    }
    Some(options)
}

/// What one trace recorded: the events (the node's snapshot included), the session's process id
/// and, when it did not reply, why, and whether the status file's supervisor pid named a running ragon
/// process when the trace began.
#[cfg_attr(not(windows), allow(dead_code))]
struct Recording {
    events: Vec<Event>,
    session: Option<(u32, Option<String>)>,
    supervisor_running: bool,
}

/// Runs `ragon audit …` and returns the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let Some(options) = parse(args) else {
        println!("{USAGE}");
        return 2;
    };
    if !cfg!(windows) {
        println!("On a node the egress audit is `ragon-admin audit egress` (strace); this one is for Windows.");
        return 2;
    }
    // The node this machine runs: judged with the session, or alone with --node. A machine
    // without one (a client) is audited as before.
    let state_dir = options.state_dir.clone().unwrap_or_else(default_state_dir);
    let status_text = std::fs::read_to_string(state_dir.join(STATUS_FILE)).ok();
    let mut not_run = Vec::new();
    let node = match node_status(status_text.as_deref(), unix_now()) {
        Ok(node) => Some(node),
        Err(why) => {
            if options.node_only {
                not_run.push(format!("{why}: start it (ragon server start), or pass --state-dir <dir> for a node kept elsewhere"));
            }
            None
        }
    };
    let target = if options.node_only {
        None
    } else {
        match session_target(options.host.clone()) {
            Ok(target) => Some(target),
            Err(why) => {
                not_run.push(why);
                None
            }
        }
    };
    if target.is_none() && node.is_none() {
        println!("{}", Status::NotRun.mark());
        for why in &not_run {
            println!("   - {why}");
        }
        return Status::NotRun.exit_code();
    }
    match windows::record(target.as_ref().map(|(host, _)| host.as_str()), node.as_ref(), options.keep) {
        Ok(recording) => {
            let (status, lines) = verdict(&recording, target.as_ref(), node.as_ref(), &not_run);
            for line in lines {
                println!("{line}");
            }
            status.exit_code()
        }
        Err(reason) => {
            println!("{}", Status::TraceFailed.mark());
            println!("   - {reason}");
            Status::TraceFailed.exit_code()
        }
    }
}

/// The model server a session is audited against, or why there is none to audit against: none
/// known, or one that does not answer. Asked before tracing, so this connection is not judged; a
/// server that is not there would have the session wait out its whole timeout for nothing.
fn session_target(host: Option<String>) -> Result<(String, (Option<IpAddr>, u16)), String> {
    let inputs = crate::node::inputs();
    let Some(host) = host.or_else(|| known_host(&inputs)) else {
        return Err(NO_HOST.to_string());
    };
    let Some(model) = model_endpoint(&host) else {
        return Err(format!("{host} is not a model server URL: pass --host <URL>"));
    };
    if let (Some(address), port) = model
        && std::net::TcpStream::connect_timeout(&std::net::SocketAddr::new(address, port), std::time::Duration::from_secs(5)).is_err()
    {
        return Err(not_answering(&host, inputs.is_node));
    }
    Ok((host, model))
}

/// Why a session was not audited against a server that does not answer, with the remedy that fits
/// the machine: a node starts its own server; a client chooses a node that runs.
fn not_answering(host: &str, is_node: bool) -> String {
    if is_node {
        format!(
            "nothing answers at {host}, this node's own model server: start it (ragon server start), or pass --host <URL>; a computer that uses a node on the network instead removes its node-id file first"
        )
    } else {
        format!("the model server at {host} does not answer: start it, choose another node (ragon node use <address>), or pass --host <URL>")
    }
}

/// Why a session gave no reply, from what it printed: its last error line (a model server that
/// refuses requests, for example while it runs a benchmark), else that it timed out or ended.
#[cfg_attr(not(windows), allow(dead_code))]
fn no_reply_reason(printed: &str, timed_out: Option<std::time::Duration>) -> String {
    const LIMIT: usize = 300;
    let error = printed.lines().rev().map(str::trim).find(|line| line.contains("ERROR") || line.contains("unexpected status"));
    match (error, timed_out) {
        (Some(line), _) => {
            let line = line.split_once("ERROR").map_or(line, |(_, rest)| rest.trim_start_matches(':').trim());
            if line.chars().count() > LIMIT {
                format!("{}…", line.chars().take(LIMIT).collect::<String>())
            } else {
                line.to_string()
            }
        }
        (None, Some(after)) => format!("it timed out after {} s", after.as_secs()),
        (None, None) => "it ended without one".to_string(),
    }
}

fn word(status: Status) -> &'static str {
    match status {
        Status::Pass => "pass",
        Status::Fail => "fail",
        Status::TraceFailed => "trace failed",
        Status::NotRun => "not run",
    }
}

/// The verdict and the report. A session alone reads as it always has; with a node, the session
/// and the node each get a section and the worst verdict is the audit's.
fn verdict(
    recording: &Recording,
    target: Option<&(String, (Option<IpAddr>, u16))>,
    node: Option<&NodeStatus>,
    not_run: &[String],
) -> (Status, Vec<String>) {
    let judged_session = target.zip(recording.session.as_ref()).map(|((host, model), (root, no_reply))| {
        let session = session(&recording.events, *root);
        let (status, problems) = judge(&session, *model, no_reply.as_deref());
        (host, *model, session, status, problems)
    });
    let Some(node) = node else {
        let Some((_, model, session, status, problems)) = judged_session else {
            return (Status::TraceFailed, vec![Status::TraceFailed.mark().to_string(), "   - the trace recorded no session".to_string()]);
        };
        return (status, render(&session, model, status, &problems));
    };
    let mut overall = if not_run.is_empty() { Status::Pass } else { Status::NotRun };
    let mut body = Vec::new();
    match &judged_session {
        Some((host, model, session, status, problems)) => {
            overall = overall.worst(*status);
            body.push(format!("Session (ragon exec against {host}): {}", word(*status)));
            body.extend(problems.iter().map(|problem| format!("   - {problem}")));
            body.extend(render_tree(session, *model, "model server", "   "));
        }
        None if target.is_some() => {
            overall = overall.worst(Status::TraceFailed);
            body.push("Session: trace failed".to_string());
            body.push("   - the trace recorded no session".to_string());
        }
        None if !not_run.is_empty() => {
            body.push("Session: not run".to_string());
            body.extend(not_run.iter().map(|why| format!("   - {why}")));
        }
        None => {}
    }
    let engine = node.engine_pid.map_or("none (between restarts)".to_string(), |pid| pid.to_string());
    let tree = observe(&recording.events, &node.roots(), &[node.port]);
    let (status, problems) = if recording.supervisor_running {
        judge_node(&tree, node.port)
    } else {
        (Status::TraceFailed, vec![format!(
            "process {} named in the status file is not a running ragon supervisor, so the node's tree could not be found",
            node.supervisor_pid
        )])
    };
    overall = overall.worst(status);
    body.push(format!("Node (supervisor pid {}, engine pid {engine}, {}): {}", node.supervisor_pid, node.url, word(status)));
    body.extend(problems.iter().map(|problem| format!("   - {problem}")));
    body.extend(render_tree(&tree, (Some(IpAddr::from([127, 0, 0, 1])), node.port), "this node's port", "   "));
    let mut lines = vec![overall.mark().to_string()];
    lines.extend(body);
    lines.extend(not_judged(true));
    (overall, lines)
}

/// The model server this machine uses, found without a browse: a browse is a multicast DNS query,
/// and the audit is about to count those.
fn known_host(inputs: &crate::node::Inputs) -> Option<String> {
    match crate::node::resolve(inputs, &NoBrowse) {
        crate::node::Resolution::Host(host) => Some(host),
        crate::node::Resolution::LastAddress { node, .. } => Some(node.model_url()),
        crate::node::Resolution::Overlay { node } => Some(node.model_url()),
        _ => None,
    }
}

struct NoBrowse;

impl crate::node::Browser for NoBrowse {
    fn browse(&self, _wanted: Option<&crate::node::Node>) -> Vec<crate::node::Advert> {
        Vec::new()
    }
}

/// The throwaway repository, `CODEX_HOME` and configuration a session runs with, so nothing of
/// the user's influences the result and nothing of theirs is touched. The code index is off: its
/// indexers run detached and outlive the session, so they are not part of what this can show.
#[cfg_attr(not(windows), allow(dead_code))]
fn prepare(work: &Path, host: &str) -> std::io::Result<()> {
    let repo = work.join("repo");
    std::fs::create_dir_all(&repo)?;
    std::fs::create_dir_all(work.join("home"))?;
    std::fs::write(repo.join("README.md"), "A throwaway repository for `ragon audit egress`.\n")?;
    let git = ["-c", "user.name=audit", "-c", "user.email=audit@localhost", "-c", "commit.gpgsign=false"];
    for step in [&["init", "-q"][..], &["add", "-A"], &["commit", "-qm", "audit"]] {
        let _ = std::process::Command::new("git").args(git).args(step).current_dir(&repo).output();
    }
    let host = toml::Value::String(host.to_string());
    std::fs::write(work.join("config.toml"), format!("vllm_host = {host}\ncode_index_enabled = false\n"))
}

#[cfg(not(windows))]
mod windows {
    pub(super) fn record(_host: Option<&str>, _node: Option<&super::NodeStatus>, _keep: bool) -> Result<super::Recording, String> {
        Err("the ETW trace exists only on Windows".to_string())
    }
}

#[cfg(windows)]
mod windows {
    use std::collections::HashSet;
    use std::io::Read;
    use std::io::Write;
    use std::time::Duration;
    use std::time::Instant;

    use super::Event;
    use super::NodeStatus;
    use super::Recording;
    use super::trace::etw;

    const SESSION_TIMEOUT: Duration = Duration::from_secs(300);
    /// How long the trace runs before the session starts and after it ends, for ETW's buffers.
    const SETTLE: Duration = Duration::from_secs(3);
    /// How long `--node` watches the node: several of the supervisor's probe ticks.
    const NODE_WINDOW: Duration = Duration::from_secs(20);

    /// Traces the session against `host` (when given) and the running `node` (when given).
    pub(super) fn record(host: Option<&str>, node: Option<&NodeStatus>, keep: bool) -> Result<Recording, String> {
        let work = std::env::temp_dir().join(format!("ragon-audit-{}", std::process::id()));
        if let Some(host) = host {
            let _ = std::fs::remove_dir_all(&work);
            super::prepare(&work, host).map_err(|err| format!("could not prepare {}: {err}", work.display()))?;
        }
        let result = traced(host, node, &work);
        if host.is_some() {
            if keep {
                println!("Kept: {}", work.display());
            } else {
                let _ = std::fs::remove_dir_all(&work);
            }
        }
        result
    }

    fn traced(host: Option<&str>, node: Option<&NodeStatus>, work: &std::path::Path) -> Result<Recording, String> {
        let trace = etw::Trace::start()?;
        // The node runs already: its tree comes from a snapshot taken now that the trace runs, so a
        // child started in between is in one or the other.
        let (mut events, supervisor_running) = node.map_or((Vec::new(), false), |node| etw::snapshot(&node.roots()));
        std::thread::sleep(SETTLE);
        let session = match host {
            Some(host) => match session(host, work) {
                Ok(session) => Some(session),
                Err(err) => {
                    let _ = trace.finish();
                    return Err(err);
                }
            },
            None => {
                // One request from a client, answered on the node's port, beside the supervisor's
                // own probes.
                if let Some(node) = node {
                    ask(&node.url);
                }
                std::thread::sleep(NODE_WINDOW);
                None
            }
        };
        std::thread::sleep(SETTLE);
        let seeded: HashSet<u32> =
            events.iter().filter_map(|event| if let Event::Start { pid, .. } = event { Some(*pid) } else { None }).collect();
        events.extend(trace.finish().into_iter().filter(|event| !matches!(event, Event::Start { pid, .. } if seeded.contains(pid))));
        Ok(Recording { events, session, supervisor_running })
    }

    /// Runs one `ragon exec` session; returns its process id and, when it did not reply, why
    /// (the last error it printed, or that it timed out).
    fn session(host: &str, work: &std::path::Path) -> Result<(u32, Option<String>), String> {
        let exe = std::env::current_exe().map_err(|err| format!("could not find ragon.exe: {err}"))?;
        let reply = work.join("reply.txt");
        let log = std::fs::File::create(work.join("session.log"))
            .map_err(|err| format!("could not create the session log: {err}"))?;
        let mut child = std::process::Command::new(&exe)
            .args(["exec", "--skip-git-repo-check", "-o"])
            .arg(&reply)
            .arg(super::PROMPT)
            .current_dir(work.join("repo"))
            .env("CODEX_HOME", work.join("home"))
            .env("DREAMFERENCE_CONFIG_PATH", work.join("config.toml"))
            .env("DREAMFERENCE_VLLM_HOST", host)
            .env_remove("RAGON_NODE")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", exe.display()))?;
        let root = child.id();
        let started = Instant::now();
        let mut timed_out = false;
        while matches!(child.try_wait(), Ok(None)) {
            if started.elapsed() > SESSION_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if std::fs::read_to_string(&reply).is_ok_and(|text| !text.trim().is_empty()) {
            return Ok((root, None));
        }
        let printed = std::fs::read_to_string(work.join("session.log")).unwrap_or_default();
        Ok((root, Some(super::no_reply_reason(&printed, timed_out.then_some(SESSION_TIMEOUT)))))
    }

    /// One plain HTTP GET of `url`, its answer read and dropped.
    fn ask(url: &str) {
        let Some((Some(address), port)) = super::model_endpoint(url) else { return };
        let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
        let path = rest.find('/').map_or("/", |at| &rest[at..]);
        let timeout = Duration::from_secs(5);
        if let Ok(mut stream) = std::net::TcpStream::connect_timeout(&std::net::SocketAddr::new(address, port), timeout) {
            let _ = stream.set_read_timeout(Some(timeout));
            let _ = write!(stream, "GET {path} HTTP/1.0\r\nHost: {address}:{port}\r\nConnection: close\r\n\r\n");
            let _ = stream.read_to_end(&mut Vec::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: u32 = 100;
    const MODEL: (Option<IpAddr>, u16) = (Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)), 8000);

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    fn start(pid: u32, parent: u32, image: &str) -> Event {
        Event::Start { pid, parent, image: image.to_string() }
    }

    fn connect(pid: u32, address: &str, port: u16, local_port: u16) -> Event {
        Event::Connect { pid, address: address.parse().unwrap(), port, local_port, udp: false }
    }

    fn node() -> NodeStatus {
        NodeStatus { supervisor_pid: 500, engine_pid: Some(501), url: "http://127.0.0.1:8000/v1/models".to_string(), port: 8000 }
    }

    /// A session against this machine's node, and the node: the supervisor's probe, the engine's
    /// answers to the probe and to the session.
    fn node_events() -> Vec<Event> {
        vec![
            start(500, 0, "ragon.exe"),
            start(501, 500, "ragon-serve.exe"),
            start(ROOT, 42, "ragon.exe"),
            connect(500, "127.0.0.1", 8000, 50_000),
            connect(ROOT, "127.0.0.1", 8000, 50_001),
            Event::Accept { pid: 501, local_port: 8000 },
            connect(501, "127.0.0.1", 50_000, 8000),
            connect(501, "127.0.0.1", 50_001, 8000),
        ]
    }

    #[test]
    fn a_command_line_without_egress_prints_the_usage_and_exits_2() {
        assert_eq!(run_cli(&[]), 2);
        assert_eq!(run_cli(&["other".to_string()]), 2);
    }

    #[test]
    fn the_command_line_is_read() {
        assert_eq!(parse(&args(&["egress"])), Some(Options::default()));
        assert_eq!(
            parse(&args(&["egress", "--node", "--state-dir", "D:\\node", "--keep"])),
            Some(Options { node_only: true, state_dir: Some(PathBuf::from("D:\\node")), keep: true, ..Options::default() })
        );
        assert_eq!(parse(&args(&["egress", "--host", "http://127.0.0.1:8000"])).unwrap().host.as_deref(), Some("http://127.0.0.1:8000"));
        // A missing value, an unreadable URL, an unknown flag, and --node with a session's --host.
        for bad in [&["egress", "--host"][..], &["egress", "--host", "http://h:x"], &["egress", "--state-dir"], &["egress", "--all"], &[
            "egress",
            "--node",
            "--host",
            "http://127.0.0.1:8000",
        ]] {
            assert_eq!(parse(&args(bad)), None, "{bad:?}");
        }
    }

    #[test]
    fn a_server_that_does_not_answer_is_not_run_with_the_remedy_of_the_machines_role() {
        let node = not_answering("http://localhost:8000", true);
        assert!(node.contains("ragon server start") && node.contains("node-id"), "{node}");
        let client = not_answering("http://192.168.1.20:8000", false);
        assert!(client.contains("ragon node use") && !client.contains("ragon server start"), "{client}");
        assert!(!NO_HOST.contains("trace failed"));
    }

    #[test]
    fn a_session_alone_reads_as_before() {
        let recording = Recording {
            events: vec![start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000, 50_001)],
            session: Some((ROOT, None)),
            supervisor_running: false,
        };
        let target = ("http://127.0.0.1:8000".to_string(), MODEL);
        let (status, lines) = verdict(&recording, Some(&target), None, &[]);
        assert_eq!(status, Status::Pass);
        assert_eq!(lines[0], "✅ Egress audit: pass");
        assert_eq!(lines[1], "Network destinations:");
        assert!(!lines.iter().any(|line| line.contains("Node") || line.contains("WSL")));
    }

    #[test]
    fn a_session_and_the_node_are_judged_from_one_trace() {
        let recording = Recording { events: node_events(), session: Some((ROOT, None)), supervisor_running: true };
        let target = ("http://localhost:8000".to_string(), MODEL);
        let (status, lines) = verdict(&recording, Some(&target), Some(&node()), &[]);
        assert_eq!(status, Status::Pass, "{lines:#?}");
        assert!(lines.iter().any(|line| line == "Session (ragon exec against http://localhost:8000): pass"));
        assert!(lines.iter().any(|line| line.starts_with("Node (supervisor pid 500, engine pid 501") && line.ends_with(": pass")));
        assert!(lines.iter().any(|line| line.contains("WSL distribution")));
        // The engine reaching out fails the whole audit, though the session was clean.
        let mut events = node_events();
        events.push(connect(501, "192.0.2.1", 443, 50_002));
        let recording = Recording { events, session: Some((ROOT, None)), supervisor_running: true };
        let (status, lines) = verdict(&recording, Some(&target), Some(&node()), &[]);
        assert_eq!(status, Status::Fail);
        assert!(lines.iter().any(|line| line.contains("192.0.2.1:443") && line.contains("not on this machine")));
    }

    #[test]
    fn the_node_alone_is_judged_and_a_session_not_run_is_said() {
        let mut events = node_events();
        events.retain(|event| !matches!(event, Event::Start { pid: ROOT, .. } | Event::Connect { pid: ROOT, .. }));
        let recording = Recording { events: events.clone(), session: None, supervisor_running: true };
        assert_eq!(verdict(&recording, None, Some(&node()), &[]).0, Status::Pass);
        // A plain audit on a node whose session could not run: the node is judged, the audit is
        // not a pass.
        let why = not_answering("http://localhost:8000", true);
        let (status, lines) = verdict(&recording, None, Some(&node()), &[why.clone()]);
        assert_eq!(status, Status::NotRun);
        assert!(lines.iter().any(|line| line == "Session: not run") && lines.iter().any(|line| line.ends_with(&why)));
        // A status file whose supervisor pid names no running ragon process: nothing was seen.
        let recording = Recording { events, session: None, supervisor_running: false };
        assert_eq!(verdict(&recording, None, Some(&node()), &[]).0, Status::TraceFailed);
    }

    #[test]
    fn a_session_without_a_reply_says_why_and_is_not_run() {
        let printed = "2026-11-25T20:01:00Z ERROR codex_core: unexpected status 503 Service Unavailable: The model is running a benchmark\n";
        let why = no_reply_reason(printed, None);
        assert_eq!(why, "codex_core: unexpected status 503 Service Unavailable: The model is running a benchmark");
        assert_eq!(no_reply_reason("", Some(std::time::Duration::from_secs(180))), "it timed out after 180 s");
        assert_eq!(no_reply_reason("", None), "it ended without one");
        assert!(no_reply_reason(&format!("ERROR: {}", "x".repeat(400)), None).chars().count() <= 301);
        let target = ("http://127.0.0.1:8000".to_string(), MODEL);
        let events = vec![start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000, 50_001)];
        let recording = Recording { events, session: Some((ROOT, Some(why))), supervisor_running: false };
        let (status, lines) = verdict(&recording, Some(&target), None, &[]);
        assert_eq!(status, Status::NotRun, "{lines:#?}");
        assert!(lines.iter().any(|line| line.contains("503 Service Unavailable")), "{lines:#?}");
    }
}
