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
import sys
import urllib.parse
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any, Final

from dreamference.accounts.accounts_jwt import KEY_FILE, AccountsJwt
from dreamference.accounts.accounts_oauth import AccountsOauth
from dreamference.accounts.accounts_store import AccountsStore
from dreamference.config.ports import ACCOUNTS_PORT, DEFAULT_MODEL_PORT
from dreamference.node.node_identity import NodeIdentity

LISTEN_HOST: Final[str] = "127.0.0.1"
MAX_BODY: Final[int] = 64 * 1024
REQUEST_TIMEOUT_S: Final[float] = 10.0
METHOD_HEAD: Final[str] = "HEAD"
CHUNKED: Final[str] = "chunked"


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
        return AccountsOauth(store, signer, log=cls.log)

    @staticmethod
    def log(line: str) -> None:
        """Writes one operational line to stderr (the journal under systemd)."""
        sys.stderr.write(line + "\n")
        sys.stderr.flush()

    @classmethod
    def handler_class(cls, oauth: AccountsOauth) -> type[BaseHTTPRequestHandler]:
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
        cls, oauth: AccountsOauth, request: BaseHTTPRequestHandler
    ) -> tuple[int, dict[str, str], dict[str, Any]]:
        """Reads the body (bounded) and asks the handler; a failure is a fixed-text 500."""
        headers = {name.lower(): value for name, value in request.headers.items()}
        if CHUNKED in headers.get("transfer-encoding", "").lower():
            return oauth.api_error(411, "length_required", "Send a Content-Length.")
        try:
            length = int(headers.get("content-length", "0") or 0)
        except ValueError:
            return oauth.api_error(400, "invalid_request", "Bad Content-Length.")
        if length < 0 or length > MAX_BODY:
            return oauth.api_error(413, "body_too_large", "The body is too large.")
        body = request.rfile.read(length) if length else b""
        try:
            return oauth.handle(request.command, request.path, headers, body)
        except Exception as error:  # noqa: BLE001 - one answer for anything unexpected
            cls.log(f"internal error: {type(error).__name__}")
            return oauth.api_error(500, "server_error", "The accounts service failed.")

    @classmethod
    def server(cls, oauth: AccountsOauth, port: int = ACCOUNTS_PORT) -> HTTPServer:
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
        args = parser.parse_args(argv)
        oauth = cls.build(AccountsStore(args.store, primary_name=args.primary_name))
        httpd = cls.server(oauth, args.port)
        cls.log(f"accounts service on {LISTEN_HOST}:{httpd.server_address[1]}")
        with contextlib.suppress(KeyboardInterrupt):
            httpd.serve_forever()
        httpd.server_close()
        return 0


if __name__ == "__main__":
    sys.exit(AccountsService.main())
