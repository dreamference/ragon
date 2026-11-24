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
pub mod signin_rpc;
pub mod slash;
pub mod strings;
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
    /// Whether the gate advertises an accounts service (`"accounts": true`; 0.9.9 or later).
    pub accounts: bool,
}

/// Reads the info document; `None` when it is not one (an older node answers something else).
pub fn parse_info(text: &str) -> Option<Info> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let field = |key: &str| json.get(key).and_then(serde_json::Value::as_str).filter(|v| !v.is_empty());
    let node_id = field("node_id")?;
    let primary_id = field("primary_id").unwrap_or(node_id);
    Some(Info {
        node_id: node_id.to_string(),
        node_name: field("node_name").or_else(|| field("name")).unwrap_or(node_id).to_string(),
        primary_id: primary_id.to_string(),
        primary_name: field("primary_name").unwrap_or(primary_id).to_string(),
        accounts: json.get("accounts").is_some_and(|v| v.as_bool().unwrap_or(!v.is_null())),
    })
}

/// What `GET /ragon/v1/setup` says (§8.6): whether the node still has no account, and whether
/// this caller is on the node itself and so may make the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// The node has no account yet.
    pub open: bool,
    /// The caller is on the node (the gate saw a local peer).
    pub local: bool,
}

/// Reads the setup document; `None` when it is not one (an older node has no such route).
pub fn parse_setup(text: &str) -> Option<Setup> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let open = json.get("setup")?.as_bool()?;
    Some(Setup { open, local: json.get("local").and_then(serde_json::Value::as_bool).unwrap_or(false) })
}

/// What an interactive start does on a node with accounts (§8.6): `Create` opens the sign-in
/// screen at the first account's form, `Elsewhere` says where that account is made and runs
/// without sign-in, `SignIn` is the usual screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstRun {
    SignIn,
    Create,
    Elsewhere,
}

/// The decision of [`FirstRun`]: only an interactive start asks, so unattended callers (`exec`,
/// Night Shift, SWE-bench, CI) behave as before whatever the node says.
pub fn first_run(interactive: bool, setup: Option<&Setup>) -> FirstRun {
    match setup {
        Some(setup) if interactive && setup.open => {
            if setup.local {
                FirstRun::Create
            } else {
                FirstRun::Elsewhere
            }
        }
        _ => FirstRun::SignIn,
    }
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

/// The node's setup document, or `None` when it has none or gave no answer.
async fn fetch_setup(gate: &str) -> Option<Setup> {
    let client = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(wants_pin(gate)).timeout(Duration::from_secs(5)).build().ok()?;
    fetch(&client, &format!("{gate}/ragon/v1/setup")).await.as_deref().and_then(parse_setup)
}

/// The one line printed against a gate with no accounts service (spec §7, §15).
pub fn no_accounts_line(gate: &str) -> String {
    format!("node {} has no accounts yet (0.9.8 or older); running without sign-in", node_label(gate))
}

/// What a gate's info says of accounts, telling "no" from "no answer": `Some(true)` when it
/// advertises an accounts service, `Some(false)` when it answered without one (a 4xx such as an
/// older node's 404, or an info without the field), `None` when it gave no answer (refused, timed
/// out, a 5xx). The chat bridge acts on a definite answer only (AUTH §14.1).
pub async fn gate_accounts(gate: &str) -> Option<bool> {
    let gate = gate.trim_end_matches('/');
    let client = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(wants_pin(gate)).timeout(Duration::from_secs(5)).build().ok()?;
    let response = client.get(format!("{gate}/ragon/v1/info")).send().await.ok()?;
    let status = response.status();
    if status.is_server_error() {
        return None;
    }
    let text = response.text().await.ok()?;
    if !status.is_success() {
        // A gate that refuses plain HTTP from this machine has said nothing about accounts.
        return if https_required(status.as_u16(), &text) { None } else { Some(false) };
    }
    Some(parse_info(&text).is_some_and(|info| info.accounts))
}

/// Whether a gate runs in account mode: its info says it has an accounts service. A 404, a refused
/// connection or an info without the field mean no (a 0.9.8 gate or a plain model server).
pub async fn gate_has_accounts(gate: &str) -> bool {
    plain_accounts(gate).await == PlainAnswer::Accounts
}

/// What a gate reached over plain HTTP says of accounts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlainAnswer {
    /// Its info advertises an accounts service.
    Accounts,
    /// It refuses plain HTTP from this machine (403 `https_required`, spec §6): a 0.9.9 gate.
    HttpsRequired,
    /// Anything else: a 404 or an info without accounts (0.9.8 or older), no answer.
    NoAccounts,
}

