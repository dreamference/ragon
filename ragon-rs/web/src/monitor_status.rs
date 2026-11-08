//! `/api/monitor/status` and `/status` (specs/DREAMFERENCE_MONITORING.md §11, Phase 1): the node
//! monitor's `status --json`, run once per ten seconds however many pages ask, and drawn as a page
//! with no script. Read-only: the monitor probes and reports, it changes nothing. The program is
//! `Config::monitor`, so a test points it at a stub and never touches the real machine.

use serde_json::Value;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;

/// How long a result is reused.
pub const CACHE_SECONDS: u64 = 10;
/// How long one run of the monitor may take.
const RUN_SECONDS: u64 = 15;
/// The most output read from the monitor.
const MAX_OUTPUT: u64 = 1 << 20;

#[derive(Default)]
pub struct Cache {
    // Held across the run, so two requests at once start one process.
    last: Mutex<Option<(Instant, Value)>>,
}

impl Cache {
    pub async fn status(&self, program: &Path) -> Result<Value, String> {
        let mut last = self.last.lock().await;
        if let Some((at, value)) = last.as_ref()
            && at.elapsed() < Duration::from_secs(CACHE_SECONDS)
        {
            return Ok(value.clone());
        }
        let value = run(program).await?;
        *last = Some((Instant::now(), value.clone()));
        Ok(value)
    }
}

async fn run(program: &Path) -> Result<Value, String> {
    let mut child = Command::new(program)
        .args(["status", "--json"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("the monitor did not start: {}", e.kind()))?;
    let mut out = child.stdout.take().ok_or("no output")?.take(MAX_OUTPUT);
    let mut text = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(RUN_SECONDS), async {
        out.read_to_end(&mut text).await?;
        child.wait().await
    })
    .await
    .map_err(|_| "the monitor took too long".to_string())?
    .map_err(|e| format!("the monitor failed: {}", e.kind()))?;
    if !read.success() {
        return Err("the monitor exited with an error".to_string());
    }
    let value: Value = serde_json::from_slice(&text).map_err(|_| "the monitor's output was not JSON".to_string())?;
    if !value.get("items").is_some_and(Value::is_array) {
        return Err("the monitor's output has no items".to_string());
    }
    Ok(value)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

const STYLE: &str = r#"<style>
main { max-width: 720px; }
ul { list-style: none; padding: 0; margin: 0 0 1.5rem; }
li { display: flex; align-items: baseline; gap: 0.75rem; padding: 0.5rem 0; border-bottom: 1px solid var(--field); }
li .name { flex: 0 0 11rem; word-break: break-word; }
li .detail { flex: 1; min-width: 0; color: var(--muted); }
.state { flex: 0 0 5.5rem; font-weight: 700; }
.ok { color: #3a9d5d; } .degraded { color: #c58a00; } .down { color: #d1453b; } .off, .unknown { color: var(--muted); }
a { color: var(--accent); }
</style>"#;

/// The page for a monitor document, or for the reason there is none.
pub fn page(status: &Result<Value, String>) -> String {
    let body = match status {
        Err(reason) => format!("<h1>Status</h1>\n<p>The node monitor could not be read: {}.</p>", escape(reason)),
        Ok(value) => {
            let node = value.pointer("/node/state").and_then(Value::as_str).unwrap_or("unknown");
            let mut out = format!("<h1>Status</h1>\n<p>Node: <strong class=\"state {0}\">{0}</strong></p>\n<ul>\n", class(node));
            for item in value.get("items").and_then(Value::as_array).into_iter().flatten() {
                let text = |key| item.get(key).and_then(Value::as_str).unwrap_or("");
                let state = text("state");
                out.push_str(&format!(
                    "<li><span class=\"state {}\">{}</span><span class=\"name\">{}</span><span class=\"detail\">{}</span></li>\n",
                    class(state),
                    escape(state),
                    escape(text("name")),
                    escape(text("detail")),
                ));
            }
            out.push_str("</ul>");
            out
        }
    };
    crate::devices::page(
        "Status",
        &format!("{STYLE}\n{body}\n<p><a href=\"/\">Back to Ragon</a></p>"),
    )
}

/// A state as a CSS class: only the known words, so a made-up one cannot add markup.
fn class(state: &str) -> &'static str {
    match state {
        "ok" => "ok",
        "degraded" => "degraded",
        "down" => "down",
        "off" => "off",
        _ => "unknown",
    }
}
