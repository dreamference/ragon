# The Windows engine port: phase 1 plan

The engine port to the AMD box (Strix Halo, Radeon 8060S, gfx1151, 128 GB unified LPDDR5X, Windows 11 Pro) starts now (owner, 2026-11-10), no longer at 2.0. This note is the plan for its first phase and the briefs of its first three lanes. The analysis behind it is the engine study `specs/DREAMFERENCE_RAGON_ENGINE_AMD.md` in `ragon-engine` (§5 the plan, §6 what was run on the box) and its inventory `AMD_INVENTORY.md`; the node side is `specs/DREAMFERENCE_WINDOWS_NODE.md` §5. Measured figures live in those files, not here.

## The constraints (owner, 2026-11-10)

- **Native Windows engine**: HIP on Windows, no WSL2 for the engine. WSL2 only for the sidecars (this replaces WN2's "native per sidecar" in the Windows spec).
- **The 1.0 gate**: at least **40 tokens/s single stream, exact** (greedy identical to the reference below). The study's own W1 target, 50 tokens/s on agentlong greedy, stays the aim.
- **QA**: the coordinator over SSH, plus the Windows QA timer on the box.
- **One backend interface** for CUDA, HIP and Metal (`ragon-engine` branch `eng/backend-interface`, spec `specs/DREAMFERENCE_RAGON_ENGINE_BACKEND.md`). This port is the interface's defining second implementation (its §7, the HIP row); Metal comes after it and must fit the same seam.

## Phase 1 is the study's W0

The study's phases are M1 (the verify-kernel probe, passed 2026-10-27), W0 (toolchain and bring-up), W1 (decode), W2 (prefill and `ragon-serve`), and the inventory's W3 (vision, paged KV, `kv_pool`, batches). **Phase 1 is W0, opened by the Windows spec's A-gate (§5.4)**: the A-gate is cheap, settles whether W1's numbers are reachable at all, and two of its five items are W0 work anyway.

Inside phase 1 the A-gate items come first:

1. every GEMM shape of the served 27B model at 1 and 16 rows: at least 200 GB/s, 16-row time at most 1.15× the one-row time, M1's figure reproduced within 5% on today's driver;
2. one real engine kernel (`src/core/topk.cu`) hipified, bit-identical across two runs and against the host reference;
3. a table per decode and prefill candidate (the study's §4 shapes);
4. the attention probe at 32K context, plus the session-0 and kill probes (they block the CI runner, see the windows below);
5. tokens/s and prefill restated with ranges.

**Stop condition**: the Windows spec's decision point (§5.4) names 50 tokens/s for the whole decode step and 500 tokens/s prefill at 32K; the owner's 1.0 gate, set later, is 40 exact. So: a decode estimate **under 40 tokens/s**, or a prefill ceiling under 500 at 32K, with no fixed route to better, stops phase 1 with a report before the rest of W0 is spent; an estimate **between 40 and 50** continues and is raised to the owner as a decision (it meets the 1.0 gate, not the study's aim).

### The kernels of phase 1

The study's W0 list (norms, conv, DeltaNet recurrence, sampling, top-k, the drafter's non-GEMM parts) by file, with the inventory's class:

| File | Lines | Class | What it takes |
| --- | --- | --- | --- |
| `src/core/topk.cu` | 131 | hipify | `__shfl_*_sync`; wave32 matches. First kernel (A-gate item 2) |
| `src/core/elem.cuh` | 85 | hipify | element helpers |
| `src/core/kernels.cu` | 963 | hipify | norms, conv, DeltaNet recurrence, sampling; `prefetch.global.L2` and one `cp.async` become plain loads; programmatic launch dropped |
| `src/core/kernels.cuh` | 580 | hipify | declarations and launch wrappers |
| `src/core/launch.cuh` | 98 | rewritten | programmatic dependent launch has no HIP twin: plain launches |
| `src/core/draft_kernels.cu` | 549 | hipify | the drafter's non-GEMM kernels; `__shfl_*_sync` |
| `src/core/model.cu` | 287 | hipify | FP8 and FP4 types from the HIP headers |
| `src/core/sparse_head.cu` | 295 | hipify | `cub` becomes hipcub; its one `mma.sync` stage is rewritten |
| `tests/kernel_tests.cu`, `tests/mk_tests.cu` | 637 | follow the kernels | runtime calls hipified; driver- and PTX-specific cases follow their kernel |

Not in phase 1: `stream_gemm.cu` (W1; the A-gate's GEMM probe uses `bench/amd/vg.hip`, whose layout and software FP4/FP8 decode W1 then carries in), `attention_body.cuh` (W1), `prefill_gemm.cu` and `prefill_gdn.cu` (W2), `gemm_bf16.cu` (W2, TMA), `vision.cu` (W3). Host code in phase 1 is only what the kernel tests and the loader need: `safetensors.cpp` on `MapViewOfFile`, and tier 1 of the backend interface; tier 2 (the runtime facade over about 60 CUDA runtime calls in `engine.cpp`, `speculate.cpp` and friends) is W1's.

## The toolchain (the study's choice, confirmed on the box)

- **TheRock 10.0.0**, already on the box outside PATH; it stays pinned (later SDKs are reported to stall on gfx1151). Its `hipcc` built and ran a trivial kernel, bit-identical over four runs, on and off PATH.
- **No AMD HIP SDK installer and no reboot**: the installer replaces the display driver, and the M1 measurements rest on the current one.
- The runtime is only the driver's HIP DLL and comgr: the shipped engine needs no SDK on the user's machine.
- Visual Studio 2022 Build Tools (MSVC, clang-cl) and the Windows SDK; CMake and Ninja; vcpkg for ICU, PCRE2 and nlohmann_json. A Visual Studio install is on the box; lane 1 first verifies that it has the Build Tools' MSVC and clang-cl and the Windows SDK, and if either is missing it stops and asks (installing them is the owner's call). CMake, Ninja and vcpkg are not yet on the box: lane 1 installs them (user-level, no reboot).
- Every device build names `--offload-arch=gfx1151` (the default target builds, then fails at run time with "device kernel image is invalid").
- **Optimisation flags**: TheRock issue 9564 reports newer Windows builds ignoring `-O`; the workaround is `-clang:-O2`. Whether it takes effect is measured in lane 1 (a kernel timed at both settings), not assumed.
- **Determinism rule from the first kernel**: fixed-order reductions, no split-K or atomic reductions, no data-dependent kernel choice between runs.

## The box's free windows (UTC)

The box is also the self-hosted Windows CI runner (labels `self-hosted, windows, x64, amd-box`). Its fixed load:

| When | What |
| --- | --- |
| daily 02:07 to about 04:30 | the release dry run |
| daily 06:30 to about 07:00 (longer on some days) | the Windows QA timer |
| Tue and Fri from 09:00, about 2.5 h | the release cut and the post-release install test |
| on push to `windows/**` or on dispatch | `windows.yml` |
| weekly, announced | the owner's app click-through |

Engine lanes work **07:00 to 02:00**, except Tuesday and Friday 09:00 to 11:30. Builds and correctness runs may share the box with a CI job; **timing runs may not**: a lane checks that the runner is idle before and after every timed run and discards the run if it was not. Probes that block the runner (A-gate item 4) run only in the evening part of the window, 20:00 to 02:00. The GB10 nights do not use the box. When a second Strix Halo box arrives, engine lanes move there and this table stops binding them.

## The gates of phase 1 (W0)

- The A-gate above, and its stop condition.
- **W0's reference**: every phase-1 kernel within its tolerance against the FP32 host references of `tests/` (as on the GB10) and the probe's FP32 reference kernel; **two runs of every kernel bit-identical**.
- `tests/kernel_tests.cu` and `tests/mk_tests.cu` green on the box from a CMake build with `-DRAGON_BACKEND=hip`, and the CUDA build on the GB10 unchanged (its tests still green) by every phase-1 commit.
- Phase 1 closes with a report that restates W1's estimate from what the A-gate measured.

The gates after it, for the record: W1 is speculative greedy identical to non-speculative greedy on all sixteen agentlong turns, two complete agentlong runs byte-identical, and the 1.0 gate of 40 tokens/s exact; quality against the GB10 (not an exactness reference) uses the L3 bars of the Windows spec. W2 is `ragon-serve` byte-identical to `ragon-run`, `--prefix-check`, and at least 500 tokens/s prefill at 32K.

## Where the work lands

All engine code goes into `ragon-engine` on branches from `eng/backend-interface`, in fresh clones (never the checkout's submodule), published through the engine queue. The HIP backend files (`src/backend/hip/types.hpp`, the CMake branch for `RAGON_BACKEND=hip`) are the backend spec's §7, written by lane 1. A lane that finds the interface needs a change records it in that spec in the same branch, so Metal inherits it.

## The first three lanes

### `p1-hip-build`: toolchain and the first kernel

On the box: verify the Build Tools' MSVC and clang-cl and the Windows SDK (missing: stop and ask), then install CMake, Ninja and vcpkg (user-level, no reboot), with ICU, PCRE2 and nlohmann_json. In a branch from `eng/backend-interface`: add `src/backend/hip/types.hpp` (`hipStream_t`, `__hip_bfloat16`, `__half`; capabilities: graphs yes, dependent launch no, green contexts no, bulk copy no), lift the `#error` and the CMake refusal for `hip`, compile the `.cu` sources as HIP for gfx1151 with clang-cl and TheRock 10.0.0. Hipify `src/core/topk.cu` and its test; record seeded inputs and CUDA outputs on a GB10. Measure whether `-clang:-O2` takes effect. **Done when**: the HIP build of the target holding `topk.cu` and its test configures and builds from a clean directory; `topk.cu` is bit-identical across two runs and against the host reference (A-gate item 2); the CUDA build on the GB10 still passes its tests; the report lists exact versions, the build line, the `-O2` finding and what `hipify-clang` could not translate.

### `p1-hip-gemm-gate`: the GEMM half of the A-gate

After `p1-hip-build` (it uses lane 1's flags finding). With `bench/amd/vg.hip`, `bw.hip` and `wmma.hip`: every GEMM shape of the served 27B model (projection, head, drafter) at 1 and 16 rows, NVFP4 and FP8, timed only while the runner is idle, at the 140 W setting (the WMMA rate probe tells 120 from 140 W). **Done when**: A-gate items 1 and 3 are answered with a table (GB/s per shape at 1 and 16 rows, the 16/1 ratio, M1 reproduced within 5% or the difference explained) and item 5's decode half is restated with a range; a decode estimate under 40 tokens/s is reported with the stop recommended, one between 40 and 50 as a decision for the owner.

### `p1-hip-portable-kernels`: the rest of W0's kernels

After `p1-hip-build`; may run beside `p1-hip-gemm-gate` (correctness only: it never times). Hipify `elem.cuh`, `kernels.cu`, `kernels.cuh`, `draft_kernels.cu`, `model.cu` and `sparse_head.cu` (hipcub; its `mma.sync` stage rewritten under the determinism rule), and turn `launch.cuh`'s programmatic launches into plain ones behind the backend interface. **Done when**: `tests/kernel_tests.cu` and `tests/mk_tests.cu` pass on the box from the HIP build, every kernel bit-identical across two runs; the CUDA build and its tests on the GB10 unchanged; the report states the time taken per file (it calibrates W0's estimate).

The A-gate's remaining items (the attention probe at 32K, the session-0 and kill probes, prefill's half of item 5) and the loader on `MapViewOfFile` are the next lanes, planned when these three report.
