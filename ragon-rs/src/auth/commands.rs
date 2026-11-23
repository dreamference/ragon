//! `ragon login`, `ragon login status`, `ragon sessions`, `ragon logout --all` and the auth line of
//! `ragon doctor` (specs/DREAMFERENCE_AUTH.md §11). The launcher handles the password forms, the
//! status and the sessions; `--device-auth`, `--with-api-key` and a plain `logout` go to Codex,
//! which this module only tells the issuer.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use base64::Engine;
use serde_json::{Value, json};

use super::strings::{self, Key, Lang};
use super::{Info, pin, switcher};

/// The OAuth client id the launcher sends; the primary records and counts it.
const CLIENT_ID: &str = "ragon-cli";

/// What a command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cmd {
    /// The two-choice prompt.
    Screen,
    /// A password grant (or account creation) with the username given; no password on the line.
    Password {
        /// The account name.
        username: String,
        /// Create the account instead of signing in.
        create: bool,
        /// Read the password from stdin without a prompt.
        stdin: bool,
    },
    /// `--create` alone: the create form.
    CreateScreen,
    /// `ragon login status [--all]`.
    Status {
        /// List every primary's entry.
        all: bool,
    },
    /// `ragon login status --json`: what the desktop app's account window shows, as one JSON line.
    StatusJson,
    /// `ragon sessions [--revoke <id>]`.
    Sessions {
        /// The session to revoke, if one was named.
        revoke: Option<String>,
    },
    /// `ragon logout --all`.
    LogoutAll,
    /// Hand the arguments to Codex with the issuer set.
    Forward(Vec<String>),
}

/// Reads the arguments after `login` (`logout` for `logout_args`).
///
/// # Errors
/// A usage message for a flag combination the forms do not take.
pub fn parse_login(args: &[String]) -> Result<Cmd, String> {
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    match args.first().map(String::as_str) {
        Some("status") if has("--json") => return Ok(Cmd::StatusJson),
        Some("status") => return Ok(Cmd::Status { all: has("--all") }),
        Some("sessions") => return parse_sessions(&args[1..]),
        _ => {}
    }
    if has("--device-auth") || has("--with-api-key") || has("--help") || has("-h") {
        let mut forward = vec!["login".to_string()];
        forward.extend(args.iter().cloned());
        return Ok(Cmd::Forward(forward));
    }
    let mut username = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--username" => username = Some(iter.next().ok_or("--username needs a name")?.clone()),
            "--password-stdin" | "--create" => {}
            other if other.starts_with("--username=") => username = Some(other["--username=".len()..].to_string()),
            other => return Err(format!("unknown argument `{other}` for ragon login")),
        }
    }
    let (create, stdin) = (has("--create"), has("--password-stdin"));
    match username {
        Some(username) => Ok(Cmd::Password { username, create, stdin }),
        None if stdin => Err("--password-stdin needs --username <name>".to_string()),
        None if create => Ok(Cmd::CreateScreen),
        None => Ok(Cmd::Screen),
    }
}

/// Reads the arguments after `sessions`.
///
/// # Errors
/// A usage message.
pub fn parse_sessions(args: &[String]) -> Result<Cmd, String> {
    match args {
        [] => Ok(Cmd::Sessions { revoke: None }),
        [flag, id] if flag == "--revoke" => Ok(Cmd::Sessions { revoke: Some(id.clone()) }),
        [flag] if flag.starts_with("--revoke=") => Ok(Cmd::Sessions { revoke: Some(flag["--revoke=".len()..].to_string()) }),
        _ => Err("usage: ragon sessions [--revoke <id>]".to_string()),
    }
}

/// Reads the arguments after `logout`: `--all` is the launcher's, everything else Codex's.
pub fn parse_logout(args: &[String]) -> Cmd {
    if args.iter().any(|arg| arg == "--all") {
        return Cmd::LogoutAll;
    }
    let mut forward = vec!["logout".to_string()];
    forward.extend(args.iter().cloned());
    Cmd::Forward(forward)
}

