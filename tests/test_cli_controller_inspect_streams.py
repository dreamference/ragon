"""Tests for the streaming and acceptance helpers of the CLI controller's deep inspection."""

import json
from unittest.mock import MagicMock

from dreamference.cli import dreamference_cli_controller as ctl
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as C


def _resp(*chunks):
    lines = []
    for c in chunks:
        lines.append(c if isinstance(c, bytes) else b"data: " + json.dumps(c).encode())
    r = MagicMock()
    r.iter_lines.return_value = lines
    return r


def test_read_sample_stream_collects_content_reasoning_and_usage():
    r = _resp(
        b"",
        b"event: ping",
        b"data: not json",
        {"choices": [{"delta": {"content": "hi", "reasoning_content": "think"}}]},
        {"choices": [{"delta": {"reasoning": " more"}}], "usage": {"completion_tokens": 7}},
        b"data: [DONE]",
    )
    assert C._inspect_read_sample_stream(r) == ("hi", "think more", 7)


def test_acceptance_formats_rate_and_tau():
    before = {"drafts": 0, "drafted": 0, "accepted": 0}
    after = {"drafts": 10, "drafted": 30, "accepted": 15}
    assert C._inspect_acceptance(before, after) == "50.0% / 2.50"
    assert C._inspect_acceptance(before, before) == "N/A"
    assert C._inspect_acceptance(None, after) == "N/A"


def test_read_stream_records_ttft_and_decode_rate(monkeypatch):
    times = iter([10.0, 12.0])
    monkeypatch.setattr(ctl.time, "time", lambda: next(times))
    r = _resp(
        b"",
        b"junk",
        b"data: {bad",
        {"usage": {"completion_tokens": 11}},
    )
    stream = {"supports": False, "ttft": 0.0, "decode_tps": 0.0}
    # time() calls: ttft (first data line), then elapsed.
    C._inspect_read_stream(r, 9.0, stream)
    assert stream["supports"] is True
    assert stream["ttft"] == 1.0
    assert stream["decode_tps"] == 5.0


def test_read_sample_row_builds_cells(monkeypatch):
    from dreamference.cli.model_deep_inspector import ModelDeepInspector

    monkeypatch.setattr(ModelDeepInspector, "_spec_counters", staticmethod(lambda h: None))
    monkeypatch.setattr(ctl.time, "time", lambda: 12.0)
    r = _resp(
        {
            "choices": [{"delta": {"content": "x\ny", "reasoning": "r"}}],
            "usage": {"completion_tokens": 4},
        }
    )
    row = C._inspect_read_sample_row(r, "Tell me about Richard Feynman", 10.0, None, "http://h")
    assert row[1] == "[green]OK[/green]"
    assert "completions/s" in row[3]
    assert "<think>r</think>" in row[5]
    empty = C._inspect_read_sample_row(_resp(), "plain", 10.0, None, "http://h")
    assert empty[5] == "Empty response" and empty[3] == "N/A"


def _status(monkeypatch, capsys, vllm, summary, installed):
    import dreamference.context_engine as ce

    hw = {
        "is_gb10": installed,
        "machine": "m" if installed else "",
        "os_name": "os" if installed else "",
        "gpu_name": "g",
        "driver_version": "1",
        "total_unified_memory_gb": 128,
        "used_memory_gb": 1,
        "available_memory_gb": 127,
        "arch": "aarch64",
    }
    monkeypatch.setattr(ctl, "detect_gb10_hardware", lambda: hw)
    server = MagicMock()
    server.get_server_status.return_value = vllm
    monkeypatch.setattr(ctl, "VLLMServerManager", lambda host: server)
    engine = MagicMock()
    engine.load_index.return_value = summary is not None
    engine.get_summary.return_value = summary
    monkeypatch.setattr(ce, "ContextEngine", lambda: engine)
    for name, method in (
        ("ClineInstaller", "is_cline_extension_installed"),
        ("ContinueInstaller", "is_continue_extension_installed"),
        ("OpenHandsInstaller", "is_image_downloaded"),
        ("CodexInstaller", "is_installed"),
    ):
        monkeypatch.setattr(getattr(ctl, name), method, classmethod(lambda cls: installed))
    C.handle_status()
    return capsys.readouterr().out


def test_status_reports_online_with_an_index(monkeypatch, capsys):
    summary = {
        "total_indexed_files": 3,
        "total_ast_symbols": 9,
        "index_file": "i.json",
        "sqlite_file": "c.db",
    }
    out = _status(monkeypatch, capsys, {"healthy": True, "models": []}, summary, True)
    assert "Online" in out and "SQLite Context Store" in out and "Qualified" in out


def test_status_reports_starting_and_offline_without_an_index(monkeypatch, capsys):
    out = _status(
        monkeypatch, capsys, {"healthy": False, "loading_status": "x", "models": []}, None, False
    )
    assert "Starting" in out and "non-GB10" in out and "Context Engine" not in out
    out = _status(monkeypatch, capsys, {"healthy": False, "models": []}, None, False)
    assert "Offline" in out


