"""`ragon-admin server start` brings up the accounts service, `server stop` takes it down.

The service (specs/DREAMFERENCE_AUTH.md §9, port 18100 on loopback) runs beside the gate as the
user unit `ragon-accounts.service`; a gate whose `/ragon/v1/info` says `accounts` with nothing on
that port answers every sign-in 503 `accounts_unavailable` (the 0.9.9 release blocker of 11-23).
Every `systemctl` call is recorded by a fake `subprocess.run`: nothing reaches the real machine.
"""

import http.client
import json
import shutil
import subprocess
import sys
import threading

import pytest

from dreamference.accounts import accounts_service
from dreamference.accounts.accounts_service import AccountsService
from dreamference.accounts.accounts_store import AccountsStore
from dreamference.node.node_settings import ACCOUNTS_ENABLED_VARIABLE, NodeSettings
from dreamference.vllm_server.model_gate import ModelGate
from dreamference.vllm_server.vllm_server_manager import VLLMServerManager

UNIT = accounts_service.UNIT


@pytest.fixture
def calls(monkeypatch):
    """Every command `subprocess.run` is given; each one succeeds."""
    seen: list[list[str]] = []
    monkeypatch.setattr(
        subprocess,
        "run",
        lambda command, **kwargs: (
            seen.append(list(command)) or subprocess.CompletedProcess(command, 0, "", "")
        ),
    )
    monkeypatch.setattr(shutil, "which", lambda name, *a, **k: f"/usr/bin/{name}")
    monkeypatch.setattr(
        ModelGate,
        "probe",
        classmethod(lambda cls, host, timeout=2.0: {"gate": "ragon", "state": "open"}),
    )
    return seen


def systemctl(calls: list[list[str]]) -> list[list[str]]:
    """The `systemctl --user` calls that name the accounts unit, their verbs and unit only."""
    return [c[2:] for c in calls if c[:2] == ["systemctl", "--user"] and UNIT in c]


def unit_file():
    return AccountsService.unit_dir() / UNIT


