# Ragon 0.9.1 — release notes

The description between the two rules is the GitHub release text; `scripts/release_notes.py` prints it.

---

## Ragon 0.9.1

**Ragon 0.9.1 is a maintenance release.** It follows 0.9.0 with a small fix to how the project checks its macOS client; the product, its install steps, its engines and its known limits are those of 0.9.0.

### What 0.9.1 contains

- **The macOS client check uses the system `curl`.** The automated check that the macOS client's sandbox takes the network away now runs Apple's own `curl` on both the Apple-silicon and the Intel build. Before, the Intel build could pick up a different `curl` first on its path, which would hide a failure on the Apple-silicon build. This changes the project's tests only: the client you install behaves as it did in 0.9.0.

### Known limits

The limits of 0.9.0 still apply: models are served on a GB10 only, LoRA adapters are served by SGLang alone for now, one model is served at a time, and the messenger bridges are off until you turn them on.
---
