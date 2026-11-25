# plan-auth-followups: the non-blocking follow-up rows of the 0.9.9-rc.2 auth reviews

Every row here is non-blocking for 0.9.9. Each row comes from one report in scratch/reports; a row that two reports raise is listed once, under its first source. Release 0.9.10 or 1.0 for a row that fixes a defect or a gap; release 3.0 for a security-hardening row that is not a defect. Size: S (under an hour of work), M (a half day), L (more).

Source short names: `auth` = w48s-auth-secondary-keys.md, `auth-rev` = review-auth-secondary-keys.md, `disc` = w48s-client-discovery-https.md, `disc-rev` = review-client-discovery-https.md, `forge` = w48s-forge-oauth-register.md, `forge-rev` = review-forge-oauth-register.md, `rc1` = w48s-clients-rc1.md.

## Secondary nodes and their accounts

| id | what | source | size | release |
|---|---|---|---|---|
| auth-migrate-follow-role | `migrate` on a node that was pinned does not rewrite its secondary service unit; call `follow_role()` after `advertise()`. Test: migrate on a pinned node rewrites the unit | auth F1, auth-rev F1 (accounts/accounts_migrate.py) | S | 0.9.10 |
| auth-live-check-5xx | the secondary's live check caches any answer for 60 s, including a 5xx from the primary; cache only 200 and 401/403 | auth F4, auth-rev F4 (accounts/accounts_secondary.py) | S | 0.9.10 |
| auth-follow-role-stopped-unit | `follow_role` returns true for a stopped unit, and a failed try-restart prints nothing; report the unit's real state and print "accounts service did not restart: run server start" | auth F5, auth-rev F5 | S | 0.9.10 |
| auth-pinned-upgrade-hint | a pinned secondary that was upgraded fails closed until it is re-pinned; `node primary` should say "re-run node primary <uuid>", and the release notes line is already in rc.2 | auth F7, auth-rev F7 | S | 0.9.10 |
| auth-jwks-refetch | the secondary fetches the primary's keys once per process; after a key rotation valid tokens fail until restart; re-fetch after 60 s or on an unknown key id | auth F9, auth-rev F9 (accounts/accounts_secondary.py) | S | 0.9.10 |
| auth-gate-old-mode-cache | the gate keeps old-mode answers up to 60 s after a role change; B5 closes most of it; add a clear on a node.json change only if still wanted | auth F6, auth-rev F6 | S | 1.0 |
| auth-nits | `fetch_ca` reads the CA file with no size cap (cap it at 64 KiB); `choose()` has no own-node check; `node_info` reports `primary_id` null on a secondary, not the pinned id | auth F10, auth-rev F10 | S | 0.9.10 |
| auth-secondary-service-account | decision 10 is not built: a secondary does not register its service account when it pins; the pair works only because migrate registered every advertised node | auth "Other rows", auth-rev F10 (accounts/accounts_migrate.py `pin`) | M | 0.9.10 |
| auth-spec-unreachable-wording | **done 2026-11-25** (owner ruling: 401, as §10 and the code say; §7 edited). Was: spec §7 says an unreachable primary with nothing cached "serves and counts in 0.9.9"; §10 and the code say 401. Owner ruling first, then edit §7 (specs/DREAMFERENCE_AUTH.md) | auth "Other rows", auth-rev F10 | S | 0.9.10 |
| auth-secondary-crash-loop | a secondary service that starts while its primary is not advertised exits and restarts in a loop; the gate then answers 401 to every uncached request (accounts/accounts_service.py `build_secondary`) | auth "Other rows" (pre-existing, low) | S | 0.9.10 |
| auth-stray-accounts-db | a node that was pinned keeps an empty accounts.db from its primary-mode start; harmless, remove by hand if wanted | auth "Other rows" | S | 1.0 |
| auth-one-node-addresses | one node id may appear at several addresses when host and port match; the author allows it on purpose; owner ruling on refusing address differences | auth-rev round 2, R2-1 | S | 1.0 |
| auth-e2e-whoami | the test scripts call `ragon whoami`, which is not a CLI command; use `ragon login status` (scripts/auth_e2e.sh, scripts/auth_e2e_node.py) | auth "Other rows" (test script) | S | 0.9.10 |
| auth-e2e-e13-user | test step E13 revokes user `e2e` on the secondary; it should revoke the run's user on the primary (scripts/auth_e2e.sh) | auth "Other rows" (test script) | S | 0.9.10 |

## Client discovery and the HTTPS address

