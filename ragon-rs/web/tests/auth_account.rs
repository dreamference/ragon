//! The Account page and its proxy to the primary's accounts service (AUTH §12, §14.4): the page and
//! its script are served behind the device sign-in, and `/ragon/v1/*` and `/oauth/*` reach a stand-in
//! accounts service unchanged (method, path, query, body, `Authorization`), whose status and body
//! come back. No real network: the stand-in listens on an ephemeral loopback port.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use axum::http::header;
use ragon_web_server::auth;
use ragon_web_server::server;
use ragon_web_server::server::Config;
use ragon_web_server::server::Server;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Scratch {
    let dir = PathBuf::from("/tmp").join(format!("lw-acct-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

fn config(dir: &Path, accounts_url: Option<String>) -> Config {
    Config { accounts_url, ..Config::for_test(dir, dir.join("home"), 3100) }
}

const HOST: &str = "127.0.0.1:3100";
const ORIGIN: &str = "http://127.0.0.1:3100";

async fn call(server: &Arc<Server>, request: Request<Body>) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = server::router(server.clone()).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, headers, String::from_utf8_lossy(&body).into_owned())
}

async fn cookie(server: &Arc<Server>) -> String {
    let code = auth::issue_login_code(&server.config.state_dir).unwrap();
    let login = Request::builder().uri(format!("/login?code={code}")).header(header::HOST, HOST);
    let (status, headers, _) = call(server, login.body(Body::empty()).unwrap()).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    headers.get(header::SET_COOKIE).unwrap().to_str().unwrap().split(';').next().unwrap().to_string()
}

fn get(path: &str, cookie: &str) -> Request<Body> {
    Request::builder().uri(path).header(header::HOST, HOST).header(header::COOKIE, cookie).body(Body::empty()).unwrap()
}

/// A stand-in accounts service: answers every request with 201 and a fixed body, and records each
/// request's head and body.
async fn stand_in() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { return };
            let log = log.clone();
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let Ok(n) = stream.read(&mut chunk).await else { return };
                    if n == 0 {
                        return;
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buffer).to_string();
                    if let Some(split) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buffer.len() >= split + 4 + length {
                            log.lock().unwrap().push(text);
                            break;
                        }
                    }
                }
                let body = r#"{"key":"sk-ragon-test"}"#;
                let answer = format!("HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(answer.as_bytes()).await;
            });
        }
    });
    (url, seen)
}

#[tokio::test]
async fn the_account_page_and_its_script_need_the_device_sign_in_and_carry_every_section() {
    let scratch = scratch("page");
    let server = Arc::new(Server::new(config(&scratch.0, None)).unwrap());
    let anonymous = Request::builder().uri("/account").header(header::HOST, HOST).body(Body::empty()).unwrap();
    assert_eq!(call(&server, anonymous).await.0, StatusCode::UNAUTHORIZED);
    let cookie = cookie(&server).await;
    let (status, headers, body) = call(&server, get("/account", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
    for needed in ["id=\"identity-rows\"", "id=\"sessions\"", "id=\"revoke-all\"", "id=\"keys\"", "id=\"bindings\"", "id=\"pair-phone\"", "id=\"usage\"", "id=\"users\"", "id=\"snippet-config\""] {
        assert!(body.contains(needed), "missing {needed}");
    }
    assert!(!body.contains("<script>"), "scripts are external: the page's policy allows 'self' only");
    let (status, headers, script) = call(&server, get("/account.js", &cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/javascript"));
    for path in ["/ragon/v1/users/me/sessions", "/ragon/v1/users/me/keys", "/ragon/v1/bindings", "/ragon/v1/usage", "/ragon/v1/users", "/reset", "/promote", "/demote"] {
        assert!(script.contains(path), "the script never calls {path}");
    }
    assert!(!script.contains("innerHTML"));
}

#[tokio::test]
async fn the_proxy_passes_the_request_on_unchanged_and_returns_the_answer() {
    let scratch = scratch("proxy");
    let (url, seen) = stand_in().await;
    let server = Arc::new(Server::new(config(&scratch.0, Some(url))).unwrap());
    let cookie = cookie(&server).await;
    let request = Request::builder()
        .method("POST")
        .uri("/ragon/v1/users/me/keys?x=1")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::COOKIE, &cookie)
        .header(header::AUTHORIZATION, "Bearer abc")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(r#"{"name":"laptop"}"#))
        .unwrap();
    let (status, headers, body) = call(&server, request).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(body, r#"{"key":"sk-ragon-test"}"#);
    let sent = seen.lock().unwrap().join("\n").to_ascii_lowercase();
    assert!(sent.starts_with("post /ragon/v1/users/me/keys?x=1 http/1.1"), "{sent}");
    assert!(sent.contains("authorization: bearer abc") && sent.ends_with(r#"{"name":"laptop"}"#), "{sent}");
    assert!(!sent.contains("cookie"), "the device cookie never leaves this server: {sent}");
}

#[tokio::test]
async fn the_proxy_names_the_browser_for_the_gate_and_never_passes_a_mark_the_browser_sent() {
    let scratch = scratch("peer");
    let (url, seen) = stand_in().await;
    let server = Arc::new(Server::new(config(&scratch.0, Some(url))).unwrap());
    let cookie = cookie(&server).await;
    let phone: std::net::SocketAddr = "192.168.1.30:50000".parse().unwrap();
    for (peer, named) in [(Some(phone), "x-ragon-peer: 192.168.1.30"), (None, "x-ragon-peer: unknown")] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/oauth/token")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::COOKIE, &cookie)
            .header("x-ragon-local", "1")
            .header("x-ragon-peer", "127.0.0.1")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .unwrap();
        if let Some(address) = peer {
            request.extensions_mut().insert(axum::extract::ConnectInfo(address));
        }
        assert_eq!(call(&server, request).await.0, StatusCode::CREATED);
        let sent = seen.lock().unwrap().pop().unwrap().to_ascii_lowercase();
        assert!(sent.contains(named), "{sent}");
        assert_eq!(sent.matches("x-ragon-peer").count(), 1, "{sent}");
        assert!(!sent.contains("x-ragon-local"), "{sent}");
    }
}

#[tokio::test]
async fn the_proxy_refuses_without_the_sign_in_and_reports_an_unreachable_service() {
    let scratch = scratch("down");
    // Nothing listens on the discard port.
    let server = Arc::new(Server::new(config(&scratch.0, Some("http://127.0.0.1:9".to_string()))).unwrap());
    let anonymous = Request::builder().uri("/ragon/v1/users/me").header(header::HOST, HOST).body(Body::empty()).unwrap();
    assert_eq!(call(&server, anonymous).await.0, StatusCode::UNAUTHORIZED);
    let cookie = cookie(&server).await;
    let (status, _, body) = call(&server, get("/ragon/v1/users/me", &cookie)).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body.contains("accounts_unavailable"), "{body}");
    let https = Arc::new(Server::new(config(&scratch.0, Some("https://node:8000".to_string()))).unwrap());
    let cookie = self::cookie(&https).await;
    let (status, _, _) = call(&https, get("/oauth/token", &cookie)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
