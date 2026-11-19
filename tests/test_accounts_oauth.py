"""The primary's OAuth and account routes (AUTH §5.1-§5.4, §5.6, §9, §10), as Codex calls them."""

import base64
import http.client
import json
import threading
import urllib.parse

import pytest

from dreamference.accounts import AccountsJwt, AccountsOauth, AccountsService, AccountsStore
from dreamference.accounts.accounts_jwt import ACCESS_LIFETIME_S

# Codex's own client id and the claim it reads its account from (login/src/token_data.rs).
CODEX_CLIENT: str = "app_EMoamEEZ73f0CkXaXp7hrann"
CODEX_AUTH_CLAIM: str = "https://api.openai.com/auth"
ALICE_PW: str = "alice-pass-1"
BOB_PW: str = "bob-pass-2"


@pytest.fixture
def lines():
    return []


@pytest.fixture
def store(tmp_path, lines):
    return AccountsStore(
        tmp_path / "state" / "accounts.db", primary_name="box", audit_sink=lines.append
    )


@pytest.fixture
def oauth(store, tmp_path, lines):
    key = tmp_path / "state" / "signing-key.pem"
    signer = AccountsJwt(key, AccountsService.issuer("box"), "node-1", "box")
    return AccountsOauth(store, signer, log=lines.append)


@pytest.fixture
def users(store):
    store.create_user("t-alice", ALICE_PW)
    store.create_user("t-bob", BOB_PW)
    store.create_user("t-empty", "")


def post_form(oauth, path, fields):
    body = urllib.parse.urlencode(fields).encode()
    return oauth.handle("POST", path, {"content-type": "application/x-www-form-urlencoded"}, body)


def post_json(oauth, path, fields, token=None):
    headers = {"content-type": "application/json"}
    if token:
        headers["authorization"] = f"Bearer {token}"
    return oauth.handle("POST", path, headers, json.dumps(fields).encode())


def call(oauth, method, path, token=None, fields=None):
    headers = {"authorization": f"Bearer {token}"} if token else {}
    body = b""
    if fields is not None:
        headers["content-type"] = "application/json"
        body = json.dumps(fields).encode()
    return oauth.handle(method, path, headers, body)


def sign_in(oauth, username, password, **extra):
    fields = {
        "grant_type": "password",
        "username": username,
        "password": password,
        "client_id": CODEX_CLIENT,
        **extra,
    }
    status, _, body = post_form(oauth, "/oauth/token", fields)
    assert status == 200, body
    return body


def codex_claims(token):
    """Decodes the payload as Codex's `parse_chatgpt_jwt_claims` does: base64url, no padding."""
    payload = token.split(".")[1]
    assert "=" not in payload
    return json.loads(base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4)))


def test_the_password_grant_answers_a_token_set_codex_can_read(oauth, users, store):
    tokens = sign_in(oauth, "t-alice", ALICE_PW, machine="laptop")
    assert set(tokens) == {"id_token", "access_token", "refresh_token", "expires_in", "token_type"}
    assert tokens["expires_in"] == ACCESS_LIFETIME_S == 3600 and tokens["token_type"] == "Bearer"
    assert tokens["id_token"] == tokens["access_token"] and "." not in tokens["refresh_token"]
    claims = codex_claims(tokens["id_token"])
    alice = store.verify_password("t-alice", ALICE_PW)
    # IdClaims: email, the auth claim (AuthClaims) and exp (StandardJwtClaims).
    assert claims["email"] == "t-alice"
    auth = claims[CODEX_AUTH_CLAIM]
    assert auth["chatgpt_plan_type"] == "admin"
    assert auth["chatgpt_user_id"] == auth["user_id"] == auth["chatgpt_account_id"]
    assert auth["user_id"] == alice["user_id"] == claims["sub"]
    assert isinstance(claims["exp"], int) and claims["exp"] - claims["iat"] == 3600
    assert claims["aud"] == CODEX_CLIENT and claims["iss"] == "https://box.local:8000"
    assert claims["https://ragon.dev/auth"]["machine"] == "laptop"
    assert claims["https://ragon.dev/auth"]["org"] == "node-1"
    assert (
        codex_claims(sign_in(oauth, "t-bob", BOB_PW)["id_token"])[CODEX_AUTH_CLAIM][
            "chatgpt_plan_type"
        ]
        == "user"
    )


