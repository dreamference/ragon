//! compare and baseline on fixture result.json files under a temp HOME-like results root.
use ragon_profile::compare::{self, Verdict, judge};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ragon-profile-cmp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture(root: &Path, name: &str, decode: f64, ttft: f64, spread: f64, state: &str) {
    let r = json!({
        "schema": 1,
        "shape": {"name": "one-stream"},
        "workload_sha256": "w1",
        "machine": {"node": "n1"},
        "state": state,
        "figures": {
            "all": {"decode_tok_s": decode},
            "prose": {"ttft_s": {"median": ttft, "spread_pct": spread, "n": 3}},
        },
    });
    let d = root.join("runs").join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("result.json"), serde_json::to_string_pretty(&r).unwrap()).unwrap();
}

fn verdict(root: &Path, a: &str, b: &str) -> (Verdict, String) {
    let load = |n: &str| -> Value {
        serde_json::from_str(&std::fs::read_to_string(root.join("runs").join(n).join("result.json")).unwrap()).unwrap()
    };
    let o = compare::compare(&load(a), &load(b), a, b).unwrap();
    (o.verdict, o.text)
}

#[test]
fn rule_thresholds() {
    // floor 0.5 %: 0.5 is within noise, 1.0 is in between, 2.1 is a regression
    assert_eq!(judge(0.5, 0.1, 0.2).0, Verdict::NoChange);
    assert_eq!(judge(1.0, 0.1, 0.2).0, Verdict::Unclear);
    assert_eq!(judge(2.1, 0.1, 0.2).0, Verdict::Regression);
    // noise 3 %: 5 % is in between (bar is 6 %), 6.5 % is a regression, -6.5 % is faster
    assert_eq!(judge(5.0, 3.0, 1.0).0, Verdict::Unclear);
    assert_eq!(judge(6.5, 3.0, 1.0).0, Verdict::Regression);
    assert_eq!(judge(-6.5, 3.0, 1.0).0, Verdict::Faster);
    assert_eq!(judge(1.0, 0.0, 0.0).1, 0.5);
}

#[test]
fn within_noise() {
    let r = tmp("noise");
    fixture(&r, "a", 100.0, 1.00, 0.3, "clean");
    fixture(&r, "b", 99.8, 1.003, 0.4, "clean");
    let (v, t) = verdict(&r, "a", "b");
    assert_eq!(v, Verdict::NoChange, "{t}");
    assert!(t.contains("verdict: no change") && !t.contains("ABBA"));
}

#[test]
fn regression() {
    let r = tmp("regr");
    fixture(&r, "a", 100.0, 1.00, 0.3, "clean");
    fixture(&r, "b", 100.0, 1.06, 0.3, "clean"); // TTFT 6 % worse
    let (v, t) = verdict(&r, "a", "b");
    assert_eq!(v, Verdict::Regression, "{t}");
    fixture(&r, "c", 94.0, 1.00, 0.3, "clean"); // decode 6 % lower
    assert_eq!(verdict(&r, "a", "c").0, Verdict::Regression);
}

#[test]
fn in_between_asks_for_another_pair() {
    let r = tmp("mid");
    fixture(&r, "a", 100.0, 1.00, 0.3, "clean");
    fixture(&r, "b", 98.5, 1.00, 0.3, "clean"); // 1.5 % lower: above noise, below 2 %
    let (v, t) = verdict(&r, "a", "b");
    assert_eq!(v, Verdict::Unclear, "{t}");
    assert!(t.contains("one more ABBA pair") && t.contains("ragon profile run one-stream"));
}

#[test]
fn faster_and_unclean_runs() {
    let r = tmp("fast");
    fixture(&r, "a", 100.0, 1.00, 0.3, "clean");
    fixture(&r, "b", 110.0, 0.9, 0.3, "clean");
    assert_eq!(verdict(&r, "a", "b").0, Verdict::Faster);
    fixture(&r, "t", 50.0, 2.0, 0.3, "throttled (reason mask 0x4)");
    let (_, t) = verdict(&r, "a", "t");
    assert!(t.starts_with("no verdict"), "{t}");
}

#[test]
fn baseline_round_trip() {
    let r = tmp("base");
    fixture(&r, "20261108T100000Z-one-stream", 100.0, 1.0, 0.3, "clean");
    fixture(&r, "dirty", 100.0, 1.0, 0.3, "production busy");
    assert_eq!(compare::baseline_get(&r, "n1", "one-stream"), None);
    assert!(compare::baseline_show(&r, "n1", None).contains("one-stream: none"));
    assert!(compare::baseline_set(&r, "dirty").is_err());
    compare::baseline_set(&r, "20261108T100000Z-one-stream").unwrap();
    assert_eq!(
        compare::baseline_get(&r, "n1", "one-stream").as_deref(),
        Some("20261108T100000Z-one-stream")
    );
    assert!(compare::baseline_show(&r, "n1", Some("one-stream")).contains("20261108T100000Z-one-stream"));
    assert_eq!(compare::baseline_get(&r, "other", "one-stream"), None);
    // through the CLI, with `baseline` as the base
    let args = |v: &[&str]| -> Vec<String> {
        let mut a: Vec<String> = v.iter().map(|s| s.to_string()).collect();
        a.extend(["--out".into(), r.to_string_lossy().to_string()]);
        a
    };
    fixture(&r, "cand", 100.1, 1.0, 0.3, "clean");
    assert_eq!(ragon_profile::cli::run(&args(&["compare", "baseline", "cand"])), 0);
    fixture(&r, "slow", 90.0, 1.0, 0.3, "clean");
    assert_eq!(ragon_profile::cli::run(&args(&["compare", "baseline", "slow"])), 1);
}
