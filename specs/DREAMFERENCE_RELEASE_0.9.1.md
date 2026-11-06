# Ragon 0.9.1 — release notes

Status: shipped 2026-11-06

The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

---

## Ragon 0.9.1

**Ragon 0.9.1 is a maintenance release.** It follows 0.9.0 with a handful of fixes and a large clean-up of the code behind them; the product, its install steps, its engines and its known limits are those of 0.9.0.

### What 0.9.1 contains

- **A client with a leftover node id stops with a clear message.** When a machine still has a node id but nothing listens on the model port, the launcher now gives up after 30 seconds and says so, instead of waiting without end.
- **`night status` names a gate from an older install.** If a model gate started by an earlier install is still running, the status says that, where it used to report that there was no gate.
- **The client install docs are fuller.** The client section describes `ragon node list`, `ragon node use` and `ragon node exec`, recommends ripgrep, and shows how to verify a release by hand.
- **One ports module.** The model, engine, web UI and SearXNG ports and the default host are defined in one place instead of being repeated.
- **Two test fixes.** The macOS client check that the sandbox takes the network away now runs Apple's own `curl` on both the Apple-silicon and the Intel build, and a flaky desktop sync test now waits for the answer and the notice after the acknowledgement, so it passes the same way every time. Neither changes the client you install.

### Maintenance

The rest of 0.9.1 is upkeep that does not change behaviour: the lint and type-checking debt in the node, runner, model server, command-line, chat, audit, SWE-bench, Night Shift, hardware and MCP server code and in the tests folder was cleared and the quality ratchet re-locked, and new coverage tests were added for the node provisioner, node jobs, the launcher modules and the Windows build path.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, LoRA adapters are served by SGLang alone for now, one model is served at a time, and the messenger bridges are off until you turn them on.
---
