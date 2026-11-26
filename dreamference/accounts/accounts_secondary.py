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
CA_PATH: Final[str] = "/ragon/v1/ca.pem"
INFO_PATH: Final[str] = "/ragon/v1/info"
# The most of a node's `/ragon/v1/info` read at first contact.
INFO_LIMIT: Final[int] = 64 * 1024
UNREACHABLE: Final[str] = "authentication server unreachable"
FORWARDED_EXACT: Final[tuple[str, ...]] = (
    "/codex/device",
    "/.well-known/openid-configuration",
    "/ragon/v1/users",
    "/ragon/v1/nodes",
    "/ragon/v1/setup",
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
    def gate_url(cls, address: str, port: str | int | None) -> str:
        """`https://<address>:<port>` for an advertised address; an IPv6 one in brackets."""
        host = f"[{address.replace('%', '%25')}]" if ":" in address else address  # noqa: PLR2004
        return f"https://{host}:{port or 8000}"

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
                return cls.gate_url(entry["address"], entry.get("port"))
        return None

    @classmethod
    def ca_path(cls, node_id: str) -> Path:
        """Where a node's pinned CA is kept, `~/.ragon/nodes/<node id>/ca.pem` (§6)."""
        return Path("~/.ragon/nodes").expanduser() / node_id / "ca.pem"

    @classmethod
    def fetch_ca(cls, address: str, port: str | int | None, node_id: str) -> bytes:
        """A node's CA at first contact (§6): `/ragon/v1/ca.pem`, then the handshake must chain to it.

        Fetched without verification (there is nothing to verify against yet, as the launcher's
        first contact); the gate's own certificate must then verify against the CA it served, and
        the gate must say, over that CA, that it is `node_id` (`identify`).

        Raises:
            OSError: The gate is unreachable, served no certificate, its certificate does not
                chain to the CA it served, or it is not the node `node_id`.
        """
        from dreamference.audit.auth_trace import AuthTrace

        first_contact = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        first_contact.check_hostname = False
        first_contact.verify_mode = ssl.CERT_NONE  # first contact: the chain is checked below
        url = cls.gate_url(address, port) + CA_PATH
        try:
            with urllib.request.urlopen(
                url, timeout=FETCH_TIMEOUT_S, context=first_contact
            ) as reply:
                pem = reply.read()
        except (urllib.error.URLError, ssl.SSLError, TimeoutError) as error:
            raise OSError(f"its gate did not answer ({error})") from error
        if not pem.lstrip().startswith(b"-----BEGIN CERTIFICATE-----"):
            raise OSError("its gate served no CA certificate")
        if not AuthTrace.chains_to_ca(address, int(port or 8000), pem):
            raise OSError("its gate's certificate does not chain to the CA it served")
        cls.identify(address, port, pem, node_id)
        return pem

    @classmethod
    def identify(cls, address: str, port: str | int | None, pem: bytes, node_id: str) -> None:
        """The gate at `address` says, over the CA `pem`, that it is the primary `node_id` (§6).

        A CA is filed under a node id: only that node's own `/ragon/v1/info`, verified against the
        CA, binds the two, as the launcher's first contact does. A stale or shared advertisement
        (the address now held by another node) is refused here, before anything is written.

        Raises:
            OSError: No answer over the CA, another node id, a primary other than itself, or a
                CA fingerprint other than the one served.
        """
        from dreamference.vllm_server.model_gate_tls import ModelGateTls

        context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
        context.check_hostname = False  # the chain counts, as for every pinned-CA call (_https)
        try:
            context.load_verify_locations(cadata=pem.decode("ascii"))
            url = cls.gate_url(address, port) + INFO_PATH
            with urllib.request.urlopen(url, timeout=FETCH_TIMEOUT_S, context=context) as reply:
                info = json.loads(reply.read(INFO_LIMIT))
        except (urllib.error.URLError, ssl.SSLError, TimeoutError, ValueError) as error:
            raise OSError(f"its gate did not say which node it is ({error})") from error
        if not isinstance(info, dict):
            raise OSError("its gate did not say which node it is")
        said = str(info.get("node_id") or "")
        if said.lower() != node_id.lower():
            raise OSError(f"the node at {address} is {said or 'unnamed'}, not {node_id}")
        primary = info.get("primary_id")
        if primary and str(primary).lower() != node_id.lower():
            raise OSError(f"{node_id} is a secondary of {primary}, not a primary")
        served = info.get("ca_fingerprint_sha256")
        if served and served != ModelGateTls.fingerprint(pem):
            raise OSError("its gate names another CA than the one it served")

    # ----- the wire -----------------------------------------------------------------------

    def _https(
        self, method: str, url: str, headers: dict[str, str], body: bytes
    ) -> tuple[int, Any]:
        """One request to the primary, verified against its pinned CA.

        With a pinned CA only the chain counts: the advertisement gives an address, while the
        node's certificate names only its DNS names, and the CA is that one node's alone (§6).
        """
        if self.ca_file:
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            context.load_verify_locations(cafile=str(self.ca_file))
            context.check_hostname = False
        else:
            context = ssl.create_default_context()
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
        if route == ME_PATH + "/keys" and method == "POST":  # noqa: PLR2004
            return self.no_minting()
        if not self.forwarded(route):
            return AccountsOauth.api_error(404, "unknown_url", f"Unknown path {route}.")
        target = route + (f"?{split.query}" if split.query else "")
        try:
            status, document = self.ask(method, target, headers, body)
        except OSError:
            return self.unreachable()
        return self.relay(status, document)

    def no_minting(self) -> Answer:
        """The 403 for a key minted here: a secondary never mints (as `ragon-admin node user key`)."""
        primary = urllib.parse.urlsplit(self.primary_url).hostname or self.primary_url
        return AccountsOauth.api_error(
            403,
            "secondary_node",
            f"This node is a secondary of {primary}: it keeps no accounts. Mint keys on the primary.",
        )

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
