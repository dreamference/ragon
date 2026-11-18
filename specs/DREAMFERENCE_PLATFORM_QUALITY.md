# Ragon on every platform: the same quality, not the same results

**Status:** The principle was decided by the owner on 2026-11-18 (§1). The bands, the number of reference runs and the gate's scope are proposed, each with a default, in §7. This spec replaces the cross-backend exactness gate (L3) as a release gate for the Windows node (HIP), the Mac (Metal) and every later backend. It does not edit the specs it supersedes in part: their replacement text is in Appendix A, for a follow-up docs row.

**Builds on:**
- [DREAMFERENCE_WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §5.3 to §5.5 and §9, and [DREAMFERENCE_MAC_NODE](./DREAMFERENCE_MAC_NODE.md) §5 and §8: the node specs whose exactness rows this spec replaces;
- the engine repository's METAL spec §2 (the L1, L2 and L3 levels), VALIDATION §13 (the held-out perplexity mix and the quantisation gate), MILESTONES §14 (the agreement study) and the engine backend spec §7 (HIP);
- [DREAMFERENCE_RELEASE_1.0_SCOPE](./DREAMFERENCE_RELEASE_1.0_SCOPE.md), D1: the 1.0 development gate on a 100-instance slice;
- [DREAMFERENCE_RBRB](./DREAMFERENCE_RBRB.md) §0.1, §0.6 and §9: quality first, every attempt recorded, gates changed only by the owner;
- [docs/dev/swe-bench.md](../docs/dev/swe-bench.md): the slices, the verdict script and the run log.

## 0. Decisions

| # | Decision | Where |
|---|---|---|
| P1 | Every platform serves the same model to the same quality bar. Its arithmetic is its own: precision, kernels, activation quantisation and drafter may differ. Tokens and logits need not match another platform's. | §1 |
| P2 | The reference platform is the GB10 (CUDA). Every other platform is compared with it by benchmarks, at the same release, on the same slice lists and container images. | §2, §5 |
| P3 | Five checks make up the quality table: SWE-bench Verified slice 1, a perplexity ratio, drafter acceptance, long-context retrieval and the tool-call format. A quality miss blocks the platform's release; a speed miss is a note under the decode rule. | §2 |
| P4 | Determinism, eager equal to graph, speculative equal to non-speculative and kernel tests against the model's own reference arithmetic stay strict inside each platform. They are engineering checks, not cross-platform gates. | §3 |
| P5 | Agreement with CUDA's logits (KL, top-1 flips, flip gaps, perplexity agreement) stays only as an optional diagnostic with no limit. Emulating another backend's arithmetic bit for bit is not a goal. | §4 |
| P6 | One comparison table per release, in the release notes and the docs, with every run reported. | §5 |
| P7 | The blocked HIP exactness row closes. The HIP build moves to the quality table; slice 1 served by the Windows node is the next lane. | §6 |

The owner's ruling, verbatim:

```
The goal is for windows to have the same quality not the same results. ... This should also be
true for other platforms like mac. Same quality not same results. This means that you should
compare mac to windows to dgx by comparing the benchmarks. We want each implementation to be
hardware optimized for each platform using same quality not same results approach.
```

## 1. Principle

One model, one quality bar. The checkpoint and its model id are the same on every platform; what a platform does with them is chosen for its hardware. A platform may:

- multiply in the precision its matrix units offer (FP16 or BF16 where the GB10 uses block-scaled FP4 or FP8, or the reverse);
- quantise activations differently, or not at all, in the prefill and in the FFN;
- use its own kernels, summation orders, recurrence forms (chunked or sequential) and transcendental functions;
- use a drafter shape or verify shape suited to its own step time.

What it may not do is serve a worse model. "Worse" is measured, not argued: the quality table of §2 decides it, against the GB10 at the same release. A format change that moves quality further than numerical differences do (a re-quantisation of the weights, a lower-bit format) still goes through the engine's LOWBIT ladder as before; this spec governs a platform's arithmetic, not a change of model.

Why benchmarks and not logits. On 2026-11-18 the HIP build passed every within-backend check and failed the cross-backend gate on two of its lines only: mean KL 0.004572 nats against a limit of 0.0008, and a largest flip gap of 1.8029 nats against 1.75. Two HIP builds that differ in one prefill kernel are already 0.003482 nats apart, and every CUDA-against-HIP greedy divergence was a swap of the top two tokens, the largest at a gap of 0.425 nats. On CUDA itself, BF16 against FP8 KV gives a KL of 0.0062 nats and parts in 10 of 16 agentlong turns. The 0.0008-nat line was set as twice production's own warm-against-cold spread on one machine; reaching it on another platform means reproducing the GB10's prefill bit for bit, which costs that platform its own fast path and says nothing about whether the answers are good.

## 2. The quality table

### 2.1 Reference runs and bands

- **The reference** is the GB10 serving the release's engine and checkpoint, with the release's container images.
- **N, the reference's runs.** A band is built from the reference's last N runs of a check on one image, N ≥ 3 runs; the default is N = 5 runs (§7, Q1). The band is the minimum and maximum of those runs.
- **A new reference image** starts a new band. Until N runs exist on it, no platform is judged on that image; a platform release that cannot wait is judged on the previous image, with the platform running that same image.
- **Platform runs.** The platform runs each run-to-run varying check three times on the reference's image; the median of the three counts.
- **One-sided.** Only a result below the band is a miss for the slice and the format checks; a result above it is reported and is not a miss. The perplexity ratio is two-sided (§2.3).
- **A screen, then a test.** A single result below the band minimum is not by itself proof of a loss: with equal quality, one platform run falls below the minimum of N reference runs about once in N + 1 tries (one in six at N = 5 runs), and the median of three falls below it about once in nine. So a slice result below the band is decided by `scripts/swe_bench_verdict.py`, the paired one-sided test of the 1.0 development gate, with all three platform runs as the candidate and all N reference runs as the baseline. The slice check misses only when that verdict is "worse".

### 2.2 The checks

| # | Check | Dataset | Command | Runs | Pass band | A miss means |
|---|---|---|---|---|---|---|
| Q1 | SWE-bench Verified, slice 1 | `docs/dev/swe-bench/verified357-1.txt`, 39 tasks | `ragon-admin swe-bench run --subset docs/dev/swe-bench/verified357-1.txt --eval --name <platform>-<image>-<k>`, run by a GB10 as harness with the platform's node serving (the Windows spec's G20 arrangement; how the run is pointed at the node under test is **TO VERIFY**); then `scripts/swe_bench_verdict.py --slice docs/dev/swe-bench/verified357-1.txt --baseline <reference runs> --candidate <platform runs>` | Reference N (default 5); platform 3 | Median of the platform's three resolved counts ≥ the band minimum; if below, the verdict is not "worse" (§2.1) | The platform's release is blocked |
| Q2 | Perplexity ratio | VALIDATION §13's held-out mix | Work item: no perplexity runner for a non-CUDA backend is in the tree today (**TO VERIFY** against the engine's bench tools) | 1 each (greedy, teacher-forced, deterministic by §3) | Platform ÷ reference in [0.99, 1.01] (default, §7 Q2) | The platform's release is blocked |
| Q3 | Drafter acceptance | agentlong, one stream, the release's verify shape | Tokens per step from the decode benchmark that reports tok/s (the invocation is **TO VERIFY**) | Reference N; platform 3 | Note, not a gate (default, §7 Q4): within the reference's band is the target; under 75% of the reference's mean requires a diagnostic of the drafter path against its reference arithmetic | A note in the release's table |
| Q4 | Long-context retrieval | A needle at 120,000 tokens | `scripts/long_prompt_probe.py needle --tokens 120000 --depths <the reference's depths> --host <node> --out <file>` | 1 per depth | Every depth the reference retrieves, the platform retrieves | The platform's release is blocked |
| Q5 | Tool-call format | The Q1 runs' sessions; the engine's structured-output server tests | Work item: a counter of tool calls that parse against their tool's schema, over all tool calls, read from the run's session logs | From Q1's runs | Platform's parse rate ≥ the lowest of the reference's runs in the band; the server tests pass | The platform's release is blocked |

