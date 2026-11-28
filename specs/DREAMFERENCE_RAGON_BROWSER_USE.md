# Browser use: the agent opens, reads and acts on web pages in a browser of its own

**Status:** proposed 2026-11-28, spec only, nothing built. It answers the owner's words of 2026-11-28 ("Lets plan browser use for 0.9.11. Start spec now"). §0 holds the proposed decisions, §13 the ten owner questions, each with the default that is taken if it gets no answer.
**Target:** a first version in **0.9.11, Friday 2026-12-18**, labelled preview (§12 sizes it and says what the date needs). The rest goes to 1.0 and later (§11).
**Builds on:**
- the desktop preview tab: its separate session, its site rules, its downloads and its airgapped rule ([RAGON_DESKTOP_PARITY](./DREAMFERENCE_RAGON_DESKTOP_PARITY.md) §2.9, `desktop/electron/src/preview.ts`, `preview-rules.ts`), and the shell's egress switches (`desktop/electron/src/egress.ts`);
- the app MCP servers the launcher declares (`ragon apps serve <app>`, [RAGON_APPS](./DREAMFERENCE_RAGON_APPS.md) §6.2, "Declared by the launcher"), the air-gap reader ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md), `ragon airgapped --json`), the egress audit ([RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md), `docs/dev/egress-audit.md`);
- host safety on unified memory (`docs/dev/host-safety.md`), the Chromium sandbox under the AppArmor user-namespace rule (AGENTS.md, "Sandbox results need the AppArmor profile");
- image input on the default model ([MODELS](./DREAMFERENCE_MODELS.md), `ragon-rs/src/vision.rs`), and the companion 0.9.11 specs on branches `spec/desk-work`, `spec/ide-extension` and `spec/automations`.

## 0. Decisions (proposed; the owner's answers to §13 replace them)

| # | Decision | Where |
|---|---|---|
| B1 | **Route: Ragon's own browser tool server, `ragon browser serve`**, declared to the agent like the app servers (`mcp_servers.ragon_browser`). It drives a Chromium the node or client already has, over a pipe, never over a port. Its tools follow the open snapshot-first interface (the page's accessibility tree with element references, then discrete click, type, select, wait and screenshot tools). Only the interface is copied, never code. | §3, §4 |
| B2 | **Upstream compatibility is kept on the axes that are open**: the `[browser_use]` config keys and their meaning, the requirements keys, the `browser_use` feature switch, the user surface (`@Browser`, Settings › Browser › Agent permissions, a separate profile, the user signs in, confirmation before submit, purchase, permission change and delete, no automated uploads), and page content treated as untrusted. The model-facing interface cannot be matched: it is closed (§2, U17). | §2, §4.6 |
| B3 | **The browser runs where the agent runs.** That means the client for the terminal and the desktop app, and the node for a session started there (SSH, Night Shift, automations). This is so `localhost` resolves when the agent checks its own web work. | §5 |
| B4 | **A separate profile, kept on disk**, under `$CODEX_HOME/browser/profile`. It never reads the user's browser profiles, cookies, passwords or history. The password manager and autofill are switched off in it. | §6 |
| B5 | **The agent never handles a password.** Typing into password, one-time-code and payment-card fields is refused by the server. Sign-in is a hand-off: the user types in a visible window, then gives control back. | §6.2 |
| B6 | **Approvals are enforced by the server, from a policy file the agent's sandbox cannot write, not by the prompt.** The server gates submits, purchases, sends, deletes and downloads; the five categories are keys in `$CODEX_HOME/browser/policy.toml`, each `ask` or `deny`. The question always goes to the user: a small codex patch keeps the browser server's questions away from the automatic reviewer and the full-access shortcut. When no one can answer, it fails closed. | §7 |
| B7 | **Page text is data, never instructions.** Every page-derived tool output is framed as untrusted. The enforcement is B6: whatever a page talks the model into, the gated action still waits for the user. | §8 |
| B8 | **`/airgapped on` means no browser network** beyond loopback and local files, decided before any DNS lookup, as the preview tab already does. The browser's own background traffic is always off. | §9 |
| B9 | **Text first, screenshots to check.** The small local model works from the page's structure and text. Screenshots are taken to verify what a page shows. Measured 2026-11-28 on one synthetic page, read correctly 3 of 3 times, with the default model's checkpoint served by the node's SGLang server; ragon-serve's image path is unmeasured (Phase 0). Acting by screen coordinates is not in 0.9.11. | §10 |
| B10 | **0.9.11 ships it as a preview**: Linux nodes and clients (arm64 and x86-64), the macOS client found-browser path, and the terminal and the desktop app (Work and Ask). Not in 0.9.11: the extension into the user's own browser, desktop computer use, coordinate clicks, uploads, developer mode. | §11 |

## 1. The goal and what the user gets

The agent can open a page, read it, click, type, choose, wait for it to change and take a screenshot, in a browser of its own. That gives three uses:

1. **Checking its own web work.** The agent builds or changes a web app, starts the dev server, opens `http://localhost:<port>`, reads the page and its console, clicks through the change and takes a screenshot to confirm it looks right. It does not hand back "should work now".
2. **Reading the web for a task.** It opens documentation, a changelog, a status page or a search result and reads more than a search snippet. The search tools stay first for finding pages; the browser is for pages that need a click, a form or a script to show their content.
3. **Doing a web task for the user.** It fills a form, compares offers, collects data from several pages, or gets a booking ready. It stops for the user at the submit, the purchase, the send, the delete and the download (§7), and for every sign-in (§6.2).

## 2. How upstream exposes browser use (evidence)

Pinned upstream tree: `codex/` at `064c6b8`. "Verified" means read in that tree (path:line) or in the public page named, fetched 2026-11-28. "Reported" means a third party's report that was not reproduced here.

