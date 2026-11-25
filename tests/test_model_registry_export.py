"""The model registry export: round trip against the committed fixture the Rust reader tests."""

import json

from dreamference.hardware.model_matrix_registry import (
    DEFAULT_MODEL_ALIAS,
    ModelMatrixRegistry,
)
from dreamference.hardware.registry_export import SCHEMA_VERSION, RegistryExport


def test_the_fixture_is_the_export():
    assert RegistryExport.fixture_path().read_text() == RegistryExport.export_json()


def test_the_export_is_deterministic_and_round_trips():
    text = RegistryExport.export_json()
    assert text == RegistryExport.export_json()
    assert json.loads(text) == json.loads(json.dumps(RegistryExport.export()))
    assert json.loads(text)["schema_version"] == SCHEMA_VERSION


def test_the_default_recipe_and_hashes_are_the_registrys():
    exported = json.loads(RegistryExport.export_json())
    model = exported["model"]
    assert exported["default_model"] == DEFAULT_MODEL_ALIAS
    for engine, recipe in model["engines"].items():
        want = ModelMatrixRegistry.get_launch_overrides(DEFAULT_MODEL_ALIAS, engine)
        assert recipe == json.loads(json.dumps(want))
    assert model["default_engine"] == ModelMatrixRegistry.engines(DEFAULT_MODEL_ALIAS)[0]
    assert model["pinned_revision"] == ModelMatrixRegistry.pinned_revision(DEFAULT_MODEL_ALIAS)
    assert model["release_sources"]
    for repo, source in model["release_sources"].items():
        assert source == ModelMatrixRegistry.get_release_source(repo)


def test_the_metal_recipes_are_the_tiers_with_the_measured_entry_context():
    metal = json.loads(RegistryExport.export_json())["model"]["metal"]
    assert set(metal) == {"64", "128"}
    entry, full = metal["64"], metal["128"]
    assert (entry["tier"], entry["sessions"], entry["max_model_len"]) == ("entry", 2, 98_304)
    assert entry["vision"] and entry["prefix_store_gb"] == 2 and entry["prefill_chunk"] == 2048
    assert entry["sparse_head_dir"] is None
    assert (full["tier"], full["sessions"], full["max_model_len"]) == ("full", 4, 262_144)
    assert full["sparse_head_dir"] == "dreamference/qwen3.8-27b-sparse-head"
    assert entry["converted_weights_sha256"] is None is full["converted_weights_sha256"]
