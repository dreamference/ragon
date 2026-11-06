//! HTML to readable text, the way `WebTools._html_to_text` does it with BeautifulSoup.
//!
//! The Python version drops every non-content element, then joins all remaining text nodes with a
//! newline and collapses the blank runs that leaves. This does the same over html5ever's tree. The
//! two parsers recover from broken markup differently (html5ever follows the HTML standard,
//! `html.parser` does not), so the text of malformed pages can differ in where a line breaks, not
//! in what the words are.

use std::sync::LazyLock;

use ego_tree::iter::Edge;
use regex::Regex;
use scraper::Html;
use scraper::Node;

/// Everything that carries no prose. Dropped with its whole subtree, so the result reads like the
/// page rather than like its source.
pub const NON_CONTENT_TAGS: [&str; 8] = [
    "script", "style", "noscript", "svg", "canvas", "template", "iframe", "form",
];

/// Page furniture: menus, footers and sidebars. Dropped like the non-content tags. A `<header>` is
/// furniture only outside an `<article>` or `<main>`, where it holds a heading worth keeping.
const BOILERPLATE_TAGS: [&str; 3] = ["nav", "footer", "aside"];

/// The note put before the text of a page whose content is drawn by scripts.
pub const JAVASCRIPT_NOTE: &str = "[This page needs JavaScript to show its content, and ragon-fetch does not run scripts: what follows is all the page holds without it.]";

/// Below this many characters of text a page with scripts and no content is taken to be drawn by them.
const THIN_TEXT_CHARS: usize = 200;

static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());
static BLANK_RUNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n\s*").unwrap());

/// A page reduced to its title and text.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PageText {
    pub title: String,
    pub text: String,
    /// The page holds little but scripts and an empty mount point or a "enable JavaScript" notice.
    pub needs_javascript: bool,
}

fn element_name<'a>(node: &ego_tree::NodeRef<'a, Node>) -> Option<&'a str> {
    match node.value() {
        Node::Element(element) => Some(element.name()),
        _ => None,
    }
}

/// Whether an element is dropped with its subtree.
fn is_dropped(node: &ego_tree::NodeRef<Node>) -> bool {
    let Node::Element(element) = node.value() else {
        return false;
    };
    let name = element.name();
    if NON_CONTENT_TAGS.contains(&name) || BOILERPLATE_TAGS.contains(&name) {
        return true;
    }
    if matches!(
        element.attr("role"),
        Some("navigation" | "banner" | "contentinfo" | "complementary")
    ) {
        // A banner inside the article is its heading block; only page-level ones are furniture.
        return !(element.attr("role") == Some("banner") && inside_article(node));
    }
    name == "header" && !inside_article(node)
}

fn inside_article(node: &ego_tree::NodeRef<Node>) -> bool {
    node.ancestors()
        .any(|a| matches!(element_name(&a), Some("article" | "main")))
}

