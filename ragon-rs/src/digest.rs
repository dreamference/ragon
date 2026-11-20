//! Whether Ragon Digest runs inside Ragon: a long command output reaches the model as a digest,
//! and the output itself is kept in a file the agent reads by line numbers.
//!
//! The rule is the leaf crate `ragon-rs/digest`, which Codex's core calls on each command output
//! through patch `0043`. This module decides once per launch whether it runs. Off by default until
//! the SWE-bench comparison decides; `DREAMFERENCE_DIGEST_INSIDE` or `inside = true` in the
//! `[digest]` table of the config file switches it on, and `min_lines` there sets the threshold.
//! When it is off nothing is configured, so the crate hands every output back untouched.
//!
//! Two more settings in the same table, each off by default and each with its own variable (the
//! owner's answers 2 and 3 of 2026-11-20): `last_good` leaves out of a digest the lines the last
//! successful run of the same command printed too, and `hide_old` replaces every output older than
//! the last ten steps by one line naming where the whole output is (`ragon_masking::window`, which
//! works with or without the digest).
//!
//! A third, `model` (answer 1 of the same day, its own variable, off by default and only with the
//! digest on), lets the node's model pick lines of a digest by number and retell the output: the
//! crate's `model` step, asked through [`NodeModel`] with thinking off and a 20-second limit. Any
//! failure there leaves the digest as stage A chose it.

use std::path::Path;
use std::time::Duration;

use ragon_digest::model::{Reply, Selector};

/// The environment variable that switches the digest on or off for one process.
pub const ENV: &str = "DREAMFERENCE_DIGEST_INSIDE";

/// The table in the Dreamference config file.
pub const TABLE: &str = "digest";

/// The key in [`TABLE`] that switches it on.
pub const KEY: &str = "inside";

/// The key in [`TABLE`] that sets the number of lines from which an output is digested.
pub const MIN_LINES_KEY: &str = "min_lines";

/// Off until the SWE-bench comparison shows it costs no resolved task.
pub const DEFAULT: bool = false;

/// The environment variable for [`LAST_GOOD_KEY`].
pub const LAST_GOOD_ENV: &str = "DREAMFERENCE_DIGEST_LAST_GOOD";

/// The key in [`TABLE`] that leaves out of a digest the lines of the command's last good run.
pub const LAST_GOOD_KEY: &str = "last_good";

/// The environment variable for [`HIDE_OLD_KEY`].
pub const HIDE_OLD_ENV: &str = "DREAMFERENCE_DIGEST_HIDE_OLD";

/// The key in [`TABLE`] that hides outputs older than the last ten steps.
pub const HIDE_OLD_KEY: &str = "hide_old";

/// The environment variable for [`MODEL_KEY`].
pub const MODEL_ENV: &str = "DREAMFERENCE_DIGEST_MODEL";

/// The key in [`TABLE`] that lets the node's model pick a digest's lines by number.
pub const MODEL_KEY: &str = "model";

/// How long the model step waits for the node's model (spec §3.3).
pub const MODEL_TIMEOUT: Duration = Duration::from_secs(20);

/// The most tokens the model's answer may take: some line numbers and three sentences.
const ANSWER_TOKENS: u64 = 400;

fn settings() -> toml::Table {
    crate::config_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default()
}

fn table(settings: &toml::Table) -> Option<&toml::Table> {
    settings.get(TABLE).and_then(toml::Value::as_table)
}

/// Whether the digest is on for this launch: the environment first, then the config file.
pub fn enabled(env: Option<&str>, settings: &toml::Table) -> bool {
    flag(env, settings, KEY)
}

/// One switch of the `[digest]` table: the environment first, then the config file, then off.
pub fn flag(env: Option<&str>, settings: &toml::Table, key: &str) -> bool {
    if let Some(value) = env.filter(|value| !value.is_empty()) {
        return matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
    }
    table(settings).and_then(|table| table.get(key)).and_then(toml::Value::as_bool).unwrap_or(DEFAULT)
}

