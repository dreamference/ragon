//! The Account page and its way to the primary (specs/DREAMFERENCE_AUTH.md §12, §14.4).
//!
//! `/account` is a static page and `/account.js` its script: identity, sessions, API keys, phone
//! bindings and usage for everyone, and the users list for an admin. The page keeps the user's JWT
//! in the browser's local storage (`/oauth/token`, the password grant) and calls the primary's
//! `/ragon/v1/*` routes through this server, because the page's policy lets it talk to its own
//! origin only. The proxy changes nothing: method, path, query, body, `Authorization` and
//! `Content-Type` go to the accounts service, and its status, body and content type come back, so
//! every rule (who may call what, the last-admin guard) stays the service's. Plain HTTP to the
//! primary's gate only, as the forge proxy. The one header it adds is `X-Ragon-Peer`, the browser's
//! address: this server reaches the gate from loopback, and the gate must not take a phone's request
//! for one made on the node (the first account is made on the node only, AUTH §8.6).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::IntoResponse;
use axum::response::Response;
use hyper_util::rt::TokioIo;

use crate::assets;
use crate::server::Server;

/// Where the primary's gate answers when nothing else is configured.
pub const DEFAULT_ACCOUNTS_URL: &str = "http://127.0.0.1:8000";
/// The largest request body passed on: a password change or a key name is tiny.
const MAX_REQUEST: usize = 64 * 1024;
/// The largest answer passed back: a usage listing over many days.
const MAX_ANSWER: usize = 4 * 1024 * 1024;
/// Names the browser's address for the gate (`model_gate_service.py`, `RELAY_HEADER`).
pub const RELAY_HEADER: &str = "x-ragon-peer";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const ANSWER_TIMEOUT: Duration = Duration::from_secs(20);

/// Where the accounts service answers: host and port of the configured URL (plain HTTP only).
pub fn upstream(accounts_url: Option<&str>) -> Option<String> {
    let url = accounts_url.unwrap_or(DEFAULT_ACCOUNTS_URL).trim().trim_end_matches('/');
    let authority = url.strip_prefix("http://")?;
    (!authority.is_empty() && !authority.contains('/') && !authority.contains('@')).then(|| authority.to_string())
}

fn secured_html(body: &'static str, content_type: &'static str) -> Response {
    (StatusCode::OK, [(header::CONTENT_TYPE, content_type)], body).into_response()
}

/// `GET /account`.
pub async fn page() -> Response {
    secured_html(assets::ACCOUNT_HTML, "text/html; charset=utf-8")
}

/// `GET /account.js`.
pub async fn script() -> Response {
    secured_html(assets::ACCOUNT_JS, "text/javascript; charset=utf-8")
}

fn failure(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": { "code": "accounts_unavailable", "message": message } });
    (status, [(header::CONTENT_TYPE, "application/json")], body.to_string()).into_response()
}

/// Passes `/oauth/token`, `/oauth/revoke` and `/ragon/v1/*` to the accounts service.
pub async fn proxy(State(server): State<Arc<Server>>, request: Request) -> Response {
    let Some(authority) = upstream(server.config.accounts_url.as_deref()) else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "The accounts service address is not a plain http URL.");
    };
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_REQUEST).await else {
        return failure(StatusCode::PAYLOAD_TOO_LARGE, "The request is too large.");
    };
    let target = parts.uri.path_and_query().map(|value| value.as_str().to_string()).unwrap_or_else(|| parts.uri.path().to_string());
    let mut outgoing = axum::http::Request::builder().method(parts.method.clone()).uri(target).header(header::HOST, authority.as_str());
    for name in [header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT] {
        if let Some(value) = parts.headers.get(&name) {
            outgoing = outgoing.header(name, value);
        }
    }
    // No address known (a router driven without a listener): a browser that is not on the node.
    let peer = parts.extensions.get::<ConnectInfo<SocketAddr>>().map_or_else(|| "unknown".to_string(), |info| info.0.ip().to_string());
    outgoing = outgoing.header(RELAY_HEADER, peer);
    let Ok(outgoing) = outgoing.header(header::CONTENT_LENGTH, bytes.len()).body(Body::from(bytes)) else {
        return failure(StatusCode::BAD_REQUEST, "The request could not be passed on.");
    };
    let sent = tokio::time::timeout(ANSWER_TIMEOUT, send(&authority, outgoing)).await;
    match sent {
        Err(_) => failure(StatusCode::GATEWAY_TIMEOUT, "The accounts service did not answer in time."),
        Ok(Err(reason)) => failure(StatusCode::BAD_GATEWAY, &reason),
        Ok(Ok(response)) => {
            let status = response.status();
            let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
            let Ok(collected) = axum::body::to_bytes(Body::new(response.into_body()), MAX_ANSWER).await else {
                return failure(StatusCode::BAD_GATEWAY, "The accounts service's answer could not be read.");
            };
            let mut answer = (status, collected).into_response();
            if let Some(value) = content_type {
                answer.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            answer
        }
    }
}

async fn send(authority: &str, request: axum::http::Request<Body>) -> Result<axum::http::Response<hyper::body::Incoming>, String> {
    let stream = tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::TcpStream::connect(authority))
        .await
        .map_err(|_| "The accounts service did not answer in time.".to_string())?
        .map_err(|_| "The accounts service is not answering.".to_string())?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| "The accounts service is not answering.".to_string())?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender.send_request(request).await.map_err(|_| "The accounts service closed the connection.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_takes_plain_http_only() {
        assert_eq!(upstream(None).as_deref(), Some("127.0.0.1:8000"));
        assert_eq!(upstream(Some("http://10.0.0.1:9/")).as_deref(), Some("10.0.0.1:9"));
        assert_eq!(upstream(Some("https://node:8000")), None);
        assert_eq!(upstream(Some("http://a@b")), None);
    }
}
