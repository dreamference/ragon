"""`ragon-admin adapter train`: the plan, the run directory, the stages and the egress gate."""

import argparse
import os
import subprocess
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

import pytest

from dreamference.cli.adapter_command import AdapterCommand
from dreamference.hardware.adapter_train_run import AdapterTrainRun
from dreamference.hardware.adapter_train_tasks import AdapterTrainTasks

BASE_TIME = 1_700_000_000
CONFIG = SimpleNamespace(vllm_host="localhost", model="m")


def test_adapter_train_tasks_parse_rejects_a_short_header_and_a_binary_numstat() -> None:
    assert AdapterTrainTasks._parse("a\x00b") is None
    assert (
        AdapterTrainTasks._parse("h\x00p\x00n\x00e\x001700000000\x00Fix the thing\x02-\t-\tmod.py")
        is None
    )


def build_repo(path: Path, commits: int = 60) -> None:
    path.mkdir()
    env = {
        **os.environ,
        "GIT_AUTHOR_NAME": "Dev", "GIT_AUTHOR_EMAIL": "dev@example.org",
        "GIT_COMMITTER_NAME": "Dev", "GIT_COMMITTER_EMAIL": "dev@example.org",
    }  # fmt: skip

    def git(*args: str, when: int = BASE_TIME) -> None:
        subprocess.run(
            ["git", "-C", str(path), *args],
            check=True,
            capture_output=True,
            env={**env, "GIT_AUTHOR_DATE": f"{when} +0000", "GIT_COMMITTER_DATE": f"{when} +0000"},
        )

    git("init", "-q", "-b", "main")
    for i in range(commits):
        (path / f"mod{i}.py").write_text(f"def f{i}():\n    return {i}\n")
        (path / f"test_mod{i}.py").write_text(
            f"from mod{i} import f{i}\n\ndef test():\n    assert f{i}() == {i}\n"
        )
        git("add", "-A")
        git(
            "commit",
            "-q",
            "-m",
            f"Add function number {i} with its test",
            when=BASE_TIME + i * 86400,
        )


def parse(*argv: str) -> argparse.Namespace:
    root = argparse.ArgumentParser()
    AdapterCommand.add_parser(root.add_subparsers(dest="command"))
    return root.parse_args(["adapter", "train", *argv])


@pytest.fixture
def repo(tmp_path: Path) -> Path:
    build_repo(tmp_path / "repo")
    return tmp_path / "repo"


