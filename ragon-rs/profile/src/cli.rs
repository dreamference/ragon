//! `ragon profile run one-stream|four-streams [--reps N] [--url URL] [--log FILE] [--model ID]
//! [--node ID] [--out DIR] [--lock FILE] [--pid PID] [--no-gpu] [--json] [--yes]`.
use crate::machine::Sources;
use crate::output;
use crate::run::{self, Config, Shape};
use std::time::Duration;

const USAGE: &str = "usage: ragon profile run <one-stream|four-streams> [--reps N] [--url URL] [--log FILE] [--model ID] [--node ID] [--out DIR] [--lock FILE] [--pid PID] [--no-gpu] [--json] [--yes]";

pub fn run(args: &[String]) -> i32 {
    let mut it = args.iter().peekable();
    if it.next().map(String::as_str) != Some("run") {
        eprintln!("{USAGE}");
        return 1;
    }
    let Some(shape) = it.next().and_then(|s| Shape::parse(s)) else {
        eprintln!("{USAGE}\nshapes available in this build: one-stream, four-streams");
        return 1;
    };
    let home = ragon_node_locator::home_dir();
    let mut cfg = Config {
        url: "http://127.0.0.1:8000".into(),
        shape,
        reps: 3,
        log: None,
        model: None,
        node: std::env::var("RAGON_NODE_ID").unwrap_or_else(|_| "unknown".into()),
        version: std::env::var("RAGON_VERSION").unwrap_or_else(|_| "unknown".into()),
        lock_path: home.as_ref().map(|h| h.join(".ragon-gpu.lock")),
        sources: Sources::none(),
        timeout: Duration::from_secs(600),
        long_chars: crate::workload::LONG_CHARS,
    };
    let (mut out, mut json, mut pid, mut gpu) = (output::default_root(), false, None, true);
    while let Some(a) = it.next() {
        let mut val = |name: &str| -> Option<String> {
            let v = it.next().cloned();
            if v.is_none() {
                eprintln!("{name} needs a value");
            }
            v
        };
        match a.as_str() {
            "--reps" => match val("--reps")
                .and_then(|v| v.parse::<u32>().ok())
                .filter(|n| *n >= 1)
            {
                Some(n) => cfg.reps = n,
                None => return 1,
            },
            "--url" => match val("--url") {
                Some(v) => cfg.url = v,
                None => return 1,
            },
            "--log" => match val("--log") {
                Some(v) => cfg.log = Some(v.into()),
                None => return 1,
            },
            "--model" => match val("--model") {
                Some(v) => cfg.model = Some(v),
                None => return 1,
            },
            "--node" => match val("--node") {
                Some(v) => cfg.node = v,
                None => return 1,
            },
            "--out" => match val("--out") {
                Some(v) => out = Some(v.into()),
                None => return 1,
            },
            "--lock" => match val("--lock") {
                Some(v) => cfg.lock_path = Some(v.into()),
                None => return 1,
            },
            "--pid" => match val("--pid").and_then(|v| v.parse::<u32>().ok()) {
                Some(v) => pid = Some(v),
                None => return 1,
            },
            "--no-gpu" => gpu = false,
            "--json" => json = true,
            "--yes" => {}
            other => {
                eprintln!("unknown argument {other}\n{USAGE}");
                return 1;
            }
        }
    }
    cfg.sources = Sources::host(pid);
    if !gpu {
        cfg.sources.gpu_cmd = None;
    }
    let Some(out) = out else {
        eprintln!("❌ No home directory to keep results in; pass --out DIR.");
        return 1;
    };
    run_config(&cfg, &out, json)
}

/// The run itself, with every path decided by the caller.
pub fn run_config(cfg: &Config, out: &std::path::Path, json: bool) -> i32 {
    let res = match run::run(cfg) {
        Ok(r) => r,
        Err(why) => {
            eprintln!("{why}");
            return 1;
        }
    };
    match output::write(out, &res) {
        Ok(dir) => {
            if json {
                println!(
                    "{}",
                    output::normalise(&serde_json::to_string(&res.result).unwrap_or_default())
                );
            } else {
                println!("{}", output::table(&res.result));
            }
            println!("{}", output::normalise(&dir.to_string_lossy()));
        }
        Err(e) => {
            eprintln!("❌ Could not write the results: {e}");
            return 1;
        }
    }
    if res.state == "clean" { 0 } else { 1 }
}
