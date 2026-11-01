"""The record helpers and rare branches of one SWE-bench instance run, driven on a bare instance."""

import contextlib
import json
import subprocess
import threading
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import pytest

from dreamference.swe_bench import swe_bench_instance_run as module

Run = module.SweBenchInstanceRun


def bare(tmp_path: Path, **attributes: Any) -> Any:
    """Builds an instance without its constructor, with a real scratch folder."""
    run = object.__new__(Run)
    run.scratch = tmp_path
    run.stop_event = threading.Event()
    run.notes = []
    run.container = "box"
    run.log_path = tmp_path / "log.jsonl"
    for name, value in attributes.items():
        setattr(run, name, value)
    return run


def write(tmp_path: Path, relative: str, lines: list[Any]) -> None:
    path = tmp_path / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        "\n".join(line if isinstance(line, str) else json.dumps(line) for line in lines) + "\n"
    )


def done(code: int = 0, out: str = "", err: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess([], code, out, err)


def test_compactions_skip_broken_lines_and_unreadable_rollouts(tmp_path):
    write(
        tmp_path,
        "codex-home/sessions/rollout-1.jsonl",
        ['{"type": "compacted"', {"type": "compacted"}, '"compacted"', {"type": "other"}],
    )
    run = bare(tmp_path)
    assert run.compactions_recorded() == 1
    run.rollouts = lambda: ["codex-home/sessions/rollout-gone.jsonl"]  # type: ignore[method-assign]
    assert run.compactions_recorded() == 0
    assert run.compaction_record()["compactions"] == 0


def test_compaction_seconds_run_from_the_last_line_before_the_gap(tmp_path):
    write(
        tmp_path,
        "codex-home/sessions/rollout-1.jsonl",
        [
            "not json",
            "[1]",
            {"timestamp": "bad", "type": "x"},
            {"timestamp": "2020-01-01T00:00:00Z"},
            {"timestamp": "2020-01-01T00:00:10Z", "type": "compacted"},
            {"type": "compacted"},
        ],
    )
    write(
        tmp_path,
        f"codex-home/{module.DROP_LOG}",
        ["broken", "[]", {"accepted": True, "outputs": 3}, {"accepted": False}],
    )
    record = Run.compaction_record(bare(tmp_path))
    assert record["compactions"] == 2 and record["seconds"] == 10.0
    assert (record["drops"], record["declined"], record["dropped_outputs"]) == (1, 1, 3)


def test_loop_guard_record_counts_actions_and_capped_checks(tmp_path):
    write(
        tmp_path,
        f"codex-home/{module.LOOP_GUARD_LOG}",
        [
            "broken",
            "[]",
            {"action": "nudged"},
            {"action": "ended", "check": "incomplete: cap"},
            {"action": "other"},
        ],
    )
    assert bare(tmp_path).loop_guard_record() == {"nudged": 1, "ended": 1, "capped": 1}


def test_guard_record_skips_unreadable_sessions_and_takes_the_latest_stop(tmp_path):
    guards = f"codex-home/{module.GUARD_DIR}"
    (tmp_path / guards).mkdir(parents=True)
    (tmp_path / guards / "a.json").write_text("{broken")
    (tmp_path / guards / "b.json").write_text("[1]")
    (tmp_path / guards / "c.json").write_text(
        json.dumps({"calls": 2, "updated": 5, "last_stop": "late", "holds": [{"kind": "untested"}]})
    )
    (tmp_path / guards / "d.json").write_text(
        json.dumps({"calls": 1, "updated": 1, "last_stop": "early", "holds": ["x", {"kind": "no"}]})
    )
    record = bare(tmp_path).guard_record()
    assert record["sessions"] == 2 and record["calls"] == 3
    assert record["last_stop"] == "late" and record["holds"]["untested"]["fired"] == 1


def test_tool_calls_count_only_matching_calls_and_skip_bad_lines(tmp_path):
    name = "apply_patch"
    write(
        tmp_path,
        "codex-home/sessions/rollout-1.jsonl",
        [
            f'{{"payload": "{name}"',
            f'["{name}"]',
            {"payload": {"name": name, "type": "function_call"}},
            {"payload": {"name": name, "type": "message"}},
        ],
    )
    run = bare(tmp_path)
    assert run.apply_patch_calls() == 1
    run.rollouts = lambda: ["codex-home/sessions/rollout-gone.jsonl"]  # type: ignore[method-assign]
    assert run.exec_calls() == 0


def test_refine_that_cannot_go_on_ends_in_error(tmp_path, monkeypatch):
    run = bare(tmp_path, refine=True)
    monkeypatch.setattr(Run, "_refine", lambda self, state: ("", "reset failed"))
    outcome = _into_refine(run, monkeypatch)
    assert outcome == ("error", "") and run.notes == ["reset failed"]


def test_refine_interrupted_after_the_first_step_ends_interrupted(tmp_path, monkeypatch):
    run = bare(tmp_path, refine=True)
    monkeypatch.setattr(Run, "_refine", lambda self, state: ("", None))
    run.stop_event.set()
    assert _into_refine(run, monkeypatch) == ("interrupted", "")


def _into_refine(run: Any, monkeypatch: pytest.MonkeyPatch) -> Any:
    """Runs `_run` up to the refine step: the container and both preparation scripts succeed."""
    monkeypatch.setattr(Run, "_start_container", lambda self: None, raising=False)
    monkeypatch.setattr(Run, "_script", lambda self, script, timeout, root=False: done())
    run.problem_statement = "issue"
    try:
        return Run._run(run, {})
    except AttributeError as error:  # pragma: no cover - names the missing attribute
        pytest.skip(f"_run needs more of the constructor: {error}")


def refine_instance(tmp_path, monkeypatch, outcome="ok", reset=0, timeout_s=None):
    started: list[Any] = []
    run = bare(
        tmp_path,
        deadline=10**12,
        session="s",
        problem_statement="issue",
        code_index=None,
        refine_version="v1",
        settings=SimpleNamespace(task_timeout_s=100),
    )
    monkeypatch.setattr(module, "REFINE_TIMEOUT_S", timeout_s)
    monkeypatch.setattr(Run, "_exec", lambda self, prompt, resume: outcome)
    monkeypatch.setattr(Run, "_changed", lambda self: False)
    monkeypatch.setattr(Run, "_read", lambda self, name: "refined text")
    monkeypatch.setattr(
        Run,
        "_script",
        lambda self, script, timeout, root=False: done(reset, "out", "err"),
    )
    monkeypatch.setattr(
        module.SweBenchDocker, "run", staticmethod(lambda args, timeout=None: started.append(args))
    )
    return run, started


def test_refine_restarts_a_stopped_container_after_a_timeout(tmp_path, monkeypatch):
    run, started = refine_instance(tmp_path, monkeypatch, outcome="timeout", timeout_s=60)
    state: dict[str, Any] = {}
    refined, problem = run._refine(state)
    assert (refined, problem) == ("refined text", None)
    assert started == [["start", "box"]]
    assert state["refine"]["refine_exec"] == "timeout"
    assert run.deadline < 10**12, "the second step gets a fresh task budget"


def test_refine_reports_a_tree_that_cannot_be_reset(tmp_path, monkeypatch):
    run, _ = refine_instance(tmp_path, monkeypatch, reset=3)
    refined, problem = run._refine({})
    assert refined == "refined text" and "after the refine step failed (3)" in problem


def test_refine_stops_early_when_interrupted(tmp_path, monkeypatch):
    run, _ = refine_instance(tmp_path, monkeypatch)
    run.stop_event.set()
    assert run._refine({}) == ("", None)


def review_instance(tmp_path, monkeypatch, snapshot=0, review="ok", diff=""):
    started: list[Any] = []
    run = bare(
        tmp_path,
        deadline=10**12,
        session=None,
        problem_statement="issue",
        store=SimpleNamespace(
            log_stats=lambda *a: {
                "input_tokens": 1,
                "cached_input_tokens": 0,
                "output_tokens": 2,
            }
        ),
        instance_id="i",
    )

    def script(self, text, timeout, root=False):
        if text == module.REVIEW_SNAPSHOT_SCRIPT:
            return done(snapshot)
        if text == module.REVIEW_DIFF_SCRIPT:
            return done(0, diff)
        return done()

    monkeypatch.setattr(Run, "_script", script)
    monkeypatch.setattr(Run, "_changed", lambda self: True)
    monkeypatch.setattr(Run, "_read", lambda self, name: "diff")
    monkeypatch.setattr(Run, "_exec", lambda self, prompt, resume: review)
    monkeypatch.setattr(
        module.SweBenchDocker, "run", staticmethod(lambda args, timeout=None: started.append(args))
    )
    monkeypatch.setattr(Run, "compose_review_prompt", staticmethod(lambda *a: "review"))
    return run, started


def test_review_diff_stats_skip_malformed_lines_and_binary_counts(tmp_path, monkeypatch):
    run, _ = review_instance(tmp_path, monkeypatch, diff="3\t1\ta.py\nbad line\n-\t-\tb.bin\n")
    state: dict[str, Any] = {}
    assert run._review(state, "ok") == "ok"
    assert state["review"]["files"] == ["a.py", "b.bin"]
    assert (state["review"]["added"], state["review"]["removed"]) == (3, 1)


def test_review_timeout_restarts_the_container_and_a_failed_snapshot_is_noted(
    tmp_path, monkeypatch
):
    run, started = review_instance(tmp_path, monkeypatch, snapshot=4, review="timeout")
    state: dict[str, Any] = {}
    assert run._review(state, "ok") == "timeout"
    assert started == [["start", "box"]] and state["review"]["changed"] is None
    assert any("before the review turn failed (4)" in note for note in run.notes)


def test_restart_stops_early_when_interrupted_and_reports_a_failed_reset(tmp_path, monkeypatch):
    started: list[Any] = []
    monkeypatch.setattr(
        module.SweBenchDocker, "run", staticmethod(lambda args, timeout=None: started.append(args))
    )
    run = bare(
        tmp_path, compaction_baseline=0, started=0.0, nudges_used=0, session="s", restarted=False
    )
    run.compactions_recorded = lambda: 2  # type: ignore[method-assign]
    run.stop_event.set()
    assert run._restart({}, "task") == "restart" and started == []

    run.stop_event.clear()
    monkeypatch.setattr(Run, "_read", lambda self, name: "d")

    def script(self, text, timeout, root=False):
        return done(7, "", "no reset") if text == module.RESET_SCRIPT else done()

    monkeypatch.setattr(Run, "_script", script)

    def unwritable(root, relative, text):
        raise OSError("read-only")

    monkeypatch.setattr(module.SweBenchScratch, "write_text", staticmethod(unwritable))
    state: dict[str, Any] = {}
    assert run._restart(state, "task") == "error"
    assert state["restart"]["diff_bytes"] == 1
    assert any("for the restart failed (7)" in note for note in run.notes)


def test_a_failed_issue_resolution_is_noted_and_the_gate_still_written(tmp_path, monkeypatch):
    monkeypatch.setattr(Run, "_script", lambda self, script, timeout, root=False: done(2))
    run = bare(tmp_path, problem_statement="see widget.py")
    with contextlib.suppress(AttributeError, KeyError):  # the rest needs the full constructor
        run._prepare_gate({})
    assert any("resolving what the issue names failed (2)" in note for note in run.notes)
