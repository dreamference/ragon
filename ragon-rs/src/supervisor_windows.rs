//! `ragon server start|stop|status|run|install|uninstall` on Windows: the node's model server under
//! a supervisor (DREAMFERENCE_WINDOWS_NODE §4.2). The supervisor itself is the `ragon-supervisor`
//! crate (ragon-rs/supervisor): its platform-neutral core (supervisor/src/core.rs) is shared with
//! the Mac node; the service and the Job Object are its Windows parts. This module only hands the
//! arguments after `server` to it.
//!
//! `ragon server start` with no engine after `--` starts the Windows engine recipe
//! (supervisor/src/recipe.rs): `ragon-serve.exe` from the unpacked engine zip under
//! `%ProgramFiles%\Ragon\engine`, the weights from `%ProgramData%\Ragon\models`.
//!
//! On Linux and macOS the model server is `ragon-admin server start` (the Python node tooling),
//! so `ragon server` says so and exits 2.

/// Runs `ragon server <args>` and returns the process exit code.
pub fn run_cli(args: &[String]) -> i32 {
    #[cfg(windows)]
    {
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
