//! `ragon login`, `login status`, `sessions` and `logout --all` against a stub primary
//! (specs/DREAMFERENCE_AUTH.md §11).

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use ragon_launcher::auth::commands::{self, Cmd, Io};
use ragon_launcher::auth::strings::{self, Key, Lang};
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
        // The node needs its first account until one is made; the stub's caller is always local.
        let mut first_open = true;
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
                ("GET", "/ragon/v1/info") => (200, json!({"node_id": "n1", "node_name": "box", "primary_id": "p1", "primary_name": "main-box", "accounts": true})),
                ("POST", "/oauth/token") if parsed["password"] == "right" => {
                    (200, json!({"id_token": jwt(user, "user"), "access_token": "at", "refresh_token": "rt", "expires_in": 3600, "token_type": "Bearer"}))
                }
                ("POST", "/oauth/token") => (401, json!({"error": "invalid_grant"})),
                ("POST", "/ragon/v1/users") if taken.contains(&user) => (409, json!({"error": "taken"})),
                ("POST", "/ragon/v1/users") => {
                    first_open = false;
                    (200, json!({"id_token": jwt(user, "admin"), "access_token": "at", "refresh_token": "rt"}))
                }
                ("GET", "/ragon/v1/setup") => (200, json!({"setup": first_open, "local": true, "node": "main-box"})),
                ("GET", "/ragon/v1/users/me") if auth.ends_with("at") => (200, json!({"username": "ann", "role": "user"})),
                ("GET", "/ragon/v1/users/me") => (200, json!({"username": "bot", "role": "user"})),
                ("GET", "/ragon/v1/users/me/sessions") => (
                    200,
                    json!({"sessions": [{"id": "s1", "machine": "laptop", "client": "ragon-cli", "last_used": "today"}],
                           "bindings": [{"id": "b1", "kind": "signal", "last_used": "never"}]}),
                ),
                ("POST", "/ragon/v1/bindings") => (200, json!({"code": "ABC234", "chat_kind": parsed["chat_kind"], "expires_in": 3600})),
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
    assert_eq!(commands::parse_login(&args(&["status", "--json"])), Ok(Cmd::StatusJson));
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

#[cfg(unix)]
async fn pair(args: &[&str], stub: &Stub, home: &PathBuf) -> (i32, String) {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let mut input = Cursor::new(Vec::new());
    let mut out = Vec::new();
    let outcome = {
        let mut io = Io { input: &mut input, out: &mut out, tty: false };
        commands::run_pair(&args, &stub.gate, home, &mut io).await.unwrap()
    };
    (outcome.code, String::from_utf8(out).unwrap())
}

#[cfg(unix)]
#[tokio::test]
async fn pair_prints_a_code_for_the_signed_in_user_and_leaves_the_bridge_its_primary() {
    let (stub, home) = (stub(&[]), tempdir("pair"));
    let (code, out) = pair(&["telegram"], &stub, &home).await;
    assert_eq!((code, out.trim()), (1, "Not signed in. Run ragon login, then ragon pair telegram."));
    run("login", &["--username", "ann", "--password-stdin"], &stub, &home, "right\n").await;
    let (code, out) = pair(&["telegram"], &stub, &home).await;
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Pairing code for telegram: ABC234") && out.contains("60 minutes"), "{out}");
    assert!(stub.seen.lock().unwrap().iter().any(|r| r.starts_with("POST /ragon/v1/bindings {\"chat_kind\":\"telegram\"} ") && r.ends_with("Bearer at")));
    let primary: Value = serde_json::from_str(&std::fs::read_to_string(home.join("chat/primary.json")).unwrap()).unwrap();
    assert_eq!((primary["gate"].as_str(), primary["node"].as_str()), (Some(stub.gate.as_str()), Some("box")));
    let (code, out) = pair(&["fax"], &stub, &home).await;
    assert_eq!(code, 2, "{out}");
}

/// A gate that answers 404 to everything: a node without accounts (0.9.8's default).
#[cfg(unix)]
fn no_accounts_gate() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let gate = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                line.clear();
            }
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    gate
}

/// Against a node without accounts, `ragon pair` names the 0.9.7 way to pair instead of a sign-in
/// that cannot happen; a gate that does not answer at all still gets the sign-in line.
#[cfg(unix)]
#[tokio::test]
async fn pair_against_a_node_without_accounts_names_the_bridge_pairing() {
    let home = tempdir("pair-off");
    let stub = Stub { gate: no_accounts_gate(), seen: Arc::new(Mutex::new(Vec::new())) };
    let (code, out) = pair(&["telegram"], &stub, &home).await;
    assert_eq!((code, out.trim()), (1, commands::no_accounts_pair_line("telegram").as_str()));
    assert!(out.contains("ragon chat telegram pair"), "{out}");
    let (_, out) = pair(&["discord"], &stub, &home).await;
    assert!(out.contains("ragon chat discord pair"), "{out}");
    let (_, out) = pair(&["matrix"], &stub, &home).await;
    assert!(out.contains("ragon-admin matrix add-user"), "{out}");
    assert!(!home.join("chat/primary.json").exists(), "nothing is written for the bridge");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let gate = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let (code, out) = pair(&["telegram"], &Stub { gate, seen: Arc::new(Mutex::new(Vec::new())) }, &home).await;
    assert_eq!((code, out.trim()), (1, "Not signed in. Run ragon login, then ragon pair telegram."));
}

