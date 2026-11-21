"""Device authorization and authorization codes (AUTH §5.4, §5.5), one test per request sequence."""

import json
import urllib.parse

import pytest

from dreamference.accounts import (
    AccountsDevice,
    AccountsForgejoClient,
    AccountsJwt,
    AccountsOauth,
    AccountsService,
    AccountsStore,
)
from dreamference.accounts.accounts_device import CODE_LIFETIME_S, POLL_INTERVAL_S

CODEX_CLIENT = "app_EMoamEEZ73f0CkXaXp7hrann"
JSON_HEADERS = {"content-type": "application/json"}
FORM_HEADERS = {"content-type": "application/x-www-form-urlencoded"}
ISSUER = AccountsService.issuer("box")


class Clock:
    def __init__(self):
        self.now = 1000.0

    def __call__(self):
        return self.now


@pytest.fixture
def clock():
    return Clock()


@pytest.fixture
def store(tmp_path):
    return AccountsStore(tmp_path / "state" / "accounts.db", primary_name="box")


@pytest.fixture
def forgejo():
    return AccountsForgejoClient.register(
        "node-1", ["https://forge.box/user/oauth2/ragon/callback"]
    )


@pytest.fixture
def oauth(store, tmp_path, clock, forgejo):
    key = tmp_path / "state" / "signing-key.pem"
    signer = AccountsJwt(key, ISSUER, "node-1", "box")
    device = AccountsDevice(ISSUER, "https://box.local:3100/device", clock, forgejo[0])
    return AccountsOauth(store, signer, device=device)


@pytest.fixture
def alice(store):
    return store.create_user("t-alice", "alice-pass-1")


def post_json(oauth, path, body):
    return oauth.handle("POST", path, JSON_HEADERS, json.dumps(body).encode())


def post_form(oauth, path, fields):
    return oauth.handle("POST", path, FORM_HEADERS, urllib.parse.urlencode(fields).encode())


def test_codex_device_sequence_matches_device_code_auth_rs(oauth, alice, clock):
    # request_user_code: JSON {client_id}; the answer's interval is a string.
    status, _, first = post_json(
        oauth, "/api/accounts/deviceauth/usercode", {"client_id": CODEX_CLIENT}
    )
    assert status == 200
    assert set(first) == {"device_auth_id", "user_code", "interval"}
    assert first["interval"] == str(POLL_INTERVAL_S)
    assert len(first["user_code"].replace("-", "")) == 8
    poll = {"device_auth_id": first["device_auth_id"], "user_code": first["user_code"]}
    # poll_for_token: 403 while pending is "keep polling".
    assert post_json(oauth, "/api/accounts/deviceauth/token", poll)[0] == 403
    clock.now += POLL_INTERVAL_S
    assert post_json(oauth, "/api/accounts/deviceauth/token", poll)[0] == 403
    # The /device page approves the code.
    assert oauth.device.exists(first["user_code"])
    assert oauth.device.approve(first["user_code"], alice["user_id"], "laptop")
    status, _, done = post_json(oauth, "/api/accounts/deviceauth/token", poll)
    assert status == 200
    assert set(done) == {"authorization_code", "code_challenge", "code_verifier"}
    assert done["code_challenge"] == AccountsDevice.s256(done["code_verifier"])
    # exchange_code_for_tokens: form body to /oauth/token, redirect {issuer}/deviceauth/callback.
    form = {
        "grant_type": "authorization_code",
        "code": done["authorization_code"],
        "redirect_uri": f"{ISSUER}/deviceauth/callback",
        "client_id": CODEX_CLIENT,
        "code_verifier": done["code_verifier"],
    }
    status, _, tokens = post_form(oauth, "/oauth/token", form)
    assert status == 200
    assert {"id_token", "access_token", "refresh_token"} <= set(tokens)
    assert oauth.signer.verify(tokens["access_token"]) is not None
    # The code and the poll row are spent.
    assert post_form(oauth, "/oauth/token", form)[0] == 400
    assert post_json(oauth, "/api/accounts/deviceauth/token", poll)[0] == 404


def test_codex_code_needs_the_right_verifier_and_redirect(oauth, alice):
    first = post_json(oauth, "/api/accounts/deviceauth/usercode", {"client_id": CODEX_CLIENT})[2]
    oauth.device.approve(first["user_code"], alice["user_id"])
    poll = {"device_auth_id": first["device_auth_id"], "user_code": first["user_code"]}
    done = post_json(oauth, "/api/accounts/deviceauth/token", poll)[2]
    form = {
        "grant_type": "authorization_code",
        "code": done["authorization_code"],
        "redirect_uri": f"{ISSUER}/deviceauth/callback",
        "client_id": CODEX_CLIENT,
        "code_verifier": "wrong",
    }
    assert post_form(oauth, "/oauth/token", form)[2]["error"] == "invalid_grant"
    # A wrong attempt spends the code.
    form["code_verifier"] = done["code_verifier"]
    assert post_form(oauth, "/oauth/token", form)[0] == 400


