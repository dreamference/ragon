"""What one task's `ragon exec --json` log says: tokens, commands and how long their outputs were.

The output lengths are what Ragon Digest could cut: a `command_execution` item's `aggregated_output`
is the command's whole output, digest on or off; `from_rollouts` counts what the model read. Lines
are counted as `str.splitlines` does; a final line without a newline counts.
"""

import json
import re
from collections.abc import Iterable
from pathlib import Path
from typing import Any, Final

# Output lengths reported separately: the digest's low trigger (60) and its default (200).
LONG_THRESHOLDS: Final[tuple[int, ...]] = (60, 200)

# The `ragon exec --json` event and item types counted.
ITEM_COMPLETED: Final[str] = "item.completed"
TURN_COMPLETED: Final[str] = "turn.completed"
COMMAND_ITEM: Final[str] = "command_execution"
TOOL_CALL_ITEM: Final[str] = "mcp_tool_call"

TOKEN_KEYS: Final[tuple[str, ...]] = ("input_tokens", "cached_input_tokens", "output_tokens")

# What the model was given: the tool outputs of the session's rollouts under `CODEX_HOME`.
ROLLOUT_GLOB: Final[str] = "rollout-*.jsonl"
SEEN_OUTPUT_TYPES: Final[tuple[str, ...]] = ("function_call_output", "custom_tool_call_output")
TOKEN_COUNT: Final[str] = "token_count"
DIGEST_HEADER: Final[re.Pattern[str]] = re.compile(r"^\[digest\] \d+ lines", re.MULTILINE)


def empty() -> dict[str, Any]:
    """The counts of a log with no event."""
    stats: dict[str, Any] = dict.fromkeys(TOKEN_KEYS, 0)
    stats.update(turns=0, commands=0, tool_calls=0, output_lines=0, output_bytes=0, max_lines=0)
    for threshold in LONG_THRESHOLDS:
        stats[f"outputs_over_{threshold}"] = 0
        stats[f"bytes_over_{threshold}"] = 0
    return stats


def add_output(stats: dict[str, Any], output: str) -> None:
    """Counts one command's output."""
    lines = len(output.splitlines())
    size = len(output.encode())
    stats["output_lines"] += lines
    stats["output_bytes"] += size
    stats["max_lines"] = max(stats["max_lines"], lines)
    for threshold in LONG_THRESHOLDS:
        if lines > threshold:
            stats[f"outputs_over_{threshold}"] += 1
            stats[f"bytes_over_{threshold}"] += size


def add_event(stats: dict[str, Any], event: dict[str, Any]) -> None:
    """Counts one event: a finished command, a finished tool call or a finished turn."""
    kind = event.get("type")
    item = event.get("item") or {}
    if kind == ITEM_COMPLETED and item.get("type") == COMMAND_ITEM:
        stats["commands"] += 1
        add_output(stats, str(item.get("aggregated_output") or ""))
    elif kind == ITEM_COMPLETED and item.get("type") == TOOL_CALL_ITEM:
        stats["tool_calls"] += 1
    elif kind == TURN_COMPLETED:
        stats["turns"] += 1
        usage = event.get("usage") or {}
        for key in TOKEN_KEYS:
            stats[key] += int(usage.get(key) or 0)


def from_lines(lines: Iterable[str]) -> dict[str, Any]:
    """The counts of a log's lines; lines that are not JSON objects are skipped."""
    stats = empty()
    for line in lines:
        try:
            event = json.loads(line)
        except ValueError:
            continue  # e.g. "Reading additional input from stdin..."
        if isinstance(event, dict):
            add_event(stats, event)
    return stats


def from_log(path: Path) -> dict[str, Any]:
    """The counts of a log file; the counts of an empty log when it cannot be read."""
    try:
        text = path.read_bytes().decode(errors="replace")
    except OSError:
        return empty()
    return from_lines(text.splitlines())


def seen_empty() -> dict[str, Any]:
    """The counts of rollouts with no tool output."""
    stats: dict[str, Any] = {"seen_outputs": 0, "seen_lines": 0, "seen_bytes": 0, "digests": 0}
    stats.update({f"rollout_{key}": 0 for key in TOKEN_KEYS})
    return stats


def merge_rollout_tokens(stats: dict[str, Any]) -> None:
    """Raises each token count to the rollouts' own total.

    A trial killed at its time limit never finishes its turn, so the event log has no
    `turn.completed` and counts 0; the rollout's last `token_count` event has the usage so far.
    """
    for key in TOKEN_KEYS:
        stats[key] = max(int(stats[key]), int(stats.get(f"rollout_{key}", 0)))


def from_rollouts(codex_home: Path) -> dict[str, Any]:
    """What the model was given: tool outputs and how many of them were digests.

    The event log's `aggregated_output` is the command's whole output, digest or not; the rollout
    holds the text the model read, so the two together give what Ragon Digest cut.

    Args:
        codex_home: The run's `CODEX_HOME` (searched recursively for rollout files).

    Returns:
        Dict[str, Any]: `seen_outputs`, `seen_lines`, `seen_bytes` and `digests`.
    """
    stats = seen_empty()
    for path in sorted(codex_home.rglob(ROLLOUT_GLOB)) if codex_home.is_dir() else []:
        last_usage: dict[str, Any] = {}
        for line in path.read_bytes().decode(errors="replace").splitlines():
            try:
                record = json.loads(line)
            except ValueError:
                continue
            payload = record.get("payload") if isinstance(record, dict) else None
            if isinstance(payload, dict) and payload.get("type") == TOKEN_COUNT:
                info = payload.get("info")
                total = info.get("total_token_usage") if isinstance(info, dict) else None
                last_usage = total if isinstance(total, dict) else last_usage
            if not isinstance(payload, dict) or payload.get("type") not in SEEN_OUTPUT_TYPES:
                continue
            output = payload.get("output")
            text = output if isinstance(output, str) else json.dumps(output)
            stats["seen_outputs"] += 1
            stats["seen_lines"] += len(text.splitlines())
            stats["seen_bytes"] += len(text.encode())
            stats["digests"] += 1 if DIGEST_HEADER.search(text) else 0
        for key in TOKEN_KEYS:
            stats[f"rollout_{key}"] += int(last_usage.get(key) or 0)
    return stats
