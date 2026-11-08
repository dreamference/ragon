"""The node monitor's user unit, `ragon-node-monitor.service`.

On by default on every node (specs/DREAMFERENCE_MONITORING.md O11). The unit follows §8.3: a
notify service with systemd's watchdog on the monitor itself, bounded at 64 MiB, logging to
journald (standard output and error are the journal's by default for a user unit).
"""

import logging
import pathlib
import subprocess
from typing import Final

from dreamference.runner.codex_branded_builder import INSTALL_DIR

_LOG: Final = logging.getLogger(__name__)
UNIT: Final[str] = "ragon-node-monitor.service"
BINARY: Final[str] = "ragon-node-monitor"


class NodeMonitorService:
    """Writes, enables and disables the node monitor's user unit."""

    @classmethod
    def unit_dir(cls) -> pathlib.Path:
        """Return the folder of the user's systemd units.

        Returns:
            pathlib.Path: `~/.config/systemd/user`.
        """
        return pathlib.Path("~/.config/systemd/user").expanduser()

    @classmethod
    def executable(cls) -> pathlib.Path:
        """Return where the monitor binary is installed, beside `ragon`.

        Returns:
            pathlib.Path: The binary's path (units run with a minimal PATH).
        """
        return pathlib.Path(INSTALL_DIR) / "bin" / BINARY

    @classmethod
    def unit_text(cls) -> str:
        """Render the unit file (§8.3).

        Returns:
            str: The text of `ragon-node-monitor.service`.
        """
        return (
            "[Unit]\nDescription=Ragon node monitor\nAfter=default.target\n\n"
            f"[Service]\nType=notify\nNotifyAccess=main\nExecStart={cls.executable()} run\n"
            "WatchdogSec=60\nMemoryHigh=48M\nMemoryMax=64M\nCPUQuota=5%\nNice=10\n"
            "IOSchedulingClass=idle\nTasksMax=32\nStandardOutput=journal\n"
            "StandardError=journal\nSyslogIdentifier=ragon-node-monitor\n"
            "Restart=on-failure\nRestartSec=10\nRestartSteps=5\nRestartMaxDelaySec=10min\n\n"
            "[Install]\nWantedBy=default.target\n"
        )

    @classmethod
    def systemctl(cls, *arguments: str) -> subprocess.CompletedProcess:
        """Run `systemctl --user`.

        Args:
            *arguments: `systemctl --user`'s arguments.

        Returns:
            subprocess.CompletedProcess: Its result; a failure to run it is a return code 1.
        """
        try:
            return subprocess.run(
                ["systemctl", "--user", *arguments],
                capture_output=True,
                text=True,
                timeout=120,
                check=False,
            )
        except (OSError, subprocess.SubprocessError) as error:
            return subprocess.CompletedProcess(arguments, 1, "", str(error))

    @classmethod
    def setup(cls) -> bool:
        """Write the unit, enable it and start it; the unit is written even if the binary is not built.

        Returns:
            bool: True when the unit is enabled and started.
        """
        folder = cls.unit_dir()
        folder.mkdir(parents=True, exist_ok=True)
        (folder / UNIT).write_text(cls.unit_text())
        cls.systemctl("daemon-reload")
        if not cls.executable().is_file():
            _LOG.warning(
                "%s is not built; run `ragon-admin codex build`, then `node enable`.", BINARY
            )
            return False
        return cls.systemctl("enable", "--now", UNIT).returncode == 0

    @classmethod
    def off(cls) -> bool:
        """Stop and disable the unit.

        Returns:
            bool: True when systemd accepted the request.
        """
        return cls.systemctl("disable", "--now", UNIT).returncode == 0
