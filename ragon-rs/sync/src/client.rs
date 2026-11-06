//! The agent's side of the hub's routes (`ragon_web_server::sync_hub`): plain HTTP to the node's
//! `ragon web` port, the device secret as a bearer credential, and the server's own address as the
//! `Origin` its guard asks of every change. No proxy is ever used: the hub is the user's own node.

use std::time::Duration;

use serde_json::Value;
use serde_json::json;

/// What an upload came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Uploaded {
    Taken { length: u64, hash: String },
    /// The hub holds something else at that offset: here is its length and hash.
    Mismatch { length: u64, hash: String },
    /// Another device owns the thread: publish under these ids.
    Owned { fork: String, thread: String },
    /// The thread was deleted.
    Gone,
}

/// What a request for the hub says besides the bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UploadParams {
    pub offset: u64,
    pub prefix: String,
    pub thread: Option<String>,
    pub file: Option<String>,
    pub archived: bool,
    pub fork_of: Option<String>,
}

pub struct HubClient {
    base: String,
    secret: String,
    http: reqwest::Client,
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(timeout)
        .build()
        .map_err(|err| format!("could not make an HTTP client: {err}"))
}

/// The text of a refusal, from its JSON `error` when it has one.
async fn refusal(response: reqwest::Response) -> String {
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let said = serde_json::from_str::<Value>(&text).ok().and_then(|value| value.get("error").and_then(Value::as_str).map(str::to_string));
    format!("the hub answered {status}: {}", said.unwrap_or_else(|| text.trim().to_string()))
}

