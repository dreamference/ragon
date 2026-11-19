"""The OAuth client registered on the primary for a node's Forgejo (specs/DREAMFERENCE_AUTH.md §5.4, §14.2).

The authorization-code grant is offered to this client only (and to the codes the device page
approves, which `AccountsDevice` holds). The record holds the client id, the SHA-256 of its secret
and the redirect URIs it may use; the secret is shown once at registration and never kept.
"""

import hashlib
import hmac
import secrets
from dataclasses import dataclass, field
from typing import Final

CLIENT_ID_PREFIX: Final[str] = "forgejo-"


@dataclass(frozen=True)
class AccountsForgejoClient:
    """One registered Forgejo client.

    Attributes:
        client_id: The id Forgejo sends.
        secret_hash: Hex SHA-256 of the client secret.
        redirect_uris: The callback URLs the client may use, compared exactly.
    """

    client_id: str
    secret_hash: str
    redirect_uris: tuple[str, ...] = field(default_factory=tuple)

    @classmethod
    def register(
        cls, node_id: str, redirect_uris: list[str]
    ) -> tuple["AccountsForgejoClient", str]:
        """Makes a client for a node's Forgejo.

        Args:
            node_id: The node the Forgejo belongs to.
            redirect_uris: The callback URLs of its authentication source.

        Returns:
            tuple[AccountsForgejoClient, str]: The record and the secret (shown once).
        """
        secret = secrets.token_urlsafe(32)
        record = cls(
            f"{CLIENT_ID_PREFIX}{node_id}",
            hashlib.sha256(secret.encode()).hexdigest(),
            tuple(redirect_uris),
        )
        return record, secret

    def secret_matches(self, secret: str) -> bool:
        """True when `secret` is this client's secret (constant time)."""
        digest = hashlib.sha256(secret.encode()).hexdigest()
        return hmac.compare_digest(digest, self.secret_hash)

    def allows(self, redirect_uri: str) -> bool:
        """True when `redirect_uri` is one of the registered callbacks."""
        return redirect_uri in self.redirect_uris
