//! The run loop: preflight, the shapes, the undisturbed check, cold salts and the figures per shape.
use crate::figures as f;
use crate::http;
use crate::machine::{self, Sampler, Sources};
use crate::metrics::{self as m, Metrics, SpecStats};
use crate::reqline::{self, RequestLine};
use crate::workload as w;
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

/// A request meant to be cold may reuse at most this many tokens (the chat template's own head).
pub const COLD_REUSED_MAX: u64 = 32;
/// Three discards in a row end the run as "production busy".
pub const DISCARD_LIMIT: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    OneStream,
    FourStreams,
}

impl Shape {
    pub fn name(self) -> &'static str {
        match self {
            Shape::OneStream => "one-stream",
            Shape::FourStreams => "four-streams",
        }
    }
    pub fn parse(s: &str) -> Option<Shape> {
        match s {
            "one-stream" => Some(Shape::OneStream),
            "four-streams" => Some(Shape::FourStreams),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub url: String,
    pub shape: Shape,
    pub reps: u32,
    pub log: Option<PathBuf>,
    pub model: Option<String>,
    pub node: String,
    pub version: String,
    pub lock_path: Option<PathBuf>,
    pub sources: Sources,
    pub timeout: Duration,
    pub long_chars: usize,
}

#[derive(Debug)]
pub enum Refusal {
    Unreachable(String),
    Busy(String),
    LockHeld,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, fm: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Refusal::Unreachable(e) => write!(
                fm,
                "❌ The model server does not answer ({e}); nothing was changed."
            ),
            Refusal::Busy(e) => write!(fm, "❌ {e}; try after it ends."),
            Refusal::LockHeld => write!(
                fm,
                "❌ The GPU lock is held (another window or profile is open); try after it ends."
            ),
        }
    }
}

/// One finished run, ready to be written by `output`.
pub struct RunResult {
    pub result: Value,
    pub requests: Vec<Value>,
    pub machine: Vec<machine::Sample>,
    pub state: String,
}

struct Streamed {
    submit: Instant,
    chunks: Vec<(Instant, usize)>,
    text: String,
    usage_completion: Option<u64>,
}

fn request_body(model: &str, prompt: &str, max_tokens: u32) -> String {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": max_tokens,
        "temperature": 0,
        "stream": true,
        "stream_options": {"include_usage": true},
        "chat_template_kwargs": {"enable_thinking": false},
    })
    .to_string()
}

