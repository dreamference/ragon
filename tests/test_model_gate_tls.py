"""The gate's TLS: the node's CA, HTTPS and plain HTTP on one port (specs/DREAMFERENCE_AUTH.md §6).

The CA and server certificate are made in the test's home, the gate runs on a scratch loopback
port in front of a stand-in model server, and every `docker` call is faked. A peer that is not
loopback is simulated by making the gate's loopback test answer no.
"""

import asyncio
import contextlib
import hashlib
import http.client
import json
import socket
import ssl
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest
from cryptography import x509

from dreamference.vllm_server import model_gate
from dreamference.vllm_server.model_gate import ModelGate
from dreamference.vllm_server.model_gate_auth import ModelGateAuth
from dreamference.vllm_server.model_gate_service import ModelGateService
from dreamference.vllm_server.model_gate_tls import ModelGateTls

NAMES = ["localhost"]


class Echo(BaseHTTPRequestHandler):
    """A stand-in model server: answers every GET with the credential it saw."""

    def log_message(self, message_format, *args):
        return

    def do_GET(self):
        body = json.dumps({"path": self.path, "authorization": self.headers.get("Authorization")})
        data = body.encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(data)


@pytest.fixture
def certificates(tmp_path):
    """The node's CA and server certificate, made and staged as `server start` does."""
    folder = ModelGateTls.ensure(tmp_path / "tls", NAMES)
    gate_dir = tmp_path / "gate"
    gate_dir.mkdir()
    assert ModelGateTls.stage(folder, gate_dir)
    return {"folder": folder, "gate": gate_dir}


def run_gate(state_dir, tls_dir):
    """Start the gate in front of an echo server; returns what `stop` needs and the port."""
    upstream = ThreadingHTTPServer(("127.0.0.1", 0), Echo)
    threading.Thread(target=upstream.serve_forever, daemon=True).start()
    loop = asyncio.new_event_loop()
    service = ModelGateService(
        ("127.0.0.1", 0),
        ("127.0.0.1", upstream.server_address[1]),
        str(state_dir),
        tls_dir=str(tls_dir) if tls_dir else None,
    )
    port = loop.run_until_complete(service.start())
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()
    return {"loop": loop, "thread": thread, "service": service, "upstream": upstream, "port": port}


def stop_gate(gate):
    loop, service = gate["loop"], gate["service"]
    loop.call_soon_threadsafe(service.server.close)
    loop.call_soon_threadsafe(loop.stop)
    gate["thread"].join(10)

    async def finish():
        pending = asyncio.all_tasks() - {asyncio.current_task()}
        for task in pending:
            task.cancel()
        await asyncio.gather(*pending, return_exceptions=True)

    loop.run_until_complete(finish())
    loop.close()
    gate["upstream"].shutdown()
    gate["upstream"].server_close()


@pytest.fixture
def tls_gate(certificates):
    (certificates["gate"] / "node.json").write_text(
        json.dumps({"node_id": "id-1", "name": "box", "primary_id": None})
    )
    gate = run_gate(certificates["gate"], certificates["gate"] / "tls")
    gate.update(certificates)
    yield gate
    stop_gate(gate)


@pytest.fixture
def plain_gate(tmp_path):
    state = tmp_path / "plain"
    state.mkdir()
    gate = run_gate(state, None)
    yield gate
    stop_gate(gate)


@pytest.fixture
def remote(monkeypatch):
    """Every peer looks like another machine on the LAN."""
    monkeypatch.setattr(ModelGateService, "is_loopback", classmethod(lambda cls, peer: False))


@pytest.fixture
def signed_in(monkeypatch):
    """The accounts service accepts every credential (AUTH §10: a bad one gets 401 at once)."""

    async def accept(self, bearer):
        return {"name": "someone", "claims": None}

    monkeypatch.setattr(ModelGateAuth, "lookup", accept)


def client_context(gate):
    return ssl.create_default_context(cafile=str(gate["folder"] / "ca.pem"))


def ask(port, path, headers=None, context=None, method="GET"):
    """One request; returns status, body and headers."""
    if context is None:
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    else:
        connection = http.client.HTTPSConnection("127.0.0.1", port, timeout=10, context=context)
    try:
        connection.request(method, path, headers=headers or {})
        response = connection.getresponse()
        return response.status, response.read(), dict(response.getheaders())
    finally:
        connection.close()


