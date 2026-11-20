//! Chat identities bound to node users (specs/DREAMFERENCE_AUTH.md §14.1).
//!
//! A chat identity (a Telegram user id, a Discord user id, a Matrix user id) is bound to one
//! **existing** node user. `ragon chat pair <kind>` on a signed-in machine asks the primary for a
//! pairing code; the user sends it to the bot; the bridge redeems it at the primary with the
//! node's service key and keeps the binding key the primary minted, in `bindings.json` (0600).
//! The bridge then reaches the model with that key as a Bearer, as that user, never with the
//! device cookie.
//!
//! The files, in the chat folder (0700):
//!
//! - `bindings.json`: `{"bindings": {"<kind>:<identity>": {"username", "key", "chat"}}}`;
//! - `bindings.v2`: the marker that the 0.9.8 pairings were dropped (see [`drop_old_pairings`]);
//! - `primary.json`: `{"gate", "node"}`, written by `ragon chat pair`: where the primary is;
//! - `primary-ca.pem`: the primary's CA, imported by the pairing step;
//! - `pairing-hints.json`: when each chat was last told how to pair (once a day).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

/// The chat kinds a pairing code can be for (the primary checks the same list).
pub const KINDS: [&str; 3] = ["telegram", "discord", "matrix"];

/// How long an unbound chat is left alone after it was told how to pair.
pub const HINT_EVERY_SECS: u64 = 24 * 60 * 60;

/// One bound chat identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The node user's name.
    pub username: String,
    /// The key the primary minted for this binding; revoked when the binding is removed.
    pub key: String,
    /// The hub's chat key (`telegram:<chat id>`, …) the identity talks in.
    pub chat: String,
}

/// The bindings of one bridge, as `bindings.json` holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bindings {
    map: BTreeMap<String, Binding>,
}

fn identity_key(kind: &str, identity: &str) -> String {
    format!("{kind}:{identity}")
}

fn path(dir: &Path) -> PathBuf {
    dir.join("bindings.json")
}

/// Writes `text` to `path` readable by the owner only.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    file.write_all(text.as_bytes())?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

impl Bindings {
    /// Reads `bindings.json`; empty when there is none.
    pub fn load(dir: &Path) -> Bindings {
        let value: Value = std::fs::read_to_string(path(dir)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
        let mut map = BTreeMap::new();
        if let Some(items) = value.get("bindings").and_then(Value::as_object) {
            for (id, item) in items {
                let field = |name: &str| item.get(name).and_then(Value::as_str).map(str::to_string);
                if let (Some(username), Some(key)) = (field("username"), field("key")) {
                    map.insert(id.clone(), Binding { username, key, chat: field("chat").unwrap_or_default() });
                }
            }
        }
        Bindings { map }
    }

    /// Saves the bindings, mode 0600.
    ///
    /// # Errors
    /// The file cannot be written.
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let items: serde_json::Map<String, Value> = self
            .map
            .iter()
            .map(|(id, b)| (id.clone(), json!({ "username": b.username, "key": b.key, "chat": b.chat })))
            .collect();
        write_private(&path(dir), &serde_json::to_string_pretty(&json!({ "bindings": items }))?)
    }

    /// Binds (or moves) an identity: one identity maps to exactly one user.
    pub fn bind(&mut self, kind: &str, identity: &str, binding: Binding) {
        self.map.insert(identity_key(kind, identity), binding);
    }

    /// Removes an identity's binding.
    pub fn remove(&mut self, kind: &str, identity: &str) -> bool {
        self.map.remove(&identity_key(kind, identity)).is_some()
    }

    /// The binding of an identity.
    pub fn get(&self, kind: &str, identity: &str) -> Option<&Binding> {
        self.map.get(&identity_key(kind, identity))
    }

    /// The binding whose chat key is `chat` (a topic suffix after the chat id is ignored by the
    /// caller, which passes the base).
    pub fn for_chat(&self, chat: &str) -> Option<&Binding> {
        self.map.values().find(|b| b.chat == chat)
    }

    /// The identities bound for one chat kind.
    pub fn identities(&self, kind: &str) -> Vec<String> {
        let prefix = format!("{kind}:");
        self.map.keys().filter_map(|k| k.strip_prefix(&prefix).map(str::to_string)).collect()
    }

