# Ragon 0.9.9-rc.4 — release notes

Version 0.9.9-rc.4, the fourth release candidate of 0.9.9. It is a pre-release for testing. The description between the two rules is the GitHub release text; `scripts/release_notes.py --version 0.9.9-rc.4` prints it. The checklist after the second rule is not published.

---

## Ragon 0.9.9-rc.4 (pre-release)

**This is a pre-release for testing, not for everyday use.** `ragon update` and the installers keep installing 0.9.8.1; "Latest" does not change. To try this candidate you download it by hand from this release's assets. 0.9.9 is planned for Sunday 2026-11-29.

### What changed since 0.9.9-rc.3

- **A sign-in from the command line no longer stops working after an hour.** `ragon login` with a password stored the sign-in without its account id, so the hourly renewal was refused: commands failed with "Please sign in again" while `ragon login status` still said signed in. This was in every earlier candidate. The sign-in now stores the account id, and a sign-in made with an earlier candidate is repaired at the next start, so you do not have to sign in again.
- **`ragon login status` tells the truth.** It says "Session expired" when the session can no longer be renewed. On a secondary node whose primary cannot be reached it keeps saying signed in, with the note that the authentication server is unreachable.
- **You can manage your own API keys.** The Account page and the `/apikey` command can now list, create and revoke the signed-in user's own keys. In rc.3 these did nothing, because the node did not serve the addresses they call. A new key is shown once. A user sees and revokes only their own keys.
- **The Account page shows an error** when the key list cannot be loaded, where it used to show an empty table.
- **Sign-in with the git-hosting account** was checked on a running node with rc.3 and works.

The other changes since rc.3 are corrections to the two-node test script, with no change in behaviour.

### What to test

- Sign in with `ragon login` and a password, use `ragon` for more than an hour, and check that it keeps working with no new sign-in.
- A machine signed in with rc.3 or earlier: install this candidate and check that it keeps working after the hour without signing in again.
- On the Account page and with `/apikey`: create a key, use it, revoke it, and check that it stops working (on the model API within a minute).
- Sign-in with the git-hosting account on a node with accounts.
- Accounts on the primary and on a secondary: sign in on the secondary with a key or a password made on the primary.
- A Mac or Windows client on the same network as a node with accounts: check that it signs in at the node's HTTPS name.
- `ragon --version` and the node's advertisement: both print 0.9.9-rc.4.
- Phones pair again after the update, as in 0.9.9-rc.1: pair each bridge again with the `ragon chat pair` command. Earlier pairings are dropped once; the old files are kept as `<name>.bak-<date>` beside them.
- Accounts are on by default. To keep the old behaviour, set `enabled = false` under `[accounts]` in `~/.config/dreamference/config.toml`. Details: `specs/DREAMFERENCE_AUTH.md` and `docs/dev/accounts.md`.

### Known issues

- **Create API keys on the primary.** A secondary node refuses to create a key and says to do it on the primary: use `/apikey new` or the Account page there. Listing and revoking work on a secondary.
- **A primary that becomes a secondary leaves its own accounts behind.** `ragon-admin node primary <primary>` on a node that was a primary joins it to the new primary and says nothing about the accounts it held. They are kept on disk and no longer served. Move them first with `ragon-admin node user export` and `import`.
- **A revoked key** stops at once on the accounts pages and within a minute on the model API.
- **Two error lines per `ragon exec`** about a tool server that answers "not found" appear in the log on clients. The command works; the lines are noise and were there before this candidate.
- **The two-node test run** on rc.3 passed 15 of 18 checks. Of the three that failed, two need a person and one was the missing key addresses, fixed here. The run is repeated on rc.4.
- **The desktop app's sign-in form and its About window** need a person at the screen and are not yet checked.
- **A secondary whose primary is unreachable** refuses a key it has not seen before. This is intended.

### Known limits

The limits of 0.9.8.1 apply unchanged. No benchmark numbers are given for this candidate.

---

**Checklist:** not part of the release text.

- [ ] The assets are built and signed (`specs/DREAMFERENCE_RELEASE_SIGNING.md`).
- [ ] The Linux pre-release package is published under its published name (`ragon_0.9.9.rc.4_arm64.deb`) and listed in SHA256SUMS under that name.
- [ ] The release is flagged as a pre-release.
- [ ] "Latest" still points at 0.9.8.1 and `ragon update` still offers 0.9.8.1.
