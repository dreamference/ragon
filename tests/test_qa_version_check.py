"""The QA scripts accept the published `--version` line of a four-part release.

v0.9.8.1 was published printing Cargo's stamp, `ragon 0.9.8+1`, so the macOS, Linux and Windows
QA failed a release that installed and updated correctly. Until every published release prints
its own number, a check for `0.9.8.1` also accepts `0.9.8+1`, and nothing looser.
"""

import os
import re
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SCRIPTS = [ROOT / "scripts" / "qa" / "macos_qa.sh", ROOT / "scripts" / "qa" / "desktop_linux_qa.sh"]
WINDOWS = ROOT / ".github" / "workflows" / "windows-qa.yml"

# `ragon --version | tail -1` of the published v0.9.8.1 binary and of a build that prints the release.
PUBLISHED = "ragon 0.9.8+1"
FIXED = "ragon 0.9.8.1"

ACCEPTED = [
    (PUBLISHED, "0.9.8.1"),
    (FIXED, "0.9.8.1"),
    ("ragon 0.9.8", "0.9.8"),
    ("ragon 0.10.0", "0.10.0"),
]
REFUSED = [
    ("ragon 0.9.8", "0.9.8.1"),
    ("ragon 0.9.8+2", "0.9.8.1"),
    ("ragon 0.9.8+1", "0.9.8"),
    ("ragon 0.9.8.1", "0.9.8"),
    ("ragon 10.9.8+1", "0.9.8.1"),
    ("ragon 0.9.8+1 extra", "0.9.8.1"),
    ("ragon 0x9x8+1", "0.9.8.1"),
    ("", "0.9.8.1"),
    ("absent", "0.9.8.1"),
]


def helper(script: Path) -> str:
    # The scripts are piped to the test machine on standard input, so the function is cut out.
    return re.search(r"^shows_version\(\) \{.*?^\}", script.read_text(), re.S | re.M).group(0)


def shows(script: Path, line: str, want: str) -> bool:
    run = subprocess.run(
        ["bash", "-c", helper(script) + '\nshows_version "$1" "$2"', "_", line, want],
        capture_output=True,
        text=True,
    )
    assert run.stderr == ""
    return run.returncode == 0


@pytest.mark.parametrize("script", SCRIPTS, ids=lambda p: p.name)
@pytest.mark.parametrize(("line", "want"), ACCEPTED)
def test_the_release_and_its_published_form_pass(script, line, want):
    assert shows(script, line, want)


@pytest.mark.parametrize("script", SCRIPTS, ids=lambda p: p.name)
@pytest.mark.parametrize(("line", "want"), REFUSED)
def test_another_version_fails(script, line, want):
    assert not shows(script, line, want)


@pytest.mark.parametrize("script", SCRIPTS, ids=lambda p: p.name)
def test_every_version_check_goes_through_the_helper(script):
    text = script.read_text()
    # No check compares the line with the bare release number any more.
    assert not re.search(r'\$(version|after)" [!=]= \*" \$(latest|previous)"', text)
    assert len(re.findall(r'shows_version "\$(version|after)" "\$(latest|previous)"', text)) >= 2


def test_both_scripts_carry_the_same_rule():
    assert helper(SCRIPTS[0]) == helper(SCRIPTS[1])


def windows_function() -> str:
    text = WINDOWS.read_text()
    block = re.search(r"^( *)function Test-Version\(.*?^\1\}", text, re.S | re.M).group(0)
    return "\n".join(line.strip() for line in block.splitlines())


def test_the_windows_checks_go_through_the_same_rule():
    text = WINDOWS.read_text()
    assert "-notmatch [regex]::Escape($env:" not in text
    assert "if (-not (Test-Version $version $env:PREVIOUS))" in text
    assert "if (-not (Test-Version $version $env:LATEST))" in text
    func = windows_function()
    assert r"'^(\d+\.\d+\.\d+)\.(\d+)$'" in func
    assert '"$($Matches[1])+$($Matches[2])"' in func


@pytest.mark.skipif(shutil.which("pwsh") is None, reason="PowerShell is not installed here")
@pytest.mark.parametrize(
    ("line", "want", "expected"), [(*c, True) for c in ACCEPTED] + [(*c, False) for c in REFUSED]
)
def test_the_windows_rule_in_powershell(line, want, expected, tmp_path):
    script = tmp_path / "check.ps1"
    script.write_text(
        windows_function()
        + "\nif (Test-Version $env:QA_LINE $env:QA_WANT) { 'yes' } else { 'no' }\n"
    )
    env = {**os.environ, "QA_LINE": line, "QA_WANT": want}
    run = subprocess.run(
        ["pwsh", "-NoProfile", "-File", str(script)], capture_output=True, text=True, env=env
    )
    assert run.stdout.strip() == ("yes" if expected else "no")


# The releases page from 11-26 on: the 0.9.9 candidates sit above the latest release, 0.9.8.1.
PAGE = "\n".join(
    f'<a href="/dreamference/ragon/releases/tag/v{tag}">'
    for tag in ["0.9.9-rc.2", "0.9.9-rc.1", "0.9.9-rc.1", "0.9.8.1", "0.9.8", "0.9.7"]
)
PREVIOUS = [
    ("0.9.8.1", "0.9.8"),  # 11-26 to 11-28 the cut tag of the candidates gave 0.9.9, never published
    ("0.9.8", "0.9.7"),
    ("0.9.9", "0.9.8.1"),  # a latest not on the page: the first release that is not it
    ("", "0.9.8.1"),
    ("0.9.7", ""),
]

def test_the_previous_release_skips_the_candidates_above_the_latest_one():
    script = SCRIPTS[0]
    function = re.search(r"^previous_release\(\) \{.*?^\}", script.read_text(), re.S | re.M).group(0)
    for latest, want in PREVIOUS:
        run = subprocess.run(
            ["bash", "-c", function + '\nprevious_release "$1"', "_", latest],
            input=PAGE,
            capture_output=True,
            text=True,
        )
        assert (run.returncode, run.stdout.strip(), run.stderr) == (0, want, ""), latest
