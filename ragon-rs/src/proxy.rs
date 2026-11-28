//! Local traffic never goes through a proxy (specs/DREAMFERENCE_RAGON_EGRESS.md §11).
//!
//! An `HTTP_PROXY`, `HTTPS_PROXY` or `ALL_PROXY` left in a shell would also send the requests to
//! the model server and to the loopback services (SearXNG, Gmail, the code index) to that proxy:
//! at best a detour, at worst a refusal that looks like a dead server, or the prompt leaving the
//! machine. `NO_PROXY` is the one channel every client here reads: Codex for each request
//! (`NO_PROXY`, then `no_proxy`: upstream's `http-client/src/outbound_proxy.rs`), `ragon-search`
//! and `ragon-fetch` (`no_proxy`, then `NO_PROXY`), reqwest in the launcher, and Python's
//! `requests` and `urllib` in anything a session runs. Upstream Codex has no per-provider proxy
//! setting, so the variable is the way. Both spellings are set, each to the union of what either
//! held and the local hosts, because the two readers prefer opposite ones.

use std::collections::HashSet;

/// The names this machine answers to, always exempt.
pub const LOOPBACK: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

/// Adds loopback and the model server's host to `NO_PROXY` and `no_proxy`.
///
/// Called from `home::use_ragon_home`, first thing in Codex's `main()`, before any other thread
/// exists: the only point at which setting a variable is sound. The model server is not resolved
/// yet, so the hosts are the ones known without a browse: `DREAMFERENCE_VLLM_HOST` or `vllm_host`,
/// and the remembered node. A node found by a browse for the first time is remembered before Codex
/// starts, so it is exempt from the next start on; the launcher's own requests to it never use a
/// proxy (`direct_client`).
pub fn export() {
    let mut hosts: Vec<String> = Vec::new();
    hosts.extend(crate::configured_vllm_host());
    if let Some(node) = ragon_node_locator::remembered() {
        hosts.extend(node_urls(&node));
        // Its overlay name too (remote access): requests to it must not go to a proxy either.
        if node.overlay_name_is_its_own() {
            hosts.push(node.via_overlay().model_url());
        }
    }
    let upper = std::env::var("NO_PROXY").ok();
    let lower = std::env::var("no_proxy").ok();
    let merged = merged(upper.as_deref(), lower.as_deref(), &hosts);
    // SAFETY: see above; no other thread can be reading the environment yet.
    unsafe {
        std::env::set_var("NO_PROXY", &merged);
        std::env::set_var("no_proxy", &merged);
    }
}

/// The URLs `node` is reached at, which must not go to a proxy: its address, `<name>.local`, the
/// name its HTTPS gate is used by (`auth::https_gate_for`), and its own overlay name (remote access).
pub fn node_urls(node: &ragon_node_locator::Node) -> Vec<String> {
    let mut urls = vec![node.model_url()];
    if crate::auth::is_host_label(&node.name) {
        urls.push(format!("https://{}.local", node.name));
    }
    if node.overlay_name_is_its_own() && node.remote != node.address {
        urls.push(node.via_overlay().model_url());
    }
    urls
}

/// A client for the model server and the loopback services: it never uses a proxy, whatever the
/// environment says, so it also covers a node the environment does not name yet. It trusts the
/// node's pinned CA once [`trust_node_ca`] has named it (a node with accounts serves HTTPS signed
/// by its own CA, AUTH §6).
pub fn direct_client(timeout: std::time::Duration) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().no_proxy().timeout(timeout);
    if let Some(cert) = NODE_CA.get().and_then(|pem| reqwest::Certificate::from_pem(pem).ok()) {
        builder = builder.add_root_certificate(cert);
    }
    builder.build()
}

/// The session node's CA (PEM), set once by `auth::secure_gate` when the session moves to the
/// node's HTTPS gate.
static NODE_CA: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

/// Makes every later [`direct_client`] of this process trust `pem`, the session node's pinned CA.
pub fn trust_node_ca(pem: &[u8]) {
    let _ = NODE_CA.set(pem.to_vec());
}

/// The `NO_PROXY` value: every entry of `upper` and `lower` in their order without repeats, then
/// loopback and the host of each URL in `hosts` that is not already there.
pub fn merged(upper: Option<&str>, lower: Option<&str>, hosts: &[String]) -> String {
    let mut seen = HashSet::new();
    let mut entries: Vec<String> = Vec::new();
    let existing = [upper, lower].into_iter().flatten().flat_map(|value| value.split(','));
    let local = LOOPBACK.iter().map(|host| host.to_string()).chain(hosts.iter().filter_map(|url| host_name(url)));
    for entry in existing.map(|entry| entry.trim().to_string()).chain(local) {
        if !entry.is_empty() && seen.insert(entry.to_ascii_lowercase()) {
            entries.push(entry);
        }
    }
    entries.join(",")
}

