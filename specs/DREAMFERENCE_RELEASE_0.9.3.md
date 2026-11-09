# Ragon 0.9.3 — release notes

Version 0.9.3, cut day 2026-11-10, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

Range: `git log v0.9.2..origin/main`. The text was finalised on 2026-11-09 against `v0.9.2..origin/main` at a08f4dd; the 0.9.2 changes are listed again below as they stand, and the 0.9.1 changes are in `specs/DREAMFERENCE_RELEASE_0.9.1.md`.

---

## Ragon 0.9.3

**Ragon 0.9.3 completes 0.9.2: it carries every 0.9.2 change and adds the x86_64 Linux client and the macOS disk images that 0.9.2 was published without.** Those assets were missing from the 0.9.2 release page, so it listed fewer downloads than a full release. If you are on 0.9.2 you need nothing but the client download; the rest of 0.9.3 is what changed on the node side after 0.9.2: a node monitor that records its own history, new probes, a profile window, and a monitor chip in the desktop app. The product, its install steps and its engines are otherwise those of 0.9.0.

### What 0.9.2 contained (carried into 0.9.3)

- **Adapters are served by ragon-serve through the gate.** When the engine's recipe lists the LoRA-adapter feature, ragon-serve stays the engine and the gate loads and unloads adapters through it. SGLang is still chosen when the image lacks the feature, and the reason is now said. The adapter list names the engine that serves each adapter, and a load the engine reports as failed is shown as a refusal instead of as loaded.
- **Adapters show in the desktop model chip and in `ragon web`.** The model chip groups the model list into Base and Adapters, each adapter by its own name and description, with a badge and the `/adapter` warning as a tooltip for a third-party one. `ragon web` keeps an adapter's model id when a page names it for a turn.
- **The ragon-serve pin lists LoRA adapters.** The pinned ragon-serve build lists the `lora-adapters` feature and takes adapters the way that engine expects, so a node on the new image serves them itself.
- **The launcher tells the engine which workload it is.** `ragon_workload` is sent to ragon-engine as a drafter hint, per surface or from `DREAMFERENCE_RAGON_WORKLOAD`; Night Shift sends `night` and SWE-bench sends `swe-bench`.
- **`ragon-search` says why it found nothing.** When every engine refuses, each is named with its cause (rate limit, human check, timeout). A URL router can be added as a second backend; it is off by default and every address it returns is checked.
- **Google CSE is off in SearXNG by default.** `DREAMFERENCE_SEARXNG_GOOGLE_CSE=1` turns it back on.
- **Phones on the overlay.** `ragon-admin matrix start --network overlay|tailscale` picks the network, and a new homeserver defaults to the overlay on a remote-access node. The Devices page shows the Matrix steps in a Phone on the overlay section. A node whose overlay peer was enrolled under an earlier prefix still answers to its overlay name in `ragon web`.
- **Discord `/push` and `/merge` are slash commands** with the same confirmation as Telegram's.
- **The forge mirror holds a refused branch.** A branch the forge refuses for unrelated history is held like an anonymity refusal.
- **Joining a MacBook is a short script.** The site serves `/join`, which joins a MacBook to the overlay with a browser login, and the macOS runner setup runs from a pipe.
- **Three test fixes.** The macOS signing check and the Telegram mini-app check run node by its resolved path, so a node reached through `~/.local/bin` is not refused as the installed binary; the signing gate runs through bash; the live messenger test treats an unlinked Signal device as a known failure.
- **Discord bridge groundwork: egress hosts and status.** The egress audit now knows the Discord bridge. With Discord on, exactly its four hosts may be resolved and the bridge is named in the audit's result; with it off, a lookup for any of them fails the audit. The bridge itself stays off until a command turns it on.
- **The messengers design is written down.** The messengers specification now describes the source channel of a message, per-channel reply rules, citations as links, a capability matrix per channel and button-only actions, as a proposal for a later release. Nothing in the bridges changes in 0.9.2.

### Since 0.9.2

- **The node keeps a monitor history.** The node monitor samples at fixed intervals into a local sqlite store: raw samples for 48 hours, then rollups, with the store capped at 256 MiB. It tracks state with hysteresis so a reading does not flap, feeds a gate item, and decides whether the monitor itself is alive.
- **`node enable` installs the monitor as a service.** `ragon-node-monitor.service` is installed and enabled with a 64M memory cap, a watchdog and journald logging.
- **GPU, NVMe and model-file probes, and a PSI trigger.** The monitor now reads GPU Xid errors, NVMe health and the model files, and watches memory pressure (PSI). A probe whose source is missing reports unknown rather than a guess.
- **The PSI watchdog runs as a user unit.** `ragon-psi-guard` runs as a user unit while the model server is resident.
- **A profile window that takes the GPU lock.** The profile window takes the GPU lock, refuses when it cannot, logs each window in `windows.log` and checks that the node is restored afterwards. A new fanout-32 workload (32 forks of one long prefix) checks cold, warm and undisturbed behaviour.
- **Profile levels gemm and trace.** Level 2 benchmarks the matrix multiplications and level 3 traces a decode step; the profile also reports the gain from speculation and the memory bandwidth decode uses.
- **A node monitor chip in the desktop app.** The desktop app shows the node's monitor state, read from `ragon web`'s `/api/monitor/status` route by the app's main process.
- **The lockfile pins the monitor's database library.** rusqlite and its dependencies are pinned in the codex-patches lockfile so the monitor store builds the same everywhere.
- **The web crate's `Config` has a test constructor.** `Config::for_test` exists, and the bridges' and sync's tests build their `Config` with struct-update syntax, so a new field no longer breaks them.
- **The as_team publish queue is stricter.** It refuses a new branch whose base GitHub does not have, and records the clone rather than the worktree.

### Measured

The SWE-bench Verified subset that runs on this hardware (357 validated instances) was run over the weekend of 7–8 November on the 0.9.0 build of the agent, in four slices and a recovery pass. **248 of 357 instances resolved (69.5 %).** The arm was the engine (ragon-serve) with one engine session per agent, the function-call exec tool and apply-patch off.

The same model served by ragon-serve answers one agent at about 64 tokens/s against 51 on SGLang, and four agents at 31–67 tokens/s each against 13 aggregate on SGLang, with byte-identical outputs. SGLang stays ahead only at wide speculative fan-out.

These are this machine's numbers for comparing configurations, not a leaderboard score.

### Maintenance

The rest of 0.9.3 is upkeep that does not change behaviour: tests mock the monitor unit that `node enable` installs, the desktop chip test allows `ragon web`'s status route, the specs README's two Fixed tables are one, and the logging, profiling and monitoring specifications carry the second round of decisions. The 0.9.2 maintenance still applies: the lint and type-checking debt in the egress audit, the CLI controller, the model gate, node, runner and host-safety code was cleared with the quality ratchet re-locked, and new tests cover the launcher's delegate, monitor and web wrappers, its update install path, and its node, night and refine commands.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on. LoRA adapters are served by ragon-serve or SGLang, whichever the recipe allows.
---
