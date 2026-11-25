//! What `ragon audit egress` records and how it judges it (specs/DREAMFERENCE_RAGON_WINDOWS_ARM.md
//! §14, specs/DREAMFERENCE_WINDOWS_NODE.md §7): the events, the process trees, the verdicts, the
//! report, the node's status file, and on Windows the ETW recording and the process snapshot.
//!
//! Two trees are judged, from one trace:
//! - **the session**: one `ragon exec` and everything it starts. A pass needs every destination to
//!   be the model server or an allow-listed loopback port, no name looked up, no networked git;
//! - **the node** on this machine, when its supervisor runs (`ragon server`): the supervisor, the
//!   engine and whatever they started, sidecars included. Already running, so the tree is seeded
//!   from the supervisor's `status.txt` and a process snapshot taken after the trace started. A
//!   server answers its clients: traffic on a port the tree serves (the status file's port, or
//!   one the trace saw it accept on) is listed as served, not judged. Everything else the tree
//!   reaches is judged as the session's is, with the node's own port in place of the model
//!   server's.
//!
//! Nothing here depends on the rest of the launcher, so the verdict is tested on every platform
//! and the recording can be built on its own. A trace that saw nothing is never a pass.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::net::IpAddr;

/// Loopback services a session may reach besides the model server (RAGON_EGRESS §4.3).
pub const GMAIL_PORT: u16 = 8767;
pub const SEARXNG_PORT: u16 = 8888;

/// Where the launcher points `chatgpt_base_url` (`OFFLINE_CHATGPT_BASE_URL`): a connect there is a
/// call to the upstream vendor's backend that no patch closed.
pub const BLACKHOLE_PORT: u16 = 9;

/// Names a resolver answers on the machine itself; a lookup of one leaves nothing.
const LOCAL_NAMES: &[&str] = &["localhost"];

/// Helpers git starts to reach a remote: their start is a networked git command.
const NETWORKED_GIT: &[&str] = &["git-remote-https.exe", "git-remote-http.exe", "git-remote-ftp.exe", "git-remote-ftps.exe", "ssh.exe"];

/// The supervisor's status file and its age limit, as `ragon-rs/supervisor/src/cli.rs` writes and
/// reads them (`STATUS_FILE`, `STALE_AFTER_S`; a test compares them). A file older than
/// this is from a supervisor that is gone, and its pids may belong to other programs by now.
pub const STATUS_FILE: &str = "status.txt";
pub const STALE_AFTER_S: u64 = 15;

/// What the trace recorded, reduced to what the verdict reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A process started; `image` is its file name. The node's processes that were already
    /// running when the trace began come from a snapshot, in the same form.
    Start { pid: u32, parent: u32, image: String },
    /// A process connected to, or sent to, an address. `local_port` is the port on this machine
    /// (0 when the event did not carry it): for a server, its own port.
    Connect { pid: u32, address: IpAddr, port: u16, local_port: u16, udp: bool },
    /// A process accepted a TCP connection on `local_port`: it serves that port.
    Accept { pid: u32, local_port: u16 },
    /// A process asked the DNS client for a name.
    Lookup { pid: u32, name: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    TraceFailed,
    /// Nothing to audit yet (no model server known or answering, no node running): nothing was
    /// traced, so this is neither a pass nor a trace that failed.
    NotRun,
}

impl Status {
    pub fn exit_code(self) -> i32 {
        match self {
            Status::Pass => 0,
            Status::Fail => 1,
            Status::TraceFailed => 2,
            Status::NotRun => 3,
        }
    }

    pub fn mark(self) -> &'static str {
        match self {
            Status::Pass => "✅ Egress audit: pass",
            Status::Fail => "❌ Egress audit: fail",
            Status::TraceFailed => "⚠️ Egress audit: trace failed",
            Status::NotRun => "ℹ️ Egress audit: not run",
        }
    }

    /// The worse of two verdicts: a failure over a trace that failed over a pass.
    pub fn worst(self, other: Status) -> Status {
        let rank = |status: Status| match status {
            Status::Pass => 0,
            Status::NotRun => 1,
            Status::TraceFailed => 2,
            Status::Fail => 3,
        };
        if rank(other) > rank(self) { other } else { self }
    }
}

/// The model server's address and port, from its base URL. `None` for the address when the host is
/// a name other than `localhost`: a session that has to look it up fails the audit on the lookup.
pub fn model_endpoint(host: &str) -> Option<(Option<IpAddr>, u16)> {
    let (scheme, rest) = host.split_once("://").unwrap_or(("http", host));
    let authority = rest.split('/').next()?;
    let (name, port) = match authority.strip_prefix('[') {
        Some(v6) => {
            let (name, rest) = v6.split_once(']')?;
            (name, rest.strip_prefix(':'))
        }
        None => match authority.rsplit_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (authority, None),
        },
    };
    let port = match port {
        Some(port) => port.parse().ok()?,
        None if scheme == "https" => 443,
        None => 80,
    };
    let address = if name.eq_ignore_ascii_case("localhost") { Some(IpAddr::from([127, 0, 0, 1])) } else { name.parse().ok() };
    Some((address, port))
}

pub fn is_loopback(address: &IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback()),
    }
}

fn display(address: &IpAddr, port: u16) -> String {
    match address {
        IpAddr::V4(v4) => format!("{v4}:{port}"),
        IpAddr::V6(v6) => format!("[{v6}]:{port}"),
    }
}

