"""The gate's credential check (specs/DREAMFERENCE_AUTH.md §10): every row of the table, both modes."""

import asyncio
import json
import threading
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from dreamference.vllm_server.model_gate_auth import (
    CACHE_TTL_S,
    INVALID_BODY,
    ModelGateAuth,
)
from dreamference.vllm_server.model_gate_service import ModelGateService

PASSWORD = "pw-alice-1"
INVALID_BYTES = (
    b'{"error": {"message": "Incorrect API key provided.", "type": "invalid_request_error", '
    b'"param": null, "code": "invalid_api_key"}}'
)
SIGN_IN_BYTES = (
    b'{"error": {"message": "Sign in with ragon login.", "type": "invalid_request_error", '
    b'"param": null, "code": "invalid_api_key"}}'
)


class Engine(BaseHTTPRequestHandler):
    """A stand-in engine: streams two chunks, the second after `release` is set."""

    protocol_version = "HTTP/1.1"
    seen: list = []  # noqa: RUF012
    release = threading.Event()

    def log_message(self, *args):
        pass

    def do_POST(self):
        self.rfile.read(int(self.headers.get("Content-Length", 0)))
        Engine.seen.append(self.path)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(b"data: one\n\n")
        self.wfile.flush()
        Engine.release.wait(10)
        self.wfile.write(b"data: two\n\n")

    do_GET = do_POST  # noqa: N815 - the handler's own name


@pytest.fixture
def live(tmp_path, monkeypatch):
    from dreamference.accounts import AccountsService, AccountsStore

    lines: list[str] = []
    monkeypatch.setattr(ModelGateService, "log", classmethod(lambda cls, line: lines.append(line)))
    Engine.seen = []
    Engine.release = threading.Event()
    engine = ThreadingHTTPServer(("127.0.0.1", 0), Engine)
    threading.Thread(target=engine.serve_forever, daemon=True).start()
    store = AccountsStore(tmp_path / "a" / "accounts.db", primary_name="box", audit_sink=[].append)
    accounts = AccountsService.server(AccountsService.build(store), port=0)
    threading.Thread(target=accounts.serve_forever, daemon=True).start()
    state = tmp_path / "gate"
    state.mkdir()
    loop = asyncio.new_event_loop()
    service = ModelGateService(
        ("127.0.0.1", 0),
        ("127.0.0.1", engine.server_address[1]),
        str(state),
        accounts=("127.0.0.1", accounts.server_address[1]),
    )
    port = loop.run_until_complete(service.start())
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()
    yield {"port": port, "state": state, "store": store, "service": service, "lines": lines}
    Engine.release.set()
    loop.call_soon_threadsafe(service.server.close)
    loop.call_soon_threadsafe(loop.stop)
    thread.join(10)
    for server in (accounts, engine):
        server.shutdown()
        server.server_close()


def sign_in(live):
    """t-alice's tokens, as `POST /ragon/v1/users` answers them."""
    body = json.dumps({"username": "t-alice", "password": PASSWORD, "client_id": "c"}).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{live['port']}/ragon/v1/users",
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read())


def revoke_token(live, tokens):
    body = json.dumps({"token": tokens["refresh_token"], "client_id": "c"}).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{live['port']}/oauth/revoke",
        data=body,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    urllib.request.urlopen(request, timeout=10).read()


def call(live, headers=None, path="/v1/chat/completions", method="POST", read=True):
    request = urllib.request.Request(
        f"http://127.0.0.1:{live['port']}{path}",
        data=b"{}" if method == "POST" else None,
        method=method,
        headers=headers or {},
    )
    Engine.release.set()
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            return response.status, response.read() if read else b"", dict(response.headers)
    except urllib.error.HTTPError as error:
        return error.code, error.read(), dict(error.headers)


def enforce(live, value):
    (live["state"] / "auth.json").write_text(json.dumps({"enforce": value}))
    live["service"].auth.cache.clear()


def assert_invalid(result):
    status, body, headers = result
    assert status == 401
    assert body == INVALID_BYTES
    assert headers["WWW-Authenticate"] == 'Bearer realm="ragon"'


