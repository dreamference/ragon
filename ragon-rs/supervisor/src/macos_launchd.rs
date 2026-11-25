//! The Mac node's LaunchDaemon (owner ruling MN1 (b), MN4 (a); specs/DREAMFERENCE_MAC_NODE.md
//! §4.2): `ai.dreamference.ragon-node` in the system domain, run as the role account `_ragon`,
//! started at boot without a login. launchd starts `ragon server run`, the supervisor in this
//! crate, which holds the engine as its child and restarts it with the core's backoff; launchd only
//! restarts the supervisor itself when it dies.
//!
//! Everything here is pure (the plist text and the `launchctl` argument lists) and is tested on
//! every platform; running `launchctl` (root) is in src/cli.rs, macOS only.

use std::path::{Path, PathBuf};

/// The role account the daemon runs as (created by the installer, never a person's account).
pub const ROLE_ACCOUNT: &str = "_ragon";
/// Where the installed node keeps its state (status, logs, the saved power settings).
pub const STATE_DIR: &str = "/Library/Application Support/Ragon/node";
/// The node's home: `HOME` of the daemon and its children, so the node id and the advertisement
/// (`.config/dreamference/...`) sit under `/Library/Application Support/Ragon` (§4.2), not under
/// the role account's `/var/empty`.
pub const NODE_HOME: &str = "/Library/Application Support/Ragon";
/// The directory of the system daemons' plists.
pub const DAEMONS_DIR: &str = "/Library/LaunchDaemons";
/// The directory of the agents every login session loads (root's, so the user cannot change it).
pub const AGENTS_DIR: &str = "/Library/LaunchAgents";
/// Seconds launchd waits before it starts a supervisor that died again (its own floor is 10).
pub const THROTTLE_S: u32 = 10;

/// Who owns the engine (owner ruling MN4). (a) [`Kind::Daemon`], the system domain as `_ragon`,
/// with nobody logged in; (b) [`Kind::Agent`], a LaunchAgent of the login session of the account
/// whose uid it carries, the fallback with automatic login (FileVault off).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Daemon,
    Agent(u32),
}

/// The one switch between MN4 (a) and (b): `true` until the probe mac-probe-metal-daemon shows
/// that a daemon has no Metal device; `server install --launchd agent` takes (b) on one machine.
pub const MN4_DAEMON: bool = true;

/// `ragon-node` -> `ai.dreamference.ragon-node`.
pub fn label_for(service: &str) -> String {
    format!("ai.dreamference.{service}")
}

/// `/Library/LaunchDaemons/ai.dreamference.ragon-node.plist` (an agent's under /Library/LaunchAgents).
pub fn plist_path(kind: Kind, service: &str) -> PathBuf {
    let dir = match kind {
        Kind::Daemon => DAEMONS_DIR,
        Kind::Agent(_) => AGENTS_DIR,
    };
    Path::new(dir).join(format!("{}.plist", label_for(service)))
}

/// The launchd domain: `system`, or `gui/<uid>` for an agent.
pub fn domain(kind: Kind) -> String {
    match kind {
        Kind::Daemon => "system".to_string(),
        Kind::Agent(uid) => format!("gui/{uid}"),
    }
}

/// `system/ai.dreamference.ragon-node`, the target of `launchctl print|kickstart|bootout`.
pub fn service_target(kind: Kind, service: &str) -> String {
    format!("{}/{}", domain(kind), label_for(service))
}

/// What the plist describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Daemon {
    pub label: String,
    /// The full command line: the launcher, then `server run ...`.
    pub program_arguments: Vec<String>,
    /// `UserName`: the role account; empty for an agent (it runs as the session's user).
    pub user_name: String,
    pub working_directory: String,
    /// `HOME` for the supervisor and its children, if set ([`NODE_HOME`] for the daemon).
    pub home: Option<String>,
    /// An agent's plist: loaded in login (Aqua) sessions only, no `UserName`.
    pub agent: bool,
    /// launchd's own stdout/stderr for the supervisor (the engine logs to its own file).
    pub log_path: String,
}

impl Daemon {
    pub fn new(service: &str, program_arguments: Vec<String>, state_dir: &str) -> Self {
        Daemon {
            label: label_for(service),
            program_arguments,
            user_name: ROLE_ACCOUNT.to_string(),
            working_directory: state_dir.to_string(),
            home: Some(NODE_HOME.to_string()),
            agent: false,
            log_path: format!("{}/launchd.log", state_dir.trim_end_matches('/')),
        }
    }

