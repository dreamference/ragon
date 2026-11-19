//! The account switcher's launcher half: `~/.ragon/auth.json` holds every primary's entry and an
//! `active` marker (spec §11). The launcher moves the marker under a lock and never touches an
//! entry; Codex's reader and writer for the layout are patch 0041.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

/// A lock older than this is a crashed process's, and is taken over.
const STALE_LOCK: Duration = Duration::from_secs(30);

/// Marks `primary_id` active in `auth.json` under `codex_home`.
///
/// No file: a version 2 file with no entries is made, so the first-run sign-in fills the named
/// primary's. A file with no `version` is the old single-entry layout and is left to Codex.
///
/// # Errors
/// The lock is not taken within ten seconds, or the file cannot be read or written.
pub fn set_active(codex_home: &Path, primary_id: &str) -> anyhow::Result<()> {
    let file = codex_home.join("auth.json");
    let _lock = Lock::take(&codex_home.join("auth.json.lock"))?;
    let current = match std::fs::read_to_string(&file) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error).with_context(|| format!("could not read {}", file.display())),
    };
    let Some(updated) = with_active(current.as_deref(), primary_id)? else { return Ok(()) };
    super::pin::write_private(&file, updated.as_bytes())
}

/// The text of the file with `active` set to `primary_id`; `None` when nothing has to be written.
///
/// # Errors
/// The text is not JSON.
pub fn with_active(current: Option<&str>, primary_id: &str) -> anyhow::Result<Option<String>> {
    let mut doc: Value = match current {
        None => json!({ "version": 2, "active": primary_id, "entries": {} }),
        Some(text) => serde_json::from_str(text).context("auth.json is not valid JSON")?,
    };
    let Some(object) = doc.as_object_mut() else { bail!("auth.json is not a JSON object") };
    if object.get("version").and_then(Value::as_u64) != Some(2) {
        // The old layout is one entry; Codex reads it as the entry of its org claim.
        return Ok(None);
    }
    if current.is_some() && object.get("active").and_then(Value::as_str) == Some(primary_id) && object.contains_key("entries") {
        return Ok(None);
    }
    object.insert("active".into(), Value::String(primary_id.to_string()));
    object.entry("entries").or_insert_with(|| Value::Object(Map::new()));
    Ok(Some(serde_json::to_string_pretty(&doc)? + "\n"))
}

/// An exclusive lock file, released when dropped.
struct Lock(std::path::PathBuf);

impl Lock {
    fn take(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let started = Instant::now();
        loop {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Ok(Self(path.to_path_buf())),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(path)
                        .and_then(|meta| meta.modified())
                        .ok()
                        .and_then(|time| time.elapsed().ok())
                        .is_some_and(|age| age > STALE_LOCK);
                    if stale {
                        let _ = std::fs::remove_file(path);
                    } else if started.elapsed() > Duration::from_secs(10) {
                        bail!("{} is locked by another ragon", path.display());
                    } else {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                Err(error) => return Err(error).with_context(|| format!("could not lock {}", path.display())),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