def test_the_password_grant_takes_json_and_an_empty_password(oauth, users):
    status, _, body = post_json(
        oauth,
        "/oauth/token",
        {"grant_type": "password", "username": "t-empty", "password": "", "client_id": "c"},
    )
    assert status == 200 and codex_claims(body["access_token"])["email"] == "t-empty"
    assert sign_in(oauth, "t-empty", "")["refresh_token"]
    status, _, body = post_form(
        oauth,
        "/oauth/token",
        {"grant_type": "password", "username": "t-empty", "password": "x", "client_id": "c"},
    )
    assert (status, body["error"]) == (400, "invalid_grant")


def test_refresh_with_codex_json_body_keeps_the_refresh_token(oauth, users):
    first = sign_in(oauth, "t-alice", ALICE_PW)
    status, _, body = post_json(
        oauth,
        "/oauth/token",
        {
            "client_id": CODEX_CLIENT,
            "grant_type": "refresh_token",
            "refresh_token": first["refresh_token"],
        },
    )
    assert status == 200
    assert body["refresh_token"] == first["refresh_token"]
    assert codex_claims(body["id_token"])[CODEX_AUTH_CLAIM]["chatgpt_plan_type"] == "admin"
    assert codex_claims(body["access_token"])["aud"] == CODEX_CLIENT
    # Codex treats a 400 invalid_grant as permanent: a revoked token answers exactly that.
    assert call(oauth, "DELETE", "/ragon/v1/users/me/sessions", body["access_token"])[0] == 200
    status, _, body = post_json(
        oauth,
        "/oauth/token",
        {"grant_type": "refresh_token", "refresh_token": first["refresh_token"]},
    )
    assert (status, body["error"]) == (400, "invalid_grant")


@pytest.mark.parametrize(
    ("fields", "code"),
    [
        (
            {"grant_type": "password", "username": "t-alice", "password": "nope", "client_id": "c"},
            "invalid_grant",
        ),
        (
            {"grant_type": "password", "username": "nobody", "password": "", "client_id": "c"},
            "invalid_grant",
        ),
        ({"grant_type": "password", "username": "t-alice", "client_id": "c"}, "invalid_request"),
        (
            {"grant_type": "password", "username": "t-alice", "password": ALICE_PW},
            "invalid_request",
        ),
        ({"grant_type": "refresh_token"}, "invalid_request"),
        ({"grant_type": "refresh_token", "refresh_token": "made-up"}, "invalid_grant"),
        ({"grant_type": "client_credentials"}, "unsupported_grant_type"),
        ({"grant_type": "authorization_code", "code": "x"}, "invalid_request"),
        ({}, "invalid_request"),
    ],
)
def test_token_errors_use_rfc_6749_codes(oauth, users, fields, code):
    status, _, body = post_form(oauth, "/oauth/token", fields)
    assert status == 400 and body["error"] == code and set(body) == {"error", "error_description"}


def test_a_wrong_password_and_an_unknown_user_answer_the_same(oauth, users):
    wrong = post_form(
        oauth,
        "/oauth/token",
        {"grant_type": "password", "username": "t-alice", "password": "x", "client_id": "c"},
    )
    unknown = post_form(
        oauth,
        "/oauth/token",
        {"grant_type": "password", "username": "nobody", "password": "x", "client_id": "c"},
    )
    assert wrong == unknown


def test_an_unreadable_body_is_invalid_request(oauth):
    status, _, body = oauth.handle(
        "POST", "/oauth/token", {"content-type": "application/json"}, b"[1, 2"
    )
    assert (status, body["error"]) == (400, "invalid_request")
    status, _, body = oauth.handle("POST", "/oauth/token", {}, b"\xff\xfe")
    assert (status, body["error"]) == (400, "invalid_request")


