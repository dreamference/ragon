//! The account slash commands and the status line's Account item (specs/DREAMFERENCE_AUTH.md §8.4,
//! §11, §12, §13, §15).
//!
//! Patch `0039` adds `/whoami`, `/password`, `/apikey`, `/sessions`, `/users` and `/delete-account`
//! to the TUI with one dispatch arm into [`command`]. Everything that decides what is asked or how
//! an answer reads is pure ([`plan`], [`render`], [`account_from_auth`]); only [`command`] touches
//! the file, the environment and the network.

use std::time::Duration;

use super::strings::{self, Key};

use base64::Engine;
use serde_json::{Value, json};

/// What the active `auth.json` entry says about the signed-in user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// The username (the `email` claim holds it, spec §5.3).
    pub user: String,
    /// `user` or `admin`.
    pub role: String,
    /// The node's name (the `primary_name` claim), when the token has one.
    pub node: Option<String>,
    /// The bearer token the gate sees: the access token, else the stored API key.
    pub bearer: Option<String>,
}

/// One request to the gate.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The HTTP method.
    pub method: &'static str,
    /// The path under the gate's base URL.
    pub path: String,
    /// The JSON body, when there is one.
    pub body: Option<Value>,
}

/// What a command comes to: an answer without the network, or a request for the gate.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Lines to show now.
    Say(Vec<String>),
    /// A request to send.
    Ask(Request),
}

fn claims(token: &str) -> Option<Value> {
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
    serde_json::from_slice(&payload).ok()
}

/// The signed-in account from the text of `auth.json`: the active entry of a version 2 file, or
/// the single entry of an older one. `None` when nobody is signed in with a token.
pub fn account_from_auth(text: &str) -> Option<Account> {
    let file: Value = serde_json::from_str(text).ok()?;
    let entry = match file.get("entries") {
        Some(entries) => entries.get(file.get("active")?.as_str()?)?,
        None => &file,
    };
    let claims = claims(entry.pointer("/tokens/id_token")?.as_str()?)?;
    let text = |value: Option<&Value>| value.and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    let bearer = text(entry.pointer("/tokens/access_token")).or_else(|| text(entry.get("OPENAI_API_KEY")));
    Some(Account {
        user: text(claims.get("email"))?,
        role: text(claims.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")).unwrap_or_else(|| "user".to_string()),
        node: text(claims.pointer("/https:~1~1ragon.dev~1auth/primary_name")),
        bearer,
    })
}

/// The status line's Account item: `<username> @ <node name>`; `None` when nobody is signed in.
pub fn item(account: Option<&Account>) -> Option<String> {
    let account = account?;
    Some(match &account.node {
        Some(node) => format!("{} @ {node}", account.user),
        None => account.user.clone(),
    })
}

fn read_account() -> Option<Account> {
    let home = std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from).or_else(|| ragon_node_locator::home_dir().map(|home| home.join(".ragon")))?;
    account_from_auth(&std::fs::read_to_string(home.join("auth.json")).ok()?)
}

/// The Account item for the status line, read from the active entry of `auth.json`.
pub fn status_item() -> Option<String> {
    item(read_account().as_ref())
}

/// "Signed in as <user> (<role>) at <node>".
pub fn whoami(account: Option<&Account>, api_key: bool) -> String {
    match (account, api_key) {
        (Some(account), _) => match &account.node {
            Some(node) => strings::fmt(Key::SignedInAsAt, &[&account.user, &account.role, &node]),
            None => strings::fmt(Key::SignedInAs, &[&account.user, &account.role]),
        },
        (None, true) => strings::t(Key::UsingApiKey).to_string(),
        (None, false) => strings::t(Key::NotSignedIn).to_string(),
    }
}

fn usage(text: &str) -> Plan {
    Plan::Say(vec![strings::fmt(Key::Usage, &[&text])])
}

fn ask(method: &'static str, path: String, body: Option<Value>) -> Plan {
    Plan::Ask(Request { method, path, body })
}

/// Decides what a command does. `user` is the signed-in username, when known.
pub fn plan(cmd: &str, args: &str, user: Option<&str>) -> Plan {
    let words: Vec<&str> = args.split_whitespace().collect();
    let name_ok = |name: &str| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    match (cmd, words.as_slice()) {
        ("password", [current, new]) => ask("POST", "/ragon/v1/users/me/password".into(), Some(json!({"current": current, "new": new}))),
        ("password", _) => usage("/password <current> <new>  (an empty password: ragon login)"),
        ("apikey", []) => ask("GET", "/ragon/v1/users/me/keys".into(), None),
        ("apikey", ["new"]) => ask("POST", "/ragon/v1/users/me/keys".into(), Some(json!({}))),
        ("apikey", ["new", name]) if name_ok(name) => ask("POST", "/ragon/v1/users/me/keys".into(), Some(json!({"name": name}))),
        ("apikey", ["revoke", name]) if name_ok(name) => ask("DELETE", format!("/ragon/v1/users/me/keys/{name}"), None),
        ("apikey", _) => usage("/apikey [new [name] | revoke <name>]"),
        ("sessions", []) => ask("GET", "/ragon/v1/users/me/sessions".into(), None),
        ("sessions", ["revoke", "--all"]) => ask("DELETE", "/ragon/v1/users/me/sessions".into(), None),
        ("sessions", ["revoke", id]) if name_ok(id) => ask("DELETE", format!("/ragon/v1/users/me/sessions/{id}"), None),
        ("sessions", _) => usage("/sessions [revoke <id> | revoke --all]"),
        ("users", []) => ask("GET", "/ragon/v1/users".into(), None),
        ("users", [action @ ("reset" | "promote" | "demote"), name]) if name_ok(name) => ask("POST", format!("/ragon/v1/users/{name}/{action}"), None),
        ("users", ["remove", name]) if name_ok(name) => ask("DELETE", format!("/ragon/v1/users/{name}"), None),
        ("users", _) => usage("/users [reset|promote|demote|remove <name>]"),
        ("delete-account", [name]) if Some(*name) == user => ask("DELETE", "/ragon/v1/users/me".into(), None),
        ("delete-account", _) => usage("/delete-account <your username>  (the name confirms it)"),
        _ => Plan::Say(vec![format!("/{cmd} is not an account command.")]),
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Object(map) => map
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| format!("{k}={}", v.as_str().map_or_else(|| v.to_string(), str::to_string)))
            .collect::<Vec<_>>()
            .join("  "),
        other => other.as_str().map_or_else(|| other.to_string(), str::to_string),
    }
}

