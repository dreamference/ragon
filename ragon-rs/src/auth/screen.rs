//! The sign-in step (specs/DREAMFERENCE_AUTH.md §8): the screen Codex's onboarding shows while its
//! sign-in widget is at the picker, drawn here and reached through patch `0038`.
//!
//! Codex's layout and keys stay: four numbered lines, arrows or the number, Enter. Lines 1 and 2
//! open this module's forms (sign in, create an account on the primary); lines 3 and 4 hand back to
//! Codex's device-code flow and key entry. A password grant goes to the node's gate, which forwards
//! it to the primary; its tokens are written with `codex_login::save_auth` and every registered
//! auth manager reloads them, after which Codex's onboarding continues with its next step.
//!
//! [`Screen`] is pure (keys in, a [`Step`] out); the hooks the patch calls ([`key`], [`paste`],
//! [`typing`], [`render`], [`register`]) hold one screen in a static, since the launcher, the TUI
//! and its embedded app-server share one process.

use std::path::{Path, PathBuf};

use super::strings::{self, Key};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use base64::Engine as _;
use codex_login::{AuthCredentialsStoreMode, AuthDotJson, AuthKeyringBackendKind, AuthManager};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget, Wrap};

/// What the hooks return to Codex's widget: the key is not ours.
pub const NOT_MINE: u8 = 0;
/// The key was handled here.
pub const HANDLED: u8 = 1;
/// Line 3 was chosen: Codex's device-code flow.
pub const DEVICE_CODE: u8 = 2;
/// Line 4 was chosen: Codex's API-key entry.
pub const API_KEY: u8 = 3;
/// The user is signed in and has read the result: the step is complete.
pub const DONE: u8 = 4;

/// The client id a password grant names; the gate accepts any.
pub const CLIENT_ID: &str = "ragon";

const WRONG: &str = "Wrong username or password.";
const TAKEN: &str = "That username is taken.";
const BAD_NAME: &str = "Usernames are 1-32 letters, digits, '.', '_' or '-'.";

/// The node the launcher resolved, as [`super::start`] found it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Node {
    /// The gate's base URL.
    pub gate: String,
    /// The node's display name.
    pub node_name: String,
    /// The primary's display name (the node's own on a primary).
    pub primary_name: String,
    /// The pinned CA of an HTTPS gate.
    pub ca: Option<PathBuf>,
    /// Codex's home, where the tokens are written.
    pub codex_home: PathBuf,
    /// The home was used before accounts existed: the upgrade note is shown (§8.1).
    pub upgrade: bool,
    /// The node has no account yet and the launcher runs on it: the screen opens at the create
    /// form for its first account (§8.6).
    pub first: bool,
}

/// What a grant asks of the primary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grant {
    /// The username, as typed less surrounding blanks.
    pub username: String,
    /// The password exactly as typed: never trimmed, possibly empty.
    pub password: String,
    /// Create the account first.
    pub create: bool,
    /// The new password of a must-change sign-in.
    pub new_password: Option<String>,
}

/// Which form is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// Username, Password.
    SignIn,
    /// Username, Password, Confirm password.
    Create,
    /// New, Confirm, for the sign-in that was refused until the password changes.
    Change(Grant),
}

/// A form: its kind, its fields and the focused one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    /// Which form.
    pub kind: Kind,
    /// The typed values, in the order the labels list them.
    pub fields: Vec<String>,
    /// The focused field.
    pub focus: usize,
}

/// What the screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    /// The upgrade note (§8.1).
    Note,
    /// The four lines.
    Pick,
    /// A form being filled in.
    Form(Form),
    /// "Continue without a password?" for the create form.
    AskEmpty(Form),
    /// The grant is on its way.
    Working(Form),
    /// The accounts server could not be reached; "r" sends again.
    Offline(Form),
    /// Signed in; Enter continues.
    Welcome {
        /// Who signed in.
        user: String,
        /// The account just created is the primary's first, and so its admin.
        admin: bool,
    },
    /// Handed back to Codex.
    Done(String),
}

/// What a key asks of the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Not this screen's key: Codex handles it.
    Pass,
    /// Handled; draw again.
    Redraw,
    /// Codex's device-code flow.
    DeviceCode,
    /// Codex's API-key entry.
    ApiKey,
    /// The step is complete.
    Done,
    /// Send this grant; the view is already [`View::Working`].
    Send(Grant),
}

