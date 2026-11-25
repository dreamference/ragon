//! The node's job runner: one thread of the supervisor (`server run --jobs-dir D`, so it lives as
//! long as the service or the detached `server start`, never as long as an SSH session) or its own
//! process (`server job runner`). It starts queued jobs one per lane, each as `server job exec <id>`
//! in a Job Object of its own (the memory cap, a process cap, kill on close), watches the time
//! limit, the memory cap and cancels ([`Watch`]: the stop file first, the kill after the grace)
//! and records what the job process could not.
//!
//! The policy is platform-neutral and tested on Linux with the Unix host (a process tree, its RSS
//! read from /proc); [`windows`] is the Job Object host.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::job_exec::{self, Sandbox};
use crate::jobs::{self, Action, JOB_STOP_FILE, Reason, STOP_GRACE, Store, TREE_DIR, Watch};
use crate::json::Value;

/// The runner's log of a job's own process (its stderr: what `exec` could not put in the record).
pub const EXEC_LOG: &str = "exec.log";

/// A started `exec` with everything it starts.
pub trait RunningJob: Send {
    fn try_wait(&mut self) -> io::Result<Option<i32>>;
    /// True once the job passed its memory cap (it stays true).
    fn memory_tripped(&mut self) -> bool;
    /// Kills every process of the job.
    fn kill(&mut self) -> io::Result<()>;
    /// The most memory the job used, in bytes, where the host knows.
    fn peak_memory(&mut self) -> Option<u64>;
}

pub trait Spawner: Send {
    fn spawn(&self, argv: &[String], memory_bytes: u64, log: &Path) -> io::Result<Box<dyn RunningJob>>;
}

/// The host of this platform: Job Objects on Windows, process trees elsewhere.
pub struct ProcessSpawner;

impl Spawner for ProcessSpawner {
    fn spawn(&self, argv: &[String], memory_bytes: u64, log: &Path) -> io::Result<Box<dyn RunningJob>> {
        #[cfg(windows)]
        {
            windows::spawn(argv, memory_bytes, log)
        }
        #[cfg(unix)]
        {
            unix::spawn(argv, memory_bytes, log)
        }
    }
}

struct Active {
    id: String,
    lane: String,
    started: Instant,
    watch: Watch,
    job: Box<dyn RunningJob>,
}

pub struct Runner<S: Spawner> {
    pub store: Store,
    spawner: S,
    /// The argv up to `exec` (`[ragon, "server", "job", "exec"]`).
    exec_prefix: Vec<String>,
    sandbox: Sandbox,
    pub grace: Duration,
    active: Vec<Active>,
    last_prune: Option<Instant>,
}

const PRUNE_EVERY: Duration = Duration::from_secs(3600);

impl<S: Spawner> Runner<S> {
    pub fn new(store: Store, spawner: S, exec_prefix: Vec<String>, sandbox: Sandbox) -> Self {
        Runner { store, spawner, exec_prefix, sandbox, grace: STOP_GRACE, active: Vec::new(), last_prune: None }
    }

    /// Settles the jobs a previous runner left `running`; call once before the first tick.
    pub fn start(&mut self) -> Vec<String> {
        self.store.settle_orphans(&job_exec::now())
    }

    pub fn active_ids(&self) -> Vec<String> {
        self.active.iter().map(|a| a.id.clone()).collect()
    }

    /// One pass: finish what ended, apply the limits, start what may start. Returns how long
    /// until the next pass is needed (at most a second: new submissions are found by polling).
    pub fn tick(&mut self) -> Duration {
        let mut next = Duration::from_secs(1);
        let mut index = 0;
        while index < self.active.len() {
            let ended = self.active[index].job.try_wait().unwrap_or(Some(-1));
            if let Some(code) = ended {
                let mut done = self.active.swap_remove(index);
                self.finalize(&mut done, code);
                continue;
            }
            let active = &mut self.active[index];
            let elapsed = active.started.elapsed();
            let tripped = active.job.memory_tripped();
            let cancelled = self.store.read(&active.id).is_none_or(|r| jobs::status_of(&r) == "cancelled");
            match active.watch.tick(elapsed, tripped, cancelled) {
                Action::Run => {}
                Action::SoftStop(reason) => {
                    let note = self.store.read(&active.id).map(|r| reason.note(&r)).unwrap_or_default();
                    if let Ok(dir) = self.store.job_dir(&active.id) {
                        let _ = std::fs::write(dir.join(JOB_STOP_FILE), format!("{note}\n"));
                    }
                }
                Action::Kill(_) => {
                    let _ = active.job.kill();
                }
            }
            next = next.min(active.watch.due_in(active.started.elapsed()).max(Duration::from_millis(100)));
            index += 1;
        }
        self.start_queued();
        if self.last_prune.is_none_or(|at| at.elapsed() >= PRUNE_EVERY) {
            self.last_prune = Some(Instant::now());
            job_exec::prune(&self.store, job_exec::unix_now());
        }
        if !self.active.is_empty() {
            next = next.min(Duration::from_millis(250));
        }
        next
    }

