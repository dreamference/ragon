//! One task, or one attempt of a best-of-N task: a worktree and a branch of its own, `ragon exec`
//! in it, nudges while the agent only announced its work, the tests, then a commit on the branch
//! (one attempt) or the staged change kept for the chooser (an attempt). The Python runner's
//! `NightShiftTaskRun`, without refine and remote nodes.

use std::cell::Cell;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ragon_airgapped::Level;
use serde_json::{Map, Value, json};

use super::capped::{Cap, Exit, Verb, run_capped};
use super::{Ctx, is_cancelled, transition};

pub const PREAMBLE: &str = "This is a Night Shift task. It runs unattended overnight, in a git worktree of {repo} on branch {branch}, and\nnobody will answer questions: where something is unclear, make the reasonable choice and say which you made.\n\n- Work only inside this worktree.\n- Make the change itself; do not stop at describing or planning it.\n- Do not commit, push, merge or switch branches. The night run commits your changes.\n{test_line}- End by listing the files you changed.\n\nTask:\n{task}";
pub const HINT_INTRO: &str = "\n\nStrategy for this attempt: ";
pub const NUDGE: &str = "Go ahead and make the change now.";
pub const RESUME: &str = "This task was cut off by the end of last night's window. Continue it where you left off and finish it.";
pub const REFUSED_UNSEALED: &str =
    "/airgapped is on and `[night] test_sandbox` is false, so nothing would keep the tests off the network";
const ANNOUNCES_WORK: &str = r"(?i)\b(I'll|I will|I'm going to|I am going to|Let me|Next,? I|I can now|I'm about to|I shall|now I'll|I'll now|going to (?:make|add|update|implement|write|create|fix))\b";
const TEST_TAIL_LINES: usize = 200;
const WINDOW: &str = "window";

/// What the tests did.
struct Tests {
    refused: bool,
    exit_code: Option<i32>,
    timed_out: Option<&'static str>,
    tail: String,
    sandbox: String,
}

pub struct TaskRun<'a> {
    ctx: &'a Ctx,
    pub id: String,
    repo: PathBuf,
    base: String,
    pub branch: String,
    text: String,
    test_override: Option<String>,
    author: Option<Value>,
    recorded_level: Option<Level>,
    pub attempt: Option<u32>,
    pub hint: Option<String>,
    deadline: Instant,
    pub session: Option<String>,
    pub worktree: PathBuf,
    pub log_path: PathBuf,
    last_message_path: PathBuf,
    diff_path: PathBuf,
    pub test_log_path: PathBuf,
    started: Instant,
    attempts: u64,
    nudges_used: u32,
    level: Option<Level>,
    counter: Cell<u32>,
    /// An attempt's end: `{attempt, status, result}`.
    pub outcome: Option<Value>,
}

fn text_of(task: &Value, key: &str) -> Option<String> {
    task.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from)
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn announces_work(message: &str) -> bool {
    !message.is_empty() && regex_lite::Regex::new(ANNOUNCES_WORK).is_ok_and(|r| r.is_match(message))
}

/// The id of the first `thread.started` event written to `log` after `offset`.
pub fn thread_id_in(log: &Path, offset: u64) -> Option<String> {
    let mut file = std::fs::File::open(log).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    for line in BufReader::new(file).split(b'\n').map_while(Result::ok) {
        let Ok(event) = serde_json::from_slice::<Value>(&line) else { continue };
        if event.get("type").and_then(Value::as_str) == Some("thread.started") {
            return event.get("thread_id").and_then(Value::as_str).map(String::from);
        }
    }
    None
}

/// The shell a test command runs in: `bash -c` (`cmd /C` on Windows).
fn shell(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), command.into()]
    } else {
        vec!["bash".into(), "-c".into(), command.into()]
    }
}

