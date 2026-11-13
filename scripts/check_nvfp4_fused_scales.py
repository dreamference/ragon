"""Checks that an NVFP4 checkpoint's fused projection groups share one global scale.

SGLang (and vLLM) load a Qwen3.5/3.8 layer's ``in_proj_qkv`` and ``in_proj_z``, ``in_proj_b`` and
``in_proj_a``, ``q/k/v_proj`` and ``gate/up_proj`` into one fused GEMM each, and a fused NVFP4 GEMM
has one global weight scale: SGLang's ``CompressedTensorsW4A4Fp4`` takes the largest of the group's
scales and does not rescale the block scales. A checkpoint calibrated per module therefore serves
with mis-scaled weights, silently: arXiv 2609.04098 found the DeltaNet decay and write gates off by
1.82x and 2.75x, with long-context perplexity looking *better* for it. A checkpoint is safe to serve
only when every group's scales are equal, which this script checks from the safetensors headers and
the scale tensors alone (a few kilobytes read; no torch, no GPU).

    python3 scripts/check_nvfp4_fused_scales.py <snapshot directory>

Exit 0 when every group is harmonized, 1 when one is not, 2 when the checkpoint cannot be read or
has no compressed-tensors NVFP4 scales. specs/DREAMFERENCE_MODELS.md §2.2.
"""

from __future__ import annotations

import json
import re
import struct
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any, Final

# The fused groups of SGLang's Qwen3_5ForCausalLM (packed_modules_mapping in models/qwen3_5.py).
GROUPS: Final[tuple[tuple[str, ...], ...]] = (
    ("linear_attn.in_proj_qkv", "linear_attn.in_proj_z"),
    ("linear_attn.in_proj_b", "linear_attn.in_proj_a"),
    ("self_attn.q_proj", "self_attn.k_proj", "self_attn.v_proj"),
    ("mlp.gate_proj", "mlp.up_proj"),
)
SCALES: Final[tuple[str, ...]] = ("weight_global_scale", "input_global_scale")
# Equal means equal as stored; a relative tolerance only absorbs a float32 round trip.
TOLERANCE: Final[float] = 1e-6
# Safetensors layout: the 8-byte little-endian header length, the metadata key, and a scalar
# F32 tensor (the dtype the global scales are stored as, 4 bytes each).
HEADER_LEN_BYTES: Final[int] = 8
METADATA_KEY: Final[str] = "__metadata__"
SCALAR_DTYPE: Final[str] = "F32"
SCALAR_BYTES: Final[int] = 4


def _scalar_entries(header: dict[str, Any]) -> Iterator[tuple[str, int]]:
    """(name, start offset) of each scalar F32 ``*_global_scale`` entry of a safetensors header."""
    for name, meta in header.items():
        if name == METADATA_KEY or not name.endswith(SCALES):
            continue
        start, end = meta["data_offsets"]
        if meta["dtype"] != SCALAR_DTYPE or end - start != SCALAR_BYTES:
            continue
        yield name, start


def read_scalars(snapshot: Path) -> dict[str, float]:
    """Every scalar F32 tensor named ``*_global_scale`` in the snapshot's safetensors files."""
    values: dict[str, float] = {}
    for shard in sorted(snapshot.glob("*.safetensors")):
        with shard.open("rb") as handle:
            size = struct.unpack("<Q", handle.read(HEADER_LEN_BYTES))[0]
            header = json.loads(handle.read(size))
            base = HEADER_LEN_BYTES + size
            for name, start in _scalar_entries(header):
                handle.seek(base + start)
                values[name] = struct.unpack("<f", handle.read(SCALAR_BYTES))[0]
    return values


def _group_finding(
    layer: str, group: tuple[str, ...], scale: str, values: dict[str, float]
) -> tuple[bool, str | None]:
    """(counted, line) of one (layer, group, scale) triple; line is None when absent or harmonized."""
    found = [values.get(f"{layer}.{member}.{scale}") for member in group]
    if all(v is None for v in found):
        return False, None
    if any(v is None for v in found):
        return True, f"{layer} {'+'.join(group)} {scale}: missing for part of the group {found}"
    present = [v for v in found if v is not None]
    low, high = min(present), max(present)
    if high - low > TOLERANCE * max(abs(high), 1e-30):
        return True, (
            f"{layer} {'+'.join(m.split('.')[-1] for m in group)} {scale}: "
            f"{', '.join(f'{v:.6g}' for v in present)} (ratio {high / low:.3f})"
        )
    return True, None


def check(values: dict[str, float]) -> tuple[int, int, list[str]]:
    """(groups checked, groups that differ, a line per difference)."""
    layers = sorted(
        {m.group(1) for m in (re.match(r"(.*\.layers\.\d+)\.", n) for n in values) if m},
        key=lambda p: int(p.rsplit(".", 1)[1]),
    )
    checked, bad, lines = 0, 0, []
    for layer in layers:
        for group, scale in ((g, s) for g in GROUPS for s in SCALES):
            counted, line = _group_finding(layer, group, scale, values)
            if not counted:
                continue
            checked += 1
            if line is not None:
                bad += 1
                lines.append(line)
    return checked, bad, lines


def main(argv: list[str]) -> int:
    """Run the fused-group scale check on one snapshot; return the exit status."""
    if len(argv) != 1:
        print(  # noqa: T201 - usage on the console
            __doc__.strip().split("\n\n")[1]
            if __doc__
            else "usage: check_nvfp4_fused_scales.py <snapshot>"
        )
        return 2
    snapshot = Path(argv[0])
    try:
        values = read_scalars(snapshot)
    except (OSError, ValueError, KeyError, struct.error) as exc:
        print(f"❌ Cannot read {snapshot}: {exc}")  # noqa: T201 - CLI output
        return 2
    if not values:
        print(f"❌ No compressed-tensors NVFP4 global scales in {snapshot}.")  # noqa: T201 - CLI output
        return 2
    checked, bad, lines = check(values)
    if not checked:
        print(  # noqa: T201 - CLI output
            f"❌ {len(values)} global scales, but none in a fused group this script knows."
        )
        return 2
    for line in lines[:40]:
        print(f"   {line}")  # noqa: T201 - CLI output
    if bad:
        print(  # noqa: T201 - CLI output
            f"❌ {bad} of {checked} fused-group scales differ: SGLang would serve those GEMMs mis-scaled."
        )
        return 1
    print(f"✅ All {checked} fused-group scales are harmonized ({len(values)} global scales read).")  # noqa: T201 - CLI output
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