    /// Runs until `stop` is set; then kills what runs and records it as stopped by the node.
    pub fn run(&mut self, stop: &AtomicBool) {
        self.start();
        while !stop.load(Ordering::SeqCst) {
            let due = Instant::now() + self.tick();
            while Instant::now() < due && !stop.load(Ordering::SeqCst) {
                std::thread::sleep(due.saturating_duration_since(Instant::now()).min(Duration::from_millis(100)));
            }
        }
        self.shutdown();
    }

    /// Kills every running job and records it failed (the service is stopping).
    pub fn shutdown(&mut self) {
        for mut active in std::mem::take(&mut self.active) {
            let _ = active.job.kill();
            let code = active.job.try_wait().ok().flatten().unwrap_or(-1);
            let _ = self.store.update(&active.id, |r| {
                if jobs::status_of(r) == "running" {
                    r.set("note", Value::str("the job was stopped without a result: the node's service stopped"));
                }
            });
            self.finalize(&mut active, code);
        }
    }

    fn finalize(&mut self, done: &mut Active, code: i32) {
        let peak = done.job.peak_memory();
        let reason = done.watch.reason().or_else(|| done.job.memory_tripped().then_some(Reason::MemoryCap));
        let wall = done.started.elapsed().as_secs();
        let now = job_exec::now();
        let mut repo_slug = None;
        let _ = self.store.update(&done.id, |r| {
            repo_slug = r.get("repo").and_then(Value::as_str).map(String::from);
            if let Some(peak) = peak {
                r.set("peak_memory_bytes", Value::num(peak as f64));
            }
            if let Some(reason) = reason {
                r.set("stopped", Value::Bool(true));
                r.set("stop_reason", Value::str(reason.as_str()));
                // A cancel's note is the cancel; a limit's note replaces what the job wrote.
                if reason != Reason::Cancelled || r.get("note").is_none_or(Value::is_null) {
                    r.set("note", Value::str(reason.note(r)));
                }
            }
            if jobs::status_of(r) == "running" {
                // The job's process did not record an end: killed, or it crashed.
                let status = if reason == Some(Reason::Cancelled) { "cancelled" } else { "failed" };
                r.set("status", Value::str(status));
                r.set("finished", Value::str(now.clone()));
                r.set("wall_s", Value::num(wall as f64));
                if r.get("note").is_none_or(Value::is_null) {
                    r.set("note", Value::str(format!("the job's process ended without a result (exit {code})")));
                }
            } else if reason.is_some() && jobs::status_of(r) == "done" {
                r.set("status", Value::str("failed"));
            }
        });
        // A killed job leaves its worktree; remove it as exec would have.
        if let (Ok(dir), Some(slug)) = (self.store.job_dir(&done.id), repo_slug) {
            let tree = dir.join(TREE_DIR);
            if tree.exists()
                && let Ok(repo) = self.store.repo_path(&slug)
            {
                job_exec::remove_tree(&repo, &tree);
            }
            let _ = std::fs::remove_file(dir.join(JOB_STOP_FILE));
        }
    }

    fn start_queued(&mut self) {
        let records = self.store.records();
        let busy: Vec<String> = self.active.iter().map(|a| a.lane.clone()).collect();
        let starting: Vec<Value> = jobs::next_to_start(&records, &busy).into_iter().cloned().collect();
        for record in starting {
            let id = jobs::id_of(&record).to_string();
            let lane = jobs::lane_of(&record).to_string();
            if let Err(reason) = jobs::recheck(&record) {
                let _ = self.store.update(&id, |r| {
                    r.set("status", Value::str("refused"));
                    r.set("note", Value::str(reason.clone()));
                    r.set("finished", Value::str(job_exec::now()));
                });
                continue;
            }
            let mut claimed = false;
            let now = job_exec::now();
            let _ = self.store.update(&id, |r| {
                if jobs::status_of(r) == "queued" {
                    r.set("status", Value::str("running"));
                    r.set("started", Value::str(now.clone()));
                    claimed = true;
                }
            });
            if !claimed {
                continue;
            }
            let memory = record.get("memory_bytes").and_then(Value::as_u64).unwrap_or(0);
            let limit = record.get("time_s").and_then(Value::as_u64).unwrap_or(jobs::DEFAULT_TIME_S);
            let limit = Duration::from_secs(limit);
            let Ok(dir) = self.store.job_dir(&id) else { continue };
            let _ = std::fs::remove_file(dir.join(JOB_STOP_FILE));
            let argv = self.exec_argv(&id);
            match self.spawner.spawn(&argv, memory, &dir.join(EXEC_LOG)) {
                Ok(job) => self.active.push(Active {
                    id,
                    lane,
                    started: Instant::now(),
                    watch: Watch::new(limit, self.grace),
                    job,
                }),
                Err(e) => {
                    let _ = self.store.update(&id, |r| {
                        r.set("status", Value::str("failed"));
                        r.set("note", Value::str(format!("the node could not start the job: {e}")));
                        r.set("finished", Value::str(job_exec::now()));
                    });
                }
            }
        }
    }

