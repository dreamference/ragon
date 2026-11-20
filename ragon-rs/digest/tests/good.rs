//! The last good run (owner's answer 2 of 2026-11-20): off by default; on, a digest leaves out the
//! error lines and their next lines that the last successful run of the same command printed too,
//! names them by line number, keeps the last lines, and forgets a run older than a day.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use ragon_digest::{Digester, Options, good};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-digest-good-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `i` written in letters, so that distinct numbers stay distinct lines after digits are masked.
fn letters(mut i: usize) -> String {
    let mut out = String::new();
    loop {
        out.push(char::from(b'a' + (i % 26) as u8));
        i /= 26;
        if i == 0 {
            return out;
        }
    }
}

const NOISE: &str = "ERROR root: retrying the cache server (attempt 3 of 5)";
const CAUSE: &str = "error[E0308]: mismatched types in src/ledger.rs";

/// A build log of `lines` lines with the noise error every 25 lines before the last 30, at
/// 20:15:0x, and `cause` once.
fn log(lines: usize, cause: bool) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..lines {
        if i % 25 == 5 && i + 30 < lines {
            out.push_str(&format!("2026-11-20T20:15:0{}Z {NOISE}\n", i % 10));
        } else if cause && i == lines / 2 {
            out.push_str(&format!("{CAUSE}\n"));
        } else {
            out.push_str(&format!("   Compiling crate{i} v0.1.{}\n", i % 7));
        }
    }
    out.into_bytes()
}

fn digester(home: &Path, last_good: bool) -> Digester {
    Digester::with_options(home, Options { last_good, ..Options::default() })
}

fn run(digester: &Digester, command: &str, output: Vec<u8>, exit_code: i32) -> String {
    String::from_utf8(digester.process(command, output, Some(exit_code))).unwrap()
}

#[test]
fn off_by_default_nothing_is_sampled_and_nothing_left_out() {
    assert!(!Options::default().last_good);
    let home = home("off");
    let digester = Digester::new(&home, ragon_digest::MIN_LINES);
    run(&digester, "cargo build", log(400, false), 0);
    assert!(!digester.good_dir().exists());
    let digest = run(&digester, "cargo build", log(400, true), 101);
    assert!(digest.contains(NOISE), "{digest}");
    assert!(digest.contains(CAUSE), "{digest}");
}

#[test]
fn lines_the_last_good_run_printed_are_named_by_number_and_the_cause_stays() {
    let home = home("on");
    let digester = digester(&home, true);
    run(&digester, "cargo build", log(400, false), 0);
    let digest = run(&digester, "cargo build", log(400, true), 101);
    assert!(digest.contains(CAUSE), "{digest}");
    let shown_noise = digest.lines().filter(|line| line.contains(NOISE)).count();
    assert_eq!(shown_noise, 0, "{digest}");
    assert!(digest.contains("15 error lines also printed by the last successful run of this command, not shown: at lines 6, 31,"), "{digest}");
    let last = String::from_utf8(log(400, true)).unwrap().lines().last().unwrap().to_string();
    assert!(digest.trim_end().ends_with(&last), "the last lines stay: {digest}");
}

#[test]
fn a_short_good_run_counts_and_a_failed_or_other_command_does_not() {
    let home = home("short");
    let digester = digester(&home, true);
    // A failed run is never the last good run.
    run(&digester, "cargo build", log(400, false), 1);
    assert!(run(&digester, "cargo build", log(400, true), 101).contains(NOISE));
    // A good run shorter than the threshold is still sampled.
    run(&digester, "cargo build", format!("{NOISE}\nFinished\n").into_bytes(), 0);
    assert!(!run(&digester, "cargo build", log(400, true), 101).lines().any(|line| line.ends_with(NOISE)));
    // Another command has its own last good run.
    assert!(run(&digester, "cargo build --release", log(400, true), 101).contains(NOISE));
}

#[test]
fn a_good_run_older_than_a_day_is_forgotten_and_the_samples_are_bounded() {
    let home = home("old");
    let digester = digester(&home, true);
    run(&digester, "cargo build", log(400, false), 0);
    let file = good::path(&digester.good_dir(), "cargo build");
    let day_ago = SystemTime::now() - ragon_digest::KEEP_FOR - Duration::from_secs(60);
    std::fs::File::options().append(true).open(&file).unwrap().set_modified(day_ago).unwrap();
    assert!(run(&digester, "cargo build", log(400, true), 101).contains(NOISE));

    let huge: String = (0..good::MAX_SAMPLES * 3).map(|i| format!("{}\n", letters(i))).collect();
    run(&digester, "make", huge.into_bytes(), 0);
    let size = good::path(&digester.good_dir(), "make").metadata().unwrap().len();
    assert_eq!(size, (good::MAX_SAMPLES * 8) as u64);
}
