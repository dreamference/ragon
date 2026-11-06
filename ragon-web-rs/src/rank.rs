//! Re-ranks merged search results: documentation first for a documentation query, and no host
//! filling the top of the list.
//!
//! SearXNG's order stays the base (it already rewards a page several engines agree on). Two
//! adjustments follow, both stable and neither drops a result: a documentation page moves up a
//! few places when the query asks for documentation, and a host's third and later results move
//! behind the first results of other hosts.

use crate::search::SearchResult;

/// Words that mark a query as asking for documentation.
const DOCUMENTATION_WORDS: &[&str] = &[
    "docs",
    "documentation",
    "api",
    "reference",
    "manual",
    "guide",
    "tutorial",
    "usage",
    "how to",
    "syntax",
    "parameters",
    "options",
];

/// Host labels that begin a documentation site (`docs.python.org`, `developer.mozilla.org`).
const DOCUMENTATION_LABELS: &[&str] =
    &["docs", "doc", "developer", "developers", "devdocs", "man7"];

/// Hosts that are documentation without such a label.
const DOCUMENTATION_HOSTS: &[&str] = &[
    "readthedocs.io",
    "readthedocs.org",
    "devdocs.io",
    "man7.org",
    "rust-lang.org",
    "pkg.go.dev",
    "go.dev",
    "kubernetes.io",
    "react.dev",
];

/// Places a documentation page moves up for a documentation query.
const DOCUMENTATION_BOOST: usize = 4;

/// Places it moves up for a query that does not ask for documentation by word, but is not about
/// an error or a release either: a developer's plain "tool topic" query most often wants the manual.
const PLAIN_BOOST: usize = 3;

/// Words of queries whose answer is not documentation: errors and release facts.
const OTHER_WORDS: &[&str] = &[
    "error",
    "exception",
    "traceback",
    "failed",
    "cannot",
    "release",
    "released",
    "changelog",
    "version",
    "latest",
    "news",
    "vs",
    "alternative",
    "alternatives",
    "best",
];

/// Results of one host allowed before its later ones move down.
const PER_HOST: usize = 2;

/// Tells whether the query asks for documentation.
pub fn is_documentation_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    DOCUMENTATION_WORDS.iter().any(|word| {
        if word.contains(' ') {
            lower.contains(word)
        } else {
            words.contains(word)
        }
    })
}

/// Tells whether the query is about an error or a release rather than a topic.
fn is_other_query(query: &str) -> bool {
    let lower = query.to_lowercase();
    lower.split(|c: char| !c.is_alphanumeric()).any(|word| {
        OTHER_WORDS.contains(&word) || word.ends_with("error") || word.ends_with("exception")
    })
}

fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default().to_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// Tells whether a URL is a documentation page.
pub fn is_documentation(url: &str) -> bool {
    let host = host_of(url);
    if host
        .split('.')
        .next()
        .is_some_and(|first| DOCUMENTATION_LABELS.contains(&first))
    {
        return true;
    }
    if DOCUMENTATION_HOSTS
        .iter()
        .any(|known| host == *known || host.ends_with(&format!(".{known}")))
    {
        return true;
    }
    let path = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = path
        .split_once('/')
        .map_or("", |(_, path)| path)
        .to_lowercase();
    path.starts_with("docs/") || path.starts_with("doc/") || path.contains("/docs/")
}

/// Re-ranks `results` for `query`; the same results, in a better order.
pub fn rank(query: &str, results: Vec<SearchResult>) -> Vec<SearchResult> {
    let boost = if is_documentation_query(query) {
        DOCUMENTATION_BOOST
    } else if is_other_query(query) {
        0
    } else {
        PLAIN_BOOST
    };
    let mut keyed: Vec<(usize, SearchResult)> = results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            let boosted = boost > 0 && is_documentation(&result.url);
            // Doubling keeps a boosted page behind an unboosted one that was already ahead of it
            // by the whole boost, and a tie falls back to the original order.
            let place = index * 2;
            (
                if boosted {
                    place.saturating_sub(boost * 2)
                } else {
                    place
                } * 2
                    + usize::from(!boosted),
                result,
            )
        })
        .collect();
    keyed.sort_by_key(|(key, _)| *key);

    let mut kept: Vec<SearchResult> = Vec::new();
    let mut later: Vec<SearchResult> = Vec::new();
    let mut seen: Vec<(String, usize)> = Vec::new();
    for (_, result) in keyed {
        let host = host_of(&result.url);
        match seen.iter_mut().find(|(known, _)| *known == host) {
            Some((_, count)) if *count >= PER_HOST => later.push(result),
            Some((_, count)) => {
                *count += 1;
                kept.push(result);
            }
            None => {
                seen.push((host, 1));
                kept.push(result);
            }
        }
    }
    kept.extend(later);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(url: &str) -> SearchResult {
        SearchResult {
            title: url.to_string(),
            url: url.to_string(),
            snippet: String::new(),
            engine: String::new(),
            engines: Vec::new(),
            published: None,
        }
    }

    fn urls(results: &[SearchResult]) -> Vec<&str> {
        results.iter().map(|r| r.url.as_str()).collect()
    }

    #[test]
    fn a_documentation_query_is_told_by_its_words() {
        assert!(is_documentation_query(
            "python asyncio gather documentation"
        ));
        assert!(is_documentation_query("How to use rsync"));
        assert!(!is_documentation_query(
            "ModuleNotFoundError: No module named 'yaml'"
        ));
        assert!(!is_documentation_query("rapid release notes"));
    }

    #[test]
    fn documentation_pages_are_told_by_host_and_path() {
        assert!(is_documentation(
            "https://docs.python.org/3/library/asyncio.html"
        ));
        assert!(is_documentation(
            "https://developer.mozilla.org/en-US/docs/Web"
        ));
        assert!(is_documentation(
            "https://requests.readthedocs.io/en/latest/"
        ));
        assert!(is_documentation("https://example.com/docs/start"));
        assert!(!is_documentation("https://stackoverflow.com/questions/1"));
        assert!(!is_documentation("https://medium.com/@a/docsish"));
        assert!(!is_documentation("https://discuss.python.org/t/1"));
    }

    #[test]
    fn documentation_moves_up_unless_the_query_is_about_an_error_or_release() {
        let make = || {
            vec![
                result("https://blog.example.com/a"),
                result("https://stackoverflow.com/q/1"),
                result("https://docs.python.org/3/library/asyncio.html"),
            ]
        };
        let ranked = rank("asyncio gather documentation", make());
        assert_eq!(
            ranked[0].url,
            "https://docs.python.org/3/library/asyncio.html"
        );
        let plain = rank("asyncio gather error", make());
        assert_eq!(urls(&plain), urls(&make()));
    }

    #[test]
    fn a_host_fills_no_more_than_two_places_ahead_of_others() {
        let input = vec![
            result("https://a.com/1"),
            result("https://a.com/2"),
            result("https://a.com/3"),
            result("https://b.com/1"),
        ];
        let ranked = rank("anything", input);
        assert_eq!(
            urls(&ranked),
            [
                "https://a.com/1",
                "https://a.com/2",
                "https://b.com/1",
                "https://a.com/3"
            ]
        );
    }

    #[test]
    fn nothing_is_dropped() {
        let input: Vec<SearchResult> = (0..9)
            .map(|i| result(&format!("https://h{}.com/{i}", i % 2)))
            .collect();
        assert_eq!(rank("docs", input).len(), 9);
    }
}
