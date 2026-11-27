# Ragon 0.9.9-rc.6 — release notes

Version 0.9.9-rc.6, the sixth release candidate of 0.9.9. It is a pre-release for testing. The description between the two rules is the GitHub release text; `scripts/release_notes.py --version 0.9.9-rc.6` prints it. The checklist after the second rule is not published.

---

## Ragon 0.9.9-rc.6 (pre-release)

**This is a pre-release for testing, not for everyday use.** `ragon update` and the installers keep installing 0.9.8.1; "Latest" does not change. To try this candidate you download it by hand from this release's assets. 0.9.9 is planned for Sunday 2026-11-29.

### What changed since 0.9.9-rc.5

- **Two paths to the Codex vendor's servers are closed.** A program that talks to `ragon app-server` directly, not the terminal agent, the web page or the desktop app, could start a realtime call with an existing call id, and `ragon` then sent the signed-in account's token to the vendor's API host. A feedback upload from such a program sent a short error envelope, with no token and no user data, to the vendor's error service. Neither path is used by Ragon's own programs, and both were in every earlier candidate. Every session now runs with feedback uploads off and with the realtime address pointed at the node. A check in a sealed test environment shows no connection leaves the machine on either path.
- **Your own API keys** (new in rc.4) were checked on running nodes with rc.5 and work.
- **The fleet test run** on rc.5 passed 16 of 18 checks on five secondary nodes. The two that did not pass need a person at the screen.

### What to test

- Sign in with `ragon login` and a password, use `ragon` for more than an hour, and check that it keeps working with no new sign-in.
- A new install and an upgrade of a node: open the web page at its HTTPS address after the first model server start.
- A machine signed in with rc.3 or earlier: install this candidate and check that it keeps working after the hour without signing in again.
- On the Account page and with `/apikey`: create a key, use it, revoke it, and check that it stops working (on the model API within a minute).
- Sign-in with the git-hosting account on a node with accounts.
- Accounts on the primary and on a secondary: sign in on the secondary with a key or a password made on the primary.
- A Mac or Windows client on the same network as a node with accounts: check that it signs in at the node's HTTPS name.
- `ragon --version` and the node's advertisement: both print 0.9.9-rc.6.
- Phones pair again after the update, as in 0.9.9-rc.1: pair each bridge again with the `ragon chat pair` command. Earlier pairings are dropped once; the old files are kept as `<name>.bak-<date>` beside them.
- Accounts are on by default. To keep the old behaviour, set `enabled = false` under `[accounts]` in `~/.config/dreamference/config.toml`. Details: `specs/DREAMFERENCE_AUTH.md` and `docs/dev/accounts.md`.

### Known issues

- **Create API keys on the primary.** A secondary node refuses to create a key and says to do it on the primary: use `/apikey new` or the Account page there. Listing and revoking work on a secondary.
- **A primary that becomes a secondary leaves its own accounts behind.** `ragon-admin node primary <primary>` on a node that was a primary joins it to the new primary and says nothing about the accounts it held. They are kept on disk and no longer served. Move them first with `ragon-admin node user export` and `import`.
- **A revoked key** stops at once on the accounts pages and within a minute on the model API.
- **Two error lines per `ragon exec`** about a tool server that answers "not found" appear in the log on clients. The command works; the lines are noise and were there before this candidate.
- **On one older Mac the first `ragon login` attempt failed and the second worked.** The cause is not known yet.
- **The desktop app's sign-in form and its About window** need a person at the screen and are not yet checked.
- **A secondary whose primary is unreachable** refuses a key it has not seen before. This is intended.

### Known limits

The limits of 0.9.8.1 apply unchanged. No benchmark numbers are given for this candidate.

---

**Checklist:** not part of the release text.

- [ ] The assets are built and signed (`specs/DREAMFERENCE_RELEASE_SIGNING.md`).
- [ ] The Linux pre-release package is published under its published name (`ragon_0.9.9.rc.6_arm64.deb`) and listed in SHA256SUMS under that name.
- [ ] The release is flagged as a pre-release.
- [ ] "Latest" still points at 0.9.8.1 and `ragon update` still offers 0.9.8.1.
