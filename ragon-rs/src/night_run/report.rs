//! The morning report (`night_shift_report.py`), the same Markdown byte for byte.

use std::path::Path;
use std::path::PathBuf;

use chrono::DateTime;
use chrono::Local;
use serde_json::Value;

const LAST_MESSAGE_STATUSES: &[&str] = &["stalled", "failed", "interrupted"];
const SENT: &str = "sent";
const RUNNING: &str = "running";
const FIRST_REFINE: &str = "v1";
const COUNT_ORDER: &[&str] = &["done", "no-change", "stalled", "failed", "interrupted", SENT, RUNNING, "cancelled"];
const READY: &str = "ready to apply";
const LOOK: &str = "needs a look";
const NOTHING: &str = "nothing to do";
const NOTHING_STATUSES: &[&str] = &["no-change", "cancelled"];

fn verdict(status: &str) -> Option<&'static str> {
    Some(match status {
        "done" => "finished, its changes are on a branch",
        "no-change" => "ran and changed nothing",
        "stalled" => "stopped before it finished",
        "failed" => "failed",
        "interrupted" => "cut off by the end of the window",
        SENT => "sent to another node, not back yet",
        RUNNING => "still running when the report was written",
        "cancelled" => "cancelled",
        _ => return None,
    })
}

fn stage_words(stage: &str) -> Option<&'static str> {
    Some(match stage {
        "only candidate" => "only one attempt left a change, so there was nothing to compare",
        "regression filter" => "the tests removed every other attempt",
        "judge" => "the judge compared the attempts that passed the tests",
        "judge, all broke" => "every attempt failed the tests, so the judge compared them all",
        _ => return None,
    })
}

/// Python's `str(value)` for what the report prints from a record.
fn show(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        other => other.to_string(),
    }
}

/// Python truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn get_or<'a>(value: &'a Value, key: &str, default: &'a Value) -> &'a Value {
    value.get(key).unwrap_or(default)
}

fn first_line(task: &Value) -> String {
    let text = task["task"].as_str().unwrap_or("");
    text.trim().lines().next().unwrap_or("").to_string()
}

fn chars(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn minutes(seconds: &Value) -> String {
    match seconds.as_i64() {
        None => "—".to_string(),
        Some(s) if s >= 60 => format!("{} min {} s", s.div_euclid(60), s.rem_euclid(60)),
        Some(s) => format!("{s} s"),
    }
}

fn result_of(task: &Value) -> Value {
    match task.get("result") {
        Some(result) if truthy(result) => result.clone(),
        _ => Value::Object(serde_json::Map::new()),
    }
}

pub fn render(started: DateTime<Local>, finished: DateTime<Local>, tasks: &[Value], notes: &[String]) -> String {
    let mut lines = vec![
        format!("# Night Shift — {}", started.format("%Y-%m-%d")),
        String::new(),
        format!("Run started {}, finished {}.", started.format("%H:%M"), finished.format("%H:%M")),
        String::new(),
    ];
    lines.extend(summary(tasks));
    let mut by_repo: Vec<(String, Vec<&Value>)> = Vec::new();
    for task in tasks {
        let repo = task.get("repo").map(show).unwrap_or_else(|| "?".to_string());
        match by_repo.iter_mut().find(|(name, _)| *name == repo) {
            Some((_, list)) => list.push(task),
            None => by_repo.push((repo, vec![task])),
        }
    }
    for (repo, repo_tasks) in &by_repo {
        lines.push(format!("## {repo}"));
        lines.push(String::new());
        for task in repo_tasks {
            lines.extend(task_block(task));
            lines.push(String::new());
        }
    }
    if !tasks.is_empty() {
        lines.extend(review(tasks));
    }
    if !notes.is_empty() {
        lines.extend([String::new(), "## Notes".to_string(), String::new()]);
        lines.extend(notes.iter().map(|note| format!("- {note}")));
    }
    let joined = lines.join("\n");
    let cleaned: String = joined
        .chars()
        .filter(|c| {
            let code = u32::from(*c);
            !(code <= 0x08 || (0x0b..=0x1f).contains(&code) || (0x7f..=0x9f).contains(&code))
        })
        .collect();
    cleaned.trim_end().to_string() + "\n"
}

fn group(task: &Value) -> &'static str {
    let status = task["status"].as_str().unwrap_or("");
    if status == "done" && truthy(&result_of(task)["branch"]) {
        return READY;
    }
    if NOTHING_STATUSES.contains(&status) { NOTHING } else { LOOK }
}