    /// How many identities are bound.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether nothing is bound.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// The upgrade to 0.9.9: every pairing of the earlier releases is dropped, once (§14.1). The paired
/// user ids of `telegram.json` and `discord.json` go and the allow-list marker is written; every
/// phone pairs again. Runs only on a node whose gate advertises accounts ([`settle_pairings`]).
/// Returns whether anything was dropped (false on every later start).
pub fn drop_old_pairings(dir: &Path) -> bool {
    let marker = dir.join("bindings.v2");
    if marker.exists() {
        return false;
    }
    for (file, field) in [("telegram.json", "users"), ("discord.json", "users")] {
        let file = dir.join(file);
        let Some(mut value) = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        if let Some(object) = value.as_object_mut()
            && object.contains_key(field)
        {
            object.insert(field.to_string(), if field == "users" && file.ends_with("discord.json") { json!({}) } else { json!([]) });
            let _ = write_private(&file, &serde_json::to_string_pretty(&value).unwrap_or_default());
        }
    }
    let _ = std::fs::remove_file(path(dir));
    for sub in ["pairing", "discord/pairing"] {
        let _ = std::fs::remove_dir_all(dir.join(sub));
    }
    let _ = std::fs::write(&marker, ACCOUNTS_MARKER);
    true
}

/// What the one-time drop writes to `bindings.v2` when it runs on a node with accounts.
pub const ACCOUNTS_MARKER: &str = "accounts\n";
/// What 0.9.8 wrote there: it dropped the pairings whether or not the node had accounts.
pub const RELEASE_098_MARKER: &str = "0.9.9";
/// Where [`repair_098_drop`] moves 0.9.8's marker: kept, not deleted, as a record.
pub const REPAIRED_MARKER: &str = "bindings.v2.dropped-0.9.8";

/// Whether the bridge pairs through accounts, from the gate's answer (`Some(true)`: it advertises
/// an accounts service; `Some(false)`: it answered without one; `None`: no answer). Without an
/// answer nothing on disk changes and the folder decides: an accounts drop already ran, or a chat
/// is bound.
pub fn accounts_mode(dir: &Path, gate: Option<bool>) -> bool {
    gate.unwrap_or_else(|| marker_text(dir).as_deref() == Some(ACCOUNTS_MARKER.trim()) || !Bindings::load(dir).is_empty())
}

fn marker_text(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("bindings.v2")).ok().map(|text| text.trim().to_string())
}

/// What `serve` does to the folder for the gate's answer; returns the line to print, if any.
/// With accounts: the one-time drop of the 0.9.7 pairings. Without: the repair of a 0.9.8 drop.
/// Without an answer: nothing.
pub fn settle_pairings(dir: &Path, gate: Option<bool>) -> Option<&'static str> {
    match gate {
        Some(true) => drop_old_pairings(dir).then_some(
            "ragon chat: earlier pairings were dropped; pair each chat again with `ragon pair <telegram|discord|matrix>`.",
        ),
        Some(false) => repair_098_drop(dir).then_some(
            "ragon chat: 0.9.8 dropped this node's Telegram and Discord pairings, which cannot be restored: pair each user again with `ragon chat telegram pair` or `ragon chat discord pair`. Matrix needs nothing.",
        ),
        None => None,
    }
}

/// Undoes what 0.9.8 left on a node without accounts: its drop ran there although nothing could
/// replace the pairings, and its marker would stop the real drop when accounts arrive. The marker
/// is moved aside (not deleted) so that drop runs again then; the dropped lists themselves are gone
/// (0.9.8 emptied them in place), so the users pair again. Touches nothing when a chat is bound,
/// and returns whether it moved the marker.
pub fn repair_098_drop(dir: &Path) -> bool {
    if marker_text(dir).as_deref() != Some(RELEASE_098_MARKER) || !Bindings::load(dir).is_empty() {
        return false;
    }
    std::fs::rename(dir.join("bindings.v2"), dir.join(REPAIRED_MARKER)).is_ok()
}

/// `/whoami` and `/sessions` on a node without accounts.
pub const NO_ACCOUNTS: &str = "This node has no accounts: everyone paired with this bot talks to it as its owner.";

/// The one reply an unbound chat gets (at most once a day), and a wrong code's reply.
pub fn instruction(kind: &str) -> String {
    format!("This {kind} chat is not paired to a Ragon account. On a machine where you are signed in to Ragon, run `ragon pair {kind}`, then send me the code it shows.")
}

/// Whether `chat` may be told how to pair now (once a day), and records that it was.
pub fn hint_due(dir: &Path, chat: &str, now_secs: u64) -> bool {
    let file = dir.join("pairing-hints.json");
    let mut value: Value = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}));
    let last = value.get(chat).and_then(Value::as_u64);
    if last.is_some_and(|at| now_secs.saturating_sub(at) < HINT_EVERY_SECS) {
        return false;
    }
    if let Some(object) = value.as_object_mut() {
        object.retain(|_, at| at.as_u64().is_some_and(|at| now_secs.saturating_sub(at) < HINT_EVERY_SECS));
        object.insert(chat.to_string(), json!(now_secs));
    }
    let _ = write_private(&file, &value.to_string());
    true
}

/// The `/whoami` line.
pub fn whoami(username: &str, node: &str) -> String {
    format!("Paired to {username} on {node}")
}

