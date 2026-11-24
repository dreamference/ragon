"""The primary's accounts service on 127.0.0.1:18100 (specs/DREAMFERENCE_AUTH.md §5, §9).

A small HTTP/1.0 server around `AccountsOauth`, run with
`python -m dreamference.accounts.accounts_service`. It binds loopback only; the gate on port 8000
forwards the auth routes to it. Requests are served one at a time, which also bounds the memory
argon2 takes. A log line names the method, the path without its query and
the status, never a body or a header.
"""

import argparse
import contextlib
import json
import shutil
import subprocess
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any, Final

from dreamference.accounts.accounts_jwt import KEY_FILE, AccountsJwt
from dreamference.accounts.accounts_oauth import AccountsOauth
from dreamference.accounts.accounts_secondary import AccountsSecondary
from dreamference.accounts.accounts_store import AccountsStore
from dreamference.config.ports import ACCOUNTS_PORT, DEFAULT_MODEL_PORT
from dreamference.node.node_identity import NodeIdentity

LISTEN_HOST: Final[str] = "127.0.0.1"
MAX_BODY: Final[int] = 64 * 1024
REQUEST_TIMEOUT_S: Final[float] = 10.0
METHOD_HEAD: Final[str] = "HEAD"
CHUNKED: Final[str] = "chunked"
# The user unit `server start` keeps running (specs/DREAMFERENCE_AUTH.md §9): nothing else starts
# the service, and a gate whose info says `accounts` must have it behind its sign-in routes.
UNIT: Final[str] = "ragon-accounts.service"
SYSTEMCTL_TIMEOUT_S: Final[float] = 30.0


