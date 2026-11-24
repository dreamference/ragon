# Ragon 0.9.9-rc.1 — release notes

Version 0.9.9-rc.1, the first release candidate of 0.9.9. It is a pre-release for testing. The description between the two rules is the GitHub release text; `scripts/release_notes.py --version 0.9.9-rc.1` prints it. There is no benchmark section: nothing has been measured on this candidate yet.

---

## Ragon 0.9.9-rc.1 (pre-release)

**This is a pre-release for testing, not for everyday use.** `ragon update` and the installers keep installing 0.9.8.1; "Latest" does not change. To try this candidate you download it by hand from this page. The release itself is 0.9.9, planned for Sunday 2026-11-29.

### What to test: accounts are on by default

- A node now starts with accounts on. On its first interactive start, with no account yet, it asks you to make the first account there: in the terminal, or in the desktop window on that node. That account is the admin of the node.
- On another machine, a node with no account yet says in one line that its first account is made on the node itself (run ragon in a terminal there, or `ragon-admin node user add <name>`), and `ragon web` shows the same plain sentence. Until then everything runs without sign-in.
- A launcher on any machine signs in to the node's account; unattended runs (`ragon exec`, Night Shift and the like) never ask.
- To keep the old behaviour, set `enabled = false` under `[accounts]` in `~/.config/dreamference/config.toml`.
- Details: `specs/DREAMFERENCE_AUTH.md` and `docs/dev/accounts.md`.

### What to test: phones pair again

0.9.9 drops the phone pairings of the earlier releases, once. After the update, pair each Telegram and Discord phone again with `ragon chat pair telegram` or `ragon chat pair discord`. Before it drops anything, ragon saves the old files as `<name>.bak-<date>` beside them (`telegram.json`, `discord.json`, `bindings.json`). The drop runs only on a node with accounts on.

### Since 0.9.8.1

- **Accounts:** on by default; the first account is made on the node and makes it the primary; the accounts service runs beside the gate as a user unit and is restarted on every server start.
- **Desktop and `ragon web`:** the desktop window creates the first account, signs in and signs out; its account dialog keeps Tab inside; `ragon web` passes account requests on and shows a refusal as a plain sentence.
- **Launcher:** an interactive start on a node with no account opens at the first account's form; `ragon login status --json` gives the account state to a desktop window.
- **Chat bridges:** the pairing drop above, with backups.
- **Version display:** `--version`, the session header and the status card print the release's own number, including a patch release such as 0.9.8.1.
- **QA:** macOS, Linux and Windows checks accept the published `--version` line of a four-part release.
- **Models:** Minima serves its FP8 KV pool and starts and calls tools.
- **Fixes:** importing `dreamference.hardware` first no longer fails.
- The rest of the changes are tests, code-quality clean-ups and docs.

### Known limits

The limits of 0.9.8.1 apply unchanged. No benchmark numbers are given for this candidate.

---

**Checklist:** not part of the release text.

- [ ] The assets are built and signed (`specs/DREAMFERENCE_RELEASE_SIGNING.md`).
- [ ] The release is flagged as a pre-release.
- [ ] "Latest" still points at 0.9.8.1 and `ragon update` still offers 0.9.8.1.
