# Ragon authentication: Codex's sign-in, against the primary node

**Status:** decided by the owner on 2026-11-19 (§1); revision 3 of this spec, nothing built yet. The plan is a lane graph of parallel rows in `backlog/plan-auth-1.md` (§17).
**Target:** release 0.9.9, shipping on 2026-12-01, on every Ragon client (`ragon`, the desktop app, `ragon web`; Linux, Windows and Mac from the same launcher code) and every node with a model gate. Enforcement of credentials on model requests follows at version 1.0 (§10).
**Builds on:**
- the launcher in `ragon-rs/` and its `~/.ragon` home ([RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md));
- node discovery, the node id and `ragon-admin init` ([RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md));
- the model gate in front of the engine on port 8000 (`dreamference/vllm_server/model_gate_service.py`, [INFERENCE](./DREAMFERENCE_INFERENCE.md));
- the Codex patch series and its size cap ([docs/dev/codex-build.md](../docs/dev/codex-build.md));
- `ragon web` ([RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.3), the messenger bridges (`docs/dev/messengers.md`) and the node's Forgejo (`docs/dev/forge.md`);
- `/airgapped` and the egress audit ([RAGON_AIRGAPPED](./DREAMFERENCE_RAGON_AIRGAPPED.md), [RAGON_EGRESS](./DREAMFERENCE_RAGON_EGRESS.md)).

Revision 3 replaces revision 2 of the same name. Revision 2 stored an opaque token as Codex's API key, had no refresh, no TLS and no device code, and made one node user per chat identity. The owner's compatibility rulings of 2026-11-19 overrule each of those points: the primary is an OAuth2 server issuing signed JWTs, Codex's account mode runs unchanged, TLS ships now, device auth stays, and a phone binds to an existing user.

**Decisions made here, stated first because each could be read the other way:**

1. **Ragon does exactly what Codex does, with two differences and no others** (§3):
   - the username and password are typed into Ragon, in place of Codex's browser flow with the localhost callback; Codex's device-code sign-in stays;
   - the first screen's two sign-in lines read "Sign in to <primary name>" and "Create an account on <primary name>"; the API-key line stays as Codex's own path.
   Every other behaviour is Codex's, at the code path §4 names.
2. **The primary is an OAuth2 server and Codex's account mode runs unchanged** (owner 2026-11-19). Every grant returns `id_token`, `access_token` and `refresh_token`, signed JWTs carrying the claims Codex reads. Codex stores them in `auth.json` exactly as after a sign-in at the vendor's service, refreshes them with its own code and revokes them at logout (§5).
3. **One primary node keeps every account** (owner 2026-11-19). Every node serves the authentication routes on its model gate port 8000. A secondary forwards them to its pinned primary and checks credentials there, keeping the answer for 60 s (§7).
4. **Valid until logout.** The access token lives 3600 s and is refreshed by Codex's code; the refresh token never expires. A session ends at logout, at `ragon logout --all`, or when an admin resets or removes the account (owner 2026-11-19).
5. **TLS ships in 0.9.9** (owner 2026-11-19). Each node's gate serves HTTPS with a certificate from the node's own self-signed CA. Ragon pins that CA at first contact, and the installers and the pairing step add it to the user's trust store (§6).
6. **0.9.9 serves and counts; 1.0 refuses.** In 0.9.9 a model request with no credentials is served, logged and counted per day. A request with bad credentials gets 401 at once. At 1.0 a request with no credentials gets 401 too (§10).
7. **The login screen lives in `ragon-rs`** (owner 2026-11-19). A small Codex patch routes the onboarding sign-in step to it; Codex's own screen code is not edited in place. Three more patches follow (the issuer, the multi-entry `auth.json`, the sign-in RPC; §5.1, §11, §5.8). The series grows from 63,875 to about 74,300 bytes, and the cap rises from 64,000 to 76,000 bytes (§15).
8. **The empty password is a password.** It is hashed with argon2id like any other. It is never logged and never put in a URL (owner 2026-11-19).
9. **Hardening goes to version 3.0.** Key rotation, refresh-token rotation, rate limits, lockout, guessing limits for pairing and device codes, client registration and certificate rotation are on the 3.0 list in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) (owner addendum, 2026-11-19).

---

## 1. The owner's words and the scope

The owner, 2026-11-19, verbatim:

```text
we need to reenable authentication that codex had. Everything needs to be as in codex. When the user first runs ragon in a repo, it needs to authenticate against the primary ragon node. The authentication needs to be done exactly in the way codex does it against openai. ragon should ask for username and password or prompt for new username and password to be created. Empty passwords should be accepted.
```

The four constraints in those words, verbatim:
```text
1. "we need to reenable authentication that codex had. Everything needs to be as in codex."
2. "When the user first runs ragon in a repo, it needs to authenticate against the primary ragon node."
3. "The authentication needs to be done exactly in the way codex does it against openai."
4. "ragon should ask for username and password or prompt for new username and password to be created. Empty passwords should be accepted."
```

The owner's addendum, 2026-11-19, verbatim:

```text
This release: a SIMPLE implementation, not much security. The 1.0 goal is to identify users in a basic way. Keep the spec and the plan to the smallest flow that does that (Codex screens and commands re-enabled, username + password with empty allowed, one token on requests, the gate checks it); every hardening item goes to the 3.0 list, none into plan-auth-1.
```

The owner's second addendum, 2026-11-19, verbatim:

```text
Do EXACTLY what Codex does. Exactly two differences, no others: (1) username and password are typed directly into ragon (TUI prompt / app form), no browser flow and no device code; (2) the first screen offers two choices: "sign in" with an existing username and password, or "create a new username and password" right there. Keep everything else as Codex has it: the first-run prompt, auth.json and its fields/location, `ragon login|logout|login status`, token refresh and the Authorization header, the app-server login path for the desktop app. Where Codex has an API-key option, the token stands in for the key for the other clients; the screen itself shows only the two choices. The spec names each Codex behaviour it keeps with its code path, and marks the two differences explicitly as the only ones.
```

**The rulings.** The owner then ruled on every open point of revisions 1 and 2, in 29 rounds on 2026-11-19, cited below as "owner 2026-11-19". Where a later ruling refines an earlier one, the later one is followed and the spec says which:
- "no device code" in the second addendum is overruled: device auth stays, as Codex does it.
- "the screen itself shows only the two choices" is refined: Codex's screen layout stays, two of its labels change, and the API-key line stays.
- "no refresh, no expiry" is overruled: tokens are JWTs refreshed by Codex's code, valid until logout.
- "one node user per chat identity" is overruled: a phone or chat binds to an existing user.
- "TLS on the 3.0 list" is overruled: TLS ships in 0.9.9; certificate rotation stays on the 3.0 list.
- "the authorization-code grant is not offered" is refined: it is offered to the Forgejo client registered on the primary, and to the codes the device page approves (§5.4).

"When the user first runs ragon in a repo" is read with the scope ruling: the sign-in is once per **machine** and per primary, asked on the first run in any repository when nothing is stored for the resolved node's primary. It is never asked per repository.

## 2. Reference: what Codex does against the vendor's service

This is the only paragraph that names the vendor, apart from the owner's verbatim words in §1 and Codex's own identifiers in code spans. Codex as shipped signs a person in to their ChatGPT account at `auth.openai.com`. The browser flow starts a loopback callback server with PKCE (`login/src/server.rs`, `DEFAULT_ISSUER` at `:76`); the device-code flow asks `auth.openai.com/api/accounts/deviceauth/usercode`, shows a code and a page, and polls (`login/src/device_code_auth.rs:68`, `:107`, `:175`). The resulting tokens are JWTs whose claim `"https://api.openai.com/auth"` carries the plan, the user id and the account id (`login/src/token_data.rs:88-100`). Codex refreshes them at `auth.openai.com/oauth/token` and revokes them at `auth.openai.com/oauth/revoke` (`login/src/auth/manager.rs:212-213`). Its backend calls go to `chatgpt.com/backend-api/` (`core/src/config/mod.rs:4394`). The TUI's picker offers `Sign in with ChatGPT`, `Sign in with Device Code`, `Provide your own API key` and a line for a third-party cloud provider (`tui/src/onboarding/auth.rs:490-530`). Every path in this spec is under `codex/codex-rs/` unless it names another directory.

## 3. The two differences, and they are the only ones

**Difference 1: the username and password are typed into Ragon.** Codex's browser flow sends the person to the vendor's page and waits on a localhost callback (`login/src/server.rs`). Ragon asks directly instead:
- in the first-run screen in the terminal;
- at `ragon login` in the terminal, and `ragon login --username <name> --password-stdin` for scripts;
- in the desktop app's sign-in form and in `ragon web`'s login page.

The answer goes to the primary as an OAuth2 password grant (§5.4). The tokens that come back are stored and used exactly as Codex stores and uses the tokens of its browser flow. Codex's device-code sign-in is **not** replaced: `ragon login --device-auth` and the screen's device-code line run Codex's own client code against the primary (§5.5).

**Difference 2: the first screen's two choices.** Codex's picker keeps its layout, key bindings and follow-on steps (owner 2026-11-19). Two lines change:
- Codex's browser sign-in line (§2) becomes "Sign in to <primary name>": an existing username and password;
- a second line, "Create an account on <primary name>", takes the place of the third-party cloud line: a new username and password, made on the primary right there, then signed in.

`Sign in with Device Code` and `Provide your own API key` stay, with their own descriptions free of vendor names. The API-key line is Codex's own path, not a difference.

**What is not a difference.** Additions with no counterpart in Codex change no Codex behaviour, so they are not differences. The owner ruled each of them (owner 2026-11-19):
- the node side: the store, roles, the primary and secondaries (§7, §9);
- admin and self-service commands, sessions and API keys (§11 to §13);
- bindings for phones, Forgejo, Night Shift and local tools (§14);
- the serve-and-count period, which is the node's behaviour, not the client's (§10).

## 4. The Codex behaviours kept, each with its path

"Kept" means the code runs as Codex ships it, configured by the launcher, or with the patch §15 names.

