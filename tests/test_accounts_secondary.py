"""A secondary forwards to its pinned primary and caches the live check (AUTH §7, §10)."""

import hashlib

import pytest

from dreamference.accounts.accounts_secondary import CACHE_LIMIT, UNREACHABLE, AccountsSecondary


class Primary:
    """A scripted primary: records calls, answers by path, can go down."""

    def __init__(self):
        self.calls = []
        self.down = False

    def __call__(self, method, url, headers, body):
        if self.down:
            raise OSError("down")
        path = url.split("://", 1)[1].split("/", 1)[1]
        self.calls.append((method, "/" + path, dict(headers), body))
        if path.startswith(".well-known/jwks"):
            return 200, {"keys": [{"kid": "k"}]}
        if path == "ragon/v1/users/me":
            return 200, {"user_id": "u1", "username": "alice"}
        return 200, {"ok": path}


class Clock:
    def __init__(self):
        self.now = 0.0

    def __call__(self):
        return self.now


@pytest.fixture
def primary():
    return Primary()


@pytest.fixture
def clock():
    return Clock()


@pytest.fixture
def secondary(primary, clock):
    return AccountsSecondary("https://10.0.0.1:8000", transport=primary, clock=clock)


BEARER = {"authorization": "Bearer tok-1"}


@pytest.mark.parametrize(
    "route",
    [
        "/oauth/token",
        "/api/accounts/deviceauth/usercode",
        "/codex/device",
        "/ragon/v1/users",
        "/ragon/v1/users/bob",
        "/ragon/v1/bindings",
        "/ragon/v1/nodes",
    ],
)
def test_routes_are_forwarded(secondary, primary, route):
    status, _headers, body = secondary.handle("POST", route, {}, b"x")
    assert status == 200 and body == {"ok": route.lstrip("/")}
    assert primary.calls[-1][1] == route and primary.calls[-1][3] == b"x"


def test_unknown_route_is_not_forwarded(secondary, primary):
    assert secondary.handle("GET", "/nope", {}, b"")[0] == 404
    assert primary.calls == []


def test_the_setup_question_goes_to_the_primary_without_the_local_mark(secondary, primary):
    """AUTH §8.6: a caller of a secondary is never on the primary, so the mark stays behind."""
    status, _headers, body = secondary.handle(
        "GET", "/ragon/v1/setup", {"x-ragon-local": "1", "accept": "application/json"}, b""
    )
    assert status == 200 and body == {"ok": "ragon/v1/setup"}
    assert primary.calls[-1][1] == "/ragon/v1/setup"
    assert "x-ragon-local" not in primary.calls[-1][2]


def test_only_listed_headers_travel(secondary, primary):
    secondary.handle("POST", "/oauth/token", {"authorization": "Bearer a", "cookie": "c=1"}, b"")
    assert "cookie" not in primary.calls[-1][2]


def test_jwks_is_fetched_once(secondary, primary):
    for _ in range(3):
        assert secondary.handle("GET", "/.well-known/jwks.json", {}, b"")[2]["keys"]
    assert len(primary.calls) == 1


def test_live_check_is_cached_for_sixty_seconds(secondary, primary, clock):
    for _ in range(3):
        assert secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")[0] == 200
    assert len(primary.calls) == 1
    clock.now = 59
    secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")
    assert len(primary.calls) == 1
    clock.now = 61
    secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")
    assert len(primary.calls) == 2


def test_unreachable_forwarded_route_is_503(secondary, primary):
    primary.down = True
    status, _h, body = secondary.handle("POST", "/oauth/token", {}, b"")
    assert status == 503 and UNREACHABLE in body["error"]["message"]


def test_unreachable_uncached_credential_is_401(secondary, primary):
    primary.down = True
    status, _h, body = secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")
    assert status == 401 and body["error"]["message"] == UNREACHABLE


def test_cached_credential_survives_primary_loss_within_ttl(secondary, primary):
    secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")
    primary.down = True
    assert secondary.handle("GET", "/ragon/v1/users/me", BEARER, b"")[0] == 200


def test_no_credential_is_401(secondary):
    assert secondary.handle("GET", "/ragon/v1/users/me", {}, b"")[0] == 401


