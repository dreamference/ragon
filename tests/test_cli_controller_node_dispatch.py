"""`ragon-admin node`, `host`, `code` and `night` dispatch through run_cli: every branch, mocked."""

import importlib
from unittest.mock import MagicMock

import pytest

from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController


def _go(tmp_path, *argv):
    with pytest.raises(SystemExit) as info:
        DreamferenceCLIController.run_cli(["--config", str(tmp_path / "d.toml"), *argv])
    return info.value.code


def _patch(monkeypatch, module, *names, result=0):
    mod = importlib.import_module(module)
    made = {}
    for name in names:
        mock = MagicMock(name=name)
        for attr in (
            "status",
            "start",
            "stop",
            "set_model",
            "run",
            "jobs",
            "logs",
            "cancel",
            "fetch",
            "execute",
            "serve",
            "sync",
            "dispatch",
            "ask",
            "after",
            "after_",
        ):
            getattr(mock, attr).return_value = result
        mock.enable.return_value = True
        mock.disable.return_value = True
        mock.add.return_value = True
        mock.remove.return_value = True
        mock.authorize.return_value = True
        mock.ensure.return_value = "node-id"
        mock.list_lines.return_value = ["a", "b"]
        mock.split_run_arguments.side_effect = lambda c: (
            MagicMock(memory=None, time=None, test=False, gpu=False, setup=None, out=None, bind=[]),
            c,
        )
        mock.after.return_value = (0, ["synced"])
        monkeypatch.setattr(mod, name, mock)
        made[name] = mock
    return made


@pytest.fixture
def node(monkeypatch):
    made = _patch(
        monkeypatch,
        "dreamference.node",
        "NodeAdvertiser",
        "NodeIdentity",
        "NodePairing",
        "NodeRemote",
        "NodeServe",
        "NodeJob",
        "NodeJobSender",
        "NodeModelSync",
    )
    made.update(_patch(monkeypatch, "dreamference.node.node_adapter_sync", "NodeAdapterSync"))
    made.update(_patch(monkeypatch, "dreamference.forge.forge_service", "ForgeService"))
    return made


@pytest.mark.parametrize(
    ("argv", "code"),
    [
        (["node", "enable", "--yes"], 0),
        (["node", "disable"], 0),
        (["node", "status"], 0),
        (["node", "status", "n1"], 0),
        (["node", "id"], 0),
        (["node", "list"], 0),
        (["node", "add", "n1"], 0),
        (["node", "remove", "n1"], 0),
        (["node", "set", "n1", "--model", "m"], 0),
        (["node", "start", "n1"], 0),
        (["node", "stop", "n1"], 0),
        (["node", "sync-model", "n1", "m"], 0),
        (["node", "run", "n1", "echo", "hi"], 0),
        (["node", "jobs", "n1"], 0),
        (["node", "logs", "j1"], 0),
        (["node", "cancel", "j1"], 0),
        (["node", "fetch", "j1"], 0),
        (["node", "job-exec", "j1"], 0),
        (["node", "serve-job", "--key", "k"], 0),
    ],
)
def test_node_commands_dispatch(node, tmp_path, argv, code, monkeypatch):
    monkeypatch.setenv("SSH_ORIGINAL_COMMAND", "x")
    assert _go(tmp_path, *argv) == code


def test_node_authorize_reads_standard_input(node, tmp_path, monkeypatch):
    import io
    import sys

    monkeypatch.setattr(sys, "stdin", io.StringIO("ssh-ed25519 AAA"))
    assert _go(tmp_path, "node", "authorize") == 0
    node["NodeServe"].authorize.assert_called_once_with("ssh-ed25519 AAA")


def test_node_enable_and_disable_failures_exit_one(node, tmp_path):
    node["NodeAdvertiser"].enable.return_value = False
    node["NodeAdvertiser"].disable.return_value = False
    node["NodePairing"].add.return_value = False
    node["NodePairing"].remove.return_value = False
    assert _go(tmp_path, "node", "enable") == 1
    assert _go(tmp_path, "node", "disable") == 1
    assert _go(tmp_path, "node", "add", "n") == 1
    assert _go(tmp_path, "node", "remove", "n") == 1
    node["ForgeService"].setup.assert_not_called()