Example band, from the published run log (`docs/swe-bench-runs.json`): build 0.9.4 on the GB10 resolved 28, 26, 27 and 30 of 39 slice 1 tasks over four runs on 2026-11-10 and 2026-11-11, a band of 26 to 30 tasks. Bands in a release's table are computed by the same rule from the release's own runs.

### 2.3 Why these bands

- **Slice 1.** Its spread on one build is four tasks of 39, about ten points: the slice catches a large loss and nothing finer. Its value is that it measures the thing users get, end to end, with the platform's own prefill, decode, drafter and tool-call formatting.
- **Perplexity, ±1%.** The finer instrument. Numerical differences measured so far move it by 0.09% (HIP BF16, ratio 1.000911) to 0.22% (HIP FP8, 1.00224). The smallest known quality-changing shift, a weight format change, moves it by 4 to 9% (LOWBIT §3), and VALIDATION §13's quantisation gate accepts up to 2%. A band of ±1% sits four times above the measured noise and below the quantisation gate, so it passes a platform's own arithmetic and catches a wrong kernel long before the slice could see it. It is two-sided: a ratio far below one points to a different model or different data, not a better platform.
- **Acceptance.** Speculation with exact rejection does not change the output distribution, so acceptance is speed, not quality: a lower acceptance makes a platform slower, never worse. It is kept in the table because it explains a speed miss. Measured on 2026-11-18: HIP 5.33 tokens per step against the GB10's 6.74, 79% of it.
- **Retrieval at 120,000 tokens.** Long sessions are the product's main load, and a precision change in attention or the KV cache shows first at long range. The needle tool exists and is deterministic per platform.
- **Tool-call format.** An agent that cannot parse its own tool calls fails tasks for reasons a perplexity number does not show. The reference's lowest rate over its band runs is the floor.

