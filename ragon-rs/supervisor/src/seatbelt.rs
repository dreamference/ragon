//! The per-job Seatbelt profile (specs/DREAMFERENCE_MAC_NODE.md §4.5): the macOS twin of the
//! bubblewrap command line in dreamference/node/node_job.py. A pure function from the job's paths
//! and its `/airgapped` level to the profile text `sandbox-exec -f` reads, so Linux tests check
//! the text and the Mac tests check what it denies.
//!
//! What the profile does:
//! - everything is denied unless a rule below allows it (`deny default`);
//! - the system is readable, nothing outside the job's worktree and its temporary folder is
//!   writable;
//! - the homes (`/Users`, the job user's own), `/Library/Application Support/Ragon` (weights,
//!   node id, queue) and the SSH keys (the user's and the host's) are neither readable nor
//!   writable; only the folders above the worktree may be looked up by name, so the worktree can
//!   live under a hidden home;
//! - the worktree's `.git` is read-only: the host runs `git` there afterwards, and a pointer the
//!   job rewrote would run the job's hooks outside the sandbox;
//! - no GPU: the IOKit user clients of the GPU are denied, so a job cannot allocate unified
//!   memory outside its footprint cap or starve the engine's device;
//! - the network only at `/airgapped off`; at `on` no socket leaves the process, which also
//!   keeps mDNSResponder (name lookups are IPC to it on macOS) out of reach. The host control
//!   sockets (Docker's, launchd's) are refused at both levels.
//!
//! In Seatbelt's language the last rule that matches decides, so the order below matters: broad
//! allows, then the hidden paths, then the worktree, then the read-only parts and the denials
//! that must win over everything.

use std::path::{Path, PathBuf};

/// The node's state, hidden from every job (weights, the converted model, node id, queue).
pub const NODE_STATE_DIR: &str = "/Library/Application Support/Ragon";

/// Where every account's home lives; hidden as a whole, not only the job user's.
pub const HOMES_DIR: &str = "/Users";

/// The host's SSH keys and configuration (`/etc` is a link to `/private/etc`; Seatbelt matches
/// resolved paths).
pub const HOST_SSH_DIR: &str = "/private/etc/ssh";

/// The IOKit user-client classes of Apple's GPU driver (AGX) and of the IOGPU family it builds
/// on. Opening any of them is how Metal reaches the device.
pub const GPU_USER_CLIENTS: &[&str] = &[
    "AGXDeviceUserClient",
    "AGXSharedUserClient",
    "AGXCommandQueue",
    "IOGPUDeviceUserClient",
    "IOAccelDevice2",
    "IOAccelSharedUserClient2",
    "IOAccelContext2",
];

/// Sockets that control the host, refused at every `/airgapped` level (the twin of patch 0019's
/// `host_sockets_at`): Docker's, the iOS device mux, the VPN control and the file system UI's.
pub const HOST_CONTROL_SOCKETS: &[&str] = &[
    "/private/var/run/docker.sock",
    "/private/var/run/usbmuxd",
    "/private/var/run/vpncontrol.sock",
    "/private/var/run/filesystemui.socket",
];

/// launchd's per-session listener folders (`com.apple.launchd.<random>`, under /private/tmp and
/// /private/var/run): an SSH agent's socket (`SSH_AUTH_SOCK`) lives in one. Hidden and refused at
/// every level.
pub const LAUNCHD_LISTENERS: &str = r#"^/private/(tmp|var/run)/com\.apple\.launchd\.[^/]+(/|$)"#;

/// mDNSResponder's socket: a name lookup on macOS is a message to it, not a packet to port 53.
pub const MDNS_SOCKET: &str = "/private/var/run/mDNSResponder";

/// The mach services a command needs for user and group lookups (`getpwuid`), as Codex's base
/// policy allows.
const BASE_MACH_SERVICES: &[&str] = &["com.apple.system.opendirectoryd.libinfo"];

