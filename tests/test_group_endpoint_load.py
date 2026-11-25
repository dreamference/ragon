"""The group endpoint's load snapshot and header (SINGLE_ENDPOINT §2.2)."""

from dreamference.node.group_endpoint.load import load_header, load_snapshot

BASE = {
    "engine_health": "ok",
    "sessions_total": 12,
    "sessions_busy": 3,
    "queue": 1,
    "lru_age_s": 40.5,
    "mem_available_mib": 20000,
    "now": 1000.0,
    "models": ["m"],
    "engine": "ragon-serve",
    "platform": "gb10",
    "max_context": 65536,
}


def test_fields_present():
    snap = load_snapshot(BASE)
    assert snap["state"] == "ok" and snap["hold"] is None
    assert snap["sessions_total"] == 12 and snap["models"] == ["m"]
    assert snap["adapters"] == [] and snap["sidecars"] == []


def test_unknown_health_is_unhealthy():
    assert load_snapshot({**BASE, "engine_health": "weird"})["state"] == "unhealthy"


def test_hold_follows_run_json():
    live = {**BASE, "run": {"heartbeat": 900.0, "kind": "bench", "until": 2000}}
    assert load_snapshot(live)["hold"] == {"kind": "bench", "until": 2000}
    stale = {**BASE, "run": {"heartbeat": 800.0, "kind": "bench"}}
    assert load_snapshot(stale)["hold"] is None


def test_hold_gpu_lock_night_operator_precedence():
    assert load_snapshot({**BASE, "gpu_lock_held": True})["hold"]["kind"] == "gpu-lock"
    both = {**BASE, "night_window": {"until": 5}}
    assert load_snapshot(both)["hold"] == {"kind": "night", "until": 5}
    op = {**both, "gpu_lock_held": True, "operator_drain": {"until": 9}}
    assert load_snapshot(op)["hold"] == {"kind": "operator", "until": 9}


def test_header_is_one_line():
    h = load_header(load_snapshot({**BASE, "gpu_lock_held": True}))
    assert "\n" not in h
    assert h == "state=ok;sessions=3/12;queue=1;lru=40.5;mem=20000;hold=gpu-lock@"
    assert load_header(load_snapshot(BASE)).endswith("hold=none")
