//! `ragon signal …`: Ragon over Signal (specs/DREAMFERENCE_RAGON_SIGNAL.md §6). The commands
//! are the `ragon-signal` crate's (`signal/src/cli.rs`); this module only says where the bridge
//! binary is. The daemon itself is not `ragon`: the system account `ragon-signal` runs the small
//! `ragon-signal` binary that setup copies to /usr/local/lib/ragon, so `ragon signal setup` needs
//! the copy installed beside `ragon` (by `ragon update` or `ragon-admin codex build`).

use std::path::Path;
use std::path::PathBuf;

/// The bridge binary's name, installed beside `ragon` by `ragon update` and `ragon-admin codex build`.
pub const BRIDGE_COMMAND: &str = crate::update::SIGNAL_COMMAND;

/// `ragon-signal` beside the running `ragon`, links resolved (`~/.local/bin/ragon` is a link into the
/// install folder); None when there is none.
pub fn bridge_beside(exe: &Path) -> Option<PathBuf> {
    let exe = exe.canonicalize().ok()?;
    let candidate = exe.parent()?.join(BRIDGE_COMMAND);
    candidate.is_file().then_some(candidate)
}

/// `ragon signal …` from a shell. Returns the exit code.
pub async fn run_cli(args: &[String]) -> i32 {
    let bridge = std::env::current_exe().ok().and_then(|exe| bridge_beside(&exe));
    let args = args.to_vec();
    // The commands are synchronous, and the bridge's own build their own runtime; a blocking
    // thread keeps both away from the launcher's.
    let invocation = ragon_signal::cli::Invocation::new(bridge, false);
    tokio::task::spawn_blocking(move || ragon_signal::cli::run(&args, &invocation)).await.unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_is_found_beside_the_resolved_executable() {
        let root = std::env::temp_dir().join(format!("ragon-signal-beside-{}", std::process::id()));
        let install = root.join("install/bin");
        let links = root.join("links");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&links).unwrap();
        std::fs::write(install.join("ragon"), b"").unwrap();
        std::os::unix::fs::symlink(install.join("ragon"), links.join("ragon")).unwrap();
        assert_eq!(bridge_beside(&links.join("ragon")), None, "not installed yet");
        std::fs::write(install.join(BRIDGE_COMMAND), b"").unwrap();
        assert_eq!(bridge_beside(&links.join("ragon")), Some(install.canonicalize().unwrap().join(BRIDGE_COMMAND)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
