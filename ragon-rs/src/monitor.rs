//! `ragon monitor …`: the session monitor (specs/DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md Part B).
//! The commands are the `ragon-monitor` crate's (`monitor/src/cli.rs`); this module only tells it how
//! its timer starts it again: as this `ragon` with `monitor` in front of the command.

/// `ragon monitor …` from a shell or the timer. Returns the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "ragon".to_string());
    ragon_monitor::cli::run(args, &[exe, "monitor".to_string()])
}