    pub fn exec_argv(&self, id: &str) -> Vec<String> {
        let mut argv = self.exec_prefix.clone();
        argv.extend(["--jobs-dir".to_string(), self.store.root.display().to_string()]);
        argv.extend(self.sandbox.to_args());
        argv.push(id.to_string());
        argv
    }
}

/// The argv prefix that runs `server job exec` with this program (the launcher or the drill binary).
pub fn exec_prefix(exe: PathBuf) -> Vec<String> {
    vec![exe.display().to_string(), "server".into(), "job".into(), "exec".into()]
}

#[cfg(unix)]
pub mod unix {
    //! The Unix host: `exec` leads a process group; its memory is the RSS summed over every
    //! descendant (each command in a job leads a group of its own, so the tree, not the group,
    //! is the job). The Mac node needs its own memory reading (no /proc there).

    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    pub struct UnixJob {
        child: Child,
        cap: u64,
        peak: u64,
        tripped: bool,
        exited: Option<i32>,
    }

    pub fn spawn(argv: &[String], memory_bytes: u64, log: &Path) -> io::Result<Box<dyn RunningJob>> {
        let file = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(file)
            .process_group(0)
            .spawn()?;
        Ok(Box::new(UnixJob { child, cap: memory_bytes, peak: 0, tripped: false, exited: None }))
    }

    /// (pid, parent pid, resident bytes) of every process.
    fn processes() -> Vec<(u32, u32, u64)> {
        let mut all = Vec::new();
        let Ok(entries) = std::fs::read_dir("/proc") else { return all };
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
            // The fields after the command name, which ends at the last ')'.
            let Some(rest) = stat.rfind(')').map(|i| &stat[i + 2..]) else { continue };
            let fields: Vec<&str> = rest.split_whitespace().collect();
            let ppid = fields.get(1).and_then(|f| f.parse().ok()).unwrap_or(0);
            let rss_pages: u64 = fields.get(21).and_then(|f| f.parse().ok()).unwrap_or(0);
            all.push((pid, ppid, rss_pages * 4096));
        }
        all
    }

    /// The process and every descendant.
    pub fn tree(root: u32) -> Vec<(u32, u64)> {
        let all = processes();
        let mut found = vec![root];
        let mut i = 0;
        while i < found.len() {
            let parent = found[i];
            found.extend(all.iter().filter(|(_, ppid, _)| *ppid == parent).map(|(pid, _, _)| *pid));
            i += 1;
        }
        found.iter().map(|pid| (*pid, all.iter().find(|p| p.0 == *pid).map_or(0, |p| p.2))).collect()
    }

    fn signal(pid: u32, sig: &str) {
        let _ = Command::new("kill").args([sig, &pid.to_string()]).stderr(Stdio::null()).status();
    }

    impl RunningJob for UnixJob {
        fn try_wait(&mut self) -> io::Result<Option<i32>> {
            if let Some(code) = self.exited {
                return Ok(Some(code));
            }
            let status = self.child.try_wait()?;
            self.exited = status.map(|s| s.code().unwrap_or(-1));
            if self.exited.is_some() {
                // Whatever the job left behind (a daemon) goes with it, as with KILL_ON_JOB_CLOSE.
                let _ = Command::new("kill").args(["-KILL", "--", &format!("-{}", self.child.id())]).stderr(Stdio::null()).status();
            }
            Ok(self.exited)
        }

        fn memory_tripped(&mut self) -> bool {
            if !self.tripped && self.exited.is_none() {
                let used: u64 = tree(self.child.id()).iter().map(|p| p.1).sum();
                self.peak = self.peak.max(used);
                self.tripped = self.cap > 0 && used > self.cap;
            }
            self.tripped
        }

        fn kill(&mut self) -> io::Result<()> {
            for _ in 0..3 {
                let pids = tree(self.child.id());
                for (pid, _) in pids.iter().rev() {
                    signal(*pid, "-KILL");
                }
                if pids.len() <= 1 {
                    break;
                }
            }
            let _ = self.child.kill();
            let status = self.child.wait()?;
            self.exited = Some(status.code().unwrap_or(-1));
            Ok(())
        }

        fn peak_memory(&mut self) -> Option<u64> {
            (self.peak > 0).then_some(self.peak)
        }
    }
}

