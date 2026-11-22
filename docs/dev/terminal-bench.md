# Terminal-Bench with Ragon

[Terminal-Bench](https://www.tbench.ai) tasks are run by [Harbor](https://pypi.org/project/harbor/)
(`harbor run`): each task is a prebuilt amd64 image, an instruction, a time limit and tests that
write a reward. `dreamference/terminal_bench` is the agent side: Harbor starts the task's
container, the agent runs one `ragon exec` inside it against a model gate on the LAN, Harbor grades.

## Layout

- The task containers run on an x86-64 Linux box with Docker (the images are amd64 only; under
  qemu on a GB10 a task's own builds and tests run several times slower and the time limits are
  part of the score).
- The model is served by one GB10 node; the agent reaches it at `--ak model_url=http://<node>:<port>`
  with the node's service key.

## One-time setup on the box that runs the containers

1. Harbor in its own environment (not this package's), e.g. `uv venv <dir>/.venv && uv pip
   install --python <dir>/.venv/bin/python harbor`.
2. The x86-64 runtime (`tb_runtime.build`): the release's `ragon-x86_64-unknown-linux-gnu.gz` and
   `codex-code-mode-host-x86_64-unknown-linux-gnu.gz` (checked with `tb_runtime.check_sums`
   against the release's sums file) and the Ubuntu 24.04 amd64 `libc6` and `libgcc-s1` packages:

   ```python
   from pathlib import Path
   from dreamference.terminal_bench import tb_runtime
   tb_runtime.build(Path("<dir>/runtime-x86_64"), Path("<release assets>"),
                    [Path("<libc6_*.deb>"), Path("<libgcc-s1_*.deb>")])
   ```

   `<dir>/runtime-x86_64/bin/ragon --version` must print the release's version.
3. A Docker Hub login (`docker login`): the task images are on Docker Hub and an anonymous pull
   limit stops a full run.

## A run

```sh
PYTHONPATH=<checkout> RAGON_API_KEY="$(cat <key file>)" <dir>/.venv/bin/harbor run \
  -d terminal-bench/terminal-bench-2-1 \
  --agent-import-path dreamference.terminal_bench.harbor_agent:RagonAgent \
  --ak model_url=http://<node>:<port> \
  --mounts '[{"type":"bind","source":"<dir>/runtime-x86_64","target":"/opt/ragon","read_only":true}]' \
  -n 4 -k 1 -o <dir>/jobs -i <task> -i <task>
```

- `--mounts` puts the runtime in every container read-only; without it the agent uploads
  `--ak runtime_dir=<dir>/runtime-x86_64` (~460 MB) into each container.
- The key is read at run time from `RAGON_API_KEY` or `--ak service_key_file=<path>`, never passed
  as an option: Harbor writes the options into the job's `config.json`.
- `--ak digest_min_lines=60` turns Ragon Digest on with that threshold (off when unset).
- `--ak task_context=<tokens>` is the context at which a session compacts (49152, as SWE-bench).
- A leaderboard-comparable score uses the dataset's own limits: `-k 5`, no timeout or resource
  overrides.

## What a trial leaves

Under `<jobs>/<job>/<trial>/agent/`: `ragon.jsonl` (the `ragon exec --json` events),
`ragon.stderr`, `last.txt` (the final message) and `codex-home/` (the session's rollouts and,
with Digest on, the raw outputs it cut). The trial's `result.json` has the tokens and, under
`agent_result.metadata.ragon`, the counts of `tb_stats`: commands, output lines and bytes,
outputs over 60 and 200 lines, and what the model read (`seen_lines`, `digests`). The event log's
`aggregated_output` is always the whole output; with Digest on, `seen_lines` is what was left.
