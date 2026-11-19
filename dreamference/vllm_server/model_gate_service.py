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

A generation request that names its job (`X-Ragon-Job`, which coord's `ragon` calls send) gets
one `usage` line in the gate's log when its connection ends: the job, the path and the token counts
the engine's answer carried (its `usage` object, read as the answer streams by, never held back),
so the work a job did on the node can be counted. An untagged request logs nothing new.

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
import re
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
# The node's certificate and the port that serves TLS beside plain HTTP, copied the same way.
try:
    gate_tls = importlib.import_module("dreamference.vllm_server.model_gate_tls")
    gate_listener = importlib.import_module("dreamference.vllm_server.model_gate_listener")
except ImportError:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    gate_tls = importlib.import_module("model_gate_tls")
    gate_listener = importlib.import_module("model_gate_listener")
GateTls = gate_tls.ModelGateTls
GateListener = gate_listener.ModelGateListener

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

# The primary's accounts service (`dreamference/accounts/accounts_service.py`, port `ACCOUNTS_PORT`)
# answers the sign-in routes; the gate forwards them there, whatever its state, so a run holding the
# gate never blocks a sign-in or a logout (specs/DREAMFERENCE_AUTH.md §5.1). A literal: this file
# runs in the gate's container, without the package.
ACCOUNTS_UPSTREAM: Final[tuple[str, int]] = ("127.0.0.1", 18100)
ACCOUNT_PATHS: Final[tuple[str, ...]] = (
    "/oauth/token",
    "/oauth/revoke",
    "/oauth/userinfo",
    "/.well-known/jwks.json",
    "/.well-known/openid-configuration",
)
ACCOUNT_PREFIXES: Final[tuple[str, ...]] = (
    "/ragon/v1/users",
    "/ragon/v1/bindings",
    "/ragon/v1/nodes",
    "/ragon/v1/usage",
)

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
# The job a generation request names (`X-Ragon-Job`) and the answer's `usage` object, logged once
# per tagged request. A tag outside the allowed characters is ignored, so the log line stays one line.
JOB_HEADER: Final[bytes] = b"x-ragon-job"
JOB_TAG_MAX: Final[int] = 120
JOB_TAG_CHARS: Final[str] = "._:-/@+"
USAGE_KEY: Final[bytes] = b'"usage":'
# A usage object longer than this is not one: the scan gives up on it rather than buffer an answer.
USAGE_LIMIT: Final[int] = 1 << 12
# A chunk-size line of a chunked answer, which may fall inside the object between two reads.
CHUNK_LINE: Final[re.Pattern[bytes]] = re.compile(rb"\r\n[0-9a-fA-F]+(?:;[^\r\n]*)?\r\n")
OPEN_BRACE: Final[int] = ord("{")
CLOSE_BRACE: Final[int] = ord("}")

# Answered by the gate itself, open to anyone, even while a run holds the gate: who this node is
# and the CA its certificate is signed by (specs/DREAMFERENCE_AUTH.md §5.1, §6).
INFO_PATH: Final[str] = "/ragon/v1/info"
CA_PATH: Final[str] = "/ragon/v1/ca.pem"
# The node's identity, written by `ModelGate.start` beside the gate's state.
NODE_FILE: Final[str] = "node.json"
PEM_TYPE: Final[str] = "application/x-pem-file"
NOT_FOUND: Final[int] = 404
# While the gate serves TLS, plain HTTP is served only to requests without credentials (0.9.8
# clients); one with a credential, or to the sign-in and node routes, is refused (§6).
API_KEY: Final[bytes] = b"api-key"
HTTPS_ONLY_PREFIXES: Final[tuple[str, ...]] = ("/oauth", "/ragon/v1")
HTTPS_REQUIRED: Final[str] = "Use https://"
HTTPS_REQUIRED_CODE: Final[str] = "https_required"
# Days of plain-HTTP counts kept for the probe.
PLAIN_DAYS_KEPT: Final[int] = 7


class ModelGateService:
    """The proxy and the rules it applies; the rules are classmethods the host shares."""

    def __init__(
        self,
        listen: tuple[str, int],
        upstream: tuple[str, int],
        state_dir: str,
        accounts: tuple[str, int] = ACCOUNTS_UPSTREAM,
        tls_dir: str | None = None,
    ) -> None:
        """Set up the gate service.

        Args:
            listen: Address and port to serve on (the model server's public port).
            upstream: The engine's loopback address and internal port.
            state_dir: The directory holding `run.json` and `pause.json`.
            accounts: The accounts service's loopback address and port.
            tls_dir: A directory holding `server.pem`, `server.key` and `ca.pem`; with them the
                port serves HTTPS beside plain HTTP, without them plain HTTP only.
        """
        self.listen = listen
        self.upstream = upstream
        self.accounts = accounts
        self.state_dir = state_dir
        self.server: asyncio.AbstractServer | None = None
        self.adapters = GateAdapters(state_dir, upstream)
        self.tls_context = GateTls.server_context(tls_dir) if tls_dir else None
        self.ca_pem: bytes | None = None
        if self.tls_context is not None and tls_dir:
            with contextlib.suppress(OSError):
                self.ca_pem = (Path(tls_dir) / gate_tls.CA_CERT).read_bytes()
        # Plain-HTTP requests served while TLS is on, per UTC day, and who sent them today.
        self.plain_counts: dict[str, int] = {}
        self.plain_peers: set[str] = set()

    # -- the rules -----------------------------------------------------------------------------

    @classmethod
    def read_json(cls, path: str | Path) -> dict[str, Any] | None:
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
    def describe_eta(cls, seconds: object) -> str | None:
        """Describe a time left as text.

        Args:
            seconds: Seconds left, or None when not known yet.

        Returns:
            Optional[str]: `about 9 h left`, `about 1 h 25 min left`, `about 40 min left`.
        """
        if not isinstance(seconds, (int, float)) or seconds < 0:
            return None
        minutes = max(1, round(seconds / 60))
        if minutes < SECONDS_PER_MINUTE:
            return f"about {minutes} min left"
        hours, rest = divmod(minutes, SECONDS_PER_MINUTE)
        if hours < SHORT_ETA_MAX_HOURS and rest >= SHORT_ETA_MIN_MINUTES:
            return f"about {hours} h {rest} min left"
        return f"about {round(minutes / SECONDS_PER_MINUTE)} h left"

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
        pause = cls.read_json(Path(state_dir) / PAUSE_FILE) or {}
        until = pause.get("until")
        if isinstance(until, (int, float)) and until > now:
            result["paused_until"] = until
        run = cls.read_json(Path(state_dir) / RUN_FILE)
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
        await cls.respond_raw(
            writer,
            (status, reason, "application/json"),
            json.dumps(body).encode("utf-8"),
            extra,
            head_only,
        )

    @classmethod
    async def respond_raw(
        cls,
        writer: asyncio.StreamWriter,
        status: tuple[int, str, str],
        data: bytes,
        extra: list[tuple[str, str]] | None = None,
        head_only: bool = False,
    ) -> None:
        """Writes an answer of any type that closes the connection.

        Args:
            writer: The client.
            status: The status code, its reason phrase and the content type.
            data: The body.
            extra: More headers.
            head_only: For a HEAD request.
        """
        code, reason, content_type = status
        lines = [
            f"HTTP/1.1 {code} {reason}",
            f"Content-Type: {content_type}",
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
        tap: dict[str, Any] | None = None,
    ) -> None:
        """Copies bytes as they arrive until the source ends, replacing one id with another of its length.

        Each piece, once written, is fed to the usage tap (`usage_tap`) when there is one.
        """
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
            if tap is not None:
                cls.feed_usage(tap, data)

    @classmethod
    async def pipe_response(
        cls,
        source: asyncio.StreamReader,
        sink: asyncio.StreamWriter,
        replacement: tuple[bytes, bytes] | None = None,
        tap: dict[str, Any] | None = None,
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
        await cls.pipe(source, sink, replacement, tap)

    @classmethod
    def usage_tap(
        cls, method: str, path: str, headers: list[tuple[bytes, bytes]]
    ) -> dict[str, Any] | None:
        """A usage tap for a generation request that names its job (`X-Ragon-Job`), else None.

        Args:
            method: The request's method.
            path: Its path.
            headers: Its headers, names lowercased (`parse_head`).

        Returns:
            The tap (`feed_usage` fills it, `usage_line` reports it), or None.
        """
        if method != POST or path not in gate_adapters.GENERATION_PATHS:
            return None
        tag = dict(headers).get(JOB_HEADER, b"").decode("ascii", "replace").strip()
        if (
            not tag
            or len(tag) > JOB_TAG_MAX
            or not all(c.isalnum() or c in JOB_TAG_CHARS for c in tag)
        ):
            return None
        return {"job": tag, "path": path, "carry": b"", "usage": None}

    @classmethod
    def feed_usage(cls, tap: dict[str, Any], data: bytes) -> None:
        """Scans one piece of an answer for its `usage` object; the last complete one wins.

        A key or object cut by the end of the piece is carried to the next, at most `USAGE_LIMIT`
        bytes. Nothing here raises on what the engine sends.

        Args:
            tap: `usage_tap()`'s answer, updated in place.
            data: The piece, as written to the client.
        """
        buffer = tap["carry"] + data
        tap["carry"] = buffer[-(len(USAGE_KEY) - 1) :]
        start = buffer.find(USAGE_KEY)
        while start >= 0:
            found = cls.usage_object(buffer[start + len(USAGE_KEY) :])
            if found is None:
                tap["carry"] = buffer[start:]
                return
            if found:
                tap["usage"] = found
            start = buffer.find(USAGE_KEY, start + len(USAGE_KEY))

    @classmethod
    def usage_object(cls, rest: bytes) -> dict[str, Any] | None:
        """The object that follows a `"usage":` key.

        Args:
            rest: The answer from just after the key.

        Returns:
            The object; an empty dict when the value is not one (`null`, too long, unparsable);
            None when it runs past `rest`, to be read again with the next piece.
        """
        window = rest[:USAGE_LIMIT]
        body = CHUNK_LINE.sub(b"", window).lstrip()
        if not body.startswith(b"{"):
            return None if not body and len(window) < USAGE_LIMIT else {}
        depth = 0
        for end, byte in enumerate(body):
            depth += (byte == OPEN_BRACE) - (byte == CLOSE_BRACE)
            if depth == 0:
                try:
                    found = json.loads(body[: end + 1])
                except ValueError:
                    return {}
                return found if isinstance(found, dict) else {}
        return None if len(window) < USAGE_LIMIT else {}

    @classmethod
    def usage_line(cls, tap: dict[str, Any]) -> str:
        """The log line for a tagged request: `usage` and one JSON object.

        `generated` is the answer's output tokens (`output_tokens`, or `completion_tokens` on the
        chat paths), `prompt` its input tokens, `cached` the cached part of those; each is null when
        the answer carried none (the client left before the end, or the engine sent no usage).

        Args:
            tap: `usage_tap()`'s answer after the answer was copied.

        Returns:
            str: The line, without the gate's stamp.
        """
        usage = tap["usage"] or {}
        details = usage.get("input_tokens_details") or usage.get("prompt_tokens_details")
        cached = details.get("cached_tokens") if isinstance(details, dict) else None
        counts = {
            "generated": usage.get("output_tokens", usage.get("completion_tokens")),
            "prompt": usage.get("input_tokens", usage.get("prompt_tokens")),
            "cached": cached,
        }
        record = {"job": tap["job"], "path": tap["path"]}
        record.update({key: value if type(value) is int else None for key, value in counts.items()})
        return "usage " + json.dumps(record, separators=(",", ":"))

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
        await self.drain(reader, framing, expects_continue)
        await self.respond(
            writer,
            503,
            "Service Unavailable",
            self.refusal(state),
            [("Retry-After", RETRY_AFTER), ("X-Ragon-Gate", "closed")],
            head_only=method == METHOD_HEAD,
        )

    @classmethod
    async def drain(
        cls, reader: asyncio.StreamReader, framing: tuple[str, int], expects_continue: bool
    ) -> None:
        """Reads a refused request's body, bounded, so the answer is not lost to a reset."""
        if expects_continue:
            return
        with contextlib.suppress(
            TimeoutError,
            asyncio.IncompleteReadError,
            asyncio.LimitOverrunError,
            ValueError,
            ConnectionError,
        ):
            await asyncio.wait_for(
                cls.copy_body(reader, framing, None, DRAIN_LIMIT), DRAIN_TIMEOUT_S
            )

    @classmethod
    def is_loopback(cls, peer: str) -> bool:
        """True for a loopback peer, IPv4, IPv6 or IPv4-mapped."""
        try:
            address = ipaddress.ip_address(peer.split("%", 1)[0])
        except ValueError:
            return False
        mapped = getattr(address, "ipv4_mapped", None)
        return bool(address.is_loopback or (mapped is not None and mapped.is_loopback))

    @classmethod
    def needs_https(cls, path: str, headers: list[tuple[bytes, bytes]]) -> bool:
        """True for a request plain HTTP may not carry: a credential, or a sign-in or node route."""
        route = path.split("?", 1)[0]
        if any(route == prefix or route.startswith(prefix + "/") for prefix in HTTPS_ONLY_PREFIXES):
            return True
        return any(name in (AUTHORIZATION, API_KEY) for name, _ in headers)

    def count_plain(self, peer: str) -> None:
        """Counts a plain-HTTP request served while TLS is on (an older client), per UTC day."""
        day = time.strftime("%Y-%m-%d", time.gmtime())
        if day not in self.plain_counts:
            self.plain_peers.clear()
            for old in sorted(self.plain_counts)[
                : max(0, len(self.plain_counts) - PLAIN_DAYS_KEPT + 1)
            ]:
                del self.plain_counts[old]
        self.plain_counts[day] = self.plain_counts.get(day, 0) + 1
        if peer not in self.plain_peers:
            self.plain_peers.add(peer)
            self.log(f"plain HTTP from {peer} (an older client; counted for {day})")

    async def check_plain(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        request: tuple[str, str, list[tuple[bytes, bytes]]],
        peer_address: str,
    ) -> bool:
        """Applies the plain-HTTP rule while TLS is on; True when the request was refused.

        Loopback is exempt: the owner's rule keeps plain HTTP there even after 1.0.

        Args:
            reader: The client's stream.
            writer: The client's writer.
            request: Method, path and headers.
            peer_address: The client's address.

        Returns:
            bool: True when the request was answered `403` here.
        """
        if self.tls_context is None or writer.get_extra_info("ssl_object") is not None:
            return False
        if self.is_loopback(peer_address):
            return False
        method, path, headers = request
        if not self.needs_https(path, headers):
            self.count_plain(peer_address)
            return False
        self.log(f"refused plain {method} {path.split('?', 1)[0]} from {peer_address}")
        await self.drain(
            reader,
            self.framing(headers),
            EXPECT_CONTINUE in dict(headers).get(b"expect", b"").lower(),
        )
        await self.respond(
            writer,
            FORBIDDEN,
            "Forbidden",
            {
                "error": {
                    "message": HTTPS_REQUIRED,
                    "type": "invalid_request_error",
                    "code": HTTPS_REQUIRED_CODE,
                    "param": None,
                }
            },
            head_only=method == METHOD_HEAD,
        )
        return True

    def node_info(self) -> dict[str, Any]:
        """`/ragon/v1/info`: the node's id and name, its primary, and its CA's fingerprint."""
        node = self.read_json(Path(self.state_dir) / NODE_FILE) or {}
        info: dict[str, Any] = {
            key: node.get(key)
            for key in ("node_id", "name", "primary_id", "primary_name", "issuer")
        }
        info["tls"] = self.tls_context is not None
        info["ca_fingerprint_sha256"] = (
            GateTls.fingerprint(self.ca_pem) if self.ca_pem is not None else None
        )
        return info

    async def answer_node_route(self, writer: asyncio.StreamWriter, method: str, path: str) -> bool:
        """Answers `/ragon/v1/info` and `/ragon/v1/ca.pem`; True when the path was one of them."""
        route = path.split("?", 1)[0]
        if route not in (INFO_PATH, CA_PATH) or method not in (METHOD_GET, METHOD_HEAD):
            return False
        head_only = method == METHOD_HEAD
        if route == INFO_PATH:
            await self.respond(writer, 200, "OK", self.node_info(), head_only=head_only)
        elif self.ca_pem is None:
            await self.respond(
                writer,
                NOT_FOUND,
                "Not Found",
                {"error": {"message": "This node serves no TLS.", "code": "no_tls"}},
                head_only=head_only,
            )
        else:
            await self.respond_raw(writer, (200, "OK", PEM_TYPE), self.ca_pem, head_only=head_only)
        return True

    async def answer_here(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        request: tuple[str, str, list[tuple[bytes, bytes]]],
        context: tuple[str, dict[str, Any]],
    ) -> bool:
        """Answers what the gate answers itself: the plain-HTTP refusal, the probe, the node routes.

        Args:
            reader: The client's stream.
            writer: The client's writer.
            request: Method, path and headers.
            context: The client's address and the gate's state.

        Returns:
            bool: True when the request was answered here.
        """
        method, path, _ = request
        peer_address, state = context
        if await self.check_plain(reader, writer, request, peer_address):
            return True
        if path == PROBE_PATH and method in (METHOD_GET, METHOD_HEAD):
            await self.respond(
                writer, 200, "OK", self.probe_answer(state), head_only=method == METHOD_HEAD
            )
            return True
        return await self.answer_node_route(writer, method, path)

    async def handle(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        """Serves one connection: one request, decided, then refused or streamed through."""
        upstream_writer: asyncio.StreamWriter | None = None
        tasks: list[asyncio.Task] = []
        tap: dict[str, Any] | None = None
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
            if await self.answer_here(reader, writer, request, (peer_address, state)):
                return
            framing = self.framing(headers)
            expects_continue = EXPECT_CONTINUE in dict(headers).get(b"expect", b"").lower()
            if await self.divert(
                reader,
                writer,
                (state, framing, expects_continue),
                (method, path, peer_address, head),
            ):
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

            tap = self.usage_tap(method, path, headers)
            tasks = [
                asyncio.ensure_future(client_side()),
                asyncio.ensure_future(self.pipe_response(upstream_reader, writer, forward[2], tap)),
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
            self.log_usage(tap)

    async def divert(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        decision: tuple[dict[str, Any], tuple[str, int], bool],
        request: tuple[str, str, str, bytes],
    ) -> bool:
        """Answers a request that does not go to the engine: an auth route, or one refused.

        An auth route goes to the accounts service even while a run holds the gate, so a
        client can sign in or refresh during a run (specs/DREAMFERENCE_AUTH.md §9).

        Args:
            reader: The client's stream.
            writer: The client's writer.
            decision: The gate's state, the request's framing and whether it expects 100.
            request: Method, path, peer address and the raw head.

        Returns:
            bool: True when the request was answered here.
        """
        state, framing, expects_continue = decision
        method, path, peer_address, head = request
        if self.is_account_route(path):
            await self.forward_account(reader, writer, (method, head), framing)
            return True
        if not self.allows(state, peer_address, method, path):
            await self.refuse(
                reader, writer, state, framing, expects_continue, (method, path, peer_address)
            )
            return True
        return False

    @classmethod
    def is_account_route(cls, path: str) -> bool:
        """True for a path the accounts service answers (`ACCOUNT_PATHS`, `ACCOUNT_PREFIXES`)."""
        route = path.split("?", 1)[0].rstrip("/") or "/"
        return route in ACCOUNT_PATHS or any(
            route == prefix or route.startswith(prefix + "/") for prefix in ACCOUNT_PREFIXES
        )

    async def forward_account(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        request: tuple[str, bytes],
        framing: tuple[str, int],
    ) -> None:
        """Streams one request to the accounts service and its answer back.

        No engine key, no adapter rule and no usage tap: the request and its credentials go to the
        accounts service as the client sent them, one request per connection.

        Args:
            reader: The client.
            writer: The client.
            request: Its method and head.
            framing: `framing()`'s answer.
        """
        method, head = request
        try:
            accounts_reader, accounts_writer = await asyncio.wait_for(
                asyncio.open_connection(*self.accounts), UPSTREAM_CONNECT_TIMEOUT_S
            )
        except (TimeoutError, OSError):
            await self.respond_accounts_unreachable(writer, method)
            return
        tasks: list[asyncio.Task] = []
        try:
            accounts_writer.write(self.rewrite_head(head))
            await accounts_writer.drain()

            async def client_side() -> None:
                await self.copy_body(reader, framing, accounts_writer)
                await self.watch_client(reader)

            tasks = [
                asyncio.ensure_future(client_side()),
                asyncio.ensure_future(self.pipe_response(accounts_reader, writer)),
            ]
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        finally:
            await self.close_connection(tasks, (accounts_writer,))

    async def respond_accounts_unreachable(self, writer: asyncio.StreamWriter, method: str) -> None:
        """Answers an auth route when the accounts service cannot be reached (503)."""
        await self.respond(
            writer,
            503,
            "Service Unavailable",
            {
                "error": {
                    "message": "authentication server unreachable",
                    "type": "service_unavailable",
                    "code": "accounts_unavailable",
                    "param": None,
                }
            },
            [("Retry-After", "5")],
            head_only=method == METHOD_HEAD,
        )

    @classmethod
    def log_usage(cls, tap: dict[str, Any] | None) -> None:
        """Logs a tagged request's `usage_line`, once its connection is closed; nothing without a tap."""
        if tap is not None:
            cls.log(cls.usage_line(tap))

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
        print(  # noqa: T201 - the container's log on stderr
            f"{stamp} model gate: {line}", file=sys.stderr, flush=True
        )

    def probe_answer(self, state: dict[str, Any]) -> dict[str, Any]:
        """The probe's answer: the state, and while TLS is on the plain-HTTP counts per day."""
        if self.tls_context is None:
            return state
        return {**state, "tls": True, "plain_http": dict(self.plain_counts)}

    async def start(self) -> int:
        """Starts serving: HTTPS beside plain HTTP with a TLS context, plain HTTP without one.

        Returns:
            int: The port served on (useful when `listen` asked for port 0).
        """
        if self.tls_context is not None:
            listener = await GateListener.open(
                self.listen, self.handle, self.tls_context, HEAD_LIMIT
            )
            self.server = listener
            return int(listener.sockets[0].getsockname()[1])
        server = await asyncio.start_server(
            self.handle, self.listen[0], self.listen[1], limit=HEAD_LIMIT, reuse_address=True
        )
        self.server = server
        return int(server.sockets[0].getsockname()[1])

    async def serve(self) -> None:
        """Serves until cancelled."""
        port = await self.start()
        # Not "ready" or "listening on": the model loading monitor reads those words as the engine's.
        self.log(
            f"on {self.listen[0]}:{port} in front of {self.upstream[0]}:{self.upstream[1]}, "
            f"state in {self.state_dir}, "
            + ("HTTPS and plain HTTP" if self.tls_context is not None else "plain HTTP only")
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
    # The node's certificate; `<state>/tls`, staged there by `ModelGate.start`, unless named.
    parser.add_argument("--tls")
    args = parser.parse_args(argv)
    service = ModelGateService(
        _address(args.listen),
        _address(args.upstream),
        args.state,
        tls_dir=args.tls or str(Path(args.state) / gate_tls.STAGED_DIR),
    )
    with contextlib.suppress(KeyboardInterrupt):
        asyncio.run(service.serve())
    return 0


if __name__ == "__main__":
    sys.exit(main())
