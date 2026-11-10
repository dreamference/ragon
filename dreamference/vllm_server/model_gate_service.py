"""The model gate: the one door in front of the model server (specs/DREAMFERENCE_RAGON_SWE_BENCH.md §18).

This file is executed *inside* the gate's container, not on the host: `ModelGate` bind-mounts a
copy of it into the model server's own image (which carries a Python) and runs it there, as
`DiffusionServerManager` does with its service. It is imported on the host too (by the tests and
by `ModelGate` for the state it reports), so it uses the standard library only.

The engine listens on loopback at an internal port; this proxy owns the public port, on every
interface, as the engine did. Every request passes through (answers streamed, never buffered)
unless a SWE-bench run holds the gate: then a request that is not the run's is answered
503 in the API's own error shape, naming the run and its ETA, and never reaches the engine.

A `/v1/responses` body is read whole and passes `model_gate_guard.py`, which turns what `ragon`'s loop
guard asks in the body's `client_metadata` (a length cap, a sampling arm) into the request's own
fields where the body sets none; a body that asks for nothing goes on byte for byte.

A request is the run's when it comes from the run's internal Docker network: its containers reach
the model at that network's gateway, so their source address is in the network's subnet, which
the run writes into its record. No other client can send from there without the Docker socket.

The state is two files the host writes and this process only reads, on every request:
`run.json` (the run's record, refreshed by a heartbeat) and `pause.json` (`ragon-admin night
pause`). A missing, unreadable or stale run record means the gate is open, so a run that dies
without cleaning up cannot keep the model closed: a heartbeat older than `STALE_S` is ignored.
"""

import argparse
import asyncio
import contextlib
import importlib
import ipaddress
import json
import os
import sys
import time
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any, Final

# The adapter rules, copied beside this file into the gate's container (`ModelGate.start`), where
# the package is not installed and the interpreter may run isolated (`-I`), so the copy is found by
# this file's own folder.
try:
    gate_adapters = importlib.import_module("dreamference.vllm_server.model_gate_adapters")
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    gate_adapters = importlib.import_module("model_gate_adapters")
GateAdapters = gate_adapters.ModelGateAdapters
# The loop guard's rules (what `ragon` asks in a request's client_metadata), copied the same way.
try:
    gate_guard = importlib.import_module("dreamference.vllm_server.model_gate_guard")
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    gate_guard = importlib.import_module("model_gate_guard")
GateGuard = gate_guard.ModelGateGuard

# Answered by the gate itself, never forwarded: what the gate is doing, as JSON.
PROBE_PATH: Final[str] = "/ragon-gate"

# Methods, framings and status values the code compares against.
METHOD_GET: Final[str] = "GET"
METHOD_HEAD: Final[str] = "HEAD"
STATE_CLOSED: Final[str] = "closed"
KEY_PAUSED_UNTIL: Final[str] = "paused_until"
FRAMING_CHUNKED: Final[str] = "chunked"
FRAMING_LENGTH: Final[str] = "length"
CHUNKED_TOKEN: Final[bytes] = b"chunked"
EXPECT_CONTINUE: Final[bytes] = b"100-continue"
CRLF: Final[bytes] = b"\r\n"
URL_SCHEME_SEP: Final[str] = "://"
SECONDS_PER_MINUTE: Final[int] = 60
SHORT_ETA_MAX_HOURS: Final[int] = 3
SHORT_ETA_MIN_MINUTES: Final[int] = 5
REQUEST_LINE_PARTS: Final[int] = 3
HTTP_CONTINUE: Final[int] = 100
HTTP_SWITCHING_PROTOCOLS: Final[int] = 101
HTTP_OK_FLOOR: Final[int] = 200

RUN_FILE: Final[str] = "run.json"
PAUSE_FILE: Final[str] = "pause.json"

# A run's record counts only while its heartbeat is this recent (the run refreshes it every 15 s).
STALE_S: Final[float] = 180.0

# Read-only probes pass even while the gate is closed: the launcher reads /v1/models before its
# first turn, and the run's own admission, Night Shift and `ragon-code` read /metrics. None of them
# runs the model. `/health_generate` does, and is not here.
READ_ONLY_PATHS: Final[tuple[str, ...]] = (
    "/health",
    "/v1/models",
    "/metrics",
    "/get_model_info",
    "/get_server_info",
    "/server_info",
    "/model_info",
    "/version",
    "/ping",
    PROBE_PATH,
)

