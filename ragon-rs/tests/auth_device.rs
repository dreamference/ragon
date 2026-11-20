//! `ragon login --device-auth` and the app-server's `account/ragon/signIn` against a stub primary
//! (specs/DREAMFERENCE_AUTH.md §5.5, §5.8). Codex's device client is not patched, so the stub
//! answers in Codex's shape (user code, 403 while pending, approval, exchange) and a reader of
//! that shape follows it; the launcher's part is the forward with the issuer named.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ragon_launcher::auth::commands::{self, Cmd};
use ragon_launcher::auth::signin_rpc::{self, NAME_TAKEN, UNREACHABLE, WRONG_CREDENTIALS};
use ragon_launcher::auth::switcher;
use serde_json::{Value, json};

/// The issuer is a process variable: the tests that set it run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-auth-device-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn token(user: &str, role: &str) -> String {
    use base64::Engine;
    let enc = |v: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!("{}.{}.sig", enc(json!({"alg": "none"})), enc(json!({"preferred_username": user, "role": role})))
}

fn start_stub(approve_after: usize) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let gate = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let polls = AtomicUsize::new(0);
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
            let body = String::from_utf8_lossy(&body).into_owned();
            let mut parts = request.split_whitespace();
            let (method, path) = (parts.next().unwrap().to_string(), parts.next().unwrap().to_string());
            log.lock().unwrap().push(format!("{method} {path} {body}"));
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let name = parsed["username"].as_str().unwrap_or("");
            let tokens = |user: &str, role: &str| json!({"id_token": token(user, role), "access_token": "at", "refresh_token": "rt"});
            let (status, answer) = match (method.as_str(), path.as_str()) {
                ("GET", "/ragon/v1/info") => (200, json!({"node_id": "n1", "node_name": "box", "primary_id": "p1", "primary_name": "main-box"})),
                ("POST", "/api/accounts/deviceauth/usercode") => (200, json!({"device_auth_id": "d1", "user_code": "ABCD-1234", "interval": "0"})),
                ("POST", "/api/accounts/deviceauth/token") => {
                    if polls.fetch_add(1, Ordering::SeqCst) < approve_after {
                        (403, json!({}))
                    } else {
                        (200, json!({"authorization_code": "ac", "code_challenge": "cc", "code_verifier": "cv"}))
                    }
                }
                ("POST", "/oauth/token") if parsed["grant_type"] == "password" && parsed["password"] == " pw " && name == " ann " => (200, tokens(" ann ", "admin")),
                ("POST", "/oauth/token") if parsed["grant_type"] == "password" && parsed["password"] == "" && name == "empty" => (200, tokens("empty", "user")),
                ("POST", "/oauth/token") if parsed["grant_type"] == "authorization_code" => (200, tokens("dev", "user")),
                ("POST", "/oauth/token") => (401, json!({"error": "invalid_grant"})),
                ("POST", "/ragon/v1/users") if name == "taken" => (409, json!({"error": "taken"})),
                ("POST", "/ragon/v1/users") if name == "bad name" => (422, json!({})),
                ("POST", "/ragon/v1/users") => (200, tokens(name, "user")),
                _ => (404, json!({})),
            };
            let text = answer.to_string();
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
        }
    });
    (gate, seen)
}

#[tokio::test(flavor = "current_thread")]
async fn the_device_form_is_forwarded_with_the_issuer_and_the_stub_answers_in_codexs_shape() {
    let (gate, seen) = start_stub(2);
    let args: Vec<String> = vec!["--device-auth".into()];
    let Ok(Cmd::Forward(forward)) = commands::parse_login(&args) else { panic!("device auth is forwarded to Codex") };
    assert_eq!(commands::with_issuer(forward, &gate), ["login", "--device-auth", "--experimental_issuer", gate.as_str()]);

    // What Codex's unpatched client does: user code, poll through 403, exchange the code.
    let client = reqwest::Client::new();
    let code: Value = client.post(format!("{gate}/api/accounts/deviceauth/usercode")).json(&json!({"client_id": "c"})).send().await.unwrap().json().await.unwrap();
    assert_eq!(code["user_code"], "ABCD-1234");
    let mut pending = 0;
    let approved = loop {
        let answer = client
            .post(format!("{gate}/api/accounts/deviceauth/token"))
            .json(&json!({"device_auth_id": code["device_auth_id"], "user_code": code["user_code"]}))
            .send()
            .await
            .unwrap();
        if answer.status() == 403 {
            pending += 1;
            continue;
        }
        break answer.json::<Value>().await.unwrap();
    };
    assert_eq!(pending, 2);
    let body = json!({"grant_type": "authorization_code", "code": approved["authorization_code"], "redirect_uri": format!("{gate}/deviceauth/callback"), "code_verifier": approved["code_verifier"]});
    let tokens: Value = client.post(format!("{gate}/oauth/token")).json(&body).send().await.unwrap().json().await.unwrap();
    assert_eq!(tokens["access_token"], "at");
    assert!(seen.lock().unwrap().iter().any(|line| line.contains("/deviceauth/callback")));
}

#[tokio::test(flavor = "current_thread")]
async fn the_signin_rpc_keeps_the_text_as_typed_and_writes_the_active_entry() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (gate, seen) = start_stub(0);
    unsafe { std::env::set_var("CODEX_APP_SERVER_LOGIN_ISSUER", &gate) };
    let home = tempdir("signin");

    let ok = signin_rpc::sign_in(&home, false, " ann ", " pw ").await.unwrap();
    assert_eq!((ok.username.as_str(), ok.role.as_str(), ok.must_change_password), (" ann ", "admin", false));
    assert!(seen.lock().unwrap().iter().any(|line| line.contains(r#""username":" ann ""#) && line.contains(r#""password":" pw ""#)));
    let state = switcher::read_state(&home).unwrap();
    assert_eq!(state.entries["p1"]["tokens"]["access_token"], "at");

    let empty = signin_rpc::sign_in(&home, false, "empty", "").await.unwrap();
    assert_eq!(empty.username, "empty");

    assert_eq!(signin_rpc::sign_in(&home, false, "ann", "wrong").await.unwrap_err().code, WRONG_CREDENTIALS);
    assert_eq!(signin_rpc::sign_in(&home, true, "taken", "x").await.unwrap_err().code, NAME_TAKEN);
    assert_eq!(signin_rpc::sign_in(&home, true, "bad name", "x").await.unwrap_err().code, NAME_TAKEN);
    assert_eq!(signin_rpc::sign_in(&home, true, "newbie", "x").await.unwrap().username, "newbie");
}

#[tokio::test(flavor = "current_thread")]
async fn the_signin_rpc_names_an_unreachable_primary() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let free = TcpListener::bind("127.0.0.1:0").unwrap();
    let gate = format!("http://{}", free.local_addr().unwrap());
    drop(free);
    unsafe { std::env::set_var("CODEX_APP_SERVER_LOGIN_ISSUER", &gate) };
    let home = tempdir("down");
    assert_eq!(signin_rpc::sign_in(&home, false, "ann", "x").await.unwrap_err().code, UNREACHABLE);
}