/// `roots` and everything they started, however deep.
pub fn process_tree(events: &[Event], roots: &[u32]) -> HashSet<u32> {
    let mut tree: HashSet<u32> = roots.iter().copied().collect();
    loop {
        let before = tree.len();
        for event in events {
            if let Event::Start { pid, parent, .. } = event
                && tree.contains(parent)
            {
                tree.insert(*pid);
            }
        }
        if tree.len() == before {
            return tree;
        }
    }
}

/// What one tree did.
#[derive(Debug, Default)]
pub struct Session {
    /// Destination to the number of events.
    pub destinations: BTreeMap<(IpAddr, u16), usize>,
    pub lookups: BTreeMap<String, usize>,
    /// Image to the number of starts (snapshot entries included).
    pub processes: BTreeMap<String, usize>,
    /// Peers the tree answered on a port it serves, to the number of events: not judged.
    pub served: BTreeMap<IpAddr, usize>,
    /// The ports the tree serves.
    pub serving: Vec<u16>,
}

/// The session's tree: `root` and everything it started. It serves nothing.
pub fn session(events: &[Event], root: u32) -> Session {
    observe(events, &[root], &[])
}

/// A tree's traffic. TCP on a port in `served` (or one the tree was seen accepting on) is its
/// answers to clients, listed in [`Session::served`]; the rest is what the tree reached.
pub fn observe(events: &[Event], roots: &[u32], served: &[u16]) -> Session {
    let tree = process_tree(events, roots);
    let mut serving: Vec<u16> = served.to_vec();
    for event in events {
        if let Event::Accept { pid, local_port } = event
            && tree.contains(pid)
            && *local_port != 0
            && !serving.contains(local_port)
        {
            serving.push(*local_port);
        }
    }
    serving.sort_unstable();
    let mut session = Session { serving, ..Session::default() };
    for event in events {
        match event {
            Event::Start { pid, image, .. } if tree.contains(pid) => {
                *session.processes.entry(image.clone()).or_default() += 1;
            }
            Event::Connect { pid, address, local_port, udp: false, .. }
                if tree.contains(pid) && *local_port != 0 && session.serving.contains(local_port) =>
            {
                *session.served.entry(*address).or_default() += 1;
            }
            Event::Connect { pid, address, port, .. } if tree.contains(pid) => {
                *session.destinations.entry((*address, *port)).or_default() += 1;
            }
            Event::Lookup { pid, name } if tree.contains(pid) => {
                *session.lookups.entry(name.trim_end_matches('.').to_lowercase()).or_default() += 1;
            }
            _ => {}
        }
    }
    session
}

/// Whether `address:port` is the model server at `model` (a loopback model is reached at any
/// loopback address).
fn is_endpoint(model: (Option<IpAddr>, u16), address: &IpAddr, port: u16) -> bool {
    let (model_address, model_port) = model;
    port == model_port && (model_address == Some(*address) || (model_address.is_some_and(|a| is_loopback(&a)) && is_loopback(address)))
}

/// The problems in a tree's traffic, and how many events reached `model` (the allowed endpoint).
fn problems(session: &Session, model: (Option<IpAddr>, u16)) -> (Vec<String>, usize) {
    let mut problems = Vec::new();
    let mut model_events = 0;
    for ((address, port), count) in &session.destinations {
        let target = display(address, *port);
        if is_endpoint(model, address, *port) {
            model_events += count;
        } else if !is_loopback(address) {
            problems.push(format!("connected to {target} ({count}x): not on this machine"));
        } else if *port == BLACKHOLE_PORT {
            problems.push(format!(
                "connected to {target} ({count}x): a call to the upstream vendor's backend that no patch closes (it failed here only because the launcher redirects that URL)"
            ));
        } else if *port != GMAIL_PORT && *port != SEARXNG_PORT {
            problems.push(format!("connected to {target} ({count}x): a local port that is not on the allowlist"));
        }
    }
    for (name, count) in &session.lookups {
        if !LOCAL_NAMES.contains(&name.as_str()) {
            problems.push(format!("asked a resolver for {name} ({count}x)"));
        }
    }
    for image in session.processes.keys() {
        if NETWORKED_GIT.contains(&image.to_lowercase().as_str()) {
            problems.push(format!("started {image}, which git runs to reach a remote"));
        }
    }
    (problems, model_events)
}

/// The session's verdict and its reasons. `model` is the model server's endpoint; `no_reply` is
/// `None` when the session answered and otherwise why it did not. A session without a reply did
/// not exercise ragon, so it is not run rather than a failed trace: the model server refused it
/// (a benchmark night answers 503) or was too slow. A forbidden connection fails it all the same.
pub fn judge(session: &Session, model: (Option<IpAddr>, u16), no_reply: Option<&str>) -> (Status, Vec<String>) {
    let (problems, model_events) = problems(session, model);
    if !problems.is_empty() {
        return (Status::Fail, problems);
    }
    if let Some(why) = no_reply {
        return (Status::NotRun, vec![format!(
            "the session got no reply ({why}), so it did not exercise ragon: run the audit again when the model server takes requests"
        )]);
    }
    if model_events == 0 {
        return (Status::TraceFailed, vec![
            "no connection to the model server was recorded, although the session replied: the trace did not see this session's network events".to_string(),
        ]);
    }
    (Status::Pass, Vec::new())
}

