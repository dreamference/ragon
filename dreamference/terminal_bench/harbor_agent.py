"""`ragon` as a Harbor agent.

Run with `harbor run --agent-import-path dreamference.terminal_bench.harbor_agent:RagonAgent`.

The only module of the package that imports `harbor` (not a dependency of this package: run Harbor
from its own environment with this checkout on `PYTHONPATH`). Agent options (`--ak name=value`):

- `model_url` (required): the model gate, `http://<node>:<port>`.
- `runtime_dir`: the x86-64 runtime (`tb_runtime.build`), uploaded into each container unless it is
  already mounted at `/opt/ragon` (`--mounts`, read-only: much cheaper than ~460 MB per trial).
- `service_key_file`: the serving node's key file; `RAGON_API_KEY` in Harbor's environment wins.
  The key is read here, at run time, and never passed as an option: Harbor writes the options into
  the job's `config.json`.
- `task_context` (49152), `digest_min_lines` (unset: Ragon Digest off).

Options are popped before Harbor's constructor sees them, so Harbor's own checks are untouched.
"""

import shlex
from pathlib import Path

from harbor.agents.installed.base import BaseInstalledAgent, with_prompt_template
from harbor.environments.base import BaseEnvironment
from harbor.models.agent.context import AgentContext

from dreamference.terminal_bench import tb_agent_spec, tb_stats
from dreamference.terminal_bench.tb_runtime import CONTAINER_MOUNT


class RagonAgent(BaseInstalledAgent):
    """Runs one `ragon exec` per task inside the task's container."""

    def __init__(self, logs_dir: Path, **kwargs: object) -> None:
        """Takes this agent's options out of `kwargs`, then builds Harbor's agent."""
        model_url = kwargs.pop("model_url", None)
        if not model_url:
            raise ValueError("RagonAgent needs --ak model_url=http://<node>:<port>")
        self.model_url = str(model_url)
        runtime = kwargs.pop("runtime_dir", None)
        self.runtime_dir = Path(str(runtime)).expanduser() if runtime else None
        key_file = kwargs.pop("service_key_file", None)
        self.service_key_file = Path(str(key_file)) if key_file else None
        self.task_context = int(str(kwargs.pop("task_context", tb_agent_spec.DEFAULT_TASK_CONTEXT)))
        min_lines = kwargs.pop("digest_min_lines", None)
        self.digest_min_lines = int(str(min_lines)) if min_lines not in {None, "", "0", 0} else None
        super().__init__(logs_dir, **kwargs)

    @staticmethod
    def name() -> str:
        """The agent's name in Harbor's results."""
        return "ragon"

    def get_version_command(self) -> str | None:
        """How Harbor reads the installed version."""
        return f"{CONTAINER_MOUNT}/bin/ragon --version"

    def parse_version(self, stdout: str) -> str:
        """`ragon 0.9.8+1` -> `0.9.8+1`."""
        return stdout.strip().removeprefix("ragon").strip()

    async def install(self, environment: BaseEnvironment) -> None:
        """Puts the runtime at `/opt/ragon` unless a mount already did."""
        mounted = await environment.exec(command=tb_agent_spec.installed_check(), user="root")
        if mounted.return_code == 0:
            return
        if self.runtime_dir is None or not (self.runtime_dir / "bin" / "ragon").exists():
            raise RuntimeError("no runtime at /opt/ragon and no --ak runtime_dir=<dir> to upload")
        await environment.upload_dir(self.runtime_dir, CONTAINER_MOUNT)
        await self.exec_as_root(environment, f"chmod -R a+rX {CONTAINER_MOUNT}")

    @with_prompt_template
    async def run(
        self, instruction: str, environment: BaseEnvironment, context: AgentContext
    ) -> None:
        """Runs the task; the event log is written under the trial's agent directory as it goes."""
        env = tb_agent_spec.environment(
            self.model_url,
            tb_agent_spec.service_key(self.service_key_file),
            digest=self.digest_min_lines is not None,
        )
        command = tb_agent_spec.exec_command(instruction, self.task_context)
        config = tb_agent_spec.config_toml(self.digest_min_lines)
        if config:
            command = (
                f"mkdir -p {tb_agent_spec.CODEX_HOME} && printf %s {shlex.quote(config)} "
                f"> {tb_agent_spec.RAGON_CONFIG_PATH} && {command}"
            )
        await self.exec_as_agent(environment, command, env=env)

    def populate_context_post_run(self, context: AgentContext) -> None:
        """Tokens, command-output counts and what the model read into the trial's result."""
        stats = tb_stats.from_log(self.logs_dir / tb_agent_spec.EVENT_LOG_NAME)
        stats.update(tb_stats.from_rollouts(self.logs_dir / tb_agent_spec.CODEX_HOME_NAME))
        tb_stats.merge_rollout_tokens(stats)
        context.n_input_tokens = stats["input_tokens"]
        context.n_cache_tokens = stats["cached_input_tokens"]
        context.n_output_tokens = stats["output_tokens"]
        context.metadata = {**(context.metadata or {}), "ragon": stats}
