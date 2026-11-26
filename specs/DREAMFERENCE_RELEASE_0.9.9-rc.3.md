# Ragon 0.9.9-rc.3 — release notes

Version 0.9.9-rc.3, the third release candidate of 0.9.9. It is a pre-release for testing. The description between the two rules is the GitHub release text; `scripts/release_notes.py --version 0.9.9-rc.3` prints it. The checklist after the second rule is not published.

---

## Ragon 0.9.9-rc.3 (pre-release)

**This is a pre-release for testing, not for everyday use.** `ragon update` and the installers keep installing 0.9.8.1; "Latest" does not change. To try this candidate you download it by hand from this release's assets. 0.9.9 is planned for Sunday 2026-11-29.

### What changed since 0.9.9-rc.2

- **Sign-in with the git-hosting account: the cause is fixed.** The node's Forgejo now trusts the node's certificate authority before the Ragon sign-in source is added to it. In rc.2 the source could not be added, and the node's authorize page returned an error (HTTP 500). The fix is in this candidate; its check on a running node is repeated on rc.3 and is not yet done.
- **No warning about an unknown setting at every start.** `ragon` no longer writes the setting that the agent reported as unrecognised on every start. The sign-in address is now read from the backend address the launcher already writes. An override through `RAGON_ISSUER` still works.
- **The two-node test script is corrected.** It asks `ragon login status` for the signed-in user, accepts the sign-in session cookie, revokes a fresh key on the primary, gives the model enough output tokens, and every node command has a time limit. This changes the test, not the product.

The other changes since rc.2 are tests and code-style fixes with no change in behaviour.

### What to test

- Sign-in with the git-hosting account on a node with accounts: the authorize page opens and the sign-in completes.
- Sign in and let the session refresh on a client with no `RAGON_ISSUER` set: both work, and `ragon` starts with no warning about an unknown setting.
- Accounts on the primary and on a secondary: sign in on the secondary with a key or a password made on the primary; check that a revoked key stops working at once.
- A secondary you pinned with 0.9.9-rc.1 or earlier: run `ragon-admin node primary <primary>` once, then check that sign-in works.
- A Mac or Windows client on the same network as a node with accounts: check that it signs in at the node's HTTPS name.
- `ragon --version` and the node's advertisement: both print 0.9.9-rc.3.
- Phones pair again after the update, as in 0.9.9-rc.1: pair each bridge again with the `ragon chat pair` command. Earlier pairings are dropped once; the old files are kept as `<name>.bak-<date>` beside them.
- Accounts are on by default. To keep the old behaviour, set `enabled = false` under `[accounts]` in `~/.config/dreamference/config.toml`. Details: `specs/DREAMFERENCE_AUTH.md` and `docs/dev/accounts.md`.

### Known issues

- **Sign-in with the git-hosting account** is fixed in the code but not yet confirmed on a running node (see above).
- **Two error lines per `ragon exec`** about a tool server answering 404 appear in the log on clients. The command works; the lines are noise and were there before this candidate.
- **The two-node test run** on rc.2 passed 13 of 18 checks. Of the five that failed, two need a person, two were errors of the test script (corrected here) and one was the git-hosting sign-in above. The run is repeated on rc.3.
- **The desktop app's sign-in form and its About window** were not checked on rc.2; they need a person at the screen.
- **A secondary whose primary is unreachable** refuses a key it has not seen before (HTTP 401). This is intended.

### Known limits

The limits of 0.9.8.1 apply unchanged. No benchmark numbers are given for this candidate.

---

**Checklist:** not part of the release text.

- [ ] The assets are built and signed (`specs/DREAMFERENCE_RELEASE_SIGNING.md`).
- [ ] The Linux pre-release package is published under its published name (`ragon_0.9.9.rc.3_arm64.deb`) and listed in SHA256SUMS under that name.
- [ ] The release is flagged as a pre-release.
- [ ] "Latest" still points at 0.9.8.1 and `ragon update` still offers 0.9.8.1.
