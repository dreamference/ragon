# Ragon 0.9.2 — release notes

Version 0.9.2, cut day 2026-11-09, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

Range: `git log v0.9.1..origin/main`. The text was finalised on 2026-11-08 against `v0.9.1..main` at c32a09f; the 0.9.1 changes are listed in `specs/DREAMFERENCE_RELEASE_0.9.1.md` and are excluded here.

---

## Ragon 0.9.2

**Ragon 0.9.2 lets the model gate serve LoRA adapters through ragon-serve, and adds a few fixes for search, phones and nodes, and lays the ground for the Discord bridge.** The product, its install steps and its engines are otherwise those of 0.9.0.

### What 0.9.2 contains

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

### Measured

The SWE-bench Verified subset that runs on this hardware (357 validated instances) was run over the weekend of 7–8 November on the 0.9.0 build of the agent, in four slices and a recovery pass. **248 of 357 instances resolved (69.5 %).** The arm was the engine (ragon-serve) with one engine session per agent, the function-call exec tool and apply-patch off.

The same model served by ragon-serve answers one agent at about 64 tokens/s against 51 on SGLang, and four agents at 31–67 tokens/s each against 13 aggregate on SGLang, with byte-identical outputs. SGLang stays ahead only at wide speculative fan-out.

These are this machine's numbers for comparing configurations, not a leaderboard score.

### Maintenance

The rest of 0.9.2 is upkeep that does not change behaviour: the lint and type-checking debt in the egress audit, the CLI controller, the model gate, node, runner and host-safety code was cleared with the quality ratchet re-locked, and new tests cover the launcher's delegate, monitor and web wrappers, its update install path, and its node, night and refine commands.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on. LoRA adapters are now served by ragon-serve or SGLang, whichever the recipe allows.
---
