# Ragon node authentication: Codex's login, re-enabled against the node

**Status:** decided by the owner on 2026-11-19 (§1); spec only, nothing built. The implementation plan has ten rows, all held until the coordinator releases them one at a time (§13).
**Target:** every Ragon client (`ragon`, the desktop app, `ragon web`) and every node that serves the model (the GB10 node first; the Windows and Mac nodes when their gates exist).
**Builds on:**
- the launcher in `ragon-rs/`, its node resolver tiers and `~/.ragon` home ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md), [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md) §6);
- the model gate in front of the engine (`dreamference/vllm_server/model_gate_service.py`, [INFERENCE](./DREAMFERENCE_INFERENCE.md));
- the Codex patch series and its size cap ([docs/dev/codex-build.md](../docs/dev/codex-build.md));
- `ragon web`'s own credentials, which stay as they are ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3);
- `/airgapped` and the egress audit ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md), [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md)).

**Decisions made here, stated first because each could be read the other way:**

1. **The node is the issuer.** Codex's own sign-in code runs unchanged in shape: the browser flow with PKCE and the one-time-code (device) flow. Only the issuer differs: it is the node, not the vendor's service. The node serves the issuer routes on the model port, behind the same gate as the model (§5).
2. **The primary node is the node the client resolves.** There is no new role. It is the node the resolver tiers end on ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md) §6.1, the last tier being the mDNS browse). Each node keeps its own accounts; a pair of nodes shares nothing in 1.x (§4).
3. **The node's page asks for the username and password.** It does not happen in the terminal. Codex opens the vendor's sign-in page in a browser; Ragon opens the node's page the same way. That page has two forms: "Sign in to <node name>" and "Create an account on <node name>". Without a usable browser (an SSH session, `ragon web` on a phone), Codex's one-time-code screen is used instead (§6).
4. **The empty password is accepted.** It is hashed like any other (§7.2).
5. **One credential per sign-in: the node key.** The node key is a random bearer token. It is sent as `Authorization: Bearer` on every model request and it works as an API key for any client that speaks the model API. It does not expire in 1.x. It ends at `ragon logout`, `ragon-admin node user reset` or `ragon-admin node user remove` (§5.4).
6. **The client stores the sign-in in Codex's API-key mode, not in its account mode.** This is the one place Ragon departs from what Codex does with the vendor. In Codex, account mode turns on calls to the vendor's backend from about sixty places outside the login crate: plugins, connectors, rate limits, cloud tasks, the model list and analytics. Examples are `core-plugins/src/remote.rs`, `chatgpt/src/connectors.rs`, `app-server/src/request_processors/account_processor/rate_limit_resets.rs` and `models-manager/src/manager.rs:562`. In API-key mode the key goes to the configured provider and nowhere else (§9.1).
7. **Nothing more than identifying the user** (the owner's scope, §1). There are no roles, lockout, rate limits, TLS or token binding, and no password policy beyond "may be empty". These are listed for 3.0 in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) §"Release 3.0: authentication hardening".
8. **The check is switched by a setting that ships off and is turned on by the last row.** Each row lands with `[node] model_auth = "off"`. Row 10 turns the default to `"on"`, after unattended callers and the egress audit carry keys (§13). Nights and benchmarks never meet a half-built check.

---

## 1. The owner's words and the scope

The owner, 2026-11-19, verbatim:

```text
we need to reenable authentication that codex had. Everything needs to be as in codex. When the user first runs ragon in a repo, it needs to authenticate against the primary ragon node. The authentication needs to be done exactly in the way codex does it against openai. ragon should ask for username and password or prompt for new username and password to be created. Empty passwords should be accepted.
```

The owner's scope, 2026-11-19 09:17 UTC, verbatim:

> this release = a simple implementation, not much security; the 1.0 goal is to identify users in a basic way (username + password, empty allowed, a token on requests). Hardening (lockout, rate limits, TLS, token binding) stays on the 3.0 list. The spec and plan-auth-1 follow this: smallest flow that identifies the user, Codex's screens and commands re-enabled, nothing more.

This spec reads the two together:
- "Exactly in the way codex does it" means Codex's code paths, screens and commands, with the node in place of the vendor's service.
- "Smallest flow" means every addition must be needed to identify the user. Anything that only makes the identification harder to forge goes to version 3.0.

## 2. Reference: what Codex does

This is the only paragraph that names the vendor; all paths are under `codex/codex-rs/`.

**Browser flow.** `codex login`, or the first run of the TUI when the provider has `requires_openai_auth`, starts a local HTTP server on loopback port 1455 (fallback port 1457; `login/src/server.rs:76-79`, `:635`, `:668-676`). It opens `https://auth.openai.com/oauth/authorize` in the browser with a PKCE S256 challenge and a random state (`login/src/server.rs:584-616`, `login/src/oauth/pkce.rs:14-28`, `login/src/oauth/authorization.rs:44-48`). The person signs in to their ChatGPT account on OpenAI's page, or creates one there. The page redirects to the loopback callback `/auth/callback` on that port (`login/src/server.rs:193`, `:363`). The callback then:
- exchanges the code at `/oauth/token` (`:709-717`);
- tries a token exchange for a vendor API key, and tolerates a failure (`:440-444`, `:1013-1047`);
- writes `auth.json` (`:831-875`).

**One-time-code flow.** `codex login --device-auth`, or the "Sign in with Device Code" choice, asks the issuer for a user code (`login/src/device_code_auth.rs:37-39`, `:68-78`). It prints the code and the URL `{issuer}/codex/device` (`:149-158`, `:175`) and polls for up to 15 minutes, treating 403 and 404 as "not yet" (`:107`, `:131-138`). It then exchanges the code it receives with the redirect `{issuer}/deviceauth/callback` (`:206-215`).

**auth.json.** It is written with mode 0600 at `$CODEX_HOME/auth.json` (`login/src/auth/storage.rs:154-155`, `:206-222`; the struct is at `:39-65`).
- It holds `auth_mode`, `OPENAI_API_KEY`, `tokens` (`id_token` as a raw JWT, `access_token`, `refresh_token`, `account_id`) and `last_refresh` (`login/src/token_data.rs:11-24`).
- The keyring and ephemeral stores are options (`config/src/types.rs:112-125`).

**Tokens on requests.** Requests carry `Authorization: Bearer` (`model-provider/src/bearer_auth_provider.rs:31-46`).
- In account mode, tokens are refreshed 5 minutes before expiry, or after 8 days without one (`login/src/auth/manager.rs:203-204`, `:2393-2406`).
- A 401 is answered by a reload, then a refresh, then a retry (`core/src/client.rs:1475`, `:1522-1536`).

**Logout and status.**
- `codex logout` revokes the refresh token at the issuer, warns on failure, and deletes the stored credentials (`login/src/auth/manager.rs:994-1019`, `login/src/auth/revoke.rs:55-75`).
- `codex login status` prints to stderr:
  - "Logged in using" followed by the vendor's account name;
  - "Logged in using an API key - " followed by a masked key;
  - or "Not logged in", which exits 1 (`cli/src/login.rs:443-508`, `:579-586`).
- `codex login --with-api-key` reads a key from stdin (`cli/src/main.rs:497-538`).

**The TUI.**
- It shows the login screen only when the provider has `requires_openai_auth` and nothing is stored (`tui/src/lib.rs:2296-2304`).
- The picker offers four choices (`tui/src/onboarding/auth.rs:488-527`): sign in with the vendor's account in the browser, "Sign in with Device Code", "Provide your own API key", and a third-party cloud choice.
- The app-server exposes the same flows to the desktop app as `account/login/start`, `account/login/cancel`, `account/logout` and `account/read`, with the notifications `account/login/completed` and `account/updated` (`app-server-protocol/src/protocol/common.rs:1276`, `:1297`, `:1303`, `:1443`, `:1982`, `:2035`; parameters in `v2/account.rs:61-176`).

The rest of this spec calls the vendor's account mode "account mode" and the provider flag `requires_openai_auth` by its name.

## 3. Terms

| Term | Meaning |
|---|---|
| node | A machine that serves the model and runs the model gate ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md)). |
| primary node | The node a client's resolver picks for this session (§4). It is not a setting or a role. |
| node id | The UUID in `~/.config/dreamference/node-id` on the node (`dreamference/node/node_identity.py`), advertised in the mDNS TXT record and reported by the gate probe. |
| account | A username and a password hash in the node's account store (§7.2). |
| node key | The bearer token a sign-in yields: `rgn_` followed by 43 base64url characters (32 random bytes). It is stored on the client in `auth.json` and on the node only as its SHA-256 (§7). |
| service account | The reserved account `ragon-service` whose key unattended callers on the node use (§10.3). |