def test_the_ca_is_made_once_with_private_keys(tmp_path):
    folder = ModelGateTls.ensure(tmp_path / "tls", NAMES)
    assert sorted(path.name for path in folder.iterdir()) == [
        "ca.key",
        "ca.pem",
        "server.key",
        "server.pem",
    ]
    for secret in ("ca.key", "server.key"):
        assert (folder / secret).stat().st_mode & 0o777 == 0o600
    assert folder.stat().st_mode & 0o777 == 0o700
    ca_key, ca_pem = (folder / "ca.key").read_bytes(), (folder / "ca.pem").read_bytes()
    server = (folder / "server.pem").read_bytes()
    ModelGateTls.ensure(folder, NAMES)
    assert (folder / "server.pem").read_bytes() == server  # still fits: kept
    ModelGateTls.ensure(folder, [*NAMES, "box.local"])
    assert (folder / "ca.key").read_bytes() == ca_key  # never rotated
    assert (folder / "ca.pem").read_bytes() == ca_pem
    renewed = x509.load_pem_x509_certificate((folder / "server.pem").read_bytes())
    names = renewed.extensions.get_extension_for_class(x509.SubjectAlternativeName).value
    assert {"localhost", "box.local"} <= set(names.get_values_for_type(x509.DNSName))
    assert renewed.issuer == x509.load_pem_x509_certificate(ca_pem).subject


def test_staging_never_copies_the_cas_key(certificates):
    staged = certificates["gate"] / "tls"
    assert sorted(path.name for path in staged.iterdir()) == ["ca.pem", "server.key", "server.pem"]
    assert (staged / "server.key").stat().st_mode & 0o777 == 0o600
    (certificates["folder"] / "server.key").unlink()
    assert not ModelGateTls.stage(certificates["folder"], certificates["gate"])
    assert not staged.exists()  # an incomplete set is not served


def test_the_fingerprint_is_the_sha256_of_the_certificate(certificates):
    pem = (certificates["folder"] / "ca.pem").read_bytes()
    der = ssl.PEM_cert_to_DER_cert(pem.decode())
    expected = hashlib.sha256(der).hexdigest().upper()
    fingerprint = ModelGateTls.fingerprint(pem)
    assert fingerprint.replace(":", "") == expected
    assert len(fingerprint.split(":")) == 32


def test_https_serves_the_node_info_and_the_ca_without_credentials(tls_gate):
    context = client_context(tls_gate)
    status, body, _ = ask(tls_gate["port"], "/ragon/v1/info", context=context)
    assert status == 200
    info = json.loads(body)
    ca_pem = (tls_gate["folder"] / "ca.pem").read_bytes()
    assert info["node_id"] == "id-1"
    assert info["name"] == "box"
    assert info["primary_id"] is None
    assert info["tls"] is True
    assert info["ca_fingerprint_sha256"] == ModelGateTls.fingerprint(ca_pem)
    status, body, headers = ask(tls_gate["port"], "/ragon/v1/ca.pem", context=context)
    assert (status, body) == (200, ca_pem)
    assert headers["Content-Type"] == "application/x-pem-file"


def test_https_carries_credentials_through(tls_gate, remote, signed_in):
    status, body, _ = ask(
        tls_gate["port"],
        "/v1/embeddings",
        {"Authorization": "Bearer token-1"},
        context=client_context(tls_gate),
    )
    assert status == 200
    assert json.loads(body)["authorization"] == "Bearer token-1"


def test_plain_http_without_credentials_is_served_and_counted(tls_gate, remote):
    status, body, _ = ask(tls_gate["port"], "/v1/models")
    assert status == 200
    assert json.loads(body)["path"] == "/v1/models"
    counts = tls_gate["service"].plain_counts
    assert list(counts.values()) == [1]
    status, body, _ = ask(tls_gate["port"], "/ragon-gate")
    assert json.loads(body)["plain_http"] == counts


@pytest.mark.parametrize(
    ("path", "headers"),
    [
        ("/v1/models", {"Authorization": "Bearer token-1"}),
        ("/v1/models", {"api-key": "token-1"}),
        ("/oauth/token", {}),
        ("/ragon/v1/info", {}),
        ("/ragon/v1/ca.pem", {}),
    ],
)
def test_plain_http_with_a_credential_or_a_node_route_is_told_to_use_https(
    tls_gate, remote, path, headers
):
    status, body, _ = ask(tls_gate["port"], path, headers)
    assert status == 403
    assert json.loads(body)["error"]["message"] == "Use https://"
    assert tls_gate["service"].plain_counts == {}


