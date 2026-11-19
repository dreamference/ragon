"""The primary's token signer (specs/DREAMFERENCE_AUTH.md §5.2, §5.3, §9).

One RS256 key pair per primary, a PEM file mode 0600 beside the account store, created on first
use and never rotated. The access token and the id token carry the same claims, under the names
Codex reads (`login/src/token_data.rs`). Nothing here ever logs a key or a token.
"""

import base64
import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any, Final

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding, rsa

ACCESS_LIFETIME_S: Final[int] = 3600
KEY_MODE: Final[int] = 0o600
KEY_BITS: Final[int] = 2048
KEY_FILE: Final[str] = "signing-key.pem"
ALGORITHM: Final[str] = "RS256"
OPENAI_AUTH_CLAIM: Final[str] = "https://api.openai.com/auth"
RAGON_AUTH_CLAIM: Final[str] = "https://ragon.dev/auth"
JWT_PARTS: Final[int] = 3


class AccountsJwt:
    """Signs and checks the primary's JWTs and publishes its key as a JWKS."""

    def __init__(
        self, key_path: Path, issuer: str, node_id: str = "", primary_name: str = ""
    ) -> None:
        """Loads the signing key, creating it (mode 0600) when it does not exist yet.

        Args:
            key_path: The PEM file of the private key.
            issuer: The primary's issuer URL, `https://<primary>.local:8000`.
            node_id: The primary's node id (the `org` claim).
            primary_name: The primary's name.
        """
        self.key_path = key_path
        self.issuer = issuer
        self.node_id = node_id
        self.primary_name = primary_name
        self.key = self._load_or_create(key_path)
        public = self.key.public_key()
        der = public.public_bytes(
            serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo
        )
        self.kid = self.b64url(hashlib.sha256(der).digest()[:12])
        self.public = public

    @classmethod
    def _load_or_create(cls, path: Path) -> rsa.RSAPrivateKey:
        """Reads the PEM key, or writes a new one with mode 0600 from its first byte."""
        if path.exists():
            path.chmod(KEY_MODE)
            loaded = serialization.load_pem_private_key(path.read_bytes(), password=None)
            if not isinstance(loaded, rsa.RSAPrivateKey):
                raise ValueError(f"{path} is not an RSA key.")
            return loaded
        key = rsa.generate_private_key(public_exponent=65537, key_size=KEY_BITS)
        pem = key.private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        )
        path.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(path, os.O_CREAT | os.O_EXCL | os.O_WRONLY, KEY_MODE)
        with os.fdopen(fd, "wb") as handle:
            handle.write(pem)
        return key

    @staticmethod
    def b64url(data: bytes) -> str:
        """Encodes base64url without padding, as JWTs do."""
        return base64.urlsafe_b64encode(data).decode("ascii").rstrip("=")

    @staticmethod
    def b64url_decode(text: str) -> bytes:
        """Decodes base64url with or without padding."""
        return base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))

    def claims(
        self,
        user: dict[str, Any],
        client_id: str,
        session_id: int,
        machine: str,
        now: int | None = None,
    ) -> dict[str, Any]:
        """Builds the claims of §5.3, plus `sid` (the session) under the Ragon claim.

        Args:
            user: The user, as the store returns it.
            client_id: The grant's client.
            session_id: The refresh token's session.
            machine: The machine name of the session.
            now: The issue time; defaults to the clock.

        Returns:
            dict[str, Any]: The claims.
        """
        issued = int(time.time()) if now is None else int(now)
        uid = user["user_id"]
        return {
            "iss": self.issuer,
            "sub": uid,
            "aud": client_id,
            "iat": issued,
            "exp": issued + ACCESS_LIFETIME_S,
            "email": user["username"],
            "name": user["username"],
            "preferred_username": user["username"],
            OPENAI_AUTH_CLAIM: {
                "chatgpt_plan_type": user["role"],
                "chatgpt_user_id": uid,
                "chatgpt_account_id": uid,
                "user_id": uid,
            },
            RAGON_AUTH_CLAIM: {
                "org": self.node_id,
                "primary_name": self.primary_name,
                "machine": machine,
                "sid": session_id,
            },
        }

    def sign(self, claims: dict[str, Any]) -> str:
        """Signs the claims as a compact RS256 JWT."""
        header = {"alg": ALGORITHM, "typ": "JWT", "kid": self.kid}
        signing_input = ".".join(
            self.b64url(json.dumps(part, separators=(",", ":")).encode())
            for part in (header, claims)
        )
        signature = self.key.sign(signing_input.encode(), padding.PKCS1v15(), hashes.SHA256())
        return f"{signing_input}.{self.b64url(signature)}"

    def verify(self, token: str, now: int | None = None) -> dict[str, Any] | None:
        """Checks a JWT's signature, algorithm, issuer and expiry.

        Args:
            token: The compact JWT.
            now: The time to check `exp` against; defaults to the clock.

        Returns:
            dict[str, Any] | None: Its claims, or None for anything not valid now.
        """
        parts = token.split(".")
        if len(parts) != JWT_PARTS:
            return None
        try:
            header = json.loads(self.b64url_decode(parts[0]))
            claims = json.loads(self.b64url_decode(parts[1]))
            signature = self.b64url_decode(parts[2])
            self.public.verify(
                signature, f"{parts[0]}.{parts[1]}".encode(), padding.PKCS1v15(), hashes.SHA256()
            )
        except (ValueError, InvalidSignature):
            return None
        if not isinstance(header, dict) or header.get("alg") != ALGORITHM:
            return None
        if not isinstance(claims, dict) or claims.get("iss") != self.issuer:
            return None
        clock = int(time.time()) if now is None else now
        exp = claims.get("exp")
        if not isinstance(exp, int) or exp <= clock:
            return None
        return claims

    def jwks(self) -> dict[str, Any]:
        """Returns the public key as a JWK Set, for `/.well-known/jwks.json`."""
        numbers = self.public.public_numbers()

        def as_b64(value: int) -> str:
            return self.b64url(value.to_bytes((value.bit_length() + 7) // 8, "big"))

        return {
            "keys": [
                {
                    "kty": "RSA",
                    "use": "sig",
                    "alg": ALGORITHM,
                    "kid": self.kid,
                    "n": as_b64(numbers.n),
                    "e": as_b64(numbers.e),
                }
            ]
        }
