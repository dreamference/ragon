"""The gate's answers for an upstream Codex (specs/DREAMFERENCE_AUTH.md §5.9): list, headers, stubs, CORS."""

import asyncio
import base64
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.error import HTTPError
from urllib.request import Request, urlopen

import pytest

from dreamference.vllm_server.model_gate_compat import ModelGateCompat as Compat
from dreamference.vllm_server.model_gate_service import ModelGateService

SERVED = "qwen/served"
LIST = {
    "object": "list",
    "data": [
        {"id": SERVED, "object": "model", "created": 7, "owned_by": "vllm", "max_model_len": 4096}
    ],
}


def test_the_api_list_gains_the_aliases_with_the_served_created_time():
    answer = Compat.models(LIST, "/v1/models")
    ids = [entry["id"] for entry in answer["data"]]
    assert ids == [SERVED, "ragon", "ragon-default", "gpt-6-astra"]
    alias = answer["data"][1]
    assert alias["owned_by"] == "ragon" and alias["created"] == 7
    assert answer["data"][0]["owned_by"] == "vllm"


def test_codex_asks_with_client_version_and_gets_model_infos():
    answer = Compat.models(LIST, "/v1/models?client_version=0.158.0")
    slugs = [info["slug"] for info in answer["models"]]
    assert slugs[0] == SERVED and "ragon" in slugs
    first = answer["models"][0]
    assert first["context_window"] == 4096 and first["visibility"] == "list"
    assert {"shell_type", "truncation_policy", "supported_reasoning_levels"} <= set(first)


def test_an_alias_runs_the_served_model_and_an_unknown_model_is_not_found():
    ids = [SERVED]
    assert Compat.alias_target("ragon", ids) == SERVED
    assert Compat.alias_target(SERVED, ids) is None
    assert Compat.unknown("nope", ids) and not Compat.unknown("ragon", ids)
    assert not Compat.unknown("nope", [])
    error = Compat.unknown_model("nope")["error"]
    assert error["code"] == "model_not_found" and error["param"] == "model"


def test_parameters_the_engine_rejects_are_removed_with_a_note():
    body = json.dumps(
        {
            "model": "m",
            "store": False,
            "user": "u",
            "include": ["x"],
            "reasoning": {"effort": "low", "summary": "auto"},
            "input": [],
        }
    ).encode()
    cleaned, note = Compat.sanitize("/v1/responses", body)
    record = json.loads(cleaned)
    assert "store" not in record and "user" not in record and "include" not in record
    assert record["reasoning"] == {"effort": "low"}
    assert "store" in note and "reasoning.summary" in note
    assert Compat.sanitize("/v1/responses", b'{"model": "m"}') == (b'{"model": "m"}', None)
    assert Compat.sanitize("/v1/embeddings", body) == (body, None)
    assert Compat.sanitize("/v1/responses", b"not json") == (b"not json", None)


def test_the_limit_headers_say_nothing_is_reached():
    headers = dict(Compat.limit_headers(now=1000))
    assert headers["x-codex-primary-used-percent"] == "0"
    assert headers["x-codex-secondary-used-percent"] == "0"
    assert headers["x-codex-primary-reset-at"] == str(1000 + 300 * 60)
    assert (
        headers["x-codex-credits-has-credits"] == "true"
        and headers["x-codex-credits-unlimited"] == "true"
    )
    assert headers["x-codex-active-limit"] == "codex"
    assert headers["x-request-id"] == headers["x-oai-request-id"]


def test_head_with_does_not_repeat_a_header_the_engine_sent():
    head = b"HTTP/1.1 200 OK\r\nX-Request-Id: engine\r\n\r\n"
    out = Compat.head_with(head, [("x-request-id", "gate"), ("x-a", "1")])
    assert out.count(b"equest-") == 1 and b"x-a: 1\r\n" in out and out.endswith(b"\r\n\r\n")
    assert Compat.head_with(head, []) == head


def jwt(claims):
    part = base64.urlsafe_b64encode(json.dumps(claims).encode()).decode().rstrip("=")
    return f"Bearer h.{part}.s"


def test_the_stubs_answer_with_the_signed_in_user_and_the_rest_is_unknown_url():
    bearer = jwt({"sub": "u1", "preferred_username": "ada", "role": "admin"})
    status, body = Compat.backend("/backend-api/accounts/check", bearer)
    entry = body["accounts"][0]
    assert status == 200 and body["default_account_id"] == "u1"
    assert entry["id"] == entry["account_id"] == "u1" and entry["name"] == "ada"
    assert entry["workspace_backend_origin"] == entry["account_routing_override"] == "NO_CONSTRAINT"
    assert "plan_type" not in entry
    assert Compat.backend("/backend-api/profiles/me", bearer)[1]["username"] == "ada"
    assert Compat.backend("/backend-api/wham/usage")[1]["credits"]["unlimited"] is True
    assert Compat.backend("/backend-api/settings/user") == (200, {})
    assert Compat.backend("/backend-api/workspace-messages")[1]["items"] == []
    assert Compat.backend("/backend-api/wham/tasks/abc")[0] == 404
    status, body = Compat.backend("/backend-api/nothing/here?x=1")
    assert status == 404 and body["error"]["code"] == "unknown_url"
    assert Compat.user_from("Bearer garbage")["username"] == "local"


