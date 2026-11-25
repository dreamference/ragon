//! `ragon night run` against a fake `ragon` (as `tests/test_night_shift.py` does): a shell script
//! that plays one scripted turn per `exec` call and runs `sandbox -- cmd` as given.

use super::*;
use crate::night::Request;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ragon-night-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_default();
    dir
}

/// A repository with one commit and an identity of its own, the queue directory, and a fake
/// `ragon` whose `exec` calls play `modes` in order (one line per call, nudges included):
/// `change` writes hello.txt, `broken` also writes BROKEN, `nothing` answers and changes nothing,
/// `stall` promises the change and makes none.
#[cfg(unix)]
fn setup(name: &str, modes: &[&str]) -> (PathBuf, PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;
    let root = scratch(name);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap_or_default();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.email", "t@t"],
        vec!["config", "user.name", "t"],
        vec!["commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        let _ = Command::new("git").arg("-C").arg(&repo).args(&args).output();
    }
    let repo = night::native_path(repo.canonicalize().unwrap_or(repo));
    std::fs::write(root.join("modes"), modes.join("\n") + "\n").unwrap_or_default();
    let fake = root.join("ragon");
    let script = r#"#!/bin/sh
root='ROOT'
echo "$*" >> "$root/calls"
if [ "$1" = sandbox ]; then
  while [ "$1" != "--" ]; do shift; done
  shift
  exec "$@"
fi
n=$(grep -c '^exec' "$root/calls")
mode=$(sed -n "${n}p" "$root/modes")
last=''; wt=''
while [ $# -gt 0 ]; do
  case "$1" in
    -o) last=$2; shift 2 ;;
    -C) wt=$2; shift 2 ;;
    *) shift ;;
  esac
done
echo "{\"type\":\"thread.started\",\"thread_id\":\"thread-$n\"}"
case "$mode" in
  change) echo "call $n" > "$wt/hello.txt"; msg='Changed hello.txt.' ;;
  broken) echo "call $n" > "$wt/hello.txt"; touch "$wt/BROKEN"; msg='Changed hello.txt.' ;;
  stall) msg="I'll make the change now." ;;
  *) msg='Nothing needed changing.' ;;
esac
printf '%s' "$msg" > "$last"
echo '{"type":"turn.completed"}'
"#
    .replace("ROOT", &root.to_string_lossy());
    std::fs::write(&fake, script).unwrap_or_default();
    let _ = std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755));
    (repo, root.join("night"), fake)
}

fn queue(dir: &Path, repo: &Path, test: Option<&str>, task: &str) -> String {
    let request = Request::Add { test: test.map(str::to_string), on: None, best_of: None, task: task.to_string() };
    let lines = night::run(dir, request, repo);
    lines[0].split_whitespace().nth(1).unwrap_or_default().to_string()
}

fn options(fake: &Path) -> Options {
    let mut options = Options::new(fake.to_path_buf());
    options.task_timeout = Duration::from_secs(60);
    options.test_timeout = Duration::from_secs(60);
    options
}

fn task(dir: &Path, id: &str) -> Value {
    night::read_task(&dir.join("tasks").join(format!("{id}.json"))).unwrap_or_default()
}

