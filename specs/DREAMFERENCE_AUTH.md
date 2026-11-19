# Ragon authentication: Codex's sign-in, against the primary node

**Status:** decided by the owner on 2026-11-19 (§1); spec only, nothing built. The plan has ten rows, all held until the coordinator releases them one at a time (§14).
**Target:** release 0.9.9, on every Ragon client (`ragon`, the desktop app, `ragon web`; Linux, Windows and Mac from the same launcher code) and every node with a model gate. Enforcement follows at version 1.0 (§8).
**Builds on:**
- the launcher in `ragon-rs/` and its `~/.ragon` home ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md));
- node discovery, the node id and `ragon-admin init` ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md));
- the model gate in front of the engine on port 8000 (`dreamference/vllm_server/model_gate_service.py`, [INFERENCE](./DREAMFERENCE_INFERENCE.md));
- the Codex patch series and its size cap ([docs/dev/codex-build.md](../docs/dev/codex-build.md));
- `ragon web`'s own owner token and device cookies, which stay as they are ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3);
- `/airgapped` and the egress audit ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md), [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md)).

This document replaces the first version of the same name (browser flow against the node, accounts per node, no roles). The owner's second addendum and rulings of 2026-11-19 overrule that design.

**Decisions made here, stated first because each could be read the other way:**

1. **Ragon does exactly what Codex does, with two differences and no others** (§3):
   - the username and password are typed into Ragon itself;
   - the first screen offers "sign in" or "create a new username and password".
   Every other behaviour is Codex's, at the code path §4 names.
2. **The sign-in is stored as Codex's API key.** The token the primary returns goes where Codex keeps an API key, through Codex's own `account/login/start` `apiKey` path. This follows the owner's "the token stands in for the key" (§1). Codex's account mode is not used: it is built around the vendor's account and refreshes and revokes against the vendor's URLs (`codex/codex-rs/login/src/auth/manager.rs:203-213`). It is not a third difference, because the storage, header and commands are Codex's own (§6, open question 1).
3. **One primary node keeps every account** (owner 2026-11-19). Every node serves the authentication routes on its model gate port 8000. A secondary relays them to its pinned primary and checks tokens there, keeping the answer for 60 s (§5).
4. **The token never expires and is never refreshed.** It ends at logout, at `ragon logout --all`, or when the admin resets or removes the account (owner 2026-11-19). Addendum 2's "keep token refresh" is overruled by this later ruling. Codex's refresh code is not reached, since it runs only for account-mode tokens (§4).
5. **0.9.9 serves and logs; 1.0 refuses.** In 0.9.9 a model request without a valid token is served, logged and counted. At 1.0 the same request gets 401. One gate setting switches between the two (§8).
6. **The empty password is a password.** It is hashed with argon2id like any other. It is never logged and never put in a URL (owner 2026-11-19).
7. **Nothing beyond identifying the user goes into 0.9.9.** Rate limits, lockout, TLS and token binding go to the 3.0 list in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) (owner addendum, 2026-11-19).

---

## 1. The owner's words and the scope

The owner, 2026-11-19, verbatim:

```text
we need to reenable authentication that codex had. Everything needs to be as in codex. When the user first runs ragon in a repo, it needs to authenticate against the primary ragon node. The authentication needs to be done exactly in the way codex does it against openai. ragon should ask for username and password or prompt for new username and password to be created. Empty passwords should be accepted.
```

The four constraints in those words, one sentence group each:
```text
1. "we need to reenable authentication that codex had. Everything needs to be as in codex."
2. "When the user first runs ragon in a repo, it needs to authenticate against the primary ragon node."
3. "The authentication needs to be done exactly in the way codex does it against openai."
4. "ragon should ask for username and password or prompt for new username and password to be created. Empty passwords should be accepted."
```

The owner's addendum, 2026-11-19, verbatim:

> This release: a SIMPLE implementation, not much security. The 1.0 goal is to identify users in a basic way. Keep the spec and the plan to the smallest flow that does that (Codex screens and commands re-enabled, username + password with empty allowed, one token on requests, the gate checks it); every hardening item goes to the 3.0 list, none into plan-auth-1.

The owner's second addendum, 2026-11-19, verbatim and binding:

> Do EXACTLY what Codex does. Exactly two differences, no others: (1) username and password are typed directly into ragon (TUI prompt / app form), no browser flow and no device code; (2) the first screen offers two choices: "sign in" with an existing username and password, or "create a new username and password" right there. Keep everything else as Codex has it: the first-run prompt, auth.json and its fields/location, `ragon login|logout|login status`, token refresh and the Authorization header, the app-server login path for the desktop app. Where Codex has an API-key option, the token stands in for the key for the other clients; the screen itself shows only the two choices. The spec names each Codex behaviour it keeps with its code path, and marks the two differences explicitly as the only ones.

The owner's rulings of 2026-11-19 settle these points:
- the endpoint;
- the token's life;
- serve-and-log, then enforce;
- the release;
- the topology;
- the choice of primary;
- roles;
- sign-up;
- self-service;
- API keys;
- the service account;
- passwords;
- the scope;
- the screen text;
- the rules.

They are cited below as "owner 2026-11-19". Where a ruling is later than addendum 2 and differs from it (refresh), the ruling wins and the spec says so.

"When the user first runs ragon in a repo" is read with the scope ruling: the sign-in is once per **machine**, asked on the first run in any repository on a machine with nothing stored. It is never asked per repository.

## 2. Reference: what Codex does against the vendor's service

This is the only paragraph that names the vendor; all paths are under `codex/codex-rs/`.

**Sign-in.**
- `codex login`, or the first TUI run when the provider has `requires_openai_auth` and nothing is stored (`tui/src/lib.rs:2296-2304`), signs the person in to their ChatGPT account at `auth.openai.com`.
- It does so either in the browser (a loopback callback server with PKCE, `login/src/server.rs`) or with a one-time code (`login/src/device_code_auth.rs`).
- The TUI's picker offers "Sign in with ChatGPT", "Sign in with Device Code", "Provide your own API key" and a cloud choice (`tui/src/onboarding/auth.rs:490-530`).

**Storage.**
- The result goes to `$CODEX_HOME/auth.json`, mode `0600` (`login/src/auth/storage.rs:154-155`, `:206-222`).
- The fields are `auth_mode`, `OPENAI_API_KEY`, `tokens`, `last_refresh` and others (`storage.rs:39-65`, `login/src/token_data.rs:11-33`).
- A keyring store is an option (`config/src/types.rs:112-125`).

**Tokens.**
- Account-mode tokens are refreshed every 8 days (`login/src/auth/manager.rs:203-204`).
- Every request carries `Authorization: Bearer` (`model-provider/src/bearer_auth_provider.rs:31-46`).

**Logout.**
- `codex logout` revokes the refresh token at `auth.openai.com/oauth/revoke` (`manager.rs:213`, `:994-1019`; `login/src/auth/revoke.rs:68-85`).
- It then deletes the stored credentials (`manager.rs:981`).

**API key.**
- `codex login --with-api-key` reads an API key from stdin and stores it as API-key mode (`cli/src/login.rs:204`, `manager.rs:1022`).
- `CODEX_API_KEY` in the environment overrides everything (`manager.rs:1499-1504`).

## 3. The two differences, and they are the only ones

**Difference 1: the username and password are typed into Ragon.** Codex sends the person to the vendor's page in a browser, or shows a one-time code. Ragon asks directly:
- in the TUI's first-run screen;
- at `ragon login` in the terminal;
- in the desktop app's and `ragon web`'s sign-in form.

There is no browser flow and no device code.

**Difference 2: the first screen offers two choices.** Codex's picker lists its sign-in methods (§2). Ragon's lists exactly two:
- "Sign in": an existing username and password;
- "Create a new username and password": made on the primary right there, then signed in.

The screen shows nothing else. There is no API-key choice on it: the API key stays reachable as Codex's `ragon login --with-api-key` (§4) for other clients and scripts.

**What these differences are not.** Additions with no counterpart in Codex do not change any Codex behaviour, so they are not differences. The owner ruled each of them (owner 2026-11-19):
- the node side (§5, §7);
- roles and admin commands (§10);
- per-user API keys (§11);
- `/whoami`, `/password`, `/apikey` and `/users`;
- `ragon logout --all`.

