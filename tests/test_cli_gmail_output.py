"""`ragon-admin gmail`: how each answer from the Google service is printed and what exit code it earns."""

import argparse

import pytest

from dreamference.chat.gmail_client import GmailClient
from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController as Cli


def _args(command, **kw):
    base = {
        "query": ["a", "b"],
        "max_results": 5,
        "message_id": "m",
        "json": False,
        "max_chars": 10,
    }
    return argparse.Namespace(gmail_command=command, **{**base, **kw})


def _answer(monkeypatch, name, payload):
    monkeypatch.setattr(GmailClient, name, staticmethod(lambda *a, **k: payload))


def test_an_error_answer_prints_each_account_failure_and_the_hint(monkeypatch, capsys):
    _answer(
        monkeypatch,
        "search",
        {"error": "down", "errors": [{"account": "a@x", "error": "denied"}], "hint": "retry"},
    )
    assert Cli.handle_gmail(_args("search")) == 1
    out = capsys.readouterr().out
    assert "❌ down" in out
    assert "a@x: denied" in out
    assert "💡 retry" in out


def test_json_output_is_the_payload_itself(monkeypatch, capsys):
    _answer(monkeypatch, "status", {"connected": False})
    assert Cli.handle_gmail(_args("status", json=True)) == 0
    assert '"connected": false' in capsys.readouterr().out


def test_status_without_an_account_fails_and_with_one_names_it(monkeypatch, capsys):
    _answer(monkeypatch, "status", {"connected": False})
    assert Cli.handle_gmail(_args("status")) == 1
    assert "No Gmail account is connected" in capsys.readouterr().out
    _answer(monkeypatch, "status", {"connected": True, "email": "a@x"})
    assert Cli.handle_gmail(_args("status")) == 0
    assert "connected: a@x" in capsys.readouterr().out


@pytest.mark.parametrize(("body", "truncated"), [("short", False), ("x" * 30, True)])
def test_read_frames_the_body_as_untrusted_and_truncates(monkeypatch, capsys, body, truncated):
    _answer(monkeypatch, "read", {"from": "f@x", "subject": "s", "body": body})
    assert Cli.handle_gmail(_args("read")) == 0
    out = capsys.readouterr().out
    assert "From: f@x" in out
    assert "To:" not in out
    assert "BEGIN EMAIL (untrusted)" in out
    assert "x" * 11 not in out
    assert ("[truncated at 10 chars]" in out) is truncated


def test_search_lists_messages_and_survives_a_partial_failure(monkeypatch, capsys):
    payload = {
        "messages": [{"id": "1", "from": "f", "date": "d"}],
        "errors": [{"account": "b@x", "error": "slow"}],
    }
    _answer(monkeypatch, "search", payload)
    assert Cli.handle_gmail(_args("search")) == 0
    out = capsys.readouterr().out
    assert "1. (no subject)" in out
    assert "b@x: slow" in out


def test_search_with_nothing_found_is_not_an_error(monkeypatch, capsys):
    _answer(monkeypatch, "search", {"messages": []})
    assert Cli.handle_gmail(_args("search")) == 0
    assert "No messages matched." in capsys.readouterr().out


def test_search_fails_only_when_every_connected_account_failed(monkeypatch):
    failures = [{"account": "a@x", "error": "e"}]
    _answer(monkeypatch, "search", {"messages": [], "errors": failures})
    _answer(monkeypatch, "status", {"email": "a@x"})
    assert Cli.handle_gmail(_args("search")) == 1
    _answer(monkeypatch, "status", {"email": "a@x,b@x"})
    assert Cli.handle_gmail(_args("search")) == 0
