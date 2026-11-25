//! Best of N: N attempts of one task, each in its own worktree and branch, then the chooser picks
//! one, which is committed and renamed to the task's branch. The Python runner's
//! `NightShiftBestOfN`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::capped::Verb;
use super::chooser::{Candidate, Choice, PyRandom, ServerJudge, choose};
use super::task_run::{TaskRun, tail_chars};
use super::{Ctx, hint_for, is_cancelled, resolve_hints, transition};

const NO_CANDIDATE_ORDER: [&str; 4] = ["stalled", "no-change", "timed-out", "failed"];
const JUDGE_TIMEOUT: Duration = Duration::from_secs(600);
const MIN_JUDGE_TIMEOUT: Duration = Duration::from_secs(60);
const CANDIDATE: &str = "candidate";
const INTERRUPTED: &str = "interrupted";
const CANCELLED: &str = "cancelled";
const ONLY_CANDIDATE: &str = "only candidate";
const REGRESSION_FILTER: &str = "regression filter";
const ALL_BROKE: &str = "judge, all broke";
const JUDGE_SKIPPED: &str = "judge skipped";

/// One attempt's end, as the chooser and the record see it.
struct Outcome {
    attempt: u32,
    status: String,
    result: Map<String, Value>,
    worktree: PathBuf,
    branch: String,
    hint: Option<String>,
    log: String,
    test_log: Option<String>,
}

/// What the chooser decided, plus what the record keeps of the judging.
#[derive(Default)]
struct Decision {
    stage: String,
    points: Value,
    survivors: Vec<String>,
    chosen: Vec<String>,
    chosen_attempt: Option<u32>,
    judge_calls: usize,
    judge_errors: usize,
    judge_log: Option<String>,
    judge_unmade: usize,
}

pub struct BestOfN<'a> {
    ctx: &'a Ctx,
    task: Value,
    id: String,
    repo: PathBuf,
    base: String,
    branch: String,
    text: String,
    deadline: Instant,
    best_of: u32,
    hints: Vec<String>,
    attempts: u64,
    started: Instant,
    short: Option<String>,
}

