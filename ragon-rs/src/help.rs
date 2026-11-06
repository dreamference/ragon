//! Codex's help text, rebranded as it is parsed.
//!
//! Codex's subcommands and options describe themselves in doc comments — "Run Codex
//! non-interactively", "Manage Codex plugins" — dozens of them across several crates. Patching each
//! string would make the patch series grow with every Codex release, so patch 0002 instead has
//! `cli_main` parse through [`parse`], which walks the clap command tree once and rewrites the
//! prose before clap renders any help or error.
//!
//! Only the product name changes. Paths and identifiers that merely contain it — `~/.codex`,
//! `$CODEX_HOME`, `codex-code-mode-host`, `openai/codex` — are left alone, because they are real
//! and renaming them in help would describe files that do not exist.

use std::ffi::OsString;

use clap::Command;
use clap::Parser;

/// Parses `args` into `T` exactly as `T::parse_from` would, but with rebranded help text.
pub fn parse<T: Parser>(args: Vec<OsString>) -> T {
    let mut command = rebrand_command(T::command());
    let mut matches = command.clone().get_matches_from(args);
    T::from_arg_matches_mut(&mut matches).unwrap_or_else(|err| err.format(&mut command).exit())
}

/// Rewrites every piece of prose on a command, its arguments and, recursively, its subcommands.
pub fn rebrand_command(mut command: Command) -> Command {
    if let Some(text) = command.get_about().map(|s| rebrand_text(&s.to_string())) {
        command = command.about(text);
    }
    if let Some(text) = command.get_long_about().map(|s| rebrand_text(&s.to_string())) {
        command = command.long_about(text);
    }
    if let Some(text) = command.get_before_help().map(|s| rebrand_text(&s.to_string())) {
        command = command.before_help(text);
    }
    if let Some(text) = command.get_before_long_help().map(|s| rebrand_text(&s.to_string())) {
        command = command.before_long_help(text);
    }
    if let Some(text) = command.get_after_help().map(|s| rebrand_text(&s.to_string())) {
        command = command.after_help(text);
    }
    if let Some(text) = command.get_after_long_help().map(|s| rebrand_text(&s.to_string())) {
        command = command.after_long_help(text);
    }
    if let Some(name) = command.get_bin_name().map(rebrand_text) {
        command = command.bin_name(name);
    }
    if let Some(name) = command.get_display_name().map(rebrand_text) {
        command = command.display_name(name);
    }
    command
        .mut_args(|mut arg| {
            if let Some(text) = arg.get_help().map(|s| rebrand_text(&s.to_string())) {
                arg = arg.help(text);
            }
            if let Some(text) = arg.get_long_help().map(|s| rebrand_text(&s.to_string())) {
                arg = arg.long_help(text);
            }
            arg
        })
        .mut_subcommands(rebrand_command)
}

