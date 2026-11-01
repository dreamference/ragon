# Ragon 0.9.0 — release notes

The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

---

## Ragon 0.9.0

**Ragon is a local, air-gapped agentic pair programmer for a single NVIDIA GB10 box with 128 GB of unified memory.** Its agent, `ragon`, is a branded build of the open-source Codex agent, and it is served by the project's own model server, `ragon-serve`, on loopback: no cloud, no account, no API bill, no telemetry. Dreamference is the open-source project behind it.

### Install

```
curl -fsSL https://github.com/dreamference/ragon/releases/latest/download/install.sh | bash
```

Then run `ragon-admin server start` on the GB10 and `ragon` in a project. The [getting started guide](https://github.com/dreamference/ragon/blob/main/docs/getting-started.md) has the rest. The installers cover GB10 nodes and Linux, macOS and Windows clients.

### What 0.9.0 contains

- **`ragon`, the coding agent in your terminal.** It reads and edits your code and runs commands in a sandbox. `/airgapped on` gives every sandboxed command an empty network, and `ragon-admin audit egress` shows that a real session talks to nothing but your own machine.
- **The node and client split.** A GB10 is a node that runs the model; clients on other machines find it on the network by themselves. A second GB10 becomes a node over SSH. Remote access and a Forgejo on every node come with it.
- **The desktop app and `ragon web`.** The same page in a window or a browser: Ask for chat, Work for projects, with threads, diffs and a terminal.
- **The messengers.** Signal, Telegram and Matrix bridges ship switched off until a command turns one on.
- **A code index and a file index.** `ragon-code` answers definitions, references and callers. `ragon-docs` indexes the files you choose so the agent can search them, without opening a socket.
- **`ragon-serve` as the default engine.** It serves `qwen3.8-27b-nvfp4-dflash2`, reads images and holds four conversations at the full context. SGLang stays the fallback through the 0.9 releases.
- **Night Shift.** Queue tasks with `/night add`. Each runs overnight in its own git worktree, and nothing is merged or pushed.
- **Signed releases.** `ragon update` checks every archive against the release's signature before it swaps anything.

### Engines

`server start` uses `ragon-serve` for the default model. SGLang stays available through the 0.9 releases: `ragon-admin server start --engine sglang`, or `server_engine = "sglang"` in `dreamference.toml`. It goes in 1.0, with `ragon-serve` as the only engine.

If the engine's image cannot be pulled or the engine does not become healthy, and you have not chosen an engine, `server start` starts SGLang instead and prints one warning line. Choosing an engine yourself (`--engine`, `server_engine`) turns this off: you get the error.

### Known limits

- Models are served on a GB10 only; the Linux, macOS and Windows clients do not run a model.
- LoRA adapters are served by SGLang alone for now: while one is installed, `server start` uses SGLang by itself and says so.
- One model is served at a time.
- The messenger bridges are off until you turn them on, and each needs its own setup.
---
