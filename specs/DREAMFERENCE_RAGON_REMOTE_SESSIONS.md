# Codex remote sessions in Ragon

**Status:** draft for the owner (2026-11-27), written on branch `spec/remote-sessions`; not landed (release freeze until 0.9.9). The owner's answers of 12:25, the ruling of 12:35 and the answers to Q1-Q10 of 22:11 (with the 1.0 date and lanes of 22:07) are in §0. Nothing here is built yet.

**Names:** post-rename (`ragon`, `ragon-admin`, `~/.ragon`, `ragon web` on port 3100, the gate on port 8000).

**Pinned Codex:** `codex/` at tag `rust-v0.158.0`. Every `file:line` without a prefix is under `codex/codex-rs/`. Paths that start with `ragon-rs/`, `codex-patches/`, `dreamference/`, `desktop/` or `specs/` are in this repository.

**Builds on:**
- [AUTH](./DREAMFERENCE_AUTH.md): one base URL on the gate (§5.1), the account RPCs (§5.8), the `/backend-api/` stubs (§5.9), the node CA (§6), one primary and its secondaries (§7), the 3.0 list (§19).
- [RAGON_REMOTE_ACCESS](./DREAMFERENCE_RAGON_REMOTE_ACCESS.md): the overlay, enrolment and the locator tier by name.
- [RAGON_SYNC](./DREAMFERENCE_RAGON_SYNC.md) §16.3 and §18: the control socket probe and Ragon's own live hub.
- [RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) (the `ragon web` paragraph near line 322): one shared app-server per node, one connection per tab.
- [DESKTOP_ELECTRON](./DREAMFERENCE_DESKTOP_ELECTRON.md) and [DESKTOP_PARITY](./DREAMFERENCE_DESKTOP_PARITY.md) §1.3-1.4: the single-instance lock and the protocol counts.
- [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) §10.9 (declared exceptions) and [RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md) §5.2 (`/node` as a local-only exception).
- `docs/dev/launcher.md` (lines 53-60): the patches and forced settings that close vendor traffic today.

---

## 0. Decisions, stated first

**The owner's goal**, verbatim: "We need to support all codex remote session functionality as close and compatible to codex as possible. Run agent to write a spec. The goal is to be absolutely compatible".

**The governing ruling** (owner, 2026-11-27 12:35), verbatim: "Everything that goes to [the vendor] in codex needs to go through our primary node. In this way we can achieve 100 percent compatibility".
- The primary stands in for the vendor's servers. The client keeps the same code path, the same requests and the same protocol. Only the destination changes.
- Client patches stay as few as possible: ideally only base addresses and trust roots.

**The owner's answers** (2026-11-27 12:25):

| # | Topic | Answer |
|---|---|---|
| 1 | Remote control | Our own relay. Same protocol, through the user's nodes (HTTPS, accounts, overlay). Nothing goes to the vendor. |
| 2 | Cloud tasks | `ragon cloud` on the user's own nodes. Same commands and flow; a task runs as a node job. |
| 3 | Who may attach | Verbatim: "All users for now but everyone goes through primary node and primary node decides where to place task". |
| 4 | Several clients on one session | Exactly as Codex. Documented here; nothing changed. |
| 5 | Encryption | Across machines only `wss://` with the node certificate. `ws://` only on loopback. This is the one named difference. |
| 6 | First piece | Direct attach over node TLS with accounts, in 0.9.10 (Mon 12-14). |
| 7 | The 1.0 gate | Everything: direct attach, relay, cloud. Plan 12-27. Size honestly and give the earliest realistic date (§11). |
| 8 | The last client disconnects | Exactly as Codex. Codex keeps the work running (§5, F12), so no follow-up question is needed. |
| 9 | Phone | The node's web page for now. A native app comes after 1.0: listed, not designed. |
| 10 | `/airgapped on` | Attach from another machine stays allowed on the own network or overlay, as a declared local-only exception in the egress audit. |

**The coordinator's reading of answer 3** (a reading, not the owner's words):
- Any signed-in user may attach to any session. A per-owner rule is a 3.0 item.
- Every remote request enters through the primary. The primary places it (load, free memory, model up, GPU lock, benchmark gate) and then routes or redirects it.
- An unmodified Codex client must work while knowing only the primary's address.
- With the primary down, secondaries use their cached keys, and a secondary with nothing cached refuses (AUTH §7).

**The owner's answers to Q1-Q10** (2026-11-27 22:11, from DECISIONS.md; where they differ from the earlier recommendation, it says so):

| # | Topic | Answer | Sections changed |
|---|---|---|---|
| Q1 | Telemetry | The primary answers analytics and metrics with success and **stores nothing** | §3.3, §7.6 |
| Q2 | Feedback | Kept **30 days on the primary** for the admin, never forwarded; the TUI's `/feedback` **stays removed** | §3.3, §7.6 |
| Q3 | Updater | The daemon fetches **the primary's install script, which runs `ragon update`** | §3.3, A9, §7.6 |
| Q4 | Plugins | **Differs from the recommendation.** The vendor's catalogue is **mirrored at build time on our build machines and shipped inside Ragon**. Users' nodes never contact the vendor; the primary serves the shipped copy (the owner resolved the conflict with the no-vendor rule this way) | §3.2, §3.3, §7.6, §8, T11, T18 |
| Q5 | Re-point versus close | **Differs from the recommendation.** Re-point **everything** through the primary, data-only paths included; what the primary does with the data is Q1 and Q2 | §3.2, §3.3, D5, D8, §6.4, §7.6, T11 |
| Q6 | Placement in 0.9.10 | The node this account used last, then the primary, then the least-loaded secondary | §7.2 |
| Q7 | Relay controllers | Controllers speak the standard app-server protocol | §7.4 |
| Q8 | Cloud code | The node forge, plus other git URLs the node can reach | §7.5 |
| Q9 | Linux trust | **Differs from the recommendation.** The installer **adds the node CA to the system store** on Linux (chosen over documenting `SSL_CERT_FILE`); Ragon's own client keeps the trust-root patch (A6) | §3.4, D4, A11, §8, T1 |
| Q10 | Ragon's own clients | The same route through the primary as an unmodified client; no redirect | §6.3, §8 |

**The 1.0 date** (owner, 22:07): 1.0 stays **Sun 2026-12-27**, with **four lanes on remote sessions from Tue 2026-12-01** (§11).

**What this spec decides on its own** (each one is open to the owner):
- An unmodified Codex client follows no WebSocket redirect (§6.3). So "routes or redirects" means the primary **proxies** for Codex clients. Ragon's own clients take the same route (owner Q10).
- The primary carries one WebSocket per client. In 0.9.10 it places a whole connection on one node. In 1.0 it routes per thread (§7.3).
- The build-time plugin mirror (Q4) includes the plugin sources the catalogue points at, not only the list, because a node that never contacts the vendor could not install from a list. A plugin whose licence does not allow redistribution is left out of the mirror and logged by the build (§7.6).
- "Re-point everything" (Q5) covers every vendor constant in Appendix C, also rows with no Ragon function: those go to the primary, which answers Codex's own not-found or feature-off shape (§7.6).

## 1. Observed versus only read

**Observed** (an earlier run of this job, 2026-11-27; a throwaway home folder, a mock model server, `ragon app-server --listen ws://127.0.0.1:18998 --ws-auth capability-token`, nothing on any node touched):
- No token or a wrong token gets `401`; the right `Authorization: Bearer` gets `101`.
- `initialize` returns `codexHome` (a host path) and a user agent naming the OS and CPU.
- `remoteControl/status/changed` arrives three times with status `disabled`, `serverName` set to the **host name of the box**, and an installation id. Every client sees the host name.
- Client A runs `thread/start` and `turn/start` and gets the mock answer. Client B runs `thread/loaded/list` and `thread/resume` and sees A's turn live. A closes; B runs its own turn. Client C connects later, resumes, and sees three turns.
- `thread/resume` with full history sends a `deprecationNotice` ("use excludeTurns:true then thread/turns/list + thread/items/list").
- The TUI (`ragon --remote ws://127.0.0.1:18998 --remote-auth-token-env …`) connects and sends its start-up requests. It was not driven further (no real terminal).
- The server starts its remote-control loop even when remote control is disabled. It derives the URL from `chatgpt_base_url`, logs `remote control requires ChatGPT authentication` and checks auth every second.
- `--listen unix://PATH` turns PATH into a symlink to `/tmp/codex-daemon-<uid>/<sha256>`, mode `0600`, no token needed.

**Not observed:**
- No unmodified upstream Codex binary was run at either end. Every observation is a Ragon build against a Ragon build, plus a small Python client. The first conformance test (§10) closes this gap.
- No `wss://` was tried: the app-server has no TLS listener (§5, F5).
- Remote control, cloud tasks, `codex apply`, the daemon and its updater, exec-server and tcp-tunnel were read in the source, not run.

**Read:** everything else, in the source at the pinned tag and in the vendor's public docs (fetched 2026-11-27). A row's citation is its evidence. Where the docs and the source disagree, the source wins and the row says so.

## 2. Terms

- **Host:** the machine whose app-server runs the session (it holds the files, runs the tools, writes the rollout).
- **Client:** anything that speaks the app-server protocol to a host: the TUI with `--remote`, the desktop app, `ragon web`, a phone page.
- **Primary, secondary:** as in AUTH §7. The primary keeps the accounts. A secondary keeps none.
- **Front door:** the primary's gate on port 8000. Every remote request enters there.
- **Vendor surface:** every host and path that Codex as shipped can contact at the vendor (§3).
- **Re-point:** keep the client's code path and change only the address it sends to. **Close:** a patch that stops the request. Ragon has both today (§3.3).

## 3. The vendor surface map (the owner's ruling of 12:35)

This section comes before the per-function part because the ruling governs it. **A** lists every vendor service that Codex at the pinned tag can contact. **B** says what the primary must serve for each row. **C** covers the rows that touch a standing rule. **D** says how a client finds and trusts the primary.

**The address column** takes one of three values:
- **config key:** a `config.toml` key moves it (Ragon's launcher writes the key);
- **env var:** only an environment variable moves it;
- **hard-coded:** only a patch moves it, either because the URL is a constant or because a host allowlist in the code refuses any other host.

Literal vendor hosts appear only in code font, as the source writes them.

### 3.1 A: every vendor service

| Row | Service | Codex code | Request and answer | Address | Ragon today |
|---|---|---|---|---|---|
| V1 | Model API (Responses, HTTP and WebSocket) | `codex-api/src/endpoint/responses_websocket.rs:405`; `model-provider/src/provider.rs:411-417` | `POST {base}/responses`, SSE or WS stream of Responses events; Bearer at the upgrade | config key: the provider `base_url`; `openai_base_url` (`config/src/config_toml.rs:424`) | served by every gate (AUTH §5.9, §10) |
| V2 | Model list | `codex-api/src/endpoint/models.rs:34-44` | `GET {base}/models?client_version=…` returns `{"models":[ModelInfo…]}` | config key: as V1 | served (AUTH §5.9) |
| V3 | Realtime conversation, and the sideband attach to an existing call | `core/src/realtime_conversation.rs:691-708`, `:1924-1927`; `codex-api/src/endpoint/realtime_call.rs:62-63`; `codex-api/src/endpoint/realtime_websocket/methods.rs` | WS upgrade with Bearer; WebRTC call creation; the sideband is `wss://api.openai.com/v1/realtime?call_id=…` | config key: as V1, and `experimental_realtime_*_base_url`; the sideband is **hard-coded** | the gate checks the Bearer at the upgrade (AUTH §5.9) and has no realtime model. **Open for a raw client**: `thread/realtime/start` with an existing call sends the Ragon bearer to the vendor's sideband (`features.realtime_conversation` is on by default); the bridge refuses the method (`ragon-rs/web/src/policy.rs:820`), a raw client is not refused |
| V4 | Sign-in (issuer, token, device code, authorize) | `app-server/src/request_processors/account_processor.rs:28`; patch `0040` | OAuth grants, `/api/accounts/deviceauth/*`, the device page | config key: `chatgpt_base_url` (patch 0040 strips `/backend-api`); env var `CODEX_APP_SERVER_LOGIN_ISSUER` | served by the primary; a secondary forwards (AUTH §5.1, §7) |
| V5 | Account backend: `wham/usage`, `accounts/check`, `profiles/me`, `settings/user`, `workspace-messages` | `backend-client/src/client.rs:145`, `:402-403`, `:419`, `:729`, `:736` | JSON GETs with the account Bearer and `chatgpt-account-id` | config key: `chatgpt_base_url` | stubs on every gate (AUTH §5.9) |
| V6 | Thread usage | `backend-client/src/client/thread_usage.rs:115-118` | JSON GET | config key: `chatgpt_base_url` | 404 shape (AUTH §5.9) |
| V7 | Remote-control relay: `wham/remote/control/server`, `/enroll`, `/refresh`, `/pair`, `/pair/status`; client list and revoke | `app-server-transport/src/transport/remote_control/protocol.rs:224-240`, `:300-312`; `remote_control/mod.rs:798`, `:807` | HTTPS enrol, refresh and pair; then one outbound WSS with envelopes (`client_id`, `stream_id`, `seq_id`, acks, chunks); headers `x-codex-installation-id`, `x-codex-host-device-kind`, `x-codex-subscribe-cursor` | config key `chatgpt_base_url`, **but hard-coded**: the host must be the vendor's or loopback (`protocol.rs:193-212`, `:283-285`) | missing. The loop starts and waits for account auth (observed); a `<node>.local` base would be refused by the host check |
| V8 | Cloud tasks: `wham/tasks`, `tasks/list`, `tasks/{id}`, `sibling_turns` | `backend-client/src/client.rs:459`, `:495`; `cloud-tasks/src/lib.rs:54-56` | JSON create, list, get, diff | env var `CODEX_CLOUD_TASKS_BASE_URL`, **and hard-coded**: saved credentials go only to the vendor's hosts on port 443 (`cloud-tasks/src/util.rs:47-80`) | missing. The gate answers the 404 shape, **but `ragon cloud` never asks the gate** (§5, F26) |
| V9 | Cloud environments: `wham/environments`, `environments/by-repo/github/{owner}/{repo}` | `cloud-tasks/src/env_detect.rs:50-92`, `:289` | JSON list | as V8 | missing |
| V10 | `codex apply TASK_ID` | `chatgpt/src/get_task.rs:38`; `chatgpt/src/chatgpt_client.rs:18`, `:53-97` | `GET {base}/wham/tasks/{id}` with `OAI-Product-Sku: codex` | config key: `chatgpt_base_url` | 404 shape on the gate |
| V11 | Cloud-config bundle | `cloud-config/src/service.rs:35-57`, `:486-540` | `GET /wham/config/bundle` every 15 min, only for business, education and enterprise plans | config key: `chatgpt_base_url` | 404 shape; never asked, because the `accounts/check` stub reports no such plan |
| V12 | Daemon updater | `app-server-daemon/src/update_loop.rs:66-68`, `:380`, `:532-625` | downloads `https://chatgpt.com/codex/install.sh` (`install.ps1` on Windows) and pipes it to `/bin/sh -s` (`:380`, `:532-545`; read, not run) | **hard-coded** | **open, conditional.** Not patched (0008 patches only `codex update`). It runs only for a managed daemon with `autoUpdateEnabled` (default true, `app-server-daemon/src/settings.rs:14-35`) and only where the daemon finds the standalone install layout; whether a Ragon install has that layout was not checked (Appendix C, G3) |
| V13 | TUI update check | the TUI's update module; forced off at `ragon-rs/src/lib.rs:1619` | asks the vendor's public release feed about new releases | config key: `check_for_update_on_startup` | closed: forced `false` by the launcher |
| V14 | Desktop app download (`codex app`) | `cli/src/desktop_app/mac.rs:10-12`, `:237`; `cli/src/desktop_app/windows.rs:7-8` | `persistent.oaistatic.com/codex-app-prod/Codex.dmg`; a store page on Windows | flag `--download-url`; otherwise hard-coded | open when run; it installs the vendor's app |
| V15 | Usage analytics | `analytics/src/client.rs:142` | event batches | config key `[analytics] enabled` | closed: patch `0013` never builds the queue |
| V16 | Metrics export | `otel/src/config.rs` (patch `0015`) | metrics to the vendor's collector | hard-coded | closed: patch `0015` |
| V17 | Feedback upload | `app-server-protocol/src/protocol/common.rs:1339` (`feedback/upload`); `feedback/src/lib.rs:57-58`, `:593-601`; `app-server/src/request_processors/feedback_processor.rs:49-55`, `:276` | a crash-report envelope with optional logs and the rollout, sent to the vendor's error-report service | **hard-coded** DSN; `feedback.enabled` defaults to true and the launcher does not set it | closed in the TUI by patch `0009`. **Open for a raw client**: the RPC still uploads. The bridge refuses it (`ragon-rs/web/src/policy.rs:820`); a client on the socket or, later, the front door is not refused |
| V18 | Plugin catalogue: featured list, curated sync, remote index | `core-plugins/src/remote.rs:1038`, `:2258`; `core-plugins/src/remote_legacy.rs:129`; `core-plugins/src/manager.rs` (patch `0015`) | JSON lists; a `git ls-remote` of the vendor's plugin repo | config key `chatgpt_base_url` for the index; hard-coded for the git repo | closed: patch `0015` and the forced `features.remote_plugin=false`, `features.plugin_sharing=false`; AUTH §5.9 plans the node's own index |
| V19 | Tips and announcements | `tui/src/tooltips.rs` (patch `0015`) | a text file from the vendor's code host | hard-coded | closed: patch `0015` |
| V20 | Connectors and apps | `connectors/src/lib.rs:191`, `:270`; `chatgpt/src/connectors.rs:175` | JSON lists | config key: `chatgpt_base_url` | stub: an empty list (AUTH §5.9); patch `0022` adds Ragon's apps |
| V21 | `codex doctor` reachability | `cli/src/doctor.rs` (patch `0016`) | probes of the vendor's API, auth, WebSocket and release feeds | hard-coded | closed: patch `0016` |
| V22 | Agent identity (workload sign-in) | `agent-identity/src/lib.rs:41-44` | JWT issue and account calls | hard-coded | not used: Ragon runs no workload identity |
| V23 | Exec-server registry and Noise relay | `exec-server/src/remote.rs:131-155`, `:711-745`; `cli/src/exec_server_command.rs:70-89`, `:345-424` | `POST {base}/cloud/environment/{id}/register`; a Noise relay stream | flag `--remote BASE_URL`; API-key mode is hard-coded to the vendor's hosts | not used; nothing points it anywhere |
| V24 | Terminal pet art | the launcher drops `tui.pet` (`ragon-rs/src/lib.rs`, after `:1630`) | images from `persistent.oaistatic.com` | config key `tui.pet` | closed: patch `0010` and the launcher |
| V25 | File upload for apps and MCP | `codex-api/src/files.rs:127`, `:149`, `:354-355` | `POST {base}/files`, then `…/uploaded` | config key: `chatgpt_base_url` | unknown-URL 404 on the gate |
| V26 | The apps MCP server (`codex_apps`) | `codex-mcp/src/mcp/mod.rs:600-620` | MCP over HTTP at `{base}/ps/mcp` | config key: `chatgpt_base_url` | unknown-URL 404; patch `0022` adds Ragon's own apps |
| V27 | Personal access token check | `login/src/auth/personal_access_token.rs:11-13` | `GET …/user-auth-credential/whoami` at the vendor's sign-in host | env var `CODEX_ACCESS_TOKEN` turns it on | open only when the user exports that variable |
| V28 | `codex responses-api-proxy` | `responses-api-proxy/src/lib.rs:52-53` | a local forwarder whose default upstream is the vendor's API | flag `--upstream-url` | open when run with the default |

The survey of every vendor URL constant in the tree is in Appendix C. Rows found there that touch no remote-session path are listed, not designed.

### 3.2 B: what the primary serves, per row

"Exists" means it is built and on main. "Small" is up to 2 agent-days. "Large" is more.

| Row | The primary serves | A secondary (forwards from 0.9.10, ruling 11-26) | The primary is unreachable |
|---|---|---|---|
| V1-V3 | exists (served by every gate). Under the ruling, an unmodified client sends to the primary, which proxies to the node it places; small | serves its own model; forwards a request the primary sent it | a client pointed at the primary fails; a Ragon client resolved to a secondary keeps working while its bearer is cached (AUTH §7) |
| V4 | exists | forwards (AUTH §7) | sign-in fails; nothing else can stand in (AUTH §7) |
| V5, V6 | exists (stubs) | answers itself (AUTH §7) | unchanged on a secondary |
| V7 | large: the host-side endpoints exactly as `protocol.rs` expects, plus the controller side, which Codex's source does not contain (§7.4) | forwards to the primary; a host enrolled through a secondary is enrolled at the primary | hosts keep retrying (Codex's own backoff); relayed clients lose the session; direct attach to a secondary still works |
| V8-V10 | large: the task store, the environment list, the node job and the diff (§7.5) | forwards | `ragon cloud` fails with the Codex error for an unreachable backend |
| V11 | exists (404 shape) | answers itself | unchanged |
| V12 | small: an install script on the primary that runs `ragon update` (§3.3) | forwards | the updater logs and retries; nothing installs |
| V13 | small: the primary answers the release-feed request with Ragon's current release (Q5, §3.3) | forwards | no notice |
| V14 | small: the primary serves the Ragon desktop installer | forwards | the command fails |
| V15, V16 | small: accept and drop (§3.3) | forwards | nothing is lost |
| V17 | small (§3.3) | forwards | the upload fails, as at the vendor |
| V18 | small: the vendor's catalogue as mirrored at build time and shipped inside Ragon, plus the node's own index (AUTH §5.9); the git repo is served by the primary from the shipped mirror (Q4, §7.6) | forwards | an empty list |
| V19 | small: the tips file shipped inside Ragon (Q5) | forwards | no tip |
| V20 | exists (stub) | answers itself | unchanged |
| V21 | small: re-point the checks at the primary | — | doctor reports the primary unreachable |
| V22, V23, V24, V27, V28 and the remaining Appendix C rows | small: each constant re-pointed at the primary (Q5), which answers Codex's not-found shape (V22, V23 until a Ragon environment registry after 1.0, V24), checks a token against Ragon accounts (V27), or is the upstream (V28) | forwards | the same failure as an unreachable vendor |
| V25, V26 | small: the node's own apps (patch `0022`) at the same paths; files kept on the node that runs the thread | forwards | the app call fails |
| V27, V28 | none: the launcher clears `CODEX_ACCESS_TOKEN`; the proxy's default becomes the primary's `/v1` | — | — |

### 3.3 C: rows that touch a standing rule

Each item was a question in §12; all of them are answered (owner, 2026-11-27 22:11) and the table says what is built.

| Item | Rows | Standing rule today | Decided | Owner |
|---|---|---|---|---|
| Telemetry | V15, V16 | patches `0013` and `0015` stop it at the client | re-point both (Q5): patches `0013` and `0015` are rewritten so the queue and the exporter send to the primary instead of being off. The primary answers the analytics and metrics paths with success and **stores nothing** (Q1), so no client sends anything anywhere else | Q1, Q5 |
| Feedback upload | V17 | patch `0009` removes `/feedback` from the TUI | the primary accepts the upload and keeps it **on the primary only**, readable by an admin (`ragon-admin feedback list`), deleted after 30 days. Never forwarded. The upload constant points at the primary (Q5). The TUI's `/feedback` **stays removed** (patch `0009` kept); uploads come from the app-server's `feedback/upload` | Q2, Q5 |
| Update checks | V12, V13, V19 | `check_for_update_on_startup` forced false; patch `0008` makes `codex update` run `ragon update`; the daemon updater is open | patch V12's constant to `{primary}/ragon/v1/install.sh`, which runs `ragon update`, the one Ragon route (Q3). V13 is re-pointed at the primary and the forced `check_for_update_on_startup=false` is lifted (Q5). V19 is re-pointed at the primary | Q3, Q5 |
| Plugin catalogue | V18 | patch `0015` and two forced feature flags | the vendor's catalogue is **mirrored at build time on our build machines and shipped inside Ragon** (§7.6). The primary serves the shipped copy, merged with the node's own index (AUTH §5.9), at the paths Codex asks; the plugin git repo is served by the primary from the same copy. Users' nodes never contact the vendor. `features.remote_plugin` is no longer forced off; `features.plugin_sharing` stays off (sharing would publish to the vendor) | Q4 |
| Re-point versus close | all | each closing patch was written before the ruling | **re-point everything** at the primary, data-only paths included: remote control (`0007`), cloud (`0006`), `codex apply`, the daemon updater, `codex app`, doctor (`0016`), analytics and metrics (`0013`, `0015`), tips, the update check, pet art, the V3 sideband and every other constant of Appendix C. What the primary keeps is Q1 and Q2. `-c feedback.enabled=false` stays on the server side of V17 only until the primary's store exists (A7). The audit allowlist gains one destination, the primary (§3.4) | Q5 |

**The closures the ruling reverses** (from `docs/dev/launcher.md:53-60` and §10 of Appendix B):