/// Reads a plain gate's answer to `GET /ragon/v1/info`; see [`PlainAnswer`].
pub fn plain_answer(status: u16, body: &str) -> PlainAnswer {
    if https_required(status, body) {
        PlainAnswer::HttpsRequired
    } else if (200..300).contains(&status) && parse_info(body).is_some_and(|info| info.accounts) {
        PlainAnswer::Accounts
    } else {
        PlainAnswer::NoAccounts
    }
}

async fn plain_accounts(gate: &str) -> PlainAnswer {
    let gate = gate.trim_end_matches('/');
    let Ok(client) = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(wants_pin(gate)).timeout(Duration::from_secs(5)).build() else {
        return PlainAnswer::NoAccounts;
    };
    let Ok(response) = client.get(format!("{gate}/ragon/v1/info")).send().await else {
        return PlainAnswer::NoAccounts;
    };
    let status = response.status().as_u16();
    plain_answer(status, &response.text().await.unwrap_or_default())
}

/// The gate's refusal of a request over plain HTTP from another machine (spec §6): `403` with
/// `{"error": {"code": "https_required", …}}`.
pub fn https_required(status: u16, body: &str) -> bool {
    status == 403
        && serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .is_some_and(|json| json.pointer("/error/code").and_then(serde_json::Value::as_str) == Some("https_required"))
}

/// The line printed when the gate refuses plain HTTP and this session could not move to HTTPS: it
/// runs without sign-in, and says why (instead of [`no_accounts_line`], which would be untrue).
pub fn https_required_line(gate: &str) -> String {
    HTTPS_FALLBACK.get().cloned().unwrap_or_else(|| {
        format!(
            "node {} signs in over https only and this machine reaches it over plain http ({gate}); running without sign-in",
            node_label(gate)
        )
    })
}

/// Why [`secure_gate`] kept a node with accounts on plain HTTP, said once by [`start`].
static HTTPS_FALLBACK: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// A host label (`dgx-a`): what a node's name must be before `<name>.local` is built from it.
pub fn is_host_label(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The HTTPS gate of a node reached at `http_host` (spec §5.1): the same port; the host as it is
/// when it is a name, `<node_name>.local` when it is an address, because the node's certificate
/// names `<name>.local` and its host name, never an address (§6). `None` when `http_host` is not
/// a plain-HTTP URL or the name is not a host label.
pub fn https_gate_for(http_host: &str, node_name: &str) -> Option<String> {
    let url = reqwest::Url::parse(http_host.trim()).ok()?;
    if url.scheme() != "http" {
        return None;
    }
    let port = url.port_or_known_default()?;
    let host = url.host_str()?;
    let is_address = host.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok();
    let name = if is_address {
        if !is_host_label(node_name) {
            return None;
        }
        format!("{node_name}.local")
    } else {
        host.to_string()
    };
    Some(format!("https://{name}:{port}"))
}

/// The same URL over TLS: `http://h:p` -> `https://h:p` (the gate tells TLS from plain by the first
/// byte, so one port serves both).
fn same_authority_https(http_host: &str) -> Option<String> {
    let rest = http_host.trim().trim_end_matches('/').strip_prefix("http://")?;
    Some(format!("https://{rest}"))
}

/// What [`secure_gate`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Secured {
    /// Use the host as it is: already HTTPS, on this machine, or a node without accounts.
    AsIs,
    /// The node has accounts: use this HTTPS gate. Its CA is pinned and trusted by this process's
    /// direct clients (`proxy::direct_client`); [`start`] hands it to Codex.
    Https(String),
}

