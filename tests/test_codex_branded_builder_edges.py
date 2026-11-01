"""The branded builder's source commit, the UI build's failure messages and the Windows build lock."""

import subprocess
import sys
import types
from pathlib import Path
from unittest.mock import patch

import pytest

from dreamference.runner import codex_branded_builder as builder_module
from dreamference.runner.codex_branded_builder import CodexBrandedBuilder


def test_source_commit_is_the_checked_out_hash_only_with_a_codex_tree(tmp_path):
    (tmp_path / "codex-rs").mkdir()
    ok = subprocess.CompletedProcess([], 0, "abc123\n", "")
    with (
        patch.object(builder_module, "CODEX_SUBMODULE_DIR", str(tmp_path)),
        patch.object(builder_module.subprocess, "run", return_value=ok),
    ):
        assert CodexBrandedBuilder.source_commit() == "abc123"
    (tmp_path / "codex-rs").rmdir()
    with (
        patch.object(builder_module, "CODEX_SUBMODULE_DIR", str(tmp_path)),
        patch.object(builder_module.subprocess, "run", return_value=ok),
    ):
        assert CodexBrandedBuilder.source_commit() is None


@pytest.mark.parametrize(
    "failing, words",
    [
        (["ci", "--no-audit", "--no-fund"], "npm ci failed"),
        (["run", "build"], "did not build"),
        (None, "wrote no index.html"),
    ],
)
def test_the_ui_build_says_which_step_failed(tmp_path, capsys, failing, words):
    ui = tmp_path / "ui"
    ui.mkdir()
    (ui / "package.json").write_text("{}")

    def fake_call(command, cwd=None, env=None):
        return 1 if command[1:] == failing else 0

    with (
        patch.object(builder_module, "WEB_UI_DIR", str(ui)),
        patch.object(builder_module, "WEB_UI_DIST", str(ui / "dist")),
        patch.object(builder_module.shutil, "which", return_value="/usr/bin/npm"),
        patch.object(builder_module.subprocess, "call", side_effect=fake_call),
    ):
        assert CodexBrandedBuilder.build_web_ui() is None
    assert words in capsys.readouterr().out


def test_the_ui_project_missing_is_named(tmp_path, capsys):
    with patch.object(builder_module, "WEB_UI_DIR", str(tmp_path)):
        assert CodexBrandedBuilder.build_web_ui() is None
    assert "project is missing" in capsys.readouterr().out


def test_the_windows_lock_waits_once_and_then_takes_the_lock(tmp_path, capsys, monkeypatch):
    attempts: list[int] = []

    def locking(fileno, mode, size):
        attempts.append(fileno)
        if len(attempts) < 3:
            raise OSError("held")

    fake = types.SimpleNamespace(locking=locking, LK_NBLCK=2)
    sleeps: list[float] = []
    monkeypatch.setitem(sys.modules, "msvcrt", fake)
    monkeypatch.setattr("time.sleep", sleeps.append)
    with (
        patch.object(builder_module, "IS_WINDOWS", True),
        Path(tmp_path / "lock").open("w") as handle,
    ):
        CodexBrandedBuilder._lock_exclusive(handle)
    assert len(attempts) == 3 and sleeps == [2, 2]
    assert capsys.readouterr().out.count("Another ragon build is running") == 1
