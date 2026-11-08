# Node monitoring: what a Ragon node watches, where it says so, and what it may do on its own

**Status:** Proposed; spec only, building later. The owner's questions are in §0.1.

**Names:** post-rename (`ragon`, `ragon-admin`, `~/.ragon`, `ragon web` on port 3100). Nodes are written `node-a` (the first node set up, the one the user works from) and `node-b` (another GB10); real names and addresses are never written here. Paths are repository-relative or under `~`.

**Builds on:**
- [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md): the client and node halves, the advertised service and the forced-command channel to a second node (§3, §18);
- [RAGON_FLEET](./DREAMFERENCE_RAGON_FLEET.md): several GB10s managed from one;
- [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §4.1 (the `RagonNode` service supervisor) and §4.2 (unattended lifecycle and host safety on Windows);
- [RAGON_SWE_BENCH](./DREAMFERENCE_RAGON_SWE_BENCH.md) §18: the model gate and its hold during a run;
- [RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md) §2.6 (the forge's memory allowance) and its mirror and backup timers;
- [RAGON_REMOTE_ACCESS](./DREAMFERENCE_RAGON_REMOTE_ACCESS.md): the overlay, the node's CA and the public certificate;
- [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md) §0 item 7 and §18: notices to the phone through the bridges the user already has on;
- [RAGON_CHAT](./DREAMFERENCE_RAGON_CHAT.md), [RAGON_SIGNAL](./DREAMFERENCE_RAGON_SIGNAL.md): the bridges and how each behaves under `/airgapped`;
- [RAGON_CLAUDE_PLUGIN](./DREAMFERENCE_RAGON_CLAUDE_PLUGIN.md) Part B §7: the *session* monitor `ragon monitor`, whose alert rules this spec reuses and whose names it must not collide with;
- [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) and [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md);
- the engine's own specification in the `ragon-engine` submodule: `specs/API.md` §6 (the counters on `/metrics`) and `specs/SESSIONS.md` (resident session slots);
- `docs/dev/host-safety.md`: the two layers, the pre-load plan and the side-service allowance.
- **Not yet on main, written in parallel:** `DREAMFERENCE_LOGGING.md` and `DREAMFERENCE_PROFILING.md`. §10 states the interface this spec assumes from each; no section of either is cited.

**What prompted it.** A node today has many health checks, each behind its own command (`ragon-admin status`, `server status`, `night status`, `remote status`, `node status`, `forge status`, the sidecars' own `status`), and nothing that runs them continuously, keeps their history or tells the user when one goes wrong. The one alert a product unit sends today (the public certificate's renewal failure) calls a script the product does not ship (§2.6). A model server that wedged at 02:00 is found at 09:00 by the first prompt that hangs.

---

## 0. Decisions, stated first

1. **One small resident watcher per node, in Rust,** `ragon-node-monitor`, a user service beside `ragon web`. It only reads, samples on fixed intervals, keeps its history on the node, and is bounded at 64 MiB (§8). The session monitor `ragon monitor` stays what it is (§2.7).
2. **Each watched item has a probe, an interval and thresholds** (§3), and a state: `ok`, `degraded`, `down`, `off` (not turned on, never an error) or `unknown` (the probe itself could not run). The node has one state, the worst that counts (§4).
3. **It tells the user through what is already theirs:** the CLI, journald, a status page in `ragon web`, a chip in the desktop app, and, only after `ragon-admin monitor notify telegram|signal|matrix`, one of the user's messengers. Off by default; never a cloud, never telemetry (§5).
4. **It does almost nothing on its own:** it may restart a failed unit once; it re-pulls nothing, reloads no model, never kills the model server except through the PSI watchdog's existing path, never reboots and never power-cycles. Every action is journalled with a reason the user can read (§6).
5. **Several nodes: one fleet status over the channels that already exist,** the forced SSH command and the open model port; no new open port (§7).
6. **Windows gets the same items and states** through the Windows equivalents of systemd, journald and PSI (§9).
7. **Phase 1 is a status page and a CLI with no alerts,** over probes that already exist (§11).

### 0.1 Questions for the owner

| # | Question | Answer |
|---|---|---|
| O1 | Names: the crate `ragon-rs/node-monitor/`, the binary and user unit `ragon-node-monitor`, state under `~/.local/share/dreamference/monitor/node/`, config in `[monitor.node]`, the CLI `ragon-admin monitor …`; the session monitor keeps `ragon monitor`, `ragon-monitor.timer` and the rest of `monitor/` (§2.7, §8.1)? | **Recommended: yes** |
| O2 | The resident watcher in Rust rather than Python: 64 MiB against a Python process that must avoid most of `dreamference`'s import tree, and the same language as the Windows supervisor (§8.2)? | **Recommended: Rust** |
| O3 | The liveness completion: a one-token request to the engine only when it has served nothing for 10 minutes, at most every 10 minutes, never while the gate holds for a SWE-bench run and never when every resident session slot is taken, so that it can never evict a user's session; otherwise the engine's own counters prove liveness (§3.3)? | **Recommended: yes, idle only** |
| O4 | A PSI guard while the model server is resident: run the existing `MemoryPressureWatchdog` persistently as its own small unit, `ragon-psi-guard`, so a pressure spike after the load is caught by the same thresholds and kill path as during it; the monitor only reports (§6.3)? | **Recommended: yes** |
| O5 | Docker reloads the model container after a crash or reboot (`--restart unless-stopped`) with no pre-flight and no watchdog. For now the monitor reports such a load as `degraded` ("loaded by Docker, not by `server start`"); changing the restart policy is a separate backlog item (§2.2, §3.1)? | **Recommended: report now, change later** |
| O6 | Pages only for `down` (and for a few named `degraded` items, §4.4); everything else in one daily digest at 08:00 local; quiet hours 23:00–07:00 as the session monitor's, during which only the urgent pages go out (§4.4)? | **Recommended: yes** |
| O7 | Delivery: the monitor's alerts reach the bridges through a new always-on route in `ragon web`, `/api/monitor/notices`, read by the same poll the bridges already run for sync notices; when `ragon web` itself is down, each bridge says so after five minutes of failed polls (§5.3)? | **Recommended: yes** |
| O8 | Replace the call to `~/.local/bin/owner-notify` in `ragon-remote-certificate-alert.service` with `ragon-admin monitor notify send`, so the product's one alert goes through the product (§2.6, §5.3)? | **Recommended: yes** |
| O9 | Fleet: only the node where `monitor notify` is set sends pages, for every node it watches; every node keeps its own history (§7)? | **Recommended: yes** |
| O10 | The monitor may restart a failed unit from a fixed list, once per unit per six hours, and only after systemd's own `Restart=` has given up (§6.1)? | **Recommended: yes** |
| O11 | The monitor service on by default on every node from Phase 2, with notifications off until `monitor notify` names a channel (§5, §11)? | **Recommended: yes** |

---

## 1. Scope and the rules that bind it

**In scope:** watching a node's own health continuously (the model server and its gate, memory, disk, GPU, units, sidecars, the forge, remote access, certificates, backups), keeping the history on the node, showing it, telling the user, and the few actions in §6. Watching the other nodes of the user's fleet from one of them.

**Out of scope:** application logs and their retention (LOGGING, §10.1); deep measurement such as traces and kernel timelines (PROFILING, §10.2); the session monitor's checks of agent transcripts (CLAUDE_PLUGIN Part B); benchmarking; monitoring a client machine that serves no model (a client sees its node's status through the node).

**Product rules that bind every section:**
- **Local and air-gapped. No telemetry, ever.** Nothing leaves the machine, the LAN or the user's overlay unless the user sends it. A notification is sent only to a messenger the user has set up and named in `monitor notify`. The egress audit keeps passing with the monitor on (§12).
- **One node serves one model on a GB10.** The monitor knows which model by asking the server (`model_key_for_served_id`), never the config.
- **Host safety on unified memory is a hard constraint.** The monitor is itself a resident process beside the model server and is bounded (§8.3). It never starts a load.
- **The messengers are the user's own channels and are off until turned on.** The monitor turns none on; `monitor notify` only chooses among those already on.
- **Windows node parity** (§9).
- **Tests never touch the real machine** (§8.5).
- **The engine is ragon-serve; SGLang is the fallback.** Every engine probe reads both engines' counter names, as `NightShiftHost.metrics` does today.

## 2. Today

### 2.1 Health checks that exist, each on demand

| What | Where | What it does |
|---|---|---|
| `ragon-admin status` | `dreamference/cli/dreamference_cli_controller.py:1392` (`handle_status`) | Hardware, server status, the context index. Imports the hardware stack, which pulls in torch: about two seconds and 0.7 GB per call, unfit for a loop. |
| `server status` | `dreamference/vllm_server/vllm_server_manager.py:2444` (`get_server_status`) | Health, `/v1/models`, `docker inspect`, recent logs. |
| Night Shift's host probes | `dreamference/night_shift/night_shift_host.py`: `served_model` :157, `metrics` :177, `host_safety_ok` :286, `mem_available_bytes` :303, `scope_memory_current` :345 | The served model, the engine's counters under both engines' names, the host-safety verdict, MemAvailable, a scope's memory. Every one only reads. |
| The gate | `dreamference/vllm_server/model_gate.py:209` (`probe`), `model_gate_service.py:638` | `GET /ragon-gate` answers `{"gate": "ragon", "state": …}` with the pause, the SWE-bench run holding the gate, its heartbeat, and a stale run. |
| The tool-call canary | `dreamference/vllm_server/tool_call_canary.py` | One tool-call request at server start; never again. |
| Per-feature `status` | `dreamference_cli_controller.py`: night :2138, search engines :2741, images :2782, voice :2799, matrix :2821, remote :2876, desktop :2678 | Each prints its own feature's state. |
| Nodes | `dreamference/node/node_advertiser.py:163` (`NodeAdvertiser.status`), `node_remote.py:147` (`NodeRemote.status`), `node_remote.py:27` (`list_lines`) | This node's advertisement; another node's state over the forced command; every node's model, context and load read from its open model port, without SSH. |
| The forced command | `dreamference/node/node_serve.py:48` | `status`, `stop`, `start`, `info` (JSON), `set-model`, `unpair`, `model-receive`, `job-*`. |
| Fleet probe | `dreamference/node/fleet_probe.py` | A POSIX script printing `key=value` lines about a machine. |
| Free disk | `dreamference/forge/forge_storage.py:110`, `dreamference/node/node_model_sync.py:251` | Each feature checks the room it needs before writing. |
| Certificates | `dreamference/remote/remote_certificate_authority.py:273` (`expiry`) | `remote status` prints "valid N more days". |
| The launcher | `ragon-rs/src/lib.rs:1126` | Waits for the server and reads `/v1/models` before every session. |
| `ragon web` | `ragon-rs/web/` | `/healthz` (its own), `/api/airgapped`, `/api/night/status`, `/api/forge`, `/devices`, `/api/docs`. No page shows the node's health. |
| The desktop app | `desktop/ui/` | The model chip (`ComposerView.tsx:387`), a working-count chip (`App.tsx:3293`), the node list with in use / away (`SettingsDialog.tsx:553`). No health chip. |

### 2.2 Host safety guards only the load

`check_host_safety()` (`vllm_server_manager.py:1127`) refuses an unsafe host before a load (sar present, earlyoom or systemd-oomd, swap of at least `MIN_SWAP_GB` = 64, `MIN_FREE_KBYTES` = 1048576, a transient of at least `MIN_LOAD_TRANSIENT_GB` = 4). The `MemoryPressureWatchdog` (`dreamference/vllm_server/psi_watchdog.py`: full avg10 ≥ 60 % for 5 s, or avg60 ≥ 25 %, sampled every second from `/proc/pressure/memory`, three kill paths, then `docker update --restart=no` at :424) is a thread of the `server start` process: created at `vllm_server_manager.py:2333`, stopped at :2372, :2385 and :2402. **Once the model is loaded, nothing watches PSI** except earlyoom or systemd-oomd.

The model container runs with `--restart unless-stopped` (`vllm_server_manager.py:1076`), and so does the gate (`model_gate.py:112`). **After a crash of the engine or a reboot, Docker loads the model again with no pre-flight, no load plan and no watchdog.** `docs/dev/host-safety.md` records why this matters on GB10: ragon-serve's allocations are charged to no cgroup, so only PSI and a MemAvailable floor see them.

### 2.3 Units and containers that restart themselves

User units with `Restart=on-failure`: `ragon-web` (`ragon-rs/web/src/cli.rs:194`), `ragon-chat` (`ragon-rs/chat/src/cli.rs:89`), `ragon-sync` (`ragon-rs/sync/src/cli.rs:213`), `ragon-forge` (`dreamference/forge/forge_service.py:104`); `ragon-signal@` under its system account (`ragon-rs/signal/src/unit.rs:199`); `Restart=always` on `ragon-remote-tunnel` and the Matrix units. Timers: `ragon-night.timer`, `ragon-forge-tick.timer` (every 10 minutes), `ragon-forge-backup.timer`, `ragon-forge-check.timer` (a monthly `restore --check`), `ragon-forge-mirror.path`, `ragon-remote-certificate.timer` (daily, 04:17 UTC). Also `ragon-node.service`. The sidecars (SearXNG, the Google service, image search, speech-to-text) are Docker containers with `--restart unless-stopped`.

Nothing records how often any of these restarted, and a unit that hit systemd's start limit stays failed until someone runs `systemctl --user status`.

### 2.4 Power-cycling exists and must stay manual

`dreamference/node/node_power_cycle.py` and `node_shelly_plug.py` power-cycle a hung node through a smart plug's local API. The monitor never calls them (§6).

### 2.5 The notice path to the phone

`ragon web` serves `/api/sync/notices?after=` (`ragon-rs/web/src/sync_hub.rs:70`, :434; `struct Notice` at `sync_live.rs:186`) **only when sync is on**. The chat hub polls it (`ragon-rs/chat/src/hub.rs:1279`, `push_sync_notices`; `chat_repos.rs:1103`, `notices_after`, for Telegram and Discord), and so does the Signal bridge (`ragon-rs/signal/src/serve.rs:300`), which reads through `ragon web` because its system account cannot read the user's config. Under `/airgapped on`: Signal acts only at `off`, Telegram pauses, Matrix keeps working.

### 2.6 One product alert, sent through a script the product does not ship

`ragon-remote-certificate.timer` has `OnFailure=ragon-remote-certificate-alert.service`, whose `ExecStart` is `%h/.local/bin/owner-notify '…'` (`dreamference/remote/remote_public_certificate.py:641`). No installer puts `owner-notify` there; only the project's own scripts use it (`scripts/qa`, `scripts/remote_acceptance`, `scripts/messenger_live`). On a user's node this unit fails silently, which is the one case it exists for.

### 2.7 The session monitor, and the names it already holds

`ragon-rs/monitor/` is the `ragon-monitor` crate behind `ragon monitor` (CLAUDE_PLUGIN Part B §7, M0). It reads agent transcripts, runs checks C1–C7 (`checks.rs`), writes `summary.md`/`summary.json` (`summary.rs`) and alerts through a `notify_command` (`config.rs`, run with a 30 s timeout at `cli.rs:187`). Its alert rules are what a node monitor needs too (`alerts.rs`): episodes so one fault is one alert, at most one alert per kind and subject per hour, `max_alerts_per_hour`, quiet hours 23–07 with urgent alerts still sent. Its unit (`units.rs`) runs every five minutes from `ragon-monitor.timer` under `MemoryMax=512M`, `Nice=10`, `IOSchedulingClass=idle`; the hourly digest timer is off.

It already holds `~/.local/share/dreamference/monitor/` (`config.rs:52`), the `[monitor]`, `[monitor.waits]` and `[monitor.rules]` tables, and the unit names `ragon-monitor.*`. This spec takes none of them (O1).

### 2.8 Backups

No NAS or general backup spec exists. The forge's backups do (`dreamference/forge/forge_backup.py`, `ragon-forge-backup.timer`, the monthly check). §3 watches those; a NAS item is added when its spec is written.

## 3. What is watched

Each item: its probe, its interval, and the thresholds for `degraded` and `down`. A threshold is reached only after the hysteresis in §4.2. Every number is a default under `[monitor.node]`; none is a hard limit of the product. An item whose feature is not turned on is `off`.

### 3.1 The model server

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| Container | `docker inspect` on the model container: running, restart count, `StartedAt` | 30 s | restarted by Docker since the last `server start` (O5); restart count rose | not running, and no `server stop` recorded |
| Engine health | `GET /health` on the engine's loopback port (`ModelGate.engine_port()`), 2 s timeout | 15 s | answers slower than 1 s | no 200 for 3 samples while the container runs, after the load window |
| Models | `GET /v1/models` through the gate's public port, as a client sees it | 60 s | the served id is not a registry model | fails while engine health passes (the gate or the port is the fault) |
| Liveness | §3.3 | — | — | — |

**The load window.** From a `server start` (or a container `StartedAt` the monitor has not seen) until `/health` first answers 200, the model server is `starting`, which counts as `degraded` for the node and never pages; a load that has not finished in 20 minutes is `down`.

### 3.2 The gate

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| Gate | `GET /ragon-gate` on the public port (`ModelGate.probe`) | 30 s | `stale` (a run's heartbeat is old: a SWE-bench run died holding the gate); a gate from an older install (`is_stale`) | no answer while the container runs |

A gate holding for a live run is `ok` and shown as "holding for a SWE-bench run, N/M done", never as a fault: requests from outside the run are refused on purpose. `paused_until` is shown too.

### 3.3 Liveness: is the engine still producing tokens

A `/health` 200 proves the process answers HTTP, not that it decodes. Liveness is decided in this order:

1. **Counters, free:** if the engine's generation counter (`sglang:generation_tokens_total`, under the name ragon-serve and SGLang both export) rose since the last sample, the engine is live. Nothing else runs.
2. **Nothing to prove:** if no request is running or queued (`num_running_reqs` and `num_queue_reqs` are 0) and the counter did not move, the engine is idle, not dead.
3. **A one-token completion,** only when (O3): the engine has served nothing for 10 minutes; the last such probe was 10 or more minutes ago; the gate is not holding for a run (§3.2); and at least one resident session slot is free (`ragon:sessions_resident` below the engine's slot count, ENGINE `SESSIONS.md`). ragon-serve keeps a small number of resident sessions (two by default in Ragon); a probe that took the last slot could evict a user's session and cost them a full prefill. The request goes to the engine's loopback port with `max_tokens` 1 and no session id, a 30 s timeout, and is logged as the monitor's.
4. **Wedged:** requests are running or queued and the generation counter has not moved for 60 s: `down` ("requests waiting, no tokens for 60 s"). This is the case that today is found by the next prompt that hangs.

`degraded` when one probe completion fails; `down` after two in a row, or in case 4.

### 3.4 The engine's counters

Read from `/metrics` on the engine's loopback port every 30 s, under both engines' names (ENGINE `API.md` §6; parsed today by `NightShiftHost.metrics`). These are recorded for the history and the status page; few of them change a state.

| Counter | Derived | `degraded` |
|---|---|---|
| `generation_tokens_total`, `prompt_tokens_total` | decode and prefill tokens per second over the window | — (shown only) |
| `time_to_first_token_seconds` (sum and count) | mean TTFT per window; its 7-day median | mean over 15 min above three times the 7-day median |
| `num_queue_reqs`, `num_running_reqs` | queue depth | more than 4 queued for 5 minutes |
| `max_total_num_tokens` and the tokens in use, where exported | KV pool use | above 95 % for 10 minutes |
| `ragon:sessions`, `ragon:sessions_resident`, `ragon:session_evictions_total` | session pressure, evictions per hour | more than 6 evictions in an hour |
| `ragon:nonfinite_logits_total` | any increase | any increase (shown with the time) |
| `ragon:repetition_stops_total` | per hour | — (shown only) |

The KV pool's in-use figure is exported by SGLang; whether ragon-serve exports it is for the engine's `API.md` §6 to say, and the row reads "not exported" until it does. `/spec_stats` (the speculative decoder's acceptance) is recorded every 5 minutes, shown only.

### 3.5 Memory

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| PSI | `/proc/pressure/memory`, `some` and `full` avg10 and avg60 | 10 s | `full` avg60 ≥ 5 % | `full` avg10 ≥ 30 % for 30 s, half the watchdog's trip |
| MemAvailable | `/proc/meminfo` | 10 s | below 8 GiB | below 4 GiB |
| Swap | `/proc/meminfo` | 60 s | more than 8 GiB used | — |
| OOM kills | `/proc/vmstat` `oom_kill` | 60 s | any increase (shown with the time) | — |
| Host-safety layer | earlyoom or systemd-oomd active; swap and sysctls as `check_host_safety()` requires | 15 min | any of them missing | — |
| PSI guard | `ragon-psi-guard` running while the model server is (O4) | 30 s | not running | — |

The monitor reports pressure; it does not act on it (§6.3). Its `down` comes before the watchdog's trip on purpose, so the user hears of a rising pressure before the model server is killed for it.

### 3.6 Disk

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| Each filesystem holding `/`, Docker's data root, the model cache, `~/.local/share/dreamference` and `~/.ragon` (each counted once) | `statvfs` | 5 min | below 50 GiB or 10 % free | below 15 GiB free |
| Growth | the free figure's slope over 24 h | 1 h | full within 3 days at that rate | — |

### 3.7 GPU

On GB10 the GPU shares the host's memory, so its memory is §3.5. The probe is the GB10 telemetry the `hardware/` package already reads (`nvidia-smi` queries): utilisation, temperature, power, clocks and throttle reasons, every 30 s, shown and kept. `degraded` when a thermal or power-brake throttle reason stays set for 5 minutes, or when the driver does not answer; `down` when the GPU is gone from the driver while the model server should run (an Xid that took the device).

### 3.8 Units, sidecars and Docker

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| Units | `systemctl --user show -p LoadState,ActiveState,SubState,Result,NRestarts` for the product's units (§2.3) that are enabled | 60 s | `NRestarts` rose in the last hour; a timer's last run failed | `failed`, or inactive while enabled |
| `ragon-signal@` | its state through `systemctl show` (it runs under its own system account; reading its state needs no privilege) | 60 s | as above | as above |
| Sidecars | `docker inspect` (state, health, restart count) for the sidecars that are on, and their own health URL on loopback | 60 s | restarted in the last hour | not running while on |
| Docker | `docker info` (a read, allowed) | 60 s | slower than 5 s | no answer |

A unit or sidecar that is not enabled is `off`. The list of product units is one table in the code, extended by each feature that adds a unit, with a test that every unit the product writes is in it.

### 3.9 Node, overlay and forge

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| Advertisement | `NodeAdvertiser.status()`: the service file and the published record | 5 min | published model differs from the served one | not published while the node is a node and advertising is on |
| Overlay | the remote-access tunnel unit and the overlay interface's state; the node's own overlay address answers on `ragon web`'s port | 5 min | the interface is up and the node does not answer on it | the tunnel unit failed |
| Forge | `ragon-forge` health on loopback | 60 s | — | not answering while on |
| Forge mirror | the mirror's last sync time, read from the forge's API on loopback | 10 min | behind by more than 1 h | behind by more than 24 h |
| Forge backup | the newest backup's age and the last monthly `restore --check` result | 1 h | older than 26 h | older than 72 h, or the check failed |

### 3.10 Certificates and keys

| Item | Probe | Interval | `degraded` | `down` |
|---|---|---|---|---|
| The node's CA | `RemoteCertificateAuthority.expiry()`'s source, read directly | 24 h | expires within 60 days | within 14 days |
| Issued node and device certificates | `notAfter` of each under the remote-access state | 24 h | within 21 days | within 7 days |
| The public certificate | its `notAfter`, and the last run of `ragon-remote-certificate.timer` | 24 h | within 21 days, or the last renewal failed | within 7 days |

The keys themselves are never read into the monitor; only the certificates' public fields are.

### 3.11 The monitor itself

A heartbeat file written every sample cycle. `ragon-admin monitor status` says "the monitor has not run for N minutes" when it is older than three intervals, and `ragon web`'s page says the same. Its own memory and CPU are shown (§8.3).

## 4. States

### 4.1 Per item

`ok`, `degraded`, `down`, `off` (the feature is not turned on), `unknown` (the probe could not run: a permission, a missing tool, a timeout of the probe itself). `unknown` is shown and counted as `degraded` for the node only after an hour, so a missing tool is not a page.

### 4.2 Hysteresis

- An item enters `degraded` or `down` after its threshold holds for **N consecutive samples** (default 3; 1 for the daily and hourly probes) and leaves it after **M consecutive good samples** (default 3) and at least 2 minutes.
- **Flapping:** three or more state changes within 30 minutes hold the item at `degraded` with the note "flapping" until it has been stable for 30 minutes. A flapping item sends one page, not one per change.
- Thresholds have a gap where both a rise and a fall are needed, e.g. MemAvailable enters `degraded` below 8 GiB and leaves it above 10 GiB.

### 4.3 The node's state

The worst of its items, except that only these items can make the node `down`: the model server (§3.1, §3.3), memory (`down` in §3.5), and the disk holding the model cache or `~`. Every other `down` makes the node `degraded`. A node with no model server running by the user's choice (`server stop` recorded) is `idle`, not `down`.

### 4.4 Page or digest

- **Page** (sent as soon as it happens, through §5.3): the node goes `down`; any item goes `down`; the model server or the gate stays `degraded` for 30 minutes; a restart action failed (§6.1); the PSI guard killed the model server; a certificate within 7 days.
- **Digest** (once a day at 08:00 local, O6): every item that was `degraded` in the last 24 h, the restarts, the actions, the counters' day (tokens, mean TTFT, evictions), disk growth.
- **Recovery** is sent only for an item that was paged, as one line ("model server ok again after 14 min").
- **Quiet hours** 23:00–07:00 (the session monitor's default): only the node going `down`, memory `down` and disk `down` are sent; the rest wait for 07:00 or the digest.
- **Rate:** the session monitor's rules, reused (`alerts.rs`): one episode is one page, at most one page per item per hour, at most 6 pages per hour in all.

## 5. Channels

### 5.1 Local, always on

- **CLI:** `ragon-admin monitor status [--json] [--fleet] [--item NAME]` prints the node's state and every item that is not `ok` (`--all` for every item); `ragon-admin monitor history [--since 24h] [--item NAME] [--json]` prints transitions, actions and sampled values; `ragon-admin monitor actions` lists §6's journal. Each exits 0 for `ok`, 1 for `degraded`, 2 for `down`, so a script can use it.
- **journald:** every transition and every action is one journal entry with `SYSLOG_IDENTIFIER=ragon-node-monitor` and structured fields (`MONITOR_ITEM`, `MONITOR_STATE`, `MONITOR_FROM`, `MONITOR_REASON`), at `warning` for `degraded`, `err` for `down`, `notice` for recovery and actions. Samples are never journalled.
- **`ragon web`:** a status page, `/status`, behind the same device credential as the rest of `ragon web` (a paired device sees it, nothing else does), and `GET /api/monitor/status` returning the JSON of `monitor status --json`. The page shows the node state, the items grouped as in §3, the last 24 h of tokens per second, TTFT, PSI and MemAvailable as small charts, and the actions. Both routes are added to `ragon-rs/web/policy.json`, with conformance cases, so the desktop app's enforcer allows them too.
- **The desktop app:** a status chip beside the model chip, coloured by the node state, opening the status page in a window. It reads `/api/monitor/status` from the node it uses, so a client shows its node's health.

### 5.2 Storage

The monitor's data stays on the node, under `~/.local/share/dreamference/monitor/node/`:

| File | What | Retention |
|---|---|---|
| `monitor.sqlite` (WAL) | samples, 1-minute and 1-hour rollups, transitions, actions, sent alerts | raw samples 48 h; 1-minute rollups 14 days; 1-hour rollups 400 days; transitions, actions and alerts 400 days |
| `status.json` | the current state, rewritten each cycle (atomic rename) | current only |
| `heartbeat` | the time of the last cycle | current only |

The database is capped at 256 MiB: when it passes the cap, the oldest raw samples go first, then the oldest minute rollups. Pruning runs hourly; `VACUUM` weekly when idle. sqlite, not ring files, because `history` asks by item and time range, and the rollups are queries.

### 5.3 The user's messengers, off by default

`ragon-admin monitor notify telegram|signal|matrix|off` names **one** channel (O11); `off` is the default. It accepts only a messenger the user has already set up (`ragon chat telegram setup`, `ragon signal setup`, `ragon-admin matrix start`) and says which command sets one up otherwise. `ragon-admin monitor notify test` sends one test line. `ragon-admin monitor notify send "<text>"` sends a line through the chosen channel; it replaces `owner-notify` in `ragon-remote-certificate-alert.service` (O8).

**Delivery (O7).** The monitor appends each page and digest to an outbox in its database. `ragon web` serves it at `/api/monitor/notices?after=`, independent of sync being on, in the same shape as `/api/sync/notices`. The chat hub and the Signal bridge poll it with the code that polls sync notices today, each only when it is the channel `monitor notify` names. A notice carries its text, its severity, the node's name and a link to `/status`.

**The circularity, said plainly.** Delivery needs `ragon web` and a bridge running, and both are things the monitor watches. So:
- when `ragon web` is down, the bridge's poll fails; after five minutes of failed polls the bridge sends one line of its own ("the node's web service is not answering; check `ragon-admin monitor status`"), and one when it answers again;
- when the bridge itself is down, nothing reaches the phone; the outbox keeps the page and the bridge delivers it, with its age, when it returns; the digest names any page that waited more than an hour;
- the CLI and journald always work.

**Under `/airgapped on`.** Signal sends nothing and Telegram pauses (§2.5): a page for those channels waits in the outbox and is delivered, with its age, when the level returns to `off`; pages older than 24 h are folded into the next digest instead. Matrix keeps working. The status page, the app's chip, the CLI and journald are unaffected by the level.

**No other channel.** No e-mail, no push service, no webhook to a third party, no vendor cloud. A user who wants another channel uses `[monitor.node] notify_command`, the session monitor's mechanism: a local command run with the text as its argument and a 30 s timeout, which the user writes and owns.

## 6. Actions

The monitor watches; it acts only in the cases below, and every action is a row in the actions journal (§5.2), a journal entry and a line on the status page, each with the reason in words the user can read: what was seen, what was done, what happened.

### 6.1 Restart a failed unit, once (O10)

For a unit in the product's list (§3.8) that is `failed` after systemd's own `Restart=` has given up (the start limit hit, or a unit with no `Restart=`), the monitor runs `systemctl --user reset-failed` and `systemctl --user start` for that unit, **once per unit per six hours**. If the unit fails again, the monitor pages "restarted once at 03:12, failed again: <the unit's last journal line>" and does nothing more. Never for: the model server, the gate, a unit under another account (`ragon-signal@`), a timer's service whose next run will retry anyway, or any unit while the node is `down` for memory. A sidecar container is not restarted by the monitor; Docker's policy already does.

### 6.2 What it never does

- It re-pulls nothing: no image, no model, no update.
- It never starts, reloads or restarts the model server. A load needs `check_host_safety()`, the load plan and the watchdog, which only `ragon-admin server start` has.
- It never kills the model server, except through the PSI watchdog's existing path (§6.3).
- It never reboots, never power-cycles (§2.4), never runs `sudo`.
- It never changes a setting, a restart policy or a messenger's state.

### 6.3 Memory pressure after the load (O4)

Today's watchdog protects only the load (§2.2). The recommendation: a small unit, `ragon-psi-guard.service`, started and stopped with the model server, running the existing `MemoryPressureWatchdog` with its thresholds and its three kill paths unchanged, and its `docker update --restart=no` after a kill so Docker does not reload into the same pressure. It is Python because the watchdog is, and it must import only `psi_watchdog.py` and what that needs (never the hardware stack), bounded at 64 MiB. The monitor does not kill: it shows the guard's state (§3.5), and when the guard fires it pages "the PSI guard stopped the model server at 03:12: full pressure 71 % for 5 s; `ragon-admin server start` loads it again after the pre-flight". This keeps one kill path in the product, written once.

## 7. Several nodes

One node watches the others over the channels that already exist; no new port opens on any node.

- **Every 60 s,** for each node in `node list`, the open model port is read as `NodeRemote.list_lines()` reads it today: the served model, its context and its load. A node that does not answer for 3 rounds is `unreachable`.
- **Every 5 minutes,** for each paired node, the forced command gains one read-only operation, `monitor-status`, which prints the remote monitor's `status.json`. It adds no privilege: the forced command already runs `status` and `info`.
- **Fleet status** is the list of nodes with each node's state and its items not `ok`: `ragon-admin monitor status --fleet`, and a "Fleet" section on the status page.
- **Pages (O9):** only the node where `monitor notify` names a channel sends pages, for itself and for every node it watches; another node's own monitor keeps its history and its local channels and sends nothing. A node that is `unreachable` is paged once by the watching node, with a hint that a hung node can be power-cycled by hand (`ragon-admin node power-cycle <node>`), never automatically.
- The watched nodes are found from the pairing records, by their recorded addresses, never by an mDNS browse in the loop (an mDNS query is what the egress audit counts as a failure).

## 8. Implementation

### 8.1 Where it lives

A new crate, `ragon-rs/node-monitor/`, building one binary, `ragon-node-monitor`, installed beside `ragon` (not on PATH; `ragon-admin monitor` runs it). It uses the session monitor's alert rules by depending on `ragon-rs/monitor` as a library (moving `alerts.rs`'s episode, rate and quiet-hours types behind a small public API there, with no behaviour change). It does not join the session monitor's crate: the session monitor reads transcripts on a timer, this one samples the host continuously, and the two have different units, bounds and owners.

### 8.2 Why Rust (O2)

- **Memory:** a resident process beside a model server on unified memory must stay small. A Rust sampler with sqlite fits in 64 MiB. A Python one could too, but only by avoiding most of `dreamference`'s import tree (`handle_status` pulls in torch at 0.7 GB), which every later change would have to keep doing.
- **Windows parity:** the Windows node's supervisor is Rust (WINDOWS_NODE §4.1); the probes behind a trait (§8.4) get a Windows implementation in the same crate.
- **Reuse:** `ragon web`, the bridges and the session monitor are Rust, so the notices route, the outbox and the alert rules are shared code, not a second copy.
- **What stays in Python:** `ragon-admin monitor`'s parser (a thin front that runs the binary and passes its exit code through), the steady-state PSI guard (§6.3), and the existing probes that the Rust side asks instead of re-implementing where they hold logic, not just reads: the host-safety verdict is read from `ragon-admin server preflight --json`, a new read-only command over `check_host_safety()`, every 15 minutes.

### 8.3 Its memory bound

`ragon-node-monitor.service` (O1): `MemoryMax=64M`, `MemoryHigh=48M`, `CPUQuota=5%`, `Nice=10`, `IOSchedulingClass=idle`, `TasksMax=32`, `Restart=on-failure`. Each probe has its own timeout and runs on one thread; a probe that hangs is abandoned and marked `unknown`, never waited on. The service is registered in `VLLMServerManager.side_service_allowance()` at its cap, as the forge is, so the pre-load check sets aside its 64 MiB like every other side service's.

### 8.4 Its shape

- A `Probe` trait: `name`, `interval`, `sample() -> Sample`, one implementation per §3 row. The host is reached only through a small `Host` trait (read a file, run a command with a timeout, an HTTP GET on loopback, `statvfs`), so every probe is testable with fixtures.
- A scheduler that runs the probes at their intervals, a state machine per item (§4), the store (§5.2), the outbox, and the actions (§6) behind an `Actions` trait.
- Config: `[monitor.node]` in `~/.config/dreamference/config.toml`: thresholds, intervals, `notify`, `notify_command`, quiet hours, the digest's hour. Unknown keys are refused.
- Subcommands: `run` (the service), `status`, `history`, `actions`, `notify set|test|send`, `doctor` (each probe once, with timings).

### 8.5 Its tests

All probes are mocked; nothing touches the real machine.
- Rust: every probe against fixture inputs (`/proc/pressure/memory` and `/proc/meminfo` text, `/metrics` text under both engines' names, `systemctl show` and `docker inspect` output, certificate files), the state machine's hysteresis and flapping, the node state rule, page-or-digest, quiet hours, the restart-once rule over a fake clock, retention and the size cap on a temporary database. HTTP probes run against an in-process server on an ephemeral port, never 8000 or 18000. Run with `cargo test` in a copy of the folder, as the session monitor's are.
- Python: `ragon-admin monitor` with the binary replaced by a stand-in; `server preflight --json` with the host mocked; the PSI guard unit's command line. `tests/conftest.py`'s guards stay as they are.
- `ragon web`: `/status`, `/api/monitor/status` and `/api/monitor/notices` with a fixture `status.json`; the policy conformance cases for both enforcers.
- A test that every unit the product writes is in the monitor's unit list (§3.8), and one that the monitor's binary never names a non-loopback host.

## 9. Windows parity

The items and the states are the same; the probes and the places change (WINDOWS_NODE §4.1, §4.2).

| Linux | Windows |
|---|---|
| `ragon-node-monitor.service` (systemd user unit) | a task of the `RagonNode` service supervisor, under its non-admin account |
| `systemctl show` for the units | the Service Control Manager's state of the node's services; Task Scheduler's last result for what are timers on Linux |
| journald | the Windows Event Log, under a `Ragon` source, with the same fields as event data |
| `/proc/pressure/memory`, `/proc/meminfo` | `GlobalMemoryStatusEx`, the low-memory resource notification, the DXGI memory budget for the GPU |
| The PSI guard's kill (§6.3) | `TerminateJobObject` on the engine's job object, as WINDOWS_NODE §4.2 describes |
| `statvfs` | `GetDiskFreeSpaceEx` |
| Restart once (§6.1) | the SCM's start of the stopped service, once per six hours |
| `~/.local/share/dreamference/monitor/node/` | `%ProgramData%\Ragon\monitor\` |

The status page, the CLI, the notices route and the fleet channel are the same code.

## 10. Relation to the other operations specs

### 10.1 LOGGING

LOGGING decides where the product's logs go, their format, their rotation and how long they are kept. The monitor writes no logs of its own beyond one journal entry per transition and action (§5.1), which follow LOGGING's rules for the journal, and it reads no log to decide a state: every state comes from a probe in §3. An alert names the command that shows the relevant log (`journalctl --user -u <unit>`, `ragon-admin server logs`) rather than quoting it. This spec assumes LOGGING keeps journald as the node's log sink on Linux and the Event Log on Windows; if it chooses otherwise, §5.1 follows it.

### 10.2 PROFILING

PROFILING covers deliberate, on-demand measurement: traces, kernel timelines, per-request breakdowns, run when someone asks. The monitor keeps only cheap continuous figures (§3.4) and never starts a profile; a monitor history of tokens per second and TTFT is what tells the user that a profile is worth taking, and when. This spec assumes PROFILING reads the engine's counters from the same `/metrics` and adds none that the monitor must sample; any it adds are shown here only if they cost one scrape.

## 11. Rollout

- **Phase 1: the status page and the CLI, no alerts, no service.** `ragon-node-monitor status --json` runs each probe once, over what exists today: the engine's `/health`, `/v1/models` and `/metrics`, the gate's `/ragon-gate`, `/proc`, `statvfs`, `systemctl show`, `docker inspect`, the certificates' `notAfter`, the forge's backup age. `ragon-admin monitor status`, `/status` and `/api/monitor/status` in `ragon web` (cached for 10 s), and the desktop app's chip. No history, no states over time, no actions, no messages.
- **Phase 2: the service.** `ragon-node-monitor.service` on by default on a node (O11): sampling at §3's intervals, the store, hysteresis and node state, journald, `monitor history`, charts on the status page, the liveness completion (O3). Still no messages and no actions.
- **Phase 3: alerts and the few actions.** `monitor notify`, `/api/monitor/notices` and the bridges' poll (O7), pages, digest, quiet hours, `owner-notify` replaced (O8), restart-once (O10), `ragon-psi-guard` (O4), the Docker-reload report (O5).
- **Phase 4: the fleet.** The forced command's `monitor-status`, fleet status, pages for other nodes (O9).
- **Phase 5: Windows,** with WINDOWS_NODE's phases.

## 12. Acceptance

1. With the monitor running, `ragon-admin audit egress` passes unchanged, and an strace of `ragon-node-monitor` over an hour shows connections only to loopback ports and, in Phase 4, to paired nodes over SSH; no DNS query.
2. `ragon-node-monitor`'s unit holds under 64 MiB (`MemoryMax`), and its 24-hour mean CPU is under 1 % of one core.
3. `ragon-admin monitor status` returns in under 2 s and imports nothing from `dreamference.hardware`.
4. Stopping the engine's container while the user has not run `server stop`: the model server is `down` within 60 s, the node is `down`, and (Phase 3, notify on) one page arrives; starting it again gives one recovery line.
5. With requests queued and the generation counter frozen for 60 s (a fixture), liveness is `down` with the reason "requests waiting, no tokens for 60 s".
6. The liveness completion is never sent while the gate holds for a run, while every resident session slot is taken, or within 10 minutes of the engine's last token; a test with each condition and a fake clock proves it.
7. A gate holding for a live SWE-bench run shows as `ok` ("holding"); one whose run's heartbeat went stale shows as `degraded`.
8. A unit that hits its start limit is restarted once, the action is journalled with its reason, and a second failure within six hours pages and restarts nothing.
9. The monitor never runs `docker pull`, `docker start`, `docker restart`, `reboot`, `systemctl reboot`, `sudo` or the power-cycle code; a test over its command table proves it.
10. `ragon-admin monitor notify signal` is refused with the setup command when Signal is not set up; `monitor notify off` is the default after install.
11. Under `/airgapped on`, a page for Telegram or Signal waits in the outbox and arrives with its age after the level returns to `off`; a page for Matrix arrives at once.
12. With `ragon web` stopped, the chosen bridge sends one "not answering" line after five minutes and one when it answers again.
13. An item crossing its threshold and back four times in 30 minutes produces one page and the note "flapping".
14. The database stays under 256 MiB with 30 days of samples at §3's intervals; raw samples older than 48 h are gone.
15. A certificate expiring in 6 days is `down` and paged; at 20 days it is in the digest only.
16. `ragon-remote-certificate-alert.service`, as written by the product, runs only programs the product installs.
17. On Windows (Phase 5), the same fixtures give the same states, and transitions appear in the Event Log under `Ragon`.

## 13. Decisions

*(For the owner.)*
