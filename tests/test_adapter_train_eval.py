"""The adapter train eval stage: E1 to E5 with every server and agent call mocked."""

import json
from pathlib import Path
from typing import Any, ClassVar

import pytest

from dreamference.hardware.adapter_train_eval import AdapterTrainEval

HOSTS = {
    "base": "http://base",
    "adapter": "http://adapter",
    "adapter_speculation_off": "http://adapter-off",
    "adapter_base_drafter": "http://adapter-bd",
}


def _holdout(base: list[str], adapter: list[str]) -> Any:
    def runner(host: str, label: str) -> dict[str, Any]:
        return {"resolved": base if label == "base" else adapter, "attempted": ["a", "b", "c", "d"]}

    return runner


def _speed(table: dict[str, float]) -> Any:
    return lambda host: table[host]


def _stage(tmp_path: Path, **kw: Any) -> AdapterTrainEval:
    args: dict[str, Any] = {
        "holdout_runner": _holdout(["a"], ["a", "b", "c"]),
        "speed_runner": _speed(
            {"http://base": 100.0, "http://adapter": 98.0, "http://adapter-bd": 90.0}
        ),
        "completer": lambda host, prompt: prompt.upper(),
        "auditor": lambda host: 0,
    }
    args.update(kw)
    return AdapterTrainEval(tmp_path, dict(HOSTS), **args)


def test_all_checks_pass_and_one_report_is_written(tmp_path: Path) -> None:
    report = _stage(tmp_path).execute()
    assert report["passed"] and report["make_default"]
    assert report["E1"]["difference"] == 2 and report["E1"]["gain"]
    assert report["E2"]["status"] == "skipped" and report["E2"]["note"]
    assert report["E3"]["tokens_per_second"]["adapter_base_drafter"] == 90.0
    written = json.loads((tmp_path / "eval" / "report.json").read_text())
    assert written["E5"]["passed"]
    assert "Adapter eval" in (tmp_path / "eval" / "report.md").read_text()


def test_speed_warns_above_five_percent_and_refuses_nothing(tmp_path: Path) -> None:
    report = _stage(tmp_path).execute()
    assert report["E3"]["loss_pct"]["adapter"] == 2.0
    assert report["E3"]["loss_pct"]["adapter_base_drafter"] == 10.0
    assert any("adapter_base_drafter" in w and "10.0%" in w for w in report["warnings"])
    assert not any("E3: adapter decodes" in w for w in report["warnings"])
    assert report["passed"]


def test_no_gain_means_not_the_default(tmp_path: Path) -> None:
    report = _stage(tmp_path, holdout_runner=_holdout(["a"], ["a", "b"])).execute()
    assert not report["E1"]["gain"] and not report["make_default"]
    assert any(w.startswith("E1: no gain") for w in report["warnings"])


def test_control_slice_loses_at_most_one(tmp_path: Path) -> None:
    def control(host: str) -> dict[str, Any]:
        done = ["x", "y", "z"] if host == "http://base" else ["x"]
        return {"resolved": done, "attempted": ["x", "y", "z"]}

    report = _stage(tmp_path, control_runner=control).execute()
    assert report["E2"]["lost"] == ["y", "z"] and report["E2"]["regressed"]
    assert not report["make_default"]


def test_exactness_failure_fails_the_stage(tmp_path: Path) -> None:
    replies = {"http://adapter": "same", "http://adapter-off": "other"}
    report = _stage(tmp_path, completer=lambda host, prompt: replies[host]).execute()
    assert not report["E4"]["passed"] and len(report["E4"]["differing"]) == 4
    assert not report["passed"]


def test_egress_failure_fails_the_stage_and_names_the_adapter_host(tmp_path: Path) -> None:
    seen: list[str] = []

    def audit(host: str) -> int:
        seen.append(host)
        return 1

    report = _stage(tmp_path, auditor=audit).execute()
    assert seen == ["http://adapter"] and not report["passed"]


def test_missing_hosts_are_refused(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="adapter"):
        AdapterTrainEval(tmp_path, {"base": "http://base"})


def test_run_without_hosts_says_what_is_missing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    class FakeRun:
        directory = tmp_path

    assert AdapterTrainEval.run(FakeRun()) == 1
    assert "eval_hosts" in capsys.readouterr().out


def test_grade_holdout_resolves_a_task_when_any_attempt_passes() -> None:
    class FakeGenerate:
        attempts = 2
        tasks: ClassVar = [
            ({"id": "t1"}, "holdout"),
            ({"id": "t2"}, "holdout"),
            ({"id": "t3"}, "train"),
        ]

        def prepare_clone(self) -> None: ...

        def detect_all(self) -> bool:
            return True

        def load_state(self, task_id: str) -> dict[str, Any]:
            return {"detect": {"fail_to_pass": [] if task_id == "t2" else ["f"]}}

        def attempt(
            self, spec: dict[str, Any], grading: dict[str, Any], number: int
        ) -> dict[str, Any]:
            return {"passed": number == 2}

    result = AdapterTrainEval.grade_holdout(FakeGenerate())
    assert result == {"resolved": ["t1"], "attempted": ["t1"]}


def test_grade_holdout_stops_when_the_window_closes() -> None:
    class Closed:
        attempts = 1
        tasks: ClassVar = [({"id": "t1"}, "holdout")]

        def prepare_clone(self) -> None: ...

        def detect_all(self) -> bool:
            return True

        def load_state(self, task_id: str) -> dict[str, Any]:
            return {"detect": {"fail_to_pass": ["f"]}}

        def attempt(self, *args: Any) -> None:
            return None

    assert AdapterTrainEval.grade_holdout(Closed())["stopped"]


def test_eval_complete_returns_chat_content(monkeypatch: pytest.MonkeyPatch) -> None:
    import io

    from dreamference.hardware import adapter_train_eval as eval_mod

    calls: list[str] = []

    class Reply(io.BytesIO):
        pass

    def opener(url, timeout):
        if isinstance(url, str):
            calls.append(url)
            return Reply(json.dumps({"data": [{"id": "m1"}]}).encode())
        calls.append(str(getattr(url, "full_url", url)))
        return Reply(json.dumps({"choices": [{"message": {"content": "hello"}}]}).encode())

    monkeypatch.setattr(eval_mod.urllib.request, "urlopen", opener)
    result = AdapterTrainEval.complete("http://srv", "say hi")
    assert result == "hello"
    assert calls == ["http://srv/v1/models", "http://srv/v1/chat/completions"]


def test_eval_complete_returns_empty_string_when_content_is_none(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    import io

    from dreamference.hardware import adapter_train_eval as eval_mod

    class Reply(io.BytesIO):
        pass

    def opener(url, timeout):
        if isinstance(url, str):
            return Reply(json.dumps({"data": [{"id": "m1"}]}).encode())
        return Reply(json.dumps({"choices": [{"message": {"content": None}}]}).encode())

    monkeypatch.setattr(eval_mod.urllib.request, "urlopen", opener)
    assert AdapterTrainEval.complete("http://srv", "x") == ""