## 4. The primary node

- **Which node.** The client signs in to the node its resolver returns, using the tiers of [RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md) §6.1 unchanged: an explicit host, `RAGON_NODE`, this machine, the overlay, the remembered `node.json`, the mDNS browse. "Primary" names the outcome of that resolution and adds no role. Decision 7 of RAGON_NODE stands.
- **Per node, not per repository.** Credentials are kept per node id (§7.1). The first run in a repository asks to sign in only when no credentials exist for the resolved node. The per-repository question, whether to trust the directory, stays Codex's (`tui/src/onboarding/trust_directory.rs`). See §15 Q2.
- **One store per node.** Each node keeps its own accounts. A person who uses two nodes signs in to each. Paired nodes, replicas and jobs on another node share no accounts in 1.x. Calls between nodes use the service key (§10.3).
- **When the gate does not check.** If the gate reports `auth: false`, or an older gate reports nothing (§5.6), the launcher leaves `requires_openai_auth` off. Nothing is asked and nothing is sent: the 0.9 behaviour.

## 5. Protocol

### 5.1 Where the issuer lives

The issuer is the model gate on the node's model port (8000 by default; `dreamference/config/ports.py`). Its base URL is `http://<node host>:<model port>`. A client reaches it exactly as it reaches the model: same host, port, overlay and proxy rules (`NO_PROXY` in `ragon-rs/src/proxy.rs`). No new port, advertisement or firewall rule is needed.

The issuer is not on the web port (3100):
- 3100 is `ragon web`'s and is loopback-only with `--no-web`;
- not every node runs it;
- its credentials are a separate system ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3).

### 5.2 Routes the gate serves

