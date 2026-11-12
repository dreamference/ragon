# Ragon on macOS as a node

**Draft for review, 2026-11-12.** Nothing is built and nothing has run on a Mac node. Written from the owner's decision Q204 of 2026-11-12: the Mac node (Apple silicon, Metal) is part of the 1.0 gate, beside the Windows node.
**Target:** an Apple silicon Mac with 128 GB of unified memory (a Mac Studio with an M5 Max first); a 64 GB Mac mini M5 Pro is the Metal development box, not a node (MN5).
**Builds on:** [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md), whose structure this document mirrors and whose decisions WN1 to WN8 it reuses where they carry over; [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md); [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md); [RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md); the ragon-engine repository's `DREAMFERENCE_RAGON_ENGINE_METAL.md` (the engine port: kernels, exactness, memory plan, milestones M0 to M9) and the engine backlog item `eng-metal-backend`.

Every figure below carries a tag: **[E]** an estimate, **[?]** unknown. Nothing here is measured on a Mac yet; the engine's own figures are in its Metal specification and are not repeated.

---

## 0. Summary for the review

**The decision.** Q204: a Mac serves the default model through a native Metal build of `ragon-serve` and is a node in the full sense: advertised, paired, running jobs, Night Shift, sidecars, the forge and the web UI. 1.0 waits for it, as it waits for the Windows node.

**What makes the Mac easier than Windows.**

1. **It is Unix.** OpenSSH with `command=` in `authorized_keys`, POSIX signals, `tar`, `flock(2)` and a Unix web crate are already there. The forced command and model sync carry over nearly unchanged (§4.5).
2. **Bonjour is the operating system's.** mDNSResponder runs on every Mac; the node registers its service with it rather than running a second responder (§4.4).
3. **The client already ships for macOS**, and `ragon web` already writes a launchd agent there (`LAUNCHD_LABEL` in `ragon-rs/web/src/cli.rs`). The lifecycle has a precedent to follow (§4.2).

**What makes it harder.**

1. **No cgroups, no bubblewrap, no PSI.** A job's memory cap, the kill path during a load and the sandbox are each new on macOS (§4.3, §4.5).
2. **No strace.** The egress audit needs a different method on macOS, and a DNS lookup there is IPC to mDNSResponder, not a socket the process opens (§4.6).
3. **Unattended boot.** With FileVault on, a Mac that loses power waits at the unlock screen until someone types a password; and whether Metal works from a launchd daemon with nobody logged in is the first probe (§4.2).

**The calendar, stated plainly.** The engine's Metal specification puts its first release (M8) at about the end of March 2027 **[E]**, from code starting 2026-11-18. If 1.0 waits for the Mac node, it waits for that, unless the engine work is restaged or staffed differently (§9). The node track does not need the engine: a stand-in engine that answers `/v1/models` lets the node's own parts (§4, §6) be built and their checks passed first; the checks that need Metal wait for the engine (§9).

**Decisions wanted:** the table in §2, each with a default the work proceeds on.

---

## 1. Goals and non-goals

### 1.1 Goals

- A Mac installed with `--role node` does what a GB10 node does for its clients: serves the default model through `ragon-serve` (Metal), advertises itself, pairs, runs jobs over SSH, receives models, runs Night Shift, and offers search, speech, image search, the forge and the web UI.
- Unattended: it comes back after a restart with nobody signed in (with the FileVault caveat of MN3).
- Host safety at the GB10's level: a runaway load kills the engine, not the machine.
- The same client experience: a client neither knows nor cares that the node is a Mac.
- The same served model and the same model id: the Metal build multiplies the checkpoint's own NVFP4 and FP8 values (Metal spec §3.2); only the tile order differs.

### 1.2 Non-goals (owner to confirm, MN8)