#[cfg(windows)]
pub mod windows {
    //! The Windows host: `exec` and everything it starts run in a Job Object with the job's memory
    //! cap and a process cap, created suspended and resumed only once inside, killed whole.

    use super::*;
    use crate::windows_job::{ENGINE_CREATION_FLAGS, JobLimits, LimitedJob};
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    pub struct WindowsJob {
        child: Child,
        job: LimitedJob,
        exited: Option<i32>,
    }

    pub fn spawn(argv: &[String], memory_bytes: u64, log: &Path) -> io::Result<Box<dyn RunningJob>> {
        let job = LimitedJob::new(JobLimits { memory_bytes, max_processes: jobs::MAX_PROCESSES })?;
        let file = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(file.try_clone()?)
            .stderr(file)
            .creation_flags(ENGINE_CREATION_FLAGS)
            .spawn()?;
        if let Err(e) = job.adopt_suspended(&child) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok(Box::new(WindowsJob { child, job, exited: None }))
    }

    impl RunningJob for WindowsJob {
        fn try_wait(&mut self) -> io::Result<Option<i32>> {
            if self.exited.is_none() {
                self.exited = self.child.try_wait()?.map(|s| s.code().unwrap_or(-1));
                if self.exited.is_some() {
                    // What the job left running goes with it.
                    let _ = self.job.terminate(1);
                }
            }
            Ok(self.exited)
        }

        fn memory_tripped(&mut self) -> bool {
            self.job.memory_tripped()
        }

        fn kill(&mut self) -> io::Result<()> {
            let killed = self.job.terminate(1).or_else(|_| self.child.kill());
            let status = self.child.wait()?;
            self.exited = Some(status.code().unwrap_or(-1));
            killed
        }

        fn peak_memory(&mut self) -> Option<u64> {
            self.job.peak_memory()
        }
    }
}

