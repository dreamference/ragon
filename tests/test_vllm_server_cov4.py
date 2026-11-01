"""Branch coverage for the tool-call canary's stream handling and the loading monitor's status."""

import json

import requests

from dreamference.vllm_server.model_loading_monitor import ModelLoadingMonitor
from dreamference.vllm_server.tool_call_canary import ToolCallCanary


def _data(event):
    return "data: " + json.dumps(event)


def test_read_events_ends_on_done_and_skips_noise():
    lines = [
        b"",
        "event: x",
        "data: not json",
        "data: [1, 2]",
        _data({"type": "a"}),
        "data: [DONE]",
    ]
    events, finished = ToolCallCanary.read_events(lines, 10.0, clock=lambda: 0.0)
    assert events == [{"type": "a"}]
    assert finished is True


def test_read_events_ends_on_a_final_event_and_decodes_bytes():
    lines = [_data({"type": "response.failed"}).encode(), _data({"type": "later"})]
    events, finished = ToolCallCanary.read_events(lines, 10.0, clock=lambda: 0.0)
    assert [e["type"] for e in events] == ["response.failed"]
    assert finished is True


def test_read_events_without_a_final_event_is_not_finished():
    events, finished = ToolCallCanary.read_events([_data({"type": "a"})], 10.0, clock=lambda: 0.0)
    assert len(events) == 1
    assert finished is False


def test_read_events_stops_when_the_deadline_has_passed():
    events, finished = ToolCallCanary.read_events([_data({"type": "a"})], 1.0, clock=lambda: 5.0)
    assert events == []
    assert finished is False


class _Broken:
    status_code = 200

    def iter_lines(self):
        raise requests.ConnectionError("reset")
        yield b""  # pragma: no cover


def test_a_stream_that_breaks_off_is_reported_and_the_response_closed():
    closed = []
    response = _Broken()
    response.close = lambda: closed.append(True)  # type: ignore[attr-defined]
    ok, line = ToolCallCanary.run("http://h", "m", post=lambda *a, **k: response)
    assert ok is False
    assert "stream broke off" in line
    assert closed == [True]


class _NoClose:
    status_code = 200

    def iter_lines(self):
        return iter([])


def test_a_response_without_close_and_without_an_end_is_a_timeout_line():
    ok, line = ToolCallCanary.run("http://h", "m", timeout=3, post=lambda *a, **k: _NoClose())
    assert ok is False
    assert "no complete answer within 3 s" in line


def test_the_monitor_status_lists_stages_and_progress_prints_them(capsys):
    monitor = ModelLoadingMonitor(list, lambda: False)
    assert monitor.get_status()["elapsed_seconds"] == 0
    monitor.start_time = 1.0
    monitor.stages_reached = {"loading", "mystery"}
    status = monitor.get_status()
    assert status["stages_reached"] == ["loading", "mystery"]
    assert status["server_ready"] is False
    monitor.print_progress()
    out = capsys.readouterr().out
    assert "Loading" in out and "loading" in out and "mystery" in out
    monitor.server_ready = True
    monitor.print_progress()
    assert "READY" in capsys.readouterr().out
