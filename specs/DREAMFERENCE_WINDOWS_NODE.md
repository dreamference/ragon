# Ragon on Windows as a node

**Draft for review, 2026-11-06.** Nothing is built. The owner answered WN1 to WN8 on 2026-11-06; the answers are folded into §2 and the rest of the text may still show the earlier recommendations until the review.
**Target:** AMD Strix Halo (Ryzen AI Max+ 395, Radeon 8060S `gfx1151`, 128 GB LPDDR5X-8000, Windows 11 Pro) first; NVIDIA Windows PCs second.
**Builds on:** the AMD study (`DREAMFERENCE_RAGON_ENGINE_AMD.md`, on branch `specs/amd-port` of the engine's old local clone, not yet in the ragon-engine repository; backlog `win-engine-carry-study` carries it); [RAGON_AMD_NODES](./DREAMFERENCE_RAGON_AMD_NODES.md); [RAGON_WINDOWS_ARM](./DREAMFERENCE_RAGON_WINDOWS_ARM.md) §8–§10, §13, §14, §16, §23; [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md); [RAGON_FORGEJO](./DREAMFERENCE_RAGON_FORGEJO.md); the ragon-engine repository's `DREAMFERENCE_RAGON_ENGINE_METAL.md` and `DREAMFERENCE_RAGON_ENGINE_LOWBIT.md`; the survey [windows-node](./research/windows-node.md).

Every figure below carries a tag: **[M]** measured on the AMD box, **[E]** an estimate, **[?]** unknown. Week counts are engineer-weeks and are estimates.

---

## 0. Summary for the review

**The owner's decisions of 2026-11-06.** Ragon fully supports Windows as a node. AMD first, NVIDIA second. The model server is a port of `ragon-serve`, not llama.cpp. On AMD any quantisation that is fastest there is acceptable. Scope is everything a GB10 node has. 1.0 waits for it; 0.9.x cuts continue twice a week. A second Strix Halo box is being bought for engine work.

**Five things this draft found that change the plan.**

1. **The engine's feasibility is already half answered.** The AMD study passed its first milestone: the served NVFP4 checkpoint streams close to the box's read bandwidth at 16 rows **[M]**. The spike is therefore smaller than it looked, and starts from the unmeasured parts (§5.3).
2. **The premise on format is half right.** Strix Halo has no FP4 matrix unit, so the block-scaled prefill path cannot be reused. But the GB10's *decode* path never used FP4 tensor cores: `stream_gemm` decodes NVFP4 and FP8 to FP16 in registers and runs FP16 `mma.sync`. The same trick works on WMMA. The result: **"the fastest quantisation" reduces to "the fewest bytes that pass the quality ladder"**, which is the GB10's own question, and the port need not fork the served model. The only format choice specific to AMD is prefill's (§5.2).
3. **Docker Desktop fits a node badly.** It needs WSL2 or Hyper-V (WSL, Virtual Machine Platform and Hyper-V are all off on the box **[M]**), needs a paid subscription above 250 employees or $10M revenue, and by public reports does not run without a logged-in user. A node runs unattended; the box has no console session **[M]**. It stays first in the measuring order, as the brief asks, but is not this draft's recommendation for the product (§4.3, WN2).
4. **Native equivalents are thinner than hoped.** Forgejo's release page lists Linux assets only (amd64, arm-6, arm64, for 16.0.7 and 17.0.1). SearXNG's documentation lists no Windows install. So the recommendation is a small Ragon-owned Linux distribution in WSL2 for sidecars and forge, with the engine native. This touches the owner's "no WSL" decision and needs a decision (WN2).
5. **The largest open question is not Docker but Python.** `ragon-admin` does not exist on Windows. The node half, Night Shift and the sidecar and forge management are Python (the node package alone is 9,128 lines). "No Python" was decided for the client. A node needs its own answer (WN1).

**The first measurable milestone** is the end of the spike (§5.4): every GEMM shape of the served 27B at 1 and 16 rows on the current driver, at least 200 GB/s and M16/M1 at most 1.15, one hipified engine kernel bit-identical across two runs, and a table per format and prefill candidate, all recorded. That closes in a decision point: port viable or not, and which format.

**The size, as estimates.** Engine 15–21 weeks by the study, plus 2–4 for what the engine gained since (§5.5). Node tooling, Night Shift and gate 7–8. Sidecars, forge and web 3–4 on the recommended route, 7–11 native. With the spike (1–2), about 28–39 engineer-weeks in all (§10). The AMD study's kernels compete with the Metal port for the same specialist from 2026-11-18 (WN7).

**Decisions wanted tomorrow:** the table in §2.

---

## 1. Goals and non-goals

### 1.1 Goals

- A Windows machine with `-Role node` does what a GB10 node does for its clients: serves the default model through `ragon-serve`, advertises itself, pairs, runs jobs over SSH, receives models, runs Night Shift, and offers search, speech, image search, the forge and the web UI.
- Unattended: it comes back after a reboot or a power cut with nobody signed in.
- Host safety at the GB10's level: a runaway load kills the engine, not the machine.
- The same client experience: a client neither knows nor cares that the node is Windows.
- One served model across platforms, unless a quality gate says otherwise (§5.2).

### 1.2 Non-goals (owner to confirm the list, WN6)

- A Linux node on AMD. "Later, if ever" stands, and no dual boot.
- An interim llama.cpp engine. llama.cpp remains a measuring baseline only.
- The SWE-bench harness on Windows. It stays on a GB10 and the Windows node only serves (G20). The model gate exists for SWE-bench nights on a node, so it is not ported either.
- Hosting on the Windows node, at least for the first gate: the remote-access control plane (NetBird), the messenger bridges, the Google service, the fleet hub, serving LoRA adapters (SGLang only today), and `node provision`'s Linux root helper.
- Vulkan or any second GPU backend.
- `Qwen3.8-Flash-Next` as the AMD model. AMD_NODES D7 stays open and nothing here depends on it. Measured on the box, Flash-Next at 83.6 GB left Windows 4.3 GB free and does not coexist with a runner job **[M]**.

---

## 2. Decisions for the owner

| # | Question | Options | Recommendation | Owner answer |
|---|---|---|---|---|
| **WN1** | What language is the node half on Windows? | (a) Ship an embedded Python and add Windows branches to the Linux-bound node, forge and Night Shift code. (b) Rust subcommands of the launcher (`ragon node`, `ragon server`, `ragon night run`), reading a JSON export of the model registry, as AMD_NODES §6.3 sketched. (c) All of it in a Linux VM, which is the excluded W2 route. | **(b) for what must be native**: lifecycle, advertisement, pairing, jobs, Night Shift, host safety, with about 4,200 lines of Python as the model and `node.rs`, `node_command.rs` and `night.rs` (3,219 lines of Rust) as the base. Anything in the Linux distribution of WN2 keeps today's Python unchanged.  | **Owner, 2026-11-06:** Decided: (b). Rust launcher subcommands. |
| **WN2** | How do sidecars and the forge run? | (a) Docker Desktop, the brief's first choice. (b) Docker Engine inside a Ragon-owned WSL2 distribution, `ragon-admin` unchanged inside it, engine native. No licence question. (c) Native per sidecar. | **Measure (a) first, ship (b), keep (c) as per-sidecar fallbacks.** This overrides the "no WSL" decision, whose reasons were speed through paravirtualised GPU and two memory layers for the *engine*. Under (b) the engine never enters WSL. **Needs the owner's explicit yes.**  | **Owner, 2026-11-06:** Decided: (c). Native per sidecar; no WSL, no Docker Desktop. |
| **WN3** | May the shared box have WSL or Docker Desktop enabled before the second box arrives? | Enabling either needs a reboot and changes the machine the release runner uses. | Wait for the second box if it arrives within about three weeks; otherwise once, straight after a release cut, with a written rollback.  | **Owner, 2026-11-06:** Decided: no reboot is needed; TheRock is already on the box. |
| **WN4** | Format for AMD | The same NVFP4 checkpoint decoded in shader; or other formats by the micro-benchmark | **Same checkpoint for decode**; prefill format at the spike's decision point (§5.2). Move to the all-NVFP4 projections when the GB10 does, or earlier on AMD alone only if LOWBIT §7's ladder passes.  | **Owner, 2026-11-06:** Decided: the same NVFP4 checkpoint for decode. |
| **WN5** | Carve-out on a node: 96 GB (D3) or 64 GB? | 96 leaves Windows 31.6 GB; 64 leaves about 64 GB. | **Measure, expect 64 for the 27B.** The engine's budget is about 57 GB **[E]**. 96 exists for Flash-Next. Windows, the distribution and any CI build then share 64 GB and not 32.  | **Owner, 2026-11-06:** Decided: measure; expect 64 GB. |
| **WN6** | What exactly must 1.0 have? | NVIDIA too, or AMD only; the §1.2 list | **AMD's gate (§9) required; NVIDIA follows in a 1.x** unless a machine and the §12 probes are in hand by the time AMD's W1 gate passes.  | **Owner, 2026-11-06:** Decided: the AMD gate is required for 1.0; NVIDIA follows in a 1.x. |
| **WN7** | Who gets the kernel work first: Windows or Metal? | Metal starts 2026-11-18 and runs about 18 weeks. | **AMD spike now, either way** (it costs days). Then choose. If 1.0 waits for Windows and not for the Mac, Windows goes first or Metal gets a second person. §10 gives the scenarios.  | **Owner, 2026-11-06:** Decided: Windows and Metal in parallel; Metal on the M5 Pro mini from its arrival. |
| **WN8** | Signing | A code-signing certificate for `ragon-serve.exe`, the supervisor and the code object carriers | Buy before the gate: Smart App Control is off on our box (D2) but on for many customers, and it blocks unsigned binaries.  | **Owner, 2026-11-06:** Decided: signing at the 1.0 gate. |


Two further owner answers (2026-11-06): the install takes **one UAC elevation**, and the model server runs as a **dedicated local service account**.

---

## 3. What this replaces

| Earlier decision | Now |
|---|---|
| The AMD port waits for 2.0 | **Superseded**: 1.0 waits for it. [DREAMFERENCE_2_0](./DREAMFERENCE_2_0.md) §3 moves out, with what it says on dependencies. |
| A Windows node is inference only, "sidecars: none" (AMD_NODES §6.3) | **Superseded**: the full node. |
| No Python on Windows | Stands for the client. Open for the node (WN1). |
| No WSL2 | Stands for the engine. Open for sidecars and forge (WN2). |
| Windows only, no Linux phase; no interim llama.cpp | Stand. |
| Carve-out 96 GB (D3); Smart App Control off on the box (D2) | Stand on the box until WN5 and WN8 are decided. |
| Windows client bugs W1–W10 as a dependency (2_0 §3) | Nearly gone: W2 fixed by patch 0033, W4–W10 in 1.7.2. Left: W1 (hooks on Windows), W11 (several users' sandbox setup). W11 matters here: a forced-command user and the owner are two users (§4.2). |

---

## 4. The node's parts on Windows

### 4.1 Overview

| Part | GB10 | Windows, as proposed | State |
|---|---|---|---|---|
| Model server | SGLang or `ragon-serve` in Docker | `ragon-serve.exe`, native, a child in a Job Object (§5) | Probes only |
| Lifecycle | `ragon-admin server start`, systemd | `ragon server start/stop/status`; a supervisor as a Windows service that survives logoff (§4.2) | To build |
| Model registry | Python `MATRIX` | JSON exported at release, read by Rust | To build |
| Host safety | `check_host_safety`, PSI watchdog | DXGI budget, `GlobalMemoryStatusEx`, low-memory notification, `TerminateJobObject` (RAGON_WINDOWS_ARM §9) | Designed |
| Identity | `~/.config/dreamference/node-id` | `%ProgramData%\Ragon\node-id`, ACLs | To build |
| Advertisement | Avahi file, `_ragon-node._tcp` | `mdns-sd` responder in the supervisor, or `DnsServiceRegister`; firewall rules | Browse tested, advertise never |
| Pairing | `NodePairing` over SSH | Same protocol, Win32-OpenSSH | To verify |
| Jobs | Forced command, systemd user unit, memory cap, time limit, bubblewrap | Forced command `ragon node serve-job`; Job Object, memory cap, time limit, Windows sandbox | To build |
| Model sync | tar over SSH | The same tar stream received by Rust (`tar.exe` ships with Windows) | To build |
| Night Shift | Python, systemd timer, `flock`, worktrees | `ragon night run` in Rust; Task Scheduler; same queue format (RAGON_WINDOWS_ARM §13) | Designed |
| Search | SearXNG container | §4.3 | Open (WN2) |
| Speech | speaches CPU container, `faster-whisper-small` | §4.3 | Open (WN2) |
| Image search | Sidecar plus `infinity` on CPU, SigLIP | §4.3 | Open (WN2) |
| Forge | Forgejo rootless container, 1 GiB cap, timers, second Avahi service | §4.3 | Open (WN2) |
| Web UI | `ragon web --lan` on 3100 | The same crate, ported: Unix-only code appears in nine files under `ragon-rs/web/src` | To port |
| Tool-call canary | Python | Rust, in the supervisor | To build |
| Egress audit | strace | `ragon audit egress` (ETW, built) extended to the engine's tree, the supervisor, and the distribution by the existing strace audit, merged (RAGON_WINDOWS_ARM §14) | Partly built |
| Power | `node_power_settings` | `powercfg`: no sleep on mains; Modern Standby is on this box **[M]** | To build |

### 4.2 Unattended lifecycle

Facts from the trial, all **[M]**: a child started by `Start-Process` dies with the SSH session; a scheduled task did not run under SSH; `Win32_Process` `Create` survives. All of the AMD study's measurements ran over SSH with no console session, so a process in session 0 reaches the GPU.

**Unknown [?]:** a process started *at boot, with no logon* under a service account. Does HIP see the GPU? Does the driver's budget match? It is the first host probe of the spike, because it decides the design.

**Proposed:** one Windows service, `RagonNode`, running `ragon` as a supervisor under a dedicated non-administrator account. It holds the engine's Job Object, the watchdog, the advertisement and the canary. State lives under `%ProgramData%\Ragon`, with ACLs that let the job account and the sandbox accounts read what they must. If the probe says no GPU from a service, the fallback is a startup task with stored credentials or S4U, with the same layout.

The sandbox over SSH works since patch `0033`. W11 (several users' elevated sandbox setup breaks each other, `sandbox_users.rs`) must be fixed for the job account: the owner and the forced-command user are different users on one machine.

### 4.3 Sidecars and the forge

The containers are Linux images. On Windows that means a Linux kernel, whichever product carries it.

| Route | What it costs | What it gives | Evidence |
|---|---|---|---|---|
| (a) Docker Desktop | WSL2 or Hyper-V on; subscription above 250 employees or $10M (each customer's own concern, and we cannot silently install it); **public reports that it does not start without a signed-in user** | Same images; little to write | Docker's install page; Docker forum threads. Not yet measured by us |
| (b) Docker Engine in a Ragon WSL2 distribution | WSL2 on, one reboot; a distribution of 3–5 GB plus images of about 15 GB (RAGON_WINDOWS_ARM §8.3); a memory cap in `.wslconfig`; `networkingMode = mirrored` | Same images, **and `ragon-admin images`, `voice`, `forge` and their systemd timers run unchanged inside**; no licence; no second implementation of the forge's 17 Python files | Designed as W2; not measured. Boot with no logon **[?]** |
| (c) Native | See below; several unsupported builds to maintain | No VM, no second memory layer | See below |

Native, per part:

| Part | Native route | Verdict |
|---|---|---|
| Speech | CPU-only here too: `faster-whisper` on Windows, or `whisper.cpp`, behind the same `/api/transcribe` | Feasible; to measure |
| Image search | SigLIP on CPU through ONNX Runtime; the small service rewritten or Python | Feasible, moderate work |
| SearXNG | The project documents no Windows install; a third-party native toolkit exists | Unsupported by upstream; poor fit for a supported product |
| Forgejo | No Windows assets on the release page. Build the pinned tag from source with SQLite and own the result; or Gitea, a different product and a migration | Costly; not exercised by upstream's CI, to verify |

**Fallback that needs no code on Windows:** a client's search, speech and image search may be served by another node on the LAN. It does not meet "everything a GB10 node has" on a single-box site, so it is a fallback, not a plan.

**Recommendation:** WN2 (b). The order of work follows the brief: backlog item 24 measures Docker Desktop, item 25 the distribution; the same probes, the same table; the owner sees both before anything is built on either.

---

## 5. The engine port

### 5.1 Strategy

The study's route, HIP on TheRock 10.0.0, chosen over Vulkan (20–30 weeks, ranked last), a Linux phase (not pursued) and gufo or llama.cpp (excluded). The structure follows METAL: **no device abstraction layer**, a build compiles one engine or the other, host files are shared, GPU files are mirrored. Shared seams with the Metal work: mapping (`safetensors.cpp:29`), memory admission (`admission.hpp:72-80`), the HTTP front. Those are designed once, by whichever port reaches them first.

Reuse **[E]**, from the study's inventory at c5ec4ec: device code 7,051 lines, of which about 4,600 rewritten (`stream_gemm.cu`, `attention_body.cuh`, `prefill_gemm.cu`, `prefill_gdn.cu`, `gemm_bf16.cu`, the MMA part of `sparse_head.cu`) and 2,400 through hipify (`kernels.cu`, `draft_kernels.cu`, `model.cu`, `topk.cu`). Host code (tokenizer, accept, lookup, tree, sampling, prefix: about 2,600 lines) is reused. `engine.cpp`, `speculate.cpp` and `drafter.cpp` go through hipify; green contexts are dropped; CUDA graphs become `hipGraph`; PDL has no HIP equivalent. proxygen and folly are replaced by a small server over `ragon_api`, ideally the same one Metal ends up with.

What the box has for it: TheRock 10.0.0 under `C:\TheRock\build` (`hipcc`, `hipInfo`, `hipify-clang`), Visual Studio 2022 BuildTools with MSVC 14.44 and clang-cl, Windows SDK 10.0.26100 **[M]**, more than the study recorded. vcpkg and a standalone CMake are absent **[M]**.

Run time needs only the driver's `amdhip64_7.dll` and `amd_comgr_3.dll`: a code object compiled by TheRock runs with TheRock off PATH **[M]**, so the release is a zip of `ragon-serve.exe`, code objects and vcpkg DLLs (§6).

**Rule from the first kernel on:** fixed-order reductions, no split-K or atomic reductions, no data-dependent choice of kernel between runs, so exactness can be gated at all.

### 5.2 Format

Measured at 140 W **[M]**, study §6.5–§6.7; the figures are in the engine repository's `bench/amd/` folder and are not repeated here:

| | Result | Note |
|---|---|---|
| Read stream, whole box | measured | The ceiling for any decode |
| NVFP4, decoded in shader (`v_perm_b32` table), 16 × 64 tiles | measured | Steady over 90 s; M16/M1 within the gate; error against FP32 small |
| FP8 | measured | M16/M1 near 1 |
| WMMA throughput | measured | FP16, BF16, INT8, INT4; no FP4 or FP8 matrix unit on gfx11 |

The decode rate is close to the bus. **No 4-bit format can beat that per byte, so only fewer bytes make decode faster.** Per verify pass:

| Candidate | Bytes per pass | Against the served | Quality route |
|---|---|---|---|---|
| Served NVFP4 + FP8 projections | about 17.6 GB | same | The same checkpoint, loader and model id. Metal's L3 gate against CUDA (§5.3) |
| All-NVFP4 projections (the registry's Minima candidate) | about 14.4 GB, −18% | ×1.16 **[E]** LOWBIT | LOWBIT §7. Already the GB10's next candidate |
| GGUF `Q4_K` | about 16.7 GB, −5% | little | Changes the validated model for little: not proposed, measured for the record |
| 3-bit FFN | fewer | ×1.10 on its own **[E]** LOWBIT | LOWBIT §7, its riskiest rung |

**Prefill is where AMD really differs.** The GB10's prefill uses block-scaled FP4 MMA; WMMA offers none. The candidates: W4A16 or W8A16 on FP16 (accurate), W8A8 on INT8 (gufo's route), W4A4 on INT4 (but halogen shows +9% prefill for −0.45 points of top-1 at 64+ rows, so a quality question under LOWBIT §7). The spike measures all of them.

**Effect of the owner's latitude:** it lets AMD take a format the GB10 hasn't, if and only if it passes LOWBIT §7. The recommendation is not to spend that freedom unless the micro-benchmarks show more than the table above expects (WN4).

### 5.3 Exactness and quality

- **Within a build:** two runs byte-identical; speculative equals non-speculative on all 16 agentlong turns; `--spec-check`, `--prefix-check` from `ragon-run`.
- **Against the GB10:** METAL's L3 gate verbatim for the same checkpoint: 54 prompts, Fisher threshold 85 flips, flip gap ≤ 1.75 nats, top-5 100%, mean KL ≤ 0.0008 nats, perplexity ratio in [0.999, 1.001], acceptance within spread.
- **Prefill** falls under L3's prefill positions; Metal emulates CUDA's activation quantisation for it. A prefill format that cannot meet L3 goes to LOWBIT §7's ladder and is the owner's call.

### 5.4 The unknowns, and the first milestone

| Unknown | Why it matters | Where answered |
|---|---|---|
| Whole-step time: 21.2 GB a step at 100–110 ms **[E]** is arithmetic from one GEMM | The number everything rests on | A, W1 |
| Attention at 25–32K with BF16 KV; L2 is 2 MB against about 24 MB on the GB10; 32 MB MALL; DeltaNet state does not stay on chip | The study's largest non-GEMM cost | A (attention probe) |
| Current driver: M1 ran on AMD Software 26.10.1; event timing is wrong for long batches (use the wall clock) | Drivers move | A |
| 140 W is a front knob Windows cannot read; at 120 W the NVFP4 rate fell clearly **[M]** | Silent loss of speed on a customer's box | A: a short WMMA rate probe tells the two power settings apart, tested as the check |
| `cooperativeLaunch` 0: no co-residency of persistent blocks | Spin barriers in megakernel stages can stall; stages are optional (`RAGON_MK_FFN`, `RAGON_MK_GDN` in the tree) | W1 |
| TDR about 2 s: no kernel may exceed it | Prefill chunk sizes, persistent blocks giving up at 50 ms | A, W2 |
| Kill with work in flight: is the carve-out's memory returned? | The watchdog's design | A (host probes) |
| Session 0 at boot | §4.2 | A (host probes) |
| No rocprofv3 on Windows | Tuning by wall clock and counters only | W1 |
| Windows-specific penalty: gufo reports 7.6–9.5%, `hipStreamSynchronize` about 0.4 ms late, copy hand-offs about 150 µs | Budget per step | W1 |

**The first measurable milestone, A-gate.** Everything on the current driver at the 140 W setting:

1. Every GEMM shape of the served 27B (FFN, projections, head, drafter) at 1 and 16 rows: **≥ 200 GB/s, M16/M1 ≤ 1.15**. M1's shape reproduced within 5% as a check that nothing moved.
2. One hipified engine kernel (`topk.cu`, the smallest real path) against host references, **bit-identical across two runs**.
3. A row in one table per decode candidate and prefill candidate: rate, error against an FP32 reference, estimated milliseconds per step.
4. The attention probe at 32K and the session-0 and kill probes reported.
5. A restated estimate of tokens per second and prefill, each with its range.

**Decision point:** a report to the owner with "viable or not" and "which format", with the thresholds that would say no: a whole-step estimate under 50 tok/s on agent output, or a prefill ceiling under 500 at 32K, with the served checkpoint and no fixed route to better.

### 5.5 Phases and gates after the decision

From the study's §5. Effort **[E]**.

| Phase | Work | Weeks | Gate |
|---|---|---|---|---|
| **W0** Bring-up | CMake with clang-cl, vcpkg (ICU, PCRE2, nlohmann_json), hipify host, portable kernels, `MapViewOfFile` | 3–4 | FP32 references within tolerance; two runs bit-identical; kernel tests green |
| **W1** Decode | Stream GEMM, head, drafter, attention with fixed-order combine, DeltaNet replay, tree and lookup, `hipGraph`, `ragon-run` | 6–8 | Speculative equals non-speculative on 16 agentlong turns; byte-identical runs; **≥ 50 tok/s agentlong greedy**; L3 |
| **W2** Serving | Prefill on WMMA, HTTP, WDDM host safety, packaging, signing | 6–9 | Serve equals run byte for byte; `--prefix-check`; **prefill ≥ 500 at 32K**; a Work session from a client |
| **W3** Parity | **Not in the study:** vision (`vision.cu`, 559 lines), paged KV and `kv_pool`, batches of two, structured output | 2–4 | Metal's M5/M6 tests by name; L3 on image prompts |

The inventory refresh (backlog `win-engine-inventory`) restates this table. The engine has grown since c5ec4ec; it is 19,587 source lines today.

Estimates to judge the result by, **all [E]**: decode 55–65 tok/s on agent output; prefill 550–670 at 32K (700–850 at empty context); an agent turn at 32K about 6–7 s, against about 4 s on the GB10, about 15 s on gufo and about 18 s on llama.cpp with DFlash2. Prefill stays 4–5× below the GB10's whatever the engine. Baseline on the same box, **[M]**: llama.cpp decode 10.5–10.9 tok/s at 32K, prefill 226 at 32K.

### 5.6 Memory

Budget from INTEGRATION §12 **[E]**: weights and drafter about 19 GB, embedding about 3.5, sessions 30, prefix 2–4, scratch 2: about 57 GB. It fits 96 GB and 64 GB. Windows has 31.6 GB at 96; 26.8 GB was free at idle with the runner service present **[M]**. The pool is sized from the DXGI budget (`QueryVideoMemoryInfo`), never `hipMemGetInfo` (`hipInfo` double-counts: 107.87 GB **[M]**), and never past the carve-out: backing there runs at 0.63 GiB/s, decode falls 6–22% **[M]** and overflow can freeze the box.

Host safety follows RAGON_WINDOWS_ARM §9.2–§9.4, unchanged except that the engine is `ragon-serve`. Trip: available memory under 8 GB, or usage above 90% of budget, for 3 s; then `TerminateJobObject`. The distribution (WN2) adds a second layer: `.wslconfig memory` caps it at 6–8 GB **[E]**, and `side_service_allowance()` has a Windows twin that reserves it.

---

## 6. Install and update

`install.ps1 -Role node` (a new parameter; today's are `-Version`, `-NoPath`, `-NoSandbox`, `-NoApp`). Without `-Role`, nothing changes for a client.

1. **Check first, change nothing:** Windows 11 build, `gfx1151`, the driver's HIP and comgr DLLs at or above the pinned version, carve-out, cable or Wi-Fi (the box's Wi-Fi gave about 103 Mbps **[M]**; a model over it takes hours), Smart App Control state, power setting by rate probe.
2. Client steps as today (assets, checksum, `%LOCALAPPDATA%\Programs\Ragon\bin`, HKCU PATH).
3. Assets of the node: `ragon-engine-<version>-windows-gfx1151.zip`, SHA-256 against `SHA256SUMS`, unpacked under `%ProgramFiles%\Ragon`, not user-writable. No TheRock (1.68 GB, not needed at run time).
4. One elevation, for: the service and its account; `%ProgramData%\Ragon` and ACLs; firewall rules for TCP 8000 and 3100, the forge's ports and UDP 5353, program-scoped, Private and Domain only; `powercfg`; `sshd` `Match` block for the job user. Over SSH there is no UAC: `[Environment]::UserInteractive` is false, so the script needs an elevated session and says so.
5. Carve-out: read by ADLX; if under the need, say so and how; **never reboot unasked**. Changing it needs one reboot.
6. Sidecars and forge per WN2: enabling WSL also needs one reboot; the script resumes with a second run.
7. The model: `ragon model pull` from Hugging Face (about 2.2 MB/s in the trial **[M]**), or `sync-model` from another node.
8. `ragon audit egress` once, elevated; print the verdict.

**Update.** `ragon update` on a node: refuse while a job is in flight, or wait with a limit; stop the supervisor; swap binaries and engine zip; start; health check by a canary completion; roll back on failure by keeping the previous version's directory. The engine's update never asks for the toolchain. A driver update stays the user's: the supervisor checks the pinned minimum at start, and the rate probe after a driver change.

**Uninstall.** `ragon uninstall --node` removes the service, tasks, firewall rules and the distribution, keeps weights and the forge's data unless asked twice.

---

## 7. Security assumptions

- **The LAN is trusted**, the earlier decision for GB10 nodes. The model server listens on the LAN as there. Firewall rules are per program, Private and Domain; on a Public network the node advertises nothing and says why.
- **The forced-command account is not an administrator.** The account used on the AMD box today for agents is the local administrator; that is a test convenience and never the node's design. Win32-OpenSSH reads administrators' keys from a different file and its `DefaultShell` handling of `command=` must be verified (backlog `win-node-forced-command`), since it is the node's one remote entry.
- **Jobs are contained** by a Job Object (memory cap, time limit, kill on close) and the Windows sandbox accounts (`CodexSandboxOffline/Online`, ACLs, WFP). A cap on committed memory does not limit GPU memory, hence the DXGI check.
- **New at `/airgapped on`, in the distribution route:** sandboxed commands must reach neither `wsl.exe` nor `\\wsl$` nor Docker's pipe. Each is a way out of the empty network, the Windows twin of patch 0019's `host_sockets_at`. This must be tested by the egress audit and by a sandbox test; it is a condition on WN2 (b).
- **Egress.** The audit covers the engine's tree, the supervisor and the distribution. Windows' own traffic is outside the verdict and the report says so. Defender's sample submission can upload unknown binaries; signing reduces that and the docs say it.
- **Integrity.** Engine zip and code objects are pinned by SHA-256 in the release and installed where users cannot write. The driver runs whatever code object it is given, so this matters.
- **Signing.** WN8. Smart App Control off on our box proves nothing for customers.
- **Disk encryption.** The AMD box has no BitLocker **[M]**. Ragon does not require it on a node, as it doesn't on a GB10; the docs state what is at rest: the forge's repositories, job worktrees, session records. Owner to confirm.
- **A TDR reset takes out every GPU client on the machine.** That is a denial of service available to any job with GPU access, so jobs do not get direct GPU access beyond what the Windows sandbox already denies. To verify.

---

## 8. CI on the AMD box

The box is also the Windows CI runner: `windows.yml`, `release-build.yml` and the QA timer at 06:30 UTC via `windows-qa.yml`; release cuts run about 09:00–10:30 UTC.

**Rules until the second box:**

- Release jobs win. Engine and gate work checks `Get-Process Runner.Worker` and stops when one appears; none of it runs in the release window or the QA slot.
- Items that reboot, change the carve-out, enable a Windows feature or hold the GPU for long are marked BLOCKS-RUNNER in the backlog and are scheduled with the coordinator; the runner's service and directory are never touched.
- Nothing leaves a server resident when it stops.

**New jobs:**

1. **Host build, no GPU**: engine host code and tests, on any Windows runner.
2. **Node smoke, no GPU**: `-Role node` install with a stand-in engine, advertisement seen from a loopback browser, forced command, a job, status, uninstall. On a GitHub-hosted runner where possible, to keep load off the box.
3. **Engine build and kernel tests**, on the box's label with GPU, per merge to the engine's port branch. Whether the runner's service account reaches the GPU **[?]**: measured before designing this; if not, a separate job runner on the second box.
4. **The gate** (§9), weekly and before each 1.0 candidate, on the second box once it is here.

**When the second box arrives:** it is the engine's and the gate's. The first keeps the runner.

---

## 9. The acceptance gate

"A Windows node works" means all of these pass, in one run, on a fresh install, from `scripts/windows_node_gate.ps1` and a client-side twin, with a report. Thresholds are the study's; they may tighten, never silently loosen.

| # | Check | Pass |
|---|---|---|
| G1 | Fresh Windows, `install.ps1 -Role node`, then a reboot with nobody signed in | Service up; health endpoint ok within the limit |
| G2 | A Linux and a Windows client on the LAN, no configuration | Both find the node over mDNS |
| G3 | Pair from a client and from a GB10 | Pairing succeeds; a wrong key is refused |
| G4 | Decode, agentlong | **≥ 50 tok/s** greedy; speculative equals non-speculative; two runs byte-identical |
| G5 | Prefill at 32K | **≥ 500 tok/s** |
| G6 | L3 against the GB10 | §5.3 |
| G7 | A Work session from a client, 32K turn, tools | Completes; the 32K turn within the limit set after W1 |
| G8 | Images in, structured output, batches of two | The engine's own server tests pass |
| G9 | Job over the forced command: run, status, fetch, cancel, disconnect mid-run | The job survives the disconnect; its memory cap and time limit trip when exceeded; no shell is reachable |
| G10 | `sync-model` from a GB10 over cable | Hash equal; served after restart |
| G11 | Night Shift: `/night add --on <node>`, three attempts, chooser, report | Commit in a worktree; nothing merged or pushed |
| G12 | Search, voice, image search | `ragon-search`, `/api/transcribe`, `image_search` all answer from a client |
| G13 | The forge | Push over SSH, `/forge/` through `ragon web`, second mDNS service seen, backup runs |
| G14 | `ragon web --lan` from a phone and the desktop app against the node | The policy's conformance cases pass |
| G15 | Memory (RAGON_WINDOWS_ARM §9.4), with the distribution running | Engine killed, desktop responsive, nothing else killed, memory back, restart succeeds; then the same with KV past budget |
| G16 | Kill and power | Kill the engine at work: restart within the limit. Cut power: back unattended |
| G17 | `ragon audit egress` on a node, with everything running | Exit 0. At `/airgapped on`, no way out through `wsl.exe` or Docker's pipe |
| G18 | `ragon update` over a running node, then a forced failure | Up to date; rolled back on failure |
| G19 | 8-hour soak under agentlong, 60-minute thermal | No restart; step-time drift reported |
| G20 | A SWE-bench night served by the Windows engine and run by a GB10, the node otherwise idle | No significant loss by McNemar (LOWBIT §7, rung 4) |

G20 uses a GB10 as harness, so Windows needs no SWE-bench code: the node only serves. It can run once G4–G7 pass.

---

## 10. What 1.0 requires

**On AMD:** G1–G20 on the AMD box and on the second box; user and developer docs; the release assets; the 2.0 document amended; WN1–WN8 settled. **On NVIDIA:** per WN6.

| Work | Weeks **[E]** | Source |
|---|---|---|
| Spike (A) | 1–2 | This draft; most of it is measuring |
| Engine W0–W2 | 15–21 | AMD study §5 |
| Engine W3 parity | 2–4 | This draft, from Metal's M5/M6; restated by the inventory |
| Node tooling | 4–5 | AMD_NODES §6.4 |
| Night Shift, Rust | 2 | AMD_NODES §6.4 |
| Gate | 1 | AMD_NODES §6.4 |
| Sidecars, forge, web | 3–4 (WN2 b), 7–11 (native) | This draft's rough estimate, the weakest number here |
| Installer, CI, signing, docs | included above, about 2 of the node's | This draft |

**About 28–39 engineer-weeks, spike included (WN2 b).** Node work does not need the engine: a stand-in engine answers `/v1/models`, so tracks B to E run in parallel with A and W0.

Calendar shapes, not dates:

| Scenario | Shape | Cost |
|---|---|---|
| S1. Windows first | The kernel specialist on Windows, a second person on the node track | Calendar about 18–27 weeks. Metal's first release moves back by the same |
| S2. Parallel | Two kernel tracks; each Windows change and each GB10 change mirrored twice | Needs a second kernel person, and METAL's shared seams to be done first; calendar near S1 if staffed |
| S3. Metal first | Metal's M8 about the end of March 2027 **[E]**; then Windows | 1.0 moves by about 18 weeks; contradicts "1.0 waits for Windows" only if the Mac is not itself 1.0 |

**Phasing inside 1.0.** The gate's parts can land as 0.9.x features behind `-Role node`, marked preview, so the release train never waits and no client regresses: B and C can ship while W1 is in progress. Only the *1.0 cut* waits for G1–G20.

---

## 11. Work list

`windows-node-backlog` in scratch, moved to `specs/` or the backlog tool at review. Forty items, group A first.

---

## 12. NVIDIA second

The engine stays CUDA, so most of §5 does not apply and §5.1's host seams, §6 and §7 carry over. What decides it:

1. **Which machines.** The research note ordered a Windows on Arm RTX Spark laptop, 128 GB: unified, 45–80 W, CUDA 13.4 for `aarch64-pc-windows-msvc`. A discrete x64 RTX PC has too little VRAM for the 57 GB budget, even when the weights fit, unless sessions shrink. The owner says which class counts (WN6).
2. **The toolchain question, without a GPU** (research note §8, stage 0.1): does ptxas under MSVC accept the block-scaled MMAs for the target architecture? If not, the engine has no Windows build for that class.
3. **Memory**: `cudaMalloc` dedicated then spilling; no `cudaMallocManaged` (research note §3, §9).
4. **Gate**: §9 unchanged except G4–G6's thresholds, set from that machine's first measurements (research note §9).

Started only after AMD's W1 gate, except the toolchain probe, which needs no GPU and can run any time.

---

## 13. Sources

Local: the AMD study and `bench/amd/`; AMD_NODES §6, §12, §13; RAGON_WINDOWS_ARM §8–§10, §13, §14, §16, §17, §23; LOWBIT §0, §7, §8; METAL §4, §7; the reports on the AMD box.

Public, read 2026-11-06: ROCm/TheRock issue 9564 (Windows `gfx1151`: code compiled with 10.1.0 runs about 50× slower than with 10.0.0 and triggers repeated TDR); TheRock `windows_support.md` ("relatively new and not yet mature"); rocm.docs.amd.com component support; Docker Desktop's Windows install page (subscription terms, WSL2 or Hyper-V); Docker forum threads on starting without login (reports, not tested); codeberg.org/forgejo/forgejo releases (assets for 16.0.7 and 17.0.1: Linux amd64, arm-6, arm64 only); docs.searxng.org (install methods); Microsoft's `DnsServiceRegister` page. Study §7 for gufo, halogen and WMMA.
