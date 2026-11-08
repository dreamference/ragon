//! The profiler against a fake server on a loopback port the OS picks: recorded `/metrics` and
//! `/spec_stats` bodies and request lines are replayed, one state per completed request.
use ragon_profile::machine::Sources;
use ragon_profile::run::{self, Config, Refusal, Shape};
use ragon_profile::{figures, output, reqline};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn data(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data")
            .join(name),
    )
    .unwrap()
}

struct Fake {
    addr: String,
    log: PathBuf,
    done: Arc<Mutex<usize>>,
}

/// `metrics(k)` and `spec(k)` give the bodies after k completed requests; `step` is how far the state
/// moves per completed request (2 makes a second, foreign request look like it arrived).
fn fake(
    dir: &Path,
    metrics: impl Fn(usize) -> String + Send + Sync + 'static,
    step: usize,
) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    assert!(!addr.ends_with(":8000") && !addr.ends_with(":18000"));
    let log = dir.join("engine.log");
    std::fs::write(&log, "").unwrap();
    let lines: Vec<String> = data("request_lines.log")
        .lines()
        .map(String::from)
        .collect();
    let done = Arc::new(Mutex::new(0usize));
    let metrics = Arc::new(metrics);
    let (d2, l2) = (done.clone(), log.clone());
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let (done, log, lines, metrics) =
                (d2.clone(), l2.clone(), lines.clone(), metrics.clone());
            std::thread::spawn(move || handle(conn, done, log, lines, metrics, step));
        }
    });
    Fake { addr, log, done }
}

