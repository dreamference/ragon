"""The probes `NodeLanes.lanes` asks of a model server (`NightShiftHost` or a test's stand-in)."""

from typing import Protocol


class LaneProbes(Protocol):
    """What `NodeLanes.lanes` asks of a model server."""

    def served_model(self, host: str) -> tuple[str, int] | None:
        """Asks the server what it serves."""
        ...

    def metrics(self, host: str) -> dict[str, float] | None:
        """Reads the server's metrics."""
        ...