/// Replaces the product name in a piece of help text.
///
/// "OpenAI Codex" and "Codex CLI" become "Ragon"; any other capitalised "Codex" as a word becomes
/// "Ragon"; a lowercase `codex` becomes `ragon` only where it is the command a user types —
/// a whole word not attached to a path, a variable or a longer identifier. A line that points at
/// the upstream vendor's documentation (`openai.com`) is dropped: it documents another product.
pub fn rebrand_text(text: &str) -> String {
    let text = text
        .lines()
        .filter(|line| !line.to_ascii_lowercase().contains("openai.com"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.replace("OpenAI Codex", "Ragon").replace("Codex CLI", "Ragon");
    let text = replace_word(&text, "Codex", "Ragon", |_, _| true);
    replace_word(&text, "codex", "ragon", |before, after| {
        !matches!(before, Some('.' | '/' | '~' | '$' | '-' | '_' | '@'))
            && !matches!(after, Some('-' | '_' | '/' | '.' | '@'))
    })
}

/// Replaces `word` where it stands alone (not inside a longer alphanumeric run) and `allowed`
/// accepts the characters on either side.
fn replace_word(
    text: &str,
    word: &str,
    replacement: &str,
    allowed: impl Fn(Option<char>, Option<char>) -> bool,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut before: Option<char> = None;
    while let Some(index) = rest.find(word) {
        let (head, tail) = rest.split_at(index);
        out.push_str(head);
        let prev = head.chars().next_back().or(before);
        let next = tail[word.len()..].chars().next();
        let standalone = !prev.is_some_and(char::is_alphanumeric)
            && !next.is_some_and(char::is_alphanumeric);
        if standalone && allowed(prev, next) {
            out.push_str(replacement);
        } else {
            out.push_str(word);
        }
        before = word.chars().next_back();
        rest = &tail[word.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_product_name_is_replaced_in_prose() {
        assert_eq!(rebrand_text("Codex CLI"), "Ragon");
        assert_eq!(rebrand_text("Run Codex non-interactively"), "Run Ragon non-interactively");
        assert_eq!(rebrand_text("a Codex-provided sandbox"), "a Ragon-provided sandbox");
        assert_eq!(rebrand_text("OpenAI Codex (v1)"), "Ragon (v1)");
        assert_eq!(
            rebrand_text("Export metrics.\n\nSee https://developers.openai.com/codex/x for more details."),
            "Export metrics.\n"
        );
    }

    #[test]
    fn the_typed_command_is_replaced_but_paths_and_identifiers_are_not() {
        assert_eq!(rebrand_text("codex exec [OPTIONS]"), "ragon exec [OPTIONS]");
        assert_eq!(rebrand_text("  codex plugin marketplace add ./x"), "  ragon plugin marketplace add ./x");
        assert_eq!(rebrand_text("run `codex` again"), "run `ragon` again");
        assert_eq!(rebrand_text("from `~/.codex/config.toml`"), "from `~/.codex/config.toml`");
        assert_eq!(rebrand_text("$CODEX_HOME/x"), "$CODEX_HOME/x");
        assert_eq!(rebrand_text("codex-code-mode-host"), "codex-code-mode-host");
        assert_eq!(rebrand_text("openai/codex releases"), "openai/codex releases");
        assert_eq!(rebrand_text("codexes"), "codexes");
    }

    #[test]
    fn help_is_rebranded_through_the_whole_command_tree() {
        let command = rebrand_command(
            Command::new("codex")
                .about("Codex CLI")
                .arg(clap::Arg::new("x").long("x").help("Used by Codex"))
                .subcommand(Command::new("exec").about("Run Codex non-interactively")),
        );
        assert_eq!(command.get_about().map(ToString::to_string), Some("Ragon".into()));
        let arg_help = command
            .get_arguments()
            .find(|arg| arg.get_id() == "x")
            .and_then(|arg| arg.get_help())
            .map(ToString::to_string);
        assert_eq!(arg_help, Some("Used by Ragon".into()));
        let exec_about = command
            .find_subcommand("exec")
            .and_then(|exec| exec.get_about())
            .map(ToString::to_string);
        assert_eq!(exec_about, Some("Run Ragon non-interactively".into()));
    }

    #[derive(Parser, Debug)]
    #[command(name = "codex", about = "OpenAI Codex CLI")]
    struct Sample {
        /// Prompt for Codex
        #[arg(long)]
        prompt: Option<String>,
    }

    #[test]
    fn the_parse_entry_point_yields_the_typed_arguments() {
        let sample: Sample = parse(vec!["codex".into(), "--prompt".into(), "hi".into()]);
        assert_eq!(sample.prompt.as_deref(), Some("hi"));
        let sample: Sample = parse(vec!["codex".into()]);
        assert_eq!(sample.prompt, None);
    }

    #[test]
    fn a_line_pointing_at_the_vendors_documentation_is_dropped() {
        assert_eq!(rebrand_text("Run it.\nSee https://developers.openai.com/codex/cli"), "Run it.");
        assert_eq!(rebrand_text("Docs at OpenAI.com\nkeep"), "keep");
        assert_eq!(rebrand_text(""), "");
    }

    #[test]
    fn the_product_names_become_one_name() {
        assert_eq!(rebrand_text("OpenAI Codex"), "Ragon");
        assert_eq!(rebrand_text("Codex CLI is here"), "Ragon is here");
        assert_eq!(rebrand_text("Codex, the agent"), "Ragon, the agent");
    }

    #[test]
    fn a_capitalised_name_inside_a_longer_word_stays() {
        assert_eq!(rebrand_text("Codexes and MyCodex"), "Codexes and MyCodex");
    }

    #[test]
    fn a_lowercase_name_beside_a_path_mark_stays() {
        for text in ["~/codex", "a/codex/b", "codex.toml", "my-codex", "codex_home", "user@codex", "codex@1"] {
            assert_eq!(rebrand_text(text), text);
        }
    }

    #[test]
    fn every_standing_command_in_a_line_is_replaced() {
        assert_eq!(rebrand_text("codex exec and codex resume"), "ragon exec and ragon resume");
        assert_eq!(rebrand_text("(codex)"), "(ragon)");
    }

    #[test]
    fn the_after_help_and_the_binary_name_are_rebranded() {
        let command = rebrand_command(
            Command::new("codex")
                .bin_name("codex")
                .display_name("Codex")
                .after_help("More on Codex")
                .before_help("About Codex"),
        );
        assert_eq!(command.get_bin_name(), Some("ragon"));
        assert_eq!(command.get_display_name(), Some("Ragon"));
        assert_eq!(command.get_after_help().map(ToString::to_string), Some("More on Ragon".into()));
        assert_eq!(command.get_before_help().map(ToString::to_string), Some("About Ragon".into()));
    }

    #[test]
    fn a_command_without_prose_is_left_alone() {
        let command = rebrand_command(Command::new("tool").subcommand(Command::new("sub")));
        assert!(command.get_about().is_none());
        assert!(command.find_subcommand("sub").is_some_and(|sub| sub.get_about().is_none()));
    }

    #[test]
    fn nested_subcommands_are_reached() {
        let command = rebrand_command(
            Command::new("codex").subcommand(Command::new("a").subcommand(Command::new("b").about("Codex deep"))),
        );
        let deep = command.find_subcommand("a").and_then(|a| a.find_subcommand("b")).and_then(|b| b.get_about());
        assert_eq!(deep.map(ToString::to_string), Some("Ragon deep".into()));
    }
}
