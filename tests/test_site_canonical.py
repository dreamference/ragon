"""The product site: docs canonical links carry /docs/, and the site root has robots.txt and sitemap.xml."""

from __future__ import annotations

import importlib.util
import re
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pytest

ROOT = Path(__file__).resolve().parent.parent
WEBSITE = ROOT / "website"
SITE = "https://ragon.dreamference.ai"
DOCS = f"{SITE}/docs/"


def _load_builder() -> ModuleType:
    spec = importlib.util.spec_from_file_location(
        "website_builder", WEBSITE / "src" / "website_builder.py"
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


BUILDER = _load_builder().WebsiteBuilder
LANGUAGES = BUILDER.languages()
CODES = [lang["code"] for lang in LANGUAGES]


@pytest.fixture(scope="module")
def built_docs(tmp_path_factory: pytest.TempPathFactory) -> Path:
    """Build the MkDocs site (strict, as the workflow does) into a temporary folder."""
    if importlib.util.find_spec("mkdocs") is None:
        pytest.skip("mkdocs is not installed here; the docs workflow builds the site")
    out = tmp_path_factory.mktemp("docs") / "site"
    result = subprocess.run(
        [sys.executable, "-m", "mkdocs", "build", "--strict", "--site-dir", str(out)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=600,
        check=False,
    )
    assert result.returncode == 0, result.stderr[-2000:]
    return out


def test_mkdocs_site_url_carries_the_docs_path() -> None:
    match = re.search(r"^site_url:\s*(\S+)\s*$", (ROOT / "mkdocs.yml").read_text(), re.M)
    assert match is not None
    assert match.group(1) == DOCS


def test_built_docs_canonical_links_start_with_the_docs_path(built_docs: Path) -> None:
    pages = sorted(built_docs.rglob("*.html"))
    pages = [page for page in pages if page.name != "404.html"]  # the error page has no canonical
    assert len(pages) > 5
    for page in pages:
        found = re.findall(
            r'<link rel="canonical" href="([^"]+)"', page.read_text(encoding="utf-8")
        )
        assert found, page
        assert all(href.startswith(DOCS) for href in found), (page, found)
    start = (built_docs / "index.html").read_text(encoding="utf-8")
    assert f'<link rel="canonical" href="{DOCS}"' in start


def test_built_docs_sitemap_lists_only_docs_addresses(built_docs: Path) -> None:
    locs = re.findall(
        r"<loc>([^<]+)</loc>", (built_docs / "sitemap.xml").read_text(encoding="utf-8")
    )
    assert locs
    assert all(loc.startswith(DOCS) for loc in locs), locs


def test_site_root_has_robots_txt_that_allows_all_and_names_both_sitemaps() -> None:
    robots = (WEBSITE / "robots.txt").read_text(encoding="utf-8")
    assert robots == BUILDER.robots()
    assert "User-agent: *\nAllow: /\n" in robots
    assert "Disallow" not in robots
    assert f"Sitemap: {SITE}/sitemap.xml\n" in robots
    assert f"Sitemap: {DOCS}sitemap.xml\n" in robots


def test_site_root_sitemap_lists_every_language_front_page() -> None:
    sitemap = (WEBSITE / "sitemap.xml").read_text(encoding="utf-8")
    assert sitemap == BUILDER.sitemap(LANGUAGES)
    locs = re.findall(r"<loc>([^<]+)</loc>", sitemap)
    assert sorted(locs) == sorted(f"{SITE}{BUILDER.page_path(code)}" for code in CODES)
    assert len(set(locs)) == len(CODES) == 20
    assert f"{SITE}/" in locs


def test_build_writes_robots_and_sitemap_into_the_root(tmp_path: Path) -> None:
    BUILDER.build(out_dir=tmp_path)
    assert (tmp_path / "robots.txt").read_text(encoding="utf-8") == BUILDER.robots()
    assert (tmp_path / "sitemap.xml").read_text(encoding="utf-8") == BUILDER.sitemap(LANGUAGES)