/// The text under a node with runs of whitespace folded to one space, scripts and styles left out.
fn flat_text(node: ego_tree::NodeRef<Node>) -> String {
    let mut out = String::new();
    let mut skipping = None;
    for edge in node.traverse() {
        match edge {
            Edge::Open(n) if skipping.is_none() => match n.value() {
                Node::Element(e) if NON_CONTENT_TAGS.contains(&e.name()) => skipping = Some(n.id()),
                Node::Text(t) => {
                    out.push_str(t);
                    out.push(' ');
                }
                _ => {}
            },
            Edge::Close(n) if skipping == Some(n.id()) => skipping = None,
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text under a node exactly as written, for a code block.
fn raw_text(node: ego_tree::NodeRef<Node>) -> String {
    let mut out = String::new();
    for n in node.descendants() {
        if let Node::Text(t) = n.value() {
            let hidden = n.ancestors().any(|a| {
                a.id() != node.id()
                    && matches!(element_name(&a), Some(name) if NON_CONTENT_TAGS.contains(&name))
            });
            if !hidden {
                out.push_str(t);
            }
        } else if element_name(&n) == Some("br") {
            out.push('\n');
        }
    }
    out
}

/// A table as pipe-separated rows, with a divider after a header row.
fn table_rows(table: ego_tree::NodeRef<Node>) -> String {
    let mut rows: Vec<String> = Vec::new();
    for tr in table
        .descendants()
        .filter(|n| element_name(n) == Some("tr"))
    {
        let cells: Vec<ego_tree::NodeRef<Node>> = tr
            .children()
            .filter(|c| matches!(element_name(c), Some("td" | "th")))
            .collect();
        if cells.is_empty() {
            continue;
        }
        let texts: Vec<String> = cells.iter().map(|c| flat_text(*c)).collect();
        let header = cells.iter().all(|c| element_name(c) == Some("th"));
        let divider = header && rows.is_empty();
        rows.push(format!("| {} |", texts.join(" | ")));
        if divider {
            rows.push(format!("|{}", " --- |".repeat(cells.len())));
        }
    }
    rows.join("\n")
}

/// One stretch of the page: prose that is whitespace-folded, or a block kept as written.
enum Segment<'a> {
    Prose(Vec<std::borrow::Cow<'a, str>>),
    Block(String),
}

/// Extracts the title and the readable text of an HTML document.
///
/// Headings keep a `#` prefix, list items a `-`, `<pre>` blocks stay verbatim in a fence and tables
/// become pipe rows; menus, footers and sidebars are dropped. A page that is only scripts says so.
pub fn html_to_text(html: &str) -> PageText {
    let document = Html::parse_document(html);
    let mut segments: Vec<Segment> = Vec::new();
    let mut title: Option<String> = None;
    let mut in_title = false;
    let mut prefix: Option<String> = None;
    let mut scripts = 0usize;
    let mut js_notice = false;
    let mut mount_point = false;
    // The node whose subtree is being skipped. Iterative, because a recursive walk overflows the
    // stack on a page nested a few thousand elements deep.
    let mut skipping = None;

    for edge in document.tree.root().traverse() {
        match edge {
            Edge::Open(node) => {
                if skipping.is_some() {
                    continue;
                }
                match node.value() {
                    Node::Element(element) if element.name() == "pre" => {
                        let code = raw_text(node);
                        let code = code.trim_matches('\n');
                        if !code.trim().is_empty() {
                            segments.push(Segment::Block(format!("```\n{code}\n```")));
                        }
                        skipping = Some(node.id());
                    }
                    Node::Element(element) if element.name() == "table" => {
                        let rows = table_rows(node);
                        if !rows.is_empty() {
                            segments.push(Segment::Block(rows));
                        }
                        skipping = Some(node.id());
                    }
                    Node::Element(element) if is_dropped(&node) => {
                        match element.name() {
                            "script" => scripts += 1,
                            "noscript" => {
                                js_notice |= flat_text(node).to_lowercase().contains("javascript");
                            }
                            _ => {}
                        }
                        skipping = Some(node.id());
                    }
                    Node::Element(element) if element.name() == "title" && title.is_none() => {
                        in_title = true;
                        title = Some(String::new());
                    }
                    Node::Element(element) => {
                        if let Some(level) = element
                            .name()
                            .strip_prefix('h')
                            .and_then(|d| d.parse::<usize>().ok())
                            .filter(|l| (1..=6).contains(l))
                        {
                            prefix = Some(format!("{} ", "#".repeat(level)));
                        } else if element.name() == "li" {
                            prefix = Some("- ".to_string());
                        }
                        if matches!(
                            element.attr("id"),
                            Some("root" | "app" | "__next" | "__nuxt" | "___gatsby")
                        ) && !node.children().any(|c| {
                            c.value().is_element()
                                || matches!(c.value(), Node::Text(t) if !t.trim().is_empty())
                        }) {
                            mount_point = true;
                        }
                    }
                    Node::Text(text) => {
                        if in_title {
                            // `get_text(strip=True)`: each piece stripped, joined with nothing.
                            if let Some(title) = title.as_mut() {
                                title.push_str(text.trim());
                            }
                        }
                        let piece: std::borrow::Cow<str> = match prefix.take() {
                            Some(p) if !text.trim().is_empty() => {
                                format!("{p}{}", text.trim_start()).into()
                            }
                            other => {
                                prefix = other;
                                std::borrow::Cow::Borrowed(&**text)
                            }
                        };
                        match segments.last_mut() {
                            Some(Segment::Prose(pieces)) => pieces.push(piece),
                            _ => segments.push(Segment::Prose(vec![piece])),
                        }
                    }
                    _ => {}
                }
            }
            Edge::Close(node) => {
                if skipping == Some(node.id()) {
                    skipping = None;
                } else if in_title
                    && matches!(node.value(), Node::Element(element) if element.name() == "title")
                {
                    in_title = false;
                }
            }
        }
    }

    let parts: Vec<String> = segments
        .into_iter()
        .map(|segment| match segment {
            Segment::Block(block) => block,
            Segment::Prose(pieces) => {
                let text = pieces.join("\n");
                let text = SPACES.replace_all(&text, " ");
                BLANK_RUNS.replace_all(&text, "\n\n").trim().to_string()
            }
        })
        .filter(|part| !part.is_empty())
        .collect();
    let text = parts.join("\n");
    let thin = text.chars().count() < THIN_TEXT_CHARS;
    let needs_javascript = thin && (js_notice || mount_point || (scripts > 0 && text.is_empty()));
    PageText {
        title: title.unwrap_or_default(),
        text,
        needs_javascript,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_non_content_and_keeps_the_title() {
        let page = html_to_text(
            "<html><head><title> The  Title </title><style>p{}</style>\
             <script>var x = 1;</script></head><body><h1>Heading</h1>\
             <p>First &amp; second</p><form><input value=x>Form text</form>\
             <svg><title>icon</title><text>vector</text></svg><p>Last</p></body></html>",
        );
        assert_eq!(page.title, "The  Title");
        assert!(page.text.contains("Heading"));
        assert!(page.text.contains("First & second"));
        for dropped in ["var x", "p{}", "Form text", "icon", "vector"] {
            assert!(
                !page.text.contains(dropped),
                "{dropped:?} leaked into {:?}",
                page.text
            );
        }
        assert!(page.text.ends_with("Last"));
    }

    #[test]
    fn collapses_spaces_and_blank_runs_but_keeps_paragraph_breaks() {
        let page = html_to_text("<p>a \t  b</p>\n\n   \n<p>c</p><p>d</p>");
        assert_eq!(page.text, "a b\n\nc\nd");
    }

    #[test]
    fn a_title_split_by_markup_is_joined_without_separators() {
        let page = html_to_text("<title>A &amp; B</title><p>x</p>");
        assert_eq!(page.title, "A & B");
    }

    #[test]
    fn a_page_without_a_title_has_an_empty_one() {
        assert_eq!(html_to_text("<p>only text</p>").title, "");
    }

    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let html = "<div>".repeat(20_000) + "deep" + &"</div>".repeat(20_000);
        assert_eq!(html_to_text(&html).text, "deep");
    }

    #[test]
    fn comments_and_the_doctype_are_not_text() {
        let page = html_to_text("<!DOCTYPE html><!-- note --><p>body</p>");
        assert_eq!(page.text, "body");
    }
}
