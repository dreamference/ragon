"""The local canvas page: it serves its HTML and a status JSON, 404s the rest, binds loopback."""

import http.client
import json
import threading

from dreamference import web_canvas
from dreamference.web_canvas import CanvasHandler


def _get(path):
    server = web_canvas.socketserver.TCPServer(("127.0.0.1", 0), CanvasHandler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        conn = http.client.HTTPConnection("127.0.0.1", server.server_address[1], timeout=5)
        conn.request("GET", path)
        response = conn.getresponse()
        return response.status, response.getheader("Content-Type"), response.read()
    finally:
        server.shutdown()
        server.server_close()


def test_the_index_is_the_canvas_page():
    for path in ("/", "/index.html"):
        status, kind, body = _get(path)
        assert status == 200 and kind.startswith("text/html")
        assert body.decode("utf-8") == web_canvas.CANVAS_HTML


def test_the_status_endpoint_reports_hardware_server_and_index(monkeypatch):
    class _Manager:
        def get_server_status(self):
            return {"healthy": True, "host": "h", "models": ["m"]}

    class _Context:
        def __init__(self, loaded):
            self.loaded = loaded

        def load_index(self):
            return self.loaded

        def get_summary(self):
            return {"total_indexed_files": 3}

    monkeypatch.setattr(web_canvas, "detect_gb10_hardware", lambda: {"gpu_name": "gb"})
    monkeypatch.setattr(web_canvas, "VLLMServerManager", _Manager)
    monkeypatch.setattr(web_canvas, "ContextEngine", lambda: _Context(True))
    status, kind, body = _get("/api/status")
    assert status == 200 and kind == "application/json"
    assert json.loads(body) == {
        "hardware": {"gpu_name": "gb"},
        "vllm": {"healthy": True, "host": "h", "models": ["m"]},
        "context": {"total_indexed_files": 3},
    }
    monkeypatch.setattr(web_canvas, "ContextEngine", lambda: _Context(False))
    assert json.loads(_get("/api/status")[2])["context"] == {"total_indexed_files": 0}


def test_any_other_path_is_a_404():
    assert _get("/nope")[0] == 404


def test_the_server_binds_loopback_and_runs_in_a_thread(monkeypatch, capsys):
    bound = []

    class _Server:
        def __init__(self, address, handler):
            bound.append((address, handler))

        def serve_forever(self):
            bound.append("served")

    monkeypatch.setattr(web_canvas.socketserver, "TCPServer", _Server)
    thread = web_canvas.start_web_canvas_server(port=8123)
    thread.join(timeout=5)
    assert bound[0] == (("127.0.0.1", 8123), CanvasHandler) and bound[1] == "served"
    assert "http://localhost:8123" in capsys.readouterr().out
