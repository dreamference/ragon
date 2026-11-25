"""The platform layer of the registry and the launch: per-platform image, GPU arguments and the fit.

strix-halo-linux is the AMD Strix Halo node (gfx1151, ROCm/HIP) on Linux. Nothing here touches a
GPU or Docker: the platform comes from DREAMFERENCE_PLATFORM and the GPU memory is faked.
"""

import subprocess
import sys
from pathlib import Path

import pytest
from test_ragon_serve_engine import _stub_start, split_at_image

from dreamference.hardware import gpu_platform
from dreamference.hardware.gpu_platform import (
    GB10_GPU_ARGS,
    PLATFORM_ENV,
    PLATFORM_GB10,
    PLATFORM_STRIX_HALO_LINUX,
    STRIX_HALO_GPU_ARGS,
    current_platform,
    gpu_docker_args,
    platform_ragon_sessions,
    strix_fit_sessions,
)
from dreamference.hardware.hardware_manager import HardwareManager
from dreamference.hardware.model_matrix_registry import (
    DEFAULT_MODEL_ALIAS,
    ENGINE_RAGON,
    QWEN38_RAGON_HIP_IMAGE,
    QWEN38_RAGON_IMAGE,
    ModelMatrixRegistry,
    image_for_platform,
    require_pinned_image,
)
from dreamference.vllm_server.vllm_server_manager import VLLMServerManager

PIN_SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "pin_ragon_serve_image.py"


@pytest.fixture
def strix(monkeypatch):
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_STRIX_HALO_LINUX)


def test_the_platform_comes_from_the_environment_else_the_gb10(monkeypatch):
    monkeypatch.delenv(PLATFORM_ENV, raising=False)
    monkeypatch.setattr(gpu_platform.Path, "exists", lambda self: False)
    assert current_platform() == PLATFORM_GB10
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_STRIX_HALO_LINUX)
    assert current_platform() == PLATFORM_STRIX_HALO_LINUX
    monkeypatch.setenv(PLATFORM_ENV, "tpu")
    with pytest.raises(ValueError):
        current_platform()


def test_each_platform_gives_the_container_its_own_gpu_arguments():
    assert gpu_docker_args(PLATFORM_GB10) == ["--gpus", "all"] == list(GB10_GPU_ARGS)
    amd = gpu_docker_args(PLATFORM_STRIX_HALO_LINUX)
    assert amd == list(STRIX_HALO_GPU_ARGS) and "--gpus" not in amd
    assert amd[: 2 * 2] == ["--device", "/dev/kfd", "--device", "/dev/dri"]
    assert "seccomp=unconfined" in amd and "memlock=-1" in amd and "--ipc=host" in amd
    assert [amd[i + 1] for i, a in enumerate(amd) if a == "--group-add"] == ["video", "render"]


@pytest.mark.parametrize(
    ("usable_gb", "sessions"), [(90.0, 4), (68.7, 2), (43.0, 1), (40.0, 0), (200.0, 4), (0.0, 0)]
)
def test_the_fit_picks_four_two_one_or_refuses(usable_gb, sessions):
    assert strix_fit_sessions(usable_gb, 4) == sessions


def test_the_fit_never_exceeds_the_recipe_and_counts_an_exact_multiple():
    assert strix_fit_sessions(24 + 4 + 14.4 * 3, 4) == 3
    assert strix_fit_sessions(90.0, 2) == 2


def test_only_strix_cuts_sessions_from_the_gpu_memory(monkeypatch):
    monkeypatch.setattr(gpu_platform, "strix_usable_gpu_gb", lambda: 68.7)
    assert platform_ragon_sessions(4, PLATFORM_GB10) is None
    assert platform_ragon_sessions(4, PLATFORM_STRIX_HALO_LINUX) == 2