- **An interim MLX or llama.cpp engine.** The owner decided on 2026-10-28 that the Mac's local model is the native Metal port of the engine only. The 2026-11-05 note that a Mac node could use "MLX or llama.cpp Metal" is superseded by that decision and by Q204. llama.cpp and MLX remain measuring baselines.
- **Intel Macs**, iPhone and iPad, the Neural Engine (Metal spec §1).
- **Docker on the node** for the model server: macOS containers have no GPU (Metal spec §6.4). Sidecars per MN2.
- **The SWE-bench harness on the Mac.** It stays on a GB10; the Mac node only serves, and a GB10-run night served by the Mac is the quality check (§8, G20). The model gate is not ported.
- **Hosting on the Mac node, at least for the first gate:** the remote-access control plane, the messenger bridges, the Google service, the fleet hub, LoRA adapters (served by ragon-serve on CUDA or SGLang only today), and `node provision`'s Linux root helper.
- **A node on a 64 GB Mac** (MN5), and on battery as a supported mode (MN6).

---

## 2. Decisions for the owner

Each row has a default; the work proceeds on it until the owner answers (owner rule of 2026-11-10).

| # | Question | Options | Default | Owner answer |
|---|---|---|---|---|
| **MN1** | What language is the node half on macOS? | (a) Python `ragon-admin` with macOS branches; macOS has no system Python (the `/usr/bin/python3` stub asks to install the Command Line Tools). (b) The Rust launcher subcommands decided for Windows (WN1 b): `ragon node`, `ragon server`, `ragon night run`, reading the registry's JSON export. | **(b), shared with Windows.** The Mac is the cheaper second consumer of the same Rust: its Unix parts (SSH, signals, `tar`, locks) are closer to Linux than to Windows. | |
| **MN2** | How do sidecars and the forge run? | (a) Docker Desktop or another Linux VM; (b) native per sidecar, as decided for Windows (WN2 c). | **(b) native**, for the same reasons: no VM memory layer beside the engine on unified memory, no licence question, no logged-in user needed. §4.7 says per part. | |
| **MN3** | FileVault on a node | (a) Required off for unattended power-cut recovery. (b) Left on: a power cut needs one unlock at the machine; planned restarts (`ragon update`, macOS updates) use `fdesetup authrestart`. | **(b).** Disk encryption stays the user's choice, as on a GB10; the installer reports the state and what it means; G16 records which case was tested. | |
| **MN4** | Who owns the engine process? | (a) A LaunchDaemon running as a dedicated hidden role account (`_ragon`), as Windows' dedicated service account. (b) A LaunchAgent of the owner's login session, with automatic login. | **(a)** if the first probe (§4.2) shows Metal reachable from a daemon with nobody logged in; **(b)** only as the fallback, and only with FileVault off, since automatic login is unavailable with FileVault on. | |
| **MN5** | Which Mac is the gate box? | The 128 GB Mac Studio M5 Max; the 64 GB Mac mini M5 Pro. | **The 128 GB Mac Studio**: the owner's node floor is 128 GB (2026-11-05). The 64 GB mini is the Metal development box and CI runner; the Metal spec's 64 GB recipe stays supported for users but is not the gate. | |
| **MN6** | A laptop as a node | Supported; preview; not supported. | **Preview.** A MacBook Pro with 128 GB on mains, lid open or with an external display, is the same node; on battery the node stops advertising `state=ready` and says why (§4.2). Closed-lid sleep is not prevented. | |
| **MN7** | Signing | Developer ID and hardened runtime for `ragon-serve`, the converter, the supervisor and the `.metallib` files; notarization. | **Signed and notarized at the 1.0 gate**, as the Metal spec §6.4 says. An unsigned daemon is blocked by Gatekeeper and prompts at the application firewall. | |
| **MN8** | What exactly must 1.0 have on the Mac? | G1 to G20 (§8) and the §1.2 list. | **G1 to G20 on the gate box**, the §1.2 non-goals as written. | |
| **MN9** | The engine's structure: mirrored files (Metal spec §1: "no device abstraction layer") or one backend interface for CUDA, HIP and Metal (the engine backlog's `eng-backend-interface`, from the owner's later decision)? | Either. | **The later decision governs the engine; this document does not depend on it.** The node needs only the seam of §5, which is the same under both. | |
| **MN10** | Local Network privacy (macOS 15 and later) | A daemon or tool that talks to LAN devices may need the user's approval [?]. | **Probe first** (§4.4); if a role-account daemon needs approval, the installer says where to grant it, once. | |

