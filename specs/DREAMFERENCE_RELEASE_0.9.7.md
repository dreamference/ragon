# Ragon 0.9.7 "Vritra" — release notes

Version 0.9.7, cut day 2026-11-17, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. Releases carry a codename from 0.9.4 on (dragons); this one is Vritra.

Range: `git log v0.9.6..origin/main`, read at da9d6062f on 2026-11-16 (114 commits). The engine is new: ragon-engine 46ab4259, the image pinned by digest (`ragon-serve@sha256:f4391e2e`). Everything in `specs/DREAMFERENCE_RELEASE_0.9.6.md` still applies.

---

## Ragon 0.9.7 "Vritra"

**Ragon 0.9.7 ships a new engine image with a fan-out policy that is 1.4 to 1.5 times faster at 32 forks with identical output, and a fair engine-versus-SGLang table measured on it.** If you are on 0.9.6, update the client and let the node pull the new image.

### Measured

**Engine against SGLang.** The 0.9.7 engine image (engine 46ab4259, sparse head confirmed in every serve log) and production's SGLang image, same checkpoint and drafter, one box, one GPU. Two runs per cell in the order A-B-B-A; the spread is the difference between the two runs over their mean.

| Cell | ragon-serve | SGLang | engine / SGLang |
|---|---|---|---|
| One stream, warm decode tok/s | 54.6 (spread 0.2 %) | 44.1 (1.8 %) | 1.24x |
| Two streams, pooled tok/s | 40.2 (0.5 %) | 32.6 (3.6 %) | 1.23x |
| Four warm streams, slowest stream tok/s | 36.9 (1.5 %) | 32.1 (22.4 %) | 1.15x |
| Four warm streams, pooled tok/s | 42.2 (0.8 %) | 38.9 (4.3 %) | 1.08x |
| 32 forks x 32K, wall tok/s, SGLang launched for equal concurrency (33 running requests) | 241.5 (1.7 %) | 162.9 (7.5 %) | 1.48x |
| 32 forks x 32K, wall tok/s, SGLang's production launch (8 running requests) | 241.5 (1.7 %) | 100.8 (10.3 %) | 2.40x |
| One stream at 25.7K tokens, step time (ms, lower is better) | 98.1 (0.0 %) | 107.5 (0.0 %) | 9 % shorter |
| One stream at 25.7K tokens, decode tok/s | 44.0 (0.0 %) | 33.9 (0.0 %) | 1.30x |

The 32-fork row is shown twice on purpose: against SGLang launched for 33 concurrent requests the engine is 1.48x, against SGLang's production launch, which admits 8 at a time, 2.40x. SGLang's four-stream slowest-stream cell has a 22 % spread, so its 1.15x is the least certain row. Two rows favour SGLang: on four streams the engine's worst time to a first token was about 57 s against SGLang's 25 s, and 12 of 16 turns were served warm against 15 of 16, a known case of partial prefix reuse that is not fixed in this release. These are this machine's numbers for comparing engines, not a promise for yours.

**The fan-out policy, before and after, in one session.** 32 forks x 32K, the 0.9.6 image against the 0.9.7 image on the same box and shape: decode span 192 to 287 tok/s (1.49x), wall-clock 171 to 242 tok/s (1.41x, wall 18.7 to 13.3 s). Time to a first token is unchanged (2.24 s and 2.20 s). Greedy output is byte-identical on 16 of 16 turns (5,271 decoded tokens and 782 verify passes each, 0 errors).

**SWE-bench Verified, four-slice set on the 0.9.7 image:** 0.9.7: 241 (spread 232-248); 0.9.6: 232-236. The 0.9.7 set sits inside the spread of earlier sets and is not claimed as a gain; single runs differ by 5 to 8 gross flipped tasks per 106, and no slice is a headline score. Two session slots, 60-minute task limit, 0 starved.

**RBRB, week 2026-W46:** 8.0 % of generated development tokens were Ragon's (912,892 of 11.4 million; 80 of 210 commits were made by Ragon), 0 reverts. The gate for this cut was 2 %. Source: `docs/rbrb.json`.

### Since 0.9.6

- **A new engine image: ragon-engine 46ab4259, `ragon-serve@sha256:f4391e2e`.** Its fan-out policy has two levers, both exact and on by default: the full drafter head above 6 drafting forks, and no drafter-off on wide verifies. All four review gates passed: greedy identity 16 of 16 in every pair against the 0.9.6 image, four-stream identity with equal server counters, one-stream step time within +0.05 ms of 0.9.6, and a smoke run of 10 tasks with 0 timeouts and 0 starved.
- **RBRB follows the share.** The ratchet now tracks the share of Ragon's tokens: the next target is last week's share minus 2 points, with a floor of 2 %. The README section sits just after the intro, and its chart draws the share and the revert-rate quality band. The weekly point is written by `--ingest`; the spec adopts four more amendments.
- **First run in one command.** `ragon-admin init` does the first run's steps, each tested with mocks; the launcher waits for the model server with a progress line and then says how to start it.
- **Client node pick.** With several coding nodes and none remembered, a session uses the least loaded one; the session's node is exempt from a proxy and reaches print mode's child.
- **Night Shift.** A finished task can be retried, `list --all` shows them all, the report opens with a summary block (counts, one next command per task), and the cards show the retry and apply commands as text. The 1.0 plan is in `docs/dev`.
- **Adapter training, first stages.** The data stage (first-parent task candidates and the hold-out split), the generate stage (graded best-of-k rollouts of Ragon on the served base) and the eval stage (one report for E1 to E5) are in; the plan states its throughput assumption and sync scope.
- **Node monitoring.** `ragon-admin monitor status`, a `/status` page with inline strip charts and its JSON; the page shows an empty-state line when there are no samples.
- **Logging.** Every `docker run` takes the bounded log options from one helper; the desktop main process's console lines pass the redaction rules.
- **`ragon-code` asks once** before indexing a repository over 1 GB, and remembers the answer in `ragon-code.toml`.
- **The messenger live test** judges the latest release in its own prefix and retries a model-sample check once; exit code 3 clears the release marker.
- **Lint and typing cleanups.** The ratchet dropped from 999 to 760 lint findings and the mypy findings of the runner and config modules reached zero (named constants, `_say` helpers instead of prints, helpers split out of long functions). Many were written by Ragon's local model through the RBRB routing, passed the same tests and land gates, and none was reverted. They change no behaviour.
- **Docs.** Remote access has a connection check and troubleshooting; the node monitoring page describes what main has.

### Windows engine port

The Windows engine port (HIP) passed its first gates: the first kernels are byte-identical, GEMM is within the bar and the decode estimate is inside the band.

### Known limits

FP8 KV cache stays off by default. The 24-row speculative tree stays off. The four-stream first-token tail and the 12 of 16 warm turns above are known. The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on.

---

**Checklist:** not part of the release text.

- [ ] Engine image pin on main (`f4391e2e`, da9d6062f).
- [ ] The 05:30 dry run of the chain is green.
