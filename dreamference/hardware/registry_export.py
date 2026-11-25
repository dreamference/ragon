"""The default model's registry entry as deterministic JSON, for the Rust reader.

A node with no Python (Windows) reads its launch recipe and the release assets' SHA-256 hashes from
this export, which is committed as ``ragon-rs/vectors/model-registry.json`` and published as a
release asset. Field names are the registry's own (``ModelSpec`` and the recipe keys).
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from dreamference.hardware.model_matrix_registry import (
    DEFAULT_MODEL_ALIAS,
    MODELS_RELEASE_TAG,
    ModelMatrixRegistry,
)

SCHEMA_VERSION = 1


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
                ``engines`` (each engine's launch recipe) and ``release_sources`` (each
                release-hosted repository's revision and files with ``url`` and ``sha256``).
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
