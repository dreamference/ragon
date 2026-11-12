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
        assert set(point) == keys
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
