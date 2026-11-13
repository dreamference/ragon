"""Reads the agent's side of a test: the rollouts of the threads the suite's questions opened.

A rollout (`$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*.jsonl`) is one JSON record per line. Only
rollouts written since the run began are opened, and a rollout whose first user message does not
carry the run's marker is closed again at that line, so the owner's other threads are never read
past their first question.
"""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Final

# Rollout record and payload types and roles the reader looks at.
RESPONSE_ITEM: Final = "response_item"
EVENT_MSG: Final = "event_msg"
MESSAGE: Final = "message"
USER: Final = "user"
ASSISTANT: Final = "assistant"
FUNCTION_CALL: Final = "function_call"
CUSTOM_TOOL_CALL: Final = "custom_tool_call"
TURN_ABORTED: Final = "turn_aborted"
TASK_COMPLETE: Final = "task_complete"
# A user message as it appears in a raw line once spaces are removed.
USER_ROLE: Final = '"role":"user"'


@dataclass
class Session:
    """What a test needs from one thread's rollout."""

    path: Path
    first_user: str = ""
    users: list[str] = field(default_factory=list)
    answers: list[str] = field(default_factory=list)
    commands: list[str] = field(default_factory=list)
    tools: list[str] = field(default_factory=list)
    aborted: int = 0
    completed: int = 0


def user_text(payload: dict[str, Any]) -> str | None:
    """The text of a `response_item` user message, or None for anything else."""
    return message_text(payload, USER)


def message_text(payload: dict[str, Any], role: str) -> str | None:
    """The text of a `response_item` message of `role`, or None for anything else."""
    if payload.get("type") != MESSAGE or payload.get("role") != role:
        return None
    parts = payload.get("content") or []
    return "".join(part.get("text", "") for part in parts if isinstance(part, dict))


def call_command(payload: dict[str, Any]) -> tuple[str, str] | None:
    """A tool call's name and the command (or input) it ran, or None for anything else."""
    kind = payload.get("type")
    if kind == FUNCTION_CALL:
        arguments = payload.get("arguments") or ""
        try:
            value = json.loads(arguments)
        except (ValueError, TypeError):
            value = None
        if isinstance(value, dict):
            cmd = value.get("cmd") or value.get("command")
            if isinstance(cmd, list):
                return payload.get("name", ""), " ".join(map(str, cmd))
            if isinstance(cmd, str):
                return payload.get("name", ""), cmd
        return payload.get("name", ""), arguments
    if kind == CUSTOM_TOOL_CALL:
        return payload.get("name", ""), payload.get("input") or ""
    return None


def take_item(session: Session, payload: dict[str, Any], marker: str) -> bool:
    """Files one `response_item` payload into `session`.

    False when it is the first real user message and lacks `marker`.
    """
    text = user_text(payload)
    if text is not None and not text.lstrip().startswith("<"):
        if not session.first_user:
            if marker not in text:
                return False
            session.first_user = text
        session.users.append(text)
        return True
    text = message_text(payload, ASSISTANT)
    if text is not None:
        session.answers.append(text)
        return True
    call = call_command(payload)
    if call is not None:
        session.tools.append(call[0])
        session.commands.append(call[1])
    return True


def take_event(session: Session, payload: dict[str, Any]) -> None:
    """Counts an `event_msg` payload's aborted and completed turns into `session`."""
    if payload.get("type") == TURN_ABORTED:
        session.aborted += 1
    elif payload.get("type") == TASK_COMPLETE:
        session.completed += 1


def read_session(lines: list[str], marker: str, path: Path) -> Session | None:
    """Parses one rollout's lines; None when its first user message does not carry `marker`.

    The first user message is the first one that is not an environment or instructions preamble
    (those start with `<`).
    """
    session = Session(path=path)
    for line in lines:
        try:
            record = json.loads(line)
        except ValueError:
            continue
        payload = record.get("payload") or {}
        kind = record.get("type")
        if kind == RESPONSE_ITEM:
            if not take_item(session, payload, marker):
                return None
        elif kind == EVENT_MSG:
            take_event(session, payload)
    return session if session.first_user else None


def head_lines(path: Path, marker: str) -> list[str]:
    """A rollout's lines, read up to the user message that shows its first real question lacks `marker`."""
    with path.open(errors="replace") as handle:
        lines = []
        for line in handle:
            lines.append(line)
            if USER_ROLE in line.replace(" ", "") and not read_partial_has_marker(lines, marker):
                break
    return lines


def recent_sessions(root: Path, since: float, marker: str) -> list[Session]:
    """The rollouts under `root` written since `since` whose first question carries `marker`."""
    found = []
    for path in sorted(root.rglob("rollout-*.jsonl")):
        try:
            if path.stat().st_mtime < since:
                continue
            lines = head_lines(path, marker)
        except OSError:
            continue
        session = read_session(lines, marker, path)
        if session is not None:
            found.append(session)
    return found


def read_partial_has_marker(lines: list[str], marker: str) -> bool:
    """Says whether reading should go on: the first real user message so far carries `marker` (or none yet)."""
    session = read_session(lines, marker, Path())
    if session is not None:
        return True
    # No real user message yet (only a preamble) also means "go on".
    for line in lines:
        try:
            payload = json.loads(line).get("payload") or {}
        except ValueError:
            continue
        text = user_text(payload)
        if text is not None and not text.lstrip().startswith("<"):
            return False
    return True
