"""The gate's credential check (specs/DREAMFERENCE_AUTH.md §10).

Executed inside the gate's container beside `model_gate_service.py`, so it uses the standard
library only: the RS256 check is the RSA public operation and a PKCS#1 v1.5 comparison on `int`s.

A model request carries `Authorization: Bearer <token or key>` or `api-key: <key>`, never the URL.
A JWT is verified here (signature against the primary's JWKS, `exp`, `iss`), then the accounts
service says whether the session, or the key, is still valid (`/oauth/userinfo`); that answer is
kept 60 s, keyed by the SHA-256 of the credential, so a revoked token stops being served within a
minute while a response already streaming finishes. A request with no credential is served and
counted while `auth_enforce` is false (the default), and refused when it is true.
"""

import asyncio
import base64
import hashlib
import hmac
import json
import time
from pathlib import Path
from typing import Any, Final

AUTH_FILE: Final[str] = "auth.json"
CACHE_TTL_S: Final[float] = 60.0
CACHE_LIMIT: Final[int] = 4096
KEY_PREFIX: Final[str] = "sk-ragon-"
BEARER_REALM: Final[str] = 'Bearer realm="ragon"'
JWT_PARTS: Final[int] = 3
ALGORITHM: Final[str] = "RS256"
OPENAI_AUTH_CLAIM: Final[str] = "https://api.openai.com/auth"
RAGON_AUTH_CLAIM: Final[str] = "https://ragon.dev/auth"
LOOKUP_TIMEOUT_S: Final[float] = 5.0
HTTP_OK: Final[int] = 200
HTTP_UNAUTHORIZED: Final[int] = 401
# SHA-256's DigestInfo prefix (RFC 8017 §9.2, note 1).
SHA256_PREFIX: Final[bytes] = bytes.fromhex("3031300d060960864801650304020105000420")
URL_CREDENTIAL_NAMES: Final[frozenset[str]] = frozenset(
    {
        "api_key",
        "api-key",
        "apikey",
        "key",
        "token",
        "access_token",
        "id_token",
        "bearer",
        "authorization",
        "auth",
    }
)
# Routes that stay open at 1.0 (§10): the model list and the probes.
OPEN_PATHS: Final[tuple[str, ...]] = ("/v1/models",)

INVALID_BODY: Final[dict[str, Any]] = {
    "error": {
        "message": "Incorrect API key provided.",
        "type": "invalid_request_error",
        "param": None,
        "code": "invalid_api_key",
    }
}
SIGN_IN_BODY: Final[dict[str, Any]] = {
    "error": {
        "message": "Sign in with ragon login.",
        "type": "invalid_request_error",
        "param": None,
        "code": "invalid_api_key",
    }
}
UNREACHABLE_BODY: Final[dict[str, Any]] = {
    "error": {
        "message": "authentication server unreachable",
        "type": "invalid_request_error",
        "param": None,
        "code": "invalid_api_key",
    }
}

# What `check` answers in `verdict`.
BEARER_SCHEME: Final[str] = "bearer"
CLIENT_KIND_PUNCTUATION: Final[str] = "-_."
KEY_TYPE: Final[str] = "RSA"
SERVE: Final[str] = "serve"
REFUSE: Final[str] = "refuse"


