//! Sampler, rotation, stalled sink and level-spec behaviour (LOGGING §4, §5.4, §6).

use ragon_log::{
    Decision, Env, Field, FileSink, Level, LogLevelSpec, Logger, Request, Rotation, Sampler, Sink, Synthetic,
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn env() -> Env {
    Env::new("/home/u", "h.example.test", "u")
}

struct Collect(Arc<Mutex<Vec<String>>>);
impl Sink for Collect {
    fn write(&mut self, _l: Level, line: &str, _now: u64) -> bool {
        self.0.lock().unwrap().push(line.to_string());
        true
    }
}

struct Stalled;
impl Sink for Stalled {
    fn write(&mut self, _l: Level, _line: &str, _now: u64) -> bool {
        std::thread::sleep(Duration::from_millis(50));
        true
    }
}

#[test]
fn a_thousand_identical_lines_in_a_second_keep_ten() {
    let mut s = Sampler::new(Level::Info);
    let mut written = 0;
    let mut synth = Vec::new();
    for i in 0..1000u64 {
        let now = 1_000_000 + i / 2;
        synth.extend(s.tick(now));
        if s.admit(now, Level::Info, "same message", None) == Decision::Write {
            written += 1;
        }
    }
    assert_eq!(written, 10);
    synth.extend(s.tick(1_002_000));
    assert!(synth.contains(&Synthetic::Suppressed { site: "same message".into(), count: 990 }), "{synth:?}");
}

#[test]
fn another_messages_error_still_passes() {
    let mut s = Sampler::new(Level::Info);
    for _ in 0..100 {
        s.admit(5_000, Level::Info, "noisy", None);
    }
    assert_eq!(s.admit(5_000, Level::Error, "different failure", None), Decision::Write);
}

#[test]
fn request_closing_lines_have_their_own_budget_and_a_summary() {
    let mut s = Sampler::new(Level::Info);
    let mut written = 0;
    s.tick(9_000);
    for i in 0..300 {
        if s.admit(9_000, Level::Info, "request done", Some(Request { ms: i as f64, error: false })) == Decision::Write
        {
            written += 1;
        }
    }
    assert_eq!(written, 200);
    let synth = s.tick(19_500);
    assert!(synth.iter().any(|x| matches!(x, Synthetic::Summary { count: 100, .. })), "{synth:?}");
}

#[test]
fn level_spec_parses_defaults_overrides_and_warns() {
    let spec = LogLevelSpec::parse("debug,engine=trace,gate=off,web=loud");
    assert_eq!(spec.level_for("engine"), Some(Level::Trace));
    assert_eq!(spec.level_for("gate"), None);
    assert_eq!(spec.level_for("web"), Some(Level::Debug));
    assert_eq!(spec.warnings.len(), 1, "{:?}", spec.warnings);
}

#[test]
fn rotation_keeps_five_files_with_old_ones_compressed_and_loses_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let rot = Rotation { max_bytes: 2000, keep: 5, max_age: Duration::from_secs(14 * 86_400) };
    let mut a = FileSink::new(dir.path(), "engine", rot);
    let mut b = FileSink::new(dir.path(), "engine", rot);
    let total = 120;
    for i in 0..total {
        let line = format!("2026-11-08T14:03:12.481Z engine INFO line n={i:04} {}", "x".repeat(60));
        let sink = if i % 2 == 0 { &mut a } else { &mut b };
        assert!(sink.write(Level::Info, &line, 1_000 + i as u64 * 1_100), "line {i} dropped");
    }
    let names: Vec<String> =
        std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert!(names.contains(&"engine.log".to_string()), "{names:?}");
    assert!(names.contains(&"engine.1.log".to_string()), "{names:?}");
    assert!(names.iter().any(|n| n.ends_with(".2.log.gz")), "{names:?}");
    assert!(!names.iter().any(|n| n.ends_with(".1.log.gz")), "{names:?}");
    assert!(!names.iter().any(|n| n.contains(".5.")), "{names:?}");
    // No line twice, and the newest lines are all present.
    let mut seen = Vec::new();
    for n in names.iter().filter(|n| n.ends_with(".log") && n.starts_with("engine")) {
        for l in std::fs::read_to_string(dir.path().join(n)).unwrap().lines() {
            seen.push(l.to_string());
        }
    }
    let mut uniq = seen.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(uniq.len(), seen.len(), "duplicated lines");
    assert!(seen.iter().any(|l| l.contains(&format!("n={:04}", total - 1))));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("engine.log")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn an_unwritable_directory_drops_counts_and_never_raises() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, b"x").unwrap();
    let mut s = FileSink::new(&file, "engine", Rotation::default());
    assert!(!s.write(Level::Info, "line", 1_000));
    assert!(!s.write(Level::Info, "line", 1_100));
    // Within the second after a failure the sink does not try again, and the count is pending later.
    assert!(s.pending_drop(1_200).is_none() || s.pending_drop(1_200).unwrap().0 >= 1);
    let p = s.pending_drop(5_000).expect("a drop report is pending");
    assert!(p.0 >= 2, "{p:?}");
}

#[test]
fn a_stalled_sink_never_blocks_the_caller() {
    let log = Logger::with_sink("engine", Some(Level::Info), env(), b"salt".to_vec(), Box::new(Stalled));
    let start = std::time::Instant::now();
    for i in 0..100_000i64 {
        log.error("tick failed", &[Field::count("n", i)]);
    }
    assert!(start.elapsed() < Duration::from_secs(5), "callers waited {:?}", start.elapsed());
    assert!(log.dropped() > 0, "a full queue must drop and count");
}

#[test]
fn a_logger_writes_formatted_lines_and_flushes() {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let log = Logger::with_sink("engine", Some(Level::Info), env(), b"salt".to_vec(), Box::new(Collect(lines.clone())));
    log.info("engine ready", &[Field::ms("ms", 12)]);
    log.debug("hidden", &[]);
    assert!(log.flush(Duration::from_secs(2)));
    let got = lines.lock().unwrap().clone();
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(got[0].ends_with(" engine INFO engine ready ms=12"), "{}", got[0]);
}
