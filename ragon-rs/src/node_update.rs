//! `ragon update` on a Windows node (specs/DREAMFERENCE_WINDOWS_NODE.md section 6, "Update").
//!
//! `install.ps1 -Role node` places the node under `%ProgramFiles%\Ragon`:
//! `current\bin` (the service's `ragon.exe` and its commands), `current\engine` (the unpacked
//! engine zip), and writes `node-install`, a record of the tag, the service and its state folder.
//! An update stages the new tree in `staging`, then, with no job in flight, stops the service,
//! renames `current` to `previous` and `staging` to `current` (the path the service and the
//! program-scoped firewall rule name never changes), starts the service and waits for it to be
//! healthy; when it is not, the old tree is put back and started. `previous` stays as the
//! rollback until the next update.
//!
//! The orchestration is generic over [`NodeHost`], so its tests run against a temp prefix and a
//! host that records; [`CommandHost`] is the real one and drives the node's own `ragon server`.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

/// The record `install.ps1` writes beside `current`.
pub const RECORD: &str = "node-install";

/// What the record says about an installed node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeInstall {
    /// `%ProgramFiles%\Ragon`.
    pub root: PathBuf,
    pub tag: String,
    pub service: String,
    pub port: u16,
    pub state_dir: PathBuf,
    /// The job queue the service runs (`jobs=`, written when the service was installed with
    /// `--jobs-dir`); `None` for one installed without node jobs.
    pub jobs_dir: Option<PathBuf>,
}

impl NodeInstall {
    pub fn current(&self) -> PathBuf {
        self.root.join("current")
    }
    pub fn previous(&self) -> PathBuf {
        self.root.join("previous")
    }
    pub fn staging(&self) -> PathBuf {
        self.root.join("staging")
    }
    pub fn ragon(&self) -> PathBuf {
        self.current().join("bin").join("ragon.exe")
    }
}

/// Reads the record's `key=value` lines. `None` when a field the update needs is missing.
pub fn parse_record(text: &str, root: &Path) -> Option<NodeInstall> {
    let field = |key: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
            .map(|value| value.trim().to_string())
    };
    Some(NodeInstall {
        root: root.to_path_buf(),
        tag: field("tag")?,
        service: field("service")?,
        port: field("port")?.parse().ok()?,
        state_dir: PathBuf::from(field("state")?),
        jobs_dir: field("jobs").filter(|v| !v.is_empty()).map(PathBuf::from),
    })
}

/// A path in the form two spellings of the same file share: resolved (junctions and links
/// followed, when it exists), without Windows' `\\?\` prefix, and case-folded on Windows.
fn comparable(path: &Path) -> PathBuf {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    PathBuf::from(if cfg!(windows) { text.to_lowercase() } else { text.to_string() })
}

/// Refuses when `exe` is one of the node's own files (under `%ProgramFiles%\Ragon`, reached
/// directly or through the `C:\Ragon\bin` junction on the machine PATH). Replacing those from
/// the running service's tree would put a new client beside the old engine with no job check,
/// stop or rollback, so `ragon update` stops here, before anything is downloaded.
pub fn refuse_node_own(exe: &Path, install: &NodeInstall) -> anyhow::Result<()> {
    if comparable(exe).starts_with(comparable(&install.root)) {
        let client = std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join(r"Programs\Ragon\bin\ragon.exe").display().to_string())
            .unwrap_or_else(|| r"%LOCALAPPDATA%\Programs\Ragon\bin\ragon.exe".to_string());
        bail!(
            "this ragon.exe ({}) is the node's own copy (C:\\Ragon\\bin is on the machine PATH, ahead of \
             yours); nothing was changed. Update with the client install, from an elevated shell: \
             & \"{client}\" update",
            exe.display()
        );
    }
    Ok(())
}

/// The installed node, if this machine has one: `RAGON_NODE_PROGRAM_FILES` (a test's prefix) or
/// `%ProgramFiles%`, then `Ragon\node-install`.
pub fn installed() -> Option<NodeInstall> {
    let base = std::env::var_os("RAGON_NODE_PROGRAM_FILES")
        .filter(|v| !v.is_empty())
        .or_else(|| if cfg!(windows) { std::env::var_os("ProgramFiles") } else { None })?;
    let root = PathBuf::from(base).join("Ragon");
    parse_record(&std::fs::read_to_string(root.join(RECORD)).ok()?, &root)
}