/// The arguments Codex's `login` gets for a forwarded form: `--device-auth` names the issuer.
pub fn with_issuer(mut args: Vec<String>, gate: &str) -> Vec<String> {
    if args.iter().any(|arg| arg == "--device-auth") && !args.iter().any(|arg| arg.starts_with("--experimental_issuer")) {
        args.push("--experimental_issuer".to_string());
        args.push(gate.trim_end_matches('/').to_string());
    }
    args
}

/// What running a command came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The exit status when nothing is forwarded.
    pub code: i32,
    /// Arguments to hand to Codex, issuer included, when the launcher's part is done.
    pub forward: Option<Vec<String>>,
}

impl Outcome {
    fn done(code: i32) -> Self {
        Self { code, forward: None }
    }
}

/// A connection to the node's gate: where to ask, whom to trust and which entry is ours.
pub struct Primary {
    /// The gate's base URL, without a trailing slash.
    pub gate: String,
    /// What `/ragon/v1/info` said, when the gate has one.
    pub info: Option<Info>,
    home: PathBuf,
    client: reqwest::Client,
    ca_pem: Option<Vec<u8>>,
}

fn http_error(error: reqwest::Error) -> anyhow::Error {
    anyhow::anyhow!("could not reach the accounts server: {error}")
}

/// Asks the gate who it is, pins its CA at first contact and builds the client Codex's own trust
/// would give: the pinned CA only.
///
/// # Errors
/// The client cannot be built or the node's certificate changed since it was pinned.
pub async fn connect(gate: &str, codex_home: &Path) -> anyhow::Result<Primary> {
    let gate = gate.trim_end_matches('/').to_string();
    let base = || reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10));
    let mut info = None;
    let mut ca_pem: Option<Vec<u8>> = None;
    if super::wants_pin(&gate) {
        let lenient = base().danger_accept_invalid_certs(true).build().context("could not build the HTTP client")?;
        if let Some(found) = get_text(&lenient, &format!("{gate}/ragon/v1/info")).await.as_deref().and_then(super::parse_info) {
            if let Some(pem) = get_text(&lenient, &format!("{gate}/ragon/v1/ca.pem")).await.filter(|pem| pem.contains("BEGIN CERTIFICATE")) {
                let path = match pin::check_or_pin(codex_home, &found.node_id, &pem)? {
                    pin::Outcome::Changed => bail!(
                        "The certificate of {} has changed. If the node was set up again, run ragon login --reset-pin.",
                        found.node_name
                    ),
                    pin::Outcome::Pinned(path) | pin::Outcome::Known(path) => path,
                };
                ca_pem = std::fs::read(path).ok();
            }
            info = Some(found);
        }
    }
    let mut builder = base();
    if let Some(pem) = &ca_pem
        && let Ok(cert) = reqwest::Certificate::from_pem(pem)
    {
        builder = builder.add_root_certificate(cert);
    }
    let client = builder.build().context("could not build the HTTP client")?;
    if info.is_none() {
        info = get_text(&client, &format!("{gate}/ragon/v1/info")).await.as_deref().and_then(super::parse_info);
    }
    Ok(Primary { gate, info, home: codex_home.to_path_buf(), client, ca_pem })
}

/// `ragon login status --json`: one JSON line the desktop app's account window is drawn from
/// (docs/dev/desktop.md, "Accounts"), never an error: an unreachable gate fills `error`.
///
/// `text` is the string table in `lang`, placeholders unfilled, keyed by the [`Key`] names.
/// `accounts` is the gate's info, `setup` and `local` its `GET /ragon/v1/setup` (`false` when the
/// gate has none), `signedIn` whether the stored entry holds a session, `user` and `role` its own claims (read offline, as `ragon
/// doctor` does), `apiKey` whether the bearer is a key rather than a session.
pub async fn status_json(gate: &str, codex_home: &Path, lang: Lang) -> Value {
    match connect(gate, codex_home).await {
        Ok(primary) => status_doc(&primary, lang).await,
        Err(error) => {
            let mut doc = status_base(lang);
            doc["node"] = Value::String(super::node_label(gate.trim_end_matches('/')));
            doc["error"] = Value::String(format!("{error:#}"));
            doc
        }
    }
}

