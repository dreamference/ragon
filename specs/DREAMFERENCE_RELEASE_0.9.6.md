# Ragon 0.9.6 "Ladon" — release notes

Version 0.9.6, cut day 2026-11-13, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. Releases carry a codename from 0.9.4 on (dragons); this one is Ladon.

Range: `git log v0.9.5..origin/main`, read at ed0d7ec on 2026-11-13 (63 commits). The engine is 0.9.5's: ragon-engine 47277016 with the session-protect fix, the image pinned by digest. Everything in `specs/DREAMFERENCE_RELEASE_0.9.5.md` still applies.

---

## Ragon 0.9.6 "Ladon"

**Ragon 0.9.6 makes the Ragon-Builds-Ragon benchmark (RBRB) public, with its section in the README, its chart and the commit trailers that feed it, and it publishes a fair engine-versus-SGLang table measured on the release image.** If you are on 0.9.5 you need nothing but the client download.

### Since 0.9.5

- **RBRB, the Ragon-Builds-Ragon benchmark, is public.** The README has a section and a chart with its first point (week 2026-W46). The rule it states: quality first, progress second. The spec (`specs/DREAMFERENCE_RBRB.md`) records what is built, where the build differs from the plan, the eight adopted amendments and the seven still open; the data file is `docs/rbrb.json`.
- **Commits say how much of them Ragon wrote.** `as_team commit` takes `--ragon-tokens N` (the `Ragon-Tokens` trailer) and `--made-with ragon` (the `Made-with` trailer), and the counter reads both. The model gate logs one usage line per generation request that carries a job tag: the job, the path and the generated, prompt and cached token counts.
- **The delegated edit tool guards the quality gates.** A task diff that touches the ratchet baselines and their tests, `conftest.py`, the RBRB files or the existing acceptance test is refused; the ruff check fails only on findings the edit introduced; `systemctl --user` falls back to `/run/user/<uid>` when `XDG_RUNTIME_DIR` is unset.
- **35 small cleanups were made by Ragon through the RBRB routing.** Named constants instead of magic values, one-line docstring summaries, pathlib paths, typed mypy findings, and `_say` helpers instead of bare prints, in the night shift, node, web, agent, code-index, models and benchmark modules (and the swe-bench ones). Each was written by Ragon's local model, passed its test suite and the land queue, and none was reverted. They change no behaviour.
- **The test suite is steadier.** Tests for the request deadline and the deadline adapter, asyncio loops woken where a sandbox refuses `send()`, and a stand-in `getent` so no night-script test waits on the host's resolver.
- **Specs.** The Mac node draft (launchd, host safety, Bonjour, install role, forced command, egress audit, Metal seam) and the Codex log store described after its audit (LOGGING §2.3).

### Measured

**Engine against SGLang.** On the pinned release image (engine 2f8062b3, sparse head confirmed in every serve log) and production's SGLang image, with the same checkpoint and drafter on one GPU. Two runs per cell in the order A-B-B-A; spreads are the difference between the two runs over their mean.

| Cell | ragon-serve | SGLang | engine / SGLang |
|---|---|---|---|
| One stream, warm decode tok/s | 54.8 (spread 0.4 %) | 45.0 (10.5 %) | 1.22x |
| Two streams, pooled tok/s | 40.4 (0.8 %) | 32.2 (5.2 %) | 1.25x |
| Four warm streams, pooled tok/s | 42.4 (2.6 %) | 40.3 (7.0 %) | at parity (1.05x) |
| 32 forks, tok/s, SGLang launched for equal concurrency (33 running requests) | 171.9 (0.3 %) | 152.7 (0.7 %) | 1.13x |
| 32 forks, tok/s, SGLang's production launch (8 running requests) | 171.9 (0.3 %) | 97.5 (1.7 %) | 1.76x |
| One stream at 25.7K tokens, step time (ms, lower is better) | 98.1 (0.2 %) | 107.0 (0.5 %) | 8 % shorter |
| One stream at 25.7K tokens, decode tok/s | 44.1 (0.2 %) | 31.4 (2.1 %) | 1.41x |

The four-stream row was first measured unfairly: the harness's warm-up requests were rejected by SGLang, so it re-read 20 to 50K tokens on every turn and looked 50 s slower to a first token. With a harness that SGLang accepts, it matches the engine. The 32-fork row is shown twice on purpose: against SGLang launched for 33 concurrent requests the engine is 1.13x, against SGLang's production launch, which admits 8 at a time, 1.76x. Greedy output is byte-identical to the 0.9.3 engine on 16 of 16 turns. The engine wins at one to four agents and at fan-out. These are this machine's numbers for comparing configurations, not a leaderboard score.

**SWE-bench Verified on the 0.9.5 image** (the engine 0.9.6 ships), two session slots, 60-minute task limit, 0 starved timeouts in every run. Single runs differ by 5 to 8 gross flipped tasks per 106.

| Slice | 0.9.5 image | 0.9.4 image | 0.9.1 night | 0.9.0 weekend |
|---|---|---|---|---|
| 1 (39 tasks) | 28 | 26, 27, 28, 30 | 28 and 24 | 30 |
| 3 (106) | 69 | 77 | 73 | 76 |
| 4 (106) | 69 | 67 | 69 | 72 |

Slice 3 is 8 below the same recipe on the 0.9.4 image the night before (slice 1 shows the four 0.9.4 runs); a repeat of that slice on the same 0.9.5 image ran last night and is not graded yet, so the run-to-run spread on this image is pending. Slice 4 is above the 0.9.4 image and equal to the 0.9.1 night. The slice-1 spread across 0.9.4 runs is 4 flips of 39 (26 to 30), and the earlier baselines differ from each other by 13 to 15 flips per 106; none of these is a headline score.

**The session-protect fix works.** With four agents busy, a fifth concurrent conversation waited 141 s on 0.9.4 and 32 s on 0.9.5, and every run since shows 0 starved.

**RBRB this week:** 1.7 % of generated development tokens were Ragon's (53 landed commits), target 2 %; quality: Ragon's commits passed the same tests and land gates, 0 reverts. Source: `rbrb-gate --week 2026-W46` at the time of writing (127,913 of 7,515,366 generated tokens), below the target.

### Known limits

FP8 KV cache stays off by default: its quality gate passed and the ruling is pending. The 24-row speculative tree stays off, its gain being inside the seed spread. The live messenger test judges a stale install, so its row does not yet say anything about 0.9.6. The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on.

---
