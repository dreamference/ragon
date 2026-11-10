//! The log level spec (specs/DREAMFERENCE_LOGGING.md §4.2-4.3): the grammar that `RAGON_LOG`
//! and `log.env` carry, and the precedence that picks the spec. Python reads the same grammar
//! through `dreamference/config/log_level_spec.py`; both read `ragon-rs/vectors/log_level_cases.txt`.

use std::collections::BTreeMap;

/// The levels of §4.1, in the order the grammar names them.
pub const LEVELS: [&str; 6] = ["error", "warn", "info", "debug", "trace", "off"];

/// The default when no tier sets a spec (§4.3, step 3).
pub const DEFAULT_LOG_LEVEL: &str = "info";

/// The variable that carries the spec in the environment and in `log.env`.
pub const ENV_NAME: &str = "RAGON_LOG";

/// A parsed spec: an optional default and the component overrides, with one warning per ignored part.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LogLevelSpec {
    pub default: Option<String>,
    pub overrides: BTreeMap<String, String>,
    pub warnings: Vec<String>,
}

impl LogLevelSpec {
    /// Parse a spec string. An unknown level or an empty part is ignored with a warning; the
    /// rest of the string still applies. A default must come first.
    pub fn parse(spec: &str) -> Self {
        let mut out = Self::default();
        if spec.trim().is_empty() {
            return out;
        }
        for (index, raw) in spec.split(',').enumerate() {
            let part = raw.trim();
            if let Some((component, level)) = part.split_once('=') {
                let (component, level) = (component.trim(), level.trim());
                if component.is_empty() || !LEVELS.contains(&level) {
                    out.warnings
                        .push(format!("ignored log level part {part:?}"));
                } else {
                    out.overrides
                        .insert(component.to_string(), level.to_string());
                }
            } else if index == 0 && LEVELS.contains(&part) {
                out.default = Some(part.to_string());
            } else {
                out.warnings
                    .push(format!("ignored log level part {part:?}"));
            }
        }
        out
    }
}

/// Whether a tier holds a spec: a value that is not blank. A blank tier counts as unset.
pub fn tier_value(candidate: Option<&str>) -> Option<&str> {
    candidate.map(str::trim).filter(|value| !value.is_empty())
}

/// The text of `RAGON_LOG` in a `log.env` file: the last `RAGON_LOG=` line, quotes removed.
pub fn read_log_env(text: &str) -> Option<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix("RAGON_LOG="))
        .map(|value| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
        .last()
}

/// The spec that applies (§4.3, the Rust and C++ tiers): `RAGON_LOG` from the environment, then
/// `log.env`, then `info`. A blank value in a tier counts as unset.
pub fn resolve_spec(environment: Option<&str>, log_env_text: Option<&str>) -> String {
    let from_log_env = log_env_text.and_then(read_log_env);
    tier_value(environment)
        .or_else(|| tier_value(from_log_env.as_deref()))
        .unwrap_or(DEFAULT_LOG_LEVEL)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_and_overrides_parse() {
        let spec = LogLevelSpec::parse("info,engine=debug,gate=trace");
        assert_eq!(spec.default.as_deref(), Some("info"));
        assert_eq!(
            spec.overrides.get("engine").map(String::as_str),
            Some("debug")
        );
        assert!(spec.warnings.is_empty());
    }

    #[test]
    fn the_environment_beats_log_env_and_log_env_beats_the_default() {
        assert_eq!(
            resolve_spec(Some("debug"), Some("RAGON_LOG=warn\n")),
            "debug"
        );
        assert_eq!(
            resolve_spec(Some("  "), Some("RAGON_LOG=\"warn\"\n")),
            "warn"
        );
        assert_eq!(resolve_spec(None, None), "info");
        assert_eq!(resolve_spec(None, Some("OTHER=1\n")), "info");
    }
}
