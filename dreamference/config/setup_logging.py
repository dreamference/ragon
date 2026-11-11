# ruff: noqa: PLR2004 - field kinds, levels and byte widths are the spec's literals
"""`setup_logging(component)`: called once per entry point; the process's writer is `ragon_log`."""

import os
import secrets
from collections.abc import Mapping
from importlib import metadata
from pathlib import Path

from dreamference.config.log_field import LogField
from dreamference.config.log_file_sink import LogFileSink
from dreamference.config.log_level_spec import LogLevelSpec
from dreamference.config.log_redactor import LogRedactor
from dreamference.config.log_sink import LogSink
from dreamference.config.log_stream_sink import LogStreamSink
from dreamference.config.log_writer import LogWriter

ragon_log = LogWriter()


def state_logs_dir(environ: Mapping[str, str] | None = None) -> Path:
    """Return `~/.local/state/dreamference/logs` (`$XDG_STATE_HOME`), LOGGING §3.2."""
    env = os.environ if environ is None else environ
    base = env.get("XDG_STATE_HOME") or str(Path.home() / ".local" / "state")
    return Path(base) / "dreamference" / "logs"


def log_salt(directory: Path) -> bytes:
    """Return the machine's pseudonym salt, made on first use (mode 0600, never leaves the machine)."""
    path = directory.parent / "log-salt"
    try:
        return bytes.fromhex(path.read_text().strip())
    except (OSError, ValueError):
        pass
    salt = secrets.token_bytes(32)
    try:
        path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as handle:
            handle.write(salt.hex())
    except OSError:
        pass
    return salt


def choose_sink(component: str, directory: Path, environ: Mapping[str, str]) -> LogSink:
    """Pick the one sink of §3.1: journald and Docker take stderr, everything else a file."""
    if environ.get("JOURNAL_STREAM"):
        return LogStreamSink(journald=True)
    if environ.get("RAGON_LOG_SINK") == "stderr" or Path("/.dockerenv").exists():
        return LogStreamSink()
    return LogFileSink(directory, component)


def setup_logging(
    component: str,
    spec: str | None = None,
    sink: LogSink | None = None,
    environ: Mapping[str, str] | None = None,
) -> LogWriter:
    """Configure the process's writer and write its `start` line; never raises.

    Args:
        component: The component name (`ragon-rs/vectors/logging/components.txt`).
        spec: The level spec; the config's `log_level` (and `RAGON_LOG`) when None.
        sink: A sink for tests; chosen by the process kind when None.
        environ: The environment; `os.environ` when None.

    Returns:
        The writer, also available as `ragon_log`.
    """
    env = os.environ if environ is None else environ
    try:
        if spec is None:
            spec = env.get("RAGON_LOG", "").strip()
            if not spec:
                from dreamference.config.dreamference_config import DreamferenceConfig

                spec = DreamferenceConfig().log_level
        directory = state_logs_dir(env)
        redactor = LogRedactor.for_machine(log_salt(directory))
        chosen = sink or choose_sink(component, directory, env)
        ragon_log.configure(component, LogLevelSpec.parse(spec), chosen, redactor)
        try:
            version = metadata.version("dreamference")
        except metadata.PackageNotFoundError:
            version = "unknown"
        ragon_log.info(
            "start",
            LogField.version(version=version),
            LogField.count(pid=os.getpid(), log_format=1),
        )
    except Exception:  # noqa: BLE001, S110 - logging never stops an entry point
        pass
    return ragon_log
