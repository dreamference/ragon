"""The default model's registry entry as deterministic JSON, for the Rust reader.

A node with no Python (Windows) reads its launch recipe and the release assets' SHA-256 hashes from
this export, which is committed as ``ragon-rs/vectors/model-registry.json`` and published as a
release asset. Field names are the registry's own (``ModelSpec`` and the recipe keys).
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from dreamference.hardware.memory_tier import TIER_ENTRY, MemoryTier, classify_memory
from dreamference.hardware.model_matrix_registry import (
    DEFAULT_MODEL_ALIAS,
    MODELS_RELEASE_TAG,
    ModelMatrixRegistry,
)

SCHEMA_VERSION = 1

# Metal (Mac) recipes, one per memory class. The entry numbers are the ones measured on a 64 GB
# Mac (entry-03 probe): with the drafter, vision and the 2 GB prefix store, two windows of 98,304
# tokens keep 16 GiB for the OS; 2 x 131,072 only fits with vision off.
METAL_ENTRY_MAX_CONTEXT = 98_304
METAL_PREFILL_CHUNK = 2048  # the measured prefill chunk
SPARSE_HEAD_REPO = "dreamference/qwen3.8-27b-sparse-head"
SPARSE_HEAD_DRAFTER = "sparse-head"
# Mac classification readings (GiB): a 64 GB and a 128 GB machine as hw.memsize reports them.
_METAL_MEMORY_GIB = {"64": 64.0, "128": 128.0}


def _metal_recipe(tier: MemoryTier) -> dict[str, Any]:
    """One Metal recipe from the shared tier classification (entry context from the probe)."""
    recipe = tier.recipe
    if recipe is None:
        raise ValueError(f"no node recipe for tier {tier.tier}")
    entry = tier.tier == TIER_ENTRY
    return {
        "tier": tier.tier,
        "memory_class": tier.memory_class,
        "sessions": recipe.sessions,
        "max_model_len": METAL_ENTRY_MAX_CONTEXT if entry else recipe.max_model_len,
        "prefill_chunk": METAL_PREFILL_CHUNK,
        "vision": recipe.vision,
        "drafter": recipe.drafter,
        "prefix_store_gb": recipe.prefix_store_gb,
        "gpu_cap_gb": recipe.gpu_cap_gb,
        "os_floor_gb": recipe.os_floor_gb,
        "sparse_head_dir": SPARSE_HEAD_REPO if recipe.drafter == SPARSE_HEAD_DRAFTER else None,
        # SHA-256 of the converted (Metal) weights; null until the conversion is published.
        "converted_weights_sha256": None,
    }


def _metal_recipes() -> dict[str, dict[str, Any]]:
    return {
        key: _metal_recipe(classify_memory(gib, platform="mac"))
        for key, gib in _METAL_MEMORY_GIB.items()
    }


def _release_repos(recipes: dict[str, dict[str, Any]]) -> list[str]:
    """The repositories the recipes name that are hosted as release assets, sorted."""
    repos: set[str] = set()
    for recipe in recipes.values():
        drafter = (recipe.get("speculative_config") or {}).get("model")
        for repo in (drafter, *(a.get("repo") for a in recipe.get("engine_assets") or [])):
            if repo and ModelMatrixRegistry.get_release_source(repo):
                repos.add(str(repo))
    return sorted(repos)


class RegistryExport:
    """The default model's registry entry and its JSON export."""

    @classmethod
    def export(cls, model_key: str = DEFAULT_MODEL_ALIAS) -> dict[str, Any]:
        """The export the Rust reader consumes.

        Returns:
            dict: ``schema_version``, the release ``models_release_tag``, ``default_model`` and its
                ``model`` entry: the ModelSpec fields, ``pinned_revision``, ``default_engine``,
                ``engines`` (each engine's launch recipe) ``release_sources`` (each
                release-hosted repository's revision and files with ``url`` and ``sha256``) and
                ``metal`` (the Mac recipes keyed by memory class ``"64"`` and ``"128"``).
        """
        spec = ModelMatrixRegistry.get_spec(model_key)
        if spec is None:
            raise KeyError(model_key)
        engines = ModelMatrixRegistry.engines(model_key)
        recipes = {e: ModelMatrixRegistry.get_launch_overrides(model_key, e) for e in engines}
        model = {
            "name": spec.name,
            "params_b": spec.params_b,
            "supported_precisions": list(spec.supported_precisions),
            "min_memory_gb": spec.min_memory_gb,
            "max_memory_gb": spec.max_memory_gb,
            "compatible_gb10": spec.compatible_gb10,
            "hf_repo_id": spec.hf_repo_id,
            "supports_vision": spec.supports_vision,
            "pinned_revision": ModelMatrixRegistry.pinned_revision(model_key),
            "default_engine": engines[0],
            "engines": recipes,
            "release_sources": {
                r: ModelMatrixRegistry.get_release_source(r) for r in _release_repos(recipes)
            },
            "metal": _metal_recipes(),
        }
        return {
            "schema_version": SCHEMA_VERSION,
            "models_release_tag": MODELS_RELEASE_TAG,
            "default_model": model_key,
            "model": model,
        }

    @classmethod
    def export_json(cls) -> str:
        """The export as the committed fixture's exact text: sorted keys, two-space indent."""
        return json.dumps(cls.export(), indent=2, sort_keys=True, ensure_ascii=False) + "\n"

    @classmethod
    def fixture_path(cls) -> Path:
        """The fixture's path in a checkout."""
        return Path(__file__).resolve().parents[2] / "ragon-rs" / "vectors" / "model-registry.json"


if __name__ == "__main__":  # pragma: no cover - writes the fixture
    RegistryExport.fixture_path().write_text(RegistryExport.export_json())
