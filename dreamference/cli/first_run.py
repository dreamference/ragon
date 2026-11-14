"""`ragon-admin init`'s first run: install checks, model download, server start, a `ragon` smoke.

Each step is a command of its own (`ragon-admin host check`, `ragon-admin model download`,
`ragon-admin server start`, `ragon exec`); `init` runs them in order with one progress line per
step, sends their output to one log, and stops at the first that fails, naming the command that
runs it alone (owner Q105, 2026-11-10).
"""

import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
from collections.abc import Callable
from pathlib import Path
from typing import Final

from dreamference.config import DreamferenceConfig

LOG_NAME: Final[str] = "first-run.log"
# What one command line of `ragon-admin` is when run from this same Python, wherever it is installed.
ADMIN_CODE: Final[str] = "import sys; from dreamference.cli import main; sys.exit(main())"
LOOPBACK_NAMES: Final[frozenset[str]] = frozenset({"localhost", "127.0.0.1", "::1"})
SMOKE_PROMPT: Final[str] = "Reply with the single word: ready"
# The first answer of a freshly loaded model, with the launcher's own wait for the server.
SMOKE_TIMEOUT_S: Final[int] = 900
ANSWER_SHOWN_CHARS: Final[int] = 40
SECONDS_PER_MINUTE: Final[int] = 60

StepResult = tuple[bool, str]