/// How a grant ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Signed in; `admin` when the account was just created as the primary's admin.
    Signed {
        /// The account's role is admin and it was created by this grant.
        admin: bool,
    },
    /// Wrong username or password.
    Wrong,
    /// The username exists.
    Taken,
    /// The password must change first.
    MustChange,
    /// The accounts server is down or the gate unreachable.
    Unreachable,
    /// Anything else, with the text to show.
    Failed(String),
}

/// The tokens of a successful grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// The id token (a JWT).
    pub id_token: String,
    /// The access token.
    pub access_token: String,
    /// The refresh token.
    pub refresh_token: String,
}

/// The sign-in screen's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// The node's display name.
    pub node_name: String,
    /// The primary's display name.
    pub primary_name: String,
    /// The current view.
    pub view: View,
    /// The highlighted line of the picker, 0-3.
    pub highlight: usize,
    /// The error under the form or the picker.
    pub error: Option<String>,
    /// The node was in use before accounts existed.
    pub upgrade: bool,
    /// The node has no account yet: the create form is for its first (§8.6).
    pub first: bool,
}

/// `[A-Za-z0-9._-]{1,32}`, the store's rule.
pub fn valid_username(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The body of a password grant (accounts_oauth.py `password_grant`).
pub fn grant_body(grant: &Grant, machine: &str) -> serde_json::Value {
    let mut body = serde_json::json!({
        "grant_type": "password",
        "client_id": CLIENT_ID,
        "username": grant.username,
        "password": grant.password,
        "machine": machine,
    });
    if grant.create {
        body["create"] = serde_json::Value::Bool(true);
    }
    if let Some(new) = &grant.new_password {
        body["new_password"] = serde_json::Value::String(new.clone());
    }
    body
}

/// Reads the token endpoint's answer: the tokens, or how the grant failed.
pub fn classify(status: u16, body: &str) -> Result<Tokens, Outcome> {
    let json: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    let text = |key: &str| json.get(key).and_then(serde_json::Value::as_str).map(str::to_string);
    if (200..300).contains(&status) {
        return match (text("id_token"), text("access_token"), text("refresh_token")) {
            (Some(id_token), Some(access_token), Some(refresh_token)) => Ok(Tokens { id_token, access_token, refresh_token }),
            _ => Err(Outcome::Failed(strings::t(Key::NoTokens).to_string())),
        };
    }
    if matches!(status, 502..=504) {
        return Err(Outcome::Unreachable);
    }
    let description = text("error_description").unwrap_or_default();
    Err(match text("error").as_deref() {
        Some("invalid_grant") => Outcome::Wrong,
        Some("password_change_required") => Outcome::MustChange,
        Some("accounts_unavailable") => Outcome::Unreachable,
        Some("invalid_request") if description.ends_with("already exists.") => Outcome::Taken,
        _ if !description.is_empty() => Outcome::Failed(description),
        _ => Outcome::Failed(strings::fmt(Key::ServerAnswered, &[&status.to_string()])),
    })
}

/// The claims of a JWT, unverified (the gate signed it; this only reads it for display).
pub fn claims(jwt: &str) -> serde_json::Value {
    let payload = jwt.split('.').nth(1).unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The account's role, from the id token's auth claim.
pub fn role(id_token: &str) -> String {
    claims(id_token)["https://api.openai.com/auth"]["chatgpt_plan_type"].as_str().unwrap_or_default().to_string()
}

/// `auth.json`'s entry for the tokens, as Codex's own browser login writes it.
pub fn auth_entry(tokens: &Tokens) -> anyhow::Result<AuthDotJson> {
    Ok(serde_json::from_value(super::token_entry(&tokens.id_token, &tokens.access_token, &tokens.refresh_token))?)
}

/// Whether a password may go to this gate: HTTPS, or plain HTTP to this machine.
pub fn may_send(gate: &str) -> bool {
    if gate.starts_with("https://") {
        return true;
    }
    gate.starts_with("http://") && matches!(super::node_label(gate).as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// A home in use before accounts: it has sessions or history.
pub fn used_before(codex_home: &Path) -> bool {
    codex_home.join("history.jsonl").exists() || std::fs::read_dir(codex_home.join("sessions")).is_ok_and(|mut dir| dir.next().is_some())
}

impl Form {
    fn new(kind: Kind) -> Self {
        let count = if matches!(kind, Kind::Create) { 3 } else { 2 };
        Self { kind, fields: vec![String::new(); count], focus: 0 }
    }

    /// The field labels.
    pub fn labels(&self) -> Vec<&'static str> {
        match self.kind {
            Kind::SignIn => vec![strings::t(Key::FieldUsername), strings::t(Key::FieldPassword)],
            Kind::Create => vec![strings::t(Key::FieldUsername), strings::t(Key::FieldPassword), strings::t(Key::FieldConfirmPassword)],
            Kind::Change(_) => vec![strings::t(Key::FieldNewPassword), strings::t(Key::FieldConfirm)],
        }
    }

    fn masked(&self, field: usize) -> bool {
        !matches!(self.kind, Kind::SignIn | Kind::Create) || field > 0
    }

    /// Who the form signs in.
    pub fn user(&self) -> String {
        match &self.kind {
            Kind::Change(grant) => grant.username.clone(),
            _ => self.fields[0].trim().to_string(),
        }
    }

    fn clear_passwords(&mut self) {
        let change = matches!(self.kind, Kind::Change(_));
        for (index, field) in self.fields.iter_mut().enumerate() {
            if change || index > 0 {
                field.clear();
            }
        }
        self.focus = if change { 0 } else { 1 };
    }
}

impl Screen {
    /// A screen for the node: at the create form for a node's first account, at the note on an
    /// upgrade, at the picker otherwise.
    pub fn new(node: &Node) -> Self {
        Self {
            node_name: node.node_name.clone(),
            primary_name: node.primary_name.clone(),
            view: if node.first {
                View::Form(Form::new(Kind::Create))
            } else if node.upgrade {
                View::Note
            } else {
                View::Pick
            },
            highlight: if node.first { 1 } else { 0 },
            error: None,
            upgrade: node.upgrade,
            first: node.first,
        }
    }

    /// The text being typed is a form's: printable keys are text, not Codex's quit or select keys.
    pub fn typing(&self) -> bool {
        matches!(self.view, View::Form(_))
    }

    /// Handles one key press.
    pub fn key(&mut self, key: KeyEvent) -> Step {
        if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return Step::Pass;
        }
        let code = key.code;
        match std::mem::replace(&mut self.view, View::Pick) {
            View::Note => {
                self.view = if code == KeyCode::Enter { View::Pick } else { View::Note };
                Step::Redraw
            }
            View::Pick => self.pick_key(code),
            View::Form(form) => self.form_key(form, code),
            View::AskEmpty(form) => match code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter => self.send(form),
                KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                    self.view = View::Form(form);
                    Step::Redraw
                }
                _ => {
                    self.view = View::AskEmpty(form);
                    Step::Redraw
                }
            },
            View::Working(form) => {
                self.view = View::Working(form);
                Step::Redraw
            }
            View::Offline(form) => match code {
                KeyCode::Char('r' | 'R') | KeyCode::Enter => self.send(form),
                KeyCode::Esc => {
                    self.error = None;
                    self.view = View::Form(form);
                    Step::Redraw
                }
                _ => {
                    self.view = View::Offline(form);
                    Step::Redraw
                }
            },
            View::Welcome { user, admin } => {
                if code == KeyCode::Enter {
                    self.view = View::Done(user);
                    Step::Done
                } else {
                    self.view = View::Welcome { user, admin };
                    Step::Redraw
                }
            }
            View::Done(user) => {
                self.view = View::Done(user);
                Step::Pass
            }
        }
    }

    fn pick_key(&mut self, code: KeyCode) -> Step {
        let chosen = match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.highlight = self.highlight.saturating_sub(1);
                None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.highlight = (self.highlight + 1).min(3);
                None
            }
            KeyCode::Char('1' | 'y') => Some(0),
            KeyCode::Char('2' | 'n') => Some(1),
            KeyCode::Char('3') => Some(2),
            KeyCode::Char('4') => Some(3),
            KeyCode::Enter => Some(self.highlight),
            _ => None,
        };
        let Some(line) = chosen else { return Step::Redraw };
        self.highlight = line;
        self.error = None;
        match line {
            0 => self.view = View::Form(Form::new(Kind::SignIn)),
            1 => self.view = View::Form(Form::new(Kind::Create)),
            2 => return Step::DeviceCode,
            _ => return Step::ApiKey,
        }
        Step::Redraw
    }

    fn form_key(&mut self, mut form: Form, code: KeyCode) -> Step {
        let last = form.fields.len() - 1;
        match code {
            KeyCode::Esc => {
                self.error = None;
                return Step::Redraw;
            }
            KeyCode::Char(c) => form.fields[form.focus].push(c),
            KeyCode::Backspace => {
                form.fields[form.focus].pop();
            }
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1).min(last),
            KeyCode::BackTab | KeyCode::Up => form.focus = form.focus.saturating_sub(1),
            KeyCode::Enter if form.focus < last => form.focus += 1,
            KeyCode::Enter => return self.submit(form),
            _ => {}
        }
        self.view = View::Form(form);
        Step::Redraw
    }

    fn submit(&mut self, mut form: Form) -> Step {
        let user = form.user();
        let error = match &form.kind {
            Kind::SignIn | Kind::Create if !valid_username(&user) => Some((BAD_NAME, 0)),
            Kind::Create if form.fields[1] != form.fields[2] => Some((strings::t(Key::PasswordMismatch), 1)),
            Kind::Change(_) if form.fields[0] != form.fields[1] => Some((strings::t(Key::PasswordMismatch), 0)),
            _ => None,
        };
        if let Some((text, focus)) = error {
            self.error = Some(text.to_string());
            form.focus = focus;
            self.view = View::Form(form);
            return Step::Redraw;
        }
        if matches!(form.kind, Kind::Create) && form.fields[1].is_empty() {
            self.view = View::AskEmpty(form);
            return Step::Redraw;
        }
        self.send(form)
    }

    fn send(&mut self, form: Form) -> Step {
        let grant = match &form.kind {
            Kind::SignIn | Kind::Create => Grant {
                username: form.user(),
                password: form.fields[1].clone(),
                create: matches!(form.kind, Kind::Create),
                new_password: None,
            },
            Kind::Change(grant) => Grant { new_password: Some(form.fields[0].clone()), ..grant.clone() },
        };
        self.error = None;
        self.view = View::Working(form);
        Step::Send(grant)
    }

    /// Takes a pasted text into the focused field.
    pub fn paste(&mut self, text: &str) -> bool {
        let View::Form(form) = &mut self.view else { return false };
        form.fields[form.focus].push_str(text.trim_end_matches(['\r', '\n']));
        true
    }

    /// Applies a grant's outcome to the form it came from.
    pub fn apply(&mut self, grant: &Grant, outcome: Outcome) {
        let mut form = match std::mem::replace(&mut self.view, View::Pick) {
            View::Working(form) | View::Offline(form) | View::Form(form) | View::AskEmpty(form) => form,
            other => {
                self.view = other;
                return;
            }
        };
        self.error = None;
        self.view = match outcome {
            Outcome::Signed { admin } => View::Welcome { user: grant.username.clone(), admin },
            Outcome::MustChange => View::Form(Form::new(Kind::Change(Grant { new_password: None, ..grant.clone() }))),
            Outcome::Unreachable => {
                self.error = Some(format!(
                    "{} cannot reach its accounts server {}. Try again or ask the node's admin.",
                    self.node_name, self.primary_name
                ));
                View::Offline(form)
            }
            Outcome::Wrong | Outcome::Taken | Outcome::Failed(_) => {
                self.error = Some(match outcome {
                    Outcome::Wrong => WRONG.to_string(),
                    Outcome::Taken => TAKEN.to_string(),
                    Outcome::Failed(text) => text,
                    _ => unreachable!(),
                });
                match &form.kind {
                    Kind::Create if self.error.as_deref() == Some(TAKEN) => form.focus = 0,
                    _ => form.clear_passwords(),
                }
                View::Form(form)
            }
        };
    }

    /// The lines the screen draws.
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut title = format!("  Sign in to {}", self.node_name);
        if self.primary_name != self.node_name {
            title.push_str(&format!(" (accounts kept by {})", self.primary_name));
        }
        match &self.view {
            View::Note => {
                lines.push(Line::from(format!(
                    "  Ragon now has accounts. Sign in or create one on {}; your repositories and settings are unchanged. {}",
                    self.node_name,
                    strings::t(Key::PhonesPairAgain)
                )));
                lines.push("".into());
                lines.push(press("to continue"));
            }
            View::Pick => {
                lines.push(title.into());
                lines.push("".into());
                let items = [
                    (strings::fmt(Key::SignInTitle, &[&self.primary_name]), strings::t(Key::SignInHint).to_string()),
                    (strings::fmt(Key::CreateTitle, &[&self.primary_name]), strings::t(Key::CreateHint).to_string()),
                    (strings::t(Key::DeviceTitle).to_string(), strings::t(Key::DeviceHint).to_string()),
                    (strings::t(Key::ApiKeyTitle).to_string(), strings::t(Key::ApiKeyHint).to_string()),
                ];
                for (index, (text, description)) in items.into_iter().enumerate() {
                    if index == self.highlight {
                        lines.push(Line::from(vec![format!("> {}. ", index + 1).cyan().dim(), text.cyan()]));
                        lines.push(Line::from(format!("     {description}")).fg(Color::Cyan).add_modifier(Modifier::DIM));
                    } else {
                        lines.push(format!("  {}. {text}", index + 1).into());
                        lines.push(Line::from(format!("     {description}")).style(Style::default().add_modifier(Modifier::DIM)));
                    }
                    lines.push("".into());
                }
                lines.push(press("to continue"));
            }
            View::Form(form) | View::AskEmpty(form) | View::Working(form) | View::Offline(form) => {
                if self.first && matches!(form.kind, Kind::Create) {
                    lines.push(format!("  {} {}", strings::fmt(Key::FirstAccount, &[&self.primary_name]), strings::t(Key::PhonesPairAgain)).into());
                    lines.push("".into());
                }
                lines.push(match &form.kind {
                    Kind::SignIn => title.into(),
                    Kind::Create => format!("  Create an account on {}", self.primary_name).into(),
                    Kind::Change(grant) => format!("  Set a new password for {}", grant.username).into(),
                });
                lines.push("".into());
                for (index, label) in form.labels().iter().enumerate() {
                    let value = &form.fields[index];
                    let shown = if form.masked(index) { "•".repeat(value.chars().count()) } else { value.clone() };
                    let focused = index == form.focus && matches!(self.view, View::Form(_));
                    let caret = if focused { "> " } else { "  " };
                    let line = Line::from(vec![format!("{caret}{label}: ").into(), shown.into(), if focused { "█".dim() } else { "".into() }]);
                    lines.push(if focused { line.cyan() } else { line });
                }
                lines.push("".into());
                match self.view {
                    View::AskEmpty(_) => lines.push("  Continue without a password? (y/n)".into()),
                    View::Working(_) => lines.push(format!("  {}", strings::t(Key::SigningIn)).dim().into()),
                    View::Offline(_) => lines.push(Line::from(vec!["  Press ".dim(), "r".into(), " to try again, ".dim(), "Esc".into(), " to go back".dim()])),
                    _ => lines.push(Line::from(vec![
                        "  Press ".dim(),
                        "Enter".into(),
                        " to continue, ".dim(),
                        "Tab".into(),
                        " for the next field, ".dim(),
                        "Esc".into(),
                        " to go back".dim(),
                    ])),
                }
            }
            View::Welcome { user, admin } => {
                lines.push(format!("✓ {}", strings::fmt(Key::SignedIn, &[&self.primary_name, user])).green().into());
                lines.push("".into());
                if *admin {
                    let mut text = format!(
                        "  You are the first user of {} and its admin. Others can create accounts at the login screen; /users manages them.",
                        self.primary_name
                    );
                    if self.upgrade || self.first {
                        text.push(' ');
                        text.push_str(strings::t(Key::PhonesPairAgain));
                    }
                    lines.push(text.into());
                    lines.push("".into());
                }
                lines.push(press("to continue"));
            }
            View::Done(user) => lines.push(format!("✓ {}", strings::fmt(Key::SignedIn, &[&self.primary_name, user])).green().into()),
        }
        if let Some(error) = &self.error {
            lines.push("".into());
            lines.push(format!("  {error}").red().into());
        }
        lines
    }
}