/// Pairs this device with the hub at `base` (`http://host:port`) using a code from `ragon web pair`.
/// Returns the device id and its secret.
pub async fn pair(base: &str, code: &str, name: &str) -> Result<(String, String), String> {
    let response = client(Duration::from_secs(30))?
        .post(format!("{base}/api/sync/pair"))
        .header("Origin", base)
        .json(&json!({ "code": code, "name": name }))
        .send()
        .await
        .map_err(|err| format!("could not reach the node's ragon web at {base}: {err}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("the node's hub is off: run `ragon sync enable` on the node first".to_string());
    }
    if !response.status().is_success() {
        return Err(refusal(response).await);
    }
    let value: Value = response.json().await.map_err(|err| err.to_string())?;
    let device = value.get("device").and_then(Value::as_str).ok_or("the hub gave no device id")?;
    let secret = value.get("secret").and_then(Value::as_str).ok_or("the hub gave no secret")?;
    Ok((device.to_string(), secret.to_string()))
}

impl HubClient {
    pub fn new(base: &str, secret: &str) -> Result<HubClient, String> {
        Ok(HubClient { base: base.trim_end_matches('/').to_string(), secret: secret.to_string(), http: client(Duration::from_secs(60))? })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http.request(method, format!("{}{path}", self.base)).bearer_auth(&self.secret).header("Origin", &self.base)
    }

    async fn send(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
        builder.send().await.map_err(|err| format!("could not reach the hub at {}: {err}", self.base))
    }

    pub async fn status(&self) -> Result<Value, String> {
        let response = self.send(self.request(reqwest::Method::GET, "/api/sync/status")).await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        response.json().await.map_err(|err| err.to_string())
    }

    /// Every change after `since`; with `wait`, held by the hub until there is one.
    pub async fn manifest(&self, since: u64, wait: u64) -> Result<Value, String> {
        let response = self.send(self.request(reqwest::Method::GET, &format!("/api/sync/manifest?since={since}&wait={wait}"))).await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        response.json().await.map_err(|err| err.to_string())
    }

    pub async fn upload(&self, rollout: &str, params: &UploadParams, bytes: Vec<u8>) -> Result<Uploaded, String> {
        let mut query: Vec<(&str, String)> = vec![("offset", params.offset.to_string()), ("prefix", params.prefix.clone())];
        if let Some(thread) = &params.thread {
            query.push(("thread", thread.clone()));
        }
        if let Some(file) = &params.file {
            query.push(("file", file.clone()));
        }
        if params.archived {
            query.push(("archived", "1".to_string()));
        }
        if let Some(fork_of) = &params.fork_of {
            query.push(("fork_of", fork_of.clone()));
        }
        let builder = self.request(reqwest::Method::PUT, &format!("/api/sync/rollouts/{rollout}")).query(&query).body(bytes);
        let response = self.send(builder).await?;
        let status = response.status();
        if status == reqwest::StatusCode::GONE {
            return Ok(Uploaded::Gone);
        }
        if !status.is_success() && status != reqwest::StatusCode::CONFLICT {
            return Err(refusal(response).await);
        }
        let value: Value = response.json().await.map_err(|err| err.to_string())?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let length = value.get("length").and_then(Value::as_u64).unwrap_or(0);
        Ok(match (status.is_success(), value.get("error").and_then(Value::as_str)) {
            (true, _) => Uploaded::Taken { length, hash: text("hash") },
            (false, Some("owned")) => Uploaded::Owned { fork: text("fork"), thread: text("thread") },
            _ => Uploaded::Mismatch { length, hash: text("hash") },
        })
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        let response = self.send(self.request(reqwest::Method::POST, path).json(&body)).await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        response.json().await.map_err(|err| err.to_string())
    }

    /// A thread's archive state or name, with this device's clock reading.
    pub async fn set_meta(&self, thread: &str, body: Value) -> Result<Value, String> {
        self.post(&format!("/api/sync/threads/{thread}/meta"), body).await
    }

    /// Deletes a thread everywhere.
    pub async fn delete(&self, thread: &str) -> Result<Value, String> {
        self.post(&format!("/api/sync/threads/{thread}/delete"), json!({})).await
    }

    /// Takes this device's thread off the hub, without deleting it anywhere (`[sync] exclude`).
    pub async fn withdraw(&self, thread: &str) -> Result<Value, String> {
        self.post(&format!("/api/sync/threads/{thread}/withdraw"), json!({})).await
    }

    /// A host's post (`/api/sync/host`, SYNC §18.2): its events and acks; the answer carries the
    /// commands for it. With `wait`, held by the hub until a command comes.
    pub async fn host(&self, body: Value, wait: u64) -> Result<Value, String> {
        let builder = self.request(reqwest::Method::POST, &format!("/api/sync/host?wait={wait}")).json(&body);
        let response = self.send(builder).await?;
        if !response.status().is_success() {
            return Err(refusal(response).await);
        }
        response.json().await.map_err(|err| err.to_string())
    }

    /// Ends this device's credential on the hub (`ragon sync forget`).
    pub async fn forget(&self) -> Result<Value, String> {
        self.post("/api/sync/forget", json!({})).await
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    const SECRET: &str = "not-a-secret";

    /// A stand-in hub on loopback: one canned answer, resolving to the request it was asked.
    async fn stand_in(status: &str, body: &str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let answer = format!("HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        let served = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = socket.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some((head, sent)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| line.split_once(':').filter(|(name, _)| name.eq_ignore_ascii_case("content-length")).and_then(|(_, value)| value.trim().parse::<usize>().ok()))
                        .unwrap_or(0);
                    if sent.len() >= length {
                        break;
                    }
                }
            }
            socket.write_all(answer.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (base, served)
    }

    fn first_line(request: &str) -> &str {
        request.lines().next().unwrap_or_default()
    }

    /// An upload with the default parameters against one canned answer.
    async fn upload_answered(status: &str, body: &str) -> Result<Uploaded, String> {
        let (base, served) = stand_in(status, body).await;
        let outcome = HubClient::new(&base, SECRET).unwrap().upload("r1", &UploadParams::default(), b"line\n".to_vec()).await;
        served.await.unwrap();
        outcome
    }

    #[test]
    fn a_hub_address_loses_its_trailing_slashes() {
        assert_eq!(HubClient::new("http://node:3100/", SECRET).unwrap().base(), "http://node:3100");
        assert_eq!(HubClient::new("http://node:3100//", SECRET).unwrap().base(), "http://node:3100");
        assert_eq!(HubClient::new("http://node:3100", SECRET).unwrap().base(), "http://node:3100");
    }

    #[tokio::test]
    async fn a_request_carries_the_secret_and_the_origin_the_guard_asks_for() {
        let (base, served) = stand_in("200 OK", r#"{"cursor":9}"#).await;
        let answer = HubClient::new(&format!("{base}/"), SECRET).unwrap().manifest(5, 30).await.unwrap();
        assert_eq!(answer["cursor"], 9);

        let request = served.await.unwrap();
        assert_eq!(first_line(&request), "GET /api/sync/manifest?since=5&wait=30 HTTP/1.1");
        let lower = request.to_ascii_lowercase();
        assert!(lower.contains(&format!("authorization: bearer {SECRET}")), "{request}");
        assert!(lower.contains(&format!("origin: {base}")), "{request}");
    }

    #[tokio::test]
    async fn an_upload_answer_says_what_became_of_the_bytes() {
        assert_eq!(upload_answered("200 OK", r#"{"length":12,"hash":"h1"}"#).await, Ok(Uploaded::Taken { length: 12, hash: "h1".to_string() }));
        assert_eq!(upload_answered("409 Conflict", r#"{"length":4,"hash":"h2"}"#).await, Ok(Uploaded::Mismatch { length: 4, hash: "h2".to_string() }));
        assert_eq!(
            upload_answered("409 Conflict", r#"{"error":"owned","fork":"f1","thread":"t1"}"#).await,
            Ok(Uploaded::Owned { fork: "f1".to_string(), thread: "t1".to_string() })
        );
        assert_eq!(upload_answered("410 Gone", "").await, Ok(Uploaded::Gone));
    }

    #[tokio::test]
    async fn a_refusal_quotes_the_hub_error_or_its_plain_text() {
        let quoted = upload_answered("403 Forbidden", r#"{"error":"revoked"}"#).await.unwrap_err();
        assert!(quoted.contains("403") && quoted.ends_with(": revoked"), "{quoted}");
        let plain = upload_answered("500 Internal Server Error", "  it broke \n").await.unwrap_err();
        assert!(plain.contains("500") && plain.ends_with(": it broke"), "{plain}");
    }

    #[tokio::test]
    async fn an_upload_says_only_the_parameters_it_has() {
        let (base, served) = stand_in("200 OK", "{}").await;
        let params = UploadParams { offset: 3, prefix: "abc".to_string(), ..UploadParams::default() };
        HubClient::new(&base, SECRET).unwrap().upload("r1", &params, b"line\n".to_vec()).await.unwrap();
        let request = served.await.unwrap();
        assert_eq!(first_line(&request), "PUT /api/sync/rollouts/r1?offset=3&prefix=abc HTTP/1.1");
        assert!(request.ends_with("\r\n\r\nline\n"), "{request}");

        let (base, served) = stand_in("200 OK", "{}").await;
        let params = UploadParams {
            offset: 0,
            prefix: String::new(),
            thread: Some("t1".to_string()),
            file: Some("rollout.jsonl".to_string()),
            archived: true,
            fork_of: Some("r0".to_string()),
        };
        HubClient::new(&base, SECRET).unwrap().upload("r1", &params, Vec::new()).await.unwrap();
        let request = served.await.unwrap();
        assert_eq!(first_line(&request), "PUT /api/sync/rollouts/r1?offset=0&prefix=&thread=t1&file=rollout.jsonl&archived=1&fork_of=r0 HTTP/1.1");
    }

    #[tokio::test]
    async fn pairing_returns_the_device_and_its_secret() {
        let (base, served) = stand_in("200 OK", r#"{"device":"d1","secret":"s1"}"#).await;
        assert_eq!(pair(&base, "CODE-1", "laptop").await, Ok(("d1".to_string(), "s1".to_string())));
        let request = served.await.unwrap();
        assert_eq!(first_line(&request), "POST /api/sync/pair HTTP/1.1");
        assert!(request.to_ascii_lowercase().contains(&format!("origin: {base}")), "{request}");
        let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body, json!({ "code": "CODE-1", "name": "laptop" }));
    }

    #[tokio::test]
    async fn pairing_says_why_it_failed() {
        let (base, served) = stand_in("404 Not Found", "").await;
        assert!(pair(&base, "c", "n").await.unwrap_err().contains("ragon sync enable"));
        served.await.unwrap();

        let (base, served) = stand_in("403 Forbidden", r#"{"error":"bad code"}"#).await;
        assert!(pair(&base, "c", "n").await.unwrap_err().ends_with(": bad code"));
        served.await.unwrap();

        let (base, served) = stand_in("200 OK", r#"{"secret":"s1"}"#).await;
        assert_eq!(pair(&base, "c", "n").await, Err("the hub gave no device id".to_string()));
        served.await.unwrap();

        let (base, served) = stand_in("200 OK", r#"{"device":"d1"}"#).await;
        assert_eq!(pair(&base, "c", "n").await, Err("the hub gave no secret".to_string()));
        served.await.unwrap();
    }

    #[tokio::test]
    async fn a_thread_change_goes_to_its_own_route() {
        let (base, served) = stand_in("200 OK", r#"{"ok":true}"#).await;
        let answer = HubClient::new(&base, SECRET).unwrap().set_meta("t1", json!({ "name": "n" })).await.unwrap();
        assert_eq!(answer["ok"], true);
        assert_eq!(first_line(&served.await.unwrap()), "POST /api/sync/threads/t1/meta HTTP/1.1");

        let (base, served) = stand_in("200 OK", "{}").await;
        HubClient::new(&base, SECRET).unwrap().delete("t1").await.unwrap();
        assert_eq!(first_line(&served.await.unwrap()), "POST /api/sync/threads/t1/delete HTTP/1.1");

        let (base, served) = stand_in("200 OK", "{}").await;
        HubClient::new(&base, SECRET).unwrap().withdraw("t1").await.unwrap();
        assert_eq!(first_line(&served.await.unwrap()), "POST /api/sync/threads/t1/withdraw HTTP/1.1");

        let (base, served) = stand_in("200 OK", "{}").await;
        HubClient::new(&base, SECRET).unwrap().host(json!({ "host": "h" }), 25).await.unwrap();
        assert_eq!(first_line(&served.await.unwrap()), "POST /api/sync/host?wait=25 HTTP/1.1");

        let (base, served) = stand_in("200 OK", "{}").await;
        HubClient::new(&base, SECRET).unwrap().forget().await.unwrap();
        assert_eq!(first_line(&served.await.unwrap()), "POST /api/sync/forget HTTP/1.1");
    }

    #[tokio::test]
    async fn a_hub_that_is_away_is_named_in_the_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);

        let err = HubClient::new(&base, SECRET).unwrap().status().await.unwrap_err();
        assert!(err.starts_with(&format!("could not reach the hub at {base}")), "{err}");
        let err = pair(&base, "c", "n").await.unwrap_err();
        assert!(err.starts_with(&format!("could not reach the node's ragon web at {base}")), "{err}");
    }
}