impl<'a> TaskRun<'a> {
    /// `attempt` is the attempt's number in a best-of-N task, `None` for a task of one attempt.
    pub fn new(ctx: &'a Ctx, task: &Value, deadline: Instant, attempt: Option<u32>, hint: Option<String>) -> Self {
        let id = text_of(task, "id").unwrap_or_default();
        let suffix = attempt.map(|k| format!("-a{k}")).unwrap_or_default();
        let base_branch = text_of(task, "branch").unwrap_or_else(|| format!("night/{id}"));
        let logs = ctx.night.join("logs");
        TaskRun {
            ctx,
            repo: PathBuf::from(text_of(task, "repo").unwrap_or_default()),
            base: text_of(task, "base").unwrap_or_default(),
            branch: format!("{base_branch}{suffix}"),
            text: text_of(task, "task").unwrap_or_default(),
            test_override: text_of(task, "test"),
            author: task.get("author").filter(|a| a.is_object()).cloned(),
            recorded_level: task.get("airgapped").and_then(Value::as_str).and_then(Level::parse),
            attempt,
            hint,
            deadline,
            session: if attempt.is_some() { None } else { text_of(task, "session") },
            worktree: ctx.night.join("worktrees").join(format!("{id}{suffix}")),
            log_path: logs.join(format!("{id}{suffix}.jsonl")),
            last_message_path: logs.join(format!("{id}{suffix}.last.txt")),
            diff_path: logs.join(format!("{id}{suffix}.diff")),
            test_log_path: logs.join(format!("{id}{suffix}.test.log")),
            started: Instant::now(),
            attempts: task.get("attempts").and_then(Value::as_u64).unwrap_or(0),
            nudges_used: 0,
            level: None,
            counter: Cell::new(0),
            outcome: None,
            id,
        }
    }

    pub fn run(&mut self) -> String {
        if self.attempt.is_some() {
            if self.cancelled() {
                return self.finish("cancelled", None, None);
            }
            return self.run_steps();
        }
        if self.cancelled() {
            return transition(&self.ctx.night, &self.id, "cancelled", None, vec![]);
        }
        self.attempts += 1;
        transition(&self.ctx.night, &self.id, "running", None, vec![("attempts", json!(self.attempts))]);
        self.run_steps()
    }

    fn git(&self, cwd: &Path, verb: Verb, args: &[&str]) -> (i32, String) {
        self.ctx.git.run(cwd, verb, args)
    }

    fn cancelled(&self) -> bool {
        is_cancelled(&self.ctx.night, &self.id)
    }

    fn run_steps(&mut self) -> String {
        if let Some(problem) = self.prepare_worktree() {
            return self.finish("failed", Some(&problem), None);
        }
        let (test_command, mut test_source) = self.detect_test();
        if self.cancelled() {
            return self.cleanup_and_finish("cancelled", None);
        }
        self.fix_level();
        let prompt = if self.session.is_some() { RESUME.to_string() } else { self.compose_prompt(test_command.as_deref()) };
        let outcome = self.exec(&prompt, self.session.is_some());
        if outcome == "interrupted" {
            return self.interrupted();
        }
        if outcome == "error" && !self.has_changes() {
            let last = self.last_message();
            let last = if last.is_empty() { "ragon exec exited with an error; see the log".to_string() } else { last };
            return self.cleanup_and_finish("failed", Some(last));
        }
        while !self.has_changes() && self.nudges_used < self.ctx.settings.nudges && announces_work(&self.last_message()) {
            if self.cancelled() {
                return self.cleanup_and_finish("cancelled", None);
            }
            self.nudges_used += 1;
            if self.exec(NUDGE, true) == "interrupted" {
                return self.interrupted();
            }
        }
        if self.cancelled() {
            return self.cleanup_and_finish("cancelled", None);
        }
        if !self.has_changes() {
            let last = self.last_message();
            let status = if announces_work(&last) { "stalled" } else { "no-change" };
            return self.cleanup_and_finish(status, Some(last));
        }
        self.git(&self.worktree, Verb::Add, &["-A"]);
        let mut tests = test_command.as_deref().map(|command| self.run_tests(command));
        if tests.as_ref().is_some_and(|t| t.refused) {
            tests = None;
            test_source = format!("{test_source}; not run: {REFUSED_UNSEALED}");
        }
        if self.cancelled() {
            return self.cleanup_and_finish("cancelled", None);
        }
        if tests.as_ref().is_some_and(|t| t.timed_out == Some(WINDOW)) {
            return self.interrupted();
        }
        self.commit(test_command.as_deref(), &test_source, tests.as_ref())
    }

