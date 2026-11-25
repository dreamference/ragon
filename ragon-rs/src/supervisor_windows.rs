//! `ragon server start|stop|status|run|install|uninstall` on Windows: the node's model server under
//! a supervisor (DREAMFERENCE_WINDOWS_NODE §4.2). The supervisor itself is the `ragon-supervisor`
//! crate (ragon-rs/supervisor): its platform-neutral core (supervisor/src/core.rs) is shared with
//! the Mac node; the service and the Job Object are its Windows parts. This module only hands the
//! arguments after `server` to it, and the sender's job operations (`job-submit`, `job-list`, ...)
//! to the node's job store (node_job_windows.rs).
//!
//! On Linux and macOS the model server is `ragon-admin server start` (the Python node tooling),
//! so `ragon server` says so and exits 2.

// The job store is the supervisor crate's, a Windows-only dependency of the launcher.
#[cfg(windows)]
#[path = "node_job_windows.rs"]
pub mod node_job_windows;

/// Runs `ragon server <args>` and returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    #[cfg(windows)]
    {
        if node_job_windows::is_job_operation(args) {
            return node_job_windows::run_cli(args);
        }
        ragon_supervisor::cli::run(args)
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        eprintln!("`ragon server` is the Windows node's supervisor; on this platform the model server is `ragon-admin server start`.");
        2
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    #[test]
    fn elsewhere_it_points_at_the_node_tooling() {
        assert_eq!(super::run_cli(&["status".to_string()]), 2);
    }
}
