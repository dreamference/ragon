# Ragon 0.9.9-rc.2 — release notes

Version 0.9.9-rc.2, the second release candidate of 0.9.9. It is a pre-release for testing. The description between the two rules is the GitHub release text; `scripts/release_notes.py --version 0.9.9-rc.2` prints it. The checklist after the second rule is not published.

---

## Ragon 0.9.9-rc.2 (pre-release)

**This is a pre-release for testing, not for everyday use.** `ragon update` and the installers keep installing 0.9.8.1; "Latest" does not change. To try this candidate you download it by hand from this release's assets. 0.9.9 is planned for Sunday 2026-11-29.

### What changed since 0.9.9-rc.1

- **Secondary nodes use the primary's accounts.** A secondary pinned to a primary signs people in and mints keys through the primary's accounts, not its own empty store. A secondary never mints a key. `ragon-admin node user ...` is refused on a pinned secondary. A revoked key stops working on a secondary at once, not up to two minutes later.
- **Pin only an advertised primary.** `node primary <name or id>` pins only a node that advertises itself as the primary, fetches its certificate authority once, and requires the node's HTTPS answer to chain to it and to name the same node id. A name that several nodes advertise is refused. The success line and `node primary` print the certificate's SHA-256 fingerprint, so you can compare it on both machines.
- **A secondary pinned with an earlier release must run the pin once more.** Run `ragon-admin node primary <primary>` once on each secondary after you update it. Without this the secondary's accounts service keeps its old setting.
- **Clients use the HTTPS address found by discovery and never fall back to plain HTTP.** A node with accounts is used at its HTTPS name, checked against the node's certificate authority. A pinned node is never downgraded, refused or impersonated at discovery, and no password is sent over plain HTTP to another machine. `ragon node use` prints why it kept a node on plain HTTP when it did (for example, when the node's `.local` name does not resolve).
- **The sign-in refusal is explained.** A node that refuses plain sign-in traffic from another machine now says that the node signs in over HTTPS only, and what to check. It no longer reads as "no accounts yet".
- **The node reports its real version.** A node advertises, lists and logs the version of the release it runs, not the 0.9.0 that the 0.9.9 candidates showed.
- **The Linux pre-release package has its published name.** The pre-release `.deb` for arm64 is published with a dot where the package version has a tilde: `ragon_0.9.9.rc.2_arm64.deb`. The package itself keeps its version 0.9.9~rc.2. A full release is unchanged.

### What to test

- Accounts on the primary and on a secondary: sign in on the secondary with a key or a password made on the primary; check that a revoked key stops working at once.
- A secondary you pinned with 0.9.9-rc.1 or earlier: run `ragon-admin node primary <primary>` once, then check that sign-in works.
- A Mac or Windows client on the same network as a node with accounts: check that it signs in at the node's HTTPS name, and that it says so when the node refuses plain HTTP.
- `ragon --version` and the node's advertisement: both print 0.9.9-rc.2.
- Phones pair again after the update, as in 0.9.9-rc.1: pair each bridge again with the `ragon chat pair` command. Earlier pairings are dropped once; the old files are kept as `<name>.bak-<date>` beside them.
- Accounts are on by default. To keep the old behaviour, set `enabled = false` under `[accounts]` in `~/.config/dreamference/config.toml`. Details: `specs/DREAMFERENCE_AUTH.md` and `docs/dev/accounts.md`.

### Known issues

- **Sign-in with the git-hosting account** on the node's authorize page returns an error (HTTP 500). A fix is in preparation and is not in rc.2 unless it is listed above.
- **The two-node test run** on 0.9.9-rc.1 failed 12 of 18 checks. The secondary's key and password sign-in cause is fixed in this candidate. The full run is repeated on rc.2; its results are in the release report, not in these notes.
- **Clients on Mac and Windows** were blocked on rc.1 by the plain-HTTP sign-in refusal and the version 0.9.0. Both are fixed here; the sign-in check on those clients is repeated on rc.2.
- **Device approval** from a second machine returned 403 in the rc.1 run. It is not yet re-tested on rc.2.

### Known limits

The limits of 0.9.8.1 apply unchanged. No benchmark numbers are given for this candidate.

---

**Checklist:** not part of the release text.

- [ ] The assets are built and signed (`specs/DREAMFERENCE_RELEASE_SIGNING.md`).
- [ ] The Linux pre-release package is published under its published name (`ragon_0.9.9.rc.2_arm64.deb`) and listed in SHA256SUMS under that name.
- [ ] The release is flagged as a pre-release.
- [ ] "Latest" still points at 0.9.8.1 and `ragon update` still offers 0.9.8.1.