    fn prepare_worktree(&self) -> Option<String> {
        let worktree = path_text(&self.worktree);
        if self.attempt.is_some() {
            if self.worktree.is_dir() {
                self.git(&self.repo, Verb::Worktree, &["remove", "--force", &worktree]);
            }
            self.git(&self.repo, Verb::Branch, &["-D", &self.branch]);
        }
        if self.worktree.is_dir() {
            return None;
        }
        if self.git(&self.repo, Verb::CatFile, &["-e", &format!("{}^{{commit}}", self.base)]).0 != 0 {
            let short: String = self.base.chars().take(10).collect();
            return Some(format!("the base commit {short} no longer exists in {}", self.repo.display()));
        }
        if let Some(parent) = self.worktree.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let reference = format!("refs/heads/{}", self.branch);
        let exists = self.git(&self.repo, Verb::RevParse, &["--verify", "--quiet", &reference]).0 == 0;
        let mut args: Vec<&str> = vec!["add", "-q", &worktree];
        if exists {
            args.push(&self.branch);
        } else {
            args.extend(["-b", self.branch.as_str(), self.base.as_str()]);
        }
        let (code, output) = self.git(&self.repo, Verb::Worktree, &args);
        (code != 0).then(|| format!("git worktree add failed: {}", tail_chars(output.trim(), 300)))
    }

