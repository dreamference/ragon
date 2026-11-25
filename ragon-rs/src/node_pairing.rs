//! Pairing with a node over SSH, in Rust (the port of `NodePairing` and the node half of
//! `NodeServe`, specs/DREAMFERENCE_WINDOWS_NODE.md WN1). No Python runs on the node.
//!
//! The protocol is unchanged: the sender's key goes into the node's `authorized_keys` with one
//! forced command, `serve-job`, and every restriction; the sender then asks `info` through the new
//! key and keeps the pairing only when the node reports the id it advertised. Against Win32-OpenSSH
//! the account is an ordinary user (never an administrator, whose keys live in another file), so
//! the file is `<profile>\.ssh\authorized_keys`, and its ACL is cut down to that user, SYSTEM and
//! the administrators, which `StrictModes` demands.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use sha2::Digest;
use sha2::Sha256;

pub const KEY_NAME: &str = "ragon-node_ed25519";
/// What the restricted key may not do (`restrict` covers every capability a later OpenSSH adds).
pub const KEY_RESTRICTIONS: &str = "restrict,no-pty,no-port-forwarding,no-agent-forwarding,no-X11-forwarding,no-user-rc";
/// Marks Ragon's lines in `authorized_keys` from the user's own.
pub const KEY_COMMENT: &str = "ragon-node";
/// Where the node keeps its authorised keys, below the account's profile. On Windows the profile
/// is `%USERPROFILE%`; the path does not come from the state layout (that is the service's
/// ProgramData tree) but from what OpenSSH reads for a non-administrator account.
pub const AUTHORIZED_KEYS: &str = ".ssh/authorized_keys";

/// Whether `id` may name a node: it names a record file and a host-key alias.
pub fn is_node_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_alphanumeric()
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
}

/// The name a node's host key is stored under: pinned to the node, not to an address.
pub fn host_alias(node_id: &str) -> String {
    format!("ragon-node-{node_id}")
}

/// The `authorized_keys` line for a paired sender: one forced command, no terminal, no forwarding.
pub fn authorized_line(public_key: &str, serve_command: &str) -> Result<String, String> {
    let trimmed = public_key.trim();
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    if fields.len() < 2 || trimmed.contains('\n') || !["ssh-", "ecdsa-", "sk-"].iter().any(|p| fields[0].starts_with(p)) {
        return Err("not a public key".into());
    }
    if serve_command.chars().any(|c| matches!(c, '"' | '\\' | '\n')) {
        return Err("the forced command cannot be quoted".into());
    }
    Ok(format!("command=\"{serve_command}\",{KEY_RESTRICTIONS} {} {} {KEY_COMMENT}", fields[0], fields[1]))
}

/// A short digest of the key, written into its forced command so `unpair` removes that key's line
/// and no other.
pub fn key_tag(public_key: &str) -> Result<String, String> {
    let body = public_key.trim().split_whitespace().nth(1).ok_or("not a public key")?;
    let digest = Sha256::digest(body.as_bytes());
    Ok(digest.iter().take(8).map(|b| format!("{b:02x}")).collect())
}

