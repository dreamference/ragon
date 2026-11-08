//! `profile gemm` parsing and `profile trace` figures, with fixed text (no GPU, no server).
use ragon_profile::figures::{bandwidth_use_pct, speculation_gain};
use ragon_profile::{gemm, trace};

#[test]
fn gemm_lines_are_read_per_row_count_against_the_peak() {
    let text =
        "stream peak: 250.0 GB/s\nqkv M=1 100.0 GB/s\nqkv M=16 200.0 GB/s\nqkv M=7 9.0 GB/s\n";
    let r = gemm::parse(text, &[1, 16]);
    assert_eq!(r["peak_gb_s"].as_f64(), Some(250.0));
    assert_eq!(r["rows"]["16"][0]["gb_s"].as_f64(), Some(200.0));
    assert!(r["rows"].get("7").is_none());
    assert!(gemm::table(&r).contains("| 16 | qkv | 200.0 | 80.0 |"));
}

#[test]
fn trace_gives_percentiles_longest_gain_and_bandwidth() {
    let on = "{\"t\":1,\"kind\":\"verify\",\"tokens\":3,\"ms\":10}\n{\"t\":2,\"kind\":\"verify\",\"tokens\":3,\"ms\":30}\nnot json\n{\"t\":3,\"kind\":\"prefill\",\"ms\":99}\n";
    let off = "{\"t\":1,\"kind\":\"decode\",\"tokens\":1,\"ms\":10}\n";
    let (a, b) = (trace::parse(on), trace::parse(off));
    assert_eq!(a.len(), 3);
    let s = trace::summarise(&a);
    assert_eq!(s["decode_steps"], 2);
    assert_eq!(s["longest"][0]["ms"].as_f64(), Some(99.0));
    // 6 tokens in 40 ms = 150 tok/s against 100 tok/s.
    assert_eq!(
        speculation_gain(
            trace::decode_tok_s(&a).unwrap(),
            trace::decode_tok_s(&b).unwrap()
        ),
        Some(1.5)
    );
    // 10 GB per 40 ms = 250 GB/s of a 500 GB/s peak.
    assert_eq!(bandwidth_use_pct(10e9, 0.04, 500.0), Some(50.0));
}

#[test]
fn trace_writes_into_runs_and_gemm_refuses_without_the_bench() {
    let d = std::env::temp_dir().join(format!("prof-trace-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("t.jsonl");
    std::fs::write(&f, "{\"t\":1,\"kind\":\"verify\",\"tokens\":2,\"ms\":8}\n").unwrap();
    let args: Vec<String> = [f.to_str().unwrap(), "--out", d.to_str().unwrap()]
        .map(String::from)
        .to_vec();
    assert_eq!(trace::run(&args), 0);
    let runs: Vec<_> = std::fs::read_dir(d.join("runs"))
        .unwrap()
        .flatten()
        .collect();
    assert!(runs[0].file_name().to_string_lossy().ends_with("-trace"));
    assert!(runs[0].path().join("table.md").is_file());
    let g: Vec<String> = [
        "gemm",
        "--bench",
        "/nonexistent/bench",
        "--out",
        d.to_str().unwrap(),
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(gemm::run(&g), 1);
    std::fs::remove_dir_all(&d).ok();
}