@pytest.mark.parametrize("mode", [False, True])
def test_a_valid_token_and_a_valid_key_are_served_with_the_user_logged(live, mode):
    enforce(live, mode)
    tokens = sign_in(live)
    key = live["store"].mint_key("t-alice")
    assert call(live, {"Authorization": f"Bearer {tokens['access_token']}"})[0] == 200
    assert call(live, {"Authorization": f"Bearer {key}"})[0] == 200
    assert call(live, {"api-key": key})[0] == 200
    assert sum("request user=t-alice" in line for line in live["lines"]) == 3


def test_no_credential_is_served_logged_and_counted_when_not_enforcing(live):
    enforce(live, False)
    status, body, _ = call(live, {"User-Agent": "curl/8.0"})
    assert status == 200
    assert body.startswith(b"data: one")
    call(live, {"originator": "ragon"})
    call(live, {"User-Agent": "curl/8.0"})
    assert any("request user=- kind=curl" in line for line in live["lines"])
    counts = live["service"].auth.unauthenticated
    assert sorted(counts.values()) == [1, 2]
    assert {key[1] for key in counts} == {"curl", "ragon"}


def test_no_credential_is_refused_when_enforcing_with_the_sign_in_message(live):
    enforce(live, True)
    status, body, headers = call(live)
    assert (status, body) == (401, SIGN_IN_BYTES)
    assert headers["WWW-Authenticate"] == 'Bearer realm="ragon"'
    assert Engine.seen == []


@pytest.mark.parametrize("mode", [False, True])
def test_a_garbage_or_revoked_credential_is_401_in_both_modes(live, mode):
    enforce(live, mode)
    assert_invalid(call(live, {"Authorization": "Bearer not-a-token"}))
    assert_invalid(call(live, {"Authorization": "Bearer sk-ragon-nope"}))
    assert_invalid(call(live, {"api-key": "sk-ragon-nope"}))
    tokens = sign_in(live)
    revoke_token(live, tokens)
    assert_invalid(call(live, {"Authorization": f"Bearer {tokens['access_token']}"}))
    assert Engine.seen == []


@pytest.mark.parametrize("mode", [False, True])
def test_an_account_header_that_does_not_match_the_token_is_401_and_a_key_ignores_it(live, mode):
    enforce(live, mode)
    tokens = sign_in(live)
    bearer = {"Authorization": f"Bearer {tokens['access_token']}"}
    claims = json.loads(ModelGateAuth.b64url_decode(tokens["access_token"].split(".")[1]))
    account = claims["https://api.openai.com/auth"]["chatgpt_account_id"]
    org = claims["https://ragon.dev/auth"]["org"]
    assert call(live, {**bearer, "ChatGPT-Account-ID": account})[0] == 200
    assert call(live, {**bearer, "OpenAI-Organization": org, "OpenAI-Project": "p"})[0] == 200
    assert_invalid(call(live, {**bearer, "ChatGPT-Account-ID": "someone-else"}))
    assert_invalid(call(live, {**bearer, "OpenAI-Organization": "other-node"}))
    key = live["store"].mint_key("t-alice")
    odd = {"ChatGPT-Account-ID": "x", "OpenAI-Organization": "y"}
    assert call(live, {"Authorization": f"Bearer {key}", **odd})[0] == 200


@pytest.mark.parametrize("mode", [False, True])
def test_a_credential_in_the_url_is_401_and_never_logged(live, mode):
    enforce(live, mode)
    live["store"].create_user("t-alice", PASSWORD)
    key = live["store"].mint_key("t-alice")
    for query in (f"api_key={key}", f"x=1&access_token={key}", f"api-key={key}"):
        assert_invalid(call(live, path=f"/v1/chat/completions?{query}"))
    assert key not in " ".join(live["lines"])
    assert Engine.seen == []


@pytest.mark.parametrize("mode", [False, True])
def test_the_open_routes_need_no_credential(live, mode):
    enforce(live, mode)
    assert call(live, path="/v1/models", method="GET")[0] == 200
    assert call(live, path="/health", method="GET")[0] == 200


def test_the_open_route_status_and_a_url_credential_on_the_model_list(live):
    enforce(live, True)
    assert call(live, path="/v1/models?limit=1", method="GET")[0] == 200


def test_a_secondary_with_the_primary_unreachable_refuses_an_uncached_credential(live):
    tokens = sign_in(live)
    bearer = {"Authorization": f"Bearer {tokens['access_token']}"}
    assert call(live, bearer)[0] == 200  # cached for 60 s
    live["service"].auth.accounts = ("127.0.0.1", 1)
    live["service"].auth.jwks = None
    assert call(live, bearer)[0] == 200  # still cached
    status, body, _ = call(live, {"Authorization": "Bearer sk-ragon-uncached"})
    assert status == 401
    assert json.loads(body)["error"]["message"] == "authentication server unreachable"


