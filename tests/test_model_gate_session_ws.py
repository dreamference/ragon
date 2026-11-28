"""The gate's remote-session front door (specs/DREAMFERENCE_RAGON_REMOTE_SESSIONS.md §6.2, A1, A2).

A WebSocket upgrade on `/` is a Codex `--remote` attach: refused 403 with an `Origin` header and 401
without a valid bearer, otherwise carried to the app-server's Unix socket with every frame copied
unchanged both ways. `/readyz` and `/healthz` answer as Codex's listener does. Everything runs on
loopback with a stand-in app-server on a socket in the test's folder and the accounts lookup faked.
"""

import asyncio
import base64
import contextlib
import hashlib
import json
import os
import ssl
import struct

import pytest

from dreamference.vllm_server.model_gate_auth import ModelGateAuth
from dreamference.vllm_server.model_gate_service import ModelGateService
from dreamference.vllm_server.model_gate_tls import ModelGateTls

GOOD = "sk-ragon-good"
GUID = b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
TEXT, BINARY, CLOSE, PING, PONG = 0x1, 0x2, 0x8, 0x9, 0xA
CAP_HEADER = b"x-codex-websocket-max-unfragmented-message-bytes"


def frame(opcode: int, payload: bytes, masked: bool) -> bytes:
    """One final frame; a client's is masked, a server's is not."""
    head = bytes([0x80 | opcode])
    size = len(payload)
    bit = 0x80 if masked else 0
    if size < 126:
        head += bytes([bit | size])
    elif size < 1 << 16:
        head += bytes([bit | 126]) + struct.pack("!H", size)
    else:
        head += bytes([bit | 127]) + struct.pack("!Q", size)
    if not masked:
        return head + payload
    mask = os.urandom(4)
    return head + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload))


async def read_frame(reader: asyncio.StreamReader) -> tuple[int, bytes]:
    """Reads one frame (opcode, unmasked payload)."""
    first, second = await reader.readexactly(2)
    size = second & 0x7F
    if size == 126:
        size = struct.unpack("!H", await reader.readexactly(2))[0]
    elif size == 127:
        size = struct.unpack("!Q", await reader.readexactly(8))[0]
    mask = await reader.readexactly(4) if second & 0x80 else None
    payload = await reader.readexactly(size)
    if mask:
        payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
    return first & 0x0F, payload


def accept_key(key: bytes) -> bytes:
    return base64.b64encode(hashlib.sha1(key + GUID).digest())


class AppServer:
    """A stand-in app-server on a Unix socket: Codex's handshake answer, then a scripted peer.

    It echoes text and binary frames with their type, answers a ping with a pong, sends a
    notification and a ping of its own when asked (`push`), closes with 1011 when asked (`bye`),
    and answers a close with the same code. Every frame and the handshake are recorded.
    """

    def __init__(self, path, status=b"101 Switching Protocols"):
        self.path = str(path)
        self.status = status
        self.heads: list[bytes] = []
        self.frames: list[tuple[int, bytes]] = []
        self.ended = asyncio.Event()
        self.server = None
        self.writers: list = []

    async def start(self):
        self.server = await asyncio.start_unix_server(self.serve, self.path)

    async def stop(self):
        self.server.close()
        for writer in self.writers:
            writer.close()
        await asyncio.wait_for(self.server.wait_closed(), 5)

    async def serve(self, reader, writer):
        self.writers.append(writer)
        try:
            head = await reader.readuntil(b"\r\n\r\n")
            self.heads.append(head)
            if not self.status.startswith(b"101"):
                writer.write(b"HTTP/1.1 " + self.status + b"\r\nContent-Length: 0\r\n\r\n")
                await writer.drain()
                return
            key = next(
                line.split(b":", 1)[1].strip()
                for line in head.split(b"\r\n")
                if line.lower().startswith(b"sec-websocket-key:")
            )
            writer.write(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                b"Sec-WebSocket-Accept: "
                + accept_key(key)
                + b"\r\n"
                + CAP_HEADER
                + b": 16777216\r\n\r\n"
            )
            await writer.drain()
            await self.converse(reader, writer)
        except (asyncio.IncompleteReadError, ConnectionError):
            pass
        finally:
            self.ended.set()
            writer.close()

    async def converse(self, reader, writer):
        while True:
            opcode, payload = await read_frame(reader)
            self.frames.append((opcode, payload))
            if opcode == CLOSE:
                writer.write(frame(CLOSE, payload, masked=False))
                await writer.drain()
                return
            if opcode == PING:
                writer.write(frame(PONG, payload, masked=False))
            elif payload == b"push":
                writer.write(frame(TEXT, b'{"method":"thread/started"}', masked=False))
                writer.write(frame(PING, b"srv", masked=False))
            elif payload == b"bye":
                writer.write(frame(CLOSE, struct.pack("!H", 1011) + b"bye", masked=False))
                await writer.drain()
                return
            else:
                writer.write(frame(opcode, payload, masked=False))
            await writer.drain()