/// What the bridge knows of its primary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Primary {
    /// The gate's base URL.
    pub gate: String,
    /// The node's display name, for the confirmation line.
    pub node: String,
    /// The node's service key, which only redeems codes.
    pub service_key: String,
    /// The primary's CA, when it has its own.
    pub ca_pem: Option<Vec<u8>>,
}

/// Why a code did not bind.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// The primary said the code is not valid (a wrong code is the same reply as an expired one).
    Invalid,
    /// The primary could not be asked: no configuration, or it is down (§7).
    Unreachable,
}

/// A redeemed code.
#[derive(Debug, PartialEq, Eq)]
pub struct Redeemed {
    pub username: String,
    pub key: String,
}

/// Writes where the primary is, for the bridge: called by the pairing step.
///
/// # Errors
/// The folder cannot be written.
pub fn write_primary(dir: &Path, gate: &str, node: &str, ca_pem: Option<&[u8]>) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_private(&dir.join("primary.json"), &json!({ "gate": gate, "node": node }).to_string())?;
    if let Some(pem) = ca_pem {
        write_private(&dir.join("primary-ca.pem"), &String::from_utf8_lossy(pem))?;
    }
    Ok(())
}

/// The node's service key: `RAGON_API_KEY`, else `~/.config/dreamference/service-key`.
pub fn service_key(home: &Path) -> Option<String> {
    let from_env = std::env::var("RAGON_API_KEY").ok();
    let key = from_env.or_else(|| std::fs::read_to_string(home.join(".config/dreamference/service-key")).ok())?;
    let key = key.trim().to_string();
    (!key.is_empty()).then_some(key)
}

impl Primary {
    /// Reads `primary.json` and the service key; `None` while the machine was never paired from.
    pub fn load(dir: &Path, home: &Path) -> Option<Primary> {
        let value: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("primary.json")).ok()?).ok()?;
        let gate = value.get("gate")?.as_str()?.trim_end_matches('/').to_string();
        let node = value.get("node").and_then(Value::as_str).unwrap_or("this node").to_string();
        Some(Primary { gate, node, service_key: service_key(home).unwrap_or_default(), ca_pem: std::fs::read(dir.join("primary-ca.pem")).ok() })
    }

    fn client(&self) -> Result<reqwest::Client, Refused> {
        let mut builder = reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(10));
        if let Some(pem) = &self.ca_pem
            && let Ok(cert) = reqwest::Certificate::from_pem(pem)
        {
            builder = builder.add_root_certificate(cert);
        }
        builder.build().map_err(|_| Refused::Unreachable)
    }

    /// Redeems a pairing code for the chat identity: the primary binds it to the code's user and
    /// answers the user's name and the binding's key (shown once).
    ///
    /// # Errors
    /// [`Refused::Invalid`] for a code the primary refuses, [`Refused::Unreachable`] otherwise.
    pub async fn redeem(&self, code: &str, kind: &str, identity: &str) -> Result<Redeemed, Refused> {
        if self.service_key.is_empty() {
            return Err(Refused::Unreachable);
        }
        let body = json!({ "code": code, "chat_kind": kind, "chat_identity": identity });
        let response = self
            .client()?
            .post(format!("{}/ragon/v1/bindings/redeem", self.gate))
            .bearer_auth(&self.service_key)
            .json(&body)
            .send()
            .await
            .map_err(|_| Refused::Unreachable)?;
        let status = response.status();
        let value: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            let field = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
            return match (field("username"), field("key")) {
                (Some(username), Some(key)) => Ok(Redeemed { username, key }),
                _ => Err(Refused::Unreachable),
            };
        }
        if status.is_client_error() { Err(Refused::Invalid) } else { Err(Refused::Unreachable) }
    }
}