---

## 3. What this replaces

| Earlier decision or text | Now |
|---|---|
| 1.0 ships GB10-only; a Mac node is post-1.0 (owner, 2026-11-05) | **Superseded by Q204**: the Mac node is in the 1.0 gate. |
| A Mac node may use MLX or llama.cpp (2026-11-05 note) | **Superseded**: native Metal `ragon-serve` only (owner, 2026-10-28; Q204). |
| `install.sh` refuses `--role node` on macOS ("macOS cannot be a Ragon node") | To change with the node install (§6); the refusal stays until G1 passes. |
| `ragon web` on macOS is a per-user launchd agent | Stays for clients. A node runs it under the node's daemon (§4.2). |
| Metal spec §1: no device abstraction layer | Open against the later backend-interface decision (MN9); the node is indifferent. |
| Metal spec §4.7: an idle Mac sleeps | Holds for a client. A node on mains must answer SSH and mDNS while idle, so it does not system-sleep; only the display does (§4.2). |

---

## 4. The node's parts on macOS

### 4.1 Overview

| Part | GB10 | macOS, as proposed | State |
|---|---|---|---|
| Model server | SGLang or `ragon-serve` in Docker | `ragon-serve` (Metal), native, a child of the supervisor (§5) | Engine spec only |
| Lifecycle | `ragon-admin server start`, systemd | `ragon server start/stop/status`; a supervisor as a LaunchDaemon (§4.2) | To build |
| Model registry | Python `MATRIX` | The JSON export decided for Windows, with Metal recipes per memory size | To build |
| Host safety | `check_host_safety`, PSI watchdog | GPU working-set cap, memory-pressure level, swap and compressor, `SIGKILL` of the engine (§4.3) | Designed here |
| Identity | `~/.config/dreamference/node-id` | `/Library/Application Support/Ragon/node-id`, owned by the role account | To build |
| Advertisement | Avahi file, `_ragon-node._tcp` | Registered with mDNSResponder (`DNSServiceRegister`), same TXT keys (§4.4) | Browse works today, advertise never |
| Pairing | `NodePairing` over SSH | Same protocol, macOS's OpenSSH (Remote Login) | To verify |
| Jobs | Forced command, systemd user unit, memory cap, time limit, bubblewrap | Same forced command; supervisor-enforced memory cap and time limit; Seatbelt sandbox (§4.5) | To build |
| Model sync | `tar` over SSH | The same stream; plus the converted weights between Macs (§6) | To build |
| Night Shift | Python, systemd timer, `flock`, worktrees | `ragon night run` in Rust (shared with Windows); launchd `StartCalendarInterval`; same queue format | To build |
| Search, speech, image search | Containers | Native per MN2 (§4.7) | Open |
| Forge | Forgejo rootless container | Forgejo built for darwin-arm64 from the pinned tag (§4.7) | Open |
| Web UI | `ragon web --lan` on 3100 | The same crate, already built for macOS | Works on a client |
| Egress audit | strace | Seatbelt-confined run plus socket sampling (§4.6) | To build |
| Power | `node_power_settings` | `pmset`: no system sleep on mains, restart after power failure (§4.2) | To build |

### 4.2 Unattended lifecycle (launchd)