#[cfg(not(any(unix, windows)))]
compile_error!("the job runner needs a Unix or Windows host");

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// A job that a test drives by hand.
    #[derive(Default)]
    struct Fake {
        exit: Option<i32>,
        tripped: bool,
        killed: bool,
    }

    struct FakeJob(Arc<Mutex<Fake>>);

    impl RunningJob for FakeJob {
        fn try_wait(&mut self) -> io::Result<Option<i32>> {
            Ok(self.0.lock().unwrap().exit)
        }
        fn memory_tripped(&mut self) -> bool {
            self.0.lock().unwrap().tripped
        }
        fn kill(&mut self) -> io::Result<()> {
            let mut f = self.0.lock().unwrap();
            f.killed = true;
            f.exit = Some(1);
            Ok(())
        }
        fn peak_memory(&mut self) -> Option<u64> {
            Some(42)
        }
    }

    type Spawned = Vec<(String, Arc<Mutex<Fake>>)>;

    #[derive(Clone, Default)]
    struct FakeSpawner(Arc<Mutex<Spawned>>);

    impl Spawner for FakeSpawner {
        fn spawn(&self, argv: &[String], _: u64, _: &Path) -> io::Result<Box<dyn RunningJob>> {
            let fake = Arc::new(Mutex::new(Fake::default()));
            self.0.lock().unwrap().push((argv.last().unwrap().clone(), fake.clone()));
            Ok(Box::new(FakeJob(fake)))
        }
    }

    fn temp_store(name: &str) -> Store {
        let root = std::env::temp_dir().join(format!("ragon-runner-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Store::new(root)
    }

    fn submit(store: &Store, lane: &str, submitted: &str, time: &str) -> String {
        let n = store.ids().len();
        let request = crate::json::parse(&format!(
            r#"{{"id": "20261125-1000-{n:03x}", "repo": "demo", "commit": "{}", "command": ["true"], "memory": "1G", "time": "{time}", "lane": "{lane}"}}"#,
            "a".repeat(40)
        ))
        .unwrap();
        let record = store.submit(&request, &jobs::NodePolicy::default(), submitted).unwrap();
        jobs::id_of(&record).to_string()
    }

    fn runner(store: Store, spawner: FakeSpawner) -> Runner<FakeSpawner> {
        let mut r = Runner::new(store, spawner, vec!["ragon".into(), "server".into(), "job".into(), "exec".into()], Sandbox::None);
        r.grace = Duration::from_millis(0);
        r
    }

    #[test]
    fn lanes_queue_one_job_each() {
        let store = temp_store("lanes");
        let a1 = submit(&store, "a", "2026-11-25T10:00:00Z", "1h");
        let a2 = submit(&store, "a", "2026-11-25T10:00:01Z", "1h");
        let b1 = submit(&store, "b", "2026-11-25T10:00:02Z", "1h");
        let spawner = FakeSpawner::default();
        let mut r = runner(Store::new(store.root.clone()), spawner.clone());
        r.tick();
        let mut started = r.active_ids();
        started.sort();
        let mut want = vec![a1.clone(), b1.clone()];
        want.sort();
        assert_eq!(started, want);
        assert_eq!(jobs::status_of(&store.read(&a2).unwrap()), "queued");
        // a1 ends: its process recorded done; a2 starts in lane a.
        store.update(&a1, |r| r.set("status", Value::str("done"))).unwrap();
        spawner.0.lock().unwrap()[0].1.lock().unwrap().exit = Some(0);
        r.tick();
        assert!(r.active_ids().contains(&a2));
        assert_eq!(jobs::status_of(&store.read(&a1).unwrap()), "done");
        let argv = r.exec_argv(&a2);
        assert_eq!(argv[..6], ["ragon", "server", "job", "exec", "--jobs-dir", &store.root.display().to_string()]);
        let _ = std::fs::remove_dir_all(&store.root);
    }

    #[test]
    fn the_memory_cap_and_a_cancel_end_a_job() {
        let store = temp_store("cap");
        let id = submit(&store, "default", "2026-11-25T10:00:00Z", "1h");
        let spawner = FakeSpawner::default();
        let mut r = runner(Store::new(store.root.clone()), spawner.clone());
        r.tick();
        spawner.0.lock().unwrap()[0].1.lock().unwrap().tripped = true;
        r.tick(); // grace 0: killed at once
        assert!(spawner.0.lock().unwrap()[0].1.lock().unwrap().killed);
        r.tick();
        let rec = store.read(&id).unwrap();
        assert_eq!(jobs::status_of(&rec), "failed");
        assert_eq!(rec.get("stop_reason").and_then(Value::as_str), Some("memory cap"));
        assert_eq!(rec.get("peak_memory_bytes").and_then(Value::as_u64), Some(42));
        assert!(rec.get("note").and_then(Value::as_str).unwrap().contains("memory cap (1G)"));

        let id2 = submit(&store, "default", "2026-11-25T10:00:05Z", "1h");
        r.tick();
        assert_eq!(r.active_ids(), vec![id2.clone()]);
        assert!(store.cancel(&id2, "2026-11-25T10:01:00Z").unwrap());
        r.tick();
        r.tick();
        let rec = store.read(&id2).unwrap();
        assert_eq!(jobs::status_of(&rec), "cancelled");
        assert_eq!(rec.get("note").and_then(Value::as_str), Some("the job was cancelled"));
        let _ = std::fs::remove_dir_all(&store.root);
    }

    #[test]
    fn orphans_are_settled_and_a_soft_stop_writes_the_stop_file() {
        let store = temp_store("orphans");
        let id = submit(&store, "default", "2026-11-25T10:00:00Z", "1s");
        store.update(&id, |r| r.set("status", Value::str("running"))).unwrap();
        let spawner = FakeSpawner::default();
        let mut r = runner(Store::new(store.root.clone()), spawner.clone());
        assert_eq!(r.start(), vec![id.clone()]);
        assert_eq!(jobs::status_of(&store.read(&id).unwrap()), "failed");

        let id2 = submit(&store, "default", "2026-11-25T10:00:01Z", "1s");
        r.grace = Duration::from_secs(60);
        r.tick();
        std::thread::sleep(Duration::from_millis(1100));
        r.tick();
        let stop = std::fs::read_to_string(store.job_dir(&id2).unwrap().join(JOB_STOP_FILE)).unwrap();
        assert!(stop.contains("time limit (1s)"), "{stop}");
        assert!(!spawner.0.lock().unwrap()[0].1.lock().unwrap().killed);
        let _ = std::fs::remove_dir_all(&store.root);
    }
}
