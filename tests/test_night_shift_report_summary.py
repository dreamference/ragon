"""The morning report's summary block under its title."""

from datetime import datetime

from dreamference.night_shift.night_shift_report import NightShiftReport


def _task(task_id: str, status: str, text: str = "fix it", **extra: object) -> dict:
    return {"id": task_id, "status": status, "task": text, "repo": "/r", "base": "a" * 40, **extra}


def _render(tasks: list[dict]) -> str:
    return NightShiftReport.render(datetime(2026, 1, 1, 22, 0), tasks, [])


def _summary(text: str) -> list[str]:
    block = text.split("## Summary\n", 1)[1].split("\n## ", 1)[0]
    return [line for line in block.splitlines() if line]


def test_summary_sits_under_the_title_and_blocks_stay() -> None:
    text = _render([_task("t1", "done", result={"branch": "night/t1"})])
    assert text.index("# Night Shift") < text.index("## Summary") < text.index("## /r")
    assert "### t1 — done" in text and "## Review" in text


def test_every_status_is_counted_and_ordered() -> None:
    statuses = [
        "cancelled",
        "no-change",
        "stalled",
        "failed",
        "interrupted",
        "sent",
        "running",
        "done",
    ]
    tasks = [_task(f"t{i}", s) for i, s in enumerate(statuses)]
    tasks[-1]["result"] = {"branch": "night/t7"}
    lines = _summary(_render(tasks))
    assert (
        lines[0]
        == "1 done, 1 no-change, 1 stalled, 1 failed, 1 interrupted, 1 sent, 1 running, 1 cancelled"
    )
    ids = [line.split()[1] for line in lines[1:]]
    assert ids == ["t7", "t2", "t3", "t4", "t5", "t6", "t0", "t1"]
    assert "ragon night apply t7" in lines[1] and "ragon night show t2" in lines[2]
    assert "(nothing to do)" in lines[-1]


def test_best_of_task_and_remote_task_lines() -> None:
    best = _task(
        "b1",
        "done",
        "first line\nsecond",
        result={"branch": "night/b1", "best_of": {"n": 3, "ran": 3, "chosen": 1}},
    )
    remote = _task("r1", "sent", on="box", sent_to={"name": "box"})
    lines = _summary(_render([remote, best]))
    assert lines[1].startswith("- b1 (ready to apply): first line — ")
    assert "second" not in lines[1]
    assert "not back yet" in lines[2] and "ragon night show r1" in lines[2]


def test_done_without_a_branch_needs_a_look_and_control_characters_go() -> None:
    text = _render([_task("t1", "done", "evil \x1b]52;c;x\x07 task")])
    assert "\x1b" not in text and "\x07" not in text
    assert "(needs a look)" in _summary(text)[1]
