//! Whether Ragon Digest runs inside Ragon: a long command output reaches the model as a digest,
//! and the output itself is kept in a file the agent reads by line numbers.
//!
//! The rule is the leaf crate `ragon-rs/digest`, which Codex's core calls on each command output
//! through patch `0043`. This module decides once per launch whether it runs. Off by default until
//! the SWE-bench comparison decides; `DREAMFERENCE_DIGEST_INSIDE` or `inside = true` in the
//! `[digest]` table of the config file switches it on, and `min_lines` there sets the threshold.
//! When it is off nothing is configured, so the crate hands every output back untouched.
//!
//! Two more settings in the same table, each off by default and each with its own variable (the
//! owner's answers 2 and 3 of 2026-11-20): `last_good` leaves out of a digest the lines the last
//! successful run of the same command printed too, and `hide_old` replaces every output older than
//! the last ten steps by one line naming where the whole output is (`ragon_masking::window`, which
//! works with or without the digest).

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

/// The environment variable for [`LAST_GOOD_KEY`].
pub const LAST_GOOD_ENV: &str = "DREAMFERENCE_DIGEST_LAST_GOOD";

/// The key in [`TABLE`] that leaves out of a digest the lines of the command's last good run.
pub const LAST_GOOD_KEY: &str = "last_good";

/// The environment variable for [`HIDE_OLD_KEY`].
pub const HIDE_OLD_ENV: &str = "DREAMFERENCE_DIGEST_HIDE_OLD";

/// The key in [`TABLE`] that hides outputs older than the last ten steps.
pub const HIDE_OLD_KEY: &str = "hide_old";

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
    flag(env, settings, KEY)
}

/// One switch of the `[digest]` table: the environment first, then the config file, then off.
pub fn flag(env: Option<&str>, settings: &toml::Table, key: &str) -> bool {
    if let Some(value) = env.filter(|value| !value.is_empty()) {
        return matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on");
    }
    table(settings).and_then(|table| table.get(key)).and_then(toml::Value::as_bool).unwrap_or(DEFAULT)
}

/// Whether hiding old outputs is on for this launch.
pub fn hide_old_now() -> bool {
    flag(std::env::var(HIDE_OLD_ENV).ok().as_deref(), &settings(), HIDE_OLD_KEY)
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

/// Switches the digest and hiding old outputs on for this process when their settings say so;
/// does nothing otherwise.
pub fn configure(codex_home: &Path) {
    let settings = settings();
    let env = |name: &str| std::env::var(name).ok();
    if flag(env(HIDE_OLD_ENV).as_deref(), &settings, HIDE_OLD_KEY) {
        ragon_masking::window::prune(codex_home, std::time::SystemTime::now());
        ragon_masking::window::set_window(ragon_masking::window::Window::default(), codex_home);
    }
    if !enabled(env(ENV).as_deref(), &settings) {
        return;
    }
    let last_good = flag(env(LAST_GOOD_ENV).as_deref(), &settings, LAST_GOOD_KEY);
    ragon_digest::configure_with(codex_home, ragon_digest::Options { min_lines: min_lines(&settings), last_good });
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
    fn last_good_and_hide_old_are_off_by_default_and_each_has_its_own_switch() {
        let empty = toml::Table::new();
        for key in [LAST_GOOD_KEY, HIDE_OLD_KEY] {
            assert!(!flag(None, &empty, key), "{key}");
            assert!(!flag(None, &parse("[digest]\ninside = true\n"), key), "{key}: inside does not switch it on");
            assert!(flag(None, &parse(&format!("[digest]\n{key} = true\n")), key), "{key}");
            assert!(!flag(Some("0"), &parse(&format!("[digest]\n{key} = true\n")), key), "{key}");
            assert!(flag(Some("1"), &empty, key), "{key}");
        }
        assert!(!enabled(None, &parse("[digest]\nlast_good = true\nhide_old = true\n")));
        assert_ne!(LAST_GOOD_ENV, HIDE_OLD_ENV);
    }

    #[test]
    fn the_threshold_is_two_hundred_lines_unless_set() {
        assert_eq!(min_lines(&toml::Table::new()), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 80\n")), 80);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = 0\n")), 200);
        assert_eq!(min_lines(&parse("[digest]\nmin_lines = -5\n")), 200);
    }
}
