"""The egress audit's sign-in trace (specs/DREAMFERENCE_AUTH.md §16).

This module provides the AuthTrace class. Sign-in goes to the resolved node's gate, or to the gate
of the pinned primary when the node has none. The allowlist gains exactly that second address, and
only when the certificate it presents chains to the pinned CA. The trace then runs `ragon login`,
`ragon login status`, a token refresh and `ragon logout` (whose revoke is the last call) in a
throwaway home with strace and reports every address they reached.
"""

import base64
import json
import socket
import ssl
import subprocess
import uuid
from pathlib import Path
from typing import Any, Final
from urllib.parse import urlparse
from urllib.request import urlopen

from dreamference.audit.egress_trace import EgressTrace
from dreamference.audit.strace_parser import StraceParser

INFO_PATH: Final[str] = "/ragon/v1/info"
CA_PATH: Final[str] = "/ragon/v1/ca.pem"
STEP_TIMEOUT_S: Final[int] = 60
HANDSHAKE_TIMEOUT_S: Final[float] = 5.0
HTTPS_PORT: Final[int] = 443
# The traced syscalls: the connections and names a login makes (the strace flags of EgressAudit).
TRACED_SYSCALLS: Final[str] = "connect,sendto,sendmsg,sendmmsg,execve,write,writev"
# An access token that expired long ago, unsigned: Codex reads the claims without checking them, so
# it makes the next call refresh (specs/DREAMFERENCE_AUTH.md §5.3).
EXPIRED_AT: Final[int] = 1
REFRESH: Final[str] = "refresh"


