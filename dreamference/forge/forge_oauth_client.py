"""The node's Forgejo as an OAuth2 client of the primary (specs/DREAMFERENCE_AUTH.md §14.2).

`ragon-admin` registers one client for the node's Forgejo (`AccountsForgejoClient`; the id and the
secret stay 0600 in the node's state directory) and adds the primary's gate as an authentication
source with `forgejo admin auth add-oauth --provider openidConnect`, pointed at the gate's
openid-configuration. A Ragon user then signs in to Forgejo by the authorization-code grant through
`ragon web`'s page. Forgejo links the account by the token's `sub` (the user id), so a rename on the
primary follows the same Forgejo account; the email is `<username>@<primary name>.local`.
Forgejo's local login stays on for `forge-admin`, `owner` and `mirror`.
"""

import hashlib
import json
import re
import socket
import urllib.error
import urllib.parse
import urllib.request
from http import HTTPStatus
from pathlib import Path
from typing import Any, Final

from dreamference.accounts.accounts_forgejo_client import CLIENT_ID_PREFIX, AccountsForgejoClient
from dreamference.config.ports import ACCOUNTS_PORT
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
# The node's own accounts service: the primary answers itself, a secondary forwards (§5.1).
ACCOUNTS_URL: Final[str] = f"http://127.0.0.1:{ACCOUNTS_PORT}"
REGISTER_PATH: Final[str] = "/ragon/v1/nodes/forgejo"
HTTP_TIMEOUT_S: Final[float] = 10.0
# Inside the container: the CA that signs the primary's gate, beside the image's own roots.
CA_MOUNT_DIR: Final[str] = "/etc/ragon-ca"
CA_MOUNT_FILE: Final[str] = f"{CA_MOUNT_DIR}/ca.pem"
CERT_DIRS: Final[str] = f"/etc/ssl/certs:{CA_MOUNT_DIR}"
STATE_ISSUER: Final[str] = "oauth_issuer"
STATE_CLIENT: Final[str] = "oauth_client_id"
IPV6_MARK: Final[str] = ":"
LOOPBACK: Final[tuple[str, ...]] = ("127.", "::1")
LOCAL_SIGN_IN: Final[str] = "Forgejo keeps its local sign-in until then."


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
        kept = ForgeSettings.read_secret(SECRET_SECRET)
        client_id = f"{CLIENT_ID_PREFIX}{node_id}"
        if kept and ForgeSettings.read_secret(ID_SECRET) == client_id:
            # The record must hash the kept secret, the one Forgejo sends.
            digest = hashlib.sha256(kept.encode()).hexdigest()
            return AccountsForgejoClient(client_id, digest, tuple(uris)), None
        record, secret = AccountsForgejoClient.register(node_id, uris)
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

    @classmethod
    def local_json(
        cls, path: str, payload: dict[str, Any] | None = None, key: str | None = None
    ) -> dict[str, Any]:
        """Call the node's own accounts service and read its JSON answer.

        Args:
            path: The route.
            payload: A JSON body to POST; None for a GET.
            key: A Bearer credential, if the route needs one.

        Returns:
            dict[str, Any]: The answer.

        Raises:
            urllib.error.HTTPError: The service refused.
            OSError: The service (or, behind a secondary, its primary) could not be reached.
            ValueError: The answer was not a JSON object.
        """
        data = None if payload is None else json.dumps(payload).encode()
        request = urllib.request.Request(ACCOUNTS_URL + path, data=data)
        if data is not None:
            request.add_header("Content-Type", "application/json")
        if key:
            request.add_header("Authorization", f"Bearer {key}")
        with urllib.request.urlopen(request, timeout=HTTP_TIMEOUT_S) as reply:
            answer = json.loads(reply.read().decode())
        if not isinstance(answer, dict):
            raise ValueError("not a JSON object")
        return answer

    @classmethod
    def issuer(cls) -> str | None:
        """Return the primary's issuer, read from the node's own discovery document.

        Returns:
            str | None: The issuer URL, or None when the primary could not be asked.
        """
        try:
            issuer = cls.local_json(DISCOVERY_PATH).get("issuer")
        except (OSError, ValueError):
            return None
        return issuer if isinstance(issuer, str) and issuer.startswith("https://") else None

    @classmethod
    def put_client(cls, node_id: str, record: AccountsForgejoClient) -> str | None:
        """Store the client on the primary: in its own store here, else through the gate.

        Args:
            node_id: This node's id.
            record: The client record (the hash of the secret, never the secret).

        Returns:
            str | None: None when the primary has the record, else why it does not.
        """
        from dreamference.node.node_settings import NodeSettings
        from dreamference.node.service_key import ServiceKey

        if NodeSettings.pinned_primary() is None:
            from dreamference.accounts.accounts_store import AccountsStore

            AccountsStore().put_forgejo_client(node_id, record)
            return None
        key = ServiceKey.read()
        if not key:
            return "this node has no service key (~/.config/dreamference/service-key)"
        payload = {
            "node_id": node_id,
            "secret_hash": record.secret_hash,
            "redirect_uris": list(record.redirect_uris),
        }
        try:
            cls.local_json(REGISTER_PATH, payload, key)
        except urllib.error.HTTPError as error:
            if error.code == HTTPStatus.FORBIDDEN:
                return "the primary refused the client (HTTP 403): an admin registers it there"
            return f"the primary refused the client (HTTP {error.code})"
        except (OSError, ValueError):
            return "the primary could not be reached"
        return None

    @classmethod
    def ensure(cls, node_id: str, forge_url: str) -> tuple[list[str], bool]:
        """Register the client on the primary and keep Forgejo's `ragon` source in line.

        Safe to run at every start and tick: the client record is written only when it changed,
        and Forgejo is touched only when the issuer or the client id changed.

        Args:
            node_id: This node's id.
            forge_url: The forge's root URL as the browser reaches it.

        Returns:
            tuple[list[str], bool]: Lines to print, and whether the forge must restart to load
            a new or changed source.
        """
        record, _ = cls.register(node_id, forge_url)
        issuer = cls.issuer()
        if issuer is None:
            return [
                "⚠️  Ragon sign-in to the forge is not set up: the primary could not be reached. "
                + LOCAL_SIGN_IN
            ], False
        problem = cls.put_client(node_id, record)
        if problem is not None:
            return [
                f"⚠️  Ragon sign-in to the forge is not set up: {problem}. {LOCAL_SIGN_IN}"
            ], False
        state = ForgeSettings.state()
        if state.get(STATE_ISSUER) == issuer and state.get(STATE_CLIENT) == record.client_id:
            return [], False
        if not cls.ensure_source(issuer):
            return [
                f"⚠️  Forgejo did not take the `{SOURCE_NAME}` sign-in source. {LOCAL_SIGN_IN}"
            ], False
        ForgeSettings.update_state(**{STATE_ISSUER: issuer, STATE_CLIENT: record.client_id})
        host = urllib.parse.urlsplit(issuer).hostname or issuer
        return [f"🔑 The forge offers sign-in with a Ragon account of {host}."], True

    @classmethod
    def ca_file(cls) -> Path | None:
        """Return the CA that signs the primary's gate: the pinned one, or this node's own.

        Returns:
            Path | None: The file, or None when it is not there.
        """
        from dreamference.accounts.accounts_secondary import AccountsSecondary
        from dreamference.node.node_settings import NodeSettings
        from dreamference.vllm_server.model_gate_tls import CA_CERT, ModelGateTls

        pinned = NodeSettings.pinned_primary()
        path = AccountsSecondary.ca_path(pinned) if pinned else ModelGateTls.directory() / CA_CERT
        return path if path.is_file() else None

    @classmethod
    def resolve(cls, host: str) -> str | None:
        """Resolve a name on the host (the container cannot resolve `.local`), IPv4 first.

        Args:
            host: The name.

        Returns:
            str | None: An address, or None when the name does not resolve.
        """
        try:
            found = socket.getaddrinfo(host, None, proto=socket.IPPROTO_TCP)
        except OSError:
            return None
        addresses = sorted({str(info[4][0]) for info in found}, key=lambda a: (IPV6_MARK in a, a))
        usable = [a for a in addresses if not a.startswith(LOOPBACK)]
        return usable[0] if usable else None

    @classmethod
    def container_arguments(
        cls,
    ) -> tuple[list[tuple[str, str]], list[tuple[str, str]], dict[str, str]]:
        """Return what the container needs to reach the issuer kept in the forge's state.

        Returns:
            tuple: (`--add-host` name and address pairs, read-only mounts, environment).
        """
        issuer = ForgeSettings.state().get(STATE_ISSUER)
        if not isinstance(issuer, str) or not issuer:
            return [], [], {}
        hosts: list[tuple[str, str]] = []
        name = urllib.parse.urlsplit(issuer).hostname
        address = cls.resolve(name) if name else None
        if name and address:
            hosts.append((name, address))
        ca = cls.ca_file()
        if ca is None:
            return hosts, [], {}
        return hosts, [(str(ca), CA_MOUNT_FILE)], {"SSL_CERT_DIR": CERT_DIRS}
