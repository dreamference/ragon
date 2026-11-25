import pytest

from dreamference.hardware.memory_tier import (
    TIER_ENTRY,
    TIER_FULL,
    TIER_NONE,
    classify_memory,
)


@pytest.mark.parametrize(
    ("kwargs", "tier", "klass"),
    [
        ({"ram_gib": 32, "platform": "mac"}, TIER_NONE, None),
        ({"ram_gib": 36, "platform": "mac"}, TIER_NONE, None),
        ({"ram_gib": 48, "platform": "mac"}, TIER_NONE, None),
        ({"ram_gib": 62}, TIER_ENTRY, 64),
        ({"ram_gib": 16, "carve_out_gib": 46, "platform": "linux-amd"}, TIER_ENTRY, 64),
        ({"ram_gib": 64, "platform": "mac"}, TIER_ENTRY, 64),
        ({"ram_gib": 90}, TIER_ENTRY, 64),
        ({"ram_gib": 121, "platform": "gb10"}, TIER_FULL, 128),
        ({"ram_gib": 128, "platform": "mac"}, TIER_FULL, 128),
        ({"ram_gib": 32, "carve_out_gib": 96, "platform": "linux-amd"}, TIER_FULL, 128),
    ],
)
def test_tier_vectors(kwargs, tier, klass):
    result = classify_memory(**kwargs)
    assert result.tier == tier
    assert result.memory_class == klass
    assert result.status_fields()["tier"] == tier


def test_gb10_recipe_is_the_full_recipe():
    r = classify_memory(121, platform="gb10").recipe
    assert (r.sessions, r.max_model_len, r.drafter, r.vision) == (4, 262144, "sparse-head", True)
    assert (r.prefix_store_gb, r.gpu_cap_gb, r.os_floor_gb) == (4, None, None)


def test_amd_full_recipe_window():
    assert classify_memory(128, platform="linux-amd").recipe.max_model_len == 196608


def test_entry_recipe_and_fields():
    t = classify_memory(64, platform="mac")
    r = t.recipe
    assert (r.sessions, r.max_model_len, r.drafter, r.prefix_store_gb) == (2, 131072, "general", 2)
    assert (r.gpu_cap_gb, r.os_floor_gb) == (48, 16)
    assert t.status_fields() == {
        "tier": "entry",
        "sessions": 2,
        "memory_class": 64,
        "max_model_len": 131072,
    }


def test_refusal_48_exact():
    t = classify_memory(48, platform="mac")
    assert t.recipe is None
    assert t.sessions == 0
    assert t.refusal == (
        "This Mac has 48 GB of memory. A Ragon node needs 64 GB or more: the model takes about "
        "19 GB and each conversation's context takes more, and the computer needs room of its "
        "own. You can still use this Mac as a client of another node. 64 GB gives the "
        "entry-level node, 128 GB the full node."
    )


def test_refusal_32_and_linux_reading_snaps():
    t = classify_memory(31, platform="linux")
    assert "has 32 GB" in t.refusal
    assert "the model alone would leave the computer no room to work" in t.refusal
    assert "Mac" not in t.refusal


def test_carve_out_64_on_64_machine_is_refused_with_the_number():
    t = classify_memory(0.5, platform="linux-amd", carve_out_gib=64)
    assert t.tier == TIER_NONE
    assert "between 40 and 48 GB" in t.refusal
    assert "64 GB" in t.refusal


def test_carve_out_too_small_is_refused_too():
    assert classify_memory(40, platform="windows", carve_out_gib=24).refusal