These are the paths Codex calls (§2), plus two pages. They are served by the gate itself and never forwarded to the engine.

| Route | Method | Codex caller | What the node does |
|---|---|---|---|
| `/oauth/authorize` | GET | `login/src/server.rs:584-616` | The sign-in page (§6.1). It requires `response_type=code`, `code_challenge_method=S256`, a `code_challenge`, a `state`, and a `redirect_uri` whose host is `localhost` or `127.0.0.1`. Other query parameters are ignored. |
| `/oauth/authorize` | POST | the page's form | Checks or creates the account (§6.1) and redirects to `redirect_uri?code=…&state=…`. The code is 32 random bytes, single use, valid 10 minutes, and bound to the challenge and the redirect URI. |
| `/oauth/token` | POST (form) | `login/src/server.rs:709-717` | `grant_type=authorization_code` with `code`, `code_verifier`, `redirect_uri`, `client_id`. Verifies S256 and returns `{id_token, access_token, refresh_token}` (§5.3). Any other grant gets 400 `{"error":"unsupported_grant_type"}`. That includes the token exchange of `:1013-1047`, whose failure Codex tolerates (`:440-444`). |
| `/oauth/revoke` | POST (JSON) | `login/src/auth/revoke.rs:30-53` | Deletes the session whose key is `token`. It answers 200 whether or not a session matched. |
| `/api/accounts/deviceauth/usercode` | POST | `login/src/device_code_auth.rs:68-78` | Returns `{device_auth_id, user_code, interval: 5}`. The user code is 8 characters from an unambiguous alphabet, valid 15 minutes. |
| `/api/accounts/deviceauth/token` | POST | `login/src/device_code_auth.rs:107` | 403 until the code is used on the device page. Then, once, returns `{authorization_code, code_challenge, code_verifier}` (`:55-60`). |
| `/codex/device` | GET, POST | the URL printed at `:175` | The same page as §6.1 with a user-code field in front. |
| `/ragon-gate` | GET | the launcher's probe | The existing probe. It gains `auth` (bool) and `node` (the node id) (§5.6). |

`client_id` is accepted and ignored. Codex sends its built-in id from the CLI (`cli/src/login.rs:157`) and from the app-server (`login/src/auth/manager.rs:1719-1724`). The node has one client and no reason to tell them apart. The `id_token_add_organizations`, `codex_cli_simplified_flow`, `originator` and `allowed_workspace_id` parameters (`login/src/server.rs:595-600`) are ignored as well.

### 5.3 What a sign-in returns

The node issues one node key per sign-in, in one new session. The token response carries it as both `access_token` and `refresh_token`:
- the access token is what the client stores as its key (§7.1);
- the refresh token is what `ragon logout` revokes (§8.1).

`id_token` is a JWT signed with HS256 using the node's `signing.key`. Its claims are `iss` (the issuer URL), `sub` (the username), `email` (the username, the claim Codex shows), `iat`, `exp` (`iat` + 10 years, since nothing expires in 1.x), and the account-id claim Codex reads (`login/src/token_data.rs:75-77`, `login/src/server.rs:849-853`), set to the node id. Nothing on the node trusts an `id_token` in 1.x. It exists because Codex's parser reads the user's name from it (`login/src/server.rs:844`).

### 5.4 The node key