/// The node's verdict. The node may reach its own port on loopback (the supervisor's probe) and
/// the allow-listed loopback services; what it answers on the ports it serves is not judged. The
/// supervisor probes its engine every few seconds, so a trace that recorded no connection to the
/// node's own port did not see the node's network events: never a pass.
pub fn judge_node(session: &Session, node_port: u16) -> (Status, Vec<String>) {
    let own = (Some(IpAddr::from([127, 0, 0, 1])), node_port);
    let (problems, own_events) = problems(session, own);
    if !problems.is_empty() {
        return (Status::Fail, problems);
    }
    if own_events == 0 {
        return (Status::TraceFailed, vec![format!(
            "the supervisor's probe of its engine (loopback port {node_port}) was not recorded: the trace did not see the node's network events"
        )]);
    }
    (Status::Pass, Vec::new())
}

/// The distribution's verdict (WINDOWS_ARM §14 W2, WINDOWS_NODE §7): the sidecars and the forge
/// in the WSL distribution reach the network through the WSL host process, so their audit is a
/// strace run inside the distribution, merged here. It waits for the owner's WN2 answer (which
/// route runs the sidecars); until then there is no verdict and the report says what is not
/// judged. A node's pass is therefore a pass of the supervisor's tree only.
pub fn distribution_verdict() -> Option<(Status, Vec<String>)> {
    None
}

/// One tree's part of the report: what it reached, what it served, its lookups and processes.
/// `endpoint` names the allowed endpoint (`model server`, `this node's port`).
pub fn render_tree(session: &Session, model: (Option<IpAddr>, u16), endpoint: &str, indent: &str) -> Vec<String> {
    let mut lines = vec![format!("{indent}Network destinations:")];
    if session.destinations.is_empty() {
        lines.push(format!("{indent}   none"));
    }
    for ((address, port), count) in &session.destinations {
        let label = if is_endpoint(model, address, *port) {
            endpoint
        } else if !is_loopback(address) {
            "not on this machine"
        } else {
            match *port {
                GMAIL_PORT => "Gmail search service",
                SEARXNG_PORT => "SearXNG",
                _ => "not on the allowlist",
            }
        };
        lines.push(format!("{indent}   {:<24} {count:>4}x  {label}", display(address, *port)));
    }
    if !session.serving.is_empty() {
        let ports: Vec<String> = session.serving.iter().map(u16::to_string).collect();
        let peers: Vec<String> = session.served.iter().map(|(peer, count)| format!("{peer} ({count}x)")).collect();
        lines.push(format!(
            "{indent}Served on port {} (answers to clients, not judged): {}",
            ports.join(", "),
            if peers.is_empty() { "none".to_string() } else { peers.join(", ") }
        ));
    }
    let names: Vec<String> = session.lookups.iter().map(|(name, count)| format!("{name} ({count}x)")).collect();
    lines.push(format!("{indent}DNS lookups: {}", if names.is_empty() { "none".to_string() } else { names.join(", ") }));
    let programs: Vec<String> = session.processes.iter().map(|(image, count)| format!("{image} ({count})")).collect();
    lines.push(format!("{indent}Processes: {}", if programs.is_empty() { "none".to_string() } else { programs.join(", ") }));
    lines
}

/// What the verdict never covers.
pub fn not_judged(node: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if node && distribution_verdict().is_none() {
        lines.push(
            "Not judged yet: the WSL distribution's traffic (sidecars, forge); its audit joins this one once the sidecar route (WN2) is settled."
                .to_string(),
        );
    }
    lines.push(
        "Not judged: Windows' own traffic (Defender, Windows Update, telemetry) is outside these processes, so \"ragon made no connection\" is not \"this computer made none\". Defender's automatic sample submission can upload an unknown program, ragon's own included, to Microsoft."
            .to_string(),
    );
    lines
}

/// The report of a session audit: the verdict and anything unexpected first, then everything
/// seen, then what the verdict does not cover.
pub fn render(session: &Session, model: (Option<IpAddr>, u16), status: Status, problems: &[String]) -> Vec<String> {
    let mut lines = vec![status.mark().to_string()];
    lines.extend(problems.iter().map(|problem| format!("   - {problem}")));
    lines.extend(render_tree(session, model, "model server", ""));
    lines.extend(not_judged(false));
    lines
}

/// The running node, from the supervisor's status file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeStatus {
    pub supervisor_pid: u32,
    pub engine_pid: Option<u32>,
    /// The URL the supervisor probes, and its port: the port the node serves.
    pub url: String,
    pub port: u16,
}

impl NodeStatus {
    pub fn roots(&self) -> Vec<u32> {
        let mut roots = vec![self.supervisor_pid];
        roots.extend(self.engine_pid);
        roots
    }
}