| Codex behaviour | Code path | In Ragon |
|---|---|---|
| First-run sign-in step when the provider needs auth and nothing is stored | `tui/src/lib.rs:2296-2304`, called at `:1285-1304` | Kept. The launcher's provider entry sets `requires_openai_auth = true` (§15), so the step shows. Patch `0038` routes the step's widget to the screen in `ragon-rs` (decision 7) |
| The picker's layout, key bindings and follow-on steps | `tui/src/onboarding/auth.rs:490-530` | Kept in the `ragon-rs` screen, with the two labels of difference 2 |
| Account mode: `auth.json` with `auth_mode`, `tokens` (`id_token`, `access_token`, `refresh_token`, `account_id`), `last_refresh` | `login/src/auth/storage.rs:41-64`; `login/src/token_data.rs`; written by `persist_tokens_async` `login/src/server.rs:831-873` | Kept. The `ragon-rs` screen writes the same structure through Codex's public `codex_login::save_auth` (`login/src/lib.rs:72`, `login/src/auth/manager.rs:1149`) |
| Claims read from the id token: email, plan, user id, account id | `login/src/token_data.rs:72-100`, `:176-187` | Kept: the primary's JWT carries them (§5.3) |
| No signature check on the client; any algorithm accepted | `login/src/token_data.rs:129-140` (`decode_jwt_payload`) | Kept: the primary signs with RS256 so that Forgejo and secondaries can verify; Codex reads without verifying, as against the vendor |
| Plan shown raw when unknown | `protocol/src/auth.rs:68`, `:102`; `login/src/token_data.rs:48` | Kept: the plan claim holds the role, `user` or `admin` |
| Proactive refresh 5 minutes before `exp`, an 8-day fallback, recovery after a 401 | `login/src/auth/manager.rs:203-204`, `:3004-3026`, `:1849-1862` | Kept, against the primary through `CODEX_REFRESH_TOKEN_URL_OVERRIDE` (`manager.rs:214`, `:1726-1728`) |
| Refresh request body (JSON: `client_id`, `grant_type`, `refresh_token`) | `login/src/auth/manager.rs:1637`; `login/src/oauth/client.rs:80-87`, `:108-110` | Kept: the primary accepts this body (§5.4) |
| Logout: revoke at the issuer, then delete the stored entry | `login/src/auth/revoke.rs:39-52`; `login/src/auth/manager.rs:213`, `:215` | Kept, against the primary through `CODEX_REVOKE_TOKEN_URL_OVERRIDE` |
| Device-code sign-in | `login/src/device_code_auth.rs:68`, `:107`, `:131-138`, `:175`, `:206-215` | Kept unpatched: the primary serves Codex's device paths (§5.5) |
| `login --with-api-key` from stdin, the API key in `OPENAI_API_KEY` beside `tokens` | `cli/src/login.rs:204`; `login/src/auth/manager.rs:1022`; `login/src/auth/storage.rs:45` | Kept: an `sk-ragon-` key (§13) |
| `CODEX_API_KEY` overrides stored auth for headless runs | `login/src/auth/manager.rs:954`, `:964`; `exec/src/lib.rs:726`; `app-server/src/in_process_bootstrap.rs:154` | Kept: the launcher maps `RAGON_API_KEY` to it (§5.7) |
| `Authorization: Bearer` on every model request; `ChatGPT-Account-ID` with account-mode tokens | `model-provider/src/bearer_auth_provider.rs:32-42`; `model-provider/src/auth.rs:105-110`, `:205-219` | Kept; the gate checks the header against the token (§10) |
| App-server account RPCs: `account/login/start`, `account/login/cancel`, `account/logout`, `account/read`, `account/rateLimits/read`; notifications `account/login/completed`, `account/updated` | `app-server-protocol/src/protocol/common.rs:1276`, `:1297`, `:1303`, `:1309`, `:1443`, `:1982-1984`, `:2035`; params `app-server-protocol/src/protocol/v2/account.rs:64` | Kept: names, parameters and events unchanged (§5.8) |
| `/logout` in the TUI | `tui/src/slash_command.rs` | Kept: patch `0005`, which hides it, is deleted (§15) |
| The status card's account line: email and plan | `tui/src/status/card.rs:748-757`, `:864`; `tui/src/app_server_session.rs:684-687` | Kept: it shows the username (email claim) and the role (plan claim) |
| `auth.json` file mode `0600` on Unix; file store by default | `login/src/auth/storage.rs:214-217`; `config/src/types.rs:116-118` | Kept; the launcher adds the Windows ACL Codex does not set (§15) |
| Custom CA for every HTTP client | `http-client/src/custom_ca.rs:12`, `:61` (`CODEX_CA_CERTIFICATE`, falling back to `SSL_CERT_FILE`) | Kept: the launcher points it at the pinned node CA (§6) |
| Local compaction for a provider other than the vendor's | `model-provider/src/provider.rs:411-417` | Kept: the custom provider compacts locally, as upstream |

## 5. Wire compatibility with Codex and the vendor's API

**The goal** (owner 2026-11-19). Two kinds of client work against a node without knowing it is not the vendor's service:
- Ragon's build of Codex, signed in with tokens or with an API key;
- an **unpatched upstream Codex** or any SDK client written for the vendor's API, pointed at the node with `OPENAI_BASE_URL` and an `sk-ragon-` key.

Only the login screen and the account-creation call are Ragon's own. A suite test proves the second kind (§18).

### 5.1 One base URL

The node's gate serves every path at one base, `https://<node>.local:8000` (owner 2026-11-19). The issuer, the refresh URL, the revoke URL, the backend base and the model API all point there:

| Path | Purpose | Served by |
|---|---|---|
| `/v1/responses`, `/v1/chat/completions`, `/v1/embeddings` (if the engine serves it), `/v1/models` | the model API | every gate (§10) |
| `/oauth/token` | password, refresh-token, device-code and authorization-code grants (§5.4) | the primary; a secondary forwards |
| `/oauth/revoke` | logout | the primary; a secondary forwards |
| `/oauth/device/code` | device authorization, RFC 8628 shape (§5.5) | the primary; a secondary forwards |
| `/api/accounts/deviceauth/usercode`, `/api/accounts/deviceauth/token` | device authorization, the shape Codex's client sends (§5.5) | the primary; a secondary forwards |
| `/codex/device`, `/oauth/authorize` | redirects to the login page on the primary's `ragon web` (§14.4) | the primary; a secondary forwards |
| `/.well-known/jwks.json`, `/.well-known/openid-configuration`, `/oauth/userinfo` | the public key, the discovery document Forgejo reads, the claims of a token | the primary; a secondary forwards |
| `/ragon/v1/info` | node name, node id, primary name, primary id, issuer URL; no credentials needed | every gate |
| `/ragon/v1/ca.pem` | the node's CA certificate for pinning (§6) | every gate |
| `/ragon/v1/users*` | account creation and the user and admin API (§5.6) | the primary; a secondary forwards |
| `/backend-api/*` | the endpoints Codex's backend client calls (§5.9) | every gate, stubs |

**Codex has no issuer setting in `config.toml`.** The issuer is a constant (`login/src/server.rs:76`), overridden only by the hidden flag `--experimental_issuer` (`cli/src/main.rs:527-530`, used at `cli/src/login.rs:351`, `:403`) and by `CODEX_APP_SERVER_LOGIN_ISSUER` for the app-server (`app-server/src/request_processors/account_processor.rs:28`, `:599-602`). The refresh and revoke URLs have their own overrides (`login/src/auth/manager.rs:212-215`). None of these is limited to debug builds. The owner's "the issuer URL written into config.toml at every start" is met by a small patch to the login crate (owner 2026-11-19, question 1: patch, not environment variables alone):
- **Patch `0040-ragon-login-issuer`** (about 0.7 KB, counted toward the cap of §15). The login crate reads the issuer from one function: the environment override when set, else the top-level key `ragon_issuer` of `config.toml`, else the constant. The same function serves the refresh and revoke URLs (`login/src/auth/manager.rs:212-215`: the `/oauth/token` and `/oauth/revoke` paths on that issuer, unless their own overrides are set) and the device-code client (`login/src/server.rs:76`). The result: a plain `codex login --device-auth` or an IDE extension started with no environment still reaches the primary.
- the launcher writes `chatgpt_base_url = "https://<node>.local:8000/backend-api/"` and `ragon_issuer = "https://<node>.local:8000"` into `~/.ragon/config.toml` beside the provider entry (`config/src/config_toml.rs:409`; the second key is read by patch `0040` alone), at every start, as it writes `model_catalog.json` today;
- it still sets `CODEX_APP_SERVER_LOGIN_ISSUER`, `CODEX_REFRESH_TOKEN_URL_OVERRIDE` (`…/oauth/token`) and `CODEX_REVOKE_TOKEN_URL_OVERRIDE` (`…/oauth/revoke`) in the environment of every process it starts, so that a process started with another `config.toml` still follows the resolved node (the environment wins over the file), and passes `--experimental_issuer` to `login --device-auth` as before.

**Issuer discovery** (owner 2026-11-19). The launcher resolves the node as today, then asks `GET /ragon/v1/info`. The answer names the primary, whose id selects the `auth.json` entry (§15). The node is never asked at login.

**The advertisement is a node setting** (0.9.8 fix). `/ragon/v1/info` says `"accounts": true` only when the node sets `[accounts] enabled = true` in `~/.config/dreamference/config.toml` (or `DREAMFERENCE_ACCOUNTS_ENABLED=1`); the default is off in 0.9.8, so a launcher sees no accounts and stays unauthenticated as in 0.9.7. **The default is on from 0.9.9** (`DEFAULT_ACCOUNTS_ENABLED` in `dreamference/node/node_settings.py`).

### 5.2 Tokens

| Token | Form | Lifetime |
|---|---|---|
| `access_token` | JWT, RS256, signed by the primary | `expires_in` 3600 s; Codex refreshes it 5 minutes before `exp` |
| `id_token` | JWT, RS256, the same claims | as the access token |
| `refresh_token` | opaque, 32 random bytes, base64url; stored as SHA-256 on the primary | no expiry; reusable; ends at revoke (rotation is a 3.0 item) |
| API key | `sk-ragon-` plus 32 random bytes, base64url; stored as SHA-256 | no expiry; ends at revoke by name |

**The algorithm.** Codex reads any algorithm without a signature check (`login/src/token_data.rs:129-140`), so either Ed25519 or RS256 works without a patch. The primary uses **RS256**, because Forgejo's OpenID Connect source expects it and every JWT library a secondary might use supports it. One key pair per primary, created when the node becomes primary, never rotated in 0.9.9.

**Mid-stream revocation** (owner 2026-11-19). A response in flight completes; the next request with the revoked credential gets `401`.

### 5.3 Claims

| Claim | Value | Codex reads it at |
|---|---|---|
| `iss` | the primary's issuer URL, `https://<primary>.local:8000` | not read by Codex; checked by secondaries and Forgejo |
| `sub` | the user id | not read by Codex |
| `aud` | the `client_id` of the grant | not read by Codex |
| `exp`, `iat` | issue time and issue time plus 3600 s | `login/src/auth/manager.rs:3004-3026` (refresh timing) |
| `email` | the username, for Ragon's clients (owner 2026-11-19: "the email claim holds the username") | `login/src/token_data.rs:72-85`, `:176-178`; shown by `tui/src/status/card.rs:748-757` |
| `name`, `preferred_username` | the username | not read by Codex; read by Forgejo |
| `"https://api.openai.com/auth"` → `chatgpt_plan_type` | the role, `user` or `admin` | `login/src/token_data.rs:88-100`; unknown plans are shown raw (`protocol/src/auth.rs:68`, `:102`) |
| same → `chatgpt_user_id`, `chatgpt_account_id` | the user id | `login/src/token_data.rs:184-187`; `account_id` in `auth.json` (`login/src/server.rs:847-852`); sent back as `ChatGPT-Account-ID` (`model-provider/src/auth.rs:105-110`) |
| `"https://ragon.dev/auth"` → `org`, `primary_name`, `machine` | the primary's node id, its name, the machine name of the session | not read by Codex (it has no organisation claim); read by the gate and `ragon login status` |

**The email claim.** Two rulings meet here: "the email claim holds the username" and "the synthetic `<username>@<primary name>.local` in every email field". Codex prints the email claim where it shows the account, and the owner wants the username there. **Default** (open question 2): tokens issued to Ragon's own clients carry the bare username in `email`; the id token and `/oauth/userinfo` answer issued to the Forgejo client carry the synthetic address in `email` and the username in `preferred_username`; the synthetic address is the user's email everywhere else (the Account page, `/whoami`, exports). The address is never mailed.

**Gated features.** Where Codex gates a feature on the plan, the role counts as enabled: the node is the server, there is no tier (owner 2026-11-19).

### 5.4 Grants at `/oauth/token`

The endpoint accepts form-encoded and JSON bodies, because Codex sends the refresh request as JSON (`login/src/auth/manager.rs:1637`) and the device exchange as a form (`login/src/device_code_auth.rs:206-215`). Every successful grant returns the same set: `id_token`, `access_token`, `refresh_token`, `expires_in`, `token_type: "Bearer"` (owner 2026-11-19), so the client writes `auth.json` the same way after each.