fn status_base(lang: Lang) -> Value {
    let text: serde_json::Map<String, Value> =
        Key::ALL.iter().map(|key| (format!("{key:?}"), Value::String(strings::text(lang, *key).to_string()))).collect();
    json!({
        "lang": lang.code(), "text": text, "reachable": false, "accounts": false, "setup": false, "local": false,
        "node": null, "primary": null, "signedIn": false, "user": null, "role": null, "apiKey": false, "error": null,
    })
}

/// [`status_json`] for a gate already reached.
pub async fn status_doc(primary: &Primary, lang: Lang) -> Value {
    let mut doc = status_base(lang);
    doc["node"] = Value::String(primary.node_name());
    doc["primary"] = Value::String(primary.primary_name());
    if let Some(info) = &primary.info {
        doc["reachable"] = Value::Bool(true);
        doc["accounts"] = Value::Bool(info.accounts);
    }
    if doc["accounts"] == Value::Bool(true)
        && let Some((setup, local)) = primary.setup().await
    {
        doc["setup"] = Value::Bool(setup);
        doc["local"] = Value::Bool(local);
    }
    let entry = primary.entry();
    let session = entry.as_ref().and_then(|e| e.pointer("/tokens/access_token")).and_then(Value::as_str).is_some_and(|t| !t.is_empty());
    if session && let Some((user, role)) = entry.as_ref().and_then(claims_of) {
        doc["user"] = Value::String(user);
        doc["role"] = Value::String(role);
    }
    doc["signedIn"] = Value::Bool(session);
    doc["apiKey"] = Value::Bool(!session && primary.bearer().is_some());
    doc
}

