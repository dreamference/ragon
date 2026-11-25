//! Pairing with another node over SSH, in Rust (the semantics of `dreamference/node/node_pairing.py`;
//! specs/DREAMFERENCE_WINDOWS_NODE.md WN1: a Windows node has no Python).
//!
//! Controlling a node is not open to the network: it goes through a key made for nothing else,
//! which the node restricts to one forced command. This module holds the same rules as the
//! Python `NodePairing`, so a Windows client and a Windows node (Win32-OpenSSH) pair with each
//! other and with a GB10 node by the same protocol:
//!
//! * the key is `~/.ssh/ragon-node_ed25519`, the records and the pinned host keys live under
//!   `~/.config/dreamference/nodes`, the host key is pinned to the node *id* (`HostKeyAlias`),
//!   never to an address;
//! * the `authorized_keys` line is `command="<serve>",restrict,... <type> <key> ragon-node`;
//! * a node id from the network names a file, so only `[A-Za-z0-9][A-Za-z0-9-]{0,63}` is taken;
//! * a host key different from the one pinned for that id is refused.
//!
//! What differs on Windows is only where the line is written (an administrator's keys are read
//! from `%ProgramData%\ssh\administrators_authorized_keys`, [`authorized_keys_file`]) and how the
//! forced command is spelled (forward slashes, since the line refuses a backslash,
//! [`forced_command`]). The side effects (running ssh) go through [`Transport`], so the whole flow
//! is tested with a fake node and no network.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use serde_json::Value;

/// The private key's file name under `~/.ssh`.
pub const KEY_NAME: &str = "ragon-node_ed25519";
/// What the restricted key may not do beside running anything but the forced command.
pub const KEY_RESTRICTIONS: &str = "restrict,no-pty,no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-user-rc";
/// Marks Ragon's lines in `authorized_keys` from the user's own.
pub const KEY_COMMENT: &str = "ragon-node";
/// Shortest id prefix that names a node.
pub const MIN_PARTIAL_ID_LENGTH: usize = 4;

/// Whether `id` is a node id (a UUID, or anything of the same alphabet up to 64 characters).
pub fn is_node_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    id.len() <= 64 && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// A paired node's record (`<id>.json`); an address is never stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub node: String,
    pub name: String,
    pub user: String,
    pub ssh_port: u16,
    pub overlay_name: Option<String>,
}

impl Record {
    fn to_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("node".into(), self.node.clone().into());
        map.insert("name".into(), self.name.clone().into());
        map.insert("user".into(), self.user.clone().into());
        map.insert("ssh_port".into(), self.ssh_port.into());
        if let Some(overlay) = &self.overlay_name {
            map.insert("overlay_name".into(), overlay.clone().into());
        }
        Value::Object(map)
    }

    fn from_json(value: &Value) -> Option<Record> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
        let node = text("node").filter(|n| is_node_id(n))?;
        let name = text("name").filter(|n| !n.is_empty())?;
        Some(Record {
            node,
            name,
            user: text("user").unwrap_or_default(),
            ssh_port: value.get("ssh_port").and_then(Value::as_u64).and_then(|p| u16::try_from(p).ok()).unwrap_or(22),
            overlay_name: text("overlay_name"),
        })
    }
}

/// The `authorized_keys` line a node writes for a paired sender.
///
/// Errors if the key is not one public key on one line, or the command cannot be quoted.
pub fn authorized_line(public_key: &str, serve_command: &str) -> Result<String, String> {
    let key = public_key.trim();
    let fields: Vec<&str> = key.split_whitespace().collect();
    if fields.len() < 2 || key.contains('\n') || !["ssh-", "ecdsa-", "sk-"].iter().any(|p| fields[0].starts_with(p)) {
        return Err("not a public key".into());
    }
    if serve_command.contains(['"', '\\', '\n']) {
        return Err("the forced command cannot be quoted".into());
    }
    Ok(format!("command=\"{serve_command}\",{KEY_RESTRICTIONS} {} {} {KEY_COMMENT}", fields[0], fields[1]))
}

