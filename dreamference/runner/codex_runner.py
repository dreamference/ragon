"""Codex Session Runner for Dreamference.

This module provides the CodexRunner class, which `ragon-admin run` uses when the configured
agent is Codex. It builds `ragon` if needed and hands over to it.

It no longer sets up the session. Waiting for the model server, writing the model catalog and
`~/.ragon/config.toml`, the system prompt with its web-access section, and the local-model options
all happen inside `ragon` itself, in the launcher crate `ragon-rs/`. Keeping a Python copy of that
logic here would give the two entry points two setups to keep in step.
"""

import os
import subprocess
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Final

from dreamference.config import DreamferenceConfig
from dreamference.runner.codex_installer import CodexInstaller

# The variable the launcher reads for the compatibility mode (ragon-rs/src/compat.rs).
COMPAT_ENV: Final[str] = "DREAMFERENCE_RAGON_COMPAT"


def _say(text: str) -> None:
    """Writes one line for the person launching ragon."""
    sys.stdout.write(text + "\n")


class CodexRunner:
    """Runner class that launches the Rust `ragon` agent."""

    def __init__(self, config: DreamferenceConfig | None = None):
        """Initializes CodexRunner with a configuration instance.

        Args:
            config (Optional[DreamferenceConfig]): Configuration instance (defaults to DreamferenceConfig()).
        """
        self.config: DreamferenceConfig = config or DreamferenceConfig()

    def run_session(
        self,
        prompt: str | None = None,
        debug: bool = False,
        agent_args: Sequence[str] | None = None,
        compat: str | None = None,
    ) -> int:
        """Builds `ragon` if it is missing or stale, then runs it.

        Args:
            prompt (Optional[str]): Optional initial prompt, passed as Codex's positional PROMPT.
            debug (bool): Enable verbose debug logging.
            agent_args (Optional[Sequence[str]]): Codex command-line arguments forwarded verbatim.
            compat (str | None): The compatibility mode for this session (`claude` or `off`, which
                the parser checks), passed in the launcher's variable; None leaves the launcher's
                own setting.

        Returns:
            int: The agent's exit code.
        """
        if not CodexInstaller.install_if_missing():
            return 1
        ragon = CodexInstaller.get_codex_executable()
        if not ragon:
            _say("❌ `ragon` is not built.")
            _say("💡 Build it with: `ragon-admin codex build`")
            return 1

        cmd: list[str] = [ragon, *(agent_args or [])]
        # Codex takes the initial prompt as a positional argument; it has no --message option.
        if prompt:
            cmd.append(prompt)

        env = os.environ.copy()
        # The launcher reads the vLLM URL from the environment first, so a host given to
        # ragon-admin on the command line or in a non-default config file reaches it.
        env["DREAMFERENCE_VLLM_HOST"] = self.config.vllm_host
        # Same for the Gmail prompt opt-out, which the launcher reads the same way.
        env["DREAMFERENCE_RAGON_GMAIL"] = "true" if self.config.ragon_gmail else "false"
        if compat is not None:
            env[COMPAT_ENV] = compat
        if debug:
            # Codex has no --debug flag; verbosity is RUST_LOG. The TUI owns the terminal, so its
            # log goes to logs_2.sqlite in ragon's home folder rather than to a file.
            env["RUST_LOG"] = os.getenv(
                "RUST_LOG", "codex_mcp=trace,codex_core=debug,codex_app_server=debug,info"
            )
            _say(f"🐞 Debug logging on (RUST_LOG={env['RUST_LOG']})")
            _say(f"   TUI logs go to: {Path(CodexInstaller.home_dir()) / 'logs_2.sqlite'}")
            _say("   Read MCP lifecycle with: ragon-admin logs mcp")

        try:
            return subprocess.call(cmd, env=env)
        except OSError as error:
            _say(f"❌ Failed to run ragon: {error}")
            return 1