def test_plain_http_from_loopback_keeps_its_credentials(tls_gate, signed_in):
    status, body, _ = ask(tls_gate["port"], "/v1/embeddings", {"Authorization": "Bearer token-1"})
    assert status == 200
    assert json.loads(body)["authorization"] == "Bearer token-1"
    assert tls_gate["service"].plain_counts == {}


def test_a_failed_handshake_does_not_stop_the_port(tls_gate):
    with socket.create_connection(("127.0.0.1", tls_gate["port"]), timeout=10) as broken:
        broken.sendall(b"\x16\x03\x01\x00\x05hello")  # one record, not a ClientHello
        with contextlib.suppress(OSError):
            broken.recv(100)
    with socket.create_connection(("127.0.0.1", tls_gate["port"]), timeout=10):
        pass  # a client that closes before its first byte
    status, _, _ = ask(tls_gate["port"], "/ragon/v1/info", context=client_context(tls_gate))
    assert status == 200


def test_without_certificates_the_gate_serves_plain_http_as_before(plain_gate, remote, signed_in):
    status, body, _ = ask(plain_gate["port"], "/v1/embeddings", {"Authorization": "Bearer token-1"})
    assert status == 200
    assert json.loads(body)["authorization"] == "Bearer token-1"
    status, body, _ = ask(plain_gate["port"], "/ragon/v1/info")
    assert status == 200
    assert json.loads(body)["tls"] is False
    status, body, _ = ask(plain_gate["port"], "/ragon/v1/ca.pem")
    assert status == 404
    assert json.loads(body)["error"]["code"] == "no_tls"
    assert plain_gate["service"].plain_counts == {}


def fake_docker(monkeypatch):
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda command, **kwargs: subprocess.CompletedProcess(command, 0, "", ""),
    )
    monkeypatch.setattr(
        ModelGate,
        "probe",
        classmethod(lambda cls, host, timeout=2.0: {"gate": "ragon", "state": "open"}),
    )


def test_starting_the_gate_stages_the_certificate_and_the_node_info(monkeypatch):
    monkeypatch.delenv("DREAMFERENCE_ACCOUNTS_ENABLED", raising=False)
    fake_docker(monkeypatch)
    assert ModelGate.start(8000, "image")
    for source in ("model_gate_tls.py", "model_gate_listener.py"):
        assert (model_gate.GATE_DIR / source).read_text() == (
            model_gate.SERVICE_SOURCE.with_name(source).read_text()
        )
    staged = model_gate.GATE_DIR / "tls"
    assert sorted(path.name for path in staged.iterdir()) == ["ca.pem", "server.key", "server.pem"]
    assert not list(model_gate.GATE_DIR.rglob("ca.key"))
    assert (staged / "ca.pem").read_bytes() == (ModelGateTls.directory() / "ca.pem").read_bytes()
    node = json.loads((model_gate.GATE_DIR / "node.json").read_text())
    assert set(node) == {"node_id", "name", "primary_id", "primary_name", "issuer", "accounts"}
    assert node["primary_id"] is None  # no account store on this machine
    assert node["accounts"] is True  # accounts are on by default from 0.9.9


def test_a_node_that_cannot_make_its_certificate_serves_plain_http(monkeypatch, capsys):
    fake_docker(monkeypatch)
    stale = model_gate.GATE_DIR / "tls"
    stale.mkdir(parents=True)
    (stale / "server.pem").write_text("old")

    def broken(cls, directory=None, names=None):
        raise ImportError("no cryptography")

    monkeypatch.setattr(ModelGateTls, "ensure", classmethod(broken))
    assert ModelGate.start(8000, "image")
    assert not stale.exists()
    assert "serves plain HTTP" in capsys.readouterr().out