- **Format.** `rgn_` + base64url(32 random bytes), from `secrets.token_urlsafe(32)` on the node.
- **Lifetime.** None in 1.x. A session ends at `ragon logout` (revoke), at `ragon-admin node user reset <name>` (all of that user's sessions) and at `ragon-admin node user remove <name>`. The gate records each session's last use, at minute granularity, for `user list`.
- **No refresh.** API-key mode never refreshes (§9.1), and the node refuses the refresh grant. A revoked key gets 401 on its next request.
- **Several sessions.** Each sign-in from another machine or home adds a session. Signing in again on the same machine replaces the file, but the old session stays until revoked or reset. §15 Q6 discusses expiry.

### 5.5 What the gate checks

When `model_auth` is on, every request to the gate passes this check before it is proxied:

1. **Open paths pass.** These are `GET /health`, `GET /v1/models`, `/ragon-gate` and the issuer routes of §5.2. The model list stays open so readiness probes and version checks keep working (§15 Q8).
2. **The bearer must be a live node key.** `Authorization: Bearer <key>` is required. The gate looks the key's SHA-256 up in `sessions.json`, which it reloads when the file's mtime changes, so a revoke takes effect on the next request. `x-api-key: <key>` is accepted the same way for clients that send keys that way.
3. **Otherwise, 401.** The response has `WWW-Authenticate: Bearer realm="ragon"` and an error body in the shape the model API already uses:
   `{"error":{"message":"Sign in to this Ragon node: run `ragon login`.","type":"invalid_request_error","code":"invalid_api_key"}}`.
4. **Identity is logged, the key is not forwarded.** The gate drops the client's `Authorization` before the engine. It already does this (`rewrite_head` in `dreamference/vllm_server/model_gate_service.py`), so the engine never sees a node key. The gate logs the username with the request line it already logs. It never logs the key, its hash or a password.

These existing gate rules run after the check: the SWE-bench hold (503 `benchmark_running` outside the run subnet), the pause file, the refused paths and the job header. A sign-in during a SWE-bench hold therefore still works, because the issuer routes are open. Only model requests from the LAN wait.

When `model_auth` is off, steps 2 and 3 are skipped. The issuer routes are still served, so accounts can be created before the check is turned on.

### 5.6 How the client learns the gate checks

The launcher already probes `/ragon-gate` before its first model call. The probe's JSON gains `"auth": true|false` and `"node": "<node id>"`. The launcher sets `requires_openai_auth = true` on its provider only when `auth` is true (§9.2).

| Client | Gate | Result |
|---|---|---|
| new | new, `auth: true` | Sign-in screen on first run; key on every request. |
| new | new, `auth: false` | As 0.9: no screen, no key. |
| new | old (no `auth` field) | As 0.9. |
| old | new, `auth: true` | Every model call gets the 401 of §5.5. Its message names `ragon login`, and an old client's `ragon update` fixes it. |

## 6. The screens

### 6.1 The node's sign-in page

The node's sign-in page is served by `/oauth/authorize` and `/codex/device`.

- **It is static HTML** with no script, a strict `Content-Security-Policy` (`default-src 'none'; style-src 'self'; form-action 'self'`) and `Cache-Control: no-store`.
- **The title is the node's name:** the `name` the mDNS advertisement already carries, or the hostname.
- **Two forms, side by side or as two tabs:**
  - **"Sign in to <node name>":** a username, a password and the button "Sign in". A wrong name or password gets one message, "Wrong username or password", whichever was wrong.
  - **"Create an account on <node name>":** a username, a password, "Confirm password" and the button "Create account". The two passwords must match, and both may be empty. A taken name gets "That username is taken". The new account is signed in at once.
- **Usernames** are 1–32 characters from `[a-z0-9._-]`, folded to lower case. `ragon-service` is reserved.
- **Passwords are taken by POST only.** They never appear in a URL, a log line or an error page.
- **After success** the browser is redirected to Codex's own callback, which shows Codex's success page (`login/src/success_page.rs`). That page links to the vendor's platform only when the issuer is the vendor's (`login/src/success_page.rs:82`). With the node as issuer, no vendor link is shown. Row 4 checks this.

Anyone who can reach the node can create an account. That is the trusted-LAN premise (README, "Accepted by design"), and 1.x keeps it (§11).

### 6.2 The terminal (TUI)

When the gate checks and nothing is stored for this node, the first run shows Codex's onboarding: Welcome, then the sign-in picker, then the trust question (`tui/src/onboarding/onboarding_screen.rs:141`, `:162-170`, `:189-203`). The picker's entries become:

| Codex's entry (`tui/src/onboarding/auth.rs`) | Ragon's entry | Flow |
|---|---|---|
| browser sign-in (`:498`) | "Sign in to <node name>" with "Opens the node's sign-in page in your browser; you can create an account there" | Browser flow (§2) against the node. |
| one-time code (`:506`) | "Sign in with a one-time code" with "For a machine without a browser" | One-time-code flow against the node. |
| API key (`:517`) | "Use a node key" with "Paste a key from `ragon login status --show-key` on another machine" | Codex's API-key screen (`:704-774`). |
| third-party cloud (`:526-527`) | hidden | — |

- **On success** the screen shows "✓ Signed in to <node name> as <user>" in place of Codex's line (`:671`). A pasted key shows "✓ Node key saved" in place of `:683-685`.
- **The node name** reaches the TUI as `RAGON_NODE_NAME`, set by the launcher from the resolved node.

### 6.3 The desktop app

The desktop app talks to `ragon app-server` (`desktop/electron/src/server.ts:64`). Codex's app-server already carries the same flows (§2), so the app needs:
- `ragon-rs/web/policy.json` to allow `account/login/start`, `account/login/cancel` and `account/logout` next to the existing `account/read`, and to pass the notifications `account/login/completed` and `account/updated`;
- the app's sign-in view, shown when `account/read` reports the provider needs sign-in and no account, to offer the same three entries as §6.2;
- the browser entry to open the returned `authUrl` with the system browser (Electron's `shell.openExternal`). The callback lands on the desktop machine's own loopback port 1455, as in Codex.

### 6.4 `ragon web`

`ragon web` keeps its own owner token and paired-device cookies ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3). It runs the app-server as the person who started it (`ragon-rs/web/src/app_server.rs:1-8`), so its model requests use that person's node key from `~/.ragon`.
- **No key stored.** The web UI shows the same sign-in view as the desktop app, but offers only the one-time code. The browser looking at the UI is often a phone, which cannot reach a loopback callback on the web host. The verification URL is the node's `/codex/device` page.
- **One account per instance.** In 1.x, a `ragon web` instance has one account: its owner's. Paired devices act as that account. Separate users per device belong with teams in version 2.0 (§15 Q5).

## 7. Storage

### 7.1 On the client

- **The path is `~/.ragon/nodes/<node id>/auth.json`,** one file per node.
  - Codex's single `$CODEX_HOME/auth.json` (`login/src/auth/storage.rs:154-155`) becomes this path through a hook in the launcher crate. That is the same pattern as `ragon_launcher::home::use_ragon_home()` (patch `0014`).
  - The node id comes from the resolver (`RAGON_SESSION_NODE`, `ragon-rs/node-locator/src/lib.rs:375`).
  - A client that cannot learn a node id (an explicit host on an old gate) uses `~/.ragon/nodes/default/auth.json`.
- **The fields are Codex's,** with these values:
  - `auth_mode: "apikey"` (§9.1);
  - `OPENAI_API_KEY`: the node key (Codex's field name, kept so that the file is Codex's format);
  - `tokens`: the `id_token`, the `access_token` and `refresh_token` (both the node key), and `account_id` (the node id);
  - `last_refresh`: the sign-in time.
- **Permissions.** Mode 0600 on create (`login/src/auth/storage.rs:206-222`), in a directory with mode 0700. On Windows, the file inherits the user profile's ACL (row 10 of §13).
- **File store only.** The launcher forces `cli_auth_credentials_store = "file"`, so the key never goes to a keyring that `ragon logout` might not clear (`config/src/types.rs:112-125`).
- **No carry-over.** `auth.json` is not copied when the home is created. `CARRY_OVER` in `ragon-rs/src/home.rs` already excludes it.

### 7.2 On the node

**The directory is `~/.local/share/dreamference/accounts/`** (0700, files 0600), owned by the node's user. It is mounted read-write into the gate container at `/ragon-accounts`, next to the existing read-only `/ragon-gate` mount, so turning auth on restarts the gate once. It holds four files:

| File | Content |
|---|---|
| `users.json` | `{"<name>": {"hash": "<PHC string>", "created": "<UTC time>"}}`. |
| `sessions.json` | `{"<sha256 hex of node key>": {"user": "<name>", "created": "…", "last_used": "…", "client": "<User-Agent, cut to 80 characters>"}}`. |
| `signing.key` | 32 random bytes, for the `id_token` signature. Created on first use. |
| `pending.json` | Authorization codes and device codes in flight: hashes only, each expiring within 15 minutes. |

Writes go to a temporary file in the same directory, then are renamed, under an `flock` on `accounts.lock`. The admin command (§8.2) and the gate share the lock.

**Hashing: scrypt.**
- Parameters: `ln=15, r=8, p=1` (N = 32768, 32 MiB), a 16-byte random salt and a 32-byte output. It is stored as `$scrypt$ln=15,r=8,p=1$<salt b64>$<hash b64>`, with base64 unpadded.
- The empty password is hashed with its own salt like any other. Nothing stores or compares it in clear.
- Verification compares in constant time (`hmac.compare_digest`).

Argon2id was the brief's first choice. scrypt is used because it is the stronger choice available to both sides without a new dependency on the gate:
- the gate is stdlib-only in the engine image, and `hashlib.scrypt` is in the standard library, with `maxmem` set to 64 MiB;
- argon2 is not, and neither is any argon2 package in `setup.py`.

The Rust node half on Windows and Mac (row 10) adds the `scrypt` crate to `ragon-rs`; version 0.11 is already in the Codex build's lockfile. A stored hash records its parameters, so a later move to argon2id (3.0) can rehash at the next sign-in.

## 8. Commands

### 8.1 `ragon login`, `ragon logout`, `ragon login status`

Codex's commands return (`cli/src/main.rs:497-547`, `cli/src/login.rs`). The launcher stops refusing them: `login` and `logout` leave `REMOVED_COMMANDS` (`ragon-rs/src/lib.rs:148-153`). Each acts on the resolved node.

| Command | Behaviour (Codex's, with the node as issuer) | Output (stderr, as Codex) |
|---|---|---|
| `ragon login` | Browser flow. Signing in replaces this node's stored key. The old session is revoked first (Codex clears before login, `cli/src/login.rs:122-137`). | "Starting local login server on http://localhost:1455. If your browser did not open, navigate to this URL to authenticate: …" (Codex's wording with the device hint pointing at `ragon login --device-auth`), then "Successfully logged in to <node name> as <user>". |
| `ragon login --device-auth` | One-time-code flow. | Codex's prompt (`login/src/device_code_auth.rs:149-158`) with the node's URL. |
| `ragon login --with-api-key` | Reads a node key from stdin and stores it in API-key mode, after a check against the gate (one `GET /ragon-gate` with the key). | "Successfully logged in to <node name>". |
| `ragon login status` | Reads this node's file. | "Logged in to <node name> as <user> - rgn_ab***yz" (Codex's `safe_format_key`, `cli/src/login.rs:579-586`), or "Not logged in to <node name>" with exit 1. With `--show-key`, the full key goes to stdout (for §10.4). |
| `ragon logout` | Revokes at the node and deletes this node's file. A failed revoke only warns, as in Codex (`login/src/auth/manager.rs:1011-1013`). | "Successfully logged out of <node name>", or "Not logged in to <node name>" (exit 0). |

