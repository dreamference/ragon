//! The seam between a search and the engine that answers it.
//!
//! A [`SearchBackend`] turns a query into the raw JSON that [`crate::search::from_searxng`]
//! reads. SearXNG is the default; the owner's URL router is the second implementation, off unless
//! `[search] backends` names it (and `router_url` says where it is). Which backends are used is the user's choice,
//! `[search] backends = ["searxng"]` in `~/.config/dreamference/config.toml`; it is never read
//! from the repository being worked in. A name nothing implements is skipped, and with no usable
//! name the answer is SearXNG, so a config that says nothing changes nothing.

use serde_json::Value;

use crate::agent;
use crate::node_locator::Node;
use crate::search::{JSON_FORMAT_HINT, SearchError, SearchOptions, UNAVAILABLE_HINT, start_hint};

/// The name SearXNG goes by in `[search] backends`.
pub const SEARXNG: &str = "searxng";

/// The name the URL router goes by in `[search] backends`.
pub const ROUTER: &str = "router";

/// The key under which a backend's answer carries what the reader should be told (a fallback).
pub const NOTE_KEY: &str = "ragon_note";

/// The most pages one router answer may have fetched to verify them.
const MAX_VERIFIED: usize = 10;

/// One place a search can be sent.
pub trait SearchBackend {
    /// The name used in `[search] backends`.
    fn name(&self) -> &'static str;

    /// Whether answering needs the network (a loopback instance on this machine counts: it is
    /// the engine's own sockets that reach out). `/airgapped on` refuses every such backend.
    fn uses_network(&self) -> bool {
        true
    }

    /// Runs the query and returns the engine's JSON answer.
    fn fetch(&self, query: &str, options: &SearchOptions) -> Result<Value, SearchError>;
}

/// The refusal for `backend` at `level`: every backend that uses the network is refused at
/// `/airgapped on`, whatever the caller checked before, so a backend added later cannot forget.
pub fn refused_at(level: crate::Level, backend: &dyn SearchBackend) -> Option<SearchError> {
    (level == crate::Level::On && backend.uses_network()).then(|| SearchError {
        error: crate::AIRGAPPED_ON_MESSAGE.to_string(),
        hint: None,
    })
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

/// The value of `key` under `[search]` in a config file's text, unquoted and with any comment
/// cut; `None` when the table or the key is missing.
fn search_value(config: &str, key: &str) -> Option<String> {
    let mut in_search = false;
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if let Some(table) = line.strip_prefix('[') {
            in_search = table.trim_end_matches(']').trim() == "search";
        } else if in_search {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// The names under `[search] backends` in a config file's text, in order; empty when the table
/// or the key is missing. Only a one-line array of quoted strings is read.
pub fn backend_names(config: &str) -> Vec<String> {
    let Some(value) = search_value(config, "backends") else {
        return Vec::new();
    };
    let value = value.trim_start_matches('[').trim_end_matches(']');
    value
        .split(',')
        .map(|name| {
            name.trim()
                .trim_matches(|c| c == '"' || c == '\'')
                .to_lowercase()
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// `[search] router_url` from a config file's text: where the URL router answers.
pub fn router_url(config: &str) -> Option<String> {
    search_value(config, "router_url")
        .map(|value| value.trim_matches(|c| c == '"' || c == '\'').to_string())
        .filter(|url| !url.is_empty())
}

/// The owner's URL router, asked over HTTP. PROVISIONAL: the interface is built to the spec
/// (§8.1) until the owner states the real one. The router answers `GET <router_url>?q=<query>`
/// with `{"results": [{"url", "title", "snippet"}]}`. It never invents a URL: every address it
/// returns is fetched here, and only those that answer are passed on.
pub struct RouterBackend {
    url: Option<String>,
}

impl RouterBackend {
    /// A router at `url`; `None` when `[search] router_url` is not set.
    pub fn new(url: Option<String>) -> Self {
        Self { url }
    }
}

impl SearchBackend for RouterBackend {
    fn name(&self) -> &'static str {
        ROUTER
    }

    fn fetch(&self, query: &str, _options: &SearchOptions) -> Result<Value, SearchError> {
        let fail = |error: String| SearchError { error, hint: None };
        let url = self.url.as_deref().ok_or_else(|| {
            fail("the URL router is on but [search] router_url is not set".to_string())
        })?;
        crate::fetch::require_http_url(url).map_err(fail)?;
        let body = agent(None)
            .get(url)
            .query("q", query)
            .set("Accept", "application/json")
            .call()
            .map_err(|error| fail(format!("the URL router at {url} failed: {error}")))?
            .into_string()
            .map_err(|error| fail(format!("the URL router at {url} failed: {error}")))?;
        let answer: Value = serde_json::from_str(&body)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| fail(format!("the URL router at {url} did not return JSON")))?;
        let returned = answer
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut verified = Vec::new();
        for item in returned.iter().take(MAX_VERIFIED) {
            let Some(address) = item.get("url").and_then(Value::as_str) else {
                continue;
            };
            // A fetch decides: an address that does not answer is not cited.
            if !crate::fetch::fetch(address, 200).is_ok_and(|page| page.status < 400) {
                continue;
            }
            let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
            verified.push(serde_json::json!({
                "title": if text("title").is_empty() { address } else { text("title") },
                "url": address,
                "content": text("snippet"),
                "engine": ROUTER,
            }));
        }
        if verified.is_empty() {
            return Err(fail(format!(
                "the URL router at {url} returned {} address(es), none of which answered",
                returned.len()
            )));
        }
        Ok(serde_json::json!({"query": query, "results": verified}))
    }
}

/// The router first, SearXNG when the router fails or has nothing that verifies; the answer then
/// carries a note that says so, which `ragon-search` prints.
pub struct RouterWithFallback<'a> {
    router: RouterBackend,
    fallback: SearxngBackend<'a>,
}

impl SearchBackend for RouterWithFallback<'_> {
    fn name(&self) -> &'static str {
        ROUTER
    }

    fn fetch(&self, query: &str, options: &SearchOptions) -> Result<Value, SearchError> {
        // The router answers the general web; a news or image question goes to SearXNG as before.
        if options.searxng_category() != "general" {
            return self.fallback.fetch(query, options);
        }
        match self.router.fetch(query, options) {
            Ok(answer) => Ok(answer),
            Err(failure) => {
                let mut answer = self.fallback.fetch(query, options)?;
                answer[NOTE_KEY] =
                    Value::String(format!("{}; answered by SearXNG instead", failure.error));
                Ok(answer)
            }
        }
    }
}

/// The backend a search uses: the first name in `names` something implements, else SearXNG. The
/// router is used only when named; a router that fails falls back to SearXNG.
pub fn choose<'a>(
    names: &[String],
    router_url: Option<String>,
    base_url: &'a str,
    node: Option<&'a Node>,
) -> Box<dyn SearchBackend + 'a> {
    for name in names {
        match name.as_str() {
            SEARXNG => break,
            ROUTER => {
                return Box::new(RouterWithFallback {
                    router: RouterBackend::new(router_url),
                    fallback: SearxngBackend::new(base_url, node),
                });
            }
            _ => {}
        }
    }
    Box::new(SearxngBackend::new(base_url, node))
}