class FirstRun:
    """The four steps of a node's first run, each its own command, run in order."""

    @classmethod
    def log_path(cls) -> Path:
        """Where every step's output goes: beside the installer's log.

        Returns:
            Path: `~/.local/state/dreamference/first-run.log`.
        """
        return Path.home() / ".local" / "state" / "dreamference" / LOG_NAME

    @classmethod
    def run(cls, config: DreamferenceConfig, config_path: str | None, model: str | None) -> int:
        """Runs the steps in order, one progress line each, and stops at the first that fails.

        Args:
            config: The resolved configuration (model server URL, model, token).
            config_path: The `--config` given to `init`, handed on to every `ragon-admin` step.
            model: The `--model` given to `init`, or None for the configured one.

        Returns:
            int: 0 when every step passed, else 1.
        """
        log = cls.log_path()
        log.parent.mkdir(parents=True, exist_ok=True)
        model = model or config.model
        model_flag = ["--model", model] if model else []
        steps: list[tuple[str, str, Callable[[], StepResult]]] = [
            (
                "install checks",
                "ragon-admin host check",
                lambda: cls.install_checks(log, config, config_path),
            ),
            (
                "model download",
                f"ragon-admin model download {' '.join(model_flag)}".strip(),
                lambda: cls.model_download(log, config, config_path, model_flag),
            ),
            (
                "server start",
                f"ragon-admin server start {' '.join(model_flag)}".strip(),
                lambda: cls.server_start(log, config, config_path, model_flag),
            ),
            ("first ragon run", f'ragon exec "{SMOKE_PROMPT}"', lambda: cls.smoke(log, config)),
        ]
        print(f"📝 Each step's output goes to {log}")
        for number, (name, command, step) in enumerate(steps, 1):
            print(f"[{number}/{len(steps)}] {name} …", end="", flush=True)
            started = time.monotonic()
            passed, detail = step()
            elapsed = cls.duration(time.monotonic() - started)
            if not passed:
                print(f" ❌ {detail} ({elapsed})")
                print(f"   💡 Run this step alone: {command}; its output: {log}")
                return 1
            print(f" ✅ {detail} ({elapsed})")
        print("✅ This node is ready: run `ragon` in a project.")
        return 0

    @classmethod
    def install_checks(
        cls, log: Path, config: DreamferenceConfig, config_path: str | None
    ) -> StepResult:
        """Checks that docker and `ragon` are installed and the host settings are in place.

        Args:
            log: The first run's log.
            config: The resolved configuration.
            config_path: The `--config` handed on.

        Returns:
            StepResult: Whether the checks passed, and what they found.
        """
        if shutil.which("docker") is None:
            return False, "docker was not found: the model server runs in Docker; install it"
        from dreamference.runner import CodexInstaller
        from dreamference.runner.codex_branded_builder import CodexBrandedBuilder

        if CodexInstaller.get_codex_executable() is None:
            fix = (
                "`ragon-admin codex build`" if CodexBrandedBuilder.has_source() else "the installer"
            )
            return False, f"ragon is not installed: {fix} installs it"
        if cls.admin(log, config, config_path, ["host", "check"]) != 0:
            return False, "host settings missing: `ragon-admin host setup` applies them"
        return True, "docker, ragon and the host settings are in place"

    @classmethod
    def model_download(
        cls, log: Path, config: DreamferenceConfig, config_path: str | None, model_flag: list[str]
    ) -> StepResult:
        """Downloads the model and its drafter (`ragon-admin model download`), a no-op when present.

        Args:
            log: The first run's log.
            config: The resolved configuration.
            config_path: The `--config` handed on.
            model_flag: `["--model", MODEL]`, or empty for the configured model.

        Returns:
            StepResult: Whether the download succeeded.
        """
        if not cls.serves_here(config):
            return True, f"skipped: the model server is {config.vllm_host}, not this machine"
        if cls.admin(log, config, config_path, ["model", "download", *model_flag]) != 0:
            return False, "the download failed"
        return True, "the weights are in the Hugging Face cache"

    @classmethod
    def server_start(
        cls, log: Path, config: DreamferenceConfig, config_path: str | None, model_flag: list[str]
    ) -> StepResult:
        """Starts the model server (`ragon-admin server start`) unless it already serves the model.

        Args:
            log: The first run's log.
            config: The resolved configuration.
            config_path: The `--config` handed on.
            model_flag: `["--model", MODEL]`, or empty for the configured model.

        Returns:
            StepResult: Whether the server answers.
        """
        if not cls.serves_here(config):
            return True, f"skipped: the model server is {config.vllm_host}, not this machine"
        from dreamference.hardware import AdapterVerifier, model_key_for_served_id

        wanted = model_flag[1] if model_flag else config.model
        served = AdapterVerifier.served_id(config.vllm_host)
        if served and model_key_for_served_id(served, wanted) == wanted:
            # A second `init` must not restart a server that already answers.
            return True, f"already serving {wanted}"
        if cls.admin(log, config, config_path, ["server", "start", *model_flag]) != 0:
            return False, "the model server did not start"
        return True, f"serving {wanted}"

    @classmethod
    def smoke(cls, log: Path, config: DreamferenceConfig) -> StepResult:
        """Runs one `ragon exec` turn against the model server and checks it answers.

        Args:
            log: The first run's log.
            config: The resolved configuration.

        Returns:
            StepResult: Whether `ragon` answered, and the start of its answer.
        """
        from dreamference.runner import CodexInstaller

        ragon = CodexInstaller.get_codex_executable()
        if ragon is None:
            return False, "ragon is not installed"
        # An unattended caller names the server: a node browse is a multicast DNS query.
        env = {**os.environ, "DREAMFERENCE_VLLM_HOST": config.vllm_host}
        with tempfile.TemporaryDirectory(prefix="ragon-first-run-") as folder:
            answer_path = Path(folder) / "answer.txt"
            command = [
                ragon,
                "exec",
                "--skip-git-repo-check",
                "-s",
                "read-only",
                "-C",
                folder,
                "-o",
                str(answer_path),
                SMOKE_PROMPT,
            ]
            try:
                code = cls.logged(
                    log, command, env, SMOKE_TIMEOUT_S, f'ragon exec "{SMOKE_PROMPT}"'
                )
            except subprocess.TimeoutExpired:
                return False, f"ragon did not answer within {SMOKE_TIMEOUT_S} s"
            answer = answer_path.read_text(encoding="utf-8").strip() if answer_path.exists() else ""
        if code != 0 or not answer:
            return False, "ragon gave no answer"
        return True, f"ragon answered: {answer[:ANSWER_SHOWN_CHARS]}"

    @classmethod
    def admin(
        cls, log: Path, config: DreamferenceConfig, config_path: str | None, arguments: list[str]
    ) -> int:
        """Runs one `ragon-admin` command from this same Python, its output into the log.

        Args:
            log: The first run's log.
            config: The resolved configuration, whose token reaches the step by environment.
            config_path: The `--config` handed on, or None.
            arguments: The subcommand and its arguments.

        Returns:
            int: The command's exit code.
        """
        command = [sys.executable, "-c", ADMIN_CODE]
        if config_path:
            command += ["--config", config_path]
        env = dict(os.environ)
        if config.hf_token:
            # By environment, never on a command line every process can read.
            env["HF_TOKEN"] = config.hf_token
        return cls.logged(
            log, [*command, *arguments], env, None, f"ragon-admin {' '.join(arguments)}"
        )

    @classmethod
    def logged(
        cls, log: Path, command: list[str], env: dict[str, str], timeout: int | None, shown: str
    ) -> int:
        """Runs a command with no terminal on its input, appending its output to the log.

        Args:
            log: The first run's log.
            command: The command line.
            env: Its environment.
            timeout: Seconds before it is stopped, or None.
            shown: The command as the log names it.

        Returns:
            int: The command's exit code.
        """
        with log.open("a", encoding="utf-8") as out:
            out.write(f"\n$ {shown}\n")
            out.flush()
            return subprocess.run(
                command,
                stdin=subprocess.DEVNULL,
                stdout=out,
                stderr=subprocess.STDOUT,
                env=env,
                timeout=timeout,
                check=False,
            ).returncode

    @classmethod
    def serves_here(cls, config: DreamferenceConfig) -> bool:
        """Whether the configured model server is this machine's own.

        Args:
            config: The resolved configuration.

        Returns:
            bool: True for a loopback URL.
        """
        return urllib.parse.urlparse(config.vllm_host).hostname in LOOPBACK_NAMES

    @classmethod
    def duration(cls, seconds: float) -> str:
        """A step's duration for its progress line.

        Args:
            seconds: The elapsed time.

        Returns:
            str: `12 s` or `3 min 4 s`.
        """
        minutes, rest = divmod(round(seconds), SECONDS_PER_MINUTE)
        return f"{minutes} min {rest} s" if minutes else f"{rest} s"