`--experimental_issuer` keeps working (`cli/src/main.rs:533`) and overrides the node, which is useful for tests.

`ragon exec` with the check on and nothing stored stops before the first turn: "Not signed in to <node name>. Run `ragon login` (or `ragon login --device-auth` without a browser)." It exits 1. This is the launcher's check, made before Codex starts, because a headless run cannot show the screen.

### 8.2 `ragon-admin node user list|add|reset|remove`

These subcommands run on the node and read the store directly; there is no network call.

- **`list`** prints, one user per line, the name, the creation date, the number of live sessions and the last use. It never shows hashes or keys.
- **`add <name>`** prompts for a password twice. Empty is accepted, and `--password-stdin` reads one line instead. It fails if the name is taken or reserved.
- **`reset <name>`** sets a new password (prompted, as for `add`) and revokes all of the user's sessions. This is account recovery in 1.x.
- **`remove <name>`** deletes the user and their sessions. The command asks first unless `--yes` is given.

There is no bootstrap: the first account is made on the first sign-in page. `node remove` already exists (`dreamference/cli/dreamference_cli_controller.py:2489`), so `user` is a nested subparser, like `power-config`.

## 9. The Codex patches

### 9.1 Why API-key mode

`persist_tokens_async` writes account mode for every sign-in (`login/src/server.rs:856`). In account mode, Codex consumes the stored tokens in about sixty places outside the login crate, and most of them call the vendor's backend at `chatgpt_base_url`. The launcher points that URL at a closed port (`ragon-rs/src/lib.rs:111`), so each such call is either a failure in the session or a failure of the egress audit ([RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md) §"127.0.0.1:9 is a failure").