fn press(what: &str) -> Line<'static> {
    Line::from(vec!["  Press ".dim(), "Enter".into(), format!(" {what}").dim()])
}

// ----- the hooks patch 0038 calls

static NODE: OnceLock<Node> = OnceLock::new();
static SCREEN: Mutex<Option<Screen>> = Mutex::new(None);
static MANAGERS: Mutex<Vec<Weak<AuthManager>>> = Mutex::new(Vec::new());

/// Records the node [`super::start`] resolved; the screen draws nothing without one.
pub fn set_node(node: Node) {
    let _ = NODE.set(node);
}

/// Registers an auth manager to reload after a sign-in (the embedded app-server's).
pub fn register(manager: &Arc<AuthManager>) {
    if let Ok(mut managers) = MANAGERS.lock() {
        managers.retain(|weak| weak.strong_count() > 0);
        managers.push(Arc::downgrade(manager));
    }
}

fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> Option<R> {
    let node = NODE.get()?;
    let mut screen = SCREEN.lock().ok()?;
    Some(f(screen.get_or_insert_with(|| Screen::new(node))))
}

/// A key at Codex's picker (`picking`); returns one of the codes above.
pub fn key(picking: bool, key: KeyEvent, frame: impl Fn() + Send + 'static) -> u8 {
    if !picking {
        return NOT_MINE;
    }
    match with(|screen| screen.key(key)) {
        None | Some(Step::Pass) => NOT_MINE,
        Some(Step::Redraw) => HANDLED,
        Some(Step::DeviceCode) => DEVICE_CODE,
        Some(Step::ApiKey) => API_KEY,
        Some(Step::Done) => DONE,
        Some(Step::Send(grant)) => {
            let node = NODE.get().cloned().unwrap_or_default();
            std::thread::spawn(move || {
                let outcome = sign_in(&node, &grant);
                with(|screen| screen.apply(&grant, outcome));
                frame();
            });
            HANDLED
        }
    }
}

