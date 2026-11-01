"""Night Shift gap branches: the meminfo readers and refine mode's piece parser and version lookup."""

import io

import pytest

from dreamference.night_shift import night_shift_host
from dreamference.night_shift.night_shift_host import NightShiftHost
from dreamference.night_shift.refine_prompt import VERSIONS, RefinePrompt


class FakeProc:
    """Stands in for `Path("/proc/meminfo")` with fixed contents."""

    def __init__(self, text: str):
        self.text = text

    def __call__(self, name: str) -> "FakeProc":
        assert name == "/proc/meminfo"
        return self

    def open(self) -> io.StringIO:
        return io.StringIO(self.text)


MEMINFO = "MemTotal:       1000 kB\nMemFree: 5 kB\nMemAvailable:    250 kB\n"


def test_meminfo_values_are_read_in_bytes(monkeypatch):
    monkeypatch.setattr(night_shift_host, "Path", FakeProc(MEMINFO))
    assert NightShiftHost.mem_total_bytes() == 1000 * 1024
    assert NightShiftHost.mem_available_bytes() == 250 * 1024


def test_meminfo_without_the_field_reads_zero(monkeypatch):
    monkeypatch.setattr(night_shift_host, "Path", FakeProc("SwapTotal: 1 kB\n"))
    assert NightShiftHost.mem_total_bytes() == 0
    assert NightShiftHost.mem_available_bytes() == 0


def test_parse_pieces_splits_on_markers_and_drops_the_end_marker():
    text = (
        "ignored preamble\n<!-- a-one -->\nline 1\nline 2\n<!-- b -->\nonly\n<!-- end -->\ntrailing"
    )
    assert RefinePrompt.parse_pieces(text) == {"a-one": "line 1\nline 2", "b": "only"}


def test_parse_pieces_of_text_without_markers_is_empty():
    assert RefinePrompt.parse_pieces("no markers here\nat all") == {}


def test_pieces_of_a_known_version_and_an_unknown_one():
    assert RefinePrompt.pieces_of("v1") is VERSIONS["v1"]
    with pytest.raises(ValueError, match="unknown refine version 'v9'"):
        RefinePrompt.pieces_of("v9")
