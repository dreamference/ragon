//! The browser sign-in of `ragon web` (specs/DREAMFERENCE_AUTH.md §5.8, §14.4).
//!
//! `ragon web` has no accounts of its own. The login page signs in with the primary's `/oauth/token`
//! password grant (through this server, so the page stays same-origin) and keeps the JWT in the
//! browser's local storage. This module forwards the page's calls to the accounts service on
//! loopback, checks a Bearer JWT with the service's own `/ragon/v1/users/me`, and holds the
//! browser session's tokens for the app-server: handed over with `chatgptAuthTokens`, refreshed
//! when the app-server sends `account/chatgptAuthTokens/refresh`.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;

use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio_tungstenite::tungstenite::Message as UpstreamMessage;

use crate::relay::next_internal_id;

/// The accounts service on loopback (`ports.py` ACCOUNTS_PORT).
pub const ACCOUNTS_ADDR: &str = "127.0.0.1:18100";

/// The OAuth client id of the browser page.
pub const CLIENT_ID: &str = "ragon-web";

/// How long a verified Bearer is trusted before the service is asked again.
const VERIFIED_FOR: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_ANSWER_BYTES: u64 = 1024 * 1024;

/// Who a verified token belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub user_id: String,
    pub username: String,
    pub role: String,
}

impl Identity {
    fn from_profile(profile: &Value) -> Option<Identity> {
        let username = profile.get("username")?.as_str()?.to_string();
        let user_id = match profile.get("user_id").or_else(|| profile.get("id")) {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Number(number)) => number.to_string(),
            _ => username.clone(),
        };
        let role = profile.get("role").and_then(Value::as_str).unwrap_or("user").to_string();
        Some(Identity { user_id, username, role })
    }
}

/// The tokens of the browser session the app-server works under.
#[derive(Clone, Debug)]
struct Handoff {
    access_token: String,
    refresh_token: String,
    identity: Identity,
}

/// The sign-in state of one `ragon web`.
pub struct Login {
    addr: String,
    handoff: Mutex<Option<Handoff>>,
    verified: Mutex<HashMap<String, (Instant, Identity)>>,
    upstreams: Mutex<HashMap<u64, UnboundedSender<UpstreamMessage>>>,
    /// Ids of the `account/login/start` requests sent, whose answers the page never sees.
    sent: Mutex<std::collections::HashSet<i64>>,
}

/// The `Authorization: Bearer` value of a request, when it has one.
pub fn bearer(header: Option<&str>) -> Option<&str> {
    let token = header?.strip_prefix("Bearer ")?.trim();
    (!token.is_empty()).then_some(token)
}

/// `application/x-www-form-urlencoded` of `fields`.
pub fn form_encode(fields: &[(&str, &str)]) -> String {
    fn escape(text: &str) -> String {
        let mut out = String::new();
        for byte in text.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
                _ => out.push_str(&format!("%{byte:02X}")),
            }
        }
        out
    }
    fields.iter().map(|(name, value)| format!("{}={}", escape(name), escape(value))).collect::<Vec<_>>().join("&")
}