    /// MN4 (b): the same job as a LaunchAgent of the login session (its user's own `HOME`).
    pub fn agent(service: &str, program_arguments: Vec<String>, state_dir: &str) -> Self {
        Daemon { user_name: String::new(), home: None, agent: true, ..Daemon::new(service, program_arguments, state_dir) }
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

fn key_string(out: &mut String, key: &str, value: &str) {
    out.push_str(&format!("\t<key>{key}</key>\n\t<string>{}</string>\n", escape(value)));
}

/// The LaunchDaemon plist. `KeepAlive` restarts the supervisor only after a failure: a clean stop
/// (`server stop`, exit 0) stays stopped. An engine's refusal (exit 3) never reaches launchd: the
/// supervisor keeps running and reports `state: refused` without retrying. No `AbandonProcessGroup`:
/// launchd stopping the job also kills what is left of it (the engine's guard sees to the engine).
pub fn plist(daemon: &Daemon) -> String {
    let mut out = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n",
    );
    key_string(&mut out, "Label", &daemon.label);
    out.push_str("\t<key>ProgramArguments</key>\n\t<array>\n");
    for arg in &daemon.program_arguments {
        out.push_str(&format!("\t\t<string>{}</string>\n", escape(arg)));
    }
    out.push_str("\t</array>\n");
    if !daemon.agent {
        key_string(&mut out, "UserName", &daemon.user_name);
    }
    if let Some(home) = &daemon.home {
        out.push_str("\t<key>EnvironmentVariables</key>\n\t<dict>\n");
        out.push_str(&format!("\t\t<key>HOME</key>\n\t\t<string>{}</string>\n\t</dict>\n", escape(home)));
    }
    if daemon.agent {
        key_string(&mut out, "LimitLoadToSessionType", "Aqua");
    }
    key_string(&mut out, "WorkingDirectory", &daemon.working_directory);
    out.push_str("\t<key>RunAtLoad</key>\n\t<true/>\n");
    out.push_str("\t<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>\n");
    out.push_str(&format!("\t<key>ThrottleInterval</key>\n\t<integer>{THROTTLE_S}</integer>\n"));
    key_string(&mut out, "ProcessType", "Standard");
    key_string(&mut out, "StandardOutPath", &daemon.log_path);
    key_string(&mut out, "StandardErrorPath", &daemon.log_path);
    out.push_str("</dict>\n</plist>\n");
    out
}

/// `launchctl bootstrap system <plist>` (`gui/<uid>` for an agent): load (and, with RunAtLoad, start) it.
pub fn bootstrap_args(kind: Kind, service: &str) -> Vec<String> {
    vec!["bootstrap".into(), domain(kind), plist_path(kind, service).display().to_string()]
}

/// `launchctl bootout system/<label>`: stop (SIGTERM, the supervisor stops the engine) and unload.
pub fn bootout_args(kind: Kind, service: &str) -> Vec<String> {
    vec!["bootout".into(), service_target(kind, service)]
}

/// `launchctl kickstart system/<label>`: start a loaded daemon that is not running.
pub fn kickstart_args(kind: Kind, service: &str) -> Vec<String> {
    vec!["kickstart".into(), service_target(kind, service)]
}

/// `launchctl print system/<label>`: readable without root.
pub fn print_args(kind: Kind, service: &str) -> Vec<String> {
    vec!["print".into(), service_target(kind, service)]
}

/// The installed kind from which plist exists (the daemon's first) and the console user's uid.
pub fn installed_kind(service: &str, exists: impl Fn(&Path) -> bool, console_uid: Option<u32>) -> Option<Kind> {
    if exists(&plist_path(Kind::Daemon, service)) {
        return Some(Kind::Daemon);
    }
    let agent = Kind::Agent(console_uid.unwrap_or(0));
    exists(&plist_path(agent, service)).then_some(agent)
}

/// `server install --launchd daemon|agent`: the kind to install, [`MN4_DAEMON`] when not given.
/// An agent needs the uid of the account whose session runs it.
pub fn kind_to_install(choice: Option<&str>, uid: Option<u32>) -> Result<Kind, String> {
    match (choice, MN4_DAEMON) {
        (Some("daemon"), _) | (None, true) => Ok(Kind::Daemon),
        (Some("agent"), _) | (None, false) => {
            uid.filter(|&u| u != 0).map(Kind::Agent).ok_or_else(|| "an agent needs the login account (--account)".to_string())
        }
        (Some(other), _) => Err(format!("--launchd takes daemon or agent, not {other:?}")),
    }
}

/// The `service_state` value of `server status` from `launchctl print`'s output: `running`,
/// `waiting` (loaded, not running), `not loaded` (launchd does not know it), or `unknown`.
pub fn parse_print_state(output: &str) -> &'static str {
    if output.contains("Could not find service") {
        return "not loaded";
    }
    // The job's own `state = ...` is the first one at one tab of indent; nested blocks
    // (endpoints, the spawn's sub-dicts) indent deeper.
    for line in output.lines() {
        let indent = line.len() - line.trim_start_matches('\t').len();
        let Some(value) = line.trim().strip_prefix("state = ") else { continue };
        if indent != 1 {
            continue;
        }
        return match value.trim() {
            // `xpcproxy`: launchd is exec'ing the job (it already has a pid), seen for a moment at start.
            "running" | "xpcproxy" => "running",
            "not running" | "waiting" | "spawn scheduled" | "exited" => "waiting",
            _ => "unknown",
        };
    }
    "unknown"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon() -> Daemon {
        Daemon::new(
            "ragon-node",
            vec![
                "/Library/Application Support/Ragon/bin/ragon".into(),
                "server".into(),
                "run".into(),
                "--state-dir".into(),
                STATE_DIR.into(),
                "--".into(),
                "/Library/Application Support/Ragon/bin/ragon-serve".into(),
                "--model".into(),
                "a&b<c>".into(),
            ],
            STATE_DIR,
        )
    }

