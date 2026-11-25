//! The night run against the stand-in `ragon` of the shared vectors (vectors/night-run/
//! fake_ragon.py): three attempts, the chooser, the lock, the report, and proof that nothing is
//! merged or pushed. Unix only: the stand-in is a Python script and the tests run `bash -c`.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::{Local, TimeZone};
use serde_json::{Value, json};

use super::*;

const RECORD: &str = include_str!("../../vectors/night-run/best_of_record.json");
const REPORT: &str = include_str!("../../vectors/night-run/best_of_report.md");
const TEST: &str = "grep -q hi hello.txt";

/// The record as the queue writer renders it in this build. The shipped binary keeps keys in
/// insertion order (serde_json `preserve_order`, unified in through the TUI), and then this is
/// the golden file byte for byte. The launcher's own test build sorts keys: the golden is then
/// re-rendered through the same writer (`night::update_task`), which still pins the indent, the
/// escapes and the final newline. `cargo test -p ragon-launcher --features
/// serde_json/preserve_order night_run` runs the exact comparison.
fn golden_record() -> String {
    let probe: Value = serde_json::from_str(r#"{"b": 0, "a": 0}"#).unwrap();
    if probe.as_object().unwrap().keys().next().map(String::as_str) == Some("b") {
        return RECORD.to_string();
    }
    let golden: Value = serde_json::from_str(RECORD).unwrap();
    serde_json::to_string_pretty(&golden).unwrap() + "\n"
}
const ID: &str = "20151001-0100-abc";

struct Scratch {
    root: PathBuf,
    repo: PathBuf,
    night: PathBuf,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(cwd).args(args).output().expect("git runs");
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A repository with one commit and an empty queue, as test_night_shift.py's `setup` makes them.
fn scratch(name: &str) -> Scratch {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let root = std::env::temp_dir().join(format!("night-run-{name}-{}-{nanos}", std::process::id()));
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "night@test"]);
    git(&repo, &["config", "user.name", "Night Test"]);
    std::fs::write(repo.join("README.md"), "readme\n").unwrap();
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    Scratch { night: root.join("night"), repo, root }
}

/// Queues a task the way the launcher writes it (test_night_shift.py's `queue`).
fn queue(s: &Scratch, test: Option<&str>) -> Value {
    let base = git(&s.repo, &["rev-parse", "HEAD"]).trim().to_string();
    let record = json!({
        "id": ID,
        "repo": s.repo.to_string_lossy(),
        "base": base,
        "branch": format!("night/{ID}"),
        "task": "Add hello.txt",
        "test": test,
        "model_at_add": "test-model",
        "status": "queued",
        "history": [{"at": "2020-10-01T21:00:00+01:00", "status": "queued"}],
        "attempts": 0,
        "nudges": 0,
        "session": null,
        "result": null,
    });
    std::fs::create_dir_all(s.night.join("tasks")).unwrap();
    std::fs::write(task_path(&s.night, ID), serde_json::to_string(&record).unwrap()).unwrap();
    record
}

fn fake_ragon() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("vectors/night-run/fake_ragon.py")
}

fn ctx(s: &Scratch, best_of: u32, plan: &[&str]) -> Ctx {
    let settings = Settings::from_table(&json!({"best_of": best_of}));
    let mut ctx = Ctx::new(&s.night, &fake_ragon(), settings);
    ctx.use_scope = false;
    ctx.window_end = Some(Instant::now() + Duration::from_secs(600));
    ctx.extra_env.push(("FAKE_BON_PLAN".into(), serde_json::to_string(plan).unwrap()));
    ctx
}

fn run(ctx: &Ctx, task: &Value) -> String {
    run_task(ctx, task, Instant::now() + Duration::from_secs(600))
}

fn branches(repo: &Path) -> Vec<String> {
    git(repo, &["branch", "--list", "night/*", "--format=%(refname:short)"]).split_whitespace().map(String::from).collect()
}

