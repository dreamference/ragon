"""Codex CLI Provisioning & Detection for Dreamference.

This module provides the CodexInstaller class, which resolves the Codex executable the runner
launches: the Ragon-branded build compiled from the `codex/` submodule by CodexBrandedBuilder.
An upstream `codex` on PATH is deliberately never used, not even as a fallback -- a silent fallback
would put the unbranded agent back on screen with nothing to say it had happened.
"""

import os
import pathlib
from typing import Final

from dreamference.runner.codex_branded_builder import CodexBrandedBuilder

# The adapter a `ragon` session starts with (ragon-rs/src/adapter.rs). Unattended callers (Night
# Shift, SWE-bench, the egress audit, Codex's test runner) set it to the empty string, which is the
# base, so a user's `ragon_adapter` default never reaches their runs; an arm names its adapter
# here explicitly (specs/DREAMFERENCE_RAGON_ADAPTERS.md §10.4).
ADAPTER_ENV: Final[str] = "DREAMFERENCE_RAGON_ADAPTER"

# Where a new interactive `ragon` session edits (ragon-rs/src/worktree.rs): `off` keeps an unattended
# TUI session (the egress audit's `--tui`) in its own folder whatever the user's shell says.
WORKTREE_ENV: Final[str] = "DREAMFERENCE_RAGON_WORKTREE"


class CodexInstaller:
    """Installer and verifier class for the Ragon-branded Codex (`ragon-codex`)."""

    @classmethod
    def get_codex_executable(cls) -> str | None:
        """Locates the branded `ragon-codex` executable.

        Returns:
            Optional[str]: Absolute path to the executable, or None if it has not been built.
        """
        path = CodexBrandedBuilder.executable_path()
        if pathlib.Path(path).is_file() and os.access(path, os.X_OK):
            return path
        return None

    @classmethod
    def home_dir(cls) -> str:
        """Returns the configuration folder `ragon` uses: `$CODEX_HOME` if set, else `~/.ragon`.

        Not `~/.codex`: that is upstream Codex's folder, where a ChatGPT login may live, and the
        launcher (`ragon-rs/src/home.rs`) keeps Ragon out of it. This mirrors its resolution so
        the Python side reads the same session logs.

        Returns:
            str: Absolute path of the folder, whether or not it exists yet.
        """
        return os.environ.get("CODEX_HOME") or os.path.expanduser("~/.ragon")

    @classmethod
    def is_installed(cls) -> bool:
        """Checks that the branded build and the web commands its prompt names are installed and
        match the current submodule, patches and sources.

        Returns:
            bool: True if no build is needed.
        """
        return CodexBrandedBuilder.is_current() and CodexBrandedBuilder.web_tools_are_current()

    @classmethod
    def install_if_missing(cls) -> bool:
        """Builds the branded Codex from the submodule if it is missing or out of date.

        Returns:
            bool: True if an up-to-date `ragon-codex` is installed afterwards.
        """
        if cls.is_installed():
            print("✅ ragon is already built.")
            return True
        return CodexBrandedBuilder.build()
