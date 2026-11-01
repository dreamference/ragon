"""`ragon-admin night every|autofill|archive|pause|resume`: the handlers, with Night Shift stubbed."""

import argparse
from types import SimpleNamespace

import pytest

from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as Cli
from dreamference.night_shift import (
    NightShiftArchive,
    NightShiftAutofill,
    NightShiftQueue,
    NightShiftRecurring,
    NightShiftSettings,
)
from dreamference.runner.codex_installer import CodexInstaller
from dreamference.vllm_server.model_gate import ModelGate


@pytest.fixture(autouse=True)
def night_dir(tmp_path, monkeypatch):
    monkeypatch.setattr(NightShiftQueue, "night_dir", classmethod(lambda cls: tmp_path))
    return tmp_path


def _every(command, **kw):
    base = {
        "weekly": None,
        "days": None,
        "every": None,
        "monthly": None,
        "task": ["t"],
        "test": None,
    }
    return argparse.Namespace(night_every_command=command, **{**base, **kw})


def test_every_add_without_a_cadence_flag_is_daily(monkeypatch, capsys):
    seen = {}

    def add(night_dir, cwd, task, cadence, value, test):
        seen.update(task=task, cadence=cadence, value=value)
        return {"id": "r1"}

    monkeypatch.setattr(NightShiftRecurring, "add", staticmethod(add))
    monkeypatch.setattr(NightShiftRecurring, "describe", staticmethod(lambda t: "every day"))
    assert Cli.handle_night_every(_every("add", task=["fix", "it"])) == 0
    assert seen == {"task": "fix it", "cadence": "daily", "value": None}
    assert "Repeating task r1, every day" in capsys.readouterr().out


def test_every_add_uses_the_first_cadence_flag_that_is_set(monkeypatch):
    seen = {}

    def add(night_dir, cwd, task, cadence, value, test):
        seen.update(cadence=cadence, value=value)
        return {"id": "r"}

    monkeypatch.setattr(NightShiftRecurring, "add", staticmethod(add))
    monkeypatch.setattr(NightShiftRecurring, "describe", staticmethod(lambda t: ""))
    Cli.handle_night_every(_every("add", days=3))
    assert seen == {"cadence": "days", "value": 3}


def test_every_add_reports_a_refused_task(monkeypatch, capsys):
    def add(*args):
        raise ValueError("no such day")

    monkeypatch.setattr(NightShiftRecurring, "add", staticmethod(add))
    assert Cli.handle_night_every(_every("add")) == 1
    assert "no such day" in capsys.readouterr().out


def test_every_list_prints_one_json_line_per_template(monkeypatch, capsys):
    monkeypatch.setattr(
        NightShiftRecurring, "templates", staticmethod(lambda d: [{"id": "a"}, {"id": "b"}])
    )
    assert Cli.handle_night_every(_every("list")) == 0
    assert capsys.readouterr().out.splitlines() == ['{"id": "a"}', '{"id": "b"}']


@pytest.mark.parametrize(("dropped", "code"), [(True, 0), (False, 1)])
def test_every_drop_exit_code(monkeypatch, capsys, dropped, code):
    monkeypatch.setattr(NightShiftRecurring, "drop", staticmethod(lambda d, i: dropped))
    assert Cli.handle_night_every(_every("drop", id="x")) == code
    assert ("dropped" in capsys.readouterr().out) is dropped


def test_every_unknown_subcommand_is_usage(capsys):
    assert Cli.handle_night_every(_every(None)) == 2
    assert "usage: ragon-admin night every" in capsys.readouterr().out


def test_autofill_refuses_when_ragon_is_not_built(monkeypatch, capsys):
    monkeypatch.delenv("RAGON_NIGHT_RAGON_BIN", raising=False)
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    assert Cli.handle_night_autofill(False, None) == 1
    assert "ragon is not built" in capsys.readouterr().out


