# Ragon 0.9.2 — release notes

Version 0.9.2, cut day 2026-11-10, built on the arm64 build box and hosted runners. The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

Range: this was written against 0.9.0 (`v0.9.0..main`) because 0.9.1 was not published when it was drafted; the 0.9.1 changes are listed in `specs/DREAMFERENCE_RELEASE_0.9.1.md` and are excluded here. Re-read `git log v0.9.1..origin/main` on cut day and adjust the list if 0.9.1 went out first.

On cut day, before the run: delete the `{{TBD` line in the description below. Until it is deleted, `scripts/release_notes.py` refuses the text and the `validate` job stops a non-draft run.

---

## Ragon 0.9.2

**Ragon 0.9.2 lets the model gate serve LoRA adapters through ragon-serve, and lays the ground for the Discord bridge.** The product, its install steps and its engines are otherwise those of 0.9.0.

{{TBD: remove this line on cut day, after re-reading git log v0.9.1..origin/main against the list below}}

### What 0.9.2 contains

- **Adapters are served by ragon-serve through the gate.** When the engine's recipe lists the LoRA-adapter feature, ragon-serve stays the engine and the gate loads and unloads adapters through it. SGLang is still chosen when the image lacks the feature, and the reason is now said. The adapter list names the engine that serves each adapter, and a load the engine reports as failed is shown as a refusal instead of as loaded.
- **Discord bridge groundwork: egress hosts and status.** The egress audit now knows the Discord bridge. With Discord on, exactly its four hosts may be resolved and the bridge is named in the audit's result; with it off, a lookup for any of them fails the audit. The bridge itself stays off until a command turns it on.
- **The messengers design is written down.** The messengers specification now describes the source channel of a message, per-channel reply rules, citations as links, a capability matrix per channel and button-only actions, as a proposal for a later release. Nothing in the bridges changes in 0.9.2.

### Maintenance

The rest of 0.9.2 is upkeep that does not change behaviour: the lint and type-checking debt in the egress audit, the CLI controller, the model gate, node, runner and host-safety code was cleared with the quality ratchet re-locked, and new tests cover the launcher's delegate, monitor and web wrappers, its update install path, and its node, night and refine commands.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, one model is served at a time, and the messenger bridges are off until you turn them on. LoRA adapters are now served by ragon-serve or SGLang, whichever the recipe allows.
---