**Proposed (MN4 a):** one LaunchDaemon, `/Library/LaunchDaemons/ai.dreamference.ragon-node.plist`, label `ai.dreamference.ragon-node`, `UserName` the hidden role account `_ragon` (no login shell, no password, not an administrator), `RunAtLoad` and `KeepAlive` with a throttle. It runs `ragon` as a supervisor that holds the engine as a child process, the host-safety watchdog, the Bonjour registration, the tool-call canary and `ragon web --lan`. Night Shift is a second daemon with `StartCalendarInterval`, the twin of the systemd timer. State lives under `/Library/Application Support/Ragon` (weights, converted model, node id, queue) with permissions that let the job user read what it must.

**The first host probe [?]:** does a process started by launchd at boot, in the system domain, with nobody logged in, get a Metal device, allocate up to the working-set cap and run a compute pipeline? (Compute needs no window server, but this is unmeasured.) It decides MN4 and is the Mac twin of Windows' session-0 probe. A second probe: is the GPU's memory returned to the system when the engine is killed with work in flight?

**Power** (`pmset`, set once at install, restored at uninstall):

- On mains: `sleep 0` (no system sleep), display sleep allowed, `autorestart 1` (start after a power failure, on desktops that support it), `womp 1` (wake for network access).
- The engine holds no power assertion of its own on a node; the setting above replaces the Metal spec's per-request assertion, which stays for clients.
- On battery (laptops, MN6): the supervisor reports `state=battery` in its status and stops admitting new sessions below a charge threshold [E]; it does not fight sleep.

**FileVault (MN3).** A Mac with FileVault on boots to the unlock screen and starts no daemon until unlocked. `ragon update` and other planned restarts use `fdesetup authrestart` when the user allows it; an unplanned power cut needs one unlock at the machine, and the installer says so.

### 4.3 Host safety on unified memory

The rules of the GB10 carry over in meaning (`docs/dev/host-safety.md`, Metal spec §4.8): refuse an unsafe load before it starts, kill the engine and nothing else when memory runs out during one, and never let a model load freeze the host.

**Before a load** (the twin of `check_host_safety()`, in the supervisor):

| Check | Source | Refuse when |
|---|---|---|
| The engine's plan fits | `ragon-serve`'s own plan, exit 3 (Metal spec §4.8) | the plan says no |
| GPU working-set cap | Metal's `recommendedMaxWorkingSetSize` | the recipe needs more than the cap minus the reserve |
| The cap was not raised past the reserve | `iogpu.wired_limit_mb` sysctl | it leaves macOS less than the recipe's reserve; Ragon never raises it unasked |
| Memory pressure now | `kern.memorystatus_vm_pressure_level` | warning or critical |
| Swap and compressor | `vm.swapusage`, `host_statistics64` | swap in use above a threshold [E] before the load |
| Thermal state | `NSProcessInfo.thermalState` | critical |
| Another heavy GPU user | the device's current allocation | it leaves too little for the plan |

**During a load and while serving** (the twin of the PSI watchdog): the supervisor subscribes to memory-pressure events (`DISPATCH_SOURCE_TYPE_MEMORYPRESSURE`) and samples swap and the compressor once a second. Trip: pressure critical for 3 s [E], or swap growing by more than a threshold [E] during the load; then `SIGKILL` to the engine's process group by pid. There is no container and no daemon in the kill path, so the kill is one system call from a process that is already running; the supervisor is small and allocates nothing after start, so it is the process most likely to be scheduled under pressure. The engine's own admission answers 503 `host_memory_low` at the warning level before the supervisor ever trips.

**Different from Linux:** macOS compresses and swaps rather than livelocking in reclaim, and jetsam kills processes by priority under pressure. The risk is therefore less a frozen host than a host that swaps the user's applications out while the engine keeps its wired GPU memory. The trip thresholds are set from G15's measurements, never assumed.

