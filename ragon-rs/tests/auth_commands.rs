//! `ragon login`, `login status`, `sessions` and `logout --all` against a stub primary
//! (specs/DREAMFERENCE_AUTH.md §11).

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use ragon_launcher::auth::commands::{self, Cmd, Io};
use ragon_launcher::auth::switcher;
use serde_json::{Value, json};

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-auth-commands-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn jwt(user: &str, role: &str) -> String {
    let enc = |v: Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!("{}.{}.sig", enc(json!({"alg": "none"})), enc(json!({"preferred_username": user, "role": role})))
}

/// A stub primary: answers the routes the commands use and records `METHOD path body`.
struct Stub {
    gate: String,
    seen: Arc<Mutex<Vec<String>>>,
}

fn stub(taken: &'static [&'static str]) -> Stub {
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
            let mut auth = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.trim().is_empty() {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if lower.starts_with("authorization:") {
                    auth = line.trim().to_string();
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8_lossy(&body).into_owned();
            let mut parts = request.split_whitespace();
            let (method, path) = (parts.next().unwrap().to_string(), parts.next().unwrap().to_string());
            log.lock().unwrap().push(format!("{method} {path} {body} {auth}"));
            let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let user = parsed.get("username").and_then(Value::as_str).unwrap_or("");
            let (status, answer) = match (method.as_str(), path.as_str()) {
                ("GET", "/ragon/v1/info") => (200, json!({"node_id": "n1", "node_name": "box", "primary_id": "p1", "primary_name": "main-box"})),
                ("POST", "/oauth/token") if parsed["password"] == "right" => {
                    (200, json!({"id_token": jwt(user, "user"), "access_token": "at", "refresh_token": "rt", "expires_in": 3600, "token_type": "Bearer"}))
                }
                ("POST", "/oauth/token") => (401, json!({"error": "invalid_grant"})),
                ("POST", "/ragon/v1/users") if taken.contains(&user) => (409, json!({"error": "taken"})),
                ("POST", "/ragon/v1/users") => (200, json!({"id_token": jwt(user, "admin"), "access_token": "at", "refresh_token": "rt"})),
                ("GET", "/ragon/v1/users/me") if auth.ends_with("at") => (200, json!({"username": "ann", "role": "user"})),
                ("GET", "/ragon/v1/users/me") => (200, json!({"username": "bot", "role": "user"})),
                ("GET", "/ragon/v1/users/me/sessions") => (
                    200,
                    json!({"sessions": [{"id": "s1", "machine": "laptop", "client": "ragon-cli", "last_used": "today"}],
                           "bindings": [{"id": "b1", "kind": "signal", "last_used": "never"}]}),
                ),
                ("DELETE", _) => (200, json!({})),
                _ => (404, json!({})),
            };
            let text = answer.to_string();
            let _ = write!(stream, "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
        }
    });
    Stub { gate, seen }
}

async fn run(which: &str, args: &[&str], stub: &Stub, home: &PathBuf, stdin: &str) -> (commands::Outcome, String) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let mut input = Cursor::new(stdin.as_bytes().to_vec());
    let mut out = Vec::new();
    let outcome = {
        let mut io = Io { input: &mut input, out: &mut out, tty: false };
        commands::run(which, &args, &stub.gate, home, &mut io).await.unwrap()
    };
    (outcome, String::from_utf8(out).unwrap())
}

#[test]
fn the_forms_are_parsed_and_the_codex_ones_forwarded() {
    let args = |list: &[&str]| list.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    assert_eq!(commands::parse_login(&[]), Ok(Cmd::Screen));
    assert_eq!(commands::parse_login(&args(&["--create"])), Ok(Cmd::CreateScreen));
    assert_eq!(
        commands::parse_login(&args(&["--username", "ann", "--password-stdin"])),
        Ok(Cmd::Password { username: "ann".into(), create: false, stdin: true })
    );
    assert!(commands::parse_login(&args(&["--password-stdin"])).is_err());
    assert_eq!(commands::parse_login(&args(&["status", "--all"])), Ok(Cmd::Status { all: true }));
    assert_eq!(commands::parse_sessions(&args(&["--revoke", "s1"])), Ok(Cmd::Sessions { revoke: Some("s1".into()) }));
    assert_eq!(commands::parse_logout(&args(&["--all"])), Cmd::LogoutAll);
    let Ok(Cmd::Forward(device)) = commands::parse_login(&args(&["--device-auth"])) else { panic!("device auth is forwarded") };
    assert_eq!(
        commands::with_issuer(device, "https://box.local:8000/"),
        args(&["login", "--device-auth", "--experimental_issuer", "https://box.local:8000"])
    );
    assert_eq!(commands::parse_login(&args(&["--with-api-key"])), Ok(Cmd::Forward(args(&["login", "--with-api-key"]))));
    assert_eq!(commands::parse_logout(&[]), Cmd::Forward(args(&["logout"])));
}

