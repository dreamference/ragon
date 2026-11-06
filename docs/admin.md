# `ragon-admin` reference

`ragon-admin` runs everything around the agent: the model server, the sidecars (search, images, voice, Google), the desktop app,
models, and the mail commands the agent calls (`gmail`). The agent itself is `ragon`, and its web
commands are programs of their own, `ragon-search` and `ragon-fetch`; see [Terminal agent](ragon.md).

!!! note "Generated from the CLI"
    This page is generated from `ragon-admin`'s own argument parser by
    `scripts/gen_admin_reference.py`. Run `ragon-admin <command> --help` for the same text locally.


## Global options

| Option | Description |
|---|---|
| `--config` | Path to custom Ragon config file (.toml, .yaml or .json) |
| `--agent` | Select primary AI agent runner (default: codex) One of: `codex`, `cline`, `continue`, `openhands`. |
| `--hf-token` | HuggingFace API access token (or set via HF_TOKEN env var) |

## Commands

### `ragon-admin init`

Initialize .dreamference project workspace and agent configs.

| Option | Description |
|---|---|
| `--model` | Model name served on vLLM GB10 endpoint. |
| `--vllm-host` | vLLM server URL. |
| `--draft-model` | Speculative decoding draft model name. |
| `--agent` | Primary AI agent runner. One of: `codex`, `cline`, `continue`, `openhands`. |
| `--hf-token` | HuggingFace API access token. |

### `ragon-admin run`

Run an autonomous coding task.

| Option | Description |
|---|---|
| `prompt` | Task prompt for AI agent. |
| `--model` | Model name served on vLLM GB10 endpoint. |
| `--draft-model` | Speculative decoding draft model name. |
| `--agent` | Primary AI agent runner. One of: `codex`, `cline`, `continue`, `openhands`. |
| `--hf-token` | HuggingFace API access token. |
| `--debug` | Enable verbose debug output. |
| `--compat` | The compatibility mode for this session (ragon only; ragon --compat) One of: `claude`, `off`. |

### `ragon-admin status`

Display local GB10 hardware & agent connection status.

### `ragon-admin index`

Index codebase AST & TF-IDF vector context.

| Option | Description |
|---|---|
| `--dir` | Directory to index. |
| `--force` | Force reindexing. |

### `ragon-admin mcp`

Run stdio MCP server for JetBrains & VS Code extensions.

### `ragon-admin model`

Model operations.

#### `ragon-admin model list`

List available model names and HuggingFace repos.

#### `ragon-admin model download`

Pre-download LLM & draft model weights into local HuggingFace cache.

| Option | Description |
|---|---|
| `--model` | Specific model to pre-download. |
| `--all` | Pre-download all qualified GB10 models. |
| `--tensorize`, `--no-tensorize` | Auto-convert model to tensorize format after download (default: False) |

### `ragon-admin main-model`

Main model operations.

#### `ragon-admin main-model set`

Set the main model.

| Option | Description |
|---|---|
| `model_name` | Name of the model to set as main. |

#### `ragon-admin main-model inspect`

Inspect the currently running main model by running sample prompts.

| Option | Description |
|---|---|
| `--deep` | Also report quantization map, KV geometry, sampling provenance, graph coverage and per-workload speculative acceptance (sends extra requests; slower) |

### `ragon-admin clear-tensorize-cache`

Clear local tensorizer model cache only.

### `ragon-admin clear`

Clear operations.

#### `ragon-admin clear model-cache`

Clear local HuggingFace and tensorizer model caches.

#### `ragon-admin clear tensorize-cache`

Clear local tensorizer model cache only.

### `ragon-admin endpoints`

Print the model server's endpoints (the standard /v1 API) and credentials.

### `ragon-admin server`

Manage the model server container (start, stop, remove).

#### `ragon-admin server start`

Launch the local model server optimized for GB10 unified memory.