| Grant | Body | Notes |
|---|---|---|
| `password` | `username`, `password`, `client_id`; optional `machine` | Difference 1. A service account is refused with `invalid_grant`. With the must-change flag set (§9) the answer is `{"error": "password_change_required"}` and the screen asks for a new password first |
| `refresh_token` | `refresh_token`, `client_id` | Codex's own body. Answers a new access and id token; the refresh token is returned unchanged |
| `urn:ietf:params:oauth:grant-type:device_code` | `device_code`, `client_id` | RFC 8628 (§5.5) |
| `authorization_code` | `code`, `redirect_uri`, `client_id`, `code_verifier` | For two callers only: the Forgejo client registered by `ragon-admin` (§14.2), and a code the device page approved for Codex's device client (§5.5) |

**Account creation** is the one Ragon-specific call behind the second choice: `POST /ragon/v1/users` with `username` and `password`, which answers the same token set as a password grant (owner 2026-11-19).

**PKCE.** `code_challenge` and `code_verifier` are accepted and ignored on the password and device grants; they are checked on the authorization-code grant (owner 2026-11-19).

**`client_id`.** Any value is accepted, recorded on the session and counted; there is no registration in 0.9.9 except the Forgejo client (owner 2026-11-19).

**Errors** follow `RFC 6749`: `invalid_grant` (wrong username or password, unknown or revoked refresh token, an expired device code), `invalid_request`, `unsupported_grant_type`, with HTTP 400 or 401. The text never says which of username and password was wrong.

**Revoke.** `POST /oauth/revoke` accepts Codex's body, `token`, `token_type_hint` and `client_id` (`login/src/auth/revoke.rs:39-52`). Revoking a refresh token ends that session and every access token issued from it (the gate checks the session at the live check, §10). It answers 200 for an unknown token, as RFC 7009 says.

**Token exchange for an API key.** After a browser sign-in Codex tries a token exchange for an API key and tolerates its failure (`login/src/server.rs:440-444`, `:1013-1047`). That path belongs to the replaced browser flow; the primary answers `unsupported_grant_type` if it is ever called.

### 5.5 Device authorization

The owner ruled RFC 8628, and also that Codex's device client stays unpatched. Codex's client does not speak RFC 8628: it posts to `{issuer}/api/accounts/deviceauth/usercode` (`login/src/device_code_auth.rs:68`, `:166-169`), polls `{issuer}/api/accounts/deviceauth/token` (`:107`), and on success exchanges an authorization code at `{issuer}/oauth/token` (`:206-215`). **Default** (open question 3): the primary serves both shapes over one `device_codes` table.

**Codex's shape**, for `ragon login --device-auth` and the screen's device line:
- `POST /api/accounts/deviceauth/usercode` with `{client_id}` answers `device_auth_id`, `user_code` and `interval` as a string, `"5"` (`login/src/device_code_auth.rs:28-34`, `:47-53`).
- Codex prints `{issuer}/codex/device` and the code (`:175`). That path redirects to `ragon web`'s `/device` page on the primary.
- `POST /api/accounts/deviceauth/token` with `{device_auth_id, user_code}` answers 403 while the code is pending, which Codex treats as "keep polling" (`:108`, `:131-138`). Once the page has approved it, the answer is `{authorization_code, code_challenge, code_verifier}` (`:56-60`).
- Codex exchanges the code at `/oauth/token` with `redirect_uri = {issuer}/deviceauth/callback` and stores the tokens.

**The RFC 8628 shape**, for other clients:
- `POST /oauth/device/code` answers `device_code`, `user_code`, `verification_uri` (`https://<primary>.local:3100/device`), `verification_uri_complete`, `expires_in: 900` and `interval: 5`.
- The client polls `/oauth/token` with the device-code grant and gets `authorization_pending`, `slow_down` or `expired_token`, as the RFC says, then the token set.

**Both shapes:** codes are valid 15 minutes; the poll interval is 5 s; the user code is 8 characters from an alphabet without look-alike letters, shown as two groups of four characters; the `/device` page asks for the code, then shows the same two choices as the terminal screen (sign in or create), then approves (owner 2026-11-19).

### 5.6 The Ragon-only API: `/ragon/v1/users`

Outside `/v1`, so no upstream client collides with it (owner 2026-11-19). Bodies are JSON; errors use the shape of §10.

| Method and path | Who | Does |
|---|---|---|
| `POST /ragon/v1/users` | anyone | creates an account; answers the token set (§5.4). The first non-service account becomes admin |
| `GET /ragon/v1/users/me` | the user | username, user id, role, email, primary name; also the secondaries' live check (§7) |
| `POST /ragon/v1/users/me/password` | the user | `{current, new}`; empty allowed; clears the must-change flag |
| `GET`, `POST`, `DELETE /ragon/v1/users/me/keys[/<name>]` | the user | lists, mints (shown once) and revokes the user's API keys |
| `GET`, `DELETE /ragon/v1/users/me/sessions[/<id>]` | the user | lists and revokes sessions and bindings; `DELETE …/sessions` revokes all (`--all`) |
| `DELETE /ragon/v1/users/me` | the user | deletes the account (§12) |
| `GET /ragon/v1/users/me/usage` | the user | the user's own usage per day |
| `GET /ragon/v1/users`, `POST /ragon/v1/users/<name>/reset`, `…/promote`, `…/demote`, `DELETE /ragon/v1/users/<name>`, `POST /ragon/v1/users/<name>/keys`, `GET /ragon/v1/usage` | admins | the admin functions of §12 |
| `POST /ragon/v1/bindings`, `GET`, `DELETE /ragon/v1/bindings[/<id>]` | the user; a bridge with its node's service key | pairing codes and bindings (§14.1) |
| `POST /ragon/v1/nodes` | `ragon-admin` on a node, with the primary's admin credentials or at pin time | creates the node's service account (§13) |

### 5.7 Environment variables

| Variable | Read by | Meaning |
|---|---|---|
| `RAGON_API_KEY`, `OPENAI_API_KEY` | the launcher | an `sk-ragon-` key; the launcher passes it to Codex as `CODEX_API_KEY` (`login/src/auth/manager.rs:954`), which overrides the stored entry. `RAGON_API_KEY` wins when both are set |
| `RAGON_BASE_URL`, `OPENAI_BASE_URL` | the launcher | a node URL in place of the resolved node. `RAGON_BASE_URL` wins |
| `RAGON_HOME` | the launcher | the home in place of `~/.ragon`, as `CODEX_HOME` is for Codex; `CODEX_HOME` is never honoured (patch `0014` stands) |
| `OPENAI_API_KEY`, `OPENAI_BASE_URL` | upstream SDK clients | the key and `https://<node>.local:8000/v1` |

The product-wide rename of `DREAMFERENCE_*` variables to `RAGON_*` is a separate plan (`plan-ragon-env`); this spec names only `RAGON_API_KEY`, `RAGON_BASE_URL` and `RAGON_HOME`.

### 5.8 The app-server

Codex's account RPCs keep their names, parameters and events (owner 2026-11-19; `app-server-protocol/src/protocol/common.rs:1276`, `:1297`, `:1303`, `:1309`, `:1321`, `:1443`, `:1982-1984`, `:2035`). `LoginAccountParams` has the variants `apiKey`, `chatgpt`, `chatgptDeviceCode`, `chatgptAuthTokens` and a third-party cloud variant (`app-server-protocol/src/protocol/v2/account.rs:64`). Each client of the app-server uses one of them:
- **The desktop app** shows the two-choice form in place of the browser step and calls a **new app-server RPC** (owner 2026-11-19, question 4: a new RPC, not a restart):
  - Name `account/ragon/signIn`, a client request in `app-server-protocol/src/protocol/common.rs` beside `account/login/start`, with params and result defined in `protocol/v2/account.rs`.
  - Params: `{ "mode": "signIn" | "create", "username": string, "password": string }`. The password is sent as typed: empty and untrimmed are valid.
  - Result: `{ "type": "ragon", "username": string, "role": string, "mustChangePassword": bool }`. A refusal is a JSON-RPC error with code `-32001` (wrong credentials), `-32002` (name taken or invalid, `create` only), or `-32003` (authentication server unreachable); the message is the screen's own text (§8.5).
  - Behaviour: the handler runs the password grant, or the account creation, at the issuer of patch `0040`, writes the entry of the active primary through the login crate's `save_auth`, and reloads `AuthManager` (`login/src/auth/manager.rs:2481`). It then sends the usual `account/updated` notification, so the app shows the user at once: **the app-server is not restarted** and keeps its threads.
  - Patch `0042-ragon-account-signin-rpc` (about 3.6 KB, counted toward the cap of §15). The handler is in `app-server/src/request_processors/account_processor.rs`, with one call into `ragon_launcher` in the style of `0028-adapter-slash-command`, so the grant, the CA pinning and the strings are the launcher's. This is a protocol change: the generated schema files under `app-server-protocol/schema/` are regenerated in the same row. The existing RPCs, with their names, parameters and events, are not touched.
  - The device line uses `chatgptDeviceCode` unchanged; the key line uses `apiKey` unchanged.
- **`ragon web`** holds the JWT in the browser. Its server hands the browser session's tokens to the app-server with `chatgptAuthTokens` and answers `account/chatgptAuthTokens/refresh` (`common.rs:1801`) with a refresh at the primary: the external-token mode Codex provides for clients that own the tokens.
- **IDE extensions built for Codex** keep working against Ragon's app-server: they see the same RPCs and an account with the username as email and the role as plan (`app-server-protocol/src/protocol/v2/account.rs:22`, `:557`).

`account/rateLimits/read` works because account-mode auth counts as Codex's backend (`app-server/src/request_processors/account_processor.rs:1127-1131`; `protocol/src/auth.rs:54-62`) and the gate answers the usage stub (§5.9).

### 5.9 The model API and the vendor-only endpoints

**Checks, errors and headers** are in §10. **Models, parameters and limits:**
- **`/v1/models`** lists the served id with `owned_by` the engine, as today, and the aliases `ragon`, `ragon-default` and the default model name that an upstream Codex sends, which is the priority-1 entry of `models-manager/models.json` (`:4`, `:74`; chosen at `models-manager/src/manager.rs:724-731`). Alias entries carry `owned_by: "ragon"` and the served id's `created` time (owner 2026-11-19). A request naming an alias runs the served model. An unknown model gets 404 with `code: "model_not_found"`.
- **Codex's model fetch.** With account-mode auth Codex asks `GET {base_url}/models?client_version=…` (`codex-api/src/endpoint/models.rs:34-44`, `:64`; `models-manager/src/manager.rs:565-569`) and expects `{"models": [ModelInfo…]}` (`protocol/src/openai_models.rs:838-843`). The gate answers that shape when `client_version` is in the query, built from the launcher's catalog fields, and the vendor's list shape otherwise.
- **Parameters** are forwarded. A parameter the engine rejects is removed with a logged note and never causes a `400` (owner 2026-11-19): `reasoning.effort` and `reasoning.summary` where the engine has no reasoning control, `store`, `metadata`, `service_tier`, `user`, `prompt_cache_key`, `safety_identifier`, `include` entries the engine does not produce, `previous_response_id` (the engine keeps no responses; Codex sends full input), `parallel_tool_calls` where unsupported, `logprobs` and `top_logprobs` where unsupported. The gate's existing adapter rules stay (`model_gate_service.py:834-898`).
- **Usage and limit headers.** Every `/v1/responses` answer carries the headers Codex's status line reads (`codex-api/src/rate_limits.rs:67-84`, credits `:182-222`): `x-codex-primary-used-percent: 0`, `x-codex-secondary-used-percent: 0`, the window and reset headers, `x-codex-credits-has-credits: true`, `x-codex-credits-unlimited: true` and `x-codex-active-limit` (`codex-api/src/api_bridge.rs:267`).
- **Server-side compaction** is forwarded when the engine implements it, otherwise answered with the 404 shape so a client falls back to local compaction (owner 2026-11-19). Codex calls the remote path only for the vendor's provider (`model-provider/src/provider.rs:411-417`), so Ragon's provider compacts locally either way.