@pytest.fixture
def signed_in(monkeypatch):
    """The accounts service knows one key; any other credential is invalid."""

    async def lookup(self, bearer):
        return {"name": "alice", "claims": None} if bearer == GOOD else None

    monkeypatch.setattr(ModelGateAuth, "lookup", lookup)


@pytest.fixture
def quiet(monkeypatch):
    lines: list[str] = []
    monkeypatch.setattr(ModelGateService, "log", classmethod(lambda cls, line: lines.append(line)))
    return lines


def upgrade(path="/", token=GOOD, extra=b"") -> bytes:
    """What `codex --remote ws://… --remote-auth-token-env` sends."""
    auth = b"Authorization: Bearer " + token.encode() + b"\r\n" if token else b""
    return (
        b"GET " + path.encode() + b" HTTP/1.1\r\nHost: localhost:8000\r\nConnection: Upgrade\r\n"
        b"Upgrade: websocket\r\nSec-WebSocket-Version: 13\r\n"
        b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n" + auth + extra + b"\r\n"
    )


async def with_gate(tmp_path, body, status=b"101 Switching Protocols", socket=True, tls_dir=None):
    """Runs `body(port, app)` against a gate in front of a stand-in app-server."""
    sock = tmp_path / "app-server.sock"
    app = AppServer(sock, status)
    if socket:
        await app.start()
    (tmp_path / "state").mkdir(exist_ok=True)
    gate = ModelGateService(
        ("127.0.0.1", 0),
        ("127.0.0.1", 9),
        str(tmp_path / "state"),
        tls_dir=tls_dir,
        app_server_socket=str(sock),
    )
    port = await gate.start()
    try:
        return await asyncio.wait_for(body(port, app), 15)
    finally:
        gate.server.close()
        if socket:
            await app.stop()


async def exchange(port, data, context=None):
    """Sends a request and reads the whole answer (the gate closes after a refusal)."""
    reader, writer = await asyncio.open_connection(
        "127.0.0.1", port, ssl=context, server_hostname="localhost" if context else None
    )
    writer.write(data)
    await writer.drain()
    # A check that was removed lets the attach through (101, then silence): fail by name at once.
    answer = await asyncio.wait_for(reader.read(), 3)
    writer.close()
    return answer


async def attach(port, context=None):
    reader, writer = await asyncio.open_connection(
        "127.0.0.1", port, ssl=context, server_hostname="localhost" if context else None
    )
    writer.write(upgrade())
    await writer.drain()
    head = await reader.readuntil(b"\r\n\r\n")
    return reader, writer, head


def status(answer: bytes) -> int:
    return int(answer.split(b" ", 2)[1])


def body(answer: bytes) -> bytes:
    return answer.split(b"\r\n\r\n", 1)[1]


# -- A2: health ----------------------------------------------------------------------------------