async fn get_text(client: &reqwest::Client, url: &str) -> Option<String> {
    let response = client.get(url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.text().await.ok()
}

impl Primary {
    /// The key of this primary's entry in `auth.json`.
    pub fn primary_id(&self) -> String {
        self.info.as_ref().map_or_else(|| super::node_label(&self.gate), |info| info.primary_id.clone())
    }

    /// The primary's display name.
    pub fn primary_name(&self) -> String {
        self.info.as_ref().map_or_else(|| super::node_label(&self.gate), |info| info.primary_name.clone())
    }

    /// The node's display name.
    pub fn node_name(&self) -> String {
        self.info.as_ref().map_or_else(|| super::node_label(&self.gate), |info| info.node_name.clone())
    }

    fn entry(&self) -> Option<Value> {
        switcher::read_state(&self.home).ok()?.entries.get(&self.primary_id()).cloned()
    }

    /// The bearer this machine presents: the entry's access token or stored key, else the
    /// environment's key.
    fn bearer(&self) -> Option<String> {
        let entry = self.entry();
        let from = |pointer: &str| entry.as_ref()?.pointer(pointer)?.as_str().filter(|v| !v.is_empty()).map(str::to_string);
        from("/tokens/access_token")
            .or_else(|| from("/OPENAI_API_KEY"))
            .or_else(|| super::api_key(std::env::var("RAGON_API_KEY").ok().as_deref(), std::env::var("OPENAI_API_KEY").ok().as_deref()))
    }

    async fn refresh(&self) -> bool {
        let Some(mut entry) = self.entry() else { return false };
        let Some(refresh) = entry.pointer("/tokens/refresh_token").and_then(Value::as_str).map(str::to_string) else { return false };
        let body = json!({ "grant_type": "refresh_token", "refresh_token": refresh, "client_id": CLIENT_ID });
        let Ok(response) = self.client.post(format!("{}/oauth/token", self.gate)).json(&body).send().await else { return false };
        if !response.status().is_success() {
            return false;
        }
        let Ok(tokens) = response.json::<Value>().await else { return false };
        for key in ["id_token", "access_token"] {
            if let Some(value) = tokens.get(key).cloned() {
                entry["tokens"][key] = value;
            }
        }
        entry["last_refresh"] = Value::String(chrono::Utc::now().to_rfc3339());
        switcher::save_entry(&self.home, &self.primary_id(), entry).is_ok()
    }

    /// A request with this machine's bearer; one refresh and retry when the gate says it expired.
    async fn authed(&self, method: reqwest::Method, path: &str) -> anyhow::Result<reqwest::Response> {
        self.authed_json(method, path, None).await
    }

    /// [`Primary::authed`] with a JSON body.
    async fn authed_json(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> anyhow::Result<reqwest::Response> {
        let Some(bearer) = self.bearer() else { bail!("not signed in") };
        let send = |bearer: String| {
            let request = self.client.request(method.clone(), format!("{}{path}", self.gate)).bearer_auth(bearer);
            match &body {
                Some(body) => request.json(body),
                None => request,
            }
            .send()
        };
        let response = send(bearer).await.map_err(http_error)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && self.refresh().await
            && let Some(bearer) = self.bearer()
        {
            return send(bearer).await.map_err(http_error);
        }
        Ok(response)
    }

    async fn token_request(&self, path: &str, body: Value) -> anyhow::Result<Result<Value, String>> {
        let response = self.client.post(format!("{}{path}", self.gate)).json(&body).send().await.map_err(http_error)?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if status.is_success() && json.get("access_token").is_some() {
            return Ok(Ok(json));
        }
        let code = json.get("error").and_then(Value::as_str).unwrap_or("");
        Ok(Err(match (status.as_u16(), code) {
            (_, "password_change_required") => "Your password must be changed first: sign in on the node's web page or ask its admin.".to_string(),
            (409, _) => "That username is taken.".to_string(),
            (400 | 422, _) if path.contains("/users") => "Usernames are 1-32 letters, digits, '.', '_' or '-'.".to_string(),
            (400 | 401, _) => "Wrong username or password.".to_string(),
            (code, _) => format!("{} answered {code}.", self.primary_name()),
        }))
    }

    /// Signs in with the password grant and stores the entry.
    ///
    /// # Errors
    /// The accounts server is unreachable or the entry cannot be written; a refusal is `Ok(Err(text))`.
    pub async fn sign_in(&self, username: &str, password: &str) -> anyhow::Result<Result<(), String>> {
        let body = json!({
            "grant_type": "password", "username": username, "password": password,
            "client_id": CLIENT_ID, "machine": machine_name(),
        });
        self.store(self.token_request("/oauth/token", body).await?)
    }

    /// Creates the account and stores the entry.
    ///
    /// # Errors
    /// As [`Primary::sign_in`].
    pub async fn create_account(&self, username: &str, password: &str) -> anyhow::Result<Result<(), String>> {
        let body = json!({ "username": username, "password": password, "client_id": CLIENT_ID, "machine": machine_name() });
        self.store(self.token_request("/ragon/v1/users", body).await?)
    }

    fn store(&self, tokens: Result<Value, String>) -> anyhow::Result<Result<(), String>> {
        let tokens = match tokens {
            Ok(tokens) => tokens,
            Err(text) => return Ok(Err(text)),
        };
        let text = |key: &str| tokens.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let entry = json!({
            "auth_mode": "chatgpt",
            "OPENAI_API_KEY": null,
            "tokens": {
                "id_token": text("id_token"),
                "access_token": text("access_token"),
                "refresh_token": text("refresh_token"),
                "account_id": null,
            },
            "last_refresh": chrono::Utc::now().to_rfc3339(),
        });
        switcher::save_entry(&self.home, &self.primary_id(), entry)?;
        Ok(Ok(()))
    }

    /// What the gate's `GET /ragon/v1/setup` says: whether the node still needs its first account
    /// and whether this caller is on the node itself. `None` when the gate has no such page.
    pub async fn setup(&self) -> Option<(bool, bool)> {
        let text = get_text(&self.client, &format!("{}/ragon/v1/setup", self.gate)).await?;
        super::parse_setup(&text).map(|setup| (setup.open, setup.local))
    }

    /// The username and role in the id token of this primary's stored entry.
    pub fn identity(&self) -> Option<(String, String)> {
        claims_of(&self.entry()?)
    }

    async fn me(&self) -> Option<(String, String)> {
        let response = self.authed(reqwest::Method::GET, "/ragon/v1/users/me").await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let json: Value = response.json().await.ok()?;
        let field = |key: &str| json.get(key).and_then(Value::as_str).map(str::to_string);
        Some((field("username")?, field("role").unwrap_or_else(|| "user".to_string())))
    }
}

fn machine_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// `(username, role)` from an entry's id token, without asking the gate.
pub fn claims_of(entry: &Value) -> Option<(String, String)> {
    let token = entry.pointer("/tokens/id_token")?.as_str()?;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    let name = ["preferred_username", "username", "sub"].iter().find_map(|k| claims.get(*k)?.as_str())?;
    let role = claims.get("role").and_then(Value::as_str).unwrap_or("user");
    Some((name.to_string(), role.to_string()))
}

/// `ragon login status`: the line and the exit status.
pub async fn status_line(primary: &Primary) -> (String, i32) {
    let entry = primary.entry();
    let key_only = entry.as_ref().is_some_and(|e| e.pointer("/tokens/access_token").and_then(Value::as_str).is_none_or(str::is_empty))
        && primary.bearer().is_some();
    if entry.is_none() && primary.bearer().is_none() {
        return ("Not signed in. Run ragon login.".to_string(), 1);
    }
    let live = primary.me().await;
    let offline = entry.as_ref().and_then(claims_of);
    let node = primary.node_name();
    let mut suffix = String::new();
    if primary.primary_name() != node {
        suffix = format!(" (accounts kept by {})", primary.primary_name());
    }
    let reachable = live.is_some();
    let who = live.or(offline);
    let line = match (who, key_only) {
        (Some((user, _)), true) => format!("Using API key as {user}"),
        (None, true) => "Using API key".to_string(),
        (Some((user, role)), false) => format!("Signed in as {user} ({role}) at {node}{suffix}"),
        (None, false) => format!("Signed in at {node}{suffix}"),
    };
    let line = if reachable { line } else { format!("{line} (authentication server unreachable)") };
    (line, 0)
}

/// `ragon login status --all`: one line per entry, the active one marked.
pub fn status_all_lines(codex_home: &Path) -> Vec<String> {
    let state = switcher::read_state(codex_home).unwrap_or_default();
    if state.entries.is_empty() {
        return vec!["Not signed in. Run ragon login.".to_string()];
    }
    state
        .entries
        .iter()
        .map(|(id, entry)| {
            let mark = if state.active.as_deref() == Some(id.as_str()) { "*" } else { " " };
            let who = claims_of(entry).map_or_else(
                || if entry.get("OPENAI_API_KEY").and_then(Value::as_str).is_some() { "API key".to_string() } else { "signed in".to_string() },
                |(user, role)| format!("{user} ({role})"),
            );
            format!("{mark} {id}: {who}")
        })
        .collect()
}

/// The auth line of `ragon doctor`, read from the entry alone.
pub fn doctor_line(codex_home: &Path, primary_name: Option<&str>) -> String {
    let state = switcher::read_state(codex_home).unwrap_or_default();
    let entry = state.active.as_ref().and_then(|id| state.entries.get(id));
    match (entry.and_then(claims_of), primary_name) {
        (Some((user, role)), Some(name)) => format!("signed in as {user} ({role}) at {name}"),
        (Some((user, role)), None) => format!("signed in as {user} ({role})"),
        (None, _) if entry.is_some() => "signed in with an API key".to_string(),
        _ => "not signed in (run ragon login)".to_string(),
    }
}

/// Reads one line; a terminal's password is read without echo.
///
/// # Errors
/// The input ends.
pub fn read_line(input: &mut dyn BufRead) -> anyhow::Result<String> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        bail!("input ended");
    }
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(unix)]
fn with_echo_off<T>(body: impl FnOnce() -> T) -> T {
    // SAFETY: termios calls on stdin with a zeroed struct the call fills; the old state is restored.
    unsafe {
        let mut old: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &raw mut old) != 0 {
            return body();
        }
        let mut quiet = old;
        quiet.c_lflag &= !libc::ECHO;
        libc::tcsetattr(0, libc::TCSANOW, &raw const quiet);
        let result = body();
        libc::tcsetattr(0, libc::TCSANOW, &raw const old);
        result
    }
}

