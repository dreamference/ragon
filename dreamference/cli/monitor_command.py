"""`ragon-admin monitor status [--json]`: the node's health, from `ragon-node-monitor`."""

import argparse
import os
import pathlib
import subprocess
import sys
from typing import Final

from dreamference.runner.codex_branded_builder import INSTALL_DIR

MONITOR_COMMAND: Final[str] = "monitor"
MONITOR_BINARY: Final[str] = "ragon-node-monitor"
STATUS: Final[str] = "status"
RUN_TIMEOUT_SECONDS: Final[int] = 60
NOT_BUILT_EXIT: Final[int] = 1


class MonitorCommand:
    """Runs the `monitor` group (the session monitor stays `ragon monitor`)."""

    @classmethod
    def add_parser(
        cls, subparsers: "argparse._SubParsersAction[argparse.ArgumentParser]"
    ) -> argparse.ArgumentParser:
        """Adds `monitor` and its subcommands.

        Args:
            subparsers: The top-level parser's subcommands.

        Returns:
            argparse.ArgumentParser: The group's parser.
        """
        parser = subparsers.add_parser(
            MONITOR_COMMAND, help="Show the node's health (forwards to ragon-node-monitor)"
        )
        commands = parser.add_subparsers(dest="monitor_command")
        status = commands.add_parser(STATUS, help="Show the node's state and its checks")
        status.add_argument("--json", action="store_true", help="Print the monitor's JSON as is")
        return parser

    @classmethod
    def executable(cls) -> str | None:
        """Locates the installed `ragon-node-monitor`, beside `ragon`.

        Returns:
            str | None: Absolute path, or None when it has not been built.
        """
        path = pathlib.Path(INSTALL_DIR) / "bin" / MONITOR_BINARY
        return str(path) if path.is_file() and os.access(path, os.X_OK) else None

    @classmethod
    def run(cls, args: argparse.Namespace) -> int:
        """Runs one `monitor` subcommand.

        Args:
            args: The parsed arguments.

        Returns:
            int: The exit code (the binary's own for `status`).
        """
        if getattr(args, "monitor_command", None) != STATUS:
            print("usage: ragon-admin monitor status [--json]")
            return 2
        return cls.status(bool(args.json))

    @classmethod
    def status(cls, as_json: bool) -> int:
        """Forwards to `ragon-node-monitor status`.

        Args:
            as_json: Print the monitor's JSON unchanged.

        Returns:
            int: 0, the monitor's exit code, or 1 when it is not built.
        """
        binary = cls.executable()
        if binary is None:
            print(
                f"❌ {MONITOR_BINARY} is not built. Run `ragon-admin codex build` "
                "(or update the node) to install it.",
                file=sys.stderr,
            )
            return NOT_BUILT_EXIT
        command = [binary, STATUS] + (["--json"] if as_json else [])
        try:
            done = subprocess.run(
                command, capture_output=True, text=True, timeout=RUN_TIMEOUT_SECONDS, check=False
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            print(f"❌ {MONITOR_BINARY} did not run: {error}", file=sys.stderr)
            return 1
        sys.stdout.write(done.stdout)
        sys.stderr.write(done.stderr)
        return done.returncode
