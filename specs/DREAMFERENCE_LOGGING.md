# Ragon logging: one place, one line format, no content

**Status:** Proposed; spec only, building later. The owner's questions are in §0.1.

**Names:** post-rename. These are:
- `ragon`, the agent and launcher;
- `ragon-admin`, the Python administration CLI;
- `~/.ragon`, which is `CODEX_HOME`;
- `ragon web` on port 3100, the same page as the desktop app;
- `ragon-serve`, the engine.

Machines are `node-a` and `node-b`. Paths are repository-relative or start at `~`. No person's name, host name, user name or address is written here (ORGANISATION §7.8).

**Builds on:**
- [CLI](./DREAMFERENCE_CLI.md) §4.8: today's `ragon-admin logs [server|mcp]` and `server logs`, kept as aliases (§8.3).
- [ORGANISATION](./DREAMFERENCE_ORGANISATION.md) §7.8: logs, fixtures and results use `~`, `node-a`/`node-b` or the node id, and addresses from `192.0.2.0/24`. This spec turns that rule for the repository into a rule for every log the product writes on a user's machine.
- [EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) §3 and §10: the strace audit of a real session. §7.7 adds one check to it.
- [SYNC](./DREAMFERENCE_RAGON_SYNC.md) §4: the table of what syncs and what never does. Logs and diagnostics bundles join the "never" rows.
- [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §4.2: the `RagonNode` service under a non-administrator account, with state under `%ProgramData%`.
- [FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md): the one container that bounds its log today (`--log-opt max-size=10m --log-opt max-file=3`). It is the precedent for §6.3.
- `AGENTS.md` and `docs/dev/host-safety.md`:
  - tests never touch the real machine;
  - anything beside a resident model server bounds its memory;
  - the one-reader precedent: `ragon-rs/airgapped/` answers `ragon airgapped --json` for Python and the app.
- Two companions that do not exist yet: [PROFILING](./DREAMFERENCE_PROFILING.md) and [MONITORING](./DREAMFERENCE_MONITORING.md) (§12).

**Why.** When something breaks on a user's node, nobody can look at it remotely, and nobody should be able to. The user has to be able to answer three questions alone:
- what happened;
- which version did it;
- what can be sent to the developers without sending anything private.

Today that is not possible:
- The logs are spread over five roots.
- They come in a dozen formats, with no shared timestamp.
- One launch path writes prompt and completion text into a container log.
- Only one container's log has a size bound.

This document fixes where logs go, what a line looks like, what may never be written in one, and how the user packs a redacted bundle to send by hand.

---

## 0.1 Questions for the owner

Each question has a recommended answer. "Yes" to a question accepts its recommendation.

- **O1. Where logs live.** Use one directory, `~/.local/state/dreamference/logs/` (`$XDG_STATE_HOME`), for every process Ragon starts that does not run under systemd or Docker. Units log to the user journal and containers to Docker's bounded log (§3).
  *Recommended: yes.* Reasons:
  - XDG names "state" as the home for logs.
  - The tree already keeps the fleet logs and the model gate there.
  - `~/.ragon` is the agent's home, and its files belong to Codex and to sync.
- **O2. The environment variable.** Use `RAGON_LOG`, read by every Python, Rust and C++ component, as the environment tier of a new config key `log_level`. There is no `DREAMFERENCE_LOG_LEVEL` alias, and `RUST_LOG` stays Codex's own (§4).
  *Recommended: yes.*
- **O3. The line format.** Use one key=value text line (§5), not JSON lines. JSON lines stay only for machine streams: the engine's step trace, perf records, and the existing event files.
  *Recommended: yes.* Reasons:
  - People read these lines in `journalctl`, `docker logs` and `tail`.
  - `ragon logs --json` gives machines JSON.
- **O4. Container log bounds.** Set them per container, with `--log-opt` from one helper that every `docker run` builder calls. Never edit `/etc/docker/daemon.json` for logging.
  *Recommended: per container.* Reasons:
  - The daemon file is root's and is shared with the user's other containers.
  - Changing it needs a Docker restart, which would stop a resident model server.
- **O5. The bundle command's name.** `ragon-admin diag`, also available as `ragon diag`, rather than `support bundle`.
  *Recommended: `diag`.* There is no support desk, and the word should not promise one.
- **O6. Who reads and bundles.** One Rust crate, `ragon-rs/logs`, exposed as `ragon logs` and `ragon diag` (both with `--json`). `ragon-admin logs` and `ragon-admin diag` forward to it.
  *Recommended: yes.* Reasons:
  - It follows the `airgapped` precedent.
  - The Windows node's half is in Rust launcher subcommands (WINDOWS_NODE §4), so one reader serves both systems.
- **O7. The Diagnostics page on paired devices.** A paired device may view the page behind its credential. It cannot change a level or download a bundle; a bundle is only made on the node, by its owner, with a command.
  *Recommended: yes.*
- **O8. Codex's `logs_2.sqlite`.** Keep upstream's TRACE filter and 10-day retention. Treat the database as an artefact (§7.6), so the bundle carries only a re-redacted lifecycle extract, never the raw file. Audit what its TRACE targets write before any patch (rollout P7).
  *Recommended: yes.*
- **O9. Network addresses.** At `info`, a non-loopback address is written as `[addr]`. At `debug` and `trace`, it is written as is, so the user can see it on their own machine. Every bundle replaces it with a pseudonym.
  *Recommended: yes.*
- **O10. vLLM's request logging.** Remove `--enable-log-requests --enable-log-outputs --max-log-len 2048` from the launch builder in P0. A developer who needs them passes them by hand.
  *Recommended: remove.* No model uses vLLM today, but the path would write up to 2 KB of every prompt and completion into the container log.
- **O11. Retention numbers.**
  - Files: 10 MiB × 5 per component, compressed, for at most 14 days, with 256 MiB for the whole directory.
  - Containers: 10 MiB × 3, and 20 MiB × 5 for the engine.
  - Start records: the last 20 per component.
  - Incident records: 30 days and at most 50 files.

  *Recommended: yes.*
- **O12. journald.** Ragon never changes journald's configuration, which is system-wide and root's. Its writers limit their own rate (§6.2).
  *Recommended: yes.*

---

## 1. Scope and the binding rules

**In scope:**
- Where every Ragon component writes its operational log.
- The levels and how to set them.
- The one line format.
- Rotation and retention.
- The privacy boundary: what is never written, and how the writer enforces it.
- The `logs` and `diag` commands.
- The read-only Diagnostics page.
- The tests that hold all of the above.

**Out of scope:**
- Transcripts, Night Shift task logs, job output and SWE-bench run records. These are *artefacts* of the user's work, not logs; §7.6 draws the line.
- Profiling (PROFILING) and health checks and alerts (MONITORING). Both are §12.
- Codex's internal tracing, beyond how it is read and bundled.

**Binding product rules.** Every section below obeys them.

1. **Local and air-gapped; no telemetry, ever.** No component sends a log line, a count or a crash report anywhere. Nothing leaves the machine unless the user sends it: `diag` writes a file and stops. The product contains no upload code for logs, and the egress audit fails if a log reader talks to the network (§7.7).
2. **One node serves one model on a GB10.** "The engine" is a single process in a single container, so its logs, start record and plan are singular.
3. **Windows node parity.** Every location, format and command has a Windows form (§3.5), and the reader is the same Rust code.
4. **Messengers are the user's own, and off until turned on.** A messenger bridge's log records that a message came and went, with its size and a pseudonymous chat, and never its body (§7.1). No messenger command returns log lines.
5. **Tests never touch the real machine.** Writer, reader and bundle tests run under conftest's temporary `HOME`. They never run `journalctl` or `docker logs` for real; those calls are mocked, as `docker` is today.
6. **Host safety on unified memory is a hard constraint.** Logging has to stay small next to a resident model server:
   - Writers bound their buffers.
   - Rotation runs in the writer, never in a resident daemon.
   - `diag` streams into the archive and caps every source.
   - Nothing here keeps a log in memory beyond a 200-line ring per process (§6.4).
7. **The engine is ragon-serve; SGLang is the fallback.** ragon-serve's own lines follow §5. SGLang's lines are third-party and are passed through the reader and the bundle's redactor unchanged in shape.

---

## 2. Today (surveyed on `main` at the 0.9.2 cut)

### 2.1 Python

- **`logging` is used by eleven modules and configured by none.** They are:
  - `chat/matrix_homeserver.py:35`;
  - six in `remote/`: `remote_certificate_authority.py:26`, `remote_box.py:47`, `remote_relay_certificate.py:40`, `remote_node_peer.py:33`, `remote_public_certificate.py:57` and `remote_control_plane.py:50`;
  - `runner/continue_installer.py:11` and `runner/cline_installer.py:11`;
  - `node/fleet_hub_fetch.py:35` and `node/fleet_session.py:30`.

  There is no `basicConfig` and no handler anywhere, so Python's last-resort handler prints `WARNING` and above as bare messages to stderr. `info` and `debug` from these modules go nowhere.
- **About 1 058 `print()` calls.** Most are CLI output for the user (the emoji voice in `AGENTS.md`), which is correct and stays. Some are operational messages in long-running paths, such as the model gate and the fleet.
- **The model gate** (`vllm_server/model_gate_service.py:823`, `log()`):
  - It prints `time.strftime('%Y-%m-%dT%H:%M:%S')`, which is local time, not UTC, with no zone.
  - It writes to stderr in its own container.
  - Its state is under `~/.local/state/dreamference/model-gate` (`model_gate.py:33`).
- **The fleet**:
  - `node/node_provisioner.py:51` writes to `~/.local/state/dreamference/fleet/<date>-<host>.log`, so **the host name is in the file name**.
  - `node/fleet_session.py:505–516` appends every command it runs on the other machine with its full stdout and stderr. Stdin is recorded only as a note.
- **GPU holds** append to `~/.ragon-gpu-holds.log` (`node/node_role_provisioner.py:68`), outside any state directory.
- **`ragon-admin run --debug`** (`runner/codex_runner.py:77–84`) sets `RUST_LOG=codex_mcp=trace,codex_core=debug,codex_app_server=debug,info` and says the TUI logs to `logs_2.sqlite`.

### 2.2 Rust (`ragon-rs/`, `ragon-web-rs/`, `ragon-code-rs/`, `ragon-docs-rs/`)

- **No logging crate at all.** There is no `tracing`, `log` or `env_logger` dependency in any of Ragon's own crates; output is `eprintln!` and `println!`. Nothing reads `RAGON_LOG`. Two places set `RUST_LOG` for a child:
  - `RUST_LOG=warn` for the app-server under `ragon web` (`ragon-rs/web/src/app_server.rs:148`);
  - `RUST_LOG=debug` in `compat_print.rs:757`.

  `ragon-docs` reads its own `RAGON_DOCS_TRACE_SCOPES` (`ragon-docs-rs/src/index.rs:800`).
- **Files the crates write.** Each uses its own place, format and rotation:

| File | Writer | Rotation |
|---|---|---|
| `~/.ragon/log/ragon-app.log` | the desktop app's stderr, captured by the launcher (`ragon-rs/src/app.rs:40, 272–296`) | replaced on every launch |
| `~/.ragon/web/app-server.log` | `ragon web`'s app-server child (`ragon-rs/web/src/app_server.rs:159`) | none (append) |
| `~/Library/Logs/ragon-web.log` (macOS) | the launchd plist (`ragon-rs/web/src/cli.rs:241, 291`) | none |
| `~/.local/share/dreamference/monitor/checks.log` | the session monitor (`ragon-rs/monitor/src/cli.rs:407–416`) | at about 1 MB to `.log.1` |
| `~/.ragon/masking/log.jsonl` | output masking (`ragon-rs/masking/src/dropping.rs:58, 340–355`); fields are sizes, counts and a `call_id`, no content | at 256 KiB to `.old` |
| `~/.ragon/loop-guard/events.jsonl` | the loop guard (`ragon-rs/src/loop_guard.rs:585, 657`); fields are a thread id, enums and counts, no content | none |
| `~/.ragon/docs/index.log` | `ragon-docs` indexing (`ragon-docs-rs/src/main.rs:232`) | none |
| `<repo>/.dreamference/supervisor-<pid>.log`, `logs/<unit>.log` | the `ragon-code` index supervisor (`ragon-code-rs/src/index/supervisor.rs:211, 360`; `paths.rs:56`) | none, **inside the user's repository** |

- `ragon-rs/src/home.rs:141`, which migrates `~/.codex`, does not carry `logs_2.sqlite` over. That is correct, and stays.

### 2.3 Codex's own store (read-only, from the pinned submodule)

- Codex writes its `tracing` events to `~/.ragon/logs_2.sqlite`; patch 0014 moved the home there. The filter is `codex-rs/state/src/log_db.rs:61` `default_filter()`:
  - **TRACE for every target**;
  - except that the HTTP transport and SSE targets are capped at DEBUG, so full request bodies "remain available to explicit TRACE subscribers" and stay out of the default store.
- Retention is `LOG_RETENTION_DAYS = 10` (`state/src/runtime/logs.rs:3`), pruned by each process.
- `ragon-admin logs mcp` (`cli/dreamference_cli_controller.py:4162`) reads the database read-only (`SELECT … ORDER BY rowid DESC LIMIT 8000`) and keeps MCP lifecycle lines.
- Patches that already close log channels to the vendor:
  - 0009 removes `/feedback`, which uploaded session logs;
  - 0011 removes the TUI analytics view;
  - 0013 turns analytics off;
  - 0015 sets `OtelExporter::None` and removes Statsig.

  No patch changes a log level.

### 2.4 The engine (`ragon-engine/`, read-only here)

- Output is free-text stderr from `src/serve/main.cpp`:
  - **The request line** (`:1116`) is counts and timings only: session, prompt tokens, reused tokens, prefill seconds, chunks, tokens per second, passes and finish reason. In content it is already compliant; only its shape changes.
  - **`request refused: %s`** (`:1270`): `why` comes from `refuse_request(mem_available_mb(), …)` and contains numbers only.
  - **The start print** (`:1920–2008`): refused-to-start; KV page pool; sessions and batch size; `loading <model_dir>` with **an absolute path**; weights loaded; repetition stop; pairing; `ragon-serve: serving <name> on <host>:<port>`.
  - **`plan_json`** goes to stdout (`:1944`).
- **There is no version or build stamp.** The start print and `CMakeLists.txt` carry none, so a log cannot say which build was running.
- Tuning comes from the environment: `RAGON_KEEPALIVE_SECONDS`, `RAGON_KV_PAGES`, `RAGON_PREFIX_KV_MB`, `RAGON_TREE*` and `RAGON_DRAFT*` (`speculate.cpp:204–253`).
- **`RAGON_STEP_TRACE=FILE`** writes JSON lines, one per scheduler unit (`step_trace.hpp`). The fields are `t`, `pass`, `kind`, `turn`, `sessions`, `rows`, `tokens`, `left`, `ms`, `side` and `cand`. `tokens` is the *count* each request chose in that step, not token ids. `bench/agentlong/step_trace.py` reads it.
- **`RAGON_DUMP_STEPS` and `RAGON_DUMP_HIDDEN`** (`speculate.cpp:108–124`) write binary dumps of hidden states. These are derived from content and are developer-only.
- The image ships `libglog` (`docker/ragon-serve/Dockerfile:46`), which is unused for the serve log.

### 2.5 Containers

- **Names** are `dreamference-*`: `vllm-<port>` (the engine, whichever it is), `gate-`, `gmail`, `searxng`, `stt`, `image-search`, `siglip`, `matrix`, `remote`, `diffusion-`, `mcp-server`, `openhands` and `vllm-tensorizer`.
- **Only the forge bounds its log** (`forge/forge_container.py:121–124`). Every other container uses Docker's default `json-file` driver with **no size limit**. `node/node_role_provisioner.py:236` merges `daemon.json` with the NVIDIA runtime and download attempts, and no `log-opts`.
- **The vLLM launch path** (`vllm_server/vllm_server_manager.py:869–872`, inside `build_launch_command` at `:605`) passes `--enable-log-requests --enable-log-outputs --max-log-len 2048`. The SGLang and ragon-serve branches return before it, and no recipe selects vLLM today. Anyone who did would get up to 2 KB of every prompt and every completion in `docker logs`. O10 removes the flags.
- `VLLM_NO_USAGE_STATS=1` and `DO_NOT_TRACK=1` are set for the engine containers (`vllm_server_manager.py:165–168, 1098`).
- Reading: `show_request_logs` (`vllm_server_manager.py:2430`) and `cli/dreamference_cli_controller.py:272, 760` run `docker logs [-f] dreamference-vllm-<port>`.

### 2.6 systemd units

All units are named `ragon-*`. None sets `StandardOutput=`, so all of them log to journald.

- **User units:**
  - `ragon-web.service` (`ragon-rs/web/src/cli.rs:43`);
  - `ragon-chat.service` (`ragon-rs/chat/src/cli.rs:33`);
  - `ragon-sync.service` (`ragon-rs/sync/src/cli.rs:38`);
  - `ragon-monitor` and `ragon-monitor-digest`, each a service and a timer (`ragon-rs/monitor/src/units.rs:8–10`);
  - `ragon-forge.service`, the timers `ragon-forge-tick`, `-backup` and `-check`, and `ragon-forge-mirror.path` (`forge/forge_service.py:9–14`);
  - `ragon-night`, a service and a timer (`night_shift/night_shift_scheduler.py:40`);
  - `ragon-matrix-proxy` and `ragon-matrix-tls` (`chat/matrix_homeserver.py:55, 59`);
  - `ragon-remote-tunnel`; `ragon-remote-control`, a socket and a service; `ragon-remote-certificate`, a service and a timer; and `ragon-remote-certificate-alert` (`remote/remote_settings.py:68–115`);
  - `ragon-job-<id>` (`node/node_job.py:150`).
- **System units:**
  - `ragon-signal.service` and `ragon-signal@.service` (`ragon-rs/signal/src/unit.rs:10, 19–21`);
  - `ragon-relay.service` on the relay box (`remote/remote_box.py:55, 359`).
- **Hints that name `journalctl`:** `forge/forge_service.py:332`, `remote/remote_access.py:412`, `remote/remote_public_certificate.py:642` and `ragon-rs/signal/src/cli.rs:989`.
- `vllm_server_manager.py:44` notes that journald competes for unified memory.

### 2.7 The desktop app

- It has no `electron-log` and no `crashReporter`; patch-level settings keep crash reports off.
- The main process writes `console.error` lines prefixed `ragon-app:` (`desktop/electron/src/main.ts:524, 1238`), which reach `ragon-app.log` (§2.2).
- Help › Show Logs opens `~/.ragon/log` (`main.ts:317–322`).

### 2.8 Windows

- `install.ps1` writes only `%TEMP%\ragon-elevated-<guid>\elevated.log` and `verify.out`/`.err` for the elevated step (`install.ps1:420–422, 500`).
- Nothing uses the Event Log.
- WINDOWS_NODE §4.2 proposes the `RagonNode` service with state under `%ProgramData%`, and says nothing yet about logs.

### 2.9 Artefacts that look like logs

These are covered by §7.6:
- Night Shift: `~/.ragon/night/logs/<task>{.jsonl,.last.txt,.test.log,.commit.log,.diff,.judge.jsonl}` (`night_shift/night_shift_task_run.py:189–221`, `best_of_n.py:407`).
- Node jobs: `~/.ragon/jobs/<id>/output.log` (`node/node_job.py:112, 139, 443`).
- SWE-bench:
  - `harness-<run>.log` (`swe_bench/swe_bench_harness.py:390`);
  - `index.log` (`swe_bench_code_index.py:309`);
  - `~/.local/share/dreamference/swe-bench/runs/<run>/logs/<id>.jsonl` (`swe_bench_run_store.py:178`).
- Scripts: `~/.cache/dreamference/night-traces/*.jsonl` (`scripts/swe_bench_ab.sh:301`) and `~/.cache/dreamference/turn-end-compaction/*.log`.

### 2.10 Redaction today

There are four redactors, each local to one feature, and none in a shared log path:
- `desktop/ui/src/windowtools.ts:24` `maskSecrets()`;
- `desktop/ui/src/exportthread.ts:36` `redact()`, which writes the working directory as `.`, the home as `~`, e-mail addresses as `[email]` and masks tokens;
- `ragon-rs/web/src/chat_repos.rs:2294` `sanitize_error`;
- `ragon-rs/chat/src/repo_notice.rs:417` `sanitize_kind_for_log`.

### 2.11 What follows from the survey

| Problem | Where | Fixed in |
|---|---|---|
| Prompt and completion text in a container log (vLLM path) | `vllm_server_manager.py:869` | P0 (O10) |
| A host name in a log file's name | `node_provisioner.py:51` | P0 |
| Local time, no zone | the model gate | P0 |
| Unbounded container logs (all but the forge) | §2.5 | P0 (O4) |
| A log written inside the user's repository | `ragon-code` supervisor | P1 |
| Five roots: `~/.local/share`, `~/.local/state`, `~/.cache`, `~/.ragon`, `<repo>/.dreamference` | §2.2, §2.9 | P1 (§3) |
| Python loggers never configured; no Rust logging at all | §2.1, §2.2 | P1 |
| No engine version, build or flag stamp | §2.4 | P2 |
| No redaction in any log path | §2.10 | P1 |
| No way to hand over "what happened" | — | P3, P4 |

---

## 3. Where logs live

### 3.1 One rule per kind of process

| The process runs… | Its log goes to | Read with |
|---|---|---|
| as a systemd unit (user or system) | **journald**, from stderr | `ragon logs <component>` or `journalctl --user -u 'ragon-<component>*'` |
| in a Docker container | **Docker's log**, bounded (§6.3) | `ragon logs <component>` or `docker logs dreamference-<name>` |
| as anything else Ragon starts (the desktop app, a CLI run, the launcher, an indexer outside a unit) | **a file**, `~/.local/state/dreamference/logs/<component>.log` | `ragon logs <component>` or `tail` |
| Codex's own `tracing` | `~/.ragon/logs_2.sqlite` (upstream; O8) | `ragon logs codex` (lifecycle extract) |

Each process has **one sink**. A writer chooses it at start-up:
- **journald**, when `JOURNAL_STREAM` is set: systemd sets it, and its device and inode match stderr.
- **stderr**, inside a container, where Docker captures it.
- **the file**, otherwise.

No process writes the same line to two sinks. A CLI command's output to the terminal is not log output (§5.6).

### 3.2 The directory (O1)

```
~/.local/state/dreamference/            ($XDG_STATE_HOME/dreamference; exists today: fleet/, model-gate/)
  logs/                                 mode 0700
    <component>.log                     the current file, mode 0600
    <component>.1.log.gz … .4.log.gz    rotated (§6.1)
    starts/<component>/<UTC>.json       start records (§6.4)
    incidents/<UTC>-<component>.log     incident records (§6.4)
  diag/ragon-diag-<UTC>.tar.gz          bundles (§9), mode 0600
~/.config/dreamference/
  log.env                               RAGON_LOG=…, written by `ragon-admin logs level` (§4.3)
  log-salt                              32 random bytes for pseudonyms (§7.4), mode 0600, never bundled
```

**Why `~/.local/state` and not `~/.ragon`:**
- `~/.ragon` is Codex's home: its sessions, its SQLite stores, and what SYNC reads.
- Most components (the engine, the gate, the forge, the sidecars) have nothing to do with the agent.
- XDG names state as the home for logs, and the tree already keeps the fleet logs and the model gate there.

**Why not `~/.cache`:** a cache may be deleted at any time, and a log is the thing one needs after something went wrong.

**Files that move.** P1 moves every operational log in §2.2 into this directory, under its component name:
- `ragon-app.log` becomes `app.log`;
- `app-server.log` becomes `app-server.log`;
- `checks.log` becomes `monitor.log`;
- `docs/index.log` becomes `docs-index.log`;
- the `ragon-code` supervisor logs become `code-index.log`, with the repository as a pseudonymous field;
- `~/.ragon-gpu-holds.log` becomes `gpu-holds.log`.

Help › Show Logs opens the new directory. The masking and loop-guard event files are machine streams owned by their features and stay where they are (§5.5).

### 3.3 Components and unit names

A **component** is `[a-z][a-z0-9-]*`. One table, `ragon-rs/vectors/logging/components.txt`, lists them, and Python, Rust and the tests read it:

`admin`, `launcher`, `app`, `app-server`, `web`, `engine`, `sglang`, `gate`, `night`, `job`, `swe-bench`, `forge`, `sync`, `chat`, `signal`, `matrix`, `remote`, `relay`, `monitor`, `code-index`, `docs-index`, `search`, `google`, `stt`, `images`, `fleet`, `gpu-holds`, `audit`, `mcp`, `installer`, `codex`.

**The unit rule:**
- A unit is named `ragon-<component>[-<part>]`, or `ragon-<component>@<instance>` for a template.
- It sets `SyslogIdentifier=ragon-<component>`, so `journalctl -t ragon-forge` gathers every part of the forge.
- It sets `EnvironmentFile=-%h/.config/dreamference/log.env` (§4.3).

All of today's units already follow the name rule. P1 adds the two settings, and a test holds them (§11.4).

**The container rule:**
- A container keeps its `dreamference-<name>` name.
- It carries the label `ragon.component=<component>`.
- It gets the bounded log options (§6.3).

### 3.4 Codex's store

`logs_2.sqlite` stays where Codex puts it, with Codex's filter and retention (O8). It is never moved, never written by Ragon, and never bundled raw. `ragon logs codex` and `ragon-admin logs mcp` read it read-only, as `logs mcp` does today.

### 3.5 Windows and macOS

| | Location | Notes |
|---|---|---|
| Windows, per user (`ragon`, the app, CLI runs) | `%LOCALAPPDATA%\Ragon\logs\` | Same layout as §3.2 |
| Windows, the `RagonNode` service (WINDOWS_NODE §4.2) | `%ProgramData%\Ragon\logs\` | ACL: the service account and administrators; the user reads through `ragon logs` |
| Windows Event Log | source `Ragon`, the Application log | **Lifecycle only**: service start, stop, refused to start, crashed. One line each, the same message and fields, never anything at `debug` |
| macOS client | `~/Library/Logs/Ragon/` | The launchd plist for `ragon web` moves its file here |

Windows has no journald, and its containers are native processes (WINDOWS_NODE §4), so every Windows component writes the file sink and rotates in the writer. The reader treats a Windows node exactly like a file-only Linux process.

---

## 4. Levels and configuration

### 4.1 Levels

| Level | For |
|---|---|
| `error` | Something failed that the user has to know about: a start refused, a crash, a request failed for a reason that is not the client's |
| `warn` | Something was wrong but handled: a retry, a fallback to SGLang, a memory reserve refused a request |
| `info` (**default**) | Lifecycle and one line per unit of work: start and stop with versions, a request done with counts and timings, a job queued, run and finished, a sync round, a message relayed with its size |
| `debug` | More events and more fields of the same kind: per-chunk timings, retries in detail, each state transition, addresses (O9). **Never content** (§7.3) |
| `trace` | Per-step detail for developers of that component: the scheduler's choices and cache hits. Same boundary as `debug` |

### 4.2 The specification string

There is one grammar everywhere:

```
spec      := default ("," override)*  |  override ("," override)*
default   := level
override  := component "=" level
level     := "error" | "warn" | "info" | "debug" | "trace" | "off"
```

Examples:
- `RAGON_LOG=debug` sets every component to `debug`.
- `RAGON_LOG=info,engine=debug,gate=trace` keeps everything at `info` except those two components.

An unknown component or level is a warning at start-up, and the rest of the string still applies. Python maps a logger name to its component by package: `dreamference.forge.*` maps to `forge`, and the table in `components.txt` maps the rest.

### 4.3 Precedence (O2)

The level follows DreamferenceConfig's four tiers, as a new config key `log_level` holding a spec string:

1. a `--log-level` flag where a command has one (the constructor keyword argument);
2. **`RAGON_LOG`** in the environment (the environment tier; the one exception to the `DREAMFERENCE_*` naming, because Rust and C++ read the same name);
3. `log_level = "…"` in `dreamference.toml`, the local file first and then `~/.config/dreamference/config.toml` (a flat key, like every other key there);
4. `DEFAULT_LOG_LEVEL: Final = "info"`.

Rust and C++ do not read `dreamference.toml`. They use:
1. `RAGON_LOG` from the environment;
2. then `~/.config/dreamference/log.env`;
3. then `info`.

`ragon-admin logs level <spec>` saves `log_level` in the user config and writes `log.env` from it, so units (through `EnvironmentFile=`), containers (`-e RAGON_LOG` from the same file) and standalone binaries all agree. Units and containers pick the new level up at their next restart. `ragon-admin logs level` prints which ones need one and never restarts the engine itself.

`RUST_LOG` stays Codex's own: it filters Codex's `tracing`, as it does today. `ragon --debug` and `ragon-admin run --debug` keep setting it, and also set `RAGON_LOG=launcher=debug,app-server=debug`.

---

## 5. One line format (O3)

### 5.1 The line

```
2026-11-08T14:03:12.481Z engine INFO request done session=s-3f2a9c prompt_tokens=18233 reused=17904 prefill_s=0.41 chunks=3 tok_s=47.2 passes=812 finish=stop
2026-11-08T14:03:12.902Z gate WARN request refused reason=no_session client=[addr] queued=2
2026-11-08T14:05:40.017Z sync INFO round done device=d-71c0 threads_up=3 bytes_up=48213 ms=212
```

Grammar, held by one regular expression in `ragon-rs/vectors/logging/line.re`:

```
line      := ts " " component " " LEVEL " " message (" " field)*
ts        := YYYY-MM-DD "T" hh:mm:ss.mmm "Z"          UTC, always milliseconds, always Z
component := [a-z][a-z0-9-]*                          from components.txt
LEVEL     := ERROR | WARN | INFO | DEBUG | TRACE
message   := [a-z0-9][a-z0-9 _.:/-]{0,119}            a literal from the source, no "=", no quote
field     := key "=" value
key       := [a-z][a-z0-9_]*
value     := bare | quoted
bare      := [A-Za-z0-9._:/~+@\[\]-]+
quoted    := '"' ( [^"\\\n] | '\"' | '\\' | '\n' | '\t' )* '"'
```

The rules:
- A line is at most 4 KiB. A longer one is cut at the last whole field, and `truncated=1` is added.
- A line holds no raw newline, carriage return or control character; inside quotes they are escaped.
- The message comes first because it is what a person scans. Fields come after, in the order the call site gives them.
- **Under journald,** the writer prefixes the line with the syslog priority, `<3>` to `<7>`. journald strips the prefix, stores `PRIORITY=` and keeps the rest. The line keeps its own timestamp, which costs 25 bytes and buys one thing: `diag` can merge sources by the time the event happened, not by when it was received.
- **Under Docker,** the line goes to stderr as is.
- **In a file,** it is written as is, one `write` per line.

### 5.2 Why text and not JSON lines

- **People read these lines first.** They read them in `journalctl`, `docker logs`, `tail` and an issue the user pastes by hand. `k=v` stays readable at 120 columns, while JSON's quoting and braces double a line.
- **Machines still get structure.** The grammar is regular, so `ragon logs --json` parses every line into `{"ts","component","level","message","fields":{…}}`, and the Diagnostics page and MONITORING use that.
- **JSON lines stay for data streams** (§5.5), whose readers are programs and whose records are tables, not sentences.

### 5.3 Fields have types

The writer API takes typed fields. The type decides how a value is written and redacted (§7.4):

| Type | Written as | Example |
|---|---|---|
| `count`, `bytes`, `ms`, `s`, `ratio` | a number | `prompt_tokens=18233 bytes=48213 ms=212` |
| `id` | Ragon's own short id: a session, request, job, thread, device or node id | `session=s-3f2a9c node=n-0b1e` |
| `enum` | one of a fixed set the call site declares | `finish=stop reason=no_session` |
| `version` | a version, git sha or image digest | `version=0.9.2 git=da9776b` |
| `path` | Ragon's own files only; home becomes `~` | `model_dir=~/.cache/dreamference/models/qwen3.8-27b-nvfp4` |
| `url` | scheme, host and path only: no user info, no query, no fragment | `url=http://127.0.0.1:8000/v1/chat/completions` |
| `addr` | `[addr]` at `info`, as is at `debug` (O9) | `client=[addr]` |
| `error` | an error class plus its message after `redact()` | `error=ConnectionRefused error_msg="connect: connection refused"` |
| `pseudo` | a salted pseudonym (§7.4) | `chat=p-9c41e2 project=p-5a07dd` |
| `size_of` | the size of something that is never written | `args_bytes=812 output_bytes=12044 output_lines=301` |

**There is no free-text field type.** A call site that wants to log a string from outside picks one of these types or does not log it.

### 5.4 The engine's three outputs

ragon-serve is C++ in the engine repository. Its changes land there, through the engine's publish queue.

1. **The start record.** At start-up the engine writes one `engine INFO start` line and one JSON file, `starts/engine/<UTC>.json` (the file is written by the node's starter, which captures the engine's stdout; see below). The line carries:
   - `version`, and `git`, the engine's short sha, both stamped by CMake at build;
   - `arch=sm_121`, `cuda`, `driver`, `image` (the image digest);
   - `model` (the registry key) and `model_dir`, written with `~`;
   - `flags`: the command-line flags plus every `RAGON_*` tuning variable that is set, with its value. All of these are numbers or names.

   The JSON file adds the plan (`plan_json`, today printed to stdout at `main.cpp:1944`) and the KV page pool, sessions and batch size from the start print. `serving <name> on <host>:<port>` becomes `listening addr=… port=…`, with the address under the `addr` rule.
2. **The request log.** One line per finished request, `engine INFO request done …`, with today's counts and timings (§2.4). The session id is the engine's own short id, never a value the client sent. A refused request is `engine WARN request refused reason=<enum> mem_available_mb=… reserve_mb=…`.
3. **The step and perf stream.** `RAGON_STEP_TRACE` stays a JSON-lines file, because it is PROFILING's input (§12), with today's fields plus `ts` (UTC ISO) beside `t`. It is off unless set, and the product never sets it (§7.3).

The node's starter (`server start`) reads the engine's start JSON from the container's stdout. It writes the start record, keeps the last 20 (§6.4), and is the only Python code that touches the engine's output.

### 5.5 Streams that stay JSON lines

These are records for programs, owned by their feature, and checked by the same redaction tests:
- `masking/log.jsonl`;
- `loop-guard/events.jsonl`;
- the engine's step trace;
- PROFILING's perf records.

Each stream:
- names its fields in a schema next to its writer;
- holds only §5.3's types;
- carries a `ts` in UTC ISO. The masking log's `at`, in Unix seconds, gets `ts` beside it in P1.

### 5.6 What is not a log line

- **What a command prints for the person who ran it** is CLI output, not a log, and it keeps the voice in `AGENTS.md`. Examples: `ragon-admin server start`'s progress, `✅ Server ready`, a table. It is not timestamped and not captured.
- **A command that runs under a unit or a timer** (such as `ragon-admin night run`) uses the writer for what it does, and keeps `print` only for messages meant for a person at a terminal.

---

## 6. Rotation and retention

### 6.1 Files

Each writer rotates its own file:
- At **10 MiB**, `<component>.log` becomes `<component>.1.log`, which is compressed to `.1.log.gz` after the rename, and older files shift up.
- **Five** files are kept.
- On every rotation the writer deletes its own component's files older than **14 days**.

The whole `logs/` directory has a budget of **256 MiB**. `ragon-admin logs prune`, which `diag` and `server start` also run, deletes the oldest rotated files of any component until the directory fits.

No daemon watches the directory, because rotation in the writer costs nothing while idle. Rotation is atomic (`rename`) and safe against two processes of one component, such as two CLI runs:
- each process opens the file with `O_APPEND`;
- the rotating process takes an advisory lock on `<component>.lock`;
- a process that loses the race reopens the file.

### 6.2 journald

**Ragon never edits `journald.conf`** (O12). The user journal's own limits apply. On a default system that is the smaller of 10% of the file system and 4 GiB, shared with everything else.

journald holds its buffers in memory that the model server needs, so a writer limits its own rate:
- at most **50 lines per second** per process at `info`, after which further lines are dropped;
- one `WARN log lines dropped count=N` line follows once the second has passed.

At `debug` and `trace` the limit is 500 lines per second. journald's own `RateLimitBurst` stays as the backstop.

### 6.3 Containers (O4)

One helper, `ContainerLogOptions.args(component)`, returns:
- `--log-driver json-file --log-opt max-size=10m --log-opt max-file=3`;
- `--label ragon.component=<component>`;
- `-e RAGON_LOG=…` from `log.env`.

The engine container gets `max-size=20m --log-opt max-file=5`. **Every** `docker run` that Ragon builds calls the helper:
- the engine;
- the gate;
- every sidecar;
- the forge, which replaces its own options;
- SWE-bench's harness containers.

A test holds it (§11.4). `daemon.json` is never changed for logging.

### 6.4 What is kept for a release-blocker report

A release blocker is investigated after the fact, often days later, so three things are kept beyond the rotating logs:

1. **Start records.** For each component, keep the last **20** start JSON files: the version stamps, the engine's plan and flags, the effective `RAGON_LOG`, and the exit status and reason of the previous run. A component appends that exit status and reason to its own last record on a clean stop, or its starter does on the next start. The engine writes one, and so do the gate and every unit Ragon starts.
2. **Incident records.** When a component logs `ERROR`, its writer dumps the last **200** lines it emitted from a ring in memory, about 100 KB at most, into `incidents/<UTC>-<component>.log`. The same happens when a supervisor sees a crash: the PSI watchdog's kill, a unit's failure, or the engine's container exiting non-zero. In that case the supervisor also adds the last 500 lines of the container's log, re-redacted. Records are kept for **30 days**, at most 50 files.
3. **The journal window**, as journald keeps it.

`ragon-admin diag` (§9) packs these with the last N hours of every component. The release checklist's blocker template asks for one bundle from the machine that showed the bug.

### 6.5 Codex's store and artefacts

Codex's store keeps upstream's 10 days (O8). Artefacts keep their feature's own retention, such as Night Shift's and the jobs' cleanup. This spec does not change either.

---

## 7. The privacy boundary

This section is the heart of the document. Every rule here holds **at every level, `trace` included, and in every sink and every bundle**.

### 7.1 Never written, at any level

| Never | Instead |
|---|---|
| Prompt text, completion text, reasoning text, a system prompt | counts: `prompt_tokens`, `completion_tokens` |
| Token ids or logits (they are content in another encoding) | counts |
| File contents, diffs, search results, documents, images, audio | `bytes`, `lines`, a pseudonymous `file` at `debug` |
| Tool-call arguments and tool outputs | `tool=<name>` (an enum), `args_bytes`, `output_bytes`, `exit_code`, `ms` |
| Messenger message bodies, captions, attachment names | `bytes`, `kind=<enum>`, `chat=<pseudonym>` |
| Phone numbers, e-mail addresses, chat handles, display names | a pseudonym |
| Tokens, API keys, passwords, cookies, pairing codes, private keys, the web credential, OAuth codes | nothing: the field does not exist (§7.4) |
| URL query strings, fragments, user info | the URL without them |
| User names | `~`, or nothing |
| Host names (of this machine or any other) | the node id (`node=n-…`), or `node-a`/`node-b` in fixtures |
| Absolute home paths | `~/…` |
| Paths inside the user's projects and document collections, and the project's own location | `project=<pseudonym>`; at `debug`, a path relative to the project root, never the root itself |
| Shell commands the agent or a job ran | `cmd=<program name>` at `debug`, its exit code and duration; never its arguments or output |
| Environment variables' values (except `RAGON_*` tuning numbers and `RAGON_LOG`) | their names |

### 7.2 What is logged

- **Lifecycle:** start, ready, stop, refused to start, crashed, restarted, each with the reason as an enum.
- **Versions and plan:** what was running and how it was configured (§5.4).
- **Units of work, one line each:**
  - a request, a turn, a tool call, a job, a sync round, a relayed message, an index pass, a backup;
  - each with ids, counts, sizes, durations and an outcome enum.
- **Resources:**
  - memory available and reserved;
  - PSI averages at the watchdog's decisions;
  - KV pages in use;
  - disk free at the points that check it.
- **Errors:** a class, a redacted message, and where in Ragon (a module or function name, never a user path).
- **Configuration that is not a secret:** config key names, and values that are enums, numbers or Ragon paths.

### 7.3 How `debug` and `trace` differ

`debug` and `trace` add events and fields of the same types:
- per-chunk and per-step timings;
- each retry;
- each state transition;
- non-loopback addresses (O9);
- program names of commands;
- paths relative to a project root.

They never add content. A developer who needs content uses tools the product never runs, and whose output never lands in `logs/` or a bundle:
- `RAGON_DUMP_STEPS` and `RAGON_DUMP_HIDDEN`;
- vLLM's request logging, passed by hand;
- Codex's `RUST_LOG=trace` with its own subscriber;
- a debugger.

A test checks that product code never sets these (§11.4). There is no "log everything" switch, and none will be added.

### 7.4 Redaction at the writer

Redaction happens **once, at the writer, before a line exists**, in all three languages from one set of rules. It is not a filter applied later to text. It has four steps:

1. **The message is a literal.** The writer API takes the message as a string constant (Rust: `&'static str`; C++: a string literal; Python: the first argument, checked by the format test to be a literal at every call site).
2. **Typed fields** (§5.3) decide how each value is written. A `pseudo` value is `p-` plus the first 6 bytes (12 hex) of `HMAC-SHA256(log-salt, kind ‖ value)`:
   - it is stable on one machine, so lines can be correlated;
   - it cannot be reversed by whoever receives a bundle;
   - the salt never leaves the machine.
3. **`redact()` runs on every `error` message and every quoted value.** It replaces, in order:
   - private-key blocks (`-----BEGIN … -----END …`) with `[key]`;
   - bearer and basic credentials (`Authorization: …`, `Bearer …`) with `[secret]`;
   - known token shapes with `[secret]`: GitHub `gh[pousr]_…` and `github_pat_…`, Slack `xox[abposr]-…`, `sk-…`, JWTs `eyJ….….…`, AWS `AKIA…`, Telegram `<digits>:<35 chars>`;
   - `key=value` and `"key": "value"` pairs whose key is secret-like (rule 4) with `[secret]`;
   - URL user info, query and fragment, which are dropped;
   - e-mail addresses with `[email]`;
   - the machine's own home directory, any `/home/<x>`, `/Users/<x>` or `C:\Users\<x>`, with `~`;
   - the machine's own host name, its FQDN and its user name with `[host]` and `[user]`. These are read once at start-up, so no list in the repository names anyone;
   - IPv4 and IPv6 addresses other than loopback with `[addr]`, at `info`.
4. **Secret-like field names are refused.** A field whose key, split on `_`, ends in `token`, `key`, `secret`, `password`, `passwd`, `cookie`, `credential` or `auth`, or equals one of them, is written as `[secret]`. So `api_key`, `web_token` and `auth` become `[secret]`, while `prompt_tokens` and `key_count` stay, because the rule matches whole segments at the end. The fixtures pin this down (§11.1).

Redaction counts are kept per process. A bundle reports them, and an incident record notes them.

### 7.5 One implementation per language, one set of fixtures

| Language | Writer | Lives in |
|---|---|---|
| Python | a `logging.Formatter`, a `Filter` that drops untyped `extra=` values, and `setup_logging(component)` called once per entry point; `ragon_log.info("request done", count(prompt_tokens=…), …)` for typed fields | `dreamference/config/` (a new `log_writer.py` and `log_redactor.py`, one class each) |
| Rust | `ragon-rs/log`, `std` only, with an optional `tracing` layer so a crate that adopts `tracing` later writes the same lines | `ragon-rs/log/` |
| C++ (engine) | `log.hpp`, header-only, no `glog` | `ragon-engine/src/serve/log.hpp` |

All three are tested against `ragon-rs/vectors/logging/` (§11.1). The engine repository keeps a copy of the vectors, and a test here compares it with this repository's copy when the submodule is checked out.

### 7.6 Logs and artefacts

An **artefact** is a file whose purpose is the user's work or its record. Artefacts:
- stay where their feature puts them, under its retention;
- **are never written to `logs/`**;
- **are never put into a bundle**, by any flag. A user who wants to share one attaches it by hand.

A **log** describes what the software did, under §7.1.

| Artefact | Why it is one |
|---|---|
| Codex rollouts (`~/.ragon/sessions/`) and the SQLite stores, `logs_2.sqlite` included (O8) | conversations; Codex's TRACE store may hold tool payloads (unaudited) |
| Night Shift's `night/logs/<task>.*` | the agent's transcript, its test output, its diff |
| Node jobs' `jobs/<id>/output.log` | the user's script's output |
| SWE-bench run records and harness logs, `night-traces/`, `turn-end-compaction/` | benchmark transcripts |
| The fleet's session log | full stdout and stderr of commands on another machine. P0 renames the file to `<date>-<node-id>.log` and moves it under the rule: the log keeps command program names, exit codes and durations, and the output becomes an artefact beside it |

The desktop app's log and `app-server.log` hold Electron's and Codex's stderr, so content can appear there. P1 routes them through the writer: the app's main process uses the Python-equivalent typed writer in TypeScript, and the app-server child's stderr is wrapped with `redact()` line by line before it is written. Until then, the bundle carries them only after a full `redact()` pass and flags them in the manifest.

### 7.7 The egress audit's role

The writer keeps content out of logs. The egress audit proves that logs do not leave the machine:
- **A new check.** Under strace, a process that opens a file under `~/.local/state/dreamference/logs/`, `~/.ragon/logs_2.sqlite` or a `diag/` archive, or that reads the journal, and *also* makes a non-loopback connection, fails the audit. The process is named in the report. It applies to `audit egress` and its `--web`, `--app` and `--sync` modes.
- **Sync.** SYNC §4 gets two rows marked "**Never**": logs and diagnostics bundles. The sync protocol gets no message type that could carry either.
- **Messengers** have no command that returns log lines, and MONITORING's alerts carry a fixed message and counts, never a log line (§12).
- **No upload code.** A test asserts that `ragon-rs/logs` and the Python writer import no HTTP client and open no socket.

---

## 8. Reading logs: `ragon logs` and `ragon-admin logs`

### 8.1 The surface

```
ragon-admin logs list
ragon-admin logs <component> [--since 1h|30m|2026-11-08T12:00Z] [--until …] [--follow]
                             [--level warn] [--grep TEXT] [--json] [-n 200]
ragon-admin logs level [<spec>]          # show, or set log_level and log.env (§4.3)
ragon-admin logs prune                   # apply §6.1's budget now
```

`ragon logs …` takes the same arguments, and `ragon-admin logs …` forwards to it (O6).

**`logs list`** prints one row per component on this machine:
- the component;
- its sink (`journald ragon-web.service`, `file ~/.local/state/dreamference/logs/app.log`, `container dreamference-vllm-8000`, or `sqlite ~/.ragon/logs_2.sqlite`);
- its size on disk;
- the time of its last line;
- the level in effect;
- the time of its last `ERROR`.

**`logs <component>`** reads the component's sink:
- `journalctl --user -u 'ragon-<component>*' -o json` (or `--system` for `signal` and `relay`); the reader runs the `journalctl` binary, so there is no libsystemd dependency;
- `docker logs --timestamps` for the container with the label;
- the file and its rotated files, decompressed as it goes;
- the store, for `codex`.

The options:
- `--level` keeps lines at that level and above. For journald it maps to `PRIORITY`, and in files to the level token.
- Lines not in the §5 format (SGLang, SearXNG, Conduit and other third-party containers) are shown as they are. In `--json` they get level `null` and `"raw": true`.
- `--follow` follows one sink.

### 8.2 The merged view

`ragon-admin logs all --since 1h` merges every component by each line's own timestamp. It is the view a person uses after "it broke at about three", and the one `diag` writes.

### 8.3 Compatibility

- `ragon-admin logs` with no target keeps today's behaviour, CLI §4.8: it follows the engine's container. With the new reader that means `logs engine --follow`, or `logs sglang` when SGLang is the running engine; the reader asks the server, not the config.
- `logs server` and `server logs` are kept as aliases.
- `logs mcp` keeps its current meaning: `logs codex --grep mcp`, lifecycle only.
- `--port` is kept and picks the engine's container by port.

---

## 9. `ragon-admin diag`: the bundle the user may send by hand (O5)

```
ragon-admin diag [--hours 24] [--component c1,c2] [--out PATH] [--print]
```

`ragon diag` takes the same arguments.

**What it writes.** One archive, `~/.local/state/dreamference/diag/ragon-diag-<UTC>.tar.gz`, mode 0600. Every source is capped, and the archive is streamed rather than built in memory. It contains:

```
MANIFEST.json      created (UTC), --hours, components, and for every file: source, lines, bytes,
                   redaction counts by rule, "flagged" for §7.6's transitional files
versions.json      ragon-admin, ragon, Codex tag and patch list, ragon-serve version, git and image digest,
                   SGLang image digest, OS, kernel, NVIDIA driver, CUDA, GPU name, memory total;
                   the node id, never the host name
config.json        the effective DreamferenceConfig: every key; values that are enums, numbers or
                   Ragon paths; "[set]" or "[unset]" for anything else; the effective RAGON_LOG
logs/<component>.log   the last N hours, §5 lines, re-redacted (cap: 20 MiB each)
logs/codex.log     the lifecycle extract from logs_2.sqlite, re-redacted (cap: 5 MiB)
starts/            the start records (§6.4)
incidents/         the incident records within --hours, or the newest 5 if none are inside it
host/              df for /, the model and state directories; free memory; PSI averages; the
                   `nvidia-smi -q` sections for clocks, memory, power and errors; `docker ps`;
                   `docker inspect` of Ragon's containers without Env, with mount sources as `~`
units.txt          `systemctl --user list-units 'ragon-*'` and their states
```

**What it never contains:**
- artefacts (§7.6);
- `logs_2.sqlite` itself;
- `config.toml`'s provider secrets;
- `log-salt`;
- any key, token or credential file;
- the contents of `~/.ragon` beyond the files above.

**Re-redaction, failing closed.** Every text file in the archive passes `redact()` a second time, as at `info` (O9: addresses become pseudonyms in a bundle). Then a final scan looks for:
- the machine's own home path, host name, FQDN and user name;
- every address on its network interfaces;
- the shapes from §7.4.

If anything remains, `diag` writes no archive. It names the file and line and exits non-zero. That is a bug in a writer, and the fix belongs there.

**What it prints:**

```
✅ Wrote ~/.local/state/dreamference/diag/ragon-diag-20261108T140312Z.tar.gz (3.4 MB)
   24 h of 14 components, 3 start records, 1 incident. Redacted: 212 values (see MANIFEST.json).
   Nothing was sent. Look inside before you share it:  tar -tzf <file>  ·  ragon diag --print
```

`--print` lists what would go in, with sizes, and writes nothing.

**Sending.** The product has no command, button or address for sending a bundle (rule 1). The user attaches the file wherever they choose, by hand.

---

## 10. The Diagnostics page (`ragon web` and the desktop app)

The desktop app and `ragon web` show one page at `/diagnostics`: in the app, Settings › Diagnostics, and in the web UI, the same page. It is **read-only** and contains:

1. **Components.** The rows of `logs list` (§8.1), with a status dot from the unit's or container's state and the time of the last `ERROR`.
2. **The engine.** The last start record: version, git, image digest, model, plan, flags. Below it come the last 20 `request done` lines as a table of counts and timings.
3. **A log viewer.** One component at a time, the last 500 lines, filtered by level, with a text filter in the page. It is fed by `ragon logs <c> --json -n 500`, and by `--follow` while it is open.
4. **Incidents.** The list of incident records, each openable as text.
5. **A note** with the command to make a bundle (`ragon-admin diag`), the path where bundles go, and "Nothing is sent".

**What it cannot do:**
- change a level;
- make, list or download a bundle;
- show an artefact;
- open a path outside `logs/`.

The route is a `GET` that serves only these views. It goes into `ragon-rs/web/policy.json` as read-only, and both the web server and the app run its conformance cases. A paired device sees the page behind its credential (O7). SYNC still never carries logs: the page reads live from the node it is connected to.

---

## 11. Tests

### 11.1 Redaction fixtures (shared)

`ragon-rs/vectors/logging/redact/*.json` holds cases of the form `{ "level", "message", "fields": [{ "type", "key", "value" }], "env": { "home", "host", "user", "addrs" }, "expected" }`. Rust, Python and C++ each have a test that runs every case and compares the line byte for byte. The cases include at least:
- every row of §7.1, given in the wrong type, which must not appear in the line;
- every token shape in §7.4;
- `prompt_tokens=18233` and `key_count=3`, which stay, and `api_key`, `web_token`, `auth` and `x_password`, which become `[secret]`;
- a URL with user info, a query and a fragment;
- a home path, a host name and a user name taken from `env`, never from the real machine;
- a 10 KB value, cut at 4 KiB with `truncated=1`;
- a value with a newline, a quote and a control character;
- an address at `info` (`[addr]`) and at `debug` (as is);
- a pseudonym, stable for one salt and different for another.

The fixtures use only the documentation addresses and placeholder names (ORGANISATION §7.8).

### 11.2 The format test

`tests/test_log_format.py`:
- Every writer's output in every test matches `line.re`.
- No `logging.basicConfig`, `logging.StreamHandler(` or `FileHandler(` appears outside the Python writer.
- Every `ragon_log.*` and `logger.*` call has a literal first argument; this is checked with `ast`, not regular expressions.
- The Rust daemon crates (`web`, `chat`, `sync`, `monitor`, `signal`, `code-index`, `docs-index`) count `eprintln!` under a ratchet that may not rise, as the ruff ratchet does.

### 11.3 The leak test

Each writer runs under conftest's temporary `HOME`, with a fake host name, a fake user name and a planted token in its environment. The test asserts that none of them appears in any line, file or bundle. `diag` is built from a fake tree with a planted secret in a log file and must refuse (§9). A second run, with the secret removed, must write the archive.

### 11.4 Holding the rules in code

- The launch builders never emit `--enable-log-requests`, `--enable-log-outputs`, `--max-log-len` (vLLM) or `--log-requests` (SGLang), checked for every registry entry and engine.
- No product code sets `RAGON_DUMP_*`, `RAGON_STEP_TRACE` or `RUST_LOG=trace`. Benchmarks under `bench/` and `scripts/` are exempt.
- Every `docker run` builder calls `ContainerLogOptions`, checked by building every container's argument list with Docker mocked.
- Every unit template Ragon writes is named `ragon-<component>…` with a component from `components.txt`, and sets `SyslogIdentifier` and `EnvironmentFile`.
- `ragon-rs/logs` and the Python writer import no network client.
- The egress audit's new check (§7.7) has a fixture: a strace log in which a process opens `logs/` and connects to `192.0.2.10`, which must fail.

### 11.5 The index test

The existing index tests over `specs/README.md` hold the row for this document.

---

## 12. Companions

**PROFILING** ([DREAMFERENCE_PROFILING.md](./DREAMFERENCE_PROFILING.md), not written yet) owns performance measurement: what is timed, how a profile run is started, and how results are compared across builds. Its inputs come from this spec:
- the engine's step trace and perf records, JSON lines (§5.4, §5.5);
- the `ms`, `s` and `tok_s` fields of `request done`;
- the start records, so that a profile is tied to a version and a plan.

PROFILING may add fields, but they must be of §5.3's types and must meet §7. It may not add a content capture. A profile run that needs one, such as a hidden-state dump, is a developer tool under §7.3, and its output never enters `logs/` or a bundle.

**MONITORING** ([DREAMFERENCE_MONITORING.md](./DREAMFERENCE_MONITORING.md), not written yet) owns health checks and alerts: which conditions matter, how often they are checked, and how the user is told. It **reads** logs, through `ragon logs --json` and the journal, and **writes none of its own** beyond the §5 lines of its own component. That includes today's session monitor's `checks.log`, which moves under §3.2. An alert it sends through a messenger the user turned on carries a fixed message, a component, a level and counts, never a log line or a field value beyond §5.3's types.

---

## 13. Rollout

Building waits for the owner's go. Each phase is shippable alone.

| Phase | Ships | Size |
|---|---|---|
| **P0: stop the leaks** | the vLLM request and output flags removed (O10); the fleet's file name uses the node id and its command output becomes an artefact; the model gate's timestamp in UTC with `Z`; `ContainerLogOptions` on every `docker run` (§6.3); §11.4's flag and container tests | small: Python only |
| **P1: the writers** | `components.txt`, `line.re` and the redaction vectors; the Python writer and `setup_logging()` at every entry point; `ragon-rs/log`; `log_level` and `RAGON_LOG` with `log.env`; `SyslogIdentifier` and `EnvironmentFile` in every unit; the files of §3.2 moved into `logs/` (`ragon-code`'s out of the user's repository); the app and app-server streams through `redact()`; the format, leak and index tests | medium |
| **P2: the engine** | `log.hpp`; the version stamp from CMake; the start line and record; `request done` and `request refused` in the format; `ts` in the step trace; the vectors' copy, in the engine repository through its queue | small, in `ragon-engine` |
| **P3: reading** | `ragon-rs/logs`; `ragon logs` and `ragon-admin logs list|<component>|all|level|prune` with the aliases of §8.3; `--json` | medium |
| **P4: the bundle** | start and incident records (§6.4); `ragon diag` and `ragon-admin diag` with re-redaction that fails closed; the egress audit's new check; SYNC §4's two rows | medium |
| **P5: the page** | `/diagnostics` in `ragon web` and the desktop app; its policy entry and conformance cases | small to medium |
| **P6: Windows** | the file sink and rotation at the locations of §3.5; Event Log lifecycle lines for `RagonNode`; `ragon logs` and `ragon diag` on Windows, with the Windows node's build | with WINDOWS_NODE |
| **P7: Codex's store** | an audit of what `logs_2.sqlite` records at TRACE in a real session, with fixtures, and a decision with the owner: keep it as is (O8), or a small patch lowering the default filter to INFO for named targets | small; a patch only if the audit finds content |

---

## 14. Acceptance

Every statement here is a test or a check someone can run.

1. On a node with the default configuration, after one agent session, one Night Shift task and one sync round, every line in `logs/`, in `journalctl --user -t 'ragon-*'` (Ragon's identifiers) and in `docker logs` of Ragon-owned containers whose writer Ragon controls matches `line.re`.
2. In the same run, a scan of all those sources finds none of the following:
   - the home path, host name or user name;
   - any non-loopback address at `info`;
   - any substring of the session's prompts, completions, tool arguments or tool outputs. The test plants a 32-character marker in each.
3. With `RAGON_LOG=trace`, statement 2 still holds.
4. Every Ragon-started container has `max-size` and `max-file` in `docker inspect … .HostConfig.LogConfig`.
5. No launch builder, for any registry entry and engine, emits a request- or output-logging flag.
6. `ragon-admin logs level info,engine=debug` changes `log_level` in the user config and `log.env`, and the engine's next start logs `DEBUG` lines while every other component stays at `INFO`.
7. `logs/` never exceeds 256 MiB after `logs prune`. No file in it is older than 14 days, except start records (20 per component) and incident records (30 days).
8. The engine's start line carries `version`, `git`, `image`, `model` and `flags`. The start record holds the plan. A bundle's `versions.json` matches them.
9. `ragon-admin diag` writes one archive with mode 0600, prints its path and "Nothing was sent", and contains no file listed in §7.6. With a planted secret it writes nothing and exits non-zero.
10. The egress audit fails a process that reads `logs/` and connects to a non-loopback address, and passes today's sessions.
11. The Diagnostics page, from a paired device, shows components, the engine's start record and a log tail. It offers no control that changes a level or makes or downloads a bundle. Its route is read-only in `policy.json`.
12. On Windows, `ragon logs list` and `ragon diag` work against `%LOCALAPPDATA%\Ragon\logs\` and `%ProgramData%\Ragon\logs\`, and the Event Log holds only the lifecycle lines of `RagonNode`.
13. The redaction fixtures pass byte for byte in Python, Rust and C++.

---

## 15. Decisions

*(Left empty for the owner.)*