/// Rewrites the record's tag line.
pub fn write_tag(install: &NodeInstall, tag: &str) -> anyhow::Result<()> {
    let path = install.root.join(RECORD);
    let text = std::fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
    let lines: Vec<String> = text
        .lines()
        .map(|line| if line.starts_with("tag=") { format!("tag={tag}") } else { line.to_string() })
        .collect();
    std::fs::write(&path, lines.join("\r\n") + "\r\n").with_context(|| format!("could not write {}", path.display()))
}

/// What an update needs of the machine.
pub trait NodeHost {
    /// Jobs running or queued, from `ragon server job list`.
    fn jobs_in_flight(&self) -> anyhow::Result<usize>;
    fn stop(&self) -> anyhow::Result<()>;
    /// Starts the service from `current` and returns once it is healthy; an error when it is not
    /// within the limit.
    fn start_healthy(&self) -> anyhow::Result<()>;
    /// Renames a tree (a hook so tests can make one fail).
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
}

/// The message for a failed `server job list`: a service from before node jobs does not know
/// the verb, and the update cannot tell whether a job runs on it.
pub fn job_list_error(stderr: &str) -> String {
    if stderr.contains("unknown server command") {
        "this node's service was installed from a release without node jobs (`ragon server job list` \
         is unknown to it), so the update cannot tell whether a job is running and will not move it on. \
         Reinstall the node, from an elevated shell: install.ps1 -Role node -Uninstall, then \
         install.ps1 -Role node (weights and the node's data are kept)"
            .to_string()
    } else {
        format!("ragon server job list failed: {}", stderr.trim())
    }
}

/// Counts the records of `server job list`'s JSON array that are running or queued.
pub fn count_in_flight(listing: &str) -> anyhow::Result<usize> {
    let value: serde_json::Value = serde_json::from_str(listing.trim()).context("the job list is not JSON")?;
    let records = value.as_array().context("the job list is not an array")?;
    Ok(records
        .iter()
        .filter(|record| matches!(record["status"].as_str(), Some("running" | "queued")))
        .count())
}

/// True when every entry of a zip's listing (`tar -tf`) stays inside the folder it is unpacked to.
pub fn entries_safe(listing: &str) -> bool {
    listing.lines().map(str::trim).filter(|line| !line.is_empty()).all(|entry| {
        let entry = entry.replace('\\', "/");
        !entry.starts_with('/')
            && !entry.contains(':')
            && !entry.split('/').any(|part| part == "..")
    })
}

/// How an update ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Updated,
    /// The new tree did not come up healthy; the old one runs again (`restarted`: it came up).
    RolledBack { reason: String, restarted: bool },
}

/// Swaps the staged tree in. `staging` must hold `bin\ragon.exe` and `engine\ragon-serve.exe`.
/// Stops when a job is running or queued on the node. `ragon update` asks before it downloads
/// anything (`refuse_busy_node`), and `swap` asks again right before it stops the service, since a
/// job can arrive while the release downloads.
pub fn refuse_busy(host: &dyn NodeHost) -> anyhow::Result<()> {
    let running = host.jobs_in_flight().context("could not tell whether a job is running; nothing was changed")?;
    if running > 0 {
        bail!("{running} job(s) running or queued on this node; update when they finish (ragon server job list). Nothing was changed");
    }
    Ok(())
}

/// `refuse_busy` against the installed node's own `ragon server job list`.
pub fn refuse_busy_node(install: &NodeInstall) -> anyhow::Result<()> {
    refuse_busy(&CommandHost { install, wait_s: wait_seconds() })
}