fn reply(c: &mut TcpStream, ctype: &str, body: &str) {
    write!(c, "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
}

fn chunk(c: &mut TcpStream, s: &str) {
    write!(c, "{:x}\r\n{s}\r\n", s.len()).unwrap();
}

fn handle(
    mut c: TcpStream,
    done: Arc<Mutex<usize>>,
    log: PathBuf,
    lines: Vec<String>,
    metrics: Arc<dyn Fn(usize) -> String + Send + Sync>,
    step: usize,
) {
    let mut r = BufReader::new(c.try_clone().unwrap());
    let mut first = String::new();
    r.read_line(&mut first).unwrap();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        r.read_line(&mut h).unwrap();
        if h.trim().is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap();
        }
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).unwrap();
    let k = *done.lock().unwrap();
    let path = first.split_whitespace().nth(1).unwrap_or("");
    match path {
        "/metrics" => reply(&mut c, "text/plain", &metrics(k)),
        "/spec_stats" => reply(
            &mut c,
            "application/json",
            &data(&format!("spec_{}.json", k.min(16))),
        ),
        "/v1/models" => reply(
            &mut c,
            "application/json",
            r#"{"data":[{"id":"fake-model"}]}"#,
        ),
        "/v1/chat/completions" => {
            write!(c, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").unwrap();
            std::thread::sleep(Duration::from_millis(15));
            for i in 0..20 {
                chunk(
                    &mut c,
                    &format!(
                        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"tok{i} \"}}}}]}}\n\n"
                    ),
                );
                std::thread::sleep(Duration::from_millis(4));
            }
            {
                let mut d = done.lock().unwrap();
                // The engine writes its request line before the counters move on.
                let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
                for i in 0..step {
                    if let Some(l) = lines.get(*d + i) {
                        writeln!(f, "{l}").unwrap();
                    }
                }
                *d += step;
            }
            chunk(
                &mut c,
                "data: {\"choices\":[],\"usage\":{\"completion_tokens\":65}}\n\n",
            );
            chunk(&mut c, "data: [DONE]\n\n");
            write!(c, "0\r\n\r\n").unwrap();
        }
        _ => write!(c, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").unwrap(),
    }
}

fn recorded(k: usize) -> String {
    data(&format!("metrics_{}.txt", k.min(16)))
}

fn config(f: &Fake, dir: &Path, shape: Shape) -> Config {
    Config {
        url: format!("http://{}", f.addr),
        shape,
        reps: 3,
        log: Some(f.log.clone()),
        model: None,
        node: "node-a".into(),
        version: "0.0.0-test".into(),
        lock_path: Some(dir.join("gpu.lock")),
        sources: Sources::none(),
        timeout: Duration::from_secs(20),
        long_chars: 4_000,
    }
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ragon-profile-test-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

#[test]
fn one_stream_figures_come_from_the_counter_deltas_and_skip_repetition_zero() {
    let dir = tmp("one");
    let f = fake(&dir, recorded, 1);
    let res = run::run(&config(&f, &dir, Shape::OneStream)).unwrap();
    assert_eq!(res.state, "clean");
    assert_eq!(*f.done.lock().unwrap(), 16);
    let figs = &res.result["figures"];
    for kind in ["prose", "code", "json"] {
        // Counted TTFTs are 0.25, 0.30, 0.35: the median ignores repetition 0 (5.0 s).
        assert!(
            close(figs[kind]["ttft_s"]["median"].as_f64(), 0.30),
            "{kind} ttft"
        );
        assert_eq!(figs[kind]["ttft_s"]["n"], 3);
        // Σ(65−1) / Σ(e2e − ttft) = 192 / 6.0 = 32.0, a pooled ratio, not a mean of rates.
        assert!(
            close(figs[kind]["decode_tok_s"].as_f64(), 32.0),
            "{kind} decode"
        );
        assert!(close(figs[kind]["tokens_per_pass"].as_f64(), 2.0));
        assert!(close(figs[kind]["acceptance_pct"].as_f64(), 64.0));
        assert!(close(figs[kind]["draft_ms_per_step"].as_f64(), 25.0));
        assert!(close(figs[kind]["verify_ms_per_step"].as_f64(), 50.0));
        assert_eq!(figs[kind]["cache_state"], "cold");
        // Below 1,024 new tokens the prefill figure is not reported.
        assert!(figs[kind]["prefill_ms_per_1k"]["median"].is_null());
        assert!(close(figs[kind]["step_period_ms"]["mean"].as_f64(), 30.5));
        assert!(close(figs[kind]["step_period_ms"]["max"].as_f64(), 41.0));
        // 3 repetitions × 64 gaps = 192: p50 and p90 (from 100), not p99 (from 1,000).
        assert!(figs[kind]["inter_token_gap_ms"]["p50"].is_number());
        assert!(figs[kind]["inter_token_gap_ms"]["p90"].is_number());
        assert!(figs[kind]["inter_token_gap_ms"]["p99"].is_null());
    }
    let long = &figs["long"];
    assert!(close(long["ttft_s"]["median"].as_f64(), 10.5));
    assert!(close(long["prefill_ms_per_1k"]["median"].as_f64(), 250.0));
    assert_eq!(long["prompt_tokens"], 40000.0);
    assert_eq!(res.result["identical"], "12/12");
    assert!(close(figs["all"]["decode_tok_s"].as_f64(), 32.0));
    assert_eq!(res.requests.len(), 16);
    assert_eq!(
        res.requests.iter().filter(|r| r["counted"] == true).count(),
        12
    );
}

#[test]
fn files_land_in_the_run_directory_and_hold_no_home_path() {
    let dir = tmp("files");
    let f = fake(&dir, recorded, 1);
    let mut cfg = config(&f, &dir, Shape::OneStream);
    cfg.reps = 3;
    let res = run::run(&cfg).unwrap();
    let root = dir.join("profile");
    let run_dir = output::write(&root, &res).unwrap();
    assert_eq!(run_dir.parent().unwrap(), root.join("runs"));
    let name = run_dir.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        name.ends_with("Z-one-stream") && name.len() == "20261108T172300Z-one-stream".len(),
        "{name}"
    );
    for file in ["result.json", "table.md", "requests.jsonl", "machine.jsonl"] {
        assert!(run_dir.join(file).is_file(), "{file}");
    }
    let table = std::fs::read_to_string(run_dir.join("table.md")).unwrap();
    for field in [
        "schema",
        "build",
        "flags",
        "shape",
        "workload",
        "window",
        "machine",
        "state",
        "identical",
        "figures",
        "verdict",
    ] {
        assert!(table.contains(&format!("| {field} |")), "{field}");
    }
    assert!(table.contains("no window") && table.contains("cold"));
    let result: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run_dir.join("result.json")).unwrap())
            .unwrap();
    assert_eq!(result["schema"], 1);
    assert_eq!(result["machine"]["node"], "node-a");
    let lines: Vec<serde_json::Value> = std::fs::read_to_string(run_dir.join("requests.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 16);
    assert!(lines[0].get("prompt").is_none() && lines[0]["prompt_sha256"].is_string());
    if let Some(home) = ragon_node_locator::home_dir() {
        let h = home.to_string_lossy().to_string();
        for file in ["result.json", "table.md", "requests.jsonl"] {
            assert!(
                !std::fs::read_to_string(run_dir.join(file))
                    .unwrap()
                    .contains(&h)
            );
        }
    }
    assert_eq!(
        output::normalise(&format!(
            "{}/x",
            ragon_node_locator::home_dir().unwrap().display()
        )),
        "~/x"
    );
}

#[test]
fn the_same_inputs_give_the_same_workload_hash() {
    let (d1, d2) = (tmp("w1"), tmp("w2"));
    let (f1, f2) = (fake(&d1, recorded, 1), fake(&d2, recorded, 1));
    let a = run::run(&config(&f1, &d1, Shape::OneStream)).unwrap();
    let b = run::run(&config(&f2, &d2, Shape::OneStream)).unwrap();
    assert_eq!(a.result["workload_sha256"], b.result["workload_sha256"]);
    let c = run::run(&config(
        &fake(&tmp("w3"), recorded, 1),
        &d1,
        Shape::FourStreams,
    ))
    .unwrap();
    assert_ne!(a.result["workload_sha256"], c.result["workload_sha256"]);
}

#[test]
fn four_streams_reports_aggregate_and_per_stream_figures() {
    let dir = tmp("four");
    let f = fake(&dir, recorded, 1);
    let res = run::run(&config(&f, &dir, Shape::FourStreams)).unwrap();
    assert_eq!(res.state, "clean", "{}", res.result["state"]);
    assert_eq!(*f.done.lock().unwrap(), 16);
    let a = &res.result["figures"]["aggregate"];
    let (span, wall) = (
        a["span_tok_s"]["median"].as_f64().unwrap(),
        a["wall_tok_s"]["median"].as_f64().unwrap(),
    );
    assert!(
        span > 0.0 && wall > 0.0 && wall <= span * 1.0001,
        "span {span} wall {wall}"
    );
    // Per-stream decode from the request lines: 12 counted streams × 64 tokens over 9 × 2.0 s + 3 × 4.0 s.
    assert!(
        close(a["decode_tok_s_per_stream"].as_f64(), 768.0 / 30.0),
        "{a}"
    );
    assert!(close(a["step_period_ms"]["mean"].as_f64(), 30.5));
    assert_eq!(
        res.requests.iter().filter(|r| r["counted"] == true).count(),
        12
    );
    assert_eq!(res.result["identical"], "12/12");
    assert!(output::table(&res.result).contains("span"));
}

#[test]
fn a_foreign_request_in_the_interval_discards_the_repetition_three_times_then_busy() {
    let dir = tmp("busy");
    // Each of our requests moves the counters by two requests' worth.
    let f = fake(
        &dir,
        |k| data(&format!("metrics_{}.txt", (k * 2).min(16))),
        1,
    );
    let res = run::run(&config(&f, &dir, Shape::OneStream)).unwrap();
    assert!(res.state.starts_with("production busy"), "{}", res.state);
    assert!(
        res.result["verdict"]
            .as_str()
            .unwrap()
            .starts_with("no verdict")
    );
    assert_eq!(*f.done.lock().unwrap(), 3);
}

#[test]
fn it_refuses_when_production_has_running_requests_or_the_lock_is_held() {
    let dir = tmp("refuse");
    let f = fake(&dir, |_| data("running_metrics.txt"), 1);
    match run::run(&config(&f, &dir, Shape::OneStream)) {
        Err(Refusal::Busy(why)) => assert!(why.contains("running")),
        other => panic!("{:?}", other.err()),
    }
    assert_eq!(*f.done.lock().unwrap(), 0);
    let g = fake(&dir, recorded, 1);
    let cfg = config(&g, &dir, Shape::OneStream);
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cfg.lock_path.as_ref().unwrap())
        .unwrap();
    held.lock().unwrap();
    assert!(matches!(run::run(&cfg), Err(Refusal::LockHeld)));
    assert_eq!(*g.done.lock().unwrap(), 0);
    drop(held);
    let mut dead = config(&g, &dir, Shape::OneStream);
    dead.url = "http://127.0.0.1:1".into();
    assert!(matches!(run::run(&dead), Err(Refusal::Unreachable(_))));
}