HEAD_LIMIT: Final[int] = 1 << 20
BUFFER: Final[int] = 1 << 16
UPSTREAM_CONNECT_TIMEOUT_S: Final[float] = 10.0
# A refused request's body is read before the answer, so closing never resets the connection
# under the client (which would lose the 503 and its message); bounded in size and time.
DRAIN_LIMIT: Final[int] = 64 << 20
DRAIN_TIMEOUT_S: Final[float] = 30.0

REFUSAL_CODE: Final[str] = "benchmark_running"
# Codex retries a 5xx about 30 times and honours Retry-After with no upper bound: 0 makes those
# retries immediate, so the message reaches the user in a moment instead of after ~25 s of
# backoff. Never a real delay: an hour here would hang a turn for a day.
RETRY_AFTER: Final[str] = "0"

DROPPED_REQUEST_HEADERS: Final[tuple[bytes, ...]] = (
    b"connection",
    b"keep-alive",
    b"proxy-connection",
)
# Replaced when the gate sends a body of its own, or the engine's key.
FRAMING_HEADERS: Final[tuple[bytes, ...]] = (b"content-length", b"transfer-encoding")
AUTHORIZATION: Final[bytes] = b"authorization"
# A generation request's body is read whole only while adapters are served; bounded.
BODY_LIMIT: Final[int] = 64 << 20
READ_METHODS: Final[tuple[str, ...]] = ("GET", "HEAD")
POST: Final[str] = "POST"
FORBIDDEN: Final[int] = 403
TOO_LARGE: Final[int] = 413
BAD_GATEWAY: Final[int] = 502