def test_the_cache_is_keyed_by_the_hash_and_lasts_sixty_seconds(live):
    tokens = sign_in(live)
    auth = live["service"].auth
    assert CACHE_TTL_S == 60.0
    call(live, {"Authorization": f"Bearer {tokens['access_token']}"})
    assert len(auth.cache) == 1
    (digest,) = auth.cache
    assert len(digest) == 64
    assert tokens["access_token"] not in digest


def test_a_revocation_in_the_middle_of_a_response_lets_it_finish(live):
    tokens = sign_in(live)
    request = urllib.request.Request(
        f"http://127.0.0.1:{live['port']}/v1/chat/completions",
        data=b"{}",
        method="POST",
        headers={"Authorization": f"Bearer {tokens['access_token']}"},
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        assert response.readline() == b"data: one\n"
        response.readline()
        revoke_token(live, tokens)
        Engine.release.set()
        assert response.read() == b"data: two\n\n"


def test_the_helpers_read_the_carriers_and_the_client_kind():
    credential = ModelGateAuth.credential
    assert credential([(b"authorization", b"Bearer abc")]) == "abc"
    assert credential([(b"api-key", b"k")]) == "k"
    assert credential([]) is None
    assert credential([(b"authorization", b"Basic zzz")]) == "Basic zzz"
    assert ModelGateAuth.client_kind([(b"originator", b"ragon_cli")]) == "ragon_cli"
    assert ModelGateAuth.client_kind([(b"user-agent", b"curl/8.0 x")]) == "curl"
    assert ModelGateAuth.client_kind([]) == "unknown"
    assert not ModelGateAuth.gated("GET", "/v1/models")
    assert not ModelGateAuth.gated("POST", "/health")
    assert ModelGateAuth.gated("POST", "/v1/responses")
    assert json.dumps(INVALID_BODY).encode() == INVALID_BYTES


def test_a_forged_or_expired_jwt_is_refused_locally():
    import tempfile
    from pathlib import Path

    from dreamference.accounts import AccountsJwt

    with tempfile.TemporaryDirectory() as folder:
        signer = AccountsJwt(Path(folder) / "k.pem", "https://box.local:8000", "n", "box")
        other = AccountsJwt(Path(folder) / "o.pem", "https://box.local:8000", "n", "box")
        user = {"user_id": "u", "username": "t-alice", "role": "user"}
        good = signer.sign(signer.claims(user, "c", 1, "m"))
        jwks = signer.jwks()
        verify = ModelGateAuth.verify_jwt
        assert verify(good, jwks, "https://box.local:8000") is not None
        assert verify(good, jwks, "https://elsewhere:8000") is None
        assert verify(good, other.jwks(), "https://box.local:8000") is None
        assert verify(good, jwks, "https://box.local:8000", now=10**11) is None
        head, claims, signature = good.split(".")
        assert verify(f"{head}.{claims}", jwks, "https://box.local:8000") is None
        assert (
            verify(f"{head}.{claims}.{signature[:-4]}AAAA", jwks, "https://box.local:8000") is None
        )


def test_model_gate_auth_url_has_credential_flags_only_credential_names_in_the_query():
    has_credential = ModelGateAuth.url_has_credential
    assert has_credential(b"GET /v1/chat/completions?api_key=secret HTTP/1.1\r\nHost: gate\r\n\r\n")
    assert has_credential(
        b"GET /v1/chat/completions?x=1&access_token=secret HTTP/1.1\r\nHost: gate\r\n\r\n"
    )
    assert has_credential(b"GET /v1/chat/completions?API-KEY=secret HTTP/1.1\r\nHost: gate\r\n\r\n")
    assert not has_credential(b"GET /v1/chat/completions?x=1&y=2 HTTP/1.1\r\nHost: gate\r\n\r\n")
    assert not has_credential(b"GET /v1/chat/completions?q=api_key HTTP/1.1\r\nHost: gate\r\n\r\n")
    assert not has_credential(b"GET /v1/chat/completions HTTP/1.1\r\nHost: gate\r\n\r\n")
    assert not has_credential(b"GARBAGE\r\nHost: gate\r\n\r\n")
