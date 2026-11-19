"""The vendor's Python SDK, unmodified, against the gate over TLS (specs/DREAMFERENCE_AUTH.md §18).

The SDK is the `sdk` test extra and is never in the node venv, so the file skips without it. The
gate runs on a scratch loopback port with the node's CA, in front of the mock engine and a real
accounts service; the SDK trusts the CA and sends a per-user `sk-ragon-` key.
"""

import asyncio
import json
import ssl
import sys
import threading
from pathlib import Path

import pytest

openai = pytest.importorskip("openai")
httpx = pytest.importorskip("httpx")

sys.path.insert(0, str(Path(__file__).parent / "fixtures"))
from mock_engine import MODEL, REPLY, MockEngine  # noqa: E402

from dreamference.accounts import AccountsService, AccountsStore  # noqa: E402
from dreamference.vllm_server.model_gate_service import ModelGateService  # noqa: E402
from dreamference.vllm_server.model_gate_tls import ModelGateTls  # noqa: E402


@pytest.fixture
def gate(tmp_path, monkeypatch):
    lines: list[str] = []
    monkeypatch.setattr(ModelGateService, "log", classmethod(lambda cls, line: lines.append(line)))
    engine = MockEngine().start()
    folder = ModelGateTls.ensure(tmp_path / "tls", ["localhost"])
    state = tmp_path / "gate"
    state.mkdir()
    assert ModelGateTls.stage(folder, state)
    (state / "node.json").write_text(
        json.dumps({"node_id": "id-1", "name": "box", "primary_id": None})
    )
    store = AccountsStore(tmp_path / "a" / "accounts.db", primary_name="box", audit_sink=[].append)
    store.create_user("t-alice", "correct horse battery staple")
    key = store.mint_key("t-alice")
    accounts = AccountsService.server(AccountsService.build(store), port=0)
    threading.Thread(target=accounts.serve_forever, daemon=True).start()
    loop = asyncio.new_event_loop()
    service = ModelGateService(
        ("127.0.0.1", 0),
        ("127.0.0.1", engine.port),
        str(state),
        accounts=("127.0.0.1", accounts.server_address[1]),
        tls_dir=str(state / "tls"),
    )
    port = loop.run_until_complete(service.start())
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()

    def enforce(value: bool) -> None:
        (state / "auth.json").write_text(json.dumps({"enforce": value}))
        service.auth.cache.clear()

    def client(api_key: str | None) -> "openai.OpenAI":
        return openai.OpenAI(
            base_url=f"https://localhost:{port}/v1",
            # The SDK insists on a key; the gate sees none when the header is stripped.
            api_key=api_key or "unused",
            max_retries=0,
            timeout=10,
            http_client=httpx.Client(
                verify=ssl.create_default_context(cafile=str(folder / "ca.pem")),
                event_hooks={"request": [] if api_key else [strip_authorization]},
            ),
        )

    yield {"client": client, "key": key, "enforce": enforce, "engine": engine, "service": service}
    loop.call_soon_threadsafe(service.server.close)
    loop.call_soon_threadsafe(loop.stop)
    thread.join(10)
    for server in (accounts,):
        server.shutdown()
        server.server_close()
    engine.stop()


def strip_authorization(request) -> None:
    request.headers.pop("Authorization", None)


def exercise(client) -> None:
    """Models, one response, one stream and one error, each parsed by the SDK."""
    assert MODEL in [model.id for model in client.models.list().data]
    reply = client.chat.completions.create(
        model=MODEL, messages=[{"role": "user", "content": "ping"}]
    )
    assert reply.choices[0].message.content == REPLY
    stream = client.chat.completions.create(
        model=MODEL, messages=[{"role": "user", "content": "ping"}], stream=True
    )
    assert (
        "".join(chunk.choices[0].delta.content or "" for chunk in stream if chunk.choices) == REPLY
    )
    with pytest.raises(openai.NotFoundError) as error:
        client.chat.completions.create(model="nope", messages=[{"role": "user", "content": "x"}])
    assert "`nope` does not exist" in str(error.value)  # the gate words the model error itself
    assert error.value.code == "model_not_found"


@pytest.mark.parametrize("mode", [False, True])
def test_the_sdk_with_a_per_user_key_is_served_over_tls(gate, mode):
    gate["enforce"](mode)
    exercise(gate["client"](gate["key"]))
    assert gate["service"].auth.unauthenticated == {}


def test_the_sdk_with_no_key_is_served_and_counted_when_not_enforcing(gate):
    gate["enforce"](False)
    exercise(gate["client"](None))
    counts = gate["service"].auth.unauthenticated
    assert sum(counts.values()) >= 3  # the open model list is not counted; the three posts are


def test_the_sdk_with_no_key_is_refused_when_enforcing(gate):
    gate["enforce"](True)
    client = gate["client"](None)
    assert MODEL in [model.id for model in client.models.list().data]  # open route
    seen = len(gate["engine"].seen)
    with pytest.raises(openai.AuthenticationError) as error:
        client.chat.completions.create(model=MODEL, messages=[{"role": "user", "content": "x"}])
    assert error.value.status_code == 401
    assert "ragon login" in str(error.value)
    assert len(gate["engine"].seen) == seen


def test_the_sdk_with_a_wrong_key_is_refused_in_both_modes(gate):
    for mode in (False, True):
        gate["enforce"](mode)
        with pytest.raises(openai.AuthenticationError):
            gate["client"]("sk-ragon-nope").chat.completions.create(
                model=MODEL, messages=[{"role": "user", "content": "x"}]
            )
