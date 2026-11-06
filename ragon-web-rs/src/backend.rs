//! The seam between a search and the engine that answers it.
//!
//! A [`SearchBackend`] turns a query into the raw JSON that [`crate::search::from_searxng`]
//! reads. SearXNG is the only implementation. Which backends are used is the user's choice,
//! `[search] backends = ["searxng"]` in `~/.config/dreamference/config.toml`; it is never read
//! from the repository being worked in. A name nothing implements is skipped, and with no usable
//! name the answer is SearXNG, so a config that says nothing changes nothing.

use serde_json::Value;

use crate::agent;
use crate::node_locator::Node;
use crate::search::{JSON_FORMAT_HINT, SearchError, SearchOptions, UNAVAILABLE_HINT, start_hint};

/// The name SearXNG goes by in `[search] backends`.
pub const SEARXNG: &str = "searxng";

/// One place a search can be sent.
pub trait SearchBackend {
    /// The name used in `[search] backends`.
    fn name(&self) -> &'static str;

    /// Runs the query and returns the engine's JSON answer.
    fn fetch(&self, query: &str, options: &SearchOptions) -> Result<Value, SearchError>;
}

/// A SearXNG instance, asked over its JSON API.
pub struct SearxngBackend<'a> {
    base_url: &'a str,
    node: Option<&'a Node>,
}

impl<'a> SearxngBackend<'a> {
    /// A backend for the instance at `base_url`; `node` is the machine it is on (`None`: this one).
    pub fn new(base_url: &'a str, node: Option<&'a Node>) -> Self {
        Self { base_url, node }
    }
}

impl SearchBackend for SearxngBackend<'_> {
    fn name(&self) -> &'static str {
        SEARXNG
    }

    fn fetch(&self, query: &str, options: &SearchOptions) -> Result<Value, SearchError> {
        let base_url = self.base_url;
        let endpoint = format!("{}/search", base_url.trim_end_matches('/'));
        let time_range = options.time_range.as_deref().unwrap_or_default();
        let mut request = agent(None)
            .get(&endpoint)
            .query("q", query)
            .query("format", "json")
            .query("categories", options.searxng_category())
            .query("language", &options.language);
        if !time_range.is_empty() {
            request = request.query("time_range", time_range);
        }
        let response = request.set("Accept", "application/json").call();
        let body = match response {
            Ok(response) => response.into_string(),
            Err(ureq::Error::Status(status, response)) => {
                return Err(SearchError {
                    error: format!(
                        "SearXNG at {base_url} answered HTTP {status} {}",
                        response.status_text()
                    ),
                    hint: if status == 403 {
                        Some(JSON_FORMAT_HINT.to_string())
                    } else {
                        Some(UNAVAILABLE_HINT.to_string())
                    },
                });
            }
            Err(error) => {
                return Err(SearchError {
                    error: format!("SearXNG at {base_url} is unreachable: {error}"),
                    hint: Some(start_hint(self.node)),
                });
            }
        };
        body.ok()
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .filter(Value::is_object)
            .ok_or_else(|| SearchError {
                error: format!("SearXNG at {base_url} did not return JSON"),
                hint: Some(JSON_FORMAT_HINT.to_string()),
            })
    }
}

/// The names under `[search] backends` in a config file's text, in order; empty when the table
/// or the key is missing. Only a one-line array of quoted strings is read.
pub fn backend_names(config: &str) -> Vec<String> {
    let mut in_search = false;
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if let Some(table) = line.strip_prefix('[') {
            in_search = table.trim_end_matches(']').trim() == "search";
        } else if in_search {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if key.trim() != "backends" {
                continue;
            }
            let value = value.trim().trim_start_matches('[').trim_end_matches(']');
            return value
                .split(',')
                .map(|name| {
                    name.trim()
                        .trim_matches(|c| c == '"' || c == '\'')
                        .to_lowercase()
                })
                .filter(|name| !name.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// The backend a search uses: the first name in `names` something implements, else SearXNG.
pub fn choose<'a>(
    names: &[String],
    base_url: &'a str,
    node: Option<&'a Node>,
) -> Box<dyn SearchBackend + 'a> {
    // SearXNG is the only implementation: whatever the names, it answers.
    let _ = names;
    Box::new(SearxngBackend::new(base_url, node))
}

/// [`choose`] with the names from the user's config.
pub fn chosen<'a>(base_url: &'a str, node: Option<&'a Node>) -> Box<dyn SearchBackend + 'a> {
    let text = std::env::var_os("HOME")
        .map(|home| std::path::Path::new(&home).join(".config/dreamference/config.toml"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    choose(&backend_names(&text), base_url, node)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_come_from_the_search_table_only() {
        let text =
            "[other]\nbackends = [\"x\"]\n[search]\n# c\nbackends = [\"SearXNG\", 'brave'] # t\n";
        assert_eq!(backend_names(text), vec!["searxng", "brave"]);
        assert!(backend_names("[server]\nport = 1\n").is_empty());
        assert!(backend_names("").is_empty());
    }

    #[test]
    fn no_name_or_an_unknown_one_still_means_searxng() {
        for names in [vec![], vec!["nothing".to_string()]] {
            assert_eq!(choose(&names, "http://x", None).name(), SEARXNG);
        }
    }
}
