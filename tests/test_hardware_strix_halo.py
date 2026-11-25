"""Platform detection and memory facts of an AMD Strix Halo, from fake sysfs trees."""

import os
from pathlib import Path

import pytest

from dreamference.hardware import hardware_manager
from dreamference.hardware.hardware_manager import HardwareManager
from dreamference.hardware.hardware_platform import HardwarePlatform
from dreamference.hardware.strix_halo_memory import StrixHaloMemory

GIB = 2**30


def _device(root: Path, slot: str, vendor: str, device: str) -> None:
    folder = root / "pci" / slot
    folder.mkdir(parents=True)
    (folder / "vendor").write_text(vendor + "\n")
    (folder / "device").write_text(device + "\n")


def _machine(
    root: Path,
    monkeypatch: pytest.MonkeyPatch,
    pci: tuple[str, str] | None,
    gfx: str | None,
    arch: str = "x86_64",
) -> None:
    (root / "pci").mkdir()
    if pci:
        _device(root, "0000:c5:00.0", *pci)
    for number, target in enumerate(("0", gfx) if gfx else ("0",)):
        node = root / "kfd" / str(number)
        node.mkdir(parents=True)
        (node / "properties").write_text(f"simd_count 80\ngfx_target_version {target}\n")
    monkeypatch.setattr(hardware_manager, "PCI_DEVICES_DIR", str(root / "pci"))
    monkeypatch.setattr(hardware_manager, "KFD_NODES_DIR", str(root / "kfd"))
    monkeypatch.setattr(
        HardwareManager, "_query_nvidia_smi", staticmethod(lambda: ("N/A", "N/A", 0.0))
    )
    real = os.uname()
    monkeypatch.setattr(
        hardware_manager.os,
        "uname",
        lambda: type("U", (), {"machine": arch, "sysname": real.sysname})(),
    )


def test_a_strix_halo_is_found_from_sysfs_facts(tmp_path, monkeypatch):
    _machine(tmp_path, monkeypatch, ("0x1002", "0x1586"), "110501")
    assert HardwareManager.detect_platform() is HardwarePlatform.STRIX_HALO_LINUX


@pytest.mark.parametrize(
    "pci, gfx, arch",
    [
        (("0x1002", "0x1585"), "110501", "x86_64"),  # another AMD APU
        (("0x1002", "0x1586"), "110000", "x86_64"),  # a different gfx target
        (("0x1002", "0x1586"), None, "x86_64"),  # the driver names no GPU
        (("0x8086", "0x1586"), "110501", "x86_64"),  # not AMD
        (None, "110501", "x86_64"),
        (("0x1002", "0x1586"), "110501", "aarch64"),
    ],
)
def test_nothing_else_is_a_strix_halo(tmp_path, monkeypatch, pci, gfx, arch):
    _machine(tmp_path, monkeypatch, pci, gfx, arch)
    assert HardwareManager.detect_platform() is None


def test_a_gb10_is_found_beside_it(tmp_path, monkeypatch):
    _machine(tmp_path, monkeypatch, ("0x10de", "0x2e12"), None, "aarch64")
    monkeypatch.setattr(hardware_manager, "MEMINFO_PATH", str(tmp_path / "none"))
    assert HardwareManager.detect_platform() is HardwarePlatform.GB10


def _memory_files(root: Path, monkeypatch, vram: int, gtt: int, cmdline: str = "") -> None:
    for name, vendor, vram_value in (("card0", "0x1002", vram), ("card1", "0x8086", 999 * GIB)):
        card = root / "drm" / name / "device"
        card.mkdir(parents=True)
        (card / "vendor").write_text(vendor + "\n")
        (card / "mem_info_vram_total").write_text(f"{vram_value}\n")
        (card / "mem_info_gtt_total").write_text(f"{gtt}\n")
    (root / "meminfo").write_text("MemTotal:       65466940 kB\nMemAvailable:   62893496 kB\n")
    (root / "cmdline").write_text(f"BOOT_IMAGE=/vmlinuz root=UUID=x ro {cmdline}\n")
    monkeypatch.setattr(hardware_manager, "DRM_DIR", str(root / "drm"))
    monkeypatch.setattr(hardware_manager, "MEMINFO_PATH", str(root / "meminfo"))
    monkeypatch.setattr(hardware_manager, "CMDLINE_PATH", str(root / "cmdline"))


def test_the_memory_facts_come_from_the_amd_card_meminfo_and_cmdline(tmp_path, monkeypatch):
    _memory_files(
        tmp_path, monkeypatch, 64 * GIB, 31 * GIB, "ttm.pages_limit=1 quiet amdgpu.gttsize=2"
    )
    memory = HardwareManager.strix_halo_memory()
    assert memory.vram_bytes == 64 * GIB and memory.gtt_bytes == 31 * GIB
    assert memory.ram_bytes == 65466940 * 1024
    assert memory.cmdline_options == ("ttm.pages_limit=1", "amdgpu.gttsize=2")
    assert memory.usable_gib == 95


@pytest.mark.parametrize(
    "vram, gtt, allowed, said",
    [
        (64, 31, True, "usable by the GPU about 95.0 GiB"),
        (32, 31, True, "fewer than 4 sessions"),
        (8, 16, False, "below the 43 GiB"),
        (0, 0, True, "reports no VRAM size"),
    ],
)
def test_the_memory_verdict(vram, gtt, allowed, said):
    ok, text = HardwareManager.strix_halo_memory_verdict(
        StrixHaloMemory(vram * GIB, gtt * GIB, 62 * GIB, ())
    )
    assert ok is allowed and said in text


def test_without_any_files_the_facts_are_zero(tmp_path, monkeypatch):
    monkeypatch.setattr(hardware_manager, "DRM_DIR", str(tmp_path / "x"))
    monkeypatch.setattr(hardware_manager, "MEMINFO_PATH", str(tmp_path / "y"))
    monkeypatch.setattr(hardware_manager, "CMDLINE_PATH", str(tmp_path / "z"))
    assert HardwareManager.strix_halo_memory() == StrixHaloMemory(0, 0, 0, ())
