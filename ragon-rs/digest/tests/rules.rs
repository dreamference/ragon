//! The rule steps measured on the digest check set, each against the digest as it was
//! ([`Rules::TODAY`]): a digest only when it is shorter, a trigger in bytes, long lines cut, the
//! header's facts, the first lines and the lines above the first error, the first error on top, and
//! a filter after a pipe that does not make a read.

use std::path::PathBuf;

use ragon_digest::select::{MAX_LINES, Rules, failing_tests};
use ragon_digest::{Digester, Options};

fn home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-digest-rules-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn digester(name: &str, min_lines: usize, rules: Rules) -> Digester {
    Digester::with_options(&home(name), Options { min_lines, ..Options::default() }).with_rules(rules)
}

fn text(digester: &Digester, command: &str, output: &str, exit_code: i32) -> String {
    String::from_utf8(digester.process(command, output.as_bytes().to_vec(), Some(exit_code))).unwrap()
}

/// A pytest run: a banner, passing tests, a traceback whose error is at line 152, two failures.
fn pytest_log() -> String {
    let mut out = String::from("============ test session starts ============\nplatform linux -- Python 3.11\nrootdir: /testbed\n");
    for i in 0..140 {
        out.push_str(&format!("tests/test_a.py::test_ok_{i} PASSED\n"));
    }
    out.push_str("    def test_broken():\n        value = compute(3)\n>       assert value == 4\n");
    out.push_str("E       assert 5 == 4\n");
    for i in 0..60 {
        out.push_str(&format!("tests/test_b.py::test_more_{i} PASSED\n"));
    }
    out.push_str("FAILED tests/test_a.py::test_broken - assert 5 == 4\nFAILED tests/test_b.py::test_other\n");
    out.push_str("======= 2 failed, 200 passed in 3.2s =======\n");
    out
}

#[test]
fn the_default_is_the_set_that_beat_truncation() {
    let rules = Rules::default();
    assert!(rules.shorter_only && rules.facts && rules.order && rules.fill && rules.producer);
    assert_eq!((rules.head, rules.context_before, rules.min_bytes, rules.max_line_chars), (3, 2, 8000, 400));
    assert_eq!(Rules::TODAY, Rules { shorter_only: false, head: 0, facts: false, context_before: 0, order: false, fill: false, producer: false, min_bytes: 0, max_line_chars: 0 });
}

#[test]
fn a_digest_that_is_not_shorter_hands_the_output_back() {
    // Eight short lines with an error: today's digest at a five-line trigger is longer than them.
    let output: String = (0..8).map(|i| if i == 4 { "error: broke\n".to_string() } else { format!("ok {i}\n") }).collect();
    let today = text(&digester("short-today", 5, Rules::TODAY), "make", &output, 2);
    assert!(today.starts_with("[digest]") && today.len() >= output.len(), "{today}");
    assert_eq!(text(&digester("short-rules", 5, Rules::default()), "make", &output, 2), output);
}

#[test]
fn a_few_wide_lines_are_digested_with_their_long_lines_cut() {
    let output: String = (0..10).map(|i| format!("row {i}: {}\n", "x".repeat(2000))).collect();
    assert_eq!(text(&digester("wide-today", 61, Rules::TODAY), "./dump", &output, 0), output);
    let digest = text(&digester("wide-rules", 61, Rules::default()), "./dump", &output, 0);
    assert!(digest.starts_with("[digest] 10 lines, exit code 0; no error lines."), "{digest}");
    assert!(digest.contains("… [line cut, 2007 characters]"), "{digest}");
    assert!(digest.len() < output.len() / 4, "{}", digest.len());
}

#[test]
fn the_header_names_the_errors_and_the_failing_tests() {
    let lines: Vec<String> = pytest_log().lines().map(str::to_string).collect();
    assert_eq!(failing_tests(&lines), ["test_broken", "test_other"]);
    let digest = text(&digester("facts", 61, Rules::default()), "cd /testbed && pytest -q", &pytest_log(), 1);
    let header = digest.lines().next().unwrap();
    assert!(header.contains("exit code 1; 4 error lines, the first at line 147; 2 failing tests: test_broken, test_other."), "{header}");
    for (line, name) in [("test a::b ... FAILED", "a::b"), ("FAIL: test_x (pkg.tests.T)", "test_x"), ("--- FAIL: TestY (0.00s)", "TestY")] {
        assert_eq!(failing_tests(&[line.to_string()]), [name], "{line}");
    }
}

#[test]
fn the_first_error_comes_first_with_the_lines_above_it_and_the_banner_stays() {
    let digest = text(&digester("order", 61, Rules::default()), "pytest", &pytest_log(), 1);
    assert!(digest.lines().count() <= MAX_LINES, "{digest}");
    let body: Vec<&str> = digest.lines().skip(1).collect();
    // The first error and the two lines above it, right under the header.
    assert_eq!(body[0].trim_start(), "145:         value = compute(3)", "{digest}");
    assert_eq!(body[1].trim_start(), "146: >       assert value == 4");
    assert_eq!(body[2].trim_start(), "147: E       assert 5 == 4");
    // The banner's first lines, and the last line last.
    assert!(digest.contains("  1: ============ test session starts"), "{digest}");
    assert!(digest.contains("  3: rootdir: /testbed"), "{digest}");
    assert!(digest.ends_with("======= 2 failed, 200 passed in 3.2s =======\n"), "{digest}");
    // Today: no lines above the error, and it comes after the repeat summary.
    let today = text(&digester("order-today", 61, Rules::TODAY), "pytest", &pytest_log(), 1);
    assert!(!today.contains("146: >       assert value == 4"), "{today}");
    assert!(!today.contains("  1: ===="), "{today}");
}

#[test]
fn a_test_run_piped_to_a_filter_is_digested() {
    let command = "cd /testbed && python -m pytest tests 2>&1 | tail -n 300";
    assert_eq!(text(&digester("pipe-today", 61, Rules::TODAY), command, &pytest_log(), 1), pytest_log());
    assert!(text(&digester("pipe-rules", 61, Rules::default()), command, &pytest_log(), 1).starts_with("[digest]"));
    let read = "cat tests/test_a.py | head -n 300";
    assert_eq!(text(&digester("pipe-read", 61, Rules::default()), read, &pytest_log(), 0), pytest_log());
}
