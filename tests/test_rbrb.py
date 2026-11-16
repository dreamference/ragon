"""The RBRB data file, its chart and the generator script."""

import importlib.util
import json
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parent.parent


def _load() -> ModuleType:
    spec = importlib.util.spec_from_file_location("rbrb_script", ROOT / "scripts" / "rbrb.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _data() -> dict:
    return json.loads((ROOT / "docs" / "rbrb.json").read_text(encoding="utf-8"))


def test_schema() -> None:
    data = _data()
    assert set(data) == {"name", "unit", "weights", "method", "points", "quality"}
    assert data["weights"] == {"output": 1.0, "input": 0.25, "cache_read": 0.025}
    assert set(data["method"]) >= {"0", "1"}
    assert data["points"]
    keys = {
        "week", "from", "to", "ragon_generated_tokens", "other_generated_tokens",
        "rbrb_share", "notes", "method_version",
    }  # fmt: skip
    for point in data["points"]:
        assert keys <= set(point) <= keys | {"ragon_tasks", "commits", "partial_note"}
        assert isinstance(point["ragon_generated_tokens"], int)
        assert isinstance(point["other_generated_tokens"], int)


def test_share_matches_counts() -> None:
    for point in _data()["points"]:
        total = point["ragon_generated_tokens"] + point["other_generated_tokens"]
        assert abs(point["rbrb_share"] - point["ragon_generated_tokens"] / total) < 5e-4


def test_svg_is_regenerated_byte_identical() -> None:
    module = _load()
    assert (ROOT / "images" / "rbrb.svg").read_text(encoding="utf-8") == module.render(_data())


def test_check_flag() -> None:
    module = _load()
    assert module.main(["--check"]) == 0


def test_chart_has_two_lines_and_deterministic_badge() -> None:
    module = _load()
    data = _data()
    data["quality"] = [
        {
            "week": "2026-W46",
            "ragon_reverted": 0,
            "ragon_commits": 6,
            "other_reverted": 5,
            "other_commits": 402,
        },
        {
            "week": "2026-W47",
            "ragon_reverted": 1,
            "ragon_commits": 10,
            "other_reverted": 4,
            "other_commits": 300,
        },
    ]
    svg = module.render(data)
    assert svg == module.render(data)
    assert svg.count("<polyline") == 2  # one point has no line; the band has two
    assert "Quality band" in svg
    assert module.revert_percent(data["quality"][1], "ragon") == 10.0


def test_badge_is_regenerated_byte_identical() -> None:
    module = _load()
    badge = (ROOT / "images" / "rbrb-badge.svg").read_text(encoding="utf-8")
    assert badge == module.render_badge(_data())
    assert f"{module.share_percent(_data()['points'][-1]):.2f} %" in badge


def test_ingest_upserts_the_week_and_is_idempotent(tmp_path: Path) -> None:
    module = _load()
    data = _data()
    export = {
        "week": "2026-W47",
        "ragon_generated": 100,
        "other_generated": 900,
        "ragon_tasks_attempted": 3,
        "ragon_tasks_landed": 2,
    }
    counts = {"ragon_commits": 4, "other_commits": 6, "ragon_reverted": 1, "other_reverted": 0}
    once = module.ingest(json.loads(json.dumps(data)), export, counts)
    twice = module.ingest(json.loads(json.dumps(once)), export, counts)
    assert once == twice
    assert [p["week"] for p in once["points"]] == ["2026-W46", "2026-W47"]
    new = once["points"][-1]
    assert (new["from"], new["to"], new["rbrb_share"]) == ("2026-11-16", "2026-11-22", 0.1)
    assert new["commits"] == {"ragon": 4, "all": 10}
    assert once["quality"][0]["ragon_reverted"] == 1
    assert once["quality"][0]["band"] == "undecided"
    assert "partial_note" in once["points"][0]
    readme = module.update_readme("a\n<!-- rbrb:latest -->\nold\n<!-- /rbrb:latest -->\nb\n", once)
    assert "Latest value: **10.00 %** (2026-W47)." in readme and "old" not in readme
    module.render(once)  # the chart still renders with the new fields


def test_git_counts_reads_this_repository() -> None:
    module = _load()
    counts = module.git_counts("2026-W46")
    assert set(counts) == {"ragon_commits", "other_commits", "ragon_reverted", "other_reverted"}
    assert counts["other_commits"] >= 0


def test_closed_week_is_the_week_before_today() -> None:
    import datetime

    module = _load()
    # Monday 00:30 UTC of week 47 must publish week 46; any day of week 47 does
    assert module.closed_week(datetime.date(2026, 11, 16)) == "2026-W46"
    assert module.closed_week(datetime.date(2026, 11, 22)) == "2026-W46"
    assert module.closed_week(datetime.date(2026, 11, 23)) == "2026-W47"
    assert module.closed_week(datetime.date(2027, 1, 4)) == "2026-W53"  # across the year boundary