def test_node_enable_sets_up_the_forge_and_disable_rebinds_it(node, tmp_path):
    assert _go(tmp_path, "node", "enable") == 0
    assert _go(tmp_path, "node", "disable") == 0
    node["ForgeService"].setup.assert_called_once()
    node["ForgeService"].rebind.assert_called_once()


def test_node_provision_prepare_askpass_and_power(monkeypatch, tmp_path):
    prov = _patch(monkeypatch, "dreamference.node.node_provisioner", "NodeProvisioner")
    prov["NodeProvisioner"].return_value.run.return_value = 0
    prep = _patch(monkeypatch, "dreamference.node.node_prepare", "NodePrepare")
    prep["NodePrepare"].run.return_value = True
    ask = _patch(monkeypatch, "dreamference.node.fleet_askpass", "FleetAskpass")
    _patch(monkeypatch, "dreamference.node.node_power_settings", "NodePowerSettings")
    assert _go(tmp_path, "node", "provision", "h1") == 0
    assert _go(tmp_path, "node", "prepare") == 0
    assert _go(tmp_path, "node", "askpass") == 0
    ask["FleetAskpass"].ask.assert_called_once()
    assert _go(tmp_path, "node", "power", "status", "n1") in (0, 1, 2)


def test_host_and_code_commands(monkeypatch, tmp_path):
    host = _patch(monkeypatch, "dreamference.vllm_server", "HostSafetySetup")
    host["HostSafetySetup"].check.return_value = True
    host["HostSafetySetup"].setup.return_value = False
    assert _go(tmp_path, "host", "check") == 0
    assert _go(tmp_path, "host", "setup", "--yes") == 1
    code = _patch(monkeypatch, "dreamference.cli.code_index_setup", "CodeIndexSetup")
    code["CodeIndexSetup"].install.return_value = True
    assert _go(tmp_path, "code", "setup") == 0


def test_matrix_commands(monkeypatch, tmp_path):
    m = _patch(monkeypatch, "dreamference.chat.matrix_homeserver", "MatrixHomeserver")[
        "MatrixHomeserver"
    ]
    for name in ("start", "stop", "status", "add_user", "set_push", "remove"):
        getattr(m, name).return_value = 0
    assert _go(tmp_path, "matrix", "start") == 0
    assert _go(tmp_path, "matrix", "stop") == 0
    assert _go(tmp_path, "matrix", "status") == 0
    assert _go(tmp_path, "matrix", "add-user", "bob") == 0
    assert _go(tmp_path, "matrix", "push", "on") == 0
    assert _go(tmp_path, "matrix", "remove", "--yes") == 0
    m.set_push.assert_called_with(True)


def test_remote_commands(monkeypatch, tmp_path):
    r = _patch(monkeypatch, "dreamference.remote.remote_access", "RemoteAccess")["RemoteAccess"]
    for name in ("setup", "status", "remove", "code", "bundle_file", "peers", "revoke"):
        getattr(r, name).return_value = 0
    assert _go(tmp_path, "remote", "setup", "x.example", "--yes") == 0
    assert _go(tmp_path, "remote", "status") == 0
    assert _go(tmp_path, "remote", "remove", "--yes") == 0
    assert _go(tmp_path, "remote", "code") == 0
    assert _go(tmp_path, "remote", "peers") == 0
    assert _go(tmp_path, "remote", "revoke", "p1") == 0