def test_dry_run_prints_the_plan_and_writes_nothing(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    args = parse("mine", "--repo", str(repo), "--dry-run")
    assert AdapterCommand.run(args, CONFIG) == 0  # type: ignore[arg-type]
    out = capsys.readouterr().out
    assert "candidate tasks" in out and "held out" in out
    assert "token estimate" in out and "GPU hours" in out
    assert "trainer: local" in out and "leaves the node: nothing" in out
    assert not AdapterTrainRun.root().exists()


def test_dry_run_names_what_would_leave_the_node(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    args = parse("mine", "--repo", str(repo), "--trainer", "ssh://gpu1", "--dry-run")
    with patch(
        "dreamference.config.airgapped_resolver.AirgappedResolver.level", return_value="off"
    ):
        assert AdapterCommand.run(args, CONFIG) == 0  # type: ignore[arg-type]
    assert "sent to gpu1 only" in capsys.readouterr().out


def test_a_remote_trainer_is_refused_at_airgapped_on(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    args = parse("mine", "--repo", str(repo), "--trainer", "ssh://gpu1", "--dry-run")
    with patch("dreamference.config.airgapped_resolver.AirgappedResolver.level", return_value="on"):
        assert AdapterCommand.run(args, CONFIG) == 1  # type: ignore[arg-type]
    assert "/airgapped on" in capsys.readouterr().out


def test_an_unreadable_level_fails_closed(repo: Path) -> None:
    args = parse("mine", "--repo", str(repo), "--trainer", "ssh://gpu1", "--dry-run")
    with patch(
        "dreamference.config.airgapped_resolver.AirgappedResolver.binary",
        return_value="/nonexistent/ragon",
    ):
        assert AdapterCommand.run(args, CONFIG) == 1  # type: ignore[arg-type]


def test_a_bad_trainer_and_a_bad_name_are_refused(repo: Path) -> None:
    assert (
        AdapterCommand.run(
            parse("mine", "--repo", str(repo), "--trainer", "gpu1", "--dry-run"), None
        )
        == 2
    )  # type: ignore[arg-type]
    assert AdapterCommand.run(parse("../x", "--repo", str(repo), "--dry-run"), None) == 2  # type: ignore[arg-type]


def test_the_data_stage_writes_under_the_run_directory_and_the_next_says_its_lane(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    with (
        patch("dreamference.node.node_identity.NodeIdentity.read", return_value=object()),
        patch("dreamference.cli.adapter_command.AdapterVerifier.served_base", return_value=None),
    ):
        assert AdapterCommand.run(parse("mine", "--repo", str(repo)), CONFIG) == 0  # type: ignore[arg-type]
        run = AdapterTrainRun("mine")
        assert (run.directory / "tasks.json").exists()
        assert run.directory.is_relative_to(Path.home())
        assert not (repo / "tasks.json").exists()
        assert run.next_stage() == "generate"
        assert AdapterCommand.run(parse("mine", "--repo", str(repo), "--resume"), CONFIG) == 1  # type: ignore[arg-type]
    assert "lane adt-generate" in capsys.readouterr().out


def test_the_first_upload_asks_for_the_host_typed(repo: Path) -> None:
    run = AdapterTrainRun("mine")
    with patch("builtins.input", return_value="wrong"):
        assert AdapterCommand._consent(run, "gpu1") is False
    assert run.state()["consented"] == []
    with patch("builtins.input", return_value="gpu1"):
        assert AdapterCommand._consent(run, "gpu1") is True
    assert AdapterCommand._consent(run, "gpu1") is True


def test_the_estimate_follows_the_plan() -> None:
    full = AdapterTrainRun.estimate(300)
    assert full["tokens"] == 6_000_000 and 6 < full["gpu_hours"] < 11
    assert AdapterTrainRun.estimate(300, budget_hours=1)["gpu_hours"] == pytest.approx(1)


def test_adapter_train_run_state_ignores_a_state_file_that_is_not_an_object(repo: Path) -> None:
    run = AdapterTrainRun("mine")
    run.directory.mkdir(parents=True)
    (run.directory / "state.json").write_text("[1, 2]", encoding="utf-8")
    assert run.state() == {"done": [], "consented": [], "left_node": []}


def test_adapter_command_train_refuses_an_existing_run_without_resume(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    AdapterTrainRun("mine").mark_done("data")
    with (
        patch("dreamference.node.node_identity.NodeIdentity.read", return_value=object()),
        patch("dreamference.cli.adapter_command.AdapterVerifier.served_base", return_value=None),
    ):
        assert AdapterCommand.run(parse("mine", "--repo", str(repo)), CONFIG) == 1  # type: ignore[arg-type]
    assert "add --resume" in capsys.readouterr().out


def test_adapter_command_train_says_when_every_stage_is_done(
    repo: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    run = AdapterTrainRun("mine")
    for stage in ("data", "generate", "train", "eval", "install"):
        run.mark_done(stage)
    with (
        patch("dreamference.node.node_identity.NodeIdentity.read", return_value=object()),
        patch("dreamference.cli.adapter_command.AdapterVerifier.served_base", return_value=None),
    ):
        assert AdapterCommand.run(parse("mine", "--repo", str(repo), "--resume"), CONFIG) == 0  # type: ignore[arg-type]
    assert "Every stage" in capsys.readouterr().out