**Sidecars** are bounded by the same supervisor (§4.5's memory cap) and reserved in the plan, the twin of `side_service_allowance()`.

### 4.4 Bonjour advertisement

The node registers `_ragon-node._tcp` with the system's mDNSResponder through `DNSServiceRegister` (in libSystem; no extra library), with exactly the TXT keys of `NodeServiceFile` (`proto`, `node`, `version`, `web`, `search`, `state`, `main`, `remote`) and, when the forge is on, the second service `_ragon-forge._tcp`. The instance name is the computer's local host name, as `%h` is on Linux. `server start` and `stop` update `state` with `DNSServiceUpdateRecord`; nothing is written to disk. A test asserts that the TXT keys equal `NodeServiceFile`'s, so the two platforms cannot drift.

**Why not the `mdns-sd` responder:** the client's browse already uses `mdns-sd` on Linux, macOS and Windows (`ragon-rs/src/node.rs`), and stays. Advertising is different: a second responder beside mDNSResponder on UDP 5353 duplicates answers and conflicts on names. The system's responder also handles sleep proxy and interface changes.

**The firewall.** macOS's application firewall allows a signed binary's incoming connections by rule (`socketfilterfw --add`, `--unblockapp`), set once at install for `ragon-serve`, `ragon` (web on 3100) and the forge; an unsigned binary would prompt (MN7). On a network the user marks as public, the node advertises nothing and says why, as on Windows.

**Local Network privacy (MN10) [?].** macOS 15 asks the user before a process reaches devices on the LAN. Whether that applies to a launchd daemon running as a role account, and to the client's browse from Terminal, is the third probe, run on the same development box as §4.2's two.

### 4.5 The node install role, pairing and the forced command

**Remote Login.** Pairing and jobs need macOS's OpenSSH server. Turning it on from a script needs Full Disk Access (`systemsetup -setremotelogin`), so the installer checks it and, if off, tells the user to switch on Remote Login in System Settings; it never asks for Full Disk Access for itself.

**The forced command** is the same as on Linux: `node add` authorises the pairing key with `command="ragon node serve-job",restrict` in the job user's `authorized_keys`, and `serve-job` refuses anything but its protocol. macOS's sshd honours `command=` and `restrict` as Linux's does [?: verify on the shipped OpenSSH version]. Key-only login for that user goes in `/etc/ssh/sshd_config.d/` (included by macOS's `sshd_config`), checked with `sshd -t` first. The job user is not an administrator.

**Jobs.** No systemd user unit, no cgroup, no bubblewrap. The design:

- **Supervision:** `serve-job` hands the job to the node supervisor over a local socket readable only by the job user; the supervisor spawns it in its own process group, so a dropped SSH session does not kill it (G9).
- **Memory cap:** macOS ignores `RLIMIT_AS` and `RLIMIT_RSS` for this purpose. The supervisor samples each job's process tree's physical footprint (`proc_pid_rusage`, `ri_phys_footprint`) once a second and kills the group over the cap. A one-second sample can overshoot; the cap is set below the plan's reserve by that margin [E].
- **Time limit:** enforced by the supervisor, `SIGTERM` then `SIGKILL`.
- **Sandbox:** a Seatbelt profile, as Codex's own macOS sandbox uses, generated per job: the job's worktree writable; the home, `/Library/Application Support/Ragon` and the SSH keys hidden; no GPU (IOKit user clients of the GPU denied); network per `/airgapped` (none at `on`, so the egress rule holds for jobs).
- **Git:** jobs and the forge need git. macOS's `/usr/bin/git` is a stub that asks to install the Command Line Tools; the installer checks `xcode-select -p` and refuses with the command to run, rather than letting an unattended job pop a dialog.

### 4.6 The egress audit on macOS

There is no strace, and DTrace needs System Integrity Protection relaxed, which a product never asks for. The audit (`ragon audit egress`, the Rust twin of the Windows audit) runs one real `ragon exec` against the node's own engine, plus `ragon-serve`, the supervisor and the sidecars, by three methods; the first decides, the other two must not contradict it:

1. **Confined run (the verdict).** Each process runs under a Seatbelt profile that allows outbound network only to the allowlisted loopback ports (the model server, search 8888, image search 8768) and denies everything else, with denials logged. Every denial in the unified log (`com.apple.sandbox`) during the run fails it. **DNS:** on macOS a name lookup is IPC to mDNSResponder over `/var/run/mDNSResponder`, not a socket to port 53, so a socket trace would never see it; the profile denies that path, and any denial there counts as a query that would have left the machine.
2. **Observed run (corroboration).** The same session unconfined, with every process in the tree's sockets sampled once a second (libproc, `proc_pidfdinfo`). Any socket that is not listening, loopback to an allowlisted port, or an accepted loopback connection fails it. Sampling misses short connections, which is why it does not decide alone.
3. **Static check** for the engine's binaries, as Metal spec §6.5 item 1.

Exit 0, 1 or 2 as on Linux. **Outside the verdict, and said in the report:** macOS's own traffic (mDNSResponder's multicast, software update, Gatekeeper's notarization check on a binary's first launch). An unattended caller names its node in `DREAMFERENCE_VLLM_HOST`, as on Linux, so no browse runs during the audit.

At `/airgapped on`, a sandboxed command must reach neither mDNSResponder's socket nor a host control socket (the Docker socket, if one exists, and `launchd`'s): the macOS twin of patch 0019's `host_sockets_at`, tested by the audit and a sandbox test.

### 4.7 Sidecars and the forge (MN2 b)

| Part | Native route on macOS | Verdict |
|---|---|---|
| Speech | `faster-whisper` on CPU, or `whisper.cpp` (which has its own Metal path; CPU first, so it never competes with the engine for the GPU), behind the same `/api/transcribe` | Feasible |
| Image search | SigLIP on CPU through ONNX Runtime, behind the same service interface | Feasible |
| SearXNG | A Python application: a pinned SearXNG in its own managed Python environment, owned by the role account. "No Python" covers Ragon's node half (MN1), not a third-party sidecar | Feasible; upstream documents no macOS install, so ours is the supported one |
| Forgejo | No darwin assets on the release page; Go builds the pinned tag for darwin-arm64 with SQLite directly | Feasible; build and own the result, check upstream's tests on macOS |

Each runs as a LaunchDaemon under the role account, memory-capped by the supervisor and reserved in the plan.

---

## 5. The Metal backend seam

The engine is the ragon-engine repository's business (Metal spec; backlog `eng-metal-backend`, after `eng-backend-interface`). The node depends on it only at a process boundary, which holds whichever structure MN9 settles on:

| Seam | What the node relies on |
|---|---|
| The asset | `ragon-engine-macos-arm64-<version>.tar.gz` (binaries, `.metallib` files, licence), signed, notarized and pinned in `SHA256SUMS` (Metal spec §6.4) |
| The weights | `ragon-convert` turns the cached Hugging Face checkpoint into the Metal tile order once, on the Mac, deterministically; its output's SHA-256 is pinned in the model registry; `sync-model` between two Macs may copy the converted blob instead |
| The recipe | Chosen by the registry from the Mac's memory size (Metal spec §4.2's table): `--sessions`, `--max-context`, prefill chunk, vision on or off, the sparse-head directory as on Linux |
| Start | The supervisor runs `ragon-serve` natively; exit 3 means the plan does not fit and is reported, not retried |
| The API | The same HTTP surface and the same model id as the CUDA build (`/v1/models`, `/health`, `/metrics`, `/v1/responses`, …), so the launcher, the canary and the clients need no Mac branch |
| Pressure | 503 `host_memory_low` while memory is short; `/metrics` adds the thermal, battery and working-set gauges |
| Exactness | The engine's own gates (Metal spec §2, L3 against CUDA); the node gate only checks they were run on the release candidate |

A stand-in engine with the same CLI and `/v1/models` lets §4 and §6 be built and gated before M1.