/// The mach services a networked job needs: name resolution, TLS trust, network configuration
/// (Codex's network policy).
const NETWORK_MACH_SERVICES: &[&str] = &[
    "com.apple.dnssd.service",
    "com.apple.bsd.dirhelper",
    "com.apple.system.opendirectoryd.membership",
    "com.apple.SecurityServer",
    "com.apple.networkd",
    "com.apple.ocspd",
    "com.apple.trustd.agent",
    "com.apple.TrustEvaluationAgent",
    "com.apple.SystemConfiguration.DNSConfiguration",
    "com.apple.SystemConfiguration.configd",
];

/// What one job's profile is made of. Every path must already be resolved (`canonicalize`):
/// Seatbelt compares real paths, so `/tmp/x` would never match `/private/tmp/x`.
#[derive(Debug, Clone)]
pub struct JobSandbox {
    /// The job's worktree: readable and writable.
    pub tree: PathBuf,
    /// The job's own temporary folder (its `TMPDIR`): readable and writable.
    pub tmp: PathBuf,
    /// The home of the account the job runs as; hidden even when it is not under `/Users`.
    pub home: PathBuf,
    /// Folders the job may read and not write (a built environment, the node's allowed data).
    pub readable: Vec<PathBuf>,
    /// False at `/airgapped on`: no network at all.
    pub network: bool,
}

/// A string as a Seatbelt literal: in double quotes, with `\` and `"` escaped.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn path_text(path: &Path) -> String {
    quote(&path.to_string_lossy())
}

