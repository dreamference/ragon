"""`dreamference.__version__` is the installed release, in the release's spelling (w48s F4).

The release workflow stamps only setup.py; a literal in `dreamference/__init__.py` stayed 0.9.0 on
0.9.9 rc.1 nodes (the advertisement, `ragon node list`, the serve summary). These tests run the
workflow's own stamp step and check that what pip then installs reads back as the release version.
"""

from __future__ import annotations

import re
import subprocess
from pathlib import Path

import pytest
from packaging.version import Version

import dreamference
from dreamference import release_spelling

REPO = Path(__file__).resolve().parent.parent
RELEASES = ["0.9.9", "0.9.9-rc.1", "0.9.10-rc.12", "1.0.0-dev.42", "0.9.9-alpha.1", "0.9.9-beta.2"]


def _stamp_step() -> str:
    """The `Stamp version` step of release-build.yml, verbatim."""
    workflow = (REPO / ".github" / "workflows" / "release-build.yml").read_text(encoding="utf-8")
    match = re.search(r"- name: Stamp version\n\s+run: (.+)\n", workflow)
    assert match, "release-build.yml has no one-line `Stamp version` step"
    return match.group(1)


def _release_regex() -> str:
    workflow = (REPO / ".github" / "workflows" / "release.yml").read_text(encoding="utf-8")
    match = re.search(r'\[\[ "\$VERSION" =~ (\S+) \]\]', workflow)
    assert match, "release.yml no longer checks the version format"
    return match.group(1)


@pytest.mark.parametrize("release", RELEASES)
def test_the_stamped_wheel_reads_back_as_the_release(release: str, tmp_path: Path) -> None:
    """A wheel stamped by the workflow reads back as exactly the release version.

    Stamp a copy of setup.py as the workflow does; pip installs Version(...) of it; the package
    must read that back as the release, pre-release spelling included.
    """
    assert re.fullmatch(_release_regex(), release)
    (tmp_path / "setup.py").write_text(
        (REPO / "setup.py").read_text(encoding="utf-8"), encoding="utf-8"
    )
    subprocess.run(
        ["bash", "-c", _stamp_step()],
        cwd=tmp_path,
        env={"VERSION": release, "PATH": "/usr/bin:/bin"},
        check=True,
        capture_output=True,
    )
    stamped = re.search(r'^\s*version="([^"]+)"', (tmp_path / "setup.py").read_text(), re.MULTILINE)
    assert stamped and stamped.group(1) == release
    installed = str(Version(stamped.group(1)))  # what importlib.metadata reports after pip install
    assert release_spelling(installed) == release


@pytest.mark.parametrize(
    ("installed", "release"),
    [
        ("0.9.9rc1", "0.9.9-rc.1"),
        ("0.9.9", "0.9.9"),
        ("1.0.0.dev42", "1.0.0-dev.42"),
        ("0.9.9a1", "0.9.9-alpha.1"),
        ("0.9.9b2", "0.9.9-beta.2"),
        ("0.9.9.post1", "0.9.9.post1"),
        ("0.9.9rc1.dev2", "0.9.9rc1.dev2"),
        ("0.9.9+local", "0.9.9+local"),
    ],
)
def test_release_spelling(installed: str, release: str) -> None:
    assert release_spelling(installed) == release


def test_the_package_version_comes_from_the_install(monkeypatch: pytest.MonkeyPatch) -> None:
    """A 0.9.9rc1 wheel says 0.9.9-rc.1, whatever literal the source carries."""
    monkeypatch.setattr(dreamference.metadata, "version", lambda name: "0.9.9rc1")
    assert dreamference._installed_version() == "0.9.9-rc.1"


def test_no_hard_coded_version_reaches_the_node() -> None:
    """`__version__` is computed, not a literal the release forgets to stamp."""
    source = (REPO / "dreamference" / "__init__.py").read_text(encoding="utf-8")
    assert not re.search(r'^__version__\s*=\s*["\']', source, re.MULTILINE)
    from dreamference.node.node_advertiser import NodeAdvertiser

    assert NodeAdvertiser.version() == dreamference.__version__
