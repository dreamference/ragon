//! The two ways the night run starts a program: `git` through [`Git`], which knows only the verbs a
//! night run needs (none of them merges, pushes or fetches), and `ragon exec` or the tests through
//! [`run_capped`], under a memory cap and a wall-clock limit.
//!
//! The caps: on Linux a `systemd-run --user --scope` with `MemoryMax`, as the Python runner has it
//! (off in tests); on Windows a Job Object with `JOB_OBJECT_LIMIT_JOB_MEMORY` and
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, the child created suspended and resumed only inside the job,
//! so nothing it starts escapes the cap or outlives the run. The time limit is the wall clock on
//! both: the process group (Linux) or the job (Windows) is ended when it passes.

use std::cell::RefCell;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The git verbs a night run uses. There is no `push`, `merge`, `pull`, `fetch`, `rebase` or
/// `remote` here: a night run never brings anything into the user's branches or sends anything
/// anywhere, and a verb that is not in this list cannot be run through [`Git`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Add,
    Branch,
    CatFile,
    Commit,
    Diff,
    Reset,
    Clean,
    RevParse,
    Show,
    Status,
    Worktree,
}

impl Verb {
    pub fn name(self) -> &'static str {
        match self {
            Verb::Add => "add",
            Verb::Branch => "branch",
            Verb::CatFile => "cat-file",
            Verb::Commit => "commit",
            Verb::Diff => "diff",
            Verb::Reset => "reset",
            Verb::Clean => "clean",
            Verb::RevParse => "rev-parse",
            Verb::Show => "show",
            Verb::Status => "status",
            Verb::Worktree => "worktree",
        }
    }
}

/// How long one git call may take. A worktree add of a large repository is the slowest.
const GIT_TIMEOUT: Duration = Duration::from_secs(300);

/// Runs git with the hardening of `ragon-safegit` and the repository's hooks switched off, and
/// keeps every command line it ran (tests read it to prove what was not run).
pub struct Git {
    no_hooks: OsString,
    pub log: RefCell<Vec<Vec<String>>>,
}

impl Git {
    /// `hooks_off` is the `core.hooksPath` that holds no hooks: `/dev/null` on Unix, an empty folder
    /// on Windows (git there cannot use `NUL` as a folder).
    pub fn new(hooks_off: &Path) -> Self {
        Git { no_hooks: format!("core.hooksPath={}", hooks_off.display()).into(), log: RefCell::new(Vec::new()) }
    }

    /// `git -C cwd [-c identity…] <verb> args…`. Returns the exit code (-1 when git could not run)
    /// and stdout followed by stderr, as the Python runner's `_git` does.
    pub fn run(&self, cwd: &Path, verb: Verb, args: &[&str]) -> (i32, String) {
        self.run_as(cwd, &[], verb, args)
    }

    pub fn run_as(&self, cwd: &Path, config: &[String], verb: Verb, args: &[&str]) -> (i32, String) {
        let no_hooks = self.no_hooks.to_string_lossy().into_owned();
        let mut words: Vec<&str> = vec!["-c", &no_hooks];
        for pair in config {
            words.push("-c");
            words.push(pair);
        }
        words.push(verb.name());
        words.extend_from_slice(args);
        self.log.borrow_mut().push(words.iter().map(|w| (*w).to_string()).collect());
        match ragon_safegit::run(ragon_safegit::command(cwd, &words), GIT_TIMEOUT) {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                (output.status.code().unwrap_or(-1), text)
            }
            Err(error) => (-1, error.to_string()),
        }
    }

    /// The verbs every logged command ran, for the tests.
    pub fn verbs(&self) -> Vec<String> {
        self.log
            .borrow()
            .iter()
            .filter_map(|words| {
                let mut i = 0;
                while words.get(i).map(String::as_str) == Some("-c") {
                    i += 2;
                }
                words.get(i).cloned()
            })
            .collect()
    }
}