#[cfg(not(unix))]
fn with_echo_off<T>(body: impl FnOnce() -> T) -> T {
    body()
}

fn prompt(out: &mut dyn Write, input: &mut dyn BufRead, label: &str, secret: bool, tty: bool) -> anyhow::Result<String> {
    write!(out, "{label}")?;
    out.flush()?;
    let line = if secret && tty { with_echo_off(|| read_line(input)) } else { read_line(input) };
    if secret && tty {
        writeln!(out)?;
    }
    line
}

/// Where the commands run: the gate, the home, the input and the output; `tty` says whether the
/// password may be asked for with a prompt.
pub struct Io<'a> {
    /// The input the forms and `--password-stdin` read.
    pub input: &'a mut dyn BufRead,
    /// Where lines go.
    pub out: &'a mut dyn Write,
    /// Whether stdin is a terminal.
    pub tty: bool,
}

async fn finish_sign_in(primary: &Primary, result: anyhow::Result<Result<(), String>>, io: &mut Io<'_>, created: bool) -> anyhow::Result<i32> {
    match result? {
        Ok(()) => {
            let (line, _) = status_line(primary).await;
            writeln!(io.out, "{line}")?;
            if created && primary.entry().as_ref().and_then(claims_of).is_some_and(|(_, role)| role == "admin") {
                writeln!(
                    io.out,
                    "You are the first user of {} and its admin. Others can create accounts at the login screen; /users manages them.",
                    primary.primary_name()
                )?;
            }
            Ok(0)
        }
        Err(text) => {
            writeln!(io.out, "{text}")?;
            Ok(1)
        }
    }
}