/// test_night_run_vectors.py's `normalize`.
fn normalize(text: &str, night: &Path, repo: &Path, base: &str) -> String {
    let text = text
        .replace(&night.to_string_lossy().into_owned(), "<NIGHT>")
        .replace(&repo.to_string_lossy().into_owned(), "<REPO>")
        .replace(base, "<BASE>")
        .replace(&base[..12], "<BASE12>");
    let re = |p: &str| regex_lite::Regex::new(p).unwrap();
    let text = re(r#""at": "[^"]*""#).replace_all(&text, r#""at": "<AT>""#).into_owned();
    let text = re(r#""wall_s": \d+"#).replace_all(&text, r#""wall_s": 0"#).into_owned();
    let text = re(r"finished \d\d:\d\d\.").replace_all(&text, "finished <HH:MM>.").into_owned();
    re(r"wall time: [^,\n]*|, \d+ s,|, \d+ min \d+ s,")
        .replace_all(&text, |c: &regex_lite::Captures| {
            if c[0].starts_with("wall") { "wall time: <T>".to_string() } else { ", <T>,".to_string() }
        })
        .into_owned()
}

#[test]
fn three_attempts_write_the_record_and_report_the_python_runner_writes() {
    let s = scratch("vectors");
    let task = queue(&s, Some(TEST));
    let ctx = ctx(&s, 3, &["good", "bad", "good"]);
    assert_eq!(run(&ctx, &task), "done");
    let base = task["base"].as_str().unwrap();
    let written = std::fs::read_to_string(task_path(&s.night, ID)).unwrap();
    assert_eq!(normalize(&written, &s.night, &s.repo, base), golden_record());
    let record = read(&s.night, ID).unwrap();
    let started = Local.with_ymd_and_hms(2015, 10, 1, 1, 0, 0).unwrap();
    let report = report::render(started, Local::now(), &[record], &["a note".to_string()]);
    assert_eq!(normalize(&report, &s.night, &s.repo, base), REPORT);
    assert_eq!(branches(&s.repo), vec![format!("night/{ID}")]);
    let worktrees = git(&s.repo, &["worktree", "list", "--porcelain"]);
    assert_eq!(worktrees.matches("worktree ").count(), 1, "attempt worktrees are removed: {worktrees}");
    let log = git(&s.repo, &["log", "--format=%s", &format!("night/{ID}")]);
    assert_eq!(log.lines().next(), Some("night: Add hello.txt"));
}

#[test]
fn nothing_is_merged_or_pushed() {
    let s = scratch("no-push");
    let remote = s.root.join("remote.git");
    git(&s.root, &["init", "-q", "--bare", &remote.to_string_lossy()]);
    git(&s.repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
    git(&s.repo, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    let remote_before = git(&remote, &["for-each-ref"]);
    let head_before = git(&s.repo, &["rev-parse", "HEAD"]);
    let branch_before = git(&s.repo, &["symbolic-ref", "HEAD"]);
    let task = queue(&s, Some(TEST));
    let ctx = ctx(&s, 3, &["good", "bad", "good"]);
    assert_eq!(run(&ctx, &task), "done");
    let allowed = ["add", "branch", "cat-file", "commit", "diff", "reset", "clean", "rev-parse", "show", "status", "worktree"];
    for verb in ctx.git.verbs() {
        assert!(allowed.contains(&verb.as_str()), "the night run ran git {verb}");
    }
    for words in ctx.git.log.borrow().iter() {
        for word in ["push", "merge", "fetch", "pull", "rebase", "cherry-pick", "remote"] {
            assert!(!words.iter().any(|w| w == word), "the night run ran {words:?}");
        }
    }
    assert_eq!(git(&remote, &["for-each-ref"]), remote_before, "the remote is unchanged");
    assert_eq!(git(&s.repo, &["rev-parse", "HEAD"]), head_before, "the checkout's HEAD is unchanged");
    assert_eq!(git(&s.repo, &["symbolic-ref", "HEAD"]), branch_before, "the checkout's branch is unchanged");
    assert_eq!(git(&s.repo, &["status", "--porcelain"]), "", "the checkout is untouched");
    assert!(!s.repo.join("hello.txt").exists());
}

#[test]
fn the_judge_picks_among_passing_attempts() {
    let s = scratch("judge");
    let task = queue(&s, Some(TEST));
    let ctx = ctx(&s, 3, &["fine", "GOOD", "fine"]);
    *ctx.judge.borrow_mut() = Some(Box::new(|a: &Candidate, b: &Candidate| {
        let score = |c: &Candidate| if c.patch.contains("GOOD") { 9.0 } else { 3.0 };
        Some((score(a), score(b)))
    }));
    assert_eq!(run(&ctx, &task), "done");
    let record = read(&s.night, ID).unwrap();
    let best_of = &record["result"]["best_of"];
    assert_eq!(best_of["chosen"], json!(2), "{best_of}");
    assert!(!best_of["stage"].as_str().unwrap().starts_with("judge skipped"), "{best_of}");
    assert!(best_of["judge_calls"].as_u64().unwrap() > 0);
    assert_eq!(best_of["judge_errors"], json!(0));
    let judge_log = PathBuf::from(best_of["judge_log"].as_str().unwrap());
    assert!(std::fs::read_to_string(judge_log).unwrap().lines().count() > 0);
    let committed = git(&s.repo, &["show", &format!("night/{ID}:hello.txt")]);
    assert_eq!(committed, "hi GOOD\n");
}

#[test]
fn one_attempt_commits_on_the_task_branch() {
    let s = scratch("single");
    let task = queue(&s, Some(TEST));
    let ctx = ctx(&s, 1, &[]);
    assert_eq!(run(&ctx, &task), "done");
    let record = read(&s.night, ID).unwrap();
    assert_eq!(record["result"]["test_result"], json!("passed"));
    assert_eq!(record["result"]["branch"], json!(format!("night/{ID}")));
    assert_eq!(record["session"], json!("thread-a0"));
    assert!(record["result"].get("best_of").is_none());
    assert_eq!(branches(&s.repo), vec![format!("night/{ID}")]);
}

#[test]
fn no_attempt_with_a_change_leaves_no_branch() {
    let s = scratch("none");
    let task = queue(&s, None);
    let ctx = ctx(&s, 2, &["none", "none"]);
    assert_eq!(run(&ctx, &task), "no-change");
    let record = read(&s.night, ID).unwrap();
    let best_of = &record["result"]["best_of"];
    assert_eq!(best_of["stage"], json!("none"));
    assert_eq!(best_of["reason"], json!("no attempt left a change to choose"));
    assert_eq!(best_of["ran"], json!(2));
    assert!(branches(&s.repo).is_empty());
    assert!(!s.night.join("worktrees").join(format!("{ID}-a1")).exists());
}

#[test]
fn a_held_runner_lock_refuses_a_second_run() {
    let s = scratch("lock");
    queue(&s, Some(TEST));
    let lock = s.night.join("runner.lock");
    let held = runner_lock(&lock, "another night run").expect("the first holder gets the lock");
    let ctx = ctx(&s, 1, &[]);
    let (code, lines) = run_night(&ctx, &lock, None);
    assert_eq!(code, 1);
    assert_eq!(lines, vec!["⚠️  Another night run holds the runner lock.".to_string()]);
    assert_eq!(read(&s.night, ID).unwrap()["status"], json!("queued"), "the task was not touched");
    drop(held);
    // A child that another test forks in this instant holds a copy of the lock until it execs,
    // so a refused run is retried; it leaves the task alone.
    let started = Instant::now();
    let (code, lines) = loop {
        let (code, lines) = run_night(&ctx, &lock, None);
        if code != 1 || started.elapsed() > Duration::from_secs(10) {
            break (code, lines);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(code, 0, "{lines:?}");
    assert_eq!(read(&s.night, ID).unwrap()["status"], json!("done"));
    assert!(lines.last().unwrap().starts_with("✅ Night Shift finished: 1 done. Report: "), "{lines:?}");
}

#[test]
fn a_cancelled_task_does_not_run() {
    let s = scratch("cancel");
    let task = queue(&s, Some(TEST));
    transition(&s.night, ID, "cancel-requested", None, vec![]);
    let ctx = ctx(&s, 3, &["good", "good", "good"]);
    assert_eq!(run(&ctx, &task), "cancelled");
    assert!(branches(&s.repo).is_empty());
}

#[test]
fn the_schedule_is_a_daily_limited_task_scheduler_entry() {
    let exe = Path::new(r"C:\Program Files\Ragon\ragon.exe");
    assert_eq!(
        schtasks_create(exe, "01:00").join(" "),
        r#"/Create /TN Ragon Night Shift /TR "C:\Program Files\Ragon\ragon.exe" night run /SC DAILY /ST 01:00 /RL LIMITED /F"#
    );
    assert_eq!(schtasks_delete().join(" "), "/Delete /TN Ragon Night Shift /F");
    assert_eq!(schtasks_create(exe, "01:00")[2], SCHEDULED_TASK);
}

#[test]
fn defaults_match_the_python_runner() {
    let settings = Settings::default();
    assert_eq!(settings.best_of, 3);
    assert_eq!(settings.nudges, 2);
    assert_eq!(settings.task_timeout_s, 90 * 60);
    assert_eq!(settings.test_timeout_s, 20 * 60);
    assert_eq!(settings.task_memory, "8G");
    assert!(settings.test_sandbox);
    assert_eq!(hint_for(1, &resolve_hints(Some(&json!(true)))), None);
    assert_eq!(hint_for(2, &resolve_hints(Some(&json!(true)))).as_deref(), Some(DEFAULT_HINTS[0]));
    assert_eq!(attempts_for(&json!({"id": "x", "session": "s"}), &settings, Path::new("/nonexistent")), 1);
    assert_eq!(attempts_for(&json!({"id": "x"}), &settings, Path::new("/nonexistent")), 3);
}

#[test]
fn the_lock_sits_in_the_node_state_layout_on_windows() {
    let path = runner_lock_path(Path::new("/q/night"));
    if cfg!(windows) {
        assert!(path.ends_with(Path::new("Ragon").join("night").join("runner.lock")));
    } else {
        assert_eq!(path, Path::new("/q/night/runner.lock"));
    }
}