    /// The test command and where it came from, in the Python runner's order.
    fn detect_test(&self) -> (Option<String>, String) {
        if let Some(command) = &self.test_override {
            return (Some(command.clone()), "--test".into());
        }
        let (code, shown) = self.git(&self.worktree, Verb::Show, &[&format!("{}:dreamference.toml", self.base)]);
        let from_repo = (code == 0)
            .then(|| shown.parse::<toml::Table>().ok())
            .flatten()
            .and_then(|t| t.get("night").and_then(|n| n.get("test")).and_then(|v| v.as_str()).map(String::from))
            .filter(|s| !s.is_empty());
        if let Some(command) = from_repo {
            return (Some(command), "dreamference.toml [night] test".into());
        }
        if let Some(command) = &self.ctx.settings.test {
            return (Some(command.clone()), "config [night] test".into());
        }
        let wt = &self.worktree;
        if ["pyproject.toml", "pytest.ini", "setup.py"].iter().any(|n| wt.join(n).is_file()) && wt.join("tests").is_dir() {
            for venv in [wt.join(".venv"), self.repo.join(".venv")] {
                let python = if cfg!(windows) { venv.join("Scripts").join("python.exe") } else { venv.join("bin").join("python") };
                if python.is_file() {
                    let quoted = super::report::shell_quote(&path_text(&python));
                    return (Some(format!("{quoted} -m pytest -q")), "pytest (repository .venv)".into());
                }
            }
            return (Some("python3 -m pytest -q".into()), "pytest".into());
        }
        if wt.join("Cargo.toml").is_file() {
            return (Some("cargo test".into()), "Cargo.toml".into());
        }
        let has_test_script = std::fs::read_to_string(wt.join("package.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|p| p.get("scripts").and_then(|s| s.get("test")).cloned())
            .is_some_and(|t| t.as_str().is_some_and(|s| !s.is_empty()) || t.as_bool() == Some(true));
        if has_test_script {
            return (Some("npm test".into()), "package.json".into());
        }
        (None, "none found".into())
    }

    /// The `/airgapped` level: the highest of the worktree's, the repository's, `[night]
    /// airgapped` and the one recorded; recorded on the task when it changed.
    fn fix_level(&mut self) -> Level {
        if let Some(level) = self.level {
            return level;
        }
        let ids = [self.session.as_deref().unwrap_or("")];
        let environment = std::env::var(ragon_airgapped::ENV_VAR).ok();
        let mut levels: Vec<Level> = vec![ragon_airgapped::resolve_in(&ids, environment.clone(), &self.worktree).level];
        if self.repo != self.worktree {
            levels.push(ragon_airgapped::resolve_in(&ids, environment, &self.repo).level);
        }
        let configured = match &self.ctx.settings.airgapped {
            Some(Value::Bool(on)) => Some(if *on { Level::On } else { Level::Off }),
            Some(Value::String(name)) => Level::parse(name),
            _ => None,
        };
        levels.extend(configured);
        levels.extend(self.recorded_level);
        let level = levels.into_iter().max().unwrap_or(Level::Off);
        self.level = Some(level);
        if Some(level) != self.recorded_level {
            transition(&self.ctx.night, &self.id, "running", None, vec![("airgapped", json!(level.name()))]);
        }
        level
    }

    pub fn compose_prompt(&self, test_command: Option<&str>) -> String {
        let test_line = test_command
            .map(|cmd| format!("- When the change is made, run `{cmd}` and fix what it reports.\n"))
            .unwrap_or_default();
        let prompt = PREAMBLE
            .replace("{repo}", &self.repo.display().to_string())
            .replace("{branch}", &self.branch)
            .replace("{test_line}", &test_line)
            .replace("{task}", &self.text);
        match &self.hint {
            Some(hint) if !hint.is_empty() => format!("{prompt}{HINT_INTRO}{hint}"),
            _ => prompt,
        }
    }

    fn unit(&self) -> String {
        self.counter.set(self.counter.get() + 1);
        format!("ragon-night-{}-{}", self.id, self.counter.get())
    }

    fn capped(&self, command: &[String], output: &Path, timeout: Option<Duration>, append: bool, extra: &[(String, String)]) -> Exit {
        let limit = match timeout {
            Some(timeout) => self.deadline.min(Instant::now() + timeout),
            None => self.deadline,
        };
        let unit = self.unit();
        let stop = || self.ctx.stopped();
        let cap = Cap { unit: &unit, memory: &self.ctx.settings.task_memory, use_scope: self.ctx.use_scope, limit, stop: &stop };
        let words: Vec<OsString> = command.iter().map(OsString::from).collect();
        let env = self.ctx.child_env(self.level.map(Level::name), extra);
        run_capped(&words, &self.worktree, output, append, &env, &cap).unwrap_or(Exit::Code(-1))
    }

    fn exec(&mut self, prompt: &str, resume: bool) -> &'static str {
        let mut command: Vec<String> = vec![
            path_text(&self.ctx.ragon),
            "exec".into(),
            "--json".into(),
            "-o".into(),
            path_text(&self.last_message_path),
            "-C".into(),
            path_text(&self.worktree),
            "-s".into(),
            "workspace-write".into(),
            "--skip-git-repo-check".into(),
        ];
        if let Some(budget) = self.ctx.context_budget {
            command.extend(["-c".into(), format!("model_auto_compact_token_limit={budget}")]);
        }
        command.extend(["-c".into(), "sandbox_workspace_write.writable_roots=[]".into()]);
        match (&self.session, resume) {
            (Some(session), true) => command.extend(["resume".into(), session.clone(), prompt.into()]),
            _ => command.push(prompt.into()),
        }
        if let Some(parent) = self.log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let offset = std::fs::metadata(&self.log_path).map(|m| m.len()).unwrap_or(0);
        let exit = self.capped(&command, &self.log_path.clone(), None, true, &[]);
        if let Some(session) = thread_id_in(&self.log_path, offset)
            && self.session.as_deref() != Some(session.as_str())
        {
            self.session = Some(session.clone());
            if self.attempt.is_none() {
                transition(&self.ctx.night, &self.id, "running", None, vec![("session", json!(session))]);
            }
        }
        match exit {
            Exit::TimedOut => "interrupted",
            Exit::Code(0) => "ok",
            Exit::Code(_) => "error",
        }
    }

    fn run_tests(&mut self, test_command: &str) -> Tests {
        let level = self.fix_level();
        let settings = &self.ctx.settings;
        let sandboxed = self.ctx.use_sandbox && settings.test_sandbox;
        let sealed = level == Level::On;
        if sealed && !settings.test_sandbox {
            return Tests { refused: true, exit_code: None, timed_out: None, tail: String::new(), sandbox: String::new() };
        }
        let mut command = shell(test_command);
        let mut extra = Vec::new();
        if sandboxed {
            let mut words: Vec<String> = vec![
                path_text(&self.ctx.ragon),
                "sandbox".into(),
                "-c".into(),
                "sandbox_mode=\"workspace-write\"".into(),
                "-c".into(),
                "sandbox_workspace_write.writable_roots=[]".into(),
                "-c".into(),
                format!("sandbox_workspace_write.network_access={}", if sealed { "false" } else { "true" }),
                "--".into(),
            ];
            words.extend(command);
            command = words;
            if let Some(session) = &self.session {
                extra.push(("CODEX_THREAD_ID".to_string(), session.clone()));
            }
        }
        let output = self.test_log_path.clone();
        if let Some(parent) = output.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let exit = self.capped(&command, &output, Some(Duration::from_secs(settings.test_timeout_s)), false, &extra);
        let text = std::fs::read(&output).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(TEST_TAIL_LINES)..].join("\n");
        let sandbox = match (sandboxed, sealed) {
            (true, true) => "workspace-write, no network (/airgapped on)",
            (true, false) => "workspace-write",
            (false, _) => "off ([night] test_sandbox = false)",
        };
        let (exit_code, timed_out) = match exit {
            Exit::Code(code) => (Some(code), None),
            Exit::TimedOut => {
                let window = Instant::now() >= self.deadline || self.ctx.stopped();
                (None, Some(if window { WINDOW } else { "test_timeout" }))
            }
        };
        Tests { refused: false, exit_code, timed_out, tail, sandbox: sandbox.into() }
    }

