//! Levels (§4.1) and the level of one component under a parsed spec (§4.2, §4.3).

use crate::{DEFAULT_LOG_LEVEL, ENV_NAME, LogLevelSpec, resolve_spec};
use std::path::Path;

/// A log level, most severe first so that `a <= b` reads "a is at least as severe as b".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl Level {
    /// The upper-case word in the line.
    pub fn word(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    /// The lower-case name the spec grammar and the redactor use.
    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }

    /// The syslog priority prefix journald reads (§5.1): ERROR 3, WARN 4, INFO 6, DEBUG and TRACE 7.
    pub fn syslog_prefix(self) -> &'static str {
        match self {
            Level::Error => "<3>",
            Level::Warn => "<4>",
            Level::Info => "<6>",
            Level::Debug | Level::Trace => "<7>",
        }
    }

    /// A level word of the grammar: `Some(None)` is `off`, `None` an unknown word.
    pub fn from_word(word: &str) -> Option<Option<Level>> {
        Some(match word {
            "error" => Some(Level::Error),
            "warn" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            "trace" => Some(Level::Trace),
            "off" => None,
            _ => return None,
        })
    }
}

impl LogLevelSpec {
    /// The level of one component, as the Python writer picks it: its override, else the
    /// default, else `info`. `None` means `off`.
    pub fn level_for(&self, component: &str) -> Option<Level> {
        let word = self
            .overrides
            .get(component)
            .or(self.default.as_ref())
            .map(String::as_str)
            .unwrap_or(DEFAULT_LOG_LEVEL);
        Level::from_word(word).unwrap_or(Some(Level::Info))
    }

    /// The spec in force (§4.3): `RAGON_LOG` from the environment, then `log.env`, then `info`.
    pub fn from_env(log_env_file: Option<&Path>) -> Self {
        let environment = std::env::var(ENV_NAME).ok();
        let log_env = log_env_file.and_then(|path| std::fs::read_to_string(path).ok());
        Self::parse(&resolve_spec(environment.as_deref(), log_env.as_deref()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_beats_the_default_and_off_is_none() {
        let spec = LogLevelSpec::parse("warn,engine=debug,gate=off");
        assert_eq!(spec.level_for("engine"), Some(Level::Debug));
        assert_eq!(spec.level_for("gate"), None);
        assert_eq!(spec.level_for("web"), Some(Level::Warn));
        assert_eq!(LogLevelSpec::parse("").level_for("web"), Some(Level::Info));
    }
}
