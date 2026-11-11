"""The shape of a log sink: what the writer thread calls (LOGGING §5.4)."""

from typing import Protocol


class LogSink(Protocol):
    """Anything that writes one line and may be closed."""

    def write(self, line: str) -> None:
        """Write one line."""

    def close(self) -> None:
        """Release the sink."""