def test_a_service_account_cannot_take_the_password_grant(oauth, store):
    store.create_user("t-alice", ALICE_PW)
    store.ensure_service_account("abcdef01-0000-0000-0000-000000000000")
    status, _, body = post_form(
        oauth,
        "/oauth/token",
        {
            "grant_type": "password",
            "username": "ragon-node-abcdef01",
            "password": "",
            "client_id": "c",
        },
    )
    assert (status, body["error"]) == (400, "invalid_grant")


def test_a_must_change_account_signs_in_only_with_a_new_password(oauth, users, store):
    one_time = store.reset_password("t-bob")
    fields = {"grant_type": "password", "username": "t-bob", "password": one_time, "client_id": "c"}
    status, _, body = post_form(oauth, "/oauth/token", fields)
    assert (status, body["error"]) == (400, "password_change_required")
    tokens = sign_in(oauth, "t-bob", one_time, new_password="bob-new-3")
    assert tokens["access_token"]
    assert store.verify_password("t-bob", "bob-new-3")["must_change"] is False


def test_the_create_flag_and_the_users_route_make_an_account(oauth, users, store):
    tokens = sign_in(oauth, "t-carol", "c-pw", create="true")
    assert codex_claims(tokens["id_token"])[CODEX_AUTH_CLAIM]["chatgpt_plan_type"] == "user"
    status, _, body = post_form(
        oauth,
        "/oauth/token",
        {
            "grant_type": "password",
            "username": "t-carol",
            "password": "c-pw",
            "client_id": "c",
            "create": "1",
        },
    )
    assert (status, body["error"]) == (400, "invalid_request")  # already exists
    status, _, body = post_json(
        oauth, "/ragon/v1/users", {"username": "t-dave", "password": "", "machine": "desk"}
    )
    assert status == 200 and codex_claims(body["id_token"])["email"] == "t-dave"
    assert codex_claims(body["id_token"])["aud"] == "ragon"
    status, _, body = post_json(oauth, "/ragon/v1/users", {"username": "t-dave", "password": ""})
    assert (status, body["error"]["code"]) == (400, "invalid_request")
    status, _, body = post_json(oauth, "/ragon/v1/users", {"username": "x"})
    assert (status, body["error"]["type"]) == (400, "invalid_request_error")
    assert store.verify_password("t-dave", "") is not None


def test_revoke_takes_codex_body_for_a_refresh_or_an_access_token(oauth, users):
    first = sign_in(oauth, "t-alice", ALICE_PW)
    second = sign_in(oauth, "t-alice", ALICE_PW)
    status, _, body = post_json(
        oauth,
        "/oauth/revoke",
        {
            "token": first["refresh_token"],
            "token_type_hint": "refresh_token",
            "client_id": CODEX_CLIENT,
        },
    )
    assert (status, body) == (200, {})
    assert call(oauth, "GET", "/ragon/v1/users/me", first["access_token"])[0] == 401
    assert call(oauth, "GET", "/ragon/v1/users/me", second["access_token"])[0] == 200
    # An access token ends its whole session, refresh token included.
    status, _, _ = post_json(
        oauth, "/oauth/revoke", {"token": second["access_token"], "token_type_hint": "access_token"}
    )
    assert status == 200
    refresh = post_json(
        oauth,
        "/oauth/token",
        {"grant_type": "refresh_token", "refresh_token": second["refresh_token"]},
    )
    assert refresh[2]["error"] == "invalid_grant"
    # RFC 7009: an unknown token is still a 200; a missing one is invalid_request.
    assert post_form(oauth, "/oauth/revoke", {"token": "unknown"})[:2] == (200, {})
    status, _, body = post_form(oauth, "/oauth/revoke", {})
    assert (status, body["error"]) == (400, "invalid_request")