/// One HTTP request to the service at `addr`; the status and the JSON body (Null when empty).
pub async fn call(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: Option<(&str, &str)>) -> Result<(u16, Value), String> {
    let mut stream = match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(Ok(stream)) => stream,
        _ => return Err("the accounts service is not reachable".to_string()),
    };
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAccept: application/json\r\n");
    for (name, value) in headers {
        if value.contains(['\r', '\n']) {
            return Err("bad header".to_string());
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    let payload = body.map(|(_, text)| text).unwrap_or("");
    if let Some((content_type, _)) = body {
        head.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\n\r\n", payload.len()));
    let exchange = async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(payload.as_bytes()).await?;
        let mut raw = Vec::new();
        (&mut stream).take(MAX_ANSWER_BYTES).read_to_end(&mut raw).await?;
        Ok::<_, std::io::Error>(raw)
    };
    let raw = match tokio::time::timeout(ANSWER_TIMEOUT, exchange).await {
        Ok(Ok(raw)) => raw,
        _ => return Err("the accounts service did not answer".to_string()),
    };
    let (status, body) = crate::sidecars::parse_response(&raw)?;
    Ok((status, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

impl Login {
    pub fn new(addr: &str) -> Login {
        Login { addr: addr.to_string(), handoff: Mutex::new(None), verified: Mutex::new(HashMap::new()), upstreams: Mutex::new(HashMap::new()), sent: Mutex::new(Default::default()) }
    }

    /// The service's answer to a grant at `/oauth/token`, passed on unchanged.
    pub async fn token(&self, fields: &[(&str, &str)]) -> Result<(u16, Value), String> {
        call(&self.addr, "POST", "/oauth/token", &[], Some(("application/x-www-form-urlencoded", &form_encode(fields)))).await
    }

    /// A call to the service on behalf of the signed-in user.
    pub async fn as_user(&self, method: &str, path: &str, token: &str, body: Option<&Value>) -> Result<(u16, Value), String> {
        let bearer = format!("Bearer {token}");
        let text = body.map(Value::to_string);
        call(&self.addr, method, path, &[("Authorization", &bearer)], text.as_deref().map(|text| ("application/json", text))).await
    }

    /// Who the Bearer JWT is, asked of the service (so revoked sessions stop at once, after at most
    /// the short cache). None when the service refuses it or cannot be reached: it opens nothing.
    pub async fn verify(&self, token: &str) -> Option<Identity> {
        let key = crate::auth::sha256_hex(token);
        {
            let mut verified = self.verified.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            verified.retain(|_, (when, _)| when.elapsed() < VERIFIED_FOR);
            if let Some((_, identity)) = verified.get(&key) {
                return Some(identity.clone());
            }
        }
        let (status, profile) = self.as_user("GET", "/ragon/v1/users/me", token, None).await.ok()?;
        if status != 200 {
            return None;
        }
        let identity = Identity::from_profile(&profile)?;
        self.verified.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(key, (Instant::now(), identity.clone()));
        Some(identity)
    }

    /// The `account/login/start` request that gives the app-server the session's tokens.
    pub fn login_request(&self, access_token: &str, identity: &Identity) -> Value {
        let id = next_internal_id();
        self.sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(id);
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "account/login/start",
            "params": {
                "type": "chatgptAuthTokens",
                "accessToken": access_token,
                "chatgptAccountId": identity.user_id,
                "chatgptPlanType": identity.role,
            },
        })
    }

    /// Keeps the browser session's tokens and hands them to every app-server connection.
    /// Returns how many connections were told.
    pub fn set_session(&self, access_token: &str, refresh_token: &str, identity: Identity) -> usize {
        *self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Handoff { access_token: access_token.to_string(), refresh_token: refresh_token.to_string(), identity: identity.clone() });
        let mut told = 0;
        for tx in self.upstreams.lock().unwrap_or_else(std::sync::PoisonError::into_inner).values() {
            if tx.send(UpstreamMessage::text(self.login_request(access_token, &identity).to_string())).is_ok() {
                told += 1;
            }
        }
        told
    }

    /// Forgets the session at logout.
    pub fn clear_session(&self) {
        *self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    /// Whether a browser session is held.
    pub fn has_session(&self) -> bool {
        self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_some()
    }

    /// A new app-server connection: remembered for later hand-overs, and given the tokens now.
    pub fn register(&self, tab: u64, tx: UnboundedSender<UpstreamMessage>) {
        let held = self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        if let Some(held) = held {
            let _ = tx.send(UpstreamMessage::text(self.login_request(&held.access_token, &held.identity).to_string()));
        }
        self.upstreams.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(tab, tx);
    }

    /// A closed connection.
    pub fn forget(&self, tab: u64) {
        self.upstreams.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&tab);
    }

    /// The answer to the app-server's `account/chatgptAuthTokens/refresh` request `id`: a refresh
    /// at the primary with the session's refresh token, keeping the new set.
    pub async fn refresh_answer(&self, id: Value) -> Value {
        let held = self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let failed = |message: &str| json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32001, "message": message } });
        let Some(held) = held else {
            return failed("No one is signed in to ragon web.");
        };
        let fields = [("grant_type", "refresh_token"), ("refresh_token", held.refresh_token.as_str()), ("client_id", CLIENT_ID)];
        let set = match self.token(&fields).await {
            Ok((200, set)) => set,
            Ok(_) => {
                self.clear_session();
                return failed("The session ended: sign in again.");
            }
            Err(reason) => return failed(&reason),
        };
        let Some(access) = set.get("access_token").and_then(Value::as_str) else {
            return failed("The authentication server sent no token.");
        };
        let refresh = set.get("refresh_token").and_then(Value::as_str).unwrap_or(&held.refresh_token);
        *self.handoff.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Handoff { access_token: access.to_string(), refresh_token: refresh.to_string(), identity: held.identity.clone() });
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "accessToken": access, "chatgptAccountId": held.identity.user_id, "chatgptPlanType": held.identity.role },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_needs_the_scheme_and_a_value() {
        assert_eq!(bearer(Some("Bearer abc ")), Some("abc"));
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(None), None);
    }

    #[test]
    fn form_fields_are_escaped() {
        assert_eq!(form_encode(&[("a", "x y&z"), ("b", "")]), "a=x%20y%26z&b=");
    }
}

impl Login {
    /// Whether `value` answers one of the `account/login/start` requests sent here (removing it).
    pub fn take_login_answer(&self, value: &Value) -> bool {
        if value.get("method").is_some() {
            return false;
        }
        let Some(id) = value.get("id").and_then(Value::as_i64) else {
            return false;
        };
        self.sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&id)
    }
}