fn stream_request(
    addr: &str,
    model: &str,
    prompt: &str,
    max_tokens: u32,
    timeout: Duration,
) -> Result<Streamed, String> {
    let mut out = Streamed {
        submit: Instant::now(),
        chunks: Vec::new(),
        text: String::new(),
        usage_completion: None,
    };
    let body = request_body(model, prompt, max_tokens);
    out.submit = Instant::now();
    http::post_stream(
        addr,
        "/v1/chat/completions",
        &body,
        timeout,
        &mut |data, at| {
            if data == "[DONE]" {
                return;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else {
                return;
            };
            if let Some(n) = v
                .pointer("/usage/completion_tokens")
                .and_then(Value::as_u64)
            {
                out.usage_completion = Some(n);
            }
            let piece = v
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !piece.is_empty() {
                out.text.push_str(piece);
                out.chunks.push((at, piece.len()));
            }
        },
    )?;
    if out.chunks.is_empty() {
        return Err("the stream carried no text".into());
    }
    Ok(out)
}

struct Server {
    addr: String,
    timeout: Duration,
    log: Option<PathBuf>,
}

impl Server {
    fn metrics(&self) -> Result<Metrics, String> {
        http::get(&self.addr, "/metrics", self.timeout).map(|t| Metrics::parse(&t))
    }
    fn spec(&self) -> SpecStats {
        http::get(&self.addr, "/spec_stats", self.timeout)
            .map(|t| SpecStats::parse(&t))
            .unwrap_or_default()
    }
    fn log_len(&self) -> u64 {
        self.log
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0)
    }
    fn lines_since(&self, offset: u64) -> Vec<RequestLine> {
        let Some(p) = &self.log else { return vec![] };
        let Ok(bytes) = std::fs::read(p) else {
            return vec![];
        };
        let tail = bytes.get(offset as usize..).unwrap_or(&[]);
        reqline::parse_all(&String::from_utf8_lossy(tail))
    }
    /// Counters after a request: waits briefly for the request counter to move past `before`.
    fn settled(&self, before: &Metrics, want: f64) -> Result<Metrics, String> {
        let mut now = self.metrics()?;
        for _ in 0..40 {
            if now.delta(before, m::E2E_COUNT) >= want || !before.has(m::E2E_COUNT) {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            now = self.metrics()?;
        }
        Ok(now)
    }
}

/// Checks that the server answers and is idle, and takes the GPU lock (held until dropped).
pub fn preflight(cfg: &Config) -> Result<Option<std::fs::File>, Refusal> {
    let addr = http::host_port(&cfg.url);
    let met = http::get(&addr, "/metrics", cfg.timeout)
        .map(|t| Metrics::parse(&t))
        .map_err(Refusal::Unreachable)?;
    let (run, q) = (met.get(m::RUNNING), met.get(m::QUEUED));
    if run > 0.0 || q > 0.0 {
        return Err(Refusal::Busy(format!(
            "Production has {run} running and {q} queued requests"
        )));
    }
    let Some(path) = &cfg.lock_path else {
        return Ok(None);
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(|e| Refusal::Unreachable(format!("lock file: {e}")))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(_) => Err(Refusal::LockHeld),
    }
}

/// One attempt of a lone request, measured from the server's counter deltas.
fn lone(
    srv: &Server,
    model: &str,
    prompt: &str,
    max_tokens: u32,
    cold: bool,
) -> Result<Result<Value, String>, String> {
    let before = srv.metrics()?;
    let (spec0, off) = (srv.spec(), srv.log_len());
    if before.get(m::RUNNING) + before.get(m::QUEUED) > 0.0 {
        return Ok(Err("production busy before the request".into()));
    }
    let s = stream_request(&srv.addr, model, prompt, max_tokens, srv.timeout)?;
    let after = srv.settled(&before, 1.0)?;
    let spec1 = srv.spec();
    let lines = srv.lines_since(off);
    let t0 = s.submit;
    let first = s.chunks[0].0.duration_since(t0).as_secs_f64();
    let last = s.chunks[s.chunks.len() - 1]
        .0
        .duration_since(t0)
        .as_secs_f64();
    let counters = before.has(m::E2E_COUNT);
    if counters && after.delta(&before, m::E2E_COUNT) != 1.0 {
        return Ok(Err(format!(
            "{} requests arrived in the interval",
            after.delta(&before, m::E2E_COUNT)
        )));
    }
    let (ttft, e2e, prompt_tokens, reused, generated, passes, source) = if counters {
        let n = after.delta(&before, m::TTFT_COUNT).max(1.0);
        (
            after.delta(&before, m::TTFT_SUM) / n,
            after.delta(&before, m::E2E_SUM),
            after.delta(&before, m::PROMPT) as u64,
            after.delta(&before, m::CACHED) as u64,
            after.delta(&before, m::GENERATED) as u64,
            after.delta(&before, m::VERIFY),
            "server",
        )
    } else {
        let g = s.usage_completion.unwrap_or(s.chunks.len() as u64);
        (first, last, 0, 0, g, 0.0, "client")
    };
    if cold && source == "server" && reused > COLD_REUSED_MAX {
        return Ok(Err(format!("a cold request reused {reused} tokens")));
    }
    let line = lines.last().cloned();
    let (prompt_tokens, reused) = match (&line, source) {
        (Some(l), "client") => (l.prompt_tokens, l.reused_tokens),
        _ => (prompt_tokens, reused),
    };
    let lens: Vec<usize> = s.chunks.iter().map(|c| c.1).collect();
    let per_chunk = f::tokens_per_chunk(&lens, generated);
    let ev: Vec<(f64, u64)> = s
        .chunks
        .iter()
        .zip(&per_chunk)
        .map(|(c, k)| (c.0.duration_since(t0).as_secs_f64(), *k))
        .collect();
    let gaps = f::gaps_ms(&ev);
    let drafted = after.delta(&before, m::DRAFTED);
    let accepted = after.delta(&before, m::ACCEPTED);
    let steps = spec1.total("steps") - spec0.total("steps");
    let draft_ms_step = (steps > 0.0)
        .then(|| (spec1.total("draft_seconds") - spec0.total("draft_seconds")) / steps * 1000.0);
    let verify_ms_step = (steps > 0.0
        && spec1
            .drafters
            .values()
            .any(|d| d.contains_key("verify_seconds")))
    .then(|| (spec1.total("verify_seconds") - spec0.total("verify_seconds")) / steps * 1000.0);
    Ok(Ok(json!({
        "source": source,
        "cache_state": if reused <= COLD_REUSED_MAX { "cold" } else { "reused" },
        "prompt_tokens": prompt_tokens, "reused_tokens": reused, "generated": generated,
        "ttft_s": ttft, "e2e_s": e2e, "verify_passes": passes,
        "tokens_per_pass": f::tokens_per_pass(generated as f64, passes),
        "drafted": drafted, "accepted": accepted,
        "draft_ms_per_step": draft_ms_step, "verify_ms_per_step": verify_ms_step,
        "client_ttft_s": first, "client_e2e_s": last,
        "bucket": f::bucket(prompt_tokens),
        "prefill_s": line.as_ref().map(|l| l.prefill_s),
        "prefill_ms_per_1k": line.as_ref().and_then(|l| f::prefill_ms_per_1k(l.prefill_s, l.prompt_tokens, l.reused_tokens)),
        "step_period_mean_ms": line.as_ref().and_then(|l| l.step_period_mean_ms),
        "step_period_max_ms": line.as_ref().and_then(|l| l.step_period_max_ms),
        "gaps_ms": gaps,
        "out_sha256": w::sha256_hex(&s.text),
    })))
}

fn num(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn col(recs: &[&Value], k: &str) -> Vec<f64> {
    recs.iter().filter_map(|r| num(r, k)).collect()
}

fn med_spread(v: &[f64]) -> Value {
    json!({"median": f::median(v), "spread_pct": f::spread_pct(v), "n": v.len()})
}

fn kind_figures(recs: &[&Value]) -> Value {
    let gaps: Vec<f64> = recs
        .iter()
        .flat_map(|r| r["gaps_ms"].as_array().cloned().unwrap_or_default())
        .filter_map(|x| x.as_f64())
        .collect();
    let (p50, p90, p99) = f::gap_percentiles(&gaps);
    let sum = |k: &str| col(recs, k).iter().sum::<f64>();
    let tau = (sum("verify_passes") > 0.0).then(|| {
        let toks: f64 = recs
            .iter()
            .filter_map(|r| num(r, "generated"))
            .map(|g| (g - 1.0).max(0.0))
            .sum();
        toks / sum("verify_passes")
    });
    json!({
        "ttft_s": med_spread(&col(recs, "ttft_s")),
        "cache_state": recs.first().map(|r| r["cache_state"].clone()),
        "prompt_tokens": f::median(&col(recs, "prompt_tokens")),
        "reused_tokens": f::median(&col(recs, "reused_tokens")),
        "prefill_ms_per_1k": med_spread(&col(recs, "prefill_ms_per_1k")),
        "decode_tok_s": f::decode_tok_s(&col(recs, "generated"), &col(recs, "e2e_s"), &col(recs, "ttft_s")),
        "inter_token_gap_ms": {"p50": p50, "p90": p90, "p99": p99, "gaps": gaps.len()},
        "tokens_per_pass": tau,
        "acceptance_pct": f::acceptance_pct(sum("accepted"), sum("drafted")),
        "draft_ms_per_step": f::median(&col(recs, "draft_ms_per_step")),
        "verify_ms_per_step": f::median(&col(recs, "verify_ms_per_step")),
        "step_period_ms": {"mean": f::median(&col(recs, "step_period_mean_ms")), "max": col(recs, "step_period_max_ms").into_iter().fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))))},
        "energy_j_per_token": null,
    })
}