def test_night_commands(monkeypatch, tmp_path, capsys):
    ns = _patch(monkeypatch, "dreamference.night_shift", "NightShiftRunner", "NightShiftScheduler")
    ns["NightShiftScheduler"].enable.return_value = True
    ns["NightShiftScheduler"].disable.return_value = False
    ns["NightShiftScheduler"].status.return_value = "scheduled"
    ns["NightShiftRunner"].run.return_value = 0
    gate = _patch(monkeypatch, "dreamference.vllm_server.model_gate", "ModelGate")["ModelGate"]
    gate.describe.return_value = ["gate line"]
    ctl = DreamferenceCLIController
    for name in (
        "handle_gate_pause",
        "handle_night_autofill",
        "handle_night_every",
        "handle_night_archive",
    ):
        monkeypatch.setattr(ctl, name, classmethod(lambda cls, *a, **k: 0))
    assert _go(tmp_path, "night", "enable") == 0
    assert _go(tmp_path, "night", "disable") == 1
    assert _go(tmp_path, "night", "status") == 0
    assert "gate line" in capsys.readouterr().out
    assert _go(tmp_path, "night", "run") == 0
    assert _go(tmp_path, "night", "pause") == 0
    assert _go(tmp_path, "night", "resume") == 0
    assert _go(tmp_path, "night", "autofill") == 0
    assert _go(tmp_path, "night", "every", "1h") in (0, 2)
    assert _go(tmp_path, "night", "archive") == 0
    assert _go(tmp_path, "night") == 2


def test_docs_searxng_google_voice_and_unknown_subcommands(monkeypatch, tmp_path):
    docs = _patch(monkeypatch, "dreamference.runner.docs_index_setup", "DocsIndexSetup")
    docs["DocsIndexSetup"].install.return_value = False
    assert _go(tmp_path, "docs", "setup") == 1
    sx = _patch(monkeypatch, "dreamference.cli.searxng_command", "SearxngCommand")
    sx["SearxngCommand"].run.return_value = 0
    assert _go(tmp_path, "searxng", "status") == 0
    monkeypatch.setattr(DreamferenceCLIController, "_run_voice", classmethod(lambda cls, c: 0))
    assert _go(tmp_path, "voice", "status") == 0


def test_codex_start_stop_and_logs_and_index(monkeypatch, tmp_path, capsys):
    import subprocess

    from dreamference.cli import dreamference_cli_controller as ctl
    from dreamference.runner.codex_branded_builder import CodexBrandedBuilder

    monkeypatch.setattr(ctl.CodexInstaller, "install_if_missing", classmethod(lambda cls: True))
    monkeypatch.setattr(
        ctl.CodexInstaller, "get_codex_executable", classmethod(lambda cls: "/x/ragon")
    )
    popen = MagicMock()
    monkeypatch.setattr(subprocess, "Popen", popen)
    monkeypatch.setattr(subprocess, "call", MagicMock(return_value=0))
    DreamferenceCLIController.run_cli(["--config", str(tmp_path / "d.toml"), "codex", "start"])
    popen.assert_called_once()
    DreamferenceCLIController.run_cli(["--config", str(tmp_path / "d.toml"), "codex", "stop"])
    subprocess.call.return_value = 3
    assert _go(tmp_path, "codex", "stop") == 3
    monkeypatch.setattr(ctl.CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    assert _go(tmp_path, "codex", "start") == 1
    monkeypatch.setattr(ctl.CodexInstaller, "install_if_missing", classmethod(lambda cls: False))
    assert _go(tmp_path, "codex", "start") == 1
    audit = _patch(monkeypatch, "dreamference.audit", "EgressAudit")["EgressAudit"]
    ident = _patch(monkeypatch, "dreamference.node", "NodeIdentity")
    monkeypatch.setattr(CodexBrandedBuilder, "is_current", classmethod(lambda cls: False))
    monkeypatch.setattr(CodexBrandedBuilder, "build", classmethod(lambda cls, force=False: True))
    monkeypatch.setattr(
        ctl.DreamferenceCLIController, "_refuse_during_night_run", classmethod(lambda cls, w: None)
    )
    assert _go(tmp_path, "codex", "build") == 0
    audit.after_build.assert_called_once()
    monkeypatch.setattr(CodexBrandedBuilder, "build", classmethod(lambda cls, force=False: False))
    assert _go(tmp_path, "codex", "build") == 1
    assert ident
