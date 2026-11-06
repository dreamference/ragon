"""`night status` tells a gate from an older install from no gate at all."""

import json
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from conftest import REAL_GATE_IS_STALE

from dreamference.vllm_server.model_gate import ModelGate
from dreamference.vllm_server.model_gate_service import PROBE_PATH


class Answers(BaseHTTPRequestHandler):
    """Answers the probe path as the test's `probe_body` says (None: 404) and /v1/models 200."""

    probe_body: object = None

    def do_GET(self):
        if self.path == PROBE_PATH and self.probe_body is not None:
            body = json.dumps(self.probe_body).encode()
        elif self.path == "/v1/models":
            body = b"{}"
        else:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


@pytest.fixture
def server(monkeypatch):
    monkeypatch.setattr(ModelGate, "is_stale", REAL_GATE_IS_STALE)
    handler = type("H", (Answers,), {})
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    yield handler, f"http://127.0.0.1:{httpd.server_address[1]}"
    httpd.shutdown()
    httpd.server_close()


def container(monkeypatch, running):
    def run(cmd, **kwargs):
        return subprocess.CompletedProcess(
            cmd, 0 if running else 1, "true\n" if running else "", ""
        )

    monkeypatch.setattr(subprocess, "run", run)


def test_a_foreign_gate_name_is_a_stale_gate(server):
    handler, host = server
    handler.probe_body = {"gate": "other"}
    assert ModelGate.is_stale(host)


def test_a_404_with_a_running_gate_container_is_a_stale_gate(server, monkeypatch):
    _, host = server
    container(monkeypatch, True)
    assert ModelGate.is_stale(host)


def test_a_404_without_a_gate_container_is_no_gate(server, monkeypatch):
    _, host = server
    container(monkeypatch, False)
    assert not ModelGate.is_stale(host)


def test_no_answer_is_no_gate(monkeypatch):
    monkeypatch.setattr(ModelGate, "is_stale", REAL_GATE_IS_STALE)
    assert not ModelGate.is_stale("http://127.0.0.1:1", timeout=0.5)


def test_describe_names_each_case(server, monkeypatch):
    _, host = server
    container(monkeypatch, True)
    assert "older install" in ModelGate.describe(host)[-1]
    assert "`ragon-admin server start` replaces it" in ModelGate.describe(host)[-1]
    container(monkeypatch, False)
    assert "No gate answers" in ModelGate.describe(host)[-1]
