//! `ragon profile gemm [--rows 1,16,32,48,64] [--bench PATH] [--ms N] [--lock FILE] [--out DIR] [--json]`
//! (§4.2, level 2): runs the engine image's GEMM bench (`ragon-stream-tests --bench-groups`, with
//! `RAGON_BENCH_MS`) while holding the GPU lock, and reports GB/s per GEMM at M rows beside the
//! streaming peak. It refuses, and says why, when the bench binary is not present (the image does not
//! carry it) or the lock is held (a window or a profile is open). The output goes to
//! `runs/<UTC stamp>-gemm/` (§5.5). The bench's lines are read as `... M=<rows> ... <x> GB/s`; a line
//! with `peak` and GB/s is the streaming peak.
use crate::output;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEFAULT_ROWS: [u32; 5] = [1, 16, 32, 48, 64];
const BENCH: &str = "ragon-stream-tests";
const USAGE: &str = "usage: ragon profile gemm [--rows 1,16,32,48,64] [--bench PATH] [--ms N] [--lock FILE] [--out DIR] [--json]";

/// The number before `GB/s` in a line.
fn gbs(line: &str) -> Option<f64> {
    let i = line.find("GB/s")?;
    line[..i]
        .split(|c: char| c.is_whitespace() || c == '=' || c == ':')
        .rev()
        .find_map(|w| w.parse::<f64>().ok())
}

/// The M of a `M=<n>` token.
fn rows_of(line: &str) -> Option<u32> {
    line.split(|c: char| c.is_whitespace() || c == ',')
        .find_map(|w| w.strip_prefix("M=").or_else(|| w.strip_prefix("m=")))
        .and_then(|n| n.parse().ok())
}

/// Parses the bench output into `{peak_gb_s, rows: {M: [{name, gb_s}]}}`, keeping only `rows`.
pub fn parse(text: &str, rows: &[u32]) -> Value {
    let mut peak = None;
    let mut by_m: std::collections::BTreeMap<u32, Vec<Value>> = Default::default();
    for line in text.lines() {
        let Some(g) = gbs(line) else { continue };
        if line.to_ascii_lowercase().contains("peak") {
            peak = Some(g);
        } else if let Some(m) = rows_of(line).filter(|m| rows.contains(m)) {
            let name = line
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches(':');
            by_m.entry(m)
                .or_default()
                .push(json!({"name": name, "gb_s": g}));
        }
    }
    let rows_json: serde_json::Map<String, Value> = by_m
        .into_iter()
        .map(|(m, v)| (m.to_string(), Value::Array(v)))
        .collect();
    json!({"peak_gb_s": peak, "rows": rows_json})
}

/// The fixed table: per M, each GEMM's GB/s and its share of the peak.
pub fn table(r: &Value) -> String {
    let peak = r["peak_gb_s"].as_f64();
    let mut out = format!(
        "| M | gemm | GB/s | % of peak |\n|---|---|---|---|\n| peak | streaming | {} | 100.0 |\n",
        peak.map_or("n/a".into(), |p| format!("{p:.1}"))
    );
    if let Some(map) = r["rows"].as_object() {
        let mut ms: Vec<&String> = map.keys().collect();
        ms.sort_by_key(|k| k.parse::<u32>().unwrap_or(0));
        for m in ms {
            for e in map[m].as_array().into_iter().flatten() {
                let g = e["gb_s"].as_f64().unwrap_or(0.0);
                let pct = peak.map_or("n/a".into(), |p| format!("{:.1}", g / p * 100.0));
                out.push_str(&format!(
                    "| {m} | {} | {g:.1} | {pct} |\n",
                    e["name"].as_str().unwrap_or("")
                ));
            }
        }
    }
    out
}

fn find_bench(given: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = given {
        return p.is_file().then(|| p.to_path_buf());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join(BENCH))
        .find(|p| p.is_file())
}

