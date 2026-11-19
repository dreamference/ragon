//! The bearer the local tools send to the gate (specs/DREAMFERENCE_AUTH.md §14.5).
//!
//! The IDE plugin, the MCP tools and the code index have no sign-in screen. They read the active
//! entry of `~/.ragon/auth.json` (the version 2 layout of §11, or a one-entry file) through this
//! one helper, or take `RAGON_API_KEY`, and never ask for a password.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The variable that carries a key and wins over the file.
pub const RAGON_API_KEY_ENV: &str = "RAGON_API_KEY";

/// What a tool prints when there is no bearer.
pub const NOT_SIGNED_IN: &str = "Not signed in. Run ragon login.";

/// The bearer in an `auth.json` text: the active entry's key, else its access token. A file with
/// no `version` is one entry. `None` when the text holds no usable credential.
pub fn bearer_from_text(text: &str) -> Option<String> {
    let doc: Value = serde_json::from_str(text).ok()?;
    let entry = match doc.get("entries") {
        Some(entries) => {
            let active = doc.get("active").and_then(Value::as_str)?;
            entries.get(active)?
        }
        None => &doc,
    };
    let key = entry.get("OPENAI_API_KEY").and_then(Value::as_str).filter(|k| !k.is_empty());
    let token = entry.pointer("/tokens/access_token").and_then(Value::as_str).filter(|t| !t.is_empty());
    key.or(token).map(str::to_string)
}

/// The bearer for `home` (the Ragon home, `~/.ragon`): `env_key` when set and non-empty, else the
/// active entry of `<home>/auth.json`.
pub fn bearer(home: &Path, env_key: Option<&str>) -> Option<String> {
    if let Some(key) = env_key.filter(|k| !k.trim().is_empty()) {
        return Some(key.trim().to_string());
    }
    bearer_from_text(&std::fs::read_to_string(home.join("auth.json")).ok()?)
}

/// The Ragon home of this process: `CODEX_HOME`, else `~/.ragon`.
pub fn ragon_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".ragon"))
}

/// The bearer of this process, from the environment and the home it names.
pub fn current_bearer() -> Option<String> {
    bearer(&ragon_home()?, std::env::var(RAGON_API_KEY_ENV).ok().as_deref())
}

/// The `RAGON_API_KEY` a child tool is started with: the current bearer, or `None` (the child
/// then prints `NOT_SIGNED_IN` itself).
pub fn child_env() -> Option<(&'static str, String)> {
    current_bearer().map(|b| (RAGON_API_KEY_ENV, b))
}
