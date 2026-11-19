"""A secondary node's accounts service: proxy and verify, no store (specs/DREAMFERENCE_AUTH.md §7).

A secondary keeps no accounts. It forwards the authentication routes unchanged to its pinned
primary, holds the primary's public key (fetched once), and keeps the primary's answer to the live
check (`GET /ragon/v1/users/me` with the request's bearer) for 60 s in memory, keyed by the SHA-256
of the bearer. The JWT signature alone never admits a request here: there is no local trust. With
the primary unreachable, a forwarded route answers 503 and an uncached live check answers 401
"authentication server unreachable" (§10). `/ragon/v1/info`, `/ragon/v1/ca.pem` and the
`/backend-api/*` stubs are the gate's own and never reach this service.
"""

import hashlib
import json
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from pathlib import Path
from typing import Any, Final

from dreamference.accounts.accounts_oauth import ME_PATH, AccountsOauth, Answer

CACHE_TTL_S: Final[float] = 60.0
CACHE_LIMIT: Final[int] = 4096
FETCH_TIMEOUT_S: Final[float] = 5.0
JWKS_PATH: Final[str] = "/.well-known/jwks.json"
USERINFO_PATH: Final[str] = "/oauth/userinfo"
UNREACHABLE: Final[str] = "authentication server unreachable"
FORWARDED_EXACT: Final[tuple[str, ...]] = (
    "/codex/device",
    "/.well-known/openid-configuration",
    "/ragon/v1/users",
    "/ragon/v1/nodes",
)
FORWARDED_PREFIXES: Final[tuple[str, ...]] = (
    "/oauth",
    "/api/accounts/deviceauth",
    "/ragon/v1/users",
    "/ragon/v1/bindings",
    "/ragon/v1/nodes",
)
FORWARD_HEADERS: Final[tuple[str, ...]] = (
    "authorization",
    "api-key",
    "content-type",
    "accept",
    "chatgpt-account-id",
    "openai-organization",
)

# method, url, headers, body -> status, JSON body (or None); raises OSError when unreachable.
Transport = Callable[[str, str, dict[str, str], bytes], tuple[int, Any]]


