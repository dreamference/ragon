# Ragon Ask and the Ragon web server

**Status:** built (§16 to §19). The decision: **one web UI, `ragon web`, with Ragon's own pieces behind it.** This spec describes what the browser UI offers, Ask threads, and search over the user's own files.
**Builds on:**
- [RAGON_DESKTOP](./DREAMFERENCE_RAGON_DESKTOP.md): the Work window on `ragon app-server`, the bridge's allow-list (§4.3), the air-gap rule in the server (§8.2, patch `0023`), Night Shift's busy marker (§8.3); and its Electron rebuild (branch `desktop/electron`), which copies the upstream vendor's desktop app;
- [RAGON_APPS](./DREAMFERENCE_RAGON_APPS.md): Gmail, Drive and Calendar through `/apps`, served by the Google service on port 8767;
- [IMAGE_SEARCH](./DREAMFERENCE_IMAGE_SEARCH.md), [RAGON_GMAIL](./DREAMFERENCE_RAGON_GMAIL.md), [GOA](./DREAMFERENCE_GOA.md): the sidecars that stay;
- [CONTEXT](./DREAMFERENCE_CONTEXT.md) and [RAGON_CODE_INDEX](./DREAMFERENCE_RAGON_CODE_INDEX.md): the indexing machinery the document index reuses;
- [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md): what an advertised node publishes; [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md); [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md); the security review (branch `security/review-1`).

---

## 0. Decisions, stated first

