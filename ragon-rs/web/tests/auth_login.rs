//! The browser sign-in of `ragon web` (specs/DREAMFERENCE_AUTH.md §5.8, §14.4) against a stand-in
//! for the accounts service on an ephemeral loopback port: the login page and its calls, the
//! Bearer check on `/ws` and `/api/transcribe`, the device approval, and the hand-over of the
//! session's tokens to the app-server with their refresh.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header;
use ragon_web_server::server;
use ragon_web_server::server::Config;
use ragon_web_server::server::Server;
use ragon_web_server::web_login::Login;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tower::ServiceExt;

const HOST: &str = "127.0.0.1:3100";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The accounts service: `at1` and `at2` are valid access tokens of `ann`, a refresh gives `at2`.
async fn accounts() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).into_owned();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text.lines().find_map(|l| l.strip_prefix("Content-Length: ")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                        if raw.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&raw).into_owned();
                let first = text.lines().next().unwrap_or_default().to_string();
                let body = text.split("\r\n\r\n").nth(1).unwrap_or_default();
                let bearer_ok = text.contains("Authorization: Bearer at1") || text.contains("Authorization: Bearer at2");
                let (status, answer) = if first.starts_with("POST /oauth/token") {
                    if body.contains("grant_type=password") && body.contains("password=pw") {
                        (200, json!({ "access_token": "at1", "refresh_token": "rt1" }))
                    } else if body.contains("grant_type=refresh_token") && body.contains("refresh_token=rt1") {
                        (200, json!({ "access_token": "at2", "refresh_token": "rt2" }))
                    } else {
                        (400, json!({ "error": "invalid_grant", "error_description": "Wrong name or password." }))
                    }
                } else if first.starts_with("GET /ragon/v1/users/me") && bearer_ok {
                    (200, json!({ "username": "ann", "user_id": 7, "role": "admin" }))
                } else if first.starts_with("POST /oauth/device/approve") && bearer_ok {
                    (200, json!({ "approved": body.contains("ABCD1234") }))
                } else {
                    (401, json!({ "error": "invalid_token" }))
                };
                let payload = answer.to_string();
                let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}", payload.len());
                let _ = stream.write_all(reply.as_bytes()).await;
            });
        }
    });
    addr
}

async fn server_for(tag: &str) -> (Arc<Server>, Scratch) {
    let dir = PathBuf::from("/tmp").join(format!("al-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config = Config { accounts_addr: accounts().await, ..Config::for_test(&dir, dir.join("home"), 3100) };
    (Server::new(config).unwrap(), Scratch(dir))
}

fn req(method: &str, uri: &str) -> axum::http::request::Builder {
    Request::builder().method(method).uri(uri).header(header::HOST, HOST).header(header::ORIGIN, format!("http://{HOST}"))
}

async fn call(server: &Arc<Server>, request: Request<Body>) -> (StatusCode, String) {
    let response = server::router(server.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&body).into_owned())
}

fn post_json(uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = req("POST", uri).header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn the_login_pages_open_without_a_credential() {
    let (server, _dir) = server_for("pages").await;
    for path in ["/signin", "/device", "/oauth/authorize?client_id=forgejo"] {
        let (status, body) = call(&server, req("GET", path).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains("/signin.js"), "{path}");
    }
    let (status, _) = call(&server, req("GET", "/signin.js").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    let response = server::router(server.clone()).oneshot(req("GET", "/codex/device").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(response.headers()[header::LOCATION], "/device");
}

#[tokio::test]
async fn the_page_signs_in_with_the_password_grant() {
    let (server, _dir) = server_for("login").await;
    let (status, body) = call(&server, post_json("/api/auth/login", None, json!({ "username": "ann", "password": "pw" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["access_token"], "at1");
    let (status, body) = call(&server, post_json("/api/auth/login", None, json!({ "username": "ann", "password": "no" }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Wrong name or password."), "{body}");
    let (status, body) = call(&server, post_json("/api/auth/refresh", None, json!({ "refresh_token": "rt1" }))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("at2"), "{body}");
}

#[tokio::test]
async fn the_socket_and_the_voice_upload_take_the_jwt_as_bearer() {
    let (server, _dir) = server_for("bearer").await;
    for path in ["/ws", "/api/transcribe"] {
        let method = if path == "/ws" { "GET" } else { "POST" };
        let (status, _) = call(&server, req(method, path).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} without a credential");
        let (status, _) = call(&server, req(method, path).header(header::AUTHORIZATION, "Bearer nope").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} with a bad token");
    }
    // Past the check, the voice route refuses the missing audio type: not a 401.
    let (status, _) = call(&server, req("POST", "/api/transcribe").header(header::AUTHORIZATION, "Bearer at1").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // The JWT opens nothing else.
    let (status, _) = call(&server, req("GET", "/api/airgapped").header(header::AUTHORIZATION, "Bearer at1").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_device_code_is_approved_only_for_a_signed_in_user() {
    let (server, _dir) = server_for("device").await;
    let (status, _) = call(&server, post_json("/api/auth/device", None, json!({ "user_code": "ABCD1234" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(&server, post_json("/api/auth/device", Some("at1"), json!({ "user_code": "ABCD1234" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.contains("true"), "{body}");
}

#[tokio::test]
async fn the_session_is_handed_to_the_app_server_and_refreshed_for_it() {
    let (server, _dir) = server_for("session").await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    server.login.register(1, tx);
    let (status, body) = call(&server, post_json("/api/auth/session", Some("at1"), json!({ "refresh_token": "rt1" }))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sent: Value = serde_json::from_str(rx.recv().await.unwrap().to_text().unwrap()).unwrap();
    assert_eq!(sent["method"], "account/login/start");
    assert_eq!(sent["params"]["type"], "chatgptAuthTokens");
    assert_eq!(sent["params"]["accessToken"], "at1");
    assert_eq!(sent["params"]["chatgptAccountId"], "7");
    assert_eq!(sent["params"]["chatgptPlanType"], "admin");
    // The app-server asks for new tokens: the answer comes from a refresh at the primary.
    let answer = server.login.refresh_answer(json!(41)).await;
    assert_eq!(answer["id"], 41);
    assert_eq!(answer["result"]["accessToken"], "at2");
    // A second request uses the rotated refresh token, which the stand-in no longer knows.
    let answer = server.login.refresh_answer(json!(42)).await;
    assert!(answer["error"].is_object(), "{answer}");
    // Logout forgets the session; a later refresh has nothing to refresh.
    let (status, _) = call(&server, req("DELETE", "/api/auth/session").header(header::AUTHORIZATION, "Bearer at1").body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!server.login.has_session());
}

#[tokio::test]
async fn a_token_the_service_does_not_know_is_not_handed_over() {
    let (server, _dir) = server_for("forged").await;
    let (status, _) = call(&server, post_json("/api/auth/session", Some("forged"), json!({ "refresh_token": "x" }))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!server.login.has_session());
    let login = Login::new("127.0.0.1:9");
    assert!(login.verify("at1").await.is_none(), "an unreachable service opens nothing");
}
