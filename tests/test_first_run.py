"""`ragon-admin init`'s first run (owner Q105): every step is mocked, nothing touches the machine."""

import subprocess
import sys
from pathlib import Path

import pytest

from dreamference import hardware
from dreamference.cli import first_run as first_run_module
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController
from dreamference.cli.first_run import ADMIN_CODE, SMOKE_PROMPT, FirstRun
from dreamference.config.dreamference_config import DreamferenceConfig
from dreamference.hardware import AdapterVerifier
from dreamference.runner import CodexInstaller
from dreamference.runner.codex_branded_builder import CodexBrandedBuilder

MODEL = "qwen3.8-27b-nvfp4-dflash2"
RAGON = "/opt/ragon/bin/ragon"
LOCAL = "http://localhost:8000"
STEP_NAMES = ("host check", "model download", "server start", "ragon exec")


def config(tmp_path: Path, host: str = LOCAL, token: str | None = None) -> DreamferenceConfig:
    """A configuration read from no file, with the model server and the token given."""
    return DreamferenceConfig(
        config_file=str(tmp_path / "none.toml"), vllm_host=host, model=MODEL, hf_token=token
    )


def name_of(argv: list[str]) -> str:
    """Which step a recorded command line is."""
    if argv[0] == RAGON:
        return "ragon exec"
    words = [word for word in argv[3:] if not word.startswith("--") and not word.endswith(".toml")]
    return " ".join(words[:2])


@pytest.fixture
def machine(monkeypatch):
    """Docker, an installed `ragon`, no server answering, and a recorder for every command."""
    record: dict = {"calls": [], "fail": None, "served": None, "answer": "ready"}

    def run(argv, **kwargs):
        record["calls"].append((list(argv), kwargs))
        name = name_of(argv)
        if name == "ragon exec" and record["answer"]:
            Path(argv[argv.index("-o") + 1]).write_text(record["answer"], encoding="utf-8")
        return subprocess.CompletedProcess(argv, 1 if name == record["fail"] else 0)

    monkeypatch.setattr(first_run_module.subprocess, "run", run)
    monkeypatch.setattr(first_run_module.shutil, "which", lambda name: f"/usr/bin/{name}")
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: RAGON))
    monkeypatch.setattr(
        AdapterVerifier, "served_id", classmethod(lambda cls, host: record["served"])
    )
    monkeypatch.setattr(hardware, "model_key_for_served_id", lambda served, preferred=None: served)
    return record


def names(record) -> list[str]:
    return [name_of(argv) for argv, _ in record["calls"]]


def progress_lines(out: str) -> list[str]:
    return [line for line in out.splitlines() if line.startswith("[")]


def test_the_four_steps_run_in_order_with_one_progress_line_each(tmp_path, machine, capsys):
    assert FirstRun.run(config(tmp_path), str(tmp_path / "mine.toml"), None) == 0
    assert names(machine) == list(STEP_NAMES)
    lines = progress_lines(capsys.readouterr().out)
    assert [line.split(" …")[0] for line in lines] == [
        "[1/4] install checks",
        "[2/4] model download",
        "[3/4] server start",
        "[4/4] first ragon run",
    ]
    assert all("✅" in line for line in lines)
    assert "ragon answered: ready" in lines[-1]
    # Every step runs with no terminal on its input and its output in the first run's log.
    for _, kwargs in machine["calls"]:
        assert kwargs["stdin"] is subprocess.DEVNULL
        assert kwargs["stdout"].name == str(FirstRun.log_path())
    assert FirstRun.log_path().read_text(encoding="utf-8").count("\n$ ") == 4


def test_each_step_is_a_command_of_its_own(tmp_path, machine):
    FirstRun.run(config(tmp_path), "mine.toml", MODEL)
    parser = DreamferenceCLIController.build_parser()
    admin_calls = [argv for argv, _ in machine["calls"] if argv[0] == sys.executable]
    assert [argv[:3] for argv in admin_calls] == [[sys.executable, "-c", ADMIN_CODE]] * 3
    parsed = [parser.parse_args(argv[3:]) for argv in admin_calls]
    assert [(args.command, args.config) for args in parsed] == [
        ("host", "mine.toml"),
        ("model", "mine.toml"),
        ("server", "mine.toml"),
    ]
    assert parsed[0].host_command == "check"
    assert (parsed[1].model_command, parsed[1].model) == ("download", MODEL)
    assert (parsed[2].server_command, parsed[2].model) == ("start", MODEL)