async fn account_form(primary: &Primary, io: &mut Io<'_>, create: bool) -> anyhow::Result<i32> {
    let username = prompt(io.out, io.input, "Username: ", false, io.tty)?;
    let password = prompt(io.out, io.input, "Password: ", true, io.tty)?;
    if create {
        let again = prompt(io.out, io.input, "Confirm password: ", true, io.tty)?;
        if again != password {
            writeln!(io.out, "The passwords do not match.")?;
            return Ok(1);
        }
        if password.is_empty() {
            let answer = prompt(io.out, io.input, "Continue without a password? [y/N] ", false, io.tty)?;
            if !answer.trim().eq_ignore_ascii_case("y") {
                return Ok(1);
            }
        }
        let result = primary.create_account(&username, &password).await;
        return finish_sign_in(primary, result, io, true).await;
    }
    let result = primary.sign_in(&username, &password).await;
    finish_sign_in(primary, result, io, false).await
}

/// Runs a launcher-handled `login` command.
///
/// # Errors
/// The accounts server cannot be reached or the home cannot be written.
pub async fn run_login(primary: &Primary, cmd: Cmd, io: &mut Io<'_>) -> anyhow::Result<Outcome> {
    let code = match cmd {
        Cmd::Screen => {
            writeln!(io.out, "Sign in to {}", primary.node_name())?;
            if primary.primary_name() != primary.node_name() {
                writeln!(io.out, "(accounts kept by {})", primary.primary_name())?;
            }
            writeln!(io.out, "  1. Sign in to {}", primary.primary_name())?;
            writeln!(io.out, "  2. Create an account on {}", primary.primary_name())?;
            writeln!(io.out, "  3. Sign in with a device code (ragon login --device-auth)")?;
            writeln!(io.out, "  4. Provide your own API key (ragon login --with-api-key)")?;
            let choice = prompt(io.out, io.input, "Choose 1-4: ", false, io.tty)?;
            match choice.trim() {
                "1" => account_form(primary, io, false).await?,
                "2" => account_form(primary, io, true).await?,
                "3" => return Ok(Outcome { code: 0, forward: Some(with_issuer(vec!["login".into(), "--device-auth".into()], &primary.gate)) }),
                "4" => return Ok(Outcome { code: 0, forward: Some(vec!["login".into(), "--with-api-key".into()]) }),
                _ => {
                    writeln!(io.out, "Choose 1, 2, 3 or 4.")?;
                    1
                }
            }
        }
        Cmd::CreateScreen => account_form(primary, io, true).await?,
        Cmd::Password { username, create, stdin } => {
            let password = if stdin { read_line(io.input)? } else { prompt(io.out, io.input, "Password: ", true, io.tty)? };
            let result = if create { primary.create_account(&username, &password).await } else { primary.sign_in(&username, &password).await };
            finish_sign_in(primary, result, io, create).await?
        }
        Cmd::Status { all } => {
            if all {
                for line in status_all_lines(&primary.home) {
                    writeln!(io.out, "{line}")?;
                }
                0
            } else {
                let (line, code) = status_line(primary).await;
                writeln!(io.out, "{line}")?;
                code
            }
        }
        Cmd::StatusJson => {
            writeln!(io.out, "{}", status_doc(primary, Lang::detect()).await)?;
            0
        }
        Cmd::Sessions { revoke } => sessions(primary, revoke.as_deref(), io).await?,
        Cmd::LogoutAll => logout_all(primary, io).await?,
        Cmd::Forward(args) => return Ok(Outcome { code: 0, forward: Some(with_issuer(args, &primary.gate)) }),
    };
    Ok(Outcome::done(code))
}

