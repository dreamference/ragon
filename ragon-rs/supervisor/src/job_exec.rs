//! Runs one job (`server job exec <id>`, started by the runner inside the job's Job Object): a
//! worktree of the job repository at the job's commit, the setup command, the command, the test,
//! and a commit of what changed on `job/<id>`. The port of `NodeJob.execute` in
//! `dreamference/node/node_job.py`; platform-neutral, so the Mac node reuses it.
//!
//! The commands run in the node's job sandbox ([`Sandbox`]). On Windows that is the launcher's
//! `ragon sandbox` at the UNELEVATED level (a restricted token of the job's own account, here the
//! service's `NT SERVICE\ragon-node`): it never provisions the shared `CodexSandbox*` accounts,
//! whose passwords every user's elevated setup resets, which is how W11 (the owner's sandbox and
//! the job account's sandbox breaking each other) is avoided rather than fixed.
//!
//! The runner asks a job to stop by writing the `stop` file in its folder (the text is the note
//! to record); this process then ends its command and records the job without a commit. If it
//! does not finish within the grace, the runner kills the whole Job Object and records it itself.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::jobs::{self, JOB_STOP_FILE, OUTPUT_LOG, Store, TREE_DIR};
use crate::json::Value;

/// How a job's commands are contained beside the Job Object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sandbox {
    /// As they are (tests; the drill binary, which has no sandbox of its own).
    None,
    /// `<ragon> -c windows.sandbox="unelevated" sandbox -P <profile> -C <tree> -- <command>`.
    Unelevated { ragon: PathBuf, profile: String },
}

pub const DEFAULT_PERMISSION_PROFILE: &str = ":workspace";

impl Sandbox {
    pub fn name(&self) -> &'static str {
        match self {
            Sandbox::None => "none",
            Sandbox::Unelevated { .. } => "unelevated",
        }
    }

    /// The argv that runs `command` in `tree` under this sandbox.
    pub fn wrap(&self, tree: &Path, command: &[String]) -> Vec<String> {
        match self {
            Sandbox::None => command.to_vec(),
            Sandbox::Unelevated { ragon, profile } => {
                let mut argv = vec![
                    ragon.display().to_string(),
                    "-c".into(),
                    "windows.sandbox=\"unelevated\"".into(),
                    "sandbox".into(),
                    "-P".into(),
                    profile.clone(),
                    "-C".into(),
                    tree.display().to_string(),
                    "--".into(),
                ];
                argv.extend(command.iter().cloned());
                argv
            }
        }
    }

    /// The `--sandbox` argument that names it again (what the runner passes to `exec`).
    pub fn to_args(&self) -> Vec<String> {
        match self {
            Sandbox::None => vec!["--sandbox".into(), "none".into()],
            Sandbox::Unelevated { ragon, profile } => vec![
                "--sandbox".into(),
                "unelevated".into(),
                "--ragon".into(),
                ragon.display().to_string(),
                "--permission-profile".into(),
                profile.clone(),
            ],
        }
    }
}

/// A shell command (the setup and the test) as an argv. On Windows the command is written to the
/// batch file `script` and the argv is `cmd.exe /d /c <script>`: `ragon sandbox` (like Rust's
/// `Command`) quotes each argument for the C runtime, with `\"` for a quote, which cmd.exe does not
/// read, so a command with quotes in it would break on the way; the script's path carries none.
/// `script` lies in the job's folder, outside the tree, so it is never committed.
pub fn shell(command: &str, script: &Path) -> io::Result<Vec<String>> {
    if cfg!(windows) {
        std::fs::write(script, format!("@echo off\r\n{command}\r\nexit /b %ERRORLEVEL%\r\n"))?;
        Ok(vec!["cmd.exe".into(), "/d".into(), "/c".into(), path_str(script)])
    } else {
        let _ = script;
        Ok(vec!["bash".into(), "-c".into(), command.into()])
    }
}

