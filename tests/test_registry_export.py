"""The Metal recipe export: round trip against the committed fixture the Rust reader tests."""

import json

from dreamference.hardware.metal_recipes import MetalRecipes
from dreamference.hardware.model_matrix_registry import ModelMatrixRegistry


def test_the_fixture_is_the_export():
    assert MetalRecipes.fixture_path().read_text() == MetalRecipes.export_json()


def test_the_export_round_trips_through_json():
    exported = MetalRecipes.export()
    assert json.loads(MetalRecipes.export_json()) == exported
    assert exported["model"] in ModelMatrixRegistry.MATRIX
    assert exported["weights_sha256"] is None


def test_recipes_by_memory_size():
    assert MetalRecipes.recipe_for(16) is None
    assert MetalRecipes.recipe_for(64)["max_context"] == 131072
    gate = MetalRecipes.recipe_for(128)
    assert (gate["sessions"], gate["batch"], gate["max_context"]) == (4, 4, 262144)
    assert gate["memory_gb"] == MetalRecipes.export()["gate_memory_gb"]
    assert all(r["sparse_head_dir"] for r in MetalRecipes.export()["recipes"])