def _drift(monkeypatch, expected, running):
    monkeypatch.setattr(
        ctl.VLLMServerManager, "build_launch_command", lambda self, **kw: list(expected)
    )
    return C._detect_recipe_drift("m", 8000, list(running))


def test_recipe_drift_reports_matching_and_differing_flags(monkeypatch):
    from dreamference.hardware import ModelMatrixRegistry

    monkeypatch.setattr(ModelMatrixRegistry, "engines", classmethod(lambda cls, m: []))
    expected = ["docker", "img", "serve", "m", "--a=1", "--b", "2", "--c"]
    same = _drift(monkeypatch, expected, ["serve", "m", "--a=1", "--b", "2", "--c"])
    assert list(same) == ["Recipe Match"]
    rows = _drift(monkeypatch, expected, ["serve", "m", "--a=2", "--d", "x"])
    assert rows["  drift --a"] == "running=2  recipe=1"
    assert rows["  drift --b"] == "running=absent  recipe=2"
    assert rows["  drift --c"] == "running=absent  recipe=(set)"
    assert rows["  drift --d"] == "running=x  recipe=absent"


def test_recipe_drift_gives_up_without_an_entry_point_or_on_error(monkeypatch):
    from dreamference.hardware import ModelMatrixRegistry

    monkeypatch.setattr(ModelMatrixRegistry, "engines", classmethod(lambda cls, m: []))
    assert _drift(monkeypatch, ["docker", "img", "--a"], ["--a"]) == {}

    def boom(self, **kw):
        raise RuntimeError("x")

    monkeypatch.setattr(ctl.VLLMServerManager, "build_launch_command", boom)
    assert C._detect_recipe_drift("m", 8000, ["serve"]) == {}


def test_recipe_drift_picks_the_running_engine(monkeypatch):
    from dreamference.hardware import ModelMatrixRegistry
    from dreamference.vllm_server.ragon_serve_launch_builder import RAGON_SERVE_PROGRAM
    from dreamference.vllm_server.sglang_launch_builder import SGLANG_ENTRY_POINT

    seen = []
    monkeypatch.setattr(
        ModelMatrixRegistry, "engines", classmethod(lambda cls, m: ["ragon", "sglang"])
    )

    def build(self, **kw):
        seen.append(kw["engine"])
        return ["x", "serve", "--a"]

    monkeypatch.setattr(ctl.VLLMServerManager, "build_launch_command", build)
    C._detect_recipe_drift("m", 1, [RAGON_SERVE_PROGRAM, "--a"])
    C._detect_recipe_drift("m", 1, [SGLANG_ENTRY_POINT, "--a"])
    assert seen[0] != seen[1] and None not in seen


def _insp(stdout, returncode=0):
    return MagicMock(returncode=returncode, stdout=stdout, stderr="")


def _deep(monkeypatch, run, profile):
    import subprocess

    from dreamference.cli.model_deep_inspector import ModelDeepInspector as D

    monkeypatch.setattr(subprocess, "run", run)
    rows = {"k": "v"}
    for name in ("quantization_map", "kv_geometry", "sampling_provenance", "graph_coverage"):
        monkeypatch.setattr(D, name, staticmethod(lambda *a, **k: rows))
    monkeypatch.setattr(D, "profile_acceptance_by_workload", staticmethod(lambda *a, **k: profile))
    out = MagicMock()
    cfg = MagicMock(model="m")
    C._render_deep_inspection(out, cfg, "http://h:9000", "http://h:9000/v1", "m")
    return "\n".join(str(c.args[0]) for c in out.print.call_args_list)


def test_deep_inspection_renders_the_profile_table_and_running_n(monkeypatch):
    cmd = [
        "serve",
        "--kv-cache-dtype",
        "fp8",
        "--speculative-config",
        '{"num_speculative_tokens": 3}',
    ]
    data = json.dumps([{"Config": {"Cmd": cmd}}])
    profile = [
        {"category": "code", "acceptance": 50.0, "tau": 2.0, "slots": [0.9, 0.1], "suggested_n": 4},
        {"category": "chat", "acceptance": 20.0, "tau": 1.2, "slots": [], "suggested_n": 2},
    ]
    text = _deep(monkeypatch, lambda *a, **k: _insp(data), profile)
    assert "Running n=3" in text and "2-4" in text and "Quantization Map" in text


def test_deep_inspection_tolerates_a_missing_container_and_empty_profile(monkeypatch):
    import subprocess

    def boom(*a, **k):
        raise subprocess.SubprocessError("x")

    assert "No speculative counters" in _deep(monkeypatch, boom, [])
    assert "No speculative counters" in _deep(monkeypatch, lambda *a, **k: _insp("", 1), [])
    profile = [{"category": "c", "acceptance": 1.0, "tau": 1.0, "slots": [], "suggested_n": 1}]
    bad = json.dumps([{"Config": {"Cmd": ["--speculative-config", "{bad"]}}])
    text = _deep(monkeypatch, lambda *a, **k: _insp(bad), profile)
    assert "Running n=" not in text
    assert "No speculative counters" not in _deep(monkeypatch, lambda *a, **k: _insp("[]"), profile)
