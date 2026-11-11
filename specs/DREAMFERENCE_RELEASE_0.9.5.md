# Ragon 0.9.5 "Nidhogg" — release notes

Version 0.9.5, cut day 2026-11-11, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. Releases carry a codename from 0.9.4 on (dragons); this one is Nidhogg.

Range: `git log v0.9.4..origin/main`, read at f61f19f on 2026-11-11. The engine image is the one 0.9.4 shipped: the session-protect fix (a new conversation could wait behind recently used sessions) had not landed on engine main when this text was written, so it is not claimed here and is listed under Known limits. Everything in `specs/DREAMFERENCE_RELEASE_0.9.4.md` still applies.

---

## Ragon 0.9.5 "Nidhogg"

**Ragon 0.9.5 is 0.9.4 plus a configurable log level and a Codex log store that no longer keeps your prompts, and it publishes the full SWE-bench check of the 0.9.4 engine.** If you are on 0.9.4 you need nothing but the client download.

### Since 0.9.4

- **The log level is configurable.** `log_level` is read from the constructor, then `RAGON_LOG`, then the config file, then defaults to `info`; the Rust side (`ragon-log`, `log.env`) reads the same spec grammar, checked against one shared vector file.
- **The Codex log store keeps prompts and reply text out.** The submission debug line logs the id and kind instead of the payload, and the store's filter caps the TUI's reply-text trace at DEBUG. The audit of the store is written down in the developer docs.
- **SWE-bench runs name the missing image's box.** When a built image is absent, the error names the box that has it, and the night checks the built images before arming.
- **Upkeep.** Code-owner entries for the new files and tests for the log-level tiers; nothing else changes behaviour.

### Measured

The 0.9.4 engine (integration build 7d831bf) was run on the four slices of the 357 validated SWE-bench Verified instances on 10 and 11 November, one engine session per agent, against two baselines: the 0.9.1 nights and the 0.9.0 weekend run. These are single runs; two runs of the same binary on the same box differ by 5 gross flipped tasks per 39, and the two baselines differ from each other by 13 to 15 flips per 106.

| Slice | 0.9.4 engine | 0.9.1 night | 0.9.0 weekend |
|---|---|---|---|
| 1 (39 tasks, four runs) | 28, 26, 27, 30 (mean 27.75) | 28 and 24 | 30 |
| 2 (106) | 69 | 67 | 70 |
| 3 (106) | 69 | 73 | 76 |
| 4 (106) | 75 | 69 | 72 |

Slice 3 is 4 below the 0.9.1 night and 7 below the weekend, inside the spread between the baselines themselves; slice 4 is above both. The sum of the four slices is 241 of 357 for the 0.9.4 engine (one run per slice, slice 1 as 28), against 237 for the 0.9.1 nights (slice 1 as 28) and 248 for the 0.9.0 weekend. It is a sum of slices with the spread above, not a headline score.

Timeouts caused by the session-protect stall (a task that never got a first reply) are counted apart from ordinary 60-minute timeouts: slice 1 had 3 timeouts, one of them starved; slice 3 had 4 timeouts, of which one (django-16082) is consistent with starvation by inference only, the engine trace carrying no task id; slice 2 had 4 timeouts and slice 4's are in its report, neither checked per task for starvation. The 0.9.1 night engine has the same rule, so this is not new.

The same model served by ragon-serve answers one agent at about 64 tokens/s against 51 on SGLang, and four agents at 31–67 tokens/s each against 13 aggregate on SGLang, with byte-identical outputs. SGLang stays ahead only at wide speculative fan-out.

These are this machine's numbers for comparing configurations, not a leaderboard score.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on. **When every session slot is taken (production runs 4), ragon-serve does not evict a session used in the last 30 seconds (`--session-protect-seconds`), so a new conversation can wait behind them until its client times out.** It predates 0.9.4 and is not fixed in 0.9.5.
---