| Option | Description |
|---|---|
| `--model` | Model name to serve (default: the configured main model; examples: qwen3.8-27b-nvfp4-dflash2, llama-3.3-70b) |
| `--port` | Port for the model server's /v1 API. |
| `--engine` | Engine to serve the model with (default: server_engine in the config, else the model's own: ragon-serve for the default model since 0.9.0; SGLang with `sglang`). One of: `ragon`, `sglang`. |
| `--no-gate` | Serve the port from the engine itself, without the model gate that lets a SWE-bench run refuse other requests. |
| `--quantization` | Quantization method (int8, fp8, awq) |
| `--draft-model` | Speculative decoding draft model (e.g. qwen2.5-coder-1.5b) |
| `--num-speculative-tokens` | Number of speculative tokens to propose. |
| `--hf-token` | HuggingFace API access token. |
| `--num-scheduler-steps` | Multi-step scheduling iterations per step. |
| `--attention-backend` | Attention backend (FLASHINFER, FLASH_ATTN, auto) |
| `--kv-cache-dtype` | KV cache precision (auto, fp8) |
| `--api-key` | API key for the model server's /v1 API (optional; not set by default) |
| `--enable-auto-tool-choice` | Enable automatic tool choice for function calling (default: enabled) |
| `--tool-call-parser` | Tool call parser name (default: from the model's registry recipe, e.g. hermes, qwen3_xml) |
| `--reasoning-parser` | Reasoning-channel parser for models that emit separate thinking output (e.g. qwen3) |
| `--moe-backend` | Mixture-of-experts kernel backend (e.g. marlin, flashinfer-b12x); GB10 requires an SM121-safe choice. |
| `--max-num-batched-tokens` | Max tokens per batch for chunked prefill (GB10 optimization) |
| `--guided-decoding-backend` | Structured-outputs backend for deterministic JSON/tool calls (auto, xgrammar, guidance). Unset leaves vLLM's own default. |
| `--tensorize`, `--no-tensorize` | Save and load model in tensorize (.tensors) format (default: False) |
| `--docker-image` | Docker image for vLLM. Unset uses the model's own docker_image recipe entry, then the pinned default. |

#### `ragon-admin server stop`

Stop the running model server.

| Option | Description |
|---|---|
| `--port` | Port of the server to stop. |

#### `ragon-admin server remove`

Remove the model server's container.

| Option | Description |
|---|---|
| `--port` | Port of the server to remove. |

#### `ragon-admin server logs`

Tail the model server Docker container logs.

| Option | Description |
|---|---|
| `--port` | Port of the server to tail logs for. |

### `ragon-admin logs`

Tail the model server Docker container logs.

| Option | Description |
|---|---|
| `target` | server: model server container logs. mcp: the agent's MCP server lifecycle, read from ~/.ragon/logs_2.sqlite, or $CODEX_HOME (the TUI logs there, not to a file) One of: `server`, `mcp`. |
| `--port` | Port of the server to tail logs for. |

### `ragon-admin codex`

Build ragon and manage its app-server daemon.

#### `ragon-admin codex build`

Build ragon from the pinned upstream source in codex/ and the patches in codex-patches/.

| Option | Description |
|---|---|
| `--force` | Rebuild even if the installed build is current. |
| `--no-audit` | Do not trace the new build's network use afterwards (`ragon-admin audit egress`) |

#### `ragon-admin codex start`

Start ragon's app-server daemon in the background.

#### `ragon-admin codex stop`

Stop ragon's app-server daemon.

#### `ragon-admin codex test`

Run the upstream test suite on ragon's patched tree, except the tests in codex-tests/ragon-skips.toml.

| Option | Description |
|---|---|
| `-E`, `--filter` | nextest filterset to narrow the run to. |
| `--test-threads` | Tests run at once (default 8) |
| `--jobs` | Parallel compile jobs (default 6) |
| `--memory-max` | Memory the run may use (default 24G) |
| `--accept-snapshots` | Rewrite the selected TUI snapshots and keep those that differ from upstream's by the name alone. |

### `ragon-admin code`

Install the pinned tools of ragon-code's code index.

#### `ragon-admin code setup`

Install codebase-memory-mcp, the scip CLI and the language indexers, each checked against its pin.

### `ragon-admin host`

Check or apply the host settings a model load and ragon's sandbox need (swap, sysctls, earlyoom, sysstat, bubblewrap, Wi-Fi power saving).

#### `ragon-admin host check`

Show what `server start` would refuse over, and Wi-Fi power saving (reported, but it blocks no model load), changing nothing.

#### `ragon-admin host setup`

Apply the settings; each command is printed first and sudo asks for your password.

| Option | Description |
|---|---|
| `--yes` | Never wait for input: run through `sudo -n` (root, NOPASSWD or a fresh sudo timestamp), with no terminal needed; change nothing if sudo would ask. |

### `ragon-admin night`

Run the Night Shift queue overnight (tasks are queued with /night add).

#### `ragon-admin night enable`

Install the systemd user timer that runs the queue every night.

| Option | Description |
|---|---|
| `--window` | HH:MM-HH:MM (default: [night] window, 01:00-07:00) |

#### `ragon-admin night disable`

Remove the Night Shift timer.

#### `ragon-admin night status`

Show the timer, the window and the queue of every repository.

#### `ragon-admin night run`

Work through the queue now, until the window ends.

| Option | Description |
|---|---|
| `--until` | HH:MM to stop at (default: the end of the window) |
| `--minutes` | Run for this many minutes instead. |
| `--idle-minutes` | Minutes the model must have been idle first (default 10) |
| `--ignore-open-sessions` | Do not wait for open ragon sessions to close (for testing; their requests still pause the run) |

#### `ragon-admin night every`

Repeating tasks, queued by each night's run when due.

#### `ragon-admin night every add`

Repeat a task in this repository every night, every week, on chosen weekdays, every N nights or on a day of the month (`night every --<cadence>` is short for it).

| Option | Description |
|---|---|
| `--daily` | Every night. |
| `--weekly` | Every week, that day's night. One of: `mon`, `tue`, `wed`, `thu`, `fri`, `sat`, `sun`. |
| `--days` | Those weekdays' nights, comma-separated: mon,wed,fri. |
| `--every` | Every N nights (1 to 365), counted from the night it is set. |
| `--monthly` | That day of each month, 1 to 31; in a shorter month, the last. |
| `--test` | The test command each run uses. |
| `task` | What to do, in words. |

#### `ragon-admin night every list`

The repeating tasks, as JSON lines.

#### `ragon-admin night every drop`

Stop repeating a task (what it already queued stays).

| Option | Description |
|---|---|
| `id` | The repeating task's id (r...) |

#### `ragon-admin night archive`

Move finished runs' records, transcripts and reports older than some weeks to night/archive/ (branches stay).

| Option | Description |
|---|---|
| `--weeks` | How old a run must be, in weeks (default 4) |

#### `ragon-admin night autofill`

Queue up to 5 bounded tasks from QA findings, failing CI tests and open spec items (also runs at the start of `night run` when [night] autofill = true).

| Option | Description |
|---|---|
| `--dry-run` | Say what would be queued; queue nothing. |
| `--max` | Tasks at most (default [night] autofill_max). One of: `1`, `2`, `3`, `4`, `5`. |

#### `ragon-admin night pause`

Let every request through the model gate while a SWE-bench run holds it (the run waits for them, as before the gate).

| Option | Description |
|---|---|
| `--for` | How long: 90m, 2h, 45s, or minutes (default 1h) |

#### `ragon-admin night resume`

End a pause: the model gate refuses everyone but the benchmark run again.

### `ragon-admin swe-bench`

Run ragon over SWE-bench instances on this machine and grade the patches.

#### `ragon-admin swe-bench setup`

Install the harness, download the dataset, build the ragon runtime for the instance images.

| Option | Description |
|---|---|
| `--dataset` | verified (default), lite, full, or a HuggingFace id. |
| `--validate` | Also check which instances grade correctly here (pulls their images) |
| `--instances` | With --validate: comma-separated instance ids. |
| `--limit` | With --validate: only the first N instances, sorted by id. |
| `--force` | With --validate: check again instances that already have a result. |

#### `ragon-admin swe-bench smoke`

Prove the whole pipeline on five instances; run refuses until this has passed.

| Option | Description |
|---|---|
| `--idle-minutes` | Minutes the model must have been idle first (default 10) |
| `--ignore-open-sessions` | Do not wait for open ragon sessions to close (for testing) |

#### `ragon-admin swe-bench run`

The agent phase: one ragon exec per instance, producing predictions.jsonl.

| Option | Description |
|---|---|
| `--dataset` | verified (default), lite, full, or a HuggingFace id. |
| `--instances` | Comma-separated instance ids. |
| `--subset` | A file of instance ids, one per line. |
| `--limit` | Only the first N selected instances, sorted by id. |
| `--name` | The run's name (letters, digits, ., _ and -); an existing run of that name is resumed. |
| `--eval` | Grade the predictions when the agent phase ends. |
| `--code-index` | universal: index each instance's repository on the host and give the agent ragon-code (default off); exact: the same with the SCIP stores alone and no graph. One of: `off`, `universal`, `exact`. |
| `--prompt` | The system prompt the agent starts with: default, offline (default without the web and email blocks), high-swe, or a custom one in $CODEX_HOME/system-prompts (default: the configured one) |
| `--mask` | on: mask old tool outputs in the agent's requests (context budget spec §4.1; default off) One of: `off`, `on`. |
| `--strip-names` | Take the files, modules, functions and classes the reference fix touches out of each issue's text before the agent sees it. |
| `--refine` | Two steps per instance: a session that studies the issue and writes a refined description without changing the repository, then a fresh session that fixes it. |
| `--refine-version` | With --refine: which texts the two steps get. v1 (the default) is the measured one; v2 stops protecting what the issue contradicts and lists those tests with their new values, says what the issue changes, names one option where the issue leaves a choice open, wants checks the bug fails, and checks every claim against the repository (refine spec §10) One of: `v1`, `v2`. |
| `--task-rules` | Rules added to the task prompt, comma-separated, of: issue-v1, tests, tests-v2 (default none). tests: never change an existing test, keep your own scripts in /tmp, and compare failing tests by name with and without the change. tests-v2: the same, except that a test the change fails is weighed against the issue, which decides whether the test or the change is wrong. issue-v1: work out exactly what the issue asks for before editing, follow the pattern of the sibling code that does the same thing, and run the issue's example after the last edit. Rules stack (e.g. tests-v2,issue-v1); the prompt has them in the order listed here, whatever order they are given in. |
| `--hooks` | Rules enforced in the agent's session by Codex hooks, comma-separated, of: issue-v1 (default none). issue-v1: the first edit is held once until the files and functions the issue names have been read, and the first stop is held once if the issue shows an example that no command since the last edit has run. Independent of --task-rules, which only asks. |
| `--until` | HH:MM after which no new instance starts. |
| `--idle-minutes` | Minutes the model must have been idle first (default 10) |
| `--ignore-open-sessions` | Do not wait for open ragon sessions to close (for testing) |
| `--label` | What the model gate's refusal calls this run, e.g. "nightly run" (default: SWE-bench run <name>) |
| `--restart-after-compactions` | Once the fix session's N-th compaction is recorded, stop it and start one fresh session with the task and the stopped session's diff as a file it may apply or discard; the tree is reset first, the time limit is not (default 0: never) |
| `--turn-end-compaction` | Compaction at the end of a turn in every session, through Codex's model_post_turn_compact_threshold_percent on the exec command line: on (the launcher's rule for the run's limit), off (pinned at 0) or a percentage of the usable window (1-99); absent, the key is not set and exec never compacts at a turn's end (spec §16) |
| `--apply-patch` | How the agent is offered Codex's apply_patch tool: freeform (a custom tool, which only ragon-engine serves) or off; the launcher's own choice when absent (spec §22). The pinned Codex has no function form, and ragon refuses one. One of: `off`, `freeform`. |
| `--exec-tool` | How the agent is offered Code Mode's exec tool: function (translated, which SGLang serves) or freeform (Codex's custom tool, which only ragon-engine serves and SGLang drops); the launcher's own choice when absent: function on SGLang and vLLM (spec §23) One of: `function`, `freeform`. |
| `--compat` | The compatibility mode for every session (claude: ragon working the way Claude Code does); the launcher's own setting when absent, which is off in a container. One of: `claude`, `off`. |
| `--drop-old-outputs` | When a session is about to compact, first drop the output of its older tool calls (the calls stay, the last 10 outputs and short ones are kept) and summarise only if that is not enough; sets the launcher's switch in every session (compaction spec §13) |
| `--loop-guard` | Switch on ragon's loop detector in every session: a response that repeats itself is cut where the loop began and the model is told once to stop and answer (DREAMFERENCE_RAGON_LOOP_GUARD; docs/dev/loop-guard.md). Off by default. |
| `--output-budget` | Cap every response at TOKENS (max_output_tokens, set by the model gate from ragon's request metadata); a capped response is followed by one nudge to answer. Needs a gate started by this version (`ragon-admin server start`). Default: no cap. |
| `--sampling` | nonthinking: the model card's non-thinking sampling (temperature 0.7, top_p 0.8, top_k 20, presence_penalty 1.5), filled in by the model gate; default: the server's own (the checkpoint's thinking-mode values). Needs a gate started by this version. One of: `default`, `nonthinking`. |
| `--test-guard` | The launcher's test guard in every session: a change that weakens an existing test is pointed out, and the first stop while it stands is held once (spec §29) |
| `--finish-check` | The launcher's finish check in every session: the first stop after a change no test has run on, mid-work, or calling failures pre-existing unchecked is held once (spec §29) |
| `--review-turn` | After the agent stops with a changed tree, resume its session once more to re-read the issue, read its diff, run the tests of the modules it changed and fix what does not hold, within the task's time limit; the patch is collected after that turn (default off) |
| `--strategy-hint` | Append strategy hint N (1 to 6) after the issue in every prompt, so the rounds of a best-of-N night work in different ways; 0 for none (default 0) |
| `--remove-images` | With --eval: work one repository at a time, in chunks of at most --image-chunk instances, and remove a chunk's images once it is graded. |
| `--image-chunk` | With --eval --remove-images: the most instances of one repository whose images are on disk at once (default [swe_bench] image_chunk, 25; 0: the whole repository) |
| `--shard` | A new run covers only the I-th of K shards of the selected instances, balanced by difficulty and repository and the same on every machine, so K machines share one list; `swe-bench merge` puts the K runs back together. |

#### `ragon-admin swe-bench eval`

The grading phase: the upstream harness applies each patch and runs the tests.

| Option | Description |
|---|---|
| `run` | The run (default: the latest) |
| `--drop-test-hunks` | Grade the same predictions again with every test file left out of each patch, as a separate grading (eval-drop-test-hunks/); no agent runs. |
| `--remove-images` | Grade one repository at a time, in chunks of at most --image-chunk instances, and remove the images this grading pulled once their chunk is graded. |
| `--image-chunk` | With --remove-images: the most instances of one repository graded at once (default [swe_bench] image_chunk, 25; 0: the whole repository) |

#### `ragon-admin swe-bench report`

Print the resolved rate and what it was measured with.

| Option | Description |
|---|---|
| `run` | The run (default: the latest) |
| `--against` | Compare with this run, instance by instance. |
| `--drop-test-hunks` | Report the grading with test files dropped (eval --drop-test-hunks); with --against the other run's plain grading is the comparison, which may be the same run's. |

#### `ragon-admin swe-bench status`

Runs, their progress, images and disk.

#### `ragon-admin swe-bench build-images`

Build the arm64 instance images of a dataset nobody publishes them for (multilingual), from its task repository at the pinned commit.

| Option | Description |
|---|---|
| `--dataset` | A dataset with a task repository (default multilingual) |
| `--instances` | Comma-separated instance ids. |
| `--subset` | A file of instance ids, one per line. |
| `--languages` | Comma-separated languages to take: c, cpp, rust, go, java, js, ruby, php. |
| `--limit` | Only the first N selected instances, sorted by id. |
| `--force` | Build again an image already there. |
| `--validate` | Then validate each built image: the reference patch must resolve, a no-op patch must not. |

#### `ragon-admin swe-bench pin-images`

Compare local community images with their pinned digests.

| Option | Description |
|---|---|
| `--accept` | Pin the local digest of these images. |
| `--new` | Pin every local image that has no pin yet. |

#### `ragon-admin swe-bench shard`

Split a task list into K balanced shards, written as lists of their own.

| Option | Description |
|---|---|
| `list` | The task list, one instance id per line. |
| `--shards` | How many shards. |
| `--out-dir` | Where to write <list>-<I>of<K>.txt (default: beside the list) |
| `--dataset` | The dataset the ids are from (default verified) |
| `--balance-by` | Also spread the tasks of this other list evenly over the shards (repeatable), e.g. the fresh ones; the shards are then not those run --shard makes, and say so. |

#### `ragon-admin swe-bench merge`

Put the runs of one list's shards together as one run, which report reads like any other.

| Option | Description |
|---|---|
| `name` | The merged run's name. |
| `runs` | The shards' runs (copy a run made on another machine into this machine's runs directory first) |

#### `ragon-admin swe-bench clean`

Remove a run's containers and scratch; with --images, the instance images.

| Option | Description |
|---|---|
| `run` | The run (default: every run's containers) |
| `--images` | Also remove the instance images. |

### `ragon-admin audit`

Check what a Ragon session does on the network.

#### `ragon-admin audit egress`

Trace one real ragon session and list every network destination and process, with a verdict.

| Option | Description |
|---|---|
| `--tui` | Trace the full-screen interface on a pseudo-terminal instead of `ragon exec` (needs pexpect and pyte) |
| `--app` | Trace the desktop app (ragon-app) with its window hidden, on the display DISPLAY names. |
| `--web` | Trace the web server, `ragon web serve`, answering one Ask thread instead of `ragon exec`. |
| `--compat` | Trace the compatibility mode's print mode, `ragon --compat claude -p`, in a repository with Claude Code's files. One of: `claude`. |
| `--docs` | Trace the local file index instead: `ragon-docs index` and `search` over a fixture folder must reach nothing. |
| `--sync` | Trace one sync agent cycle instead (`ragon sync agent --once`) against a throwaway hub: it must reach the node's ragon web port and nothing else. |
| `--prompt` | Prompt for the traced session (default: a one-word reply) |
| `--json` | Also write the full result to $CODEX_HOME/audit/<timestamp>-<session>.json (the session is exec, tui, app, web, docs or sync) |

### `ragon-admin node`

Advertise this machine on the local network so clients find it with no address typed.

#### `ragon-admin node enable`

Advertise the node and publish the web UI and web search to the local network.

| Option | Description |
|---|---|
| `--no-web` | Keep the web UI on this machine; clients get ragon and web search only. |
| `--yes` | Never wait for input: root through `sudo -n` only, with no terminal needed (what install.sh runs) |
| `--no-forge` | Do not set up the node's forge (Forgejo); `forge on` does it later. |

#### `ragon-admin node disable`

Stop advertising and put the web UI and web search back on this machine only.

#### `ragon-admin node status`

Show the node id, what is advertised and published, and what a browse of the network returns; with a name, that paired node's status.

| Option | Description |
|---|---|
| `name` | A paired node: show its `ragon-admin status` instead. |

#### `ragon-admin node id`

Print this node's id, writing it first if this machine has none yet.

#### `ragon-admin node list`

List every node on the local network: its model, its load, and whether it is paired.

#### `ragon-admin node add`

Pair with another node over SSH, once, so it can be managed from here.

| Option | Description |
|---|---|
| `name` | The node's name, address or id, as `node list` shows it. |
| `--user` | The account on that node (default: this user's name) |
| `--ssh-port` | That node's SSH port (default 22) |

#### `ragon-admin node remove`

Unpair a node: remove the key on both sides.

| Option | Description |
|---|---|
| `name` | The paired node. |

#### `ragon-admin node set`

Assign a model to a paired node and start it there.

| Option | Description |
|---|---|
| `name` | The paired node. |
| `--model` | A key of that node's model matrix. |

#### `ragon-admin node start`

Start a paired node's model server.

| Option | Description |
|---|---|
| `name` | The paired node. |

#### `ragon-admin node stop`

Stop a paired node's model server.

| Option | Description |
|---|---|
| `name` | The paired node. |

#### `ragon-admin node run`

Run a command on a paired node, in this repository at HEAD; its changes come back as a branch.

| Option | Description |
|---|---|
| `name` | The paired node. |
| `--memory` | The job's memory cap (default 8G; the node sets the ceiling) |
| `--time` | The job's time limit (default 90m; the node sets the ceiling) |
| `--test` | A command that decides pass or fail, run after the job's own. |
| `--gpu` | Ask for the GPU (nodes refuse this for now) |
| `--setup` | The command that builds the job's environment, run once per lock-file content and kept on the node. |
| `--out` | A folder the job writes that comes back as files (to ~/.ragon/jobs/received/<id>), never as a commit. |
| `--bind` | A path on the node to bind read-only; the node's [node] bindable decides which are allowed (repeatable) |
| `job_command` | -- then the command and its arguments. |

#### `ragon-admin node jobs`

List the jobs on a paired node, or on every paired node.

| Option | Description |
|---|---|
| `name` | A paired node (default: all) |

#### `ragon-admin node logs`

Show a job's output again, or continue it.

| Option | Description |
|---|---|
| `job` | The job id. |

#### `ragon-admin node cancel`

Stop a running job.

| Option | Description |
|---|---|
| `job` | The job id. |

#### `ragon-admin node fetch`

Bring a job's result branch into the repository it was sent from, and its --out files.

| Option | Description |
|---|---|
| `job` | The job id. |

#### `ragon-admin node sync-model`

Copy a model's files from this machine's cache to a paired node, so it need not download them.

| Option | Description |
|---|---|
| `name` | The paired node. |
| `model` | A key of the model matrix. |
| `--address` | Reach the node at this address instead, such as its QSFP link's. |

#### `ragon-admin node power-cycle`

Switch a hung node's smart plug off and on again (`power-config` says which plug).

| Option | Description |
|---|---|
| `name` | The paired node. |
| `--off-seconds` | How long the power stays off (default 20, at least 5) |
| `--wait-minutes` | How long to wait for the node to answer afterwards (default 10) |
| `--force` | Cut the power even though the node answers. |

#### `ragon-admin node power-config`

Say which smart plug powers which node (node-power.json).

#### `ragon-admin node power-config plug`

A plug this machine can reach on its own network.

| Option | Description |
|---|---|
| `plug` | The plug's name. |
| `--host` | Its host name here, such as shellyplug-aabbcc.local. |
| `--kind` | The brand (shelly) |
| `--channel` | The switch's id (0 on a plug) |
| `--generation` | The Shelly API generation, 1 or 2. |

#### `ragon-admin node power-config node`

Which plug powers a paired node.

| Option | Description |
|---|---|
| `name` | The paired node. |
| `--plug` | The plug's name. |
| `--via` | The paired node at that site that reaches the plug. |

#### `ragon-admin node power-config show`

Print the plugs and the nodes they power.

#### `ragon-admin node provision`

Set up new GB10s from this one: install Ragon, the root half, the model, pairing, start (FLEET spec); re-run on paired nodes, it is the fleet update.

| Option | Description |
|---|---|
| `hosts` | Host names, addresses or paired nodes; none lists unprovisioned GB10s on the network. |
| `--all` | Every paired node: the fleet update. |
| `--user` | The account on the machines (default: this user's name) |
| `--model` | The model each node is assigned (default: this machine's configured model) |
| `--from` | What to install: this (default) or release[=X.Y.Z]. |
| `--per-host-password` | With several hosts, ask each machine's password separately (default: one password for all) |
| `--mesh` | Also pair every node with every other (not built yet) |
| `--web` | Also share the web UI (`ragon web`) on the LAN there: `node enable` without `--no-web`. |
| `--no-start` | Leave the model server stopped. |
| `--no-forge` | Set the nodes up without their forge (Forgejo) |
| `--restart` | Restart a running model server. |
| `--os-update` | NVIDIA's OS and firmware update first, with a reboot. |
| `--dry-run` | Connect and read only, then print what each machine would change. |
| `--via` | With one host: copy and install over this address instead (a QSFP link's) |
| `--match` | With no hosts: a name pattern for the browse instead of spark-/gx10-/zgx-. |
| `--start-timeout` | Seconds to wait for a started model server (default 1200) |
| `--model-source` | copy (default): rsync the weights from here; hf: the node downloads the public weights from Hugging Face, each file is checked by SHA-256, only what the Hub lacks is sent from here. One of: `copy`, `hf`. |
| `--role` | Also leave it as a night or engine node: main key, docker, the root helper sudo allows, key-only SSH, the role's setup and a readiness report (docs/dev/node.md) One of: `night`, `engine`. |

#### `ragon-admin node prepare`

(Run with sudo) the root steps of a node install, for the user who ran sudo, and nothing else.

#### `ragon-admin node job-exec`

(Run inside a job's systemd unit) carry out one job.

| Option | Description |
|---|---|
| `job` | The job id. |

#### `ragon-admin node authorize`

(Run by `node add` on the other node) authorise a public key, read from standard input, for node operations only.

#### `ragon-admin node serve-job`

(Run by sshd as a paired key's forced command) carry out one node operation.

| Option | Description |
|---|---|
| `--key` | The connecting key's tag. |

### `ragon-admin forge`

The node's Forgejo forge: status, on/off, move, promote, backups, keys, the GitHub mirror.

#### `ragon-admin forge status`

Role, repositories, mirror freshness, last backup, GitHub mirror, storage.

#### `ragon-admin forge on`

Turn the forge on (and set it up).

#### `ragon-admin forge off`

Stop the forge and its advertisement; the data is kept.

#### `ragon-admin forge move`

Hand the forge of record to another (paired) node.

| Option | Description |
|---|---|
| `node` | The paired node that becomes the record. |
| `--back` | Move the role back to the node it came from. |

#### `ragon-admin forge promote`

Make a node the forge of record from its newest backup (when the record is lost).

| Option | Description |
|---|---|
| `node` | A paired node, or none for this one. |

#### `ragon-admin forge backup`

Take a backup now and send it to two other nodes.

#### `ragon-admin forge restore`

Check that the newest backup restores, every ref.

| Option | Description |
|---|---|
| `--check` | Restore into a scratch folder and compare refs (nothing running is touched) |

#### `ragon-admin forge key`

SSH keys of the owner account.

#### `ragon-admin forge key add`

Add a public key to the owner account.

| Option | Description |
|---|---|
| `file` | The public key file. |

#### `ragon-admin forge github-mirror`

This fleet's mirror of every branch to GitHub.

| Option | Description |
|---|---|
| `--all-branches` | Mirror every branch and tag, never deleting on GitHub. |
| `--off` | Stop mirroring. |
| `--account` | The `gh` account whose login pushes. |

#### `ragon-admin forge password`

Print the owner's password again (this node's terminal only).

| Option | Description |
|---|---|
| `--show` |  |

#### `ragon-admin forge import`

Import a GitHub repository's branches and tags.

| Option | Description |
|---|---|
| `source` | The GitHub repository's URL. |
| `name` | owner/name on the forge (an organisation is created when missing) |
| `--anonymity` | The push hook's check (default off) One of: `full`, `denylist`, `off`. |
| `--exempt` | A path the check skips (repeatable) |
| `--mirror` | Mirror the forge's branches back to the source. |

#### `ragon-admin forge archive`

Import git bundles into a private, archived repository that is never mirrored.

| Option | Description |
|---|---|
| `name` | owner/name on the forge. |
| `bundles` | Bundle files. |

#### `ragon-admin forge mirrors`

Mirror nodes.

| Option | Description |
|---|---|
| `mirrors_command` | sync: hand the record's plan to every paired node. One of: `sync`. |

#### `ragon-admin forge run`

(The forge's unit) run the container in the foreground.

#### `ragon-admin forge tick`

(The forge's timer) storage budget, hooks, GitHub and node mirrors.

### `ragon-admin benchmark_server`

Run vLLM serve benchmark using Sonnet dataset.

| Option | Description |
|---|---|
| `--port` | Port of the server to benchmark. |
| `--model` | Model name to benchmark. |
| `--dataset-path` | Path to the sonnet dataset inside the container. Unset probes the known locations for the image the server is running. |
| `--num-prompts` | Number of prompts to benchmark. |
| `--max-concurrency` | Max concurrency for requests. |

### `ragon-admin desktop`

Ragon desktop app (a native window onto the local deployment).

#### `ragon-admin desktop install`

Install the desktop app's packages (Electron, from npm) and the AppArmor profile its sandbox needs.

#### `ragon-admin desktop run`

Open the Ragon desktop window.

#### `ragon-admin desktop build`

Build a distributable desktop bundle.

#### `ragon-admin desktop status`

Report whether the desktop app can be built and launched.

### `ragon-admin gmail`

Search and read connected Gmail accounts (read-only).

#### `ragon-admin gmail search`

Search with Gmail query syntax.

| Option | Description |
|---|---|
| `query` | Gmail query, e.g. from:alice newer_than:7d. |
| `-n`, `--max-results` | Messages to return (max 20) |
| `--json` | Emit raw JSON. |

#### `ragon-admin gmail read`

Read one message by the id search printed.

| Option | Description |
|---|---|
| `message_id` | Message id exactly as `gmail search` printed it. |
| `--max-chars` | Body characters to print. |
| `--json` | Emit raw JSON. |

#### `ragon-admin gmail status`

Show which accounts are connected.

| Option | Description |
|---|---|
| `--json` | Emit raw JSON. |

### `ragon-admin docs`

Manage the local file index's run-time files (ragon-docs).

#### `ragon-admin docs setup`

Install PDFium, ONNX Runtime, the embedding model and OCR's models and libraries ragon-docs loads (pinned, checked).

### `ragon-admin searxng`

Manage the local SearXNG search container.

#### `ragon-admin searxng start`

Start SearXNG on 127.0.0.1:8888 (recreates one made on Docker's default bridge).

#### `ragon-admin searxng status`

Show each search engine's state (ok, failing, suspended).

### `ragon-admin google`

Manage the local Google service (Gmail, Drive, Calendar).

#### `ragon-admin google start`

Start the Google service on 127.0.0.1:8767 (adopts an existing container).

#### `ragon-admin google stop`

Remove the Google service container; connected accounts stay stored.

#### `ragon-admin google status`

Show whether it runs and which accounts hold which apps.

### `ragon-admin adapter`

Install and manage LoRA adapters for the served model.

#### `ragon-admin adapter list`

Show the installed adapters (no network).

| Option | Description |
|---|---|
| `--available` | Also list the adapters this release catalogues. |
| `--json` | Print JSON. |

#### `ragon-admin adapter install`

Download, verify and install an adapter (the only adapter step that uses the network).

| Option | Description |
|---|---|
| `name` | A catalogued adapter, as <name> or <name>@<version>. |
| `--repo` | An adapter published after this release: <org>/<repo>@<commit>. |
| `--third-party` | Install an adapter that is not signed by Dreamference (asks you to type its name) |
| `--hf-token` | A Hugging Face token, for a private repository. |

#### `ragon-admin adapter remove`

Remove an installed adapter and its files.

| Option | Description |
|---|---|
| `name` | The installed adapter. |

#### `ragon-admin adapter verify`

Check installed adapters again: signature, files and the served base.

| Option | Description |
|---|---|
| `name` | One adapter (default: all) |

### `ragon-admin images`

Manage image search (the image_search tool and /images/).

#### `ragon-admin images start`

Start the image search service on 127.0.0.1:8768 (SigLIP pre-filter best-effort).

| Option | Description |
|---|---|
| `--no-siglip` | Skip the SigLIP pre-filter; rank the first candidates directly. |

#### `ragon-admin images stop`

Remove the image search containers; stored images stay.

#### `ragon-admin images status`

Show the containers, the store and whether the tool is offered.

#### `ragon-admin images mcp`

Serve the image_search tool over stdio (ragon starts this).

### `ragon-admin voice`

Manage speech-to-text for the microphone in the app and ragon web.

#### `ragon-admin voice start`

Start speech-to-text (Whisper on the CPU) on 127.0.0.1:8100.

#### `ragon-admin voice stop`

Remove the speech-to-text container; its model stays cached.

#### `ragon-admin voice status`

Show whether speech-to-text runs.

### `ragon-admin matrix`

Manage the private Matrix homeserver for chatting with Ragon from a phone.

#### `ragon-admin matrix start`

Start the homeserver (no route out), its loopback proxy and its way in from the phone.

| Option | Description |
|---|---|
| `--network` | Where a NEW homeserver is reached: the remote-access overlay (default when set up) or Tailscale. One of: `overlay`, `tailscale`. |

#### `ragon-admin matrix stop`

Stop the homeserver; accounts and messages are kept.

#### `ragon-admin matrix status`

Show the server name, the container, Tailscale and the accounts.

#### `ragon-admin matrix add-user`

Create an account for the phone and allow it to talk to Ragon.

| Option | Description |
|---|---|
| `name` | The user name (lowercase letters, digits, . _ = -) |

#### `ragon-admin matrix push`

Let push notifications leave the machine (event ids only), or not.

| Option | Description |
|---|---|
| `state` | One of: `on`, `off`. |

#### `ragon-admin matrix remove`

Delete the homeserver, every account and every message.

| Option | Description |
|---|---|
| `--yes` | Confirm the deletion. |

### `ragon-admin remote`

Reach this node from outside the LAN: a NetBird overlay run here, one rented relay box.

#### `ragon-admin remote setup`

Set up (or update) the control plane here, the box over SSH, the tunnel, and this node's peer.

| Option | Description |
|---|---|
| `dns_name` | The box's public DNS name; `ssh <name>` must log in. |
| `--yes` | Do not ask; never wait for a sudo password. |
| `--ssh` | Your SSH Host alias for the box, when `ssh <dns_name>` itself does not log in (remembered; never an address) |
| `--ca-scope` | The domain the node's new CA may sign under (default: the parent of the box's name) |
| `--box-only` | Deploy the box and stop: nothing runs on this node (no container, tunnel or sudo) |

#### `ragon-admin remote status`

Show the tunnel, the box's units, the certificates and the peers.

#### `ragon-admin remote remove`

Remove the box's relay and haproxy and the tunnel; keep the CA and the enrolments.

| Option | Description |
|---|---|
| `--purge` | Also delete the control plane, the CA and every enrolment. |
| `--yes` | Do not ask. |

#### `ragon-admin remote code`

Print a one-time code and wait for one client to enrol with it on this LAN.

| Option | Description |
|---|---|
| `--bundle-file` | Instead: write one ephemeral peer's bundle (one-use, 10-minute key) to a new 0600 file, no code and no listener (an off-LAN test runner) |
| `--ttl` | With --bundle-file: an ordinary peer's key that waits this long (90m, 2h; at most 24h), still one use; unused, it expires by itself. |
| `--join` | With --bundle-file: write the one line that installs NetBird and joins on a new machine with no ragon (2h unless --ttl) |
| `--name` | With --join: the new machine's name in the overlay (one DNS label) |
| `--ssh-user` | With --join: the account for the machine's gx10-remote ssh alias. |

#### `ragon-admin remote peers`

List the enrolled peers.

#### `ragon-admin remote revoke`

Delete a peer: its key is forgotten; it can rejoin only on the LAN.

| Option | Description |
|---|---|
| `peer` | The peer's name or overlay name (or a prefix) |

#### `ragon-admin remote certificate`

The control plane's public certificate for the box's name (Let's Encrypt, DNS-01 here).

#### `ragon-admin remote certificate issue`

Get it and serve it from the control plane; renewed daily by a user timer.

| Option | Description |
|---|---|
| `--staging` | Test against Let's Encrypt's staging CA; its certificate is never served. |

#### `ragon-admin remote certificate relay`

Give the relay its own public certificate under its own name (relay.<domain>).

| Option | Description |
|---|---|
| `--staging` | Test against Let's Encrypt's staging CA; nothing changes on the box. |
| `--name` | The relay's name (default: relay. and the box's parent domain) |

#### `ragon-admin remote certificate relay-off`

Put the node CA's certificate and the box's name back on the relay.

#### `ragon-admin remote certificate renew`

Renew them when due (the timer's command).

#### `ragon-admin remote certificate off`

Serve the node CA's certificate again and stop renewing.

### `ragon-admin web`

Launch Web Canvas UI interactive pair-programming pane.

| Option | Description |
|---|---|
| `--port` | Port for Web Canvas UI. |
