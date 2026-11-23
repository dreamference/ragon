"""`server start` outcomes in the CLI controller: ready, canary results, fatal, stopped, Ctrl-C."""

import contextlib
import itertools
from unittest.mock import MagicMock

import requests

from dreamference.cli import dreamference_cli_controller as ctl
from dreamference.node import NodeAdvertiser
from dreamference.vllm_server import VLLMServerManager

C = ctl.DreamferenceCLIController


class _Monitor:
    server_ready = False
    fatal = None

    def __init__(self, ready=False, fatal=None):
        self.server_ready, self.fatal = ready, fatal

    def start(self):
        pass

    def stop(self):
        pass

    def get_status(self):
        return {"stages_reached": [], "elapsed_seconds": 1.5}

    def print_progress(self):
        pass


def _start(monkeypatch, monitor, *, start_server=None, warm=1.0, post=None, extra=()):
    monkeypatch.setattr(ctl, "create_model_loading_monitor", lambda *a, **k: monitor)
    monkeypatch.setattr(VLLMServerManager, "start_server", start_server or (lambda self, **k: None))
    monkeypatch.setattr(VLLMServerManager, "warm_up", lambda self: warm)
    from dreamference.vllm_server import DiffusionServerManager

    monkeypatch.setattr(
        DiffusionServerManager, "remove_leftover", classmethod(lambda cls, port=8001: None)
    )
    stopped = []
    monkeypatch.setattr(VLLMServerManager, "stop_server", lambda self, port=None: stopped.append(1))
    monkeypatch.setattr(NodeAdvertiser, "on_server_ready", classmethod(lambda cls: None))
    monkeypatch.setattr(NodeAdvertiser, "on_server_stopped", classmethod(lambda cls: None))
    monkeypatch.setattr(C, "_run_tool_call_canary", classmethod(lambda cls, *a: None))
    suggested = []
    monkeypatch.setattr(
        C, "_suggest_engine_fallback", classmethod(lambda cls, *a: suggested.append(a))
    )
    monkeypatch.setattr(
        requests,
        "post",
        post
        or (lambda *a, **k: MagicMock(status_code=200, json=lambda: {"choices": [{"text": "hi"}]})),
    )
    argv = ["server", "start", "--model", "qwen3.8-27b-nvfp4-dflash2", *extra]
    with contextlib.suppress(SystemExit):
        C.run_cli(argv)
    return None, stopped, suggested


def _resp(status, text=None):
    return MagicMock(status_code=status, json=lambda: {"choices": [{"text": text}]})


def test_dreamference_cli_controller_after_server_ready_reports_a_ready_diffusion_sidecar(
    monkeypatch, capsys
):
    """The ready path reports a diffusion sidecar that answers /health as 'ok'."""
    import time as time_mod

    from dreamference.hardware import ModelMatrixRegistry
    from dreamference.vllm_server import DiffusionServerManager

    sidecar = MagicMock()
    sidecar.check_health.return_value = True
    sidecar.get_load_state.return_value = "ok"
    sidecar.host = "http://localhost:8001"
    monkeypatch.setattr(
        requests,
        "get",
        lambda *a, **k: MagicMock(status_code=200, json=lambda: {"status": "ok"}),
    )
    monkeypatch.setattr(ModelMatrixRegistry, "diffusion_enabled", classmethod(lambda cls: True))
    monkeypatch.setattr(
        DiffusionServerManager,
        "start_server",
        lambda self, **k: True,
    )
    ticks = itertools.count(1.0, 1.0)
    monkeypatch.setattr(time_mod, "time", lambda: next(ticks))
    slept = []
    monkeypatch.setattr(time_mod, "sleep", lambda s: slept.append(s))

    _start(monkeypatch, _Monitor(ready=True))

    out = capsys.readouterr().out
    assert "Diffusion model ready at http://localhost:8001/v1" in out
    assert not slept


def test_ready_server_runs_a_passing_canary(monkeypatch, capsys):
    _start(monkeypatch, _Monitor(ready=True))
    assert "Canary Passed" in capsys.readouterr().out