pub fn swap(install: &NodeInstall, host: &dyn NodeHost, new_tag: &str) -> anyhow::Result<Outcome> {
    let staging = install.staging();
    for needed in ["bin/ragon.exe", "engine/ragon-serve.exe"] {
        if !staging.join(needed).is_file() {
            bail!("the staged update has no {needed}; nothing was changed");
        }
    }
    refuse_busy(host)?;
    host.stop().context("could not stop the service; nothing was changed")?;
    let (current, previous) = (install.current(), install.previous());
    let restart = |why: String| -> anyhow::Error {
        match host.start_healthy() {
            Ok(()) => anyhow::anyhow!("{why}; the old version was started again"),
            Err(e) => anyhow::anyhow!("{why}; and the old version did not start: {e}"),
        }
    };
    if previous.exists() {
        if let Err(e) = std::fs::remove_dir_all(&previous) {
            return Err(restart(format!("could not remove the older rollback folder: {e}")));
        }
    }
    if let Err(e) = host.rename(&current, &previous) {
        return Err(restart(format!("could not move the current version aside: {e}")));
    }
    if let Err(e) = host.rename(&staging, &current) {
        let _ = host.rename(&previous, &current);
        return Err(restart(format!("could not put the new version in place: {e}")));
    }
    match host.start_healthy() {
        Ok(()) => {
            write_tag(install, new_tag)?;
            Ok(Outcome::Updated)
        }
        Err(error) => {
            let _ = host.stop();
            let failed = install.root.join("failed");
            let _ = std::fs::remove_dir_all(&failed);
            // Whatever happens below, the service is started again from a `current` folder and
            // the error says which version that is.
            let started = |running: &str| match host.start_healthy() {
                Ok(()) => format!("{running} is running"),
                Err(e) => format!("{running} is in place but did not start ({e}): run `ragon server status`"),
            };
            if let Err(e) = host.rename(&current, &failed) {
                bail!(
                    "the new version {new_tag} was not healthy ({error}) and could not be moved away ({e}); {}; \
                     the old version {} is kept in {}",
                    started(new_tag),
                    install.tag,
                    previous.display()
                );
            }
            if let Err(e) = host.rename(&previous, &current) {
                let running = match host.rename(&failed, &current) {
                    Ok(()) => started(new_tag),
                    Err(undo) => format!("there is no current version ({undo}): the service cannot start"),
                };
                bail!(
                    "the new version {new_tag} was not healthy ({error}) and the old version {} could not be \
                     put back ({e}); {running}. The old version is in {}",
                    install.tag,
                    previous.display()
                );
            }
            let _ = std::fs::remove_dir_all(&failed);
            let restarted = host.start_healthy().is_ok();
            Ok(Outcome::RolledBack { reason: error.to_string(), restarted })
        }
    }
}

/// The real host: the node's own `ragon server ...` commands, run from `current`.
pub struct CommandHost<'a> {
    pub install: &'a NodeInstall,
    /// Seconds `server start` waits for a healthy engine.
    pub wait_s: u64,
}

impl CommandHost<'_> {
    fn server(&self, args: &[&str]) -> anyhow::Result<std::process::Output> {
        let output = std::process::Command::new(self.install.ragon())
            .arg("server")
            .args(args)
            .output()
            .with_context(|| format!("could not run {}", self.install.ragon().display()))?;
        Ok(output)
    }
}

impl NodeHost for CommandHost<'_> {
    fn jobs_in_flight(&self) -> anyhow::Result<usize> {
        let jobs = self.install.jobs_dir.as_ref().map(|dir| dir.display().to_string());
        let mut args = vec!["job", "list"];
        if let Some(jobs) = &jobs {
            args.extend(["--jobs-dir", jobs.as_str()]);
        }
        let output = self.server(&args)?;
        if !output.status.success() {
            bail!("{}", job_list_error(&String::from_utf8_lossy(&output.stderr)));
        }
        count_in_flight(&String::from_utf8_lossy(&output.stdout))
    }

    fn stop(&self) -> anyhow::Result<()> {
        let state = self.install.state_dir.display().to_string();
        let output = self.server(&["stop", "--name", &self.install.service, "--state-dir", &state])?;
        if output.status.success() { Ok(()) } else { bail!("ragon server stop: {}", String::from_utf8_lossy(&output.stderr).trim()) }
    }

    fn start_healthy(&self) -> anyhow::Result<()> {
        let state = self.install.state_dir.display().to_string();
        let (wait, port) = (self.wait_s.to_string(), self.install.port.to_string());
        let common = ["--name", self.install.service.as_str(), "--state-dir", state.as_str(), "--port", port.as_str()];
        let mut start = vec!["start", "--wait", wait.as_str()];
        start.extend(common);
        let _ = self.server(&start)?;
        let mut status = vec!["status"];
        status.extend(common);
        let output = self.server(&status)?;
        if output.status.success() {
            Ok(())
        } else {
            bail!("the service is not healthy after {} s: {}", self.wait_s, String::from_utf8_lossy(&output.stdout).replace('\n', " ").trim())
        }
    }
}

