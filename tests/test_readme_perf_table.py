"""The README links the published documentation site named by mkdocs.yml."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def _site_url() -> str:
    """Return mkdocs.yml's site_url (a regex: the file holds tags safe_load rejects)."""
    match = re.search(
        r"^site_url:\s*(\S+)\s*$", (ROOT / "mkdocs.yml").read_text(encoding="utf-8"), re.M
    )
    assert match
    return match.group(1)


def test_readme_links_the_pages_site_from_mkdocs_site_url() -> None:
    """README.md contains mkdocs.yml's exact site_url, under the badges and in the install section."""
    site_url = _site_url()
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert site_url
    assert f"**Documentation:** [ragon.dreamference.ai]({site_url})" in readme
    assert f"Read the docs: [ragon.dreamference.ai]({site_url})" in readme
    assert "github.com/dreamference/ragon" in (ROOT / "docs" / "index.md").read_text(
        encoding="utf-8"
    )
