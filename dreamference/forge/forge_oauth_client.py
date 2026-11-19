"""The node's Forgejo as an OAuth2 client of the primary (specs/DREAMFERENCE_AUTH.md §14.2).

`ragon-admin` registers one client for the node's Forgejo (`AccountsForgejoClient`; the id and the
secret stay 0600 in the node's state directory) and adds the primary's gate as an authentication
source with `forgejo admin auth add-oauth --provider openidConnect`, pointed at the gate's
openid-configuration. A Ragon user then signs in to Forgejo by the authorization-code grant through
`ragon web`'s page. Forgejo links the account by the token's `sub` (the user id), so a rename on the
primary follows the same Forgejo account; the email is `<username>@<primary name>.local`.
Forgejo's local login stays on for `forge-admin`, `owner` and `mirror`.
"""

import re
from typing import Final

from dreamference.accounts.accounts_forgejo_client import AccountsForgejoClient
from dreamference.forge.forge_container import ForgeContainer
from dreamference.forge.forge_settings import ForgeSettings

SOURCE_NAME: Final[str] = "ragon"
PROVIDER: Final[str] = "openidConnect"
SCOPES: Final[str] = "openid profile email"
CALLBACK_PATH: Final[str] = f"/user/oauth2/{SOURCE_NAME}/callback"
DISCOVERY_PATH: Final[str] = "/.well-known/openid-configuration"
ID_SECRET: Final[str] = "oauth-client-id"
SECRET_SECRET: Final[str] = "oauth-client-secret"
EMAIL_SUFFIX: Final[str] = ".local"
LIST_ROW: Final[re.Pattern] = re.compile(r"^\s*(\d+)\s+(\S+)\s+")


class ForgeOauthClient:
    """Registers the node's Forgejo on the primary and adds the gate as its authentication source."""

    @classmethod
    def linked_email(cls, username: str, primary_name: str) -> str:
        """Return the email a Ragon user's Forgejo account carries.

        Args:
            username: The user's name on the primary.
            primary_name: The primary's host name (without `.local`).

        Returns:
            str: `<username>@<primary name>.local`.
        """
        return f"{username}@{primary_name}{EMAIL_SUFFIX}"

    @classmethod
    def callback_url(cls, forge_url: str) -> str:
        """Return the redirect URI Forgejo sends the browser back to.

        Args:
            forge_url: The forge's root URL as the browser reaches it.

        Returns:
            str: The callback URL of the authentication source.
        """
        return forge_url.rstrip("/") + CALLBACK_PATH

    @classmethod
    def register(cls, node_id: str, forge_url: str) -> tuple[AccountsForgejoClient, str | None]:
        """Register the client once; on later calls return the kept id and no new secret.

        The record is for the primary to store; the secret is written 0600 to the node's state
        directory and shown to nothing else.

        Args:
            node_id: The node's id.
            forge_url: The forge's root URL as the browser reaches it.

        Returns:
            tuple[AccountsForgejoClient, str | None]: The record, and the secret when this call
            made it (None when it was already kept).
        """
        uris = [cls.callback_url(forge_url)]
        record, secret = AccountsForgejoClient.register(node_id, uris)
        if (
            ForgeSettings.read_secret(SECRET_SECRET)
            and ForgeSettings.read_secret(ID_SECRET) == record.client_id
        ):
            return record, None
        ForgeSettings.write_secret(ID_SECRET, record.client_id)
        ForgeSettings.write_secret(SECRET_SECRET, secret)
        return record, secret

    @classmethod
    def source_id(cls) -> int | None:
        """Return the id of the `ragon` authentication source, if Forgejo has it.

        Returns:
            int | None: The source id, or None when it is not there.
        """
        result = ForgeContainer.forgejo(["admin", "auth", "list"], timeout=120)
        if result.returncode != 0:
            return None
        for line in (result.stdout or "").splitlines():
            match = LIST_ROW.match(line)
            if match and match.group(2) == SOURCE_NAME:
                return int(match.group(1))
        return None

    @classmethod
    def ensure_source(cls, issuer: str) -> bool:
        """Add the gate as an authentication source, or update it in place.

        Args:
            issuer: The primary gate's issuer URL (its openid-configuration is read from it).

        Returns:
            bool: Whether Forgejo took the source.
        """
        client_id = ForgeSettings.read_secret(ID_SECRET)
        secret = ForgeSettings.read_secret(SECRET_SECRET)
        if not client_id or not secret:
            return False
        existing = cls.source_id()
        arguments = [
            "admin",
            "auth",
            "update-oauth" if existing is not None else "add-oauth",
        ]
        if existing is not None:
            arguments += ["--id", str(existing)]
        else:
            arguments += ["--name", SOURCE_NAME]
        arguments += [
            "--provider",
            PROVIDER,
            "--key",
            client_id,
            "--secret",
            secret,
            "--auto-discover-url",
            issuer.rstrip("/") + DISCOVERY_PATH,
            "--scopes",
            SCOPES,
        ]
        return ForgeContainer.forgejo(arguments, timeout=120).returncode == 0
