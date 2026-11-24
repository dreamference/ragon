//! A discovered node with accounts is used over HTTPS, and a gate's refusal of plain HTTP is told
//! as what it is (w48s F2; specs/DREAMFERENCE_AUTH.md §5.1, §6).
//!
//! The advertisement carries an address and a port and no scheme; the 0.9.9 gate refuses
//! `/ragon/v1/*`, `/oauth/*` and credentials over plain HTTP from another machine with 403
//! `https_required`. Before this, the launcher read that 403 as "no accounts (0.9.8 or older)" and
//! `ragon login` printed "answered 403".

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

use ragon_launcher::auth::{self, PlainAnswer, Secured, commands};

/// The gate's refusal of plain HTTP, byte for byte (model_gate_service.py, HTTPS_REQUIRED).
const REFUSAL: &str = r#"{"error":{"message":"Use https://","type":"invalid_request_error","code":"https_required","param":null}}"#;

/// Serves one fixed answer to every request on a loopback port; returns the gate URL.
fn serve(status: &str, body: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let reply = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-https-discovery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_refusal_is_recognised_and_nothing_else_is() {
    assert!(auth::https_required(403, REFUSAL));
    assert!(!auth::https_required(200, REFUSAL));
    assert!(!auth::https_required(403, r#"{"error":"forbidden"}"#));
    assert!(!auth::https_required(403, ""));
}

#[test]
fn a_plain_answer_is_one_of_three() {
    assert_eq!(auth::plain_answer(403, REFUSAL), PlainAnswer::HttpsRequired);
    assert_eq!(auth::plain_answer(200, r#"{"node_id":"n1","accounts":true}"#), PlainAnswer::Accounts);
    assert_eq!(auth::plain_answer(200, r#"{"node_id":"n1"}"#), PlainAnswer::NoAccounts);
    assert_eq!(auth::plain_answer(404, "{}"), PlainAnswer::NoAccounts);
}

#[tokio::test]
async fn a_refusal_of_plain_http_is_not_an_answer_about_accounts() {
    let gate = serve("403 Forbidden", REFUSAL);
    assert!(!auth::gate_has_accounts(&gate).await);
    // The chat bridge acts on a definite answer only (AUTH §14.1): a refusal is none.
    assert_eq!(auth::gate_accounts(&gate).await, None);
    let older = serve("404 Not Found", "{}");
    assert_eq!(auth::gate_accounts(&older).await, Some(false));
}

#[test]
fn the_https_gate_is_the_nodes_name_at_the_same_port() {
    assert_eq!(auth::https_gate_for("http://192.168.1.20:8000", "dgx-a").as_deref(), Some("https://dgx-a.local:8000"));
    assert_eq!(auth::https_gate_for("http://192.168.1.20:8000/", "dgx-a").as_deref(), Some("https://dgx-a.local:8000"));
    assert_eq!(auth::https_gate_for("http://[fe80::1]:9000", "dgx-a").as_deref(), Some("https://dgx-a.local:9000"));
    // A name stays: the certificate names the host name and `<name>.local`.
    assert_eq!(auth::https_gate_for("http://dgx-a.local:8000", "other").as_deref(), Some("https://dgx-a.local:8000"));
    // Only a host label becomes a host; an HTTPS host needs no move.
    assert_eq!(auth::https_gate_for("http://192.168.1.20:8000", "evil.example.com"), None);
    assert_eq!(auth::https_gate_for("http://192.168.1.20:8000", ""), None);
    assert_eq!(auth::https_gate_for("https://dgx-a.local:8000", "dgx-a"), None);
}

#[test]
fn host_labels() {
    assert!(auth::is_host_label("dgx-a"));
    assert!(auth::is_host_label("box7"));
    for bad in ["", "-a", "a-", "a.b", "a b", "a/b", &"x".repeat(64)] {
        assert!(!auth::is_host_label(bad), "{bad}");
    }
}

#[tokio::test]
async fn a_host_on_this_machine_stays_plain() {
    let gate = serve("403 Forbidden", REFUSAL);
    let home = home("loopback");
    assert_eq!(auth::secure_gate(&gate, &home, "n1", "box").await.unwrap(), Secured::AsIs);
    assert_eq!(auth::secure_gate("https://box.local:8000", &home, "n1", "box").await.unwrap(), Secured::AsIs);
}

/// 192.0.2.0/24 is TEST-NET-1 (RFC 5737): nothing answers there.
const NOWHERE: &str = "http://192.0.2.1:8000";

#[tokio::test]
async fn a_node_never_pinned_that_gives_no_tls_answer_stays_plain() {
    let home = home("unpinned");
    assert_eq!(auth::secure_gate(NOWHERE, &home, "n1", "box").await.unwrap(), Secured::AsIs);
}

#[tokio::test]
async fn a_pinned_node_is_never_used_over_plain_http() {
    let home = home("pinned");
    let pin = auth::pin::path(&home, "n1");
    std::fs::create_dir_all(pin.parent().unwrap()).unwrap();
    std::fs::write(&pin, "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n").unwrap();
    // Not answering (the node is starting): its HTTPS gate, by name, and the model wait waits there.
    assert_eq!(auth::secure_gate(NOWHERE, &home, "n1", "box").await.unwrap(), Secured::Https("https://box.local:8000".to_string()));
}

#[test]
fn the_lines_say_what_happened() {
    for line in [
        auth::https_required_line("http://192.168.1.20:8000"),
        auth::fallback_line("box", Some("https://box.local:8000")),
        auth::fallback_line("box", None),
        auth::no_downgrade_line("box", Some("https://box.local:8000")),
        commands::https_only_line("box", "http://192.168.1.20:8000"),
    ] {
        assert!(line.contains("https"), "{line}");
        assert!(!line.contains("0.9.8") && !line.contains("no accounts") && !line.contains("answered 403"), "{line}");
    }
    assert!(auth::fallback_line("box", Some("https://box.local:8000")).contains("box.local resolve"));
    assert!(auth::no_downgrade_line("box", None).contains("box.local resolve"));
}

#[tokio::test]
async fn a_sign_in_refused_over_plain_http_says_so() {
    let gate = serve("403 Forbidden", REFUSAL);
    let home = home("signin");
    let primary = commands::connect(&gate, &home).await.unwrap();
    let message = primary.sign_in("ada", "pw").await.unwrap().unwrap_err();
    assert!(message.contains("https only"), "{message}");
    assert!(!message.contains("answered 403"), "{message}");
}

/// B1: a pinned node whose advert gives no usable name is refused, never used over plain HTTP.
#[tokio::test]
async fn a_pinned_node_without_a_usable_name_is_refused_not_downgraded() {
    let home = home("pinned-noname");
    let pin = auth::pin::path(&home, "n1");
    std::fs::create_dir_all(pin.parent().unwrap()).unwrap();
    std::fs::write(&pin, "-----BEGIN CERTIFICATE-----\n-----END CERTIFICATE-----\n").unwrap();
    for name in ["", "a.b", "x_y", "192.0.2.1"] {
        let refused = auth::secure_gate(NOWHERE, &home, "n1", name).await;
        assert!(refused.is_err(), "name {name:?} gave {refused:?}");
    }
    // Nothing pinned under that id: the old plain route, without sign-in.
    assert_eq!(auth::secure_gate(NOWHERE, &home, "n2", "").await.unwrap(), Secured::AsIs);
}

/// B2: once the advertised id is pinned, only an answer with that id stands for the node.
#[test]
fn a_pinned_advert_is_answered_only_by_its_own_id() {
    assert!(auth::answers_as_advertised("n1", true, "n1"));
    assert!(!auth::answers_as_advertised("n1", true, "n2"));
    assert!(!auth::answers_as_advertised("n1", true, ""));
    // Never pinned: first contact, the answer's id is the one pinned.
    assert!(auth::answers_as_advertised("n1", false, "n2"));
    assert!(auth::answers_as_advertised("", false, "n2"));
}

/// B3: credentials never go to another machine over plain HTTP, and a plain "accounts" from
/// another machine gets no sign-in form.
#[test]
fn credentials_never_go_to_another_machine_over_plain_http() {
    assert!(auth::plain_to_another_machine("http://192.0.2.1:8000"));
    assert!(auth::plain_to_another_machine("HTTP://192.0.2.1:8000/"));
    assert!(auth::plain_to_another_machine("http://dgx-a.local:8000"));
    assert!(auth::plain_to_another_machine("http://localhost:1@192.0.2.1:8000"));
    assert!(!auth::plain_to_another_machine("https://192.0.2.1:8000"));
    assert!(!auth::plain_to_another_machine("http://127.0.0.1:8000"));
    assert!(!auth::plain_to_another_machine("http://localhost:8000"));
    assert!(!auth::plain_to_another_machine("http://[::1]:8000"));
    let remote = "http://192.0.2.1:8000";
    assert_eq!(auth::plain_answer_counted(remote, PlainAnswer::Accounts), PlainAnswer::HttpsRequired);
    assert_eq!(auth::plain_answer_counted(remote, PlainAnswer::NoAccounts), PlainAnswer::NoAccounts);
    assert_eq!(auth::plain_answer_counted("http://127.0.0.1:8000", PlainAnswer::Accounts), PlainAnswer::Accounts);
    assert_eq!(auth::plain_answer_counted("https://192.0.2.1:8000", PlainAnswer::Accounts), PlainAnswer::Accounts);
}
