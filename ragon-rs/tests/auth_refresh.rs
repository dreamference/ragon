//! A password sign-in outlives its access token: Codex's own refresh path takes the entry the
//! launcher wrote and refreshes it at the gate (G3, 0.9.9). Its own binary, because the refresh
//! URL is process-wide environment.

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use codex_login::{AuthCredentialsStoreMode, AuthKeyringBackendKind, AuthManager};
use ragon_launcher::auth::commands::{self, Io};
use ragon_launcher::auth::{self, switcher};
use serde_json::{Value, json};

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-auth-refresh-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn jwt(claims: Value) -> String {
    let enc = |v: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!("{}.{}.sig", enc(json!({"alg": "RS256", "typ": "JWT"})), enc(claims))
}

fn payload(token: &str) -> Value {
    let part = token.split('.').nth(1).unwrap();
    serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
}

/// The accounts server's token set: the id token is the access token, a JWT whose auth claim
/// carries the user's id as the account id. `generation` 0 is already expired.
fn token_set(user: &str, generation: u64) -> Value {
    let now = chrono::Utc::now().timestamp();
    let exp = if generation == 0 { now - 60 } else { now + 3600 };
    let token = jwt(json!({
        "iss": "stub", "sub": format!("uid-{user}"), "aud": "ragon", "iat": exp - 3600, "exp": exp,
        "preferred_username": user, "role": "user", "generation": generation,
        "https://api.openai.com/auth": {
            "chatgpt_plan_type": "user", "chatgpt_user_id": format!("uid-{user}"),
            "chatgpt_account_id": format!("uid-{user}"), "user_id": format!("uid-{user}"),
        },
    }));
    json!({"id_token": token, "access_token": token, "refresh_token": format!("rt-{user}"), "expires_in": 3600, "token_type": "Bearer"})
}

struct Stub {
    gate: String,
    seen: Arc<Mutex<Vec<Value>>>,
}

/// One stub primary for the binary, its refresh URL set before any test runs.
fn stub() -> &'static Stub {
    static STUB: OnceLock<Stub> = OnceLock::new();
    STUB.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let gate = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                if reader.read_line(&mut request).unwrap_or(0) == 0 {
                    continue;
                }
                let mut length = 0usize;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; length];
                reader.read_exact(&mut body).unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let mut parts = request.split_whitespace();
                let (method, path) = (parts.next().unwrap().to_string(), parts.next().unwrap().to_string());
                let grant = body["grant_type"].as_str().unwrap_or_default().to_string();
                let (status, answer) = match (method.as_str(), path.as_str(), grant.as_str()) {
                    ("GET", "/ragon/v1/info", _) => (200, json!({"node_id": "n1", "node_name": "box", "primary_id": "p1", "primary_name": "box", "accounts": true})),
                    ("POST", "/oauth/token", "password") => (200, token_set(body["username"].as_str().unwrap_or_default(), 0)),
                    ("POST", "/oauth/token", "refresh_token") => {
                        let user = body["refresh_token"].as_str().unwrap_or_default().strip_prefix("rt-").unwrap_or_default().to_string();
                        (200, token_set(&user, 1))
                    }
                    _ => (404, json!({})),
                };
                if path == "/oauth/token" {
                    let mut entry = body.clone();
                    entry.as_object_mut().map(|o| o.remove("password"));
                    log.lock().unwrap().push(entry);
                }
                let text = answer.to_string();
                let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
            }
        });
        // What the launcher hands Codex for every run (auth::start).
        for (key, value) in auth::codex_env(&gate, None, None, None) {
            unsafe { std::env::set_var(key, value) };
        }
        Stub { gate, seen }
    })
}

async fn sign_in(user: &str, home: &Path) {
    let args: Vec<String> = ["--username", user, "--password-stdin"].iter().map(|a| (*a).to_string()).collect();
    let mut input = Cursor::new(b"pw\n".to_vec());
    let mut out = Vec::new();
    let outcome = {
        let mut io = Io { input: &mut input, out: &mut out, tty: false };
        commands::run("login", &args, &stub().gate, home, &mut io).await.unwrap()
    };
    assert_eq!(outcome.code, 0, "{}", String::from_utf8_lossy(&out));
}

async fn codex(home: &Path) -> Arc<AuthManager> {
    AuthManager::shared(
        home.to_path_buf(),
        false,
        AuthCredentialsStoreMode::File,
        None,
        None,
        AuthKeyringBackendKind::default(),
        codex_login::test_support::transport_default_auth_route_config(),
    )
    .await
}

fn stored(home: &Path) -> Value {
    switcher::read_state(home).unwrap().entries["p1"].clone()
}

fn refreshes(user: &str) -> usize {
    let token = format!("rt-{user}");
    stub().seen.lock().unwrap().iter().filter(|r| r["grant_type"] == "refresh_token" && r["refresh_token"] == token.as_str()).count()
}

#[tokio::test]
async fn codex_refreshes_an_expired_password_sign_in() {
    let home = tempdir("expired");
    sign_in("ann", &home).await;
    assert_eq!(stored(&home)["tokens"]["account_id"], "uid-ann");
    assert_eq!(payload(stored(&home)["tokens"]["access_token"].as_str().unwrap())["generation"], 0);

    // Codex's first use of an access token that expired: the proactive refresh.
    let manager = codex(&home).await;
    let auth = manager.auth().await.expect("signed in");
    assert_eq!(refreshes("ann"), 1, "Codex asked the gate once");
    assert_eq!(payload(&auth.get_token().unwrap())["generation"], 1, "Codex uses the new token");
    let entry = stored(&home);
    assert_eq!(payload(entry["tokens"]["access_token"].as_str().unwrap())["generation"], 1, "and stores it");
    assert_eq!(entry["tokens"]["account_id"], "uid-ann");
    assert_eq!(entry["tokens"]["refresh_token"], "rt-ann");

    // A 401 later on: the recovery after an unauthorized answer refreshes too.
    let mut recovery = manager.unauthorized_recovery();
    recovery.next().await.expect("the recovery reloads");
    recovery.next().await.expect("then refreshes at the gate");
    assert_eq!(refreshes("ann"), 2);
}

#[tokio::test]
async fn an_rc_sign_in_without_account_id_is_refreshed_once_repaired() {
    let home = tempdir("repair");
    sign_in("bob", &home).await;
    // What 0.9.9-rc.1 to rc.3 stored.
    let mut entry = stored(&home);
    entry["tokens"]["account_id"] = Value::Null;
    switcher::save_entry(&home, "p1", entry).unwrap();

    let error = codex(&home).await.refresh_token().await.expect_err("no account id, no refresh");
    assert!(error.to_string().contains("another account"), "{error}");
    assert_eq!(refreshes("bob"), 0);

    // The launcher's start repairs the file before Codex reads it.
    assert!(switcher::fill_account_ids(&home).unwrap());
    assert_eq!(stored(&home)["tokens"]["account_id"], "uid-bob");
    assert!(!switcher::fill_account_ids(&home).unwrap(), "nothing left to repair");
    let auth = codex(&home).await.auth().await.expect("signed in");
    assert_eq!(payload(&auth.get_token().unwrap())["generation"], 1);
    assert_eq!(refreshes("bob"), 1);
}
