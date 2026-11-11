# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""The writer's own rate limits (LOGGING §6.2): per message, per process, and what always passes."""

from dataclasses import dataclass, field
from typing import Final

PER_MESSAGE: Final[int] = 10
PER_PROCESS_INFO: Final[int] = 50
PER_PROCESS_DEBUG: Final[int] = 500
CLOSING_BUDGET: Final[int] = 200
SUMMARY_EVERY_S: Final[int] = 10
CLOSING_MESSAGES: Final[frozenset[str]] = frozenset({"request done", "request refused"})


@dataclass
class LogSampler:
    """Decides, per record, whether a line is written, and what summary lines a second leaves.

    It works on the record's own second, so it needs no clock and tests are deterministic.
    """

    verbose: bool = False
    second: int = -1
    keys: dict[tuple[int, str], int] = field(default_factory=dict)
    errors_seen: set[str] = field(default_factory=set)
    total: int = 0
    closing: int = 0
    dropped: int = 0
    lost: dict[tuple[int, str], int] = field(default_factory=dict)
    pending_requests: int = 0
    pending_errors: int = 0
    pending_ms: list[float] = field(default_factory=list)
    summary_since: int = -1

    def roll(self, second: int) -> list[tuple[int, str, str, dict[str, float | int]]]:
        """Close finished seconds and return the notes they leave.

        Args:
            second: The current second (whole epoch seconds).

        Returns:
            Notes as `(level, message, site, numbers)`; level 1 is WARN, 2 INFO.
        """
        notes: list[tuple[int, str, str, dict[str, float | int]]] = []
        if second <= self.second:
            return notes
        if self.second >= 0:
            for (_level, message), count in self.lost.items():
                notes.append((1, "log lines suppressed", message, {"count": count}))
            if self.dropped:
                notes.append((1, "log lines dropped", "", {"count": self.dropped}))
            if self.pending_requests and (
                self.summary_since < 0 or second - self.summary_since >= SUMMARY_EVERY_S
            ):
                ms = sorted(self.pending_ms)
                numbers: dict[str, float | int] = {
                    "count": self.pending_requests,
                    "errors": self.pending_errors,
                }
                if ms:
                    numbers["p50_ms"] = ms[len(ms) // 2]
                    numbers["p95_ms"] = ms[min(len(ms) - 1, int(len(ms) * 0.95))]
                notes.append((2, "requests summary", "", numbers))
                self.pending_requests = self.pending_errors = 0
                self.pending_ms = []
                self.summary_since = second
        self.second = second
        self.keys.clear()
        self.errors_seen.clear()
        self.lost.clear()
        self.total = self.closing = self.dropped = 0
        return notes

    def admit(self, level: int, message: str, ms: float | None = None) -> bool:
        """Return whether this record is written; count it otherwise.

        Args:
            level: 0 error .. 4 trace.
            message: The literal message.
            ms: A closing line's duration, for the summary.
        """
        if message in CLOSING_MESSAGES:
            if self.closing < CLOSING_BUDGET:
                self.closing += 1
                return True
            self.pending_requests += 1
            self.pending_errors += 1 if message == "request refused" else 0
            if ms is not None:
                self.pending_ms.append(ms)
            return False
        key = (level, message)
        seen = self.keys.get(key, 0) + 1
        self.keys[key] = seen
        if level == 0 and message not in self.errors_seen:
            self.errors_seen.add(message)
            self.total += 1
            return True
        if seen > PER_MESSAGE:
            self.lost[key] = self.lost.get(key, 0) + 1
            return False
        cap = PER_PROCESS_DEBUG if self.verbose else PER_PROCESS_INFO
        if self.total >= cap:
            self.dropped += 1
            return False
        self.total += 1
        return True