### 2.4 Quality misses and speed misses

- **Quality** (Q1, Q2, Q4, Q5): a miss blocks the platform's release. RBRB §9 applies: a band is changed only by the owner.
- **Speed** (decode tok/s, Q3): a miss is a note under the decode rule. On agentlong, greedy, one stream: 50 tok/s or more passes; from 40 to 50 tok/s the miss is recorded as a DECISION and the chain goes on, with the owner able to veto; under 40 tok/s is a STOP (owner, Q36). Measured on 2026-11-18: HIP speculative decode 53.5 tok/s with BF16 KV and 48.0 tok/s with FP8 KV.

### 2.5 Relation to the 1.0 development gate

RELEASE_1.0_SCOPE D1 judges a release of the reference itself: a 100-instance slice, the candidate not worse than the previous baseline at p = 0.05 with a margin of three instances. It stays the GB10's gate. The quality table judges a platform against the GB10 at the same release; it does not replace D1, and a platform cannot pass while the GB10 fails D1.

## 3. Checks that stay strict within each platform

These are engineering checks. Each platform runs them against itself, on every build, with no tolerance where the rule is equality. They catch races, uninitialised memory, a graph that is not the eager path and a speculative path that is not exact. They are not compared across platforms.

| Check | Rule | How |
|---|---|---|
| Determinism | Two greedy runs give identical bytes | `ragon-run --greedy-out` twice, compared |
| Eager equals graph | The captured or pre-encoded step gives the eager step's bytes | `ragon-run --graph-check` (CUDA, HIP); `--icb-check` (Metal) |
| Speculative equals non-speculative | Every agentlong turn identical, chain, tree and lookup | `ragon-run --spec-check` |
| Prefill equivalence | A chunked or resumed prefill equals one from scratch | `ragon-run --prefix-check`, `--chunk-check` |
| Batch invariance | A sequence batched equals itself alone | `ragon-run --batch-check` |
| Kernel tests | Each kernel against the model's own reference arithmetic (an FP32 or FP64 host reference) within a stated tolerance; each kernel bit-identical across two runs | The backend's kernel test suite (`ctest` on HIP) |