/// Whether hiding old outputs is on for this launch.
pub fn hide_old_now() -> bool {
    flag(std::env::var(HIDE_OLD_ENV).ok().as_deref(), &settings(), HIDE_OLD_KEY)
}

/// The threshold in lines: the config file's `min_lines` when it gives a positive number.
pub fn min_lines(settings: &toml::Table) -> usize {
    table(settings)
        .and_then(|table| table.get(MIN_LINES_KEY))
        .and_then(toml::Value::as_integer)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(ragon_digest::MIN_LINES)
}

/// The node's model as the digest's model step asks it: `/v1/chat/completions` on the node, with
/// the session's key when one is set.
pub struct NodeModel {
    client: reqwest::Client,
    url: String,
    model: String,
    key: Option<String>,
}

impl NodeModel {
    /// The model `model` served at `host`, given up on after `timeout`.
    pub fn new(host: &str, model: &str, key: Option<String>, timeout: Duration) -> Option<Self> {
        let client = crate::proxy::direct_client(timeout).ok()?;
        let url = format!("{}/v1/chat/completions", host.trim_end_matches('/'));
        Some(Self { client, url, model: model.to_string(), key })
    }

    /// The request: one user message, thinking off, deterministic, a short answer.
    pub fn body(&self, prompt: &str) -> serde_json::Value {
        serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "temperature": 0,
            "max_tokens": ANSWER_TOKENS,
            "chat_template_kwargs": {"enable_thinking": false},
        })
    }
}

/// The answer's text in a chat completion.
pub fn content(body: &serde_json::Value) -> Option<String> {
    body.pointer("/choices/0/message/content").and_then(serde_json::Value::as_str).map(str::to_string)
}

impl Selector for NodeModel {
    fn ask<'a>(&'a self, prompt: &'a str) -> Reply<'a> {
        Box::pin(async move {
            let mut request = self.client.post(&self.url).json(&self.body(prompt));
            if let Some(key) = &self.key {
                request = request.bearer_auth(key);
            }
            let response = request.send().await.map_err(|error| error.to_string())?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("the node answered {status}"));
            }
            let body: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
            content(&body).ok_or_else(|| "no answer text".to_string())
        })
    }
}