pub fn run(args: &[String]) -> i32 {
    let mut it = args.iter();
    let (mut rows, mut bench, mut ms) = (DEFAULT_ROWS.to_vec(), None::<PathBuf>, 200u32);
    let mut lock = ragon_node_locator::home_dir().map(|h| h.join(".ragon-gpu.lock"));
    let (mut out, mut json_out) = (output::default_root(), false);
    if it.next().map(String::as_str) != Some("gemm") {
        eprintln!("{USAGE}");
        return 1;
    }
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned();
        match a.as_str() {
            "--rows" => match val().map(|v| {
                v.split(',')
                    .map(|n| n.trim().parse::<u32>())
                    .collect::<Result<Vec<_>, _>>()
            }) {
                Some(Ok(r)) if !r.is_empty() => rows = r,
                _ => {
                    eprintln!("{USAGE}");
                    return 1;
                }
            },
            "--bench" => bench = val().map(Into::into),
            "--ms" => match val().and_then(|v| v.parse().ok()) {
                Some(n) => ms = n,
                None => return 1,
            },
            "--lock" => lock = val().map(Into::into),
            "--out" => out = val().map(Into::into),
            "--json" => json_out = true,
            "--yes" => {}
            _ => {
                eprintln!("{USAGE}");
                return 1;
            }
        }
    }
    let Some(out) = out else {
        eprintln!("{USAGE}");
        return 1;
    };
    let Some(bench) = find_bench(bench.as_deref()) else {
        eprintln!(
            "❌ The GEMM bench ({BENCH}) is not here: this engine image does not carry it, so there is nothing to run (PROFILING §4.2)."
        );
        return 1;
    };
    let _lock = match take_lock(lock.as_deref()) {
        Ok(f) => f,
        Err(why) => {
            eprintln!("{why}");
            return 1;
        }
    };
    let started = chrono::Utc::now();
    let run = Command::new(&bench)
        .arg("--bench-groups")
        .env("RAGON_BENCH_MS", ms.to_string())
        .output();
    let text = match run {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            eprintln!("❌ The bench failed ({}).", o.status);
            return 1;
        }
        Err(e) => {
            eprintln!("❌ Could not start the bench: {e}");
            return 1;
        }
    };
    let mut result = parse(&text, &rows);
    result["schema"] = json!(1);
    result["shape"] = json!({"name": "gemm", "rows": rows, "bench_ms": ms});
    result["started_utc"] = json!(started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    if result["rows"].as_object().is_none_or(|m| m.is_empty()) {
        eprintln!("❌ The bench printed no GEMM lines for the rows asked.");
        return 1;
    }
    match write(&out, &result, &text) {
        Ok(dir) => {
            let t = if json_out {
                result.to_string()
            } else {
                table(&result)
            };
            println!("{}", output::normalise(&t));
            println!("{}", output::normalise(&dir.to_string_lossy()));
            0
        }
        Err(e) => {
            eprintln!("❌ Could not write the results: {e}");
            1
        }
    }
}

fn take_lock(path: Option<&Path>) -> Result<Option<std::fs::File>, String> {
    let Some(path) = path else { return Ok(None) };
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).ok();
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| format!("❌ lock file: {e}"))?;
    f.try_lock().map(|()| Some(f)).map_err(|_| {
        "❌ The GPU lock is held (a window or a profile is open); try after it ends.".to_string()
    })
}

/// Writes `result.json`, `table.md` and the bench's raw output under `runs/<stamp>-gemm/`.
pub fn write(root: &Path, result: &Value, raw: &str) -> std::io::Result<PathBuf> {
    let stamp = result["started_utc"]
        .as_str()
        .unwrap_or("0")
        .replace([':', '-'], "");
    let dir = root.join("runs").join(format!("{stamp}-gemm"));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("result.json"),
        output::normalise(&serde_json::to_string_pretty(result)?),
    )?;
    std::fs::write(dir.join("table.md"), output::normalise(&table(result)))?;
    std::fs::write(dir.join("bench.txt"), output::normalise(raw))?;
    Ok(dir)
}