class ModelGateService:
    """The proxy and the rules it applies; the rules are classmethods the host shares."""

    def __init__(self, listen: tuple[str, int], upstream: tuple[str, int], state_dir: str) -> None:
        """Set up the gate service.

        Args:
            listen: Address and port to serve on (the model server's public port).
            upstream: The engine's loopback address and internal port.
            state_dir: The directory holding `run.json` and `pause.json`.
        """
        self.listen = listen
        self.upstream = upstream
        self.state_dir = state_dir
        self.server: asyncio.AbstractServer | None = None
        self.adapters = GateAdapters(state_dir, upstream)

    # -- the rules -----------------------------------------------------------------------------

    @classmethod
    def read_json(cls, path: str) -> dict[str, Any] | None:
        """Read a JSON file.

        Args:
            path: A JSON file.

        Returns:
            Optional[Dict[str, Any]]: Its object, or None if it is missing or not an object.
        """
        try:
            with Path(path).open("r", encoding="utf-8") as handle:
                record = json.load(handle)
        except (OSError, ValueError):
            return None
        return record if isinstance(record, dict) else None

    @classmethod
    def describe_eta(cls, seconds: Any) -> str | None:
        """Describe a time left as text.

        Args:
            seconds: Seconds left, or None when not known yet.

        Returns:
            Optional[str]: `about 9 h left`, `about 1 h 25 min left`, `about 40 min left`.
        """
        if not isinstance(seconds, (int, float)) or seconds < 0:
            return None
        minutes = max(1, int(round(seconds / 60)))
        if minutes < SECONDS_PER_MINUTE:
            return f"about {minutes} min left"
        hours, rest = divmod(minutes, SECONDS_PER_MINUTE)
        if hours < SHORT_ETA_MAX_HOURS and rest >= SHORT_ETA_MIN_MINUTES:
            return f"about {hours} h {rest} min left"
        return f"about {int(round(minutes / SECONDS_PER_MINUTE))} h left"

    @classmethod
    def message(cls, run: dict[str, Any]) -> str:
        """The refusal's text.

        Args:
            run: The run's record.

        Returns:
            str: e.g. "The model is running a benchmark (run-a, 37/100 done, about 9 h left).
            Try later or run `ragon-admin night pause`."
        """
        parts = [str(run.get("label") or f"SWE-bench run {run.get('run', '?')}")]
        total, done = run.get("total"), run.get("done")
        if isinstance(total, int) and total > 0 and isinstance(done, int):
            parts.append(f"{done}/{total} done")
        eta = cls.describe_eta(run.get("eta_s"))
        if eta:
            parts.append(eta)
        return (
            f"The model is running a benchmark ({', '.join(parts)}). "
            "Try later or run `ragon-admin night pause`."
        )

    @classmethod
    def state(cls, state_dir: str, now: float | None = None) -> dict[str, Any]:
        """What the gate does right now, from its two files.

        Args:
            state_dir: The directory holding `run.json` and `pause.json`.
            now: The time; defaults to the clock.

        Returns:
            Dict[str, Any]: `state` (`open`, `closed` or `paused`), and with a live run its
            `run`, `id`, `label`, `done`, `total`, `eta_s`, `networks` and refusal `message`;
            `paused_until` while a pause is in force; `stale` when a run's record was ignored.
        """
        now = time.time() if now is None else now
        # `loop_guard`: this gate applies `ragon`'s length cap and sampling arm (model_gate_guard.py);
        # a SWE-bench arm that needs them refuses a gate without it.
        result: dict[str, Any] = {
            "gate": "ragon",
            "state": "open",
            "loop_guard": gate_guard.VERSION,
        }
        pause = cls.read_json(os.path.join(state_dir, PAUSE_FILE)) or {}
        until = pause.get("until")
        if isinstance(until, (int, float)) and until > now:
            result["paused_until"] = until
        run = cls.read_json(os.path.join(state_dir, RUN_FILE))
        if run is None:
            return result
        heartbeat = run.get("heartbeat")
        networks = run.get("networks")
        if (
            not isinstance(heartbeat, (int, float))
            or now - heartbeat > STALE_S
            or not isinstance(networks, list)
            or not networks
        ):
            result["stale"] = run.get("run")
            return result
        result.update(
            {
                key: run.get(key)
                for key in ("run", "id", "label", "done", "total", "eta_s", "started")
            }
        )
        result["networks"] = [str(network) for network in networks]
        result["message"] = cls.message(run)
        result["state"] = "paused" if KEY_PAUSED_UNTIL in result else STATE_CLOSED
        return result

    @classmethod
    def from_networks(cls, peer: str, networks: list[str]) -> bool:
        """Whether a peer address belongs to one of the networks.

        Args:
            peer: The client's address.
            networks: The run's subnets.

        Returns:
            bool: Whether the address is in one of them.
        """
        try:
            address = ipaddress.ip_address(peer.split("%", 1)[0])
        except ValueError:
            return False
        mapped = getattr(address, "ipv4_mapped", None)
        address = mapped or address
        for network in networks:
            try:
                if address in ipaddress.ip_network(network, strict=False):
                    return True
            except ValueError:
                continue
        return False

    @classmethod
    def allows(cls, state: dict[str, Any], peer: str, method: str, path: str) -> bool:
        """Whether a request passes.

        Args:
            state: `state()`'s answer.
            peer: The client's address.
            method: The request's method.
            path: Its path, without the query.

        Returns:
            bool: True unless a run holds the gate and the request is neither the run's nor a
            read-only probe.
        """
        if state.get("state") != STATE_CLOSED:
            return True
        if method in (METHOD_GET, METHOD_HEAD) and (
            path in READ_ONLY_PATHS or path.startswith("/v1/models/")
        ):
            return True
        return cls.from_networks(peer, state.get("networks") or [])

    @classmethod
    def refusal(cls, state: dict[str, Any]) -> dict[str, Any]:
        """The body of the answer that refuses a request.

        Args:
            state: `state()`'s answer while closed.

        Returns:
            Dict[str, Any]: The body, in the API's own error shape.
        """
        return {
            "error": {
                "message": state.get("message") or cls.message({}),
                "type": "service_unavailable",
                "code": REFUSAL_CODE,
                "param": None,
            }
        }

    # -- HTTP ----------------------------------------------------------------------------------

    @classmethod
    def parse_head(cls, head: bytes) -> tuple[str, str, list[tuple[bytes, bytes]]] | None:
        """Parse a request's line and headers.

        Args:
            head: The request line and headers, ending with an empty line.

        Returns:
            Optional[tuple]: `(method, path, headers)`, the path without its query; None if
            the head is not an HTTP/1.x request.
        """
        lines = head.split(b"\r\n")
        parts = lines[0].split(b" ")
        if len(parts) != REQUEST_LINE_PARTS or not parts[2].startswith(b"HTTP/1."):
            return None
        method = parts[0].decode("latin-1")
        target = parts[1].decode("latin-1")
        # Absolute form only: a `://` in an origin-form target's query
        # (`/health_generate?u=http://x/v1/models`) must not make it a read-only path.
        if not target.startswith("/") and URL_SCHEME_SEP in target:
            target = "/" + target.split(URL_SCHEME_SEP, 1)[1].partition("/")[2]
        path = target.split("?", 1)[0] or "/"
        headers = []
        for line in lines[1:]:
            if not line:
                continue
            name, separator, value = line.partition(b":")
            if not separator:
                return None
            headers.append((name.strip().lower(), value.strip()))
        return method, path, headers

    @classmethod
    def rewrite_head(cls, head: bytes, key: str | None = None, length: int | None = None) -> bytes:
        """One request per connection.

        The hop-by-hop connection headers go and `Connection: close` takes their place, so the
        engine closes after its answer and a client never sends a second request down a connection
        that was let through for the first.

        Args:
            head: The request line and headers, ending with an empty line.
            key: The engine's API key, which then replaces whatever `Authorization` came.
            length: The length of a body the gate sends in place of the client's.

        Returns:
            bytes: The head to send to the engine.
        """
        dropped = (
            DROPPED_REQUEST_HEADERS
            + (FRAMING_HEADERS if length is not None else ())
            + ((AUTHORIZATION,) if key else ())
        )
        lines = head.split(b"\r\n")
        kept = [lines[0]] + [
            line
            for line in lines[1:]
            if line and line.partition(b":")[0].strip().lower() not in dropped
        ]
        if length is not None:
            kept.append(f"Content-Length: {length}".encode("latin-1"))
        if key:
            kept.append(f"Authorization: Bearer {key}".encode("latin-1"))
        return b"\r\n".join([*kept, b"Connection: close", b"", b""])

    @classmethod
    def framing(cls, headers: list[tuple[bytes, bytes]]) -> tuple[str, int]:
        """How a request's body is delimited.

        Args:
            headers: The request's headers.

        Returns:
            Tuple[str, int]: `("chunked", 0)`, `("length", n)` or `("none", 0)`.
        """
        values = dict(headers)
        if CHUNKED_TOKEN in values.get(b"transfer-encoding", b"").lower():
            return FRAMING_CHUNKED, 0
        try:
            length = int(values.get(b"content-length", b"0") or 0)
        except ValueError:
            length = 0
        return (FRAMING_LENGTH, length) if length > 0 else ("none", 0)

    @classmethod
    async def copy_trailers(
        cls, reader: asyncio.StreamReader, send: Callable[[bytes], Awaitable[None]]
    ) -> None:
        """Moves a chunked body's trailers, up to and including the empty line."""
        while True:
            trailer = await reader.readuntil(CRLF)
            await send(trailer)
            if trailer == CRLF:
                return

    @classmethod
    async def copy_body(
        cls,
        reader: asyncio.StreamReader,
        framing: tuple[str, int],
        sink: asyncio.StreamWriter | None,
        limit: int | None = None,
    ) -> None:
        """Moves one request body from the client to the engine (or nowhere, to discard it).

        Args:
            reader: The client.
            framing: `framing()`'s answer.
            sink: The engine, or None to discard.
            limit: Stop after this many bytes (only when discarding).
        """
        kind, remaining = framing
        moved = 0

        async def send(data: bytes) -> None:
            nonlocal moved
            moved += len(data)
            if sink is not None:
                sink.write(data)
                await sink.drain()

        if kind == FRAMING_LENGTH:
            while remaining > 0 and (limit is None or moved < limit):
                data = await reader.read(min(BUFFER, remaining))
                if not data:
                    return
                remaining -= len(data)
                await send(data)
        elif kind == FRAMING_CHUNKED:
            while limit is None or moved < limit:
                line = await reader.readuntil(b"\r\n")
                await send(line)
                size = int(line.split(b";", 1)[0].strip() or b"0", 16)
                if size == 0:
                    await cls.copy_trailers(reader, send)
                    return
                await send(await reader.readexactly(size + 2))

    @classmethod
    async def respond(
        cls,
        writer: asyncio.StreamWriter,
        status: int,
        reason: str,
        body: dict[str, Any],
        extra: list[tuple[str, str]] | None = None,
        head_only: bool = False,
    ) -> None:
        """Writes a JSON answer that closes the connection.

        Args:
            writer: The client.
            status: The status code.
            reason: Its reason phrase.
            body: The JSON body.
            extra: More headers.
            head_only: For a HEAD request.
        """
        data = json.dumps(body).encode("utf-8")
        lines = [
            f"HTTP/1.1 {status} {reason}",
            "Content-Type: application/json",
            f"Content-Length: {len(data)}",
            "Connection: close",
        ]
        lines += [f"{name}: {value}" for name, value in (extra or [])]
        writer.write(
            ("\r\n".join(lines) + "\r\n\r\n").encode("latin-1") + (b"" if head_only else data)
        )
        await writer.drain()

    @classmethod
    async def pipe(
        cls,
        source: asyncio.StreamReader,
        sink: asyncio.StreamWriter,
        replacement: tuple[bytes, bytes] | None = None,
    ) -> None:
        """Copies bytes as they arrive until the source ends, replacing one id with another of its length."""
        carry = b""
        while True:
            read = data = await source.read(BUFFER)
            if replacement is not None:
                data, carry = (
                    GateAdapters.replace_stream(data, carry, *replacement) if data else (carry, b"")
                )
            if not data:
                if not read:
                    return
                continue  # The whole read may be the start of the id: held back, not the end.
            sink.write(data)
            await sink.drain()

    @classmethod
    async def pipe_response(
        cls,
        source: asyncio.StreamReader,
        sink: asyncio.StreamWriter,
        replacement: tuple[bytes, bytes] | None = None,
    ) -> None:
        """Copies the engine's answer.

        Its head with `Connection: close` (so the client never pools a connection the gate let
        through, whatever the engine says), then the body as it arrives. Interim `1xx` heads (`100
        Continue`) pass as they are.
        """
        while True:
            head = await source.readuntil(b"\r\n\r\n")
            parts = head.split(b"\r\n", 1)[0].split(b" ", 2)
            code = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
            if HTTP_CONTINUE <= code < HTTP_OK_FLOOR and code != HTTP_SWITCHING_PROTOCOLS:
                sink.write(head)
                await sink.drain()
                continue
            sink.write(cls.rewrite_head(head))
            await sink.drain()
            break
        await cls.pipe(source, sink, replacement)

    @classmethod
    async def watch_client(cls, reader: asyncio.StreamReader) -> None:
        """Returns when the client goes away after its request.

        Anything it sends meanwhile (a pipelined second request) is never forwarded.
        """
        while await reader.read(BUFFER):
            pass

    async def respond_bad_request(self, writer: asyncio.StreamWriter) -> None:
        """Answers a head that is not HTTP/1.x."""
        await self.respond(
            writer,
            400,
            "Bad Request",
            {
                "error": {
                    "message": "Not an HTTP/1.x request.",
                    "type": "invalid_request_error",
                    "code": None,
                    "param": None,
                }
            },
        )

    async def respond_bad_gateway(self, writer: asyncio.StreamWriter, method: str) -> None:
        """Answers a request the engine could not be reached for."""
        await self.respond(
            writer,
            502,
            "Bad Gateway",
            {
                "error": {
                    "message": "The model server is not answering behind its gate; it may still be loading.",
                    "type": "service_unavailable",
                    "code": "engine_unavailable",
                    "param": None,
                }
            },
            head_only=method == METHOD_HEAD,
        )

    async def refuse(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        state: dict[str, Any],
        framing: tuple[str, int],
        expects_continue: bool,
        request: tuple[str, str, str],
    ) -> None:
        """Refuses a request while a run holds the gate (503, the body drained first)."""
        method, path, peer_address = request
        self.log(f"refused {method} {path} from {peer_address} ({state.get('run')})")
        if not expects_continue:
            with contextlib.suppress(
                TimeoutError,
                asyncio.IncompleteReadError,
                asyncio.LimitOverrunError,
                ValueError,
                ConnectionError,
            ):
                await asyncio.wait_for(
                    self.copy_body(reader, framing, None, DRAIN_LIMIT), DRAIN_TIMEOUT_S
                )
        await self.respond(
            writer,
            503,
            "Service Unavailable",
            self.refusal(state),
            [("Retry-After", RETRY_AFTER), ("X-Ragon-Gate", "closed")],
            head_only=method == METHOD_HEAD,
        )

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        """Serves one connection: one request, decided, then refused or streamed through."""
        upstream_writer: asyncio.StreamWriter | None = None
        tasks: list[asyncio.Task] = []
        try:
            peer = writer.get_extra_info("peername")
            peer_address = str(peer[0]) if peer else ""
            try:
                head = await reader.readuntil(b"\r\n\r\n")
            except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, ConnectionError):
                return
            request = self.parse_head(head)
            if request is None:
                await self.respond_bad_request(writer)
                return
            method, path, headers = request
            state = self.state(self.state_dir)
            if path == PROBE_PATH and method in (METHOD_GET, METHOD_HEAD):
                await self.respond(writer, 200, "OK", state, head_only=method == METHOD_HEAD)
                return
            framing = self.framing(headers)
            expects_continue = EXPECT_CONTINUE in dict(headers).get(b"expect", b"").lower()
            if not self.allows(state, peer_address, method, path):
                await self.refuse(
                    reader, writer, state, framing, expects_continue, (method, path, peer_address)
                )
                return
            forward = await self.adapter_request(reader, writer, (method, path, head), framing)
            if forward is None:
                return
            try:
                upstream_reader, upstream_writer = await asyncio.wait_for(
                    asyncio.open_connection(*self.upstream), UPSTREAM_CONNECT_TIMEOUT_S
                )
            except (TimeoutError, OSError):
                await self.respond_bad_gateway(writer, method)
                return
            assert upstream_writer is not None
            upstream_writer.write(forward[0] + (forward[1] or b""))
            await upstream_writer.drain()

            async def client_side() -> None:
                if forward[1] is None:
                    await self.copy_body(reader, framing, upstream_writer)
                await self.watch_client(reader)

            tasks = [
                asyncio.ensure_future(client_side()),
                asyncio.ensure_future(self.pipe_response(upstream_reader, writer, forward[2])),
            ]
            # The answer is over when the engine closes (it was asked to) or the client leaves,
            # which also tells the engine to stop generating.
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        except (
            ConnectionError,
            asyncio.IncompleteReadError,
            asyncio.LimitOverrunError,
            ValueError,
            OSError,
        ):
            pass
        finally:
            await self.close_connection(tasks, (upstream_writer, writer))

    @staticmethod
    async def close_connection(
        tasks: list[asyncio.Task], ends: tuple[asyncio.StreamWriter | None, ...]
    ) -> None:
        """Cancels the copy tasks and closes both ends of a connection."""
        for task in tasks:
            task.cancel()
        for task in tasks:
            with contextlib.suppress(BaseException):
                await task
        for end in ends:
            if end is None:
                continue
            with contextlib.suppress(OSError, RuntimeError):
                end.close()

    async def adapter_request(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        request: tuple[str, str, bytes],
        framing: tuple[str, int],
    ) -> tuple[bytes, bytes | None, tuple[bytes, bytes] | None] | None:
        """Applies the adapter rules (`model_gate_adapters.py`) to a request the gate lets through.

        Args:
            reader: The client.
            writer: The client, for an answer the gate gives itself.
            request: Its method, path and head.
            framing: `framing()`'s answer.

        Returns:
            The head to send, the body to send in place of the client's (None to stream the
            client's), and the (engine id, client id) to swap back in the answer; None when the
            gate answered the request itself.
        """
        method, path, head = request
        if path in gate_adapters.REFUSED_PATHS:
            self.log(f"refused {method} {path}: only the gate loads adapters")
            await self.respond(
                writer,
                FORBIDDEN,
                "Forbidden",
                GateAdapters.refusal(
                    FORBIDDEN,
                    "adapter_loading_refused",
                    "Adapters are loaded by the node's model gate only.",
                )[1],
            )
            return None
        listing = self.adapters.listing()
        # `/v1/responses` bodies are read for the loop guard (`model_gate_guard.py`) even without
        # adapters; every other request without adapters streams through as before.
        guarded = GateGuard.applies(method, path)
        if listing is None and not guarded:
            return self.rewrite_head(head), None, None
        key = self.adapters.key() if listing is not None else None
        if listing is not None and method in READ_METHODS and path == gate_adapters.MODELS_PATH:
            status, body = await self.adapters.engine_call("GET", gate_adapters.MODELS_PATH, None)
            reason = "OK" if status == gate_adapters.HTTP_OK else "Bad Gateway"
            await self.respond(
                writer,
                status or BAD_GATEWAY,
                reason,
                GateAdapters.models(body, listing),
                head_only=method == READ_METHODS[1],
            )
            return None
        if (
            method != POST
            or path not in gate_adapters.GENERATION_PATHS
            or not self.body_is_read(listing, framing)
        ):
            return self.rewrite_head(head, key), None, None
        refusal, sent, replacement = await self.read_and_rewrite(
            reader, framing, listing, guarded=guarded
        )
        if refusal is not None:
            await self.respond(writer, refusal[0], "Refused", refusal[1])
            return None
        return self.rewrite_head(head, key, len(sent)), sent, replacement

    @classmethod
    def body_is_read(cls, listing: dict[str, object] | None, framing: tuple[str, int]) -> bool:
        """Whether the gate reads a generation request's body whole before sending it on.

        With adapters, always (`model_gate_adapters.py`). Without, only for the loop guard and only
        a body that is whole and within the limit: a chunked or larger one streams through as before.

        Args:
            listing: The adapters served, or None.
            framing: `framing()`'s answer.

        Returns:
            bool: True to read it.
        """
        return listing is not None or (
            framing[0] == gate_adapters.FRAMING_LENGTH and framing[1] <= BODY_LIMIT
        )

    async def read_and_rewrite(
        self,
        reader: asyncio.StreamReader,
        framing: tuple[str, int],
        listing: dict[str, object] | None,
        *,
        guarded: bool,
    ) -> tuple[tuple[int, dict[str, Any]] | None, bytes, tuple[bytes, bytes] | None]:
        """Reads a generation request's body and applies the loop guard's and the adapters' rules.

        Args:
            reader: The client.
            framing: `framing()`'s answer.
            listing: The adapters served, or None.
            guarded: Whether the loop guard's rules apply (`ModelGateGuard.applies`).

        Returns:
            tuple: A refusal (status and body) or None; the body to send; the (engine id, client id)
            to swap back in the answer, or None.
        """
        request_body = await GateAdapters.read_body(reader, framing, BODY_LIMIT)
        if request_body is None:
            return (
                GateAdapters.refusal(
                    TOO_LARGE, "request_too_large", "The request is larger than the gate reads."
                ),
                b"",
                None,
            )
        if guarded:
            request_body, note = GateGuard.rewrite(request_body)
            if note:
                self.log(note)
        if listing is None:
            return None, request_body, None
        return await self.adapters.rewrite(request_body, listing)

    @classmethod
    def log(cls, line: str) -> None:
        """One line to the container's log."""
        stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        print(f"{stamp} model gate: {line}", file=sys.stderr, flush=True)

    async def start(self) -> int:
        """Starts serving.

        Returns:
            int: The port served on (useful when `listen` asked for port 0).
        """
        self.server = await asyncio.start_server(
            self.handle, self.listen[0], self.listen[1], limit=HEAD_LIMIT, reuse_address=True
        )
        return int(self.server.sockets[0].getsockname()[1])

    async def serve(self) -> None:
        """Serves until cancelled."""
        port = await self.start()
        # Not "ready" or "listening on": the model loading monitor reads those words as the engine's.
        self.log(
            f"on {self.listen[0]}:{port} in front of {self.upstream[0]}:{self.upstream[1]}, "
            f"state in {self.state_dir}"
        )
        server = self.server
        assert server is not None
        async with server:
            await server.serve_forever()


def _address(text: str) -> tuple[str, int]:
    host, _, port = text.rpartition(":")
    return host.strip("[]") or "0.0.0.0", int(port)


def main(argv: list[str] | None = None) -> int:
    """Runs the gate: `--listen 0.0.0.0:8000 --upstream 127.0.0.1:18000 --state /ragon-gate`."""
    parser = argparse.ArgumentParser(description="Ragon's model gate")
    parser.add_argument("--listen", required=True)
    parser.add_argument("--upstream", required=True)
    parser.add_argument("--state", required=True)
    args = parser.parse_args(argv)
    service = ModelGateService(_address(args.listen), _address(args.upstream), args.state)
    with contextlib.suppress(KeyboardInterrupt):
        asyncio.run(service.serve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
