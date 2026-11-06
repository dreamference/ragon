//! `ragon delegate …`: the Claude Code delegation plugin server and CLI
//! (specs/DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md Part A).
//! The implementation lives in the `ragon-delegate` leaf crate (`delegate/src/cli.rs`).

/// `ragon delegate …` from Claude Code's MCP stdio transport or a shell. Returns the exit code.
pub fn run_cli(args: &[String]) -> i32 {
    ragon_delegate::cli::run(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn the_wrapper_passes_the_exit_code_of_the_crate_through() {
        assert_eq!(run_cli(&args(&["--help"])), 0, "help succeeds");
        assert_eq!(run_cli(&[]), 2, "no subcommand is a usage error");
        assert_eq!(run_cli(&args(&["mcp", "--bogus"])), 2, "an unknown argument to mcp is refused");
    }
}