@pytest.mark.parametrize("path", ["/readyz", "/healthz"])
def test_the_health_paths_answer_200_with_no_body_as_codex_does(tmp_path, quiet, path):
    async def body_(port, app):
        get = await exchange(port, f"GET {path} HTTP/1.1\r\nHost: x\r\n\r\n".encode())
        head = await exchange(port, f"HEAD {path} HTTP/1.1\r\nHost: x\r\n\r\n".encode())
        post = await exchange(
            port, f"POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\n\r\n".encode()
        )
        origin = await exchange(
            port, f"GET {path} HTTP/1.1\r\nHost: x\r\nOrigin: https://evil\r\n\r\n".encode()
        )
        return get, head, post, origin

    get, head, post, origin = asyncio.run(with_gate(tmp_path, body_))
    assert status(get) == 200 and body(get) == b"" and b"Content-Length: 0" in get
    assert b"Content-Type" not in get
    assert status(head) == 200 and body(head) == b""
    assert status(post) == 405 and b"Allow: GET,HEAD" in post
    assert status(origin) == 403 and body(origin) == b""


def test_the_health_paths_need_no_credential_and_no_app_server(tmp_path, quiet):
    async def body_(port, app):
        return await exchange(port, b"GET /readyz HTTP/1.1\r\nHost: x\r\n\r\n")

    assert status(asyncio.run(with_gate(tmp_path, body_, socket=False))) == 200


# -- A1: refusals before the upgrade --------------------------------------------------------------


def test_an_origin_header_is_refused_403_before_the_credential(tmp_path, quiet, signed_in):
    async def body_(port, app):
        answer = await exchange(port, upgrade(extra=b"Origin: https://evil.example\r\n"))
        return answer, app.heads

    answer, heads = asyncio.run(with_gate(tmp_path, body_))
    assert status(answer) == 403 and body(answer) == b""
    assert heads == []


@pytest.mark.parametrize(
    ("token", "message"),
    [(None, b"Sign in with ragon login."), ("sk-ragon-wrong", b"Incorrect API key provided.")],
)
def test_a_missing_or_bad_bearer_is_refused_401_and_never_reaches_the_app_server(
    tmp_path, quiet, signed_in, token, message
):
    async def body_(port, app):
        return await exchange(port, upgrade(token=token)), app.heads

    answer, heads = asyncio.run(with_gate(tmp_path, body_))
    assert status(answer) == 401
    assert b'WWW-Authenticate: Bearer realm="ragon"' in answer
    assert json.loads(body(answer))["error"]["message"].encode() == message
    assert heads == []


def test_a_credential_in_the_url_is_401_even_with_a_good_bearer(tmp_path, quiet, signed_in):
    async def body_(port, app):
        return await exchange(port, upgrade(path="/?access_token=x")), app.heads

    answer, heads = asyncio.run(with_gate(tmp_path, body_))
    assert status(answer) == 401 and heads == []


def test_an_unreachable_accounts_service_is_401_with_its_own_message(tmp_path, quiet, monkeypatch):
    async def lookup(self, bearer):
        raise OSError("down")

    monkeypatch.setattr(ModelGateAuth, "lookup", lookup)

    async def body_(port, app):
        return await exchange(port, upgrade())

    answer = asyncio.run(with_gate(tmp_path, body_))
    assert status(answer) == 401
    assert b"authentication server unreachable" in body(answer)


def test_a_bearer_is_needed_even_where_model_routes_do_not_enforce(tmp_path, quiet, signed_in):
    # No auth.json: the node does not enforce credentials on /v1, but a session still needs one.
    async def body_(port, app):
        return await exchange(port, upgrade(token=None))

    assert status(asyncio.run(with_gate(tmp_path, body_))) == 401


def test_a_plain_attach_from_another_machine_is_refused_403(quiet, signed_in):
    sent = bytearray()

    class Writer:
        def get_extra_info(self, name):
            return None

        def write(self, data):
            sent.extend(data)

        async def drain(self):
            return

    service = ModelGateService(("127.0.0.1", 0), ("127.0.0.1", 9), "/nonexistent")
    headers = ModelGateService.parse_head(upgrade())[2]
    user = asyncio.run(service.admit_session(Writer(), headers, ("192.0.2.7", upgrade())))
    assert user is None
    assert status(bytes(sent)) == 403 and b"https_required" in sent


def test_a_missing_app_server_is_502(tmp_path, quiet, signed_in):
    async def body_(port, app):
        return await exchange(port, upgrade())

    answer = asyncio.run(with_gate(tmp_path, body_, socket=False))
    assert status(answer) == 502 and b"app_server_unavailable" in answer