#[cfg(not(unix))]
#[tokio::test]
async fn pair_says_it_is_not_available_on_windows_and_exits_two() {
    let (stub, home) = (stub(&[]), tempdir("pair"));
    let args = vec!["telegram".to_string()];
    let mut input = Cursor::new(Vec::new());
    let mut out = Vec::new();
    let outcome = {
        let mut io = Io { input: &mut input, out: &mut out, tty: false };
        commands::run_pair(&args, &stub.gate, &home, &mut io).await.unwrap()
    };
    let out = String::from_utf8(out).unwrap();
    assert_eq!(outcome.code, 2, "{out}");
    assert!(out.contains("not available on Windows"), "{out}");
}

/// `ragon login status --json` against a node that needs its first account, after it is made, and
/// in another language: the three states of the desktop app's account window
/// (docs/dev/desktop.md, "Accounts").
#[tokio::test]
async fn status_json_draws_the_desktop_account_window() {
    let (stub, home) = (stub(&[]), tempdir("status-json"));
    let (outcome, out) = run("login", &["status", "--json"], &stub, &home, "").await;
    assert_eq!(outcome.code, 0, "{out}");
    assert_eq!(out.lines().count(), 1, "one JSON line: {out}");
    let doc: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (&doc["reachable"], &doc["accounts"], &doc["setup"], &doc["local"], &doc["signedIn"], &doc["user"], &doc["apiKey"]),
        (&json!(true), &json!(true), &json!(true), &json!(true), &json!(false), &Value::Null, &json!(false))
    );
    assert_eq!((&doc["node"], &doc["primary"], &doc["error"]), (&json!("box"), &json!("main-box"), &Value::Null));

    let (made, _) = run("login", &["--create", "--username", "ann", "--password-stdin"], &stub, &home, "right\n").await;
    assert_eq!(made.code, 0);
    let doc = commands::status_json(&stub.gate, &home, Lang::Fr).await;
    assert_eq!((&doc["setup"], &doc["signedIn"], &doc["user"], &doc["role"]), (&json!(false), &json!(true), &json!("ann"), &json!("admin")));
    assert_eq!((&doc["lang"], &doc["text"]["SignOutAction"]), (&json!("fr"), &json!("Se déconnecter")));
    let text = doc["text"].as_object().unwrap();
    assert_eq!(text.len(), Key::ALL.len(), "the whole table, every key by name");
    assert!(!out.contains("right") && !doc.to_string().contains("right"), "no password in the document");
}

/// A gate that does not answer still gives a document and status 0; the desktop app draws "not
/// reachable" from it instead of failing.
#[tokio::test]
async fn status_json_of_an_unreachable_gate_is_still_a_document() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let stub = Stub { gate: format!("http://{}", listener.local_addr().unwrap()), seen: Arc::new(Mutex::new(Vec::new())) };
    drop(listener);
    let home = tempdir("status-json-down");
    let (outcome, out) = run("login", &["status", "--json"], &stub, &home, "").await;
    assert_eq!(outcome.code, 0, "{out}");
    let doc: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!((&doc["reachable"], &doc["accounts"], &doc["setup"], &doc["signedIn"]), (&json!(false), &json!(false), &json!(false), &json!(false)));
    assert_eq!(doc["lang"].as_str().map(str::len), Some(2));
}

/// A node without accounts (every route 404): `accounts` false, so the window shows nothing.
#[cfg(unix)]
#[tokio::test]
async fn status_json_of_a_node_without_accounts_says_so() {
    let home = tempdir("status-json-off");
    let doc = commands::status_json(&no_accounts_gate(), &home, Lang::En).await;
    assert_eq!((&doc["accounts"], &doc["setup"], &doc["signedIn"]), (&json!(false), &json!(false), &json!(false)));
}

/// The desktop app's tests draw the window from `tests/fixtures/account-text.json`: the string
/// table in every language. It must be the table itself; `RAGON_WRITE_FIXTURES=1` rewrites it.
#[test]
fn the_desktop_text_fixture_is_the_string_table() {
    let mut table = serde_json::Map::new();
    for lang in Lang::ALL {
        let text: serde_json::Map<String, Value> =
            Key::ALL.iter().map(|key| (format!("{key:?}"), Value::String(strings::text(lang, *key).to_string()))).collect();
        table.insert(lang.code().to_string(), Value::Object(text));
    }
    let want = serde_json::to_string_pretty(&Value::Object(table)).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/account-text.json");
    if std::env::var_os("RAGON_WRITE_FIXTURES").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &want).unwrap();
    }
    // Line endings are the checkout's business (`core.autocrlf` on Windows); the table has no raw CR.
    let have = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
    assert!(have == want, "{} is stale: run this test with RAGON_WRITE_FIXTURES=1", path.display());
}
