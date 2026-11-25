"""`logs mcp`, `desktop`, `gmail`, `run`, `clear` and `status` dispatch through run_cli, mocked."""

import sqlite3
from unittest.mock import MagicMock

import pytest

from dreamference.cli import dreamference_cli_controller as ctl
from dreamference.runner import CodexInstaller


def _go(tmp_path, *argv):
    with pytest.raises(SystemExit) as info:
        ctl.DreamferenceCLIController.run_cli(["--config", str(tmp_path / "d.toml"), *argv])
    return info.value.code


def _home(monkeypatch, tmp_path):
    monkeypatch.setattr(CodexInstaller, "home_dir", classmethod(lambda cls: str(tmp_path)))


def test_logs_mcp_without_a_database(monkeypatch, tmp_path, capsys):
    _home(monkeypatch, tmp_path)
    assert _go(tmp_path, "logs", "mcp") == 1
    assert "No ragon log database" in capsys.readouterr().out


def test_logs_mcp_unreadable_database(monkeypatch, tmp_path, capsys):
    _home(monkeypatch, tmp_path)
    (tmp_path / "logs_2.sqlite").write_text("not a database")
    assert _go(tmp_path, "logs", "mcp") == 1
    assert "Could not read" in capsys.readouterr().out


def test_logs_mcp_lists_lifecycle_entries(monkeypatch, tmp_path, capsys):
    _home(monkeypatch, tmp_path)
    conn = sqlite3.connect(tmp_path / "logs_2.sqlite")
    conn.execute("CREATE TABLE logs (msg TEXT)")
    for msg in (
        "server_name=alpha} Service initialized as client",
        "server_name=beta} task cancelled",
        "server_name=beta} task cancelled",
        "server_name=gamma} nothing of note",
        "unrelated",
    ):
        conn.execute("INSERT INTO logs VALUES (?)", (msg,))
    conn.commit()
    conn.close()
    assert _go(tmp_path, "logs", "mcp") == 0
    out = capsys.readouterr().out
    assert "alpha" in out and "task cancelled" in out and "gamma" not in out


def test_logs_mcp_with_no_entries(monkeypatch, tmp_path, capsys):
    _home(monkeypatch, tmp_path)
    conn = sqlite3.connect(tmp_path / "logs_2.sqlite")
    conn.execute("CREATE TABLE logs (msg TEXT)")
    conn.commit()
    conn.close()
    assert _go(tmp_path, "logs", "mcp") == 0
    assert "No MCP lifecycle entries" in capsys.readouterr().out


@pytest.mark.parametrize("sub", ["install", "run", "build", "status"])
def test_desktop_subcommands(monkeypatch, tmp_path, sub):
    import dreamference.chat as chat
    from dreamference.node import NodeIdentity

    runner = MagicMock()
    for name in ("install", "run", "build", "status"):
        getattr(runner, name).return_value = 7
    monkeypatch.setattr(chat, "DesktopRunner", runner)
    monkeypatch.setattr(NodeIdentity, "ensure", classmethod(lambda cls: None))
    assert _go(tmp_path, "desktop", sub) == 7


def test_gmail_exit_code_comes_from_the_handler(monkeypatch, tmp_path):
    monkeypatch.setattr(
        ctl.DreamferenceCLIController, "handle_gmail", classmethod(lambda cls, a: 3)
    )
    assert _go(tmp_path, "gmail", "status") == 3


def test_clear_caches(monkeypatch, tmp_path):
    monkeypatch.setattr(ctl, "clear_model_cache", lambda: True)
    monkeypatch.setattr(ctl, "clear_tensorizer_cache", lambda: False)
    assert _go(tmp_path, "clear", "model-cache") == 0
    assert _go(tmp_path, "clear", "tensorize-cache") == 1
    assert _go(tmp_path, "clear-tensorize-cache") == 1


def test_status_command(monkeypatch, tmp_path):
    called = []
    monkeypatch.setattr(
        ctl.DreamferenceCLIController, "handle_status", classmethod(lambda cls: called.append(1))
    )
    ctl.DreamferenceCLIController.run_cli(["--config", str(tmp_path / "d.toml"), "status"])
    assert called


def _wheel_only(monkeypatch, tmp_path, *, source: bool, binary: bool):
    from dreamference.runner import codex_branded_builder as builder

    monkeypatch.setattr(builder, "INSTALL_DIR", str(tmp_path / "install"))
    monkeypatch.setattr(builder.CodexBrandedBuilder, "has_source", classmethod(lambda cls: source))
    if binary:
        (tmp_path / "install" / "bin").mkdir(parents=True)
        exe = tmp_path / "install" / "bin" / ("ragon" + builder.EXE_SUFFIX)
        exe.write_text("#!/bin/sh\n")
        exe.chmod(0o755)


@pytest.mark.parametrize("command", ["init", "run", "status"])
def test_a_wheel_only_install_says_where_the_binaries_come_from(
    monkeypatch, tmp_path, capsys, command
):
    _wheel_only(monkeypatch, tmp_path, source=False, binary=False)
    from dreamference.vllm_server import SandboxPrerequisite

    monkeypatch.setattr(SandboxPrerequisite, "gate", classmethod(lambda cls, *a: False))
    assert _go(tmp_path, command, *(["x"] if command == "run" else [])) == 1
    err = capsys.readouterr().err
    assert err.count("not installed") == 1
    assert "releases/latest/download/install.sh" in err
    assert "releases/latest/download/install.ps1" in err


@pytest.mark.parametrize(("source", "binary"), [(True, False), (False, True), (True, True)])
def test_the_line_is_absent_with_a_checkout_or_installed_binaries(
    monkeypatch, tmp_path, capsys, source, binary
):
    _wheel_only(monkeypatch, tmp_path, source=source, binary=binary)
    from dreamference.vllm_server import SandboxPrerequisite

    monkeypatch.setattr(SandboxPrerequisite, "gate", classmethod(lambda cls, *a: False))
    assert _go(tmp_path, "status") == 1
    assert "not installed" not in capsys.readouterr().err


def test_other_commands_stay_silent_on_a_wheel_only_install(monkeypatch, tmp_path, capsys):
    _wheel_only(monkeypatch, tmp_path, source=False, binary=False)
    from dreamference.vllm_server import SandboxPrerequisite

    monkeypatch.setattr(SandboxPrerequisite, "gate", classmethod(lambda cls, *a: False))
    assert _go(tmp_path, "clear", "model-cache") == 1
    assert "not installed" not in capsys.readouterr().err
