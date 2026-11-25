//! `ragon server start|stop|status|run|install|uninstall` on macOS: the Mac node's model server
//! under a supervisor (owner ruling MN1 (b); specs/DREAMFERENCE_MAC_NODE.md §4.2). The supervisor is
//! the `ragon-supervisor` crate (ragon-rs/supervisor), the same platform-neutral core
//! (supervisor/src/core.rs) as the Windows node; its macOS parts are the LaunchDaemon
//! `ai.dreamference.ragon-node` run as `_ragon` (supervisor/src/macos_launchd.rs), the power
//! settings on mains (macos_power.rs) and the guard that takes the engine down with a dead
//! supervisor (macos_guard.rs). This module only hands the arguments after `server` to it.
//!
//! Elsewhere it is not used: Windows goes through src/supervisor_windows.rs, Linux has
//! `ragon-admin server start`.

/// Runs `ragon server <args>` and returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    #[cfg(target_os = "macos")]
    {
        ragon_supervisor::cli::run(args)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = args;
        eprintln!("`ragon server` here is the Mac node's supervisor; it runs on macOS only.");
        2
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bad_call_exits_2_everywhere() {
        // macOS: the usage (`--help` is not a command); elsewhere: the "macOS only" notice.
        assert_eq!(super::run_cli(&["--help".to_string()]), 2);
    }
}