/// Reads the supervisor's status file (`key: value` lines, `none` for an absent value). `Err`
/// says why there is no running node to audit: no file, a supervisor that stopped, or a file not
/// updated for longer than [`STALE_AFTER_S`] (the supervisor is gone; its pids may be reused).
pub fn node_status(text: Option<&str>, now_unix: u64) -> Result<NodeStatus, String> {
    let Some(text) = text else {
        return Err("no node supervisor has run here (no status file)".to_string());
    };
    let get = |key: &str| -> Option<String> {
        text.lines()
            .filter_map(|line| line.split_once(": "))
            .find(|(k, _)| k.trim() == key)
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty() && v != "none")
    };
    let Some(supervisor_pid) = get("supervisor_pid").and_then(|pid| pid.parse().ok()) else {
        return Err("the node supervisor is stopped".to_string());
    };
    match get("updated_unix").and_then(|t| t.parse::<u64>().ok()) {
        Some(updated) if now_unix.saturating_sub(updated) <= STALE_AFTER_S => {}
        _ => {
            return Err(format!(
                "the node supervisor's status was last written at {}, so it is not running",
                get("updated").unwrap_or_else(|| "an unknown time".to_string())
            ));
        }
    }
    let url = get("url").ok_or("the node supervisor's status names no URL")?;
    let (_, port) = model_endpoint(&url).ok_or_else(|| format!("the node supervisor's status has an unreadable URL: {url}"))?;
    Ok(NodeStatus { supervisor_pid, engine_pid: get("engine_pid").and_then(|pid| pid.parse().ok()), url, port })
}

/// Where the supervisor keeps its status file by default (`ragon-rs/supervisor/src/cli.rs`
/// `default_state_dir`, on Windows): `%ProgramData%\Ragon\node`.
pub fn default_state_dir() -> std::path::PathBuf {
    let base = std::env::var_os("ProgramData").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from(r"C:\ProgramData"));
    base.join("Ragon").join("node")
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(windows)]
pub mod etw {
    //! The recording: one ETW real-time session with three providers, which needs Administrator
    //! rights or the Performance Log Users group: Microsoft-Windows-Kernel-Process (to follow the
    //! trees), Kernel-Network (TCP connects, sends and accepts, UDP sends) and DNS-Client (every
    //! lookup, which strace on Linux could not name). And the process snapshot that seeds the
    //! node's tree, taken after the trace started so no child is lost in between.

    use std::net::IpAddr;
    use std::sync::Arc;
    use std::sync::Mutex;

    use ferrisetw::EventRecord;
    use ferrisetw::SchemaLocator;
    use ferrisetw::UserTrace;
    use ferrisetw::parser::Parser;
    use ferrisetw::provider::Provider;

    use super::Event;

    const KERNEL_PROCESS: &str = "22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716";
    const KERNEL_NETWORK: &str = "7dd42a49-5329-4832-8dfd-43d979153a88";
    const DNS_CLIENT: &str = "1c95126e-7eea-49a9-a3fe-a378b03ddb4d";
    /// WINEVENT_KEYWORD_PROCESS, and Kernel-Network's IPv4 and IPv6 keywords.
    const PROCESS_KEYWORD: u64 = 0x10;
    const NETWORK_KEYWORDS: u64 = 0x30;
    /// Kernel-Process: ProcessStart.
    const PROCESS_START: u16 = 1;
    /// Kernel-Network: TCP send and connect (IPv4, IPv6), UDP send (IPv4, IPv6).
    const TCP_OUT: [u16; 4] = [10, 12, 26, 28];
    const UDP_OUT: [u16; 2] = [42, 58];
    /// Kernel-Network: TCP connection accepted (IPv4, IPv6).
    const TCP_ACCEPT: [u16; 2] = [15, 31];
    /// DNS-Client: a query started.
    const DNS_QUERY: u16 = 3006;

    type Sink = Arc<Mutex<Vec<Event>>>;

    fn push(sink: &Sink, event: Event) {
        sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(event);
    }