---

## 6. Install and update

`install.sh --role node` on macOS (today it refuses, `install.sh` lines 199–200). Without `--role`, nothing changes for a client.

1. **Check first, change nothing:** Apple silicon, the macOS version the engine's release names, memory (128 GB for the gate, MN5; the Metal spec's smaller recipes warn), the GPU working-set cap, FileVault state, Remote Login, the Command Line Tools, power source (desktop or laptop), cable or Wi-Fi.
2. **Client steps as today.**
3. **Node assets:** the engine tarball, SHA-256 against `SHA256SUMS`, unpacked under `/usr/local/lib/ragon` (root-owned, not user-writable).
4. **One `sudo`**, for: the role account `_ragon` and the job user (a second hidden, non-administrator account that owns job worktrees and the forced-command key); the LaunchDaemons; `/Library/Application Support/Ragon` and its permissions; firewall rules; `pmset`; the `sshd_config.d` file for the job user. Over SSH, `sudo` asks once on the terminal; the script says so up front.
5. **The model:** download from Hugging Face then `ragon-convert`, or `sync-model` from another node (another Mac sends the converted blob; a GB10 sends the checkpoint and the Mac converts).
6. **The egress audit** once, printing the verdict.

**Update.** `ragon update` on a node refuses while a job is in flight (or waits with a limit), stops the daemons with `launchctl bootout`, swaps binaries and the engine, starts them with `launchctl bootstrap`, checks health with a canary completion, and rolls back to the kept previous version on failure. A macOS update stays the user's; the supervisor checks the minimum version at start.

**Uninstall.** `ragon uninstall --node` removes the daemons, the role account, the firewall rules and the `pmset` changes, and keeps weights and the forge's data unless asked twice.

---

## 7. Security assumptions

- **The LAN is trusted**, as for GB10 nodes; on a network marked public the node advertises nothing.
- **The forced-command user is not an administrator**, and neither is the role account. Neither has a password.
- **Jobs are contained** by the supervisor's memory cap and time limit and a Seatbelt profile; they get no GPU access, so a job cannot starve or crash the engine's device.
- **Integrity.** Engine binaries, `.metallib` files and the converted weights' hash are pinned and installed where users cannot write; Gatekeeper and notarization add a second check.
- **Egress.** §4.6; macOS's own traffic is outside the verdict and the report says so.
- **Disk encryption.** MN3. The docs state what is at rest: the forge's repositories, job worktrees, session records.

---

## 8. The acceptance gate

"A Mac node works" means all of these pass in one run, on a fresh install, from `scripts/mac_node_gate.sh` and a client-side twin, with a report. The engine thresholds are the Metal spec's milestone targets for the gate box's memory size; they may tighten, never silently loosen.

| # | Check | Pass |
|---|---|---|
| G1 | Fresh macOS, `install.sh --role node`, then a restart with nobody logged in | Daemon up; health ok within the limit (FileVault per MN3, recorded) |
| G2 | A Linux, a Windows and a macOS client on the LAN, no configuration | All find the node over mDNS |
| G3 | Pair from a client and from a GB10 | Pairing succeeds; a wrong key is refused |
| G4 | Decode, agentlong | The Metal spec's M3 target; speculative equals non-speculative; two runs byte-identical |
| G5 | Prefill | The Metal spec's M4 target |
| G6 | L3 against the GB10 | Metal spec §2 |
| G7 | A Work session from a client, a long turn, tools | Completes within the limit set after M3 |
| G8 | Images in, structured output, batches of two | The engine's server tests pass |
| G9 | Job over the forced command: run, status, fetch, cancel, disconnect mid-run | Survives the disconnect; memory cap and time limit trip; no shell reachable; no GPU |
| G10 | `sync-model` from a GB10 and from another Mac, over cable | Hash equal; served after restart |
| G11 | Night Shift: `/night add --on <node>`, three attempts, chooser, report | Commit in a worktree; nothing merged or pushed |
| G12 | Search, voice, image search | All answer from a client |
| G13 | The forge | Push over SSH, `/forge/` through `ragon web`, second mDNS service seen, backup runs |
| G14 | `ragon web --lan` from a phone and the desktop app | The policy's conformance cases pass |
| G15 | Memory: a load past the cap, then KV past budget, with the sidecars running | Engine killed, desktop responsive, nothing else killed, memory back, restart succeeds |
| G16 | Kill and power | Kill the engine at work: restart within the limit. Cut power: back unattended (FileVault off) or after one unlock (on) |
| G17 | `ragon audit egress` on the node with everything running | Exit 0; at `/airgapped on`, no way out through mDNSResponder or a host socket |
| G18 | `ragon update` over a running node, then a forced failure | Up to date; rolled back on failure |
| G19 | 8-hour soak, 60-minute thermal | No restart; step-time drift and thermal state reported |
| G20 | A SWE-bench night served by the Mac and run by a GB10 | No significant loss (McNemar), the Metal spec's M7 |

