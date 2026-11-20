"""Metal recipes per Mac memory size (engine Metal spec §4.2), exported for the Rust reader.

The table is the spec's "Memory plan per Mac size": one default recipe per RAM size, with the
GPU-resident cap the plan assumes (an estimate, TO VERIFY on each machine). The converted weights'
SHA-256 (spec §5, "the weights") is a slot per model that stays ``None`` until the first conversion
is pinned; a start refuses on a mismatch once it is set. Linux recipes are not touched.
"""

import json
from pathlib import Path
from typing import Any, Final

# The model the recipes are for (the registry's default).
METAL_MODEL_KEY: Final[str] = "qwen3.8-27b-nvfp4-dflash2"

# MN5: 128 GB is the Mac node's gate recipe.
METAL_GATE_MEMORY_GB: Final[int] = 128

# Where the vision tower and the sparse head sit; the directory is relative to the model's snapshot.
METAL_SPARSE_HEAD_DIR: Final[str] = "sparse-head"

_ROWS: Final[tuple[dict[str, Any], ...]] = (
    {
        "memory_gb": 36,
        "gpu_cap_gb": 27,
        "sessions": 2,
        "max_context": 32768,
        "prefill_chunk": 512,
        "vision": False,
    },
    {
        "memory_gb": 48,
        "gpu_cap_gb": 36,
        "sessions": 2,
        "max_context": 65536,
        "prefill_chunk": 1024,
        "vision": True,
    },
    {
        "memory_gb": 64,
        "gpu_cap_gb": 48,
        "sessions": 2,
        "max_context": 131072,
        "prefill_chunk": 2048,
        "vision": True,
    },
    {
        "memory_gb": 128,
        "gpu_cap_gb": 96,
        "sessions": 4,
        "batch": 4,
        "max_context": 262144,
        "prefill_chunk": 2048,
        "vision": True,
    },
    {
        "memory_gb": 256,
        "gpu_cap_gb": 192,
        "sessions": 8,
        "max_context": 262144,
        "prefill_chunk": 2048,
        "vision": True,
    },
)


class MetalRecipes:
    """The Metal launch recipes by memory size and their JSON export."""

    @classmethod
    def export(cls) -> dict[str, Any]:
        """The export the Rust reader consumes (`ragon-rs/vectors/metal-recipes.json`).

        Returns:
            dict: ``model``, the weights' ``weights_sha256`` slot (``None`` until pinned), the gate
                memory size and the recipes, smallest memory first, each with the sparse-head directory.
        """
        recipes = [{**row, "sparse_head_dir": METAL_SPARSE_HEAD_DIR} for row in _ROWS]
        return {
            "version": 1,
            "model": METAL_MODEL_KEY,
            "weights_sha256": None,
            "gate_memory_gb": METAL_GATE_MEMORY_GB,
            "recipes": recipes,
        }

    @classmethod
    def export_json(cls) -> str:
        """The export as the committed fixture's exact text."""
        return json.dumps(cls.export(), indent=2) + "\n"

    @classmethod
    def recipe_for(cls, memory_gb: int) -> dict[str, Any] | None:
        """The recipe of the largest listed size that fits ``memory_gb`` of RAM, or None below 36 GB."""
        best = None
        for recipe in cls.export()["recipes"]:
            if recipe["memory_gb"] <= memory_gb:
                best = recipe
        return best

    @classmethod
    def fixture_path(cls) -> Path:
        """The fixture's path in a checkout."""
        return Path(__file__).resolve().parents[2] / "ragon-rs" / "vectors" / "metal-recipes.json"