/// The forced command as the line wants it: a Windows path with forward slashes (the line refuses
/// a backslash, and Win32-OpenSSH runs `C:/Users/x/ragon.exe node serve-job` fine).
pub fn forced_command(executable: &str) -> String {
    format!("{} node serve-job", executable.replace('\\', "/"))
}

/// Where Win32-OpenSSH reads the keys of `user`: an administrator's from the shared file, anyone
/// else's from their own `.ssh`.
pub fn authorized_keys_file(is_administrator: bool, program_data: &Path, home: &Path) -> PathBuf {
    if is_administrator {
        program_data.join("ssh").join("administrators_authorized_keys")
    } else {
        home.join(".ssh").join("authorized_keys")
    }
}

/// Adds `line` to the text of an `authorized_keys` file, replacing an earlier Ragon line for the
/// same key and leaving the user's own lines alone. Returns the new text.
pub fn install_line(existing: &str, line: &str) -> String {
    let key_blob = line.split_whitespace().rev().nth(1).unwrap_or("");
    let mut out: Vec<&str> = existing
        .lines()
        .filter(|l| !(l.trim_end().ends_with(KEY_COMMENT) && l.split_whitespace().rev().nth(1) == Some(key_blob)))
        .collect();
    out.push(line);
    out.join("\n") + "\n"
}

/// What a node's `unpair` leaves: every Ragon line removed from `existing`.
pub fn remove_lines(existing: &str) -> String {
    let kept: Vec<&str> = existing.lines().filter(|l| !l.trim_end().ends_with(KEY_COMMENT)).collect();
    if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" }
}

/// The name a node's host key is stored under: pinned to the node, not to an address.
pub fn host_alias(node_id: &str) -> String {
    format!("ragon-node-{node_id}")
}

/// A hint naming the user tried when a login failed with "Permission denied", else "".
pub fn permission_hint(error: &str, user: &str) -> String {
    if error.to_lowercase().contains("permission denied") {
        format!("💡 The login as '{user}' was refused. If the account there has another name, give it with --user.\n")
    } else {
        String::new()
    }
}

/// What a run of ssh returned.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// The side effects of pairing; the real one spawns `ssh`, a test answers as a node would.
pub trait Transport {
    /// Runs `ssh <args>`; `input` goes to its standard input.
    fn ssh(&self, args: &[String], input: Option<&str>) -> Output;
    /// Creates the pairing key if missing; returns the public key's text.
    fn ensure_key(&self, key: &Path) -> Option<String>;
}

/// Pairing state under one home directory.
pub struct NodePairing {
    pub home: PathBuf,
}

impl NodePairing {
    pub fn key_path(&self) -> PathBuf {
        self.home.join(".ssh").join(KEY_NAME)
    }
    pub fn nodes_dir(&self) -> PathBuf {
        self.home.join(".config").join("dreamference").join("nodes")
    }
    pub fn known_hosts(&self) -> PathBuf {
        self.nodes_dir().join("known_hosts")
    }

    /// `-i <identity> -o IdentitiesOnly=yes`: offer this one key and no other.
    pub fn identity_options(identity: &Path) -> Vec<String> {
        vec!["-i".into(), identity.display().to_string(), "-o".into(), "IdentitiesOnly=yes".into()]
    }

    /// The options every connection to a paired node uses.
    pub fn ssh_options(&self, record: &Record, accept_new: bool) -> Vec<String> {
        let mut v = Self::identity_options(&self.key_path());
        v.extend(["-p".to_string(), record.ssh_port.to_string()]);
        for (k, val) in [
            ("BatchMode", "yes".to_string()),
            ("UserKnownHostsFile", self.known_hosts().display().to_string()),
            ("HostKeyAlias", host_alias(&record.node)),
            ("StrictHostKeyChecking", if accept_new { "accept-new" } else { "yes" }.to_string()),
            ("ConnectTimeout", "10".to_string()),
        ] {
            v.extend(["-o".to_string(), format!("{k}={val}")]);
        }
        v
    }

