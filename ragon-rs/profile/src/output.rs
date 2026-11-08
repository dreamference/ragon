//! `result.json`, `table.md`, `requests.jsonl` and `machine.jsonl` under
//! `<root>/runs/<UTC stamp>-<shape>/` (§5.4, §5.5). Writers normalise the home directory to `~` and
//! drop the host name, so a table can be pasted into a spec unchanged (ORGANISATION §7.8).
use crate::run::RunResult;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn default_root() -> Option<PathBuf> {
    ragon_node_locator::home_dir().map(|h| h.join(".local/share/dreamference/profile"))
}

fn hostname() -> Option<String> {
    let h = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()?
        .trim()
        .to_string();
    (h.len() > 2 && h != "localhost").then_some(h)
}

pub fn normalise(text: &str) -> String {
    let mut t = text.to_string();
    if let Some(home) = ragon_node_locator::home_dir() {
        let h = home.to_string_lossy().to_string();
        if h.len() > 1 {
            t = t.replace(&h, "~");
        }
    }
    if let Some(h) = hostname() {
        t = t.replace(&h, "node");
    }
    t
}

fn n(v: &Value, digits: usize) -> String {
    v.as_f64()
        .map(|x| format!("{x:.digits$}"))
        .unwrap_or_else(|| "n/a".into())
}

fn med(v: &Value, digits: usize) -> String {
    match v.get("median").and_then(Value::as_f64) {
        Some(m) => format!("{m:.digits$} (±{}%)", n(&v["spread_pct"], 1)),
        None => "n/a".into(),
    }
}

fn kind_line(name: &str, f: &Value) -> String {
    let g = &f["inter_token_gap_ms"];
    format!(
        "{name}: TTFT {} s ({}; {} of {} tokens reused), prefill/1K {} ms, decode {} tok/s, gap p50 {} p90 {} p99 {} ms, τ {}, accept {}%, step {} ms mean / {} max",
        med(&f["ttft_s"], 3),
        f["cache_state"].as_str().unwrap_or("n/a"),
        n(&f["reused_tokens"], 0),
        n(&f["prompt_tokens"], 0),
        med(&f["prefill_ms_per_1k"], 1),
        n(&f["decode_tok_s"], 1),
        n(&g["p50"], 1),
        n(&g["p90"], 1),
        n(&g["p99"], 1),
        n(&f["tokens_per_pass"], 2),
        n(&f["acceptance_pct"], 1),
        n(&f["step_period_ms"]["mean"], 1),
        n(&f["step_period_ms"]["max"], 1),
    )
}

pub fn table(r: &Value) -> String {
    let figs = &r["figures"];
    let mut lines: Vec<String> = vec![];
    if let Some(a) = figs.get("aggregate") {
        lines.push(format!(
            "span {} tok/s, wall {} tok/s",
            med(&a["span_tok_s"], 1),
            med(&a["wall_tok_s"], 1)
        ));
        lines.push(format!(
            "per-stream decode {} tok/s (pooled {})",
            med(&a["per_stream_tok_s"], 1),
            n(&a["decode_tok_s_per_stream"], 1)
        ));
        lines.push(format!(
            "TTFT {} s ({})",
            med(&a["ttft_s"], 3),
            a["ttft_source"].as_str().unwrap_or("n/a")
        ));
        lines.push(format!(
            "step period {} ms mean, {} max",
            n(&a["step_period_ms"]["mean"], 1),
            n(&a["step_period_ms"]["max"], 1)
        ));
    } else {
        for k in ["prose", "code", "json", "long"] {
            if let Some(f) = figs.get(k) {
                lines.push(kind_line(k, f));
            }
        }
        lines.push(format!(
            "decode (all, pooled) {} tok/s; energy per token n/a",
            n(&figs["all"]["decode_tok_s"], 1)
        ));
    }
    let ms = &r["machine"]["summary"];
    let kb = |v: &Value| {
        v.as_f64()
            .map(|x| format!("{:.1} GB", x * 1024.0 / 1e9))
            .unwrap_or_else(|| "n/a".into())
    };
    let machine = format!(
        "{}; driver {}; power limit {} W; SM clock median {} MHz (normal n/a); MemAvailable min {}; RSS max {}; PSI max {}",
        r["machine"]["node"].as_str().unwrap_or("n/a"),
        ms["driver"].as_str().unwrap_or("n/a"),
        n(&ms["power_limit_w"], 0),
        n(&ms["sm_clock_median_mhz"], 0),
        kb(&ms["mem_available_min_kb"]),
        kb(&ms["rss_max_kb"]),
        n(&ms["psi_max"], 2),
    );
    let b = &r["build"];
    let rows = [
        ("schema", r["schema"].to_string()),
        (
            "build",
            format!(
                "Ragon {}; engine {}; model {}; drafter {}",
                b["ragon_version"].as_str().unwrap_or("n/a"),
                b["engine"].as_str().unwrap_or("n/a"),
                b["model"].as_str().unwrap_or("n/a"),
                b["drafter"].as_str().unwrap_or("n/a")
            ),
        ),
        ("flags", r["flags"].as_str().unwrap_or("").to_string()),
        ("shape", r["shape"].to_string()),
        (
            "workload",
            r["workload_sha256"].as_str().unwrap_or("").to_string(),
        ),
        ("window", r["window"].as_str().unwrap_or("").to_string()),
        ("machine", machine),
        ("state", r["state"].as_str().unwrap_or("").to_string()),
        (
            "identical",
            r["identical"].as_str().unwrap_or("").to_string(),
        ),
        ("figures", lines.join("<br>")),
        ("verdict", r["verdict"].as_str().unwrap_or("").to_string()),
    ];
    let mut out = String::from("| field | content |\n|---|---|\n");
    for (k, v) in rows {
        out += &format!("| {k} | {} |\n", v.replace('|', "/"));
    }
    out
}

/// Writes the four files; returns the run directory.
pub fn write(root: &Path, res: &RunResult) -> std::io::Result<PathBuf> {
    let stamp = res.result["started_utc"]
        .as_str()
        .unwrap_or("0")
        .replace([':', '-'], "");
    let shape = res.result["shape"]["name"].as_str().unwrap_or("run");
    let dir = root.join("runs").join(format!(
        "{}-{shape}",
        stamp.trim_end_matches('Z').to_string() + "Z"
    ));
    std::fs::create_dir_all(&dir)?;
    let put = |name: &str, text: String| std::fs::write(dir.join(name), normalise(&text));
    put(
        "result.json",
        serde_json::to_string_pretty(&res.result).unwrap_or_default() + "\n",
    )?;
    put("table.md", table(&res.result))?;
    let mut rq = String::new();
    for r in &res.requests {
        let mut r = r.clone();
        if let Some(o) = r.as_object_mut() {
            o.remove("gaps_ms");
            o.insert("gap_count".into(), Value::from(0));
        }
        rq += &(r.to_string() + "\n");
    }
    put("requests.jsonl", rq)?;
    let mut mj = String::new();
    for s in &res.machine {
        mj += &(serde_json::to_string(s).unwrap_or_default() + "\n");
    }
    put("machine.jsonl", mj)?;
    Ok(dir)
}