/// The folders above `path`, root first, without `/` and without `path` itself.
pub fn ancestors_of(path: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = path
        .ancestors()
        .skip(1)
        .filter(|p| p.parent().is_some() && !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect();
    out.reverse();
    out
}

/// The paths a job may neither read nor write: every home, its own account's home, the node's
/// state and the host's SSH keys.
pub fn hidden_paths(sandbox: &JobSandbox) -> Vec<PathBuf> {
    let mut hidden = vec![
        PathBuf::from(HOMES_DIR),
        PathBuf::from(NODE_STATE_DIR),
        PathBuf::from(HOST_SSH_DIR),
    ];
    if !sandbox.home.starts_with(HOMES_DIR) {
        hidden.push(sandbox.home.clone());
    }
    hidden
}

/// The profile text for one job.
pub fn profile(sandbox: &JobSandbox) -> String {
    let mut p = String::new();
    let mut line = |s: &str| {
        p.push_str(s);
        p.push('\n');
    };
    line("; Ragon node job sandbox, generated per job (specs/DREAMFERENCE_MAC_NODE.md §4.5).");
    line("(version 1)");
    line("(deny default)");
    line("(allow process-exec)");
    line("(allow process-fork)");
    line("(allow signal (target same-sandbox))");
    line("(allow process-info* (target same-sandbox))");
    line("(allow sysctl-read)");
    line("(allow ipc-posix-sem)");
    line("(allow pseudo-tty)");
    line("(allow iokit-open (iokit-registry-entry-class \"RootDomainUserClient\"))");
    for service in BASE_MACH_SERVICES {
        line(&format!("(allow mach-lookup (global-name {}))", quote(service)));
    }
    line("; The system is readable; what follows hides the private parts.");
    line("(allow file-read*)");
    line("(allow file-write-data file-ioctl (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/tty\") (literal \"/dev/dtracehelper\"))");
    line("(allow file-read* file-write* file-ioctl (regex #\"^/dev/fd/\") (literal \"/dev/ptmx\") (regex #\"^/dev/ttys[0-9]+$\"))");

    line("; Hidden: every home, the node's state, the SSH keys.");
    for path in hidden_paths(sandbox) {
        line(&format!("(deny file-read* file-write* (subpath {}))", path_text(&path)));
    }
    let mut lookups: Vec<PathBuf> = ancestors_of(&sandbox.tree);
    for extra in ancestors_of(&sandbox.tmp) {
        if !lookups.contains(&extra) {
            lookups.push(extra);
        }
    }
    if !lookups.is_empty() {
        line("; The folders above the worktree may be looked up, not listed or read.");
        for path in &lookups {
            line(&format!("(allow file-read-metadata (literal {}))", path_text(path)));
        }
    }

    line("; The job's worktree and temporary folder.");
    for path in [&sandbox.tree, &sandbox.tmp] {
        line(&format!("(allow file-read* file-write* (subpath {}))", path_text(path)));
    }
    for path in &sandbox.readable {
        line(&format!("(allow file-read* (subpath {}))", path_text(path)));
    }
    line("; The worktree's .git pointer stays read-only.");
    line(&format!("(deny file-write* (subpath {}))", path_text(&sandbox.tree.join(".git"))));

    line("; No GPU: Metal's IOKit user clients are denied.");
    for class in GPU_USER_CLIENTS {
        line(&format!("(deny iokit-open (iokit-user-client-class {}))", quote(class)));
    }

    if sandbox.network {
        line("; /airgapped off: the network is allowed.");
        line("(allow network-outbound)");
        line("(allow network-inbound)");
        line("(allow network-bind)");
        line("(allow system-socket)");
        for service in NETWORK_MACH_SERVICES {
            line(&format!("(allow mach-lookup (global-name {}))", quote(service)));
        }
    } else {
        line("; /airgapped on: no network, and no name lookups through mDNSResponder.");
        line("(deny network-outbound)");
        line("(deny network-inbound)");
        line("(deny network-bind)");
        line(&format!("(deny network-outbound (literal {}))", quote(MDNS_SOCKET)));
        line("(deny mach-lookup (global-name \"com.apple.dnssd.service\"))");
    }
    line("; The host's control sockets and launchd's listeners (SSH agents), at every level.");
    for socket in HOST_CONTROL_SOCKETS {
        line(&format!("(deny network-outbound (literal {}))", quote(socket)));
    }
    line(&format!("(deny file-read* file-write* network-outbound (regex #\"{LAUNCHD_LISTENERS}\"))"));
    p
}

/// The command line that runs `command` under `profile_path`.
pub fn wrap(profile_path: &Path, command: &[String]) -> Vec<String> {
    let mut argv = vec![
        "/usr/bin/sandbox-exec".to_string(),
        "-f".to_string(),
        profile_path.to_string_lossy().into_owned(),
        "--".to_string(),
    ];
    argv.extend(command.iter().cloned());
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(network: bool) -> JobSandbox {
        JobSandbox {
            tree: PathBuf::from("/Users/ragonjob/.ragon/jobs/20261125-1200-abc/tree"),
            tmp: PathBuf::from("/private/var/folders/xy/20261125-1200-abc"),
            home: PathBuf::from("/Users/ragonjob"),
            readable: vec![PathBuf::from("/Volumes/data/corpus")],
            network,
        }
    }

    fn index_of(text: &str, needle: &str) -> usize {
        text.find(needle).unwrap_or_else(|| panic!("{needle} not in profile:\n{text}"))
    }

    #[test]
    fn everything_is_denied_unless_allowed() {
        let text = profile(&sandbox(true));
        assert!(text.starts_with("; Ragon node job sandbox"));
        assert!(text.contains("(version 1)\n(deny default)\n"));
        // Nothing writable outside the job's two folders: the only broad allow is read.
        for l in text.lines().filter(|l| l.starts_with("(allow") && l.contains("file-write")) {
            assert!(
                l.contains("/dev/") || l.contains("/tree") || l.contains("20261125-1200-abc"),
                "unexpected writable rule: {l}"
            );
        }
    }

    #[test]
    fn the_homes_the_node_state_and_the_ssh_keys_are_hidden_after_the_broad_read() {
        let text = profile(&sandbox(true));
        let read_all = index_of(&text, "(allow file-read*)\n");
        for hidden in [
            "(deny file-read* file-write* (subpath \"/Users\"))",
            "(deny file-read* file-write* (subpath \"/Library/Application Support/Ragon\"))",
            "(deny file-read* file-write* (subpath \"/private/etc/ssh\"))",
        ] {
            assert!(index_of(&text, hidden) > read_all, "{hidden} must come after the broad read");
        }
    }

    #[test]
    fn a_home_outside_users_is_hidden_too() {
        let mut s = sandbox(false);
        s.home = PathBuf::from("/private/var/ragon-job");
        let text = profile(&s);
        assert!(text.contains("(deny file-read* file-write* (subpath \"/private/var/ragon-job\"))"));
        let s = sandbox(false);
        assert_eq!(hidden_paths(&s).iter().filter(|p| p.starts_with("/Users")).count(), 1);
    }

    #[test]
    fn the_worktree_is_writable_after_the_hidden_home_and_its_git_pointer_is_not() {
        let text = profile(&sandbox(true));
        let home = index_of(&text, "(subpath \"/Users\"))");
        let tree = index_of(
            &text,
            "(allow file-read* file-write* (subpath \"/Users/ragonjob/.ragon/jobs/20261125-1200-abc/tree\"))",
        );
        let git = index_of(
            &text,
            "(deny file-write* (subpath \"/Users/ragonjob/.ragon/jobs/20261125-1200-abc/tree/.git\"))",
        );
        assert!(home < tree && tree < git);
        // The folders above it are looked up by name, never listed.
        for anc in ["/Users", "/Users/ragonjob", "/Users/ragonjob/.ragon/jobs/20261125-1200-abc"] {
            assert!(text.contains(&format!("(allow file-read-metadata (literal \"{anc}\"))")), "{anc}");
        }
        assert!(!text.contains("(allow file-read-metadata (literal \"/\"))"));
        assert!(text.contains("(allow file-read* (subpath \"/Volumes/data/corpus\"))"));
    }

    #[test]
    fn the_gpu_user_clients_are_denied() {
        let text = profile(&sandbox(true));
        for class in ["AGXDeviceUserClient", "IOGPUDeviceUserClient"] {
            assert!(text.contains(&format!("(deny iokit-open (iokit-user-client-class \"{class}\"))")));
        }
        // The only IOKit allow is the power-management root client Codex's base policy keeps.
        let allows: Vec<&str> = text.lines().filter(|l| l.starts_with("(allow iokit-open")).collect();
        assert_eq!(allows, vec!["(allow iokit-open (iokit-registry-entry-class \"RootDomainUserClient\"))"]);
    }

    #[test]
    fn airgapped_on_allows_no_network_and_no_name_lookup() {
        let text = profile(&sandbox(false));
        assert!(!text.contains("(allow network"));
        assert!(!text.contains("(allow system-socket)"));
        assert!(!text.contains("com.apple.SystemConfiguration"));
        assert!(text.contains("(deny network-outbound)"));
        assert!(text.contains("(deny network-outbound (literal \"/private/var/run/mDNSResponder\"))"));
        assert!(text.contains("(deny mach-lookup (global-name \"com.apple.dnssd.service\"))"));
    }

    #[test]
    fn airgapped_off_allows_the_network_but_never_the_host_control_sockets() {
        let text = profile(&sandbox(true));
        let allow = index_of(&text, "(allow network-outbound)");
        assert!(text.contains("(allow mach-lookup (global-name \"com.apple.dnssd.service\"))"));
        for socket in HOST_CONTROL_SOCKETS {
            let deny = index_of(&text, &format!("(deny network-outbound (literal \"{socket}\"))"));
            assert!(deny > allow, "{socket} must be denied after the network allow");
        }
        let listeners = index_of(&text, "(deny file-read* file-write* network-outbound (regex #\"^/private/(tmp|var/run)/com\\.apple\\.launchd\\.");
        assert!(listeners > allow);
        assert!(profile(&sandbox(false)).contains(LAUNCHD_LISTENERS));
    }

    #[test]
    fn paths_are_quoted() {
        assert_eq!(quote(r#"a"b\c"#), r#""a\"b\\c""#);
        let mut s = sandbox(false);
        s.tree = PathBuf::from("/Users/x/it's \"here\"");
        assert!(profile(&s).contains(r#"(subpath "/Users/x/it's \"here\"")"#));
    }

    #[test]
    fn wrap_puts_sandbox_exec_in_front() {
        let argv = wrap(Path::new("/j/sandbox.sb"), &["make".into(), "test".into()]);
        assert_eq!(argv, ["/usr/bin/sandbox-exec", "-f", "/j/sandbox.sb", "--", "make", "test"]);
    }
}
