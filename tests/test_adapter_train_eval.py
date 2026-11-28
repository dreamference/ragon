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


def test_adapter_train_eval_check_speed_notes_the_missing_base_drafter_row(tmp_path: Path) -> None:
    hosts = dict(HOSTS)
    del hosts["adapter_base_drafter"]
    stage = AdapterTrainEval(
        tmp_path,
        hosts,
        holdout_runner=_holdout(["a"], ["a", "b", "c"]),
        speed_runner=_speed({"http://base": 100.0, "http://adapter": 98.0}),
        completer=lambda host, prompt: prompt.upper(),
        auditor=lambda host: 0,
    )
    entry = stage.check_speed()
    assert entry["note"] == "no base-drafter server named: that row is not recorded"
    assert "adapter_base_drafter" not in entry["tokens_per_second"]


def test_adapter_train_eval_check_exactness_skips_without_a_speculation_off_host(
    tmp_path: Path,
) -> None:
    hosts = dict(HOSTS)
    del hosts["adapter_speculation_off"]
    completer: list[str] = []

    def spy(host: str, prompt: str) -> str:
        completer.append(host)
        return prompt.upper()

    stage = AdapterTrainEval(
        tmp_path,
        hosts,
        holdout_runner=_holdout(["a"], ["a", "b", "c"]),
        speed_runner=_speed({"http://base": 100.0, "http://adapter": 98.0}),
        completer=spy,
        auditor=lambda host: 0,
    )
    entry = stage.check_exactness()
    assert entry == {
        "status": "skipped",
        "passed": False,
        "note": "no speculation-off server named",
    }
    assert completer == []


def test_run_without_hosts_says_what_is_missing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    class FakeRun:
        directory = tmp_path

    assert AdapterTrainEval.run(FakeRun()) == 1
    assert "eval_hosts" in capsys.readouterr().out


def test_adapter_train_eval_run_writes_the_report_and_returns_by_passed(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    config = {
        "eval_hosts": {
            "base": "http://base",
            "adapter": "http://adapter",
            "adapter_speculation_off": "http://adapter-off",
            "adapter_base_drafter": "http://adapter-bd",
            "not_a_host_key": "http://other",
        }
    }
    (tmp_path / "train-config.json").write_text(json.dumps(config), encoding="utf-8")

    class FakeRun:
        directory = tmp_path

    seen: dict[str, Any] = {}

    def fake_init(self: Any, out_dir: Path, hosts: dict[str, str], **kw: Any) -> None:
        seen["out_dir"] = out_dir
        seen["hosts"] = hosts

    def fake_holdout_runner(out_dir: Path, config: dict[str, Any]) -> None:
        seen["runner_out_dir"] = out_dir
        seen["runner_config"] = config
        return

    monkeypatch.setattr(
        AdapterTrainEval, "generate_holdout_runner", staticmethod(fake_holdout_runner)
    )
    monkeypatch.setattr(AdapterTrainEval, "__init__", fake_init)
    monkeypatch.setattr(AdapterTrainEval, "execute", lambda self: {"passed": True})
    monkeypatch.setattr(
        AdapterTrainEval, "report_path", lambda self, name: tmp_path / "eval" / name
    )

    assert AdapterTrainEval.run(FakeRun()) == 0
    assert seen["out_dir"] == tmp_path
    assert "not_a_host_key" not in seen["hosts"]
    assert seen["runner_out_dir"] == tmp_path
    assert seen["runner_config"] == config
    assert "Eval report" in capsys.readouterr().out


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


def test_decode_speed_takes_the_median_of_the_kinds(monkeypatch: pytest.MonkeyPatch) -> None:
    from dreamference.hardware import adapter_train_eval as eval_mod

    class FakeDone:
        returncode = 0
        stdout = ""
        stderr = ""

    def fake_run(command: list[str], **_kwargs: Any) -> FakeDone:
        out = Path(command[command.index("--out") + 1])
        out.write_text(
            json.dumps({"kinds": {"short": {"decode_tps": 100.0}, "long": {"decode_tps": 90.0}}}),
            encoding="utf-8",
        )
        return FakeDone()

    monkeypatch.setattr(eval_mod.subprocess, "run", fake_run)
    assert AdapterTrainEval.decode_speed("http://srv") == 95.0


def test_holdout_runner_returns_none_without_repo(tmp_path: Path) -> None:
    assert AdapterTrainEval.generate_holdout_runner(tmp_path, {}) is None


def test_holdout_runner_calls_the_generate_stage_and_grades(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    import dreamference.hardware.adapter_train_generate as gen_mod
    import dreamference.hardware.adapter_train_tasks as tasks_mod

    holdout_tasks = [("t1", "t2")]

    def fake_collect(repo: Path) -> tuple:
        assert repo == tmp_path / "repo"
        return ([], holdout_tasks)

    class FakeGenerate:
        out_dir: ClassVar[Path | None] = None
        tasks: ClassVar = [({"id": "t1"}, "holdout")]

        def __init__(self, *args: Any, **kwargs: Any) -> None:
            type(self).out_dir = args[1]
            self.repo = args[0]
            self.host = args[4]
            self.attempts = kwargs.get("attempts", 2)

        def prepare_clone(self) -> None: ...

        def detect_all(self) -> bool:
            return True

        def load_state(self, task_id: str) -> dict[str, Any]:
            return {"detect": {"fail_to_pass": ["f"]}}

        def attempt(self, spec, grading, number) -> dict[str, Any]:
            return {"passed": True}

    monkeypatch.setattr(tasks_mod.AdapterTrainTasks, "collect", staticmethod(fake_collect))
    monkeypatch.setattr(gen_mod, "AdapterTrainGenerate", FakeGenerate)

    runner = AdapterTrainEval.generate_holdout_runner(
        tmp_path, {"repo": str(tmp_path / "repo"), "test_cmd": "pytest"}
    )
    assert runner is not None
    result = runner("http://base", "base")

    assert FakeGenerate.out_dir == tmp_path / "eval" / "base"
    assert result["resolved"] and result["attempted"]


def test_adapter_train_eval_grade_holdout_stops_when_detection_fails() -> None:
    class FailedDetect:
        attempts = 2
        tasks: ClassVar = [("task-1", "holdout")]

        def prepare_clone(self) -> None: ...

        def detect_all(self) -> bool:
            return False

    result = AdapterTrainEval.grade_holdout(FailedDetect())
    assert result == {"resolved": [], "attempted": [], "stopped": True}