fn text_of(task: &Value, key: &str) -> String {
    task.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

/// 0 when the tests passed, 1 when they failed or ran out of time, `None` when untested.
pub fn regression(result: &Map<String, Value>) -> Option<u32> {
    if result.get("timed_out").is_some_and(|v| v.as_bool() == Some(true)) {
        return Some(1);
    }
    let verdict = result.get("test_result").and_then(Value::as_str).filter(|s| !s.is_empty()).unwrap_or("untested");
    if verdict == "passed" {
        return Some(0);
    }
    (verdict.starts_with("failed") || verdict.starts_with("timed out")).then_some(1)
}

fn numbers(ids: &[String]) -> String {
    ids.iter().map(|id| id.trim_start_matches('a')).collect::<Vec<_>>().join(", ")
}

/// Why the chosen attempt was chosen, in a sentence.
pub fn reason(record: &Map<String, Value>, chosen_ids: &[String]) -> String {
    let Some(chosen) = record.get("chosen").and_then(Value::as_u64) else {
        return "no attempt left a change to choose".into();
    };
    let stage = record.get("stage").and_then(Value::as_str).unwrap_or("");
    if stage == ONLY_CANDIDATE {
        return format!("attempt {chosen} was the only one that left a change");
    }
    if stage == REGRESSION_FILTER {
        return format!(
            "attempt {chosen} was the only one the regression filter kept: the other attempts' tests failed or ran out of time"
        );
    }
    let mut text = judge_reason(record, stage, chosen);
    if let Some(unmade) = record.get("judge_unmade").and_then(Value::as_u64).filter(|n| *n > 0) {
        text += &format!(
            "; the window closed while it judged, so {unmade} comparison(s) were not made and counted as ties"
        );
    }
    if chosen_ids.len() > 1 && !stage.starts_with(JUDGE_SKIPPED) {
        text += &format!("; attempts {} tied at the top, and the tie went to {chosen}", numbers(chosen_ids));
    }
    text
}

fn judge_reason(record: &Map<String, Value>, stage: &str, chosen: u64) -> String {
    if stage.starts_with(JUDGE_SKIPPED) {
        return format!("{stage}, so the tie went to the attempt whose tests passed, then the earliest");
    }
    let candidates = record
        .get("attempts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|e| e.get("status").and_then(Value::as_str) == Some(CANDIDATE)).count())
        .unwrap_or(0);
    if stage == ALL_BROKE {
        return format!(
            "every attempt's tests failed, so the judge compared all {candidates} in pairs and gave attempt {chosen} the most points"
        );
    }
    let test_command = record.get("test_command").and_then(Value::as_str);
    if test_command.is_none() || test_command == Some("none") {
        return format!(
            "no test command, so the judge alone decided: it compared all {candidates} attempts in pairs and gave attempt {chosen} the most points"
        );
    }
    let survivors: Vec<String> = record
        .get("survivors")
        .and_then(Value::as_array)
        .map(|s| s.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    format!(
        "the judge compared the attempts the regression filter kept ({}) in pairs and gave attempt {chosen} the most points",
        numbers(&survivors)
    )
}

impl<'a> BestOfN<'a> {
    pub fn new(ctx: &'a Ctx, task: &Value, deadline: Instant) -> Self {
        let id = text_of(task, "id");
        let branch = task.get("branch").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        let hints = match task.get("best_of_hints") {
            Some(value) if !value.is_null() => resolve_hints(Some(value)),
            _ => ctx.settings.best_of_hints.clone(),
        };
        BestOfN {
            ctx,
            repo: PathBuf::from(text_of(task, "repo")),
            base: text_of(task, "base"),
            branch: branch.unwrap_or_else(|| format!("night/{id}")),
            text: text_of(task, "task"),
            deadline,
            best_of: super::attempts_for(task, &ctx.settings, &ctx.night).max(1),
            hints,
            attempts: task.get("attempts").and_then(Value::as_u64).unwrap_or(0),
            started: Instant::now(),
            short: None,
            task: task.clone(),
            id,
        }
    }

    pub fn run(&mut self) -> String {
        if is_cancelled(&self.ctx.night, &self.id) {
            return transition(&self.ctx.night, &self.id, CANCELLED, None, vec![]);
        }
        self.attempts += 1;
        transition(&self.ctx.night, &self.id, "running", None, vec![("attempts", json!(self.attempts))]);
        let mut outcomes = Vec::new();
        self.run_attempts(&mut outcomes)
    }

    fn git(&self, verb: Verb, args: &[&str]) -> (i32, String) {
        self.ctx.git.run(&self.repo, verb, args)
    }

    fn attempt_deadline(&self, number: u32) -> Instant {
        if number == 1 {
            return self.deadline;
        }
        let limit = Instant::now() + Duration::from_secs(self.ctx.settings.task_timeout_s);
        self.ctx.window_end.map_or(limit, |end| end.min(limit))
    }

    fn run_attempts(&mut self, outcomes: &mut Vec<Outcome>) -> String {
        let mut runs: Vec<TaskRun<'a>> = Vec::new();
        for number in 1..=self.best_of {
            if self.ctx.window_over() {
                self.short = Some(format!("the window closed before attempt {number} could start"));
                break;
            }
            if let Some(short) = self.wont_fit(number, outcomes) {
                self.short = Some(short);
                break;
            }
            if is_cancelled(&self.ctx.night, &self.id) {
                self.discard(outcomes.iter());
                let timing = self.timing(outcomes);
                return self.finish(CANCELLED, None, timing);
            }
            let deadline = self.attempt_deadline(number);
            let mut attempt = TaskRun::new(self.ctx, &self.task, deadline, Some(number), hint_for(number, &self.hints));
            let status = attempt.run();
            let (status, result) = match attempt.outcome.take() {
                Some(outcome) => (
                    outcome["status"].as_str().unwrap_or(&status).to_string(),
                    outcome.get("result").and_then(Value::as_object).cloned().unwrap_or_default(),
                ),
                None => (status, Map::new()),
            };
            outcomes.push(Outcome {
                attempt: number,
                status: status.clone(),
                result,
                worktree: attempt.worktree.clone(),
                branch: attempt.branch.clone(),
                hint: attempt.hint.clone().filter(|h| !h.is_empty()),
                log: attempt.log_path.to_string_lossy().into_owned(),
                test_log: attempt.test_log_path.exists().then(|| attempt.test_log_path.to_string_lossy().into_owned()),
            });
            runs.push(attempt);
            if status == CANCELLED {
                self.discard(outcomes.iter());
                let timing = self.timing(outcomes);
                return self.finish(CANCELLED, None, timing);
            }
            if status == INTERRUPTED {
                self.short = Some(format!("the window closed during attempt {number}, which was discarded"));
                break;
            }
        }
        self.discard(outcomes.iter().filter(|o| o.status == INTERRUPTED));
        if !outcomes.iter().any(|o| o.status == CANDIDATE) {
            return self.no_candidate(outcomes);
        }
        let decision = self.choose(outcomes);
        let chosen = decision.chosen_attempt.unwrap_or(1);
        let run = runs.iter().find(|r| r.attempt == Some(chosen)).expect("the chosen attempt ran");
        self.commit(run, chosen, &decision, outcomes)
    }

    fn wont_fit(&self, number: u32, outcomes: &[Outcome]) -> Option<String> {
        let end = self.ctx.window_end?;
        if number == 1 {
            return None;
        }
        let need = outcomes
            .iter()
            .filter(|o| o.status != INTERRUPTED)
            .map(|o| o.result.get("wall_s").and_then(Value::as_u64).unwrap_or(0))
            .max()
            .unwrap_or(0);
        let left = end.saturating_duration_since(Instant::now()).as_secs();
        if need == 0 || left >= need {
            return None;
        }
        Some(format!(
            "attempt {number} was not started: {} min of the window were left, less than the {} min the longest attempt took",
            left / 60,
            need / 60
        ))
    }

    fn no_candidate(&mut self, outcomes: &[Outcome]) -> String {
        self.discard(outcomes.iter());
        let mut result = self.timing(outcomes);
        let decision = Decision { stage: "none".into(), ..Decision::default() };
        result.insert("best_of".into(), self.record(&decision, outcomes));
        let last = outcomes
            .iter()
            .rev()
            .filter_map(|o| o.result.get("last_message").and_then(Value::as_str))
            .find(|m| !m.is_empty())
            .map(String::from);
        if outcomes.is_empty() || outcomes.iter().any(|o| o.status == INTERRUPTED) {
            if let Some(last) = last {
                result.insert("last_message".into(), json!(last));
            }
            result.insert(
                "note".into(),
                json!("the window closed before any attempt finished; the task starts over next night"),
            );
            return self.finish(INTERRUPTED, None, result);
        }
        let all_failed = outcomes.iter().all(|o| o.status == "failed");
        let mut status = if all_failed {
            "failed"
        } else {
            NO_CANDIDATE_ORDER.iter().copied().find(|s| outcomes.iter().any(|o| o.status == *s)).unwrap_or("failed")
        };
        if status == "timed-out" {
            status = "no-change";
        }
        if let Some(last) = last.filter(|_| status == "stalled" || status == "failed") {
            result.insert("last_message".into(), json!(last));
        }
        self.finish(status, None, result)
    }

    fn choose(&self, outcomes: &[Outcome]) -> Decision {
        let pool: Vec<Candidate> = outcomes
            .iter()
            .filter(|o| o.status == CANDIDATE)
            .map(|o| Candidate {
                id: format!("a{}", o.attempt),
                patch: o
                    .result
                    .get("diff")
                    .and_then(Value::as_str)
                    .and_then(|p| std::fs::read(p).ok())
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default(),
                applied: true,
                p2p_failed: regression(&o.result),
            })
            .collect();
        let digest = Sha256::digest(self.id.as_bytes());
        let seed = u64::from(u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]));
        let survivors = pool.iter().filter(|c| !c.breaks()).count();
        let contested = pool.len() > 1 && (survivors > 1 || survivors == 0);
        let mut log: Vec<Value> = Vec::new();
        let mut unmade = 0usize;
        let mut rng = PyRandom::from_str_seed(&self.id);
        let mut skipped: Option<&str> = None;
        let choice: Choice = if contested && self.ctx.window_over() {
            skipped = Some("judge skipped: the window had closed");
            choose(&pool, &mut |_: &Candidate, _: &Candidate| None, &mut rng)
        } else if contested && self.ctx.model_host.is_none() && self.ctx.judge.borrow().is_none() {
            skipped = Some("judge skipped: no model server");
            choose(&pool, &mut |_: &Candidate, _: &Candidate| None, &mut rng)
        } else if let Some(judge) = self.ctx.judge.borrow_mut().as_mut() {
            let mut bounded = |a: &Candidate, b: &Candidate| {
                if self.ctx.window_over() {
                    unmade += 1;
                    return None;
                }
                let ratings = judge(a, b);
                log.push(json!({"a": a.id, "b": b.id, "ratings": ratings.map(|(x, y)| vec![x, y])}));
                ratings
            };
            choose(&pool, &mut bounded, &mut rng)
        } else {
            let mut timeout = JUDGE_TIMEOUT;
            if let Some(end) = self.ctx.window_end {
                timeout = end.saturating_duration_since(Instant::now()).min(JUDGE_TIMEOUT).max(MIN_JUDGE_TIMEOUT);
            }
            let mut server = ServerJudge::new(self.ctx.model_host.as_deref().unwrap_or(""), &self.text, Some(seed), timeout);
            let choice = {
                let mut bounded = |a: &Candidate, b: &Candidate| {
                    if self.ctx.window_over() {
                        unmade += 1;
                        return None;
                    }
                    server.compare(a, b)
                };
                choose(&pool, &mut bounded, &mut rng)
            };
            log = server.log;
            choice
        };
        let mut decision = Decision {
            stage: skipped.map(String::from).unwrap_or_else(|| choice.stage.clone()),
            points: choice.points_json(),
            survivors: choice.survivors.clone(),
            chosen: choice.chosen.clone(),
            judge_unmade: unmade,
            judge_calls: log.len(),
            judge_errors: log
                .iter()
                .filter(|r| r.get("error").is_some_and(|e| !e.is_null() && e != "") || r.get("ratings").is_none_or(Value::is_null))
                .count(),
            ..Decision::default()
        };
        if !log.is_empty() {
            let path = self.ctx.night.join("logs").join(format!("{}.judge.jsonl", self.id));
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
            let lines: String = log.iter().map(|r| format!("{r}\n")).collect();
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
                use std::io::Write as _;
                let _ = file.write_all(lines.as_bytes());
            }
            decision.judge_log = Some(path.to_string_lossy().into_owned());
        }
        let breaks = |id: &str| pool.iter().find(|c| c.id == id).is_some_and(Candidate::breaks);
        let number = |id: &str| id[1..].parse::<u32>().unwrap_or(u32::MAX);
        let top: Vec<String> = if choice.chosen.is_empty() { vec![pool[0].id.clone()] } else { choice.chosen.clone() };
        decision.chosen_attempt =
            top.iter().min_by_key(|id| (breaks(id), number(id))).map(|id| number(id));
        decision
    }

    fn commit(&mut self, run: &TaskRun<'_>, chosen: u32, decision: &Decision, outcomes: &[Outcome]) -> String {
        let outcome = outcomes.iter().find(|o| o.attempt == chosen).expect("the chosen outcome");
        let mut result = outcome.result.clone();
        let (status, output) = run.commit_staged();
        if status != "done" {
            self.discard(outcomes.iter());
            let note = format!("git commit failed: {}", tail_chars(output.trim(), 300));
            result.extend(self.timing(outcomes));
            result.insert("best_of".into(), self.record(decision, outcomes));
            return self.finish("failed", Some(&note), result);
        }
        self.discard(outcomes.iter().filter(|o| o.attempt != chosen));
        self.git(Verb::Worktree, &["remove", "--force", &outcome.worktree.to_string_lossy()]);
        let renamed = self.git(Verb::Branch, &["-M", &outcome.branch, &self.branch]).0 == 0;
        let final_branch = if renamed { self.branch.clone() } else { outcome.branch.clone() };
        let dropped = ["worktree", "session", "diff", "timed_out"];
        let mut result: Map<String, Value> = result.into_iter().filter(|(key, _)| !dropped.contains(&key.as_str())).collect();
        result.extend(self.timing(outcomes));
        result.insert("branch".into(), json!(final_branch));
        let stat = self.git(Verb::Diff, &["--stat", &format!("{}..{final_branch}", self.base)]).1;
        result.insert("diff_stat".into(), json!(stat.trim_end()));
        result.insert("best_of".into(), self.record(decision, outcomes));
        self.finish("done", None, result)
    }

    fn record(&self, decision: &Decision, outcomes: &[Outcome]) -> Value {
        let attempts: Vec<Value> = outcomes
            .iter()
            .map(|o| {
                let mut entry = Map::new();
                entry.insert("attempt".into(), json!(o.attempt));
                entry.insert("status".into(), json!(o.status));
                entry.insert("wall_s".into(), o.result.get("wall_s").cloned().unwrap_or(Value::Null));
                for key in ["test_result", "diff", "timed_out"] {
                    if let Some(value) = o.result.get(key).filter(|v| !v.is_null()) {
                        entry.insert(key.into(), value.clone());
                    }
                }
                if let Some(hint) = &o.hint {
                    entry.insert("hint".into(), json!(hint));
                }
                if !o.log.is_empty() {
                    entry.insert("log".into(), json!(o.log));
                }
                if let Some(test_log) = &o.test_log {
                    entry.insert("test_log".into(), json!(test_log));
                }
                if let Some(stat) = o.result.get("diff_stat").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    entry.insert("changes".into(), json!(stat.lines().last().unwrap_or("").trim()));
                }
                Value::Object(entry)
            })
            .collect();
        let test_command = outcomes
            .iter()
            .filter_map(|o| o.result.get("test_command").and_then(Value::as_str))
            .find(|c| !c.is_empty())
            .map_or(Value::Null, |c| json!(c));
        let mut record = Map::new();
        record.insert("n".into(), json!(self.best_of));
        record.insert("ran".into(), json!(outcomes.len()));
        record.insert("chosen".into(), json!(decision.chosen_attempt));
        record.insert("stage".into(), json!(decision.stage));
        let points = if decision.points.as_object().is_some_and(|p| !p.is_empty()) { decision.points.clone() } else { json!({}) };
        record.insert("points".into(), points);
        record.insert("survivors".into(), json!(decision.survivors));
        record.insert("judge_calls".into(), json!(decision.judge_calls));
        record.insert("judge_errors".into(), json!(decision.judge_errors));
        record.insert("judge_log".into(), json!(decision.judge_log));
        record.insert("test_command".into(), test_command);
        record.insert("attempts".into(), Value::Array(attempts));
        if let Some(short) = &self.short {
            record.insert("why".into(), json!(short));
        }
        if decision.judge_unmade > 0 {
            record.insert("judge_unmade".into(), json!(decision.judge_unmade));
        }
        let why = reason(&record, &decision.chosen);
        record.insert("reason".into(), json!(why));
        Value::Object(record)
    }

    fn discard<'o>(&self, outcomes: impl Iterator<Item = &'o Outcome>) {
        for outcome in outcomes {
            if outcome.worktree.is_dir() {
                self.git(Verb::Worktree, &["remove", "--force", &outcome.worktree.to_string_lossy()]);
            }
            self.git(Verb::Branch, &["-D", &outcome.branch]);
        }
    }

    fn timing(&self, outcomes: &[Outcome]) -> Map<String, Value> {
        let mut timing = Map::new();
        timing.insert("attempts".into(), json!(self.attempts));
        let nudges: u64 = outcomes.iter().map(|o| o.result.get("nudges").and_then(Value::as_u64).unwrap_or(0)).sum();
        timing.insert("nudges".into(), json!(nudges));
        timing.insert("wall_s".into(), json!(self.started.elapsed().as_secs()));
        if let Some(node) = &self.ctx.model_node {
            timing.insert("model_node".into(), json!(node));
        }
        timing
    }

    fn finish(&self, status: &str, note: Option<&str>, result: Map<String, Value>) -> String {
        transition(
            &self.ctx.night,
            &self.id,
            status,
            note,
            vec![("result", Value::Object(result)), ("attempts", json!(self.attempts))],
        )
    }
}