    #[test]
    fn the_names() {
        assert_eq!(label_for("ragon-node"), "ai.dreamference.ragon-node");
        assert_eq!(
            plist_path(Kind::Daemon, "ragon-node"),
            PathBuf::from("/Library/LaunchDaemons/ai.dreamference.ragon-node.plist")
        );
        assert_eq!(service_target(Kind::Daemon, "ragon-node"), "system/ai.dreamference.ragon-node");
        assert_eq!(
            plist_path(Kind::Agent(501), "ragon-node"),
            PathBuf::from("/Library/LaunchAgents/ai.dreamference.ragon-node.plist")
        );
        assert_eq!(service_target(Kind::Agent(501), "ragon-node"), "gui/501/ai.dreamference.ragon-node");
    }

    #[test]
    fn the_plist_is_a_daemon_for_the_role_account_restarted_only_after_a_failure() {
        let text = plist(&daemon());
        assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist"));
        assert!(text.contains("\t<key>Label</key>\n\t<string>ai.dreamference.ragon-node</string>\n"));
        assert!(text.contains("\t<key>UserName</key>\n\t<string>_ragon</string>\n"));
        assert!(text.contains("\t<key>RunAtLoad</key>\n\t<true/>\n"));
        assert!(text.contains("<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>\n\t</dict>"));
        assert!(text.contains("<key>ThrottleInterval</key>\n\t<integer>10</integer>"));
        assert!(text.contains(
            "<key>StandardErrorPath</key>\n\t<string>/Library/Application Support/Ragon/node/launchd.log</string>"
        ));
        assert!(!text.contains("AbandonProcessGroup"));
        assert!(!text.contains("LimitLoadToSessionType"), "a daemon, not an agent");
        assert!(text.contains(
            "\t<key>EnvironmentVariables</key>\n\t<dict>\n\t\t<key>HOME</key>\n\t\t<string>/Library/Application Support/Ragon</string>\n\t</dict>\n"
        ));
        assert!(text.ends_with("</dict>\n</plist>\n"));
    }