def test_jwks_and_discovery(oauth):
    status, _, jwks = call(oauth, "GET", "/.well-known/jwks.json")
    assert status == 200 and jwks == oauth.signer.jwks()
    status, _, discovery = call(oauth, "GET", "/.well-known/openid-configuration")
    assert status == 200 and discovery["issuer"] == "https://box.local:8000"
    assert discovery["jwks_uri"] == "https://box.local:8000/.well-known/jwks.json"
    assert discovery["token_endpoint"].endswith("/oauth/token")
    assert discovery["revocation_endpoint"].endswith("/oauth/revoke")
    assert {"password", "refresh_token"} <= set(discovery["grant_types_supported"])
    status, headers, _ = call(oauth, "POST", "/.well-known/jwks.json")
    assert status == 405 and headers["Allow"] == "GET"
    assert call(oauth, "GET", "/oauth/token")[0] == 405
    assert call(oauth, "GET", "/ragon/v1/nothing")[2]["error"]["code"] == "unknown_url"


def test_me_profile_userinfo_and_an_api_key(oauth, users, store):
    tokens = sign_in(oauth, "t-alice", ALICE_PW)
    status, _, me = call(oauth, "GET", "/ragon/v1/users/me", tokens["access_token"])
    assert status == 200
    assert (me["username"], me["role"], me["email"]) == ("t-alice", "admin", "t-alice@box.local")
    assert me["must_change"] is False and me["primary_name"] == "box"
    status, _, info = call(oauth, "GET", "/oauth/userinfo", tokens["access_token"])
    assert status == 200 and info["sub"] == me["user_id"] and info["email"] == "t-alice"
    key = store.mint_key("t-bob")
    status, _, me = oauth.handle("GET", "/ragon/v1/users/me", {"api-key": key}, b"")
    assert status == 200 and me["username"] == "t-bob"


@pytest.mark.parametrize(
    "headers",
    [
        {},
        {"authorization": "Bearer nope"},
        {"authorization": "Basic abc"},
        {"api-key": "sk-ragon-x"},
    ],
)
def test_a_missing_or_bad_credential_is_the_section_10_401(oauth, users, headers):
    status, out_headers, body = oauth.handle("GET", "/ragon/v1/users/me", headers, b"")
    assert status == 401 and out_headers["WWW-Authenticate"] == 'Bearer realm="ragon"'
    assert body == {
        "error": {
            "message": "Incorrect API key provided.",
            "type": "invalid_request_error",
            "param": None,
            "code": "invalid_api_key",
        }
    }


@pytest.mark.parametrize(
    "path",
    [
        "/oauth/token?password=x",
        "/oauth/token?refresh_token=x",
        "/ragon/v1/users/me?access_token=x",
        "/oauth/revoke?token=x",
        "/ragon/v1/users/me?api-key=sk-ragon-x",
    ],
)
def test_a_credential_in_the_url_is_refused(oauth, users, path):
    status, headers, body = oauth.handle("POST", path, {}, b"")
    assert status == 401 and "WWW-Authenticate" in headers
    assert body["error"]["code"] == "invalid_api_key"


def test_password_change(oauth, users, store):
    token = sign_in(oauth, "t-alice", ALICE_PW)["access_token"]
    path = "/ragon/v1/users/me/password"
    status, _, body = call(oauth, "POST", path, token, {"current": "wrong", "new": "n"})
    assert (status, body["error"]["code"]) == (403, "invalid_password")
    assert call(oauth, "POST", path, token, {"current": ALICE_PW})[0] == 400
    assert call(oauth, "GET", path, token)[0] == 405
    status, _, body = call(oauth, "POST", path, token, {"current": ALICE_PW, "new": ""})
    assert (status, body) == (200, {"changed": True})
    assert store.verify_password("t-alice", "") is not None
    assert store.verify_password("t-alice", ALICE_PW) is None