/// How a capped command ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    Code(i32),
    /// The limit passed or the run was stopped; the command was ended.
    TimedOut,
}

/// What [`run_capped`] needs besides the command.
pub struct Cap<'a> {
    /// `ragon-night-<task>-<n>`: the scope's name on Linux.
    pub unit: &'a str,
    /// `[night] task_memory` as written (`8G`).
    pub memory: &'a str,
    /// Wrap in a systemd scope (Linux only; the tests switch it off, as the Python tests do).
    pub use_scope: bool,
    pub limit: Instant,
    pub stop: &'a dyn Fn() -> bool,
}

/// How long a SIGTERM may take before SIGKILL, as `KILL_GRACE_S` in the Python runner.
#[cfg(unix)]
const KILL_GRACE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);

/// `[night] task_memory` in bytes: a number with an optional K, M, G or T (powers of 1024), as
/// systemd reads `MemoryMax`. `None` for anything else (`infinity`, a percentage), which leaves
/// the job without a memory cap.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn memory_bytes(text: &str) -> Option<u64> {
    let text = text.trim();
    let (number, unit) = match text.char_indices().last() {
        Some((i, c)) if c.is_ascii_alphabetic() => (&text[..i], c.to_ascii_uppercase()),
        _ => (text, 'B'),
    };
    let shift = match unit {
        'B' => 0,
        'K' => 10,
        'M' => 20,
        'G' => 30,
        'T' => 40,
        _ => return None,
    };
    let value: f64 = number.trim().parse().ok().filter(|v: &f64| v.is_finite() && *v > 0.0)?;
    Some((value * (1u64 << shift) as f64) as u64)
}

/// Runs `command` in `cwd` with `env` added, its output going to `output` (appended, or the file
/// started over), stdin closed, until it exits or `cap.limit` passes or `cap.stop()` says so.
pub fn run_capped(
    command: &[OsString],
    cwd: &Path,
    output: &Path,
    append: bool,
    env: &[(String, String)],
    cap: &Cap<'_>,
) -> io::Result<Exit> {
    let sink = OpenOptions::new().create(true).write(true).append(append).truncate(!append).open(output)?;
    let errors = sink.try_clone()?;
    let words = prefixed(command, cap);
    let mut cmd = Command::new(&words[0]);
    cmd.args(&words[1..]).current_dir(cwd).stdin(Stdio::null()).stdout(sink).stderr(errors);
    for (name, value) in env {
        cmd.env(name, value);
    }
    platform::run(cmd, cap)
}

