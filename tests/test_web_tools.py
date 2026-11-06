"""Tests for the MCP server's web tools: they run `ragon-search` and `ragon-fetch`, here stand-ins."""

import json
import re
import stat
from pathlib import Path

import pytest

from dreamference.mcp_server import web_tools
from dreamference.mcp_server.web_tools import WebTools

REPO = Path(__file__).resolve().parent.parent


@pytest.fixture
def bindir(tmp_path, monkeypatch):
    """A directory that is the whole PATH, and a way to put a stand-in program in it."""
    monkeypatch.setattr(web_tools, "BINARY_DIRS", ())
    monkeypatch.setenv("PATH", str(tmp_path))
    monkeypatch.setattr(WebTools, "airgapped_level", classmethod(lambda cls: "off"))

    def install(name: str, body: str) -> Path:
        path = tmp_path / name
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    install.dir = tmp_path
    return install


def argv_echo(name: str, out: Path) -> str:
    """A script body that records its arguments, one per line, and prints a fixed object."""
    return f'printf "%s\\n" "$@" > {out}\n'


def test_the_tools_pass_on_what_the_programs_print_unchanged(bindir):
    reply = {"query": "q", "result_count": 1, "answers": [], "results": [{"url": "https://a.dev"}]}
    bindir("ragon-search", f"echo '{json.dumps(reply)}'\n")
    assert WebTools.search("q") == reply


def test_search_sends_its_options_as_the_program_takes_them(bindir):
    out = bindir.dir / "argv"
    bindir("ragon-search", argv_echo("s", out) + 'echo "{}"\n')
    WebTools.search("-rf docs", 3, "social", "pt", "week")
    assert out.read_text().split("\n")[:-1] == [
        "--json", "-n", "3", "-c", "social", "-l", "pt", "-t", "week", "--", "-rf docs",
    ]  # fmt: skip
    WebTools.search("x")
    assert "-l" not in out.read_text() and "-t" not in out.read_text()


def test_fetch_clamps_the_limit_and_adds_the_source_to_cite(bindir):
    out = bindir.dir / "argv"
    page = {"url": "u", "final_url": "https://example.org/final", "status": 200, "text": "t"}
    bindir("ragon-fetch", argv_echo("f", out) + f"echo '{json.dumps(page)}'\n")
    outcome = WebTools.fetch("u", 10**9)
    assert outcome == {**page, "source": "https://example.org/final"}
    assert out.read_text().split("\n")[:5] == ["--json", "--max-chars", "100000", "--", "u"]


def test_a_program_that_reports_an_error_has_its_wording_returned_as_it_is(bindir):
    wording = "SearXNG could not reach any search engine: brave: blocked"
    bindir("ragon-search", f"echo '{json.dumps({'error': wording})}'\nexit 1\n")
    assert WebTools.search("q") == {"error": wording}


def test_a_missing_program_is_said_with_what_installs_it(bindir):
    for outcome in (WebTools.search("q"), WebTools.fetch("https://example.org")):
        assert "is not installed" in outcome["error"] and "codex build" in outcome["error"]


def test_a_program_that_prints_nothing_readable_is_said_with_its_exit(bindir):
    bindir("ragon-fetch", "echo oops >&2\nexit 3\n")
    outcome = WebTools.fetch("https://example.org")
    assert outcome["url"] == "https://example.org"
    assert "exit 3" in outcome["error"] and "oops" in outcome["error"]


def test_a_program_that_takes_too_long_is_stopped_and_said(bindir, monkeypatch):
    monkeypatch.setattr(web_tools, "PROCESS_TIMEOUT_S", 0.3)
    bindir("ragon-search", "while :; do :; done\n")
    assert "took longer" in WebTools.search("q")["error"]


def test_at_air_gap_on_or_with_an_empty_query_nothing_is_run(bindir, monkeypatch):
    marker = bindir.dir / "ran"
    for name in ("ragon-search", "ragon-fetch"):
        bindir(name, f"touch {marker}\necho '{{}}'\n")
    assert WebTools.search("   ")["error"] == "empty query"
    monkeypatch.setattr(WebTools, "airgapped_level", classmethod(lambda cls: "on"))
    for outcome in (WebTools.fetch("https://example.org"), WebTools.search("q")):
        assert outcome["airgapped"] == "on" and outcome["error"] == web_tools.AIRGAPPED_ON_MESSAGE
    assert not marker.exists()


def test_a_program_beside_ragon_is_found_when_path_does_not_name_it(bindir, monkeypatch):
    monkeypatch.setenv("PATH", str(bindir.dir / "nowhere"))
    monkeypatch.setattr(web_tools, "BINARY_DIRS", (str(bindir.dir),))
    bindir("ragon-search", "echo '{\"result_count\": 0}'\n")
    assert WebTools.search("q") == {"result_count": 0}


def test_the_fields_the_tools_hand_back_are_the_ones_the_programs_document():
    """Parity pin: the MCP page keys are the program's `--json` keys, plus the `source` to cite."""
    source = REPO / "ragon-web-rs" / "src" / "bin" / "ragon-fetch.rs"
    if not source.is_file():
        pytest.skip("a release install has no ragon-web-rs sources")
    documented = re.search(r"Emit JSON \(([^)]*)\)", source.read_text())
    assert documented, "ragon-fetch's --json help no longer lists its fields"
    fields = {name.strip() for name in documented.group(1).split(",")}
    assert fields == {"url", "final_url", "status", "content_type", "title", "text", "truncated"}
    assert "source" in (web_tools.__doc__ or "") and "source" in WebTools.fetch.__doc__