def test_autofill_prefers_the_environment_binary_and_maximum_over_the_setting(monkeypatch, capsys):
    monkeypatch.setenv("RAGON_NIGHT_RAGON_BIN", "/x/ragon")
    monkeypatch.setattr(NightShiftAutofill, "repo_for", staticmethod(lambda r: "repo"))
    seen = {}

    def fill(night_dir, repo, binary, today, maximum, dry_run):
        seen.update(repo=repo, binary=binary, maximum=maximum, dry_run=dry_run)
        return ["queued one"]

    monkeypatch.setattr(NightShiftAutofill, "fill", staticmethod(fill))
    assert Cli.handle_night_autofill(True, 2) == 0
    assert seen == {"repo": "repo", "binary": "/x/ragon", "maximum": 2, "dry_run": True}
    assert "🌙 queued one" in capsys.readouterr().out


def test_autofill_falls_back_to_the_settings_maximum(monkeypatch):
    monkeypatch.setenv("RAGON_NIGHT_RAGON_BIN", "/x/ragon")
    monkeypatch.setattr(NightShiftAutofill, "repo_for", staticmethod(lambda r: None))
    seen = {}
    monkeypatch.setattr(
        NightShiftAutofill,
        "fill",
        staticmethod(lambda *a, **k: seen.update(maximum=a[4]) or []),
    )
    Cli.handle_night_autofill(False, None)
    assert seen["maximum"] == NightShiftSettings().autofill_max


def test_archive_reports_a_refused_age(monkeypatch, capsys):
    def archive(d, weeks):
        raise ValueError("weeks must be positive")

    monkeypatch.setattr(NightShiftArchive, "archive", staticmethod(archive))
    assert Cli.handle_night_archive(0) == 1
    assert "weeks must be positive" in capsys.readouterr().out


@pytest.mark.parametrize(("holder", "shown"), [("a night run", "A night run"), (None, "A run")])
def test_archive_refuses_while_a_run_holds_the_queue(monkeypatch, capsys, holder, shown):
    monkeypatch.setattr(NightShiftArchive, "archive", staticmethod(lambda d, w: None))
    monkeypatch.setattr(NightShiftQueue, "runner_holder", classmethod(lambda cls, d=None: holder))
    assert Cli.handle_night_archive(4) == 1
    assert f"{shown} is working through the queue" in capsys.readouterr().out


@pytest.mark.parametrize("skipped", [[], ["a.log"]])
def test_archive_summary_warns_only_about_skipped_files(monkeypatch, capsys, skipped):
    outcome = SimpleNamespace(tasks=[1, 2], logs=3, reports=1, skipped=skipped)
    monkeypatch.setattr(NightShiftArchive, "archive", staticmethod(lambda d, w: outcome))
    assert Cli.handle_night_archive(4) == 0
    out = capsys.readouterr().out
    assert "Moved 2 finished runs older than 4 weeks, 3 log files and 1 reports" in out
    assert ("stayed" in out) is bool(skipped)


def test_resume_says_whether_a_pause_was_in_force(monkeypatch, capsys):
    monkeypatch.setattr(ModelGate, "describe", staticmethod(lambda host: ["gate line"]))
    monkeypatch.setattr(ModelGate, "resume", staticmethod(lambda: True))
    assert Cli.handle_gate_pause("h", None) == 0
    assert "Pause ended" in capsys.readouterr().out
    monkeypatch.setattr(ModelGate, "resume", staticmethod(lambda: False))
    assert Cli.handle_gate_pause("h", None) == 0
    out = capsys.readouterr().out
    assert "No pause was in force" in out
    assert "gate line" in out


def test_pause_refuses_a_bad_or_empty_duration(monkeypatch, capsys):
    assert Cli.handle_gate_pause("h", "banana") == 2
    assert "--for:" in capsys.readouterr().out
    monkeypatch.setattr(NightShiftSettings, "parse_duration", staticmethod(lambda d: 0))
    assert Cli.handle_gate_pause("h", "0") == 2
    assert "longer than nothing" in capsys.readouterr().out


def test_pause_starts_the_pause_for_the_parsed_seconds(monkeypatch, capsys):
    seen = []
    monkeypatch.setattr(NightShiftSettings, "parse_duration", staticmethod(lambda d: 90))
    monkeypatch.setattr(ModelGate, "pause", staticmethod(lambda s: seen.append(s) or {"until": 0}))
    monkeypatch.setattr(ModelGate, "describe", staticmethod(lambda host: ["gate line"]))
    assert Cli.handle_gate_pause("h", "90s") == 0
    assert seen == [90]
    assert "lets every request through until" in capsys.readouterr().out