def test_usable_memory_is_the_carve_out_plus_gtt_only_in_mode_a(tmp_path, monkeypatch):
    dev = tmp_path / "card1" / "device"
    dev.mkdir(parents=True)
    monkeypatch.setattr(gpu_platform, "_DRM_ROOT", tmp_path)
    (dev / "mem_info_gtt_total").write_text(str(40 * 10**9))
    (dev / "mem_info_vram_total").write_text(str(int(68.7e9)))
    assert gpu_platform.strix_usable_gpu_gb() == pytest.approx(68.7)
    (dev / "mem_info_vram_total").write_text(str(2 * 10**9))
    assert gpu_platform.strix_usable_gpu_gb() == pytest.approx(42.0)
    monkeypatch.setattr(gpu_platform, "_DRM_ROOT", tmp_path / "none")
    assert gpu_platform.strix_usable_gpu_gb() == 0.0


def test_the_registry_resolves_one_recipe_key_to_an_image_per_platform(monkeypatch):
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_GB10)
    gb10 = ModelMatrixRegistry.get_launch_overrides(DEFAULT_MODEL_ALIAS, ENGINE_RAGON)
    assert gb10["docker_image"] == QWEN38_RAGON_IMAGE
    assert gb10["images"][PLATFORM_GB10] == QWEN38_RAGON_IMAGE
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_STRIX_HALO_LINUX)
    amd = ModelMatrixRegistry.get_launch_overrides(DEFAULT_MODEL_ALIAS, ENGINE_RAGON)
    assert amd["docker_image"] == QWEN38_RAGON_HIP_IMAGE
    assert QWEN38_RAGON_HIP_IMAGE.startswith("ghcr.io/dreamference/ragon-serve-hip@sha256:")
    # Every other flag of the recipe is the same on both platforms.
    assert {k: v for k, v in amd.items() if k != "docker_image"} == {
        k: v for k, v in gb10.items() if k != "docker_image"
    }
    with pytest.raises(ValueError, match="no ragon-serve image"):
        image_for_platform({PLATFORM_GB10: "x"}, PLATFORM_STRIX_HALO_LINUX)


def test_an_unpinned_image_is_refused_until_the_engine_image_job_pins_it():
    with pytest.raises(ValueError, match="not pinned"):
        require_pinned_image(QWEN38_RAGON_HIP_IMAGE)
    assert require_pinned_image(QWEN38_RAGON_IMAGE) == QWEN38_RAGON_IMAGE


def test_the_old_gb10_flag_is_still_read_and_the_platforms_are_a_set():
    spec = ModelMatrixRegistry.get_spec(DEFAULT_MODEL_ALIAS)
    assert spec.compatible_gb10 is True
    assert spec.compatible_with(PLATFORM_GB10) and spec.compatible_with(PLATFORM_STRIX_HALO_LINUX)
    other = next(s for s in ModelMatrixRegistry.MATRIX.values() if s is not spec)
    assert other.compatible_platforms == frozenset({PLATFORM_GB10})
    assert not other.compatible_with(PLATFORM_STRIX_HALO_LINUX)


def test_the_launch_command_has_the_device_args_and_no_gpus_flag_on_strix(monkeypatch):
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_GB10)
    gb10 = VLLMServerManager().build_launch_command(model=DEFAULT_MODEL_ALIAS, engine=ENGINE_RAGON)
    gb10_prefix, gb10_server = split_at_image(gb10, QWEN38_RAGON_IMAGE)
    at = gb10_prefix.index("--gpus")
    assert gb10_prefix[at : at + 2] == ["--gpus", "all"]

    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_STRIX_HALO_LINUX)
    amd = VLLMServerManager().build_launch_command(model=DEFAULT_MODEL_ALIAS, engine=ENGINE_RAGON)
    amd_prefix, amd_server = split_at_image(amd, QWEN38_RAGON_HIP_IMAGE)
    assert "--gpus" not in amd_prefix
    assert amd_prefix[at : at + len(STRIX_HALO_GPU_ARGS)] == list(STRIX_HALO_GPU_ARGS)
    # Byte-identical otherwise: swapping the GPU arguments back gives the GB10's command.
    restored = [*amd_prefix[:at], "--gpus", "all", *amd_prefix[at + len(STRIX_HALO_GPU_ARGS) :]]
    assert restored == gb10_prefix and amd_server == gb10_server


