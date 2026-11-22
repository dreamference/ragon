"""Every dreamference package imports first in a fresh interpreter (no import cycle)."""

import subprocess
import sys
from pathlib import Path

import pytest

PACKAGES = [
    "accounts",
    "audit",
    "chat",
    "cli",
    "config",
    "context_engine",
    "forge",
    "hardware",
    "mcp_server",
    "night_shift",
    "node",
    "remote",
    "runner",
    "swe_bench",
    "vllm_server",
]
ROOT = Path(__file__).resolve().parent.parent


@pytest.mark.parametrize("package", PACKAGES)
def test_package_imports_first(package: str) -> None:
    result = subprocess.run(
        [sys.executable, "-c", f"import dreamference.{package}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert result.returncode == 0, result.stderr[-1500:]


def test_hardware_registry_imports_first() -> None:
    result = subprocess.run(
        [sys.executable, "-c", "import dreamference.hardware.model_matrix_registry"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
        check=False,
    )
    assert result.returncode == 0, result.stderr[-1500:]
