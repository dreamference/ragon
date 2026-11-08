//! `ragon profile trace <file> [--off FILE] [--plan-bytes N] [--peak-gbs G] [--out DIR] [--json]`
//! (§4.3, level 3): reads a finished `RAGON_STEP_TRACE` file (JSON lines with `t`, `kind`,
//! `sessions`, `rows`, `tokens`, `ms`, …) and prints the step-period percentiles and the longest
//! steps. With `--off` (a trace of the same shape with the drafter off) it adds the speculation gain;
//! with `--plan-bytes` and `--peak-gbs` it adds the decode bandwidth use. The summary is written to
//! `runs/<UTC stamp>-trace/` (§5.5). Nothing here starts or sets a trace.
use crate::figures::{bandwidth_use_pct, percentile, speculation_gain};
use crate::output;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const LONGEST: usize = 5;

/// One decode-side step of a trace.
#[derive(Debug, Clone)]
pub struct Step {
    pub t: f64,
    pub kind: String,
    pub tokens: f64,
    pub ms: f64,
}

/// Parses the JSON lines; a line that is not an object with a numeric `ms` is skipped.
pub fn parse(text: &str) -> Vec<Step> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter_map(|v| {
            Some(Step {
                t: v["t"].as_f64().unwrap_or(0.0),
                kind: v["kind"].as_str().unwrap_or("").to_string(),
                tokens: v["tokens"].as_f64().unwrap_or(0.0),
                ms: v["ms"].as_f64().filter(|m| *m > 0.0)?,
            })
        })
        .collect()
}

fn is_decode(s: &Step) -> bool {
    !s.kind.to_ascii_lowercase().contains("prefill")
}

/// Decode tokens per second of one stream: Σ tokens / Σ step seconds over the decode steps.
pub fn decode_tok_s(steps: &[Step]) -> Option<f64> {
    let (tok, ms) = steps
        .iter()
        .filter(|s| is_decode(s))
        .fold((0.0, 0.0), |(a, b), s| (a + s.tokens, b + s.ms));
    (tok > 0.0 && ms > 0.0).then(|| tok / (ms / 1000.0))
}

/// The summary of one trace as a JSON object.
pub fn summarise(steps: &[Step]) -> Value {
    let ms: Vec<f64> = steps
        .iter()
        .filter(|s| is_decode(s))
        .map(|s| s.ms)
        .collect();
    let mut longest: Vec<&Step> = steps.iter().collect();
    longest.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    let longest: Vec<Value> = longest
        .iter()
        .take(LONGEST)
        .map(|s| json!({"t": s.t, "kind": s.kind, "ms": s.ms}))
        .collect();
    json!({
        "steps": steps.len(),
        "decode_steps": ms.len(),
        "step_ms": {
            "p50": percentile(&ms, 50.0), "p90": percentile(&ms, 90.0),
            "p99": percentile(&ms, 99.0), "max": percentile(&ms, 100.0),
        },
        "decode_tok_s": decode_tok_s(steps),
        "longest": longest,
    })
}

fn fmt(v: &Value) -> String {
    v.as_f64().map_or("n/a".into(), |x| format!("{x:.1}"))
}

/// The fixed table for a summary.
pub fn table(s: &Value) -> String {
    let mut out = String::from("| figure | value |\n|---|---|\n");
    let mut row = |k: &str, v: String| out.push_str(&format!("| {k} | {v} |\n"));
    row(
        "steps (decode)",
        format!("{} ({})", s["steps"], s["decode_steps"]),
    );
    for p in ["p50", "p90", "p99", "max"] {
        row(&format!("step period {p} (ms)"), fmt(&s["step_ms"][p]));
    }
    row("decode tok/s, per stream", fmt(&s["decode_tok_s"]));
    if let Some(g) = s["speculation_gain"].as_f64() {
        row("speculation gain (on / off)", format!("{g:.2}"));
    }
    if let Some(b) = s["decode_bandwidth_use_pct"].as_f64() {
        row("decode bandwidth use (% of peak)", format!("{b:.1}"));
    }
    for l in s["longest"].as_array().into_iter().flatten() {
        row(
            &format!("longest step at t={}", l["t"]),
            format!(
                "{} ms ({})",
                fmt(&l["ms"]),
                l["kind"].as_str().unwrap_or("")
            ),
        );
    }
    out
}

const USAGE: &str = "usage: ragon profile trace <file> [--off FILE] [--plan-bytes N] [--peak-gbs G] [--out DIR] [--json]";

pub fn run(args: &[String]) -> i32 {
    let mut it = args.iter();
    let (mut file, mut off, mut bytes, mut peak) = (None::<PathBuf>, None::<PathBuf>, None, None);
    let (mut out, mut json_out) = (output::default_root(), false);
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned();
        match a.as_str() {
            "--off" => off = val().map(Into::into),
            "--plan-bytes" => bytes = val().and_then(|v| v.parse::<f64>().ok()),
            "--peak-gbs" => peak = val().and_then(|v| v.parse::<f64>().ok()),
            "--out" => out = val().map(Into::into),
            "--json" => json_out = true,
            s if !s.starts_with("--") && file.is_none() => file = Some(s.into()),
            _ => {
                eprintln!("{USAGE}");
                return 1;
            }
        }
    }
    let (Some(file), Some(out)) = (file, out) else {
        eprintln!("{USAGE}");
        return 1;
    };
    let read = |p: &Path| {
        std::fs::read_to_string(p).map_err(|e| format!("❌ Cannot read {}: {e}", p.display()))
    };
    let steps = match read(&file).map(|t| parse(&t)) {
        Ok(s) if !s.is_empty() => s,
        Ok(_) => {
            eprintln!(
                "❌ {} holds no step lines (JSON lines with a numeric ms).",
                output::normalise(&file.to_string_lossy())
            );
            return 1;
        }
        Err(e) => {
            eprintln!("{}", output::normalise(&e));
            return 1;
        }
    };
    let mut summary = summarise(&steps);
    if let Some(off) = off {
        match read(&off).map(|t| decode_tok_s(&parse(&t))) {
            Ok(off_rate) => {
                summary["speculation_gain"] = json!(
                    decode_tok_s(&steps)
                        .zip(off_rate)
                        .and_then(|(a, b)| speculation_gain(a, b))
                );
            }
            Err(e) => {
                eprintln!("{}", output::normalise(&e));
                return 1;
            }
        }
    }
    if let (Some(b), Some(p), Some(period)) = (bytes, peak, summary["step_ms"]["p50"].as_f64()) {
        summary["decode_bandwidth_use_pct"] = json!(bandwidth_use_pct(b, period / 1000.0, p));
    }
    match write(&out, &summary) {
        Ok(dir) => {
            let text = if json_out {
                summary.to_string()
            } else {
                table(&summary)
            };
            println!("{}", output::normalise(&text));
            println!("{}", output::normalise(&dir.to_string_lossy()));
            0
        }
        Err(e) => {
            eprintln!("❌ Could not write the results: {e}");
            1
        }
    }
}

/// Writes `result.json` and `table.md` under `<root>/runs/<UTC stamp>-trace/`.
pub fn write(root: &Path, summary: &Value) -> std::io::Result<PathBuf> {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let dir = root.join("runs").join(format!("{stamp}-trace"));
    std::fs::create_dir_all(&dir)?;
    let result = json!({"schema": 1, "shape": {"name": "trace"}, "summary": summary});
    std::fs::write(
        dir.join("result.json"),
        output::normalise(&serde_json::to_string_pretty(&result)?),
    )?;
    std::fs::write(dir.join("table.md"), output::normalise(&table(summary)))?;
    Ok(dir)
}
