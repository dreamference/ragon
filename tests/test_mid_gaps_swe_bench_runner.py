"""Coverage for the SWE-bench runner's lanes across paired nodes and its interrupt handler."""

from __future__ import annotations

from types import SimpleNamespace

import pytest

from dreamference.node.node_lanes import NodeLanes
from dreamference.node.node_pairing import NodePairing
from dreamference.swe_bench.swe_bench_runner import SweBenchRunner


def settings():
    return SimpleNamespace(nodes=["second"], max_parallel=4, task_context=1000)


def test_a_run_with_no_paired_node_has_only_this_machines_lane(monkeypatch):
    monkeypatch.setattr(NodePairing, "paired", classmethod(lambda cls: False))
    lanes, notes = SweBenchRunner.lanes("http://h:1", ("m", 10), settings(), 2)
    assert [lane["name"] for lane in lanes] == ["this machine"] and notes == []


def test_a_paired_nodes_lane_follows_this_machines_with_a_note_and_the_skipped_reasons(
    monkeypatch,
):
    monkeypatch.setattr(NodePairing, "paired", classmethod(lambda cls: True))
    monkeypatch.setattr(NodeLanes, "wanted", classmethod(lambda cls, nodes: ["second"]))
    seen = {}

    def build(cls, host, served, nodes, hostops, budget):
        seen["budget"] = budget
        return (
            [
                {"name": "this machine", "host": host, "parallel": 9},
                {"name": "second", "host": "http://n:1", "parallel": 3},
            ],
            ["third skipped"],
        )

    monkeypatch.setattr(NodeLanes, "lanes", classmethod(build))
    monkeypatch.setattr(
        SweBenchRunner, "host", SimpleNamespace(parallelism=lambda cap, pool, context: cap + pool)
    )
    lanes, notes = SweBenchRunner.lanes("http://h:1", ("m", 10), settings(), 2)
    assert lanes[0]["parallel"] == 2
    assert "second's model server (http://n:1): up to 3" in notes[0]
    assert notes[1] == "third skipped"
    assert seen["budget"](5)[1] is None


def test_the_interrupt_handler_raises_a_keyboard_interrupt():
    with pytest.raises(KeyboardInterrupt):
        SweBenchRunner._raise_interrupt()


def test_a_run_without_the_dataset_or_a_built_ragon_stops_before_anything_starts(
    monkeypatch, capsys
):
    from dreamference.swe_bench.swe_bench_harness import SweBenchHarness
    from dreamference.swe_bench.swe_bench_runtime import SweBenchRuntime

    monkeypatch.setattr(SweBenchRunner, "smoke_passed", classmethod(lambda cls: True))
    monkeypatch.setattr(SweBenchHarness, "rows", classmethod(lambda cls, dataset: []))
    assert SweBenchRunner.run() == 1
    assert "dataset is not downloaded" in capsys.readouterr().out
    monkeypatch.setattr(SweBenchHarness, "rows", classmethod(lambda cls, dataset: [{"a": 1}]))
    monkeypatch.setattr(SweBenchRuntime, "installed_ragon", classmethod(lambda cls: None))
    assert SweBenchRunner.run() == 1
    assert "ragon is not built" in capsys.readouterr().out


def test_a_custom_prompt_with_no_file_has_no_digest_and_a_built_in_one_has_none_either():
    assert SweBenchRunner.prompt_digest("no-such-custom-prompt") is None


def test_a_command_with_empty_output_has_no_output(monkeypatch):
    import subprocess

    monkeypatch.setattr(
        subprocess, "run", lambda command, **kw: SimpleNamespace(stdout="  \n", stderr="")
    )
    assert SweBenchRunner._output(["x"]) is None