def test_standard_device_sequence(oauth, alice, clock):
    status, _, first = post_form(oauth, "/oauth/device/code", {"client_id": "cli"})
    assert status == 200
    assert first["expires_in"] == CODE_LIFETIME_S == 900
    assert first["interval"] == POLL_INTERVAL_S == 5
    assert first["verification_uri"] == "https://box.local:3100/device"
    assert first["user_code"] in first["verification_uri_complete"]
    grant = {
        "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
        "device_code": first["device_code"],
        "client_id": "cli",
    }
    assert post_form(oauth, "/oauth/token", grant)[2]["error"] == "authorization_pending"
    assert post_form(oauth, "/oauth/token", grant)[2]["error"] == "slow_down"
    clock.now += POLL_INTERVAL_S
    assert post_form(oauth, "/oauth/token", grant)[2]["error"] == "authorization_pending"
    assert oauth.device.approve(first["user_code"].lower(), alice["user_id"])
    clock.now += POLL_INTERVAL_S
    status, _, tokens = post_form(oauth, "/oauth/token", grant)
    assert status == 200 and tokens["token_type"] == "Bearer"
    clock.now += POLL_INTERVAL_S
    assert post_form(oauth, "/oauth/token", grant)[2]["error"] == "expired_token"


def test_codes_expire_after_fifteen_minutes_in_both_shapes(oauth, alice, clock):
    codex = post_json(oauth, "/api/accounts/deviceauth/usercode", {"client_id": "c"})[2]
    std = post_form(oauth, "/oauth/device/code", {"client_id": "c"})[2]
    clock.now += CODE_LIFETIME_S
    poll = {"device_auth_id": codex["device_auth_id"], "user_code": codex["user_code"]}
    assert post_json(oauth, "/api/accounts/deviceauth/token", poll)[0] == 404
    assert not oauth.device.approve(std["user_code"], alice["user_id"])
    grant = {"grant_type": AccountsDevice and "urn:ietf:params:oauth:grant-type:device_code"}
    grant.update(device_code=std["device_code"], client_id="c")
    assert post_form(oauth, "/oauth/token", grant)[2]["error"] == "expired_token"


def test_user_codes_are_eight_characters_without_lookalikes():
    for _ in range(50):
        code = AccountsDevice.new_user_code()
        assert len(code) == 8
        assert not set(code) & set("01OIL")


def test_denied_code_stops_both_shapes(oauth):
    codex = post_json(oauth, "/api/accounts/deviceauth/usercode", {"client_id": "c"})[2]
    assert oauth.device.deny(codex["user_code"])
    poll = {"device_auth_id": codex["device_auth_id"], "user_code": codex["user_code"]}
    assert post_json(oauth, "/api/accounts/deviceauth/token", poll)[0] == 400


def test_forgejo_client_authorization_code_grant(oauth, alice, forgejo):
    client, secret = forgejo
    redirect = client.redirect_uris[0]
    code = oauth.device.issue_forgejo_code(alice["user_id"], client.client_id, redirect)
    form = {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect,
        "client_id": client.client_id,
        "client_secret": "nope",
    }
    assert post_form(oauth, "/oauth/token", form)[0] == 400
    code = oauth.device.issue_forgejo_code(alice["user_id"], client.client_id, redirect)
    form.update(code=code, client_secret=secret)
    assert post_form(oauth, "/oauth/token", form)[0] == 200
    assert oauth.device.issue_forgejo_code(alice["user_id"], "other", redirect) is None
    assert oauth.device.issue_forgejo_code(alice["user_id"], client.client_id, "https://x/") is None


def test_authorization_code_grant_refuses_unknown_codes(oauth):
    form = {
        "grant_type": "authorization_code",
        "code": "nothing",
        "redirect_uri": "https://x/",
        "client_id": "c",
    }
    assert post_form(oauth, "/oauth/token", form)[2]["error"] == "invalid_grant"
    assert "authorization_code" in oauth.discovery()["grant_types_supported"]


def test_accounts_device_redeem_refuses_mismatched_client_or_redirect_uri(oauth, alice, forgejo):
    client, _ = forgejo
    redirect = client.redirect_uris[0]
    code = oauth.device.issue_forgejo_code(alice["user_id"], client.client_id, redirect)
    # A foreign client_id spends the code and is refused.
    error, row = oauth.device.redeem(code, "other", redirect, "", "")
    assert row is None
    assert error == "The client or the redirect URI does not match the code."
    # The wrong redirect URI is refused the same way.
    code = oauth.device.issue_forgejo_code(alice["user_id"], client.client_id, redirect)
    error, row = oauth.device.redeem(code, client.client_id, "https://x/", "", "")
    assert row is None
    assert error == "The client or the redirect URI does not match the code."