Measured on 2026-11-18, HIP: graph-check and prefix-check pass; speculative equal to non-speculative on 16 of 16 agentlong turns; run one equal to run two on 16 of 16 turns; kernel tests 23 of 23.

## 4. No longer a gate

- **Cross-backend agreement.** The L3 measures (top-1 flips against a Fisher threshold, flip gaps, top-5 agreement, mean KL, the identical-turn share, perplexity agreement within [0.999, 1.001]) stay available as an optional diagnostic for a platform's engineers. They carry no limit, block nothing and appear in no release table.
- **Bit-identical emulation of another backend.** No platform is asked to reproduce another backend's activation quantisation, summation order or recurrence form in order to match its logits. Work towards that end is not planned and not reviewed as progress.
- **Kernel bytes equal to CUDA's.** A kernel's test compares it with the model's reference arithmetic, not with the GB10's output bytes; a fixed-order kernel that happens to give CUDA's bytes is not required to.

## 5. Comparison method

One table per release, in that release's notes and in the docs' benchmarks page, one row per platform:

| Platform | Image / commit | Slice 1 (resolved of 39, each run) | Reference band | Perplexity ratio | Acceptance (tokens per step) | Decode tok/s |
|---|---|---|---|---|---|---|
| GB10 (CUDA), reference | the release's image and engine commit | every run of the N | min–max | 1.000 | mean, min–max | agentlong, greedy |
| Windows node (HIP) | same image; HIP commit | each of three runs, median marked | the GB10's | ratio | mean | agentlong, greedy |
| Mac (Metal) | same image; Metal commit | each of three runs, median marked | the GB10's | ratio | mean | agentlong, greedy |

Rules:
- The same slice lists and the same container images on every platform; a platform that cannot use the reference's image says so in its row and is not compared until it can.
- Every run is reported, including a run that was repeated, with its reason (RBRB §0.6). No run is dropped for being low, and none is chosen for being high.
- Q4 and Q5 are reported as pass or fail with their numbers in a note under the table.
- Numbers in the table are those the release's record writes (`scripts/release_benchmark.py record`); nightly results stay internal, as the docs' publication rule says.

## 6. Transition

