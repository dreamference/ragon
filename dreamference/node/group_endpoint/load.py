"""The node's load signal for the group endpoint (SINGLE_ENDPOINT §2.2).

Pure functions: the caller reads the engine, ``run.json``, the GPU lock and the
clock, and hands the facts over as one mapping; nothing here touches the machine.
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Final

RUN_HEARTBEAT_STALE_S: Final[float] = 180.0
NIGHT: Final[str] = "night"
STATES: Final[tuple[str, ...]] = ("ok", "loading", "unhealthy")
PASSTHROUGH_LISTS: Final[tuple[str, ...]] = ("models", "adapters", "sidecars")


def _hold(state: Mapping[str, Any], now: float) -> dict[str, Any] | None:
    """Pick the hold: operator drain, a live run, the GPU lock, the night window."""
    drain = state.get("operator_drain")
    if drain:
        until = drain.get("until") if isinstance(drain, Mapping) else None
        return {"kind": "operator", "until": until}
    run = state.get("run")
    if isinstance(run, Mapping):
        beat = run.get("heartbeat")
        if isinstance(beat, (int, float)) and now - beat <= RUN_HEARTBEAT_STALE_S:
            kind = NIGHT if run.get("kind") == NIGHT else "bench"
            return {"kind": kind, "until": run.get("until")}
    if state.get("gpu_lock_held"):
        return {"kind": "gpu-lock", "until": state.get("gpu_lock_until")}
    night = state.get("night_window")
    if night:
        return {
            "kind": "night",
            "until": night.get("until") if isinstance(night, Mapping) else None,
        }
    return None


def load_snapshot(state: Mapping[str, Any]) -> dict[str, Any]:
    """Build the ``/ragon/v1/load`` body from the facts in ``state``.

    Args:
        state: engine_health (``ok``/``loading``/``unhealthy``), sessions_total,
            sessions_busy, queue, lru_age_s, mem_available_mib, run (run.json
            contents with ``heartbeat`` and ``kind``), gpu_lock_held,
            night_window, operator_drain, now, and the capability fields.

    Returns:
        The fields of spec section 2.2.
    """
    now = float(state.get("now", 0.0))
    health = state.get("engine_health", "unhealthy")
    snap: dict[str, Any] = {
        "state": health if health in STATES else "unhealthy",
        "sessions_total": int(state.get("sessions_total", 0)),
        "sessions_busy": int(state.get("sessions_busy", 0)),
        "queue": int(state.get("queue", 0)),
        "lru_age_s": state.get("lru_age_s"),
        "mem_available_mib": state.get("mem_available_mib"),
        "hold": _hold(state, now),
    }
    for key in PASSTHROUGH_LISTS:
        snap[key] = list(state.get(key) or [])
    for key in ("engine", "platform", "max_context"):
        snap[key] = state.get(key)
    return snap


def load_header(snapshot: Mapping[str, Any]) -> str:
    """Render the one-line ``X-Ragon-Load`` value (``key=value`` pairs, ``;``-separated)."""
    hold = snapshot.get("hold")
    hold_text = (
        "none"
        if not hold
        else f"{hold['kind']}@{'' if hold.get('until') is None else hold['until']}"
    )

    def cell(value: object) -> str:
        return "-" if value is None else str(value)

    parts = [
        ("state", snapshot["state"]),
        ("sessions", f"{snapshot['sessions_busy']}/{snapshot['sessions_total']}"),
        ("queue", snapshot["queue"]),
        ("lru", cell(snapshot.get("lru_age_s"))),
        ("mem", cell(snapshot.get("mem_available_mib"))),
        ("hold", hold_text),
    ]
    return ";".join(f"{k}={cell(v)}" for k, v in parts)