class ModelGateAuth:
    """The §10 check for one gate."""

    def __init__(self, state_dir: str, accounts: tuple[str, int]) -> None:
        """Set up the check.

        Args:
            state_dir: The gate's state folder (`auth.json` lives there).
            accounts: The accounts service's address (the primary's).
        """
        self.state_dir = state_dir
        self.accounts = accounts
        self.cache: dict[str, tuple[float, dict[str, Any] | None]] = {}
        self.jwks: tuple[float, dict[str, Any], str] | None = None
        self.unauthenticated: dict[tuple[str, str, str], int] = {}

    # -- the rules -----------------------------------------------------------------------------

    def enforce(self) -> bool:
        """Whether a request with no credential is refused (`[node] auth_enforce`, default false)."""
        try:
            data = json.loads((Path(self.state_dir) / AUTH_FILE).read_text())
        except (OSError, ValueError):
            return False
        return isinstance(data, dict) and data.get("enforce") is True

    @classmethod
    def gated(cls, method: str, path: str) -> bool:
        """Whether a request needs the check: a model route, not the open model list."""
        route = path.rstrip("/") or "/"
        if not route.startswith("/v1/"):
            return False
        return not (
            method in ("GET", "HEAD")
            and any(route == p or route.startswith(p + "/") for p in OPEN_PATHS)
        )

    @classmethod
    def url_has_credential(cls, head: bytes) -> bool:
        """Whether the request target's query carries a credential (never logged, always 401)."""
        line = head.split(b"\r\n", 1)[0].decode("latin-1")
        parts = line.split(" ")
        if len(parts) < 2 or "?" not in parts[1]:  # noqa: PLR2004 - method and target
            return False
        query = parts[1].split("?", 1)[1].split("#", 1)[0]
        for pair in query.split("&"):
            name = pair.partition("=")[0].replace("%2D", "-").replace("%2d", "-").lower()
            if name in URL_CREDENTIAL_NAMES:
                return True
        return False

    @classmethod
    def credential(cls, headers: list[tuple[bytes, bytes]]) -> str | None:
        """The bearer or `api-key` value, or None when the request carries neither."""
        values = dict(headers)
        auth = values.get(b"authorization", b"").decode("latin-1").strip()
        if auth:
            scheme, _, token = auth.partition(" ")
            if scheme.lower() == BEARER_SCHEME and token.strip():
                return token.strip()
            return auth  # a malformed header is a credential, and an invalid one
        key = values.get(b"api-key", b"").decode("latin-1").strip()
        return key or None

    @classmethod
    def client_kind(cls, headers: list[tuple[bytes, bytes]]) -> str:
        """A short label for the client: `originator`, else the User-Agent's product."""
        values = dict(headers)
        text = values.get(b"originator", b"") or values.get(b"user-agent", b"")
        word = text.decode("latin-1").split("/", 1)[0].split(" ", 1)[0].strip().lower()
        kept = "".join(c for c in word if c.isalnum() or c in CLIENT_KIND_PUNCTUATION)[:32]
        return kept or "unknown"

    @classmethod
    def b64url_decode(cls, text: str) -> bytes:
        """Base64url without padding."""
        return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))

    @classmethod
    def verify_jwt(
        cls, token: str, jwks: dict[str, Any], issuer: str, now: float | None = None
    ) -> dict[str, Any] | None:
        """Checks a JWT's RS256 signature, `iss` and `exp` locally.

        Args:
            token: The compact JWT.
            jwks: The primary's JWK Set.
            issuer: The issuer the claims must name.
            now: The clock; defaults to the time.

        Returns:
            dict[str, Any] | None: The claims, or None for anything not valid now.
        """
        parts = token.split(".")
        decoded = cls.decode_jwt(parts, issuer, time.time() if now is None else now)
        if decoded is None:
            return None
        header, claims, signature = decoded
        for jwk in jwks.get("keys", []):
            if not isinstance(jwk, dict) or jwk.get("kty") != KEY_TYPE:
                continue
            if header.get("kid") and jwk.get("kid") not in (None, header.get("kid")):
                continue
            if cls.rsa_ok(jwk, f"{parts[0]}.{parts[1]}".encode(), signature):
                return claims
        return None

    @classmethod
    def decode_jwt(
        cls, parts: list[str], issuer: str, clock: float
    ) -> tuple[dict[str, Any], dict[str, Any], bytes] | None:
        """Decodes a JWT's parts and checks `alg`, `iss` and `exp`; None when any fails."""
        if len(parts) != JWT_PARTS:
            return None
        try:
            header = json.loads(cls.b64url_decode(parts[0]))
            claims = json.loads(cls.b64url_decode(parts[1]))
            signature = cls.b64url_decode(parts[2])
        except ValueError:
            return None
        if not isinstance(header, dict) or header.get("alg") != ALGORITHM:
            return None
        if not isinstance(claims, dict) or claims.get("iss") != issuer:
            return None
        exp = claims.get("exp")
        if not isinstance(exp, int) or isinstance(exp, bool) or exp <= clock:
            return None
        return header, claims, signature

    @classmethod
    def rsa_ok(cls, jwk: dict[str, Any], message: bytes, signature: bytes) -> bool:
        """RSASSA-PKCS1-v1_5 with SHA-256, by the public operation."""
        try:
            n = int.from_bytes(cls.b64url_decode(str(jwk["n"])), "big")
            e = int.from_bytes(cls.b64url_decode(str(jwk["e"])), "big")
        except (KeyError, ValueError):
            return False
        size = (n.bit_length() + 7) // 8
        if n <= 0 or len(signature) != size:
            return False
        recovered = pow(int.from_bytes(signature, "big"), e, n).to_bytes(size, "big")
        digest = SHA256_PREFIX + hashlib.sha256(message).digest()
        padding = size - len(digest) - 3
        if padding < 8:  # noqa: PLR2004 - RFC 8017 minimum
            return False
        expected = b"\x00\x01" + b"\xff" * padding + b"\x00" + digest
        return hmac.compare_digest(recovered, expected)

    @classmethod
    def headers_match(cls, claims: dict[str, Any], headers: list[tuple[bytes, bytes]]) -> bool:
        """`ChatGPT-Account-ID` and `OpenAI-Organization`, when sent, must match the token."""
        values = dict(headers)
        account = values.get(b"chatgpt-account-id", b"").decode("latin-1").strip()
        org = values.get(b"openai-organization", b"").decode("latin-1").strip()
        openai = claims.get(OPENAI_AUTH_CLAIM)
        ragon = claims.get(RAGON_AUTH_CLAIM)
        if account:
            expected = openai.get("chatgpt_account_id") if isinstance(openai, dict) else None
            if account != expected:
                return False
        if org:
            expected_org = ragon.get("org") if isinstance(ragon, dict) else None
            if org != expected_org:
                return False
        return True

    # -- the primary ---------------------------------------------------------------------------

    async def fetch(self, target: str, bearer: str | None = None) -> tuple[int, Any]:
        """One GET to the accounts service: `(status, JSON body or None)`; raises OSError if down."""
        extra = f"Authorization: Bearer {bearer}\r\n" if bearer else ""
        request = (
            f"GET {target} HTTP/1.1\r\nHost: accounts\r\n{extra}Accept: application/json\r\n"
            "Connection: close\r\n\r\n"
        ).encode("latin-1")

        async def run() -> bytes:
            reader, writer = await asyncio.open_connection(*self.accounts)
            try:
                writer.write(request)
                await writer.drain()
                return await reader.read(-1)
            finally:
                writer.close()

        try:
            raw = await asyncio.wait_for(run(), LOOKUP_TIMEOUT_S)
        except TimeoutError as error:
            raise OSError("accounts service timed out") from error
        head, _, body = raw.partition(b"\r\n\r\n")
        try:
            status = int(head.split(b" ", 2)[1])
        except (IndexError, ValueError) as error:
            raise OSError("accounts service answered nothing usable") from error
        try:
            return status, json.loads(body)
        except ValueError:
            return status, None

    async def keys(self) -> tuple[dict[str, Any], str]:
        """The primary's JWKS and issuer, kept 60 s; raises OSError when unreachable."""
        now = time.monotonic()
        if self.jwks is not None and now - self.jwks[0] < CACHE_TTL_S:
            return self.jwks[1], self.jwks[2]
        status, jwks = await self.fetch("/.well-known/jwks.json")
        _, config = await self.fetch("/.well-known/openid-configuration")
        issuer = config.get("issuer") if isinstance(config, dict) else None
        if status != HTTP_OK or not isinstance(jwks, dict) or not isinstance(issuer, str):
            raise OSError("accounts service has no keys")
        self.jwks = (now, jwks, issuer)
        return jwks, issuer

    @classmethod
    def forwarding(cls) -> bool:
        """True on a secondary: its accounts service already keeps each live answer 60 s (§7)."""
        from dreamference.node.node_settings import NodeSettings

        return NodeSettings.pinned_primary() is not None

    async def lookup(self, bearer: str) -> dict[str, Any] | None:
        """The live answer for a credential: its user, or None when invalid; cached 60 s.

        On a secondary the gate keeps nothing itself: a second 60 s here on top of the
        service's would let a revoked credential live up to two minutes (§7 says one).

        Raises:
            OSError: The accounts service is unreachable and the credential is not cached.
        """
        digest = hashlib.sha256(bearer.encode("latin-1", "replace")).hexdigest()
        now = time.monotonic()
        keep = not self.forwarding()
        if not keep:
            self.cache.clear()
        hit = self.cache.get(digest)
        if hit is not None and now - hit[0] < CACHE_TTL_S:
            return hit[1]
        claims: dict[str, Any] | None = None
        if not bearer.startswith(KEY_PREFIX):
            jwks, issuer = await self.keys()
            claims = self.verify_jwt(bearer, jwks, issuer)
            if claims is None:
                if keep:
                    self.remember(digest, now, None)
                return None
        status, body = await self.fetch("/oauth/userinfo", bearer)
        user: dict[str, Any] | None = None
        if status == HTTP_OK and isinstance(body, dict):
            name = body.get("preferred_username") or body.get("email") or body.get("name")
            user = {"name": str(name or body.get("sub") or "?"), "claims": claims}
        elif status != HTTP_UNAUTHORIZED:
            raise OSError(f"accounts service answered {status}")
        if keep:
            self.remember(digest, now, user)
        return user

    def remember(self, digest: str, now: float, user: dict[str, Any] | None) -> None:
        """Stores one answer, dropping the oldest when the cache is full."""
        if len(self.cache) >= CACHE_LIMIT:
            self.cache.pop(next(iter(self.cache)))
        self.cache[digest] = (now, user)

    # -- the decision --------------------------------------------------------------------------

    async def check(
        self, head: bytes, method: str, path: str, headers: list[tuple[bytes, bytes]]
    ) -> dict[str, Any]:
        """Decides one request.

        Args:
            head: The raw request head (for the URL check).
            method: The method.
            path: The path without its query.
            headers: The parsed headers, names lowercased.

        Returns:
            dict[str, Any]: `verdict` (`serve` or `refuse`), `user` (a name, `-` for none), and
            for a refusal `body` and `status`.
        """
        if not self.gated(method, path):
            return {"verdict": SERVE, "user": None}
        if self.url_has_credential(head):
            return self.refusal(401, INVALID_BODY)
        bearer = self.credential(headers)
        if bearer is None:
            if self.enforce():
                return self.refusal(401, SIGN_IN_BODY)
            return {"verdict": SERVE, "user": "-"}
        return await self.check_credential(bearer, headers)

    async def check_credential(
        self, bearer: str, headers: list[tuple[bytes, bytes]]
    ) -> dict[str, Any]:
        """Decides a request that carries a credential."""
        try:
            user = await self.lookup(bearer)
        except OSError:
            return self.refusal(401, UNREACHABLE_BODY)
        if user is None:
            return self.refusal(401, INVALID_BODY)
        claims = user["claims"]
        if claims is not None and not self.headers_match(claims, headers):
            return self.refusal(401, INVALID_BODY)
        return {"verdict": SERVE, "user": user["name"]}

    @classmethod
    def refusal(cls, status: int, body: dict[str, Any]) -> dict[str, Any]:
        """A refusal's record."""
        return {"verdict": REFUSE, "user": None, "status": status, "body": body}

    def count_unauthenticated(self, kind: str, source: str) -> int:
        """Counts a served request with no credential per day, client kind and source."""
        key = (time.strftime("%Y-%m-%d", time.gmtime()), kind, source)
        self.unauthenticated[key] = self.unauthenticated.get(key, 0) + 1
        return self.unauthenticated[key]