/// `shlex.join`: what the commit message shows of the command.
pub fn shlex_join(words: &[String]) -> String {
    words
        .iter()
        .map(|w| {
            let safe = !w.is_empty() && w.bytes().all(|c| c.is_ascii_alphanumeric() || b"@%+=:,./-_".contains(&c));
            if safe { w.clone() } else { format!("'{}'", w.replace('\'', "'\"'\"'")) }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs git with the node's own settings only: no hooks, no prompt, the job repository trusted
/// explicitly (another account pushes to it, so its owner is not this account).
pub fn git(args: &[&str]) -> Output {
    let hooks = if cfg!(windows) { "core.hooksPath=NUL" } else { "core.hooksPath=/dev/null" };
    let result = Command::new("git")
        .args(["-c", hooks, "-c", "safe.directory=*", "-c", "core.fsmonitor=false"])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(Stdio::null())
        .output();
    match result {
        Ok(out) => Output {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(e) => Output { code: 127, stdout: String::new(), stderr: format!("could not run git: {e}") },
    }
}

fn tail(text: &str, n: usize) -> String {
    let t = text.trim();
    let start = t.char_indices().rev().nth(n.saturating_sub(1)).map(|(i, _)| i).unwrap_or(0);
    t[start..].to_string()
}

fn path_str(path: &Path) -> String {
    path.display().to_string()
}

/// The worktree's administrative folder in `repo` (`worktrees/<name>`), as the `.git` file that
/// `git worktree add` just wrote names it; None when it names anything else. Read before the job
/// runs: the job can rewrite the worktree's `.git`.
pub fn admin_dir(tree: &Path, repo: &Path) -> Option<PathBuf> {
    let line = std::fs::read_to_string(tree.join(".git")).ok()?;
    let target = line.trim().strip_prefix("gitdir: ")?;
    let admin = plain(std::fs::canonicalize(tree.join(target)).ok()?);
    let worktrees = plain(std::fs::canonicalize(repo).ok()?).join("worktrees");
    (admin.parent() == Some(worktrees.as_path()) && admin.is_dir()).then_some(admin)
}

/// `canonicalize` on Windows returns a verbatim path (`\\?\C:\...`, `\\?\UNC\host\share\...`),
/// which git refuses as `--git-dir` ("not a git repository"); this is the same path without the
/// prefix. Other paths are returned as they are.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy().into_owned();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// Removes a job's worktree, also when the job replaced its `.git`.
pub fn remove_tree(repo: &Path, tree: &Path) {
    let (r, t) = (path_str(repo), path_str(tree));
    git(&["--git-dir", &r, "worktree", "remove", "--force", &t]);
    if tree.exists() {
        let _ = std::fs::remove_dir_all(tree);
        git(&["--git-dir", &r, "worktree", "prune"]);
    }
}

/// Commits what the job changed on `job/<id>`, git pointed at the administrative folder and never
/// at the worktree's `.git` (which the job could have replaced). None when nothing changed or a
/// step failed (the note says which).
pub fn commit(record: &mut Value, tree: &Path, admin: &Path) -> Option<String> {
    let (a, t) = (path_str(admin), path_str(tree));
    let location = ["--git-dir", a.as_str(), "--work-tree", t.as_str()];
    let run = |rest: &[&str]| git(&[&location[..], rest].concat());
    let status = run(&["status", "--porcelain"]);
    if status.code != 0 {
        record.set("note", Value::str(format!("git status failed: {}", tail(&status.stderr, 200))));
        return None;
    }
    if status.stdout.trim().is_empty() {
        return None;
    }
    let id = jobs::id_of(record).to_string();
    let branch = format!("job/{id}");
    let author = record.get("author").cloned().unwrap_or(Value::Null);
    let field = |key: &str, default: &str| author.get(key).and_then(Value::as_str).unwrap_or(default).to_string();
    let name = format!("user.name={}", field("name", "Ragon Job"));
    let email = format!("user.email={}", field("email", "ragon-job@localhost"));
    let words: Vec<String> = record
        .get("command")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    let first_line: String = shlex_join(&words).chars().take(72).collect();
    let message = format!("job: {first_line}");
    let steps: [(&str, Vec<&str>); 3] = [
        ("checkout", vec!["checkout", "-q", "-b", &branch]),
        ("add", vec!["add", "-A"]),
        ("commit", vec!["-c", &name, "-c", &email, "commit", "-q", "-m", &message]),
    ];
    for (step, args) in steps {
        let out = run(&args);
        if out.code != 0 {
            record.set("note", Value::str(format!("git {step} failed: {}", tail(&out.stderr, 200))));
            return None;
        }
    }
    Some(branch)
}

/// What ended a command.
enum Ended {
    Exited(i32),
    /// The runner asked the job to stop; the text is its note.
    Stopped(String),
}

fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(-1)
}

/// Asks a command's processes to end, then kills them. On Unix the command leads its own process
/// group; on Windows its Job Object of its own was terminated before (see `run`).
fn end(child: &mut Child) {
    #[cfg(unix)]
    {
        let group = format!("-{}", child.id());
        let _ = Command::new("kill").args(["-TERM", "--", &group]).stderr(Stdio::null()).status();
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = Command::new("kill").args(["-KILL", "--", &group]).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run(argv: &[String], tree: &Path, log: &File, job_id: &str, stop_file: &Path) -> Ended {
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(tree)
        .stdin(Stdio::null())
        .env("RAGON_JOB", job_id)
        .env("GIT_TERMINAL_PROMPT", "0")
        // CPU-only: a job that reached the GPU would be outside its memory cap.
        .env("CUDA_VISIBLE_DEVICES", "")
        .env("NVIDIA_VISIBLE_DEVICES", "none")
        .env("HIP_VISIBLE_DEVICES", "");
    let streams = log.try_clone().and_then(|out| Ok((out, log.try_clone()?)));
    let Ok((out, err)) = streams else { return Ended::Exited(127) };
    command.stdout(out).stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Suspended until it is in its own Job Object (below).
        command.creation_flags(crate::windows_job::ENGINE_CREATION_FLAGS);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            let _ = writeln!(&*log, "could not start {}: {e}", argv[0]);
            return Ended::Exited(127);
        }
    };
    // On Windows the command and everything it starts (the program behind `cmd.exe` or `ragon
    // sandbox`) are in a Job Object of their own, nested in the runner's: a stop ends them all, and
    // what the command leaves running dies with the job when the step ends, so nothing holds the
    // tree that is removed afterwards.
    #[cfg(windows)]
    let step_job = match crate::windows_job::Job::new().and_then(|job| job.adopt_suspended(&child).map(|()| job)) {
        Ok(job) => Some(job),
        Err(e) => {
            let _ = writeln!(&*log, "--- note: the step has no Job Object of its own ({e}); a stop ends only its first process");
            let _ = crate::windows_job::resume_threads(child.id());
            None
        }
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ended::Exited(exit_code(status)),
            Ok(None) => {}
            Err(_) => return Ended::Exited(-1),
        }
        if let Some(note) = stop_requested(stop_file) {
            #[cfg(windows)]
            if let Some(job) = &step_job {
                let _ = job.terminate(1);
            }
            end(&mut child);
            return Ended::Stopped(note);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn stop_requested(stop_file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(stop_file).ok()?;
    let note = text.trim();
    Some(if note.is_empty() || note == "cancelled" { "the job was cancelled".into() } else { note.into() })
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn now() -> String {
    crate::core::utc_timestamp(unix_now())
}

/// Runs the job and returns its exit code (the test's when the command passed and the test failed;
/// 1 when it could not run or was stopped).
pub fn execute(store: &Store, id: &str, sandbox: &Sandbox) -> i32 {
    let Some(mut record) = store.read(id) else {
        eprintln!("no job {id}");
        return 1;
    };
    if !matches!(jobs::status_of(&record), "queued" | "running") {
        eprintln!("job {id} is {}, not queued", jobs::status_of(&record));
        return 1;
    }
    if let Err(reason) = jobs::recheck(&record) {
        let _ = store.update(id, |r| {
            r.set("status", Value::str("refused"));
            r.set("note", Value::str(reason.clone()));
            r.set("finished", Value::str(now()));
        });
        eprintln!("{reason}");
        return 1;
    }
    let Ok(directory) = store.job_dir(id) else { return 1 };
    let tree = directory.join(TREE_DIR);
    let stop_file = directory.join(JOB_STOP_FILE);
    let repo = match store.repo_path(record.get("repo").and_then(Value::as_str).unwrap_or_default()) {
        Ok(repo) => repo,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let started = Instant::now();
    let started_at = now();
    let mut changed = false;
    let _ = store.update(id, |r| {
        if matches!(jobs::status_of(r), "queued" | "running") {
            r.set("status", Value::str("running"));
            if r.get("started").is_none_or(Value::is_null) {
                r.set("started", Value::str(started_at.clone()));
            }
            r.set("sandbox", Value::str(sandbox.name()));
            changed = true;
        }
    });
    if !changed {
        eprintln!("job {id} was cancelled before it started");
        return 1;
    }
    record.set("sandbox", Value::str(sandbox.name()));
    let log = match OpenOptions::new().create(true).append(true).open(directory.join(OUTPUT_LOG)) {
        Ok(log) => log,
        Err(e) => {
            record.set("note", Value::str(format!("cannot open the output log: {e}")));
            finish(store, id, &mut record, 1, started);
            return 1;
        }
    };
    let code = steps(&mut record, &repo, &tree, &log, &stop_file, sandbox);
    remove_tree(&repo, &tree);
    finish(store, id, &mut record, code, started);
    code
}

fn steps(record: &mut Value, repo: &Path, tree: &Path, log: &File, stop_file: &Path, sandbox: &Sandbox) -> i32 {
    let id = jobs::id_of(record).to_string();
    let commit_sha = record.get("commit").and_then(Value::as_str).unwrap_or_default().to_string();
    let added = git(&["--git-dir", &path_str(repo), "worktree", "add", "--detach", &path_str(tree), &commit_sha]);
    let admin = if added.code == 0 { admin_dir(tree, repo) } else { None };
    let Some(admin) = admin else {
        let note = if added.code != 0 {
            format!("git worktree add failed: {}", tail(&added.stderr, 200))
        } else {
            "git worktree add left no administrative folder in the repository".to_string()
        };
        record.set("note", Value::str(note));
        return 1;
    };
    if sandbox == &Sandbox::None && cfg!(windows) {
        let _ = writeln!(&*log, "--- note: no sandbox beside the Job Object (the drill binary)");
    }
    let run_step = |label: &str, argv: Vec<String>| -> Result<i32, String> {
        if !label.is_empty() {
            let _ = writeln!(&*log, "\n--- {label}");
        }
        match run(&sandbox.wrap(tree, &argv), tree, log, &id, stop_file) {
            Ended::Exited(code) => Ok(code),
            Ended::Stopped(note) => Err(note),
        }
    };
    let script = |name: &str, command: &str| {
        shell(command, &stop_file.with_file_name(name)).map_err(|e| format!("cannot write the {name} script: {e}"))
    };
    if let Some(setup) = record.get("setup").and_then(Value::as_str).map(String::from) {
        let argv = match script("setup.cmd", &setup) {
            Ok(argv) => argv,
            Err(note) => {
                record.set("note", Value::str(note));
                return 1;
            }
        };
        match run_step(&format!("setup: {setup}"), argv) {
            Ok(0) => {}
            Ok(code) => {
                record.set("setup_exit_code", Value::num(code));
                record.set("note", Value::str(format!("the setup command failed (exit {code})")));
                return code;
            }
            Err(note) => return stopped(record, note),
        }
    }
    let command: Vec<String> = record
        .get("command")
        .and_then(Value::as_array)
        .map(|c| c.iter().filter_map(Value::as_str).map(String::from).collect())
        .unwrap_or_default();
    let mut code = match run_step("", command) {
        Ok(code) => code,
        Err(note) => return stopped(record, note),
    };
    record.set("exit_code", Value::num(code));
    if code == 0
        && let Some(test) = record.get("test").and_then(Value::as_str).map(String::from)
    {
        let argv = match script("test.cmd", &test) {
            Ok(argv) => argv,
            Err(note) => {
                record.set("note", Value::str(note));
                return 1;
            }
        };
        match run_step(&format!("test: {test}"), argv) {
            Ok(test_code) => {
                record.set("test_exit_code", Value::num(test_code));
                code = test_code;
            }
            Err(note) => return stopped(record, note),
        }
    }
    let branch = commit(record, tree, &admin);
    record.set("branch", Value::opt_str(branch));
    code
}

fn stopped(record: &mut Value, note: String) -> i32 {
    record.set("note", Value::str(note));
    record.set("stopped", Value::Bool(true));
    1
}

/// Writes the job's final state: `cancelled` stays (a cancel raced the end), otherwise done or
/// failed by the code.
fn finish(store: &Store, id: &str, record: &mut Value, code: i32, started: Instant) {
    let wall = started.elapsed().as_secs();
    let finished = now();
    let _ = store.update(id, |current| {
        let status = if jobs::status_of(current) == "cancelled" {
            "cancelled"
        } else if code == 0 {
            "done"
        } else {
            "failed"
        };
        let mut merged = record.clone();
        // What the runner or a cancel wrote meanwhile stays (the started time, a fetch, a note).
        for key in ["started", "fetched"] {
            if let Some(value) = current.get(key) {
                merged.set(key, value.clone());
            }
        }
        if merged.get("note").is_none_or(Value::is_null)
            && let Some(note) = current.get("note").filter(|n| !n.is_null())
        {
            merged.set("note", note.clone());
        }
        merged.set("status", Value::str(status));
        merged.set("finished", Value::str(finished.clone()));
        merged.set("wall_s", Value::num(wall as f64));
        *current = merged;
    });
}

/// Removes finished jobs whose keep is over: the folder, a worktree left behind, `refs/jobs/<id>`
/// and the branch `job/<id>` in the job repository. Returns the ids removed.
pub fn prune(store: &Store, now_unix: u64) -> Vec<String> {
    let mut removed = Vec::new();
    for record in store.records() {
        if !Store::expired(&record, now_unix) {
            continue;
        }
        let id = jobs::id_of(&record).to_string();
        let Ok(dir) = store.job_dir(&id) else { continue };
        if let Ok(repo) = store.repo_path(record.get("repo").and_then(Value::as_str).unwrap_or_default())
            && repo.is_dir()
        {
            let r = path_str(&repo);
            remove_tree(&repo, &dir.join(TREE_DIR));
            git(&["--git-dir", &r, "update-ref", "-d", &format!("refs/jobs/{id}")]);
            git(&["--git-dir", &r, "branch", "-D", &format!("job/{id}")]);
        }
        if std::fs::remove_dir_all(&dir).is_ok() {
            removed.push(id);
        }
    }
    removed
}

/// Appends the output from `offset` to `sink`; returns the new offset.
pub fn copy_output(store: &Store, id: &str, offset: u64, sink: &mut dyn Write) -> io::Result<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let path = store.job_dir(id).map_err(io::Error::other)?.join(OUTPUT_LOG);
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(offset),
        Err(e) => return Err(e),
    };
    file.seek(SeekFrom::Start(offset))?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    sink.write_all(&buffer)?;
    sink.flush()?;
    Ok(offset + buffer.len() as u64)
}

/// The exit code `logs --follow` ends with: 0 for done, otherwise the job's (the test's) code.
pub fn result_code(record: &Value) -> i32 {
    if jobs::status_of(record) == "done" {
        return 0;
    }
    let code = |key: &str| record.get(key).and_then(Value::as_f64).map(|c| c as i32);
    let chosen = if code("exit_code") == Some(0) { code("test_exit_code") } else { code("exit_code") };
    chosen.filter(|c| *c != 0).unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlex_join_quotes_like_python() {
        let words: Vec<String> = ["python", "-c", "print('hi')", "a b", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(shlex_join(&words), r#"python -c 'print('"'"'hi'"'"')' 'a b' ''"#);
    }

    #[test]
    fn verbatim_paths_become_plain_for_git() {
        assert_eq!(plain(PathBuf::from(r"\\?\C:\jobs\repos\d.git")), PathBuf::from(r"C:\jobs\repos\d.git"));
        assert_eq!(plain(PathBuf::from(r"\\?\UNC\host\share\d.git")), PathBuf::from(r"\\host\share\d.git"));
        assert_eq!(plain(PathBuf::from("/srv/jobs/d.git")), PathBuf::from("/srv/jobs/d.git"));
    }

    #[test]
    fn the_unelevated_sandbox_wraps_the_command() {
        let sandbox = Sandbox::Unelevated { ragon: PathBuf::from("ragon"), profile: ":workspace".into() };
        let argv = sandbox.wrap(Path::new("tree"), &["cmd".into(), "/c".into(), "dir".into()]);
        assert_eq!(
            argv,
            ["ragon", "-c", "windows.sandbox=\"unelevated\"", "sandbox", "-P", ":workspace", "-C", "tree", "--", "cmd", "/c", "dir"]
        );
        assert_eq!(Sandbox::None.wrap(Path::new("t"), &["x".into()]), ["x"]);
        assert_eq!(sandbox.to_args()[..2], ["--sandbox", "unelevated"]);
    }

    #[test]
    fn result_codes_follow_the_linux_node() {
        let rec = |status: &str, exit: Value, test: Value| {
            Value::Object(vec![
                ("status".into(), Value::str(status)),
                ("exit_code".into(), exit),
                ("test_exit_code".into(), test),
            ])
        };
        assert_eq!(result_code(&rec("done", Value::num(0), Value::Null)), 0);
        assert_eq!(result_code(&rec("failed", Value::num(3), Value::Null)), 3);
        assert_eq!(result_code(&rec("failed", Value::num(0), Value::num(5))), 5);
        assert_eq!(result_code(&rec("failed", Value::Null, Value::Null)), 1);
        assert_eq!(result_code(&rec("cancelled", Value::Null, Value::Null)), 1);
    }
}
