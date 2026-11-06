//! A corpus of saved pages pins what `ragon-fetch` keeps and drops (backlog item 23).

use ragon_web::fetch::page;
use ragon_web::html_text::{JAVASCRIPT_NOTE, html_to_text};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn a_docs_page_keeps_headings_code_tables_and_lists() {
    let text = html_to_text(&fixture("docs_page.html")).text;
    for kept in [
        "# Widget guide",
        "## Install",
        "## Options",
        "```\npip install widget\nwidget --init\n    indented line\n\nafter a blank\n```",
        "| Flag | Meaning |",
        "| --- | --- |",
        "| --fast | Skip checks |",
        "| --quiet | No output |",
        "- First point",
        "- Second point",
        "Install the widget, then run it.",
    ] {
        assert!(text.contains(kept), "{kept:?} missing from:\n{text}");
    }
}

#[test]
fn a_docs_page_loses_its_navigation_and_footer() {
    let page = html_to_text(&fixture("docs_page.html"));
    for dropped in [
        "Site logo",
        "Pricing",
        "Sign in",
        "Sidebar link",
        "Footer Corp",
        "analytics",
    ] {
        assert!(
            !page.text.contains(dropped),
            "{dropped:?} leaked into:\n{}",
            page.text
        );
    }
    assert!(!page.needs_javascript);
    assert_eq!(page.title, "Widget guide");
}

#[test]
fn a_page_drawn_by_javascript_says_so() {
    let fetched = page(
        "u",
        "u".into(),
        200,
        "text/html".into(),
        fixture("spa_page.html").as_bytes(),
        10_000,
    );
    assert!(
        fetched.text.starts_with(JAVASCRIPT_NOTE),
        "{}",
        fetched.text
    );
    assert_eq!(fetched.title, "Dashboard");
}

#[test]
fn a_short_page_with_an_analytics_script_is_not_called_javascript_only() {
    let page = html_to_text(&fixture("short_static_page.html"));
    assert!(!page.needs_javascript);
    assert_eq!(page.text, "Hi\nHello.");
}

#[test]
fn an_article_header_survives_but_a_page_header_does_not() {
    let page = html_to_text(
        "<header>Top bar</header><article><header><h1>Story</h1></header><p>Body</p></article>",
    );
    assert_eq!(page.text, "# Story\nBody");
}