class AccountsService:
    """Serves `AccountsOauth` over HTTP on loopback."""

    @classmethod
    def issuer(cls, primary_name: str) -> str:
        """The primary's issuer URL, `https://<primary>.local:8000`."""
        return f"https://{primary_name}.local:{DEFAULT_MODEL_PORT}"

    @classmethod
    def build(cls, store: AccountsStore, key_path: Path | None = None) -> AccountsOauth:
        """Builds the handler for a store, its key beside the store unless named.

        Args:
            store: The account store.
            key_path: The signing key; defaults to `signing-key.pem` beside the store.

        Returns:
            AccountsOauth: The handler.
        """
        signer = AccountsJwt(
            key_path or store.path.parent / KEY_FILE,
            cls.issuer(store.primary_name),
            NodeIdentity.read() or "",
            store.primary_name,
        )
        return AccountsOauth(store, signer, log=cls.log, on_first=lambda: cls.first_account(store))

    @classmethod
    def first_account(cls, store: AccountsStore) -> None:
        """After the node's first account, this node becomes the primary.

        As `node migrate` makes it: `auth=primary` advertised and its service account registered,
        so phones and the other nodes find it (specs/DREAMFERENCE_AUTH.md §8.6).
        """
        from dreamference.accounts.accounts_migrate import AccountsMigrate

        AccountsMigrate.migrate(store, NodeIdentity.read() or "", [])
        cls.log("first account made: this node is the primary")

    # ----- the user unit -----------------------------------------------------------------

    @classmethod
    def unit_dir(cls) -> Path:
        """The user's systemd units, `~/.config/systemd/user`."""
        return Path("~/.config/systemd/user").expanduser()

    @classmethod
    def unit_text(cls) -> str:
        """The unit: this interpreter runs the service; a secondary's forwards to its primary."""
        from dreamference.config.unit_log_directives import UnitLogDirectives
        from dreamference.node.node_settings import NodeSettings

        command = f"{sys.executable} -m dreamference.accounts.accounts_service"
        if NodeSettings.pinned_primary():
            command += " --secondary"
        return (
            "[Unit]\n"
            "Description=Ragon accounts service (sign-in behind the model gate)\n"
            "After=network-online.target\n\n"
            "[Service]\n" + UnitLogDirectives.block("gate") + f"ExecStart={command}\n"
            "Restart=on-failure\n"
            "RestartSec=10\n"
            "MemoryMax=256M\n\n"
            "[Install]\n"
            "WantedBy=default.target\n"
        )

    @classmethod
    def systemctl(cls, *args: str) -> int:
        """Runs `systemctl --user`; its exit status, 1 when it could not run."""
        try:
            done = subprocess.run(
                ["systemctl", "--user", *args],
                capture_output=True,
                text=True,
                timeout=SYSTEMCTL_TIMEOUT_S,
                check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return 1
        return done.returncode

    @classmethod
    def ensure_running(cls) -> bool:
        """Writes the unit, enables it and (re)starts it, as `server start` replaces the gate.

        Restarted on every call: the unit names only the interpreter, so after `ragon update` its
        text is unchanged while the package it runs is new.

        Called by `server start` on a node with accounts on. A machine without systemd (a Mac node)
        gets a line naming the command that runs the service by hand.

        Returns:
            bool: True when systemd accepted the start.
        """
        if shutil.which("systemctl") is None:
            print(  # noqa: T201 - console output of server start
                "⚠️  No systemd here: run `python -m dreamference.accounts.accounts_service` "
                "for sign-in to work."
            )
            return False
        folder = cls.unit_dir()
        folder.mkdir(parents=True, exist_ok=True)
        unit = folder / UNIT
        text = cls.unit_text()
        changed = not unit.is_file() or unit.read_text() != text
        if changed:
            unit.write_text(text)
            cls.systemctl("daemon-reload")
        started = cls.systemctl("enable", UNIT) == 0 and cls.systemctl("restart", UNIT) == 0
        if not started:
            print(f"⚠️  The accounts service did not start: systemctl --user status {UNIT}")  # noqa: T201
        return started

    @classmethod
    def follow_role(cls) -> bool:
        """After `node primary` changed the role: the unit rewritten, a running service restarted.

        `server start` writes the unit with the role of that moment, so a node pinned after it
        kept answering sign-ins from its own empty store (the w48s test pair, 11-24). A node
        whose `server start` never wrote the unit is left alone (the next start writes it), and
        a stopped service stays stopped (`try-restart`), as `server stop` left it.

        Returns:
            bool: True when systemd took the `try-restart` (a stopped service stays stopped).
        """
        unit = cls.unit_dir() / UNIT
        if shutil.which("systemctl") is None or not unit.is_file():
            return False
        text = cls.unit_text()
        if unit.read_text() != text:
            unit.write_text(text)
            cls.systemctl("daemon-reload")
        return cls.systemctl("try-restart", UNIT) == 0

    @classmethod
    def stop(cls) -> bool:
        """Stops and disables the unit, as `server stop` stops the gate it answers behind.

        Disabled too: a stopped gate's container stays stopped at boot, so the service does.

        Returns:
            bool: True when there was no unit or systemd accepted the stop.
        """
        if shutil.which("systemctl") is None or not (cls.unit_dir() / UNIT).is_file():
            return True
        return cls.systemctl("disable", "--now", UNIT) == 0

    @classmethod
    def build_secondary(
        cls, primary_url: str | None = None, ca_file: Path | None = None
    ) -> AccountsSecondary:
        """Builds the secondary's handler for the pinned primary (§7).

        Args:
            primary_url: The primary's gate; found from its advertisement when None.
            ca_file: The primary's pinned CA; `~/.ragon/nodes/<id>/ca.pem` when it exists.

        Returns:
            AccountsSecondary: The handler.

        Raises:
            SystemExit: No primary is pinned, or its advertisement is not on the network.
        """
        from dreamference.node.node_settings import NodeSettings

        node_id = NodeSettings.pinned_primary()
        if primary_url is None:
            if node_id is None:
                raise SystemExit("No primary is pinned: run `ragon-admin node primary <node id>`.")
            primary_url = AccountsSecondary.find_primary(node_id)
            if primary_url is None:
                raise SystemExit(f"The primary {node_id} is not advertised on this network.")
        if ca_file is None and node_id:
            pinned = AccountsSecondary.ca_path(node_id)
            ca_file = pinned if pinned.is_file() else None
        if ca_file is None:
            cls.log(
                f"no CA pinned for the primary {node_id}: run `ragon-admin node primary {node_id}`"
                " again while it is up"
            )
        return AccountsSecondary(primary_url, ca_file)

    @staticmethod
    def log(line: str) -> None:
        """Writes one operational line to stderr (the journal under systemd)."""
        sys.stderr.write(line + "\n")
        sys.stderr.flush()

    @classmethod
    def handler_class(
        cls, oauth: "AccountsOauth | AccountsSecondary"
    ) -> type[BaseHTTPRequestHandler]:
        """A request handler class bound to `oauth`."""

        class Handler(BaseHTTPRequestHandler):
            server_version = "ragon-accounts"
            sys_version = ""
            timeout = REQUEST_TIMEOUT_S

            def log_message(self, format: str, *args: object) -> None:  # noqa: A002
                """Silences the default log, which would print the query."""

            def dispatch(self) -> None:
                route = urllib.parse.urlsplit(self.path).path
                status, headers, body = cls.answer(oauth, self)
                data = json.dumps(body).encode("utf-8")
                self.send_response_only(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("Connection", "close")
                for name, value in headers.items():
                    self.send_header(name, value)
                self.end_headers()
                if self.command != METHOD_HEAD:
                    self.wfile.write(data)
                cls.log(f"{self.command} {route} {status}")

            do_GET = do_POST = do_DELETE = do_PUT = do_PATCH = do_HEAD = dispatch  # noqa: N815

        return Handler

    @classmethod
    def answer(
        cls, oauth: "AccountsOauth | AccountsSecondary", request: BaseHTTPRequestHandler
    ) -> tuple[int, dict[str, str], dict[str, Any]]:
        """Reads the body (bounded) and asks the handler; a failure is a fixed-text 500."""
        headers = {name.lower(): value for name, value in request.headers.items()}
        if CHUNKED in headers.get("transfer-encoding", "").lower():
            return AccountsOauth.api_error(411, "length_required", "Send a Content-Length.")
        try:
            length = int(headers.get("content-length", "0") or 0)
        except ValueError:
            return AccountsOauth.api_error(400, "invalid_request", "Bad Content-Length.")
        if length < 0 or length > MAX_BODY:
            return AccountsOauth.api_error(413, "body_too_large", "The body is too large.")
        body = request.rfile.read(length) if length else b""
        try:
            return oauth.handle(request.command, request.path, headers, body)
        except Exception as error:  # noqa: BLE001 - one answer for anything unexpected
            cls.log(f"internal error: {type(error).__name__}")
            return AccountsOauth.api_error(500, "server_error", "The accounts service failed.")

    @classmethod
    def server(
        cls, oauth: "AccountsOauth | AccountsSecondary", port: int = ACCOUNTS_PORT
    ) -> HTTPServer:
        """An HTTP server on 127.0.0.1:`port`, not yet serving (port 0 picks one)."""
        return HTTPServer((LISTEN_HOST, port), cls.handler_class(oauth))

    @classmethod
    def main(cls, argv: list[str] | None = None) -> int:
        """Runs the service until interrupted.

        Args:
            argv: The arguments; defaults to the command line.

        Returns:
            int: The exit status.
        """
        parser = argparse.ArgumentParser(prog="dreamference.accounts.accounts_service")
        parser.add_argument("--port", type=int, default=ACCOUNTS_PORT)
        parser.add_argument("--store", type=Path, default=None, help="the accounts.db file")
        parser.add_argument("--primary-name", default=None)
        parser.add_argument(
            "--secondary", action="store_true", help="forward to the pinned primary"
        )
        parser.add_argument("--primary-url", default=None, help="the pinned primary's gate")
        args = parser.parse_args(argv)
        from dreamference.node.node_settings import NodeSettings

        # The pin decides, not only the flag: a unit written before `node primary` must not
        # answer sign-ins from a local store a secondary never keeps (§7).
        if args.secondary or (args.store is None and NodeSettings.pinned_primary()):
            oauth: AccountsOauth | AccountsSecondary = cls.build_secondary(args.primary_url)
        else:
            oauth = cls.build(AccountsStore(args.store, primary_name=args.primary_name))
        httpd = cls.server(oauth, args.port)
        cls.log(f"accounts service on {LISTEN_HOST}:{httpd.server_address[1]}")
        with contextlib.suppress(KeyboardInterrupt):
            httpd.serve_forever()
        httpd.server_close()
        return 0


if __name__ == "__main__":
    sys.exit(AccountsService.main())