/// The home a chat folder (`<home>/.ragon/chat`) sits in.
pub fn home_of(dir: &Path) -> PathBuf {
    dir.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

/// Whether a whole message is shaped like a pairing code (six letters and digits).
pub fn looks_like_code(text: &str) -> bool {
    let text = text.trim();
    text.len() == 6 && text.chars().all(|c| "ABCDEFGHJKMNPQRSTUVWXYZ23456789".contains(c))
}

/// The node name `primary.json` holds, for `/whoami`.
pub fn node_name(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("primary.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("node").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| "this node".to_string())
}

/// The account commands chat refuses: they are answered at a computer, where the password is
/// typed (§14.1). `/whoami` and `/sessions` are the only ones chat answers.
pub const REFUSED_COMMANDS: [&str; 9] = ["/login", "/logout", "/users", "/password", "/keys", "/user", "/account", "/signin", "/signout"];

/// The refusal line.
pub const REFUSAL: &str = "Account commands are not available in chat: run them at a computer where you are signed in. /whoami and /sessions work here.";

impl Primary {
    /// The chat bindings of the user whose binding key is `key`: `(id, kind, identity)`.
    pub async fn sessions(&self, key: &str) -> Result<Vec<(String, String, String)>, Refused> {
        let response = self.client()?.get(format!("{}/ragon/v1/bindings", self.gate)).bearer_auth(key).send().await.map_err(|_| Refused::Unreachable)?;
        if !response.status().is_success() {
            return Err(Refused::Unreachable);
        }
        let value: Value = response.json().await.map_err(|_| Refused::Unreachable)?;
        let text = |item: &Value, name: &str| item.get(name).map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).unwrap_or_default();
        Ok(value
            .get("bindings")
            .and_then(Value::as_array)
            .map(|items| items.iter().map(|b| (text(b, "id"), text(b, "chat_kind"), text(b, "chat_identity"))).collect())
            .unwrap_or_default())
    }

    /// Revokes one of the user's bindings (and its key).
    pub async fn revoke(&self, key: &str, id: &str) -> Result<(), Refused> {
        let response = self.client()?.delete(format!("{}/ragon/v1/bindings/{id}", self.gate)).bearer_auth(key).send().await.map_err(|_| Refused::Unreachable)?;
        if response.status().is_success() { Ok(()) } else { Err(Refused::Invalid) }
    }
}

/// Binds `identity` to the user the code belongs to, through the primary, and saves the binding.
/// The one call the adapters make when a message carries a code.
///
/// # Errors
/// [`Refused`], or `Unreachable` when the binding could not be saved.
pub async fn pair(dir: &Path, home: &Path, kind: &str, code: &str, identity: &str, chat: &str) -> Result<(Redeemed, String), Refused> {
    let primary = Primary::load(dir, home).ok_or(Refused::Unreachable)?;
    let redeemed = primary.redeem(code, kind, identity).await?;
    let mut bindings = Bindings::load(dir);
    bindings.bind(kind, identity, Binding { username: redeemed.username.clone(), key: redeemed.key.clone(), chat: chat.to_string() });
    bindings.save(dir).map_err(|_| Refused::Unreachable)?;
    let line = whoami(&redeemed.username, &primary.node);
    Ok((redeemed, line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ragon-bindings-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_binding_round_trips_and_one_identity_has_one_user() {
        let dir = dir();
        let mut bindings = Bindings::default();
        bindings.bind("telegram", "42", Binding { username: "ada".into(), key: "sk-ragon-1".into(), chat: "telegram:42".into() });
        bindings.bind("telegram", "42", Binding { username: "bob".into(), key: "sk-ragon-2".into(), chat: "telegram:42".into() });
        bindings.save(&dir).unwrap();
        let loaded = Bindings::load(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded.get("telegram", "42").unwrap().username, "bob");
        assert_eq!(loaded.for_chat("telegram:42").unwrap().key, "sk-ragon-2");
        #[cfg(unix)]
        assert_eq!(std::fs::metadata(path(&dir)).unwrap().permissions().mode_bits(), 0o600);
    }

    #[cfg(unix)]
    trait Bits {
        fn mode_bits(&self) -> u32;
    }
    #[cfg(unix)]
    impl Bits for std::fs::Permissions {
        fn mode_bits(&self) -> u32 {
            std::os::unix::fs::PermissionsExt::mode(self) & 0o777
        }
    }

    #[test]
    fn the_upgrade_drops_old_pairings_once() {
        let dir = dir();
        std::fs::write(dir.join("telegram.json"), r#"{"token":"t","users":[1,2]}"#).unwrap();
        std::fs::write(dir.join("discord.json"), r#"{"token":"d","users":{"9":"c"}}"#).unwrap();
        assert!(drop_old_pairings(&dir));
        let telegram: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("telegram.json")).unwrap()).unwrap();
        assert_eq!(telegram["users"], json!([]));
        assert_eq!(telegram["token"], "t");
        let discord: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("discord.json")).unwrap()).unwrap();
        assert_eq!(discord["users"], json!({}));
        assert!(!drop_old_pairings(&dir), "the second start drops nothing");
    }

    #[test]
    fn an_unbound_chat_is_told_once_a_day() {
        let dir = dir();
        assert!(hint_due(&dir, "telegram:1", 1_000));
        assert!(!hint_due(&dir, "telegram:1", 1_000 + 60));
        assert!(hint_due(&dir, "telegram:2", 1_060));
        assert!(hint_due(&dir, "telegram:1", 1_000 + HINT_EVERY_SECS));
    }

    #[test]
    fn the_instruction_names_the_command_and_the_confirmation_the_account() {
        assert!(instruction("matrix").contains("ragon pair matrix"));
        assert_eq!(whoami("ada", "dgx-1"), "Paired to ada on dgx-1");
    }
}