fn is_tag(tag: &str) -> bool {
    tag.len() == 16 && tag.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The file text after authorising `public_key`: any earlier line of the same key is replaced, the
/// user's other lines stay, the new one is last.
pub fn with_authorised(existing: &str, public_key: &str, serve_command_for: &dyn Fn(&str) -> String) -> Result<String, String> {
    let tag = key_tag(public_key)?;
    let line = authorized_line(public_key, &serve_command_for(&tag))?;
    let mut kept: Vec<&str> = existing.lines().filter(|l| !l.contains(&format!("--key {tag}"))).collect();
    kept.push(&line);
    Ok(kept.join("\n") + "\n")
}

/// The file text after removing the line `tag` wrote, or None when no Ragon line carries it. A
/// line of the user's own that mentions the tag is never removed.
pub fn without_tag(existing: &str, tag: &str) -> Option<String> {
    if !is_tag(tag) {
        return None;
    }
    let marker = format!("--key {tag}");
    let lines: Vec<&str> = existing.lines().collect();
    let kept: Vec<&str> = lines.iter().copied().filter(|l| !(l.contains(&marker) && l.trim_end().ends_with(KEY_COMMENT))).collect();
    if kept.len() == lines.len() {
        return None;
    }
    Some(if kept.is_empty() { String::new() } else { kept.join("\n") + "\n" })
}

/// `<home>/.ssh/authorized_keys`.
pub fn authorized_keys_path(home: &Path) -> PathBuf {
    home.join(AUTHORIZED_KEYS)
}

/// Writes `text` to `path` through a staging file, readable by the account only.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".{}.{}.tmp", path.file_name().and_then(|n| n.to_str()).unwrap_or("keys"), std::process::id()));
    std::fs::write(&staging, text)?;
    restrict(&staging)?;
    std::fs::rename(&staging, path)
}

#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

/// Win32-OpenSSH with StrictModes refuses a key file others can write: inheritance is cut and the
/// account, SYSTEM and the administrators keep full control.
#[cfg(windows)]
fn restrict(path: &Path) -> std::io::Result<()> {
    let user = std::env::var("USERNAME").unwrap_or_default();
    let status = Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r"])
        .arg(format!("{user}:F"))
        .args(["/grant:r", "*S-1-5-18:F", "/grant:r", "*S-1-5-32-544:F"])
        .stdout(Stdio::null())
        .status()?;
    if status.success() { Ok(()) } else { Err(std::io::Error::other("icacls refused the key file")) }
}

#[cfg(not(any(unix, windows)))]
fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// The node half of `authorize`: puts the restricted line for `public_key` into `path`.
pub fn authorize(path: &Path, public_key: &str, serve_command_for: &dyn Fn(&str) -> String) -> Result<(), String> {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let text = with_authorised(&existing, public_key, serve_command_for)?;
    write_private(path, &text).map_err(|e| e.to_string())
}

/// The node half of `unpair`: true when a line was removed.
pub fn unauthorize(path: &Path, tag: &str) -> bool {
    let Ok(existing) = std::fs::read_to_string(path) else { return false };
    match without_tag(&existing, tag) {
        Some(text) => write_private(path, &text).is_ok(),
        None => false,
    }
}

/// The host-key lines (`<type> <key>`) pinned under `alias` in a known-hosts text.
fn pinned(known_hosts: &str, alias: &str) -> Vec<String> {
    known_hosts
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            (f.first() == Some(&alias) && f.len() >= 3).then(|| format!("{} {}", f[1], f[2]))
        })
        .collect()
}

/// What pinning a session's host keys does to the known-hosts text: `Ok(Some(new text))` writes,
/// `Ok(None)` is already pinned, `Err` is a different key than the one pinned before.
pub fn pin_host_keys(known_hosts: &str, node_id: &str, keys: &[String]) -> Result<Option<String>, String> {
    if keys.is_empty() {
        return Err("The provisioning session recorded no host key to pin.".into());
    }
    let alias = host_alias(node_id);
    let have = pinned(known_hosts, &alias);
    if !have.is_empty() {
        if have.iter().any(|k| keys.contains(k)) {
            return Ok(None);
        }
        return Err(format!(
            "The host key of {}… differs from the one pinned at an earlier pairing; refusing. If the machine was reinstalled, remove its pairing first.",
            node_id.chars().take(8).collect::<String>()
        ));
    }
    let mut text = known_hosts.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for key in keys {
        text.push_str(&format!("{alias} {key}\n"));
    }
    Ok(Some(text))
}

/// The options every connection to a paired node uses; `accept_new` only while pairing.
pub fn ssh_options(key: &Path, known_hosts: &Path, node_id: &str, port: u16, accept_new: bool) -> Vec<String> {
    let mut args = identity_options(key);
    args.extend(["-p".into(), port.to_string(), "-o".into(), "BatchMode=yes".into()]);
    args.extend(["-o".into(), format!("UserKnownHostsFile={}", known_hosts.display())]);
    args.extend(["-o".into(), format!("HostKeyAlias={}", host_alias(node_id))]);
    args.extend(["-o".into(), format!("StrictHostKeyChecking={}", if accept_new { "accept-new" } else { "yes" })]);
    args.extend(["-o".into(), "ConnectTimeout=10".into()]);
    args
}

