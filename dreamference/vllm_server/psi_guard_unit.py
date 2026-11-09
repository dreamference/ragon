"""The memory-pressure watchdog as its own user unit, `ragon-psi-guard`.

A thread inside `ragon-admin` dies with it; a user unit keeps guarding the resident model server
after `server start` has returned, and `server stop` takes it down with the container.
"""

import contextlib
import subprocess
import sys
from typing import Final

PSI_GUARD_UNIT: Final[str] = "ragon-psi-guard"
SYSTEMD_TIMEOUT_S: Final[float] = 15.0


class PsiGuardUnit:
    """Starts and stops the `ragon-psi-guard` user unit."""

    @classmethod
    def start(cls, container_name: str) -> bool:
        """Starts the unit watching one container, replacing a stale unit of the same name.

        Args:
            container_name (str): The container the watchdog kills on sustained pressure.

        Returns:
            bool: True if systemd accepted the unit, False if it could not be started.
        """
        cls.stop()
        cmd = [
            "systemd-run",
            "--user",
            f"--unit={PSI_GUARD_UNIT}",
            "--collect",
            "--quiet",
            sys.executable,
            "-m",
            "dreamference.vllm_server.psi_watchdog",
            container_name,
        ]
        try:
            result = subprocess.run(
                cmd, capture_output=True, timeout=SYSTEMD_TIMEOUT_S, check=False
            )
        except (OSError, subprocess.SubprocessError):
            return False
        return result.returncode == 0

    @classmethod
    def stop(cls) -> None:
        """Stops the unit; a no-op when it is not running."""
        with contextlib.suppress(OSError, subprocess.SubprocessError):
            subprocess.run(
                ["systemctl", "--user", "stop", PSI_GUARD_UNIT],
                capture_output=True,
                timeout=SYSTEMD_TIMEOUT_S,
                check=False,
            )
