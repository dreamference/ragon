"""The GPU platforms a node can run on: how to name one, give a container its GPU, and size sessions.

Two platforms serve ragon-serve today: the NVIDIA GB10 (`gb10`, the default and the only one every
earlier release knew) and AMD Strix Halo on Linux (`strix-halo-linux`, gfx1151, ROCm/HIP). The
registry resolves one recipe key to a different image per platform; this module holds the rest of
what differs: the container's GPU arguments and the memory fit.

All memory here is in GB (10^9 bytes), as the engine's plan is: a 64 GiB carve-out is 68.7 GB.
"""

import os
from pathlib import Path
from typing import Final

PLATFORM_GB10: Final[str] = "gb10"
PLATFORM_STRIX_HALO_LINUX: Final[str] = "strix-halo-linux"
PLATFORMS: Final[tuple[str, ...]] = (PLATFORM_GB10, PLATFORM_STRIX_HALO_LINUX)
DEFAULT_PLATFORM: Final[str] = PLATFORM_GB10

# Overrides detection (a test, or a box whose probe has not landed); one of PLATFORMS.
PLATFORM_ENV: Final[str] = "DREAMFERENCE_PLATFORM"

GB10_GPU_ARGS: Final[tuple[str, ...]] = ("--gpus", "all")
# The AMD container prefix (AMD nodes spec §7). `--ipc=host` is also in the launch's own argument
# list on both platforms; the duplicate is harmless to docker and keeps this prefix whole.
STRIX_HALO_GPU_ARGS: Final[tuple[str, ...]] = (
    "--device",
    "/dev/kfd",
    "--device",
    "/dev/dri",
    "--group-add",
    "video",
    "--group-add",
    "render",
    "--security-opt",
    "seccomp=unconfined",
    "--ipc=host",
    "--ulimit",
    "memlock=-1",
)

# The session fit on strix-halo-linux, in GB. ESTIMATES until amdlinux-memory-mode measures them
# (plan §2: 68.7 GB device - 44.8 GB free in the pool = about 24 GB; KV 4.8 GB per session at
# 65,536 tokens, linear, so 14.4 GB at the recipe's 196,608).
STRIX_WEIGHTS_WORKSPACE_GB: Final[float] = 24.0
STRIX_SESSION_KV_GB: Final[float] = 14.4
STRIX_RESERVE_GB: Final[float] = 4.0

# A firmware carve-out below this means the GPU draws from GTT (mode A of the plan): the pool is
# GTT, not the carve-out. Placeholder for the platform facts amdlinux-install-target records.
_CARVE_OUT_MODE_A_MAX_GB: Final[float] = 8.0
_DRM_ROOT: Final[Path] = Path("/sys/class/drm")


def current_platform() -> str:
    """Names the platform this node runs on.

    The environment wins; then an AMD GPU device (`/dev/kfd`) without an NVIDIA one means
    strix-halo-linux; everything else is the GB10, as before.

    Returns:
        str: One of PLATFORMS.

    Raises:
        ValueError: When the environment names a platform that does not exist.
    """
    named = os.environ.get(PLATFORM_ENV, "").strip()
    if named:
        if named not in PLATFORMS:
            raise ValueError(f"{PLATFORM_ENV}={named!r} is not one of {', '.join(PLATFORMS)}")
        return named
    if Path("/dev/kfd").exists() and not Path("/dev/nvidiactl").exists():
        return PLATFORM_STRIX_HALO_LINUX
    return DEFAULT_PLATFORM


def gpu_docker_args(platform: str | None = None) -> list[str]:
    """The `docker run` arguments that give a container the GPU, for a platform.

    Args:
        platform: One of PLATFORMS; None is the current one.

    Returns:
        list[str]: A fresh list.
    """
    platform = platform or current_platform()
    if platform == PLATFORM_STRIX_HALO_LINUX:
        return list(STRIX_HALO_GPU_ARGS)
    return list(GB10_GPU_ARGS)


def strix_usable_gpu_gb() -> float:
    """Reads the GPU-usable memory of the first AMD card: vram_total, plus gtt_total in mode A.

    Returns:
        float: GB (10^9), 0.0 when no card reports it.
    """
    for card in sorted(_DRM_ROOT.glob("card[0-9]*/device")):
        vram = _read_bytes(card / "mem_info_vram_total")
        if vram is None:
            continue
        gtt = _read_bytes(card / "mem_info_gtt_total") or 0
        usable = vram + (gtt if vram / 1e9 < _CARVE_OUT_MODE_A_MAX_GB else 0)
        return usable / 1e9
    return 0.0


def _read_bytes(path: Path) -> int | None:
    """Reads one integer from a sysfs file, None when it is missing or not a number."""
    try:
        return int(path.read_text().strip())
    except (OSError, ValueError):
        return None


def strix_fit_sessions(usable_gb: float, recipe_sessions: int) -> int:
    """How many sessions fit in the GPU-usable memory at the recipe's context.

    sessions = min(recipe, floor((usable - weights and workspace - reserve) / per-session KV)).
    Fewer than the recipe's is a cut the caller logs; 0 is a refusal.

    Args:
        usable_gb: GPU-usable memory in GB (10^9).
        recipe_sessions: The recipe's session count.

    Returns:
        int: Sessions that fit, 0 when not even one does.
    """
    room = usable_gb - STRIX_WEIGHTS_WORKSPACE_GB - STRIX_RESERVE_GB
    # Rounded first so an exact multiple (43.2 GB of room is three sessions) is not lost to float noise.
    fits = int(round(room / STRIX_SESSION_KV_GB, 6) // 1) if room > 0 else 0
    return max(0, min(recipe_sessions, fits))


def platform_ragon_sessions(recipe_sessions: int, platform: str | None = None) -> int | None:
    """The sessions the platform's memory allows, or None where the platform does not cut them.

    Args:
        recipe_sessions: The recipe's session count.
        platform: One of PLATFORMS; None is the current one.

    Returns:
        int | None: None on the GB10 (its fit is the engine's plan); on strix-halo-linux the fit,
            0 meaning refuse.
    """
    platform = platform or current_platform()
    if platform != PLATFORM_STRIX_HALO_LINUX:
        return None
    return strix_fit_sessions(strix_usable_gpu_gb(), recipe_sessions)