/// Offers this one key and no other (many agent keys end a login with "Too many authentication failures").
pub fn identity_options(identity: &Path) -> Vec<String> {
    vec!["-i".into(), identity.display().to_string(), "-o".into(), "IdentitiesOnly=yes".into()]
}

/// Where the pairing key lives for `home`.
pub fn key_path(home: &Path) -> PathBuf {
    home.join(".ssh").join(KEY_NAME)
}

/// Creates the key pair on first use; the public key's text, or None when `ssh-keygen` failed.
pub fn ensure_key(key: &Path) -> Option<String> {
    let public = PathBuf::from(format!("{}.pub", key.display()));
    if !key.is_file() || !public.is_file() {
        std::fs::create_dir_all(key.parent()?).ok()?;
        for stale in [key, public.as_path()] {
            let _ = std::fs::remove_file(stale);
        }
        let ok = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", KEY_COMMENT, "-f"])
            .arg(key)
            .stdout(Stdio::null())
            .status()
            .ok()?
            .success();
        if !ok {
            return None;
        }
    }
    Some(std::fs::read_to_string(public).ok()?.trim().to_string())
}

/// What the `info` request answered, reduced to the id it reports.
pub fn reported_node(info_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(info_json).ok()?;
    value.get("node")?.as_str().map(str::to_string)
}

/// The record kept per paired node (never an address).
pub fn record_json(node: &str, name: &str, user: &str, ssh_port: u16) -> serde_json::Value {
    serde_json::json!({"node": node, "name": name, "user": user, "ssh_port": ssh_port})
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

fn nodes_dir(home: &Path) -> PathBuf {
    home.join(".config").join("dreamference").join("nodes")
}

/// The node's own forced command: this executable, `node serve-job --key <tag>`.
fn serve_command(tag: &str) -> String {
    let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "ragon".into());
    format!("{exe} node serve-job --key {tag}")
}

/// `ragon node authorize` (public key on stdin) and `ragon node unauthorize <tag>`: the node half.
pub fn run_node_side(args: &[String]) -> i32 {
    let Some(home) = home() else {
        eprintln!("no profile directory");
        return 1;
    };
    let path = authorized_keys_path(&home);
    match args {
        [cmd] if cmd == "authorize" => {
            let mut key = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut key);
            match authorize(&path, &key, &serve_command) {
                Ok(()) => {
                    println!("Authorised for Ragon node operations only ({}).", path.display());
                    0
                }
                Err(e) => {
                    println!("{e}");
                    1
                }
            }
        }
        [cmd, tag] if cmd == "unauthorize" => {
            if unauthorize(&path, tag) {
                println!("unpaired");
                0
            } else {
                println!("no such key");
                1
            }
        }
        _ => 2,
    }
}

