"""A night run that has no `ragon` to run is refused before it takes the runner lock.

No test here starts `ragon` or a model server: the lock is replaced, so no run goes further.
"""

import contextlib

from dreamference.night_shift import NightShiftQueue, NightShiftRunner, NightShiftSettings
from dreamference.runner.codex_installer import CodexInstaller


def _lock(monkeypatch) -> list:
    taken = []

    @contextlib.contextmanager
    def runner_lock(night_dir):
        taken.append(night_dir)
        yield False

    monkeypatch.setattr(NightShiftQueue, "runner_lock", staticmethod(runner_lock))
    return taken


def test_a_night_run_without_a_built_ragon_is_refused_before_the_lock(
    tmp_path, monkeypatch, capsys
):
    taken = _lock(monkeypatch)
    monkeypatch.delenv("RAGON_NIGHT_RAGON_BIN", raising=False)
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    code = NightShiftRunner.run(
        minutes=5, night_dir=tmp_path / "night", settings=NightShiftSettings({})
    )
    assert code == 1
    assert "ragon is not built" in capsys.readouterr().out
    assert taken == []


def test_a_night_run_takes_ragon_from_the_environment_before_the_installed_one(
    tmp_path, monkeypatch, capsys
):
    # The environment names a ragon, so the missing installed one does not refuse the run: it goes
    # on to the lock (held elsewhere here) with the configured model server.
    taken = _lock(monkeypatch)
    monkeypatch.setenv("RAGON_NIGHT_RAGON_BIN", str(tmp_path / "ragon"))
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    code = NightShiftRunner.run(
        minutes=5, night_dir=tmp_path / "night", settings=NightShiftSettings({})
    )
    assert code == 1
    out = capsys.readouterr().out
    assert "ragon is not built" not in out and "Another night run holds the runner lock" in out
    assert taken == [tmp_path / "night"]