/// Switches the digest and hiding old outputs on for this process when their settings say so;
/// does nothing otherwise. `host` and `model` are the node and the model the session uses, which
/// the model step asks.
pub fn configure(codex_home: &Path, host: &str, model: &str) {
    let settings = settings();
    let env = |name: &str| std::env::var(name).ok();
    if flag(env(HIDE_OLD_ENV).as_deref(), &settings, HIDE_OLD_KEY) {
        ragon_masking::window::prune(codex_home, std::time::SystemTime::now());
        ragon_masking::window::set_window(ragon_masking::window::Window::default(), codex_home);
    }
    if !enabled(env(ENV).as_deref(), &settings) {
        return;
    }
    let last_good = flag(env(LAST_GOOD_ENV).as_deref(), &settings, LAST_GOOD_KEY);
    let mut model_step = flag(env(MODEL_ENV).as_deref(), &settings, MODEL_KEY);
    if model_step {
        let key = crate::auth::api_key(env("RAGON_API_KEY").as_deref(), env("OPENAI_API_KEY").as_deref());
        model_step = NodeModel::new(host, model, key, MODEL_TIMEOUT)
            .is_some_and(|node| ragon_digest::model::set_selector(Box::new(node)));
    }
    let options = ragon_digest::Options { min_lines: min_lines(&settings), last_good, model: model_step };
    ragon_digest::configure_with(codex_home, options);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn it_is_off_unless_switched_on() {
        assert!(!enabled(None, &toml::Table::new()));
        assert!(!enabled(Some(""), &toml::Table::new()));
        assert!(!enabled(None, &parse("[digest]\ninside = false\n")));
        assert!(!enabled(None, &parse("inside = true\n")));
    }

    #[test]
    fn the_config_file_or_the_environment_switches_it_on() {
        assert!(enabled(None, &parse("[digest]\ninside = true\n")));
        for value in ["1", "true", "YES", "on"] {
            assert!(enabled(Some(value), &toml::Table::new()), "{value}");
        }
    }

    #[test]
    fn the_environment_wins_over_the_config_file() {
        assert!(!enabled(Some("0"), &parse("[digest]\ninside = true\n")));
        assert!(!enabled(Some("off"), &parse("[digest]\ninside = true\n")));
        assert!(enabled(Some("1"), &parse("[digest]\ninside = false\n")));
    }

    #[test]
    fn last_good_and_hide_old_are_off_by_default_and_each_has_its_own_switch() {
        let empty = toml::Table::new();
        for key in [LAST_GOOD_KEY, HIDE_OLD_KEY, MODEL_KEY] {
            assert!(!flag(None, &empty, key), "{key}");
            assert!(!flag(None, &parse("[digest]\ninside = true\n"), key), "{key}: inside does not switch it on");
            assert!(flag(None, &parse(&format!("[digest]\n{key} = true\n")), key), "{key}");
            assert!(!flag(Some("0"), &parse(&format!("[digest]\n{key} = true\n")), key), "{key}");
            assert!(flag(Some("1"), &empty, key), "{key}");
        }
        assert!(!enabled(None, &parse("[digest]\nlast_good = true\nhide_old = true\n")));
        assert_ne!(LAST_GOOD_ENV, HIDE_OLD_ENV);
        assert_ne!(MODEL_ENV, LAST_GOOD_ENV);
    }

    /// A stand-in node on loopback: it reads one request whole, hands it over, and answers with
    /// `status` and `body` after `delay`.
    fn stand_in(status: &'static str, body: &'static str, delay: Duration) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                request.push_str(&line);
            }
            let mut payload = vec![0; length];
            let _ = reader.read_exact(&mut payload);
            request.push_str(&String::from_utf8_lossy(&payload));
            let _ = send.send(request);
            std::thread::sleep(delay);
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        (base, receive)
    }

    #[tokio::test]
    async fn the_node_is_asked_with_thinking_off_and_its_answer_text_comes_back() {
        let answer = r#"{"choices":[{"message":{"role":"assistant","content":"LINES: 12\nSUMMARY: It failed."}}]}"#;
        let (base, request) = stand_in("200 OK", answer, Duration::ZERO);
        let node = NodeModel::new(&base, "served-model", Some("k-test".to_string()), MODEL_TIMEOUT).unwrap();
        assert_eq!(node.ask("which lines?").await.unwrap(), "LINES: 12\nSUMMARY: It failed.");
        let request = request.recv().unwrap();
        assert!(request.starts_with("POST /v1/chat/completions "), "{request}");
        assert!(request.to_ascii_lowercase().contains("authorization: bearer k-test"));
        let body: serde_json::Value = serde_json::from_str(&request[request.find('{').unwrap()..]).unwrap();
        assert_eq!(body["model"], "served-model");
        assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
        assert_eq!(body["messages"][0]["content"], "which lines?");
    }

    #[tokio::test]
    async fn a_refusal_or_a_slow_node_is_an_error_so_stage_a_stands() {
        let (base, _request) = stand_in("401 Unauthorized", "{}", Duration::ZERO);
        let node = NodeModel::new(&base, "m", None, MODEL_TIMEOUT).unwrap();
        assert!(node.ask("x").await.unwrap_err().contains("401"));
        let (base, _request) = stand_in("200 OK", "{}", Duration::from_secs(3));
        let node = NodeModel::new(&base, "m", None, Duration::from_millis(300)).unwrap();
        let started = std::time::Instant::now();
        assert!(node.ask("x").await.is_err());
        assert!(started.elapsed() < Duration::from_secs(2), "the limit holds");
        assert_eq!(MODEL_TIMEOUT, Duration::from_secs(20));
    }

    #[test]
    fn the_threshold_is_two_hundred_lines_unless_set() {
        assert_eq!(min_lines(&toml::Table::new()), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 80\n")), 80);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 0\n")), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = -5\n")), 200);
    }
}