class AuthTrace:
    """Admits the primary's gate and traces the four sign-in commands."""

    @classmethod
    def fetch_info(cls, gate: str, ca_pem: bytes | None = None) -> dict[str, Any]:
        """Read `/ragon/v1/info` from a gate.

        Args:
            gate: The gate's base URL.
            ca_pem: The pinned CA, when the gate serves HTTPS.

        Returns:
            dict[str, Any]: The document, empty when the gate did not answer with one.
        """
        context = ssl.create_default_context(cadata=ca_pem.decode()) if ca_pem else None
        try:
            with urlopen(
                gate.rstrip("/") + INFO_PATH, timeout=HANDSHAKE_TIMEOUT_S, context=context
            ) as page:
                document = json.loads(page.read())
        except (OSError, ValueError):
            return {}
        return document if isinstance(document, dict) else {}

    @classmethod
    def primary_gate(cls, info: dict[str, Any], node_gate: str) -> tuple[str, int] | None:
        """The pinned primary's gate, when it is not the resolved node's own.

        Args:
            info: The resolved node's info document (`issuer` names the primary's gate).
            node_gate: The resolved node's gate URL.

        Returns:
            tuple[str, int] | None: Host and port, or None when the node is the primary or the
            document names none.
        """
        issuer = info.get("issuer")
        if not isinstance(issuer, str) or not issuer:
            return None
        parsed = urlparse(issuer)
        node = urlparse(node_gate)
        if not parsed.hostname:
            return None
        port = parsed.port or HTTPS_PORT
        if (parsed.hostname, port) == (node.hostname, node.port or HTTPS_PORT):
            return None
        return parsed.hostname, port

    @classmethod
    def chains_to_ca(cls, host: str, port: int, ca_pem: bytes) -> bool:
        """Whether the certificate at `host:port` chains to the pinned CA.

        Only the chain counts (the CA is the only trust root); a name mismatch does not.

        Args:
            host: The gate's host.
            port: The gate's port.
            ca_pem: The pinned CA certificate.

        Returns:
            bool: True when the handshake verified against that CA alone.
        """
        try:
            context = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
            context.load_verify_locations(cadata=ca_pem.decode())
            context.check_hostname = False
            context.verify_mode = ssl.CERT_REQUIRED
            with (
                socket.create_connection((host, port), timeout=HANDSHAKE_TIMEOUT_S) as raw,
                context.wrap_socket(raw, server_hostname=host),
            ):
                return True
        except (OSError, ValueError, UnicodeDecodeError):
            return False

    @classmethod
    def admitted(
        cls, info: dict[str, Any], node_gate: str, ca_pem: bytes | None
    ) -> tuple[frozenset[str], tuple[str, ...]]:
        """The addresses and names the primary's gate adds to the allowlist.

        Args:
            info: The resolved node's info document.
            node_gate: The resolved node's gate URL.
            ca_pem: The pinned CA; without one nothing is admitted.

        Returns:
            tuple[frozenset[str], tuple[str, ...]]: `addr:port` destinations and the primary's host
            name (which a lookup may ask for); both empty unless the primary has its own gate and
            its certificate chains to the pinned CA.
        """
        gate = cls.primary_gate(info, node_gate)
        if gate is None or ca_pem is None or not cls.chains_to_ca(gate[0], gate[1], ca_pem):
            return frozenset(), ()
        host, port = gate
        try:
            addresses = {
                item[4][0] for item in socket.getaddrinfo(host, port, type=socket.SOCK_STREAM)
            }
        except OSError:
            return frozenset(), ()
        return frozenset(f"{address}:{port}" for address in addresses), (host.lower(),)

    @classmethod
    def steps(cls, username: str) -> list[tuple[str, list[str], str]]:
        """The four commands, each as a label, its arguments and its standard input.

        Args:
            username: The throwaway account the first one creates (an empty password is accepted).

        Returns:
            list[tuple[str, list[str], str]]: Login, status, refresh and logout.
        """
        return [
            ("login", ["login", "--username", username, "--create", "--password-stdin"], "\n"),
            ("status", ["login", "status"], ""),
            (REFRESH, ["login", "status"], ""),
            ("logout", ["logout"], ""),
        ]

    @classmethod
    def expire_access_token(cls, codex_home: str) -> bool:
        """Make the stored access token stale, so the next call refreshes it.

        Args:
            codex_home: The throwaway `CODEX_HOME`.

        Returns:
            bool: True when an entry was changed.
        """
        path = Path(codex_home, "auth.json")
        try:
            document = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return False
        changed = False
        entries = document.get("accounts") if isinstance(document.get("accounts"), dict) else {}
        for entry in [document, *entries.values()]:
            tokens = entry.get("tokens") if isinstance(entry, dict) else None
            if isinstance(tokens, dict) and tokens.get("access_token"):
                tokens["access_token"] = _expired_jwt()
                changed = True
        if changed:
            path.write_text(json.dumps(document), encoding="utf-8")
        return changed

    @classmethod
    def trace(
        cls, ragon_bin: str, env: dict[str, str], work_dir: str
    ) -> tuple[EgressTrace, dict[str, int]]:
        """Run the four commands under strace and parse what they connected to.

        Args:
            ragon_bin: The `ragon` under test.
            env: Its environment (throwaway `CODEX_HOME`, the model server, `RAGON_API_KEY`).
            work_dir: A scratch directory.

        Returns:
            tuple[EgressTrace, dict[str, int]]: The merged trace and each step's exit code.
        """
        username = f"audit-{uuid.uuid4().hex[:8]}"
        text = ""
        codes: dict[str, int] = {}
        for label, args, stdin in cls.steps(username):
            if label == REFRESH:
                cls.expire_access_token(env["CODEX_HOME"])
            path = str(Path(work_dir, f"auth-{label}.txt"))
            command = [
                "strace",
                "-f",
                "-qq",
                "-yy",
                "-e",
                f"trace={TRACED_SYSCALLS}",
                "-s",
                "256",
                "-o",
                path,
                ragon_bin,
                *args,
            ]
            try:
                done = subprocess.run(
                    command,
                    input=stdin,
                    text=True,
                    env=env,
                    capture_output=True,
                    timeout=STEP_TIMEOUT_S,
                    check=False,
                )
                codes[label] = done.returncode
            except (OSError, subprocess.TimeoutExpired):
                codes[label] = -1
            if Path(path).is_file():
                text += Path(path).read_text(errors="replace") + "\n"
        return StraceParser.parse(text), codes


def _expired_jwt() -> str:
    """An unsigned token whose `exp` is long past."""

    def part(value: dict[str, Any]) -> str:
        return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")

    return f"{part({'alg': 'none'})}.{part({'exp': EXPIRED_AT})}."