def test_sessions_list_and_delete_one_or_all(oauth, users):
    first = sign_in(oauth, "t-alice", ALICE_PW, machine="laptop")
    second = sign_in(oauth, "t-alice", ALICE_PW, machine="desk")
    bob = sign_in(oauth, "t-bob", BOB_PW)
    status, _, body = call(oauth, "GET", "/ragon/v1/users/me/sessions", second["access_token"])
    assert status == 200
    sessions = {s["machine"]: s for s in body["sessions"]}
    assert set(sessions) == {"laptop", "desk"}
    assert sessions["desk"]["current"] and not sessions["laptop"]["current"]
    assert sessions["desk"]["client_id"] == CODEX_CLIENT
    assert all("refresh" not in json.dumps(s) for s in body["sessions"])
    laptop = sessions["laptop"]["id"]
    path = f"/ragon/v1/users/me/sessions/{laptop}"
    assert call(oauth, "DELETE", path, second["access_token"]) == (200, {}, {"revoked": 1})
    assert call(oauth, "DELETE", path, second["access_token"])[2]["error"]["code"] == (
        "session_not_found"
    )
    assert call(oauth, "GET", path, second["access_token"])[0] == 405
    assert call(oauth, "GET", "/ragon/v1/users/me", first["access_token"])[0] == 401
    # Bob cannot end Alice's session, and `--all` ends only the caller's own.
    bob_path = f"/ragon/v1/users/me/sessions/{sessions['desk']['id']}"
    assert call(oauth, "DELETE", bob_path, bob["access_token"])[0] == 404
    status, _, body = call(oauth, "DELETE", "/ragon/v1/users/me/sessions", second["access_token"])
    assert (status, body) == (200, {"revoked": 1})
    assert call(oauth, "GET", "/ragon/v1/users/me", second["access_token"])[0] == 401
    assert call(oauth, "GET", "/ragon/v1/users/me", bob["access_token"])[0] == 200


def test_delete_account_and_the_last_admin(oauth, users, store):
    alice = sign_in(oauth, "t-alice", ALICE_PW)["access_token"]
    bob = sign_in(oauth, "t-bob", BOB_PW)
    status, _, body = call(oauth, "DELETE", "/ragon/v1/users/me", alice)
    assert (status, body["error"]["code"]) == (409, "last_admin")
    assert call(oauth, "PUT", "/ragon/v1/users/me", alice)[0] == 405
    assert call(oauth, "DELETE", "/ragon/v1/users/me", bob["access_token"]) == (
        200,
        {},
        {"deleted": True},
    )
    assert call(oauth, "GET", "/ragon/v1/users/me", bob["access_token"])[0] == 401
    refresh = post_json(
        oauth,
        "/oauth/token",
        {"grant_type": "refresh_token", "refresh_token": bob["refresh_token"]},
    )
    assert refresh[2]["error"] == "invalid_grant"
    assert store.verify_password("t-bob", BOB_PW) is None