    fn commit(&mut self, test_command: Option<&str>, test_source: &str, tests: Option<&Tests>) -> String {
        if self.attempt.is_some() {
            let mut result = self.result(test_command, test_source, tests);
            result.extend(self.staged_change(false));
            return self.finish("candidate", None, Some(result));
        }
        let (outcome, detail) = self.commit_staged();
        if outcome != "done" {
            let note = format!("git commit failed: {}", tail_chars(detail.trim(), 300));
            let result = self.result(test_command, test_source, tests);
            return self.finish("failed", Some(&note), Some(result));
        }
        let stat = self.git(&self.repo, Verb::Diff, &["--stat", &format!("{}..{}", self.base, self.branch)]).1;
        self.git(&self.repo, Verb::Worktree, &["remove", "--force", &path_text(&self.worktree)]);
        let mut result = self.result(test_command, test_source, tests);
        result.insert("branch".into(), json!(self.branch));
        result.insert("diff_stat".into(), json!(stat.trim_end()));
        self.finish("done", None, Some(result))
    }

    /// Commits what is staged in the worktree on its branch, hooks off, as `night: <first line>`.
    /// Returns `("done", "")` or `("failed", git's output)`.
    pub fn commit_staged(&self) -> (&'static str, String) {
        let first: String = self.text.trim().lines().next().unwrap_or("task").chars().take(72).collect();
        let first = if first.is_empty() { "task".to_string() } else { first };
        let message = format!("night: {first}");
        let mut config = vec!["commit.gpgsign=false".to_string()];
        if let Some(author) = &self.author {
            let field = |key: &str| author.get(key).and_then(Value::as_str).unwrap_or("").to_string();
            config.push(format!("user.name={}", field("name")));
            config.push(format!("user.email={}", field("email")));
        }
        let (code, output) = self.ctx.git.run_as(&self.worktree, &config, Verb::Commit, &["-q", "-m", &message]);
        if code == 0 { ("done", String::new()) } else { ("failed", output) }
    }

    fn has_changes(&self) -> bool {
        let (code, output) = self.git(&self.worktree, Verb::Status, &["--porcelain"]);
        code == 0 && !output.trim().is_empty()
    }

    fn last_message(&self) -> String {
        std::fs::read(&self.last_message_path).map(|b| String::from_utf8_lossy(&b).trim().to_string()).unwrap_or_default()
    }

    fn window_closed(&self) -> bool {
        self.ctx.window_over()
    }

