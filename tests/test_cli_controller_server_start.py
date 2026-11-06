"""`server start` outcomes in the CLI controller: ready, canary results, fatal, stopped, Ctrl-C."""

import contextlib
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