API-key mode sends the key to the configured provider:
- `model-provider/src/auth.rs:307-325`;
- the model list in `model-provider/src/models_endpoint.rs:840` and `models-manager/src/manager.rs:562`.

It also never refreshes. Storing the node sign-in in API-key mode keeps Codex's screens and flows and adds no vendor call.

### 9.2 What the launcher does without a patch

- When the probe says `auth: true`, the launcher adds `-c model_providers.openai-custom.requires_openai_auth=true` to the overrides it already passes (`with_local_model_args`, `ragon-rs/src/lib.rs:1658-1696`). This turns on Codex's login screen (`tui/src/lib.rs:2296-2304`) and the bearer on requests.
- It sets `CODEX_REVOKE_TOKEN_URL_OVERRIDE=<issuer>/oauth/revoke` and `CODEX_REFRESH_TOKEN_URL_OVERRIDE=<issuer>/oauth/token` (`login/src/auth/manager.rs:214-215`). The second is never used in API-key mode but closes the default.
- It sets `RAGON_AUTH_ISSUER=<issuer>` and `RAGON_NODE_NAME` for the hooks below.
- It forces `cli_auth_credentials_store = "file"`.
- It removes `login`/`logout` from `REMOVED_COMMANDS`, adds the `ragon exec` check (§8.1), and adds `--show-key`.

### 9.3 What comes back from the patches, and what changes

The patches call small functions in the launcher crate (`ragon_launcher::auth`), as `0014` does for the home. Strings and logic live in that crate, and the hunks stay one or two lines each.

| Patch | Hunks |
|---|---|
| `0038-node-issuer` | `login/src/server.rs`: the default issuer comes from `ragon_launcher::auth::issuer()` when `RAGON_AUTH_ISSUER` is set (`:76`, `:115`). `persist_tokens_async` writes `auth_mode: ApiKey` with the access token as the key when the issuer is a node (`:831-875`). `login/src/auth/revoke.rs`: `managed_chatgpt_tokens` also accepts API-key mode with stored tokens (`:79-85`), so logout revokes. `login/src/auth/storage.rs:154-155`: the file path through `ragon_launcher::auth::auth_file()`. |
| `0039-node-login-screens` | `tui/src/onboarding/auth.rs`: the picker's labels and descriptions (§6.2), the third-party cloud choice hidden, and the success lines. `cli/src/login.rs`: the success, status and logout strings, plus the device hint. |
| `0040-node-login-commands` | Undoes the hunks of `0005-hide-vendor-login.patch` that hide `login`/`logout` (`cli/src/main.rs`) and turn off `/logout` (`tui/src/slash_command.rs`), by deleting them from `0005`, not by a new hunk. Undoes `0016`'s `auth_check` hunk in the doctor, which then reports the node sign-in. Adds `--show-key` to `login status`. |

Patch `0001`'s status-card strings ("signed in", "API key configured") stay. They already read correctly for a node key.

**The size cap.** `tests/test_codex_branded_builder.py:322` holds the series under 64,000 bytes. It stands at 63,875 now, leaving 125 bytes. The three patches are estimated at about +5.5 KB, less about 1 KB removed from `0005` and `0016`. Row 3 raises the cap to 70,000 bytes, a raise that needs the owner's word (§15 Q7). Rows 4 and 5 stay inside it. Each row records the measured series size in `docs/dev/codex-build.md`, as earlier raises did.

## 10. Compatibility

### 10.1 Air-gapped mode

- **Sign-in needs no internet.** The issuer is the node, reached as the model is. `/airgapped on` blocks the internet, not the LAN or the node, so sign-in works air-gapped.
- **Nothing vendor-side is loaded.** The sign-in page uses no external font, script or image.

### 10.2 The egress audit