fn branches(repo: &Path) -> String {
    git(repo, &["branch", "--list", "night/*"]).unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn three_attempts_the_regression_filter_keeps_the_one_whose_tests_pass() {
    let (repo, dir, fake) = setup("filter", &["broken", "change", "broken"]);
    let id = queue(&dir, &repo, Some("test ! -f BROKEN"), "Add hello.txt");
    let outcome = run(&dir, &options(&fake));
    assert_eq!(outcome.code, 0, "{:?}", outcome.lines);
    assert_eq!(outcome.lines[0], format!("{id}: done"));
    let record = task(&dir, &id);
    let best = &record["result"]["best_of"];
    assert_eq!(best["ran"], 3);
    assert_eq!(best["chosen"], 2);
    assert_eq!(best["stage"], "regression filter");
    assert_eq!(best["survivors"], json!(["a2"]));
    assert_eq!(best["attempts"][0]["test_result"], "failed (exit 1)");
    assert_eq!(best["attempts"][1]["test_result"], "passed");
    assert_eq!(record["result"]["branch"], format!("night/{id}"));
    assert_eq!(record["result"]["test_result"], "passed");
    // One branch is left, holding attempt 2's change, committed; the attempts' worktrees are gone.
    assert_eq!(branches(&repo).trim(), format!("night/{id}"));
    let shown = git(&repo, &["show", &format!("night/{id}:hello.txt")]).unwrap_or_default();
    assert_eq!(shown, "call 2");
    let subject = git(&repo, &["log", "-1", "--format=%s", &format!("night/{id}")]).unwrap_or_default();
    assert_eq!(subject, "night: Add hello.txt");
    assert!(std::fs::read_dir(dir.join("worktrees")).map(|d| d.count()).unwrap_or(0) == 0);
    // The discarded attempts' diffs are kept for `apply --attempt N`.
    assert!(read(&dir.join("logs").join(format!("{id}-a1.diff"))).contains("BROKEN"));
    // The readers of the Python runner's records read this one.
    let report = read(outcome.report.as_deref().unwrap_or(Path::new("/nonexistent")));
    assert!(report.starts_with("# Night Shift — "), "{report}");
    assert!(report.contains(&format!("- {id} (ready to apply): Add hello.txt — finished, its changes are on a branch")), "{report}");
    assert!(report.contains("- Best of 3: 3 of 3 attempts ran; chose attempt 2 by regression filter"), "{report}");
    assert!(report.contains("(deciding stage: the tests removed every other attempt)"), "{report}");
    assert!(report.contains("  - Attempt 1: candidate, 2 files changed, 1 insertion(+), tests failed (exit 1)"), "{report}");
    let shown = night::run(&dir, Request::Report, &repo);
    assert!(shown.iter().any(|line| *line == format!("### {id} — done")), "{shown:?}");
    let applied = night::run(&dir, Request::Apply(id.clone()), &repo);
    assert!(applied.iter().any(|line| line.contains("hello.txt")), "{applied:?}");
    assert_eq!(read(&repo.join("hello.txt")).trim(), "call 2");
    // The fake saw three execs and three sandboxed test runs.
    let calls = read(&dir.parent().unwrap_or(&dir).join("calls"));
    assert_eq!(calls.lines().filter(|l| l.starts_with("exec")).count(), 3);
    assert_eq!(calls.lines().filter(|l| l.starts_with("sandbox")).count(), 3);
}

#[cfg(unix)]
#[test]
fn with_no_test_command_the_judge_decides_between_the_attempts() {
    let (repo, dir, fake) = setup("judge", &["change", "change", "change"]);
    let id = queue(&dir, &repo, None, "Add hello.txt");
    let mut options = options(&fake);
    // Prefers the patch that writes "call 3".
    options.judge = Some(Box::new(|_issue: &str, a: &str, b: &str| {
        let rate = |patch: &str| if patch.contains("+call 3") { 9.0 } else { 3.0 };
        Some((rate(a), rate(b)))
    }));
    let outcome = run(&dir, &options);
    assert_eq!(outcome.lines[0], format!("{id}: done"));
    let best = &task(&dir, &id)["result"]["best_of"];
    assert_eq!(best["stage"], "judge");
    assert_eq!(best["chosen"], 3);
    assert!(best["judge_calls"].as_u64().unwrap_or(0) >= 3, "{best}");
    assert!(best["reason"].as_str().unwrap_or_default().starts_with("no test command, so the judge alone decided"));
    assert_eq!(git(&repo, &["show", &format!("night/{id}:hello.txt")]).unwrap_or_default(), "call 3");
}

#[cfg(unix)]
#[test]
fn a_stalled_attempt_is_nudged_and_the_only_change_needs_no_comparison() {
    // Attempt 1 stalls and acts on the nudge; attempts 2 and 3 change nothing.
    let (repo, dir, fake) = setup("only", &["stall", "change", "nothing", "nothing"]);
    let id = queue(&dir, &repo, None, "Add hello.txt");
    let outcome = run(&dir, &options(&fake));
    assert_eq!(outcome.lines[0], format!("{id}: done"));
    let record = task(&dir, &id);
    assert_eq!(record["result"]["best_of"]["stage"], "only candidate");
    assert_eq!(record["result"]["best_of"]["chosen"], 1);
    assert_eq!(record["result"]["nudges"], 1);
    let calls = read(&dir.parent().unwrap_or(&dir).join("calls"));
    assert!(calls.contains(&format!("resume thread-1 {NUDGE}")), "{calls}");
}

#[cfg(unix)]
#[test]
fn attempts_that_change_nothing_leave_no_branch() {
    let (repo, dir, fake) = setup("nothing", &["nothing", "stall", "nothing"]);
    let id = queue(&dir, &repo, None, "Add hello.txt");
    let mut options = options(&fake);
    options.nudges = 0;
    let outcome = run(&dir, &options);
    assert_eq!(outcome.lines[0], format!("{id}: stalled"));
    assert!(branches(&repo).trim().is_empty());
    let report = read(outcome.report.as_deref().unwrap_or(Path::new("/nonexistent")));
    assert!(report.contains("- Best of 3: 3 of 3 attempts ran; none chosen"), "{report}");
    assert!(report.contains("- Last message: Nothing needed changing."), "{report}");
}

#[test]
fn a_held_runner_lock_stops_a_second_run() {
    let dir = scratch("lock").join("night");
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let held = runner_lock(&dir).ok().flatten();
    assert!(held.is_some());
    let outcome = run(&dir, &Options::new(PathBuf::from("ragon")));
    assert_eq!((outcome.lines, outcome.code), (vec![HELD.to_string()], 1));
    assert_eq!(read(&dir.join("runner.lock")), HOLDER);
    if let Some(held) = held {
        // Unlocked, not only closed: a test forking beside this one may hold a copy of the descriptor.
        let _ = held.unlock();
    }
    let outcome = run(&dir, &Options::new(PathBuf::from("ragon")));
    assert_eq!(outcome.lines, vec!["Nothing queued.".to_string()]);
}

#[test]
fn the_tournament_gives_every_candidate_two_comparisons_and_reads_ratings() {
    let mut rng = StdRng::seed_from_u64(seed("t"));
    let mut seen = Vec::new();
    let points = tournament(5, &mut rng, |a, b| {
        seen.push((a.min(b), a.max(b)));
        Some((a as f64, b as f64))
    });
    let mut sorted = seen.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), seen.len(), "a pair was played twice: {seen:?}");
    for i in 0..5 {
        assert!(seen.iter().filter(|(a, b)| *a == i || *b == i).count() >= 2, "{seen:?}");
    }
    assert!(points[4] > points[0], "{points:?}");
    assert_eq!(ratings("ok\nRATING_A: **7\nRATING_B: 12"), Some((7.0, 10.0)));
    assert_eq!(ratings("RATING_A: 3"), None);
}

#[test]
fn durations_and_sizes_read_as_the_night_table_writes_them() {
    assert_eq!(duration("90m"), Some(Duration::from_secs(5400)));
    assert_eq!(duration("2h"), Some(Duration::from_secs(7200)));
    assert_eq!(duration("x"), None);
    assert_eq!(bytes("8G"), Some(8 << 30));
    assert_eq!(bytes("512M"), Some(512 << 20));
}