fn identical(recs: &[&Value]) -> (usize, usize) {
    let mut hashes: Vec<&str> = recs
        .iter()
        .filter_map(|r| r["out_sha256"].as_str())
        .collect();
    let n = hashes.len();
    if n == 0 {
        return (0, 0);
    }
    hashes.sort();
    let top = hashes
        .iter()
        .map(|h| hashes.iter().filter(|x| *x == h).count())
        .max()
        .unwrap_or(0);
    (top, n)
}

pub fn run(cfg: &Config) -> Result<RunResult, Refusal> {
    let _lock = preflight(cfg)?;
    let addr = http::host_port(&cfg.url);
    let srv = Server {
        addr: addr.clone(),
        timeout: cfg.timeout,
        log: cfg.log.clone(),
    };
    let model = cfg.model.clone().or_else(|| {
        let t = http::get(&addr, "/v1/models", cfg.timeout).ok()?;
        serde_json::from_str::<Value>(&t)
            .ok()?
            .pointer("/data/0/id")?
            .as_str()
            .map(String::from)
    });
    let model = model.unwrap_or_else(|| "default".into());
    let sampler = Sampler::start(cfg.sources.clone());
    let started = chrono::Utc::now();
    let prefix = w::long_prefix(cfg.long_chars);
    let mut manifest = vec![
        cfg.shape.name().to_string(),
        model.clone(),
        format!("reps={}", cfg.reps),
    ];
    let (mut records, mut windows): (Vec<Value>, Vec<(f64, f64)>) = (vec![], vec![]);
    let mut state = "clean".to_string();
    let mut figures = serde_json::Map::new();
    let ident: String;
    match cfg.shape {
        Shape::OneStream => {
            let kinds: [(&str, String, u32); 4] = [
                ("prose", w::PROSE.to_string(), 256),
                ("code", w::CODE.to_string(), 384),
                ("json", w::JSON.to_string(), 256),
                ("long", format!("{prefix}\n\n{}", w::LONG_QUESTION), 128),
            ];
            let (mut ok_n, mut all_n) = (0, 0);
            'kinds: for (kind, body, max) in &kinds {
                manifest.push(format!("{kind}:{}:{max}", w::sha256_hex(body)));
                for rep in 0..=cfg.reps {
                    let (mut attempt, mut misses) = (0u32, 0u32);
                    let t_start = sampler.now_s();
                    let rec = loop {
                        let salt = if attempt == 0 {
                            w::salt(cfg.shape.name(), rep, 0)
                        } else {
                            format!(
                                "[profile salt: shape={} rep={rep} visit=0 attempt={attempt}]\n",
                                cfg.shape.name()
                            )
                        };
                        let prompt = format!("{salt}{body}");
                        match lone(&srv, &model, &prompt, *max, true) {
                            Ok(Ok(v)) => break Some(v),
                            Ok(Err(why)) => {
                                misses += 1;
                                attempt += 1;
                                if misses >= DISCARD_LIMIT {
                                    state = format!("production busy ({why})");
                                    break None;
                                }
                            }
                            Err(e) => {
                                state = format!("failed: {e}");
                                break None;
                            }
                        }
                    };
                    let Some(mut rec) = rec else { break 'kinds };
                    if rep > 0 {
                        windows.push((t_start, sampler.now_s()));
                    }
                    rec["kind"] = json!(kind);
                    rec["rep"] = json!(rep);
                    rec["counted"] = json!(rep > 0);
                    rec["attempts"] = json!(attempt + 1);
                    rec["prompt_sha256"] = json!(w::sha256_hex(body));
                    records.push(rec);
                }
                let counted: Vec<&Value> = records
                    .iter()
                    .filter(|r| r["kind"] == *kind && r["counted"] == true)
                    .collect();
                let (a, b) = identical(&counted);
                ok_n += a;
                all_n += b;
                figures.insert(kind.to_string(), kind_figures(&counted));
            }
            let mut idn = format!("{ok_n}/{all_n}");
            let all: Vec<&Value> = records.iter().filter(|r| r["counted"] == true).collect();
            figures.insert("all".into(), json!({"decode_tok_s": f::decode_tok_s(&col(&all, "generated"), &col(&all, "e2e_s"), &col(&all, "ttft_s"))}));
            if ok_n != all_n && all_n > 0 {
                idn = format!("{idn} (repetitions of one prompt differ)");
            }
            ident = idn;
        }
        Shape::FourStreams => {
            manifest.push(format!("prefix:{}", w::sha256_hex(&prefix)));
            for q in w::STREAM_QUESTIONS {
                manifest.push(format!("q:{}", w::sha256_hex(q)));
            }
            let mut per_rep: Vec<Value> = vec![];
            for rep in 0..=cfg.reps {
                let (mut attempt, mut misses) = (0u32, 0u32);
                let t_start = sampler.now_s();
                let rec = loop {
                    let salt = if attempt == 0 {
                        w::salt(cfg.shape.name(), rep, 0)
                    } else {
                        format!(
                            "[profile salt: shape=four-streams rep={rep} visit=0 attempt={attempt}]\n"
                        )
                    };
                    match four(&srv, &model, &format!("{salt}{prefix}\n\n"), cfg.timeout) {
                        Ok(Ok(v)) => break Some(v),
                        Ok(Err(why)) => {
                            misses += 1;
                            attempt += 1;
                            if misses >= DISCARD_LIMIT {
                                state = format!("production busy ({why})");
                                break None;
                            }
                        }
                        Err(e) => {
                            state = format!("failed: {e}");
                            break None;
                        }
                    }
                };
                let Some(mut rec) = rec else { break };
                if rep > 0 {
                    windows.push((t_start, sampler.now_s()));
                }
                rec["rep"] = json!(rep);
                rec["counted"] = json!(rep > 0);
                per_rep.push(rec);
            }
            let counted: Vec<&Value> = per_rep.iter().filter(|r| r["counted"] == true).collect();
            let stream_recs: Vec<&Value> = counted
                .iter()
                .flat_map(|r| r["streams"].as_array().unwrap().iter())
                .collect();
            figures.insert(
                "aggregate".into(),
                json!({
                    "span_tok_s": med_spread(&col(&counted, "span_tok_s")),
                    "wall_tok_s": med_spread(&col(&counted, "wall_tok_s")),
                    "decode_tok_s_per_stream": f::decode_tok_s(&col(&stream_recs, "generated"), &col(&stream_recs, "e2e_s"), &col(&stream_recs, "ttft_s")),
                    "per_stream_tok_s": med_spread(&col(&stream_recs, "decode_tok_s")),
                    "step_period_ms": {"mean": f::median(&col(&stream_recs, "step_period_mean_ms")), "max": col(&stream_recs, "step_period_max_ms").into_iter().fold(None, |a: Option<f64>, x| Some(a.map_or(x, |a| a.max(x))))},
                    "ttft_s": med_spread(&col(&stream_recs, "ttft_s")),
                    "ttft_source": counted.first().map(|r| r["ttft_source"].clone()),
                }),
            );
            // The base arm compared with itself: the same question gives the same text in every repetition?
            let mut same = 0;
            let mut total = 0;
            for qi in 0..4 {
                let hs: Vec<&Value> = stream_recs
                    .iter()
                    .filter(|s| s["question"] == qi)
                    .copied()
                    .collect();
                let (a, b) = identical(&hs);
                same += a;
                total += b;
            }
            ident = if same == total {
                format!("{same}/{total}")
            } else {
                "identity: batch-dependent".to_string()
            };
            for r in per_rep {
                let rep = r["rep"].clone();
                let counted = r["counted"].clone();
                for mut s in r["streams"].as_array().cloned().unwrap_or_default() {
                    s["rep"] = rep.clone();
                    s["counted"] = counted.clone();
                    s["kind"] = json!("long");
                    records.push(s);
                }
            }
        }
    }
    let samples = sampler.stop();
    let overall = machine::summarise(&samples, None);
    let mut throttled = 0;
    for (a, b) in &windows {
        throttled |= machine::summarise(&samples, Some((*a, *b))).throttle_mask_seen;
    }
    if throttled != 0 && state == "clean" {
        state = format!("throttled (reason mask {throttled:#x})");
    }
    manifest.sort();
    let result = json!({
        "schema": 1,
        "build": {"ragon_version": cfg.version, "engine": "n/a", "model": model, "drafter": "n/a"},
        "flags": "production",
        "shape": {"name": cfg.shape.name(), "reps_counted": cfg.reps, "warmup_reps": 1, "streams": if cfg.shape == Shape::FourStreams { 4 } else { 1 }, "long_prefix_chars": cfg.long_chars},
        "workload_sha256": w::sha256_hex(&manifest.join("\n")),
        "window": "no window",
        "started_utc": started.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "machine": {"node": cfg.node, "summary": overall, "sm_clock_normal_mhz": null},
        "state": state,
        "identical": ident,
        "figures": Value::Object(figures),
        "verdict": if state == "clean" { "no verdict (single arm; compare needs two runs)".to_string() } else { format!("no verdict ({state})") },
    });
    Ok(RunResult {
        result,
        requests: records,
        machine: samples,
        state,
    })
}

