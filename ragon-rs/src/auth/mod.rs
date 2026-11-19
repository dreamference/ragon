//! The launcher's half of the account system (specs/DREAMFERENCE_AUTH.md §5.1, §5.7, §6, §11, §15).
//!
//! At every start, once the node is resolved, [`start`] asks the node's gate who it is
//! (`/ragon/v1/info`), pins the node's CA at first contact, marks the primary's entry active in
//! `auth.json` and sets the environment every Codex process inherits. Everything here degrades to
//! a no-op against a node that has no gate yet (plain HTTP, no `/ragon/v1/info`): such a node is
//! served as before.

pub mod commands;
pub mod local;
pub mod pin;
pub mod screen;
pub mod slash;
pub mod switcher;

pub use local::{NOT_SIGNED_IN, bearer, bearer_from_text, child_env, current_bearer, ragon_home};

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};

/// Where Codex's device and password logins go.
pub const ISSUER_ENV: &str = "CODEX_APP_SERVER_LOGIN_ISSUER";
/// The refresh URL (`<gate>/oauth/token`).
pub const REFRESH_ENV: &str = "CODEX_REFRESH_TOKEN_URL_OVERRIDE";
/// The revoke URL (`<gate>/oauth/revoke`).
pub const REVOKE_ENV: &str = "CODEX_REVOKE_TOKEN_URL_OVERRIDE";
/// The CA file Codex's HTTP clients trust for the node.
pub const CA_ENV: &str = "CODEX_CA_CERTIFICATE";
/// Codex's own API key variable; it overrides the stored entry.
pub const API_KEY_ENV: &str = "CODEX_API_KEY";

/// What `GET /ragon/v1/info` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// The node's own id: the folder name of its pinned CA.
    pub node_id: String,
    /// The node's display name.
    pub node_name: String,
    /// The primary's id: the key of the `auth.json` entry.
    pub primary_id: String,
    /// The primary's display name.
    pub primary_name: String,
}

/// Reads the info document; `None` when it is not one (an older node answers something else).
pub fn parse_info(text: &str) -> Option<Info> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let field = |key: &str| json.get(key).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty());
    let node_id = field("node_id")?;
    let primary_id = field("primary_id").unwrap_or(node_id);
    Some(Info {
        node_id: node_id.to_string(),
        node_name: field("node_name").unwrap_or(node_id).to_string(),
        primary_id: primary_id.to_string(),
        primary_name: field("primary_name").unwrap_or(primary_id).to_string(),
    })
}

/// The provider's display name: the gate's host name, without scheme or port.
pub fn node_label(gate: &str) -> String {
    let rest = gate.split_once("://").map_or(gate, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = if host.starts_with('[') { host.split(']').next().map_or(host, |h| &h[1..]) } else { host.split(':').next().unwrap_or(host) };
    if host.is_empty() { crate::PROVIDER.to_string() } else { host.to_string() }
}

/// The key Codex gets as `CODEX_API_KEY`: `RAGON_API_KEY`, else `OPENAI_API_KEY`; an empty value
/// counts as unset.
pub fn api_key(ragon: Option<&str>, openai: Option<&str>) -> Option<String> {
    [ragon, openai].into_iter().flatten().map(str::trim).find(|key| !key.is_empty()).map(str::to_string)
}

/// The `User-Agent` suffix Codex appends to its own: `ragon/<version>`.
pub fn user_agent_suffix() -> String {
    format!("ragon/{}", env!("CARGO_PKG_VERSION"))
}

/// The environment every Codex process gets for this node, as name and value.
pub fn codex_env(gate: &str, ca: Option<&Path>, ragon_key: Option<&str>, openai_key: Option<&str>) -> Vec<(&'static str, String)> {
    let gate = gate.trim_end_matches('/');
    let mut env = vec![
        (ISSUER_ENV, gate.to_string()),
        (REFRESH_ENV, format!("{gate}/oauth/token")),
        (REVOKE_ENV, format!("{gate}/oauth/revoke")),
    ];
    if let Some(ca) = ca {
        env.push((CA_ENV, ca.to_string_lossy().into_owned()));
    }
    if let Some(key) = api_key(ragon_key, openai_key) {
        env.push((API_KEY_ENV, key));
    }
    env
}

/// The pinned CA's path when the gate is HTTPS and its CA is pinned; used by the tests as the
/// decision point of [`start`].
pub fn wants_pin(gate: &str) -> bool {
    gate.starts_with("https://")
}

async fn fetch(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.text().await.ok()
}

/// Does everything the launcher owes the account system at a start. See the module comment.
///
/// # Errors
/// A CA that changed for a known node id (spec §6) stops the launcher.
pub async fn start(gate: &str, codex_home: &Path) -> anyhow::Result<()> {
    let gate = gate.trim_end_matches('/');
    let mut ca: Option<PathBuf> = None;
    let mut known: Option<Info> = None;
    if wants_pin(gate) {
        // First contact has no CA to trust yet: the info and the CA are public documents, and what
        // they say is checked against the pin below and by Codex, which trusts only the pin.
        let client = reqwest::Client::builder()
            .no_proxy()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(5))
            .build()
            .context("could not build the HTTP client")?;
        if let Some(info) = fetch(&client, &format!("{gate}/ragon/v1/info")).await.as_deref().and_then(parse_info) {
            let presented = fetch(&client, &format!("{gate}/ragon/v1/ca.pem")).await;
            if let Some(presented) = presented.filter(|pem| pem.contains("BEGIN CERTIFICATE")) {
                match pin::check_or_pin(codex_home, &info.node_id, &presented)? {
                    pin::Outcome::Changed => bail!(
                        "The certificate of {} has changed. If the node was set up again, run ragon login --reset-pin.",
                        info.node_name
                    ),
                    pin::Outcome::Pinned(path) => {
                        pin::import_into_trust_store(&path, &info.node_id);
                        ca = Some(path);
                    }
                    pin::Outcome::Known(path) => ca = Some(path),
                }
            }
            switcher::set_active(codex_home, &info.primary_id)?;
            known = Some(info);
        }
    }
    // The sign-in screen (screen.rs) draws from what was found here and does no I/O of its own
    // until a grant is sent. The upgrade note needs a home used before and no entry for the primary.
    let label = node_label(gate);
    let (node_name, primary_name) = known.as_ref().map_or((label.clone(), label), |info| (info.node_name.clone(), info.primary_name.clone()));
    screen::set_node(screen::Node {
        gate: gate.to_string(),
        node_name,
        primary_name,
        ca: ca.clone(),
        codex_home: codex_home.to_path_buf(),
        upgrade: screen::used_before(codex_home),
    });
    let ragon_key = std::env::var("RAGON_API_KEY").ok();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();
    // SAFETY: called from the launcher's start-up path before the Codex session spawns threads of
    // its own, as `proxy.rs` and `home.rs` do for theirs.
    unsafe {
        for (name, value) in codex_env(gate, ca.as_deref(), ragon_key.as_deref(), openai_key.as_deref()) {
            std::env::set_var(name, value);
        }
    }
    if let Ok(mut suffix) = codex_login::default_client::USER_AGENT_SUFFIX.lock() {
        *suffix = Some(user_agent_suffix());
    }
    Ok(())
}
