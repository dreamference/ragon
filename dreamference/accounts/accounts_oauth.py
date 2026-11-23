"""The primary's OAuth and account routes (specs/DREAMFERENCE_AUTH.md §5.1-§5.4, §5.6, §9, §10).

A pure handler: method, path, headers and body in; status, headers and JSON body out. The HTTP
server around it is `AccountsService`. `/oauth/*` errors use RFC 6749's shape (`error`,
`error_description`); `/ragon/v1/*` errors use the vendor's shape of §10. No answer and no log line
carries a password, a token or a key, and a sign-in failure never says which half was wrong.
"""

import json
import re
import urllib.parse
from collections.abc import Callable
from http import HTTPStatus
from typing import Any, Final

from dreamference.accounts.accounts_bindings import AccountsBindings
from dreamference.accounts.accounts_device import GRANT_AUTH_CODE, GRANT_DEVICE, AccountsDevice
from dreamference.accounts.accounts_error import AccountsError
from dreamference.accounts.accounts_jwt import ACCESS_LIFETIME_S, RAGON_AUTH_CLAIM, AccountsJwt
from dreamference.accounts.accounts_store import (
    KEY_PREFIX,
    ROLE_ADMIN,
    SERVICE_PREFIX,
    AccountsStore,
)
from dreamference.accounts.accounts_usage import AccountsUsage

GRANT_PASSWORD: Final[str] = "password"
GRANT_REFRESH: Final[str] = "refresh_token"
JSON_TYPE: Final[str] = "application/json"
BEARER_REALM: Final[str] = 'Bearer realm="ragon"'
ME_PATH: Final[str] = "/ragon/v1/users/me"
USERS_PATH: Final[str] = "/ragon/v1/users"
BINDINGS_PATH: Final[str] = "/ragon/v1/bindings"
NODES_PATH: Final[str] = "/ragon/v1/nodes"
USAGE_PATH: Final[str] = "/ragon/v1/usage"
SETUP_PATH: Final[str] = "/ragon/v1/setup"
# Set by the gate, and only for a peer on the node's own loopback (it drops any copy a client
# sends): the first account is made on the node itself (specs/DREAMFERENCE_AUTH.md §8.6).
LOCAL_HEADER: Final[str] = "x-ragon-local"
LOCAL_MARK: Final[str] = "1"
MAX_DAYS: Final[int] = 366
NODE_ID: Final[re.Pattern[str]] = re.compile(r"^[0-9a-fA-F-]{8,64}$")
SESSION_PATH: Final[re.Pattern[str]] = re.compile(r"^/ragon/v1/users/me/sessions/(\d+)$")
# Parameter names that carry a credential: never accepted in a URL (§10).
SECRET_PARAMS: Final[frozenset[str]] = frozenset(
    {
        "password",
        "new_password",
        "refresh_token",
        "access_token",
        "id_token",
        "token",
        "api_key",
        "api-key",
        "key",
    }
)
TRUE_WORDS: Final[frozenset[str]] = frozenset({"1", "true", "yes", "on"})
SIGNIN_FAILED: Final[str] = "The username or the password is wrong."
BEARER_SCHEME: Final[str] = "bearer"
GET: Final[str] = "GET"
POST: Final[str] = "POST"
DELETE: Final[str] = "DELETE"

Answer = tuple[int, dict[str, str], dict[str, Any]]
Route = Callable[[dict[str, str], bytes], Answer]


