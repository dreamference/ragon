"""The gate's port when it serves TLS: HTTPS and plain HTTP on one port (specs/DREAMFERENCE_AUTH.md §6).

Each accepted connection's first byte is read without taking it: a TLS handshake (0x16) is
served over TLS with the node's certificate, anything else as plain HTTP, as before, so 0.9.8
clients keep working. Both reach the same handler as `asyncio.start_server` would call it.

Runs inside the gate's container beside `model_gate_service.py`: standard library only.
"""

import asyncio
import contextlib
import importlib
import socket
import ssl
import sys
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any, Final

try:
    gate_tls = importlib.import_module("dreamference.vllm_server.model_gate_tls")
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    gate_tls = importlib.import_module("model_gate_tls")
GateTls = gate_tls.ModelGateTls

# An IPv6 address names its parts with colons, an IPv4 address or a host name never.
IPV6_SEPARATOR: Final[str] = ":"

Handler = Callable[[asyncio.StreamReader, asyncio.StreamWriter], Awaitable[None]]


class ModelGateListener(asyncio.AbstractServer):
    """A listening socket that serves each connection over TLS or plain HTTP, by its first byte."""

    def __init__(
        self, sock: socket.socket, handler: Handler, context: ssl.SSLContext, limit: int
    ) -> None:
        """Set up the listener; `open` starts it.

        Args:
            sock: The bound, listening, non-blocking socket.
            handler: Called with each connection's reader and writer.
            context: The server's TLS context.
            limit: The stream reader's buffer limit.
        """
        self.sock = sock
        self.handler = handler
        self.context = context
        self.limit = limit
        self.loop = asyncio.get_running_loop()
        self.closing = False
        self.connections: set[asyncio.Task[None]] = set()
        self.accepting: asyncio.Task[None] = self.loop.create_task(self.accept_loop())

    @classmethod
    async def open(
        cls, address: tuple[str, int], handler: Handler, context: ssl.SSLContext, limit: int
    ) -> "ModelGateListener":
        """Binds `address` and starts accepting.

        Args:
            address: Host and port (port 0 picks one).
            handler: Called with each connection's reader and writer.
            context: The server's TLS context.
            limit: The stream reader's buffer limit.

        Returns:
            ModelGateListener: The running listener.
        """
        host, port = address
        family = socket.AF_INET6 if IPV6_SEPARATOR in host else socket.AF_INET
        sock = socket.create_server((host, port), family=family, backlog=128)
        sock.setblocking(False)
        return cls(sock, handler, context, limit)

    @property
    def sockets(self) -> tuple[socket.socket, ...]:
        """The listening socket, as `asyncio.Server.sockets` names it."""
        return () if self.closing else (self.sock,)

    async def accept_loop(self) -> None:
        """Accepts connections until closed; each one is served by its own task."""
        while not self.closing:
            try:
                connection, _ = await self.loop.sock_accept(self.sock)
            except OSError:
                if self.closing:
                    return
                # Out of descriptors, or a connection reset before it was taken: try again.
                await asyncio.sleep(0.1)
                continue
            task = self.loop.create_task(self.serve_connection(connection))
            self.connections.add(task)
            task.add_done_callback(self.connections.discard)

    async def serve_connection(self, connection: socket.socket) -> None:
        """Tells TLS from plain HTTP by the first byte and hands the stream to the handler."""
        try:
            first = await asyncio.wait_for(GateTls.peek(connection), gate_tls.SNIFF_TIMEOUT_S)
            if not first:
                connection.close()
                return
            reader = asyncio.StreamReader(limit=self.limit)
            protocol = asyncio.StreamReaderProtocol(reader, self.handler)
            options: dict[str, Any] = (
                {"ssl": self.context, "ssl_handshake_timeout": gate_tls.HANDSHAKE_TIMEOUT_S}
                if GateTls.is_tls(first)
                else {}
            )
            await self.loop.connect_accepted_socket(lambda: protocol, connection, **options)
        except (OSError, TimeoutError, ssl.SSLError, ValueError):
            # A failed handshake or a silent client: nothing to answer.
            connection.close()

    def close(self) -> None:
        """Stops accepting; connections already open run to their end."""
        if self.closing:
            return
        self.closing = True
        self.accepting.cancel()
        self.sock.close()

    async def wait_closed(self) -> None:
        """Waits until the listener stopped accepting."""
        with contextlib.suppress(asyncio.CancelledError):
            await self.accepting

    def get_loop(self) -> asyncio.AbstractEventLoop:
        """The loop the listener runs in."""
        return self.loop

    def is_serving(self) -> bool:
        """True until closed."""
        return not self.closing

    async def start_serving(self) -> None:
        """Nothing to do: the listener serves from `open`."""

    async def serve_forever(self) -> None:
        """Serves until cancelled or closed."""
        try:
            await self.accepting
        finally:
            self.close()
