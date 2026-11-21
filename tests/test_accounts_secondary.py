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