/// A paste at Codex's picker; true when a form took it.
pub fn paste(text: &str) -> bool {
    with(|screen| screen.paste(text)).unwrap_or(false)
}

/// A form has the focus.
pub fn typing() -> bool {
    with(|screen| screen.typing()).unwrap_or(false)
}

/// Draws the screen at Codex's picker (`picking`) or after a sign-in (`signed`); false leaves
/// the area to Codex.
pub fn render(picking: bool, signed: bool, area: Rect, buf: &mut Buffer) -> bool {
    if !(picking || signed) {
        return false;
    }
    let Some(lines) = with(|screen| {
        if signed && !matches!(screen.view, View::Done(_)) {
            vec![format!("✓ {}", strings::fmt(Key::SignedInOnly, &[&screen.primary_name])).green().into()]
        } else {
            screen.lines()
        }
    }) else {
        return false;
    };
    Paragraph::new(lines).wrap(Wrap { trim: false }).render(area, buf);
    true
}

fn machine() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .map(|name| name.trim().to_string())
        .unwrap_or_default()
}

fn client(node: &Node) -> anyhow::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(20));
    if let Some(ca) = &node.ca {
        // The password goes only to the gate whose CA was pinned at first contact.
        let pem = std::fs::read(ca)?;
        builder = builder.tls_built_in_root_certs(false).add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
    }
    Ok(builder.build()?)
}

