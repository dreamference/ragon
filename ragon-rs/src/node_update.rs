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
    })
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
pub fn swap(install: &NodeInstall, host: &dyn NodeHost, new_tag: &str) -> anyhow::Result<Outcome> {
    let staging = install.staging();
    for needed in ["bin/ragon.exe", "engine/ragon-serve.exe"] {
        if !staging.join(needed).is_file() {
            bail!("the staged update has no {needed}; nothing was changed");
        }
    }
    let running = host.jobs_in_flight().context("could not tell whether a job is running; nothing was changed")?;
    if running > 0 {
        bail!("{running} job(s) running or queued on this node; update when they finish (ragon server job list). Nothing was changed");
    }
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
    if let Err(e) = std::fs::rename(&current, &previous) {
        return Err(restart(format!("could not move the current version aside: {e}")));
    }
    if let Err(e) = std::fs::rename(&staging, &current) {
        let _ = std::fs::rename(&previous, &current);
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
            std::fs::rename(&current, &failed).context("the new version failed and could not be moved away")?;
            std::fs::rename(&previous, &current).context("the new version failed and the old one could not be restored")?;
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
        let output = self.server(&["job", "list"])?;
        if !output.status.success() {
            bail!("ragon server job list failed: {}", String::from_utf8_lossy(&output.stderr).trim());
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

/// Unpacks a zip with the system's `tar` (bsdtar reads zip), after checking its listing.
pub fn extract_zip(zip: &Path, dest: &Path) -> anyhow::Result<()> {
    let listing = std::process::Command::new("tar").arg("-tf").arg(zip).output().context("could not run tar")?;
    if !listing.status.success() {
        bail!("{} is not a readable zip", zip.display());
    }
    if !entries_safe(&String::from_utf8_lossy(&listing.stdout)) {
        bail!("{} has an entry outside its folder; nothing was installed", zip.display());
    }
    std::fs::create_dir_all(dest)?;
    let status = std::process::Command::new("tar").arg("-xf").arg(zip).arg("-C").arg(dest).status().context("could not run tar")?;
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
/// `SHA256SUMS` (`all_sums`), the client binaries the node runs are copied from `client_dir` (just
/// installed from the same release, under the same checks), both are staged, and [`swap`] does the
/// rest. Needs an elevated shell, since `%ProgramFiles%` is not writable by users.
pub async fn update<F, Fut>(
    install: &NodeInstall,
    tag: &str,
    assets: &std::collections::HashMap<&str, &str>,
    download: &F,
    all_sums: &[u8],
    client_dir: &Path,
) -> anyhow::Result<()>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<Vec<u8>>>,
{
    let exe = std::env::current_exe()?;
    if exe.starts_with(install.current()) {
        bail!(
            "this ragon.exe is the node's own; run `ragon update` from the client install ({}), so the node's files can be replaced",
            client_dir.display()
        );
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
        for entry in std::fs::read_dir(install.current().join("bin"))? {
            let name = entry?.file_name();
            let from = client_dir.join(&name);
            if from.is_file() {
                std::fs::copy(&from, staging.join("bin").join(&name))?;
            }
        }
        extract_zip(&zip_path, &staging.join("engine"))
    })();
    let _ = std::fs::remove_file(&zip_path);
    if let Err(error) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    let host = CommandHost { install, wait_s: 120 };
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
        FakeHost { install, jobs, failing_starts: failing.to_vec(), calls: RefCell::new(Vec::new()) }
    }

    #[test]
    fn the_record_reads_back_and_a_short_one_is_refused() {
        let root = Path::new("C:/Program Files/Ragon");
        let parsed = parse_record("tag=v0.9.9\nservice=ragon-node\nport=8000\nstate=C:\\ProgramData\\Ragon\\node\n", root).unwrap();
        assert_eq!((parsed.tag.as_str(), parsed.port), ("v0.9.9", 8000));
        assert_eq!(parsed.state_dir, PathBuf::from("C:\\ProgramData\\Ragon\\node"));
        assert_eq!(parse_record("tag=v0.9.9\nservice=ragon-node\n", root), None);
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
