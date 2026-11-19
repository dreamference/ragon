"""The nightly compatibility script (specs/DREAMFERENCE_AUTH.md §18 Live), run against a mocked engine."""

import importlib.util
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

SPEC = importlib.util.spec_from_file_location(
    "auth_compat_nightly", Path(__file__).parents[1] / "scripts" / "auth_compat_nightly.py"
)
nightly = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(nightly)

KEY = "sk-test-secret"


class Engine(BaseHTTPRequestHandler):
    """A mocked engine that wants the Bearer key and answers the four SDK calls."""

    def log_message(self, *args):
        pass

    def send(self, status, body, ctype="application/json"):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.headers.get("Authorization") != f"Bearer {KEY}":
            self.send(401, b'{"error":{"message":"no"}}')
            return
        self.send(
            200, json.dumps({"object": "list", "data": [{"id": "m", "object": "model"}]}).encode()
        )

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if body["model"] != "m":
            self.send(404, b'{"error":{"code":"model_not_found"}}')
            return
        if body.get("stream"):
            self.send(200, b'data: {"choices":[]}\n\ndata: [DONE]\n\n', "text/event-stream")
            return
        self.send(200, b'{"choices":[{"message":{"role":"assistant","content":"ok"}}]}')


@pytest.fixture
def engine():
    server = ThreadingHTTPServer(("127.0.0.1", 0), Engine)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield f"http://127.0.0.1:{server.server_address[1]}"
    server.shutdown()
    server.server_close()


def fake_codex(tmp_path, code):
    path = tmp_path / "codex"
    path.write_text(f"#!/bin/sh\n{code}\n")
    path.chmod(0o755)
    return str(path)


def by_name(results):
    return dict(results)


def test_every_http_check_passes_against_the_mocked_engine(engine, tmp_path):
    results = by_name(nightly.run_checks(engine, KEY, "m", fake_codex(tmp_path, "echo ok")))
    assert [results[n] for n in ("models", "response", "stream", "error", "codex-turn")] == [
        None
    ] * 5


def test_a_wrong_key_or_a_failing_codex_is_a_difference(engine, tmp_path):
    bad = by_name(nightly.run_checks(engine, "wrong", "m", fake_codex(tmp_path, "exit 3")))
    assert "status 401" in bad["models"]
    assert bad["codex-turn"] == "exit 3"


def test_main_prints_one_line_per_check_exits_nonzero_and_hides_the_key(
    engine, tmp_path, monkeypatch, capsys
):
    monkeypatch.setenv("AUTH_COMPAT_KEY", KEY)
    code = nightly.main(
        ["--base-url", engine, "--model", "m", "--codex", fake_codex(tmp_path, "exit 1")]
    )
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 6 and code == 1
    assert lines[0] == "PASS models" and lines[-1].startswith("FAIL codex-turn")
    assert KEY not in "\n".join(lines)


def test_main_exits_zero_when_all_pass_with_the_sdk(engine, tmp_path, monkeypatch, capsys):
    pytest.importorskip("openai")
    monkeypatch.setenv("AUTH_COMPAT_KEY", KEY)
    assert (
        nightly.main(
            ["--base-url", engine, "--model", "m", "--codex", fake_codex(tmp_path, "echo ok")]
        )
        == 0
    )
    assert capsys.readouterr().out.count("PASS") == 6


def test_a_missing_key_fails_setup(monkeypatch, capsys):
    monkeypatch.delenv("AUTH_COMPAT_KEY", raising=False)
    assert nightly.main(["--base-url", "http://x", "--model", "m"]) == 1
    assert capsys.readouterr().out.startswith("FAIL setup")
