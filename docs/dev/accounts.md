# Accounts: the service, the launcher half, the patches and the cap

The specification is `specs/DREAMFERENCE_AUTH.md`; the user guide is [accounts](../accounts.md). This note says where the pieces are and what to keep when touching them.

## The accounts service (Python, `dreamference/accounts/`)

The primary node runs it inside the model gate; it keeps everything in one SQLite file under the node's state directory (mode `0600`).

| File | Holds |
|---|---|
| `accounts_store.py` | users, argon2id password hashes, refresh tokens and API keys as SHA-256 only, the audit lines (never a password, token or key) |
| `accounts_service.py`, `accounts_oauth.py`, `accounts_jwt.py`, `accounts_auth_code.py` | `/oauth/token` grants, the vendor-shaped tokens and claims, authorization codes |
| `accounts_device.py`, `accounts_device_entry.py` | the device authorization flow and its entry page |
| `accounts_admin.py`, `accounts_migrate.py` | `ragon-admin node user …`, `node primary`, `node migrate` |
| `accounts_secondary.py` | a secondary's pinned primary, public key and 60 s answer cache; it keeps no accounts |
| `accounts_bindings.py` | phone and chat pairings |
| `accounts_usage.py` | usage per user, day and client kind, and the unauthenticated counts |
| `accounts_forgejo_client.py`, `forge_oauth_client.py`, `forge_accounts.py` | Forgejo sign-in through the primary |

`accounts_error.py` fixes the error shapes: `/oauth/*` uses RFC 6749's, `/ragon/v1/*` the vendor's. The node's service account and its key file live in `dreamference/node/service_key.py`.

## The launcher half (Rust, `ragon-rs/src/auth/`)

`mod.rs::start` runs at every start: it asks the gate who it is (`/ragon/v1/info`), pins the node's CA at first contact (`pin.rs`), marks the primary's entry active (`switcher.rs`) and sets the environment every Codex process inherits. Against a node with no gate it does nothing. `screen.rs` is the sign-in screen, `commands.rs` the `login`, `logout` and `sessions` forms, `slash.rs` the account slash commands, `signin_rpc.rs` the app-server request, `strings.rs` the five-language table. It writes `auth.json` only through Codex's `save_auth`; the `active` marker is the one thing it edits directly.

## The first account (AUTH §8.6)

`GET /ragon/v1/setup` says whether the node still has no account and whether the caller is on the node. The gate (`model_gate_service.py`, `mark_local` and `on_this_machine`) strips a client's `X-Ragon-Local` and adds it only for a loopback peer or one from the address it reached; the service (`accounts_oauth.py`, `first_account_refused`) refuses the first account without it. The first account runs `AccountsMigrate.migrate` once (`accounts_service.py::first_account`, and `accounts_admin.py::_first_account` for `node user add`). `AccountsService.ensure_running` installs the `ragon-accounts.service` user unit from `ModelGate.start`. In the launcher, `auth::start(gate, home, interactive)` reads the setup document only when `interactive`, and `first_run` decides: the create form (`screen::Node.first`), the plain line for another machine's node, or the usual screen. Unattended callers must stay outside that branch; `tests/auth_old_gate.rs` holds the test.

## The patches and the cap

`0005-hide-vendor-login` is deleted; `0038` to `0042` are the sign-in step, the account commands and status item, the login issuer, the active entry in `auth.json` and the `account/ragon/signIn` request. Their sizes, and the cap's history up to 115,500 bytes, are in [codex-build.md](codex-build.md). The cap is `test_the_patches_stay_small` in `tests/test_codex_branded_builder.py`; raise it only explicitly, in the row that needs it, and record the measured size in that row's report. The generated app-server schema files are regenerated in `0042`'s row and nowhere else.

## Keep when touching this

- A struct the dependants build (the web crate's `Config`) needs every initializer found by `rg -n "Config \{" ragon-rs --type rust` before a change is done.
- A password never goes on a command line, into a log or an audit line.
- A secondary stores no accounts; do not add a table to it.
- The gate check of AUTH §10 (served and counted in 0.9.9, refused at 1.0) has its own tests; a change to the gate runs them.
