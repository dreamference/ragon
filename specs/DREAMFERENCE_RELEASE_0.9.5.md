# Ragon 0.9.5 "Nidhogg" — release notes

Version 0.9.5, cut day 2026-11-11, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. Releases carry a codename from 0.9.4 on (dragons); this one is Nidhogg.

Range: `git log v0.9.4..origin/main`, read at f61f19f on 2026-11-11. The engine image is new: ragon-engine 47277016, which is 0.9.4's engine 2f8062b3 plus the session-protect fix (engine main on 2026-11-11), built with 0.9.4's recipe (`scripts/build_ragon_serve_image.sh`, grammar library compiled in) and pinned by digest. Everything in `specs/DREAMFERENCE_RELEASE_0.9.4.md` still applies.

---

## Ragon 0.9.5 "Nidhogg"

**Ragon 0.9.5 is 0.9.4 with an engine that admits a new conversation when every session slot is taken, plus a configurable log level and a Codex log store that no longer keeps your prompts, and it publishes the full SWE-bench check of the 0.9.4 engine.** If you are on 0.9.4 you need nothing but the client download.

### Since 0.9.4

- **A new conversation could wait behind recently used sessions when all session slots were taken; the engine now admits it.** ragon-serve does not evict a session used in the last 30 seconds (`--session-protect-seconds`), and production runs four slots. When all four belonged to conversations that kept coming back, as agents and their sub-agents do, a new conversation could wait behind them until its client timed out. The engine now admits it: a conversation that has waited the protection window takes the least recently used slot. With four agents busy and a fifth conversation arriving, the fifth is admitted after one protection window (about 32 seconds); before, it waited until the agents stopped and their protection ran out (about 140 seconds in the same test). The ragon-serve image is built from engine 47277016 with the same recipe as 0.9.4's.
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

Timeouts caused by the session-protect stall (a task that never got a first reply) are counted apart from ordinary 60-minute timeouts: slice 1 had 3 timeouts, one of them starved; slice 3 had 4 timeouts, of which one (django-16082) is consistent with starvation by inference only, the engine trace carrying no task id; slice 2 had 4 timeouts and slice 4's are in its report, neither checked per task for starvation. The 0.9.1 night engine has the same rule, so this is not new; 0.9.5's engine fixes it (above).

The same model served by ragon-serve answers one agent at about 64 tokens/s against 51 on SGLang, and four agents at 31–67 tokens/s each against 13 aggregate on SGLang, with byte-identical outputs. SGLang stays ahead only at wide speculative fan-out.

These are this machine's numbers for comparing configurations, not a leaderboard score.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on.
---
