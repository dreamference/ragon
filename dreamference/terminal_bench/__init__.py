"""Terminal-Bench with `ragon` as the agent.

Terminal-Bench (https://www.tbench.ai) is run by the Harbor framework (`harbor run`), which starts
each task's prebuilt image, hands the task to an agent inside it and grades the result with the
task's own tests. This package is the agent side only:

- `tb_runtime`: an x86-64 copy of `ragon` that starts inside any task image (the images are
  amd64), built from the release's Linux x86-64 binaries the way `SweBenchRuntime` relocates the
  host's binary.
- `tb_agent_spec`: the environment and the `ragon exec` command line for one task, plain strings.
- `tb_stats`: tokens and command-output lengths read from the `ragon exec --json` log, and what
  the model read (Ragon Digest's cut) from the session's rollouts.
- `harbor_agent`: the Harbor agent class (`--agent-import-path
  dreamference.terminal_bench.harbor_agent:RagonAgent`); the only module that imports `harbor`,
  which is not a dependency of this package.

How to run it: docs/dev/terminal-bench.md.
"""

# The Harbor-free modules (`harbor_agent` needs `harbor`).
from dreamference.terminal_bench import tb_agent_spec, tb_runtime, tb_stats

__all__ = ["tb_agent_spec", "tb_runtime", "tb_stats"]