def _start_on_strix(monkeypatch, usable_gb):
    monkeypatch.setenv(PLATFORM_ENV, PLATFORM_STRIX_HALO_LINUX)
    monkeypatch.setattr(gpu_platform, "strix_usable_gpu_gb", lambda: usable_gb)
    mgr = VLLMServerManager()
    calls: dict = {}
    _stub_start(monkeypatch, mgr, calls)
    # The hip image is a placeholder until it is pinned; the start is about the fit.
    monkeypatch.setattr(
        "dreamference.vllm_server.vllm_server_manager.require_pinned_image", lambda image: image
    )
    monkeypatch.setattr(
        mgr,
        "build_launch_command",
        lambda **kwargs: calls.setdefault("sessions", kwargs.get("ragon_sessions")) and None,
    )
    monkeypatch.setattr(
        mgr,
        "ragon_serve_plan",
        lambda *a, **k: {
            "sessions": a[2]["sessions"],
            "batch": a[2]["batch"],
            "max_context": 196608,
            "planned_bytes": 10_000_000_000,
        },
    )
    return mgr, calls


def test_start_on_strix_starts_with_the_sessions_that_fit_and_says_so(monkeypatch, capsys):
    mgr, calls = _start_on_strix(monkeypatch, 68.7)
    with pytest.raises(TypeError):
        mgr.start_server(model=DEFAULT_MODEL_ALIAS, background=True, engine=ENGINE_RAGON)
    assert calls["sessions"] == 2
    assert "Only 2 of 4 sessions fit" in capsys.readouterr().out


def test_start_on_strix_refuses_below_one_session(monkeypatch, capsys):
    mgr, calls = _start_on_strix(monkeypatch, 40.0)
    with pytest.raises((SystemExit, RuntimeError)):
        mgr.start_server(model=DEFAULT_MODEL_ALIAS, background=True, engine=ENGINE_RAGON)
    assert "sessions" not in calls
    assert "Not enough GPU memory for one ragon-serve session" in capsys.readouterr().out


def test_the_strix_compatibility_check_uses_the_measured_gpu_memory(monkeypatch, strix):
    monkeypatch.setattr(gpu_platform, "strix_usable_gpu_gb", lambda: 68.7)
    monkeypatch.setattr("dreamference.hardware.hardware_manager.strix_usable_gpu_gb", lambda: 68.7)
    ok, _ = HardwareManager.check_model_compatibility(DEFAULT_MODEL_ALIAS)
    assert ok
    monkeypatch.setattr("dreamference.hardware.hardware_manager.strix_usable_gpu_gb", lambda: 10.0)
    ok, msg = HardwareManager.check_model_compatibility(DEFAULT_MODEL_ALIAS)
    assert not ok and "GPU-usable" in msg
    ok, msg = HardwareManager.check_model_compatibility("the-122b-not-on-strix")
    assert ok  # unknown model: unchanged behaviour


def test_pin_script_moves_only_the_hip_digest_with_platform(tmp_path):
    registry = tmp_path / "registry.py"
    source = Path(gpu_platform.__file__).parent / "model_matrix_registry.py"
    registry.write_text(source.read_text())
    digest = "sha256:" + "ab" * 32
    run = subprocess.run(
        [
            sys.executable,
            str(PIN_SCRIPT),
            "c" * 40,
            digest,
            "--platform",
            "strix-halo-linux",
            "--registry",
            str(registry),
        ],
        capture_output=True,
        text=True,
    )
    assert run.returncode == 0, run.stderr
    new = registry.read_text()
    assert f"ragon-serve-hip@{digest}" in new
    assert QWEN38_RAGON_IMAGE in new and "c" * 40 not in new
