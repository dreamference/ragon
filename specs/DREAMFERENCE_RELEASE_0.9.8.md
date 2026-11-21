# Ragon 0.9.8 "Illuyanka" — release notes

Version 0.9.8, cut day 2026-11-20, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it. Releases carry a codename from 0.9.4 on (dragons); this one is Illuyanka.

Range: `git log v0.9.7..origin/main`, read at b4670fb29 on 2026-11-19 (112 commits). There is no new engine image: the 0.9.7 image stays pinned (`ragon-serve@sha256:f4391e2e`, ragon-engine 46ab4259). Everything in `specs/DREAMFERENCE_RELEASE_0.9.7.md` still applies.

---

## Ragon 0.9.8 "Illuyanka"

**Ragon 0.9.8 makes the Signal bridge quiet: it now sends one new message per progress step instead of editing a message, so nothing flickers on your phone.** The engine is unchanged; update the client and keep the 0.9.7 image.

### What you will notice

- **Signal sends new messages, not edits.** A long turn now gets one progress message after about 20 seconds (none if the answer is already arriving), then the answer. Editing a status message stays available: set `delivery = "edit"` or run `ragon signal delivery edit`. A bridge you already installed picks up the new default; `ragon signal delivery messages` sets it back. Telegram and Matrix deliver as before, but on a node without accounts (the default) 0.9.8 dropped the Telegram and Discord pairings and refused paired phones on every bridge; 0.9.8.1 fixes this (see `specs/DREAMFERENCE_RELEASE_0.9.8.1.md`).

### Measured

**SWE-bench Verified, reference set on the 0.9.7 image.** The same image, run repeatedly per slice: slice 1 six runs (25 to 30 of 39, mean 27.5), slice 2 five runs (64 to 73 of 106, mean 68.6), slice 3 four runs (70 to 76 of 106, mean 72.8), slice 4 four runs (67 to 74 of 106, mean 70.3); the set of four slices averages 239, and the sum of each slice's lowest and highest run is 226 to 253. Single runs differ from each other by several tasks, so no slice is claimed to be better or worse than another run or an earlier build beyond that spread. This set is the quality band the Windows and Mac ports are judged against.

**RBRB, week 2026-W47 so far:** 10.3 % of generated development tokens were Ragon's (721,060 of 7.0 million), against the 7 % gate for this cut: the gate passes. 86 of 145 attempted Ragon tasks landed, 0 reverts. Source: `docs/rbrb.json` and `rbrb-gate --week 2026-W47`; the week closes on Sunday and its final value replaces this one.

### Since 0.9.7

- **The platform quality specification.** Ragon on another platform is judged on the same quality, not the same results: the Windows and Mac ports are compared with the GB10 by benchmark bands, each band built from N = 5 reference runs on one image, a result below the band is a miss, and speed is reported apart from quality. The reference set above is where the bands come from.
- **RBRB chart.** It now shows the running week so far, the landed Ragon tasks per week, a public quality band for reverts, and the target.
- **Ragon-made cleanups.** 72 of the 112 commits since 0.9.7 were landed by Ragon through the RBRB routing: typing modernisation (`Optional[X]` to `X | None`), test coverage for modules that had none, named constants for magic values, output through one helper, docstring and line-length fixes. They change no behaviour, passed the same tests and land gates, and none was reverted.
- **Night Shift on Windows.** A task file the system refuses to rename for a moment is retried.
- **Docs.** The messenger pages describe the delivery setting.

### Windows engine port

The Windows engine port (HIP) passed its decode gate and its first speed lane.

### Coming in 0.9.9 (2026-12-01)

User accounts on the node: you sign in or create an account at first start, the same way Codex's login works against the node. API keys for other tools. Phones pair with your account. Without accounts, phones pair again in 0.9.8.1 with the 0.9.7 commands (see `specs/DREAMFERENCE_RELEASE_0.9.8.1.md`).

### Known limits

The 0.9.7 engine image is unchanged, so its limits are too: FP8 KV cache stays off by default and the 24-row speculative tree stays off. The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on. The accounts code is present but off: the node advertises accounts only when `accounts.enabled` is set, so the launcher asks for no sign-in.

---

**Checklist:** not part of the release text.

- [ ] Engine image pin on main is still `f4391e2e`.
- [ ] SWE reference set: add the slice 3 and slice 4 fifth runs when graded (not stated above).
- [x] RBRB W47 figures re-read from `rbrb-gate --week 2026-W47` on the cut day (re-read 05:45 UTC).
- [ ] The dry run of the chain is green.