def test_ready_server_with_failed_warm_up_and_corrupt_canary(monkeypatch, capsys):
    _, _, suggested = _start(
        monkeypatch, _Monitor(ready=True), warm=None, post=lambda *a, **k: _resp(200, "!!!")
    )
    out = capsys.readouterr().out
    assert "Warm-up request failed" in out and "Output corrupted" in out and suggested


def test_canary_skipped_on_http_error_and_on_exception(monkeypatch, capsys):
    _, _, suggested = _start(monkeypatch, _Monitor(ready=True), post=lambda *a, **k: _resp(500))
    assert "skipped" in capsys.readouterr().out and suggested

    def boom(*a, **k):
        raise requests.ConnectionError("down")

    _, _, suggested = _start(monkeypatch, _Monitor(ready=True), post=boom)
    assert "failed to execute" in capsys.readouterr().out and suggested


def test_fatal_monitor_stops_the_server(monkeypatch, capsys):
    monkeypatch.setattr(C, "_exit_unless_fall_back", classmethod(lambda cls, *a: None))
    _, stopped, _ = _start(monkeypatch, _Monitor(fatal="out of memory"))
    assert stopped and "out of memory" in capsys.readouterr().out


def test_stopped_container_is_reported(monkeypatch, capsys):
    monkeypatch.setattr(C, "_exit_unless_fall_back", classmethod(lambda cls, *a: None))
    _start(monkeypatch, _Monitor())
    assert "stopped before it answered" in capsys.readouterr().out


def test_fallback_after_fatal_hands_over_to_sglang(monkeypatch):
    handed = []
    monkeypatch.setattr(C, "_exit_unless_fall_back", classmethod(lambda cls, *a: "why"))
    monkeypatch.setattr(C, "_fall_back_to_sglang", classmethod(lambda cls, *a: handed.append(a)))
    _start(monkeypatch, _Monitor())
    assert handed


def test_runtime_error_before_the_container_goes_through_fall_back(monkeypatch):
    def refuse(self, **k):
        raise RuntimeError("no docker")

    seen = []
    monkeypatch.setattr(C, "_raise_unless_fall_back", classmethod(lambda cls, *a: seen.append(a)))
    _start(monkeypatch, _Monitor(), start_server=refuse)
    assert seen


def test_ctrl_c_terminates_the_process(monkeypatch, capsys):
    import subprocess

    def interrupt(self, **k):
        self.process = MagicMock()
        self.process.wait.side_effect = subprocess.TimeoutExpired("d", 5)
        raise KeyboardInterrupt

    _start(monkeypatch, _Monitor(), start_server=interrupt)
    assert "Shutting down" in capsys.readouterr().out


def test_dreamference_cli_controller_start_diffusion_sidecar_nulls_the_manager_on_failure(
    monkeypatch, capsys
):
    """A failing or raising diffusion start_server leaves no manager for the health poll."""
    import time as time_mod

    from dreamference.hardware import ModelMatrixRegistry
    from dreamference.vllm_server import DiffusionServerManager

    monkeypatch.setattr(ModelMatrixRegistry, "diffusion_enabled", classmethod(lambda cls: True))
    ticks = itertools.count(1.0, 1.0)
    monkeypatch.setattr(time_mod, "time", lambda: next(ticks))
    slept = []
    monkeypatch.setattr(time_mod, "sleep", lambda s: slept.append(s))

    # A False return (the docker failure the manager already reported) drops the handle,
    # so the ready path polls nothing: no sidecar line, no sleeping.
    monkeypatch.setattr(DiffusionServerManager, "start_server", lambda self, **k: False)
    _start(monkeypatch, _Monitor(ready=True))
    out = capsys.readouterr().out
    assert "Diffusion model ready" not in out and not slept

    # An exception while starting is caught, named, and the handle is dropped the same way.
    monkeypatch.setattr(
        DiffusionServerManager, "remove_leftover", classmethod(lambda cls, port=8001: None)
    )

    def start_sidecar_error(self, **k):
        raise RuntimeError("no docker")

    monkeypatch.setattr(DiffusionServerManager, "start_server", start_sidecar_error)
    _start(monkeypatch, _Monitor(ready=True))
    out = capsys.readouterr().out
    assert "Diffusion sidecar failed to start: no docker" in out and not slept