    fn process(sink: Sink) -> Provider {
        Provider::by_guid(KERNEL_PROCESS)
            .any(PROCESS_KEYWORD)
            .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                if record.event_id() != PROCESS_START {
                    return;
                }
                let Ok(schema) = locator.event_schema(record) else { return };
                let parser = Parser::create(record, &schema);
                let (Ok(pid), Ok(parent)) = (parser.try_parse::<u32>("ProcessID"), parser.try_parse::<u32>("ParentProcessID")) else {
                    return;
                };
                let image: String = parser.try_parse("ImageName").unwrap_or_default();
                let image = image.rsplit(['\\', '/']).next().unwrap_or_default().to_string();
                push(&sink, Event::Start { pid, parent, image });
            })
            .build()
    }

    fn network(sink: Sink) -> Provider {
        Provider::by_guid(KERNEL_NETWORK)
            .any(NETWORK_KEYWORDS)
            .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                let id = record.event_id();
                let udp = UDP_OUT.contains(&id);
                let accept = TCP_ACCEPT.contains(&id);
                if !udp && !accept && !TCP_OUT.contains(&id) {
                    return;
                }
                let Ok(schema) = locator.event_schema(record) else { return };
                let parser = Parser::create(record, &schema);
                let Ok(pid) = parser.try_parse::<u32>("PID") else { return };
                // Ports are recorded in network byte order; `sport` is this machine's end.
                let local_port = parser.try_parse::<u16>("sport").map(u16::from_be).unwrap_or(0);
                if accept {
                    push(&sink, Event::Accept { pid, local_port });
                    return;
                }
                let (Ok(address), Ok(port)) = (parser.try_parse::<IpAddr>("daddr"), parser.try_parse::<u16>("dport")) else {
                    return;
                };
                push(&sink, Event::Connect { pid, address, port: u16::from_be(port), local_port, udp });
            })
            .build()
    }

    fn dns(sink: Sink) -> Provider {
        Provider::by_guid(DNS_CLIENT)
            .add_callback(move |record: &EventRecord, locator: &SchemaLocator| {
                if record.event_id() != DNS_QUERY {
                    return;
                }
                let Ok(schema) = locator.event_schema(record) else { return };
                let parser = Parser::create(record, &schema);
                if let Ok(name) = parser.try_parse::<String>("QueryName") {
                    push(&sink, Event::Lookup { pid: record.process_id(), name });
                }
            })
            .build()
    }

    /// A running trace.
    pub struct Trace {
        trace: UserTrace,
        sink: Sink,
    }

    impl Trace {
        pub fn start() -> Result<Trace, String> {
            let sink: Sink = Arc::default();
            let trace = UserTrace::new()
                .named(format!("RagonEgressAudit{}", std::process::id()))
                .enable(process(sink.clone()))
                .enable(network(sink.clone()))
                .enable(dns(sink.clone()))
                .start_and_process()
                .map_err(|err| {
                    format!("could not start an ETW trace ({err:?}): it needs Administrator rights or the Performance Log Users group")
                })?;
            Ok(Trace { trace, sink })
        }

        /// Stops the trace and returns what it recorded.
        pub fn finish(self) -> Vec<Event> {
            let _ = self.trace.stop();
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::mem::take(&mut *self.sink.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
        }
    }

    /// The processes running now that descend from `roots` (roots included), as `Start` events,
    /// and whether the first root is running with an image named `ragon…` (the supervisor:
    /// `ragon.exe server run`, or the drills' `ragon-supervisor.exe`). A process is taken as a
    /// child only when it started after its parent, so a parent id that now names another
    /// program does not pull that program's children in.
    pub fn snapshot(roots: &[u32]) -> (Vec<Event>, bool) {
        let processes = running();
        let created = |pid: u32| processes.iter().find(|p| p.0 == pid).and_then(|p| p.3);
        let mut tree: Vec<u32> = roots.iter().copied().filter(|root| processes.iter().any(|p| p.0 == *root)).collect();
        let mut events: Vec<Event> = processes
            .iter()
            .filter(|p| tree.contains(&p.0))
            .map(|p| Event::Start { pid: p.0, parent: 0, image: p.2.clone() })
            .collect();
        loop {
            let before = tree.len();
            for (pid, parent, image, started) in &processes {
                if tree.contains(pid) || !tree.contains(parent) || pid == parent {
                    continue;
                }
                if let (Some(child), Some(parent_started)) = (started, created(*parent))
                    && *child < parent_started
                {
                    continue;
                }
                tree.push(*pid);
                events.push(Event::Start { pid: *pid, parent: *parent, image: image.clone() });
            }
            if tree.len() == before {
                break;
            }
        }
        let supervisor = roots
            .first()
            .and_then(|root| processes.iter().find(|p| p.0 == *root))
            .is_some_and(|p| p.2.to_lowercase().starts_with("ragon"));
        (events, supervisor)
    }

    /// Every running process: pid, parent pid, image, creation time (when it can be read).
    fn running() -> Vec<(u32, u32, String, Option<u64>)> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::CreateToolhelp32Snapshot;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::PROCESSENTRY32W;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32FirstW;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::Process32NextW;
        use windows_sys::Win32::System::Diagnostics::ToolHelp::TH32CS_SNAPPROCESS;
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        use windows_sys::Win32::System::Threading::OpenProcess;
        use windows_sys::Win32::System::Threading::PROCESS_QUERY_LIMITED_INFORMATION;

        let mut out = Vec::new();
        // SAFETY: plain Win32 calls on a snapshot handle this function owns and closes; the entry
        // is zeroed and its size set as Process32FirstW requires.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return out;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut more = Process32FirstW(snap, &mut entry) != 0;
            while more {
                let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
                let image = String::from_utf16_lossy(&entry.szExeFile[..len]);
                let pid = entry.th32ProcessID;
                let mut started = None;
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle != 0 {
                    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
                    let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
                    if GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) != 0 {
                        started = Some((u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime));
                    }
                    CloseHandle(handle);
                }
                out.push((pid, entry.th32ParentProcessID, image, started));
                more = Process32NextW(snap, &mut entry) != 0;
            }
            CloseHandle(snap);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: u32 = 100;
    const MODEL: (Option<IpAddr>, u16) = (Some(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)), 8000);

    /// The node: supervisor 500, engine 501 (its child), a sidecar 502 started by the engine.
    const SUPERVISOR: u32 = 500;
    const ENGINE: u32 = 501;
    const NODE_PORT: u16 = 18123;

    fn start(pid: u32, parent: u32, image: &str) -> Event {
        Event::Start { pid, parent, image: image.to_string() }
    }

    fn connect(pid: u32, address: &str, port: u16) -> Event {
        Event::Connect { pid, address: address.parse().unwrap(), port, local_port: 50_000, udp: false }
    }

    /// A send from `local_port` (an answer, when the tree serves that port).
    fn send_from(pid: u32, local_port: u16, address: &str, port: u16) -> Event {
        Event::Connect { pid, address: address.parse().unwrap(), port, local_port, udp: false }
    }

    /// A clean node: the snapshot's supervisor and engine, the supervisor's probes of its engine,
    /// the engine's answers to the probe and to a client on the LAN, and a sidecar the engine
    /// started that reaches SearXNG on loopback.
    fn clean_node() -> Vec<Event> {
        vec![
            start(SUPERVISOR, 0, "ragon.exe"),
            start(ENGINE, SUPERVISOR, "ragon-standin-engine.exe"),
            connect(SUPERVISOR, "127.0.0.1", NODE_PORT),
            Event::Accept { pid: ENGINE, local_port: NODE_PORT },
            send_from(ENGINE, NODE_PORT, "127.0.0.1", 50_001),
            send_from(ENGINE, NODE_PORT, "192.168.1.50", 61_000),
            start(502, ENGINE, "sidecar.exe"),
            connect(502, "127.0.0.1", SEARXNG_PORT),
            // Outside the node's tree: a client on this machine and Defender.
            start(900, 1, "MsMpEng.exe"),
            connect(900, "20.190.1.1", 443),
        ]
    }

    #[test]
    fn the_model_servers_url_gives_its_address_and_port() {
        assert_eq!(model_endpoint("http://127.0.0.1:8000"), Some((Some([127, 0, 0, 1].into()), 8000)));
        assert_eq!(model_endpoint("http://localhost:30000/v1"), Some((Some([127, 0, 0, 1].into()), 30000)));
        assert_eq!(model_endpoint("http://192.168.1.20:8000/"), Some((Some([192, 168, 1, 20].into()), 8000)));
        assert_eq!(model_endpoint("http://[::1]:8000"), Some((Some("::1".parse().unwrap()), 8000)));
        assert_eq!(model_endpoint("https://spark.local"), Some((None, 443)));
        assert_eq!(model_endpoint("http://host:port"), None);
    }

    #[test]
    fn only_the_sessions_tree_is_judged() {
        let events = [
            start(ROOT, 1, "ragon.exe"),
            start(101, ROOT, "codex-command-runner.exe"),
            start(102, 101, "cmd.exe"),
            start(200, 1, "MsMpEng.exe"),
            connect(ROOT, "127.0.0.1", 8000),
            connect(102, "127.0.0.1", 8888),
            // Defender, outside the tree.
            connect(200, "20.190.1.1", 443),
            Event::Lookup { pid: 200, name: "wdcp.microsoft.com".to_string() },
        ];
        let session = session(&events, ROOT);
        assert_eq!(process_tree(&events, &[ROOT]), HashSet::from([ROOT, 101, 102]));
        assert_eq!(judge(&session, MODEL, None), (Status::Pass, Vec::new()));
    }

    #[test]
    fn the_verdict_names_each_problem() {
        let events = [
            start(ROOT, 1, "ragon.exe"),
            start(101, ROOT, "git-remote-https.exe"),
            connect(ROOT, "127.0.0.1", 8000),
            connect(ROOT, "104.18.1.1", 443),
            connect(ROOT, "127.0.0.1", BLACKHOLE_PORT),
            connect(ROOT, "::1", 5000),
            Event::Lookup { pid: ROOT, name: "ab.chatgpt.com.".to_string() },
            Event::Lookup { pid: ROOT, name: "localhost".to_string() },
        ];
        let (status, problems) = judge(&session(&events, ROOT), MODEL, None);
        assert_eq!(status, Status::Fail);
        assert_eq!(problems.len(), 5, "{problems:#?}");
        assert!(problems[0].starts_with("connected to 104.18.1.1:443 (1x): not on this machine"));
        assert!(problems.iter().any(|problem| problem.contains("127.0.0.1:9") && problem.contains("upstream vendor")));
        assert!(problems.iter().any(|problem| problem.contains("[::1]:5000") && problem.contains("not on the allowlist")));
        assert!(problems.iter().any(|problem| problem == "asked a resolver for ab.chatgpt.com (1x)"));
        assert!(problems.iter().any(|problem| problem.contains("git-remote-https.exe")));
    }

    #[test]
    fn a_node_on_the_network_is_allowed_by_its_address_and_port() {
        let node = (Some(IpAddr::from([192, 168, 1, 20])), 8000);
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "192.168.1.20", 8000)];
        assert_eq!(judge(&session(&events, ROOT), node, None).0, Status::Pass);
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "192.168.1.20", 8000), connect(ROOT, "192.168.1.20", 22)];
        assert_eq!(judge(&session(&events, ROOT), node, None).0, Status::Fail);
        // A loopback model port is not allowed for a session whose model is elsewhere.
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "192.168.1.20", 8000), connect(ROOT, "127.0.0.1", 8000)];
        assert_eq!(judge(&session(&events, ROOT), node, None).0, Status::Fail);
    }

    #[test]
    fn a_trace_that_showed_nothing_is_never_a_pass() {
        // Replied, yet no connection to the model server seen: the network events were not read.
        let events = [start(ROOT, 1, "ragon.exe")];
        assert_eq!(judge(&session(&events, ROOT), MODEL, None).0, Status::TraceFailed);
        assert_eq!(Status::TraceFailed.exit_code(), 2);
    }

    #[test]
    fn a_session_the_model_server_refused_is_not_run_unless_it_connected_out() {
        let refused = "unexpected status 503 Service Unavailable: the model is running a benchmark";
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000)];
        let (status, problems) = judge(&session(&events, ROOT), MODEL, Some(refused));
        assert_eq!(status, Status::NotRun);
        assert!(problems[0].contains(refused), "{problems:?}");
        // No reply does not excuse a forbidden connection.
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000), connect(ROOT, "203.0.113.7", 443)];
        assert_eq!(judge(&session(&events, ROOT), MODEL, Some(refused)).0, Status::Fail);
    }

    #[test]
    fn the_report_says_what_the_verdict_does_not_cover() {
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000)];
        let session = session(&events, ROOT);
        let lines = render(&session, MODEL, Status::Pass, &[]);
        assert_eq!(lines[0], "✅ Egress audit: pass");
        assert!(lines.iter().any(|line| line.contains("127.0.0.1:8000") && line.ends_with("model server")));
        assert!(lines.last().unwrap().starts_with("Not judged: Windows' own traffic"));
        // A client's report says nothing of a distribution it does not have.
        assert!(!lines.iter().any(|line| line.contains("WSL")));
    }

    #[test]
    fn a_url_without_a_port_gets_its_schemes_default() {
        assert_eq!(model_endpoint("https://node.local/v1"), Some((None, 443)));
        assert_eq!(model_endpoint("http://node.local"), Some((None, 80)));
        assert_eq!(model_endpoint("10.0.0.5:8000"), Some((Some(IpAddr::from([10, 0, 0, 5])), 8000)));
    }

    #[test]
    fn a_malformed_model_url_gives_nothing() {
        assert_eq!(model_endpoint("http://host:notaport"), None);
        assert_eq!(model_endpoint("http://host:99999"), None);
        assert_eq!(model_endpoint("http://[::1/v1"), None);
    }

    #[test]
    fn a_bracketed_address_and_localhost_are_read() {
        assert_eq!(model_endpoint("http://[::1]:8000/v1"), Some((Some("::1".parse().unwrap()), 8000)));
        assert_eq!(model_endpoint("http://LocalHost:18000"), Some((Some(IpAddr::from([127, 0, 0, 1])), 18000)));
    }

    #[test]
    fn a_name_lookup_other_than_localhost_fails_the_audit() {
        let events = [
            start(ROOT, 1, "ragon.exe"),
            connect(ROOT, "127.0.0.1", 8000),
            Event::Lookup { pid: ROOT, name: "example.com".to_string() },
            Event::Lookup { pid: ROOT, name: "localhost".to_string() },
        ];
        let (status, problems) = judge(&session(&events, ROOT), MODEL, None);
        assert_eq!(status, Status::Fail);
        assert!(problems.iter().any(|p| p.contains("example.com")));
        assert!(!problems.iter().any(|p| p.contains("localhost")));
    }

    #[test]
    fn a_networked_git_helper_fails_the_audit() {
        let events = [start(ROOT, 1, "ragon.exe"), start(101, ROOT, "Git-Remote-HTTPS.exe"), connect(ROOT, "127.0.0.1", 8000)];
        let (status, problems) = judge(&session(&events, ROOT), MODEL, None);
        assert_eq!(status, Status::Fail);
        assert!(problems.iter().any(|p| p.contains("reach a remote")));
    }

    #[test]
    fn the_vendor_blackhole_port_is_named_in_its_problem() {
        let events = [start(ROOT, 1, "ragon.exe"), connect(ROOT, "127.0.0.1", 8000), connect(ROOT, "127.0.0.1", BLACKHOLE_PORT)];
        let (status, problems) = judge(&session(&events, ROOT), MODEL, None);
        assert_eq!(status, Status::Fail);
        assert!(problems.iter().any(|p| p.contains("upstream vendor")));
    }

    #[test]
    fn a_process_started_outside_the_tree_is_not_in_it() {
        let events = [start(ROOT, 1, "ragon.exe"), start(101, ROOT, "a.exe"), start(102, 101, "b.exe"), start(200, 1, "other.exe")];
        let tree = process_tree(&events, &[ROOT]);
        assert!(tree.contains(&102) && !tree.contains(&200));
    }

    #[test]
    fn a_clean_node_passes_and_its_answers_to_clients_are_not_judged() {
        let events = clean_node();
        let node = observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]);
        assert_eq!(judge_node(&node, NODE_PORT), (Status::Pass, Vec::new()));
        // The answers to the probe and to the LAN client are listed as served, not as destinations.
        assert_eq!(node.served.get(&"192.168.1.50".parse::<IpAddr>().unwrap()), Some(&1));
        assert!(!node.destinations.keys().any(|(address, _)| !is_loopback(address)));
        assert_eq!(node.processes.get("sidecar.exe"), Some(&1));
        // Defender is outside the tree.
        assert!(!node.destinations.contains_key(&("20.190.1.1".parse().unwrap(), 443)));
    }

    #[test]
    fn a_stand_in_engine_that_opens_a_forbidden_connection_fails_the_node() {
        let mut events = clean_node();
        events.push(connect(ENGINE, "192.0.2.1", 443));
        let (status, problems) = judge_node(&observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]), NODE_PORT);
        assert_eq!(status, Status::Fail);
        assert_eq!(problems, vec!["connected to 192.0.2.1:443 (1x): not on this machine".to_string()]);
        // A child the engine started during the trace is judged too, and so are its lookups.
        let mut events = clean_node();
        events.extend([start(503, 502, "helper.exe"), Event::Lookup { pid: 503, name: "telemetry.example".to_string() }]);
        let (status, problems) = judge_node(&observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]), NODE_PORT);
        assert_eq!(status, Status::Fail);
        assert!(problems.iter().any(|p| p.contains("telemetry.example")));
    }

    #[test]
    fn a_port_the_node_was_seen_accepting_on_is_served() {
        // The sidecar serves 3100 to the LAN; its answers there are not egress.
        let mut events = clean_node();
        events.extend([Event::Accept { pid: 502, local_port: 3100 }, send_from(502, 3100, "192.168.1.60", 55_000)]);
        let node = observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]);
        assert_eq!(node.serving, vec![3100, NODE_PORT]);
        assert_eq!(judge_node(&node, NODE_PORT).0, Status::Pass);
        // A UDP send from a served port is never an answer on a TCP connection.
        let mut events = clean_node();
        events.push(Event::Connect { pid: ENGINE, address: "192.168.1.50".parse().unwrap(), port: 5353, local_port: NODE_PORT, udp: true });
        assert_eq!(judge_node(&observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]), NODE_PORT).0, Status::Fail);
    }

    #[test]
    fn a_node_trace_without_the_supervisors_probe_is_never_a_pass() {
        let events: Vec<Event> =
            clean_node().into_iter().filter(|event| !matches!(event, Event::Connect { pid: SUPERVISOR, .. })).collect();
        let (status, problems) = judge_node(&observe(&events, &[SUPERVISOR, ENGINE], &[NODE_PORT]), NODE_PORT);
        assert_eq!(status, Status::TraceFailed);
        assert!(problems[0].contains("probe"));
    }

    #[test]
    fn the_node_report_lists_what_it_served_and_says_the_distribution_is_not_judged() {
        let node = observe(&clean_node(), &[SUPERVISOR, ENGINE], &[NODE_PORT]);
        let lines = render_tree(&node, (Some(IpAddr::from([127, 0, 0, 1])), NODE_PORT), "this node's port", "   ");
        assert!(lines.iter().any(|line| line.contains("127.0.0.1:18123") && line.ends_with("this node's port")));
        assert!(lines.iter().any(|line| line.contains("Served on port 18123") && line.contains("192.168.1.50 (1x)")));
        assert!(distribution_verdict().is_none());
        assert!(not_judged(true)[0].contains("WSL distribution"));
    }

    #[test]
    fn the_status_file_gives_the_running_nodes_pids_and_port() {
        let text = "name: ragon-node\nstate: healthy\nsupervisor_pid: 77\nengine_pid: 4242\nurl: http://127.0.0.1:18123/v1/models\n\
                    restarts: 0\nlast_exit: none\nnext_start_s: none\nlast_error: none\nupdated: 2026-11-25T10:00:00Z\nupdated_unix: 1000\n";
        let status = node_status(Some(text), 1010).unwrap();
        assert_eq!(status, NodeStatus { supervisor_pid: 77, engine_pid: Some(4242), url: "http://127.0.0.1:18123/v1/models".to_string(), port: 18123 });
        assert_eq!(status.roots(), vec![77, 4242]);
        // Between restarts there is no engine; the supervisor is still audited.
        let between = text.replace("engine_pid: 4242", "engine_pid: none");
        assert_eq!(node_status(Some(&between), 1010).unwrap().roots(), vec![77]);
    }

    #[test]
    fn a_missing_stopped_or_stale_status_file_is_no_running_node() {
        assert!(node_status(None, 0).unwrap_err().contains("no status file"));
        let stopped = "supervisor_pid: none\nengine_pid: none\nurl: http://127.0.0.1:8000/v1/models\nupdated_unix: 1000\n";
        assert!(node_status(Some(stopped), 1000).unwrap_err().contains("stopped"));
        let stale = "supervisor_pid: 77\nengine_pid: 42\nurl: http://127.0.0.1:8000/v1/models\nupdated: 2026-11-25T10:00:00Z\nupdated_unix: 1000\n";
        assert!(node_status(Some(stale), 1000 + STALE_AFTER_S).is_ok());
        assert!(node_status(Some(stale), 1001 + STALE_AFTER_S).unwrap_err().contains("2026-11-25T10:00:00Z"));
    }

    #[test]
    fn the_worst_verdict_wins_and_not_run_has_its_own_exit_code() {
        assert_eq!(Status::Pass.worst(Status::Fail), Status::Fail);
        assert_eq!(Status::TraceFailed.worst(Status::Pass), Status::TraceFailed);
        assert_eq!(Status::Fail.worst(Status::TraceFailed), Status::Fail);
        assert_eq!(Status::NotRun.exit_code(), 3);
        assert_eq!(Status::NotRun.mark(), "ℹ️ Egress audit: not run");
    }

    #[test]
    fn the_status_file_is_read_as_the_supervisor_writes_it() {
        assert_eq!(STATUS_FILE, ragon_supervisor::cli::STATUS_FILE);
        assert_eq!(STALE_AFTER_S, ragon_supervisor::cli::STALE_AFTER_S);
        if cfg!(windows) {
            assert_eq!(default_state_dir(), ragon_supervisor::cli::default_state_dir());
        }
        let snap = ragon_supervisor::core::Snapshot {
            state: ragon_supervisor::core::State::Healthy,
            engine_pid: Some(4242),
            restarts: 0,
            last_exit: None,
            next_start_s: None,
            last_error: None,
        };
        let text = ragon_supervisor::cli::status_file_text("ragon-node", "http://127.0.0.1:18123/v1/models", &snap, Some(77), 1000);
        assert_eq!(node_status(Some(&text), 1000).unwrap().roots(), vec![77, 4242]);
    }
}
