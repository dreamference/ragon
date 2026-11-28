"""The node's shared app-server, started with the gate (spec §7.1 A3, A4, A7).

All mocked: systemctl is the gate's seam, the socket folders live in the test's tmp dir.
"""

import os
import re
import stat
from pathlib import Path

import pytest

from dreamference.runner.codex_installer import CodexInstaller
from dreamference.vllm_server import model_gate
from dreamference.vllm_server.model_gate import ModelGate

APP_SERVER_RS = Path(__file__).resolve().parents[1] / "ragon-rs" / "web" / "src" / "app_server.rs"


@pytest.fixture
def folders(tmp_path, monkeypatch):
    runtime = tmp_path / "run"
    runtime.mkdir(mode=0o700)
    daemon = tmp_path / "codex-daemon"
    monkeypatch.setenv("XDG_RUNTIME_DIR", str(runtime))
    monkeypatch.setattr(ModelGate, "app_server_daemon_dir", classmethod(lambda cls: daemon))
    return runtime / "ragon", daemon


@pytest.fixture
def systemd(tmp_path, monkeypatch, folders):
    calls: list[tuple[str, ...]] = []
    ragon = tmp_path / "bin" / "ragon"
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: str(ragon)))
    monkeypatch.setattr(model_gate.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(ModelGate, "systemctl", classmethod(lambda cls, *a: calls.append(a) or 0))
    return calls


def unit_file() -> Path:
    return Path.home() / ".config" / "systemd" / "user" / model_gate.APP_SERVER_UNIT


def test_the_shared_server_is_a_user_unit_of_the_gates_user(systemd, folders):
    assert ModelGate.start_app_server(8000)
    # A user unit (no User=, under ~/.config/systemd/user) runs as the user who ran the gate's start,
    # and the gate's container runs as that same uid:gid (A4).
    text = unit_file().read_text()
    assert "User=" not in text and "Group=" not in text
    gate = ModelGate.run_command(8000, "image")
    assert gate[gate.index("--user") + 1] == f"{os.getuid()}:{os.getgid()}"
    assert ("enable", model_gate.APP_SERVER_UNIT) in systemd
    assert ("restart", model_gate.APP_SERVER_UNIT) in systemd
    assert "Environment=DREAMFERENCE_VLLM_HOST=http://localhost:8000" in text
    assert "SyslogIdentifier=ragon-app-server" in text


def test_the_shared_server_listens_where_ragon_web_looks_with_feedback_and_realtime_off(
    systemd, folders
):
    ModelGate.start_app_server(8000)
    runtime, _ = folders
    exec_start = next(
        line for line in unit_file().read_text().splitlines() if line.startswith("ExecStart=")
    )
    assert "-c feedback.enabled=false" in exec_start
    assert "-c features.realtime_conversation=false" in exec_start
    assert exec_start.endswith(f"app-server --listen unix://{runtime / 'app-server.sock'}")


def test_an_unchanged_unit_is_started_not_restarted(systemd):
    ModelGate.start_app_server(8000)
    systemd.clear()
    assert ModelGate.start_app_server(8000)
    assert ("start", model_gate.APP_SERVER_UNIT) in systemd
    assert all(call[0] not in {"restart", "daemon-reload"} for call in systemd)


def test_the_socket_folders_are_private_and_mounted_at_their_own_paths(systemd, folders):
    runtime, daemon = folders
    assert ModelGate.prepare_app_server_dirs()
    for folder in (runtime, daemon):
        assert stat.S_IMODE(folder.stat().st_mode) == 0o700
    command = ModelGate.run_command(8000, "image")
    assert f"{runtime}:{runtime}" in command
    assert f"{daemon}:{daemon}" in command
    assert f"RAGON_APP_SERVER_SOCKET={runtime / 'app-server.sock'}" in command


def test_without_the_folders_nothing_is_mounted(systemd, folders):
    assert ModelGate.app_server_mounts() == []


def test_without_a_launcher_the_gate_mounts_nothing_and_creates_no_folder(folders):
    # Installed nowhere (as in every test): the gate's command does not depend on the host.
    runtime, daemon = folders
    assert ModelGate.app_server_launcher() is None
    assert ModelGate.app_server_mounts() == []
    assert not runtime.exists() and not daemon.exists()


def test_unit_words_escape_specifiers_variables_and_quotes():
    assert ModelGate.unit_word("/opt/ragon") == "/opt/ragon"
    assert ModelGate.unit_word("/a b/100%$x") == '"/a b/100%%$$x"'
    assert ModelGate.unit_word('a"b\\c') == '"a\\"b\\\\c"'


def test_without_ragon_or_systemd_nothing_is_written(systemd, monkeypatch):
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    assert not ModelGate.start_app_server(8000)
    assert systemd == []
    assert not unit_file().exists()


@pytest.mark.skipif(not APP_SERVER_RS.is_file(), reason="release install: no ragon-rs source")
def test_the_gate_and_ragon_web_pass_the_same_settings():
    source = APP_SERVER_RS.read_text()
    block = re.search(r"pub const SHARED_SETTINGS[^=]*=\s*\[(.*?)\];", source, re.S)
    assert block is not None
    assert tuple(re.findall(r'"([^"]+)"', block.group(1))) == model_gate.APP_SERVER_SETTINGS