**Vendor-only endpoints.** Codex's backend client sends these to `chatgpt_base_url` when signed in with account-mode tokens. Ragon's build already turns several off (patches `0006`, `0013`, `0015`); an upstream Codex reaches them only if its `chatgpt_base_url` points at the node. The gate answers under `/backend-api/`:

| Endpoint | Caller | Gate answer |
|---|---|---|
| `wham/usage` (and `api/codex/usage` when the base has no `/backend-api`) | `backend-client/src/client/rate_limit_resets.rs:124-129`, `backend-client/src/client.rs:149-155`; read at `tui/src/app/startup.rs:987-994` | stub: a well-formed answer with no limit reached and unlimited credits |
| `accounts/check` | `backend-client/src/client.rs:402-403` | stub: the signed-in user's account, role as plan |
| `profiles/me` | `backend-client/src/client.rs:419` | stub: username and synthetic email |
| `settings/user` | `backend-client/src/client.rs:736` | stub: empty settings |
| `workspace-messages` | `backend-client/src/client.rs:729` | stub: an empty list |
| plugin index (remote plugins) | `core-plugins/src/remote.rs:1038`, `:2258`; `core-plugins/src/remote/search.rs:44`; `core-plugins/src/remote_legacy.rs:129`; gated at `remote.rs:2347-2355` | the node's own plugin index; an empty list until it has one; nothing at all with `/airgapped` on (§16) |
| connectors | `connectors/src/lib.rs:191`, `:270`; `chatgpt/src/connectors.rs:175`; `core/src/connectors.rs:141` | stub: an empty list |
| cloud tasks | `backend-client/src/client.rs:459`, `:495` | 404 shape |
| config bundle | `backend-client/src/client.rs:540` | 404 shape |
| thread usage | `backend-client/src/client/thread_usage.rs:115-118` | 404 shape |
| analytics | `analytics/src/client.rs:142` | 404 shape; Ragon's build sends none (patch `0013`) |

Any other unknown path gets the vendor's 404 error JSON: `{"error": {"message": "Unknown path …", "type": "invalid_request_error", "param": null, "code": "unknown_url"}}`.

**WebSockets.** Codex's Responses WebSocket and realtime clients send the same Bearer at the upgrade (`codex-api/src/endpoint/responses_websocket.rs:405`, `:439`; `core/src/realtime_conversation.rs:1924-1927`). The gate checks it there like any request. Codex's `websocket-auth` crate is the server side of the app-server's own WebSocket listener (`websocket-auth/src/lib.rs:288`, `:333`, `:383-393`), not a client path; it stays as it is. The owner's "voice WebSocket behind `ragon web` requires the Bearer" is met on `ragon web`'s `/ws` and on `POST /api/transcribe`, which take the JWT (§14.4). `ragon web` has no realtime socket today (`ragon-rs/web/src/policy.rs:725` refuses `thread/realtime/start`). Open question 5.

## 6. TLS

**Certificates** (owner 2026-11-19: "self-signed TLS now"). The CA that `dreamference/remote` keeps does not fit: it is name-constrained to the remote scope (`dreamference/remote/remote_certificate_authority.py:64-74`), so a certificate for `<node>.local` would be rejected by every client. Each node instead gets its own self-signed CA, created by `ragon-admin init` (and by `ragon-admin node tls init` on an existing node):
- the CA key and certificate, and a server certificate for the node's names (`<node>.local`, the names the node advertises, `localhost`), under the node's state dir in `tls/`, keys mode `0600`;
- the server certificate is valid 825 days; the CA ten years; rotation is a 3.0 item.

