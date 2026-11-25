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
/// The directory of the system daemons' plists.
pub const DAEMONS_DIR: &str = "/Library/LaunchDaemons";
/// Seconds launchd waits before it starts a supervisor that died again (its own floor is 10).
pub const THROTTLE_S: u32 = 10;

/// `ragon-node` -> `ai.dreamference.ragon-node`.
pub fn label_for(service: &str) -> String {
    format!("ai.dreamference.{service}")
}

/// `/Library/LaunchDaemons/ai.dreamference.ragon-node.plist`.
pub fn plist_path(service: &str) -> PathBuf {
    Path::new(DAEMONS_DIR).join(format!("{}.plist", label_for(service)))
}

/// `system/ai.dreamference.ragon-node`, the target of `launchctl print|kickstart|bootout`.
pub fn service_target(service: &str) -> String {
    format!("system/{}", label_for(service))
}

/// What the plist describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Daemon {
    pub label: String,
    /// The full command line: the launcher, then `server run ...`.
    pub program_arguments: Vec<String>,
    pub user_name: String,
    pub working_directory: String,
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
            log_path: format!("{}/launchd.log", state_dir.trim_end_matches('/')),
        }
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
    key_string(&mut out, "UserName", &daemon.user_name);
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

/// `launchctl bootstrap system <plist>`: load (and, with RunAtLoad, start) the daemon.
pub fn bootstrap_args(service: &str) -> Vec<String> {
    vec!["bootstrap".into(), "system".into(), plist_path(service).display().to_string()]
}

/// `launchctl bootout system/<label>`: stop (SIGTERM, the supervisor stops the engine) and unload.
pub fn bootout_args(service: &str) -> Vec<String> {
    vec!["bootout".into(), service_target(service)]
}

/// `launchctl kickstart system/<label>`: start a loaded daemon that is not running.
pub fn kickstart_args(service: &str) -> Vec<String> {
    vec!["kickstart".into(), service_target(service)]
}

/// `launchctl print system/<label>`: readable without root.
pub fn print_args(service: &str) -> Vec<String> {
    vec!["print".into(), service_target(service)]
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
            plist_path("ragon-node"),
            PathBuf::from("/Library/LaunchDaemons/ai.dreamference.ragon-node.plist")
        );
        assert_eq!(service_target("ragon-node"), "system/ai.dreamference.ragon-node");
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
            bootstrap_args("ragon-node"),
            ["bootstrap", "system", "/Library/LaunchDaemons/ai.dreamference.ragon-node.plist"]
        );
        assert_eq!(bootout_args("ragon-node"), ["bootout", "system/ai.dreamference.ragon-node"]);
        assert_eq!(kickstart_args("ragon-node"), ["kickstart", "system/ai.dreamference.ragon-node"]);
        assert_eq!(print_args("ragon-node"), ["print", "system/ai.dreamference.ragon-node"]);
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
