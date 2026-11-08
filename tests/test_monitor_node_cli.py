"""`ragon-admin monitor status [--json]`: it forwards to `ragon-node-monitor`, spawning nothing real."""

import subprocess

import pytest

from dreamference.cli import monitor_command
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController
from dreamference.cli.monitor_command import MonitorCommand


def _parser():
    return DreamferenceCLIController.build_parser()


@pytest.fixture
def fake_binary(monkeypatch):
    """A found binary, and a recorder for the spawn."""
    calls = []
    monkeypatch.setattr(MonitorCommand, "executable", classmethod(lambda cls: "/fake/monitor"))

    def run(command, **kwargs):
        calls.append(command)
        return subprocess.CompletedProcess(
            command, 0, stdout='{"node":{"state":"ok"}}\n', stderr=""
        )

    monkeypatch.setattr(monitor_command.subprocess, "run", run)
    return calls


def test_json_prints_the_binarys_output_as_is(fake_binary, capsys):
    args = _parser().parse_args(["monitor", "status", "--json"])
    assert MonitorCommand.run(args) == 0
    assert fake_binary == [["/fake/monitor", "status", "--json"]]
    assert capsys.readouterr().out == '{"node":{"state":"ok"}}\n'


def test_without_json_asks_for_the_table(fake_binary):
    args = _parser().parse_args(["monitor", "status"])
    assert MonitorCommand.run(args) == 0
    assert fake_binary == [["/fake/monitor", "status"]]


def test_the_binarys_exit_code_and_stderr_pass_through(monkeypatch, capsys):
    monkeypatch.setattr(MonitorCommand, "executable", classmethod(lambda cls: "/fake/monitor"))
    monkeypatch.setattr(
        monitor_command.subprocess,
        "run",
        lambda command, **kw: subprocess.CompletedProcess(command, 2, stdout="", stderr="usage\n"),
    )
    assert MonitorCommand.status(False) == 2
    assert "usage" in capsys.readouterr().err


def test_a_missing_binary_says_it_is_not_built_and_spawns_nothing(monkeypatch, capsys):
    monkeypatch.setattr(MonitorCommand, "executable", classmethod(lambda cls: None))
    monkeypatch.setattr(
        monitor_command.subprocess, "run", lambda *a, **k: pytest.fail("nothing is spawned")
    )
    assert MonitorCommand.status(True) == 1
    assert "not built" in capsys.readouterr().err


def test_a_spawn_failure_is_reported(monkeypatch, capsys):
    monkeypatch.setattr(MonitorCommand, "executable", classmethod(lambda cls: "/fake/monitor"))

    def boom(*a, **k):
        raise OSError("denied")

    monkeypatch.setattr(monitor_command.subprocess, "run", boom)
    assert MonitorCommand.status(False) == 1
    assert "denied" in capsys.readouterr().err


def test_the_binary_is_looked_for_beside_ragon(monkeypatch, tmp_path):
    monkeypatch.setattr(monitor_command, "INSTALL_DIR", str(tmp_path))
    assert MonitorCommand.executable() is None
    binary = tmp_path / "bin" / monitor_command.MONITOR_BINARY
    binary.parent.mkdir()
    binary.write_text("#!/bin/sh\n")
    binary.chmod(0o755)
    assert MonitorCommand.executable() == str(binary)


@pytest.mark.parametrize(
    "argv", [["monitor", "status", "--bogus"], ["monitor", "status", "extra"], ["monitor", "nope"]]
)
def test_unknown_arguments_are_rejected(argv):
    with pytest.raises(SystemExit) as error:
        _parser().parse_args(argv)
    assert error.value.code == 2


def test_no_subcommand_prints_the_usage(capsys):
    args = _parser().parse_args(["monitor"])
    assert MonitorCommand.run(args) == 2
    assert "usage" in capsys.readouterr().out