fn summary(tasks: &[Value]) -> Vec<String> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let status_of = |task: &Value| task["status"].as_str().map(str::to_string);
    let mut counts: Vec<String> = Vec::new();
    for status in COUNT_ORDER {
        let n = tasks.iter().filter(|task| status_of(task).as_deref() == Some(status)).count();
        if n > 0 {
            counts.push(format!("{n} {status}"));
        }
    }
    let other = tasks
        .iter()
        .filter(|task| !status_of(task).is_some_and(|status| COUNT_ORDER.contains(&status.as_str())))
        .count();
    if other > 0 {
        counts.push(format!("{other} other"));
    }
    let mut lines = vec!["## Summary".to_string(), String::new(), counts.join(", "), String::new()];
    for wanted in [READY, LOOK, NOTHING] {
        for task in tasks {
            if group(task) != wanted {
                continue;
            }
            let status = get_or(task, "status", &Value::Null);
            let verdict = verdict(status.as_str().unwrap_or("None"))
                .map(str::to_string)
                .unwrap_or_else(|| format!("status {}", show(status)));
            let verb = if wanted == READY { "apply" } else { "show" };
            let id = show(get_or(task, "id", &Value::Null));
            lines.push(format!("- {id} ({wanted}): {} — {verdict}; `ragon night {verb} {id}`", chars(&first_line(task), 100)));
        }
    }
    lines.push(String::new());
    lines
}

/// Python's `shlex.quote`.
pub fn shell_quote(text: &str) -> String {
    if text.is_empty() {
        return "''".to_string();
    }
    let safe = text.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c));
    if safe { text.to_string() } else { format!("'{}'", text.replace('\'', "'\"'\"'")) }
}

fn review(tasks: &[Value]) -> Vec<String> {
    let mut lines = vec!["## Review".to_string(), String::new()];
    let branches: Vec<&Value> = tasks.iter().filter(|task| truthy(&result_of(task)["branch"])).collect();
    for task in &branches {
        let base = chars(task["base"].as_str().unwrap_or(""), 12);
        lines.push(format!(
            "- `git -C {} diff {base}..{}`",
            shell_quote(task["repo"].as_str().unwrap_or("")),
            show(&task["result"]["branch"])
        ));
    }
    if branches.is_empty() {
        lines.push("- No branch to review.".to_string());
    } else {
        lines.push("- Nothing was merged into your checkout. `ragon night apply <id>` (or `/night apply <id>`), run in the repository, brings a task's changes there as uncommitted changes; it refuses, changing nothing, if they conflict with yours.".to_string());
    }
    lines.push("- `git worktree list` shows the worktrees kept for interrupted tasks.".to_string());
    lines
}

fn task_block(task: &Value) -> Vec<String> {
    let result = result_of(task);
    let id = show(get_or(task, "id", &Value::Null));
    let status = get_or(task, "status", &Value::Null);
    let status_text = status.as_str().unwrap_or("");
    let mut lines = vec![format!("### {id} — {}", show(status)), String::new(), format!("- Task: {}", first_line(task))];
    lines.extend(where_lines(task, &result));
    if status_text == SENT {
        lines.push("- Not finished there yet: its branch comes back with a later run.".to_string());
    }
    if truthy(&result["branch"]) {
        lines.push(format!(
            "- Branch: `{}`, not merged; `ragon night apply {id}` brings it into your checkout",
            show(&result["branch"])
        ));
    }
    if truthy(&result["diff_stat"]) {
        let stat = result["diff_stat"].as_str().unwrap_or("");
        lines.push(format!("- Changes: {}", stat.lines().last().unwrap_or("").trim()));
    }
    if truthy(&result["test_command"]) {
        let sandbox = if truthy(&result["test_sandbox"]) { format!(", sandbox: {}", show(&result["test_sandbox"])) } else { String::new() };
        let question = Value::String("?".to_string());
        lines.push(format!(
            "- Tests: `{}` ({}): {}{sandbox}",
            show(&result["test_command"]),
            show(get_or(&result, "test_source", &question)),
            show(get_or(&result, "test_result", &question))
        ));
    }
    let zero = Value::from(0);
    let attempts = result.get("attempts").unwrap_or_else(|| get_or(task, "attempts", &zero));
    lines.push(format!(
        "- Attempts: {}, nudges: {}, wall time: {}",
        show(attempts),
        show(get_or(&result, "nudges", &zero)),
        minutes(&result["wall_s"])
    ));
    if let Some(best) = result.get("best_of").filter(|best| best.is_object()) {
        lines.extend(best_of_lines(best));
    }
    if let Some(refine) = result.get("refine").filter(|refine| refine.is_object()) {
        lines.push(refine_line(refine));
    }
    if LAST_MESSAGE_STATUSES.contains(&status_text) && truthy(&result["last_message"]) {
        let message = result["last_message"].as_str().unwrap_or("").trim().replace('\n', " ");
        lines.push(format!("- Last message: {}", chars(&message, 400)));
    }
    if status_text == RUNNING {
        lines.push("- Still running when the report was written.".to_string());
    }
    lines
}