def test_the_copied_service_serves_https_on_its_own_as_the_container_runs_it(tmp_path):
    """The files ModelGate.start copies, run without the package, find the staged certificate."""
    copy = tmp_path / "copy"
    copy.mkdir()
    source = model_gate.SERVICE_SOURCE
    for name in (
        "model_gate_service.py",
        "model_gate_adapters.py",
        "model_gate_guard.py",
        "model_gate_tls.py",
        "model_gate_listener.py",
        "model_gate_auth.py",
        "model_gate_compat.py",
    ):
        (copy / name).write_text(source.with_name(name).read_text())
    state = tmp_path / "state"
    state.mkdir()
    folder = ModelGateTls.ensure(tmp_path / "tls", NAMES)
    assert ModelGateTls.stage(folder, state)
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    process = subprocess.Popen(
        [
            sys.executable,
            "-I",
            str(copy / "model_gate_service.py"),
            "--listen",
            f"127.0.0.1:{port}",
            "--upstream",
            "127.0.0.1:9",
            "--state",
            str(state),
        ],
        cwd=str(tmp_path),
        stderr=subprocess.PIPE,
        text=True,
    )
    context = ssl.create_default_context(cafile=str(folder / "ca.pem"))
    try:
        for _ in range(100):
            try:
                status, body, _ = ask(port, "/ragon/v1/ca.pem", context=context)
                break
            except OSError:
                time.sleep(0.05)
        else:
            pytest.fail("the gate never answered")
        assert (status, body) == (200, (folder / "ca.pem").read_bytes())
        status, body, _ = ask(port, "/ragon-gate")  # plain HTTP still answers on the same port
        assert json.loads(body)["tls"] is True
    finally:
        process.terminate()
        _, errors = process.communicate(timeout=10)
    assert "Traceback" not in errors


def test_the_node_info_advertises_accounts_only_when_the_node_opted_in(tls_gate):
    context = client_context(tls_gate)
    info = json.loads(ask(tls_gate["port"], "/ragon/v1/info", context=context)[1])
    assert info["accounts"] is False  # the fixture's node.json says nothing about accounts
    node = tls_gate["gate"] / "node.json"
    node.write_text(json.dumps({"node_id": "id-1", "name": "box", "accounts": True}))
    info = json.loads(ask(tls_gate["port"], "/ragon/v1/info", context=context)[1])
    assert info["accounts"] is True


def test_accounts_are_on_by_default_and_the_variable_and_the_toml_turn_them_off(
    tmp_path, monkeypatch
):
    from dreamference.node.node_settings import NodeSettings

    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.delenv("DREAMFERENCE_ACCOUNTS_ENABLED", raising=False)
    assert NodeSettings.accounts_enabled() is True  # on by default from 0.9.9
    config = tmp_path / ".config" / "dreamference" / "config.toml"
    config.parent.mkdir(parents=True)
    config.write_text("[accounts]\nenabled = false\n")
    assert NodeSettings.accounts_enabled() is False
    monkeypatch.setenv("DREAMFERENCE_ACCOUNTS_ENABLED", "1")  # the variable beats the toml
    assert NodeSettings.accounts_enabled() is True
    monkeypatch.setenv("DREAMFERENCE_ACCOUNTS_ENABLED", "0")
    config.write_text("")
    assert NodeSettings.accounts_enabled() is False
    monkeypatch.setenv("DREAMFERENCE_ACCOUNTS_ENABLED", "false")
    assert NodeSettings.accounts_enabled() is False
    monkeypatch.delenv("DREAMFERENCE_ACCOUNTS_ENABLED")
    assert NodeSettings.accounts_enabled() is True


def test_model_gate_service_count_plain_prunes_days_older_than_the_kept_window(
    tmp_path, monkeypatch
):
    """`count_plain` keeps only the newest seven UTC days of plain-HTTP counts."""
    import datetime as _dt

    from dreamference.vllm_server import model_gate_service as mgs

    service = ModelGateService(
        ("127.0.0.1", 0),
        ("127.0.0.1", 9),
        str(tmp_path),
    )
    old_days = [f"2026-01-{i:02d}" for i in range(1, 11)]
    service.plain_counts = dict.fromkeys(old_days, 3)
    service.plain_peers.add("10.0.0.1")
    # A UTC time that formats to 2026-02-01.
    fake_now = _dt.datetime(2026, 2, 1, 12, 0, 0, tzinfo=_dt.UTC)
    monkeypatch.setattr(
        mgs.time,
        "gmtime",
        lambda *args: fake_now.timetuple(),
    )
    service.count_plain("10.0.0.2")
    kept = set(service.plain_counts)
    assert "2026-02-01" in kept
    assert service.plain_counts["2026-02-01"] == 1
    assert len(kept) == mgs.PLAIN_DAYS_KEPT
    assert min(kept) == old_days[len(old_days) - mgs.PLAIN_DAYS_KEPT + 1]
    assert service.plain_peers == {"10.0.0.2"}