The serve-and-log period (§8) is the node's behaviour, not the client's.

## 4. The Codex behaviours kept, each with its path

All paths are under `codex/codex-rs/`. "Kept" means the code runs as Codex ships it, or with the hunk named in §12.

| Codex behaviour | Code path | In Ragon |
|---|---|---|
| First-run sign-in screen when the provider needs auth and nothing is stored | `tui/src/lib.rs:2296-2304`, called at `:1285-1304` | Kept. Today the launcher writes its provider table without the auth flag, so it is false and the screen never shows (`ragon-rs/src/lib.rs:1563-1569`); row 3 sets `requires_openai_auth = true` there. The screen's picker content is difference 2 |
| The screen submits through the app-server's `account/login/start` | `tui/src/onboarding/auth.rs:888-900` (`LoginAccountParams::ApiKey`) | Kept: the form's token is submitted as the `apiKey` |
| `account/login/start`, `account/logout`, `account/read`, notifications `account/login/completed` and `account/updated` | `app-server-protocol/src/protocol/common.rs:1276`, `:1303`, `:1443`, `:1982`, `:2035`; params `app-server-protocol/src/protocol/v2/account.rs:64-100`; handler `app-server/src/request_processors/account_processor.rs:322-372` | Kept unchanged: the desktop app and `ragon web` use them, with no new variant |
| `auth.json` at the home, mode 0600, its fields | `login/src/auth/storage.rs:39-65`, `:154-155`, `:206-222` | Kept: `~/.ragon/auth.json` (the launcher's `CODEX_HOME`), `auth_mode: "apikey"` and the API-key field holding the token, written by Codex's `login_with_api_key` (`login/src/auth/manager.rs:1022`) |
| Credentials store choice (file, keyring, auto) | `config/src/types.rs:112-125` | Kept, default file |
| `login`, `logout`, `login status` commands | `cli/src/main.rs:158`, `:161`, `:498-550`, dispatch `:1553-1597` | Kept; `ragon login` without a flag asks in the terminal (difference 1, §9) |
| `login --with-api-key` from stdin | `cli/src/login.rs:204` | Kept: a token or a per-user API key |
| Status lines "Logged in using an API key - …" (masked), "Not logged in" (exit 1) | `cli/src/login.rs:443-508`, `safe_format_key` `:579-586` | Kept for API keys; for a login token the line names the user and role (owner 2026-11-19), §9 |
| Logout: revoke at the issuer, then delete the stored credentials | `login/src/auth/manager.rs:981`, `:994-1019`; `login/src/auth/revoke.rs:55-147` | Kept; the revoke goes to the primary through Codex's override `CODEX_REVOKE_TOKEN_URL_OVERRIDE` (`manager.rs:215`, `revoke.rs:134-147`), which the launcher sets |
| `/logout` in the TUI | `tui/src/slash_command.rs:69`, `:153`, `:261` | Kept: Ragon's patch `0005` that hides it is removed |
| `Authorization: Bearer` on every model request | `model-provider/src/auth.rs:197-222`, `:307-325`; `model-provider/src/bearer_auth_provider.rs:31-46` | Kept. API-key mode carries no account id, so no account header is sent (`login/src/auth/manager.rs:614-625`) |
| 401 handling: reload the stored auth, retry | `core/src/client.rs:2525` (`UnauthorizedRecoveryExecution`) | Kept unchanged |
| `CODEX_API_KEY` overrides stored auth for headless runs | `login/src/auth/manager.rs:964-966`, `:1499-1504`; `app-server/src/in_process_bootstrap.rs:154` | Kept: unattended callers pass their service key this way (§12) |
| What the auth flag does not turn on | `core/src/client.rs:1124-1133` and `model-provider-info/src/lib.rs:599-601` (the vendor's backend needs the provider named as the vendor's); `core/src/tools/spec_plan.rs:756-762` (needs account-mode auth); `app-server/src/turn_cost_worker.rs:101-123`, `:450-465` (the API-key cost query runs only with a telemetry exporter, and then at the provider's base URL, the node) | Checked: setting the flag on Ragon's provider wakes no call to the vendor's service; row 8's trace confirms it |
| Token refresh | `login/src/auth/manager.rs:203-204` | Not reached: refresh runs only for account-mode tokens, and the owner ruled that tokens never expire (decision 4) |

## 5. Topology: one primary

**The primary keeps everything** (owner 2026-11-19):
- users, roles, password hashes, login tokens and API keys live on one primary node, which is the authentication server;
- every other node is a secondary and keeps no accounts.

**Choosing the primary.**
- `ragon-admin init` (`dreamference/cli/dreamference_cli_controller.py`, `_ARG_INIT`) asks once: "primary (keeps the accounts) or secondary". It lists the primaries it sees advertised on the LAN.
- A primary adds `auth=primary` to its mDNS TXT record (`dreamference/node/node_service_file.py:68-77`; the client parses it at `ragon-rs/src/node.rs:1081`).
- A secondary **pins** the primary's node id in its node config (`[node] primary = "<node id>"`). It uses the advertisement only to find the primary's address.
- `ragon-admin node primary [<node>]` shows or changes the choice.

**The upgrade to 0.9.9.**
- One node is made primary and every other node a secondary, by the coordinator at the upgrade.
- A node never asked at init behaves as a secondary with no primary. It serves and logs (§8) and `ragon-admin status` says "no primary".

**What every node serves on port 8000** (owner 2026-11-19: the port every client already reaches):

| Route | Purpose |
|---|---|
| `GET /ragon-auth/info` | The node's display name, whether it is the primary, and the primary's display name. Used for the screen text (§6); no ids or addresses are shown |
| `POST /oauth/token` | `grant_type=password` with `username` and `password` in the form body (RFC 6749 §4.3). Returns `{access_token, token_type: "Bearer"}` and the user's role |
| `POST /ragon-auth/accounts` | Create an account (`username`, `password` in the JSON body) and return a token as above |
| `POST /oauth/revoke` | Codex's revoke body as Codex sends it (`login/src/auth/revoke.rs:48-53`, `:97-133`): revokes that one token |
| `POST /ragon-auth/revoke-all` | Bearer: revokes every login token of the bearer's user |
| `GET /ragon-auth/whoami` | Bearer: `{user, role, service}`; also the check secondaries make |
| `POST /ragon-auth/password` | Bearer: change one's own password (empty allowed) |
| `GET/POST/DELETE /ragon-auth/keys[/<name>]` | Bearer: list, mint and revoke one's own API keys by name |
| `GET/POST /ragon-auth/users[/<name>/<action>]` | Bearer of an admin: list, reset, remove, promote |
| `POST /ragon-auth/nodes` | Open: a secondary registers its service account by node id and receives its key once (§11, open question 4) |

On the primary, the gate forwards these routes to the accounts service on loopback (§7). On a secondary, the gate relays them to the pinned primary's port 8000. A client therefore only ever talks to the node it resolved, on the port it already uses.

**Secondaries check live and cache for 60 s** (owner 2026-11-19).
- For each request a secondary asks the primary's `/ragon-auth/whoami` with the request's bearer.
- It keeps `(valid, user, role)` in memory for 60 s, keyed by the token's SHA-256.
- Revocation therefore takes effect on a secondary within a minute; the owner accepted that lag.
- If the primary cannot be reached and the token is not cached, the answer is "authentication server unreachable" (§8 says what that means in 0.9.9 and at version 1.0).
- The primary checks its own store the same way, through loopback, with the same 60 s cache.

**Windows and Mac nodes** have no model gate ([WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md), [MAC_NODE](./DREAMFERENCE_MAC_NODE.md)).
- They cannot be primary; `ragon-admin init` does not offer it there.
- As secondaries they serve without a check until they have gates (open question 5).
- A client whose node has no `/ragon-auth/info` signs in against the advertised primary directly, on the primary's port 8000, which is the same port.

## 6. The screens (difference 1 and difference 2)

**The text** (owner 2026-11-19):
- The title is "Sign in to <node name>".
- On a secondary, the line "(accounts kept by <primary name>)" follows the title.
- No ids or addresses appear on screen.

The two choices are "Sign in" and "Create a new username and password". Both show two fields, Username and Password. The password is masked with "•", as Codex's own credential form masks it (`tui/src/onboarding/bedrock.rs:386-428`).

**The TUI.**
- The first-run screen appears where Codex's does (§4).
- Its picker is replaced by the two choices.
- The form is Ragon's: Codex's form trims every field and refuses an empty one (`tui/src/onboarding/bedrock.rs:307-312`), and the password here may be empty and is never trimmed.
- The form's state, its checks and the HTTP call live in the launcher crate (`ragon-rs`, module `auth`). The patch draws the form's lines and passes keys to it.
- On success the token is submitted exactly as Codex submits an API key: `account/login/start` with `LoginAccountParams::ApiKey` (`tui/src/onboarding/auth.rs:888-900`). Codex then writes `auth.json` and moves on.

**Errors**, shown in the form, never with the password:
- "Wrong username or password."
- "That username is taken."
- "Usernames are 1-32 letters, digits, '.', '_' or '-'."
- "Authentication server unreachable."

**The desktop app** (`desktop/electron/`).
- It spawns the app-server (`desktop/electron/src/server.ts:64`) and reads `account/read`.
- When nothing is stored, it shows the same two-choice form.
- It calls the node's `/oauth/token` or `/ragon-auth/accounts`, then submits the token with `account/login/start` `apiKey`, Codex's app-server path for the desktop app (§4).

**`ragon web`.**
- Its own owner token and device cookies stay as they are ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3).
- Its app-server runs as the instance's owner and uses the owner's `~/.ragon/auth.json`.
- When the owner is not signed in, the web page shows the two-choice form. The web server makes the call to the node, then `account/login/start`.
- `ragon-rs/web/policy.json` gains `account/login/start`, `account/logout` and the two notifications. It already allows `account/read`.

## 7. The node side

**The accounts service.**
- It is a small Python service on the primary, on loopback only, on a new port constant in `dreamference/config/ports.py` (`ACCOUNTS_PORT`, default 18100). It runs as a systemd user unit beside the gate's.
- It hashes passwords with argon2id through `argon2-cffi`, a new `install_requires` entry in `setup.py`.
- The gate runs inside the engine image, which has no argon2 (`dreamference/vllm_server/model_gate.py:112-146`). So the gate never hashes a password: it forwards those routes.

**The store.**
- It is one SQLite file, mode 0600, under the node's state dir (`~/.local/state/dreamference/auth/accounts.db`).
- `users`: name, lower-cased name (unique), argon2id hash, role, service flag, created.
- `tokens`: SHA-256 of the token, user, kind (login or key), key name (unique per user), created, last used.
- Tokens are never stored in clear.

**Tokens and keys.**
- A login token is `ragon-t.<primary node id>.<random>`; an API key is `ragon-k.<primary node id>.<random>`. The random part is 32 bytes, URL-safe base64.
- The prefix lets the client and the status line tell a login token from a key. The node id names the primary that issued it, which is how the stored sign-in is keyed by the primary (open question 1).
- The gate treats them alike: Bearer, same rights (owner 2026-11-19).

**Rules** (owner 2026-11-19):
- A username is 1-32 characters of `[A-Za-z0-9._-]`, unique on the primary regardless of case.
- A password is 0-256 characters. The empty password is hashed like any other.
- There is no expiry, no lockout and no rate limit in 0.9.9 (3.0 list).
- Passwords appear in no log, no URL and no error text. The gate's log line for these routes carries the route and the status code only.

**First admin.** The first account created that is not a service account becomes admin (owner 2026-11-19: the first account created becomes admin). Service accounts are excluded because the primary's own service account is created at init, before any person signs up.

**Sign-up is open** (owner 2026-11-19). Anyone at the sign-in screen may create an account; the admin can remove it.

## 8. The gate check: 0.9.9 serves, 1.0 refuses

The gate reads `Authorization` before it drops the client's header (`rewrite_head`, `model_gate_service.py:378-408`). It checks the bearer as in §5 and adds `user=<name>` to its request log line, next to the job header (`JOB_HEADER`, `:137`). Identifying the user in the log is the 1.0 goal.

| Case | 0.9.9 (`[node] auth_enforce = false`, the default) | 1.0 (`auth_enforce = true`, the default from then) |
|---|---|---|
| Valid token or key | served, `user=<name>` logged | served, `user=<name>` logged |
| No bearer | served, logged as `user=-` with reason `no token`, counted | 401 "sign in with `ragon login`" |
| Invalid or revoked bearer | served, reason `invalid token`, counted | 401 "invalid or revoked token" |
| Secondary, primary unreachable, token not cached | served, reason `authentication server unreachable`, counted | 401 "authentication server unreachable" (owner 2026-11-19) |
| Auth routes relayed to an unreachable primary | 503 "authentication server unreachable" | same |

**The counted warning.**
- The gate counts unauthenticated requests by reason and writes the counts to its status file. `ragon-admin status` shows them.
- It logs one warning per client address and reason per minute.

**The client in 0.9.9.**
- The TUI still shows the sign-in screen on the first run (the owner's words).
- `ragon exec` never prompts, as Codex's `exec` never does: with the auth flag set and nothing stored, Codex sends the request without a bearer (`codex/codex-rs/model-provider/src/auth.rs:205-221`), and the node serves and counts it.
- `GET /v1/models` and the health routes stay open at 1.0 too, so readiness checks need no key.

## 9. Commands and slash commands

**`ragon login`.**
- The launcher stops refusing `login` and `logout` (`ragon-rs/src/lib.rs:148-154`, `REMOVED_COMMANDS`, `ACCOUNT_REASON`, and the test that asserts the refusal).
- With no flag, `ragon login` is difference 1 on the terminal. It shows the two choices, then "Username:" and "Password:" with no echo.
- It then hands the token to Codex's `login --with-api-key` on stdin. Codex writes `auth.json` and prints "Successfully logged in" (`cli/src/login.rs:46`).
- `ragon login --with-api-key` is Codex's, unchanged.

**`ragon login status`** (Codex's command):
- for a login token: "Logged in to <node name> as <user> (<role>)";
- for an API key: Codex's own line, followed by the key's user;
- with nothing stored: "Not logged in", exit 1;
- with the primary unreachable: "Logged in (authentication server unreachable)".

**`ragon logout`** (owner 2026-11-19: "logout = this machine").
- Codex's logout runs: it revokes this machine's token on the primary, through `/oauth/revoke`, then deletes `auth.json`.
- A per-user API key is not revoked by logout, as Codex never revokes an API key at logout (`login/src/auth/revoke.rs:68-85`).
- **`ragon logout --all`** first calls `/ragon-auth/revoke-all` with the stored token, then runs the same logout. It revokes every login token of the user, on every machine; per-user API keys survive and are revoked by name (open question 7). The launcher handles the flag; Codex's CLI is unchanged.

**Slash commands** (owner 2026-11-19), in the TUI. The desktop app and `ragon web` have the same actions in their account menu:
- `/logout`: Codex's, re-enabled.
- `/whoami`: user, role and node, asked live.
- `/password`: opens the masked form with one field and changes one's own password. Empty is allowed.
- `/apikey [name]`: mints a key, shown once in the transcript. `/apikey list` and `/apikey revoke <name>` manage the rest.
- `/users`: admins only. It lists the users, and `/users reset|remove|promote <name>` acts on one.
  - A reset sets the password to empty; the person then sets their own with `/password`.
  - A reset revokes the person's login tokens.

## 10. Roles and admin commands

There are two roles, user and admin (owner 2026-11-19). On the primary, `ragon-admin node user` works on the store directly and needs no token:

```text
ragon-admin node user list
ragon-admin node user add <name> [--admin] [--password-stdin]
ragon-admin node user reset <name> [--password-stdin]    # default: empty password; revokes login tokens
ragon-admin node user remove <name>                       # revokes every token and key
ragon-admin node user promote <name>
ragon-admin node user key <name> [--name <key name>] [--revoke <key name>] [--list]
```

On a secondary these commands say "accounts are kept by <primary name>; run this there". The parser follows the nested-subparser pattern of `power-config` (`dreamference/cli/dreamference_cli_controller.py`).

## 11. API keys and the service account

**API keys** (owner 2026-11-19).
- These are per-user keys, separate from login tokens, as Codex keeps its API key apart from its account tokens (`login/src/auth/storage.rs:39-65`).
- They are minted by the admin (`ragon-admin node user key`) or by the user (`/apikey`).
- A key is shown once and stored hashed. A user may hold several, each revoked by name.
- Other clients and scripts put the key in their API-key field. The gate treats it like a token.

**The service account** (owner 2026-11-19).
- Each node has one, named `ragon-node-<first 8 characters of its node id>`.
- It is created on the primary when the node is set up: as the primary at init, or when a secondary pins its primary.
- Its role is user and it is marked "service" in the list. It cannot sign in at the screen: `/oauth/token` refuses it.
- Its API key is kept in a 0600 file in the node's state dir (`~/.local/state/dreamference/auth/service.key`).
- A secondary asks for it at pin time on the open route `/ragon-auth/nodes`, keyed by its node id (open question 4).

**Unattended callers** read that file and pass the key as `CODEX_API_KEY` to the Codex runs they start (§4), or as the bearer on direct model calls:

| Caller | Site |
|---|---|
| Night Shift | `dreamference/night_shift/night_shift_task_run.py:932`, `:1020` |
| SWE-bench harness | `dreamference/swe_bench/swe_bench_instance_run.py:1385` |
| egress audit | `dreamference/audit/egress_audit.py:358` |
| Codex test runner | `dreamference/runner/codex_test_runner.py:91`, `:531`; `dreamference/runner/codex_runner.py:78` |
| first-run check | `dreamference/cli/first_run.py:189` |

## 12. Codex patches and the size cap

| Patch | Contents |
|---|---|
| `0005-hide-vendor-login` | **Deleted** (1,054 B). Its two hunks hide `login`/`logout` (`cli/src/main.rs`) and turn off `/logout` (`tui/src/slash_command.rs`). |
| `0038-node-sign-in` | Status line for a `ragon-t.` token, via `ragon_launcher::auth` (`cli/src/login.rs`, the status branch at `:443-508`). Revoke a `ragon-t.` token in API-key mode, only when `CODEX_REVOKE_TOKEN_URL_OVERRIDE` is set, so a logout can never reach the vendor's URL (`login/src/auth/revoke.rs:68-85`). |
| `0039-sign-in-screen` | The two-choice picker in place of Codex's four (`tui/src/onboarding/auth.rs:490-530`), and drawing and key routing for the launcher's form. On success it calls the existing `apiKey` submission (`:888-900`). |
| `0040-account-slash-commands` | `/whoami`, `/password`, `/apikey`, `/users` in `tui/src/slash_command.rs`, with one dispatch arm into `ragon_launcher::auth`, in the style of `0028-adapter-slash-command`. |

There is no app-server protocol change, so the generated schema files under `app-server-protocol/schema/` stay as they are.

**The size cap.**
- `tests/test_codex_branded_builder.py:322` holds the series in `codex-patches/` under 64,000 bytes. It stands at 63,875, leaving 125 bytes.
- Estimate: `0038` about 1.5 KB, `0039` about 3 KB, `0040` about 3 KB, less the 1,054 B of `0005`. That is about +6.5 KB net, so about 70,400.
- **Default:** raise the cap to 74,000 bytes in row 3 (open question 2). Each row records the measured size.

## 13. Compatibility

- **Egress.** Every authentication call goes to the resolved node's model port, which the audit already allows (`dreamference/audit/egress_audit.py:202-224`). The allowlist is unchanged.
  - The audit's throwaway home has no `auth.json`. It passes the node's service key as `CODEX_API_KEY` (row 8).
  - It gains a trace of `ragon login`, `login status` and `logout` showing that they reach only that port, the revoke included.
- **Air-gapped.** `/airgapped` blocks the internet, not the LAN; sign-in works on it unchanged.
- **Other clients.** Any client that speaks the model API uses a per-user API key in its API-key field.
- **Windows and Mac clients.** These ship in 0.9.9 with the same launcher code. Codex's file store is the same there (`login/src/auth/storage.rs:206-222`).
- **Older clients.** 0.9.8 clients send no bearer. They are served and counted until version 1.0.

## 14. Phases and the plan

Release 0.9.9 carries rows 1-10 of `backlog/plan-auth-1.md`, all held, released one at a time:
1. account store and admin commands;
2. auth routes and the gate check;
3. launcher sign-in and patch `0038`;
4. sign-in screen `0039`, with `0005` deleted;
5. `ragon login|logout|status` and slash commands `0040`;
6. desktop app and web;
7. unattended callers;
8. egress audit;
9. docs;
10. Windows and Mac clients.

Release 1.0 sets `auth_enforce = true`. That is one line and is not in this plan.

## 15. Tests

- **Store.** Username rules. Case-insensitive uniqueness. An empty password signs in. The first non-service account becomes admin. A reset revokes login tokens. Remove revokes all.
- **Routes.**
  - The `/oauth/token` form body works.
  - Revoke accepts Codex's body.
  - A password never appears in the gate or service logs (a log scan in the test).
- **Gate.**
  - Each row of §8's table, in both modes.
  - A secondary's 60 s cache.
  - The unreachable primary.
- **Launcher.**
  - `login` and `logout` are no longer refused.
  - `logout --all` calls revoke-all.
  - The form keeps an empty, untrimmed password.
- **Patches.** The series applies. `cargo test` for the touched Codex crates runs in the build export, never in `codex/`. The size cap holds.
- **Egress.** The login trace stays on the model port.

## 16. Out of scope

- OAuth with an outside identity provider.
- Single sign-on across nodes beyond the one primary.
- Password policy beyond "may be empty".
- Account recovery beyond the admin's reset.
- Rate limiting, lockout, TLS between client and node, token binding, key expiry: the 3.0 list in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md).

## 17. Open questions, each with its default

1. **Storage mode.** The sign-in is stored in Codex's API-key mode (decision 2). Account mode refreshes and revokes against the vendor's URLs (`login/src/auth/manager.rs:203-213`).
   - **Default:** API-key mode. The token's `ragon-t.` prefix marks it as a login token.
   - Codex keeps one `auth.json` object, so a machine is signed in to one primary at a time. The owner's "keyed by the primary's node id" is met by the primary's node id inside the token (§7): at start the launcher compares it with the resolved node's primary (`/ragon-auth/info`). On a mismatch the launcher prints "signed in to another primary; run `ragon login`".
2. **The patch cap.** **Default:** raise it from 64,000 to 74,000 bytes in row 3, for `0038`-`0040`. This needs the owner's word. Row 3 is held for it.
3. **Admin reset.** **Default:** a reset sets the password to empty. `--password-stdin` sets a given one instead.
4. **A secondary's service account.** **Default:** a secondary registers itself on the open route `/ragon-auth/nodes` at pin time, keyed by its node id, with no admin step. The admin sees it marked "service" and may remove it.
5. **Windows and Mac nodes at 1.0.** **Default:** they serve without a check until their gates exist; that is a row of their own specs, not of this plan. `ragon-admin status` there says "no auth check".
6. **The accounts service port.** **Default:** 18100 on loopback, as a constant in `dreamference/config/ports.py`.
7. **What `--all` revokes.** The ruling says "all of the user's tokens"; API keys are separate from login tokens (owner 2026-11-19). **Default:** `--all` revokes every login token and leaves API keys, which other clients and scripts hold, to `/apikey revoke <name>`.

## 18. Changes to other specs

These are made when the rows land, not by this document:
- **[RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md):**
  - Decision 7 ("nodes have no roles") is superseded by the primary and secondary roles (§5).
  - Decision 4 ("no authentication") gains a pointer here.
  - `ragon-admin init` gains the primary question.
  - The TXT record gains `auth=primary`.
- **[RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.5:** "no account system" becomes "the owner's sign-in from `~/.ragon/auth.json`".
- **[RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md):** `login` and `logout` leave the removed commands. `0005` leaves the patch list; `0038`-`0040` join it.
- **[SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md):** #9 (the model server on port 8000) gains "served and counted in 0.9.9, refused at 1.0". The 3.0 list is updated by this change.
- **[README](./README.md), "Accepted by design":** the row on the model server "with no API key" changes at version 1.0.

## Sources

- The owner's words, addendum, second addendum and rulings of 2026-11-19 (the coordinator's decisions file).
- Codex at the vendored tree `codex/codex-rs/`, at the paths cited.
- The model gate, the launcher, the node code and `ragon web` at the paths cited in this repository.
