//! The launcher against a gate with and without an accounts service (specs/DREAMFERENCE_AUTH.md §7, §15).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;

use ragon_launcher::auth;

/// Serves one fixed answer to every request on a loopback port; returns the gate URL.
fn serve(status: &str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn provider_needs_sign_in(accounts: bool) -> Option<bool> {
    let text = ragon_launcher::updated_config_for("", Path::new("/h/c.json"), "http://x:8000", accounts).unwrap();
    let parsed: toml::Table = toml::from_str(&text).unwrap();
    parsed["model_providers"]["ragon"].get("requires_openai_auth").and_then(toml::Value::as_bool)
}

#[tokio::test]
async fn info_with_accounts_is_account_mode() {
    let gate = serve("200 OK", r#"{"node_id":"n1","node_name":"Box","accounts":true}"#);
    assert!(auth::gate_has_accounts(&gate).await);
    assert_eq!(provider_needs_sign_in(true), Some(true));
}

#[tokio::test]
async fn info_without_accounts_is_unauthenticated() {
    let gate = serve("200 OK", r#"{"node_id":"n1","node_name":"Box"}"#);
    assert!(!auth::gate_has_accounts(&gate).await);
    assert_eq!(provider_needs_sign_in(false), None);
}

#[tokio::test]
async fn a_404_is_unauthenticated() {
    let gate = serve("404 Not Found", "{}");
    assert!(!auth::gate_has_accounts(&gate).await);
}

#[tokio::test]
async fn a_refused_connection_is_unauthenticated() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(!auth::gate_has_accounts(&format!("http://127.0.0.1:{port}")).await);
}

#[test]
fn the_line_names_the_node_and_no_vendor() {
    let line = auth::no_accounts_line("https://box.local:8000");
    assert_eq!(line, "node box.local has no accounts yet (0.9.8 or older); running without sign-in");
    let lower = line.to_lowercase();
    assert!(!lower.contains("openai") && !lower.contains("chatgpt") && !lower.contains("codex"));
}

#[test]
fn the_setup_document_says_whether_the_first_account_is_open_and_who_may_make_it() {
    assert_eq!(auth::parse_setup(r#"{"setup":true,"local":true,"node":"box"}"#), Some(auth::Setup { open: true, local: true }));
    assert_eq!(auth::parse_setup(r#"{"setup":false,"node":"box"}"#), Some(auth::Setup { open: false, local: false }));
    assert_eq!(auth::parse_setup(r#"{"error":"not_found"}"#), None);
    assert_eq!(auth::parse_setup("<html>"), None);
    assert_eq!(auth::parse_info(r#"{"node_id":"n1","name":"box"}"#).unwrap().node_name, "box");
}

#[test]
fn only_an_interactive_start_on_a_node_with_no_account_is_asked_about_its_first() {
    use auth::FirstRun;
    let open_here = auth::Setup { open: true, local: true };
    let open_elsewhere = auth::Setup { open: true, local: false };
    let done = auth::Setup { open: false, local: true };
    assert_eq!(auth::first_run(true, Some(&open_here)), FirstRun::Create);
    assert_eq!(auth::first_run(true, Some(&open_elsewhere)), FirstRun::Elsewhere);
    assert_eq!(auth::first_run(true, Some(&done)), FirstRun::SignIn);
    assert_eq!(auth::first_run(true, None), FirstRun::SignIn);
    // Unattended starts (`exec`, Night Shift, SWE-bench, CI) never see the setup.
    for setup in [Some(&open_here), Some(&open_elsewhere), Some(&done), None] {
        assert_eq!(auth::first_run(false, setup), FirstRun::SignIn);
    }
}

const NO_ACCOUNT_YET_ELSEWHERE: &str = r#"{"node_id":"n1","name":"box","accounts":true,"setup":true,"local":false,"node":"box"}"#;

#[tokio::test]
async fn an_interactive_start_against_another_node_with_no_account_runs_without_sign_in() {
    let gate = serve("200 OK", NO_ACCOUNT_YET_ELSEWHERE);
    let dir = std::env::temp_dir().join(format!("ragon-first-remote-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(!auth::start(&gate, &dir, true).await.unwrap(), "no form that the node would refuse");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_unattended_start_against_a_node_with_no_account_is_unchanged() {
    let gate = serve("200 OK", NO_ACCOUNT_YET_ELSEWHERE);
    let dir = std::env::temp_dir().join(format!("ragon-first-unattended-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(auth::start(&gate, &dir, false).await.unwrap(), "as before the setup existed: the node is in account mode");
    let _ = std::fs::remove_dir_all(&dir);
}
