//! `ragon delegate …`: the Claude Code delegation plugin server and CLI
//! (specs/DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md Part A).
//! The implementation lives in the `ragon-delegate` leaf crate (`delegate/src/cli.rs`).

/// `ragon delegate …` from Claude Code's MCP stdio transport or a shell. Returns the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    ragon_delegate::cli::run(args)
}