**The gate serves HTTPS** on port 8000 with that certificate (`ssl=` on the gate's `asyncio.start_server`, `model_gate_service.py:969`). `ragon web` on port 3100 serves the same certificate.

**Pinning.** At its first contact with a node id the launcher fetches `/ragon/v1/ca.pem` and stores it as `~/.ragon/nodes/<node id>/ca.pem`. From then on it verifies the node against that CA only and points Codex's HTTP clients at it with `CODEX_CA_CERTIFICATE` (`http-client/src/custom_ca.rs:12`, `:61`). A changed CA for a known node id stops the launcher with "The certificate of <node> has changed. If the node was set up again, run ragon login --reset-pin." A secondary pins its primary's CA the same way when it pins the primary.

**Trust store** (owner 2026-11-19). The installers and the pairing step add the primary's CA, and the resolved node's, to the user's own trust store, so browsers warn about nothing: on Linux the user's NSS database (`~/.pki/nssdb`) and the user certificate directory the desktop app reads; on Windows the current user's root store; on Mac the login keychain. Nothing is installed system-wide.

**Upstream clients.** `ragon-admin node user key` and `/apikey` print the CA file's path and the lines `export SSL_CERT_FILE=…`, `REQUESTS_CA_BUNDLE=…` and `NODE_EXTRA_CA_CERTS=…` (§13).

**Plain HTTP and older clients.** The owner ruled "plain HTTP is refused except on loopback", and also that 0.9.8 clients, which speak plain HTTP to port 8000, are served and counted until version 1.0. **Default** (open question 6): in 0.9.9 the gate tells TLS from plain HTTP by the first byte on port 8000. A plain request without credentials is served and counted as an older client; a plain request to `/oauth/*` or `/ragon/v1/*`, or one carrying a Bearer or `api-key`, is refused with `403` "Use https://". At version 1.0 plain HTTP is refused except on loopback.

## 7. Topology: one primary

**The primary keeps everything** (owner 2026-11-19).
- One primary node is the authentication server. It keeps users, roles, password hashes, tokens, API keys, device codes, pairing codes and the JWT signing key.
- Every other node is a secondary and keeps no accounts. It holds only the primary's public key and a 60 s answer cache in memory.

**Choosing the primary.**
- `ragon-admin init` (`dreamference/cli/dreamference_cli_controller.py`, `_ARG_INIT`) asks once: "primary (keeps the accounts) or secondary". It lists the primaries it sees advertised on the LAN, by name.
- A primary adds `auth=primary` to its mDNS TXT record (`dreamference/node/node_service_file.py:68-77`; the client parses the record at `ragon-rs/src/node.rs:1081`).
- A secondary **pins** the primary's node id in its node config (`[node] primary = "<node id>"`). It uses the advertisement only to find the primary's address; an advertisement with another id is ignored.
- `ragon-admin node primary` shows the choice; `ragon-admin node primary <node id>` changes it. On the primary itself, `ragon-admin node primary --self` makes the node the primary.
- Setting a node up (as primary, or when it pins a primary) creates its service account on the primary (§13).

**The upgrade to 0.9.9.** The coordinator makes one node the primary and every other node a secondary (owner 2026-11-19); that is an operations step, not product code. During development a test primary and a test secondary are used, and the test primary's store is wiped before the real primary is set (§17).

**Secondaries proxy and verify.**
- A secondary forwards `/oauth/*`, `/api/accounts/deviceauth/*`, `/codex/device`, `/.well-known/jwks.json`, `/.well-known/openid-configuration`, `/ragon/v1/users*`, `/ragon/v1/bindings*` and `/ragon/v1/nodes` unchanged to its pinned primary's gate (§5.1). It answers `/ragon/v1/info`, `/ragon/v1/ca.pem` and the `/backend-api/*` stubs itself. A client never needs the primary's address; the screen names the primary (§8).
- A secondary fetches the primary's public key once from `/.well-known/jwks.json` and verifies JWT signatures locally.
- For each request, a secondary also asks the primary whether the token or key is still valid (`GET /ragon/v1/users/me` with the request's bearer) and keeps `(valid, user id, username, role)` in memory for 60 s, keyed by the SHA-256 of the bearer. That round trip covers revocation and role changes (owner 2026-11-19). A revoked token can therefore work on a secondary for up to a minute; the owner accepted the lag.
- With the primary unreachable and the bearer not cached, the secondary answers `401` "authentication server unreachable", in 0.9.9 and at 1.0 alike (§10; owner, 2026-11-25). The JWT signature alone never admits a request on a secondary: there is no local trust.
- The primary checks its own store directly, with the same 60 s cache for keys.

**Moving the primary** (owner 2026-11-19). `ragon-admin node user export` on the old primary (JSON: users, roles, key hashes; never tokens), `ragon-admin node user import` on the new one, `ragon-admin node primary <new id>` on every secondary. The new primary has a new signing key, so everyone signs in again.

**Windows and Mac nodes** (owner 2026-11-19, the thirteenth question) **ship with the auth gate at version 1.0; no ungated node ships.** Neither can be primary. Each runs the §10 check in front of its model API, as a secondary: it holds the primary's public key, forwards the authentication routes to its pinned primary, answers `/ragon/v1/info`, `/ragon/v1/ca.pem` and the `/backend-api/*` stubs, serves HTTPS with its own node CA (§6), and counts unauthenticated requests until 1.0 as every gate does. The requirement is a line in the 1.0 requirements of [WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §10 and [MAC_NODE](./DREAMFERENCE_MAC_NODE.md) §9, and the check joins their acceptance gates (a request without a credential is refused at 1.0). Until their gates exist those nodes are not part of the 0.9.9 plan, which has no Windows or Mac node. A client whose resolved node has no gate yet signs in at the node's pinned primary, found by its advertisement. The Windows and Mac **clients** ship in 0.9.9 from the same launcher code (§16).

## 8. The screens

### 8.1 The upgrade note

The first start after the 0.9.9 upgrade, on a machine whose home has no `auth.json` entry for the primary, shows one explanation screen before the choices (owner 2026-11-19):

```text
Ragon now has accounts. Sign in or create one on <node name>; your repositories and settings are unchanged. Phones need to pair again.
```

Enter continues to the choices. A node that has no account yet opens at the first account's form instead (§8.6). There is no "later": the installers are non-interactive, and the first start is Codex's onboarding moment (owner 2026-11-19). A fresh install shows no note, only the choices.

### 8.2 The two-choice screen

The screen sits where Codex's sign-in step sits (§4), drawn by `ragon-rs` through patch `0038` (§15). Its title is "Sign in to <node name>", followed on a secondary by "(accounts kept by <primary name>)"; no ids or addresses appear (owner 2026-11-19). The lines, in Codex's order and with Codex's keys (arrows or the number, Enter):

| Line | Codex's line it replaces | Leads to |
|---|---|---|
| 1. Sign in to <primary name> | Codex's browser sign-in line (§2) | the sign-in form (difference 1) |
| 2. Create an account on <primary name> | the third-party cloud line | the create form (difference 1) |
| 3. Sign in with a device code | `Sign in with Device Code` | Codex's device flow against the primary (§5.5) |
| 4. Provide your own API key | `Provide your own API key` | Codex's key entry, for an `sk-ragon-` key (§13) |

The descriptions under lines 3 and 4 name no vendor: "Sign in from another device with a one-time code" (Codex's own text) and "Paste an API key minted with /apikey or by an admin". Line order is open question 7.

**The sign-in form:** Username, Password. **The create form:** Username, Password, Confirm password; no email, no display name (owner 2026-11-19). In both the password is masked with "•", never trimmed and may be empty; Codex's own credential form trims every field and refuses an empty one (`tui/src/onboarding/bedrock.rs:307-312`), so the forms are `ragon-rs` code, as the screen is. An empty password at the create form asks "Continue without a password?" once.

**Errors**, shown in the form, never with the password:
- "Wrong username or password."
- "That username is taken."
- "Usernames are 1-32 letters, digits, '.', '_' or '-'."
- "The passwords do not match."
- "<node name> cannot reach its accounts server <primary name>. Try again or ask the node's admin." with "r" to retry. There is no unauthenticated start (owner 2026-11-19).

**After success** the screen writes the tokens with `codex_login::save_auth`, asks Codex's auth manager to reload (`login/src/auth/manager.rs:2481`) and hands control back to Codex's onboarding, which continues with its own next step.

**The first admin.** After the first account is created, one line appears before the normal start (owner 2026-11-19):

```text
You are the first user of <primary name> and its admin. Others can create accounts at the login screen; /users manages them.
```

On the upgrade of an existing node the line is followed by "Phones need to pair again." (§14.1).

**Must change the password.** After a sign-in with the one-time password of a reset (§9), the screen shows the change form, "Set a new password for <user>", New and Confirm, empty allowed, before anything else.

### 8.3 The other screens

- **The desktop app** (`desktop/electron/`) shows the same lines and forms in its own window when `account/read` reports no account; the forms call the launcher, as §5.8 says.
- **`ragon web`'s login page** on the primary shows the same lines (without the API-key line) for the device page and Forgejo (§14.4).
- **`ragon login`** in the terminal shows the same lines as a plain prompt without the full-screen layout; the password is read without echo.

### 8.4 The status bar

Codex's status line has no account item (`tui/src/bottom_pane/status_line_setup.rs:56`, the item list; the host name is item `Hostname` at `:79`). Patch `0039` adds one item, drawn from `ragon-rs`: "<username> @ <node name>" (owner 2026-11-19). The launcher puts it in the default status line it writes. The primary's name appears only in `ragon login status`, when it differs from the node.

### 8.5 Languages

The login, account and session strings ship in English, Portuguese, Spanish, German and French in 0.9.9 (owner 2026-11-19). The launcher has no string table today; the only translated strings in the repository are the website's (`website/i18n/`). **Default** (open question 8): one table in `ragon-rs` for the strings this spec adds, keyed by message id, the language taken from `LC_ALL`, `LC_MESSAGES` or `LANG` and English as the fallback; the desktop app and `ragon web` read the same table through the launcher. Codex's own strings stay in English.

### 8.6 The first account

A node with accounts on and no account yet (every 0.9.8.1 node on its first start after the 0.9.9 upgrade, and every fresh install) asks for its first account at the first interactive start (owner 2026-11-23, "sign-in with setup"):

- **The terminal launcher on the node** opens the screen at the create form, not at the note or the choices: "<node> has no accounts yet. Create the first one: it is the admin of <node>. Phones need to pair again." Name, password and the password again, with the rules of §8.2 and of `ragon-admin node user add` (an empty password asks once). The account is created on this node, which becomes the network's primary (`ragon-admin node migrate`'s steps: primary role, advertised, its service account), the user is signed in, and the start continues with the first-admin line of §8.2.
- **`ragon-admin node user add <name>`** on the node does the same for its first account: it makes the node the primary, or says to run `ragon-admin node migrate` when it cannot. On a pinned secondary it changes nothing about the role.
- **A launcher on another machine** whose node has no account yet gets one plain line and runs without sign-in, as the node serves without it in 0.9.9 (§10): "<node> has no accounts yet. Its first account is made on <node> itself: run ragon in a terminal there, or ragon-admin node user add <name>. Running without sign-in until then." It never shows a form the node would refuse.
- **`ragon web` and the desktop app** in 0.9.9-rc.1 show the node's refusal as the same plain sentence when a create is tried from them; their own first-account form is an rc.2 item.
- **Unattended starts** (`ragon exec`, the route rows, Night Shift, SWE-bench, the egress audit, CI) never ask: only an interactive start reads the setup document, so they behave as before.

How the node knows: `GET /ragon/v1/setup` (public) answers `{"setup": <no account yet>, "local": <the caller is on this machine>, "node": <name>}`. The gate strips any `X-Ragon-Local` a client sends and adds `X-Ragon-Local: 1` only for a caller on the node: a loopback peer, or one whose address is the address it reached (a launcher that reaches its own node by its LAN name). The accounts service listens on 127.0.0.1 only and refuses the first account without that mark: it answers `403` with `first_account_on_node` and the sentence above. After the first account the rules of §9 apply unchanged. The service runs as the user unit `ragon-accounts.service`, which `ragon-admin server start` installs and starts when accounts are on (`--secondary` on a pinned secondary).

**Phones.** The first account binds no old pairing: all of them were dropped at the upgrade with a backup kept (§14.1). The user pairs again at once with `ragon chat pair <kind>` or `ragon signal setup`; an unpaired phone is told how once a day.

## 9. The node side: store, identity, rules, audit

**The store** (owner 2026-11-19). One SQLite file under the node's state dir, mode `0600`, backed up with the state dir:

| Table | Columns |
|---|---|
| `users` | user id (random, stable), username, lower-cased username (unique), argon2id hash, role, service flag, must-change-password flag, created, removed |
| `tokens` | SHA-256 of the refresh token, user id, client id, machine name, created, last use, revoked |
| `keys` | SHA-256 of the key, user id, key name (unique per user), created, last use, revoked |
| `device_codes` | SHA-256 of the device code, user code, client id, expires, approved user id, consumed |
| `bindings` | chat kind, chat identity, user id, created (§14) |
| `pairing_codes` | code, user id, chat kind, expires, consumed (§14) |
| `usage` | day, user id, client kind, requests, input and output tokens (§9 usage) |

**The accounts service.** A new service beside the gate owns the store and answers the authentication routes of §5 on loopback port 18100 (open question 11); the gate forwards those routes to it and asks it for the credential check of §10. On a secondary the same service keeps no store: it holds the primary's public key and the 60 s cache of §7, and forwards everything else to the primary.

Nothing is stored in clear except usernames, names and counters. Access tokens are JWTs and are not stored (they expire in 3600 s and are checked by signature plus the user's state).

**The signing key.** One RS256 key pair per primary (§5.2), created when the node becomes primary, never rotated in 0.9.9 (rotation is on the 3.0 list). The private key is a file in the state dir, mode `0600`.

**Identity** (owner 2026-11-19).
- Each user has a random stable user id. It goes in the user-id and account-id claims; the username goes in the email and name claims of the tokens Ragon's clients get (§5.3).
- The user's email, shown on the Account page, in `/whoami` and in exports, and put in the Forgejo client's id token, is the synthetic `<username>@<primary name>.local` (§5.3, open question 2). It is never mailed. A real email is a later option through a `/profile` command, not in 0.9.9.
- A rename keeps the user id, so keys, Night Shift tasks and Forgejo links follow it.

**Rules** (owner 2026-11-19).
- A username is 1-32 characters of `[A-Za-z0-9._-]`, unique on the primary regardless of case.
- A password is 0-256 characters. The empty password is hashed with argon2id like any other.
- Passwords and tokens appear in no log, no URL and no error text.
- No expiry of passwords, no lockout and no rate limit in 0.9.9 (the 3.0 list, §19).

**Roles** (owner 2026-11-19): user and admin, in 0.9.9 and 1.0. The first account created at the screen becomes admin; service accounts never count as the first. A guest role is a later idea and is not specified here.

**Sign-up is open** (owner 2026-11-19): anyone at the screen may create an account; an admin can remove it.

**Admin limits** (owner 2026-11-19). Admins may promote and demote each other. Demoting or removing the last admin is refused with "<name> is the last admin of <primary name>." `ragon-admin` on the primary can always promote, so the node owner cannot be locked out.

**Reset** (owner 2026-11-19). `ragon-admin node user reset <name>` and `/users reset <name>` print a random one-time password once. At the next sign-in the person must set a new password (empty allowed) before anything else; the reset revokes the person's refresh tokens.

**Audit** (owner 2026-11-19). One journald line per sign-in, account creation, reset, revoke, failure and pairing: username, event, client kind, result. Never a password, token or key. `ragon-admin node user log` reads it. Nothing leaves the node.

**Usage** (owner 2026-11-19). The primary counts requests and tokens per user per day and per client kind (`ragon-admin node user usage`; `/users` for admins, the user's own figures in `/whoami`). The session id appears only as a label in the day's log. Usage is never exported and is not part of the RBRB figures. Secondaries report their counts to the primary with the live check of §7.

**Unauthenticated clients** (owner 2026-11-19). Every gate counts the model requests it serves without credentials in 0.9.9, per day and per client kind and source address, and keeps the count beside the usage table (a secondary sends its count to the primary once a day). `ragon-admin node user usage --unauthenticated` prints the days; the figure tells the owner when version 1.0's refusal (§10) is safe.

**Client kind.** Codex's `originator` and `User-Agent` values are kept and `+ragon/<version>` is appended to the User-Agent by Ragon's build (owner 2026-11-19). The node's counters tell upstream clients (no suffix) from Ragon's build, and record the OAuth `client_id` of each grant (any value accepted, no registration in 0.9.9).
## 10. The gate check: 0.9.9 serves and counts, 1.0 refuses

Every model route (`/v1/responses`, `/v1/chat/completions`, `/v1/embeddings`, the Responses WebSocket) is checked the same way (owner 2026-11-19). The gate reads the credential before it rewrites the request head (`rewrite_head`, `dreamference/vllm_server/model_gate_service.py:378-408`) and adds `user=<name>` and the client kind to its request log line, beside the job header (`JOB_HEADER`, `:137`). Identifying the user in the log is the 1.0 goal.

**Carriers.** `Authorization: Bearer <token or key>` and the `api-key: <key>` header (owner 2026-11-19). A credential in the URL query is refused with the 401 shape and never logged.

**The check.** A JWT is verified locally (signature, `exp`, `iss`); then the session, and for a key the key, is looked up in the accounts service (§7), whose answer `(valid, user id, username, role)` is kept 60 s, keyed by the SHA-256 of the credential. On a secondary the lookup is the live call to the primary.

**Headers** (owner 2026-11-19). Everything Codex sends is accepted: `ChatGPT-Account-ID`, `OpenAI-Organization`, `OpenAI-Project`, `originator`, `session_id` and the other session headers. With a login token, `ChatGPT-Account-ID` must equal the token's account id and `OpenAI-Organization`, when sent, must equal the primary's node id; a mismatch is a 401. With an API key both are ignored, as the vendor ignores them for key auth. The node never requires a header that Codex does not send.

| Case | 0.9.9 (`[node] auth_enforce = false`, the default) | Version 1.0 (`auth_enforce = true`) |
|---|---|---|
| Valid token or key | served, `user=<name>` logged | served, `user=<name>` logged |
| No credential | served, logged as `user=-`, counted per day (§9) | 401, message "Sign in with ragon login." |
| Invalid, expired or revoked credential | 401, code `invalid_api_key` | 401, code `invalid_api_key` |
| Account header that does not match the token | 401, code `invalid_api_key` | 401, code `invalid_api_key` |
| Credential in the URL | 401, code `invalid_api_key`, the URL not logged | same |
| Secondary, primary unreachable, credential not cached | 401, message "authentication server unreachable" | same |
| Authentication route forwarded to an unreachable primary | 503, message "authentication server unreachable" | same |

A bad credential gets 401 already in 0.9.9: Codex refreshes after a 401 and recovers (`login/src/auth/manager.rs:1849-1862`), and serving a revoked token would break logout. Only a request with **no** credential, which is what a 0.9.8 client sends, is served and counted until version 1.0 (owner 2026-11-19).

**The error shape** (owner 2026-11-19) is the vendor's:

```json
{"error": {"message": "Incorrect API key provided.", "type": "invalid_request_error", "param": null, "code": "invalid_api_key"}}
```

with `WWW-Authenticate: Bearer realm="ragon"` on every 401. Other codes use the same body: `model_not_found` (404), `unknown_url` (404), and the engine's own errors unchanged.

**Open at version 1.0 too:** `GET /v1/models`, `/ragon/v1/info`, `/ragon/v1/ca.pem`, the JWKS and discovery documents and the health routes, so readiness checks need no key.

**CORS** (owner 2026-11-19). The gate answers preflight and adds `Access-Control-Allow-Origin` only for the node's own `ragon web` origins (`https://<node>.local:3100` and the names the node advertises) and the desktop app's origin; never a wildcard.

**The client in 0.9.9.** `ragon exec` never prompts, as Codex's `exec` never does: with nothing stored it sends no credential (`model-provider/src/auth.rs:205-221`) and the node serves and counts the request until version 1.0. Its unattended callers set `RAGON_API_KEY` (§13).

**Windows and Mac nodes** ship with the same check in 1.0, as secondaries (§7; owner 2026-11-19). `ragon-admin status` shows "auth check on" for every node, and a node that cannot run the check does not ship.

## 11. Commands

The launcher stops refusing `login` and `logout` (`ragon-rs/src/lib.rs:148-154`, `REMOVED_COMMANDS` and `ACCOUNT_REASON`, and the test that asserts the refusal). Codex's CLI commands are not patched (the login crate and the app-server are, §15): the launcher handles the forms and passes the rest to Codex.

| Command | Handled by | Does |
|---|---|---|
| `ragon login` | launcher | the two-choice prompt (§8.3), then the password grant or account creation; writes the entry with `codex_login::save_auth` |
| `ragon login --username <name> --password-stdin` | launcher | the password grant for scripts; the password from stdin (owner 2026-11-19); never on the command line |
| `ragon login --device-auth` | Codex | `codex login --device-auth --experimental_issuer https://<primary>.local:8000` (§5.5) |
| `ragon login --with-api-key` | Codex | the key from stdin into `OPENAI_API_KEY` of the entry (`cli/src/login.rs:204`) |
| `ragon login status` | launcher | "Signed in as <user> (<role>) at <node>", "Using API key as <user>", or "Not signed in. Run ragon login." with exit 1; the primary's name is added when it differs from the node; "(authentication server unreachable)" is appended when the live check fails (owner 2026-11-19) |
| `ragon login sessions` | launcher | the user's sessions (machine name, client kind, last use) and bindings; `--revoke <id>` revokes one |
| `ragon logout` | Codex | revokes this machine's refresh token at the primary (`login/src/auth/revoke.rs`), then deletes this primary's entry, the stored API key included; other primaries' entries stay (owner 2026-11-19) |
| `ragon logout --all` | launcher, then Codex | `DELETE /ragon/v1/users/me/sessions` revokes every session of the user on every machine, then the normal logout; API keys survive and are revoked by name |
| `ragon doctor` | launcher | adds the line "signed in as <username> (<role>) at <primary name>", or the not-signed-in line (owner 2026-11-19) |

**Slash commands** in the TUI (patch `0039`, with one dispatch arm into `ragon_launcher`, in the style of `0028-adapter-slash-command`); the desktop app and `ragon web` have the same actions in their account menu and Account page:
- `/logout`: Codex's, re-enabled by deleting patch `0005`.
- `/whoami`: user, role, node and primary, asked live; for a user, the day's usage.
- `/password`: the current password, then the new one twice; empty allowed.
- `/apikey`, `/apikey list`, `/apikey revoke <name>`: §13.
- `/sessions`: lists and revokes the user's sessions and bindings.
- `/users`: admins only (§12).
- `/delete-account`: §12.

**The account switcher** (owner 2026-11-19, question 9: one file). Codex's `auth.json` holds one entry and has no switcher. The owner's "one entry per primary node id" is met by one `~/.ragon/auth.json` that holds every primary's entry and marks one of them active, with a patch that teaches Codex's reader the layout:
- **Layout** (mode `0600`, written under a lock; the file replaces the single-entry file at first write, and a single-entry file is read as one entry whose key is the primary id in its org claim):

```json
{
  "version": 2,
  "active": "<primary node id>",
  "entries": {
    "<primary node id>": { "auth_mode": "...", "OPENAI_API_KEY": "...", "tokens": { }, "last_refresh": "..." },
    "<other primary node id>": { }
  }
}
```

  Each value is exactly the single entry Codex writes today (`AuthDotJson`), so one entry is the old file's content.
- **Patch `0041-ragon-auth-active-entry`** (about 2.4 KB, counted toward the cap of §15) changes Codex's reader and writer in `login/src/auth/storage.rs`: loading returns `entries[active]`; saving, `save_auth` included, replaces `entries[active]` and keeps the other entries and the marker; deleting (`logout`) removes `entries[active]` and moves the marker to none. A file with no `version` is the old layout and is read and written as before, so unpatched tooling keeps working on a one-entry file.
- At every start the launcher resolves the node's primary id (`/ragon/v1/info`) and, under a lock, sets `active` to it when an entry for it exists. With no entry for it, `active` names it anyway and the first-run sign-in screen fills it. The launcher changes `active` by editing the file; it never moves the other entries.
- `ragon login status --all` lists the entries by primary name, the active one marked.

## 12. Roles, admin commands and self-service

**Admin commands on the primary** (owner 2026-11-19). `ragon-admin node user` with the sub-commands below, in `dreamference/cli/dreamference_cli_controller.py` beside the `node` parser (the nested pattern of `power-config`):

| Command | Does |
|---|---|
| `list` | users with role, service flag, created, last use; `--sessions` adds every token and binding |
| `add <name>` | creates a user; the password is read from the terminal without echo (empty allowed) |
| `reset <name>` | prints a random one-time password once; the next sign-in must set a new one (§9) |
| `remove <name>` | removes the user, revokes the user's tokens, keys and bindings |
| `promote <name>` / `demote <name>` | role change; the last admin cannot be demoted (§9) |
| `key <name> [--name <key name>] [--revoke <key name>]` | mints an API key, prints it once with the snippets of §13 |
| `export` / `import <file>` | the JSON of §7 (users, roles, key hashes; never tokens) |
| `log` | the audit lines of §9 from journald |
| `usage [--days N]` | requests and tokens per user per day and per client kind |

`ragon-admin node service-key rotate` mints a new service key, rewrites the key file and revokes the old key (owner 2026-11-19).

**`/users` in ragon** (TUI, desktop app, `ragon web`) for admins: list, reset a password, remove, promote, demote, and every user's sessions. A user who is not admin gets "/users needs the admin role." The slash command calls `/ragon/v1/users*` on the gate with the signed-in token, so the same rules apply as for `ragon-admin`.

**Self-service, for every signed-in user** (owner 2026-11-19):
- `ragon login status` and `/whoami`: "Signed in as <user> (<role>) at <node>", "Using API key as <user>", or "Not signed in. Run ragon login." (§11).
- `/password` and the app's password form: the current password, then the new one twice; empty allowed after the "Continue without a password?" question.
- `/apikey`: mints, lists and revokes the user's own keys (§13).
- `/sessions` and `ragon login sessions`: the user's tokens (machine name, last use) and phone or chat bindings; revokes one.
- `/delete-account`: asks for the username as confirmation, removes the account, its tokens, keys and bindings, and the phone data stored under the user (§14). Night Shift tasks and Forgejo content stay under a "former user" label. The last admin cannot delete themselves.

**Chat commands** about the account are read-only: `/whoami` and `/sessions` (list). `/password`, `/apikey` and `/delete-account` exist in the terminal, the desktop app and `ragon web` only (owner 2026-11-19).

## 13. API keys and the service account

**Per-user keys** (owner 2026-11-19). An API key has the form `sk-ragon-<random>` (32 random bytes, base64url). It is minted by an admin (`ragon-admin node user key <name>`) or by the user (`/apikey`), shown once, stored as a SHA-256 hash, and revoked by name. A user may hold several keys. A key is kept apart from login tokens, as Codex keeps its API-key credential apart from its account tokens (`codex/codex-rs/login/src/auth/storage.rs:39-65`, the `OPENAI_API_KEY` field beside `tokens`). The gate treats a key and a token alike: same check, same rights, same error shape (§10).

**Where keys go.**
- In ragon: `ragon login --with-api-key` reads the key from stdin and stores it in the `OPENAI_API_KEY` field of the primary's `auth.json` entry, as Codex's own `--with-api-key` login does (`codex/codex-rs/login/src/auth/manager.rs:1022`, `login_with_api_key`). This is Codex's path, not a difference.
- In other tools (scripts, an upstream Codex, editors and agents that speak the vendor's API): the tool's API-key field, or the environment of §5.7.

**The snippets** (owner 2026-11-19). `ragon-admin node user key <name>` and `/apikey` end by printing:
1. the `config.toml` provider block for an upstream Codex:
   ```toml
   model_provider = "ragon"
   [model_providers.ragon]
   name = "Ragon"
   base_url = "https://<node>.local:8000/v1"
   wire_api = "responses"
   env_key = "RAGON_API_KEY"
   ```
   This block is for an upstream Codex, where `env_key` is Codex's own way to name a key variable. Ragon's build does not use it: its provider entry has no `env_key` and reads the stored entry (§15).
2. the export lines for SDK tools: `export OPENAI_BASE_URL=https://<node>.local:8000/v1` and `export OPENAI_API_KEY=sk-ragon-…` (or the `RAGON_*` pair);
3. the certificate line of §6: the path of the node's CA file and `export SSL_CERT_FILE=…`, `REQUESTS_CA_BUNDLE=…` and `NODE_EXTRA_CA_CERTS=…`.

**The service account** (owner 2026-11-19).
- Each node has one, `ragon-node-<short node id>` (the first 8 hex characters of the node id), created on the primary when the node is set up as primary or when it pins a primary.
- Role user, marked "service" in `list`, cannot sign in at the screen (the password grant refuses it with `invalid_grant`).
- Its key sits in a file under the node's state dir, mode `0600`, readable by the node's login user whose units run the nights.
- Readers: Night Shift (`dreamference/night_shift/night_shift_task_run.py:932`, `:1020`), the SWE-bench harness (`swe_bench_instance_run.py:1385`), the egress audit (`dreamference/audit/egress_audit.py:358`), the Codex test runner (`codex_test_runner.py:91`, `:531`; `codex_runner.py:78`) and the first-run check (`first_run.py:189`). Each sets `RAGON_API_KEY` from the file for the `ragon` it starts.
- Bots are service accounts only; there are no human-like bot users (owner 2026-11-19).

## 14. Bindings: phones and chats, Forgejo, Night Shift, local tools

### 14.1 Phones and chat accounts

A phone or chat identity binds to an **existing** node user (owner 2026-11-19, refining the earlier "one user per chat" line). There are no per-chat users and no phone-only users in 0.9.9.

**Pairing** runs in ragon on any signed-in machine:
- `ragon signal setup` and `ragon chat pair <telegram|discord|matrix>` use the signed-in username and ask for the password again (the same rules as the screen).
- The pairing step also adds the primary's CA to the user's trust store on that machine, as the installers do (§6), so the Account page and the login page open without a warning.
- Signal: the QR code appears there. Each user links their own Signal account as a device; the bridge runs one linked device per user (`ragon-rs/signal/src/cli.rs:1077`, `:1090` today link one device). Note to Self per linked account stays.
- Telegram, Discord, Matrix: ragon shows a 6-character pairing code, valid 60 minutes, consumed by the first use. The user sends it to the bot from the phone; the bot binds that chat id (or Matrix user id) to the node user.
- The primary records the binding (§9 `bindings`); the node's bridge daemon fetches it.
- The bridge daemon holds an API key minted for it per binding, in its own `0600` store (`/var/lib/ragon-signal*`, `$RAGON_HOME/chat/`; `ragon-rs/chat/src/store.rs:3-11`). The key is revoked when the binding is removed.
- The phone shows one confirmation, "Paired to <username> on <node name>", then chats look as today. `/whoami` in the chat answers the same line.

**Rules.**
- One chat identity maps to exactly one node user; re-pairing moves it. A user may bind any number of phones and chats. `/sessions` lists bindings beside machine tokens and revokes one.
- An unbound chat gets exactly one reply explaining how to pair, at most once per day per chat, and silence otherwise; a wrong code gets the same reply. The audit counts the attempts. Limits are on the 3.0 list.
- Groups: each message runs as its author's node user; unpaired members get one pairing hint per member per day; one shared thread per group as today. Discord stays DM-only (owner Q101).
- Removing the user, or revoking a Signal binding in `/sessions`, makes the bridge unlink its device from that Signal account and delete its local Signal state at once.
- With the primary unreachable and the binding's key not cached, the bridge replies "authentication server unreachable, try again" (§7).
- Phone data (uploads, voice notes, transcripts) is stored under the bound user's id and deleted with the account.
- Nights from a phone: `/night add` through a bridge carries the bound user; `/night report` in the chat shows that user's tasks.

**What changes in the bridges.** Today a bridge reaches the model through `ragon web` `/ws` with a device cookie traded for an 8-digit `ragon web pair` code (`ragon-rs/signal/src/agent.rs:2-28`, `ragon-rs/chat/src/agent.rs:146-151`). In 0.9.9 the bridge sends the binding's key as Bearer and `ragon web` runs the request as the bound user. `docs/dev/messengers.md` and `scratch/discord-phase0/run-phase0.sh` follow.

**At the upgrade** every existing pairing is dropped, the owner's Signal link included; every phone pairs again. The release notes, the upgrade screen (§8) and the first-admin line say so.

### 14.2 Forgejo

Forgejo uses the gate as its OAuth2 provider in 0.9.9 (owner 2026-11-19). Today each node's Forgejo (`ragon-forge`, `dreamference/forge/forge_settings.py:21`) has no OAuth source, and `ragon web` signs in to it as `owner` with a password (`docs/dev/forge.md:23`).

- `ragon-admin` registers one OAuth client for each node's Forgejo on the primary (client id and secret in the node's state dir) and adds it as an authentication source with `forgejo admin auth add-oauth --provider openidConnect`, pointed at the gate's discovery document (§5.1).
- Forgejo reads that discovery document from inside its container, so the container must trust the CA that signs the gate (§5.1): the issuer is kept as `oauth_trust_issuer` in the forge's state and the forge restarted with that CA mounted at `/etc/ragon-ca/ca.pem` (`SSL_CERT_DIR`) and the issuer's name added to its hosts before `add-oauth` runs; never a skipped verification. A refused `add-oauth` is retried three times, printed with Forgejo's reason as an error, and retried at every ten-minute tick until it is taken (`dreamference/forge/forge_oauth_client.py`).
- In 0.9.9 only the primary's Forgejo takes the source; a secondary's Forgejo keeps its local sign-in until 0.9.10 (owner 2026-11-25).
- The gate offers the authorization-code grant to that registered client only. `/oauth/authorize` redirects to the login page on the primary's `ragon web` (§14.4); after sign-in the page redirects back to Forgejo with the code.
- Forgejo accounts are linked by username at the first OAuth sign-in; a user without one gets a Forgejo account then. Forgejo's local login stays on for `forge-admin`, `owner` and `mirror` (`dreamference/forge/forge_accounts.py:39-63`).

### 14.3 Night Shift

`/night add` records the signed-in username (and user id) on the task; today the task has no user field (`ragon-rs/src/night.rs:366-383`). The runner runs under the node's service account. The report names the user. `/night report` lists the caller's tasks, every task for admins (owner 2026-11-19).

### 14.4 ragon web, the browser login page and the Account page

- `ragon web` signs in with the same `/oauth/token` password grant and keeps the JWT in the browser's local storage until logout (owner 2026-11-19). There are no separate web accounts. Its server hands the tokens to its app-server through Codex's external-token login, `chatgptAuthTokens`, and answers the app-server's refresh request by refreshing at the primary (§5.8).
- `ragon web`'s `/ws` and `POST /api/transcribe` (voice) take the JWT as Bearer, beside the owner token and device cookie (open question 5). The owner token and device cookies of `ragon-rs/web/src/auth.rs:42-44` remain for the node's own owner path.
- One login page on the primary's `ragon web` serves the device-code verification (`/device`, §5.5) and Forgejo's authorization-code flow. It shows the same two choices as the terminal screen.
- The Account page: identity, sessions, API keys with the snippets, password change, delete account, phone pairing codes, and the usage view (admins every user, users their own). Same functions as the terminal commands.

### 14.5 Local tools

The IDE MCP server (`dreamference/mcp_server/mcp_server.py`), the context engine and the code index (`ragon-code-rs/`, which reaches the gate through `vllm_host`) read `~/.ragon/auth.json` like ragon and send the entry's token or key. They have no screen of their own; with no entry they print "Not signed in. Run ragon login." The desktop app and the terminal share the same `auth.json` (owner 2026-11-19).
## 15. The launcher and the Codex patches

**The provider entry replaces `--oss`.** Today the launcher prepends `--oss --local-provider openai-custom` to every Codex run (`ragon-rs/src/lib.rs:14`, the `PROVIDER` constant at `:106`, the injection at `:1653-1682`). In 0.9.9 it writes a provider entry instead into `~/.ragon/config.toml` at every start (§5.1), `chatgpt_base_url` as a top-level key beside it:

```toml
model_provider = "ragon"
chatgpt_base_url = "https://<node>.local:8000/backend-api/"

[model_providers.ragon]
name = "<node name>"
base_url = "https://<node>.local:8000/v1"
wire_api = "responses"
requires_openai_auth = true
```

- `requires_openai_auth = true` turns on Codex's account mode, the first-run sign-in step and the `Authorization` header from `auth.json` (§4). The entry has no `env_key`: the stored entry, or `CODEX_API_KEY`, supplies the bearer.
- `share_scope.rs` sets its overrides under `model_providers.openai-custom.*` (`ragon-rs/src/share_scope.rs:142-146`); they move to `model_providers.ragon.*` in the same row.
- The `<node>` is the node the launcher resolves today. A client whose node has no gate uses the gate of the node's pinned primary (§7).

**The environment the launcher sets** for every Codex process it starts (§5.7):
- `CODEX_APP_SERVER_LOGIN_ISSUER`, `CODEX_REFRESH_TOKEN_URL_OVERRIDE` and `CODEX_REVOKE_TOKEN_URL_OVERRIDE`, all pointing at the resolved node's gate (§5.1; patch `0040` also reads the issuer from `config.toml`);
- `CODEX_CA_CERTIFICATE`, the pinned CA of that node (§6);
- `CODEX_API_KEY` from `RAGON_API_KEY`, else from `OPENAI_API_KEY`; `RAGON_API_KEY` wins when both are set;
- the User-Agent suffix `+ragon/<version>` through `USER_AGENT_SUFFIX` (`login/src/auth/default_client.rs:41`).

**The auth module.** A new `ragon-rs/src/auth/` holds the screen of §8, the commands of §11, the switcher, the CA pinning store and the translations of §8.5. It writes `auth.json` only through Codex's public `codex_login::save_auth` (`login/src/lib.rs:72`) and makes a running session reread it with `AuthManager::reload` (`login/src/auth/manager.rs:2481`). Codex's login crate is patched in two places only, patches `0040` and `0041` (§5.1, §11); the launcher still writes entries only through `save_auth`, and it edits the `active` marker of §11 directly.

**The CA pinning store.** `~/.ragon/nodes/<node id>/ca.pem`, written at first contact (§6) and at pairing (§14.1). A later certificate from another CA under the same node id stops the launcher with the message of §6.

**Windows.** Codex sets mode `0600` on Unix only (`login/src/auth/storage.rs:214-217`). On Windows the launcher sets an access list on `auth.json` and the pinning store that grants the current user alone. The file store stays the default on every system (`config/src/types.rs:116-118`).

**The patches.**

| Patch | Contents | Size |
|---|---|---|
| `0005-hide-vendor-login` | **Deleted.** Its two hunks hide `login` and `logout` (`cli/src/main.rs`) and turn off `/logout` (`tui/src/slash_command.rs`). `REMOVED_COMMANDS` and `ACCOUNT_REASON` in the launcher (`ragon-rs/src/lib.rs:148-154`) lose `login` and `logout` in the same row | −1,054 bytes |
| `0038-ragon-sign-in-step` | Routes the onboarding sign-in step (`tui/src/lib.rs:2296-2304`; the widget in `tui/src/onboarding/auth.rs`) to the screen in `ragon-rs`, which draws it and returns once `auth.json` is written | about 1.8 KB |
| `0039-ragon-account-commands` | The slash commands `/whoami`, `/password`, `/apikey`, `/sessions`, `/users` and `/delete-account` in `tui/src/slash_command.rs`, with one dispatch arm into `ragon-rs`, in the style of `0028-adapter-slash-command`; and the status line's Account item (`tui/src/bottom_pane/status_line_setup.rs:56`, beside `Hostname` at `:79`) | about 3.0 KB |
| `0040-ragon-login-issuer` | The login crate takes the issuer, the refresh URL and the revoke URL from one function: the environment, else `ragon_issuer` in `config.toml`, else the constant (`login/src/server.rs:76`, `login/src/auth/manager.rs:212-215`) | about 0.7 KB |
| `0041-ragon-auth-active-entry` | `auth.json` holds every primary's entry with an `active` marker; the reader and writer in `login/src/auth/storage.rs` use the active entry (§11) | about 2.4 KB |
| `0042-ragon-account-signin-rpc` | The app-server request `account/ragon/signIn` and its handler, with one call into `ragon_launcher`; the schema files regenerated (§5.8) | about 3.6 KB |

The one app-server protocol change is the new request of patch `0042`; the generated schema files under `app-server-protocol/schema/` are regenerated in its row and nowhere else.

**The size cap.** `tests/test_codex_branded_builder.py:322` holds the series in `codex-patches/` under 64,000 bytes; it stands at 63,875. With the table above the series comes to about 74,300 bytes (the 0.7, 2.4 and 3.6 KB of patches `0040` to `0042` added to 67,600 bytes). **The cap rises from 64,000 to 76,000 bytes** in the first row that lands a patch of the series (patch `0040`, row `auth-l2-login-patches`; the brief's default, "raise to what rows need"). Each patch row records its measured size in its report.

## 16. Compatibility

**Egress.** Every authentication and model call goes to the resolved node's gate on port 8000, which the audit already allows (`dreamference/audit/egress_audit.py:202-224`), or to the gate of the pinned primary when the resolved node has none. The allowlist gains exactly that second address, admitted only when the certificate it presents chains to the pinned CA. The audit:
- passes the node's service key as `RAGON_API_KEY`, since its throwaway home has no `auth.json`;
- gains a trace of `ragon login`, `login status`, a token refresh and `logout`, showing that each reaches only those addresses, the revoke included.

**Air-gapped.** `/airgapped` blocks the internet, not the LAN, so sign-in works with it on. The remote plugin calls of §5.9 go to the node's own plugin index, and with `/airgapped` on they are not made at all; the string patch `0030` shows for them is kept.

**Windows and Mac clients** ship in 0.9.9 from the same launcher code. Codex's file store behaves the same there (`login/src/auth/storage.rs:206-222`); the access list of §15 replaces the Unix file mode on Windows; the trust-store import uses each system's own user store (§6).

**Older clients.** A 0.9.8 client sends no bearer. It is served and counted until version 1.0 (§10). Its first run after the upgrade shows the upgrade note of §8.1.

**Upstream clients.** An unmodified Codex, or any client of the vendor's model API, reaches the gate with a per-user key and the configuration of §13. It is counted as an upstream client because its User-Agent has no `+ragon/` suffix (§9).

**Shared workstations.** `auth.json` lives in each Unix user's own `~/.ragon`, so two people on one machine sign in separately; nothing is shared between home directories.

## 17. Delivery: the lane graph

The plan is `backlog/plan-auth-1.md`. Its rows form five lanes that run in parallel, followed by a short serial tail. Rows depend on each other within a lane, and across lanes only where one needs code another lands.

| Lane | Rows (in order) | Needs from other lanes |
|---|---|---|
| L1 store and OAuth | store, roles, admin commands, service accounts; the OAuth2 and JWT service (`/oauth/token` password and refresh grants, revoke, JWKS, `/ragon/v1/users`); device auth in both shapes and the authorization-code grant; the bindings and usage API | none |
| L2 launcher and Codex patches | provider entry, issuer, environment, pinning and switcher; the sign-in screen with patch `0038`, the deletion of `0005` and the cap; the CLI commands; patch `0039` with the status line; the device-auth client and the desktop app's form; translations | the screen needs the OAuth service (L1) and TLS (L3); the device client needs device auth (L1) |
| L3 gate | TLS with the node CA and pinning; the credential check (Bearer and `api-key`, JWT verify, 60 s cache, error shapes, serve-and-count); the compatibility surface (`/v1/models` aliases, headers, usage stub, parameter stripping, CORS); a test with the vendor's SDK against a mocked engine | the check needs the OAuth service (L1) |
| L4 ragon web | the login page (device verification and Forgejo authorization, the JWT in local storage); the Account page | device auth (L1); the bindings API (L1) |
| L5 bindings | phone pairing through the bridges; Forgejo as an OAuth2 client; Night Shift's user; IDE, MCP and the code index reading `auth.json` | the bindings API (L1), the CLI (L2), the login page (L4) |

**The serial tail:**
1. secondaries forwarding to the primary, the primary question at `ragon-admin init`, and the migration command (after L1 and L3);
2. the egress allowlist and trace (after L3);
3. `docs/accounts.md`, Getting Started and the developer docs (after L2);
4. the upgrade note (after L2);
5. a nightly compatibility run against the live gate (after L3);
6. **the end-to-end gate:** a test primary and a test secondary, every client, the Discord bridge included; a red run holds the release; held for the coordinator;
7. **the fleet candidate, 2026-11-28:** after the end-to-end gate, install on every node of the fleet, one primary and the rest secondaries, run the migration command, and repeat the end-to-end run on the real fleet by 2026-11-30; held for the coordinator.

**Sizing.** The plan is sized for four rows in flight. The critical path runs store → OAuth service → sign-in screen → patch `0039` → translations → docs → end-to-end gate, 30.5 hours of row time (the plan's header recomputes it from the rows' estimates). The other rows fit beside it at four in flight.

**Calendar.**
- 2026-11-20 to 2026-11-24: the five lanes and the first five tail rows.
- By 2026-11-26: the end-to-end gate green on the test pair, two days before the fleet date.
- 2026-11-28: the fleet candidate is installed.
- By 2026-11-30: the end-to-end run is green on the real fleet.
- 2026-12-01, 09:00 UTC: release 0.9.9 ships. Nothing slips.

**Verification of the first slice.** Jobs verify the first rows with test users on the test primary. There is no owner trial row; the row that lands the screen writes a trial script for the owner that points at the test primary, and says so in its report.

## 18. Tests

- **Store.** Username rules and case-insensitive uniqueness. The empty password signs in. The first account that is not a service account becomes admin. A reset prints a one-time password and revokes the person's refresh tokens. Removing a user revokes everything. The last admin cannot be demoted or removed.
- **OAuth routes.**
  - The password, refresh and authorization-code grants, with the JSON refresh body Codex sends (§4) and the form body of the standard.
  - Revoke with Codex's body.
  - Every claim of §5.3 in the id token; the signature checks against the JWKS.
  - Device auth in both shapes (§5.5), each driven by its own client: Codex's `device_code_auth.rs` unmodified, and a client of the standard's form.
  - A password never appears in the gate's, the service's or the journal's output (a log scan inside the test).
- **Gate.** Each row of §10's table, in 0.9.9's mode and in version 1.0's; the error bodies and headers byte for byte; a secondary's 60 s cache; the primary unreachable; the unauthenticated counter.
- **Compatibility.** The vendor's Python SDK, unmodified, against the gate with a per-user key and a mocked engine: a model list, a response, a stream, an error. An upstream Codex with the configuration of §13 runs one turn.
- **Launcher.**
  - `login` and `logout` are no longer refused.
  - The screen keeps an empty, untrimmed password and writes `auth.json` with Codex's fields.
  - `logout --all` revokes every session and keeps API keys.
  - The switcher keeps one file per primary.
  - A changed CA stops the launcher.
- **Patches.** The series applies. `cargo test` for the touched Codex crates runs in the build export, never in `codex/`. The size cap of §15 holds.
- **Egress.** The login trace of §16 stays on the allowed addresses.
- **Live.** A nightly run of the compatibility tests against a real gate, and the end-to-end gate of §17.

## 19. Out of scope, and the 3.0 list

Not in 0.9.9 or version 1.0:
- sign-in with an outside identity provider;
- a guest role;
- real email addresses, password policy beyond "may be empty", account recovery beyond the admin's reset;
- a keyring store as the default (Codex's keyring mode stays available).

The hardening items are on the 3.0 list in [SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md) (owner addendum, 2026-11-19):
- rotation of the JWT signing key;
- rotation of refresh tokens on use;
- rate limits on the token route, account creation, the device and pairing routes and the secondaries' checks;
- lockout after repeated wrong passwords;
- guessing limits for pairing codes and device user codes;
- registration of OAuth `client_id` values;
- certificate rotation and a PKI beyond each node's own CA.

## 20. Decisions

The owner answered the thirteen questions on 2026-11-19. Nine confirm the default the rows were built on; four (1, 4, 9, 13) changed it, and their sections carry the change. None is open.

1. **How the issuer reaches Codex.** **Decided (2026-11-19), differs from the default:** a patch to the login crate, `0040-ragon-login-issuer`, reads the issuer from `config.toml`; the environment variables and `chatgpt_base_url` stay as well (§5.1, §15).
2. **The email claim.** **Decided (2026-11-19), the default:** the username for Ragon's clients; `<username>@<primary name>.local` in the Forgejo client's id token and on the Account page (§5.3).
3. **Device-auth shapes.** **Decided (2026-11-19), the default:** both Codex's paths and the standard's, served from one table of codes (§5.5).
4. **The desktop app's sign-in.** **Decided (2026-11-19), differs from the default:** a new app-server request `account/ragon/signIn` (patch `0042`); the form calls it and the app-server is not restarted (§5.8).
5. **WebSockets and `ragon web`'s other routes.** **Decided (2026-11-19), the default:** the same Bearer check at the upgrade of every WebSocket and on `/ws` and `/api/transcribe` (§5.9, §14.4).
6. **Plain HTTP in 0.9.9.** **Decided (2026-11-19), the default:** served only to requests with no credentials, so that 0.9.8 clients keep working; a plain request that carries a credential, or goes to the sign-in routes, is refused with `403` "Use https://" (§6).
7. **The order of the picker's lines.** **Decided (2026-11-19), the default:** Codex's order, the two changed labels in the places of the lines they replace (§8.2).
8. **Translations.** **Decided (2026-11-19), the default:** a table in `ragon-rs`, the language from `LC_ALL`, `LC_MESSAGES` and `LANG`, English when none matches (§8.5).
9. **The account switcher.** **Decided (2026-11-19), differs from the default:** one `auth.json` holds every primary's entry with an `active` marker; Codex's reader and writer use the active entry (patch `0041`; layout in §11).
10. **A secondary's service account.** **Decided (2026-11-19), the default:** the secondary registers it on the primary through `/ragon/v1/nodes` when it pins the primary, keyed by its node id, with no admin step; the admin sees it marked "service" and may remove it (§13).
11. **The accounts service port.** **Decided (2026-11-19), the default:** 18100 on loopback, a constant in `dreamference/config/ports.py`; the gate forwards the authentication routes to it (§9).
12. **What `--all` revokes.** **Decided (2026-11-19), the default:** every session of the user on every machine; API keys stay, and are revoked by name (§11).
13. **Windows and Mac nodes at version 1.0.** **Decided (2026-11-19), differs from the default (an option the owner added):** both ship with the auth gate in 1.0 and no ungated node ships; the requirement is in their specs (§7).

## 21. Changes to other specs

These are made when the rows land, not by this document:
- **[RAGON_NODE](./DREAMFERENCE_RAGON_NODE.md):** decision 7 ("nodes have no roles") is superseded by the primary and secondary roles (§7); decision 4 ("no authentication") gains a pointer here; `ragon-admin init` gains the primary question; the TXT record gains `auth=primary`; the gate serves HTTPS (§6).
- **[RAGON_ASK](./DREAMFERENCE_RAGON_ASK.md) §4.5:** "no account system" becomes the signed-in user of `~/.ragon/auth.json`, and `ragon web`'s login page (§14.4).
- **[RAGON_CODEX](./DREAMFERENCE_RAGON_CODEX.md):** `login` and `logout` leave the removed commands; `0005` leaves the patch list, `0038` to `0042` join it; `--oss` gives way to the provider entry (§15).
- **[SECURITY_REVIEW](./DREAMFERENCE_SECURITY_REVIEW.md):** item 9 (the model server on port 8000) gains "served and counted in 0.9.9, refused at version 1.0"; the 3.0 list follows §19 (changed with this revision).
- **[WINDOWS_NODE](./DREAMFERENCE_WINDOWS_NODE.md) §10 and [MAC_NODE](./DREAMFERENCE_MAC_NODE.md) §9:** the 1.0 requirements gain the auth gate (§7; done with this revision).
- **[README](./README.md), "Accepted by design":** the row on the model server with no API key changes at version 1.0.

## Sources

- The owner's words, addenda and rulings of 2026-11-19, kept in the coordinator's decisions file.
- Codex at the vendored tree `codex/codex-rs/`, at the paths cited.
- The model gate, the launcher, the node code and `ragon web` at the paths cited in this repository.