/// Moves a discovered node with accounts from plain HTTP to its HTTPS gate (AUTH §5.1, §6): the
/// advertisement carries an address and a port and no scheme, and the gate refuses sign-in, tokens
/// and keys over plain HTTP from another machine. `node_id` and `node_name` are the
/// advertisement's.
///
/// The node's info and CA are fetched over TLS at the same address without verification, as
/// [`start`] does at first contact; the gate is then asked again by name, trusting only that CA,
/// and the CA is pinned only once that works. A node pinned before is never used over plain HTTP
/// again (a stored token would cross the network in the clear): while it does not answer (not up
/// yet) its HTTPS gate is used with the pinned CA, and the model wait says so. A node never pinned
/// whose HTTPS gate cannot be reached by name falls back to plain HTTP without sign-in, and
/// [`start`] says why.
///
/// # Errors
/// A pinned node that answers but not by name, or whose CA changed.
pub async fn secure_gate(host: &str, codex_home: &Path, node_id: &str, node_name: &str) -> anyhow::Result<Secured> {
    let host = host.trim().trim_end_matches('/');
    let Some(tls) = same_authority_https(host).filter(|_| !crate::host_is_local(host)) else {
        return Ok(Secured::AsIs);
    };
    let pin_of = |id: &str| Some(pin::path(codex_home, id)).filter(|path| !id.is_empty() && path.is_file());
    let lenient = reqwest::Client::builder()
        .no_proxy()
        .danger_accept_invalid_certs(true)
        .timeout(Duration::from_secs(3))
        .build()
        .context("could not build the HTTP client")?;
    let Some(info) = fetch(&lenient, &format!("{tls}/ragon/v1/info")).await.as_deref().and_then(parse_info) else {
        // An older node (no TLS on its port), or one that is not up yet.
        if let Some(path) = pin_of(node_id)
            && let Some(gate) = https_gate_for(host, node_name)
            && let Ok(pem) = std::fs::read(&path)
        {
            crate::proxy::trust_node_ca(&pem);
            return Ok(Secured::Https(gate));
        }
        return Ok(Secured::AsIs);
    };
    let pinned = pin_of(&info.node_id).is_some() || pin_of(node_id).is_some();
    if !info.accounts && !pinned {
        return Ok(Secured::AsIs);
    }
    let presented = fetch(&lenient, &format!("{tls}/ragon/v1/ca.pem")).await.filter(|pem| pem.contains("BEGIN CERTIFICATE"));
    let gate = https_gate_for(host, &info.node_name);
    let reached = match (&presented, &gate) {
        (Some(pem), Some(gate)) => answers_by_name(gate, pem).await,
        _ => false,
    };
    let (Some(presented), Some(gate), true) = (presented, gate.clone(), reached) else {
        if pinned {
            bail!("{}", no_downgrade_line(&info.node_name, gate.as_deref()));
        }
        let _ = HTTPS_FALLBACK.set(fallback_line(&info.node_name, gate.as_deref()));
        return Ok(Secured::AsIs);
    };
    match pin::check_or_pin(codex_home, &info.node_id, &presented)? {
        pin::Outcome::Changed => bail!(
            "The certificate of {} has changed. If the node was set up again, run ragon login --reset-pin.",
            info.node_name
        ),
        pin::Outcome::Pinned(path) => pin::import_into_trust_store(&path, &info.node_id),
        pin::Outcome::Known(_) => {}
    }
    crate::proxy::trust_node_ca(presented.as_bytes());
    Ok(Secured::Https(gate))
}

