"""The definition sentence of D3 (specs/DREAMFERENCE_AEO.md §1.2) is held as one constant.

The README's first paragraph must be that sentence, word for word.
"""

from pathlib import Path
from typing import Final

REPO: Final[Path] = Path(__file__).resolve().parent.parent
README: Final[Path] = REPO / "README.md"

# D3: the one definition sentence, identical everywhere (site, README, docs, …).
DEFINITION: Final[str] = (
    "Ragon is an open-source coding agent that runs entirely on a desk-side "
    "NVIDIA GB10 machine, with no cloud."
)


def test_readme_definition():
    """The README's first paragraph is the D3 definition sentence."""
    lines = README.read_text(encoding="utf-8").splitlines()
    # The first paragraph is the first non-blank, non-markup line.
    first_para = next(
        line.strip()
        for line in lines
        if line.strip() and not line.strip().startswith(("<", "![", "|", "#"))
    )
    assert first_para == DEFINITION, (
        f"README first paragraph is:\n  {first_para!r}\nexpected:\n  {DEFINITION!r}"
    )


def test_docs_home():
    """site_description and the docs home open with D3; title and product link."""
    mkdocs = (REPO / "mkdocs.yml").read_text(encoding="utf-8")
    assert f"site_description: {DEFINITION}" in mkdocs
    text = (REPO / "docs" / "index.md").read_text(encoding="utf-8")
    front, body = text.split("---\n", 2)[1:]
    assert "title: What is Ragon?" in front
    first_para = next(
        line.strip()
        for line in body.splitlines()
        if line.strip() and not line.strip().startswith(("<", "![", "|", "#"))
    )
    assert first_para.startswith(DEFINITION)
    assert "](https://ragon.dreamference.ai/)" in body