fn where_lines(task: &Value, result: &Value) -> Vec<String> {
    if truthy(&task["on"]) {
        let name = &task["sent_to"]["name"];
        let target = if truthy(name) { show(name) } else { show(&task["on"]) };
        return vec![if truthy(&result["node"]) {
            format!("- Ran on: {target}, by that node's own runner and model server")
        } else {
            format!("- Sent to: {target}")
        }];
    }
    if truthy(&result["model_node"]) {
        return vec![format!("- Model server: {}'s (the task ran on this machine)", show(&result["model_node"]))];
    }
    Vec::new()
}

fn refine_line(refine: &Value) -> String {
    let edited = if truthy(&refine["study_edited"]) { "; it changed the worktree, which was put back" } else { "" };
    let version = refine.get("version").map(show).unwrap_or_else(|| FIRST_REFINE.to_string());
    let version = if version != FIRST_REFINE { format!(" (refine-{version})") } else { String::new() };
    let zero = Value::from(0);
    format!(
        "- Refine: studied first for {}, a {}-character description{edited}{version}",
        minutes(&refine["study_s"]),
        show(get_or(refine, "refined_chars", &zero))
    )
}

fn best_of_lines(best: &Value) -> Vec<String> {
    let chosen = &best["chosen"];
    let question = Value::String("?".to_string());
    let n = show(get_or(best, "n", &question));
    let mut ran = format!("{} of {n} attempts ran", show(get_or(best, "ran", &Value::from(0))));
    if truthy(&best["why"]) {
        ran.push_str(&format!(" ({})", show(&best["why"])));
    }
    let mut lines = Vec::new();
    if chosen.is_null() {
        lines.push(format!("- Best of {n}: {ran}; none chosen"));
    } else {
        let mut points: Vec<(String, f64)> = best["points"]
            .as_object()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect())
            .unwrap_or_default();
        points.sort_by(|x, y| x.0.cmp(&y.0));
        let scored = if points.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = points.iter().map(|(k, v)| format!("{k} {v:.2}")).collect();
            format!("; points {}", parts.join(", "))
        };
        lines.push(format!(
            "- Best of {n}: {ran}; chose attempt {} by {}{scored}",
            show(chosen),
            show(get_or(best, "stage", &question))
        ));
    }
    if truthy(&best["reason"]) {
        let stage = if truthy(&best["stage"]) { show(&best["stage"]) } else { String::new() };
        let tail = stage_words(&stage).map(|words| format!(" (deciding stage: {words})")).unwrap_or_default();
        lines.push(format!("  - Why: {}{tail}", show(&best["reason"])));
    }
    if truthy(&best["judge_calls"]) {
        let errors = if truthy(&best["judge_errors"]) { format!(", {} unreadable or failed", show(&best["judge_errors"])) } else { String::new() };
        let log = if truthy(&best["judge_log"]) { format!("; answers in `{}`", show(&best["judge_log"])) } else { String::new() };
        lines.push(format!("  - Judge: {} call(s){errors}{log}", show(&best["judge_calls"])));
    }
    if let Some(entries) = best["attempts"].as_array() {
        lines.extend(entries.iter().map(|entry| attempt_line(entry, chosen)));
    }
    lines
}

fn attempt_line(entry: &Value, chosen: &Value) -> String {
    let question = Value::String("?".to_string());
    let mut parts = vec![show(get_or(entry, "status", &question))];
    if truthy(&entry["changes"]) {
        parts.push(show(&entry["changes"]));
    }
    if truthy(&entry["test_result"]) {
        parts.push(format!("tests {}", show(&entry["test_result"])));
    }
    parts.push(minutes(&entry["wall_s"]));
    let attempt = get_or(entry, "attempt", &Value::Null);
    if attempt == chosen {
        parts.push("chosen: the branch".to_string());
    } else if truthy(&entry["diff"]) {
        parts.push(format!("diff `{}`", show(&entry["diff"])));
    }
    for (key, label) in [("log", "log"), ("test_log", "test log")] {
        if truthy(&entry[key]) {
            parts.push(format!("{label} `{}`", show(&entry[key])));
        }
    }
    if truthy(&entry["hint"]) {
        parts.push(format!("hint: {}", show(&entry["hint"])));
    }
    format!("  - Attempt {}: {}", show(attempt), parts.join(", "))
}

/// `reports/<date>.md`, or `<date>-2.md`, `-3`, … when a report of that date exists.
pub fn write(night: &Path, started: DateTime<Local>, text: &str) -> std::io::Result<PathBuf> {
    let reports = night.join("reports");
    std::fs::create_dir_all(&reports)?;
    let date = started.format("%Y-%m-%d").to_string();
    let mut path = reports.join(format!("{date}.md"));
    let mut counter = 2;
    while path.exists() {
        path = reports.join(format!("{date}-{counter}.md"));
        counter += 1;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}
