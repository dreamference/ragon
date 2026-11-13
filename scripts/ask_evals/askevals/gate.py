"""The suite's hold on the model gate for its night slot (spec §6, §9.5).

While the hold lasts, the gate refuses every request but read-only probes, so a phone question
gets the bridge's "busy with a benchmark" answer instead of competing with the suite. The suite
itself reaches the engine on its own loopback port (`127.0.0.1:18000` behind the gate on 8000,
SWE_BENCH §18.1), so the record names a network no client is in (TEST-NET-1, RFC 5737). The
record is the one a SWE-bench run writes (`ModelGate.write_run`), refreshed every 15 s; a hold
whose process dies goes stale after the gate's `STALE_S` and the gate opens by itself.
"""

from __future__ import annotations

import contextlib
import os
import secrets
import threading
import time
from typing import Final, Protocol

HEARTBEAT_S: Final[float] = 15.0
NO_CLIENT_NETWORK: Final[str] = "192.0.2.0/24"


class _GateLike(Protocol):
    """What `GateHold` asks of the gate: refresh and remove the run's record."""

    def write_run(self, record: dict) -> None: ...

    def clear_run(self, hold_id: str) -> None: ...


class GateHold:
    """Closes the gate for a `with` block; `gate` is the `ModelGate` class (a seam for tests)."""

    def __init__(
        self,
        label: str,
        total: int,
        gate: _GateLike | None = None,
        networks: list[str] | None = None,
    ) -> None:
        """Remembers the hold's shape; the gate is the real `ModelGate` unless a seam takes it."""
        if gate is None:
            from dreamference.vllm_server.model_gate import ModelGate

            gate = ModelGate
        self.gate = gate
        self.label = label
        # The suite's own runs come in on loopback past the gate; a runner whose agents sit in a
        # container (Polyglot, Terminal-Bench) names that container network's subnet instead.
        self.networks = networks or [NO_CLIENT_NETWORK]
        self.total = total
        self.done = 0
        self.id = secrets.token_hex(8)
        self.started = time.time()
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._beat, name="ask-evals-gate", daemon=True)

    def record(self) -> dict:
        """Builds the run record the gate writes, with a live done/eta count."""
        elapsed = time.time() - self.started
        eta = int(elapsed / self.done * (self.total - self.done)) if self.done else None
        return {
            "run": "ask-evals",
            "id": self.id,
            "label": self.label,
            "networks": self.networks,
            "heartbeat": time.time(),
            "pid": os.getpid(),
            "started": self.started,
            "done": self.done,
            "total": self.total,
            "eta_s": eta,
        }

    def _beat(self) -> None:
        """Keeps the gate's record fresh until the hold is released."""
        while not self._stop.wait(HEARTBEAT_S):
            with contextlib.suppress(Exception):  # a heartbeat never takes the suite down
                self.gate.write_run(self.record())

    def advance(self) -> None:
        """Counts one finished case toward the eta."""
        self.done += 1

    def __enter__(self) -> GateHold:
        """Writes the first record and starts the heartbeat thread."""
        self.gate.write_run(self.record())
        self._thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        """Stops the heartbeat and opens the gate by clearing this hold's record."""
        self._stop.set()
        self._thread.join(HEARTBEAT_S + 5)
        self.gate.clear_run(self.id)
