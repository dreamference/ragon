//! The four Windows CI failures of 2026-11 (gh runs 44688217465, 44695421308, 44705071695,
//! 44717146852), with GitHub's line prefixes removed: every line that says what failed survives
//! the digest, under the line number it has in the raw output.

use std::path::Path;

use ragon_digest::select::{MAX_LINES, digest};
use ragon_digest::text;

fn digest_of(name: &str) -> (Vec<String>, String) {
    let raw = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap();
    let lines = text::lines(&raw);
    let out = digest(&lines, Some(1), Path::new("/tmp/digest/raw/0123456789abcdef.log"));
    (lines, out)
}

/// Every numbered line of the digest is the raw output's line of that number, cleaned.
fn numbers_hold(lines: &[String], out: &str) {
    for line in out.lines().filter(|line| !line.starts_with("[digest]")) {
        let (number, text) = line.split_once(": ").expect(line);
        let number: usize = number.trim().parse().expect(line);
        assert_eq!(lines[number - 1], text, "line {number}");
    }
}

fn check(name: &str, needles: &[&str]) {
    let (lines, out) = digest_of(name);
    assert!(out.lines().count() <= MAX_LINES, "{name}: {} lines\n{out}", out.lines().count());
    assert!(out.starts_with(&format!("[digest] {} lines, exit code 1.", lines.len())), "{out}");
    for needle in needles {
        assert!(out.contains(needle), "{name}: {needle:?} missing from\n{out}");
    }
    numbers_hold(&lines, &out);
    for tail in &lines[lines.len().saturating_sub(20)..] {
        assert!(out.contains(tail.as_str()), "{name}: last line {tail:?} missing");
    }
}

#[test]
fn a_missing_python_module() {
    check(
        "windows-44688217465.log",
        &["Traceback (most recent call last):", "ModuleNotFoundError: No module named 'fcntl'", "##[error]Process completed with exit code 1."],
    );
}

#[test]
fn a_rust_compile_error() {
    check(
        "windows-44695421308.log",
        &[
            "error[E0433]: cannot find module or crate `ragon_chat` in this scope",
            "error: could not compile `ragon-launcher` (lib) due to 3 previous errors",
            "The ragon build failed",
        ],
    );
}

#[test]
fn a_rust_test_failure_among_passing_tests_named_after_failures() {
    check(
        "windows-44705071695.log",
        &[
            "test pair_prints_a_code_for_the_signed_in_user_and_leaves_the_bridge_its_primary ... FAILED",
            "panicked at ragon\\tests\\auth_commands.rs:236:5:",
            "assertion `left == right` failed",
            "left: (2,",
            "right: (1,",
            "test result: FAILED. 8 passed; 1 failed",
            "error: test failed, to rerun pass `-p ragon-launcher --test auth_commands`",
        ],
    );
    let (_, out) = digest_of("windows-44705071695.log");
    assert!(!out.contains("a_sandbox_failure_says_what_fixes_it ... ok"), "{out}");
}

#[test]
fn a_vitest_timeout_and_a_rust_test_failure() {
    check(
        "windows-44717146852.log",
        &[
            "FAIL  src/night.test.ts > Night Shift inside a thread (DESKTOP §16 B40) > applies through ragon night apply",
            "Error: Test timed out in 5000ms.",
            "Test Files  1 failed | 48 passed",
            "test the_trust_store_commands_stay_in_the_users_own_store ... FAILED",
            "error: test failed, to rerun pass `-p ragon-launcher --test auth_provider`",
        ],
    );
}