/// The host of a URL such as `http://192.168.1.20:8000/v1`, without scheme, port, path or the
/// brackets of an IPv6 address; `None` when there is none.
pub fn host_name(url: &str) -> Option<String> {
    let rest = url.trim();
    let rest = rest.split_once("://").map_or(rest, |(_, after)| after);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    (!host.is_empty()).then(|| host.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_come_out_of_urls() {
        assert_eq!(host_name("http://192.168.1.20:8000/v1").as_deref(), Some("192.168.1.20"));
        assert_eq!(host_name("http://spark.local:8000").as_deref(), Some("spark.local"));
        assert_eq!(host_name("https://[fe80::1]:8000/").as_deref(), Some("fe80::1"));
        assert_eq!(host_name("gb10:8000").as_deref(), Some("gb10"));
        assert_eq!(host_name("http://user:secret@host:1/").as_deref(), Some("host"));
        assert_eq!(host_name(""), None);
    }

    #[test]
    fn existing_entries_stay_first_and_local_hosts_are_added_once() {
        let hosts = vec!["http://192.168.1.20:8000".to_string(), "http://localhost:8000".to_string()];
        assert_eq!(
            merged(Some("corp.example, .internal"), Some("other.example,corp.example"), &hosts),
            "corp.example,.internal,other.example,localhost,127.0.0.1,::1,192.168.1.20"
        );
    }

    #[test]
    fn with_nothing_set_loopback_alone_is_exempt() {
        assert_eq!(merged(None, Some(""), &[]), "localhost,127.0.0.1,::1");
    }

    #[test]
    fn a_second_merge_changes_nothing() {
        let hosts = vec!["http://gb10:8000".to_string()];
        let once = merged(Some("a.example"), None, &hosts);
        assert_eq!(merged(Some(&once), Some(&once), &hosts), once);
    }

    #[test]
    fn loopback_names_are_the_three_local_spellings() {
        assert_eq!(LOOPBACK, ["localhost", "127.0.0.1", "::1"]);
    }

    #[test]
    fn a_url_loses_query_fragment_and_surrounding_space() {
        assert_eq!(host_name("  http://host.example?x=1  ").as_deref(), Some("host.example"));
        assert_eq!(host_name("http://host.example#top").as_deref(), Some("host.example"));
        assert_eq!(host_name("host.example/v1/models").as_deref(), Some("host.example"));
    }

    #[test]
    fn a_url_without_a_host_gives_none() {
        assert_eq!(host_name("http://"), None);
        assert_eq!(host_name("http:///path"), None);
        assert_eq!(host_name("http://:8000"), None);
        assert_eq!(host_name("   "), None);
        assert_eq!(host_name("http://[]:8000"), None);
    }

    #[test]
    fn an_ipv6_literal_loses_its_brackets_and_port() {
        assert_eq!(host_name("http://[::1]:8000/v1").as_deref(), Some("::1"));
        assert_eq!(host_name("[fd00::2]").as_deref(), Some("fd00::2"));
    }

    #[test]
    fn user_info_is_never_a_host() {
        assert_eq!(host_name("http://@host:1").as_deref(), Some("host"));
        assert_eq!(host_name("ftp://a:b@c.example/x").as_deref(), Some("c.example"));
    }

    #[test]
    fn entries_repeat_across_the_two_spellings_only_once_whatever_the_case() {
        assert_eq!(merged(Some("Corp.Example"), Some("corp.example,LOCALHOST"), &[]), "Corp.Example,LOCALHOST,127.0.0.1,::1");
    }

    #[test]
    fn blank_entries_and_stray_commas_are_dropped() {
        assert_eq!(merged(Some(", a.example,, ,"), None, &[]), "a.example,localhost,127.0.0.1,::1");
    }

    #[test]
    fn a_url_with_no_host_adds_nothing() {
        let hosts = vec!["".to_string(), "http://".to_string(), "http://gb10:8000".to_string()];
        assert_eq!(merged(None, None, &hosts), "localhost,127.0.0.1,::1,gb10");
    }

    #[test]
    fn the_same_host_in_two_urls_is_listed_once() {
        let hosts = vec!["http://gb10:8000/v1".to_string(), "https://gb10:9000".to_string()];
        assert_eq!(merged(None, None, &hosts), "localhost,127.0.0.1,::1,gb10");
    }

    #[test]
    fn the_direct_client_builds_for_any_timeout() {
        assert!(direct_client(std::time::Duration::from_millis(1)).is_ok());
        assert!(direct_client(std::time::Duration::from_secs(600)).is_ok());
    }

    #[test]
    fn the_nodes_own_overlay_name_skips_the_proxy_too() {
        let node = ragon_node_locator::Node {
            node: "11111111-aaaa".to_string(),
            address: "192.168.50.105".to_string(),
            model_port: 8000,
            remote: "mightling-11111111-aaaa.netbird.selfhosted".to_string(),
            ..Default::default()
        };
        let urls = node_urls(&node);
        assert!(urls.contains(&"http://mightling-11111111-aaaa.netbird.selfhosted:8000".to_string()), "{urls:?}");
        let stranger = ragon_node_locator::Node { remote: "laptop.netbird.selfhosted".to_string(), ..node };
        assert_eq!(node_urls(&stranger), vec!["http://192.168.50.105:8000".to_string()]);
    }
}