- **No new destination.** The issuer is the model host and port, which the audit already allows ([docs/dev/egress-audit.md](../docs/dev/egress-audit.md)).
- **The callback is a listener, not an outbound connection.** Codex's callback listens on loopback ports 1455/1457, and the browser connects to it. The audit traces the session's own outbound connections, so the allowlist (`allowed_ports` in `dreamference/audit/egress_audit.py`) gains nothing.
- **Row 9 adds checks that:**
  - an audited session signed in with a node key makes no connection to the closed port (`CHATGPT_BLACKHOLE_PORT`), which proves §9.1 holds in practice;
  - the issuer it uses equals the model host it was given.

### 10.3 Unattended callers

These callers on the node use the service account:
- Night Shift (`night_shift_task_run.py`);
- SWE-bench and its helpers (`swe_bench_instance_run.py`, `swe_bench_code_index.py`, `swe_bench_gate_hold.py`);
- the egress audits (`egress_audit.py`, `sync_egress_audit.py`);
- `codex_runner.py`;
- the first-run check (`cli/first_run.py`).

The service account works like this:
- **Creation.** The gate's start (`dreamference/vllm_server/model_gate.py`, on the host) creates `ragon-service` with a random password nobody uses, if it is missing. It writes one live key for it to `~/.config/dreamference/service.key` (0600). `ragon-admin node user reset ragon-service` rotates it.
- **Callers that start `ragon`** set `RAGON_API_KEY` from that file. When `RAGON_API_KEY` is set, the launcher passes `-c model_providers.openai-custom.env_key="RAGON_API_KEY"` and leaves `requires_openai_auth` off. Codex then sends the key from the environment (`model-provider/src/auth.rs:288-303`) and never shows a screen. This is the path Codex's own `CODEX_API_KEY` takes, under Ragon's name.
- **Callers that call the model over HTTP** read the key with one helper, `dreamference.node.node_accounts.service_key()`, and add the header. Instance containers and sidecars get it by environment, never in a mounted file inside a repository.
- **Unchanged.**
  - The Codex test runner points at the closed port (`codex_test_runner.py:91`) and is not affected.
  - Readiness probes use the open paths (§5.5).
- **Calls to another node** (paired nodes, `--node` jobs, replicas) use that node's service key. `ragon-admin node add` copies it once over the SSH connection it already opens, to `~/.config/dreamference/nodes/<node id>/service.key` (§15 Q4).

### 10.4 Third-party agents

Any client that speaks the model API and takes an API key works unchanged: point its base URL at the node and give it a node key. The key comes from `ragon login status --show-key` on a signed-in machine. It can also come from a sign-in the agent's own machine makes (`ragon login`, then `--show-key`). There is no separate key type and no key command in 1.x (§15 Q3).

### 10.5 Windows and Mac