class AccountsOauth:
    """Answers `/oauth/*`, `/.well-known/*` and `/ragon/v1/users*` for one primary."""

    def __init__(
        self,
        store: AccountsStore,
        signer: AccountsJwt,
        log: Callable[[str], None] | None = None,
        device: AccountsDevice | None = None,
        on_first: Callable[[], None] | None = None,
    ) -> None:
        """Binds the handler to its store and signer.

        Args:
            store: The account store.
            signer: The primary's token signer (it knows the node id and the primary's name).
            log: Receives operational lines (never a secret); defaults to nowhere.
            device: The table of device and authorization codes; one is made when absent.
            on_first: Runs once the node's first account exists (it makes the node the primary).
        """
        self.store = store
        self.signer = signer
        self.log = log or (lambda _line: None)
        self.on_first = on_first or (lambda: None)
        issuer = signer.issuer.rstrip("/")
        web = urllib.parse.urlsplit(issuer)
        self.device = device or AccountsDevice(issuer, f"{web.scheme}://{web.hostname}:3100/device")
        self.bindings = AccountsBindings(store)
        self.usage = AccountsUsage(store)
        # The fixed routes: path -> (the methods allowed, the answer).
        self.routes: dict[str, tuple[tuple[str, ...], Route]] = {
            "/.well-known/jwks.json": ((GET,), lambda _h, _b: (200, {}, self.signer.jwks())),
            "/.well-known/openid-configuration": (
                (GET,),
                lambda _h, _b: (200, {}, self.discovery()),
            ),
            "/oauth/token": ((POST,), self.token),
            "/oauth/revoke": ((POST,), self.revoke),
            "/oauth/userinfo": ((GET, POST), lambda headers, _b: self.userinfo(headers)),
            "/api/accounts/deviceauth/usercode": ((POST,), self.codex_usercode),
            "/api/accounts/deviceauth/token": ((POST,), self.codex_poll),
            "/oauth/device/code": ((POST,), self.standard_code),
            USERS_PATH: ((POST,), self.create),
            SETUP_PATH: ((GET,), lambda headers, _b: (200, {}, self.setup(headers))),
            NODES_PATH: ((POST,), self.register_node),
            BINDINGS_PATH: ((GET, POST), self.bindings_root),
            BINDINGS_PATH + "/redeem": ((POST,), self.redeem_binding),
            USAGE_PATH: ((GET,), self.usage_read),
            USAGE_PATH + "/unauthenticated": ((GET, POST), self.usage_unauthenticated),
        }

    # ----- shapes -------------------------------------------------------------------------

    @classmethod
    def oauth_error(cls, status: int, code: str, description: str) -> Answer:
        """An RFC 6749 error answer."""
        return status, {}, {"error": code, "error_description": description}

    @classmethod
    def api_error(cls, status: int, code: str, message: str) -> Answer:
        """A §10 error answer; a 401 carries `WWW-Authenticate`."""
        headers = {"WWW-Authenticate": BEARER_REALM} if status == HTTPStatus.UNAUTHORIZED else {}
        body = {
            "error": {
                "message": message,
                "type": "invalid_request_error",
                "param": None,
                "code": code,
            }
        }
        return status, headers, body

    @classmethod
    def unauthorized(cls) -> Answer:
        """The §10 401 for a missing, invalid, expired or revoked credential."""
        return cls.api_error(401, "invalid_api_key", "Incorrect API key provided.")

    @classmethod
    def parse_body(cls, headers: dict[str, str], body: bytes) -> dict[str, Any] | None:
        """Reads a JSON or form-encoded body into a flat dict.

        Args:
            headers: The request headers, names lower-case.
            body: The raw body.

        Returns:
            dict[str, Any] | None: The fields (an empty value kept), or None when unreadable.
        """
        kind = headers.get("content-type", "").split(";", 1)[0].strip().lower()
        try:
            text = body.decode("utf-8")
        except UnicodeDecodeError:
            return None
        if kind == JSON_TYPE or (not kind and text.lstrip().startswith("{")):
            try:
                parsed = json.loads(text) if text.strip() else {}
            except ValueError:
                return None
            return parsed if isinstance(parsed, dict) else None
        pairs = urllib.parse.parse_qsl(text, keep_blank_values=True)
        return dict(pairs)

    @classmethod
    def query_has_secret(cls, path: str) -> bool:
        """True when the URL's query names a credential parameter (§10)."""
        query = urllib.parse.urlsplit(path).query
        names = {name.lower() for name, _ in urllib.parse.parse_qsl(query, keep_blank_values=True)}
        return bool(names & SECRET_PARAMS)

    @classmethod
    def is_true(cls, value: object) -> bool:
        """Reads a flag sent as JSON true or as a form word."""
        return value is True or (isinstance(value, str) and value.strip().lower() in TRUE_WORDS)

    # ----- tokens -------------------------------------------------------------------------

    def token_set(
        self,
        user: dict[str, Any],
        client_id: str,
        machine: str,
        session_id: int,
        refresh_token: str,
    ) -> dict[str, Any]:
        """The answer of every successful grant (§5.4)."""
        claims = self.signer.claims(user, client_id, session_id, machine)
        token = self.signer.sign(claims)
        return {
            "id_token": token,
            "access_token": token,
            "refresh_token": refresh_token,
            "expires_in": ACCESS_LIFETIME_S,
            "token_type": "Bearer",
        }

    def new_session(self, user: dict[str, Any], client_id: str, machine: str) -> dict[str, Any]:
        """Opens a session for the user and answers its token set."""
        refresh, session_id = self.store.open_session(user["username"], client_id, machine)
        return self.token_set(user, client_id, machine, session_id, refresh)

    # ----- routing ------------------------------------------------------------------------

    def handle(self, method: str, path: str, headers: dict[str, str], body: bytes) -> Answer:
        """Answers one request.

        Args:
            method: The HTTP method.
            path: The request target, query included.
            headers: The headers, names lower-case.
            body: The raw body.

        Returns:
            Answer: Status, extra headers and the JSON body.
        """
        route = urllib.parse.urlsplit(path).path.rstrip("/") or "/"
        if self.query_has_secret(path):
            return self.unauthorized()
        # Pseudo-headers (as HTTP/2 has them, so no real header collides) carry what a route needs.
        headers = {**headers, ":method": method, ":query": urllib.parse.urlsplit(path).query}
        fixed = self.routes.get(route)
        if fixed is not None:
            allowed, answer = fixed
            return self.only(method, *allowed) or answer(headers, body)
        if route.startswith(BINDINGS_PATH + "/"):
            return self.only(method, DELETE) or self.remove_binding(
                headers, route[len(BINDINGS_PATH) + 1 :]
            )
        if route == ME_PATH or route.startswith(ME_PATH + "/"):
            return self.me(method, route, headers, body)
        return self.api_error(404, "unknown_url", f"Unknown path {route}.")

    @classmethod
    def only(cls, method: str, *allowed: str) -> Answer | None:
        """A 405 unless the method is one of `allowed`."""
        if method in allowed:
            return None
        status, headers, body = cls.api_error(
            405, "method_not_allowed", f"{method} is not allowed here."
        )
        return status, {**headers, "Allow": ", ".join(allowed)}, body

    def discovery(self) -> dict[str, Any]:
        """The OpenID Connect discovery document."""
        issuer = self.signer.issuer
        return {
            "issuer": issuer,
            "token_endpoint": f"{issuer}/oauth/token",
            "revocation_endpoint": f"{issuer}/oauth/revoke",
            "userinfo_endpoint": f"{issuer}/oauth/userinfo",
            "jwks_uri": f"{issuer}/.well-known/jwks.json",
            "grant_types_supported": [
                GRANT_PASSWORD,
                GRANT_REFRESH,
                GRANT_DEVICE,
                GRANT_AUTH_CODE,
            ],
            "device_authorization_endpoint": f"{issuer}/oauth/device/code",
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
            "token_endpoint_auth_methods_supported": ["none"],
            "scopes_supported": ["openid", "profile", "email", "offline_access"],
            "claims_supported": ["sub", "email", "name", "preferred_username"],
        }

    # ----- /oauth/token -------------------------------------------------------------------

    def token(self, headers: dict[str, str], body: bytes) -> Answer:
        """The grants of §5.4, form-encoded or JSON."""
        fields = self.parse_body(headers, body)
        if fields is None:
            return self.oauth_error(400, "invalid_request", "The body is not JSON or a form.")
        grant = fields.get("grant_type")
        if grant == GRANT_PASSWORD and self.is_true(fields.get("create")):
            refused = self.first_account_refused(headers)
            if refused is not None:
                return self.oauth_error(403, "first_account_on_node", refused)
        handlers = {
            GRANT_PASSWORD: self.password_grant,
            GRANT_REFRESH: self.refresh_grant,
            GRANT_DEVICE: self.device_code_grant,
            GRANT_AUTH_CODE: self.auth_code_grant,
        }
        if grant in handlers:
            return handlers[grant](fields)
        if not grant:
            return self.oauth_error(400, "invalid_request", "grant_type is missing.")
        return self.oauth_error(400, "unsupported_grant_type", "This grant type is not supported.")

    @classmethod
    def text_fields(cls, fields: dict[str, Any], *names: str) -> list[str] | None:
        """The named fields as strings, or None when one is missing or not a string."""
        values = [fields.get(name) for name in names]
        if any(not isinstance(value, str) for value in values):
            return None
        return [str(value) for value in values]

    def password_grant(self, fields: dict[str, Any]) -> Answer:
        """Signs in with a username and password (possibly empty); `create` makes the account."""
        values = self.text_fields(fields, "username", "password", "client_id")
        if values is None:
            return self.oauth_error(
                400, "invalid_request", "username, password and client_id are required."
            )
        username, password, client_id = values
        machine = str(fields.get("machine") or "")
        if self.is_true(fields.get("create")):
            first = self.store.first_account_open()
            try:
                self.store.create_user(username, password)
            except AccountsError as error:
                return self.oauth_error(400, "invalid_request", str(error))
            if first:
                self.first_made()
        user = self.store.verify_password(username, password, client="ragon")
        if user is None:
            return self.oauth_error(400, "invalid_grant", SIGNIN_FAILED)
        if user["must_change"]:
            new_password = fields.get("new_password")
            if not isinstance(new_password, str):
                return (
                    400,
                    {},
                    {
                        "error": "password_change_required",
                        "error_description": "Choose a new password to sign in.",
                    },
                )
            try:
                self.store.set_password(user["username"], new_password)
            except AccountsError as error:
                return self.oauth_error(400, "invalid_request", str(error))
            user = {**user, "must_change": False}
        return 200, {}, self.new_session(user, client_id, machine)

    def refresh_grant(self, fields: dict[str, Any]) -> Answer:
        """Codex's refresh: a new access and id token, the refresh token returned unchanged."""
        values = self.text_fields(fields, "refresh_token")
        if values is None or not values[0]:
            return self.oauth_error(400, "invalid_request", "refresh_token is required.")
        session = self.store.session_for_refresh(values[0])
        if session is None:
            self.store.audit("refresh", "-", "ragon", "invalid_grant")
            return self.oauth_error(400, "invalid_grant", "The refresh token is not valid.")
        client_id = fields.get("client_id")
        client = client_id if isinstance(client_id, str) and client_id else session["client_id"]
        answer = self.token_set(
            session["user"], client, session["machine"], session["session_id"], values[0]
        )
        return 200, {}, answer

    # ----- device authorization (§5.5) ----------------------------------------------------

    def codex_usercode(self, headers: dict[str, str], body: bytes) -> Answer:
        """Codex's `POST /api/accounts/deviceauth/usercode`."""
        fields = self.parse_body(headers, body)
        values = None if fields is None else self.text_fields(fields, "client_id")
        if values is None:
            return self.oauth_error(400, "invalid_request", "client_id is required.")
        return 200, {}, self.device.codex_usercode(values[0])

    def codex_poll(self, headers: dict[str, str], body: bytes) -> Answer:
        """Codex's `POST /api/accounts/deviceauth/token`: 403 while pending."""
        fields = self.parse_body(headers, body)
        values = None if fields is None else self.text_fields(fields, "device_auth_id", "user_code")
        if values is None:
            return self.oauth_error(400, "invalid_request", "device_auth_id and user_code needed.")
        status, answer = self.device.codex_poll(values[0], values[1])
        return status, {}, answer

    def standard_code(self, headers: dict[str, str], body: bytes) -> Answer:
        """RFC 8628's `POST /oauth/device/code`."""
        fields = self.parse_body(headers, body)
        values = None if fields is None else self.text_fields(fields, "client_id")
        if values is None:
            return self.oauth_error(400, "invalid_request", "client_id is required.")
        return 200, {}, self.device.standard_code(values[0])

    def device_code_grant(self, fields: dict[str, Any]) -> Answer:
        """The device-code grant: the RFC's pending words, then the token set."""
        values = self.text_fields(fields, "device_code", "client_id")
        if values is None:
            return self.oauth_error(400, "invalid_request", "device_code and client_id needed.")
        word, entry = self.device.device_grant(values[0], values[1])
        if entry is None or entry.user is None:
            return self.oauth_error(400, word, f"The device sign-in answered {word}.")
        user = self.store.user_by_id(entry.user)
        if user is None:
            return self.oauth_error(400, "invalid_grant", "The account no longer exists.")
        return 200, {}, self.new_session(user, entry.client_id, entry.machine)

    def auth_code_grant(self, fields: dict[str, Any]) -> Answer:
        """The authorization-code grant: Codex's device client and the Forgejo client only."""
        values = self.text_fields(fields, "code", "redirect_uri", "client_id")
        if values is None:
            return self.oauth_error(
                400, "invalid_request", "code, redirect_uri and client_id are required."
            )
        verifier = fields.get("code_verifier")
        secret = fields.get("client_secret")
        error, row = self.device.redeem(
            values[0],
            values[2],
            values[1],
            verifier if isinstance(verifier, str) else "",
            secret if isinstance(secret, str) else "",
        )
        if row is None:
            return self.oauth_error(400, "invalid_grant", error or "The code is not valid.")
        user = self.store.user_by_id(row.user)
        if user is None:
            return self.oauth_error(400, "invalid_grant", "The account no longer exists.")
        return 200, {}, self.new_session(user, row.client_id, row.machine)

    # ----- /oauth/revoke ------------------------------------------------------------------

    def revoke(self, headers: dict[str, str], body: bytes) -> Answer:
        """RFC 7009 with Codex's body: a refresh or access token ends its session."""
        fields = self.parse_body(headers, body)
        token = None if fields is None else fields.get("token")
        if not isinstance(token, str) or not token:
            return self.oauth_error(400, "invalid_request", "token is required.")
        claims = self.signer.verify(token)
        if claims is not None:
            sid = claims.get(RAGON_AUTH_CLAIM, {}).get("sid")
            if isinstance(sid, int):
                self.store.revoke_sessions(str(claims.get("sub")), sid)
        else:
            self.store.revoke_refresh_token(token)
        self.store.audit("revoke", "-", "ragon", "ok")
        return 200, {}, {}

    # ----- bearer -------------------------------------------------------------------------

    @classmethod
    def credential(cls, headers: dict[str, str]) -> str | None:
        """The credential of `Authorization: Bearer` or `api-key`, if any."""
        auth = headers.get("authorization", "")
        scheme, _, value = auth.partition(" ")
        if scheme.lower() == BEARER_SCHEME and value.strip():
            return value.strip()
        key = headers.get("api-key", "").strip()
        return key or None

    def caller(self, headers: dict[str, str]) -> tuple[dict[str, Any], int | None] | None:
        """The signed-in user and their session id (None for an API key), or None."""
        credential = self.credential(headers)
        if credential is None:
            return None
        if credential.startswith(KEY_PREFIX):
            user = self.store.verify_key(credential)
            return None if user is None else (user, None)
        claims = self.signer.verify(credential)
        if claims is None:
            return None
        sid = claims.get(RAGON_AUTH_CLAIM, {}).get("sid")
        user_id = str(claims.get("sub"))
        if not isinstance(sid, int) or not self.store.session_alive(sid, user_id):
            return None
        user = self.store.user_by_id(user_id)
        return None if user is None else (user, sid)

    def userinfo(self, headers: dict[str, str]) -> Answer:
        """OpenID Connect userinfo for the bearer of a token."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        user = found[0]
        return (
            200,
            {},
            {
                "sub": user["user_id"],
                "email": user["username"],
                "name": user["username"],
                "preferred_username": user["username"],
            },
        )

    # ----- /ragon/v1/users ----------------------------------------------------------------

    def create(self, headers: dict[str, str], body: bytes) -> Answer:
        """Creates an account and answers the token set of a password grant (§5.4)."""
        fields = self.parse_body(headers, body)
        values = None if fields is None else self.text_fields(fields, "username", "password")
        if fields is None or values is None:
            return self.api_error(
                400, "invalid_request", "username and password are required (JSON)."
            )
        username, password = values
        refused = self.first_account_refused(headers)
        if refused is not None:
            return self.api_error(403, "first_account_on_node", refused)
        first = self.store.first_account_open()
        try:
            user = self.store.create_user(username, password)
        except AccountsError as error:
            return self.api_error(400, "invalid_request", str(error))
        if first:
            self.first_made()
        client_id = str(fields.get("client_id") or "ragon")
        machine = str(fields.get("machine") or "")
        return 200, {}, self.new_session(user, client_id, machine)

    # ----- the first account (§8.6) ------------------------------------------------------------

    @classmethod
    def is_local(cls, headers: dict[str, str]) -> bool:
        """Whether the gate marked the request as coming from the node's own loopback."""
        return headers.get(LOCAL_HEADER, "").strip() == LOCAL_MARK

    def setup(self, headers: dict[str, str]) -> dict[str, Any]:
        """`GET /ragon/v1/setup`: whether the node still needs its first account.

        `local` says whether this caller may make it (it is on the node itself).
        """
        return {
            "setup": self.store.first_account_open(),
            "local": self.is_local(headers),
            "node": self.store.primary_name,
        }

    def first_account_refused(self, headers: dict[str, str]) -> str | None:
        """The plain refusal when the first account is asked for from another machine, else None."""
        if not self.store.first_account_open() or self.is_local(headers):
            return None
        node = self.store.primary_name
        return (
            f"{node} has no accounts yet. Its first account is made on {node} itself: "
            f"run ragon in a terminal there, or ragon-admin node user add <name>."
        )

    def first_made(self) -> None:
        """Runs `on_first` after the node's first account; a failure is logged, never raised."""
        try:
            self.on_first()
        except Exception as error:  # noqa: BLE001 - the account exists; the rest is logged
            self.log(
                f"first account: could not make this node the primary ({type(error).__name__})"
            )

    def me(self, method: str, route: str, headers: dict[str, str], body: bytes) -> Answer:
        """The signed-in user's own routes."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        user, sid = found
        match = SESSION_PATH.match(route)
        if match:
            return self.only(method, DELETE) or self.end_session(user, int(match.group(1)))
        actions: dict[str, dict[str, Callable[[], Answer]]] = {
            ME_PATH: {
                GET: lambda: (200, {}, self.profile(user)),
                DELETE: lambda: self.delete_account(user),
            },
            ME_PATH + "/password": {POST: lambda: self.change_password(user, headers, body)},
            ME_PATH + "/sessions": {
                GET: lambda: (200, {}, {"sessions": self.session_list(user, sid)}),
                DELETE: lambda: self.end_all_sessions(user),
            },
        }
        methods = actions.get(route)
        if methods is None:
            return self.api_error(404, "unknown_url", f"Unknown path {route}.")
        action = methods.get(method)
        return action() if action else (self.only(method, *methods) or self.unauthorized())

    def end_session(self, user: dict[str, Any], session_id: int) -> Answer:
        """`DELETE /ragon/v1/users/me/sessions/<id>`: one of the caller's own sessions."""
        if not self.store.revoke_sessions(user["user_id"], session_id):
            return self.api_error(404, "session_not_found", "No such session.")
        self.store.audit("revoke", user["username"], "ragon", "ok")
        return 200, {}, {"revoked": 1}

    def end_all_sessions(self, user: dict[str, Any]) -> Answer:
        """`DELETE /ragon/v1/users/me/sessions` (`ragon logout --all`): every session of the caller."""
        revoked = self.store.revoke_sessions(user["user_id"])
        self.store.audit("revoke", user["username"], "ragon", f"all:{revoked}")
        return 200, {}, {"revoked": revoked}

    def profile(self, user: dict[str, Any]) -> dict[str, Any]:
        """`GET /ragon/v1/users/me`."""
        primary = self.store.primary_name
        return {
            "username": user["username"],
            "user_id": user["user_id"],
            "role": user["role"],
            "email": f"{user['username']}@{primary}.local",
            "primary_name": primary,
            "must_change": user["must_change"],
        }

    def session_list(self, user: dict[str, Any], current: int | None) -> list[dict[str, Any]]:
        """The user's live sessions, the caller's own marked."""
        return [
            {**session, "current": session["id"] == current}
            for session in self.store.sessions(user["user_id"])
        ]

    def change_password(self, user: dict[str, Any], headers: dict[str, str], body: bytes) -> Answer:
        """`{current, new}`; either may be empty; clears the must-change flag."""
        fields = self.parse_body(headers, body)
        values = None if fields is None else self.text_fields(fields, "current", "new")
        if values is None:
            return self.api_error(400, "invalid_request", "current and new are required.")
        current, new = values
        if self.store.verify_password(user["username"], current, client="ragon") is None:
            return self.api_error(403, "invalid_password", "The current password is wrong.")
        try:
            self.store.set_password(user["username"], new)
        except AccountsError as error:
            return self.api_error(400, "invalid_request", str(error))
        return 200, {}, {"changed": True}

    def delete_account(self, user: dict[str, Any]) -> Answer:
        """`DELETE /ragon/v1/users/me`; the last admin cannot leave."""
        try:
            self.store.remove_user(user["username"])
        except AccountsError as error:
            return self.api_error(409, "last_admin", str(error))
        return 200, {}, {"deleted": True}

    # ----- /ragon/v1/bindings, /nodes, /usage (§5.6, §9, §14.1) ------------------------------

    def bindings_root(self, headers: dict[str, str], body: bytes) -> Answer:
        """`GET /ragon/v1/bindings` lists the caller's bindings; `POST` makes a pairing code."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        user = found[0]
        if headers.get(":method") == GET:
            return (200, {}, {"bindings": self.bindings.list(user["user_id"])})
        fields = self.parse_body(headers, body)
        if fields is None:
            return self.api_error(400, "invalid_request", "The body is not valid.")
        try:
            return 200, {}, self.bindings.create_code(user["user_id"], str(fields.get("chat_kind")))
        except AccountsError as error:
            return self.api_error(400, "invalid_request", str(error))

    def redeem_binding(self, headers: dict[str, str], body: bytes) -> Answer:
        """`POST /ragon/v1/bindings/redeem`: a bridge, with its node's service key, binds a chat."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        if not found[0]["service"]:
            return self.api_error(403, "forbidden", "Only a node's service key may redeem a code.")
        fields = self.parse_body(headers, body)
        if fields is None:
            return self.api_error(400, "invalid_request", "The body is not valid.")
        code, kind, identity = (fields.get(n) for n in ("code", "chat_kind", "chat_identity"))
        if not isinstance(code, str) or not isinstance(identity, str):
            return self.api_error(400, "invalid_request", "Send code, chat_kind and chat_identity.")
        try:
            return 200, {}, self.bindings.redeem(code, str(kind), identity)
        except AccountsError as error:
            return self.api_error(400, "invalid_code", str(error))

    def remove_binding(self, headers: dict[str, str], binding_id: str) -> Answer:
        """`DELETE /ragon/v1/bindings/<id>`: removes one of the caller's bindings."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        if not self.bindings.remove(found[0]["user_id"], binding_id):
            return self.api_error(404, "not_found", "No such binding.")
        return 200, {}, {"deleted": True}

    def register_node(self, headers: dict[str, str], body: bytes) -> Answer:
        """`POST /ragon/v1/nodes`: creates a node's service account; a key only the first time.

        Open at pin time by design (AUTH open question 10); the account is a service account
        with no password anyone knows, and the admin may remove it.
        """
        fields = self.parse_body(headers, body)
        node_id = fields.get("node_id") if fields else None
        if not isinstance(node_id, str) or not NODE_ID.match(node_id):
            return self.api_error(400, "invalid_request", "Send the node's node_id.")
        name = SERVICE_PREFIX + node_id.replace("-", "")[:8].lower()
        existed = any(u["username"] == name for u in self.store.list_users())
        account = self.store.ensure_service_account(node_id)
        answer: dict[str, Any] = {"username": account["username"], "created": not existed}
        if not existed:
            answer["key"] = self.store.mint_key(account["username"], "node")
        return (200 if existed else 201), {}, answer

    def usage_read(self, headers: dict[str, str], body: bytes) -> Answer:
        """`GET /ragon/v1/usage?days=N`: the caller's own rows; an admin's are every user's."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        user = found[0]
        days = self.days_param(headers)
        rows = (
            self.store.usage(days)
            if user["role"] == ROLE_ADMIN
            else self.usage.for_user(user["username"], days)
        )
        return 200, {}, {"usage": rows}

    def usage_unauthenticated(self, headers: dict[str, str], body: bytes) -> Answer:
        """A node's daily unauthenticated counts (POST) and the admin's read of them (GET)."""
        found = self.caller(headers)
        if found is None:
            return self.unauthorized()
        user = found[0]
        if headers.get(":method") == GET:
            if user["role"] != ROLE_ADMIN:
                return self.api_error(403, "forbidden", "Only an admin reads these counts.")
            return (
                200,
                {},
                {"unauthenticated": self.usage.unauthenticated(self.days_param(headers))},
            )
        if not user["service"] and user["role"] != ROLE_ADMIN:
            return self.api_error(403, "forbidden", "Only a node's service key sends counts.")
        return self.receive_counts(user["username"], headers, body)

    def receive_counts(self, node: str, headers: dict[str, str], body: bytes) -> Answer:
        """Stores the counts a node sent for one day."""
        fields = self.parse_body(headers, body)
        counts = fields.get("counts") if fields else None
        day = fields.get("day") if fields else None
        if not isinstance(counts, list) or not isinstance(day, str):
            return self.api_error(400, "invalid_request", "Send day and counts.")
        try:
            stored = self.usage.add_unauthenticated(node, day, counts)
        except AccountsError as error:
            return self.api_error(400, "invalid_request", str(error))
        return 200, {}, {"stored": stored}

    @classmethod
    def days_param(cls, headers: dict[str, str]) -> int:
        """The `days` of the request, 7 when absent."""
        query = dict(urllib.parse.parse_qsl(headers.get(":query", "")))
        raw = query.get("days", "7")
        return int(raw) if raw.isdigit() and 0 < int(raw) <= MAX_DAYS else 7