fn prefixed(command: &[OsString], cap: &Cap<'_>) -> Vec<OsString> {
    if !(cfg!(target_os = "linux") && cap.use_scope) {
        return command.to_vec();
    }
    let mut words: Vec<OsString> = [
        "systemd-run",
        "--user",
        "--scope",
        "--quiet",
        &format!("--unit={}", cap.unit),
        "-p",
        &format!("MemoryMax={}", cap.memory),
        "-p",
        "MemorySwapMax=0",
        "-p",
        "CPUQuota=400%",
        "--",
        "choom",
        "-n",
        "500",
        "--",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    words.extend(command.iter().cloned());
    words
}

#[cfg(unix)]
mod platform {
    use super::*;
    use std::os::unix::process::{CommandExt, ExitStatusExt};

    pub fn run(mut cmd: Command, cap: &Cap<'_>) -> io::Result<Exit> {
        cmd.process_group(0);
        let mut child = cmd.spawn()?;
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(Exit::Code(status.code().unwrap_or_else(|| -status.signal().unwrap_or(1))));
            }
            if Instant::now() >= cap.limit || (cap.stop)() {
                let group = child.id() as i32;
                for (signal, name) in [(libc::SIGTERM, "SIGTERM"), (libc::SIGKILL, "SIGKILL")] {
                    if cfg!(target_os = "linux") && cap.use_scope {
                        let _ = Command::new("systemctl")
                            .args(["--user", "kill", &format!("--signal={name}"), &format!("{}.scope", cap.unit)])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                    // SAFETY: plain syscall on the group this function created.
                    unsafe { libc::killpg(group, signal) };
                    let waited = Instant::now();
                    while waited.elapsed() < KILL_GRACE {
                        if child.try_wait()?.is_some() {
                            return Ok(Exit::TimedOut);
                        }
                        std::thread::sleep(POLL);
                    }
                }
                let _ = child.wait();
                return Ok(Exit::TimedOut);
            }
            std::thread::sleep(POLL);
        }
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use ragon_supervisor::windows_job::{ENGINE_CREATION_FLAGS, Job};
    use std::os::windows::process::CommandExt;

    pub fn run(mut cmd: Command, cap: &Cap<'_>) -> io::Result<Exit> {
        let job = Job::with_memory_cap(memory_bytes(cap.memory))?;
        cmd.creation_flags(ENGINE_CREATION_FLAGS);
        let mut child = cmd.spawn()?;
        if let Err(error) = job.adopt_suspended(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let exit = loop {
            if let Some(status) = child.try_wait()? {
                break Exit::Code(status.code().unwrap_or(-1));
            }
            if Instant::now() >= cap.limit || (cap.stop)() {
                let _ = job.terminate(1);
                let _ = child.wait();
                break Exit::TimedOut;
            }
            std::thread::sleep(POLL);
        };
        // Dropping the job closes its handle, which ends whatever the command left running.
        drop(job);
        Ok(exit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_reads_like_systemd() {
        assert_eq!(memory_bytes("8G"), Some(8 << 30));
        assert_eq!(memory_bytes("512M"), Some(512 << 20));
        assert_eq!(memory_bytes("1.5g"), Some(3 << 29));
        assert_eq!(memory_bytes("4096"), Some(4096));
        assert_eq!(memory_bytes("infinity"), None);
        assert_eq!(memory_bytes("50%"), None);
    }

    #[test]
    fn the_scope_wraps_only_when_asked() {
        let stop = || false;
        let mut cap = Cap { unit: "ragon-night-x-1", memory: "8G", use_scope: false, limit: Instant::now(), stop: &stop };
        let command = vec![OsString::from("ragon"), OsString::from("exec")];
        assert_eq!(prefixed(&command, &cap), command);
        cap.use_scope = true;
        let words = prefixed(&command, &cap);
        if cfg!(target_os = "linux") {
            assert_eq!(words[0], "systemd-run");
            assert!(words.contains(&OsString::from("MemoryMax=8G")));
            assert_eq!(&words[words.len() - 2..], &command[..]);
        } else {
            assert_eq!(words, command);
        }
    }

    /// A directory removed when the test ends.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("night-run-capped-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Scratch(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_limit_ends_the_whole_group() {
        let dir = Scratch::new();
        let out = dir.path().join("out.log");
        let stop = || false;
        let cap = Cap {
            unit: "u",
            memory: "1G",
            use_scope: false,
            limit: Instant::now() + Duration::from_millis(300),
            stop: &stop,
        };
        let command: Vec<OsString> = ["sh", "-c", "sleep 30 & echo started; wait"].iter().map(OsString::from).collect();
        let began = Instant::now();
        assert_eq!(run_capped(&command, dir.path(), &out, false, &[], &cap).unwrap(), Exit::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(10));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "started\n");
    }

    #[cfg(unix)]
    #[test]
    fn output_is_appended_or_started_over_and_the_code_comes_back() {
        let dir = Scratch::new();
        let out = dir.path().join("out.log");
        let stop = || false;
        let cap = Cap { unit: "u", memory: "1G", use_scope: false, limit: Instant::now() + Duration::from_secs(30), stop: &stop };
        let say = |text: &str, code: i32| -> Vec<OsString> {
            vec!["sh".into(), "-c".into(), format!("echo {text}; echo $NIGHT_X >&2; exit {code}").into()]
        };
        let env = vec![("NIGHT_X".to_string(), "x".to_string())];
        assert_eq!(run_capped(&say("a", 0), dir.path(), &out, false, &env, &cap).unwrap(), Exit::Code(0));
        assert_eq!(run_capped(&say("b", 3), dir.path(), &out, true, &env, &cap).unwrap(), Exit::Code(3));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "a\nx\nb\nx\n");
        assert_eq!(run_capped(&say("c", 0), dir.path(), &out, false, &[], &cap).unwrap(), Exit::Code(0));
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "c\n\n");
    }

    /// `powershell -Command <script>`.
    #[cfg(windows)]
    fn powershell(script: &str) -> Vec<OsString> {
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", script].iter().map(OsString::from).collect()
    }

    /// A script that starts a second PowerShell writing `late` after four seconds, says `started`,
    /// then sleeps for `sleep` seconds.
    #[cfg(windows)]
    fn leaves_a_child(late: &Path, sleep: u32) -> Vec<OsString> {
        powershell(&format!(
            "Start-Process -NoNewWindow powershell -ArgumentList '-NoProfile','-Command','Start-Sleep 4; Set-Content -Path ''{}'' -Value x'; 'started'; Start-Sleep {sleep}",
            late.display()
        ))
    }

    #[cfg(windows)]
    #[test]
    fn the_limit_ends_the_whole_job() {
        let dir = Scratch::new();
        let (out, late) = (dir.path().join("out.log"), dir.path().join("late.txt"));
        let stop = || false;
        let cap = Cap { unit: "u", memory: "1G", use_scope: false, limit: Instant::now() + Duration::from_secs(2), stop: &stop };
        let began = Instant::now();
        assert_eq!(run_capped(&leaves_a_child(&late, 60), dir.path(), &out, false, &[], &cap).unwrap(), Exit::TimedOut);
        assert!(began.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_secs(6));
        assert!(!late.exists(), "the child outlived the limit");
    }

    #[cfg(windows)]
    #[test]
    fn closing_the_job_ends_what_the_command_left_running() {
        let dir = Scratch::new();
        let (out, late) = (dir.path().join("out.log"), dir.path().join("late.txt"));
        let stop = || false;
        let cap = Cap { unit: "u", memory: "1G", use_scope: false, limit: Instant::now() + Duration::from_secs(60), stop: &stop };
        assert_eq!(run_capped(&leaves_a_child(&late, 0), dir.path(), &out, false, &[], &cap).unwrap(), Exit::Code(0));
        assert!(std::fs::read_to_string(&out).unwrap().contains("started"));
        std::thread::sleep(Duration::from_secs(6));
        assert!(!late.exists(), "the child outlived the job");
    }

    #[cfg(windows)]
    #[test]
    fn an_allocation_past_the_memory_cap_fails() {
        let dir = Scratch::new();
        let out = dir.path().join("out.log");
        let stop = || false;
        let script = "$ErrorActionPreference = 'Stop'; 'started'; $b = New-Object byte[] 1500000000; 'allocated'";
        let run = |memory: &str| {
            let cap = Cap { unit: "u", memory, use_scope: false, limit: Instant::now() + Duration::from_secs(60), stop: &stop };
            let exit = run_capped(&powershell(script), dir.path(), &out, false, &[], &cap).unwrap();
            (exit, std::fs::read_to_string(&out).unwrap())
        };
        let (exit, text) = run("8G");
        assert_eq!(exit, Exit::Code(0), "{text}");
        assert!(text.contains("allocated"), "{text}");
        let (exit, text) = run("400M");
        assert_ne!(exit, Exit::Code(0), "{text}");
        assert!(text.contains("started") && !text.contains("allocated"), "{text}");
    }
}