/// Why a node with accounts stays on plain HTTP this session (said by [`start`]).
pub fn fallback_line(node_name: &str, gate: Option<&str>) -> String {
    match gate {
        Some(gate) => format!(
            "node {node_name} has accounts and signs in only at {gate}, which does not answer from this machine \
             (does {} resolve here?); running without sign-in over plain http",
            node_label(gate)
        ),
        None => format!("node {node_name} has accounts and signs in over https only; running without sign-in over plain http"),
    }
}

/// Refuses plain HTTP to a node this machine has pinned (and so may hold a token for).
pub fn no_downgrade_line(node_name: &str, gate: Option<&str>) -> String {
    let at = gate.map(|gate| format!(" at {gate}")).unwrap_or_default();
    let name = gate.map_or_else(|| format!("{node_name}.local"), node_label);
    format!(
        "Node {node_name} has accounts and this machine signed in to it over https before, but its https gate{at} \
         does not answer from here (does {name} resolve?). Ragon does not use plain http for it."
    )
}

/// Whether the HTTPS gate answers its info by name, trusting only the presented CA: the name
/// resolves here and the certificate names it.
async fn answers_by_name(gate: &str, pem: &str) -> bool {
    let Ok(cert) = reqwest::Certificate::from_pem(pem.as_bytes()) else {
        return false;
    };
    let Ok(client) = reqwest::Client::builder().no_proxy().add_root_certificate(cert).timeout(Duration::from_secs(3)).build() else {
        return false;
    };
    fetch(&client, &format!("{gate}/ragon/v1/info")).await.as_deref().and_then(parse_info).is_some()
}


/// Does everything the launcher owes the account system at a start. See the module comment.
/// `interactive` is a start a person sits at: only then is a node with no account yet asked about
/// its first (§8.6).
///
/// # Errors
/// A CA that changed for a known node id (spec §6) stops the launcher.
pub async fn start(gate: &str, codex_home: &Path, interactive: bool) -> anyhow::Result<bool> {
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
            if info.accounts {
                switcher::set_active(codex_home, &info.primary_id)?;
            }
            known = Some(info);
        }
    }
    let mut accounts = if wants_pin(gate) {
        let accounts = known.as_ref().is_some_and(|info| info.accounts);
        if !accounts {
            eprintln!("{}", no_accounts_line(gate));
        }
        accounts
    } else {
        match plain_accounts(gate).await {
            PlainAnswer::Accounts => true,
            PlainAnswer::HttpsRequired => {
                eprintln!("{}", https_required_line(gate));
                false
            }
            PlainAnswer::NoAccounts => {
                eprintln!("{}", no_accounts_line(gate));
                false
            }
        }
    };
    // The sign-in screen (screen.rs) draws from what was found here and does no I/O of its own
    // until a grant is sent. The upgrade note needs a home used before and no entry for the primary.
    let label = node_label(gate);
    let (node_name, primary_name) = known.as_ref().map_or((label.clone(), label), |info| (info.node_name.clone(), info.primary_name.clone()));
    let setup = if accounts && interactive { fetch_setup(gate).await } else { None };
    let first = match first_run(interactive, setup.as_ref()) {
        FirstRun::Create => true,
        FirstRun::Elsewhere => {
            // No form that would only be refused: the gate serves without sign-in until then.
            eprintln!("{}", strings::fmt(strings::Key::FirstAccountElsewhere, &[&primary_name]));
            accounts = false;
            false
        }
        FirstRun::SignIn => false,
    };
    screen::set_node(screen::Node {
        gate: gate.to_string(),
        node_name,
        primary_name,
        ca: ca.clone(),
        codex_home: codex_home.to_path_buf(),
        upgrade: screen::used_before(codex_home),
        first,
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
    Ok(accounts)
}