def test_an_app_server_refusal_is_passed_back_as_it_came(tmp_path, quiet, signed_in):
    async def body_(port, app):
        return await exchange(port, upgrade())

    answer = asyncio.run(with_gate(tmp_path, body_, status=b"403 Forbidden"))
    assert answer.startswith(b"HTTP/1.1 403 Forbidden\r\n")


# -- A1: the handshake and the frames ------------------------------------------------------------


def test_the_handshake_is_codexs_and_the_bearer_stays_at_the_gate(tmp_path, quiet, signed_in):
    async def body_(port, app):
        _reader, writer, head = await attach(port)
        writer.close()
        await app.ended.wait()
        return head, app.heads

    head, heads = asyncio.run(with_gate(tmp_path, body_))
    assert head.startswith(b"HTTP/1.1 101 ")
    assert b"Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=" in head
    assert CAP_HEADER in head  # the app-server's own headers come back
    upstream = heads[0]
    assert upstream.startswith(b"GET / HTTP/1.1\r\n")
    assert b"sec-websocket-key: dGhlIHNhbXBsZSBub25jZQ==" in upstream
    assert b"sec-websocket-version: 13" in upstream
    assert b"uthorization" not in upstream and GOOD.encode() not in upstream
    assert any("session attached user=alice" in line for line in quiet)


def test_frames_go_both_ways_unchanged(tmp_path, quiet, signed_in):
    request = json.dumps({"id": 1, "method": "initialize", "params": {}}).encode()
    large = os.urandom(200_000)

    async def body_(port, app):
        reader, writer, _ = await attach(port)
        writer.write(frame(TEXT, request, masked=True))
        writer.write(frame(BINARY, large, masked=True))
        writer.write(frame(PING, b"cli", masked=True))
        writer.write(frame(TEXT, b"push", masked=True))
        await writer.drain()
        got = [await read_frame(reader) for _ in range(5)]
        writer.close()
        return got, list(app.frames)

    got, seen = asyncio.run(with_gate(tmp_path, body_))
    assert seen[:3] == [(TEXT, request), (BINARY, large), (PING, b"cli")]
    assert got == [
        (TEXT, request),
        (BINARY, large),
        (PONG, b"cli"),
        (TEXT, b'{"method":"thread/started"}'),
        (PING, b"srv"),
    ]


def test_a_client_close_reaches_the_app_server_with_its_code_and_comes_back(
    tmp_path, quiet, signed_in
):
    async def body_(port, app):
        reader, writer, _ = await attach(port)
        writer.write(frame(CLOSE, struct.pack("!H", 4001) + b"done", masked=True))
        await writer.drain()
        answer = await read_frame(reader)
        rest = await reader.read()
        await asyncio.wait_for(app.ended.wait(), 5)
        writer.close()
        return answer, rest, app.frames[-1]

    answer, rest, last = asyncio.run(with_gate(tmp_path, body_))
    assert last == (CLOSE, struct.pack("!H", 4001) + b"done")
    assert answer == (CLOSE, struct.pack("!H", 4001) + b"done")
    assert rest == b""  # the gate closed the client's side after the app-server closed


def test_an_app_server_close_reaches_the_client_with_its_code(tmp_path, quiet, signed_in):
    async def body_(port, app):
        reader, writer, _ = await attach(port)
        writer.write(frame(TEXT, b"bye", masked=True))
        await writer.drain()
        answer = await read_frame(reader)
        rest = await reader.read()
        writer.close()
        return answer, rest

    answer, rest = asyncio.run(with_gate(tmp_path, body_))
    assert answer == (CLOSE, struct.pack("!H", 1011) + b"bye")
    assert rest == b""
    assert any("session closed user=alice" in line for line in quiet)


def test_a_client_that_drops_is_a_disconnect_at_the_app_server(tmp_path, quiet, signed_in):
    async def body_(port, app):
        reader, writer, _ = await attach(port)
        writer.write(frame(TEXT, b"hello", masked=True))
        await writer.drain()
        await read_frame(reader)
        writer.transport.abort()
        await asyncio.wait_for(app.ended.wait(), 5)
        return app.ended.is_set()

    assert asyncio.run(with_gate(tmp_path, body_))