def test_the_smoke_names_the_model_server_and_runs_read_only(tmp_path, machine):
    FirstRun.run(config(tmp_path), None, None)
    argv, kwargs = machine["calls"][-1]
    assert argv[:2] == [RAGON, "exec"] and argv[-1] == SMOKE_PROMPT
    assert argv[argv.index("-s") + 1] == "read-only" and "--skip-git-repo-check" in argv
    # An unattended caller names the server: a node browse is a multicast DNS query.
    assert kwargs["env"]["DREAMFERENCE_VLLM_HOST"] == LOCAL
    assert kwargs["timeout"] == first_run_module.SMOKE_TIMEOUT_S


@pytest.mark.parametrize("failing", STEP_NAMES)
def test_a_failing_step_stops_the_run_and_names_its_own_command(tmp_path, machine, capsys, failing):
    machine["fail"] = failing
    assert FirstRun.run(config(tmp_path), None, None) == 1
    ran = names(machine)
    assert ran == list(STEP_NAMES[: STEP_NAMES.index(failing) + 1])
    out = capsys.readouterr().out
    assert "❌" in progress_lines(out)[-1]
    standalone = {
        "host check": "ragon-admin host check",
        "model download": "ragon-admin model download",
        "server start": "ragon-admin server start",
        "ragon exec": "ragon exec",
    }[failing]
    assert f"Run this step alone: {standalone}" in out


def test_a_smoke_without_an_answer_fails(tmp_path, machine, capsys):
    machine["answer"] = ""
    assert FirstRun.run(config(tmp_path), None, None) == 1
    assert "ragon gave no answer" in capsys.readouterr().out


def test_a_server_already_serving_the_model_is_not_restarted(tmp_path, machine, capsys):
    machine["served"] = MODEL
    assert FirstRun.run(config(tmp_path), None, None) == 0
    assert names(machine) == ["host check", "model download", "ragon exec"]
    assert f"already serving {MODEL}" in capsys.readouterr().out


def test_a_server_serving_another_model_is_started(tmp_path, machine):
    machine["served"] = "some-other-model"
    FirstRun.run(config(tmp_path), None, None)
    assert "server start" in names(machine)


def test_missing_docker_fails_the_checks_before_any_command(tmp_path, machine, monkeypatch, capsys):
    monkeypatch.setattr(first_run_module.shutil, "which", lambda name: None)
    assert FirstRun.run(config(tmp_path), None, None) == 1
    assert machine["calls"] == []
    assert "docker was not found" in capsys.readouterr().out


@pytest.mark.parametrize(
    "source, fix", [(True, "`ragon-admin codex build`"), (False, "the installer")]
)
def test_a_missing_ragon_names_how_to_install_it(
    tmp_path, machine, monkeypatch, capsys, source, fix
):
    monkeypatch.setattr(CodexInstaller, "get_codex_executable", classmethod(lambda cls: None))
    monkeypatch.setattr(CodexBrandedBuilder, "has_source", classmethod(lambda cls: source))
    assert FirstRun.run(config(tmp_path), None, None) == 1
    assert machine["calls"] == []
    assert f"ragon is not installed: {fix} installs it" in capsys.readouterr().out


def test_the_token_reaches_the_steps_by_environment_never_the_command_line(tmp_path, machine):
    FirstRun.run(config(tmp_path, token="hf_secret"), None, None)
    for argv, kwargs in machine["calls"]:
        assert "hf_secret" not in " ".join(argv)
        if argv[0] == sys.executable:
            assert kwargs["env"]["HF_TOKEN"] == "hf_secret"


def test_a_remote_model_server_is_neither_downloaded_nor_started(tmp_path, machine, capsys):
    assert FirstRun.run(config(tmp_path, host="http://node.local:8000"), None, None) == 0
    assert names(machine) == ["host check", "ragon exec"]
    assert capsys.readouterr().out.count("skipped: the model server is http://node.local:8000") == 2


def test_init_exits_with_the_first_runs_failure(monkeypatch, tmp_path):
    from dreamference.context_engine import ContextEngine

    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(
        ContextEngine,
        "index_workspace",
        lambda self, force_reindex=False: {"total_indexed_files": 0, "total_ast_symbols": 0},
    )
    monkeypatch.setattr(FirstRun, "run", classmethod(lambda cls, config, path, model: 1))
    with pytest.raises(SystemExit) as stopped:
        DreamferenceCLIController.run_cli(["--config", str(tmp_path / "mine.toml"), "init"])
    assert stopped.value.code == 1


@pytest.mark.parametrize("seconds, shown", [(4.2, "4 s"), (185, "3 min 5 s")])
def test_a_step_duration_reads_in_minutes_and_seconds(seconds, shown):
    assert FirstRun.duration(seconds) == shown