/// Four sessions at once on one prefix, distinct questions; per-request figures from the request lines.
fn four(
    srv: &Server,
    model: &str,
    head: &str,
    timeout: Duration,
) -> Result<Result<Value, String>, String> {
    let before = srv.metrics()?;
    if before.get(m::RUNNING) + before.get(m::QUEUED) > 0.0 {
        return Ok(Err("production busy before the request".into()));
    }
    let off = srv.log_len();
    let barrier = Arc::new(Barrier::new(4));
    let t0 = Instant::now();
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let (b, addr, model) = (barrier.clone(), srv.addr.clone(), model.to_string());
            let prompt = format!("{head}{}", w::STREAM_QUESTIONS[i]);
            std::thread::spawn(move || {
                b.wait();
                stream_request(&addr, &model, &prompt, 192, timeout)
            })
        })
        .collect();
    let mut results = vec![];
    for h in handles {
        results.push(
            h.join()
                .map_err(|_| "a stream thread panicked".to_string())??,
        );
    }
    let after = srv.settled(&before, 4.0)?;
    let counters = before.has(m::E2E_COUNT);
    if counters && after.delta(&before, m::E2E_COUNT) != 4.0 {
        return Ok(Err(format!(
            "{} requests arrived in the interval",
            after.delta(&before, m::E2E_COUNT)
        )));
    }
    let lines = srv.lines_since(off);
    let rel = |t: Instant| t.duration_since(t0).as_secs_f64();
    let firsts: Vec<f64> = results.iter().map(|r| rel(r.chunks[0].0)).collect();
    let lasts: Vec<f64> = results
        .iter()
        .map(|r| rel(r.chunks[r.chunks.len() - 1].0))
        .collect();
    let last_first = firsts.iter().cloned().fold(f64::MIN, f64::max);
    let last = lasts.iter().cloned().fold(f64::MIN, f64::max);
    let submit0 = results
        .iter()
        .map(|r| rel(r.submit))
        .fold(f64::MAX, f64::min);
    let gen_total = if counters {
        after.delta(&before, m::GENERATED)
    } else {
        0.0
    };
    let mut tokens_after = 0.0;
    let mut gens = vec![];
    let mut streams = vec![];
    let use_lines = lines.len() == 4;
    let mut used = vec![false; lines.len()];
    for (i, r) in results.iter().enumerate() {
        let g = r.usage_completion.unwrap_or(r.chunks.len() as u64);
        gens.push(g);
        let lens: Vec<usize> = r.chunks.iter().map(|c| c.1).collect();
        let per = f::tokens_per_chunk(&lens, g);
        tokens_after += r
            .chunks
            .iter()
            .zip(&per)
            .filter(|(c, _)| rel(c.0) > last_first)
            .map(|(_, k)| *k as f64)
            .sum::<f64>();
        let ev: Vec<(f64, u64)> = r
            .chunks
            .iter()
            .zip(&per)
            .map(|(c, k)| (rel(c.0), *k))
            .collect();
        let gaps = f::gaps_ms(&ev);
        // Pair each stream with an unused line of the same token count (equal counts pair in any order; the pooled figure does not depend on it).
        let pick = use_lines
            .then(|| (0..lines.len()).find(|j| !used[*j] && lines[*j].generated == g))
            .flatten();
        if let Some(j) = pick {
            used[j] = true;
        }
        let line = pick.map(|j| lines[j].clone());
        let client_dec =
            (g > 1 && lasts[i] > firsts[i]).then(|| (g as f64 - 1.0) / (lasts[i] - firsts[i]));
        let (ttft, e2e, ttft_src) = match &line {
            Some(l) => (
                l.prefill_s,
                l.prefill_s + l.decode_s,
                "request line (prefill only; queue wait is not on the line)",
            ),
            None => (
                firsts[i] - rel(r.submit),
                lasts[i] - rel(r.submit),
                "client clock",
            ),
        };
        streams.push(json!({
            "kind": "long", "question": i, "generated": g, "ttft_s": ttft, "e2e_s": e2e, "ttft_source": ttft_src,
            "client_ttft_s": firsts[i] - rel(r.submit), "client_e2e_s": lasts[i] - rel(r.submit),
            "decode_tok_s": line.as_ref().map(|l| l.decode_tok_s).or(client_dec),
            "prompt_tokens": line.as_ref().map(|l| l.prompt_tokens), "reused_tokens": line.as_ref().map(|l| l.reused_tokens),
            "prefill_s": line.as_ref().map(|l| l.prefill_s),
            "step_period_mean_ms": line.as_ref().and_then(|l| l.step_period_mean_ms),
            "step_period_max_ms": line.as_ref().and_then(|l| l.step_period_max_ms),
            "verify_passes": line.as_ref().map(|l| l.passes),
            "gaps_ms": gaps, "out_sha256": w::sha256_hex(&r.text),
        }));
    }
    let gen_sum: u64 = gens.iter().sum();
    Ok(Ok(json!({
        "span_tok_s": f::aggregate_span(tokens_after, last_first, last),
        "wall_tok_s": f::aggregate_wall(gen_sum as f64, submit0, last),
        "generated_total_client": gen_sum, "generated_total_server": gen_total,
        "ttft_source": if use_lines { "request line (prefill only)" } else { "client clock" },
        "streams": streams,
    })))
}