    fn staged_change(&self, stage: bool) -> Map<String, Value> {
        if stage {
            self.git(&self.worktree, Verb::Add, &["-A"]);
        }
        let diff = self.git(&self.worktree, Verb::Diff, &["--cached", "--binary"]).1;
        if let Some(parent) = self.diff_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.diff_path, diff);
        let stat = self.git(&self.worktree, Verb::Diff, &["--cached", "--stat"]).1;
        let mut change = Map::new();
        change.insert("diff".into(), json!(path_text(&self.diff_path)));
        change.insert("diff_stat".into(), json!(stat.trim_end()));
        change.insert("worktree".into(), json!(path_text(&self.worktree)));
        change.insert("branch".into(), json!(self.branch));
        change.insert("session".into(), json!(self.session));
        change
    }

    fn interrupted(&mut self) -> String {
        if self.attempt.is_some() {
            if !self.window_closed() && self.has_changes() {
                let mut result = Map::new();
                result.insert("timed_out".into(), json!(true));
                result.insert("test_result".into(), json!("untested (the attempt's time ran out)"));
                result.insert("last_message".into(), json!(self.last_message()));
                result.extend(self.timing());
                result.extend(self.staged_change(true));
                return self.finish("candidate", None, Some(result));
            }
            let status = if self.window_closed() { "interrupted" } else { "timed-out" };
            let mut result = Map::new();
            result.insert("last_message".into(), json!(self.last_message()));
            result.extend(self.timing());
            return self.finish(status, None, Some(result));
        }
        let mut result = Map::new();
        result.insert("branch".into(), json!(self.branch));
        result.insert("worktree".into(), json!(path_text(&self.worktree)));
        result.insert("last_message".into(), json!(self.last_message()));
        result.extend(self.timing());
        self.finish("interrupted", None, Some(result))
    }

    fn cleanup_and_finish(&mut self, status: &str, last_message: Option<String>) -> String {
        if self.worktree.is_dir() {
            self.git(&self.repo, Verb::Worktree, &["remove", "--force", &path_text(&self.worktree)]);
        }
        self.git(&self.repo, Verb::Branch, &["-D", &self.branch]);
        let mut result = self.timing();
        let keeps = status == "stalled" || status == "failed";
        if let Some(last) = last_message.filter(|_| keeps) {
            result.insert("last_message".into(), json!(last));
        }
        if keeps {
            result.insert("log".into(), json!(path_text(&self.log_path)));
        }
        self.finish(status, None, Some(result))
    }

    fn finish(&mut self, status: &str, note: Option<&str>, result: Option<Map<String, Value>>) -> String {
        let mut result = result.filter(|r| !r.is_empty()).unwrap_or_else(|| self.timing());
        if status == "failed"
            && let Some(note) = note
        {
            result.entry("last_message").or_insert_with(|| json!(note));
        }
        if let Some(attempt) = self.attempt {
            self.outcome = Some(json!({"attempt": attempt, "status": status, "result": Value::Object(result)}));
            return status.to_string();
        }
        transition(
            &self.ctx.night,
            &self.id,
            status,
            note,
            vec![("result", Value::Object(result)), ("attempts", json!(self.attempts))],
        )
    }

    fn result(&self, test_command: Option<&str>, test_source: &str, tests: Option<&Tests>) -> Map<String, Value> {
        let mut result = self.timing();
        result.insert("test_command".into(), json!(test_command.unwrap_or("none")));
        result.insert("test_source".into(), json!(test_source));
        match tests {
            Some(tests) => {
                let verdict = match (tests.timed_out, tests.exit_code) {
                    (Some(_), _) => format!("timed out after {} min", self.ctx.settings.test_timeout_s / 60),
                    (None, Some(0)) => "passed".to_string(),
                    (None, code) => format!("failed (exit {})", code.unwrap_or(-1)),
                };
                result.insert("test_result".into(), json!(verdict));
                result.insert("test_tail".into(), json!(tests.tail));
                result.insert("test_sandbox".into(), json!(tests.sandbox));
            }
            None => {
                result.insert("test_result".into(), json!("untested"));
            }
        }
        result
    }

    fn timing(&self) -> Map<String, Value> {
        let mut timing = Map::new();
        timing.insert("attempts".into(), json!(self.attempts));
        timing.insert("nudges".into(), json!(self.nudges_used));
        timing.insert("wall_s".into(), json!(self.started.elapsed().as_secs()));
        if let Some(node) = &self.ctx.model_node {
            timing.insert("model_node".into(), json!(node));
        }
        timing
    }
}

/// The last `count` characters of `text` (Python's `text[-count:]`).
pub fn tail_chars(text: &str, count: usize) -> String {
    let total = text.chars().count();
    text.chars().skip(total.saturating_sub(count)).collect()
}