#[tokio::test]
async fn password_stdin_signs_in_and_status_names_the_user() {
    let (stub, home) = (stub(&[]), tempdir("signin"));
    let (outcome, out) = run("login", &["--username", "ann", "--password-stdin"], &stub, &home, "right\n").await;
    assert_eq!(outcome.code, 0, "{out}");
    assert!(out.contains("Signed in as ann (user) at box (accounts kept by main-box)"), "{out}");
    let state = switcher::read_state(&home).unwrap();
    assert_eq!(state.active.as_deref(), Some("p1"));
    assert_eq!(state.entries["p1"]["tokens"]["refresh_token"], "rt");
    let seen = stub.seen.lock().unwrap().join("\n");
    assert!(seen.contains("\"password\":\"right\""), "the password reaches the gate in the body only");
    let (status, line) = run("login", &["status"], &stub, &home, "").await;
    assert_eq!((status.code, line.contains("Signed in as ann (user) at box")), (0, true), "{line}");
    let (_, all) = run("login", &["status", "--all"], &stub, &home, "").await;
    assert_eq!(all.trim(), "* p1: ann (user)");
    assert_eq!(doctor(&home), "signed in as ann (user)");
}

fn doctor(home: &PathBuf) -> String {
    commands::doctor_line(home, None)
}

#[tokio::test]
async fn a_wrong_password_writes_nothing_and_status_exits_one() {
    let (stub, home) = (stub(&[]), tempdir("wrong"));
    let (outcome, out) = run("login", &["--username", "ann", "--password-stdin"], &stub, &home, "nope\n").await;
    assert_eq!(outcome.code, 1);
    assert!(out.contains("Wrong username or password."), "{out}");
    assert!(!home.join("auth.json").exists());
    let (status, line) = run("login", &["status"], &stub, &home, "").await;
    assert_eq!(status.code, 1);
    assert_eq!(line.trim(), "Not signed in. Run ragon login.");
    assert_eq!(doctor(&home), "not signed in (run ragon login)");
}

#[tokio::test]
async fn create_names_the_first_admin_and_a_taken_name_is_refused() {
    let (stub, home) = (stub(&["bob"]), tempdir("create"));
    let (outcome, out) = run("login", &["--create", "--username", "ann", "--password-stdin"], &stub, &home, "pw\n").await;
    assert_eq!(outcome.code, 0, "{out}");
    assert!(out.contains("You are the first user of main-box and its admin."), "{out}");
    let (outcome, out) = run("login", &["--create", "--username", "bob", "--password-stdin"], &stub, &tempdir("taken"), "pw\n").await;
    assert_eq!(outcome.code, 1);
    assert!(out.contains("That username is taken."), "{out}");
}

#[tokio::test]
async fn the_screen_reads_the_choice_then_the_form() {
    let (stub, home) = (stub(&[]), tempdir("screen"));
    let (outcome, out) = run("login", &[], &stub, &home, "1\nann\nright\n").await;
    assert_eq!(outcome.code, 0, "{out}");
    assert!(out.contains("Sign in to box") && out.contains("2. Create an account on main-box"), "{out}");
    let (outcome, _) = run("login", &[], &stub, &tempdir("screen3"), "3\n").await;
    assert_eq!(
        outcome.forward,
        Some(["login", "--device-auth", "--experimental_issuer", stub.gate.as_str()].map(String::from).to_vec())
    );
}

#[tokio::test]
async fn forwarded_forms_carry_the_issuer_and_stay_unhandled() {
    let (stub, home) = (stub(&[]), tempdir("forward"));
    let (outcome, _) = run("login", &["--device-auth"], &stub, &home, "").await;
    assert_eq!(outcome.forward.unwrap().last().map(String::as_str), Some(stub.gate.as_str()));
    let (outcome, _) = run("login", &["--with-api-key"], &stub, &home, "").await;
    assert_eq!(outcome.forward, Some(vec!["login".to_string(), "--with-api-key".to_string()]));
    let (outcome, _) = run("logout", &[], &stub, &home, "").await;
    assert_eq!(outcome.forward, Some(vec!["logout".to_string()]));
}

#[tokio::test]
async fn sessions_list_and_revoke_use_the_bearer() {
    let (stub, home) = (stub(&[]), tempdir("sessions"));
    let (outcome, out) = run("sessions", &[], &stub, &home, "").await;
    assert_eq!((outcome.code, out.trim()), (1, "Not signed in. Run ragon login."));
    run("login", &["--username", "ann", "--password-stdin"], &stub, &home, "right\n").await;
    let (_, out) = run("sessions", &[], &stub, &home, "").await;
    assert!(out.contains("session s1  laptop  ragon-cli  last used today") && out.contains("binding b1  signal"), "{out}");
    let (outcome, out) = run("sessions", &["--revoke", "s1"], &stub, &home, "").await;
    assert_eq!(outcome.code, 0, "{out}");
    assert!(stub.seen.lock().unwrap().iter().any(|r| r.starts_with("DELETE /ragon/v1/users/me/sessions/s1 ") && r.ends_with("Bearer at")));
}

#[tokio::test]
async fn logout_all_deletes_the_sessions_then_hands_the_logout_to_codex() {
    let (stub, home) = (stub(&[]), tempdir("logoutall"));
    run("login", &["--username", "ann", "--password-stdin"], &stub, &home, "right\n").await;
    let (outcome, out) = run("logout", &["--all"], &stub, &home, "").await;
    assert_eq!(outcome.forward, Some(vec!["logout".to_string()]), "{out}");
    assert!(stub.seen.lock().unwrap().iter().any(|r| r.starts_with("DELETE /ragon/v1/users/me/sessions ")), "all sessions, no key route");
    assert!(!stub.seen.lock().unwrap().iter().any(|r| r.contains("/keys")), "API keys are kept");
}