/// Lists the user's sessions and bindings, or revokes one.
async fn sessions(primary: &Primary, revoke: Option<&str>, io: &mut Io<'_>) -> anyhow::Result<i32> {
    if primary.bearer().is_none() {
        writeln!(io.out, "Not signed in. Run ragon login.")?;
        return Ok(1);
    }
    if let Some(id) = revoke {
        let response = primary.authed(reqwest::Method::DELETE, &format!("/ragon/v1/users/me/sessions/{id}")).await?;
        if response.status().is_success() {
            writeln!(io.out, "Revoked session {id}.")?;
            return Ok(0);
        }
        writeln!(io.out, "Could not revoke {id}: {} answered {}.", primary.primary_name(), response.status().as_u16())?;
        return Ok(1);
    }
    let response = primary.authed(reqwest::Method::GET, "/ragon/v1/users/me/sessions").await?;
    if !response.status().is_success() {
        writeln!(io.out, "Could not list sessions: {} answered {}.", primary.primary_name(), response.status().as_u16())?;
        return Ok(1);
    }
    let doc: Value = response.json().await.unwrap_or(Value::Null);
    let list = |key: &str| doc.get(key).and_then(Value::as_array).cloned().or_else(|| doc.as_array().cloned().filter(|_| key == "sessions")).unwrap_or_default();
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).unwrap_or("-").to_string();
    for item in list("sessions") {
        writeln!(
            io.out,
            "session {}  {}  {}  last used {}",
            field(&item, "id"),
            field(&item, "machine"),
            field(&item, "client"),
            field(&item, "last_used")
        )?;
    }
    for item in list("bindings") {
        writeln!(io.out, "binding {}  {}  {}", field(&item, "id"), field(&item, "kind"), field(&item, "last_used"))?;
    }
    Ok(0)
}

/// `ragon logout --all`: revokes every session of the user, then the normal logout is Codex's.
async fn logout_all(primary: &Primary, io: &mut Io<'_>) -> anyhow::Result<i32> {
    if primary.bearer().is_some() {
        let response = primary.authed(reqwest::Method::DELETE, "/ragon/v1/users/me/sessions").await?;
        if response.status().is_success() {
            writeln!(io.out, "Revoked every session of this account on every machine; API keys are kept.")?;
        } else if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            writeln!(io.out, "Could not revoke the sessions: {} answered {}.", primary.primary_name(), response.status().as_u16())?;
            return Ok(1);
        }
    }
    Ok(0)
}

/// Runs a whole `login`/`logout`/`sessions` command: parse, connect, run.
///
/// `which` is `"login"`, `"logout"` or `"sessions"`; `args` follow it.
///
/// # Errors
/// A usage message is printed and answered as status 2; connection and file errors come back.
pub async fn run(which: &str, args: &[String], gate: &str, codex_home: &Path, io: &mut Io<'_>) -> anyhow::Result<Outcome> {
    let cmd = match which {
        "login" => parse_login(args),
        "logout" => Ok(parse_logout(args)),
        _ => parse_sessions(args),
    };
    let cmd = match cmd {
        Ok(cmd) => cmd,
        Err(usage) => {
            writeln!(io.out, "{usage}")?;
            return Ok(Outcome::done(2));
        }
    };
    if let Cmd::Status { all: true } = cmd {
        for line in status_all_lines(codex_home) {
            writeln!(io.out, "{line}")?;
        }
        return Ok(Outcome::done(0));
    }
    if let Cmd::StatusJson = cmd {
        writeln!(io.out, "{}", status_json(gate, codex_home, Lang::detect()).await)?;
        return Ok(Outcome::done(0));
    }
    let primary = connect(gate, codex_home).await?;
    if let Cmd::LogoutAll = cmd {
        let code = logout_all(&primary, io).await?;
        let forward = (code == 0).then(|| vec!["logout".to_string()]);
        return Ok(Outcome { code, forward });
    }
    run_login(&primary, cmd, io).await
}

