"""`ragon` as a Harbor agent, for Terminal-Bench (specs/DREAMFERENCE_RAGON_ASSISTANT_EVALS.md §10.1).

Harbor loads it by import path (`harbor run -a ragon_agent:RagonAgent`, with this folder on
`PYTHONPATH`). It does what SWE-bench does with `ragon` in an instance's container:

- **setup** uploads SWE-bench's runtime copy of `ragon` (`RAGON_RUNTIME_DIR`, SWE_BENCH §12: copies
  of the binaries whose loader and libraries come along, so they start on an older glibc) to
  `/opt/ragon`;
- **run** starts `ragon exec --json --dangerously-bypass-approvals-and-sandbox` on the instruction,
  in the task's working folder, with the model server named in `RAGON_MODEL_HOST` (the task's
  network policy lets that one address through, D7) and its own `CODEX_HOME`. The container is the
  sandbox, as in SWE-bench. The session's rollout comes back into the trial's logs, and its output
  tokens into Harbor's context.

Nothing here is installed from the network: the agent brings its own binary.
"""

from __future__ import annotations

import os
import shlex
from pathlib import Path
from typing import Any, Final

from harbor.agents.base import BaseAgent
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

REMOTE_RUNTIME: Final[str] = "/opt/ragon"
REMOTE_HOME: Final[str] = "/tmp/ragon-home"
AGENT_LIMIT_S: Final[int] = 30 * 60


def ragon_command(instruction: str, model_host: str) -> str:
    """The shell command that runs one task (exported for the tests)."""
    env = (
        f"CODEX_HOME={REMOTE_HOME} DREAMFERENCE_VLLM_HOST={shlex.quote(model_host)} "
        "DREAMFERENCE_RAGON_PROMPT=default"
    )
    return (
        f"mkdir -p {REMOTE_HOME} && {env} timeout {AGENT_LIMIT_S} {REMOTE_RUNTIME}/bin/ragon exec --json "
        f"--dangerously-bypass-approvals-and-sandbox --skip-git-repo-check {shlex.quote(instruction)} "
        f"> {REMOTE_HOME}/events.jsonl 2>&1"
    )


def output_tokens(sessions: Path) -> int:
    """The output tokens of every turn under a downloaded `sessions` folder."""
    import sys  # noqa: PLC0415

    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "ask_evals"))
    from askevals import rollout  # noqa: PLC0415

    turns = rollout.read_home(sessions.parent)
    return sum(t.output_tokens or 0 for t in turns)


class RagonAgent(BaseAgent):
    """Ragon's `ragon`, the Codex fork, as Terminal-Bench's agent."""

    @staticmethod
    def name() -> str:
        return "ragon-ragon"

    def version(self) -> str | None:
        return os.environ.get("RAGON_VERSION")

    async def setup(self, environment: BaseEnvironment) -> None:
        runtime = os.environ.get("RAGON_RUNTIME_DIR")
        if not runtime or not (Path(runtime) / "bin" / "ragon").exists():
            raise RuntimeError("RAGON_RUNTIME_DIR must name SWE-bench's runtime copy of ragon")
        await environment.exec(command=f"mkdir -p {REMOTE_RUNTIME}", user="root")
        await environment.upload_dir(runtime, REMOTE_RUNTIME)
        await environment.exec(command=f"chmod -R a+rX {REMOTE_RUNTIME}", user="root")

    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        model_host = os.environ.get("RAGON_MODEL_HOST")
        if not model_host:
            raise RuntimeError(
                "RAGON_MODEL_HOST must name the model server as the task sees it"
            )
        result: Any = await environment.exec(
            command=ragon_command(instruction, model_host), timeout_sec=AGENT_LIMIT_S + 60
        )
        logs = Path(self.logs_dir)
        logs.mkdir(parents=True, exist_ok=True)
        try:
            await environment.download_dir(REMOTE_HOME, logs / "ragon-home")
            context.n_output_tokens = output_tokens(logs / "ragon-home" / "sessions")
        except Exception:  # noqa: BLE001 - a missing rollout must not hide the task's own result
            pass
        (logs / "ragon-exit.txt").write_text(f"{getattr(result, 'return_code', None)}\n")