/// Sends the grant and, on success, writes the tokens and reloads every registered auth manager.
pub fn sign_in(node: &Node, grant: &Grant) -> Outcome {
    if !may_send(&node.gate) {
        return Outcome::Failed(strings::fmt(Key::PlainHttp, &[&node.node_name]));
    }
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
        return Outcome::Failed(strings::t(Key::CouldNotStart).to_string());
    };
    runtime.block_on(async {
        let Ok(client) = client(node) else { return Outcome::Failed(strings::t(Key::CertUnreadable).to_string()) };
        let url = format!("{}/oauth/token", node.gate.trim_end_matches('/'));
        let Ok(response) = client.post(url).json(&grant_body(grant, &machine())).send().await else { return Outcome::Unreachable };
        let status = response.status().as_u16();
        let body = response.text().await.unwrap_or_default();
        let tokens = match classify(status, &body) {
            Ok(tokens) => tokens,
            Err(outcome) => return outcome,
        };
        if let Err(error) = save(&node.codex_home, &tokens) {
            return Outcome::Failed(strings::fmt(Key::CouldNotSave, &[&error.to_string()]));
        }
        reload().await;
        Outcome::Signed { admin: grant.create && role(&tokens.id_token) == "admin" }
    })
}

/// Writes the tokens where Codex keeps them (the file store, the launcher's default).
pub fn save(codex_home: &Path, tokens: &Tokens) -> anyhow::Result<()> {
    let entry = auth_entry(tokens)?;
    codex_login::save_auth(codex_home, &entry, AuthCredentialsStoreMode::File, AuthKeyringBackendKind::Direct)?;
    Ok(())
}

/// Asks every registered auth manager to read the stored sign-in again.
pub async fn reload() {
    let managers: Vec<Arc<AuthManager>> =
        MANAGERS.lock().map(|managers| managers.iter().filter_map(Weak::upgrade).collect()).unwrap_or_default();
    for manager in managers {
        manager.reload().await;
    }
}
