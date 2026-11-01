# Ragon Image Search Tool — Technical Specification

**Status:** v14 — Implemented (updated from the source as built; v13 was the pre-implementation draft). Constants re-checked against `image_search_service.py`.
**Target:** Ragon (sidecar ecosystem)
**Source:** `dreamference/chat/image_search_service.py` (the sidecar), `image_search_sidecar.py` (`ragon-admin images start|stop|status`), `image_search_mcp.py` (the `image_search` tool, `ragon-admin images mcp`), `ragon-rs/src/images.rs` (the launcher's declaration), `ragon-rs/web/src/sidecars.rs` and the app's `app-protocol.ts` (serving `/images/`). Tests: `tests/test_image_search_service.py`, `tests/test_image_search_sidecar.py`.
**Design:** the tool is an MCP tool, the images are served at `/images/<id>.jpg` by `ragon web` and the app and shown inline, and the sidecar runs on `dreamference-sidecars` (RAGON_ASK §19.2). The funnel (§2 on) is described below.

---

## 1. Overview

A built-in `Image Search` tool lets the LLM search the internet for images and display them inline in chat. It is implemented as an independent **Ragon sidecar container (`dreamference-image-search`, port 8768)** and exposed to `ragon` sessions as the `ragon-images` stdio MCP server (`ragon-admin images mcp`, declared by `ragon-rs/src/images.rs`).

One `POST /search` call runs the whole funnel: SearXNG's image category with a **wide candidate pool (30)**, **in-memory thumbnail fetches** behind a hardened SSRF guard, a **SigLIP pre-filter**, **perceptual-hash collapse** of visual near-duplicates, a **vision rank-and-filter pass** by the served multimodal model (with the shortlist's full downloads already **prefetching speculatively**), then persistence of the winners into a local store (`~/.config/dreamference/image-search/data/images`) that `ragon web` and the desktop app serve back at `/images/{file_id}.jpg`. Caching locally is the point: hotlinked images die of CORP blocks and link rot; cached ones render permanently.

