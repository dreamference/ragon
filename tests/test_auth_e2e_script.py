"""The accounts end-to-end gate script (specs/DREAMFERENCE_AUTH.md §17, tail row 6): dry runs only."""

import subprocess
import sys
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(sys.platform == "win32", reason="POSIX shell script")
SCRIPT = Path(__file__).resolve().parent.parent / "scripts" / "auth_e2e.sh"
STEPS = [
    "E0a",
    "E0b",
    "E0c",
    "E1",
    "E2",
    "E3",
    "E4",
    "E5",
    "E6",
    "E7",
    "E8",
    "E9",
    "E10",
    "E11",
    "E12",
    "E13",
    "E14",
]


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["bash", str(SCRIPT), *args], capture_output=True, text=True, timeout=60, check=False
    )


def test_a_dry_run_lists_every_step_and_is_not_a_pass() -> None:
    done = _run("--primary", "a", "--secondary", "b", "--dry-run")
    assert done.returncode == 0
    for step in STEPS:
        assert f"[plan] {step}:" in done.stdout
    assert "not a gate pass" in done.stdout


def test_usage_errors_exit_two() -> None:
    assert _run("--primary", "a").returncode == 2
    assert _run("--primary", "a", "--secondary", "a").returncode == 2


def test_a_real_run_outside_the_gpu_window_is_blocked() -> None:
    done = _run("--primary", "a", "--secondary", "b", "--now", "2026-11-20 21:00")
    assert done.returncode == 3
    assert "GPU window" in done.stderr


def test_a_real_run_with_an_unreachable_node_is_blocked() -> None:
    done = _run(
        "--primary", "no-such-host.invalid", "--secondary", "b", "--now", "2026-11-20 12:00"
    )
    assert done.returncode == 3
    assert "unreachable" in done.stderr