---

## 9. What 1.0 requires

G1 to G20 on the gate box; user and developer docs; the release assets; MN1 to MN10 settled.

| Work | Weeks **[E]** | Source |
|---|---|---|
| Engine M0–M8 | about 18 | Metal spec §7 |
| Node tooling on macOS (lifecycle, advertisement, host safety, jobs), on top of the Windows Rust node half | 3–4 | This draft; less than Windows' 4–5 because the Unix parts carry over |
| Night Shift on macOS | under 1, once the Rust runner exists for Windows | This draft |
| Sidecars, forge, web | 2–3 | This draft; the weakest number here |
| Egress audit on macOS | 1 | This draft |
| Installer, signing, notarization, docs, gate | 2 | This draft |

**The engine is the long pole.** From 2026-11-18, M8 lands around the end of March 2027 **[E]**. The owner's choices: (S1) 1.0 waits for M8; (S2) a second kernel engineer, or restaged milestones (M5's server before M3's pipeline, accepting a slower first decode), to bring M8 forward; (S3) the Mac node ships as a 1.0 preview behind `--role node` once G1–G3, G9–G14 and G16–G18 pass with a stand-in engine, and the 1.0 cut then waits for the checks that need the Metal engine (G4–G8, G15, G19, G20). **Default: S3's staging**, so the node track never waits for the engine, while the 1.0 cut still waits for the whole gate as Q204 says.

---

## 10. Work list

To be added to the backlog at review, node items first (they need no engine): the two host probes of §4.2 and the Local Network probe of §4.4 on the development box; the registry's JSON export with Metal recipes; the Rust supervisor's macOS lifecycle and `pmset`; Bonjour registration with the TXT-key test; the forced command and the job supervisor on macOS; the macOS egress audit; native sidecars and the forge; `install.sh --role node` on macOS; the gate scripts.

---

## 11. Sources

Local: the ragon-engine repository's Metal specification (§1, §2, §3.2, §4.2, §4.7, §4.8, §6.4, §6.5, §7, §9); WINDOWS_NODE §2, §4, §6, §7, §9, §10; RAGON_NODE; `docs/dev/node.md`, `docs/dev/host-safety.md`, `docs/dev/egress-audit.md`; `NodeServiceFile` (`dreamference/node/node_service_file.py`); `install.sh`; `ragon-rs/web/src/cli.rs` (`launchd_plist`); `ragon-rs/src/node.rs` (the browse).

Apple's documentation, to be read and cited at review: launchd's property-list keys (`UserName`, `KeepAlive`, `StartCalendarInterval`); `pmset`; `fdesetup authrestart`; `DNSServiceRegister`; the application firewall's `socketfilterfw`; Metal's `recommendedMaxWorkingSetSize`; the memory-pressure dispatch source; Seatbelt profiles as Codex uses them; Local Network privacy.
