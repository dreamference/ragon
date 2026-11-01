"""Branch tests for the audit's pseudo-terminal session, with a fake child and no real process."""

import builtins
import json
import signal
import sys
import types

import pytest

from dreamference.audit import tui_session
from dreamference.audit.tui_session import TuiSession


def test_missing_modules_names_what_cannot_be_imported(monkeypatch):
    real_import = builtins.__import__

    def fake_import(name, *args, **kwargs):
        if name == "pyte":
            raise ImportError(name)
        return real_import(name, *args, **kwargs)

    monkeypatch.setitem(sys.modules, "pexpect", types.ModuleType("pexpect"))
    monkeypatch.setattr(builtins, "__import__", fake_import)
    assert TuiSession.missing_modules() == ["pyte"]


def test_reply_in_skips_unreadable_blank_and_foreign_events(tmp_path, monkeypatch):
    sessions = tmp_path / "sessions" / "2026"
    sessions.mkdir(parents=True)
    done = {"payload": {"type": "task_complete", "last_agent_message": "  "}}
    lines = ["not json", json.dumps([1]), json.dumps({"payload": "x"}), json.dumps(done)]
    (sessions / "a.jsonl").write_text("\n".join(lines))
    assert TuiSession.reply_in(str(tmp_path)) is None

    real_open = tui_session.pathlib.Path.open

    def failing_open(self, *args, **kwargs):
        if self.name == "a.jsonl":
            raise OSError("gone")
        return real_open(self, *args, **kwargs)

    monkeypatch.setattr(tui_session.pathlib.Path, "open", failing_open)
    assert TuiSession.reply_in(str(tmp_path)) is None


class FakeChild:
    """Stands in for a pexpect child: alive for a number of checks, records closes."""

    def __init__(self, alive_checks):
        self.alive_checks = alive_checks
        self.pid = 4242
        self.closed = False

    def isalive(self):
        if self.alive_checks > 0:
            self.alive_checks -= 1
            return True
        return False

    def close(self, force=False):
        self.closed = force


@pytest.fixture
def fast_clock(monkeypatch):
    monkeypatch.setattr(tui_session.time, "sleep", lambda s: None)


def test_stop_returns_when_the_group_is_already_gone(monkeypatch, fast_clock):
    def gone(pid, sig):
        raise ProcessLookupError

    monkeypatch.setattr(tui_session.os, "killpg", gone)
    child = FakeChild(alive_checks=1)
    TuiSession._stop(child)
    assert child.closed


def test_stop_escalates_to_kill_when_term_is_ignored(monkeypatch, fast_clock):
    sent = []
    monkeypatch.setattr(tui_session.os, "killpg", lambda pid, sig: sent.append(sig))
    clock = iter(range(0, 10_000, 5))
    monkeypatch.setattr(tui_session.time, "monotonic", lambda: next(clock))
    # alive for: the entry check, the term wait loop, then dead after SIGKILL.
    child = FakeChild(alive_checks=5)
    TuiSession._stop(child)
    assert sent[0] == signal.SIGTERM
    assert signal.SIGKILL in sent
    assert child.closed


def test_stop_leaves_a_dead_child_alone(monkeypatch):
    monkeypatch.setattr(tui_session.os, "killpg", lambda *a: pytest.fail("no kill expected"))
    child = FakeChild(alive_checks=0)
    TuiSession._stop(child)
    assert child.closed


def _install_fakes(monkeypatch, child, reads):
    class FakeTimeoutError(Exception):
        pass

    class FakeEofError(Exception):
        pass

    pending = list(reads)

    def read_nonblocking(size, timeout):
        item = pending.pop(0) if pending else FakeEofError()
        if isinstance(item, Exception):
            raise item
        return item

    child.read_nonblocking = read_nonblocking
    child.send = lambda data: child.sent.append(data)
    child.sent = []
    pexpect = types.ModuleType("pexpect")
    pexpect.TIMEOUT = FakeTimeoutError
    pexpect.EOF = FakeEofError
    pexpect.spawn = lambda *a, **k: child
    pyte = types.ModuleType("pyte")

    class Screen:
        def __init__(self, cols, rows):
            self.display = []

    class ByteStream:
        def __init__(self, screen):
            self.screen = screen

        def feed(self, data):
            self.screen.display = [data.decode()]

    pyte.Screen, pyte.ByteStream = Screen, ByteStream
    monkeypatch.setitem(sys.modules, "pexpect", pexpect)
    monkeypatch.setitem(sys.modules, "pyte", pyte)
    return FakeTimeoutError, FakeEofError


def test_run_reports_start_when_the_process_exits_before_the_composer(monkeypatch, fast_clock):
    child = FakeChild(alive_checks=0)
    _install_fakes(monkeypatch, child, [b"booting"])
    result = TuiSession.run(["ragon"], "/tmp", {}, "/tmp", "hi", timeout=5)
    assert result["stage"] == "start"
    assert result["replied"] is False
    assert child.closed


def test_run_types_the_prompt_and_quits_after_a_reply(monkeypatch, fast_clock):
    child = FakeChild(alive_checks=0)
    _install_fakes(monkeypatch, child, [b">_ Ragon \xe2\x80\xba"])
    for name in ("SETTLE_AFTER_READY_S", "TYPE_PAUSE_S", "SETTLE_AFTER_REPLY_S"):
        monkeypatch.setattr(tui_session, name, 0)
    monkeypatch.setattr(TuiSession, "reply_in", classmethod(lambda cls, home: "pong"))
    result = TuiSession.run(["ragon"], "/tmp", {}, "/tmp", "say\nhi", timeout=5)
    assert result == {"replied": True, "stage": "quit", "screen": ">_ Ragon \u203a"}
    assert child.sent == ["say hi", "\r", "/quit", "\r"]


def test_run_keeps_going_past_a_read_timeout(monkeypatch, fast_clock):
    child = FakeChild(alive_checks=0)
    timeout_exc, _ = _install_fakes(monkeypatch, child, [])
    reads = [timeout_exc(), b">_ Ragon \xe2\x80\xba"]

    def read(size, timeout):
        item = reads.pop(0) if reads else OSError()
        if isinstance(item, Exception):
            raise item
        return item

    child.read_nonblocking = read
    monkeypatch.setattr(TuiSession, "reply_in", classmethod(lambda cls, home: None))
    for name in ("SETTLE_AFTER_READY_S", "TYPE_PAUSE_S"):
        monkeypatch.setattr(tui_session, name, 0)
    result = TuiSession.run(["ragon"], "/tmp", {}, "/tmp", "hi", timeout=0)
    assert result["stage"] in ("composer", "start")
    assert result["replied"] is False