class AccountsSecondary:
    """Same interface as `AccountsOauth` (`handle`), answered by the primary."""

    def __init__(
        self,
        primary_url: str,
        ca_file: Path | None = None,
        transport: Transport | None = None,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        """Binds the service to its pinned primary.

        Args:
            primary_url: The primary's gate, `https://<address>:8000`.
            ca_file: The primary's CA, pinned at first contact; the system store when None.
            transport: Replaces the HTTPS call (tests); raises OSError when the primary is down.
            clock: A monotonic clock in seconds.
        """
        self.primary_url = primary_url.rstrip("/")
        self.ca_file = ca_file
        self.transport = transport or self._https
        self.clock = clock
        self.jwks_document: dict[str, Any] | None = None
        self.cache: dict[str, tuple[float, Answer]] = {}

    @classmethod
    def find_primary(
        cls, node_id: str, browse: Callable[[], list[dict[str, str]]] | None = None
    ) -> str | None:
        """The pinned primary's gate URL from its advertisement; another id is ignored (§7)."""
        if browse is None:
            from dreamference.node.node_browser import NodeBrowser

            browse = NodeBrowser.browse
        for entry in browse():
            if entry.get("node") == node_id and entry.get("address"):
                host = entry["address"]
                host = f"[{host.replace('%', '%25')}]" if ":" in host else host  # noqa: PLR2004
                return f"https://{host}:{entry.get('port') or 8000}"
        return None

    # ----- the wire -----------------------------------------------------------------------

    def _https(
        self, method: str, url: str, headers: dict[str, str], body: bytes
    ) -> tuple[int, Any]:
        """One request to the primary, verified against its pinned CA."""
        context = ssl.create_default_context(cafile=str(self.ca_file) if self.ca_file else None)
        request = urllib.request.Request(url, data=body or None, headers=headers, method=method)
        try:
            with urllib.request.urlopen(request, timeout=FETCH_TIMEOUT_S, context=context) as reply:
                status, raw = reply.status, reply.read()
        except urllib.error.HTTPError as error:
            status, raw = error.code, error.read()
        except (urllib.error.URLError, ssl.SSLError, TimeoutError) as error:
            raise OSError(UNREACHABLE) from error
        try:
            return status, json.loads(raw)
        except ValueError:
            return status, None

    def ask(
        self, method: str, path: str, headers: dict[str, str], body: bytes = b""
    ) -> tuple[int, Any]:
        """Sends one request to the primary with only the headers that matter."""
        kept = {name: value for name, value in headers.items() if name in FORWARD_HEADERS}
        return self.transport(method, self.primary_url + path, kept, body)

    # ----- the routes ---------------------------------------------------------------------

    @classmethod
    def forwarded(cls, route: str) -> bool:
        """True for a route a secondary hands to its primary."""
        return route in FORWARDED_EXACT or any(
            route == prefix or route.startswith(prefix + "/") for prefix in FORWARDED_PREFIXES
        )

    @classmethod
    def unreachable(cls) -> Answer:
        """The §10 row for an authentication route with the primary down."""
        return (
            503,
            {},
            {
                "error": {
                    "message": UNREACHABLE,
                    "type": "invalid_request_error",
                    "param": None,
                    "code": "accounts_unavailable",
                }
            },
        )

    def handle(self, method: str, path: str, headers: dict[str, str], body: bytes) -> Answer:  # noqa: PLR0911
        """Answers one request: the held key and the cached live check, else the primary's answer."""
        split = urllib.parse.urlsplit(path)
        route = split.path.rstrip("/") or "/"
        if AccountsOauth.query_has_secret(path):
            return AccountsOauth.unauthorized()
        if route == JWKS_PATH and method == "GET":  # noqa: PLR2004
            return self.jwks()
        if route == ME_PATH and method == "GET":  # noqa: PLR2004
            return self.live_check(headers)
        if route == USERINFO_PATH and method in ("GET", "POST"):
            return self.userinfo(headers)
        if not self.forwarded(route):
            return AccountsOauth.api_error(404, "unknown_url", f"Unknown path {route}.")
        target = route + (f"?{split.query}" if split.query else "")
        try:
            status, document = self.ask(method, target, headers, body)
        except OSError:
            return self.unreachable()
        return self.relay(status, document)

    @classmethod
    def relay(cls, status: int, document: Any) -> Answer:  # noqa: ANN401
        """The primary's answer, with the `WWW-Authenticate` a 401 carries."""
        headers = {"WWW-Authenticate": 'Bearer realm="ragon"'} if status == 401 else {}  # noqa: PLR2004
        return status, headers, document if isinstance(document, dict) else {}

    def jwks(self) -> Answer:
        """The primary's public key, fetched once and then kept."""
        if self.jwks_document is None:
            try:
                status, document = self.ask("GET", JWKS_PATH, {})
            except OSError:
                return self.unreachable()
            if status != 200 or not isinstance(document, dict):  # noqa: PLR2004
                return self.relay(status, document)
            self.jwks_document = document
        return 200, {}, self.jwks_document

    # ----- the live check -----------------------------------------------------------------

    def live_check(self, headers: dict[str, str]) -> Answer:
        """`GET /ragon/v1/users/me` for the request's bearer, kept 60 s keyed by its SHA-256.

        An unreachable primary with nothing cached is a 401 "authentication server unreachable".
        """
        bearer = AccountsOauth.credential(headers)
        if bearer is None:
            return AccountsOauth.unauthorized()
        digest = hashlib.sha256(bearer.encode("latin-1", "replace")).hexdigest()
        now = self.clock()
        hit = self.cache.get(digest)
        if hit is not None and now - hit[0] < CACHE_TTL_S:
            return hit[1]
        try:
            status, document = self.ask("GET", ME_PATH, headers)
        except OSError:
            status_, headers_, body = AccountsOauth.api_error(401, "invalid_api_key", UNREACHABLE)
            return status_, headers_, body
        answer = self.relay(status, document)
        if len(self.cache) >= CACHE_LIMIT:
            self.cache.pop(next(iter(self.cache)))
        self.cache[digest] = (now, answer)
        return answer

    def userinfo(self, headers: dict[str, str]) -> Answer:
        """OpenID userinfo from the live check; an unreachable primary is a 503 (the gate's cue)."""
        status, answer_headers, document = self.live_check(headers)
        if status == 401 and document.get("error", {}).get("message") == UNREACHABLE:  # noqa: PLR2004
            return self.unreachable()
        if status != 200:  # noqa: PLR2004
            return status, answer_headers, document
        name = document.get("username", "")
        return (
            200,
            {},
            {
                "sub": document.get("user_id", ""),
                "email": name,
                "name": name,
                "preferred_username": name,
            },
        )