def test_wss_with_the_node_certificate_attaches_from_another_machine(
    tmp_path, quiet, signed_in, monkeypatch
):
    monkeypatch.setattr(ModelGateService, "is_loopback", classmethod(lambda cls, peer: False))
    folder = ModelGateTls.ensure(tmp_path / "tls", ["localhost"])
    gate_dir = tmp_path / "gate"
    gate_dir.mkdir()
    assert ModelGateTls.stage(folder, gate_dir)
    context = ssl.create_default_context(cafile=str(folder / "ca.pem"))

    async def body_(port, app):
        reader, writer, head = await attach(port, context)
        writer.write(frame(TEXT, b"over tls", masked=True))
        await writer.drain()
        echoed = await read_frame(reader)
        writer.close()
        with contextlib.suppress(ConnectionError, ssl.SSLError):
            await writer.wait_closed()
        plain = await exchange(port, upgrade())
        return head, echoed, plain

    head, echoed, plain = asyncio.run(with_gate(tmp_path, body_, tls_dir=str(gate_dir / "tls")))
    assert head.startswith(b"HTTP/1.1 101 ")
    assert echoed == (TEXT, b"over tls")
    assert status(plain) == 403  # plain ws:// from another machine: named difference D1


# -- the helpers ---------------------------------------------------------------------------------


def test_only_a_websocket_upgrade_on_the_root_is_a_session():
    def headers(value):
        return [(b"upgrade", value)]

    assert ModelGateService.is_session_upgrade("GET", "/", headers(b"WebSocket"))
    assert ModelGateService.is_session_upgrade("GET", "/", headers(b"h2c, websocket"))
    assert not ModelGateService.is_session_upgrade("GET", "/", [])
    assert not ModelGateService.is_session_upgrade("GET", "/v1/responses", headers(b"websocket"))
    assert not ModelGateService.is_session_upgrade("POST", "/", headers(b"websocket"))


def test_the_app_server_socket_follows_ragons_convention(monkeypatch, tmp_path):
    monkeypatch.delenv("RAGON_APP_SERVER_SOCKET", raising=False)
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(tmp_path))
    assert ModelGateService.default_app_server_socket() == str(
        tmp_path / "ragon" / "app-server.sock"
    )
    monkeypatch.setenv("RAGON_APP_SERVER_SOCKET", str(tmp_path / "named.sock"))
    assert ModelGateService.default_app_server_socket() == str(tmp_path / "named.sock")
    monkeypatch.delenv("RAGON_APP_SERVER_SOCKET")
    monkeypatch.delenv("XDG_RUNTIME_DIR")
    assert ModelGateService.default_app_server_socket() is None


def test_the_bearer_never_appears_in_the_gate_log(tmp_path, quiet, signed_in, capsys):
    wrong = "sk-ragon-wrong-secret"

    async def body_(port, app):
        await exchange(port, upgrade(token=wrong))
        await exchange(port, upgrade(path="/?access_token=" + wrong, token=wrong))
        await exchange(port, upgrade(token=None, extra=b"Origin: https://evil.example\r\n"))
        _reader, writer, _head = await attach(port)
        writer.close()
        await app.ended.wait()

    asyncio.run(with_gate(tmp_path, body_))
    assert any("session attached" in line for line in quiet)
    seen = "\n".join(quiet) + capsys.readouterr().out + capsys.readouterr().err
    assert GOOD not in seen and wrong not in seen


@pytest.mark.parametrize("separator", [b"\n", b"\r", b"\0"])
def test_a_bare_line_end_or_nul_in_a_header_is_400_and_reaches_no_one(
    tmp_path, quiet, signed_in, separator
):
    async def body_(port, app):
        injected = b"Sec-WebSocket-Protocol: x" + separator + b"X-Injected: 1\r\n"
        return await exchange(port, upgrade(extra=injected)), app.heads

    answer, heads = asyncio.run(with_gate(tmp_path, body_))
    assert status(answer) == 400
    assert heads == []
