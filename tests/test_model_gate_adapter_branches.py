"""The gate's adapter layer where a body, a request or the engine's answer is not what it expects."""

import asyncio
import json

import pytest

from dreamference.vllm_server.model_gate_adapters import ModelGateAdapters

LISTING = {"base": "org/base", "adapters": {"fix": {"rank": 8}}}


def _read_body(data, framing):
    """The body `read_body` returns, and what it leaves on the stream for the next request."""

    async def read():
        reader = asyncio.StreamReader()
        reader.feed_data(data)
        reader.feed_eof()
        body = await ModelGateAdapters.read_body(reader, framing, 1024)
        return body, await reader.read()

    return asyncio.run(read())


@pytest.fixture
def adapters(tmp_path):
    return ModelGateAdapters(str(tmp_path), ("127.0.0.1", 1))


# -- read_body -----------------------------------------------------------------------------------------


def test_a_request_without_a_body_is_read_as_empty_without_touching_the_stream():
    assert _read_body(b"GET /next HTTP/1.1\r\n", ("none", 0)) == (b"", b"GET /next HTTP/1.1\r\n")


def test_a_chunked_bodys_trailers_are_read_past_and_left_out():
    data = b"3\r\nabc\r\n0\r\nX-Trailer: 1\r\n\r\nGET /next"

    assert _read_body(data, ("chunked", 0)) == (b"abc", b"GET /next")


# -- rewrite -------------------------------------------------------------------------------------------


def test_a_body_that_is_not_a_json_object_goes_to_the_engine_as_it_came(adapters):
    for body in (b"not json", b'["org/base+fix"]'):
        assert asyncio.run(adapters.rewrite(body, LISTING)) == (None, body, None)


def test_a_request_for_the_base_goes_through_unchanged(adapters):
    body = json.dumps({"model": "org/base", "messages": []}).encode()

    assert asyncio.run(adapters.rewrite(body, LISTING)) == (None, body, None)


# -- models --------------------------------------------------------------------------------------------


def test_an_engine_list_that_cannot_be_read_lists_no_models():
    for body in (b"<html>", b"[]"):
        assert ModelGateAdapters.models(body, LISTING) == {"object": "list", "data": []}


def test_adapters_are_not_listed_when_the_engine_does_not_list_their_base():
    body = json.dumps({"object": "list", "data": [{"id": "org/other"}]}).encode()

    assert ModelGateAdapters.models(body, LISTING)["data"] == [{"id": "org/other"}]


# -- sync ----------------------------------------------------------------------------------------------


def test_an_engine_that_does_not_answer_leaves_the_adapters_unsynced(adapters, monkeypatch):
    calls = []

    async def engine_call(method, path, payload):
        calls.append((method, path))
        return 0, b""

    monkeypatch.setattr(adapters, "engine_call", engine_call)

    asyncio.run(adapters.sync(LISTING))

    assert calls == [("GET", "/v1/models")]
    assert adapters.synced is False
    assert adapters.loaded == {}


# -- the engine's answer -------------------------------------------------------------------------------


def test_a_chunked_answer_is_joined_and_stops_at_its_last_chunk():
    answer = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nab\r\n1\r\nc\r\n0\r\n\r\n2\r\nzz\r\n"

    assert ModelGateAdapters.parse_response(answer) == (200, b"abc")


def test_a_chunk_size_that_is_not_hex_ends_the_body():
    assert ModelGateAdapters.dechunk(b"2\r\nab\r\nzz\r\ncd\r\n") == b"ab"


# -- ragon-serve and the gate [key]

RAGON = {**LISTING, "engine": "ragon"}


def test_a_ragon_serve_request_keeps_its_id_and_swaps_nothing_back(adapters):
    adapters.loaded["fix"] = True
    adapters.synced = True
    body = json.dumps({"model": "org/base+fix", "messages": []}).encode()

    refusal, sent, swap = asyncio.run(adapters.rewrite(body, RAGON))

    assert refusal is None and swap is None
    assert json.loads(sent)["model"] == "org/base+fix"


def test_the_separator_follows_the_engine_in_the_list():
    assert ModelGateAdapters.separator(RAGON) == "+"
    assert ModelGateAdapters.separator(LISTING) == ":"
    assert ModelGateAdapters.engine_model("org/base+fix", "+") == "org/base+fix"
    assert ModelGateAdapters.engine_model("org/base+fix") == "org/base:fix"


def test_ragon_serves_own_adapter_entries_are_read_and_hidden_from_clients():
    body = json.dumps({"data": [{"id": "org/base"}, {"id": "org/base+fix"}]}).encode()

    assert ModelGateAdapters.engine_adapters(body, "+") == ["fix"]
    shown = ModelGateAdapters.models(body, RAGON)["data"]
    # The engine's own entry is dropped; the one shown is the gate's, from the verified list.
    assert [entry["id"] for entry in shown] == ["org/base", "org/base+fix"]
    assert "ragon_adapter" in shown[1]


def test_a_load_the_engine_answers_with_success_false_is_a_refusal_not_the_base(adapters):
    calls = []

    async def engine_call(method, path, payload):
        calls.append(path)
        return 200, b'{"success": false, "message": "rank too large"}'

    adapters.engine_call = engine_call
    adapters.synced = True
    listing = {**RAGON, "adapters": {"fix": {"path": "/x", "files": {}}}}
    body = json.dumps({"model": "org/base+fix"}).encode()

    refusal, _, _ = asyncio.run(adapters.rewrite(body, listing))

    assert refusal is not None and refusal[0] == 503
    assert refusal[1]["error"]["code"] == "adapter_unavailable"
    assert "rank too large" in refusal[1]["error"]["message"]
    assert "fix" not in adapters.loaded


def test_load_and_unload_routes_stay_refused_on_the_public_port():
    from dreamference.vllm_server.model_gate_adapters import REFUSED_PATHS

    assert "/load_lora_adapter" in REFUSED_PATHS and "/unload_lora_adapter" in REFUSED_PATHS