    pub fn record_path(&self, node_id: &str) -> Result<PathBuf, String> {
        if !is_node_id(node_id) {
            return Err(format!("{node_id:?} is not a node id"));
        }
        Ok(self.nodes_dir().join(format!("{node_id}.json")))
    }

    /// Writes a record (atomically, and never with an address).
    pub fn save(&self, record: &Record) -> Result<(), String> {
        let path = self.record_path(&record.node)?;
        fs::create_dir_all(self.nodes_dir()).map_err(|e| e.to_string())?;
        let staging = path.with_file_name(format!(".{}.{}.tmp", record.node, std::process::id()));
        let text = serde_json::to_string_pretty(&record.to_json()).map_err(|e| e.to_string())? + "\n";
        fs::write(&staging, text).map_err(|e| e.to_string())?;
        fs::rename(&staging, &path).map_err(|e| e.to_string())
    }

    /// The paired nodes' records, by name; unreadable files are skipped.
    pub fn paired(&self) -> Vec<Record> {
        let mut out: Vec<Record> = fs::read_dir(self.nodes_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_str::<Value>(&fs::read_to_string(e.path()).ok()?).ok())
            .filter_map(|v| Record::from_json(&v))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// A paired node by name or id (or an id prefix of four characters or more).
    pub fn find(&self, name: &str) -> Option<Record> {
        let wanted = name.trim().to_lowercase();
        self.paired().into_iter().find(|r| {
            wanted == r.name.to_lowercase()
                || wanted == r.node.to_lowercase()
                || (wanted.len() >= MIN_PARTIAL_ID_LENGTH && r.node.to_lowercase().starts_with(&wanted))
        })
    }

    /// Pins a session's accepted host keys to the node id. A different key already pinned is
    /// refused (the machine answering is not the one paired before).
    pub fn pin_host_keys(&self, node_id: &str, keys: &[String]) -> Result<(), String> {
        if keys.is_empty() {
            return Err("The provisioning session recorded no host key to pin.".into());
        }
        let alias = host_alias(node_id);
        let path = self.known_hosts();
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let pinned: Vec<String> = existing
            .lines()
            .filter(|l| l.split_whitespace().next() == Some(alias.as_str()))
            .map(|l| l.split_whitespace().skip(1).take(2).collect::<Vec<_>>().join(" "))
            .collect();
        if !pinned.is_empty() {
            if pinned.iter().any(|p| keys.contains(p)) {
                return Ok(());
            }
            return Err(format!(
                "The host key of {}… differs from the one pinned at an earlier pairing; refusing. If the machine was reinstalled, `ragon node remove` it first.",
                node_id.chars().take(8).collect::<String>()
            ));
        }
        fs::create_dir_all(self.nodes_dir()).map_err(|e| e.to_string())?;
        let mut text = existing;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        for key in keys {
            text.push_str(&format!("{alias} {key}\n"));
        }
        fs::write(&path, text).map_err(|e| e.to_string())
    }

    /// Drops a node's pinned host keys.
    pub fn unpin(&self, node_id: &str) {
        let alias = host_alias(node_id);
        let path = self.known_hosts();
        if let Ok(text) = fs::read_to_string(&path) {
            let kept: Vec<&str> = text.lines().filter(|l| l.split_whitespace().next() != Some(alias.as_str())).collect();
            let _ = fs::write(&path, kept.iter().map(|l| format!("{l}\n")).collect::<String>());
        }
    }

    /// The ssh argv to ask a paired node's forced command for `request`, at `address`.
    pub fn ssh_command(&self, record: &Record, address: &str, request: &str) -> Vec<String> {
        let mut v = self.ssh_options(record, false);
        v.push(format!("{}@{address}", record.user));
        v.push(request.to_string());
        v
    }

    /// Sends one request to a paired node through the restricted key.
    pub fn run(&self, t: &dyn Transport, record: &Record, address: &str, request: &str) -> Output {
        t.ssh(&self.ssh_command(record, address, request), None)
    }

    /// Pairs through an open provisioning login at `address` (the login's own options in
    /// `login_args`, its accepted host keys in `host_keys`): reads the node's id and name, pins the
    /// host key, has the node authorise the key, then checks that the *restricted* key answers
    /// with the same id. Any failure removes what was written.
    pub fn pair(
        &self,
        t: &dyn Transport,
        address: &str,
        user: &str,
        ssh_port: u16,
        login_args: &[String],
        host_keys: &[String],
    ) -> Result<Record, String> {
        let remote = |cmd: &str, input: Option<&str>| {
            let mut args = login_args.to_vec();
            args.push(format!("{user}@{address}"));
            args.push(cmd.to_string());
            t.ssh(&args, input)
        };
        let answer = remote("ragon node id && hostname", None);
        let words: Vec<&str> = answer.stdout.split_whitespace().collect();
        if answer.code != 0 || words.len() < 2 || !is_node_id(words[0]) {
            return Err(format!("{address} did not report a node id (is Ragon installed there?).{}", permission_hint(&answer.stderr, user)));
        }
        let record = Record { node: words[0].into(), name: words[1].into(), user: user.into(), ssh_port, overlay_name: None };
        let public_key = t.ensure_key(&self.key_path()).ok_or("ssh-keygen could not create the pairing key.")?;
        self.pin_host_keys(&record.node, host_keys)?;
        let authorized = remote("ragon node authorize", Some(&format!("{public_key}\n")));
        if authorized.code != 0 {
            let tail: String = authorized.stderr.trim().chars().rev().take(200).collect::<Vec<_>>().into_iter().rev().collect();
            return Err(format!("{} did not authorise the key: {tail}", record.name));
        }
        self.save(&record)?;
        let check = self.run(t, &record, address, "info");
        let info: Value = if check.code == 0 { serde_json::from_str(&check.stdout).unwrap_or(Value::Null) } else { Value::Null };
        if info.get("node").and_then(Value::as_str) != Some(record.node.as_str()) {
            if let Ok(p) = self.record_path(&record.node) {
                let _ = fs::remove_file(p);
            }
            return Err(format!("{} did not answer through the new key; pairing removed.", record.name));
        }
        Ok(record)
    }

    /// Unpairs: asks the node to drop the key's line, forgets the record and the pinned key.
    /// Returns whether the node confirmed (a false means its `authorized_keys` may still hold it).
    pub fn remove(&self, t: &dyn Transport, record: &Record, address: &str) -> bool {
        let confirmed = self.run(t, record, address, "unpair").code == 0;
        if let Ok(p) = self.record_path(&record.node) {
            let _ = fs::remove_file(p);
        }
        self.unpin(&record.node);
        confirmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const KEY: &str = "ssh-ed25519 AAAAC3Nza client";
    const ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ragon-pairing-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A node as sshd sees it: the login (no `restrict` key) runs anything; the restricted key
    /// answers `info`/`unpair` only, and only for the key it was given.
    struct FakeNode {
        id: &'static str,
        authorized: RefCell<Option<String>>,
        keys_file: RefCell<String>,
    }
    impl Transport for FakeNode {
        fn ssh(&self, args: &[String], input: Option<&str>) -> Output {
            let request = args.last().unwrap().as_str();
            let restricted = args.iter().any(|a| a.contains("ragon-node_ed25519"));
            let ok = |s: &str| Output { code: 0, stdout: s.into(), stderr: String::new() };
            let denied = Output { code: 255, stdout: String::new(), stderr: "Permission denied (publickey).".into() };
            match (restricted, request) {
                (false, "ragon node id && hostname") => ok(&format!("{}\nwin-box\n", self.id)),
                (false, "ragon node authorize") => {
                    let line = authorized_line(input.unwrap(), &forced_command("C:\\Users\\a\\ragon.exe")).unwrap();
                    let mut keys = self.keys_file.borrow_mut();
                    *keys = install_line(&keys, &line);
                    *self.authorized.borrow_mut() = input.map(|s| s.trim().to_string());
                    ok("")
                }
                (true, _) if self.authorized.borrow().is_none() => denied,
                (true, "info") => ok(&format!("{{\"node\": \"{}\"}}", self.id)),
                (true, "unpair") => {
                    let mut keys = self.keys_file.borrow_mut();
                    *keys = remove_lines(&keys);
                    *self.authorized.borrow_mut() = None;
                    ok("")
                }
                _ => denied,
            }
        }
        fn ensure_key(&self, _: &Path) -> Option<String> {
            Some(KEY.into())
        }
    }

    fn node(id: &'static str) -> FakeNode {
        FakeNode { id, authorized: RefCell::new(None), keys_file: RefCell::new("ssh-rsa USERS own@box\n".into()) }
    }

    #[test]
    fn the_line_forces_one_command_and_restricts_the_key() {
        let line = authorized_line("  ssh-ed25519 AAAA c omment ", "C:/r/ragon.exe node serve-job").unwrap();
        assert_eq!(line, format!("command=\"C:/r/ragon.exe node serve-job\",{KEY_RESTRICTIONS} ssh-ed25519 AAAA ragon-node"));
        assert!(authorized_line("not a key", "x").is_err());
        assert!(authorized_line("ssh-ed25519", "x").is_err());
        assert!(authorized_line("ssh-ed25519 A\nssh-rsa B", "x").is_err());
        assert!(authorized_line(KEY, "C:\\bad").is_err());
        assert!(authorized_line(KEY, "a\"b").is_err());
    }

    #[test]
    fn a_windows_command_is_spelled_with_forward_slashes() {
        assert_eq!(forced_command("C:\\Users\\a\\ragon.exe"), "C:/Users/a/ragon.exe node serve-job");
        assert!(authorized_line(KEY, &forced_command("C:\\Users\\a\\ragon.exe")).is_ok());
    }

    #[test]
    fn an_administrators_keys_go_to_the_shared_file() {
        let (pd, home) = (Path::new("C:/ProgramData"), Path::new("C:/Users/a"));
        assert!(authorized_keys_file(true, pd, home).ends_with("ssh/administrators_authorized_keys"));
        assert!(authorized_keys_file(false, pd, home).ends_with(".ssh/authorized_keys"));
    }

    #[test]
    fn installing_replaces_the_same_key_and_keeps_the_users_lines() {
        let line = authorized_line(KEY, "/r/ragon node serve-job").unwrap();
        let once = install_line("ssh-rsa USERS own@box\n", &line);
        assert_eq!(install_line(&once, &line), once);
        assert_eq!(once.lines().count(), 2);
        assert_eq!(remove_lines(&once), "ssh-rsa USERS own@box\n");
    }

    #[test]
    fn node_ids_are_checked_before_they_name_a_file() {
        assert!(is_node_id(ID));
        for bad in ["", "-a", "../x", "a/b", "a b", &"a".repeat(65), "a\\b"] {
            assert!(!is_node_id(bad), "{bad}");
        }
        assert!(NodePairing { home: PathBuf::from("/h") }.record_path("../x").is_err());
    }

    #[test]
    fn options_pin_the_host_key_to_the_id_and_offer_one_key() {
        let p = NodePairing { home: PathBuf::from("/h") };
        let r = Record { node: ID.into(), name: "n".into(), user: "u".into(), ssh_port: 2222, overlay_name: None };
        let o = p.ssh_options(&r, false).join(" ");
        assert!(o.contains("IdentitiesOnly=yes") && o.contains("-p 2222") && o.contains("BatchMode=yes"));
        assert!(o.contains(&format!("HostKeyAlias=ragon-node-{ID}")) && o.contains("StrictHostKeyChecking=yes"));
        assert!(p.ssh_options(&r, true).join(" ").contains("StrictHostKeyChecking=accept-new"));
    }

    #[test]
    fn a_node_pairs_through_the_restricted_key_and_unpairs() {
        let home = temp("ok");
        let p = NodePairing { home: home.clone() };
        let n = node(ID);
        let record = p.pair(&n, "win-box", "a", 22, &[], &["ssh-ed25519 HOSTKEY".into()]).unwrap();
        assert_eq!(record.name, "win-box");
        assert_eq!(p.find("0f1e").unwrap().node, ID);
        assert_eq!(p.find("WIN-BOX").unwrap().node, ID);
        assert!(p.find("0f1").is_none());
        let stored = fs::read_to_string(p.record_path(ID).unwrap()).unwrap();
        assert!(!stored.contains("address"));
        assert!(fs::read_to_string(p.known_hosts()).unwrap().starts_with(&format!("ragon-node-{ID} ssh-ed25519 HOSTKEY")));
        assert!(n.keys_file.borrow().contains("restrict,"));
        assert!(p.remove(&n, &record, "win-box"));
        assert!(p.paired().is_empty() && !n.keys_file.borrow().contains(KEY_COMMENT));
        assert!(!fs::read_to_string(p.known_hosts()).unwrap().contains(ID));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_node_that_answers_with_another_id_is_removed() {
        struct Liar(FakeNode);
        impl Transport for Liar {
            fn ssh(&self, a: &[String], i: Option<&str>) -> Output {
                let mut out = self.0.ssh(a, i);
                if a.last().unwrap() == "info" {
                    out.stdout = "{\"node\": \"someone-else\"}".into();
                }
                out
            }
            fn ensure_key(&self, k: &Path) -> Option<String> {
                self.0.ensure_key(k)
            }
        }
        let home = temp("liar");
        let p = NodePairing { home: home.clone() };
        let err = p.pair(&Liar(node(ID)), "h", "a", 22, &[], &["ssh-ed25519 K".into()]).unwrap_err();
        assert!(err.contains("pairing removed"));
        assert!(p.paired().is_empty());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_different_host_key_for_a_paired_id_is_refused() {
        let home = temp("pin");
        let p = NodePairing { home: home.clone() };
        p.pin_host_keys(ID, &["ssh-ed25519 ONE".into()]).unwrap();
        p.pin_host_keys(ID, &["ssh-ed25519 ONE".into()]).unwrap();
        let err = p.pin_host_keys(ID, &["ssh-ed25519 TWO".into()]).unwrap_err();
        assert!(err.contains("differs"));
        assert!(p.pin_host_keys("other", &[]).is_err());
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn a_bad_login_or_a_foreign_machine_pairs_nothing() {
        let home = temp("bad");
        let p = NodePairing { home: home.clone() };
        struct Refuse;
        impl Transport for Refuse {
            fn ssh(&self, _: &[String], _: Option<&str>) -> Output {
                Output { code: 255, stdout: String::new(), stderr: "Permission denied (publickey).".into() }
            }
            fn ensure_key(&self, _: &Path) -> Option<String> {
                Some(KEY.into())
            }
        }
        let err = p.pair(&Refuse, "h", "a", 22, &[], &["ssh-ed25519 K".into()]).unwrap_err();
        assert!(err.contains("did not report a node id") && err.contains("--user"));
        assert!(p.paired().is_empty() && !p.known_hosts().exists());
        // an id that could name another path is not taken from the network
        let evil = node("../etc");
        assert!(p.pair(&evil, "h", "a", 22, &[], &["ssh-ed25519 K".into()]).is_err());
        assert!(p.paired().is_empty());
        // the restricted key is refused until the node authorised it: "bad key refused"
        let fresh = node(ID);
        let record = Record { node: ID.into(), name: "n".into(), user: "a".into(), ssh_port: 22, overlay_name: None };
        assert_eq!(p.run(&fresh, &record, "h", "info").code, 255);
        let _ = fs::remove_dir_all(home);
    }
}