def test_server_start_writes_enables_and_starts_the_accounts_unit(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    text = unit_file().read_text()
    assert f"ExecStart={sys.executable} -m dreamference.accounts.accounts_service" in text
    assert "--secondary" not in text
    assert "MemoryMax=" in text  # bounded beside the resident model server
    assert "Restart=on-failure" in text
    assert "WantedBy=default.target" in text  # back at boot, as the gate's container is
    assert "SyslogIdentifier=ragon-gate" in text  # logged with the gate it answers behind
    verbs = [c[0] for c in systemctl(calls)]
    assert verbs.index("enable") < verbs.index("restart"), verbs


def test_the_unit_starts_after_the_gate_answers_not_before(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    gate_run = next(i for i, c in enumerate(calls) if c[:3] == ["docker", "run", "-d"])
    first_unit = next(i for i, c in enumerate(calls) if UNIT in c)
    assert gate_run < first_unit


def test_a_secondary_runs_the_service_as_a_forwarder(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    NodeSettings.set_pinned_primary("primary-node-id")
    assert ModelGate.start(8000, "image")
    assert "-m dreamference.accounts.accounts_service --secondary" in unit_file().read_text()


def test_with_accounts_off_server_start_leaves_the_unit_alone(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "0")
    assert ModelGate.start(8000, "image")
    assert systemctl(calls) == []
    assert ModelGate.node_info()["accounts"] is False


def test_every_gate_start_restarts_the_service_so_an_update_runs_its_new_code(calls, monkeypatch):
    """An unchanged unit is restarted too.

    The unit's text names only the interpreter: after `ragon update` it is unchanged while the
    package under it is new, so a start that restarts only on a changed unit keeps the old code.
    """
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    calls.clear()
    assert ModelGate.start(8000, "image")  # the unit file is unchanged now
    verbs = [c[0] for c in systemctl(calls)]
    assert {"restart", "try-restart"} & set(verbs), verbs


def test_server_stop_stops_the_accounts_service(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    calls.clear()
    VLLMServerManager().stop_server(8000)
    assert ["docker", "stop", "dreamference-gate-8000"] in calls
    assert any(c[0] in ("stop", "disable") for c in systemctl(calls)), systemctl(calls)


def test_server_stop_on_a_node_that_never_ran_the_service_calls_no_systemctl(calls):
    VLLMServerManager().stop_server(8000)
    assert systemctl(calls) == []


def test_server_remove_stops_the_accounts_service_too(calls, monkeypatch):
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    calls.clear()
    VLLMServerManager().remove_server(8000)
    assert any(c[0] in ("stop", "disable") for c in systemctl(calls)), systemctl(calls)


@pytest.mark.parametrize("enabled", ["0", "1"])
@pytest.mark.parametrize("role", ["none", "primary", "secondary"])
@pytest.mark.parametrize("store", [False, True])
def test_a_node_never_advertises_accounts_it_does_not_start(monkeypatch, enabled, role, store):
    """The invariant behind the blocker: no `"accounts": true` without the service.

    `"accounts": true` in the gate's info means the same `server start` configured the service
    behind the gate's sign-in routes.
    """
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, enabled)
    if role == "primary":
        NodeSettings.set_self_primary()
    elif role == "secondary":
        NodeSettings.set_pinned_primary("primary-node-id")
    if store:
        AccountsStore(AccountsStore.default_path(), primary_name="box", audit_sink=[].append)
    started: list[bool] = []
    monkeypatch.setattr(
        AccountsService, "ensure_running", classmethod(lambda cls: started.append(True) or True)
    )
    ModelGate.start_accounts()
    if ModelGate.node_info()["accounts"]:
        assert started, f"advertises accounts but starts no service ({enabled=}, {role=}, {store=})"


def test_the_started_service_answers_its_health_route_on_loopback_only(tmp_path):
    """What the unit runs: discovery answers 200 with the issuer, bound to 127.0.0.1."""
    store = AccountsStore(tmp_path / "s" / "accounts.db", primary_name="box", audit_sink=[].append)
    httpd = AccountsService.server(AccountsService.build(store), port=0)
    assert httpd.server_address[0] == "127.0.0.1"
    thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    thread.start()
    try:
        connection = http.client.HTTPConnection("127.0.0.1", httpd.server_address[1], timeout=10)
        connection.request("GET", "/.well-known/openid-configuration")
        response = connection.getresponse()
        document = json.loads(response.read())
        connection.close()
    finally:
        httpd.shutdown()
        httpd.server_close()
    assert response.status == 200
    assert document["issuer"] == AccountsService.issuer("box")


# ----- `node primary` after `server start` (the w48s test pair, 11-24) ------------------------


def _pin(monkeypatch):
    from dreamference.accounts.accounts_migrate import AccountsMigrate

    monkeypatch.setattr(AccountsMigrate, "advertise", classmethod(lambda cls: None))
    entry = {"node": "primary-node-id", "name": "alpha", "address": "192.0.2.7"}
    AccountsMigrate.pin(entry, lambda address, port: b"-----BEGIN CERTIFICATE-----\nx\n")


def test_pinning_after_server_start_turns_the_running_service_into_a_forwarder(calls, monkeypatch):
    """The pair's cause: the unit kept its primary mode and answered from an empty store."""
    monkeypatch.setenv(ACCOUNTS_ENABLED_VARIABLE, "1")
    assert ModelGate.start(8000, "image")
    assert "--secondary" not in unit_file().read_text()
    calls.clear()
    _pin(monkeypatch)
    assert "-m dreamference.accounts.accounts_service --secondary" in unit_file().read_text()
    verbs = [c[0] for c in systemctl(calls)]
    assert verbs == ["try-restart"], verbs  # a stopped service stays stopped
    assert ["systemctl", "--user", "daemon-reload"] in calls


def test_pinning_before_any_server_start_calls_no_systemctl(calls, monkeypatch):
    _pin(monkeypatch)
    assert systemctl(calls) == []
    assert not unit_file().exists()


def test_the_service_forwards_when_pinned_even_from_an_old_unit(monkeypatch):
    """A unit written without `--secondary` still runs as the forwarder on a pinned node."""
    NodeSettings.set_pinned_primary("primary-node-id")
    built: list[str] = []

    class Server:
        server_address = ("127.0.0.1", 18100)

        def serve_forever(self):
            return None

        def server_close(self):
            return None

    monkeypatch.setattr(
        AccountsService,
        "build_secondary",
        classmethod(lambda cls, url=None, ca_file=None: built.append("secondary") or object()),
    )
    monkeypatch.setattr(
        AccountsService, "build", classmethod(lambda cls, store: built.append("primary"))
    )
    monkeypatch.setattr(AccountsService, "server", classmethod(lambda cls, oauth, port: Server()))
    assert AccountsService.main([]) == 0
    assert built == ["secondary"]


def test_a_pinned_node_with_a_stray_store_is_not_its_own_primary(monkeypatch):
    """Third's info said `primary: true` from the store its primary-mode start had left."""
    AccountsStore(AccountsStore.default_path(), primary_name="box", audit_sink=[].append)
    NodeSettings.set_pinned_primary("primary-node-id")
    info = ModelGate.node_info()
    assert info["primary_id"] is None and info["issuer"] is None