/// `ragon pair <chat kind>` on Windows: the messenger bridges are not built there yet.
///
/// # Errors
/// Never; the signature matches the Unix build.
#[cfg(not(unix))]
pub async fn run_pair(_args: &[String], _gate: &str, _codex_home: &Path, io: &mut Io<'_>) -> anyhow::Result<Outcome> {
    writeln!(io.out, "`ragon pair` is not available on Windows yet: the messenger bridges run on Linux and macOS only.")?;
    Ok(Outcome::done(2))
}

/// What `ragon pair <kind>` says against a node without accounts: the 0.9.7 way to pair.
pub fn no_accounts_pair_line(kind: &str) -> String {
    match kind {
        "matrix" => "This node has no accounts, so Matrix needs no pairing: the accounts `ragon-admin matrix add-user` makes can talk to the bot.".to_string(),
        _ => format!("This node has no accounts, so {kind} pairs as before: run `ragon chat {kind} pair` on the machine the bridge runs on and send the bot the /pair line it prints."),
    }
}

/// `ragon pair <chat kind>`: asks the primary for a pairing code for this signed-in user and
/// prints it, and leaves the chat bridge what it needs to redeem it: the gate, the node's name and
/// the primary's CA, in the chat folder (`<codex_home>/chat`).
///
/// # Errors
/// The accounts server cannot be reached or the chat folder cannot be written.
#[cfg(unix)]
pub async fn run_pair(args: &[String], gate: &str, codex_home: &Path, io: &mut Io<'_>) -> anyhow::Result<Outcome> {
    let kinds = ragon_chat::bindings::KINDS;
    let kind = match args {
        [kind] if kinds.contains(&kind.as_str()) => kind.as_str(),
        _ => {
            writeln!(io.out, "Usage: ragon pair <{}>", kinds.join("|"))?;
            return Ok(Outcome::done(2));
        }
    };
    let primary = connect(gate, codex_home).await?;
    if primary.bearer().is_none() {
        // A node without accounts (0.9.8's default) pairs phones as 0.9.7 did, on the bridge's machine.
        if super::gate_accounts(gate).await == Some(false) {
            writeln!(io.out, "{}", no_accounts_pair_line(kind))?;
            return Ok(Outcome::done(1));
        }
        writeln!(io.out, "Not signed in. Run ragon login, then ragon pair {kind}.")?;
        return Ok(Outcome::done(1));
    }
    let response = primary.authed_json(reqwest::Method::POST, "/ragon/v1/bindings", Some(json!({ "chat_kind": kind }))).await?;
    let status = response.status();
    let doc: Value = response.json().await.unwrap_or(Value::Null);
    let Some(code) = doc.get("code").and_then(Value::as_str).filter(|_| status.is_success()) else {
        writeln!(io.out, "Could not get a pairing code: {} answered {}.", primary.primary_name(), status.as_u16())?;
        return Ok(Outcome::done(1));
    };
    let minutes = doc.get("expires_in").and_then(Value::as_u64).map_or(60, |secs| secs.div_ceil(60));
    ragon_chat::bindings::write_primary(&codex_home.join("chat"), &primary.gate, &primary.node_name(), primary.ca_pem.as_deref())?;
    writeln!(io.out, "Pairing code for {kind}: {code}")?;
    writeln!(io.out, "Send it to the {kind} bot as a message (or /pair {code}); it works once, for {minutes} minutes.")?;
    let home = codex_home.parent().unwrap_or(codex_home);
    if ragon_chat::bindings::service_key(home).is_none() {
        writeln!(io.out, "Note: this machine has no service key (RAGON_API_KEY or ~/.config/dreamference/service-key); the bridge cannot redeem codes without it.")?;
    }
    Ok(Outcome::done(0))
}
