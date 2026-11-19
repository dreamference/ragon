//! The CA pinning store: `~/.ragon/nodes/<node id>/ca.pem` (spec §6, §15).
//!
//! Written at first contact with a node id and at pairing; a different CA presented later for the
//! same id is a [`Outcome::Changed`]. The pinned CA is also added to the user's own trust store
//! (never the system's) so browsers and the desktop app warn about nothing.

use std::path::{Path, PathBuf};

use anyhow::Context;

/// What a presented CA means against the store.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// First contact: the CA was stored at this path.
    Pinned(PathBuf),
    /// The stored CA is the presented one.
    Known(PathBuf),
    /// The stored CA is another one.
    Changed,
}

/// The pinned CA of a node id under `codex_home`. The id is reduced to a safe folder name.
pub fn path(codex_home: &Path, node_id: &str) -> PathBuf {
    let safe: String = node_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
    codex_home.join("nodes").join(safe.trim_matches('.')).join("ca.pem")
}

/// The certificate's text without line endings and spacing, so a CRLF copy compares equal.
fn canonical(pem: &str) -> String {
    pem.split_whitespace().collect()
}

/// Pins `presented` for `node_id` when none is stored; otherwise compares.
///
/// # Errors
/// The store cannot be written.
pub fn check_or_pin(codex_home: &Path, node_id: &str, presented: &str) -> anyhow::Result<Outcome> {
    let target = path(codex_home, node_id);
    if let Ok(stored) = std::fs::read_to_string(&target) {
        return Ok(if canonical(&stored) == canonical(presented) { Outcome::Known(target) } else { Outcome::Changed });
    }
    write_private(&target, presented.as_bytes())?;
    Ok(Outcome::Pinned(target))
}

/// Forgets a node's pin (`ragon login --reset-pin`). Returns whether there was one.
pub fn reset(codex_home: &Path, node_id: &str) -> bool {
    std::fs::remove_file(path(codex_home, node_id)).is_ok()
}

/// Writes a file only the current user can read, making its folders.
pub(crate) fn write_private(target: &Path, contents: &[u8]) -> anyhow::Result<()> {
    let dir = target.parent().context("a pin has no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("could not make {}", dir.display()))?;
    let staging = dir.join(format!(".{}.{}.tmp", target.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()), std::process::id()));
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staging).with_context(|| format!("could not write {}", staging.display()))?;
        file.write_all(contents)?;
    }
    std::fs::rename(&staging, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&staging);
    })?;
    restrict_to_current_user(target);
    Ok(())
}

/// The arguments of the command that leaves a Windows file to the current user alone: no inherited
/// entries, full control for the user. Codex sets mode 0600 on Unix only (spec §15).
pub fn windows_acl_args(file: &Path, user: &str) -> Vec<String> {
    vec![file.to_string_lossy().into_owned(), "/inheritance:r".into(), "/grant:r".into(), format!("{user}:(F)")]
}

/// Restricts a file to the current user on Windows; a no-op elsewhere (the mode did it).
pub fn restrict_to_current_user(file: &Path) {
    #[cfg(windows)]
    if let Ok(user) = std::env::var("USERNAME") {
        let _ = std::process::Command::new("icacls").args(windows_acl_args(file, &user)).output();
    }
    #[cfg(not(windows))]
    let _ = file;
}

/// The commands that add a CA file to the user's own trust store on `os` (`linux`, `macos`,
/// `windows`), each as a program and arguments. Nothing is system-wide.
pub fn trust_commands(os: &str, home: &Path, pem: &Path, name: &str) -> Vec<Vec<String>> {
    let pem_text = pem.to_string_lossy().into_owned();
    match os {
        "linux" => vec![vec![
            "certutil".into(),
            "-d".into(),
            format!("sql:{}", home.join(".pki").join("nssdb").display()),
            "-A".into(),
            "-t".into(),
            "C,,".into(),
            "-n".into(),
            format!("ragon-{name}"),
            "-i".into(),
            pem_text,
        ]],
        "macos" => vec![vec![
            "security".into(),
            "add-trusted-cert".into(),
            "-r".into(),
            "trustRoot".into(),
            "-k".into(),
            home.join("Library").join("Keychains").join("login.keychain-db").to_string_lossy().into_owned(),
            pem_text,
        ]],
        "windows" => vec![vec!["certutil".into(), "-user".into(), "-addstore".into(), "Root".into(), pem_text]],
        _ => Vec::new(),
    }
}

/// Where the user's certificate directory keeps the copy the desktop app reads (Linux).
pub fn user_certificate_copy(home: &Path, name: &str) -> PathBuf {
    home.join(".local").join("share").join("ca-certificates").join(format!("ragon-{name}.crt"))
}

/// Adds the pinned CA to the user's trust store, best effort: a missing tool or a refusal only
/// means a browser warns, so nothing here stops a start.
pub fn import_into_trust_store(pem: &Path, node_id: &str) {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) else { return };
    let name: String = node_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if cfg!(target_os = "linux") {
        let copy = user_certificate_copy(&home, &name);
        if let Some(dir) = copy.parent() {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::copy(pem, &copy);
        }
    }
    for command in trust_commands(std::env::consts::OS, &home, pem, &name) {
        let Some((program, args)) = command.split_first() else { continue };
        let _ = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output();
    }
}