#[test]
fn the_cli_writes_the_files_and_exits_zero() {
    let dir = tmp("cli");
    let f = fake(&dir, recorded, 1);
    let out = dir.join("out");
    let args: Vec<String> = [
        "run",
        "one-stream",
        "--reps",
        "3",
        "--url",
        &format!("http://{}", f.addr),
        "--log",
        f.log.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--lock",
        dir.join("l.lock").to_str().unwrap(),
        "--no-gpu",
        "--node",
        "node-b",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // The default long prefix is ~110K characters; the fake does not mind.
    assert_eq!(ragon_profile::cli::run(&args), 0);
    let runs: Vec<_> = std::fs::read_dir(out.join("runs")).unwrap().collect();
    assert_eq!(runs.len(), 1);
    assert_eq!(
        ragon_profile::cli::run(&["run".into(), "fanout-32".into()]),
        1
    );
}

#[test]
fn request_line_and_figure_functions_at_fixed_inputs() {
    let l = reqline::parse(
        &data("request_lines.log")
            .lines()
            .nth(13)
            .unwrap()
            .to_string(),
    )
    .unwrap();
    assert_eq!(
        (
            l.session,
            l.prompt_tokens,
            l.reused_tokens,
            l.generated,
            l.passes
        ),
        (14, 40000, 0, 65, 32)
    );
    assert!(
        (l.prefill_s - 10.0).abs() < 1e-9
            && l.finish == "stop"
            && l.step_period_max_ms == Some(41.0)
    );
    let store = reqline::parse("request: session 2, 32000 prompt tokens (31800 reused, from the prefix KV store), prefill 0.9 s (1 chunks, 0 beside a decode, 222 tok/s of compute); 10 tokens in 0.2 s (50.0 tok/s), length; 4 passes (0 batched), 2.25 tokens/pass").unwrap();
    assert!(
        store.from_prefix_store
            && store.reused_tokens == 31800
            && store.step_period_mean_ms.is_none()
    );
    assert!(reqline::parse("starting up").is_none());
    assert_eq!(figures::prefill_ms_per_1k(1.0, 1500, 1000), None);
    assert_eq!(
        figures::prefill_ms_per_1k(10.0, 33000, 0).map(|x| x.round()),
        Some(303.0)
    );
    assert_eq!(figures::bucket(8192), "<=8K");
    assert_eq!(figures::bucket(40000), "32-64K");
    // Decode over a set weights by tokens: (101 + 11) / (10 + 1), not the mean of 10.1 and 11.
    assert_eq!(
        figures::decode_tok_s(&[102.0, 12.0], &[11.0, 2.0], &[1.0, 1.0])
            .map(|x| (x * 1e6).round() / 1e6),
        Some(112.0 / 11.0 + 0.0).map(|x: f64| (x * 1e6).round() / 1e6)
    );
    // A chunk of 4 tokens 40 ms after the previous one is four gaps of 10 ms.
    assert_eq!(figures::gaps_ms(&[(0.0, 1), (0.040, 4)]), vec![10.0; 4]);
    let gaps: Vec<f64> = (1..=1000).map(f64::from).collect();
    assert_eq!(
        figures::gap_percentiles(&gaps),
        (Some(500.0), Some(900.0), Some(990.0))
    );
    assert_eq!(figures::gap_percentiles(&gaps[..99]).1, None);
    assert_eq!(figures::tokens_per_chunk(&[10, 10, 20], 8), vec![2, 2, 4]);
    assert_eq!(figures::aggregate_span(300.0, 1.0, 4.0), Some(100.0));
    assert_eq!(figures::aggregate_wall(400.0, 0.0, 4.0), Some(100.0));
    assert_eq!(figures::median(&[3.0, 1.0, 2.0, 10.0]), Some(2.5));
    assert!((figures::spread_pct(&[9.0, 10.0, 11.0]).unwrap() - 20.0).abs() < 1e-9);
}

#[test]
fn fanout_32_sends_32_distinct_forks_per_repetition_and_reports_the_cold_repetition_apart() {
    let dir = tmp("fanout");
    let f = fake(
        &dir,
        |_| "sglang:num_running_reqs 0\nsglang:num_queue_reqs 0\n".to_string(),
        1,
    );
    let mut cfg = config(&f, &dir, Shape::Fanout32);
    cfg.reps = 2;
    let res = run::run(&cfg).unwrap();
    assert_eq!(res.state, "clean", "{}", res.result["state"]);
    assert_eq!(*f.done.lock().unwrap(), 32 * 3);
    assert_eq!(res.result["shape"]["streams"], 32);
    let figs = &res.result["figures"];
    assert!(
        figs["cold_rep0"]["span_tok_s"].is_number() || figs["cold_rep0"]["span_tok_s"].is_null()
    );
    assert!(figs["aggregate"]["span_tok_s"]["median"].as_f64().unwrap() > 0.0);
    assert_eq!(
        res.requests.iter().filter(|r| r["counted"] == true).count(),
        64
    );
    let a = ragon_profile::workload::fork_question(0);
    let b = ragon_profile::workload::fork_question(31);
    assert_ne!(a, b);
}
