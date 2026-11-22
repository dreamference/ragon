"""The environment and command line of one `ragon exec` run in a Terminal-Bench task container.

Plain strings and dicts, no Harbor, so they are tested without it. They follow the SWE-bench
harness (`SweBenchInstanceRun._start_container`, `_exec`) with three differences: the task's own
working directory instead of `/testbed`, the container's own `HOME` (the tasks' tests may look
there), and `CODEX_HOME` and the event log under Harbor's agent log directory, which is a host
directory, so the rollouts and the log survive a timeout.

Only standard-library imports: Harbor's interpreter imports this module and need not have this
package's dependencies (which is why the `NO_PROXY` merge of `ProxyBypass` is repeated here).
"""

import os
import shlex
from pathlib import Path
from typing import Final
from urllib.parse import urlsplit

from dreamference.terminal_bench.tb_runtime import CONTAINER_MOUNT

# Harbor mounts the trial's agent directory here (`EnvironmentPaths.agent_dir`).
AGENT_LOG_DIR: Final[str] = "/logs/agent"
CODEX_HOME_NAME: Final[str] = "codex-home"
CODEX_HOME: Final[str] = f"{AGENT_LOG_DIR}/{CODEX_HOME_NAME}"
EVENT_LOG_NAME: Final[str] = "ragon.jsonl"
STDERR_NAME: Final[str] = "ragon.stderr"
LAST_MESSAGE_NAME: Final[str] = "last.txt"

# The context at which a task's session compacts, as SWE-bench (`DEFAULT_TASK_CONTEXT`).
DEFAULT_TASK_CONTEXT: Final[int] = 49_152

# The service key file of a node (`ServiceKey`), and the variable `ragon` reads it from.
SERVICE_KEY_FILE: Final[Path] = Path("~/.config/dreamference/service-key")
API_KEY_ENV: Final[str] = "RAGON_API_KEY"

# A URL with a scheme or `//` names a host; a bare `host:port` gets the `//` added.
NETLOC_MARK: Final[str] = "//"

LOOPBACK_HOSTS: Final[tuple[str, ...]] = ("localhost", "127.0.0.1", "::1")


def service_key(path: Path | None = None) -> str | None:
    """The key to send to the model gate: `RAGON_API_KEY` if set, else the key file's content.

    Args:
        path: The key file; the node's own when None.

    Returns:
        Optional[str]: The key, or None when there is none.
    """
    key = os.environ.get(API_KEY_ENV, "").strip()
    if key:
        return key
    try:
        key = (path or SERVICE_KEY_FILE).expanduser().read_text().strip()
    except OSError:
        return None
    return key or None


def no_proxy(existing: str | None, model_url: str) -> str:
    """`NO_PROXY`: the existing entries, loopback and the model server's host, each once."""
    host = urlsplit(model_url if NETLOC_MARK in model_url else f"{NETLOC_MARK}{model_url}").hostname
    entries: list[str] = []
    for entry in [*(existing or "").split(","), *LOOPBACK_HOSTS, host or ""]:
        entry = entry.strip()
        if entry and entry.lower() not in (seen.lower() for seen in entries):
            entries.append(entry)
    return ",".join(entries)


def environment(
    model_url: str,
    api_key: str | None,
    existing_no_proxy: str | None = None,
    digest: bool = False,
) -> dict[str, str]:
    """The variables `ragon exec` runs with, on top of the container's own.

    Args:
        model_url: The model gate, e.g. `http://<node>:8000` (`DREAMFERENCE_VLLM_HOST`: an
            unattended caller never browses for a node).
        api_key: The serving node's service key, or None.
        existing_no_proxy: The host's `NO_PROXY`, kept.
        digest: Ragon Digest on (`DREAMFERENCE_DIGEST_INSIDE`); its threshold is in the config file
            (`config_toml`).

    Returns:
        Dict[str, str]: The variables.
    """
    env = {
        "DREAMFERENCE_VLLM_HOST": model_url,
        "CODEX_HOME": CODEX_HOME,
        # The base model unless a run names an adapter: never a user's default.
        "DREAMFERENCE_RAGON_ADAPTER": "",
        "RAGON_NIGHT_RUN": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "DREAMFERENCE_DIGEST_INSIDE": "1" if digest else "0",
    }
    if api_key:
        env[API_KEY_ENV] = api_key
    merged = no_proxy(existing_no_proxy, model_url)
    env["NO_PROXY"] = env["no_proxy"] = merged
    return env


def config_toml(digest_min_lines: int | None) -> str:
    """The `config.toml` written into `CODEX_HOME` before the run: the digest threshold, if any."""
    if not digest_min_lines:
        return ""
    return f"[digest]\nmin_lines = {int(digest_min_lines)}\n"


def exec_command(instruction: str, task_context: int = DEFAULT_TASK_CONTEXT) -> str:
    """The shell command that runs the task, from the container's working directory.

    The events go to the log as they come (`tee` is not used: the exit status is `ragon`'s), the
    stdin is closed (`ragon exec` would wait for more input), and the run is not sandboxed: the
    container is the sandbox, as in SWE-bench.

    Args:
        instruction: The task's instruction.
        task_context: The context at which the session compacts.

    Returns:
        str: One shell command line.
    """
    argv = [
        f"{CONTAINER_MOUNT}/bin/ragon",
        "exec",
        "--json",
        "-o",
        f"{AGENT_LOG_DIR}/{LAST_MESSAGE_NAME}",
        "--dangerously-bypass-approvals-and-sandbox",
        "--skip-git-repo-check",
        "-c",
        f"model_auto_compact_token_limit={int(task_context)}",
        instruction,
    ]
    return (
        f'mkdir -p {CODEX_HOME} && {shlex.join(argv[:-1])} -C "$PWD" '
        f"{shlex.quote(argv[-1])} < /dev/null > {AGENT_LOG_DIR}/{EVENT_LOG_NAME} "
        f"2> {AGENT_LOG_DIR}/{STDERR_NAME}"
    )


def installed_check() -> str:
    """The command that succeeds when the runtime is already in the container (a bind mount)."""
    return f"test -x {CONTAINER_MOUNT}/bin/ragon"
