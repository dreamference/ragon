"""The shared vectors of `ragon night run` (ragon-rs/src/night_run.rs), the Windows node's night
runner: the Python runner, driven by the same stand-in `ragon` (ragon-rs/vectors/night-run/
fake_ragon.py), must write the task record and the report the vectors hold, byte for byte once
paths, commits and times are replaced by placeholders. The Rust side's test reads the same files.

`REGENERATE_NIGHT_RUN_VECTORS=1` rewrites them from the Python runner.
"""

import json
import os
import re
import time
from datetime import datetime
from pathlib import Path

import pytest
from test_night_shift import git, queue, setup  # noqa: F401 - fixtures

from dreamference.night_shift import (
    NightShiftBestOfN,
    NightShiftQueue,
    NightShiftReport,
    NightShiftSettings,
)

ROOT = Path(__file__).resolve().parent.parent
VECTORS = ROOT / "ragon-rs" / "vectors" / "night-run"
FAKE = VECTORS / "fake_ragon.py"
TEST = "grep -q hi hello.txt"
PLAN = ["good", "bad", "good"]
STARTED = datetime(2015, 10, 1, 1, 0)


def normalize(text: str, night: Path, repo: Path, base: str) -> str:
    """The placeholders both sides write: paths, the base commit, times and durations."""
    text = text.replace(str(night), "<NIGHT>").replace(str(repo), "<REPO>")
    text = text.replace(base, "<BASE>").replace(base[:12], "<BASE12>")
    text = re.sub(r'"at": "[^"]*"', '"at": "<AT>"', text)
    text = re.sub(r'"wall_s": \d+', '"wall_s": 0', text)
    text = re.sub(r"finished \d\d:\d\d\.", "finished <HH:MM>.", text)
    return re.sub(r"wall time: [^,\n]*|, \d+ s,|, \d+ min \d+ s,", lambda m: _time(m.group(0)), text)


def _time(found: str) -> str:
    return "wall time: <T>" if found.startswith("wall") else ", <T>,"


def check(name: str, text: str) -> None:
    path = VECTORS / name
    if os.environ.get("REGENERATE_NIGHT_RUN_VECTORS"):
        path.write_text(text)
    assert text == path.read_text(), f"{name} differs from the shared vector"


def test_three_attempts_write_the_record_and_report_the_rust_runner_writes(
    setup, monkeypatch  # noqa: F811 - the imported fixture
):
    monkeypatch.setenv("FAKE_BON_PLAN", json.dumps(PLAN))
    record = queue(setup["night"], setup["repo"], test=TEST)
    run = NightShiftBestOfN.task_run(
        setup["night"],
        record,
        NightShiftSettings({"best_of": 3}),
        str(FAKE),
        time.time() + 600,
        model_host=None,
        window_end=time.time() + 600,
    )
    assert run.run() == "done"
    path = setup["night"] / "tasks" / f"{record['id']}.json"
    base = record["base"]
    check("best_of_record.json", normalize(path.read_text(), setup["night"], setup["repo"], base))
    task = NightShiftQueue.read(setup["night"], record["id"])
    report = NightShiftReport.render(STARTED, [task], ["a note"])
    check("best_of_report.md", normalize(report, setup["night"], setup["repo"], base))
    branches = git(setup["repo"], "branch", "--list", "night/*", "--format=%(refname:short)")
    assert branches.stdout.split() == [f"night/{record['id']}"]