def test_userinfo_from_live_check(secondary):
    status, _h, body = secondary.handle("GET", "/oauth/userinfo", BEARER, b"")
    assert status == 200 and body["sub"] == "u1" and body["name"] == "alice"


def test_userinfo_unreachable_is_503(secondary, primary):
    primary.down = True
    assert secondary.handle("GET", "/oauth/userinfo", BEARER, b"")[0] == 503


def test_accounts_secondary_live_check_evicts_oldest_bearer_when_cache_is_full(secondary, primary):
    secondary.cache.clear()
    for index in range(CACHE_LIMIT):
        secondary.live_check({"authorization": f"Bearer old-{index}"})
    assert len(secondary.cache) == CACHE_LIMIT
    status, _h, _body = secondary.live_check(BEARER)
    assert status == 200
    assert len(secondary.cache) == CACHE_LIMIT
    assert hashlib.sha256(b"old-0").hexdigest() not in secondary.cache
    assert hashlib.sha256(b"tok-1").hexdigest() in secondary.cache


def test_accounts_secondary_handle_refuses_a_secret_in_the_query(secondary, primary):
    status, headers, body = secondary.handle("GET", "/oauth/userinfo?access_token=x", {}, b"")
    assert status == 401
    assert body["error"]["code"] == "invalid_api_key"
    assert "WWW-Authenticate" in headers
    assert primary.calls == []


# ----- the pinned CA over a real handshake (the w48s test pair, 11-24) -----------------------


@pytest.fixture
def node_gate(tmp_path):
    """A node's gate on an address its certificate does not name, serving its CA and `me`.

    The advertisement gives an address, the node certificate names only DNS names and loopback:
    127.0.0.2 stands for the LAN address a secondary dials.
    """
    import json
    import ssl
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

    from dreamference.vllm_server.model_gate_tls import (
        CA_CERT,
        SERVER_CERT,
        SERVER_KEY,
        ModelGateTls,
    )

    folder = ModelGateTls.ensure(tmp_path / "tls", ["node.example"])
    ca = (folder / CA_CERT).read_bytes()

    class Gate(BaseHTTPRequestHandler):
        def log_message(self, message_format, *args):
            return

        def do_GET(self):
            data = (
                ca if self.path == "/ragon/v1/ca.pem" else json.dumps({"username": "ann"}).encode()
            )
            self.send_response(200)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    try:
        server = ThreadingHTTPServer(("127.0.0.2", 0), Gate)
    except OSError:
        pytest.skip("127.0.0.2 is not a loopback address here")
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(folder / SERVER_CERT, folder / SERVER_KEY)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    yield {"port": server.server_address[1], "ca": ca, "tmp": tmp_path}
    server.shutdown()
    server.server_close()


def test_the_ca_is_fetched_and_then_verifies_the_primary_by_its_address(node_gate):
    pem = AccountsSecondary.fetch_ca("127.0.0.2", node_gate["port"])
    assert pem == node_gate["ca"]
    ca_file = node_gate["tmp"] / "ca.pem"
    ca_file.write_bytes(pem)
    secondary = AccountsSecondary(
        AccountsSecondary.gate_url("127.0.0.2", node_gate["port"]), ca_file
    )
    status, _headers, body = secondary.live_check({"authorization": "Bearer k"})
    assert (status, body["username"]) == (200, "ann")


def test_another_nodes_ca_does_not_verify_the_primary(node_gate):
    from dreamference.vllm_server.model_gate_tls import CA_CERT, ModelGateTls

    other = ModelGateTls.ensure(node_gate["tmp"] / "other", ["other.example"]) / CA_CERT
    secondary = AccountsSecondary(AccountsSecondary.gate_url("127.0.0.2", node_gate["port"]), other)
    status, _headers, body = secondary.live_check({"authorization": "Bearer k"})
    assert (status, body["error"]["message"]) == (401, UNREACHABLE)


def test_gate_url_brackets_an_ipv6_address():
    assert AccountsSecondary.gate_url("fe80::1%eth0", "8443") == "https://[fe80::1%25eth0]:8443"
    assert AccountsSecondary.gate_url("192.0.2.7", None) == "https://192.0.2.7:8000"