/// `ragon node pair <user@host> [port]`: the sender half, over the real protocol. The node's
/// password is asked once, by ssh itself, on the terminal.
pub fn run_pair(target: &str, port: u16) -> i32 {
    let Some(home) = home() else { return 1 };
    let Some((user, host)) = target.split_once('@') else {
        eprintln!("Usage: ragon node pair <user@host> [port]");
        return 2;
    };
    let Some(public) = ensure_key(&key_path(&home)) else {
        eprintln!("ssh-keygen could not create the pairing key.");
        return 1;
    };
    let known = nodes_dir(&home).join("known_hosts");
    let _ = std::fs::create_dir_all(nodes_dir(&home));
    // One login: its id and name, then the authorisation, the host key pinned by the first.
    let probe = Command::new("ssh")
        .args(["-p", &port.to_string(), "-o", &format!("UserKnownHostsFile={}", known.display())])
        .args(["-o", "StrictHostKeyChecking=accept-new", "-o", "HostKeyAlias=ragon-node-pending"])
        .arg(target)
        .arg("ragon node id")
        .stderr(Stdio::inherit())
        .output();
    let Ok(out) = probe else { return 1 };
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let mut words = text.split_whitespace();
    let (Some(id), Some(name)) = (words.next(), words.next()) else {
        eprintln!("{host} did not report a node id (is Ragon installed there?).");
        return 1;
    };
    if !out.status.success() || !is_node_id(id) {
        eprintln!("{host} did not report a node id (is Ragon installed there?).");
        return 1;
    }
    // Move the pending alias to the node's alias, refusing a different key than one pinned earlier.
    let all = std::fs::read_to_string(&known).unwrap_or_default();
    let keys: Vec<String> = pinned(&all, "ragon-node-pending");
    let rest: String = all.lines().filter(|l| !l.starts_with("ragon-node-pending ")).map(|l| format!("{l}\n")).collect();
    match pin_host_keys(&rest, id, &keys) {
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
        Ok(text) => {
            if let Err(e) = std::fs::write(&known, text.unwrap_or(rest)) {
                eprintln!("{e}");
                return 1;
            }
        }
    }
    let mut child = match Command::new("ssh")
        .args(ssh_options(&key_path(&home), &known, id, port, true).into_iter().filter(|a| a != "BatchMode=yes").collect::<Vec<_>>())
        .args(["-o", "BatchMode=no"])
        .arg(target)
        .arg("ragon node authorize")
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return 1,
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = std::io::Write::write_all(&mut stdin, format!("{public}\n").as_bytes());
    }
    if !child.wait().map(|s| s.success()).unwrap_or(false) {
        eprintln!("{name} did not authorise the key.");
        return 1;
    }
    let record_path = nodes_dir(&home).join(format!("{id}.json"));
    let _ = std::fs::write(&record_path, record_json(id, name, user, port).to_string());
    let check = Command::new("ssh").args(ssh_options(&key_path(&home), &known, id, port, false)).arg(target).arg("info").output();
    let answered = check.ok().filter(|o| o.status.success()).and_then(|o| reported_node(&String::from_utf8_lossy(&o.stdout)));
    if answered.as_deref() != Some(id) {
        let _ = std::fs::remove_file(&record_path);
        eprintln!("{name} did not answer through the new key; pairing removed.");
        return 1;
    }
    println!("Paired with {name}. This key can ask it for node operations and nothing else.");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "ssh-ed25519 AAAAfirst a";

    fn command(tag: &str) -> String {
        format!("/bin/ragon node serve-job --key {tag}")
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ragon-pairing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_line_forces_one_command_and_restricts_everything() {
        let line = authorized_line(KEY, "/bin/ragon node serve-job --key x").unwrap();
        assert!(line.starts_with("command=\"/bin/ragon node serve-job --key x\",restrict,no-pty"));
        assert!(line.ends_with("ssh-ed25519 AAAAfirst ragon-node"));
    }

    #[test]
    fn something_that_is_not_one_public_key_is_refused() {
        for key in ["", "ssh-ed25519", "not a key at all", "ssh-ed25519 AAAA\nssh-ed25519 BBBB"] {
            assert!(authorized_line(key, "x").is_err(), "{key:?}");
        }
        assert!(authorized_line(KEY, "a\"b").is_err());
        assert!(authorized_line(KEY, "a\\b").is_err());
    }

    #[test]
    fn authorising_twice_keeps_one_line_and_the_users_own() {
        let d = dir("twice");
        let path = authorized_keys_path(&d);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "ssh-ed25519 AAAAown my laptop\n").unwrap();
        authorize(&path, KEY, &command).unwrap();
        authorize(&path, KEY, &command).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("AAAAfirst").count(), 1);
        assert!(text.contains("AAAAown"));
    }

    #[test]
    fn unpairing_removes_only_the_connecting_keys_line() {
        let d = dir("unpair");
        let path = authorized_keys_path(&d);
        let second = "ssh-ed25519 AAAAsecond b";
        authorize(&path, KEY, &command).unwrap();
        authorize(&path, second, &command).unwrap();
        assert!(unauthorize(&path, &key_tag(KEY).unwrap()));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("AAAAfirst") && text.contains("AAAAsecond"));
    }

    #[test]
    fn unpairing_without_a_real_tag_removes_nothing() {
        let d = dir("badtag");
        let path = authorized_keys_path(&d);
        authorize(&path, KEY, &command).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        for tag in ["", "not-a-tag", "0123456789abcdef --key x"] {
            assert!(!unauthorize(&path, tag), "{tag:?}");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn a_users_own_line_naming_the_same_tag_is_never_removed() {
        let tag = key_tag(KEY).unwrap();
        let own = format!("ssh-ed25519 AAAAown my laptop --key {tag}\n");
        assert_eq!(without_tag(&own, &tag), None);
    }

    #[test]
    fn authorising_something_that_is_not_a_public_key_writes_nothing() {
        let d = dir("notkey");
        let path = authorized_keys_path(&d);
        for key in ["", "ssh-ed25519", "not a key at all"] {
            assert!(authorize(&path, key, &command).is_err());
        }
        assert!(!path.exists());
    }

    #[test]
    fn the_key_tag_is_sixteen_hex_digits_of_the_keys_body() {
        let tag = key_tag(KEY).unwrap();
        assert!(is_tag(&tag));
        assert_ne!(tag, key_tag("ssh-ed25519 AAAAsecond b").unwrap());
        assert_eq!(tag, key_tag("ssh-ed25519 AAAAfirst another comment").unwrap());
    }

    #[test]
    fn a_node_id_names_no_other_path() {
        assert!(is_node_id("0123abcd-4567"));
        for bad in ["", "-a", "../x", "a/b", "a b", &"a".repeat(65)] {
            assert!(!is_node_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_host_key_is_pinned_to_the_node_and_a_different_one_is_refused() {
        let first = vec!["ssh-ed25519 AAAAone".to_string()];
        let text = pin_host_keys("", "node-a", &first).unwrap().unwrap();
        assert_eq!(text, "ragon-node-node-a ssh-ed25519 AAAAone\n");
        assert_eq!(pin_host_keys(&text, "node-a", &first), Ok(None));
        let other = vec!["ssh-ed25519 AAAAtwo".to_string()];
        assert!(pin_host_keys(&text, "node-a", &other).is_err());
        // a key pinned for one node does not refuse another
        assert!(pin_host_keys(&text, "node-b", &other).unwrap().is_some());
        assert!(pin_host_keys("", "node-a", &[]).is_err());
    }

    #[test]
    fn every_connection_checks_the_pinned_key_and_offers_only_the_pairing_key() {
        let args = ssh_options(Path::new("/k"), Path::new("/kh"), "n1", 2222, false);
        assert!(args.windows(2).any(|w| w == ["-o", "StrictHostKeyChecking=yes"]));
        assert!(args.windows(2).any(|w| w == ["-o", "IdentitiesOnly=yes"]));
        assert!(args.windows(2).any(|w| w == ["-o", "HostKeyAlias=ragon-node-n1"]));
        assert!(args.windows(2).any(|w| w == ["-p", "2222"]));
        assert!(ssh_options(Path::new("/k"), Path::new("/kh"), "n1", 22, true).contains(&"StrictHostKeyChecking=accept-new".to_string()));
    }

    #[test]
    fn only_the_advertised_id_confirms_a_pairing() {
        assert_eq!(reported_node("{\"node\":\"n1\"}").as_deref(), Some("n1"));
        for bad in ["", "not json", "{}", "[]", "{\"node\":3}"] {
            assert_eq!(reported_node(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_record_carries_no_address() {
        let record = record_json("n1", "spark", "me", 22);
        assert!(record.get("address").is_none());
        assert_eq!(record["node"], "n1");
    }
}