- **The specs this replaces in part** are the Windows node spec (§5.3, §5.4's decision point, §5.5's gates, §9), the Mac node spec (§5's exactness row, §8) and, in the engine repository, the METAL spec (§0 to §3, §5, §7 to §9) and the engine backend spec §7. Their replacement text is in Appendix A. This job does not edit them; a follow-up docs row applies the appendix.
- **The HIP exactness row**, blocked on 2026-11-18 on L3's KL and flip-gap lines, closes with this spec. The HIP build's within-platform checks (§3) all pass; its quality is now decided by the table of §2.
- **The next lane** is slice 1 served by the Windows node: three runs on the GB10's current image, harnessed by a GB10, against the GB10's band on that image, followed by Q2 and Q4 as their runners exist.
- **The Mac** enters the table at its M1 milestone with the same checks.

## 7. Open questions for the owner

| # | Question | Default |
|---|---|---|
| Q1 | N, the number of reference runs in a band, and how a platform's runs are judged | N = 5 runs on one image; the platform runs three times, its median is screened against the band minimum, and a result below is decided by the paired verdict (§2.1). A lone run judged against the minimum of N runs would fail about once in N + 1 tries at equal quality, so a single run is never the decision. |
| Q2 | The perplexity band | [0.99, 1.01]: four times the measured numerical noise (0.09 to 0.22%), half the quantisation gate's 2% (§2.3) |
| Q3 | Slice 1 alone, or slices 1 and 2, for the 1.0 platform gate | Slice 1 alone (39 tasks, one night). Slices 1 and 2 (145 tasks) would narrow the band at the cost of a second night per platform run; D1's 100-instance slice stays the GB10's own gate. |
| Q4 | Is the acceptance band a gate or a note | A note: exact rejection keeps the output distribution, so acceptance is speed. Under 75% of the reference's mean triggers a diagnostic of the drafter path. |

## Appendix A. Replacement text

For the follow-up docs row. Each entry is one replacement: the location, the current text (quoted, with "…" where unchanged text is left out) and the new text. Counts: Windows node spec 11, Mac node spec 4, METAL spec 17, engine backend spec 2.

### A.1 Windows node spec (`specs/DREAMFERENCE_WINDOWS_NODE.md`)

Kept unchanged: §5.3's "Within a build" bullet and §5.4's item 2 (`topk.cu` bit-identical across two runs); both are within-platform checks (§3).

**W-1.** §5.2 table, row "Served NVFP4 + FP8 projections", column "Quality route".
```
Old: The same checkpoint, loader and model id. Metal's L3 gate against CUDA (§5.3)
New: The same checkpoint, loader and model id. PLATFORM_QUALITY §2's quality table against the GB10
```

**W-2.** §5.3, heading.
```
Old: ### 5.3 Exactness and quality
New: ### 5.3 Within-platform exactness, and quality against the GB10
```

**W-3.** §5.3, bullet "Against the GB10".
```
Old: - **Against the GB10:** METAL's L3 gate verbatim for the same checkpoint: 54 prompts, Fisher threshold
     85 flips, flip gap ≤ 1.75 nats, top-5 100%, mean KL ≤ 0.0008 nats, perplexity ratio in [0.999, 1.001],
     acceptance within spread.
New: - **Against the GB10:** the same quality, not the same results (PLATFORM_QUALITY). The release gate is
     PLATFORM_QUALITY §2's quality table: SWE-bench slice 1 within the GB10's band, perplexity ratio in
     [0.99, 1.01], long-context retrieval and tool-call format at the GB10's rate. Drafter acceptance is a
     note. Logit agreement with CUDA (KL, flips) is an optional diagnostic with no limit (PLATFORM_QUALITY §4).
```

**W-4.** §5.3, bullet "Prefill".
```
Old: - **Prefill** falls under L3's prefill positions; Metal emulates CUDA's activation quantisation for it.
     A prefill format that cannot meet L3 goes to LOWBIT §7's ladder and is the owner's call.
New: - **Prefill** uses the arithmetic that is fastest on this hardware (FP16 WMMA, a chunked or sequential
     recurrence, its own activation quantisation or none). CUDA's activation quantisation is not emulated.
     Its quality is judged by the quality table; a change of the weights' format still goes to LOWBIT §7's
     ladder and is the owner's call.
```

**W-5.** §5.4, the decision point.
```
Old: … with the thresholds that would say no: a whole-step estimate under 50 tok/s on agent output, or a
     prefill ceiling under 500 at 32K, …
New: … with the thresholds that would say no: a whole-step estimate under 40 tok/s on agent output (a STOP;
     40 to 50 tok/s is a recorded DECISION the owner may veto, PLATFORM_QUALITY §2.4), or a prefill ceiling
     under 500 at 32K, …
```

**W-6.** §5.5, row W1, column "Gate".
```
Old: Speculative equals non-speculative on 16 agentlong turns; byte-identical runs; **≥ 50 tok/s agentlong
     greedy**; L3
New: Speculative equals non-speculative on 16 agentlong turns; byte-identical runs; decode by the decode rule
     (**≥ 50 tok/s** passes, 40–50 a recorded DECISION, < 40 STOP); perplexity ratio (PLATFORM_QUALITY Q2)
```

**W-7.** §5.5, row W3, column "Gate".
```
Old: Metal's M5/M6 tests by name; L3 on image prompts
New: Metal's M5/M6 tests by name; the quality table (PLATFORM_QUALITY §2) on the release candidate
```

**W-8.** §9, lead paragraph.
```
Old: … with a report. Thresholds are the study's; they may tighten, never silently loosen.
New: … with a report. Thresholds are the study's; they may tighten, never silently loosen. Quality
     thresholds are PLATFORM_QUALITY's and change only by the owner.
```

**W-9.** §9, row G4.
```
Old: | G4 | Decode, agentlong | **≥ 50 tok/s** greedy; speculative equals non-speculative; two runs byte-identical |
New: | G4 | Decode, agentlong | The decode rule: **≥ 50 tok/s** greedy passes, 40–50 a recorded DECISION, < 40 STOP; speculative equals non-speculative; two runs byte-identical |
```

**W-10.** §9, row G6.
```
Old: | G6 | L3 against the GB10 | §5.3 |
New: | G6 | Quality against the GB10 | PLATFORM_QUALITY §2: Q2, Q4 and Q5 pass; Q3 reported |
```

**W-11.** §9, row G20 and the sentence after the table.
```
Old: | G20 | A SWE-bench night served by the Windows engine and run by a GB10, the node otherwise idle | No
     significant loss by McNemar (LOWBIT §7, rung 4) |
     …
     G20 uses a GB10 as harness, so Windows needs no SWE-bench code: the node only serves. It can run once
     G4–G7 pass.
New: | G20 | SWE-bench slice 1 served by the Windows engine and run by a GB10, three runs, the node otherwise
     idle | PLATFORM_QUALITY Q1: the median within the GB10's band, or not worse by the paired verdict |
     …
     G20 uses a GB10 as harness, so Windows needs no SWE-bench code: the node only serves. It can run once
     G4–G7 pass. Its runs feed Q5 (tool-call format).
```

### A.2 Mac node spec (`specs/DREAMFERENCE_MAC_NODE.md`)

**M-1.** §5, seam table, row "Exactness".
```
Old: | Exactness | The engine's own gates (Metal spec §2, L3 against CUDA); the node gate only checks they were
     run on the release candidate |
New: | Exactness and quality | Within the Metal build, the engine's own checks (PLATFORM_QUALITY §3); against
     the GB10, PLATFORM_QUALITY §2's quality table. The node gate only checks they were run on the release
     candidate |
```

**M-2.** §8, row G4.
```
Old: | G4 | Decode, agentlong | The Metal spec's M3 target; speculative equals non-speculative; two runs byte-identical |
New: | G4 | Decode, agentlong | The decode rule (≥ 50 tok/s passes, 40–50 a recorded DECISION, < 40 STOP), the Metal spec's M3 target recorded against it; speculative equals non-speculative; two runs byte-identical |
```

**M-3.** §8, row G6.
```
Old: | G6 | L3 against the GB10 | Metal spec §2 |
New: | G6 | Quality against the GB10 | PLATFORM_QUALITY §2: Q2, Q4 and Q5 pass; Q3 reported |
```

**M-4.** §8, row G20.
```
Old: | G20 | A SWE-bench night served by the Mac and run by a GB10 | No significant loss (McNemar), the Metal
     spec's M7 |
New: | G20 | SWE-bench slice 1 served by the Mac and run by a GB10, three runs | PLATFORM_QUALITY Q1: the
     median within the GB10's band, or not worse by the paired verdict |
```

### A.3 METAL spec (engine repository, `specs/DREAMFERENCE_RAGON_ENGINE_METAL.md`)

**E-1.** §0, bullet "Exactness".
```
Old: … Across CUDA and Metal, bit identity is not promised; it is replaced by a statistical gate against the
     CUDA engine (§2).
New: … Across CUDA and Metal, neither bit identity nor logit agreement is a goal: the Metal build serves the
     same model to the same quality, judged by PLATFORM_QUALITY's benchmark table against the GB10 (§2).
```

**E-2.** §1, row "The same exactness rules".
```
Old: | The same exactness rules | §2: within the Metal build, the CUDA engine's rules bit for bit; across builds,
     a stated statistical gate. |
New: | The same exactness rules, the same quality | §2: within the Metal build, the CUDA engine's rules bit for
     bit; across builds, PLATFORM_QUALITY's quality table. |
```

**E-3.** §2, heading.
```
Old: ## 2. Exactness across CUDA and Metal
New: ## 2. Exactness within Metal, quality against CUDA
```

**E-4.** §2, row "L2, across Macs", last column.
```
Old: The L3 gate run once per GPU family and per macOS major version; …
New: The quality table (PLATFORM_QUALITY §2) run once per GPU family and per macOS major version; …
```

**E-5.** §2, row "L3, across CUDA and Metal".
```
Old: | L3, across CUDA and Metal | The Metal build against the CUDA engine on the same requests | The same weights
     bit for bit (§3.2), the same quantization points (…), the same algorithms; differences only in summation
     order, matrix-unit internals and `exp`/`rsqrt` implementations. … | The statistical gate below |
New: | L3, across CUDA and Metal | The Metal build against the CUDA engine on the same requests | The same
     weights bit for bit (§3.2); quantization points, algorithms and summation order chosen for Apple's matrix
     units. Greedy outputs may part | PLATFORM_QUALITY's quality table; the agreement measures below as a
     diagnostic |
```

**E-6.** §2, the L3 gate and its five bullets.
```
Old: **The L3 gate** (the reference is the CUDA engine at the release's commit, teacher-forced; …):
     - on the agreement study's 54 prompts …, every flip's gap ≤ 1.75 nats, top-5 agreement 100%;
     - mean KL(CUDA ‖ Metal) ≤ 0.0008 nats, …;
     - perplexity on VALIDATION §13's held-out mix within a ratio of [0.999, 1.001] …;
     - acceptance (tokens per step) on agentlong within the run-to-run spread of the CUDA engine's …;
     - informational, not a gate: the share of agentlong turns whose greedy output is identical …
New: **The release gate** is PLATFORM_QUALITY §2: SWE-bench slice 1 within the GB10's band, perplexity on
     VALIDATION §13's held-out mix within a ratio of [0.99, 1.01] of the GB10's, long-context retrieval and
     tool-call format at the GB10's rate; acceptance on agentlong reported as a note.
     **The L3 diagnostic** (optional, no limit, PLATFORM_QUALITY §4): top-1 flips, flip gaps, top-5
     agreement and mean KL against the CUDA engine's pinned teacher-forced logits on the agreement study's
     54 prompts, and the share of identical agentlong turns.
```

**E-7.** §3.2, "Does it change outputs against CUDA?".
```
Old: … Outputs differ only as L3 allows (§2): summation order and transcendental functions. Quality is
     therefore gated as a numerical port, not as a format change: the L3 gate (agreement, KL, perplexity
     ratio) at M1 and at every release, and one SWE-bench night … at M7, …
New: … Outputs differ by the Metal build's own arithmetic. Quality is gated as a numerical port, not as a
     format change: PLATFORM_QUALITY's quality table at M1 (Q2) and at every release (all checks), …
```

**E-8.** §3.8, the prefill paragraph.
```
Old: … The Metal prefill keeps the same quantization points and emulates the products: activations quantized
     to NVFP4 or FP8 by the same rule, both operands decoded to FP16 (exactly, §3.2), multiplied with FP32
     accumulation, … Each product is the one the block-scaled MMA forms; only the summation order differs. …
New: … The Metal prefill chooses its arithmetic for Apple's matrix units: weights decoded to FP16 (exactly,
     §3.2), activations in FP16 or quantized where that is faster, FP32 accumulation. CUDA's activation
     quantization is not emulated; the quality table judges the result (PLATFORM_QUALITY §2). …
```

**E-9.** §5, the acceptance sentence.
```
Old: Acceptance depends only on the two models' numbers, which L3 keeps equal, so it is taken from the GB10
     unchanged: 6.749 tokens per step on agentlong …
New: Acceptance depends on the two models' numbers on this hardware; the estimate takes the GB10's 6.749
     tokens per step on agentlong … and M2 measures the Mac's own (PLATFORM_QUALITY Q3, a note).
```

**E-10.** §7, row M1, column "Exactness gate".
```
Old: … greedy deterministic across repeats; the L3 gate against the CUDA engine (§2), perplexity ratio included
New: … greedy deterministic across repeats; the perplexity ratio against the GB10 in [0.99, 1.01]
     (PLATFORM_QUALITY Q2)
```

**E-11.** §7, row M2, column "Measurable target".
```
Old: Tokens per step on agentlong within the CUDA engine's run-to-run spread at the same verify shape; …
New: Tokens per step on agentlong recorded against the GB10's band (PLATFORM_QUALITY Q3, a note); …
```

**E-12.** §7, row M4, columns "What" and "Exactness gate".
```
Old: Prefill GEMMs with CUDA's activation quantization emulated; … | `--chunk-check`, `--prefix-check`; the L3
     gate's prefill positions (the first token after the prompt)
New: Prefill GEMMs in the Metal build's own arithmetic (§3.8); … | `--chunk-check`, `--prefix-check`;
     long-context retrieval (PLATFORM_QUALITY Q4)
```

**E-13.** §7, row M6, column "Exactness gate".
```
Old: … prompts token-identical to the Linux build's on image requests; the L3 gate on image prompts
New: … prompts token-identical to the Linux build's on image requests; the engine's server tests on image
     prompts
```

**E-14.** §7, row M7, column "Exactness gate".
```
Old: The L3 gate on the release candidate; the night with no significant loss (McNemar)
New: The quality table on the release candidate (PLATFORM_QUALITY §2), slice 1 included
```

**E-15.** §7, row M9, column "Exactness gate".
```
Old: L1 checks and the L3 gate per GPU family
New: L1 checks and the quality table per GPU family
```

**E-16.** §8, tests table, row `ragon-l3-check`, and §6 risk row "A macOS update changes kernel bits (L2)".
```
Old: | — | `ragon-l3-check`: the L3 gate (§2) against the pinned CUDA reference; … |
     … golden files keyed by OS build; the L3 gate re-run | Standing |
New: | — | `ragon-l3-check`: the L3 diagnostic (§2, no limit) against the pinned CUDA reference; … |
     … golden files keyed by OS build; the quality table re-run | Standing |
```

**E-17.** §9, open question 1.
```
Old: 1. **Cross-platform exactness.** Accept L3 (same weights and quantization points, a statistical gate) as
     the definition of "the same model" across CUDA and Metal, with no attempt at bit identity? *Default: yes.*
New: 1. **Cross-platform exactness.** Decided by the owner on 2026-11-18: the same quality, not the same
     results. "The same model" means the same weights and the quality table of PLATFORM_QUALITY §2; L3 is a
     diagnostic with no limit.
```

### A.4 Engine backend spec (engine repository, `specs/DREAMFERENCE_RAGON_ENGINE_BACKEND.md`, §7 HIP)

**B-1.** §7, the first paragraph's last sentence.
```
Old: The entry points and their contracts are §2 unchanged; the exactness rules of AMD apply within the HIP
     build.
New: The entry points and their contracts are §2 unchanged; within the HIP build the within-platform checks
     of PLATFORM_QUALITY §3 apply bit for bit; against the GB10, PLATFORM_QUALITY §2's quality table decides.
```

**B-2.** §7, bullet "The exactness rule for the HIP backend (coordinator, 2026-11-14)".
```
Old: … every kernel gives identical bytes on every run of the same backend; kernels that move, convert, add or
     reduce in a fixed order give CUDA's bytes; kernels that go through the hardware `exp`, `rsqrt`,
     `sin`/`cos`, `pow` or `log1p` are held to the FP32/FP64 host reference's tolerance across backends. …
New: … every kernel gives identical bytes on every run of the same backend; every kernel is held to the
     FP32/FP64 host reference's stated tolerance. CUDA's bytes are not a requirement for any kernel
     (owner, 2026-11-18, PLATFORM_QUALITY §4). …
```