| id | what | source | size | release |
|---|---|---|---|---|
| client-overlay-403-advice | on the overlay route the 403 message tells the user to remove vllm_host or check the `.local` name, which cannot help there; add an overlay variant (ragon-rs/src/auth/commands.rs, auth/mod.rs) | disc R-F1, disc-rev F1 | S | 0.9.10 |
| client-first-contact-ca-scope | first contact pins a node's CA and imports it into the user's browser trust store; the CA is not limited to the node's names, so a LAN impostor could be trusted for any site. Owner decision: limit the CA to the node's names, or do not import at discovery (ragon-rs/src/auth/mod.rs `secure_gate`, pin.rs:129) | disc R-F2, disc-rev F2 | M | 0.9.10 |
| client-bearer-plain-routes | the bearer may go out on a plain-HTTP route; the check was not completed. Verify it first; if it does, send the bearer only over https or loopback (auth/mod.rs `codex_env`, auth/local.rs:35, delegate.rs:8, code_index.rs:199) | disc R-F3, disc-rev F3 | M | 0.9.10 |
| client-version-checkout | `dreamference.__version__` on a checkout can report an installed release's version, and the `_SOURCE_VERSION` literal can drift from setup.py; use metadata only when it is this file, and test the literal (dreamference/__init__.py) | disc R-F4, disc-rev F4 | S | 0.9.10 |
| client-tls-listener-tests | no tests for the HTTPS discovery success path, a changed CA, or a node that answers TLS but not by its name; add a local TLS listener with a test CA (ragon-rs/tests/auth_https_discovery.rs) | disc R-F5, disc-rev F5 | M | 0.9.10 |
| client-pick-https-wording | `ragon node use https://...` is refused with "a node's model port speaks http", which is false for a 0.9.9 gate; reword and fix the test that pins it (ragon-rs/src/node.rs:863, test at node.rs:1671) | disc "Other gaps" | S | 0.9.10 |
| client-vllm-host-plain | a configured vllm_host that names an http address is used as it is | disc "Other gaps" | S | 1.0 |
| client-advert-scheme | the node advertisement has no scheme record, so node-locator and `ragon web` still build http from it; needs a scheme record in the advertisement (a contract change) | disc "Other gaps" (dreamference/node/node_service_file.py:78) | L | 1.0 |

## Clients and installer (Mac and Windows test run on rc.1)

| id | what | source | size | release |
|---|---|---|---|---|
| install-asset-timeout | install.sh downloads have no timeout; a stalled download hung for more than ten minutes; add connect and speed limits so a stall fails with a named error (install.sh:385, :342) | rc1 F1 | S | 0.9.10 |
| update-check-flag | `ragon update` has no `--check`; the release cannot be read as "newer" without installing; add `--check` that prints current, latest and the asset, exit 0 or 1 (ragon-rs/src/update.rs:419) | rc1 F3, disc "Rows F1, F3, F5" | S | 0.9.10 |
| gui-check-owner-step | the About box and the sign-in form cannot be checked over ssh; owner step at the desktop, with the recipe in disc | rc1 F5, disc "Rows F1, F3, F5" | S | 0.9.10 |

## Forgejo sign-in (git-hosting account)

| id | what | source | size | release |
|---|---|---|---|---|
| forge-oauth-secondary-register | a secondary's Forgejo gets its Ragon client only from one admin step on the primary (`ragon-admin forge register <node>`, or approval when the node is pinned); there is no service-key path. Until then the release note says secondary forges keep local sign-in. Tests: tests/test_forge_oauth_client.py | forge F1, forge-rev F1, forge "Follow-up rows" | M | 0.9.10 |
| forge-oauth-live-check | check on a live pair: with the primary down, Forgejo starts and shows local sign-in; the first add-oauth works before the host entry and CA are mounted; the E9 row of scripts/auth_e2e.sh | forge-rev F5, forge "Follow-up rows" | S | 0.9.10 |
| forge-ensure-errors | `ensure_oauth` catches only OSError, RuntimeError and ValueError; anything else from the node id or the settings stops the tick before the timers start (dreamference/forge/forge_service.py:442) | forge-rev F5 | S | 1.0 |
| forge-docker-timeout | a hung add-oauth returns code 127 with no traceback when the container command times out (dreamference/forge/forge_container.py:56-80) | forge-rev F5 | S | 1.0 |
| forge-oauth-secret-argv | the client secret is on the process list while add-oauth or update-oauth runs (host and container); pass it from a 0600 file or stdin, or document the assumption of no untrusted local users | forge-rev F2, forge "Follow-up rows" | S | 3.0 |
| forge-oauth-service-name | a service key can write clients for any node id with the same first 8 hex digits; compare the full node id (dreamference/accounts/accounts_store.py:527) | forge-rev F3, forge "Follow-up rows" | S | 3.0 |
| forge-oauth-token-audience | tokens for a Forgejo client are accepted by the gate; they should be good only at the userinfo route. Reviewer placed this at 1.0; release 3.0 as security hardening, not a defect | forge-rev F4, forge "Follow-up rows" | M | 3.0 |
| forge-oauth-authorize-names-client | the authorize page should show the client id and the redirect host before Allow (ragon-rs/web/static/login.html, login.js) | forge "Follow-up rows" (B1c) | S | 3.0 |