| Patch or setting | What it closes | Under the default |
|---|---|---|
| `0006-hide-cloud` | hides `codex cloud` (still runnable) | re-point: `ragon cloud` shown, backed by the primary (§7.5) |
| `0007-hide-remote-control` | hides `codex remote-control` (still runnable) | re-point: shown, backed by the primary's relay (§7.4) |
| `0009-remove-feedback` | `/feedback` in the TUI | keep: the TUI command stays removed (Q2); the upload path itself is re-pointed (Q5) |
| `0013-disable-usage-analytics` | the analytics queue | rewritten: the queue sends to the primary, which stores nothing (Q1, Q5) |
| `0015-no-vendor-network` | metrics, plugin catalogue, tips | rewritten: every URL it closes points at the primary (Q5); the catalogue is the shipped mirror (Q4) |
| `0016-doctor-offline` | doctor's vendor probes | re-point the probes at the primary (small) |
| forced `check_for_update_on_startup=false`, `tui.pet="disabled"`, `features.remote_plugin=false`, `features.plugin_sharing=false` | update notice, pet art, plugin sharing | lift the first three, re-pointed at the primary (Q4, Q5; pet art gets the primary's not-found answer, so no pet shows); keep `plugin_sharing` off, it has no target but the vendor |
| the egress allowlist (model server, 8767, 8888, 8768) | every other destination | add the primary's gate (§3.4) |

### 3.4 D: how a client finds and trusts the primary

**The address.**
- `ragon login` already writes `chatgpt_base_url = "{gate}/backend-api/"` and the provider base (`ragon-rs/src/lib.rs:1612-1630`). Today `{gate}` is the node the launcher resolved. Under the ruling it is **the primary's gate**: `https://<primary>.local:8000` on the LAN, the overlay name on the overlay (RAGON_REMOTE_ACCESS §5).
- The launcher finds the primary by the `auth=primary` TXT record (AUTH §7; `ragon-rs/src/node.rs:1081`) and remembers it by node id, never by address (RAGON_NODE §6.1).
- `ragon --remote` with no address means `wss://<primary>:8000`. `--remote-auth-token-env` is filled from the saved account token.
- An unmodified Codex needs three things: the same two config keys (or `-c` overrides), `--remote wss://<primary>.local:8000` with a token variable, and the CA (below). It needs nothing else.
- Codex's `--remote` needs a host and an explicit port, path `/`, and no query (`tui/src/lib.rs:444-481`). The front door therefore serves the session WebSocket at the root of port 8000. It tells it from HTTP by the `Upgrade` header.

**Trust.**
- Each node has its own CA (AUTH §6). The client pins the primary's CA at `~/.ragon/nodes/<primary id>/ca.pem`, and the launcher points Codex's HTTP and relay clients at it with `CODEX_CA_CERTIFICATE` (`http-client/src/custom_ca.rs:61-62`; `remote_control/websocket.rs:1392-1401`).
- The `--remote` WebSocket does **not** read `CODEX_CA_CERTIFICATE`. It uses the OS roots through tokio-tungstenite with `rustls-tls-native-roots` (`Cargo.toml:512-515`; `app-server-client/src/remote.rs:738`).
  - For Ragon's build, a small trust-root patch: the `--remote` dial uses the same CA loader as the HTTP client (A6, Q9).
  - For an unmodified Codex: on macOS and Windows the installers already add the primary's CA to the user's store (AUTH §6). On Linux the native-roots loader reads the system store, not `~/.pki/nssdb`, so **the installer adds the primary's CA to the system store** (owner Q9; A11): `/usr/local/share/ca-certificates/ragon-<primary id>.crt` and `update-ca-certificates` on Debian and Ubuntu, `/etc/pki/ca-trust/source/anchors/` and `update-ca-trust` on Fedora; removed by the uninstaller, replaced when the primary's CA changes. That the native-roots loader then trusts it is conformance test T1. `SSL_CERT_FILE` is not the documented route.
- When the primary proxies to a secondary, the client sees only the primary's certificate. The hop from the primary to the secondary uses that secondary's pinned CA (AUTH §6).

**The egress audit** sees one more allowlisted destination, the primary's gate on port 8000, and nothing else (RAGON_EGRESS). A client that reaches any vendor host is a failed audit, as today.

## 4. The compatibility rule

### 4.1 Rules (each one is a test in §10)

- **R1 Same flags.** Every flag that Appendix B lists (§1-§8, per subcommand) is accepted by `ragon` with Codex's name, value syntax, default and meaning. The value check is part of this: for example `--remote` needs an explicit port and path `/`, and a token is refused over non-loopback `ws://`. A flag Ragon hides stays accepted.
- **R2 Same config keys.** Every key that Appendix B lists (§9, and the key rows of §1-§8) is read from `config.toml` with Codex's name, type, default and effect. The launcher's forced values (§3.3) are the only exceptions, and each is listed there.
- **R3 Same environment variables**: every variable in Appendix B §9 and in the address column of Appendix C, with Codex's names and effects.
- **R4 Unmodified Codex client to a Ragon server.** An unmodified `codex` at the pinned tag, given only the primary's address, a token variable and the CA, runs every client-side function in §5 against a Ragon primary. It sees the same results, errors and notifications as against a Codex app-server, apart from the named differences (§4.2).
- **R5 Ragon client to an unmodified Codex server.** `ragon --remote …` attaches to an unmodified `codex app-server` (behind any TLS terminator for `wss://`, or on loopback `ws://`). Every function then works as with `codex --remote`.
- **R6 Same wire format.** JSON-RPC without the `jsonrpc` field (`app-server-protocol/src/jsonrpc_lite.rs`); the same method names, params, results, error codes (including `-32001` and `-32600`) and notification order. Between the client and the host, the primary never rewrites a frame, with one exception: the 1.0 router remaps server-request ids per upstream and maps them back (§7.3). A test checks that the client sees ids unchanged.
- **R7 Same token modes.** The app-server's `capability-token` and `signed-bearer-token` modes stay exactly as Codex has them. The front door adds Ragon account tokens (AUTH) as one more bearer the client sends in the same `Authorization: Bearer` header.
- **R8 Same files and sockets.** Rollout files, `session_index`, `auth.json`, `config.toml`, `environments.toml`, the control socket, `loaded-threads.json` and the daemon's `settings.json` keep Codex's formats. Their paths move under `~/.ragon` (patch `0014`), as today.
- **R9 Same multi-client behaviour.** Fan-out with one request id, first answer wins, `serverRequest/resolved` to all, owner-only user verification, slow-consumer disconnect, work that continues after the last client leaves (§5, F10-F13). This holds through the front door as well.
- **R10 Same vendor-facing requests.** For each row of §3.1 that the primary serves, a Codex client sends to the primary exactly what it would send to the vendor: method, path, headers and body. A recorded transcript checks it (§10).

### 4.2 Named differences

| # | Difference | Why |
|---|---|---|
| D1 | Across machines only `wss://` with the node certificate; `ws://` only on loopback. The front door has no plain `ws://` on the network | owner answer 5. Codex's client already refuses to send a token over non-loopback `ws://` (`app-server-client/src/remote.rs:119-127`), and Codex's server refuses a non-loopback `ws://` listener without auth (`app-server-transport/src/transport/websocket.rs:129-141`). So the difference affects only unauthenticated loopback-free setups, which Codex itself does not allow |
| D2 | Destinations: every vendor address becomes the primary | owner ruling 12:35 |
| D3 | Host-allowlist patches: the relay (`remote_control/protocol.rs:193-212`) and cloud (`cloud-tasks/src/util.rs:47-80`) accept the primary's host | these two checks are hard-coded. Without a patch, Ragon's build cannot point them at the primary. An unmodified Codex can still use the relay through a loopback forwarder on its own machine (§7.4) |
| D4 | Trust-root patch: the `--remote` dial reads `CODEX_CA_CERTIFICATE` | Ragon's client pins the primary's CA by node id. An unmodified Codex on Linux uses the system store, to which the installer adds the CA (Q9, A11) |
| D5 | Telemetry, feedback and update paths go to the primary like everything else (Q5); analytics and metrics are answered and dropped (Q1), feedback is kept 30 days on the primary and never forwarded (Q2), the updater runs `ragon update` (Q3). The TUI's `/feedback` stays removed (Q2) | standing privacy rules; owner Q1-Q3, Q5 |
| D6 | The relay's controller side is defined by Ragon (§7.4) | Codex's source contains only the host side. The vendor's controller is closed, so no unmodified controller exists to test against |
| D7 | Cloud tasks run as node jobs on the user's nodes, not in the vendor's containers (§7.5) | owner answer 2. The commands, flags and answer shapes stay the same; the environment model is simpler (§7.5) |
| D8 | Realtime and feedback upload are off on the shared server (`-c features.realtime_conversation=false`, `-c feedback.enabled=false`, A7). `thread/realtime/*` and `feedback/upload` give Codex's own feature-off answers | the realtime sideband sends the bearer to a hard-coded vendor host (V3), and feedback goes to a hard-coded vendor service (V17). No node has a realtime model; the sideband constant is re-pointed at the primary (Q5), which answers feature-off. Feedback upload comes back when the primary stores it (§7.6, Q2) |
| D9 | In 0.9.10, `thread/list`, `thread/loaded/list` and search show only the threads of the node the connection was placed on (§7.2) | the router that merges nodes is 1.0 (§7.3). The placement rule keeps one user on one node, so this is rare |
| D10 | Through the 1.0 router, `initialize` returns a fixed `codexHome`; server-request ids are renumbered per node and mapped back; requests without a `threadId` (`fs/*`, `command/exec`, `config/*`) go to the connection's home node (§7.3) | one connection spans several nodes, and the host path is not given out (§9 item 5). The client sees its own ids unchanged (R6) |
| D11 | Files and sockets live under `~/.ragon`, not `~/.codex` (R8, patch `0014`). The formats are Codex's | a Ragon install and a Codex install on one machine do not share state |
| D12 | `ragon --remote` with no address goes to the primary (A5). Codex refuses a bare `--remote` | one address for the user (§3.4). Every form Codex accepts behaves as in Codex |
| D13 | `ragon cloud` and `ragon remote-control` are hidden from help (patches `0006`, `0007`) but still run | they reach nothing useful until 1.0 (§7.4, §7.5), which removes the hiding |

## 5. Inventory and status today

One row per function family (F1-F33). Every flag, method and path in a family is in Appendix A (protocol) and Appendix B (transports). The columns are:
- **Vendor?** whether the function needs the vendor's servers in Codex as shipped;
- **Docs:** what the vendor's public docs say, and **differs** where the source does something else;
- **Today:** the status in Ragon today, one of
  - **works** as is;
  - **unsafe**: works, but unsafe as shipped;
  - **closed** by a patch;
  - **missing**.

### 5.1 Transports and attach

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F1 | stdio transport | `app-server-transport/src/transport/mod.rs:120`; `cli/src/main.rs:565-575` | default `--listen stdio://`, one JSON message per line | no | documented as the default | works (desktop app, `ragon web` spawn) |
| F2 | unix socket and control socket | `app-server-transport/src/transport/mod.rs:53-70`, `:127`; `unix_socket.rs:168-190`, `:282-299` | WebSocket over a Unix socket; `$CODEX_HOME/app-server-control/app-server-control.sock`; real socket under `/tmp/codex-daemon-<uid>/`, `0600`, same-user peer check | no | documented | works (observed); `ragon web`'s shared server listens on `$XDG_RUNTIME_DIR/ragon/app-server.sock` (`ragon-rs/web/src/app_server.rs:64-68`) |
| F3 | `ws://IP:PORT` with a capability token | `app-server-transport/src/transport/mod.rs:156`; `websocket-auth/src/lib.rs:288-320` | plain WebSocket; static bearer compared by SHA-256 | no | "experimental and unsupported" | works on loopback (observed) |
| F4 | signed-bearer mode | `websocket-auth/src/lib.rs:335-372` | HS256 JWT with a shared secret of 32 bytes or more; `exp` required; `nbf`, `iss`, `aud` checked; bearer read only at the upgrade (`:383`) | no | documented as in the source | works (read) |
| F5 | `wss://` serving | `app-server-transport/src/transport/mod.rs:100`, `:122-170` | none: the app-server has no TLS listener | no | the docs show `wss://` for `--remote` but never say how to serve it | missing; the front door terminates TLS (§7.1) |
| F6 | `--remote` client | `cli/src/main.rs:945-956`, `:2357-2375`, `:2507-2539`; `tui/src/lib.rs:444-481`; `app-server-client/src/remote.rs:119-127`, `:698-759` | TUI against a remote server; explicit port, path `/`; token from an env var, only to `wss://` or loopback `ws://` | no | documented; the docs say plain `ws://` "only for localhost or an SSH port-forward" | works on loopback (observed); `wss://` to a node needs F5 and the CA (§3.4) |
| F7 | health routes and Origin refusal | `app-server-transport/src/transport/websocket.rs:57-75`, `:89-99`, `:148-152` | `/readyz`, `/healthz`; any `Origin` header gets 403 | no | documented | works |
| F8 | non-loopback refusal | `app-server-transport/src/transport/websocket.rs:129-141`; `websocket-auth/src/lib.rs:279-284` | refuses a public `ws://` bind without `--ws-auth` | no | **differs**: the docs say non-loopback listeners "allow unauthenticated connections by default during rollout" | works |

### 5.2 Sessions and several clients

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F9 | thread start, resume, fork, list, archive, delete, queue | `app-server-protocol/src/protocol/common.rs:551`, `:563`, `:760`; `cli/src/main.rs:1437-1547`, `:1466-1513`; `cli/src/queue_cmd.rs:13` | the session life cycle, from the TUI subcommands and over the protocol | no | documented | works (observed: start, resume, loaded list) |
| F10 | several clients on one thread | `app-server/src/thread_state.rs:316`, `:343`, `:556-586`; `app-server/src/outgoing_message.rs:117`, `:143` | each thread has a set of subscribed connections; thread events go to all; global events are broadcast to initialized connections | no | documented in outline | works (observed with three clients) |
| F11 | approvals from a second client | `app-server/src/outgoing_message.rs:330`, `:467`, `:561`; `app-server/src/request_processors/thread_lifecycle.rs:864`; `app-server/src/user_verification.rs:63-70` | the same request id to every subscriber; the first answer wins; `serverRequest/resolved` to all; user verification only to the owner and refused for WebSocket and relay connections | no | mobile doc: "approve commands" from the phone | works (read; not driven in the probe) |
| F12 | disconnect of the last client | `app-server/src/message_processor.rs:864-899`; `app-server/src/outgoing_message.rs:275`; `thread_lifecycle.rs:20-130`, `:416`; `app-server/src/lib.rs:1146-1170` | the connection's subscriptions, watches and child processes end; pending approvals stay; a thread unloads only with no subscriber **and** no running turn, so work continues. Only stdio single-client mode stops the server | no | not documented | works (observed: B continued after A closed) |
| F13 | reconnect and rejoin | `app-server/src/request_processors/thread_processor.rs:4241-4354`; `thread_lifecycle.rs:566-816`; `tui/src/app/reconnect.rs:30-120`; `app-server-client/src/remote.rs:297`, `:427-498` | `thread/resume` on a loaded thread merges the live turn and replays pending approvals; the TUI retries 0/1/2/4/8 s within 120 s; the client crate never reconnects | no | not documented | works (observed: C rejoined) |
| F14 | notifications | `app-server-protocol/src/protocol/common.rs` (85 server notifications, 1 client notification); `app-server/src/transport.rs:104`, `:180` | per-connection filtering by `experimentalApi` and `optOutNotificationMethods` | no | documented | works |
| F15 | originator and version notice | `app-server/src/request_processors/initialize_processor.rs:130-132`, `:166-195`; `app-server-client/src/remote.rs:845-870`; `tui/src/status/remote_connection.rs:50-70` | the first client's name sets the process-wide user agent; a version mismatch only shows a notice | no | not documented | works |

### 5.3 Files, execution, forwarding

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F16 | file transfer (`fs/*`) | `app-server-protocol/src/protocol/common.rs:972`; `app-server/src/request_processors/fs_processor.rs:71-189` | read, write, copy, remove, list and watch any absolute host path, base64 in JSON text frames, with `sandbox: None` | no | documented | **unsafe**: any client can read `auth.json` and keys and write anywhere the server user can |
| F17 | remote exec (`command/exec`, `process/spawn`, `thread/shellCommand`) | `app-server-protocol/src/protocol/common.rs:1346-1372`; `app-server-protocol/src/protocol/v2/command_exec.rs:101` | one-off commands; the client may pick the sandbox (`danger-full-access`); `process/spawn` runs "without a Codex sandbox" | no | documented | **unsafe**: the client picks the sandbox; with `/airgapped on`, patch `0023` refuses an over-wide profile |
| F18 | exec-server | `cli/src/exec_server_command.rs:31-140`; `exec-server/src/server/transport.rs:40-234`; `exec-server-protocol/src/protocol.rs:22-57` | standalone executor (process, fs, http) on `ws://` or stdio; `environments.toml` dials one by URL or spawns one (for example over ssh) | its `--remote` registry and Noise relay: yes (V23) | not documented | **unsafe**: no non-loopback refusal, so `--listen ws://0.0.0.0:P` without auth serves anyone |
| F19 | port forwarding (`codex tcp-tunnel`) | `cli/src/main.rs:147-149`, `:1131-1132`; `tcp-tunnel/src/lib.rs:47-66`, `:198-208` | hidden; a loopback listener forwarded through an HTTP/3 CONNECT proxy given by the caller | depends on the proxy URL; none built in | not documented | works (read); no other port-forward code exists in the tree |
| F20 | SSH hosts | vendor desktop app only; in the CLI, `environments.toml` `program = "ssh"` (`exec-server/src/environment_toml.rs:38-51`) and SSH-forwarded sockets | a remote project over ssh | no | documented for the desktop app | works through an SSH forward to the unix socket or loopback `ws://` |

### 5.4 The daemon

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F21 | daemon life cycle: `app-server daemon bootstrap/start/restart/stop/version`, TUI auto-start | `cli/src/main.rs:637-675`; `app-server-daemon/src/lib.rs:436-494`, `:790-834`; `tui/src/daemon_startup.rs:25-50` | a managed app-server on the control socket, started by the TUI when `daemon_auto_start` is on | no | the in-tree README: for "Codex instances launched over SSH" | works when run by hand. The launcher's `-c` overrides skip auto-start (`tui/src/daemon_startup.rs:25-50`) |
| F22 | daemon updater | `app-server-daemon/src/update_loop.rs:63`, `:66-68`, `:380`, `:532-625`; `app-server-daemon/src/settings.rs:14-35` | after 5 min, then hourly: downloads the vendor's install script and pipes it to `/bin/sh -s` (read) | **yes** (V12) | README: "check for updates after five minutes, then hourly" | **unsafe**: open where the install layout matches (not checked for a Ragon install); a run would replace Ragon with the vendor's build |
| F23 | daemon remote-control toggles | `cli/src/main.rs:655-659`; `app-server-daemon/src/lib.rs:248`, `:269` | persisted remote-control preference | yes (V7) | README | missing (no relay to reach) |

### 5.5 Remote control (relay)

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F24 | host link, pairing, client list and revoke: `codex remote-control [start|stop|pair]`, `app-server --remote-control`, `remoteControl/*` RPCs | `cli/src/remote_control_cmd.rs:32-130`, `:472-484`; `app-server-transport/src/transport/remote_control/protocol.rs:106-312`; `remote_control/auth.rs:64`, `:98-143`; `app-server-protocol/src/protocol/common.rs:1144-1180` | the host enrols with the relay, keeps one outbound WSS, and serves controllers' JSON-RPC wrapped in envelopes; pairing gives a short code; account auth only | **yes** (V7) | **differs**: the docs say the host is the vendor's desktop app and that it "can't be set up from the Codex CLI"; the source has `codex remote-control` and `pair` | **closed** in name (patch `0007` hides the command) but still runnable; it reaches nothing because the host check refuses the node |
| F25 | the controller side | not in the source (only the environments list, `app-server-transport/src/transport/remote_control/clients.rs:270`) | the phone or desktop that drives a host through the relay | yes (the vendor's closed apps) | mobile doc: phones follow tasks, approve, review diffs | missing |

### 5.6 Cloud

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F26 | `codex cloud` (TUI, `exec`, `status`, `list`, `apply`, `diff`) | `cloud-tasks/src/cli.rs:6-119`; `cloud-tasks/src/lib.rs:49-71`, `:818-822` | browse, submit, follow and apply cloud tasks | **yes** (V8) | cloud doc: the vendor's containers, environments, repositories, internet settings | **unsafe**: patch `0006` only hides it. The base comes only from `CODEX_CLOUD_TASKS_BASE_URL` (default the vendor), which `ragon-rs` never sets, so a run reaches the vendor and **may send the saved Ragon account token there** (read, not observed) |
| F27 | `codex apply TASK_ID` | `chatgpt/src/apply_command.rs:16-22`; `chatgpt/src/get_task.rs:38` | fetches a task's diff and applies it | yes (V10) | cloud doc (apply from the CLI) | missing (the gate answers 404) |
| F28 | environment discovery | `cloud-tasks/src/env_detect.rs:50-92` | finds the environment for the repo's origin | yes (V9) | cloud doc | missing |
| F29 | cloud-config bundle | `cloud-config/src/service.rs:35-57` | admin-pushed config for business plans | yes (V11) | not documented | works as "none" (never asked) |

### 5.7 Accounts and the rest

| F | Function | Source | What it does | Vendor? | Docs | Today |
|---|---|---|---|---|---|---|
| F30 | sign-in token modes for remote use | `app-server-protocol/src/protocol/common.rs:1276`; patches `0040`, `0042`; AUTH §5.8 | account tokens for the bearer; `account/chatgptAuthTokens/refresh` | yes in Codex (V4) | documented | works (AUTH; through the gate) |
| F31 | `codex app` | `cli/src/app_cmd.rs:5-13`; `cli/src/desktop_app/mac.rs:10-12`; `windows.rs:7-8` | opens or downloads the desktop app | yes (V14) | documented | **unsafe**: downloads the vendor's app |
| F32 | `codex agents` | `cli/src/main.rs:333-342`, `:1051-1129` | all sessions on the daemon or a remote server | no | documented | works with `--remote` (read) |
| F33 | relay-free mobile and web view | `ragon web` (RAGON_ASK) and Ragon's live hub (RAGON_SYNC §18) | Ragon's own page and hub; not a Codex function | no | — | works on the LAN; only two approvals are answerable through the hub (RAGON_SYNC §18) |

**Count:** 33 families. Works today: 21 (F1-F4, F6-F15, F19-F21, F29, F30, F32, F33; F6 on loopback only). Unsafe: 6 (F16, F17, F18, F22, F26, F31). Closed in name: 1 (F24). Missing: 5 (F5, F23, F25, F27, F28). "Works" means that it works on one machine or over SSH; across machines every family also needs the front door (§6, §7).

### 5.8 What each family needs built

| F | 0.9.10 (§7.1, §7.2) | 1.0 (§7.3-§7.6) | Later |
|---|---|---|---|
| F1-F4, F7, F8 | nothing; tests only (A10) | — | — |
| F5 | A1, A2: the front door terminates TLS | — | — |
| F6 | A5 (default address), A6 (trust root) | Windows and macOS clients (§7.6) | — |
| F9-F15 | A3, A4 (one shared server); placement (§7.2) | the router (§7.3), so one connection sees every node | — |
| F16 | nothing. **Through the front door, every signed-in account gets this with no sandbox, over the network** | — | §9 items 1, 2 (3.0) |
| F17 | nothing. **The same: every signed-in account can run commands with the sandbox it picks**; `/airgapped on` keeps patch `0023` | — | §9 items 1, 3 (3.0) |
| F18 | nothing; Ragon does not start an exec-server | — | §9 item 7 (3.0) |
| F19, F20 | nothing; they work as in Codex | — | — |
| F21 | nothing (its updater is F22) | — | — |
| F22 | A9: close it | the primary's `install.sh` (§7.6, V12) | — |
| F23, F24 | — | the relay host side and the host-check patch (§7.4) | — |
| F25 | — | the controller side (§7.4); the phone page (§7.6) | a native app (§9 item 9) |
| F26 | A8: close the token leak | cloud tasks as node jobs (§7.5) | — |
| F27, F28 | — | §7.5 | — |
| F29 | nothing; it stays "none" | — | — |
| F30 | nothing (AUTH) | — | — |
| F31 | — | the Ragon desktop installer at Codex's path (§7.6, V14) | — |
| F32 | nothing | the router (§7.3) | — |
| F33 | nothing | the phone page drives every session (§7.6) | a native app |

## 6. The front door

### 6.1 What it is

The front door is the session WebSocket on the gate's TLS port `:8000`, at path `/`. The gate already serves every vendor path on that port (AUTH §5.1, §5.9). A request with `Upgrade: websocket` on `/` is a session attach; every other request goes to the gate's routes as today. One port, one certificate and one address serve the model API, sign-in, the account stubs, the relay, cloud and sessions. Under the ruling, that address is the primary's.

### 6.2 One attach, step by step (0.9.10)

1. The client opens `wss://<primary>.local:8000/` with `Authorization: Bearer <token>`. This is exactly what `codex --remote … --remote-auth-token-env VAR` sends (`app-server-client/src/remote.rs:698-759`).
2. The front door refuses an `Origin` header with 403, as Codex does (`app-server-transport/src/transport/websocket.rs:148-152`). It refuses a missing or bad bearer with 401 before the upgrade, as Codex does (`websocket-auth/src/lib.rs:383`).
3. The bearer is a Ragon account token, checked like every gate route (AUTH §5, §7). The 60 s cache and the "authentication server unreachable" answer are AUTH's.
4. The primary places the connection on a node (§7.2). Placement happens once, at the upgrade.
5. The front door on the placed node opens a WebSocket on that node's shared app-server socket (`$XDG_RUNTIME_DIR/ragon/app-server.sock`, `ragon-rs/web/src/app_server.rs:64-68`). It then copies frames both ways **unchanged**: text, binary, ping, pong and close.
6. Backpressure is passed through, and nothing is buffered without bound. So Codex's slow-consumer rule and its `-32001` overload error reach the client as they would directly (R9).
7. When either side closes, the other side is closed with the same code. The app-server therefore sees the same disconnect a direct client would cause (F12).

### 6.3 Why proxy, not redirect

- The Codex client follows no redirect: it dials the URL it was given and fails on any non-101 answer (`app-server-client/src/remote.rs:738`).
- Its URL check allows only path `/` and no query (`remote.rs:119-127`, Appendix B §1), so the client cannot name a node or a thread in the URL either.
- An unmodified client that knows only the primary's address therefore needs the primary to carry its frames.
- Ragon's own clients take the same route through the primary and follow no redirect (owner Q10).

### 6.4 What the front door does not do

- It does not filter or rewrite frames (R6). The bridge's `allowedRequests` (`ragon-rs/web/policy.json:2-3`) is for `ragon web` and the desktop app only. A front-door client gets the full app-server surface, as a Codex client does from `codex app-server --listen ws://`.
- The two requests that would reach the vendor from the server side are closed by server config, not by a frame filter. The node's shared app-server starts with `-c feedback.enabled=false` (until the primary's feedback store exists, Q2) and `-c features.realtime_conversation=false` (V3, V17).
- It does not check the bearer again during a live connection. Codex checks only at the upgrade too (§9).

## 7. What must be built

### 7.1 Direct attach (0.9.10)

| Part | What | How it stays compatible | Size |
|---|---|---|---|
| A1 | The session WebSocket in the gate (§6.2): upgrade on `/`, Origin 403, bearer 401, frame copy to the local socket | the client sees a Codex app-server: same handshake, same frames, same close codes | 2 days |
| A2 | `/readyz` and `/healthz` on the front door, with Codex's answers (`app-server-transport/src/transport/websocket.rs:57-75`) | same paths and status codes | ½ day |
| A3 | The shared app-server is always on. Today `ragon web` starts it on its first connect (`ragon-rs/web/src/app_server.rs:98`); 0.9.10 starts it with the gate (one per node, one per OS user) | the same server and the same socket, so `ragon web`, the desktop (Q10) and remote clients see the same threads (F10) | 1 day |
| A4 | The gate and the app-server run as the same OS user, because the socket checks the peer's user (`app-server-transport/src/transport/unix_socket.rs:282-299`) | unchanged Codex check | ½ day (a test) |
| A5 | `ragon --remote` with no address means `wss://<primary>.local:8000/`, and the token variable is filled from the signed-in account. An explicit address works as in Codex | Codex's flags, defaults and checks unchanged (R1); the only addition is the default when the address is left out, which Codex refuses | 1 day |
| A6 | The trust-root patch: the `--remote` dial also trusts `CODEX_CA_CERTIFICATE` (D4) | Codex ignores that variable on this path, so the patch adds trust and changes nothing else | 1 day |
| A7 | Server config: `-c feedback.enabled=false`, `-c features.realtime_conversation=false` on the shared server (§6.4) | `-c` overrides that Codex itself accepts | ½ day |
| A8 | Close the `ragon cloud` leak now: the launcher sets `CODEX_CLOUD_TASKS_BASE_URL={gate}/backend-api`. The host check (`cloud-tasks/src/util.rs:47-80`) then keeps the token from going out, and the gate answers the 404 shape | the same command, now failing like a Codex backend with no task service | ½ day |
| A9 | Close the daemon updater now: patch `INSTALL_URL` (`app-server-daemon/src/update_loop.rs:66-68`) to the primary's `install.sh` (owner Q3) | same commands; the update goes to `ragon update` | ½ day |
| A10 | Tests T1-T14 (§10) | — | 2 days |
| A11 | The Linux installer adds the primary's CA to the system store, removes it on uninstall and replaces it when the CA changes (§3.4, owner Q9). `ragon remote join` already does this on Debian (`ragon-rs/src/remote_join.rs:361-363`, `dreamference/remote/remote_join_script.py:118-121`); A11 moves it into the node and client installer and adds Fedora | an unmodified Codex trusts the primary through the OS roots, as it does on macOS and Windows | 1 day |

Total: about 10 agent-days. One lane can finish by 12-14 if it starts by 12-01; two lanes give a margin.

### 7.2 Placement and secondaries (0.9.10)

- **Who decides.** Every attach is placed by the primary (owner answer 3), through a small call: `POST {primary}/ragon/v1/sessions/place` with the account and a hint gives back a node id and a short-lived ticket signed by the primary.
- **The rule (owner Q6).** In order:
  1. the node where this account last attached, if it is up;
  2. otherwise the primary, if it has room;
  3. otherwise the least-loaded secondary that serves the account's model.
- **Attach at the primary.** If the chosen node is the primary, the frames go to the primary's own socket. If it is a secondary, the primary opens `wss://<secondary>.local:8000/` with the ticket, under the node CA, and copies frames on. The client sees only the primary's certificate.
- **Attach at a secondary.** A secondary that gets an attach asks the primary to place it, then serves it locally or forwards it as above. This is the 11-26 forwarding rule, applied to sessions.
- **The primary is unreachable.**
  - A secondary with the bearer in its AUTH cache serves the attach locally.
  - With nothing cached, it answers 401 "authentication server unreachable" (AUTH §7).
  - A client that knows only the primary's address cannot attach. That is the cost of one address, and it is stated, not hidden.
- **What 0.9.10 does not do.** A connection sees only the threads of the node it was placed on. `thread/list` therefore does not show threads that live on other nodes. The 1.0 router (§7.3) fixes this. The placement rule's first step makes it rare for one user.
- **Size.** About 3 agent-days: the place call, the ticket, node-to-node forwarding and the tests.

### 7.3 The router (1.0)

With the router, one client connection sees every node. The primary keeps the client's WebSocket and opens one upstream per node it needs.
- **Initialize.** The client's `initialize` and `initialized` are replayed to each new upstream, with the same `clientInfo` and capabilities. The client gets the primary's own answer. That answer has the same shape, and `codexHome` is replaced by a fixed value (§9).
- **Requests with a `threadId`** go to the node that holds the thread. The primary learns the map from `thread/start`, `thread/resume`, `thread/fork` and `thread/list` results.
- **Requests without one** go to the connection's home node, the one placed at the upgrade. These are `fs/*`, `command/exec`, `process/spawn`, `config/*`, `model/list` and `account/*`.
- **Lists.** `thread/list`, `thread/loaded/list` and the search requests ask every node; the answers are merged in Codex's sort order, under a cursor that the primary encodes.
- **Server requests.** The primary renumbers each upstream's server-request ids into one id space and maps the answer back. Approvals from several clients keep Codex's rule: the first answer wins, and `serverRequest/resolved` goes to all clients (F11).
- **Global notifications** (account, config, rate limits) from several upstreams are sent once.
- **Thread placement.** `thread/start` is placed per thread by the primary (§7.2 rule). `thread/fork` goes to the node of its source.
- **Test.** The router passes the same transcript tests as direct attach (§10).
- **Size.** About 8 agent-days.

### 7.4 Remote control through the primary (1.0)

- **The host side, exactly as Codex.** The primary serves `/backend-api/wham/remote/control/server/enroll`, `/refresh`, `/pair` and `/pair/status`, and the WSS `/server`. Their shapes, headers and envelopes are those of `app-server-transport/src/transport/remote_control/protocol.rs:106-312`: `client_id`, `stream_id`, `seq_id`, acks, chunks, and the `x-codex-installation-id`, `x-codex-host-device-kind` and `x-codex-subscribe-cursor` headers.
- **The host-check patch (D3).** Ragon's build accepts the primary's host in `protocol.rs:193-212` and `:283-285`. An unmodified Codex host can reach the primary only through a loopback forwarder on its own machine, because that check allows only the vendor's hosts and loopback. `ragon relay-forward` is a small listener on 127.0.0.1 that copies to the primary.
- **The controller side (D6).** Codex's source has none. Controllers speak the standard app-server protocol (owner Q7). A relayed host is one more place target. A controller is any session client attached at the front door, which places the connection on that host. The primary turns the client's frames into the host's envelopes and back.
  - So `ragon --remote`, an unmodified `codex --remote`, `ragon web` on the primary and the phone page all control a relayed host the same way.
  - The host is chosen by pairing: a paired host becomes the account's first placement choice while it is linked. `remoteControl/*` RPCs list and revoke paired clients, as in Codex.
- **Reconnect.** Hosts use Codex's backoff and the subscribe cursor. The primary keeps unacked envelopes per stream, so a controller that reconnects within Codex's window resumes.
- **Size.** About 10 agent-days.

### 7.5 Cloud tasks as node jobs (1.0)

- **The routes.** The primary serves `/backend-api/wham/tasks` (create), `/wham/tasks/list`, `/wham/tasks/{id}`, `…/sibling_turns`, `/wham/environments` and `environments/by-repo/…`, with the JSON shapes in `backend-client/src/client.rs:459-583`. The existing 404 shape goes away.
- **A task.** A task is a node job (owner answer 2). The primary places it, the node clones the environment's repository into a fresh worktree, and the node runs the prompt with `ragon exec` under the normal sandbox. The node stores the turns and the diff on the primary.
  - `codex cloud`'s list, status and diff then read these.
  - `codex apply TASK_ID` (V10) gets the same diff and applies it locally.
- **Environments.** An environment is a named repository URL plus a setup script, kept on the primary. The repository comes from the node forge (RAGON_SYNC), or from any other git URL the node can reach (owner Q8).
- **Patches.** `cloud-tasks/src/util.rs:47-80` accepts the primary's host (D3), and the launcher sets `CODEX_CLOUD_TASKS_BASE_URL` (A8). Patch `0006` is then removed and `ragon cloud` is shown.
- **What differs (D7).** Internet settings and the vendor's container images do not exist. The fields are accepted and listed as "node default".
- **Size.** About 11 agent-days.

### 7.6 The other re-points (1.0)

| Row | Build | Size |
|---|---|---|
| V12 | `{primary}/ragon/v1/install.sh` (and `.ps1`) that runs `ragon update` (A9 lands the constant, Q3) | 1 day |
| V13 | the release-feed path on the primary answers with Ragon's current release; the forced `check_for_update_on_startup=false` is lifted (Q5) | ½ day |
| V14 | the primary serves the Ragon desktop installer at the path `codex app` asks; patch the constant | 1 day |
| V15, V16 | patches `0013` and `0015` rewritten to send to the primary (Q5); accept-and-drop routes there, nothing stored (Q1) | 1 day |
| V17 | the upload constant at the primary; the feedback store on the primary, 30 days, admin-readable, never forwarded (Q2); then `feedback.enabled` back to true on the server; the TUI's `/feedback` stays removed | 1½ days |
| V18 build | the **plugin mirror at build time** (Q4): a release-build step on our build machines fetches the vendor's catalogue and the plugin sources it lists at a pinned date, drops plugins whose licence forbids redistribution (logged), writes `plugins-mirror.tar.zst` with a manifest and checksums, and ships it inside every Ragon release. Only the build machines contact the vendor; the step's destination is recorded in the release build's egress notes, never on a user's node | 2 days |
| V18 serve, V25, V26 | the primary serves the shipped catalogue merged with the node's own index (AUTH §5.9), the plugin git repo from the mirror, and apps at Codex's paths; `features.remote_plugin` lifted | 2 days, shared with AUTH's plan |
| V19, V24 | tips from a file shipped inside Ragon; pet art answered not-found; forced `tui.pet` lifted (Q5) | ½ day |
| V3, V22, V23, V27, V28, rest of Appendix C | every remaining vendor constant re-pointed at the primary (Q5), with Codex's not-found or feature-off answer there; T11 extended to every constant | 2 days |
| V21 | doctor's probes re-pointed at the primary (patch `0016` rewritten) | ½ day |
| Phone | the node's web page drives every remote session through the primary (§8) | 4 days |
| Win/Mac | trust and attach on Windows and macOS clients (§8) | 3 days |

## 8. How it fits Ragon

- **TLS and the node CA.**
  - The front door uses the gate's node certificate, the one AUTH §6 already issues; there is no new certificate.
  - Ragon clients pin `~/.ragon/nodes/<id>/ca.pem` and pass it with `CODEX_CA_CERTIFICATE`; A6 extends that to `--remote`.
  - On Linux the installer also adds the primary's CA to the system store (owner Q9, A11), so an unmodified Codex trusts it as on macOS and Windows.
  - Node-to-node forwarding (§7.2) uses the same CA.
  - Plain `ws://` stays on loopback (D1).
- **Accounts.** The bearer is the Ragon account access token. Every signed-in user may attach to every session (owner answer 3, the coordinator's reading). A per-owner rule is a 3.0 item (§9). Codex's own token modes stay for `ragon app-server --listen ws://… --ws-auth …` exactly as in Codex (R7).
- **`ragon web`'s single app-server.** The front door uses the same server and socket, so a session started on the web page is the same thread a remote TUI sees. Several clients behave exactly as in Codex (owner answer 4, F10-F13).
- **The desktop single-instance lock.**
  - The desktop app keeps its lock (`desktop/electron/src/bootstrap.ts:14`), but it starts its own stdio app-server (`desktop/electron/src/server.ts:64`).
  - Two app-servers on one `~/.ragon` can load the same rollout and write it twice. This was **not traced** and is a risk.
  - Decided: the desktop attaches to the node's shared server over the socket, like `ragon web`, and every Ragon client goes through the primary (owner Q10).
- **`/airgapped on`.**
  - Attach over the LAN or the overlay is a declared local-only exception (owner answer 10): one `declared_exceptions` entry for the primary's `:8000` (`dreamference/audit/egress_audit.py:811`).
  - The relay and cloud stay inside the node set, so no vendor destination appears.
  - A9 and A8 remove the two vendor destinations that a run could reach today.
- **The egress audit.** It sees one destination, the primary (§3.4). A Ragon run that contacts any vendor host is a failure, as today. Since every vendor constant points at the primary (Q5), this holds for data-only paths too. The plugin mirror's fetch happens on our build machines only (Q4) and never appears in a user's audit.
- **Secondaries and the primary.** Placement and forwarding are in §7.2, and the primary-down behaviour is AUTH §7's. Windows and macOS nodes are secondaries only (AUTH §7). They serve the front door the same way and are placed like Linux secondaries.
- **Windows and macOS clients.**
  - An unmodified Codex trusts the user store, which AUTH's installer fills (AUTH §6).
  - The daemon's Unix-only parts (the control socket, `/tmp/codex-daemon-<uid>/`) do not exist on Windows. The same is true in Codex.
- **The phone bridges.**
  - The phone uses the node's web page (owner answer 9), which `ragon web` serves. Through the primary, it lists and drives every session.
  - The messenger bridges (RAGON_SYNC) are notification paths, not session clients, and are unchanged.
  - A native app comes after 1.0 and is listed only.

## 9. Security beyond 0.9.9 (listed for 3.0, not designed)

1. A per-owner attach rule: who may attach to whose session. Today every signed-in user may attach to every session.
2. `fs/*` runs with no sandbox (`app-server/src/request_processors/fs_processor.rs:71-189`). Any attached client can read `auth.json` and keys and can write anywhere the server user can.
3. `command/exec` takes a sandbox chosen by the client (`app-server-protocol/src/protocol/v2/command_exec.rs:101`), and `process/spawn` runs with no sandbox.
4. The bearer is checked only at the upgrade, so a revoked token keeps a live connection open.
5. `initialize` returns the host's `codexHome` path, and `remoteControl/status/changed` returns the host name (observed).
6. The first client's name becomes the global originator for every client (`initialize_processor.rs:166-195`).
7. `exec-server` has no non-loopback refusal (F18).
8. An audit log of attaches, approvals and file writes per account.
9. A native phone app.

## 10. Conformance tests

### 10.1 The harness

- **Two binaries.** `codex` is built from the pinned tag with no patches, and `ragon` is built from main. CI builds the upstream binary once per tag, caches it, and never runs cargo inside `codex/`. The build copies the tree first.
- **A mock model.** A small Responses server answers fixed turns and fixed tool calls. The attach probe of this job used one (§1), so no GPU is needed.
- **Compared transcripts.** Every test records the JSON-RPC frames on both sides of the front door. It then compares them with the same run against an unmodified `codex app-server` on loopback.
  - Ids, timestamps, paths and the thread ids are normalized.
  - Everything else must be equal: methods, params, results, error codes and notification order.
- **No real machine.** Tests run in a temporary `CODEX_HOME`/`~/.ragon` and on loopback ports. They use a test CA. They never touch the node's model server.

### 10.2 The tests

| T | What | Rules | Runs in CI without a GPU | Needs a node |
|---|---|---|---|---|
| T1 | Linux trust (Q9): in a Debian and a Fedora container, the installer's CA step adds the test CA to the system store; an unmodified `codex --remote wss://…` then attaches with no variable set; uninstall removes it and the attach fails | R4, D4, A11 | yes | — |
| T2 | the flag and value checks of `--remote`, `--remote-auth-token-env`, `--listen`, `--ws-auth` and its sub-flags, in `ragon` and `codex`: same accept and refuse cases, same messages | R1 | yes | — |
| T3 | the config keys and env vars of Appendix B §9 and the key rows of Appendix B §1-§8 have the same defaults and effects (a table test over `config.toml`) | R2, R3 | yes | — |
| T4 | unmodified `codex` TUI and `codex exec` against the Ragon front door: start, resume, fork, list, archive; compared transcript | R4, R6 | yes | — |
| T5 | `ragon --remote` against an unmodified `codex app-server --listen ws://127.0.0.1:P --ws-auth capability-token`; the same script as T4 | R5, R7 | yes | — |
| T6 | three clients on one thread: fan-out, an approval answered by the second, `serverRequest/resolved` to all, the first answer wins | R9 | yes | — |
| T7 | disconnect and reconnect: the last client leaves mid-turn, the turn finishes, a new client resumes and gets the pending approval | R9 | yes | — |
| T8 | slow consumer and overload: a client that stops reading is disconnected; a flood gets `-32001`. Same as on loopback | R6, R9 | yes | — |
| T9 | file transfer and remote exec (`fs/*`, `command/exec`, `process/spawn`): same answers through the front door | R6 | yes | — |
| T10 | Origin 403, bad bearer 401, `/readyz` and `/healthz` | R4 | yes | — |
| T11 | vendor requests: every row of §3.1 and every constant of Appendix C reaches the primary with the same method, path, headers and body that Codex sends to the vendor; a fake vendor host on the test network sees nothing (Q5) | R10 | yes | — |
| T12 | the egress audit during T4-T9 sees only the primary | §3.4 | yes | — |
| T13 | placement and forwarding: two gates on loopback with test certs act as primary and secondary; attach at either one; the primary is down with and without a cached bearer | §7.2 | yes | — |
| T14 | a real node set: attach from another machine over the LAN and the overlay, with `/airgapped on`; Windows and macOS clients | §8 | — | yes |
| T15 | relay: an unmodified Codex host through `ragon relay-forward`, a Ragon host direct, a controller reconnect (1.0) | R4, D3, D6 | yes | — |
| T16 | cloud: `codex cloud exec`, `list`, `status`, `diff` and `codex apply` against the primary, where a task runs as a node job (1.0) | R4, D7 | with the mock model | the real run |
| T17 | router: T4-T9 again with threads on two nodes; merged `thread/list` (1.0) | R6, R9 | yes | — |
| T18 | plugins (Q4): a release build contains the mirror and its manifest; an unmodified Codex lists the catalogue and installs a plugin from the primary with the fake vendor host unreachable (1.0) | R10, Q4 | yes | — |

## 11. Phases

The owner's date and lanes (2026-11-27 22:07): **1.0 on Sun 2026-12-27, four lanes on remote sessions from Tue 2026-12-01.** 0.9.10 stays Mon 2026-12-14.

### 11.1 Size after the answers

| Phase | Date | Scope | Agent-days |
|---|---|---|---|
| 0.9.10 | Mon 2026-12-14 | §7.1 A1-A11 and §7.2: direct attach through the primary, placement (Q6), secondaries, the trust-root patch, the Linux system-store CA (Q9), closing A8 and A9, T1-T14 | about 13 |
| 1.0 | **Sun 2026-12-27** | §7.3 router (8), §7.4 relay (10), §7.5 cloud (11), §7.6 re-points of everything (about 12, Q4 and Q5 included), phone page (4), Win/Mac (3), T15-T18 (3½) | about 51½ |
| later | — | the native phone app; a Ragon environment registry for `exec-server --remote` (V23); everything in §9 (3.0) | — |

The answers added about 7½ agent-days to the earlier 57: the build-time plugin mirror (Q4, 2 + its test), re-pointing every data-only path and every Appendix C constant (Q5, about 4), and the Linux system-store step (Q9, 1). Total: **about 64½ agent-days**.

**Capacity.** Four lanes on the weekdays from Tue 12-01 to Thu 12-24 give 18 × 4 = **72 lane-days**, so about 7½ lane-days of margin. Code stops on Thu 12-24; Fri 12-25 to Sun 12-27 are the release candidate, the live run on the fleet (T14, T16 real run) and the release. Weekends are not counted; they are the margin's margin.

**The messenger bridges get what is left, and that is nothing from these four lanes.** The 7½ lane-days of margin are the remote sessions' only buffer and are not given away before Mon 12-21. If they are still unused then, the lanes take messenger docs and egress lines (2 days) in the last week. The messenger 1.0 scope (spec `DREAMFERENCE_RAGON_MESSENGERS_KAKAO_WECHAT.md` §12.2 with the owner's answers: WeChat iLink 4, the hooks listener 5, the KakaoTalk adapter 5, the WeChat add-on page for the user-installed unofficial client 1, docs and egress 2, about 17 days) therefore needs **a fifth lane from 12-01**, outside the four; without one it misses 12-27. **WeCom team mode goes to 1.0.1** (owner: in 1.0 only if a lane is free, and none is). Kakao push notifications are not in 1.0 either way (owner).

### 11.2 The lane plan, week by week

| Week | L1 front door and router | L2 client, trust, re-points | L3 relay and phone | L4 cloud and plugins |
|---|---|---|---|---|
| W1 Tue 12-01 - Fri 12-04 | A1 session WebSocket in the gate, A2, A3, A4, A7 | A5, A6, A11, A8, A9 | §7.4 host side: enrol, refresh, pair, `/server` WSS on the primary | §7.5 task store, `/wham/tasks*` routes and their shapes |
| W2 Mon 12-07 - Fri 12-11 | §7.2 placement, ticket, node-to-node forwarding; T13 | A10 tests T1-T12, T14 dry run; 0.9.10 rc on Thu 12-10; then V12, V13 | §7.4 controller side (D6), reconnect and the subscribe cursor, `ragon relay-forward` | environments (Q8: forge and reachable git URLs), the node job, the diff, `codex apply` |
| W3 Mon 12-14 - Fri 12-18 | **0.9.10 out Mon 12-14**; §7.3 router: initialize replay, thread map, id renumbering | V14, V15-V17 (feedback store), V19, V24, V21 | T15; phone page (§8) | patch `0006` removed, T16 on the mock; V18 build: the plugin mirror step |
| W4 Mon 12-21 - Thu 12-24 | router: merged lists, global notifications; T17 | V3, V22, V23, V27, V28 and the rest of Appendix C; T11 over every constant; Win/Mac (§8) | phone page done; T14 on the real node set | V18 serve, V25, V26; T18; T16 real run |
| Fri 12-25 - Sun 12-27 | 1.0 rc, fleet run, release | fix lane | fix lane | fix lane |

Per lane: L1 about 17, L2 about 16½, L3 about 15, L4 about 16 agent-days, against 18 each.

### 11.3 The two biggest risks to 12-27

1. **The router is on the critical path with about one day to spare.** It cannot start before placement lands (W2) and takes about 8 days, so it ends on Wed 12-23 at the earliest. A slip of 0.9.10 past 12-14 moves it one for one. If it is not done on 12-24, 1.0 ships with D9 (one node per connection) as a named difference, and that is the owner's call on 12-24, not before.
2. **The relay's controller side (D6) has no upstream peer to test against.** Codex's source contains only the host side, so the only proof is our own T15, and a mistake in the envelope handling shows up only against a real host. L3 has no slack before the phone page; if the relay runs over by more than two days, the phone page takes margin from L2.

### 11.4 First week, ready for dispatch on Mon 2026-11-30 (work from Tue 12-01)

| id | files | test command | one line |
|---|---|---|---|
| rs-w1-front-door | `dreamference/vllm_server/model_gate_service.py`, `dreamference/vllm_server/model_gate_tls.py` (if the upgrade needs it), new `tests/test_model_gate_session_ws.py` | `python -m pytest -q tests/test_model_gate_session_ws.py tests/test_model_gate_tls.py tests/test_model_gate_auth.py` | A1, A2: the session WebSocket on `/` of port 8000 (Upgrade header, Origin 403, bearer 401, frame copy to the local app-server socket) and Codex's `/readyz` and `/healthz` answers |
| rs-w1-shared-server | `ragon-rs/web/src/app_server.rs`, `ragon-rs/src/plugins.rs` (`vendor_settings`), the gate's start unit | `cargo test --release -p ragon-web-server app_server` and `cargo test --release -p ragon-launcher plugins` | A3, A4, A7: the shared app-server starts with the gate, one per node and OS user, same user as the gate, with `feedback.enabled=false` and `features.realtime_conversation=false` |
| rs-w1-remote-default | `ragon-rs/src/lib.rs` (the `--remote` argument path), `codex-patches/` (new patch for the bare `--remote`), new `ragon-rs/tests/remote_default.rs` | `cargo test --release -p ragon-launcher --test remote_default` | A5: `ragon --remote` with no address dials `wss://<primary>.local:8000/` and fills the token variable from the signed-in account; explicit addresses behave as in Codex |
| rs-w1-trust | new `codex-patches/0044-remote-ca-certificate.patch` (`app-server-client/src/remote.rs:738`), `ragon-rs/src/remote_join.rs`, the Linux installer, `tests/test_codex_branded_builder.py` | `python -m pytest -q tests/test_codex_branded_builder.py` and `cargo test --release -p ragon-launcher remote_join` | A6, A11: the `--remote` dial also trusts `CODEX_CA_CERTIFICATE`; the Linux installer adds, replaces and removes the primary's CA in the system store (Debian and Fedora) |
| rs-w1-close-two | `ragon-rs/src/lib.rs` (`codex_env`: `CODEX_CLOUD_TASKS_BASE_URL`), new `codex-patches/0045-daemon-install-url.patch` (`app-server-daemon/src/update_loop.rs:66-68`), gate route for `/ragon/v1/install.sh` | `python -m pytest -q tests/test_model_gate.py tests/test_codex_branded_builder.py` and `cargo test --release -p ragon-launcher codex_env` | A8, A9: `ragon cloud` goes to the gate and gets the 404 shape; the daemon updater fetches the primary's install script, which runs `ragon update` |
| rs-w1-relay-host | new `dreamference/remote/relay_host.py`, `dreamference/vllm_server/model_gate_service.py` (routes), new `tests/test_relay_host.py` | `python -m pytest -q tests/test_relay_host.py` | §7.4 host side: enrol, refresh, pair, pair/status and the `/server` WSS with Codex's envelopes and headers, tested against the host code at the pinned tag |
| rs-w1-cloud-store | new `dreamference/remote/cloud_tasks.py`, `dreamference/vllm_server/model_gate_service.py` (routes), new `tests/test_cloud_tasks_routes.py` | `python -m pytest -q tests/test_cloud_tasks_routes.py` | §7.5 first half: the task store on the primary and `/backend-api/wham/tasks`, `/tasks/list`, `/tasks/{id}`, `sibling_turns` with the shapes of `backend-client/src/client.rs:459-583` (no node job yet) |

Rows 1-2 and 3-5 are L1 and L2 (0.9.10); rows 6 and 7 are L3 and L4 (1.0). Every row also runs the egress audit tests it touches (`python -m pytest -q tests/test_egress_audit_auth.py`) and adds its CODEOWNERS line for new files.

## 12. Open questions

None. Q1-Q10 were answered by the owner on 2026-11-27 (22:11); the answers are in §0 and in the sections listed there.

## 13. Sources

- Codex at `rust-v0.158.0`; every path without a prefix is under `codex/codex-rs/`. A script checked that every cited file exists at that tag and that every cited line range lies inside it (§1). It checks where a citation points, not what the line says.
- Vendor docs, read on 2026-11-27:
  - the app-server page (`developers.openai.com/codex/app-server`);
  - remote connections (`developers.openai.com/codex/remote-connections`);
  - cloud (`developers.openai.com/codex/cloud`);
  - mobile (`developers.openai.com/codex/mobile`).
  - The daemon's README is read from the tree (`app-server-daemon/README.md`); its web copy did not load.
- Ragon specs: DREAMFERENCE_AUTH (§5.1, §5.8, §5.9, §6, §7, §10), DREAMFERENCE_RAGON_REMOTE_ACCESS, DREAMFERENCE_RAGON_SYNC (§18), DREAMFERENCE_RAGON_ASK, the desktop specs, the egress audit (§10.9), the airgapped spec (§5.2), `docs/dev/launcher.md`.
- The attach probe of this job (§1): a patched `ragon app-server` with a mock model on loopback, three clients, the unix socket and the TUI over `ws://`.

## Appendices

The appendices are the survey notes of this job, with paths made relative. In Appendix C, `{GATE}`, `BASE`, `PROV`, `compat` and `svc` are the abbreviations its head defines.

### Appendix A. Codex app-server JSON-RPC protocol: inventory for remote sessions

Source: `codex/codex-rs`, tag `rust-v0.158.0`. This is a read-only survey.
All paths are relative to `codex-rs/`. In the tables, `common.rs` means `app-server-protocol/src/protocol/common.rs`.

**Row counts.** There are 267 method rows:
- 170 client requests
- 11 server requests
- 85 server->client notifications
- 1 client->server notification

**Column legend.**

The experimental column takes one of these values:
- `gated` means `#[experimental("...")]` is on the method. The server rejects it unless the connection sent `capabilities.experimentalApi=true` (`app-server/src/message_processor.rs:975-978`).
- `fields` means `inspect_params`: the method itself is stable, but some params fields are experimental and are rejected without opt-in.
- `doc-only` means the doc comment says EXPERIMENTAL but nothing enforces it.
- `deprecated` means the method is marked deprecated or legacy.
- `no` means none of the above.

The vendor column names the vendor servers a method can contact:
- `backend` is the model or Responses API.
- `account` is vendor-account auth, billing or usage.
- `cloud` is Codex Cloud.
- `remote control` is `chatgpt.com` `/backend-api/wham/remote/control/server`.
- `analytics` and `feedback` are self-explanatory.
- `apps/connectors` covers apps, plugins and marketplace.

Only cells that cite a file:line were traced in code. Cells marked `(inferred)`, and plain `backend`/`account`/`no` cells without a citation, were assigned from the method's evident purpose and were not traced to an HTTP call.

**Analytics applies to more than the vendor column shows.** Every thread-scoped server request and notification is also passed to analytics: `ThreadScopedOutgoingMessageSender`, `app-server/src/outgoing_message.rs:199-209` and `:330`. The vendor column does not repeat this.

#### 0. Macro locations (`common.rs`)

| macro | definition | invocation | notes |
|---|---|---|---|
| `client_request_definitions!` | common.rs:212-462 | common.rs:498-1491 | `Variant => "method" { params, response }`; `inspect_params` marks field-level experimental checks |
| `server_request_definitions!` | common.rs:1497-1664 | common.rs:1761-1832 | serde `tag="method"`, `rename_all=camelCase`; unnamed variants become camelCase method names (1822, 1828) |
| `server_notification_definitions!` | common.rs:1668-1723 | common.rs:1916-2040 | `tag="method", content="params"`; `account/login/completed` is named via explicit `#[serde(rename)]` at 2035 |
| `client_notification_definitions!` | common.rs:1725-1751 | common.rs:2061-2063 | one variant only |
| `ServerNotificationEnvelope` | common.rs:~2042-2058 | n/a | adds optional `emittedAtMs` |
| Wire message types | app-server-protocol/src/rpc.rs:37 (`JSONRPCMessage`, untagged), :46 (`JSONRPCRequest`, optional W3C `trace`) | n/a | "We do not do true JSON-RPC 2.0" (rpc.rs:1-2): there is no `"jsonrpc"` field |

#### 1a. Client requests (client -> server), 170 rows

##### Connection and user verification

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| initialize | client request | common.rs:499 | performs the handshake (see §5); must be the first request | no | analytics (`track_initialize`); also sets the process-global originator and UA suffix used on all later vendor calls |
| server/diagnostics | client request | common.rs:507 | returns server diagnostics | gated | no |
| userVerification/status | client request | common.rs:514 | reads local user-verification (biometric/PIN) state | gated | no (inferred) |
| userVerification/enroll | client request | common.rs:521 | enrolls user verification | gated | no (inferred) |
| userVerification/delete | client request | common.rs:528 | removes enrollment | gated | no (inferred) |
| userVerification/verify | client request | common.rs:535 | runs a verification prompt | gated | no (inferred) |
| userVerification/cancel | client request | common.rs:542 | cancels a verification prompt | gated | no (inferred) |

##### Threads (lifecycle, metadata, queue, history)

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| thread/start | client request | common.rs:551 | creates a thread and subscribes this connection | fields | no (the first turn does) |
| thread/resume | client request | common.rs:557 | loads or rejoins a thread by id, `history` or `path`; subscribes this connection (see §3) | fields (`history`, `path`, `initialTurnsPage`) | `history` is "FOR CODEX CLOUD" (v2/thread.rs:354ff) |
| thread/fork | client request | common.rs:563 | forks a thread into a new one and subscribes | fields | no |
| thread/archive | client request | common.rs:569 | shuts down the thread and archives its rollout | no | no |
| thread/delete | client request | common.rs:574 | deletes a thread | no | no (inferred) |
| thread/unsubscribe | client request | common.rs:579 | removes this connection from the thread's subscribers; returns notLoaded, notSubscribed or unsubscribed (v2/thread.rs:719,733) | no | no |
| thread/increment_elicitation | client request | common.rs:589 | raises the outstanding-elicitation counter (pauses timeouts) | gated | no |
| thread/decrement_elicitation | client request | common.rs:598 | lowers the elicitation counter | gated | no |
| thread/name/set | client request | common.rs:603 | renames a thread | no | no |
| thread/goal/set | client request | common.rs:608 | sets the thread goal | no | no |
| thread/goal/get | client request | common.rs:613 | reads the goal | no | no |
| thread/goal/clear | client request | common.rs:618 | clears the goal | no | no |
| thread/queue/add | client request | common.rs:624 | adds a queued user message | gated | no |
| thread/queue/list | client request | common.rs:630 | lists the queue | gated | no |
| thread/queue/update | client request | common.rs:636 | edits a queued item | gated | no |
| thread/queue/delete | client request | common.rs:642 | removes a queued item | gated | no |
| thread/queue/reorder | client request | common.rs:648 | reorders the queue | gated | no |
| thread/queue/start | client request | common.rs:654 | starts processing the queue | gated | backend (starts turns) |
| thread/metadata/update | client request | common.rs:659 | updates thread metadata | fields | no |
| thread/attachment/add | client request | common.rs:665 | attaches a file or item to a thread | no | no (inferred) |
| thread/attachment/list | client request | common.rs:670 | lists attachments | no | no |
| thread/attachment/remove | client request | common.rs:675 | removes an attachment | no | no |
| thread/section/move | client request | common.rs:680 | moves a thread between sections | no | no |
| thread/settings/update | client request | common.rs:686 | changes thread settings (model, sandbox and others) | gated + fields | no |
| thread/memoryMode/set | client request | common.rs:693 | sets the memory mode | gated | no (inferred) |
| memory/status | client request | common.rs:699 | reports memory status | gated | no (inferred) |
| memory/reset | client request | common.rs:705 | resets memory | gated | no (inferred) |
| rollout/compress | client request | common.rs:712 | compresses rollout files | gated | no |
| thread/unarchive | client request | common.rs:717 | restores an archived thread | no | no |
| thread/compact/start | client request | common.rs:722 | compacts context | no | backend (inferred) |
| thread/shellCommand | client request | common.rs:727 | runs a user shell command inside the thread | no | no |
| thread/approveGuardianDeniedAction | client request | common.rs:732 | overrides an action the guardian auto-review denied | no | no (inferred) |
| thread/backgroundTerminals/clean | client request | common.rs:738 | kills finished background terminals | gated | no |
| thread/backgroundTerminals/list | client request | common.rs:744 | lists background terminals | gated | no |
| thread/backgroundTerminals/terminate | client request | common.rs:750 | terminates one background terminal | gated | no |
| thread/revert | client request | common.rs:755 | reverts the thread to an earlier turn; replaces the removed `thread/rollback` (app-server/README.md:320) | no | no |
| thread/list | client request | common.rs:760 | lists stored threads (paged) | fields | no |
| project/list | client request | common.rs:767 | lists projects | gated | no |
| project/read | client request | common.rs:773 | reads a project | gated | no |
| project/create | client request | common.rs:779 | creates a project | gated | no |
| project/import | client request | common.rs:785 | imports a project from a path | gated | no |
| project/update | client request | common.rs:791 | updates a project | gated | no |
| project/move | client request | common.rs:797 | moves a project | gated | no |
| project/delete | client request | common.rs:803 | deletes a project | gated | no |
| threadSection/list | client request | common.rs:808 | lists sidebar sections | no | no |
| threadSection/create | client request | common.rs:813 | creates a section | no | no |
| threadSection/update | client request | common.rs:818 | updates a section | no | no |
| threadSection/delete | client request | common.rs:823 | deletes a section | no | no |
| thread/search | client request | common.rs:829 | full-text search over threads | gated | no |
| thread/searchOccurrences | client request | common.rs:835 | finds match occurrences inside a thread | gated | no |
| thread/loaded/list | client request | common.rs:841 | returns the ids of threads loaded in server memory (v2/thread.rs:1625,1637) | no | no |
| thread/read | client request | common.rs:846 | reads a stored thread without subscribing | no | no |
| thread/turns/list | client request | common.rs:851 | pages through turns | no | no |
| thread/items/list | client request | common.rs:857 | pages through items | no | no |
| thread/inject_items | client request | common.rs:864 | appends raw Responses API items to history | no | no (they are sent to the backend on the next turn) |

##### Skills, hooks, marketplace, plugins, apps

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| skills/list | client request | common.rs:869 | lists skills | no | no |
| skills/extraRoots/set | client request | common.rs:874 | sets extra host directories that are scanned for skills | no | no |
| hooks/list | client request | common.rs:879 | lists hooks | no | no |
| marketplace/add | client request | common.rs:884 | adds a plugin marketplace source | no | apps/connectors (inferred; may fetch a remote source) |
| marketplace/remove | client request | common.rs:889 | removes a marketplace | no | no (inferred) |
| marketplace/upgrade | client request | common.rs:894 | refreshes a marketplace | no | apps/connectors (inferred) |
| plugin/list | client request | common.rs:899 | lists plugins | no | apps/connectors (inferred) |
| plugin/search | client request | common.rs:905 | searches plugins | gated | apps/connectors (inferred) |
| plugin/installed | client request | common.rs:910 | lists installed plugins | no | no (inferred) |
| plugin/reconcile | client request | common.rs:915 | reconciles installed plugins with config | no | apps/connectors (inferred) |
| plugin/read | client request | common.rs:920 | reads plugin details | no | apps/connectors (inferred) |
| plugin/skill/read | client request | common.rs:925 | reads a skill inside a plugin | no | no (inferred) |
| plugin/share/save | client request | common.rs:930 | saves or shares a plugin to the workspace | no | apps/connectors (inferred) |
| plugin/share/updateTargets | client request | common.rs:935 | changes share targets | no | apps/connectors (inferred) |
| plugin/share/list | client request | common.rs:940 | lists shared plugins | no | apps/connectors (inferred) |
| plugin/share/checkout | client request | common.rs:945 | checks out a shared plugin | no | apps/connectors (inferred) |
| plugin/share/delete | client request | common.rs:950 | deletes a share | no | apps/connectors (inferred) |
| app/read | client request | common.rs:955 | reads an app/connector | doc-only | apps/connectors (inferred) |
| app/list | client request | common.rs:960 | lists apps/connectors | doc-only | apps/connectors (inferred) |
| app/installed | client request | common.rs:965 | lists installed apps | doc-only | apps/connectors (inferred) |

##### Host filesystem, skill config, plugin install

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| fs/readFile | client request | common.rs:972 | reads any absolute host path and returns base64 (v2/fs.rs:7-22); runs with `sandbox: None` (fs_processor.rs:71) | no | no |
| fs/writeFile | client request | common.rs:977 | writes any absolute host path from base64 (v2/fs.rs:25-33; fs_processor.rs:90, sandbox None) | no | no |
| fs/createDirectory | client request | common.rs:982 | creates a host directory (sandbox None, fs_processor.rs:108) | no | no |
| fs/getMetadata | client request | common.rs:987 | stats a host path (fs_processor.rs:122) | no | no |
| fs/readDirectory | client request | common.rs:992 | lists a host directory (fs_processor.rs:141) | no | no |
| fs/remove | client request | common.rs:997 | deletes a host path (fs_processor.rs:169) | no | no |
| fs/copy | client request | common.rs:1002 | copies host paths (fs_processor.rs:189) | no | no |
| fs/watch | client request | common.rs:1007 | watches a host path, producing `fs/changed`; cleaned up when the connection closes | no | no |
| fs/unwatch | client request | common.rs:1012 | stops a watch | no | no |
| skills/config/write | client request | common.rs:1017 | writes skill enable/disable config | no | no |
| plugin/install | client request | common.rs:1022 | installs a plugin | no | apps/connectors (inferred) |
| plugin/uninstall | client request | common.rs:1027 | uninstalls a plugin | no | no (inferred) |

##### Turns, realtime, review, models

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| turn/start | client request | common.rs:1032 | starts a turn (user input plus per-turn overrides) | fields | backend |
| turn/settings/update | client request | common.rs:1039 | changes settings of the running turn | gated | no |
| turn/steer | client request | common.rs:1044 | injects input into the running turn | fields | backend |
| turn/interrupt | client request | common.rs:1050 | interrupts the running turn | no | no |
| thread/realtime/start | client request | common.rs:1056 | starts a realtime voice session | gated | backend (realtime) |
| thread/realtime/appendAudio | client request | common.rs:1062 | streams audio in | gated | backend (realtime) |
| thread/realtime/appendText | client request | common.rs:1068 | sends text in | gated | backend (realtime) |
| thread/realtime/appendSpeech | client request | common.rs:1074 | sends speech in | gated | backend (realtime) |
| thread/realtime/stop | client request | common.rs:1080 | stops realtime | gated | backend (realtime) |
| thread/timeline/list | client request | common.rs:1086 | lists the realtime timeline | gated | no |
| thread/realtime/listVoices | client request | common.rs:1092 | lists voices | gated | backend (inferred) |
| review/start | client request | common.rs:1097 | starts a code-review turn | no | backend |
| model/list | client request | common.rs:1103 | lists available models | no | backend model catalog (inferred) |
| account/gatewayOAuth/read | client request | common.rs:1108 | reads gateway OAuth state | no | account |
| account/gatewayOAuth/login | client request | common.rs:1113 | starts a gateway OAuth login | no | account |
| account/gatewayOAuth/cancel | client request | common.rs:1118 | cancels it | no | account (inferred) |
| modelProvider/capabilities/read | client request | common.rs:1123 | reads provider capabilities | no | no (inferred) |
| experimentalFeature/list | client request | common.rs:1128 | lists feature flags | no | no (inferred) |
| permissionProfile/list | client request | common.rs:1133 | lists permission profiles | no | no |
| experimentalFeature/enablement/set | client request | common.rs:1138 | toggles a feature flag (persisted to host config) | no | no |

##### Remote control, collaboration, environments

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| remoteControl/enable | client request | common.rs:1144 | enrolls the host and connects it outbound to the `chatgpt.com` relay | gated | remote control (transport/remote_control/protocol.rs:193-284) |
| remoteControl/disable | client request | common.rs:1150 | disconnects from the relay | gated | remote control |
| remoteControl/status/read | client request | common.rs:1156 | reads relay status | gated | remote control (inferred) |
| remoteControl/pairing/start | client request | common.rs:1162 | starts pairing a remote device | gated | remote control (`.../server/pair`) |
| remoteControl/pairing/status | client request | common.rs:1168 | polls pairing | gated | remote control (`.../pair/status`) |
| remoteControl/client/list | client request | common.rs:1174 | lists paired clients | gated | remote control (inferred) |
| remoteControl/client/revoke | client request | common.rs:1180 | revokes a paired client | gated | remote control (inferred) |
| collaborationMode/list | client request | common.rs:1187 | lists collaboration modes | gated | no |
| mock/experimentalMethod | client request | common.rs:1194 | test-only method | gated | no |
| environment/add | client request | common.rs:1201 | adds an execution environment | gated | no (inferred) |
| environment/info | client request | common.rs:1208 | reads environment info | gated | no (inferred) |
| environment/status | client request | common.rs:1215 | reads environment status | gated | no (inferred) |

##### MCP and Windows sandbox

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| mcpServer/oauth/login | client request | common.rs:1221 | runs OAuth for a configured MCP server | no | third-party MCP OAuth (not vendor) |
| config/mcpServer/reload | client request | common.rs:1227 | reloads MCP servers (spawns host processes) | no | no |
| mcpServerStatus/list | client request | common.rs:1233 | lists MCP server status | no | no |
| mcpServer/resource/read | client request | common.rs:1239 | reads an MCP resource | no | depends on the MCP server |
| mcpServer/event/stream/start | client request | common.rs:1246 | subscribes to an MCP event stream | gated | depends on the MCP server |
| mcpServer/event/stream/stop | client request | common.rs:1253 | unsubscribes | gated | no |
| mcpServer/tool/call | client request | common.rs:1259 | calls an MCP tool directly, without a turn | no | depends on the MCP server (apps/connectors when the MCP is the vendor connector MCP) |
| windowsSandbox/setupStart | client request | common.rs:1265 | starts Windows sandbox setup | no | no |
| windowsSandbox/readiness | client request | common.rs:1270 | checks readiness | no | no |

##### Account and feedback

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| account/login/start | client request | common.rs:1276 | starts vendor-account or API-key login | fields | account |
| account/bedrock/discover | client request | common.rs:1284 | discovers AWS Bedrock config | gated | AWS (not vendor) |
| account/bedrock/setup | client request | common.rs:1291 | configures Bedrock | gated | AWS (not vendor) |
| account/login/cancel | client request | common.rs:1297 | cancels login | no | account (inferred) |
| account/logout | client request | common.rs:1303 | logs out; deletes host credentials | no | account (inferred) |
| account/rateLimits/read | client request | common.rs:1309 | reads rate limits | no | account |
| account/rateLimitResetCredit/consume | client request | common.rs:1315 | spends a rate-limit reset credit | no | account |
| account/usage/read | client request | common.rs:1321 | reads token usage | no | account |
| account/workspaceMessages/read | client request | common.rs:1327 | reads workspace banner messages | no | account |
| account/sendAddCreditsNudgeEmail | client request | common.rs:1333 | asks the vendor to email an add-credits nudge | no | account |
| feedback/upload | client request | common.rs:1339 | uploads feedback, optionally with logs and rollout | no | feedback (inferred) |

##### Command and process execution (on the host)

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| command/exec | client request | common.rs:1346 | runs a one-off command with no thread (v2/command_exec.rs:30); the **client may pass `sandboxPolicy`** (not gated, command_exec.rs:101) and `disableTimeout`/`disableOutputCap` (:68,:73); defaults to the configured policy | fields (`permissionProfile`) | no |
| command/exec/write | client request | common.rs:1353 | writes stdin to it | no | no |
| command/exec/terminate | client request | common.rs:1359 | kills it | no | no |
| command/exec/resize | client request | common.rs:1365 | resizes its PTY | no | no |
| process/spawn | client request | common.rs:1372 | spawns a host process "without a Codex sandbox" (v2/process.rs:28) | gated | no |
| process/writeStdin | client request | common.rs:1379 | writes its stdin | gated | no |
| process/kill | client request | common.rs:1386 | kills it | gated | no |
| process/resizePty | client request | common.rs:1393 | resizes its PTY | gated | no |

##### Config and external-agent import

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| config/read | client request | common.rs:1399 | reads the effective config (contains host paths) | no | no |
| externalAgentConfig/detect | client request | common.rs:1404 | detects other agents' config on the host | no | no |
| externalAgentConfig/import | client request | common.rs:1409 | imports it | no | no |
| externalAgentConfig/import/recordHistory | client request | common.rs:1414 | records the import history | no | no |
| externalAgentConfig/import/readHistories | client request | common.rs:1419 | reads the import histories | no | no |
| config/value/write | client request | common.rs:1424 | writes one value to the host `config.toml` | no | no |
| config/batchWrite | client request | common.rs:1430 | writes several config values | no | no |
| configRequirements/read | client request | common.rs:1437 | reads managed requirements | no | no (inferred) |
| account/read | client request | common.rs:1443 | reads the current account | no | account (inferred; may refresh tokens) |

##### Deprecated and fuzzy search

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| getConversationSummary | client request | common.rs:1450 | v1 conversation summary | deprecated | no |
| gitDiffToRemote | client request | common.rs:1455 | v1: git diff of cwd against the remote | deprecated | no (runs git on the host) |
| getAuthStatus | client request | common.rs:1461 | v1 auth status | deprecated | account (inferred) |
| fuzzyFileSearch | client request | common.rs:1468 | one-shot fuzzy file search under host roots | no | no |
| fuzzyFileSearch/sessionStart | client request | common.rs:1474 | starts an incremental search session | gated | no |
| fuzzyFileSearch/sessionUpdate | client request | common.rs:1480 | updates the query | gated | no |
| fuzzyFileSearch/sessionStop | client request | common.rs:1486 | stops the session | gated | no |

#### 1b. Server requests (server -> client; the client must respond), 11 rows

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| item/commandExecution/requestApproval | server request | common.rs:1765 | approves a command; experimental fields are stripped per connection (app-server/src/transport.rs:180) | fields | no |
| item/fileChange/requestApproval | server request | common.rs:1772 | approves a patch | no | no |
| item/tool/requestUserInput | server request | common.rs:1778 | the model asks the user structured questions | doc-only | no |
| mcpServer/elicitation/request | server request | common.rs:1784 | an MCP server elicits input | no | no |
| item/permissions/requestApproval | server request | common.rs:1790 | approves a permission escalation | no | no |
| item/tool/call | server request | common.rs:1796 | executes a client-provided dynamic tool | no | no |
| `account/chatgptAuthTokens/refresh` | server request | common.rs:1801 | asks the client for fresh vendor-account tokens (externally managed auth) | no | account (tokens are used against the backend) |
| attestation/generate | server request | common.rs:1807 | asks the client for a device attestation; only sent if `capabilities.requestAttestation` is set | no | account/backend (the attestation is forwarded to the vendor; inferred) |
| currentTime/read | server request | common.rs:1814 | asks the client for its current time | gated | no |
| applyPatchApproval | server request | common.rs:1822 | v1 patch approval (legacy turns) | deprecated | no |
| execCommandApproval | server request | common.rs:1828 | v1 exec approval (legacy turns) | deprecated | no |

#### 1c. Notifications, server -> client, 85 rows

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| error | notification server->client | common.rs:1918 | turn or thread error | no | no |
| thread/started | notification server->client | common.rs:1919 | a thread was created | no | no |
| thread/status/changed | notification server->client | common.rs:1920 | thread status changed (idle, active, closing) | no | no |
| thread/archived | notification server->client | common.rs:1921 | a thread was archived | no | no |
| thread/deleted | notification server->client | common.rs:1922 | a thread was deleted | no | no |
| thread/unarchived | notification server->client | common.rs:1923 | a thread was unarchived | no | no |
| thread/closed | notification server->client | common.rs:1924 | a thread was unloaded or closed | no | no |
| thread/reverted | notification server->client | common.rs:1925 | a thread was reverted | no | no |
| skills/changed | notification server->client | common.rs:1926 | the skill set changed | no | no |
| thread/name/updated | notification server->client | common.rs:1927 | the thread was renamed | no | no |
| thread/attachment/updated | notification server->client | common.rs:1928 | attachments changed | no | no |
| thread/goal/updated | notification server->client | common.rs:1929 | the goal was set | no | no |
| thread/goal/cleared | notification server->client | common.rs:1930 | the goal was cleared | no | no |
| thread/queue/changed | notification server->client | common.rs:1932 | the queue changed | gated | no |
| project/changed | notification server->client | common.rs:1934 | a project changed | gated | no |
| thread/project/updated | notification server->client | common.rs:1936 | a thread's project changed | gated | no |
| thread/environment/connected | notification server->client | common.rs:1938 | an execution environment connected | gated | no |
| thread/environment/disconnected | notification server->client | common.rs:1940 | an environment disconnected | gated | no |
| thread/settings/updated | notification server->client | common.rs:1942 | thread settings changed | gated | no |
| thread/tokenUsage/updated | notification server->client | common.rs:1943 | token usage update (also sent on resume) | no | no |
| turn/started | notification server->client | common.rs:1944 | a turn started | no | no |
| hook/started | notification server->client | common.rs:1945 | a hook started | no | no |
| turn/completed | notification server->client | common.rs:1946 | a turn finished | no | no |
| hook/completed | notification server->client | common.rs:1947 | a hook finished | no | no |
| turn/diff/updated | notification server->client | common.rs:1948 | aggregated turn diff | no | no |
| turn/plan/updated | notification server->client | common.rs:1949 | plan update | no | no |
| item/started | notification server->client | common.rs:1950 | an item started | no | no |
| item/autoApprovalReview/started | notification server->client | common.rs:1951 | guardian review started | no | no |
| item/autoApprovalReview/completed | notification server->client | common.rs:1952 | guardian review finished | no | no |
| autoApprovalReview/strictReviewRequired | notification server->client | common.rs:1954 | strict review required | gated | no |
| item/completed | notification server->client | common.rs:1955 | an item finished | no | no |
| rawResponseItem/completed | notification server->client | common.rs:1957 | raw Responses API item; documented as internal, for Codex Cloud | no | cloud (consumer) |
| rawResponse/completed | notification server->client | common.rs:1959 | raw response finished | no | no |
| item/agentMessage/delta | notification server->client | common.rs:1960 | assistant text delta | no | no |
| item/plan/delta | notification server->client | common.rs:1962 | plan text delta | no | no |
| command/exec/outputDelta | notification server->client | common.rs:1964 | `command/exec` output; sent only to the requesting connection | no | no |
| process/outputDelta | notification server->client | common.rs:1967 | `process/spawn` output | gated | no |
| process/exited | notification server->client | common.rs:1970 | a process exited | gated | no |
| item/commandExecution/outputDelta | notification server->client | common.rs:1971 | tool command output | no | no |
| item/commandExecution/terminalInteraction | notification server->client | common.rs:1972 | stdin was written to a tool terminal | no | no |
| item/fileChange/outputDelta | notification server->client | common.rs:1974 | patch output | deprecated | no |
| item/fileChange/patchUpdated | notification server->client | common.rs:1975 | the patch changed | no | no |
| serverRequest/resolved | notification server->client | common.rs:1976 | a pending server request was answered or cancelled; sent to all subscribers (thread_lifecycle.rs:864) | no | no |
| item/mcpToolCall/progress | notification server->client | common.rs:1977 | MCP tool progress | no | no |
| mcpServer/oauthLogin/completed | notification server->client | common.rs:1978 | MCP OAuth finished | no | no |
| mcpServer/startupStatus/updated | notification server->client | common.rs:1979 | MCP startup status | no | no |
| mcpServer/event/stream/notification | notification server->client | common.rs:1981 | MCP event-stream payload | gated | no |
| account/updated | notification server->client | common.rs:1982 | account changed | no | no |
| account/gatewayOAuth/changed | notification server->client | common.rs:1983 | gateway OAuth changed | no | no |
| account/rateLimits/updated | notification server->client | common.rs:1984 | rate limits changed | no | no |
| app/list/updated | notification server->client | common.rs:1985 | the app list changed | no | no |
| remoteControl/status/changed | notification server->client | common.rs:1986 | relay status changed | no (the method is not gated, but the requests that produce it are) | no |
| externalAgentConfig/import/progress | notification server->client | common.rs:1987 | import progress | no | no |
| externalAgentConfig/import/completed | notification server->client | common.rs:1988 | import finished | no | no |
| fs/changed | notification server->client | common.rs:1989 | a watched host path changed | no | no |
| item/reasoning/summaryTextDelta | notification server->client | common.rs:1990 | reasoning summary delta | no | no |
| item/reasoning/summaryPartAdded | notification server->client | common.rs:1991 | new summary part | no | no |
| item/reasoning/textDelta | notification server->client | common.rs:1992 | raw reasoning delta | no | no |
| thread/compacted | notification server->client | common.rs:1994 | context was compacted | deprecated | no |
| model/rerouted | notification server->client | common.rs:1995 | the backend rerouted the model | no | no |
| model/verification | notification server->client | common.rs:1996 | model verification state | no | no |
| modelProvider/authRecoveryStarted | notification server->client | common.rs:1997 | provider auth recovery started | no | no |
| modelProvider/authRecoveryCompleted | notification server->client | common.rs:1998 | provider auth recovery finished | no | no |
| turn/moderationMetadata | notification server->client | common.rs:2000 | moderation metadata | gated | no |
| model/safetyBuffering/updated | notification server->client | common.rs:2001 | safety buffering state | no | no |
| warning | notification server->client | common.rs:2002 | generic warning | no | no |
| guardianWarning | notification server->client | common.rs:2003 | guardian warning | no | no |
| deprecationNotice | notification server->client | common.rs:2004 | deprecation notice, for example on a paginated resume with full turns | no | no |
| configWarning | notification server->client | common.rs:2005 | config warnings, sent right after initialize | no | no |
| fuzzyFileSearch/sessionUpdated | notification server->client | common.rs:2006 | fuzzy results | no | no |
| fuzzyFileSearch/sessionCompleted | notification server->client | common.rs:2007 | fuzzy search finished | no | no |
| thread/realtime/started | notification server->client | common.rs:2009 | realtime started | gated | no |
| thread/realtime/itemAdded | notification server->client | common.rs:2011 | realtime item added | gated | no |
| thread/realtime/item/started | notification server->client | common.rs:2013 | realtime item started | gated | no |
| thread/realtime/item/transcript/delta | notification server->client | common.rs:2015 | item transcript delta | gated | no |
| thread/realtime/item/completed | notification server->client | common.rs:2017 | realtime item finished | gated | no |
| thread/realtime/transcript/delta | notification server->client | common.rs:2019 | transcript delta | gated | no |
| thread/realtime/transcript/done | notification server->client | common.rs:2021 | transcript finished | gated | no |
| thread/realtime/outputAudio/delta | notification server->client | common.rs:2023 | output audio chunk | gated | no |
| thread/realtime/sdp | notification server->client | common.rs:2025 | WebRTC SDP | gated | no |
| thread/realtime/error | notification server->client | common.rs:2027 | realtime error | gated | no |
| thread/realtime/closed | notification server->client | common.rs:2029 | realtime closed | gated | no |
| windows/worldWritableWarning | notification server->client | common.rs:2032 | Windows world-writable directory warning | no | no |
| windowsSandbox/setupCompleted | notification server->client | common.rs:2033 | Windows sandbox setup finished | no | no |
| account/login/completed | notification server->client | common.rs:2035-2038 | login flow finished | no | no |

#### 1d. Notifications, client -> server, 1 row

| name | kind | file:line | what it does | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| initialized | notification client->server | common.rs:2061-2063 | the client acknowledges the initialize response | no | no |

**Methods that do not exist.**
- `thread/rollback` was removed (app-server/README.md:320). Use `thread/revert`.
- There is no explicit `thread/subscribe`. A connection subscribes as a side effect of `thread/start`, `thread/resume` or `thread/fork`, and leaves through `thread/unsubscribe` or a disconnect.

#### 2. Multi-client semantics (server side)

| item | kind | file:line | behavior | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| Per-connection session state | semantics | app-server/src/message_processor.rs:~176,182 | `ConnectionSessionState`/`InitializedConnectionSessionState` in a OnceLock; holds experimental_api_enabled, opted_out_notification_methods, client name and version, request_attestation and mcp extensions, all **per connection** | n/a | no |
| Pre-initialize gate | semantics | message_processor.rs:971-972 | any request before `initialize` fails with "Not initialized" | n/a | no |
| Double initialize | semantics | app-server/src/request_processors/initialize_processor.rs:72,145 | fails with "Already initialized" | n/a | no |
| experimentalApi is per connection | semantics | initialize_processor.rs:75-80 (TODO) | the opt-in is per connection, but threads are shared; the TODO admits this gives odd cross-client behavior | n/a | no |
| Thread subscription map | semantics | app-server/src/thread_state.rs:316 (`ThreadEntry.connection_ids`), :343 (`live_connections`, `threads`, `thread_ids_by_connection`) | each thread has a set of subscribed connections | n/a | no |
| Subscribe | semantics | thread_state.rs:556 (`try_ensure_connection_subscribed`), :586 (`try_add_connection_to_thread`) | start, resume and fork add the caller | n/a | no |
| Unsubscribe | semantics | thread_state.rs:512; thread_processor.rs:1018 | `thread/unsubscribe` removes the caller; the thread stays loaded | n/a | no |
| Notification routing | semantics | app-server/src/outgoing_message.rs:117 (`OutgoingEnvelope::{ToConnection,Broadcast}`), :143 (`ThreadScopedOutgoingMessageSender`) | thread events go to every subscribed connection; global events (account, config and others) are broadcast | n/a | analytics: `track_notification` (outgoing_message.rs:199-209) |
| Broadcast scope | semantics | app-server/src/transport.rs:204 (`route_outgoing_envelope`) | broadcasts go only to *initialized* connections | n/a | no |
| Per-connection filtering | semantics | transport.rs:104 (`should_skip_notification_for_connection`) | drops gated notifications for connections without experimentalApi, and drops exact-name `optOutNotificationMethods` | n/a | no |
| Per-connection field stripping | semantics | transport.rs:180 (`filter_outgoing_message_for_connection`) | strips experimental fields from command-approval requests | n/a | no |
| Server-request fan-out | semantics | outgoing_message.rs:330 (`send_request_to_connections`) | the **same request id** is sent to every subscriber | n/a | analytics (request tracked) |
| First responder wins | semantics | outgoing_message.rs:467 (`notify_client_response`), :561 (`take_connection_callback`) | the first answer resolves the request; later answers are ignored; `serverRequest/resolved` then goes to all subscribers (thread_lifecycle.rs:864) | n/a | no |
| Owner-only requests | semantics | outgoing_message.rs:~330 | user-verification elicitations go only to the owning connection: "Reconnect and other subscribers cannot answer it" | n/a | no |
| Request cancellation | semantics | outgoing_message.rs:215 (`abort_pending_server_requests`), :598 (`cancel_requests_for_thread`) | pending requests are cancelled when the turn state changes, for example on interrupt or turn end | n/a | no |
| Slow consumer | semantics | transport.rs:~160 | a connection whose outbound queue is full is **disconnected** | n/a | no |
| Inbound backpressure | semantics | app-server-transport/src/transport/mod.rs:24 (CHANNEL_CAPACITY=128), :51, :228 | when the incoming queue is full, the request is answered with error -32001 "Server overloaded; retry later." | n/a | no |
| Process-global originator | semantics | initialize_processor.rs:130-132,166-195 | the first non-exempt client's `clientInfo.name` and version set the global originator and UA suffix for **all** later vendor traffic, from every client | n/a | analytics, backend, account |
| Single-client mode | semantics | app-server/src/lib.rs:1146-1170 | in stdio single-client mode, closing the connection stops the server | n/a | no |

#### 3. Reconnect and resume

| item | kind | file:line | behavior | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| Disconnect cleanup | semantics | message_processor.rs:864-899 | closes and drains the rpc gate (CONNECTION_RPC_DRAIN_TIMEOUT), then tears down this connection's fs watches, command/exec and process/spawn children, and thread subscriptions | n/a | no |
| Pending server requests survive a disconnect | semantics | outgoing_message.rs:275 | `connection_closed` drops only the request contexts of the closed connection; pending approvals stay alive | n/a | no |
| Thread survives a disconnect | semantics | thread_state.rs:606 (`remove_connection`), thread_processor.rs:3560 | the closed connection is removed from each thread; threads left without subscribers start the unload timer | n/a | no |
| Auto-unload | semantics | app-server/src/request_processors/thread_lifecycle.rs:20-130 (`UnloadingState`), :416 | a thread unloads after `thread_unload_delay` only if it has **no subscribers and is not active**; a running turn keeps it loaded | n/a | no |
| Discover live threads | semantics | common.rs:841 / thread_processor.rs:851,2764 | `thread/loaded/list` returns the ids of threads in memory | n/a | no |
| Rejoin a running thread | semantics | thread_processor.rs:4241 (`resume_running_thread`), :4501 (enqueue), thread_lifecycle.rs:566-816 | `thread/resume{threadId}` on a loaded thread subscribes the caller. The response contains stored turns **merged with the in-flight active-turn snapshot**, followed by `thread/tokenUsage/updated`, a goal snapshot, and **replay of pending server requests** (thread_lifecycle.rs:808, outgoing_message.rs:446) | n/a | no |
| Closing race | semantics | thread_lifecycle.rs:566-816 | if an unload is pending, resume fails with -32600 "thread {id} is closing; retry" | n/a | no |
| Resume overrides | semantics | thread_processor.rs:4310-4354 | override mismatches (model, cwd, sandbox, config) are **ignored with a warning** when another subscriber exists, the thread is active, or shutdown fails. If the thread is idle, has no subscribers, is not running and accepts direct input, the server shuts it down and **cold-resumes it with the overrides** | n/a | no |
| Stale path | semantics | thread_processor.rs:4296-4305 | if `path` is given and differs from the active rollout path, resume fails, and the error echoes both host paths | n/a | no |
| history on a running thread | semantics | thread_processor.rs:4249-4260 | `history` with the id of a running thread is a hard error ("already running") | n/a | cloud (history mode) |
| excludeTurns / initialTurnsPage | semantics | app-server-protocol/src/protocol/v2/thread.rs:354ff | `excludeTurns` returns metadata only; `initialTurnsPage` (gated) pages the history; a paginated full-history resume emits `deprecationNotice` (thread_processor.rs:~4370) | fields | no |
| Daemon recovery | semantics | app-server-transport/src/transport/mod.rs:56 (`loaded-threads.json`), app-server/src/lib.rs:959,1023 | the daemon snapshots its loaded threads (that they are reloaded after a restart is inferred) | n/a | no |
| Client crate: no auto-reconnect | semantics | app-server-client/src/remote.rs:297,427-498 | `RemoteAppServerClient` emits `AppServerEvent::Disconnected{message}` and fails pending requests; it never reconnects | n/a | no |
| TUI reconnect loop | semantics | tui/src/app/reconnect.rs:1-3,30-120 | out-of-process only. Backoff is 0/1/2/4/8 s within a shared 120 s deadline. Each attempt opens a fresh connection, calls `initialize`, then `thread/resume` with `PreserveExistingThread`; "no user operation is retried"; "thread is closing" is retried; other -32600 errors mean the thread is gone | n/a | no |
| TUI endpoint selection | semantics | tui/src/lib.rs:444-480 (`resolve_remote_addr`), cli/src/main.rs:954 (`--remote-auth-token-env`) | `--remote` accepts `ws://host:port`, `wss://host:port`, `unix://` or `unix://PATH`; the bearer token is read from the named env var | n/a | no |
| Client token guard | semantics | app-server-client/src/remote.rs:119-127 | the client refuses to send a bearer token except over `wss://` or loopback `ws://` | n/a | no |
| Remote-control relay replay | semantics | app-server-transport/src/transport/remote_control/protocol.rs:106-190 | relay envelopes carry `client_id`, `stream_id` and `seq_id`. Backend `Ack{seq_id, segment_id}` lets the host keep only unacked chunks across a relay reconnect; there is also a `x-codex-subscribe-cursor` header (remote_control/websocket.rs:71) | gated (enable) | remote control |

#### 4. Framing per transport

| transport | kind | file:line | framing | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| Common wire format | framing | app-server-protocol/src/rpc.rs:1-2,37,46 | one JSON object per message: Request, Notification, Response or Error, untagged, with **no `"jsonrpc"` field**; a request may carry `trace` | n/a | no |
| Listen URL parse | framing | app-server-transport/src/transport/mod.rs:81,122 | `stdio://`, `unix://`, `unix://PATH`, `ws://IP:PORT`, `off`; ConnectionOrigin is Stdio, InProcess, WebSocket or RemoteControl (:198) | n/a | no |
| Bad JSON | framing | transport/mod.rs:211 | invalid JSON is logged and dropped; the connection stays open | n/a | no |
| stdio | framing | transport/stdio.rs:73 (reads with `lines()`), :141 (writes JSON+`\n`) | newline-delimited JSON; no line-length cap found | n/a | no |
| unix socket | framing | transport/unix_socket.rs:31 (mode 0600), :34, ~162-200 | WebSocket over UDS (`accept_hdr_async_with_config`); one text frame per message; the `x-codex-websocket-max-unfragmented-message-bytes` header advertises min(16 MiB frame, 64 MiB message); the socket defaults to `$CODEX_HOME/app-server-control/app-server-control.sock` (mod.rs:53-54); `/daemon/shutdown` uses a pid handshake (~211) | n/a | no |
| ws:// | framing | transport/websocket.rs:290 (outbound loop), :330 (inbound loop), :48 (outbound channel 32Ki) | axum WebSocket; one text frame per message, in both directions; ping gets a pong; **binary frames are dropped** with a warning; close ends the connection; no explicit size override found, so presumably the library defaults apply (16 MiB frame, 64 MiB message; unverified); **no TLS on the server listener** (only `ws://IP:PORT` is parsed; rustls in app-server-transport is used only by remote_control) | n/a | no |
| ws:// auth | framing | websocket.rs:89 (an `Origin` header gets 403), :111, :135-141; websocket-auth/src/lib.rs:279,288,385-394 | `Authorization: Bearer` with `--ws-auth capability-token\|signed-bearer-token`; the server refuses to start a non-loopback listener without auth; `/readyz` and `/healthz` at websocket.rs:148-151 | n/a | no |
| Version compat | framing | tui/src/app/startup.rs:860, tui/src/app/reconnect.rs:515, app-server-client/src/remote.rs:~852 | there is no version negotiation. The client parses the server version from `userAgent`, and the TUI only shows a notice on a mismatch | n/a | no |
| Client side (ws/uds) | framing | app-server-client/src/remote.rs:67,814-818,70 | the client allows 128 MiB frames and messages; UDS uses the handshake URL `ws://localhost/rpc` | n/a | no |
| remote control | framing | transport/remote_control/protocol.rs:106-190, segment.rs:19-23, websocket.rs:69-84 | an **outbound** WebSocket from the host to `wss://chatgpt.com/backend-api/wham/remote/control/server` (protocol v3). Envelopes are JSON `{type, client_id, stream_id, seq_id, ...}`. The JSON-RPC message sits inside `client_message`/`server_message`, or is split into base64 `*_message_chunk` segments (target 100 KiB, max 150 KiB per segment, max 1024 segments, 100 MiB reassembled). The envelope also has `ack`, `ping`/`pong` and `client_closed` | gated | remote control |

#### 5. Initialize handshake fields

| field | kind | file:line | meaning | experimental? | touches vendor servers? |
|---|---|---|---|---|---|
| InitializeParams | handshake | app-server-protocol/src/protocol/v1.rs:29 | `{clientInfo, capabilities}` | no | no |
| clientInfo.name | handshake | v1.rs:37; initialize_processor.rs:100-104,130-132 | must be a valid HTTP header value; the first non-exempt client sets the global originator; it also enables user verification for InProcess `codex-tui` and Stdio `Codex Desktop` only (:107-111) | no | backend/account (originator header) |
| clientInfo.title | handshake | v1.rs:37 | display name | no | no |
| clientInfo.version | handshake | v1.rs:37 | becomes the UA suffix `"{name}; {version}"` | no | backend/account |
| capabilities.explicitGatewayOauth | handshake | v1.rs:50 | opts into explicit gateway OAuth; **sticky** for later connections | no | account |
| capabilities.experimentalApi | handshake | v1.rs:53 | unlocks `gated` methods, fields and notifications for this connection only | no | no |
| capabilities.requestAttestation | handshake | v1.rs:56 | allows the server to send `attestation/generate` | no | account (inferred) |
| `capabilities.mcpServerOpenaiFormElicitation` | handshake | v1.rs:61 | legacy flag for form-style MCP elicitation | no | no |
| capabilities.optOutNotificationMethods | handshake | v1.rs:65 | exact method names to suppress for this connection | no | no |
| capabilities.extensions | handshake | v1.rs:69 | client MCP extensions | no | no |
| response.userAgent | handshake | v1.rs:74; initialize_processor.rs:212-217 | server UA (the client parses the version from it, remote.rs:~852) | no | no |
| response.codexHome | handshake | v1.rs:77 | **absolute host path** of CODEX_HOME | no | no |
| response.platformFamily | handshake | v1.rs:80 | `std::env::consts::FAMILY` | no | no |
| response.platformOs | handshake | v1.rs:83 | `std::env::consts::OS` | no | no |
| post-initialize | handshake | initialize_processor.rs:~185, send_initialize_notifications_to_connection | sends analytics `track_initialize`, then `configWarning` notifications to this connection; the client then sends the `initialized` notification | no | analytics |
| outbound state copy | handshake | app-server/src/lib.rs:1191-1211 | after initialize, the per-connection experimental and opt-out flags are copied into the outbound router | no | no |

#### Notes (surprises relevant to remote use)

- **Unsandboxed read and write of the host filesystem.**
  - `fs/readFile`, `fs/writeFile`, `fs/remove`, `fs/copy`, `fs/createDirectory`, `fs/readDirectory` and `fs/getMetadata` accept any `AbsolutePathBuf`. They run with `sandbox: None` (app-server/src/request_processors/fs_processor.rs:71-189). They are **not** experimental-gated.
  - Any authenticated remote client can read `~/.codex/auth.json`, SSH keys and similar files, and can write anywhere the server user can.
- **Arbitrary host execution.**
  - `process/spawn` (gated, but any client can opt in with `experimentalApi`) runs commands "without a Codex sandbox".
  - `command/exec` (not gated) runs with a client-chosen `sandboxPolicy`, or with the configured policy if none is given.
  - `thread/shellCommand` (not gated) runs a user shell command.
  - `config/value/write` and `config/batchWrite` can change the sandbox and approval policy for later sessions.
  - `config/mcpServer/reload` spawns configured MCP servers.
- **Host path leaks.**
  - The initialize response includes `codexHome`, `platformFamily` and `platformOs`.
  - The stale-path resume error echoes both the requested and the active rollout path (thread_processor.rs:4296-4305).
  - `config/read`, `thread/list` and `thread/read` return cwd and rollout paths.
  - `command/exec` and `fs/*` errors carry host paths.
- **The client chooses the sandbox.**
  - These fields are *not* experimental-gated: `command/exec.sandboxPolicy` (v2/command_exec.rs:101), `thread/start.sandbox` (v2/thread.rs:94) and `turn/start.sandboxPolicy` (v2/turn.rs:222).
  - A remote client can therefore request `danger-full-access`.
  - Whether managed `requirements.toml` constrains this was not traced.
  - `approvalPolicy` is gated, but only behind the client-controlled `experimentalApi` flag (thread.rs:88, turn.rs:215).
- **No TLS on the app-server listener.**
  - The server parses only `ws://IP:PORT`, and refuses a non-loopback listener without `--ws-auth`.
  - The stock client refuses to send a bearer token over non-loopback `ws://` (remote.rs:119-127).
  - So stock client to stock server across hosts needs a TLS-terminating proxy that serves `wss://` (and must not add an `Origin` header, which gets 403), or an SSH tunnel to loopback or to the unix socket.
- **Gating is per connection and opt-in by the client.** "Experimental" is not an access control: any client sets `experimentalApi: true`. `doc-only` methods such as `app/*` and `item/tool/requestUserInput` need no opt-in at all. Notifications for a shared thread differ between connections that have opted in and those that have not (initialize_processor.rs:75-80 TODO).
- **Approvals.**
  - Server requests go to every subscriber with the same id, and the first answer wins.
  - A disconnected client's pending approval stays pending and is replayed to whoever resumes the thread.
  - User-verification elicitations are the exception: they go only to the owner connection, and "reconnect and other subscribers cannot answer".
- **Global side effects from one client.**
  - The first client's `clientInfo.name` and version set the process-global originator and user agent for all vendor requests.
  - `explicitGatewayOauth` is sticky across connections.
  - `skills/extraRoots/set`, `experimentalFeature/enablement/set` and config writes affect every client.
- **Local-only or host-only methods.** These are meaningless or dangerous for a remote client:
  - `windowsSandbox/*` and `windows/worldWritableWarning`
  - `userVerification/*` (enabled only for in-process `codex-tui` or stdio `Codex Desktop`)
  - `externalAgentConfig/*`, which scans host configs of other agents
  - `fuzzyFileSearch*` and `gitDiffToRemote`, which operate on the host cwd
  - `account/bedrock/*`, which reads AWS config on the host
  - `/daemon/shutdown` on the unix socket
- **There is no session resumption at the transport level.**
  - The ws and uds client crate never reconnects. The TUI reconnect is a fresh `initialize` followed by `thread/resume`.
  - Only the remote-control relay has seq_id/ack replay, and that relay is vendor-hosted at `chatgpt.com`.
  - The relay URL is restricted to `chatgpt.com` and `chatgpt-staging.com`, or localhost (protocol.rs:193-284).
- **Backpressure disconnects.** A slow remote consumer is disconnected rather than throttled (transport.rs:~160). A full inbound queue returns -32001. Clients must handle both.
- **Binary WebSocket frames are dropped.** All payloads, including `fs/*` file contents, travel as base64 inside JSON text frames. Server-side limits are the tungstenite defaults; the client allows 128 MiB.
- **Analytics.** Every thread-scoped server request and notification is also passed to the analytics client (outgoing_message.rs:199-209,330), in addition to `track_initialize`.
- **Coverage gaps.**
  - Server-side handlers were traced for `fs/*`, resume, unsubscribe, initialize and routing.
  - Vendor contact marked `(inferred)` was not traced to an HTTP call.
  - `app-server-test-client` was listed but not read in detail. It is a test harness with plugin-analytics smoke tests.

### Appendix B. Codex remote-session functions outside the JSON-RPC method list (rust-v0.158.0)

Source: `codex/codex-rs` at tag `rust-v0.158.0`, read only.
All `file:line` paths are relative to `codex-rs/`, except section 10, which points at
`codex-patches/` and `ragon-rs/`.
The "Vendor?" column says whether the function needs the vendor's servers to work.

#### 1. App-server transports, the `--remote` client, and which subcommands take `--remote`

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `AppServerTransport` {Stdio, UnixSocket, WebSocket, Off} | transport | app-server-transport/src/transport/mod.rs:81-86 | The four listener kinds the app-server can run | no |
| `stdio://` (default listen URL) | transport | app-server-transport/src/transport/mod.rs:120 | Default `--listen`. Newline-delimited JSON-RPC on stdin/stdout (stdio.rs:73, :185) | no |
| `unix://` / `unix://PATH` | transport | app-server-transport/src/transport/mod.rs:127 | WebSocket framing over a Unix socket. An empty path means the default control socket | no |
| `ws://IP:PORT` | transport | app-server-transport/src/transport/mod.rs:156 | Plain WebSocket on TCP, served by axum. The value must parse as a SocketAddr, so it needs an IP, not a hostname | no |
| `off` | transport | app-server-transport/src/transport/mod.rs:152 | No listener (useful only with remote control) | no |
| no `wss://` listener | crate behaviour | app-server-transport/src/transport/mod.rs:100, :122-170 | The listen parser has no TLS branch. The error lists only stdio/unix/ws/off. The server never serves wss | no |
| `app-server --listen URL` | flag | cli/src/main.rs:565-572 | Picks the transport (default `stdio://`) | no |
| `app-server --stdio` | flag | cli/src/main.rs:575 | Shorthand for stdio. Conflicts with `--listen` | no |
| `app-server --remote-control` (hidden) | flag | cli/src/main.rs:578-580 | Starts with remote control enabled for this run only (EnabledEphemeral, :1249-1258) | yes, `chatgpt.com` |
| `app-server --managed-daemon` (hidden) | flag | cli/src/main.rs:583-584 | Marks the process as daemon-managed, which allows `/daemon/shutdown` over the socket | no |
| `app-server --analytics-default-enabled` | flag | cli/src/main.rs:601-602 | Turns analytics on by default (the analytics endpoint is the vendor's) | yes, if analytics is sent |
| `app-server proxy --sock SOCKET_PATH` | command | cli/src/main.rs:682, :1382-1390 | Relays raw stdio bytes to the control socket (default socket if omitted) | no |
| `codex stdio-to-uds SOCKET` | command | cli/src/main.rs:1787-1794; stdio-to-uds/src/lib.rs:12-46 | Raw byte relay between stdin/stdout and a UDS. Rejects `--remote` | no |
| WS banner + `/readyz` `/healthz` | crate behaviour | app-server-transport/src/transport/websocket.rs:57-75, :148-152 | Prints `ws://addr` and health URLs. HTTP health routes run beside the upgrade route | no |
| Origin-header rejection | crate behaviour | app-server-transport/src/transport/websocket.rs:89-99 | Any request carrying `Origin` gets 403, which blocks browser cross-site WS | no |
| non-loopback refusal | crate behaviour | app-server-transport/src/transport/websocket.rs:129-141 | Refuses a non-loopback `ws://` bind unless `--ws-auth` is set | no |
| WS framing | crate behaviour | app-server-transport/src/transport/websocket.rs:375-376 | Text frames carry JSON-RPC, Binary frames are dropped with a warning, Ping/Pong are answered | no |
| control socket path | file/socket path | app-server-transport/src/transport/mod.rs:53-56, :64-70 | `$CODEX_HOME/app-server-control/app-server-control.sock` (rendezvous name) | no |
| protected real socket | file/socket path | app-server-transport/src/transport/unix_socket.rs:282-299; uds/src/daemon_directory.rs:13-31 | The real socket is `/tmp/codex-daemon-<uid>/<sha256(canonical rendezvous path)>`, in a 0700 directory | no |
| socket perms / peer check | crate behaviour | app-server-transport/src/transport/unix_socket.rs:31, :56, :66-67; uds/src/lib.rs:65 | Socket 0600, directory 0700, owner checks. The peer must be the same non-elevated user | no |
| UDS WS handshake | crate behaviour | app-server-transport/src/transport/unix_socket.rs:168-190; app-server-client/src/remote.rs:70 | A WebSocket upgrade over the UDS (client dials `ws://localhost/rpc`) | no |
| `/daemon/shutdown` | crate behaviour | app-server-transport/src/transport/unix_socket.rs:172-178, :209-217 | Shutdown handshake (pid exchanged as text). Only for a Managed server, otherwise 403 "unmanaged server" | no |
| startup lock | file/socket path | app-server-transport/src/transport/mod.rs:72-78; unix_socket.rs:302 | `app-server-control/app-server-startup.lock` serialises startup | no |
| `loaded-threads.json` | file/socket path | app-server-transport/src/transport/mod.rs:58-62 | `$CODEX_HOME/app-server-daemon/loaded-threads.json`, used for daemon recovery | no |
| `--remote ADDR` (root) | flag | cli/src/main.rs:945-956 | Connects the TUI to a remote app-server: `ws://host:port`, `wss://host:port`, `unix://`, `unix://PATH` | no |
| `--remote-auth-token-env ENV_VAR` | flag | cli/src/main.rs:945-956, :2357-2375 | Reads the bearer token from the named env var (trimmed, must be non-empty). Requires `--remote` | no |
| `resolve_remote_endpoint` | crate behaviour | cli/src/main.rs:2507-2539 | A token may be sent only to `wss://` or loopback `ws://` | no |
| `resolve_remote_addr` | crate behaviour | tui/src/lib.rs:444-481 | Validates the remote URL: an explicit port, path `/`, no query or fragment | no |
| `websocket_url_supports_auth_token` | crate behaviour | app-server-client/src/remote.rs:119-124 | Bearer allowed only for wss or ws to localhost/loopback | no |
| `connect_websocket_endpoint` | crate behaviour | app-server-client/src/remote.rs:698-759 | Dials with `Authorization: Bearer` (:723-731) through tokio-tungstenite | no |
| client wss TLS roots | crate behaviour | Cargo.toml:512-515 | tokio-tungstenite `rustls-tls-native-roots`. wss is dialled using the OS root store, not CODEX_CA_CERTIFICATE | no |
| `connect_local_daemon` | crate behaviour | app-server-client/src/remote.rs:188-196, :761-795 | Connects to the daemon UDS with the NonElevatedCurrentUser peer policy | no |
| TUI auto-start of shared daemon | crate behaviour | tui/src/startup_orchestration.rs:494-497, :525-535 | Plain `codex` starts or joins the local daemon when `daemon_auto_start` is on (default on) | no (but see daemon updater) |
| daemon auto-start exclusions | crate behaviour | tui/src/daemon_startup.rs:25-50 | Skipped for `--no-daemon`, `--oss`, workload identity, `CODEX_EXEC_SERVER_URL`, `--profile`, `-c` | no |
| `--no-daemon` | flag | tui/src/cli.rs:83-85; tui/src/startup_orchestration.rs:16-18 | Uses an embedded app-server. Cannot be combined with `--remote` | no |
| default-daemon socket probe | crate behaviour | tui/src/lib.rs:509-530 | Probes the default control socket with a UDS connect | no |
| user verification blocked remotely | crate behaviour | app-server/src/user_verification.rs:63-70 | User verification is refused for WebSocket and RemoteControl connections | no |
| accepts `--remote`: TUI root, `agents` | command | cli/src/main.rs:1079-1129 | Interactive TUI against the remote. `agents` requires `--remote` on some platforms (:1118) | no |
| accepts `--remote`: `resume` | command | cli/src/main.rs:1437-1460 | Resumes a session on the remote server | no |
| accepts `--remote`: `fork` | command | cli/src/main.rs:1526-1547 | Forks a session on the remote server | no |
| accepts `--remote`: `archive` | command | cli/src/main.rs:1466, :879-908 | Archives a session on the remote server | no |
| accepts `--remote`: `unarchive` | command | cli/src/main.rs:1513, :879-908 | Restores an archived session on the remote server | no |
| accepts `--remote`: `delete` | command | cli/src/main.rs:1491, :879-908 | Deletes a session on the remote server | no |
| accepts `--remote`: `queue` | command | cli/src/main.rs:1479; cli/src/queue_cmd.rs:13 | Queues a prompt on the remote server | no |
| no `--remote` check: `tcp-tunnel` | command | cli/src/main.rs:1131-1132 | Dispatched without calling the reject helper | n/a |
| `reject_remote_mode_for_subcommand` | crate behaviour | cli/src/main.rs:2139-2156 | "`--remote` is only supported for interactive TUI commands". Applied to the subcommands in the next row | no |
| rejects `--remote`: `exec` | command | cli/src/main.rs:1135 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `review` | command | cli/src/main.rs:1154 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `mcp` | command | cli/src/main.rs:1172 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `plugin` | command | cli/src/main.rs:1184 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `app-server (every subcommand)` | command | cli/src/main.rs:1233, :2293-2328 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `remote-control` | command | cli/src/main.rs:1416 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `app` | command | cli/src/main.rs:1430 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `migrate-rollouts` | command | cli/src/main.rs:1506 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `login` | command | cli/src/main.rs:1554 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `logout` | command | cli/src/main.rs:1598 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `completion` | command | cli/src/main.rs:1610 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `update` | command | cli/src/main.rs:1618 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `doctor` | command | cli/src/main.rs:1626 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `cloud` | command | cli/src/main.rs:1640 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `sandbox` | command | cli/src/main.rs:1663, :1675 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `debug` | command | cli/src/main.rs:1710-1750 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `execpolicy` | command | cli/src/main.rs:1758 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `apply` | command | cli/src/main.rs:1767 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `responses-api-proxy` | command | cli/src/main.rs:1779 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `stdio-to-uds` | command | cli/src/main.rs:1788 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `exec-server` | command | cli/src/main.rs:1797 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| rejects `--remote`: `features` | command | cli/src/main.rs:1807-1850 | Errors out through `reject_remote_mode_for_subcommand` when given `--remote` | no |
| `websocket-client` crate | crate behaviour | websocket-client/src/lib.rs:56, :84-99; dialer.rs:49-147 | Generic WS dialler with custom CA and HTTP-proxy support. Used by other clients, not by app-server `--remote` | no |

#### 2. websocket-auth (shared by app-server and exec-server)

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `--ws-auth capability-token` | auth mode | websocket-auth/src/lib.rs:33, :62-65, :288-320 | A static bearer token, compared in constant time against its SHA-256 | no |
| `--ws-auth signed-bearer-token` | auth mode | websocket-auth/src/lib.rs:33, :335-372 | HS256 JWT with a shared secret. `exp` required; `nbf`, `iss`, `aud` checked | no |
| `--ws-token-file PATH` | flag | websocket-auth/src/lib.rs:37, :416, :436 | Token file. The trimmed content is the token. The path must be absolute | no |
| `--ws-token-sha256 HEX` | flag | websocket-auth/src/lib.rs:41 | Gives the token hash directly instead of a file | no |
| `--ws-shared-secret-file PATH` | flag | websocket-auth/src/lib.rs:45, :27, :403 | HS256 secret, at least 32 bytes | no |
| `--ws-issuer` | flag | websocket-auth/src/lib.rs:49 | Expected JWT `iss` | no |
| `--ws-audience` | flag | websocket-auth/src/lib.rs:53 | Expected JWT `aud` | no |
| `--ws-max-clock-skew-seconds` | flag | websocket-auth/src/lib.rs:57, :26 | Allowed clock skew (default 30) | no |
| settings exclusivity | crate behaviour | websocket-auth/src/lib.rs:156-230 | Rejects combinations of flags that do not belong to the chosen mode | no |
| `is_unauthenticated_non_loopback_listener` | crate behaviour | websocket-auth/src/lib.rs:279-284 | The predicate the app-server uses to refuse a public bind with no auth | no |
| `bearer_token_from_headers` | crate behaviour | websocket-auth/src/lib.rs:383 | Reads `Authorization: Bearer`. Checked only at upgrade, never per message | no |

#### 3. app-server-daemon

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `app-server daemon bootstrap [--remote-control]` | command | cli/src/main.rs:637, :688-690; app-server-daemon/src/lib.rs:790-834 | "Install durable local app-server management for SSH-driven use". Copies the package, saves settings, starts the pid backend and the updater | no (the updater: yes) |
| `app-server daemon start` | command | cli/src/main.rs:640; app-server-daemon/src/lib.rs:436-450 | Starts the managed backend, then `ensure_managed_updater` | no (the updater: yes) |
| `app-server daemon restart` | command | cli/src/main.rs:643; app-server-daemon/src/lib.rs:485-494 | Stops with a grace period and restarts | no |
| `app-server daemon update [--from-cli] [-y]` | command | cli/src/main.rs:646-653; app-server-daemon/src/lib.rs:286 | Updates the managed install | yes, `chatgpt.com` install script (unless `--from-cli`) |
| `app-server daemon enable-remote-control` | command | cli/src/main.rs:655-656; app-server-daemon/src/lib.rs:248, :269 | Sets the persisted remote-control preference and restarts with RC | yes, `chatgpt.com` |
| `app-server daemon disable-remote-control` | command | cli/src/main.rs:658-659 | Clears the RC preference | no |
| `app-server daemon stop` | command | cli/src/main.rs:662 | Stops the daemon through the socket shutdown | no |
| `app-server daemon version` | command | cli/src/main.rs:665 | Prints the daemon version | no |
| `app-server daemon pid-update-loop` (hidden) | command | cli/src/main.rs:667-675; app-server-daemon/src/lib.rs:276 | The internal updater loop process | yes, `chatgpt.com` |
| pid backend launch args | crate behaviour | app-server-daemon/src/backend/pid.rs:376-386; pid_start.rs:116 | Runs `codex app-server [--remote-control] --listen unix:// --managed-daemon`. No systemd or launchd units | no |
| daemon state files | file/socket path | app-server-daemon/src/lib.rs:50-56 | `$CODEX_HOME/app-server-daemon/{daemon.pid, daemon-updater.pid, daemon.lock, settings.json}` plus legacy `app-server.pid` and `app-server-updater.pid` | no |
| backend stderr log | file/socket path | app-server-daemon/src/backend/pid.rs:448-449 | `<pid>.stderr.log` | no |
| managed install root | file/socket path | app-server-daemon/src/managed_install.rs:21; prepare_install.rs:224-256 | `$CODEX_HOME/packages/app-server-daemon/releases/<ver-target or local-digest-target>` and a `current` symlink. This is a local copy | no |
| `settings.json` | config key | app-server-daemon/src/settings.rs:14-35, :44-96 | camelCase: `remoteControlEnabled` (false), `featureOverrides`, `updater.autoUpdateEnabled` (TRUE), update interval 60 min, shutdown grace 60 s (max 300) | no |
| updater gate | crate behaviour | app-server-daemon/src/lib.rs:869-900; managed_install.rs:74-115 | Runs only if auto-update is on and `current` is a stable `X.Y.Z-<target>` release with a matching `auto-update-version` marker | no |
| marker written by the daemon itself | crate behaviour | app-server-daemon/src/prepare_install.rs:223, :253, :268-334; lib.rs:430, :467, :800 | `prepare` runs on daemon start/restart/bootstrap (including TUI auto-start). For a stable-versioned package (workspace version is `0.158.0`, Cargo.toml:160) it names the release `X.Y.Z-<target>` and writes `auto-update-version`, which can make the updater eligible. Whether it fires depends on a package layout being present (prepare_install.rs:28-40); not verified end to end | yes, `chatgpt.com` if it fires |
| updater `INSTALL_URL` | crate behaviour | app-server-daemon/src/update_loop.rs:63, :66-68, :650-690 | First check after 5 min, then every 60 min. Downloads `https://chatgpt.com/codex/install.sh` (`install.ps1` on Windows) | yes, `chatgpt.com` |
| `run_installer_script` | crate behaviour | app-server-daemon/src/update_loop.rs:532-625 | Runs the downloaded script with `/bin/sh` or powershell | yes, `chatgpt.com` |
| installer env (`CODEX_INSTALL_DEFER_SELECTION`, `CODEX_INSTALL_DAEMON_ONLY`, `CODEX_RELEASE=latest`, `CODEX_NON_INTERACTIVE=1`, `CODEX_INSTALL_IF_LATEST`, `CODEX_INSTALL_IF_CURRENT`, `CODEX_UPDATE_FROM_RELEASE`, `CODEX_HOME`) | env var | app-server-daemon/src/update_loop.rs:532-625 | Passed to the installer script | yes, `chatgpt.com` |
| `CODEX_DAEMON_TELEMETRY_HANDOFF` | env var | app-server-daemon/src/telemetry.rs:7 | Hands telemetry context to the daemon child | no |
| `CODEX_DAEMON_SHUTDOWN_SOCKET` | env var | app-server-transport/src/lib.rs:9 | Marks a managed launch that accepts the socket shutdown | no |
| `CODEX_DAEMON_SHUTDOWN_FILE` | env var | app-server-transport/src/daemon_shutdown.rs:14 | Windows updater shutdown through a request file | no |
| daemon client | crate behaviour | app-server-daemon/src/client.rs:25-26 | Control-socket JSON-RPC client named `codex_app_server_daemon` | no |
| daemon install confirm | crate behaviour | cli/src/daemon_install.rs | Asks "Copy this package? [y/N]" before copying the CLI package | no |
| TUI daemon recovery | crate behaviour | tui/src/daemon_recovery.rs:99 | Restarts the daemon with the current features when it is lost | no |
| archive/queue reuse daemon | crate behaviour | tui/src/session_archive_commands.rs:79, :231-270 | Uses the implicit daemon socket, otherwise an embedded app-server | no |
| linux sandbox masks daemon socket directory | crate behaviour | linux-sandbox/src/bwrap.rs:429-435; daemon_mounts.rs:24 | Under bwrap, sandboxed commands cannot reach `/tmp/codex-daemon-<uid>` (AF_UNIX itself is allowed, landlock.rs:221) | no |

#### 4. Remote control

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `codex remote-control` (foreground) | command | cli/src/remote_control_cmd.rs:64-96, :110-130 | No subcommand: runs an app-server on a temporary `/tmp/codex-rc-*/rc.sock` with RC EnabledEphemeral | yes, `chatgpt.com` |
| `remote-control start` | command | cli/src/remote_control_cmd.rs:53-61 | Starts the daemon with RC enabled | yes, `chatgpt.com` |
| `remote-control stop` | command | cli/src/remote_control_cmd.rs:53-61 | Stops RC | no |
| `remote-control pair` | command | cli/src/remote_control_cmd.rs:53-61, :472-484 | Prints a short-lived "Pairing code: ..." | yes, `chatgpt.com` `/pair` |
| `remote-control --json` | flag | cli/src/remote_control_cmd.rs:32-35 | Machine-readable output | n/a |
| outbound control WSS | transport | app-server-transport/src/transport/remote_control/protocol.rs:300-312 | Dials `wss://chatgpt.com/backend-api/wham/remote/control/server`, plus `/enroll`, `/refresh`, `/pair`, `/pair/status`. It is outbound only, so no port is opened | yes, `chatgpt.com` |
| allowed RC hosts | crate behaviour | app-server-transport/src/transport/remote_control/protocol.rs:193-212 | Only https `chatgpt.com`, `chatgpt-staging.com` and their subdomains, or http/https localhost | yes (localhost only for testing) |
| `remote_control_url` = `chatgpt_base_url` | config key | app-server/src/lib.rs:850 | The RC endpoint is derived from the `chatgpt_base_url` config key | yes, `chatgpt.com` |
| RC auth | auth mode | app-server-transport/src/transport/remote_control/auth.rs:64, :98-143 | vendor-account login only ("API key auth is not supported"). Sends the bearer and `chatgpt-account-id` | yes, vendor account |
| RC headers | crate behaviour | remote_control/server_api.rs:30; host_device.rs:4 | `x-codex-installation-id`, `x-codex-host-device-kind` | yes |
| RC startup modes | crate behaviour | app-server-transport/src/transport/remote_control/mod.rs:66-84; app-server/src/lib.rs:862-880 | Start modes ResolvePersisted, DisabledEphemeral, EnabledEphemeral. Policy Allowed or DisabledByRequirements | no |
| RC persistence | file/socket path | remote_control/persistence.rs:82, :120-149 | Enrollment and the preference are stored in the SQLite state DB (`set_remote_control_enabled`). RC fails if the DB is missing (app-server/src/lib.rs:831-845) | no |
| RC client list/revoke | crate behaviour | remote_control/mod.rs:798, :807 | Lists and revokes paired controllers | yes, `chatgpt.com` |
| `CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED=1` | env var | app-server-transport/src/transport/remote_control/mod.rs:87-97; cli/src/main.rs:1015 | Read and then removed at startup. Forces RC off for this run | no |
| `allow_remote_control` | config key (requirements.toml) | config/src/config_requirements.rs:184, :1054; app-server/src/request_processors/config_processor.rs:496 | An admin requirement that disables RC (DisabledByRequirements) | no |
| `[features] remote_control` | config key | features/src/lib.rs:1768-1771 | Feature flag, stage Removed, kept only for compatibility | no |
| RC connection origin | crate behaviour | app-server-transport/src/transport/remote_control/client_tracker.rs:172 | `ConnectionOrigin::RemoteControl`: a full JSON-RPC client, except user verification is blocked | yes |
| agent identity | auth mode | agent-identity/src/lib.rs:41-44 | Issuer `https://chatgpt.com/codex-backend/agent-identity`, authapi `https://auth.openai.com/api/accounts` | yes, `chatgpt.com` and `auth.openai.com` |

#### 5. agents / queue / archive / delete / unarchive / app

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `codex agents [--remote]` | command | cli/src/main.rs:144, :333-340, :1051-1129 | Browses all sessions on the shared daemon or the remote. Bails under workload identity (:1107) | no |
| `agents --cd DIR` / `-C DIR` | flag | cli/src/main.rs:340-342 | "Use this directory for new tasks on a remote server" | no |
| `codex queue` | command | cli/src/main.rs:206, :1479; cli/src/queue_cmd.rs:13 | Queues a prompt on the daemon or the remote | no |
| `codex archive` | command | cli/src/main.rs:209, :376-382, :1466 | Archives a session (daemon, remote, or embedded) | no |
| `codex unarchive` | command | cli/src/main.rs:218, :1513 | Restores an archived session | no |
| `codex delete` | command | cli/src/main.rs:212, :402, :1491 | Deletes a session | no |
| `codex app [PATH]` | command | cli/src/app_cmd.rs:5-13; cli/src/main.rs:1429-1435 | Opens the desktop app with `codex://threads/new?...` (mac.rs:156, windows.rs:73). It does not use the daemon | yes, to download the app |
| `app --download-url` | flag | cli/src/app_cmd.rs:5-13 | Overrides the installer download URL | yes by default |
| desktop installer URLs | crate behaviour | cli/src/desktop_app/mac.rs:10-12, :237; windows.rs:7-8 | `persistent.oaistatic.com/codex-app-prod/Codex.dmg` (curl); the Windows store 9PLM9XGG6VKS | yes, `oaistatic.com` or the Windows store |

#### 6. exec-server and exec-server-protocol

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `codex exec-server` | command | cli/src/exec_server_command.rs:31-33; cli/src/main.rs:235-236, :1797 | Runs a standalone executor that serves process, fs and http JSON-RPC | no (local mode) |
| exec-server `--listen` | flag | cli/src/exec_server_command.rs:59-65; exec-server/src/server/transport.rs:40, :74-87 | `ws://IP:PORT` (default `ws://127.0.0.1:0`), `stdio` or `stdio://`. No wss listener | no |
| exec-server WS listener | transport | exec-server/src/server/transport.rs:178-215 | axum `/` upgrade and `/readyz`. Origin header gets 403 (:232-234). Prints `ws://addr` | no |
| exec-server lacks a non-loopback refusal | crate behaviour | exec-server/src/server/transport.rs:1-2, :178-195 | Doc: "unauthenticated listeners remain supported for compatibility". `is_unauthenticated_non_loopback_listener` is called only by the app-server (websocket.rs:135), so `--listen ws://0.0.0.0:P` without `--ws-auth` serves process, fs and http to anyone who can reach the port | no |
| exec-server stdio + ws-auth | crate behaviour | exec-server/src/server/transport.rs:112-118 | "WebSocket auth requires a WebSocket listener, not stdio" | no |
| exec-server `--ws-auth*` | flag | cli/src/exec_server_command.rs:68 | Same websocket-auth flags as section 2 | no |
| `--strict-config` | flag | cli/src/exec_server_command.rs:31-48 | Rejects an unknown config | no |
| `--linux-sandbox-pid-namespace` | flag | cli/src/exec_server_command.rs:48 | PID-namespace mode (default isolate) | no |
| `--concurrent-requests` | flag | cli/src/exec_server_command.rs:31-58 | Request concurrency (default 1) | no |
| `--remote BASE_URL` | flag | cli/src/exec_server_command.rs:70-78 | Registers this exec-server as a remote environment with a registry. Requires `--environment-id` | yes, in practice a vendor registry (needs vendor-account or API-key auth) |
| `--remote-transport noise\|direct` | flag | cli/src/exec_server_command.rs:80-89 | Noise relay (default) or direct. Direct requires `--aws-sigv4` | yes, with `--remote` |
| `--environment-id`, `--name` | flag | cli/src/exec_server_command.rs:92, :96 | Remote environment identity | yes, with `--remote` |
| `--use-agent-identity-auth` | flag | cli/src/exec_server_command.rs:99-106, :345-365 | Agent-identity auth taken from `CODEX_ACCESS_TOKEN` | yes, `chatgpt.com` |
| `--aws-sigv4`, `--aws-profile`, `--aws-region`, `--aws-service` (execute-api) | flag / auth mode | cli/src/exec_server_command.rs:109-131; cli/src/exec_server_auth.rs:20 | Signs registration with AWS SigV4 | yes, the AWS API gateway behind the registry |
| `--exit-on-stdin-close` / `CODEX_EXEC_SERVER_EXIT_ON_STDIN_CLOSE` | flag / env var | cli/src/exec_server_command.rs:140; exec-server/src/lib.rs:60-62 | Exits when the parent's stdin pipe closes | no |
| `exec-server forward --connect URL` | command | cli/src/exec_server_command.rs:336-343 | Registers an existing WS exec-server as a remote environment | yes, the registry |
| remote registration auth | auth mode | cli/src/exec_server_command.rs:345-424 | Needs vendor-account or API-key login. API key is allowed only to https `openai.com/openai.org` (and subdomains) or loopback | yes, the vendor |
| `register_environment` | crate behaviour | exec-server/src/remote.rs:131-155 | `POST {base}/cloud/environment/{id}/register` | yes |
| Noise relay | transport | exec-server/src/remote.rs:59, :711-745; noise_channel.rs:35, :135 | Rendezvous relay with `noise_hybrid_ik_v1`, prologue `codex-exec-server-relay-noise/v1` | yes, the relay |
| `CODEX_EXEC_SERVER_URL` | env var | exec-server/src/environment.rs:56, :79; environment_provider.rs:50-64 | Points sessions at an exec-server. `none` disables environment access. Also disables daemon auto-start | no (unless the URL is a vendor's) |
| `CODEX_EXEC_SERVER_NOISE_REGISTRY_URL` / `_ENVIRONMENT_ID` / `_CHATGPT_ACCOUNT_ID` | env var | exec-server/src/environment.rs:57-62 | Client side of the Noise relay environment | yes |
| `CODEX_API_KEY`, `CODEX_ACCESS_TOKEN`, `CODEX_CONNECTORS_TOKEN` | env var | exec-server/src/client/route_aware_http_client.rs:49-51 | Credentials for the exec-server HTTP routes | yes, the vendor |
| `$CODEX_HOME/environments.toml` | file/socket path | exec-server/src/environment_toml.rs:24, :225 | Named execution environments | no |
| environments.toml keys: `default`, `include_local`, `[[environments]]` | config key | exec-server/src/environment_toml.rs:28-35 | Default environment id and whether `local` is offered | no |
| `[[environments]]` `id`, `url`, `auth_bearer_token`, `program`, `args`, `env`, `cwd`, `connect_timeout_sec`, `initialize_timeout_sec` | config key | exec-server/src/environment_toml.rs:38-51, :113-150, :300-312 | `url` (ws/wss) dials an exec-server. `program`/`args`/`env`/`cwd` spawn one over stdio (e.g. ssh) | no |
| exec-server-protocol methods | crate behaviour | exec-server-protocol/src/protocol.rs:22-57 | `initialize`, `process/start\|read\|write\|signal\|terminate\|output\|exited\|closed`, `environment/info\|status`, `fs/*` (13 methods), `capabilityRoots/discoverV1`, `http/request`, `http/request/bodyDelta` | no |

#### 7. tcp-tunnel and other port forwarding

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `codex tcp-tunnel` (hidden, "Internal") | command | cli/src/main.rs:147-149, :1131-1132; tcp-tunnel/src/lib.rs:1 | Forwards a loopback TCP listener through an HTTP/3 (MASQUE) CONNECT proxy | depends on the proxy URL; no host is built in |
| `--proxy-url` | flag | tcp-tunnel/src/lib.rs:47-49 | HTTPS origin of the HTTP/3 proxy (QUIC with ALPN h3, :198) | caller-supplied |
| `--proxy-origins-file` | flag | tcp-tunnel/src/lib.rs:50-52 | Allowlist file of approved proxy origins | no |
| `--target` | flag | tcp-tunnel/src/lib.rs:53-55, :115-120 | CONNECT authority. Needs a nonzero port and no credentials | no |
| `--listen-addr` (default `127.0.0.1:0`) | flag | tcp-tunnel/src/lib.rs:56-57, :161-162, :208 | Must be loopback. Prints `LISTENING addr` | no |
| `--auth-token-stdin` / `--auth-token-updates-stdin` / `--connect-headers-stdin` | flag | tcp-tunnel/src/lib.rs:58-66; control.rs:14-52 | Bearer and extension headers fed over stdin (16 KiB, 16 headers max) | no |
| other port forwarding | crate behaviour | (whole tree) | No other port-forward or `forward_port` code. The only listeners are app-server ws/UDS, exec-server ws, and the tcp-tunnel loopback | no |

#### 8. Cloud tasks (`codex cloud`)

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `codex cloud` / alias `cloud-tasks` (TUI) | command | cli/src/main.rs:223-225; cloud-tasks/src/cli.rs:6-12 | "[EXPERIMENTAL]" TUI that browses Codex Cloud tasks | yes, `chatgpt.com` |
| `cloud exec [QUERY]` | command | cloud-tasks/src/cli.rs:17-18, :31-33 | Submits a cloud task without the TUI | yes, `chatgpt.com` `/wham/tasks` |
| `cloud exec --env ENV_ID` | flag | cloud-tasks/src/cli.rs:36-37 | Target cloud environment (required) | yes |
| `cloud exec --attempts N` | flag | cloud-tasks/src/cli.rs:40-45, :57-59 | Best-of-N attempts, 1-4 (default 1) | yes |
| `cloud exec --branch BRANCH` | flag | cloud-tasks/src/cli.rs:48-49 | Git branch to run (default: the current branch) | yes |
| `cloud status TASK_ID` | command | cloud-tasks/src/cli.rs:19-20, :76-78 | Shows task status | yes |
| `cloud list` | command | cloud-tasks/src/cli.rs:21-22 | Lists tasks | yes, `/wham/tasks/list` |
| `cloud list --env ENV_ID` | flag | cloud-tasks/src/cli.rs:84-85 | Filters by environment | yes |
| `cloud list --limit N` | flag | cloud-tasks/src/cli.rs:88-89, :68-70 | 1-20 (default 20) | yes |
| `cloud list --cursor CURSOR` | flag | cloud-tasks/src/cli.rs:92-93 | Pagination cursor | yes |
| `cloud list --json` | flag | cloud-tasks/src/cli.rs:96-97 | JSON output | yes |
| `cloud apply TASK_ID` | command | cloud-tasks/src/cli.rs:23-24, :102-104 | Applies a task's diff locally | yes |
| `cloud apply --attempt N` | flag | cloud-tasks/src/cli.rs:107-108 | Which attempt to apply (1-based) | yes |
| `cloud diff TASK_ID` | command | cloud-tasks/src/cli.rs:25-26, :113-115 | Prints a task's unified diff | yes |
| `cloud diff --attempt N` | flag | cloud-tasks/src/cli.rs:118-119 | Which attempt to show (1-based) | yes |
| `cloud -c key=value` | flag | cloud-tasks/src/cli.rs:9 | Config overrides | n/a |
| `CODEX_CLOUD_TASKS_BASE_URL` | env var | cloud-tasks/src/lib.rs:54-56; util.rs:47-75 | Backend URL, default `https://chatgpt.com/backend-api`. Saved credentials are sent only to `chatgpt.com`, `chat.openai.com` or `chatgpt-staging.com` on port 443 | yes |
| `CODEX_CLOUD_TASKS_MODE=mock` | env var | cloud-tasks/src/lib.rs:49-71 | Debug builds only: uses `cloud-tasks-mock-client` (environment discovery still goes to the network) | partly |
| `CODEX_CLOUD_TASKS_FORCE_INTERNAL` | env var | cloud-tasks/src/lib.rs:818-822 | Forces the internal (wham) path | yes |
| `CODEX_STARTING_DIFF` | env var | cloud-tasks-client/src/http.rs:347 | Starting diff attached to a new task | yes |
| `CODEX_TUI_ROUNDED` | env var | cloud-tasks/src/ui.rs:64 | UI style only | no |
| cloud auth | auth mode | cloud-tasks/src/util.rs:83-100 | AuthManager with `chatgpt_base_url`: vendor-account login plus account id | yes, vendor account |
| environment discovery | crate behaviour | cloud-tasks/src/env_detect.rs:50-92, :289 | `/wham/environments/by-repo/github/{owner}/{repo}` from the code host's origins, then `/wham/environments` | yes |
| backend-client paths | crate behaviour | backend-client/src/client.rs:145, :402-737 | `/wham/{accounts/check, profiles/me, tasks/list, tasks/{id}, tasks/{id}/turns/{t}/sibling_turns, tasks, config/bundle, settings/user, workspace-messages}` (or `/api/codex/...` for CodexApi hosts) | yes |
| `codex apply TASK_ID` | command | `chatgpt/src/apply_command.rs:16-22`; get_task.rs:38; cli/src/main.rs:1767 | Fetches `/wham/tasks/{id}` and applies the diff | yes, `chatgpt.com` |
| `chatgpt` client | crate behaviour | `chatgpt/src/chatgpt_client.rs:18`, :53-97 | GET against `chatgpt_base_url` with vendor-account tokens and `OAI-Product-Sku: codex` | yes |
| cloud-config bundle | crate behaviour | cloud-config/src/service.rs:35-57, :486-540; cli/src/cloud_config.rs:16-70 | `GET /wham/config/bundle` for business, education and enterprise vendor plans. Refreshed every 15 min | yes, `chatgpt.com` |
| cloud-config cache | file/socket path | cloud-config/src/cache.rs:24 | `cloud-config-bundle-cache.json`, signed and scoped to the user and account | no |
| `chatgpt_base_url` | config key | config/src/config_toml.rs:409; profile_toml.rs:44; core/src/config/mod.rs:4394; config_requirements.rs:167 | Default `https://chatgpt.com/backend-api/`. Drives cloud, apply, remote control and cloud-config. Can be pinned by requirements | yes |

#### 9. config.toml keys and env vars that affect the above (not already listed)

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| `[features] daemon_auto_start` | config key | features/src/lib.rs:935-939 | Stable, default TRUE: the TUI starts or joins the shared daemon | no (the updater: yes) |
| `[features] tui_app_server` | config key | features/src/lib.rs:1780-1784 | Removed (always on) | no |
| `[analytics] enabled` | config key | config/src/types.rs:225-228 | Turns analytics off | yes when on |
| `[cloud] skills.enabled` | config key | config/src/config_toml.rs:150-153, :421 | Cloud skills (the host must supply a provider) | yes |
| `openai_base_url` | config key | config/src/config_toml.rs:424 | Overrides the built-in `openai` provider base URL | yes by default |
| `CODEX_HOME` | env var | app-server-transport/src/transport/mod.rs:58-78 | Root of the control socket, the daemon state and the packages | no |
| `CODEX_CA_CERTIFICATE`, `SSL_CERT_FILE` | env var | http-client/src/custom_ca.rs:61-62 | Custom CA for http-client and websocket-client. Not used by the app-server `--remote` wss dial | no |
| `CODEX_APP_SERVER_LOGIN_ISSUER` | env var | app-server/src/request_processors/account_processor.rs:28 | Overrides the login issuer for app-server login | yes by default (`auth.openai.com`) |
| `CODEX_APP_SERVER_DEV_OPEN_APP_URL` | env var | app-server/src/request_processors/account_processor.rs:31 | Dev override for the open-app URL | n/a |

#### 10. Ragon patches (`codex-patches/`, read only)

| name | kind | file:line | what it does | vendor? |
|---|---|---|---|---|
| 0006-hide-cloud | patch | 0006-hide-cloud.patch, cli/src/main.rs hunk @@-221 (line 225) | `codex cloud` gets `hide = true`. Hidden from help but still runnable, so it still reaches `chatgpt.com` | closes nothing |
| 0007-hide-remote-control | patch | 0007-hide-remote-control.patch, cli/src/main.rs hunk @@-170 (line 173-174) | `codex remote-control` gets `#[clap(hide = true)]`. Still runnable, and `app-server --remote-control` and `daemon enable-remote-control` are untouched | closes nothing |
| 0013-disable-usage-analytics | patch | 0013-disable-usage-analytics.patch, analytics/src/client.rs hunk @@-247 (line 249) | `queue: (false && ...)`: the analytics queue is never built, so nothing is sent whatever `--analytics-default-enabled` or `[analytics]` say | closes analytics egress |
| 0015-no-vendor-network | patch | 0015-no-vendor-network.patch, core-plugins/src/manager.rs @@-732; remote_legacy.rs @@-127; otel/src/config.rs @@-19; tui/src/tooltips.rs @@-76/-214/-248 | Stops the curated-plugin sync, the featured-plugins fetch (`chatgpt.com`), vendor metrics (`ab.chatgpt.com`) and the tooltip/announcement fetch. No remote-session transport is touched | closes plugin, otel and announcement egress |
| 0016-doctor-offline | patch | 0016-doctor-offline.patch, cli/src/doctor.rs @@-1202 (const at line 1205), @@-2278 websocket_reachability, @@-2698 provider_reachability; doctor/updates.rs @@-53 | `doctor` skips auth, websocket reachability, provider reachability and update checks | closes doctor egress |
| 0019-airgapped | patch | 0019-airgapped.patch, app-server/src/extensions.rs @@-95 (line 96); cli/src/main.rs @@-2051; linux-sandbox/src/linux_run_main.rs @@-394; sandboxing/src/policy_transforms.rs @@-667; tui slash_command.rs and chatwidget hunks | Adds `/airgapped`. When sealed, commands get `NetworkSandboxPolicy::Restricted` and are denied host sockets (docker, containerd, podman, dbus, snapd, `$XDG_RUNTIME_DIR/{bus,systemd,docker,podman,containerd,ragon}`, ragon-rs/airgapped/src/lib.rs:953-972), and Full Access is refused. It acts on the tool sandbox, not on app-server, daemon or remote listeners | closes sandboxed-command egress only |
| 0023-airgapped-app-server | patch | 0023-airgapped-app-server.patch, core/Cargo.toml @@-33; core/src/config/mod.rs @@-4246 (line 4247) | Adds a validator on the permission profile in `Config` so app-server clients (including `--remote` and the daemon) cannot pick an unsandboxed or over-wide profile while airgapped | closes the airgap bypass through app-server config |
| 0001-brand-ragon-name | patch | 0001-brand-ragon-name.patch, line 15 (+) | Help text only: rewrites `codex --remote` and `codex agents` to `ragon ...` | no change |
| 0008-ragon-update | patch | 0008-ragon-update.patch, cli/src/main.rs @@-1626 | `codex update` runs `ragon_launcher::update::run()`. The daemon updater (update_loop.rs, `chatgpt.com/codex/install.sh`) is NOT patched: `grep -lE 'app-server-daemon\|update_loop\|install\.sh'` matches no patch | daemon updater egress still in source (gated, see section 3) |
| 0014-ragon-home | patch | 0014-ragon-home.patch, cli/src/main.rs @@-1019 | `use_ragon_home()` at startup: `~/.ragon` replaces `~/.codex`, so every `$CODEX_HOME` socket, daemon-state and package path in sections 1 and 3 moves under it (the `/tmp/codex-daemon-<uid>` directory does not) | no change to egress |
| patches matching remote\|listen\|websocket\|daemon\|exec-server\|cloud\|tunnel | patch | all 0001-0043 | Beyond the rows above, only 0030 (remote plugin catalog strings) matches. No patch touches app-server transports, websocket-auth, the daemon updater, remote-control endpoints, exec-server `--remote`/Noise, tcp-tunnel, `codex app` downloads or `codex apply` | open: see vendor column above |

### Appendix C. Vendor surface map: Codex CLI rust-v0.158.0 (064c6b8) under Ragon

**Source tree.** `codex/codex-rs`, which is the root of every Codex path in this appendix. Every such reference is a line that was opened at this tag.

**Ragon trees.**
- `ragon-rs/`, `codex-patches/` and `dreamference/` are relative to the Ragon repository root.
- Lines marked "(per Appendix B §N)" were carried over from `transports.md` and not re-opened here.

**Exceptions to "every line opened".**
- Rows marked "(per Appendix B §N)" were not re-opened.
- References relayed from DREAMFERENCE_AUTH.md ("AUTH §x l.n") cite the spec, not the code.
- The E1 URL forms come from test literals (protocol.rs:300-312).
- A few references name only a file or a patch hunk (`@@-n`), with no line.

**Abbreviations.**
- `GATE` is `https://<node>.local:8000` (DREAMFERENCE_AUTH.md §5.1, lines 129-150).
- `BASE` is `chatgpt_base_url`. The launcher writes `"{GATE}/backend-api/"` (ragon-rs/src/lib.rs:1629).
- `PROV` is the `ragon` model provider: `base_url={GATE}/v1`, `wire_api=responses` (lib.rs:1628, :1642-1643).
- `compat` is `dreamference/vllm_server/model_gate_compat.py`.
- `svc` is `dreamference/vllm_server/model_gate_service.py`.

**How the gate answers `/backend-api/<tail>`** (compat:352-408):
- **stub**: a tail ending in a `USAGE_TAILS` (:66), `EMPTY_LIST_TAILS` (:67-75), settings/user (:76), accounts/check (:77) or profiles/me (:78) entry.
- **404-shape**: a tail starting with a `SHAPE_404_TAILS` entry (:80-85, matched at :367): wham/tasks, wham/config, wham/threads, analytics-events.
- **unknown_url**: anything else gets the vendor-shaped 404 (:111-113, :369).

The stub test is a suffix test (`endswith`), so `ps/plugins/installed` hits the `plugins/installed` stub.

**The bridge.** `ragon web` and the desktop app's main process forward only what `allowedRequests` lists (ragon-rs/web/policy.json:2-3). The test at ragon-rs/web/src/policy.rs:820 asserts that `feedback/upload`, `account/login/start`, `remoteControl/enable` and `thread/realtime/start` are NOT allowed.

The bridge does not cover a client that talks to `ragon app-server` over stdio directly, such as an IDE extension or a script. Neither the launcher nor a patch refuses `app-server`.

#### A. Sign-in and tokens

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| A1 | `auth.openai.com/oauth/authorize` | browser OAuth (PKCE) | login/src/server.rs:76 (DEFAULT_ISSUER), :584-603 build_authorize_url | login server run (server.rs) | query: client_id, redirect_uri, code_challenge, state | `issuer()`, added by patch 0040 server.rs @@-74: `RAGON_ISSUER`, else the top-level `chatgpt_base_url` minus /backend-api, else DEFAULT_ISSUER. Upstream also has `--experimental_issuer` (cli main.rs:527-530, per AUTH §5.1 l.147) and `CODEX_APP_SERVER_LOGIN_ISSUER` | only on a Codex-side login | **Re-pointed** to GATE by 0040 and the launcher env (ragon-rs/src/auth/mod.rs:169-174). /oauth/authorize is not in ACCOUNT_PATHS (svc:118-124). AUTH §5.1 l.139-140 says the primary serves it, and the accounts service has it (dreamference/accounts/accounts_oauth.py:41, :560). Either way it is not a vendor host. Ragon's own `login` is a launcher subcommand (lib.rs:143-146). Not a vendor host. |
| A2 | `auth.openai.com/oauth/token` (code exchange) | exchange the code for tokens | login/src/server.rs:717, :1024 `{issuer}/oauth/token` | same file | form: grant_type=authorization_code, code, verifier → id/access/refresh tokens | issuer() | with A1 | **Re-pointed** to GATE. The gate forwards it to the accounts service (svc:119). |
| A3 | `auth.openai.com/api/accounts/deviceauth/usercode`, /deviceauth/token; verification page {issuer}/codex/device | device-code login | login/src/device_code_auth.rs:68, :107, :167-175, :187-189, :206 | same file | JSON {client_id} → {device_auth_id, user_code, interval}; poll → {authorization_code, code_verifier} | issuer() | only on a Codex-side `--device-auth` login or `account/login/start` | **Re-pointed** to GATE (0040). deviceauth is not in ACCOUNT_PATHS (svc:118-124). AUTH §5.1 l.139-140 says the primary serves it (dreamference/accounts/accounts_oauth.py:137-138). Either way it is not a vendor host. The bridge refuses `account/login/start` (policy.rs:820). Not a vendor host. |
| A4 | `auth.openai.com/oauth/token` (refresh) | token refresh | login/src/auth/manager.rs:212 (REFRESH const, removed by 0040 @@-209), override env at :214 | manager.rs, 0040 @@-1725 `format!("{}/oauth/token", issuer())` | JSON {client_id, grant_type=refresh_token, refresh_token} | `CODEX_REFRESH_TOKEN_URL_OVERRIDE` | on with vendor-account auth | **Re-pointed** by 0040 and the launcher (`{GATE}/oauth/token`, auth/mod.rs:173). Forwarded (svc:119). |
| A5 | `auth.openai.com/oauth/revoke` | logout revoke | manager.rs:213, override env at :215 | login/src/auth/revoke.rs (0040 @@-142) | JSON {token, token_type_hint} | `CODEX_REVOKE_TOKEN_URL_OVERRIDE` | on logout | **Re-pointed** (auth/mod.rs:174). Forwarded (svc:120). |
| A6 | `auth.openai.com/api/accounts/v1/user-auth-credential/whoami` | classify a personal access token | login/src/auth/personal_access_token.rs:11-13, :39-51 | manager.rs:505, :1058, :1541 (reads `CODEX_ACCESS_TOKEN`, :953-955, :969) | GET with Bearer PAT → identity JSON | `CODEX_AUTHAPI_BASE_URL` (personal_access_token.rs:12) | **off**: only when the user exports a PAT in `CODEX_ACCESS_TOKEN` | **OPEN (opt-in by env).** The launcher neither overrides nor clears `CODEX_ACCESS_TOKEN` or `CODEX_AUTHAPI_BASE_URL`; grep of ragon-rs/src found neither. |
| A7 | `auth.openai.com/oauth/token`, `auth.api.openai.org/oauth/token` (workload identity) | federated token exchange | login/src/auth/workload_identity.rs:32-33 | :127-129 (selected by `OPENAI_FEDERATION_RULE_ID` or the identity-token file), :131-168 | token-exchange form | env markers | off (env opt-in) | **Closed by BASE.** classify_auth_environment (:216-240) accepts only `chatgpt.com`, `chat.openai.com` or `chatgpt-staging.com` bases, so the gate base errors before any call. |
| A8 | `auth.openai.com/api/accounts/v1/agent/register` and …/task/register; `chatgpt.com/backend-api/wham/agent-identities/jwks` | agent-identity registration | agent-identity/src/lib.rs:41-44, :58-79; login/src/auth/agent_identity.rs:10-11 | agent_identity.rs:24-57 `ChatGptEnvironment::from_chatgpt_base_url` | signed registration JSON | `CODEX_AGENT_IDENTITY_AUTHAPI_BASE_URL`, `_JWKS_BASE_URL` | off (agent-identity auth only) | **Closed by BASE.** Fails with "Agent Identity only supports production and staging vendor environments" for the gate base. |
| A9 | `chatgpt.com/codex/open-app`, `platform.openai.com` | links on the login success page | login/src/success_page.rs:7, :83 | n/a | HTML link | const | n/a | Link only, no call. |

#### B. Model API (provider base)

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| B1 | {PROV}/responses (built-in default `chatgpt.com/backend-api/codex` or `api.openai.com/v1`) | turns, SSE | codex-api/src/endpoint/responses.rs:139; defaults at model-provider-info/src/lib.rs:77, :415-428 | ModelClient (core/src/client.rs) | Responses API request → SSE events | `model_provider` and `model_providers.<id>.base_url` | on | **Served by the gate** at `/v1/responses` (PROV, lib.rs:1642-1643). |
| B2 | {PROV}/responses over WS | websocket turns | codex-api/src/endpoint/responses_websocket.rs:400, :434 | same | `response.create` frames | PROV | per provider | Gate (PROV); AUTH §5.9 l.296. |
| B3 | {PROV}/models | model list and ETag | codex-api/src/endpoint/models.rs:33-34 | models manager (AUTH §5.9 l.273) | GET → {models:[…]} | PROV | on | **Served by the gate** at `/v1/models`. |
| B4 | remote compaction | compact | model-provider/src/provider.rs:411-417 (remote compaction only for `is_openai()` or one other cloud) | n/a | n/a | n/a | n/a | **Not called.** The `ragon` provider reports `Unsupported`, so compaction is local. No `responses/compact` path exists in codex-api/src at this tag (grep). |
| B5 | {PROV}/memories/trace_summarize | memory summaries | codex-api/src/endpoint/memories.rs:32-33 | core/src/client.rs:733-737 | JSON raw memories → summaries | PROV | memories feature | Goes to the gate (PROV), not the vendor. |
| B6 | {PROV}/alpha/search | standalone web search | codex-api/src/endpoint/search.rs:15 | SearchClient (same file) | SearchRequest → SearchResponse | PROV | per feature | Goes to the gate (PROV), not the vendor. |
| B7 | {PROV}/realtime/calls | WebRTC call creation (SDP) | codex-api/src/endpoint/realtime_call.rs:62-63 | core/src/realtime_conversation.rs:700-708 | SDP + session config → {call_id, sdp, sideband headers} | PROV, or `experimental_realtime_webrtc_call_base_url` (realtime_conversation.rs:1354-1358) | realtime feature | Goes to the gate. A grep of svc found no realtime route, so the WebRTC path should stop here and not reach B9 from this side. |
| B8 | {PROV}/realtime (WS transport) | realtime voice over WS | codex-api/src/endpoint/realtime_websocket/methods.rs | realtime_conversation.rs:691-694 | realtime events | PROV, or `experimental_realtime_ws_base_url` (realtime_conversation.rs:1341-1352) | realtime feature | Goes to the gate (PROV), not the vendor. |
| B9 | **`wss://api.openai.com/v1/realtime`?call_id=…** (sideband) | attach to an existing realtime call | codex-api/src/endpoint/realtime_websocket/methods.rs:62 `OPENAI_REALTIME_API_BASE_URL`, default at :849, override :854-856, URL built at :993-1005 | core/src/realtime_conversation.rs:726 → realtime_conversation/existing_call.rs:56-64 | WS. Headers from core/src/client.rs:715-728: the **configured provider's auth**, i.e. the Ragon key or gate token, plus an attestation header | per-request `sideband_base_url`, else `experimental_realtime_ws_base_url` (realtime_conversation.rs:1341-1350); unset means `api.openai.com` | feature `realtime_conversation` Stable, default_enabled true (features/src/lib.rs:1762-1765) | **OPEN, LOUD.** Reachable through the app-server RPC `thread/realtime/start` with an `ExistingCall` transport. The client supplies `call_id`; the dial needs no prior gate call and carries the Ragon credential to the vendor. Patch 0010 hides only the TUI /voice. The bridge refuses the RPC (policy.rs:820), but a direct `ragon app-server` client still reaches it. |
| B10 | embeddings | n/a | n/a | n/a | n/a | n/a | n/a | No embeddings endpoint found in non-test .rs at this tag (grep for `embeddings`). |
| B11 | `api.openai.com/v1` or `chatgpt.com/backend-api/codex` (built-in `openai` provider) | the vendor model API | model-provider-info/src/lib.rs:77, :415-428 | B1 | B1 | `-c model_provider="openai"` | **off** in ragon: the launcher injects `model_provider="ragon"` (ragon-rs/src/lib.rs:1754-1762) | **OPEN (opt-in by flag).** A user's own `-c model_provider=…` is left alone (lib.rs:1738-1740; test :1974). The launcher sets neither `OPENAI_BASE_URL` nor `openai_base_url` (config/src/config_toml.rs:424), so the built-in provider keeps its vendor default. |
| B12 | `api.openai.com/v1/responses` (`responses-api-proxy` default upstream) | local forwarding proxy | responses-api-proxy/src/lib.rs:52-53 (`--upstream-url` default) | cli/src/main.rs:227-229 (hidden subcommand), :1778 | reads an API key on stdin, forwards /v1/responses | `--upstream-url` | off (the user runs it) | **OPEN (opt-in by command).** Not in REMOVED_COMMANDS (ragon-rs/src/lib.rs:158-163). |
| B13 | {PROV}/images/generations, {PROV}/images/edits | image generation tool | codex-api/src/endpoint/images.rs:19 (ImagesClient), :68, :81 | ext/image-generation/src/backend.rs:79-96 (`provider.api_provider()` and `api_auth()`) | JSON image request → ImageResponse | PROV | condition not checked | Goes to the gate (PROV), not the vendor. |

#### C. backend-api: accounts, usage, settings, files (BASE → gate)

PathStyle: when BASE contains /backend-api, paths are `/wham/...`; otherwise `/api/codex/...` (backend-client/src/client.rs:149-155). All C rows go to GATE `/backend-api/...`; none can reach the vendor while BASE is the gate.

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| C1 | {BASE}/wham/accounts/check | account check | client.rs:402-403 | BackendClient | GET → accounts JSON | BASE | vendor-account auth | **Gate stub** (compat:77). |
| C2 | {BASE}/wham/profiles/me | profile | client.rs:419-420 | BackendClient | GET → profile | BASE | vendor-account auth | **Gate stub** (compat:78). |
| C3 | {BASE}/wham/settings/user | user settings | client.rs:736-737 | BackendClient | GET → settings | BASE | vendor-account auth | **Gate stub** (compat:76). |
| C4 | {BASE}/wham/workspace-messages | banners and messages | client.rs:729-730 | BackendClient | GET → list | BASE | vendor-account auth | **Gate stub**, empty list (compat:67-75). |
| C5 | {BASE}/wham/accounts/send_add_credits_nudge_email | billing nudge | client.rs:714-720 | BackendClient | POST | BASE | user action | Gate unknown_url 404. |
| C6 | {BASE}/wham/usage | rate limits and usage | backend-client/src/client/rate_limit_resets.rs:126-127 | /status, rateLimits (AUTH §5.8 l.267) | GET → RateLimitStatus | BASE | vendor-account auth | **Gate stub** (compat:66). |
| C7 | {BASE}/wham/usage/rate-limit-reset-credits (+ consume) | reset credits | rate_limit_resets.rs:134-137, :151 | same | GET/POST | BASE | user action | Gate unknown_url 404 (the suffix does not match `wham/usage`). |
| C8 | {BASE}/wham/usage/thread_usage/query, …/query_v2, …/plan_limit_history, …/thread-estimates/query, analytics `{prefix}/{route}` | usage analytics | thread_usage.rs:115-118, task_usage.rs:99, plan_history.rs:68, `chatgpt_turn_cost.rs:52-55`, analytics.rs:56 | BackendClient | POST query JSON | BASE | on in the usage views | Gate unknown_url 404. |
| C9 | {BASE}/files, {BASE}/files/{id}/uploaded | file upload for MCP/apps | codex-api/src/files.rs:127, :149, :354-355 | `core/src/mcp_openai_file.rs:247-248` (`turn_context.config.chatgpt_base_url`) | POST {file_name, file_size, use_case} → upload URL | BASE | when a tool asks for a vendor file | Gate unknown_url 404. |
| C10 | {BASE}/accounts/verified_access | cyber-policy notice prefetch | tui/src/daybreak.rs:78-81 | daybreak.rs:82-86 with the auth headers; prefetched from tui/src/app/app_server_events.rs:331, tui/src/app/reconnect.rs:481 | GET → notice | BASE | condition not checked | Gate unknown_url 404. |
| C11 | workspace routing: the provider origin is rewritten to `workspace_backend_origin` | account-owned routing | model-provider/src/workspace_routing.rs:39-75 (https only, path "/"); origin chosen at app-server/src/request_processors/account_processor/workspace_routing.rs:427-468; field backend-client/src/types.rs:142 | app-server's WorkspaceRoutingResolver (same file :107) reads C1 | `workspace_backend_origin` and `account_routing_override` from the C1 response | C1's answer | vendor-account auth | **Closed by the gate.** The C1 stub returns `"NO_CONSTRAINT"` for both fields (compat:380-397), so the origin stays the configured base (workspace_routing.rs:444-461). It is no new host. A vendor-served C1 could point the origin anywhere https. |

#### D. Cloud tasks, apply, cloud-config, exec-server registry

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| D1 | {BASE}/wham/tasks/list, /wham/tasks/{id}, …/sibling_turns, POST /wham/tasks | Codex Cloud tasks | client.rs:459-460, :495-496, :510-515, :582-583; fallback base cloud-tasks/src/lib.rs:55 | `cloud` TUI (cloud-tasks) | task list/detail/diff JSON | BASE | on `cloud` | **Refused by the launcher**: `cloud`, `cloud-tasks` (ragon-rs/src/lib.rs:158-165). Patch 0006 hides it. The gate answers 404-shape (compat:80-85). |
| D2 | {BASE}/wham/tasks/{id} (`apply`) | apply a cloud task diff | `chatgpt/src/get_task.rs:37-39` `/wham/tasks/{task_id}` | `chatgpt/src/apply_command.rs:34` | task detail JSON | BASE | on `apply` | `apply` is NOT refused (REMOVED_COMMANDS is only cloud). The gate answers 404-shape (wham/tasks). Closed by BASE. |
| D3 | {BASE}/wham/environments, {BASE}/wham/environments/by-repo/{provider}/{owner}/{repo} | pick a cloud environment | cloud-tasks/src/env_detect.rs:57, :90, :330, :373 | cloud-tasks/src/lib.rs:2034 (`cloud` TUI) | GET → environment rows | BASE | on `cloud` | Only reachable through `cloud`, which is refused. The gate would answer unknown_url: `wham/environments` is not a 404-shape tail. |
| D4 | {BASE}/wham/config/bundle | enterprise cloud-config bundle | client.rs:540-541; cloud-config/src/bundle_loader.rs:27, :34, :106-107 | cloud-config/src/service.rs:50-58 (needs `uses_codex_backend` and a business, edu or enterprise plan) | GET → TOML bundle | BASE | plan-gated | Gate 404-shape (wham/config). Closed by BASE. |
| D5 | {remote}/cloud/environment/{id}/register, /connect, /validate + Noise relay | exec-server environment registry | exec-server/src/remote.rs:131-155, :237, :341; relay remote.rs:59, :711-745 (per Appendix B §6) | cli/src/exec_server_command.rs:70-78 `--remote URL`; auth :345-424, `--use-agent-identity-auth` :99-106 (per Appendix B §6) | JSON {executor_public_key} → registration | `--remote` (no default) | off | **OPEN (opt-in by flag)**: only if the user passes `--remote` at a vendor registry. There is no built-in vendor default. |
| D6 | {BASE}/wham/threads* | none | no caller in non-test .rs at this tag (grep `wham/threads`, `/threads"`) | n/a | n/a | n/a | n/a | The gate's 404-shape covers a tail Codex does not call here. |

#### E. Remote control relay

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| E1 | wss {BASE}/wham/remote/control/server, + /enroll, /refresh, /pair, /pair/status | phone/web remote control of the app server | URL forms at app-server-transport/src/transport/remote_control/protocol.rs:300-312 (test literals); host check :193-211 | remote_control_url = `config.chatgpt_base_url` (app-server/src/lib.rs:850); CLI remote_control_cmd.rs:53-130 (per Appendix B §4) | enroll JSON, WS frames | BASE | daemon `remote_control_enabled: false` (app-server-daemon/src/settings.rs:30). The loop starts and polls auth (notes.md, OBSERVED) | **Closed by BASE.** `is_allowed_remote_control_chatgpt_host` accepts only `chatgpt.com`, `chatgpt-staging.com` or loopback, so `<node>.local` fails validation. On a loopback gate it dials /backend-api/wham/remote/control/server and gets unknown_url 404. Patch 0007 hides the command; the bridge refuses `remoteControl/enable`. |

#### F. Plugins, connectors, apps MCP, skills, MCP directories

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| F1 | {BASE}/ps/plugins/suggested/codex?scope=GLOBAL | recommended plugins | core-plugins/src/remote.rs:1032-1039 | remote catalogue; `ensure_chatgpt_auth` at :2347-2355 | GET → plugin list | BASE | feature remote_plugin default true (features/src/lib.rs:1517-1519) | **-c `features.remote_plugin=false`** (ragon-rs/src/plugins.rs:35, :42-47) plus config (:104-108). The gate answers unknown_url. |
| F2 | {BASE}/ps/plugins/installed | installed remote plugins | remote.rs:2257-2258 | same | GET → list | BASE | remote_plugin | **Gate stub**, empty list (suffix `plugins/installed`, compat:67-75). Also -c off. |
| F3 | {BASE}/ps/plugins/search | catalogue search | core-plugins/src/remote/search.rs:42-44 | same | GET → list | BASE | remote_plugin | -c off. The gate answers unknown_url. |
| F4 | {BASE}/plugins/featured?platform= | featured list | core-plugins/src/remote_legacy.rs:123-130 | plugins manager | GET → ids | BASE | plugins default true (features lib.rs:1439-1441) | **Patch 0015** (remote_legacy.rs @@-127 returns an empty list). Also a gate stub. |
| F5 | `api.github.com`, `github.com/openai/plugins.git`, `chatgpt.com/backend-api/plugins/export/curated` | curated catalogue startup sync | core-plugins/src/startup_sync.rs:23, :27, :30 | core-plugins manager.rs | git clone, zip | consts | on | **Patch 0015** (manager.rs @@-732 `if false &&`). |
| F6 | sharing a plugin through the vendor's accounts; link `chatgpt.com/codex/plugins/share/{name}` | plugin sharing | features lib.rs:1523-1525; link at tui/src/app/plugin_mentions.rs:158 | n/a | n/a | BASE | default true | **-c `features.plugin_sharing=false`** (plugins.rs:35). |
| F7 | {BASE}/connectors/directory/list?token=…&external_logos=true | app (connector) directory | connectors/src/lib.rs:267-273 | core/src/connectors.rs:141 (gated on Feature::Apps, default true, features lib.rs:1349-1351) | GET → connectors page | BASE | on | **Gate stub**, empty list. Patch 0022 replaces the app list with ragon_launcher::apps::list() (apps_processor.rs @@-72, tui connectors.rs @@-150). |
| F8 | {BASE}/connectors/directory/list_workspace?external_logos=true | workspace connectors | connectors/src/lib.rs:190-191 | same | GET | BASE | on (Apps) | Gate unknown_url 404 (no stub). 0022 as F7. |
| F9 | {BASE}/ps/apps/batch | app metadata batch | `chatgpt/src/connectors.rs:172-175` | same | POST ids → apps | BASE | Apps | Gate unknown_url 404. 0022. |
| F10 | {BASE}/ps/mcp (`/api/codex` added if BASE has neither form) | the "codex_apps" MCP server | codex-mcp/src/mcp/mod.rs:600-607, config :610-620 (`McpServerAuth::ChatGpt`, header `X-OpenAI-Product-Sku`) | core/src/mcp.rs:350-358 when `mcp_config.apps_enabled` | streamable-HTTP MCP | BASE | `enable_mcp_apps` default false (features lib.rs:1361-1363); the launcher also writes false if absent (lib.rs:1656-1657) | Off. If enabled, the gate answers unknown_url 404. |
| F11 | `chatgpt.com/oauth/codex/client.json`, …/{callback_id}/client.json | MCP OAuth CIMD client id | rmcp-client/src/oauth_client_registration.rs:116-124 | same; only when CIMD registration is offered (:100-108) | URL sent as client_id to a third-party MCP authorization server | const | only for OAuth MCP servers that use CIMD | **Not a Codex call.** The third-party server may fetch `chatgpt.com`, which identifies the client as Codex. Indirect; noted, not closed. |
| F12 | `developers.openai.com/mcp` | docs-MCP attribution tag | `codex-mcp/src/openai_docs_source_attribution.rs:10-11` | n/a | string tag | const | n/a | Link or tag only, no call. |
| F13 | skill catalogue, MCP directory | n/a | n/a | n/a | n/a | n/a | n/a | **Searched, absent.** No remote skill catalogue or MCP directory URL exists beyond F7/F8. The search covered `format!`/`const` URL literals with `skills` or `directory`, every `https://` constant (74 hits), and the readers of `chatgpt_base_url`. That sweep added C10, C11 and F14 and re-cited D2. A final deduplicated list of vendor-host literals in non-test .rs (section I) found no further request target. |
| F14 | {BASE}/public/plugins/workspace/{id}, {BASE}/ps/plugins/{id}/shares, {BASE}/ps/plugins/workspace/created, {BASE}/public/plugins/workspace/upload-url | plugin sharing (F6) | core-plugins/src/remote/share.rs:287, :324, :392, :416 | plugin share flow | GET/POST share JSON, upload URL | BASE | plugin_sharing default true | **-c `features.plugin_sharing=false`** (plugins.rs:35). If enabled, the gate answers unknown_url 404. |

#### G. Updates, desktop app, pets, doctor

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| G1 | `api.github.com/repos/openai/codex/releases/latest`; a package manager; `registry.npmjs.org/@openai%2fcodex` | TUI update check | tui/src/updates.rs:61-62, :102-103; tui/src/npm_registry.rs:5, :79 | updates.rs:28, :153 (gate `check_for_update_on_startup`) | GET → latest version | `check_for_update_on_startup` | default true | **Closed**: -c `check_for_update_on_startup=false` (plugins.rs:43) and config.toml (lib.rs:1621). |
| G2 | `codex update` | self-update | n/a | cli main.rs (patch 0008 @@-1626) | n/a | n/a | on demand | **Patch 0008** routes it to `ragon_launcher::update::run` (Ragon's own releases via `api.github.com`, ragon-rs/src/update.rs:216; not the vendor). |
| G3 | **`https://chatgpt.com/codex/install.sh`** (Windows: install.ps1) | daemon auto-updater: fetch the vendor installer and pipe it to `/bin/sh -s` (update_loop.rs:380, :532-545; read, not run) | app-server-daemon/src/update_loop.rs:66, :68; INITIAL_UPDATE_DELAY 5 min :63 | update_loop.rs:651 `http.get(INSTALL_URL)` | GET → shell script | const (no key) | `auto_update_enabled: true` (app-server-daemon/src/settings.rs:32), hourly (:14). A scheduled run fetches only when `is_stable_standalone_release` holds (update_loop.rs:477-488; app-server-daemon/src/managed_install.rs:74-86): `{CODEX_HOME}/packages/app-server-daemon/current` must point into `releases/`. Bootstrap builds that only from a complete Codex package layout (prepare_install.rs:28-41, :145-147) | **OPEN, LOUD, conditional.** No patch touches it. Whether a ragon install has a Codex package layout was not checked; without one, bootstrap fails ("this CLI has no complete local package") and the loop never fetches. `ragon app-server daemon start/bootstrap` is not refused (REMOVED_COMMANDS lib.rs:158-163). The TUI auto-start is skipped under the launcher's -c and --oss (tui/src/daemon_startup.rs:25-50; notes.md). |
| G4 | `persistent.oaistatic.com/codex-app-prod/Codex.dmg`, …-x64.dmg | desktop app download | cli/src/desktop_app/mac.rs:10-12, download :235-240 (curl) | cli main.rs:177, :1429 Subcommand::App | DMG | const | on `codex app` | **Closed by the launcher**: `app` is intercepted (ragon-rs/src/lib.rs:409-413) and never reaches Codex. |
| G5 | vendor appcast(-x64).xml, windows-store-update.json, `chatgpt.com/backend-api/wham/app/appcast`, `api.github.com` releases, `formulae.brew.sh` | `doctor` update row | cli/src/doctor/updates.rs:36-46 | updates_check (:53+) | GET → feeds | consts | on `doctor` | **Patch 0016** (updates.rs @@-53 RAGON_DOCTOR_OFFLINE). |
| G6 | `chatgpt.com/backend-api/`, `api.openai.com/v1` reachability | `doctor` network rows | cli/src/doctor/network.rs:30; doctor.rs:2582, :2632 | doctor | HEAD/GET | consts and config | on `doctor` | **Patch 0016** (doctor.rs @@-1202, @@-2278, @@-2698). |
| G7 | `persistent.oaistatic.com/codex/pets/v1/`… | terminal pet art | tui/src/pets/asset_pack.rs:29, :100 | pets | GET webp | const | if a pet is chosen | **Closed**: -c `tui.pet="disabled"` (plugins.rs:43) and patch 0010 (hides /pets). |

#### H. Feedback, analytics, telemetry, announcements

| # | host/path | purpose | defined at | called at | shape | address key | default on? | Ragon today |
|---|---|---|---|---|---|---|---|---|
| H1 | **`o33249.ingest.us.sentry.io`** (`SENTRY_DSN`) | feedback and bug upload | feedback/src/lib.rs:57-58, upload_feedback :593-601 | app-server/src/request_processors/feedback_processor.rs:49-55 (RPC `feedback/upload`), :276 `snapshot.upload_feedback(...)` | error-report envelope from FeedbackUploadParams and the session snapshot | `feedback.enabled`, default **true** (core/src/config/mod.rs:4454-4458); the guard is feedback_processor.rs:62-66 | on | **OPEN, LOUD.** Patch 0009 hides only the TUI /feedback. The launcher sets no `feedback.enabled` (grep of ragon-rs/src). The bridge refuses `feedback/upload` (policy.rs:820), but a direct `ragon app-server` client still uploads to the vendor's error-report service. |
| H2 | {BASE}/codex/analytics-events/events | usage analytics | analytics/src/client.rs:140-142 | queue built at client.rs:241-251 | batched events JSON | BASE, `analytics.enabled` | on | **Patch 0013** (`queue: false && …`). The gate would answer unknown_url anyway: the tail starts with `codex/`, so it misses the `analytics-events` 404 shape. |
| H3 | `ab.chatgpt.com/otlp/v1/metrics` (vendor metrics OTLP, `statsig-api-key`) | metrics | otel/src/config.rs:9-11, resolve_exporter :13-30 | otel exporter | OTLP-HTTP JSON | `otel.exporter` (vendor default) | on in release builds | **Patch 0015** (otel/src/config.rs @@-19 `if true` → None in every build). |
| H4 | `raw.githubusercontent.com/openai/codex/main/announcement_tip.toml` | announcement tip | tui/src/tooltips.rs:19 | tooltips | GET TOML | const | on | **Patch 0015** (tooltips.rs @@-76, @@-214, @@-248). |

#### I. Links only (no request is made)

- Help and docs: `help.openai.com` (tui feedback_note_view.rs:39, plugin_catalog.rs:66, safety_buffering.rs:9, notices.rs:152).
- `developers.openai.com` (session/mod.rs:504, sandboxing bwrap.rs:20, memories_settings_view.rs:36).
- Usage and billing pages: `chatgpt.com/codex/settings/usage` and billing pages (tui backend_banners/actions.rs:7-57, status/card.rs:57).
- Code host: `github.com/openai/codex` issues and releases (feedback_view.rs:18, notices.rs:42-84, update_prompt.rs:42).
- `chatgpt.com/cyber` (core/src/session/mod.rs:503).
- `help.rs:76-80` in ragon-rs drops the `openai.com` lines from help.
- These are URLs printed to the user. They become a contact only if the user opens them.
- **Final sweep.** Every distinct vendor-host literal in non-test .rs was listed and deduplicated by host and first two path segments. The ones not in a row above are:
  - JWT claim and issuer names, never fetched: `https://api.openai.com/auth` (login/src/token_data.rs:77) and `https://chatgpt.com/codex-backend/agent-identity` (agent-identity/src/lib.rs:41).
  - Inline test literals: app_link_view.rs:936 google-calendar, enroll.rs:525 remote/control, realtime_call.rs:579 and methods.rs:2335 `v1/live`, `chatgpt_cloudflare_cookies.rs:228-253`, cli main.rs:2808-2830 `service.openai.com`, onboarding/auth.rs:1420 `chatgpt.com/device`.
  - one other cloud, staging and example hosts.
  - Docs links: platform, help, developers and `learn.chatgpt.com`.

#### Still open in shipped `ragon`

**1. Default-on once a reachable command runs, if the install has a Codex package layout**
- **G3: daemon updater, GET `https://chatgpt.com/codex/install.sh`.** It fetches the installer and pipes it to `/bin/sh -s` (update_loop.rs:380, :532-545; read, not run). A scheduled fetch needs a stable standalone release under `{CODEX_HOME}/packages/app-server-daemon` (managed_install.rs:74-86), which bootstrap builds only from a Codex package layout (prepare_install.rs:145-147). Whether a ragon install has one was not checked. It is started by `ragon app-server daemon start|bootstrap`, which is not refused. auto_update_enabled defaults to true. No patch touches it.

**2. Needs a client speaking to `ragon app-server` directly** (the bridge refuses these for `ragon web` and the desktop app):
- **H1: Feedback upload via `feedback/upload`.** `feedback.enabled` defaults to true and the launcher does not set it.
- **B9: realtime sideband, `wss://api.openai.com/v1/realtime`, via `thread/realtime/start` with an ExistingCall transport.** It sends the Ragon credential to the vendor. `realtime_conversation` is default on.

**3. Opt-in by env, flag or config**
- A6: PAT whoami at `auth.openai.com` when the user exports `CODEX_ACCESS_TOKEN`.
- B11: `-c model_provider="openai"`, which goes to `api.openai.com/v1` or `chatgpt.com/backend-api/codex`.
- B12: `ragon responses-api-proxy`, whose default upstream is `api.openai.com`.
- D5: `exec-server --remote <OpenAI registry>`.
- B8/B9: `experimental_realtime_*_base_url` pointing at the vendor.
- Indirect only: F11, the CIMD client id URL at `chatgpt.com`. A third-party server fetches it; Codex does not.

**Cheap closures** (suggestions from the survey, superseded: the owner's Q5 re-points every one of these at the primary, §3.3 and §7.6):
- `-c feedback.enabled=false` in `vendor_settings()`.
- `-c features.realtime_conversation=false`.
- Refuse `app-server daemon`, or patch update_loop.rs:651.
- Clear `CODEX_ACCESS_TOKEN` and `CODEX_AUTHAPI_BASE_URL` in `codex_env`.

#### Already served by the gate

- **Stubs**: wham/usage (C6), wham/accounts/check (C1), wham/profiles/me (C2), wham/settings/user (C3), wham/workspace-messages (C4), connectors/directory/list (F7), plugins/featured (F4), ps/plugins/installed (F2, suffix match).
- **404-shape**: wham/tasks* (D1, D2), wham/config/bundle (D4); wham/threads has no caller (D6).
- **unknown_url 404**:
  - C5, C7, C8, C9 (files);
  - C10 (verified_access), D3 (environments, behind the refused `cloud`);
  - F1, F3, F8, F9, F10 (ps/mcp), F14 (share paths);
  - H2 (analytics);
  - E1 on a loopback gate.
- **Model API served**: /v1/responses (B1, B2), /v1/models (B3). B5, B6, B7 and B13 go to the gate too; grep found no realtime route in svc.
- **Routing**: C11 is pinned to the configured base by the C1 stub's `NO_CONSTRAINT`.
- **Forwarded to the accounts service**: /oauth/token (A2, A4), /oauth/revoke (A5), /oauth/userinfo, jwks, openid-configuration, /ragon/v1/* (svc:118-137).
- **Re-pointed, not in the gate's ACCOUNT_PATHS**: /oauth/authorize (A1), /api/accounts/deviceauth/* (A3). AUTH §5.1 l.139-140 says the primary serves them (dreamference/accounts/accounts_oauth.py:41, :137-138). Not a vendor host either way.

#### Counts

- 65 rows in total: 9 A, 13 B, 11 C, 6 D, 1 E, 14 F, 7 G, 4 H.
- 8 rows are OPEN or indirect (open-list items 1-3): 1 default-on once a command runs (G3, conditional on a package layout), 2 direct app-server only, 4 opt-in, 1 indirect (F11). B8 is counted only through its opt-in config.
- 25 rows are answered by the gate: 8 stub, 3 404-shape, 14 unknown_url (counting E1 on a loopback gate, C10, D3 behind `cloud`, and F14 if enabled). Six more rows (A2, A4, A5, B1, B2, B3) are served or forwarded by it. B5-B8 and B13 go to it, and C11 is pinned by it.
- The rest are closed by a patch, `-c`/config, the launcher, a base-URL check, or are links or not called.
