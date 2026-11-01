"""`server start` hands over to SGLang once when the default engine (ragon-serve) cannot start.

Only the default engine does: a flag, `DREAMFERENCE_SERVER_ENGINE` or `server_engine` in the config
is a choice, and a failure of a chosen engine is the error. The container of the failed engine is
stopped and removed before SGLang starts. Nothing here runs Docker or a model server.
"""

import pytest

from dreamference.cli import dreamference_cli_controller as controller
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController
from dreamference.hardware.model_matrix_registry import ENGINE_RAGON, ENGINE_SGLANG
from dreamference.vllm_server import DiffusionServerManager, VLLMServerManager


class _Monitor:
    """A model-loading monitor that never became ready; `fatal` says why, when it did not."""

    server_ready = False

    def __init__(self, fatal=None):
        self.fatal = fatal

    def start(self):
        pass

    def stop(self):
        pass


class _Harness:
    """Stubs the manager's launch, stop and remove, and records the order of what happened."""

    def __init__(self, monkeypatch, first_failure):
        self.events = []
        self.engines = []
        self.first_failure = first_failure
        monkeypatch.setattr(
            controller,
            "create_model_loading_monitor",
            lambda *a, **k: _Monitor("model load failed" if first_failure == "fatal" else None),
        )
        monkeypatch.setattr(VLLMServerManager, "start_server", lambda mgr, **k: self._start(**k))
        monkeypatch.setattr(
            DiffusionServerManager, "remove_leftover", classmethod(lambda cls, port=8001: None)
        )
        monkeypatch.setattr(
            VLLMServerManager, "stop_server", lambda s, port=8000: self.events.append("stop")
        )
        monkeypatch.setattr(
            VLLMServerManager, "remove_server", lambda s, port=8000: self.events.append("remove")
        )

    def _start(self, **kwargs):
        self.engines.append(kwargs.get("engine"))
        self.events.append(f"start:{kwargs.get('engine')}")
        if len(self.engines) == 1:
            if self.first_failure == "pull":
                raise RuntimeError("Docker image 'x' is missing locally and could not be obtained.")
            return  # "fatal" and "stopped": the monitor says the load did not succeed
        raise SystemExit(0)  # the second launch is the one under test; it ends the test here


def _start(argv):
    with pytest.raises(SystemExit) as raised:
        DreamferenceCLIController.run_cli(["server", "start", *argv])
    return raised.value.code


@pytest.mark.parametrize("failure", ["pull", "fatal", "stopped"])
def test_a_default_engine_that_cannot_start_is_replaced_by_sglang_with_one_warning(
    monkeypatch, capsys, failure
):
    harness = _Harness(monkeypatch, failure)
    assert _start([]) == 0
    assert harness.engines[1] == ENGINE_SGLANG
    assert harness.engines[0] in (None, ENGINE_RAGON)
    # The failed engine's container is gone before SGLang starts.
    assert harness.events.index("remove") < harness.events.index(f"start:{ENGINE_SGLANG}")
    out = capsys.readouterr().out
    warnings = [line for line in out.splitlines() if "serving with SGLang instead" in line]
    assert len(warnings) == 1 and warnings[0].startswith("⚠️")
    assert "--engine ragon" in warnings[0] and "--engine sglang" in warnings[0]
    assert "server_engine" in warnings[0]


def test_an_engine_chosen_with_the_flag_gets_the_error_and_no_fallback(monkeypatch, capsys):
    harness = _Harness(monkeypatch, "pull")
    with pytest.raises(RuntimeError, match="could not be obtained"):
        DreamferenceCLIController.run_cli(["server", "start", "--engine", "ragon"])
    assert harness.engines == [ENGINE_RAGON] and "remove" not in harness.events
    assert "serving with SGLang instead" not in capsys.readouterr().out


def test_an_engine_chosen_in_the_config_or_environment_gets_no_fallback(monkeypatch, capsys):
    monkeypatch.setenv("DREAMFERENCE_SERVER_ENGINE", "ragon")
    harness = _Harness(monkeypatch, "pull")
    with pytest.raises(RuntimeError):
        DreamferenceCLIController.run_cli(["server", "start"])
    assert harness.engines == [ENGINE_RAGON]
    assert "serving with SGLang instead" not in capsys.readouterr().out


def test_an_explicit_engine_whose_load_fails_exits_without_a_fallback(monkeypatch, capsys):
    harness = _Harness(monkeypatch, "fatal")
    assert _start(["--engine", "ragon"]) == 1
    assert harness.engines == [ENGINE_RAGON]
    assert "serving with SGLang instead" not in capsys.readouterr().out


def test_sglang_chosen_explicitly_is_never_retried(monkeypatch):
    harness = _Harness(monkeypatch, "pull")
    with pytest.raises(RuntimeError):
        DreamferenceCLIController.run_cli(["server", "start", "--engine", "sglang"])
    assert harness.engines == [ENGINE_SGLANG]


def test_the_fallback_is_tried_once(monkeypatch):
    monkeypatch.setattr(
        controller, "create_model_loading_monitor", lambda *a, **k: _Monitor("model load failed")
    )
    monkeypatch.setattr(
        DiffusionServerManager, "remove_leftover", classmethod(lambda cls, port=8001: None)
    )
    engines = []

    def always_fails(manager, **kwargs):
        engines.append(kwargs.get("engine"))

    monkeypatch.setattr(VLLMServerManager, "start_server", always_fails)
    monkeypatch.setattr(VLLMServerManager, "stop_server", lambda s, port=8000: None)
    monkeypatch.setattr(VLLMServerManager, "remove_server", lambda s, port=8000: None)
    assert _start([]) == 1
    assert len(engines) == 2 and engines[1] == ENGINE_SGLANG


def test_the_unattended_callers_name_the_engine_so_the_fallback_never_applies():
    from pathlib import Path

    script = Path(__file__).resolve().parents[1] / "scripts" / "night_local_arm.sh"
    if script.is_file():  # a release install has no scripts/
        text = script.read_text()
        assert "server start --engine" in text
        assert "server start\n" not in text and 'server start"' not in text
    # The rule itself: an explicit choice is never replaced.
    assert (
        DreamferenceCLIController._may_fall_back("qwen3.8-27b-nvfp4-dflash2", None, True) is False
    )
    assert (
        DreamferenceCLIController._may_fall_back("qwen3.8-27b-nvfp4-dflash2", ENGINE_SGLANG, False)
        is False
    )