def test_no_secret_reaches_an_answer_or_a_log_line(tmp_path, capsys):
    """The log scan: every answer, audit line and service log line of a session's life."""
    audit: list[str] = []
    store = AccountsStore(
        tmp_path / "s" / "accounts.db", primary_name="box", audit_sink=audit.append
    )
    oauth = AccountsService.build(store)
    oauth.log = AccountsService.log
    httpd = AccountsService.server(oauth, port=0)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    port = httpd.server_address[1]

    def request(method, path, fields=None, token=None, form=False):
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        headers = {}
        body = None
        if fields is not None:
            body = urllib.parse.urlencode(fields) if form else json.dumps(fields)
            headers["Content-Type"] = (
                "application/x-www-form-urlencoded" if form else "application/json"
            )
        if token:
            headers["Authorization"] = f"Bearer {token}"
        connection.request(method, path, body=body, headers=headers)
        response = connection.getresponse()
        text = response.read().decode()
        connection.close()
        return response.status, text

    answers: list[str] = []
    secrets = ["alice-secret-pw", "wrong-secret-pw", "new-secret-pw", "bob-secret-pw"]
    try:
        grant = {"grant_type": "password", "client_id": CODEX_CLIENT}
        status, text = request(
            "POST",
            "/oauth/token",
            {**grant, "username": "t-alice", "password": secrets[0], "create": "1"},
            form=True,
        )
        assert status == 200
        tokens = json.loads(text)
        secrets += [tokens["refresh_token"], tokens["access_token"]]
        for attempt in (
            ("POST", "/oauth/token", {**grant, "username": "t-alice", "password": secrets[1]}),
            ("POST", "/oauth/token", {**grant, "username": "nobody", "password": secrets[1]}),
            ("POST", "/oauth/token", {"grant_type": "refresh_token", "refresh_token": secrets[1]}),
            ("POST", f"/oauth/token?password={secrets[1]}", None),
            ("POST", f"/ragon/v1/users/me?access_token={tokens['access_token']}", None),
            ("POST", "/ragon/v1/users", {"username": "t-alice", "password": secrets[3]}),
            ("POST", "/oauth/revoke", {"token": secrets[1]}),
        ):
            answers.append(request(*attempt)[1])
        token = tokens["access_token"]
        answers.append(request("GET", "/ragon/v1/users/me", token=token)[1])
        answers.append(request("GET", "/ragon/v1/users/me/sessions", token=token)[1])
        answers.append(
            request(
                "POST",
                "/ragon/v1/users/me/password",
                {"current": secrets[1], "new": secrets[2]},
                token,
            )[1]
        )
        answers.append(
            request(
                "POST",
                "/ragon/v1/users/me/password",
                {"current": secrets[0], "new": secrets[2]},
                token,
            )[1]
        )
        answers.append(request("POST", "/oauth/revoke", {"token": tokens["refresh_token"]})[1])
        answers.append(request("GET", "/ragon/v1/users/me", token=token)[1])
        answers.append(
            request(
                "POST",
                "/oauth/token",
                {"grant_type": "refresh_token", "refresh_token": tokens["refresh_token"]},
            )[1]
        )
    finally:
        httpd.shutdown()
        httpd.server_close()
    log = capsys.readouterr().err
    assert "POST /oauth/token 400" in log and "GET /ragon/v1/users/me 401" in log
    assert "POST /oauth/token 401" in log  # the query was dropped from the log line
    journal = "\n".join(audit)
    assert "event=signin" in journal and "event=revoke" in journal
    for secret in secrets:
        for name, text in (("answer", "\n".join(answers)), ("log", log), ("audit", journal)):
            assert secret not in text, f"a secret reached the {name}"
    assert "$argon2" not in "\n".join(answers) + log + journal


def test_the_service_answers_a_fixed_500_and_bounds_the_body(tmp_path, capsys):
    store = AccountsStore(tmp_path / "s" / "accounts.db", primary_name="box", audit_sink=[].append)
    oauth = AccountsService.build(store)

    def boom(*_args):
        raise RuntimeError("secret detail sk-ragon-abc")

    oauth.handle = boom
    httpd = AccountsService.server(oauth, port=0)
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    port = httpd.server_address[1]
    try:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.request("GET", "/.well-known/jwks.json")
        response = connection.getresponse()
        text = response.read().decode()
        assert response.status == 500 and response.getheader("Cache-Control") == "no-store"
        assert json.loads(text)["error"]["code"] == "server_error"
        assert "secret detail" not in text
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
        connection.putrequest("POST", "/oauth/token")
        connection.putheader("Content-Length", str(10**7))
        connection.endheaders()
        response = connection.getresponse()
        assert response.status == 413
        response.read()
    finally:
        httpd.shutdown()
        httpd.server_close()
    err = capsys.readouterr().err
    assert "internal error: RuntimeError" in err and "secret detail" not in err


def test_the_service_listens_on_loopback_with_its_key_beside_the_store(tmp_path):
    assert AccountsService.issuer("box") == "https://box.local:8000"
    oauth = AccountsService.build(
        AccountsStore(tmp_path / "s" / "accounts.db", primary_name="box", audit_sink=[].append)
    )
    assert oauth.signer.key_path == tmp_path / "s" / "signing-key.pem"
    httpd = AccountsService.server(oauth, port=0)
    try:
        assert httpd.server_address[0] == "127.0.0.1"
    finally:
        httpd.server_close()