def test_cors_is_only_for_the_listed_origins_and_never_a_wildcard(tmp_path):
    (tmp_path / "web-origins.json").write_text(json.dumps(["https://node.local:3100", "*"]))
    allowed = Compat.origins(str(tmp_path))
    assert allowed == ["app://-", "https://node.local:3100"]
    assert Compat.cors_headers("https://evil.example", allowed) == []
    assert Compat.cors_headers(None, allowed) == []
    ok = dict(Compat.cors_headers("https://node.local:3100", allowed, preflight=True))
    assert (
        ok["Access-Control-Allow-Origin"] == "https://node.local:3100"
        and "Access-Control-Allow-Methods" in ok
    )
    assert Compat.origins(str(tmp_path / "missing")) == ["app://-"]


class Engine(BaseHTTPRequestHandler):
    """Lists one model; echoes a POST body's model and keys."""

    def log_message(self, *args):
        pass

    def do_GET(self):
        data = json.dumps(LIST).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self):
        record = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        data = json.dumps({"model": record["model"], "keys": sorted(record)}).encode()
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


@pytest.fixture
def gate(tmp_path):
    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Engine)
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    (tmp_path / "web-origins.json").write_text(json.dumps(["https://node.local:3100"]))
    loop = asyncio.new_event_loop()
    service = ModelGateService(
        ("127.0.0.1", 0), ("127.0.0.1", upstream.server_address[1]), str(tmp_path)
    )
    port = loop.run_until_complete(service.start())
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()
    yield f"http://127.0.0.1:{port}"
    loop.call_soon_threadsafe(loop.stop)
    thread.join(5)
    upstream.shutdown()
    upstream.server_close()


def call(url, method="GET", body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    request = Request(
        url,
        data=data,
        method=method,
        headers={"Content-Type": "application/json", **(headers or {})},
    )
    try:
        with urlopen(request, timeout=10) as answer:
            return answer.status, dict(answer.headers), answer.read()
    except HTTPError as error:
        return error.code, dict(error.headers), error.read()


def test_the_live_gate_lists_aliases_in_both_shapes(gate):
    _, _, raw = call(gate + "/v1/models")
    assert "ragon-default" in [entry["id"] for entry in json.loads(raw)["data"]]
    _, _, raw = call(gate + "/v1/models?client_version=1.2.3")
    assert json.loads(raw)["models"][0]["slug"] == SERVED


def test_the_live_gate_maps_an_alias_drops_parameters_and_adds_headers(gate):
    status, headers, raw = call(
        gate + "/v1/responses", "POST", {"model": "ragon", "store": True, "input": []}
    )
    assert status == 200
    answer = json.loads(raw)
    assert answer["model"] == SERVED and "store" not in answer["keys"]
    lowered = {name.lower(): value for name, value in headers.items()}
    assert lowered["x-codex-primary-used-percent"] == "0" and "x-request-id" in lowered


def test_the_live_gate_answers_an_unknown_model_404_in_the_vendors_shape(gate):
    status, _, raw = call(gate + "/v1/responses", "POST", {"model": "nope", "input": []})
    assert status == 404 and json.loads(raw)["error"]["code"] == "model_not_found"


def test_the_live_gate_answers_backend_api_itself(gate):
    status, _, raw = call(gate + "/backend-api/wham/usage")
    assert status == 200 and json.loads(raw)["credits"]["unlimited"] is True
    status, _, raw = call(gate + "/backend-api/other")
    assert status == 404 and json.loads(raw)["error"]["code"] == "unknown_url"


def test_the_live_gate_allows_cors_for_a_listed_origin_only(gate):
    status, headers, _ = call(
        gate + "/v1/models", "OPTIONS", headers={"Origin": "https://node.local:3100"}
    )
    assert status == 204 and headers["Access-Control-Allow-Origin"] == "https://node.local:3100"
    _, headers, _ = call(gate + "/v1/models", "OPTIONS", headers={"Origin": "https://evil.example"})
    assert "Access-Control-Allow-Origin" not in headers
    _, headers, _ = call(gate + "/backend-api/wham/usage", headers={"Origin": "app://-"})
    assert headers["Access-Control-Allow-Origin"] == "app://-"
