# Ragon Profiling: measuring the model server and the agent the same way every time

**Status:** Proposed; spec only, building later. The owner's questions are in §0.1.

**Names:** post-rename (`ragon`, `ragon-admin`, `~/.ragon`, `ragon web` on port 3100); nodes `node-a`/`node-b`; the engine is ragon-serve (the `ragon-engine` repository, C++20/CUDA), SGLang the fallback engine.

**Builds on:**
- [INFERENCE](./DREAMFERENCE_INFERENCE.md) §5.3: the SGLang recipe and the one-stream speed method `scripts/decode_speed.py` implements (warm-up per prompt, median of three, decode net of time to first token, every answer checked);
- [ORGANISATION](./DREAMFERENCE_ORGANISATION.md) §7.8 (logs, fixtures and results: `~`, `node-a`/`node-b`, no user or host names) and §7.9 (the checks before each release, the top-level allowlist);
- [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §5.1 (the HIP port, one engine per build), §5.4 (no `rocprofv3` on Windows: tuning by wall clock and counters only) and §9 (gates G4, decode ≥ 50 tok/s on agentlong, and G5, prefill ≥ 500 tok/s at 32K);
- the engine's specs (`ragon-engine/specs/`): VALIDATION §13 (exactness, quality, the replay benchmark and the Nsight method) and BATCHING (the stage tables this spec's table format generalises);
- [docs/dev/host-safety.md](../docs/dev/host-safety.md): `check_host_safety()` before a load and the PSI watchdog during it;
- [RAGON_SWE_BENCH](./DREAMFERENCE_RAGON_SWE_BENCH.md): the harness, its per-task `wall_s` and the model gate it holds;
- the sibling specs being written at the same time, cited by name only: `DREAMFERENCE_LOGGING.md` (owns the per-request log line's format) and `DREAMFERENCE_MONITORING.md` (owns the thresholds the node watches).

## 0.1 Questions for the owner

Each has a recommended answer; "yes" accepts it.

- **O1. The command's name.** `ragon-admin profile` is the user-facing profiler (§6). `ragon-admin benchmark_server` is retired: it runs `vllm bench serve` inside the model server's container, and the ragon-serve image has no vLLM, so on the default engine it cannot run (§2.3). *Recommended: yes, `profile`; `benchmark_server` removed in Phase 1 with a one-line pointer to `profile` for one release.*
- **O2. Where the measuring code lives.** The measurement core (workloads, timing, the result file, the comparison) is a Rust leaf crate, `ragon-rs/profile/`, run as `ragon profile`; `ragon-admin profile` on Linux owns only the window (stop, start through `start_server()`, restore) and calls it. The Windows node, whose node half is Rust launcher subcommands, gets the same core with no Python. *Recommended: yes.*
- **O3. The standard workloads the product ships.** The engine's agentlong set never leaves the machine it was recorded on (only its manifest of hashes is committed), so it cannot be a product workload. The product ships public, fixed workloads: `decode_speed.py`'s three prompts for one stream, and a long-context prefix built deterministically from the installed package's own source files for the 32K and fan-out shapes. agentlong stays the engine developers' gate on `node-a`/`node-b`. *Recommended: yes.*
- **O4. Where results are kept.** Raw results on the machine under `~/.local/share/dreamference/profile/` (never in a repository); published figures only through the existing release entry `docs/releases/<version>.json` (a new `profile` block) and the generated `docs/benchmarks.md`. No tracked top-level `reports/` directory (§7.9 item 4 would need an allowlist change); the engine's own stage tables stay in the engine's specs. *Recommended: yes.*
- **O5. The regression rule.** Noise is the larger of the two arms' own spread over their warm repetitions, at least 0.5%. Within noise: no change. Worse by more than twice the noise and at least 2%: a regression. In between: one more ABBA pair, then "slower, within 2%" if it stays there, which is not a regression (§7.2). *Recommended: yes.*
- **O6. The release gate.** At each cut, on one node with the release candidate installed as production, the `one-stream` shape runs against the idle production server (no window, about three minutes): decode tok/s and TTFT not a regression against the previous release on the same node, every answer passes its check. A failure holds the cut until the owner says go. *Recommended: yes.*
- **O7. Speed in the app.** No tok/s or TTFT in the desktop app or `ragon web` through Phase 3. Later, one row in the thread's details panel ("Last turn: TTFT 1.2 s, 63 tok/s"), from the server's own figures for that turn. *Recommended: yes, later, details panel only.*
- **O8. Deep profiles in the product.** No `--deep` flag: Nsight Systems and Nsight Compute stay hand tools of the engine developers, never shipped, never on a release build's default path. *Recommended: yes.*

## 1. Scope and the rules that bind it

This spec defines what Ragon measures about speed and memory, how each figure is defined, how a measurement is made so two of them can be compared, where results go, and the command a user runs. It covers the model server (ragon-serve and the SGLang fallback), an agent turn, and a SWE-bench task. It does not cover correctness evaluation (the engine's VALIDATION §13 and the SWE-bench spec) beyond the byte-identical gate a speed claim needs, the log format (LOGGING) or alerting (MONITORING).

The product rules apply unchanged:

1. **No telemetry, ever.** Every figure stays on the machine. Nothing is sent anywhere unless the user sends it. Publishing is a person's act through a release entry (§5.4).
2. **One node, one model.** A profile measures the model the node serves. A window that starts another configuration stops production first and restores it afterwards. The two never run side by side.
3. **Host safety is a hard constraint.** Every server a profile starts goes through `check_host_safety()` and the PSI watchdog. A profile that cannot keep both does not run.
4. **Tests never touch the real machine.** The profiler's tests use a fake server, a fake clock and fake units, as every other test does.
5. **Windows node parity** (§8): the same figures, the same file and the same command on the AMD node. Only the deep level differs.
6. **Anonymity** (ORGANISATION §7.8): results name `node-a`/`node-b` or the node id, paths under `~`, and no user or host name.

## 2. Today

Surveyed on 2026-11-08: Ragon at `origin/main` (da9776b), and the engine at the engine repository's branch `eng/drafters-resident-rebased` (8907a53, queued for engine main). Ragon's `ragon-engine/` submodule is pinned at an older revision and never reaches a build; production runs the image `scripts/build_ragon_serve_image.sh` builds and `scripts/pin_ragon_serve_image.py` pins. Engine line numbers below are for 8907a53.

### 2.1 In the engine (always on)

- **`GET /metrics`** (`src/serve/main.cpp`, the route near line 1726; the renderer near lines 190–288): Prometheus text. It carries SGLang's names, so one reader serves both engines: `sglang:prompt_tokens_total`, `cached_tokens_total`, `generation_tokens_total`, `spec_verify_calls_total`, `e2e_request_latency_seconds_sum/_count`, `time_to_first_token_seconds_sum/_count`, and the gauges `num_running_reqs`, `num_queue_reqs`, `max_total_num_tokens`. It adds `ragon:` names: drafted and accepted draft tokens per drafter, memory refusals, session evictions, interleaved prefill chunks, batched steps, KV pool exhaustion and evictions, drafter-off steps and switches, and resident sessions.
- **`GET /spec_stats`**: JSON with the speculation counters (tree and sparse steps, drafter-off steps, ride steps, shadow counts) and, per drafter, requests, steps, drafted, accepted, `draft_seconds` and `commit_seconds`. The draft time is here but not in `/metrics`.
- **One stderr line per request** (`main.cpp` near line 1528): `request: session N, P prompt tokens (R reused[, from the prefix KV store]), prefill S s (C chunks, B beside a decode, T tok/s of compute); N tokens in S s (T tok/s), finish; P passes (B batched), X tokens/pass; step period M ms mean, X max over N; beside a prefill M ms mean, X max over N`. Every engine report's step period comes from this line.
- **Start-up lines**: the memory plan, whether the prefix KV store is on and how many MB it got, and whether step graphs are on.

### 2.2 In the engine (on request and by hand)

- **`RAGON_PROFILE=1`** (`src/core/engine.cpp:254`): synchronises after each phase and adds up its wall time. It prints `profile <phase> <seconds>` at exit (line 552). It **turns the step graphs off** (line 261), so its figures are for attribution only and are never production figures. LoRA adds `lora_decode/<kernel>` entries (`engine_lora.cpp:230`).
- **`ragon-run`** (`tools/ragon_run.cpp`): the checks `--spec-check`, `--batch-check`, `--fanout-check`, `--joint-check` (and others), each comparing a fast path's tokens against a plain one.
- **`--plan-only`** (`src/serve/admission.hpp:198, 215`): prints the memory plan as one JSON line and exits. Ragon's pre-load check reads it (`dreamference/vllm_server/vllm_server_manager.py:1790–1824, 2133`).
- **`ragon-stream-tests --bench-groups`** with `RAGON_BENCH_MS`: GB/s per GEMM at M rows.
- **Bench scripts** (`bench/`): `agentlong/run.py` (one stream over the sixteen fixed turns; reads `/metrics` around each request; defines decode tok/s, ms per step and tokens per step; records each turn's text SHA-256 for `--compare`), `fanout/run.py` (N forks of one prefix; defines the span and wall rates), `agentlong/mem_guard.sh` (a copy of the PSI watchdog's rule for servers a bench starts by hand), `profile/bubbles.py` (a step's GPU busy and idle time from an Nsight Systems trace).
- **VALIDATION §13** sets the engine's three gates (exactness, quality, the replay speed benchmark) and an Nsight method. Its checklist asks for "Nsight traces archived with the release".

### 2.3 In Ragon

- **`ragon-admin inspect`** (`dreamference/cli/dreamference_cli_controller.py`, `_inspect_probe_streaming` near line 3276): one streamed request. TTFT is measured on the client clock, and decode as `(tokens − 1) / (elapsed − TTFT)` (lines 3348–3364), printed as "Streaming: Yes (TTFT …, decode … tok/s on code)". A correctness canary runs beside it.
- **`ragon-admin benchmark_server`** (lines 2638, 4978): `docker exec dreamference-vllm-<port> vllm bench serve` on vLLM's sonnet set. The ragon-serve image (`docker/ragon-serve/Dockerfile`) has no vLLM, so it cannot run on the default engine. The pinned SGLang image was not checked.
- **`scripts/decode_speed.py`**: the INFERENCE §5.3 method, against any OpenAI-compatible server. Its answers are checked (prose, compiling code, parseable JSON), and a failed check makes the exit status 1.
- **`scripts/engine_ab_remote.sh`**: the engine A/B's server switch on a development node. It takes the GPU lock (`flock ~/.ragon-gpu.lock`), stops production, starts an arm, waits for `/v1/models` and restores production at the end.
- **Readers of `/metrics`**: Night Shift (`dreamference/night_shift/night_shift_host.py:178`: running and queued requests, a request counter, the KV pool) and the launcher's compaction limit (`ragon-rs/src/compaction.rs:152`: the KV tokens).
- **SWE-bench**: `wall_s` per instance (`dreamference/swe_bench/swe_bench_instance_run.py:745`, whole seconds), summed in the report (`swe_bench_report.py:241`). The gate hold (`swe_bench_gate_hold.py`) refuses other traffic while a run lasts.
- **Releases**: `scripts/release_benchmark.py` writes `docs/releases/<version>.json` and `docs/benchmarks.md`, with SWE-bench figures only. The release dry run (`.github/workflows/release-dry-run.yml`) runs on hosted runners, with no GPU and no speed step.
- **The app and `ragon web`**: the thread shows its context use only ("context N%", `desktop/ui/src/App.tsx:3296`; the details panel row at 3741, from `ThreadTokenUsage`). No tok/s or TTFT appears anywhere in the UI. `ragon-rs/monitor/` is the session monitor of agent transcripts, not a performance monitor.

### 2.4 The practice (the engine reports of 2026-11-07 and 11-08)

The engine work is measured by hand in a fixed way, written down here so it becomes the rule:

- **GPU windows.** Production is stopped, the port and GPU are checked free, the steps run, and production is restored healthy. Each window is logged with its UTC span, e.g. "Window W2 (04:49–05:37, production restored healthy)". A window holds the GPU lock, has a 40-minute limit, and a step that runs over it is moved to the next window rather than counted as a failure.
- **Warm repetitions.** Repetition 0 captures the graphs and is not counted. The cold 32-fork start runs at 63–65 tok/s against about 200 warm, reproduced four times.
- **ABBA order** for two builds (base, new, new, base), on one machine in one window.
- **Byte-identical first.** Every speed claim follows a 16/16 identical text check on agentlong (or 52/52 on the drafter replay).
- **The figures**: span and wall tok/s; draft + verify ms per step; the step period; tokens per pass; `RAGON_PROFILE`'s per-phase split; GEMM GB/s at M = 1/16/32/48/64; MemAvailable minimum; prefill per 1K new tokens; the sum of TTFT.
- **Observed noise**: agentlong decode between ABBA arms 0.3–0.4% (82.04 against 82.38 s; 63.97 against 64.05 tok/s); 32×2 warm repetitions 232.0–235 tok/s (about 1.3%).

What is missing: one place that defines the figures, a result format, a window rule that holds outside a single job's helper script, a user command that works on the default engine, a baseline anything can compare to, and any speed check at a release.

## 3. What is measured

Every figure has one definition, one unit and one source. **The server's counters are canonical.** A client's clock is a check: it must not be faster than the server's (TTFT, decode) and may be slower by the network and streaming overhead.

| Figure | Definition | Unit | Source |
|---|---|---|---|
| **TTFT** | From the request's arrival at the engine to its first generated token, queue wait included | s, 3 decimals | `time_to_first_token_seconds_sum` delta around a lone request; for concurrent requests, the request line's prefill time plus its queue wait |
| **Prefill per 1K** | prefill seconds × 1000 / (prompt tokens − reused tokens) | ms per 1K new tokens, reported with the context bucket (≤ 8K, 8–32K, 32–64K, > 64K) | the request line |
| **Decode tok/s, per stream** | (generated − 1) / (end-to-end − TTFT); the first token belongs to prefill. Over a set: Σ(generated − 1) / Σ(e2e − TTFT), weighted by tokens, never a mean of rates | tok/s, 1 decimal | `/metrics` deltas (as `agentlong/run.py`) |
| **Decode tok/s, aggregate: span** | Tokens produced after the last stream's first token, divided by the time from that first token to the last token of any stream | tok/s | per-token arrival times (as `fanout/run.py`) |
| **Decode tok/s, aggregate: wall** | All generated tokens / (last token − first submission), prefills included | tok/s | the same |
| **Step period** | Time between the starts of consecutive decode steps of one request; mean and max, apart from steps with another request's prefill chunk between | ms | the request line |
| **Draft / verify split** | Per step: draft seconds and verify seconds over the steps in the interval (per batch step for batched shapes); commit counted only where it is not overlapped | ms per step | `/spec_stats` deltas |
| **Tokens per pass** | (generated − 1) / verify passes | tokens | `/metrics` |
| **Acceptance rate** | accepted draft tokens / drafted tokens, per drafter | % | `ragon:spec_accepted_tokens_total` / `ragon:spec_drafted_tokens_total` |
| **KV and pool** | The planned bytes (weights, KV or page pool, prefix store, scratch), the prefix store's state (on, MB), and the MemAvailable minimum over the run | bytes in the file; GB (10⁹) in tables | `--plan-only`, the start-up lines, `/proc/meminfo` read once a second |
| **GEMM GB/s** | Bytes the GEMM streams (weights, activations read, output written, as the bench counts them) / its median time, at M rows; reported beside the streaming microbenchmark's measured peak | GB/s | `ragon-stream-tests --bench-groups` |
| **Agent turn wall** | From the turn's start to its completion (`turn/started` to `turn/completed`), tools included; split into model time (Σ request e2e) and the rest | s | the session's events, with the server's counters for the model part |
| **SWE-bench task wall** | `wall_s` of the instance, from the agent's start to its end | s (integer) | the harness's state file; sum and median per run |

**Sampling rules**, for every figure:

1. **Warm only.** Each arm starts with a warm-up that is not counted: one request per prompt for request shapes, repetition 0 for batch shapes, which captures the graphs. A cold-start figure is a separate, named figure and is never mixed in.
2. **Repetitions.** At least three counted repetitions for a single arm, reported as the median and the spread (max − min) / median. A comparison uses ABBA with at least one counted repetition per arm visit, and reports the mean of the two visits.
3. **One request at a time** wherever per-request figures come from counter deltas, unless the shape is concurrent by definition. A concurrent shape takes its per-request figures from the request lines.
4. **Level-2 figures** (§4.2) are never compared with level-1 figures: `RAGON_PROFILE` turns graphs off and synchronises, so it changes what it measures.
5. **Greedy** (temperature 0) for every speed figure, so two arms produce the same tokens and the byte-identical gate applies. A sampled shape is named as sampled and has no identity gate.

## 4. Levels of profiling

### 4.1 Level 1: always on, cheap

These are the engine's counters, kept on in every build and every engine. They do no synchronisation and allocate nothing per step.

- **`/metrics` is the always-on surface** (decided here; MONITORING reads it). The names stay SGLang-compatible so the SGLang fallback answers the same reader. Additions to ragon-serve, where a figure of §3 has no counter yet: `ragon:decode_seconds_total`, `ragon:prefill_seconds_total`, `ragon:prefill_new_tokens_total`, `ragon:spec_draft_seconds_total`, `ragon:spec_verify_seconds_total`, and a histogram `ragon:step_period_seconds` (buckets 50, 100, 150, 200, 250, 300, 400, 600 ms, 1 s, +Inf). Under SGLang, `ragon:` figures read as "n/a", and a comparison across engines uses only the shared names.
- **`/spec_stats` stays** as the detailed JSON for the profiler. It is not a monitoring surface.
- **The per-request line stays** in the engine's log. Its format is owned by LOGGING; the profiler parses it and nothing else does.
- Nothing is pushed anywhere. `/metrics` answers on the engine's port, which is loopback behind the model gate.

### 4.2 Level 2: on request, inside a window

These are switched on for one run, inside a profile window (§5.1), and never on a production server:

- `RAGON_PROFILE=1` (per-phase split, the `<kind>passes` counter), `RAGON_KSPLIT` and the other environment knobs;
- the `ragon-run` checks (`--spec-check`, `--batch-check`, `--fanout-check`, `--joint-check`);
- the GEMM bench (`ragon-stream-tests --bench-groups`, `RAGON_BENCH_MS`);
- `--plan-only`, which is cheap enough to run outside a window too, since it loads nothing.

`ragon-admin profile` offers the per-phase split and the GEMM bench only when the pinned engine image carries the binaries. Otherwise it refuses and says why. The checks stay engine-developer tools.

### 4.3 Level 3: deep, by hand only

Nsight Systems (with `--cuda-graph-trace=node`, so a graph's kernels appear one by one), Nsight Compute, and `bench/profile/bubbles.py` over a trace. Only by hand, only in a window, never shipped, never on a release build's default path (O8). Traces hold absolute paths and host names, so they are not committed or attached to a release. They are kept in the results directory, and only their summary (per-kernel ms, busy/idle, GB/s) goes into a table. This narrows the engine checklist's "Nsight traces archived with the release" to "kept on the node, summary in the release entry".

## 5. The method

### 5.1 The profile window

A **window** is any period in which production is not serving because a measurement runs. The rule is the same for `ragon-admin profile` and for an engine developer's own script:

1. **Refuse** when any of these holds: a `ragon-swe-*` unit is active; the SWE-bench gate hold is closed; a Night Shift run is in progress; the GPU lock (`~/.ragon-gpu.lock`) is held; another window is open; production has running or queued requests (`num_running_reqs` or `num_queue_reqs` above 0); `check_host_safety()` fails.
2. **Take the GPU lock** (`flock ~/.ragon-gpu.lock`) for the whole window, so engine developers and the product share one lock.
3. **Log the start** (§5.5) and **stop production** through `ragon-admin server stop`. Check that the port is free, no model-server container or process is left, and the GPU is idle.
4. **Start each configuration through `start_server()`** with the shape's recipe overrides, so `check_host_safety()` runs before the load and the PSI watchdog during it. A developer's raw `ragon-serve` runs beside `mem_guard.sh`, the engine's copy of the same rule.
5. **Run the steps.** A window lasts at most 40 minutes. A step that would overrun is moved to the next window and is not a failure.
6. **Restore**: stop the configuration; check the port free and the GPU idle; start production through `ragon-admin server start`; wait until `/v1/models` answers with the production model id. Only then is the window **restored healthy**.
7. **Log the end** and release the lock. If restoring fails, the window's log line says so, the lock is released, and the node's own alert channel is told (MONITORING). The profiler never retries a failed production start in a loop.

A profile that needs no other configuration and finds production idle runs **against production, with no window**. It still takes the GPU lock, so no window can open under it.

### 5.2 Repetitions and order

- Warm-up, then the counted repetitions (§3 rule 2).
- **ABBA** for every comparison: base, candidate, candidate, base. The pair is on the same machine, in the same window, as close together as the shapes allow ("the same minute"). Two windows on the same night are acceptable only when one window cannot hold both arms, and the result says so.
- **The same inputs.** The same request bodies byte for byte (hash-checked, as `agentlong/run.py` does), the same seed, the same context, the same flags apart from the one under test.
- Base and candidate run on **the same machine**. A figure from another GB10 is a reference only: the drafter replay's reference on another box sat about 0.5% above the same build's figure.

### 5.3 The byte-identical gate

Before any speed claim between two builds that are meant to produce the same output, every greedy request's output text has the same SHA-256 in both arms (n/n), and the verify passes match. If the outputs differ, there is no speed figure: the result is "not identical", with the first differing request. A change meant to alter outputs (a new drafter policy that changes greedy tokens, a model, a quantisation) replaces this gate with the engine's quality gate (VALIDATION §13), and the result names which gate it used.

### 5.4 How a result is written

Each run writes `result.json` (every figure, every repetition, raw counter deltas) and `table.md` with one fixed table:

| field | content |
|---|---|
| build | Ragon version; engine and image digest (or engine commit for a developer build); model id; drafter |
| flags | the recipe's difference from production, or "production" |
| shape | the shape's name and parameters (streams, context, max new tokens, reps counted) |
| window | UTC span, "restored healthy" or not; "no window" for production runs |
| machine | `node-a`/`node-b` or the node id; MemAvailable minimum |
| identical | n/n, or "not identical at request k", or "quality gate" |
| figures | one column per §3 figure that applies: median, spread %, and for comparisons base, candidate and Δ% |
| verdict | within noise / regression / faster / unclear (§7.2) |

Writers normalise paths to `~` and replace host names before writing, as ORGANISATION §7.8 requires, so a table can be pasted into a spec or a release entry unchanged.

### 5.5 Where results are kept

- **On the machine**: `~/.local/share/dreamference/profile/`:
  - `runs/<UTC stamp>-<shape>/`: `result.json`, `table.md`, the engine's log for the run, any trace;
  - `windows.log`: one line per window (start and end UTC, label, production stopped, restored healthy, port and GPU free before and after, MemAvailable minimum);
  - `baselines.json`: the accepted figure per shape on this node, with the run it came from (§7.1).

  Kept until removed by hand. A run directory without traces is a few hundred KB.
- **Published**: only through a release entry (O4). `docs/releases/<version>.json` gets a `profile` block with the release gate's table (§7.3). `docs/benchmarks.md` is regenerated from the entries. Engine stage tables stay in the engine's specs, as today. Nothing goes in a tracked top-level `reports/` directory.

## 6. The user-facing surface

### 6.1 `ragon-admin profile`

```
ragon-admin profile run <shape> [--reps N] [--yes] [--json]
ragon-admin profile compare <run-a> <run-b>
ragon-admin profile baseline [--set <run>]
ragon-admin profile windows [--last N]
ragon-admin profile gemm [--rows 1,16,32,48,64]       (level 2; only when the image carries the bench)
```

The standard shapes:

| Shape | Workload | Needs a window | Figures |
|---|---|---|---|
| `one-stream` | the three fixed prompts (prose, code, JSON), thinking off, plus one 32K-context turn on the shipped prefix | no, when production is idle | TTFT, prefill per 1K, decode per stream, tokens per pass, acceptance, step period |
| `four-streams` | four sessions at once on the shipped prefix, distinct questions | no when production's sessions cover four, else yes | aggregate span and wall, per-stream decode, step period, MemAvailable minimum |
| `fanout-32` | 32 forks of one 32K prefix, greedy | yes (the recipe needs 33 sessions at 32,768) | span and wall, draft + verify per step, step period, pool bytes, cold rep 0 reported apart |
| `swe-slice` | N instances (default 8) of the seeded gate slice (`scripts/swe_bench_slice.py`) through `ragon-admin swe-bench run` | no window; it holds the SWE-bench gate | task wall (sum, median), agent turn wall, the server's figures over the run |

`run` prints the §5.4 table and the path of the run directory. A shape that needs a window states what it will stop and for how long, and waits for a yes (`--yes` skips the question). It refuses on the §5.1 conditions, printing the reason in one line (for example, "❌ A SWE-bench run is active (ragon-swe-im100-refine); try after it ends."). It always restores production, also on Ctrl-C and on an error. A run that ended without "restored healthy" exits 2.

`compare` applies §7.2 to two runs of the same shape and prints the verdict. It refuses runs of different shapes or different machines, and two arms not interleaved as ABBA are labelled "unpaired".

### 6.2 What the app shows

Today, nothing about speed (§2.3). Through Phase 3, still nothing: speed is a node-administration matter, and a per-turn figure from the client's clock would contradict the server's. Later (O7), one row in the thread's details panel beside "Context", from the server's own figures for the turn's last request.

### 6.3 Other users of the figures

`ragon-admin inspect` keeps its streaming probe as a quick check and labels its numbers "client clock". `scripts/decode_speed.py` becomes the `one-stream` shape's three-prompt half; the script stays as a standard-library entry point that writes the same `result.json`.

## 7. Baselines, regressions and the release gate

### 7.1 Baselines

A **baseline** is the accepted result of a shape on one node, recorded with `profile baseline --set`. It is set when a release is installed (from the release gate's run), after an engine change lands with a measured gain, or by hand. MONITORING compares the live counters against the baseline's figures for the same shape. A baseline from another node is never used.

### 7.2 When a difference counts

For one figure of one shape, with base and candidate measured as in §5.2:

- **noise** = max(spread of the base's counted repetitions, spread of the candidate's, 0.5%);
- **within noise**: |Δ| ≤ noise. Reported as "no change".
- **regression**: worse by more than max(2 × noise, 2%);
- **faster**: better by more than max(2 × noise, 2%);
- **unclear**: anything in between. One more ABBA pair runs; if it is still unclear, the verdict is "slower (or faster) within 2%", which is not a regression and not a gain to claim.

The figures judged are the shape's **decisive** ones: decode per stream and TTFT for `one-stream`; span tok/s for the concurrent shapes; task wall for `swe-slice`, where the noise floor is 5% because model nondeterminism across tasks is larger. Other figures are reported but not judged. The figures of §2.4 support these numbers: ABBA arms 0.3–0.4% apart and warm repetitions 1.3% apart sit inside the band, and the 6% TTFT loss the KV-pages job rejected sits well outside it.

### 7.3 The release gate (O6)

A release has none today. The proposed gate is cheap and needs no window:

1. On one node, after the release candidate is installed as production (the release test procedure already installs it), with production idle and no SWE-bench run active: `ragon-admin profile run one-stream --reps 3`.
2. **Pass** when every answer passes its check (the `decode_speed.py` checks), and decode per stream and TTFT are not a regression (§7.2) against the previous release's gate run **on the same node**.
3. The table goes into `docs/releases/<version>.json` under `profile`, with the node id, not the host name. `scripts/release_benchmark.py check` validates the block.
4. A fail holds the cut and is reported to the owner with the table. The owner may release anyway, and the release notes then say so.

The byte-identical gate does not apply across releases, since the model, drafter or engine may legitimately change outputs. It stays the gate for engine changes (§5.3), which happen before the cut.

## 8. The Windows node (AMD Strix Halo)

The same definitions, file format, shapes and verdict rule apply; the measuring core is the same Rust crate (O2). What differs:

- **Level 1**: the HIP port of ragon-serve keeps `/metrics`, `/spec_stats` and the request line unchanged. The profiler cannot tell the platforms apart by the API.
- **Level 2**: `RAGON_PROFILE`'s per-phase synchronisation and the GEMM bench port with the engine (they use events and timers, not CUDA-only tools). The GEMM table reports against the AMD box's own streaming microbenchmark peak.
- **Level 3**: none on Windows (WINDOWS_NODE §5.4: no `rocprofv3`). Attribution there is by level 2 alone, which the per-phase split was built for.
- **The window**: production is the Windows service, stopped and started through the node half's own commands. The GPU lock is a named mutex in place of `flock`. Host safety is the Windows equivalent (memory notifications and Job Objects) that WINDOWS_NODE specifies. MemAvailable becomes the available physical memory from the OS.
- **Paths**: the results directory is the platform's local data directory (`%LOCALAPPDATA%\dreamference\profile\`), normalised in tables as `~`.
- **Baselines**: G4 (decode ≥ 50 tok/s, agentlong greedy) and G5 (prefill ≥ 500 tok/s at 32K) are the first. The `one-stream` shape's 32K turn gives the G5 figure on the shipped prefix. G4 stays on agentlong, run by the engine developers.
- **Workload parity**: the shipped prefix is built from the installed package's sources. A Windows node without Python ships the same prefix as a data file, byte-identical to the Linux one (a test compares their hashes).

## 9. Relation to LOGGING and MONITORING

**LOGGING** owns the format of the engine's per-request line and of every other performance log line (field names, units, the order, how a line is versioned). The profiler is a reader of those lines: it parses the format LOGGING defines, and when the format changes, LOGGING's version marker tells the profiler which parser to use. This spec defines what each figure means (§3). LOGGING defines how a figure is written in a line. Neither repeats the other.

**MONITORING** owns the always-on watch: which `/metrics` figures the node samples, how often, the thresholds, and the alerts through the user's own channels (off until turned on). The profiler produces what the monitor compares against: the per-node baselines of §7.1, with their noise. The monitor never starts a profile, and the profiler never alerts. A monitor finding ("decode 15% under the baseline for an hour") is the cue for a person to run `ragon-admin profile`.

## 10. Rollout

- **Phase 0 (this document).** The definitions, the method and the questions.
- **Phase 1: measure what is there.** The Rust core (`ragon-rs/profile/`, O2) with the §3 figures from `/metrics`, `/spec_stats` and the request line; `result.json` and the fixed table; `ragon-admin profile run one-stream|four-streams` against idle production, with no window; `compare`, `baseline`, the results directory; `benchmark_server` retired (O1); `decode_speed.py` writing the same result format. Tests use a fake server that replays recorded `/metrics` bodies and request lines.
- **Phase 2: windows.** The §5.1 window with the GPU lock, refusals, `windows.log` and the restore check; `fanout-32`; the level-1 counter additions in ragon-serve (engine repository, through its publish queue); `profile gemm` where the image carries the bench.
- **Phase 3: baselines and the release gate.** The `profile` block in `docs/releases/<version>.json` and `release_benchmark.py check`; the gate of §7.3 in the release procedure; baselines handed to MONITORING.
- **Phase 4: the agent and SWE-bench.** `swe-slice`; agent turn wall from the session's events.
- **Phase 5: the Windows node,** with the engine port's W1 (WINDOWS_NODE §5.5).
- **Later (O7):** the details-panel row in the app and `ragon web`.

## 11. Acceptance

1. `ragon-admin profile run one-stream` against a fake server prints the §5.4 table, writes `result.json` with every §3 figure the shape names, and computes decode as Σ(generated − 1)/Σ(e2e − TTFT) from the counter deltas (a test with fixed deltas checks the number).
2. Repetition 0 never enters a counted figure (a test feeds a slow repetition 0 and checks the median ignores it).
3. `run` refuses, with one line naming the reason and without stopping anything, when a `ragon-swe-*` unit is active, the SWE-bench gate is held, the GPU lock is held, production has running or queued requests, or `check_host_safety()` fails (one test each, with every unit and lock faked).
4. A window started through `run fanout-32` calls `server stop`, starts its configuration through `start_server()` (so the safety check and the watchdog run), and calls `server start` for production on success, on an exception and on interrupt. The window line in `windows.log` records the UTC span and whether it was restored healthy (tests with a fake manager).
5. A run whose production restore fails exits 2 and logs "not restored". The profiler does not retry the start.
6. `compare` declares "within noise", "regression", "faster" and "unclear" exactly by §7.2 on fixed inputs at the boundaries (0.5% floor, 2% minimum, twice the noise). It refuses two runs of different shapes or machines.
7. `compare` gives no speed verdict when the outputs differ; it reports "not identical at request k".
8. No table or `result.json` contains a host name, a user name or an absolute home path (a test runs the writer with a home and host name planted in the inputs).
9. The profiler opens no connection beyond the model server's loopback port and its own node's commands (it runs under the egress audit's rule).
10. `benchmark_server` is gone from the parser, and the CLI reference and docs say `profile`.
11. Phase 3: `release_benchmark.py check` fails a release entry whose `profile` block is missing a field or names a host.
12. Phase 5: on the Windows node, `one-stream` writes a `result.json` that the Linux `compare` reads, and the shipped prefix's hash equals the Linux one.

## 12. Decisions

*(Left for the owner's answers to §0.1.)*
