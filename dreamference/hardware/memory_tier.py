"""The node's memory tier from injected readings (entry-level node plan, section 1).

One shared function for every platform: the per-platform readers (Linux ``MemTotal`` plus the AMD
graphics carve-out, Mac ``hw.memsize``, Windows RAM plus the ADLX carve-out) hand their numbers in;
nothing here reads the machine. ``full`` is 100 GiB and above (GB10 reports about 121 GiB), ``entry``
is 60 GiB to under 100 GiB, ``none`` is under 60 GiB (no node role; client and runner stay).
"""

from dataclasses import dataclass
from typing import Any, Final

TIER_FULL: Final[str] = "full"
TIER_ENTRY: Final[str] = "entry"
TIER_NONE: Final[str] = "none"

FULL_MIN_GIB: Final[float] = 100.0
ENTRY_MIN_GIB: Final[float] = 60.0

# Entry recipe: the GPU may use at most 48 GB and the OS keeps at least 16 GB.
ENTRY_GPU_CAP_GB: Final[int] = 48
ENTRY_OS_FLOOR_GB: Final[int] = 16
# The graphics carve-out a 64 GB machine can give (AMD Linux, Windows).
CARVE_OUT_MIN_GB: Final[int] = 40
CARVE_OUT_MAX_GB: Final[int] = 48

PLATFORM_AMD: Final[str] = "linux-amd"
_MARKETING_SIZES: Final[tuple[int, ...]] = (8, 16, 24, 32, 36, 48, 64, 96, 128, 192, 256)
_LOW_MEMORY_GB: Final[int] = 40  # below this, the model alone leaves no room


@dataclass(frozen=True)
class TierRecipe:
    """The serving recipe of one tier."""

    sessions: int
    max_model_len: int
    drafter: str
    vision: bool
    prefix_store_gb: int
    gpu_cap_gb: int | None
    os_floor_gb: int | None


@dataclass(frozen=True)
class MemoryTier:
    """The classification of a machine: what node status and the mDNS TXT advertise."""

    tier: str
    memory_class: int | None
    memory_gb: int
    recipe: TierRecipe | None
    refusal: str | None = None

    @property
    def sessions(self) -> int:
        """Sessions the node serves; 0 when the machine has no node role."""
        return self.recipe.sessions if self.recipe else 0

    def status_fields(self) -> dict[str, Any]:
        """The `tier`, `sessions`, `memory_class` fields (plus `max_model_len` when a node)."""
        fields: dict[str, Any] = {
            "tier": self.tier,
            "sessions": self.sessions,
            "memory_class": self.memory_class,
        }
        if self.recipe:
            fields["max_model_len"] = self.recipe.max_model_len
        return fields


def _full_recipe(platform: str) -> TierRecipe:
    return TierRecipe(
        sessions=4,
        max_model_len=196_608 if platform == PLATFORM_AMD else 262_144,
        drafter="sparse-head",
        vision=True,
        prefix_store_gb=4,
        gpu_cap_gb=None,
        os_floor_gb=None,
    )


def _entry_recipe() -> TierRecipe:
    return TierRecipe(
        sessions=2,
        max_model_len=131_072,
        drafter="general",
        vision=True,
        prefix_store_gb=2,
        gpu_cap_gb=ENTRY_GPU_CAP_GB,
        os_floor_gb=ENTRY_OS_FLOOR_GB,
    )


def marketing_gb(gib: float) -> int:
    """The size a person calls the machine: the nearest standard size within 8 %, else rounded."""
    best = min(_MARKETING_SIZES, key=lambda size: abs(size - gib))
    return best if abs(best - gib) <= best * 0.08 else round(gib)


def machine_word(platform: str) -> str:
    """Neutral wording for the machine, never a vendor-internal term."""
    return {"mac": "Mac", "windows": "PC"}.get(platform, "computer")


def refusal_text(memory_gb: int, platform: str) -> str:
    """The node-role refusal for a machine under 60 GiB (plan section 1.4)."""
    reason = (
        "the model alone would leave the computer no room to work"
        if memory_gb < _LOW_MEMORY_GB
        else "the model takes about 19 GB and each conversation's context takes more, "
        "and the computer needs room of its own"
    )
    word = machine_word(platform)
    return (
        f"This {word} has {memory_gb} GB of memory. A Ragon node needs 64 GB or more: "
        f"{reason}. You can still use this {word} as a client of another node. "
        "64 GB gives the entry-level node, 128 GB the full node."
    )


def carve_out_refusal(carve_out_gb: float, platform: str) -> str:
    """The refusal for a graphics carve-out a 64 GB machine cannot give (plan section 1.3)."""
    word = machine_word(platform)
    return (
        f"The graphics memory of this {word} is set to {carve_out_gb:.0f} GB. On a 64 GB machine "
        f"that leaves the system too little room. Set it to between {CARVE_OUT_MIN_GB} and "
        f"{CARVE_OUT_MAX_GB} GB, then run the check again."
    )


def classify_memory(
    ram_gib: float,
    *,
    platform: str = "linux",
    carve_out_gib: float = 0.0,
) -> MemoryTier:
    """Classify a machine from injected readings.

    ``ram_gib`` is physical memory in GiB (Linux ``MemTotal``, Mac ``hw.memsize``, Windows RAM);
    ``carve_out_gib`` is the graphics carve-out that the OS does not count in it (AMD
    ``mem_info_vram_total``, Windows ADLX), added to get the physical total. ``platform`` is
    one of ``gb10``, ``linux``, ``linux-amd``, ``mac``, ``windows``.
    """
    carve = max(carve_out_gib, 0.0)
    physical = max(ram_gib, 0.0) + carve
    memory_gb = marketing_gb(physical)
    if physical >= FULL_MIN_GIB:
        return MemoryTier(TIER_FULL, 128, memory_gb, _full_recipe(platform))
    if physical >= ENTRY_MIN_GIB:
        if carve > 0 and not CARVE_OUT_MIN_GB <= carve <= CARVE_OUT_MAX_GB:
            return MemoryTier(
                TIER_NONE,
                None,
                memory_gb,
                None,
                carve_out_refusal(carve, platform),
            )
        return MemoryTier(TIER_ENTRY, 64, memory_gb, _entry_recipe())
    return MemoryTier(TIER_NONE, None, memory_gb, None, refusal_text(memory_gb, platform))
