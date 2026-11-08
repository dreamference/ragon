"""The correctness canary of `ragon-admin benchmark`.

The canary judges whether a served model still answers correctly (arithmetic, code that parses,
a real tool call), since a wrong kernel on SM12x emits garbage rather than raising. No test reaches
Docker or a server.
"""

import pytest

from dreamference.cli.dreamference_cli_controller import DreamferenceCLIController


class Reply:
    def __init__(self, status_code, message=None):
        self.status_code = status_code
        self.message = message

    def json(self):
        return {"choices": [{"message": self.message}]}


class Session:
    """Answers the three probes in order: arithmetic, code, tool call."""

    def __init__(self, *replies):
        self.replies = list(replies)
        self.payloads = []

    def post(self, url, json=None, headers=None, timeout=None):
        self.payloads.append(json)
        reply = self.replies.pop(0)
        if isinstance(reply, Exception):
            raise reply
        return reply


def canary(*replies):
    session = Session(*replies)
    rows = DreamferenceCLIController._run_correctness_canary(
        session, "http://localhost:8000/v1/chat/completions", "served/model", {}
    )
    return session, rows


TOOL_CALL = {
    "tool_calls": [{"function": {"name": "get_weather", "arguments": '{"city": "Paris"}'}}]
}


def test_a_healthy_model_passes_all_three_probes():
    session, rows = canary(
        Reply(200, {"content": "391"}),
        Reply(200, {"content": "```python\ndef add(a, b):\n    return a + b\n```"}),
        Reply(200, TOOL_CALL),
    )
    assert all(row.startswith("[green]pass") for row in rows.values()), rows
    assert "get_weather" in rows["Canary: tool call"]
    # Thinking off for every probe, and only the last one offers a tool.
    assert [p["chat_template_kwargs"] for p in session.payloads] == [{"enable_thinking": False}] * 3
    assert ["tools" in p for p in session.payloads] == [False, False, True]
    assert session.payloads[2]["tool_choice"] == "auto"


def test_garbage_fails_each_probe_for_its_own_reason():
    _, rows = canary(
        Reply(200, {"content": "!!!!!!!!"}),
        Reply(200, {"content": "def add(a, b) return a +"}),
        Reply(200, {"content": "I would call get_weather for Paris."}),
    )
    assert rows["Canary: arithmetic"].startswith("[red]FAIL[/red] — expected 391, got '!!!!!!!!'")
    assert rows["Canary: code"].startswith("[red]FAIL[/red] — output does not parse")
    assert "check --tool-call-parser" in rows["Canary: tool call"]


def test_a_missing_content_is_a_failure_not_a_crash():
    _, rows = canary(Reply(200, {"content": None}), Reply(200, {"content": None}), Reply(200, {}))
    assert rows["Canary: arithmetic"].startswith("[red]FAIL")
    assert rows["Canary: tool call"].startswith("[red]FAIL")


@pytest.mark.parametrize("failure", [Reply(503), OSError("refused")], ids=["http-error", "raises"])
def test_an_unreachable_server_is_reported_unreachable_not_failed(failure):
    _, rows = canary(failure, failure, failure)
    assert set(rows.values()) == {"[red]unreachable[/red]"}