- **Clients.** They are the same Rust: the hooks and patches have no platform branch.
  - Windows needs the browser opener (Codex's `webbrowser`) and the ACL on `auth.json`.
  - The Mac needs nothing beyond the browser.
- **Nodes.** Their node half is Rust ([WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §4.1, [MAC_NODE](./DREAMFERENCE_MAC_NODE.md)). It implements the same routes, store format and hash, so an account file moves between platforms. Until those nodes have a gate, they serve without the check (`auth: false`).

## 11. Out of scope

These are out of scope for 1.x, as the owner set it:
- an outside identity provider (OAuth or OIDC against a third party);
- single sign-on across nodes, and shared accounts between nodes;
- roles (an admin is whoever has a shell on the node), teams and per-user quotas (version 2.0);
- password policy beyond "may be empty";
- account recovery other than `ragon-admin node user reset <name>`;
- per-device users in `ragon web` (version 2.0).

The hardening goes to 3.0 and is listed in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) §"Release 3.0: authentication hardening":
- rate limits on the sign-in, device and token routes;
- lockout;
- TLS between client and node, since passwords and keys cross the LAN in clear in 1.x;
- token binding and key expiry;
- closing open sign-up;
- a minimum password;
- a protected key store on the client (keyring);
- protection of the signing key;
- cross-site request protection for the sign-in form.

## 12. What changes for the person using Ragon

- **The first run against a node with the check on** shows Codex's sign-in picker once per node. After that, nothing changes.
- **The model server is no longer open to anyone on the LAN.** Anyone can still create an account, so it is open to anyone who signs in.
- **Unattended jobs and benchmarks are unaffected** once rows 7 and 8 have landed and the check is on.
- **`ragon login status`** reports which node and which user.

## 13. Phases (the rows of `backlog/plan-auth-1.md`)

Each row is one repository, one file set and one test, can land alone, and is held until the coordinator releases it.

| Row | What | Repository | Needs the owner |
|---|---|---|---|
| 1 | Node account store and `ragon-admin node user …` | ragon (Python) | no |
| 2 | Issuer routes and the gate check, behind `model_auth` (off), and the read-write mount | ragon (Python) | no |
| 3 | Launcher: probe fields, provider flags, env, file hook, `ragon_launcher::auth`; patch `0038` and the cap raise | ragon (Rust + patch) | yes: the cap raise (§15 Q7) |
| 4 | Patch `0039`: the screens' strings | ragon (patch) | no |
| 5 | Patch `0040` and the launcher: `ragon login`, `logout`, `login status`, `--show-key`, `exec` check | ragon (Rust + patch) | no |
| 6 | Desktop app and `ragon web`: policy and sign-in view | ragon (Electron, web) | no |
| 7 | Service account, `service.key` and the unattended callers | ragon (Python) | no |
| 8 | Egress audit checks (§10.2) | ragon (Python) | no |
| 9 | Docs: the user page, `docs/dev/launcher.md`, one `AGENTS.md` line; turn `model_auth` on by default | ragon (docs + one default) | no |
| 10 | Windows and Mac clients (and the node-half routes, held until those gates exist) | ragon (Rust) | no |

## 14. Tests

- **Store.**
  - Round trip of an empty and a non-empty password.
  - Wrong password refused.
  - A stored hash is never the password, and no file holds it in clear.
  - The empty password's hash differs between two users.
  - `reset` revokes sessions; `remove` deletes them.
  - The reserved name is refused.
  - File modes are 0600 for files and 0700 for the directory.
- **Issuer.**
  - The full browser flow against the gate with a fake engine: authorize, POST, code, token with S256.
  - A wrong verifier is refused.
  - A code is single use.
  - A non-loopback `redirect_uri` is refused.
  - The device flow: 403 until used, then the code once.
  - Revoke ends the session on the next request.
  - No password or key appears in the gate's log (a grep in the test).
- **Gate.**
  - With `model_auth` on: 401 without a key, with a bad key and with a revoked key; 200 with a live key.
  - The open paths stay open.
  - The engine never receives the client's `Authorization`.
  - With it off: unchanged behaviour (the existing gate tests).
- **Launcher.**
  - Probe `auth: true` adds `requires_openai_auth`; `false` or missing does not.
  - `RAGON_API_KEY` uses `env_key`.
  - The file path is per node id.
  - `ragon exec` without credentials exits 1 with the message.
  - `login`/`logout` are no longer refused.
- **Codex build.**
  - The patched build's `login status`, `logout` and API-key-mode persistence work against a local issuer stub.
  - The series stays under the cap.
- **Egress.** The audited signed-in session makes no connection to the closed port (§10.2).

## 15. Open questions, each with its default

1. **Where the password is typed.** The owner wrote "ragon should ask for username and password". In Codex's flow the question is asked by the issuer's page in the browser, not by the terminal.
   - **Default:** the node's page (§6.1). It is Codex's flow exactly, needs no new terminal screen and keeps the patches smallest.
   - The alternative is a terminal form in the TUI that posts to the node. It would be a new screen Codex does not have, and would cost about 3 KB more of patch.
2. **"When the user first runs ragon in a repo".** **Default:** sign-in is per node, asked on the first run in any repository against a node with no stored credentials. The per-repository question stays Codex's trust screen.
3. **A key command for third-party agents.** **Default:** none in 1.x. `ragon login status --show-key` prints the stored key. A `ragon key create|list|revoke` for extra named keys is a 2.0 item with teams.
4. **The service key between nodes.** **Default:** `ragon-admin node add` copies the paired node's `service.key` over its SSH connection. Pairs made before row 7 get it from `ragon-admin node add --refresh-key <node>` (part of row 7).
5. **Users in `ragon web`.** **Default:** one account per instance (its owner's) in 1.x. Per-device users come with 2.0 teams.
6. **Node key lifetime.** **Default:** no expiry in 1.x. Sessions end at logout, reset or remove. Expiry and refresh are a 3.0 item; Codex's refresh path returns then with account-mode tokens or a node-side refresh grant.
7. **The patch cap.** **Default:** raise it from 64,000 to 70,000 bytes in row 3, for patches `0038`–`0040`. Each row records the measured size. Row 3 is held for the owner's word on this.
8. **The model list without a key.** **Default:** `GET /v1/models` stays open, so probes, `ragon doctor` and readiness checks need no key. It reveals only the served model's name.
9. **Choosing among several nodes.** **Default:** when the mDNS browse finds several nodes, the least-loaded pick ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md) §6.1) prefers nodes this client has credentials for. With none, it picks as today and the sign-in screen appears for the chosen node.
10. **The `model_auth` default.** **Default:** off until row 9 turns it on. A node owner can set it either way in `[node]`. A node with it off serves as 0.9 did.

## 16. Changes to other specs

These are made when the rows land, not by this document:
- **[RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md):** decision 4 ("No authentication") gains "using the model needs a node account once `model_auth` is on ([AUTH](./DREAMFERENCE_AUTH.md))". §4's binds table gains the issuer routes on the model port.
- **[RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.5:** "No account system" becomes "no account system of its own: model requests use the owner's node key ([AUTH](./DREAMFERENCE_AUTH.md) §6.4)".
- **[README](./README.md), "Accepted by design":** the row on the model server "with no API key" is updated when `model_auth` turns on.
- **[SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) #9:** gains the same update. The 3.0 list is added by this change.
- **[RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md):** `login` and `logout` leave the removed commands; patches `0038`–`0040` are added to its patch list.
- **[RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md):** the signed-in session check of §10.2.

## Sources

- The owner's decisions of 2026-11-19 (coordinator DECISIONS, AUTH and AUTH SCOPE).
- Codex's login crate, TUI onboarding, CLI and app-server at the paths cited, read at the vendored tree under `codex/codex-rs/`.
- The model gate, the launcher, the node resolver and `ragon web` at the paths cited in this repository.