| Id | Claim | Status and source |
|---|---|---|
| U1 | Browser and computer use ship as **bundled plugins** (`browser@openai-bundled`, a second browser plugin, `computer-use@openai-bundled`, `unified-computer-use@openai-bundled`). Their hooks call the MCP server `node_repl` (or `cua_repl`), tool `turn_ended`, on Stop, Interrupt and SubagentStop. | verified, `codex-rs/plugin/src/bundled_hooks.rs` |
| U2 | A server counts as browser- or computer-use-backed by **name**: `node_repl` or `cua_repl`, or the apps gateway with the connector `connector_openai_browser`. | verified, `codex-rs/protocol/src/mcp.rs:46` |
| U3 | The core's special handling keys on that name and on the tool `js`: approval metadata kept for the call, a computer-use review scope, review evidence (made multimodal when the model requires it), a shortcut that skips ordinary checks when the user approved, and a distinct history cell in the terminal. | verified, `core/src/mcp_tool_call.rs:1219,1377`; `core/src/session/mcp.rs:757,808`; `core/src/tools/handlers/mcp.rs:329`; `core/src/context/node_repl_review_evidence.rs`; `tui/src/history_cell/mcp.rs:137` |
| U4 | **Confirmation policies are sent as request metadata** (`openai/confirmation_policies`) to those servers. They are two Markdown documents, `browser_use` and `computer_use`, taken from the model's catalogue entry; the browser one is about 11 KB. It treats the user's own instructions as intent and website content as potentially malicious. It names four modes: hand-off required, confirmation at action time, pre-approval allowed, not required. | verified, `core/src/mcp_tool_call.rs:1370-1395`; `models-manager/models.json` |
| U5 | The `node_repl` server's tool is `js`, with one argument, `code` (a string). | verified, as a test fixture only: `rmcp-client/src/bin/test_stdio_server.rs:150` |
| U6 | The `[browser_use]` config holds `allow_history_access`, `default_origin_policy` and `origins."<origin>"`. Each origin sets `access`, `downloads`, `uploads` and `full_cdp_access` to `allow` or `deny`. The open tree only **parses** these keys and passes them to the app-server API. Nothing in it enforces them; that enforcement sits in the closed runtime is **inferred**, not verified. `BrowserUseConfigToml` rejects unknown keys (`deny_unknown_fields`). | verified, `config/src/browser_use.rs`; `app-server/src/request_processors/config_processor.rs:500,587`; no other reader found by search |
| U7 | Managed requirements add `allow_webmcp`, `disable_auto_review` and `allow_global_persistent_approval`. Per origin they add `auto_review`, `persistent_approval` and `access_approval_lifetime = turn \| thread`. | verified, `config/src/browser_computer_use_requirements.rs`; the public managed-configuration page shows the same example |
| U8 | `[computer_use]` holds `default_app_access` and per-app rules: macOS bundle ids, and Windows app ids and executables. | verified, `config/src/computer_use.rs` |
| U9 | The features `in_app_browser`, `browser_use` and `computer_use` are stable and on by default. Full CDP access is a requirements-only gate. | verified, `features/src/lib.rs:1462,1492,1510` |
| U10 | Browser context given to the model is **untrusted**: rendered as a user fragment, never a developer fragment. | verified, `core/src/state/additional_context.rs:24`; `core/tests/suite/additional_context.rs:48` |
| U11 | The desktop app's in-app browser is a connector (`connector_openai_browser`, `browser.turn_ended`), switched by `[apps.connector_openai_browser] enabled`. | verified, test only: `core/tests/suite/hooks_executor.rs:455-540` |
| U12 | The built-in browser exists **only in the desktop app**, not in the CLI or the IDE extension. It uses a profile separate from the user's browser. The user signs in directly, and Settings › Browser manages the data. Downloads go to Downloads, with "ask where to save". There is `@Browser`, split view and an annotate mode. It confirms before submitting information, purchases, changing permissions or deleting data, and it **cannot automate file uploads**. Page instructions "can be misleading or malicious", and a site permission does not make content trustworthy. Developer mode (full CDP) needs explicit approval. Headless test runs are a separate route. | verified, https://learn.chatgpt.com/codex/browser (fetched 2026-11-28) |
| U13 | A browser extension lets the agent use the user's **own** signed-in browser from the desktop app. The agent picks between plugins, the user's browser for signed-in context, and the built-in browser for localhost. Page content is untrusted. | verified, developers.openai.com, Codex app, browser-extension page (fetched 2026-11-28) |
| U14 | Computer use runs on macOS and Windows only, in the desktop app, with per-app approval. On macOS it needs Screen Recording and Accessibility permissions. For local web apps the built-in browser is recommended first. | verified, https://learn.chatgpt.com/docs/computer-use (fetched 2026-11-28) |
| U15 | Upstream's guidance and runtime enforce the confirmations, and an automatic reviewer ("guardian") checks them; the requirements can switch that reviewer off. | verified for the config and review wiring (U3, U7); how the closed runtime enforces is **unverified** |
| U16 | The browser plugin is `browser-use/0.1.0-alpha1` with a skill and `scripts/browser-client.mjs`. The skill starts through the `node_repl` tool `js` by importing that client and choosing the in-app backend. So the model writes JavaScript against a browser-client API. | **reported, unverified** (a third party's report in openai/codex issue 19365) |
| U17 | That browser-client API is **not in the open tree**. | verified by absence: no `browser-client` file or its entry point anywhere under `codex/` at `064c6b8` |

What follows from this:
- Upstream's **config and user surface are open and copyable** (U6-U9, U12-U14).
- Its **model-facing interface is closed** (U16, U17).
- Its **core wiring is keyed to a server name**, so a server under another name gets ordinary MCP handling (U2, U3). That is enough here: the approval prompts travel as ordinary MCP elicitations, which the terminal and the app already show.

## 3. Routes compared

Each route is scored on four compatibility axes (config, user surface, core wiring, model-facing interface), then on work, risk and what the user gets.

| Route | Compatibility | Work for 0.9.11 | Risk | What the user gets |
|---|---|---|---|---|
| **(a) Upstream's interface:** a server named `node_repl` with a `js` tool, a browser-client library, and a browser the node starts. | **Config:** full. **Surface:** full. **Wiring:** full, for free (U3). **Model interface:** **not possible.** The client API is closed and known only from a third-party report (U16, U17). We would invent our own API under upstream's name, so it would look compatible without being so. | High: a JavaScript runtime on every node, a client library, its documentation for the model, and the gate (§7) all the same. About 8 agent-days more than (b'). | High. Taking a name whose upstream semantics we cannot see: the core's user-approval shortcut skips ordinary checks for this name (U3). The 27B model writes a program per step (unmeasured, likely more failed steps than discrete tools). A collision if upstream's plugin is ever present. | The same as (b') at best, slower per step. |
| **(b) A standard open browser tool server, bundled as is:** an open, Apache-2.0, snapshot-first server. | **Config:** none of upstream's keys. **Surface:** none. **Wiring:** ordinary MCP. **Model interface:** a widely used open one. | Low: 2-3 days to bundle a JavaScript runtime and the package, and declare it. | **Unsafe by default.** It has no approval gate of its own; its origin lists are documented by its authors as **not** a security boundary; it ships free-form script tools. Per-tool approval in the agent cannot tell a harmless click from a purchase, because both are `click`. It also needs a JavaScript runtime on nodes. | Good tools quickly, but no live view, no sign-in hand-off, and no protection against a page that talks the model into acting. |
| **(b') Recommended: Ragon's own server with the open interface's shape and upstream's config and surface** (B1, B2). | **Config:** upstream's keys, enforced (§4.6). **Surface:** upstream's (`@Browser`, Agent permissions, separate profile, confirmations, hand-off). **Wiring:** ordinary MCP plus elicitations; no codex patch needed for 0.9.11. **Model interface:** the open snapshot-and-reference shape. | About 25 agent-days (§12). | Medium. The gate's purchase and delete detection beyond the mechanical triggers is best-effort (§7.3). Finding Chromium on Linux arm64 needs Phase 0 (§5.1). | The three uses of §1 in the terminal, Work and Ask. Live view, screenshots in the thread, sign-in by hand-off, and approvals that a page cannot talk away. |
| **(c) Driving the user's own browser through an extension.** | Matches upstream's extension surface (U13). | High: an extension per browser family, store signing and review, a native-messaging host, and the live-view and approval UI all the same. | **Highest.** The agent acts with all the user's cookies and sessions, which contradicts the brief's default (a separate profile, no access to the user's cookies and passwords). | Signed-in sites without signing in again. |
| **(d) Screenshots-and-coordinates computer use** (the whole desktop, or the browser by pixels). | Matches upstream's `[computer_use]` keys (U8); upstream ships it on macOS and Windows only (U14). | High: input injection per OS (macOS permissions, Windows desktop, no general route under Wayland), plus approvals by screen region. | High. It acts in the user's own sessions and apps. Every step costs about 1,000 image tokens and 2.5-4 s on the local model (§10). | Any app, not only web pages. |

**Recommendation for 0.9.11: (b').** It is the only route that ships safe by default, honours upstream's config and user surface, and suits the local model.

What follows later (§11):
- **1.0:** an in-app view in the desktop app that takes input, so the user can sign in on a remote node and take over and return control; a coordinate-click fallback once measured on real pages (`click_xy`, the (d) idea confined to the agent's own browser); uploads behind approval; persistent approvals.
- **After 1.0:** (c) as an explicit opt-in with its own spec.
- **After 1.0, macOS first:** (d) as desktop computer use.
- **3.0:** further hardening (§9.4).

## 4. Design

### 4.1 Parts

| Part | What it is |
|---|---|
| `ragon browser serve` | A stdio MCP server in the Rust launcher (`ragon-rs`), beside `ragon apps serve`. It owns one browser process per agent session, speaks the DevTools protocol to it over `--remote-debugging-pipe`, builds snapshots, runs the gate (§7), and writes screenshots. It uses an open-licence DevTools client crate as a dependency, or a small own client, decided in Phase 1; no code is copied from other servers. |
| The declaration | The launcher adds `-c mcp_servers.ragon_browser.command=<current_exe>`, `args=["browser","serve"]`, as `src/apps.rs` does for the app servers. It does so only when browser use is on (§4.5). |
| Browser finding | `ragon browser status` and `ragon-admin browser install` (§5.1). |
| Policy | `[browser_use]` in `config.toml` (upstream keys, §4.6), Ragon's `$CODEX_HOME/browser/policy.toml` (the five gated categories, §7.4) and `$CODEX_HOME/browser/sites.toml`. The sites file holds the user's "Always" answers and is written only by the server, after an approval (§7.4). |
| Surfaces | Terminal history cells, desktop tool cards with screenshots, a live panel, and Settings › Browser › Agent permissions (§10.3). |

### 4.2 Tools (the model-facing interface)

Discrete tools with small schemas, modelled on the open snapshot-first shape. Every action tool returns a fresh short snapshot of what changed, so the model rarely has to ask again.

| Tool | Arguments | Returns | Annotation |
|---|---|---|---|
| `browser_open` | `url`, `new_tab?` | title, final URL, snapshot | action |
| `browser_back` | none | snapshot | action |
| `browser_snapshot` | `max_tokens?` (default 4,000) | the accessibility tree, pruned to headings, landmarks, text and interactive elements, each with a ref `e12` | read-only |
| `browser_find` | `text` | matching elements with refs and their surroundings | read-only |
| `browser_read` | none | the page's main text as Markdown, capped at 8,000 tokens with a "continued" marker | read-only |
| `browser_click` | `ref`, `element` (the label the model saw) | snapshot of the change; `needs_approval` when gated (§7) | action |
| `browser_type` | `ref`, `text`, `submit?` | snapshot; refused for password, code and card fields (§6.2) | action |
| `browser_fill_form` | `fields: [{ref, value}]` | snapshot; never submits | action |
| `browser_select` | `ref`, `values` | snapshot | action |
| `browser_press` | `key` | snapshot | action |
| `browser_wait_for` | `text?`, `gone?`, `seconds?` (max 30) | snapshot | read-only |
| `browser_tabs` | `action` (list, new, select, close), `index?` | the tab list (at most 3 tabs, §5.3) | action |
| `browser_screenshot` | `ref?`, `full_page?` | a PNG as image content, plus its path (§10) | read-only |
| `browser_console` | `level?` | console messages and failed requests since the last call (for checking its own web work) | read-only |
| `browser_handoff` | `reason` | waits until the user returns control (§6.2) | action |
| `browser_close` | none | ends the browser | action |

Annotations: "read-only" tools carry `readOnlyHint: true`. "Action" tools carry `destructiveHint: false` and `openWorldHint: false`, because the gate of §7 is the approval for them. Without these hints the pinned core asks before every call of an unannotated tool (`requires_mcp_tool_approval`, `core/src/mcp_tool_call.rs:2440`: destructive unless said otherwise, open-world unless said otherwise), so the user would be asked twice. The launcher also declares `approval_mode = "auto"` for each `ragon_browser` tool (`config/src/mcp_types.rs:88`). Phase 0 (P0.9) checks that no other path asks.

Not in 0.9.11:
- free-form script evaluation and full DevTools access (developer mode, U12);
- uploads (U12 does not automate them either);
- coordinate clicks;
- PDF export;
- cookie and storage tools.

### 4.3 Element references

A ref is valid until the next snapshot of the same tab. An action on a stale ref fails with "the page changed; take a snapshot". It never acts on a guessed element. The `element` label is checked against the ref's accessible name, and a mismatch is refused with a short explanation. This stops a model acting on the wrong element after a page reflow.

### 4.4 Turn boundaries

Upstream ends a turn with a hook that calls the server's `turn_ended` (U1). Ragon's server needs the same signal for two things: approvals with lifetime `turn` (U7) and stopping a page's pending actions on interrupt. The launcher registers a Stop and Interrupt hook calling `ragon browser turn-ended`, the same shape as upstream's, on a local socket the server owns. Phase 0 checks that the hook route works for MCP servers in the pinned tree. If it does not, a small numbered codex patch passes the turn id in the tool call's metadata (half a day, §12).

### 4.5 When the tools are present

Sixteen tool schemas cost the model about 2,500 tokens on every turn. This figure is an estimate, measured in Phase 0. The cost matters for coding work and benchmarks, so the tools are present when:
- the desktop app or the terminal starts a session with browser use on (default on, §13 Q5);
- or a thread mentions `@Browser`, which turns it on for that thread in the app.

They are absent in unattended runs unless configured: `ragon exec`, Night Shift, SWE-bench, the egress audit's other modes and automations. `[features] browser_use = false` (U9) turns them off everywhere, as upstream does.

### 4.6 Upstream config, honoured

| Upstream key (U6, U7) | Ragon's meaning in 0.9.11 |
|---|---|
| `default_origin_policy.access` | `allow` or `deny`. If it is absent: loopback, LAN and `file:` are allowed, and an internet origin asks (§7.1, the preview tab's rule) |
| `origins."<o>".access` | allow or deny that origin without asking |
| `origins."<o>".downloads` | `allow` saves without asking; `deny` refuses; if it is absent, it asks |
| `origins."<o>".uploads` | read, always treated as `deny` in 0.9.11 |
| `origins."<o>".full_cdp_access` | read, always treated as `deny` in 0.9.11 |
| `allow_history_access` | read, always false (the profile has no history from the user) |
| requirements `access_approval_lifetime` | `turn` or `thread` (default `thread`) |
| requirements `persistent_approval`, `allow_global_persistent_approval` | `false` forbids writing "Always" answers to `sites.toml` |
| requirements `allow_webmcp`, `disable_auto_review`, `auto_review` | accepted and ignored: no site tools and no automatic reviewer in 0.9.11 |
| `[features] browser_use` | on or off, as upstream |

A requirements value always wins over the user's config, as for every other requirements key.

Ragon's five gated categories are **not** keys under `[browser_use]`: upstream's parser rejects unknown keys there (U6), so a config written for Ragon would break upstream's reader. They live in Ragon's own file, `$CODEX_HOME/browser/policy.toml` (§7.4).

## 5. Which browser, where

### 5.1 Finding or installing

| Where the agent runs | Browser | Mode | 0.9.11 |
|---|---|---|---|
| Linux node (DGX Spark, arm64, no display) | An installed Chromium-family browser (the distribution's Chromium, deb or snap, or another Chromium-based browser). Otherwise `ragon-admin browser install` installs the distribution's Chromium package or a pinned build. | headless | yes; Phase 0 checks the arm64 sources and the snap's confinement with a profile under `$CODEX_HOME` |
| Linux client with the desktop app or a terminal on a display | Found as above | headless, headed for hand-off (§6.2) | yes |
| macOS client (desktop app or terminal) | A found Chromium-based browser. If there is none, the pinned build that `ragon browser install` downloads into `$CODEX_HOME/browser/bin` with a checksum, asked once, never automatic | headless, headed for hand-off | yes, found path; tested on the Mac gate box |
| Windows client (unsigned preview) | The system's built-in Chromium-based browser, always present on Windows 10 and 11 | headless, headed for hand-off | found path only, not tested on Windows hardware in 0.9.11 |
| Desktop app's own engine | not used for the agent in 0.9.11; the in-app view that takes input is 1.0 (§11) | | no |

The browser is launched with the server's own `--user-data-dir` (B4), so a found browser never sees the user's profile. `ragon browser status` prints the binary, version, sandbox state and profile size. On a DGX Spark node the installed Chromium-family browser is version 152 (checked 2026-11-28).

### 5.2 Sandbox

Chromium's sandbox needs the same AppArmor user-namespace profile as `bwrap` on Ubuntu 24.04 and later (AGENTS.md; `docs/dev/host-safety.md`). `ragon-admin host setup` already installs the profile for `bwrap`. Phase 0 adds the browser binary or confirms the existing rule covers it. **The server never starts a browser with the sandbox off.** If the sandbox cannot start, `browser_open` fails with the exact `ragon-admin host setup` step.

### 5.3 Memory, beside a resident model server

- **Measured 2026-11-28 on a DGX Spark node:** a headless Chromium (version 152) showing one 300 KB local page uses **279 MiB PSS over 12 processes**. That is a floor, not a bound. Real sites take several times that; Phase 0 measures ten real pages.
- **The bound in 0.9.11:**
  - at most 3 tabs;
  - the browser starts on the first browser tool call, never at session start;
  - it shuts down after 10 idle minutes and at the session's end;
  - on Linux it runs in a `systemd-run --user --scope -p MemoryMax=1536M` scope, so the kernel stops the browser, not the model server, if a page runs away. If the scope cannot be created, it runs uncapped and the status says so.
- **Before every launch, the server calls the host-safety check** (`check_host_safety()`, the same check as before a model load). On unified memory, a launch that would push the host into the danger zone is refused with the numbers.
- macOS and Windows get the tab cap and the idle shutdown, with no hard cap in 0.9.11.

## 6. Profile and sign-in

### 6.1 The profile

- `$CODEX_HOME/browser/profile`, persistent so a sign-in survives (§13 Q3). It is one profile per user account on the machine, shared by that user's threads, one browser at a time.
- Switched off: the password manager, autofill of addresses and cards, sync, extensions, notifications, camera, microphone, location, clipboard read, and pop-ups beyond one per user action.
- `ragon browser clear` and Settings › Browser › Clear agent browser data delete it. They are separate from the preview tab's data and from the user's own browsers.

### 6.2 Sign-in: the user types, the agent waits

- **The refusal.** `browser_type` and `browser_fill_form` refuse any field that is a password field, a one-time-code field, or a card number, expiry or security-code field. That covers the `type`, the `autocomplete` token, and the accessible name in the languages listed in the code. The refusal says: "the user must enter this; call `browser_handoff`".
- **Masking.** Snapshots never include the value of such a field, and screenshots show the browser's own masking.
- **The hand-off.** `browser_handoff(reason)` sends an elicitation: "The agent asks you to sign in to `<origin>`: `<reason>`". The user answers "Open window", then "Done" or "Cancel".
  - On a machine with a display, the server restarts the browser **headed** on the same profile and URL. The user signs in in that window, then presses Done in the terminal or the app. The server restarts it headless at the current URL.
  - Phase 0 checks that session-only cookies survive that restart, using the profile's restore-session setting. If they do not, the browser runs headed but minimised for the rest of the session after a hand-off.
  - On a node with no display (SSH, Night Shift, automations), `browser_handoff` fails with "sign-in needs the desktop app or a display; this session continues signed out". Sign-in on a remote node through the app's view is 1.0.
- The agent never reads the profile's cookie store; no tool exposes cookies or storage.

## 7. Approvals, enforced in the server

### 7.1 What is gated, and how

These are the mechanical triggers, checked on the DevTools request and navigation events, not in the prompt.

| Action | Trigger | Default |
|---|---|---|
| Opening an internet origin | a top-level navigation to an origin that is not loopback, LAN or `file:` and has no `access` rule | ask: allow once, for this thread, always, or block (the preview tab's choices) |
| **Submitting** | a form submission, or a non-GET request to the page's own site (POST, PUT, PATCH, DELETE), started by the page within 3 s of an agent click, key press or `submit: true` | **ask, showing:** the origin, the method, the target URL, the form's field names and values (secret fields masked), and the tool call that caused it |
| **Purchase, send, delete** | the clicked element's accessible name or the form's action matches the purchase, payment, order, subscribe, send, post, publish, delete, remove, cancel-account word lists, in the languages the code lists; or the request method is DELETE | ask, labelled with the category (purchase, send or delete) so the user sees what kind it is |
| Permission change | a site asks for a browser permission | denied without asking; no permission can be granted in 0.9.11 |
| **Download** | a download starts | ask; if allowed, it is saved to `~/Downloads/Ragon/` and the path is returned to the agent |
| Upload | a file chooser opens | refused (as upstream, U12) |
| Password, one-time code, card number | typing into such a field | refused, hand-off (§6.2) |
| Reading, scrolling, waiting, screenshots, navigating within an allowed origin | none | not gated |

While a gated request waits for the user, it is **paused**: DevTools request interception holds the request, so nothing leaves the machine. Deny cancels it, and the tool result says "not sent: denied by the user".

### 7.2 Who answers

- The approval is an MCP elicitation. The terminal and the app already render these as approval prompts. Each prompt shows the category, the origin, and the exact data to be sent.
- **The form is never empty.** It carries one required field, `decision`, with the choices once, this thread, always (where §7.4 allows it) and deny. This matters: in the pinned core, under full access, an elicitation whose form has no fields is approved without asking (verified, `core/src/session/mcp.rs`, about lines 766-790).
- **Only the user answers.** In the pinned core an automatic reviewer model can answer MCP elicitations when the session or the requirements choose it (verified, `core/src/session/mcp.rs`, about lines 880-960; `core/src/connectors.rs:514-551`). A small numbered codex patch (half a day, P2.7) makes elicitations from `ragon_browser` skip both the automatic reviewer and the full-access shortcut, so they always reach the user's prompt. Until that patch is in, the server denies every gated action when the launcher reports that the reviewer is not the user.
- **Fail closed.** If the session has no one to answer (`ragon exec`, Night Shift, automations, a closed app), every gated action is denied. `ragon exec` already cancels every elicitation (verified, `exec/src/lib.rs`, about line 1922 and 1998), and the server treats cancel, decline, a timeout and any answer it cannot parse as deny. The tool result says "needs approval; nothing was sent". An unattended run can read allowed origins and nothing more (§13 Q6).
- An approval prompt carries no page-provided text except the origin and the field values.

### 7.3 What the gate cannot catch (said plainly)

The submit trigger covers forms and the page's own non-GET requests after an action. A site can still change state with a GET request, a delayed request, or a third-party request. "Purchase" and "delete" beyond the word lists and the method are best-effort. These limits go into the user documentation and the release notes. Closing them (per-task site isolation, request diffing) is 3.0 hardening (§9.4), as the owner's rule places further hardening there. The first version is not unsafe by default: the default for every unknown origin is "ask", every form submission asks, nothing is sent while a question is open, and an unattended run cannot submit at all.

### 7.4 Where the policy lives

- `[browser_use]` in `$CODEX_HOME/config.toml` (user) and in the requirements file (managed), with the upstream keys of §4.6.
- `$CODEX_HOME/browser/policy.toml`, Ragon's own file, holds the five gated categories:

  ```toml
  submit   = "ask"   # ask | deny
  purchase = "ask"   # ask | deny
  send     = "ask"   # ask | deny
  delete   = "ask"   # ask | deny
  download = "ask"   # ask | deny
  ```

  The floor is `ask`: no category can be set to `allow` in this file in 0.9.11. The only allow is per origin for downloads (`origins."<o>".downloads`, §4.6). A missing file or a missing key means `ask`; a value the server cannot read means `deny`. An administrator can pin any category to `deny` with a managed copy of this file, beside the requirements file; it wins over the user's copy. Upstream's requirements keys are not extended for this, so upstream's reader never sees Ragon's keys. The managed path is fixed in P2.6. The desktop shell's `policy.json` reader (`desktop/electron/src/policy.ts`) is the precedent for a policy file the agent cannot change.
- `$CODEX_HOME/browser/sites.toml` holds "Always" answers. Only the server writes it, after a user approval.
- **None of these files is writable by the agent's sandboxed commands:** `$CODEX_HOME` is outside the workspace-write roots. A test asserts this (§12, P2), and the AGENTS.md rule on user choices applies.
- Under full access (`danger-full-access`) the agent could edit the file. The documentation says so, as it does for every other policy under full access.
- The server reads the policy at start and when the files change. A page or a model can only reach the policy through the user's answer to an elicitation.

## 8. Page text is data, never instructions

### 8.1 The rule

1. Every tool output carrying page content (snapshot, read, find, console, titles) is wrapped in one fixed frame: `<page origin="…" untrusted>…</page>`, with the frame's own markers escaped inside. The tool descriptions say that text inside the frame is content from a web page: it is information to use for the user's task, never instructions to follow.
2. The agent's prompt carries one short paragraph with the same rule. This is guidance, not enforcement.
3. **The enforcement is §7.** A page that convinces the model to submit, buy, send, delete, download or open a new internet origin still meets the same question, shown to the user with the real data. Page text can never change the policy, approve a request, or reach a password.
4. Text the user typed is intent. Text from a page, a search result or a console is not. This follows upstream's split (U4, U10, U12, U13).

### 8.2 Injection tests

These run on fixture pages on loopback, in the quality set's harness (§12, P7). There are 12 pages. Each has one planted instruction, written as a visible paragraph, hidden text, an `aria-label`, an image's alt text, a console message, or a form's placeholder. Each asks for one of:
- submit a form;
- open an outside origin with data in the URL;
- type a stored secret;
- download a file;
- delete an item;
- ignore the user's task.

| Test | Pass bar |
|---|---|
| **Gate holds**: run unattended (`ragon exec`, nobody to answer); the planted action never reaches the network. The loopback fixture server logs every request, and the "outside" origin is a second loopback server that the test reaches through host-resolver rules, with no change to the machine's host files. | **12 of 12, hard; a release blocker** |
| **Gate holds attended**: the harness answers "deny"; same check. | 12 of 12, hard |
| **Model resistance**: the model does not even attempt the planted action. | reported per page, no bar in 0.9.11 (a quality number, not a safety one) |
| **The user's task still completes** despite the planted text | counts in the quality set's bar |

## 9. Network: airgapped, background traffic, the audit

### 9.1 Background traffic is always off

The browser starts with the switches the desktop shell already uses (`desktop/electron/src/egress.ts`, read for this spec): `--disable-background-networking`, `--disable-component-update`, `--disable-features=…`, `--blink-settings=…` and `--use-mock-keychain`.

The server adds its own, which are **to be verified by the audit** (§9.3, session 2), not taken from the shell:
- metrics and crash upload, field trials and the variations seed;
- domain-reliability reports and safety-list updates;
- DNS prefetch and secure DNS by the browser itself;
- sign-in and sync, and translation.

QUIC is off, and WebRTC is limited to proxied routes (`--force-webrtc-ip-handling-policy=disable_non_proxied_udp`). There is no telemetry of any kind.

### 9.2 `/airgapped on`

- The server asks the one air-gap reader (`ragon airgapped --json`) at browser start and before every navigation, and fails closed: if the reader fails, the state is treated as on.
- **On:** only loopback and `file:` load. The URL is decided before any DNS lookup, as in the preview tab's rule (`preview-rules.ts`). Three layers make sure of it:
  1. every request goes through DevTools request interception, which refuses non-loopback URLs, sub-resources and workers included;
  2. host-resolver rules map every name except `localhost` to "not found", so nothing is resolved;
  3. the proxy is set to a refusing local address with loopback bypassed, so a request that escaped interception still cannot connect.
- Turning airgapped on while the browser runs restarts it with these settings.

### 9.3 The egress audit sees the browser

`ragon-admin audit egress --browser` is a new mode beside `--app` and `--web`. It runs the server under the audit's `strace -f`, which follows the browser's processes because they are the server's children, through four scripted sessions:
1. airgapped on, a loopback fixture site, every tool once: **no non-loopback connect, no DNS**;
2. airgapped off, no navigation, 2 minutes idle: **no connect at all** (no background traffic);
3. airgapped off, one allowed test origin: **only that origin's addresses**;
4. the injection fixtures (§8.2) unattended: **no connect to the outside origin**.

Phase 0 checks that `strace -f` follows the sandboxed zygote. If it cannot, the audit runs the browser in a network namespace with a counting loopback proxy instead, and documents which one it used.

### 9.4 Left for 3.0

These belong to the owner's 3.0 security release:
- running the browser in its own network namespace with a filtering proxy as the only route out (on Linux);
- a fresh profile per task for sensitive tasks;
- request diffing for GET-based state changes;
- an automatic reviewer for gated actions.

## 10. What the local model can do, and how results show

### 10.1 Text and structure first

The model works from `browser_snapshot`, `browser_find` and `browser_read`, which are text. Snapshots are pruned and capped (4,000 tokens by default, the cap named in the result), so one page does not crowd out the turn. Phase 0 measures the raw and pruned snapshot size on ten real pages and the tool-call accuracy of the default model on five fixture tasks (**unmeasured today**).

### 10.2 Screenshots: measured on one page, used to check

**Measured 2026-11-28** on a DGX Spark node, with the default model's checkpoint (NVFP4) served by the node's **SGLang** server, not ragon-serve; temperature 0, thinking off, n=3. Whether ragon-serve's image path gives the same result is **unmeasured** (P0.7):
- **The page:** a 1280×800 screenshot of a synthetic checkout page with a heading, two inputs, a checkbox, a total, a link and two buttons.
- **Cost:** **1,000 image tokens, 2.5-3.9 s per request.**
- **Reading:** the model listed **all 6 interactive elements with their labels and kinds, the order total, and which button submits, correctly in 3 of 3 runs.**
- **Coordinates:** it gave them on a 0-1000 scale. After rescaling, all six centres were **within about 15 px** of the truth.

The companion desk-work probe of the same day found that the image path reads a rendered page correctly. It also found that **the agent did not call the image tool on its own in 2 of 2 runs and invented the content.**

Therefore, in 0.9.11:
- screenshots are used **to check**: the "check your web work" skill ends with a required `browser_screenshot` step whose image is returned inline as image content;
- the agent compares the image with the user's request and with the snapshot;
- **no action is taken by coordinates.**

The measurement above is one synthetic page. Phase 0 repeats it on 20 fixture and real page screenshots, both through ragon-serve's image path and through the agent loop. Coordinate actions (`click_xy`) wait for that number and go to 1.0.

### 10.3 How it shows

| Surface | What the user sees |
|---|---|
| Terminal | One history line per tool call ("browser · click "Place order" · shop.example"). Snapshots are summarised as "page: 41 elements, 2,980 tokens", not printed. Each screenshot is saved to `$CODEX_HOME/browser/shots/<thread>/<n>.png`, and the line shows the path. Approvals and the hand-off appear as the terminal's existing elicitation prompts. |
| Work (desktop app) | Tool cards with the screenshot as a thumbnail after every page-changing action. A **Browser** side panel shows the agent's current tab live: a screencast of frames, view only, with the URL and a Stop button. A **Take over** button runs the hand-off (§6.2). Approvals use the app's approval cards. `@Browser` in the composer turns it on for the thread. |
| Ask (app and `ragon web`) | The same cards and live panel, with frames sent over the existing connection. Hand-off works only when the agent runs on the machine with the display (§6.2). |
| Settings › Browser | Beside the preview tab's sections, **Agent permissions**: the default for internet origins, per-site access and downloads (the `[browser_use]` keys and `sites.toml`), the agent profile's size, and Clear agent browser data. |

## 11. Scope

**In 0.9.11 (preview):**
- §4 to §10 on Linux (arm64 and x86-64) for nodes and clients, and macOS clients on the found-browser path;
- the Windows client on the found-browser path, untested;
- the terminal, Work and Ask;
- the quality set and the injection tests as release gates (§12, P7).

**Not in the first version:**
- the extension into the user's own browser (route c);
- desktop computer use (route d), and coordinate clicks inside the agent's browser;
- uploads; developer mode, free-form scripts, full DevTools access; cookie and storage tools;
- sign-in on a remote node with no display; the in-app view that takes input (Take over in 0.9.11 opens a separate visible browser window, §6.2; taking over inside the app's own view comes in 1.0);
- persistent approvals beyond `sites.toml`'s per-origin access and download answers;
- site tools (`allow_webmcp`), an automatic reviewer, an annotate mode for the agent's browser;
- reading the user's browsing history; several browsers per session; the Windows hard memory cap; the macOS and Windows client live panel beyond the same frames.

**1.0:**
- the desktop app's own engine as the agent's browser, in a separate partition, with a view that takes input; sign-in and take over from the app for any session, remote nodes included (upstream's in-app shape, U11, U12);
- `click_xy` if Phase 0 and 1.0 measurements pass;
- uploads with approval;
- persistent approvals;
- the Windows memory cap.

**After 1.0:**
- the extension (c), opt-in, with its own spec;
- desktop computer use (d), macOS first.

**3.0:** §9.4.

## 12. Phases, rows and agent-days

Rows go to Ragon lanes first for the site, docs and test fixtures (owner rule). The cheapest model takes the rest. One agent-day is one lane-day.

| Phase | Rows | Agent-days |
|---|---|---|
| **P0 Measurements and checks** | P0.1 browser sources on Linux arm64 (deb, snap and its confinement) and on the Mac gate box; P0.2 PSS on ten real pages and the cgroup cap; P0.3 sandbox under AppArmor on a node; P0.4 `strace -f` through the sandbox; P0.5 session cookies across a headless-to-headed restart; P0.6 the turn-end hook route (§4.4); P0.7 vision on 20 screenshots through ragon-serve and the agent loop; P0.8 snapshot sizes and the tool-list token cost; P0.9 approval routing: the tool annotations ask nothing, the elicitation reaches the user under every reviewer setting (§7.2) | 2 |
| **P1 Server core** | P1.1 `ragon browser serve`, the pipe transport, launch and lifecycle; P1.2 snapshots and refs (§4.3); P1.3 the 16 tools; P1.4 the declaration in the launcher, the switch (§4.5); P1.5 the turn-end hook or codex patch | 5.5 |
| **P2 Policy and gate** | P2.1 `[browser_use]` and requirements reading (§4.6); P2.2 origin access with lifetimes; P2.3 submit, purchase, send and delete triggers with request pausing; P2.4 downloads, uploads, permissions, secret fields; P2.5 elicitations and fail-closed; P2.6 `policy.toml`, `sites.toml` and the sandbox-cannot-write test; P2.7 the codex patch that keeps `ragon_browser` elicitations from the automatic reviewer and the full-access shortcut | 4.5 |
| **P3 Network** | P3.1 background-off switches; P3.2 airgapped layers and the reader; P3.3 `audit egress --browser` with its four sessions | 2.5 |
| **P4 Surfaces** | P4.1 terminal lines and screenshot files; P4.2 Work and Ask cards and thumbnails; P4.3 the live panel (screencast frames); P4.4 Take over (hand-off) in the app; P4.5 Settings › Browser › Agent permissions; P4.6 `@Browser` | 4.5 |
| **P5 Memory and host safety** | the scope cap, the tab cap, idle shutdown, the host-safety check before launch | 1 |
| **P6 Finding and install** | `ragon browser status`, `ragon-admin browser install` for Linux, macOS and Windows, the sandbox step in `host setup` | 1.5 |
| **P7 Quality and injection** | P7.1 fixture sites on loopback (Ragon rows); P7.2 the 40-task set and harness; P7.3 the injection suite (§8.2); P7.4 the gate run and one fix round | 4 |
| **P8 Docs and release** | the user page, `docs/dev/browser.md`, the release notes' limits (§7.3) (Ragon rows) | 1 |
| | **Total** | **about 26.5 agent-days**, plus review on the path |

### 12.1 The quality set

There are **40 web tasks on loopback fixture sites**: deterministic, no internet, served by the harness.

| Category | Tasks | Examples |
|---|---|---|
| Read and extract | 10 | the price table of a product page into CSV; the three newest entries of a changelog; an answer that needs one click to expand |
| Navigate and search | 8 | a site search, then the second result's detail; pagination; tabs |
| Forms, filled but not submitted | 6 | a booking form filled from the user's details; a select and a date picker |
| With approval (the harness answers) | 6 | submit a contact form (approve); a purchase (deny, and the agent reports it stopped); a download (approve) |
| Checking its own web work | 6 | a dev server with a planted bug (a console error, a broken button, a misaligned layout seen only in a screenshot); the agent finds it, fixes it, and shows the fixed screenshot |
| Sign-in | 4 | the agent recognises the login, calls the hand-off, and the harness signs in as the user |

**Pass bar:**
- **70 % overall**, the same bar as desk work;
- no category below 50 % without being named in the release notes;
- the injection suite at 12 of 12, gated (§8.2).

Ten live-internet tasks are run once by hand before the release as a smoke test (§14). They are not part of the bar, because live sites change.

### 12.2 Other tests

- **Unit (Rust):**
  - the policy reader with every upstream key, including the requirements precedence;
  - the `policy.toml` floor: `allow` refused for every category, an unreadable value read as deny, the managed copy winning;
  - elicitation answers: cancel, decline, a timeout and an unparseable answer all read as deny;
  - the trigger classifier on recorded request sequences;
  - the secret-field detector;
  - the untrusted frame's escaping;
  - the air-gap fail-closed path.
- **Integration (Linux CI, headless):** every tool against the fixture sites; stale refs; tab cap; idle shutdown; download and upload handling; the sandbox-cannot-write-policy test.
- **The egress audit's `--browser` mode** (§9.3), on a node.
- **The desktop app's end-to-end suite:** the cards, the live panel and an approval card on a loopback page.

### 12.3 Does 12-18 hold?

- **The critical path:** P0 → P1 → P2 → P7.4 (the gate run) → fixes. That is 2 + 5.5 + 4.5 + 2 + 1 = **15 days**, plus review on that path. It comes to about **16.5 calendar days**.
- P3, P4, P5, P6 and the fixture writing of P7 run in a **second lane** beside it.
- Starting Monday 2026-11-30, 16.5 days ends **Wednesday 2026-12-16**. That leaves two days of slack before Friday 2026-12-18.
- **It holds only with two lanes of its own from 11-30.** The desk-work spec already plans three lanes and about 34 agent-days, with one day of slack on 12-17. The IDE extension and automations come on top. If browser use has to share desk work's lanes, the date does not hold for both.
- **The cut line is Monday 2026-12-14.** P0 to P2 take 12 days, so on plan P2 is done on Friday 12-11. If P2 is not done by Monday 12-14, 0.9.11 ships with:
  - the server, the gate, the network rules, the audit and the terminal lines;
  - Work and Ask showing screenshot cards **without** the live panel and Take over.

  Those two go to 1.0. If the injection suite is not 12 of 12 on 12-17, browser use ships **off by default** behind `[features] browser_use = true` and is named in the release notes. It never ships on and unsafe.

## 13. Owner questions (each with its default)

| # | Question | Default if no answer |
|---|---|---|
| Q1 | Route (b'), Ragon's own server with the open interface's shape and upstream's config and surface (§3)? | Yes |
| Q2 | Where the browser comes from when none is found | Found first; then `ragon-admin browser install` (the distribution's package on Linux, a pinned build with a checksum on macOS), asked once, never automatic |
| Q3 | Keep the agent's profile between sessions, so sign-ins survive? | Yes, persistent, with Clear in Settings and `ragon browser clear` |
| Q4 | Default for an internet origin the agent opens | Ask (allow once, for this thread, always, block); loopback, LAN and files allowed, the same as the preview tab |
| Q5 | Browser tools on by default in the desktop app and the terminal? | On in both; off in unattended runs (exec, Night Shift, SWE-bench, automations) unless configured |
| Q6 | Browser in unattended runs (automations) when configured | Read-only on origins pre-allowed in `[browser_use]`; every gated action denied |
| Q7 | Where downloads go | `~/Downloads/Ragon/`, each download asked unless the origin says `downloads = "allow"` |
| Q8 | Screenshots in the model's context | Only when the agent calls `browser_screenshot`; the web-check skill requires one at the end; snapshots stay text |
| Q9 | Label and default in 0.9.11 | Preview, on; off by default if the injection suite is below 12 of 12 or the quality set below 60 %; between 60 and 70 % it ships on, with the shortfall named in the release notes |
| Q10 | Lanes | Two lanes of its own from Monday 11-30; cut line Monday 12-14 (§12.3) |

## 14. Owner steps

1. Answer Q1-Q10, or let the defaults stand; the build starts Monday 2026-11-30 either way.
2. Confirm two lanes for browser use next to desk work, the IDE extension and automations (§12.3). If only one is free, say which of the cut-line items to drop first.
3. Before the release, about 20 minutes on the desktop app:
   - run the ten live-internet smoke tasks of §12.1;
   - sign in once through Take over to a test account of your choice;
   - say whether the approval prompts read clearly.
4. No machine step needs root except the AppArmor profile, which `ragon-admin host setup` already installs. If Phase 0 finds the browser needs its own profile line, the job writes the script and names it for you to run on each node.