/// [`choose`] with the names and the router address from the user's config.
pub fn chosen<'a>(base_url: &'a str, node: Option<&'a Node>) -> Box<dyn SearchBackend + 'a> {
    let text = std::env::var_os("HOME")
        .map(|home| std::path::Path::new(&home).join(".config/dreamference/config.toml"))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    choose(&backend_names(&text), router_url(&text), base_url, node)
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
            assert_eq!(choose(&names, None, "http://x", None).name(), SEARXNG);
        }
    }

    #[test]
    fn the_router_is_chosen_only_when_named_and_before_searxng() {
        let named =
            |names: &[&str]| -> Vec<String> { names.iter().map(|n| n.to_string()).collect() };
        assert_eq!(
            choose(&named(&["router"]), None, "http://x", None).name(),
            ROUTER
        );
        assert_eq!(
            choose(&named(&["router", "searxng"]), None, "http://x", None).name(),
            ROUTER
        );
        assert_eq!(
            choose(&named(&["searxng", "router"]), None, "http://x", None).name(),
            SEARXNG
        );
        assert_eq!(choose(&[], None, "http://x", None).name(), SEARXNG);
    }

    #[test]
    fn the_router_address_comes_from_the_search_table() {
        let text =
            "[other]\nrouter_url = \"http://no\"\n[search]\nrouter_url = \"http://r:1/q\" # c\n";
        assert_eq!(router_url(text).as_deref(), Some("http://r:1/q"));
        assert_eq!(router_url("[search]\nrouter_url = \"\"\n"), None);
        assert_eq!(router_url(""), None);
    }

    #[test]
    fn a_network_backend_is_refused_at_airgapped_on_and_only_there() {
        let backend = SearxngBackend::new("http://x", None);
        assert!(refused_at(crate::Level::On, &backend).is_some());
        assert!(refused_at(crate::Level::Off, &backend).is_none());
    }
}