/// The system's `tar` (bsdtar reads zip): the one in System32, not whatever is first on PATH.
fn system_tar() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(|root| PathBuf::from(root).join("System32").join("tar.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("tar"))
}

/// Seconds the new version has to be healthy: `RAGON_NODE_UPDATE_WAIT_S`, else ten minutes, the
/// supervisor's own limit for an engine that loads a model (core.rs).
pub fn wait_seconds() -> u64 {
    std::env::var("RAGON_NODE_UPDATE_WAIT_S").ok().and_then(|v| v.parse().ok()).filter(|&s| s > 0).unwrap_or(600)
}

/// Unpacks a zip with the system's `tar` (bsdtar reads zip), after checking its listing.
pub fn extract_zip(zip: &Path, dest: &Path) -> anyhow::Result<()> {
    let listing = std::process::Command::new(system_tar()).arg("-tf").arg(zip).output().context("could not run tar")?;
    if !listing.status.success() {
        bail!("{} is not a readable zip", zip.display());
    }
    if !entries_safe(&String::from_utf8_lossy(&listing.stdout)) {
        bail!("{} has an entry outside its folder; nothing was installed", zip.display());
    }
    std::fs::create_dir_all(dest)?;
    let status = std::process::Command::new(system_tar()).arg("-xf").arg(zip).arg("-C").arg(dest).status().context("could not run tar")?;
    if !status.success() {
        bail!("could not unpack {}", zip.display());
    }
    Ok(())
}

/// The node's engine asset in a release: `ragon-engine-<version>-windows-gfx1151.zip`.
pub fn engine_asset(tag: &str) -> String {
    format!("ragon-engine-{}-windows-gfx1151.zip", tag.trim_start_matches('v'))
}

/// Updates the node to release `tag`: the engine zip is checked against the release's signed
/// `SHA256SUMS` (`all_sums`), `binaries` are the release's own command binaries for this machine
/// (downloaded and checked against the signed sums by the caller, never copied from a client
/// folder the user can write), both are staged, and [`swap`] does the rest. Needs an elevated
/// shell, since `%ProgramFiles%` is not writable by users.
pub async fn update<F, Fut>(
    install: &NodeInstall,
    tag: &str,
    assets: &std::collections::HashMap<&str, &str>,
    download: &F,
    all_sums: &[u8],
    binaries: &[(String, Vec<u8>)],
) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<u8>>>,
{
    refuse_node_own(&std::env::current_exe()?, install)?;
    if !binaries.iter().any(|(name, _)| name == "ragon.exe") {
        bail!("release {tag} carries no ragon.exe for this machine; the node was not updated");
    }
    let asset = engine_asset(tag);
    if !assets.contains_key(asset.as_str()) {
        bail!("release {tag} carries no {asset}; the node was not updated");
    }
    let expected = crate::update::parse_sums(&String::from_utf8_lossy(all_sums))
        .remove(&asset)
        .with_context(|| format!("SHA256SUMS has no checksum for {asset}; the node was not updated"))?;
    let zip_bytes = download(asset.clone()).await?;
    if crate::update::hex_sha256(&zip_bytes) != expected {
        bail!("{asset} does not match its checksum in SHA256SUMS; the node was not updated");
    }
    let staging = install.staging();
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(staging.join("bin")).with_context(|| {
        format!("could not write under {}: run `ragon update` from an elevated shell (Run as administrator)", install.root.display())
    })?;
    let zip_path = install.root.join("staging.zip");
    std::fs::write(&zip_path, &zip_bytes)?;
    let staged = (|| -> anyhow::Result<()> {
        for (name, binary) in binaries {
            std::fs::write(staging.join("bin").join(name), binary)?;
        }
        extract_zip(&zip_path, &staging.join("engine"))
    })();
    let _ = std::fs::remove_file(&zip_path);
    if let Err(error) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let host = CommandHost { install, wait_s: wait_seconds() };
    let outcome = swap(install, &host, tag);
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    match outcome? {
        Outcome::Updated => {
            println!("✅ node updated to {tag}; the previous version is kept in {}", install.previous().display());
            Ok(())
        }
        Outcome::RolledBack { reason, restarted } => bail!(
            "the node did not come up healthy on {tag} ({reason}); rolled back to {}{}",
            install.tag,
            if restarted { ", running again" } else { ", but it did not start: run `ragon server status`" }
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeHost<'a> {
        install: &'a NodeInstall,
        jobs: usize,
        /// Starts that fail, by call number (1-based).
        failing_starts: Vec<usize>,
        /// Renames that fail, by call number (1-based).
        failing_renames: Vec<usize>,
        renames: RefCell<usize>,
        calls: RefCell<Vec<String>>,
    }

    impl NodeHost for FakeHost<'_> {
        fn jobs_in_flight(&self) -> anyhow::Result<usize> {
            self.calls.borrow_mut().push("jobs".into());
            Ok(self.jobs)
        }
        fn stop(&self) -> anyhow::Result<()> {
            self.calls.borrow_mut().push("stop".into());
            Ok(())
        }
        fn start_healthy(&self) -> anyhow::Result<()> {
            let number = self.calls.borrow().iter().filter(|c| c.starts_with("start")).count() + 1;
            let version = std::fs::read_to_string(self.install.current().join("bin/ragon.exe")).unwrap_or_default();
            self.calls.borrow_mut().push(format!("start {version}"));
            if self.failing_starts.contains(&number) { bail!("not healthy") } else { Ok(()) }
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            *self.renames.borrow_mut() += 1;
            if self.failing_renames.contains(&self.renames.borrow()) {
                return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "in use"));
            }
            std::fs::rename(from, to)
        }
    }

    fn node(name: &str) -> NodeInstall {
        let root = std::env::temp_dir().join(format!("ragon-node-update-{name}-{}", std::process::id())).join("Ragon");
        let _ = std::fs::remove_dir_all(&root);
        let install = NodeInstall {
            root: root.clone(),
            tag: "v0.9.9".into(),
            service: "ragon-node".into(),
            port: 8000,
            state_dir: root.join("state"),
            jobs_dir: None,
        };
        for (dir, version) in [(install.current(), "old"), (install.staging(), "new")] {
            std::fs::create_dir_all(dir.join("bin")).unwrap();
            std::fs::create_dir_all(dir.join("engine")).unwrap();
            std::fs::write(dir.join("bin/ragon.exe"), version).unwrap();
            std::fs::write(dir.join("engine/ragon-serve.exe"), version).unwrap();
        }
        std::fs::write(root.join(RECORD), "tag=v0.9.9\r\nservice=ragon-node\r\ncreated=user:RagonJob\r\nport=8000\r\nstate=C:\\x\r\n").unwrap();
        install
    }

    fn host<'a>(install: &'a NodeInstall, jobs: usize, failing: &[usize]) -> FakeHost<'a> {
        FakeHost {
            install,
            jobs,
            failing_starts: failing.to_vec(),
            failing_renames: Vec::new(),
            renames: RefCell::new(0),
            calls: RefCell::new(Vec::new()),
        }
    }

    #[test]
    fn a_failed_restore_puts_the_new_version_back_and_starts_it() {
        // Renames: 1 current->previous, 2 staging->current, 3 current->failed, 4 previous->current.
        let install = node("restore");
        let mut host = host(&install, 0, &[1]);
        host.failing_renames = vec![4];
        let error = swap(&install, &host, "v0.9.10").unwrap_err().to_string();
        assert!(error.contains("could not be put back") && error.contains("v0.9.10 is running"), "{error}");
        assert_eq!(std::fs::read_to_string(install.current().join("bin/ragon.exe")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(install.previous().join("bin/ragon.exe")).unwrap(), "old");
        assert_eq!(*host.calls.borrow(), vec!["jobs", "stop", "start new", "stop", "start new"]);
    }

    #[test]
    fn a_failed_move_away_still_starts_a_service_and_says_which() {
        let install = node("moveaway");
        let mut host = host(&install, 0, &[1]);
        host.failing_renames = vec![3];
        let error = swap(&install, &host, "v0.9.10").unwrap_err().to_string();
        assert!(error.contains("could not be moved away") && error.contains("old version v0.9.9 is kept"), "{error}");
        assert_eq!(*host.calls.borrow(), vec!["jobs", "stop", "start new", "stop", "start new"]);
    }

    #[test]
    fn the_nodes_own_ragon_is_refused_through_a_link_and_the_client_is_not() {
        let install = node("own");
        let exe = install.current().join("bin/ragon.exe");
        assert!(refuse_node_own(&exe, &install).unwrap_err().to_string().contains("node's own copy"));
        // The machine PATH reaches it through a junction (C:\Ragon\bin -> current\bin).
        let link = install.root.parent().unwrap().join("link-bin");
        let _ = std::fs::remove_dir_all(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(install.current().join("bin"), &link).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(install.current().join("bin"))
            .output()
            .unwrap()
            .status
            .success());
        assert!(refuse_node_own(&link.join("ragon.exe"), &install).is_err());
        #[cfg(windows)]
        {
            let upper = PathBuf::from(link.join("RAGON.EXE").display().to_string().to_uppercase());
            assert!(refuse_node_own(&upper, &install).is_err());
        }
        let client = install.root.parent().unwrap().join("client/bin");
        std::fs::create_dir_all(&client).unwrap();
        std::fs::write(client.join("ragon.exe"), "client").unwrap();
        assert!(refuse_node_own(&client.join("ragon.exe"), &install).is_ok());
    }

    #[test]
    fn a_release_without_its_own_ragon_exe_is_refused_before_any_download() {
        let install = node("nobinaries");
        let assets = std::collections::HashMap::from([(engine_asset("v0.9.10").leak() as &str, "unused")]);
        let download = |name: String| async move {
            assert!(name.is_empty(), "downloaded {name}");
            anyhow::Ok(Vec::<u8>::new())
        };
        let only_host = vec![("codex-code-mode-host.exe".to_string(), b"host".to_vec())];
        let error = futures::executor::block_on(update(&install, "v0.9.10", &assets, &download, b"", &only_host))
            .unwrap_err()
            .to_string();
        assert!(error.contains("no ragon.exe"), "{error}");
        assert_eq!(std::fs::read_to_string(install.staging().join("bin/ragon.exe")).unwrap(), "new");
    }

    #[test]
    fn a_service_without_node_jobs_gets_a_reinstall_message() {
        let message = job_list_error("ragon: unknown server command \"job\"\n");
        assert!(message.contains("install.ps1 -Role node -Uninstall"), "{message}");
        assert_eq!(job_list_error("disk full\n"), "ragon server job list failed: disk full");
    }

    #[test]
    fn the_record_reads_back_and_a_short_one_is_refused() {
        let root = Path::new("C:/Program Files/Ragon");
        let parsed = parse_record("tag=v0.9.9\nservice=ragon-node\nport=8000\nstate=C:\\ProgramData\\Ragon\\node\n", root).unwrap();
        assert_eq!((parsed.tag.as_str(), parsed.port), ("v0.9.9", 8000));
        assert_eq!(parsed.state_dir, PathBuf::from("C:\\ProgramData\\Ragon\\node"));
        assert_eq!(parsed.jobs_dir, None);
        assert_eq!(parse_record("tag=v0.9.9\nservice=ragon-node\n", root), None);
        let jobs = parse_record("tag=v1\nservice=s\nport=8000\nstate=C:\\s\njobs=C:\\ProgramData\\Ragon\\jobs\n", root).unwrap();
        assert_eq!(jobs.jobs_dir, Some(PathBuf::from("C:\\ProgramData\\Ragon\\jobs")));
    }

    #[test]
    fn a_job_in_flight_stops_the_update_before_anything_changes() {
        let install = node("jobs");
        let host = host(&install, 2, &[]);
        let error = swap(&install, &host, "v0.9.10").unwrap_err().to_string();
        assert!(error.contains("2 job(s) running"), "{error}");
        assert_eq!(std::fs::read_to_string(install.current().join("bin/ragon.exe")).unwrap(), "old");
        assert!(install.staging().exists());
        assert_eq!(*host.calls.borrow(), vec!["jobs"]);
    }

    #[test]
    fn a_busy_node_is_refused_before_anything_is_downloaded_and_an_idle_one_passes() {
        let install = node("busy");
        let busy = host(&install, 1, &[]);
        let error = refuse_busy(&busy).unwrap_err().to_string();
        assert!(error.contains("1 job(s) running or queued"), "{error}");
        assert_eq!(*busy.calls.borrow(), vec!["jobs"]);
        let idle = host(&install, 0, &[]);
        refuse_busy(&idle).unwrap();
        assert_eq!(*idle.calls.borrow(), vec!["jobs"]);
    }

    #[test]
    fn a_good_update_swaps_the_trees_keeps_the_old_one_and_records_the_tag() {
        let install = node("good");
        let host = host(&install, 0, &[]);
        assert_eq!(swap(&install, &host, "v0.9.10").unwrap(), Outcome::Updated);
        assert_eq!(std::fs::read_to_string(install.current().join("bin/ragon.exe")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(install.previous().join("bin/ragon.exe")).unwrap(), "old");
        assert!(!install.staging().exists());
        assert_eq!(*host.calls.borrow(), vec!["jobs", "stop", "start new"]);
        let record = parse_record(&std::fs::read_to_string(install.root.join(RECORD)).unwrap(), &install.root).unwrap();
        assert_eq!(record.tag, "v0.9.10");
        assert!(std::fs::read_to_string(install.root.join(RECORD)).unwrap().contains("created=user:RagonJob"));
    }

    #[test]
    fn a_bad_update_is_rolled_back_and_the_old_version_runs_again() {
        let install = node("bad");
        let host = host(&install, 0, &[1]);
        let outcome = swap(&install, &host, "v0.9.10").unwrap();
        assert_eq!(outcome, Outcome::RolledBack { reason: "not healthy".into(), restarted: true });
        assert_eq!(std::fs::read_to_string(install.current().join("bin/ragon.exe")).unwrap(), "old");
        assert!(!install.previous().exists());
        assert!(!install.root.join("failed").exists());
        assert_eq!(*host.calls.borrow(), vec!["jobs", "stop", "start new", "stop", "start old"]);
        assert!(std::fs::read_to_string(install.root.join(RECORD)).unwrap().contains("tag=v0.9.9"));
    }

    #[test]
    fn a_staged_tree_without_the_engine_is_refused_untouched() {
        let install = node("noengine");
        std::fs::remove_file(install.staging().join("engine/ragon-serve.exe")).unwrap();
        let host = host(&install, 0, &[]);
        assert!(swap(&install, &host, "v0.9.10").unwrap_err().to_string().contains("engine/ragon-serve.exe"));
        assert!(host.calls.borrow().is_empty());
    }

    #[test]
    fn an_older_rollback_folder_is_replaced_by_the_one_just_made() {
        let install = node("older");
        std::fs::create_dir_all(install.previous().join("bin")).unwrap();
        std::fs::write(install.previous().join("bin/ragon.exe"), "older").unwrap();
        let host = host(&install, 0, &[]);
        swap(&install, &host, "v0.9.10").unwrap();
        assert_eq!(std::fs::read_to_string(install.previous().join("bin/ragon.exe")).unwrap(), "old");
    }

    #[test]
    fn running_and_queued_jobs_count_and_finished_ones_do_not() {
        let listing = r#"[{"id":"a","status":"done"},{"id":"b","status":"running"},{"id":"c","status":"queued"},{"id":"d","status":"failed"}]"#;
        assert_eq!(count_in_flight(listing).unwrap(), 2);
        assert_eq!(count_in_flight("[]").unwrap(), 0);
        assert!(count_in_flight("not json").is_err());
    }

    #[test]
    fn a_zip_listing_that_leaves_its_folder_is_unsafe() {
        assert!(entries_safe("ragon-serve.exe\nkernels/a.co\nkernels\\b.co\n"));
        assert!(!entries_safe("ragon-serve.exe\n../evil.txt\n"));
        assert!(!entries_safe("kernels\\..\\..\\evil.txt\n"));
        assert!(!entries_safe("/etc/passwd\n"));
        assert!(!entries_safe("C:/Windows/x.dll\n"));
    }
}
