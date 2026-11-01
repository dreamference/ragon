# Architecture

Ragon is a set of local services around one model server. Everything below runs on the GB10.

Ragon is made by **Dreamference**, and its Python side is the `dreamference` package. That is why
settings and paths are named `dreamference.toml`, `DREAMFERENCE_VLLM_HOST` and
`~/.local/share/dreamference/`.

```mermaid
flowchart LR
    T["ragon<br/>terminal agent"]
    B["Browser"]
    D["ragon-app<br/>desktop window"]
    W["ragon web<br/>:3100"]
    H["Helpers<br/>SearXNG · Whisper<br/>Google · Image search"]
    subgraph models["Model server"]
        V["ragon-serve, main model<br/>:8000"]
    end
    T --> V
    T -- "search, fetch, apps, images" --> H
    B --> W
    W -- "ragon app-server" --> V
    W -- "voice, images" --> H
    D -- "ragon app-server" --> V
    D -- "voice, images" --> H
```

| Part | What it is | Where it runs |
|---|---|---|
| Model server | ragon-serve (or SGLang, or vLLM) serving the main model through the standard `/v1` chat API | Docker container, port 8000 |
| `ragon` | The terminal agent | A native binary, linked from `~/.local/bin` |
| `ragon web` | Ask and Work in a browser, relayed to `ragon app-server` | Part of `ragon`, port 3100 |
| SearXNG | Metasearch: queries public search engines for web search | Docker container, reachable only from this machine |
| Whisper | Speech-to-text for the microphone button, on the CPU (`ragon-admin voice start`) | Docker container, reachable only from this machine |
| Google service | Read-only Gmail, Drive and Calendar for `/apps` | Docker container, reachable only from this machine |
| Image search | Finds images for the `image_search` tool and keeps them on this machine (`ragon-admin images start`) | Docker container, reachable only from this machine |
| `ragon-app` | An Electron window showing Ask and Work, with its own `ragon app-server` | Native app |
| `ragon-admin` | Manages all of the above | Python CLI |

The sidecars share one Docker network, `dreamference-sidecars`; image search reaches the model
server through the host's bridge address, because the model server shares the host's network.

## How `ragon` is built

```mermaid
flowchart LR
    C[Upstream source<br/>pinned release] --> E[exported copy]
    P[Ragon patches] --> E
    L[Ragon launcher<br/>Rust crate] --> E
    E --> X[cargo build] --> Y[ragon +<br/>codex-code-mode-host]
```

- **Pinned source.** The source Ragon was forked from sits in the repository pinned to a stable
  release, and is never edited in place.
- **Built from a copy.** `ragon-admin codex build` exports that release to a scratch directory,
  adds Ragon's launcher crate, applies a short series of patches, and compiles.
  - The patches are small: the product name in the interface, and one-line hooks into the launcher.
  - The rest of Ragon's changes live in the launcher: finding the model, the prompt, and the
    removed cloud-only commands.
- **Easy to upgrade.** Moving to a newer upstream release is a change of pin plus refreshing
  whichever patches stop applying.
- **Two programs.** The build produces `ragon` and `codex-code-mode-host`, the helper that runs
  the agent's Code Mode JavaScript in its own V8 engine. They are installed side by side.
- **Rebuilt only when needed.** A build is recorded against the exact source, patches and launcher
  it came from, so an unchanged tree is not rebuilt.

## Keeping the host alive

On the GB10 the GPU and the operating system share one pool of memory. A model load that
overshoots doesn't just fail: it can push the whole machine into a stall. Ragon guards against
that in two layers:

1. **Before a load.** `ragon-admin server start` inspects swap, kernel memory settings and whether
   an out-of-memory guard (`earlyoom` or `systemd-oomd`) is present and configured. If loading
   could freeze the host, it stops and explains what to change.
2. **During a load.** A watchdog samples the kernel's memory-pressure signal (PSI) every
   second. If pressure spikes, or stays high, it kills the model container before the host stalls,
   through the fastest route the host can still take.
