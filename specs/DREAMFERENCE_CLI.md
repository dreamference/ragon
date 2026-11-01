# Ragon CLI Reference

> **Version:** 0.9.0 (`setup.py`)
> **Subject:** Command Suite, Subcommands, Configuration, Environment Variables
> **Checked against the code** (`dreamference/cli/dreamference_cli_controller.py`, `build_parser()`: a script walked the parser and found every subcommand and every visible `--option` in this document; the launcher's own subcommands read from `ragon-rs/src/lib.rs`)

---

## Table of Contents

- [1. CLI Overview](#1-cli-overview)
- [2. Global Options](#2-global-options)
- [3. Subcommand Summary](#3-subcommand-summary)
- [4. Detailed Command Reference](#4-detailed-command-reference)
- [5. Configuration Hierarchy](#5-configuration-hierarchy)
- [6. Environment Variables](#6-environment-variables)

---

## 1. CLI Overview

**Entry points** (`setup.py` `console_scripts`):

- `ragon-admin`: the administration CLI (`dreamference.cli:main`, controller `DreamferenceCLIController` in `dreamference/cli/`). Everything in this document.
- `ragon`: **not** a Python entry point. It is the Rust binary built by `ragon-admin codex build`: Codex with Ragon's branding and launcher compiled in, linked at `~/.local/bin/ragon`. It takes Codex's command line, plus subcommands the launcher answers itself before Codex parses anything (`LAUNCHER_SUBCOMMANDS` in `ragon-rs/src/lib.rs`):
  - `ragon app` (the desktop app), `ragon night …`, `ragon airgapped [default <level>]`, `ragon node list|use|forget|join`, `ragon worktree …`, `ragon refine …`, `ragon sync …`, `ragon uninstall`;
  - `ragon compat [status|use claude|off|doctor]` and the flag `--compat claude|off`, the compatibility mode: `ragon` working the way Claude Code does, reading its project files (`DREAMFERENCE_RAGON_COMPAT` or `ragon_compat`, the key read only from the user's own `~/.config/dreamference/config.toml`; see `DREAMFERENCE_RAGON_CLAUDE.md` §0.4);
  - `ragon prompt [list|show [<name>]|use <name>]`, which chooses the system prompt new sessions get (`default`, `offline`, `high-swe`, `ask` or a custom one; `DREAMFERENCE_RAGON_PROMPT` or `ragon_prompt`, see `DREAMFERENCE_RAGON_PROMPT.md`);
  - `ragon skill …` (other agents' skills, `DREAMFERENCE_RAGON_SKILLS.md`), `ragon docs add|remove|search|read|status` (the document index, `DREAMFERENCE_RAGON_LOCAL_INDEX.md`), `ragon apps`, `ragon ledger`, `ragon notice`, `ragon audit`, `ragon loop-guard`, `ragon guard`, `ragon monitor …` and `ragon delegate …` (`DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md`);
  - `ragon web serve|start|stop|status|open|pair` (the web UI on port 3100, `DREAMFERENCE_RAGON_ASK.md`), `ragon chat …` (Matrix and Telegram) and `ragon signal setup|status|start|stop|trust|remove|unit` (Linux), both off until set up (`DREAMFERENCE_RAGON_CHAT.md`, `DREAMFERENCE_RAGON_SIGNAL.md`);
  - `ragon update`, a subcommand patch `0008` adds to Codex's own parser.

  `login`, `logout`, `cloud` and `cloud-tasks` are refused with the reason. See `DREAMFERENCE_RAGON_CODEX.md`.

There is no `chat` subcommand. The interactive agent is `ragon`. The other agents (Cline, Continue, OpenHands) are reachable through `ragon-admin run "…" --agent …`.

**Framework:** argparse + Rich terminal UI. Parsing is strict: every command rejects unknown arguments.

**Commands:**

- **Setup:** `init`, `host {check,setup}`, `model {list,download}`, `main-model {set,inspect}`, `diffusion-model {set}` (absent while diffusion is switched off, §4.2), `adapter {list,install,remove,verify}`, `clear {model-cache,tensorize-cache}`, `docs setup`
- **Agents:** `run`, `codex {build,start,stop,test}`, `night {enable,disable,status,run,every,archive,autofill,pause,resume}`
- **Measurement and checks:** `swe-bench {setup,build-images,smoke,run,eval,report,status,clean,shard,merge}`, `audit {egress}`
- **Model server and fleet:** `server {start,stop,remove,logs}`, `logs [server|mcp]`, `endpoints`, `benchmark_server`, `node {enable,disable,status,id,list,add,remove,set,start,stop,sync-model,power-cycle,power-config,run,jobs,logs,cancel,fetch,provision,prepare}` (plus `authorize`, `serve-job`, `job-exec` and `askpass`, which a person does not type, §4.25), `forge {status,on,off,move,promote,backup,restore,key,github-mirror,password,import,archive,mirrors}` (plus `run` and `tick`, §4.30), `remote {setup,status,remove,code,peers,revoke,certificate}` (§4.31)
- **Web UI, desktop and messengers:** `desktop {install,run,build,status}`, `matrix {start,stop,status,add-user,push,remove}`; `ragon web` itself is the launcher's
- **Agent tools:** `gmail {search,read,status}`, `google {start,stop,status}`, `images {start,stop,status,mcp}`, `voice {start,stop,status}`, `searxng {start,status}`, `code setup`; search and fetch are commands of their own, `ragon-search` and `ragon-fetch` (§4.16), and so is the code index, `ragon-code` (§4.22)
- **Context and IDE:** `index`, `mcp`, `web`, `status`

---

## 2. Global Options

These options come before the subcommand (`ragon-admin --agent cline run "…"`). `init` and `run` also accept most of them after the subcommand.

| Flag | Type | Description |
| :--- | :--- | :--- |
| `--config PATH` | Path | Custom Ragon config file |
| `--agent {codex,cline,continue,openhands}` | Choice | Agent runner (default `codex`) |
| `--hf-token TOKEN` | String | HuggingFace token (else `HF_TOKEN` / `DREAMFERENCE_HF_TOKEN`) |

`--debug` belongs to `run`; they are not global.

---

## 3. Subcommand Summary

| Subcommand | Description | Key Args |
| :--- | :--- | :--- |
| **`init`** | Download weights, write the config, force a workspace re-index | `[--model] [--vllm-host] [--draft-model] [--agent] [--hf-token]` |
| **`run`** | Run one task with the selected agent | `PROMPT [--model] [--draft-model] [--agent] [--hf-token] [--debug] [--compat claude\|off]` (`--compat` sets `DREAMFERENCE_RAGON_COMPAT` for that `ragon` session; refused for the other agents) |
| **`status`** | Hardware, vLLM and agent, and context-index panels | — |
| **`index`** | AST + FTS5 + TF-IDF + embedding workspace index | `[--dir PATH] [--force]` |
| **`mcp`** | stdio MCP server for JetBrains / VS Code | — |
| **`web`** | Web Canvas status page | `[--port 8501]` |
| **`model list`** | List the model matrix | — |
| **`model download`** | Pre-download weights to the HF cache | `[--model] [--all] [--tensorize/--no-tensorize]` |
| **`main-model set`** | Pin the main model in the config | `MODEL` |
| **`main-model inspect`** | Probe the running main model | `[--deep]` |
| **`diffusion-model set`** | Pin the diffusion model served beside the main one | `MODEL` |
| **`clear model-cache`** | Delete the HF and `~/.cache/dreamference` model caches (see §4.17) | — |
| **`clear tensorize-cache`** | Delete the tensorizer cache under `~/.cache/dreamference` (see §4.18) | — |
| **`endpoints`** | Print endpoints and credentials | — |
| **`server start`** | Launch vLLM, plus the diffusion sidecar | many; see §4.6 |
| **`server stop` / `remove`** | Stop / remove the vLLM and diffusion containers | `[--port 8000] [--diffusion-port 8001]` |
| **`server logs`** / **`logs [server]`** | Tail the vLLM container | `[--port 8000]` |
| **`logs mcp`** | Codex MCP lifecycle lines from `~/.ragon/logs_2.sqlite` (or `$CODEX_HOME`) | — |
| **`benchmark_server`** | `vllm bench serve` on the Sonnet dataset | `[--port] [--model] [--dataset-path] [--num-prompts 8] [--max-concurrency 1]` |
| **`codex build`** | Build `ragon` from the `codex/` submodule and `codex-patches/`, plus `ragon-search`, `ragon-fetch`, `ragon-code` and `ragon-docs`; then the egress audit | `[--force] [--no-audit]` |
| **`codex start` / `stop`** | Start / stop the Codex app-server daemon using `ragon` | — |
| **`codex test`** | Run Codex's own test suite on Ragon's patched tree, except the tests in `codex-tests/ragon-skips.toml` | `[-E FILTER] [--test-threads 8] [--jobs 6] [--memory-max 24G] [--accept-snapshots]` |
| **`code setup`** | Install the pinned tools the code index (`ragon-code`) runs | — |
| **`night …`** | Night Shift: the timer and the overnight run of the `/night` queue | `enable [--window]`, `disable`, `status`, `run [--until] [--minutes] [--idle-minutes] [--ignore-open-sessions]`, `every {add,list,drop}`, `archive [--weeks 4]`, `autofill [--dry-run] [--max N]`, `pause [--for DURATION]`, `resume` (the model gate during a SWE-bench run); see §4.21 |
| **`swe-bench …`** | Run `ragon` over SWE-bench instances and have the upstream harness grade the patches | `setup`, `build-images`, `smoke`, `run`, `eval`, `report`, `status`, `clean`, `shard`, `merge`; see §4.23 |
| **`audit egress`** | Trace one real `ragon` session (or the TUI, the desktop app, `ragon web`, the document index, `ragon sync`) and list every network destination and process, with a verdict | `[--tui \| --app \| --web \| --docs \| --sync \| --compat claude] [--prompt P] [--json]`; see §4.24 |
| **`node …`** | Advertise this machine on the local network so clients find it with no address typed; list other nodes and, once paired over SSH, manage them, copy a model to them and run jobs on them | `enable [--no-web] [--yes] [--no-forge]`, `disable`, `status [NAME]`, `id`, `list`, `add NAME`, `remove NAME`, `set NAME --model M`, `start NAME`, `stop NAME`, `sync-model NAME MODEL`, `power-cycle NAME`, `power-config {plug,node,show}`, `run NAME [--gpu] … -- CMD`, `jobs`, `logs`, `cancel`, `fetch`, `provision [HOST…]`, `prepare`; see §4.25 |
| **`host …`** | The host settings a model load is refused without: report them, or apply them with sudo | `check`, `setup [--yes]`; also Wi-Fi power saving, which blocks no load; see §4.26 |
| **`images …`** | Image search: the sidecar and the `image_search` tool's MCP server | `start [--no-siglip]`, `stop`, `status`, `mcp`; see §4.19 |
| **`voice …`** | Speech-to-text for the microphone in the app and `ragon web` | `start`, `stop`, `status`; see §4.19 |
| **`desktop …`** | The Electron desktop app (`ragon-app`) | `install`, `run`, `build`, `status` |
| **`docs setup`** | Fetch what `ragon-docs` loads: PDFium, ONNX Runtime, the embedding model and OCR's models and libraries, pinned | — |
| **`google …`** | The Google service (Gmail, Drive, Calendar for `/apps`) without the web UI, on `127.0.0.1:8767` | `start`, `stop`, `status`; see §4.27 |
| **`matrix …`** | The private Matrix homeserver behind `ragon chat` (off by default) | `start`, `stop`, `status`, `add-user NAME`, `push on\|off`, `remove [--yes]`; see §4.28 |
| **`adapter …`** | Install and manage LoRA adapters for the served model | `list [--available] [--json]`, `install [NAME] [--repo R] [--third-party] [--hf-token T]`, `remove NAME`, `verify [NAME]`; see §4.29 |
| **`forge …`** | The node's Forgejo forge: status, on/off, move, promote, backups, keys, the GitHub mirror | `status`, `on`, `off`, `move [NODE] [--back]`, `promote [NODE]`, `backup`, `restore --check`, `key add FILE`, `github-mirror --all-branches\|--off [--account A]`, `password --show`, `import SOURCE NAME …`, `archive NAME BUNDLES…`, `mirrors sync`; see §4.30 |
| **`remote …`** | Reach this node from outside the LAN: a NetBird overlay run here, one rented relay box | `setup DNS_NAME …`, `status`, `remove [--purge] [--yes]`, `code [--bundle-file P] [--ttl D] [--join] …`, `peers`, `revoke PEER`, `certificate {issue,relay,relay-off,renew,off}`; see §4.31 |
| **`searxng start`** | Start the SearXNG container on `127.0.0.1:8888`, on the network `dreamference-sidecars`; replaces one made on Docker's default bridge ([DOCKER §6](./DREAMFERENCE_DOCKER.md)) and writes its settings | — |
| **`searxng status`** | Each general search engine's state: ok, failing, suspended until when, not loaded, off; read from SearXNG's logs, nothing searched | — |
| **`gmail …`** | Read-only Gmail search and read | `search QUERY [-n 10] [--json]`, `read ID [--max-chars 8000] [--json]`, `status [--json]` |

A top-level `clear-tensorize-cache` subcommand is the older spelling of `clear tensorize-cache` and does the same (until recently it parsed and did nothing).

---

## 4. Detailed Command Reference

### 4.1. `ragon-admin init`

```bash
ragon-admin init [--model MODEL] [--draft-model DRAFT_MODEL] [--vllm-host HOST] [--agent …] [--hf-token …]
```

**Behaviour:**
1. Downloads the main (and draft) weights into the HF cache, tensorizing them if `use_tensorizer` is set.
2. Writes a minimal `dreamference.toml` via `config_generator.generate_default_init_config`, at the path `ConfigPathResolver` resolves.
3. Forces a full workspace re-index (`ContextEngine.index_workspace(force_reindex=True)`).

---

### 4.2. `ragon-admin main-model` / `diffusion-model`

> **The diffusion model is switched off** (`DIFFUSION_ENABLED` in `hardware/model_matrix_registry.py`). While it is off, `diffusion-model` is not a command, `server start` starts no sidecar and removes a leftover one without a word, `server stop`/`remove` remove a leftover too, `endpoints` prints no diffusion URL, and `model list`/`model download` leave the diffusion model out. The `--diffusion-*` flags are accepted with their help suppressed. Everything about the sidecar in this document describes it switched on.

```bash
ragon-admin main-model set MODEL
ragon-admin main-model inspect [--deep]
ragon-admin diffusion-model set MODEL
```

- **`main-model set`:** writes the model to `dreamference.toml`, which pins it, and reminds you to run `ragon-admin images start` again when image search runs (it names the served model). It refuses a diffusion checkpoint.
- **`diffusion-model set`:** the counterpart for the diffusion sidecar. It refuses a model vLLM can serve.
- **`main-model inspect`:** probes the running model: tool calling, JSON mode, reasoning tags, streaming TTFT, latency. `--deep` also reports the quantization map, KV geometry, sampling provenance, graph coverage and per-workload speculative acceptance. That sends extra requests.

---

### 4.3. `ragon-admin run`

```bash
ragon-admin run "PROMPT" [--model MODEL] [--draft-model DRAFT_MODEL] [--agent …] [--hf-token …] [--debug] [--compat claude|off]
```

**Behaviour:** dispatches on `config.agent_runner` to one of `CodexRunner` (also for any unrecognised value), `ClineRunner`, `ContinueRunner` or `OpenHandsRunner`, then calls `run_session(prompt=…, debug=…)`. `--compat` sets `DREAMFERENCE_RAGON_COMPAT` for that `ragon` session and is refused for the other agents.

- **Codex (default):**
  - builds `ragon` if it is missing or stale;
  - runs `ragon "PROMPT"`, passing the vLLM host as `DREAMFERENCE_VLLM_HOST`;
  - with `--debug`, sets `RUST_LOG=codex_mcp=trace,codex_core=debug,codex_app_server=debug,info`.
- **Cline:** prints the prompt and opens VS Code.
- **Continue / OpenHands:** launch their UI; the prompt is unused.

---

### 4.4. `ragon-admin status`

**Panels:**

1. **NVIDIA GB10 Hardware Status:** GB10 qualification, GPU name, driver version, total / used / available unified memory, architecture.
2. **vLLM & Agent Status:**
   - vLLM health and served models;
   - active agent runner (default CODEX), configured model, tensorize status, draft model;
   - HF token presence;
   - prefix caching / chunked prefill, multi-step scheduling, KV cache dtype (`from model recipe` unless overridden), tool-call parser;
   - install state of each agent (Cline, Continue, OpenHands, Codex);
   - the Ragon config path.
3. **Context Engine Index Status:** indexed files, AST symbols, index path, SQLite store path. Shown only when an index exists.

---

### 4.5. `ragon-admin index`

```bash
ragon-admin index [--dir PATH] [--force]
```

Indexes the workspace, the current directory unless `--dir` is given, with Python `ast` symbols, FTS5, TF-IDF and `nomic-embed-text-v1.5` embeddings. It writes `.dreamference/context_index.json` and `.dreamference/context.db` (SQLite: FTS5, and the embeddings as plain float32 blobs; sqlite-vec is not used). It refuses to run while a Night Shift run holds its lock. See `DREAMFERENCE_CONTEXT.md`.

---

### 4.6. `ragon-admin server start`

```bash
ragon-admin server start [--model MODEL] [--port 8000] [--engine ragon|sglang] [--quantization Q] [--draft-model D] [--num-speculative-tokens N]
  [--hf-token T] [--num-scheduler-steps N] [--attention-backend B] [--kv-cache-dtype D] [--api-key K]
  [--enable-auto-tool-choice] [--tool-call-parser P] [--reasoning-parser P] [--moe-backend B]
  [--max-num-batched-tokens N] [--guided-decoding-backend B] [--tensorize/--no-tensorize]
  [--docker-image IMG] [--diffusion-model M] [--diffusion-port 8001] [--no-diffusion] [--no-gate]
```

**Behaviour:**
1. Refuses while a Night Shift run holds its lock (§4.21), then runs the host-safety pre-flight (`check_host_safety`).
2. Resolves the serving engine (`--engine`, else `server_engine` in the config, else the model's default: `ragon-serve` for the default model since 0.9.0, with SGLang one flag away; when LoRA adapters are installed for the model and `ragon-serve` is resolved without `--engine`, it falls back to SGLang, and an explicit `--engine ragon` is refused). On a node it then starts the Google service, image search and speech-to-text when absent (`ensure_on_node`); a failure is a warning naming the command to retry and never blocks the model.
3. Starts the diffusion sidecar `dreamference-diffusion-<diffusion-port>` **first**, unless `--no-diffusion`, so that vLLM's free-memory check accounts for it.
4. Starts the engine in Docker, under the PSI memory-pressure watchdog. It streams logs and memory until the health check passes, runs the correctness and streamed `/v1/responses` tool-call canaries (`ToolCallCanary`), then exits, leaving the server running.

**The model gate** (`RAGON_SWE_BENCH.md` §18). The engine listens on `127.0.0.1:<port + 10000>` and a second container, `dreamference-gate-<port>` (the engine's image, `--network host`, the same restart policy, 256 MiB), serves `<port>` on every interface in front of it. Every client reaches the model through it; it passes everything through untouched unless a SWE-bench run holds it. It starts after the old engine container is removed and before the new one; if it cannot start, the engine serves `<port>` itself, as before. `--no-gate` does the same on purpose. `server stop` and `remove` stop and remove the gate with the engine.

Per-model flags come from the model matrix's `launch_overrides` (or `engine_recipes[engine]`). Options given here override them, and anything unset falls back to the recipe. `--docker-image` overrides the model's pinned image.

**Speculation.** `--draft-model` and `--num-speculative-tokens` are turned into `--speculative-config` JSON by `VLLMServerManager.resolve_speculative_config()`, never into separate flags: vLLM 0.2x has no `--speculative-model` or `--num-speculative-tokens`. The depth passed is the config's resolved value (flag > `DREAMFERENCE_SPECULATIVE_TOKENS` > file > default), and it applies only together with `--draft-model`; without one, the recipe's speculative config is used exactly. See `DREAMFERENCE_INFERENCE.md` §3.

**Docker command** (`VLLMServerManager.start_server`):
```bash
docker run --ipc=host --network host --restart unless-stopped --name dreamference-vllm-<port> --gpus all \
  --cpus=<limit> --memory=<N>g --memory-swap=<N>g --oom-score-adj=800 \
  -v <$HF_HOME or ~/.cache/huggingface>:/root/.cache/huggingface \
  -v ~/.cache/dreamference:/root/.cache/dreamference \
  -e VLLM_NO_USAGE_STATS=1 -e DO_NOT_TRACK=1 [-e HF_TOKEN=<token>] [recipe env vars] \
  -e VLLM_CACHE_ROOT=/root/.cache/dreamference/vllm -e CUTE_DSL_ARCH=sm_121a -e VLLM_LOGGING_LEVEL=DEBUG \
  -e VLLM_DEBUG_LOG_API_SERVER_RESPONSE=1 -e VLLM_DEBUG_LOG_API_SERVER_REQUEST=1 \
  --entrypoint vllm <image> serve <hf_repo> [vllm flags]
```

The line above is the vLLM form. A model whose recipe names `engine: sglang` (the default model does) gets the same `docker run` prefix and mounts, and `SGLangLaunchBuilder` builds what follows the image; for `engine: ragon` (`--engine ragon`), `RagonServeLaunchBuilder` builds the `ragon-serve` launch (`DREAMFERENCE_INFERENCE.md`).

The container's memory limit is derived from the model's `gpu_memory_utilization` plus headroom, capped below total memory by a host reserve. `--memory-swap` equals `--memory`, so the container cannot swap.

---

### 4.7. `ragon-admin server stop` / `remove`

```bash
ragon-admin server stop   [--port 8000] [--diffusion-port 8001]
ragon-admin server remove [--port 8000] [--diffusion-port 8001]
```

- `stop` runs `docker stop` on both `dreamference-vllm-<port>` and `dreamference-diffusion-<diffusion-port>` (and stops/removes `dreamference-gate-<port>`).
- `remove` runs `docker rm -f` on both (and `dreamference-gate-<port>`).

Both are safe no-ops when the containers don't exist.

---

### 4.8. `ragon-admin logs` / `server logs`

```bash
ragon-admin logs [server|mcp] [--port 8000]
ragon-admin server logs [--port 8000]
```

- **`logs`, `logs server` and `server logs`:** tail `dreamference-vllm-<port>` (`show_request_logs`).
- **`logs mcp`:** reads the Codex TUI's tracing database `~/.ragon/logs_2.sqlite` (`$CODEX_HOME` if set; not upstream Codex's `~/.codex`) read-only, and prints MCP server lifecycle lines. The TUI logs there, not to a file. Record them with `RUST_LOG=codex_mcp=trace ragon`.

---

### 4.9. `ragon-admin benchmark_server`

```bash
ragon-admin benchmark_server [--port 8000] [--model qwen3.8-27b-nvfp4-dflash2] [--dataset-path P] [--num-prompts 8] [--max-concurrency 1]
```

Runs vLLM's serving benchmark inside the running container, on the Sonnet dataset. The dataset is embedded in `sonnet_dataset.py` and staged at `/tmp/dreamference-sonnet.txt`. When `--dataset-path` is unset, the known in-image locations are probed first.

---

### 4.10. `ragon-admin mcp`

stdio JSON-RPC MCP server (`serverInfo.name` `dreamference-mcp-server`) for JetBrains and VS Code. Its tools (`mcp_tool_registry.py`):

- `ide_get_active_editor`, `ide_get_diagnostics`, `ide_get_open_files`, `ide_open_file`: these read and write the in-process `IDEState`, which is empty unless an IDE companion populates it.
- `ide_apply_diff`: always answers `status: not_applied` and tells the agent to edit the file directly. Nothing in the server writes files. Originally it answered `success`, so an agent carried on as though its edit had landed.
- `web_search`, `web_fetch`: `WebTools`, through the local SearXNG instance and direct HTTP.
- `workspace_search_code`: `CodeIndexSearch.search` (`ragon-code`) when `ragon-code` is installed and the workspace is indexed, falling back to `ContextEngine.search_code`.

Protocol handling:
- methods: `initialize`, `ping`, `tools/list`, `tools/call`;
- notifications (no `id`) are never answered;
- malformed JSON gets `-32700` with a null id;
- an exception inside a method gets `-32603` carrying the request's id.

Originally `notifications/initialized` drew a `-32601` reply, and a failing tool was reported as a parse error with no id, so the client never got its answer.

It is **not** registered with `ragon`; see `DREAMFERENCE_RAGON_CODEX.md` for why.

---

### 4.11. `ragon-admin endpoints`

It prints two tables:

1. **Available endpoints (Standard /v1):**
   - `/v1/models` (GET)
   - `/v1/chat/completions` (POST)
   - `/v1/completions` (POST)
   - `/v1/embeddings` (POST, if supported)
   - `/health` (GET)
2. **Credentials:**
   - the base URL on localhost and on the LAN IP;
   - the diffusion model URL (`http://localhost:8001/v1`), only while diffusion is switched on;
   - the API key, which is optional and set only by `server start --api-key`;
   - the `Authorization: Bearer <key>` header format.

Clients address the model by its full HF repo id (e.g. `RadixArk/Qwen3.8-27B-NVFP4`, the default model's).

---

### 4.12. `ragon-admin web`

```bash
ragon-admin web [--port 8501]
```

Serves the Web Canvas page (`web_canvas.py`) on `127.0.0.1:<port>` (until recently, `0.0.0.0`) with `socketserver.TCPServer`: a status page with a live unified-memory gauge fed by `GET /api/status` (`{hardware, vllm, context}`).

---

### 4.13. `ragon-admin model list`

Lists `ModelMatrixRegistry.MATRIX`: names, HF repos, compatibility.

### 4.14. `ragon-admin model download`

```bash
ragon-admin model download [--model MODEL] [--all] [--tensorize/--no-tensorize]
```

- **Without `--all`:** downloads `--model` or the configured model, plus its draft model if any.
- **With `--all`:** every `compatible_gb10` matrix entry.
- **`--tensorize`:** converts to tensorizer format after the download. The default is off.

---

### 4.15. `ragon-admin codex`

```bash
ragon-admin codex build [--force] [--no-audit]
ragon-admin codex start
ragon-admin codex stop
ragon-admin codex test [-E FILTER] [--test-threads 8] [--jobs 6] [--memory-max 24G] [--accept-snapshots]
```

- **`build`:** builds `ragon` and `codex-code-mode-host` (plus, on Linux, `ragon-signal` and `ragon-delegate`, installed beside `ragon`) with `CodexBrandedBuilder`, the web commands (`ragon-search`, `ragon-fetch`), `ragon-code` and (on Linux) `ragon-docs`, and links `ragon`, `ragon-admin`, `ragon-search`, `ragon-fetch`, `ragon-code` and `ragon-docs` into `~/.local/bin`. It refuses while a Night Shift run holds its lock. It skips the build when the recorded build key is current, unless `--force`. See `DREAMFERENCE_RAGON_CODEX.md`.
- **`start` / `stop`:** run `ragon app-server daemon start|stop`, building `ragon` first if it is missing. (The help's "Codex comic server" typo is fixed: it now reads "ragon's app-server daemon".)
- **`test`:** runs Codex's own tests with `cargo-nextest` on the patched export, inside a memory-capped scope (`--memory-max`). `-E`/`--filter` takes a nextest filterset; `--accept-snapshots` rewrites the selected TUI snapshots and keeps those that differ from upstream's by the name alone. See `DREAMFERENCE_RAGON_CODEX.md`.

---

### 4.16. `ragon-search`, `ragon-fetch`, `gmail`

These are the commands the `ragon` agent is told to use in its prompt.

```bash
ragon-search "QUERY" [-n 5] [--json]                # local SearXNG
ragon-search --read "QUESTION" [--pages 3]          # search and read the top pages (1-5), numbered sources
ragon-fetch URL [--max-chars 8000] [--json]         # readable text of one page
ragon-admin gmail search "GMAIL QUERY" [-n|--max-results 10] [--json]   # at most 20
ragon-admin gmail read MESSAGE_ID [--max-chars 8000] [--json]
ragon-admin gmail status [--json]
```

`gmail` talks to the Google service `dreamference-gmail` (started by `ragon-admin google start`, §4.27), and is read-only. See `DREAMFERENCE_RAGON_GMAIL.md`.

Search and fetch are programs of their own rather than `ragon-admin` subcommands: they are the agent's most frequent commands, and `ragon-admin` administers the machine. Both are Rust binaries from `ragon-web-rs/` (`DREAMFERENCE_RAGON_CODEX.md` §4.1), not console scripts, so they work from a shell with no virtualenv; `ragon-admin codex build` installs them beside `ragon` and links them into `~/.local/bin`. They were `ragon-admin search` and `ragon-admin fetch` until recently; both subcommands are gone rather than aliased.

---

### 4.17. `ragon-admin clear model-cache`

`ModelDownloader.clear_cache()` removes exactly two directories: the HuggingFace hub (`~/.cache/huggingface/hub`, or `$HF_HOME/hub`) and the tensorizer cache (`~/.cache/dreamference/tensorizer`). Their parents are left alone: `~/.cache/huggingface` holds the HuggingFace login token, and `~/.cache/dreamference` holds vLLM's torch.compile cache, the `ragon` build cache (`ragon-codex/`, kept under its original name so Cargo's target directory is not discarded), fonts and test logs. Originally it deleted both parents.

Containers write parts of these caches as root, which `rmtree` cannot remove as the user. When a directory survives, the command says so, prints the `sudo rm -rf` that would finish the job, and exits 1 rather than reporting success.

### 4.18. `ragon-admin clear tensorize-cache`

`ModelDownloader.clear_tensorizer_cache()` removes `~/.cache/dreamference/tensorizer` only, with the same report-and-exit-1 behaviour for root-owned leftovers. Originally it removed the parent, all of `~/.cache/dreamference`, including vLLM's compile cache (a cold recompile costs 8–12 minutes) and the `ragon` build cache.

---

### 4.19. `ragon-admin images` and `voice`

```bash
ragon-admin images start [--no-siglip] | stop | status | mcp
ragon-admin voice start | stop | status
```

`images` runs the image search sidecar (and the SigLIP pre-filter, best-effort) on `dreamference-sidecars`, replacing containers found on another network; `images mcp` is the `image_search` tool over stdio, which the launcher declares for a session (ASK §6). `voice` runs speech-to-text (speaches on the CPU, `127.0.0.1:8100`) for `ragon web`'s `/api/transcribe` and the app (ASK §7).

### 4.20. `ragon-admin desktop`

`install`, `run`, `build`, `status` for the Electron desktop app (binary `ragon-app`, project `desktop/electron/`). `install` checks Node 22.13+ (Forge 8's minimum) and npm (and `dpkg` for the `.deb`; no `fakeroot` since Forge 8) and, on Ubuntu, writes the dev AppArmor profile Chromium's sandbox needs, with sudo; `build` makes the `.deb`; `run` opens it. `ragon app` opens the same app. On a release install the `.deb` is the app. See `DREAMFERENCE_RAGON_DESKTOP_ELECTRON.md` and `docs/dev/desktop.md`.

### 4.21. `ragon-admin night`

```bash
ragon-admin night enable [--window HH:MM-HH:MM]     # default: [night] window, 01:00-07:00
ragon-admin night disable
ragon-admin night status
ragon-admin night run [--until HH:MM] [--minutes N] [--idle-minutes N] [--ignore-open-sessions]
ragon-admin night every add (--daily | --weekly DAY | --days DAYS | --every N | --monthly D) [--test CMD] TASK...
ragon-admin night every list | drop ID
ragon-admin night archive [--weeks 4]
ragon-admin night autofill [--dry-run] [--max N]
ragon-admin night pause [--for DURATION]           # default 1h; 90m, 2h, 45s, or minutes
ragon-admin night resume
```

Night Shift (`dreamference/night_shift/`, `DREAMFERENCE_RAGON_NIGHT_SHIFT.md`). Tasks are queued from `ragon` with `/night add [--test CMD] [--on NODE] [--best-of N]` (or `ragon night add …` from a shell; `--best-of`, the task's own attempt count over `[night] best_of`); these commands run them.

- **`enable` / `disable`:** install or remove the systemd user timer `ragon-night.timer` that starts `night run` at the window's start.
- **`status`:** the timer, the window and the queue of every repository, and what the model gate is doing.
- **`every`:** repeating tasks (`NightShiftRecurring`), queued by each night's run when due (`add` with `--daily`, `--weekly`, `--days`, `--every N` or `--monthly D`; `list`; `drop ID`).
- **`archive`:** moves finished runs' records, transcripts and reports older than `--weeks` (default 4) to `night/archive/` (`NightShiftArchive`); branches stay.
- **`autofill`:** queues up to 5 bounded tasks from QA findings, failing CI tests and open spec items (`NightShiftAutofill`; also runs at the start of `night run` when `[night] autofill = true`).
- **`pause` / `resume`:** while a SWE-bench run holds the model gate, it refuses every other request (`RAGON_SWE_BENCH.md` §18). `pause` lets everything through until `--for` has passed (default one hour; a second `pause` extends it from now and keeps one interval); the run starts no new instance meanwhile while others use the model, as before the gate, and its report names the interval. `resume` ends it early. A pause given with no run holding the gate also covers a run that starts before it ends.
- **`run`:** works through the queue now, until the window ends (`--until`, or `--minutes` from now). It waits for the model server to have been idle for `--idle-minutes` (default 10). `--ignore-open-sessions` skips the wait for open `ragon` sessions; it is for testing.

While a night run holds its lock, `server start`, `codex build` and `index` refuse to run. A SWE-bench run takes the same lock (§4.23), so the two exclude each other and the refusal names whichever holds it.

### 4.22. `ragon-admin code setup`, `ragon-admin searxng start`, `ragon-admin searxng status`

- **`code setup`:** installs the pinned, checksum-verified tools the code index runs, and records the toolchains (Go, JDK 17+, .NET SDK 8+, and Maven and Gradle when installed) the optional indexers need. The index itself is the Rust binary `ragon-code`, built and linked by `codex build`. See `DREAMFERENCE_RAGON_CODE_INDEX.md`.
- **`searxng start`:** creates the SearXNG container on the project network `dreamference-sidecars`, published on `127.0.0.1:8888` only. A container found on Docker's default bridge is replaced. See `DREAMFERENCE_DOCKER.md` §6. On a node that `node enable` advertised (§4.25) it is published beyond loopback instead.
  It first writes `~/.config/searxng/settings.yml` (`SearxngSettings`): the JSON format, the limiter off, every engine that answered from this machine, each in its own category, and none run by a Russian or Chinese company (removed outright; the list and the owners are in `DREAMFERENCE_RAGON_AIRGAPPED.md` §14.10), with suspensions of 15 minutes for "too many requests" and "access denied". The file carries a version line; a file with it is rewritten when the version is newer, keeping its `secret_key`, and the running container is restarted to read it. A file without it is the user's own and is left alone.
- **`searxng status`:** each general engine's state, from the container's logs since it started: `suspended` until when and why (SearXNG logs every suspension with its length), `failing` (an error in the last 15 minutes, for an engine that is not suspended, such as a CAPTCHA with a zero-second suspension), `not loaded` (its start-up failed), `ok` (no error logged recently; SearXNG does not log successes) or `off`. It sends no search. Exit 0 when a web engine is ok, 1 when none is, 2 when SearXNG is not running.

### 4.23. `ragon-admin swe-bench`

```bash
ragon-admin swe-bench setup [--dataset verified|lite|full|multilingual|<HF id>] [--validate [--instances IDS] [--limit N] [--force]]
ragon-admin swe-bench build-images [--dataset multilingual] [--instances IDS | --subset FILE] [--languages L] [--limit N] [--force] [--validate]
ragon-admin swe-bench smoke [--idle-minutes N] [--ignore-open-sessions]
ragon-admin swe-bench run [--dataset D] [--instances IDS | --subset FILE] [--limit N] [--name NAME] [--shard I/K]
                           [--eval [--remove-images] [--image-chunk N]] [--code-index off|universal|exact] [--prompt <name>]
                           [--mask off|on] [--strip-names] [--refine [--refine-version v1|v2]] [--task-rules tests|tests-v2|issue-v1[,…]] [--review-turn]
                           [--hooks issue-v1] [--restart-after-compactions N] [--turn-end-compaction on|off|1-99]
                           [--apply-patch off|function|freeform] [--exec-tool function|freeform] [--compat claude|off] [--drop-old-outputs]
                           [--loop-guard] [--output-budget TOKENS] [--sampling default|nonthinking]
                           [--test-guard] [--finish-check]
                           [--until HH:MM] [--idle-minutes N] [--ignore-open-sessions] [--label TEXT]
ragon-admin swe-bench eval [RUN] [--drop-test-hunks] [--remove-images] [--image-chunk N]
ragon-admin swe-bench report [RUN] [--against RUN] [--drop-test-hunks]
ragon-admin swe-bench status
ragon-admin swe-bench clean [RUN] [--images]
ragon-admin swe-bench shard LIST --shards K [--out-dir DIR] [--dataset verified] [--balance-by OTHER]...
ragon-admin swe-bench merge NAME RUN...
```

SWE-bench on this machine (`dreamference/swe_bench/`, `DREAMFERENCE_RAGON_SWE_BENCH.md`). `RUN` defaults to the latest run.

- **`setup`:** installs the upstream harness (`swebench` 5.0.2) in a virtualenv of its own, downloads the dataset and builds the relocated copy of `ragon` that starts inside the instance images. `--validate` also checks which instances grade correctly here (the reference patch resolves, a no-op patch does not), which pulls their images.
- **`build-images`:** builds the arm64 instance images of a dataset nobody publishes them for (`multilingual`), from its task repository at the pinned commit (`SweBenchImageBuilder`).
- **`smoke`:** proves the whole pipeline on five fixed instances. `run` refuses until a smoke has passed with the installed harness version.
- **`run`:** the agent phase. One `ragon exec` per instance, each inside that instance's own container on the internal Docker network `ragon-swe-bench`, which reaches the model server and nothing else. It writes `predictions.jsonl`. A run with an existing `--name` is resumed; `--shard I/K` runs only the I-th of K balanced shards. `--eval` grades when the agent phase ends; `--remove-images` then works one repository at a time in chunks of at most `--image-chunk` instances (default 25) and removes a chunk's images once graded. `--code-index universal` indexes each instance's repository on the host and gives the agent `ragon-code` (default `off`). `--prompt <name>` starts the agent under that system prompt (`default`, `offline`, `high-swe`, or a custom one from `$CODEX_HOME/system-prompts/`, mounted read-only); without it the configured one. The manifest records it, with the custom file's SHA-256, and `report --against` names it when two runs differ. The arms: `--code-index exact` gives the SCIP stores alone and no graph; `--mask on` masks old tool outputs in the agent's requests (RAGON_CONTEXT_BUDGET §4.1); `--strip-names` takes the files, modules, functions and classes the reference fix touches out of the issue; `--refine` runs two sessions per instance, one that studies the issue and writes a refined description without changing the repository and a fresh one that fixes it (RAGON_REFINE), and `--refine-version v2` gives both steps refine-v2's texts instead of the measured v1 (RAGON_REFINE §10; refused without `--refine`, recorded in the manifest as `refine_version`); `--task-rules tests` adds rules to the task prompt (never change an existing test, keep your own scripts in `/tmp`, compare failing tests by name with and without the change). Rules stack, comma-separated (`tests-v2,issue-v1`); `--review-turn` resumes the agent's session once more to review and test its diff before the patch is collected (RAGON_SWE_BENCH §19). `--hooks issue-v1` enforces the issue-v1 rule through Codex hooks (the first edit held until the files and functions the issue names were read, the first stop held if the issue's example was not run since the last edit). `--restart-after-compactions N` stops the fix session at its N-th compaction and starts one fresh session with the task and the stopped diff as a file (default 0, never). `--turn-end-compaction` sets Codex's `model_post_turn_compact_threshold_percent` on the exec command line (`on`, `off` or a percentage; absent, exec never compacts at a turn's end). `--apply-patch` and `--exec-tool` choose how `apply_patch` and Code Mode's `exec` are offered (`function`, which SGLang serves, or `freeform`, which only ragon-engine serves; absent, the launcher chooses). `--drop-old-outputs` drops older tool outputs before a compaction summarises. `--loop-guard`, `--output-budget TOKENS` and `--sampling default|nonthinking` switch on the loop detector, response token cap and sampling arm (`docs/dev/loop-guard.md`). `--test-guard` and `--finish-check` switch on the launcher's guards in every session (RAGON_SWE_BENCH §29): a change that weakens an existing test is pointed out and the first stop while it stands held once; the first stop after a source change no test has run on, one that ends mid-work, and one that calls failures pre-existing unchecked are held once each; the report prints a `Guards` line and, in every run, a `Weakened tests` line. While it runs it holds the model gate: any request that does not come from its network is refused with HTTP 503 naming the run, `--label` (default `SWE-bench run <name>`), its progress and the time left (`RAGON_SWE_BENCH.md` §18).
- **`eval`:** the grading phase: the upstream harness applies each patch and runs the tests. It needs no model. `--drop-test-hunks` grades the same predictions again with every test file left out of each patch, as a separate grading (`eval-drop-test-hunks/`); `--remove-images` grades one repository at a time in chunks of `--image-chunk` and removes its images.
- **`report`:** the resolved rate and what it was measured with; `--against` compares two runs instance by instance; `--drop-test-hunks` reports that second grading.
- **`status`:** runs, their progress, the images present, free disk, and what the model gate is doing.
- **`clean`:** removes a run's containers and scratch (every run's, with no `RUN`); `--images` also removes the instance images.
- **`shard` / `merge`:** split a task list into `K` balanced shards (`SweBenchShards`) and merge the finished shard runs into one run (`SweBenchShardMerge`).

Files: the harness, dataset, runtime and validation results under `~/.cache/dreamference/swe-bench/`; one directory per run under `~/.local/share/dreamference/swe-bench/runs/`. Settings come from the `[swe_bench]` table (§5.1). `run` and `smoke` use Night Shift's admission (idle model, memory) and its runner lock.

### 4.24. `ragon-admin audit egress`

```bash
ragon-admin audit egress [--tui | --app | --web | --docs | --sync | --compat claude] [--prompt PROMPT] [--json]
```

Runs one real `ragon exec` session under `strace`, in a throwaway repository with a throwaway `CODEX_HOME`, and prints every network destination, every name asked of a resolver and every process the session started, with a verdict (`dreamference/audit/`, `DREAMFERENCE_RAGON_EGRESS.md`). `--tui` traces the full-screen interface on a pseudo-terminal instead of `ragon exec` (needs `pexpect` and `pyte`); `--app` the desktop app with both windows hidden, on the display `DISPLAY` names; `--web` `ragon web serve` answering one Ask thread; `--docs` the document index (`ragon-docs index` and `search` over a fixture folder must reach nothing); `--sync` traces one `ragon sync agent --once` cycle against a throwaway hub (`SyncEgressAudit`). `codex build` runs the audit after installing a new binary unless `--no-audit`. `--prompt` replaces the default prompt (a one-word reply) and is refused with `--docs` and `--sync`, which run no session; `--json` also writes the full result to `$CODEX_HOME/audit/<timestamp>-<session>.json` (`exec`, `tui`, `app`, `web`, `docs`, `sync` or `claude`). Exit status: 0 on a pass, 1 on an unexpected destination, 2 when the trace itself failed. `--compat claude` traces the compatibility mode's print mode, `ragon --compat claude -p "<prompt>" --output-format json`, in a throwaway repository that holds a `CLAUDE.md`, a `.claude/settings.json` with one hook and one permission rule, a `.claude/commands/` file and a `.claude/agents/` file, so every path of the mode that reads them runs inside the trace (`dreamference/audit/compat_print_session.py`). It counts as answered when its last line is a successful `result` with an answer in it. `codex build` also runs this trace after the other two when the user's new sessions start in the mode (`DREAMFERENCE_RAGON_COMPAT=claude` or `ragon_compat = "claude"`), and writes the record as `<timestamp>-claude.json`.

### 4.25. `ragon-admin node`

```bash
ragon-admin node enable [--no-web] [--yes] [--no-forge]
ragon-admin node disable
ragon-admin node status [NAME]
ragon-admin node id
ragon-admin node list
ragon-admin node add NAME [--user USER] [--ssh-port 22]
ragon-admin node remove NAME
ragon-admin node set NAME --model MODEL
ragon-admin node start NAME | stop NAME
ragon-admin node sync-model NAME MODEL [--address ADDR]
ragon-admin node power-cycle NAME [--off-seconds 20] [--wait-minutes 10] [--force]
ragon-admin node power-config {plug PLUG --host HOST [--kind shelly] [--channel 0] [--generation 2] | node NAME --plug PLUG [--via NODE] | show}
ragon-admin node run NAME [--gpu] [--memory 8G] [--time 90m] [--test CMD] [--setup CMD] [--out DIR] [--bind PATH]... -- COMMAND...
ragon-admin node jobs [NAME] | logs JOB | cancel JOB | fetch JOB
ragon-admin node provision [HOST…] [--all] [--user U] [--model M] [--from this|release[=X.Y.Z]] [--per-host-password]
                          [--web] [--no-start] [--no-forge] [--restart] [--os-update] [--dry-run] [--via ADDR] [--match PATTERN]
                          [--start-timeout 1200] [--model-source copy|hf] [--role night|engine] [--mesh]
sudo ragon-admin node prepare
```

The node half of the client/server split (`dreamference/node/`, `DREAMFERENCE_RAGON_NODE.md`).

- **`enable`:** installs the Avahi service file that advertises this machine as `_ragon-node._tcp`, publishes the web UI and SearXNG beyond loopback, sets up the node's Forgejo forge unless `--no-forge` (§4.30), and records that it did, so a later `ragon configure` or `searxng start` keeps those addresses. `--no-web` keeps the web UI on this machine; clients then get `ragon` and web search only. Root is needed once, for the file under `/etc/avahi`: the command is printed and `sudo` prompts on the terminal; `--yes` runs it with `sudo -n` and never waits for input (what `install.sh` uses). If the file cannot be installed, nothing is published. It prints that anyone on the local network can then use the node, with nothing encrypted or authenticated, and that the web UI's one account (and its Gmail tool) is shared.
- **`disable`:** stops advertising and puts the web UI and web search back on this machine only.
- **`status`:** the node id, what is advertised and published, and what a browse of the network returns; with a name, that paired node's status. `id` prints this machine's node id, creating it if absent.
- **`enable`** also warns, without stopping, when Wi-Fi power saving is on (it lost up to ~80% of mDNS queries on the GB10s' MT7925).
- **`list`:** every node on the local network, with its model, its load and whether it is paired. It needs no pairing: those figures come from each node's open model port.
- **`add`:** pairs with another node over SSH, once, so it can be managed from here. `NAME` is the node's name, address or id as `list` shows it. It makes a key used for nothing else, and the other node authorises it for one forced command only, with no terminal and no forwarding; the node's host key is pinned to its id.
- **`remove`:** unpairs: removes the key on both sides.
- **`set` / `start` / `stop`:** assign a model (a key of that node's model matrix) and start it there, or start or stop a paired node's model server. Each is carried out by that node's own `ragon-admin`, with its own host-safety checks.
- **`list`** also shows each node's free memory, for this machine and for paired nodes (asked over the pairing; memory is not on the open model port).
- **`sync-model`:** copies a model's Hugging Face cache folders (and its drafter's) to a paired node over the pairing, so it need not download them; `--address` uses another address of the same node, such as its QSFP link's. The node accepts only folders of a key of its own matrix, checks each weight file's checksum, and keeps files it already has.
- **`power-cycle` / `power-config`:** configure which Shelly smart plug powers which paired node (`~/.config/dreamference/node-power.json`, `NodePowerSettings`) and switch a hung node's plug off and on again (`NodePowerCycle`, `NodeShellyPlug`).
- **`run`:** runs a command on a paired node, in this repository at `HEAD`, in a sandboxed, memory-capped and time-limited unit there; its changes come back as the branch `job/<id>`. `--setup` builds an environment once per lock-file content and binds it read-only into later jobs; `--out` names a folder that comes back as files to `~/.ragon/jobs/received/<id>/`, never committed; `--bind` binds a path on the node read-only, if the node's `[node] bindable` allows it. `--gpu` asks for the GPU, which nodes refuse for now. `jobs`, `logs`, `cancel` and `fetch` follow a job; a finished job is pruned on the node a day after it was fetched, or 14 days after it finished.

- **`provision`:** sets up more GB10s from this one (`DREAMFERENCE_RAGON_FLEET.md`): with hosts (names, addresses or paired nodes) it logs in with the account's password, runs `node prepare` there with one `sudo`, installs Ragon from this machine's build (`--from this`, the default) or a release, copies the model over the LAN (`--model-source copy`, the default, or `--model-source hf` to fetch public weights from Hugging Face verified by SHA-256 and send only release-hosted files from here), pairs and starts the node (and its forge unless `--no-forge`; `--role night|engine` also applies that role's setup via `NodeRoleProvisioner`); with none it lists unprovisioned GB10s on the network (`--match` changes the name pattern from `spark-`/`gx10-`/`zgx-`); `--all` re-runs it on every paired node, which is the fleet update. One password serves every host unless `--per-host-password`; `--dry-run` connects and reads only; `--via` copies over another address (a QSFP link's); `--os-update` runs NVIDIA's OS and firmware update first, with a reboot; `--web` also shares the web UI (`ragon web`) on the LAN there, through `node enable` without `--no-web`; `--no-start` leaves the model server stopped, `--restart` restarts a running one. `--mesh` (every node paired with every other) is not built. `/node provision` in the TUI runs the same command (RAGON_NODE §18.9).
- **`prepare`:** run with `sudo` on a new GB10 (by hand, or by `provision`): every root step of a node install and nothing else, for the user who invoked sudo: the host settings, the `docker` group, lingering, an empty Avahi service file owned by the user, bubblewrap's AppArmor profile, NVIDIA's telemetry service disabled, and Wi-Fi power saving off (exactly `nmcli connection modify <conn> 802-11-wireless.powersave 2` and `iw dev <if> set power_save off`, neither of which reconnects). It never touches sshd, any other network setting, the firewall, users, APT sources or other NVIDIA services.

Four more subcommands are not typed by a person: `node job-exec <job>` (run inside a job's systemd unit to carry out that job), `node authorize` (run by `node add` on the other node; it reads a public key on standard input) and `node serve-job [--key TAG]` (the forced command sshd starts for a paired key; it refuses anything but info, status, start, stop, set-model, model-receive, unpair, the two git services for a job repository, and the job and night-task requests), and `node askpass` (answers ssh's password prompt during a `provision` run that holds the passwords). There is no primary node: the machine `ragon-admin node …` is typed on is the one doing the managing. Pairing was first run against a scratch sshd on loopback; it has also run between two GB10s (RAGON_NODE §18.10).

The switches are kept in `~/.config/dreamference/node-advertise.json`, not in `dreamference.toml`, because that file is resolved from the working directory first. On a client the counterpart is `ragon node list|use|forget|join`, in the launcher.

### 4.26. `ragon-admin host`

```bash
ragon-admin host check     # what `server start` would refuse over, and Wi-Fi power saving (blocks no load); changes nothing; exit 1 if anything is listed
ragon-admin host setup     # applies it: each command printed, then run through sudo
ragon-admin host setup --yes   # the same with sudo -n: never waits for input (install.sh on a node)
```

`HostSafetySetup` applies what `check_host_safety()` (§4.6) only prints: sysstat, an armed earlyoom, 64 GB of swap, and the two sysctls; also Wi-Fi power saving off on each connected Wi-Fi interface (`nmcli … 802-11-wireless.powersave 2`, plus `iw … power_save off` for immediate effect; Ethernet only or no NetworkManager: nothing is said). It also covers the agent's prerequisite: on Ubuntu (`kernel.apparmor_restrict_unprivileged_userns = 1`) it installs `/etc/apparmor.d/ragon-bwrap`, a profile granting `userns` to `/usr/bin/bwrap` alone, so Codex's sandbox works from a systemd unit, the Night Shift timer or an SSH job, not only from a terminal (`sandbox_prerequisite.py`). Every other `ragon-admin` run checks that sandbox from a throwaway user unit (~25 ms) and, when it fails, offers to fix it, to turn Night Shift off, or to ask again later; `mcp`, `host` and `node serve-job` never ask. Loaded and verified on this machine. The cases it reports instead of acting on, and what was verified, are in [SETUP §3.3](./DREAMFERENCE_SETUP.md). `install.sh` and `scripts/install_gb10.sh` both run `host setup`.

**On a release install** (no checkout; [SETUP §3.2](./DREAMFERENCE_SETUP.md)) `codex build` builds nothing: it reports the installed release binaries and refreshes the links, or says how to install them; `desktop build` and `desktop install` point at the release's `.deb`.

### 4.27. `ragon-admin google`

```bash
ragon-admin google start     # the Google service on 127.0.0.1:8767; adopts the web UI's container if it exists
ragon-admin google stop      # removes the container; connected accounts stay stored
ragon-admin google status    # whether it runs, and which accounts hold which apps
```

The Google service (`dreamference-gmail`, `chat/google_service.py`) holds the Google tokens and answers the connect pages and the read-only Gmail, Drive and Calendar endpoints that `/apps` and `ragon-admin gmail` use. Before it, only `chat configure` created it, so a node without the web UI had none. It is created on `dreamference-sidecars` (DOCKER §6), runs as the invoking user, and is also started by `server start` on a node when it is absent. See `DREAMFERENCE_RAGON_APPS.md` §5.1.

### 4.28. `ragon-admin matrix`

```bash
ragon-admin matrix start | stop | status
ragon-admin matrix add-user NAME      # an account for the phone, allowed to talk to Ragon
ragon-admin matrix push on|off        # whether push notifications (event ids only) may leave the machine
ragon-admin matrix remove [--yes]     # the homeserver, every account and every message
```

The private Matrix homeserver behind `ragon chat`'s Matrix adapter (`chat/matrix_homeserver.py`, `DREAMFERENCE_RAGON_CHAT.md` §5): tuwunel pinned by digest, on an internal Docker network with no route out, federation off, reached through a loopback proxy and `tailscale serve`. `start` brings up all three; `stop` stops it and keeps accounts and messages, and disables the units and `tailscale serve`, so it stays off across a reboot; a missing `tailscale` is reported as a failed step and does not stop `stop` or `remove`. Off by default: nothing else starts it.

### 4.29. `ragon-admin adapter`

```bash
ragon-admin adapter list [--available] [--json]
ragon-admin adapter install [NAME] [--repo <org>/<repo>@<commit>] [--third-party] [--hf-token TOKEN]
ragon-admin adapter remove NAME
ragon-admin adapter verify [NAME]
```

Installs and manages LoRA adapters for the served base model on a node (`cli/adapter_command.py`, `DREAMFERENCE_RAGON_ADAPTERS.md` §3). Only `install` uses the network; `list`, `verify` and `remove` read local files only. Installed adapters are served by SGLang at the next `ragon-admin server start` and listed in `/v1/models` as `<base>+<name>` for `ragon --adapter <name>` or `/adapter <name>`.

### 4.30. `ragon-admin forge`

```bash
ragon-admin forge status | on | off
ragon-admin forge move [NODE] [--back] | promote [NODE]
ragon-admin forge backup | restore --check
ragon-admin forge key add FILE | password --show
ragon-admin forge github-mirror (--all-branches | --off) [--account ACCOUNT]
ragon-admin forge import SOURCE NAME [--anonymity full|denylist|off] [--exempt PATH]... [--mirror]
ragon-admin forge archive NAME BUNDLES...
ragon-admin forge mirrors sync
```

Manages the node's Forgejo instance (`forge/forge_command.py`, `DREAMFERENCE_RAGON_FORGEJO.md` §7): the first node is the forge of record and paired nodes are read-only mirrors. `run` and `tick` are run by the forge's systemd user unit and timer.

### 4.31. `ragon-admin remote`

```bash
ragon-admin remote setup DNS_NAME [--yes] [--ssh DESTINATION] [--ca-scope DOMAIN] [--box-only]
ragon-admin remote status | peers | revoke PEER
ragon-admin remote remove [--purge] [--yes]
ragon-admin remote code [--bundle-file PATH [--ttl DURATION] [--join [--name NAME] [--ssh-user USER]]]
ragon-admin remote certificate {issue [--staging] | relay [--staging] [--name NAME] | relay-off | renew | off}
```

Reaches the node from outside the LAN over a NetBird overlay whose control plane and private CA run on the node, with one rented relay box configured over SSH (`remote/remote_access.py`, `DREAMFERENCE_RAGON_REMOTE_ACCESS.md`).

---

## 5. Configuration Hierarchy

Every field resolves in `DreamferenceConfig.__init__`, highest priority first:

1. **Constructor arguments**: the CLI passes `--model`, `--agent`, … through them.
2. **Environment variables**: `DREAMFERENCE_*`, `HF_TOKEN`.
3. **Config file**, resolved by `ConfigPathResolver`, first match wins:
   1. `--config`;
   2. `DREAMFERENCE_CONFIG_PATH`;
   3. `./dreamference.toml` or `./dreamference.json`;
   4. `~/.config/dreamference/config.toml`.

   A repository's own `./dreamference.toml` or `./dreamference.json` may set no key that chooses a host, port, URL or other network address (`NETWORK_KEYS`, today only `vllm_host`), not even one on this machine (owner decision; RAGON_CODEX §4): the key is ignored with one line on stderr naming the file and the key, and the user's file, the environment or an argument decides. `save_config()` leaves the repository's line as it was and writes no such key there; `init` writes none. A file named by `--config` or `DREAMFERENCE_CONFIG_PATH` is the user's choice and may set them. Without it, a clone chose the model server for every `ragon-admin` command, and the agent runner passed that host to `ragon` as `DREAMFERENCE_VLLM_HOST`, which the launcher trusts.
4. **Built-in defaults**: the module-level `DEFAULT_*` constants.

### 5.1. Config File Example

The file is TOML; this one is the user's own (`~/.config/dreamference/config.toml`), since a repository's file may not set `vllm_host`:

```toml
vllm_host = "http://localhost:8000"
model = "qwen3.8-27b-nvfp4-dflash2"
diffusion_model = "tiny-a2d-coder-0.5b-diffusion"
agent_runner = "codex"
num_speculative_tokens = 8
enable_prefix_caching = true
enable_chunked_prefill = true
num_scheduler_steps = 8
attention_backend = "auto"
# kv_cache_dtype unset = use the model recipe's value
# server_engine unset = use the model's default ("sglang"; "ragon" opts into ragon-serve)
ragon_gmail = true
ragon_airgapped = "off"     # off | on
ragon_compaction_ledger = true    # hand the agent a rule-built ledger after each compaction
ragon_prompt = "default"    # the system prompt new sessions get (ragon prompt use <name>)
ragon_refine = false        # refine mode: study the task first, then solve it (RAGON_REFINE)
ragon_refine_version = "v1" # its texts: v1 (measured) or v2 (RAGON_REFINE §10)
ragon_compat = "off"        # claude | off: the compatibility mode (ragon compat use; read from this file only)
ragon_claude_md = false     # outside the mode: CLAUDE.md as Codex's fallback project document

[night]                      # Night Shift, read by `ragon-admin night` (NightShiftSettings)
window = "01:00-07:00"

[swe_bench]                  # read by `ragon-admin swe-bench` (SweBenchSettings)
max_parallel = 3
```

The tables take these keys, each with a built-in default:
- **`[night]`:** `window`, `max_parallel`, `task_timeout`, `test_timeout`, `task_memory`, `nudges`, `test`, `test_sandbox`, `airgapped`, `prompt`, `refine`, `refine_version`, `task_context`, `compact_at`, `compaction_audit`, `idle_minutes`, `index`, `index_timeout`, `nodes`, `best_of` (attempts per task, default 3; `8961650`), `autofill`, `autofill_max`, `autofill_repo`;
- **`[swe_bench]`:** `max_parallel`, `task_timeout`, `task_memory`, `task_cpus`, `nudges`, `idle_minutes`, `task_context`, `eval_workers`, `eval_memory`, `eval_timeout`, `disk_reserve`, `image_chunk`, `image_cache_lists`, `image_cache_floor`, `build_memory`, `build_cpus`, `nodes`;
- **`[node]`:** `bindable`, the folders a job sent to this node may bind read-only.

The launcher reads a few more top-level keys itself (`ragon_docs`, `ragon_context_window`, `ragon_adapter`, among others); `DREAMFERENCE_RAGON_CODEX.md` lists them.

A `sandbox = …` line left in an older file is ignored: the option was removed.

**Fixed: `save_config()` used to drop the `[night]` table.** It rewrote the file from `DreamferenceConfig`'s flat fields, so `ragon-admin main-model set`, `diffusion-model set` and `init` removed Night Shift's settings. `ConfigFileStorageManager.save_config_dict()` now carries over every table already in the file that the saved keys do not name; `test_saving_keeps_tables_other_readers_own` covers it.

`save_config()` deliberately writes only values that differ from the defaults, so a round trip does not fossilise defaults into the TOML. Originally `vllm_host` and `agent_runner` were the exception, written whatever their value; they now follow the rule (`test_a_default_host_and_agent_are_not_fossilised_into_the_config`). A file saved before that keeps its two lines until the next save.

---

## 6. Environment Variables

| Variable | Default | Description |
| :--- | :--- | :--- |
| `DREAMFERENCE_CONFIG_PATH` | (resolver order, §5) | Config file path |
| `DREAMFERENCE_VLLM_HOST` | `http://localhost:8000` | vLLM endpoint. Also read by `ragon`'s launcher. |
| `DREAMFERENCE_MODEL` | `qwen3.8-27b-nvfp4-dflash2` | Main model alias |
| `DREAMFERENCE_SERVER_ENGINE` | (unset: model's default, `sglang`) | Engine `server start` serves the model with (`ragon`, `sglang` or `vllm`); also `server_engine` in the config file and `server start --engine` |
| `DREAMFERENCE_DIFFUSION_MODEL` | `tiny-a2d-coder-0.5b-diffusion` | Diffusion sidecar model |
| `DREAMFERENCE_DRAFT_MODEL` | (unset) | Draft model alias |
| `DREAMFERENCE_SPECULATIVE_TOKENS` | `8` | Speculative token count |
| `DREAMFERENCE_AGENT` / `DREAMFERENCE_RUNNER` | `codex` | Agent runner (`codex`, `cline`, `continue`, `openhands`) |
| `DREAMFERENCE_USE_TENSORIZER` | `false` | Tensorize after download |
| `DREAMFERENCE_RAGON_ADAPTER` | (unset: base model) | LoRA adapter new `ragon` sessions start with (`ragon --adapter <name>` or `/adapter <name>`); also `ragon_adapter` in the config file (`ragon-rs/src/adapter.rs`) |
| `DREAMFERENCE_RAGON_GMAIL` | `true` | Add the Gmail section to `ragon`'s prompt when an account is connected |
| `DREAMFERENCE_RAGON_CONTEXT_WINDOW` | unset | The model's context window in tokens, overriding what the server reports (also `ragon_context_window` in the config file). Without either, `ragon` reads `max_model_len`, `context_length` or `max_context_length` from `/v1/models`, then llama.cpp's `/props` `n_ctx`, then its `n_ctx_train`, else assumes 32,768. Ollama reports none, so set it there |
| `DREAMFERENCE_RAGON_COMPAT` | `off` | The compatibility mode (`claude` or `off`; `--compat` on the command line sets it for the process and what it starts); also the config key `ragon_compat`, which `ragon compat use` writes and which is read only from `~/.config/dreamference/config.toml`, never from a repository's `dreamference.toml` or `DREAMFERENCE_CONFIG_PATH`. A value naming no mode stops the start, from the flag or the variable; from the file it is reported and the mode stays off. Fixed for a session's life (`DREAMFERENCE_RAGON_CLAUDE.md` §0.4) |
| `DREAMFERENCE_RAGON_CLAUDE_MD` | `off` | Outside the compatibility mode, makes a folder's `CLAUDE.md` Codex's fallback project document (`project_doc_fallback_filenames` gains `CLAUDE.md` for the session, read only where a folder has no `AGENTS.md`); also the config key `ragon_claude_md`. In the mode, `CLAUDE.md` comes through the memory hook instead (`DREAMFERENCE_RAGON_CLAUDE.md` §9.1) |
| `DREAMFERENCE_RAGON_PROMPT` | `default` | System prompt for new `ragon` sessions (`default`, `offline`, `high-swe`, `ask`, or a custom one in `$CODEX_HOME/system-prompts/<name>.md`); also the config key `ragon_prompt`, which `ragon prompt use` writes. An unknown name is skipped with a warning. A resumed session keeps the prompt it started with |
| `DREAMFERENCE_RAGON_AIRGAPPED` | `off` | How much of the internet a `ragon` session may use (`off`, `on`); also the config key `ragon_airgapped` |
| `DREAMFERENCE_RAGON_COMPACTION_LEDGER` | `true` | Register `ragon ledger` as the hook that runs after each compaction of a `ragon` session ([RAGON_COMPACTION §11](./DREAMFERENCE_RAGON_COMPACTION.md)); also the config key `ragon_compaction_ledger` |
| `DREAMFERENCE_RAGON_REFINE` | `false` | Refine mode for new tasks: a first session studies the task and writes a refined description without changing anything, then a fresh one does it ([RAGON_REFINE](./DREAMFERENCE_RAGON_REFINE.md)); also the config key `ragon_refine` |
| `DREAMFERENCE_RAGON_REFINE_VERSION` | `v1` | Which texts refine mode uses: `v1`, the measured ones, or `v2` ([RAGON_REFINE §10](./DREAMFERENCE_RAGON_REFINE.md)); also `ragon --refine-version v1\|v2` and the config key `ragon_refine_version`. A value that is not a version passes to the next tier |
| `DREAMFERENCE_RAGON_MASK` | off | Observation masking: old tool outputs replaced in the request by a placeholder naming a saved copy (patch `0021`, [RAGON_CONTEXT_BUDGET](./DREAMFERENCE_RAGON_CONTEXT_BUDGET.md)); also the config key `ragon_mask_tool_output`, with `[ragon_mask]` for the numbers |
| `DREAMFERENCE_RAGON_TURN_END_COMPACTION` | on | Compaction at the end of a turn; `exec` is left alone unless its command line sets the threshold (SWE-bench's `--turn-end-compaction`). `0` switches it off. Also the config key `ragon_turn_end_compaction` (`ragon-rs/src/compaction.rs`) |
| `DREAMFERENCE_RAGON_DROP_OLD_OUTPUTS` | off | Before a compaction summarises, drop older tool outputs first; also `ragon_drop_old_outputs`. SWE-bench's `--drop-old-outputs` sets it |
| `DREAMFERENCE_RAGON_TEST_GUARD`, `DREAMFERENCE_RAGON_FINISH_CHECK` | off | The guards (`ragon-rs/src/guard.rs`, [RAGON_SWE_BENCH §29](./DREAMFERENCE_RAGON_SWE_BENCH.md)): a change that weakens an existing test is pointed out and the first stop while it stands held once; the first stop after a source change no test has run on, one mid-work, and one calling failures pre-existing with no run on the unchanged code held once each. Also `ragon_test_guard`, `ragon_finish_check`. SWE-bench's `--test-guard`, `--finish-check` set them |
| `DREAMFERENCE_RAGON_COMPACT_SUMMARY_WORDS` | `800` | Length bound of a compaction summary in words; `0` is off (`ragon-rs/src/summary_cap.rs`) |
| `DREAMFERENCE_RAGON_APPLY_PATCH`, `DREAMFERENCE_RAGON_EXEC_TOOL` | the launcher's choice | How `apply_patch` (`off`, `function`, `freeform`) and Code Mode's `exec` (`function`, `freeform`) are offered; for benchmark arms (`swe-bench run --apply-patch`, `--exec-tool`) |
| `DREAMFERENCE_RAGON_IMAGES` | on when image search runs | `0`, `false`, `no` or `off` leaves the `image_search` tool out of a session (`ragon-rs/src/images.rs`) |
| `DREAMFERENCE_RAGON_CODE_TOOLS` | on | Offer the code index to the model as MCP tools; off leaves the shell commands and their prompt block. Also `ragon_code_tools` |
| `DREAMFERENCE_RAGON_DOCS` | on | Offer the document index (`docs_search`, `docs_read`) when `ragon-docs` is installed and a collection exists; also `ragon_docs` |
| `DREAMFERENCE_RAGON_DRIVE` / `_CALENDAR` | on when connected | Switch Drive or Calendar off for a session in `/apps`, as `DREAMFERENCE_RAGON_GMAIL` does Gmail ([RAGON_APPS](./DREAMFERENCE_RAGON_APPS.md) §7) |
| `DREAMFERENCE_SEARXNG_URL` | `http://127.0.0.1:8888` | SearXNG instance used by `ragon-search` and the MCP server's `web_search` |
| `CODEX_HOME` | `~/.ragon` | `ragon`'s home folder: sessions, config, skills (`skills/`), the Night Shift queue (`night/`), audit results (`audit/`) |
| `RAGON_NODE` | (unset) | On a client: the node one `ragon` command uses, by name, address or id, instead of the remembered one |
| `RAGON_RELEASE_REPO` | `dreamference/ragon` | Where `ragon update` looks for releases, e.g. a fork |
| `RAGON_RELEASE_API` | GitHub's API | Another API root for `install.sh` (the tests' stand-in server, or a draft release's) |
| `RAGON_INSTALL_DIR`, `RAGON_VENV` | `~/.local/share/dreamference/ragon`, `~/.local/share/dreamference/venv` | Where `install.sh` puts the binaries and `ragon-admin`'s virtualenv |
| `HF_TOKEN` / `DREAMFERENCE_HF_TOKEN` | (unset) | HuggingFace token |
| `HF_HOME` | `~/.cache/huggingface` | HF cache root (the hub cache is `$HF_HOME/hub`) |

The table lists what a user sets. Left out on purpose are the variables one component sets for another (`RAGON_NIGHT_RUN`, `RAGON_UPSTREAM_TESTS`, the `RAGON_CODE_*` paths the launcher, the indexers and the SWE-bench runner pass along, the `RAGON_GMAIL_*`, `RAGON_IMAGE_*`, `RAGON_SIGLIP_URL` and `RAGON_VISION_*` settings of the sidecar containers, `DREAMFERENCE_DIFFUSION_PORT` and `DREAMFERENCE_DIFFUSION_MODEL_ID` inside the diffusion container), the `DREAMFERENCE_RAGON_DOCS_PDF_*` tuning knobs of `pdf_search` (`ragon-rs/src/`), the build-time `RAGON_VERSION` the release workflow sets, and test seams (`RAGON_NIGHT_RAGON_BIN`, `RAGON_TEST_NODE_ID`). Each is described in the module that reads it.

**Tuning keys** (config file or CLI only, no environment variable):
- `enable_prefix_caching`;
- `enable_chunked_prefill`;
- `num_scheduler_steps`;
- `attention_backend`;
- `kv_cache_dtype` (default: unset, so the model recipe decides);
- `tool_call_parser` (default `hermes`, overridden by the recipe);
- `max_num_batched_tokens` (`8192`);
- `guided_decoding_backend` (`xgrammar`).

---

## See Also

- **[DREAMFERENCE_AGENTS.md](./DREAMFERENCE_AGENTS.md):** agent runners
- **[DREAMFERENCE_RAGON_CODEX.md](./DREAMFERENCE_RAGON_CODEX.md):** the `ragon` binary
- **[DREAMFERENCE_INFERENCE.md](./DREAMFERENCE_INFERENCE.md):** vLLM configuration
- **[DREAMFERENCE_DOCKER.md](./DREAMFERENCE_DOCKER.md):** model caching and Docker management