**Deviation from the v13 draft, adopted deliberately:** the sidecar is **standard library + Pillow, not FastAPI**. It follows the Gmail sidecar exactly — a single staged file in a stock `python:3-slim` container, nothing to install at boot beyond Pillow itself (pip-installed into `/tmp` on start, skipped when present, degraded gracefully when offline; only as one of two pinned wheels, `PILLOW_REQUIREMENT` with `--require-hashes`, bumped with the image's Python). The draft's architecture — endpoints, funnel, hourly GC — is implemented whole; only the web framework differs.

---

## 2. The funnel, as implemented

```
LLM custom tool call {"queries": ["ragon bird flying"], "count": 6}   (count optional, 1–10, default 4)
        │
        ▼
POST http://dreamference-image-search:8768/search   (X-Ragon-Image-Token shared-secret header)
        │
        ▼
SearXNG image search (http://dreamference-searxng:8080, format=json)
round-robin merge across queries, dedupe by image URL, cap 30
        │
        ▼
DEDUPE LOOKUP (SQLite, by source URL): a known image reuses its cached file and skips all
network work — but it does NOT skip judgment. Cached candidates ride through the ranking with
their local file standing in for the thumbnail: an image cached for one query is not thereby
relevant to the next. (v13 treated the cache as pre-approved; a live person-search re-served
three unrelated cached images, and that behaviour is now a regression test.)
        │
        ▼
THUMB STEP: parallel in-memory fetches, 512 KB / 5 s caps, unfetchable candidates drop out
        │
        ▼
SIGLIP PRE-FILTER (http://dreamference-siglip:9100, Infinity): cosine-scored, keep the top
max(8, count+2). DEGRADES to first-N when unavailable — which on GB10 it is: Infinity
publishes amd64 images only and GB10 is aarch64 (see §7).
        │
        ▼
PHASH COLLAPSE: hand-rolled 64-bit DCT pHash (Pillow + stdlib, no numpy/imagehash),
Hamming ≤ 8 merges to the first representative
        │
        ▼
RANK-AND-FILTER ─┬─ SPECULATIVE PREFETCH (concurrent)
   (vision)      │   hardened full downloads of every FRESH shortlist candidate begin when
                 │   the rank call is dispatched; cached candidates prefetch nothing
  one standard /v1 vision call to the served vLLM model: shortlist thumbnails (downscaled to
  256px JPEG by `_rank_thumb`) + query →
  strict-JSON array of the MATCHING indices, best first. A successful verdict is a FILTER:
  indices it leaves out stay out (an empty array is a valid answer), and only a transport or
  parse failure falls back to the unfiltered order. Even a single candidate is judged — one
  wrong image confidently embedded is the exact complaint that added filtering.
        │
        ▼
RECONCILE: winners = verdict ∩ successful downloads, up to `count`; in-flight fetches of
rank-dropped candidates are cancelled
        │
        ▼
PERSIST: winners re-encoded to JPEG (normalises format, strips non-image payload) into the
store; phash + source URL + title recorded in SQLite
        │
        ▼
{"response": "![title](/images/{file_id}.jpg)\n\n…",
 "instructions": "…include these lines VERBATIM… <the same markdown again>"}
```

The `instructions` field restates the Markdown embeds and forbids answering with a bare
acknowledgement, because a model given only a pointer ("embed the above") has answered `Done`
and shown nothing. The MCP tool description (`TOOL["description"]` in `image_search_mcp.py`) reinforces the same
contract from its side.

---

## 3. Sidecar service design

Single self-contained file, staged to `~/.config/dreamference/image-search/service.py` and
bind-mounted at `/config` — it runs on the host for imports/tests and as `__main__` in the
container, like the Gmail service.

### 3.1 Storage — user-owned bind mount, not a named volume

The container runs `--user uid:gid`, and Docker creates named volumes root-owned — the exact
permission trap the torch.compile cache documented. The store is therefore a directory under
the same bind mount: `/config/data/images/*.jpg` plus `/config/data/db.sqlite` (source URL,
file id, pHash, title, timestamp). **GC:** a background thread deletes images older than 7 days
hourly and then trims oldest-first to a 2 GB cap; a DB row whose file was collected reads as
absent and is purged.

### 3.3 Tool registration

**Current (`ImageSearchSidecar` + `ImageSearchMcp`):** `ragon-admin images start` (`ImageSearchSidecar.start` in `dreamference/chat/image_search_sidecar.py`, also invoked by `ragon-admin server start` on a node) generates the shared secret once into `~/.config/dreamference/image-search/secret` (mode `0600`) and starts `dreamference-image-search` on the `dreamference-sidecars` Docker network (`127.0.0.1:8768`). When the secret file exists, the machine is a node, and `/airgapped` is not `on`, the launcher (`ragon-rs/src/images.rs`) declares `ragon-images` (`~/.local/bin/ragon-admin images mcp`, `ImageSearchMcp` in `dreamference/chat/image_search_mcp.py`) as an MCP server for the `ragon` session. Every `image_search` call re-checks the air-gap configuration and the parent process's runtime seal (`SEAL_DIR_NAME = "ragon-airgapped"`), then `POST`s to `http://127.0.0.1:8768/search` with `X-Ragon-Image-Token`. The sidecar also serves `GET /images/{id}.jpg` and `GET /images/{id}.json` (`source_url`, `title`, `host`) without a token.

---

## 4. Provider & inference interactions

- **SearXNG** — `SEARXNG_CONTAINER_URL` (`http://dreamference-searxng:8080`, the deployment's own instance on `dreamference-sidecars`, JSON API). Relative
  `thumbnail_src` values are resolved against the SearXNG base.
- **SigLIP** — `dreamference-siglip`, an Infinity server (`michaelf34/infinity:latest-cpu`, CPU,
  `google/siglip-base-patch16-224`, weights in named volume `dreamference-siglip-cache`), provisioned best-effort by
  `ImageSearchSidecar.start_siglip()`. See §7 for the arm64 gap.
- **Vision** — the served vLLM model, reached at `DockerBridge.container_vllm_url(vllm_host)` with the served model id resolved by `ImageSearchSidecar.vision_model(vllm_host, model)` (reading `/v1/models` first and falling back to `resolve_model_hf_repo(model)`); both passed as
  environment (`RAGON_VISION_URL`, `RAGON_VISION_MODEL`), never recomputed in the sidecar.

---

## 5. Security & hardening

The hardened downloader implements, and the tests assert:
- **DNS pinning with private-address rejection** — every resolved address is checked
  (private, loopback, link-local, multicast, reserved, unspecified, or otherwise not globally
  routable, such as the shared address space 100.64/10 that tailnets use) and the checked address is
  the one connected to, so a rebinding answer cannot swap targets between check and connect.
- **Redirect capping** (3 hops), every hop re-validated.
- **Streamed size caps** — 512 KB thumbnails, 15 MB full images; over-cap bodies are refused,
  not truncated.
- **Magic-byte sniffing** (JPEG/PNG/GIF/WebP) — Content-Type is never trusted.
- **Exactly one SSRF exemption:** SearXNG's own host, because image results routinely carry
  `thumbnail_src` as SearXNG's private `/image_proxy` route. Full-size downloads get no
  exemptions at all. The same private address behind any other hostname stays refused
  (regression-tested).

---

## 6. Presentation

**Current (`ragon web` and the desktop app):** `ragon web` (`ragon-rs/web/src/sidecars.rs`) and the Electron desktop app (`desktop/electron/src/app-protocol.ts`) serve `/images/<id>.jpg` directly from `~/.config/dreamference/image-search/data/images` and render the Markdown image embeds inline in the assistant turn.

---

## 7. Known platform gap — SigLIP on aarch64

Infinity publishes amd64 images only; GB10 is aarch64, so `dreamference-siglip` cannot start
there. The start is best-effort, the warning names the reason, and the funnel degrades to its
first-N candidates — the vision rank-and-filter still runs and is the stronger judgment. On an
amd64 deployment the provisioning works as written. If the pre-filter becomes load-bearing on
GB10, the follow-up is a small torch/transformers SigLIP sidecar in the Gmail/image-search
mould (aarch64 wheels exist for both).

---

## 8. Testing

**Unit (offline, `tests/test_image_search_service.py`):** SSRF assertions (private ranges,
the single SearXNG exemption, redirect cap, size cap, magic bytes), pHash collapse/separation,
store dedupe and GC (age + size cap), and the funnel with injected fakes (round-robin merge,
cached-skip-network-but-not-judgment, vision verdict as filter, empty verdict, requested
count satisfied and gracefully under-satisfied, unfetchable candidates dropping out). The funnel's collaborators are constructor-injected for exactly this.

**Sidecar and MCP (`tests/test_image_search_sidecar.py`):** `ImageSearchSidecar` lifecycle (`start`, `stop`, `status`, secret creation, network migration onto `dreamference-sidecars`, SigLIP best-effort start, vision model resolution) and `ImageSearchMcp` stdio JSON-RPC protocol (`initialize`, `tools/list` without network I/O, `tools/call` forwarding with `X-Ragon-Image-Token`, and refusal when `/airgapped` is `on` or sealed by the parent PID).