1. **One agent, no second chat backend.** Chat becomes an **Ask thread**: a conversation with the same `ragon` agent, in a scratch folder instead of a repository. This is how the upstream vendor's desktop app does chat: it has no separate chat server either (RAGON_DESKTOP §1).
2. **One UI, two hosts.** The Work UI in `desktop/ui/` becomes the **Ragon UI** (Work and Ask threads, settings, apps). The Electron app shows it over `app://`; a new **web server, `ragon web`**, serves the same build to a browser. The UI talks through one message interface, so the same code runs in both (the upstream vendor's app re-dispatches its IPC as window `MessageEvent`s for exactly this reason).
3. **One bridge policy.** The allow-list, the dropped thread fields and the "answer only pending requests" rule (RAGON_DESKTOP §4.3, `desktop/bridge/src/lib.rs`) move into one data file, `desktop/bridge/policy.json` (built as `ragon-rs/web/policy.json`, §16), enforced by `ragon-rs/web/src/policy.rs` (inside `ragon web`) and by the Electron main process (`desktop/electron/src/policy.ts`, §18.1). Both run the same conformance vectors, so the two cannot drift.
4. **The web server requires a credential on every request, loopback included.** Sandboxed commands can reach loopback whenever the network is on (`/airgapped off`), and a published default credential would be reachable by them (security review). Nothing is reachable from the LAN unless the node is advertised and a device has been paired.
5. **Search over the user's files is new and local.** `ragon-docs` indexes folders the user chooses, offline, inside the same memory budget and network-less sandbox as `ragon-code`.

---

## 1. What the web UI offers

Each feature and the piece that provides it.

| Feature | Where | Provided by |
|---|---|---|
| Browser chat UI with history, search, rename | the Ragon UI | **Ask threads**: `thread/list`, `thread/search`, `thread/name/set`, `thread/archive` (all in the allow-list) |
| The model | `ragon app-server` | Nothing to register: it reads the model server through the launcher's tiers |
| The "Ragon" persona and its instructions | the named prompt **`ask`** (`ragon-rs/prompts/ask.md`) | Chosen per thread by the bridge (§3.2) |
| Web search, then opening results | **`ragon-search --read`** | Search, fetch the top pages, return extracts with numbered sources |
| Gmail, Drive, Calendar | **`/apps`** | The agent's MCP tools; connecting through the Google service's own `/connect` pages (port 8767), linked from the UI's Settings → Apps |
| Image search | an **`image_search` MCP tool** | The sidecar on port 8768; images served by `ragon web` and the app at `/images/` (§6) |
| Voice dictation | the UI's microphone button | Audio goes to the speech sidecar through `ragon web` or the app's main process (§7) |
| Image upload and understanding | the protocol's `localImage` input | Uploads are written into the thread's scratch folder and sent as `localImage`; Ask sends images only when `ModelSpec.supports_vision` is true |
| Python execution | the agent | It runs code itself, in its sandbox, in the scratch folder |
| Sign-in | pairing | Devices are paired instead of signing in (§4.3); Ragon is personal, one owner per machine (§11) |
| The desktop Ask window | the Electron app | The app's Ask view |
| The web UI on the LAN for clients | `ragon web` | The advertised port, devices paired (§8) |
| Telemetry | none | No telemetry exists in Ragon's code |

---

## 2. Architecture

```
 Browser (any device, paired) ─┐                         ┌─ ragon-search --read / ragon-fetch ── SearXNG (8888)
 Electron app (Ragon) ─────┤  Ragon UI           │  /apps MCP: Gmail, Drive, Calendar ─ Google service (8767)
                               │  (desktop/ui, one build) │  image_search MCP ───────────────── image sidecar (8768)
                               ▼                          │  docs_* MCP ─────────────────────── ragon-docs index
   app://  ── Electron main: policy.json ─┐               │  code_* MCP ─────────────────────── ragon-code index
   https/ws ── ragon web: policy.json ─────┼─► ragon app-server ─► model server (8000)
                                          │   (one per user)
   microphone ── /api/transcribe ─────────┴────────────────────► speech sidecar (8100)
```

### 2.1 One app-server per user, two ways to reach it

The upstream vendor's app starts its own app-server over stdio, and also supports connecting to a **local daemon** over a socket. Ragon does the same:
- **`ragon web` running** (always, on a node; on demand elsewhere): it owns the user's `ragon app-server`, started as `ragon app-server --listen unix://$XDG_RUNTIME_DIR/ragon/app-server.sock` (the pinned app-server accepts `stdio://`, `unix://PATH` and `ws://IP:PORT`). The Electron app connects to that socket instead of starting a second server, so a thread started in the browser streams in the app and vice versa.
- **`ragon web` not running:** the Electron app starts `resources/ragon app-server` over stdio, exactly as the upstream vendor's app starts its bundled agent.

Phase 0 measures whether two app-servers on one `~/.ragon` can coexist at all (their thread database and session files). If they can't, the Electron app always prefers the socket, and starts `ragon web` in local-only mode rather than a second stdio server.

### 2.2 The transport interface

The UI never talks to a transport directly. It uses one object with the shape the Electron preload exposes (`sendMessageFromView(msg)`, and messages arriving as window `MessageEvent`s):
- **in Electron,** the preload implements it over IPC;
- **in a browser,** a 2 KB adapter implements it over one WebSocket to `ragon web` (`/ws`), with the same chunking for large payloads.

Everything else (`rpc.ts`, `store.ts`, the views) is shared and tested once.

### 2.3 The bridge policy, once

`desktop/bridge/policy.json` (built as `ragon-rs/web/policy.json`, §16) holds:
- `allowedRequests`, `allowedNotifications` (today's `ALLOWED_REQUESTS`/`ALLOWED_NOTIFICATIONS`);
- `threadOpeners` and `droppedThreadFields` (`baseInstructions`, `developerInstructions`, `modelProvider`, `config`, `personality`; plus `dynamicTools`, `environments`, `selectedCapabilityRoots`, `history`, `path`, §16);
- the pending-request rule (answers only to server requests not yet answered);
- **`airgappedRefusedRequests`**: requests allowed only while `/airgapped` is off, judged by the level of the thread they name, else the configured one (the plugin installs, RAGON_AIRGAPPED §14.12; the composer's `!` command, `thread/shellCommand`, §14.13), with a reason of their own in `airgappedRefusalReasons`. The host supplies the level from the launcher's resolver;
- **`namedPrompts`:** the prompt names the UI may ask for (`default`, `ask`, `high-swe`, plus `chat` and `chat-work` for the phone bridges), which the policy layer, not the UI, turns into `baseInstructions` (§3.2).

`desktop/bridge/vectors/*.json` (built as `ragon-rs/web/vectors/outgoing.json`, §16) are conformance cases (message in → accepted or refused, fields out). The Rust crate's tests and the Electron main process's tests both run every vector.

---

## 3. Ask threads

### 3.1 What an Ask thread is

- **A thread with no repository.** The policy layer creates `~/.ragon/ask/<thread-id>/` and starts the thread with that `cwd`, sandbox `workspace-write` rooted there, and `runtimeWorkspaceRoots` limited to it.
- **The network follows the air-gap level,** as for any session: on at `off` (so `ragon-search` and `ragon-fetch` work), none at `on`. At `on` the UI says why search, apps and image search are unavailable, from `/airgapped`'s own status text.
- **Approvals:** reading tools need none; anything that would write outside the scratch folder is refused by the sandbox as usual.
- **Everything the agent makes stays in the folder:** downloaded pages, notes, generated files. The thread's view lists them, with "Reveal in folder" in the app and "Download" in the browser.

### 3.2 The `ask` prompt, chosen safely

`ragon-rs/prompts/ask.md`, a named prompt (RAGON_PROMPT), composed with the launcher's `web`, `email` and `code` blocks. It says:
- the agent answers questions and does research for the user; it isn't working in a repository;
- for current facts it uses `ragon-search --read`, and cites sources as `[n]` with the list at the end;
- for the user's files it uses `docs_search`/`docs_read`, citing `path` and page;
- for mail, files in Drive and calendars it uses the `/apps` tools, treating their content as untrusted;
- it writes anything long or reusable to the scratch folder and links it;
- it keeps answers short unless asked (cave mode applied as usual until it was removed).

**The UI never sends prompt text.** It asks for `prompt: "ask"`; the policy layer reads the composed text from `ragon prompt show ask --composed` (a new flag printing exactly what a session would receive) and sets `baseInstructions` itself. A name outside `namedPrompts` is refused. This is the one place the policy layer *adds* a dropped field, and the conformance vectors cover it.

### 3.3 What the Ask view offers

| Feature | How |
|---|---|
| New question, history, search, rename, archive | `thread/start` with `prompt: "ask"`, `thread/list` filtered by the Ask root, `thread/search`, `thread/name/set`, `thread/archive` |
| Streaming answers with citations | the stream as in Work; `[n]` markers linked to the source list |
| Attach images | written to the scratch folder, sent as `localImage`; only when the served model has vision (`ModelSpec.supports_vision`) |
| Attach files (PDF, documents) | written to the scratch folder; the prompt tells the agent to read them with `ragon-docs extract` (§5.3) |
| Voice | the microphone button (§7) |
| Images in answers | `image_search` results rendered as a gallery with a lightbox (§6; built as inline Markdown images, §19.2) |
| Apps | Settings → Apps (built under Skills and tools → Apps, `desktop/ui/src/ExtensionsView.tsx`): `app/list` with each app's state and its Connect link (the Google service's page) |
| Continue as Work | "Open in a project": forks the thread into a Work thread on a chosen folder (`thread/fork` with a new `cwd`); built as **Continue in a project** (DESKTOP §16 B33) |

---

## 4. The Ragon web server: `ragon web`

### 4.1 What it is

A subcommand of the `ragon` launcher (Rust, in `ragon-rs/web` (`ragon-web-server`), routed by `ragon-rs/src/web.rs`):
- `ragon web start [--lan] [--port N]`, `stop`, `status`, `open [--no-browser | --print-url]`, `pair`, `devices [--scope <device> watch|control]`, `revoke <device>`, `serve [--lan] [--port N]`, `ask [--port N] <question>`;
- run as a systemd user unit, `ragon-web.service` (on macOS, the launchd agent `ai.dreamference.ragon-web`), written and started by `ragon web start` itself (on a node, by `ragon-admin node enable`; there is no `ragon-admin web enable`, §16).

It serves:
- `/` and `/assets/*`: the Ragon UI build, embedded in the binary (no files to go stale, nothing loaded from elsewhere);
- `/ws`: the bridge, one WebSocket per tab, vetted by `policy.json` and relayed to the app-server socket;
- `/api/upload`: attachments into the thread's scratch folder, size-capped (images 20 MB, files 100 MB);
- `/api/transcribe`: audio to the speech sidecar (§7);
- `/images/*`: the image sidecar's store, read-only (§6);
- `/api/apps`: the Connect links for `/apps`, pointing at the Google service (remains 501; the UI reads `app/list` over the bridge instead, §19.2);
- `/healthz`.

**Port 3100**, kept in one constant so bookmarks never move.

### 4.2 Binding

- **Default: `127.0.0.1:3100`.**
- **An advertised node** (`ragon-admin node enable`): also the LAN address, advertised as `web=3100` in the `_ragon-node._tcp` record. `node enable --no-web` keeps it on loopback, as today.

### 4.3 Credentials: required everywhere

- **This machine:** `ragon web open` (and the Electron app, when it uses the web server's socket instead) reads `~/.ragon/web/token` (0600) and opens the browser on a one-time login URL. The URL is exchanged for a session cookie (HttpOnly, SameSite=Strict, Secure where TLS is on), and the one-time code dies with it.
- **Another device on the LAN:** `ragon web pair` (or the UI's Settings → Devices) shows an 8-digit code and a QR code, valid for 10 minutes, for one device. The device enters the code, gets a long-lived device cookie, and is listed by `ragon web devices` with its name and last use; `revoke` ends it.
- **Every request** carries a valid session. WebSocket upgrades and every POST also check `Origin` against the server's own origins, and `Host` against the addresses it serves (DNS rebinding). This is the fix the security review applied to the Google service, applied from the start.
- **Why loopback needs a credential too:** at `/airgapped off`, a command the agent runs, or a page a prompt injection convinced it to fetch, can reach `127.0.0.1:3100`. Without a credential, a command could start a thread with Full Access, or approve its own request through the bridge.
- **The Google service's `/connect` pages stay on loopback.** A paired device sees the apps' state, but connecting an account is done on the node itself, where the browser can complete Google's sign-in on `localhost`.

### 4.4 Secure context and the LAN

Browsers allow the microphone and the clipboard API only on HTTPS or `localhost`. Over plain HTTP from a phone (`http://192.0.2.10:3100`), voice and "copy" don't work. Options, in order:
1. **Phase A: accept it,** and say so in the UI. Text works from any device. The Electron app on a client keeps working, because it loads the UI over `app://` and reaches the node through its own connection.
2. **Phase D: TLS for an advertised node.** `ragon-admin web tls` creates a per-node certificate authority, prints its fingerprint, and offers the CA file to paired devices, which install it once. This is a decision for the user (§14), because installing a CA on a phone is a real step.

**Decided (the user):** the web UI stays text-only from other devices; most people will use the desktop app, which needs no certificate. Phase D is not scheduled. If it is ever built, it should be a name-constrained authority (`.local` names and private addresses only), verified on iOS and Android before relying on the constraint, and the desktop app should pin the node's certificate at pairing instead of installing the authority system-wide.

Copying still works over plain HTTP: selecting and copying text, Ctrl+V and pasting a screenshot (the `paste` event's `clipboardData`) are not restricted. Only `navigator.clipboard` is, so every copy button must fall back to `document.execCommand('copy')` on a hidden textarea when `navigator.clipboard` is undefined; a test runs the button with `navigator.clipboard` removed.

### 4.5 What `ragon web` never does

- No telemetry, no analytics, no remote fonts or scripts. The pages carry a strict CSP (`default-src 'none'`, scripts and styles `'self'`, `connect-src 'self'`).
- No route to the model server, SearXNG or the sidecars beyond the four listed in §4.1.
- No account system, no password: devices are paired, sessions are cookies.

---

## 5. Search over the user's files: `ragon-docs`

> **Specified in full and built in [RAGON_LOCAL_INDEX](./DREAMFERENCE_RAGON_LOCAL_INDEX.md) (`ragon-docs-rs/`):** the subsections below are the earlier design sketch, with notes where the built crate differs.

### 5.1 Scope

- **The user chooses folders:** `ragon docs add ~/Documents`, `ragon docs list`, `ragon docs forget <folder>` (removes its entries and chunks; built as `ragon docs remove <name>`, with `~/Documents` and `~/Downloads` indexed by default, `ragon-docs-rs/src/main.rs`); also in the UI's Settings → Files (built as Settings › Your documents, `desktop/ui/src/SettingsDialog.tsx`).
- **Formats:** Markdown, plain text, HTML, PDF (its text layer), DOCX, ODT, EML and MBOX. Scanned PDFs (images without text) are listed as "no text"; OCR is a later phase (built with RapidOCR and Tesseract OSD in `ragon-docs-rs/src/ocr/`, alongside reST, Org, LaTeX, CSV/TSV, spreadsheets and presentations in `ragon-docs-rs/src/extract/`).
- **Excluded by default:** `~/.ragon`, `~/.ssh`, `~/.gnupg`, keyrings and browser profiles, `.git` and dependency folders, and anything matching `.ragonignore`. Code repositories are `ragon-code`'s job.
- **Limits:** 100 MB per file, and a per-folder file count reported before indexing starts.

### 5.2 How it searches

- **Phase 1, keyword:** SQLite FTS5 with BM25 over chunks. A chunk is a heading section (Markdown, HTML, DOCX) or a page (PDF), split further at 2,000 characters. Each chunk keeps its path, page, heading and modification time.
- **Phase 2, meaning:** dense vectors from `nomic-embed-text-v1.5` on the CPU (the context engine's model, already cached here, pinned to the CPU because the GPU belongs to the model server; CONTEXT), stored as float32 blobs beside the FTS table, combined with BM25 by reciprocal-rank fusion. Built only if Phase 1's measurements (§12) show keyword search misses what users ask. (As built in `ragon-docs-rs/src/embed.rs` and `search.rs`, Phase 1 shipped with both FTS5—plus a CJK trigram table—and dense vectors from `snowflake-arctic-embed-m-v2.0` int8 on ONNX Runtime CPU, fused by weighted RRF; [RAGON_LOCAL_INDEX](./DREAMFERENCE_RAGON_LOCAL_INDEX.md) §15–§16.)

### 5.3 How it runs

- **Indexing** is done by `ragon-admin docs index` on the node, admitted against the same host-wide memory budget as `ragon-code` and run in a network-less bwrap sandbox inside `ragon-index.slice` (RAGON_CODE_INDEX's three rules). Extraction is Python (`pypdf` for PDFs, `python-docx`, the standard library's `email` and `html.parser`), decided in Phase 0 by quality on a sample of real files rather than by language preference. (As built in `ragon-docs-rs/`, indexing is run by `ragon docs index` or automatically by `ragon-docs session` inside `ragon-index.slice` (`ragon-docs-rs/src/host.rs`), extraction is in Rust with `dlopen`'d PDFium, and `ragon-admin docs setup` installs the pinned libraries and models.)
- **Freshness:** files are re-read when their size or modification time changes. While `ragon web` or the app is running, a watch (inotify) queues changes; otherwise `ragon docs index` (and Night Shift, before its tasks) catches up.
- **Queries only read:** a small read-only MCP server, `ragon-docs mcp`, offers the agent three tools:
  - `docs_search(query, k)`: chunks with path, page and a snippet;
  - `docs_read(path, page|chunk)`: the text of one place;
  - `docs_list(folder)` (not built as an MCP tool; `ragon-docs mcp` offers `docs_search`, `docs_read` and `pdf_search`, `ragon-docs-rs/src/mcp.rs`).
  The launcher declares it like `ragon-code`'s server, so every session, Ask or Work, has it.
- **`ragon-docs extract <file>`** prints a file's text, for attachments in an Ask thread.
- **Air gap:** neither indexing nor querying touches the network, so the tools stay available at `/airgapped on`, unlike search and apps.
- **Privacy:** the index holds the documents' text. It lives in `~/.ragon/docs/index.db` (0600; built as one SQLite file per collection at `$CODEX_HOME/docs/<collection>.db`, mode 0600, `ragon-docs-rs/src/store.rs`), is never copied to another machine (not by `sync-model`, not by pairing), and `forget` (built as `ragon docs remove <name>`) deletes the collection's database file.

### 5.4 Relation to other specs

[PDF_SEARCH](./DREAMFERENCE_PDF_SEARCH.md) (proposed, not built; built in `ragon-docs-rs/src/pdf_search.rs`) is about PDFs on the web; `ragon-docs` is about files on the disk. They share the extraction code once both exist. The old context engine (`dreamference/context_engine/`) indexes code and is superseded by `ragon-code`; its embedding and storage code is reused for Phase 2 (not reused: `ragon-docs-rs/` has its own Rust ONNX Runtime embedding and SQLite storage).

---

## 6. Images

- **The tool:** `image_search(queries, count)`, served over MCP by a thin `ragon-admin images mcp` that calls the existing sidecar's `POST /search` with its token. It returns Markdown images pointing at `/images/<id>.jpg`.
- **Serving:** `ragon web` and the Electron app's `app://` handler serve `/images/*` read-only from the sidecar's store (`~/.config/dreamference/image-search`).
- **Presentation:** the gallery and lightbox are rebuilt as a React component in the Ragon UI, replacing `GALLERY_SCRIPT`/`GALLERY_CSS`.
- **Network:** the sidecar moves to `dreamference-sidecars`. It still uses SearXNG and the served model's vision, as today (IMAGE_SEARCH §2). Off at `/airgapped on`.

---

## 7. Voice

- **The sidecar stays:** `dreamference-stt` (speaches, CPU, `faster-whisper-small`), moved to `dreamference-sidecars`, published on `127.0.0.1:8100` only.
- **The UI:** a microphone button in the composer records with `MediaRecorder`. On release the audio goes to `/api/transcribe` (web) or to the main process (Electron), which forwards it to the sidecar's `/v1/audio/transcriptions`, and the text is **inserted into the composer**, not sent. The user reads it first.
- **At `/airgapped on`:** voice still works, because transcription is local.

---

## 8. Nodes and clients

- **On a node,** `ragon web` runs as a service; the advertised record names `web=3100`.
- **A browser on another device** pairs once (§4.3), then uses Ask and Work on the node: the agent runs on the node, in the node's folders.
- **The Electron app on a client** runs its own `ragon app-server` against the node's model server, as Work does today. Ask threads on a client run on the client, with search through the node's SearXNG (`ragon-search` already reads `node.json`). `/apps` and `ragon-docs` are local to wherever the agent runs: on a client they show "on the node only" until a later phase.
- **The client app has no TCP forwarder.** Ask runs on the client against the node's model server; the app offers "Open the node in a browser".

---

## 9. Security and privacy, together

- **One family of processes to audit:** `ragon web`, `ragon app-server`, the MCP servers, and the sidecars. The egress audit gains `--web`: it traces `ragon web` and its app-server through a scripted Ask turn with search, image search and a docs query, and must pass with only loopback and the search engines' traffic from SearXNG (which the audit already accounts for).
- **No published credential anywhere.** Every credential is per install.
- **The policy layer, the air gap in the server (patch `0023`), the sandbox and the untrusted-content wrapping** are unchanged and apply to Ask exactly as to Work.
- **The Electron app's fuses and CSP** (branch `desktop/electron`) apply to the Ask view, which is part of the same bundle.

---

## 10. Delivery

Everything in this spec ships in 0.9.0: Ask threads, `ragon web` (loopback; LAN with pairing on advertised nodes), `ragon-search --read`, `ragon-docs`, image search, voice and apps in the Ragon UI, and `policy.json` with vectors. The web UI is `ragon web` (port 3100) and the desktop app shows the same page. Image search and voice are `ragon-admin images start` and `ragon-admin voice start`, and `server start` starts both on a node. `ragon-admin node provision --web` shares `ragon web` on the LAN through `node enable`.

## 11. What Ask does not do, on purpose

- **Several user accounts and an admin panel.** Ragon is a personal tool: one owner per machine, with paired devices.
- **Assistants, projects and sharing.** Ask has threads and folders; sharing a thread means sharing its scratch folder.
- **Connectors** (Slack, Confluence and the rest). Pulling company data from other services runs against the product's local-first promise. `/apps` covers the Google services the user connects; `ragon-docs` covers files on the disk.
- **Google sign-in to the UI.** Devices are paired instead.

---

## 12. Phase 0: measurements before building

| # | Question | How | Decides |
|---|---|---|---|
| 1 | Can two `ragon app-server` processes share `~/.ragon`? | Start two, run a turn in each, list threads from both, check the thread database and session files | §2.1: whether the app must always use the socket |
| 2 | Does `--listen unix://` behave like stdio for every method the UI uses? | Replay a recorded Work session over the socket | The web server's transport |
| 3 | WebSocket relay overhead | Time first-token and stream rate through `ragon web` against stdio, same prompt | Whether the relay needs batching |
| 4 | Extraction quality | 20 real PDFs, 10 DOCX, 10 HTML pages: `pypdf` against `pdfminer.six` against `pdftotext` | §5.3's extractor |
| 5 | Keyword search quality | 30 questions over a real Documents folder: does the right chunk appear in the top 5? | Whether Phase 2 (dense) is needed |
| 6 | Indexing memory and time beside the model | Index 5,000 files with the model server serving; watch earlyoom's margin | The memory budget's share for `ragon-docs` |
| 7 | Voice end to end | Record in the Electron app and a browser on localhost; check latency and text | §7 |
| 8 | `ragon-search --read` against a plain web search | The same 20 research questions both ways; compare sources and answers | Whether the web answers are good enough |

---

## 13. Tests

- **Policy:** `policy.json` vectors run by the Rust crate and by the Electron main process; one extra vector per named prompt.
- **`ragon web`:** routes, the credential on every route (including `/healthz` from the LAN), pairing (expiry, one use), `Origin`/`Host` checks, upload size caps, and refusals at `/airgapped on` for the routes that need the network.
- **UI:** the existing Vite tests plus the Ask view, run once against the IPC adapter and once against the WebSocket adapter.
- **`ragon-docs`:** extraction fixtures per format, chunking, BM25 ranking on a fixture corpus, `forget` leaving nothing behind, the sandbox having no network (as `ragon-code`'s tests do).
- **Egress:** `audit egress --web` must pass before Phase A ships.

---

## 14. Questions for the user

1. ~~Port~~ **Decided:** 3100 for now (the user may revisit it; keep the port in one constant and the node advert, so a change is one edit).
2. ~~Voice from phones~~ **Decided:** text only from other devices' browsers; voice through the desktop app (§4.4).
3. ~~Phase A's scope~~ **Decided:** `ragon-docs` ships on its own as soon as it is ready (agent and command line); Ask and `ragon web` pick it up when they land.

---

## 15. Changes to other specs when this is built

- **RAGON_DESKTOP:** Chat becomes Ask; `policy.json` replaces the hard-coded lists; the forwarder is retired.
- **RAGON_NODE:** the advertised `web` port becomes 3100; §5's "the web UI has one account" risk is replaced by pairing.
- **RAGON_APPS:** Connect is linked from the UI's Settings → Apps.
- **IMAGE_SEARCH:** an MCP tool; served by `ragon web` (built, §19.2).
- **RAGON_EGRESS:** the `--web` mode.
- **ARCHITECTURE, CLI, SETUP, AGENTS.md:** the web UI is `ragon web`.

---

## 16. What was built (Phase A, first part: branch `ask/phase-a`)

Built: the bridge policy as data, Ask threads, and `ragon web` with its credentials. Not built yet: the UI's Ask view, voice, images, `ragon-docs`.

**The policy is data, and both hosts are held to the same cases.** `ragon-rs/web/policy.json` holds the allow-lists, the dropped fields, the named prompts and the Ask rules. `ragon-rs/web/vectors/outgoing.json` holds the conformance cases: 26 of them, each with fixed stubs (a prompt composes to `PROMPT:<name>`, folders are `SCRATCH/n`, threads `ask-*` are Ask threads). They live beside the crate, not in `desktop/bridge/`, because the build copies only `ragon-rs/` into the Codex export. The desktop app should read the same file and run the same cases (built in §18). Five things differ from §2.3 and §3.1, all found while checking the pinned protocol (`app-server-protocol/src/protocol/v2/thread.rs`):

- **More fields are dropped from every thread opener.** Besides the five the desktop dropped, the policy removes `dynamicTools`, `environments`, `selectedCapabilityRoots`, `history` and `path`. Each loads tools, environments, plugins, a forged history or a rollout file.
- **A prompt is chosen on `thread/start` only.** A resumed session keeps the prompt it recorded, so `prompt` on resume or fork is refused.
- **An Ask folder is named after its thread once the thread exists.** The thread id does not exist when `thread/start` is vetted, and a running session's `cwd` cannot be renamed. So the folder is created as `~/.ragon/ask/q-<random>/`, and the server's answer makes `ask/<thread-id>` a link to it. The link is also how an Ask thread is recognised afterwards: a resume or fork of one keeps its folder and sandbox whatever the UI asks, and uploads land there. The one exception is a fork naming a Work folder (an existing folder, named by an absolute path, outside the Ask root: a project's primary folder), which goes there as a Work thread with the same sandbox and none of the UI's sandbox, permissions or extra roots (DESKTOP §16 B33, `forkRequest` in `policy.json`).
- **`runtimeWorkspaceRoots` is dropped, not set.** The field is experimental, so a client that has not opted in cannot send it. Without it the writable root is the `cwd`.
- **The prompt text comes from a subprocess.** It is `ragon prompt show <name> --composed`, run in the Ask root. `--composed` prints the text alone, exactly. The launcher's own composition is shared through `prompt::current_parts`.

**`ragon web`** is the crate `ragon-rs/web` (`ragon-web-server`); the launcher's `src/web.rs` only routes to it. Its pieces:

- **Credentials.** One-time login codes (two minutes), 8-digit pairing codes (ten minutes, one use; ten wrong codes withdraw every pending code), and device cookies. Sessions from `ragon web open` live in memory, so a restart means `ragon web open` again.
- **Nothing a sandboxed command can read is a credential.** Codex's sandbox limits writes, not reads, so a command the agent runs can read everything under `~/.ragon/web/`. Three consequences:
  - The owner token (`token`) opens `/healthz` alone, for `ragon web status`.
  - Pending codes are stored under their SHA-256, so listing the folder reveals none.
  - Devices are stored as hashes.

  Signing in needs a code written by the user's own shell; the sandbox cannot write that folder. An earlier draft also accepted the token as `Bearer` on `/ws`, which would have let an injected command open the bridge.
- **Request checks.** `Host` and `Origin` checks, and the CSP of §4.5 widened to what the UI's own `<meta>` allows.
- **The relay.** One app-server connection per tab, on `$XDG_RUNTIME_DIR/ragon/app-server.sock`. `ragon web` starts `ragon -c features.code_mode_host=true app-server --listen unix://…` when nothing answers there, passing `DREAMFERENCE_VLLM_HOST` when this machine is a node or has the host configured.
- **The browser bridge.** `/bridge.js` defines `window.electronBridge` and `ragonWindowType = "web"` over the WebSocket, so the desktop UI runs unchanged.
- **Uploads.** `/api/upload` writes into Ask folders only, within 20 MB for images and 100 MB for files.
- **Night Shift.** The busy marker is kept per tab, and Night Shift now treats `web` as non-interactive.
- **`ragon web ask "<question>"`.** One Ask thread through the running server, for scripts and for `ragon-admin audit egress --web`. It signs in like a browser: it writes a login code and trades it for a session cookie. That mode traces `ragon web serve`, and everything it starts, while the untraced client asks.

**Not built in this part:**

- **Unimplemented routes.** `/api/transcribe`, `/images/*` and `/api/apps` answer 501.
- **The embedded UI.** `build.rs` embeds the UI from `RAGON_WEB_UI_DIST`, but no build sets it yet, so `/` serves a placeholder until the desktop branch's `desktop/ui` is merged and the builder points at its `dist`. (Built in §17.)
- **Pairing extras.** No QR code, and no `ragon-admin web enable`; `ragon web start` writes and starts the user unit itself.

**Verified on this machine.**

- **Web crate:** `cargo test` in a copy of `ragon-rs/web`: 22 unit tests and 11 server tests. The server tests cover the credential on every route, pairing (one use, expiry, withdrawal after ten wrong codes), `Host` and `Origin`, upload caps, and a tab through the policy to a stand-in app-server on a Unix socket. They also check that the owner token opens nothing but `/healthz`. They use no real network and no real `ragon`.
- **Launcher:** `cargo test -p ragon-launcher -p ragon-web-server` in an export of the pinned Codex with the patches applied: 163 launcher tests, including the `ask` prompt.
- **Egress:** `ragon-admin audit egress --web`, with a debug build from that export and the served Qwen3.8 model, **passes**. It connected only to the model server (3×) and the Gmail service (2×, the `ask` prompt's email block), sent no DNS query, and started `ragon` three times: the server, `prompt show --composed`, and the app-server.

---

## 17. What was built (Phase A, second part: branch `web/ask-ui`)

Built: the UI embedded in every `ragon` build, the Ask view, attachments, the phone layout, the desktop app's Ask window on `ragon web`, and the node advert on 3100.

**The UI is in the binary.** `CodexBrandedBuilder` builds `desktop/ui` before cargo (`npm ci` once, then `npm run build`) and sets `RAGON_WEB_UI_DIST` for `ragon-rs/web/build.rs`. A build without npm stops: a `ragon` whose `ragon web` serves the placeholder is the bug being fixed. The UI's sources are part of the build key, so a UI change rebuilds `ragon`. `build.rs` embeds from an absolute, non-canonical path, which avoids Windows' `\\?\` prefix. The release, client and Windows `ragon` jobs set up Node 22. The page has a favicon (the app's mark, bundled under `assets/`).

**One page, two hosts, two views.** In `ragon web`, the UI shows **Ask** by default and **Work** one click away (`#work`). In the desktop app's Work window (`app://`), it shows Work alone. That window's bridge (`desktop/electron/src/bridge.ts`) does not have the policy layer that turns `prompt: "ask"` into text and a folder, so it cannot run Ask. The page tells its host apart by `window.ragonWindowType` (`html[data-host]`), and the frameless title bar's padding and drag region apply to the app alone. A browser keeps its own context menu, which a phone needs for copy and paste.

**Ask threads.** "New question" clears the view. The first message starts the thread with `{ model, prompt: "ask" }` and no `cwd`; the policy sets the folder, the sandbox and the composed prompt (§16). `work/start` now answers with `ask_root` (canonical). The UI lists the threads whose `cwd` is a `q-<hex>` folder under that root, and keeps them out of Work's projects. Search uses `thread/search` (with a snippet), rename uses `thread/name/set`, and archive uses `thread/archive`, which also handles the server's `thread/archived` notification.

**Attachments.** The clip button and pasted screenshots (the `paste` event's files) attach files to the next message. They are uploaded only after the thread exists, because `/api/upload` needs the `ask/<thread-id>` link. Images (PNG, JPEG, GIF, WebP) go as `localImage`. Other files are named in the text with their path in the thread's folder. A message may be an image alone. The model's vision is not checked; the served model has it.

**Copy.** Each answer has a copy button. Without `navigator.clipboard` (plain HTTP from another device, §4.4), it falls back to `document.execCommand("copy")` on a hidden textarea. A test runs it with `navigator.clipboard` undefined.

**Phone layout.** Below 720 px, the thread list becomes a drawer behind a ☰ button, with a backdrop that closes it. Inputs are 16 px, so mobile browsers do not zoom in. The chips that do not fit are hidden, and the composer clears the safe area. The page is text only from other devices; nothing asks for the microphone.

**The desktop app's Chat is Ask**. The window (`chat.ts`, menu label "Ask") loads the Ragon UI from `ragon web` on this machine, with no preload and no IPC. `web.ts` checks `ragon web status`. When nothing answers, it starts `ragon web serve` as the app's child and stops it on quit; a server the user runs as a unit is left alone. It then signs the window in with a link from `ragon web open --print-url`, a new flag that prints the one-time link alone. A 401 after a server restart gets one fresh sign-in. The window never navigates off that server, and links open in the system browser. On Windows, which has no `ragon web` yet, Ask opens Work. On a client, Ask runs on the client against the node's model server, as §8 says; the bundled `ragon` finds the node. The app grants no permission except clipboard writes (no microphone). `ragon app` opens Ask (`--ask`, with `--chat` kept as an alias), and so do `ragon-admin desktop run` and `status`. `audit egress --app` allows `ragon web`'s 3100.

**The node advert names `ragon web`.** `node enable` advertises `web=3100` when `ragon` is installed, and runs `ragon web start --lan`. `--no-web` and `disable` run `ragon web start` again, back on loopback, when the unit was on the LAN.

**Verified on this machine**, with no live service touched:
- `desktop/ui`: 22 vitest cases (11 new: Ask threads, attachments, the copy fallback, archive and rename in the store), typecheck, and the production build.
- `ragon-rs/web`: `cargo test` in a copy, with and without the UI embedded: 22 unit and 12 server tests. The new server test serves the embedded page with the bridge first and every asset it names.
- `desktop/electron`: 20 vitest cases, including `web.test.ts` (a running server used as it is; a missing one started, owned and stopped; only a one-time loopback link loaded), and the typecheck.
- Launcher: `cargo test --release -p ragon-launcher -p ragon-web-server` in a scratch export of the pinned Codex with the patches applied, the UI embedded and a scratch `HOME`: 229 launcher tests on the tree merged with main (with `ragon app`'s Ask window), 22 + 12 web tests.
- `CodexBrandedBuilder.build_web_ui()` run for real (npm in `desktop/ui` only; nothing installed, no `ragon` run). `ragon-admin codex build` was not run here, because it installs into the live folder.
- The Python suite.
- A headless Chrome (playwright-core, a throwaway profile) against a scratch `ragon web` built from this branch: the real server crate with the UI embedded, in front of a scripted app-server on a Unix socket, port 3199, a scratch home. All 23 checks passed:
  - sign-in by a one-time link, then Ask as the default view, with no console error and no CSP violation;
  - a question with an image: `thread/start` carried `baseInstructions` from the stand-in composer, the sandbox and an Ask folder, with no `prompt`; the image was written into that folder and sent as `localImage`;
  - the copy button with `navigator.clipboard` removed called `execCommand("copy")`;
  - rename, search with and without a match, and archive;
  - the Work view;
  - at 390 px: no horizontal scroll, the drawer closed, opened and closed again, and 16 px inputs;
  - a browser without a session got 401.

**Not built in this part:**
- **The desktop app's Work window still has no Ask.** Ask needs the policy in the Electron main process (§2.3: `bridge.ts` reading `policy.json` and running the vectors), or Work reaching `ragon web`'s socket (§2.1). (Built in §18, the first way.)
- **Two app-servers on one `~/.ragon`.** With `ragon web` (socket) and the app's Work (stdio) both running, Phase 0's question 1 is still unmeasured.
- **Images, apps and voice in the UI.** `/images/*`, `/api/apps` and `/api/transcribe` still answer 501. The gallery, Settings → Apps and the microphone are out of scope by the user's decision.
- **Pairing in the UI.** There is no Settings → Devices, and no QR code; `ragon web pair` prints the code. (Built in §18.)

---

## 18. What was built (Phase A, third part)

Built: Ask in the desktop app's own window, and pairing a device by QR code. Two of the gaps listed in §17.

### 18.1 Ask in the app window: the policy in the main process

**The choice.** §17 named two ways: the policy in the Electron main process (§2.3), or the app window reaching `ragon web`'s socket (§2.1). §0.3, §2.3 and §16 all ask for the first, and the second is the open question of §2.1 and Phase 0 question 1 (one app-server or two), which is not decided here. So the main process enforces `policy.json` itself, and the app window keeps its own `ragon app-server` over stdio, as Work had.

**One file, one set of cases.** `desktop/electron/src/policy.ts` imports `ragon-rs/web/policy.json` at build time (Vite inlines it, as `include_str!` does in the crate), so a packaged app cannot be pointed at another file. It is a line-for-line port of `policy.rs`: the allow-lists, the dropped thread fields, a prompt only on `thread/start`, a named prompt turned into `baseInstructions`, `default` setting nothing, a scratch prompt confining the thread, and a resumed or forked Ask thread kept in its folder. `policy.test.ts` runs every case of `ragon-rs/web/vectors/outgoing.json` with the shared stubs, so the two hosts are held to the same 26 cases. The hard-coded lists in `bridge.ts` are gone; the Python test that checks the allow-list against the pinned protocol now reads `policy.json`. One consequence: the app window may now send `app/list`, which `ragon web` already allowed.

**Ask folders, as `ragon web` makes them** (`desktop/electron/src/ask.ts`, a port of `ask.rs` and of `/api/upload`):
- **The prompt.** Before vetting, `server.ts` runs `ragon prompt show <name> --composed` in `$CODEX_HOME/ask`, with the app-server's own `ragon` and a 60-second limit, as `prompts.rs` does. The environment is the app-server's without `LOG_FORMAT` and `RUST_LOG`, which belong to the server. The launcher answers `prompt` before Codex parses anything, so no log line can reach stdout anyway. Vetting stays synchronous, so the policy and its cases are the same code.
- **The folder.** It is `ask/q-<16 hex>`, 0700 and canonical. When the server answers the `thread/start`, `ask/<thread-id>` becomes a relative link to it. On Windows it is a junction, which needs no privilege and which the Rust side's `canonicalize` also follows. `ragon web` on Windows would write a plain file instead, but `ragon web` does not run there.
- **`ask_root`.** `work/start` answers with it, so the page tells Ask threads from projects exactly as in a browser.
- **Attachments.** These go over the same IPC channel as `ask/upload`: the bytes in base64, cut into 1 MB pieces like any large message. The checks are `ragon web`'s: the thread must have an Ask folder, images are capped at 20 MB and other files at 100 MB, the file name is cleaned the same way, and nothing is overwritten (`-1`, `-2`, … as on the server). `app://` has no `/api/upload`, so `bridge.upload` picks the channel by host.

**The page.** The page (`desktop/ui`) shows Ask and Work in both hosts. `#work` opens Work and anything else opens Ask, as in `ragon web`.
- **Opening on Work.** The app window opens on Work when the menu's Work entry, `--work`, a folder, a thread or a `ragon://` link asked for it.
- **Switching an open window.** A new `view` message on the for-view channel switches a window that is already open.
- **The header's Ask button is gone.** It opened the separate Ask window; Ask is now in the sidebar.
- **The menu.** The menu's Ask entry still opens the Ask window on `ragon web` (§17), which the Python tests and `audit egress --app` pin. On Windows, where that window never existed, it now opens the app window on Ask, so Windows has Ask for the first time. (Folded into the app window on every platform in §18.6.)

### 18.2 Pairing by QR code

**The Devices page.** `ragon web` serves `/devices` (§4.3's Settings → Devices), linked from the sidebar of the browser host as "Pair a phone or another device". It is a page of the server's own, not a view of the React UI, because pairing exists only where `ragon web` runs. It has no script; its forms post back to the server.
- **Who may use it.** Only this machine's session, from `ragon web open`, may use it (`Server::owner_session`). That session's link is for loopback.
- **A paired device.** It gets a 403 page telling it to pair or revoke on the Ragon machine. So a stolen phone cookie cannot mint more devices, and a device cannot revoke the others.
- **Checks.** The page needs a credential like every route, and its POSTs pass the `Origin` check.
- **The page's parts.** It lists the paired devices (name, when paired, last use) with a Revoke button for each. **Show a pairing code** answers with the page itself, not a redirect, so the code never sits in a URL on this machine.
- **On loopback only,** it explains that no other device can reach the server and issues no code.

**What the QR code carries.** It is `http://<address>:<port>/pair?code=<8 digits>`: the pairing code of §4.3, valid ten minutes, one use, ten wrong tries withdrawing it. Nothing longer-lived goes in it: never the owner token, a session or a device cookie (a test reads the page for both).
- **Opening the link.** `GET /pair?code=` only fills in the form's field, and only with eight digits; anything else is not shown back. The person still presses Pair, so a link preview or prefetch cannot spend the code.
- **Which address comes first.** The address shown big is the one a phone most likely reaches: 192.168/16 and 10/8, then Tailscale's 100.64/10, then other addresses, then 172.16/12, where Docker's bridges are, then names. `.local` needs mDNS on the phone and does not cross Tailscale. The other addresses are folded underneath, each with its own code, labelled "(Tailscale)" where it applies.
- **Fixed at start.** The addresses are the ones `ragon web serve --lan` read at start. An interface that comes up later is not offered until the server restarts.

**Drawn locally.** The QR codes are drawn by `qrcodegen` (Nayuki, MIT, no dependencies), a new dependency of `ragon-web-server` (`src/qr.rs`), with ECC level M.
- **As SVG on the page.** The modules are drawn on white whatever the page's theme, and they are inline elements, which the existing CSP allows.
- **As half blocks in the terminal.** `ragon web pair` now prints the same code, black on white set explicitly, when stdout is a terminal. It lists the most reachable address first.
- **No service.** No QR service and no script from elsewhere is used.

### 18.3 The two app-servers: what this part learned

This part did not decide §2.1. What it changes, and what was read on the way (the app's own second server is gone since §18.6; the risk below remains for a `ragon web` the user runs beside the app):
- **Two app-servers can run on one `~/.ragon` today.** With the app open, both windows are in use: the app window on its own stdio app-server, and the Ask window on `ragon web`'s app-server on the socket. Ask threads from either live under the one `~/.ragon/ask/` and are recognised by the same `ask/<thread-id>` links. Because the folder rules are the same, either host can resume the other's Ask thread in its folder.
- **The thread database is built for several connections.** Codex's state databases open read-write pools in WAL mode with a five-second busy timeout (`state/src/sqlite.rs`), which SQLite supports across processes. Read in the submodule's source, not measured.
- **The rollout files may not be.** Codex appends a thread's history to its JSONL rollout file. A search for lock calls (`flock`, `lock_exclusive`, `try_lock_exclusive`, `fs2`, `fd_lock`) in `rollout/` and `core/src` found none. This was read, not measured, in the submodule checkout, not confirmed at the pinned tag. (Verified at `rust-v0.158.0`, [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md) §2 and §16.6: `codex-rs/rollout/src/writer_lock.rs` holds a non-blocking exclusive `flock` on `~/.ragon/thread-writer-locks/<thread>.lock` per thread.) So the risk is the same thread loaded in both servers at once: two writers on one rollout file, each with its own in-memory turn state. The app makes that possible, since one question can be open in the app window and in the Ask window. Neither server knows about the other's turn: the busy markers are per process, and Night Shift reads them all.
- **What Phase 0 question 1 still has to measure.** Run a turn on one thread in both servers, and check the rollout file and `thread/list` from each. Until then, the app window with its own server is what Work had already done since §17. If the answer is "one server", the app window moves to `ragon web`'s socket, and this policy layer stays as the guard for the stdio fallback (§2.1).

### 18.4 Verified on this machine

No live service was touched: no model server, no installed `ragon`, a scratch `HOME` for everything run.
- **`desktop/electron`.** 29 vitest cases, 8 of them new:
  - `policy.test.ts`: the policy is `ragon web`'s file, all 26 conformance cases, `namedPrompt`, and a bad policy refused.
  - `ask.test.ts`: folders, links, a planted link out of the root, uploads within the caps and names, the prompt composed by a scripted `ragon` in the Ask root, and an Ask thread through `AppServer` end to end. The end-to-end case checks the composed prompt, a fresh folder, the sandbox, the link, an attachment, a resume kept in its folder, a prompt on resume refused, Work left alone, and `feedback/upload` refused.
  - The typecheck and `npm run package` pass. The policy is in the main bundle, and the credentials test ran against the built bundles.
- **The real app.** `npm run e2e`: the packaged main process under Playwright-Electron, windows hidden, a scripted `ragon`, a scratch `HOME`. Two runs:
  - Work opens on `app://-/index.html#work`.
  - Clicking Ask, attaching an image and sending sent `thread/start` with the composed prompt, the sandbox and a folder under the Ask root. `ask/thread-1` linked to that folder, and the image was in it and went out as `localImage`.
- **`desktop/ui`.** 26 vitest cases (4 new: the app host's upload over the channel, base64 of a large file, the `view` message, the host check), the typecheck and the production build.
- **`ragon-rs/web`.** `cargo test` in a copy, with and without the UI embedded: 30 unit tests and 14 server tests.
  - **New unit tests:** the QR link, the address order, the SVG and the terminal drawing, the Devices page's escaping, its loopback case, the folded addresses, and the eight-digit check.
  - **New server tests:** `/devices` end to end, and a loopback server issuing no code. The end-to-end test covers the owner's page, a code and its QR codes, a foreign `Origin` refused, the prefilled pairing form, and an HTML-looking code not shown back. It then pairs from the code, checks that the paired device is refused the page, a new code and a revoke, and has the owner revoke it. The three new routes are in the credential test.
- **The QR codes decode.** A code drawn by `qr::svg` and by `qr::terminal` for `http://192.0.2.10:3100/pair?code=01234567`, rendered to pixels and read by OpenCV's `QRCodeDetector` in a scratch virtualenv, gave back exactly that link, both ways.
- **The Python suite.**
- **Not run:** the launcher's tests in a Codex export. `ragon-rs/src/` did not change; the only change the export sees is the new `qrcodegen` dependency of `ragon-web-server`, a leaf crate that Cargo adds to the workspace's lock at the next `codex build`, which does not use `--locked`. Its MIT licence is on the allow-list of the workspace's `deny.toml`, and that file bans nothing it brings.

### 18.5 Still not built

- **The menu's Ask window.** It still uses `ragon web` and its app-server, a second server on the same home (§18.3). Folding it into the app window is the §2.1 decision. (Built in §18.6.)
- **Images, apps and voice in the UI.** `/images/*`, `/api/apps` and `/api/transcribe` still answer 501; out of scope by the user's decision (§17). (Images and voice built in §19.2; `/api/apps` still answers 501.)
- **Phase 0's measurements**, question 1 above all.

### 18.6 One window, one app-server in the app (branch `desktop/one-app-server`)

> **Superseded in part:** one app-server stands; one window does not. The app has several windows on that app-server, routed per window and vetted as before, with one composer per thread ([DESKTOP_ELECTRON](./DREAMFERENCE_RAGON_DESKTOP_ELECTRON.md) §13, the owner's decision).

**The decision (the user's).** The menu's Ask no longer opens a window of its own on `ragon web`: it brings the app window forward on Ask. While the app runs, the app itself starts exactly one app-server on `~/.ragon`, the app window's own over stdio (§18.1). This settles §2.1 for the desktop app the first way §17 named, the policy in the main process; `ragon web` keeps its own app-server on its socket, for browsers and phones, unchanged.

**What changed in the app** (`desktop/electron/`):
- **One window.** `main.ts` keeps one `BrowserWindow`, the app window, and one `showView(view)`. The menu's Ask and Work (Ctrl+1, Ctrl+2), the tray's, `ragon app` alone and a second instance with no target bring it forward on that view (restored if minimised), sending the page the `view` message of §18.1 when it is already open; `--work`, a folder, a thread or a `ragon://` link open it on Work. No path opens a second window.
- **Removed:** `chat.ts` (the Ask window), `web.ts` (finding, starting and stopping `ragon web`, the one-time sign-in link) and `web.test.ts`; the `chat` block of `app.json`; the page-to-main `work/open-chat` message, which nothing sent since the header's Ask button went (§18.1); the Windows special case, since every platform now does what Windows did. The window's title is "Ragon" (it opens on Ask; the page's `<title>` was already that).
- **Unchanged:** the policy layer, the Ask folders and uploads (§18.1), `ragon app`'s arguments (`ragon-rs/src/app.rs`: comments, the help line and one error message only, which no longer name `ragon web` or an Ask window), and `ragon web`, including `open --print-url`, which no longer has a caller in the app.

**The audit's app mode** (`dreamference/audit/egress_audit.py`). The app is started with no argument, as `ragon app` starts it (it was `--work`), in the same hidden session: the window on Ask, its app-server started, quit after 40 seconds. Port 3100 is no longer on the app session's allowlist, which is now the `exec` session's: a regression to a window on `ragon web`, and so to a second app-server, shows as a connect to 3100 and fails the audit (a test plays one back). `--app`'s help text and `docs/admin.md` say "its window hidden". The macOS workflow's start check runs the same audit session and needed only its comment changed.

**What this does and does not settle.** The second server the app itself created is gone. A `ragon web` the user runs (`ragon web start`, or on an advertised node) beside the app is still a second app-server on the same home, so §18.3's risk, two writers on one rollout file when the same thread is open in both, remains for that case, and Phase 0 question 1 is still open for it.

**Verified on this machine**, a scratch `HOME` for everything run, no live service touched:
- `desktop/electron`: typecheck; 25 vitest cases (`web.test.ts`'s 5 gone with its module); the bundle check now anchors on the app-server's command line and checks that no `--print-url` is left in the main bundle. `npm run package` passes.
- `npm run e2e` (Playwright-Electron, scripted `ragon`, hidden windows): 3 passed. The new case starts the app with no argument and checks that the one window is on `app://-/index.html` (Ask), that the File menu's Work and then Ask switch it (`#work`, then `#`) with `BrowserWindow.getAllWindows()` still 1, and that the stand-in `ragon` was run with `app-server` exactly once and never with `web`.
- `desktop/ui`: 26 vitest cases and the typecheck (one comment changed).
- The Python suite (`-k "not codex_branded_builder"`): 931 passed, 89 skipped.
- Not run: the launcher's `cargo test` (`app.rs` changed in comments and two message strings only), and `ragon-admin audit egress --app` against a real build (it needs the installed `ragon` and a model server).

---

## 19. What was built (image search and voice)

Built: the minimal image search and voice of §6 and §7 (the user's decision).

### 19.2 Image search and voice, minimal

**Image search (§6).**
- `ragon-admin images start [--no-siglip]|stop|status` (`ImageSearchSidecar`): the existing service on `dreamference-sidecars`, its store and secret in `~/.config/dreamference/image-search`, the vision re-rank pointed at the served model's id from `/v1/models`. SigLIP is best-effort and skipped with `--no-siglip` or when it does not start. A container found on another network is replaced. The spec named only `images mcp`; `start|stop|status` are added, as the Google service has them.
- `ragon-admin images mcp` (`chat/image_search_mcp.py`) serves `image_search(queries, count)` over stdio. It runs before the sandbox check, which could print. `initialize` and `tools/list` touch nothing. A call refuses at a configured `on` (`DreamferenceConfig.resolve_airgapped_level`) or when the parent session sealed itself at `on`, reading `ragon-airgapped`'s seal folder as `ragon-apps`' `sealed_by` does: a second reader of that format, a drift risk like the byte-identical Rust copies.
- The launcher declares it (`ragon-rs/src/images.rs`, `-c mcp_servers.ragon_images.…`) on a node with a local model server, the configured air gap not `on`, the secret file present, `~/.local/bin/ragon-admin` linked, and `DREAMFERENCE_RAGON_IMAGES` not off. Nothing is probed over the network at start.
- The service now answers `![title](/images/<id>.jpg)`. `ragon web` serves `/images/<16 hex>.jpg` read-only from the store (`ragon-rs/web/src/sidecars.rs`; links refused, any other name 404), behind the credential like every route. The app's `app://` handler does the same (`imageFile` in `app-protocol.ts`). The page's Markdown shows those images and still no other (`markdown.ts`). The gallery and lightbox were not rebuilt: images are inline.

**Voice (§7).**
- `ragon-admin voice start|stop|status` (`SpeechSidecar`): speaches on the CPU on `dreamference-sidecars`, `127.0.0.1:8100`, its model in the volume `dreamference-stt-cache`, one on another network replaced.
- `ragon web` answers `POST /api/transcribe` (audio only, 25 MB at most): a multipart request to the sidecar on loopback, `{text}` back, 503 naming `ragon-admin voice start` when nothing answers. The app's main process takes `voice/transcribe` over the one IPC channel and does the same with `fetch` (`voice.ts`).
- The composer has a 🎤 button where the page may record (`canRecord`: a secure context with `getUserMedia` and `MediaRecorder`), so in the app and in `ragon web` on this machine, never on a phone over plain HTTP (§14.2). It toggles (press, speak, press again) rather than push-to-talk; the text goes into the draft, never sent.
- The app grants the microphone to its own page only, audio only (`grantsRequest`/`grantsCheck` in `egress.ts`); every other permission is still refused.

### 19.6 The user's answers, built the same day

- **`server start` on a node starts image search and voice when they are missing,** as it starts the Google service (`ensure_on_node` on each):
  - Nothing happens on a client, or where the container exists, running or not.
  - Image search starts without SigLIP and without waiting for its health route: its first boot installs Pillow. Speech-to-text fetches its model detached (`docker exec -d`).
  - The vision re-rank is told the id the server will serve, the checkpoint's repository (`vision_model`), since the model is not answering yet.
  - A failure, or an exception from Docker, is a warning naming the command to retry (`ragon-admin images start`, `ragon-admin voice start`). It never blocks the model start; a test runs `server start` with both failing and checks that the load still runs.
  - conftest stubs both starts, as it stubs the Google service's (`REAL_IMAGE_SEARCH_START`, `REAL_SPEECH_START`).
- **`audit egress --web` has an image search turn** (§9):
  - Where image search is set up (its secret file exists and a `ragon-admin` is found), the scratch home is laid out as a node, with a copy of the secret and a `ragon-admin` that runs the code under audit (fixed: a link to `~/.local/bin/ragon-admin` ran main's environment, which has no `images mcp`), so the traced session is offered the tool; that server is first asked for its tools, and one that cannot start is reported as `not asked: …`.
  - A second question asks for one picture, and its MCP server is traced with the app-server.
  - Port 8768 is on the allowlist of every session, and the report and the JSON say `Image search turn: answered with an image from the image search service (<id>.jpg) | answered with an image line, but … not in the image search store | answered, without an image | not asked: … | no answer | skipped: …`; an image counts only when the store holds it. `ragon-admin images status` asks the sessions' `~/.local/bin/ragon-admin` the same way and says `offered but broken` when it cannot serve the tool.
  - The turn is reported, not judged: the verdict stays the egress one.
  - Not run against the real sidecar.
- **Verified:** the Python suite, 907 passed, 91 skipped, with the same one failure from the environment. One run also failed `test_night_shift.py::…bundled_ragon…`, a timing race on a loaded machine; it passed on three reruns and on the full rerun. No Rust or front-end code changed in this part.