/// The lines for the gate's answer to a request made for `cmd`.
pub fn render(cmd: &str, status: u16, body: &str) -> Vec<String> {
    let json: Option<Value> = serde_json::from_str(body).ok();
    let message = json
        .as_ref()
        .and_then(|v| v.pointer("/error/message").or_else(|| v.get("error")).or_else(|| v.get("message")))
        .and_then(Value::as_str)
        .map(str::to_string);
    match status {
        200..=299 => {}
        401 => return vec![strings::t(Key::SessionEnded).to_string()],
        403 if cmd == "users" => return vec![strings::t(Key::NeedsAdmin).to_string()],
        _ => return vec![format!("/{cmd}: {}", message.unwrap_or_else(|| strings::fmt(Key::NodeAnswered, &[&status.to_string()])))],
    }
    let mut lines = match json {
        Some(Value::Array(items)) if items.is_empty() => vec![strings::t(Key::NothingToShow).to_string()],
        Some(Value::Array(items)) => items.iter().map(describe).collect(),
        Some(Value::Object(map)) => match map.get("key").and_then(Value::as_str) {
            Some(key) => vec![strings::fmt(Key::NewApiKey, &[key])],
            None => vec![describe(&Value::Object(map))],
        },
        _ => vec![strings::t(Key::Done).to_string()],
    };
    if lines.iter().all(String::is_empty) {
        lines = vec![strings::t(Key::Done).to_string()];
    }
    lines
}

fn send(gate: &str, request: &Request, bearer: Option<&str>) -> Result<(u16, String), String> {
    let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|e| e.to_string())?;
    let url = format!("{}{}", gate.trim_end_matches('/'), request.path);
    let ca = std::env::var_os(super::CA_ENV).and_then(|path| std::fs::read(path).ok());
    let request = request.clone();
    let bearer = bearer.map(str::to_string);
    // Its own thread and runtime: the TUI calls in from inside Codex's runtime, which cannot block.
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        runtime.block_on(async {
            let mut client = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10));
            if let Some(cert) = ca.and_then(|pem| reqwest::Certificate::from_pem(&pem).ok()) {
                client = client.add_root_certificate(cert);
            }
            let mut call = client.build().map_err(|e| e.to_string())?.request(method, url);
            if let Some(bearer) = bearer {
                call = call.bearer_auth(bearer);
            }
            if let Some(body) = &request.body {
                call = call.json(body);
            }
            let response = call.send().await.map_err(|_| "the node did not answer".to_string())?;
            let status = response.status().as_u16();
            Ok((status, response.text().await.unwrap_or_default()))
        })
    })
    .join()
    .map_err(|_| "the request failed".to_string())?
}

/// Runs one account command (`cmd` is the slash command's name, `args` the rest of the line) and
/// returns the lines to show.
pub fn command(cmd: &str, args: &str) -> Vec<String> {
    let account = read_account();
    let api_key = std::env::var_os(super::API_KEY_ENV).is_some_and(|v| !v.is_empty());
    if cmd == "whoami" && (account.is_some() || !api_key) {
        return vec![whoami(account.as_ref(), api_key)];
    }
    let request = match cmd {
        "whoami" => Request { method: "GET", path: "/ragon/v1/users/me".into(), body: None },
        _ => match plan(cmd, args, account.as_ref().map(|a| a.user.as_str())) {
            Plan::Say(lines) => return lines,
            Plan::Ask(request) => request,
        },
    };
    let Some(gate) = std::env::var(super::ISSUER_ENV).ok().filter(|g| !g.is_empty()) else {
        return vec![strings::t(Key::NoAccounts).to_string()];
    };
    let bearer = std::env::var(super::API_KEY_ENV).ok().filter(|k| !k.is_empty()).or_else(|| account.as_ref().and_then(|a| a.bearer.clone()));
    match send(&gate, &request, bearer.as_deref()) {
        Ok((status, body)) => render(cmd, status, &body),
        Err(error) => vec![format!("/{cmd}: {error}")],
    }
}
