# Ragon Architecture Overview

> - **Version:** 0.9.0 (`dreamference.__version__`, `setup.py`)
> - **Target Hardware:** NVIDIA GB10 (Blackwell SM121, 128 GB unified LPDDR5X)
> - **Deployment Model:** single node; the model, the code and the sessions stay on the machine. Web search, page fetch and Gmail reach the internet at the default `/airgapped off`; only `/airgapped on` allows none of them
> - **License:** AGPL-3.0-or-later
> - **Checked against the code** (packages and modules against the tracked source tree, the model matrix, the containers, the launcher's subcommands, the release assets)

---

## 1. Executive Summary

**Ragon** (by Dreamference) is a local agentic coding platform for a single NVIDIA GB10: inference runs on the machine and no code or prompt goes to a cloud model. It is not air-gapped by default, because the agent's web search, page fetch and Gmail tools use the internet; `/airgapped on` switches those off and takes the network away from the agent's sandboxed commands, with the holes `DREAMFERENCE_RAGON_AIRGAPPED.md` lists (see also `DREAMFERENCE_RAGON_EGRESS.md`). It serves one open model in Docker, Qwen3.8-27B on SGLang by default (with `ragon-serve`, `engine: ragon`, opt-in, and the vLLM launcher kept for vLLM recipes) behind the model gate (`ModelGate`), and puts these front ends on the same local standard /v1 endpoint:

- **`ragon`:** the terminal coding agent, and the default. It is a Ragon-branded build of Codex CLI with a Rust launcher compiled in that points it at the local model. It is built from a pinned fork (`codex/` submodule) plus small patches (`codex-patches/`) and the launcher crate (`ragon-rs/`).
- **`ragon web`:** Ragon's own web UI (`ragon-rs/web/`, the page in `desktop/ui/`), served on port 3100. **Ask** threads are `ragon` sessions under a bridge policy (the `ask` prompt, a scratch folder, uploads); **Work** drives `ragon app-server`. A credential is checked on every request, and phones and other devices are paired. See `DREAMFERENCE_RAGON_ASK.md`.
- **The desktop app `ragon-app`:** an Electron app (`desktop/electron/`) with `ragon` bundled inside: windows showing Ask and Work, all on its own one `ragon app-server` over stdio (RAGON_ASK §18.6; several windows, a terminal or a diff detachable, DESKTOP_ELECTRON §13). A `.deb` for Linux; a Mac `.dmg` as an unsigned preview. See `DREAMFERENCE_RAGON_DESKTOP_ELECTRON.md`.
- **Messengers, off by default:** Signal (`ragon signal`, linked to the owner's account, answering in Note to Self) and Matrix or Telegram (`ragon chat`, with the homeserver from `ragon-admin matrix`). Each is a paired client of `ragon web`, so every message becomes an Ask thread. See `DREAMFERENCE_RAGON_SIGNAL.md` and `DREAMFERENCE_RAGON_CHAT.md`.
- **Other agents:** Cline, Continue and OpenHands, through `ragon-admin run --agent …`.

The agent's tools beyond Codex's own are commands installed beside `ragon`: `ragon-search` and `ragon-fetch` (the web, through a local SearXNG), `ragon-code` (the code index, over MCP), `ragon-docs` (the user's own documents, over MCP) and `ragon-admin gmail`; `/apps` lists read-only Gmail, Google Drive and Google Calendar.

Everything is administered through **`ragon-admin`**, the Python CLI.

Now the GB10 can also be offered to the local network as a **node**: `ragon-admin node enable` advertises it over mDNS (`_ragon-node._tcp`), and `ragon` and `ragon-search` on another machine find it with no address typed. Clients are released for Linux (arm64 and x86-64), macOS and, as an unsigned preview, Windows (`install.ps1`). A second GB10 can join: the two are paired over SSH (`DREAMFERENCE_RAGON_NODE.md` §18.10). `ragon-admin node provision` sets up more GB10s from a node (`DREAMFERENCE_RAGON_FLEET.md`).

```text
+------------------------------------------------------------------------------------+
|  Front ends                                                                        |
|   ragon (Rust, Codex fork)   ragon web :3100 (Ask, Work) / ragon-app (Electron)       |
|   IDEs (MCP)   ragon signal / ragon chat (off by default)                          |
|   ragon-admin run --agent cline|continue|openhands                                  |
+-----------------------+--------------------------------+---------------------------+
                        | Standard /v1 HTTP              | stdio MCP (ragon-admin mcp)
+-----------------------v--------------------------------v---------------------------+
|  dreamference/ (Python, ragon-admin)                                                |
|   config  hardware  vllm_server  runner  chat  context_engine  mcp_server          |
|   night_shift  swe_bench  audit  node  forge  remote  cli                          |
|  Rust beside ragon: ragon-search, ragon-fetch, ragon-code, ragon-docs, ragon-signal      |
+-----------------------+------------------------------------------------------------+
                        | docker run
+-----------------------v------------------------------------------------------------+
|  Containers on the GB10                                                            |
|   dreamference-model-gate-8000 -> dreamference-vllm-8000 (SGLang or ragon-serve)    |
|   dreamference-gmail, -searxng, -image-search, -stt, -forgejo; -matrix (off)       |
+------------------------------------------------------------------------------------+
|  NVIDIA GB10 — Blackwell GPU + Arm CPU sharing 128 GB unified memory               |
+------------------------------------------------------------------------------------+
```

---

## 2. Models

The model matrix (`hardware/model_matrix_registry.py`, `MATRIX`) is the source of truth for launch flags. Per-model tuning lives in each entry's `launch_overrides` (and `engine_recipes` for alternate engines); a model may pin its own Docker image and name its engine (`engine: sglang`, `engine: ragon`, or `engine: vllm`).

| Alias | Weights | Role |
| --- | --- | --- |
| `qwen3.8-27b-nvfp4-dflash2` | `RadixArk/Qwen3.8-27B-NVFP4` | **The** main model (`DEFAULT_MODEL_ALIAS`); served by `ragon-serve` (`engine: ragon`) by default since 0.9.0, with SGLang the `--engine sglang` fallback (and the engine for adapters until ragon-serve serves them), with the DFlash2 drafter, a 196,608-token context for each of four conversations on ragon-serve (262K on SGLang), vision on SGLang |
| `qwen3.8-27b-minima-nvfp4-dflash2` | `minima-ai/mnma_qwen3.8_27b_nvfp4` | Trial candidate (all-NVFP4, SGLang + DFlash2, text-only; `DREAMFERENCE_MODELS.md` §2.2), not a default |
| `qwen3.8-flash-next-nvfp4-mtp` | `local-inference-lab/Qwen3.8-Flash-Next-NVFP4` | Trial candidate (125B + 51B n-gram + 4B MTP in NVFP4/MXFP8, vLLM; `DREAMFERENCE_MODELS.md` §2.3), not a default |
| `qwen3.8-27b-dflash2-draft` | `dreamference/qwen3.8-27b-dflash2-nvfp4-scalesearch` | Its DFlash2 drafter (searched-scale NVFP4), not served on its own |
| `tiny-a2d-coder-0.5b-diffusion` | `dllm-collection/Qwen2.5-Coder-0.5B-Instruct-diffusion-bd3lm-v0.1` | Default diffusion model, served by the sidecar (vLLM cannot serve diffusion checkpoints). **Not offered:** diffusion is switched off (`DIFFUSION_ENABLED = False`), so it is never started, downloaded or listed |

The Qwen 3.5 122B and Qwen 3.6 35B entries, their drafter and their vLLM images were removed; a removed alias is answered with the release that removed it and the command that switches to the main model. See `DREAMFERENCE_MODELS.md` and `DREAMFERENCE_INFERENCE.md`.

---

## 3. Packages

Each package's `__init__.py` is a re-export facade with an explicit `__all__`. The same-named top-level modules (`dreamference/config.py`, `cli.py`, …) that used to sit beside them, dead shims shadowed by the packages, were deleted (PYTHON_QUALITY §11).

### 3.1. `config/`: 4-tier configuration

Every field in `DreamferenceConfig.__init__` resolves, highest priority first:
1. constructor argument (the CLI);
2. `DREAMFERENCE_*` environment variable;
3. config file (`--config`, `DREAMFERENCE_CONFIG_PATH`, `./dreamference.toml`/`.json`, `~/.config/dreamference/config.toml`);
4. module-level `DEFAULT_*` constant.

`save_config()` writes only values that differ from the defaults, and carries over tables already in the file. Those tables belong to other readers: `[night]` (`NightShiftSettings`) and `[swe_bench]` (`SweBenchSettings`) are read from the same file by their own classes, not by `DreamferenceConfig`.

### 3.2. `hardware/`: models, downloads, adapters, telemetry

- `model_matrix_registry.py` / `model_spec.py`: the matrix, and `ModelSpec` (`supports_vision`, `is_diffusion`, `launch_overrides`, `engine_recipes`).
- `model_downloader.py`, `release_asset_downloader.py`: HF downloads, Dreamference release-asset downloads and tensorizer caching.
- `adapter_manifest.py`, `adapter_signature.py`, `adapter_verifier.py`, `adapter_store.py`, `adapter_catalog.py`, `adapter_installer.py`, `adapter_base_digest.py`: signed LoRA adapters (`DREAMFERENCE_RAGON_ADAPTERS.md`).
- `hardware_manager.py`, `hardware_telemetry.py`, `memory_metrics.py`: GB10 detection and memory figures.

### 3.3. `vllm_server/`: model serving, the model gate and host safety

- `vllm_server_manager.py`: builds the `docker run …` command from recipe plus config, starts, stops, removes, tails logs, and resets a stale torch.compile cache.
- `sglang_launch_builder.py`, `sglang_runtime_patcher.py`: what follows the image for a model whose recipe names `engine: sglang`, and host-side runtime patching of container Python files verified by SHA-256; `ragon_serve_launch_builder.py`: what follows the image for `engine: ragon` (`ragon-serve`); the `docker run` prefix, and so the host-safety layer, is shared across engines.
- `model_gate.py`, `model_gate_service.py`, `model_gate_adapters.py`, `model_gate_guard.py`, `adapter_serving.py`: the model gate container (`dreamference-model-gate-<port>`) in front of the engine, LoRA adapter routing, and the loop-guard/SWE-bench admission proxy.
- `chat_template_patcher.py`: patches a checkpoint's chat template on a copy at launch, from the registry entry's `chat_template_patches`; a non-matching anchor stops the start.
- `vllm_launch_options.py`, `tool_call_canary.py`, `wifi_power_save.py`: flag merging, post-boot `/v1/responses` tool-call check, and Wi-Fi power-save check.
- `model_loading_monitor.py`, `vllm_startup_monitor.py`, `vllm_log_streamer.py`, `vllm_server_status.py`, `diagnostics.py`: load progress and status.
- **Host safety:** unified memory means a bad load can freeze the whole host. Two layers guard against it:
  - `check_host_safety()` (`host_safety_setup.py`, `sandbox_prerequisite.py`) runs *before* the load (swap, sysctl, earlyoom/systemd-oomd);
  - `psi_watchdog.MemoryPressureWatchdog` runs *during* it, sampling `/proc/pressure/memory` and killing the container on sustained pressure.
- `diffusion_server_manager.py` / `diffusion_openai_service.py`: the diffusion sidecar. It runs in the main model's image, starts *before* vLLM, and is capped at `--memory=8g`. **Switched off** (`DIFFUSION_ENABLED = False` in `hardware/model_matrix_registry.py`): `server start` starts no sidecar and removes one an older Ragon left behind; the code and its tests are kept.

### 3.4. `runner/`: agents

Four runner/installer pairs (Codex, Cline, Continue, OpenHands; Goose and Aider were removed) plus `vllm_readiness_waiter.py`, `safe_git.py` and `docs_index_setup.py` (`ragon-admin docs setup`, the models `ragon-docs` loads). For Codex, the default, `codex_branded_builder.py` builds `ragon` from the submodule, patches and launcher, and `codex_test_runner.py` runs Codex's own tests on that tree (`ragon-admin codex test`). See `DREAMFERENCE_AGENTS.md` and `DREAMFERENCE_RAGON_CODEX.md`.

### 3.5. `chat/`: the sidecars, the desktop app and what the web chat left

- `docker_bridge.py`: the loopback-to-bridge rewrite a container needs to reach the model server (OpenHands).
- `gmail_search_service.py`, `google_workspace_reader.py`, `gmail_credentials.py`, `gmail_client.py`: the Google service (read-only Gmail, Drive and Calendar) and the Gmail client; `google_service.py` starts that service (`ragon-admin google start|stop|status`).
- `image_search_service.py`, `image_search_sidecar.py`, `image_search_mcp.py`: the image search sidecar, `ragon-admin images start|stop|status`, and the `image_search` MCP tool (`images mcp`); `speech_sidecar.py`: speech-to-text, `ragon-admin voice`.
- `searxng_sidecar.py`, `searxng_settings.py`, `searxng_status.py`, `searxng_engine_event.py`, `sidecar_network.py`: the SearXNG container (`ragon-admin searxng start`), its engine settings/status, and the user-defined network the sidecars are created on.
- `matrix_homeserver.py`: the private Matrix homeserver behind `ragon chat` (`ragon-admin matrix`), off by default.
- `desktop_runner.py` / `desktop_installer.py` / `desktop_protocol_types.py`: building and running the Electron app `ragon-app` (project in `desktop/electron/`, UI in `desktop/ui/`) and the app-server's TypeScript types for Work.

See `DREAMFERENCE_RAGON_ASK.md` §19, `DREAMFERENCE_RAGON_GMAIL.md`, `DREAMFERENCE_RAGON_APPS.md`, `DREAMFERENCE_IMAGE_SEARCH.md` and `DREAMFERENCE_RAGON_CHAT.md`.

### 3.6. `context_engine/`: workspace index

Python `ast` symbols (`ast_symbol_extractor.py`), TF-IDF, SQLite FTS5 and `nomic-embed-text-v1.5` embeddings stored as plain float32 blobs in SQLite (`sqlite_context_storage.py`, `embedding_calculator.py`; sqlite-vec is not used), all written to `.dreamference/`. It serves the web canvas, and `workspace_search_code` over MCP where the workspace has no `ragon-code` index (§3.7); `ragon` does not use it. See `DREAMFERENCE_CONTEXT.md`.

### 3.7. `mcp_server/`: IDE companion

A stdio MCP server (`ragon-admin mcp`) with `ide_*` tools over an in-process `IDEState`, plus `web_search` / `web_fetch` (`web_tools.py`) and `workspace_search_code`, which `code_index_search.py` answers from `ragon-code` when the workspace is indexed and the context engine answers otherwise.

### 3.8. `cli/`: `ragon-admin`

`dreamference_cli_controller.py` (`build_parser`, `run_cli`), `adapter_command.py` (`ragon-admin adapter`), `searxng_command.py`, `model_deep_inspector.py` (`main-model inspect --deep`), `sonnet_dataset.py` (for `benchmark_server`) and `code_index_setup.py` (`code setup`). See `DREAMFERENCE_CLI.md`.

### 3.9. `night_shift/`: the overnight queue

`ragon-admin night {enable,disable,status,run,pause,resume,archive,autofill,recurring}`: the queue the launcher writes (`night_shift_queue.py`), settings (`night_shift_settings.py`), host probes (`night_shift_host.py`), one task in its own git worktree (`night_shift_task_run.py`), best-of-N and chooser v1 (`night_shift_best_of_n.py`, `night_shift_chooser.py`, `chooser_candidate.py`), admission and scheduling (`night_shift_runner.py`), the code-index refresh before a repository's tasks (`night_shift_index.py`), the morning report and compaction audit (`night_shift_report.py`, `night_shift_compaction_audit.py`), the systemd user timer (`night_shift_scheduler.py`), tasks handed to another node (`night_shift_remote.py`), archiving, autofill and recurring tasks (`night_shift_archive.py`, `archive_outcome.py`, `night_shift_autofill.py`, `autofill_candidate.py`, `night_shift_recurring.py`), and refine mode's texts (`refine_prompt.py`). See `DREAMFERENCE_RAGON_NIGHT_SHIFT.md`.

### 3.10. `swe_bench/`: SWE-bench on this machine

`ragon-admin swe-bench {setup,smoke,run,eval,report,status,clean,shard,merge,build-images}` (`swe_bench_command.py`). The agent phase (`swe_bench_runner.py`, `swe_bench_instance_run.py`) runs one `ragon exec` per instance inside that instance's own container, on an internal Docker network that reaches only the model server (guarded by `swe_bench_gate_hold.py` and `ModelGate`), with a relocated copy of `ragon` (`swe_bench_runtime.py`); the upstream harness, in a virtualenv of its own (`swe_bench_harness.py`), validates instances and grades patches (`swe_bench_evaluator.py`). Images are third-party arm64 builds or built/cached (`swe_bench_images.py`, `swe_bench_image_builder.py`, `swe_bench_image_cache.py`); `--code-index universal` adds `ragon-code` as an arm (`swe_bench_code_index.py`), `--strip-names` takes the fix's names out of the issue (`swe_bench_name_stripper.py`), `eval --drop-test-hunks` regrades without the agent's test edits (`swe_bench_patch_filter.py`), and a relay reaches another node's model server (`swe_bench_relay.py`). It shares Night Shift's admission and runner lock. See `DREAMFERENCE_RAGON_SWE_BENCH.md`.

### 3.11. `audit/`: what a session does on the network

`ragon-admin audit egress` (`egress_audit.py`) runs one real `ragon exec` under `strace` in a throwaway repository and home, parses the trace (`strace_parser.py`, `egress_trace.py`) and gives a verdict (`egress_verdict.py`); `--tui` drives the full-screen interface on a pseudo-terminal (`tui_session.py`), `--docs` runs the document index's scenario (`docs_egress_audit.py`), `--sync` runs the sync scenario (`sync_egress_audit.py`), and `compat_print_session.py` / `preview_page.py` cover compatibility and preview scenarios. See `DREAMFERENCE_RAGON_EGRESS.md`.

### 3.12. `node/`: the GB10 as a node

`ragon-admin node {enable,disable,status,id,list,add,remove,set,start,stop,sync-model,run,jobs,logs,cancel,fetch,prepare,provision,power-cycle,power-config}`. Advertising is `node_advertiser.py`: the Avahi service file that advertises `_ragon-node._tcp` (`node_service_file.py`), the node's stable id (`node_identity.py`), the switches for what is published beyond loopback (`node_settings.py`) and a browse of the network as clients see it (`node_browser.py`). Managing another node is `node_remote.py`, over an SSH pairing (`node_pairing.py`) whose key the other node restricts to one forced command (`node_serve.py`). Jobs on another node are `node_job.py` and `node_job_sender.py`, `node sync-model` is `node_model_sync.py`, adapters travel to a node through `node_adapter_sync.py`, and `node_lanes.py` spreads a run's tasks over several model servers. `node prepare` (`node_prepare.py`, the root half of an install) and `node provision` (`node_provisioner.py`, `node_role_provisioner.py` with the `fleet_*.py` helpers) set up more GB10s from this one, and `node_power_cycle.py` / `node_power_settings.py` / `node_shelly_plug.py` handle smart-plug power cycling. The client side is in the Rust crates (§3.15). See `DREAMFERENCE_RAGON_NODE.md`.

### 3.13. `forge/`: Forgejo on every node

`ragon-admin forge` (`forge_command.py`, `forge_service.py`, `forge_container.py`, `forge_app_ini.py`, `forge_storage.py`, `forge_accounts.py`, `forge_repositories.py`, `forge_mirrors.py`, `forge_github_mirror.py`, `forge_anonymity.py`, `forge_backup.py`, `forge_move.py`, `forge_phone.py`, `forge_bind.py`, `forge_serve.py`, `forge_settings.py`): the self-hosted Forgejo instance (`dreamference-forgejo`), read-only mirrors on paired nodes, pre-receive anonymity hook, and backup/move. See `DREAMFERENCE_RAGON_FORGEJO.md`.

### 3.14. `remote/`: remote access over NetBird

`ragon-admin remote` (`remote_access.py`, `remote_control_plane.py`, `remote_certificate_authority.py`, `remote_box.py`, `remote_enrolment.py`, `remote_join_script.py`, `remote_node_peer.py`, `remote_public_certificate.py`, `remote_relay_certificate.py`, `remote_bundle_options.py`, `remote_settings.py`, `remote_tunnel.py`): the node's self-hosted NetBird control plane, its internal CA, relay setup over SSH, and LAN enrolment. See `DREAMFERENCE_RAGON_REMOTE_ACCESS.md`.

### 3.15. Outside the Python package

| Path | What |
| --- | --- |
| `codex/` | Submodule: the `dreamference/codex` fork, pinned to `rust-v0.158.0`, never edited |
| `codex-patches/` | Patch series applied to an exported copy at build time (32 patches, `0001`–`0035` with `0003`, `0004` and `0017` removed; 57,571 bytes against a 64,000-byte cap) |
| `ragon-rs/` | The launcher crate compiled into `ragon`: the model-server resolution, the catalog and prompts, `update`, `/usage`, `/night`, `/airgapped`, `/node`, `/adapter`, `/worktree`, compatibility mode (`--compat claude` / `ragon-claude`), `ragon skill`, `ragon prompt`, `ragon docs`, refine, the compaction ledger, and the subcommands `ragon web`, `ragon chat`, `ragon signal`, `ragon sync`, `ragon monitor` and `ragon delegate` (`/cavemode` was removed). Member crates: `airgapped/` and `node-locator/` (standard library only), `safegit/`, `loop-guard/`, `masking/`, `skills/`, `apps/`, `tools/`, `monitor/`, `delegate/`, `sync/`, `web/` (`ragon web`), `signal/` (also the `ragon-signal` daemon) and `chat/` |
| `ragon-web-rs/` | `ragon-search` and `ragon-fetch`, the agent's web commands: a standalone crate installed beside `ragon`. It carries byte-identical copies of the two leaf crates' sources |
| `ragon-code-rs/` | `ragon-code`, the code index: a standalone crate installed beside `ragon` |
| `ragon-docs-rs/` | `ragon-docs`, the index of the user's own documents (text, Markdown, PDF): a standalone crate, released for Linux beside `ragon`; `ragon-admin docs setup` fetches what it loads (PDFium, ONNX Runtime, the embedding model), pinned |
| `desktop/electron/`, `desktop/ui/` | The Electron app `ragon-app` and the Ragon UI it shares with `ragon web` |
| `ragon-engine/` | Submodule: the Qwen3.8-27B model server (`ragon-serve`, C++20 and CUDA), its own repository; served from its prebuilt image, the default since 0.9.0 (`--engine ragon`) |
| `dreamference/web_canvas.py` | `ragon-admin web` status page |
| `.github/workflows/` | `release.yml` (manually triggered: Python dist, `ragon` and its companions for Linux, macOS and Windows, the desktop `.deb`; `SHA256SUMS` signed with Ed25519 in the `release` environment), `build-clients.yml`, `windows.yml`, `mac-preview.yml`, `docs.yml`, `cla.yml` |

---

## 4. Roadmap & Implementation Verification

- [x] GB10 model matrix, unified-memory targeting and host-safety guards
- [x] Docker vLLM lifecycle, weight pre-download, tensorizer, benchmark, deep inspection
- [x] Diffusion sidecar beside the main model (switched off; the code is kept)
- [x] Agent runners (Cline, Continue, OpenHands) and the stdio MCP server
- [x] Context engine (AST, FTS5, TF-IDF, embeddings) and web canvas
- [x] `ragon`: branded Codex from a pinned fork, Rust launcher, `update`, `app`, `/usage`
- [x] Ragon web UI with branding, web and image search, voice, Gmail; desktop window
- [x] Code index for `ragon` (`ragon-code`: codebase-memory-mcp + SCIP), implemented; its §14 lists the parts not built (`DREAMFERENCE_RAGON_CODE_INDEX.md`)
- [x] Cave mode (`/cavemode`, removed) and Night Shift (`/night`, `ragon-admin night`)
- [x] `/airgapped` (Phase 1, in part), the egress audit for `exec` sessions (`ragon-admin audit egress`) and SWE-bench (`ragon-admin swe-bench`, Phase 1)
- [x] Client/server split (`ragon-admin node`, `ragon node`, SSH pairing between nodes): Part 1 built in part, Parts 2 and 3 (other nodes as extra model servers, `node sync-model`, `node run --setup/--out/--bind`, `/night add --on`); two GB10s paired (`DREAMFERENCE_RAGON_NODE.md` §18.10); sending from a client is not built
- [x] `/airgapped` at two levels (`duckduckgo` removed), `on` refused with Full Access, the start-up line; the sandbox prerequisite (`ragon-admin host setup`, the AppArmor profile for `bwrap`)
- [x] Named system prompts (`/prompt` Phase 1: `ragon prompt`, `default` and `high-swe`) and skills Phase 3 (Hermes, ClawHub, repository skills)
- [x] Observation masking (patch `0021`), `/apps` without a vendor sign-in (`0022`), the desktop Work window on `ragon app-server`
- [x] Signed releases (Ed25519 over `SHA256SUMS`); the desktop app on Electron; refine mode (off by default); clients for x86-64 Linux, macOS and Windows (preview)
- [x] `node provision` / `node prepare` (FLEET Phases 1–3, tested offline; not yet run against a second machine) and the unattended node install (`install.sh`: one sudo prompt at most, host setup, `node enable`, model download and server start)
- [x] `ragon web` with Ask threads, the bridge policy and the browser view; `ragon-docs` Phase 1; Signal and Matrix/Telegram bridges, off by default
- [ ] Messengers tried against real services, the Windows client run on Windows hardware
- [ ] Proposed specs are marked *Proposed* in `specs/README.md`

---

## See Also

- **[DREAMFERENCE_MODELS.md](./DREAMFERENCE_MODELS.md):** model matrix and default rationale
- **[DREAMFERENCE_INFERENCE.md](./DREAMFERENCE_INFERENCE.md):** vLLM launch recipes and flags
- **[DREAMFERENCE_AGENTS.md](./DREAMFERENCE_AGENTS.md):** agent runtimes
- **[DREAMFERENCE_RAGON_CODEX.md](./DREAMFERENCE_RAGON_CODEX.md):** the `ragon` binary
- **[DREAMFERENCE_CONTEXT.md](./DREAMFERENCE_CONTEXT.md):** context engine
- **[DREAMFERENCE_DOCKER.md](./DREAMFERENCE_DOCKER.md):** Docker and caches
- **[DREAMFERENCE_CLI.md](./DREAMFERENCE_CLI.md):** CLI reference
- **[DREAMFERENCE_SETUP.md](./DREAMFERENCE_SETUP.md):** installation
- **[DREAMFERENCE_CODEBASE.md](./DREAMFERENCE_CODEBASE.md):** codebase layout