    #[test]
    fn the_arguments_are_listed_in_order_and_escaped() {
        let text = plist(&daemon());
        let args: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.contains("<key>ProgramArguments</key>"))
            .skip(2)
            .take_while(|l| !l.contains("</array>"))
            .collect();
        assert_eq!(args.len(), 9);
        assert_eq!(args[0], "\t\t<string>/Library/Application Support/Ragon/bin/ragon</string>");
        assert_eq!(args[1], "\t\t<string>server</string>");
        assert_eq!(args[8], "\t\t<string>a&amp;b&lt;c&gt;</string>");
        assert_eq!(escape("\"'"), "&quot;&apos;");
    }

    #[test]
    fn the_launchctl_arguments() {
        assert_eq!(
            bootstrap_args(Kind::Daemon, "ragon-node"),
            ["bootstrap", "system", "/Library/LaunchDaemons/ai.dreamference.ragon-node.plist"]
        );
        assert_eq!(bootout_args(Kind::Daemon, "ragon-node"), ["bootout", "system/ai.dreamference.ragon-node"]);
        assert_eq!(kickstart_args(Kind::Daemon, "ragon-node"), ["kickstart", "system/ai.dreamference.ragon-node"]);
        assert_eq!(print_args(Kind::Daemon, "ragon-node"), ["print", "system/ai.dreamference.ragon-node"]);
        let agent = Kind::Agent(501);
        assert_eq!(
            bootstrap_args(agent, "ragon-node"),
            ["bootstrap", "gui/501", "/Library/LaunchAgents/ai.dreamference.ragon-node.plist"]
        );
        assert_eq!(bootout_args(agent, "ragon-node"), ["bootout", "gui/501/ai.dreamference.ragon-node"]);
        assert_eq!(kickstart_args(agent, "ragon-node"), ["kickstart", "gui/501/ai.dreamference.ragon-node"]);
    }

    #[test]
    fn the_agent_plist_runs_in_the_login_session_without_a_user_name() {
        let d = daemon();
        let agent = Daemon::agent("ragon-node", d.program_arguments.clone(), STATE_DIR);
        let text = plist(&agent);
        assert!(!text.contains("UserName"), "an agent runs as the session's user");
        assert!(!text.contains("EnvironmentVariables"), "the user's own HOME");
        assert!(text.contains("\t<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>\n"));
        assert!(text.contains("\t<key>RunAtLoad</key>\n\t<true/>\n"));
        assert!(text.contains("<key>KeepAlive</key>\n\t<dict>\n\t\t<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert!(text.contains("<key>ThrottleInterval</key>\n\t<integer>10</integer>"));
    }

    #[test]
    fn one_switch_picks_the_daemon_and_install_can_take_the_agent() {
        const { assert!(MN4_DAEMON, "MN4 (a) until the Metal-from-a-daemon probe says otherwise") };
        assert_eq!(kind_to_install(None, Some(501)), Ok(Kind::Daemon));
        assert_eq!(kind_to_install(Some("daemon"), None), Ok(Kind::Daemon));
        assert_eq!(kind_to_install(Some("agent"), Some(501)), Ok(Kind::Agent(501)));
        assert!(kind_to_install(Some("agent"), None).is_err());
        assert!(kind_to_install(Some("agent"), Some(0)).is_err(), "never root's session");
        assert!(kind_to_install(Some("launchd"), Some(501)).is_err());
    }

    #[test]
    fn the_installed_kind_is_read_from_the_plist_that_exists() {
        let daemon_plist = plist_path(Kind::Daemon, "ragon-node");
        let agent_plist = plist_path(Kind::Agent(0), "ragon-node");
        assert_eq!(installed_kind("ragon-node", |p| p == daemon_plist, Some(501)), Some(Kind::Daemon));
        assert_eq!(installed_kind("ragon-node", |p| p == agent_plist, Some(501)), Some(Kind::Agent(501)));
        assert_eq!(installed_kind("ragon-node", |_| false, Some(501)), None);
    }

    #[test]
    fn the_state_from_launchctl_print() {
        // A service launchd does not know, as a normal user sees it (macOS 27).
        let missing = "Bad request.\nCould not find service \"ai.dreamference.ragon-node\" in domain for system\n";
        assert_eq!(parse_print_state(missing), "not loaded");
        let running = "system/ai.dreamference.ragon-node = {\n\tactive count = 1\n\tpath = /Library/LaunchDaemons/ai.dreamference.ragon-node.plist\n\ttype = LaunchDaemon\n\tstate = running\n\n\tprogram = /x/ragon\n\tendpoints = {\n\t\tstate = active\n\t}\n\tpid = 412\n}\n";
        assert_eq!(parse_print_state(running), "running");
        let idle = "system/ai.dreamference.ragon-node = {\n\tendpoints = {\n\t\tstate = active\n\t}\n\tstate = not running\n}\n";
        assert_eq!(parse_print_state(idle), "waiting");
        assert_eq!(parse_print_state("gui/501/x = {\n\tstate = xpcproxy\n\tpid = 71380\n}\n"), "running");
        assert_eq!(parse_print_state("system/x = {\n\tstate = penalty box\n}\n"), "unknown");
        assert_eq!(parse_print_state(""), "unknown");
    }
}
