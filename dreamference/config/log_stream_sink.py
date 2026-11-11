# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""The stream sink (LOGGING §3.1, §5.1): stderr under journald (with the syslog priority) or Docker."""

import sys
from typing import Final, TextIO

PRIORITY: Final[tuple[str, ...]] = ("<3>", "<4>", "<6>", "<7>", "<7>")


class LogStreamSink:
    """Writes each line to a stream, prefixed with `<n>` under journald."""

    def __init__(self, stream: TextIO | None = None, journald: bool = False) -> None:
        """Hold the stream.

        Args:
            stream: The stream; `sys.stderr` read at each write when None.
            journald: Prefix each line with the syslog priority of its level.
        """
        self.stream = stream
        self.journald = journald

    def write(self, line: str) -> None:
        """Write one line; the level is read back from the line's third field.

        Args:
            line: The line without its newline.
        """
        prefix = ""
        if self.journald:
            fields = line.split(" ", 3)
            level = (
                ("ERROR", "WARN", "INFO", "DEBUG", "TRACE").index(fields[2])
                if len(fields) > 2 and fields[2] in ("ERROR", "WARN", "INFO", "DEBUG", "TRACE")
                else 2
            )
            prefix = PRIORITY[level]
        stream = self.stream or sys.stderr
        stream.write(prefix + line + "\n")
        stream.flush()

    def close(self) -> None:
        """Nothing to close: the stream is the process's."""
