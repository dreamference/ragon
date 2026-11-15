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
        let mut value = run(program, &["status", "--json"]).await?;
        // The charts are a bonus: no history (an older monitor, no store) leaves the page without them.
        if let Ok(history) = run(program, &["history"]).await
            && let Some(series) = history.get("series")
        {
            value["history"] = series.clone();
        }
        *last = Some((Instant::now(), value.clone()));
        Ok(value)
    }
}

async fn run(program: &Path, args: &[&str]) -> Result<Value, String> {
    let mut child = Command::new(program)
        .args(args)
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
    if args[0] == "status" && !value.get("items").is_some_and(Value::is_array) {
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
h2 { font-size: 1.1rem; margin: 1.5rem 0 0.5rem; }
.charts { display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: 0.75rem; margin: 0 0 1.5rem; }
.chart .name { color: var(--muted); font-size: 0.85rem; word-break: break-word; }
.chart svg { width: 100%; height: 40px; display: block; background: var(--field); }
.chart polyline { fill: none; stroke: var(--accent); stroke-width: 1.5; }
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
            out.push_str("</ul>\n");
            out.push_str(&charts(value.get("history")));
            out
        }
    };
    crate::devices::page(
        "Status",
        &format!("{STYLE}\n{body}\n<p><a href=\"/\">Back to Ragon</a></p>"),
    )
}

const CHART_WIDTH: f64 = 200.0;
const CHART_HEIGHT: f64 = 40.0;

/// One inline SVG strip chart per series (`[{name, points: [[ts, rank], ...]}]`, rank 0 ok to 2 down),
/// or one line saying there is no history yet. No script, and every name is escaped.
fn charts(history: Option<&Value>) -> String {
    let series: Vec<&Value> = history
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|s| s.get("points").and_then(Value::as_array).is_some_and(|p| !p.is_empty()))
        .collect();
    let mut out = String::from("<h2>History</h2>\n");
    if series.is_empty() {
        out.push_str("<p class=\"empty\">No samples recorded yet.</p>\n");
        return out;
    }
    out.push_str("<div class=\"charts\">\n");
    for entry in series {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
        let points: Vec<(f64, f64)> = entry
            .get("points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?.clamp(0.0, 2.0))))
            .collect();
        let (first, last) = (points.first().map_or(0.0, |p| p.0), points.last().map_or(0.0, |p| p.0));
        let span = (last - first).max(1.0);
        let line: Vec<String> = points
            .iter()
            .map(|(ts, rank)| {
                let x = (ts - first) / span * (CHART_WIDTH - 2.0) + 1.0;
                let y = CHART_HEIGHT - 3.0 - rank / 2.0 * (CHART_HEIGHT - 6.0);
                format!("{x:.1},{y:.1}")
            })
            .collect();
        out.push_str(&format!(
            "<div class=\"chart\"><div class=\"name\">{0}</div><svg viewBox=\"0 0 {1} {2}\" preserveAspectRatio=\"none\" role=\"img\" aria-label=\"{0}\"><polyline points=\"{3}\"/></svg></div>\n",
            escape(name),
            CHART_WIDTH,
            CHART_HEIGHT,
            line.join(" ")
        ));
    }
    out.push_str("</div>\n");
    out
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
