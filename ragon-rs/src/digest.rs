//! Whether Ragon Digest runs inside Ragon: a long command output reaches the model as a digest,
//! and the output itself is kept in a file the agent reads by line numbers.
//!
//! The rule is the leaf crate `ragon-rs/digest`, which Codex's core calls on each command output
//! through patch `0043`. This module decides once per launch whether it runs. Off by default until
//! the SWE-bench comparison decides; `DREAMFERENCE_DIGEST_INSIDE` or `inside = true` in the
//! `[digest]` table of the config file switches it on, and `min_lines` there sets the threshold.
//! When it is off nothing is configured, so the crate hands every output back untouched.

use std::path::Path;

/// The environment variable that switches the digest on or off for one process.
pub const ENV: &str = "DREAMFERENCE_DIGEST_INSIDE";

/// The table in the Dreamference config file.
pub const TABLE: &str = "digest";

/// The key in [`TABLE`] that switches it on.
pub const KEY: &str = "inside";

/// The key in [`TABLE`] that sets the number of lines from which an output is digested.
pub const MIN_LINES_KEY: &str = "min_lines";

/// Off until the SWE-bench comparison shows it costs no resolved task.
pub const DEFAULT: bool = false;

fn settings() -> toml::Table {
    crate::config_file()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default()
}

fn table(settings: &toml::Table) -> Option<&toml::Table> {
    settings.get(TABLE).and_then(toml::Value::as_table)
}

/// Whether the digest is on for this launch: the environment first, then the config file.
pub fn enabled(env: Option<&str>, settings: &toml::Table) -> bool {
    if let Some(value) = env.filter(|value| !value.is_empty()) {
        return matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
    }
    table(settings).and_then(|table| table.get(KEY)).and_then(toml::Value::as_bool).unwrap_or(DEFAULT)
}

/// The threshold in lines: the config file's `min_lines` when it gives a positive number.
pub fn min_lines(settings: &toml::Table) -> usize {
    table(settings)
        .and_then(|table| table.get(MIN_LINES_KEY))
        .and_then(toml::Value::as_integer)
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n > 0)
        .unwrap_or(ragon_digest::MIN_LINES)
}

/// Switches the digest on for this process when the setting says so; does nothing otherwise.
pub fn configure(codex_home: &Path) {
    let settings = settings();
    if !enabled(std::env::var(ENV).ok().as_deref(), &settings) {
        return;
    }
    ragon_digest::configure(codex_home, min_lines(&settings));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> toml::Table {
        text.parse().unwrap()
    }

    #[test]
    fn it_is_off_unless_switched_on() {
        assert!(!enabled(None, &toml::Table::new()));
        assert!(!enabled(Some(""), &toml::Table::new()));
        assert!(!enabled(None, &parse("[digest]\ninside = false\n")));
        assert!(!enabled(None, &parse("inside = true\n")));
    }

    #[test]
    fn the_config_file_or_the_environment_switches_it_on() {
        assert!(enabled(None, &parse("[digest]\ninside = true\n")));
        for value in ["1", "true", "YES", "on"] {
            assert!(enabled(Some(value), &toml::Table::new()), "{value}");
        }
    }

    #[test]
    fn the_environment_wins_over_the_config_file() {
        assert!(!enabled(Some("0"), &parse("[digest]\ninside = true\n")));
        assert!(!enabled(Some("off"), &parse("[digest]\ninside = true\n")));
        assert!(enabled(Some("1"), &parse("[digest]\ninside = false\n")));
    }

    #[test]
    fn the_threshold_is_two_hundred_lines_unless_set() {
        assert_eq!(min_lines(&toml::Table::new()), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 80\n")), 80);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 0\n")), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = -5\n")), 200);
    }
}
